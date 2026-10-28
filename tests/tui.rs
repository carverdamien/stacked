use std::path::{Path, PathBuf};
use std::process::Command;

use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;

use stacked::db::{Db, Note, stack_change};
use stacked::thread::{Role, append_turn};
use stacked::tui::{App, Effect, Focus, Layout, Mode, Pane, PatchesPane, Screen, Show, Spot};

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

/// stacked run in `dir` on the database `db`, as the tests run it: its own
/// things (its settings, the databases opened last) kept in `home` beside
/// the database (`$STACKED_HOME`), never in the home of whoever runs the
/// tests; what it writes in the temporary directory, in `tmp` beside it
/// (`$TMPDIR`), removed with the test's folder.
fn stacked(dir: &Path, db: &Path, args: &[&str]) {
    let tmp = db.with_file_name("tmp");
    std::fs::create_dir_all(&tmp).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_stacked"))
        .current_dir(dir)
        .arg("-f")
        .arg(db)
        .args(args)
        .env("STACKED_HOME", db.with_file_name("home"))
        .env("TMPDIR", tmp)
        .env_remove("STACKED_CONFIG")
        .env_remove("STACKED_APP_STATE")
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn the_editor_s_tests_read_and_keep_nothing_of_the_home() {
    // In the tests' own process, which the binary's start never set up:
    // no settings' file read, the defaults used, and nothing kept, wherever
    // the environment points.
    let nowhere = Path::new(stacked::home::NOWHERE);
    assert_eq!(*stacked::home::get(), stacked::home::Home::none());
    assert_eq!(stacked::config::path(), nowhere);
    let (c, wrong) = stacked::config::Config::load(&stacked::config::path());
    assert!(wrong.is_empty(), "{wrong:?}");
    assert_eq!(c.render(), stacked::config::Config::default().render());
    // The stacked an ask calls back is told so too.
    let mut c = Command::new("true");
    stacked::home::get().pass_on(&mut c);
    for (k, v) in c.get_envs() {
        assert_eq!(v, Some(nowhere.as_os_str()), "{k:?}");
    }
    assert_eq!(c.get_envs().count(), 3);
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

/// The settings of the editor's tests, rather than a file's: the defaults,
/// with the setup of what each kind of ask may use done, as a user's are
/// once it is saved, so that their asks go. The tests share their process,
/// and with it these settings: each one sets them before any reads them.
fn settings() -> stacked::config::Config {
    let mut c = stacked::config::Config::default();
    c.agent.setup = Some(stacked::setup::VERSION);
    c
}

/// The editor on `db`, with the tests' settings (`settings`): a
/// developer's own must not change them.
fn open(db: &Path) -> stacked::db::Result<Editor> {
    stacked::config::init(settings());
    // No agent of the user's is ever asked: the tests' stand-in is. No
    // browser opens a link either: a stand-in does.
    fake_claude();
    fake_opener();
    App::with_config(db, settings()).map(Editor)
}

/// The editor a test opened (`open`). Once it goes, at the end of the test,
/// it waits for the asks the test started to be over (`asks_over`), before
/// the test's folder is removed.
struct Editor(App);

impl std::ops::Deref for Editor {
    type Target = App;

    fn deref(&self) -> &App {
        &self.0
    }
}

impl std::ops::DerefMut for Editor {
    fn deref_mut(&mut self) -> &mut App {
        &mut self.0
    }
}

/// When the test failed, its asks are not waited for: the test may have
/// left an agent held for ever.
impl Drop for Editor {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            asks_over(&mut self.0);
        }
    }
}

/// Waits for the asks `app` started to be over: the work away from the
/// screen done (`settle`), and no process working in the folder of the
/// database, none whose command names what is in it, as those of an ask do
/// (its claude, the stacked that records and lands what claude did). Else
/// an ask's processes would write in the folder as it is removed, and leave
/// some of it. With no `ps` to say, it does not wait for them.
fn asks_over(app: &mut App) {
    app.settle();
    // The folder as the test named it, and as the asks name it: its real
    // path.
    let given = app.path.parent().map(Path::to_path_buf).unwrap_or_default();
    let Ok(real) = given.canonicalize() else { return };
    let inside: Vec<String> = [&given, &real].iter().filter(|d| d.is_absolute()).map(|d| format!("{}/", d.display())).collect();
    let mut working = String::new();
    for _ in 0..300 {
        let Ok(ps) = Command::new("ps").args(["axww", "-o", "command="]).output() else { break };
        working = String::from_utf8_lossy(&ps.stdout)
            .lines()
            .filter(|l| inside.iter().any(|i| l.contains(i.as_str())))
            .collect::<Vec<_>>()
            .join("\n");
        if working.is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert!(working.is_empty(), "still at work in {}:\n{working}", real.display());
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

/// A colour as shown: the terminal's own foreground or background, or one
/// of its own.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Shown {
    Fg,
    Bg,
    Is(ratatui::style::Color),
}

/// How a cell looks: its colours as shown, swapped when it is reversed,
/// and its other attributes.
fn looks(c: &ratatui::buffer::Cell) -> (Shown, Shown, ratatui::style::Modifier) {
    use ratatui::style::{Color, Modifier};
    let shown = |c: Color, own: Shown| if c == Color::Reset { own } else { Shown::Is(c) };
    let (fg, bg) = (shown(c.fg, Shown::Fg), shown(c.bg, Shown::Bg));
    match c.modifier.contains(Modifier::REVERSED) {
        true => (bg, fg, c.modifier - Modifier::REVERSED),
        false => (fg, bg, c.modifier),
    }
}

#[test]
fn the_mouse_marks_a_side_pane_s_line_and_elsewhere_the_cursor_follows_it() {
    use ratatui::style::Modifier;
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    let drawn = |app: &mut App| {
        let mut buf = Buffer::empty(Rect::new(0, 0, 120, 30));
        app.draw(&mut buf);
        buf
    };
    let screen = draw(&mut app);
    let (x, y) = at(&screen, "Bug 2 - add b");
    let (_, y1) = at(&screen, "Bug 1 - shout two");
    // Over the second patch: marked, underlined, and not picked: what was
    // picked stays so, the first patch in the middle.
    let picked = (app.focus, app.on_stack, app.sel);
    app.hover(x, y);
    assert_eq!((app.focus, app.on_stack, app.sel), picked);
    assert_eq!(app.hovered, Some(Spot::Patch(Some(1))));
    let buf = drawn(&mut app);
    assert!(buf[(x, y)].modifier.contains(Modifier::UNDERLINED) && !buf[(x, y1)].modifier.contains(Modifier::UNDERLINED));
    assert!(!draw(&mut app).contains("+bee"), "the first patch still in the middle");
    // A click picks it, its patch in the middle; another is Enter: read,
    // the cursor in the middle.
    app.click(x, y, false);
    assert_eq!((app.focus, app.on_stack, app.sel), (Focus::List, false, 1));
    let buf = drawn(&mut app);
    assert!(buf[(x, y)].modifier.contains(Modifier::REVERSED | Modifier::UNDERLINED), "picked, and under the mouse");
    assert!(draw(&mut app).contains("+bee"));
    app.click(x, y, false);
    assert_eq!(app.focus, Focus::Diff, "a second click");
    // The mouse gone from the screen: nothing marked.
    app.unhover();
    assert!(!drawn(&mut app)[(x, y)].modifier.contains(Modifier::UNDERLINED));
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
    // Writing, a click in the comment's box leaves it open.
    let (x, y) = at(&draw(&mut app), "a comment on:");
    app.click(x, y + 1, false);
    assert!(matches!(app.mode, Mode::Edit(_)));
    save(&mut app, "a bee");
    // Outside a menu, a click closes it.
    keys(&mut app, &[KeyCode::Char(' ')]);
    draw(&mut app);
    app.click(0, 29, false);
    assert_eq!(app.mode, Mode::Normal);
    // The comment, in the comments pane (from column 90): marked by the
    // mouse over it, not picked; picked by a click, then gone to.
    let (x, y) = at_from(&draw(&mut app), "a bee", 90);
    let focus = app.focus;
    app.hover(x, y);
    assert_eq!((app.focus, app.hovered), (focus, Some(Spot::Note(0))));
    assert_ne!(app.focus, Focus::Notes);
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

/// The editor that `open` opens, clicked with each button. Out of its box,
/// on `out`, a click closes it when nothing is written, and picks nothing
/// of what is under it. With something written, a click out of the box
/// opens the menu that Esc opens, next to the click as any menu a click
/// opens, though it covers what is written there; a click out of that menu
/// goes back to what is written, as Keep writing does. Opened with Esc, the
/// menu is beside what is written, which stays in sight, and a click on
/// what is written goes back to it too. A click in the box puts the cursor
/// where the click is, and one on the box's edge does nothing.
fn clicked_in_and_out_of_the_editor(app: &mut App, open: impl Fn(&mut App), out: &str) {
    let picked = |app: &App| (app.focus, app.sel, app.cursor, app.note_sel);
    let writing = |app: &App, text: &str| matches!(&app.mode, Mode::Edit(e) if e.ed.text() == text);
    let was = picked(app);
    for right in [false, true] {
        // The menu open, drawn where a click on `(x, y)` puts it.
        let placed = |app: &mut App, (x, y): (u16, u16)| {
            let screen = draw(app);
            let (bx, by, w, h) = menu_box(&screen, "the comment");
            assert_eq!((bx, by), next_to_click((x, y), w, h), "right: {right}:\n{screen}");
        };
        open(app);
        assert!(writing(app, ""), "{:?}", app.mode);
        let (x, y) = at(&draw(app), out);
        assert_eq!(app.click(x, y, right), Effect::None);
        assert_eq!(app.mode, Mode::Normal, "nothing written, closed (right: {right})");
        assert_eq!(picked(app), was, "nothing picked (right: {right})");
        open(app);
        typed(app, "a zebra?");
        let (x, y) = at(&draw(app), out);
        app.click(x, y, right);
        assert!(matches!(&app.mode, Mode::Menu(m) if m.title == "the comment"), "right: {right}: {:?}", app.mode);
        assert_eq!(picked(app), was, "nothing picked (right: {right})");
        let menu = items(app);
        placed(app, (x, y));
        // Out of the menu, on the screen's top line: back to what is
        // written, as it was. Out of its box again, there: the menu again,
        // next to that click.
        app.click(0, 0, right);
        assert!(writing(app, "a zebra?"), "out of the menu (right: {right}): {:?}", app.mode);
        draw(app);
        app.click(0, 0, right);
        assert_eq!(items(app), menu, "right: {right}");
        placed(app, (0, 0));
        let (x, y) = at(&draw(app), "Keep writing");
        app.click(x, y, right);
        assert!(writing(app, "a zebra?"), "kept writing (right: {right}): {:?}", app.mode);
        // Opened with Esc, the same menu, beside what is written, which stays
        // in sight: a click on what is written, out of the menu, goes back to
        // it.
        keys(app, &[KeyCode::Esc]);
        assert_eq!(items(app), menu, "right: {right}");
        let (x, y) = at(&draw(app), "zebra?");
        app.click(x, y, right);
        assert!(writing(app, "a zebra?"), "back to it (right: {right}): {:?}", app.mode);
        // In the box: the cursor where the click is; on its edge, where it
        // was.
        let (x, y) = at(&draw(app), "zebra?");
        app.click(x, y, right);
        assert_eq!(draw_at(app).1, Some((x, y)), "right: {right}");
        typed(app, "striped ");
        assert!(writing(app, "a striped zebra?"), "right: {right}: {:?}", app.mode);
        let (screen, cursor) = draw_at(app);
        let (x, y) = at(&screen, "enter: a new line");
        app.click(x, y, right);
        assert_eq!(draw_at(app).1, cursor, "its edge (right: {right})");
        assert!(writing(app, "a striped zebra?"), "right: {right}: {:?}", app.mode);
        // Esc opens the same menu as the click out of the box did.
        keys(app, &[KeyCode::Esc]);
        assert_eq!(items(app), menu, "right: {right}");
        choose(app, "Throw it away");
        assert_eq!((app.mode.clone(), picked(app)), (Mode::Normal, was), "right: {right}");
    }
}

#[test]
fn a_click_out_of_the_comment_being_written_is_esc_and_one_in_it_puts_the_cursor_there() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    // A comment on a line, in the middle, in each of the three views of the
    // code; out of its box, a patch.
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    for _ in 0..3 {
        clicked_in_and_out_of_the_editor(&mut app, |app| keys(app, &[KeyCode::Enter]), "Bug 2 - add b");
        keys(&mut app, &[KeyCode::Char('1')]);
    }
    // A comment on the patch, under its title in the patches pane; out of
    // its box, a line of the patch.
    keys(&mut app, &[KeyCode::Left]);
    let comment = |app: &mut App| {
        choose(app, "Comment on this patch…");
    };
    clicked_in_and_out_of_the_editor(&mut app, comment, "+TWO");
    assert!(notes_of(&db, "s", 0).is_empty(), "nothing saved");
}

#[test]
fn a_click_out_of_a_reply_is_esc_in_the_comments_pane_and_in_the_middle() {
    let (_tmp, db, _src) = setup();
    let (mut app, _) = a_thread_with_references(&db);
    let reply = |app: &mut App| {
        choose_as(app, "Reply", "here");
    };
    // In the comments pane, under the comment; out of its box, a patch.
    keys(&mut app, &[KeyCode::Right]);
    assert_eq!(app.focus, Focus::Notes);
    clicked_in_and_out_of_the_editor(&mut app, reply, "Bug 2 - add b");
    // In the middle, between the last reply and the references; out of its
    // box, the first reference, under it.
    keys(&mut app, &[KeyCode::Left, KeyCode::Home]);
    down_to(&mut app, "     ▎   Which one?");
    clicked_in_and_out_of_the_editor(&mut app, reply, "r1 ↗ patch 1 · a.txt: TWO");
}

/// The title of the menu open.
fn menu_title(app: &App) -> String {
    match &app.mode {
        Mode::Menu(m) => m.title.clone(),
        other => panic!("no menu open: {other:?}"),
    }
}

/// Where the box of the menu titled `title` is on a screen drawn: the
/// column and the row of its top left corner, then its width and its
/// height.
fn menu_box(screen: &str, title: &str) -> (usize, usize, usize, usize) {
    let (x, y) = at(screen, &format!("┌─ {title} "));
    let (x, y) = (usize::from(x), usize::from(y));
    let lines: Vec<Vec<char>> = screen.lines().map(|l| l.chars().collect()).collect();
    let w = lines[y][x..].iter().position(|&c| c == '┐').unwrap_or_else(|| panic!("no right edge:\n{screen}")) + 1;
    let h = lines[y..].iter().position(|l| l[x] == '└').unwrap_or_else(|| panic!("no bottom edge:\n{screen}")) + 1;
    (x, y, w, h)
}

/// The screen drawn `w` columns wide and `h` rows high.
fn draw_sized(app: &mut App, w: u16, h: u16) -> String {
    let mut buf = Buffer::empty(Rect::new(0, 0, w, h));
    app.draw(&mut buf);
    (0..h).map(|y| (0..w).map(|x| buf[(x, y)].symbol()).collect::<String>()).collect::<Vec<_>>().join("\n")
}

/// Where the top left corner of a menu `w` by `h` goes, opened by a click
/// on cell `(x, y)` of the screen that `draw` draws, between its top bar
/// and its status line: across and down, just after the cell clicked where
/// the box fits whole there, else just before it, else as far along as the
/// box fits whole.
fn next_to_click((x, y): (u16, u16), w: usize, h: usize) -> (usize, usize) {
    let side = |at: usize, start: usize, len: usize, room: usize| {
        let after = (at + 1).saturating_sub(start);
        if after + len <= room {
            after
        } else if at >= start + len && at - start <= room {
            at - start - len
        } else {
            room.saturating_sub(len)
        }
    };
    (side(x.into(), 0, w, 120), 1 + side(y.into(), 1, h, 28))
}

#[test]
fn a_menu_opened_by_a_click_is_drawn_next_to_it_and_one_opened_by_a_key_in_the_middle() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    // Space, a key: the menu in the middle of the panes, which are the rows
    // between the top bar and the status line.
    keys(&mut app, &[KeyCode::Char(' ')]);
    let title = menu_title(&app);
    let keyed = menu_box(&draw(&mut app), &title);
    let (x, y, w, h) = keyed;
    assert_eq!((x, y), ((120 - w) / 2, 1 + (28 - h) / 2), "in the middle");
    keys(&mut app, &[KeyCode::Esc]);
    // A right click on the line: its menu, the top left corner of its box
    // just below and right of the cell clicked, which stays in sight.
    let (x, y) = at(&draw(&mut app), "+TWO");
    let (cx, cy) = (x + 2, y);
    app.click(cx, cy, true);
    assert_eq!(menu_title(&app), title);
    let screen = draw(&mut app);
    let placed = menu_box(&screen, &title);
    assert_eq!((placed.0, placed.1), (usize::from(cx) + 1, usize::from(cy) + 1), "{screen}");
    assert_eq!(at(&screen, "+TWO"), (x, y), "{screen}");
    // A click on a choice, where the menu is drawn, picks the choice, and
    // the menu stays where it is; so it does once what `?` says is
    // clicked away.
    for (pick, picked) in [("[ ] in $EDITOR", "[ ] here  [x] in $EDITOR"), ("[ ] here", "[x] here  [ ] in $EDITOR")] {
        let (x, y) = at(&draw(&mut app), pick);
        app.click(x + 1, y, false);
        let screen = draw(&mut app);
        assert!(screen.contains(picked), "{screen}");
        assert_eq!(menu_box(&screen, &title), placed, "{screen}");
    }
    keys(&mut app, &[KeyCode::Char('?')]);
    app.click(0, 0, false);
    assert_eq!(menu_box(&draw(&mut app), &title), placed);
    // A click on an item does the item.
    let (x, y) = at(&draw(&mut app), "Comment on this line");
    app.click(x, y, false);
    assert!(matches!(app.mode, Mode::Edit(_)), "{:?}", app.mode);
    // Space again, the mouse having gone over the line: in the middle.
    keys(&mut app, &[KeyCode::Esc]);
    app.hover(cx, cy);
    keys(&mut app, &[KeyCode::Char(' ')]);
    assert_eq!(menu_box(&draw(&mut app), &title), keyed);
}

#[test]
fn by_the_edges_of_the_screen_a_menu_opened_by_a_click_goes_left_of_it_or_over_it_whole() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    let screen = draw(&mut app);
    let ((left, high), (_, low)) = (at(&screen, "+TWO"), at(&screen, " line 40"));
    // Each time, the box is whole between the top bar and the status line,
    // and the cell clicked is in sight.
    let opened = |app: &mut App, x: u16, y: u16| {
        app.click(x, y, true);
        let screen = draw(app);
        let (bx, by, w, h) = menu_box(&screen, &menu_title(app));
        assert!(bx + w <= 120 && by >= 1 && by + h <= 29, "whole:\n{screen}");
        let (x, y) = (usize::from(x), usize::from(y));
        assert!(!(bx..bx + w).contains(&x) || !(by..by + h).contains(&y), "the cell clicked in sight:\n{screen}");
        keys(app, &[KeyCode::Esc]);
        (bx, by, w, h)
    };
    // By the right edge: left of the cell clicked, under it.
    let (x, y, w, _) = opened(&mut app, 118, high);
    assert_eq!((x + w, y), (118, usize::from(high) + 1));
    // By the bottom: right of it, over it.
    let (x, y, _, h) = opened(&mut app, left, low);
    assert_eq!((x, y + h), (usize::from(left) + 1, usize::from(low)));
    // In the corner: left of it, over it.
    let (x, y, w, h) = opened(&mut app, 118, low);
    assert_eq!((x + w, y + h), (118, usize::from(low)));
}

#[test]
fn a_menu_opened_by_a_click_stays_whole_on_a_screen_made_smaller() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    let (_, y) = at(&draw_sized(&mut app, 160, 40), " line 40");
    app.click(150, y, true);
    let title = menu_title(&app);
    // By the right edge: left of the cell clicked, under it.
    let first = menu_box(&draw_sized(&mut app, 160, 40), &title);
    assert_eq!((first.0 + first.2, first.1), (150, usize::from(y) + 1));
    // Smaller, the cell clicked off the screen: moved left and up only as
    // far as the box needs to be whole, against the right edge and the
    // status line. As large again: where it was.
    let smaller = |app: &mut App| {
        let screen = draw_sized(app, 80, 20);
        let (x, y, w, h) = menu_box(&screen, &title);
        assert_eq!((x + w, y + h), (80, 19), "{screen}");
        screen
    };
    smaller(&mut app);
    assert_eq!(menu_box(&draw_sized(&mut app, 160, 40), &title), first);
    // An item is done by a click where it is drawn.
    let (x, y) = at(&smaller(&mut app), "Comment on this line");
    app.click(x, y, false);
    assert!(matches!(app.mode, Mode::Edit(_)), "{:?}", app.mode);
}

#[test]
fn a_menu_that_a_click_on_an_item_opens_is_drawn_next_to_that_click() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    // The stack's header right-clicked: its menu. Show… clicked in that
    // menu: the menu of which comments the panes show, next to the second
    // click.
    let header = at(&draw(&mut app), "≡ s · 2 patches");
    app.click(header.0, header.1, true);
    assert_eq!(menu_title(&app), "the stack s");
    let (x, y) = at(&draw(&mut app), "Show…");
    app.click(x, y, false);
    assert_eq!(menu_title(&app), "show the comments");
    let screen = draw(&mut app);
    let (bx, by, _, _) = menu_box(&screen, "show the comments");
    assert_eq!((bx, by), (usize::from(x) + 1, usize::from(y) + 1), "{screen}");
    // Show… chosen with keys in the menu a click opened: in the middle.
    keys(&mut app, &[KeyCode::Esc]);
    app.click(header.0, header.1, true);
    draw(&mut app);
    choose(&mut app, "Show…");
    let (bx, by, w, h) = menu_box(&draw(&mut app), "show the comments");
    assert_eq!((bx, by), ((120 - w) / 2, 1 + (28 - h) / 2));
}

#[test]
fn a_box_checked_in_a_menu_a_click_opened_leaves_the_menu_where_it_is() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "a zebra?");
    // The banner right-clicked, on the screen's second row: what waits on
    // you, next to the click, over the banner's lines.
    draw(&mut app);
    app.click(10, 1, true);
    let title = "what waits on you";
    assert_eq!(menu_title(&app), title);
    let placed = |app: &mut App| {
        let (x, y, _, _) = menu_box(&draw(app), title);
        (x, y)
    };
    assert_eq!(placed(&mut app), (11, 2));
    // The comment checked with a click, then unchecked with Enter: in the
    // batch, then out of it, and the menu, built again each time, where it
    // was.
    let (x, y) = at(&draw(&mut app), "[ ] patch 1");
    app.click(x, y, false);
    assert_eq!(batched(&db).len(), 1);
    assert!(draw(&mut app).contains("[x] patch 1"));
    assert_eq!(placed(&mut app), (11, 2));
    keys(&mut app, &[KeyCode::Enter]);
    assert!(batched(&db).is_empty());
    assert!(draw(&mut app).contains("[ ] patch 1"));
    assert_eq!(placed(&mut app), (11, 2));
}

