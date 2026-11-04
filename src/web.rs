//! `stacked web`: the editor in a browser. A server on this machine runs
//! the editor as the terminal does, its screen drawn into a buffer of the
//! browser's size; the browser is sent the rows that changed, and sends
//! back its keys and clicks. It is sent too, unless the settings' `[web]`
//! say otherwise (`notifications`), the notices of what the editor learns
//! of every stack (`tui::notices`), which the page shows as the system's
//! banners while it is not what you look at. Its half is wasm
//! (web/client.rs, built by the build script), carried by a little JS
//! (web/index.html).
//!
//! Only this machine's pages, holding the token the browser was opened
//! with, may see it or drive it: a page from elsewhere could otherwise send
//! it keys. What runs `$EDITOR` runs it where `stacked web` was started; the
//! pages are told meanwhile, and say so over their screen.
//!
//! It listens on the port `--port` names, else on the one the settings'
//! `[web]` say (`listen`); the token is new each time, whatever the port.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::Color;
use unicode_width::UnicodeWidthStr;

use crate::config::{Port, Web, program};
use crate::db::{Error, Result};
use crate::tui::{App, Effect, Notice};

const PAGE: &str = include_str!("../web/index.html");
const CLIENT: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/client.wasm"));

/// Where the cursor is, in a run's modifiers: as web/client.rs reads it.
const CURSOR: u32 = 1 << 16;

/// What comes to the editor from the browsers.
enum Input {
    /// A browser come, by its number among those `Coming` holds until this
    /// is read, and its screen, `cols` by `rows`.
    Join(usize, u16, u16),
    /// A line it sent: a key, a click, its size.
    Line(String),
}

/// What the editor's thread shares with the threads that answer the
/// browsers' events: the browsers come whose join it has not read yet, each
/// by its number, with where it is sent the rows; the number of the next;
/// and, while `$EDITOR` runs, the line that says so. The editor's thread
/// reads nothing until `$EDITOR` exits: a browser that comes meanwhile is
/// told at once, and one come just before, which the editor's thread does
/// not know yet, is told as it starts.
#[derive(Default)]
struct Coming {
    browsers: Vec<(usize, Sender<String>)>,
    next: usize,
    editing: Option<String>,
}

/// `coming`, locked: as it was left, should a thread holding it have
/// panicked.
fn lock(coming: &Mutex<Coming>) -> MutexGuard<'_, Coming> {
    coming.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Browser `id`, come, taken out of those `coming` holds: where it is sent
/// the rows, the editor's alone from now on.
fn came(coming: &Mutex<Coming>, id: usize) -> Option<Sender<String>> {
    let mut c = lock(coming);
    let at = c.browsers.iter().position(|(i, _)| *i == id)?;
    Some(c.browsers.remove(at).1)
}

/// How many requests the server is answering, the browsers' events among
/// them, each counted from when it comes (`Answering::one`) until it is
/// answered: `stacked web` ends once none is, so that each has what it
/// was sent, and its answer.
#[derive(Clone, Default)]
struct Answering(Arc<AtomicUsize>);

/// A request counted among those being answered, while it is.
struct Answered(Arc<AtomicUsize>);

impl Answering {
    /// A request come: counted until what this gives goes.
    fn one(&self) -> Answered {
        self.0.fetch_add(1, Ordering::SeqCst);
        Answered(self.0.clone())
    }

