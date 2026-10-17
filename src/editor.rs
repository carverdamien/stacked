//! The comment editor, inline where the comment will be: in the patch under
//! its line, in the comments pane under the comment it answers, in the
//! patches pane under the stack's header or a patch. After self-review's.
//! It edits text; what is done with the text is the app's, which the editor
//! asks for with an `Op`.

use std::cell::Cell;

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

#[derive(Debug, Clone, PartialEq)]
pub struct Editor {
    lines: Vec<Vec<char>>,
    row: usize,
    col: usize,
    vtop: Cell<usize>,
    initial: String,
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
        }
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
        let typed = !k.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
        let (lines, row, col) = (&mut self.lines, &mut self.row, &mut self.col);
        match k.code {
            KeyCode::Esc => {
                return if self.modified() {
                    Op::Done(self.text())
                } else {
                    Op::Close
                };
            }
            KeyCode::Enter => {
                let rest = lines[*row].split_off(*col);
                lines.insert(*row + 1, rest);
                *row += 1;
                *col = 0;
            }
            KeyCode::Backspace => {
                if *col > 0 {
                    lines[*row].remove(*col - 1);
                    *col -= 1;
                } else if *row > 0 {
                    *col = lines[*row - 1].len();
                    let cur = lines.remove(*row);
                    lines[*row - 1].extend(cur);
                    *row -= 1;
                }
            }
            KeyCode::Delete => {
                if *col < lines[*row].len() {
                    lines[*row].remove(*col);
                } else if *row + 1 < lines.len() {
                    let next = lines.remove(*row + 1);
                    lines[*row].extend(next);
                }
            }
            KeyCode::Home => *col = 0,
            KeyCode::End => *col = lines[*row].len(),
            KeyCode::Left => {
                if *col > 0 {
                    *col -= 1;
                } else if *row > 0 {
                    *row -= 1;
                    *col = lines[*row].len();
                }
            }
            KeyCode::Right => {
                if *col < lines[*row].len() {
                    *col += 1;
                } else if *row + 1 < lines.len() {
                    *row += 1;
                    *col = 0;
                }
            }
            KeyCode::Up => *row = row.saturating_sub(1),
            KeyCode::Down => *row = (*row + 1).min(lines.len() - 1),
            KeyCode::Char(c) if typed => {
                lines[*row].insert(*col, c);
                *col += 1;
            }
            _ => {}
        }
        self.col = self.col.min(self.lines[self.row].len());
        Op::Stay
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
