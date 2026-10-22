use std::path::{Path, PathBuf};
use std::process::Command;

use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;

use stacked::db::{Db, Note, stack_change};
use stacked::thread::{Role, append_turn};
use stacked::tui::{App, Effect, Focus, Layout, Mode, Pane, Screen, Show};

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
    // No agent of the user's is ever asked: the tests' stand-in is.
    fake_claude();
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

/// Waits for what the agent brings back to land, then accepts all of it,
/// as a review accepting everything does.
fn accept_landed(app: &mut App, db: &Path) {
    for _ in 0..300 {
        app.refresh();
        let d = Db::load(db).unwrap();
        let going = d.runs().iter().any(|r| r.state == stacked::db::RunState::Started);
        if !going && !stacked::held::queue(&d, "s").is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    Db::edit(db, |d| {
        let ids: Vec<String> = stacked::held::queue(d, "s").into_iter().map(|r| r.id.clone()).collect();
        for id in ids {
            stacked::held::close(d, &id, &Default::default(), Some(true))?;
        }
        Ok(())
    })
    .unwrap();
    app.refresh();
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
    let (x, y) = at(&draw(&mut app), "[ ] in $EDITOR");
    app.hover(x, y);
    match &app.mode {
        Mode::Menu(m) => assert_eq!(m.items[m.sel].label, "Comment on this line"),
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
    let (x, y) = at(&screen, "Comment on this line");
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

/// As `choose`, the item's choice drawn as `pick` (or starting so)
/// picked first, with →.
fn choose_as(app: &mut App, label: &str, pick: &str) -> Effect {
    if !matches!(app.mode, Mode::Menu(_)) {
        assert_eq!(key(app, KeyCode::Char(' ')), Effect::None);
    }
    let all = items(app);
    let want = all
        .iter()
        .position(|(l, _)| l.starts_with(label))
        .unwrap_or_else(|| panic!("no {label:?} in {all:?}"));
    for _ in 0..all.len() * 2 {
        match &app.mode {
            Mode::Menu(m) if m.sel == want => break,
            _ => {
                key(app, KeyCode::Down);
            }
        }
    }
    let list = choices_of(app, want);
    assert!(list.iter().any(|c| c.starts_with(pick)), "no {pick:?} among {label:?}'s {list:?}");
    for _ in 0..list.len() {
        let shown = match &app.mode {
            Mode::Menu(m) => m.items[want].choices.as_ref().map(|c| c.list[c.picked].shown.clone()),
            _ => None,
        };
        if shown.is_some_and(|s| s.starts_with(pick)) {
            break;
        }
        key(app, KeyCode::Right);
    }
    key(app, KeyCode::Enter)
}

/// The choices of the open menu's item `i`, as drawn.
fn choices_of(app: &App, i: usize) -> Vec<String> {
    match &app.mode {
        Mode::Menu(m) => m.items[i].choices.as_ref().map(|c| c.list.iter().map(|c| c.shown.clone()).collect()).unwrap_or_default(),
        _ => vec![],
    }
}

/// The choices of the open menu's item `label`, as drawn.
fn choices_named(app: &App, label: &str) -> Vec<String> {
    let at = items(app).iter().position(|(l, _)| l == label).unwrap_or_else(|| panic!("no {label:?} in {:?}", items(app)));
    choices_of(app, at)
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
    assert!(stack.contains(&"Ask agent about all".to_string()), "{stack:?}");
    assert!(!stack.contains(&"Move this patch…".to_string()), "{stack:?}");
    // A line's.
    keys(&mut app, &[KeyCode::Down, KeyCode::Right]);
    down_to(&mut app, "+TWO");
    let line = labels(&mut app);
    assert_eq!(line[0], "Comment on this line", "{line:?}");
    keys(&mut app, &[KeyCode::Char(' ')]);
    assert_eq!(choices_named(&app, "Comment on this line"), ["here", "in $EDITOR"]);
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
    keys(&mut app, &[KeyCode::Char('?'), KeyCode::Down]);
    key(&mut app, KeyCode::Char('?'));
    assert!(draw(&mut app).contains("Press Enter to reply to this comment."));
    keys(&mut app, &[KeyCode::Char('?')]);
    assert!(matches!(app.mode, Mode::Menu(_)), "back in the menu");
}

#[test]
fn left_on_the_patches_cycles_them_compact_hidden_expanded() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    let screen = draw(&mut app);
    assert!(app.list == Pane::Expanded && screen.contains("  2 Bug 2 - add b"), "expanded at the start:\n{screen}");
    // Compact: each patch's number and the lines it adds and removes.
    keys(&mut app, &[KeyCode::Left]);
    assert!(app.list == Pane::Compact && app.focus == Focus::List);
    let screen = draw(&mut app);
    assert!(screen.contains("\n  1 +2 −2 ") && screen.contains("\n  2 +1 −0 "), "{screen}");
    assert!(!screen.contains("Bug 2 - add b"), "no title:\n{screen}");
    // Hidden: the middle has the width.
    keys(&mut app, &[KeyCode::Left]);
    assert!(app.list == Pane::Hidden && app.focus == Focus::Diff);
    let screen = draw(&mut app);
    assert!(!screen.contains("Bug 2 - add b"), "the patches are gone:\n{screen}");
    assert!(screen.lines().nth(1).unwrap().starts_with("✉ message"), "the patch has the width:\n{screen}");
    assert!(screen.contains("patch 1 of 2"), "{screen}");
    // Expanded again, the cursor in it.
    keys(&mut app, &[KeyCode::Left]);
    assert!(app.list == Pane::Expanded && app.focus == Focus::List);
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
    // Further right than the comments: a line each, compact; then they
    // make way for the patch.
    let shown = |app: &mut App| draw(app).matches("why so loud?").count();
    assert_eq!(shown(&mut app), 2, "in its thread, and in the pane");
    keys(&mut app, &[KeyCode::Right]);
    assert!(app.comments == Pane::Compact && app.focus == Focus::Notes);
    assert!(draw(&mut app).contains("you · a.txt: TWO"));
    assert_eq!(shown(&mut app), 1, "in its thread alone");
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
    assert!(app.notes_shown() && app.comments == Pane::Expanded && app.focus == Focus::Notes);
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
        ["Save it", "Send it", "Save it and ask agent", "Attach a reference…", "", "Throw it away", "Keep writing"]
    );
    // When, a choice of the row above the ask: now, or later, in the batch.
    assert_eq!(choices_named(&app, "Send it"), ["now", "later"]);
    // How it is asked, a choice of its item; a line's comment, of a patch,
    // may be asked of the whole stack.
    assert_eq!(choices_named(&app, "Save it and ask agent"), ["change", "the whole stack", "answer only"]);
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
    // (Under the top bar, the banner: the comments open.)
    keys(&mut app, &[KeyCode::Left]);
    choose(&mut app, "Comment on this patch…");
    let screen = draw(&mut app);
    assert!(screen.lines().nth(1).unwrap().contains("○ 1 open comment"), "{screen}");
    assert_eq!(screen.lines().nth(2).unwrap().chars().nth(60), Some('│'), "half the width:\n{screen}");
    assert!(screen.lines().nth(4).unwrap().contains("a comment on the patch"), "under the patch:\n{screen}");
    save(&mut app, "split this");
    let n = notes_of(&db, "s", 0).into_iter().find(|n| n.text == "split this").unwrap();
    assert_eq!(n.file, "✉ message");
    // And one on the stack, under the header.
    keys(&mut app, &[KeyCode::Up]);
    choose(&mut app, "Comment on the stack…");
    assert!(draw(&mut app).lines().nth(3).unwrap().contains("a comment on the stack"));
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
    assert_eq!(choose(&mut app, "Edit a.txt here in $EDITOR"), Effect::EditCode { file: "a.txt".into(), line: 2, again: None });
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
    assert_eq!(choose(&mut app, "Edit a.txt here"), Effect::EditCode { file: "a.txt".into(), line: 39, again: None });
    keys(&mut app, &[KeyCode::Left]);
    assert!(matches!(choose(&mut app, "Edit this patch in $EDITOR"), Effect::EditPatch { line: Some(_) }));
}

#[test]
fn references_attached_to_a_comment_are_named_in_it_and_asking_waits_for_what_it_names() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    // A comment to point at.
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "why so loud?");
    // Another, naming references not attached yet.
    down_to(&mut app, "+LINE 39");
    keys(&mut app, &[KeyCode::Enter]);
    typed(&mut app, "Same as @r1, and @r2?");
    keys(&mut app, &[KeyCode::Esc]);
    // The agent is not asked while they name nothing, and the menu says so.
    let Mode::Menu(m) = &app.mode else { panic!("{:?}", app.mode) };
    assert_eq!(m.note.as_deref(), Some("@r1, @r2 names no reference attached: attach one, or write it otherwise, to ask the agent"));
    assert!(items(&app).iter().any(|(l, ok)| l == "Save it and ask agent" && !ok), "{:?}", items(&app));
    // Attached: the comment on TWO.
    choose(&mut app, "Attach a reference…");
    assert_eq!(items(&app)[0].0, "Lines of a file…");
    choose(&mut app, "patch 1 · a.txt: TWO");
    assert!(matches!(app.mode, Mode::Edit(_)), "{:?}", app.mode);
    assert_eq!(app.status, "attached as r1: write @r1 where you mean it");
    let screen = draw(&mut app);
    assert!(screen.contains("r1 ↗ patch 1 · a.txt: TWO · \"why so loud?\""), "{screen}");
    let Mode::Edit(e) = &app.mode else { panic!("{:?}", app.mode) };
    assert_eq!(e.ed.broken(), ["@r2"]);
    // Drawn: @r1 as a link, @r2 as broken, in another colour.
    let mut buf = Buffer::empty(Rect::new(0, 0, 120, 40));
    app.draw(&mut buf);
    let cell = |text: &str| {
        (0..40u16)
            .find_map(|y| {
                let line: String = (0..120u16).map(|x| buf[(x, y)].symbol()).collect();
                line.find(text).map(|k| buf[(line[..k].chars().count() as u16, y)].style())
            })
            .unwrap_or_else(|| panic!("no {text:?}"))
    };
    let (named, broken) = (cell("@r1,"), cell("@r2?"));
    let underlined = ratatui::style::Modifier::UNDERLINED;
    assert!(named.add_modifier.contains(underlined) && broken.add_modifier.contains(underlined), "{named:?} {broken:?}");
    assert_ne!(named.fg, broken.fg, "{named:?} {broken:?}");
    // Lines of a file: the comment's own file, as its patch leaves it, to
    // say which lines; the file has them, or it says so.
    keys(&mut app, &[KeyCode::Esc]);
    choose(&mut app, "Attach a reference…");
    choose(&mut app, "Lines of a file…");
    let Mode::Lines(l) = &app.mode else { panic!("{:?}", app.mode) };
    assert_eq!(l.ed.text(), "a.txt: @1");
    assert_eq!(app.hint(), "enter: attach these lines · esc: back to the comment");
    typed(&mut app, "900");
    keys(&mut app, &[KeyCode::Enter]);
    assert_eq!(app.status, "a.txt has 43 lines as patch 1 leaves it");
    assert!(matches!(app.mode, Mode::Lines(_)));
    for _ in 0..3 {
        key(&mut app, KeyCode::Backspace);
    }
    typed(&mut app, "2-3");
    keys(&mut app, &[KeyCode::Enter]);
    assert_eq!(app.status, "attached as r2: write @r2 where you mean it");
    let Mode::Edit(e) = &app.mode else { panic!("{:?}", app.mode) };
    assert!(e.ed.broken().is_empty());
    keys(&mut app, &[KeyCode::Esc]);
    assert!(items(&app).iter().any(|(l, ok)| l == "Save it and ask agent" && *ok), "{:?}", items(&app));
    choose(&mut app, "Save it");
    // Kept with the comment, by their numbers.
    let a = notes_of(&db, "s", 0).into_iter().find(|n| n.text == "why so loud?").unwrap();
    let b = notes_of(&db, "s", 0).into_iter().find(|n| n.text == "Same as @r1, and @r2?").unwrap();
    assert_eq!(b.attached.iter().map(|r| r.name()).collect::<Vec<_>>(), ["r1", "r2"]);
    assert_eq!(b.attached[1].to, stacked::refs::To::Lines("a.txt:2-3 @1".into()));
    // Under it, a line each; Enter on the comment's goes to that comment.
    let screen = draw(&mut app);
    assert!(screen.contains("r1 ↗ patch 1 · a.txt: TWO · \"why so loud?\""), "{screen}");
    assert!(screen.contains("r2 ⇢ a.txt:2-3 · as patch 1 leaves it"), "{screen}");
    down_to(&mut app, "     ▎   r1 ↗");
    keys(&mut app, &[KeyCode::Enter]);
    draw(&mut app);
    assert_eq!(app.rows[app.cursor].note.as_deref(), Some(a.id.as_str()), "{}", app.rows[app.cursor].text);
    // Closed, it stays attached, said so; Enter reads it as it was.
    Db::edit(&db, |d| d.set_note(Note { text: String::new(), ..a.clone() })).unwrap();
    app.refresh();
    draw(&mut app);
    let said: Vec<&str> = app.rows.iter().map(|r| r.text.as_str()).filter(|t| t.contains(" ↗ ")).collect();
    assert_eq!(said, ["     ▎   r1 ↗ patch 1 · a.txt: TWO · \"why so loud?\" · closed"]);
    down_to(&mut app, "     ▎   r1 ↗");
    keys(&mut app, &[KeyCode::Enter]);
    assert_eq!(app.screen, Screen::Text);
    let t = app.text.as_ref().unwrap();
    assert!(t.title.starts_with("a closed comment · patch 1 · a.txt: TWO"), "{}", t.title);
    assert!(t.lines.iter().any(|(l, _)| l.contains("why so loud?")), "{:?}", t.lines);
    keys(&mut app, &[KeyCode::Esc, KeyCode::Home]);
    // Detached, the mention of it names nothing: the agent is not asked.
    down_to(&mut app, "+LINE 39");
    keys(&mut app, &[KeyCode::Enter]);
    keys(&mut app, &[KeyCode::Esc]);
    choose(&mut app, "Detach a reference…");
    choose(&mut app, "r1 ↗");
    assert_eq!(app.status, "r1 is detached");
    let Mode::Edit(e) = &app.mode else { panic!("{:?}", app.mode) };
    assert_eq!(e.ed.broken(), ["@r1"]);
    keys(&mut app, &[KeyCode::Esc]);
    assert!(items(&app).iter().any(|(l, ok)| l == "Save it and ask agent" && !ok), "{:?}", items(&app));
    choose(&mut app, "Throw the changes away");
    assert_eq!(notes_of(&db, "s", 0).into_iter().find(|n| n.id == b.id).unwrap().attached.len(), 2);
}

#[test]
fn mentions_in_a_thread_are_links_to_what_they_name_the_agents_numbered_too() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "why so loud?");
    down_to(&mut app, "+LINE 39");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "Same as @r1?");
    let a = notes_of(&db, "s", 0).into_iter().find(|n| n.text == "why so loud?").unwrap();
    let b = notes_of(&db, "s", 0).into_iter().find(|n| n.text == "Same as @r1?").unwrap();
    // r1 attached; the agent answers, attaching r2, naming it and r9;
    // you answer naming both.
    Db::edit(&db, |d| {
        let said = "As @r2 shows, not @r9.\n\n```refs\nr2 a.txt:2 @1\n```";
        let text = append_turn(&append_turn(&b.text, Role::Agent, said, Some("answer")), Role::You, "Compare @r{1,2}.", None);
        d.set_note(Note {
            text,
            attached: vec![stacked::refs::Attached {
                n: 1,
                to: stacked::refs::To::Comment {
                    change: a.change.clone(),
                    id: a.id.clone(),
                },
            }],
            ..b.clone()
        })
    })
    .unwrap();
    app.refresh();
    draw(&mut app);
    let row = |app: &App, text: &str| app.rows.iter().find(|r| r.text.contains(text)).cloned().unwrap_or_else(|| panic!("no {text:?}"));
    assert_eq!(row(&app, "Same as @r1?").names, [1]);
    assert_eq!(row(&app, "As @r2 shows").names, [2], "r9 names nothing");
    assert_eq!(row(&app, "Compare @r{1,2}.").names, [1, 2]);
    let under: Vec<String> = app.rows.iter().filter(|r| r.text.contains(" ↗ ") || r.text.contains(" ⇢ ")).map(|r| r.text.trim().to_string()).collect();
    assert_eq!(under, ["▎   r1 ↗ patch 1 · a.txt: TWO · \"why so loud?\"", "▎   r2 ⇢ a.txt:2 · as patch 1 leaves it"]);
    // Drawn: what holds as a link, what does not as broken.
    let mut buf = Buffer::empty(Rect::new(0, 0, 160, 60));
    app.draw(&mut buf);
    let cell = |text: &str| {
        (0..60u16)
            .find_map(|y| {
                let line: String = (0..160u16).map(|x| buf[(x, y)].symbol()).collect();
                line.find(text).map(|k| buf[(line[..k].chars().count() as u16, y)].style())
            })
            .unwrap_or_else(|| panic!("no {text:?}"))
    };
    let (named, broken) = (cell("@r2 shows"), cell("@r9."));
    let underlined = ratatui::style::Modifier::UNDERLINED;
    assert!(named.add_modifier.contains(underlined) && broken.add_modifier.contains(underlined), "{named:?} {broken:?}");
    assert_ne!(named.fg, broken.fg, "{named:?} {broken:?}");
    // Enter on a line naming one: there; the agent's lines, previewed.
    down_to(&mut app, "     ▎   Same as @r1?");
    keys(&mut app, &[KeyCode::Enter]);
    draw(&mut app);
    assert_eq!(app.rows[app.cursor].note.as_deref(), Some(a.id.as_str()), "{}", app.rows[app.cursor].text);
    down_to(&mut app, "     ▎   As @r2 shows");
    keys(&mut app, &[KeyCode::Enter]);
    let Mode::Preview(p) = &app.mode else { panic!("{:?}", app.mode) };
    assert_eq!(p.title, "a.txt:2 · as patch 1 leaves it");
    keys(&mut app, &[KeyCode::Esc]);
    // Naming two: which.
    down_to(&mut app, "     ▎   Compare @r{1,2}.");
    keys(&mut app, &[KeyCode::Enter]);
    let Mode::Menu(m) = &app.mode else { panic!("{:?}", app.mode) };
    assert_eq!(m.title, "the references it names");
    assert_eq!(items(&app).into_iter().map(|(l, _)| l).collect::<Vec<_>>(), ["r1 ↗ patch 1 · a.txt: TWO · \"why so loud?\"", "r2 ⇢ a.txt:2 · as patch 1 leaves it"]);
    keys(&mut app, &[KeyCode::Esc]);
    // Written on again: the agent's is there to name, and the next attached
    // is r3.
    keys(&mut app, &[KeyCode::Home]);
    down_to(&mut app, "+LINE 39");
    keys(&mut app, &[KeyCode::Enter]);
    let Mode::Edit(e) = &app.mode else { panic!("{:?}", app.mode) };
    assert!(e.ed.broken().is_empty(), "{:?}", e.ed.broken());
    // Its references under the editor, as the comment's lines: the agent's
    // among them.
    let screen = draw(&mut app);
    assert!(screen.contains("▎   r2 ⇢ a.txt:2 · as patch 1 leaves it") && !screen.contains("(the agent's)"), "{screen}");
    keys(&mut app, &[KeyCode::Esc]);
    choose(&mut app, "Attach a reference…");
    choose(&mut app, "Lines of a file…");
    typed(&mut app, "a.txt:3 @1");
    keys(&mut app, &[KeyCode::Enter]);
    assert_eq!(app.status, "attached as r3: write @r3 where you mean it");
}

#[test]
fn what_is_written_in_editor_is_in_its_place_in_the_conversation_its_references_under_it() {
    use std::cell::RefCell;
    use std::rc::Rc;
    use stacked::tui::Target;
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "why so loud?");
    down_to(&mut app, "+LINE 39");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "Same as @r1?");
    let a = notes_of(&db, "s", 0).into_iter().find(|n| n.text == "why so loud?").unwrap();
    let b = notes_of(&db, "s", 0).into_iter().find(|n| n.text == "Same as @r1?").unwrap();
    // r1 attached; the agent answers, attaching r2.
    Db::edit(&db, |d| {
        let said = "As @r2 shows.\n\n```refs\nr2 a.txt:2 @1\n```";
        d.set_note(Note {
            text: append_turn(&b.text, Role::Agent, said, Some("answer")),
            attached: vec![stacked::refs::Attached {
                n: 1,
                to: stacked::refs::To::Comment {
                    change: a.change.clone(),
                    id: a.id.clone(),
                },
            }],
            ..b.clone()
        })
    })
    .unwrap();
    app.refresh();
    // $EDITOR as the tests have it: what it opens and where kept, and a
    // line written where the cursor is.
    let seen = Rc::new(RefCell::new(Vec::<(String, Option<usize>)>::new()));
    let opened = seen.clone();
    let write = |line: &'static str| -> stacked::ops::Editing {
        let opened = opened.clone();
        Box::new(move |file: &Path, at: Option<usize>| -> stacked::db::Result<()> {
            let text = std::fs::read_to_string(file).unwrap();
            opened.borrow_mut().push((text.clone(), at));
            let mut lines: Vec<&str> = text.lines().collect();
            lines[at.unwrap() - 1] = line;
            std::fs::write(file, lines.join("\n")).unwrap();
            Ok(())
        })
    };
    stacked::ops::stand_in(Some(write("Then @r2 it is.")));
    down_to(&mut app, "     ▎   As @r2 shows.");
    let e = choose_as(&mut app, "Reply", "in $EDITOR");
    assert_eq!(e, Effect::EditComment(Target::Reply { change: b.change.clone(), id: b.id.clone() }));
    app.effect(e).unwrap();
    // The conversation above, the cursor on the line after the last reply,
    // the references below: all of it dropped but what you wrote.
    let (text, at) = seen.borrow_mut().remove(0);
    assert_eq!(
        text.lines().collect::<Vec<_>>(),
        [
            "# you:",
            "#   Same as @r1?",
            "# agent:",
            "#   As @r2 shows.",
            "",
            "",
            "# The comment's references, to name as @r1, @r{1,2}, @r{2-4} or @r*:",
            "#   r1 ↗ patch 1 · a.txt: TWO · \"why so loud?\"",
            "#   r2 ⇢ a.txt:2 · as patch 1 leaves it (the agent's)",
            "#",
            "# Write your reply above these lines, under the conversation. Lines",
            "# starting with # are dropped; with nothing written, nothing is saved.",
        ]
    );
    assert_eq!(at, Some(5));
    let b = notes_of(&db, "s", 0).into_iter().find(|n| n.id == b.id).unwrap();
    assert!(b.text.ends_with("```\nThen @r2 it is."), "{}", b.text);
    // On a line with a comment, a comment written there is one more turn of
    // it: under it, the same way.
    stacked::ops::stand_in(Some(write("Still loud.")));
    keys(&mut app, &[KeyCode::Home]);
    down_to(&mut app, "+TWO");
    let e = choose_as(&mut app, "Comment on this line", "in $EDITOR");
    assert!(matches!(&e, Effect::EditComment(Target::Line { key, .. }) if *key == a.id), "{e:?}");
    app.effect(e).unwrap();
    let (text, at) = seen.borrow_mut().remove(0);
    assert!(text.starts_with("# you:\n#   why so loud?\n\n\n# Write your reply above these lines"), "{text}");
    assert_eq!(at, Some(3));
    assert_eq!(notes_of(&db, "s", 0).into_iter().find(|n| n.id == a.id).unwrap().text, "why so loud?\nStill loud.");
    // Your reply written again in $EDITOR: in its place, the cursor on it,
    // the references under it; a line of yours starting with # kept.
    stacked::ops::stand_in(Some(write("Then @r1 it is.\n# Mine, kept")));
    keys(&mut app, &[KeyCode::Home]);
    down_to(&mut app, "     ▎   Then @r2 it is.");
    let e = choose_as(&mut app, "Edit your reply", "in $EDITOR");
    assert_eq!(e, Effect::EditComment(Target::Edit { change: b.change.clone(), id: b.id.clone() }));
    app.effect(e).unwrap();
    let (text, at) = seen.borrow_mut().remove(0);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[..6], ["# you:", "#   Same as @r1?", "# agent:", "#   As @r2 shows.", "Then @r2 it is.", ""]);
    assert_eq!(lines[6], "# The comment's references, to name as @r1, @r{1,2}, @r{2-4} or @r*:");
    assert_eq!(lines[10], "# Write your reply again above these lines. The lines stacked wrote here,");
    assert_eq!(at, Some(5));
    let b = notes_of(&db, "s", 0).into_iter().find(|n| n.id == b.id).unwrap();
    assert!(b.text.ends_with("```\nThen @r1 it is.\n# Mine, kept"), "{}", b.text);
    // The agent's reply written again: under who said it, what follows it
    // under it; left as it was, nothing changes.
    stacked::ops::stand_in(Some(Box::new({
        let opened = seen.clone();
        move |file: &Path, at: Option<usize>| -> stacked::db::Result<()> {
            opened.borrow_mut().push((std::fs::read_to_string(file).unwrap(), at));
            Ok(())
        }
    })));
    keys(&mut app, &[KeyCode::Home]);
    down_to(&mut app, "     ▎   As @r2 shows.");
    let e = choose_as(&mut app, "Edit the agent's reply", "in $EDITOR");
    assert!(matches!(&e, Effect::EditComment(Target::Turn { turn: 1, .. })), "{e:?}");
    app.effect(e).unwrap();
    let (text, at) = seen.borrow_mut().remove(0);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[..5], ["# you:", "#   Same as @r1?", "# agent:", "As @r2 shows.", ""]);
    assert!(text.contains("\n# The conversation goes on after it:\n# you:\n#   Then @r1 it is.\n#   # Mine, kept\n#\n"), "{text}");
    assert_eq!(at, Some(4));
    assert_eq!(notes_of(&db, "s", 0).into_iter().find(|n| n.id == b.id).unwrap().text, b.text);
    stacked::ops::stand_in(None);
}