#[test]
fn a_click_out_of_a_comment_opens_its_menu_next_to_the_click_over_the_comment_too_and_esc_beside_it() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    typed(&mut app, "a zebra?");
    let screen = draw(&mut app);
    let comment = at(&screen, "a zebra?");
    // Out of the comment's box, on the line it is on, just over it: the
    // menu asking what to do with the comment, next to the click, over the
    // comment.
    let (x, y) = at(&screen, "+TWO");
    app.click(x, y, false);
    assert_eq!(menu_title(&app), "the comment");
    let screen = draw(&mut app);
    let (bx, by, _, _) = menu_box(&screen, "the comment");
    assert_eq!((bx, by), (usize::from(x) + 1, usize::from(y) + 1), "{screen}");
    assert!(!screen.contains("a zebra?"), "covered:\n{screen}");
    // A click out of the menu: back to the comment, what is written in it
    // kept, in sight again.
    app.click(0, 0, false);
    assert!(matches!(&app.mode, Mode::Edit(e) if e.ed.text() == "a zebra?"), "{:?}", app.mode);
    assert_eq!(at(&draw(&mut app), "a zebra?"), comment);
    // Esc, a key: the menu under the comment, in the middle across, and
    // the comment in sight.
    keys(&mut app, &[KeyCode::Esc]);
    let screen = draw(&mut app);
    let (_, bottom) = at(&screen, "└─ enter: a new line");
    let (bx, by, w, _) = menu_box(&screen, "the comment");
    assert_eq!((bx, by), ((120 - w) / 2, usize::from(bottom) + 1), "{screen}");
    assert_eq!(at(&screen, "a zebra?"), comment);
    choose(&mut app, "Throw it away");
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
    assert!(screen.contains("enter: read it · space: menu · 1: view · 2: read · 3: whole · ?: what can I do here"), "{screen}");
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
fn the_compact_panes_keep_the_cursor_in_sight_on_a_shorter_screen() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    draw(&mut app);
    keys(&mut app, &[KeyCode::Enter, KeyCode::Char('1'), KeyCode::Char('1')]);
    assert_eq!(app.layout, Layout::Compact);
    down_to(&mut app, "+TWO");
    // Drawn 10 lines high, the cursor's line past the panes: its row comes
    // up to their last line, still the cursor's.
    let mut buf = Buffer::empty(Rect::new(0, 0, 120, 10));
    app.draw(&mut buf);
    let screen: Vec<String> = (0..10).map(|y| (0..120).map(|x| buf[(x, y)].symbol()).collect()).collect();
    assert_eq!(app.rows[app.cursor].text, "+TWO", "{}", screen.join("\n"));
    assert!(screen[8].contains("+TWO"), "on the last line of the panes:\n{}", screen.join("\n"));
    // Taller again: where it is.
    let screen = draw(&mut app);
    assert_eq!(app.rows[app.cursor].text, "+TWO", "{screen}");
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
    // Only 1, 2, 3 and / are keys of their own: 1, 2 and / as in self-review.
    for c in ('a'..='z').chain('A'..='Z').chain("0456789!@#$%^&*()-=_+[]{};:'\",.<>\\|`~".chars()) {
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
    stacked::config::init(settings());
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
    stacked::config::init(settings());
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
fn a_comment_on_a_file_s_name_is_on_the_file_as_a_whole() {
    use stacked::rows::Kind;
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "a.txt");
    assert_eq!(app.rows[app.cursor].kind, Kind::File);
    // Said so where the cursor is, and in its menu.
    key(&mut app, KeyCode::Char('?'));
    let screen = draw(&mut app);
    assert!(screen.contains("Press Enter to create a comment on this file, as a whole."), "{screen}");
    keys(&mut app, &[KeyCode::Esc, KeyCode::Char(' ')]);
    let Mode::Menu(m) = &app.mode else { panic!("{:?}", app.mode) };
    assert_eq!(m.title, "file: a.txt");
    assert!(m.items.iter().any(|i| i.label == "Comment on this file"), "{:?}", m.items);
    key(&mut app, KeyCode::Char('?'));
    assert!(draw(&mut app).contains("Press Enter to create a comment on this file, as a whole."));
    // Written: under the file's name, before its hunks, and said to be on
    // the file as a whole.
    keys(&mut app, &[KeyCode::Esc, KeyCode::Esc, KeyCode::Enter]);
    assert!(matches!(app.mode, Mode::Edit(_)), "{:?}", app.mode);
    save(&mut app, "why this file?");
    let head = app.rows.iter().position(|r| r.head && r.note.is_some()).unwrap();
    assert_eq!((app.rows[head - 1].kind, app.rows[head - 1].text.starts_with("a.txt")), (Kind::File, true));
    assert!(app.rows[head..].iter().any(|r| r.kind == Kind::Hunk), "its hunks under it");
    let screen = draw(&mut app);
    assert!(screen.contains("a.txt, as a whole"), "{screen}");
    let n = notes_of(&db, "s", 0).into_iter().find(|n| n.text.contains("why this file?")).unwrap();
    assert_eq!((n.file.as_str(), stacked::diff::is_file_key(&n.line)), ("a.txt", true));
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

/// The search of the middle open: what is typed, whether it is selected,
/// and what the bar says of the matches.
fn finding(app: &App) -> (String, bool, String) {
    match &app.mode {
        Mode::Find(f) => (f.ed.line().0, f.selected, f.said.clone()),
        other => panic!("no search open: {other:?}"),
    }
}

/// The row the middle's cursor is on.
fn on_row(app: &App) -> String {
    app.rows[app.cursor].text.clone()
}

/// The screen drawn, 120 wide and `h` high.
fn drawn(app: &mut App, h: u16) -> Buffer {
    let mut buf = Buffer::empty(Rect::new(0, 0, 120, h));
    app.draw(&mut buf);
    buf
}

/// What `buf` shows, a line for each of its rows.
fn text_of(buf: &Buffer) -> String {
    (0..buf.area.height)
        .map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

/// The attributes of each cell of `text`, where the screen in `buf` shows
/// it from column `from` on.
fn attrs(buf: &Buffer, text: &str, from: usize) -> Vec<ratatui::style::Modifier> {
    let (x, y) = at_from(&text_of(buf), text, from);
    (0..text.chars().count() as u16).map(|k| buf[(x + k, y)].modifier).collect()
}

/// Where the middle starts on the screens of `setup`: past the patches
/// pane, expanded.
const MIDDLE: usize = 31;

#[test]
fn slash_searches_the_middle_as_it_is_typed_and_tab_goes_round_its_matches() {
    use ratatui::style::Modifier;
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    draw(&mut app);
    // From the patches pane: the search is the middle's, the cursor there.
    assert_eq!((app.focus, app.cursor), (Focus::List, 0));
    keys(&mut app, &[KeyCode::Char('/')]);
    assert_eq!(finding(&app), (String::new(), false, String::new()));
    assert_eq!(app.focus, Focus::Diff);
    let (screen, cursor) = draw_at(&mut app);
    assert!(screen.lines().last().unwrap().starts_with(" /   type what to look for · esc: back"), "{screen}");
    assert_eq!(cursor, Some((2, 29)), "after the /, in the bottom bar");
    // As it is typed: on the first match from the cursor down.
    typed(&mut app, "tw");
    assert_eq!(on_row(&app), "   Bug 1 - shout two");
    typed(&mut app, "o");
    assert_eq!(finding(&app), ("two".to_string(), false, "match 1 of 3".to_string()));
    let (screen, cursor) = draw_at(&mut app);
    let bar = screen.lines().last().unwrap();
    assert!(
        bar.starts_with(" /two   match 1 of 3   tab: the next match · shift-tab: the one before · enter: stay there · esc: back where you were"),
        "{bar}"
    );
    assert_eq!(cursor, Some((5, 29)));
    // Lit, each match reversed from how its line is drawn: on the cursor's
    // line, out of the cursor, bold and underlined too.
    let buf = drawn(&mut app, 30);
    let other = attrs(&buf, "-two", MIDDLE);
    assert!(!other[0].contains(Modifier::REVERSED) && other[1..].iter().all(|m| m.contains(Modifier::REVERSED)), "{other:?}");
    let current = attrs(&buf, "shout two", MIDDLE);
    assert!(current[..6].iter().all(|m| m.contains(Modifier::REVERSED)), "the cursor's line: {current:?}");
    assert!(
        current[6..].iter().all(|m| !m.contains(Modifier::REVERSED) && m.contains(Modifier::BOLD | Modifier::UNDERLINED)),
        "its match: {current:?}"
    );
    // Tab goes on, past the last round to the first; shift-Tab back.
    for (k, row, said) in [
        (KeyCode::Tab, "-two", "match 2 of 3"),
        (KeyCode::Tab, "+TWO", "match 3 of 3"),
        (KeyCode::Tab, "   Bug 1 - shout two", "match 1 of 3: the search started again at the top"),
        (KeyCode::BackTab, "+TWO", "match 3 of 3: the search started again at the bottom"),
        (KeyCode::BackTab, "-two", "match 2 of 3"),
    ] {
        keys(&mut app, &[k]);
        assert_eq!((on_row(&app), finding(&app).2), (row.to_string(), said.to_string()), "{k:?}");
    }
    // Enter stays there, the search over, nothing lit; the cursor in the
    // middle, the bar's keys back.
    keys(&mut app, &[KeyCode::Enter]);
    assert_eq!((app.mode.clone(), app.focus, on_row(&app)), (Mode::Normal, Focus::Diff, "-two".to_string()));
    assert!(app.status.is_empty(), "{}", app.status);
    let buf = drawn(&mut app, 30);
    assert!(attrs(&buf, "+TWO", MIDDLE).iter().all(|m| m.is_empty()), "nothing lit");
    assert!(text_of(&buf).lines().last().unwrap().starts_with(" enter: comment · space: menu"), "{}", text_of(&buf));
}

#[test]
fn esc_ends_the_search_of_the_middle_where_the_cursor_was_and_a_click_out_of_it_does_too() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    // Twelve lines high: the middle has ten, the second hunk out of sight.
    drawn(&mut app, 12);
    key(&mut app, KeyCode::Right);
    key(&mut app, KeyCode::Down);
    drawn(&mut app, 12);
    let was = (app.cursor, app.top);
    assert_eq!(was, (1, 0));
    key(&mut app, KeyCode::Char('/'));
    typed(&mut app, "line 40");
    let screen = text_of(&drawn(&mut app, 12));
    assert_eq!(on_row(&app), " line 40");
    assert!(app.top > 1 && screen.contains(" line 40"), "scrolled to it: {screen}");
    // Esc: back where it was, scrolled as it was.
    key(&mut app, KeyCode::Esc);
    let screen = text_of(&drawn(&mut app, 12));
    assert_eq!((app.cursor, app.top, app.focus, app.mode.clone()), (was.0, was.1, Focus::Diff, Mode::Normal));
    assert!(!screen.contains(" line 40"), "{screen}");
    assert_eq!(app.status, "the search is cancelled: the cursor is back where it was");
    // From the patches pane, back there; a click out of the bar is Esc, and
    // picks nothing.
    key(&mut app, KeyCode::Left);
    draw(&mut app);
    keys(&mut app, &[KeyCode::Char('/')]);
    typed(&mut app, "LINE 39");
    let screen = draw(&mut app);
    assert_eq!(on_row(&app), "-line 39");
    let (x, y) = at(&screen, "Bug 2 - add b");
    app.click(x, y, false);
    assert_eq!((app.mode.clone(), app.focus, app.sel, app.cursor), (Mode::Normal, Focus::List, 0, 1));
    // A click on the bar: nothing.
    keys(&mut app, &[KeyCode::Char('/')]);
    app.click(3, 29, false);
    assert!(matches!(app.mode, Mode::Find(_)), "{:?}", app.mode);
}

#[test]
fn the_next_slash_opens_with_the_last_search_selected_and_up_and_down_go_through_those_before() {
    use ratatui::style::Modifier;
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    draw(&mut app);
    keys(&mut app, &[KeyCode::Right]);
    // Kept as Enter ends them, the latest first: not as Esc does, nor when
    // nothing matched.
    for (q, end) in [("two", KeyCode::Enter), ("three", KeyCode::Enter), ("line 40", KeyCode::Esc), ("zebra", KeyCode::Enter)] {
        keys(&mut app, &[KeyCode::Char('/')]);
        typed(&mut app, q);
        keys(&mut app, &[end]);
    }
    assert_eq!(app.finds, ["three", "two"]);
    assert_eq!(on_row(&app), " three");
    // The next opens with the last, selected, searched at once.
    keys(&mut app, &[KeyCode::Char('/')]);
    assert_eq!(finding(&app), ("three".to_string(), true, "match 1 of 1".to_string()));
    let buf = drawn(&mut app, 30);
    assert!(text_of(&buf).lines().last().unwrap().starts_with(" /three   match 1 of 1"), "{}", text_of(&buf));
    assert!((2..7).all(|x| buf[(x, 29)].modifier.contains(Modifier::REVERSED)), "selected");
    // What is typed takes its place, searched from where the cursor was.
    typed(&mut app, "line 4");
    assert_eq!((finding(&app), on_row(&app)), (("line 4".to_string(), false, "match 1 of 1".to_string()), " line 40".to_string()));
    // ↑ the searches before, each selected and searched; ↓ the later, then
    // what was typed.
    for (k, text, selected, row) in [
        (KeyCode::Up, "three", true, " three"),
        (KeyCode::Up, "two", true, "   Bug 1 - shout two"),
        (KeyCode::Up, "two", true, "   Bug 1 - shout two"),
        (KeyCode::Down, "three", true, " three"),
        (KeyCode::Down, "line 4", false, " line 40"),
        (KeyCode::Down, "line 4", false, " line 40"),
    ] {
        keys(&mut app, &[k]);
        assert_eq!((finding(&app).0, finding(&app).1, on_row(&app)), (text.to_string(), selected, row.to_string()), "{k:?}");
    }
    keys(&mut app, &[KeyCode::Up, KeyCode::Up]);
    assert_eq!(finding(&app).2, "match 1 of 3: the search started again at the top", "two, past the cursor's row");
    // Selected, Backspace takes it away: nothing searched, the cursor
    // where it was.
    keys(&mut app, &[KeyCode::Backspace]);
    assert_eq!((finding(&app), on_row(&app)), ((String::new(), false, String::new()), " three".to_string()));
    keys(&mut app, &[KeyCode::Esc]);
    assert_eq!(app.finds, ["three", "two"], "as they were");
}

#[test]
fn a_search_of_the_middle_ignores_case_and_the_signs_of_the_code_and_says_when_nothing_matches() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    draw(&mut app);
    keys(&mut app, &[KeyCode::Right]);
    // Whatever their case, capitals typed too: as self-review's search.
    for q in ["TWO", "Two", "two"] {
        keys(&mut app, &[KeyCode::Char('/')]);
        typed(&mut app, q);
        assert_eq!((finding(&app).2, on_row(&app)), ("match 1 of 3".to_string(), "   Bug 1 - shout two".to_string()), "{q}");
        keys(&mut app, &[KeyCode::Esc]);
    }
    // A line of code is read without its sign: no line is -two.
    keys(&mut app, &[KeyCode::Char('/')]);
    typed(&mut app, "-two");
    assert_eq!((finding(&app).2, app.cursor), ("nothing matches".to_string(), 0), "back where it was");
    let screen = draw(&mut app);
    assert!(screen.lines().last().unwrap().starts_with(" /-two   nothing matches   enter or esc: back where you were"), "{screen}");
    // Enter with nothing matched: as Esc, and said.
    keys(&mut app, &[KeyCode::Enter]);
    assert_eq!((app.mode.clone(), app.cursor), (Mode::Normal, 0));
    assert_eq!(app.status, "nothing matches \"-two\": the cursor is back where it was");
    assert!(app.finds.is_empty(), "{:?}", app.finds);
}

#[test]
fn a_search_of_the_middle_finds_a_comment_the_message_and_a_file_s_name() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "why so loud?");
    keys(&mut app, &[KeyCode::Home]);
    for (q, row) in [("loud", "     ▎   why so loud?"), ("because", "   Because."), ("a.txt", "a.txt")] {
        keys(&mut app, &[KeyCode::Char('/')]);
        typed(&mut app, q);
        assert_eq!(on_row(&app), row, "{q}");
        keys(&mut app, &[KeyCode::Esc]);
    }
    // From the comments pane, the middle's all the same: Esc goes back to
    // the comments, Enter leaves the cursor in the middle, on the match.
    keys(&mut app, &[KeyCode::Right]);
    assert_eq!(app.focus, Focus::Notes);
    for (end, focus) in [(KeyCode::Esc, Focus::Notes), (KeyCode::Enter, Focus::Diff)] {
        keys(&mut app, &[KeyCode::Char('/')]);
        typed(&mut app, "loud");
        keys(&mut app, &[end]);
        assert_eq!(app.focus, focus, "{end:?}");
    }
    assert_eq!(on_row(&app), "     ▎   why so loud?");
}

#[test]
fn a_search_of_the_middle_side_by_side_goes_to_the_row_of_either_side() {
    use ratatui::style::Modifier;
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    draw(&mut app);
    keys(&mut app, &[KeyCode::Right, KeyCode::Char('1')]);
    assert_eq!(app.layout, Layout::Split);
    keys(&mut app, &[KeyCode::Char('/')]);
    typed(&mut app, "two");
    keys(&mut app, &[KeyCode::Tab]);
    assert_eq!(on_row(&app), "-two", "the old side's");
    // Both sides lit, on one line: on the cursor's side, the match gone to.
    let buf = drawn(&mut app, 30);
    let screen = text_of(&buf);
    assert_eq!(at(&screen, "-two").1, at(&screen, "+TWO").1, "{screen}");
    let (old, new) = (attrs(&buf, "-two", MIDDLE), attrs(&buf, "+TWO", MIDDLE));
    assert!(old[1..].iter().all(|m| m.contains(Modifier::BOLD | Modifier::UNDERLINED) && !m.contains(Modifier::REVERSED)), "{old:?}");
    assert!(new[1..].iter().all(|m| m.contains(Modifier::REVERSED) && !m.contains(Modifier::BOLD)), "{new:?}");
    keys(&mut app, &[KeyCode::Tab]);
    assert_eq!(on_row(&app), "+TWO", "the new side's");
    keys(&mut app, &[KeyCode::Esc]);
    // Compact, each side a pane of its own: the same.
    keys(&mut app, &[KeyCode::Char('1')]);
    assert_eq!(app.layout, Layout::Compact);
    keys(&mut app, &[KeyCode::Char('/')]);
    typed(&mut app, "two");
    for row in ["-two", "+TWO", "   Bug 1 - shout two", "-two"] {
        keys(&mut app, &[KeyCode::Tab]);
        assert_eq!(on_row(&app), row);
    }
    // Both scrolling, the cursor's line is lit across: the other side's
    // match reversed from it, not bold.
    let buf = drawn(&mut app, 30);
    let new = attrs(&buf, "+TWO", MIDDLE);
    assert!(new[1..].iter().all(|m| !m.contains(Modifier::REVERSED) && !m.contains(Modifier::BOLD)), "{new:?}\n{}", text_of(&buf));
    assert!(new[0].contains(Modifier::REVERSED), "{new:?}");
    keys(&mut app, &[KeyCode::Enter]);
    assert_eq!((app.mode.clone(), on_row(&app)), (Mode::Normal, "-two".to_string()));
}

#[test]
fn a_search_of_the_middle_while_reading_types_what_space_would_mark() {
    use ratatui::style::Modifier;
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    draw(&mut app);
    keys(&mut app, &[KeyCode::Right, KeyCode::Char('2')]);
    assert!(app.reading);
    let before = std::fs::read(&db).unwrap();
    keys(&mut app, &[KeyCode::Char('/')]);
    // The second hunk's header, git's line 35 after it, then its lines.
    typed(&mut app, "line 3");
    assert_eq!((finding(&app).2, on_row(&app)), ("match 1 of 6".to_string(), "@@ -39,5 +39,5 @@ line 35   read 0/2 lines".to_string()));
    keys(&mut app, &[KeyCode::Tab]);
    assert_eq!(on_row(&app), " line 36");
    // Lit past the margin.
    let buf = drawn(&mut app, 30);
    let next = attrs(&buf, "line 37", MIDDLE);
    assert!(next[..6].iter().all(|m| m.contains(Modifier::REVERSED)) && !next[6].contains(Modifier::REVERSED), "{next:?}");
    keys(&mut app, &[KeyCode::Tab, KeyCode::Enter]);
    assert_eq!((app.mode.clone(), on_row(&app)), (Mode::Normal, " line 37".to_string()));
    assert_eq!(std::fs::read(&db).unwrap(), before, "nothing marked read");
}

#[test]
fn the_search_of_the_middle_s_key_is_the_settings_and_its_own_keys_are_as_they_are() {
    let (_tmp, db, _src) = setup();
    let (cfg, wrong) = stacked::config::Config::parse("[keys]\nfind = \"f\"\nback = \"q\"\n").unwrap();
    assert!(wrong.is_empty(), "{wrong:?}");
    stacked::config::init(settings());
    let mut app = App::with_config(&db, cfg).unwrap();
    draw(&mut app);
    keys(&mut app, &[KeyCode::Char('/')]);
    assert_eq!(app.mode, Mode::Normal, "/ no longer is");
    keys(&mut app, &[KeyCode::Char('f')]);
    assert!(matches!(app.mode, Mode::Find(_)), "f is: {:?}", app.mode);
    // Its own keys as they are: q and f typed, Esc back, as the bar says.
    typed(&mut app, "qf");
    assert_eq!(finding(&app).0, "qf");
    let screen = draw(&mut app);
    assert!(screen.lines().last().unwrap().contains("nothing matches   enter or esc: back where you were"), "{screen}");
    keys(&mut app, &[KeyCode::Esc]);
    assert_eq!(app.mode, Mode::Normal);
    // The help names it as the settings do.
    keys(&mut app, &[KeyCode::Char('?')]);
    let screen = draw(&mut app);
    assert!(screen.contains("Press f to search what the middle shows"), "{screen}");
}

#[test]
fn the_search_s_keys_from_match_to_match_are_the_settings_and_may_be_keys_of_the_screens_too() {
    let (_tmp, db, _src) = setup();
    let ctrl = |c| KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL);
    // Remapped, ctrl-n and ctrl-p go from match to match; Tab no longer.
    let (cfg, wrong) = stacked::config::Config::parse("[keys]\nfind_next = \"ctrl-n\"\nfind_previous = \"ctrl-p\"\n").unwrap();
    assert!(wrong.is_empty(), "{wrong:?}");
    stacked::config::init(settings());
    let mut app = App::with_config(&db, cfg).unwrap();
    draw(&mut app);
    keys(&mut app, &[KeyCode::Right, KeyCode::Char('/')]);
    typed(&mut app, "two");
    assert_eq!(app.key(ctrl('n')), Effect::None);
    assert_eq!(on_row(&app), "-two");
    assert_eq!(app.key(ctrl('p')), Effect::None);
    assert_eq!(on_row(&app), "   Bug 1 - shout two");
    keys(&mut app, &[KeyCode::Tab]);
    assert_eq!((on_row(&app), finding(&app).0), ("   Bug 1 - shout two".to_string(), "two".to_string()), "Tab does nothing there now");
    let screen = draw(&mut app);
    assert!(screen.lines().last().unwrap().contains("ctrl-n: the next match · ctrl-p: the one before"), "{screen}");
    keys(&mut app, &[KeyCode::Esc]);
    let said = "In the search, ctrl-n goes to the next match and ctrl-p to the one before";
    assert!(app.help().iter().any(|l| l.contains(said)), "{:?}", app.help());
    // Tab on the screens, and in the search still: each its own there.
    let (cfg, wrong) = stacked::config::Config::parse("[keys]\nmenu = \"tab\"\n").unwrap();
    assert!(wrong.is_empty(), "{wrong:?}");
    let mut app = App::with_config(&db, cfg).unwrap();
    draw(&mut app);
    keys(&mut app, &[KeyCode::Right, KeyCode::Tab]);
    assert!(matches!(app.mode, Mode::Menu(_)), "Tab is the menu: {:?}", app.mode);
    keys(&mut app, &[KeyCode::Esc, KeyCode::Char('/')]);
    typed(&mut app, "two");
    keys(&mut app, &[KeyCode::Tab]);
    assert_eq!((on_row(&app), finding(&app).2), ("-two".to_string(), "match 2 of 3".to_string()), "and the next match in the search");
    // Space, the menu no longer, is typed there as ever.
    typed(&mut app, " ");
    assert_eq!(finding(&app).0, "two ");
}

#[test]
fn a_search_of_the_middle_reads_a_line_folded_as_one_and_goes_to_the_piece_of_its_match() {
    use ratatui::style::Modifier;
    use stacked::rows::Kind;
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir(&src).unwrap();
    git(&src, &["init", "-q", "-b", "main"]);
    git(&src, &["config", "commit.gpgsign", "false"]);
    git(&src, &["config", "tag.gpgsign", "false"]);
    std::fs::write(src.join("a.txt"), "one\n").unwrap();
    git(&src, &["add", "."]);
    git(&src, &["commit", "-qm", "base"]);
    // A line of code with a zebra far in it, and a summary of words each
    // there once, both too wide for the middle.
    let long = format!("two{} zebra{}", " and more".repeat(20), " and more".repeat(10));
    std::fs::write(src.join("a.txt"), format!("one\n{long}\n")).unwrap();
    let summary = "alpha bravo charlie delta echo foxtrot golf hotel india juliet kilo lima mike november oscar papa quebec romeo sierra tango uniform victor whiskey xray yankee zulu";
    git(&src, &["commit", "-qam", &format!("Bug 1 - add a long line\n\n{summary}")]);
    let db = tmp.path().join("db.jsonl");
    stacked(&src, &db, &["import", "s", "HEAD~1"]);
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    // The summary, folded between words: the words either side of the fold
    // match as one, from its first piece, lit on both.
    let said: Vec<(usize, String)> = (0..app.rows.len()).filter(|&i| app.rows[i].msg_at == Some(2)).map(|i| (i, app.rows[i].text.trim().to_string())).collect();
    assert!(said.len() > 1, "{said:?}");
    let (last, next) = (said[0].1.rsplit(' ').next().unwrap().to_string(), said[1].1.split(' ').next().unwrap().to_string());
    keys(&mut app, &[KeyCode::Char('/')]);
    typed(&mut app, &format!("{last} {next}"));
    assert_eq!((app.cursor, finding(&app).2), (said[0].0, "match 1 of 1".to_string()));
    let buf = drawn(&mut app, 30);
    let lit = attrs(&buf, &next, MIDDLE);
    assert!(lit.iter().all(|m| m.contains(Modifier::BOLD | Modifier::UNDERLINED)), "{lit:?}");
    keys(&mut app, &[KeyCode::Esc]);
    // A word of the second piece: the cursor on it.
    keys(&mut app, &[KeyCode::Char('/')]);
    typed(&mut app, &next);
    assert_eq!(app.cursor, said[1].0);
    keys(&mut app, &[KeyCode::Esc]);
    // The code: the cursor on the piece holding the match; its line one
    // match, however many of its pieces hold it.
    keys(&mut app, &[KeyCode::Char('/')]);
    typed(&mut app, "zebra");
    let row = app.rows[app.cursor].clone();
    assert!(row.kind == Kind::Add && row.cont && row.text.contains("zebra"), "{:?}", row.text);
    keys(&mut app, &[KeyCode::Esc, KeyCode::Char('/')]);
    typed(&mut app, "and more");
    assert_eq!((finding(&app).2, app.rows[app.cursor].cont), ("match 1 of 1".to_string(), false));
}

#[test]
fn with_each_file_whole_the_search_finds_the_lines_no_hunk_has_and_goes_there() {
    use ratatui::style::Modifier;
    let (_tmp, db, _src) = setup_whole();
    let mut app = open(&db).unwrap();
    draw(&mut app);
    keys(&mut app, &[KeyCode::Enter]);
    // With the hunks, no hunk has a.txt's line 22.
    keys(&mut app, &[KeyCode::Char('/')]);
    typed(&mut app, "line 19");
    assert_eq!(finding(&app).2, "nothing matches");
    keys(&mut app, &[KeyCode::Esc]);
    // Each file whole: there, out of sight, and the cursor goes to it, the
    // middle scrolled to it, the match lit on its dim line.
    whole(&mut app);
    keys(&mut app, &[KeyCode::Char('/')]);
    typed(&mut app, "line 19");
    assert_eq!((finding(&app).2, on_row(&app)), ("match 1 of 1".to_string(), " line 19".to_string()));
    assert!(app.rows[app.cursor].outside, "a line in no hunk");
    let buf = drawn(&mut app, 30);
    assert!(attrs(&buf, "line 19", MIDDLE).iter().all(|m| m.contains(Modifier::BOLD | Modifier::UNDERLINED)), "{}", text_of(&buf));
    // While the bar is open, 3 is typed: the view stays.
    keys(&mut app, &[KeyCode::Backspace, KeyCode::Char('3')]);
    assert_eq!((finding(&app).0, app.whole_files), ("line 13".to_string(), true));
    keys(&mut app, &[KeyCode::Backspace, KeyCode::Char('9')]);
    // Enter there; then 3, as from any line in no hunk: the closest line
    // in a hunk, said.
    keys(&mut app, &[KeyCode::Enter]);
    assert_eq!((app.mode.clone(), on_row(&app)), (Mode::Normal, " line 19".to_string()));
    keys(&mut app, &[KeyCode::Char('3')]);
    assert_eq!((app.whole_files, on_row(&app)), (false, " line 2".to_string()));
    assert_eq!(app.status, "line 22 of a.txt is in no hunk: the cursor is on the closest line in a hunk, above it");
    // The search kept: the hunks have it not.
    keys(&mut app, &[KeyCode::Char('/')]);
    assert_eq!(finding(&app), ("line 19".to_string(), true, "nothing matches".to_string()));
    // On a line of a hunk, Enter, then 3: the same line, each file whole.
    typed(&mut app, "LINE 39");
    keys(&mut app, &[KeyCode::Enter]);
    assert_eq!(on_row(&app), "-line 39", "the line removed first, whatever its case");
    whole(&mut app);
    assert_eq!((app.whole_files, on_row(&app)), (true, "-line 39".to_string()));
}

