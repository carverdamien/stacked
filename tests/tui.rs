use std::path::{Path, PathBuf};
use std::process::Command;

use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;

use stacked::db::{Db, Note, stack_change};
use stacked::thread::{Role, append_turn};
use stacked::tui::{App, Effect, Focus, Layout, Mode, Screen, Show};

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

fn stacked(dir: &Path, db: &Path, args: &[&str]) {
    let out = Command::new(env!("CARGO_BIN_EXE_stacked"))
        .current_dir(dir)
        .arg("-f")
        .arg(db)
        .args(args)
        .env("STACKED_CONFIG", "/nonexistent/stacked.toml")
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
}

/// A database holding stack `s` of two patches, and another stack `t`; and
/// the repository.
fn setup() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir(&src).unwrap();
    git(&src, &["init", "-q", "-b", "main"]);
    git(&src, &["config", "commit.gpgsign", "false"]);
    git(&src, &["config", "tag.gpgsign", "false"]);
    let lines: String = (1..=40).map(|n| format!("line {n}\n")).collect();
    std::fs::write(src.join("a.txt"), format!("one\ntwo\nthree\n{lines}")).unwrap();
    git(&src, &["add", "."]);
    git(&src, &["commit", "-qm", "base"]);
    std::fs::write(src.join("a.txt"), format!("one\nTWO\nthree\n{}", lines.replace("line 39\n", "LINE 39\n"))).unwrap();
    git(&src, &["commit", "-qam", "Bug 1 - shout two\n\nBecause."]);
    std::fs::write(src.join("b.txt"), "bee\n").unwrap();
    git(&src, &["add", "."]);
    git(&src, &["commit", "-qm", "Bug 2 - add b"]);
    let db = tmp.path().join("db.jsonl");
    stacked(&src, &db, &["import", "t", "HEAD~1"]);
    stacked(&src, &db, &["import", "s", "HEAD~2"]);
    (tmp, db, src)
}

/// The editor on `db`, with the settings of no file: the tests share their
/// process, and a developer's own settings must not change them.
fn open(db: &Path) -> stacked::db::Result<App> {
    stacked::config::init(stacked::config::Config::default());
    App::with_config(db, stacked::config::Config::default())
}

/// Draws the screen; what it shows, and where the terminal's cursor is.
fn draw_at(app: &mut App) -> (String, Option<(u16, u16)>) {
    let mut buf = Buffer::empty(Rect::new(0, 0, 120, 30));
    let at = app.draw(&mut buf);
    let text = (0..buf.area.height)
        .map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");
    (text, at)
}

fn draw(app: &mut App) -> String {
    draw_at(app).0
}

fn key(app: &mut App, code: KeyCode) -> Effect {
    app.key(KeyEvent::new(code, KeyModifiers::NONE))
}

fn keys(app: &mut App, codes: &[KeyCode]) {
    for c in codes {
        assert_eq!(key(app, *c), Effect::None, "{c:?}");
        draw(app);
    }
}

fn typed(app: &mut App, text: &str) {
    for c in text.chars() {
        key(app, KeyCode::Char(c));
    }
}

/// The menu's items: label, and whether it can be chosen.
fn items(app: &App) -> Vec<(String, bool)> {
    match &app.mode {
        Mode::Menu(m) => m.items.iter().map(|i| (i.label.clone(), i.enabled)).collect(),
        _ => vec![],
    }
}

/// Opens the menu (space) unless one is open, and does the item whose label
/// starts with `label`, going there with ↓ and doing it with enter.
fn choose(app: &mut App, label: &str) -> Effect {
    if !matches!(app.mode, Mode::Menu(_)) {
        assert_eq!(key(app, KeyCode::Char(' ')), Effect::None);
    }
    let all = items(app);
    let want = all
        .iter()
        .position(|(l, _)| l.starts_with(label))
        .unwrap_or_else(|| panic!("no {label:?} in {all:?}"));
    assert!(all[want].1, "{label:?} cannot be chosen: {all:?}");
    for _ in 0..all.len() * 2 {
        match &app.mode {
            Mode::Menu(m) if m.sel == want => break,
            _ => {
                key(app, KeyCode::Down);
            }
        }
    }
    let e = key(app, KeyCode::Enter);
    draw(app);
    e
}

/// Writes `text` in the editor open, and saves it.
fn save(app: &mut App, text: &str) {
    assert!(matches!(app.mode, Mode::Edit(_)), "{:?}", app.mode);
    typed(app, text);
    keys(app, &[KeyCode::Esc]);
    choose(app, "Save it");
}

/// Down the patch until the cursor is on the row starting with `text`.
fn down_to(app: &mut App, text: &str) {
    draw(app);
    for _ in 0..app.rows.len() {
        if app.rows[app.cursor].text.starts_with(text) {
            return;
        }
        key(app, KeyCode::Down);
        draw(app);
    }
    panic!("no row {text:?}");
}

/// Draws until the screen shows `what`, the file read again meanwhile.
fn wait_for(app: &mut App, what: &str) -> String {
    // Patient: the asks of every test run at once, each its worktree to
    // make.
    for _ in 0..300 {
        app.refresh();
        let screen = draw(app);
        if screen.contains(what) {
            return screen;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    panic!("never shown: {what:?}\n{}", draw(app));
}

/// What History…'s first pane, the history, shows: two fifths of the
/// screen's 120 columns.
fn history_pane(screen: &str) -> String {
    screen.lines().map(|l| l.chars().take(48).collect::<String>()).collect::<Vec<_>>().join("\n")
}

/// The comments of stack `name`'s patch `k`.
fn notes_of(db: &Path, name: &str, k: usize) -> Vec<Note> {
    let d = Db::load(db).unwrap();
    let s = d.stack(name).unwrap();
    d.notes(&d.patch(&s.patches[k]).change).into_iter().cloned().collect()
}

/// Where `text` is on a screen drawn: its first cell.
fn at(screen: &str, text: &str) -> (u16, u16) {
    at_from(screen, text, 0)
}

/// Where `text` is on a screen drawn, from column `from` on.
fn at_from(screen: &str, text: &str, from: usize) -> (u16, u16) {
    for (y, line) in screen.lines().enumerate() {
        let rest: String = line.chars().skip(from).collect();
        if let Some(b) = rest.find(text) {
            return ((from + rest[..b].chars().count()) as u16, y as u16);
        }
    }
    panic!("no {text:?} on the screen from column {from}:\n{screen}");
}

#[test]
fn the_cursor_follows_the_mouse() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    // Over the second patch: picked, its patch in the middle; a click on it
    // is Enter: read, the cursor in the middle.
    let (x, y) = at(&draw(&mut app), "Bug 2 - add b");
    app.hover(x, y);
    assert_eq!((app.focus, app.on_stack, app.sel), (Focus::List, false, 1));
    assert!(draw(&mut app).contains("+bee"));
    app.click(x, y, false);
    assert_eq!(app.focus, Focus::Diff, "one click");
    // Over a line of it: the cursor there.
    let (x, y) = at(&draw(&mut app), "+bee");
    app.hover(x, y);
    assert!(app.rows[app.cursor].text.starts_with("+bee"));
    // In a menu, over an item: the one Enter does.
    keys(&mut app, &[KeyCode::Char(' ')]);
    let (x, y) = at(&draw(&mut app), "Comment in $EDITOR");
    app.hover(x, y);
    match &app.mode {
        Mode::Menu(m) => assert_eq!(m.items[m.sel].label, "Comment in $EDITOR"),
        other => panic!("{other:?}"),
    }
    // Writing, the mouse moves nothing.
    keys(&mut app, &[KeyCode::Esc, KeyCode::Enter]);
    assert!(matches!(app.mode, Mode::Edit(_)));
    let (cursor, screen) = (app.cursor, draw(&mut app));
    let (x, y) = at(&screen, "Bug 1 - shout two");
    app.hover(x, y);
    assert!(matches!(app.mode, Mode::Edit(_)) && app.cursor == cursor && app.focus == Focus::Diff);
}

#[test]
fn a_click_picks_and_a_second_does_as_enter() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    // A patch clicked is picked: it shows in the middle.
    let (x, y) = at(&draw(&mut app), "Bug 2 - add b");
    assert_eq!(app.click(x, y, false), Effect::None);
    assert_eq!((app.focus, app.on_stack, app.sel), (Focus::List, false, 1));
    assert!(draw(&mut app).contains("+bee"));
    // Clicked again, as Enter: read, the cursor in the middle.
    app.click(x, y, false);
    assert_eq!(app.focus, Focus::Diff);
    // A line right-clicked: picked, and its menu, as Space.
    let (x, y) = at(&draw(&mut app), "+bee");
    app.click(x, y, true);
    assert!(app.rows[app.cursor].text.starts_with("+bee"));
    assert!(matches!(app.mode, Mode::Menu(_)), "{:?}", app.mode);
    // In a menu, a click does the item; in the box but on none, nothing.
    let screen = draw(&mut app);
    let (x, y) = at(&screen, "Comment on this line…");
    app.click(x, y - 1, false);
    assert!(matches!(app.mode, Mode::Menu(_)), "the box's edge");
    app.click(x, y, false);
    assert!(matches!(app.mode, Mode::Edit(_)), "{:?}", app.mode);
    // Writing, a click does nothing.
    draw(&mut app);
    app.click(1, 2, false);
    assert!(matches!(app.mode, Mode::Edit(_)));
    save(&mut app, "a bee");
    // Outside a menu, a click closes it.
    keys(&mut app, &[KeyCode::Char(' ')]);
    draw(&mut app);
    app.click(0, 29, false);
    assert_eq!(app.mode, Mode::Normal);
    // The comment, in the comments pane (from column 90): picked, then
    // gone to.
    let (x, y) = at_from(&draw(&mut app), "a bee", 90);
    app.click(x, y, false);
    assert_eq!((app.focus, app.note_sel), (Focus::Notes, 0));
    app.click(x, y, false);
    assert_eq!(app.focus, Focus::Diff, "gone to");
    // Lists too: History…'s second record (the import, a move after it),
    // picked, then gone to, in the tree.
    Db::edit(&db, |d| stacked::ops::move_patch(d, "s", 1, 0).map(|_| ())).unwrap();
    app.refresh();
    keys(&mut app, &[KeyCode::Left, KeyCode::Home]);
    choose(&mut app, "History…");
    let screen = draw(&mut app);
    let (x, y) = at(&screen, "import");
    app.click(x, y, false);
    assert_eq!(app.pick, 1);
    app.click(x, y, false);
    assert_eq!(app.screen, Screen::Tree);
}

