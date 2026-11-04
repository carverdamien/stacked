//! `stacked web`: the editor in a browser. A server on this machine runs
//! the editor as the terminal does, its screen drawn into a buffer of the
//! browser's size; the browser is sent the rows that changed, and sends
//! back its keys and clicks. It is sent too, unless the settings' `[web]`
//! say otherwise (`notifications`), the notices of what the editor learns
//! of every stack (`tui::notices`), which the page shows as the system's
//! banners while it is not what you look at; and, unless they say
//! otherwise (`bell`), a line after those learned at once, banners or not,
//! for the page to ring its bell then. Its half is wasm
//! (web/client.rs, built by the build script), carried by a little JS
//! (web/index.html).
//!
//! Only this machine's pages, holding the token the browser was opened
//! with, may see it or drive it: a page from elsewhere could otherwise send
//! it keys. A terminal `$EDITOR` runs in the page, unless the settings'
//! `[web]` say otherwise (`editor`): in a terminal of stacked web's own
//! (`pty`), whose screen the pages are sent as the editor's own is, and
//! whose keys, pastes and mouse are what they send (`in_page`). Else, and
//! for an editor with a window of its own, it runs where `stacked web` was
//! started; the pages are told meanwhile, and say so over their screen, and
//! what they do meanwhile is dropped, as StackED drops what is done in its
//! window.
//!
//! It listens on the port `--port` names, else on the one the settings'
//! `[web]` say (`listen`). The token is new each time, but with a port the
//! settings fix: there it is kept for the next launch, in a file of
//! stacked's own beside its state (`home::Home::web_token`), and that
//! launch listens with it, and gives the tab already open on it `WAIT` to
//! come back before it takes a new one and opens the browser (`welcome`).
//! A page that loses the server says so over its screen, and the browser
//! asks for it again each second, at the same address with the same token,
//! until it answers, or refuses the token (web/index.html). A page closed
//! says so; once the last has, and none came back within `WAIT`, the
//! editor is left as Quit leaves it (`all_gone`).

use std::cell::RefCell;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::Color;
use serde::{Deserialize, Serialize};
use unicode_width::UnicodeWidthStr;

use crate::config::{Port, Web, WebEditor, own_window, program};
use crate::db::{Error, Result};
use crate::pty::{Pty, Screen};
use crate::tui::{App, Effect, Notice};

const PAGE: &str = include_str!("../web/index.html");
const CLIENT: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/client.wasm"));

/// Where the cursor is, in a run's modifiers: as web/client.rs reads it.
const CURSOR: u32 = 1 << 16;

/// What comes to the editor from the browsers.
enum Input {
    /// A browser come, by its number among those `Coming` holds until this
    /// is read, its screen, `cols` by `rows`, and whether it comes back from
    /// another launch, its tab open since then (`running_again`).
    Join(usize, u16, u16, bool),
    /// A line it sent: a key, a click, its size.
    Line(String),
    /// What the editor run in the page wrote in its terminal, by the number
    /// of that terminal, as its reader read it: None once there is no more
    /// (`in_page`).
    Output(usize, Option<Vec<u8>>),
}

/// What the editor's thread shares with the threads that answer the
/// browsers: the browsers come whose join it has not read yet, each by its
/// number, with where it is sent the rows; the number of the next; and,
/// from when the browsers are told that `$EDITOR` runs where `stacked web`
/// was started, or in a window of its own, to when they are sent the screen
/// after it, the line that tells them. The editor's thread reads nothing
/// until `$EDITOR` exits then: a browser that comes meanwhile is told at
/// once, and one come just before, which the editor's thread does not know
/// yet, is told as it starts. And what the browsers send meanwhile is
/// dropped as it is read, but a new size (`kept_while_editing`). An editor
/// run in the page is none of that: the editor's thread reads what comes
/// as ever, for the editor (`in_page`).
///
/// Under the same lock, what the threads answering the browsers learn, for
/// the launch and the editor's thread to act on: the token a request needs,
/// and, while the launch waits for the tab of the launch before, whether a
/// page came with it, each changed under the lock by the one or the other,
/// so that a page is either come back or refused (`came_back`); the
/// browsers' events open, and when a page last said it goes, by which the
/// editor's thread knows its last page gone (`all_gone`); and whether the
/// editor is left.
#[derive(Default)]
struct Coming {
    browsers: Vec<(usize, Sender<String>)>,
    next: usize,
    editing: Option<String>,
    /// The token each request needs: the launch's own, else the one the
    /// launch before kept, until its tab is found gone (`came_back`).
    token: String,
    /// The launch waits for the tab opened with the token the launch before
    /// kept (`Bound::kept`): any request of a page's with it, but its lines,
    /// is that tab come back, `back`.
    waiting: bool,
    back: bool,
    /// The browsers' events open now, each counted from when it comes until
    /// its page closes it, or it is ended here (`watch`); and whether one
    /// ever was.
    open: usize,
    ever: bool,
    /// When a page last said it goes (`going`), unless a browser's events
    /// came since.
    going: Option<Instant>,
    /// The editor is left (`leave`): a browser's events that come after are
    /// told so at once, rather than kept waiting for what never comes.
    left: bool,
}

/// `coming`, locked: as it was left, should a thread holding it have
/// panicked.
fn lock(coming: &Mutex<Coming>) -> MutexGuard<'_, Coming> {
    coming.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Whether `line`, read from a browser while `$EDITOR` runs where `stacked
/// web` was started, or in a window of its own, is done all the same, once
/// it exits: a new size (`s COLS ROWS`), so that the screen
/// sent after fits the page as it is then. All else is dropped, as StackED
/// drops what is done in its window meanwhile: keys, clicks, the wheel,
/// drags, the mouse going over the screen or leaving it, pastes, a notice
/// clicked.
fn kept_while_editing(line: &str) -> bool {
    line.split_whitespace().next() == Some("s")
}