#[test]
fn the_editor_in_the_middle_goes_between_the_last_reply_and_the_references() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "why so loud?");
    down_to(&mut app, "+LINE 39");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "Same as @r1?");
    let a = notes_of(&db, "s", 0).into_iter().find(|n| n.text == "why so loud?").unwrap();
    let b = notes_of(&db, "s", 0).into_iter().find(|n| n.text == "Same as @r1?").unwrap();
    // r1 attached; the agent answers, attaching r2.
    Db::edit(&db, |d| {
        let said = "As @r2 shows.\n\n```refs\nr2 a.txt:2 @1\n```";
        d.set_note(Note {
            text: append_turn(&b.text, Role::Agent, said, Some("answer")),
            attached: vec![stacked::refs::Attached {
                n: 1,
                to: stacked::refs::To::Comment {
                    change: a.change.clone(),
                    id: a.id.clone(),
                },
            }],
            ..b.clone()
        })
    })
    .unwrap();
    app.refresh();
    // Where each is drawn in the middle, top to bottom: the last reply, the
    // editor's top, the references, these once on the screen.
    let order = |app: &mut App| {
        let screen = draw(app);
        for text in ["r1 ↗ patch 1 · a.txt: TWO", "r2 ⇢ a.txt:2"] {
            assert_eq!(screen.matches(text).count(), 1, "{text:?} once:\n{screen}");
        }
        let y = |text: &str| at(&screen, text).1;
        (y("▎   As @r2 shows."), y("┌─"), y("▎   r1 ↗ patch 1 · a.txt: TWO"), y("▎   r2 ⇢ a.txt:2"), screen)
    };
    down_to(&mut app, "     ▎   As @r2 shows.");
    choose_as(&mut app, "Reply", "here");
    assert!(matches!(&app.mode, Mode::Edit(e) if matches!(e.target, stacked::tui::Target::Reply { .. })), "{:?}", app.mode);
    let (reply, top, r1, r2, screen) = order(&mut app);
    assert!(reply < top && top < r1 && r1 < r2, "{screen}");
    // A reference attached meanwhile is under it too, as the editor has it;
    // thrown away, it is gone.
    typed(&mut app, "Then?");
    keys(&mut app, &[KeyCode::Esc]);
    choose(&mut app, "Attach a reference…");
    choose(&mut app, "Lines of a file…");
    typed(&mut app, "a.txt:3 @1");
    keys(&mut app, &[KeyCode::Enter]);
    assert_eq!(app.status, "attached as r3: write @r3 where you mean it");
    let (_, top, r1, _, screen) = order(&mut app);
    let r3 = at(&screen, "▎   r3 ⇢ a.txt:3 · as patch 1 leaves it").1;
    assert!(top < r1 && r1 < r3, "{screen}");
    keys(&mut app, &[KeyCode::Esc]);
    choose(&mut app, "Throw it away");
    assert!(!draw(&mut app).contains("r3 ⇢"));
    // Your comment written again: under what comes before it, the agent's
    // reply under the editor, over the references.
    keys(&mut app, &[KeyCode::Home]);
    down_to(&mut app, "     ▎   Same as @r1?");
    choose_as(&mut app, "Edit your comment", "here");
    assert!(matches!(&app.mode, Mode::Edit(e) if matches!(e.target, stacked::tui::Target::Turn { turn: 0, .. })), "{:?}", app.mode);
    let (reply, top, r1, _, screen) = order(&mut app);
    assert!(top < reply && reply < r1, "{screen}");
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
    choose(&mut app, "Close this comment");
    assert_eq!(items(&app)[0].0, "Close it");
    keys(&mut app, &[KeyCode::Enter]);
    assert!(draw(&mut app).contains("why so loud?"), "enter kept it");
    choose(&mut app, "Close this comment");
    choose(&mut app, "Close it");
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
    choose_as(&mut app, "Reply", "here");
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
    assert!(all.contains(&"Ask agent about all".to_string()) && all.contains(&"Show…".to_string()), "{all:?}");
    assert_eq!(choices_named(&app, "Ask agent about all")[..2], ["change (2)", "answer only (2)"]);
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
    assert_eq!(items(&app)[0].0, "Edit your comment");
    choose_as(&mut app, "Edit your comment", "here");
    assert!(matches!(&app.mode, Mode::Edit(e) if e.ed.text() == "why so loud?"), "{:?}", app.mode);
    assert!(draw(&mut app).contains("your comment: edit it, or add to it"));
    typed(&mut app, " really");
    keys(&mut app, &[KeyCode::Esc]);
    choose(&mut app, "Save it");
    let n = notes_of(&db, "s", 0).remove(0);
    assert_eq!(n.text, "why so loud? really");
    // Enter on its line opens it too. Left as it was, Esc still asks what to
    // do with it: it may be sent as it is. Nothing would be saved, nor
    // thrown away: it is left as it is.
    keys(&mut app, &[KeyCode::Up]);
    assert!(app.rows[app.cursor].text.starts_with("+TWO"));
    keys(&mut app, &[KeyCode::Enter]);
    assert!(matches!(&app.mode, Mode::Edit(e) if e.ed.text() == "why so loud? really"), "{:?}", app.mode);
    // The editor's foot and the bar say so; changed, what Esc does then.
    assert!(draw(&mut app).contains("enter: a new line · esc: send as it is, or leave it"));
    assert_eq!(app.hint(), "enter: a new line · esc: send as it is, or leave it");
    typed(&mut app, "!");
    assert!(draw(&mut app).contains("enter: a new line · esc: save, send or throw away"));
    key(&mut app, KeyCode::Backspace);
    keys(&mut app, &[KeyCode::Esc]);
    let labels: Vec<String> = items(&app).into_iter().map(|(l, _)| l).collect();
    assert_eq!(labels, ["Send it", "Ask agent", "Attach a reference…", "", "Leave it as it is", "Keep writing"]);
    assert_eq!(choices_named(&app, "Ask agent"), ["change", "the whole stack", "answer only"]);
    assert_eq!(app.help()[0], "Press Enter to ask the agent about the comment as it is (may change the code).");
    choose(&mut app, "Leave it as it is");
    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(app.status, "left as it is");
    assert_eq!(notes_of(&db, "s", 0).remove(0).text, "why so loud? really");
    keys(&mut app, &[KeyCode::Down]);
    // Or in $EDITOR.
    let target = stacked::tui::Target::Edit { change: n.change.clone(), id: n.id.clone() };
    assert_eq!(choose_as(&mut app, "Edit your comment", "in $EDITOR"), Effect::EditComment(target));
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
    choose_as(&mut app, "Reply", "here");
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
                    stage: stacked::db::Stage::Ready,
                    on: None,
                    was: None,
                }),
                ..n
            })
        })
        .unwrap();
    };
    wait(&db);
    app.refresh();
    choose_as(&mut app, "Edit your reply", "here");
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
    choose_as(&mut app, "Edit your reply", "here");
    typed(&mut app, " now");
    keys(&mut app, &[KeyCode::Esc]);
    choose(&mut app, "Throw the changes away");
    assert!(app.status.contains("as it was: back on the agent's queue (answer only)"), "{}", app.status);
    // Not while the agent works on it.
    wait_for(&mut app, "⋯ Reading the patch.");
    keys(&mut app, &[KeyCode::Char(' ')]);
    assert!(items(&app).contains(&("Edit your reply".to_string(), false)), "{:?}", items(&app));
    keys(&mut app, &[KeyCode::Esc]);
    accept_landed(&mut app, &db);
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
fn a_turn_is_drawn_in_its_author_s_colour_what_they_said_in_the_terminal_s() {
    use ratatui::style::{Color, Modifier};
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "why so loud?");
    let n = notes_of(&db, "s", 0).remove(0);
    Db::edit(&db, |d| {
        d.set_note(Note {
            text: append_turn(&n.text, Role::Agent, "It is a shout.", Some("answer")),
            ..n.clone()
        })
    })
    .unwrap();
    app.refresh();
    let mut buf = Buffer::empty(Rect::new(0, 0, 120, 30));
    app.draw(&mut buf);
    let screen = (0..buf.area.height)
        .map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");
    // Each turn has a bar at its left and its author's name, in the
    // author's colour, the name bold; what they said is in the terminal's
    // own colour, as code never is.
    for (who, said, colour) in [("you", "why so loud?", 172), ("agent", "It is a shout.", 134)] {
        let (x, y) = at(&screen, &format!("▎ {who}:"));
        assert_eq!(buf[(x, y)].fg, Color::Indexed(colour), "{who}'s bar:\n{screen}");
        let name = &buf[(x + 2, y)];
        assert!(name.fg == Color::Indexed(colour) && name.modifier.contains(Modifier::BOLD), "{who}'s name: {name:?}");
        let (x, y) = at(&screen, &format!("▎   {said}"));
        assert_eq!(buf[(x, y)].fg, Color::Indexed(colour), "the bar of what {who} said");
        assert_eq!(buf[(x + 4, y)].fg, Color::Reset, "what {who} said");
    }
    // Lines a patch adds are green, as ever.
    let (x, y) = at(&screen, "+TWO");
    assert_eq!(buf[(x, y)].fg, Color::Green);
}

#[test]
fn esc_asks_what_to_do_with_a_comment_unless_nothing_would_be_done() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    // With nothing written, Esc closes the editor: nothing would be saved.
    // So too once what was written is cut away.
    keys(&mut app, &[KeyCode::Enter, KeyCode::Esc]);
    assert_eq!(app.mode, Mode::Normal);
    keys(&mut app, &[KeyCode::Enter]);
    typed(&mut app, "why");
    ctrl(&mut app, KeyCode::Char('a'));
    ctrl(&mut app, KeyCode::Char('k'));
    keys(&mut app, &[KeyCode::Esc]);
    assert_eq!(app.mode, Mode::Normal);
    assert!(notes_of(&db, "s", 0).is_empty());
    // Answered, the agent's reply written again is still the last word:
    // nothing can be asked from it. Left as it was, Esc closes it; changed,
    // Esc offers to save it, not to ask.
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "why so loud?");
    let n = notes_of(&db, "s", 0).remove(0);
    Db::edit(&db, |d| {
        d.set_note(Note {
            text: append_turn(&n.text, Role::Agent, "It is a shout.", Some("answer")),
            ..n.clone()
        })
    })
    .unwrap();
    app.refresh();
    down_to(&mut app, "     ▎ agent:");
    choose_as(&mut app, "Edit the agent's reply", "here");
    assert_eq!(app.hint(), "enter: a new line · esc: close");
    keys(&mut app, &[KeyCode::Esc]);
    assert_eq!(app.mode, Mode::Normal);
    keys(&mut app, &[KeyCode::Home]);
    down_to(&mut app, "     ▎ agent:");
    choose_as(&mut app, "Edit the agent's reply", "here");
    typed(&mut app, " Loudly.");
    keys(&mut app, &[KeyCode::Esc]);
    let labels: Vec<String> = items(&app).into_iter().map(|(l, _)| l).collect();
    assert_eq!(labels, ["Save it", "", "Throw the changes away", "Keep writing"]);
    choose(&mut app, "Throw the changes away");
    assert_eq!(app.status, "your changes are thrown away: the comment is as it was");
    // Replied to, the last word is yours: opened again and left as it was,
    // Esc still asks what to do with it, and it is asked as it is.
    choose_as(&mut app, "Reply", "here");
    save(&mut app, "louder");
    keys(&mut app, &[KeyCode::Enter]);
    assert!(matches!(&app.mode, Mode::Edit(e) if e.ed.text() == "louder"), "{:?}", app.mode);
    keys(&mut app, &[KeyCode::Esc]);
    assert_eq!(items(&app)[1], ("Ask agent".to_string(), true));
    let before = notes_of(&db, "s", 0).remove(0).text;
    choose_as(&mut app, "Ask agent", "answer only");
    assert_eq!(notes_of(&db, "s", 0).remove(0).text, before, "nothing written: asked as it is");
    assert!(app.status.contains("asked the agent about 1 comment (answer only)"), "{}", app.status);
    let said: Vec<String> = stacked::thread::thread_turns(&notes_of(&db, "s", 0).remove(0).text).into_iter().map(|t| t.body).collect();
    assert_eq!(said, ["why so loud?", "It is a shout.", "louder"]);
    accept_landed(&mut app, &db);
    wait_for(&mut app, "agent replied (answer only)");
}

/// The agent changing `scope` of stack `s`, asked by comment (change, id):
/// its run as it is while it works, its process this one.
fn changing(db: &Path, change: &str, id: &str, scope: serde_json::Value) {
    Db::edit(db, |d| {
        let from = d.head("s").unwrap().0;
        let ask = serde_json::json!({"way": "change", "scope": scope, "from": from, "notes": [{"label": "c1", "change": change, "id": id}]});
        d.add_run(stacked::db::Run {
            id: "run-changing".into(),
            what: "edit".into(),
            stack: "s".into(),
            state: stacked::db::RunState::Started,
            pid: Some(std::process::id()),
            ask: Some(ask),
            time: stacked::db::now(),
            ..Default::default()
        })
        .map(|_| ())
    })
    .unwrap();
}

#[test]
fn while_the_agent_starts_one_line_says_what_it_runs() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "why so loud?");
    let n = notes_of(&db, "s", 0).remove(0);
    changing(&db, &n.change, &n.id, serde_json::json!({"patch": n.change}));
    app.refresh();
    assert!(draw(&mut app).contains("⋯ the agent is starting…"));
    // Each command run to start it, as it runs: one line, the last.
    let event = |e: serde_json::Value| {
        Db::append(&db, &[stacked::db::Record::Event { run: "run-changing".into(), event: e }]).unwrap();
    };
    let step = |text: &str| event(serde_json::json!({"type": "stacked", "text": text}));
    step("git worktree add --detach /tmp/stacked-run-changing 1234567890ab");
    app.refresh();
    let screen = draw(&mut app);
    assert!(screen.contains("⋯ starting: git worktree add --detach /tmp/stacked-r │"), "cut where the line ends:\n{screen}");
    step("git am --keep-non-patch, 1 patch");
    app.refresh();
    let screen = draw(&mut app);
    assert!(screen.contains("⋯ starting: git am --keep-non-patch, 1 patch"), "{screen}");
    assert_eq!(screen.matches("⋯ ").count(), 1, "one line:\n{screen}");
    // Once the agent writes, what it writes.
    event(serde_json::json!({"type": "system", "subtype": "init", "model": "m"}));
    app.refresh();
    let screen = draw(&mut app);
    assert!(screen.contains("⋯ ● session started · m") && !screen.contains("is starting"), "{screen}");
}

#[test]
fn while_the_agent_changes_the_stack_it_is_not_edited_by_hand_nor_its_comment_written_on() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "why so loud?");
    let n = notes_of(&db, "s", 0).remove(0);
    changing(&db, &n.change, &n.id, serde_json::json!({"patch": n.change}));
    app.refresh();
    // Its comment is read, not written on: its reply is to what it was asked.
    keys(&mut app, &[KeyCode::Down, KeyCode::Char(' ')]);
    let all = items(&app);
    assert!(all.contains(&("Reply".into(), false)) && all.contains(&("Edit your comment".into(), false)), "{all:?}");
    keys(&mut app, &[KeyCode::Esc]);
    // The patch is not edited by hand, and the menu says why; other lines
    // are commented on as ever.
    down_to(&mut app, " three");
    keys(&mut app, &[KeyCode::Char(' ')]);
    let why = "the agent is changing patch 1: the stack can be edited by hand again once you have reviewed the agent's work";
    let Mode::Menu(m) = &app.mode else { panic!("{:?}", app.mode) };
    assert_eq!(m.note.as_deref(), Some(why));
    let all = items(&app);
    assert!(all.contains(&("Comment on this line".into(), true)), "{all:?}");
    assert!(all.contains(&("Edit this patch in $EDITOR".into(), false)), "{all:?}");
    assert!(all.iter().any(|(l, ok)| l.starts_with("Edit a.txt here") && !ok), "{all:?}");
    keys(&mut app, &[KeyCode::Esc]);
    // Nor moved, dropped, undone.
    keys(&mut app, &[KeyCode::Left, KeyCode::Char(' ')]);
    let all = items(&app);
    for l in ["Move this patch…", "Edit this patch in $EDITOR", "Drop this patch…"] {
        assert!(all.contains(&(l.into(), false)), "{l}: {all:?}");
    }
    assert!(!all.iter().any(|(l, ok)| l.starts_with("Undo") && *ok), "{all:?}");
    keys(&mut app, &[KeyCode::Esc]);
    // Landed, the stack is the user's again.
    Db::edit(&db, |d| {
        let r = d.run("run-changing").cloned().unwrap();
        d.add_run(stacked::db::Run {
            state: stacked::db::RunState::Done,
            ..r
        })
        .map(|_| ())
    })
    .unwrap();
    app.refresh();
    keys(&mut app, &[KeyCode::Char(' ')]);
    assert!(items(&app).contains(&("Drop this patch…".into(), true)), "{:?}", items(&app));
}

/// What the agent brought back, landed and held for review: its reply
/// `text` to comment (change, id), with a change of the stack if `change`
/// (the stack as it is, made again).
fn held(db: &Path, run: &str, change: &str, id: &str, text: &str, with_change: bool) {
    Db::edit(db, |d| {
        let (from, s) = d.head("s").map(|(r, s)| (r, s.clone())).unwrap();
        let way = if with_change { "change" } else { "answer" };
        let ask = serde_json::json!({"way": way, "scope": {"patch": change}, "from": from, "notes": [{"label": "c1", "change": change, "id": id}]});
        let stack = with_change.then(|| stacked::held::Change {
            patches: s.patches.clone(),
            base: s.base.clone(),
            op: "agent: patch 1".into(),
            of: "patch 1".into(),
            from: None,
        });
        let reply = stacked::held::Reply {
            change: change.into(),
            id: id.into(),
            text: text.into(),
            tag: way.into(),
            ..Default::default()
        };
        d.add_run(stacked::db::Run {
            id: run.into(),
            what: way.into(),
            stack: "s".into(),
            state: stacked::db::RunState::Done,
            ask: Some(ask),
            held: Some(stacked::held::Held {
                at: stacked::db::now(),
                stack,
                replies: vec![reply],
                ..Default::default()
            }),
            ..Default::default()
        })
        .map(|_| ())
    })
    .unwrap();
}

#[test]
fn a_comment_is_read_as_markdown() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "why so loud?");
    let n = notes_of(&db, "s", 0).remove(0);
    let reply = "It is **on purpose**:\n- `TWO` stands out\n- see [bug 1](https://b/1)\n```\n  let x = 1;\n```\n| a | b |\n|---|---|\n| 1 | `2` |";
    let text = append_turn(&n.text, Role::Agent, reply, Some("answer"));
    Db::edit(&db, |d| d.set_note(Note { text, ..n.clone() })).unwrap();
    app.refresh();
    let mut buf = Buffer::empty(Rect::new(0, 0, 120, 30));
    app.draw(&mut buf);
    let screen = (0..30).map(|y| (0..120).map(|x| buf[(x, y)].symbol()).collect::<String>()).collect::<Vec<_>>().join("\n");
    // Its markup gone; its list bulleted; its code block as it is, fences
    // left out.
    for line in ["▎   It is on purpose:", "▎   • TWO stands out", "▎   • see bug 1 (https://b/1)", "▎      let x = 1; ", "▎   a │ b", "▎   1 │ 2"] {
        assert!(screen.contains(line), "{line:?}:\n{screen}");
    }
    assert!(!screen.contains("**") && !screen.contains("```") && !screen.contains("`TWO`"), "{screen}");
    // Bold where it says so, the link underlined, in the terminal's own colour.
    use ratatui::style::Modifier;
    let (x, y) = at(&screen, "▎   It is on purpose");
    let cell = &buf[(x + 10, y)];
    assert!(cell.symbol() == "o" && cell.modifier.contains(Modifier::BOLD), "{cell:?}");
    let (x, y) = at(&screen, "▎   • see bug 1 (");
    let cell = &buf[(x + 10, y)];
    assert!(cell.symbol() == "b" && cell.modifier.contains(Modifier::UNDERLINED), "{cell:?}");
    // Code in the colour of the settings for it: light grey on dark grey.
    use ratatui::style::Color;
    let (x, y) = at(&screen, "▎   • TWO stands");
    let cell = &buf[(x + 6, y)];
    assert!(cell.symbol() == "T" && cell.bg == Color::Indexed(236) && cell.fg == Color::Indexed(252), "{cell:?}");
    let (x, y) = at(&screen, "▎      let x = 1;");
    assert_eq!(buf[(x + 4, y)].bg, Color::Indexed(236), "the block's left edge");
    // Listed, it says its first line, its markup left out.
    assert!(screen.contains("   It is on purpose:"), "{screen}");
}

#[test]
fn the_patches_pane_says_where_head_is_in_the_checkout() {
    let (_tmp, db, src) = setup();
    let mut app = open(&db).unwrap();
    // Run elsewhere than a checkout of the stack's repository: no HEAD.
    app.refresh();
    app.settle();
    assert!(!draw(&mut app).contains("▶ HEAD"));
    // On the branch the stack was imported from: after its top.
    app.checkout = src.clone();
    app.look_at_head();
    app.settle();
    let screen = draw(&mut app);
    let row = |screen: &str, text: &str| at(screen, text).1;
    assert_eq!(row(&screen, "▶ HEAD main"), row(&screen, "  2 Bug 2") + 1, "{screen}");
    // Moved outside stacked, to patch 1: under it.
    git(&src, &["checkout", "-q", "--detach", "HEAD~1"]);
    let short = |rev: &str| {
        let out = Command::new("git").args(["rev-parse", rev]).current_dir(&src).output().unwrap();
        String::from_utf8_lossy(&out.stdout)[..8].to_string()
    };
    app.look_at_head();
    app.settle();
    let screen = draw(&mut app);
    assert_eq!(row(&screen, &format!("▶ HEAD {}", short("HEAD"))), row(&screen, "  1 Bug 1") + 1, "{screen}");
    // A commit of its own on top: said so.
    std::fs::write(src.join("c.txt"), "c\n").unwrap();
    git(&src, &["add", "c.txt"]);
    git(&src, &["commit", "-qm", "Mine"]);
    app.look_at_head();
    app.settle();
    let screen = draw(&mut app);
    assert_eq!(row(&screen, &format!("▶ HEAD {} +1", short("HEAD"))), row(&screen, "  1 Bug 1") + 1, "{screen}");
    // Not on the stack: under the header, said so.
    git(&src, &["checkout", "-q", "-b", "other", "HEAD~2"]);
    std::fs::write(src.join("d.txt"), "d\n").unwrap();
    git(&src, &["add", "d.txt"]);
    git(&src, &["commit", "-qm", "Other"]);
    app.look_at_head();
    app.settle();
    let screen = draw(&mut app);
    assert_eq!(row(&screen, "▶ HEAD other: off the stack"), row(&screen, "≡ s") + 1, "{screen}");
}

