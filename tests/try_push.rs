//! The try push, built with the `try-push` feature: a comment asked as a
//! try push, the agent's stand-in selecting its tasks, each with why; the
//! tasks landing for review, marked, answered and sent back; and closing
//! the review with them, which pushes them: `./mach try fuzzy --no-push`
//! checked, then the push, its login shown, its link a reply, and the
//! comment locked until you stop following it. The editor is drawn into a
//! buffer and driven by keys, as in `tests/tui.rs`; a stand-in answers for
//! mach (`fake_mach`), and the repository's remote of Firefox's is a name
//! and a ref, never fetched: no test reaches an agent, mach, Lando or
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
/// the temporary directory in `tmp` beside it; and, as `tests/cli.rs` runs
/// it, the folders of its asks' own (a try push's copy of the task graph)
/// in `todo` beside it (`$STACKED_TODO`), never in the temporary
/// directory's, which every stacked shares.
fn stacked(dir: &Path, db: &Path, args: &[&str]) {
    stacked_with(dir, db, args, &[]);
}

/// As `stacked`, with `env` besides; what it printed.
fn stacked_with(dir: &Path, db: &Path, args: &[&str], env: &[(&str, &Path)]) -> String {
    let out = stacked_out(dir, db, args, env);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// As `stacked_with`, whether it succeeds or not: how it ended.
fn stacked_out(dir: &Path, db: &Path, args: &[&str], env: &[(&str, &Path)]) -> std::process::Output {
    let tmp = db.with_file_name("tmp");
    std::fs::create_dir_all(&tmp).unwrap();
    Command::new(env!("CARGO_BIN_EXE_stacked"))
        .current_dir(dir)
        .arg("-f")
        .arg(db)
        .args(args)
        .env("STACKED_HOME", db.with_file_name("home"))
        .env("TMPDIR", tmp)
        .env("STACKED_TODO", db.with_file_name("todo"))
        .env_remove("STACKED_CONFIG")
        .env_remove("STACKED_APP_STATE")
        .envs(env.iter().copied())
        .output()
        .unwrap()
}

/// A database holding stack `s` of two patches, and the repository: its
/// base on `origin`, a remote mach takes as Firefox's, by its address, as
/// if fetched (a ref of it at the base), never reached.
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
    git(&src, &["remote", "add", "origin", "https://github.com/mozilla-firefox/firefox"]);
    git(&src, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    std::fs::write(src.join("a.txt"), format!("one\nTWO\nthree\n{}", lines.replace("line 39\n", "LINE 39\n"))).unwrap();
    git(&src, &["commit", "-qam", "Bug 1 - shout two\n\nBecause."]);
    std::fs::write(src.join("b.txt"), "bee\n").unwrap();
    git(&src, &["add", "."]);
    git(&src, &["commit", "-qm", "Bug 2 - add b"]);
    let db = tmp.path().join("db.jsonl");
    stacked(&src, &db, &["import", "s", "HEAD~2"]);
    (tmp, db, src)
}

/// The settings of these tests: the defaults, with which every ask goes
/// as the first does, no setup before it.
fn settings() -> stacked::config::Config {
    stacked::config::Config::default()
}

/// The editor on `db`, with the tests' settings and the stand-ins for
/// claude, for mach and for the program that opens links.
fn open(db: &Path) -> Editor {
    stacked::config::init(settings());
    fake_claude();
    fake_mach();
    fake_opener();
    fake_web();
    // A push followed is looked at each time the editor launches what may go.
    stacked::try_push::look_every(0);
    Editor(App::with_config(db, settings()).unwrap())
}

/// A push as the stand-in for Lando and Treeherder has it, by its Lando
/// job: Lando's state of it, the revision it pushed and why it failed; then
/// Treeherder's push of it, if it has it, and its jobs, each a label, a
/// state, a result, a task and its run; and the pages read of it.
#[derive(Clone, Default)]
struct Pushed {
    status: String,
    revision: String,
    error: String,
    push: Option<u64>,
    jobs: Vec<(String, String, String, String, u64)>,
    gets: Vec<String>,
}

/// The pushes the stand-in for Lando and Treeherder has, by Lando job.
static PUSHED: std::sync::Mutex<std::collections::BTreeMap<u64, Pushed>> = std::sync::Mutex::new(std::collections::BTreeMap::new());

/// Each test's own Lando job, for the stand-in for mach to push as.
fn new_job(db: &Path) -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(5000);
    let job = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    std::fs::write(db.with_file_name("mach-job"), job.to_string()).unwrap();
    PUSHED.lock().unwrap().insert(job, Pushed { status: "SUBMITTED".into(), ..Default::default() });
    job
}

/// Push `job` as the stand-in has it, changed by `change`.
fn pushed(job: u64, change: impl FnOnce(&mut Pushed)) {
    change(PUSHED.lock().unwrap().get_mut(&job).unwrap());
}

/// The pages of push `job` read, as the stand-in was asked for them.
fn gets(job: u64) -> Vec<String> {
    PUSHED.lock().unwrap().get(&job).map(|p| p.gets.clone()).unwrap_or_default()
}

/// The stand-in for Lando and Treeherder, for every test of this file: it
/// answers the plain GETs that follow a push, from `PUSHED`.
fn fake_web() {
    static SET: std::sync::Once = std::sync::Once::new();
    SET.call_once(|| stacked::try_push::web_stand_in(Some(Box::new(answer))));
}

/// What the stand-in answers for `url`, as Lando's and Treeherder's APIs
/// would: a Lando job; Treeherder's push of a revision; a push's jobs.
fn answer(url: &str) -> Result<String, String> {
    let mut all = PUSHED.lock().unwrap();
    if let Some(job) = url.strip_prefix("https://lando.moz.tools/landing_jobs/") {
        let job: u64 = job.trim_end_matches('/').parse().map_err(|_| format!("no Lando job in {url}"))?;
        let p = all.get_mut(&job).ok_or_else(|| format!("Lando has no job {job}"))?;
        p.gets.push(url.to_string());
        let landed = p.status == "LANDED";
        return Ok(serde_json::json!({
            "id": job,
            "status": p.status,
            "commit_id": if landed { p.revision.clone() } else { String::new() },
            "error": p.error,
            "repository": "try",
            "url": format!("https://lando.moz.tools/landings/{job}"),
        })
        .to_string());
    }
    if let Some(rev) = url.strip_prefix("https://treeherder.mozilla.org/api/project/try/push/?revision=") {
        let p = all.values_mut().find(|p| p.revision == rev).ok_or_else(|| format!("no push of {rev}"))?;
        p.gets.push(url.to_string());
        let results: Vec<serde_json::Value> = p.push.map(|id| serde_json::json!({"id": id, "revision": rev})).into_iter().collect();
        return Ok(serde_json::json!({"meta": {"revision": rev, "count": results.len()}, "results": results}).to_string());
    }
    if let Some(rest) = url.strip_prefix("https://treeherder.mozilla.org/api/jobs/?push_id=") {
        let id: u64 = rest.split('&').next().unwrap_or("").parse().map_err(|_| format!("no push in {url}"))?;
        let p = all.values_mut().find(|p| p.push == Some(id)).ok_or_else(|| format!("Treeherder has no push {id}"))?;
        p.gets.push(url.to_string());
        let rows: Vec<serde_json::Value> = p.jobs.iter().map(|(label, state, result, task, retry)| serde_json::json!([label, state, result, task, retry])).collect();
        return Ok(serde_json::json!({
            "count": rows.len(),
            "next": null,
            "previous": null,
            "results": rows,
            "job_property_names": ["job_type_name", "state", "result", "task_id", "retry_id"],
        })
        .to_string());
    }
    Err(format!("the stand-in for Lando and Treeherder does not answer {url}"))
}

/// A job as the stand-in's Treeherder has it.
fn treeherder_job(label: &str, state: &str, result: &str, task: &str, retry: u64) -> (String, String, String, String, u64) {
    (label.into(), state.into(), result.into(), task.into(), retry)
}