    /// Waits for every request to be answered, as long as `TELLING` at most:
    /// for a page that reads nothing, or a request that never ends.
    fn wait(&self) {
        let started = Instant::now();
        while self.0.load(Ordering::SeqCst) > 0 && started.elapsed() < TELLING {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

impl Drop for Answered {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// How long the editor left waits for the requests it is answering.
const TELLING: Duration = Duration::from_secs(5);

/// The server, listening: its port, and the token its pages need.
pub struct Bound {
    listener: TcpListener,
    pub port: u16,
    pub token: String,
}

/// Listens on `port` of 127.0.0.1, any free one for 0. A port in use is
/// said so, in a line naming it, and no other is tried.
pub fn bind(port: u16) -> Result<Bound> {
    let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|e| match e.kind() {
        std::io::ErrorKind::AddrInUse if port != 0 => Error(format!("port {port} of 127.0.0.1 is in use")),
        _ => Error(format!("cannot listen on 127.0.0.1:{port}: {e}")),
    })?;
    let port = listener.local_addr().map_err(|e| Error(e.to_string()))?.port();
    Ok(Bound { listener, port, token: token() })
}

/// Listens as `stacked web` does: on `given`, the port `--port` names, as
/// it is, any free one for 0; else as the settings' `[web]` say, `web`: any
/// free port for `"random"`; for `"fixed-random"`, the port kept, else any
/// free one, then kept, written as `web.kept_port` in the settings' file at
/// `settings` for the launches after; or the port `web.port` names. A port
/// in use is said, and no other is tried. With the server, why the port
/// taken could not be kept, if it could not: it is listened on all the
/// same, and the next launch takes any free one again.
pub fn listen(given: Option<u16>, web: &Web, settings: &Path) -> Result<(Bound, Option<String>)> {
    let port = match (given, web.port, web.kept_port) {
        (Some(p), ..) | (None, Port::Fixed(p), _) | (None, Port::FixedRandom, Some(p)) => p,
        (None, Port::Random, _) => 0,
        (None, Port::FixedRandom, None) => {
            let bound = bind(0)?;
            let unkept = crate::config::set(settings, "web.kept_port", &[bound.port.to_string()]).err().map(|e| {
                format!(
                    "port {} cannot be kept in the settings as web.kept_port, so the next launch with web.port = \"fixed-random\" takes any free port again: {e}",
                    bound.port
                )
            });
            return Ok((bound, unkept));
        }
    };
    Ok((bind(port)?, None))
}

impl Bound {
    /// The page to open, the token in it.
    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}/?t={}", self.port, self.token)
    }

    /// Serves the editor on the database at `path` until it is quit there.
    pub fn serve(self, path: &Path) -> Result<()> {
        let (tx, rx) = mpsc::channel();
        let Bound { listener, port, token } = self;
        let answering = Answering::default();
        let counting = answering.clone();
        let coming = Arc::new(Mutex::new(Coming::default()));
        let shared = coming.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let (token, tx, coming, answered) = (token.clone(), tx.clone(), shared.clone(), counting.one());
                std::thread::spawn(move || {
                    let _ = handle(stream, &token, port, &tx, &coming);
                    drop(answered);
                });
            }
        });
        run(path, rx, &coming, &answering)
    }
}

/// Opens the browser on `url`, as stacked opens a web link
/// (`refs::open_url`: `$STACKED_OPEN`'s program, else as this system opens
/// one), away from the server, which serves meanwhile: what went wrong
/// said on stderr.
pub fn open_browser(url: &str) {
    let url = url.to_string();
    std::thread::spawn(move || {
        if let Err(why) = crate::refs::open_url(&url) {
            eprintln!("stacked: {why}");
        }
    });
}

/// Sixteen random bytes, in hex.
fn token() -> String {
    let mut bytes = [0u8; 16];
    let read = std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut bytes));
    if read.is_err() {
        // No /dev/urandom: the time, this process, an address.
        let seed = format!("{:?} {} {:p}", std::time::SystemTime::now(), std::process::id(), &bytes);
        bytes.copy_from_slice(&sha1_smol::Sha1::from(seed).digest().bytes()[..16]);
    }
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Set by SIGINT, ctrl-c where `stacked web` was started: the editor is
/// left as Quit leaves it, but for a ctrl-c while `$EDITOR` ran there,
/// which was the editor's.
static INTERRUPTED: AtomicBool = AtomicBool::new(false);

/// Set by SIGTERM (a kill) and SIGHUP (that terminal closed): the editor is
/// left as Quit leaves it, once back from what it does.
static TERMINATED: AtomicBool = AtomicBool::new(false);