#[test]
fn the_search_follows_each_file_whole_read_while_it_is_open() {
    let (_tmp, db, _src) = setup_whole();
    let mut app = open(&db).unwrap();
    draw(&mut app);
    keys(&mut app, &[KeyCode::Enter]);
    down_to(&mut app, "+TWO");
    // 3, and the search at once, before the middle is drawn with the files
    // read: the hunks have it not.
    key(&mut app, KeyCode::Char('3'));
    key(&mut app, KeyCode::Char('/'));
    typed(&mut app, "line 19");
    assert_eq!(finding(&app).2, "nothing matches");
    // Read, and drawn: searched again from where / was pressed, as if typed
    // now, the cursor there.
    app.wait_files();
    draw(&mut app);
    assert!(app.whole_files);
    assert_eq!((finding(&app).2, on_row(&app)), ("match 1 of 1".to_string(), " line 19".to_string()));
    // Esc: back where / was pressed, found again in the rows built anew.
    keys(&mut app, &[KeyCode::Esc]);
    assert_eq!((app.mode.clone(), on_row(&app)), (Mode::Normal, "+TWO".to_string()));
    // On a match, the rows built anew under it (narrower): the match
    // counted again, the cursor on it still.
    keys(&mut app, &[KeyCode::Char('/')]);
    typed(&mut app, "TWO");
    assert_eq!((finding(&app).2, on_row(&app)), ("match 2 of 2".to_string(), "+TWO".to_string()));
    let mut buf = Buffer::empty(Rect::new(0, 0, 100, 30));
    app.draw(&mut buf);
    assert_eq!((finding(&app).2, on_row(&app)), ("match 2 of 2".to_string(), "+TWO".to_string()));
}

/// A file deep in folders, its path longer than the patches pane is wide.
const DEEP: &str = "browser/components/urlbar/src/tui/banner.rs";

/// A database holding stack `f` of three patches: the first modifies a.txt
/// in four places and `DEEP` in one, the second b.txt, c.txt and d.txt,
/// the third a.txt; and the repository.
fn setup_files() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir_all(src.join(DEEP).parent().unwrap()).unwrap();
    git(&src, &["init", "-q", "-b", "main"]);
    git(&src, &["config", "commit.gpgsign", "false"]);
    let lines: String = (1..=40).map(|n| format!("line {n}\n")).collect();
    for f in ["a.txt", "b.txt", "c.txt", "d.txt", DEEP] {
        std::fs::write(src.join(f), &lines).unwrap();
    }
    git(&src, &["add", "."]);
    git(&src, &["commit", "-qm", "base"]);
    // Lines `at` of file `f`, from 1, in capitals.
    let shout = |f: &str, at: &[usize]| {
        let text = std::fs::read_to_string(src.join(f)).unwrap();
        let line = |(k, l): (usize, &str)| if at.contains(&(k + 1)) { l.to_uppercase() } else { l.to_string() };
        let text: String = text.lines().enumerate().map(|kl| line(kl) + "\n").collect();
        std::fs::write(src.join(f), text).unwrap();
    };
    shout("a.txt", &[2, 12, 22, 32]);
    shout(DEEP, &[5]);
    git(&src, &["commit", "-qam", "Bug 1 - two files\n\nBecause."]);
    for f in ["b.txt", "c.txt", "d.txt"] {
        shout(f, &[1]);
    }
    git(&src, &["commit", "-qam", "Bug 2 - three files"]);
    shout("a.txt", &[39]);
    git(&src, &["commit", "-qam", "Bug 3 - one file"]);
    let db = tmp.path().join("db.jsonl");
    stacked(&src, &db, &["import", "f", "HEAD~3"]);
    (tmp, db, src)
}

/// A database holding stack `w` of two patches, and the repository. Patch 1
/// changes a.txt at two lines far apart, its lines 2 and 42, adds b.txt,
/// deletes c.txt, renames d.txt to e.txt with a change, and changes f.bin,
/// which is not text; patch 2 changes a.txt's line 20 and g.txt's first
/// line, g.txt's line 10 being longer than the middle.
fn setup_whole() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir(&src).unwrap();
    git(&src, &["init", "-q", "-b", "main"]);
    git(&src, &["config", "commit.gpgsign", "false"]);
    let lines: String = (1..=40).map(|n| format!("line {n}\n")).collect();
    let dees = |changed: bool| -> String { (1..=30).map(|n| if changed && n == 15 { "DEE 15\n".to_string() } else { format!("dee {n}\n") }).collect() };
    let gs = |changed: bool| -> String {
        (1..=20)
            .map(|n| match n {
                1 if changed => "G ONE\n".to_string(),
                10 => format!("g ten, {}\n", "and on ".repeat(30)),
                n => format!("g {n}\n"),
            })
            .collect()
    };
    std::fs::write(src.join("a.txt"), format!("one\ntwo\nthree\n{lines}")).unwrap();
    std::fs::write(src.join("c.txt"), "see\nsea\n").unwrap();
    std::fs::write(src.join("d.txt"), dees(false)).unwrap();
    std::fs::write(src.join("f.bin"), [0u8, 1, 2, 3]).unwrap();
    std::fs::write(src.join("g.txt"), gs(false)).unwrap();
    git(&src, &["add", "."]);
    git(&src, &["commit", "-qm", "base"]);
    let a = format!("one\nTWO\nthree\n{}", lines.replace("line 39\n", "LINE 39\n"));
    std::fs::write(src.join("a.txt"), &a).unwrap();
    std::fs::write(src.join("b.txt"), "bee\n").unwrap();
    std::fs::remove_file(src.join("c.txt")).unwrap();
    git(&src, &["mv", "d.txt", "e.txt"]);
    std::fs::write(src.join("e.txt"), dees(true)).unwrap();
    std::fs::write(src.join("f.bin"), [0u8, 1, 2, 4]).unwrap();
    git(&src, &["add", "-A"]);
    git(&src, &["commit", "-qm", "Bug 1 - every kind of file"]);
    std::fs::write(src.join("a.txt"), a.replace("line 17\n", "LINE 17\n")).unwrap();
    std::fs::write(src.join("g.txt"), gs(true)).unwrap();
    git(&src, &["commit", "-qam", "Bug 2 - a.txt again, and g.txt"]);
    let db = tmp.path().join("db.jsonl");
    stacked(&src, &db, &["import", "w", "HEAD~2"]);
    (tmp, db, src)
}

/// The middle's lines of file `path`: each row of a line, added, removed
/// or kept, from its name to the next file's.
fn lines_of(app: &App, path: &str) -> Vec<String> {
    use stacked::rows::Kind;
    let at = app.rows.iter().position(|r| r.kind == Kind::File && r.path.as_deref() == Some(path)).unwrap_or_else(|| panic!("no {path}"));
    app.rows[at + 1..]
        .iter()
        .take_while(|r| r.kind != Kind::File)
        .filter(|r| matches!(r.kind, Kind::Add | Kind::Del | Kind::Ctx) && r.note.is_none())
        .map(|r| r.text.clone())
        .collect()
}

/// Presses 3, and once the files are read, draws the screen.
fn whole(app: &mut App) {
    keys(app, &[KeyCode::Char('3')]);
    app.wait_files();
    draw(app);
}

/// Up the patch until the cursor is on the row starting with `text`.
fn up_to(app: &mut App, text: &str) {
    draw(app);
    for _ in 0..app.rows.len() {
        if app.rows[app.cursor].text.starts_with(text) {
            return;
        }
        key(app, KeyCode::Up);
        draw(app);
    }
    panic!("no row {text:?} above");
}

/// The line of the screen the middle's cursor is drawn on: the line whose
/// cells right of the patches pane are in the cursor's colours.
fn cursor_line_y(app: &mut App) -> usize {
    let mut buf = Buffer::empty(Rect::new(0, 0, 120, 30));
    app.draw(&mut buf);
    let lit = |x: u16, y: u16| buf[(x, y)].modifier.contains(ratatui::style::Modifier::REVERSED);
    (1..29).find(|&y| (31..88).any(|x| lit(x, y))).unwrap_or_else(|| panic!("no cursor in the middle")) as usize
}

/// Checks that the middle's cursor is drawn on line `y` of the screen,
/// where it was; or, with too few rows over it to keep it there, that the
/// middle is drawn from its first row, the cursor as far down as they take
/// it. Then `y` is where it is.
fn kept_on_its_line(app: &mut App, y: &mut usize, what: &str) {
    let now = cursor_line_y(app);
    let from_top = match app.layout {
        Layout::Compact => app.compact.top[app.compact.side()] == 0,
        _ => app.top == 0,
    };
    assert!(now == *y || (now < *y && from_top), "{what}: on line {now} of the screen, not {y}:\n{}", draw(app));
    *y = now;
}

/// Whether the cell of `screen` where `text` starts, past its first
/// character, is drawn dim.
fn dim_at(app: &mut App, screen: &str, text: &str) -> bool {
    let (x, y) = at(screen, text);
    let mut buf = Buffer::empty(Rect::new(0, 0, 120, 30));
    app.draw(&mut buf);
    buf[(x + 1, y)].modifier.contains(ratatui::style::Modifier::DIM)
}

#[test]
fn three_shows_each_file_whole_no_hunk_headers_the_lines_removed_in_place() {
    use stacked::rows::{Kind, Row};
    let (_tmp, db, src) = setup_whole();
    let index = std::fs::read(src.join(".git/index")).unwrap();
    let mut app = open(&db).unwrap();
    draw(&mut app);
    keys(&mut app, &[KeyCode::Enter]);
    let hunks = app.rows.clone();
    assert!(hunks.iter().any(|r| r.kind == Kind::Hunk));
    // Its files are read away from the screen, as the status says, its
    // hunks shown meanwhile.
    key(&mut app, KeyCode::Char('3'));
    assert!(app.whole_files);
    assert!(app.status.starts_with("reading the files this patch changes, to show each whole"), "{}", app.status);
    app.wait_files();
    let screen = draw(&mut app);
    assert!(app.status.starts_with("whole files: each file this patch changes is shown as the patch leaves it"), "{}", app.status);
    assert!(screen.contains("· whole files") && !screen.contains("@@"), "{screen}");
    assert!(app.rows.iter().all(|r| r.kind != Kind::Hunk), "no hunk's header");
    // a.txt as the patch leaves it, each line removed where it was; the
    // lines between the hunks in none, with no key, drawn dim.
    let mut a: Vec<String> = [" one", "-two", "+TWO", " three"].map(String::from).to_vec();
    a.extend((1..=38).map(|n| format!(" line {n}")));
    a.extend(["-line 39", "+LINE 39", " line 40"].map(String::from));
    assert_eq!(lines_of(&app, "a.txt"), a);
    let outside: Vec<&Row> = app.rows.iter().filter(|r| r.outside && r.path.as_deref() == Some("a.txt")).collect();
    let between: Vec<String> = (3..=35).map(|n| format!(" line {n}")).collect();
    assert_eq!(outside.iter().map(|r| r.text.clone()).collect::<Vec<_>>(), between);
    assert!(outside.iter().all(|r| r.key.is_none() && r.hunk.is_none()));
    assert!(dim_at(&mut app, &screen, " line 10") && !dim_at(&mut app, &screen, "+TWO"), "{screen}");
    // A file added, all of it added; deleted, all of it removed; renamed,
    // whole with its change; not text, as its hunks say it.
    assert_eq!(lines_of(&app, "b.txt"), ["+bee"]);
    assert_eq!(lines_of(&app, "c.txt"), ["-see", "-sea"]);
    let mut e: Vec<String> = (1..=30).map(|n| format!(" dee {n}")).collect();
    e.splice(14..15, ["-dee 15".to_string(), "+DEE 15".to_string()]);
    assert_eq!(lines_of(&app, "e.txt"), e);
    let bin = |rows: &[Row]| -> Vec<Row> {
        let at = rows.iter().position(|r| r.path.as_deref() == Some("f.bin")).unwrap();
        rows[at..].iter().take_while(|r| r.kind != Kind::Blank).cloned().collect()
    };
    assert_eq!(bin(&app.rows), bin(&hunks));
    assert!(bin(&app.rows).iter().any(|r| r.text == "GIT binary patch"));
    // Every line of the hunks is the row it is in the hunks: its key too.
    let keyed = |rows: &[Row]| -> Vec<Row> { rows.iter().filter(|r| r.key.is_some()).cloned().collect() };
    assert_eq!(keyed(&app.rows), keyed(&hunks));
    // Patch 2: a.txt as it leaves it, patch 1 under it: TWO in no hunk.
    keys(&mut app, &[KeyCode::Left, KeyCode::Down, KeyCode::Right]);
    app.wait_files();
    draw(&mut app);
    assert!(app.rows.iter().any(|r| r.outside && r.text == " TWO"), "{:?}", lines_of(&app, "a.txt"));
    assert!(lines_of(&app, "a.txt").contains(&"+LINE 17".to_string()));
    // Nothing is written in the repository: its index as it was, and
    // nothing changed in the checkout.
    assert_eq!(std::fs::read(src.join(".git/index")).unwrap(), index);
    let status = Command::new("git").args(["status", "--porcelain"]).current_dir(&src).output().unwrap();
    assert!(status.stdout.is_empty(), "{}", String::from_utf8_lossy(&status.stdout));
    // 3 again: the hunks.
    keys(&mut app, &[KeyCode::Char('3')]);
    assert!(!app.whole_files && app.status.starts_with("hunks: each file this patch changes"), "{}", app.status);
    assert!(draw(&mut app).contains("@@"));
}

#[test]
fn comments_are_on_the_same_lines_whole_and_in_hunks_and_none_goes_on_a_line_in_no_hunk() {
    let (_tmp, db, _src) = setup_whole();
    let mut app = open(&db).unwrap();
    draw(&mut app);
    keys(&mut app, &[KeyCode::Enter]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "why so loud?");
    let hunks = app.rows.clone();
    whole(&mut app);
    // Under its line, whole too.
    let at = app.rows.iter().position(|r| r.text == "+TWO").unwrap();
    assert!(app.rows[at + 1].head && app.rows[at + 1].note.is_some(), "{:?}", app.rows[at + 1]);
    assert!(draw(&mut app).contains("why so loud?"));
    // A line in no hunk takes none: Enter opens the menu, which has no
    // comment to write there, the status saying why; so does Space; `?`
    // says it too.
    down_to(&mut app, " line 10");
    let why = "line 13 of a.txt is in no hunk of this patch: no comment goes on it";
    for k in [KeyCode::Enter, KeyCode::Char(' ')] {
        key(&mut app, k);
        assert!(matches!(app.mode, Mode::Menu(_)), "{k:?}");
        assert!(!items(&app).iter().any(|(l, _)| l.starts_with("Comment")), "{k:?}: {:?}", items(&app));
        assert!(items(&app).iter().any(|(l, _)| l.starts_with("Edit a.txt here")), "{k:?}: {:?}", items(&app));
        assert_eq!(app.status, why, "{k:?}");
        keys(&mut app, &[KeyCode::Esc]);
        assert_eq!(app.mode, Mode::Normal);
    }
    let help = app.help();
    assert!(help[0].starts_with("Line 13 of a.txt is in no hunk of this patch, so no comment goes on it"), "{help:?}");
    // A comment written whole, on a line of a hunk: on its key, as in the
    // hunks.
    down_to(&mut app, "+LINE 39");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "and this?");
    let line = hunks.iter().find(|r| r.text == "+LINE 39").and_then(|r| r.key.clone()).unwrap();
    assert!(notes_of(&db, "w", 0).iter().any(|n| n.line == line && n.text == "and this?"));
    // Both under their lines, in the hunks again.
    keys(&mut app, &[KeyCode::Char('3')]);
    draw(&mut app);
    for text in ["+TWO", "+LINE 39"] {
        let at = app.rows.iter().position(|r| r.text == text).unwrap();
        assert!(app.rows[at + 1].head, "{text}: {:?}", app.rows[at + 1]);
    }
}

#[test]
fn three_keeps_the_cursor_on_its_line_and_out_of_the_whole_files_goes_to_the_closest_line_in_a_hunk() {
    let (_tmp, db, _src) = setup_whole();
    let mut app = open(&db).unwrap();
    draw(&mut app);
    keys(&mut app, &[KeyCode::Enter]);
    // To each file whole: on the same line, on its line of the screen.
    down_to(&mut app, "+LINE 39");
    let mut y = cursor_line_y(&mut app);
    whole(&mut app);
    assert_eq!(app.rows[app.cursor].text, "+LINE 39");
    kept_on_its_line(&mut app, &mut y, "whole");
    // Back, from a line in no hunk: the closest line in a hunk, by their
    // numbers, a tie going to the line above; the status says so. Line 22
    // of a.txt is as far from line 5, the first hunk's last, as from line
    // 39, the second's first.
    up_to(&mut app, " line 19");
    let mut y = cursor_line_y(&mut app);
    keys(&mut app, &[KeyCode::Char('3')]);
    assert_eq!(app.rows[app.cursor].text, " line 2");
    assert_eq!(app.status, "line 22 of a.txt is in no hunk: the cursor is on the closest line in a hunk, above it");
    kept_on_its_line(&mut app, &mut y, "the hunks");
    whole(&mut app);
    assert_eq!(app.rows[app.cursor].text, " line 2", "a line of a hunk, kept");
    down_to(&mut app, " line 20");
    keys(&mut app, &[KeyCode::Char('3')]);
    assert_eq!(app.rows[app.cursor].text, " line 36");
    assert_eq!(app.status, "line 23 of a.txt is in no hunk: the cursor is on the closest line in a hunk, below it");
    // A hunk's header, to each file whole: its first line.
    up_to(&mut app, "@@ -39");
    whole(&mut app);
    assert_eq!(app.rows[app.cursor].text, " line 36");
    // 1, 2 and 3 in any order: the cursor on its line, on its line of the
    // screen.
    down_to(&mut app, "+LINE 39");
    let mut y = cursor_line_y(&mut app);
    keys(&mut app, &[KeyCode::Left, KeyCode::Left, KeyCode::Left]);
    assert!(app.focus == Focus::Diff && !app.list.shown(), "the patches hidden, for two sides to fit");
    kept_on_its_line(&mut app, &mut y, "the patches hidden");
    for k in ['1', '2', '3', '1', '3', '2', '1', '3', '2', '3'] {
        keys(&mut app, &[KeyCode::Char(k)]);
        app.wait_files();
        app.wait_blocks();
        draw(&mut app);
        assert_eq!(app.rows[app.cursor].text, "+LINE 39", "after {k}");
        kept_on_its_line(&mut app, &mut y, &format!("after {k}"));
    }
    assert_eq!((app.layout, app.reading, app.whole_files), (Layout::Unified, true, true));
}

#[test]
fn the_three_keys_combine_each_view_of_the_code_commenting_or_reading_the_hunks_or_each_file_whole() {
    let (_tmp, db, _src) = setup_whole();
    let mut app = open(&db).unwrap();
    draw(&mut app);
    keys(&mut app, &[KeyCode::Enter]);
    down_to(&mut app, "+LINE 39");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "why so loud?");
    let key = app.rows[app.cursor].key.clone().unwrap();
    let change = app.subject_change().unwrap();
    // The lines changed further down read already: reading, Space on its
    // line leaves the cursor there, nothing being left to read below.
    let below: Vec<String> = app.rows[app.cursor + 1..]
        .iter()
        .filter(|r| matches!(r.kind, stacked::rows::Kind::Add | stacked::rows::Kind::Del))
        .filter_map(|r| r.key.clone())
        .collect();
    assert_eq!(below.len(), 5, "+bee, -see, -sea, -dee 15, +DEE 15");
    Db::edit(&db, |d| d.mark_read(&change, below)).unwrap();
    app.refresh();
    // The patches hidden: two sides fit beside the comments.
    keys(&mut app, &[KeyCode::Left, KeyCode::Left, KeyCode::Left]);
    let mut y = cursor_line_y(&mut app);
    let mut seen = Vec::new();
    for layout in [Layout::Unified, Layout::Split, Layout::Compact] {
        for reading in [false, true] {
            for whole in [false, true] {
                app.wait_files();
                app.wait_blocks();
                let screen = draw(&mut app);
                let what = format!("{layout:?}, reading: {reading}, whole: {whole}");
                assert_eq!((app.layout, app.reading, app.whole_files), (layout, reading, whole), "{what}");
                // The cursor on its line, on its line of the screen, through
                // 1, 2 and 3.
                assert_eq!(app.rows[app.cursor].text, "+LINE 39", "{what}");
                kept_on_its_line(&mut app, &mut y, &what);
                // The hunks, or each file whole: no hunk's header, the lines in
                // no hunk drawn, dim.
                assert_eq!(screen.contains("@@ -39,5 +39,5 @@"), !whole, "{what}:\n{screen}");
                if whole {
                    assert!(dim_at(&mut app, &screen, " line 35"), "{what}:\n{screen}");
                }
                if layout != Layout::Unified {
                    let line = screen.lines().find(|l| l.contains("-line 39")).unwrap_or_else(|| panic!("{what}:\n{screen}"));
                    assert!(line.contains("+LINE 39"), "{what}: the old side beside the new: {line}");
                }
                // The comment under its line.
                assert!(screen.contains("why so loud?"), "{what}:\n{screen}");
                // Reading: Space marks the hunk read, on the keys the hunks
                // have, the margin then saying Enter takes the mark back;
                // and it does.
                if reading {
                    keys(&mut app, &[KeyCode::Char(' ')]);
                    assert_eq!(app.status, "read: this hunk, 2 lines · nothing left to read below", "{what}");
                    assert!(app.db.read_keys(&change).contains(&key), "{what}");
                    let screen = draw(&mut app);
                    assert!(app.rows[app.cursor].read && screen.contains("✓→○ +LINE 39"), "{what}:\n{screen}");
                    keys(&mut app, &[KeyCode::Enter]);
                    assert!(app.status.starts_with("unread: 2 lines"), "{what}: {}", app.status);
                    assert!(!app.db.read_keys(&change).contains(&key), "{what}");
                    kept_on_its_line(&mut app, &mut y, &format!("{what}, read and unread"));
                }
                seen.push(what);
                keys(&mut app, &[KeyCode::Char('3')]);
            }
            keys(&mut app, &[KeyCode::Char('2')]);
        }
        keys(&mut app, &[KeyCode::Char('1')]);
    }
    assert_eq!(seen.len(), 12, "{seen:?}");
}

#[test]
fn whole_a_line_in_no_hunk_is_folded_takes_no_read_mark_and_the_files_listed_go_to_each() {
    let (_tmp, db, _src) = setup_whole();
    let mut app = open(&db).unwrap();
    draw(&mut app);
    // Patch 2, its files listed under it, each whole: down onto g.txt, its
    // name on the middle's first line, under the patch's title.
    keys(&mut app, &[KeyCode::Down, KeyCode::Left]);
    whole(&mut app);
    keys(&mut app, &[KeyCode::Down, KeyCode::Down]);
    assert_eq!(app.rows[app.cursor].text, "g.txt");
    let screen = draw(&mut app);
    let lines: Vec<&str> = screen.lines().collect();
    assert!(right_of_list(lines[1]).starts_with("   Bug 2 - a.txt again, and g.txt"), "{screen}");
    assert!(right_of_list(lines[2]).starts_with("g.txt"), "{screen}");
    // Its line 10 is in no hunk, and longer than the middle: folded, as a
    // line of a hunk is. On its second piece, two sides: the same piece.
    keys(&mut app, &[KeyCode::Enter]);
    down_to(&mut app, " g ten");
    keys(&mut app, &[KeyCode::Down]);
    let piece = app.rows[app.cursor].clone();
    assert!(piece.cont && piece.outside && piece.new_no == Some(10), "{piece:?}");
    keys(&mut app, &[KeyCode::Left, KeyCode::Left, KeyCode::Char('1')]);
    assert_eq!(app.layout, Layout::Split);
    let r = &app.rows[app.cursor];
    assert!(r.cont && r.outside && r.new_no == Some(10) && !app.rows[app.cursor - 1].cont, "{r:?}");
    keys(&mut app, &[KeyCode::Char('1'), KeyCode::Char('1')]);
    // Reading: no read mark goes on it, and Space goes on to the next line
    // not read, none here; Enter takes none back.
    keys(&mut app, &[KeyCode::Char('2')]);
    app.wait_blocks();
    draw(&mut app);
    assert!(app.rows[app.cursor].outside);
    let help = app.help();
    assert!(help[0].starts_with("Line 10 of g.txt is in no hunk of this patch, so no read mark goes on it."), "{help:?}");
    keys(&mut app, &[KeyCode::Char(' ')]);
    assert_eq!(app.status, "line 10 of g.txt is in no hunk of this patch, so no read mark goes on it, and nothing is left to read below");
    keys(&mut app, &[KeyCode::Enter]);
    assert_eq!(app.status, "line 10 of g.txt is in no hunk of this patch: no read mark is on it");
    assert!(app.hint().contains("3: hunks"), "{}", app.hint());
}

#[test]
fn three_on_the_stack_s_own_view_says_why_and_is_no_key_on_other_screens() {
    let (_tmp, db, _src) = setup_whole();
    let mut app = open(&db).unwrap();
    draw(&mut app);
    keys(&mut app, &[KeyCode::Up]);
    assert!(app.on_stack);
    let rows = app.rows.clone();
    keys(&mut app, &[KeyCode::Char('3')]);
    assert!(app.whole_files);
    assert_eq!(app.status, "whole files: each patch shows the files it changes whole; the stack itself has no files");
    assert_eq!(app.rows, rows, "the stack as it was");
    // On a patch, each file whole.
    keys(&mut app, &[KeyCode::Down]);
    app.wait_files();
    draw(&mut app);
    assert!(app.rows.iter().any(|r| r.outside));
    // Elsewhere, 3 is no key.
    keys(&mut app, &[KeyCode::Up]);
    choose(&mut app, "History…");
    assert_eq!(app.screen, Screen::History);
    let screen = draw(&mut app);
    keys(&mut app, &[KeyCode::Char('3')]);
    assert!(app.whole_files && app.screen == Screen::History);
    assert_eq!(draw(&mut app), screen);
}

