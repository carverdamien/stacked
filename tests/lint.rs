//! Linting a stack from its header's menu, `Lint the stack (CMD)`: the
//! item, grayed and why; the linter of the stack's checkout, `./mach lint
//! --fix` where `./mach` is at its top, nothing written; the job run away
//! from the screen, patch by patch, in the stack's lint worktree, never in
//! the checkout; each fix folded into the earliest patch it shows in; a fix
//! that would keep a patch above from applying stopping the phase, the
//! fixes from there on in a patch on top with a comment ready to send;
//! nothing landing when the linter fixes and reports nothing; what it
//! reports listed, and alone landing read only; the queue: one lint job at
//! a time, an edit asked meanwhile waiting for it, then going on from what
//! it made; and the stop: a linter that hangs stopped from the banner and by
//! `stacked stop`, nothing landing, a job waiting its turn dropped, the next
//! job starting clean. The editor is drawn into a buffer and driven by keys,
//! as in `tests/tui.rs`. A stand-in runs in place of the linter
//! (`fake_linter`), never a real one: from the tests' own processes, stacked
//! runs no other.

use std::path::{Path, PathBuf};
use std::process::Command;

use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;

use stacked::db::{Db, Run, RunState};
use stacked::tui::{App, Effect, Focus, Mode, Screen};

fn git(dir: &Path, args: &[&str]) -> String {
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
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// stacked run in `dir` on the database `db`, as the tests run it: its own
/// things kept in `home` beside the database, the setup of what each kind
/// of ask may use done there; what it writes in the temporary directory in
/// `tmp` beside it. Whether it did, what it printed, and what it said.
fn stacked(dir: &Path, db: &Path, args: &[&str]) -> (bool, String, String) {
    let (home, tmp) = (db.with_file_name("home"), db.with_file_name("tmp"));
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(home.join("config.toml"), format!("[agent]\nsetup = {}\n", stacked::setup::VERSION)).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_stacked"))
        .current_dir(dir)
        .arg("-f")
        .arg(db)
        .args(args)
        .env("STACKED_HOME", home)
        .env("TMPDIR", tmp)
        .env("STACKED_TODO", db.with_file_name("todo"))
        .env_remove("STACKED_CONFIG")
        .env_remove("STACKED_APP_STATE")
        // No agent of the user's is ever asked, nor a linter of theirs run.
        .env("STACKED_CLAUDE", "/usr/bin/false")
        .env("GIT_AUTHOR_NAME", "A U Thor")
        .env("GIT_AUTHOR_EMAIL", "author@example.com")
        .env("GIT_COMMITTER_NAME", "C O Mitter")
        .env("GIT_COMMITTER_EMAIL", "committer@example.com")
        .output()
        .unwrap();
    (out.status.success(), String::from_utf8_lossy(&out.stdout).into_owned(), String::from_utf8_lossy(&out.stderr).into_owned())
}

/// As `stacked`, which must do it: what it printed.
fn ok(dir: &Path, db: &Path, args: &[&str]) -> String {
    let (fine, out, err) = stacked(dir, db, args);
    assert!(fine, "stacked {args:?}: {err}");
    out
}

/// a.txt of the base: twenty lines, numbered.
fn lines() -> String {
    (1..=20).map(|n| format!("line {n}\n")).collect()
}

/// A database holding stack `s`, a commit of each of `commits` (the files
/// it writes, whole) on a base of a.txt and b.txt (and an executable
/// `mach`, if `mach`), and stack `t` of its first commit alone; and the
/// repository. The linter of its checkout is set to `linter` with `stacked
/// setup lint`, if one is given.
fn setup(commits: &[&[(&str, &str)]], mach: bool, linter: Option<&[&str]>) -> (tempfile::TempDir, PathBuf, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src");
    std::fs::create_dir(&src).unwrap();
    git(&src, &["init", "-q", "-b", "main"]);
    git(&src, &["config", "commit.gpgsign", "false"]);
    std::fs::write(src.join("a.txt"), lines()).unwrap();
    std::fs::write(src.join("b.txt"), "bee\n").unwrap();
    if mach {
        std::fs::write(src.join("mach"), "#!/bin/sh\necho 'not the real mach: the tests never run it' >&2\nexit 2\n").unwrap();
        git(&src, &["update-index", "--add", "--chmod=+x", "mach"]);
    }
    git(&src, &["add", "."]);
    git(&src, &["commit", "-qm", "base"]);
    for (k, c) in commits.iter().enumerate() {
        for (f, text) in c.iter() {
            std::fs::write(src.join(f), text).unwrap();
        }
        git(&src, &["add", "."]);
        git(&src, &["commit", "-qm", &format!("Bug {} - patch {}", k + 1, k + 1)]);
    }
    let db = tmp.path().join("db.jsonl");
    let n = commits.len();
    ok(&src, &db, &["import", "t", &format!("HEAD~{n}..HEAD~{}", n - 1)]);
    ok(&src, &db, &["import", "s", &format!("HEAD~{n}")]);
    if let Some(words) = linter {
        let mut args = vec!["setup", "lint"];
        args.extend(words);
        ok(&src, &db, &args);
    }
    (tmp, db, src)
}

/// The patches of a stack whose second line, in a.txt, ends with white
/// space, as the first patch writes it; the second patch writes its
/// eighteenth with white space too, and b.txt.
fn two_fixes() -> (String, String) {
    let one = lines().replace("line 2\n", "TWO  \n");
    let two = one.replace("line 18\n", "EIGHTEEN \n");
    (one, two)
}

/// The settings of these tests: the defaults, with the setup of what each
/// kind of ask may use done, as a user's are once it is saved.
fn settings() -> stacked::config::Config {
    let mut c = stacked::config::Config::default();
    c.agent.setup = Some(stacked::setup::VERSION);
    c
}

/// The editor on `db`, with the tests' settings and the stand-ins for
/// claude and for the linter.
fn open(db: &Path) -> Editor {
    stacked::config::init(settings());
    fake_claude();
    fake_linter();
    let mut app = App::with_config(db, settings()).unwrap();
    draw(&mut app);
    Editor(app)
}

/// The editor a test opened: once it goes, it waits for the work the test
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
            over(&mut self.0);
        }
    }
}

