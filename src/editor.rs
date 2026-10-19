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
    /// Esc with nothing changed: close.
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
        }
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
    /// the part it plays, and where the cursor is among them.
    fn visual(&self, wrap_w: usize) -> (Vec<(String, LineRole)>, usize, usize) {
        let wrap_w = wrap_w.max(1);
        let strings: Vec<String> = self.lines.iter().map(|l| l.iter().collect()).collect();
        let roles = thread_line_roles(&strings);
        let mut vis: Vec<(String, LineRole)> = Vec::new();
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
            vis.extend(segs.into_iter().map(|s| (s, roles[li])));
        }
        if cur_vrow >= vis.len() {
            vis.push((String::new(), LineRole::You));
        }
        (vis, cur_vrow, cur_vcol)
    }

    /// How many lines the box needs inside, `inner` columns wide: it grows
    /// with the text.
    pub fn height(&self, inner: usize) -> usize {
        self.visual(inner).0.len().max(1)
    }

    /// Draws the editor as a box of `w` by `h` at `(x, y)` of `area`: `title`
    /// on its top edge, the text inside (scrolled when taller), the keys on
    /// its bottom edge. Where the cursor goes.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(&self, buf: &mut Buffer, area: Rect, x: usize, y: usize, w: usize, h: usize, title: &str) -> (u16, u16) {
        let (w, h) = (w.max(8), h.max(3));
        let inner = w - 2;
        let text_h = h - 2;
        let (vis, cur_vrow, cur_vcol) = self.visual(inner);
        let mut vtop = self.vtop.get().min(vis.len().saturating_sub(text_h));
        if cur_vrow < vtop {
            vtop = cur_vrow;
        } else if cur_vrow >= vtop + text_h {
            vtop = cur_vrow + 1 - text_h;
        }
        self.vtop.set(vtop);
        window(buf, area, x, y, w, h, title, color::comment());
        let foot = " enter: a new line · esc: save, send or throw away ";
        let room = inner.saturating_sub(2);
        put(buf, area, x + 2, y + h - 1, room, &foot.chars().take(room).collect::<String>(), color::meta());
        if vtop > 0 {
            put(buf, area, x + w - 4, y, 1, "▲", color::meta());
        }
        if vtop + text_h < vis.len() {
            put(buf, area, x + w - 4, y + h - 1, 1, "▼", color::meta());
        }
        for i in 0..text_h {
            let Some((seg, role)) = vis.get(vtop + i) else { break };
            let st = match role {
                LineRole::You => color::comment(),
                LineRole::Agent => color::add(),
                LineRole::Reviewer => color::reviewer(),
                LineRole::Fence => color::meta(),
            };
            put(buf, area, x + 1, y + 1 + i, inner, seg, st);
        }
        (
            area.x + (x + 1 + cur_vcol.min(inner.saturating_sub(1))) as u16,
            area.y + (y + 1 + cur_vrow - vtop) as u16,
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