#[test]
fn the_key_for_each_file_whole_is_a_setting() {
    let (_tmp, db, _src) = setup_whole();
    let (cfg, wrong) = stacked::config::Config::parse("[keys]\nwhole = \"w\"\n").unwrap();
    assert!(wrong.is_empty(), "{wrong:?}");
    stacked::config::init(stacked::config::Config::default());
    let mut app = Editor(App::with_config(&db, cfg).unwrap());
    draw(&mut app);
    keys(&mut app, &[KeyCode::Enter]);
    assert!(draw(&mut app).contains("w: whole"));
    keys(&mut app, &[KeyCode::Char('3')]);
    assert!(!app.whole_files, "3 is no key now");
    keys(&mut app, &[KeyCode::Char('w')]);
    assert!(app.whole_files);
    app.wait_files();
    draw(&mut app);
    assert!(app.say_keys(app.hint()).contains("w: hunks"), "{}", app.hint());
    assert!(app.help().iter().any(|l| l.ends_with("; w to see each file as its hunks again.")), "{:?}", app.help());
}

/// The patches pane's lines under its header, as drawn on a screen, each
/// cut where the pane ends.
fn pane_lines(screen: &str) -> Vec<String> {
    screen.lines().skip(2).map(|l| l.split('│').next().unwrap().trim_end().to_string()).take_while(|l| !l.is_empty()).collect()
}

/// The words of each of those lines.
fn pane_words(screen: &str) -> Vec<String> {
    pane_lines(screen).iter().map(|l| l.split_whitespace().collect::<Vec<_>>().join(" ")).collect()
}

/// How the cell where `text` starts in the patches pane (its first 40
/// columns) looks, the screen drawn.
fn pane_cell(app: &mut App, text: &str) -> ratatui::buffer::Cell {
    let mut buf = Buffer::empty(Rect::new(0, 0, 120, 30));
    app.draw(&mut buf);
    for y in 0..30u16 {
        let line: String = (0..40u16).map(|x| buf[(x, y)].symbol()).collect();
        if let Some(k) = line.find(text) {
            return buf[(line[..k].chars().count() as u16, y)].clone();
        }
    }
    panic!("no {text:?} in the patches pane");
}

#[test]
fn left_on_the_patches_lists_the_files_of_the_patch_picked_then_hides_them() {
    let (_tmp, db, _src) = setup_files();
    let mut app = open(&db).unwrap();
    let screen = draw(&mut app);
    assert!(app.list == PatchesPane::Expanded, "expanded at the start");
    assert_eq!(pane_words(&screen), ["1 Bug 1 - two files", "2 Bug 2 - three files", "3 Bug 3 - one file"], "{screen}");
    // Listing files: each patch as expanded, and under the one picked its
    // files, in the middle's order, each with the lines the patch adds and
    // removes in it. The pane is as wide as the paths need, a third of the
    // screen at most: a path longer than that keeps its end, without the
    // lines.
    keys(&mut app, &[KeyCode::Left]);
    assert!(app.list == PatchesPane::Files && app.focus == Focus::List && app.file_on().is_none());
    let screen = draw(&mut app);
    let lines = pane_lines(&screen);
    assert_eq!(
        lines[..4],
        ["  1 Bug 1 - two files", "      a.txt                        +4 −4", "      …/urlbar/src/tui/banner.rs", "  2 Bug 2 - three files"],
        "{screen}"
    );
    assert_eq!(screen.lines().nth(2).unwrap().chars().nth(40), Some('│'), "40 columns:\n{screen}");
    assert_eq!(lines.len(), 5, "{lines:?}");
    // The stack's header has none: no patch is picked.
    keys(&mut app, &[KeyCode::Up]);
    assert!(app.on_stack);
    assert_eq!(pane_words(&draw(&mut app)), ["1 Bug 1 - two files", "2 Bug 2 - three files", "3 Bug 3 - one file"]);
    // Hidden: the middle has the width.
    keys(&mut app, &[KeyCode::Down, KeyCode::Left]);
    assert!(app.list == PatchesPane::Hidden && app.focus == Focus::Diff);
    let screen = draw(&mut app);
    assert!(!screen.contains("Bug 2 - three files"), "the patches are gone:\n{screen}");
    assert!(screen.lines().nth(1).unwrap().starts_with("✉ message"), "the patch has the width:\n{screen}");
    // Expanded again, the cursor in it.
    keys(&mut app, &[KeyCode::Left]);
    assert!(app.list == PatchesPane::Expanded && app.focus == Focus::List);
    assert_eq!(pane_words(&draw(&mut app)).len(), 3, "no files");
}

#[test]
fn down_onto_a_file_shows_it_in_the_middle_its_name_at_the_top() {
    let (_tmp, db, _src) = setup_files();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Left, KeyCode::Down]);
    // On a.txt: the middle's cursor on its name, the cursor in the patches.
    assert_eq!((app.focus, app.sel, app.file_on()), (Focus::List, 0, Some(0)));
    assert_eq!(app.rows[app.cursor].text, "a.txt");
    assert!(pane_cell(&mut app, "a.txt").modifier.contains(ratatui::style::Modifier::REVERSED), "the cursor on it");
    assert!(!pane_cell(&mut app, "1 Bug 1").modifier.contains(ratatui::style::Modifier::REVERSED), "not on its patch");
    // On the next, further down than the middle shows: its name on the
    // middle's first line, under the patch's title, pinned.
    keys(&mut app, &[KeyCode::Down]);
    assert_eq!(app.file_on(), Some(1));
    assert_eq!(app.rows[app.cursor].text, DEEP);
    let screen = draw(&mut app);
    let lines: Vec<&str> = screen.lines().collect();
    assert!(right_of_list(lines[1]).starts_with("   Bug 1 - two files"), "{screen}");
    assert!(right_of_list(lines[2]).starts_with(DEEP), "{screen}");
    assert!(app.hint().starts_with("enter: read it"), "{}", app.hint());
    let help = app.help();
    assert_eq!(help[0], "Press Enter to read this file.");
    assert!(help.iter().any(|l| l.contains("↑ above the first for the patch itself, ↓ under the last for the next patch")), "{help:?}");
}

#[test]
fn down_past_the_last_file_picks_the_next_patch_its_files_listed() {
    let (_tmp, db, _src) = setup_files();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Left, KeyCode::Down, KeyCode::Down, KeyCode::Down]);
    // Patch 2, from its top, as any patch picked; its files under it, and
    // patch 1's gone.
    assert_eq!((app.sel, app.file_on(), app.cursor), (1, None, 0));
    let screen = draw(&mut app);
    assert_eq!(
        pane_words(&screen),
        ["1 Bug 1 - two files", "2 Bug 2 - three files", "b.txt +1 −1", "c.txt +1 −1", "d.txt +1 −1", "3 Bug 3 - one file"],
        "{screen}"
    );
    assert!(screen.contains("│   Bug 2 - three files"), "{screen}");
    keys(&mut app, &[KeyCode::Down, KeyCode::Down]);
    assert_eq!(app.rows[app.cursor].text, "c.txt");
    // Past the last patch's last file, nowhere.
    keys(&mut app, &[KeyCode::Down, KeyCode::Down, KeyCode::Down]);
    assert_eq!((app.sel, app.file_on()), (2, Some(0)));
    keys(&mut app, &[KeyCode::Down]);
    assert_eq!((app.sel, app.file_on()), (2, Some(0)));
    // Home and End: the header, and the last patch itself; PageDown and
    // PageUp, a page down or up the list as shown.
    keys(&mut app, &[KeyCode::Home]);
    assert!(app.on_stack && app.file_on().is_none());
    keys(&mut app, &[KeyCode::End]);
    assert!(!app.on_stack && app.sel == 2 && app.file_on().is_none());
    keys(&mut app, &[KeyCode::PageUp]);
    assert!(!app.on_stack && app.sel == 0 && app.file_on().is_none());
    keys(&mut app, &[KeyCode::PageUp]);
    assert!(app.on_stack);
    keys(&mut app, &[KeyCode::PageDown]);
    assert!(!app.on_stack && app.sel == 0);
    keys(&mut app, &[KeyCode::PageDown]);
    assert!(app.sel == 2 && app.file_on().is_none(), "past patch 1's files and patch 2, the last");
}

#[test]
fn up_past_the_first_file_reaches_the_patch_then_the_one_above() {
    let (_tmp, db, _src) = setup_files();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Left, KeyCode::End, KeyCode::Up, KeyCode::Down]);
    assert_eq!((app.sel, app.file_on()), (1, Some(0)));
    assert_eq!(app.rows[app.cursor].text, "b.txt");
    // The patch itself, the middle on it from its start.
    keys(&mut app, &[KeyCode::Up]);
    assert_eq!((app.sel, app.file_on(), app.cursor, app.top), (1, None, 0, 0));
    assert!(pane_cell(&mut app, "2 Bug 2").modifier.contains(ratatui::style::Modifier::REVERSED));
    // Then the one above, picked as any: its own files listed in place.
    keys(&mut app, &[KeyCode::Up]);
    assert_eq!((app.sel, app.file_on()), (0, None));
    let words = pane_words(&draw(&mut app));
    assert_eq!(words[..3], ["1 Bug 1 - two files", "a.txt +4 −4", "…/urlbar/src/tui/banner.rs"], "{words:?}");
    assert!(!words.iter().any(|w| w.starts_with("b.txt")), "{words:?}");
    // Esc from a file goes up to its patch too; from the patch, to the
    // header.
    keys(&mut app, &[KeyCode::Down, KeyCode::Down, KeyCode::Esc]);
    assert_eq!((app.sel, app.file_on(), app.on_stack), (0, None, false));
    keys(&mut app, &[KeyCode::Esc]);
    assert!(app.on_stack);
}

#[test]
fn enter_or_right_on_a_file_goes_into_the_middle_there() {
    let (_tmp, db, _src) = setup_files();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Left, KeyCode::Down, KeyCode::Down, KeyCode::Enter]);
    assert_eq!(app.focus, Focus::Diff);
    assert_eq!(app.rows[app.cursor].text, DEEP);
    down_to(&mut app, "+LINE 5");
    // Back, on that file; up to a.txt, and → there.
    keys(&mut app, &[KeyCode::Left]);
    assert_eq!((app.focus, app.file_on()), (Focus::List, Some(1)));
    keys(&mut app, &[KeyCode::Up, KeyCode::Right]);
    assert_eq!(app.focus, Focus::Diff);
    assert_eq!(app.rows[app.cursor].text, "a.txt");
}

#[test]
fn a_click_on_a_file_goes_there_and_a_second_into_the_middle() {
    use ratatui::style::Modifier;
    let (_tmp, db, _src) = setup_files();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Left, KeyCode::Right]);
    let (x, y) = at(&draw(&mut app), "…/urlbar");
    // The mouse over it: underlined, and not picked.
    app.hover(x, y);
    assert_eq!(app.hovered, Some(Spot::File(1)));
    assert_eq!(app.focus, Focus::Diff);
    assert!(pane_cell(&mut app, "…/urlbar").modifier.contains(Modifier::UNDERLINED));
    // A click: the cursor on it in the patches, the middle on the file.
    app.click(x, y, false);
    assert_eq!((app.focus, app.file_on()), (Focus::List, Some(1)));
    draw(&mut app);
    assert_eq!(app.rows[app.cursor].text, DEEP);
    // Another, as Enter: into the middle there.
    app.click(x, y, false);
    assert_eq!(app.focus, Focus::Diff);
    assert_eq!(app.rows[app.cursor].text, DEEP);
    // A click on its patch: the patch itself, from its start.
    let (x, y) = at(&draw(&mut app), "1 Bug 1");
    keys(&mut app, &[KeyCode::Left]);
    app.click(x, y, false);
    assert_eq!((app.focus, app.sel, app.file_on(), app.cursor), (Focus::List, 0, None, 0));
}

#[test]
fn the_file_the_middle_s_cursor_is_in_is_picked_and_left_comes_back_to_it() {
    use ratatui::style::Color;
    let (_tmp, db, _src) = setup_files();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Left, KeyCode::Right]);
    let picked = |app: &mut App, text: &str| pane_cell(app, text).bg == Color::Indexed(238);
    // In the message: the patch is the pick.
    assert!(picked(&mut app, "1 Bug 1") && !picked(&mut app, "a.txt"));
    // In a.txt, then in the file after it: that file.
    down_to(&mut app, "+LINE 12");
    assert!(picked(&mut app, "a.txt") && !picked(&mut app, "1 Bug 1") && !picked(&mut app, "…/urlbar"));
    down_to(&mut app, "+LINE 5");
    assert!(picked(&mut app, "…/urlbar") && !picked(&mut app, "a.txt"));
    // ← comes back to it, the middle where it was; → goes back there.
    keys(&mut app, &[KeyCode::Left]);
    assert_eq!((app.focus, app.file_on()), (Focus::List, Some(1)));
    assert!(app.rows[app.cursor].text.starts_with("+LINE 5"));
    assert!(pane_cell(&mut app, "…/urlbar").modifier.contains(ratatui::style::Modifier::REVERSED));
    keys(&mut app, &[KeyCode::Right]);
    assert!(app.focus == Focus::Diff && app.rows[app.cursor].text.starts_with("+LINE 5"));
    // Back in the message, ← comes back to the patch itself.
    keys(&mut app, &[KeyCode::Home, KeyCode::Left]);
    assert_eq!((app.focus, app.sel, app.file_on()), (Focus::List, 0, None));
}

#[test]
fn listing_files_the_comments_pane_and_the_compact_view_are_as_they_were() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Left, KeyCode::Right]);
    assert_eq!(app.list, PatchesPane::Files);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "why so loud?");
    // → on the comments: compact, a line each, then hidden.
    keys(&mut app, &[KeyCode::Right, KeyCode::Right]);
    assert!(app.comments == Pane::Compact && app.focus == Focus::Notes);
    assert!(draw(&mut app).contains("you · a.txt: TWO"));
    keys(&mut app, &[KeyCode::Right]);
    assert!(!app.notes_shown() && app.focus == Focus::Diff);
    // 1: side by side, then the compact view, each side a pane.
    keys(&mut app, &[KeyCode::Char('1'), KeyCode::Char('1')]);
    assert_eq!(app.layout, Layout::Compact);
    assert!(draw(&mut app).contains("side-by-side, compact <>"));
    assert_eq!(app.list, PatchesPane::Files, "the patches as they were");
}

#[test]
fn side_by_side_too_narrow_says_what_gives_the_middle_room() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    // A comment beside patch 1: on 120 columns, the patches and the
    // comments, expanded, leave the middle 58, short of two sides.
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "why so loud?");
    // What 1 says fits on the status line, as long as the screen is wide.
    let one = |app: &mut App| {
        keys(app, &[KeyCode::Char('1')]);
        assert!(app.status.chars().count() < 120, "{}", app.status);
        app.status.clone()
    };
    let said = one(&mut app);
    assert_eq!(said, "side-by-side needs 60 columns: press ← twice on the patches to hide them, or → on the comments to make them compact");
    assert_eq!(app.layout, Layout::Unified);
    // Listing the files gives no room: ← once more hides the patches.
    keys(&mut app, &[KeyCode::Left, KeyCode::Left]);
    assert_eq!(app.list, PatchesPane::Files);
    let said = one(&mut app);
    assert_eq!(said, "side-by-side needs 60 columns: press ← on the patches to hide them, or → on the comments to make them compact");
    // The comments compact: the middle has the room.
    keys(&mut app, &[KeyCode::Right, KeyCode::Right, KeyCode::Right]);
    assert_eq!(app.comments, Pane::Compact);
    let said = one(&mut app);
    assert_eq!((app.layout, said.as_str()), (Layout::Split, "side-by-side view"));
    // Both out of sight on a screen narrower than two sides: nothing gives
    // more room.
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Left, KeyCode::Left, KeyCode::Right, KeyCode::Right, KeyCode::Right]);
    assert!(app.list == PatchesPane::Hidden && !app.notes_shown());
    app.draw(&mut Buffer::empty(Rect::new(0, 0, 50, 30)));
    key(&mut app, KeyCode::Char('1'));
    assert_eq!(app.status, "side-by-side needs 60 columns, more than the screen has");
}

#[test]
fn reading_each_file_listed_says_the_lines_left_to_mark_read() {
    use ratatui::style::{Color, Modifier};
    let (_tmp, db, _src) = setup_files();
    let mut app = open(&db).unwrap();
    // Reading, listing files: a.txt's 8 changed lines, in four hunks, and
    // the deep file's 2, all left to mark read, in place of the lines the
    // patch adds and removes; the deep file's path is cut to leave room for
    // its count.
    keys(&mut app, &[KeyCode::Char('2'), KeyCode::Left]);
    let screen = draw(&mut app);
    assert_eq!(pane_lines(&screen)[1..3], ["      a.txt                       8 left", "      …/urlbar/src/tui/banner.rs  2 left"], "{screen}");
    // A hunk of a.txt read: its count goes down, the other file's does not.
    keys(&mut app, &[KeyCode::Enter]);
    app.wait_blocks();
    down_to(&mut app, "-line 2");
    keys(&mut app, &[KeyCode::Char(' ')]);
    assert!(app.status.contains("read: this hunk, 2 lines"), "{}", app.status);
    let words = pane_words(&draw(&mut app));
    assert_eq!(words[..3], ["1 Bug 1 - two files 2/12", "a.txt 6 left", "…/urlbar/src/tui/banner.rs 2 left"], "{words:?}");
    // All of a.txt read: a tick, the middle's cursor gone on to the deep
    // file.
    keys(&mut app, &[KeyCode::Char(' '), KeyCode::Char(' '), KeyCode::Char(' ')]);
    let words = pane_words(&draw(&mut app));
    assert_eq!(words[1..3], ["a.txt ✓", "…/urlbar/src/tui/banner.rs 2 left"], "{words:?}");
    assert_eq!(app.rows[app.cursor].text, "-line 5");
    // The tag looks as the rest of its line: picked, as the file the
    // middle's cursor is in, then under the cursor, back in the patches.
    assert_eq!(pane_cell(&mut app, "2 left").bg, Color::Indexed(238));
    assert_eq!(pane_cell(&mut app, "✓").bg, Color::Reset);
    keys(&mut app, &[KeyCode::Left]);
    assert_eq!(app.file_on(), Some(1));
    assert!(pane_cell(&mut app, "2 left").modifier.contains(Modifier::REVERSED));
    let said = "Each file listed under the patch says how many of its changed lines are left to mark read, as 7 left, or ✓ once all of them are read.";
    assert!(app.help().iter().any(|l| l == said), "{:?}", app.help());
    // Enter takes a mark back: a.txt has lines left again.
    keys(&mut app, &[KeyCode::Right, KeyCode::Home]);
    down_to(&mut app, "+LINE 32");
    keys(&mut app, &[KeyCode::Enter]);
    assert!(app.status.contains("unread: 2 lines"), "{}", app.status);
    assert_eq!(pane_words(&draw(&mut app))[1], "a.txt 2 left");
    // Commenting, the lines the patch adds and removes, as before.
    keys(&mut app, &[KeyCode::Char('2'), KeyCode::Left]);
    let words = pane_words(&draw(&mut app));
    assert_eq!(words[..3], ["1 Bug 1 - two files", "a.txt +4 −4", "…/urlbar/src/tui/banner.rs"], "{words:?}");
    assert!(!app.help().iter().any(|l| l == said), "{:?}", app.help());
}

#[test]
fn reading_a_file_s_lines_of_white_space_alone_count_only_when_the_settings_say_so() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir(&src).unwrap();
    git(&src, &["init", "-q", "-b", "main"]);
    git(&src, &["config", "commit.gpgsign", "false"]);
    std::fs::write(src.join("f.txt"), "one\n").unwrap();
    std::fs::write(src.join("g.txt"), "one\n").unwrap();
    git(&src, &["add", "."]);
    git(&src, &["commit", "-qm", "base"]);
    // f.txt gets two lines of text, an empty line and a blank one between
    // them; g.txt the empty line and the blank one alone.
    std::fs::write(src.join("f.txt"), "one\ntwo\n\n  \nthree\n").unwrap();
    std::fs::write(src.join("g.txt"), "one\n\n  \n").unwrap();
    git(&src, &["commit", "-qam", "Bug 1 - more"]);
    let db = tmp.path().join("db.jsonl");
    stacked(&src, &db, &["import", "s", "HEAD~1"]);
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Char('2'), KeyCode::Left]);
    let words = pane_words(&draw(&mut app));
    assert_eq!(words, ["1 Bug 1 - more 0/3", "f.txt 2 left", "g.txt ✓"], "nothing of g.txt needs reading");
    // With whitespace_needs_review, as the settings may say, the empty and
    // blank lines count as any other.
    let mut cfg = stacked::config::Config::default();
    cfg.view.whitespace_needs_review = true;
    let mut all = App::with_config(&db, cfg).unwrap();
    keys(&mut all, &[KeyCode::Char('2'), KeyCode::Left]);
    let words = pane_words(&draw(&mut all));
    assert_eq!(words, ["1 Bug 1 - more 0/7", "f.txt 4 left", "g.txt 2 left"]);
}

#[test]
fn reading_a_file_with_no_newline_at_its_end_ticks_it_and_its_patch_once_read() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir(&src).unwrap();
    git(&src, &["init", "-q", "-b", "main"]);
    git(&src, &["config", "commit.gpgsign", "false"]);
    // The last line of f.txt has no newline, before the patch and after it:
    // under each side's, git writes `\ No newline at end of file`.
    std::fs::write(src.join("f.txt"), "one\ntwo").unwrap();
    git(&src, &["add", "."]);
    git(&src, &["commit", "-qm", "base"]);
    std::fs::write(src.join("f.txt"), "one\nTWO").unwrap();
    git(&src, &["commit", "-qam", "Bug 1 - shout"]);
    let db = tmp.path().join("db.jsonl");
    stacked(&src, &db, &["import", "s", "HEAD~1"]);
    let mut app = open(&db).unwrap();
    // Reading, listing files: the line removed, the line added and the
    // title are the lines to read.
    keys(&mut app, &[KeyCode::Char('2'), KeyCode::Left]);
    let screen = draw(&mut app);
    assert_eq!(pane_words(&screen), ["1 Bug 1 - shout 0/3", "f.txt 2 left"], "{screen}");
    assert!(screen.contains("3/3 lines to review"), "{screen}");
    keys(&mut app, &[KeyCode::Enter]);
    app.wait_blocks();
    assert_eq!(app.rows.iter().filter(|r| r.text == "\\ No newline at end of file").count(), 2);
    // The title read, then the hunk.
    down_to(&mut app, "   Bug 1 - shout");
    keys(&mut app, &[KeyCode::Char(' ')]);
    assert!(app.status.contains("read: the title, 1 line"), "{}", app.status);
    assert_eq!(app.rows[app.cursor].text, "-two");
    keys(&mut app, &[KeyCode::Char(' ')]);
    assert!(app.status.contains("read: this hunk, 2 lines · nothing left to read below"), "{}", app.status);
    // All of it read: the file's tick and the patch's, and nothing left to
    // review on the top bar.
    let screen = draw(&mut app);
    assert_eq!(pane_words(&screen), ["1 Bug 1 - shout ✓", "f.txt ✓"], "{screen}");
    assert!(screen.contains("reading 3/3") && screen.contains("nothing left to review"), "{screen}");
    assert!(app.rows.iter().any(|r| r.text == "f.txt   read 2/2 lines"));
}

