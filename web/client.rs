//! The browser's half of `stacked web`, compiled to wasm by the build
//! script: what the server sends of the screen made HTML for the page, and
//! the page's keys, clicks, wheel and pastes made the lines the server
//! reads. The page's JS only carries them (web/index.html).
//!
//! The server's lines: `s COLS ROWS`, the screen's size; `r Y RUNS`, row Y,
//! its runs one after the other, RS (\x1e) between them, each its style, US
//! (\x1f), its text, a style `FG,BG,MODS` (a colour: none for the page's,
//! 0 to 15, or #rrggbb; MODS ratatui's modifiers, and CURSOR where the
//! cursor is); `bye`, the editor gone; `n RUN US TITLE US BODY`, a notice of
//! what the editor learned of run RUN, for the page to show as the system's
//! banner while it is not what you look at; `e PROGRAM`, `$EDITOR` running
//! PROGRAM, for the page to say over its screen until the next screen
//! comes. A page of an older stacked does nothing with a line it does not
//! know, such as a notice.
//!
//! The lines for the server: `k NAME MODS`, a key (`k c HEX MODS` for a
//! character, by its code), MODS `c` ctrl, `a` alt, `s` shift, or `-`;
//! `m X Y l|r`, a click on a cell, with the left or the right button, said
//! as the button is pressed; `u X Y`, the left button let go over a cell
//! (`u` over none); `w X Y N`, the wheel turned N lines over a cell, down,
//! or up below 0 (`w N` over none); `v X Y`, the mouse over another cell
//! while the page has the focus, or, when the page gets the focus back,
//! over the cell where the mouse was last seen while the page did not have
//! it; `d X Y` for that, the left button down; `o`, the mouse gone from the
//! screen, or the page losing the focus; `n RUN`, the notice of run RUN
//! clicked, which brought the page forward.

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

/// A turn of the wheel said in pixels, as the page's `deltaMode` says it:
/// a trackpad's, and any wheel's in Chrome and Safari. Else it is said in
/// lines (1) or in pages (2): a mouse's notch, as Firefox says it to a page
/// that reads the mode first (web/index.html).
const PIXELS: u32 = 0;

/// What the page does with one of the server's lines: `size COLS ROWS`;
/// `row Y`, and on the next line the row's HTML; `bye`; `notice`, and on
/// the next lines what `notice` makes of it; `editing`, and on the next
/// lines what to say over the screen (`editing`); else nothing.
pub fn screen(msg: &str) -> String {
    if let Some(n) = msg.strip_prefix("n ") {
        return notice(n);
    }
    if let Some(program) = msg.strip_prefix("e ") {
        return format!("editing\n{}", editing(program));
    }
    let mut parts = msg.splitn(3, ' ');
    match (parts.next(), parts.next(), parts.next()) {
        (Some("s"), Some(cols), Some(rows)) => format!("size {cols} {rows}"),
        (Some("r"), Some(y), runs) => format!("row {y}\n{}", row_html(runs.unwrap_or(""))),
        (Some("bye"), ..) => "bye".into(),
        _ => String::new(),
    }
}

/// A notice of the server's, `RUN US TITLE US BODY`, made what the page
/// shows as the system's banner: its tag, which a banner of the same run
/// takes the place of, as when two pages show it; its title; its body; and
/// the line for the server once it is clicked, `n RUN`, each on a line of
/// its own. Nothing for one short of a field, or whose run is empty or
/// holds a space, which the line for the server could not say.
fn notice(n: &str) -> String {
    let mut fields = n.split('\u{1f}');
    let (Some(run), Some(title), Some(body)) = (fields.next(), fields.next(), fields.next()) else { return String::new() };
    if run.is_empty() || run.contains(char::is_whitespace) {
        return String::new();
    }
    let line = |s: &str| s.replace(['\n', '\r'], " ");
    format!("notice\n{run}\n{}\n{}\nn {run}", line(title), line(body))
}