extern "C" fn on_signal(sig: libc::c_int) {
    // A store, which a signal's handler may safely do, and nothing else.
    match sig {
        libc::SIGINT => INTERRUPTED.store(true, Ordering::SeqCst),
        _ => TERMINATED.store(true, Ordering::SeqCst),
    }
}

/// SIGINT, SIGTERM and SIGHUP, from now on, leave the editor served as Quit
/// does: for `stacked web` to call, as the process is its own.
pub fn catch_signals() {
    let handler = on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t;
    for sig in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
        unsafe { libc::signal(sig, handler) };
    }
}

/// The editor left, as the terminal's is once quit: the browsers told,
/// those `coming` holds too, the agents' worktrees it made ahead removed,
/// the asks still running said. Then what is `answering` is waited for, for
/// each browser to be told and the request that quit to be answered:
/// `stacked web` ends with this, and what is not written by then never is.
/// A page not told, its events cut, would try them again and again, never
/// saying that the editor is gone.
fn leave(app: &App, browsers: Vec<Sender<String>>, coming: &Mutex<Coming>, answering: &Answering) {
    // Each sent its last, then nothing more: its events end once written.
    let came = std::mem::take(&mut lock(coming).browsers);
    for b in browsers.into_iter().chain(came.into_iter().map(|(_, b)| b)) {
        let _ = b.send("bye".into());
    }
    app.leave();
    app.say_still_running();
    answering.wait();
}

/// The editor, as the terminal runs it: what the browsers send done, its
/// screen drawn after, the rows that changed sent them, after the notices
/// of what it learned meanwhile; while `$EDITOR` runs, the browsers told so
/// first, those `coming` holds too, and sent the screen whole after; until
/// it is left, once what it is `answering` is answered.
fn run(path: &Path, rx: Receiver<Input>, coming: &Mutex<Coming>, answering: &Answering) -> Result<()> {
    let mut app = App::new(path)?;
    // What it learns, told the browsers, unless the settings say not to.
    if app.cfg.web.notifications {
        app.keep_notices();
    }
    // Opened: on StackED's File → Open Recent too.
    crate::state::opened_in_editor(path);
    app.launch();
    let mut browsers: Vec<Sender<String>> = Vec::new();
    let mut size = (100, 30);
    let mut shown: Vec<String> = Vec::new();
    let mut launched = Instant::now();
    loop {
        let first = match rx.recv_timeout(Duration::from_millis(250)) {
            Ok(i) => Some(i),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => {
                leave(&app, browsers, coming, answering);
                return Ok(());
            }
        };
        // Ctrl-c, a kill, the terminal closed: left as Quit leaves it.
        if INTERRUPTED.load(Ordering::SeqCst) || TERMINATED.load(Ordering::SeqCst) {
            leave(&app, browsers, coming, answering);
            return Ok(());
        }
        // All that came meanwhile, before it is drawn: a paste is many keys.
        let mut whole = false;
        for input in first.into_iter().chain(std::iter::from_fn(|| rx.try_recv().ok())) {
            match input {
                Input::Join(id, cols, rows) => {
                    let Some(tx) = came(coming, id) else { continue };
                    // What the editor learns of the database as the browser
                    // comes was there before it: read first, and its notices
                    // told the browsers there before alone, so that a page
                    // is told only of what lands once it is there.
                    app.refresh();
                    let told: Vec<String> = app.take_notices().iter().map(notice).collect();
                    tell(&mut browsers, &told);
                    browsers.push(tx);
                    size = fitted(cols, rows);
                    whole = true;
                }
                Input::Line(line) => {
                    let was = size;
                    match act(&mut app, &line, &mut size) {
                        Effect::None => {}
                        Effect::Quit => {
                            leave(&app, browsers, coming, answering);
                            return Ok(());
                        }
                        // $EDITOR, where `stacked web` was started: the
                        // browsers told first, the program it runs named,
                        // those come whose join is not read yet among them,
                        // as nothing is read until it exits; then sent the
                        // screen after, whole, changed or not, which takes
                        // the place of what they say over it, whether it ran
                        // or not. A ctrl-c there meanwhile was the editor's.
                        e => {
                            let said = editing(&program(&crate::config::get().editor()));
                            {
                                let mut c = lock(coming);
                                for (_, b) in &c.browsers {
                                    let _ = b.send(said.clone());
                                }
                                c.editing = Some(said.clone());
                            }
                            tell(&mut browsers, &[said]);
                            if let Err(e) = app.effect(e) {
                                app.status = e.0;
                            }
                            lock(coming).editing = None;
                            INTERRUPTED.store(false, Ordering::SeqCst);
                            whole = true;
                        }
                    }
                    whole |= size != was;
                }
            }
        }
        app.refresh();
        if launched.elapsed() > Duration::from_secs(5) {
            app.launch();
            launched = Instant::now();
        }
        let mut buf = Buffer::empty(Rect::new(0, 0, size.0, size.1));
        let cursor = app.draw(&mut buf);
        let now = rows(&buf, cursor);
        let whole = whole || now.len() != shown.len();
        // The notices first: a page sent the screen that says what landed
        // was sent its notice.
        let mut sent: Vec<String> = app.take_notices().iter().map(notice).collect();
        if whole {
            sent.push(format!("s {} {}", size.0, size.1));
        }
        for (y, row) in now.iter().enumerate() {
            if whole || shown.get(y) != Some(row) {
                sent.push(row.clone());
            }
        }
        tell(&mut browsers, &sent);
        shown = now;
    }
}

