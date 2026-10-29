//! The try push, built with the `try-push` feature: a comment asked as a
//! try push, the agent's stand-in selecting its tasks, each with why; the
//! tasks landing for review, marked, answered and sent back; and closing
//! the review with them. The editor is drawn into a buffer and driven by
//! keys, as in `tests/tui.rs`; no test reaches an agent, mach, Lando or
//! Treeherder.

#![cfg(feature = "try-push")]

use std::path::{Path, PathBuf};
use std::process::Command;

use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;

use stacked::db::{Db, Note};
use stacked::tui::{App, Effect, Mode, Screen};

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

/// stacked run in `dir` on the database `db`, as `tests/tui.rs` runs it:
/// its own things kept in `home` beside the database, what it writes in
/// the temporary directory in `tmp` beside it.
fn stacked(dir: &Path, db: &Path, args: &[&str]) {
    stacked_with(dir, db, args, &[]);
}

/// As `stacked`, with `env` besides; what it printed.
fn stacked_with(dir: &Path, db: &Path, args: &[&str], env: &[(&str, &Path)]) -> String {
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
        .envs(env.iter().copied())
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// A database holding stack `s` of two patches, and the repository.
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
    stacked(&src, &db, &["import", "s", "HEAD~2"]);
    (tmp, db, src)
}

/// The settings of these tests: the defaults, with the setup of what each
/// kind of ask may use done, as a user's are once it is saved.
fn settings() -> stacked::config::Config {
    let mut c = stacked::config::Config::default();
    c.agent.setup = Some(stacked::setup::VERSION);
    c
}

/// The editor on `db`, with the tests' settings and the stand-ins for
/// claude and for the program that opens links.
fn open(db: &Path) -> Editor {
    stacked::config::init(settings());
    fake_claude();
    fake_opener();
    Editor(App::with_config(db, settings()).unwrap())
}

/// The editor a test opened: once it goes, it waits for the asks the test
/// started to be over, before the test's folder is removed.
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

impl Drop for Editor {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            asks_over(&mut self.0);
        }
    }
}

