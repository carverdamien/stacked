//! `stacked web`, served for real: what a browser asks of it over HTTP, and
//! what it sends back as the editor's screen changes.
#![cfg(feature = "web")]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStderr, Command, Stdio};
use std::time::{Duration, Instant};

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_AUTHOR_NAME", "A U Thor")
        .env("GIT_AUTHOR_EMAIL", "author@example.com")
        .env("GIT_COMMITTER_NAME", "C O Mitter")
        .env("GIT_COMMITTER_EMAIL", "committer@example.com")
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

/// stacked, as the tests run it: its own things (its settings, the
/// databases opened last) kept in `home`, a folder of the test's
/// (`$STACKED_HOME`), never in the home of whoever runs the tests; what it
/// writes in the temporary directory, in `tmp` beside it (`$TMPDIR`),
/// removed with the test's folder.
fn stacked(home: &Path) -> Command {
    let tmp = home.with_file_name("tmp");
    std::fs::create_dir_all(&tmp).unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_stacked"));
    cmd.env("STACKED_HOME", home).env("TMPDIR", tmp).env_remove("STACKED_CONFIG").env_remove("STACKED_APP_STATE");
    cmd
}

/// A database holding stack `s` of two patches.
fn setup() -> (tempfile::TempDir, PathBuf) {
    setup_with("Bug 1 - shout two")
}