#[test]
fn titles_on_the_left_the_patch_in_the_middle() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    assert_eq!(app.stack, "s", "the stack edited last");
    let screen = draw(&mut app);
    assert!(screen.contains(" ≡ s · 2 patches"), "the stack's own place, at the top:\n{screen}");
    assert!(screen.contains("  1 Bug 1 - shout two"), "{screen}");
    assert!(screen.contains("  2 Bug 2 - add b"), "{screen}");
    assert!(screen.contains("│✉ message"), "{screen}");
    assert!(screen.contains("-two") && screen.contains("+TWO"), "{screen}");
    assert!(screen.contains("s @") && screen.contains("2 patches on "), "{screen}");
    assert!(screen.contains("enter: read it · space: menu · 1: view · 2: read · ?: what can I do here"), "{screen}");
    keys(&mut app, &[KeyCode::Down]);
    let screen = draw(&mut app);
    assert!(screen.contains("+bee") && !screen.contains("+TWO"), "{screen}");
    // Up from the first patch: the stack itself, in the middle.
    keys(&mut app, &[KeyCode::Up, KeyCode::Up]);
    assert!(app.on_stack);
    let screen = draw(&mut app);
    assert!(screen.contains("the stack s · 2 patches on") && screen.contains("no comments on the stack itself"), "{screen}");
}

#[test]
fn one_cycles_the_views_of_the_code() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    draw(&mut app);
    keys(&mut app, &[KeyCode::Enter]);
    assert_eq!(app.focus, Focus::Diff);
    // Side by side: the old line on the left, the new on the right.
    keys(&mut app, &[KeyCode::Char('1')]);
    assert_eq!(app.layout, Layout::Split);
    let screen = draw(&mut app);
    let line = screen.lines().find(|l| l.contains("-two")).unwrap();
    assert!(line.contains("+TWO"), "{line}");
    assert!(screen.contains("· side-by-side"), "{screen}");
    // Compact: each side a pane of its own, scrolled together, or one alone.
    keys(&mut app, &[KeyCode::Char('1')]);
    assert_eq!(app.layout, Layout::Compact);
    let screen = draw(&mut app);
    assert!(screen.contains("side-by-side, compact <>"), "{screen}");
    let line = screen.lines().find(|l| l.contains("-two")).unwrap();
    assert!(line.contains("+TWO"), "{line}");
    // Both scrolling, the cursor is on the new side; on the old one, its
    // line there.
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Left]);
    assert!(app.status.contains("scrolling the old side"), "{}", app.status);
    assert_eq!(app.rows[app.cursor].text, "-two");
    assert!(draw(&mut app).contains("compact <"));
    keys(&mut app, &[KeyCode::Right, KeyCode::Right]);
    assert!(app.status.contains("scrolling the new side"), "{}", app.status);
    assert_eq!(app.rows[app.cursor].text, "+TWO", "the cursor is on the new side's line");
    // Past the old side, ← goes on to the patches.
    keys(&mut app, &[KeyCode::Left, KeyCode::Left, KeyCode::Left]);
    assert_eq!(app.focus, Focus::List);
    // And back to one column.
    keys(&mut app, &[KeyCode::Char('1')]);
    assert_eq!(app.layout, Layout::Unified);
    // The view is 1's: it is not in the menu.
    keys(&mut app, &[KeyCode::Char(' ')]);
    assert!(!items(&app).iter().any(|(l, _)| l.contains("view")), "{:?}", items(&app));
}

#[test]
fn no_letter_is_a_shortcut() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    let before = std::fs::read(&db).unwrap();
    let first = draw(&mut app);
    // Only 1 and 2 are keys of their own, as in self-review.
    for c in ('a'..='z').chain('A'..='Z').chain("03456789!@#$%^&*()-=_+[]{};:'\",.<>/\\|`~".chars()) {
        assert_eq!(key(&mut app, KeyCode::Char(c)), Effect::None, "{c:?}");
        assert_eq!(app.mode, Mode::Normal, "{c:?} did something");
        assert_eq!(app.screen, Screen::Main, "{c:?}");
    }
    assert_eq!(draw(&mut app), first, "nothing moved");
    assert_eq!(std::fs::read(&db).unwrap(), before, "nothing was written");
    // ctrl-c, as in every terminal program, quits.
    assert_eq!(app.key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)), Effect::Quit);
}

#[test]
fn the_keys_are_the_settings() {
    let (_tmp, db, _src) = setup();
    let (cfg, wrong) = stacked::config::Config::parse(
        "[keys]\nup = [\"up\", \"k\"]\ndown = [\"down\", \"j\"]\nmenu = \"m\"\nquit = [\"q\", \"ctrl-c\"]\n",
    )
    .unwrap();
    assert!(wrong.is_empty(), "{wrong:?}");
    stacked::config::init(stacked::config::Config::default());
    let mut app = App::with_config(&db, cfg).unwrap();
    draw(&mut app);
    keys(&mut app, &[KeyCode::Char('j')]);
    assert_eq!(app.sel, 1, "j is down");
    keys(&mut app, &[KeyCode::Char(' ')]);
    assert_eq!(app.mode, Mode::Normal, "space no longer is the menu");
    keys(&mut app, &[KeyCode::Char('m')]);
    assert!(matches!(app.mode, Mode::Menu(_)), "m is");
    keys(&mut app, &[KeyCode::Esc, KeyCode::Char('?')]);
    let screen = draw(&mut app);
    assert!(screen.contains("Press m for all that can be done here."), "the help says so:\n{screen}");
    keys(&mut app, &[KeyCode::Esc]);
    assert!(draw(&mut app).contains("enter: read it · m: menu"), "and the bar");
    // In the comment editor, letters are what is typed.
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+bee");
    keys(&mut app, &[KeyCode::Enter]);
    typed(&mut app, "jkmq");
    keys(&mut app, &[KeyCode::Esc]);
    choose_with(&mut app, "Save it", 'm');
    assert_eq!(notes_of(&db, "s", 1)[0].text, "jkmq");
    assert_eq!(key(&mut app, KeyCode::Char('q')), Effect::Quit);
}

#[test]
fn what_is_said_names_the_keys_of_the_settings() {
    let (_tmp, db, _src) = setup();
    let (cfg, wrong) = stacked::config::Config::parse("[keys]\nup = [\"k\", \"up\"]\ndown = [\"j\", \"down\"]\n").unwrap();
    assert!(wrong.is_empty(), "{wrong:?}");
    stacked::config::init(stacked::config::Config::default());
    let app = App::with_config(&db, cfg).unwrap();
    // The arrows alone, as in pairs.
    assert_eq!(app.say_keys("press ↓ to pick it, ↑ to go back, ↑↓ for another".into()), "press j to pick it, k to go back, kj for another");
}

/// As `choose`, the menu opened with `menu`.
fn choose_with(app: &mut App, label: &str, menu: char) -> Effect {
    if !matches!(app.mode, Mode::Menu(_)) {
        key(app, KeyCode::Char(menu));
    }
    choose(app, label)
}

#[test]
fn quitting_is_in_the_menu_at_its_end() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    draw(&mut app);
    // ↑ from the first item goes around to the last.
    keys(&mut app, &[KeyCode::Char(' '), KeyCode::Up]);
    assert_eq!(items(&app).last().unwrap().0, "Quit");
    assert_eq!(key(&mut app, KeyCode::Enter), Effect::Quit);
}

#[test]
fn the_menu_goes_by_place() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    draw(&mut app);
    let labels = |app: &mut App| {
        keys(app, &[KeyCode::Char(' ')]);
        let all: Vec<String> = items(app).into_iter().map(|(l, _)| l).collect();
        keys(app, &[KeyCode::Esc]);
        all
    };
    // A patch's, then what every menu ends with.
    let patch = labels(&mut app);
    assert!(patch.contains(&"Move this patch…".to_string()) && patch.contains(&"Comment on this patch…".to_string()), "{patch:?}");
    assert!(!patch.iter().any(|l| l.starts_with("Tree of edits") || l.starts_with("Apply")), "{patch:?}");
    assert_eq!(patch.last().unwrap(), "Quit");
    // The stack's, on the header only.
    keys(&mut app, &[KeyCode::Up]);
    let stack = labels(&mut app);
    for label in ["Comment on the stack…", "Show…", "Tree of edits…", "History…", "Apply the stack to the worktree…", "Delete this stack…"] {
        assert!(stack.contains(&label.to_string()), "{label}: {stack:?}");
    }
    assert!(stack.iter().any(|l| l.starts_with("Ask agent about all 0")), "{stack:?}");
    assert!(!stack.contains(&"Move this patch…".to_string()), "{stack:?}");
    // A line's.
    keys(&mut app, &[KeyCode::Down, KeyCode::Right]);
    down_to(&mut app, "+TWO");
    let line = labels(&mut app);
    assert_eq!(line[..2], ["Comment on this line…", "Comment in $EDITOR"], "{line:?}");
}