/// What the page says over its screen while `$EDITOR` runs `program`, in
/// StackED's words over its window (src/app.rs `waiting`): where it runs,
/// and that stacked waits for it.
fn editing(program: &str) -> String {
    let program = if program.trim().is_empty() { "the editor" } else { program };
    format!("The text is open in {program}, in the terminal `stacked web` was started from if {program} runs in one.\nstacked goes on once {program} exits.")
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

/// What the page's mouse has done: the cell it is over, as the server was
/// last told, and the cell where it was last seen over the screen while the
/// page did not have the focus, unless it left the screen since. When the
/// page has the focus again, the mouse is said over that second cell at
/// once, as StackED says it of a window that is the active one again
/// (`Pointer::focus`, in src/app.rs).
#[derive(Default)]
pub struct Mouse {
    over: Option<(u32, u32)>,
    seen: Option<(u32, u32)>,
}

/// The cell at `(x, y)` pixels of the screen, its cells `cw` by `ch`.
fn cell(x: f64, y: f64, cw: f64, ch: f64) -> Option<(u32, u32)> {
    if cw <= 0.0 || ch <= 0.0 || x < 0.0 || y < 0.0 {
        return None;
    }
    Some(((x / cw) as u32, (y / ch) as u32))
}

/// A click at `(x, y)` pixels of the screen, its cells `cw` by `ch`, made
/// a line for the server: on a cell, with the right button if `right`.
/// The click with the left button that gives the page the focus back may
/// come before the focus: if the mouse was seen over a cell while the page
/// did not have the focus, the mouse is said over that cell first, as
/// `focus` says it, so that the click acts as it does after the focus, and
/// the focus then has nothing more to say. A click with the right button
/// leaves a window in the background on macOS: the mouse is not said over
/// a cell before it.
pub fn click(mouse: &mut Mouse, x: f64, y: f64, cw: f64, ch: f64, right: bool) -> String {
    let Some(at) = cell(x, y, cw, ch) else { return String::new() };
    let click = format!("m {} {} {}", at.0, at.1, if right { "r" } else { "l" });
    let over = if right { String::new() } else { focus(mouse) };
    if over.is_empty() { click } else { format!("{over}\n{click}") }
}

/// The mouse at `(x, y)` pixels of the screen, its cells `cw` by `ch`, made
/// a line for the server when it is over another cell than the one it was
/// over, while the page has the focus (`focused`). Without the focus, the
/// mouse is not followed: if it was over a cell, it is said gone from the
/// screen, as `left` says it, and after that nothing is said until the page
/// has the focus again. The cell where the mouse is seen meanwhile is kept,
/// for `focus` to say.
pub fn moved(mouse: &mut Mouse, x: f64, y: f64, cw: f64, ch: f64, focused: bool) -> String {
    let at = cell(x, y, cw, ch);
    if !focused {
        mouse.seen = at;
        return if mouse.over.take().is_some() { "o".into() } else { String::new() };
    }
    // Followed again: where the mouse was seen without the focus is past.
    mouse.seen = None;
    match at {
        Some(at) if mouse.over != Some(at) => {
            mouse.over = Some(at);
            format!("v {} {}", at.0, at.1)
        }
        _ => String::new(),
    }
}

/// The mouse at `(x, y)` pixels of the screen, its cells `cw` by `ch`, the
/// left button down: as `moved` says it, `d X Y` rather than `v X Y`, for
/// the editor to drag what the press was held on (HEAD's row), or else to
/// take it for the mouse going by.
pub fn dragged(mouse: &mut Mouse, x: f64, y: f64, cw: f64, ch: f64, focused: bool) -> String {
    let said = moved(mouse, x, y, cw, ch, focused);
    match said.strip_prefix("v ") {
        Some(at) => format!("d {at}"),
        None => said,
    }
}

/// The left button let go at `(x, y)` pixels of the screen, its cells `cw`
/// by `ch`: `u X Y`, or `u` over no cell, for the editor to put down what
/// it drags, or to click what the press was held on.
pub fn up(x: f64, y: f64, cw: f64, ch: f64) -> String {
    match cell(x, y, cw, ch) {
        Some(at) => format!("u {} {}", at.0, at.1),
        None => "u".into(),
    }
}

/// The mouse gone from the screen: said, and the cell it was over
/// forgotten, so that coming back over it says so; the cell where it was
/// seen while the page did not have the focus is forgotten too.
pub fn left(mouse: &mut Mouse) -> String {
    *mouse = Mouse::default();
    "o".into()
}

/// The page losing the focus: the mouse said gone, as `left` says it, and
/// not followed until the page has the focus again. The cell where the
/// mouse was seen while the page did not have the focus, which the browser
/// may send before the loss of the focus, is kept.
pub fn blur(mouse: &mut Mouse) -> String {
    mouse.over = None;
    "o".into()
}

/// The page given the focus back: the mouse said at once over the cell
/// where it was last seen while the page did not have the focus, so that
/// the click that gave the focus back acts on what is under the mouse, as
/// any click does where the mouse is followed. If the mouse was not seen
/// over the screen meanwhile, or left the screen since, nothing is said,
/// and the mouse is followed from its next movement.
pub fn focus(mouse: &mut Mouse) -> String {
    let Some(at) = mouse.seen.take() else { return String::new() };
    mouse.over = Some(at);
    format!("v {} {}", at.0, at.1)
}

/// The wheel turned `dy` at `(x, y)` pixels of the screen, its cells `cw`
/// by `ch`: once it turned a notch or more, made a line for the server, the
/// cell under the mouse and the lines turned, one for each notch, down (up,
/// below 0). Turned in pixels (`mode`, `PIXELS`), it is kept with what it
/// turned before, a notch for each 40 pixels. Turned in lines or in pages,
/// as Firefox says a mouse's notch, each turn is a notch, whatever the
/// lines or the pages it says, and what the pixels kept is left as it was:
/// a line on the screen for each notch, always. The editor scrolls what is
/// under the mouse, or takes them for ↑↓.
pub fn wheel(turned: &mut f64, x: f64, y: f64, cw: f64, ch: f64, dy: f64, mode: u32) -> String {
    let lines = match mode {
        PIXELS => {
            *turned += dy;
            let mut lines = 0i64;
            while turned.abs() >= NOTCH {
                lines += turned.signum() as i64;
                *turned -= NOTCH * turned.signum();
            }
            lines
        }
        // A notch, down or up as it turns; turned sideways alone, none.
        _ if dy > 0.0 => 1,
        _ if dy < 0.0 => -1,
        _ => 0,
    };
    match (lines, cell(x, y, cw, ch)) {
        (0, _) => String::new(),
        (n, Some(at)) => format!("w {} {} {n}", at.0, at.1),
        (n, None) => format!("w {n}"),
    }
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
    static MOUSE: Mutex<super::Mouse> = Mutex::new(super::Mouse { over: None, seen: None });

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
        let mut mouse = MOUSE.lock().unwrap();
        give(super::click(&mut mouse, x, y, cw, ch, right != 0))
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn moved(x: f64, y: f64, cw: f64, ch: f64, focused: u32) -> usize {
        let mut mouse = MOUSE.lock().unwrap();
        give(super::moved(&mut mouse, x, y, cw, ch, focused != 0))
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn dragged(x: f64, y: f64, cw: f64, ch: f64, focused: u32) -> usize {
        let mut mouse = MOUSE.lock().unwrap();
        give(super::dragged(&mut mouse, x, y, cw, ch, focused != 0))
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn up(x: f64, y: f64, cw: f64, ch: f64) -> usize {
        give(super::up(x, y, cw, ch))
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn left() -> usize {
        let mut mouse = MOUSE.lock().unwrap();
        give(super::left(&mut mouse))
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn blur() -> usize {
        let mut mouse = MOUSE.lock().unwrap();
        give(super::blur(&mut mouse))
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn focus() -> usize {
        let mut mouse = MOUSE.lock().unwrap();
        give(super::focus(&mut mouse))
    }

    #[unsafe(no_mangle)]
    pub extern "C" fn wheel(x: f64, y: f64, cw: f64, ch: f64, dy: f64, mode: u32) -> usize {
        let mut turned = TURNED.lock().unwrap();
        give(super::wheel(&mut turned, x, y, cw, ch, dy, mode))
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
    fn a_notice_is_a_banner_for_the_page_its_click_a_line_for_the_server() {
        // Its tag, its title, its body, and the line its click sends, each
        // on a line of its own, whatever spaces they hold.
        let n = "n run-1\u{1f}stacked: bug-2084854\u{1f}A landed ask waits for your review: 1 reply.";
        assert_eq!(screen(n), "notice\nrun-1\nstacked: bug-2084854\nA landed ask waits for your review: 1 reply.\nn run-1");
        // A new line in a title or a body is a space, so that each stays on
        // its own line.
        assert_eq!(screen("n r\u{1f}a\nb\u{1f}c\r\nd"), "notice\nr\na b\nc  d\nn r");
        // One short of a field, or whose run is empty or holds a space:
        // nothing.
        assert_eq!(screen("n run-1\u{1f}stacked: s"), "");
        assert_eq!(screen("n \u{1f}stacked: s\u{1f}Done."), "");
        assert_eq!(screen("n a run\u{1f}stacked: s\u{1f}Done."), "");
    }

    #[test]
    fn the_editor_running_is_said_over_the_screen_by_the_program_it_runs() {
        assert_eq!(
            screen("e vim"),
            "editing\nThe text is open in vim, in the terminal `stacked web` was started from if vim runs in one.\nstacked goes on once vim exits."
        );
        // No program named: the editor.
        assert_eq!(
            screen("e "),
            "editing\nThe text is open in the editor, in the terminal `stacked web` was started from if the editor runs in one.\nstacked goes on once the editor exits."
        );
        assert_eq!(screen("e"), "");
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
        let mut mouse = Mouse::default();
        assert_eq!(click(&mut mouse, 17.0, 41.0, 8.0, 20.0, false), "m 2 2 l");
        assert_eq!(click(&mut mouse, 0.0, 0.0, 8.0, 20.0, true), "m 0 0 r");
        assert_eq!(click(&mut mouse, 5.0, 5.0, 0.0, 20.0, false), "");
        // The mouse moved: said when it is over another cell.
        assert_eq!(moved(&mut mouse, 17.0, 41.0, 8.0, 20.0, true), "v 2 2");
        assert_eq!(moved(&mut mouse, 20.0, 45.0, 8.0, 20.0, true), "", "the same cell");
        assert_eq!(moved(&mut mouse, 25.0, 45.0, 8.0, 20.0, true), "v 3 2");
        // Gone from the screen: said; back over the same cell, said again.
        assert_eq!(left(&mut mouse), "o");
        assert_eq!(moved(&mut mouse, 25.0, 45.0, 8.0, 20.0, true), "v 3 2");
        // The wheel turned in pixels: said with the cell under the pointer
        // once it turned a notch (40 pixels), a line for each, with what it
        // turned before; over no cell, the lines alone.
        let mut turned = 0.0;
        assert_eq!(wheel(&mut turned, 17.0, 41.0, 8.0, 20.0, 30.0, PIXELS), "");
        assert_eq!(wheel(&mut turned, 17.0, 41.0, 8.0, 20.0, 30.0, PIXELS), "w 2 2 1", "a notch, with what it turned before");
        assert_eq!(wheel(&mut turned, 25.0, 45.0, 8.0, 20.0, -100.0, PIXELS), "w 3 2 -2", "two notches up");
        assert_eq!(wheel(&mut turned, 25.0, 45.0, 8.0, 20.0, -39.0, PIXELS), "", "short of a notch again");
        assert_eq!(wheel(&mut turned, 5.0, 5.0, 0.0, 20.0, 119.0, PIXELS), "w 2", "over no cell");
        assert_eq!(paste("a\r\nb\t"), "k c 61 -\nk Enter -\nk c 62 -\nk c 20 -");
    }

    #[test]
    fn the_wheel_turned_in_lines_or_in_pages_is_a_line_for_each_turn() {
        // The modes the page passes besides pixels, as `deltaMode` has them.
        const LINES: u32 = 1;
        const PAGES: u32 = 2;
        // In lines, as Firefox says a mouse's notch: a line for each turn,
        // down or up as it turns, whatever the lines it says, with nothing
        // kept: 39 pixels after them are short of a notch still.
        let mut turned = 0.0;
        assert_eq!(wheel(&mut turned, 17.0, 41.0, 8.0, 20.0, 3.0, LINES), "w 2 2 1", "three lines");
        assert_eq!(wheel(&mut turned, 17.0, 41.0, 8.0, 20.0, 1.0, LINES), "w 2 2 1", "a line");
        assert_eq!(wheel(&mut turned, 25.0, 45.0, 8.0, 20.0, -2.0, LINES), "w 3 2 -1", "two lines up");
        assert_eq!(wheel(&mut turned, 17.0, 41.0, 8.0, 20.0, 0.0, LINES), "", "turned sideways alone");
        // In pages: a line for each turn as well; over no cell, the line alone.
        assert_eq!(wheel(&mut turned, 17.0, 41.0, 8.0, 20.0, 1.0, PAGES), "w 2 2 1", "a page");
        assert_eq!(wheel(&mut turned, 5.0, 5.0, 0.0, 20.0, -1.0, PAGES), "w -1", "a page up, over no cell");
        assert_eq!(wheel(&mut turned, 17.0, 41.0, 8.0, 20.0, 39.0, PIXELS), "", "nothing kept of the lines");
        // Turned in lines between turns in pixels: what the pixels kept, 39,
        // left as it was, so that a pixel more makes their notch.
        assert_eq!(wheel(&mut turned, 17.0, 41.0, 8.0, 20.0, 3.0, LINES), "w 2 2 1");
        assert_eq!(wheel(&mut turned, 17.0, 41.0, 8.0, 20.0, -3.0, LINES), "w 2 2 -1");
        assert_eq!(wheel(&mut turned, 17.0, 41.0, 8.0, 20.0, 1.0, PIXELS), "w 2 2 1", "39 pixels kept, and one");
        assert_eq!(wheel(&mut turned, 17.0, 41.0, 8.0, 20.0, 39.0, PIXELS), "", "nothing more kept since");
    }

    #[test]
    fn a_drag_and_the_left_button_let_go_are_lines_for_the_server_too() {
        let mut mouse = Mouse::default();
        // Pressed: the click, as ever. Then the mouse over another cell with
        // the button down: said as the mouse going by is, `d` for `v`.
        assert_eq!(click(&mut mouse, 17.0, 41.0, 8.0, 20.0, false), "m 2 2 l");
        assert_eq!(dragged(&mut mouse, 17.0, 41.0, 8.0, 20.0, true), "d 2 2");
        assert_eq!(dragged(&mut mouse, 20.0, 45.0, 8.0, 20.0, true), "", "the same cell");
        assert_eq!(dragged(&mut mouse, 33.0, 45.0, 8.0, 20.0, true), "d 4 2");
        // The page without the focus: not followed, as the mouse is not.
        assert_eq!(dragged(&mut mouse, 41.0, 45.0, 8.0, 20.0, false), "o");
        // Let go: with the cell it is over, over none off the screen.
        assert_eq!(up(33.0, 45.0, 8.0, 20.0), "u 4 2");
        assert_eq!(up(-5.0, 45.0, 8.0, 20.0), "u");
    }

    #[test]
    fn the_mouse_is_followed_only_while_the_page_has_the_focus() {
        let mut mouse = Mouse::default();
        assert_eq!(moved(&mut mouse, 17.0, 41.0, 8.0, 20.0, true), "v 2 2");
        // The page losing the focus: the mouse said gone, as when it leaves
        // the screen, then not followed: it is said over no cell.
        assert_eq!(blur(&mut mouse), "o");
        assert_eq!(moved(&mut mouse, 25.0, 45.0, 8.0, 20.0, false), "");
        assert_eq!(moved(&mut mouse, 33.0, 45.0, 8.0, 20.0, false), "");
        // A click with the right button, which leaves the page in the
        // background on macOS, acts all the same, and the mouse is said over
        // no cell before it.
        assert_eq!(click(&mut mouse, 33.0, 45.0, 8.0, 20.0, true), "m 4 2 r");
        assert_eq!(moved(&mut mouse, 41.0, 45.0, 8.0, 20.0, false), "");
        // The focus back: the mouse said at once over the cell where it was
        // last seen, then followed.
        assert_eq!(focus(&mut mouse), "v 5 2");
        assert_eq!(moved(&mut mouse, 17.0, 41.0, 8.0, 20.0, true), "v 2 2");
        // Moved without the focus while it was over a cell, the loss of the
        // focus not said yet: the mouse said gone, once, then not followed.
        // The loss said after keeps the cell where the mouse was seen.
        assert_eq!(moved(&mut mouse, 25.0, 45.0, 8.0, 20.0, false), "o");
        assert_eq!(moved(&mut mouse, 33.0, 45.0, 8.0, 20.0, false), "");
        assert_eq!(blur(&mut mouse), "o");
        assert_eq!(focus(&mut mouse), "v 4 2");
    }

    #[test]
    fn where_the_mouse_was_seen_without_the_focus_is_said_once_the_page_has_it_again() {
        let mut mouse = Mouse::default();
        // Seen over the screen while the page does not have the focus: the
        // last cell it was seen over said at once when the page has the
        // focus again, then the mouse followed.
        assert_eq!(moved(&mut mouse, 17.0, 41.0, 8.0, 20.0, false), "");
        assert_eq!(moved(&mut mouse, 25.0, 45.0, 8.0, 20.0, false), "");
        assert_eq!(focus(&mut mouse), "v 3 2");
        assert_eq!(focus(&mut mouse), "", "said once");
        assert_eq!(moved(&mut mouse, 26.0, 46.0, 8.0, 20.0, true), "", "the same cell");
        assert_eq!(moved(&mut mouse, 33.0, 45.0, 8.0, 20.0, true), "v 4 2");
        // Gone from the screen since: forgotten, the focus back saying
        // nothing, and a click acting alone.
        assert_eq!(blur(&mut mouse), "o");
        assert_eq!(moved(&mut mouse, 25.0, 45.0, 8.0, 20.0, false), "");
        assert_eq!(left(&mut mouse), "o");
        assert_eq!(focus(&mut mouse), "");
        assert_eq!(click(&mut mouse, 25.0, 45.0, 8.0, 20.0, false), "m 3 2 l");
        assert_eq!(moved(&mut mouse, 25.0, 45.0, 8.0, 20.0, true), "v 3 2", "followed from its next movement");
        // Not seen since the page lost the focus: the focus back says
        // nothing, where the mouse is not known.
        assert_eq!(blur(&mut mouse), "o");
        assert_eq!(focus(&mut mouse), "");
        // Followed before the browser says the focus back, the page having
        // it already: the focus has nothing more to say.
        assert_eq!(blur(&mut mouse), "o");
        assert_eq!(moved(&mut mouse, 17.0, 41.0, 8.0, 20.0, false), "");
        assert_eq!(moved(&mut mouse, 25.0, 45.0, 8.0, 20.0, true), "v 3 2");
        assert_eq!(focus(&mut mouse), "");
    }

    #[test]
    fn the_click_back_in_focus_acts_on_the_cell_seen_whatever_the_order_of_the_events() {
        // The page in the background, the mouse seen over the cell (3, 2),
        // then a click there. Browsers send the click (mousedown) before the
        // focus or after, and a movement may come first, the page having
        // the focus before the browser says so. Whichever comes first, the
        // server is told that the mouse is over the cell, then the click on
        // it.
        let background = || {
            let mut mouse = Mouse::default();
            moved(&mut mouse, 17.0, 41.0, 8.0, 20.0, true);
            blur(&mut mouse);
            moved(&mut mouse, 25.0, 45.0, 8.0, 20.0, false);
            mouse
        };
        let lines = |said: &[String]| said.iter().flat_map(|s| s.lines()).map(str::to_string).collect::<Vec<_>>();
        let mut m = background();
        let focus_first = [focus(&mut m), click(&mut m, 25.0, 45.0, 8.0, 20.0, false)];
        let mut m = background();
        let click_first = [click(&mut m, 25.0, 45.0, 8.0, 20.0, false), focus(&mut m)];
        let mut m = background();
        let moved_first =
            [moved(&mut m, 26.0, 46.0, 8.0, 20.0, true), click(&mut m, 25.0, 45.0, 8.0, 20.0, false), focus(&mut m)];
        let mut m = background();
        let moved_between =
            [focus(&mut m), moved(&mut m, 26.0, 46.0, 8.0, 20.0, true), click(&mut m, 25.0, 45.0, 8.0, 20.0, false)];
        for said in [&focus_first[..], &click_first[..], &moved_first[..], &moved_between[..]] {
            assert_eq!(lines(said), ["v 3 2", "m 3 2 l"], "{said:?}");
        }
        // The click before the focus, and the mouse moving between the two:
        // the page still without the focus, the mouse is not followed, and
        // the focus then says the mouse over the cell where it was last seen.
        let mut m = background();
        assert_eq!(click(&mut m, 25.0, 45.0, 8.0, 20.0, false), "v 3 2\nm 3 2 l");
        assert_eq!(moved(&mut m, 33.0, 45.0, 8.0, 20.0, false), "o");
        assert_eq!(focus(&mut m), "v 4 2");
        // Later clicks act alone, as any click does.
        assert_eq!(click(&mut m, 33.0, 45.0, 8.0, 20.0, false), "m 4 2 l");
    }
}
