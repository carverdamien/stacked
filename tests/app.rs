//! `stacked app`, its editor run as the window runs it, with no window:
//! what the window sends it, and the screens it sends back.
#![cfg(feature = "app")]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use stacked::app::{Input, Output, Session};

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

/// The settings, rather than any file's: an editor that edits nothing
/// (`true`), so that what opens `$EDITOR` opens no real one. First in each
/// test, before any editor reads them: the tests share them, and the first
/// to read them reads them for all.
fn settings() {
    stacked::config::init(stacked::config::Config {
        editor: Some("true".into()),
        ..Default::default()
    });
}

/// A database holding stack `s` of two patches.
fn setup() -> (tempfile::TempDir, PathBuf) {
    settings();
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

/// Row `y` of `buf`, as text.
fn row(buf: &Buffer, y: u16) -> String {
    (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect()
}

fn text(buf: &Buffer) -> String {
    (0..buf.area.height).map(|y| row(buf, y)).collect::<Vec<_>>().join("\n")
}

/// The cell `said` starts at in `buf`, if it is there.
fn find(buf: &Buffer, said: &str) -> Option<(u16, u16)> {
    (0..buf.area.height).find_map(|y| {
        let cells: Vec<&str> = (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect();
        (0..cells.len()).find(|x| cells[*x..].concat().starts_with(said)).map(|x| (x as u16, y))
    })
}

/// What the editor sends, read until it sends what `wanted` says: that.
fn until(s: &Session, what: &str, wanted: impl Fn(&Output) -> bool) -> Output {
    let started = Instant::now();
    let mut last = None;
    while started.elapsed() < Duration::from_secs(20) {
        match s.recv(Duration::from_millis(100)) {
            Some(o) if wanted(&o) => return o,
            Some(o) => last = Some(o),
            None => {}
        }
    }
    match last {
        Some(Output::Screen(buf, _)) => panic!("never sent {what}; the last screen:\n{}", text(&buf)),
        other => panic!("never sent {what}; the last sent: {other:?}"),
    }
}

/// The screens it sends, read until one is as `wanted` says: that one.
fn screen(s: &Session, what: &str, wanted: impl Fn(&Buffer) -> bool) -> Buffer {
    match until(s, what, |o| matches!(o, Output::Screen(buf, _) if wanted(buf))) {
        Output::Screen(buf, _) => buf,
        _ => unreachable!(),
    }
}

fn press(s: &Session, code: KeyCode) {
    s.send(Input::Key(KeyEvent::new(code, KeyModifiers::NONE)));
}

#[test]
fn the_window_s_editor_draws_at_its_size_and_does_what_it_is_sent() {
    let (_tmp, db) = setup();
    assert_eq!(stacked::config::get().editor(), "true", "no real editor");
    let s = Session::open(&db).unwrap();
    // Nothing is drawn before the window says its size.
    assert_eq!(s.recv(Duration::from_millis(400)), None);
    s.send(Input::Size(100, 30));
    let buf = screen(&s, "the patches", |b| row(b, 3).contains("Bug 2 - add b"));
    assert_eq!(buf.area, Rect::new(0, 0, 100, 30));
    // ↓: the second patch picked, the middle its patch.
    press(&s, KeyCode::Down);
    screen(&s, "the second patch", |b| text(b).contains("+bee"));
    // A click on the first patch, the screen's third row, picks it.
    s.send(Input::Click { x: 5, y: 2, right: false });
    screen(&s, "the first patch", |b| text(b).contains("+TWO"));
    // The mouse over the second: underlined, and nothing else picked; gone
    // from the grid, nothing underlined.
    s.send(Input::Hover(5, 3));
    let buf = screen(&s, "the second patch hovered", |b| b[(5, 3)].modifier.contains(Modifier::UNDERLINED));
    assert!(text(&buf).contains("+TWO"), "the first patch still in the middle");
    s.send(Input::Unhover);
    screen(&s, "the mouse gone", |b| !b[(5, 3)].modifier.contains(Modifier::UNDERLINED));
    // A window too small: drawn at the least the editor draws well.
    s.send(Input::Size(10, 3));
    screen(&s, "the least screen", |b| b.area == Rect::new(0, 0, 40, 10));
    s.send(Input::Size(100, 30));
    screen(&s, "the screen as it was", |b| b.area == Rect::new(0, 0, 100, 30));
    // The patch in $EDITOR, from its menu: the window told to wait, then
    // sent the screen after, though the editor changed nothing.
    press(&s, KeyCode::Char(' '));
    let menu = screen(&s, "the patch's menu", |b| find(b, "Edit this patch in $EDITOR").is_some());
    let (x, y) = find(&menu, "Edit this patch in $EDITOR").unwrap();
    s.send(Input::Click { x, y, right: false });
    until(&s, "the window told to wait", |o| *o == Output::Editing);
    screen(&s, "the screen after", |b| !text(b).contains("Edit this patch in $EDITOR"));
    // ctrl-c quits.
    s.send(Input::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)));
    until(&s, "bye", |o| *o == Output::Bye);
}

#[test]
fn stacked_app_is_a_command_of_its_own() {
    let out = Command::new(env!("CARGO_BIN_EXE_stacked"))
        .args(["app", "--help"])
        .env("STACKED_CONFIG", "/nonexistent/stacked.toml")
        .output()
        .unwrap();
    let help = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success() && help.starts_with("StackED: the editor in a window of its own"), "{help}");
}

#[test]
fn a_database_that_cannot_be_read_is_said_before_any_window() {
    settings();
    let tmp = tempfile::tempdir().unwrap();
    // A directory where the database would be.
    let e = Session::open(tmp.path()).err().expect("no session on a directory");
    assert!(!e.0.is_empty());
}