#[test]
fn question_mark_says_what_can_be_done_where_the_cursor_is() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    draw(&mut app);
    key(&mut app, KeyCode::Char('?'));
    let screen = draw(&mut app);
    assert!(screen.contains("what can I do here"), "{screen}");
    assert!(screen.contains("Press Enter to read this patch."), "{screen}");
    // The next key closes it, and does what it does: here, into the patch.
    keys(&mut app, &[KeyCode::Right]);
    assert_eq!(app.focus, Focus::Diff);
    down_to(&mut app, "+TWO");
    key(&mut app, KeyCode::Char('?'));
    let screen = draw(&mut app);
    assert!(screen.contains("Press Enter to create a comment on this line."), "{screen}");
    assert!(screen.contains("Press 1 for the side-by-side view of the code; 2 to read"), "{screen}");
    // Enter after reading it does just that.
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "why so loud?");
    keys(&mut app, &[KeyCode::Down]);
    key(&mut app, KeyCode::Char('?'));
    assert!(draw(&mut app).contains("Press Enter to edit what you said last, or add to it."));
    keys(&mut app, &[KeyCode::Esc]);
    // In a menu, it says what the selected item does.
    keys(&mut app, &[KeyCode::Char(' ')]);
    key(&mut app, KeyCode::Char('?'));
    assert!(draw(&mut app).contains("Press Enter to edit what you said last, or add to it."));
    keys(&mut app, &[KeyCode::Char('?'), KeyCode::Down, KeyCode::Down]);
    key(&mut app, KeyCode::Char('?'));
    assert!(draw(&mut app).contains("Press Enter to reply to this comment."));
    keys(&mut app, &[KeyCode::Char('?')]);
    assert!(matches!(app.mode, Mode::Menu(_)), "back in the menu");
}

#[test]
fn left_on_the_patches_hides_them() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    draw(&mut app);
    keys(&mut app, &[KeyCode::Left]);
    assert!(!app.list && app.focus == Focus::Diff);
    let screen = draw(&mut app);
    assert!(!screen.contains("Bug 2 - add b"), "the patches are gone:\n{screen}");
    assert!(screen.lines().nth(1).unwrap().starts_with("✉ message"), "the patch has the width:\n{screen}");
    assert!(screen.contains("patch 1 of 2"), "{screen}");
    keys(&mut app, &[KeyCode::Left]);
    assert!(app.list && app.focus == Focus::List);
    assert!(draw(&mut app).contains("  2 Bug 2 - add b"));
}

#[test]
fn right_on_the_comments_hides_them() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "why so loud?");
    assert!(app.notes_shown());
    let cursor = app.cursor;
    keys(&mut app, &[KeyCode::Right]);
    assert_eq!(app.focus, Focus::Notes);
    // Further right than the comments: they make way for the patch.
    keys(&mut app, &[KeyCode::Right]);
    assert!(!app.notes_shown() && app.focus == Focus::Diff);
    assert_eq!(app.cursor, cursor, "the patch is where it was");
    let screen = draw(&mut app);
    assert!(!screen.contains("you · a.txt: TWO"), "the pane is gone:\n{screen}");
    assert!(screen.contains("· 1 comment"), "how many there are, in the bar:\n{screen}");
    // Put away it stays, on another patch and back; → brings it back.
    keys(&mut app, &[KeyCode::Left, KeyCode::Down, KeyCode::Up, KeyCode::Right]);
    assert!(!app.notes_shown());
    key(&mut app, KeyCode::Char('?'));
    let screen = draw(&mut app);
    assert!(screen.contains("→ to show"), "{screen}");
    keys(&mut app, &[KeyCode::Right]);
    assert!(app.notes_shown() && app.focus == Focus::Notes);
    assert!(draw(&mut app).contains("you · a.txt: TWO"));
}

#[test]
fn moving_and_dropping_patches() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    draw(&mut app);
    choose(&mut app, "Move this patch");
    keys(&mut app, &[KeyCode::Down]);
    assert!(draw(&mut app).contains(" ↕ Bug 1 - shout two"), "where it would go is shown");
    keys(&mut app, &[KeyCode::Enter]);
    assert_eq!(app.sel, 1, "the selection follows the patch");
    assert!(draw(&mut app).contains("  1 Bug 2 - add b"));
    choose(&mut app, "Drop this patch");
    assert!(draw(&mut app).contains("drop Bug 1 - shout two?"));
    choose(&mut app, "Drop it");
    let screen = draw(&mut app);
    assert!(!screen.contains("shout two"), "{screen}");
    choose(&mut app, "Undo: drop");
    choose(&mut app, "Undo: move");
    assert!(draw(&mut app).contains("  1 Bug 1 - shout two"));
    choose(&mut app, "Redo: move");
    assert!(draw(&mut app).contains("  1 Bug 2 - add b"));
    // Deleting the stack is the stack's: on its header.
    keys(&mut app, &[KeyCode::Home]);
    choose(&mut app, "Delete this stack");
    choose(&mut app, "Delete it");
    assert!(draw(&mut app).contains("s is deleted:"));
    choose(&mut app, "Undo: delete");
    assert!(draw(&mut app).contains("  1 Bug 2 - add b"));
}

#[test]
fn a_move_that_does_not_apply_stays_to_be_placed() {
    let (_tmp, db, src) = setup();
    // A third patch that needs the first.
    let lines: String = (1..=40).map(|n| format!("line {n}\n")).collect();
    std::fs::write(src.join("a.txt"), format!("one\nII\nthree\n{}", lines.replace("line 39\n", "LINE 39\n"))).unwrap();
    git(&src, &["commit", "-qam", "Bug 3 - two again"]);
    stacked(&src, &db, &["import", "--replace", "s", "HEAD~3"]);
    let mut app = open(&db).unwrap();
    draw(&mut app);
    keys(&mut app, &[KeyCode::End]);
    choose(&mut app, "Move this patch");
    keys(&mut app, &[KeyCode::Home, KeyCode::Enter]);
    assert!(app.status.contains("does not apply"), "{}", app.status);
    assert!(matches!(app.mode, Mode::Moving { .. }), "still placing it");
    // The status line says why, and, never cut, how to go on from there.
    let screen = draw(&mut app);
    let status = screen.lines().last().unwrap().trim_end();
    assert!(status.contains("does not apply") && status.ends_with("· ↑↓: elsewhere · esc: leave it where it was"), "{status}");
    keys(&mut app, &[KeyCode::Esc]);
    assert!(draw(&mut app).contains("  3 Bug 3 - two again"), "left where it was");
}

#[test]
fn comments_are_written_where_they_will_show() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    // Enter comments on the line, in an editor under it; the arrows edit.
    keys(&mut app, &[KeyCode::Enter]);
    typed(&mut app, "wy so loud?");
    for _ in 0.."y so loud?".len() {
        key(&mut app, KeyCode::Left);
    }
    typed(&mut app, "h");
    let (screen, at) = draw_at(&mut app);
    assert!(screen.contains("a comment on: TWO") && screen.contains("why so loud?"), "{screen}");
    assert!(at.is_some(), "the terminal's cursor is in the editor");
    let under = screen.lines().position(|l| l.contains("+TWO")).unwrap();
    assert!(screen.lines().nth(under + 1).unwrap().contains("a comment on: TWO"), "under its line:\n{screen}");
    // Enter breaks the line; esc asks what to do with it.
    keys(&mut app, &[KeyCode::End, KeyCode::Enter]);
    typed(&mut app, "really");
    keys(&mut app, &[KeyCode::Esc]);
    let all: Vec<String> = items(&app).into_iter().map(|(l, _)| l).collect();
    assert_eq!(
        all,
        [
            "Save it",
            "Save it and ask agent",
            "Save it and ask agent, the whole stack",
            "Save it and ask agent, answer only",
            "",
            "Throw it away",
            "Keep writing"
        ]
    );
    // Esc keeps writing too.
    keys(&mut app, &[KeyCode::Esc]);
    assert!(matches!(app.mode, Mode::Edit(_)));
    typed(&mut app, "?");
    keys(&mut app, &[KeyCode::Esc]);
    choose(&mut app, "Save it");
    let screen = draw(&mut app);
    assert!(screen.contains("└ comment:") && screen.contains("why so loud?") && screen.contains("really?"), "{screen}");
    assert!(screen.contains("[1]"), "{screen}");
    // On the comment, the last word yours, enter opens it under it, to edit
    // or add to.
    keys(&mut app, &[KeyCode::Down, KeyCode::Enter]);
    assert!(draw(&mut app).contains("your comment: edit it, or add to it"));
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "it is a shout");
    let notes = notes_of(&db, "s", 0);
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].text, "why so loud?\nreally?\nit is a shout");
    // Thrown away, or closed with nothing written, nothing is.
    let before = std::fs::read(&db).unwrap();
    down_to(&mut app, "+LINE 39");
    keys(&mut app, &[KeyCode::Enter]);
    typed(&mut app, "never mind");
    keys(&mut app, &[KeyCode::Esc]);
    choose(&mut app, "Throw it away");
    keys(&mut app, &[KeyCode::Enter, KeyCode::Esc]);
    assert_eq!(app.mode, Mode::Normal, "nothing written, nothing asked");
    assert_eq!(std::fs::read(&db).unwrap(), before);

    // A comment on the patch, under it on the left, the pane wider meanwhile.
    keys(&mut app, &[KeyCode::Left]);
    choose(&mut app, "Comment on this patch…");
    let screen = draw(&mut app);
    assert_eq!(screen.lines().nth(1).unwrap().chars().nth(60), Some('│'), "half the width:\n{screen}");
    assert!(screen.lines().nth(3).unwrap().contains("a comment on the patch"), "under the patch:\n{screen}");
    save(&mut app, "split this");
    let n = notes_of(&db, "s", 0).into_iter().find(|n| n.text == "split this").unwrap();
    assert_eq!(n.file, "✉ message");
    // And one on the stack, under the header.
    keys(&mut app, &[KeyCode::Up]);
    choose(&mut app, "Comment on the stack…");
    assert!(draw(&mut app).lines().nth(2).unwrap().contains("a comment on the stack"));
    save(&mut app, "rebase onto main");
    let screen = draw(&mut app);
    assert!(screen.contains(" ≡ s · 2 patches · 1 comment"), "{screen}");
    assert!(screen.contains("comments on the stack") && screen.contains("rebase onto main"), "{screen}");
    let d = Db::load(&db).unwrap();
    assert_eq!(d.notes(&stack_change("s")).len(), 1);
}