/// A screen of `cols` by `rows`, within what the editor draws well.
fn fitted(cols: u16, rows: u16) -> (u16, u16) {
    (cols.clamp(40, 400), rows.clamp(10, 200))
}

/// `lines` sent to each of the browsers, in order: those gone, whose events
/// ended, forgotten.
fn tell(browsers: &mut Vec<Sender<String>>, lines: &[String]) {
    browsers.retain(|b| lines.iter().all(|m| b.send(m.clone()).is_ok()));
}

/// `text` with its control characters spaces, as a row's text has them: in
/// a line for a browser, no new line, nor US.
fn plain(text: &str) -> String {
    text.chars().map(|c| if c.is_control() { ' ' } else { c }).collect()
}

/// Notice `n` as web/client.rs reads it: `n RUN US TITLE US BODY`, the run
/// it is of, its title and its body, each `plain`.
fn notice(n: &Notice) -> String {
    format!("n {}\u{1f}{}\u{1f}{}", plain(&n.run), plain(&n.title), plain(&n.body))
}

/// The line that tells a browser `$EDITOR` runs `program`, as web/client.rs
/// reads it: `e PROGRAM`, the program `plain`.
fn editing(program: &str) -> String {
    format!("e {}", plain(program))
}

/// What a line a browser sent does: a key pressed (`k NAME MODS`, `k c HEX
/// MODS`), a click on a cell (`m X Y l|r`), the left button pressed for
/// it, which HEAD's row holds for a drag (`App::down`), the wheel turned N
/// lines over a cell (`w X Y N`, down, up below 0; `w N` over none), the
/// mouse over another (`v X Y`), over another with the left button down
/// (`d X Y`), or gone from the screen (`o`), the left button let go over a
/// cell (`u X Y`; `u` over none), a new size (`s COLS ROWS`), the notice of
/// run RUN clicked, the page brought forward (`n RUN`, `App::noticed`).
fn act(app: &mut App, line: &str, size: &mut (u16, u16)) -> Effect {
    let parts: Vec<&str> = line.split_whitespace().collect();
    match parts.as_slice() {
        ["k", rest @ ..] => match key(rest) {
            Some(k) => app.key(k),
            None => Effect::None,
        },
        ["m", x, y, button] => match (x.parse(), y.parse()) {
            (Ok(x), Ok(y)) if *button == "r" => app.click(x, y, true),
            (Ok(x), Ok(y)) => app.down(x, y),
            _ => Effect::None,
        },
        ["d", x, y] => {
            if let (Ok(x), Ok(y)) = (x.parse(), y.parse()) {
                app.drag(x, y);
            }
            Effect::None
        }
        ["u", x, y] => match (x.parse(), y.parse()) {
            (Ok(x), Ok(y)) => app.up(Some((x, y))),
            _ => Effect::None,
        },
        ["u"] => app.up(None),
        ["w", x, y, n] => match (x.parse(), y.parse(), n.parse()) {
            (Ok(x), Ok(y), Ok(n)) => app.wheel(Some((x, y)), crate::tui::Turned::Lines(n)),
            _ => Effect::None,
        },
        ["w", n] => match n.parse() {
            Ok(n) => app.wheel(None, crate::tui::Turned::Lines(n)),
            Err(_) => Effect::None,
        },
        ["v", x, y] => {
            if let (Ok(x), Ok(y)) = (x.parse(), y.parse()) {
                app.hover(x, y);
            }
            Effect::None
        }
        ["o"] => {
            app.unhover();
            Effect::None
        }
        ["s", cols, rows] => {
            if let (Ok(c), Ok(r)) = (cols.parse(), rows.parse()) {
                *size = fitted(c, r);
            }
            Effect::None
        }
        ["n", run] => {
            app.noticed(run);
            Effect::None
        }
        _ => Effect::None,
    }
}