/// The editor launching what may go, the pushes followed looked at, until
/// `there` holds of the try push as the database has it.
fn look_until(app: &mut App, db: &Path, there: impl Fn(&stacked::db::Run) -> bool) -> stacked::db::Run {
    for _ in 0..100 {
        app.launch();
        app.settle();
        if let Some(r) = push_run(db).filter(|r| there(r)) {
            return r;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    panic!("the try push followed never got there: {:?}", push_run(db));
}

/// The stand-in for mach, for every test of this file, run as `./mach try
/// fuzzy` would be in the push's worktree. It writes what it is run with in
/// `mach-calls` beside the test's database: its arguments; whether it could
/// read anything; whether its output is held back; how many commits mach
/// would push; the presets file it is given, and what that holds. Then it
/// says what mach says: with `--no-push`, the try_task_config.json of the
/// tasks its queries name, how many times each runs, and the priority the
/// preset gives; else its login's page and code, waiting while `mach-hold`
/// is there, then the push's link. `mach-mode` beside the database has it
/// leave the last task out (`drop`), ask a question (`ask`), or fail as it
/// pushes (`fail`).
fn fake_mach() -> PathBuf {
    static FAKE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    FAKE.get_or_init(|| {
        let script = r#"#!/bin/sh
git=$(git rev-parse --path-format=absolute --git-common-dir)
dir="$git/../.."
mode=$(cat "$dir/mach-mode" 2>/dev/null)
{
  printf 'args:'
  for a in "$@"; do printf ' %s' "$a"; done
  echo
  if read -r line; then echo "stdin: open"; else echo "stdin: closed"; fi
  echo "unbuffered: ${PYTHONUNBUFFERED:-}"
  echo "commits: $(git rev-list --count HEAD --not --remotes=origin)"
  if [ -n "$MACH_TRY_PRESET_PATHS" ]; then
    echo "presets: $MACH_TRY_PRESET_PATHS"
    tr -d '\n' < "$MACH_TRY_PRESET_PATHS"
    echo
  fi
} >> "$dir/mach-calls"
check=no; rebuild=""; preset=""; labels=""
while [ $# -gt 0 ]; do
  case "$1" in
    --no-push) check=yes ;;
    --rebuild) shift; rebuild="$1" ;;
    --preset) shift; preset="$1" ;;
    -q) shift; labels="$labels$(printf '%s' "$1" | tr ' ' '\n' | grep -v '^|$')
" ;;
  esac
  shift
done
if [ "$mode" = ask ]; then
  echo "warning: failed to detect current user for 'hg.mozilla.org'"
  echo "Pernosco requires a Mozilla e-mail address to view its reports."
  printf 'Do you have an @mozilla.com address? [Y/n]: '
  if ! read -r answer; then
    echo 'Traceback (most recent call last):' >&2
    echo 'EOFError: EOF when reading a line' >&2
    exit 1
  fi
fi
if [ "$check" = yes ]; then
  echo "Commit message:"
  echo "Fuzzy query=..."
  echo
  echo 'Pushed via `mach try fuzzy`'
  echo "Calculated try_task_config.json:"
  tasks=$(printf '%s' "$labels" | sed 's/^\^//; s/\$$//' | LC_ALL=C sort)
  if [ "$mode" = drop ]; then tasks=$(printf '%s\n' "$tasks" | sed '$d'); fi
  priority=""
  if [ -n "$preset" ] && grep -q '"lowest"' "$MACH_TRY_PRESET_PATHS"; then priority=lowest; fi
  echo "{"
  echo '    "parameters": {'
  echo '        "optimize_target_tasks": false,'
  echo '        "try_task_config": {'
  echo '            "env": {"TRY_SELECTOR": "fuzzy"},'
  if [ -n "$priority" ]; then echo "            \"priority\": \"$priority\","; fi
  if [ -n "$rebuild" ]; then echo "            \"rebuild\": $rebuild,"; fi
  echo '            "tasks": ['
  printf '%s\n' "$tasks" | sed '/^$/d; s/.*/                "&",/; $ s/,$//'
  echo '            ]'
  echo '        }'
  echo '    },'
  echo '    "version": 2'
  echo "}"
  exit 0
fi
echo "Pushing to try..."
if [ "$mode" = fail ]; then
  echo "abort: error submitting patches to Lando."
  echo "Lando returned HTTP 503 (Service Unavailable)."
  exit 1
fi
echo "1. On your computer or mobile device navigate to: https://auth.example.com/activate?user_code=WXYZ-1234"
echo "2. Enter the following code: WXYZ-1234"
printf 'Waiting...'
while [ -e "$dir/mach-hold" ]; do sleep 0.1; printf '.'; done
echo
echo "Submitting stack of $(git rev-list --count HEAD --not --remotes=origin) nodes and the try commit."
echo "try submission success in 2.0s:"
echo " https://treeherder.mozilla.org/jobs?repo=try&landoInstance=lando-prod-2025&landoCommitID=$(cat "$dir/mach-job" 2>/dev/null || echo 4242)"
"#;
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR"));
        let (path, part) = (dir.join("try-mach.sh"), dir.join(format!("try-mach-{}.sh", std::process::id())));
        std::fs::write(&part, script).unwrap();
        Command::new("chmod").arg("+x").arg(&part).status().unwrap();
        std::fs::rename(&part, &path).unwrap();
        stacked::try_push::stand_in(&path);
        path
    })
    .clone()
}

/// What the stand-in for mach was run with, as `fake_mach` wrote it beside
/// `db`: each run's arguments.
fn mach_args(db: &Path) -> Vec<String> {
    mach_calls(db).lines().filter_map(|l| l.strip_prefix("args: ")).map(String::from).collect()
}

/// All that the stand-in for mach wrote of how it was run, beside `db`.
fn mach_calls(db: &Path) -> String {
    std::fs::read_to_string(db.with_file_name("mach-calls")).unwrap_or_default()
}

/// The try push's run on `db`, the last one.
fn push_run(db: &Path) -> Option<stacked::db::Run> {
    let d = Db::load(db).unwrap();
    d.runs().into_iter().rev().find(|r| r.what == stacked::db::TRY_PUSH).cloned()
}