#[test]
fn head_is_moved_along_the_stack_only_once_enter_leaves_the_mode() {
    let (_tmp, db, src) = setup();
    let mut app = open(&db).unwrap();
    let rev = |rev: &str| {
        let out = Command::new("git").args(["rev-parse", rev]).current_dir(&src).output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    let row = |screen: &str, text: &str| at(screen, text).1;
    // Not a checkout of the stack's repository: nothing to move.
    app.refresh();
    app.settle();
    keys(&mut app, &[KeyCode::Char(' ')]);
    assert!(!items(&app).iter().any(|(l, _)| l == "Move HEAD…"), "{:?}", items(&app));
    keys(&mut app, &[KeyCode::Esc]);
    app.checkout = src.clone();
    app.look_at_head();
    app.settle();
    let was = rev("HEAD");
    // From patch 1's menu: HEAD shown under it, to place.
    choose(&mut app, "Move HEAD…");
    assert!(matches!(app.mode, Mode::MovingHead { to: 1 }), "{:?}", app.mode);
    let screen = draw(&mut app);
    assert_eq!(row(&screen, "↕ HEAD main"), row(&screen, "  1 Bug 1") + 1, "{screen}");
    assert_eq!(app.hint(), "↑↓ to place HEAD · enter: move your checkout there · esc: leave it where it was");
    // ↑↓ place it; the checkout does not change meanwhile, nor on Esc.
    keys(&mut app, &[KeyCode::Up]);
    assert_eq!(row(&draw(&mut app), "↕ HEAD main"), row(&screen, "≡ s") + 1);
    keys(&mut app, &[KeyCode::Down, KeyCode::Down, KeyCode::Down]);
    assert!(matches!(app.mode, Mode::MovingHead { to: 2 }), "{:?}", app.mode);
    assert_eq!(rev("HEAD"), was);
    keys(&mut app, &[KeyCode::Esc]);
    app.settle();
    assert!(matches!(app.mode, Mode::Normal));
    assert_eq!(rev("HEAD"), was);
    // Enter moves it there, once the mode is left: detached, main as it was.
    choose(&mut app, "Move HEAD…");
    keys(&mut app, &[KeyCode::Enter]);
    app.settle();
    assert_eq!(app.status, "HEAD is at patch 1 of s, detached: main is as it was");
    assert_eq!((rev("HEAD"), rev("main")), (rev("main~1"), was));
    let screen = draw(&mut app);
    assert_eq!(row(&screen, &format!("▶ HEAD {}", &rev("HEAD")[..8])), row(&screen, "  1 Bug 1") + 1, "{screen}");
}

#[test]
fn a_patch_whose_message_differs_in_the_checkout_is_marked_and_made_the_same_either_way() {
    let (_tmp, db, src) = setup();
    let mut app = open(&db).unwrap();
    let subject = || {
        let out = Command::new("git").args(["log", "-1", "--format=%s"]).current_dir(&src).output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    // The branch's top: patch 2 by its diff, its message other.
    git(&src, &["commit", "-q", "--amend", "-m", "Bug 2 - add a bee"]);
    app.checkout = src.clone();
    app.look_at_head();
    app.settle();
    let marked = |app: &mut App| draw(app).lines().any(|l| l.contains("  2 Bug 2") && l.contains("✉"));
    assert!(marked(&mut app), "{}", draw(&mut app));
    // HEAD's line says how many are to sync.
    assert!(draw(&mut app).contains("▶ HEAD main · 1 to sync"), "{}", draw(&mut app));
    // Its menu syncs it one way or the other, the stack's first; it says
    // both messages, and that the one on the arrow's left is kept.
    keys(&mut app, &[KeyCode::Down, KeyCode::Char(' ')]);
    let Mode::Menu(m) = &app.mode else { panic!("{:?}", app.mode) };
    let note = m.note.clone().unwrap_or_default();
    assert!(note.contains("\nstacked: Bug 2 - add b\nworkspace: Bug 2 - add a bee\n"), "{note}");
    assert!(note.contains("The message on the left of the arrow is kept, and it is written over the one on the right."), "{note}");
    let screen = draw(&mut app);
    assert!(screen.contains("[x] stacked → workspace  [ ] workspace → stacked"), "{screen}");
    choose(&mut app, "Sync commit message");
    app.settle();
    assert!(app.status.starts_with("main has the stack's message of patch 2"), "{}", app.status);
    assert_eq!(subject(), "Bug 2 - add b");
    assert!(!marked(&mut app) && !draw(&mut app).contains("to sync"));
    // The workspace's kept, picked with →: an edit by hand, its review open
    // at once.
    git(&src, &["commit", "-q", "--amend", "-m", "Bug 2 - add a bee"]);
    app.look_at_head();
    app.settle();
    keys(&mut app, &[KeyCode::Char(' ')]);
    for _ in 0..20 {
        if matches!(&app.mode, Mode::Menu(m) if m.items[m.sel].label == "Sync commit message") {
            break;
        }
        key(&mut app, KeyCode::Down);
    }
    assert_eq!(app.hint(), "↑↓ choose · ←→: which way the message goes · enter: do it · esc: close · ?: what it does");
    keys(&mut app, &[KeyCode::Right, KeyCode::Enter]);
    assert_eq!(app.screen, Screen::Review);
    assert_eq!(app.status, "took the message of patch 2 from your checkout: it waits for your review");
}

#[test]
fn the_agent_s_reply_waits_for_review_and_enter_takes_it() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "why so loud?");
    let n = notes_of(&db, "s", 0).remove(0);
    held(&db, "run-answer", &n.change, &n.id, "It is a shout.", false);
    app.refresh();
    // Its line says so, and nothing is written on it before it is taken.
    keys(&mut app, &[KeyCode::Down]);
    let screen = draw(&mut app);
    assert!(screen.contains("the agent's reply waits for your review") && !screen.contains("It is a shout."), "{screen}");
    // The comments pane marks it, and counts it; Show… lists it apart.
    assert!(screen.contains("◆ you · a.txt: TWO") && screen.contains("1 comment · 1 to review"), "{screen}");
    assert!(app.show_counts().contains(&(Show::ToReview, 1)) && app.show_counts().contains(&(Show::ToSend, 0)));
    keys(&mut app, &[KeyCode::Char(' ')]);
    let all = items(&app);
    for (l, ok) in [("Accept the agent's reply", true), ("Reply", false), ("Edit your comment", false)] {
        assert!(all.contains(&(l.into(), ok)), "{l}: {all:?}");
    }
    assert!(!all.iter().any(|(l, _)| l == "Ask agent"), "{all:?}");
    keys(&mut app, &[KeyCode::Esc]);
    // Enter takes it into the thread, at once: out of the review.
    assert!(app.hint().starts_with("enter: accept the agent's reply"), "{}", app.hint());
    keys(&mut app, &[KeyCode::Enter]);
    assert!(app.status.contains("you accepted the agent's reply"), "{}", app.status);
    assert!(draw(&mut app).contains("It is a shout."));
    assert!(stacked::held::queue(&Db::load(&db).unwrap(), "s").is_empty(), "nothing left of it to review");
    // A reply going with a change of the stack is reviewed with it; the
    // stack is not edited by hand meanwhile.
    held(&db, "run-change", &n.change, &n.id, "Changed it.", true);
    app.refresh();
    keys(&mut app, &[KeyCode::Char(' ')]);
    let all = items(&app);
    assert!(all.contains(&("Review the agent's work…".into(), true)) && !all.iter().any(|(l, _)| l.starts_with("Accept")), "{all:?}");
    // Enter opens the review of that work, on the reply.
    keys(&mut app, &[KeyCode::Esc, KeyCode::Enter]);
    assert_eq!(app.screen, Screen::Review);
    assert!(draw(&mut app).contains("its reply, as it would be in its thread"), "on the reply");
    app.screen = Screen::Main;
    app.reviewing = None;
    keys(&mut app, &[KeyCode::Up, KeyCode::Char(' ')]);
    let Mode::Menu(m) = &app.mode else { panic!("{:?}", app.mode) };
    assert_eq!(
        m.note.as_deref(),
        Some("the agent's change of patch 1 waits for your review: the stack can be edited by hand again once you have reviewed it")
    );
    assert!(items(&app).contains(&("Edit this patch in $EDITOR".into(), false)), "{:?}", items(&app));
}

#[test]
fn landed_work_is_reviewed_from_the_banner_part_by_part() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "why so loud?");
    down_to(&mut app, "+LINE 39");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "and this?");
    let notes = notes_of(&db, "s", 0);
    // What the agent brought back: patch 2 dropped, a reply to each
    // comment, a comment of its own on patch 1's title.
    Db::edit(&db, |d| {
        let (from, s) = d.head("s").map(|(r, s)| (r, s.clone())).unwrap();
        let p1 = d.patch(&s.patches[0]).clone();
        let reply = |n: &Note, text: &str| stacked::held::Reply {
            change: n.change.clone(),
            id: n.id.clone(),
            text: text.into(),
            tag: "change".into(),
            ..Default::default()
        };
        let shown: Vec<serde_json::Value> = notes.iter().map(|n| serde_json::json!({"label": "c1", "change": n.change, "id": n.id})).collect();
        d.add_run(stacked::db::Run {
            id: "run-review".into(),
            what: "change".into(),
            stack: "s".into(),
            state: stacked::db::RunState::Done,
            request: "why so loud?".into(),
            ask: Some(serde_json::json!({"way": "change", "scope": "stack", "from": from, "notes": shown})),
            held: Some(stacked::held::Held {
                at: stacked::db::now(),
                stack: Some(stacked::held::Change {
                    patches: vec![p1.id.clone()],
                    base: s.base.clone(),
                    op: "agent: the stack".into(),
                    of: "the stack".into(),
                    from: None,
                }),
                replies: vec![reply(&notes[0], "It shouts less now."), reply(&notes[1], "Left as it was.")],
                opened: vec![Note {
                    change: p1.change.clone(),
                    id: "agent:run-review:0".into(),
                    line: stacked::view::title_key(&p1),
                    text: "```agent change\nPatch 2 is gone.\n```".into(),
                    file: stacked::view::MSG_PATH.into(),
                    preview: p1.title().into(),
                    by: Some("agent".into()),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        })
        .map(|_| ())
    })
    .unwrap();
    app.refresh();
    // The banner says what waits. Esc goes to it from the middle, open, a
    // line for what landed; Esc again comes back where the cursor was.
    let screen = draw(&mut app);
    assert!(
        screen.contains("⟳ 1 landed ask waits for your review · the next: a change of the stack, 2 replies, 1 opened comment"),
        "{screen}"
    );
    assert!(!screen.contains("   ⟳ Review"), "closed:\n{screen}");
    let (focus, cursor) = (app.focus, app.cursor);
    assert_eq!(focus, Focus::Diff);
    keys(&mut app, &[KeyCode::Esc]);
    assert!(app.on_banner);
    let screen = draw(&mut app);
    assert!(screen.contains("\n   ⟳ Review a change of the stack, 2 replies, 1 opened comment, asked \"why so loud?\""), "{screen}");
    assert!(app.hint().contains("enter: review it · space: pick one · esc: back where you were"), "{}", app.hint());
    keys(&mut app, &[KeyCode::Esc]);
    assert!(!app.on_banner && app.focus == focus && app.cursor == cursor, "back where it was");
    assert!(!draw(&mut app).contains("   ⟳ Review"), "closed again");
    // ↑ from the stack's header goes there too.
    keys(&mut app, &[KeyCode::Left, KeyCode::Home, KeyCode::Up]);
    assert!(app.on_banner);
    keys(&mut app, &[KeyCode::Char(' ')]);
    assert!(items(&app)[0].0.starts_with("Review a change of the stack, 2 replies"), "{:?}", items(&app));
    // On its last line, a comment open; what landed is on top.
    keys(&mut app, &[KeyCode::Esc, KeyCode::Home, KeyCode::Enter]);
    assert_eq!(app.screen, Screen::Review);
    // Its three sections; the change of the stack, a line for each patch.
    let screen = draw(&mut app);
    for l in ["stack operations · to review", "noop    1 Bug 1 - shout two", "remove  2 Bug 2 - add b", " comments opened", " replies"] {
        assert!(screen.contains(l), "{l}:\n{screen}");
    }
    assert!(screen.contains("what accepting it does to the stack"), "{screen}");
    // Enter accepts; each comment is shown where it would be.
    assert_eq!(app.hint(), "↑↓ choose · enter: accept · →: look closer · space: menu · esc: save, discard or close");
    keys(&mut app, &[KeyCode::Enter]);
    assert!(draw(&mut app).contains("stack operations · accepted"));
    assert_eq!(app.hint(), "↑↓ choose · enter: to review again · →: look closer · space: menu · esc: save, discard or close");
    down_to_review(&mut app, "Patch 2 is");
    assert!(draw(&mut app).contains("a comment it opened, where it would be"));
    keys(&mut app, &[KeyCode::Enter]);
    down_to_review(&mut app, "It shouts less now.");
    // Again, it is to review again, not rejected: that is the menu's.
    keys(&mut app, &[KeyCode::Enter, KeyCode::Enter]);
    assert!(draw(&mut app).contains("its reply, as it would be in its thread"));
    let marks = |app: &App| app.reviewing.as_ref().unwrap().marks.clone();
    assert!(!marks(&app).contains_key("reply 0"), "{:?}", marks(&app));
    choose(&mut app, "Mark as rejected");
    let m = marks(&app);
    assert_eq!((m["stack"], m["opened 0"], m["reply 0"]), (true, true, false));
    // The change rejected rejects every comment with it; accepted again,
    // each is as it was. (On top, the comments that asked.)
    keys(&mut app, &[KeyCode::Home]);
    assert!(selected_line(&mut app).unwrap().starts_with("   keep "));
    down_to_review(&mut app, "stack operations");
    choose(&mut app, "Mark as rejected");
    assert!(marks(&app).values().all(|m| !m), "{:?}", marks(&app));
    choose(&mut app, "Mark as accepted");
    assert_eq!(marks(&app), [("opened 0", true), ("reply 0", false), ("stack", true)].map(|(k, m)| (k.to_string(), m)).into());
    // Esc says what is marked, by kind, and closes only once all are.
    keys(&mut app, &[KeyCode::Esc]);
    let Mode::Menu(m) = &app.mode else { panic!("{:?}", app.mode) };
    assert_eq!(
        m.note.as_deref(),
        Some("asking comments: 2 kept\nthe change of the stack: accepted\ncomments opened: 1 accepted\nreplies: 1 rejected, 1 to review")
    );
    assert!(items(&app).contains(&("Close review".into(), false)), "{:?}", items(&app));
    // Saved, the marks are there when it is opened again.
    choose(&mut app, "Save");
    assert_eq!(app.screen, Screen::Main);
    assert_eq!(Db::load(&db).unwrap().review("run-review").unwrap().marks.len(), 3);
    app.on_banner = true;
    keys(&mut app, &[KeyCode::Enter]);
    assert_eq!(marks(&app).len(), 3);
    // Thrown away, what was marked since is not.
    down_to_review(&mut app, "Left as it was.");
    keys(&mut app, &[KeyCode::Enter, KeyCode::Esc]);
    choose(&mut app, "Discard");
    assert!(app.status.contains("thrown away"), "{}", app.status);
    assert_eq!(Db::load(&db).unwrap().review("run-review").unwrap().marks.len(), 3);
    // What is not marked accepted at once, then closed: what was accepted is
    // applied, the rest dropped.
    app.on_banner = true;
    keys(&mut app, &[KeyCode::Enter, KeyCode::Esc]);
    choose(&mut app, "Accept all remaining and Close");
    choose(&mut app, "Accept them and close");
    assert!(
        app.status.contains("reviewed: its change accepted; 1 opened comment accepted; 1 reply accepted, 1 rejected"),
        "{}",
        app.status
    );
    assert_eq!(app.screen, Screen::Main);
    let d = Db::load(&db).unwrap();
    assert_eq!(d.stack("s").unwrap().patches.len(), 1, "patch 2 dropped");
    let notes = notes_of(&db, "s", 0);
    let said: String = notes.iter().map(|n| n.text.clone()).collect();
    assert!(said.contains("Left as it was.") && !said.contains("It shouts less now.") && said.contains("Patch 2 is gone."), "{said}");
    assert!(!draw(&mut app).contains("⟳"), "nothing waits");
}

/// Down the review's list until the cursor is on the line holding `text`.
fn down_to_review(app: &mut App, text: &str) {
    for _ in 0..50 {
        if selected_line(app).is_some_and(|l| l.contains(text)) {
            return;
        }
        let sel = app.reviewing.as_ref().map(|r| r.sel);
        key(app, KeyCode::Down);
        if app.reviewing.as_ref().map(|r| r.sel) == sel {
            break;
        }
    }
    panic!("no line {text:?} in the review:\n{}", draw(app));
}

/// The text of the review's line the cursor is on, as drawn.
fn selected_line(app: &mut App) -> Option<String> {
    let mut buf = Buffer::empty(Rect::new(0, 0, 120, 30));
    app.draw(&mut buf);
    (0..30).find_map(|y| {
        let reversed = (0..48).all(|x| buf[(x, y)].modifier.contains(ratatui::style::Modifier::REVERSED));
        reversed.then(|| (0..48).map(|x| buf[(x, y)].symbol()).collect())
    })
}

fn ctrl(app: &mut App, code: KeyCode) {
    app.key(KeyEvent::new(code, KeyModifiers::CONTROL));
}

#[test]
fn any_turn_is_written_again_and_the_conversation_rewound() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "why so loud?");
    // The agent answers at length, and you ask more.
    let n = notes_of(&db, "s", 0).remove(0);
    Db::edit(&db, |d| {
        let text = append_turn(&n.text, Role::Agent, "It is a shout. Shouting is for the lines that matter.", Some("answer"));
        d.set_note(Note {
            text: append_turn(&text, Role::You, "and why here?", None),
            ..n.clone()
        })
    })
    .unwrap();
    app.refresh();
    // On the agent's turn, the menu writes it again, or rewinds the
    // conversation from your comment above it.
    down_to(&mut app, "     ▎ agent:");
    keys(&mut app, &[KeyCode::Char(' ')]);
    let labels: Vec<String> = items(&app).into_iter().map(|(l, _)| l).collect();
    assert_eq!(labels[..3], ["Edit the agent's reply", "Reply", "Rewind the conversation from your comment"], "{labels:?}");
    keys(&mut app, &[KeyCode::Esc]);
    // Trimmed to what matters, with a shell's keys: from the line's start,
    // four words on, the rest cut. Edited, the turn is in the editor only.
    choose_as(&mut app, "Edit the agent's reply", "here");
    assert!(matches!(&app.mode, Mode::Edit(e) if e.ed.text().ends_with("lines that matter.")), "{:?}", app.mode);
    let screen = draw(&mut app);
    assert!(screen.contains("the agent's reply: edit it"), "{screen}");
    assert_eq!(screen.matches("Shouting is for the lines").count(), 1, "once, in the editor:\n{screen}");
    ctrl(&mut app, KeyCode::Char('a'));
    for _ in 0..4 {
        ctrl(&mut app, KeyCode::Right);
    }
    ctrl(&mut app, KeyCode::Char('k'));
    keys(&mut app, &[KeyCode::Esc]);
    choose(&mut app, "Save it");
    let text = notes_of(&db, "s", 0).remove(0).text;
    let turns = stacked::thread::thread_turns(&text);
    let said: Vec<(Role, &str)> = turns.iter().map(|t| (t.role, t.body.as_str())).collect();
    assert_eq!(said, [(Role::You, "why so loud?"), (Role::Agent, "It is a shout."), (Role::You, "and why here?")]);
    assert_eq!(turns[1].tag.as_deref(), Some("answer"), "still the agent's answer");
    // Rewound from your comment: the editor empty for the new text, what
    // goes under it as # lines, and in the editor only.
    choose_as(&mut app, "Rewind the conversation from your comment", "here");
    let Mode::Edit(e) = &app.mode else { panic!("{:?}", app.mode) };
    assert_eq!(e.ed.text(), "# you:\n#   why so loud?\n# agent:\n#   It is a shout.\n# you:\n#   and why here?");
    let screen = draw(&mut app);
    assert!(screen.contains("rewind from your comment: write what replaces it"), "{screen}");
    assert_eq!(screen.matches("why so loud?").count(), 1, "{screen}");
    // Left with nothing written, the conversation is as it was; so too with
    // lines stacked wrote cut, and nothing else written.
    keys(&mut app, &[KeyCode::Esc]);
    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(notes_of(&db, "s", 0).remove(0).text, text);
    choose_as(&mut app, "Rewind the conversation from your comment", "here");
    for _ in 0..3 {
        ctrl(&mut app, KeyCode::Char('k'));
    }
    assert!(matches!(&app.mode, Mode::Edit(e) if e.ed.text().starts_with("#   why so loud?")), "{:?}", app.mode);
    keys(&mut app, &[KeyCode::Esc]);
    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(notes_of(&db, "s", 0).remove(0).text, text);
    // Written, the new text takes the place of all of it.
    choose_as(&mut app, "Rewind the conversation from your comment", "here");
    typed(&mut app, "why so very loud?");
    keys(&mut app, &[KeyCode::Esc]);
    choose(&mut app, "Save it");
    let text = notes_of(&db, "s", 0).remove(0).text;
    assert_eq!(text, "why so very loud?");
    // From a reply of yours, the turns before it are kept; lines of yours
    // starting with # too.
    Db::edit(&db, |d| {
        let n = d.note(&n.change, &n.id).cloned().unwrap();
        let text = append_turn(&n.text, Role::Agent, "It shouts.", Some("answer"));
        let text = append_turn(&text, Role::You, "Why here?", None);
        d.set_note(Note {
            text: append_turn(&text, Role::Agent, "Because.", Some("answer")),
            ..n
        })
    })
    .unwrap();
    app.refresh();
    keys(&mut app, &[KeyCode::Home]);
    down_to(&mut app, "     ▎   Because.");
    // In $EDITOR, from your reply above, the third turn.
    let rewind = stacked::tui::Target::Rewind { change: n.change.clone(), id: n.id.clone(), from: 2 };
    assert_eq!(choose_as(&mut app, "Rewind the conversation from your reply", "in $EDITOR"), Effect::EditComment(rewind));
    choose_as(&mut app, "Rewind the conversation from your reply", "here");
    typed(&mut app, "# Why here?");
    keys(&mut app, &[KeyCode::Enter]);
    typed(&mut app, "And not there?");
    keys(&mut app, &[KeyCode::Esc]);
    choose(&mut app, "Save it");
    let text = notes_of(&db, "s", 0).remove(0).text;
    let said: Vec<String> = stacked::thread::thread_turns(&text).into_iter().map(|t| t.body).collect();
    assert_eq!(said, ["why so very loud?", "It shouts.", "# Why here?\nAnd not there?"]);
    // Your last word, written again, is Edit your reply's: no rewind there.
    keys(&mut app, &[KeyCode::Home]);
    down_to(&mut app, "     ▎   And not there?");
    keys(&mut app, &[KeyCode::Char(' ')]);
    assert!(!items(&app).iter().any(|(l, _)| l.starts_with("Rewind")), "{:?}", items(&app));
    keys(&mut app, &[KeyCode::Esc]);
    // Its history says what was done.
    let d = Db::load(&db).unwrap();
    let mut said = Vec::new();
    let mut before: Option<&Note> = None;
    for (_, x) in d.note_history(&n.change, &n.id) {
        said.push(stacked::history::note_change(before, x));
        before = Some(x);
    }
    assert!(said.contains(&"you edited the agent's reply".to_string()), "{said:?}");
    assert!(said.contains(&"you rewound the conversation from your comment".to_string()), "{said:?}");
    assert_eq!(said.last().map(String::as_str), Some("you rewound the conversation from your reply"), "{said:?}");
}

#[test]
fn a_comment_is_collapsed_by_hand_to_one_quiet_line() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "why so loud?");
    let n = notes_of(&db, "s", 0).remove(0);
    Db::edit(&db, |d| {
        d.set_note(Note {
            text: append_turn(&n.text, Role::Agent, "It is a shout.", Some("answer")),
            ..n.clone()
        })
    })
    .unwrap();
    app.refresh();
    keys(&mut app, &[KeyCode::Down]);
    draw(&mut app);
    let rows = app.rows.len();
    choose(&mut app, "Collapse this comment");
    // One quiet line, the cursor still on it: how it starts, and its turns.
    let row = &app.rows[app.cursor];
    assert!(row.head && row.text == "   ▸ comment: why so loud? · 2 turns", "{row:?}");
    assert!(app.rows.len() < rows);
    assert!(app.status.contains("press Enter on its line to expand it"), "{}", app.status);
    // The middle shows no more of it; the comments pane still lists it.
    let screen = draw(&mut app);
    assert!(!app.rows.iter().any(|r| r.text.contains("It is a shout.")), "{screen}");
    assert!(screen.contains("✓ you · a.txt: TWO"), "{screen}");
    assert!(app.hint().starts_with("enter: expand it"), "{}", app.hint());
    assert!(Db::load(&db).unwrap().collapsed(&n.change, &n.id), "kept");
    // Its menu expands it first, as enter does.
    keys(&mut app, &[KeyCode::Char(' ')]);
    assert_eq!(items(&app)[0].0, "Expand this comment");
    assert!(!items(&app).iter().any(|(l, _)| l == "Collapse this comment"), "{:?}", items(&app));
    keys(&mut app, &[KeyCode::Esc, KeyCode::Enter]);
    assert_eq!(app.rows.len(), rows);
    assert!(app.rows.iter().any(|r| r.text.contains("It is a shout.")));
    assert!(!Db::load(&db).unwrap().collapsed(&n.change, &n.id));
    // Collapsed again, it is expanded to be replied to.
    choose(&mut app, "Collapse this comment");
    choose_as(&mut app, "Reply", "here");
    assert!(matches!(app.mode, Mode::Edit(_)), "{:?}", app.mode);
    draw(&mut app);
    assert!(app.rows.iter().any(|r| r.text.contains("It is a shout.")));
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
    assert_eq!(all[2], "Save it and ask agent", "{all:?}");
    assert_eq!(choices_named(&app, "Save it and ask agent")[..2], ["change", "the whole stack"]);
    choose(&mut app, "Save it");
    // Its menu has it too, beside asking about its patch alone.
    keys(&mut app, &[KeyCode::Down]);
    assert!(app.rows[app.cursor].head);
    keys(&mut app, &[KeyCode::Char(' ')]);
    let all: Vec<String> = items(&app).into_iter().map(|(l, _)| l).collect();
    assert!(all.iter().any(|l| l == "Ask agent"), "{all:?}");
    assert_eq!(choices_named(&app, "Ask agent")[..2], ["change", "the whole stack"]);
    choose_as(&mut app, "Ask agent", "the whole stack");
    assert!(app.status.contains("asked the agent about 1 comment (may change the whole stack)"), "{}", app.status);
    let screen = wait_for(&mut app, "⋯ Reading the patch.");
    assert!(screen.contains("with the agent (may change the whole stack)"), "{screen}");
    // The ask has the whole stack; its answer lands in the thread, on its
    // line.
    let d = Db::load(&db).unwrap();
    let run = d.runs().last().unwrap().id.clone();
    assert_eq!(stacked::ask::run_scope(&d, &run), Some(stacked::ask::Scope::Stack));
    drop(d);
    accept_landed(&mut app, &db);
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
    assert!(all.contains(&"Ask agent".to_string()), "{all:?}");
    assert!(!choices_named(&app, "Ask agent").contains(&"the whole stack".to_string()));
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
    choose_as(&mut app, "Save it and ask agent", "answer only");
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
    // Its answer lands, and waits for your review: the banner says so, and
    // nothing else changes, not even the bottom line.
    let screen = wait_for(&mut app, "⟳ 1 landed ask waits for your review");
    assert!(screen.contains("the next: 1 reply, asked \"why so loud?\"") && !screen.contains("Seen."), "{screen}");
    assert!(!app.status.contains("answer done"), "{}", app.status);
    // Accepted, it is a turn of the thread.
    accept_landed(&mut app, &db);
    let screen = wait_for(&mut app, "agent replied (answer only)");
    assert!(screen.contains("Seen.") && screen.contains("✓ you · a.txt: TWO"), "{screen}");
    assert!(screen.contains("1 to read") && !screen.contains("⟳"), "{screen}");

    // Show… picks what both panes show, each with its count; kept per stack.
    keys(&mut app, &[KeyCode::Right, KeyCode::Home]);
    choose(&mut app, "Show…");
    let all: Vec<String> = items(&app).into_iter().map(|(l, _)| l).collect();
    assert_eq!(all[..5], ["● all (1)", "  to send (0)", "  to review (0)", "  to read (1)", "  to read: read-only (1)"], "{all:?}");
    choose(&mut app, "  to send");
    assert_eq!(app.show, Show::ToSend);
    let screen = draw(&mut app);
    assert!(!screen.contains("Seen."), "hidden in the patch too:\n{screen}");
    assert_eq!(Db::load(&db).unwrap().view("s"), Some("to send"));
    let mut again = open(&db).unwrap();
    assert_eq!(again.show, Show::ToSend, "kept in the file");
    draw(&mut again);
}