/// A key as a browser's line names it.
fn key(parts: &[&str]) -> Option<KeyEvent> {
    let (code, mods) = match parts {
        ["c", hex, mods] => (KeyCode::Char(char::from_u32(u32::from_str_radix(hex, 16).ok()?)?), *mods),
        [name, mods] => (
            match *name {
                "Enter" => KeyCode::Enter,
                "Esc" => KeyCode::Esc,
                "Backspace" => KeyCode::Backspace,
                "Delete" => KeyCode::Delete,
                "Tab" => KeyCode::Tab,
                "BackTab" => KeyCode::BackTab,
                "Up" => KeyCode::Up,
                "Down" => KeyCode::Down,
                "Left" => KeyCode::Left,
                "Right" => KeyCode::Right,
                "PageUp" => KeyCode::PageUp,
                "PageDown" => KeyCode::PageDown,
                "Home" => KeyCode::Home,
                "End" => KeyCode::End,
                _ => return None,
            },
            *mods,
        ),
        _ => return None,
    };
    let mut m = KeyModifiers::NONE;
    for (c, k) in [('c', KeyModifiers::CONTROL), ('a', KeyModifiers::ALT), ('s', KeyModifiers::SHIFT)] {
        if mods.contains(c) {
            m |= k;
        }
    }
    Some(KeyEvent::new(code, m))
}

/// A colour as web/client.rs reads it: none for the page's own, 0 to 15,
/// or #rrggbb.
fn colour(c: Color) -> String {
    let named = match c {
        Color::Reset => return String::new(),
        Color::Black => 0,
        Color::Red => 1,
        Color::Green => 2,
        Color::Yellow => 3,
        Color::Blue => 4,
        Color::Magenta => 5,
        Color::Cyan => 6,
        Color::Gray => 7,
        Color::DarkGray => 8,
        Color::LightRed => 9,
        Color::LightGreen => 10,
        Color::LightYellow => 11,
        Color::LightBlue => 12,
        Color::LightMagenta => 13,
        Color::LightCyan => 14,
        Color::White => 15,
        Color::Rgb(r, g, b) => return format!("#{r:02x}{g:02x}{b:02x}"),
        Color::Indexed(i) if i < 16 => i,
        // xterm's 256: a cube of six levels, then grays.
        Color::Indexed(i) if i < 232 => {
            let level = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            let i = i - 16;
            return format!("#{:02x}{:02x}{:02x}", level(i / 36), level(i / 6 % 6), level(i % 6));
        }
        Color::Indexed(i) => {
            let g = 8 + (i - 232) * 10;
            return format!("#{g:02x}{g:02x}{g:02x}");
        }
    };
    named.to_string()
}