/// Waits for the try push on `db` to be where `there` says, the editor
/// looking meanwhile: its run then.
fn wait_push(app: &mut App, db: &Path, there: impl Fn(&stacked::db::Run) -> bool) -> stacked::db::Run {
    for _ in 0..300 {
        app.refresh();
        if let Some(r) = push_run(db).filter(|r| there(r)) {
            app.refresh();
            return r;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    panic!("the try push never got there: {:?}\n{}", push_run(db), mach_calls(db));
}

/// The cursor moved onto the head of comment `id`, in the middle.
fn to_comment(app: &mut App, id: &str) {
    if app.focus != stacked::tui::Focus::Diff {
        app.focus = stacked::tui::Focus::Diff;
    }
    draw(app);
    let at = app.rows.iter().position(|r| r.note.as_deref() == Some(id) && r.head).unwrap_or_else(|| panic!("no comment {id}"));
    for _ in 0..app.rows.len() {
        match app.cursor.cmp(&at) {
            std::cmp::Ordering::Less => key(app, KeyCode::Down),
            std::cmp::Ordering::Greater => key(app, KeyCode::Up),
            std::cmp::Ordering::Equal => break,
        };
        draw(app);
    }
    assert_eq!(app.cursor, at);
}

/// Whether comment `id` of patch 1 is locked, as one the agent works on.
fn locked(db: &Path, id: &str) -> bool {
    let d = Db::load(db).unwrap();
    stacked::ask::with_agent(&d).keys().any(|(_, i)| i == id)
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
/// what `answer_with` wrote beside the test's database, once no `hold` is
/// there. First it writes down its arguments there (`claude_args`), and,
/// as the agent would, downloads the task graph where its prompt says:
/// `{}`, nothing reached.
fn fake_claude() -> PathBuf {
    static FAKE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    FAKE.get_or_init(|| {
        let result = serde_json::to_string(&serde_json::json!({"type": "result", "result": r#"{"replies": {"c1": "Seen."}}"#})).unwrap();
        let script = format!(
            "#!/bin/sh\necho '{{\"type\":\"system\",\"subtype\":\"init\",\"model\":\"fake\"}}'\n\
             git=$(git rev-parse --path-format=absolute --git-common-dir)\n\
             printf '%s\\n' \"$@\" > \"$git/../../claude-args-$$\"\n\
             graph=$(printf '%s\\n' \"$2\" | sed -n 's/.*download it once into `\\([^`]*\\)`.*/\\1/p')\n\
             test -z \"$graph\" || echo '{{}}' > \"$graph\"\n\
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

/// What git says in `dir`, run with `args`, trimmed.
fn git_out(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git").args(args).current_dir(dir).output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// The title of the menu open.
fn menu_title(app: &App) -> String {
    match &app.mode {
        Mode::Menu(m) => m.title.clone(),
        other => panic!("no menu open: {other:?}"),
    }
}

/// The choices of how the agent is asked, of the menu open's `Ask agent`.
fn hows(app: &App) -> Vec<String> {
    let Mode::Menu(m) = &app.mode else { panic!("no menu open: {:?}", app.mode) };
    let ask = m.items.iter().find(|i| i.label == "Ask agent").unwrap_or_else(|| panic!("no Ask agent in {:?}", items(app)));
    ask.choices[0].list.iter().map(|c| c.shown.clone()).collect()
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

/// The last turn of the thread of patch 1's first comment, as written,
/// the agent's or a try push's.
fn last_turn(db: &Path) -> String {
    let n = notes_of(db, 0).remove(0);
    stacked::thread::thread_turns(&n.text).pop().map(|t| t.body).unwrap_or_default()
}

/// The prompt the try push's last ask was given.
fn last_prompt(db: &Path) -> String {
    let d = Db::load(db).unwrap();
    let r = d.runs().into_iter().rev().find(|r| r.what == "try").cloned().expect("a try push's ask");
    stacked::ask::info(&r).unwrap().prompt
}

/// A comment on patch 1 as a whole, `text`, written from the patches pane,
/// and asked as a try push as it is saved.
fn ask_try(app: &mut App, text: &str) {
    keys(app, &[KeyCode::Home, KeyCode::Down]);
    choose(app, "Comment on this patch…");
    typed(app, text);
    keys(app, &[KeyCode::Esc]);
    choose_as(app, "Ask agent", "try push");
}

/// A comment on the stack, `text`, written from the patches pane, and
/// asked as a try push as it is saved.
fn ask_try_on_stack(app: &mut App, text: &str) {
    keys(app, &[KeyCode::Home]);
    choose(app, "Comment on the stack…");
    typed(app, text);
    keys(app, &[KeyCode::Esc]);
    choose_as(app, "Ask agent", "try push");
}

/// What the stand-in for claude was run with by each ask, as it wrote it
/// down beside the database: its arguments, a line each, the prompt one of
/// them.
fn claude_args(db: &Path) -> Vec<String> {
    std::fs::read_dir(db.parent().unwrap())
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("claude-args-"))
        .map(|e| std::fs::read_to_string(e.path()).unwrap_or_default())
        .collect()
}

/// The try push asks going on `db`, once there are `n` and the stand-in
/// of each has downloaded its copy of the task graph: each by its run's
/// id, with the folder of its own that copy is in. The editor launches
/// what waits meanwhile.
fn tries_going(app: &mut App, db: &Path, n: usize) -> Vec<(String, PathBuf)> {
    let mut going = Vec::new();
    for _ in 0..300 {
        app.refresh();
        let d = Db::load(db).unwrap();
        going = d
            .runs()
            .into_iter()
            .filter(|r| r.what == "try" && r.state == stacked::db::RunState::Started)
            .filter_map(|r| Some((r.id.clone(), PathBuf::from(stacked::ask::info(r)?.todo?))))
            .collect();
        if going.len() == n && going.iter().all(|(_, dir)| dir.join(stacked::try_push::GRAPH).exists()) {
            return going;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    panic!("not {n} try push asks going, each its copy of the task graph downloaded: {going:?}");
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
    // The how of a comment on patch 1 as a whole: try push, beside the
    // others.
    keys(&mut app, &[KeyCode::Home, KeyCode::Down]);
    choose(&mut app, "Comment on this patch…");
    save(&mut app, "Run what tests a.txt, as many as you see fit.");
    to_comment(&mut app, &notes_of(&db, 0)[0].id);
    keys(&mut app, &[KeyCode::Char(' ')]);
    assert_eq!(hows(&app), ["change", "the whole stack", "answer only", "build and test", "try push"]);
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
    // Its tasks, in place of those sent back; all accepted, then closed:
    // they are pushed.
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
    assert!(!note.contains("not built yet"), "{note}");
    choose(&mut app, "Accept them and close");
    assert!(
        app.status.starts_with("reviewed: 3 tasks accepted; 1 reply accepted; the try push has started: the comment that asked shows what mach writes"),
        "{}",
        app.status
    );
    assert!(stacked::held::queue(&Db::load(&db).unwrap(), "s").is_empty());
    let r = wait_push(&mut app, &db, |r| r.push.as_ref().is_some_and(|p| p.link.is_some()));
    assert_eq!(r.push.unwrap().labels.len(), 3);
}

#[test]
fn each_try_push_ask_downloads_the_task_graph_into_a_folder_of_its_own_gone_once_it_lands_or_is_stopped() {
    let (_tmp, db, src) = setup();
    let mut app = open(&db);
    tasks_answer(&db, &[("build-linux64/debug", "The build the tests need.")], "One task.");
    // Two asks at once, held as they go: patch 1's and the stack's.
    let hold = db.with_file_name("hold");
    std::fs::write(&hold, "").unwrap();
    ask_try(&mut app, "Run the build.");
    assert!(app.status.starts_with("asked the agent about 1 comment (try push)"), "{}", app.status);
    ask_try_on_stack(&mut app, "Run the build of the stack.");
    assert!(app.status.starts_with("asked the agent about 1 comment (try push)"), "{}", app.status);
    let going = tries_going(&mut app, &db, 2);
    assert_ne!(going[0].1, going[1].1, "two asks, two folders");
    // Each folder its own, beside the rebase plans' (the test's own, beside
    // its database), outside your checkout: its prompt names the copy in it,
    // to download and to read, and claude is let write there (`--add-dir`).
    let plans = db.canonicalize().unwrap().with_file_name("stacked-todo");
    let args = claude_args(&db);
    let d = Db::load(&db).unwrap();
    for (id, dir) in &going {
        assert_eq!(dir.parent(), Some(plans.as_path()), "{}", dir.display());
        let copy = dir.join("full-task-graph.json");
        let prompt = stacked::ask::info(d.run(id).unwrap()).unwrap().prompt;
        for said in [
            format!("download it once into `{}`, a scratch file in a folder of your own, the one place you may write", copy.display()),
            format!("`curl -sL --compressed -o {}` with its address", copy.display()),
            format!("`jq '[\"LABEL\", …] - keys' {}`", copy.display()),
        ] {
            assert!(prompt.contains(&said), "{said:?} in {prompt}");
        }
        assert!(args.iter().any(|a| a.contains(&format!("\n--add-dir\n{}\n", dir.display()))), "{args:?}");
    }
    // Nothing written in your checkout.
    assert_eq!(git_out(&src, &["status", "--porcelain"]), "");
    // Landed, each folder is gone, its copy with it.
    std::fs::remove_file(&hold).unwrap();
    landed(&mut app, &db, 2);
    for (_, dir) in &going {
        assert!(!dir.exists(), "{}", dir.display());
    }
    assert_eq!(git_out(&src, &["status", "--porcelain"]), "");
    // Stopped as it goes, its folder goes too.
    std::fs::write(&hold, "").unwrap();
    ask_try_on_stack(&mut app, "Run it on Windows too.");
    let going = tries_going(&mut app, &db, 1);
    let (run, stopped) = stacked::ask::stop(&db, &going[0].0).unwrap();
    assert!(stopped && run.state == stacked::db::RunState::Failed, "{run:?}");
    assert!(!going[0].1.exists(), "{}", going[0].1.display());
    std::fs::remove_file(&hold).unwrap();
    assert_eq!(git_out(&src, &["status", "--porcelain"]), "");
}

#[test]
fn a_try_push_ask_works_in_a_worktree_of_its_own_your_checkout_as_it_was_while_it_works_and_after() {
    let (_tmp, db, src) = setup();
    let mut app = open(&db);
    tasks_answer(&db, &[("build-linux64/debug", "The build the tests need.")], "One task.");
    // A change of yours, not committed: it stays as it is.
    std::fs::write(src.join("b.txt"), "mine\n").unwrap();
    let yours = || [&["rev-parse", "HEAD"][..], &["symbolic-ref", "--short", "HEAD"], &["status", "--porcelain", "--untracked-files=all"]].map(|a| git_out(&src, a));
    let before = yours();
    assert_eq!(before[1..], ["main".to_string(), "M b.txt".to_string()]);
    let hold = db.with_file_name("hold");
    std::fs::write(&hold, "").unwrap();
    ask_try(&mut app, "Run the build.");
    let going = tries_going(&mut app, &db, 1);
    // Its own worktree, the one for answers, the patches up to the
    // comment's applied there: patch 1 at its HEAD.
    let wt = src.canonicalize().unwrap().join(".stacked/s/answer-only-agent");
    let d = Db::load(&db).unwrap();
    let i = stacked::ask::info(d.run(&going[0].0).unwrap()).unwrap();
    assert_eq!(i.worktree.as_deref(), Some(wt.to_str().unwrap()));
    drop(d);
    assert_eq!(git_out(&wt, &["log", "-1", "--format=%s"]), "Bug 1 - shout two");
    assert!(last_prompt(&db).contains("The current directory is a git worktree made for you"), "{}", last_prompt(&db));
    assert_eq!(yours(), before, "your checkout while the agent works");
    std::fs::remove_file(&hold).unwrap();
    landed(&mut app, &db, 1);
    assert_eq!(yours(), before, "and after");
}

#[test]
fn a_try_push_ask_is_not_given_the_checkout_s_linter_as_a_change_is() {
    let (_tmp, db, src) = setup();
    stacked(&src, &db, &["setup", "lint", "eslint", "--fix"]);
    let mut app = open(&db);
    assert_eq!(stacked::lint::Linter::of(&src).rule().as_deref(), Some("Bash(eslint *)"), "a change would be given it");
    tasks_answer(&db, &[("build-linux64/debug", "The build the tests need.")], "One task.");
    ask_try(&mut app, "Run the build.");
    landed(&mut app, &db, 1);
    // It reads only, as an answer does: what it may use holds no linter,
    // and its prompt says none.
    let args = claude_args(&db);
    assert_eq!(args.len(), 1, "{args:?}");
    let tools = args[0].lines().skip_while(|l| *l != "--allowedTools").nth(1).unwrap_or_default().to_string();
    assert!(tools.contains("Read,") && !tools.contains("Bash(eslint *)"), "{tools}");
    assert!(!last_prompt(&db).contains("This checkout's linter"), "{}", last_prompt(&db));
}

/// Comment `id` of patch 1 put on the queue as a try push, as an older
/// stacked let a comment on a line be: in the batch (`Stage::Batch`), or
/// ready to go (`Stage::Ready`).
fn queued_as_try(db: &Path, id: &str, stage: stacked::db::Stage) {
    let n = notes_of(db, 0).into_iter().find(|n| n.id == id).unwrap();
    let queued = stacked::db::Queued {
        way: stacked::db::Way::Try,
        batch: format!("b-{id}"),
        stack: false,
        stage,
        on: None,
        was: None,
        effort: None,
    };
    Db::edit(db, |d| d.set_note(Note { queued: Some(queued), failed: None, ..n })).unwrap();
}

#[test]
fn a_try_push_is_asked_of_the_stack_or_a_patch_as_a_whole_never_of_a_line() {
    let (_tmp, db, src) = setup();
    let mut app = open(&db);
    // A comment on the stack, or on patch 1 as a whole, may be asked as a
    // try push, as it may be sent to build and test.
    keys(&mut app, &[KeyCode::Home]);
    choose(&mut app, "Comment on the stack…");
    typed(&mut app, "Run it all.");
    keys(&mut app, &[KeyCode::Esc]);
    assert_eq!(hows(&app), ["change", "answer only", "build and test", "try push"]);
    choose(&mut app, "Save it");
    keys(&mut app, &[KeyCode::Home, KeyCode::Down]);
    choose(&mut app, "Comment on this patch…");
    typed(&mut app, "Run its tests.");
    keys(&mut app, &[KeyCode::Esc]);
    assert_eq!(hows(&app), ["change", "the whole stack", "answer only", "build and test", "try push"]);
    choose(&mut app, "Save it");
    // One on a line may not: neither as it is written, nor on its menu.
    keys(&mut app, &[KeyCode::Right]);
    down_to(&mut app, "+TWO");
    keys(&mut app, &[KeyCode::Enter]);
    typed(&mut app, "Test this line.");
    keys(&mut app, &[KeyCode::Esc]);
    assert_eq!(hows(&app), ["change", "the whole stack", "answer only"]);
    choose(&mut app, "Save it");
    keys(&mut app, &[KeyCode::Down, KeyCode::Char(' ')]);
    assert_eq!(hows(&app), ["change", "the whole stack", "answer only"]);
    keys(&mut app, &[KeyCode::Esc]);
    // Batched as a try push, as an older stacked let it be: sent from the
    // banner, it is refused, saying why. It stays a comment, out of the
    // batch, its ask failed with that reason.
    let line = notes_of(&db, 0).into_iter().find(|n| n.text == "Test this line.").unwrap();
    let why = stacked::ask::TRY_OF_A_PATCH;
    let refused = |db: &Path| notes_of(db, 0).into_iter().find(|n| n.id == line.id).map(|n| (n.queued.is_none(), n.failed));
    queued_as_try(&db, &line.id, stacked::db::Stage::Batch);
    app.refresh();
    keys(&mut app, &[KeyCode::Esc]);
    keys(&mut app, &[KeyCode::Char(' ')]);
    choose(&mut app, "Send 1 try push, each on its own");
    assert_eq!(app.status, format!("1 comment not sent: {why}"));
    assert_eq!(refused(&db), Some((true, Some(why.to_string()))));
    // Checked in the batch again, as it was batched last: refused so too.
    app.on_banner = true;
    keys(&mut app, &[KeyCode::Char(' ')]);
    let toggle = items(&app).into_iter().map(|(l, _)| l).find(|l| l.contains("\"Test this line.\"")).unwrap();
    choose(&mut app, &toggle);
    assert_eq!(app.status, why);
    assert_eq!(refused(&db), Some((true, Some(why.to_string()))));
    keys(&mut app, &[KeyCode::Esc]);
    // Ready to go: refused as it would go.
    queued_as_try(&db, &line.id, stacked::db::Stage::Ready);
    stacked::ask::launch(&db).unwrap();
    assert_eq!(refused(&db), Some((true, Some(why.to_string()))));
    // The command line's batch refuses it too, saying why, and fails.
    queued_as_try(&db, &line.id, stacked::db::Stage::Batch);
    let out = stacked_out(&src, &db, &["send", "s", "--batch"], &[]);
    let (said, err) = (String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(!out.status.success() && err.contains("1 of them was not sent"), "{err}");
    assert!(said.contains(&format!("\tnot sent (try push): {why}")), "{said}");
    assert_eq!(refused(&db), Some((true, Some(why.to_string()))));
    // Nothing was asked.
    assert!(Db::load(&db).unwrap().runs().iter().all(|r| r.what != "try"));
}

#[test]
fn a_try_push_accepted_is_checked_then_pushed_its_login_shown_its_link_a_reply_until_you_stop_following_it() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db);
    tasks_answer(
        &db,
        &[("test-linux2404-64/opt-xpcshell-1", "It runs the tests of a.txt."), ("build-linux64/debug", "The build the tests need.")],
        "Two tasks.",
    );
    ask_try(&mut app, "Run the tests of a.txt.");
    landed(&mut app, &db, 1);
    let id = notes_of(&db, 0)[0].id.clone();
    // mach to wait at its login, for it to be seen.
    std::fs::write(db.with_file_name("mach-hold"), "").unwrap();
    keys(&mut app, &[KeyCode::Esc, KeyCode::Enter]);
    assert_eq!(app.screen, Screen::Review);
    keys(&mut app, &[KeyCode::Esc]);
    choose(&mut app, "Accept all remaining and Close");
    choose(&mut app, "Accept them and close");
    assert!(app.status.contains("the try push has started"), "{}", app.status);
    // Checked with --no-push, then pushed: the login waited for, its page
    // and code in the status line and in the comment's stream.
    let r = wait_push(&mut app, &db, |r| r.push.as_ref().is_some_and(|p| p.login.is_some()));
    let login = r.push.as_ref().unwrap().login.clone().unwrap();
    assert_eq!((login.url.as_str(), login.code.as_str()), ("https://auth.example.com/activate?user_code=WXYZ-1234", "WXYZ-1234"));
    assert_eq!(
        app.status,
        "mach waits for you to log in to push to try: open https://auth.example.com/activate?user_code=WXYZ-1234 and enter the code WXYZ-1234 (the comment's menu has Open the login page)"
    );
    // One query, a term for each task, fzf's OR between them.
    let labels = "-q ^test-linux2404-64/opt-xpcshell-1$ | ^build-linux64/debug$";
    assert_eq!(
        mach_args(&db),
        [format!("try fuzzy --no-push --show-chunk-numbers --all-tasks {labels}"), format!("try fuzzy --show-chunk-numbers --all-tasks {labels}")]
    );
    let calls = mach_calls(&db);
    for said in ["stdin: closed", "unbuffered: 1", "commits: 1"] {
        assert_eq!(calls.lines().filter(|l| *l == said).count(), 2, "{said:?} in {calls}");
    }
    assert!(!calls.contains("presets:"), "no preset under the threshold: {calls}");
    to_comment(&mut app, &id);
    let screen = draw(&mut app);
    for said in ["try push: mach waits for your login", "2. Enter the following code: WXYZ-1234"] {
        assert!(screen.contains(said), "{said:?} in:\n{screen}");
    }
    assert!(locked(&db, &id), "locked while pushed");
    // Its menu: what mach writes, the login page to open, the push to stop.
    keys(&mut app, &[KeyCode::Char(' ')]);
    let all = items(&app);
    for label in ["Read what mach writes for the try push", "Open the login page…", "Stop the try push…"] {
        assert!(all.iter().any(|(l, ok)| l == label && *ok), "{label:?} in {all:?}");
    }
    choose(&mut app, "Open the login page…");
    assert!(draw(&mut app).contains("https://auth.example.com/activate?user_code=WXYZ-1234"), "{}", draw(&mut app));
    keys(&mut app, &[KeyCode::Enter]);
    app.settle();
    assert_eq!(app.status, "opened https://auth.example.com/activate?user_code=WXYZ-1234 in your browser");
    // Logged in: pushed, its link a reply, a web link, the comment still
    // locked.
    std::fs::remove_file(db.with_file_name("mach-hold")).unwrap();
    let r = wait_push(&mut app, &db, |r| r.push.as_ref().is_some_and(|p| p.link.is_some()) && r.pid.is_none());
    let p = r.push.clone().unwrap();
    let link = "https://treeherder.mozilla.org/jobs?repo=try&landoInstance=lando-prod-2025&landoCommitID=4242";
    assert_eq!(p.link.as_deref(), Some(link));
    assert_eq!(p.lando.as_ref().map(|l| (l.instance.as_str(), l.job)), Some(("lando-prod-2025", 4242)));
    assert_eq!(app.status, "the try push is made: its link is a reply on the comment that asked for it");
    assert_eq!(
        last_turn(&db),
        format!("stacked pushed the 2 tasks you accepted to try, with patch 1 alone. Treeherder shows the push: @r1.\n\n```refs\nr1 {link}\n```")
    );
    assert!(locked(&db, &id), "locked while followed");
    to_comment(&mut app, &id);
    assert!(draw(&mut app).contains("try push: pushed"), "{}", draw(&mut app));
    keys(&mut app, &[KeyCode::Char(' ')]);
    let all = items(&app);
    for label in ["Open the try push on Treeherder…", "Stop following the try push…"] {
        assert!(all.iter().any(|(l, ok)| l == label && *ok), "{label:?} in {all:?}");
    }
    // Followed no more: unlocked, a reply saying so, with the link.
    choose(&mut app, "Stop following the try push…");
    choose(&mut app, "Stop following it");
    app.settle();
    assert_eq!(app.status, "stopped following the try push: it goes on in CI, and the comment is unlocked");
    assert_eq!(
        last_turn(&db),
        format!("stacked stopped following the try push: it goes on in CI, and Treeherder shows it: @r2.\n\n```refs\nr2 {link}\n```")
    );
    assert!(!locked(&db, &id));
    assert_eq!(push_run(&db).map(|r| (r.state, r.summary)), Some((stacked::db::RunState::Done, "stopped following".to_string())));
}

/// A comment of patch 1 asked as a try push of `tasks`, landed, reviewed,
/// every part accepted, closed and pushed, as Lando job `job`: the
/// comment's id.
fn pushed_as(app: &mut App, db: &Path, tasks: &[(&str, &str)]) -> String {
    tasks_answer(db, tasks, "As you asked.");
    ask_try(app, "Run what tests a.txt.");
    landed(app, db, 1);
    let id = notes_of(db, 0)[0].id.clone();
    keys(app, &[KeyCode::Esc, KeyCode::Enter]);
    keys(app, &[KeyCode::Esc]);
    choose(app, "Accept all remaining and Close");
    choose(app, "Accept them and close");
    wait_push(app, db, |r| r.push.as_ref().is_some_and(|p| p.link.is_some()) && r.pid.is_none());
    id
}

#[test]
fn a_try_push_followed_is_tracked_in_the_banner_and_its_summary_lands_for_review_once_done() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db);
    let job = new_job(&db);
    let id = pushed_as(&mut app, &db, &[("test-linux2404-64/opt-xpcshell-1", "Its tests."), ("build-linux64/debug", "Its build.")]);
    // Lando has it, not on try yet: the banner says so, its stream too.
    look_until(&mut app, &db, |r| r.push.as_ref().is_some_and(|p| p.lando_status.is_some()));
    assert_eq!(gets(job), [format!("https://lando.moz.tools/landing_jobs/{job}/")]);
    let screen = draw(&mut app);
    assert!(screen.contains("1 try push: Lando has it: submitted"), "{screen}");
    // On try: its tasks as Treeherder shows them, in the banner.
    let rev = "abcdef0123456789abcdef0123456789abcdef01";
    pushed(job, |p| {
        p.status = "LANDED".into();
        p.revision = rev.into();
        p.push = Some(77);
        p.jobs = vec![
            treeherder_job("Gecko Decision Task", "completed", "success", "dec", 0),
            treeherder_job("build-linux64/debug", "running", "unknown", "b1", 0),
            treeherder_job("test-linux2404-64/opt-xpcshell-1", "pending", "unknown", "t1", 0),
        ];
    });
    let r = look_until(&mut app, &db, |r| r.push.as_ref().and_then(|p| p.seen.as_ref()).is_some());
    assert_eq!(r.push.as_ref().and_then(|p| p.revision.as_deref()), Some(rev));
    assert_eq!(
        &gets(job)[1..],
        [
            format!("https://lando.moz.tools/landing_jobs/{job}/"),
            format!("https://treeherder.mozilla.org/api/project/try/push/?revision={rev}"),
            "https://treeherder.mozilla.org/api/jobs/?push_id=77&count=2000".to_string(),
        ]
    );
    let screen = draw(&mut app);
    assert!(screen.contains("1 try push: 0 of 2 tasks done"), "{screen}");
    keys(&mut app, &[KeyCode::Esc]);
    let screen = draw(&mut app);
    assert!(screen.contains(" try pushes, followed"), "{screen}");
    assert!(screen.contains("⇡ Try push of patch 1"), "{screen}");
    keys(&mut app, &[KeyCode::Esc]);
    assert!(locked(&db, &id), "locked while followed");
    let stream: Vec<String> = Db::load(&db).unwrap().events(&r.id).iter().filter_map(|e| e.get("text").and_then(|t| t.as_str()).map(String::from)).collect();
    for said in ["stacked: Lando has the push: SUBMITTED", "stacked: Lando pushed it to try, as revision abcdef012345", "stacked: Treeherder has the push", "stacked: 0 of 2 tasks done"] {
        assert!(stream.iter().any(|l| l == said), "{said:?} in {stream:?}");
    }
    // Done: the build retried, then passed; the test failed; a task the push
    // ran for them too, busted.
    pushed(job, |p| {
        p.jobs = vec![
            treeherder_job("Gecko Decision Task", "completed", "success", "dec", 0),
            treeherder_job("build-linux64/debug", "completed", "retry", "b1", 0),
            treeherder_job("build-linux64/debug", "completed", "success", "b1", 1),
            treeherder_job("test-linux2404-64/opt-xpcshell-1", "completed", "testfailed", "t1", 0),
            treeherder_job("build-win64/opt", "completed", "busted", "w1", 0),
        ];
    });
    let r = look_until(&mut app, &db, |r| r.state != stacked::db::RunState::Started);
    assert_eq!((r.state, r.summary.as_str()), (stacked::db::RunState::Done, "done: 1 success, 1 testfailed"));
    assert_eq!(app.status, "the try push is done: 1 success, 1 testfailed: its summary waits for your review");
    assert!(!locked(&db, &id), "unlocked once done");
    // Its summary, for your review, as an answer only's reply is.
    let link = format!("https://treeherder.mozilla.org/jobs?repo=try&landoInstance=lando-prod-2025&landoCommitID={job}");
    let summary = format!(
        "The try push is done. Treeherder shows it: @r2.\n\nOf its 2 tasks, Treeherder shows 1 as success and 1 as testfailed.\n\nThese failed:\n\n- `test-linux2404-64/opt-xpcshell-1` (testfailed): @r3\n\nThe push ran these for them too, and they failed:\n\n- `build-win64/opt` (busted): @r4\n\n```refs\nr2 {link}\nr3 https://treeherder.mozilla.org/jobs?repo=try&revision={rev}&selectedTaskRun=t1.0\nr4 https://treeherder.mozilla.org/jobs?repo=try&revision={rev}&selectedTaskRun=w1.0\n```"
    );
    let held = r.held.clone().unwrap();
    assert_eq!((held.replies.len(), held.replies[0].text.as_str(), held.replies[0].tag.as_str()), (1, summary.as_str(), "try"));
    assert!(draw(&mut app).contains("1 landed ask waits for your review · the next: 1 reply"), "{}", draw(&mut app));
    // Taken at once from the comment, as an answer only's reply is.
    to_comment(&mut app, &id);
    keys(&mut app, &[KeyCode::Enter]);
    assert_eq!(last_turn(&db), summary);
    assert!(stacked::held::queue(&Db::load(&db).unwrap(), "s").is_empty());
}

#[test]
fn a_try_push_s_replies_say_the_effort_its_tasks_were_selected_at() {
    use stacked::db::Effort;
    use stacked::thread::{said_by, thread_turns};
    let (_tmp, db, _src) = setup();
    let mut app = open(&db);
    let job = new_job(&db);
    // Asked at max: in its review, why the agent selected the task headed
    // with it, and the title.
    tasks_answer(&db, &[("build-linux64/debug", "Its build.")], "One task.");
    app.effort = Effort::Max;
    ask_try(&mut app, "Build it.");
    landed(&mut app, &db, 1);
    let id = notes_of(&db, 0)[0].id.clone();
    keys(&mut app, &[KeyCode::Esc, KeyCode::Enter]);
    assert_eq!(app.screen, Screen::Review);
    let screen = draw(&mut app);
    assert!(screen.contains(" the agent's work (max): 1 task to push") && screen.contains("▎ agent(max):"), "{screen}");
    keys(&mut app, &[KeyCode::Esc]);
    choose(&mut app, "Accept all remaining and Close");
    choose(&mut app, "Accept them and close");
    // Pushed: the push keeps the effort, and its link is a reply at it.
    let r = wait_push(&mut app, &db, |r| r.push.as_ref().is_some_and(|p| p.link.is_some()) && r.pid.is_none());
    assert_eq!(r.push.as_ref().and_then(|p| p.effort), Some(Effort::Max));
    let text = notes_of(&db, 0)[0].text.clone();
    let last = thread_turns(&text).pop().unwrap();
    assert!(last.body.starts_with("stacked pushed ") && said_by(&last) == "agent(max)", "{text}");
    assert!(text.contains("```agent try effort max\nstacked pushed "), "{text}");
    // Done: its summary, held for your review, at it too, and so in the
    // thread once taken.
    let rev = "abcdef0123456789abcdef0123456789abcdef02";
    pushed(job, |p| {
        p.status = "LANDED".into();
        p.revision = rev.into();
        p.push = Some(81);
        p.jobs = vec![treeherder_job("Gecko Decision Task", "completed", "success", "dec", 0), treeherder_job("build-linux64/debug", "completed", "success", "b1", 0)];
    });
    let r = look_until(&mut app, &db, |r| r.state != stacked::db::RunState::Started);
    assert_eq!(r.held.as_ref().map(|h| h.replies[0].effort), Some(Some(Effort::Max)));
    to_comment(&mut app, &id);
    assert!(draw(&mut app).contains("the agent's reply (max) waits for your review"), "{}", draw(&mut app));
    keys(&mut app, &[KeyCode::Enter]);
    let last = thread_turns(&notes_of(&db, 0)[0].text).pop().unwrap();
    assert!(last.body.starts_with("The try push is done.") && last.effort.as_deref() == Some("max"), "{last:?}");
}

#[test]
fn a_try_push_s_tasks_treeherder_never_shows_are_counted_as_not_seen_once_nothing_runs() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db);
    let job = new_job(&db);
    pushed_as(&mut app, &db, &[("test-linux2404-64/opt-xpcshell-1", "Its tests."), ("test-linux2404-64/opt-xpcshell-2", "More of them.")]);
    let rev = "1111111111111111111111111111111111111111";
    pushed(job, |p| {
        p.status = "LANDED".into();
        p.revision = rev.into();
        p.push = Some(78);
        p.jobs = vec![treeherder_job("Gecko Decision Task", "completed", "success", "dec", 0), treeherder_job("test-linux2404-64/opt-xpcshell-1", "completed", "success", "t1", 0)];
    });
    // Nothing runs any more, nothing changes: over, what is missing never to
    // show.
    let r = look_until(&mut app, &db, |r| r.state != stacked::db::RunState::Started);
    assert_eq!(r.summary, "done: 1 success, 1 not seen on Treeherder");
    let text = r.held.unwrap().replies[0].text.clone();
    for said in [
        "Of its 2 tasks, Treeherder shows 1 as success; 1 was not seen on Treeherder.",
        "Not seen on Treeherder, which shows no task that was never scheduled: `test-linux2404-64/opt-xpcshell-2`.",
    ] {
        assert!(text.contains(said), "{said:?} in {text}");
    }
    assert!(!text.contains("These failed"), "{text}");
}

#[test]
fn a_try_push_lando_did_not_push_lands_a_summary_saying_so() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db);
    let job = new_job(&db);
    let id = pushed_as(&mut app, &db, &[("build-linux64/debug", "A build.")]);
    pushed(job, |p| {
        p.status = "FAILED".into();
        p.error = "Patch 1 does not apply.".into();
    });
    let r = look_until(&mut app, &db, |r| r.state != stacked::db::RunState::Started);
    assert_eq!(r.summary, "Lando did not push it to try: FAILED: Patch 1 does not apply");
    assert_eq!(
        r.held.unwrap().replies[0].text,
        format!("The try push did not get to try: Lando says FAILED: Patch 1 does not apply. None of its tasks ran. Lando shows it: @r2.\n\n```refs\nr2 https://lando.moz.tools/landings/{job}\n```")
    );
    assert!(!locked(&db, &id));
}

#[test]
fn a_try_push_followed_is_looked_at_again_by_the_next_editor_from_where_it_was() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db);
    let job = new_job(&db);
    let id = pushed_as(&mut app, &db, &[("build-linux64/debug", "A build.")]);
    look_until(&mut app, &db, |r| r.push.as_ref().is_some_and(|p| p.lando_status.is_some()));
    drop(app);
    // Meanwhile, on try, and done.
    pushed(job, |p| {
        p.status = "LANDED".into();
        p.revision = "2222222222222222222222222222222222222222".into();
        p.push = Some(79);
        p.jobs = vec![treeherder_job("build-linux64/debug", "completed", "success", "b1", 0)];
    });
    let mut app = open(&db);
    assert!(locked(&db, &id), "followed still, as the database has it");
    let r = look_until(&mut app, &db, |r| r.state != stacked::db::RunState::Started);
    assert_eq!(r.summary, "done: 1 success");
    assert!(r.held.unwrap().replies[0].text.contains("Of its 1 task, Treeherder shows 1 as success."));
}

#[test]
fn a_try_push_whose_check_would_push_other_tasks_pushes_nothing_and_says_what_differs() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db);
    tasks_answer(&db, &[("test-linux2404-64/opt-not-a-task-1", "Not a task."), ("build-linux64/debug", "A build.")], "Two tasks.");
    ask_try(&mut app, "Build it, test it.");
    landed(&mut app, &db, 1);
    let id = notes_of(&db, 0)[0].id.clone();
    // The task graph mach reads has no such task.
    std::fs::write(db.with_file_name("mach-mode"), "drop").unwrap();
    keys(&mut app, &[KeyCode::Esc, KeyCode::Enter]);
    keys(&mut app, &[KeyCode::Esc]);
    choose(&mut app, "Accept all remaining and Close");
    choose(&mut app, "Accept them and close");
    let r = wait_push(&mut app, &db, |r| r.state != stacked::db::RunState::Started);
    assert_eq!(r.state, stacked::db::RunState::Failed);
    // Checked only: nothing pushed.
    assert_eq!(mach_args(&db).len(), 1, "{}", mach_calls(&db));
    let said = "The try push was not made: `./mach try fuzzy --no-push` would push other than what you accepted. This task you accepted is not in what mach would push, as the task graph it reads does not have it: `test-linux2404-64/opt-not-a-task-1`. Nothing was pushed: reply to say what to select instead, and ask for the try push again, for the agent to select the tasks again.";
    let thread = notes_of(&db, 0)[0].text.clone();
    assert!(thread.contains(said), "{thread}");
    assert!(app.status.starts_with("the try push was not made: `./mach try fuzzy --no-push` would push other than what you accepted"), "{}", app.status);
    // Unlocked, to reply and ask again.
    assert!(!locked(&db, &id));
    to_comment(&mut app, &id);
    keys(&mut app, &[KeyCode::Char(' ')]);
    assert!(items(&app).iter().any(|(l, ok)| l == "Reply" && *ok), "{:?}", items(&app));
}

#[test]
fn a_try_push_past_the_threshold_is_given_the_lowest_priority_in_a_preset_mach_reads() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db);
    // 1,001 tasks, one more than mach pushes without asking.
    let labels: Vec<String> = (1..=1001).map(|k| format!("test-linux2404-64/opt-mochitest-plain-{k}")).collect();
    let tasks: Vec<(&str, &str)> = labels.iter().map(|l| (l.as_str(), "As many as you asked.")).collect();
    tasks_answer(&db, &tasks, "All of them.");
    ask_try(&mut app, "Run them all.");
    landed(&mut app, &db, 1);
    keys(&mut app, &[KeyCode::Esc, KeyCode::Enter]);
    keys(&mut app, &[KeyCode::Esc]);
    choose(&mut app, "Accept all remaining and Close");
    let note = menu_note(&app);
    assert!(
        note.contains("These 1,001 tasks are more than the 1,000 mach pushes without asking: they would run at the lowest priority, as mach does when you say yes."),
        "{note}"
    );
    choose(&mut app, "Accept them and close");
    let r = wait_push(&mut app, &db, |r| r.state != stacked::db::RunState::Started || r.push.as_ref().is_some_and(|p| p.link.is_some()) && r.pid.is_none());
    assert!(r.push.as_ref().is_some_and(|p| p.lowest && p.link.is_some()), "{r:?}\n{}", mach_calls(&db));
    // Both runs of mach given the preset, from a presets file of stacked's
    // own, which the check found the lowest priority in; that file gone
    // once the push is done with it.
    let args = mach_args(&db);
    assert_eq!(args.len(), 2);
    assert!(
        args.iter().all(|a| a.contains("--show-chunk-numbers --all-tasks --preset stacked-lowest -q ^test-linux2404-64/opt-mochitest-plain-1$ | ^test-linux2404-64/opt-mochitest-plain-2$ | ")),
        "{args:?}"
    );
    let calls = mach_calls(&db);
    let presets: Vec<&str> = calls.lines().filter_map(|l| l.strip_prefix("presets: ")).collect();
    assert_eq!(presets.len(), 2, "{calls}");
    let scratch = db.canonicalize().unwrap().with_file_name("stacked-try");
    assert!(presets[0].starts_with(&scratch.to_string_lossy().into_owned()), "{presets:?}");
    for said in [r#""stacked-lowest": {"#, r#""selector": "fuzzy""#, r#""try_task_config": {"#, r#""priority": "lowest""#] {
        assert!(calls.contains(said), "{said:?} in {calls}");
    }
    assert!(!Path::new(presets[0]).exists() && !db.with_file_name("stacked-try").exists(), "the presets file removed");
    let thread = notes_of(&db, 0)[0].text.clone();
    assert!(thread.contains("stacked pushed the 1,001 tasks you accepted to try, with patch 1 alone, at the lowest priority."), "{thread}");
}