#[test]
fn reading_the_patches_pane_keeps_its_width_from_patch_to_patch() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir_all(src.join("docs/notes")).unwrap();
    git(&src, &["init", "-q", "-b", "main"]);
    git(&src, &["config", "commit.gpgsign", "false"]);
    let lines: String = (1..=20).map(|n| format!("line {n}\n")).collect();
    std::fs::write(src.join("docs/notes/today.txt"), &lines).unwrap();
    std::fs::write(src.join("b.txt"), &lines).unwrap();
    git(&src, &["add", "."]);
    git(&src, &["commit", "-qm", "base"]);
    // Patch 1 changes six lines of docs/notes/today.txt, patch 2 one line
    // of b.txt.
    std::fs::write(src.join("docs/notes/today.txt"), lines.replacen("line", "LINE", 6)).unwrap();
    git(&src, &["commit", "-qam", "Bug 1 - notes"]);
    std::fs::write(src.join("b.txt"), lines.replacen("line", "LINE", 1)).unwrap();
    git(&src, &["commit", "-qam", "Bug 2 - b"]);
    let db = tmp.path().join("db.jsonl");
    stacked(&src, &db, &["import", "w", "HEAD~2"]);
    let mut app = open(&db).unwrap();
    // How wide the patches pane is: where the line right of it is drawn.
    let width = |app: &mut App| draw(app).lines().nth(2).unwrap().chars().position(|c| c == '│');
    // Listing files: as wide as today.txt's line needs, with the lines its
    // patch adds and removes.
    keys(&mut app, &[KeyCode::Left]);
    assert_eq!(width(&mut app), Some(33));
    // Reading, as wide as its 12 lines left need, on either patch.
    keys(&mut app, &[KeyCode::Char('2')]);
    let screen = draw(&mut app);
    assert_eq!(pane_lines(&screen)[1], "      docs/notes/today.txt  12 left", "{screen}");
    assert_eq!(width(&mut app), Some(35));
    keys(&mut app, &[KeyCode::Down, KeyCode::Down]);
    assert_eq!((app.sel, app.file_on()), (1, None));
    assert_eq!(pane_words(&draw(&mut app))[2], "b.txt 2 left");
    assert_eq!(width(&mut app), Some(35));
    // Read whole, today.txt shows its tick, which takes less room, and the
    // pane stays as wide.
    keys(&mut app, &[KeyCode::Up, KeyCode::Enter]);
    app.wait_blocks();
    down_to(&mut app, "-line 1");
    keys(&mut app, &[KeyCode::Char(' ')]);
    assert_eq!(pane_words(&draw(&mut app))[1], "docs/notes/today.txt ✓");
    assert_eq!(width(&mut app), Some(35));
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
        ["Save it", "Save it and ask agent", "Send it", "Attach a reference…", "", "Throw it away", "Keep writing"]
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
fn a_line_after_no_newline_at_end_of_file_is_opened_and_gone_to_at_its_number() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir(&src).unwrap();
    git(&src, &["init", "-q", "-b", "main"]);
    git(&src, &["config", "commit.gpgsign", "false"]);
    // f.txt's last line has no newline before the patch, which gives it one
    // and a line more: git writes `\ No newline at end of file` under the
    // line removed, then the lines added.
    std::fs::write(src.join("f.txt"), "one\ntwo").unwrap();
    git(&src, &["add", "."]);
    git(&src, &["commit", "-qm", "base"]);
    std::fs::write(src.join("f.txt"), "one\ntwo\nthree\n").unwrap();
    git(&src, &["commit", "-qam", "Bug 1 - three"]);
    let db = tmp.path().join("db.jsonl");
    stacked(&src, &db, &["import", "s", "HEAD~1"]);
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    // Each line added is opened in $EDITOR at its own line of the file.
    down_to(&mut app, "+two");
    assert_eq!(choose(&mut app, "Edit f.txt here in $EDITOR"), Effect::EditCode { file: "f.txt".into(), line: 2, again: None });
    down_to(&mut app, "+three");
    assert_eq!(choose(&mut app, "Edit f.txt here in $EDITOR"), Effect::EditCode { file: "f.txt".into(), line: 3, again: None });
    // The agent's reference to line 3, previewed, then gone to: Enter goes
    // to the line added there.
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "why three?");
    let n = notes_of(&db, "s", 0).remove(0);
    Db::edit(&db, |d| {
        let n = d.note(&n.change, &n.id).cloned().unwrap();
        let said = "The file ends there.\n\n```refs\nf.txt:3 @1\n```";
        d.set_note(Note {
            text: append_turn(&n.text, Role::Agent, said, Some("answer")),
            ..n
        })
    })
    .unwrap();
    app.refresh();
    draw(&mut app);
    down_to(&mut app, "     ▎   ⇢ f.txt:3 ");
    keys(&mut app, &[KeyCode::Enter]);
    let Mode::Preview(p) = &app.mode else { panic!("{:?}", app.mode) };
    assert_eq!(p.texts(), ["    3 │ three"]);
    keys(&mut app, &[KeyCode::Enter]);
    assert!(matches!(app.mode, Mode::Normal), "{:?}", app.mode);
    assert!(app.rows[app.cursor].text.starts_with("+three"), "{}", app.rows[app.cursor].text);
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
    assert!(t.texts().iter().any(|l| l.contains("why so loud?")), "{:?}", t.texts());
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

/// The editor on `why so loud?`, a comment on TWO, and `Same as @r1?`, a
/// comment on LINE 39 whose r1 points at the first; the agent's answer to
/// the second, attaching r2, lines of a.txt; your reply after it. And the
/// second comment.
fn a_thread_with_references(db: &Path) -> (Editor, Note) {
    let mut app = open(db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "why so loud?");
    down_to(&mut app, "+LINE 39");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "Same as @r1?");
    let a = notes_of(db, "s", 0).into_iter().find(|n| n.text == "why so loud?").unwrap();
    let b = notes_of(db, "s", 0).into_iter().find(|n| n.text == "Same as @r1?").unwrap();
    Db::edit(db, |d| {
        let said = "As @r2 shows.\n\n```refs\nr2 a.txt:2 @1\n```";
        d.set_note(Note {
            text: append_turn(&append_turn(&b.text, Role::Agent, said, Some("answer")), Role::You, "Which one?", None),
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
    (app, b)
}

/// The agent's stream on the run `changing` starts: `said`, a line each.
fn agent_writes(db: &Path, said: &[&str]) {
    let events: Vec<stacked::db::Record> = said
        .iter()
        .map(|text| stacked::db::Record::Event {
            run: "run-changing".into(),
            event: serde_json::json!({"type": "assistant", "message": {"content": [{"type": "text", "text": text}]}}),
        })
        .collect();
    Db::append(db, &events).unwrap();
}

#[test]
fn the_agent_s_stream_goes_between_the_last_reply_and_the_references() {
    let (_tmp, db, _src) = setup();
    let (mut app, b) = a_thread_with_references(&db);
    // The agent at work on it: what it writes under the last reply, over
    // the references.
    changing(&db, &b.change, &b.id, serde_json::json!({"patch": b.change}));
    agent_writes(&db, &["Reading the patch."]);
    app.refresh();
    draw(&mut app);
    keys(&mut app, &[KeyCode::End]);
    let screen = draw(&mut app);
    let y = |text: &str| at(&screen, text).1;
    let (reply, stream, r1, r2) = (y("▎   Which one?"), y("⋯ Reading the patch."), y("▎   r1 ↗ patch 1 · a.txt: TWO"), y("▎   r2 ⇢ a.txt:2"));
    assert!(reply < stream && stream < r1 && r1 < r2, "{screen}");
    // Enter on the stream opens it in place, as anywhere on the comment.
    keys(&mut app, &[KeyCode::Home]);
    down_to(&mut app, "     ⋯ Reading the patch.");
    keys(&mut app, &[KeyCode::Enter]);
    assert!(matches!(&app.mode, Mode::Stream { id, .. } if *id == b.id), "{:?}", app.mode);
    keys(&mut app, &[KeyCode::Esc]);
    assert_eq!(app.mode, Mode::Normal);
    // Enter on a reference under it previews its lines.
    down_to(&mut app, "     ▎   r2 ⇢ a.txt:2");
    keys(&mut app, &[KeyCode::Enter]);
    let Mode::Preview(p) = &app.mode else { panic!("{:?}", app.mode) };
    assert_eq!(p.title, "a.txt:2 · as patch 1 leaves it");
    keys(&mut app, &[KeyCode::Esc]);
    // The stream growing over it, the cursor stays on it, and Enter still
    // previews it.
    agent_writes(&db, &["Looking at a.txt.", "Seen."]);
    app.refresh();
    let screen = draw(&mut app);
    assert!(app.rows[app.cursor].text.starts_with("     ▎   r2 ⇢ a.txt:2"), "{}", app.rows[app.cursor].text);
    assert!(screen.contains("⋯ Reading the patch.") && screen.contains("⋯ Looking at a.txt.") && screen.contains("⋯ Seen."), "{screen}");
    assert!(at(&screen, "⋯ Seen.").1 < at(&screen, "▎   r1 ↗").1, "{screen}");
    keys(&mut app, &[KeyCode::Enter]);
    assert!(matches!(&app.mode, Mode::Preview(p) if p.title == "a.txt:2 · as patch 1 leaves it"), "{:?}", app.mode);
    // Enter on the other reference goes to the comment it points at.
    keys(&mut app, &[KeyCode::Esc, KeyCode::Up]);
    assert!(app.rows[app.cursor].text.starts_with("     ▎   r1 ↗"), "{}", app.rows[app.cursor].text);
    keys(&mut app, &[KeyCode::Enter]);
    let a = notes_of(&db, "s", 0).into_iter().find(|n| n.text == "why so loud?").unwrap();
    assert!(app.rows[app.cursor].head && app.rows[app.cursor].note.as_deref() == Some(a.id.as_str()), "{}", app.rows[app.cursor].text);
    // On a line of the stream as it changes, the cursor stays on one; once
    // the agent is done, the stream goes, and the cursor stays on the
    // comment.
    down_to(&mut app, "     ⋯ Looking at a.txt.");
    agent_writes(&db, &["Done."]);
    app.refresh();
    draw(&mut app);
    assert!(app.rows[app.cursor].stream && app.rows[app.cursor].note.as_deref() == Some(b.id.as_str()), "{}", app.rows[app.cursor].text);
    Db::edit(&db, |d| {
        let r = d.run("run-changing").cloned().unwrap();
        d.add_run(stacked::db::Run { state: stacked::db::RunState::Done, ..r }).map(|_| ())
    })
    .unwrap();
    app.refresh();
    draw(&mut app);
    assert!(!app.rows.iter().any(|r| r.stream), "the stream is gone");
    assert_eq!(app.rows[app.cursor].text, "     ▎   Which one?");
}

#[test]
fn the_editor_on_a_comment_the_agent_is_at_goes_over_its_stream() {
    let (_tmp, db, _src) = setup();
    let (mut app, b) = a_thread_with_references(&db);
    // A reply being written as the agent starts on the comment, asked
    // elsewhere: the editor under the last reply, then the stream, then
    // the references.
    down_to(&mut app, "     ▎   Which one?");
    choose_as(&mut app, "Reply", "here");
    changing(&db, &b.change, &b.id, serde_json::json!({"patch": b.change}));
    agent_writes(&db, &["Reading the patch."]);
    app.refresh();
    assert!(matches!(&app.mode, Mode::Edit(e) if matches!(e.target, stacked::tui::Target::Reply { .. })), "{:?}", app.mode);
    let screen = draw(&mut app);
    let y = |text: &str| at(&screen, text).1;
    let (reply, top, stream, r1, r2) = (y("▎   Which one?"), y("┌─"), y("⋯ Reading the patch."), y("▎   r1 ↗"), y("▎   r2 ⇢"));
    assert!(reply < top && top < stream && stream < r1 && r1 < r2, "{screen}");
    keys(&mut app, &[KeyCode::Esc]);
    assert_eq!(app.mode, Mode::Normal);
    // The agent done, its reply written again as the agent starts on the
    // comment once more: the editor in that reply's place, your reply under
    // the editor, then the stream, then the references.
    Db::edit(&db, |d| {
        let r = d.run("run-changing").cloned().unwrap();
        d.add_run(stacked::db::Run { state: stacked::db::RunState::Done, ..r }).map(|_| ())
    })
    .unwrap();
    app.refresh();
    keys(&mut app, &[KeyCode::Home]);
    down_to(&mut app, "     ▎   As @r2 shows.");
    choose_as(&mut app, "Edit the agent's reply", "here");
    changing(&db, &b.change, &b.id, serde_json::json!({"patch": b.change}));
    agent_writes(&db, &["Reading it again."]);
    app.refresh();
    assert!(matches!(&app.mode, Mode::Edit(e) if matches!(e.target, stacked::tui::Target::Turn { turn: 1, .. })), "{:?}", app.mode);
    let screen = draw(&mut app);
    let y = |text: &str| at(&screen, text).1;
    let (first, top, reply, stream, r1) = (y("▎   Same as @r1?"), y("┌─"), y("▎   Which one?"), y("⋯ Reading it again."), y("▎   r1 ↗"));
    assert!(first < top && top < reply && reply < stream && stream < r1, "{screen}");
}

/// The lines under the comments drawn, a reference each, as they say it:
/// `r2 ⇢ a.txt:2-3 · as patch 1 leaves it`.
fn references_under(app: &mut App) -> Vec<String> {
    draw(app);
    app.rows.iter().filter(|r| r.is_reference()).map(|r| r.text.trim().trim_start_matches('▎').trim().to_string()).collect()
}

#[test]
fn a_reference_no_longer_named_is_detached_by_a_rewind_or_an_edit() {
    use stacked::refs::{Attached, To};
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "why so loud?");
    down_to(&mut app, "+LINE 39");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "Same as the other?");
    let a = notes_of(&db, "s", 0).into_iter().find(|n| n.text == "why so loud?").unwrap();
    let b = notes_of(&db, "s", 0).into_iter().find(|n| n.text == "Same as the other?").unwrap();
    // Attached: r1, the comment on TWO, and r5, which your reply names; r2,
    // for what it says, named by none. The agent names its r3, and attaches
    // r4 without naming it.
    let lines = |n: usize, l: &str| Attached { n, to: To::Lines(l.into()) };
    Db::edit(&db, |d| {
        let said = "It is: @r3 shows it.\n\n```refs\nr3 a.txt:2 @1\nr4 b.txt @2\n```";
        let text = append_turn(&b.text, Role::Agent, said, Some("answer"));
        let text = append_turn(&text, Role::You, "And @r1? Or @r5, @r4?", None);
        let r1 = Attached {
            n: 1,
            to: To::Comment {
                change: a.change.clone(),
                id: a.id.clone(),
            },
        };
        d.set_note(Note {
            text: append_turn(&text, Role::Agent, "Neither.", Some("answer")),
            attached: vec![r1, lines(2, "a.txt:2-3 @1"), lines(5, "a.txt:5 @1")],
            ..b.clone()
        })
    })
    .unwrap();
    app.refresh();
    let all = ["r2 ⇢ a.txt:2-3 · as patch 1 leaves it", "r3 ⇢ a.txt:2 · as patch 1 leaves it", "r4 ⇢ b.txt · as patch 2 leaves it"];
    assert_eq!(references_under(&mut app).len(), 5);
    // Rewound from your reply, the only turn naming r1 and r5: both are
    // detached, as the status line says. r2 stays, as nothing named it; so
    // do the agent's r3, which it names, and r4, which its reply attaches.
    down_to(&mut app, "     ▎   Neither.");
    choose_as(&mut app, "Rewind the conversation from your reply", "here");
    typed(&mut app, "Thanks.");
    keys(&mut app, &[KeyCode::Esc]);
    choose(&mut app, "Save it");
    assert_eq!(app.status, "r1 and r5 are no longer named: they are detached");
    assert_eq!(references_under(&mut app), all);
    // No longer the comment's, they are not told the agent either.
    let n = notes_of(&db, "s", 0).into_iter().find(|n| n.id == b.id).unwrap();
    assert_eq!(n.attached.iter().map(Attached::name).collect::<Vec<_>>(), ["r2"]);
    assert_eq!(stacked::refs::numbers(&n), [2, 3, 4]);
    // The next one attached takes a number never given in the comment;
    // detached before it is saved, its number is not given again either.
    keys(&mut app, &[KeyCode::Home]);
    down_to(&mut app, "     ▎   Thanks.");
    choose_as(&mut app, "Edit your reply", "here");
    let attach = |app: &mut App, said: &str| {
        keys(app, &[KeyCode::Esc]);
        choose(app, "Attach a reference…");
        choose(app, "Lines of a file…");
        typed(app, "a.txt:7 @1");
        keys(app, &[KeyCode::Enter]);
        assert_eq!(app.status, said);
    };
    attach(&mut app, "attached as r6: write @r6 where you mean it");
    keys(&mut app, &[KeyCode::Esc]);
    choose(&mut app, "Detach a reference…");
    choose(&mut app, "r6 ⇢");
    assert_eq!(app.status, "r6 is detached");
    attach(&mut app, "attached as r7: write @r7 where you mean it");
    typed(&mut app, " And @r7?");
    keys(&mut app, &[KeyCode::Esc]);
    choose(&mut app, "Save it");
    assert!(references_under(&mut app).contains(&"r7 ⇢ a.txt:7 · as patch 1 leaves it".to_string()));
    // Your reply written again without naming it: r7 is detached, and its
    // number is kept from being given again.
    keys(&mut app, &[KeyCode::Home]);
    down_to(&mut app, "     ▎   Thanks. And @r7?");
    choose_as(&mut app, "Edit your reply", "here");
    for _ in 0.." And @r7?".len() {
        key(&mut app, KeyCode::Backspace);
    }
    keys(&mut app, &[KeyCode::Esc]);
    choose(&mut app, "Save it");
    assert_eq!(app.status, "r7 is no longer named: it is detached");
    assert_eq!(references_under(&mut app), all);
    let n = notes_of(&db, "s", 0).into_iter().find(|n| n.id == b.id).unwrap();
    assert_eq!((n.attached.len(), n.given, stacked::refs::next_of(&n)), (1, Some(7), 8));
    // The agent's reply waiting for review names r2: a rewind that took
    // every other mention of it would keep it, as that reply goes in once
    // accepted.
    held(&db, "run-answer", &b.change, &b.id, "As @r2 says.", false);
    let d = Db::load(&db).unwrap();
    let waiting = stacked::refs::waiting(&d, &b.change, &b.id);
    assert_eq!(waiting.as_deref(), Some("As @r2 says."));
    let was = Note {
        text: append_turn(&n.text, Role::You, "So @r2?", None),
        ..n.clone()
    };
    let mut now = Note {
        text: stacked::thread::rewind(&was.text, 2, "Fine.").unwrap(),
        ..was.clone()
    };
    assert!(stacked::refs::settle(&was, &mut now, waiting.as_deref()).is_empty());
    assert_eq!(stacked::refs::settle(&was, &mut now, None), [2]);
}

#[test]
fn a_turn_written_again_keeps_its_references_but_those_no_longer_named_and_a_deleted_turn_s_count_nowhere() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "why so loud?");
    let n = notes_of(&db, "s", 0).remove(0);
    // The agent names r1 and r2, and attaches r3 without naming it; a reply
    // of its deleted the first way names r4 and attaches it.
    Db::edit(&db, |d| {
        let said = "@r1 and @r2 show it.\n\n```refs\nr1 a.txt:2 @1\nr2 a.txt:3 @1\nr3 b.txt @2\n```";
        let text = append_turn(&n.text, Role::Agent, said, Some("answer"));
        let text = format!("{text}\n```deleted agent answer\n> As @r4 says.\n>\n> ```refs\n> r4 a.txt:9 @1\n> ```\n```");
        d.set_note(Note {
            text: append_turn(&text, Role::You, "Thanks.", None),
            ..n.clone()
        })
    })
    .unwrap();
    app.refresh();
    // The deleted reply's reference is shown nowhere, and is not the
    // comment's.
    let three = ["r1 ⇢ a.txt:2 · as patch 1 leaves it", "r2 ⇢ a.txt:3 · as patch 1 leaves it", "r3 ⇢ b.txt · as patch 2 leaves it"];
    assert_eq!(references_under(&mut app), three);
    let refs = stacked::refs::references(&notes_of(&db, "s", 0).remove(0));
    assert_eq!(refs.iter().map(|(k, ..)| *k).collect::<Vec<_>>(), [1, 2, 3]);
    // Written again, the agent's reply is its text alone in the editor, its
    // references under the comment still.
    down_to(&mut app, "     ▎   @r1 and @r2 show it.");
    choose_as(&mut app, "Edit the agent's reply", "here");
    assert!(matches!(&app.mode, Mode::Edit(e) if e.ed.text() == "@r1 and @r2 show it."), "{:?}", app.mode);
    let screen = draw(&mut app);
    assert!(!screen.contains("```refs"), "{screen}");
    assert_eq!(references_under(&mut app), three);
    // Its mention of r2 taken away: r2 is taken out of its references, said
    // so; r3, which it never named, stays.
    ctrl(&mut app, KeyCode::Char('a'));
    ctrl(&mut app, KeyCode::Char('k'));
    typed(&mut app, "@r1 shows it.");
    keys(&mut app, &[KeyCode::Esc]);
    choose(&mut app, "Save it");
    assert_eq!(app.status, "r2 is no longer named: it is detached");
    assert_eq!(references_under(&mut app), [three[0], three[2]]);
    let turns = stacked::thread::all_turns(&notes_of(&db, "s", 0).remove(0).text);
    assert_eq!(
        (turns[1].body.as_str(), turns[1].tag.as_deref()),
        ("@r1 shows it.\n\n```refs\nr1 a.txt:2 @1\nr3 b.txt @2\n```", Some("answer"))
    );
    assert!(turns[2].deleted && turns[2].body.contains("r4 a.txt:9 @1"), "the deleted reply as it was: {turns:?}");
    // Attached next, a reference takes a number none had: neither r2's nor
    // the deleted reply's.
    keys(&mut app, &[KeyCode::Home]);
    down_to(&mut app, "     ▎   Thanks.");
    choose_as(&mut app, "Edit your reply", "here");
    keys(&mut app, &[KeyCode::Esc]);
    choose(&mut app, "Attach a reference…");
    choose(&mut app, "Lines of a file…");
    typed(&mut app, "a.txt:1 @1");
    keys(&mut app, &[KeyCode::Enter]);
    assert_eq!(app.status, "attached as r5: write @r5 where you mean it");
    keys(&mut app, &[KeyCode::Esc]);
    choose(&mut app, "Throw the changes away");
}

#[test]
fn a_web_link_is_attached_named_shown_whole_and_opened_by_a_stand_in() {
    use stacked::refs::{Attached, To};
    use stacked::tui::Jump;
    let (tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    // Links of this test's own: the one you attach longer than the middle
    // is wide, the agent's short.
    let here = tmp.path().file_name().unwrap().to_string_lossy().to_string();
    let url = format!("https://example.com/{here}/{}end", "long/".repeat(12));
    let agents = format!("https://example.com/{here}/agent");
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    typed(&mut app, "Is it as @r1 says?");
    keys(&mut app, &[KeyCode::Esc]);
    // A web link is attached as lines are: written on a line of its own.
    choose(&mut app, "Attach a reference…");
    assert_eq!(items(&app)[..2], [("Lines of a file…".to_string(), true), ("A web link…".to_string(), true)]);
    choose(&mut app, "A web link…");
    let Mode::Lines(l) = &app.mode else { panic!("{:?}", app.mode) };
    assert!(l.link && l.ed.text().is_empty(), "{l:?}");
    assert_eq!(app.hint(), "enter: attach this link · esc: back to the comment");
    assert!(draw(&mut app).contains("a web link, to attach: https://example.com/a/b"));
    // What is not a web link is refused, as the status line says; the line
    // stays to write again.
    for (wrong, why) in [
        ("ftp://example.com/a", "a web link starts with https:// or http://: write it whole, as https://example.com/a/b"),
        ("https://", "the link names no site after https://: write it whole, as https://example.com/a/b"),
    ] {
        typed(&mut app, wrong);
        keys(&mut app, &[KeyCode::Enter]);
        assert_eq!(app.status, why);
        assert!(matches!(&app.mode, Mode::Lines(l) if l.ed.text() == wrong), "{:?}", app.mode);
        ctrl(&mut app, KeyCode::Char('a'));
        ctrl(&mut app, KeyCode::Char('k'));
    }
    typed(&mut app, &url);
    keys(&mut app, &[KeyCode::Enter]);
    assert_eq!(app.status, "attached as r1: write @r1 where you mean it");
    let Mode::Edit(e) = &app.mode else { panic!("{:?}", app.mode) };
    assert!(e.ed.broken().is_empty(), "@r1 names it");
    // Saved and asked: the agent answers, attaching a web link of its own.
    answer_with(&db, &serde_json::json!({"replies": {"c1": {"text": "It is: see @r2.", "refs": [{"url": agents}]}}}));
    keys(&mut app, &[KeyCode::Esc]);
    choose_as(&mut app, "Save it and ask agent", "answer only");
    let n = notes_of(&db, "s", 0).remove(0);
    assert_eq!(n.attached, [Attached { n: 1, to: To::Link(url.clone()) }]);
    accept_landed(&mut app, &db);
    wait_for(&mut app, "agent replied (answer only)");
    // Under the comment, a line each: yours cut to the middle, its start
    // kept; the agent's whole.
    let under = references_under(&mut app);
    assert_eq!(under.len(), 2, "{under:?}");
    let start = under[0].strip_prefix("r1 ⇗ ").and_then(|s| s.strip_suffix('…')).unwrap_or_else(|| panic!("{under:?}"));
    assert!(url.starts_with(start) && start.starts_with("https://example.com/"), "{under:?}");
    assert_eq!(under[1], format!("r2 ⇗ {agents}"));
    let row = |app: &App, text: &str| app.rows.iter().find(|r| r.text.contains(text)).cloned().unwrap_or_else(|| panic!("no {text:?}"));
    assert_eq!(row(&app, "Is it as @r1 says?").names, [1]);
    assert_eq!(row(&app, "It is: see @r2.").names, [2]);
    // A reply written in $EDITOR lists them under it, whole.
    let seen = std::rc::Rc::new(std::cell::RefCell::new(String::new()));
    let kept = seen.clone();
    stacked::ops::stand_in(Some(Box::new(move |file: &Path, _| {
        *kept.borrow_mut() = std::fs::read_to_string(file).unwrap();
        Ok(())
    })));
    keys(&mut app, &[KeyCode::Home]);
    down_to(&mut app, "     ▎   It is: see @r2.");
    let e = choose_as(&mut app, "Reply", "in $EDITOR");
    app.effect(e).unwrap();
    stacked::ops::stand_in(None);
    assert!(seen.borrow().contains(&format!("\n#   r1 ⇗ {url}\n#   r2 ⇗ {agents} (the agent's)\n")), "{}", seen.borrow());
    // Enter on the line naming it, or on its own: the link shown whole, with
    // what Enter does there.
    for at in ["     ▎   Is it as @r1 says?", "     ▎   r1 ⇗"] {
        keys(&mut app, &[KeyCode::Home]);
        down_to(&mut app, at);
        keys(&mut app, &[KeyCode::Enter]);
        let Mode::Preview(p) = &app.mode else { panic!("{at:?}: {:?}", app.mode) };
        assert_eq!((p.title.as_str(), p.jump.clone()), ("a web link", Jump::Link(url.clone())));
        assert_eq!(p.texts(), [url.as_str(), "", "Enter opens it in your browser · Esc back"]);
        assert_eq!(app.hint(), "enter: open it in your browser · esc: back");
        let screen = draw(&mut app);
        assert!(screen.contains(&url) && screen.contains("Enter opens it in your browser · Esc back"), "{screen}");
        keys(&mut app, &[KeyCode::Esc]);
        assert_eq!(app.mode, Mode::Normal);
    }
    // Too long for the box, it is folded there, whole still.
    keys(&mut app, &[KeyCode::Enter]);
    let mut buf = Buffer::empty(Rect::new(0, 0, 60, 30));
    app.draw(&mut buf);
    let lines: Vec<String> = (0..30u16).map(|y| (0..60u16).map(|x| buf[(x, y)].symbol()).collect()).collect();
    let (top, bottom) = url.split_at(50);
    let at = lines.iter().position(|l| l.contains(top)).unwrap_or_else(|| panic!("{lines:#?}"));
    assert!(lines[at + 1].contains(bottom), "{lines:#?}");
    // Enter there opens it with the stand-in, never a browser: the status
    // line says so.
    assert_eq!(stacked::refs::opener(), fake_opener());
    keys(&mut app, &[KeyCode::Enter]);
    app.settle();
    assert_eq!(app.status, format!("opened {url} in your browser"));
    assert!(opened(&url) && !opened(&agents));
    assert_eq!(app.mode, Mode::Normal);
    // The agent's, too.
    keys(&mut app, &[KeyCode::Home]);
    down_to(&mut app, "     ▎   r2 ⇗");
    keys(&mut app, &[KeyCode::Enter, KeyCode::Enter]);
    app.settle();
    assert!(opened(&agents), "{}", app.status);
    // Rewound from your comment, the only turn naming r1: r1 is detached,
    // the agent's r2 gone with its reply; neither number is given again.
    keys(&mut app, &[KeyCode::Home]);
    down_to(&mut app, "     ▎   Is it as @r1 says?");
    choose_as(&mut app, "Rewind the conversation from your comment", "here");
    typed(&mut app, "Thanks.");
    keys(&mut app, &[KeyCode::Esc]);
    choose(&mut app, "Save it");
    assert_eq!(app.status, "r1 is no longer named: it is detached");
    assert!(references_under(&mut app).is_empty());
    let n = notes_of(&db, "s", 0).remove(0);
    assert_eq!((n.attached.len(), stacked::refs::next_of(&n)), (0, 3));
    keys(&mut app, &[KeyCode::Home]);
    down_to(&mut app, "     ▎   Thanks.");
    choose_as(&mut app, "Edit your comment", "here");
    keys(&mut app, &[KeyCode::Esc]);
    choose(&mut app, "Attach a reference…");
    choose(&mut app, "A web link…");
    typed(&mut app, &agents);
    keys(&mut app, &[KeyCode::Enter]);
    assert_eq!(app.status, "attached as r3: write @r3 where you mean it");
    keys(&mut app, &[KeyCode::Esc]);
    choose(&mut app, "Throw the changes away");
}

#[test]
fn a_comment_holding_a_fence_of_a_turn_is_yours_whole() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    for (k, line) in ["Should it read so?", "```agent", "I did it.", "```"].iter().enumerate() {
        if k > 0 {
            keys(&mut app, &[KeyCode::Enter]);
        }
        typed(&mut app, line);
    }
    keys(&mut app, &[KeyCode::Esc]);
    choose(&mut app, "Save it");
    let n = notes_of(&db, "s", 0).remove(0);
    let turns = stacked::thread::thread_turns(&n.text);
    assert_eq!(turns.len(), 1, "{}", n.text);
    assert_eq!((turns[0].role, turns[0].body.as_str()), (Role::You, "Should it read so?\n```agent\nI did it.\n```"));
    assert!(!draw(&mut app).contains("agent:"), "shown as yours");
}

