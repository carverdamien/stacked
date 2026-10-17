//! Painting on a frame's buffer: the colours, named for what they show,
//! text clipped to an area, and the boxes menus and editors are drawn in.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

pub mod color {
    use super::*;

    pub fn add() -> Style {
        Style::new().fg(Color::Green)
    }
    pub fn del() -> Style {
        Style::new().fg(Color::Red)
    }
    pub fn hunk() -> Style {
        Style::new().fg(Color::Magenta)
    }
    pub fn file() -> Style {
        Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)
    }
    pub fn comment() -> Style {
        Style::new().fg(Color::Yellow)
    }
    pub fn reviewer() -> Style {
        Style::new().fg(Color::LightBlue)
    }
    pub fn meta() -> Style {
        Style::new().fg(Color::Blue)
    }
    pub fn bar() -> Style {
        Style::new().fg(Color::White).bg(Color::Blue)
    }
    /// The bar, when the stack cannot be changed or does not apply.
    pub fn warn() -> Style {
        Style::new().fg(Color::Black).bg(Color::Yellow)
    }
    pub fn status() -> Style {
        Style::new().fg(Color::Black).bg(Color::White)
    }
    pub fn dim() -> Style {
        Style::new().add_modifier(Modifier::DIM)
    }
    /// A pane's header row.
    pub fn header() -> Style {
        Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)
    }
}

/// Writes `text` at `(x, y)` of `area`, cut at `w` and the area's edge.
pub fn put(buf: &mut Buffer, area: Rect, x: usize, y: usize, w: usize, text: &str, st: Style) {
    if y >= area.height as usize || x >= area.width as usize {
        return;
    }
    let w = w.min(area.width as usize - x);
    buf.set_stringn(area.x + x as u16, area.y + y as u16, text, w, st);
}

/// `text` cut to `w` characters and padded to them.
pub fn fit(text: &str, w: usize) -> String {
    let mut s: String = text.chars().take(w).collect();
    let n = s.chars().count();
    s.extend(std::iter::repeat_n(' ', w - n));
    s
}

/// `text` folded into lines of at most `w` characters, at spaces.
pub fn wrap(text: &str, w: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > w {
            out.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        out.push(line);
    }
    out
}

/// A cleared, bordered box of `w` by `h` at `(x, y)` of `area`, `title` on
/// its top edge.
#[allow(clippy::too_many_arguments)]
pub fn window(buf: &mut Buffer, area: Rect, x: usize, y: usize, w: usize, h: usize, title: &str, st: Style) {
    let w = w.min((area.width as usize).saturating_sub(x));
    let h = h.min((area.height as usize).saturating_sub(y));
    if w < 6 || h < 2 {
        return;
    }
    // Cleared for real, so nothing under it shows through.
    for row in y..y + h {
        for col in x..x + w {
            buf[(area.x + col as u16, area.y + row as u16)].reset();
        }
    }
    let horiz = "─".repeat(w - 2);
    put(buf, area, x, y, w, &format!("┌{horiz}┐"), st);
    put(buf, area, x, y + h - 1, w, &format!("└{horiz}┘"), st);
    for row in y + 1..y + h - 1 {
        put(buf, area, x, row, 1, "│", st);
        put(buf, area, x + w - 1, row, 1, "│", st);
    }
    if !title.is_empty() {
        let title: String = format!(" {title} ").chars().take(w - 4).collect();
        put(buf, area, x + 2, y, w - 4, &title, color::file());
    }
}
