//! The browser's half of `stacked web`, compiled to wasm by the build
//! script: what the server sends of the screen made HTML for the page, and
//! the page's keys, clicks, wheel and pastes made the lines the server
//! reads. The page's JS only carries them (web/index.html).
//!
//! The server's lines: `s COLS ROWS`, the screen's size; `r Y RUNS`, row Y,
//! its runs one after the other, RS (\x1e) between them, each its style, US
//! (\x1f), its text, a style `FG,BG,MODS` (a colour: none for the page's,
//! 0 to 15, or #rrggbb; MODS ratatui's modifiers, and CURSOR where the
//! cursor is); `bye`, the editor gone.
//!
//! The lines for the server: `k NAME MODS`, a key (`k c HEX MODS` for a
//! character, by its code), MODS `c` ctrl, `a` alt, `s` shift, or `-`;
//! `m X Y l|r`, a click on a cell, with the left or the right button; `v X
//! Y`, the mouse over another cell, while the page has the focus; `o`, the
//! mouse gone from the screen, or the page losing the focus.

/// Where the cursor is, in a run's modifiers: past ratatui's own.
pub const CURSOR: u32 = 1 << 16;

const BOLD: u32 = 1;
const DIM: u32 = 2;
const ITALIC: u32 = 4;
const UNDERLINED: u32 = 8;
const REVERSED: u32 = 64;
const CROSSED_OUT: u32 = 256;

/// The terminal's sixteen colours, as a dark terminal has them.
const PALETTE: [&str; 16] = [
    "#000000", "#cd3131", "#0dbc79", "#e5e510", "#2472c8", "#bc3fbc", "#11a8cd", "#e5e5e5", "#767676", "#f14c4c",
    "#23d18b", "#f5f543", "#3b8eea", "#d670d6", "#29b8db", "#ffffff",
];

/// The pixels the wheel turns for a line.
const NOTCH: f64 = 40.0;

/// What the page does with one of the server's lines: `size COLS ROWS`;
/// `row Y`, and on the next line the row's HTML; `bye`; else nothing.
pub fn screen(msg: &str) -> String {
    let mut parts = msg.splitn(3, ' ');
    match (parts.next(), parts.next(), parts.next()) {
        (Some("s"), Some(cols), Some(rows)) => format!("size {cols} {rows}"),
        (Some("r"), Some(y), runs) => format!("row {y}\n{}", row_html(runs.unwrap_or(""))),
        (Some("bye"), ..) => "bye".into(),
        _ => String::new(),
    }
}

/// A colour of the server's: the page's own (none), one of the sixteen, or
/// as it is.
fn colour(c: &str) -> Option<String> {
    if c.is_empty() {
        return None;
    }
    if c.starts_with('#') {
        return Some(c.to_string());
    }
    c.parse::<usize>().ok().and_then(|i| PALETTE.get(i)).map(|p| p.to_string())
}

/// `text` safe in HTML.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\n' | '\r' => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

/// A row's runs made HTML: spans coloured and marked as the terminal would
/// draw them; the cursor's cell, `cur`.
pub fn row_html(runs: &str) -> String {
    let mut out = String::new();
    for run in runs.split('\u{1e}') {
        let Some((style, text)) = run.split_once('\u{1f}') else { continue };
        let mut it = style.split(',');
        let fg = colour(it.next().unwrap_or(""));
        let bg = colour(it.next().unwrap_or(""));
        let mods: u32 = it.next().and_then(|m| m.parse().ok()).unwrap_or(0);
        if fg.is_none() && bg.is_none() && mods == 0 {
            out.push_str(&escape(text));
            continue;
        }
        let (mut fg, mut bg) = (fg.unwrap_or("var(--fg)".into()), bg.unwrap_or("var(--bg)".into()));
        if mods & REVERSED != 0 {
            std::mem::swap(&mut fg, &mut bg);
        }
        let mut css = format!("color:{fg};background:{bg}");
        if mods & BOLD != 0 {
            css.push_str(";font-weight:bold");
        }
        if mods & DIM != 0 {
            css.push_str(";opacity:.6");
        }
        if mods & ITALIC != 0 {
            css.push_str(";font-style:italic");
        }
        let lines: Vec<&str> = [(UNDERLINED, "underline"), (CROSSED_OUT, "line-through")]
            .into_iter()
            .filter(|(m, _)| mods & m != 0)
            .map(|(_, l)| l)
            .collect();
        if !lines.is_empty() {
            css.push_str(&format!(";text-decoration:{}", lines.join(" ")));
        }
        let class = if mods & CURSOR != 0 { " class=\"cur\"" } else { "" };
        out.push_str(&format!("<span{class} style=\"{css}\">{}</span>", escape(text)));
    }
    out
}

