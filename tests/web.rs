//! `stacked web`, served for real: what a browser asks of it over HTTP, and
//! what it sends back as the editor's screen changes.
#![cfg(feature = "web")]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::Command;
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

/// A database holding stack `s` of two patches.
fn setup() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir(&src).unwrap();
    git(&src, &["init", "-q", "-b", "main"]);
    git(&src, &["config", "commit.gpgsign", "false"]);
    std::fs::write(src.join("a.txt"), "one\ntwo\n").unwrap();
    git(&src, &["add", "."]);
    git(&src, &["commit", "-qm", "base"]);
    std::fs::write(src.join("a.txt"), "one\nTWO\n").unwrap();
    git(&src, &["commit", "-qam", "Bug 1 - shout two"]);
    std::fs::write(src.join("b.txt"), "bee\n").unwrap();
    git(&src, &["add", "."]);
    git(&src, &["commit", "-qm", "Bug 2 - add b"]);
    let db = tmp.path().join("db.jsonl");
    let out = Command::new(env!("CARGO_BIN_EXE_stacked"))
        .current_dir(&src)
        .args(["-f", db.to_str().unwrap(), "import", "s", "HEAD~2"])
        .env("STACKED_CONFIG", "/nonexistent/stacked.toml")
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
}
