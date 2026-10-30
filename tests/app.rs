//! `stacked app`, its editor run as the window runs it, with no window:
//! what the window sends it, and the screens it sends back; what the File
//! menu opens, its dialogs answered by the tests; and StackED made an app
//! of the system's, StackED.app on macOS, its launcher elsewhere.
#![cfg(feature = "app")]

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
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
    setup_with("Bug 1 - shout two")
}

/// As `setup`, the first patch's message `first`.
fn setup_with(first: &str) -> (tempfile::TempDir, PathBuf) {
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

#[cfg(feature = "moz-phab")]
#[test]
fn a_patch_is_submitted_to_phabricator_from_its_menu_in_the_window_too() {
    let (tmp, db) = setup();
    let arc = r#"{"phabricator.uri": "https://phabricator.example.com/", "repository.callsign": "TEST"}"#;
    std::fs::write(tmp.path().join("src/.arcconfig"), arc).unwrap();
    // A stand-in for Phabricator, which knows the repository alone, and
    // says what was asked of it.
    let asked = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let seen = asked.clone();
    stacked::submit::stand_in(Some(Box::new(move |m, _| {
        seen.lock().unwrap().push(m.to_string());
        match m {
            "diffusion.repository.search" => Ok(serde_json::json!({"data": [{"phid": "PHID-REPO-1", "fields": {"vcs": "git"}}]})),
            _ => Err(format!("{m}: not asked in this test")),
        }
    })));
    let s = Session::open(&db, tmp.path()).unwrap();
    s.send(Input::Size(100, 30));
    screen(&s, "the patches", |b| row(b, 3).contains("Bug 2 - add b"));
    // The first patch's menu, and its item clicked: what is sent is read,
    // then the dialog says it.
    press(&s, KeyCode::Char(' '));
    let item = "Submit to Phabricator as WIP";
    let menu = screen(&s, "the patch's menu", |b| find(b, item).is_some());
    let (x, y) = find(&menu, item).unwrap();
    s.send(Input::Click { x, y, right: false });
    screen(&s, "the dialog", |b| text(b).contains("send patch 1 to Phabricator as a new revision?") && text(b).contains("Patch 1 is sent to Phabricator, at"));
    // Esc: nothing is sent, nothing having been but read.
    press(&s, KeyCode::Esc);
    screen(&s, "the dialog closed", |b| !text(b).contains("send patch 1 to Phabricator"));
    assert_eq!(*asked.lock().unwrap(), ["diffusion.repository.search"]);
    stacked::submit::stand_in(None);
}

#[cfg(feature = "moz-phab")]
#[test]
fn the_status_of_each_patch_s_revision_and_its_phabricator_line_show_in_the_window_too() {
    let (tmp, db) = setup_with("Bug 1 - shout two\n\nDifferential Revision: https://phabricator.example.com/D11");
    // A stand-in for Phabricator, answering the statuses' searches of the
    // editors of this database alone, and what their Phabricator line
    // reads: D11 needs review, its latest diff the first patch's, but `Two`
    // where the patch says `TWO`.
    let d = stacked::db::Db::load(&db).unwrap();
    let raw = d.patch(&d.stack("s").unwrap().patches[0]).diff.replace("+TWO", "+Two");
    stacked::phab::statuses_stand_in(
        &db,
        Some(Box::new(move |m, _| match m {
            "differential.revision.search" => Ok(serde_json::json!({"data": [{"id": 11, "phid": "PHID-DREV-11",
                "fields": {"status": {"value": "needs-review"}, "title": "Bug 1 - shout two", "summary": ""}}]})),
            "differential.diff.search" => Ok(serde_json::json!({"data": [{"id": 5}]})),
            "differential.getrawdiff" => Ok(serde_json::Value::String(raw.clone())),
            "transaction.search" => Ok(serde_json::json!({"data": [], "cursor": {"after": null}})),
            _ => Err(format!("{m}: not asked in this test")),
        })),
    );
    let s = Session::open(&db, tmp.path()).unwrap();
    s.send(Input::Size(100, 30));
    // A letter after the number; the patch naming no revision blank, its
    // title after the column all the same. The Phabricator line over the
    // message, in the middle.
    let line = "Phabricator: D11 · Needs Review · the diff differs · no new comments";
    screen(&s, "the status", |b| row(b, 2).starts_with("  1 [R] Bug 1 - shout two") && row(b, 3).starts_with("  2     Bug 2 - add b"));
    screen(&s, "the Phabricator line", |b| find(b, line).is_some());
    // Listing files, a letter too.
    press(&s, KeyCode::Left);
    screen(&s, "the letter", |b| row(b, 2).starts_with("  1 [R] Bug 1 - shout two"));
    // Space on the line: what differs, on a screen of its own, Phabricator's
    // line of the diff taken out; Esc, back on the line.
    for k in [KeyCode::Right, KeyCode::Up, KeyCode::Char(' ')] {
        press(&s, k);
    }
    screen(&s, "what differs", |b| find(b, "D11 on Phabricator, then patch 1 as it is").is_some() && (0..b.area.height).any(|y| row(b, y).starts_with("-+Two")));
    press(&s, KeyCode::Esc);
    screen(&s, "the line again", |b| find(b, line).is_some());
    // Its menu: what differs, and the reviews of D11 pulled alone, clicked,
    // held for review.
    stacked::phab::stand_in(Some(one_on_d11));
    press(&s, KeyCode::Enter);
    let item = "Pull the reviews of D11 from Phabricator";
    let menu = screen(&s, "the line's menu", |b| find(b, item).is_some());
    assert!(find(&menu, "What differs from D11 on Phabricator…").is_some(), "{}", text(&menu));
    let (x, y) = find(&menu, item).unwrap();
    s.send(Input::Click { x, y, right: false });
    screen(&s, "what the pull brought", |b| find(b, "1 new comment from D11: it waits for your review").is_some());
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

#[test]
fn a_message_s_long_lines_are_wrapped_from_its_patch_s_menu_in_the_window_too() {
    let long = "Bug 1 - shout two\n\nThe parser read the whole file before it checked the header, which made it slow on the big files we have.";
    let (tmp, db) = setup_with(long);
    let message = |db: &Path| {
        let d = stacked::db::Db::load(db).unwrap();
        d.patch(&d.stack("s").unwrap().patches[0]).message.clone()
    };
    let before = message(&db);
    assert!(before.contains("which made it slow"), "{before}");
    let s = Session::open(&db, tmp.path()).unwrap();
    s.send(Input::Size(100, 30));
    screen(&s, "the patches", |b| row(b, 3).contains("Bug 2 - add b"));
    // The first patch's menu offers it; its item clicked, the edit lands
    // for review, its review open at once.
    press(&s, KeyCode::Char(' '));
    let item = "Wrap the long lines of this patch's message at 72 columns";
    let menu = screen(&s, "the patch's menu", |b| find(b, item).is_some());
    let (x, y) = find(&menu, item).unwrap();
    s.send(Input::Click { x, y, right: false });
    screen(&s, "the edit landed", |b| text(b).contains("at 72 columns: it waits for your review"));
    // Nothing changes in the stack before the review accepts it.
    assert_eq!(message(&db), before);
    let d = stacked::db::Db::load(&db).unwrap();
    let queue = stacked::held::queue(&d, "s");
    assert!(queue.len() == 1 && stacked::held::by_hand(queue[0]), "{queue:?}");
}

#[test]
fn move_head_says_why_it_does_not_move_head_in_the_window_too() {
    let (tmp, db) = setup();
    let src = tmp.path().join("src");
    // A commit of yours on top: Move HEAD would not move HEAD.
    std::fs::write(src.join("c.txt"), "c\n").unwrap();
    git(&src, &["add", "c.txt"]);
    git(&src, &["commit", "-qm", "Mine"]);
    let s = Session::open(&db, &src).unwrap();
    s.send(Input::Size(100, 30));
    screen(&s, "HEAD forked", |b| text(b).contains("└○ Mine"));
    // The stack's menu, from its header: Move HEAD… drawn as any item, not
    // dim; clicked, the view of why it does not move HEAD.
    press(&s, KeyCode::Up);
    press(&s, KeyCode::Char(' '));
    let item = "Move HEAD…";
    let menu = screen(&s, "the stack's menu", |b| find(b, item).is_some());
    let (x, y) = find(&menu, item).unwrap();
    assert!(!menu[(x, y)].modifier.contains(Modifier::DIM), "{}", text(&menu));
    s.send(Input::Click { x, y, right: false });
    screen(&s, "the view", |b| find(b, " why Move HEAD does not move HEAD").is_some() && text(b).contains("Move HEAD does not move HEAD from there"));
    // ↓: the commit, then what to do, the command on a line of its own.
    press(&s, KeyCode::Down);
    screen(&s, "the commit", |b| text(b).contains("only in your checkout") && text(b).contains("      git switch --detach "));
    // Esc: back to the patches.
    press(&s, KeyCode::Esc);
    screen(&s, "the patches again", |b| text(b).contains("└○ Mine") && find(b, " why Move HEAD does not move HEAD").is_none());
}

#[test]
fn the_stack_s_lint_is_offered_on_its_menu_in_the_window_too() {
    let (tmp, db) = setup();
    let src = tmp.path().join("src");
    std::fs::create_dir_all(src.join(".stacked")).unwrap();
    std::fs::write(src.join(".stacked/local.toml"), "[lint]\ncommand = \"./lint.sh --fix\"\n").unwrap();
    let s = Session::open(&db, tmp.path()).unwrap();
    s.send(Input::Size(100, 30));
    screen(&s, "the patches", |b| row(b, 3).contains("Bug 2 - add b"));
    // The stack's menu, from its header, names the linter of the stack's
    // checkout. Nothing is linted here: the item is not chosen.
    press(&s, KeyCode::Up);
    press(&s, KeyCode::Char(' '));
    screen(&s, "the stack's menu", |b| find(b, "Lint the stack (./lint.sh --fix)").is_some());
}

#[test]
fn the_ask_dialog_opens_with_nothing_set_is_clicked_and_asks_later_in_the_window_too() {
    let (tmp, db) = setup();
    // A comment on the stack, for its menu to ask about.
    let (home, file) = (tmp.path().join("home"), db.to_str().unwrap());
    let out = stacked(&home).current_dir(tmp.path()).args(["-f", file, "comment", "s", "Why?"]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    // The editors of this process run as claude a program no folder of the
    // PATH holds: Ask Now grayed, saying so, no network checked, and no
    // agent ever asked.
    let none = "stacked-no-such-claude";
    stacked::ask::stand_in(Path::new(none), Path::new(env!("CARGO_BIN_EXE_stacked")));
    let s = Session::open(&db, tmp.path()).unwrap();
    s.send(Input::Size(120, 30));
    screen(&s, "the patches", |b| text(b).contains("Bug 2 - add b"));
    press(&s, KeyCode::Home);
    screen(&s, "the stack in the middle", |b| text(b).contains("the stack s · 2 patches"));
    press(&s, KeyCode::Char(' '));
    let click = |(x, y): (u16, u16)| s.send(Input::Click { x, y, right: false });
    // Ask agent about all…, clicked: its dialog, nothing set, its buttons
    // under its lines, Ask Now and Ask Later dim, and why they are.
    let menu = screen(&s, "the stack's menu", |b| find(b, "Ask agent about all…").is_some());
    click(find(&menu, "Ask agent about all…").unwrap());
    let dialog = screen(&s, "the dialog", |b| find(b, "Permission: [ — ]").is_some());
    for said in ["ask the agent about the comments listed on the stack", "Effort: [ — ]", "< Ask Now >   < Ask Later >   < Cancel >"] {
        assert!(find(&dialog, said).is_some(), "{said:?} in:\n{}", text(&dialog));
    }
    assert!(find(&dialog, &format!("Ask Now is grayed: {none} is not found")).is_some(), "{}", text(&dialog));
    for button in ["< Ask Now >", "< Ask Later >"] {
        let (x, y) = find(&dialog, button).unwrap();
        assert!(dialog[(x, y)].style().add_modifier.contains(Modifier::DIM), "{button} dim");
    }
    let (x, y) = find(&dialog, "< Cancel >").unwrap();
    assert!(!dialog[(x, y)].style().add_modifier.contains(Modifier::DIM), "Cancel can be pressed");
    // A click on the permission's line steps it; Enter on the effort's.
    click(find(&dialog, "Permission: [ — ]").unwrap());
    screen(&s, "answer only", |b| find(b, "Permission: [answer only]").is_some());
    press(&s, KeyCode::Down);
    press(&s, KeyCode::Enter);
    let set = screen(&s, "low", |b| find(b, "Effort: [low]").is_some());
    let (x, y) = find(&set, "< Ask Later >").unwrap();
    assert!(!set[(x, y)].style().add_modifier.contains(Modifier::DIM), "Ask Later can be pressed");
    // Ask Later, clicked: in the batch, at what the dialog set.
    click((x + 2, y));
    screen(&s, "batched", |b| text(b).contains("batched (answer only): 1 ask in the batch"));
    let d = stacked::db::Db::load(&db).unwrap();
    let q = d.stack_notes("s").into_iter().find_map(|n| n.queued.clone()).expect("batched");
    assert_eq!((q.batched(), q.way, q.effort), (true, stacked::db::Way::Answer, Some(stacked::db::Effort::Low)));
}

/// The links a stand-in for the program that opens web links, in `dir`,
/// writes once it was given `n` of them, or the wait given up: the editor
/// of this process opens them with it from now on. It opens nothing: no test
/// opens a browser.
fn opened_by_stand_in(dir: &Path) -> impl Fn(usize) -> Vec<String> {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join("open.sh");
    std::fs::write(&path, "#!/bin/sh\nprintf '%s\\n' \"$1\" >> \"$(dirname \"$0\")/opened.txt\"\n").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    stacked::refs::open_links_with(&path);
    let log = dir.join("opened.txt");
    move |n| {
        let started = Instant::now();
        loop {
            let all: Vec<String> = std::fs::read_to_string(&log).unwrap_or_default().lines().map(String::from).collect();
            if all.len() >= n || started.elapsed() > Duration::from_secs(20) {
                return all;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

#[test]
fn a_link_of_a_line_of_a_message_or_of_a_comment_opens_from_its_menu_and_one_of_a_message_with_o_in_the_window_too() {
    let (tmp, db) = setup();
    let here = tmp.path().file_name().unwrap().to_string_lossy().to_string();
    let why = format!("https://example.com/{here}/why");
    let docs = format!("https://example.com/{here}/docs");
    // Patch 1's message says where to read why; your comment on its title,
    // where to read the docs.
    stacked::db::Db::edit(&db, |d| {
        let old = d.patch(&d.stack("s")?.patches[0]).clone();
        let draft = stacked::mbox::Draft {
            message: format!("Bug 1 - shout two\n\nSee {why}"),
            ..stacked::mbox::Draft::from(&old)
        };
        stacked::ops::replace(d, "s", 0, draft, "reword").map(|_| ())
    })
    .unwrap();
    stacked::db::Db::edit(&db, |d| {
        let p = d.patch(&d.stack("s")?.patches[0]).clone();
        d.set_note(stacked::db::Note {
            change: p.change.clone(),
            id: "on-title".into(),
            line: stacked::view::title_key(&p),
            file: stacked::view::MSG_PATH.into(),
            preview: p.title().into(),
            text: format!("Read [the docs]({docs}) first."),
            ..Default::default()
        })
    })
    .unwrap();
    let opened = opened_by_stand_in(tmp.path());
    let s = Session::open(&db, tmp.path()).unwrap();
    s.send(Input::Size(100, 30));
    // A right click on the line of the message opens its menu, and a click
    // on its item opens the link, the status line saying so.
    let buf = screen(&s, "the line", |b| find(b, "See https://").is_some());
    let (x, y) = find(&buf, "See https://").unwrap();
    s.send(Input::Click { x, y, right: true });
    let item = format!("Open {why} in your browser");
    let menu = screen(&s, "the line's menu", |b| find(b, &item).is_some());
    let (x, y) = find(&menu, &item).unwrap();
    s.send(Input::Click { x, y, right: false });
    screen(&s, "the link opened", |b| text(b).contains(&format!("opened {why} in your browser")));
    assert_eq!(opened(1), std::slice::from_ref(&why));
    // o, the cursor on the line, opens it again.
    press(&s, KeyCode::Char('o'));
    assert_eq!(opened(2), [why.clone(), why.clone()]);
    // A right click on the line of the comment: its menu has the link,
    // named by its text, which opens it.
    let comment = "▎   Read the docs";
    let buf = screen(&s, "the comment's line", |b| find(b, comment).is_some());
    let (x, y) = find(&buf, comment).unwrap();
    s.send(Input::Click { x, y, right: true });
    let item = "Open \"the docs\" in your browser";
    let menu = screen(&s, "the comment's menu", |b| find(b, item).is_some());
    let (x, y) = find(&menu, item).unwrap();
    s.send(Input::Click { x, y, right: false });
    screen(&s, "the comment's link opened", |b| text(b).contains(&format!("opened {docs} in your browser")));
    assert_eq!(opened(3), [why.clone(), why, docs]);
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

/// How many times the File menu woke the window set, from the thread its
/// dialogs are asked on.
struct Woken(Arc<AtomicUsize>);

impl Woken {
    fn get(&self) -> usize {
        self.0.load(Ordering::Relaxed)
    }
}

/// The File menu, its recent databases kept in `state`: what it opens, and
/// how many times it woke the window set.
fn file_menu(state: &Path) -> (File, Receiver<Opened>, Woken) {
    let (to, from) = mpsc::channel();
    let woken = Arc::new(AtomicUsize::new(0));
    let woke = woken.clone();
    let wake = move || {
        woke.fetch_add(1, Ordering::Relaxed);
    };
    (File::new(to, Box::new(wake), state.to_path_buf()), from, Woken(woken))
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

#[cfg(not(target_os = "macos"))]
#[test]
fn the_launcher_is_made_where_the_desktop_reads_it_and_made_again() {
    let tmp = tempfile::tempdir().unwrap();
    // The home and the data folder of the test's: never the real ones.
    let (home, data) = (tmp.path().join("home"), tmp.path().join("data"));
    let bundle = |data: Option<&Path>, dir: Option<&Path>| {
        let mut cmd = stacked(&home);
        cmd.env("HOME", &home).args(["app", "--bundle"]);
        match data {
            Some(data) => cmd.env("XDG_DATA_HOME", data),
            None => cmd.env_remove("XDG_DATA_HOME"),
        };
        cmd.args(dir).output().unwrap()
    };
    let entry = |data: &Path| data.join("applications/io.github.carverdamien.stacked.desktop");
    // In $XDG_DATA_HOME: the launcher in its applications, said on stdout,
    // and the icon in its icons, at each size of the theme's.
    let out = bundle(Some(&data), None);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), entry(&data).display().to_string());
    let said = String::from_utf8_lossy(&out.stderr);
    let icons = data.join("icons/hicolor");
    assert!(said.contains("StackED's launcher is made") && said.contains(&icons.display().to_string()), "{said}");
    // What the desktop reads: StackED, its icon, this stacked run as the
    // launcher runs it, with no terminal.
    let exe = std::fs::canonicalize(env!("CARGO_BIN_EXE_stacked")).unwrap();
    let text = std::fs::read_to_string(entry(&data)).unwrap();
    for line in [
        "[Desktop Entry]".to_string(),
        "Type=Application".into(),
        "Name=StackED".into(),
        format!("Exec=\"{}\" app --launched", exe.display()),
        "Icon=io.github.carverdamien.stacked".into(),
        "Terminal=false".into(),
        "Categories=Development;RevisionControl;".into(),
    ] {
        assert!(text.lines().any(|l| l == line), "{line}\n{text}");
    }
    for size in [16, 24, 32, 48, 64, 128, 256, 512] {
        let png = std::fs::read(icons.join(format!("{size}x{size}/apps/io.github.carverdamien.stacked.png"))).unwrap();
        assert!(png.starts_with(b"\x89PNG") && u32::from_be_bytes(png[16..20].try_into().unwrap()) == size, "{size}");
    }
    // Without $XDG_DATA_HOME: in ~/.local/share. In DIR, if named: there.
    assert!(bundle(None, None).status.success());
    assert!(entry(&home.join(".local/share")).is_file());
    let dir = tmp.path().join("elsewhere");
    assert!(bundle(Some(&data), Some(&dir)).status.success());
    assert!(entry(&dir).is_file() && dir.join("icons/hicolor/512x512/apps/io.github.carverdamien.stacked.png").is_file());
    // Made again: anew.
    std::fs::write(entry(&data), text.replace("Name=StackED", "Name=Old")).unwrap();
    assert!(bundle(Some(&data), None).status.success());
    assert_eq!(std::fs::read_to_string(entry(&data)).unwrap(), text);
    // Something else of its name: left alone.
    std::fs::write(entry(&data), "[Desktop Entry]\nExec=other\n").unwrap();
    let out = bundle(Some(&data), None);
    let said = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success() && said.contains("is there, and is not StackED's launcher: nothing changed"), "{said}");
    assert_eq!(std::fs::read_to_string(entry(&data)).unwrap(), "[Desktop Entry]\nExec=other\n");
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
fn the_tools_the_agent_could_not_use_are_reviewed_in_the_window_too() {
    let (tmp, db) = setup();
    refused_work(&db);
    let s = Session::open(&db, tmp.path()).unwrap();
    s.send(Input::Size(100, 30));
    screen(&s, "the banner", |b| text(b).contains("1 tool the agent could not use"));
    // From the banner, its review: the section, the item refused, what the
    // agent said of it beside them.
    press(&s, KeyCode::Esc);
    press(&s, KeyCode::Enter);
    screen(&s, "the review", |b| {
        let t = text(b);
        t.contains(" tools the agent could not use") && t.contains("✗ Bash(searchfox-cli *) · refused") && t.contains("To find Foo; I used rg instead.")
    });
    // Enter: allowed from now on.
    press(&s, KeyCode::Enter);
    screen(&s, "allowed", |b| text(b).contains("✓ Bash(searchfox-cli *) · from now on"));
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
fn a_reviewer_s_edit_is_headed_so_and_a_pull_s_review_rejects_nothing_in_the_window_too() {
    let (tmp, db) = setup();
    pulled_work(&db);
    let s = Session::open(&db, tmp.path()).unwrap();
    s.send(Input::Size(120, 30));
    // ann's comment as it was, redacted; her edit, by what it edits.
    screen(&s, "her edit", |b| {
        let t = text(b);
        t.contains("▎ ann (redacted):") && t.contains("▎ ann's edit of the comment that said \"Hi Da\":")
    });
    // The pull's review: its part accepted; Delete rejects nothing, and says
    // why; closing it, nothing to reject either.
    press(&s, KeyCode::Esc);
    press(&s, KeyCode::Enter);
    screen(&s, "the part", |b| text(b).contains("✓ ✉ message: Bug 2 - add b · Why add b?"));
    press(&s, KeyCode::Delete);
    screen(&s, "why", |b| text(b).contains("a comment pulled from Phabricator cannot be rejected: close the review to file it"));
    press(&s, KeyCode::Esc);
    let dialog = screen(&s, "the dialog", |b| text(b).contains("Keep reviewing"));
    assert!(text(&dialog).contains("Close review") && !text(&dialog).contains("Reject all remaining"), "{}", text(&dialog));
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
fn the_effort_the_agent_was_asked_at_heads_its_turns_and_names_its_work_in_the_window_too() {
    let (tmp, db) = setup();
    at_max(&db);
    let s = Session::open(&db, tmp.path()).unwrap();
    s.send(Input::Size(120, 30));
    // The thread: the agent's turn at max headed with it, the one of before
    // as ever; the comments pane saying it before what the agent said last;
    // the banner, what landed at max.
    screen(&s, "the turns", |b| {
        let t = text(b);
        ["▎ agent(max):", "▎ agent:", "agent(max): A shout.", "· the next: 1 reply, asked at max: \"Why add b?\""].iter().all(|said| t.contains(said))
    });
    // Its review, from the banner: its title, and its reply headed so.
    press(&s, KeyCode::Esc);
    press(&s, KeyCode::Enter);
    screen(&s, "the review", |b| {
        let t = text(b);
        [" the agent's work (max): 1 reply", "▎ agent(max):", "▎   For the bees."].iter().all(|said| t.contains(said))
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

/// The box of the comment being written in `buf`, if there is one: the
/// lines inside it, and the row of its bottom edge.
fn editor_box(buf: &Buffer) -> Option<(Vec<String>, u16)> {
    let (left, top) = find(buf, "┌─ a comment on")?;
    let right = (left..buf.area.width).find(|&x| buf[(x, top)].symbol() == "┐")?;
    let bottom = (top + 1..buf.area.height).find(|&y| buf[(left, y)].symbol() == "└")?;
    let inside = (top + 1..bottom).map(|y| (left + 1..right).map(|x| buf[(x, y)].symbol()).collect::<String>().trim_end().to_string());
    Some((inside.collect(), bottom))
}

#[test]
fn what_was_closed_here_scrolls_to_its_end_with_the_arrows_and_the_wheel_in_the_window_too() {
    let (tmp, db) = setup();
    closed_on_two(tmp.path(), &db);
    let s = Session::open(&db, tmp.path()).unwrap();
    s.send(Input::Size(80, 16));
    // The box's first line, as the screen shows it.
    let first = |at: &'static str| move |b: &Buffer| editor_box(b).is_some_and(|(inside, _)| inside.first().is_some_and(|l| l == at));
    // The mouse over the line picks it, and a click on it is Enter: the box
    // opens on what was closed there, from its first line, ↑↓ said on the
    // bottom bar.
    let buf = screen(&s, "the line", |b| find(b, "+TWO").is_some());
    let (x, y) = find(&buf, "+TWO").unwrap();
    s.send(Input::Hover(x, y));
    s.send(Input::Click { x, y, right: false });
    let buf = screen(&s, "what was closed there", first("you: why so loud?"));
    assert!(row(&buf, 15).contains("↑↓: read the last comment closed here"), "{}", text(&buf));
    // ↓ scrolls it a line, and so do the wheel and the trackpad, which the
    // window sends as ↓, a line for each notch; pgdn a box's worth.
    for _ in 0..3 {
        press(&s, KeyCode::Down);
    }
    screen(&s, "three lines down", first("  reason 3"));
    press(&s, KeyCode::PageDown);
    screen(&s, "a box's worth further", first("  reason 8"));
    // To its end and no further: its last line in sight, the box's bottom
    // edge above the bottom bar.
    for _ in 0..40 {
        press(&s, KeyCode::Down);
    }
    let buf = screen(&s, "its end", first("  reason 26"));
    let (inside, bottom) = editor_box(&buf).unwrap();
    assert!(inside.last().is_some_and(|l| l == "  reason 30") && bottom < 15, "{}", text(&buf));
    press(&s, KeyCode::Up);
    screen(&s, "a line back up", first("  reason 25"));
}

#[test]
fn what_waits_its_turn_is_taken_out_of_the_queue_from_the_banner_once_asked_in_the_window_too() {
    let (tmp, db) = setup();
    waiting_their_turn(&db);
    let s = Session::open(&db, tmp.path()).unwrap();
    s.send(Input::Size(140, 30));
    screen(&s, "the banner", |b| text(b).contains("Reconcile with HEAD waits its turn"));
    // On the banner, its first line the reconcile's: Delete asks first, and
    // Drop it, over Keep it, drops it.
    press(&s, KeyCode::Esc);
    screen(&s, "the reconcile's line", |b| text(b).contains("◌ Reconcile with HEAD, waiting its turn: the commit messages of patch 1 taken from HEAD"));
    press(&s, KeyCode::Delete);
    screen(&s, "the reconcile's drop asked", |b| text(b).contains("drop the reconcile with HEAD that waits its turn? Nothing was changed yet"));
    press(&s, KeyCode::Up);
    press(&s, KeyCode::Enter);
    let dropped = "the waiting reconcile with HEAD was dropped: nothing was taken from HEAD, and the stack is as it was";
    screen(&s, "the reconcile dropped", |b| text(b).contains(dropped));
    // Then the lint job's line.
    press(&s, KeyCode::Delete);
    screen(&s, "the lint job's drop asked", |b| text(b).contains("drop the lint job that waits its turn? Nothing was linted yet"));
    press(&s, KeyCode::Up);
    press(&s, KeyCode::Enter);
    screen(&s, "the lint job dropped", |b| text(b).contains("the waiting lint job was dropped: nothing was linted, and the stack is as it was"));
    // Then the comment's: its ask taken back, the comment open.
    press(&s, KeyCode::Delete);
    screen(&s, "the ask's take-back asked", |b| text(b).contains("take the edit ask on patch 1's comment out of the queue? The comment stays open"));
    press(&s, KeyCode::Up);
    press(&s, KeyCode::Enter);
    let taken = "the edit ask on patch 1's comment is taken out of the queue: the comment stays open, and is not sent";
    screen(&s, "the ask taken back", |b| text(b).contains(taken));
    let d = stacked::db::Db::load(&db).unwrap();
    assert!(stacked::reconcile::pending(&d, "s").is_none() && stacked::lint::pending(&d, "s").is_none());
    let n = d.note(&d.patch(&d.stack("s").unwrap().patches[0]).change, "c-loud").unwrap();
    assert!(n.queued.is_none() && n.text == "Shout less.", "{n:?}");
    // The agent's change over, so that leaving says nothing of it.
    over_now(&db, "run-changing");
}

/// Run `id` of `db` landed, as failed, nothing brought back.
fn over_now(db: &Path, id: &str) {
    stacked::db::Db::edit(db, |d| {
        let r = d.run(id).cloned().unwrap();
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
fn right_opens_the_comments_pane_of_a_patch_with_none_saying_why_in_the_window_too() {
    let (tmp, db) = setup();
    let s = Session::open(&db, tmp.path()).unwrap();
    s.send(Input::Size(140, 30));
    screen(&s, "the patches", |b| row(b, 3).contains("Bug 2 - add b"));
    // Into the middle, then the comments pane, though the patch has none:
    // its header, and why it lists nothing; Enter there, its menu, with the
    // filter.
    press(&s, KeyCode::Right);
    press(&s, KeyCode::Right);
    screen(&s, "the pane", |b| row(b, 1).contains("│ all · nothing listed") && text(b).contains("This patch has no comments."));
    press(&s, KeyCode::Enter);
    screen(&s, "its menu", |b| find(b, "Filter comments…").is_some());
    // ← out of it, once the menu is closed: the pane goes.
    press(&s, KeyCode::Esc);
    press(&s, KeyCode::Left);
    screen(&s, "the pane gone", |b| !text(b).contains("nothing listed") && find(b, "Filter comments…").is_none());
}