#[test]
fn a_patch_is_edited_by_hand_where_the_cursor_is() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    // On a line: its file as the patch leaves it, opened at the line; or
    // the patch, opened where the line is in it.
    assert_eq!(choose(&mut app, "Edit a.txt here in $EDITOR"), Effect::EditCode { file: "a.txt".into(), line: 2, text: None });
    let line = match choose(&mut app, "Edit this patch in $EDITOR") {
        Effect::EditPatch { line: Some(l) } => l,
        e => panic!("{e:?}"),
    };
    let d = Db::load(&db).unwrap();
    let p = d.patch(&d.stack("s").unwrap().patches[0]).clone();
    let mail = stacked::ops::render(&p);
    assert_eq!(mail.lines().nth(line - 1), Some("+TWO"));
    // Every row of the patch is where the mail has it.
    for row in app.rows.clone() {
        if row.diff_at.is_none() && row.msg_at.is_none() {
            continue;
        }
        let line = stacked::ops::mail_line(&p, &row);
        let got = mail.lines().nth(line - 1).unwrap();
        if row.msg_at == Some(0) {
            assert_eq!(got, format!("Subject: {}", p.title()));
        } else if row.msg_at.is_some() {
            assert_eq!(got, row.text.trim_start_matches("   "), "{row:?}");
        } else {
            assert_eq!(got, p.diff.lines().nth(row.diff_at.unwrap()).unwrap(), "{row:?}");
        }
    }
    // From a hunk's header, its first line; from the patches, the same as
    // where the middle's cursor is.
    down_to(&mut app, "@@ -39");
    assert_eq!(choose(&mut app, "Edit a.txt here"), Effect::EditCode { file: "a.txt".into(), line: 39, text: None });
    keys(&mut app, &[KeyCode::Left]);
    assert!(matches!(choose(&mut app, "Edit this patch in $EDITOR"), Effect::EditPatch { line: Some(_) }));
}

#[test]
fn deleting_a_comment_asks_first() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "why so loud?");
    keys(&mut app, &[KeyCode::Down]);
    // The question is asked careful, keeping it selected.
    choose(&mut app, "Delete this comment");
    assert_eq!(items(&app)[0].0, "Delete it");
    keys(&mut app, &[KeyCode::Enter]);
    assert!(draw(&mut app).contains("why so loud?"), "enter kept it");
    choose(&mut app, "Delete this comment");
    choose(&mut app, "Delete it");
    assert!(!draw(&mut app).contains("why so loud?"));
}

#[test]
fn the_comments_pane_previews_without_moving_the_patch() {
    let (_tmp, db, src) = setup();
    // A patch longer than the screen, on a stack of its own: lines changed
    // between the two that get comments.
    git(&src, &["checkout", "-q", "-b", "long", "HEAD~2"]);
    let middle: String = (1..=40)
        .map(|n| if (12..=30).contains(&n) { format!("LINE {n}\n") } else { format!("line {n}\n") })
        .collect();
    std::fs::write(src.join("a.txt"), format!("one\nTWO\nthree\n{}", middle.replace("line 39\n", "LINE 39\n"))).unwrap();
    git(&src, &["commit", "-qam", "Bug 3 - shout more"]);
    stacked(&src, &db, &["import", "long", "HEAD~1"]);
    let mut app = open(&db).unwrap();
    assert_eq!(app.stack, "long");
    let screen = draw(&mut app);
    assert!(!app.notes_shown(), "no comments, no pane:\n{screen}");
    // Two comments: near the top and near the bottom of the patch.
    keys(&mut app, &[KeyCode::Right]);
    for (line, text) in [("+TWO", "top one"), ("+LINE 39", "bottom one")] {
        keys(&mut app, &[KeyCode::Home]);
        down_to(&mut app, line);
        keys(&mut app, &[KeyCode::Enter]);
        save(&mut app, text);
    }
    let screen = draw(&mut app);
    assert!(app.notes_shown());
    assert!(screen.contains("you · a.txt: TWO") && screen.contains("you · a.txt: LINE 39"), "{screen}");
    assert!(screen.contains(" 2 comments"), "its header counts them:\n{screen}");
    // From the top of the patch, into the pane: the bottom comment is
    // shown over the bottom of the patch, the patch staying where it is.
    keys(&mut app, &[KeyCode::Home]);
    let top = app.top;
    keys(&mut app, &[KeyCode::Right]);
    assert_eq!(app.focus, Focus::Notes);
    keys(&mut app, &[KeyCode::Down]);
    let screen = draw(&mut app);
    assert_eq!(app.top, top, "the patch did not move");
    assert!(screen.contains("▼ comment 2 of 2, below"), "{screen}");
    assert!(screen.contains("bottom one"), "{screen}");
    // The first is in sight: marked where it is.
    keys(&mut app, &[KeyCode::Up]);
    let screen = draw(&mut app);
    assert!(!screen.contains("▼ comment") && screen.contains("▶"), "{screen}");
    // Enter goes there.
    keys(&mut app, &[KeyCode::Down, KeyCode::Enter]);
    assert_eq!(app.focus, Focus::Diff);
    assert!(app.rows[app.cursor].head, "on the comment");
    let screen = draw(&mut app);
    assert!(screen.contains("bottom one") && !screen.contains("▼ comment"), "{screen}");
    // From the bottom, the first is above.
    keys(&mut app, &[KeyCode::Right, KeyCode::Up]);
    assert!(draw(&mut app).contains("▲ comment 1 of 2, above"));
    // Space offers what can be done to the selected comment; a reply is
    // written in the pane, under the comment.
    choose(&mut app, "Reply…");
    let screen = draw(&mut app);
    assert!(screen.contains("a reply"), "{screen}");
    save(&mut app, "replied from the pane");
    let notes = notes_of(&db, "long", 0);
    assert!(notes.iter().any(|n| n.text == "top one\nreplied from the pane"), "{notes:?}");
    // Up from the first comment: the list's header, for all of them.
    keys(&mut app, &[KeyCode::Home]);
    let all: Vec<String> = {
        keys(&mut app, &[KeyCode::Char(' ')]);
        items(&app).into_iter().map(|(l, _)| l).collect()
    };
    assert!(all.contains(&"Ask agent about all 2".to_string()) && all.contains(&"Show…".to_string()), "{all:?}");
}

