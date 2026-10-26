//! `stacked setup agent --tui`: the tools the agent has, on a screen of
//! their own, a box each to let every ask use it or not; then what is let is
//! written in the settings, as agent.tools: the checkout's own, or with
//! `--global` the settings' file.

use std::collections::HashSet;

use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::ask::{CHANGING, GIVEN};
use crate::db::{Error, Result};
use crate::paint::{color, fit, on_cursor, put, window};

/// A row of the screen.
#[derive(Debug, Clone, PartialEq)]
enum Row {
    Heading(String),
    /// An MCP server: all its tools let, now and to come, or not.
    Server { name: String, all: bool },
    /// A tool: let or not; of a server, or not; whether it changes things.
    Tool {
        name: String,
        server: Option<String>,
        on: bool,
        changing: bool,
    },
}

/// The screen: its rows, the one picked, and the settings' tools as they
/// are now; the file they are in, if one sets them, and the one they are
/// written in.
pub struct Setup {
    rows: Vec<Row>,
    pub sel: usize,
    top: usize,
    was: Vec<String>,
    from: Option<String>,
    to: String,
    /// Esc's menu, open, its item picked: as the comment editor's, once
    /// something changed.
    pub menu: Option<usize>,
}

/// What a key comes to.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    Stay,
    /// Saved: the settings' tools, as they are to be.
    Save(Vec<String>),
    /// Left, nothing written.
    Leave,
}

/// What an item of Esc's menu does: what is chosen saved, or thrown away,
/// or the boxes picked on; a line between them.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Choice {
    Save,
    Throw,
    Keep,
    Line,
}

/// Esc's menu, as the comment editor's: saving first, throwing away under
/// a line, then going on.
const MENU: [(&str, Choice); 4] = [("Save it", Choice::Save), ("", Choice::Line), ("Throw it away", Choice::Throw), ("Keep choosing", Choice::Keep)];

/// The MCP server tool `t` is of, if it is one's (`mcp__NAME__TOOL`).
fn server_of(t: &str) -> Option<String> {
    t.strip_prefix("mcp__").and_then(|r| r.split_once("__")).map(|(s, _)| s.to_string())
}

impl Setup {
    /// The screen for the agent's `tools` and MCP `servers`, each box as
    /// the settings' tools `was` say, the file `from` setting them (if one
    /// does), to be written in the file `to`. What the settings let that
    /// the agent does not say it has (a command, a server not there now)
    /// has a row of its own, to keep or take out.
    pub fn new(tools: &[String], servers: &[String], was: &[String], from: Option<&str>, to: &str) -> Setup {
        let mut names: Vec<String> = servers.to_vec();
        for t in tools {
            if let Some(s) = server_of(t)
                && !names.contains(&s)
            {
                names.push(s);
            }
        }
        let mut rows = Vec::new();
        for s in &names {
            let all = was.contains(&format!("mcp__{s}__*"));
            rows.push(Row::Server { name: s.clone(), all });
            for t in tools.iter().filter(|t| server_of(t).as_deref() == Some(s.as_str())) {
                rows.push(Row::Tool {
                    name: t.clone(),
                    server: Some(s.clone()),
                    on: all || was.contains(t),
                    changing: false,
                });
            }
        }
        let others: Vec<&String> = tools.iter().filter(|t| server_of(t).is_none() && !GIVEN.contains(&t.as_str())).collect();
        if !others.is_empty() {
            rows.push(Row::Heading("Other tools".into()));
            for t in others {
                rows.push(Row::Tool {
                    name: t.clone(),
                    server: None,
                    on: was.contains(t),
                    changing: CHANGING.contains(&t.as_str()),
                });
            }
        }
        let known = |t: &str| {
            rows.iter().any(|r| match r {
                Row::Server { name, .. } => t == format!("mcp__{name}__*"),
                Row::Tool { name, .. } => name == t,
                Row::Heading(_) => false,
            })
        };
        let kept: Vec<String> = was.iter().filter(|t| !known(t)).cloned().collect();
        if !kept.is_empty() {
            rows.push(Row::Heading("In the settings, not among the agent's tools".into()));
            for t in kept {
                rows.push(Row::Tool {
                    changing: CHANGING.contains(&t.as_str()),
                    name: t,
                    server: None,
                    on: true,
                });
            }
        }
        let sel = rows.iter().position(|r| !matches!(r, Row::Heading(_))).unwrap_or(0);
        Setup {
            rows,
            sel,
            top: 0,
            was: was.to_vec(),
            from: from.map(String::from),
            to: to.to_string(),
            menu: None,
        }
    }

