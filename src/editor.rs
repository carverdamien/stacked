//! The comment editor, inline where the comment will be: in the patch under
//! its line, in the comments pane under the comment it answers, in the
//! patches pane under the stack's header or a patch. After self-review's.
//! It edits text; what is done with the text is the app's, which the editor
//! asks for with an `Op`.

use std::cell::{Cell, RefCell};

use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;

use crate::paint::{color, put, window};
use crate::thread::{LineRole, thread_line_roles};

/// What the editor wants done.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    Stay,
    /// Esc with nothing changed: the app closes the editor, or asks what to
    /// do with what it holds if that may be sent as it is.
    Close,
    /// Esc with the text changed: the app asks what to do with it.
    Done(String),
}

thread_local! {
    /// What ctrl-k cut last, which ctrl-y puts back: one for every comment
    /// written, as a shell's.
    static CUT: RefCell<String> = const { RefCell::new(String::new()) };
}

/// Where alt-b (ctrl-←) goes from (row, col): the start of the word before,
/// words being what is between blanks; from a line's start, the end of the
/// line before. As self-review's.
fn word_left(lines: &[Vec<char>], row: usize, col: usize) -> (usize, usize) {
    if col == 0 {
        return if row > 0 { (row - 1, lines[row - 1].len()) } else { (row, 0) };
    }
    let ln = &lines[row];
    let mut i = col;
    while i > 0 && ln[i - 1].is_whitespace() {
        i -= 1;
    }
    while i > 0 && !ln[i - 1].is_whitespace() {
        i -= 1;
    }
    (row, i)
}

/// Where alt-f (ctrl-→) goes from (row, col): the end of the word after;
/// from a line's end, the start of the line after.
fn word_right(lines: &[Vec<char>], row: usize, col: usize) -> (usize, usize) {
    let ln = &lines[row];
    if col >= ln.len() {
        return if row + 1 < lines.len() { (row + 1, 0) } else { (row, ln.len()) };
    }
    let mut i = col;
    while i < ln.len() && ln[i].is_whitespace() {
        i += 1;
    }
    while i < ln.len() && !ln[i].is_whitespace() {
        i += 1;
    }
    (row, i)
}

#[derive(Debug, Clone, PartialEq)]
pub struct Editor {
    lines: Vec<Vec<char>>,
    row: usize,
    col: usize,
    vtop: Cell<usize>,
    initial: String,
    /// The last key was ctrl-k: the next one adds to what it cut.
    cutting: bool,
    /// What shows, dimmed, while nothing is written yet, to read (↑↓
    /// scroll it): the comment last closed where this one goes. Gone once
    /// you write.
    ghost: Vec<String>,
    /// How far down it is scrolled, and how many lines the box last held.
    ghost_top: Cell<usize>,
    ghost_page: Cell<usize>,
    /// For a comment: the numbers of the references attached to it. A
    /// mention of one is drawn as a link, of none as broken. None: a
    /// mention is text as any.
    attached: Option<Vec<usize>>,
    /// The references attached, said, a line each under the text.
    under: Vec<String>,
    /// A line to write, not a comment: Enter is its caller's.
    single: bool,
}

impl Editor {
    pub fn new(text: &str) -> Editor {
        let lines: Vec<Vec<char>> = text.split('\n').map(|l| l.chars().collect()).collect();
        let row = lines.len() - 1;
        let col = lines[row].len();
        Editor {
            lines,
            row,
            col,
            vtop: Cell::new(0),
            initial: text.into(),
            cutting: false,
            ghost: Vec::new(),
            ghost_top: Cell::new(0),
            ghost_page: Cell::new(1),
            attached: None,
            under: Vec::new(),
            single: false,
        }
    }

    /// A line to write, not a comment: its caller takes Enter, and the
    /// keys at its foot say only `esc`'s.
    pub fn single(mut self) -> Editor {
        self.single = true;
        self
    }

    /// The cursor at column `col` of its line.
    pub fn set_col(&mut self, col: usize) {
        self.col = col.min(self.lines[self.row].len());
    }

    /// For a comment: the references attached to it, by their numbers,
    /// which mentions name, and as their lines under the text say them.
    pub fn set_attached(&mut self, numbers: Vec<usize>, lines: Vec<String>) {
        self.attached = Some(numbers);
        self.under = lines;
    }

