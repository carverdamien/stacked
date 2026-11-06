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
//! banner while it is not what you look at; `b LEVEL`, the bell, at LEVEL,
//! from 1 to 100, in percent of its loudest, once for what the editor
//! learned at once, after its notices, or alone when they are not sent, for
//! the page to ring while it is not what you look at, whether it shows the
//! banners or not; either with `test` last (after a US for the notice),
//! pressed on the Configure screen, shown or rung even while the page is
//! looked at; `e PROGRAM`, `$EDITOR` running PROGRAM where
//! `stacked web` was started, or in a window of its own, for the page to
//! say over its screen until the next screen comes; `t`, after a screen's
//! size, the screen that comes the editor's own, run in the page in a
//! terminal of stacked web's, until the next screen's size, which says it
//! is stacked's own again; `v BUILD LAUNCH KEPT`, first each time the page
//! asks for the lines, which stacked sends them: the build of the page and
//! its wasm (BUILD), for a page loaded from another build to load this
//! one's, the launch of stacked (LAUNCH), which the page says again as it
//! asks for the lines once it lost them, and whether that launch keeps its
//! address for the next (KEPT, 1, else 0), which the page may come back to.
//! A page of an older stacked does nothing with a line it does not know,
//! such as a notice.
//!
//! The lines for the server: `k NAME MODS`, a key (`k c HEX MODS` for a
//! character, by its code, without alt where Option made it on a Mac, or
//! without ctrl and alt where AltGr made it elsewhere), MODS `c` ctrl, `a`
//! alt, `s` shift, or `-`, F1 to F12 and Insert among them while the
//! screen is the editor's own, as all else the page has but what goes with
//! ⌘, which stays the browser's;
//! `p HEX`, text pasted while the screen is the editor's, its UTF-8 in hex,
//! as it is, for the editor (else a paste is the keys that type it);
//! `m X Y l|r`, a click on a cell, with the left or the right button, said
//! as the button is pressed; `u X Y`, the left button let go over a cell
//! (`u` over none); `w X Y N`, the wheel turned N lines over a cell, down,
//! or up below 0 (`w N` over none); `v X Y`, the mouse over another cell
//! while the page has the focus, or, when the page gets the focus back,
//! over the cell where the mouse was last seen while the page did not have
//! it; `d X Y` for that, the left button down; `o`, the mouse gone from the
//! screen, or the page losing the focus; `n RUN`, the notice of run RUN
//! clicked, which brought the page forward; `g`, the page going, closed,
//! loaded again or left for another page, which the page's JS sends alone
//! as it is hidden for good, for stacked web to quit once its last page is
//! gone.

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
/// lines what to say over the screen (`editing`); `version` and what it
/// keeps of the stacked serving it (`version`); `bell LEVEL`, for `b
/// LEVEL`, and `bell LEVEL test` for a test (`bell`); `terminal`, for `t`
/// alone; else nothing.
pub fn screen(msg: &str) -> String {
    if let Some(n) = msg.strip_prefix("n ") {
        return notice(n);
    }
    if let Some(program) = msg.strip_prefix("e ") {
        return format!("editing\n{}", editing(program));
    }
    if let Some(v) = msg.strip_prefix("v ") {
        return version(v);
    }
    if let Some(b) = msg.strip_prefix("b ") {
        return bell(b);
    }
    let mut parts = msg.splitn(3, ' ');
    match (parts.next(), parts.next(), parts.next()) {
        (Some("s"), Some(cols), Some(rows)) => format!("size {cols} {rows}"),
        (Some("r"), Some(y), runs) => format!("row {y}\n{}", row_html(runs.unwrap_or(""))),
        (Some("bye"), ..) => "bye".into(),
        (Some("t"), None, _) => "terminal".into(),
        _ => String::new(),
    }
}

/// The bell of the server's, `LEVEL` or `LEVEL test`: `bell LEVEL`, or
/// `bell LEVEL test`, for the page to ring at that level, from 1 to 100.
/// Nothing for a level that is not one, as 0, which never rings, or for
/// anything more.
fn bell(b: &str) -> String {
    let fields: Vec<&str> = b.split(' ').collect();
    let (level, test) = match fields[..] {
        [level] => (level, false),
        [level, "test"] => (level, true),
        _ => return String::new(),
    };
    match level.parse::<u8>() {
        Ok(n @ 1..=100) if level.bytes().all(|c| c.is_ascii_digit()) => format!("bell {n}{}", if test { " test" } else { "" }),
        _ => String::new(),
    }
}