/// A comment asked about, its stream opened in place.
fn asked_and_streamed(db: &Path) -> App {
    fake_claude();
    let mut app = open(db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    typed(&mut app, "why so loud?");
    keys(&mut app, &[KeyCode::Esc]);
    choose_as(&mut app, "Save it and ask agent", "answer only");
    wait_for(&mut app, "⋯ Reading the patch.");
    keys(&mut app, &[KeyCode::Down]);
    keys(&mut app, &[KeyCode::Enter]);
    assert!(matches!(app.mode, Mode::Stream { .. }), "{:?}", app.mode);
    app
}

#[test]
fn a_stream_read_in_place_folds_once_the_agent_is_done() {
    let (_tmp, db, _src) = setup();
    let mut app = asked_and_streamed(&db);
    assert_eq!(app.hint(), "↑↓: scroll · esc: fold it");
    // Its answer lands: the stream folds, the keys are the screen's again,
    // and the bottom line says what waits.
    let screen = wait_for(&mut app, "⟳ 1 landed ask waits for your review");
    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(app.status, "the agent is done: what it brought waits for your review");
    assert!(!screen.contains("● session started · fake"), "{screen}");
    keys(&mut app, &[KeyCode::Char(' ')]);
    assert!(matches!(app.mode, Mode::Menu(_)), "{:?}", app.mode);
}

#[test]
fn a_stream_read_from_the_help_folds_there() {
    let (_tmp, db, _src) = setup();
    let mut app = asked_and_streamed(&db);
    keys(&mut app, &[KeyCode::Char('?')]);
    assert!(draw(&mut app).contains("Once the agent is done, the stream folds on its own."));
    // Its answer lands: the help goes back to the screen, not the stream.
    for _ in 0..300 {
        app.refresh();
        if matches!(&app.mode, Mode::Help { back, .. } if **back == Mode::Normal) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let Mode::Help { back, .. } = &app.mode else { panic!("{:?}", app.mode) };
    assert_eq!(**back, Mode::Normal);
    assert_eq!(app.status, "the agent is done: what it brought waits for your review");
    keys(&mut app, &[KeyCode::Esc]);
    assert_eq!(app.mode, Mode::Normal);
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
    choose_as(&mut app, "Save it and ask agent", "change");
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
    accept_landed(&mut app, &db);
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
fn lines_of_white_space_alone_need_no_reading_unless_the_settings_say_so() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir(&src).unwrap();
    git(&src, &["init", "-q", "-b", "main"]);
    git(&src, &["config", "commit.gpgsign", "false"]);
    git(&src, &["config", "tag.gpgsign", "false"]);
    std::fs::write(src.join("f.txt"), "one\n").unwrap();
    git(&src, &["add", "."]);
    git(&src, &["commit", "-qm", "base"]);
    // Two lines of text, an empty line and a blank one between them.
    std::fs::write(src.join("f.txt"), "one\ntwo\n\n  \nthree\n").unwrap();
    git(&src, &["commit", "-qam", "Bug 1 - more"]);
    let db = tmp.path().join("db.jsonl");
    stacked(&src, &db, &["import", "s", "HEAD~1"]);
    let mut app = open(&db).unwrap();
    assert!(draw(&mut app).contains("2 lines to review"));
    keys(&mut app, &[KeyCode::Char('2'), KeyCode::Enter]);
    app.wait_blocks();
    let screen = draw(&mut app);
    assert!(screen.contains("reading 0/2") && screen.contains("read 0/2 lines") && screen.contains(" 0/2"), "{screen}");
    // Space reads on to what is left, never stopping at the empty line or
    // the blank one, until nothing is.
    down_to(&mut app, "+two");
    for _ in 0..4 {
        keys(&mut app, &[KeyCode::Char(' ')]);
        assert!(!["+", "+  "].contains(&app.rows[app.cursor].text.as_str()), "{}", app.status);
        if app.status.contains("nothing left to read below") {
            break;
        }
    }
    assert!(app.status.contains("nothing left to read below"), "{}", app.status);
    let screen = draw(&mut app);
    assert!(screen.contains("reading 2/2") && screen.contains("nothing left to review"), "{screen}");
    let line = screen.lines().find(|l| l.contains("1 Bug 1 - more")).unwrap();
    assert!(line.contains(" ✓│"), "all read: {line}");
    // Enter would take the mark back from the lines of code; the empty one
    // and the blank one, which need no reading, show nothing.
    assert!(screen.contains("│✓→○ +two") && screen.contains("│✓→○ +three"), "{screen}");
    let bare = screen.lines().filter(|l| l.split('│').nth(1).is_some_and(|m| m.trim_end() == "    +")).count();
    assert_eq!(bare, 2, "{screen}");
    // Needing review, as the settings may say, they are lines to read as
    // any other: those the marks made hold are read.
    let mut cfg = stacked::config::Config::default();
    cfg.view.whitespace_needs_review = true;
    let mut all = App::with_config(&db, cfg).unwrap();
    keys(&mut all, &[KeyCode::Char('2'), KeyCode::Enter]);
    all.wait_blocks();
    let screen = draw(&mut all);
    let read = all.rows.iter().filter(|r| r.read).count();
    assert!(screen.contains(&format!("reading {read}/4")), "{screen}");
    assert!(!all.rows.iter().any(|r| r.free));
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
    // A comment on a line of the first patch, closed after; the first
    // patch reworded, then dropped.
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "too loud");
    keys(&mut app, &[KeyCode::Down]);
    choose(&mut app, "Close this comment");
    choose(&mut app, "Close it");
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
    assert_eq!(items(&app)[1].0, "Save it and ask agent, answer only");
    assert!(!items(&app).iter().any(|(l, _)| l == "Save it and ask agent"), "it changes nothing");
    choose(&mut app, "Save it and ask agent, answer only");
    assert_eq!(app.screen, Screen::History);
    let d = Db::load(&db).unwrap();
    let n = d.notes(&stack_change("s")).into_iter().find(|n| n.about == Some(rev)).cloned().unwrap();
    assert_eq!(n.text, "why drop it?");
    drop(d);
    // Its answer shows under the record.
    accept_landed(&mut app, &db);
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
    choose(&mut app, "Close it and the comments it opened…");
    assert_eq!(items(&app)[1].0, "Keep them open");
    choose(&mut app, "Close them");
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
    // Opened again and left as it was, Esc closes it: nothing can be asked.
    keys(&mut app, &[KeyCode::Enter]);
    assert!(matches!(&app.mode, Mode::Edit(e) if e.ed.text() == "still fine"), "{:?}", app.mode);
    keys(&mut app, &[KeyCode::Esc]);
    assert_eq!(app.mode, Mode::Normal);
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
    // Nothing for `?` to say: no help opens, and the bottom line offers
    // none.
    assert_eq!(key(&mut app, KeyCode::Char('?')), Effect::None);
    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(app.hint(), "enter or space: menu · 1: view · 2: read");
    assert!(!draw(&mut app).contains("what can I do here"));
    // Enter, with nothing else to do, is the menu: only to quit.
    keys(&mut app, &[KeyCode::Enter]);
    assert_eq!(items(&app), vec![("Quit".to_string(), true)]);
    assert_eq!(key(&mut app, KeyCode::Enter), Effect::Quit);
}

#[test]
fn how_the_agent_is_asked_is_one_pick_kept_for_the_stack() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    typed(&mut app, "why so loud?");
    keys(&mut app, &[KeyCode::Esc]);
    // Saving: how to ask is the choice of one item, change at first.
    let at = items(&app).iter().position(|(l, _)| l == "Save it and ask agent").unwrap();
    while !matches!(&app.mode, Mode::Menu(m) if m.sel == at) {
        key(&mut app, KeyCode::Down);
    }
    assert!(draw(&mut app).contains("Save it and ask agent    [x] change  [ ] the whole stack  [ ] answer only"));
    key(&mut app, KeyCode::Right);
    assert_eq!(Db::load(&db).unwrap().choice("s", "ask"), Some("whole"));
    // Saved without asking; on the comment, Ask agent starts on the same
    // pick, the whole stack.
    keys(&mut app, &[KeyCode::Esc, KeyCode::Esc]);
    choose(&mut app, "Save it");
    keys(&mut app, &[KeyCode::Down, KeyCode::Char(' ')]);
    let at = items(&app).iter().position(|(l, _)| l == "Ask agent").unwrap();
    match &app.mode {
        Mode::Menu(m) => {
            let c = m.items[at].choices.as_ref().unwrap();
            assert_eq!(c.list[c.picked].shown, "the whole stack");
        }
        other => panic!("{other:?}"),
    }
    // Where a comment is written is a pick of its own.
    assert_eq!(Db::load(&db).unwrap().choice("s", "write"), None);
}

#[test]
fn a_closed_comment_is_shown_as_it_was_and_reopened() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    typed(&mut app, "why so loud?");
    keys(&mut app, &[KeyCode::Esc]);
    choose(&mut app, "Save it");
    // Closed from its menu, after stacked asks, which says it can be
    // reopened.
    keys(&mut app, &[KeyCode::Down]);
    choose(&mut app, "Close this comment…");
    assert!(draw(&mut app).contains("close this comment? Show…, closed, can reopen it"));
    choose(&mut app, "Close it");
    assert!(app.status.contains("Show…, closed"), "{}", app.status);
    assert!(!draw(&mut app).contains("why so loud?"));
    // Show…, closed, from the stack's header: on the patch, it is there as
    // it was, saying when it was deleted.
    keys(&mut app, &[KeyCode::Left, KeyCode::Up]);
    choose(&mut app, "Show…");
    choose(&mut app, "  closed");
    keys(&mut app, &[KeyCode::Down, KeyCode::Right]);
    let screen = draw(&mut app);
    assert!(screen.contains("why so loud?") && screen.contains("└ comment · closed @"), "{screen}");
    // Its menu, which Enter opens: to reopen it, or its history. (The
    // cursor came back where it was in the patch, under the comment.)
    keys(&mut app, &[KeyCode::Home]);
    down_to(&mut app, "   └ comment");
    assert_eq!(app.enter(), None);
    key(&mut app, KeyCode::Char(' '));
    assert_eq!(items(&app)[..2], [("Reopen it".to_string(), true), ("History of this comment…".to_string(), true)]);
    choose(&mut app, "Reopen it");
    assert!(app.status.contains("reopened"), "{}", app.status);
    let d = Db::load(&db).unwrap();
    let change = d.patch(&d.stack("s").unwrap().patches[0]).change.clone();
    assert_eq!(d.notes(&change).iter().map(|n| n.text.as_str()).collect::<Vec<_>>(), ["why so loud?"]);
    assert!(d.deleted_notes("s", &change).is_empty());
}

#[test]
fn the_comment_stays_in_sight_while_its_menu_says_what_to_do_with_it() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    typed(&mut app, "why so loud?");
    keys(&mut app, &[KeyCode::Esc]);
    assert!(matches!(app.mode, Mode::Menu(_)));
    // The comment where it is written, to read again; the menu beside it,
    // not over it; the terminal's cursor nowhere.
    let (screen, cursor) = draw_at(&mut app);
    assert!(screen.contains("a comment on: TWO") && screen.contains("why so loud?"), "{screen}");
    assert!(screen.contains("Save it and ask agent"), "{screen}");
    let text = screen.lines().position(|l| l.contains("why so loud?")).unwrap();
    let menu = screen.lines().position(|l| l.contains("Save it and ask agent")).unwrap();
    assert!(menu > text, "under it:\n{screen}");
    assert_eq!(cursor, None);
    // Kept writing: the editor again, as it was.
    choose(&mut app, "Keep writing");
    assert!(matches!(app.mode, Mode::Edit(_)));
    let (screen, cursor) = draw_at(&mut app);
    assert!(screen.contains("why so loud?") && cursor.is_some(), "{screen}");
}

/// The column, in characters, where `needle` starts in `line`.
fn column(line: &str, needle: &str) -> usize {
    line[..line.find(needle).unwrap()].chars().count()
}

#[test]
fn where_a_file_is_edited_is_a_choice_of_its_item_kept_per_stack() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    // The item's choices are radio buttons after it: a copy of the file at
    // first.
    key(&mut app, KeyCode::Char(' '));
    let want = items(&app).iter().position(|(l, _)| l.starts_with("Edit a.txt here")).unwrap();
    while !matches!(&app.mode, Mode::Menu(m) if m.sel == want) {
        key(&mut app, KeyCode::Down);
    }
    let screen = draw(&mut app);
    assert!(screen.contains(" Edit a.txt here in $EDITOR    [x] tmp/file  [ ] pwd  [ ] .stacked/s"), "{screen}");
    assert!(screen.lines().last().unwrap().contains("←→: where"), "{screen}");
    // → picks the next one, around the end, and ← the one before; each is
    // kept for the stack at once.
    key(&mut app, KeyCode::Right);
    assert!(draw(&mut app).contains("[ ] tmp/file  [x] pwd  [ ] .stacked/s"));
    keys(&mut app, &[KeyCode::Right, KeyCode::Right, KeyCode::Left]);
    assert!(draw(&mut app).contains("[ ] tmp/file  [ ] pwd  [x] .stacked/s"));
    assert_eq!(Db::load(&db).unwrap().choice("s", "edit"), Some("worktree"));
    assert_eq!(Db::load(&db).unwrap().choice("t", "edit"), None, "another stack's is its own");
    // ? says what each one is.
    key(&mut app, KeyCode::Char('?'));
    let screen = draw(&mut app);
    assert!(screen.contains("Press ← → to pick where: tmp/file, a copy of the file alone"), "{screen}");
    key(&mut app, KeyCode::Esc);
    // A click on a choice picks it; the menu stays open.
    let screen = draw(&mut app);
    let (y, line) = screen.lines().enumerate().find(|(_, l)| l.contains("[ ] pwd")).unwrap();
    assert_eq!(app.click(column(line, "[ ] pwd") as u16 + 1, y as u16, false), Effect::None);
    assert!(matches!(app.mode, Mode::Menu(_)));
    assert!(draw(&mut app).contains("[ ] tmp/file  [x] pwd  [ ] .stacked/s"));
    assert_eq!(Db::load(&db).unwrap().choice("s", "edit"), Some("pwd"));
    // Opened again, the stack's pick is there; Enter edits there.
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    key(&mut app, KeyCode::Char(' '));
    assert!(draw(&mut app).contains("[ ] tmp/file  [x] pwd  [ ] .stacked/s"));
    assert_eq!(choose(&mut app, "Edit a.txt here in $EDITOR"), Effect::EditCode { file: "a.txt".into(), line: 2, again: None });
}

/// Draws until the search has looked everywhere it looks: what it found,
/// each where and its line.
fn searched(app: &mut App) -> Vec<String> {
    for _ in 0..300 {
        app.refresh();
        draw(app);
        if app.searching.as_ref().is_some_and(|s| s.done) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let s = app.searching.as_ref().expect("a search open");
    assert!(s.done, "it never ended");
    s.found.iter().map(|f| format!("{} · {}", f.at, f.line)).collect()
}

#[test]
fn a_search_from_the_comments_finds_those_in_sight_first_and_goes_there() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "why so loud?");
    // One on patch 2 too, out of sight from patch 1.
    keys(&mut app, &[KeyCode::Left, KeyCode::Down, KeyCode::Right]);
    down_to(&mut app, "+bee");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "loud bees?");
    keys(&mut app, &[KeyCode::Left, KeyCode::Up]);
    // From the comments pane, it looks in the comments: patch 1's first.
    app.focus = Focus::Notes;
    choose(&mut app, "Search…");
    assert_eq!(app.screen, Screen::Search);
    assert!(draw(&mut app).contains(" ‹ comment › "), "{}", draw(&mut app));
    typed(&mut app, "loud");
    assert_eq!(searched(&mut app), ["patch 1 · you · why so loud?", "patch 2 · you · loud bees?"]);
    // Beside them, the match picked in its context: the line it is on.
    let screen = draw(&mut app);
    assert!(screen.contains("+TWO") && screen.contains("2 found"), "{screen}");
    // Enter goes there.
    keys(&mut app, &[KeyCode::Down, KeyCode::Enter]);
    assert_eq!(app.screen, Screen::Main);
    let row = &app.rows[app.cursor];
    assert!(row.head && app.sel == 1, "on patch 2's comment: {row:?}");
    // Searched again, the search before is suggested: Tab takes it.
    choose(&mut app, "Search…");
    typed(&mut app, "l");
    assert!(draw(&mut app).contains(" l") && app.suggestion().as_deref() == Some("loud"));
    keys(&mut app, &[KeyCode::Tab]);
    assert_eq!(app.searching.as_ref().unwrap().text, "loud");
    // Esc goes back where it was opened.
    keys(&mut app, &[KeyCode::Esc]);
    assert!(app.screen == Screen::Main && app.searching.is_none());
}

#[test]
fn a_search_from_the_middle_looks_in_the_patches_then_in_their_files() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    choose(&mut app, "Search…");
    assert!(draw(&mut app).contains(" ‹ patch › "));
    // A patch's message, then where it goes.
    typed(&mut app, "shout");
    assert_eq!(searched(&mut app), ["patch 1 · message · Bug 1 - shout two"]);
    keys(&mut app, &[KeyCode::Enter]);
    assert!(app.screen == Screen::Main && app.rows[app.cursor].msg_at == Some(0), "{:?}", app.rows[app.cursor]);
    // A line of a.txt as patch 1 leaves it, one its diff does not show,
    // with the lines around it; Enter goes to the patch's nearest line of
    // that file.
    choose(&mut app, "Search…");
    typed(&mut app, "line 20");
    assert_eq!(searched(&mut app), ["patch 1 · a.txt:23 · line 20"]);
    let screen = draw(&mut app);
    assert!(screen.contains("   23 line 20") && screen.contains("   22 line 19"), "{screen}");
    keys(&mut app, &[KeyCode::Enter]);
    assert_eq!(app.rows[app.cursor].path.as_deref(), Some("a.txt"));
}

#[test]
fn a_search_of_the_history_goes_to_its_record() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    draw(&mut app);
    choose(&mut app, "Drop this patch");
    choose(&mut app, "Drop it");
    keys(&mut app, &[KeyCode::Home]);
    choose(&mut app, "History…");
    let rev = app.history()[0].rev;
    choose(&mut app, "Search…");
    assert!(draw(&mut app).contains(" ‹ history › "));
    typed(&mut app, "drop");
    let found = searched(&mut app);
    assert!(found[0].starts_with(&format!("@{rev} · ")) && found[0].contains("drop"), "{found:?}");
    // ← → change where it looks, round.
    keys(&mut app, &[KeyCode::Left, KeyCode::Left, KeyCode::Left]);
    assert!(draw(&mut app).contains(" ‹ all › "));
    keys(&mut app, &[KeyCode::Right, KeyCode::Right, KeyCode::Right]);
    searched(&mut app);
    keys(&mut app, &[KeyCode::Enter]);
    assert_eq!(app.screen, Screen::History);
    assert_eq!(app.history()[app.pick].rev, rev);
}