/// The screen drawn, as rows of runs for web/client.rs: `r Y RUNS`, each
/// run its style, US, its text, RS between them. A wide character's cell
/// covers the next; the cursor is marked on its cell.
pub fn rows(buf: &Buffer, cursor: Option<(u16, u16)>) -> Vec<String> {
    let area = buf.area;
    (0..area.height)
        .map(|y| {
            let mut runs: Vec<(String, String)> = Vec::new();
            let mut x = 0;
            while x < area.width {
                let (cx, cy) = (area.x + x, area.y + y);
                let cell = &buf[(cx, cy)];
                let sym = cell.symbol();
                let mut mods = cell.modifier.bits() as u32;
                if cursor == Some((cx, cy)) {
                    mods |= CURSOR;
                }
                let style = format!("{},{},{mods}", colour(cell.fg), colour(cell.bg));
                let text: String = if sym.is_empty() {
                    " ".into()
                } else {
                    sym.chars().map(|c| if c.is_control() { ' ' } else { c }).collect()
                };
                match runs.last_mut() {
                    Some((s, t)) if *s == style => t.push_str(&text),
                    _ => runs.push((style, text)),
                }
                x += sym.width().max(1) as u16;
            }
            let runs: Vec<String> = runs.into_iter().map(|(s, t)| format!("{s}\u{1f}{t}")).collect();
            format!("r {y} {}", runs.join("\u{1e}"))
        })
        .collect()
}

/// StackED's icon for the page's tab, as web/index.html links it: PNGs of
/// 32 pixels square, and of 64 for a screen of high density. Both drawn
/// once, the first time a page asks for either.
fn icons() -> &'static [Vec<u8>; 2] {
    static DRAWN: std::sync::OnceLock<[Vec<u8>; 2]> = std::sync::OnceLock::new();
    DRAWN.get_or_init(|| [32, 64].map(crate::icon::png))
}

/// One request: the page, the wasm, StackED's icon for the page's tab, the
/// rows as they change (server-sent events), or a browser's lines. Only
/// from a page of this machine's, holding the token, the icon too: the page
/// links it with the token, and `/favicon.ico`, which a browser may ask for
/// on its own, without it, is refused as all else is. A browser's events
/// start among those `coming` holds, told first if `$EDITOR` runs.
fn handle(stream: TcpStream, token: &str, port: u16, tx: &Sender<Input>, coming: &Mutex<Coming>) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut out = stream;
    let mut first = String::new();
    reader.read_line(&mut first)?;
    let mut words = first.split_whitespace();
    let (method, target) = (words.next().unwrap_or(""), words.next().unwrap_or(""));
    let (mut length, mut host) = (0usize, String::new());
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header)? == 0 {
            break;
        }
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        if let Some((k, v)) = header.split_once(':') {
            match k.trim().to_ascii_lowercase().as_str() {
                "content-length" => length = v.trim().parse().unwrap_or(0),
                "host" => host = v.trim().to_string(),
                _ => {}
            }
        }
    }
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let param = |k: &str| query.split('&').find_map(|kv| kv.strip_prefix(k)?.strip_prefix('='));
    let ours = host == format!("127.0.0.1:{port}") || host == format!("localhost:{port}");
    if !ours || param("t") != Some(token) {
        return reply(&mut out, "403 Forbidden", "text/plain", b"not for this page");
    }
    match (method, path) {
        ("GET", "/") => reply(&mut out, "200 OK", "text/html; charset=utf-8", PAGE.as_bytes()),
        ("GET", "/client.wasm") => reply(&mut out, "200 OK", "application/wasm", CLIENT),
        ("GET", "/icon-32.png") => reply(&mut out, "200 OK", "image/png", &icons()[0]),
        ("GET", "/icon-64.png") => reply(&mut out, "200 OK", "image/png", &icons()[1]),
        ("GET", "/events") => {
            let n = |k: &str| param(k).and_then(|v| v.parse().ok()).unwrap_or(0);
            let (rows_tx, rows) = mpsc::channel();
            // Come, under the lock that `$EDITOR` starts under: told first
            // if it runs, else told by the editor's thread if it starts
            // before it reads this join.
            let id = {
                let mut c = lock(coming);
                if let Some(said) = &c.editing {
                    let _ = rows_tx.send(said.clone());
                }
                let id = c.next;
                c.next += 1;
                c.browsers.push((id, rows_tx));
                id
            };
            let _ = tx.send(Input::Join(id, n("cols"), n("rows")));
            out.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-store\r\n\r\n")?;
            out.flush()?;
            loop {
                match rows.recv_timeout(Duration::from_secs(15)) {
                    Ok(m) => {
                        write!(out, "data: {m}\n\n")?;
                        while let Ok(m) = rows.try_recv() {
                            write!(out, "data: {m}\n\n")?;
                        }
                    }
                    // Still there? A closed page is found out writing.
                    Err(RecvTimeoutError::Timeout) => out.write_all(b": still here\n\n")?,
                    Err(RecvTimeoutError::Disconnected) => return Ok(()),
                }
                out.flush()?;
            }
        }
        ("POST", "/input") => {
            let mut body = vec![0; length.min(1 << 20)];
            reader.read_exact(&mut body)?;
            for line in String::from_utf8_lossy(&body).lines() {
                let _ = tx.send(Input::Line(line.to_string()));
            }
            reply(&mut out, "204 No Content", "text/plain", b"")
        }
        _ => reply(&mut out, "404 Not Found", "text/plain", b"nothing here"),
    }
}