/// Whether `line`, read from a browser, says its page goes (`g`): closed,
/// loaded again, or left for another page, as the page says once it is
/// hidden for good (web/index.html). Counted as it comes, while `$EDITOR`
/// runs too, and never the editor's to read (`all_gone`).
fn going(line: &str) -> bool {
    line.split_whitespace().eq(["g"])
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

/// How long a launch with the token the launch before kept waits for the
/// tab opened with it to come back, before it takes a new token and opens
/// the browser there (`came_back`); and how long, once the last page said
/// it goes, the editor waits for a page to come, as a page loaded again
/// does, before it is left (`all_gone`).
const WAIT: Duration = Duration::from_secs(2);

/// How long after a page loses the server the browser asks for it again,
/// and again after that, as each page is told as its events start.
const RETRY: Duration = Duration::from_secs(1);

/// The server, listening: its port, the token its pages need, and, with a
/// port the settings fix, where that token is kept for the next launch, and
/// whether it is the one the launch before kept there for this port.
pub struct Bound {
    listener: TcpListener,
    pub port: u16,
    pub token: String,
    keep: Option<PathBuf>,
    kept: bool,
}

/// Listens on `port` of 127.0.0.1, any free one for 0, with a new token,
/// kept nowhere. A port in use is said so, in a line naming it, and no
/// other is tried.
pub fn bind(port: u16) -> Result<Bound> {
    let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|e| match e.kind() {
        std::io::ErrorKind::AddrInUse if port != 0 => Error(format!("port {port} of 127.0.0.1 is in use")),
        _ => Error(format!("cannot listen on 127.0.0.1:{port}: {e}")),
    })?;
    let port = listener.local_addr().map_err(|e| Error(e.to_string()))?.port();
    Ok(Bound { listener, port, token: token(), keep: None, kept: false })
}

/// Listens as `stacked web` does: on `given`, the port `--port` names, as
/// it is, any free one for 0; else as the settings' `[web]` say, `web`: any
/// free port for `"random"`; for `"fixed-random"`, the port kept, else any
/// free one, then kept, written as `web.kept_port` in the settings' file at
/// `settings` for the launches after; or the port `web.port` names. A port
/// in use is said, and no other is tried. With the server, why the port
/// taken could not be kept, if it could not: it is listened on all the
/// same, and the next launch takes any free one again.
///
/// With a port the settings fix, `"fixed-random"` or a number, the token is
/// the one the launch before kept for that port in the file at `tokens`, if
/// it kept one there (`Bound::kept`), else a new one, kept there as the
/// launch serves; with a port given, or `"random"`, a new one, and the file
/// is neither read nor written, a token kept there staying as it was.
pub fn listen(given: Option<u16>, web: &Web, settings: &Path, tokens: &Path) -> Result<(Bound, Option<String>)> {
    let (mut bound, unkept) = match (given, web.port, web.kept_port) {
        (Some(p), ..) | (None, Port::Fixed(p), _) | (None, Port::FixedRandom, Some(p)) => (bind(p)?, None),
        (None, Port::Random, _) => (bind(0)?, None),
        (None, Port::FixedRandom, None) => {
            let bound = bind(0)?;
            let unkept = crate::config::set(settings, "web.kept_port", &[bound.port.to_string()]).err().map(|e| {
                format!(
                    "port {} cannot be kept in the settings as web.kept_port, so the next launch with web.port = \"fixed-random\" takes any free port again: {e}",
                    bound.port
                )
            });
            (bound, unkept)
        }
    };
    if given.is_none() && web.port != Port::Random {
        if let Some(token) = kept_token(tokens, bound.port) {
            bound.token = token;
            bound.kept = true;
        }
        bound.keep = Some(tokens.to_path_buf());
    }
    Ok((bound, unkept))
}

/// The address a launch keeps for the next, its port fixed by the
/// settings: the port, and the token a page opened on it holds. In a file
/// only you may read, beside stacked's state (`home::Home::web_token`).
#[derive(Serialize, Deserialize)]
struct Kept {
    port: u16,
    token: String,
}

/// The token the launch before kept in `file` for `port`: none if there is
/// no file, or anything but a file there (the null device), it cannot be
/// read, it keeps another port, or what it keeps is not a token, as `token`
/// makes them.
fn kept_token(file: &Path, port: u16) -> Option<String> {
    if !std::fs::metadata(file).ok()?.is_file() {
        return None;
    }
    let kept: Kept = serde_json::from_slice(&std::fs::read(file).ok()?).ok()?;
    let a_token = !kept.token.is_empty() && kept.token.chars().all(|c| c.is_ascii_hexdigit());
    (kept.port == port && a_token).then_some(kept.token)
}

/// `token` kept in `file` as the token of `port`, for the next launch: in a
/// file that only you may read or write, made so, written whole beside it,
/// then renamed over it, so that it is never seen half written, nor by
/// another. Nowhere (the null device, or anything but a file there) keeps
/// nothing.
fn keep_token(file: &Path, port: u16, token: &str) -> std::io::Result<()> {
    if std::fs::metadata(file).is_ok_and(|m| !m.is_file()) {
        return Ok(());
    }
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = serde_json::to_string(&Kept { port, token: token.into() }).map_err(std::io::Error::other)? + "\n";
    let mut part = file.as_os_str().to_owned();
    part.push(format!(".{}", std::process::id()));
    let part = PathBuf::from(part);
    // One a process of this number left before, as it died writing it.
    let _ = std::fs::remove_file(&part);
    let written = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&part)
        .and_then(|mut f| {
            // 0600 whatever the umask, which could take more away.
            f.set_permissions(std::fs::Permissions::from_mode(0o600))?;
            f.write_all(text.as_bytes())
        })
        .and_then(|()| std::fs::rename(&part, file));
    if written.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    written
}

/// What the threads answering the browsers know of the launch, as it is
/// until it ends: its port, which a request's host names; which launch it
/// is, which a browser's events start with, and which the browser says
/// again as it asks for them again; and whether it keeps its address for
/// the next launch, which a page may come back to.
struct Serving {
    port: u16,
    launch: String,
    kept: bool,
}

impl Serving {
    /// The line each browser's events start with, as web/client.rs reads
    /// it: `v BUILD LAUNCH KEPT`, the build of the page and its wasm
    /// (`build`), this launch, and 1 if it keeps its address for the next,
    /// else 0.
    fn version(&self) -> String {
        format!("v {} {} {}", build(), self.launch, u8::from(self.kept))
    }
}