/// As `setup`, the first patch's message `first`.
fn setup_with(first: &str) -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir(&src).unwrap();
    git(&src, &["init", "-q", "-b", "main"]);
    git(&src, &["config", "commit.gpgsign", "false"]);
    std::fs::write(src.join("a.txt"), "one\ntwo\n").unwrap();
    git(&src, &["add", "."]);
    git(&src, &["commit", "-qm", "base"]);
    std::fs::write(src.join("a.txt"), "one\nTWO\n").unwrap();
    git(&src, &["commit", "-qam", first]);
    std::fs::write(src.join("b.txt"), "bee\n").unwrap();
    git(&src, &["add", "."]);
    git(&src, &["commit", "-qm", "Bug 2 - add b"]);
    let db = tmp.path().join("db.jsonl");
    let out = stacked(&tmp.path().join("home"))
        .current_dir(&src)
        .args(["-f", db.to_str().unwrap(), "import", "s", "HEAD~2"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    (tmp, db)
}

/// A request to the server on `port`, from host `host`: its status line and
/// its body.
fn ask(port: u16, host: &str, request: &str, body: &str) -> (String, Vec<u8>) {
    let (head, body) = answered(port, host, request, body);
    (head.lines().next().unwrap().to_string(), body)
}

/// As `ask`, the answer's whole head: its status line, then its headers.
fn answered(port: u16, host: &str, request: &str, body: &str) -> (String, Vec<u8>) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    write!(s, "{request} HTTP/1.1\r\nHost: {host}\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
    let mut all = Vec::new();
    s.read_to_end(&mut all).unwrap();
    let at = all.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    (String::from_utf8_lossy(&all[..at]).to_string(), all[at + 4..].to_vec())
}

/// How long a test waits for what it waits for before it fails: long, as
/// the tests of this file run at once, each its editor and its worktrees,
/// and other suites may run meanwhile. Nothing waits that long but a test
/// that fails.
const PATIENCE: Duration = Duration::from_secs(120);

/// The events' lines, read until one is as `wanted` says: what it is. The
/// server says that it is still there every fifteen seconds, so the wait
/// ends though nothing changes.
fn until(events: &mut BufReader<TcpStream>, what: &str, wanted: impl Fn(&str) -> bool) -> Vec<String> {
    let started = Instant::now();
    let mut seen = Vec::new();
    while started.elapsed() < PATIENCE {
        let mut line = String::new();
        if events.read_line(&mut line).unwrap() == 0 {
            break;
        }
        let line = line.trim_end().to_string();
        let found = wanted(&line);
        seen.push(line);
        if found {
            return seen;
        }
    }
    panic!("never sent {what}: {seen:#?}");
}

/// The events of the server on `port`, for a page `cols` wide: the screen
/// as it changes.
fn events(port: u16, token: &str, cols: u16) -> BufReader<TcpStream> {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    write!(s, "GET /events?t={token}&cols={cols}&rows=30 HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n").unwrap();
    BufReader::new(s)
}

/// Stack `s`'s checkout, under `tmp`, set to have an answer's worktree made
/// ahead, and a change's setting wrong: the bar saying so once the worktree
/// is made, the editor knowing of it then. Where the worktree is made.
fn made_ahead(tmp: &Path) -> PathBuf {
    let src = tmp.join("src");
    std::fs::create_dir_all(src.join(".stacked")).unwrap();
    std::fs::write(src.join(".stacked/local.toml"), "[agent]\nanswer_only_worktree = \"precreate\"\nworktree = \"sometimes\"\n").unwrap();
    src.join(".stacked/s/answer-only-agent")
}

/// What the bar says once the worktree `made_ahead` asks for is made.
const MADE: &str = "is not ondemand or precreate";

/// Waits for `dir` to be gone: whether it is.
fn gone(dir: &Path) -> bool {
    let started = Instant::now();
    while started.elapsed() < PATIENCE {
        if !dir.exists() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    !dir.exists()
}

/// `stacked web`, run: killed if the test fails before it exits.
struct Running(Child);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// `stacked web` on `db`, run in `tmp`, outside the stack's checkout (its
/// settings read, and said wrong, only as the worktree is made ahead); its
/// `$EDITOR` run in the terminal it was started from, as its settings say,
/// sending it `signal` (as `kill` names it) as it runs, as a key pressed in
/// that terminal would, and changing nothing but `tmp/edited`, which says
/// it ran. It, what it says on stderr after where it is, and its events.
fn web(tmp: &Path, db: &Path, signal: &str) -> (Running, BufReader<ChildStderr>, BufReader<TcpStream>, u16, String) {
    let (editor, pid, edited) = (tmp.join("editor.sh"), tmp.join("pid"), tmp.join("edited"));
    let script = format!("#!/bin/sh\nkill -{signal} \"$(cat '{}')\"\nsleep 0.2\ntouch '{}'\n", pid.display(), edited.display());
    std::fs::write(&editor, script).unwrap();
    std::fs::set_permissions(&editor, std::fs::Permissions::from_mode(0o755)).unwrap();
    settings_of(&tmp.join("home"), IN_THE_TERMINAL);
    let mut child = stacked(&tmp.join("home"))
        .args(["-f", db.to_str().unwrap(), "web", "--no-open"])
        .env("VISUAL", &editor)
        .env("EDITOR", &editor)
        .current_dir(tmp)
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    std::fs::write(&pid, child.id().to_string()).unwrap();
    let mut said = BufReader::new(child.stderr.take().unwrap());
    let child = Running(child);
    // Where it is, as it says: "the editor is at http://127.0.0.1:PORT/?t=TOKEN".
    let mut line = String::new();
    said.read_line(&mut line).unwrap();
    let url = line.split_whitespace().find_map(|w| w.strip_prefix("http://127.0.0.1:")).expect(&line);
    let (port, token) = url.split_once("/?t=").expect(&line);
    let port: u16 = port.parse().unwrap();
    let mut events = events(port, token, 400);
    until(&mut events, "the worktree made ahead", |l| l.contains(MADE));
    (child, said, events, port, token.to_string())
}

/// `signal` (as `kill` names it) sent to `stacked web`.
fn send(child: &Running, signal: &str) {
    let sent = Command::new("kill").args([format!("-{signal}"), child.0.id().to_string()]).status().unwrap();
    assert!(sent.success(), "kill -{signal}");
}

/// The text of row event `l`, without its styles.
fn text(l: &str) -> String {
    let runs = l.strip_prefix("data: r ").and_then(|r| r.split_once(' ')).map_or("", |(_, runs)| runs);
    runs.split('\u{1e}').filter_map(|r| r.split_once('\u{1f}')).map(|(_, t)| t).collect()
}

/// The first patch's menu opened with a right click, and `Edit this patch
/// in $EDITOR` clicked in it.
fn edit_first_patch(port: u16, token: &str, events: &mut BufReader<TcpStream>) {
    let (host, input) = (format!("127.0.0.1:{port}"), format!("POST /input?t={token}"));
    ask(port, &host, &input, "m 5 2 r\n");
    let item = "Edit this patch in $EDITOR";
    let seen = until(events, "the patch's menu", |l| text(l).contains(item));
    let row = seen.last().unwrap();
    let y = row.strip_prefix("data: r ").and_then(|r| r.split(' ').next()).unwrap();
    let line = text(row);
    let x = line[..line.find(item).unwrap()].chars().count() + 1;
    ask(port, &host, &input, &format!("m {x} {y} l\n"));
}

/// `stacked web` left as Quit leaves it: the page told, the process done,
/// the worktree made ahead, `made`, removed.
fn left(mut child: Running, mut said: BufReader<ChildStderr>, events: &mut BufReader<TcpStream>, made: &Path) {
    until(events, "bye", |l| l == "data: bye");
    let started = Instant::now();
    let status = loop {
        if let Some(s) = child.0.try_wait().unwrap() {
            break s;
        }
        assert!(started.elapsed() < PATIENCE, "still running");
        std::thread::sleep(Duration::from_millis(50));
    };
    let mut rest = String::new();
    said.read_to_string(&mut rest).unwrap();
    assert!(status.success(), "{status}: {rest}");
    assert!(gone(made), "removed once the editor is left");
}

#[test]
fn a_browser_sees_the_editor_and_drives_it() {
    let (_tmp, db) = setup();
    stacked::config::init(stacked::config::Config::default());
    let (port, token) = stacked::web::start(db).unwrap();
    let host = format!("127.0.0.1:{port}");
    // Without the token, or from another name, nothing.
    assert_eq!(ask(port, &host, "GET /", "").0, "HTTP/1.1 403 Forbidden");
    assert_eq!(ask(port, "evil.example:80", &format!("GET /?t={token}"), "").0, "HTTP/1.1 403 Forbidden");
    // The page, and its wasm.
    let (status, page) = ask(port, &host, &format!("GET /?t={token}"), "");
    assert_eq!(status, "HTTP/1.1 200 OK");
    assert!(String::from_utf8_lossy(&page).contains("WebAssembly.instantiateStreaming"));
    let (status, wasm) = ask(port, &host, &format!("GET /client.wasm?t={token}"), "");
    assert_eq!(status, "HTTP/1.1 200 OK");
    assert_eq!(&wasm[..4], b"\0asm");
    // The screen as it changes: its size, every row, then what changes.
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    write!(s, "GET /events?t={token}&cols=100&rows=30 HTTP/1.1\r\nHost: {host}\r\n\r\n").unwrap();
    let mut events = BufReader::new(s);
    let seen = until(&mut events, "the patches", |l| l.starts_with("data: r 3 ") && l.contains("Bug 2 - add b"));
    assert!(seen.iter().any(|l| l == "data: s 100 30"), "{seen:#?}");
    // ↓: the second patch picked, its row redrawn reversed, and the middle
    // its patch.
    assert_eq!(ask(port, &host, &format!("POST /input?t={token}"), "k Down -\n").0, "HTTP/1.1 204 No Content");
    until(&mut events, "the second patch picked", |l| l.starts_with("data: r 3 ") && l.contains(",,64\u{1f}  2 Bug 2 - add b"));
    until(&mut events, "its patch", |l| l.contains("+bee"));
    // A click on the first patch, the screen's third row, picks it.
    ask(port, &host, &format!("POST /input?t={token}"), "m 5 2 l\n");
    until(&mut events, "the first patch", |l| l.contains("+TWO"));
    // The mouse over the second marks it, underlined (8), and picks
    // nothing: the first stays in the middle. A click on it picks it; a
    // second is Enter: read, the cursor in the middle, the patch in the
    // list on grey (colour 238, the settings' `picked`), not reversed, and
    // underlined while the mouse is over it.
    ask(port, &host, &format!("POST /input?t={token}"), "v 5 3\n");
    until(&mut events, "the second patch, hovered", |l| l.starts_with("data: r 3 ") && l.contains(",,8\u{1f}  2 Bug 2 - add b"));
    ask(port, &host, &format!("POST /input?t={token}"), "m 5 3 l\n");
    until(&mut events, "the second patch picked", |l| l.contains("+bee"));
    ask(port, &host, &format!("POST /input?t={token}"), "m 5 3 l\n");
    until(&mut events, "the second patch read", |l| l.starts_with("data: r 3 ") && l.contains(",#444444,8\u{1f}  2 Bug 2 - add b"));
    // The mouse gone from the page: nothing underlined.
    ask(port, &host, &format!("POST /input?t={token}"), "o\n");
    until(&mut events, "the mouse gone", |l| l.starts_with("data: r 3 ") && l.contains(",#444444,0\u{1f}  2 Bug 2 - add b"));
}

#[test]
fn the_page_s_tab_has_stacked_s_icon_asked_for_with_the_token_as_all_else_is() {
    let (_tmp, db) = setup();
    stacked::config::init(stacked::config::Config::default());
    let (port, token) = stacked::web::start(db).unwrap();
    let host = format!("127.0.0.1:{port}");
    // The page links the icon at two sizes, and asks for each where its
    // link says, with the query of the page's own address: the token.
    let (status, page) = ask(port, &host, &format!("GET /?t={token}"), "");
    assert_eq!(status, "HTTP/1.1 200 OK");
    let page = String::from_utf8(page).unwrap();
    assert!(page.contains("icon.href = icon.dataset.path + location.search"), "{page}");
    for size in [32, 64] {
        let path = format!("/icon-{size}.png");
        let link = format!(r#"<link rel="icon" type="image/png" sizes="{size}x{size}" data-path="{path}">"#);
        assert!(page.contains(&link), "{link}: {page}");
        // Asked so, the icon drawn at that size, as a PNG.
        let (head, png) = answered(port, &host, &format!("GET {path}?t={token}"), "");
        assert!(head.starts_with("HTTP/1.1 200 OK\r\n") && head.contains("\r\nContent-Type: image/png\r\n"), "{head}");
        assert!(png == stacked::icon::png(size), "{size}: not the icon");
        // Without the token, or from another name, nothing.
        assert_eq!(ask(port, &host, &format!("GET {path}"), "").0, "HTTP/1.1 403 Forbidden");
        assert_eq!(ask(port, "evil.example:80", &format!("GET {path}?t={token}"), "").0, "HTTP/1.1 403 Forbidden");
    }
    // The icon a browser may ask for on its own, never with the token: nothing.
    assert_eq!(ask(port, &host, "GET /favicon.ico", "").0, "HTTP/1.1 403 Forbidden");
}

/// A stand-in for a browser, for node to run the page's own scripts in, as
/// web/index.html has them, then `$STACKED_SCENARIO`, which drives them
/// through `t` and checks them with node's `assert`: the page's tab, two
/// icons as its links say, the page not looked at; a canvas that draws
/// nothing but writes what it was asked to draw in its data URL, for `t`
/// to read back; pictures that load at once, from nowhere; a display, its
/// density and its window's size set by `t` (`move`, `resize`), which lays
/// out a row and a run of characters as a browser does, a row's height and
/// a run's width in its layout's 64ths of a pixel, the font's advance the
/// density's, as hinting makes it; frames run when `t` says (`frame`); the
/// wasm, once `t` starts the page (`start`), a stand-in that makes the
/// server's size line the page's and keeps the clicks, and lines sent to
/// the server kept (`said`); no server, no network and no window. An
/// exception, an assertion failed or a promise rejected that nothing
/// handles fails it.
const BROWSER: &str = r#"
const vm = require('vm'), fs = require('fs'), assert = require('assert');
process.on('unhandledRejection', e => { console.error('unhandled:', e); process.exit(1); });
const page = fs.readFileSync(process.env.STACKED_PAGE, 'utf8');
const scripts = Array.from(page.matchAll(/<script>([\s\S]*?)<\/script>/g), m => m[1]);
const on = all => (type, f) => { (all[type] ||= []).push(f); };
const off = all => (type, f) => { all[type] = (all[type] || []).filter(g => g !== f); };
const fire = (all, type) => { for (const f of [...(all[type] || [])]) f({type}); };
const ofWindow = {}, ofDocument = {}, loaded = [], banners = [];
let hidden = true, focused = false;
const links = Array.from(page.matchAll(/<link rel="icon" type="image\/png" sizes="(\d+)x\d+" data-path="([^"]+)">/g),
  ([, px, path]) => ({dataset: {path}, href: '', getAttribute: n => (n === 'sizes' ? `${px}x${px}` : null)}));
// The display: its density, the window's size in CSS pixels, a line's
// height, 1.3 times the page's 14px font, and the font's advance at each
// density.
const display = {dpr: 1, width: 1200, height: 800, line: 18.2, advance: {1: 8.4}};
const unit = v => Math.round(v * 64) / 64;
const element = tag => {
  const el = {tag, style: {setProperty(k, v) { this[k] = v; }}, children: [], dataset: {}, textContent: '', listeners: {}, parent: null,
    append(...c) { for (const x of c) { x.parent = el; el.children.push(x); } },
    remove() { if (el.parent) el.parent.children = el.parent.children.filter(x => x !== el); el.parent = null; },
    replaceChildren(...c) { el.children = []; el.append(...c); },
    addEventListener(type, f) { (el.listeners[type] ||= []).push(f); },
    // A run of characters as wide as their advances; a row as tall as its
    // own height, the screen's `--row` it is given, else its line's.
    getBoundingClientRect() {
      if (tag === 'span') return {width: unit(el.textContent.length * display.advance[display.dpr]), height: unit(display.line), left: 0, top: 0};
      if (tag === 'div' && el.parent) {
        const row = el.parent.style['--row'];
        const h = el.style.height === 'auto' || !row ? display.line : parseFloat(row);
        return {width: display.width, height: unit(h), left: 0, top: 0};
      }
      return {width: 800, height: 16, left: 0, top: 0};
    },
  };
  return el;
};
const ids = {};
const canvas = () => {
  const drawn = {width: 0, fills: [], strokes: [], texts: [], images: []};
  const g = {
    fillStyle: '', strokeStyle: '', font: '', lineWidth: 1,
    drawImage(img, x, y, w, h) { drawn.images.push([img.src, w, h]); },
    beginPath() {}, arc() {}, fill() { drawn.fills.push(this.fillStyle); }, stroke() { drawn.strokes.push(this.strokeStyle); },
    fillText(text) { drawn.texts.push([text, this.fillStyle, this.font]); },
  };
  return {
    set width(v) { drawn.width = v; }, get width() { return drawn.width; }, height: 0,
    getContext: () => g,
    toDataURL: () => 'data:image/png;base64,' + Buffer.from(JSON.stringify(drawn)).toString('base64'),
  };
};
class Image {
  set src(v) { this._src = v; loaded.push(v); queueMicrotask(() => this.onload && this.onload()); }
  get src() { return this._src; }
}
const navigator = {platform: 'Linux', sendBeacon() {}};
const document = {
  get hidden() { return hidden; },
  hasFocus: () => focused,
  querySelectorAll: q => (q === 'link[rel=icon]' ? links : []),
  getElementById: id => (ids[id] ||= element('pre')),
  createElement: tag => (tag === 'canvas' ? canvas() : element(tag)),
  addEventListener: on(ofDocument), removeEventListener: off(ofDocument),
};
// The queries for a density the page made, each told once that it no
// longer holds; the frames asked for; the events asked for; what the page
// sent the server; and the wasm, made once the page is started.
let queries = [], frames = [], frameId = 0, wasm;
const events = [], posted = [], clicks = [], ofFonts = {};
const matchMedia = q => ({
  addEventListener(type, f) { queries.push([parseFloat(q.match(/resolution: ([\d.]+)dppx/)[1]), f]); },
});
class EventSource {
  constructor(url) { this.url = url; this.readyState = 1; events.push(this); }
  close() {}
}
EventSource.CLOSED = 2;
const window = {
  document, navigator, Image, URLSearchParams, TextEncoder, TextDecoder, performance, queueMicrotask, setTimeout, console, Buffer,
  location: {search: '?t=token', reload() {}},
  devicePixelRatio: display.dpr, innerWidth: display.width, innerHeight: display.height, matchMedia, EventSource,
  requestAnimationFrame: f => { frames.push(f); return ++frameId; },
  sessionStorage: {getItem: () => null, setItem() {}},
  addEventListener: on(ofWindow), removeEventListener: off(ofWindow),
  fetch: (url, o) => (url.startsWith('/input') ? (posted.push(o.body), Promise.resolve({})) : new Promise(() => {})),
  focus() {},
};
window.window = window;
let fontsReady;
document.fonts = {ready: new Promise(r => { fontsReady = r; }), addEventListener: on(ofFonts)};
// The wasm's stand-in: a line of the server's sizing the screen made the
// page's (`size COLS ROWS`), any other nothing; a click kept as the page
// gives it; all else nothing.
const exports = () => {
  const memory = {buffer: new ArrayBuffer(4096)}, OUT = 2048;
  const give = text => { const b = new TextEncoder().encode(text); new Uint8Array(memory.buffer, OUT, b.length).set(b); return b.length; };
  const nothing = () => 0;
  return {
    memory, alloc: () => 0, out: () => OUT,
    screen: (p, n) => {
      const [op, c, r] = new TextDecoder().decode(new Uint8Array(memory.buffer, p, n)).split(' ');
      return op === 's' ? give(`size ${c} ${r}`) : 0;
    },
    click: (x, y, cw, ch, right) => { clicks.push({x, y, cw, ch, right}); return 0; },
    key: nothing, moved: nothing, dragged: nothing, up: nothing, wheel: nothing, left: nothing, blur: nothing, focus: nothing,
    paste: nothing, pasted: nothing,
  };
};
// Node's assert, but for values of two realms, the page's and node's own,
// whose arrays it would never find equal: compared as JSON.
const check = (ok, why) => assert(ok, why);
check.strictEqual = assert.strictEqual;
check.deepStrictEqual = (a, b) => assert.deepStrictEqual(JSON.parse(JSON.stringify(a)), JSON.parse(JSON.stringify(b)));
const t = {
  assert: check, loaded, banners, navigator, window,
  plain: links.map(l => l.dataset.path + '?t=token'),
  icons: () => links.map(l => l.href),
  // What the canvas was asked to draw for each icon, from its data URL.
  badges: () => links.map(l => {
    assert(l.href.startsWith('data:image/png;base64,'), `plain: ${l.href}`);
    return JSON.parse(Buffer.from(l.href.slice('data:image/png;base64,'.length), 'base64'));
  }),
  settle: () => new Promise(r => setTimeout(r, 0)),
  show() { hidden = false; fire(ofDocument, 'visibilitychange'); },
  hide() { hidden = true; fire(ofDocument, 'visibilitychange'); },
  focus() { focused = true; fire(ofWindow, 'focus'); },
  blur() { focused = false; fire(ofWindow, 'blur'); },
  look() { this.show(); this.focus(); },
  display, events, clicks,
  // The page started as the browser starts it: its wasm made, its events
  // asked for and opened, the fonts it had loaded.
  async start() {
    wasm({exports: exports()});
    await t.settle();
    events[0].onopen();
    fontsReady();
    await t.settle();
  },
  // The frame the browser draws next: what the page asked to run there run.
  async frame() {
    const now = frames;
    frames = [];
    for (const f of now) f(performance.now());
    await t.settle();
  },
  // The lines the page sent the server since this was last asked.
  said() { return posted.splice(0).join('').split('\n').filter(l => l); },
  // The window moved to a screen of density `dpr`, its font's advance
  // `advance` there, its size in CSS pixels as it was: no resize, but each
  // query for another density told so.
  move(dpr, advance) {
    display.dpr = window.devicePixelRatio = dpr;
    display.advance[dpr] = advance;
    const told = queries.filter(([d]) => d !== dpr);
    queries = queries.filter(([d]) => d === dpr);
    for (const [, f] of told) f({type: 'change'});
  },
  resize(width, height) {
    display.width = window.innerWidth = width;
    display.height = window.innerHeight = height;
    fire(ofWindow, 'resize');
  },
  // A font loaded, its advance `advance` at the density the page is on.
  fontLoaded(advance) {
    display.advance[display.dpr] = advance;
    fire(ofFonts, 'loadingdone');
  },
  // The server's line `line` read by the page.
  heard(line) { events[0].onmessage({data: line, target: events[0]}); },
  // A left click at `(x, y)` on the screen.
  click(x, y) {
    for (const f of document.getElementById('screen').listeners.mousedown || []) f({button: 0, clientX: x, clientY: y, preventDefault() {}});
  },
  away() { this.blur(); this.hide(); },
  // Banners allowed, made as the page asks, kept.
  allow() {
    window.Notification = class { constructor(title, o) { banners.push([title, o.body, o.tag]); } close() {} };
    window.Notification.permission = 'granted';
  },
};
window.t = t;
vm.createContext(window);
// Never made, unless `t` starts the page.
window.made = f => { wasm = f; };
vm.runInContext('globalThis.WebAssembly = {instantiateStreaming: () => new Promise(r => made(instance => r({instance})))};', window);
for (const s of scripts) vm.runInContext(s, window);
vm.runInContext(`(async () => {\n${process.env.STACKED_SCENARIO}\n})()`, window).then(
  () => t.settle().then(() => console.log('ok')),
  e => { console.error(e); process.exit(1); });
"#;

/// Runs `scenario` in node, on the page's scripts in `BROWSER`'s stand-in
/// for a browser; fails with what node said unless it ran through.
fn in_the_page(scenario: &str) {
    let mut node = Command::new("node")
        .arg("-")
        .env("STACKED_PAGE", concat!(env!("CARGO_MANIFEST_DIR"), "/web/index.html"))
        .env("STACKED_SCENARIO", scenario)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("node, which these tests run the page's scripts in");
    node.stdin.take().unwrap().write_all(BROWSER.as_bytes()).unwrap();
    let out = node.wait_with_output().unwrap();
    let (said, failed) = (String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success() && said.trim_end() == "ok" && failed.is_empty(), "{}\n{said}{failed}", out.status);
}

#[test]
fn a_notice_while_the_page_is_not_looked_at_counts_on_the_tab_s_icon_until_the_page_is_looked_at_again() {
    in_the_page(
        r#"
        const {assert} = t;
        assert.deepStrictEqual(t.icons(), t.plain);
        // A notice, the page hidden: each icon StackED's, at its own size,
        // with a red badge, ringed, holding 1 in white.
        notice('s', 'stacked: s', 'A landed ask waits for your review.', 'n 1');
        await t.settle();
        const one = t.badges();
        assert.deepStrictEqual(one.map(b => [b.width, b.images, b.texts.map(([text, colour]) => [text, colour])]), [
            [32, [[t.plain[0], 32, 32]], [['1', '#fff']]],
            [64, [[t.plain[1], 64, 64]], [['1', '#fff']]],
        ]);
        for (const b of one) assert.deepStrictEqual([b.fills, b.strokes], [['#d32f2f'], ['#1e1e1e']]);
        // A second makes 2; the plain icons are loaded once.
        notice('s', 'stacked: s', 'A run failed.', 'n 2');
        await t.settle();
        assert.deepStrictEqual(t.badges().map(b => b.texts[0][0]), ['2', '2']);
        assert.deepStrictEqual(t.loaded, t.plain);
        // The page shown, but its window not the one with the focus: still
        // not looked at, still 2. Its window given the focus: the plain
        // icons, and the count gone with them.
        t.show();
        await t.settle();
        assert.deepStrictEqual(t.badges().map(b => b.texts[0][0]), ['2', '2']);
        t.focus();
        assert.deepStrictEqual(t.icons(), t.plain);
        await t.settle();
        assert.deepStrictEqual(t.icons(), t.plain);
        // Away again, a notice counts from 0: 1.
        t.away();
        notice('s', 'stacked: s', 'A try push is done.', 'n 3');
        await t.settle();
        assert.deepStrictEqual(t.badges().map(b => b.texts[0][0]), ['1', '1']);
        // Looked at again from a hidden tab, the window having kept the
        // focus: the plain icons.
        t.focus();
        assert.deepStrictEqual(t.badges().map(b => b.texts[0][0]), ['1', '1']);
        t.show();
        assert.deepStrictEqual(t.icons(), t.plain);
        "#,
    );
}

#[test]
fn a_notice_while_the_page_is_looked_at_counts_nothing_and_past_9_the_badge_says_9_plus() {
    in_the_page(
        r#"
        const {assert} = t;
        t.look();
        notice('s', 'stacked: s', 'A run failed.', 'n 1');
        await t.settle();
        assert.deepStrictEqual([t.icons(), t.loaded], [t.plain, []]);
        // Not looked at, the first notice counts 1, not 2.
        t.blur();
        notice('s', 'stacked: s', 'A run failed.', 'n 2');
        await t.settle();
        assert.deepStrictEqual(t.badges().map(b => b.texts[0][0]), ['1', '1']);
        for (let i = 2; i <= 9; i++) notice('s', 'stacked: s', 'A run failed.', 'n 3');
        await t.settle();
        assert.deepStrictEqual(t.badges().map(b => b.texts[0][0]), ['9', '9']);
        const size = b => parseInt(b.texts[0][2].split(' ')[1]);
        const one = t.badges().map(size);
        notice('s', 'stacked: s', 'A run failed.', 'n 4');
        await t.settle();
        assert.deepStrictEqual(t.badges().map(b => b.texts[0][0]), ['9+', '9+']);
        // Two characters, in a smaller font than one.
        const two = t.badges().map(size);
        assert(two[0] < one[0] && two[1] < one[1], JSON.stringify([one, two]));
        "#,
    );
}

#[test]
fn a_badge_drawn_after_the_page_was_looked_at_again_is_dropped_and_banners_shown_or_not_each_notice_counts() {
    in_the_page(
        r#"
        const {assert} = t;
        // Looked at before the plain icons loaded for the badge: they stay.
        notice('s', 'stacked: s', 'A run failed.', 'n 1');
        t.look();
        await t.settle();
        assert.deepStrictEqual(t.icons(), t.plain);
        // Banners allowed: one for each notice, and each counted.
        t.away();
        t.allow();
        notice('s', 'stacked: s', 'A run failed.', 'n 2');
        notice('s', 'stacked: s', 'A try push is done.', 'n 3');
        await t.settle();
        assert.deepStrictEqual(t.banners.map(b => b[1]), ['A run failed.', 'A try push is done.']);
        assert.deepStrictEqual(t.badges().map(b => b.texts[0][0]), ['2', '2']);
        "#,
    );
}

#[test]
fn an_installed_app_s_badge_says_the_count_where_the_browser_has_one_and_without_one_nothing_throws() {
    // No setAppBadge, as in Firefox: the tab's icon alone, nothing thrown.
    in_the_page(
        r#"
        const {assert} = t;
        assert.strictEqual(t.navigator.setAppBadge, undefined);
        notice('s', 'stacked: s', 'A run failed.', 'n 1');
        await t.settle();
        assert.deepStrictEqual(t.badges().map(b => b.texts[0][0]), ['1', '1']);
        t.look();
        assert.deepStrictEqual(t.icons(), t.plain);
        "#,
    );
    // With one: the count, then cleared once the page is looked at.
    in_the_page(
        r#"
        const {assert} = t, badge = [];
        t.navigator.setAppBadge = n => { badge.push(n); return Promise.resolve(); };
        t.navigator.clearAppBadge = () => { badge.push('cleared'); return Promise.resolve(); };
        notice('s', 'stacked: s', 'A run failed.', 'n 1');
        notice('s', 'stacked: s', 'A run failed.', 'n 2');
        t.look();
        await t.settle();
        assert.deepStrictEqual(badge, [1, 2, 'cleared']);
        "#,
    );
    // One the browser refuses, its promise rejected: nothing unhandled,
    // the tab's icon as ever.
    in_the_page(
        r#"
        const {assert} = t;
        const no = () => Promise.reject(new Error('NotAllowedError'));
        t.navigator.setAppBadge = no;
        t.navigator.clearAppBadge = no;
        notice('s', 'stacked: s', 'A run failed.', 'n 1');
        await t.settle();
        assert.deepStrictEqual(t.badges().map(b => b.texts[0][0]), ['1', '1']);
        t.look();
        await t.settle();
        assert.deepStrictEqual(t.icons(), t.plain);
        "#,
    );
}

#[test]
fn the_page_moved_to_a_screen_of_another_density_without_a_resize_measures_itself_again_and_says_its_new_size_once() {
    in_the_page(
        r#"
        const {assert} = t;
        // At 1x, the font's advance 8.4 and its line 18.2: rows of 18 pixels,
        // 142 by 44 cells in 1200 by 800, asked for and said as the events
        // open; the fonts loaded as they were measured, nothing more.
        await t.start();
        assert(t.events[0].url.includes('&cols=142&rows=44'), t.events[0].url);
        assert.deepStrictEqual(t.said(), ['s 142 44']);
        await t.frame();
        assert.deepStrictEqual(t.said(), []);
        assert.strictEqual(document.getElementById('screen').style['--row'], '18px');
        // Moved to a 2x screen, its size in CSS pixels the same, where the
        // font's advance is 8.5: no resize, measured again in the next
        // frame, and said once.
        t.move(2, 8.5);
        assert.deepStrictEqual(t.said(), []);
        await t.frame();
        assert.deepStrictEqual(t.said(), ['s 141 44']);
        await t.frame();
        assert.deepStrictEqual(t.said(), []);
        // Back, the query made again for 2x tells so too.
        t.move(1, 8.4);
        await t.frame();
        assert.deepStrictEqual(t.said(), ['s 142 44']);
        // At 1.25x, 18.2 is 22.75 device pixels: a row is made 23, 18.4
        // pixels, which the layout keeps as 18.40625, fewer rows fitting.
        t.move(1.25, 8.4);
        await t.frame();
        assert.deepStrictEqual(t.said(), ['s 142 43']);
        assert.strictEqual(document.getElementById('screen').style['--row'], '18.4px');
        assert.deepStrictEqual([cw, ch], [8.4, 18.40625]);
        "#,
    );
}

#[test]
fn an_unchanged_grid_says_nothing_and_several_resizes_in_a_frame_say_the_size_once() {
    in_the_page(
        r#"
        const {assert} = t;
        await t.start();
        await t.frame();
        assert.deepStrictEqual(t.said(), ['s 142 44']);
        // Three resizes, a font loaded and a move in one frame: measured
        // once, the last size said.
        t.resize(1000, 700);
        t.resize(1100, 750);
        t.fontLoaded(8.4);
        t.move(2, 8.4);
        t.resize(1210, 800);
        assert.deepStrictEqual(t.said(), []);
        await t.frame();
        assert.deepStrictEqual(t.said(), ['s 144 44']);
        // Resized, and a font of another advance, but the same cells fit:
        // nothing said, the cell's size changed for the mouse all the same.
        t.resize(1212, 805);
        await t.frame();
        t.fontLoaded(8.41);
        await t.frame();
        assert.deepStrictEqual(t.said(), []);
        assert.deepStrictEqual([cw, ch], [8.41, 18]);
        "#,
    );
}

#[test]
fn a_font_loaded_after_the_page_was_measured_measures_it_again() {
    in_the_page(
        r#"
        const {assert} = t;
        await t.start();
        await t.frame();
        assert.deepStrictEqual(t.said(), ['s 142 44']);
        t.fontLoaded(8);
        await t.frame();
        assert.deepStrictEqual(t.said(), ['s 150 44']);
        "#,
    );
}

#[test]
fn a_click_after_a_move_to_a_screen_of_another_density_is_on_the_cell_the_display_shows_there() {
    in_the_page(
        r#"
        const {assert} = t;
        await t.start();
        await t.frame();
        // At 1.25x, the font's advance 8.4375 and a row 18.40625 pixels:
        // the middle of the cell of column 130, row 30, as the display lays
        // it out, is clicked with the cell's size there, on that cell, as
        // web/client.rs `cell` reads it; with the size at 1x, it was not.
        t.move(1.25, 8.4375);
        await t.frame();
        const [x, y] = [130.5 * 8.4375, 30.5 * 18.40625];
        t.click(x, y);
        const c = t.clicks.pop();
        assert.deepStrictEqual([Math.floor(c.x / c.cw), Math.floor(c.y / c.ch)], [130, 30]);
        assert(Math.floor(x / 8.4) !== 130 && Math.floor(y / 18) !== 30, 'the size at 1x clicks the cell anyway');
        "#,
    );
}

#[test]
fn a_tab_looked_at_again_says_its_size_if_another_tab_s_was_said_since_and_never_on_the_server_s_size_alone() {
    in_the_page(
        r#"
        const {assert} = t;
        await t.start();
        await t.frame();
        assert.deepStrictEqual(t.said(), ['s 142 44']);
        // Another tab's size, the screen sized so: nothing said in answer.
        t.heard('s 100 30');
        await t.frame();
        assert.deepStrictEqual(t.said(), []);
        // This tab looked at again: its own size said.
        t.focus();
        await t.settle();
        assert.deepStrictEqual(t.said(), ['s 142 44']);
        // Looked at again, the screen its size: nothing.
        t.heard('s 142 44');
        t.blur();
        t.focus();
        await t.settle();
        assert.deepStrictEqual(t.said(), []);
        "#,
    );
}

/// One editor of this process at a time submits to Phabricator: the
/// stand-in a submit reads is the process's own.
#[cfg(feature = "moz-phab")]
fn one_submit() -> std::sync::MutexGuard<'static, ()> {
    static ONE: std::sync::Mutex<()> = std::sync::Mutex::new(());
    ONE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Stack `s` of `setup_with`, its first patch naming D11, given a version
/// of its own after the first, which says `TWO` otherwise; and a stand-in
/// for Phabricator as a submit reads it, nothing of D11 stored, which it
/// fetches first: D11 in work, its latest diff that first version, and its
/// repository. What was asked of it.
#[cfg(feature = "moz-phab")]
fn d11_in_work() -> (tempfile::TempDir, PathBuf, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
    let (tmp, db) = setup_with("Bug 1 - shout two\n\nDifferential Revision: https://phabricator.example.com/D11");
    let raw = {
        let d = stacked::db::Db::load(&db).unwrap();
        d.patch(&d.stack("s").unwrap().patches[0]).diff.clone()
    };
    stacked::db::Db::edit(&db, |d| {
        let old = d.patch(&d.stack("s")?.patches[0]).clone();
        let draft = stacked::mbox::Draft { diff: old.diff.replace("+TWO", "+Two"), ..stacked::mbox::Draft::from(&old) };
        stacked::ops::replace(d, "s", 0, draft, "edit").map(|_| ())
    })
    .unwrap();
    let asked = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let seen = asked.clone();
    stacked::submit::stand_in(Some(Box::new(move |m, _| {
        seen.lock().unwrap().push(m.to_string());
        Ok(match m {
            "differential.revision.search" => serde_json::json!({"data": [{"id": 11, "phid": "PHID-DREV-11", "fields": {
                "title": "WIP: Bug 1 - shout two", "summary": "", "status": {"value": "changes-planned", "name": "Changes Planned"},
                "isDraft": false, "bugzilla.bug-id": "1", "repositoryPHID": "PHID-REPO-1"}}]}),
            "edge.search" => serde_json::json!({"data": [], "cursor": {"after": null}}),
            "differential.diff.search" => serde_json::json!({"data": [{"id": 5, "fields": {"revisionPHID": "PHID-DREV-11", "refs": []}}], "cursor": {"after": null}}),
            "differential.getrawdiff" => serde_json::Value::String(raw.clone()),
            "transaction.search" => serde_json::json!({"data": [], "cursor": {"after": null}}),
            "diffusion.repository.search" => serde_json::json!({"data": [{"phid": "PHID-REPO-1", "fields": {"vcs": "git"}}]}),
            _ => return Err(format!("{m}: not asked in this test")),
        })
    })));
    (tmp, db, asked)
}

#[cfg(feature = "moz-phab")]
#[test]
fn the_review_of_a_submit_opens_its_menu_with_a_right_click_and_draws_its_code_in_the_view_its_dialog_sets_in_the_browser_too() {
    let _one = one_submit();
    let (_tmp, db, asked) = d11_in_work();
    stacked::config::init(stacked::config::Config::default());
    let (port, token) = stacked::web::start(db).unwrap();
    let (host, input) = (format!("127.0.0.1:{port}"), format!("POST /input?t={token}"));
    let mut events = events(port, &token, 120);
    until(&mut events, "the patches", |l| l.starts_with("data: r ") && l.contains("Bug 2 - add b"));
    let click = |events: &mut BufReader<TcpStream>, what: &str, item: &str| {
        let seen = until(events, what, |l| text(l).contains(item));
        let (x, y) = cell_of(seen.last().unwrap(), item);
        ask(port, &host, &input, &format!("m {x} {y} l\n"));
        seen
    };
    // The first patch's submit, from its Phabricator menu, the patch's menu
    // opened with a right click: what is sent read, then reviewed on a
    // screen of its own.
    ask(port, &host, &input, "m 5 2 r\n");
    click(&mut events, "the patch's menu", "Phabricator ");
    click(&mut events, "the patch's Phabricator menu", "Submit to Phabricator as WIP");
    until(&mut events, "the review of the submit", |l| text(l).contains("D11: what is sent to Phabricator, for your review"));
    // A right click there opens its menu, as Esc does: its view of the
    // code, clicked, opens the dialog of the views of the interdiffs.
    ask(port, &host, &input, "m 60 12 r\n");
    click(&mut events, "the review's menu", "The view of the code…");
    // A click on what is compared opens its list; one on the code there
    // sets it, the code unified at once, as the middle draws a patch; Done
    // keeps it.
    click(&mut events, "what is compared, in the dialog", "Compare: [the two diffs]");
    click(&mut events, "the code in the list", "│ the code ");
    let mut seen = until(&mut events, "the code set", |l| text(l).contains("Compare: [the code]"));
    ask(port, &host, &input, "k End -\nk Enter -\n");
    let unified = |l: &str| text(l).contains(" · change unified");
    if !seen.iter().any(|l| unified(l)) {
        seen.extend(until(&mut events, "the code unified", unified));
    }
    // Esc opens the menu again; Cancel: send nothing there sends nothing.
    ask(port, &host, &input, "k Esc -\n");
    click(&mut events, "the review's menu again", "Cancel: send nothing");
    until(&mut events, "nothing sent", |l| text(l).contains("nothing was sent to Phabricator"));
    let asked = asked.lock().unwrap().clone();
    assert!(!asked.iter().any(|m| m.starts_with("differential.revision.edit") || m == "differential.creatediff"), "{asked:?}");
    stacked::submit::stand_in(None);
}

#[cfg(feature = "moz-phab")]
#[test]
fn a_patch_is_submitted_to_phabricator_from_its_menu_in_the_browser_too() {
    let _one = one_submit();
    let (tmp, db) = setup();
    let arc = r#"{"phabricator.uri": "https://phabricator.example.com/", "repository.callsign": "TEST"}"#;
    std::fs::write(tmp.path().join("src/.arcconfig"), arc).unwrap();
    // The editor served from this process: a stand-in for Phabricator,
    // which knows the repository alone, and says what was asked of it.
    let asked = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let seen = asked.clone();
    stacked::submit::stand_in(Some(Box::new(move |m, _| {
        seen.lock().unwrap().push(m.to_string());
        match m {
            "diffusion.repository.search" => Ok(serde_json::json!({"data": [{"phid": "PHID-REPO-1", "fields": {"vcs": "git"}}]})),
            _ => Err(format!("{m}: not asked in this test")),
        }
    })));
    stacked::config::init(stacked::config::Config::default());
    let (port, token) = stacked::web::start(db).unwrap();
    let (host, input) = (format!("127.0.0.1:{port}"), format!("POST /input?t={token}"));
    let mut events = events(port, &token, 100);
    until(&mut events, "the patches", |l| l.starts_with("data: r 3 ") && l.contains("Bug 2 - add b"));
    // The first patch's menu, opened with a right click, its `Phabricator`
    // item clicked, then the submit in that menu: what is sent is read,
    // then the dialog says it.
    ask(port, &host, &input, "m 5 2 r\n");
    let click = |events: &mut BufReader<TcpStream>, what: &str, item: &str| {
        let seen = until(events, what, |l| text(l).contains(item));
        let row = seen.last().unwrap();
        let y = row.strip_prefix("data: r ").and_then(|r| r.split(' ').next()).unwrap().to_string();
        let line = text(row);
        let x = line[..line.find(item).unwrap()].chars().count() + 1;
        ask(port, &host, &input, &format!("m {x} {y} l\n"));
    };
    click(&mut events, "the patch's menu", "Phabricator ");
    click(&mut events, "the patch's Phabricator menu", "Submit to Phabricator as WIP");
    let title = "send patch 1 to Phabricator as a new revision?";
    let seen_dialog = until(&mut events, "the dialog", |l| text(l).contains(title));
    let row = seen_dialog.last().unwrap().strip_prefix("data: r ").and_then(|r| r.split(' ').next()).unwrap().to_string();
    // Esc: nothing is sent, nothing having been but read.
    ask(port, &host, &input, "k Esc -\n");
    until(&mut events, "the dialog closed", |l| l.starts_with(&format!("data: r {row} ")) && !text(l).contains(title));
    assert_eq!(*asked.lock().unwrap(), ["diffusion.repository.search"]);
    stacked::submit::stand_in(None);
}

/// A stand-in for Phabricator, answering the fetches and the pulls of the
/// editors of `db` alone: D11 needs review, its latest diff the first
/// patch's, but `Two` where the patch says `TWO`; and, once what this gives
/// says so, ann's comment on it.
#[cfg(feature = "moz-phab")]
fn d11_needs_review(db: &Path) -> std::sync::Arc<std::sync::atomic::AtomicBool> {
    let d = stacked::db::Db::load(db).unwrap();
    let raw = d.patch(&d.stack("s").unwrap().patches[0]).diff.replace("+TWO", "+Two");
    let commented = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let said = commented.clone();
    stacked::phab::statuses_stand_in(
        db,
        Some(Box::new(move |m, _| match m {
            "differential.revision.search" => Ok(serde_json::json!({"data": [{"id": 11, "phid": "PHID-DREV-11",
                "fields": {"status": {"value": "needs-review"}, "title": "Bug 1 - shout two", "summary": ""}}]})),
            "edge.search" => Ok(serde_json::json!({"data": []})),
            "differential.diff.search" => Ok(serde_json::json!({"data": [{"id": 5, "fields": {"revisionPHID": "PHID-DREV-11"}}]})),
            "differential.querydiffs" => Ok(serde_json::json!({"5": {"sourceControlBaseRevision": "", "properties": {}}})),
            "differential.getrawdiff" => Ok(serde_json::Value::String(raw.clone())),
            "transaction.search" => {
                let ann = serde_json::json!({"type": "comment", "authorPHID": "PHID-USER-ann", "fields": {},
                    "comments": [{"phid": "c1", "version": 1, "dateCreated": 1, "content": {"raw": "Why shout?"}}]});
                let data = if said.load(std::sync::atomic::Ordering::SeqCst) { vec![ann] } else { vec![] };
                Ok(serde_json::json!({"data": data, "cursor": {"after": null}}))
            }
            "user.search" => Ok(serde_json::json!({"data": [{"phid": "PHID-USER-ann", "fields": {"username": "ann"}}]})),
            _ => Err(format!("{m}: not asked in this test").into()),
        })),
    );
    commented
}

#[cfg(feature = "moz-phab")]
#[test]
fn the_status_of_each_patch_s_revision_and_its_phabricator_line_show_in_the_browser_too() {
    let (_tmp, db) = setup_with("Bug 1 - shout two\n\nDifferential Revision: https://phabricator.example.com/D11");
    let commented = d11_needs_review(&db);
    stacked::config::init(stacked::config::Config::default());
    let (port, token) = stacked::web::start(db.clone()).unwrap();
    let (host, input) = (format!("127.0.0.1:{port}"), format!("POST /input?t={token}"));
    let mut events = events(port, &token, 100);
    // Nothing read as the page opens: the Phabricator line says that D11
    // is not fetched yet.
    until(&mut events, "the line, not fetched yet", |l| text(l).contains("Phabricator: D11 · not fetched yet:"));
    // Fetched from the stack's `Phabricator` menu, its items clicked.
    ask(port, &host, &input, "k Up -\n");
    ask(port, &host, &input, "k c 20 -\n");
    let click = |events: &mut BufReader<TcpStream>, what: &str, item: &str| {
        let seen = until(events, what, |l| text(l).contains(item) && text(l).contains('│'));
        let row = seen.last().unwrap();
        let y = row.strip_prefix("data: r ").and_then(|r| r.split(' ').next()).unwrap();
        let said = text(row);
        let x = said[..said.find(item).unwrap()].chars().count() + 1;
        ask(port, &host, &input, &format!("m {x} {y} l\n"));
    };
    click(&mut events, "the stack's menu", "Phabricator ");
    click(&mut events, "the stack's Phabricator menu", "Fetch the latest from Phabricator");
    ask(port, &host, &input, "k Down -\n");
    // The Phabricator line over the message, in the middle; and, on the
    // rows under it as they come with it, a letter after the number, then
    // ✗, as the line says D11's diff is otherwise; the patch naming no
    // revision blank, its title after the column all the same.
    let line = "Phabricator: D11 · Needs Review · the diff differs · no new comments";
    until(&mut events, "the Phabricator line", |l| text(l).contains(line));
    until(&mut events, "the status", |l| l.starts_with("data: r 2 ") && text(l).starts_with("  1 [R]✗ Bug 1 - shout two"));
    until(&mut events, "the title after it", |l| l.starts_with("data: r 3 ") && text(l).starts_with("  2      Bug 2 - add b"));
    // Listing files, a letter and a mark too, the title folded under itself
    // where the pane, as wide as listing files needs, is narrower.
    ask(port, &host, &input, "k Left -\n");
    until(&mut events, "the letter", |l| {
        l.starts_with("data: r 2 ") && text(l).starts_with("  1 [R]✗ Bug 1 - shout ") && !text(l).starts_with("  1 [R]✗ Bug 1 - shout two")
    });
    until(&mut events, "the title folded", |l| l.starts_with("data: r 3 ") && text(l).starts_with("         two "));
    // Space on the line: what differs, on a screen of its own, Phabricator's
    // line of the diff taken out; Esc, back on the line.
    for k in ["k Right -\n", "k Up -\n", "k c 20 -\n"] {
        ask(port, &host, &input, k);
    }
    until(&mut events, "what differs", |l| text(l).contains("D11 on Phabricator, then patch 1 as it is"));
    until(&mut events, "the line of Phabricator's diff", |l| text(l).starts_with("-+Two"));
    ask(port, &host, &input, "k Esc -\n");
    until(&mut events, "the line again", |l| text(l).contains(line));
    // Its menu: what differs, and D11 pulled, clicked: ann's comment, and
    // D11's diff as a new version of patch 1, held for review.
    commented.store(true, std::sync::atomic::Ordering::SeqCst);
    ask(port, &host, &input, "k Enter -\n");
    let item = "Pull D11 from Phabricator";
    let seen = until(&mut events, "the line's menu", |l| text(l).contains(item));
    assert!(seen.iter().any(|l| text(l).contains("What differs from D11 on Phabricator…")), "{seen:#?}");
    let row = seen.last().unwrap();
    let y = row.strip_prefix("data: r ").and_then(|r| r.split(' ').next()).unwrap();
    let said = text(row);
    let x = said[..said.find(item).unwrap()].chars().count() + 1;
    ask(port, &host, &input, &format!("m {x} {y} l\n"));
    until(&mut events, "what the pull brought", |l| text(l).contains("1 new version and 1 new comment from D11: they wait for your review"));
    // Its review opens from the banner: the new version, an operation of
    // the stack's change, and the comment.
    ask(port, &host, &input, "k Esc -\n");
    ask(port, &host, &input, "k Enter -\n");
    let seen = until(&mut events, "the pull's review", |l| text(l).contains("· modify  1 Bug 1 - shout two"));
    assert!(seen.iter().any(|l| text(l).starts_with(" pulled from Phabricator: ")), "{seen:#?}");
    stacked::phab::statuses_stand_in(&db, None);
}

/// Patch 1 of stack `s` in `db` given a new version whose message is
/// `message`, at once.
fn reworded(db: &Path, message: &str) {
    stacked::db::Db::edit(db, |d| {
        let old = d.patch(&d.stack("s")?.patches[0]).clone();
        let draft = stacked::mbox::Draft {
            message: message.into(),
            ..stacked::mbox::Draft::from(&old)
        };
        stacked::ops::replace(d, "s", 0, draft, "reword").map(|_| ())
    })
    .unwrap();
}

/// The program the editor served from this process opens web links with: a
/// stand-in, in `dir`, that opens nothing, writes each link it is given on
/// a line of the file this gives, and ends once no `hold` is beside it, for
/// the editor to be seen opening a link. No test opens a browser.
fn stand_in_opener(dir: &Path) -> PathBuf {
    let path = dir.join("open.sh");
    let script = "#!/bin/sh\nd=\"$(dirname \"$0\")\"\nprintf '%s\\n' \"$1\" >> \"$d/opened.txt\"\nwhile [ -e \"$d/hold\" ]; do sleep 0.05; done\n";
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    stacked::refs::open_links_with(&path);
    dir.join("opened.txt")
}

/// The links the stand-in (`stand_in_opener`) wrote in `log`, once it wrote
/// `n` of them, or gave up waiting.
fn links_opened(log: &Path, n: usize) -> Vec<String> {
    let started = Instant::now();
    loop {
        let all: Vec<String> = std::fs::read_to_string(log).unwrap_or_default().lines().map(String::from).collect();
        if all.len() >= n || started.elapsed() > PATIENCE {
            return all;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// A cell of `said` in row event `l`, the second of it: its column and its
/// row.
fn cell_of(l: &str, said: &str) -> (usize, String) {
    let y = l.strip_prefix("data: r ").and_then(|r| r.split(' ').next()).unwrap().to_string();
    let line = text(l);
    (line[..line.find(said).unwrap()].chars().count() + 1, y)
}

/// A comment of yours on the title of stack `s`'s first patch in `db`,
/// saying `said`.
fn on_title(db: &Path, said: &str) {
    stacked::db::Db::edit(db, |d| {
        let p = d.patch(&d.stack("s")?.patches[0]).clone();
        d.set_note(stacked::db::Note {
            change: p.change.clone(),
            id: "on-title".into(),
            line: stacked::view::title_key(&p),
            file: stacked::view::MSG_PATH.into(),
            preview: p.title().into(),
            text: said.into(),
            ..Default::default()
        })
    })
    .unwrap();
}

#[test]
fn a_link_of_a_line_of_a_message_or_of_a_comment_opens_from_its_menu_and_one_of_a_message_with_o_in_the_browser_too() {
    let (tmp, db) = setup();
    let here = tmp.path().file_name().unwrap().to_string_lossy().to_string();
    let why = format!("https://example.com/{here}/why");
    let docs = format!("https://example.com/{here}/docs");
    reworded(&db, &format!("Bug 1 - shout two\n\nSee {why}"));
    on_title(&db, &format!("Read [the docs]({docs}) first."));
    let log = stand_in_opener(tmp.path());
    stacked::config::init(stacked::config::Config::default());
    let (port, token) = stacked::web::start(db).unwrap();
    let (host, input) = (format!("127.0.0.1:{port}"), format!("POST /input?t={token}"));
    let mut events = events(port, &token, 100);
    // A right click on the line of the message opens its menu, and its item
    // opens the link, the status line saying so.
    let seen = until(&mut events, "the line", |l| text(l).contains("See https://"));
    let (x, y) = cell_of(seen.last().unwrap(), "See https://");
    // The comment, under the title, is drawn above it.
    let comment = "▎   Read the docs";
    let line = seen.iter().find(|l| text(l).contains(comment)).cloned();
    let line = line.unwrap_or_else(|| until(&mut events, "the comment's line", |l| text(l).contains(comment)).pop().unwrap());
    ask(port, &host, &input, &format!("m {x} {y} r\n"));
    let item = format!("Open {why} in your browser");
    let seen = until(&mut events, "the line's menu", |l| text(l).contains(&item));
    let (x, y) = cell_of(seen.last().unwrap(), &item);
    ask(port, &host, &input, &format!("m {x} {y} l\n"));
    until(&mut events, "the link opened", |l| text(l).contains(&format!("opened {why} in your browser")));
    assert_eq!(links_opened(&log, 1), std::slice::from_ref(&why));
    // o, the cursor on the line, opens it again: the status line says so
    // while the stand-in is held, then once it has, the editor free to open
    // the next.
    let hold = tmp.path().join("hold");
    std::fs::write(&hold, "").unwrap();
    ask(port, &host, &input, "k c 6f -\n");
    until(&mut events, "the link opening", |l| text(l).contains(&format!("opening {why}…")));
    std::fs::remove_file(&hold).unwrap();
    until(&mut events, "the link opened again", |l| text(l).contains(&format!("opened {why} in your browser")));
    assert_eq!(links_opened(&log, 2), [why.clone(), why.clone()]);
    // A right click on the line of the comment: its menu has the link,
    // named by its text, which opens it.
    let (x, y) = cell_of(&line, comment);
    ask(port, &host, &input, &format!("m {x} {y} r\n"));
    let item = "Open \"the docs\" in your browser";
    let seen = until(&mut events, "the comment's menu", |l| text(l).contains(item));
    let (x, y) = cell_of(seen.last().unwrap(), item);
    ask(port, &host, &input, &format!("m {x} {y} l\n"));
    until(&mut events, "the comment's link opened", |l| text(l).contains(&format!("opened {docs} in your browser")));
    assert_eq!(links_opened(&log, 3), [why.clone(), why, docs]);
}

#[test]
fn a_message_s_long_lines_are_wrapped_from_its_patch_s_menu_in_the_browser_too() {
    let long = "Bug 1 - shout two\n\nThe parser read the whole file before it checked the header, which made it slow on the big files we have.";
    let (_tmp, db) = setup_with(long);
    let message = |db: &Path| {
        let d = stacked::db::Db::load(db).unwrap();
        d.patch(&d.stack("s").unwrap().patches[0]).message.clone()
    };
    let before = message(&db);
    assert!(before.contains("which made it slow"), "{before}");
    stacked::config::init(stacked::config::Config::default());
    let (port, token) = stacked::web::start(db.clone()).unwrap();
    let (host, input) = (format!("127.0.0.1:{port}"), format!("POST /input?t={token}"));
    let mut events = events(port, &token, 100);
    until(&mut events, "the patches", |l| l.starts_with("data: r 3 ") && l.contains("Bug 2 - add b"));
    // The first patch's menu, opened with a right click, offers it; its item
    // clicked, the edit lands for review, its review open at once.
    ask(port, &host, &input, "m 5 2 r\n");
    let item = "Wrap the long lines of this patch's message at 72 columns";
    let seen = until(&mut events, "the patch's menu", |l| text(l).contains(item));
    let row = seen.last().unwrap();
    let y = row.strip_prefix("data: r ").and_then(|r| r.split(' ').next()).unwrap();
    let line = text(row);
    let x = line[..line.find(item).unwrap()].chars().count() + 1;
    ask(port, &host, &input, &format!("m {x} {y} l\n"));
    until(&mut events, "the edit landed", |l| text(l).contains("at 72 columns: it waits for your review"));
    // Nothing changes in the stack before the review accepts it.
    assert_eq!(message(&db), before);
    let d = stacked::db::Db::load(&db).unwrap();
    let queue = stacked::held::queue(&d, "s");
    assert!(queue.len() == 1 && stacked::held::by_hand(queue[0]), "{queue:?}");
}

#[test]
fn move_head_says_why_it_does_not_move_head_in_the_browser_too() {
    let (tmp, db) = setup();
    let src = tmp.path().join("src");
    // A commit of yours on top: Move HEAD would not move HEAD.
    std::fs::write(src.join("c.txt"), "c\n").unwrap();
    git(&src, &["add", "c.txt"]);
    git(&src, &["commit", "-qm", "Mine"]);
    // `stacked web`, run in the checkout, its home the test's.
    let mut child = stacked(&tmp.path().join("home"))
        .args(["-f", db.to_str().unwrap(), "web", "--no-open"])
        .current_dir(&src)
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut said = BufReader::new(child.stderr.take().unwrap());
    let _child = Running(child);
    let mut line = String::new();
    said.read_line(&mut line).unwrap();
    let url = line.split_whitespace().find_map(|w| w.strip_prefix("http://127.0.0.1:")).expect(&line).to_string();
    let (port, token) = url.split_once("/?t=").unwrap();
    let port: u16 = port.parse().unwrap();
    let (host, input) = (format!("127.0.0.1:{port}"), format!("POST /input?t={token}"));
    let mut events = events(port, token, 100);
    until(&mut events, "HEAD forked", |l| text(l).contains("└○ Mine"));
    // The stack's menu, opened with a right click on its header: Move HEAD…
    // drawn as any item, not dim (2); clicked, the view of why it does not
    // move HEAD.
    ask(port, &host, &input, "m 5 1 r\n");
    let item = "Move HEAD…";
    let seen = until(&mut events, "the stack's menu", |l| text(l).contains(item));
    let row = seen.last().unwrap();
    assert!(row.contains(&format!(",,0\u{1f} {item}")), "{row}");
    let y = row.strip_prefix("data: r ").and_then(|r| r.split(' ').next()).unwrap();
    let line = text(row);
    let x = line[..line.find(item).unwrap()].chars().count() + 1;
    ask(port, &host, &input, &format!("m {x} {y} l\n"));
    until(&mut events, "the view", |l| text(l).contains(" why Move HEAD does not move HEAD"));
    // ↓: the commit, then what to do, the command on a line of its own.
    ask(port, &host, &input, "k Down -\n");
    until(&mut events, "the command", |l| text(l).contains("      git switch --detach "));
    // Esc: back to the patches. Then quit, in the page.
    ask(port, &host, &input, "k Esc -\n");
    until(&mut events, "the patches again", |l| text(l).contains("└○ Mine"));
    ask(port, &host, &input, "k c 63 c\n");
    until(&mut events, "bye", |l| l == "data: bye");
}

#[test]
fn head_s_row_dragged_onto_a_patch_puts_head_down_there_in_the_browser_too() {
    let (tmp, db) = setup();
    let src = tmp.path().join("src");
    // `stacked web`, run in the checkout, its home the test's.
    let mut child = stacked(&tmp.path().join("home"))
        .args(["-f", db.to_str().unwrap(), "web", "--no-open"])
        .current_dir(&src)
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut said = BufReader::new(child.stderr.take().unwrap());
    let _child = Running(child);
    let mut line = String::new();
    said.read_line(&mut line).unwrap();
    let url = line.split_whitespace().find_map(|w| w.strip_prefix("http://127.0.0.1:")).expect(&line).to_string();
    let (port, token) = url.split_once("/?t=").unwrap();
    let port: u16 = port.parse().unwrap();
    let (host, input) = (format!("127.0.0.1:{port}"), format!("POST /input?t={token}"));
    let mut events = events(port, token, 100);
    // HEAD's row after patch 2, and patch 1's line, in the patches.
    let found = std::cell::RefCell::new((None, None));
    until(&mut events, "HEAD's row and patch 1's", |l| {
        let mut found = found.borrow_mut();
        if text(l).contains("▶ HEAD main") {
            found.0 = Some(cell_of(l, "▶ HEAD"));
        }
        if text(l).contains(" 1 Bug 1 - shout two") {
            found.1 = Some(cell_of(l, "Bug 1 - shout two"));
        }
        found.0.is_some() && found.1.is_some()
    });
    let (head, one) = found.into_inner();
    let ((hx, hy), (ox, oy)) = (head.unwrap(), one.unwrap());
    // Pressed on HEAD's row, which the page says as a click, dragged onto
    // patch 1 with the button down, then let go there: HEAD put down under
    // patch 1, as Enter puts it down.
    ask(port, &host, &input, &format!("v {hx} {hy}\nm {hx} {hy} l\nd {ox} {oy}\nu {ox} {oy}\n"));
    until(&mut events, "HEAD moved", |l| text(l).contains("HEAD is at patch 1 of s, detached"));
    ask(port, &host, &input, "k c 63 c\n");
    until(&mut events, "bye", |l| l == "data: bye");
}

#[test]
fn the_stack_s_lint_is_offered_on_its_menu_in_the_browser_too() {
    let (tmp, db) = setup();
    let src = tmp.path().join("src");
    std::fs::create_dir_all(src.join(".stacked")).unwrap();
    std::fs::write(src.join(".stacked/local.toml"), "[lint]\ncommand = \"./lint.sh --fix\"\n").unwrap();
    stacked::config::init(stacked::config::Config::default());
    let (port, token) = stacked::web::start(db).unwrap();
    let (host, input) = (format!("127.0.0.1:{port}"), format!("POST /input?t={token}"));
    let mut events = events(port, &token, 100);
    until(&mut events, "the patches", |l| l.starts_with("data: r 3 ") && l.contains("Bug 2 - add b"));
    // The stack's menu, opened with a right click on its header, names the
    // linter of the stack's checkout. Nothing is linted here: the item is
    // not clicked.
    ask(port, &host, &input, "m 5 1 r\n");
    until(&mut events, "the stack's menu", |l| text(l).contains("Lint the stack (./lint.sh --fix)"));
}

#[test]
fn quitting_in_the_page_removes_the_worktrees_made_ahead() {
    let (tmp, db) = setup();
    let made = made_ahead(tmp.path());
    stacked::config::init(stacked::config::Config::default());
    let (port, token) = stacked::web::start(db).unwrap();
    let mut events = events(port, &token, 400);
    until(&mut events, "the worktree made ahead", |l| l.contains(MADE));
    assert!(made.join(".git").exists(), "made ahead");
    // ctrl-c in the page is Quit: the page told, the worktree removed.
    let host = format!("127.0.0.1:{port}");
    ask(port, &host, &format!("POST /input?t={token}"), "k c 63 c\n");
    until(&mut events, "bye", |l| l == "data: bye");
    assert!(gone(&made), "removed once the editor is left");
}

#[test]
fn ctrl_c_where_it_runs_leaves_it_as_quit_does() {
    let (tmp, db) = setup();
    let made = made_ahead(tmp.path());
    let (child, said, mut events, port, token) = web(tmp.path(), &db, "INT");
    assert!(made.join(".git").exists(), "made ahead");
    // A ctrl-c while $EDITOR runs is the editor's: the editor served stays,
    // ↓ picking the second patch once the edit is done.
    edit_first_patch(port, &token, &mut events);
    until(&mut events, "the edit done", |l| text(l).contains("nothing changed"));
    ask(port, &format!("127.0.0.1:{port}"), &format!("POST /input?t={token}"), "k Down -\n");
    until(&mut events, "the second patch", |l| l.contains("+bee"));
    assert!(tmp.path().join("edited").exists(), "$EDITOR ran");
    // Else, SIGINT leaves it as Quit does.
    send(&child, "INT");
    left(child, said, &mut events, &made);
}

#[test]
fn a_kill_or_its_terminal_closing_leaves_it_as_quit_does() {
    // A kill while $EDITOR runs: left once the editor is done.
    let (tmp, db) = setup();
    let made = made_ahead(tmp.path());
    let (child, said, mut events, port, token) = web(tmp.path(), &db, "TERM");
    edit_first_patch(port, &token, &mut events);
    left(child, said, &mut events, &made);
    // Its terminal closed: SIGHUP, left so too.
    let (tmp, db) = setup();
    let made = made_ahead(tmp.path());
    let (child, said, mut events, _, _) = web(tmp.path(), &db, "HUP");
    send(&child, "HUP");
    left(child, said, &mut events, &made);
}

/// The program the editors served from this process run as claude: one no
/// folder of the PATH holds, so that their ask dialogs gray Ask Now, saying
/// so, and check no network; no agent is ever asked from here.
const NO_CLAUDE: &str = "stacked-no-such-claude";

fn no_claude() {
    stacked::ask::stand_in(Path::new(NO_CLAUDE), Path::new(env!("CARGO_BIN_EXE_stacked")));
}

/// Stack `s` of `db` served from this process, with a comment on the stack,
/// its menu opened on its header: the server's port, its token, and its
/// events, 120 columns wide. Its claude is `NO_CLAUDE`.
fn served_with_a_stack_comment(home: &Path, db: &Path) -> (u16, String, BufReader<TcpStream>) {
    let out = stacked(home).args(["-f", db.to_str().unwrap(), "comment", "s", "Why?"]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    no_claude();
    stacked::config::init(stacked::config::Config::default());
    let (port, token) = stacked::web::start(db.to_path_buf()).unwrap();
    let mut events = events(port, &token, 120);
    until(&mut events, "the patches", |l| l.contains("Bug 2 - add b"));
    let input = format!("POST /input?t={token}");
    ask(port, &format!("127.0.0.1:{port}"), &input, "k Home -\n");
    until(&mut events, "the stack in the middle", |l| text(l).contains("the stack s · 2 patches"));
    ask(port, &format!("127.0.0.1:{port}"), &input, "k c 20 -\n");
    (port, token, events)
}

#[test]
fn the_ask_dialog_opens_with_nothing_set_is_clicked_and_asks_later_in_the_browser_too() {
    let (tmp, db) = setup();
    let home = tmp.path().join("home");
    let (port, token, mut events) = served_with_a_stack_comment(&home, &db);
    let (host, input) = (format!("127.0.0.1:{port}"), format!("POST /input?t={token}"));
    let send = |lines: &[&str]| {
        for l in lines {
            ask(port, &host, &input, &format!("{l}\n"));
        }
    };
    let click = |seen: &[String], said: &str| {
        let (x, y) = cell_of(seen.iter().rev().find(|l| text(l).contains(said)).unwrap(), said);
        send(&[format!("m {x} {y} l").as_str()]);
    };
    // Ask agent about all…, clicked: its dialog, nothing set, Ask Now
    // grayed as claude is not found, Ask Later until both are set.
    let item = "Ask agent about all…";
    click(&until(&mut events, "the stack's menu", |l| text(l).contains(item)), item);
    let seen = until(&mut events, "the dialog", |l| text(l).contains("Permission: [ — ]"));
    assert!(seen.iter().any(|l| text(l).contains("ask the agent about the comments listed on the stack")), "{seen:#?}");
    until(&mut events, "its buttons", |l| text(l).contains("< Ask Now >   < Ask Later >   < Cancel >"));
    until(&mut events, "why Ask Now is grayed", |l| text(l).contains(&format!("Ask Now is grayed: {NO_CLAUDE} is not found")));
    // A click on the permission's line opens the list of its options under
    // it, and a click on one of them picks it: answer only, then, the list
    // opened again, change stack.
    click(&seen, "Permission: [ — ]");
    let seen = until(&mut events, "the permission's list", |l| text(l).contains("│ answer only "));
    click(&seen, "│ answer only ");
    let seen = until(&mut events, "answer only", |l| text(l).contains("Permission: [answer only]"));
    click(&seen, "Permission: [answer only]");
    let seen = until(&mut events, "the list again", |l| text(l).contains("│ change stack "));
    click(&seen, "│ change stack ");
    until(&mut events, "change stack", |l| text(l).contains("Permission: [change stack]"));
    // → steps it to the next, ← back.
    send(&["k Right -"]);
    until(&mut events, "build and test", |l| text(l).contains("Permission: [build and test]"));
    send(&["k Left -"]);
    until(&mut events, "change stack again", |l| text(l).contains("Permission: [change stack]"));
    // Enter on the effort's line opens its list, on low: ↑ goes round to
    // max, ↓ back round to low, and Enter sets it.
    send(&["k Down -", "k Enter -"]);
    until(&mut events, "the effort's list", |l| text(l).contains("│ xhigh  │"));
    send(&["k Up -", "k Down -", "k Enter -"]);
    until(&mut events, "low", |l| text(l).contains("Effort: [low]"));
    // The buttons: → from Ask Now, grayed, to Ask Later, which batches it.
    send(&["k Down -", "k Right -", "k Enter -"]);
    until(&mut events, "batched", |l| text(l).contains("batched (may change the code): 1 ask in the batch"));
    let d = stacked::db::Db::load(&db).unwrap();
    let q = d.stack_notes("s").into_iter().find_map(|n| n.queued.clone()).expect("batched");
    assert_eq!((q.batched(), q.way, q.effort), (true, stacked::db::Way::Change, Some(stacked::db::Effort::Low)));
}

#[test]
fn the_ask_dialog_s_esc_cancels_and_the_effort_of_send_it_now_stops_at_either_end_in_the_browser_too() {
    let (tmp, db) = setup();
    let home = tmp.path().join("home");
    let (port, token, mut events) = served_with_a_stack_comment(&home, &db);
    let (host, input) = (format!("127.0.0.1:{port}"), format!("POST /input?t={token}"));
    let send = |lines: &[&str]| {
        for l in lines {
            ask(port, &host, &input, &format!("{l}\n"));
        }
    };
    // Ask agent about all… with Enter, both lines set with →, then Esc:
    // closed, nothing asked, as Space opens the menu again; opened again,
    // nothing set.
    until(&mut events, "the stack's menu", |l| text(l).contains("Ask agent about all…"));
    send(&["k Down -", "k Enter -"]);
    until(&mut events, "the dialog", |l| text(l).contains("Permission: [ — ]"));
    send(&["k Right -", "k Down -", "k Right -", "k Esc -", "k c 20 -"]);
    until(&mut events, "the menu again", |l| text(l).contains("Ask agent about all…"));
    send(&["k Down -", "k Enter -"]);
    until(&mut events, "opened again, unset", |l| text(l).contains("Permission: [ — ]"));
    // Asked later, at low, from the dialog, each line set with →: the
    // comment's own menu, in the comments pane, then has Send it now, at
    // the effort it was batched at.
    send(&["k Right -", "k Down -", "k Right -", "k Down -", "k Right -", "k Enter -"]);
    until(&mut events, "batched", |l| text(l).contains("batched (answer only)"));
    send(&["k Right -", "k Right -", "k c 20 -"]);
    let sending = |l: &str, level: &str| {
        let t = text(l);
        t.contains("Send it now (answer only)") && t.contains(&format!("Effort [{level}]"))
    };
    let click = |seen: Vec<String>| {
        let row = seen.last().unwrap();
        let (x, y) = cell_of(row, "Effort [");
        send(&[format!("m {x} {y} l").as_str()]);
    };
    // A click on it, the next level, the menu on it; ← no further than low;
    // → no further than max, and a click there goes round, to low.
    click(until(&mut events, "low, as batched", |l| sending(l, "low")));
    until(&mut events, "medium, clicked", |l| sending(l, "medium"));
    send(&["k Left -"; 3]);
    until(&mut events, "low, no further", |l| sending(l, "low"));
    send(&["k Right -"; 5]);
    click(until(&mut events, "max", |l| sending(l, "max")));
    until(&mut events, "low, clicked", |l| sending(l, "low"));
    // Nothing kept for the next runs: the state of this process is nowhere.
    assert_eq!(stacked::home::get().state, Path::new(stacked::home::NOWHERE));
}

#[test]
fn the_database_the_editor_opens_is_on_the_list_stacked_s_open_recent_reads() {
    let (tmp, _) = setup();
    std::fs::copy(tmp.path().join("db.jsonl"), tmp.path().join("other.jsonl")).unwrap();
    let home = tmp.path().join("home");
    // Two at once, each on a database named from where it runs, as a
    // terminal's `stacked web` is.
    let serve = |name: &str| {
        let mut child = stacked(&home)
            .args(["-f", name, "web", "--no-open"])
            .current_dir(tmp.path())
            .stdin(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut said = BufReader::new(child.stderr.take().unwrap());
        let child = Running(child);
        let mut line = String::new();
        said.read_line(&mut line).unwrap();
        let url = line.split_whitespace().find_map(|w| w.strip_prefix("http://127.0.0.1:")).expect(&line).to_string();
        (child, url)
    };
    let both = [serve("db.jsonl"), serve("other.jsonl")];
    for (_, url) in &both {
        let (port, token) = url.split_once("/?t=").unwrap();
        let mut events = events(port.parse().unwrap(), token, 100);
        until(&mut events, "the patches", |l| l.contains("Bug 2 - add b"));
    }
    // Each from the root, on the list StackED's File → Open Recent shows.
    let state = home.join("state.json");
    let mut listed = stacked::state::recent(&state);
    listed.sort();
    let root = tmp.path().canonicalize().unwrap();
    assert_eq!(listed, [root.join("db.jsonl"), root.join("other.jsonl")]);
    #[cfg(feature = "app")]
    {
        let (to, _) = std::sync::mpsc::channel();
        let mut menu = stacked::app::windows::File::new(to, Box::new(|| {}), state).recent();
        menu.sort();
        assert_eq!(menu, listed, "as StackED's menu lists them");
    }
}

/// `stacked web --no-open` on `db`, `args` after it, run in `dir`, its home
/// `home`, what it says on stderr piped.
fn web_in(home: &Path, dir: &Path, db: &Path, args: &[&str]) -> Command {
    let mut cmd = stacked(home);
    cmd.args(["-f", db.to_str().unwrap(), "web", "--no-open"]).args(args).current_dir(dir).stdin(Stdio::null()).stderr(Stdio::piped());
    cmd
}

/// `stacked web`, run, once it said where it is: what it said before, a
/// line each, and the port and the token of the address it said.
struct Served {
    child: Running,
    said: BufReader<ChildStderr>,
    before: Vec<String>,
    port: u16,
    token: String,
}

/// `stacked web --no-open` on `db`, `args` after it, run in `dir`, its home
/// `home`, as it says where it is; or, if it ends first, all it said, a
/// line each.
fn try_served(home: &Path, dir: &Path, db: &Path, args: &[&str]) -> Result<Served, Vec<String>> {
    started(web_in(home, dir, db, args))
}

/// `stacked web`, as `cmd` runs it (`web_in`), as it says where it is; or,
/// if it ends first, all it said, a line each.
fn started(mut cmd: Command) -> Result<Served, Vec<String>> {
    let mut child = cmd.spawn().unwrap();
    let said = BufReader::new(child.stderr.take().unwrap());
    said_where(Running(child), said)
}

/// `stacked web`, run as `child`, as it says where it is, on stderr,
/// `said`; or, if it ends first, all it said, a line each.
fn said_where(child: Running, mut said: BufReader<ChildStderr>) -> Result<Served, Vec<String>> {
    let mut before = Vec::new();
    loop {
        let mut line = String::new();
        if said.read_line(&mut line).unwrap() == 0 {
            return Err(before);
        }
        // "the editor is at http://127.0.0.1:PORT/?t=TOKEN".
        if let Some(url) = line.split_whitespace().find_map(|w| w.strip_prefix("http://127.0.0.1:")) {
            let (port, token) = url.split_once("/?t=").expect(&line);
            let (port, token) = (port.parse().unwrap(), token.to_string());
            return Ok(Served { child, said, before, port, token });
        }
        before.push(line.trim_end().to_string());
    }
}

/// As `try_served`, which is to say where it is.
fn served(home: &Path, dir: &Path, db: &Path, args: &[&str]) -> Served {
    try_served(home, dir, db, args).unwrap_or_else(|said| panic!("it never said where it is: {said:#?}"))
}

/// What `stacked web` says as it ends when `port` is in use.
fn in_use(port: u16) -> String {
    format!("stacked: port {port} of 127.0.0.1 is in use")
}

/// The launch `tried`, unless it ended saying that `port`, free a moment
/// ago, is in use: another process took it first, and None says so.
fn unless_taken(tried: Result<Served, Vec<String>>, port: u16) -> Option<Served> {
    match tried {
        Ok(s) => Some(s),
        Err(said) if said == [in_use(port)] => None,
        Err(said) => panic!("it never said where it is: {said:#?}"),
    }
}

/// What `attempt` makes of a port free a moment ago, tried again while it
/// gives back None, as another process took the port first (`unless_taken`):
/// up to ten times, so that only `stacked web` itself can fail it.
fn while_taken<T>(mut attempt: impl FnMut() -> Option<T>) -> T {
    (0..10).find_map(|_| attempt()).expect("another process took each of ten ports free a moment ago first")
}

impl Served {
    /// The status line of the page it serves, asked for with `token`.
    fn page(&self, token: &str) -> String {
        ask(self.port, &format!("127.0.0.1:{}", self.port), &format!("GET /?t={token}"), "").0
    }

    /// Quit in the page, as ctrl-c there quits: waited for until it has
    /// exited, and its port is free again.
    fn quit(mut self) {
        let host = format!("127.0.0.1:{}", self.port);
        ask(self.port, &host, &format!("POST /input?t={}", self.token), "k c 63 c\n");
        let status = exit_of(&mut self.child);
        let mut rest = String::new();
        self.said.read_to_string(&mut rest).unwrap();
        assert!(status.success(), "{status}: {rest}");
    }

    /// Whether it still runs.
    fn running(&mut self) -> bool {
        self.child.0.try_wait().unwrap().is_none()
    }

    /// A page of its gone, as its tab is closed or loaded again: its
    /// events, `page`, closed, and it says it goes, as the page does once it
    /// is hidden for good.
    fn close(&self, page: BufReader<TcpStream>) {
        drop(page);
        ask(self.port, &format!("127.0.0.1:{}", self.port), &format!("POST /input?t={}", self.token), "g\n");
    }

    /// Left by itself as Quit leaves it, its last tab closed at `closed`:
    /// waited for until it has exited, no sooner than 2 seconds after, and
    /// said why.
    fn quits(mut self, closed: Instant) {
        let status = exit_of(&mut self.child);
        let after = closed.elapsed();
        let mut rest = String::new();
        self.said.read_to_string(&mut rest).unwrap();
        assert!(status.success(), "{status}: {rest}");
        assert!(after >= Duration::from_secs(2), "quit {after:?} after");
        assert!(rest.lines().any(|l| l == CLOSED), "{rest}");
    }
}

/// How `child` exited, waited for.
fn exit_of(child: &mut Running) -> std::process::ExitStatus {
    let started = Instant::now();
    loop {
        if let Some(s) = child.0.try_wait().unwrap() {
            return s;
        }
        assert!(started.elapsed() < PATIENCE, "still running");
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// `stacked web --no-open` on `db`, `args` after it, run in `dir`, its home
/// `home`, which is to exit by itself: how it exited, and all it said.
fn exited(home: &Path, dir: &Path, db: &Path, args: &[&str]) -> (std::process::ExitStatus, String) {
    let mut child = web_in(home, dir, db, args).spawn().unwrap();
    let mut said = child.stderr.take().unwrap();
    let status = exit_of(&mut Running(child));
    let mut all = String::new();
    said.read_to_string(&mut all).unwrap();
    (status, all)
}

/// A port of 127.0.0.1 that was free a moment ago: the one the system gives
/// for 0, as `stacked web` takes any free one, given back at once.
fn free_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0)).unwrap().local_addr().unwrap().port()
}

/// A port of 127.0.0.1 in use for as long as the listener this gives with
/// it is held.
fn held_port() -> (TcpListener, u16) {
    let held = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = held.local_addr().unwrap().port();
    (held, port)
}

/// The settings' file of `stacked` run with `home` (`$STACKED_HOME`), made
/// to hold `text`.
fn settings_of(home: &Path, text: &str) -> PathBuf {
    std::fs::create_dir_all(home).unwrap();
    let settings = home.join("config.toml");
    std::fs::write(&settings, text).unwrap();
    settings
}

#[test]
fn stacked_web_listens_on_any_free_port_each_time_unless_the_settings_say_otherwise() {
    let (tmp, db) = setup();
    let home = tmp.path().join("home");
    let settings = home.join("config.toml");
    // Unset, as before there was the setting, then set so: any free port
    // each time, the one taken before held meanwhile, and nothing written.
    for written in [None, Some("[web]\nport = \"random\"\n")] {
        if let Some(text) = written {
            settings_of(&home, text);
        }
        let first = served(&home, tmp.path(), &db, &[]);
        let port = first.port;
        // Its address kept for no launch after, as its events say.
        let mut page = events(port, &first.token, 100);
        let seen = until(&mut page, "which stacked", |l| l.starts_with("data: v "));
        assert_eq!(version(seen.last().unwrap()).2, "0");
        first.quit();
        // Held here, unless another took it meanwhile: either way, not free.
        let _held = TcpListener::bind(("127.0.0.1", port)).ok();
        let next = served(&home, tmp.path(), &db, &[]);
        assert!(next.before.is_empty() && next.port != port, "{:?}, on {port}", next.before);
        assert_eq!(next.page(&next.token), "HTTP/1.1 200 OK");
        next.quit();
        assert_eq!(std::fs::read_to_string(&settings).ok().as_deref(), written);
        // A token new each time, kept nowhere.
        assert!(!web_token(&home).exists(), "a token kept");
    }
}

#[test]
fn fixed_random_takes_any_free_port_the_first_time_writes_it_in_the_settings_and_listens_on_it_after_on_any_database() {
    let (tmp, db) = setup();
    let other = tmp.path().join("other.jsonl");
    std::fs::copy(&db, &other).unwrap();
    let home = tmp.path().join("home");
    let mine = "# mine\n[web]\nport = \"fixed-random\"\n";
    let settings = settings_of(&home, mine);
    let kept = |port: u16| format!("{mine}kept_port = {port}\n");
    let databases = || (std::fs::read(&db).unwrap(), std::fs::read(&other).unwrap());
    let before = databases();
    // The first launch: any free port, written beside the setting, the rest
    // of the file as it was. The next, on another database: that port
    // again, as the settings say. Should another process take the port
    // between the two, both are tried again, from the settings as they were.
    let ((port, token), next) = while_taken(|| {
        settings_of(&home, mine);
        let first = served(&home, tmp.path(), &db, &[]);
        assert!(first.before.is_empty(), "{:?}", first.before);
        assert_eq!(std::fs::read_to_string(&settings).unwrap(), kept(first.port));
        let (port, token) = (first.port, first.token.clone());
        first.quit();
        unless_taken(try_served(&home, tmp.path(), &other, &[]), port).map(|next| ((port, token), next))
    });
    // The first one's token kept for the port: the tab opened with it given
    // 2 seconds to come back, and, none here, a new one, the browser opened
    // on nothing as `--no-open` says, the first one's refused there.
    assert_eq!(next.before, [waiting(port, false), renewed()]);
    assert_eq!(next.port, port);
    assert_ne!(next.token, token);
    assert_eq!(next.page(&token), "HTTP/1.1 403 Forbidden");
    assert_eq!(next.page(&next.token), "HTTP/1.1 200 OK");
    assert_eq!(kept_in(&web_token(&home)), Some((port, next.token.clone())));
    next.quit();
    // The port kept in the settings alone, its token beside the state:
    // nothing written in the databases, nor in the state, which holds the
    // databases opened, as ever.
    assert_eq!(std::fs::read_to_string(&settings).unwrap(), kept(port));
    assert!(databases() == before, "a database was written");
    let state: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(home.join("state.json")).unwrap()).unwrap();
    assert_eq!(state.as_object().unwrap().keys().collect::<Vec<_>>(), ["recent", "open"], "{state}");
}

#[test]
fn a_port_set_is_listened_on_and_one_given_with_port_goes_over_the_settings() {
    let (tmp, db) = setup();
    let home = tmp.path().join("home");
    let (_held, used) = held_port();
    // A port set, free a moment ago: that one, the port kept for
    // fixed-random, in use, passed over.
    let (free, set) = while_taken(|| {
        let free = free_port();
        settings_of(&home, &format!("[web]\nport = {free}\nkept_port = {used}\n"));
        unless_taken(try_served(&home, tmp.path(), &db, &[]), free).map(|s| (free, s))
    });
    assert_eq!((set.port, set.page(&set.token).as_str()), (free, "HTTP/1.1 200 OK"));
    let token = set.token.clone();
    set.quit();
    // Its token kept for that port.
    let tokens = web_token(&home);
    assert_eq!(kept_in(&tokens), Some((free, token.clone())));
    let kept_then = std::fs::read(&tokens).unwrap();
    // --port giving that very port: a token of its own, the one kept
    // neither used nor waited for; unless another process took the port
    // meanwhile.
    if let Some(s) = unless_taken(try_served(&home, tmp.path(), &db, &["--port", &free.to_string()]), free) {
        assert!(s.before.is_empty(), "{:?}", s.before);
        assert_ne!(s.token, token);
        s.quit();
    }
    // Over a port set that is in use, --port: the port it gives, any free
    // one for 0.
    let settings = settings_of(&home, &format!("[web]\nport = {used}\n"));
    let (given, s) = while_taken(|| {
        let given = free_port();
        unless_taken(try_served(&home, tmp.path(), &db, &["--port", &given.to_string()]), given).map(|s| (given, s))
    });
    assert_eq!(s.port, given);
    s.quit();
    let s = served(&home, tmp.path(), &db, &["--port", "0"]);
    assert_eq!(s.page(&s.token), "HTTP/1.1 200 OK");
    s.quit();
    // Over fixed-random, none kept yet: nothing kept by that launch.
    settings_of(&home, "[web]\nport = \"fixed-random\"\n");
    served(&home, tmp.path(), &db, &["--port", "0"]).quit();
    assert_eq!(std::fs::read_to_string(&settings).unwrap(), "[web]\nport = \"fixed-random\"\n");
    // Nor any token: the one kept as it was.
    assert!(std::fs::read(&tokens).unwrap() == kept_then, "--port wrote the token kept");
}

#[test]
fn a_port_in_use_is_said_in_a_line_and_stacked_web_does_not_start() {
    let (tmp, db) = setup();
    let home = tmp.path().join("home");
    let (_held, port) = held_port();
    // Said, no other port tried, the settings as they were.
    let refused = |written: &str, args: &[&str]| {
        let settings = settings_of(&home, written);
        let (status, said) = exited(&home, tmp.path(), &db, args);
        assert_eq!((status.code(), said), (Some(1), format!("{}\n", in_use(port))), "{written:?} {args:?}");
        assert_eq!(std::fs::read_to_string(&settings).unwrap(), written);
    };
    // Set, kept for fixed-random, or given.
    refused(&format!("[web]\nport = {port}\n"), &[]);
    refused(&format!("[web]\nport = \"fixed-random\"\nkept_port = {port}\n"), &[]);
    refused("", &["--port", &port.to_string()]);
}

#[test]
fn a_port_fixed_random_cannot_keep_in_the_settings_is_said_and_listened_on_all_the_same() {
    let (tmp, db) = setup();
    let home = tmp.path().join("home");
    // A table's header as stacked does not write one: the setting written
    // beside it would not read back.
    let written = "[ web ]\nport = \"fixed-random\"\n";
    let settings = settings_of(&home, written);
    let s = served(&home, tmp.path(), &db, &[]);
    let said = format!(
        "stacked: port {} cannot be kept in the settings as web.kept_port, so the next launch with web.port = \"fixed-random\" takes any free port again: {}: ",
        s.port,
        settings.display()
    );
    assert!(s.before.first().is_some_and(|l| l.starts_with(&said)), "{:#?}", s.before);
    assert!(s.before.last().is_some_and(|l| l.ends_with("; nothing written")), "{:#?}", s.before);
    assert_eq!(s.page(&s.token), "HTTP/1.1 200 OK");
    s.quit();
    assert_eq!(std::fs::read_to_string(&settings).unwrap(), written);
}

/// Where `stacked` run with `home` (`$STACKED_HOME`) keeps the address of
/// `stacked web`, its port fixed by the settings: beside the state.
fn web_token(home: &Path) -> PathBuf {
    home.join("web-token.json")
}

/// What the file at `file` keeps, as `stacked web` keeps it: the port and
/// its token; none without the file.
fn kept_in(file: &Path) -> Option<(u16, String)> {
    let kept: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(file).ok()?).unwrap();
    Some((kept["port"].as_u64().unwrap() as u16, kept["token"].as_str().unwrap().to_string()))
}

/// The mode of the file at `file`, its permissions alone.
fn mode_of(file: &Path) -> u32 {
    std::fs::metadata(file).unwrap().permissions().mode() & 0o777
}

/// What event `l`, `v BUILD LAUNCH KEPT`, says of the stacked sending the
/// events: its build, its launch, and whether it keeps its address for the
/// next launch, `1`, or not, `0`.
fn version(l: &str) -> (String, String, String) {
    let fields: Vec<&str> = l.strip_prefix("data: v ").expect(l).split(' ').collect();
    assert_eq!(fields.len(), 3, "{l}");
    (fields[0].into(), fields[1].into(), fields[2].into())
}

/// What `stacked web` says as it starts on `port` with the token the
/// launch before kept for it, opening the browser (`open`) or not.
fn waiting(port: u16, open: bool) -> String {
    let then = if open { "else a new tab is opened on a new address" } else { "else the address is given a new token" };
    format!("stacked: a tab already open on port {port} has 2 seconds to come back, {then}")
}

/// What it says once no tab came back.
fn renewed() -> String {
    "stacked: no tab came back within 2 seconds, so the address has a new token, and a tab with the old one is refused".into()
}

/// What it says as it quits, its last tab closed.
const CLOSED: &str = "stacked: the last tab of the editor was closed, and no tab came back within 2 seconds, so stacked web quits";

/// What a tab come back from another launch is told, as a notice.
fn running_again() -> (String, String, String) {
    notice("stacked-web", "stacked", "stacked web is running again: click to switch to this tab.")
}

/// How long a test lets a tab be gone before it looks whether `stacked
/// web` quit: a second past the 2 seconds it waits for a tab to come back.
const PAST: Duration = Duration::from_secs(3);

/// A stand-in, in `dir`, for the program a `stacked web` run for real opens
/// the browser with, as `$STACKED_OPEN` names it: it opens nothing, and
/// writes each address it is given on a line of the file this gives with
/// it. No test opens a browser.
fn opener_in(dir: &Path) -> (PathBuf, PathBuf) {
    std::fs::create_dir_all(dir).unwrap();
    let (path, log) = (dir.join("open.sh"), dir.join("opened.txt"));
    std::fs::write(&path, format!("#!/bin/sh\nprintf '%s\\n' \"$1\" >> '{}'\n", log.display())).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    (path, log)
}

/// `stacked web` on `db`, run in `dir`, its home `home`, as `web_in` runs
/// it, but opening the browser, with the stand-in `opener` (`opener_in`).
fn web_opening(home: &Path, dir: &Path, db: &Path, opener: &Path) -> Command {
    let mut cmd = stacked(home);
    cmd.args(["-f", db.to_str().unwrap(), "web"]).env("STACKED_OPEN", opener).current_dir(dir).stdin(Stdio::null()).stderr(Stdio::piped());
    cmd
}

/// The page at `port` with `token`, as `stacked web` opens it.
fn address(port: u16, token: &str) -> String {
    format!("http://127.0.0.1:{port}/?t={token}")
}

/// The events of the server on `port`, asked for with `query` after
/// `/events?`, and with `header`, its line ended, if any.
fn events_with(port: u16, query: &str, header: &str) -> BufReader<TcpStream> {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    write!(s, "GET /events?{query} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{header}\r\n").unwrap();
    BufReader::new(s)
}

/// The events of the server on `port` for a page 100 wide, asked for again
/// as the browser asks once it lost them, saying `had`, the launch it had
/// them from (`Last-Event-ID`).
fn events_again(port: u16, token: &str, had: &str) -> BufReader<TcpStream> {
    events_with(port, &format!("t={token}&cols=100&rows=30"), &format!("Last-Event-ID: {had}\r\n"))
}

/// `stacked web`, as `cmd` runs it, on `port`, where the launch before
/// listened with `token`, kept for it: its events asked for with that token
/// as soon as it listens, as the tab of that launch asks again each second,
/// saying the launch it had them from, `had`. The launch, as it says where
/// it is, and those events, their status line read; None if another
/// process took the port first, or if the events were asked for too late,
/// the token renewed first, as a machine this loaded may make them.
fn come_back(mut cmd: Command, port: u16, token: &str, had: &str) -> Option<(Served, BufReader<TcpStream>)> {
    let mut child = cmd.spawn().unwrap();
    let said = BufReader::new(child.stderr.take().unwrap());
    let mut child = Running(child);
    let started = Instant::now();
    let asked = loop {
        if let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) {
            write!(s, "GET /events?t={token}&cols=100&rows=30 HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nLast-Event-ID: {had}\r\n\r\n").unwrap();
            break Some(s);
        }
        if child.0.try_wait().unwrap().is_some() || started.elapsed() > PATIENCE {
            break None;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let served = unless_taken(said_where(child, said), port)?;
    let mut events = BufReader::new(asked.expect("never listened on"));
    let mut status = String::new();
    events.read_line(&mut status).unwrap();
    (status.trim_end() == "HTTP/1.1 200 OK").then_some((served, events))
}

/// The events of the server on `port` for a page 100 wide, asked for as a
/// page loaded again asks for them, once they start (`v`); none if the
/// server is gone, or ends them first.
fn back(port: u16, token: &str) -> Option<BufReader<TcpStream>> {
    let mut s = TcpStream::connect(("127.0.0.1", port)).ok()?;
    write!(s, "GET /events?t={token}&cols=100&rows=30 HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n").ok()?;
    let mut events = BufReader::new(s);
    loop {
        let mut line = String::new();
        if events.read_line(&mut line).ok()? == 0 {
            return None;
        }
        if line.starts_with("data: v ") {
            return Some(events);
        }
    }
}

/// What `attempt` makes of the launches it starts, tried again while it
/// gives back None, as `why` says why it could not be tried: up to ten
/// times, so that only stacked can fail it.
fn tried_again<T>(why: &str, mut attempt: impl FnMut() -> Option<T>) -> T {
    (0..10).find_map(|_| attempt()).unwrap_or_else(|| panic!("each of ten tries {why}"))
}

#[test]
fn the_next_launch_on_a_fixed_port_has_the_tab_already_open_come_back_and_opens_no_new_one() {
    let (tmp, db) = setup();
    let home = tmp.path().join("home");
    let tokens = web_token(&home);
    let mut tries = 0;
    let (port, first, (build, had), log, next, mut page) = tried_again("found its port taken first, or came back too late", || {
        tries += 1;
        let (opener, log) = opener_in(&tmp.path().join(format!("try-{tries}")));
        let port = free_port();
        settings_of(&home, &format!("[web]\nport = {port}\n"));
        // No token kept yet: one of its own, kept for its port in a file
        // only you may read, beside the state, and the browser opened on its
        // address at once, nothing said before where the editor is.
        let s = unless_taken(started(web_opening(&home, tmp.path(), &db, &opener)), port)?;
        assert!(s.before.is_empty(), "{:?}", s.before);
        assert_eq!(kept_in(&tokens), Some((port, s.token.clone())));
        assert_eq!(mode_of(&tokens), 0o600);
        assert_eq!(links_opened(&log, 1), [address(port, &s.token)]);
        // Its tab, told which launch sends its events, which keeps its
        // address for the next.
        let mut tab = events(port, &s.token, 100);
        let (build, had, kept) = version(until(&mut tab, "which stacked", |l| l.starts_with("data: v ")).last().unwrap());
        assert_eq!(kept, "1");
        let first = s.token.clone();
        s.quit();
        // The next launch, the tab asking for its events again as soon as it
        // listens, as the browser does each second, saying the launch it had
        // them from.
        let (next, page) = come_back(web_opening(&home, tmp.path(), &db, &opener), port, &first, &had)?;
        Some((port, first, (build, had), log, next, page))
    });
    // Come back: said so, where the editor is said after it, the first
    // one's token, kept as it was; no new tab opened.
    assert_eq!(next.before, [waiting(port, true), "stacked: the tab already open came back".to_string()]);
    assert_eq!(next.token, first);
    assert_eq!(kept_in(&tokens), Some((port, first.clone())));
    // The tab told first which launch sends its events, the browser to ask
    // for them again a second after losing them; then, come back from
    // another launch, that stacked web runs again, for the banner it shows
    // while it is not what you look at; then the screen whole.
    let seen = until(&mut page, "the screen", |l| l.starts_with("data: s "));
    let data: Vec<&String> = seen.iter().filter(|l| l.starts_with("data: ")).collect();
    let (same, launch, kept) = version(data[0]);
    assert!(same == build && launch != had && kept == "1", "{seen:#?}");
    assert!(seen.contains(&"retry: 1000".to_string()) && seen.contains(&format!("id: {launch}")), "{seen:#?}");
    assert_eq!(notices(&seen), [running_again()]);
    assert_eq!(data.len(), 3, "{seen:#?}");
    until(&mut page, "the patches", |l| l.contains("Bug 2 - add b"));
    assert_eq!(next.page(&first), "HTTP/1.1 200 OK");
    assert_eq!(std::fs::read_to_string(&log).unwrap().lines().count(), 1, "a new tab opened");
    next.quit();
}

#[test]
fn with_no_tab_back_within_2_seconds_the_next_launch_on_a_fixed_port_renews_its_token_and_opens_a_new_tab() {
    let (tmp, db) = setup();
    let home = tmp.path().join("home");
    let tokens = web_token(&home);
    let mut tries = 0;
    let (port, first, log, next, took) = tried_again("found its port taken first", || {
        tries += 1;
        let (opener, log) = opener_in(&tmp.path().join(format!("try-{tries}")));
        let port = free_port();
        settings_of(&home, &format!("[web]\nport = {port}\n"));
        let s = unless_taken(started(web_opening(&home, tmp.path(), &db, &opener)), port)?;
        let first = s.token.clone();
        assert_eq!(links_opened(&log, 1), [address(port, &first)]);
        s.quit();
        let at = Instant::now();
        let next = unless_taken(started(web_opening(&home, tmp.path(), &db, &opener)), port)?;
        Some((port, first, log, next, at.elapsed()))
    });
    // No tab came: 2 seconds given it, then a new token, kept in the first
    // one's place, as only you may read it, said, and the browser opened on
    // the new address.
    assert!(took >= Duration::from_secs(2), "{took:?}");
    assert_eq!(next.before, [waiting(port, true), renewed()]);
    assert_ne!(next.token, first);
    assert_eq!(kept_in(&tokens), Some((port, next.token.clone())));
    assert_eq!(mode_of(&tokens), 0o600);
    assert_eq!(links_opened(&log, 2), [address(port, &first), address(port, &next.token)]);
    // The first one's token refused from then on, the events its tab asks
    // for again too, which its page then says it can be closed; the new one
    // served.
    let host = format!("127.0.0.1:{port}");
    assert_eq!(next.page(&first), "HTTP/1.1 403 Forbidden");
    assert_eq!(ask(port, &host, &format!("GET /events?t={first}&cols=100&rows=30"), "").0, "HTTP/1.1 403 Forbidden");
    assert_eq!(next.page(&next.token), "HTTP/1.1 200 OK");
    next.quit();
}

#[test]
fn a_tab_come_back_from_another_launch_is_told_stacked_web_runs_again_unless_notifications_are_off() {
    let (tmp, db) = setup();
    stacked::config::init(stacked::config::Config::default());
    let (port, token) = stacked::web::start(db.clone()).unwrap();
    let screen = |l: &str| l.starts_with("data: s ");
    // A page come: told which launch sends its events, then the screen, and
    // no notice.
    let mut page = events(port, &token, 100);
    let seen = until(&mut page, "the screen", screen);
    let (_, launch, _) = version(seen.iter().find(|l| l.starts_with("data: v ")).unwrap());
    assert!(notices(&seen).is_empty(), "{seen:#?}");
    // Its events asked for again saying that launch, as once the machine
    // slept: no notice.
    let mut same = events_again(port, &token, &launch);
    let seen = until(&mut same, "the screen", screen);
    assert!(notices(&seen).is_empty(), "{seen:#?}");
    // Saying another, as a tab come back after a launch does: told, before
    // the screen, with no bell, which rings for what the editor learned
    // alone; and so too a page loaded again, saying it in its address.
    let mut again = events_again(port, &token, "0123456789abcdef");
    let seen = until(&mut again, "the screen", screen);
    assert_eq!((notices(&seen), bells(&seen)), (vec![running_again()], 0), "{seen:#?}");
    let mut loaded = events_with(port, &format!("t={token}&cols=100&rows=30&l=0123456789abcdef"), "");
    let seen = until(&mut loaded, "the screen", screen);
    assert_eq!((notices(&seen), bells(&seen)), (vec![running_again()], 0), "{seen:#?}");
    // `web.notifications = false` in the settings: none.
    let home = tmp.path().join("home");
    settings_of(&home, "[web]\nnotifications = false\n");
    let s = served(&home, tmp.path(), &db, &[]);
    let mut again = events_again(s.port, &s.token, "0123456789abcdef");
    let seen = until(&mut again, "the screen", screen);
    assert!(notices(&seen).is_empty(), "{seen:#?}");
    s.quit();
}

#[test]
fn closing_the_last_tab_quits_as_quit_does_once_no_tab_came_back_within_2_seconds() {
    let (tmp, db) = setup();
    let made = made_ahead(tmp.path());
    let s = served(&tmp.path().join("home"), tmp.path(), &db, &[]);
    let mut page = events(s.port, &s.token, 400);
    until(&mut page, "the worktree made ahead", |l| l.contains(MADE));
    assert!(made.join(".git").exists(), "made ahead");
    // The tab closed: its page says it goes, its events closed, and none
    // comes back. Left as Quit leaves it: said why, the worktree made ahead
    // removed.
    let closed = Instant::now();
    s.close(page);
    s.quits(closed);
    assert!(gone(&made), "removed once the editor is left");
}

#[test]
fn a_tab_loaded_again_says_it_goes_comes_back_within_2_seconds_and_quits_nothing() {
    let (tmp, db) = setup();
    let home = tmp.path().join("home");
    let (mut s, mut page) = tried_again("came back later than 2 seconds", || {
        let s = served(&home, tmp.path(), &db, &[]);
        let mut page = events(s.port, &s.token, 100);
        until(&mut page, "the patches", |l| l.contains("Bug 2 - add b"));
        // Loaded again: the page goes, and the page loaded comes at once.
        let closed = Instant::now();
        s.close(page);
        let again = back(s.port, &s.token)?;
        (closed.elapsed() < Duration::from_secs(2)).then_some((s, again))
    });
    // Past the 2 seconds, it still runs, and serves the page come back.
    std::thread::sleep(PAST);
    assert!(s.running(), "it quit");
    until(&mut page, "the patches", |l| l.contains("Bug 2 - add b"));
    ask(s.port, &format!("127.0.0.1:{}", s.port), &format!("POST /input?t={}", s.token), "k Down -\n");
    until(&mut page, "the second patch", |l| l.contains("+bee"));
    s.quit();
}

#[test]
fn a_tab_cut_off_without_saying_it_goes_quits_nothing_nor_does_a_page_going_before_any_came() {
    let (tmp, db) = setup();
    let mut s = served(&tmp.path().join("home"), tmp.path(), &db, &[]);
    // Before any page came: one says it goes, as a page closed before it
    // asked for its events does, and nothing quits.
    ask(s.port, &format!("127.0.0.1:{}", s.port), &format!("POST /input?t={}", s.token), "g\n");
    std::thread::sleep(PAST);
    assert!(s.running(), "it quit");
    // A page come, then cut off without saying it goes, as when the machine
    // sleeps or the browser puts its tab aside: nothing quits.
    let mut page = events(s.port, &s.token, 100);
    until(&mut page, "the patches", |l| l.contains("Bug 2 - add b"));
    drop(page);
    std::thread::sleep(PAST);
    assert!(s.running(), "it quit");
    // It comes back, as the browser asks again, and is served.
    let mut page = events(s.port, &s.token, 100);
    until(&mut page, "the patches again", |l| l.contains("Bug 2 - add b"));
    s.quit();
}

#[test]
fn closing_one_of_two_tabs_quits_nothing_while_the_other_stays_and_closing_the_other_quits() {
    let (tmp, db) = setup();
    let mut s = served(&tmp.path().join("home"), tmp.path(), &db, &[]);
    let mut one = events(s.port, &s.token, 100);
    until(&mut one, "the patches", |l| l.contains("Bug 2 - add b"));
    let mut other = events(s.port, &s.token, 100);
    until(&mut other, "the patches", |l| l.contains("Bug 2 - add b"));
    // One closed: nothing quits, and the other is served as ever.
    s.close(one);
    std::thread::sleep(PAST);
    assert!(s.running(), "it quit");
    ask(s.port, &format!("127.0.0.1:{}", s.port), &format!("POST /input?t={}", s.token), "k Down -\n");
    until(&mut other, "the second patch", |l| l.contains("+bee"));
    // The other closed too, the last: left.
    let closed = Instant::now();
    s.close(other);
    s.quits(closed);
}

/// Work landed on stack `s` of `db`: an answer, the tool it could not use,
/// searchfox-cli, waiting for your review, with the agent's word on it.
fn refused_work(db: &Path) {
    stacked::db::Db::edit(db, |d| {
        let from = d.head("s").unwrap().0;
        d.add_run(stacked::db::Run {
            id: "run-refused".into(),
            what: "answer".into(),
            stack: "s".into(),
            state: stacked::db::RunState::Done,
            ask: Some(serde_json::json!({"way": "answer", "scope": "stack", "from": from, "notes": []})),
            held: Some(stacked::held::Held {
                at: stacked::db::now(),
                tools: vec![stacked::refusals::Refusal {
                    rules: vec!["Bash(searchfox-cli *)".into()],
                    calls: vec!["searchfox-cli --define Foo".into()],
                    why: "To find Foo; I used rg instead.".into(),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        })
        .map(|_| ())
    })
    .unwrap();
}

#[test]
fn the_tools_the_agent_could_not_use_are_reviewed_in_the_browser_too() {
    let (_tmp, db) = setup();
    refused_work(&db);
    stacked::config::init(stacked::config::Config::default());
    let (port, token) = stacked::web::start(db.clone()).unwrap();
    let (host, input) = (format!("127.0.0.1:{port}"), format!("POST /input?t={token}"));
    let mut events = events(port, &token, 100);
    until(&mut events, "the banner", |l| text(l).contains("1 tool the agent could not use"));
    // From the banner, its review: the section, the item refused, what the
    // agent said of it beside them.
    ask(port, &host, &input, "k Esc -\n");
    ask(port, &host, &input, "k Enter -\n");
    let seen = until(&mut events, "the item", |l| text(l).contains("✗ Bash(searchfox-cli *) · refused"));
    assert!(seen.iter().any(|l| text(l).contains(" tools the agent could not use")), "{seen:#?}");
    until(&mut events, "the agent's word", |l| text(l).contains("To find Foo; I used rg instead."));
    // Enter: allowed from now on.
    ask(port, &host, &input, "k Enter -\n");
    until(&mut events, "allowed", |l| text(l).contains("✓ Bash(searchfox-cli *) · from now on"));
}

/// The runs of row event `l`: each its style, `FG,BG,MODS` as
/// web/client.rs reads it, and its text.
fn runs(l: &str) -> Vec<(String, String)> {
    let runs = l.strip_prefix("data: r ").and_then(|r| r.split_once(' ')).map_or("", |(_, runs)| runs);
    runs.split('\u{1e}').filter_map(|r| r.split_once('\u{1f}')).map(|(s, t)| (s.to_string(), t.to_string())).collect()
}

#[test]
fn esc_in_a_review_marks_each_count_in_green_or_red_the_rest_dim_in_the_browser_too() {
    let (_tmp, db) = setup();
    refused_work(&db);
    stacked::config::init(stacked::config::Config::default());
    let (port, token) = stacked::web::start(db.clone()).unwrap();
    let (host, input) = (format!("127.0.0.1:{port}"), format!("POST /input?t={token}"));
    let mut events = events(port, &token, 100);
    until(&mut events, "the banner", |l| text(l).contains("1 tool the agent could not use"));
    ask(port, &host, &input, "k Esc -\n");
    ask(port, &host, &input, "k Enter -\n");
    until(&mut events, "the item", |l| text(l).contains("✗ Bash(searchfox-cli *) · refused"));
    // Esc: what the review marks, said, the count red, as the terminal's
    // colour 1, the rest dim (ratatui's modifier 2), as the terminal draws
    // them; then, allowed, green, its colour 2.
    let row = |events: &mut BufReader<TcpStream>, said: &str| runs(until(events, said, |l| text(l).contains(said)).last().unwrap());
    ask(port, &host, &input, "k Esc -\n");
    let note = row(&mut events, "tools the agent could not use: ✗ 1 refused");
    for run in [(",,2", "tools the agent could not use: "), ("1,,0", "✗ 1 refused")] {
        assert!(note.contains(&(run.0.into(), run.1.into())), "{run:?} in {note:?}");
    }
    ask(port, &host, &input, "k Esc -\n");
    ask(port, &host, &input, "k Enter -\n");
    until(&mut events, "allowed", |l| text(l).contains("✓ Bash(searchfox-cli *) · from now on"));
    ask(port, &host, &input, "k Esc -\n");
    let note = row(&mut events, "tools the agent could not use: ✓ 1 allowed from now on");
    for run in [(",,2", "tools the agent could not use: "), ("2,,0", "✓ 1 allowed from now on")] {
        assert!(note.contains(&(run.0.into(), run.1.into())), "{run:?} in {note:?}");
    }
}

/// On stack `s` of `db`, ann's thread on its first patch's title: her
/// comment, your reply, then her edit of her comment on Phabricator, her
/// comment as it was redacted. And a pull waiting for your review, holding
/// her new comment on the second patch's title.
fn pulled_work(db: &Path) {
    use stacked::db::{Note, Run, RunState};
    use stacked::thread::fenced;
    stacked::db::Db::edit(db, |d| {
        let s = d.stack("s").unwrap().clone();
        let (first, second) = (d.patch(&s.patches[0]).clone(), d.patch(&s.patches[1]).clone());
        let thread = [fenced("reviewer ann redacted", "Hi Da"), "Hi ann! My name is Damien".to_string(), fenced("reviewer ann edit Hi Da", "Hi Damien!")].join("\n");
        let note = |p: &stacked::db::Patch, id: &str, text: String| Note {
            change: p.change.clone(),
            id: id.into(),
            line: stacked::view::title_key(p),
            file: stacked::view::MSG_PATH.into(),
            preview: p.title().into(),
            text,
            by: Some("ann".into()),
            remote: vec![id.trim_start_matches("phab:").into()],
            ..Default::default()
        };
        d.set_note(note(&first, "phab:c1", thread))?;
        d.add_run(Run {
            id: "pull-1".into(),
            what: "pull".into(),
            stack: "s".into(),
            state: RunState::Done,
            held: Some(stacked::held::Held {
                at: stacked::db::now(),
                opened: vec![note(&second, "phab:c2", fenced("reviewer ann", "Why add b?"))],
                ..Default::default()
            }),
            ..Default::default()
        })
        .map(|_| ())
    })
    .unwrap();
}

#[test]
fn a_reviewer_s_edit_is_headed_so_and_a_pull_s_review_rejects_nothing_in_the_browser_too() {
    let (_tmp, db) = setup();
    pulled_work(&db);
    stacked::config::init(stacked::config::Config::default());
    let (port, token) = stacked::web::start(db.clone()).unwrap();
    let (host, input) = (format!("127.0.0.1:{port}"), format!("POST /input?t={token}"));
    let mut events = events(port, &token, 120);
    // ann's comment as it was, redacted; her edit, by what it edits.
    let seen = until(&mut events, "her edit", |l| text(l).contains("▎ ann's edit of the comment that said \"Hi Da\":"));
    assert!(seen.iter().any(|l| text(l).contains("▎ ann (redacted):")), "{seen:#?}");
    // The pull's review: its part accepted; Delete rejects nothing, and says
    // why; closing it, nothing to reject either.
    ask(port, &host, &input, "k Esc -\n");
    ask(port, &host, &input, "k Enter -\n");
    until(&mut events, "the part", |l| text(l).contains("✓ ✉ message: Bug 2 - add b · Why add b?"));
    ask(port, &host, &input, "k Delete -\n");
    until(&mut events, "why", |l| text(l).contains("a comment pulled from Phabricator cannot be rejected: close the review to file it"));
    ask(port, &host, &input, "k Esc -\n");
    let seen = until(&mut events, "the dialog", |l| text(l).contains("Keep reviewing"));
    assert!(seen.iter().any(|l| text(l).contains("Close review")), "{seen:#?}");
    assert!(!seen.iter().any(|l| text(l).contains("Reject all remaining")), "{seen:#?}");
}

/// On stack `s` of `db`, the agent's work at max: on its first patch's
/// title, a thread the agent replied to at max, after its reply of before
/// efforts were picked; and its answer to a comment on the second patch's
/// title, landed, waiting for your review.
fn at_max(db: &Path) {
    use stacked::db::{Effort, Note, Run, RunState};
    use stacked::thread::{Role, append_agent, append_turn};
    stacked::db::Db::edit(db, |d| {
        let (from, s) = d.head("s").map(|(rev, s)| (rev, s.clone())).unwrap();
        let (first, second) = (d.patch(&s.patches[0]).clone(), d.patch(&s.patches[1]).clone());
        let note = |p: &stacked::db::Patch, id: &str, text: String| Note {
            change: p.change.clone(),
            id: id.into(),
            line: stacked::view::title_key(p),
            file: stacked::view::MSG_PATH.into(),
            preview: p.title().into(),
            text,
            ..Default::default()
        };
        let before = append_turn("Was it so before?", Role::Agent, "It was.", Some("answer"));
        let thread = append_agent(&append_turn(&before, Role::You, "Why shout?", None), "A shout.", "answer", Some("max"));
        d.set_note(note(&first, "c-max", thread))?;
        d.set_note(note(&second, "c-bee", "Why add b?".into()))?;
        let ask = serde_json::json!({"way": "answer", "scope": {"patch": second.change}, "from": from, "notes": [{"label": "c1", "change": second.change, "id": "c-bee"}], "effort": "max"});
        d.add_run(Run {
            id: "run-max".into(),
            what: "answer".into(),
            stack: "s".into(),
            state: RunState::Done,
            request: "Why add b?".into(),
            ask: Some(ask),
            held: Some(stacked::held::Held {
                at: stacked::db::now(),
                replies: vec![stacked::held::Reply {
                    change: second.change.clone(),
                    id: "c-bee".into(),
                    text: "For the bees.".into(),
                    tag: "answer".into(),
                    effort: Some(Effort::Max),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        })
        .map(|_| ())
    })
    .unwrap();
}

#[test]
fn the_effort_the_agent_was_asked_at_heads_its_turns_and_names_its_work_in_the_browser_too() {
    let (_tmp, db) = setup();
    at_max(&db);
    stacked::config::init(stacked::config::Config::default());
    let (port, token) = stacked::web::start(db).unwrap();
    let (host, input) = (format!("127.0.0.1:{port}"), format!("POST /input?t={token}"));
    let mut events = events(port, &token, 120);
    // The screen as the events leave it.
    let rows: std::cell::RefCell<Vec<String>> = std::cell::RefCell::new(Vec::new());
    let on_screen = |said: &[&str]| said.iter().all(|s| rows.borrow().iter().any(|r| r.contains(*s)));
    // The thread: the agent's turn at max headed with it, the one of before
    // as ever; the comments pane saying it before what the agent said last;
    // the banner, what landed at max.
    until(&mut events, "the turns", |l| {
        shown(&mut rows.borrow_mut(), l);
        on_screen(&["▎ agent(max):", "▎ agent:", "agent(max): A shout.", "· the next: 1 reply, asked at max: \"Why add b?\""])
    });
    // Its review, from the banner: its title, and its reply headed so.
    ask(port, &host, &input, "k Esc -\n");
    ask(port, &host, &input, "k Enter -\n");
    until(&mut events, "the review", |l| {
        shown(&mut rows.borrow_mut(), l);
        on_screen(&[" the agent's work (max): 1 reply", "▎ agent(max):", "▎   For the bees."])
    });
}

/// A comment on patch 1's `+TWO`, answered in 30 lines, then closed: what a
/// comment written there again shows, dimmed, until something is written.
fn closed_on_two(tmp: &Path, db: &Path) {
    let out = stacked(&tmp.join("home"))
        .current_dir(tmp.join("src"))
        .args(["-f", db.to_str().unwrap(), "comment", "s", "--patch", "1", "--line", "a.txt:+TWO", "why so loud?"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let answer: Vec<String> = (1..=30).map(|n| format!("reason {n}")).collect();
    stacked::db::Db::edit(db, |d| {
        let change = d.patch(&d.stack("s")?.patches[0]).change.clone();
        let note = d.notes(&change).into_iter().next().cloned().unwrap();
        let text = stacked::thread::append_turn(&note.text, stacked::thread::Role::Agent, &answer.join("\n"), Some("answer"));
        d.set_note(stacked::db::Note { text, ..note.clone() })?;
        d.set_note(stacked::db::Note { text: String::new(), ..note })
    })
    .unwrap();
}

/// On stack `s` of `db`: the agent changing the stack, its process this
/// one, for as long as the test runs; and, waiting their turn behind it, a
/// reconcile with HEAD, a lint job, and a comment on the first patch asked
/// as an edit.
fn waiting_their_turn(db: &Path) {
    use stacked::db::{Note, Queued, Run, RunState, Stage, Way};
    stacked::db::Db::edit(db, |d| {
        let first = d.patch(&d.stack("s").unwrap().patches[0]).clone();
        let from = d.head("s").unwrap().0;
        let asking = Note {
            change: stacked::db::stack_change("s"),
            id: "s-c1".into(),
            text: "Make it better.".into(),
            ..Default::default()
        };
        d.set_note(asking.clone())?;
        let ask = serde_json::json!({"way": "change", "scope": "stack", "from": from, "notes": [{"label": "c1", "change": asking.change, "id": asking.id}]});
        d.add_run(Run {
            id: "run-changing".into(),
            what: "edit".into(),
            stack: "s".into(),
            state: RunState::Started,
            pid: Some(std::process::id()),
            ask: Some(ask),
            time: stacked::db::now(),
            ..Default::default()
        })?;
        d.add_run(Run {
            id: stacked::db::unique(),
            what: stacked::reconcile::WHAT.into(),
            stack: "s".into(),
            state: RunState::Started,
            request: "the commit messages of patch 1 taken from HEAD".into(),
            reconcile: Some(Box::default()),
            ..Default::default()
        })?;
        d.add_run(Run {
            id: stacked::db::unique(),
            what: stacked::db::LINT.into(),
            stack: "s".into(),
            state: RunState::Started,
            request: "./lint.sh".into(),
            lint: Some(Box::new(stacked::lint::Job {
                command: "./lint.sh".into(),
                ..Default::default()
            })),
            ..Default::default()
        })?;
        let queued = Queued {
            way: Way::Change,
            batch: format!("b{}", stacked::db::unique()),
            stack: false,
            stage: Stage::Ready,
            on: None,
            was: None,
            effort: None,
            follow: false,
        };
        d.set_note(Note {
            change: first.change.clone(),
            id: "c-loud".into(),
            line: stacked::view::title_key(&first),
            file: stacked::view::MSG_PATH.into(),
            preview: first.title().into(),
            text: "Shout less.".into(),
            queued: Some(queued),
            ..Default::default()
        })
    })
    .unwrap();
}

/// The screen `rows`, a row of text each, as event `l` leaves it.
fn shown(rows: &mut Vec<String>, l: &str) {
    if let Some(size) = l.strip_prefix("data: s ") {
        *rows = vec![String::new(); size.split(' ').nth(1).and_then(|r| r.parse().ok()).unwrap_or(0)];
    } else if let Some(y) = l.strip_prefix("data: r ").and_then(|r| r.split(' ').next()).and_then(|y| y.parse::<usize>().ok())
        && y < rows.len()
    {
        rows[y] = text(l);
    }
}

/// The box of the comment being written on the screen `rows`, if there is
/// one: the lines inside it, its bottom edge, and the row that edge is on.
fn editor_box(rows: &[String]) -> Option<(Vec<String>, String, usize)> {
    let lines: Vec<Vec<char>> = rows.iter().map(|r| r.chars().collect()).collect();
    let top = rows.iter().position(|r| r.contains("┌─ a comment on"))?;
    let left = lines[top].iter().position(|&c| c == '┌')?;
    let right = lines[top].iter().position(|&c| c == '┐')?;
    let bottom = (top + 1..lines.len()).find(|&y| lines[y].get(left) == Some(&'└'))?;
    let inside = (top + 1..bottom).map(|y| lines[y].get(left + 1..right).unwrap_or_default().iter().collect::<String>().trim_end().to_string());
    Some((inside.collect(), lines[bottom].get(left..=right).unwrap_or_default().iter().collect(), bottom))
}

#[test]
fn what_was_closed_here_scrolls_to_its_end_with_the_arrows_and_the_wheel_in_the_browser_too() {
    let (tmp, db) = setup();
    closed_on_two(tmp.path(), &db);
    stacked::config::init(stacked::config::Config::default());
    let (port, token) = stacked::web::start(db).unwrap();
    let (host, input) = (format!("127.0.0.1:{port}"), format!("POST /input?t={token}"));
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    write!(s, "GET /events?t={token}&cols=80&rows=16 HTTP/1.1\r\nHost: {host}\r\n\r\n").unwrap();
    let mut events = BufReader::new(s);
    // The screen as the events leave it, and the box's first line there.
    let rows = std::cell::RefCell::new(Vec::new());
    let first = |at: &str| editor_box(&rows.borrow()).is_some_and(|(inside, ..)| inside.first().is_some_and(|l| l == at));
    // The mouse over the line picks it, and a click on it is Enter: the box
    // opens on what was closed there, from its first line, ↑↓ said on the
    // bottom bar.
    let seen = until(&mut events, "the line", |l| {
        shown(&mut rows.borrow_mut(), l);
        text(l).contains("+TWO")
    });
    let (x, y) = cell_of(seen.last().unwrap(), "+TWO");
    ask(port, &host, &input, &format!("v {x} {y}\nm {x} {y} l\n"));
    until(&mut events, "what was closed there", |l| {
        shown(&mut rows.borrow_mut(), l);
        first("you: why so loud?") && rows.borrow().get(15).is_some_and(|r| r.contains("↑↓: read the last comment closed here"))
    });
    // The mouse in use, the wheel over the box scrolls it, by the lines it
    // turned, which the page sends with the cell under the mouse: two, then
    // one, three lines down. Over the top bar, which does not scroll,
    // nothing.
    let (bx, by) = {
        let rows = rows.borrow();
        let top = rows.iter().position(|r| r.contains("┌─ a comment on")).unwrap();
        (rows[top].chars().position(|c| c == '┌').unwrap() + 3, top + 1)
    };
    ask(port, &host, &input, &format!("w {bx} {by} 2\nw 1 0 4\nw {bx} {by} 1\n"));
    until(&mut events, "three lines down", |l| {
        shown(&mut rows.borrow_mut(), l);
        first("  reason 3")
    });
    // pgdn a box's worth; after that key, the wheel is ↑↓, wherever the
    // mouse is: a line down over the top bar.
    ask(port, &host, &input, "k PageDown -\n");
    until(&mut events, "a box's worth further", |l| {
        shown(&mut rows.borrow_mut(), l);
        first("  reason 8")
    });
    ask(port, &host, &input, "w 1 0 1\n");
    until(&mut events, "a line further", |l| {
        shown(&mut rows.borrow_mut(), l);
        first("  reason 9")
    });
    // To its end and no further: its last line in sight, the box's bottom
    // edge above the bottom bar.
    ask(port, &host, &input, &"k Down -\n".repeat(40));
    until(&mut events, "its end", |l| {
        shown(&mut rows.borrow_mut(), l);
        editor_box(&rows.borrow()).is_some_and(|(inside, edge, bottom)| {
            inside.first().is_some_and(|l| l == "  reason 26") && inside.last().is_some_and(|l| l == "  reason 30") && !edge.contains('▼') && bottom < 15
        })
    });
    // The mouse moved again: the wheel over the box scrolls it, a line up.
    ask(port, &host, &input, &format!("v {bx} {by}\nw {bx} {by} -1\n"));
    until(&mut events, "a line up", |l| {
        shown(&mut rows.borrow_mut(), l);
        first("  reason 25")
    });
}

#[test]
fn up_on_a_patch_listing_files_goes_to_the_last_file_of_the_one_above_with_the_wheel_in_the_browser_too() {
    let (_tmp, db) = setup();
    stacked::config::init(stacked::config::Config::default());
    let (port, token) = stacked::web::start(db).unwrap();
    let (host, input) = (format!("127.0.0.1:{port}"), format!("POST /input?t={token}"));
    let mut events = events(port, &token, 100);
    until(&mut events, "the patches", |l| l.starts_with("data: r 3 ") && l.contains("Bug 2 - add b"));
    // Listing files, down to patch 2's line, its file under it.
    ask(port, &host, &input, "k Left -\nk Down -\nk Down -\n");
    until(&mut events, "patch 2's file", |l| l.starts_with("data: r 4 ") && text(l).contains("b.txt"));
    // The wheel turned a notch up, which the page sends with the cell under
    // the mouse, over the middle: after those keys, ↑ where the cursor is,
    // patch 1's file listed in place of patch 2's, the cursor on it.
    ask(port, &host, &input, "w 60 10 -1\n");
    until(&mut events, "patch 1's last file", |l| l.starts_with("data: r 3 ") && l.contains(",,64\u{1f}      a.txt"));
}

#[test]
fn what_waits_its_turn_is_taken_out_of_the_queue_from_the_banner_once_asked_in_the_browser_too() {
    let (_tmp, db) = setup();
    waiting_their_turn(&db);
    stacked::config::init(stacked::config::Config::default());
    let (port, token) = stacked::web::start(db.clone()).unwrap();
    let (host, input) = (format!("127.0.0.1:{port}"), format!("POST /input?t={token}"));
    let mut events = events(port, &token, 140);
    until(&mut events, "the banner", |l| text(l).contains("Reconcile with HEAD waits its turn"));
    // On the banner, its first line the reconcile's: Delete asks first, and
    // Drop it, over Keep it, drops it.
    ask(port, &host, &input, "k Esc -\n");
    until(&mut events, "the reconcile's line", |l| text(l).contains("◌ Reconcile with HEAD, waiting its turn: the commit messages of patch 1 taken from HEAD"));
    ask(port, &host, &input, "k Delete -\n");
    until(&mut events, "the reconcile's drop asked", |l| text(l).contains("drop the reconcile with HEAD that waits its turn? Nothing was changed yet"));
    ask(port, &host, &input, "k Up -\n");
    ask(port, &host, &input, "k Enter -\n");
    let dropped = "the waiting reconcile with HEAD was dropped: nothing was taken from HEAD, and the stack is as it was";
    until(&mut events, "the reconcile dropped", |l| text(l).contains(dropped));
    // Then the lint job's line.
    ask(port, &host, &input, "k Delete -\n");
    until(&mut events, "the lint job's drop asked", |l| text(l).contains("drop the lint job that waits its turn? Nothing was linted yet"));
    ask(port, &host, &input, "k Up -\n");
    ask(port, &host, &input, "k Enter -\n");
    until(&mut events, "the lint job dropped", |l| text(l).contains("the waiting lint job was dropped: nothing was linted, and the stack is as it was"));
    // Then the comment's: its ask taken back, the comment open.
    ask(port, &host, &input, "k Delete -\n");
    until(&mut events, "the ask's take-back asked", |l| text(l).contains("take the edit ask on patch 1's comment out of the queue? The comment stays open"));
    ask(port, &host, &input, "k Up -\n");
    ask(port, &host, &input, "k Enter -\n");
    let taken = "the edit ask on patch 1's comment is taken out of the queue: the comment stays open, and is not sent";
    until(&mut events, "the ask taken back", |l| text(l).contains(taken));
    let d = stacked::db::Db::load(&db).unwrap();
    assert!(stacked::reconcile::pending(&d, "s").is_none() && stacked::lint::pending(&d, "s").is_none());
    let n = d.note(&d.patch(&d.stack("s").unwrap().patches[0]).change, "c-loud").unwrap();
    assert!(n.queued.is_none() && n.text == "Shout less.", "{n:?}");
    // The agent's change over, so that nothing is left running.
    stacked::db::Db::edit(&db, |d| {
        let r = d.run("run-changing").cloned().unwrap();
        d.add_run(stacked::db::Run {
            state: stacked::db::RunState::Failed,
            pid: None,
            ..r
        })
        .map(|_| ())
    })
    .unwrap();
}

#[test]
fn right_opens_the_comments_pane_of_a_patch_with_none_saying_why_in_the_browser_too() {
    let (_tmp, db) = setup();
    stacked::config::init(stacked::config::Config::default());
    let (port, token) = stacked::web::start(db).unwrap();
    let (host, input) = (format!("127.0.0.1:{port}"), format!("POST /input?t={token}"));
    let mut events = events(port, &token, 140);
    until(&mut events, "the patches", |l| l.starts_with("data: r 3 ") && l.contains("Bug 2 - add b"));
    // Into the middle, then the comments pane, though the patch has none:
    // its header, and why it lists nothing; Enter there, its menu, with the
    // filter.
    ask(port, &host, &input, "k Right -\n");
    ask(port, &host, &input, "k Right -\n");
    until(&mut events, "its header", |l| l.starts_with("data: r 1 ") && text(l).contains("│ all · nothing listed"));
    until(&mut events, "why", |l| text(l).contains("This patch has no comments."));
    ask(port, &host, &input, "k Enter -\n");
    until(&mut events, "its menu", |l| text(l).contains("Filter comments…"));
    // ← out of it, once the menu is closed: the pane goes.
    ask(port, &host, &input, "k Esc -\n");
    ask(port, &host, &input, "k Left -\n");
    until(&mut events, "the pane gone", |l| l.starts_with("data: r 1 ") && !text(l).contains("nothing listed"));
}

/// On stack `s` of `db`, under `tmp`: a comment on the first patch's
/// `+TWO`, `said`, asked about as a change of the stack, and the agent's
/// work for it landed, waiting for your review: `TWO` made `Two`; with
/// `add`, a patch of its own added over patch 1, `Bug 1 - a c too`, making
/// c.txt, which the stack does not hold until the work is accepted; and a
/// reply to the comment unless `quiet`. The comment, as it is then.
fn work_on_two(tmp: &Path, db: &Path, said: &str, add: bool, quiet: bool) -> stacked::db::Note {
    let out = stacked(&tmp.join("home"))
        .current_dir(tmp.join("src"))
        .args(["-f", db.to_str().unwrap(), "comment", "s", "--patch", "1", "--line", "a.txt:+TWO", said])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let n = stacked::db::Db::edit(db, |d| {
        let (from, s) = d.head("s").map(|(r, s)| (r, s.clone())).unwrap();
        let old = d.patch(&s.patches[0]).clone();
        let n = d.notes(&old.change).into_iter().next().cloned().unwrap();
        let draft = stacked::mbox::Draft {
            diff: old.diff.replace("+TWO", "+Two"),
            ..stacked::mbox::Draft::from(&old)
        };
        let mut patches = s.patches.clone();
        patches[0] = d.add_patch(stacked::ops::revise(&old, draft))?;
        if add {
            let added = stacked::mbox::Draft {
                message: "Bug 1 - a c too".into(),
                diff: "diff --git a/c.txt b/c.txt\nnew file mode 100644\n--- /dev/null\n+++ b/c.txt\n@@ -0,0 +1,2 @@\n+cee\n+dee\n".into(),
                ..stacked::mbox::Draft::from(&old)
            };
            patches.insert(1, d.add_patch(stacked::ops::new_patch(added))?);
        }
        let ask = serde_json::json!({"way": "change", "scope": "stack", "from": from, "notes": [{"label": "c1", "change": n.change, "id": n.id}]});
        let replies = match quiet {
            true => vec![],
            false => vec![stacked::held::Reply {
                change: n.change.clone(),
                id: n.id.clone(),
                text: "Quieter now.".into(),
                tag: "change".into(),
                ..Default::default()
            }],
        };
        d.add_run(stacked::db::Run {
            id: "run-agent".into(),
            what: "change".into(),
            stack: "s".into(),
            state: stacked::db::RunState::Done,
            request: n.text.clone(),
            ask: Some(ask),
            held: Some(stacked::held::Held {
                at: stacked::db::now(),
                stack: Some(stacked::held::Change {
                    patches,
                    base: s.base.clone(),
                    op: "agent: the stack".into(),
                    of: "the stack".into(),
                    from: None,
                }),
                replies,
                ..Default::default()
            }),
            ..Default::default()
        })
        .map(|_| n)
    })
    .unwrap();
    // Landed, as a landing does it: what each operation of its change needs,
    // worked out and stored with it.
    stacked::held::work_out(db, "run-agent").unwrap();
    n
}

/// `said` typed in the page, a key for each of its characters.
fn typed_in(port: u16, host: &str, input: &str, said: &str) {
    let keys: String = said.chars().map(|c| format!("k c {:x} -\n", c as u32)).collect();
    ask(port, host, input, &keys);
}

#[test]
fn a_comment_on_a_patch_the_work_adds_sends_it_back_from_its_review_in_the_browser_too() {
    let (tmp, db) = setup();
    work_on_two(tmp.path(), &db, "why so loud?", true, false);
    stacked::config::init(stacked::config::Config::default());
    let (port, token) = stacked::web::start(db.clone()).unwrap();
    let (host, input) = (format!("127.0.0.1:{port}"), format!("POST /input?t={token}"));
    let mut events = events(port, &token, 120);
    until(&mut events, "the banner", |l| text(l).contains("1 landed ask waits for your review"));
    // Its review, from the banner; in its stack diff, Enter on the patch the
    // work adds, then on a line of it, a comment there, saved.
    ask(port, &host, &input, "k Esc -\nk Enter -\n");
    until(&mut events, "the review", |l| text(l).contains("add     2 Bug 1 - a c too"));
    ask(port, &host, &input, "k Down -\nk Down -\nk Right -\nk Enter -\n");
    until(&mut events, "the patch it adds", |l| text(l).contains(" patch 2 as the work leaves it: Bug 1 - a c too"));
    ask(port, &host, &input, &format!("{}k Enter -\n", "k Down -\n".repeat(6)));
    until(&mut events, "the editor", |l| text(l).contains("a comment on: cee"));
    typed_in(port, &host, &input, "Why a c?");
    ask(port, &host, &input, "k Esc -\nk Enter -\n");
    // Listed with what you wrote, it sends the work back, from the ask
    // dialog the review's Esc menu opens, nothing set in it.
    until(&mut events, "what you wrote", |l| text(l).contains("✎ c.txt: cee · Why a c?"));
    ask(port, &host, &input, "k Esc -\n");
    until(&mut events, "the dialog", |l| text(l).contains("Send it back to the agent…"));
    ask(port, &host, &input, "k Enter -\n");
    until(&mut events, "the ask dialog", |l| text(l).contains("send back the agent's work: a change of the stack"));
    until(&mut events, "its buttons", |l| text(l).contains("< Send Now >   < Cancel >"));
    until(&mut events, "its note", |l| text(l).contains("Send Now is grayed until the permission and the effort are set."));
}

#[test]
fn each_stack_operation_is_marked_on_its_own_in_the_browser_too() {
    let (tmp, db) = setup();
    work_on_two(tmp.path(), &db, "why so loud?", true, false);
    stacked::config::init(stacked::config::Config::default());
    let (port, token) = stacked::web::start(db.clone()).unwrap();
    let (host, input) = (format!("127.0.0.1:{port}"), format!("POST /input?t={token}"));
    let mut events = events(port, &token, 120);
    until(&mut events, "the banner", |l| text(l).contains("1 landed ask waits for your review"));
    // Its review: two operations to review, the patch left as it was with
    // no mark.
    ask(port, &host, &input, "k Esc -\nk Enter -\n");
    until(&mut events, "the title", |l| text(l).contains(" stack operations · · 2 to review"));
    for said in ["   · modify  1 Bug 1 - shout two", "   · add     2 Bug 1 - a c too", "     noop    3 Bug 2 - add b"] {
        until(&mut events, said, |l| text(l).contains(said));
    }
    // The first accepted, alone, the cursor gone below it: its mark green,
    // the terminal's colour 2.
    ask(port, &host, &input, "k Down -\nk Enter -\nk Down -\n");
    until(&mut events, "the title counted", |l| text(l).contains(" stack operations · ✓ 1 accepted, · 1 to review"));
    until(&mut events, "its mark, green", |l| {
        text(l).contains("   ✓ modify  1 Bug 1 - shout two") && runs(l).contains(&("2,,0".to_string(), "✓".to_string()))
    });
}

#[test]
fn the_mouse_over_the_review_s_list_marks_a_line_and_a_click_picks_it_in_the_browser_too() {
    let (tmp, db) = setup();
    work_on_two(tmp.path(), &db, "why so loud?", true, false);
    stacked::config::init(stacked::config::Config::default());
    let (port, token) = stacked::web::start(db.clone()).unwrap();
    let (host, input) = (format!("127.0.0.1:{port}"), format!("POST /input?t={token}"));
    let mut events = events(port, &token, 120);
    until(&mut events, "the banner", |l| text(l).contains("1 landed ask waits for your review"));
    ask(port, &host, &input, "k Esc -\nk Enter -\n");
    let add = "add     2 Bug 1 - a c too";
    let seen = until(&mut events, "the review", |l| text(l).contains(add));
    let (x, y) = cell_of(seen.last().unwrap(), add);
    let row = format!("data: r {y} ");
    // The modifiers of the line's run that names the patch.
    let mods = |l: &str| runs(l).into_iter().find(|(_, t)| t.contains("Bug 1 - a c too")).and_then(|(s, _)| s.rsplit(',').next()?.parse::<u32>().ok());
    // The mouse over it: underlined (8), and not picked, not reversed (64).
    ask(port, &host, &input, &format!("v {x} {y}\n"));
    until(&mut events, "the line marked", |l| l.starts_with(&row) && mods(l).is_some_and(|m| m & 8 != 0 && m & 64 == 0));
    // A click: picked, under the cursor.
    ask(port, &host, &input, &format!("m {x} {y} l\nu {x} {y}\n"));
    until(&mut events, "the line picked", |l| l.starts_with(&row) && mods(l).is_some_and(|m| m & 64 != 0));
}

#[test]
fn the_mouse_over_the_runs_marks_a_run_and_a_click_picks_it_in_the_browser_too() {
    let (_tmp, db) = setup();
    stacked::db::Db::edit(&db, |d| {
        for k in 1..=3 {
            d.add_run(stacked::db::Run {
                id: format!("run-{k}"),
                what: "answer".into(),
                stack: "s".into(),
                state: stacked::db::RunState::Done,
                summary: format!("said {k}"),
                ..Default::default()
            })?;
        }
        Ok(())
    })
    .unwrap();
    stacked::config::init(stacked::config::Config::default());
    let (port, token) = stacked::web::start(db.clone()).unwrap();
    let (host, input) = (format!("127.0.0.1:{port}"), format!("POST /input?t={token}"));
    let mut events = events(port, &token, 120);
    until(&mut events, "the patches", |l| text(l).contains("Bug 2 - add b"));
    // Runs…, the last item of the menu but Quit: the runs, the newest first,
    // and picked.
    ask(port, &host, &input, "k c 20 -\nk End -\nk Up -\nk Enter -\n");
    let seen = until(&mut events, "the runs", |l| text(l).contains("said 2"));
    let (x, y) = cell_of(seen.last().unwrap(), "said 2");
    let row = format!("data: r {y} ");
    // The modifiers of the run's row, one run of text.
    let mods = |l: &str| runs(l).into_iter().find(|(_, t)| t.contains("said 2")).and_then(|(s, _)| s.rsplit(',').next()?.parse::<u32>().ok());
    // The mouse over the second: underlined (8), and not picked, not
    // reversed (64).
    ask(port, &host, &input, &format!("v {x} {y}\n"));
    until(&mut events, "the run marked", |l| l.starts_with(&row) && mods(l).is_some_and(|m| m & 8 != 0 && m & 64 == 0));
    // A click: picked, under the cursor.
    ask(port, &host, &input, &format!("m {x} {y} l\nu {x} {y}\n"));
    until(&mut events, "the run picked", |l| l.starts_with(&row) && mods(l).is_some_and(|m| m & 64 != 0));
}

#[test]
fn a_review_closed_in_part_closes_once_its_check_away_from_the_screen_is_done_in_the_browser_too() {
    let (tmp, db) = setup();
    work_on_two(tmp.path(), &db, "why so loud?", true, false);
    stacked::config::init(stacked::config::Config::default());
    let (port, token) = stacked::web::start(db.clone()).unwrap();
    let (host, input) = (format!("127.0.0.1:{port}"), format!("POST /input?t={token}"));
    let mut events = events(port, &token, 120);
    until(&mut events, "the banner", |l| text(l).contains("1 landed ask waits for your review"));
    // The patch the work adds rejected, alone: nothing needs it.
    ask(port, &host, &input, "k Esc -\nk Enter -\n");
    until(&mut events, "the title", |l| text(l).contains(" stack operations · · 2 to review"));
    ask(port, &host, &input, "k Down -\nk Down -\nk Delete -\n");
    until(&mut events, "the add rejected", |l| text(l).contains(" stack operations · ✗ 1 rejected, · 1 to review"));
    // The rest accepted, closed: the stack the operations accepted leave is
    // checked away from the screen, then the review closes, as the page
    // shows once it is done.
    ask(port, &host, &input, "k Esc -\nk Up -\nk Up -\nk Up -\nk Enter -\n");
    until(&mut events, "the question", |l| text(l).contains("accept every part not marked yet?"));
    ask(port, &host, &input, "k Up -\nk Enter -\n");
    until(&mut events, "closed", |l| text(l).contains("reviewed: its change accepted in part, 1 operation accepted and 1 rejected (add 2)"));
}

#[test]
fn a_thread_the_agent_opened_written_in_its_review_sends_the_work_back_in_the_browser_too() {
    use stacked::thread::{Role, append_turn};
    let (tmp, db) = setup();
    // The comment on `+TWO` is the agent's, which you replied to; the work
    // replies to nothing.
    let n = work_on_two(tmp.path(), &db, "Then quiet it.", false, true);
    stacked::db::Db::edit(&db, |d| {
        let text = append_turn(&append_turn("", Role::Agent, "This shouts.", Some("change")), Role::You, "Then quiet it.", None);
        d.set_note(stacked::db::Note { text, by: Some("agent".into()), ..n.clone() })
    })
    .unwrap();
    stacked::config::init(stacked::config::Config::default());
    let (port, token) = stacked::web::start(db.clone()).unwrap();
    let (host, input) = (format!("127.0.0.1:{port}"), format!("POST /input?t={token}"));
    let mut events = events(port, &token, 120);
    until(&mut events, "the banner", |l| text(l).contains("1 landed ask waits for your review"));
    // In the patch as the work leaves it, Enter on that thread, at its end:
    // what you said last, written again, added to.
    ask(port, &host, &input, "k Esc -\nk Enter -\n");
    until(&mut events, "the review", |l| text(l).contains("modify  1 Bug 1 - shout two"));
    ask(port, &host, &input, "k Down -\nk Right -\nk Enter -\nk End -\nk Enter -\n");
    until(&mut events, "the editor", |l| text(l).contains("your reply: edit it, or add to it"));
    typed_in(port, &host, &input, " Now.");
    ask(port, &host, &input, "k Esc -\nk Enter -\n");
    until(&mut events, "what you wrote", |l| text(l).contains("✎ a.txt: TWO · Then quiet it. Now."));
    ask(port, &host, &input, "k Esc -\n");
    until(&mut events, "the dialog", |l| text(l).contains("Send it back to the agent"));
    let d = stacked::db::Db::load(&db).unwrap();
    let now = d.note(&n.change, &n.id).unwrap();
    assert_eq!((now.by.as_deref(), now.review.as_deref()), (Some("agent"), Some("run-agent")));
}

#[test]
fn one_sets_the_change_from_a_version_to_the_next_as_a_patch_of_its_own_in_the_browser_too() {
    let (_tmp, db) = setup();
    stacked::db::Db::edit(&db, |d| {
        let old = d.patch(&d.stack("s")?.patches[0]).clone();
        let draft = stacked::mbox::Draft { diff: old.diff.replace("+TWO", "+Two"), ..stacked::mbox::Draft::from(&old) };
        stacked::ops::replace(d, "s", 0, draft, "edit").map(|_| ())
    })
    .unwrap();
    stacked::config::init(stacked::config::Config::default());
    let (port, token) = stacked::web::start(db).unwrap();
    let (host, input) = (format!("127.0.0.1:{port}"), format!("POST /input?t={token}"));
    let mut events = events(port, &token, 120);
    until(&mut events, "the patches", |l| l.starts_with("data: r ") && l.contains("Bug 2 - add b"));
    // The first patch's versions, from its menu, a right click opening it.
    ask(port, &host, &input, "m 5 2 r\n");
    let seen = until(&mut events, "the patch's menu", |l| text(l).contains("Versions of this patch…"));
    let (x, y) = cell_of(seen.last().unwrap(), "Versions of this patch…");
    ask(port, &host, &input, &format!("m {x} {y} l\n"));
    until(&mut events, "the versions", |l| text(l).contains("what changed from the version before"));
    // 1, the dialog of the views of the interdiffs; a click on what is
    // compared opens its list, and a click on the code there sets it: the
    // change from v1 to v2 as a patch of its own at once, its files read
    // away from the screen. Done keeps it.
    ask(port, &host, &input, "k c 31 -\n");
    let line = "Compare: [the two diffs]";
    let seen = until(&mut events, "what is compared, in the dialog", |l| text(l).contains(line));
    let (x, y) = cell_of(seen.last().unwrap(), line);
    ask(port, &host, &input, &format!("m {x} {y} l\n"));
    let value = "│ the code ";
    let seen = until(&mut events, "the code in the list", |l| text(l).contains(value));
    let (x, y) = cell_of(seen.last().unwrap(), value);
    ask(port, &host, &input, &format!("m {x} {y} l\n"));
    // What the screen says of the change and its line may come before the
    // dialog says the code is set, or after Done, once the dialog no longer
    // hides them.
    let mut seen = until(&mut events, "the code set", |l| text(l).contains("Compare: [the code]"));
    ask(port, &host, &input, "k End -\nk Enter -\n");
    let said = |l: &str| text(l).contains("The code is the change from v1 to v2");
    let line = |l: &str| text(l).trim_end().ends_with("+Two") && !text(l).contains("++Two");
    if !seen.iter().any(|l| said(l)) {
        seen.extend(until(&mut events, "the change said", said));
    }
    if !seen.iter().any(|l| line(l)) {
        until(&mut events, "its line", line);
    }
}

#[test]
fn one_sets_the_view_of_the_interdiffs_of_a_patch_s_versions_in_the_browser_too() {
    let (_tmp, db) = setup();
    reworded(&db, "Bug 1 - shout two, reworded");
    stacked::config::init(stacked::config::Config::default());
    let (port, token) = stacked::web::start(db).unwrap();
    let (host, input) = (format!("127.0.0.1:{port}"), format!("POST /input?t={token}"));
    let mut events = events(port, &token, 120);
    // The first patch's title, longer, over two rows.
    until(&mut events, "the patches", |l| l.starts_with("data: r 4 ") && l.contains("Bug 2 - add b"));
    // The first patch's versions, from its menu, a right click opening it.
    ask(port, &host, &input, "m 5 2 r\n");
    let seen = until(&mut events, "the patch's menu", |l| text(l).contains("Versions of this patch…"));
    let (x, y) = cell_of(seen.last().unwrap(), "Versions of this patch…");
    ask(port, &host, &input, &format!("m {x} {y} l\n"));
    until(&mut events, "the versions", |l| text(l).contains("what changed from the version before"));
    // 1: the dialog of the views of the interdiffs; a click on the view
    // opens its list, and a click on side by side there sets it, the
    // version's pane side by side at once, each half under a heading of its
    // version.
    ask(port, &host, &input, "k c 31 -\n");
    let line = "View: [interdiff]";
    let seen = until(&mut events, "the view, in the dialog", |l| text(l).contains(line));
    let (x, y) = cell_of(seen.last().unwrap(), line);
    ask(port, &host, &input, &format!("m {x} {y} l\n"));
    let value = "│ side by side ";
    let seen = until(&mut events, "side by side in the list", |l| text(l).contains(value));
    let (x, y) = cell_of(seen.last().unwrap(), value);
    ask(port, &host, &input, &format!("m {x} {y} l\n"));
    until(&mut events, "the versions side by side", |l| text(l).contains("│v2"));
}

#[test]
fn one_sets_only_what_the_patches_changed_of_a_patch_s_versions_in_the_browser_too() {
    let (_tmp, db) = setup();
    stacked::db::Db::edit(&db, |d| {
        let old = d.patch(&d.stack("s")?.patches[0]).clone();
        let draft = stacked::mbox::Draft { diff: old.diff.replace("+TWO", "+Two"), ..stacked::mbox::Draft::from(&old) };
        stacked::ops::replace(d, "s", 0, draft, "edit").map(|_| ())
    })
    .unwrap();
    stacked::config::init(stacked::config::Config::default());
    let (port, token) = stacked::web::start(db).unwrap();
    let (host, input) = (format!("127.0.0.1:{port}"), format!("POST /input?t={token}"));
    let mut events = events(port, &token, 120);
    until(&mut events, "the patches", |l| l.starts_with("data: r ") && l.contains("Bug 2 - add b"));
    // The first patch's versions, from its menu, a right click opening it.
    ask(port, &host, &input, "m 5 2 r\n");
    let seen = until(&mut events, "the patch's menu", |l| text(l).contains("Versions of this patch…"));
    let (x, y) = cell_of(seen.last().unwrap(), "Versions of this patch…");
    ask(port, &host, &input, &format!("m {x} {y} l\n"));
    until(&mut events, "the versions", |l| text(l).contains("what changed from the version before"));
    // 1, the dialog of the views of the interdiffs; a click on the lines
    // opens their list, and a click on only what changed there sets it:
    // the version's pane drawn at once without the lines the two versions
    // have alike, each place they were saying how many. Done keeps it.
    ask(port, &host, &input, "k c 31 -\n");
    let line = "Lines: [every line]";
    let seen = until(&mut events, "the lines, in the dialog", |l| text(l).contains(line));
    let (x, y) = cell_of(seen.last().unwrap(), line);
    ask(port, &host, &input, &format!("m {x} {y} l\n"));
    let value = "│ only what changed ";
    let seen = until(&mut events, "only what changed in the list", |l| text(l).contains(value));
    let (x, y) = cell_of(seen.last().unwrap(), value);
    ask(port, &host, &input, &format!("m {x} {y} l\n"));
    // What the screen says of what is left out, and the line, may come
    // before the dialog says the lines are set, or after Done, once the
    // dialog no longer hides them.
    let mut seen = until(&mut events, "the lines set", |l| text(l).contains("Lines: [only what changed]"));
    ask(port, &host, &input, "k End -\nk Enter -\n");
    let said = |l: &str| text(l).contains("lines are left out here.");
    let line = |l: &str| text(l).trim_end().ends_with("++Two");
    if !seen.iter().any(|l| said(l)) {
        seen.extend(until(&mut events, "what is left out, said", said));
    }
    if !seen.iter().any(|l| line(l)) {
        until(&mut events, "the line each version says", line);
    }
}

/// On `db`, run `id` of stack `stack`, its ask of kind `way` ("answer",
/// "change") on the whole stack, going on with its agent, its process this
/// one, for as long as the test runs: asked "Why add b?".
fn going_on(db: &Path, id: &str, stack: &str, way: &str) {
    stacked::db::Db::edit(db, |d| {
        let from = d.head(stack).unwrap().0;
        d.add_run(stacked::db::Run {
            id: id.into(),
            what: way.into(),
            stack: stack.into(),
            state: stacked::db::RunState::Started,
            request: "Why add b?".into(),
            pid: Some(std::process::id()),
            ask: Some(serde_json::json!({"way": way, "scope": "stack", "from": from, "notes": []})),
            time: stacked::db::now(),
            ..Default::default()
        })
        .map(|_| ())
    })
    .unwrap();
}

/// On `db`, run `id`, going on, landed: done, its reply waiting for your
/// review, to the comment of its stack that asked "Why add b?", which it
/// writes.
fn answer_landed(db: &Path, id: &str) {
    stacked::db::Db::edit(db, |d| lands_answer(d, id)).unwrap();
}

/// Run `id` of `d`, going on, landed as `answer_landed` lands it.
fn lands_answer(d: &mut stacked::db::Db, id: &str) -> stacked::db::Result<()> {
    let r = d.run(id).cloned().unwrap();
    let asking = stacked::db::Note { change: stacked::db::stack_change(&r.stack), id: format!("{id}-c1"), text: "Why add b?".into(), ..Default::default() };
    d.set_note(asking.clone())?;
    let reply = stacked::held::Reply { change: asking.change, id: asking.id, text: "For the bees.".into(), tag: r.what.clone(), ..Default::default() };
    d.add_run(stacked::db::Run {
        state: stacked::db::RunState::Done,
        pid: None,
        summary: "its work waits for your review: 1 reply".into(),
        held: Some(stacked::held::Held { at: stacked::db::now(), replies: vec![reply], ..Default::default() }),
        ..r
    })
    .map(|_| ())
}

/// On `db`, run `id`, going on, landed: failed, as `summary` says.
fn failed(db: &Path, id: &str, summary: &str) {
    stacked::db::Db::edit(db, |d| fails(d, id, summary)).unwrap();
}

/// Run `id` of `d`, going on, landed as `failed` lands it.
fn fails(d: &mut stacked::db::Db, id: &str, summary: &str) -> stacked::db::Result<()> {
    let r = d.run(id).cloned().unwrap();
    d.add_run(stacked::db::Run { state: stacked::db::RunState::Failed, pid: None, summary: summary.into(), ..r }).map(|_| ())
}

/// On `db`, runs `answer` and `change`, going on, landed at once, in one
/// edit, which the editor reads whole: the answer as `answer_landed` lands
/// it, the change failed as `summary` says.
fn landed_at_once(db: &Path, answer: &str, change: &str, summary: &str) {
    stacked::db::Db::edit(db, |d| {
        lands_answer(d, answer)?;
        fails(d, change, summary)
    })
    .unwrap();
}

/// The notices among the events `seen`: each its run, its title and its
/// body.
fn notices(seen: &[String]) -> Vec<(String, String, String)> {
    seen.iter()
        .filter_map(|l| {
            let mut fields = l.strip_prefix("data: n ")?.split('\u{1f}');
            Some((fields.next()?.to_string(), fields.next()?.to_string(), fields.next()?.to_string()))
        })
        .collect()
}

/// A notice as `notices` has it.
fn notice(run: &str, title: &str, body: &str) -> (String, String, String) {
    (run.into(), title.into(), body.into())
}

/// Whether event `l` is a notice.
fn is_notice(l: &str) -> bool {
    l.starts_with("data: n ")
}

/// Whether event `l` is the bell.
fn is_bell(l: &str) -> bool {
    l == "data: b"
}

/// How many times the bell is rung among the events `seen`.
fn bells(seen: &[String]) -> usize {
    seen.iter().filter(|l| is_bell(l)).count()
}

/// The notice of run `run` on stack s, landed as `answer_landed` lands it.
fn answer_notice(run: &str) -> (String, String, String) {
    notice(run, "stacked: s", "A landed ask waits for your review: 1 reply, asked \"Why add b?\".")
}

#[test]
fn what_lands_for_review_or_fails_on_any_stack_is_a_notice_and_a_click_on_one_opens_its_review_in_the_browser() {
    let (tmp, db) = setup();
    // Stack t too, made last: the one the editor shows.
    let out = stacked(&tmp.path().join("home"))
        .current_dir(tmp.path().join("src"))
        .args(["-f", db.to_str().unwrap(), "import", "t", "HEAD~1"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    going_on(&db, "run-answer", "s", "answer");
    going_on(&db, "run-change", "t", "change");
    stacked::config::init(stacked::config::Config::default());
    let (port, token) = stacked::web::start(db.clone()).unwrap();
    let (host, input) = (format!("127.0.0.1:{port}"), format!("POST /input?t={token}"));
    let mut events = events(port, &token, 120);
    let seen = until(&mut events, "t's banner", |l| text(l).contains("the agent works on 1 ask"));
    assert!(notices(&seen).is_empty(), "{seen:#?}");
    // The answer on s, the stack not shown, lands for your review: its
    // notice, titled with its stack, says so as the banner would.
    answer_landed(&db, "run-answer");
    let seen = until(&mut events, "the answer's notice", is_notice);
    assert_eq!(notices(&seen), [notice("run-answer", "stacked: s", "A landed ask waits for your review: 1 reply, asked \"Why add b?\".")]);
    // The change on t fails: as the bottom line says it.
    failed(&db, "run-change", "the agent failed (1): rate limited");
    let seen = until(&mut events, "the failure's notice", is_notice);
    assert_eq!(notices(&seen), [notice("run-change", "stacked: t", "The agent's change failed: the agent failed (1): rate limited.")]);
    // The answer's notice clicked: stack s shown, the review of its work
    // open.
    ask(port, &host, &input, "n run-answer\n");
    until(&mut events, "the review", |l| text(l).contains(" the agent's work: 1 reply"));
}

#[test]
fn what_was_there_when_a_page_came_is_told_it_by_no_notice_in_the_browser() {
    let (_tmp, db) = setup();
    // Landed before `stacked web` started: an answer, waiting for your
    // review, and a change that failed. And two asks going on.
    going_on(&db, "run-before", "s", "answer");
    answer_landed(&db, "run-before");
    going_on(&db, "run-failed", "s", "change");
    failed(&db, "run-failed", "the agent failed (1): rate limited");
    going_on(&db, "run-first", "s", "answer");
    going_on(&db, "run-next", "s", "change");
    stacked::config::init(stacked::config::Config::default());
    let (port, token) = stacked::web::start(db.clone()).unwrap();
    let mut first = events(port, &token, 120);
    let mut seen = until(&mut first, "the banner", |l| text(l).contains("1 landed ask waits for your review"));
    // What lands next: its notice alone.
    answer_landed(&db, "run-first");
    seen.extend(until(&mut first, "the notice", is_notice));
    assert_eq!(notices(&seen), [notice("run-first", "stacked: s", "A landed ask waits for your review: 1 reply, asked \"Why add b?\".")]);
    // A second page, come after it landed: none of it, but the next landing,
    // as the first page is told it too.
    let mut second = events(port, &token, 120);
    let mut seen = until(&mut second, "its banner", |l| text(l).contains("2 landed asks wait for your review"));
    failed(&db, "run-next", "the agent failed (1): rate limited");
    seen.extend(until(&mut second, "its notice", is_notice));
    let next = || notice("run-next", "stacked: s", "The agent's change failed: the agent failed (1): rate limited.");
    assert_eq!(notices(&seen), [next()]);
    assert_eq!(notices(&until(&mut first, "the next notice", is_notice)), [next()]);
}

#[test]
fn what_lands_at_once_is_told_by_its_notices_then_one_bell_with_bell_true_in_the_settings_in_the_browser() {
    let (tmp, db) = setup();
    let home = tmp.path().join("home");
    settings_of(&home, "[web]\nbell = true\n");
    going_on(&db, "run-answer", "s", "answer");
    going_on(&db, "run-change", "s", "change");
    going_on(&db, "run-next", "s", "answer");
    let s = served(&home, tmp.path(), &db, &[]);
    let mut events = events(s.port, &s.token, 120);
    let seen = until(&mut events, "the banner", |l| text(l).contains("the agent works on 3 asks"));
    assert_eq!(bells(&seen), 0, "{seen:#?}");
    // An answer and a failure landed at once: their notices, then the bell,
    // once for both, before the screen that says what landed.
    landed_at_once(&db, "run-answer", "run-change", "the agent failed (1): rate limited");
    let seen = until(&mut events, "the landing", |l| text(l).contains("1 landed ask waits for your review"));
    let mut told = notices(&seen);
    told.sort();
    let failure = notice("run-change", "stacked: s", "The agent's change failed: the agent failed (1): rate limited.");
    assert_eq!(told, [answer_notice("run-answer"), failure]);
    assert_eq!(bells(&seen), 1, "{seen:#?}");
    let bell = seen.iter().position(|l| is_bell(l)).unwrap();
    assert!(seen.iter().enumerate().all(|(i, l)| !is_notice(l) || i < bell), "{seen:#?}");
    // The next landing, alone: its notice, then the bell again.
    answer_landed(&db, "run-next");
    let seen = until(&mut events, "the next landing", |l| text(l).contains("2 landed asks wait for your review"));
    assert_eq!(notices(&seen), [answer_notice("run-next")]);
    assert_eq!(bells(&seen), 1, "{seen:#?}");
    s.quit();
}

#[test]
fn a_notice_comes_with_no_bell_with_bell_false_in_the_settings_in_the_browser() {
    let (tmp, db) = setup();
    let home = tmp.path().join("home");
    settings_of(&home, "[web]\nbell = false\n");
    going_on(&db, "run-answer", "s", "answer");
    let s = served(&home, tmp.path(), &db, &[]);
    let mut events = events(s.port, &s.token, 120);
    until(&mut events, "the banner", |l| text(l).contains("the agent works on 1 ask"));
    // Its notice, as ever, and no bell, which would come before the screen
    // that says what landed.
    answer_landed(&db, "run-answer");
    let seen = until(&mut events, "the landing", |l| text(l).contains("1 landed ask waits for your review"));
    assert_eq!(notices(&seen), [answer_notice("run-answer")]);
    assert_eq!(bells(&seen), 0, "{seen:#?}");
    s.quit();
}

#[test]
fn with_notifications_false_in_the_settings_no_notice_is_sent_but_the_bell_rings_for_each_landing_in_the_browser() {
    let (tmp, db) = setup();
    let home = tmp.path().join("home");
    settings_of(&home, "[web]\nnotifications = false\n");
    going_on(&db, "run-answer", "s", "answer");
    going_on(&db, "run-change", "s", "change");
    let s = served(&home, tmp.path(), &db, &[]);
    let mut events = events(s.port, &s.token, 120);
    let seen = until(&mut events, "the banner", |l| text(l).contains("the agent works on 2 asks"));
    assert_eq!(bells(&seen), 0, "{seen:#?}");
    // Each said on the screen as ever, with no notice, a banner's, but the
    // bell, the bell being on unless set: both would come before the screen
    // that says what landed.
    answer_landed(&db, "run-answer");
    let seen = until(&mut events, "the landing", |l| text(l).contains("1 landed ask waits for your review"));
    assert!(notices(&seen).is_empty(), "{seen:#?}");
    assert_eq!(bells(&seen), 1, "{seen:#?}");
    failed(&db, "run-change", "the agent failed (1): rate limited");
    let seen = until(&mut events, "the failure", |l| text(l).contains("the agent's change failed: the agent failed (1): rate limited"));
    assert!(notices(&seen).is_empty(), "{seen:#?}");
    assert_eq!(bells(&seen), 1, "{seen:#?}");
    s.quit();
}

#[test]
fn neither_notice_nor_bell_is_sent_with_notifications_and_bell_false_in_the_settings_in_the_browser() {
    let (tmp, db) = setup();
    let home = tmp.path().join("home");
    settings_of(&home, "[web]\nnotifications = false\nbell = false\n");
    going_on(&db, "run-answer", "s", "answer");
    going_on(&db, "run-change", "s", "change");
    let s = served(&home, tmp.path(), &db, &[]);
    let mut events = events(s.port, &s.token, 120);
    let mut seen = until(&mut events, "the banner", |l| text(l).contains("the agent works on 2 asks"));
    // Each said on the screen as ever, and nothing else, though a notice
    // and the bell would come before the screen that says what landed.
    answer_landed(&db, "run-answer");
    seen.extend(until(&mut events, "the landing", |l| text(l).contains("1 landed ask waits for your review")));
    failed(&db, "run-change", "the agent failed (1): rate limited");
    seen.extend(until(&mut events, "the failure", |l| text(l).contains("the agent's change failed: the agent failed (1): rate limited")));
    assert!(notices(&seen).is_empty(), "{seen:#?}");
    assert_eq!(bells(&seen), 0, "{seen:#?}");
    s.quit();
}

/// The revision Lando's job 4242 pushed to try, as the stand-in for Lando
/// and Treeherder has it.
#[cfg(feature = "try-push")]
const ON_TRY: &str = "0123456789abcdef0123456789abcdef01234567";

/// The stand-in for Lando and Treeherder, for this process's editors,
/// which look at the pushes they follow each time they launch what may go:
/// Lando's job 4242 pushed to try as revision `ON_TRY`, which Treeherder
/// has as its push 77, its tasks done, `test-a` as success, `test-b` as
/// testfailed. Nothing else is answered, and nothing reaches Lando nor
/// Treeherder.
#[cfg(feature = "try-push")]
fn lando_and_treeherder() {
    static SET: std::sync::Once = std::sync::Once::new();
    SET.call_once(|| {
        stacked::try_push::look_every(0);
        stacked::try_push::web_stand_in(Some(Box::new(|url: &str| {
            let job = serde_json::json!({"id": 4242, "status": "LANDED", "commit_id": ON_TRY});
            let push = serde_json::json!({"results": [{"id": 77, "revision": ON_TRY}]});
            let jobs = serde_json::json!({
                "results": [["test-a", "completed", "success", "T1", 0], ["test-b", "completed", "testfailed", "T2", 0]],
                "job_property_names": ["job_type_name", "state", "result", "task_id", "retry_id"],
                "next": null,
            });
            match url {
                "https://lando.moz.tools/landing_jobs/4242/" => Ok(job.to_string()),
                "https://treeherder.mozilla.org/api/jobs/?push_id=77&count=2000" => Ok(jobs.to_string()),
                u if u == format!("https://treeherder.mozilla.org/api/project/try/push/?revision={ON_TRY}") => Ok(push.to_string()),
                _ => Err(format!("the stand-in for Lando and Treeherder does not answer {url}")),
            }
        })));
    });
}

/// On stack `s` of `db`, its comment `s-try`, and the try push it asked
/// for, of the tasks `test-a` and `test-b`: pushed by Lando's job 4242,
/// followed, and not looked at yet.
#[cfg(feature = "try-push")]
fn pushed_to_try(db: &Path) {
    use stacked::try_push::{Lando, Push, Step};
    stacked::db::Db::edit(db, |d| {
        let rev = d.head("s").unwrap().0;
        let asking = stacked::db::Note { change: stacked::db::stack_change("s"), id: "s-try".into(), text: "Run the tests on try.".into(), ..Default::default() };
        d.set_note(asking.clone())?;
        let push = Push {
            change: asking.change,
            id: asking.id,
            work: "run-tasks".into(),
            rev,
            patches: 2,
            whole: true,
            labels: vec!["test-a".into(), "test-b".into()],
            step: Step::Pushed,
            link: Some("https://treeherder.mozilla.org/jobs?repo=try&landoInstance=lando-prod-2025&landoCommitID=4242".into()),
            lando: Some(Lando { instance: "lando-prod-2025".into(), job: 4242 }),
            ..Default::default()
        };
        d.add_run(stacked::db::Run {
            id: "run-push".into(),
            what: stacked::db::TRY_PUSH.into(),
            stack: "s".into(),
            state: stacked::db::RunState::Started,
            push: Some(Box::new(push)),
            time: stacked::db::now(),
            ..Default::default()
        })
        .map(|_| ())
    })
    .unwrap();
}

#[cfg(feature = "try-push")]
#[test]
fn a_try_push_followed_done_on_treeherder_is_a_notice_saying_how_many_of_its_tasks_failed_in_the_browser() {
    let (_tmp, db) = setup();
    lando_and_treeherder();
    stacked::config::init(stacked::config::Config::default());
    let (port, token) = stacked::web::start(db.clone()).unwrap();
    let mut events = events(port, &token, 120);
    until(&mut events, "the patches", |l| text(l).contains("Bug 2 - add b"));
    // Pushed, then followed once the editor knows of it: done on
    // Treeherder, one of its two tasks failed, its summary waiting for your
    // review.
    pushed_to_try(&db);
    let seen = until(&mut events, "the push's notice", is_notice);
    assert_eq!(notices(&seen), [notice("run-push", "stacked: s", "The try push is done: 1 of its 2 tasks failed. Its summary waits for your review.")]);
}

/// A stand-in for the editor at `dir/NAME`, which writes `ran` beside it as
/// it starts, then waits for `exit` to be there, as an editor waits for you
/// to be done, and exits, what it was given to edit as it was.
fn waiting_editor(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    let script = format!("#!/bin/sh\ntouch '{}'\nwhile [ ! -e '{}' ]; do sleep 0.05; done\n", dir.join("ran").display(), dir.join("exit").display());
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// As `served`, its editor the program at `editor`, as `$VISUAL` and
/// `$EDITOR` name it.
fn served_editing(home: &Path, dir: &Path, db: &Path, editor: &Path) -> Served {
    let mut cmd = web_in(home, dir, db, &[]);
    cmd.env("VISUAL", editor).env("EDITOR", editor);
    started(cmd).unwrap_or_else(|said| panic!("it never said where it is: {said:#?}"))
}

/// Waits for `path` to be there: whether it is.
fn there(path: &Path) -> bool {
    let started = Instant::now();
    while started.elapsed() < PATIENCE {
        if path.exists() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    path.exists()
}

/// The settings of an editor run in the terminal `stacked web` was started
/// from, rather than in the page.
const IN_THE_TERMINAL: &str = "[web]\neditor = \"terminal\"\n";

/// Each way an editor runs elsewhere than in the page, its page told so
/// meanwhile: the settings that say so, and the stand-in's name. In the
/// terminal `stacked web` was started from, as its settings say; or in a
/// window of its own, as an editor named `code` has, whatever they say.
const ELSEWHERE: [(&str, &str); 2] = [(IN_THE_TERMINAL, "slowedit"), ("", "code")];

#[test]
fn while_the_editor_runs_elsewhere_than_in_the_page_the_page_says_so_over_its_screen_a_page_come_meanwhile_too_until_the_screen_after() {
    for (settings, name) in ELSEWHERE {
        let (tmp, db) = setup();
        let home = tmp.path().join("home");
        settings_of(&home, settings);
        let editor = waiting_editor(tmp.path(), name);
        let s = served_editing(&home, tmp.path(), &db, &editor);
        let mut first = events(s.port, &s.token, 400);
        until(&mut first, "the patches", |l| text(l).contains("Bug 2 - add b"));
        // The first patch edited in $EDITOR: the page told first, the
        // program it runs named.
        edit_first_patch(s.port, &s.token, &mut first);
        let said = format!("data: e {name}");
        until(&mut first, "the editor's word", |l| l == said);
        // A page come while the editor runs: told it at once, after which
        // stacked serves it, before any screen, as the editor's thread reads
        // nothing meanwhile.
        assert!(there(&tmp.path().join("ran")), "{name}: the editor never ran");
        let mut second = events(s.port, &s.token, 400);
        let seen = until(&mut second, "its word", |l| l.starts_with("data: ") && !l.starts_with("data: v "));
        assert_eq!(seen.last(), Some(&said), "{seen:#?}");
        assert_eq!(seen.iter().filter(|l| l.starts_with("data: ")).count(), 2, "{seen:#?}");
        // The editor done: each page sent the screen whole, which takes the
        // place of the word, the bottom line saying what came of it.
        std::fs::write(tmp.path().join("exit"), "").unwrap();
        for page in [&mut first, &mut second] {
            let seen = until(page, "the screen after", |l| l.starts_with("data: s "));
            assert!(!seen.iter().any(|l| l.starts_with("data: r ")), "{seen:#?}");
            until(page, "what came of it", |l| text(l).contains("nothing changed"));
        }
        s.quit();
    }
}

#[test]
fn an_editor_that_cannot_run_in_the_terminal_is_said_over_the_screen_until_the_screen_after_says_why_in_the_browser() {
    let (tmp, db) = setup();
    let home = tmp.path().join("home");
    settings_of(&home, IN_THE_TERMINAL);
    let editor = tmp.path().join("nowhere").join("noedit");
    let s = served_editing(&home, tmp.path(), &db, &editor);
    let mut events = events(s.port, &s.token, 400);
    until(&mut events, "the patches", |l| text(l).contains("Bug 2 - add b"));
    edit_first_patch(s.port, &s.token, &mut events);
    until(&mut events, "the editor's word", |l| l == "data: e noedit");
    // Over at once: the screen whole, the bottom line saying why.
    let seen = until(&mut events, "the screen after", |l| l.starts_with("data: s "));
    assert!(!seen.iter().any(|l| l.starts_with("data: r ")), "{seen:#?}");
    let why = format!("{} failed; nothing changed", editor.display());
    until(&mut events, "why", |l| text(l).contains(&why));
    s.quit();
}

#[test]
fn what_the_pages_send_while_the_editor_runs_elsewhere_than_in_the_page_is_dropped_but_a_new_size_and_what_they_send_after_is_done_in_the_browser() {
    for (settings, name) in ELSEWHERE {
        let (tmp, db) = setup();
        let home = tmp.path().join("home");
        settings_of(&home, settings);
        let editor = waiting_editor(tmp.path(), name);
        let s = served_editing(&home, tmp.path(), &db, &editor);
        let (host, input) = (format!("127.0.0.1:{}", s.port), format!("POST /input?t={}", s.token));
        let mut first = events(s.port, &s.token, 400);
        until(&mut first, "the patches", |l| text(l).contains("Bug 2 - add b"));
        edit_first_patch(s.port, &s.token, &mut first);
        let said = format!("data: e {name}");
        until(&mut first, "the editor's word", |l| l == said);
        assert!(there(&tmp.path().join("ran")), "{name}: the editor never ran");
        let mut second = events(s.port, &s.token, 400);
        until(&mut second, "its word", |l| l == said);
        // While the editor runs: from the first page, keys that would write
        // a comment on the stack and save it, then a click on the second
        // patch; from the second page, ↓ and a right click on the first
        // patch, its menu; then the first page's new size.
        let before = std::fs::read(&db).unwrap();
        ask(s.port, &host, &input, "k Home -\nk Enter -\n");
        typed_in(s.port, &host, &input, "dropped");
        ask(s.port, &host, &input, "k Esc -\nk Enter -\nm 5 3 l\n");
        ask(s.port, &host, &input, "k Down -\nm 5 2 r\n");
        ask(s.port, &host, &input, "s 300 30\n");
        // The editor done: each page sent the screen whole at that size, the
        // bottom line saying what came of the edit, nothing else of it done:
        // the first patch still in the middle, no menu, no comment, nothing
        // written.
        std::fs::write(tmp.path().join("exit"), "").unwrap();
        for page in [&mut first, &mut second] {
            let seen = until(page, "the screen after", |l| l.starts_with("data: s "));
            assert_eq!(seen.last().map(String::as_str), Some("data: s 300 30"), "{seen:#?}");
            let rows = std::cell::RefCell::new(vec![String::new(); 30]);
            until(page, "the screen's last row", |l| {
                shown(&mut rows.borrow_mut(), l);
                l.starts_with("data: r 29 ")
            });
            let rows = rows.into_inner();
            assert!(rows[29].contains("nothing changed") && rows.iter().any(|r| r.contains("+TWO")), "{rows:#?}");
            for done in ["+bee", "Edit this patch in $EDITOR", "dropped", "┌─ a comment on"] {
                assert!(!rows.iter().any(|r| r.contains(done)), "{done}: {rows:#?}");
            }
        }
        assert!(std::fs::read(&db).unwrap() == before, "{name}: the database was written");
        // What is sent after: done.
        ask(s.port, &host, &input, "k Down -\n");
        until(&mut first, "the key after", |l| text(l).contains("+bee"));
        s.quit();
    }
}

/// A stand-in for the editor at `dir/NAME`: sh running `body`.
fn editor_script(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

/// Where, in the events `seen`, the last screen sent whole starts, its
/// size; and whether it is the editor's own, run in the page, `t` after its
/// size.
fn last_whole(seen: &[String]) -> (usize, bool) {
    let at = seen.iter().rposition(|l| l.starts_with("data: s ")).unwrap_or_else(|| panic!("no screen whole: {seen:#?}"));
    let next = seen[at + 1..].iter().find(|l| l.starts_with("data: "));
    (at, next.is_some_and(|l| l == "data: t"))
}

#[test]
fn a_terminal_editor_runs_in_the_page_its_screen_sent_as_rows_and_the_line_typed_there_is_what_stacked_takes_of_the_edit() {
    let (tmp, db) = setup();
    // Its screen: red, bold and underlined, then 256's 196, then on RGB, a
    // wide character; its size; then the line typed in the page, put in the
    // first patch's title, as an editor would.
    let editor = editor_script(
        tmp.path(),
        "pageedit",
        "printf '\\033[1;4;31mred\\033[0m \\033[38;5;196mx\\033[0m\\033[48;2;16;32;48my\\033[0m 日\\n'\n\
         printf 'size %s\\n' \"$(stty size)\"\n\
         printf 'title: '\n\
         IFS= read -r line\n\
         sed \"s/shout two/$line/\" \"$1\" > \"$1.new\" && mv \"$1.new\" \"$1\"",
    );
    let s = served_editing(&tmp.path().join("home"), tmp.path(), &db, &editor);
    let (host, input) = (format!("127.0.0.1:{}", s.port), format!("POST /input?t={}", s.token));
    let mut events = events(s.port, &s.token, 120);
    until(&mut events, "the patches", |l| text(l).contains("Bug 2 - add b"));
    edit_first_patch(s.port, &s.token, &mut events);
    // Nothing said over the screen: the editor's own, whole, its size, the
    // page's, then `t`, then its rows, as the editor drew them, the cursor
    // where it left it.
    let seen = until(&mut events, "its screen", |l| text(l).starts_with("title: "));
    assert!(!seen.iter().any(|l| l.starts_with("data: e ")), "{seen:#?}");
    let (at, its) = last_whole(&seen);
    assert!(its && seen[at] == "data: s 120 30", "{seen:#?}");
    let row = |y: usize| seen[at..].iter().rev().find(|l| l.starts_with(&format!("data: r {y} "))).map(|l| runs(l)).unwrap_or_default();
    let first = row(0);
    let styles: Vec<(&str, &str)> = first.iter().map(|(s, t)| (s.as_str(), t.as_str())).take(4).collect();
    assert_eq!(styles, [("1,,9", "red"), (",,0", " "), ("#ff0000,,0", "x"), (",#102030,0", "y")], "{first:?}");
    // Then the wide character, and the blanks after it, which `until` trims.
    assert_eq!(first[4], (",,0".to_string(), " 日".to_string()), "{first:?}");
    assert_eq!(first.len(), 5, "{first:?}");
    assert!(row(1).iter().any(|(_, t)| t.starts_with("size 30 120")), "{seen:#?}");
    assert!(row(2).iter().any(|(s, t)| s == ",,65536" && t == " "), "the cursor after `title: `: {:?}", row(2));
    // A line typed in the page: the editor's, which writes it in the title.
    typed_in(s.port, &host, &input, "typed in the page");
    ask(s.port, &host, &input, "k Enter -\n");
    // Stacked's screen whole after, the editor exited: the edit taken, as
    // an edit of the patch is, waiting for your review.
    let seen = until(&mut events, "the edit taken", |l| text(l).contains("it waits for your review"));
    assert!(!last_whole(&seen).1, "stacked's own: {seen:#?}");
    let titled = |l: &str| text(l).contains("Bug 1 - typed in the page");
    if !seen.iter().any(|l| titled(l)) {
        until(&mut events, "the title typed", titled);
    }
    let d = stacked::db::Db::load(&db).unwrap();
    let queue = stacked::held::queue(&d, "s");
    assert!(queue.len() == 1 && stacked::held::by_hand(queue[0]), "{queue:?}");
    s.quit();
}

#[test]
fn a_size_a_page_sends_reaches_the_editor_in_the_page_and_a_page_come_meanwhile_is_sent_its_screen_whole() {
    let (tmp, db) = setup();
    let exit = tmp.path().join("exit");
    // The size of its terminal, said on its first row each tenth of a
    // second, as the editor reads it, until it is to exit.
    let editor = editor_script(
        tmp.path(),
        "sizes",
        &format!("while [ ! -e '{}' ]; do printf '\\033[H\\033[Ksize %s' \"$(stty size)\"; sleep 0.1; done", exit.display()),
    );
    let s = served_editing(&tmp.path().join("home"), tmp.path(), &db, &editor);
    let (host, input) = (format!("127.0.0.1:{}", s.port), format!("POST /input?t={}", s.token));
    let mut first = events(s.port, &s.token, 120);
    until(&mut first, "the patches", |l| text(l).contains("Bug 2 - add b"));
    edit_first_patch(s.port, &s.token, &mut first);
    until(&mut first, "its size", |l| text(l).starts_with("size 30 120"));
    // A new size, the page's: its terminal's, which the editor reads, its
    // screen sent whole at that size.
    ask(s.port, &host, &input, "s 100 20\n");
    let seen = until(&mut first, "the size it reads", |l| text(l).starts_with("size 20 100"));
    assert!(seen.windows(4).any(|w| w[0] == "data: s 100 20" && w[2] == "data: t"), "{seen:#?}");
    // A page come meanwhile: sent the editor's screen whole at once, as
    // the first page has it, before any other screen.
    let mut second = events(s.port, &s.token, 100);
    let seen = until(&mut second, "the editor's screen", |l| text(l).starts_with("size 20 100"));
    let data: Vec<&String> = seen.iter().filter(|l| l.starts_with("data: ")).collect();
    assert!(data[0].starts_with("data: v ") && data[1] == "data: s 100 20" && data[2] == "data: t", "{seen:#?}");
    // Its own size, 100 by 30, the editor's from then on, as the last size
    // a page sent.
    until(&mut second, "its size", |l| text(l).starts_with("size 30 100"));
    until(&mut first, "the second page's size", |l| text(l).starts_with("size 30 100"));
    // The editor done: each page sent stacked's screen whole, at that size,
    // the bottom line saying what came of the edit.
    std::fs::write(&exit, "").unwrap();
    for page in [&mut first, &mut second] {
        let seen = until(page, "what came of it", |l| text(l).contains("nothing changed"));
        let (at, its) = last_whole(&seen);
        assert!(!its && seen[at] == "data: s 100 30", "{seen:#?}");
    }
    s.quit();
}

/// Waits for each process `pids` names to be gone, and reaped: whether they
/// are.
fn ended(pids: &[libc::pid_t]) -> bool {
    let left = || pids.iter().any(|p| unsafe { libc::kill(*p, 0) } == 0);
    let started = Instant::now();
    while left() && started.elapsed() < PATIENCE {
        std::thread::sleep(Duration::from_millis(50));
    }
    !left()
}

#[test]
fn stacked_web_going_while_the_editor_runs_in_the_page_hangs_it_up_and_leaves_nothing_of_it_running() {
    // A ctrl-c where it runs; a kill, the editor deaf to the hangup; its
    // last tab closed.
    for (how, deaf) in [("INT", false), ("TERM", true), ("closed", false)] {
        let (tmp, db) = setup();
        let pids = tmp.path().join("pids");
        // It, and a process it started, both waiting.
        let trap = if deaf { "trap '' HUP\n" } else { "" };
        let editor = editor_script(tmp.path(), "stays", &format!("{trap}sleep 300 &\necho \"$$ $!\" > '{}'\nwait", pids.display()));
        let mut s = served_editing(&tmp.path().join("home"), tmp.path(), &db, &editor);
        let mut page = events(s.port, &s.token, 120);
        until(&mut page, "the patches", |l| text(l).contains("Bug 2 - add b"));
        edit_first_patch(s.port, &s.token, &mut page);
        let started = Instant::now();
        let running: Vec<libc::pid_t> = loop {
            let read: Vec<libc::pid_t> = std::fs::read_to_string(&pids).unwrap_or_default().split_whitespace().filter_map(|w| w.parse().ok()).collect();
            if read.len() == 2 {
                break read;
            }
            assert!(started.elapsed() < PATIENCE, "{how}: the editor never ran");
            std::thread::sleep(Duration::from_millis(50));
        };
        if how == "closed" {
            // Quit as Quit does once no tab came back, the editor hung up on.
            let closed = Instant::now();
            s.close(page);
            s.quits(closed);
        } else {
            // Quit as Quit does: the page told, then stacked web done.
            send(&s.child, how);
            until(&mut page, "bye", |l| l == "data: bye");
            let status = exit_of(&mut s.child);
            let mut rest = String::new();
            s.said.read_to_string(&mut rest).unwrap();
            assert!(status.success(), "{how}: {status}: {rest}");
        }
        assert!(ended(&running), "{how}: left running: {running:?}");
    }
}

#[test]
fn an_editor_that_cannot_run_in_the_page_says_why_in_the_bottom_line_and_nothing_over_the_screen() {
    let (tmp, db) = setup();
    let editor = tmp.path().join("nowhere").join("noedit");
    let s = served_editing(&tmp.path().join("home"), tmp.path(), &db, &editor);
    let mut events = events(s.port, &s.token, 400);
    until(&mut events, "the patches", |l| text(l).contains("Bug 2 - add b"));
    edit_first_patch(s.port, &s.token, &mut events);
    // The screen whole, stacked's, the bottom line saying why; no word said
    // over it.
    let why = format!("{} failed; nothing changed", editor.display());
    let seen = until(&mut events, "why", |l| text(l).contains(&why));
    assert!(!seen.iter().any(|l| l.starts_with("data: e ")), "{seen:#?}");
    assert!(!last_whole(&seen).1, "stacked's own: {seen:#?}");
    s.quit();
}