#[test]
fn the_agents_worktrees_are_made_ahead_as_the_stack_s_checkout_says() {
    let (_tmp, db, src) = setup();
    // The checkout's own settings, over those in use: an answer's worktree
    // made ahead; a change's setting wrong, so it keeps its own (ondemand).
    std::fs::create_dir_all(src.join(".stacked")).unwrap();
    std::fs::write(src.join(".stacked/local.toml"), "[agent]\nanswer_only_worktree = \"precreate\"\nworktree = \"sometimes\"\n").unwrap();
    let mut app = open(&db).unwrap();
    let made = src.join(".stacked/s/answer-only-agent");
    for _ in 0..300 {
        app.refresh();
        if made.join(".git").exists() && !app.status.is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert!(made.join(".git").exists(), "made ahead");
    assert!(!src.join(".stacked/s/agent").exists(), "a change's is made when one is asked");
    assert!(app.status.ends_with(".stacked/local.toml: agent.worktree: \"sometimes\" is not pwd, ondemand or precreate"), "{}", app.status);
    // Removed once the editor exits.
    app.leave();
    for _ in 0..300 {
        if !made.exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert!(!made.exists(), "removed");
}

#[test]
fn the_top_bar_says_what_is_left_to_review() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    let first = |app: &mut App, w: u16| {
        let mut buf = Buffer::empty(Rect::new(0, 0, w, 30));
        app.draw(&mut buf);
        (0..w).map(|x| buf[(x, 0)].symbol()).collect::<String>()
    };
    // Nothing read yet: the 5 lines the patches change.
    let bar = first(&mut app, 120);
    assert!(bar.ends_with("   5 lines to review "), "{bar}");
    let d = Db::load(&db).unwrap();
    let t0 = d.stack("t").unwrap().time;
    let s = d.stack("s").unwrap();
    let (one, two) = (d.patch(&s.patches[0]).clone(), d.patch(&s.patches[1]).clone());
    let read = |p: &stacked::db::Patch, n: usize, time: i64| {
        stacked::db::Record::Read(stacked::db::Mark {
            change: p.change.clone(),
            keys: stacked::progress::changed_keys(p).into_iter().take(n).collect(),
            undo: None,
            time,
        })
    };
    // 2 lines read: 3 left.
    Db::append(&db, &[read(&one, 2, t0 + 600)]).unwrap();
    app.refresh();
    let bar = first(&mut app, 120);
    assert!(bar.ends_with("   3 lines to review "), "{bar}");
    // A line more, of the other patch.
    Db::append(&db, &[read(&two, 1, t0 + 1200)]).unwrap();
    app.refresh();
    let bar = first(&mut app, 120);
    assert!(bar.ends_with("   2 lines to review "), "{bar}");
    // A comment open.
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "why so loud?");
    let bar = first(&mut app, 120);
    assert!(bar.ends_with("   2 lines to review · 1 comment to close "), "{bar}");
    // With less room, less of it; with none, none.
    let bar = first(&mut app, 80);
    assert!(bar.ends_with("   2 lines · 1 comment ") && !bar.contains("to review"), "{bar}");
    let bar = first(&mut app, 50);
    assert!(!bar.contains("2 lines"), "{bar}");
    // All read, the comment closed: nothing left.
    Db::append(&db, &[read(&one, 4, t0 + 1800)]).unwrap();
    let n = notes_of(&db, "s", 0).into_iter().find(|n| n.text == "why so loud?").unwrap();
    Db::edit(&db, |d| d.set_note(Note { text: String::new(), ..n })).unwrap();
    app.refresh();
    let bar = first(&mut app, 120);
    assert!(bar.ends_with("   nothing left to review "), "{bar}");
}

/// Goes to the open menu's item `label`, and picks its choice drawn as
/// `pick` (or starting so), with →: the item is not done.
fn pick(app: &mut App, label: &str, pick: &str) {
    let all = items(app);
    let want = all.iter().position(|(l, _)| l.starts_with(label)).unwrap_or_else(|| panic!("no {label:?} in {all:?}"));
    for _ in 0..all.len() * 2 {
        match &app.mode {
            Mode::Menu(m) if m.sel == want => break,
            _ => {
                key(app, KeyCode::Down);
            }
        }
    }
    for _ in 0..choices_of(app, want).len() {
        let shown = match &app.mode {
            Mode::Menu(m) => m.items[want].choices.as_ref().map(|c| c.list[c.picked].shown.clone()),
            _ => None,
        };
        if shown.is_some_and(|s| s.starts_with(pick)) {
            return;
        }
        key(app, KeyCode::Right);
    }
    panic!("no {pick:?} among {label:?}'s choices {:?}", choices_of(app, want));
}

/// The open menu's items as drawn, a checked box before the label of those
/// that have one.
fn drawn_items(app: &App) -> Vec<String> {
    match &app.mode {
        Mode::Menu(m) => m
            .items
            .iter()
            .map(|i| match i.check {
                Some(on) => format!("{} {}", if on { "[x]" } else { "[ ]" }, i.label),
                None => i.label.clone(),
            })
            .collect(),
        _ => vec![],
    }
}

/// Stack `s`'s batched comments: each one's first line, and how it is to be
/// asked.
fn batched(db: &Path) -> Vec<(String, stacked::db::Way, bool)> {
    let d = Db::load(db).unwrap();
    d.stack_notes("s")
        .into_iter()
        .filter_map(|n| {
            let q = n.queued.as_ref().filter(|q| q.batched())?;
            Some((n.text.lines().next().unwrap_or("").to_string(), q.way, q.stack))
        })
        .collect()
}

#[test]
fn a_comment_asked_later_waits_in_the_batch() {
    use stacked::db::Way;
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    typed(&mut app, "why so loud?");
    keys(&mut app, &[KeyCode::Esc]);
    // Later, picked on the row above the ask: the ask says so.
    pick(&mut app, "Send it", "later");
    let all: Vec<String> = items(&app).into_iter().map(|(l, _)| l).collect();
    assert_eq!(all, ["Save it", "Send it", "Save it and ask agent later", "Attach a reference…", "", "Throw it away", "Keep writing"]);
    assert_eq!(
        app.help()[0],
        "Press Enter to save the comment and ask the agent (may change the code), later: it waits in the batch, which the banner sends."
    );
    choose_as(&mut app, "Save it and ask agent later", "answer only");
    assert_eq!(batched(&db), [("why so loud?".to_string(), Way::Answer, false)]);
    assert_eq!(app.status, "batched (answer only): 1 ask in the batch, which the banner sends");
    // Nothing is started: it waits for the batch to be sent.
    assert!(Db::load(&db).unwrap().runs().is_empty());
    // Kept for the stack: the next is asked later too, Enter on the row
    // above the ask asking as the ask does.
    down_to(&mut app, "+LINE 39");
    keys(&mut app, &[KeyCode::Enter]);
    typed(&mut app, "and this?");
    keys(&mut app, &[KeyCode::Esc]);
    assert!(items(&app).iter().any(|(l, _)| l == "Save it and ask agent later"), "{:?}", items(&app));
    pick(&mut app, "Save it and ask agent later", "change");
    choose(&mut app, "Send it");
    assert_eq!(batched(&db).len(), 2, "{:?}", batched(&db));
    assert!(batched(&db).contains(&("and this?".to_string(), Way::Change, false)));
    // Marked, said, and counted on the top bar.
    let screen = draw(&mut app);
    assert!(screen.lines().next().unwrap().contains(" · batch:2"), "{screen}");
    assert!(screen.contains("batched (may change the code)"), "{screen}");
    assert!(screen.contains("◇ you · a.txt: LINE 39"), "{screen}");
    // Its menu: sent now, alone, or taken out of the batch.
    keys(&mut app, &[KeyCode::Down]);
    assert!(app.rows[app.cursor].head);
    keys(&mut app, &[KeyCode::Char(' ')]);
    let all: Vec<String> = items(&app).into_iter().map(|(l, _)| l).collect();
    assert!(all.contains(&"Send it now (may change the code)".to_string()) && all.contains(&"Remove it from the batch".to_string()), "{all:?}");
    assert!(!all.iter().any(|l| l.starts_with("Ask agent")), "{all:?}");
    choose(&mut app, "Remove it from the batch");
    assert_eq!(app.status, "taken out of the batch: it is not sent");
    assert_eq!(batched(&db), [("why so loud?".to_string(), Way::Answer, false)]);
    // Asked now, as it may be once more: ready, or with the agent.
    keys(&mut app, &[KeyCode::Char(' ')]);
    pick(&mut app, "Send it", "now");
    assert!(items(&app).iter().any(|(l, _)| l == "Ask agent"), "{:?}", items(&app));
    key(&mut app, KeyCode::Esc);
    // Written again, a batched comment stays in the batch, as it is now.
    keys(&mut app, &[KeyCode::Home]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Down]);
    assert!(app.rows[app.cursor].head);
    choose_as(&mut app, "Edit your comment", "here");
    typed(&mut app, " Really?");
    keys(&mut app, &[KeyCode::Esc]);
    choose(&mut app, "Save it");
    assert_eq!(batched(&db), [("why so loud? Really?".to_string(), Way::Answer, false)]);
    // Sent now, on its own: out of the batch.
    fake_claude();
    keys(&mut app, &[KeyCode::Char(' ')]);
    choose(&mut app, "Send it now (answer only)");
    assert!(app.status.contains("asked the agent about 1 comment (answer only)"), "{}", app.status);
    assert!(batched(&db).is_empty());
    assert!(!draw(&mut app).lines().next().unwrap().contains("batch:"));
}

/// What each ask of stack `s` was started with: how, where, and how many
/// comments; once `n` are.
fn asks_made(db: &Path, n: usize) -> Vec<(stacked::db::Way, stacked::ask::Scope, usize)> {
    for _ in 0..300 {
        let d = Db::load(db).unwrap();
        let all: Vec<_> = d.runs().into_iter().filter_map(stacked::ask::info).map(|i| (i.way, i.scope, i.notes.len())).collect();
        if all.len() >= n {
            return all;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    panic!("not {n} asks made");
}

#[test]
fn the_banner_sends_the_batch_its_edits_to_one_agent_its_answers_to_one_or_each_its_own() {
    use stacked::ask::Scope;
    use stacked::db::Way;
    let (_tmp, db, _src) = setup();
    fake_claude();
    let mut app = open(&db).unwrap();
    let later = |app: &mut App, line: &str, text: &str, how: &str| {
        keys(app, &[KeyCode::Home]);
        down_to(app, line);
        keys(app, &[KeyCode::Enter]);
        typed(app, text);
        keys(app, &[KeyCode::Esc]);
        pick(app, "Send it", "later");
        choose_as(app, "Save it and ask agent later", how);
    };
    // Batched: an edit on each patch, two questions on the first; and one
    // not asked yet.
    keys(&mut app, &[KeyCode::Right]);
    later(&mut app, "+TWO", "shout less", "change");
    later(&mut app, "-two", "why so loud?", "answer only");
    later(&mut app, "+LINE 39", "and here?", "answer only");
    keys(&mut app, &[KeyCode::Home]);
    down_to(&mut app, "-line 39");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "hmm");
    keys(&mut app, &[KeyCode::Left, KeyCode::Down, KeyCode::Right]);
    later(&mut app, "+bee", "a bee?", "change");
    // The banner says it, and lists it under what landed.
    let screen = draw(&mut app);
    assert!(screen.lines().nth(1).unwrap().starts_with(" ◇ 4 batched asks · 5 open comments"), "{screen}");
    keys(&mut app, &[KeyCode::Esc]);
    assert!(app.on_banner && app.banner_sel == 0);
    let screen = draw(&mut app);
    assert!(screen.lines().nth(2).unwrap().starts_with(" open comments"), "{screen}");
    assert!(screen.lines().nth(3).unwrap().starts_with("   ◇ Send the batch… 2 edits, 2 answers only"), "{screen}");
    assert!(app.hint().contains("enter: pick what to send"), "{}", app.hint());
    // Enter on it: the whole menu, on what sends the edits; then every ask,
    // checked if batched, in the order they were written.
    keys(&mut app, &[KeyCode::Enter]);
    let all = drawn_items(&app);
    assert_eq!(
        all[..9],
        [
            "Send 2 edits to a single agent",
            "Send 2 answers only to",
            "",
            "[x] patch 1 · a.txt: TWO · \"shout less\" · may change the code",
            "[x] patch 1 · a.txt: two · \"why so loud?\" · answer only",
            "[x] patch 1 · a.txt: LINE 39 · \"and here?\" · answer only",
            "[ ] patch 1 · a.txt: line 39 · \"hmm\"",
            "[x] patch 2 · b.txt: bee · \"a bee?\" · may change the code",
            "",
        ],
        "{all:?}"
    );
    assert!(all[9..14].iter().all(|l| l.starts_with("Go to ")) && all[14].is_empty(), "{all:?}");
    assert!(matches!(&app.mode, Mode::Menu(m) if m.sel == 0));
    assert_eq!(choices_named(&app, "Send 2 answers only to"), ["a single agent", "fan out"]);
    assert_eq!(app.hint(), "↑↓ choose · enter: do it · esc: close · ?: what it does");
    // Unchecked, out of the batch; checked again, back as it was. The menu
    // stays, on it.
    keys(&mut app, &[KeyCode::Down, KeyCode::Down, KeyCode::Down]);
    assert_eq!(app.hint(), "↑↓ choose · enter: check or uncheck it · esc: close · ?: what it does");
    keys(&mut app, &[KeyCode::Enter]);
    let all = drawn_items(&app);
    assert_eq!(all[1], "Send 1 answer only to", "{all:?}");
    assert_eq!(all[4], "[ ] patch 1 · a.txt: two · \"why so loud?\"", "{all:?}");
    assert!(matches!(&app.mode, Mode::Menu(m) if m.sel == 4));
    keys(&mut app, &[KeyCode::Enter]);
    assert_eq!(drawn_items(&app)[4], "[x] patch 1 · a.txt: two · \"why so loud?\" · answer only");
    // Checked, one not asked yet goes in as the stack's asks are made.
    keys(&mut app, &[KeyCode::Down, KeyCode::Down]);
    keys(&mut app, &[KeyCode::Enter]);
    assert_eq!(drawn_items(&app)[6], "[x] patch 1 · a.txt: line 39 · \"hmm\" · may change the code");
    keys(&mut app, &[KeyCode::Enter]);
    // The edits to a single agent: one ask, of the whole stack, as they are
    // on two patches.
    choose(&mut app, "Send 2 edits to a single agent");
    assert_eq!(app.status, "sent the batch (may change the whole stack): 2 comments to a single agent");
    assert_eq!(asks_made(&db, 1), [(Way::Change, Scope::Stack, 2)]);
    assert_eq!(batched(&db).len(), 2, "the answers wait in the batch");
    // The answers fanned out, from the banner still: an ask each.
    assert!(app.on_banner && app.mode == Mode::Normal);
    keys(&mut app, &[KeyCode::Char(' ')]);
    pick(&mut app, "Send 2 answers only to", "fan out");
    keys(&mut app, &[KeyCode::Enter]);
    assert!(app.status.starts_with("sent the batch (answer only): 2 questions, an agent each"), "{}", app.status);
    assert!(batched(&db).is_empty());
    let mut all = asks_made(&db, 3);
    all.sort_by_key(|(w, _, n)| (*w == Way::Change, *n));
    let one = (Way::Answer, Scope::Patch(notes_of(&db, "s", 0)[0].change.clone()), 1);
    assert_eq!(all[..2], [one.clone(), one]);
}

#[test]
fn with_work_landed_and_a_batch_the_banner_s_menu_has_all_of_it() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "-two");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "why so loud?");
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    typed(&mut app, "shout less");
    keys(&mut app, &[KeyCode::Esc]);
    pick(&mut app, "Send it", "later");
    choose_as(&mut app, "Save it and ask agent later", "change");
    // The agent's answer to the first, landed.
    let n = notes_of(&db, "s", 0).into_iter().find(|n| n.text == "why so loud?").unwrap();
    held(&db, "run-answer", &n.change, &n.id, "It is a shout.", false);
    app.refresh();
    // On the banner: what landed on top, the batch under it.
    keys(&mut app, &[KeyCode::Esc]);
    let screen = draw(&mut app);
    let lines: Vec<&str> = screen.lines().collect();
    assert!(lines[1].starts_with(" ⟳ 1 landed ask waits for your review") && lines[1].contains(" · 1 batched ask · "), "{screen}");
    assert!(lines[2].starts_with(" read only, to review") && lines[3].starts_with("   ⟳ Review 1 reply"), "{screen}");
    assert!(lines[4].starts_with(" open comments") && lines[5].starts_with("   ◇ Send the batch… 1 edit"), "{screen}");
    // Enter on the batch, or Space anywhere: the whole menu, what landed and
    // the batch.
    keys(&mut app, &[KeyCode::Down, KeyCode::Enter]);
    let all = drawn_items(&app);
    assert!(all[0].starts_with("Review 1 reply"), "{all:?}");
    assert_eq!(all[1..5], ["", "Send 1 edit to a single agent", "", "[x] patch 1 · a.txt: TWO · \"shout less\" · may change the code"], "{all:?}");
    assert!(matches!(&app.mode, Mode::Menu(m) if m.sel == 2), "on what sends the batch");
    keys(&mut app, &[KeyCode::Esc, KeyCode::Up, KeyCode::Char(' ')]);
    assert_eq!(drawn_items(&app), all);
    assert!(matches!(&app.mode, Mode::Menu(m) if m.sel == 0));
}

/// What an edit by hand of patch 1 of stack `s` lands, as $EDITOR would
/// have it: `TWO` made `Two`; in place of landed work `work`, if given.
fn hand_edit(db: &Path, work: Option<&str>) -> String {
    Db::edit(db, |d| {
        let s = match work {
            Some(w) => d.run(w).and_then(|r| r.held.as_ref()?.stack.as_ref()).map(|c| c.patches.clone()).unwrap(),
            None => d.stack("s")?.patches.clone(),
        };
        let old = d.patch(&s[0]).clone();
        let draft = stacked::mbox::Draft {
            diff: old.diff.replace("+TWO", "+Two"),
            ..stacked::mbox::Draft::from(&old)
        };
        let id = d.add_patch(stacked::ops::revise(&old, draft))?;
        let mut patches = s.clone();
        patches[0] = id.clone();
        let op = format!("edit {} as {}", &old.id[..8], &id[..8]);
        stacked::held::hold_by_hand(d, "s", patches, &op, "patch 1", "edited a.txt in Bug 1 - shout two", work)
    })
    .unwrap()
}

#[test]
fn a_patch_whose_message_alone_changed_is_reworded_in_the_stack_operations() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    let run = Db::edit(&db, |d| {
        let s = d.stack("s")?.patches.clone();
        let old = d.patch(&s[0]).clone();
        let draft = stacked::mbox::Draft {
            message: "Bug 1 - shout two, louder\n\nBecause.".into(),
            ..stacked::mbox::Draft::from(&old)
        };
        let mut patches = s.clone();
        patches[0] = d.add_patch(stacked::ops::revise(&old, draft))?;
        stacked::held::hold_by_hand(d, "s", patches, "reword", "patch 1", "reworded Bug 1", None)
    })
    .unwrap();
    app.edited(stacked::ops::Edited::Landed { said: "reworded Bug 1".into(), run }, None, None);
    let screen = draw(&mut app);
    assert!(screen.contains("reword  1 Bug 1 - shout two, louder") && !screen.contains("modify  1"), "{screen}");
}

#[test]
fn an_edit_by_hand_lands_and_its_review_opens_at_once() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    let before = Db::load(&db).unwrap().stack("s").unwrap().patches.clone();
    // Saved in $EDITOR, it lands; the review opens on it.
    let run = hand_edit(&db, None);
    app.edited(stacked::ops::Edited::Landed { said: "edited a.txt in Bug 1 - shout two".into(), run: run.clone() }, None, None);
    assert_eq!(app.screen, Screen::Review);
    assert_eq!(app.status, "edited a.txt in Bug 1 - shout two: it waits for your review");
    let screen = draw(&mut app);
    assert!(screen.contains(" your edit by hand: a change of patch 1 · edited"), "{screen}");
    assert!(screen.contains(" stack operations · to review") && screen.contains("modify  1 Bug 1 - shout two"), "{screen}");
    assert!(screen.contains("+Two"), "what it does to the stack:\n{screen}");
    assert_eq!(Db::load(&db).unwrap().stack("s").unwrap().patches, before, "nothing changed yet");
    // Left for later: the banner says it, and the stack is not edited by
    // hand meanwhile.
    keys(&mut app, &[KeyCode::Esc]);
    choose(&mut app, "Save");
    assert_eq!(app.screen, Screen::Main);
    let screen = draw(&mut app);
    assert!(
        screen.contains(" ⟳ 1 edit by hand waits for your review · the next: a change of patch 1, by hand: edited a.txt in Bug 1 - shout two"),
        "{screen}"
    );
    keys(&mut app, &[KeyCode::Right]);
    keys(&mut app, &[KeyCode::Char(' ')]);
    assert!(items(&app).contains(&("Edit this patch in $EDITOR".into(), false)), "{:?}", items(&app));
    assert!(
        matches!(&app.mode, Mode::Menu(m) if m.note.as_deref().is_some_and(|n| n.starts_with("your edit by hand of patch 1 waits for your review"))),
        "{:?}",
        app.mode
    );
    key(&mut app, KeyCode::Esc);
    // Accepted, it is in the stack.
    keys(&mut app, &[KeyCode::Esc]);
    assert!(app.on_banner);
    keys(&mut app, &[KeyCode::Enter]);
    assert_eq!(app.screen, Screen::Review);
    keys(&mut app, &[KeyCode::Esc]);
    choose(&mut app, "Accept all remaining and Close");
    choose(&mut app, "Accept them and close");
    assert_eq!(app.status, "reviewed: its change accepted");
    let d = Db::load(&db).unwrap();
    let s = d.stack("s").unwrap();
    assert_ne!(s.patches[0], before[0]);
    assert!(d.patch(&s.patches[0]).diff.contains("+Two"));
    assert_eq!(s.op, d.run(&run).unwrap().held.as_ref().unwrap().stack.as_ref().unwrap().op);
}

#[test]
fn a_reference_the_agent_attached_is_a_line_under_the_comment_previewed_then_gone_to() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "why so loud?");
    let n = notes_of(&db, "s", 0).remove(0);
    // The agent answers, pointing at a.txt as patch 1 leaves it: a line its
    // diff has, and lines it has not.
    Db::edit(&db, |d| {
        let n = d.note(&n.change, &n.id).cloned().unwrap();
        let said = "It shouts there.\n\n```refs\na.txt:2 @1\na.txt:20-21 @1\n```";
        d.set_note(Note {
            text: append_turn(&n.text, Role::Agent, said, Some("answer")),
            ..n
        })
    })
    .unwrap();
    app.refresh();
    let screen = draw(&mut app);
    assert!(screen.contains("⇢ a.txt:2 · as patch 1 leaves it") && screen.contains("⇢ a.txt:20-21 · as patch 1 leaves it"), "{screen}");
    assert!(screen.contains("It shouts there.") && !screen.contains("```refs") && !screen.contains("a.txt:2 @1"), "{screen}");
    // Enter on the first: its line, previewed.
    down_to(&mut app, "     ▎   ⇢ a.txt:2 ");
    keys(&mut app, &[KeyCode::Enter]);
    let Mode::Preview(p) = &app.mode else { panic!("{:?}", app.mode) };
    assert_eq!(p.title, "a.txt:2 · as patch 1 leaves it");
    assert_eq!(p.lines.iter().map(|(l, _)| l.as_str()).collect::<Vec<_>>(), ["    2 │ TWO"]);
    assert!(draw(&mut app).contains("    2 │ TWO"));
    assert_eq!(app.hint(), "↑↓: scroll · enter: go there · esc: back");
    // Enter again: there, its line in patch 1's diff.
    keys(&mut app, &[KeyCode::Enter]);
    assert!(matches!(app.mode, Mode::Normal), "{:?}", app.mode);
    assert!(app.rows[app.cursor].text.starts_with("+TWO"), "{}", app.rows[app.cursor].text);
    // The other: lines patch 1's diff has not; Esc, back; Enter again, the
    // file whole, there.
    down_to(&mut app, "     ▎   ⇢ a.txt:20-21");
    keys(&mut app, &[KeyCode::Enter]);
    let Mode::Preview(p) = &app.mode else { panic!("{:?}", app.mode) };
    assert_eq!(p.lines.iter().map(|(l, _)| l.as_str()).collect::<Vec<_>>(), ["   20 │ line 17", "   21 │ line 18"]);
    keys(&mut app, &[KeyCode::Esc]);
    assert!(matches!(app.mode, Mode::Normal), "{:?}", app.mode);
    keys(&mut app, &[KeyCode::Enter, KeyCode::Enter]);
    assert_eq!(app.screen, Screen::Text);
    let t = app.text.as_ref().unwrap();
    assert_eq!(t.title, "a.txt, as patch 1 leaves it");
    assert_eq!(t.lines[19].0, "   20 │ line 17");
    assert_eq!(t.scroll, 16);
}

#[test]
fn leaving_a_review_picks_whether_the_asking_comment_closes_above_accept_and_reject_all() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "why so loud?");
    let n = notes_of(&db, "s", 0).remove(0);
    agent_changed(&db, "run-agent", &n);
    app.refresh();
    keys(&mut app, &[KeyCode::Esc, KeyCode::Enter]);
    assert_eq!(app.screen, Screen::Review);
    // Marked to close in the review: so the menu has it.
    keys(&mut app, &[KeyCode::Home, KeyCode::Enter, KeyCode::Esc]);
    let row = |app: &App| {
        let Mode::Menu(m) = &app.mode else { panic!("{:?}", app.mode) };
        let at = m.items.iter().position(|i| i.label == "Asking comment").expect("the row");
        (at, m.sel, m.items[at].choices.as_ref().unwrap().pick(), m.items.iter().map(|i| (i.label.clone(), i.action.clone())).collect::<Vec<_>>())
    };
    let (at, _, pick, all) = row(&app);
    assert_eq!(pick, "close");
    // Accept and Reject all, under it, in a section of their own.
    assert_eq!(all[at - 1].1, stacked::tui::Action::Line);
    assert_eq!(all[at + 1].0, "Accept all remaining and Close");
    assert_eq!(all[at + 2].0, "Reject all remaining and Close");
    assert_eq!(all[at + 3].1, stacked::tui::Action::Line);
    // ← keeps it: in the review too, its reply as it was.
    while row(&app).1 != at {
        keys(&mut app, &[KeyCode::Down]);
    }
    keys(&mut app, &[KeyCode::Left]);
    assert_eq!(row(&app).2, "keep");
    let marks = app.reviewing.as_ref().unwrap().marks.clone();
    assert!(!marks.contains_key("asking 0") && !marks.contains_key("reply 0"), "{marks:?}");
    // Enter picks the other, the menu staying.
    keys(&mut app, &[KeyCode::Enter]);
    assert_eq!(row(&app).2, "close");
    assert_eq!(app.reviewing.as_ref().unwrap().marks.get("asking 0"), Some(&true));
    // Accept all: closed with the review.
    choose(&mut app, "Accept all remaining and Close");
    choose(&mut app, "Accept them and close");
    assert_eq!(app.screen, Screen::Main);
    assert!(!notes_of(&db, "s", 0).iter().any(|m| m.id == n.id), "the comment that asked is closed");
}