/// The page and its wasm, named by a hash of both: a page that comes back
/// to a stacked whose build is not its own loads that stacked's page
/// (web/index.html), as what the server sends may differ.
fn build() -> &'static str {
    static BUILT: OnceLock<String> = OnceLock::new();
    BUILT.get_or_init(|| {
        let mut hash = sha1_smol::Sha1::new();
        hash.update(PAGE.as_bytes());
        hash.update(CLIENT);
        hash.digest().to_string()[..16].to_string()
    })
}

impl Bound {
    /// Serves the editor on the database at `path` until it is quit there,
    /// or its last page is closed (`all_gone`): where it is said in the
    /// terminal `stacked web` was started from, and the browser opened there
    /// if `open`, once the launch knows which address that is (`welcome`).
    pub fn serve(self, path: &Path, open: bool) -> Result<()> {
        self.serving(path, Some(open))
    }

    /// Serves the editor on the database at `path`: as `serve` does, the
    /// browser opened or not as `launch` says; with none, saying nothing and
    /// opening nothing, as the tests' servers do (`start`).
    fn serving(self, path: &Path, launch: Option<bool>) -> Result<()> {
        let (tx, rx) = mpsc::channel();
        // For the reader of an editor's terminal in the page (`in_page`).
        let output = tx.clone();
        let Bound { listener, port, token: first, keep, kept } = self;
        let serving = Arc::new(Serving { port, launch: token(), kept: keep.is_some() });
        let waiting = kept && launch.is_some();
        let coming = Arc::new(Mutex::new(Coming { token: first, waiting, ..Coming::default() }));
        let answering = Answering::default();
        let counting = answering.clone();
        let (shared, on) = (coming.clone(), serving.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let (serving, tx, coming, answered) = (on.clone(), tx.clone(), shared.clone(), counting.one());
                std::thread::spawn(move || {
                    let _ = handle(stream, &serving, &tx, &coming);
                    drop(answered);
                });
            }
        });
        if let Some(open) = launch {
            welcome(&coming, port, keep, kept, open);
        }
        run(path, rx, output, &coming, &answering)
    }
}

/// What a launch says as it serves, in the terminal it was started from,
/// and what it opens: with a token of its own, kept for the next launch if
/// the settings fix its port, where the editor is, at once, and the browser
/// opened there if `open`; with the token the launch before kept, that the
/// tab opened with it has `WAIT` to come back, waited for away from the
/// editor, which serves meanwhile (`came_back`).
fn welcome(coming: &Arc<Mutex<Coming>>, port: u16, keep: Option<PathBuf>, kept: bool, open: bool) {
    if kept {
        let then = if open { "else a new tab is opened on a new address" } else { "else the address is given a new token" };
        say(&format!("a tab already open on port {port} has {} seconds to come back, {then}", WAIT.as_secs()));
        let (coming, started) = (coming.clone(), Instant::now());
        std::thread::spawn(move || came_back(&coming, port, keep, open, started));
        return;
    }
    let token = lock(coming).token.clone();
    if let Some(file) = keep {
        kept_or_said(&file, port, &token);
    }
    at(port, &token, open);
}