    /// The mentions of what is written that name no reference attached, as
    /// written.
    pub fn broken(&self) -> Vec<String> {
        let Some(ns) = &self.attached else { return vec![] };
        let text = self.text();
        crate::refs::broken(&text, ns).into_iter().map(str::to_string).collect()
    }

    /// With `lines` shown dimmed until you write: what was closed here.
    pub fn with_ghost(mut self, lines: Vec<String>) -> Editor {
        self.ghost = lines;
        self
    }

    /// Whether what was closed here shows: nothing is written yet.
    pub fn ghosted(&self) -> bool {
        !self.ghost.is_empty()
    }

    /// The cursor at the start of the text: to write above what is there.
    pub fn to_start(&mut self) {
        (self.row, self.col) = (0, 0);
    }

    /// The text, each line's trailing blanks dropped, and blank lines at
    /// either end.
    pub fn text(&self) -> String {
        let text = self
            .lines
            .iter()
            .map(|l| l.iter().collect::<String>().trim_end().to_string())
            .collect::<Vec<_>>()
            .join("\n");
        text.trim_matches('\n').to_string()
    }

    pub fn modified(&self) -> bool {
        self.text() != self.initial.trim_matches('\n')
    }

    pub fn key(&mut self, k: KeyEvent) -> Op {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let alt = k.modifiers.contains(KeyModifiers::ALT);
        let code = match k.code {
            KeyCode::Char(c) if ctrl || alt => KeyCode::Char(c.to_ascii_lowercase()),
            c => c,
        };
        // While what was closed here shows, ↑↓ and the page keys scroll it.
        if self.ghosted() {
            let (top, page) = (self.ghost_top.get(), self.ghost_page.get().max(1));
            let to = match code {
                KeyCode::Up => Some(top.saturating_sub(1)),
                KeyCode::Down => Some(top + 1),
                KeyCode::PageUp => Some(top.saturating_sub(page)),
                KeyCode::PageDown => Some(top + page),
                _ => None,
            };
            if let Some(to) = to {
                self.ghost_top.set(to);
                return Op::Stay;
            }
        }
        // ctrl-k after ctrl-k adds to what it cut; any other key ends that.
        let more = std::mem::take(&mut self.cutting);
        let (row, col) = (self.row, self.col);
        match code {
            KeyCode::Esc => {
                return if self.modified() {
                    Op::Done(self.text())
                } else {
                    Op::Close
                };
            }
            KeyCode::Enter => self.put("\n"),
            KeyCode::Backspace => self.back(),
            KeyCode::Char('h') if ctrl => self.back(),
            KeyCode::Delete => {
                if col < self.lines[row].len() {
                    self.lines[row].remove(col);
                } else if row + 1 < self.lines.len() {
                    let next = self.lines.remove(row + 1);
                    self.lines[row].extend(next);
                }
            }
            KeyCode::Home => self.col = 0,
            KeyCode::Char('a') if ctrl => self.col = 0,
            KeyCode::End => self.col = self.lines[row].len(),
            KeyCode::Char('e') if ctrl => self.col = self.lines[row].len(),
            // By words, as a shell's: alt-b and alt-f, or ctrl or alt with
            // ← →.
            KeyCode::Left if ctrl || alt => (self.row, self.col) = word_left(&self.lines, row, col),
            KeyCode::Char('b') if alt => (self.row, self.col) = word_left(&self.lines, row, col),
            KeyCode::Right if ctrl || alt => (self.row, self.col) = word_right(&self.lines, row, col),
            KeyCode::Char('f') if alt => (self.row, self.col) = word_right(&self.lines, row, col),
            KeyCode::Left => {
                if col > 0 {
                    self.col -= 1;
                } else if row > 0 {
                    self.row -= 1;
                    self.col = self.lines[self.row].len();
                }
            }
            KeyCode::Right => {
                if col < self.lines[row].len() {
                    self.col += 1;
                } else if row + 1 < self.lines.len() {
                    self.row += 1;
                    self.col = 0;
                }
            }
            KeyCode::Up => self.row = row.saturating_sub(1),
            KeyCode::Down => self.row = (row + 1).min(self.lines.len() - 1),
            KeyCode::Char('k') if ctrl => self.cut(more),
            KeyCode::Char('y') if ctrl => self.put(&CUT.with_borrow(String::clone)),
            KeyCode::Char(c) if !ctrl && !alt => self.put(c.encode_utf8(&mut [0; 4])),
            _ => {}
        }
        self.col = self.col.min(self.lines[self.row].len());
        Op::Stay
    }