#[test]
fn the_review_closes_with_what_remains_marked_only_while_something_remains() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    let run = hand_edit(&db, None);
    app.edited(stacked::ops::Edited::Landed { said: "edited a.txt in Bug 1 - shout two".into(), run }, None, None);
    assert_eq!(app.screen, Screen::Review);
    let closing = |app: &App| items(app).into_iter().filter(|(l, _)| l.contains("Close")).collect::<Vec<_>>();
    // Its change not marked yet: closed with it accepted, or rejected.
    keys(&mut app, &[KeyCode::Esc]);
    assert_eq!(
        closing(&app),
        [("Close review", false), ("Accept all remaining and Close", true), ("Reject all remaining and Close", true)].map(|(l, ok)| (l.to_string(), ok))
    );
    // Asked first, what is left said; the menu on Keep reviewing, which
    // goes back to the review, nothing closed.
    choose(&mut app, "Reject all remaining and Close");
    let Mode::Menu(m) = &app.mode else { panic!("{:?}", app.mode) };
    assert_eq!(m.title, "reject every part not marked yet?");
    assert_eq!(items(&app), [("Reject them and close".to_string(), true), ("Keep reviewing".to_string(), true)]);
    assert_eq!(m.sel, 1);
    assert_eq!(
        m.note.as_deref(),
        Some("Not marked yet: the change of the stack. The review closes then: what you rejected is dropped for good, and what you accepted is applied.")
    );
    keys(&mut app, &[KeyCode::Enter]);
    assert!(matches!(app.mode, Mode::Normal) && app.screen == Screen::Review, "{:?}", app.mode);
    keys(&mut app, &[KeyCode::Esc]);
    choose(&mut app, "Accept all remaining");
    assert!(matches!(&app.mode, Mode::Menu(m) if m.title == "accept every part not marked yet?"), "{:?}", app.mode);
    choose(&mut app, "Keep reviewing");
    assert_eq!(stacked::held::queue(&Db::load(&db).unwrap(), "s").len(), 1, "not closed");
    keys(&mut app, &[KeyCode::Esc]);
    // Marked, it is closed as it is marked; nothing remains for the others.
    choose(&mut app, "Keep reviewing");
    keys(&mut app, &[KeyCode::Enter, KeyCode::Esc]);
    assert_eq!(
        closing(&app),
        [("Close review", true), ("Accept all remaining and Close", false), ("Reject all remaining and Close", false)].map(|(l, ok)| (l.to_string(), ok))
    );
    choose(&mut app, "Close review");
    assert_eq!(app.status, "reviewed: its change accepted");
}

/// An edit by hand of patch 1 of `s` waiting for review, its review open,
/// the editor run in the checkout the stack was imported from: on main,
/// HEAD is in sync with `s`.
fn reviewing_with_head(prepare: impl FnOnce(&Path)) -> (tempfile::TempDir, PathBuf, PathBuf, App) {
    let (tmp, db, src) = setup();
    prepare(&src);
    let mut app = open(&db).unwrap();
    app.checkout = src.clone();
    app.look_at_head();
    app.settle();
    let run = hand_edit(&db, None);
    app.edited(stacked::ops::Edited::Landed { said: "edited a.txt in Bug 1 - shout two".into(), run }, None, None);
    assert_eq!(app.screen, Screen::Review);
    app.settle();
    (tmp, db, src, app)
}

/// The commit `rev` is in the checkout at `src`.
fn rev_in(src: &Path, rev: &str) -> String {
    let out = Command::new("git").args(["rev-parse", rev]).current_dir(src).output().unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[test]
fn closing_an_edit_review_moves_head_along_when_it_was_in_sync_with_the_stack() {
    let (_tmp, db, src, mut app) = reviewing_with_head(|_| {});
    let was = rev_in(&src, "HEAD");
    keys(&mut app, &[KeyCode::Esc]);
    let row = |app: &App| {
        let Mode::Menu(m) = &app.mode else { panic!("{:?}", app.mode) };
        let k = m.items.iter().position(|i| i.label == "HEAD").expect("the row");
        (k, m.items[k].choices.as_ref().unwrap().pick(), m.items.iter().map(|i| i.label.clone()).collect::<Vec<_>>())
    };
    // HEAD is in sync with the stack: the row, sync picked, over Accept and
    // Reject all.
    let (k, picked, all) = row(&app);
    assert_eq!(picked, "sync");
    assert_eq!(all[k + 1], "Accept all remaining and Close");
    let screen = draw(&mut app);
    assert!(screen.contains("HEAD") && screen.contains("[x] sync  [ ] no sync"), "{screen}");
    // ← → pick the other, kept for the review; Enter too, the menu staying.
    pick(&mut app, "HEAD", "no sync");
    assert!(app.reviewing.as_ref().unwrap().stay);
    keys(&mut app, &[KeyCode::Enter]);
    assert_eq!(row(&app).1, "sync");
    assert!(!app.reviewing.as_ref().unwrap().stay);
    assert!(matches!(app.mode, Mode::Menu(_)), "{:?}", app.mode);
    // Not kept for the stack.
    assert_eq!(Db::load(&db).unwrap().choice("s", "head"), None);
    // Accepted: HEAD at the top of the stack as accepted, detached there, main
    // as it was.
    choose(&mut app, "Accept all remaining and Close");
    choose(&mut app, "Accept them and close");
    app.settle();
    assert_eq!(app.status, "reviewed: its change accepted; HEAD is at patch 2 of s, detached: main is as it was");
    assert_eq!(rev_in(&src, "main"), was);
    assert!(std::fs::read_to_string(src.join("a.txt")).unwrap().starts_with("one\nTwo\nthree\n"));
    // In sync with it again.
    app.look_at_head();
    app.settle();
    let screen = draw(&mut app);
    assert_eq!(at(&screen, &format!("▶ HEAD {}", &rev_in(&src, "HEAD")[..8])).1, at(&screen, "  2 Bug 2").1 + 1, "{screen}");
}

#[test]
fn closing_an_edit_review_leaves_head_where_it_is_when_picked_so_or_when_it_cannot_move_safely() {
    let labels = |app: &App| items(app).into_iter().map(|(l, _)| l).collect::<Vec<_>>();
    // No sync picked: HEAD stays.
    let (_tmp, _db, src, mut app) = reviewing_with_head(|_| {});
    let was = rev_in(&src, "HEAD");
    keys(&mut app, &[KeyCode::Esc]);
    pick(&mut app, "HEAD", "no sync");
    choose(&mut app, "Accept all remaining and Close");
    choose(&mut app, "Accept them and close");
    app.settle();
    assert_eq!(app.status, "reviewed: its change accepted");
    assert_eq!(rev_in(&src, "HEAD"), was);
    // HEAD on patch 1 only: not in sync, no row.
    let (_tmp, _db, src, mut app) = reviewing_with_head(|src| git(src, &["checkout", "-q", "--detach", "HEAD~1"]));
    keys(&mut app, &[KeyCode::Esc]);
    assert!(!labels(&app).contains(&"HEAD".to_string()), "{:?}", labels(&app));
    keys(&mut app, &[KeyCode::Esc]);
    // A commit of yours on top of the stack: not in sync either.
    git(&src, &["checkout", "-q", "main"]);
    std::fs::write(src.join("c.txt"), "c\n").unwrap();
    git(&src, &["add", "c.txt"]);
    git(&src, &["commit", "-qm", "Mine"]);
    app.look_at_head();
    app.settle();
    keys(&mut app, &[KeyCode::Esc]);
    assert!(!labels(&app).contains(&"HEAD".to_string()), "{:?}", labels(&app));
    keys(&mut app, &[KeyCode::Esc]);
    // In sync, but with a change of yours not committed: no row.
    let (_tmp, _db, src, mut app) = reviewing_with_head(|_| {});
    std::fs::write(src.join("b.txt"), "bee, edited\n").unwrap();
    app.look_at_head();
    app.settle();
    keys(&mut app, &[KeyCode::Esc]);
    assert!(!labels(&app).contains(&"HEAD".to_string()), "{:?}", labels(&app));
    keys(&mut app, &[KeyCode::Esc]);
    // Committed: the row is back; the change marked rejected, it is gone.
    git(&src, &["checkout", "-q", "b.txt"]);
    app.look_at_head();
    app.settle();
    keys(&mut app, &[KeyCode::Esc]);
    assert!(labels(&app).contains(&"HEAD".to_string()), "{:?}", labels(&app));
    keys(&mut app, &[KeyCode::Esc]);
    choose(&mut app, "Mark as rejected");
    keys(&mut app, &[KeyCode::Esc]);
    assert!(!labels(&app).contains(&"HEAD".to_string()), "{:?}", labels(&app));
    // The agent makes its changes in your checkout, which moves HEAD itself:
    // no row.
    let (_tmp, _db, _src, mut app) = reviewing_with_head(|src| {
        std::fs::create_dir_all(src.join(".stacked")).unwrap();
        std::fs::write(src.join(".stacked/local.toml"), "[agent]\nworktree = \"pwd\"\n").unwrap();
    });
    keys(&mut app, &[KeyCode::Esc]);
    assert!(!labels(&app).contains(&"HEAD".to_string()), "{:?}", labels(&app));
    // Not run in a checkout of the stack's repository: no HEAD, no row.
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    let run = hand_edit(&db, None);
    app.edited(stacked::ops::Edited::Landed { said: "edited a.txt in Bug 1 - shout two".into(), run }, None, None);
    app.settle();
    keys(&mut app, &[KeyCode::Esc]);
    assert!(!labels(&app).contains(&"HEAD".to_string()), "{:?}", labels(&app));
}

#[test]
fn landed_work_is_edited_by_hand_from_its_review_in_its_place() {
    use stacked::tui::WorkEdit;
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "why so loud?");
    let n = notes_of(&db, "s", 0).into_iter().find(|n| n.text == "why so loud?").unwrap();
    // The agent's change of the stack, and its reply, landed.
    held(&db, "run-agent", &n.change, &n.id, "Quieter now.", true);
    app.refresh();
    keys(&mut app, &[KeyCode::Esc, KeyCode::Enter]);
    assert_eq!(app.screen, Screen::Review);
    down_to_review(&mut app, "Quieter now.");
    keys(&mut app, &[KeyCode::Enter]);
    // On a patch of the stack it makes: that patch edited by hand, as mail
    // or a file of it; and the stack as a list.
    keys(&mut app, &[KeyCode::Home]);
    down_to_review(&mut app, "noop    1 Bug 1");
    keys(&mut app, &[KeyCode::Char(' ')]);
    let all: Vec<String> = items(&app).into_iter().map(|(l, _)| l).collect();
    for l in ["Edit this patch in $EDITOR", "Edit a.txt in $EDITOR", "Edit the stack as a list in $EDITOR"] {
        assert!(all.contains(&l.to_string()), "{l:?} in {all:?}");
    }
    assert!(matches!(&app.mode, Mode::Menu(m) if m.note.is_none()), "nothing said to hold the edits back: {:?}", app.mode);
    let run = "run-agent".to_string();
    assert_eq!(choose(&mut app, "Edit this patch in $EDITOR"), Effect::EditWork { run: run.clone(), edit: WorkEdit::Mail(0) });
    let edit = WorkEdit::Code {
        at: 0,
        file: "a.txt".into(),
        line: 2,
        again: None,
    };
    assert_eq!(choose(&mut app, "Edit a.txt in $EDITOR"), Effect::EditWork { run: run.clone(), edit });
    // Saved, it lands in its place: that work is rejected, the new holds its
    // reply as it was marked, and the review goes on there.
    let new = hand_edit(&db, Some(&run));
    app.edited(stacked::ops::Edited::Landed { said: "edited a.txt in Bug 1 - shout two".into(), run: new.clone() }, None, Some(run.clone()));
    assert_eq!(app.screen, Screen::Review);
    assert_eq!(app.reviewing.as_ref().unwrap().run, new);
    assert_eq!(app.status, "edited a.txt in Bug 1 - shout two: it lands in place of the work you were reviewing, which is rejected");
    let screen = draw(&mut app);
    assert!(screen.contains(" the agent's work: a change of patch 1, 1 reply"), "{screen}");
    assert!(screen.contains(" stack operations · to review") && screen.contains("modify  1 Bug 1 - shout two"), "{screen}");
    assert!(screen.contains("✓ a.txt: TWO · Quieter now."), "its mark kept:\n{screen}");
    let d = Db::load(&db).unwrap();
    assert!(d.review(&run).unwrap().closed);
    assert_eq!(d.run(&run).unwrap().summary, format!("reviewed: rejected for your edit of it by hand, run {new}"));
    let queue: Vec<String> = stacked::held::queue(&d, "s").into_iter().map(|r| r.id.clone()).collect();
    assert_eq!(queue, std::slice::from_ref(&new));
    let h = d.run(&new).unwrap().held.clone().unwrap();
    assert_eq!(h.edits, ["edited a.txt in Bug 1 - shout two"]);
    assert!(h.stack.unwrap().op.starts_with("agent: patch 1; edit "));
    drop(d);
    // Accepted: the agent's change with your edit in it, and its reply.
    keys(&mut app, &[KeyCode::Esc]);
    choose(&mut app, "Accept all remaining and Close");
    choose(&mut app, "Accept them and close");
    let d = Db::load(&db).unwrap();
    assert!(d.patch(&d.stack("s").unwrap().patches[0]).diff.contains("+Two"));
    assert!(d.note(&n.change, &n.id).unwrap().text.contains("Quieter now."));
}

/// What the agent brought back for comment `n` on patch 1, landed: `TWO`
/// made `Two`, and its reply.
fn agent_changed(db: &Path, run: &str, n: &Note) {
    agent_changed_to(db, run, n, None)
}

/// As `agent_changed`, patch 1's message made `message` too, if given.
fn agent_changed_to(db: &Path, run: &str, n: &Note, message: Option<&str>) {
    Db::edit(db, |d| {
        let (from, s) = d.head("s").map(|(r, s)| (r, s.clone())).unwrap();
        let old = d.patch(&s.patches[0]).clone();
        let draft = stacked::mbox::Draft {
            diff: old.diff.replace("+TWO", "+Two"),
            message: message.map_or(old.message.clone(), String::from),
            ..stacked::mbox::Draft::from(&old)
        };
        let mut patches = s.patches.clone();
        patches[0] = d.add_patch(stacked::ops::revise(&old, draft))?;
        let ask = serde_json::json!({"way": "change", "scope": {"patch": n.change}, "from": from, "notes": [{"label": "c1", "change": n.change, "id": n.id}]});
        d.add_run(stacked::db::Run {
            id: run.into(),
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
                    op: "agent: patch 1".into(),
                    of: "patch 1".into(),
                    from: None,
                }),
                replies: vec![stacked::held::Reply {
                    change: n.change.clone(),
                    id: n.id.clone(),
                    text: "Quieter now.".into(),
                    tag: "change".into(),
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

/// The row the cursor is on in the review's right pane, as drawn: of the
/// patch, or of the stack diff; the spaces around it gone.
fn work_row(app: &mut App) -> Option<String> {
    let mut buf = Buffer::empty(Rect::new(0, 0, 120, 30));
    app.draw(&mut buf);
    (1..30).find_map(|y| {
        let on = buf[(49, y)].modifier.contains(ratatui::style::Modifier::REVERSED);
        on.then(|| (49..120).map(|x| buf[(x, y)].symbol()).collect::<String>().trim().to_string())
    })
}

/// Down the patch, or the stack diff, the review's right pane shows until
/// its cursor is on the row starting with `text`.
fn down_to_work_row(app: &mut App, text: &str) {
    for _ in 0..80 {
        if work_row(app).is_some_and(|r| r.starts_with(text)) {
            return;
        }
        key(app, KeyCode::Down);
    }
    panic!("no row {text:?} in the patch:\n{}", draw(app));
}

#[test]
fn what_you_wrote_in_a_review_is_in_its_list_to_see_write_again_or_take_back() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "why so loud?");
    let n = notes_of(&db, "s", 0).remove(0);
    agent_changed(&db, "run-agent", &n);
    app.refresh();
    keys(&mut app, &[KeyCode::Esc, KeyCode::Enter]);
    assert_eq!(app.screen, Screen::Review);
    // A comment written in the review, on the line the work changed; and an
    // answer to its reply.
    keys(&mut app, &[KeyCode::Home]);
    down_to_review(&mut app, "modify  1 Bug 1");
    keys(&mut app, &[KeyCode::Right, KeyCode::Enter, KeyCode::Enter]);
    save(&mut app, "Not like this either.");
    keys(&mut app, &[KeyCode::Left, KeyCode::Left]);
    down_to_review(&mut app, "Quieter now.");
    choose(&mut app, "Reply to it");
    save(&mut app, "Still too loud.");
    // The cursor goes on the comment: the right pane shows it where it is,
    // under its line of the patch as the work leaves it.
    down_to_review(&mut app, "Not like this either.");
    let screen = draw(&mut app);
    assert!(screen.contains(" your comment, where it is on the patch as the work leaves it"), "{screen}");
    let (_, line) = at_from(&screen, "+Two", 49);
    let (_, comment) = at_from(&screen, "Not like this either.", 49);
    assert!(comment > line, "under its line:\n{screen}");
    assert_eq!(app.hint(), "↑↓ choose · enter: edit it · →: look closer · space: menu · esc: send back, save or discard");
    // Enter writes it again.
    keys(&mut app, &[KeyCode::Enter]);
    assert!(matches!(&app.mode, Mode::Edit(e) if e.ed.text() == "Not like this either."), "{:?}", app.mode);
    save(&mut app, " Really.");
    assert!(notes_of(&db, "s", 0).iter().any(|m| m.text == "Not like this either. Really."));
    // Space takes it back: closed, it no longer goes back with the work.
    keys(&mut app, &[KeyCode::Char(' ')]);
    let all = items(&app);
    assert!(all.contains(&("Edit it".into(), true)) && all.contains(&("Take it back".into(), true)), "{all:?}");
    choose(&mut app, "Take it back");
    assert!(!notes_of(&db, "s", 0).iter().any(|m| m.text.contains("Not like this either.")));
    assert!(!draw(&mut app).contains("Not like this either."));
    // The cursor on what is left: the answer, after the reply in its thread.
    assert!(selected_line(&mut app).is_some_and(|l| l.contains("your answer on")), "{:?}", selected_line(&mut app));
    let screen = draw(&mut app);
    assert!(screen.contains(" your answer, after the work's reply, as it would be in its thread"), "{screen}");
    let (_, reply) = at_from(&screen, "Quieter now.", 49);
    let (_, answer) = at_from(&screen, "Still too loud.", 49);
    assert!(answer > reply, "{screen}");
    choose(&mut app, "Take your answer back");
    let d = Db::load(&db).unwrap();
    assert_eq!(d.run("run-agent").unwrap().held.as_ref().unwrap().replies[0].answer, None);
    drop(d);
    // Nothing written: the review closes again, not sent back.
    assert!(!draw(&mut app).contains(" what you wrote, to send back"));
    keys(&mut app, &[KeyCode::Esc]);
    let labels: Vec<String> = items(&app).into_iter().map(|(l, _)| l).collect();
    assert!(!labels.iter().any(|l| l == "Send it back to the agent"), "{labels:?}");
}

#[test]
fn the_patch_in_a_review_has_the_comments_of_the_work_and_no_other() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    // On patch 1, a comment the agent is asked about, and one it is not.
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "why so loud?");
    down_to(&mut app, "+LINE 39");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "Not asked about.");
    let n = notes_of(&db, "s", 0).into_iter().find(|n| n.text.contains("why so loud?")).unwrap();
    agent_changed(&db, "run-agent", &n);
    // The work opens one of its own, on the patch's title.
    Db::edit(&db, |d| {
        let mut r = d.run("run-agent").unwrap().clone();
        let h = r.held.as_mut().unwrap();
        let p1 = d.patch(&h.stack.as_ref().unwrap().patches[0]).clone();
        h.opened.push(Note {
            change: p1.change.clone(),
            id: "agent:run-agent:0".into(),
            line: stacked::view::title_key(&p1),
            text: "```agent change\nThe title says two.\n```".into(),
            file: stacked::view::MSG_PATH.into(),
            preview: p1.title().into(),
            by: Some("agent".into()),
            ..Default::default()
        });
        d.add_run(r).map(|_| ())
    })
    .unwrap();
    app.refresh();
    keys(&mut app, &[KeyCode::Esc, KeyCode::Enter]);
    assert_eq!(app.screen, Screen::Review);
    keys(&mut app, &[KeyCode::Home]);
    down_to_review(&mut app, "modify  1 Bug 1");
    keys(&mut app, &[KeyCode::Right, KeyCode::Enter]);
    let mut buf = Buffer::empty(Rect::new(0, 0, 120, 80));
    app.draw(&mut buf);
    let screen: String = (0..80).map(|y| (0..120).map(|x| buf[(x, y)].symbol()).collect::<String>() + "\n").collect();
    assert!(screen.contains(" patch 1 as the work leaves it"), "{screen}");
    // The comment asked about, its reply; the one the work opened; and not
    // the other.
    for said in ["why so loud?", "Quieter now.", "The title says two."] {
        assert!(screen.contains(said), "{said:?}:\n{screen}");
    }
    assert!(!screen.contains("Not asked about."), "{screen}");
    // A comment written in the review is there too.
    keys(&mut app, &[KeyCode::Home]);
    down_to_work_row(&mut app, "+LINE 39");
    keys(&mut app, &[KeyCode::Enter]);
    typed(&mut app, "Written in the review.");
    keys(&mut app, &[KeyCode::Esc]);
    choose(&mut app, "Save it");
    let mut buf = Buffer::empty(Rect::new(0, 0, 120, 80));
    app.draw(&mut buf);
    let screen: String = (0..80).map(|y| (0..120).map(|x| buf[(x, y)].symbol()).collect::<String>() + "\n").collect();
    assert!(screen.contains("Written in the review.") && !screen.contains("Not asked about."), "{screen}");
    // A comment of its own on that line: the other one as it was, not the
    // review's.
    let notes = notes_of(&db, "s", 0);
    let other = notes.iter().find(|m| m.text.contains("Not asked about.")).unwrap();
    assert_eq!((other.text.as_str(), other.review.as_deref()), ("Not asked about.", None));
    let mine = notes.iter().find(|m| m.text == "Written in the review.").unwrap();
    assert_eq!((mine.line.as_str(), mine.review.as_deref()), (other.line.as_str(), Some("run-agent")));
}

#[test]
fn enter_on_a_line_of_the_interdiff_goes_to_that_line_of_the_patch() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "why so loud?");
    let n = notes_of(&db, "s", 0).remove(0);
    agent_changed_to(&db, "run-agent", &n, Some("Bug 1 - say two\n"));
    app.refresh();
    keys(&mut app, &[KeyCode::Esc, KeyCode::Enter]);
    assert_eq!(app.screen, Screen::Review);
    // → on the patch's line: the stack diff, the cursor on the first line
    // that changed in that patch, its subject.
    keys(&mut app, &[KeyCode::Home]);
    down_to_review(&mut app, "modify  1 Bug 1 - say two");
    keys(&mut app, &[KeyCode::Right]);
    assert_eq!(work_row(&mut app).as_deref(), Some("-Subject: Bug 1 - shout two"));
    assert_eq!(app.hint(), "↑↓: a line · enter: go to it in the patch · →: wider · ←: back to the list · esc: save, discard or close");
    // Enter: the patch as the work leaves it, the cursor on its title.
    keys(&mut app, &[KeyCode::Enter]);
    let screen = draw(&mut app);
    assert!(screen.contains(" patch 1 as the work leaves it: Bug 1 - say two"), "{screen}");
    assert_eq!(work_row(&mut app).as_deref(), Some("Bug 1 - say two"));
    assert_eq!(app.hint(), "↑↓: a line · enter: comment · →: wider · ←: back to the stack diff · esc: save, discard or close");
    // ← back to the stack diff, the cursor where it was; on the line of code
    // that took the place of another, Enter goes to it in the patch.
    keys(&mut app, &[KeyCode::Left]);
    assert_eq!(work_row(&mut app).as_deref(), Some("-Subject: Bug 1 - shout two"));
    down_to_work_row(&mut app, "++Two");
    keys(&mut app, &[KeyCode::Enter]);
    assert_eq!(work_row(&mut app).as_deref(), Some("+Two"));
    // There, Enter writes a comment on it.
    keys(&mut app, &[KeyCode::Enter]);
    assert!(matches!(&app.mode, Mode::Edit(_)), "{:?}", app.mode);
}