#[test]
fn a_try_push_that_mach_asks_a_question_of_stops_quotes_it_and_says_what_to_change() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db);
    tasks_answer(&db, &[("build-linux64/debug", "A build.")], "One task.");
    ask_try(&mut app, "Build it.");
    landed(&mut app, &db, 1);
    let id = notes_of(&db, 0)[0].id.clone();
    std::fs::write(db.with_file_name("mach-mode"), "ask").unwrap();
    keys(&mut app, &[KeyCode::Esc, KeyCode::Enter]);
    keys(&mut app, &[KeyCode::Esc]);
    choose(&mut app, "Accept all remaining and Close");
    choose(&mut app, "Accept them and close");
    let r = wait_push(&mut app, &db, |r| r.state != stacked::db::RunState::Started);
    assert_eq!(r.state, stacked::db::RunState::Failed);
    assert_eq!(mach_args(&db).len(), 1, "stopped at the check: {}", mach_calls(&db));
    let thread = notes_of(&db, 0)[0].text.clone();
    for said in [
        "The try push was not made: mach asked a question that stacked cannot answer, \"Do you have an @mozilla.com address? [Y/n]\", and stopped, as stacked gives it nothing to read. Nothing was pushed.",
        "set your hg.mozilla.org user in ~/.ssh/config",
        "or unset MOZ_USE_PERNOSCO",
    ] {
        assert!(thread.contains(said), "{said:?} in {thread}");
    }
    assert!(!locked(&db, &id));
}

