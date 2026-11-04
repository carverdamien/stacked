//! The search, on a screen of its own: on the left, where it looks (all,
//! patch, comment, history) and what it looks for, on the header, then what
//! it finds, as it finds it; on the right, the match picked, in its context.
//! Enter goes there.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Receiver;

use super::*;
use crate::search::{Found, Place, Query, Scope, Sent};

/// The search open.
pub struct Searching {
    pub scope: Scope,
    pub text: String,
    pub found: Vec<Found>,
    pub sel: usize,
    top: usize,
    /// It has looked everywhere it looks.
    pub done: bool,
    generation: u64,
    wanted: Arc<AtomicU64>,
    rx: Option<Receiver<Sent>>,
    /// What was in sight where it was opened, found first: the comments the
    /// comments pane showed, then the stack's others the filter shows; the
    /// patch the middle showed.
    shown: Vec<(String, String)>,
    filtered: Vec<(String, String)>,
    patch: Option<usize>,
    /// The screen it was opened from, to go back to.
    back: Screen,
}

impl App {
    /// Opens the search, looking where the cursor says: from the comments
    /// pane, in the comments; from the middle, in the patches; from the
    /// history, in it; else everywhere.
    pub(super) fn open_search(&mut self) {
        let scope = match (self.screen, self.focus) {
            (Screen::History, _) => Scope::History,
            (Screen::Main, Focus::Notes) => Scope::Comment,
            (Screen::Main, Focus::Diff) => Scope::Patch,
            _ => Scope::All,
        };
        let shown: Vec<(String, String)> = self.shown_notes().into_iter().map(|n| (n.change, n.id)).collect();
        let (busy, held) = (self.busy(), self.held_replies());
        let filtered: Vec<(String, String)> = self
            .db
            .stack_notes(&self.stack)
            .into_iter()
            .filter(|n| shows(self.show, n, &busy, &held, false))
            .map(|n| (n.change.clone(), n.id.clone()))
            .collect();
        self.searching = Some(Searching {
            scope,
            text: String::new(),
            found: Vec::new(),
            sel: 0,
            top: 0,
            done: true,
            generation: 0,
            wanted: Arc::new(AtomicU64::new(0)),
            rx: None,
            shown,
            filtered,
            patch: if self.on_stack { None } else { Some(self.sel) },
            back: self.screen,
        });
        self.screen = Screen::Search;
    }