    /// `text` written where the cursor is, a line broken at each `\n`, the
    /// cursor after it.
    fn put(&mut self, text: &str) {
        // Written on: what was closed here is gone.
        if !text.is_empty() {
            self.ghost.clear();
        }
        for c in text.chars() {
            if c == '\n' {
                let rest = self.lines[self.row].split_off(self.col);
                self.lines.insert(self.row + 1, rest);
                self.row += 1;
                self.col = 0;
            } else {
                self.lines[self.row].insert(self.col, c);
                self.col += 1;
            }
        }
    }

    /// Backspace: the character before the cursor gone; at a line's start,
    /// the line joined to the one before.
    fn back(&mut self) {
        if self.col > 0 {
            self.lines[self.row].remove(self.col - 1);
            self.col -= 1;
        } else if self.row > 0 {
            let cur = self.lines.remove(self.row);
            self.row -= 1;
            self.col = self.lines[self.row].len();
            self.lines[self.row].extend(cur);
        }
    }

    /// ctrl-k: the rest of the line cut, or at its end the line break,
    /// for ctrl-y to put back; after what was cut just before, if `more`.
    fn cut(&mut self, more: bool) {
        let (row, col) = (self.row, self.col);
        let cut: String = if col < self.lines[row].len() {
            self.lines[row].split_off(col).into_iter().collect()
        } else if row + 1 < self.lines.len() {
            let next = self.lines.remove(row + 1);
            self.lines[row].extend(next);
            "\n".into()
        } else {
            String::new()
        };
        if !cut.is_empty() {
            CUT.with_borrow_mut(|c| {
                if !more {
                    c.clear();
                }
                c.push_str(&cut);
            });
        }
        self.cutting = true;
    }

    /// The text as the screen lines it takes at `wrap_w` columns, each with
    /// the part it plays and the mentions on it (as characters of the
    /// screen line, each with whether it names references attached), and
    /// where the cursor is among them.
    #[allow(clippy::type_complexity)]
    fn visual(&self, wrap_w: usize) -> (Vec<(String, LineRole, Vec<(usize, usize, bool)>)>, usize, usize) {
        let wrap_w = wrap_w.max(1);
        let strings: Vec<String> = self.lines.iter().map(|l| l.iter().collect()).collect();
        let roles = thread_line_roles(&strings);
        let mut vis: Vec<(String, LineRole, Vec<(usize, usize, bool)>)> = Vec::new();
        let (mut cur_vrow, mut cur_vcol) = (0, 0);
        for (li, ln) in self.lines.iter().enumerate() {
            let segs: Vec<String> = if ln.is_empty() {
                vec![String::new()]
            } else {
                ln.chunks(wrap_w).map(|c| c.iter().collect()).collect()
            };
            if li < self.row {
                cur_vrow += segs.len();
            } else if li == self.row {
                // The cursor at the end of a full screen line goes on the next.
                let sub = self.col / wrap_w;
                cur_vrow += sub;
                cur_vcol = self.col - sub * wrap_w;
            }
            // The mentions of the line, in characters, each with whether it
            // holds.
            let marks: Vec<(usize, usize, bool)> = match &self.attached {
                Some(ns) if matches!(roles[li], LineRole::You) => {
                    let s = &strings[li];
                    let chars = |b: usize| s[..b].chars().count();
                    crate::refs::mentions(s).iter().map(|m| (chars(m.at.start), chars(m.at.end), crate::refs::holds(m, ns))).collect()
                }
                _ => vec![],
            };
            for (k, seg) in segs.into_iter().enumerate() {
                let (from, to) = (k * wrap_w, (k + 1) * wrap_w);
                let on: Vec<(usize, usize, bool)> = marks
                    .iter()
                    .filter(|(a, b, _)| *a < to && *b > from)
                    .map(|&(a, b, ok)| (a.max(from) - from, b.min(to) - from, ok))
                    .collect();
                vis.push((seg, roles[li], on));
            }
        }
        if cur_vrow >= vis.len() {
            vis.push((String::new(), LineRole::You, vec![]));
        }
        (vis, cur_vrow, cur_vcol)
    }