/// A claude that says what it does for a second, then answers each comment
/// it is sent; for every test of the editor, as they share their process.
fn fake_claude() -> PathBuf {
    static FAKE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    FAKE.get_or_init(|| {
        let path = std::env::temp_dir().join(format!("stacked-tui-claude-{}.sh", std::process::id()));
        let result = serde_json::to_string(&serde_json::json!({"type": "result", "result": r#"{"replies": {"c1": "Seen."}}"#})).unwrap();
        std::fs::write(
            &path,
            format!(
                "#!/bin/sh\necho '{{\"type\":\"system\",\"subtype\":\"init\",\"model\":\"fake\"}}'\necho '{{\"type\":\"assistant\",\"message\":{{\"content\":[{{\"type\":\"text\",\"text\":\"Reading the patch.\"}}]}}}}'\nsleep 2\ncat <<'EOF'\n{result}\nEOF\n"
            ),
        )
        .unwrap();
        Command::new("chmod").arg("+x").arg(&path).status().unwrap();
        stacked::ask::stand_in(&path, Path::new(env!("CARGO_BIN_EXE_stacked")));
        path
    })
    .clone()
}

#[test]
fn what_you_said_last_may_be_written_again() {
    let (_tmp, db, _src) = setup();
    fake_claude();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "why so loud?");
    keys(&mut app, &[KeyCode::Down]);
    assert!(app.rows[app.cursor].head);
    // Unanswered, it is the comment itself, written again where it is: the
    // menu's first, as enter does.
    keys(&mut app, &[KeyCode::Char(' ')]);
    assert_eq!(items(&app)[0].0, "Edit your comment…");
    choose(&mut app, "Edit your comment…");
    assert!(matches!(&app.mode, Mode::Edit(e) if e.ed.text() == "why so loud?"), "{:?}", app.mode);
    assert!(draw(&mut app).contains("your comment: edit it, or add to it"));
    typed(&mut app, " really");
    keys(&mut app, &[KeyCode::Esc]);
    choose(&mut app, "Save it");
    let n = notes_of(&db, "s", 0).remove(0);
    assert_eq!(n.text, "why so loud? really");
    // Enter on its line opens it too; left as it was, it closes.
    keys(&mut app, &[KeyCode::Up]);
    assert!(app.rows[app.cursor].text.starts_with("+TWO"));
    keys(&mut app, &[KeyCode::Enter]);
    assert!(matches!(&app.mode, Mode::Edit(e) if e.ed.text() == "why so loud? really"), "{:?}", app.mode);
    keys(&mut app, &[KeyCode::Esc, KeyCode::Down]);
    assert_eq!(app.mode, Mode::Normal);
    // Or in $EDITOR.
    let target = stacked::tui::Target::Edit { change: n.change.clone(), id: n.id.clone() };
    assert_eq!(choose(&mut app, "Edit your comment in $EDITOR"), Effect::EditComment(target));
    // Answered, the agent's word is last: nothing of yours to edit.
    Db::edit(&db, |d| {
        let n = d.note(&n.change, &n.id).cloned().unwrap();
        d.set_note(Note {
            text: append_turn(&n.text, Role::Agent, "It is a shout.", Some("answer")),
            ..n
        })
    })
    .unwrap();
    app.refresh();
    keys(&mut app, &[KeyCode::Char(' ')]);
    assert!(!items(&app).iter().any(|(l, _)| l.starts_with("Edit your")), "{:?}", items(&app));
    keys(&mut app, &[KeyCode::Esc]);
    // Replied to, the reply is what is written again, the rest as it was.
    choose(&mut app, "Reply…");
    save(&mut app, "louder");
    // Enter, the last word yours again, opens it.
    assert!(draw(&mut app).contains("enter: edit it · "));
    keys(&mut app, &[KeyCode::Enter]);
    assert!(matches!(&app.mode, Mode::Edit(e) if e.ed.text() == "louder"), "{:?}", app.mode);
    for _ in 0.."louder".len() {
        key(&mut app, KeyCode::Backspace);
    }
    typed(&mut app, "quieter");
    keys(&mut app, &[KeyCode::Esc]);
    choose(&mut app, "Save it");
    let text = notes_of(&db, "s", 0).remove(0).text;
    let turns = stacked::thread::thread_turns(&text);
    let said: Vec<&str> = turns.iter().map(|t| t.body.as_str()).collect();
    assert_eq!(said, ["why so loud? really", "It is a shout.", "quieter"]);
    // Waiting its turn, it leaves the agent's queue while it is edited: edited,
    // it waits to be asked again.
    let wait = |db: &Path| {
        Db::edit(db, |d| {
            let n = d.notes(&d.patch(&d.stack("s")?.patches[0]).change.clone()).into_iter().next().cloned().unwrap();
            d.set_note(Note {
                queued: Some(stacked::db::Queued {
                    way: stacked::db::Way::Answer,
                    batch: "b-waiting".into(),
                    stack: false,
                }),
                ..n
            })
        })
        .unwrap();
    };
    wait(&db);
    app.refresh();
    choose(&mut app, "Edit your reply…");
    assert!(notes_of(&db, "s", 0)[0].queued.is_none(), "off the queue meanwhile");
    typed(&mut app, ", please");
    keys(&mut app, &[KeyCode::Esc]);
    choose(&mut app, "Save it");
    assert!(app.status.contains("off the agent's queue, until you ask again"), "{}", app.status);
    let n = notes_of(&db, "s", 0).remove(0);
    assert!(n.queued.is_none() && n.text.ends_with("quieter, please"), "{n:?}");
    // The edit thrown away, it goes back on, and is asked as it was.
    wait(&db);
    app.refresh();
    choose(&mut app, "Edit your reply…");
    typed(&mut app, " now");
    keys(&mut app, &[KeyCode::Esc]);
    choose(&mut app, "Throw it away");
    assert!(app.status.contains("as it was: back on the agent's queue (answer only)"), "{}", app.status);
    // Not while the agent works on it.
    wait_for(&mut app, "⋯ Reading the patch.");
    keys(&mut app, &[KeyCode::Char(' ')]);
    assert!(items(&app).contains(&("Edit your reply…".to_string(), false)), "{:?}", items(&app));
    keys(&mut app, &[KeyCode::Esc]);
    wait_for(&mut app, "agent replied (answer only)");
    // Its history says which were edits.
    let d = Db::load(&db).unwrap();
    let n = notes_of(&db, "s", 0).remove(0);
    let mut said = Vec::new();
    let mut before: Option<&Note> = None;
    for (_, x) in d.note_history(&n.change, &n.id) {
        said.push(stacked::history::note_change(before, x));
        before = Some(x);
    }
    assert_eq!(said.iter().filter(|s| *s == "you edited it").count(), 3, "{said:?}");
    assert_eq!(said.iter().filter(|s| *s == "you replied").count(), 1, "{said:?}");
    choose(&mut app, "History of this comment…");
    let screen = draw(&mut app);
    assert!(screen.contains("· written") && screen.contains("· you edited it"), "the first edit, there:\n{screen}");
}

#[test]
fn a_comment_on_a_patch_may_be_sent_to_the_whole_stack() {
    let (_tmp, db, _src) = setup();
    fake_claude();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    typed(&mut app, "test it in a patch of its own");
    keys(&mut app, &[KeyCode::Esc]);
    // Written on a patch, it may be saved and sent past its patch.
    let all: Vec<String> = items(&app).into_iter().map(|(l, _)| l).collect();
    assert_eq!(all[1..3], ["Save it and ask agent", "Save it and ask agent, the whole stack"], "{all:?}");
    choose(&mut app, "Save it");
    // Its menu has it too, beside asking about its patch alone.
    keys(&mut app, &[KeyCode::Down]);
    assert!(app.rows[app.cursor].head);
    keys(&mut app, &[KeyCode::Char(' ')]);
    let all: Vec<String> = items(&app).into_iter().map(|(l, _)| l).collect();
    let at = all.iter().position(|l| l == "Ask agent").unwrap();
    assert_eq!(all[at + 1], "Ask agent, the whole stack", "{all:?}");
    choose(&mut app, "Ask agent, the whole stack");
    assert!(app.status.contains("asked the agent about 1 comment (may change the whole stack)"), "{}", app.status);
    let screen = wait_for(&mut app, "⋯ Reading the patch.");
    assert!(screen.contains("with the agent (may change the whole stack)"), "{screen}");
    // The ask has the whole stack; its answer lands in the thread, on its
    // line.
    let d = Db::load(&db).unwrap();
    let run = d.runs().last().unwrap().id.clone();
    assert_eq!(stacked::ask::run_scope(&d, &run), Some(stacked::ask::Scope::Stack));
    drop(d);
    let screen = wait_for(&mut app, "agent replied (change)");
    assert!(screen.contains("Seen.") && screen.contains("you · a.txt: TWO"), "{screen}");
    // A comment on the stack has the whole stack already.
    keys(&mut app, &[KeyCode::Left, KeyCode::Home]);
    choose(&mut app, "Comment on the stack…");
    save(&mut app, "and the rest of it");
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "   └ comment");
    keys(&mut app, &[KeyCode::Char(' ')]);
    let all: Vec<String> = items(&app).into_iter().map(|(l, _)| l).collect();
    assert!(all.contains(&"Ask agent".to_string()) && !all.iter().any(|l| l.contains("the whole stack")), "{all:?}");
}

#[test]
fn a_comment_is_sent_from_its_menu_and_answered_in_place() {
    let (_tmp, db, _src) = setup();
    fake_claude();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    typed(&mut app, "why so loud?");
    keys(&mut app, &[KeyCode::Esc]);
    choose(&mut app, "Save it and ask agent, answer only");
    assert!(app.status.contains("asked the agent about 1 comment (answer only)"), "{}", app.status);
    // While claude works, its stream shows under the comment, and the top
    // bar and the comments pane say so.
    let screen = wait_for(&mut app, "⋯ Reading the patch.");
    assert!(screen.contains("with the agent (answer only)"), "{screen}");
    assert!(screen.contains("agent ●1"), "{screen}");
    assert!(screen.contains("● you · a.txt: TWO") && screen.contains("1 with the agent"), "{screen}");
    // Enter on it opens the stream in place; esc folds it.
    keys(&mut app, &[KeyCode::Down]);
    assert!(app.rows[app.cursor].head);
    keys(&mut app, &[KeyCode::Enter]);
    assert!(matches!(app.mode, Mode::Stream { .. }), "{:?}", app.mode);
    assert!(draw(&mut app).contains("● session started · fake"));
    keys(&mut app, &[KeyCode::Esc]);
    assert_eq!(app.mode, Mode::Normal);
    // Its answer lands as a turn of the thread.
    let screen = wait_for(&mut app, "agent replied (answer only)");
    assert!(screen.contains("Seen.") && screen.contains("✓ you · a.txt: TWO"), "{screen}");
    assert!(screen.contains("1 to read"), "{screen}");
    assert!(app.status.contains("the agent's answer done"), "{}", app.status);

    // Show… picks what both panes show, each with its count; kept per stack.
    keys(&mut app, &[KeyCode::Right, KeyCode::Home]);
    choose(&mut app, "Show…");
    let all: Vec<String> = items(&app).into_iter().map(|(l, _)| l).collect();
    assert_eq!(all[..4], ["● all (1)", "  to send (0)", "  to read (1)", "  to read: read-only (1)"], "{all:?}");
    choose(&mut app, "  to send");
    assert_eq!(app.show, Show::ToSend);
    let screen = draw(&mut app);
    assert!(!screen.contains("Seen."), "hidden in the patch too:\n{screen}");
    assert_eq!(Db::load(&db).unwrap().view("s"), Some("to send"));
    let mut again = open(&db).unwrap();
    assert_eq!(again.show, Show::ToSend, "kept in the file");
    draw(&mut again);
}

#[test]
fn the_screen_never_waits_on_the_agent() {
    let (_tmp, db, _src) = setup();
    fake_claude();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    typed(&mut app, "why so loud?");
    keys(&mut app, &[KeyCode::Esc]);
    // Asked, the ask is started elsewhere: its worktree is not made while
    // the screen waits, and keys go on working meanwhile.
    choose(&mut app, "Save it and ask agent");
    assert!(app.working(), "the ask starts away from the screen");
    keys(&mut app, &[KeyCode::Down, KeyCode::Up]);
    app.settle();
    assert!(!app.working());
    let screen = wait_for(&mut app, "with the agent (may change the code)");
    assert!(screen.contains("agent ●1"), "{screen}");
    // A stack made by someone else is checked away from the screen too.
    Db::edit(&db, |d| stacked::ops::drop_patches(d, "s", &[1]).map(|_| ())).unwrap();
    app.refresh();
    assert!(app.working(), "the new stack is checked elsewhere");
    assert!(draw(&mut app).contains("· checking…"));
    app.settle();
    assert!(!draw(&mut app).contains("checking…"));
    wait_for(&mut app, "agent replied");
}

#[test]
fn redrawing_does_not_slow_as_the_agent_writes() {
    let (_tmp, db, _src) = setup();
    // An agent at work on a comment on the stack, its ask's prompt long, and
    // twenty thousand lines of its stream already in the file.
    Db::edit(&db, |d| {
        d.set_note(Note {
            change: stack_change("s"),
            id: "s1".into(),
            text: "Rebase it.".into(),
            ..Default::default()
        })?;
        let from = d.head("s").unwrap().0;
        d.add_run(stacked::db::Run {
            id: "r1".into(),
            what: "change".into(),
            stack: "s".into(),
            state: stacked::db::RunState::Started,
            pid: Some(std::process::id()),
            ask: Some(serde_json::json!({
                "way": "change", "scope": "stack", "from": from,
                "notes": [{"label": "c1", "change": stack_change("s"), "id": "s1"}],
                "prompt": "x".repeat(50_000),
            })),
            ..Default::default()
        })
        .map(|_| ())
    })
    .unwrap();
    let line = |n: usize| stacked::db::Record::Event {
        run: "r1".into(),
        event: serde_json::json!({"type": "assistant", "message": {"content": [{"type": "text", "text": format!("step {n}")}]}}),
    };
    Db::append(&db, &(0..20_000).map(line).collect::<Vec<_>>()).unwrap();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Home]);
    assert!(draw(&mut app).contains("⋯ step 19999"));
    // It writes on, the editor reading and drawing as it does.
    let mut slowest = std::time::Duration::ZERO;
    for k in 0..40 {
        Db::append(&db, &(0..50).map(|n| line(20_000 + k * 50 + n)).collect::<Vec<_>>()).unwrap();
        let t = std::time::Instant::now();
        app.refresh();
        draw(&mut app);
        slowest = slowest.max(t.elapsed());
    }
    assert!(draw(&mut app).contains("⋯ step 21999"), "the stream shown is the latest");
    assert!(slowest < std::time::Duration::from_millis(100), "a frame took {slowest:?}");
}