/// A notice of the server's, `RUN US TITLE US BODY`, made what the page
/// shows as the system's banner: its tag, which a banner of the same run
/// takes the place of, as when two pages show it; its title; its body; and
/// the line for the server once it is clicked, `n RUN`, each on a line of
/// its own; then `test`, for a test's (`… US test`), which the page shows
/// even while it is looked at. Nothing for one short of a field, or with
/// one more that is not `test`, or whose run is empty or holds a space,
/// which the line for the server could not say.
fn notice(n: &str) -> String {
    let mut fields = n.split('\u{1f}');
    let (Some(run), Some(title), Some(body)) = (fields.next(), fields.next(), fields.next()) else { return String::new() };
    let test = match (fields.next(), fields.next()) {
        (None, _) => false,
        (Some("test"), None) => true,
        _ => return String::new(),
    };
    if run.is_empty() || run.contains(char::is_whitespace) {
        return String::new();
    }
    let line = |s: &str| s.replace(['\n', '\r'], " ");
    format!("notice\n{run}\n{}\n{}\nn {run}{}", line(title), line(body), if test { "\ntest" } else { "" })
}

/// What the page keeps of the stacked serving it, `BUILD LAUNCH KEPT`:
/// `version BUILD LAUNCH KEPT`, the build of the page and its wasm, the
/// launch of stacked, and whether that launch keeps its address for the
/// next, 1, or not, 0. Nothing for a line short of a field or with one more,
/// or with a build or a launch of more than letters and digits, which the
/// page could not say again in an address.
fn version(v: &str) -> String {
    let fields: Vec<&str> = v.split(' ').collect();
    let [build, launch, kept] = fields[..] else { return String::new() };
    let plain = |f: &str| !f.is_empty() && f.chars().all(|c| c.is_ascii_alphanumeric());
    if !plain(build) || !plain(launch) || !matches!(kept, "0" | "1") {
        return String::new();
    }
    format!("version {build} {launch} {kept}")
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

/// The modifiers held with a key, as the page's browser says them: ctrl,
/// alt (Option, on a Mac), shift, ⌘ (`meta`), and AltGr (`altgr`, as
/// `getModifierState('AltGraph')` says it), which Windows says as ctrl and
/// alt at once, and Firefox on a Mac as Option.
#[derive(Clone, Copy, Default)]
pub struct Held {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
    pub altgr: bool,
}

/// A key the page was pressed, as the browser names it (`key`), on the key
/// of the keyboard `code` names (`KeyD`, whatever the layout), with the
/// modifiers `held`, made a line for the server; none for those not the
/// editor's (with ⌘, the browser's own: copy, paste, its tabs), nor for a
/// dead key (`Dead`, as Option-E is on a Mac before the letter it
/// accents), which is left to the browser: it names the next key by the
/// character the two make. While the screen is the editor's own, run in the
/// page (`terminal`), F1 to F12 and Insert are the editor's too, which are
/// otherwise the browser's. A character goes with ctrl and alt as a
/// terminal takes them, but for one that Option made on a Mac (`mac`): ∂
/// for Option-D, `@` for Option-L on a German keyboard, which goes as it
/// is, without alt, as Terminal.app types it; and for one that AltGr made
/// elsewhere, which goes as it is, without the ctrl and alt Windows says
/// AltGr with. Where Option or AltGr made none, the key typing its own
/// character (`own`: d for Option-D), the modifiers stay as said, as
/// StackED decides it for Option (src/app.rs `typed`). The line is the
/// same whether the screen is stacked's own or the editor's, so that both
/// are given the same characters.
pub fn key(key: &str, code: &str, held: Held, mac: bool, terminal: bool) -> String {
    let Held { ctrl, alt, shift, meta, altgr } = held;
    if meta {
        return String::new();
    }
    let mods = |ctrl: bool, alt: bool, shift: bool| -> String {
        let m: String = [(ctrl, 'c'), (alt, 'a'), (shift, 's')].iter().filter(|(on, _)| *on).map(|(_, m)| *m).collect();
        if m.is_empty() { "-".into() } else { m }
    };
    let function = key.strip_prefix('F').and_then(|n| n.parse::<u8>().ok()).is_some_and(|n| (1..=12).contains(&n));
    if terminal && (function || key == "Insert") {
        return format!("k {key} {}", mods(ctrl, alt, shift));
    }
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
            let (Some(c), None) = (cs.next(), cs.next()) else { return String::new() };
            let made = own(code) != Some(c.to_ascii_lowercase());
            let (ctrl, alt) = match (made, mac, altgr) {
                // Option's, on a Mac: as it is.
                (true, true, _) => (ctrl, false),
                // AltGr's, elsewhere: as it is, without the ctrl and alt
                // Windows says it with.
                (true, false, true) => (false, false),
                _ => (ctrl, alt),
            };
            return format!("k c {:x} {}", c as u32, mods(ctrl, alt, false));
        }
    };
    format!("k {name} {}", mods(ctrl, alt, shift && name != "BackTab"))
}