#[test]
fn what_you_write_in_the_review_of_the_agent_s_work_sends_it_back_to_the_agent() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "why so loud?");
    let n = notes_of(&db, "s", 0).remove(0);
    agent_changed(&db, "run-agent", &n);
    app.refresh();
    keys(&mut app, &[KeyCode::Esc, KeyCode::Enter]);
    assert_eq!(app.screen, Screen::Review);
    // → on a line of the stack operations: the stack diff, the cursor on the
    // line that changed; Enter, that patch as the work leaves it, a line at
    // a time, the cursor on the line in its place.
    keys(&mut app, &[KeyCode::Home]);
    down_to_review(&mut app, "modify  1 Bug 1");
    keys(&mut app, &[KeyCode::Right]);
    assert_eq!(work_row(&mut app).as_deref(), Some("-+TWO"));
    keys(&mut app, &[KeyCode::Enter]);
    let screen = draw(&mut app);
    assert!(screen.contains(" patch 1 as the work leaves it: Bug 1 - shout two"), "{screen}");
    assert_eq!(work_row(&mut app).as_deref(), Some("+Two"));
    assert_eq!(app.hint(), "↑↓: a line · enter: comment · →: wider · ←: back to the stack diff · esc: save, discard or close");
    // At its end, the comment on a line it no longer has, its reply in the
    // thread.
    keys(&mut app, &[KeyCode::End]);
    let screen = draw(&mut app);
    let (_, asked) = at_from(&screen, "why so loud?", 49);
    let (_, replied) = at_from(&screen, "Quieter now.", 49);
    assert!(replied > asked, "its reply in the thread, as the work leaves it:\n{screen}");
    keys(&mut app, &[KeyCode::Home]);
    // Enter on its new line: a comment there, saved in the review, not asked.
    down_to_work_row(&mut app, "+Two");
    keys(&mut app, &[KeyCode::Enter]);
    assert!(matches!(&app.mode, Mode::Edit(_)), "{:?}", app.mode);
    typed(&mut app, "Not like this either.");
    keys(&mut app, &[KeyCode::Esc]);
    let all = items(&app);
    assert!(all.contains(&("Save it".into(), true)) && !all.iter().any(|(l, _)| l.contains("ask agent")), "{all:?}");
    choose(&mut app, "Save it");
    assert_eq!(app.status, "saved in the review: Esc sends the work back to the agent with what you wrote");
    let mine = notes_of(&db, "s", 0).into_iter().find(|m| m.text == "Not like this either.").unwrap();
    assert_eq!(mine.review.as_deref(), Some("run-agent"));
    let screen = draw(&mut app);
    let (_, line) = at_from(&screen, "+Two", 49);
    let (_, comment) = at_from(&screen, "Not like this either.", 49);
    assert!(comment > line, "under its line:\n{screen}");
    assert_eq!(app.hint(), "↑↓: a line · enter: comment · →: wider · ←: back to the stack diff · esc: send back, save or discard");
    // On its reply, Space answers it.
    keys(&mut app, &[KeyCode::Left, KeyCode::Left]);
    down_to_review(&mut app, "Quieter now.");
    choose(&mut app, "Reply to it");
    save(&mut app, "Still too loud.");
    let d = Db::load(&db).unwrap();
    assert_eq!(d.run("run-agent").unwrap().held.as_ref().unwrap().replies[0].answer.as_deref(), Some("Still too loud."));
    drop(d);
    let screen = draw(&mut app);
    assert!(screen.contains(" what you wrote, to send back") && screen.contains("✎ your answer on a.txt: TWO · Still too loud."), "{screen}");
    let (_, reply) = at(&screen, "Quieter now.");
    let (_, answer) = at_from(&screen, "Still too loud.", 49);
    assert!(answer > reply, "after its reply, in its thread:\n{screen}");
    keys(&mut app, &[KeyCode::Char(' ')]);
    assert!(items(&app).contains(&("Edit your answer to it".into(), true)), "{:?}", items(&app));
    keys(&mut app, &[KeyCode::Esc]);
    // Not accepted, nor rejected: it goes back to the agent.
    keys(&mut app, &[KeyCode::Esc]);
    let labels: Vec<String> = items(&app).into_iter().map(|(l, _)| l).collect();
    assert_eq!(labels[0], "Send it back to the agent");
    assert!(!labels.iter().any(|l| l.contains("Close")), "{labels:?}");
    let Mode::Menu(m) = &app.mode else { panic!("{:?}", app.mode) };
    assert!(
        m.note.as_deref().unwrap().starts_with(
            "You wrote 1 comment and 1 answer to the agent's replies in this review, so the work is not accepted: it goes back to the agent, which is asked again, starting from this work.\n"
        ),
        "{:?}",
        m.note
    );
    choose(&mut app, "Send it back to the agent");
    assert_eq!(app.screen, Screen::Main);
    assert_eq!(app.status, "sent back to the agent (may change the code): 2 comments to ask again");
    let d = Db::load(&db).unwrap();
    assert!(d.review("run-agent").unwrap().closed);
    assert_eq!(d.run("run-agent").unwrap().summary, "reviewed: sent back to the agent, 2 comments to ask again");
    let thread: Vec<String> = stacked::thread::thread_turns(&d.note(&n.change, &n.id).unwrap().text).into_iter().map(|t| t.body).collect();
    assert_eq!(thread, ["why so loud?", "Quieter now.", "Still too loud."]);
    drop(d);
    // The next round goes on from that work, told so: its work lands in
    // place of the one sent back, the change carried.
    let mut next = None;
    for _ in 0..300 {
        let d = Db::load(&db).unwrap();
        let going = d.runs().iter().any(|r| r.state == stacked::db::RunState::Started);
        if let Some(r) = stacked::held::queue(&d, "s").first().filter(|_| !going) {
            next = Some(r.id.clone());
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let d = Db::load(&db).unwrap();
    let next = next.unwrap_or_else(|| panic!("never landed: {:?}", d.runs().iter().map(|r| (&r.id, &r.state, &r.summary)).collect::<Vec<_>>()));
    assert_ne!(next, "run-agent");
    let r = d.run(&next).unwrap();
    assert!(stacked::ask::info(r).unwrap().prompt.contains("This is another round"));
    let c = r.held.as_ref().unwrap().stack.as_ref().unwrap();
    assert!(d.patch(&c.patches[0]).diff.contains("+Two"), "the change of the work sent back");
}

#[test]
fn the_comments_that_asked_are_kept_or_closed_from_the_top_of_the_review() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "why so loud?");
    down_to(&mut app, "+LINE 39");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "and this?");
    let notes = notes_of(&db, "s", 0);
    answered(&db, "run-1", &notes);
    app.refresh();
    keys(&mut app, &[KeyCode::Esc, KeyCode::Enter]);
    assert_eq!(app.screen, Screen::Review);
    // On top, the comments that asked, each kept.
    let screen = draw(&mut app);
    assert!(screen.contains(" asking comments"), "{screen}");
    for n in &notes {
        assert!(screen.contains(&format!("   keep  a.txt: {} · {}", n.preview, n.text)), "{screen}");
    }
    // Enter on one: closed with the review; its reply, which goes with it,
    // accepted.
    keys(&mut app, &[KeyCode::Home]);
    assert!(selected_line(&mut app).unwrap().contains(&notes[0].text));
    assert_eq!(app.hint(), "↑↓ choose · enter: close it with the review · →: look closer · space: menu · esc: save, discard or close");
    assert!(draw(&mut app).contains("the comment that asked, as it is: kept open"));
    keys(&mut app, &[KeyCode::Enter]);
    let marks = |app: &App| app.reviewing.as_ref().unwrap().marks.clone();
    assert_eq!(marks(&app), [("asking 0", true), ("reply 0", true)].map(|(k, m)| (k.to_string(), m)).into());
    assert!(selected_line(&mut app).unwrap().starts_with("   close "));
    assert_eq!(app.hint(), "↑↓ choose · enter: keep it open · →: look closer · space: menu · esc: save, discard or close");
    // Kept again: the reply as it was.
    keys(&mut app, &[KeyCode::Enter]);
    assert!(marks(&app).is_empty(), "{:?}", marks(&app));
    // Space has it too.
    keys(&mut app, &[KeyCode::Char(' ')]);
    assert!(items(&app).contains(&("Keep it open".into(), false)), "{:?}", items(&app));
    choose(&mut app, "Close it with the review");
    assert_eq!(marks(&app).get("asking 0"), Some(&true));
    // Esc says what is closed with it, and closing closes it, once its reply
    // is in its thread; the other is kept.
    keys(&mut app, &[KeyCode::Esc]);
    assert!(!items(&app).iter().any(|(l, _)| l.contains("asking")), "{:?}", items(&app));
    assert!(matches!(&app.mode, Mode::Menu(m) if m.note.as_deref().is_some_and(|n| n.starts_with("asking comments: 1 to close, 1 kept"))), "{:?}", app.mode);
    choose(&mut app, "Accept all remaining and Close");
    choose(&mut app, "Accept them and close");
    assert_eq!(app.status, "reviewed: 1 opened comment accepted; 2 replies accepted; 1 asking comment closed");
    let d = Db::load(&db).unwrap();
    assert!(d.note(&notes[0].change, &notes[0].id).is_none(), "closed");
    let was = d.note_history(&notes[0].change, &notes[0].id).into_iter().rev().find(|(_, n)| !n.text.is_empty()).unwrap().1.text.clone();
    assert!(was.contains(&format!("Seen: {}", notes[0].text)), "its reply in first: {was}");
    assert!(d.note(&notes[1].change, &notes[1].id).is_some_and(|n| n.text.contains("Seen: ")));
}

#[test]
fn a_comment_where_one_was_closed_shows_it_dimmed_until_you_write() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "why so loud?");
    // Answered, then closed.
    Db::edit(&db, |d| {
        let n = d.notes(&d.patch(&d.stack("s")?.patches[0]).change.clone()).into_iter().next().cloned().unwrap();
        d.set_note(Note {
            text: append_turn(&n.text, Role::Agent, "It is a shout.", Some("answer")),
            ..n
        })
    })
    .unwrap();
    app.refresh();
    keys(&mut app, &[KeyCode::Down]);
    assert!(app.rows[app.cursor].head);
    choose(&mut app, "Close this comment");
    choose(&mut app, "Close it");
    assert!(notes_of(&db, "s", 0).is_empty());
    // On its line again: the editor shows it, dimmed, until you write.
    keys(&mut app, &[KeyCode::Home]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    assert!(matches!(&app.mode, Mode::Edit(e) if e.ed.ghosted()), "{:?}", app.mode);
    let screen = draw(&mut app);
    assert!(screen.contains("you: why so loud?") && screen.contains("agent: It is a shout."), "{screen}");
    assert!(screen.contains("↑↓: read the last one closed here"), "{screen}");
    assert_eq!(app.hint(), "↑↓: read the last comment closed here · enter: a new line · esc: close");
    let mut buf = Buffer::empty(Rect::new(0, 0, 120, 30));
    app.draw(&mut buf);
    let (x, y) = at(&screen, "you: why so loud?");
    assert!(buf[(x, y)].modifier.contains(ratatui::style::Modifier::DIM));
    // Written on, it is gone; saved, the comment is what you wrote.
    typed(&mut app, "louder?");
    let screen = draw(&mut app);
    assert!(!screen.contains("agent: It is a shout."), "{screen}");
    keys(&mut app, &[KeyCode::Esc]);
    choose(&mut app, "Save it");
    assert_eq!(notes_of(&db, "s", 0).into_iter().map(|n| n.text).collect::<Vec<_>>(), ["louder?"]);
    // With one open on the line, it is that one that is written on.
    keys(&mut app, &[KeyCode::Home]);
    down_to(&mut app, "-two");
    keys(&mut app, &[KeyCode::Enter]);
    assert!(matches!(&app.mode, Mode::Edit(e) if !e.ed.ghosted()), "nothing was closed on that line: {:?}", app.mode);
}

/// What the agent brought back of an ask, to answer, about `notes` of
/// patch 1: a reply to each, and a comment of its own on patch 1's title;
/// landed as run `run`.
fn answered(db: &Path, run: &str, notes: &[Note]) {
    Db::edit(db, |d| {
        let (from, s) = d.head("s").map(|(r, s)| (r, s.clone())).unwrap();
        let p1 = d.patch(&s.patches[0]).clone();
        let shown: Vec<serde_json::Value> = notes
            .iter()
            .enumerate()
            .map(|(k, n)| serde_json::json!({"label": format!("c{}", k + 1), "change": n.change, "id": n.id}))
            .collect();
        let replies = notes
            .iter()
            .map(|n| stacked::held::Reply {
                change: n.change.clone(),
                id: n.id.clone(),
                text: format!("Seen: {}", n.text),
                tag: "answer".into(),
                ..Default::default()
            })
            .collect();
        d.add_run(stacked::db::Run {
            id: run.into(),
            what: "answer".into(),
            stack: "s".into(),
            state: stacked::db::RunState::Done,
            request: notes[0].text.clone(),
            ask: Some(serde_json::json!({"way": "answer", "scope": {"patch": p1.change}, "from": from, "notes": shown})),
            held: Some(stacked::held::Held {
                at: stacked::db::now(),
                replies,
                opened: vec![Note {
                    change: p1.change.clone(),
                    id: format!("agent:{run}:0"),
                    line: stacked::view::title_key(&p1),
                    text: "```agent answer\nAnd the title?\n```".into(),
                    file: stacked::view::MSG_PATH.into(),
                    preview: p1.title().into(),
                    by: Some("agent".into()),
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
fn the_review_marks_its_opened_comments_and_its_replies_apart() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "why so loud?");
    down_to(&mut app, "+LINE 39");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "and this?");
    answered(&db, "run-1", &notes_of(&db, "s", 0));
    app.refresh();
    keys(&mut app, &[KeyCode::Esc, KeyCode::Enter]);
    assert_eq!(app.screen, Screen::Review);
    keys(&mut app, &[KeyCode::Char(' ')]);
    let all: Vec<String> = items(&app).into_iter().map(|(l, _)| l).collect();
    for l in ["Mark all opened comments accepted", "Mark all opened comments rejected", "Mark all replies accepted", "Mark all replies rejected"] {
        assert!(all.contains(&l.to_string()), "{l:?} in {all:?}");
    }
    assert!(!all.iter().any(|l| l.starts_with("Mark all comments")), "{all:?}");
    let marks = |app: &App| app.reviewing.as_ref().unwrap().marks.clone();
    choose(&mut app, "Mark all replies rejected");
    assert_eq!(marks(&app), [("reply 0", false), ("reply 1", false)].map(|(k, m)| (k.to_string(), m)).into());
    keys(&mut app, &[KeyCode::Char(' ')]);
    choose(&mut app, "Mark all opened comments accepted");
    assert_eq!(marks(&app), [("opened 0", true), ("reply 0", false), ("reply 1", false)].map(|(k, m)| (k.to_string(), m)).into());
}

#[test]
fn the_banner_has_read_only_then_edits_then_comments_each_the_one_to_review_first() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "why so loud?");
    let n = notes_of(&db, "s", 0).remove(0);
    // An edit, an answer, and an edit going on from the edit.
    held(&db, "run-1", &n.change, &n.id, "Quieter now.", true);
    held(&db, "run-2", &n.change, &n.id, "It shouts.", false);
    held(&db, "run-3", &n.change, &n.id, "Quieter still.", true);
    Db::edit(&db, |d| {
        let mut r = d.run("run-3").unwrap().clone();
        let mut ask = r.ask.clone().unwrap();
        ask["after"] = serde_json::json!("run-1");
        r.ask = Some(ask);
        d.add_run(r).map(|_| ())
    })
    .unwrap();
    app.refresh();
    let screen = draw(&mut app);
    assert!(screen.contains("3 landed asks wait for your review, 1 of them after an edit under review · the next: 1 reply"), "{screen}");
    keys(&mut app, &[KeyCode::Esc]);
    let screen = draw(&mut app);
    let rows: Vec<&str> = screen.lines().skip(2).take(8).map(str::trim_end).collect();
    let starts = [
        " read only, to review",
        "   ⟳ Review 1 reply",
        " edits, to review",
        "   ⟳ Review a change of patch 1, 1 reply",
        "   ◌ a change of patch 1, 1 reply",
        " open comments",
    ];
    for (row, want) in rows.iter().zip(starts) {
        assert!(row.starts_with(want), "{want:?}:\n{screen}");
    }
    assert!(rows[4].ends_with("after the edit under review, which you review first"), "{screen}");
    // The cursor goes from line to line, past the titles; on a work going on
    // from the edit, Enter says to review that one first.
    assert!(app.on_banner && app.banner_sel == 0);
    keys(&mut app, &[KeyCode::Down]);
    assert!(app.banner_sel == 1 && selected_banner(&mut app).starts_with("   ⟳ Review a change of patch 1"), "{}", draw(&mut app));
    keys(&mut app, &[KeyCode::Down, KeyCode::Enter]);
    assert_eq!(app.screen, Screen::Main);
    assert!(app.status.ends_with("review that one first"), "{}", app.status);
    keys(&mut app, &[KeyCode::Up, KeyCode::Enter]);
    assert_eq!(app.screen, Screen::Review);
}

/// The banner's line the cursor is on, as drawn.
fn selected_banner(app: &mut App) -> String {
    let mut buf = Buffer::empty(Rect::new(0, 0, 120, 30));
    app.draw(&mut buf);
    (2..30)
        .find(|&y| buf[(3, y)].modifier.contains(ratatui::style::Modifier::REVERSED))
        .map(|y| (0..120).map(|x| buf[(x, y)].symbol()).collect::<String>())
        .unwrap_or_default()
}

/// Patch 1 commented on, and the work runs `kinds` name landed for it: an
/// edit (`edit`), an answer (`answer`), an edit going on from `run-1`
/// (`guess`); and a build and test going, with `build`.
fn landed_kinds(kinds: &[(&str, &str)], build: bool) -> (tempfile::TempDir, PathBuf, App) {
    let (tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "why so loud?");
    let n = notes_of(&db, "s", 0).remove(0);
    for (id, kind) in kinds {
        held(&db, id, &n.change, &n.id, &format!("{kind} {id}."), *kind != "answer");
    }
    Db::edit(&db, |d| {
        for (id, _) in kinds.iter().filter(|(_, k)| *k == "guess") {
            let mut r = d.run(id).unwrap().clone();
            let mut ask = r.ask.clone().unwrap();
            ask["after"] = serde_json::json!("run-1");
            r.ask = Some(ask);
            d.add_run(r)?;
        }
        if build {
            d.add_run(stacked::db::Run {
                id: "run-build".into(),
                what: "test".into(),
                stack: "s".into(),
                state: stacked::db::RunState::Started,
                ask: Some(serde_json::json!({"way": "test", "scope": {"patch": n.change}, "from": 1, "notes": []})),
                ..Default::default()
            })?;
        }
        Ok(())
    })
    .unwrap();
    app.refresh();
    (tmp, db, app)
}

#[test]
fn accepting_an_edit_says_first_the_answers_it_asks_again_and_offers_to_stop_a_build() {
    let (_tmp, db, mut app) = landed_kinds(&[("run-1", "edit"), ("run-2", "answer")], true);
    // The edit's review, below the answer's in the banner.
    keys(&mut app, &[KeyCode::Esc, KeyCode::Down, KeyCode::Enter]);
    assert_eq!(app.reviewing.as_ref().map(|rv| rv.run.as_str()), Some("run-1"));
    keys(&mut app, &[KeyCode::Esc]);
    choose(&mut app, "Accept all remaining and Close");
    let Mode::Menu(m) = &app.mode else { panic!("{:?}", app.mode) };
    let note = m.note.clone().unwrap();
    assert!(
        note.contains("Closing this review applies its change. The 1 answer-only ask made on the stack before it is asked again: 1 waiting for your review is thrown away."),
        "{note}"
    );
    assert!(note.contains("A build and test of the stack as it was is going: it is not asked again, and you may stop it."), "{note}");
    let labels: Vec<String> = items(&app).into_iter().map(|(l, _)| l).collect();
    assert_eq!(labels, ["Accept them and close", "Accept them and close, and stop the build", "Keep reviewing"]);
    assert_eq!(m.sel, 2, "on Keep reviewing");
    choose(&mut app, "Accept them and close");
    assert!(app.status.ends_with("; 1 answer-only ask asked again"), "{}", app.status);
    let d = Db::load(&db).unwrap();
    assert!(d.review("run-2").is_some_and(|v| v.closed));
    assert_eq!(d.run("run-build").unwrap().state, stacked::db::RunState::Started, "the build goes on");
}

#[test]
fn rejecting_or_sending_back_says_first_the_edits_it_asks_again() {
    let (_tmp, _db, mut app) = landed_kinds(&[("run-1", "edit"), ("run-3", "guess")], true);
    keys(&mut app, &[KeyCode::Esc, KeyCode::Enter]);
    assert_eq!(app.reviewing.as_ref().map(|rv| rv.run.as_str()), Some("run-1"));
    // Rejected: what went on from it is asked again; the build, of the stack
    // as it is, is no matter.
    keys(&mut app, &[KeyCode::Esc]);
    choose(&mut app, "Reject all remaining and Close");
    let Mode::Menu(m) = &app.mode else { panic!("{:?}", app.mode) };
    let note = m.note.clone().unwrap();
    assert!(
        note.contains("Rejecting this change throws away the 1 edit that went on from it, as if you accepted it: 1 waiting for your review is thrown away. It is asked again from the stack as it is."),
        "{note}"
    );
    let labels: Vec<String> = items(&app).into_iter().map(|(l, _)| l).collect();
    assert_eq!(labels, ["Reject them and close", "Keep reviewing"]);
    choose(&mut app, "Keep reviewing");
    // Sent back, the same.
    down_to_review(&mut app, "edit run-1.");
    choose(&mut app, "Reply to it");
    save(&mut app, "Not quiet enough.");
    keys(&mut app, &[KeyCode::Esc]);
    choose(&mut app, "Send it back to the agent");
    let Mode::Menu(m) = &app.mode else { panic!("{:?}", app.mode) };
    let note = m.note.clone().unwrap_or_default();
    assert!(
        note.contains("Sending this work back throws away the 1 edit that went on from it: 1 waiting for your review is thrown away. It is asked again once the agent's new work lands."),
        "{note}"
    );
    choose(&mut app, "Send it back");
    assert!(app.status.ends_with("; 1 edit that went on from it asked again"), "{}", app.status);
}

#[test]
fn right_in_a_review_goes_into_its_right_pane_then_makes_it_wide() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "why so loud?");
    let n = notes_of(&db, "s", 0).into_iter().next().unwrap();
    held(&db, "run-1", &n.change, &n.id, "It is a shout.", true);
    app.refresh();
    keys(&mut app, &[KeyCode::Esc, KeyCode::Enter]);
    assert_eq!(app.screen, Screen::Review);
    // Into the pane on the right: the list beside it still.
    keys(&mut app, &[KeyCode::Right]);
    assert_eq!(app.hint(), "↑↓: a line · enter: go to it in the patch · →: wider · ←: back to the list · esc: save, discard or close");
    assert!(draw(&mut app).contains(" stack operations"));
    // Wide: the list out of sight.
    keys(&mut app, &[KeyCode::Right]);
    let screen = draw(&mut app);
    assert!(!screen.contains(" stack operations"), "{screen}");
    assert!(screen.lines().nth(1).unwrap().starts_with(" the change of patch 1: what accepting it does to the stack"), "{screen}");
    assert_eq!(app.hint(), "↑↓: a line · enter: go to it in the patch · ←: narrower · esc: save, discard or close");
    // ← narrower, then back to the list.
    keys(&mut app, &[KeyCode::Left]);
    assert!(draw(&mut app).contains(" stack operations"));
    assert!(app.reviewing.as_ref().unwrap().in_main);
    keys(&mut app, &[KeyCode::Left]);
    assert!(!app.reviewing.as_ref().unwrap().in_main);
    assert_eq!(app.hint(), "↑↓ choose · enter: accept · →: look closer · space: menu · esc: save, discard or close");
}

#[test]
fn what_a_pane_has_picked_shows_while_the_cursor_is_elsewhere() {
    use ratatui::style::{Color, Modifier};
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    let cell = |app: &mut App, text: &str| {
        let mut buf = Buffer::empty(Rect::new(0, 0, 120, 30));
        app.draw(&mut buf);
        let (x, y) = at(&draw(app), text);
        buf[(x, y)].clone()
    };
    // The cursor's own pane reverses its pick.
    let c = cell(&mut app, " 1 Bug 1 - shout two");
    assert!(c.modifier.contains(Modifier::REVERSED), "{c:?}");
    // Elsewhere, a pane's pick is on a grey background, as `picked` paints.
    keys(&mut app, &[KeyCode::Right]);
    let c = cell(&mut app, " 1 Bug 1 - shout two");
    assert!(!c.modifier.contains(Modifier::REVERSED) && c.bg == Color::Indexed(238), "{c:?}");
    // So is the review's part, while the cursor is in the pane on its
    // right.
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "why so loud?");
    let n = notes_of(&db, "s", 0).into_iter().next().unwrap();
    held(&db, "run-1", &n.change, &n.id, "It is a shout.", false);
    app.refresh();
    keys(&mut app, &[KeyCode::Esc, KeyCode::Enter, KeyCode::Right]);
    assert_eq!(app.screen, Screen::Review);
    let c = cell(&mut app, "· a.txt: TWO · It is a shout.");
    assert!(!c.modifier.contains(Modifier::REVERSED) && c.bg == Color::Indexed(238), "{c:?}");
}

#[test]
fn the_banner_tracks_the_open_comments_and_esc_goes_there_and_back() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    // No comment, nothing landed: no banner, and Esc goes back up a pane.
    keys(&mut app, &[KeyCode::Right]);
    assert!(!draw(&mut app).contains("open comment"));
    keys(&mut app, &[KeyCode::Esc]);
    assert!(app.focus == Focus::List && !app.on_banner);
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "why so loud?");
    down_to(&mut app, "+LINE 39");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "and this?");
    let notes = notes_of(&db, "s", 0);
    let said = |text: &str| notes.iter().find(|n| n.text == text).cloned().unwrap();
    let (loud, this) = (said("why so loud?"), said("and this?"));
    // Open, the comments are said in the banner.
    assert!(draw(&mut app).contains("\n ○ 2 open comments"));
    // The agent works on the first; the second, a change too, waits its
    // turn.
    changing(&db, &loud.change, &loud.id, serde_json::json!({"patch": loud.change}));
    Db::edit(&db, |d| {
        let n = d.note(&this.change, &this.id).cloned().unwrap();
        d.set_note(Note {
            queued: Some(stacked::db::Queued {
                way: stacked::db::Way::Change,
                batch: "b-waiting".into(),
                stack: false,
                stage: stacked::db::Stage::Ready,
                on: None,
                was: None,
            }),
            ..n
        })
    })
    .unwrap();
    app.refresh();
    let screen = draw(&mut app);
    assert!(screen.contains("\n ● the agent works on 1 ask · 1 comment waits its turn · 2 open comments"), "one line:\n{screen}");
    // Esc goes to it, open: the comments, the most recently updated first,
    // each as it stands.
    down_to(&mut app, "+LINE 39");
    let cursor = app.cursor;
    keys(&mut app, &[KeyCode::Esc]);
    assert!(app.on_banner && app.banner_sel == 0);
    let screen = draw(&mut app);
    let lines: Vec<&str> = screen.lines().collect();
    assert!(lines[2].starts_with(" open comments"), "{screen}");
    assert!(
        lines[3].starts_with("   ◌ patch 1 · a.txt: LINE 39 · \"and this?\" · ready (may change the code), once the agent's change on patch 1 lands"),
        "{screen}"
    );
    assert!(lines[4].starts_with("   ● patch 1 · a.txt: TWO · \"why so loud?\" · with the agent (may change the code)"), "{screen}");
    assert!(app.hint().contains("enter: go to it"), "{}", app.hint());
    // Updated, a comment goes first.
    Db::edit(&db, |d| {
        let n = d.note(&loud.change, &loud.id).cloned().unwrap();
        d.set_note(Note { text: format!("{}\nreally", n.text), ..n })
    })
    .unwrap();
    app.refresh();
    assert!(draw(&mut app).lines().nth(3).unwrap().contains("\"why so loud?\""));
    // The mouse going by does not close it, nor does it open it.
    app.hover(10, 8);
    assert!(app.on_banner, "the mouse over the panes leaves it open");
    keys(&mut app, &[KeyCode::Esc]);
    assert!(!app.on_banner && app.focus == Focus::Diff && app.cursor == cursor, "back where it was");
    app.hover(10, 1);
    assert!(!app.on_banner, "the mouse over it does not open it");
    // Enter on a line goes to its comment.
    keys(&mut app, &[KeyCode::Esc, KeyCode::Enter]);
    assert!(!app.on_banner && app.focus == Focus::Diff);
    draw(&mut app);
    assert_eq!(app.rows[app.cursor].note.as_deref(), Some(loud.id.as_str()));
    keys(&mut app, &[KeyCode::Esc, KeyCode::Down, KeyCode::Enter]);
    draw(&mut app);
    assert_eq!(app.rows[app.cursor].note.as_deref(), Some(this.id.as_str()));
    // Space picks one, among the comments too.
    keys(&mut app, &[KeyCode::Esc, KeyCode::Char(' ')]);
    let all: Vec<String> = items(&app).into_iter().map(|(l, _)| l).collect();
    assert_eq!(all[..2], ["Go to patch 1 · a.txt: TWO · \"why so loud?\"", "Go to patch 1 · a.txt: LINE 39 · \"and this?\""], "{all:?}");
    choose(&mut app, "Go to patch 1 · a.txt: TWO");
    draw(&mut app);
    assert!(!app.on_banner);
    assert_eq!(app.rows[app.cursor].note.as_deref(), Some(loud.id.as_str()));
    // ↓ past the last line, the stack's header; ↑ from there, the last line.
    keys(&mut app, &[KeyCode::Esc, KeyCode::End, KeyCode::Down]);
    assert!(!app.on_banner && app.focus == Focus::List && app.on_stack);
    keys(&mut app, &[KeyCode::Up]);
    assert!(app.on_banner && app.banner_sel == 1);
    // A click opens it too.
    keys(&mut app, &[KeyCode::Esc]);
    draw(&mut app);
    app.click(10, 1, false);
    assert!(app.on_banner);
}