    /// Whether what is chosen is not what the settings say now.
    fn changed(&self) -> bool {
        self.chosen() != self.was
    }

    /// Whether all of `server`'s tools are let.
    fn all(&self, server: &str) -> bool {
        self.rows.iter().any(|r| matches!(r, Row::Server { name, all: true } if name == server))
    }

    /// What the settings' tools are to be: each server let whole, as
    /// `mcp__NAME__*`, and each tool let.
    pub fn chosen(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut seen: HashSet<String> = HashSet::new();
        for r in &self.rows {
            let t = match r {
                Row::Server { name, all: true } => format!("mcp__{name}__*"),
                Row::Tool { name, server, on: true, .. } if !server.as_deref().is_some_and(|s| self.all(s)) => name.clone(),
                _ => continue,
            };
            if seen.insert(t.clone()) {
                out.push(t);
            }
        }
        out
    }

    /// The row picked let, or not. A server: all its tools, or none. One
    /// tool of a server let whole: the server no longer is, its other tools
    /// still are.
    fn toggle(&mut self) {
        match self.rows.get(self.sel).cloned() {
            Some(Row::Server { name, all }) => {
                for r in &mut self.rows {
                    match r {
                        Row::Server { name: n, all: a } if *n == name => *a = !all,
                        Row::Tool { server: Some(s), on, .. } if *s == name => *on = !all,
                        _ => {}
                    }
                }
            }
            Some(Row::Tool { server, on, .. }) => {
                if let Some(s) = server {
                    for r in &mut self.rows {
                        if let Row::Server { name, all } = r
                            && *name == s
                        {
                            *all = false;
                        }
                    }
                }
                if let Some(Row::Tool { on: o, .. }) = self.rows.get_mut(self.sel) {
                    *o = !on;
                }
            }
            _ => {}
        }
    }

    /// The next row that is not a heading, down or up.
    fn step(&mut self, down: bool, by: usize) {
        for _ in 0..by {
            let mut k = self.sel;
            loop {
                let next = if down { k + 1 } else { k.wrapping_sub(1) };
                match self.rows.get(next) {
                    Some(Row::Heading(_)) => k = next,
                    Some(_) => {
                        self.sel = next;
                        break;
                    }
                    None => break,
                }
            }
        }
    }

    /// A key. Esc (or q), as in the comment editor: with something changed,
    /// a menu, to save it, throw it away or keep choosing; with nothing,
    /// out. Ctrl-c is out, whatever is open.
    pub fn key(&mut self, k: KeyEvent) -> Step {
        if k.code == KeyCode::Char('c') && k.modifiers.contains(KeyModifiers::CONTROL) {
            return Step::Leave;
        }
        if let Some(at) = self.menu {
            // ↑↓ past the line between its items.
            let picks: Vec<usize> = (0..MENU.len()).filter(|&k| MENU[k].1 != Choice::Line).collect();
            let here = picks.iter().position(|&k| k == at).unwrap_or(0);
            match k.code {
                KeyCode::Up => self.menu = Some(picks[here.saturating_sub(1)]),
                KeyCode::Down => self.menu = Some(picks[(here + 1).min(picks.len() - 1)]),
                KeyCode::Esc => self.menu = None,
                KeyCode::Enter => match MENU[at].1 {
                    Choice::Save => return Step::Save(self.chosen()),
                    Choice::Throw => return Step::Leave,
                    Choice::Keep => self.menu = None,
                    Choice::Line => {}
                },
                _ => {}
            }
            return Step::Stay;
        }
        match k.code {
            KeyCode::Up => self.step(false, 1),
            KeyCode::Down => self.step(true, 1),
            KeyCode::PageUp => self.step(false, 10),
            KeyCode::PageDown => self.step(true, 10),
            KeyCode::Home => self.step(false, self.rows.len()),
            KeyCode::End => self.step(true, self.rows.len()),
            KeyCode::Char(' ') | KeyCode::Enter => self.toggle(),
            KeyCode::Char('s') => return Step::Save(self.chosen()),
            KeyCode::Esc | KeyCode::Char('q') if self.changed() => self.menu = Some(0),
            KeyCode::Esc | KeyCode::Char('q') => return Step::Leave,
            _ => {}
        }
        Step::Stay
    }