/// A key the page was pressed, as the browser names it, made a line for
/// the server; none for those not the editor's (with ⌘, the browser's own:
/// copy, paste, its tabs).
pub fn key(key: &str, ctrl: bool, alt: bool, shift: bool, meta: bool) -> String {
    if meta {
        return String::new();
    }
    let mods = |shift: bool| -> String {
        let m: String = [(ctrl, 'c'), (alt, 'a'), (shift, 's')].iter().filter(|(on, _)| *on).map(|(_, m)| *m).collect();
        if m.is_empty() { "-".into() } else { m }
    };
    let name = match key {
        "Enter" => "Enter",
        "Escape" => "Esc",
        "Backspace" => "Backspace",
        "Delete" => "Delete",
        "Tab" if shift => "BackTab",
        "Tab" => "Tab",
        "ArrowUp" => "Up",
        "ArrowDown" => "Down",
        "ArrowLeft" => "Left",
        "ArrowRight" => "Right",
        "PageUp" => "PageUp",
        "PageDown" => "PageDown",
        "Home" => "Home",
        "End" => "End",
        other => {
            // A character, shifted already if it is: by its code.
            let mut cs = other.chars();
            return match (cs.next(), cs.next()) {
                (Some(c), None) => format!("k c {:x} {}", c as u32, mods(false)),
                _ => String::new(),
            };
        }
    };
    format!("k {name} {}", mods(shift && name != "BackTab"))
}

/// A click at `(x, y)` pixels of the screen, its cells `cw` by `ch`, made
/// a line for the server: on a cell, with the right button if `right`.
pub fn click(x: f64, y: f64, cw: f64, ch: f64, right: bool) -> String {
    if cw <= 0.0 || ch <= 0.0 || x < 0.0 || y < 0.0 {
        return String::new();
    }
    format!("m {} {} {}", (x / cw) as u32, (y / ch) as u32, if right { "r" } else { "l" })
}

/// The mouse at `(x, y)` pixels of the screen, its cells `cw` by `ch`, made
/// a line for the server when it is over another cell than `last`, the one
/// it was over, while the page has the focus (`focused`). Without the
/// focus, the mouse is not followed: if it was over a cell, it is said gone
/// from the screen, as `left` says it, and after that nothing is said until
/// the page has the focus again.
pub fn moved(last: &mut Option<(u32, u32)>, x: f64, y: f64, cw: f64, ch: f64, focused: bool) -> String {
    if !focused {
        return if last.is_some() { left(last) } else { String::new() };
    }
    if cw <= 0.0 || ch <= 0.0 || x < 0.0 || y < 0.0 {
        return String::new();
    }
    let at = ((x / cw) as u32, (y / ch) as u32);
    if *last == Some(at) {
        return String::new();
    }
    *last = Some(at);
    format!("v {} {}", at.0, at.1)
}

/// The mouse gone from the screen, or the page losing the focus: said, and
/// the cell the mouse was over, `last`, forgotten, so that coming back over
/// it says so.
pub fn left(last: &mut Option<(u32, u32)>) -> String {
    *last = None;
    "o".into()
}

/// The wheel turned `dy` pixels, kept with what it turned before: a line up
/// or down for each notch of it, as ↑↓.
pub fn wheel(turned: &mut f64, dy: f64) -> String {
    *turned += dy;
    let mut out = Vec::new();
    while turned.abs() >= NOTCH {
        out.push(if *turned < 0.0 { "k Up -" } else { "k Down -" });
        *turned -= NOTCH * turned.signum();
    }
    out.join("\n")
}