#[test]
fn a_try_push_failing_as_mach_pushes_says_why_with_mach_s_last_lines() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db);
    tasks_answer(&db, &[("build-linux64/debug", "A build.")], "One task.");
    ask_try(&mut app, "Build it.");
    landed(&mut app, &db, 1);
    std::fs::write(db.with_file_name("mach-mode"), "fail").unwrap();
    keys(&mut app, &[KeyCode::Esc, KeyCode::Enter]);
    keys(&mut app, &[KeyCode::Esc]);
    choose(&mut app, "Accept all remaining and Close");
    choose(&mut app, "Accept them and close");
    let r = wait_push(&mut app, &db, |r| r.state != stacked::db::RunState::Started);
    assert_eq!((r.state, r.summary.as_str()), (stacked::db::RunState::Failed, "`./mach try fuzzy` failed, exiting with 1, before it said it pushed"));
    let thread = notes_of(&db, 0)[0].text.clone();
    assert!(
        thread.contains("The try push failed: `./mach try fuzzy` failed, exiting with 1, before it said it pushed. Its last lines:\n\n```text\nPushing to try...\nabort: error submitting patches to Lando.\nLando returned HTTP 503 (Service Unavailable).\n```\n\nNothing was pushed."),
        "{thread}"
    );
}

#[test]
fn a_try_push_stopped_as_mach_waits_for_the_login_pushes_nothing_and_unlocks_the_comment() {
    let (_tmp, db, _src) = setup();
    let mut app = open(&db);
    tasks_answer(&db, &[("build-linux64/debug", "A build.")], "One task.");
    ask_try(&mut app, "Build it.");
    landed(&mut app, &db, 1);
    let id = notes_of(&db, 0)[0].id.clone();
    std::fs::write(db.with_file_name("mach-hold"), "").unwrap();
    keys(&mut app, &[KeyCode::Esc, KeyCode::Enter]);
    keys(&mut app, &[KeyCode::Esc]);
    choose(&mut app, "Accept all remaining and Close");
    choose(&mut app, "Accept them and close");
    wait_push(&mut app, &db, |r| r.push.as_ref().is_some_and(|p| p.login.is_some()));
    to_comment(&mut app, &id);
    keys(&mut app, &[KeyCode::Char(' ')]);
    choose(&mut app, "Stop the try push…");
    assert!(menu_title(&app).starts_with("stop the try push? mach is stopped with it"), "{}", menu_title(&app));
    choose(&mut app, "Stop it");
    app.settle();
    assert_eq!(app.status, "the try push was stopped: nothing was pushed.");
    let r = push_run(&db).unwrap();
    assert_eq!((r.state, r.summary.as_str()), (stacked::db::RunState::Failed, "stopped before mach said it pushed"));
    let thread = notes_of(&db, 0)[0].text.clone();
    assert!(thread.contains("The try push was stopped while mach ran, before it said it pushed: nothing was pushed."), "{thread}");
    assert!(!locked(&db, &id));
}