#[test]
fn the_comment_editor_hides_nothing_what_it_would_cover_comes_after_it() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    // Wide enough for two sides in the half of it the middle takes while a
    // comment is written there.
    let wide = |app: &mut App| {
        let mut buf = Buffer::empty(Rect::new(0, 0, 160, 30));
        app.draw(&mut buf);
        (0..30).map(|y| (0..160).map(|x| buf[(x, y)].symbol()).collect::<String>()).collect::<Vec<_>>()
    };
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    // In each view of the code: the line, the box under it, and after the
    // box the line it would have covered, ` three`.
    for view in ["unified", "side by side", "compact"] {
        keys(&mut app, &[KeyCode::Enter]);
        typed(&mut app, "why so loud?");
        let lines = wide(&mut app);
        let screen = lines.join("\n");
        let line = lines.iter().position(|l| l.contains("TWO") && !l.contains("a comment on")).unwrap();
        let foot = lines.iter().position(|l| l.contains("enter: a new line · esc:")).unwrap_or_else(|| panic!("{view}: no box:\n{screen}"));
        assert!(lines[line + 1].contains("a comment on: TWO"), "{view}: the box under the line:\n{screen}");
        assert!(lines[foot + 1].contains("three"), "{view}: what the box would hide, after it:\n{screen}");
        assert!(!lines[line + 1..foot].iter().any(|l| l.contains("three")), "{view}:\n{screen}");
        if view == "side by side" {
            assert!(lines[foot + 1].matches("three").count() == 2, "both sides:\n{screen}");
        }
        keys(&mut app, &[KeyCode::Esc]);
        choose(&mut app, "Throw it away");
        keys(&mut app, &[KeyCode::Char('1')]);
    }
}

#[test]
fn each_pane_keeps_where_its_cursor_was_to_come_back_to() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    // The middle of patch 1, on a line far down.
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+LINE 39");
    let (cursor, top) = (app.cursor, app.top);
    // Patch 2 is read from its top; back on patch 1, the middle is where it
    // was, and as it was scrolled.
    keys(&mut app, &[KeyCode::Left, KeyCode::Down]);
    draw(&mut app);
    assert_eq!((app.sel, app.cursor), (1, 0));
    keys(&mut app, &[KeyCode::Up]);
    draw(&mut app);
    assert_eq!((app.cursor, app.top), (cursor, top), "patch 1's middle where it was");
    // The same from the stack's header, and from another stack.
    keys(&mut app, &[KeyCode::Up]);
    draw(&mut app);
    assert!(app.on_stack);
    choose(&mut app, "Switch stack…");
    down_to_pick(&mut app, "t");
    keys(&mut app, &[KeyCode::Enter]);
    assert_eq!(app.stack, "t");
    keys(&mut app, &[KeyCode::Home]);
    choose(&mut app, "Switch stack…");
    down_to_pick(&mut app, "s");
    keys(&mut app, &[KeyCode::Enter]);
    assert!(app.stack == "s" && app.on_stack, "the patches pane where it was: on the header");
    keys(&mut app, &[KeyCode::Down]);
    draw(&mut app);
    assert!(app.rows[app.cursor].text.starts_with("+LINE 39"), "{:?}", app.rows[app.cursor].text);
    // The history: where it was, on the same record when another is added.
    keys(&mut app, &[KeyCode::Down]);
    choose(&mut app, "Drop this patch");
    choose(&mut app, "Drop it");
    keys(&mut app, &[KeyCode::Home]);
    choose(&mut app, "History…");
    keys(&mut app, &[KeyCode::Down]);
    let rev = app.history()[1].rev;
    draw(&mut app);
    keys(&mut app, &[KeyCode::Esc]);
    choose(&mut app, "History…");
    assert_eq!((app.pick, app.history()[app.pick].rev), (1, rev));
    keys(&mut app, &[KeyCode::Esc, KeyCode::Down]);
    choose(&mut app, "Drop this patch");
    choose(&mut app, "Drop it");
    keys(&mut app, &[KeyCode::Home]);
    choose(&mut app, "History…");
    assert_eq!((app.pick, app.history()[app.pick].rev), (2, rev), "the same record, a newer one above it");
}

/// Down the list of stacks, from its top, to stack `name`.
fn down_to_pick(app: &mut App, name: &str) {
    key(app, KeyCode::Home);
    for _ in 0..20 {
        if app.db.stacks().get(app.pick).is_some_and(|s| s.name == name) {
            return;
        }
        key(app, KeyCode::Down);
    }
    panic!("no stack {name:?}");
}

/// What a line of the screen shows right of the patches pane.
fn right_of_list(line: &str) -> &str {
    line.split_once('│').map_or(line, |(_, rest)| rest)
}

#[test]
fn the_patch_s_title_stays_at_the_top_of_the_middle() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    // A screen of 12 lines: the patch does not fit in it.
    let small = |app: &mut App| {
        let mut buf = Buffer::empty(Rect::new(0, 0, 120, 12));
        app.draw(&mut buf);
        (0..12).map(|y| (0..120).map(|x| buf[(x, y)].symbol()).collect::<String>()).collect::<Vec<_>>().join("\n")
    };
    keys(&mut app, &[KeyCode::Right]);
    for view in ["unified", "side by side", "compact"] {
        // At the top, the title is its own row, under ✉ message.
        keys(&mut app, &[KeyCode::Home]);
        let screen = small(&mut app);
        assert!(right_of_list(screen.lines().nth(1).unwrap()).starts_with("✉ message"), "{view}:\n{screen}");
        // Far down, its row scrolled past: on the first line, the rows
        // under it, the cursor's in sight.
        for _ in 0..app.rows.len() {
            if app.rows[app.cursor].text.starts_with("+LINE 39") {
                break;
            }
            key(&mut app, KeyCode::Down);
            small(&mut app);
        }
        let screen = small(&mut app);
        let lines: Vec<&str> = screen.lines().collect();
        assert!(right_of_list(lines[1]).starts_with("   Bug 1 - shout two"), "{view}: pinned:\n{screen}");
        assert_eq!(lines.iter().filter(|l| right_of_list(l).contains("Bug 1 - shout two")).count(), 1, "{view}: once:\n{screen}");
        assert!(lines[2..lines.len() - 1].iter().any(|l| l.contains("LINE 39")), "{view}: the cursor's row:\n{screen}");
        keys(&mut app, &[KeyCode::Char('1')]);
    }
    // A click on it goes to its row.
    keys(&mut app, &[KeyCode::End]);
    small(&mut app);
    app.click(40, 1, false);
    small(&mut app);
    assert!(app.rows[app.cursor].text.starts_with("   Bug 1 - shout two"), "{:?}", app.rows[app.cursor].text);
}

#[test]
fn expanded_the_patches_say_their_titles_whole() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir(&src).unwrap();
    git(&src, &["init", "-q", "-b", "main"]);
    git(&src, &["config", "commit.gpgsign", "false"]);
    std::fs::write(src.join("a.txt"), "a\n").unwrap();
    git(&src, &["add", "."]);
    git(&src, &["commit", "-qm", "base"]);
    std::fs::write(src.join("a.txt"), "b\n").unwrap();
    let title = "Bug 3 - a title long enough to need more than one line of the patches pane, which folds it";
    git(&src, &["commit", "-qam", title]);
    let db = tmp.path().join("db.jsonl");
    stacked(&src, &db, &["import", "s", "HEAD~1"]);
    let mut app = open(&db).unwrap();
    // The patches pane's lines, under its header.
    let pane = |app: &mut App| -> Vec<String> {
        let screen = draw(app);
        screen.lines().skip(2).map(|l| l.split('│').next().unwrap().trim_end().to_string()).take_while(|l| !l.is_empty()).collect()
    };
    // Expanded: the title whole, folded under its number.
    let lines = pane(&mut app);
    assert!(lines.len() > 1 && lines[1].starts_with("    "), "{lines:?}");
    assert_eq!(lines.join(" ").split_whitespace().skip(1).collect::<Vec<_>>().join(" "), title, "{lines:?}");
    // Compact: its number, and the lines it adds and removes.
    keys(&mut app, &[KeyCode::Left]);
    assert_eq!(pane(&mut app), ["  1 +1 −1"]);
}

/// The stand-in for Phabricator, in the test of a pull: D11 holds ann's
/// comment; D12 none, and waits to be let go, then fails if it is told to.
static LET_GO: std::sync::Mutex<bool> = std::sync::Mutex::new(false);
static GONE_ON: std::sync::Condvar = std::sync::Condvar::new();
static FAIL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn phabricator(source: &str, rev: &str, _url: &str) -> Result<stacked::phab::RevisionComments, String> {
    if rev == "D12" {
        let mut go = LET_GO.lock().unwrap();
        while !*go {
            go = GONE_ON.wait(go).unwrap();
        }
        if FAIL.load(std::sync::atomic::Ordering::SeqCst) {
            return Err("transaction.search: D12 is not there".into());
        }
    }
    let ann = stacked::phab::Remote {
        phid: "c1".into(),
        author: "ann".into(),
        text: "Why shout?".into(),
        created: 1,
        path: None,
        line: 0,
        line_text: None,
        reply_to: None,
        earlier: vec![],
    };
    Ok(stacked::phab::RevisionComments {
        revision: rev.into(),
        source: source.into(),
        comments: if rev == "D11" { vec![ann] } else { vec![] },
    })
}

#[test]
fn a_pull_from_phabricator_says_in_the_banner_where_it_is_until_it_is_done() {
    // A stack of two patches, each on its revision.
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir(&src).unwrap();
    git(&src, &["init", "-q", "-b", "main"]);
    git(&src, &["config", "commit.gpgsign", "false"]);
    std::fs::write(src.join("a.txt"), "one\ntwo\n").unwrap();
    git(&src, &["add", "."]);
    git(&src, &["commit", "-qm", "base"]);
    std::fs::write(src.join("a.txt"), "one\nTWO\n").unwrap();
    git(&src, &["commit", "-qam", "Bug 1 - shout two\n\nDifferential Revision: https://phabricator.example.com/D11"]);
    std::fs::write(src.join("b.txt"), "bee\n").unwrap();
    git(&src, &["add", "."]);
    git(&src, &["commit", "-qm", "Bug 2 - add b\n\nDifferential Revision: https://phabricator.example.com/D12"]);
    let db = tmp.path().join("db.jsonl");
    stacked(&src, &db, &["import", "s", "HEAD~2"]);
    stacked::phab::stand_in(Some(phabricator));
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Left, KeyCode::Home]);
    choose(&mut app, "Pull reviews from Phabricator");
    // Under way at once, before a revision is read.
    let banner = |app: &mut App| draw(app).lines().nth(1).unwrap_or("").to_string();
    assert!(banner(&mut app).starts_with(" ⇣ pulling the reviews from Phabricator…"), "{}", banner(&mut app));
    assert_eq!(app.banner_lines(), [stacked::tui::BannerLine::Pulling]);
    // D11 read, D12 being read: the banner says which.
    let reading = " ⇣ pulling the reviews from Phabricator: D12, revision 2 of 2…";
    for _ in 0..500 {
        app.refresh();
        if banner(&mut app).starts_with(reading) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(banner(&mut app).starts_with(reading), "{}", banner(&mut app));
    // Open, its line is the read only section's.
    keys(&mut app, &[KeyCode::Esc]);
    let screen = draw(&mut app);
    let rows: Vec<&str> = screen.lines().map(str::trim_end).collect();
    assert_eq!(rows[2..4], [" read only, to review", "   ⇣ Pulling the reviews from Phabricator: D12, revision 2 of 2…"], "{screen}");
    assert_eq!(app.hint(), "↑↓ choose · enter: say what the pull brings · space: pick one · esc: back where you were · ?: what can I do here");
    keys(&mut app, &[KeyCode::Enter]);
    assert_eq!(app.status, "what is new from Phabricator will wait here for your review once the pull is done");
    // Done: its line gone, what came of it said, and what it brought waits
    // for review.
    *LET_GO.lock().unwrap() = true;
    GONE_ON.notify_all();
    app.settle();
    assert_eq!(app.status, "1 new comment from 2 revisions: it waits for your review");
    assert!(!app.banner_lines().contains(&stacked::tui::BannerLine::Pulling));
    let screen = draw(&mut app);
    assert!(!screen.contains("pulling the reviews") && screen.contains("⟳ 1 pull from Phabricator waits for your review"), "{screen}");
    // A pull that fails: its line goes, and why is said.
    FAIL.store(true, std::sync::atomic::Ordering::SeqCst);
    keys(&mut app, &[KeyCode::Esc, KeyCode::Home]);
    choose(&mut app, "Pull reviews from Phabricator");
    assert!(app.banner_lines().contains(&stacked::tui::BannerLine::Pulling));
    app.settle();
    assert_eq!(app.status, "transaction.search: D12 is not there");
    assert!(!app.banner_lines().contains(&stacked::tui::BannerLine::Pulling));
    assert!(!draw(&mut app).contains("pulling the reviews"));
    stacked::phab::stand_in(None);
}

#[test]
fn reviews_pulled_wait_in_the_banner_as_the_agent_s_work_does() {
    use stacked::phab::{Pulled, Remote, RevisionComments, Version};
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    let d = Db::load(&db).unwrap();
    let change = d.patch(&d.stack("s").unwrap().patches[0]).change.clone();
    drop(d);
    let pulled = |comments: Vec<Remote>| -> Result<Pulled, String> {
        Ok(Pulled { revisions: vec![RevisionComments { revision: "D1".into(), source: change.clone(), comments }] })
    };
    let by_ann = |phid: &str, text: &str, reply: Option<&str>| Remote {
        phid: phid.into(),
        author: "ann".into(),
        text: text.into(),
        created: 0,
        path: None,
        line: 0,
        line_text: None,
        reply_to: reply.map(String::from),
        earlier: vec![],
    };
    // A pull: ann's comment waits for review, nothing filed yet.
    Db::edit(&db, |d| stacked::review::file(d, "s", "pull-1", 1, &pulled(vec![by_ann("c1", "Why shout?", None)]))).unwrap();
    app.refresh();
    let screen = draw(&mut app);
    assert!(
        screen.contains("⟳ 1 pull from Phabricator waits for your review · the next: 1 opened comment, pulled from Phabricator"),
        "{screen}"
    );
    assert!(notes_of(&db, "s", 0).is_empty(), "nothing filed yet");
    // On the banner, its line; Enter reviews it.
    keys(&mut app, &[KeyCode::Esc]);
    assert!(draw(&mut app).contains("   ⟳ Review 1 opened comment, pulled from Phabricator"));
    keys(&mut app, &[KeyCode::Enter]);
    assert_eq!(app.screen, Screen::Review);
    let screen = draw(&mut app);
    assert!(screen.contains(" pulled from Phabricator: 1 opened comment") && screen.contains("a reviewer's comment, where it would be"), "{screen}");
    keys(&mut app, &[KeyCode::Esc]);
    assert!(draw(&mut app).contains("your review of the comments pulled"));
    choose(&mut app, "Accept all remaining and Close");
    choose(&mut app, "Accept them and close");
    let notes = notes_of(&db, "s", 0);
    assert_eq!((notes.len(), notes[0].by.as_deref()), (1, Some("ann")));
    // Pulled later, ann's reply to it waits, said on the comment; taken at
    // once from its menu, it is ann's turn of the thread.
    Db::edit(&db, |d| stacked::review::file(d, "s", "pull-2", 1, &pulled(vec![by_ann("c1", "Why shout?", None), by_ann("c2", "Still?", Some("c1"))]))).unwrap();
    app.refresh();
    keys(&mut app, &[KeyCode::Right, KeyCode::Home]);
    assert!(draw(&mut app).contains("ann's reply waits for your review"));
    down_to(&mut app, "   └ ann's comment");
    choose(&mut app, "Accept ann's reply");
    let text = notes_of(&db, "s", 0).remove(0).text;
    assert!(text.ends_with("```reviewer ann\nStill?\n```"), "{text}");
    // ann edits her comment on Phabricator: pulled, her edit waits, said on
    // the comment; taken at once, it is in the place of her turn.
    let edited = |phid: &str, text: &str, was: &[(&str, &str)]| Remote {
        earlier: was.iter().map(|(phid, text)| Version { phid: phid.to_string(), text: text.to_string() }).collect(),
        ..by_ann(phid, text, None)
    };
    let still = by_ann("c2", "Still?", Some("c1"));
    Db::edit(&db, |d| stacked::review::file(d, "s", "pull-3", 1, &pulled(vec![edited("c1v2", "Why shout so?", &[("c1", "Why shout?")]), still.clone()]))).unwrap();
    app.refresh();
    assert!(draw(&mut app).contains("ann's edit waits for your review"));
    keys(&mut app, &[KeyCode::Home]);
    down_to(&mut app, "   └ ann's comment");
    assert!(app.hint().starts_with("enter: accept ann's edit · "), "{}", app.hint());
    choose(&mut app, "Accept ann's edit");
    assert_eq!(app.status, "you accepted ann's edit: it is in the thread now");
    let text = notes_of(&db, "s", 0).remove(0).text;
    assert_eq!(text, "```reviewer ann\nWhy shout so?\n```\n```reviewer ann\nStill?\n```");
    // Edited again: in its review, the edit says whose it is, and shows the
    // thread as it would be.
    let again = edited("c1v3", "Why shout at all?", &[("c1v2", "Why shout so?"), ("c1", "Why shout?")]);
    Db::edit(&db, |d| stacked::review::file(d, "s", "pull-4", 1, &pulled(vec![again, still]))).unwrap();
    app.refresh();
    keys(&mut app, &[KeyCode::Esc]);
    assert!(draw(&mut app).contains("   ⟳ Review 1 edited comment, pulled from Phabricator"));
    keys(&mut app, &[KeyCode::Enter]);
    assert_eq!(app.screen, Screen::Review);
    let screen = draw(&mut app);
    assert!(screen.contains(" pulled from Phabricator: 1 edited comment") && screen.contains(" · ann's edit: "), "{screen}");
    assert!(screen.contains("ann's comment, edited on Phabricator, as it would be in its thread"), "{screen}");
    keys(&mut app, &[KeyCode::Esc]);
    choose(&mut app, "Accept all remaining and Close");
    choose(&mut app, "Accept them and close");
    let text = notes_of(&db, "s", 0).remove(0).text;
    assert_eq!(text, "```reviewer ann\nWhy shout at all?\n```\n```reviewer ann\nStill?\n```");
}