/// Text pasted, made the keys that type it.
pub fn paste(text: &str) -> String {
    text.chars()
        .filter(|c| *c != '\r')
        .map(|c| match c {
            '\n' => "k Enter -".to_string(),
            '\t' => "k c 20 -".to_string(),
            c => format!("k c {:x} -", c as u32),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// What the page calls: each takes its text where `alloc` made room for it,
/// and answers with the length of what it made, at `out`.
#[cfg(target_arch = "wasm32")]
mod wasm {
    use std::sync::Mutex;

    static OUT: Mutex<Vec<u8>> = Mutex::new(Vec::new());
    static TURNED: Mutex<f64> = Mutex::new(0.0);
    static OVER: Mutex<Option<(u32, u32)>> = Mutex::new(None);

    /// Room for `n` bytes, the text of the call after.
    #[unsafe(no_mangle)]
    pub extern "C" fn alloc(n: usize) -> *mut u8 {
        Box::into_raw(vec![0u8; n].into_boxed_slice()) as *mut u8
    }

    /// The text at `p`, `n` long, its room given back.
    fn text(p: *mut u8, n: usize) -> String {
        let bytes = unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(p, n)) };
        String::from_utf8_lossy(&bytes).into_owned()
    }

    fn give(s: String) -> usize {
        let mut out = OUT.lock().unwrap();
        *out = s.into_bytes();
        out.len()
    }

    /// Where what the last call made is.
    #[unsafe(no_mangle)]
    pub extern "C" fn out() -> *const u8 {
        OUT.lock().unwrap().as_ptr()
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn screen(p: *mut u8, n: usize) -> usize {
        give(super::screen(&text(p, n)))
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn key(p: *mut u8, n: usize, ctrl: u32, alt: u32, shift: u32, meta: u32) -> usize {
        give(super::key(&text(p, n), ctrl != 0, alt != 0, shift != 0, meta != 0))
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn click(x: f64, y: f64, cw: f64, ch: f64, right: u32) -> usize {
        give(super::click(x, y, cw, ch, right != 0))
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn moved(x: f64, y: f64, cw: f64, ch: f64, focused: u32) -> usize {
        let mut over = OVER.lock().unwrap();
        give(super::moved(&mut over, x, y, cw, ch, focused != 0))
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn left() -> usize {
        let mut over = OVER.lock().unwrap();
        give(super::left(&mut over))
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn wheel(dy: f64) -> usize {
        let mut turned = TURNED.lock().unwrap();
        give(super::wheel(&mut turned, dy))
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn paste(p: *mut u8, n: usize) -> usize {
        give(super::paste(&text(p, n)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_are_html_as_the_terminal_draws_them() {
        assert_eq!(screen("s 120 40"), "size 120 40");
        // Plain, then green, then reversed with the page's colours, the
        // cursor marked; what is HTML, escaped.
        let row = "r 3 ,,0\u{1f} a <b>\u{1e}2,,1\u{1f}+x\u{1e},,65600\u{1f} ";
        assert_eq!(
            screen(row),
            "row 3\n a &lt;b&gt;<span style=\"color:#0dbc79;background:var(--bg);font-weight:bold\">+x</span><span class=\"cur\" style=\"color:var(--bg);background:var(--fg)\"> </span>"
        );
        assert!(row_html("#102030,4,10\u{1f}d").contains("color:#102030;background:#2472c8;opacity:.6;text-decoration:underline"));
        assert_eq!(screen("bye"), "bye");
        assert_eq!(screen("what"), "");
    }

    #[test]
    fn keys_clicks_wheel_and_pastes_are_lines_for_the_server() {
        assert_eq!(key("Enter", false, false, false, false), "k Enter -");
        assert_eq!(key("ArrowDown", false, false, true, false), "k Down s");
        assert_eq!(key("Tab", false, false, true, false), "k BackTab -");
        assert_eq!(key("?", false, false, true, false), "k c 3f -", "shifted already");
        assert_eq!(key("c", true, false, false, false), "k c 63 c");
        assert_eq!(key(" ", false, false, false, false), "k c 20 -");
        assert_eq!(key("é", false, true, false, false), "k c e9 a");
        assert_eq!(key("v", false, false, false, true), "", "⌘V, the browser's");
        assert_eq!(key("Shift", false, false, true, false), "");
        assert_eq!(click(17.0, 41.0, 8.0, 20.0, false), "m 2 2 l");
        assert_eq!(click(0.0, 0.0, 8.0, 20.0, true), "m 0 0 r");
        assert_eq!(click(5.0, 5.0, 0.0, 20.0, false), "");
        // The mouse moved: said when it is over another cell.
        let mut over = None;
        assert_eq!(moved(&mut over, 17.0, 41.0, 8.0, 20.0, true), "v 2 2");
        assert_eq!(moved(&mut over, 20.0, 45.0, 8.0, 20.0, true), "", "the same cell");
        assert_eq!(moved(&mut over, 25.0, 45.0, 8.0, 20.0, true), "v 3 2");
        // Gone from the screen: said; back over the same cell, said again.
        assert_eq!(left(&mut over), "o");
        assert_eq!(moved(&mut over, 25.0, 45.0, 8.0, 20.0, true), "v 3 2");
        let mut turned = 0.0;
        assert_eq!(wheel(&mut turned, 30.0), "");
        assert_eq!(wheel(&mut turned, 30.0), "k Down -", "a notch, with what it turned before");
        assert_eq!(wheel(&mut turned, -100.0), "k Up -\nk Up -");
        assert_eq!(paste("a\r\nb\t"), "k c 61 -\nk Enter -\nk c 62 -\nk c 20 -");
    }

    #[test]
    fn the_mouse_is_followed_only_while_the_page_has_the_focus() {
        let mut over = None;
        assert_eq!(moved(&mut over, 17.0, 41.0, 8.0, 20.0, true), "v 2 2");
        // The page losing the focus: the mouse said gone, as when it leaves
        // the screen, then not followed.
        assert_eq!(left(&mut over), "o");
        assert_eq!(moved(&mut over, 25.0, 45.0, 8.0, 20.0, false), "");
        assert_eq!(moved(&mut over, 33.0, 45.0, 8.0, 20.0, false), "");
        // The focus back: the mouse followed from its next movement, over
        // the cell it was over before as over another.
        assert_eq!(moved(&mut over, 17.0, 41.0, 8.0, 20.0, true), "v 2 2");
        // Moved without the focus while it was over a cell, its loss not
        // said: the mouse said gone, once, then not followed.
        assert_eq!(moved(&mut over, 25.0, 45.0, 8.0, 20.0, false), "o");
        assert_eq!(moved(&mut over, 33.0, 45.0, 8.0, 20.0, false), "");
        assert_eq!(moved(&mut over, 33.0, 45.0, 8.0, 20.0, true), "v 4 2");
    }
}