#[test]
fn every_open_comment_of_the_stack_is_closed_at_once_once_asked() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "why so loud?");
    down_to(&mut app, "+LINE 39");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "and this one?");
    keys(&mut app, &[KeyCode::Left, KeyCode::Home]);
    choose(&mut app, "Comment on the stack…");
    save(&mut app, "and the rest");
    // A reply to one waits for your review: it stays open.
    let waits = notes_of(&db, "s", 0).into_iter().find(|n| n.text == "and this one?").unwrap();
    Db::edit(&db, |d| {
        let reply = stacked::held::Reply {
            change: waits.change.clone(),
            id: waits.id.clone(),
            text: "Later.".into(),
            reviewer: Some("ann".into()),
            ..Default::default()
        };
        d.add_run(stacked::db::Run {
            id: "pull-1".into(),
            what: "pull".into(),
            stack: "s".into(),
            state: stacked::db::RunState::Done,
            held: Some(stacked::held::Held {
                at: 1,
                replies: vec![reply],
                ..Default::default()
            }),
            ..Default::default()
        })
        .map(|_| ())
    })
    .unwrap();
    app.refresh();
    // On the stack's menu: asked first, how many said, keeping them open
    // picked.
    keys(&mut app, &[KeyCode::Home]);
    assert!(app.focus == Focus::List && app.on_stack, "{:?}", app.focus);
    choose(&mut app, "Close all comments…");
    let Mode::Menu(m) = &app.mode else { panic!("{:?}", app.mode) };
    assert_eq!(
        m.title,
        "close 2 of the stack's 3 open comments? 1 stays open: with the agent, or a reply waiting for your review. Show…, closed, can reopen each"
    );
    assert_eq!(items(&app), [("Close them".to_string(), true), ("Keep them open".to_string(), true)]);
    assert_eq!(m.sel, 1);
    keys(&mut app, &[KeyCode::Enter]);
    assert_eq!(Db::load(&db).unwrap().stack_notes("s").len(), 3, "kept open");
    // Closed: those that can be.
    choose(&mut app, "Close all comments…");
    choose(&mut app, "Close them");
    assert_eq!(app.status, "closed 2 comments: Show…, closed, lists them to reopen; 1 left open, with the agent or a reply waiting for your review");
    let d = Db::load(&db).unwrap();
    assert_eq!(d.stack_notes("s").into_iter().map(|n| n.text.clone()).collect::<Vec<_>>(), ["and this one?"]);
    drop(d);
    // Nothing more to close: not to be chosen.
    keys(&mut app, &[KeyCode::Char(' ')]);
    assert!(items(&app).contains(&("Close all comments…".to_string(), false)), "{:?}", items(&app));
}

#[test]
fn closing_all_says_how_many_it_closes() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Left, KeyCode::Home]);
    choose(&mut app, "Comment on the stack…");
    save(&mut app, "one");
    keys(&mut app, &[KeyCode::Home]);
    choose(&mut app, "Close all comments…");
    let Mode::Menu(m) = &app.mode else { panic!("{:?}", app.mode) };
    assert_eq!(m.title, "close the stack's open comment? Show…, closed, can reopen it");
    assert_eq!(items(&app), [("Close it".to_string(), true), ("Keep it open".to_string(), true)]);
    keys(&mut app, &[KeyCode::Esc]);
    choose(&mut app, "Comment on the stack…");
    save(&mut app, "two");
    keys(&mut app, &[KeyCode::Home]);
    choose(&mut app, "Close all comments…");
    let Mode::Menu(m) = &app.mode else { panic!("{:?}", app.mode) };
    assert_eq!(m.title, "close all 2 open comments of the stack? Show…, closed, can reopen each");
    choose(&mut app, "Close them");
    assert_eq!(app.status, "closed 2 comments: Show…, closed, lists them to reopen");
    assert!(Db::load(&db).unwrap().stack_notes("s").is_empty());
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

/// A claude that says what it does, then answers each comment it is sent;
/// for every test of the editor, as they share their process. While a test
/// holds it (`hold`), it goes on working on that test's asks, so that what
/// the editor shows while the agent works stays to be seen, however busy
/// the machine is: it answers once there is no file `hold` beside the
/// repository it works in, where `setup` puts the test's database too. It
/// answers `Seen.`, or what the file `answer` there holds (`answer_with`).
fn fake_claude() -> PathBuf {
    static FAKE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    FAKE.get_or_init(|| {
        let result = serde_json::to_string(&serde_json::json!({"type": "result", "result": r#"{"replies": {"c1": "Seen."}}"#})).unwrap();
        let script = format!(
            "#!/bin/sh\necho '{{\"type\":\"system\",\"subtype\":\"init\",\"model\":\"fake\"}}'\necho '{{\"type\":\"assistant\",\"message\":{{\"content\":[{{\"type\":\"text\",\"text\":\"Reading the patch.\"}}]}}}}'\n\
             git=$(git rev-parse --path-format=absolute --git-common-dir)\nwhile test -e \"$git/../../hold\"; do sleep 0.1; done\n\
             if test -e \"$git/../../answer\"; then cat \"$git/../../answer\"; else cat <<'EOF'\n{result}\nEOF\nfi\n"
        );
        // Written in the folder Cargo gives the tests for their files, not in
        // the temporary directory, where it would be left: the same script
        // for every run of the tests, written whole before it takes the place
        // of the one there, which another run may be running.
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR"));
        let (path, part) = (dir.join("tui-claude.sh"), dir.join(format!("tui-claude-{}.sh", std::process::id())));
        std::fs::write(&part, script).unwrap();
        Command::new("chmod").arg("+x").arg(&part).status().unwrap();
        std::fs::rename(&part, &path).unwrap();
        stacked::ask::stand_in(&path, Path::new(env!("CARGO_BIN_EXE_stacked")));
        path
    })
    .clone()
}

/// The stand-in held at work on a test's asks (`hold`): let go once this
/// is dropped.
struct Held(PathBuf);

impl Drop for Held {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Holds the stand-in at work on the asks on `db` (`fake_claude`), from
/// now on until what this gives is dropped.
fn hold(db: &Path) -> Held {
    let at = db.with_file_name("hold");
    std::fs::write(&at, "").unwrap();
    Held(at)
}

/// Has the stand-in answer the asks on `db` (`fake_claude`) with
/// `answer`, as claude ends its turn with it, in place of `Seen.`.
fn answer_with(db: &Path, answer: &serde_json::Value) {
    let result = serde_json::json!({"type": "result", "result": answer.to_string()});
    std::fs::write(db.with_file_name("answer"), format!("{result}\n")).unwrap();
}

/// The program the editor opens web links with in the tests, for every
/// test as they share their process: a stand-in that opens nothing, and
/// writes each link it is given on a line of `tui-opened.txt` beside it, in
/// the folder Cargo gives the tests for their files (`opened`). No test
/// opens a browser.
fn fake_opener() -> PathBuf {
    static FAKE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    FAKE.get_or_init(|| {
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR"));
        let (path, part) = (dir.join("tui-open.sh"), dir.join(format!("tui-open-{}.sh", std::process::id())));
        std::fs::write(&part, "#!/bin/sh\nprintf '%s\\n' \"$1\" >> \"$(dirname \"$0\")/tui-opened.txt\"\n").unwrap();
        Command::new("chmod").arg("+x").arg(&part).status().unwrap();
        std::fs::rename(&part, &path).unwrap();
        stacked::refs::open_links_with(&path);
        path
    })
    .clone()
}

/// Whether the stand-in that opens links (`fake_opener`) was given `url`,
/// by this run of the tests or another: each test's links are its own.
fn opened(url: &str) -> bool {
    let log = Path::new(env!("CARGO_TARGET_TMPDIR")).join("tui-opened.txt");
    std::fs::read_to_string(log).unwrap_or_default().lines().any(|l| l == url)
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
    assert_eq!(labels, ["Ask agent", "Send it", "Attach a reference…", "", "Leave it as it is", "Keep writing"]);
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
    let held = hold(&db);
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
    drop(held);
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
    // Lines a patch adds are green, as ever; the one the cursor is on is in
    // the cursor's colours.
    assert_eq!(app.rows[app.cursor].text, "+TWO");
    let (x, y) = at(&screen, "+LINE 39");
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
    assert_eq!(items(&app)[0], ("Ask agent".to_string(), true));
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
fn heads_line_stays_in_sight_when_the_stack_is_too_long_for_the_pane() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir(&src).unwrap();
    git(&src, &["init", "-q", "-b", "main"]);
    git(&src, &["config", "commit.gpgsign", "false"]);
    git(&src, &["config", "tag.gpgsign", "false"]);
    std::fs::write(src.join("base.txt"), "base\n").unwrap();
    git(&src, &["add", "."]);
    git(&src, &["commit", "-qm", "base"]);
    for n in 1..=12 {
        std::fs::write(src.join(format!("{n}.txt")), format!("{n}\n")).unwrap();
        git(&src, &["add", "."]);
        git(&src, &["commit", "-qm", &format!("Bug {n} - add {n}")]);
    }
    let db = tmp.path().join("db.jsonl");
    stacked(&src, &db, &["import", "s", "HEAD~12"]);
    let mut app = open(&db).unwrap();
    app.checkout = src.clone();
    app.look_at_head();
    app.settle();
    // A screen of 12 lines: the 12 patches and HEAD's line do not fit.
    let small = |app: &mut App| {
        let mut buf = Buffer::empty(Rect::new(0, 0, 120, 12));
        app.draw(&mut buf);
        (0..buf.area.height).map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect::<String>()).collect::<Vec<_>>()
    };
    let row = |lines: &[String], text: &str| lines.iter().position(|l| l.contains(text));
    // On patch 1, HEAD after patch 12: on the pane's last line, said to
    // be further down, the patches over it.
    let lines = small(&mut app);
    let screen = lines.join("\n");
    let head = row(&lines, "↓ HEAD main").unwrap_or_else(|| panic!("{screen}"));
    assert!(row(&lines, "  1 Bug 1").is_some_and(|y| y < head) && !screen.contains("12 Bug 12"), "{screen}");
    assert!(lines[head + 1..].iter().all(|l| !l.contains(" Bug ")), "{screen}");
    // On patch 12: in its place, under it.
    for _ in 0..11 {
        key(&mut app, KeyCode::Down);
        small(&mut app);
    }
    assert_eq!(app.sel, 11);
    let lines = small(&mut app);
    let screen = lines.join("\n");
    assert_eq!(row(&lines, "▶ HEAD main"), row(&lines, " 12 Bug 12").map(|y| y + 1), "{screen}");
    assert!(!screen.contains("↓ HEAD"), "{screen}");
    // HEAD at the base, the cursor on patch 12: under the header, said to
    // be further up.
    git(&src, &["checkout", "-q", "--detach", "HEAD~12"]);
    app.look_at_head();
    app.settle();
    let lines = small(&mut app);
    let screen = lines.join("\n");
    let head = row(&lines, "↑ HEAD ").unwrap_or_else(|| panic!("{screen}"));
    assert_eq!(Some(head), row(&lines, "≡ s").map(|y| y + 1), "{screen}");
    assert!(row(&lines, " 12 Bug 12").is_some_and(|y| y > head) && !screen.contains("  1 Bug 1"), "{screen}");
    // Back on patch 1: in its place, over it.
    for _ in 0..11 {
        key(&mut app, KeyCode::Up);
        small(&mut app);
    }
    let lines = small(&mut app);
    let screen = lines.join("\n");
    assert_eq!(row(&lines, "▶ HEAD "), row(&lines, "  1 Bug 1").map(|y| y - 1), "{screen}");
    assert!(!screen.contains("↑ HEAD"), "{screen}");
}

#[test]
fn lines_too_wide_for_the_pane_are_folded_each_the_same_line() {
    use stacked::rows::Kind;
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir(&src).unwrap();
    git(&src, &["init", "-q", "-b", "main"]);
    git(&src, &["config", "commit.gpgsign", "false"]);
    git(&src, &["config", "tag.gpgsign", "false"]);
    std::fs::write(src.join("a.txt"), "one\n").unwrap();
    git(&src, &["add", "."]);
    git(&src, &["commit", "-qm", "base"]);
    // A line of code, and a summary typed as one line, both too wide.
    let long = format!("two{}", " and more".repeat(30));
    std::fs::write(src.join("a.txt"), format!("one\n{long}\n")).unwrap();
    let summary = ["A summary typed as one long line, as Phabricator has them."; 4].join(" ");
    git(&src, &["commit", "-qam", &format!("Bug 1 - add a long line\n\n{summary}")]);
    let db = tmp.path().join("db.jsonl");
    stacked(&src, &db, &["import", "s", "HEAD~1"]);
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    let screen = draw(&mut app);
    // The code, folded where the pane ends: each piece after the first
    // `↪`, all of the line in them, each in sight, uncut.
    let pieces = |app: &App| app.rows.iter().filter(|r| r.kind == Kind::Add).map(|r| r.text.clone()).collect::<Vec<_>>();
    let unified = pieces(&app);
    assert!(unified.len() > 1 && unified[1..].iter().all(|p| p.starts_with('↪')), "{unified:?}");
    let back: String = unified.iter().map(|p| p.chars().skip(1).collect::<String>()).collect();
    assert_eq!(back, long);
    for p in &unified {
        assert!(screen.contains(p.trim_end()), "{p:?}\n{screen}");
    }
    // The message, folded between words: all of it there.
    let said: Vec<String> = app.rows.iter().filter(|r| r.msg_at == Some(2)).map(|r| r.text.trim().to_string()).collect();
    assert!(said.len() > 1, "{said:?}");
    assert_eq!(said.join(" "), summary);
    for l in &said {
        assert!(screen.contains(l.as_str()), "{l:?}\n{screen}");
    }
    // Side by side: folded in the half of the pane each side has; then
    // back to the one view.
    keys(&mut app, &[KeyCode::Char('1')]);
    assert_eq!(app.layout, Layout::Split);
    let screen = draw(&mut app);
    let halves = pieces(&app);
    assert!(halves.len() > unified.len(), "{halves:?}");
    for p in &halves {
        assert!(screen.contains(p.trim_end()), "{p:?}\n{screen}");
    }
    keys(&mut app, &[KeyCode::Char('1'), KeyCode::Char('1')]);
    assert_eq!(app.layout, Layout::Unified);
    // A comment on the line folded: under its last piece.
    down_to(&mut app, "+two");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "why so long?");
    let head = app.rows.iter().position(|r| r.head && r.note.is_some()).unwrap();
    assert!(app.rows[head - 1].cont && app.rows[head - 1].kind == Kind::Add, "{}", app.rows[head - 1].text);
    // Reading: the line once, however many pieces it is in; and the
    // message's title and summary, each once too.
    keys(&mut app, &[KeyCode::Char('2')]);
    let screen = draw(&mut app);
    assert!(screen.contains("reading 0/3"), "{screen}");
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
    // Enter takes it into the thread, at once: out of the review, which
    // closes, nothing else of its work left. The comment stays open.
    assert!(app.hint().starts_with("enter: accept the agent's reply"), "{}", app.hint());
    keys(&mut app, &[KeyCode::Enter]);
    assert_eq!(app.status, "you accepted the agent's reply: it is in the thread now");
    let screen = draw(&mut app);
    assert!(screen.contains("It is a shout.") && !screen.contains("⟳"), "{screen}");
    assert!(stacked::held::queue(&Db::load(&db).unwrap(), "s").is_empty(), "nothing left of it to review");
    assert_eq!(notes_of(&db, "s", 0).len(), 1, "the comment open still");
    // A reply going with a change of the stack is reviewed with it; the
    // stack is not edited by hand meanwhile.
    held(&db, "run-change", &n.change, &n.id, "Changed it.", true);
    app.refresh();
    keys(&mut app, &[KeyCode::Char(' ')]);
    let all = items(&app);
    assert!(all.contains(&("Review the agent's work…".into(), true)) && !all.iter().any(|(l, _)| l.starts_with("Accept")), "{all:?}");
    // Enter opens the review of that work, on the reply.
    keys(&mut app, &[KeyCode::Esc]);
    assert!(app.hint().starts_with("enter: review the agent's reply · "), "{}", app.hint());
    keys(&mut app, &[KeyCode::Enter]);
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
fn replies_taken_with_enter_close_their_work_s_review_once_none_is_left() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "why so loud?");
    down_to(&mut app, "+LINE 39");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "and this?");
    // An answer replying to each comment, which opened none.
    answered(&db, "run-1", &notes_of(&db, "s", 0));
    Db::edit(&db, |d| {
        let mut r = d.run("run-1").unwrap().clone();
        r.held.as_mut().unwrap().opened.clear();
        d.add_run(r).map(|_| ())
    })
    .unwrap();
    app.refresh();
    // One taken with Enter on its comment: in its thread. The other reply
    // is what is left of the work, on the banner.
    keys(&mut app, &[KeyCode::Down]);
    assert!(app.hint().starts_with("enter: accept the agent's reply"), "{}", app.hint());
    keys(&mut app, &[KeyCode::Enter]);
    assert_eq!(app.status, "you accepted the agent's reply: it is in the thread now");
    let screen = draw(&mut app);
    assert!(screen.contains("⟳ 1 landed ask waits for your review · the next: 1 reply"), "{screen}");
    assert_eq!(stacked::held::queue(&Db::load(&db).unwrap(), "s").len(), 1);
    // The other taken: nothing of the work is left, and its review closes.
    // Both comments stay open, each its reply in its thread, to reply to.
    keys(&mut app, &[KeyCode::Home]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Down, KeyCode::Enter]);
    assert_eq!(app.status, "you accepted the agent's reply: it is in the thread now");
    assert!(!draw(&mut app).contains("⟳"), "nothing waits");
    let d = Db::load(&db).unwrap();
    assert!(stacked::held::queue(&d, "s").is_empty());
    assert_eq!(d.run("run-1").unwrap().summary, "reviewed: 2 replies accepted");
    let open = notes_of(&db, "s", 0);
    assert_eq!(open.len(), 2, "both open");
    for n in &open {
        assert!(stacked::thread::last_role(&n.text) == Some(Role::Agent) && n.text.contains("Seen: "), "{}", n.text);
    }
    keys(&mut app, &[KeyCode::Char(' ')]);
    assert!(items(&app).contains(&("Reply".into(), true)), "{:?}", items(&app));
}

#[test]
fn a_reply_that_goes_with_comments_the_agent_opened_is_reviewed_with_them() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "why so loud?");
    // An answer replying to the comment, and opening one of its own.
    answered(&db, "run-1", &notes_of(&db, "s", 0));
    app.refresh();
    // Not taken at once: its menu has no Accept, and Enter opens the
    // review of the work, on the reply.
    keys(&mut app, &[KeyCode::Down, KeyCode::Char(' ')]);
    let all = items(&app);
    assert!(all.contains(&("Review the agent's work…".into(), true)) && !all.iter().any(|(l, _)| l.starts_with("Accept")), "{all:?}");
    keys(&mut app, &[KeyCode::Esc]);
    assert!(app.hint().starts_with("enter: review the agent's reply · "), "{}", app.hint());
    let refused = Db::edit(&db, |d| stacked::held::take(d, "run-1", 0)).unwrap_err();
    assert_eq!(refused.0, "this reply goes with the comments the agent opened: review them together");
    keys(&mut app, &[KeyCode::Enter]);
    assert_eq!(app.screen, Screen::Review);
    assert!(draw(&mut app).contains("its reply, as it would be in its thread"), "on the reply");
    // The work waits until its review accepts or rejects the comment it
    // opened, with the reply.
    assert_eq!(stacked::held::queue(&Db::load(&db).unwrap(), "s").len(), 1);
    keys(&mut app, &[KeyCode::Esc]);
    choose(&mut app, "Accept all remaining and Close");
    choose(&mut app, "Accept them and close");
    assert_eq!(app.status, "reviewed: 1 opened comment accepted; 1 reply accepted");
    assert!(stacked::held::queue(&Db::load(&db).unwrap(), "s").is_empty());
}

#[test]
fn a_reply_of_work_going_on_from_an_edit_under_review_waits_for_that_edit() {
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
    let on = |text: &str| notes.iter().find(|n| n.text == text).unwrap();
    // An edit about the first comment, and an answer about the second
    // going on from that edit, as if it is accepted.
    held(&db, "run-1", &on("why so loud?").change, &on("why so loud?").id, "Quieter now.", true);
    held(&db, "run-2", &on("and this?").change, &on("and this?").id, "It shouts too.", false);
    Db::edit(&db, |d| {
        let mut r = d.run("run-2").unwrap().clone();
        let mut ask = r.ask.clone().unwrap();
        ask["after"] = serde_json::json!("run-1");
        r.ask = Some(ask);
        d.add_run(r).map(|_| ())
    })
    .unwrap();
    app.refresh();
    // On the second comment, its reply is not taken at once: its menu has
    // no Accept, and Enter says to review the edit first.
    keys(&mut app, &[KeyCode::Down, KeyCode::Char(' ')]);
    let all = items(&app);
    assert!(all.contains(&("Review the agent's work…".into(), true)) && !all.iter().any(|(l, _)| l.starts_with("Accept")), "{all:?}");
    keys(&mut app, &[KeyCode::Esc]);
    let refused = Db::edit(&db, |d| stacked::held::take(d, "run-2", 0)).unwrap_err();
    assert_eq!(refused.0, "this reply goes on from the work landed before it, which waits for your review: review that one first");
    keys(&mut app, &[KeyCode::Enter]);
    assert_eq!(app.screen, Screen::Main);
    assert_eq!(app.status, "this work goes on from the work landed before it, which waits for your review: review that one first");
    assert_eq!(stacked::held::queue(&Db::load(&db).unwrap(), "s").len(), 2, "both wait");
}

#[test]
fn work_that_brought_nothing_back_is_reviewed_for_the_comment_it_was_asked_about() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "why so loud?");
    let n = notes_of(&db, "s", 0).remove(0);
    // Landed with nothing, its comment still open: held all the same.
    Db::edit(&db, |d| {
        let from = d.head("s").unwrap().0;
        let ask = serde_json::json!({"way": "answer", "scope": {"patch": n.change}, "from": from, "notes": [{"label": "c1", "change": n.change, "id": n.id}]});
        d.add_run(stacked::db::Run {
            id: "run-nothing".into(),
            what: "answer".into(),
            stack: "s".into(),
            state: stacked::db::RunState::Done,
            request: "why so loud?".into(),
            ask: Some(ask),
            held: Some(stacked::held::Held {
                at: stacked::db::now(),
                ..Default::default()
            }),
            ..Default::default()
        })
        .map(|_| ())
    })
    .unwrap();
    app.refresh();
    let screen = draw(&mut app);
    assert!(
        screen.contains("⟳ 1 landed ask waits for your review · the next: 1 asking comment to close or keep, nothing brought back"),
        "{screen}"
    );
    // Its review: the comment alone, the cursor on it. Esc says the agent
    // brought nothing back; closed, the review keeps the comment open.
    keys(&mut app, &[KeyCode::Esc, KeyCode::Enter]);
    assert_eq!(app.screen, Screen::Review);
    assert!(draw(&mut app).contains(" the agent's work: 1 asking comment to close"));
    assert!(selected_line(&mut app).unwrap().starts_with("   keep  a.txt: TWO · why so loud?"), "{:?}", selected_line(&mut app));
    keys(&mut app, &[KeyCode::Esc]);
    let Mode::Menu(m) = &app.mode else { panic!("{:?}", app.mode) };
    assert_eq!(m.note.as_deref(), Some("the agent brought nothing back\nasking comment: 1 kept"));
    choose(&mut app, "Close review");
    assert_eq!(app.status, "reviewed: the agent brought nothing back");
    assert!(!draw(&mut app).contains("⟳"), "nothing waits");
    assert_eq!(notes_of(&db, "s", 0).len(), 1, "the comment kept open");
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
    // From a patch of the list: on the banner, the patch is picked, not
    // reversed, as in a pane the cursor is not in; back, reversed again.
    let reversed = |app: &mut App| {
        let mut buf = Buffer::empty(Rect::new(0, 0, 120, 30));
        app.draw(&mut buf);
        let y = (0..30u16)
            .find(|&y| (0..40u16).map(|x| buf[(x, y)].symbol()).collect::<String>().contains("1 Bug 1 - shout two"))
            .expect("the first patch");
        buf[(2, y)].modifier.contains(ratatui::style::Modifier::REVERSED)
    };
    keys(&mut app, &[KeyCode::Left, KeyCode::Home, KeyCode::Down]);
    assert_eq!(app.focus, Focus::List);
    assert!(reversed(&mut app), "the cursor on it");
    keys(&mut app, &[KeyCode::Esc]);
    assert!(app.on_banner);
    assert!(!reversed(&mut app), "the cursor on the banner");
    keys(&mut app, &[KeyCode::Esc]);
    assert!(!app.on_banner && reversed(&mut app), "the cursor back on it");
    keys(&mut app, &[KeyCode::Right]);
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
    assert_eq!(all[1], "Save it and ask agent", "{all:?}");
    assert_eq!(choices_named(&app, "Save it and ask agent")[..2], ["change", "the whole stack"]);
    choose(&mut app, "Save it");
    // Its menu has it too, beside asking about its patch alone.
    keys(&mut app, &[KeyCode::Down]);
    assert!(app.rows[app.cursor].head);
    keys(&mut app, &[KeyCode::Char(' ')]);
    let all: Vec<String> = items(&app).into_iter().map(|(l, _)| l).collect();
    assert!(all.iter().any(|l| l == "Ask agent"), "{all:?}");
    assert_eq!(choices_named(&app, "Ask agent")[..2], ["change", "the whole stack"]);
    let held = hold(&db);
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
    drop(held);
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
    let held = hold(&db);
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
    drop(held);
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