    /// What was closed here as the screen lines it takes at `wrap_w`
    /// columns.
    fn ghost_lines(&self, wrap_w: usize) -> Vec<String> {
        let wrap_w = wrap_w.max(1);
        self.ghost
            .iter()
            .flat_map(|l| {
                let chars: Vec<char> = l.chars().collect();
                if chars.is_empty() {
                    vec![String::new()]
                } else {
                    chars.chunks(wrap_w).map(|c| c.iter().collect()).collect()
                }
            })
            .collect()
    }

    /// How many lines the box needs inside, `inner` columns wide: it grows
    /// with the text, or with what was closed here while it shows, and with
    /// the references attached.
    pub fn height(&self, inner: usize) -> usize {
        let text = if self.ghosted() { self.ghost_lines(inner).len() } else { self.visual(inner).0.len() };
        text.max(1) + self.under.len()
    }

    /// Draws the editor as a box of `w` by `h` at `(x, y)` of `area`: `title`
    /// on its top edge, the text inside (scrolled when taller), the keys on
    /// its bottom edge, `esc` saying what Esc does. Where the cursor goes.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(&self, buf: &mut Buffer, area: Rect, x: usize, y: usize, w: usize, h: usize, title: &str, esc: &str) -> (u16, u16) {
        let (w, h) = (w.max(8), h.max(3));
        let inner = w - 2;
        // The references attached, under the text: as many as leave a line
        // for it.
        let under = self.under.len().min(h.saturating_sub(3));
        let text_h = h - 2 - under;
        type Spans = Vec<(String, ratatui::style::Style)>;
        // Nothing written yet: what was closed here, dimmed, scrolled by
        // ↑↓, the cursor at its start. Else the text, scrolled to the cursor,
        // its mentions drawn as links, or as broken.
        let (vis, vtop, (cur_row, cur_vcol), foot): (Vec<Spans>, usize, (usize, usize), String) = if self.ghosted() {
            let vis: Vec<Spans> = self.ghost_lines(inner).into_iter().map(|l| vec![(l, color::dim())]).collect();
            let top = self.ghost_top.get().min(vis.len().saturating_sub(text_h));
            self.ghost_top.set(top);
            self.ghost_page.set(text_h);
            (vis, top, (0, 0), format!(" ↑↓: read the last one closed here · enter: a new line · {esc} "))
        } else {
            let (vis, cur_vrow, cur_vcol) = self.visual(inner);
            let mut vtop = self.vtop.get().min(vis.len().saturating_sub(text_h));
            if cur_vrow < vtop {
                vtop = cur_vrow;
            } else if cur_vrow >= vtop + text_h {
                vtop = cur_vrow + 1 - text_h;
            }
            self.vtop.set(vtop);
            let link = ratatui::style::Style::new().add_modifier(ratatui::style::Modifier::UNDERLINED);
            let vis = vis
                .into_iter()
                .map(|(seg, role, marks)| {
                    let st = match role {
                        LineRole::You => ratatui::style::Style::new(),
                        LineRole::Agent => color::agent(),
                        LineRole::Reviewer => color::reviewer(),
                        LineRole::Fence => color::meta(),
                    };
                    let chars: Vec<char> = seg.chars().collect();
                    let (mut spans, mut at) = (Vec::new(), 0);
                    for (a, b, ok) in marks {
                        spans.push((chars[at..a].iter().collect(), st));
                        spans.push((chars[a..b].iter().collect(), if ok { link } else { color::warn().add_modifier(ratatui::style::Modifier::UNDERLINED) }));
                        at = b;
                    }
                    spans.push((chars[at..].iter().collect(), st));
                    spans
                })
                .collect();
            let foot = if self.single { format!(" {esc} ") } else { format!(" enter: a new line · {esc} ") };
            (vis, vtop, (cur_vrow - vtop, cur_vcol), foot)
        };
        window(buf, area, x, y, w, h, title, color::note());
        let room = inner.saturating_sub(2);
        put(buf, area, x + 2, y + h - 1, room, &foot.chars().take(room).collect::<String>(), color::meta());
        if vtop > 0 {
            put(buf, area, x + w - 4, y, 1, "▲", color::meta());
        }
        if vtop + text_h < vis.len() {
            put(buf, area, x + w - 4, y + h - 1, 1, "▼", color::meta());
        }
        for i in 0..text_h {
            let Some(spans) = vis.get(vtop + i) else { break };
            let mut col = 0;
            for (s, st) in spans {
                let n = s.chars().count();
                if n > 0 && col < inner {
                    put(buf, area, x + 1 + col, y + 1 + i, inner - col, s, *st);
                }
                col += n;
            }
        }
        for (k, l) in self.under.iter().take(under).enumerate() {
            put(buf, area, x + 1, y + 1 + text_h + k, inner, &l.chars().take(inner).collect::<String>(), color::meta());
        }
        (
            area.x + (x + 1 + cur_vcol.min(inner.saturating_sub(1))) as u16,
            area.y + (y + 1 + cur_row) as u16,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(e: &mut Editor, code: KeyCode) -> Op {
        e.key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn with(e: &mut Editor, code: KeyCode, m: KeyModifiers) {
        e.key(KeyEvent::new(code, m));
    }

    fn ctrl(e: &mut Editor, c: char) {
        with(e, KeyCode::Char(c), KeyModifiers::CONTROL);
    }

    fn typed(e: &mut Editor, text: &str) {
        for c in text.chars() {
            press(e, KeyCode::Char(c));
        }
    }

    /// What the box at `buf`'s top shows on its inside row `y`.
    fn inside(buf: &Buffer, y: u16) -> String {
        (1..buf.area.width - 1).map(|x| buf[(x, y)].symbol()).collect::<String>().trim_end().to_string()
    }

    #[test]
    fn what_was_closed_here_shows_dimmed_until_you_write_and_scrolls() {
        let mut e = Editor::new("").with_ghost((1..=10).map(|n| format!("line {n}")).collect());
        assert!(e.ghosted());
        assert_eq!(e.height(20), 10, "the box grows to it");
        // In a box of 3 lines inside: dimmed, from its first line, the
        // cursor at its start.
        let draw = |e: &Editor| {
            let mut buf = Buffer::empty(Rect::new(0, 0, 70, 5));
            let area = buf.area;
            let at = e.draw(&mut buf, area, 0, 0, 70, 5, "t", "esc: close");
            (buf, at)
        };
        let (buf, at) = draw(&e);
        assert_eq!(inside(&buf, 1), "line 1");
        assert!(buf[(1, 1)].modifier.contains(ratatui::style::Modifier::DIM));
        assert_eq!(at, (1, 1));
        let foot: String = (0..70).map(|x| buf[(x, 4)].symbol()).collect();
        assert!(foot.contains("↑↓: read the last one closed here"), "{foot}");
        // ↓ scrolls it a line, a page key a box's worth; no further than its
        // end.
        press(&mut e, KeyCode::Down);
        press(&mut e, KeyCode::PageDown);
        assert_eq!(inside(&draw(&e).0, 1), "line 5");
        for _ in 0..20 {
            press(&mut e, KeyCode::Down);
        }
        assert_eq!(inside(&draw(&e).0, 1), "line 8");
        press(&mut e, KeyCode::PageUp);
        assert_eq!(inside(&draw(&e).0, 1), "line 5");
        // Written on, it is gone: the text is what is written; taken back, it
        // stays gone.
        typed(&mut e, "x");
        assert!(!e.ghosted());
        assert_eq!((e.text().as_str(), e.height(20)), ("x", 1));
        press(&mut e, KeyCode::Backspace);
        assert!(!e.ghosted() && e.text().is_empty());
        // Nothing written, Esc closes.
        let mut e = Editor::new("").with_ghost(vec!["a".into()]);
        assert_eq!(press(&mut e, KeyCode::Esc), Op::Close);
    }

    #[test]
    fn ctrl_keys_go_to_a_line_s_ends_and_cut_and_put_back() {
        let mut e = Editor::new("one two\nthree");
        ctrl(&mut e, 'a');
        typed(&mut e, "3 ");
        ctrl(&mut e, 'e');
        typed(&mut e, "!");
        assert_eq!(e.text(), "one two\n3 three!");
        // ctrl-k after "one": the rest of the line; again: the line break.
        press(&mut e, KeyCode::Up);
        ctrl(&mut e, 'a');
        for _ in 0..3 {
            press(&mut e, KeyCode::Right);
        }
        ctrl(&mut e, 'k');
        assert_eq!(e.text(), "one\n3 three!");
        ctrl(&mut e, 'k');
        assert_eq!(e.text(), "one3 three!");
        // ctrl-y puts back both, where the cursor is; again, once more.
        ctrl(&mut e, 'y');
        assert_eq!(e.text(), "one two\n3 three!");
        ctrl(&mut e, 'y');
        assert_eq!(e.text(), "one two\n two\n3 three!");
        // Another key between two ctrl-k: the second cuts anew.
        let mut e = Editor::new("a b");
        press(&mut e, KeyCode::Home);
        ctrl(&mut e, 'k');
        typed(&mut e, "x");
        press(&mut e, KeyCode::Home);
        ctrl(&mut e, 'k');
        ctrl(&mut e, 'y');
        ctrl(&mut e, 'y');
        assert_eq!(e.text(), "xx");
        // What was cut is there for the next comment too; ctrl-k with
        // nothing after the cursor keeps it.
        let mut next = Editor::new("");
        ctrl(&mut next, 'y');
        ctrl(&mut next, 'k');
        ctrl(&mut next, 'y');
        assert_eq!(next.text(), "xx");
    }

    #[test]
    fn words_are_gone_over_with_ctrl_or_alt_and_the_arrows() {
        let lines: Vec<Vec<char>> = ["ab cd", "ef"].iter().map(|l| l.chars().collect()).collect();
        assert_eq!(word_left(&lines, 0, 5), (0, 3));
        assert_eq!(word_right(&lines, 0, 0), (0, 2));
        assert_eq!(word_right(&lines, 0, 5), (1, 0), "from a line's end, the next line");
        assert_eq!(word_left(&lines, 1, 0), (0, 5));
        let mut e = Editor::new("why so loud?");
        with(&mut e, KeyCode::Left, KeyModifiers::CONTROL);
        typed(&mut e, "very ");
        with(&mut e, KeyCode::Char('b'), KeyModifiers::ALT);
        with(&mut e, KeyCode::Left, KeyModifiers::ALT);
        typed(&mut e, "oh ");
        assert_eq!(e.text(), "why oh so very loud?");
        with(&mut e, KeyCode::Right, KeyModifiers::CONTROL);
        with(&mut e, KeyCode::Char('f'), KeyModifiers::ALT);
        typed(&mut e, ",");
        assert_eq!(e.text(), "why oh so very, loud?");
    }

    #[test]
    fn edits_lines_and_says_when_it_changed() {
        let mut e = Editor::new("");
        assert_eq!(press(&mut e, KeyCode::Esc), Op::Close, "nothing written: esc closes");
        for c in "why so".chars() {
            press(&mut e, KeyCode::Char(c));
        }
        press(&mut e, KeyCode::Enter);
        for c in "loud".chars() {
            press(&mut e, KeyCode::Char(c));
        }
        // Back up the line, and on it.
        press(&mut e, KeyCode::Up);
        press(&mut e, KeyCode::End);
        press(&mut e, KeyCode::Char('?'));
        assert_eq!(e.text(), "why so?\nloud");
        // Backspace at a line's start joins it to the one before.
        press(&mut e, KeyCode::Down);
        press(&mut e, KeyCode::Home);
        press(&mut e, KeyCode::Backspace);
        assert_eq!(e.text(), "why so?loud");
        assert_eq!(press(&mut e, KeyCode::Esc), Op::Done("why so?loud".into()));
        // ctrl and alt type nothing.
        let mut e = Editor::new("a");
        e.key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL));
        assert_eq!(e.text(), "a");
    }

    #[test]
    fn grows_with_its_text() {
        let mut e = Editor::new("");
        assert_eq!(e.height(10), 1);
        for c in "0123456789abc".chars() {
            press(&mut e, KeyCode::Char(c));
        }
        assert_eq!(e.height(10), 2, "folded at the width");
    }
}