fn reply(out: &mut TcpStream, status: &str, kind: &str, body: &[u8]) -> std::io::Result<()> {
    write!(
        out,
        "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    out.write_all(body)?;
    out.flush()
}

/// For the tests: serves the database at `path` on a free port, away from
/// the caller: where, and the token.
pub fn start(path: PathBuf) -> Result<(u16, String)> {
    let bound = bind(0)?;
    let (port, token) = (bound.port, bound.token.clone());
    std::thread::spawn(move || bound.serve(&path));
    Ok((port, token))
}

#[cfg(test)]
#[path = "../web/client.rs"]
mod client;

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::{Modifier, Style};

    #[test]
    fn the_screen_is_rows_of_runs_the_client_makes_html() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 8, 2));
        buf.set_string(0, 0, "ab", Style::new().fg(Color::Green).add_modifier(Modifier::BOLD));
        buf.set_string(2, 0, "日x", Style::new());
        buf.set_string(0, 1, "<", Style::new().bg(Color::Rgb(16, 32, 48)));
        let rows = rows(&buf, Some((1, 1)));
        // The wide character covers its next cell: the row is 8 cells.
        assert_eq!(rows[0], "r 0 2,,1\u{1f}ab\u{1e},,0\u{1f}日x   ");
        assert_eq!(rows[1], "r 1 ,#102030,0\u{1f}<\u{1e},,65536\u{1f} \u{1e},,0\u{1f}      ");
        let html = client::screen(&rows[1]);
        assert!(html.starts_with("row 1\n<span style=\"color:var(--fg);background:#102030\">&lt;</span><span class=\"cur\""), "{html}");
        assert_eq!(colour(Color::Indexed(196)), "#ff0000");
        assert_eq!(colour(Color::Indexed(244)), "#808080");
    }

    #[test]
    fn a_browsers_lines_are_keys_clicks_and_sizes() {
        assert_eq!(key(&["Enter", "-"]), Some(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)));
        assert_eq!(key(&["c", "3f", "-"]), Some(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE)));
        assert_eq!(key(&["c", "63", "c"]), Some(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)));
        assert_eq!(key(&["Down", "s"]), Some(KeyEvent::new(KeyCode::Down, KeyModifiers::SHIFT)));
        assert_eq!(key(&["F1", "-"]), None);
        assert_eq!(key(&["c", "zz", "-"]), None);
        // As the client makes them.
        let line = client::key("ArrowUp", false, false, false, false);
        assert_eq!(key(&line.split_whitespace().skip(1).collect::<Vec<_>>()), Some(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)));
        assert_eq!(fitted(5, 5000), (40, 200));
    }
}