    /// The screen: what it is for, where the agent was asked (`said`), the
    /// settings' tools now; a row each; what they are to be, and what the
    /// keys do.
    pub fn draw(&mut self, buf: &mut Buffer, said: &str) {
        let area = buf.area;
        let (w, h) = (area.width as usize, area.height as usize);
        if h < 5 {
            return;
        }
        let list = |v: &[String]| if v.is_empty() { "none".to_string() } else { v.join(", ") };
        let head = format!(" The tools every ask may use, besides {} (stacked's own): {said}", GIVEN.join(", "));
        put(buf, area, 0, 0, w, &fit(&head, w), color::header());
        let now = match &self.from {
            Some(f) => format!(" Now, agent.tools in {f}: {}", list(&self.was)),
            None => format!(" Now, agent.tools: {}", list(&self.was)),
        };
        put(buf, area, 0, 1, w, &fit(&now, w), color::dim());
        let next = self.chosen();
        let (to_be, st) = if next == self.was {
            (format!(" To be, in {}: as it is", self.to), color::dim())
        } else {
            (format!(" To be, in {}: {}", self.to, list(&next)), color::note())
        };
        put(buf, area, 0, h - 2, w, &fit(&to_be, w), st);
        let room = h - 4;
        if self.sel < self.top {
            self.top = self.sel;
        } else if self.sel >= self.top + room {
            self.top = self.sel + 1 - room;
        }
        if self.rows.is_empty() {
            put(buf, area, 1, 2, w, "The agent has no tools but stacked's own.", color::dim());
        }
        for (y, (i, r)) in self.rows.iter().enumerate().skip(self.top).take(room).enumerate() {
            let mark = |on: bool| if on { "[x]" } else { "[ ]" };
            let (text, mut st) = match r {
                Row::Heading(t) => (format!(" {t}"), color::file()),
                Row::Server { name, all } => (
                    format!(" {} all of MCP server {name}'s tools, those to come too (mcp__{name}__*)", mark(*all)),
                    color::file(),
                ),
                Row::Tool { name, server: Some(s), on, .. } => {
                    let short = name.strip_prefix(&format!("mcp__{s}__")).unwrap_or(name);
                    let st = if self.all(s) { color::dim() } else { ratatui::style::Style::new() };
                    (format!("     {} {short}", mark(*on)), st)
                }
                Row::Tool { name, on, changing, .. } => {
                    let warn = if *changing { "   it changes things: an answer could too" } else { "" };
                    let st = if *changing { color::note() } else { ratatui::style::Style::new() };
                    (format!("   {} {name}{warn}", mark(*on)), st)
                }
            };
            if i == self.sel {
                st = on_cursor(st);
            }
            put(buf, area, 0, y + 2, w, &fit(&text, w), st);
        }
        // What the keys do, as the comment editor says it.
        let keys = match (self.menu, self.changed()) {
            (Some(_), _) => " ↑↓ choose · enter: do it · esc: keep choosing",
            (None, true) => " ↑↓: pick · space: let it or not · s: save · esc: save or throw away",
            (None, false) => " ↑↓: pick · space: let it or not · s: save · esc: close",
        };
        put(buf, area, 0, h - 1, w, &fit(keys, w), color::status());
        if let Some(at) = self.menu {
            self.draw_menu(buf, at);
        }
    }