    /// What is looked for, or where, changed: the search starts again, away
    /// from the screen, what it found before gone.
    fn search_again(&mut self) {
        let (path, stack) = (self.path.clone(), self.stack.clone());
        let Some(s) = self.searching.as_mut() else { return };
        s.generation += 1;
        s.wanted.store(s.generation, Ordering::Relaxed);
        s.found.clear();
        (s.sel, s.top) = (0, 0);
        s.rx = None;
        s.done = s.text.trim().is_empty();
        if s.done {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        s.rx = Some(rx);
        let q = Query {
            scope: s.scope,
            text: s.text.clone(),
            stack,
            patch: s.patch,
            shown: s.shown.clone(),
            filtered: s.filtered.clone(),
        };
        let (generation, wanted) = (s.generation, s.wanted.clone());
        std::thread::spawn(move || crate::search::run(&path, &q, generation, &wanted, &tx));
    }

    /// What the search found since it was last looked at, added to what it
    /// had found.
    pub(super) fn poll_search(&mut self) {
        let Some(s) = self.searching.as_mut() else { return };
        let got: Vec<Sent> = s.rx.as_ref().map(|rx| rx.try_iter().collect()).unwrap_or_default();
        for (g, f) in got {
            if g != s.generation {
                continue;
            }
            match f {
                Some(f) => s.found.push(f),
                None => s.done = true,
            }
        }
    }

    /// The search before that starts as what is typed does, newest first:
    /// what Tab takes.
    pub fn suggestion(&self) -> Option<String> {
        let s = self.searching.as_ref()?;
        let typed = s.text.to_lowercase();
        self.searches
            .iter()
            .find(|p| p.to_lowercase().starts_with(&typed) && p.chars().count() > s.text.chars().count())
            .cloned()
    }

    /// What was looked for kept, newest first, once it found something.
    fn remember_search(&mut self) {
        let Some(s) = &self.searching else { return };
        let t = s.text.trim().to_string();
        if t.is_empty() || s.found.is_empty() {
            return;
        }
        self.searches.retain(|p| *p != t);
        self.searches.insert(0, t);
    }

    /// Back where the search was opened from, the search stopped.
    fn leave_search(&mut self) {
        self.remember_search();
        if let Some(s) = self.searching.take() {
            s.wanted.store(u64::MAX, Ordering::Relaxed);
            self.screen = s.back;
        }
    }

    /// A key in the search: typed, what is looked for; ←→, where; ↑↓, the
    /// match picked; Tab, the search before suggested; Enter goes to the
    /// match; Esc goes back.
    pub(super) fn search_key(&mut self, k: KeyEvent) -> Effect {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let alt = k.modifiers.contains(KeyModifiers::ALT);
        let page = self.page();
        let suggested = self.suggestion();
        let Some(s) = self.searching.as_mut() else {
            self.screen = Screen::Main;
            return Effect::None;
        };
        let last = s.found.len().saturating_sub(1);
        let mut again = false;
        match k.code {
            KeyCode::Esc => {
                self.leave_search();
                return Effect::None;
            }
            KeyCode::Enter => {
                self.go_to_found();
                return Effect::None;
            }
            KeyCode::Tab => {
                if let Some(t) = suggested {
                    s.text = t;
                    again = true;
                }
            }
            KeyCode::Left | KeyCode::Right => {
                s.scope = s.scope.step(k.code == KeyCode::Right);
                again = true;
            }
            KeyCode::Up => s.sel = s.sel.saturating_sub(1),
            KeyCode::Down => s.sel = (s.sel + 1).min(last),
            KeyCode::PageUp => s.sel = s.sel.saturating_sub(page),
            KeyCode::PageDown => s.sel = (s.sel + page).min(last),
            KeyCode::Home => s.sel = 0,
            KeyCode::End => s.sel = last,
            KeyCode::Backspace => {
                again = s.text.pop().is_some();
            }
            KeyCode::Char('u') if ctrl => {
                again = !s.text.is_empty();
                s.text.clear();
            }
            KeyCode::Char(c) if !ctrl && !alt => {
                s.text.push(c);
                again = true;
            }
            _ => {}
        }
        if again {
            self.search_again();
        }
        Effect::None
    }

    /// The scope picked again, as ← → do: for a click on it.
    pub(super) fn step_scope(&mut self) {
        if let Some(s) = self.searching.as_mut() {
            s.scope = s.scope.step(true);
            self.search_again();
        }
    }

    /// The match picked gone to: its comment, its patch at its line, its
    /// record of the history.
    fn go_to_found(&mut self) {
        let Some(f) = self.searching.as_ref().and_then(|s| s.found.get(s.sel)).cloned() else { return };
        self.leave_search();
        match f.place {
            Place::Comment { change, id, .. } => {
                if let Some(name) = self.stack_of(&change)
                    && name != self.stack
                {
                    self.switch_stack(&name);
                }
                self.go_to(&change, &id);
            }
            Place::Message { change, line } => self.go_to_patch_want(&change, Want::Msg(line)),
            Place::Diff { change, at } => self.go_to_patch_want(&change, Want::Diff(at)),
            Place::File { change, path, line } => self.go_to_patch_want(&change, Want::Near(path, line)),
            Place::History { rev } => {
                self.perform(Action::GoToHistory(rev));
            }
        }
    }

    /// The stack comments on `change` are of: the one it is the stack of, or
    /// the one with a patch of it.
    fn stack_of(&self, change: &str) -> Option<String> {
        self.db
            .stacks()
            .iter()
            .find(|s| stack_change(&s.name) == change || s.patches.iter().any(|p| self.db.patch(p).change == change))
            .map(|s| s.name.clone())
    }

    /// The patch of `change`, in whichever stack has it.
    fn patch_of(&self, change: &str) -> Option<&Patch> {
        self.db
            .stacks()
            .iter()
            .flat_map(|s| s.patches.iter())
            .map(|p| self.db.patch(p))
            .find(|p| p.change == change)
    }

    /// The search: on the left, its header (where it looks, what is
    /// typed), what it found, what it came to; on the right, the match
    /// picked in its context. Where the cursor is: after what is typed.
    pub(super) fn draw_search(&mut self, buf: &mut Buffer, body: Rect) -> Option<(u16, u16)> {
        let (w, h) = (body.width as usize, body.height as usize);
        if h < 3 {
            return None;
        }
        let suggestion = self.suggestion();
        let list_w = if w < 60 { w } else { (w * 2 / 5).max(30) };
        let list = Rect::new(body.x, body.y, list_w as u16, body.height);
        // What it found, under its header: scrolled by the wheel, moved by
        // the lines it turned, as far as the last on the bottom line, and
        // since, until a key, where the wheel left it, the match picked in
        // sight or not.
        self.scrolls_on(list, (0, 1, list_w, h - 2), Wheeled::Found);
        let (turned, free) = (self.turned(Wheeled::Found), self.free(Wheeled::Found));
        let s = self.searching.as_mut()?;
        let button = format!(" ‹ {} › ", s.scope.name());
        let bx = button.chars().count().min(list_w);
        put(buf, list, 0, 0, bx, &button, color::header().add_modifier(Modifier::REVERSED));
        spot_in(&self.spots, list, (0, 0, bx, 1), Spot::SearchScope);
        let typed = format!(" {}", s.text);
        let cx = (bx + typed.chars().count()).min(list_w.saturating_sub(1));
        put(buf, list, bx, 0, list_w - bx, &fit(&typed, list_w - bx), Style::new());
        if let Some(sug) = &suggestion {
            let rest: String = sug.chars().skip(s.text.chars().count()).collect();
            put(buf, list, cx, 0, list_w.saturating_sub(cx), &rest, color::dim());
        }
        let rows = h - 2;
        if let Some(n) = turned {
            s.top = moved(s.top, n, s.found.len().saturating_sub(rows));
        }
        if !free {
            if s.sel < s.top {
                s.top = s.sel;
            } else if s.sel >= s.top + rows {
                s.top = s.sel + 1 - rows;
            }
        }
        // More found than the list has room for: its scroll bar beside them,
        // in the line on its right, else, the list alone on the screen, in
        // its last column.
        let shown = Shown { top: s.top, rows, all: s.found.len() };
        let tw = self.pane_bar(Wheeled::Found, list, buf.area, (1, rows), shown);
        let s = self.searching.as_ref()?;
        for (y, (i, f)) in s.found.iter().enumerate().skip(s.top).take(rows).enumerate() {
            let st = if i == s.sel { on_cursor(Style::new()) } else { Style::new() };
            put(buf, list, 0, y + 1, tw, &fit(&format!(" {} · {}", f.at, f.line), tw), st);
            spot_in(&self.spots, list, (0, y + 1, list_w, 1), Spot::Found(i));
        }
        let n = s.found.len();
        let said = match (s.text.trim().is_empty(), n, s.done) {
            (true, _, _) => " type what to look for".to_string(),
            (false, 0, true) => " nothing found".to_string(),
            (false, n, true) if n >= crate::search::MOST => format!(" the first {n} found"),
            (false, n, true) => format!(" {n} found"),
            (false, n, false) => format!(" {n} found · looking…"),
        };
        put(buf, list, 0, h - 1, list_w, &fit(&said, list_w), color::dim());
        if w >= 60 {
            for y in 0..body.height {
                buf.set_string(body.x + list_w as u16, body.y + y, "│", color::meta());
            }
            let main = Rect::new(body.x + (list_w + 1) as u16, body.y, (w - list_w - 1) as u16, body.height);
            self.draw_found(buf, main);
        }
        Some((body.x + cx as u16, body.y))
    }

    /// The match picked, in its context: its rows in its patch (or the
    /// stack), the lines around it in its file, its record of the history;
    /// what was looked for lit.
    fn draw_found(&self, buf: &mut Buffer, area: Rect) {
        let Some(s) = &self.searching else { return };
        let Some(f) = s.found.get(s.sel) else { return };
        let (w, h) = (area.width as usize, area.height as usize);
        put(buf, area, 0, 0, w, &fit(&format!(" {}", f.at), w), color::header());
        let mut lines: Vec<Vec<(String, Style)>> = Vec::new();
        let mut at = 0;
        match &f.place {
            Place::File { line, .. } => {
                if let Some((first, text)) = &f.context {
                    for (k, l) in text.iter().enumerate() {
                        let no = first + k;
                        if no == *line {
                            at = k;
                        }
                        lines.push(vec![(format!("{no:>5} "), color::meta()), (l.clone(), Style::new())]);
                    }
                }
            }
            Place::History { rev } => {
                for (k, l) in crate::history::about(&self.db, &self.stack, *rev).into_iter().enumerate() {
                    if at == 0 && l.trim() == f.line {
                        at = k;
                    }
                    lines.push(vec![(l, Style::new())]);
                }
            }
            Place::Comment { change, id, turn } => {
                let Some(n) = self.db.note(change, id) else { return };
                let deco = Deco {
                    pointed: self.pointed_of([n]),
                    ..Default::default()
                };
                let rows = if n.on_stack() {
                    let name = self.stack_of(change).unwrap_or_default();
                    let titles: Vec<String> = self
                        .db
                        .stack(&name)
                        .map(|s| s.patches.iter().map(|id| self.db.patch(id).title().to_string()).collect())
                        .unwrap_or_default();
                    stack_rows(&format!("the stack {name}"), &titles, &[n], w, &deco)
                } else {
                    match self.patch_of(change) {
                        Some(p) => patch_rows(p, &[n], w, &deco),
                        None => return,
                    }
                };
                at = rows
                    .iter()
                    .position(|r| r.note.as_deref() == Some(id) && r.turn == Some(*turn))
                    .or_else(|| rows.iter().position(|r| r.note.as_deref() == Some(id)))
                    .unwrap_or(0);
                lines = rows.iter().map(|r| row_parts(r, false)).collect();
            }
            Place::Message { change, line } | Place::Diff { change, at: line } => {
                let Some(p) = self.patch_of(change) else { return };
                let rows = patch_rows(p, &[], w, &Deco::default());
                let message = matches!(f.place, Place::Message { .. });
                at = rows
                    .iter()
                    .position(|r| if message { r.msg_at == Some(*line) } else { r.diff_at == Some(*line) })
                    .unwrap_or(0);
                lines = rows.iter().map(|r| row_parts(r, false)).collect();
            }
        }
        let room = h - 1;
        let top = at.saturating_sub(room / 3);
        let q = s.text.trim().to_lowercase();
        for (y, parts) in lines.iter().skip(top).take(room).enumerate() {
            put_parts(buf, area, 0, y + 1, w, parts);
            let text: String = parts.iter().map(|(t, _)| t.as_str()).collect();
            lit(buf, area, y + 1, &text, &q);
        }
    }
}

/// What was looked for, `q`, lit wherever it is in `text`, drawn on row `y`
/// of `area`.
fn lit(buf: &mut Buffer, area: Rect, y: usize, text: &str, q: &str) {
    let qc: Vec<char> = q.chars().collect();
    let tc: Vec<char> = text.chars().flat_map(|c| c.to_lowercase().next()).collect();
    if qc.is_empty() || y >= area.height as usize {
        return;
    }
    let mut i = 0;
    while i + qc.len() <= tc.len() {
        if tc[i..i + qc.len()] == qc[..] {
            for x in i..(i + qc.len()).min(area.width as usize) {
                buf[(area.x + x as u16, area.y + y as u16)].modifier.insert(Modifier::REVERSED);
            }
            i += qc.len();
        } else {
            i += 1;
        }
    }
}