/// The character the key `code` names types of its own, unshifted, as
/// StackED's `key_char` has it of a key (src/app.rs): a letter's (`KeyD`,
/// d) or a digit's (`Digit1`, 1). None for any other key, whose characters
/// the layout decides, so that what Option or AltGr types on it is taken
/// for theirs. `code` names a key by its place on a US keyboard, whatever
/// the layout (A is `KeyQ` on a French one): there, a letter Option or
/// AltGr left as it is would be taken for theirs too.
fn own(code: &str) -> Option<char> {
    match code.as_bytes() {
        [b'K', b'e', b'y', c] if c.is_ascii_uppercase() => Some(c.to_ascii_lowercase() as char),
        [b'D', b'i', b'g', b'i', b't', c] if c.is_ascii_digit() => Some(*c as char),
        _ => None,
    }
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

/// Text pasted while the screen is the editor's own, run in the page, made
/// the line that gives it the editor as it is, for the server to bracket
/// it if the editor asks: `p HEX`, its UTF-8 in hex. None for no text.
pub fn pasted(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    let hex: String = text.bytes().map(|b| format!("{b:02x}")).collect();
    format!("p {hex}")
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

    /// A key pressed: its text the key's code, a space, then what the
    /// browser names it (a space itself, for the space bar); then the
    /// modifiers held, AltGr last, whether the page is on a Mac, and whether
    /// the screen is the editor's own.
    #[unsafe(no_mangle)]
    pub extern "C" fn key(p: *mut u8, n: usize, ctrl: u32, alt: u32, shift: u32, meta: u32, altgr: u32, mac: u32, terminal: u32) -> usize {
        let text = text(p, n);
        let (code, key) = text.split_once(' ').unwrap_or(("", &text));
        let held = super::Held { ctrl: ctrl != 0, alt: alt != 0, shift: shift != 0, meta: meta != 0, altgr: altgr != 0 };
        give(super::key(key, code, held, mac != 0, terminal != 0))
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

    #[unsafe(no_mangle)]
    pub extern "C" fn pasted(p: *mut u8, n: usize) -> usize {
        give(super::pasted(&text(p, n)))
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
        // A test's: `test` after the line its click sends; any other field
        // more, nothing.
        let n = "n stacked-test\u{1f}stacked\u{1f}This is a test notice from stacked.\u{1f}test";
        assert_eq!(screen(n), "notice\nstacked-test\nstacked\nThis is a test notice from stacked.\nn stacked-test\ntest");
        assert_eq!(screen("n r\u{1f}t\u{1f}b\u{1f}tested"), "");
        assert_eq!(screen("n r\u{1f}t\u{1f}b\u{1f}test\u{1f}test"), "");
    }

    #[test]
    fn the_bell_is_a_line_of_its_own_with_its_level_for_the_page_to_ring() {
        // `b LEVEL`, after notices or with none: the page rings its bell at
        // that level; a test's, even while it is looked at.
        assert_eq!(screen("b 100"), "bell 100");
        assert_eq!(screen("b 1"), "bell 1");
        assert_eq!(screen("b 35 test"), "bell 35 test");
        // The editor gone is still that; `b` alone, at 0, which never
        // rings, past 100, or with anything else, nothing.
        assert_eq!(screen("bye"), "bye");
        for nothing in ["b", "b ", "b 0", "b 101", "b +5", "b loud", "b 5 tested", "b 5 test more", "bell"] {
            assert_eq!(screen(nothing), "", "{nothing}");
        }
    }

    #[test]
    fn the_editor_s_own_screen_is_said_by_a_line_of_its_own_after_its_size() {
        // `t` alone: the screen that comes is the editor's, run in the page;
        // the next size says it is stacked's again.
        assert_eq!(screen("t"), "terminal");
        assert_eq!(screen("s 80 24"), "size 80 24");
        assert_eq!(screen("t 1"), "");
        assert_eq!(screen("t "), "");
        assert_eq!(screen("tt"), "");
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
    fn which_stacked_serves_the_page_is_kept_for_the_page_to_come_back() {
        // Its build, its launch, and whether its address is kept.
        assert_eq!(screen("v 0a1b2c3d4e5f6a7b 9f8e7d 1"), "version 0a1b2c3d4e5f6a7b 9f8e7d 1");
        assert_eq!(screen("v 0a1b2c3d4e5f6a7b 9f8e7d 0"), "version 0a1b2c3d4e5f6a7b 9f8e7d 0");
        // One short of a field, or with one more, an empty one, one the page
        // could not say again in its address, kept neither 1 nor 0: nothing.
        assert_eq!(screen("v 0a1b 9f8e"), "");
        assert_eq!(screen("v 0a1b 9f8e 1 more"), "");
        assert_eq!(screen("v  9f8e 1"), "");
        assert_eq!(screen("v 0a1b 9f&l=x 1"), "");
        assert_eq!(screen("v 0a1b 9f8e yes"), "");
        assert_eq!(screen("v"), "");
    }

    /// The modifiers held, named as a line names them, `c` ctrl, `a` alt
    /// (Option), `s` shift, then `m` ⌘ and `g` AltGr.
    fn held(mods: &str) -> Held {
        let on = |m: char| mods.contains(m);
        Held { ctrl: on('c'), alt: on('a'), shift: on('s'), meta: on('m'), altgr: on('g') }
    }

    #[test]
    fn keys_clicks_wheel_and_pastes_are_lines_for_the_server() {
        // A key pressed elsewhere than on a Mac, as the browser names it,
        // its code and the modifiers held, the screen stacked's own, or the
        // editor's (`t`).
        let k = |name: &str, code: &str, mods: &str| key(name, code, held(mods), false, false);
        let t = |name: &str, code: &str, mods: &str| key(name, code, held(mods), false, true);
        assert_eq!(k("Enter", "Enter", ""), "k Enter -");
        assert_eq!(k("ArrowDown", "ArrowDown", "s"), "k Down s");
        assert_eq!(k("Tab", "Tab", "s"), "k BackTab -");
        assert_eq!(k("?", "Slash", "s"), "k c 3f -", "shifted already");
        assert_eq!(k("c", "KeyC", "c"), "k c 63 c");
        assert_eq!(k(" ", "Space", ""), "k c 20 -");
        assert_eq!(k("é", "Digit2", "a"), "k c e9 a", "alt-é, on a French keyboard");
        assert_eq!(k("v", "KeyV", "m"), "", "⌘V, the browser's");
        assert_eq!(k("Shift", "ShiftLeft", "s"), "");
        // F1 to F12 and Insert: the browser's, but while the screen is the
        // editor's own, run in the page; ⌘ the browser's all the same.
        assert_eq!(k("F5", "F5", ""), "", "the browser's: it loads the page again");
        assert_eq!(k("Insert", "Insert", ""), "");
        assert_eq!(t("F5", "F5", ""), "k F5 -");
        assert_eq!(t("F12", "F12", "cs"), "k F12 cs");
        assert_eq!(t("F1", "F1", "a"), "k F1 a");
        assert_eq!(t("Insert", "Insert", "s"), "k Insert s");
        assert_eq!(t("F13", "F13", ""), "");
        assert_eq!(t("F5", "F5", "m"), "", "⌘ the browser's");
        assert_eq!(t("ArrowUp", "ArrowUp", "c"), "k Up c", "the others as ever");
        assert_eq!(t("c", "KeyC", "c"), "k c 63 c", "ctrl-c the editor's");
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
        // While the screen is the editor's own: the text as it is, its
        // UTF-8 in hex, for the server to bracket it.
        assert_eq!(pasted("a\r\nb\t"), "p 610d0a6209");
        assert_eq!(pasted("日"), "p e697a5");
        assert_eq!(pasted(""), "");
    }

    #[test]
    fn a_character_option_makes_on_a_mac_is_typed_as_it_is_alt_kept_only_where_it_makes_none() {
        // Each key pressed, as the browser names it, its code and the
        // modifiers held, then its line on a Mac and elsewhere, the screen
        // stacked's own or the editor's.
        let cases = [
            // Option-D typing ∂: Option's character, as it is; elsewhere,
            // alt with it, as terminals take it.
            ("∂", "KeyD", "a", "k c 2202 -", "k c 2202 a"),
            // Option-D typing d, Option making none there: alt-d.
            ("d", "KeyD", "a", "k c 64 a", "k c 64 a"),
            // Option-Shift-D: Î, shifted already, as it is; D, where Option
            // makes none, alt-D.
            ("Î", "KeyD", "as", "k c ce -", "k c ce a"),
            ("D", "KeyD", "as", "k c 44 a", "k c 44 a"),
            // On a French keyboard, Option-5 types { and Option-Shift-5 [;
            // on a German one, Option-5 types [ and Option-L @.
            ("{", "Digit5", "a", "k c 7b -", "k c 7b a"),
            ("[", "Digit5", "as", "k c 5b -", "k c 5b a"),
            ("[", "Digit5", "a", "k c 5b -", "k c 5b a"),
            ("@", "KeyL", "a", "k c 40 -", "k c 40 a"),
            // On any, Option-; types … and Option-Shift-2 €, and
            // Option-Space a no-break space: a character typed with Option
            // on a key other than a letter or a digit is Option's.
            ("…", "Semicolon", "a", "k c 2026 -", "k c 2026 a"),
            ("€", "Digit2", "as", "k c 20ac -", "k c 20ac a"),
            ("\u{a0}", "Space", "a", "k c a0 -", "k c a0 a"),
            // With ctrl too, ctrl stays as the browser says it.
            ("∂", "KeyD", "ca", "k c 2202 c", "k c 2202 ca"),
            ("d", "KeyD", "ca", "k c 64 ca", "k c 64 ca"),
            // A dead key, Option-E: nothing, left to the browser; then E,
            // typing é, the character the two make, Option let go or held.
            ("Dead", "KeyE", "a", "", ""),
            ("é", "KeyE", "", "k c e9 -", "k c e9 -"),
            ("é", "KeyE", "a", "k c e9 -", "k c e9 a"),
            // Option with a key that is no character: alt, as ever, which
            // xterm's forms say for the editor, and Option-← and Option-→
            // stacked's word moves.
            ("ArrowLeft", "ArrowLeft", "a", "k Left a", "k Left a"),
            ("ArrowRight", "ArrowRight", "a", "k Right a", "k Right a"),
            ("ArrowUp", "ArrowUp", "as", "k Up as", "k Up as"),
            ("Backspace", "Backspace", "a", "k Backspace a", "k Backspace a"),
            ("Enter", "Enter", "a", "k Enter a", "k Enter a"),
        ];
        for (name, code, mods, on_a_mac, elsewhere) in cases {
            for terminal in [false, true] {
                assert_eq!(key(name, code, held(mods), true, terminal), on_a_mac, "{name} {code} {mods} on a Mac");
                assert_eq!(key(name, code, held(mods), false, terminal), elsewhere, "{name} {code} {mods} elsewhere");
            }
        }
    }

    #[test]
    fn a_character_altgr_makes_is_typed_as_it_is_without_ctrl_and_alt_and_alt_is_alt_elsewhere_than_on_a_mac() {
        // Each key pressed elsewhere than on a Mac, then its line, the
        // screen stacked's own or the editor's.
        let cases = [
            // AltGr-Q typing @ on a German keyboard, said with ctrl and alt
            // as Windows says it, and alone as Linux does; AltGr-E typing
            // €; AltGr-Shift-D typing Ð, shifted already: as it is.
            ("@", "KeyQ", "cag", "k c 40 -"),
            ("@", "KeyQ", "g", "k c 40 -"),
            ("€", "KeyE", "cag", "k c 20ac -"),
            ("Ð", "KeyD", "gs", "k c d0 -"),
            // AltGr making none, the key's own typed, as when Firefox on
            // Windows says ctrl-alt-d with AltGr: ctrl and alt as said.
            ("d", "KeyD", "cag", "k c 64 ca"),
            ("d", "KeyD", "g", "k c 64 -"),
            // Alt, and ctrl and alt, without AltGr: as they are.
            ("f", "KeyF", "a", "k c 66 a"),
            ("d", "KeyD", "ca", "k c 64 ca"),
            // A dead key with AltGr: nothing; AltGr with a key that is no
            // character, as the browser says it.
            ("Dead", "BracketRight", "cag", ""),
            ("ArrowLeft", "ArrowLeft", "cag", "k Left ca"),
        ];
        for (name, code, mods, line) in cases {
            for terminal in [false, true] {
                assert_eq!(key(name, code, held(mods), false, terminal), line, "{name} {code} {mods}");
            }
        }
        // On a Mac, where Firefox says AltGr with Option, Option decides:
        // its character as it is, alt where it made none.
        assert_eq!(key("@", "KeyL", held("ag"), true, false), "k c 40 -");
        assert_eq!(key("d", "KeyD", held("ag"), true, false), "k c 64 a");
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
