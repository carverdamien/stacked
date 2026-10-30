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
            "edge.search" => Ok(serde_json::json!({"data": []})),
            "differential.diff.search" => Ok(serde_json::json!({"data": [{"id": 5, "fields": {"revisionPHID": "PHID-DREV-11"}}]})),
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
    // Space on the line: what differs, on a screen of its own, Phabricator's
    // line of the diff taken out; Esc, back on the line.
    for k in ["k Right -\n", "k Up -\n", "k c 20 -\n"] {
        ask(port, &host, &input, k);
    }
    until(&mut events, "what differs", |l| text(l).contains("D11 on Phabricator, then patch 1 as it is"));
    until(&mut events, "the line of Phabricator's diff", |l| text(l).starts_with("-+Two"));
    ask(port, &host, &input, "k Esc -\n");
    until(&mut events, "the line again", |l| text(l).contains(line));
    // Its menu: what differs, and the reviews of D11 pulled alone, clicked,
    // held for review.
    stacked::phab::stand_in(Some(one_on_d11));
    ask(port, &host, &input, "k Enter -\n");
    let item = "Pull the reviews of D11 from Phabricator";
    let seen = until(&mut events, "the line's menu", |l| text(l).contains(item));
    assert!(seen.iter().any(|l| text(l).contains("What differs from D11 on Phabricator…")), "{seen:#?}");
    let row = seen.last().unwrap();
    let y = row.strip_prefix("data: r ").and_then(|r| r.split(' ').next()).unwrap();
    let said = text(row);
    let x = said[..said.find(item).unwrap()].chars().count() + 1;
    ask(port, &host, &input, &format!("m {x} {y} l\n"));
    until(&mut events, "what the pull brought", |l| text(l).contains("1 new comment from D11: it waits for your review"));
    stacked::phab::stand_in(None);
    stacked::phab::statuses_stand_in(&db, None);
}

/// The stand-in for Phabricator in the pull of
/// `the_status_of_each_patch_s_revision_…`: a comment of ann's on D11, none
/// on any other.
#[cfg(feature = "moz-phab")]
fn one_on_d11(source: &str, rev: &str, _url: &str) -> Result<stacked::phab::RevisionComments, String> {
    let ann = stacked::phab::Remote { phid: "c1".into(), author: "ann".into(), text: "Why shout?".into(), created: 1, ..Default::default() };
    let comments = if rev == "D11" { vec![ann] } else { vec![] };
    Ok(stacked::phab::RevisionComments { revision: rev.into(), source: source.into(), comments })
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
    // o, the cursor on the line, opens it again.
    ask(port, &host, &input, "k c 6f -\n");
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
    // A click on the permission's line steps it: answer only, then change
    // stack.
    click(&seen, "Permission: [ — ]");
    let seen = until(&mut events, "answer only", |l| text(l).contains("Permission: [answer only]"));
    click(&seen, "Permission: [answer only]");
    until(&mut events, "change stack", |l| text(l).contains("Permission: [change stack]"));
    // ← → do nothing there; Enter on the effort's line sets low.
    send(&["k Right -", "k Down -", "k Left -", "k Enter -"]);
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
    // Ask agent about all… with Enter, both lines set, then Esc: closed,
    // nothing asked, as Space opens the menu again; opened again, nothing
    // set.
    until(&mut events, "the stack's menu", |l| text(l).contains("Ask agent about all…"));
    send(&["k Down -", "k Enter -"]);
    until(&mut events, "the dialog", |l| text(l).contains("Permission: [ — ]"));
    send(&["k Enter -", "k Down -", "k Enter -", "k Esc -", "k c 20 -"]);
    until(&mut events, "the menu again", |l| text(l).contains("Ask agent about all…"));
    send(&["k Down -", "k Enter -"]);
    until(&mut events, "opened again, unset", |l| text(l).contains("Permission: [ — ]"));
    // Asked later, at low, from the dialog: the comment's own menu, in the
    // comments pane, then has Send it now, at the effort it was batched at.
    send(&["k Enter -", "k Down -", "k Enter -", "k Down -", "k Right -", "k Enter -"]);
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
    // ↓ scrolls it a line, and so does the wheel, which the page sends as ↓
    // for each notch it turns (two here); pgdn a box's worth.
    ask(port, &host, &input, "k Down -\n");
    ask(port, &host, &input, "k Down -\nk Down -\n");
    until(&mut events, "three lines down", |l| {
        shown(&mut rows.borrow_mut(), l);
        first("  reason 3")
    });
    ask(port, &host, &input, "k PageDown -\n");
    until(&mut events, "a box's worth further", |l| {
        shown(&mut rows.borrow_mut(), l);
        first("  reason 8")
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