#[test]
fn a_try_push_whose_process_is_gone_before_mach_said_it_pushed_ends_as_failed() {
    let (_tmp, db, _src) = setup();
    let app = open(&db);
    // A comment, and a push of it checking, its process gone.
    let mut gone = Command::new("true").spawn().unwrap();
    let dead = gone.id();
    gone.wait().unwrap();
    let d = Db::load(&db).unwrap();
    let change = d.patch(&d.stack("s").unwrap().patches[0]).change.clone();
    let rev = d.head("s").unwrap().0;
    drop(d);
    Db::edit(&db, |d| {
        d.set_note(Note { change: change.clone(), id: "c".into(), text: "Which tasks?".into(), ..Default::default() })?;
        d.add_run(stacked::db::Run {
            id: "p".into(),
            what: stacked::db::TRY_PUSH.into(),
            stack: "s".into(),
            state: stacked::db::RunState::Started,
            pid: Some(dead),
            push: Some(Box::new(stacked::try_push::Push {
                change: change.clone(),
                id: "c".into(),
                rev,
                patches: 1,
                labels: vec!["build-linux64/debug".into()],
                step: stacked::try_push::Step::Checking,
                ..Default::default()
            })),
            ..Default::default()
        })
        .map(|_| ())
    })
    .unwrap();
    assert!(locked(&db, "c"), "locked while it seems to go");
    assert_eq!(stacked::ask::land_orphans(&db, &Db::load(&db).unwrap()), 1);
    let r = push_run(&db).unwrap();
    assert_eq!((r.state, r.summary.as_str()), (stacked::db::RunState::Failed, "the process pushing it is gone"));
    assert!(!locked(&db, "c"));
    let n = Db::load(&db).unwrap().note(&change, "c").cloned().unwrap();
    assert_eq!(
        stacked::thread::thread_turns(&n.text).pop().unwrap().body,
        "The try push stopped before mach said it pushed: the process pushing it is gone, and stacked does not know whether mach sent the push."
    );
    // Pushed, it needs no process: followed, it stays.
    Db::edit(&db, |d| {
        let mut r = d.run("p").unwrap().clone();
        r.id = "q".into();
        r.state = stacked::db::RunState::Started;
        r.pid = None;
        r.push.as_mut().unwrap().step = stacked::try_push::Step::Pushed;
        r.push.as_mut().unwrap().link = Some("https://treeherder.mozilla.org/jobs?repo=try&landoInstance=i&landoCommitID=1".into());
        d.add_run(r).map(|_| ())
    })
    .unwrap();
    assert_eq!(stacked::ask::land_orphans(&db, &Db::load(&db).unwrap()), 0);
    assert!(locked(&db, "c"));
    drop(app);
}