/// Waits for the work the editor started away from the screen, and for no
/// process to be at work in the folder of its database any more: a lint
/// job's, an ask's.
fn over(app: &mut App) {
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

/// The stand-in for the linter, for every test of this file, run in place
/// of the command's program with the command's arguments, then the files:
/// it waits while `lint-hold` is beside the test's database; writes in
/// `lint-calls` there what it is run with, where, and whether it could read
/// anything; then, of each file it is given, takes out the white space each
/// line ends with, and says each line holding `NOT-LINTABLE`, which it
/// cannot fix, exiting with 1 then.
fn fake_linter() -> PathBuf {
    static FAKE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    FAKE.get_or_init(|| {
        let script = r#"#!/bin/sh
git=$(git rev-parse --path-format=absolute --git-common-dir)
dir="$git/../.."
while [ -e "$dir/lint-hold" ]; do sleep 0.1; done
{
  printf 'args:'
  for a in "$@"; do printf ' %s' "$a"; done
  echo
  echo "cwd: $(pwd -P)"
  if read -r line; then echo "stdin: open"; else echo "stdin: closed"; fi
} >> "$dir/lint-calls"
status=0
for f in "$@"; do
  [ -f "$f" ] || continue
  if grep -q 'NOT-LINTABLE' "$f"; then
    grep -n 'NOT-LINTABLE' "$f" | while IFS=: read -r no rest; do echo "$f:$no: NOT-LINTABLE cannot be fixed"; done
    status=1
  fi
  sed 's/[[:space:]]*$//' "$f" > "$f.linted" && cat "$f.linted" > "$f" && rm -f "$f.linted"
done
exit $status
"#;
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR"));
        let (path, part) = (dir.join("lint-linter.sh"), dir.join(format!("lint-linter-{}.sh", std::process::id())));
        std::fs::write(&part, script).unwrap();
        Command::new("chmod").arg("+x").arg(&part).status().unwrap();
        std::fs::rename(&part, &path).unwrap();
        stacked::lint::stand_in(&path);
        path
    })
    .clone()
}

/// The stand-in for claude, for every test of this file: it writes a.txt
/// as it finds it where it works in `claude-saw` beside the test's
/// database, and answers `Seen.`.
fn fake_claude() -> PathBuf {
    static FAKE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    FAKE.get_or_init(|| {
        let result = serde_json::to_string(&serde_json::json!({"type": "result", "result": r#"{"replies": {"c1": "Seen."}}"#})).unwrap();
        let script = format!(
            "#!/bin/sh\necho '{{\"type\":\"system\",\"subtype\":\"init\",\"model\":\"fake\"}}'\n\
             git=$(git rev-parse --path-format=absolute --git-common-dir)\n\
             cat a.txt > \"$git/../../claude-saw\"\n\
             cat <<'EOF'\n{result}\nEOF\n"
        );
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR"));
        let (path, part) = (dir.join("lint-claude.sh"), dir.join(format!("lint-claude-{}.sh", std::process::id())));
        std::fs::write(&part, script).unwrap();
        Command::new("chmod").arg("+x").arg(&part).status().unwrap();
        std::fs::rename(&part, &path).unwrap();
        stacked::ask::stand_in(&path, Path::new(env!("CARGO_BIN_EXE_stacked")));
        path
    })
    .clone()
}

/// The stand-in for the linter held at its start, on the jobs of `db`,
/// until what this gives is dropped.
struct Held(PathBuf);

impl Drop for Held {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn hold(db: &Path) -> Held {
    let at = db.with_file_name("lint-hold");
    std::fs::write(&at, "").unwrap();
    Held(at)
}

/// All that the stand-in for the linter wrote of how it was run, beside
/// `db`.
fn lint_calls(db: &Path) -> String {
    std::fs::read_to_string(db.with_file_name("lint-calls")).unwrap_or_default()
}

fn draw(app: &mut App) -> String {
    let mut buf = Buffer::empty(Rect::new(0, 0, 140, 30));
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

/// The menu of the stack's header, open: the cursor there first.
fn stack_menu(app: &mut App) {
    app.screen = Screen::Main;
    app.on_banner = false;
    app.focus = Focus::List;
    app.on_stack = true;
    draw(app);
    keys(app, &[KeyCode::Char(' ')]);
    let Mode::Menu(m) = &app.mode else { panic!("{:?}", app.mode) };
    assert!(m.title.starts_with("the stack "), "{}", m.title);
}

/// Does the item of the menu open whose label starts with `label`.
fn choose(app: &mut App, label: &str) {
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

/// The stack linted from its header's menu, its item named `label`.
fn lint(app: &mut App, label: &str) {
    stack_menu(app);
    choose(app, label);
    assert!(app.status.starts_with("the linter is to lint the stack"), "{}", app.status);
}

/// The lint jobs of `db`, the first asked first.
fn lint_runs(db: &Path) -> Vec<Run> {
    let d = Db::load(db).unwrap();
    d.runs().into_iter().filter(|r| r.what == stacked::db::LINT).cloned().collect()
}

/// Waits for the last lint job of `db` to be where `there` says, the
/// editor looking meanwhile: its run then.
fn wait_lint(app: &mut App, db: &Path, there: impl Fn(&Run) -> bool) -> Run {
    for _ in 0..600 {
        app.refresh();
        if let Some(r) = lint_runs(db).pop().filter(|r| there(r)) {
            app.refresh();
            draw(app);
            return r;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    panic!("the lint job never got there: {:?}\n{}", lint_runs(db).pop(), lint_calls(db));
}

/// Waits for the last lint job of `db` to be over.
fn linted(app: &mut App, db: &Path) -> Run {
    wait_lint(app, db, |r| r.state != RunState::Started)
}

/// File `path` as patch `n` (from 1) of stack `s` leaves it, read with
/// stacked.
fn file_at(src: &Path, db: &Path, n: usize, path: &str) -> String {
    ok(src, db, &["show", "s", &n.to_string(), "--path", path])
}

/// The work of lint job `r`, accepted whole as its review closes.
fn accept(db: &Path, r: &Run) {
    Db::edit(db, |d| stacked::held::close(d, &r.id, &Default::default(), Some(true))).unwrap();
}

#[test]
fn the_stack_s_lint_is_offered_naming_its_linter_and_grayed_with_why() {
    // No linter: the item, grayed, its menu saying why.
    let (one, _) = two_fixes();
    let (_tmp, db, src) = setup(&[&[("a.txt", &one)]], false, None);
    let mut app = open(&db);
    stack_menu(&mut app);
    assert!(items(&app).contains(&("Lint the stack".to_string(), false)), "{:?}", items(&app));
    assert!(menu_note(&app).contains(stacked::lint::NO_LINTER), "{}", menu_note(&app));
    keys(&mut app, &[KeyCode::Esc]);
    // On a patch's menu, nothing lints: only the stack's.
    app.on_stack = false;
    keys(&mut app, &[KeyCode::Char(' ')]);
    assert!(!items(&app).iter().any(|(l, _)| l.starts_with("Lint")), "{:?}", items(&app));
    keys(&mut app, &[KeyCode::Esc]);
    // Turned off: grayed, said.
    ok(&src, &db, &["setup", "lint", "--off"]);
    stack_menu(&mut app);
    assert!(items(&app).contains(&("Lint the stack".to_string(), false)), "{:?}", items(&app));
    assert!(menu_note(&app).contains(stacked::lint::OFF), "{}", menu_note(&app));
    keys(&mut app, &[KeyCode::Esc]);
    // Set: named, to choose.
    ok(&src, &db, &["setup", "lint", "./lint.sh", "--fix"]);
    stack_menu(&mut app);
    assert!(items(&app).contains(&("Lint the stack (./lint.sh --fix)".to_string(), true)), "{:?}", items(&app));
    assert!(!menu_note(&app).contains("linter"), "{}", menu_note(&app));
    keys(&mut app, &[KeyCode::Esc]);
    // While the stack may not be edited by hand (an edit by hand waits for
    // review), grayed, the menu saying why.
    let d = Db::load(&db).unwrap();
    let mut patches = d.stack("s").unwrap().patches.clone();
    drop(d);
    Db::edit(&db, |d| {
        let p = d.patch(&patches[0]).clone();
        let draft = stacked::mbox::Draft { message: "Bug 1 - reworded".into(), ..stacked::mbox::Draft::from(&p) };
        patches[0] = d.add_patch(stacked::ops::revise(&p, draft))?;
        stacked::held::hold_by_hand(d, "s", patches.clone(), "reword", "patch 1", "reworded patch 1", None)
    })
    .unwrap();
    app.refresh();
    stack_menu(&mut app);
    assert!(items(&app).contains(&("Lint the stack (./lint.sh --fix)".to_string(), false)), "{:?}", items(&app));
    assert!(menu_note(&app).contains("your edit by hand of patch 1 waits for your review"), "{}", menu_note(&app));
    keys(&mut app, &[KeyCode::Esc]);
    // And asked all the same, refused, nothing queued.
    assert!(stacked::lint::ask(&db, "s").unwrap_err().0.contains("your edit by hand of patch 1 waits for your review"));
    assert!(lint_runs(&db).is_empty());
    // A stack that cannot change: grayed too.
    let (_tmp2, db2, _src2) = setup(&[&[("a.txt", &one)]], false, Some(&["./lint.sh"]));
    let s = Db::load(&db2).unwrap().stack("s").unwrap().clone();
    Db::append(&db2, &[stacked::db::Record::Stack(stacked::db::Stack { base: None, op: "no base".into(), parent: None, ..s })]).unwrap();
    let mut app = open(&db2);
    stack_menu(&mut app);
    assert!(items(&app).iter().any(|(l, ok)| l.starts_with("Lint the stack") && !ok), "{:?}", items(&app));
    assert!(menu_note(&app).contains("read-only"), "{}", menu_note(&app));
}

#[test]
fn a_checkout_with_mach_at_its_top_lints_with_it_nothing_written() {
    let (one, _) = two_fixes();
    let (_tmp, db, src) = setup(&[&[("a.txt", &one)]], true, None);
    let mut app = open(&db);
    stack_menu(&mut app);
    assert!(items(&app).contains(&("Lint the stack (./mach lint --fix)".to_string(), true)), "{:?}", items(&app));
    keys(&mut app, &[KeyCode::Esc]);
    // Nothing written: the checkout's own settings hold no linter, and
    // `setup lint` says where it comes from.
    assert!(!src.join(".stacked/local.toml").exists());
    let (fine, out, err) = stacked(&src, &db, &["setup", "lint"]);
    assert!(fine && out == "./mach lint --fix\n", "{out}{err}");
    assert!(err.contains("this checkout's linter by default, as ./mach is at its top") && err.contains("nothing is written"), "{err}");
    // Linted, the stand-in runs in place of mach, given its arguments, then
    // the files: the real one never runs from the tests' processes.
    lint(&mut app, "Lint the stack (./mach lint --fix)");
    let r = linted(&mut app, &db);
    assert_eq!(r.state, RunState::Done, "{}", r.summary);
    assert!(lint_calls(&db).starts_with("args: lint --fix a.txt\n"), "{}", lint_calls(&db));
    let job = r.lint.as_deref().unwrap();
    assert_eq!(job.command, "./mach lint --fix");
    assert!(!src.join(".stacked/local.toml").exists());
}

#[test]
fn each_fix_is_folded_into_the_earliest_patch_it_shows_in_away_from_the_checkout() {
    let (one, two) = two_fixes();
    let (_tmp, db, src) = setup(&[&[("a.txt", &one)], &[("a.txt", &two), ("b.txt", "BEE  \n")]], false, Some(&["./lint.sh", "--fix"]));
    let mut app = open(&db);
    lint(&mut app, "Lint the stack (./lint.sh --fix)");
    let r = linted(&mut app, &db);
    assert_eq!(r.state, RunState::Done, "{}", r.summary);
    // Each patch linted at its place, on its files, in the stack's lint
    // worktree, reading nothing.
    let at = src.canonicalize().unwrap().join(".stacked/s/lint");
    let calls = lint_calls(&db);
    let want = format!("args: --fix a.txt\ncwd: {w}\nstdin: closed\nargs: --fix a.txt b.txt\ncwd: {w}\nstdin: closed\n", w = at.display());
    assert_eq!(calls, want);
    // The checkout is not touched: its files as they were, nothing changed.
    assert_eq!(std::fs::read_to_string(src.join("a.txt")).unwrap(), two);
    assert_eq!(git(&src, &["status", "--porcelain"]), "");
    // Landed for review, as one change: line 2's fix in patch 1, which wrote
    // it; line 18's and b.txt's in patch 2.
    let h = r.held.clone().unwrap();
    let c = h.stack.clone().unwrap();
    assert_eq!(c.op, "lint fixes in patches 1 and 2 (./lint.sh --fix)");
    assert_eq!(r.summary, "landed: lint fixes in patches 1 and 2 (./lint.sh --fix)");
    let report = h.lint.clone().unwrap();
    assert_eq!((report.fixed.clone(), report.stopped.clone(), report.on_top), (vec![0, 1], None, false));
    assert!(h.opened.is_empty() && report.errors.is_empty());
    // The banner says so; the review names it as the history will.
    let screen = draw(&mut app);
    assert!(screen.contains("1 lint of the stack waits for your review · the next: a change of patches 1 and 2, by the linter: ./lint.sh --fix"), "{screen}");
    keys(&mut app, &[KeyCode::Esc, KeyCode::Enter]);
    assert_eq!(app.screen, Screen::Review);
    let screen = draw(&mut app);
    assert!(screen.contains(" the linter's work: a change of patches 1 and 2 ·"), "{screen}");
    assert!(screen.contains("modify  1 Bug 1 - patch 1") && screen.contains("modify  2 Bug 2 - patch 2"), "{screen}");
    // Nothing changes before it is accepted; accepted, the history says it.
    assert_eq!(file_at(&src, &db, 2, "a.txt"), two);
    accept(&db, &r);
    assert_eq!(file_at(&src, &db, 1, "a.txt"), one.replace("TWO  ", "TWO"));
    assert_eq!(file_at(&src, &db, 2, "a.txt"), two.replace("TWO  ", "TWO").replace("EIGHTEEN ", "EIGHTEEN"));
    assert_eq!(file_at(&src, &db, 2, "b.txt"), "BEE\n");
    let d = Db::load(&db).unwrap();
    assert_eq!(d.stack("s").unwrap().op, "lint fixes in patches 1 and 2 (./lint.sh --fix)");
    // Patch 2's new version holds its own fixes alone.
    let p2 = d.patch(&d.stack("s").unwrap().patches[1]);
    assert!(!p2.diff.contains("TWO"), "{}", p2.diff);
}

#[test]
fn a_fix_that_would_keep_a_patch_above_from_applying_goes_on_top_with_a_comment_to_send() {
    // Patch 2 changes line 3, its context line 2 as patch 1 wrote it, with
    // white space at its end.
    let (one, _) = two_fixes();
    let two = one.replace("line 3\n", "THREE\n");
    let (_tmp, db, src) = setup(&[&[("a.txt", &one)], &[("a.txt", &two)]], false, Some(&["./lint.sh"]));
    let mut app = open(&db);
    let before = Db::load(&db).unwrap().stack("s").unwrap().patches.clone();
    lint(&mut app, "Lint the stack (./lint.sh)");
    let r = linted(&mut app, &db);
    // Patch 1 linted, its fixes not kept; then the top, on the files of
    // every patch from patch 1 on.
    assert_eq!(lint_calls(&db).lines().filter(|l| l.starts_with("args:")).collect::<Vec<_>>(), ["args: a.txt", "args: a.txt"]);
    let h = r.held.clone().unwrap();
    let report = h.lint.clone().unwrap();
    let stopped = report.stopped.clone().unwrap();
    assert_eq!((stopped.at, stopped.why.as_str(), report.on_top), (0, "they would keep patch 2 from applying", true));
    let c = h.stack.clone().unwrap();
    assert_eq!(c.op, "lint fixes in a patch on top from patch 1 on (./lint.sh)");
    // The patches as they were, then a patch on top, by you, holding the fix.
    assert_eq!(c.patches[..2], before[..]);
    let d = Db::load(&db).unwrap();
    let top = d.patch(&c.patches[2]).clone();
    assert_eq!(top.title(), stacked::lint::ON_TOP);
    let body = top.message.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(body.contains("`./lint.sh` made these fixes at the top of the stack, on the files of patches 1 and 2."), "{body}");
    assert!(top.diff.contains("-TWO  \n+TWO\n"), "{}", top.diff);
    // A comment on it, yours, ready to send, sent to no one.
    assert_eq!(h.opened.len(), 1);
    let n = &h.opened[0];
    assert_eq!((n.change.as_str(), n.text.as_str(), n.by.clone(), n.queued.clone()), (top.change.as_str(), stacked::lint::FOLD, None, None));
    drop(d);
    // The review shows the patch added, and the comment where it would be.
    keys(&mut app, &[KeyCode::Esc, KeyCode::Enter]);
    let screen = draw(&mut app);
    assert!(screen.contains(&format!("add     3 {}", stacked::lint::ON_TOP)), "{screen}");
    assert!(screen.contains(" comments opened"), "{screen}");
    // Accepted: the patch on top, its comment open on it to send, no ask.
    accept(&db, &r);
    let d = Db::load(&db).unwrap();
    let s = d.stack("s").unwrap();
    assert_eq!(s.patches.len(), 3);
    let notes = d.notes(&d.patch(&s.patches[2]).change);
    assert_eq!(notes.len(), 1);
    assert!(stacked::ask::sendable(notes[0]) && notes[0].queued.is_none(), "{:?}", notes[0]);
    assert!(!d.runs().iter().any(|r| r.ask.is_some()), "nothing was asked of the agent");
    assert_eq!(file_at(&src, &db, 3, "a.txt"), two.replace("TWO  ", "TWO"));
}

#[test]
fn nothing_lands_when_the_linter_fixes_and_reports_nothing() {
    let clean = lines().replace("line 2\n", "two\n");
    let (_tmp, db, _src) = setup(&[&[("a.txt", &clean)]], false, Some(&["./lint.sh"]));
    let mut app = open(&db);
    let before = Db::load(&db).unwrap().stack("s").unwrap().patches.clone();
    lint(&mut app, "Lint the stack (./lint.sh)");
    let r = linted(&mut app, &db);
    assert_eq!((r.state, r.summary.as_str(), r.held.is_none()), (RunState::Done, stacked::lint::NOTHING, true));
    // Said quietly, nothing to review.
    assert_eq!(app.status, format!("./lint.sh: {}", stacked::lint::NOTHING));
    let d = Db::load(&db).unwrap();
    assert!(stacked::held::queue(&d, "s").is_empty());
    assert_eq!(d.stack("s").unwrap().patches, before);
    // Its lines are its run's to read.
    assert!(stacked::ask::tail(&d, &r.id, 50).iter().any(|l| l == "$ ./lint.sh a.txt"), "{:?}", stacked::ask::tail(&d, &r.id, 50));
}

#[test]
fn what_the_linter_reports_is_listed_with_its_fixes_and_alone_lands_read_only() {
    // A fix in patch 1, and a line patch 2 writes that it cannot fix.
    let (one, _) = two_fixes();
    let two = one.replace("line 8\n", "eight NOT-LINTABLE\n");
    let (_tmp, db, src) = setup(&[&[("a.txt", &one)], &[("a.txt", &two)]], false, Some(&["./lint.sh"]));
    let mut app = open(&db);
    lint(&mut app, "Lint the stack (./lint.sh)");
    let r = linted(&mut app, &db);
    let h = r.held.clone().unwrap();
    assert!(h.stack.is_some());
    let report = h.lint.clone().unwrap();
    assert_eq!(report.fixed, [0]);
    assert_eq!(report.errors.len(), 1);
    assert_eq!((report.errors[0].at, report.errors[0].code), (Some(1), Some(1)));
    assert_eq!(report.errors[0].lines, ["a.txt:8: NOT-LINTABLE cannot be fixed"]);
    // Listed with the landed work, where the review shows it, to read.
    keys(&mut app, &[KeyCode::Esc, KeyCode::Enter]);
    let screen = draw(&mut app);
    assert!(screen.contains(" what the linter reported, to read") && screen.contains("patch 2 · a.txt · exited with 1"), "{screen}");
    for _ in 0..10 {
        key(&mut app, KeyCode::Down);
    }
    let screen = draw(&mut app);
    assert!(screen.contains("./lint.sh at patch 2 · a.txt · exited with 1: what it reported, and did not fix"), "{screen}");
    assert!(screen.contains("a.txt:8: NOT-LINTABLE cannot be fixed"), "{screen}");
    // And by `stacked landed`.
    let out = ok(&src, &db, &["landed", "s"]);
    assert!(out.contains("  reported\tpatch 2 · a.txt · exited with 1 (./lint.sh)\n    a.txt:8: NOT-LINTABLE cannot be fixed\n"), "{out}");
    keys(&mut app, &[KeyCode::Esc]);
    choose(&mut app, "Reject all remaining and Close");
    choose(&mut app, "Reject them and close");
    assert_eq!(app.screen, Screen::Main);
    // What it reports alone lands read only: nothing to accept, only to read
    // and close.
    let (_tmp2, db2, src2) = setup(&[&[("a.txt", &lines().replace("line 8\n", "eight NOT-LINTABLE\n"))]], false, Some(&["./lint.sh"]));
    let mut app = open(&db2);
    lint(&mut app, "Lint the stack (./lint.sh)");
    let r = linted(&mut app, &db2);
    let h = r.held.clone().unwrap();
    assert!(h.stack.is_none() && h.lint.as_ref().is_some_and(|l| l.errors.len() == 1), "{h:?}");
    assert_eq!(r.summary, "the linter fixed nothing, and reported what it did not fix: at patch 1");
    let screen = draw(&mut app);
    assert!(screen.contains("1 lint of the stack waits for your review · the next: what the linter reported, by the linter: ./lint.sh"), "{screen}");
    // In the banner's read-only work; its review only to read.
    keys(&mut app, &[KeyCode::Esc]);
    let screen = draw(&mut app);
    assert!(screen.contains(" read only, to review") && screen.contains("⟳ Review what the linter reported, by the linter: ./lint.sh"), "{screen}");
    keys(&mut app, &[KeyCode::Enter]);
    let screen = draw(&mut app);
    assert!(screen.contains(" the linter's work: what the linter reported ·"), "{screen}");
    assert!(screen.contains("a.txt:8: NOT-LINTABLE cannot be fixed"), "{screen}");
    keys(&mut app, &[KeyCode::Esc]);
    assert!(menu_note(&app).contains("what the linter reported: to read, nothing to accept or reject"), "{}", menu_note(&app));
    choose(&mut app, "Close review");
    assert!(app.status.starts_with("reviewed: what the linter reported, read"), "{}", app.status);
    let d = Db::load(&db2).unwrap();
    assert!(stacked::held::queue(&d, "s").is_empty());
    assert_eq!(file_at(&src2, &db2, 1, "a.txt"), lines().replace("line 8\n", "eight NOT-LINTABLE\n"));
}

#[test]
fn an_edit_asked_while_the_linter_runs_waits_for_it_then_goes_on_from_what_it_made() {
    let (one, two) = two_fixes();
    let (_tmp, db, src) = setup(&[&[("a.txt", &one)], &[("a.txt", &two)]], false, Some(&["./lint.sh"]));
    let mut app = open(&db);
    let held = hold(&db);
    lint(&mut app, "Lint the stack (./lint.sh)");
    let r = wait_lint(&mut app, &db, |r| r.lint.as_deref().is_some_and(|j| j.at == 1));
    // Under way: the banner says where it is.
    let screen = draw(&mut app);
    assert!(screen.contains("the linter lints patch 1 of 2"), "{screen}");
    // The stack is not edited by hand meanwhile, nor linted again, as the
    // menu says; the commands refuse too.
    stack_menu(&mut app);
    let all = items(&app);
    assert!(all.contains(&("Edit the stack as a list in $EDITOR".to_string(), false)), "{all:?}");
    assert!(all.contains(&("Lint the stack (./lint.sh)".to_string(), false)), "{all:?}");
    let why = "the linter lints patch 1 of 2 (./lint.sh): the stack can be edited by hand again once the linter is done and you have reviewed what it fixed, if it fixed anything";
    assert!(menu_note(&app).contains(why), "{}", menu_note(&app));
    keys(&mut app, &[KeyCode::Esc]);
    let (fine, _, err) = stacked(&src, &db, &["drop", "s", "1"]);
    assert!(!fine && err.contains(why), "{err}");
    // An edit asked of the stack meanwhile waits its turn, behind the
    // linter's.
    let note = stacked::db::Note {
        change: stacked::db::stack_change("s"),
        id: "s-lint-test".into(),
        text: "Make it better.".into(),
        ..Default::default()
    };
    Db::edit(&db, |d| d.set_note(note.clone())).unwrap();
    let waits = stacked::ask::send(&db, "s", &[(note.change.clone(), note.id.clone())], stacked::db::Way::Change, false, stacked::db::Effort::High).unwrap();
    assert_eq!(waits.as_deref(), Some("once the linter's work on the stack lands"));
    assert!(!Db::load(&db).unwrap().runs().iter().any(|r| r.ask.is_some()), "not started");
    // What changes the stack meanwhile, as it says.
    assert_eq!(stacked::ask::changing(&Db::load(&db).unwrap(), "s").as_deref(), Some("the linter lints patch 1 of 2"));
    // An answer goes at once, as ever.
    let question = stacked::db::Note {
        change: stacked::db::stack_change("s"),
        id: "s-lint-question".into(),
        text: "What does it do?".into(),
        ..Default::default()
    };
    Db::edit(&db, |d| d.set_note(question.clone())).unwrap();
    let answer = (question.change.clone(), question.id.clone());
    assert_eq!(stacked::ask::send(&db, "s", &[answer], stacked::db::Way::Answer, false, stacked::db::Effort::High).unwrap(), None);
    for _ in 0..300 {
        let d = Db::load(&db).unwrap();
        if d.runs().iter().any(|r| r.what == "answer" && r.state != RunState::Started) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let d = Db::load(&db).unwrap();
    assert!(d.runs().iter().any(|r| r.what == "answer" && r.state == RunState::Done), "answered while the linter ran");
    assert!(lint_runs(&db).pop().is_some_and(|r| r.state == RunState::Started), "the linter still runs");
    drop(d);
    // Let go, the linter's work lands; the edit goes on from it, as edits
    // chain, without waiting for its review.
    drop(held);
    let r = wait_lint(&mut app, &db, |l| l.id == r.id && l.state != RunState::Started);
    assert!(r.held.as_ref().is_some_and(|h| h.stack.is_some()), "{}", r.summary);
    let mut asked = None;
    for _ in 0..300 {
        app.refresh();
        let d = Db::load(&db).unwrap();
        asked = d.runs().into_iter().find(|a| a.what == "change").cloned();
        if asked.as_ref().is_some_and(|a| a.state != RunState::Started) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let asked = asked.expect("the edit was asked");
    assert_eq!(stacked::ask::info(&asked).unwrap().after.as_deref(), Some(r.id.as_str()));
    // What the agent found: the stack as the linter left it.
    let saw = std::fs::read_to_string(db.with_file_name("claude-saw")).unwrap();
    assert_eq!(saw, two.replace("TWO  ", "TWO").replace("EIGHTEEN ", "EIGHTEEN"));
}

#[test]
fn a_lint_job_whose_process_is_gone_lands_as_failed_and_frees_the_stack() {
    stacked::config::init(settings());
    let (one, _) = two_fixes();
    let (_tmp, db, _src) = setup(&[&[("a.txt", &one)]], false, Some(&["./lint.sh"]));
    // A job running, its process gone, as one killed leaves it.
    stacked::lint::ask(&db, "s").unwrap();
    Db::edit(&db, |d| {
        let r = d.runs().into_iter().find(|r| r.what == stacked::db::LINT).cloned().unwrap();
        let mut j = r.lint.clone().unwrap();
        (j.step, j.rev) = (stacked::lint::Step::Running, d.head("s").map(|(rev, _)| rev));
        d.add_run(Run {
            lint: Some(j),
            pid: Some(i32::MAX as u32),
            ..r
        })
    })
    .unwrap();
    assert!(stacked::ask::blocked(&Db::load(&db).unwrap(), "s").is_some());
    // Found as an ask whose agent is gone is, and landed as failed, nothing
    // landed: the stack is free again.
    assert_eq!(stacked::ask::land_orphans(&db, &Db::load(&db).unwrap()), 1);
    let r = lint_runs(&db).pop().unwrap();
    assert_eq!((r.state, r.summary.as_str(), r.held.is_none()), (RunState::Failed, "the process linting the stack is gone: nothing landed", true));
    assert_eq!(stacked::ask::blocked(&Db::load(&db).unwrap(), "s"), None);
}

#[test]
fn one_lint_job_runs_at_a_time_the_next_waiting_its_turn() {
    let (one, two) = two_fixes();
    let (_tmp, db, _src) = setup(&[&[("a.txt", &one)], &[("a.txt", &two)]], false, Some(&["./lint.sh"]));
    let mut app = open(&db);
    let held = hold(&db);
    lint(&mut app, "Lint the stack (./lint.sh)");
    let s = wait_lint(&mut app, &db, |r| r.lint.as_deref().is_some_and(|j| j.step == stacked::lint::Step::Running));
    // Stack t's, asked meanwhile, waits its turn, as its banner says.
    stacked::lint::ask(&db, "t").unwrap();
    stacked::ask::launch(&db).unwrap();
    let t = wait_lint(&mut app, &db, |r| r.stack == "t");
    assert_eq!(t.lint.as_deref().map(|j| j.step), Some(stacked::lint::Step::Waiting));
    let d = Db::load(&db).unwrap();
    assert_eq!(stacked::ask::blocked(&d, "t").as_deref(), Some("the linter waits its turn to lint the stack (./lint.sh): the stack can be edited by hand again once the linter is done and you have reviewed what it fixed, if it fixed anything"));
    drop(d);
    // Let go: s's lands, then t's runs, and lands.
    drop(held);
    wait_lint(&mut app, &db, |r| r.id == t.id && r.state != RunState::Started);
    let runs = lint_runs(&db);
    assert!(runs.iter().all(|r| r.state == RunState::Done), "{runs:?}");
    assert!(runs.iter().find(|r| r.id == s.id).unwrap().time <= runs.iter().find(|r| r.id == t.id).unwrap().time);
    assert_eq!(lint_calls(&db).lines().filter(|l| l.starts_with("args:")).count(), 3, "s's two patches, then t's one");
}

/// The cursor on the banner's line of lint job `run`, on the stack shown.
fn on_lint_line(app: &mut App, run: &str) {
    app.refresh();
    app.screen = Screen::Main;
    app.on_banner = true;
    let line = stacked::tui::BannerLine::Lint(run.to_string());
    app.banner_sel = app.banner_lines().iter().position(|l| *l == line).unwrap_or_else(|| panic!("no line of the linter: {:?}", app.banner_lines()));
    draw(app);
}

/// The title of the menu open.
fn menu_title(app: &App) -> String {
    match &app.mode {
        Mode::Menu(m) => m.title.clone(),
        other => panic!("no menu open: {other:?}"),
    }
}

/// Waits a moment for a stopped linter to have gone on, had it not been
/// stopped: what it would have written by then.
fn linted_meanwhile(db: &Path) -> String {
    std::thread::sleep(std::time::Duration::from_millis(600));
    lint_calls(db)
}

#[test]
fn a_linter_that_hangs_is_stopped_from_the_banner_nothing_landing_and_the_edit_that_waited_going() {
    let (one, two) = two_fixes();
    let (_tmp, db, src) = setup(&[&[("a.txt", &one)], &[("a.txt", &two)]], false, Some(&["./lint.sh"]));
    let mut app = open(&db);
    let held = hold(&db);
    lint(&mut app, "Lint the stack (./lint.sh)");
    // The linter hangs, on patch 1.
    let r = wait_lint(&mut app, &db, |r| r.pid.is_some() && r.lint.as_deref().is_some_and(|j| j.at == 1));
    let (rev, before) = {
        let d = Db::load(&db).unwrap();
        (d.head("s").unwrap().0, d.stack("s").unwrap().patches.clone())
    };
    // An edit asked meanwhile waits for it.
    let note = stacked::db::Note {
        change: stacked::db::stack_change("s"),
        id: "s-stop-test".into(),
        text: "Make it better.".into(),
        ..Default::default()
    };
    Db::edit(&db, |d| d.set_note(note.clone())).unwrap();
    let waits = stacked::ask::send(&db, "s", &[(note.change.clone(), note.id.clone())], stacked::db::Way::Change, false, stacked::db::Effort::High).unwrap();
    assert_eq!(waits.as_deref(), Some("once the linter's work on the stack lands"));
    // The banner's menu stops it; so does Delete on its line, asking first.
    on_lint_line(&mut app, &r.id);
    keys(&mut app, &[KeyCode::Char(' ')]);
    assert!(items(&app).contains(&("Stop the linter…".to_string(), true)), "{:?}", items(&app));
    keys(&mut app, &[KeyCode::Esc, KeyCode::Delete]);
    assert!(menu_title(&app).starts_with("stop the linter?"), "{}", menu_title(&app));
    let note_said = menu_note(&app);
    assert!(note_said.contains("the lint job lands at once, as stopped: nothing changes in the stack, and the fixes it made so far are thrown away"), "{note_said}");
    assert!(items(&app).contains(&("Let it go on".to_string(), true)), "{:?}", items(&app));
    choose(&mut app, "Stop it");
    app.settle();
    assert_eq!(app.status, "the linter was stopped, and nothing landed: the stack is as it was, and can be edited by hand again");
    // Landed as stopped: nothing landed, the stack as it was, said in its
    // stream.
    let stopped = lint_runs(&db).pop().unwrap();
    assert_eq!(
        (stopped.id.as_str(), stopped.state, stopped.summary.as_str(), stopped.held.is_none()),
        (r.id.as_str(), RunState::Failed, stacked::lint::STOPPED, true)
    );
    let d = Db::load(&db).unwrap();
    assert_eq!((d.head("s").unwrap().0, d.stack("s").unwrap().patches.clone()), (rev, before));
    assert!(!stacked::held::queue(&d, "s").iter().any(|w| stacked::held::linted(w)));
    let said = stacked::ask::tail(&d, &r.id, 50);
    assert!(said.iter().any(|l| l.contains("you stopped the linter: its process is stopped, and nothing lands")), "{said:?}");
    drop(d);
    assert_eq!(file_at(&src, &db, 1, "a.txt"), one);
    // The linter was stopped with it: let go, it lints nothing.
    drop(held);
    assert_eq!(linted_meanwhile(&db), "", "the linter went on");
    // The edit that waited goes, from the stack as it was.
    let mut asked = None;
    for _ in 0..300 {
        app.refresh();
        let d = Db::load(&db).unwrap();
        asked = d.runs().into_iter().find(|a| a.what == "change").cloned();
        if asked.as_ref().is_some_and(|a| a.state != RunState::Started) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let asked = asked.expect("the edit that waited was asked");
    assert_eq!(stacked::ask::info(&asked).unwrap().after, None, "not from the linter's work");
    assert_eq!(std::fs::read_to_string(db.with_file_name("claude-saw")).unwrap(), two);
}

#[test]
fn a_waiting_lint_job_is_dropped_from_the_banner_a_hanging_one_stopped_by_stacked_stop_and_the_next_starts_clean() {
    let (one, two) = two_fixes();
    let (_tmp, db, src) = setup(&[&[("a.txt", &one)], &[("a.txt", &two)]], false, Some(&["./lint.sh"]));
    let mut app = open(&db);
    let held = hold(&db);
    // Stack t's job runs, its linter hanging; s's, asked then, waits its
    // turn.
    stacked::lint::ask(&db, "t").unwrap();
    stacked::ask::launch(&db).unwrap();
    let t = wait_lint(&mut app, &db, |r| r.stack == "t" && r.pid.is_some() && r.lint.as_deref().is_some_and(|j| j.at == 1));
    lint(&mut app, "Lint the stack (./lint.sh)");
    let s = wait_lint(&mut app, &db, |r| r.stack == "s");
    assert_eq!(s.lint.as_deref().map(|j| j.step), Some(stacked::lint::Step::Waiting));
    // Dropped from s's banner, its menu asking first.
    on_lint_line(&mut app, &s.id);
    let screen = draw(&mut app);
    assert!(screen.contains("◌ The linter waits its turn to lint the stack (./lint.sh)"), "{screen}");
    keys(&mut app, &[KeyCode::Char(' ')]);
    choose(&mut app, "Drop the waiting lint job…");
    assert!(menu_title(&app).starts_with("drop the waiting lint job?"), "{}", menu_title(&app));
    assert!(menu_note(&app).contains("It is taken off the queue: nothing is linted, and nothing changes in the stack."), "{}", menu_note(&app));
    choose(&mut app, "Drop it");
    app.settle();
    assert_eq!(app.status, "the waiting lint job was dropped: nothing was linted, and the stack is as it was");
    let dropped = lint_runs(&db).into_iter().find(|r| r.id == s.id).unwrap();
    assert_eq!((dropped.state, dropped.summary.as_str(), dropped.held.is_none()), (RunState::Failed, stacked::lint::DROPPED, true));
    assert_eq!(stacked::ask::blocked(&Db::load(&db).unwrap(), "s"), None);
    // t's, stopped by `stacked stop`, its run as `stacked runs` lists it.
    let rev = Db::load(&db).unwrap().head("t").unwrap().0;
    let out = ok(&src, &db, &["stop", &t.id]);
    assert!(out.contains("\tlint t \"./lint.sh\"\tfailed\t") && out.contains(stacked::lint::STOPPED), "{out}");
    let stopped = lint_runs(&db).into_iter().find(|r| r.id == t.id).unwrap();
    assert_eq!((stopped.state, stopped.summary.as_str(), stopped.held.is_none()), (RunState::Failed, stacked::lint::STOPPED, true));
    // Nothing landed: the stack as it was, and free again, edited by hand.
    let d = Db::load(&db).unwrap();
    assert_eq!(d.head("t").unwrap().0, rev);
    assert!(stacked::held::queue(&d, "t").is_empty());
    assert_eq!(stacked::ask::blocked(&d, "t"), None);
    drop(d);
    // Over now: stopped again, it is refused.
    let (fine, _, err) = stacked(&src, &db, &["stop", &t.id]);
    assert!(!fine && err.contains("is not running: the lint job is over, failed: you stopped the linter"), "{err}");
    // Its linter was stopped with it: let go, it lints nothing.
    drop(held);
    assert_eq!(linted_meanwhile(&db), "", "the linter went on");
    // What a linter left in t's worktree, a file changed and one new, is
    // thrown away as the next job starts: it lints the stack as it is.
    let at = src.canonicalize().unwrap().join(".stacked/t/lint");
    std::fs::write(at.join("b.txt"), "left behind  \n").unwrap();
    std::fs::write(at.join("stray.txt"), "left behind\n").unwrap();
    stacked::lint::ask(&db, "t").unwrap();
    stacked::ask::launch(&db).unwrap();
    let again = wait_lint(&mut app, &db, |r| r.stack == "t" && r.id != t.id && r.state != RunState::Started);
    assert_eq!(again.state, RunState::Done, "{}", again.summary);
    let h = again.held.clone().unwrap();
    assert_eq!(h.stack.as_ref().map(|c| c.op.as_str()), Some("lint fixes in patch 1 (./lint.sh)"));
    assert!(h.lint.as_ref().is_some_and(|l| !l.on_top && l.stopped.is_none()), "{:?}", h.lint);
    assert!(!at.join("stray.txt").exists());
    accept(&db, &again);
    assert_eq!(ok(&src, &db, &["show", "t", "1", "--path", "a.txt"]), one.replace("TWO  ", "TWO"));
    let d = Db::load(&db).unwrap();
    let p1 = d.patch(&d.stack("t").unwrap().patches[0]);
    assert!(!p1.diff.contains("stray.txt") && !p1.diff.contains("b.txt"), "{}", p1.diff);
}