/// A comment asked about, its stream opened in place: the agent held at
/// work until what this gives is dropped.
fn asked_and_streamed(db: &Path) -> (Editor, Held) {
    fake_claude();
    let mut app = open(db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    typed(&mut app, "why so loud?");
    keys(&mut app, &[KeyCode::Esc]);
    let held = hold(db);
    choose_as(&mut app, "Save it and ask agent", "answer only");
    wait_for(&mut app, "⋯ Reading the patch.");
    keys(&mut app, &[KeyCode::Down]);
    keys(&mut app, &[KeyCode::Enter]);
    assert!(matches!(app.mode, Mode::Stream { .. }), "{:?}", app.mode);
    (app, held)
}

#[test]
fn a_stream_read_in_place_folds_once_the_agent_is_done() {
    let (_tmp, db, _src) = setup();
    let (mut app, held) = asked_and_streamed(&db);
    assert_eq!(app.hint(), "↑↓: scroll · esc: fold it");
    // Its answer lands: the stream folds, the keys are the screen's again,
    // and the bottom line says what waits.
    drop(held);
    let screen = wait_for(&mut app, "⟳ 1 landed ask waits for your review");
    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(app.status, "the agent is done: what it brought waits for your review");
    assert!(!screen.contains("● session started · fake"), "{screen}");
    keys(&mut app, &[KeyCode::Char(' ')]);
    assert!(matches!(app.mode, Mode::Menu(_)), "{:?}", app.mode);
}

#[test]
fn the_first_ask_opens_the_setup_and_goes_once_it_is_saved() {
    let (_tmp, db, src) = setup();
    // The setup not done in the stack's checkout, as its own settings say,
    // over the tests' own, which have it done.
    stacked(&src, &db, &["config", "--local", "agent.setup", "0"]);
    // Where the setup is saved: there, as the stack names its checkout.
    let local = src.canonicalize().unwrap().join(".stacked/local.toml");
    let sent = || {
        let d = Db::load(&db).unwrap();
        (d.runs().len(), notes_of(&db, "s", 0).iter().filter(|n| n.queued.is_some()).count())
    };
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    typed(&mut app, "why so loud?");
    keys(&mut app, &[KeyCode::Esc]);
    choose_as(&mut app, "Save it and ask agent", "answer only");
    // Saved, not sent: the setup first, the agent asked what tools it has
    // away from the screen, as the screen says.
    assert!(matches!(&app.mode, Mode::Setup(su) if su.screen.is_none()), "{:?}", app.mode);
    let screen = draw(&mut app);
    assert!(screen.contains("is being asked what tools it has") && screen.contains(" · asking the agent what tools it has…"), "{screen}");
    assert!(screen.contains(" esc: send nothing"), "{screen}");
    assert_eq!((notes_of(&db, "s", 0).len(), sent()), (1, (0, 0)), "saved, and nothing sent");
    // Esc while it is asked: nothing sent, the bar saying why.
    keys(&mut app, &[KeyCode::Esc]);
    let why = "nothing was sent: what each kind of ask may use is not set up yet, and sending again opens the setup again";
    assert_eq!((&app.mode, app.status.as_str()), (&Mode::Normal, why));
    // Asked again: once the agent said what it has, the screen `stacked
    // setup agent --tui` shows, over all of the editor; Esc there, nothing
    // changed, sends nothing either, and writes nothing.
    keys(&mut app, &[KeyCode::Down]);
    choose_as(&mut app, "Ask agent", "answer only");
    let screen = wait_for(&mut app, " What each kind of ask may use, besides what stacked always gives it: the agent's tools, in ");
    assert!(screen.contains(" Answer-only asks: agent.answer_tools") && screen.contains("   [x] Bash(curl *)   stacked's default for answers"), "{screen}");
    assert!(screen.contains(&format!(" To be, in {}: ", local.display())), "{screen}");
    let Mode::Setup(su) = &app.mode else { panic!("{:?}", app.mode) };
    let to_be = su.screen.as_ref().unwrap().chosen();
    assert_eq!((to_be.change, to_be.answer, to_be.every), (vec![], vec!["Bash(curl *)".to_string(), "Bash(jq *)".to_string()], None));
    assert!(!screen.contains("why so loud?"), "the editor under it is not drawn: {screen}");
    keys(&mut app, &[KeyCode::Esc]);
    assert_eq!((&app.mode, app.status.as_str()), (&Mode::Normal, why));
    assert_eq!(sent(), (0, 0));
    assert_eq!(std::fs::read_to_string(&local).unwrap(), "[agent]\nsetup = 0\n");
    // Saved: written in the checkout's own settings, both lists and the
    // setup done; and the ask goes.
    choose_as(&mut app, "Ask agent", "answer only");
    wait_for(&mut app, " What each kind of ask may use");
    keys(&mut app, &[KeyCode::Char('s')]);
    assert_eq!(app.mode, Mode::Normal);
    let said = format!("the setup is saved in {}; asked the agent about 1 comment (answer only)", local.display());
    assert!(app.status.starts_with(&said), "{}", app.status);
    assert_eq!(
        std::fs::read_to_string(&local).unwrap(),
        "[agent]\nsetup = 1\nchange_tools = []\nanswer_tools = [\"Bash(curl *)\", \"Bash(jq *)\"]\n"
    );
    accept_landed(&mut app, &db);
    wait_for(&mut app, "agent replied (answer only)");
    // Never again: the next goes at once.
    down_to(&mut app, "+LINE 39");
    keys(&mut app, &[KeyCode::Enter]);
    typed(&mut app, "and this one?");
    keys(&mut app, &[KeyCode::Esc]);
    choose_as(&mut app, "Save it and ask agent", "answer only");
    assert_eq!(app.mode, Mode::Normal);
    assert!(app.status.starts_with("asked the agent about 1 comment (answer only)"), "{}", app.status);
}

#[test]
fn a_stream_read_from_the_help_folds_there() {
    let (_tmp, db, _src) = setup();
    let (mut app, held) = asked_and_streamed(&db);
    keys(&mut app, &[KeyCode::Char('?')]);
    assert!(draw(&mut app).contains("Once the agent is done, the stream folds on its own."));
    // Its answer lands: the help goes back to the screen, not the stream.
    drop(held);
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
    let held = hold(&db);
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
    drop(held);
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
    // It writes on, the editor reading and drawing as it does. Each frame is
    // timed by how long this thread ran to read and draw it: the clock on the
    // wall would count too the time the machine gave to other work meanwhile.
    let ran = || {
        let mut t = libc::timespec { tv_sec: 0, tv_nsec: 0 };
        assert_eq!(unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut t) }, 0);
        std::time::Duration::new(t.tv_sec as u64, t.tv_nsec as u32)
    };
    let mut slowest = std::time::Duration::ZERO;
    for k in 0..40 {
        Db::append(&db, &(0..50).map(|n| line(20_000 + k * 50 + n)).collect::<Vec<_>>()).unwrap();
        let t = ran();
        app.refresh();
        draw(&mut app);
        slowest = slowest.max(ran() - t);
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
    // is left to read, the hunk read coming up, its header out of sight.
    keys(&mut app, &[KeyCode::Esc, KeyCode::Char(' ')]);
    assert_eq!(app.mode, Mode::Normal);
    assert!(app.status.contains("read: this hunk, 2 lines"), "{}", app.status);
    assert_eq!(app.rows[app.cursor].text, "-line 39");
    let screen = draw(&mut app);
    assert!(screen.contains("  ✓ +TWO") && app.rows.iter().any(|r| r.text.ends_with("   read 2/2 lines")), "{screen}");
    assert!(screen.contains("reading 2/6") && screen.contains(" 2/6"), "{screen}");
    keys(&mut app, &[KeyCode::Char(' ')]);
    assert!(app.status.contains("nothing left to read below"), "{}", app.status);
    let screen = draw(&mut app);
    let line = screen.lines().find(|l| l.contains("1 Bug 1 - shout two")).unwrap();
    assert!(line.contains(" 4/6│"), "the code read, its message not: {line}");
    // Enter takes the mark back.
    assert_eq!(app.rows[app.cursor].text, "-line 39", "where the last mark was made");
    keys(&mut app, &[KeyCode::Enter]);
    assert!(app.status.contains("unread: 2 lines"), "{}", app.status);
    assert!(draw(&mut app).contains("reading 2/6"));
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
fn a_patch_s_title_and_message_are_read_as_its_code_is() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    draw(&mut app);
    keys(&mut app, &[KeyCode::Char('2'), KeyCode::Enter]);
    app.wait_blocks();
    // Patch 1's message: its title, an empty line, `Because.`: two lines to
    // read, counted on its header as a file's are.
    let screen = draw(&mut app);
    assert!(screen.contains("✉ message   read 0/2 lines"), "{screen}");
    down_to(&mut app, "   Bug 1 - shout two");
    assert!(draw(&mut app).contains("○→✓    Bug 1 - shout two"), "what Space would read, marked");
    // The title, a paragraph of its own; then on, past the empty line.
    keys(&mut app, &[KeyCode::Char(' ')]);
    assert!(app.status.contains("read: the title, 1 line"), "{}", app.status);
    assert_eq!(app.rows[app.cursor].text, "   Because.");
    keys(&mut app, &[KeyCode::Char(' ')]);
    assert!(app.status.contains("read: line 3 of the message, 1 line"), "{}", app.status);
    // The code come up under the cursor, the message's rows say so.
    let screen = draw(&mut app);
    assert!(app.rows.iter().any(|r| r.text == "✉ message   read 2/2 lines"), "{:?}", app.rows.iter().map(|r| &r.text).collect::<Vec<_>>());
    assert!(app.rows.iter().any(|r| r.text == "   Because." && r.read));
    assert!(screen.contains("reading 2/6") && screen.contains(" 2/6"), "{screen}");
    // Enter on its header takes back the latest mark in it; Space there
    // reads it whole.
    keys(&mut app, &[KeyCode::Home]);
    assert!(app.rows[app.cursor].text.starts_with("✉ message"), "{}", app.rows[app.cursor].text);
    keys(&mut app, &[KeyCode::Enter]);
    assert!(app.status.contains("unread: 1 line"), "{}", app.status);
    keys(&mut app, &[KeyCode::Char(' ')]);
    assert!(app.status.contains("read: the message, 1 line"), "{}", app.status);
    draw(&mut app);
    assert!(app.rows.iter().any(|r| r.text == "✉ message   read 2/2 lines"));
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
    assert!(draw(&mut app).contains("reading 4/5"), "the title left");
}

#[test]
fn reading_on_the_cursor_goes_down_to_the_middle_then_the_code_comes_up() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    draw(&mut app);
    keys(&mut app, &[KeyCode::Char('2'), KeyCode::Enter]);
    app.wait_blocks();
    // The screen `h` lines high, and its middle line: the middle pane's,
    // under the top bar, over the bottom line.
    let draw_h = |app: &mut App, h: u16| {
        let mut buf = Buffer::empty(Rect::new(0, 0, 120, h));
        app.draw(&mut buf);
        let text = (0..h).map(|y| (0..120).map(|x| buf[(x, y)].symbol()).collect::<String>()).collect::<Vec<_>>().join("\n");
        (text, 1 + (h as usize - 2) / 2)
    };
    // The line of the screen a row is on, as what Space would read.
    let line_of = |screen: &str, text: &str| screen.lines().position(|l| l.contains(&format!("○→✓ {text}")));
    // A key pressed, the screen drawn `h` lines high after it, as it is
    // (`keys` draws it 30).
    let press = |app: &mut App, code: KeyCode, h: u16| {
        assert_eq!(key(app, code), Effect::None, "{code:?}");
        draw_h(app, h);
    };
    // The cursor down to +TWO, from the top.
    let to_two = |app: &mut App, h: u16| {
        press(app, KeyCode::Home, h);
        for _ in 0..100 {
            if app.rows[app.cursor].text == "+TWO" {
                return;
            }
            press(app, KeyCode::Down, h);
        }
        panic!("no +TWO");
    };
    // Its mark taken back, the cursor on it again.
    let unread = |app: &mut App, h: u16| {
        to_two(app, h);
        press(app, KeyCode::Enter, h);
        assert!(app.status.contains("unread"), "{}", app.status);
    };
    for view in [Layout::Unified, Layout::Split, Layout::Compact] {
        assert_eq!(app.layout, view);
        // Above the middle line: the cursor goes down to the next line not
        // read, where it is, the code staying where it is.
        to_two(&mut app, 30);
        let (screen, middle) = draw_h(&mut app, 30);
        let y = line_of(&screen, "+TWO").unwrap_or_else(|| panic!("{view:?}:\n{screen}"));
        assert!(y < middle, "{view:?}: above the middle:\n{screen}");
        let below = screen.lines().position(|l| l.contains("-line 39")).unwrap_or_else(|| panic!("{view:?}:\n{screen}"));
        assert!(below > y, "{view:?}: the next line not read, further down:\n{screen}");
        keys(&mut app, &[KeyCode::Char(' ')]);
        assert_eq!(app.rows[app.cursor].text, "-line 39", "{view:?}");
        let (screen, _) = draw_h(&mut app, 30);
        assert_eq!(line_of(&screen, "-line 39"), Some(below.min(middle)), "{view:?}: down to it:\n{screen}");
        assert!(screen.contains("  ✓ +TWO"), "{view:?}: what was read, where it was:\n{screen}");
        // On the middle line, or under it: the code comes up under the
        // cursor, which stays on its line.
        unread(&mut app, 10);
        let (screen, middle) = draw_h(&mut app, 10);
        let y = line_of(&screen, "+TWO").unwrap_or_else(|| panic!("{view:?}:\n{screen}"));
        assert!(y >= middle, "{view:?}: on the middle line or under it:\n{screen}");
        press(&mut app, KeyCode::Char(' '), 10);
        assert_eq!(app.rows[app.cursor].text, "-line 39", "{view:?}");
        let (screen, _) = draw_h(&mut app, 10);
        assert_eq!(line_of(&screen, "-line 39"), Some(y), "{view:?}: on the line the cursor was on:\n{screen}");
        // Read again from the start, in the next view.
        unread(&mut app, 30);
        keys(&mut app, &[KeyCode::Home, KeyCode::Char('1')]);
    }
}

#[test]
fn what_changed_in_a_line_almost_the_same_as_the_one_removed_is_painted_as_words() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir(&src).unwrap();
    git(&src, &["init", "-q", "-b", "main"]);
    git(&src, &["config", "commit.gpgsign", "false"]);
    std::fs::write(src.join("f.rs"), "fn f() {\n    let total = price * count;\n    old();\n    // the total of the order\n}\n").unwrap();
    git(&src, &["add", "."]);
    git(&src, &["commit", "-qm", "base"]);
    // A line almost the same, and one quite another.
    std::fs::write(src.join("f.rs"), "fn f() {\n    let total = price * quantity + tax;\n    something_else(1, 2, 3);\n    // the total of the basket\n}\n").unwrap();
    git(&src, &["commit", "-qam", "Bug 1 - the total"]);
    let db = tmp.path().join("db.jsonl");
    stacked(&src, &db, &["import", "s", "HEAD~1"]);
    let mut app = open(&db).unwrap();
    keys(&mut app, &[KeyCode::Enter]);
    let mut buf = Buffer::empty(Rect::new(0, 0, 120, 30));
    app.draw(&mut buf);
    let row = |text: &str| -> (u16, u16) {
        for y in 0..30 {
            let line: String = (0..120).map(|x| buf[(x, y)].symbol()).collect();
            if let Some(at) = line.find(text) {
                return (line[..at].chars().count() as u16, y);
            }
        }
        panic!("no {text}");
    };
    let style = |(x, y): (u16, u16), dx: u16| {
        let c = &buf[(x + dx, y)];
        (c.fg, c.bg, c.modifier)
    };
    let (word, removed, added, word_added) = (
        stacked::paint::color::del_word(),
        stacked::paint::color::del(),
        stacked::paint::color::add(),
        stacked::paint::color::add_word(),
    );
    let as_drawn = |s: ratatui::style::Style| (s.fg.unwrap_or_default(), s.bg.unwrap_or_default(), s.add_modifier);
    // The line removed: its count, as words; the rest as the line.
    let at = row("-    let total = price * count;");
    assert_eq!(style(at, 25), as_drawn(word), "count");
    assert_eq!(style(at, 5), as_drawn(removed), "let");
    // The line added: its quantity + tax, as words; the rest as the line.
    let at = row("+    let total = price * quantity + tax;");
    assert_eq!(style(at, 25), as_drawn(word_added), "quantity");
    assert_eq!(style(at, 35), as_drawn(word_added), "tax");
    assert_eq!(style(at, 5), as_drawn(added), "let");
    // Lines quite other than each other: the lines alone.
    let at = row("+    something_else(1, 2, 3);");
    assert_eq!(style(at, 5), as_drawn(added));
    let at = row("-    old();");
    assert_eq!(style(at, 5), as_drawn(removed));
    // A word changed last in its line: the room after it, the line's.
    let at = row("+    // the total of the basket");
    assert_eq!(style(at, 26), as_drawn(word_added), "basket");
    assert_eq!(style(at, 31), as_drawn(added), "after basket");
    // The cursor on each of the lines: the rest of the line is in the
    // cursor's colours, and what changed in it is in its own, which look
    // otherwise than the cursor's, as shown.
    let cursor = as_drawn(stacked::paint::on_cursor(ratatui::style::Style::new()));
    for (text, word) in [("-    let total = price * count;", word), ("+    let total = price * quantity + tax;", word_added)] {
        down_to(&mut app, text);
        let mut buf = Buffer::empty(Rect::new(0, 0, 120, 30));
        app.draw(&mut buf);
        let (x, y) = (0..30u16)
            .find_map(|y| {
                let line: String = (0..120).map(|x| buf[(x, y)].symbol()).collect();
                line.find(text).map(|at| (line[..at].chars().count() as u16, y))
            })
            .unwrap_or_else(|| panic!("no {text}"));
        let cell = |dx: u16| &buf[(x + dx, y)];
        let drawn = |dx: u16| (cell(dx).fg, cell(dx).bg, cell(dx).modifier);
        assert_eq!(drawn(5), cursor, "the cursor is on it: {text}");
        assert_eq!(drawn(25), as_drawn(word), "the word in its own colours: {text}");
        assert_ne!(looks(cell(25)), looks(cell(5)), "the word stands out under the cursor: {text}");
    }
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
    assert!(draw(&mut app).contains("3/3 lines to review"));
    keys(&mut app, &[KeyCode::Char('2'), KeyCode::Enter]);
    app.wait_blocks();
    let screen = draw(&mut app);
    assert!(screen.contains("reading 0/3") && screen.contains("read 0/2 lines") && screen.contains(" 0/3"), "{screen}");
    // The title read first, Space goes on to the code.
    down_to(&mut app, "   Bug 1 - more");
    keys(&mut app, &[KeyCode::Char(' ')]);
    assert!(app.status.contains("read: the title, 1 line"), "{}", app.status);
    assert_eq!(app.rows[app.cursor].text, "+two");
    // Space reads on to what is left, never stopping at the empty line or
    // the blank one, until nothing is.
    for _ in 0..4 {
        keys(&mut app, &[KeyCode::Char(' ')]);
        assert!(!["+", "+  "].contains(&app.rows[app.cursor].text.as_str()), "{}", app.status);
        if app.status.contains("nothing left to read below") {
            break;
        }
    }
    assert!(app.status.contains("nothing left to read below"), "{}", app.status);
    let screen = draw(&mut app);
    assert!(screen.contains("reading 3/3") && screen.contains("nothing left to review"), "{screen}");
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
    assert!(screen.contains(&format!("reading {read}/5")), "{screen}");
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
fn what_changed_between_versions_is_painted_as_words_in_interdiffs_and_stack_diffs() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    // A second version of patch 1: its title reworded.
    Db::edit(&db, |d| {
        let s = d.stack("s")?.clone();
        let mut draft = stacked::mbox::Draft::from(d.patch(&s.patches[0]));
        draft.message = "Bug 1 - shout two, reworded".into();
        stacked::ops::replace(d, "s", 0, draft, "reword").map(|_| ())
    })
    .unwrap();
    app.refresh();
    // The style of the cell `dx` after where `text` is on the screen.
    let style_of = |app: &mut App, text: &str, dx: usize| {
        let mut buf = Buffer::empty(Rect::new(0, 0, 120, 30));
        app.draw(&mut buf);
        for y in 0..30 {
            let line: String = (0..120).map(|x| buf[(x, y)].symbol()).collect();
            if let Some(at) = line.find(text) {
                let c = &buf[((line[..at].chars().count() + dx) as u16, y)];
                return (c.fg, c.bg, c.modifier);
            }
        }
        panic!("no {text}");
    };
    let as_drawn = |s: ratatui::style::Style| (s.fg.unwrap_or_default(), s.bg.unwrap_or_default(), s.add_modifier);
    let (word, line) = (stacked::paint::color::add_word(), stacked::paint::color::add());
    let added = "+Subject: Bug 1 - shout two, reworded";
    let reworded = added.find("reworded").unwrap();
    // The interdiff of the new version: the words put in, as words; the rest
    // of the line, as added.
    choose(&mut app, "Versions of this patch…");
    assert_eq!(app.screen, Screen::Versions);
    assert_eq!(style_of(&mut app, added, reworded), as_drawn(word), "reworded");
    assert_eq!(style_of(&mut app, added, 1), as_drawn(line), "Subject");
    // The stack diff of the change, in the history: the same.
    keys(&mut app, &[KeyCode::Esc, KeyCode::Home]);
    choose(&mut app, "History…");
    assert_eq!(app.screen, Screen::History);
    assert_eq!(style_of(&mut app, added, reworded), as_drawn(word), "reworded, in the history");
    assert_eq!(style_of(&mut app, added, 1), as_drawn(line), "Subject, in the history");
    // The cursor on that line of the stack diff: the line is in the
    // cursor's colours, and the words put in are still in their own.
    keys(&mut app, &[KeyCode::Right]);
    let text = |parts: &stacked::tui::Parts| parts.iter().map(|(t, _)| t.as_str()).collect::<String>();
    let k = app.hist_rows.iter().position(|parts| text(parts).contains(added)).unwrap();
    while app.hist_cursor < k {
        keys(&mut app, &[KeyCode::Down]);
    }
    let cursor = as_drawn(stacked::paint::on_cursor(ratatui::style::Style::new()));
    assert_eq!(style_of(&mut app, added, reworded), as_drawn(word), "reworded, under the cursor");
    assert_eq!(style_of(&mut app, added, 1), cursor, "Subject, under the cursor");
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
    assert_eq!(items(&app)[0].0, "Save it and ask agent, answer only");
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
    // With no base, its files cannot be read whole: its hunks, the status
    // and a line over them saying why.
    keys(&mut app, &[KeyCode::Char('3')]);
    assert_eq!(app.status, "the files of this patch could not be read whole: the stack s has no base commit to read them on");
    let screen = draw(&mut app);
    let said: Vec<&str> = app.rows.iter().filter(|r| r.kind == stacked::rows::Kind::Meta && r.diff_at.is_none()).map(|r| r.text.trim()).collect();
    assert_eq!(said.join(" "), "The files could not be read whole, so here are their hunks: the stack s has no base commit to read them on.");
    assert!(screen.contains("so here are their") && screen.contains("@@ -0,0 +1 @@"), "{screen}");
    assert_eq!(app.rows[app.cursor].text, "+one", "{screen}");
}

#[test]
fn an_empty_database() {
    let tmp = tempfile::tempdir().unwrap();
    let mut app = open(&tmp.path().join("none.jsonl")).unwrap();
    let screen = draw(&mut app);
    assert!(screen.contains("no stack yet: Space for the") && screen.contains("ways to make one"), "{screen}");
    for c in [KeyCode::Down, KeyCode::Up, KeyCode::Left, KeyCode::Right, KeyCode::Home, KeyCode::End] {
        assert_eq!(key(&mut app, c), Effect::None);
        draw(&mut app);
    }
    // Nothing for `?` to say: no help opens, and the bottom line offers
    // none.
    assert_eq!(key(&mut app, KeyCode::Char('?')), Effect::None);
    assert_eq!(app.mode, Mode::Normal);
    assert_eq!(app.hint(), "enter or space: menu · 1: view · 2: read · 3: whole");
    assert!(!draw(&mut app).contains("what can I do here"));
    // Enter, with nothing else to do, is the menu: the ways to make a
    // stack, or quit.
    keys(&mut app, &[KeyCode::Enter]);
    let labels: Vec<String> = items(&app).into_iter().map(|(l, _)| l).collect();
    let mut want = vec!["Ask the agent to make a stack…", "Import from the workspace…"];
    if cfg!(feature = "moz-phab") {
        want.push("Import from Phabricator…");
    }
    assert_eq!(labels, [&want[..], &["", "Quit"]].concat());
    assert_eq!(choose(&mut app, "Quit"), Effect::Quit);
}

