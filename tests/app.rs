//! `stacked app`, its editor run as the window runs it, with no window:
//! what the window sends it, and the screens it sends back; and what the
//! File menu opens, its panels answered by the tests.
#![cfg(feature = "app")]

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use stacked::app::windows::{Ask, File, Opened};
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

/// `program`, a stacked (this one, or the copy in StackED.app), as the
/// tests run it: its own things (its settings, the databases opened last)
/// kept in `home`, a folder of the test's (`$STACKED_HOME`), never in the
/// home of whoever runs the tests; what it writes in the temporary
/// directory, in `tmp` beside it (`$TMPDIR`), removed with the test's
/// folder.
fn stacked_at(program: &Path, home: &Path) -> Command {
    let tmp = home.with_file_name("tmp");
    std::fs::create_dir_all(&tmp).unwrap();
    let mut cmd = Command::new(program);
    cmd.env("STACKED_HOME", home).env("TMPDIR", tmp).env_remove("STACKED_CONFIG").env_remove("STACKED_APP_STATE");
    cmd
}

/// This stacked, as the tests run it (`stacked_at`).
fn stacked(home: &Path) -> Command {
    stacked_at(Path::new(env!("CARGO_BIN_EXE_stacked")), home)
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
    let out = stacked(&tmp.path().join("home"))
        .current_dir(&src)
        .args(["-f", db.to_str().unwrap(), "import", "s", "HEAD~2"])
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
        Some(Output::Screen(buf, ..)) => panic!("never sent {what}; the last screen:\n{}", text(&buf)),
        other => panic!("never sent {what}; the last sent: {other:?}"),
    }
}

/// The screens it sends, read until one is as `wanted` says: that one.
fn screen(s: &Session, what: &str, wanted: impl Fn(&Buffer) -> bool) -> Buffer {
    match until(s, what, |o| matches!(o, Output::Screen(buf, ..) if wanted(buf))) {
        Output::Screen(buf, ..) => buf,
        _ => unreachable!(),
    }
}

fn press(s: &Session, code: KeyCode) {
    s.send(Input::Key(KeyEvent::new(code, KeyModifiers::NONE)));
}

/// The screens it sends for as long as `wait`: the last, if any.
fn latest(s: &Session, wait: Duration) -> Option<Buffer> {
    let started = Instant::now();
    let mut last = None;
    while started.elapsed() < wait {
        if let Some(Output::Screen(buf, ..)) = s.recv(Duration::from_millis(100)) {
            last = Some(buf);
        }
    }
    last
}

#[test]
fn the_window_s_editor_draws_at_its_size_and_does_what_it_is_sent() {
    let (tmp, db) = setup();
    assert_eq!(stacked::config::get().editor(), "true", "no real editor");
    let s = Session::open(&db, tmp.path()).unwrap();
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
    let tmp = tempfile::tempdir().unwrap();
    let out = stacked(&tmp.path().join("home")).args(["app", "--help"]).output().unwrap();
    let help = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success() && help.starts_with("StackED: the editor in a window of its own"), "{help}");
}

#[test]
fn a_database_that_cannot_be_read_is_said_before_any_window() {
    settings();
    let tmp = tempfile::tempdir().unwrap();
    // A directory where the database would be.
    let e = Session::open(tmp.path(), tmp.path()).err().expect("no session on a directory");
    assert!(!e.0.is_empty());
}

#[test]
fn each_window_s_editor_runs_in_its_own_folder() {
    let (tmp, db) = setup();
    let src = tmp.path().join("src");
    // Two editors on one database, in one process, as two windows: one in
    // the checkout the stack was imported from, which says where its HEAD
    // is; one elsewhere, with no checkout and no HEAD to say.
    let there = Session::open(&db, &src).unwrap();
    let elsewhere = Session::open(&db, tmp.path()).unwrap();
    there.send(Input::Size(100, 30));
    elsewhere.send(Input::Size(100, 30));
    screen(&there, "HEAD, after the top patch", |b| text(b).contains("▶ HEAD main"));
    screen(&elsewhere, "the patches", |b| row(b, 3).contains("Bug 2 - add b"));
    let last = latest(&elsewhere, Duration::from_secs(2)).map(|b| text(&b)).unwrap_or_default();
    assert!(!last.contains("▶ HEAD"), "{last}");
}