    /// Esc's menu, in a box over the screen, item `at` picked.
    fn draw_menu(&self, buf: &mut Buffer, at: usize) {
        let area = buf.area;
        let (w, h) = (area.width as usize, area.height as usize);
        let title = format!("agent.tools, in {}", self.to);
        let widest = MENU.iter().map(|(l, _)| l.chars().count()).max().unwrap_or(0);
        let bw = (widest.max(title.chars().count() + 2).max(24) + 4).min(w.saturating_sub(2));
        let bh = MENU.len() + 2;
        let (x, y) = (w.saturating_sub(bw) / 2, h.saturating_sub(bh) / 2);
        window(buf, area, x, y, bw, bh, &title, color::meta());
        for (r, (label, what)) in MENU.iter().enumerate() {
            if *what == Choice::Line {
                put(buf, area, x + 1, y + 1 + r, bw - 2, &"─".repeat(bw - 2), color::meta());
                continue;
            }
            let st = if r == at { on_cursor(ratatui::style::Style::new()) } else { ratatui::style::Style::new() };
            put(buf, area, x + 1, y + 1 + r, bw - 2, &fit(&format!(" {label}"), bw - 2), st);
        }
    }
}

/// The screen, run on the terminal until it is saved or left: what to
/// write, if it is saved.
pub fn run(mut s: Setup, said: &str) -> Result<Option<Vec<String>>> {
    let io = |e: std::io::Error| Error(e.to_string());
    let mut terminal = ratatui::init();
    let out = (|| loop {
        terminal.draw(|f| s.draw(f.buffer_mut(), said)).map_err(io)?;
        let Event::Key(k) = event::read().map_err(io)? else {
            continue;
        };
        if k.kind != KeyEventKind::Press {
            continue;
        }
        match s.key(k) {
            Step::Stay => {}
            Step::Save(v) => return Ok(Some(v)),
            Step::Leave => return Ok(None),
        }
    })();
    ratatui::restore();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Rect;

    fn press(s: &mut Setup, code: KeyCode) -> Step {
        s.key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn screen(s: &mut Setup) -> String {
        let mut buf = Buffer::empty(Rect::new(0, 0, 100, 18));
        s.draw(&mut buf, "in repo");
        (0..buf.area.height)
            .map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect::<String>().trim_end().to_string())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn strings(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn each_tool_is_let_or_not_and_a_server_s_all_at_once() {
        let tools = strings(&["Read", "Bash", "WebFetch", "mcp__moz__get_bug", "mcp__moz__get_revision", "mcp__notes__write"]);
        let servers = strings(&["moz", "notes"]);
        // As the settings were: notes' write let, and a tool of another
        // agent's, kept as it is.
        let mut s = Setup::new(&tools, &servers, &strings(&["mcp__notes__write", "Bash(jq *)"]), Some("settings.toml"), "local.toml");
        let shown = screen(&mut s);
        assert!(shown.contains(" Now, agent.tools in settings.toml: mcp__notes__write, Bash(jq *)"), "{shown}");
        assert!(shown.contains(" To be, in local.toml: as it is"), "{shown}");
        assert!(shown.contains(" In the settings, not among the agent's tools\n   [x] Bash(jq *)"), "{shown}");
        assert!(shown.contains(" [ ] all of MCP server moz's tools, those to come too (mcp__moz__*)"), "{shown}");
        assert!(shown.contains("     [ ] get_bug") && shown.contains("     [x] write"), "{shown}");
        assert!(shown.contains("   [ ] Bash   it changes things: an answer could too"), "{shown}");
        assert!(!shown.contains("[ ] Read"), "given already: {shown}");
        // All of moz's, at once.
        assert_eq!(press(&mut s, KeyCode::Char(' ')), Step::Stay);
        assert!(screen(&mut s).contains("     [x] get_revision"));
        // One of them taken back: moz is no longer let whole.
        press(&mut s, KeyCode::Down);
        press(&mut s, KeyCode::Char(' '));
        // notes' write no longer; WebFetch, past the heading and Bash.
        press(&mut s, KeyCode::Down);
        press(&mut s, KeyCode::Down);
        press(&mut s, KeyCode::Down);
        press(&mut s, KeyCode::Enter);
        press(&mut s, KeyCode::Down);
        press(&mut s, KeyCode::Down);
        press(&mut s, KeyCode::Char(' '));
        assert!(screen(&mut s).contains(" To be, in local.toml: mcp__moz__get_revision, WebFetch, Bash(jq *)"), "{}", screen(&mut s));
        // Bash(jq *), of the settings, taken out too.
        press(&mut s, KeyCode::End);
        press(&mut s, KeyCode::Char(' '));
        assert_eq!(press(&mut s, KeyCode::Char('s')), Step::Save(strings(&["mcp__moz__get_revision", "WebFetch"])));
    }

    #[test]
    fn esc_is_a_menu_once_something_changed_as_in_the_comment_editor() {
        let tools = strings(&["mcp__moz__get_bug", "WebFetch"]);
        let fresh = || Setup::new(&tools, &[], &[], None, "local.toml");
        // Nothing changed: out, nothing to write.
        let mut s = fresh();
        assert!(screen(&mut s).ends_with("esc: close"), "{}", screen(&mut s));
        assert_eq!(press(&mut s, KeyCode::Esc), Step::Leave);
        // Changed: a menu, on saving it; ↑↓ past the line between its items.
        let mut s = fresh();
        press(&mut s, KeyCode::Char(' '));
        assert!(screen(&mut s).ends_with("esc: save or throw away"), "{}", screen(&mut s));
        assert_eq!(press(&mut s, KeyCode::Esc), Step::Stay);
        let shown = screen(&mut s);
        for item in ["agent.tools, in local.toml", " Save it", " Throw it away", " Keep choosing"] {
            assert!(shown.contains(item), "{item:?} in:\n{shown}");
        }
        assert!(shown.ends_with("↑↓ choose · enter: do it · esc: keep choosing"), "{shown}");
        assert_eq!(s.menu, Some(0));
        press(&mut s, KeyCode::Down);
        assert_eq!(s.menu, Some(2), "past the line");
        // Keep choosing, or Esc: back to the boxes, what was chosen kept.
        press(&mut s, KeyCode::Down);
        assert_eq!(press(&mut s, KeyCode::Enter), Step::Stay);
        assert_eq!((s.menu, s.chosen()), (None, strings(&["mcp__moz__*"])));
        press(&mut s, KeyCode::Char('q'));
        assert_eq!(press(&mut s, KeyCode::Esc), Step::Stay);
        assert_eq!(s.menu, None);
        // Saved from it; thrown away from it, each the screen's end.
        press(&mut s, KeyCode::Esc);
        assert_eq!(press(&mut s, KeyCode::Enter), Step::Save(strings(&["mcp__moz__*"])));
        let changed = || {
            let mut s = fresh();
            press(&mut s, KeyCode::Char(' '));
            press(&mut s, KeyCode::Esc);
            s
        };
        let mut s = changed();
        press(&mut s, KeyCode::Down);
        assert_eq!(press(&mut s, KeyCode::Enter), Step::Leave);
        // Ctrl-c, out whatever is open.
        let mut s = changed();
        assert_eq!(s.key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)), Step::Leave);
    }

    #[test]
    fn a_server_let_whole_is_said_so() {
        let tools = strings(&["mcp__moz__get_bug"]);
        let mut s = Setup::new(&tools, &[], &strings(&["mcp__moz__*"]), Some("settings.toml"), "settings.toml");
        assert!(screen(&mut s).contains(" [x] all of MCP server moz's tools"));
        assert_eq!(s.chosen(), ["mcp__moz__*"]);
        // No file sets them: none.
        let mut s = Setup::new(&tools, &[], &[], None, "local.toml");
        assert!(screen(&mut s).contains(" Now, agent.tools: none\n"), "{}", screen(&mut s));
    }
}