/// A repository: main, a file; `other` on it, a commit; `feature` on it,
/// main its upstream, two commits, then changes staged and more not.
fn workspace(at: &Path) {
    git(at, &["init", "-q", "-b", "main"]);
    git(at, &["config", "commit.gpgsign", "false"]);
    std::fs::write(at.join("a.txt"), "one\n").unwrap();
    git(at, &["add", "."]);
    git(at, &["commit", "-qm", "base"]);
    git(at, &["checkout", "-qb", "other"]);
    std::fs::write(at.join("b.txt"), "bee\n").unwrap();
    git(at, &["add", "."]);
    git(at, &["commit", "-qm", "Bug 3 - add b"]);
    git(at, &["checkout", "-q", "main"]);
    git(at, &["checkout", "-qb", "feature"]);
    git(at, &["branch", "-q", "--set-upstream-to=main"]);
    std::fs::write(at.join("a.txt"), "one\ntwo\n").unwrap();
    git(at, &["commit", "-qam", "Bug 1 - add two"]);
    std::fs::write(at.join("a.txt"), "one\ntwo\nthree\n").unwrap();
    git(at, &["commit", "-qam", "Bug 2 - add three"]);
    std::fs::write(at.join("a.txt"), "one\ntwo\nthree\nfour\n").unwrap();
    git(at, &["add", "a.txt"]);
    std::fs::write(at.join("a.txt"), "one\ntwo\nthree\nfour\nfive\n").unwrap();
}

#[test]
fn the_agent_is_asked_to_make_a_stack_from_a_comment_on_it_empty() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir(&src).unwrap();
    workspace(&src);
    let db = tmp.path().join("db.jsonl");
    let mut app = open(&db).unwrap();
    app.checkout = src.clone();
    // Named after the branch, as no stack is; made empty, on HEAD.
    keys(&mut app, &[KeyCode::Char(' ')]);
    choose(&mut app, "Ask the agent to make a stack…");
    assert!(matches!(&app.mode, Mode::Prompt(p) if p.ok && p.ed.text() == "feature" && p.said == "the stack feature"), "{:?}", app.mode);
    assert_eq!(app.hint(), "enter: make it, empty, then say what it should hold · esc: back");
    keys(&mut app, &[KeyCode::Enter]);
    assert_eq!(app.stack, "feature");
    let d = Db::load(&db).unwrap();
    let s = d.stack("feature").unwrap();
    let head = String::from_utf8(Command::new("git").args(["rev-parse", "HEAD"]).current_dir(&src).output().unwrap().stdout).unwrap();
    assert!(s.patches.is_empty() && s.base.as_deref() == Some(head.trim()), "{s:?}");
    drop(d);
    // A comment on it, open to say what the agent should make of it; saved,
    // the agent asked.
    assert!(matches!(&app.mode, Mode::Edit(e) if e.target == stacked::tui::Target::Stack), "{:?}", app.mode);
    assert!(app.status.starts_with("the stack feature is made, empty, on HEAD "), "{}", app.status);
    typed(&mut app, "Make a stack of two patches: four, then five.");
    keys(&mut app, &[KeyCode::Esc]);
    choose(&mut app, "Save it and ask agent");
    assert!(app.status.contains("asked the agent about 1 comment"), "{}", app.status);
    wait_for(&mut app, "⟳ 1 landed ask waits for your review");
}

#[cfg(feature = "moz-phab")]
#[test]
fn a_stack_is_imported_from_phabricator_seen_first() {
    use stacked::phab::Revision;
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir(&src).unwrap();
    workspace(&src);
    let main = String::from_utf8(Command::new("git").args(["rev-parse", "main"]).current_dir(&src).output().unwrap().stdout).unwrap();
    let main = main.trim().to_string();
    let db = tmp.path().join("db.jsonl");
    let mut app = open(&db).unwrap();
    app.checkout = src.clone();
    // Phabricator as the tests have it: D11 on main, D12 on it; read from
    // the top, or with D11's base one the repository does not have.
    let on = std::sync::Arc::new(std::sync::Mutex::new(main.clone()));
    let base = on.clone();
    stacked::phab::stack_stand_in(Some(Box::new(move |rev, link| {
        if rev != "D12" || link != Some("https://phabricator.example.com/D12") {
            return Err(format!("not as the test reads it: {rev} {link:?}"));
        }
        let trailer = |n: u32| format!("\n\nDifferential Revision: https://phabricator.example.com/D{n}");
        Ok(vec![
            Revision {
                id: "D11".into(),
                message: format!("Bug 1 - add two{}", trailer(11)),
                author: Some("Ann <ann@example.com>".into()),
                diff: "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1 +1,2 @@\n one\n+two\n".into(),
                base: Some(base.lock().unwrap().clone()),
            },
            Revision {
                id: "D12".into(),
                message: format!("Bug 2 - add three{}", trailer(12)),
                author: Some("Ann <ann@example.com>".into()),
                diff: "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1,2 +1,3 @@\n one\n two\n+three\n".into(),
                base: None,
            },
        ])
    })));
    let labels = |app: &App| items(app).into_iter().map(|(l, _)| l).collect::<Vec<_>>();
    // The revision, typed: said as it is, taken once it names one.
    keys(&mut app, &[KeyCode::Char(' ')]);
    choose(&mut app, "Import from Phabricator…");
    assert!(matches!(&app.mode, Mode::Prompt(p) if !p.ok && p.said == "give a revision: D123, or its link"), "{:?}", app.mode);
    typed(&mut app, "D1x");
    keys(&mut app, &[KeyCode::Enter]);
    assert!(matches!(app.mode, Mode::Prompt(_)), "not taken");
    for _ in 0..3 {
        key(&mut app, KeyCode::Backspace);
    }
    typed(&mut app, "https://phabricator.example.com/D12");
    assert!(matches!(&app.mode, Mode::Prompt(p) if p.ok && p.said == "D12, and the revisions it stacks on"), "{:?}", app.mode);
    // Read away from the screen, then seen before it is imported.
    keys(&mut app, &[KeyCode::Enter]);
    assert_eq!(app.status, "reading D12 and the revisions it stacks on, from Phabricator…");
    app.settle();
    let Mode::Menu(m) = &app.mode else { panic!("{:?} {}", app.mode, app.status) };
    assert_eq!(m.title, format!("2 patches from D11..D12, on {}", &main[..12]));
    assert_eq!(labels(&app), ["1 Bug 1 - add two", "2 Bug 2 - add three", "", "Import it as D12", "Name it otherwise…"]);
    choose(&mut app, "Import it as D12");
    assert_eq!(app.stack, "D12");
    let d = Db::load(&db).unwrap();
    let ps: Vec<stacked::db::Patch> = d.stack("D12").unwrap().patches.iter().map(|id| d.patch(id).clone()).collect();
    assert_eq!(ps.iter().map(|p| p.title().to_string()).collect::<Vec<_>>(), ["Bug 1 - add two", "Bug 2 - add three"]);
    assert_eq!(ps[0].author.as_deref(), Some("Ann <ann@example.com>"));
    assert!(ps[1].message.contains("Differential Revision: https://phabricator.example.com/D12"), "{}", ps[1].message);
    drop(d);
    // On a commit the repository does not have: said, nothing imported.
    *on.lock().unwrap() = "0123456789abcdef0123456789abcdef01234567".into();
    keys(&mut app, &[KeyCode::Home]);
    choose(&mut app, "New stack…");
    choose(&mut app, "Import from Phabricator…");
    typed(&mut app, "https://phabricator.example.com/D12");
    keys(&mut app, &[KeyCode::Enter]);
    app.settle();
    assert_eq!(app.status, "D11 is on 0123456789ab, which this repository does not have: fetch it first");
    assert_eq!(app.mode, Mode::Normal);
    stacked::phab::stack_stand_in(None);
}

#[test]
fn a_stack_is_imported_from_the_workspace_seen_first() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir(&src).unwrap();
    workspace(&src);
    let db = tmp.path().join("db.jsonl");
    let mut app = open(&db).unwrap();
    app.checkout = src.clone();
    let labels = |app: &App| items(app).into_iter().map(|(l, _)| l).collect::<Vec<_>>();
    let titles = |name: &str| {
        let d = Db::load(&db).unwrap();
        d.stack(name).unwrap().patches.iter().map(|id| d.patch(id).title().to_string()).collect::<Vec<_>>()
    };
    let erase = |app: &mut App, n: usize| {
        for _ in 0..n {
            key(app, KeyCode::Backspace);
        }
    };
    // The branch checked out, on its upstream; the changes not committed,
    // each checked to go on top or not.
    keys(&mut app, &[KeyCode::Char(' ')]);
    choose(&mut app, "Import from the workspace…");
    assert_eq!(
        labels(&app),
        [
            "feature on main: 2 commits",
            "Another branch…",
            "A range…",
            "",
            "With the staged changes on top, a patch of their own",
            "With the unstaged changes on top, a patch of their own"
        ]
    );
    choose(&mut app, "With the staged changes");
    let Mode::Menu(m) = &app.mode else { panic!("{:?}", app.mode) };
    assert_eq!(m.items.iter().map(|i| i.check).collect::<Vec<_>>()[4..], [Some(true), Some(false)]);
    // Seen before it is imported: its patches, on what, and the name of its
    // stack, which may be another.
    choose(&mut app, "feature on main: 2 commits");
    let Mode::Menu(m) = &app.mode else { panic!("{:?}", app.mode) };
    assert!(m.title.starts_with("3 patches from main..HEAD, on "), "{}", m.title);
    assert_eq!(labels(&app), ["1 Bug 1 - add two", "2 Bug 2 - add three", "3 Staged changes", "", "Import it as feature", "Name it otherwise…"]);
    choose(&mut app, "Name it otherwise…");
    erase(&mut app, 7);
    typed(&mut app, "a b");
    assert!(matches!(&app.mode, Mode::Prompt(p) if !p.ok && p.said.contains("cannot name a stack")), "{:?}", app.mode);
    assert_eq!(app.hint(), "esc: back");
    keys(&mut app, &[KeyCode::Enter]);
    assert!(matches!(app.mode, Mode::Prompt(_)), "not taken");
    erase(&mut app, 3);
    typed(&mut app, "fix");
    assert_eq!(app.hint(), "enter: name it so · esc: back");
    keys(&mut app, &[KeyCode::Enter]);
    choose(&mut app, "Import it as fix");
    assert_eq!(app.stack, "fix");
    assert_eq!(app.status, "imported 3 patches from main..HEAD as fix");
    assert_eq!(titles("fix"), ["Bug 1 - add two", "Bug 2 - add three", "Staged changes"]);
    // A range given by hand: said as it is typed, taken once it holds a
    // commit; another branch's, without the changes of the one checked out.
    keys(&mut app, &[KeyCode::Home]);
    assert!(app.focus == Focus::List && app.on_stack, "{:?}", app.focus);
    choose(&mut app, "New stack…");
    let mut want = vec!["Ask the agent to make a stack…", "Import from the workspace…"];
    if cfg!(feature = "moz-phab") {
        want.push("Import from Phabricator…");
    }
    assert_eq!(labels(&app), want);
    choose(&mut app, "Import from the workspace…");
    choose(&mut app, "A range…");
    assert!(matches!(&app.mode, Mode::Prompt(p) if !p.ok && p.said == "give a commit, or a base..tip range"), "{:?}", app.mode);
    typed(&mut app, "nope");
    assert!(matches!(&app.mode, Mode::Prompt(p) if p.said == "unknown ref"), "{:?}", app.mode);
    keys(&mut app, &[KeyCode::Enter]);
    assert_eq!(app.status, "unknown ref");
    erase(&mut app, 4);
    typed(&mut app, "main..other");
    assert!(matches!(&app.mode, Mode::Prompt(p) if p.ok && p.said == "1 commit"), "{:?}", app.mode);
    assert!(draw(&mut app).contains("a range of the workspace, to import"));
    keys(&mut app, &[KeyCode::Enter]);
    assert_eq!(labels(&app), ["1 Bug 3 - add b", "", "Import it as other", "Name it otherwise…"]);
    choose(&mut app, "Import it as other");
    assert_eq!(titles("other"), ["Bug 3 - add b"]);
    // Another branch, picked: each on what it is on; its stack named as no
    // other is.
    keys(&mut app, &[KeyCode::Home]);
    assert!(app.focus == Focus::List && app.on_stack, "{:?}", app.focus);
    choose(&mut app, "New stack…");
    choose(&mut app, "Import from the workspace…");
    choose(&mut app, "Another branch…");
    assert_eq!(items(&app), [("main, on nothing".to_string(), false), ("other on main: 1 commit".to_string(), true)]);
    choose(&mut app, "other on main: 1 commit");
    choose(&mut app, "Import it as other-2");
    assert_eq!(app.stack, "other-2");
    assert_eq!(titles("other-2"), ["Bug 3 - add b"]);
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
fn the_regions_drawn_are_the_panes_then_the_boxes_over_them() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    let screen = draw(&mut app);
    let rows: Vec<&str> = screen.lines().collect();
    let r = app.regions();
    // The body, under the top bar and over the bottom line, then its panes
    // side by side, a line drawn between each.
    assert_eq!((r[0].x, r[0].width, r[0].bottom()), (0, 120, 29), "{r:?}");
    let panes = &r[1..];
    assert!(panes.len() >= 2, "the patches' and the code's at least: {r:?}");
    for two in panes.windows(2) {
        assert_eq!(two[0].right() + 1, two[1].x, "{r:?}");
        assert_eq!(rows[two[0].y as usize].chars().nth(two[0].right() as usize), Some('│'), "{r:?}\n{screen}");
    }
    // A menu open over them: its box, inside its border, last.
    keys(&mut app, &[KeyCode::Char(' ')]);
    let screen = draw(&mut app);
    let menu = *app.regions().last().unwrap();
    let inside: Vec<String> = screen
        .lines()
        .skip(menu.y as usize)
        .take(menu.height as usize)
        .map(|l| l.chars().skip(menu.x as usize).take(menu.width as usize).collect())
        .collect();
    assert!(inside.iter().any(|l| l.contains("Read this patch")), "{inside:?}\n{screen}");
    // Its border not in it (the lines between its parts are).
    assert!(!inside.iter().any(|l| l.contains(['│', '┌', '┐', '└', '┘'])), "{inside:?}");
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
    // Nothing read yet: the 5 lines the patches change, and the 3 of their
    // messages, all 8.
    let bar = first(&mut app, 120);
    assert!(bar.ends_with("   8/8 lines to review "), "{bar}");
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
    // 2 lines read: 6 left of the 8.
    Db::append(&db, &[read(&one, 2, t0 + 600)]).unwrap();
    app.refresh();
    let bar = first(&mut app, 120);
    assert!(bar.ends_with("   6/8 lines to review "), "{bar}");
    // A line more, of the other patch.
    Db::append(&db, &[read(&two, 1, t0 + 1200)]).unwrap();
    app.refresh();
    let bar = first(&mut app, 120);
    assert!(bar.ends_with("   5/8 lines to review "), "{bar}");
    // A comment open.
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "why so loud?");
    let bar = first(&mut app, 120);
    assert!(bar.ends_with("   5/8 lines to review · 1 comment to close "), "{bar}");
    // With less room, less of it; with none, none.
    let bar = first(&mut app, 80);
    assert!(bar.ends_with("   5/8 lines · 1 comment ") && !bar.contains("to review"), "{bar}");
    let bar = first(&mut app, 50);
    assert!(!bar.contains("5/8 lines"), "{bar}");
    // All read, the messages too, the comment closed: nothing left.
    Db::append(&db, &[read(&one, usize::MAX, t0 + 1800), read(&two, usize::MAX, t0 + 1800)]).unwrap();
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
    assert_eq!(all, ["Save it", "Save it and ask agent later", "Send it", "Attach a reference…", "", "Throw it away", "Keep writing"]);
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
    // under the ask asking as the ask does.
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
    // diff has, and lines it has not; and at b.txt whole, as patch 2 leaves
    // it.
    Db::edit(&db, |d| {
        let n = d.note(&n.change, &n.id).cloned().unwrap();
        let said = "It shouts there.\n\n```refs\na.txt:2 @1\na.txt:20-21 @1\nb.txt @2\n```";
        d.set_note(Note {
            text: append_turn(&n.text, Role::Agent, said, Some("answer")),
            ..n
        })
    })
    .unwrap();
    app.refresh();
    let screen = draw(&mut app);
    assert!(screen.contains("⇢ a.txt:2 · as patch 1 leaves it") && screen.contains("⇢ a.txt:20-21 · as patch 1 leaves it"), "{screen}");
    assert!(screen.contains("⇢ b.txt · as patch 2 leaves it"), "{screen}");
    assert!(screen.contains("It shouts there.") && !screen.contains("```refs") && !screen.contains("a.txt:2 @1"), "{screen}");
    // Enter on the first: its line, previewed.
    down_to(&mut app, "     ▎   ⇢ a.txt:2 ");
    keys(&mut app, &[KeyCode::Enter]);
    let Mode::Preview(p) = &app.mode else { panic!("{:?}", app.mode) };
    assert_eq!(p.title, "a.txt:2 · as patch 1 leaves it");
    assert_eq!(p.texts(), ["    2 │ TWO"]);
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
    assert_eq!(p.texts(), ["   20 │ line 17", "   21 │ line 18"]);
    // The numbers of the lines it points at marked, as everywhere.
    use ratatui::style::Modifier;
    let marked = |parts: &stacked::tui::Parts| parts[0].1.add_modifier.contains(Modifier::REVERSED) && !parts[1].1.add_modifier.contains(Modifier::REVERSED);
    assert!(p.lines.iter().all(marked), "{:?}", p.lines);
    keys(&mut app, &[KeyCode::Esc]);
    assert!(matches!(app.mode, Mode::Normal), "{:?}", app.mode);
    keys(&mut app, &[KeyCode::Enter, KeyCode::Enter]);
    assert_eq!(app.screen, Screen::Text);
    let t = app.text.as_ref().unwrap();
    assert_eq!(t.title, "a.txt, as patch 1 leaves it");
    assert_eq!(t.texts()[19], "   20 │ line 17");
    assert_eq!(t.scroll, 16);
    // The file whole, the numbers of the lines it points at marked, the
    // lines around them not.
    assert!(marked(&t.lines[19]) && marked(&t.lines[20]), "{:?}", &t.lines[19..21]);
    assert!(!marked(&t.lines[18]) && !marked(&t.lines[21]), "{:?} {:?}", t.lines[18], t.lines[21]);
    // As drawn: the number reversed, what the line says not.
    let mut buf = Buffer::empty(Rect::new(0, 0, 120, 30));
    app.draw(&mut buf);
    let screen = (0..30).map(|y| (0..120).map(|x| buf[(x, y)].symbol()).collect::<String>()).collect::<Vec<_>>().join("\n");
    let reversed = |x: u16, y: u16| buf[(x, y)].modifier.contains(Modifier::REVERSED);
    let (x, y) = at(&screen, "   20 │ line 17");
    assert!(reversed(x + 3, y) && reversed(x + 4, y) && !reversed(x + 8, y), "{screen}");
    let (x, y) = at(&screen, "   19 │ line 16");
    assert!(!reversed(x + 3, y), "{screen}");
    // A file whole: every line of it previewed, the numbers plain as it
    // points at them all; Enter again, its first line in the diff of the
    // patch it is of.
    keys(&mut app, &[KeyCode::Esc]);
    down_to(&mut app, "     ▎   ⇢ b.txt");
    keys(&mut app, &[KeyCode::Enter]);
    let Mode::Preview(p) = &app.mode else { panic!("{:?}", app.mode) };
    assert_eq!((p.title.as_str(), p.texts()), ("b.txt · as patch 2 leaves it", vec!["    1 │ bee".to_string()]));
    assert!(!p.lines.iter().any(marked), "{:?}", p.lines);
    keys(&mut app, &[KeyCode::Enter]);
    assert!(matches!(app.mode, Mode::Normal), "{:?}", app.mode);
    assert!(app.rows[app.cursor].text.starts_with("+bee"), "{}", app.rows[app.cursor].text);
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
fn reviewing_with_head(prepare: impl FnOnce(&Path)) -> (tempfile::TempDir, PathBuf, PathBuf, Editor) {
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
fn landed_kinds(kinds: &[(&str, &str)], build: bool) -> (tempfile::TempDir, PathBuf, Editor) {
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
fn the_cursor_looks_the_same_on_every_line_whatever_its_colours() {
    use ratatui::buffer::Cell;
    use ratatui::style::{Modifier, Style};
    use stacked::paint::color;
    use stacked::rows::{Kind, Row};
    let (_tmp, db, _src) = setup();
    let mut app = open(&db).unwrap();
    // The colours a cell painted in `st` shows.
    let colours = |st: Style| {
        let mut c = Cell::default();
        c.set_style(st);
        let (fg, bg, _) = looks(&c);
        (fg, bg)
    };
    // As the cursor paints when the settings do not say: the terminal's
    // own colours, reversed.
    let cursor = colours(color::cursor());
    assert_eq!(cursor, (Shown::Bg, Shown::Fg));
    // Each cell of `cells`, of `what`, is in the cursor's colours, and not
    // dim.
    let under = |cells: &[Cell], what: &str| {
        assert!(cells.len() > 10, "{what}: {cells:?}");
        for (k, c) in cells.iter().enumerate() {
            let (fg, bg, m) = looks(c);
            assert!((fg, bg) == cursor && !m.contains(Modifier::DIM), "{what}, its cell {k}: {c:?}");
        }
    };
    // The first patch's row, the pane's 30 columns.
    let patch = |app: &mut App| -> Vec<Cell> {
        let (_, y) = at(&draw(app), " 1 Bug 1 - shout two");
        let mut buf = Buffer::empty(Rect::new(0, 0, 120, 30));
        app.draw(&mut buf);
        (0..30).map(|x| buf[(x, y)].clone()).collect()
    };
    under(&patch(&mut app), "the patch picked, the cursor in its pane");
    // The middle's line the cursor is on: from where its row's text starts,
    // past the reading mode's margin, to the pane's edge.
    fn cursor_line(app: &mut App) -> Vec<Cell> {
        let text = app.rows[app.cursor].text.clone();
        let mut buf = Buffer::empty(Rect::new(0, 0, 120, 30));
        app.draw(&mut buf);
        let (x, y) = (0..30)
            .find_map(|y| {
                let line: String = (31..120).map(|x| buf[(x, y)].symbol()).collect();
                let at = line.find(&text).map(|b| line[..b].chars().count()).filter(|&n| n <= 4)?;
                Some((31 + at as u16, y))
            })
            .unwrap_or_else(|| panic!("no {text:?} at the start of the middle"));
        (x..120).map(|x| buf[(x, y)].clone()).take_while(|c| c.symbol() != "│").collect()
    }
    // The cursor on the first row `is` says.
    fn go(app: &mut App, what: &str, is: impl Fn(&Row) -> bool) {
        let i = app.rows.iter().position(is).unwrap_or_else(|| panic!("no {what}"));
        for _ in 0..app.rows.len() * 2 {
            if app.cursor == i {
                return;
            }
            key(app, if app.cursor < i { KeyCode::Down } else { KeyCode::Up });
            draw(app);
        }
        panic!("the cursor never on {what}");
    }
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "why so loud?");
    // An added line, a removed one, a hunk's header, a file's name and each
    // line of a comment are in the cursor's colours under it, whatever their
    // own.
    type Is = fn(&Row) -> bool;
    let lines: [(&str, Is); 7] = [
        ("an added line", |r| r.kind == Kind::Add),
        ("a removed line", |r| r.kind == Kind::Del),
        ("a hunk's header", |r| r.kind == Kind::Hunk),
        ("a file's name", |r| r.kind == Kind::File && r.text == "a.txt"),
        ("a comment's head", |r| r.kind == Kind::Comment && r.head),
        ("who said a comment's turn", |r| r.kind == Kind::Comment && r.who),
        ("what a comment's turn says", |r| r.kind == Kind::Comment && r.text.ends_with("why so loud?")),
    ];
    for (what, is) in lines {
        go(&mut app, what, is);
        under(&cursor_line(&mut app), what);
    }
    // Elsewhere, the patches' pick is as `picked` paints it, over the
    // line's own colours: not the cursor's.
    let picked = colours(Style::new().patch(color::picked()));
    assert_ne!(picked, cursor);
    for c in patch(&mut app) {
        let (fg, bg, _) = looks(&c);
        assert_eq!((fg, bg), picked, "the patch picked, the cursor in the middle: {c:?}");
    }
    // A line read, dim, looks under the cursor as any other line does.
    keys(&mut app, &[KeyCode::Char('2')]);
    app.wait_blocks();
    go(&mut app, "the line added", |r| r.text == "+TWO");
    keys(&mut app, &[KeyCode::Char(' ')]);
    go(&mut app, "a line read", |r| r.text == "+TWO" && r.read);
    under(&cursor_line(&mut app), "a line read");
    // So does a line in no hunk, each file shown whole, dim.
    keys(&mut app, &[KeyCode::Char('3')]);
    app.wait_files();
    draw(&mut app);
    go(&mut app, "a line in no hunk", |r| r.outside);
    under(&cursor_line(&mut app), "a line in no hunk");
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
    // Listing files: the title as expanded, and its file under it, further
    // in, with the lines the patch adds and removes in it.
    keys(&mut app, &[KeyCode::Left]);
    let (expanded, files) = (lines, pane(&mut app));
    assert_eq!(files[..expanded.len()], expanded[..], "{files:?}");
    assert_eq!(files.len(), expanded.len() + 1, "{files:?}");
    let file = files.last().unwrap();
    assert!(file.starts_with("      a.txt ") && file.ends_with(" +1 −1"), "{files:?}");
}

/// The stand-in for Phabricator, in the test of a pull: D11 holds ann's
/// comment; D12 none, and waits to be let go, then fails if it is told to.
#[cfg(feature = "moz-phab")]
static LET_GO: std::sync::Mutex<bool> = std::sync::Mutex::new(false);
#[cfg(feature = "moz-phab")]
static GONE_ON: std::sync::Condvar = std::sync::Condvar::new();
#[cfg(feature = "moz-phab")]
static FAIL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg(feature = "moz-phab")]
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

#[cfg(feature = "moz-phab")]
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

#[cfg(feature = "moz-phab")]
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