#[test]
fn two_reads_rather_than_comments() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    draw(&mut app);
    keys(&mut app, &[KeyCode::Char('2')]);
    assert!(app.reading);
    assert!(draw(&mut app).contains("· reading"));
    keys(&mut app, &[KeyCode::Enter]);
    app.wait_blocks();
    down_to(&mut app, "+TWO");
    let screen = draw(&mut app);
    assert!(screen.contains("space: mark this block read · enter: unread"), "{screen}");
    assert!(screen.contains("○→✓ +TWO"), "what space would mark, marked:\n{screen}");
    key(&mut app, KeyCode::Char('?'));
    let screen = draw(&mut app);
    assert!(screen.contains("Press Space to mark this block as read: this hunk, 2 lines not read"), "{screen}");
    assert!(screen.contains("nothing here is marked read"), "{screen}");
    // Space marks it read: no menu in the code. The cursor goes on to what
    // is left to read.
    keys(&mut app, &[KeyCode::Esc, KeyCode::Char(' ')]);
    assert_eq!(app.mode, Mode::Normal);
    assert!(app.status.contains("read: this hunk, 2 lines"), "{}", app.status);
    assert_eq!(app.rows[app.cursor].text, "-line 39");
    let screen = draw(&mut app);
    assert!(screen.contains("  ✓ +TWO") && screen.contains("read 2/2 lines"), "{screen}");
    assert!(screen.contains("reading 2/4") && screen.contains(" 2/4"), "{screen}");
    keys(&mut app, &[KeyCode::Char(' ')]);
    assert!(app.status.contains("nothing left to read below"), "{}", app.status);
    let screen = draw(&mut app);
    let line = screen.lines().find(|l| l.contains("1 Bug 1 - shout two")).unwrap();
    assert!(line.contains(" ✓│"), "all read: {line}");
    // Enter takes the mark back.
    assert_eq!(app.rows[app.cursor].text, "-line 39", "where the last mark was made");
    keys(&mut app, &[KeyCode::Enter]);
    assert!(app.status.contains("unread: 2 lines"), "{}", app.status);
    assert!(draw(&mut app).contains("reading 2/4"));
    // Kept in the file.
    let mut again = open(&db).unwrap();
    draw(&mut again);
    keys(&mut again, &[KeyCode::Char('2'), KeyCode::Enter]);
    down_to(&mut again, "+TWO");
    assert!(again.rows[again.cursor].read);
    // 2 again comments: enter is for that again, space the menu.
    keys(&mut again, &[KeyCode::Char('2')]);
    keys(&mut again, &[KeyCode::Enter]);
    assert!(matches!(again.mode, Mode::Edit(_)), "{:?}", again.mode);
}

#[test]
fn reading_goes_by_the_blocks_of_the_code() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir(&src).unwrap();
    git(&src, &["init", "-q", "-b", "main"]);
    git(&src, &["config", "commit.gpgsign", "false"]);
    git(&src, &["config", "tag.gpgsign", "false"]);
    std::fs::write(src.join("f.rs"), "fn a() {\n    one();\n}\n\nfn b() {\n    two();\n}\n").unwrap();
    git(&src, &["add", "."]);
    git(&src, &["commit", "-qm", "base"]);
    std::fs::write(src.join("f.rs"), "fn a() {\n    uno();\n}\n\nfn b() {\n    dos();\n}\n").unwrap();
    git(&src, &["commit", "-qam", "Bug 1 - in Spanish"]);
    let db = tmp.path().join("db.jsonl");
    stacked(&src, &db, &["import", "s", "HEAD~1"]);
    let mut app = open(&db).unwrap();
    draw(&mut app);
    keys(&mut app, &[KeyCode::Char('2'), KeyCode::Enter]);
    app.wait_blocks();
    down_to(&mut app, "-    one();");
    // One function at a time, though both are in one hunk.
    keys(&mut app, &[KeyCode::Char(' ')]);
    assert!(app.status.contains("read: the block of lines 1–3, 2 lines"), "{}", app.status);
    assert_eq!(app.rows[app.cursor].text, "-    two();");
    keys(&mut app, &[KeyCode::Char(' ')]);
    assert!(app.status.contains("read: the block of lines 5–7, 2 lines"), "{}", app.status);
    assert!(draw(&mut app).contains("reading 4/4"));
}

#[test]
fn the_history_of_the_stack_its_patches_and_comments() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "why so loud?");
    // Enter on it, the last word yours, opens it: a line added to it.
    keys(&mut app, &[KeyCode::Down, KeyCode::Enter, KeyCode::Enter]);
    save(&mut app, "louder");
    // A comment's history: each state it was in.
    choose(&mut app, "History of this comment…");
    assert_eq!(app.screen, Screen::Text);
    let screen = draw(&mut app);
    assert!(screen.contains("· written") && screen.contains("· you replied") && screen.contains("louder"), "{screen}");
    keys(&mut app, &[KeyCode::Esc]);
    assert_eq!(app.screen, Screen::Main);

    // A patch's versions: what changed from one to the next, and any put
    // back.
    Db::edit(&db, |d| {
        let s = d.stack("s")?.clone();
        let mut draft = stacked::mbox::Draft::from(d.patch(&s.patches[0]));
        draft.message = "Bug 1 - shout two, reworded".into();
        stacked::ops::replace(d, "s", 0, draft, "reword").map(|_| ())
    })
    .unwrap();
    app.refresh();
    keys(&mut app, &[KeyCode::Left]);
    choose(&mut app, "Versions of this patch…");
    assert_eq!(app.screen, Screen::Versions);
    let screen = draw(&mut app);
    assert!(screen.contains(" v1  @") && screen.contains(" v2● @") && screen.contains(" v2, in the stack · "), "{screen}");
    assert!(screen.contains("what changed from the version before") && screen.contains("+Subject: Bug 1 - shout two, reworded"), "{screen}");
    keys(&mut app, &[KeyCode::Up]);
    choose(&mut app, "Put this version back in the stack");
    assert!(app.status.contains("that version is back in the stack"), "{}", app.status);
    keys(&mut app, &[KeyCode::Esc]);
    assert!(draw(&mut app).contains("  1 Bug 1 - shout two "));

    // The stack's: what changed it, newest first, its comments not among
    // them; enter goes to the stack it made, in the tree.
    keys(&mut app, &[KeyCode::Home]);
    choose(&mut app, "History…");
    assert_eq!(app.screen, Screen::History);
    let screen = history_pane(&draw(&mut app));
    let back = screen.lines().position(|l| l.contains("· back to version")).unwrap();
    let reword = screen.lines().position(|l| l.contains("· reword ")).unwrap();
    assert!(back < reword && screen.contains("· import "), "newest first:\n{screen}");
    assert!(!screen.contains("written") && !screen.contains("replied"), "no comments:\n{screen}");
    assert_eq!(app.history().len(), 3, "{screen}");
    keys(&mut app, &[KeyCode::Down]);
    let rev = app.history()[app.pick].rev;
    keys(&mut app, &[KeyCode::Enter]);
    assert_eq!(app.screen, Screen::Tree);
    assert_eq!(stacked::tree::graph(&Db::load(&db).unwrap(), "s")[app.pick].rev, rev);

    // The tree: what that edit changed.
    choose(&mut app, "What this edit changed…");
    let screen = draw(&mut app);
    assert!(screen.contains(" 1 ~ Bug 1 - shout two, reworded") && screen.contains("+Subject: Bug 1 - shout two, reworded"), "{screen}");
}

#[test]
fn the_history_shows_each_change_as_a_stack_diff() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    // A comment on a line of the first patch, deleted after; the first
    // patch reworded, then dropped.
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "too loud");
    keys(&mut app, &[KeyCode::Down]);
    choose(&mut app, "Delete this comment");
    choose(&mut app, "Delete it");
    Db::edit(&db, |d| {
        let s = d.stack("s")?.clone();
        let mut draft = stacked::mbox::Draft::from(d.patch(&s.patches[0]));
        draft.message = "Bug 1 - shout two, reworded".into();
        stacked::ops::replace(d, "s", 0, draft, "reword").map(|_| ())
    })
    .unwrap();
    app.refresh();
    keys(&mut app, &[KeyCode::Left]);
    choose(&mut app, "Drop this patch");
    choose(&mut app, "Drop it");
    keys(&mut app, &[KeyCode::Home]);
    choose(&mut app, "History…");
    // Only what changed the stack, each in short; the comments changed
    // nothing of it.
    let list = history_pane(&draw(&mut app));
    let line = |what: &str| list.lines().find(|l| l.contains(what)).unwrap_or_else(|| panic!("no {what:?}:\n{list}")).to_string();
    assert!(line("· drop ").contains(" −1 · drop ") && line("· reword ").contains(" ~1 · reword ") && line("· import ").contains(" +2 · import "), "{list}");
    assert_eq!(app.history().len(), 3, "{list}");
    // Beside it, the drop's stack diff: from the stack before to the one
    // after, the one dropped where it was.
    let screen = draw(&mut app);
    assert!(screen.contains(" the stack @") && screen.contains("   − Bug 1 - shout two, reworded, was 1"), "{screen}");
    assert!(screen.contains("  1   Bug 2 - add b"), "{screen}");
    // The reword's: a new version, what changed in it under it.
    keys(&mut app, &[KeyCode::Down]);
    let screen = draw(&mut app);
    assert!(screen.contains("  1 ~ Bug 1 - shout two, reworded") && screen.contains("+Subject: Bug 1 - shout two, reworded"), "{screen}");
    assert!(screen.contains("  2   Bug 2 - add b") && !screen.contains("+TWO"), "only what changed:\n{screen}");
    // The import's: the stack made, every patch added.
    keys(&mut app, &[KeyCode::Down]);
    let screen = draw(&mut app);
    assert!(screen.contains(" the stack made, @") && screen.contains("  1 + Bug 1 - shout two") && screen.contains("  2 + Bug 2 - add b"), "{screen}");
    // → to the stack diff, ↓ through it.
    keys(&mut app, &[KeyCode::Up, KeyCode::Right]);
    assert_eq!(app.hist_focus, stacked::tui::HistFocus::Diff);
    keys(&mut app, &[KeyCode::Down]);
    assert_eq!(app.hist_cursor, 1);
    // Too narrow for both: the one the cursor is in.
    let mut buf = Buffer::empty(Rect::new(0, 0, 50, 30));
    app.draw(&mut buf);
    let narrow: String = (0..buf.area.width).map(|x| buf[(x, 1)].symbol()).collect();
    assert!(narrow.starts_with(" the stack @"), "{narrow}");
    // Only to look at: nothing is written, the menu has no edits.
    let before = std::fs::read(&db).unwrap();
    keys(&mut app, &[KeyCode::Enter]);
    assert!(app.status.contains("only to look at"), "{}", app.status);
    keys(&mut app, &[KeyCode::Char(' ')]);
    let all: Vec<String> = items(&app).into_iter().map(|(l, _)| l).collect();
    assert!(!all.iter().any(|l| l.starts_with("Undo") || l.starts_with("Comment") || l.starts_with("Edit")), "{all:?}");
    keys(&mut app, &[KeyCode::Esc]);
    assert_eq!(std::fs::read(&db).unwrap(), before);
    // ← back to the history, on the same record; esc leaves it.
    keys(&mut app, &[KeyCode::Left]);
    assert_eq!(app.hist_focus, stacked::tui::HistFocus::List);
    assert!(app.history()[app.pick].text.contains("· reword "));
    keys(&mut app, &[KeyCode::Esc]);
    assert_eq!(app.screen, Screen::Main);
}