/// The File menu's panels and alerts, as the test answers them: the
/// databases picked to open, the new database named, and what is said.
#[derive(Default)]
struct Answers {
    databases: Vec<PathBuf>,
    new: Option<PathBuf>,
    said: RefCell<Vec<String>>,
}

impl Ask for Answers {
    fn databases(&self) -> Vec<PathBuf> {
        self.databases.clone()
    }

    fn new_database(&self) -> Option<PathBuf> {
        self.new.clone()
    }

    fn alert(&self, text: &str) {
        self.said.borrow_mut().push(text.to_string());
    }
}

/// The File menu, its recent databases kept in `state`: what it opens, and
/// how many times it woke the window set.
fn file_menu(state: &Path) -> (File, Receiver<Opened>, Rc<Cell<usize>>) {
    let (to, from) = mpsc::channel();
    let woken = Rc::new(Cell::new(0));
    let woke = woken.clone();
    (File::new(to, Box::new(move || woke.set(woke.get() + 1)), state.to_path_buf()), from, woken)
}

#[test]
fn file_open_opens_each_database_picked_in_a_window_of_its_own() {
    let (tmp, db) = setup();
    let src = tmp.path().join("src");
    // A copy in the checkout: its editor runs there, its database's folder,
    // and says where HEAD is.
    let inside = src.join("stacks.jsonl");
    std::fs::copy(&db, &inside).unwrap();
    let (file, from, woken) = file_menu(&tmp.path().join("state.json"));
    // The Open panel's pick: the first database twice, each a window.
    let picked = Answers {
        databases: vec![db.clone(), inside.clone(), db.clone()],
        ..Default::default()
    };
    file.open(&picked);
    assert!(picked.said.borrow().is_empty(), "{:?}", picked.said.borrow());
    let opened: Vec<Opened> = from.try_iter().collect();
    let paths: Vec<PathBuf> = opened.iter().map(|o| o.path.clone()).collect();
    assert_eq!(paths, [db.clone(), inside.clone(), db.clone()]);
    assert_eq!(woken.get(), 3);
    // Each with an editor of its own.
    for o in &opened {
        o.session.send(Input::Size(100, 30));
    }
    screen(&opened[1].session, "HEAD, the database being in the checkout", |b| text(b).contains("▶ HEAD main"));
    for o in [&opened[0], &opened[2]] {
        screen(&o.session, "the patches", |b| row(b, 3).contains("Bug 2 - add b"));
    }
    // One that cannot be opened: said, and no window.
    let picked = Answers {
        databases: vec![tmp.path().to_path_buf()],
        ..Default::default()
    };
    file.open(&picked);
    assert!(from.try_recv().is_err());
    let said = picked.said.borrow();
    assert!(said.len() == 1 && said[0].starts_with("StackED cannot open"), "{said:?}");
    // The panel cancelled: nothing.
    file.open(&Answers::default());
    assert!(from.try_recv().is_err());
}

