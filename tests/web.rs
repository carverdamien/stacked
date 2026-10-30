//! `stacked web`, served for real: what a browser asks of it over HTTP, and
//! what it sends back as the editor's screen changes.
#![cfg(feature = "web")]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
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
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    write!(s, "{request} HTTP/1.1\r\nHost: {host}\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
    let mut all = Vec::new();
    s.read_to_end(&mut all).unwrap();
    let at = all.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    let head = String::from_utf8_lossy(&all[..at]).to_string();
    (head.lines().next().unwrap().to_string(), all[at + 4..].to_vec())
}

/// The events' lines, read until one is as `wanted` says: what it is.
fn until(events: &mut BufReader<TcpStream>, what: &str, wanted: impl Fn(&str) -> bool) -> Vec<String> {
    let started = Instant::now();
    let mut seen = Vec::new();
    while started.elapsed() < Duration::from_secs(20) {
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
    for _ in 0..300 {
        if !dir.exists() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
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
/// `$EDITOR` sending it `signal` (as `kill` names it) as it runs, as a key
/// pressed in its terminal would, and changing nothing but `tmp/edited`,
/// which says it ran. It, what it says on stderr after where it is, and
/// its events.
fn web(tmp: &Path, db: &Path, signal: &str) -> (Running, BufReader<ChildStderr>, BufReader<TcpStream>, u16, String) {
    let (editor, pid, edited) = (tmp.join("editor.sh"), tmp.join("pid"), tmp.join("edited"));
    let script = format!("#!/bin/sh\nkill -{signal} \"$(cat '{}')\"\nsleep 0.2\ntouch '{}'\n", pid.display(), edited.display());
    std::fs::write(&editor, script).unwrap();
    std::fs::set_permissions(&editor, std::fs::Permissions::from_mode(0o755)).unwrap();
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
        assert!(started.elapsed() < Duration::from_secs(20), "still running");
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

#[cfg(feature = "moz-phab")]
#[test]
fn a_patch_is_submitted_to_phabricator_from_its_menu_in_the_browser_too() {
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
    // The first patch's menu, opened with a right click, and its item
    // clicked: what is sent is read, then the dialog says it.
    ask(port, &host, &input, "m 5 2 r\n");
    let item = "Submit to Phabricator as WIP";
    let seen_menu = until(&mut events, "the patch's menu", |l| text(l).contains(item));
    let row = seen_menu.last().unwrap();
    let y = row.strip_prefix("data: r ").and_then(|r| r.split(' ').next()).unwrap();
    let line = text(row);
    let x = line[..line.find(item).unwrap()].chars().count() + 1;
    ask(port, &host, &input, &format!("m {x} {y} l\n"));
    let title = "send patch 1 to Phabricator as a new revision?";
    let seen_dialog = until(&mut events, "the dialog", |l| text(l).contains(title));
    let row = seen_dialog.last().unwrap().strip_prefix("data: r ").and_then(|r| r.split(' ').next()).unwrap().to_string();
    // Esc: nothing is sent, nothing having been but read.
    ask(port, &host, &input, "k Esc -\n");
    until(&mut events, "the dialog closed", |l| l.starts_with(&format!("data: r {row} ")) && !text(l).contains(title));
    assert_eq!(*asked.lock().unwrap(), ["diffusion.repository.search"]);
    stacked::submit::stand_in(None);
}

/// A stand-in for Phabricator, answering the statuses' searches of the
/// editors of `db` alone, and what their Phabricator line reads: D11 needs
/// review, its latest diff the first patch's, but `Two` where the patch
/// says `TWO`.
#[cfg(feature = "moz-phab")]
fn d11_needs_review(db: &Path) {
    let d = stacked::db::Db::load(db).unwrap();
    let raw = d.patch(&d.stack("s").unwrap().patches[0]).diff.replace("+TWO", "+Two");
    stacked::phab::statuses_stand_in(
        db,
        Some(Box::new(move |m, _| match m {
            "differential.revision.search" => Ok(serde_json::json!({"data": [{"id": 11, "phid": "PHID-DREV-11",
                "fields": {"status": {"value": "needs-review"}, "title": "Bug 1 - shout two", "summary": ""}}]})),
            "differential.diff.search" => Ok(serde_json::json!({"data": [{"id": 5}]})),
            "differential.getrawdiff" => Ok(serde_json::Value::String(raw.clone())),
            "transaction.search" => Ok(serde_json::json!({"data": [], "cursor": {"after": null}})),
            _ => Err(format!("{m}: not asked in this test")),
        })),
    );
}

#[cfg(feature = "moz-phab")]
#[test]
fn the_status_of_each_patch_s_revision_and_its_phabricator_line_show_in_the_browser_too() {
    let (_tmp, db) = setup_with("Bug 1 - shout two\n\nDifferential Revision: https://phabricator.example.com/D11");
    d11_needs_review(&db);
    stacked::config::init(stacked::config::Config::default());
    let (port, token) = stacked::web::start(db.clone()).unwrap();
    let (host, input) = (format!("127.0.0.1:{port}"), format!("POST /input?t={token}"));
    let mut events = events(port, &token, 100);
    // A letter after the number; the patch naming no revision blank, its
    // title after the column all the same.
    until(&mut events, "the status", |l| l.starts_with("data: r 2 ") && text(l).starts_with("  1 [R] Bug 1 - shout two"));
    until(&mut events, "the title after it", |l| l.starts_with("data: r 3 ") && text(l).starts_with("  2     Bug 2 - add b"));
    // The Phabricator line over the message, in the middle.
    let line = "Phabricator: D11 · Needs Review · the diff differs · no new comments";
    until(&mut events, "the Phabricator line", |l| text(l).contains(line));
    // Listing files, a letter too.
    ask(port, &host, &input, "k Left -\n");
    until(&mut events, "the letter", |l| l.starts_with("data: r 2 ") && text(l).starts_with("  1 [R] Bug 1 - shout two"));
    // Space on the line: what differs, under it.
    for k in ["k Right -\n", "k Up -\n", "k c 20 -\n"] {
        ask(port, &host, &input, k);
    }
    until(&mut events, "what differs", |l| text(l).contains("the code: Phabricator's, then this version's"));
    until(&mut events, "the line of Phabricator's diff", |l| text(l).contains("│-Two"));
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
/// stand-in, in `dir`, that opens nothing and writes each link it is given
/// on a line of the file this gives. No test opens a browser.
fn stand_in_opener(dir: &Path) -> PathBuf {
    let path = dir.join("open.sh");
    std::fs::write(&path, "#!/bin/sh\nprintf '%s\\n' \"$1\" >> \"$(dirname \"$0\")/opened.txt\"\n").unwrap();
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
        if all.len() >= n || started.elapsed() > Duration::from_secs(20) {
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

#[test]
fn a_link_of_a_line_of_a_message_opens_from_its_menu_and_with_o_in_the_browser_too() {
    let (tmp, db) = setup();
    let here = tmp.path().file_name().unwrap().to_string_lossy().to_string();
    let why = format!("https://example.com/{here}/why");
    reworded(&db, &format!("Bug 1 - shout two\n\nSee {why}"));
    let log = stand_in_opener(tmp.path());
    stacked::config::init(stacked::config::Config::default());
    let (port, token) = stacked::web::start(db).unwrap();
    let (host, input) = (format!("127.0.0.1:{port}"), format!("POST /input?t={token}"));
    let mut events = events(port, &token, 100);
    // A right click on the line of the message opens its menu, and its item
    // opens the link, the status line saying so.
    let seen = until(&mut events, "the line", |l| text(l).contains("See https://"));
    let (x, y) = cell_of(seen.last().unwrap(), "See https://");
    ask(port, &host, &input, &format!("m {x} {y} r\n"));
    let item = format!("Open {why} in your browser");
    let seen = until(&mut events, "the line's menu", |l| text(l).contains(&item));
    let (x, y) = cell_of(seen.last().unwrap(), &item);
    ask(port, &host, &input, &format!("m {x} {y} l\n"));
    until(&mut events, "the link opened", |l| text(l).contains(&format!("opened {why} in your browser")));
    assert_eq!(links_opened(&log, 1), std::slice::from_ref(&why));
    // o, the cursor on the line, opens it again.
    ask(port, &host, &input, "k c 6f -\n");
    assert_eq!(links_opened(&log, 2), [why.clone(), why]);
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

#[test]
fn the_effort_picked_in_a_menu_is_where_the_next_run_starts() {
    let (tmp, db) = setup();
    let home = tmp.path().join("home");
    let file = db.to_str().unwrap();
    // A comment on the stack, for its menu to ask about.
    let out = stacked(&home).current_dir(tmp.path()).args(["-f", file, "comment", "s", "Why?"]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    // `stacked web`, its home the test's; the stack's menu opened on its
    // header, on `Ask agent about all`. What it said first, where it is.
    let serve = || {
        let mut child = stacked(&home)
            .args(["-f", file, "web", "--no-open"])
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
        let (port, token) = url.split_once("/?t=").unwrap();
        let (port, token): (u16, String) = (port.parse().unwrap(), token.to_string());
        let mut events = events(port, &token, 120);
        until(&mut events, "the patches", |l| l.contains("Bug 2 - add b"));
        let press = move |k: &str| {
            ask(port, &format!("127.0.0.1:{port}"), &format!("POST /input?t={token}"), &format!("k {k}\n"));
        };
        for k in ["Home -", "c 20 -", "Down -"] {
            press(k);
        }
        (child, events, press)
    };
    let asking = |l: &str| text(l).contains("Ask agent about all");
    // High at first; Tab goes to the effort, → twice picks max.
    let (mut child, mut events, press) = serve();
    until(&mut events, "high, at first", |l| asking(l) && text(l).contains("Effort [high]"));
    for k in ["Tab -", "Right -", "Right -"] {
        press(k);
    }
    until(&mut events, "max, picked", |l| asking(l) && text(l).contains("Effort [max]"));
    // Quit: kept in the state file of its home, beside the database opened.
    press("c 63 c");
    until(&mut events, "bye", |l| l == "data: bye");
    assert!(child.0.wait().unwrap().success());
    let state = home.join("state.json");
    assert_eq!(stacked::state::effort(&state), Some(stacked::db::Effort::Max));
    // The next run starts at it.
    let (_child, mut events, _) = serve();
    until(&mut events, "max, kept", |l| asking(l) && text(l).contains("Effort [max]"));
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