#[test]
fn the_history_draws_the_branches_of_the_tree() {
    let (_tmp, db, _src) = setup();
    // A move, undone, then a drop made from where the move was: two
    // branches off the import.
    Db::edit(&db, |d| stacked::ops::move_patch(d, "s", 1, 0).map(|_| ())).unwrap();
    Db::edit(&db, |d| d.undo("s").map(|_| ())).unwrap();
    Db::edit(&db, |d| stacked::ops::drop_patches(d, "s", &[1]).map(|_| ())).unwrap();
    // A comment on the drop, drawn under it, the lanes going on beside it.
    let rev = Db::load(&db).unwrap().head("s").unwrap().0;
    Db::edit(&db, |d| {
        d.set_note(Note {
            change: stack_change("s"),
            id: "h1".into(),
            text: "why?".into(),
            preview: "@x".into(),
            about: Some(rev),
            ..Default::default()
        })
    })
    .unwrap();
    let mut app = open(&db).unwrap();
    draw(&mut app);
    keys(&mut app, &[KeyCode::Home]);
    choose(&mut app, "History…");
    let screen = history_pane(&draw(&mut app));
    let line = |what: &str| screen.lines().find(|l| l.contains(what)).unwrap_or_else(|| panic!("no {what:?}:\n{screen}")).to_string();
    // As the tree draws them: the drop on the stack's lane, filled, it
    // being where the stack is; the move on a lane of its own; both joining
    // the import.
    assert!(line("· drop ").starts_with(" ●   "), "{screen}");
    assert!(line("· undo ").starts_with(" │   "), "{screen}");
    assert!(line("· move ").starts_with(" │ ○ "), "{screen}");
    assert!(line("· import ").starts_with(" ○ ┘ "), "{screen}");
    assert!(line("└ you: why?").starts_with(" │      └"), "{screen}");
    // The times line up past the lanes.
    let at = |what: &str| line(what).split("20").next().unwrap().chars().count();
    assert_eq!(at("· drop "), at("· import "));
    // The tree draws them the same.
    keys(&mut app, &[KeyCode::Esc]);
    choose(&mut app, "Tree of edits…");
    let tree = draw(&mut app);
    assert!(tree.contains(" ● @") && tree.contains(" │ ○ @") && tree.contains(" ○ ┘ @"), "{tree}");
}

#[test]
fn the_agent_is_asked_about_the_history_to_answer_only() {
    let (_tmp, db, _src) = setup();
    fake_claude();
    let mut app = open(&db).unwrap();
    draw(&mut app);
    choose(&mut app, "Drop this patch");
    choose(&mut app, "Drop it");
    keys(&mut app, &[KeyCode::Home]);
    choose(&mut app, "History…");
    let rev = app.history()[0].rev;
    // A question on the drop, written under it, asked to answer only.
    choose(&mut app, "Ask agent about this, answer only…");
    let screen = history_pane(&draw(&mut app));
    let at = screen.lines().position(|l| l.contains(&format!("@{rev} −1 · drop"))).unwrap();
    assert!(screen.lines().nth(at + 1).unwrap().contains(&format!("a question on @{rev}")), "under it:\n{screen}");
    typed(&mut app, "why drop it?");
    keys(&mut app, &[KeyCode::Esc]);
    assert_eq!(items(&app)[0].0, "Save it and ask agent, answer only");
    assert!(!items(&app).iter().any(|(l, _)| l == "Save it and ask agent"), "it changes nothing");
    choose(&mut app, "Save it and ask agent, answer only");
    assert_eq!(app.screen, Screen::History);
    let d = Db::load(&db).unwrap();
    let n = d.notes(&stack_change("s")).into_iter().find(|n| n.about == Some(rev)).cloned().unwrap();
    assert_eq!(n.text, "why drop it?");
    drop(d);
    // Its answer shows under the record.
    let screen = wait_for(&mut app, "└ agent: Seen.");
    assert!(screen.contains(" │    └ agent: Seen. · agent replied"), "under the record, the lanes beside it:\n{screen}");
    // Among the stack's comments it says what it is about, and is asked
    // about only to answer.
    choose(&mut app, "Go to its comments");
    assert!(app.on_stack && app.screen == Screen::Main);
    let screen = draw(&mut app);
    assert!(screen.contains(&format!("about @{rev} −1 · drop")) && screen.contains("history @"), "{screen}");
    keys(&mut app, &[KeyCode::Char(' ')]);
    let all: Vec<String> = items(&app).into_iter().map(|(l, _)| l).collect();
    assert!(all.contains(&"Ask agent, answer only".to_string()) && !all.contains(&"Ask agent".to_string()), "{all:?}");
    keys(&mut app, &[KeyCode::Esc]);
    choose(&mut app, &format!("Go to @{rev} in the history"));
    assert_eq!(app.screen, Screen::History);
    assert_eq!(app.history()[app.pick].rev, rev);
}

#[test]
fn a_new_database_starts_where_the_history_is_picked() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    draw(&mut app);
    choose(&mut app, "Drop this patch");
    choose(&mut app, "Drop it");
    keys(&mut app, &[KeyCode::Home]);
    choose(&mut app, "History…");
    // The newest is the drop: a new database from there has it, and before
    // it, its start.
    let rev = app.history()[0].rev;
    choose(&mut app, "Start a new database here…");
    let all = items(&app);
    assert_eq!((all[0].0.as_str(), all[1].0.as_str()), ("Write it", "Cancel"), "{all:?}");
    let before = std::fs::read(&db).unwrap();
    choose(&mut app, "Write it");
    app.settle();
    let new = db.with_file_name(format!("db-from-{rev}.jsonl"));
    assert!(app.status.contains(&format!("wrote {}", new.display())), "{}", app.status);
    assert_eq!(std::fs::read(&db).unwrap(), before, "this one is not changed");
    let made = Db::load(&new).unwrap();
    assert_eq!(made.stack("s").unwrap().patches, Db::load(&db).unwrap().stack("s").unwrap().patches);
    assert_eq!(made.tree("s").len(), 2, "its start, then the drop");
    // Again: another name.
    choose(&mut app, "Start a new database here…");
    choose(&mut app, "Write it");
    app.settle();
    assert!(db.with_file_name(format!("db-from-{rev}-2.jsonl")).exists(), "{}", app.status);
}

#[test]
fn a_comment_moved_to_the_stack_takes_the_cursor_with_it() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "rename two everywhere");
    keys(&mut app, &[KeyCode::Down]);
    assert!(app.rows[app.cursor].head);
    // The agent moves it to the stack, as it does what needs more than the
    // patch: the cursor goes with it, and says what to do there.
    let n = notes_of(&db, "s", 0).remove(0);
    Db::edit(&db, |d| stacked::ask::promote(d, "s", &n, "patch 2 says two too")).unwrap();
    app.refresh();
    let screen = draw(&mut app);
    assert!(app.on_stack && app.rows[app.cursor].head, "on it:\n{screen}");
    assert!(screen.contains("moved from patch 1 · a.txt: TWO"), "{screen}");
    assert!(
        app.status
            .starts_with("moved to the stack's comments, the cursor with it: there, Ask agent may change the whole stack · patch 2 says two too"),
        "{}",
        app.status
    );
}

#[test]
fn a_comment_moved_to_the_stack_shows_where_it_came_from() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "rename two everywhere");
    let n = notes_of(&db, "s", 0).remove(0);
    Db::edit(&db, |d| stacked::ask::promote(d, "s", &n, "patch 2 says two too")).unwrap();
    app.refresh();
    assert!(notes_of(&db, "s", 0).is_empty());
    keys(&mut app, &[KeyCode::Left, KeyCode::Home, KeyCode::Right]);
    down_to(&mut app, "   └ comment");
    let screen = draw(&mut app);
    assert!(screen.contains("moved from patch 1 · a.txt: TWO"), "{screen}");
    assert!(screen.contains("This needs more than the patch: patch 2 says two"), "{screen}");
    // It waits to be sent again; its menu goes to where it came from, or
    // puts it back.
    keys(&mut app, &[KeyCode::Char(' ')]);
    let all = items(&app);
    assert!(all.contains(&("Ask agent".to_string(), true)), "{all:?}");
    keys(&mut app, &[KeyCode::Esc]);
    choose(&mut app, "Go to where it came from");
    assert!(!app.on_stack && app.sel == 0);
    draw(&mut app);
    assert_eq!(app.rows[app.cursor].text, "+TWO");
    keys(&mut app, &[KeyCode::Left, KeyCode::Home, KeyCode::Right]);
    down_to(&mut app, "   └ comment");
    choose(&mut app, "Move it back to patch 1");
    let back = notes_of(&db, "s", 0);
    assert_eq!(back.len(), 1, "{back:?}");
    assert!(back[0].moved.is_none() && back[0].text.contains("rename two everywhere"));
}