/// Waits for the work the editor started away from the screen, and for no
/// process to be at work in the folder of its database any more.
fn asks_over(app: &mut App) {
    app.settle();
    let given = app.path.parent().map(Path::to_path_buf).unwrap_or_default();
    let Ok(real) = given.canonicalize() else { return };
    let inside: Vec<String> = [&given, &real].iter().filter(|d| d.is_absolute()).map(|d| format!("{}/", d.display())).collect();
    let mut working = String::new();
    for _ in 0..300 {
        let Ok(ps) = Command::new("ps").args(["axww", "-o", "command="]).output() else { break };
        working = String::from_utf8_lossy(&ps.stdout).lines().filter(|l| inside.iter().any(|i| l.contains(i.as_str()))).collect::<Vec<_>>().join("\n");
        if working.is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert!(working.is_empty(), "still at work in {}:\n{working}", real.display());
}

/// The stand-in for claude, for every test of this file: it answers with
/// what `answer_with` wrote beside the test's database.
fn fake_claude() -> PathBuf {
    static FAKE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    FAKE.get_or_init(|| {
        let result = serde_json::to_string(&serde_json::json!({"type": "result", "result": r#"{"replies": {"c1": "Seen."}}"#})).unwrap();
        let script = format!(
            "#!/bin/sh\necho '{{\"type\":\"system\",\"subtype\":\"init\",\"model\":\"fake\"}}'\n\
             git=$(git rev-parse --path-format=absolute --git-common-dir)\n\
             while test -e \"$git/../../hold\"; do sleep 0.1; done\n\
             if test -e \"$git/../../answer\"; then cat \"$git/../../answer\"; else cat <<'EOF'\n{result}\nEOF\nfi\n"
        );
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR"));
        let (path, part) = (dir.join("try-claude.sh"), dir.join(format!("try-claude-{}.sh", std::process::id())));
        std::fs::write(&part, script).unwrap();
        Command::new("chmod").arg("+x").arg(&part).status().unwrap();
        std::fs::rename(&part, &path).unwrap();
        stacked::ask::stand_in(&path, Path::new(env!("CARGO_BIN_EXE_stacked")));
        path
    })
    .clone()
}

/// The program the editor opens links with here: one that opens nothing.
fn fake_opener() {
    static FAKE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    FAKE.get_or_init(|| {
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR"));
        let (path, part) = (dir.join("try-open.sh"), dir.join(format!("try-open-{}.sh", std::process::id())));
        std::fs::write(&part, "#!/bin/sh\nexit 0\n").unwrap();
        Command::new("chmod").arg("+x").arg(&part).status().unwrap();
        std::fs::rename(&part, &path).unwrap();
        stacked::refs::open_links_with(&path);
        path
    });
}

/// Has the stand-in answer the asks on `db` with `answer`, as claude ends
/// its turn with it: what is written around it is prose.
fn answer_with(db: &Path, answer: &serde_json::Value) {
    let text = format!("Here they are.\n\n```json\n{answer}\n```\n");
    let result = serde_json::json!({"type": "result", "result": text});
    std::fs::write(db.with_file_name("answer"), format!("{result}\n")).unwrap();
}

/// Has the stand-in answer with `tasks`, each a label and why, and `reply`
/// to the comment asked about.
fn tasks_answer(db: &Path, tasks: &[(&str, &str)], reply: &str) {
    let tasks: Vec<serde_json::Value> = tasks.iter().map(|(l, w)| serde_json::json!({"label": l, "why": w})).collect();
    answer_with(db, &serde_json::json!({"replies": {"c1": reply}, "tasks": tasks}));
}

fn draw(app: &mut App) -> String {
    let mut buf = Buffer::empty(Rect::new(0, 0, 120, 30));
    app.draw(&mut buf);
    (0..buf.area.height).map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect::<String>()).collect::<Vec<_>>().join("\n")
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

/// What the menu open says under its items.
fn menu_note(app: &App) -> String {
    match &app.mode {
        Mode::Menu(m) => m.note.clone().unwrap_or_default(),
        other => panic!("no menu open: {other:?}"),
    }
}

/// Opens the menu unless one is open, and does the item whose label starts
/// with `label`.
fn choose(app: &mut App, label: &str) {
    if !matches!(app.mode, Mode::Menu(_)) {
        assert_eq!(key(app, KeyCode::Char(' ')), Effect::None);
    }
    let all = items(app);
    let want = all.iter().position(|(l, _)| l.starts_with(label)).unwrap_or_else(|| panic!("no {label:?} in {all:?}"));
    assert!(all[want].1, "{label:?} cannot be chosen: {all:?}");
    for _ in 0..all.len() * 2 {
        match &app.mode {
            Mode::Menu(m) if m.sel == want => break,
            _ => {
                key(app, KeyCode::Down);
            }
        }
    }
    key(app, KeyCode::Enter);
    draw(app);
}

/// As `choose`, its first dropdown on the choice drawn as `pick` first.
fn choose_as(app: &mut App, label: &str, pick: &str) {
    if !matches!(app.mode, Mode::Menu(_)) {
        assert_eq!(key(app, KeyCode::Char(' ')), Effect::None);
    }
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
    for _ in 0..8 {
        let shown = match &app.mode {
            Mode::Menu(m) => m.items[want].choices.first().map(|c| c.list[c.picked].shown.clone()),
            _ => None,
        };
        if shown.is_some_and(|s| s.starts_with(pick)) {
            break;
        }
        key(app, KeyCode::Right);
    }
    key(app, KeyCode::Enter);
    draw(app);
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

/// The text of the review's line the cursor is on, as drawn.
fn selected_line(app: &mut App) -> Option<String> {
    let mut buf = Buffer::empty(Rect::new(0, 0, 120, 30));
    app.draw(&mut buf);
    (0..30).find_map(|y| {
        let reversed = (0..48).all(|x| buf[(x, y)].modifier.contains(ratatui::style::Modifier::REVERSED));
        reversed.then(|| (0..48).map(|x| buf[(x, y)].symbol()).collect())
    })
}

/// Down the review's list until the cursor is on the line holding `text`.
fn down_to_review(app: &mut App, text: &str) {
    for _ in 0..60 {
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

/// Waits for the asks to land, `n` works then waiting for review on `s`.
fn landed(app: &mut App, db: &Path, n: usize) {
    for _ in 0..300 {
        app.refresh();
        let d = Db::load(db).unwrap();
        let going = d.runs().iter().any(|r| r.state == stacked::db::RunState::Started);
        if !going && stacked::held::queue(&d, "s").len() == n {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    panic!("never landed: {n} works");
}

/// The comments of stack `s`'s patch `k`.
fn notes_of(db: &Path, k: usize) -> Vec<Note> {
    let d = Db::load(db).unwrap();
    let s = d.stack("s").unwrap();
    d.notes(&d.patch(&s.patches[k]).change).into_iter().cloned().collect()
}

/// The prompt the try push's last ask was given.
fn last_prompt(db: &Path) -> String {
    let d = Db::load(db).unwrap();
    let r = d.runs().into_iter().rev().find(|r| r.what == "try").cloned().expect("a try push's ask");
    stacked::ask::info(&r).unwrap().prompt
}

/// A comment written on patch 1's line `+TWO`, `text`, then asked as a try
/// push from its menu.
fn ask_try(app: &mut App, text: &str) {
    keys(app, &[KeyCode::Right]);
    down_to(app, "+TWO");
    keys(app, &[KeyCode::Enter]);
    save(app, text);
    keys(app, &[KeyCode::Down]);
    choose_as(app, "Ask agent", "try push");
}

#[test]
fn a_try_push_asked_lands_the_tasks_the_comment_asks_for_each_with_why() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db);
    tasks_answer(
        &db,
        &[("test-linux2404-64/opt-xpcshell-1", "It runs the tests of a.txt."), ("build-linux64/debug", "The build the tests need.")],
        "Two tasks.",
    );
    // The comment's how: try push, beside the others.
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "Run what tests a.txt, as many as you see fit.");
    keys(&mut app, &[KeyCode::Down, KeyCode::Char(' ')]);
    let Mode::Menu(m) = &app.mode else { panic!("{:?}", app.mode) };
    let ask = m.items.iter().find(|i| i.label == "Ask agent").unwrap();
    let hows: Vec<&str> = ask.choices[0].list.iter().map(|c| c.shown.as_str()).collect();
    assert_eq!(hows, ["change", "the whole stack", "answer only", "try push"]);
    keys(&mut app, &[KeyCode::Esc]);
    choose_as(&mut app, "Ask agent", "try push");
    assert!(app.status.starts_with("asked the agent about 1 comment (try push)"), "{}", app.status);
    landed(&mut app, &db, 1);
    // Its prompt: the tasks the comment asks for, as few or as many, exact
    // labels, why each; nothing pushed, nothing changed.
    let prompt = last_prompt(&db);
    for said in ["as few or as many as they say", "exact label", "Say why each is relevant", "Do NOT push anything, nor run `./mach`", "Do NOT modify anything"] {
        assert!(prompt.contains(said), "{said:?} in {prompt}");
    }
    assert!(prompt.contains("patch 1 of 2 with the patches below it"), "{prompt}");
    // Landed, in the banner, as read-only work to review.
    let screen = draw(&mut app);
    assert!(screen.contains("⟳ 1 landed ask waits for your review · the next: 2 tasks to push to try, 1 reply"), "{screen}");
    keys(&mut app, &[KeyCode::Esc]);
    assert!(draw(&mut app).contains(" read only, to review"), "{}", draw(&mut app));
    keys(&mut app, &[KeyCode::Enter]);
    assert_eq!(app.screen, Screen::Review);
    let screen = draw(&mut app);
    for line in [" tasks to push to try", "   · test-linux2404-64/opt-xpcshell-1", "   · build-linux64/debug", " replies"] {
        assert!(screen.contains(line), "{line:?} in:\n{screen}");
    }
    // On the first task, why the agent selected it, as its comment.
    assert!(selected_line(&mut app).unwrap().contains("test-linux2404-64/opt-xpcshell-1"), "{screen}");
    for said in ["why the agent selected this task", "agent's comment", "It runs the tests of a.txt."] {
        assert!(screen.contains(said), "{said:?} in:\n{screen}");
    }
    // Enter accepts it; the next one, rejected from the menu.
    keys(&mut app, &[KeyCode::Enter]);
    assert!(draw(&mut app).contains("   ✓ test-linux2404-64/opt-xpcshell-1"));
    down_to_review(&mut app, "build-linux64/debug");
    keys(&mut app, &[KeyCode::Char(' ')]);
    let all = items(&app);
    for label in ["Reply to it", "Mark as accepted", "Mark as rejected", "Mark all tasks accepted", "Mark all tasks rejected"] {
        assert!(all.iter().any(|(l, ok)| l == label && *ok), "{label:?} in {all:?}");
    }
    choose(&mut app, "Mark as rejected");
    assert!(draw(&mut app).contains("   ✗ build-linux64/debug"));
    // A reply to why the first was selected: what you wrote, to send back.
    keys(&mut app, &[KeyCode::Up]);
    choose(&mut app, "Reply to it");
    assert!(matches!(&app.mode, Mode::Edit(_)), "{:?}", app.mode);
    assert!(draw(&mut app).contains("your answer to why the agent selected this task"), "{}", draw(&mut app));
    save(&mut app, "Run it on Windows too.");
    let screen = draw(&mut app);
    assert!(screen.contains(" what you wrote, to send back"), "{screen}");
    // The list cut to its width; the answer under the agent's reason.
    assert!(screen.contains("   ✎ your answer on test-linux2404-64/opt-xpcshe"), "{screen}");
    assert!(screen.contains("▎ you:") && screen.contains("▎   Run it on Windows too."), "{screen}");
    // Esc: it goes back to the agent, neither accepted nor rejected.
    keys(&mut app, &[KeyCode::Esc]);
    assert!(menu_note(&app).starts_with("You wrote 1 answer to why the agent selected a task in this review"), "{}", menu_note(&app));
    assert!(menu_note(&app).contains("tasks: 1 accepted, 1 rejected"), "{}", menu_note(&app));
    tasks_answer(
        &db,
        &[
            ("test-linux2404-64/opt-xpcshell-1", "It runs the tests of a.txt."),
            ("test-windows11-64-24h2/opt-xpcshell-1", "The same tests, on Windows, as you asked."),
            ("build-win64/opt", "The build the Windows tests need."),
        ],
        "Three tasks now.",
    );
    choose(&mut app, "Send it back to the agent");
    assert!(app.status.starts_with("sent back to the agent (try push): 1 comment to ask again"), "{}", app.status);
    // Your answer in the comment's thread, after the agent's reply.
    let thread = notes_of(&db, 0)[0].text.clone();
    assert!(thread.contains("Two tasks."), "{thread}");
    assert!(thread.ends_with("On the tasks the agent selected:\n- `test-linux2404-64/opt-xpcshell-1`: Run it on Windows too."), "{thread}");
    landed(&mut app, &db, 1);
    // The next round is told what it selected before, as you marked it.
    let prompt = last_prompt(&db);
    assert!(prompt.contains("This is another round: the user reviewed the tasks you selected"), "{prompt}");
    assert!(prompt.contains("- `test-linux2404-64/opt-xpcshell-1` (accepted): It runs the tests of a.txt.\n  The user answered: Run it on Windows too."), "{prompt}");
    assert!(prompt.contains("- `build-linux64/debug` (rejected): The build the tests need."), "{prompt}");
    // Its tasks, in place of those sent back; all accepted, then closed: the
    // push comes next, and nothing is pushed yet.
    keys(&mut app, &[KeyCode::Esc]);
    keys(&mut app, &[KeyCode::Enter]);
    assert_eq!(app.screen, Screen::Review);
    assert!(draw(&mut app).contains("   · test-windows11-64-24h2/opt-xpcshell-1"), "{}", draw(&mut app));
    keys(&mut app, &[KeyCode::Esc]);
    choose(&mut app, "Accept all remaining and Close");
    let note = menu_note(&app);
    assert!(
        note.contains("Closing the review pushes the 3 tasks you accepted to try, with patch 1 alone: first `./mach try fuzzy --no-push`, to see that it selects them, then the push."),
        "{note}"
    );
    assert!(note.contains("Pushing to try is not built yet in this stacked: closing the review now pushes nothing."), "{note}");
    choose(&mut app, "Accept them and close");
    assert_eq!(app.status, "reviewed: 3 tasks accepted; 1 reply accepted; pushing to try is not built yet in this stacked, so nothing was pushed");
    assert!(stacked::held::queue(&Db::load(&db).unwrap(), "s").is_empty());
}

#[test]
fn a_try_push_closed_with_no_task_accepted_pushes_nothing() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db);
    tasks_answer(&db, &[("build-linux64/debug", "A build.")], "One task.");
    ask_try(&mut app, "Build it.");
    landed(&mut app, &db, 1);
    keys(&mut app, &[KeyCode::Esc, KeyCode::Enter]);
    assert_eq!(app.screen, Screen::Review);
    keys(&mut app, &[KeyCode::Esc]);
    choose(&mut app, "Reject all remaining and Close");
    assert!(menu_note(&app).contains("No task is accepted: closing the review pushes nothing to try."), "{}", menu_note(&app));
    choose(&mut app, "Reject them and close");
    assert_eq!(app.status, "reviewed: 1 task rejected; 1 reply rejected");
}

#[test]
fn a_try_push_past_the_tasks_mach_pushes_without_asking_says_it_runs_at_the_lowest_priority() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db);
    // 1,240 tasks, more than the 1,000 mach pushes without asking.
    let labels: Vec<String> = (1..=1240).map(|k| format!("test-linux2404-64/opt-mochitest-plain-{k}")).collect();
    let tasks: Vec<(&str, &str)> = labels.iter().map(|l| (l.as_str(), "As many as you asked.")).collect();
    tasks_answer(&db, &tasks, "All of them.");
    ask_try(&mut app, "Run everything you can.");
    landed(&mut app, &db, 1);
    keys(&mut app, &[KeyCode::Esc, KeyCode::Enter]);
    keys(&mut app, &[KeyCode::Esc]);
    choose(&mut app, "Accept all remaining and Close");
    let note = menu_note(&app);
    assert!(
        note.contains(
            "These 1,240 tasks are more than the 1,000 mach pushes without asking: they would run at the lowest priority, as mach does when you say yes."
        ),
        "{note}"
    );
    // Each run three times: counted as mach counts them.
    keys(&mut app, &[KeyCode::Esc, KeyCode::Esc]);
    choose(&mut app, "Discard");
    assert_eq!(app.screen, Screen::Main);
    let run = stacked::held::queue(&Db::load(&db).unwrap(), "s")[0].id.clone();
    Db::edit(&db, |d| {
        let mut r = d.run(&run).unwrap().clone();
        let mut h = r.held.clone().unwrap();
        h.tasks.truncate(400);
        h.rebuild = Some(3);
        r.held = Some(h);
        d.add_run(r).map(|_| ())
    })
    .unwrap();
    app.refresh();
    app.on_banner = true;
    keys(&mut app, &[KeyCode::Enter]);
    assert!(draw(&mut app).contains(" tasks to push to try · each runs 3 times"), "{}", draw(&mut app));
    keys(&mut app, &[KeyCode::Esc]);
    choose(&mut app, "Accept all remaining and Close");
    let note = menu_note(&app);
    assert!(note.contains("These 400 tasks, each run 3 times, are 1,200 tasks, more than the 1,000 mach pushes without asking"), "{note}");
}

#[test]
fn a_try_push_asks_its_own_setup_first_as_its_column_is_new() {
    let (_tmp, db, src) = setup();
    // Set up by a stacked with no column for try pushes.
    stacked(&src, &db, &["config", "--local", "agent.setup", "1"]);
    let local = src.canonicalize().unwrap().join(".stacked/local.toml");
    let mut app = open(&db);
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "Why so loud?");
    keys(&mut app, &[KeyCode::Down]);
    // An answer goes at once: its kind was set up.
    choose_as(&mut app, "Ask agent", "answer only");
    assert!(app.status.starts_with("asked the agent about 1 comment (answer only)"), "{}", app.status);
    landed(&mut app, &db, 1);
    // A try push: the setup first, its column there to check.
    down_to(&mut app, "+LINE 39");
    keys(&mut app, &[KeyCode::Enter]);
    save(&mut app, "Which tasks?");
    keys(&mut app, &[KeyCode::Down]);
    choose_as(&mut app, "Ask agent", "try push");
    assert!(matches!(&app.mode, Mode::Setup(_)), "{:?}", app.mode);
    let mut screen = draw(&mut app);
    for _ in 0..300 {
        if screen.contains(" What each kind of ask may use") {
            break;
        }
        app.refresh();
        std::thread::sleep(std::time::Duration::from_millis(100));
        screen = draw(&mut app);
    }
    assert!(screen.contains(" read only  change  try push"), "{screen}");
    tasks_answer(&db, &[("build-linux64/debug", "A build.")], "One task.");
    keys(&mut app, &[KeyCode::Char('s')]);
    assert!(app.status.contains("asked the agent about 1 comment (try push)"), "{}", app.status);
    let saved = std::fs::read_to_string(&local).unwrap();
    assert!(saved.contains("setup = 2") && saved.contains("try_tools = [\"Bash(curl *)\", \"Bash(jq *)\"]"), "{saved}");
    landed(&mut app, &db, 2);
}

/// A comment written on patch 1's line `+TWO`, `text`, then put in the
/// batch as a try push from its menu, to be asked later.
fn batch_try(app: &mut App, text: &str) {
    keys(app, &[KeyCode::Right]);
    down_to(app, "+TWO");
    keys(app, &[KeyCode::Enter]);
    save(app, text);
    keys(app, &[KeyCode::Down, KeyCode::Char(' ')]);
    // Later: in the batch.
    let Mode::Menu(m) = &app.mode else { panic!("{:?}", app.mode) };
    let when = m.items.iter().position(|i| i.label == "Send it").unwrap();
    while !matches!(&app.mode, Mode::Menu(m) if m.sel == when) {
        key(app, KeyCode::Down);
    }
    keys(app, &[KeyCode::Right]);
    keys(app, &[KeyCode::Up]);
    choose_as(app, "Ask agent later", "try push");
    assert!(app.status.starts_with("batched (try push)"), "{}", app.status);
    assert!(draw(app).contains("1 batched ask"), "{}", draw(app));
}

#[test]
fn a_try_push_batched_is_sent_from_the_banner_on_its_own() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db);
    batch_try(&mut app, "Which tasks?");
    tasks_answer(&db, &[("build-linux64/debug", "A build.")], "One task.");
    keys(&mut app, &[KeyCode::Esc]);
    keys(&mut app, &[KeyCode::Char(' ')]);
    choose(&mut app, "Send 1 try push, each on its own");
    assert!(app.status.starts_with("sent the batch (try push): 1 try push, each on its own"), "{}", app.status);
    landed(&mut app, &db, 1);
}

#[test]
fn a_try_push_the_editor_batched_goes_with_the_batch_the_command_line_sends() {
    let (_tmp, db, src) = setup();
    // The command line's settings: the setup done, as the editor's is.
    stacked(&src, &db, &["config", "--local", "agent.setup", &stacked::setup::VERSION.to_string()]);
    let mut app = open(&db);
    batch_try(&mut app, "Which tasks?");
    tasks_answer(&db, &[("build-linux64/debug", "A build.")], "One task.");
    let out = stacked_with(&src, &db, &["send", "s", "--batch", "--wait"], &[("STACKED_CLAUDE", &fake_claude())]);
    assert!(out.contains("\tsent (try push)"), "{out}");
    let d = Db::load(&db).unwrap();
    assert!(d.stack_notes("s").iter().all(|n| n.queued.is_none()), "the batch is empty");
    let work = stacked::held::queue(&d, "s");
    assert_eq!(work.len(), 1);
    let labels: Vec<&str> = work[0].held.as_ref().unwrap().tasks.iter().map(|t| t.label.as_str()).collect();
    assert_eq!(labels, ["build-linux64/debug"]);
    landed(&mut app, &db, 1);
}