#[test]
fn a_try_push_on_a_base_no_remote_of_firefox_s_has_keeps_the_review_open() {
    let (_tmp, db, src) = setup();
    let mut app = open(&db);
    tasks_answer(&db, &[("build-linux64/debug", "A build.")], "One task.");
    ask_try(&mut app, "Build it.");
    landed(&mut app, &db, 1);
    // origin, as if never fetched: its base on no remote's.
    git(&src, &["update-ref", "-d", "refs/remotes/origin/main"]);
    let base = git_out(&src, &["rev-parse", "HEAD~2"]);
    let why = format!(
        "the stack's base, commit {}, is not on origin, the remote mach takes as Firefox's: mach would push 1 commit of the base's besides the stack's patches. Fetch that remote (`git fetch`), or put the stack on a commit that it has",
        &base[..12]
    );
    keys(&mut app, &[KeyCode::Esc, KeyCode::Enter]);
    keys(&mut app, &[KeyCode::Esc]);
    choose(&mut app, "Accept all remaining and Close");
    assert!(menu_note(&app).contains(&format!("Closing the review with the task you accepted keeps it open, as nothing could be pushed: {why}.")), "{}", menu_note(&app));
    choose(&mut app, "Accept them and close");
    assert_eq!(app.status, format!("the review stays open: {why}"));
    assert_eq!(app.screen, Screen::Review);
    // No remote at all: mach would push the whole history.
    git(&src, &["remote", "remove", "origin"]);
    keys(&mut app, &[KeyCode::Esc]);
    choose(&mut app, "Accept all remaining and Close");
    choose(&mut app, "Accept them and close");
    assert_eq!(
        app.status,
        "the review stays open: the repository has no remote, and mach pushes every commit that no remote has: it would push the stack's whole history"
    );
    assert!(push_run(&db).is_none() && mach_calls(&db).is_empty(), "mach never ran");
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
fn the_first_try_push_goes_at_once_with_no_setup_before_it() {
    let (_tmp, db, src) = setup();
    // Settings of before, the setup's old mark in them: passed over.
    std::fs::create_dir_all(src.join(".stacked")).unwrap();
    std::fs::write(src.join(".stacked/local.toml"), "[agent]\nsetup = 1\n").unwrap();
    let mut app = open(&db);
    // A try push, of patch 1 as a whole: it goes at once, with what an
    // answer is given and stacked's defaults for its own list.
    tasks_answer(&db, &[("build-linux64/debug", "A build.")], "One task.");
    ask_try(&mut app, "Which tasks?");
    assert!(app.status.contains("asked the agent about 1 comment (try push)"), "{}", app.status);
    assert!(matches!(app.mode, Mode::Normal), "{:?}", app.mode);
    landed(&mut app, &db, 1);
    let d = Db::load(&db).unwrap();
    let r = d.runs().into_iter().last().unwrap().clone();
    let given = stacked::ask::info(&r).unwrap().given;
    assert!(given.ends_with(",Bash(curl *),Bash(jq *)"), "{given}");
    assert_eq!(std::fs::read_to_string(src.join(".stacked/local.toml")).unwrap(), "[agent]\nsetup = 1\n", "nothing written");
}

/// A comment on patch 1 as a whole, `text`, written from the patches pane,
/// then put in the batch as a try push as it is saved, to be asked later.
fn batch_try(app: &mut App, text: &str) {
    keys(app, &[KeyCode::Home, KeyCode::Down]);
    choose(app, "Comment on this patch…");
    typed(app, text);
    keys(app, &[KeyCode::Esc]);
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