#[test]
fn an_answer_opens_comments_of_its_own() {
    let (_tmp, db, _src) = setup();
    let d = Db::load(&db).unwrap();
    let s = d.stack("s").unwrap().clone();
    let p1 = d.patch(&s.patches[0]).clone();
    let p2 = d.patch(&s.patches[1]).clone();
    drop(d);
    let key_of = |p: &stacked::db::Patch, file: &str, text: &str| {
        stacked::view::find_line(&stacked::view::note_lines(p), file, '+', text).unwrap()
    };
    let (k1, k2) = (key_of(&p1, "a.txt", "TWO"), key_of(&p2, "b.txt", "bee"));
    Db::edit(&db, |d| {
        d.set_note(Note {
            change: stack_change("s"),
            id: "s1".into(),
            text: append_turn("Review it.", Role::Agent, "Two things.", Some("answer")),
            ..Default::default()
        })?;
        for (p, k, file, text) in [(&p1, &k1, "a.txt", "Why shout?"), (&p2, &k2, "b.txt", "Say what b is.")] {
            d.set_note(Note {
                change: p.change.clone(),
                id: format!("claude:{k}"),
                line: k.clone(),
                text: format!("```claude answer\n{text}\n```"),
                file: file.into(),
                preview: "x".into(),
                by: Some("claude".into()),
                opened_by: Some((stack_change("s"), "s1".into())),
                ..Default::default()
            })?;
        }
        let n = d.notes(&p2.change)[0].clone();
        d.set_note(Note {
            text: append_turn(&n.text, Role::You, "It is for bees.", None),
            ..n
        })
    })
    .unwrap();
    let mut app = open(&db).unwrap();
    draw(&mut app);
    keys(&mut app, &[KeyCode::Home, KeyCode::Right]);
    down_to(&mut app, "   └ comment");
    let screen = draw(&mut app);
    // Where it stands, under it when too long for its first line.
    assert!(screen.contains("     agent replied (answer only)\n") || screen.contains("     agent replied (answer only) "), "{screen}");
    assert!(screen.contains("     opened 2 comments, 1 with your replies"), "{screen}");
    choose(&mut app, "Go to the comments it opened");
    draw(&mut app);
    assert!(!app.on_stack && app.sel == 0, "the first, on patch 1");
    assert!(app.rows[app.cursor].head && app.rows[app.cursor].text.contains("agent's comment"));
    keys(&mut app, &[KeyCode::Left, KeyCode::Home, KeyCode::Right]);
    down_to(&mut app, "   └ comment");
    choose(&mut app, "Delete it and the comments it opened…");
    assert_eq!(items(&app)[1].0, "Keep them");
    choose(&mut app, "Delete them");
    assert!(Db::load(&db).unwrap().stack_notes("s").is_empty());
}

#[test]
fn picking_from_the_tree_and_the_stacks() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    draw(&mut app);
    choose(&mut app, "Drop this patch");
    choose(&mut app, "Drop it");
    keys(&mut app, &[KeyCode::Home]);
    choose(&mut app, "Tree of edits");
    assert_eq!(app.screen, Screen::Tree);
    let screen = draw(&mut app);
    assert!(screen.contains("● @") && screen.contains("drop "), "{screen}");
    assert!(screen.contains("   − Bug 1 - shout two, was 1"), "what the edit changed:\n{screen}");
    // The stack before the drop, its patches on the right.
    keys(&mut app, &[KeyCode::Down]);
    let screen = draw(&mut app);
    assert!(screen.contains("import 2 patches") && screen.contains(" 2 Bug 2 - add b"), "{screen}");
    keys(&mut app, &[KeyCode::Enter]);
    assert_eq!(app.screen, Screen::Main);
    assert!(draw(&mut app).contains("  1 Bug 1 - shout two"));

    choose(&mut app, "Switch stack");
    let screen = draw(&mut app);
    assert!(screen.contains(" s (2 patches)") && screen.contains(" t (1 patches)"), "{screen}");
    keys(&mut app, &[KeyCode::Down, KeyCode::Enter]);
    assert_eq!(app.stack, "t");
    assert!(draw(&mut app).contains("  1 Bug 2 - add b"));
}

#[test]
fn runs_and_their_transcripts() {
    let (_tmp, db, _src) = setup();
    stacked::db::Db::edit(&db, |d| {
        d.add_run(stacked::db::Run {
            id: "r1".into(),
            what: "answer".into(),
            stack: "s".into(),
            state: stacked::db::RunState::Done,
            summary: "claude answered 1 comment".into(),
            output: "All good.".into(),
            ..Default::default()
        })?;
        d.add_event("r1", serde_json::json!({"type": "system", "subtype": "init", "model": "m"}))
    })
    .unwrap();
    let mut app = open(&db).unwrap();
    draw(&mut app);
    choose(&mut app, "Runs");
    let screen = draw(&mut app);
    assert!(screen.contains("done    answer s · claude answered 1 comment"), "{screen}");
    keys(&mut app, &[KeyCode::Enter]);
    let screen = draw(&mut app);
    assert!(screen.contains("● session started · m") && screen.contains("All good."), "{screen}");
    keys(&mut app, &[KeyCode::Esc, KeyCode::Esc]);
    assert_eq!(app.screen, Screen::Main);
}

#[test]
fn a_run_landing_shows_what_came_of_it() {
    let (_tmp, db, _src) = setup();
    let run = stacked::db::Run {
        id: "r1".into(),
        what: "change".into(),
        stack: "s".into(),
        state: stacked::db::RunState::Started,
        ..Default::default()
    };
    Db::edit(&db, |d| d.add_run(run.clone())).unwrap();
    let mut app = open(&db).unwrap();
    draw(&mut app);
    // Another process lands it: the editor sees the file grow.
    Db::edit(&db, |d| {
        d.add_run(stacked::db::Run {
            state: stacked::db::RunState::Done,
            summary: "claude changed the stack, @9".into(),
            ..run
        })
    })
    .unwrap();
    app.refresh();
    assert!(draw(&mut app).contains("the agent's change done: claude changed the stack, @9"));
}

#[test]
fn a_stack_that_cannot_be_checked_is_read_only() {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("db.jsonl");
    // As a stacked from before the check wrote it: no base.
    let patches: Vec<stacked::db::Patch> = ["one", "two"]
        .iter()
        .map(|f| {
            stacked::ops::new_patch(stacked::mbox::Draft {
                message: format!("Bug - add {f}"),
                diff: format!("diff --git a/{f} b/{f}\nnew file mode 100644\n--- /dev/null\n+++ b/{f}\n@@ -0,0 +1 @@\n+{f}\n"),
                author: Some("A <a@b>".into()),
                ..Default::default()
            })
        })
        .collect();
    let mut recs: Vec<stacked::db::Record> = patches.iter().cloned().map(stacked::db::Record::Patch).collect();
    recs.push(stacked::db::Record::Stack(stacked::db::Stack {
        name: "s".into(),
        patches: patches.iter().map(|p| p.id.clone()).collect(),
        base: None,
        repo: None,
        parent: None,
        op: "from before".into(),
        time: 0,
    }));
    Db::append(&db, &recs).unwrap();
    let before = std::fs::read(&db).unwrap();
    let mut app = open(&db).unwrap();
    assert!(app.read_only().is_some());
    assert!(draw(&mut app).contains("· read-only: it has no base commit"));
    // What would change the stack is offered, but cannot be chosen, and the
    // menu says why.
    keys(&mut app, &[KeyCode::Char(' ')]);
    let all = items(&app);
    for label in ["Move this patch…", "Edit this patch in $EDITOR", "Drop this patch…"] {
        assert!(all.iter().any(|(l, ok)| l == label && !ok), "{label}: {all:?}");
    }
    assert!(draw(&mut app).contains("read-only: it has no base commit"));
    keys(&mut app, &[KeyCode::Esc, KeyCode::Home, KeyCode::Char(' ')]);
    let all = items(&app);
    assert!(all.iter().any(|(l, ok)| l == "Delete this stack…" && !ok), "{all:?}");
    keys(&mut app, &[KeyCode::Esc, KeyCode::Down]);
    assert_eq!(std::fs::read(&db).unwrap(), before);
    // Comments still go in, but are not sent.
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+one");
    keys(&mut app, &[KeyCode::Enter]);
    typed(&mut app, "still fine");
    keys(&mut app, &[KeyCode::Esc]);
    assert!(items(&app).contains(&("Save it and ask agent".to_string(), false)));
    choose(&mut app, "Save it");
    assert!(draw(&mut app).contains("still fine"));
}

#[test]
fn an_empty_database() {
    let tmp = tempfile::tempdir().unwrap();
    let mut app = open(&tmp.path().join("none.jsonl")).unwrap();
    let screen = draw(&mut app);
    assert!(screen.contains("no stack yet") && screen.contains("import NAME main..HEAD"), "{screen}");
    for c in [KeyCode::Down, KeyCode::Up, KeyCode::Left, KeyCode::Right, KeyCode::Home, KeyCode::End] {
        assert_eq!(key(&mut app, c), Effect::None);
        draw(&mut app);
    }
    // Enter, with nothing else to do, is the menu: only to quit.
    keys(&mut app, &[KeyCode::Enter]);
    assert_eq!(items(&app), vec![("Quit".to_string(), true)]);
    assert_eq!(key(&mut app, KeyCode::Enter), Effect::Quit);
}