#[test]
fn file_new_makes_an_empty_database_and_never_replaces_a_file() {
    settings();
    let tmp = tempfile::tempdir().unwrap();
    let (file, from, _) = file_menu(&tmp.path().join("state.json"));
    let new = tmp.path().join("new.jsonl");
    let named = Answers {
        new: Some(new.clone()),
        ..Default::default()
    };
    file.create(&named);
    assert!(named.said.borrow().is_empty(), "{:?}", named.said.borrow());
    let opened = from.try_recv().expect("opened in a window");
    assert_eq!(opened.path, new);
    // A database as stacked starts one, which its commands read.
    let db = stacked::db::Db::load(&new).unwrap();
    assert!(db.warnings.is_empty() && db.stacks().is_empty());
    let out = stacked(&tmp.path().join("home")).args(["-f", new.to_str().unwrap(), "stacks"]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    opened.session.send(Input::Size(100, 30));
    screen(&opened.session, "the new database's screen", |_| true);
    drop(opened);
    // That name again, which the Save panel offers to replace: refused,
    // nothing opened, the file as it was.
    let before = std::fs::read(&new).unwrap();
    file.create(&named);
    assert!(from.try_recv().is_err());
    {
        let said = named.said.borrow();
        assert!(said.len() == 1 && said[0].contains("is there already") && said[0].contains("does not replace a file"), "{said:?}");
    }
    assert_eq!(std::fs::read(&new).unwrap(), before);
    // A file of another kind: the same.
    let notes = tmp.path().join("notes.jsonl");
    std::fs::write(&notes, "mine\n").unwrap();
    let named = Answers {
        new: Some(notes.clone()),
        ..Default::default()
    };
    file.create(&named);
    assert!(from.try_recv().is_err() && named.said.borrow().len() == 1);
    assert_eq!(std::fs::read_to_string(&notes).unwrap(), "mine\n");
    // The panel cancelled: nothing.
    file.create(&Answers::default());
    assert!(from.try_recv().is_err());
}

#[test]
fn open_recent_lists_the_databases_opened_last_and_drops_those_gone() {
    settings();
    let tmp = tempfile::tempdir().unwrap();
    let state = tmp.path().join("state.json");
    let (a, b) = (tmp.path().join("a.jsonl"), tmp.path().join("b.jsonl"));
    for db in [&a, &b] {
        stacked::db::Db::create(db).unwrap();
        stacked::state::opened(&state, db);
    }
    let (file, from, _) = file_menu(&state);
    assert_eq!(file.recent(), [b.clone(), a.clone()]);
    // One deleted: gone from the menu, and from the state kept.
    std::fs::remove_file(&b).unwrap();
    assert_eq!(file.recent(), std::slice::from_ref(&a));
    let kept = std::fs::read_to_string(&state).unwrap();
    assert!(kept.contains("a.jsonl") && !kept.contains("b.jsonl"), "{kept}");
    // Its item, in a menu built before: said to be gone, nothing opened.
    let answers = Answers::default();
    file.open_recent(&b, &answers);
    assert!(from.try_recv().is_err());
    assert!(answers.said.borrow()[0].contains("is no longer there"), "{:?}", answers.said.borrow());
    // Another: opened in a window.
    file.open_recent(&a, &answers);
    assert_eq!(from.try_recv().unwrap().path, a);
    // Clear Menu: none recent.
    file.clear();
    assert!(file.recent().is_empty());
}

#[cfg(target_os = "macos")]
#[test]
fn the_bundle_is_made_as_macos_reads_an_app_and_made_again() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let bundle = |dir: &Path| stacked(&home).args(["app", "--bundle"]).arg(dir).output().unwrap();
    let ok = |cmd: &mut Command| {
        let out = cmd.output().unwrap();
        assert!(out.status.success(), "{:?}: {}", cmd.get_program(), String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    let out = bundle(tmp.path());
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let app = tmp.path().join("StackED.app");
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), app.display().to_string());
    // As macOS reads it: its Info.plist, its signature, its icon at every
    // size, and the stacked in it, which runs.
    let contents = app.join("Contents");
    ok(Command::new("plutil").arg("-lint").arg(contents.join("Info.plist")));
    ok(Command::new("codesign").args(["--verify", "--strict"]).arg(&app));
    let pictures = tmp.path().join("check.iconset");
    let icns = contents.join("Resources/StackED.icns");
    ok(Command::new("iconutil").args(["-c", "iconset"]).arg(&icns).arg("-o").arg(&pictures));
    assert_eq!(std::fs::read_dir(&pictures).unwrap().count(), 10, "16 to 512 points, each twice as dense too");
    let copy = contents.join("MacOS/stacked");
    let version = ok(stacked_at(&copy, &home).arg("--version"));
    assert_eq!(version.trim(), format!("stacked {}", env!("CARGO_PKG_VERSION")));
    // Not by the stacked in it, which it would remove before copying.
    let out = stacked_at(&copy, &home).args(["app", "--bundle"]).arg(tmp.path()).output().unwrap();
    let said = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success() && said.contains("this stacked is the one in"), "{said}");
    assert!(contents.join("MacOS/stacked").exists());
    // Made again: anew.
    std::fs::write(contents.join("Resources/left.txt"), "x").unwrap();
    assert!(bundle(tmp.path()).status.success());
    assert!(!contents.join("Resources/left.txt").exists() && icns.exists());
    // Something else of its name: left alone.
    let other = tmp.path().join("other");
    std::fs::create_dir_all(other.join("StackED.app/Contents")).unwrap();
    std::fs::write(other.join("StackED.app/Contents/Info.plist"), "not ours").unwrap();
    let out = bundle(&other);
    let said = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success() && said.contains("is there, and is not StackED: nothing changed"), "{said}");
    assert_eq!(std::fs::read_to_string(other.join("StackED.app/Contents/Info.plist")).unwrap(), "not ours");
}