/// The tab opened with the token the launch before kept, given `WAIT` from
/// `started` to come back, a request of a page's with that token but its
/// lines: come, said so, with the address in use; else the token renewed,
/// under the lock the requests are checked under, so that a page with the
/// old one is refused from then on, kept for the next launch, and said,
/// with where the editor is now, the browser opened there if `open`.
/// Nothing once the editor is left meanwhile.
fn came_back(coming: &Mutex<Coming>, port: u16, keep: Option<PathBuf>, open: bool, started: Instant) {
    let (back, token) = loop {
        {
            let mut c = lock(coming);
            if c.left {
                return;
            }
            if c.back || started.elapsed() >= WAIT {
                c.waiting = false;
                if !c.back {
                    c.token = token();
                }
                break (c.back, c.token.clone());
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    if back {
        say("the tab already open came back");
        at(port, &token, false);
        return;
    }
    say(&format!("no tab came back within {} seconds, so the address has a new token, and a tab with the old one is refused", WAIT.as_secs()));
    if let Some(file) = keep {
        kept_or_said(&file, port, &token);
    }
    at(port, &token, open);
}

/// Where the editor is now, on `port` with `token`, said, and the browser
/// opened there if `open`.
fn at(port: u16, token: &str, open: bool) {
    let url = format!("http://127.0.0.1:{port}/?t={token}");
    say(&format!("the editor is at {url} · quit it there, or ctrl-c here"));
    if open {
        open_browser(&url);
    }
}

/// `token` kept in `file` for `port`, for the next launch; else said why
/// not, without the token.
fn kept_or_said(file: &Path, port: u16, token: &str) {
    if let Err(e) = keep_token(file, port, token) {
        say(&format!("the token of this address cannot be kept in {}, so the next launch opens a new tab: {e}", file.display()));
    }
}

/// `said` in the terminal `stacked web` was started from, after
/// `stacked: `: not with eprintln!, which panics with nowhere to say it, as
/// once that terminal closed.
fn say(said: &str) {
    let _ = writeln!(std::io::stderr().lock(), "stacked: {said}");
}

/// Opens the browser on `url`, as stacked opens a web link
/// (`refs::open_url`: `$STACKED_OPEN`'s program, else as this system opens
/// one), away from the server, which serves meanwhile: what went wrong
/// said on stderr.
pub fn open_browser(url: &str) {
    let url = url.to_string();
    std::thread::spawn(move || {
        if let Err(why) = crate::refs::open_url(&url) {
            say(&why);
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
/// which was the editor's. An editor run in the page, which hears none of
/// that terminal's keys, is hung up on first (`in_page`).
static INTERRUPTED: AtomicBool = AtomicBool::new(false);

/// Set by SIGTERM (a kill) and SIGHUP (that terminal closed): the editor is
/// left as Quit leaves it, once back from what it does; an editor run in
/// the page hung up on first.
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

/// Whether `stacked web` is to go, as a ctrl-c where it was started, a kill
/// or that terminal closing says.
fn quitting() -> bool {
    INTERRUPTED.load(Ordering::SeqCst) || TERMINATED.load(Ordering::SeqCst)
}

/// Said as `stacked web` quits once its last tab is gone (`all_gone`).
fn last_tab_closed() {
    say(&format!(
        "the last tab of the editor was closed, and no tab came back within {} seconds, so stacked web quits",
        WAIT.as_secs()
    ));
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
    // Those that come after are told so at once (`handle`).
    let came = {
        let mut c = lock(coming);
        c.left = true;
        std::mem::take(&mut c.browsers)
    };
    for b in browsers.into_iter().chain(came.into_iter().map(|(_, b)| b)) {
        let _ = b.send("bye".into());
    }
    app.leave();
    app.say_still_running();
    answering.wait();
}

/// Whether the editor's last page is gone: one said it goes (`going`) at
/// least `WAIT` ago, no browser's events came since (a page loaded again
/// asks for them), none is open, and one was, as a launch whose tab never
/// came is not quit so. A page cut off without saying it goes, as when the
/// machine sleeps or the browser puts its tab aside, quits nothing: it
/// comes back. Once `WAIT` is past, what the page said is forgotten, so
/// that a page cut off later quits nothing either.
fn all_gone(coming: &Mutex<Coming>) -> bool {
    let mut c = lock(coming);
    match c.going {
        Some(at) if at.elapsed() >= WAIT => {
            c.going = None;
            c.open == 0 && c.ever
        }
        _ => false,
    }
}

/// What a tab come back from another launch is told, as a notice of the
/// editor's, for the page to show as the system's banner while it is not
/// what you look at; a click on it switches to the tab. Of no run, so that
/// the click does nothing more (`App::noticed`).
fn running_again() -> Notice {
    Notice {
        run: "stacked-web".into(),
        title: "stacked".into(),
        body: "stacked web is running again: click to switch to this tab.".into(),
    }
}

/// The editor, as the terminal runs it: what the browsers send done, its
/// screen drawn after, the rows that changed sent them, after the notices
/// of what it learned meanwhile and their bell, as the settings say
/// (`told`); while a terminal `$EDITOR` runs in the page, its screen sent
/// them instead, what they send its own (`in_page`), the browsers and the
/// screen's size lent it meanwhile, and the screen sent whole after; while
/// it runs where `stacked web` was started, or in a window of its own, the
/// browsers told so first, those `coming` holds too, what they send
/// dropped, and sent the screen whole after; until it is left, as it is
/// quit, or as its last page is gone (`all_gone`), once what it is
/// `answering` is answered. Its terminal's reader in the page writes to
/// `output`, among what the browsers send.
fn run(path: &Path, rx: Receiver<Input>, output: Sender<Input>, coming: &Arc<Mutex<Coming>>, answering: &Answering) -> Result<()> {
    let mut app = App::new(path)?;
    // What it learns, told the browsers, for their banners, their bell or
    // both, unless the settings say neither.
    let web = app.cfg.web.clone();
    if web.notifications || web.bell {
        app.keep_notices();
    }
    // Opened: on StackED's File → Open Recent too.
    crate::state::opened_in_editor(path);
    app.launch();
    let mut browsers: Vec<Sender<String>> = Vec::new();
    let mut size = (100, 30);
    let mut shown: Vec<String> = Vec::new();
    let mut launched = Instant::now();
    // What the editor in the page reads and is lent while it runs.
    let rx = Rc::new(rx);
    let lent = Rc::new(RefCell::new(Lent::default()));
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
        if quitting() {
            leave(&app, browsers, coming, answering);
            return Ok(());
        }
        // Its last tab closed, and none come back since: left so too.
        if all_gone(coming) {
            last_tab_closed();
            leave(&app, browsers, coming, answering);
            return Ok(());
        }
        // All that came meanwhile, before it is drawn: a paste is many keys.
        // And whether `$EDITOR` ran, the screen after it then to send whole.
        let mut whole = false;
        let mut ran = false;
        for input in first.into_iter().chain(std::iter::from_fn(|| rx.try_recv().ok())) {
            match input {
                Input::Join(id, cols, rows, again) => {
                    let Some(tx) = came(coming, id) else { continue };
                    // What the editor learns of the database as the browser
                    // comes was there before it: read first, and its notices
                    // told the browsers there before alone, so that a page
                    // is told only of what lands once it is there.
                    app.refresh();
                    tell(&mut browsers, &told(&app.take_notices(), &web));
                    // A tab come back from another launch: told stacked web
                    // runs again, for the banner it shows while it is not
                    // what you look at, as the notices are, unless the
                    // settings say not to; with no bell, as it tells
                    // nothing the editor learned.
                    if again && web.notifications {
                        let _ = tx.send(notice(&running_again()));
                    }
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
                        // $EDITOR, a terminal's, in the page, unless the
                        // settings say otherwise: lent the browsers and the
                        // screen's size while it runs (`in_page`), which it
                        // gives back once it exited, each time it runs for
                        // what is edited, and the browsers that came
                        // meanwhile; those, as browsers that come, told
                        // none of what the editor learned before. Then the
                        // screen after, whole, sent all of them, the bottom
                        // line saying what came of it. `stacked web` gone
                        // meanwhile, the editor hung up on: left so.
                        e if web.editor == WebEditor::Page && !own_window(&crate::config::get().editor()) => {
                            *lent.borrow_mut() = Lent { browsers: std::mem::take(&mut browsers), size, ..Lent::default() };
                            let terminal: crate::ops::Terminal = {
                                let (lent, rx, output, coming, web) = (lent.clone(), rx.clone(), output.clone(), coming.clone(), web.clone());
                                Box::new(move |cmd: &mut Command| in_page(cmd, &lent, &rx, &output, &coming, &web))
                            };
                            crate::ops::own_terminal(Some(terminal));
                            let done = app.effect(e);
                            crate::ops::own_terminal(None);
                            let back = std::mem::take(&mut *lent.borrow_mut());
                            (browsers, size) = (back.browsers, back.size);
                            if let Err(e) = done {
                                app.status = e.0;
                            }
                            if back.gone {
                                last_tab_closed();
                            }
                            if back.gone || quitting() {
                                browsers.extend(back.meanwhile);
                                leave(&app, browsers, coming, answering);
                                return Ok(());
                            }
                            app.refresh();
                            tell(&mut browsers, &told(&app.take_notices(), &web));
                            browsers.extend(back.meanwhile);
                            whole = true;
                            ran = true;
                        }
                        // $EDITOR, where `stacked web` was started, or in a
                        // window of its own: the browsers told first, the
                        // program it runs named, those come whose join is
                        // not read yet among them, as nothing is read until
                        // it exits; from then on, what they send is dropped
                        // as it is read, until they are sent the screen
                        // after, whole, changed or not, which takes the
                        // place of what they say over it, whether it ran or
                        // not. A ctrl-c there meanwhile was the editor's.
                        e => {
                            let said = editing(&program(&crate::config::get().editor()));
                            {
                                let mut c = lock(coming);
                                for (_, b) in &c.browsers {
                                    let _ = b.send(said.clone());
                                }
                                tell(&mut browsers, std::slice::from_ref(&said));
                                c.editing = Some(said);
                            }
                            if let Err(e) = app.effect(e) {
                                app.status = e.0;
                            }
                            INTERRUPTED.store(false, Ordering::SeqCst);
                            whole = true;
                            ran = true;
                        }
                    }
                    whole |= size != was;
                }
                // What an editor run in the page wrote once it exited, read
                // late: nothing to show.
                Input::Output(..) => {}
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
        // The notices first, and their bell: a page sent the screen that
        // says what landed was sent its notice, and told to ring.
        let mut sent = told(&app.take_notices(), &web);
        if whole {
            sent.push(format!("s {} {}", size.0, size.1));
        }
        for (y, row) in now.iter().enumerate() {
            if whole || shown.get(y) != Some(row) {
                sent.push(row.clone());
            }
        }
        if ran {
            // The screen after `$EDITOR`, sent under the lock its end is
            // said under: what a browser sends once it has that screen is
            // done, what it sent before that is dropped.
            let mut c = lock(coming);
            tell(&mut browsers, &sent);
            c.editing = None;
        } else {
            tell(&mut browsers, &sent);
        }
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

/// What the editor's thread lends the editor run in the page while it runs
/// (`in_page`), and gets back once it exited: the browsers, and those come
/// meanwhile, kept apart; the screen's size, the last a page sent, fitted;
/// and whether its last page went meanwhile (`all_gone`), `stacked web`
/// then to quit.
#[derive(Default)]
struct Lent {
    browsers: Vec<Sender<String>>,
    meanwhile: Vec<Sender<String>>,
    size: (u16, u16),
    gone: bool,
}

/// How often at most the editor run in the page has its screen sent the
/// browsers, the rows that changed since: about 60 times a second.
const FRAME: Duration = Duration::from_millis(16);

/// How long the editor's thread waits for what comes while the editor runs
/// in the page, before it looks again whether the editor exited, or
/// `stacked web` is to go.
const LOOK: Duration = Duration::from_millis(50);

/// How long it waits at most, while what the editor was written waits for
/// its terminal to take it, before it writes it again.
const TAKING: Duration = Duration::from_millis(10);

/// How long the editor run in the page may write nothing before its screen
/// is sent all the same, blank, so that the page shows whose screen it is.
const QUIET: Duration = Duration::from_millis(250);

/// The line that says to a browser that the screen sent with it, after its
/// size, is the editor's own, run in the page: every key the page has goes
/// to the editor then, F1 to F12 and Insert too, and a paste as it is,
/// until the next screen's size, which says it is stacked's own again, as
/// web/client.rs reads it.
const TERMINAL: &str = "t";

/// The terminals made so far for an editor run in the page, each by its
/// number: what one wrote, read late, is not taken for what the next one
/// writes.
static TERMINALS: AtomicUsize = AtomicUsize::new(0);

/// The editor run in the page: `cmd`, as `ops::run_editor` builds it, run
/// in a terminal of stacked web's own (`pty::Pty`), of the screen's size,
/// the last a page sent; its screen sent the browsers it is lent as
/// stacked's own is (`rows`), the rows that changed, at most every `FRAME`,
/// the first time whole, after `TERMINAL`, as soon as the editor writes, or
/// once it wrote nothing for `QUIET`; what they send made the bytes xterm
/// sends for it (`pty::key`, `pty::paste`, `pty::Screen::mouse`), or a new
/// size its terminal's; a browser that comes sent its screen whole at once,
/// after the notice of a tab come back, as the settings' `web` say. How it
/// exited, once it did. Else an error: the terminal not made, or the editor
/// not started, said so; or `stacked web` to go (a ctrl-c, a kill, its
/// terminal closed, its last page gone), the editor hung up on as its
/// terminal goes. Its terminal's reader writes to `output`, read from `rx`
/// with what the browsers send; their joins taken from `coming`.
fn in_page(cmd: &mut Command, lent: &RefCell<Lent>, rx: &Receiver<Input>, output: &Sender<Input>, coming: &Mutex<Coming>, web: &Web) -> std::io::Result<ExitStatus> {
    // Gone already, as an edit that asks the editor again may after one
    // hung up on: not started.
    if quitting() || lent.borrow().gone {
        return Err(std::io::Error::other("stacked web quits, so it was not started; nothing changed"));
    }
    let (cols, height) = lent.borrow().size;
    let number = TERMINALS.fetch_add(1, Ordering::SeqCst);
    let mut screen = Screen::new(cols, height);
    let out = output.clone();
    let mut pty = Pty::start(cmd, cols, height, move |o| {
        let _ = out.send(Input::Output(number, o));
    })?;
    // The editor's rows as the browsers have them, and their size: none
    // sent yet.
    let mut shown: Option<((u16, u16), Vec<String>)> = None;
    // Whether its screen changed since, and whether it is to be sent whole.
    let (mut changed, mut whole) = (false, true);
    let started = Instant::now();
    let mut sent: Option<Instant> = None;
    loop {
        let due = match (changed, sent) {
            (true, Some(at)) => FRAME.saturating_sub(at.elapsed()),
            (true, None) => Duration::ZERO,
            (false, _) if shown.is_none() => QUIET.saturating_sub(started.elapsed()),
            _ => LOOK,
        };
        let first = rx.recv_timeout(due.min(if pty.waiting() { TAKING } else { LOOK })).ok();
        // All that came meanwhile, a frame's time of it at most, so that an
        // editor that writes without end holds up nothing else.
        let batch = Instant::now();
        let more = std::iter::from_fn(|| if batch.elapsed() < FRAME { rx.try_recv().ok() } else { None });
        for input in first.into_iter().chain(more) {
            match input {
                Input::Output(of, Some(bytes)) if of == number => {
                    let answers = screen.process(&bytes);
                    pty.write(&answers);
                    changed = true;
                }
                // The end of what it writes, which its exit says; or what
                // the terminal of an editor before wrote, read late.
                Input::Output(..) => {}
                Input::Join(id, c, r, again) => {
                    let Some(page) = came(coming, id) else { continue };
                    if again && web.notifications {
                        let _ = page.send(notice(&running_again()));
                    }
                    // Its screen whole, as the browsers there have it; none
                    // sent yet, the first goes whole to all.
                    if let Some((at, drawn)) = &shown {
                        for line in whole_screen(*at, drawn) {
                            let _ = page.send(line);
                        }
                    }
                    lent.borrow_mut().meanwhile.push(page);
                    if sized(fitted(c, r), lent, &mut screen, &pty) {
                        (changed, whole) = (true, true);
                    }
                }
                Input::Line(line) => {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    match parts.as_slice() {
                        ["s", c, r] => {
                            if let (Ok(c), Ok(r)) = (c.parse(), r.parse())
                                && sized(fitted(c, r), lent, &mut screen, &pty)
                            {
                                (changed, whole) = (true, true);
                            }
                        }
                        ["k", rest @ ..] => {
                            if let Some(bytes) = crate::pty::key(rest, screen.application_cursor()) {
                                pty.write(&bytes);
                            }
                        }
                        ["p", hex] => {
                            if let Some(text) = unhex(hex) {
                                pty.write(&crate::pty::paste(&text, screen.bracketed_paste()));
                            }
                        }
                        ["m" | "u" | "d" | "v" | "w", ..] => {
                            let bytes = screen.mouse(&parts);
                            pty.write(&bytes);
                        }
                        // The mouse gone from the screen, a notice clicked:
                        // nothing, while the editor runs.
                        _ => {}
                    }
                }
            }
        }
        pty.flush();
        // `stacked web` to go: the editor hung up on, as `pty` goes.
        if quitting() {
            return Err(std::io::Error::other("stacked web quit, so it was hung up on; nothing changed"));
        }
        if all_gone(coming) {
            lent.borrow_mut().gone = true;
            return Err(std::io::Error::other("the last tab of the editor was closed, so it was hung up on; nothing changed"));
        }
        if let Some(status) = pty.exited()? {
            return Ok(status);
        }
        let first_due = shown.is_none() && started.elapsed() >= QUIET;
        if (changed && sent.is_none_or(|at| at.elapsed() >= FRAME)) || first_due {
            let (buf, cursor) = screen.buffer();
            let now = rows(&buf, cursor);
            let size = lent.borrow().size;
            let lines: Vec<String> = match &shown {
                Some((at, drawn)) if !whole && *at == size && drawn.len() == now.len() => {
                    now.iter().zip(drawn).filter(|(n, d)| n != d).map(|(n, _)| n.clone()).collect()
                }
                _ => whole_screen(size, &now),
            };
            let mut l = lent.borrow_mut();
            let Lent { browsers, meanwhile, .. } = &mut *l;
            tell(browsers, &lines);
            tell(meanwhile, &lines);
            shown = Some((size, now));
            (changed, whole, sent) = (false, false, Some(Instant::now()));
        }
    }
}

/// The screen of the editor run in the page whole, its rows `drawn`, of
/// `size`, as a browser is sent it: its size, then `TERMINAL`, then each
/// row.
fn whole_screen((cols, height): (u16, u16), drawn: &[String]) -> Vec<String> {
    let mut lines = vec![format!("s {cols} {height}"), TERMINAL.to_string()];
    lines.extend(drawn.iter().cloned());
    lines
}

/// The screen of the editor run in the page and its terminal made `to`, a
/// size a page sent, fitted, which the system tells the editor, unless they
/// are of that size already: whether they were made so.
fn sized(to: (u16, u16), lent: &RefCell<Lent>, screen: &mut Screen, pty: &Pty) -> bool {
    let mut l = lent.borrow_mut();
    if l.size == to {
        return false;
    }
    l.size = to;
    screen.set_size(to.0, to.1);
    let _ = pty.resize(to.0, to.1);
    true
}

/// The text of a paste's line, `p HEX` (web/client.rs `pasted`): its UTF-8
/// in hex, two digits a byte. None for what is not that.
fn unhex(hex: &str) -> Option<String> {
    if !hex.len().is_multiple_of(2) || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let bytes: Vec<u8> = (0..hex.len()).step_by(2).filter_map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok()).collect();
    Some(String::from_utf8_lossy(&bytes).into_owned())
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

/// The line that tells a browser to ring its bell, as web/client.rs reads
/// it: `b`, once for the notices learned at once.
const BELL: &str = "b";

/// What the browsers are told of `learned`, the notices the editor learned
/// at once, as the settings' `[web]` say (`web`): each notice's line, for
/// its banner, unless `notifications` is false; then the bell's, once for
/// them all, unless `bell` is false. Nothing for no notice.
fn told(learned: &[Notice], web: &Web) -> Vec<String> {
    let mut lines: Vec<String> = if web.notifications { learned.iter().map(notice).collect() } else { Vec::new() };
    if web.bell && !learned.is_empty() {
        lines.push(BELL.into());
    }
    lines
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
/// on its own, without it, is refused as all else is. While the launch
/// waits for the tab of the launch before, a request of a page's with the
/// token, but its lines, is that tab come back, under the lock the token is
/// renewed under. A browser's events start among those `coming` holds,
/// told first which stacked serves them, then if `$EDITOR` runs where
/// `stacked web` was started, or in a window of its own, and are counted
/// open until their page closes them (`watch`); once the editor is left,
/// they are told so at once. A browser's lines are dropped as they are read
/// while `$EDITOR` runs so, but a new size; and its page going is counted,
/// never read by the editor. An editor run in the page has them all, as
/// the editor's thread reads them (`in_page`).
fn handle(stream: TcpStream, serving: &Serving, tx: &Sender<Input>, coming: &Mutex<Coming>) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut out = stream;
    let mut first = String::new();
    reader.read_line(&mut first)?;
    let mut words = first.split_whitespace();
    let (method, target) = (words.next().unwrap_or(""), words.next().unwrap_or(""));
    let (mut length, mut host, mut last) = (0usize, String::new(), String::new());
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
                // The launch the browser had its events from, as it asks for
                // them again (`write_events`).
                "last-event-id" => last = v.trim().to_string(),
                _ => {}
            }
        }
    }
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let param = |k: &str| query.split('&').find_map(|kv| kv.strip_prefix(k)?.strip_prefix('='));
    let port = serving.port;
    let ours = host == format!("127.0.0.1:{port}") || host == format!("localhost:{port}");
    let allowed = ours && {
        let mut c = lock(coming);
        let fine = param("t") == Some(c.token.as_str());
        if fine && c.waiting && method == "GET" {
            c.back = true;
        }
        fine
    };
    if !allowed {
        return reply(&mut out, "403 Forbidden", "text/plain", b"not for this page");
    }
    match (method, path) {
        ("GET", "/") => reply(&mut out, "200 OK", "text/html; charset=utf-8", PAGE.as_bytes()),
        ("GET", "/client.wasm") => reply(&mut out, "200 OK", "application/wasm", CLIENT),
        ("GET", "/icon-32.png") => reply(&mut out, "200 OK", "image/png", &icons()[0]),
        ("GET", "/icon-64.png") => reply(&mut out, "200 OK", "image/png", &icons()[1]),
        ("GET", "/events") => {
            let n = |k: &str| param(k).and_then(|v| v.parse().ok()).unwrap_or(0);
            // From another launch: the one the page had its events from, as
            // the browser says it asking for them again, else as the page
            // says it, loaded again since (`l`).
            let had = Some(last.as_str()).filter(|l| !l.is_empty()).or_else(|| param("l"));
            let again = had.is_some_and(|l| !l.is_empty() && l != serving.launch);
            let (rows_tx, rows) = mpsc::channel();
            // Come, under the lock that `$EDITOR` starts under, and that the
            // editor is left under: told first which stacked serves it, then
            // if `$EDITOR` runs, else told by the editor's thread if it
            // starts before it reads this join, and counted open; or, the
            // editor left, told so, and not come.
            let id = {
                let mut c = lock(coming);
                let _ = rows_tx.send(serving.version());
                if c.left {
                    let _ = rows_tx.send("bye".into());
                    drop(rows_tx);
                    None
                } else {
                    if let Some(said) = &c.editing {
                        let _ = rows_tx.send(said.clone());
                    }
                    let id = c.next;
                    c.next += 1;
                    c.browsers.push((id, rows_tx));
                    c.open += 1;
                    c.ever = true;
                    c.going = None;
                    Some(id)
                }
            };
            if let Some(id) = id {
                let _ = tx.send(Input::Join(id, n("cols"), n("rows"), again));
            }
            // Read until the page closes them, as they are written: counted
            // open no longer as soon as it does, whatever is written then.
            std::thread::scope(|s| {
                if id.is_some() {
                    s.spawn(|| watch(&mut reader, coming));
                }
                let written = write_events(&mut out, &rows, &serving.launch);
                // Ended, for the page and for `watch`.
                let _ = out.shutdown(Shutdown::Both);
                written
            })
        }
        ("POST", "/input") => {
            let mut body = vec![0; length.min(1 << 20)];
            reader.read_exact(&mut body)?;
            // Read under the lock that `$EDITOR`'s run starts and ends
            // under: what comes while it runs dropped, but a new size; what
            // comes before or after it done. A page going, counted whenever
            // it comes (`all_gone`).
            let mut c = lock(coming);
            for line in String::from_utf8_lossy(&body).lines() {
                if going(line) {
                    c.going = Some(Instant::now());
                } else if c.editing.is_none() || kept_while_editing(line) {
                    let _ = tx.send(Input::Line(line.to_string()));
                }
            }
            drop(c);
            reply(&mut out, "204 No Content", "text/plain", b"")
        }
        _ => reply(&mut out, "404 Not Found", "text/plain", b"nothing here"),
    }
}

/// A browser's events, their connection read from `reader`, though the
/// page sends nothing on it, until the page closes it, or it is cut, or it
/// is ended here: then counted open no longer, and ended for the thread
/// that writes them too, whose next write fails.
fn watch(reader: &mut BufReader<TcpStream>, coming: &Mutex<Coming>) {
    let mut buf = [0u8; 64];
    loop {
        match reader.read(&mut buf) {
            Ok(n) if n > 0 => {}
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            _ => break,
        }
    }
    lock(coming).open -= 1;
    let _ = reader.get_ref().shutdown(Shutdown::Both);
}

/// What the editor sends a browser, as it comes from `rows`, written to
/// `out` as server-sent events, each line an event: with the first, how
/// long after losing them the browser asks for them again (`RETRY`), and
/// `launch`, this launch, as their id, which the browser says as it asks
/// again; and every fifteen seconds that nothing comes, a comment, by which
/// a closed page is found out writing. Until the editor sends no more, as
/// it is left, or the page is gone.
fn write_events(out: &mut TcpStream, rows: &Receiver<String>, launch: &str) -> std::io::Result<()> {
    out.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-store\r\n\r\n")?;
    out.flush()?;
    let mut fields = format!("retry: {}\nid: {launch}\n", RETRY.as_millis());
    loop {
        match rows.recv_timeout(Duration::from_secs(15)) {
            Ok(m) => {
                write!(out, "{fields}data: {m}\n\n")?;
                fields.clear();
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
/// the caller, saying nothing and opening nothing: where, and the token.
pub fn start(path: PathBuf) -> Result<(u16, String)> {
    let bound = bind(0)?;
    let (port, token) = (bound.port, bound.token.clone());
    std::thread::spawn(move || bound.serving(&path, None));
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
        let line = client::key("ArrowUp", false, false, false, false, false);
        assert_eq!(key(&line.split_whitespace().skip(1).collect::<Vec<_>>()), Some(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)));
        assert_eq!(fitted(5, 5000), (40, 200));
    }

    #[test]
    fn the_editor_in_the_page_is_sent_its_screen_whole_after_its_size_and_t_and_is_given_the_keys_and_pastes_the_client_makes() {
        // Its screen whole: the size, then `t`, then the rows, as the client
        // reads them.
        let mut screen = Screen::new(40, 2);
        screen.process(b"\x1b[32mvi\x1b[0m");
        let (buf, cursor) = screen.buffer();
        let lines = whole_screen((40, 2), &rows(&buf, cursor));
        let read: Vec<String> = lines.iter().map(|l| client::screen(l)).collect();
        assert_eq!((read[0].as_str(), read[1].as_str()), ("size 40 2", "terminal"));
        assert!(read[2].starts_with("row 0\n<span style=\"color:#0dbc79;background:var(--bg)\">vi</span>"), "{read:?}");
        assert_eq!(read.len(), 4);
        // The keys the client makes while the screen is the editor's, made
        // the bytes xterm sends: F5, ↑ in the cursor keys' application mode,
        // ctrl-c.
        let bytes = |line: String, application: bool| crate::pty::key(&line.split_whitespace().skip(1).collect::<Vec<_>>(), application);
        assert_eq!(bytes(client::key("F5", false, false, false, false, true), false), Some(b"\x1b[15~".to_vec()));
        assert_eq!(bytes(client::key("ArrowUp", false, false, false, false, true), true), Some(b"\x1bOA".to_vec()));
        assert_eq!(bytes(client::key("c", true, false, false, false, true), false), Some(vec![3]));
        assert_eq!(bytes(client::key("Tab", false, false, true, false, true), false), Some(b"\x1b[Z".to_vec()));
        // A paste, its text as the client sends it, back as it was; what is
        // not hex, none.
        let text = "one\r\ntwo 日\t";
        let line = client::pasted(text);
        assert_eq!(unhex(line.strip_prefix("p ").unwrap()).as_deref(), Some(text));
        assert_eq!(unhex(""), Some(String::new()));
        for wrong in ["6", "zz", "+f", "日"] {
            assert_eq!(unhex(wrong), None, "{wrong}");
        }
    }

    #[test]
    fn which_stacked_serves_a_page_and_the_page_going_are_lines_each_side_reads() {
        // The line a browser's events start with, as the client reads it:
        // the build, the launch, whether its address is kept.
        let serving = Serving { port: 1, launch: token(), kept: true };
        assert_eq!(client::screen(&serving.version()), format!("version {} {} 1", build(), serving.launch));
        let unkept = Serving { kept: false, ..serving };
        assert_eq!(client::screen(&unkept.version()), format!("version {} {} 0", build(), unkept.launch));
        // The build: of the page and its wasm, the same each time.
        assert_eq!(build().len(), 16);
        assert_eq!(build(), build());
        // The page going: `g` alone.
        assert!(going("g") && going(" g "));
        assert!(!going("g 1") && !going("k c 67 -") && !going(""));
    }

    #[test]
    fn the_notices_learned_at_once_are_told_for_their_banners_then_one_bell_as_the_settings_say() {
        let learned = [
            Notice { run: "run-1".into(), title: "stacked: s".into(), body: "Done.".into() },
            Notice { run: "run-2".into(), title: "stacked: t".into(), body: "Failed.".into() },
        ];
        let web = |notifications, bell| Web { notifications, bell, ..Web::default() };
        let (first, second) = ("n run-1\u{1f}stacked: s\u{1f}Done.", "n run-2\u{1f}stacked: t\u{1f}Failed.");
        // Each notice's line, for its banner, then the bell, once for both.
        assert_eq!(told(&learned, &web(true, true)), [first, second, BELL]);
        // Without the bell, the notices' lines alone; without the banners,
        // the bell alone; with neither, nothing.
        assert_eq!(told(&learned, &web(true, false)), [first, second]);
        assert_eq!(told(&learned, &web(false, true)), [BELL]);
        assert!(told(&learned, &web(false, false)).is_empty());
        // Nothing learned: no bell.
        assert!(told(&[], &web(true, true)).is_empty());
        // As the client reads them: a banner for each, then the bell.
        let read: Vec<String> = told(&learned, &web(true, true)).iter().map(|l| client::screen(l)).collect();
        assert!(read[0].starts_with("notice\nrun-1\n") && read[1].starts_with("notice\nrun-2\n"), "{read:?}");
        assert_eq!(read[2], "bell");
    }

    #[test]
    fn a_token_is_kept_for_its_port_in_a_file_only_you_may_read_written_whole() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("home").join("web-token.json");
        let mode = |f: &Path| std::fs::metadata(f).unwrap().permissions().mode() & 0o777;
        assert_eq!(kept_token(&file, 8080), None, "none kept yet");
        // Kept, its folder made: for its port alone, the file 0600, nothing
        // left beside it.
        let first = token();
        keep_token(&file, 8080, &first).unwrap();
        assert_eq!(kept_token(&file, 8080), Some(first.clone()));
        assert_eq!(kept_token(&file, 8081), None, "kept for another port");
        assert_eq!(mode(&file), 0o600);
        let names: Vec<_> = std::fs::read_dir(file.parent().unwrap()).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names, ["web-token.json"]);
        // Another for another port, in its place: 0600 again, though the
        // file was opened to others meanwhile.
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
        let next = token();
        keep_token(&file, 9090, &next).unwrap();
        assert_eq!((kept_token(&file, 8080), kept_token(&file, 9090)), (None, Some(next)));
        assert_eq!(mode(&file), 0o600);
        // What is not a token, or cannot be read: none.
        std::fs::write(&file, "{\"port\": 8080, \"token\": \"a b&c\"}").unwrap();
        assert_eq!(kept_token(&file, 8080), None);
        std::fs::write(&file, "{\"port\": 8080, \"token\": \"\"}").unwrap();
        assert_eq!(kept_token(&file, 8080), None);
        std::fs::write(&file, "not json").unwrap();
        assert_eq!(kept_token(&file, 8080), None);
        // Nowhere keeps nothing, and is not written; nor is a folder in its
        // place, nor anything beside it.
        let nowhere = Path::new(crate::home::NOWHERE);
        keep_token(nowhere, 8080, &first).unwrap();
        assert_eq!(kept_token(nowhere, 8080), None);
        assert!(!Path::new(&format!("{}.{}", crate::home::NOWHERE, std::process::id())).exists());
        let folder = tmp.path().join("folder");
        std::fs::create_dir(&folder).unwrap();
        keep_token(&folder, 8080, &first).unwrap();
        assert_eq!(kept_token(&folder, 8080), None);
        assert!(std::fs::read_dir(&folder).unwrap().next().is_none());
        let mut names: Vec<_> = std::fs::read_dir(tmp.path()).unwrap().map(|e| e.unwrap().file_name()).collect();
        names.sort();
        assert_eq!(names, ["folder", "home"]);
    }
}
