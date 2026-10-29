//! A try push of Firefox's CI, built with the `try-push` feature. A
//! comment asked as a try push (`Way::Try`) has the agent select the CI
//! tasks it asks for, as few or as many as it says, each by its exact label
//! in the task graph and with why it is relevant: it pushes nothing and
//! changes nothing. What it selects lands for your review, as any of its
//! work does: each task accepted or rejected, its reason shown as the
//! agent's comment, which you may answer, the work then going back to the
//! agent for another round. Closing the review with tasks accepted pushes
//! them to try.
//!
//! The push (`begin`, then `push` in a process of its own) is made from a
//! worktree of the stack's own, `.stacked/NAME/try`: the stack's patches
//! as commits on its base, up to the comment's patch, or all of them for a
//! comment on the stack. mach pushes every commit of HEAD that none of the
//! remotes it takes as Firefox's has, so the base must be on one of them,
//! and the commits it would push are the patches alone, checked before
//! mach runs. `./mach try fuzzy --no-push` runs first, each task its own
//! exact query, and what it would push is compared with what was accepted:
//! the tasks, how many times each runs, and its priority. Then `./mach try
//! fuzzy` pushes, its lines in the comment's stream as they come, the page
//! and code of its login shown as it waits for them. mach never reads
//! anything: its stdin is closed, so a question it asks stops it, and is
//! said. Past mach's threshold, the lowest priority it sets once told yes
//! is given beforehand, in a preset file of stacked's own. Pushed, its link
//! is a reply on the comment that asked, a web link, and the comment stays
//! locked until you stop following the push.

use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::db::{Db, Error, Note, Record, Result, Run, RunState, err, now};
use crate::refs::{Pointer, Pointing};
use crate::thread::{Role, append_turn};

/// A task the agent selected for a try push: its exact label in the task
/// graph; why it is relevant, as the agent said it, its references in a
/// `refs` block at its end; and your answer to that, written in the
/// review, with which the work goes back to the agent.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Task {
    pub label: String,
    #[serde(default)]
    pub why: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
}

/// How many tasks mach pushes without asking: past them, it asks on its
/// terminal whether to go on, and, told yes, runs them at the lowest
/// priority (push.py's `LARGE_PUSH_THRESHOLD`).
pub const THRESHOLD: usize = 1000;

/// How many times each task may be run, as mach's `--rebuild` takes it:
/// once is no rebuild.
pub const REBUILDS: std::ops::RangeInclusive<u32> = 2..=20;

/// How many tasks a push of `labels` labels schedules, as push.py counts
/// them against its threshold: each label once, times `rebuild`.
pub fn scheduled(labels: usize, rebuild: Option<u32>) -> usize {
    labels * rebuild.unwrap_or(1) as usize
}

pub use crate::paint::thousands;

/// Whether `label` can be a task's label as `./mach try fuzzy` is given
/// it, a term that selects that task alone: something, and no white space,
/// which would split it into terms.
fn label_of(text: &str) -> Option<String> {
    let t = text.trim();
    (!t.is_empty() && !t.chars().any(|c| c.is_whitespace() || c.is_control())).then(|| t.to_string())
}

/// The tasks the agent's answer `answer` (its last JSON object) selects,
/// in the order it gave them, each label once, the first reason kept; and
/// how many times each is to run, when it asked for more than once. A task
/// is `{"label", "why"}`, its reason a string or what is written with
/// references (`{"text", "refs"}`); a label alone is a task with no reason.
/// A label that cannot select its task (empty, or holding white space) is
/// left out.
pub fn tasks_of(answer: Option<&Map<String, Value>>) -> (Vec<Task>, Option<u32>) {
    let mut out: Vec<Task> = Vec::new();
    let given = answer.and_then(|o| o.get("tasks")).and_then(Value::as_array).cloned().unwrap_or_default();
    for t in &given {
        let (label, why) = match t {
            Value::String(s) => (label_of(s), String::new()),
            Value::Object(o) => (
                o.get("label").and_then(Value::as_str).and_then(label_of),
                o.get("why").map(crate::refs::written).unwrap_or_default(),
            ),
            _ => (None, String::new()),
        };
        let Some(label) = label else { continue };
        if !out.iter().any(|k| k.label == label) {
            out.push(Task { label, why: why.trim().to_string(), answer: None });
        }
    }
    let rebuild = answer
        .and_then(|o| o.get("rebuild"))
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
        .filter(|n| *n >= *REBUILDS.start())
        .map(|n| n.min(*REBUILDS.end()));
    (out, rebuild)
}

/// What a try push ask is told it does, `what` being what it pushes (the
/// stack, or a patch with those below it), `here` where it works and how
/// it reads the stack, `web` what it may read of the web: the tasks the
/// comments ask for, as few or as many as they say, exact labels, why each;
/// and nothing pushed, nothing changed. When it may run curl, `graph` being
/// the file of its own it downloads the task graph into, it is told where
/// the labels are, and to check each it selects against the task graph
/// (`labels`).
pub fn prompt(what: &str, here: &str, web: &str, graph: Option<&Path>) -> String {
    let labels = graph.map(labels).unwrap_or_default();
    format!(
        "You are selecting the CI tasks of a try push of {what}, as the comments below ask for \
         them: as few or as many as they say, and for what they say. Each task is one of \
         Firefox's CI tasks, named by its exact label in the task graph, as `./mach try fuzzy \
         --show-chunk-numbers` lists them: `test-linux2404-64/opt-mochitest-browser-chrome-3`, \
         `build-linux64/debug`, `source-test-mozlint-eslint`; a chunked test is a task for each \
         chunk, its label ending with the chunk's number, and no label is a pattern. Say why \
         each is relevant: to what the patches change, and to what is asked. {here} Do NOT \
         push anything, nor run `./mach`: once the user has reviewed the tasks you select, \
         stacked pushes those they accept, with `./mach try fuzzy`, and checks first, with its \
         `--no-push`, that each label selects its task; a label the task graph does not have is \
         said then, and nothing is pushed. Do NOT modify anything: no file, no commit, no \
         branch. You cannot build the project or run its tests; reason from the code. \
         {web}{labels}"
    )
}

/// Where the decision task of mozilla-central's latest push keeps what it
/// made, as Taskcluster's index finds it: `target-tasks.json`,
/// `full-task-graph.json`.
const DECISION: &str = "https://firefox-ci-tc.services.mozilla.com/api/index/v1/task/\
gecko.v2.mozilla-central.latest.taskgraph.decision/artifacts/public";

/// The file a try push ask that may run curl downloads the task graph into,
/// in the folder of its own stacked makes it, which `ask` keeps beside the
/// rebase plans' and removes once the ask lands.
pub const GRAPH: &str = "full-task-graph.json";

/// What a try push ask is told of where to find the labels, when it may
/// read the web with curl: those of the tasks of mozilla-central's latest
/// push, and autoland's; and that push's full task graph, every task it
/// may select, to check each label it selects against before it answers,
/// downloaded once into `graph`, its scratch file, and read there, and a
/// download that fails said in its answer.
fn labels(graph: &Path) -> String {
    let file = graph.display();
    let word = quoted(&file.to_string());
    format!(
        " The labels of the tasks of mozilla-central's latest push are a JSON list at \
         {DECISION}/target-tasks.json, and autoland's with `autoland` in place of \
         `mozilla-central`: a try push may run any of them. The full task graph of \
         mozilla-central's latest push, every task `./mach try fuzzy --all-tasks` can select, \
         is the JSON object at {DECISION}/full-task-graph.json, whose keys are the tasks' \
         labels: check each label you select against it before you answer. It is large: \
         download it once into `{file}`, a scratch file in a folder of your own, the one \
         place you may write (`curl -sL --compressed -o {word}` with its address), and read \
         that copy: `jq '[\"LABEL\", …] - keys' {word}` prints the labels you list that the \
         graph does not have. If the download fails, say so in your answer, and answer \
         without it."
    )
}

/// `word` as sh reads it as one word: as it is, when it is only letters,
/// digits and `-_./=:`; else between single quotes.
fn quoted(word: &str) -> String {
    match word.chars().all(|c| c.is_ascii_alphanumeric() || "-_./=:".contains(c)) {
        true => word.to_string(),
        false => format!("'{}'", word.replace('\'', "'\\''")),
    }
}

/// What a try push ask ends its answer with, `end` saying how it gives
/// it: its replies, the tasks it selects, how many times each runs.
pub fn answer_said(end: &str) -> String {
    format!(
        "\n{end}: {{\"replies\": {{label: your reply to that comment: which tasks you selected, and \
         why, in short}}, \"tasks\": [{{\"label\": the task's exact label, \"why\": why it is relevant, \
         to what the patches change and to what is asked}}], \"rebuild\": how many times each task \
         runs, from 2 to 20, only when what is asked wants repeated runs (an intermittent failure)}}, \
         labels as above ([c1] is \"c1\"), each task once, in the order you would read them."
    )
}

/// The JSON Schema of what a try push ask gives besides its replies:
/// `tasks`, each its label and why, written as anything the agent writes
/// is (`written`, with references or not); `rebuild`, how many times each
/// runs.
pub fn schema(written: &Value) -> Vec<(&'static str, Value)> {
    let task = serde_json::json!({
        "type": "object",
        "properties": {"label": {"type": "string"}, "why": written},
        "required": ["label", "why"],
        "additionalProperties": false
    });
    vec![
        ("tasks", serde_json::json!({"type": "array", "items": task})),
        ("rebuild", serde_json::json!({"type": "integer", "minimum": REBUILDS.start(), "maximum": REBUILDS.end()})),
    ]
}

/// What a round asked again of a try push is told it selected before:
/// each task, as the user marked it (`mark`, by its place), with why, and
/// what the user answered to that; and how many times each was to run.
pub fn chosen_said(tasks: &[Task], rebuild: Option<u32>, mark: &dyn Fn(usize) -> Option<bool>) -> String {
    if tasks.is_empty() {
        return String::new();
    }
    let mut out = String::from("\n\nThe tasks you selected then, each as the user marked it, with why you selected it and what the user answered:");
    for (k, t) in tasks.iter().enumerate() {
        let marked = match mark(k) {
            Some(true) => "accepted",
            Some(false) => "rejected",
            None => "not marked",
        };
        let (why, _) = crate::refs::split(&t.why);
        let why = if why.trim().is_empty() { "no reason given".to_string() } else { why.trim().to_string() };
        out += &format!("\n- `{}` ({marked}): {why}", t.label);
        if let Some(a) = &t.answer {
            out += &format!("\n  The user answered: {}", a.trim());
        }
    }
    if let Some(n) = rebuild {
        out += &format!("\nEach was to run {n} times.");
    }
    out
}

/// What your answers to the reasons of `tasks` say in the thread of the
/// comment that asked, as a turn of yours, once the work goes back to the
/// agent: each task's label, then what you answered. None without any.
pub fn answers_turn(tasks: &[Task]) -> Option<String> {
    let answered: Vec<String> =
        tasks.iter().filter_map(|t| Some(format!("- `{}`: {}", t.label, t.answer.as_deref()?.trim()))).collect();
    (!answered.is_empty()).then(|| format!("On the tasks the agent selected:\n{}", answered.join("\n")))
}

/// The tasks a try push pushes, said: `the task you accepted`, `the 3
/// tasks you accepted`.
pub fn tasks_said(n: usize) -> String {
    match n {
        1 => "the task you accepted".to_string(),
        n => format!("the {} tasks you accepted", thousands(n)),
    }
}

/// The patches a try push pushes, `n` of them from the bottom, said:
/// `patch 1 alone`, `patches 1 to 3`; for a comment on the stack
/// (`whole`), `every patch of the stack`.
pub fn patches_said(n: usize, whole: bool) -> String {
    match (whole, n) {
        (true, _) => "every patch of the stack".to_string(),
        (false, 1) => "patch 1 alone".to_string(),
        (false, n) => format!("patches 1 to {n}"),
    }
}

// -- the push ---------------------------------------------------------------- //

/// What a try push's run is, as `Run::what` says it.
pub const WHAT: &str = crate::db::TRY_PUSH;

/// The worktrees try pushes are pushed from, of the stack's own: `try`,
/// `try-2`… under `.stacked/NAME`, beside the agents'.
pub const TRY: &str = "try";

/// The preset stacked gives mach for a push past its threshold, in a file
/// of its own that `MACH_TRY_PRESET_PATHS` names: the lowest priority, as
/// mach sets it itself once told yes, so that it asks nothing.
pub const LOWEST: &str = "stacked-lowest";

/// A try push, as stacked makes it of the tasks a review accepted: kept in
/// its run (`Run::push`), each step written as it goes.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Push {
    /// The comment that asked for it: its change and id. Its replies go
    /// there, and it is locked while the push goes on.
    pub change: String,
    pub id: String,
    /// The run whose work's review accepted the tasks.
    pub work: String,
    /// The stack it pushes, by its rev, and how many of its patches,
    /// bottom first: those up to the comment's, or, for a comment on the
    /// stack (`whole`), all of them.
    pub rev: usize,
    pub patches: usize,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub whole: bool,
    /// The tasks, by label, in the order the agent gave them.
    pub labels: Vec<String>,
    /// How many times each runs, when more than once.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rebuild: Option<u32>,
    /// Past mach's threshold: at the lowest priority.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub lowest: bool,
    #[serde(default)]
    pub step: Step,
    /// The worktree it is pushed from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<String>,
    /// The login mach waits for, while it does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub login: Option<Login>,
    /// Once pushed: its link, and where Lando has it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lando: Option<Lando>,
    /// Followed: when it was last looked at, on Lando and Treeherder (0:
    /// not yet).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub looked: i64,
    /// Lando's state of it, as last seen, until it pushes it to try.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lando_status: Option<String>,
    /// The revision Lando pushed to try, and its push on Treeherder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub treeherder: Option<u64>,
    /// Its tasks, as Treeherder showed them last.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seen: Option<Seen>,
    /// How many looks in a row found nothing of it pending or running, and
    /// nothing new: at `QUIET`, it is over, what is missing never to show.
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub quiet: u32,
    /// Why the last look could not read Lando or Treeherder, as its stream
    /// said it once: none since a look could.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trouble: Option<String>,
}

fn is_zero(n: &i64) -> bool {
    *n == 0
}

fn is_zero_u32(n: &u32) -> bool {
    *n == 0
}

/// A try push's tasks as Treeherder shows them: of those it pushes (the
/// tasks accepted, each as many times as it runs), how many there are and
/// how many are done, by result; those done that failed; the tasks the push
/// ran for them that failed (their builds, the decision task); those not
/// seen; and whether anything of it is pending or running still.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Seen {
    pub total: usize,
    pub done: usize,
    /// How many of those done ended each way, as Treeherder says it:
    /// `success`, `testfailed`, `busted`, `exception`…
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub results: std::collections::BTreeMap<String, usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failed: Vec<Shown>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub besides: Vec<Shown>,
    /// Each label Treeherder does not show as many times as it is pushed,
    /// with how many times it does not.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unseen: Vec<(String, usize)>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub going: bool,
}

/// A task as Treeherder shows it: its label, its result, and its link
/// there, the run of it shown.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Shown {
    pub label: String,
    pub result: String,
    pub link: String,
}

impl Seen {
    /// How far it is, said: `12 of 30 tasks done, 1 failed`.
    pub fn said(&self) -> String {
        let failed = match self.failed.len() {
            0 => String::new(),
            n => format!(", {n} failed"),
        };
        format!("{} of {} tasks done{failed}", thousands(self.done), thousands(self.total))
    }

    /// How many were not seen on Treeherder.
    pub fn unseen_count(&self) -> usize {
        self.unseen.iter().map(|(_, n)| n).sum()
    }
}

/// Where a try push is.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Step {
    /// Its worktree made, the commits mach would push checked.
    #[default]
    Preparing,
    /// `./mach try fuzzy --no-push` run, what it would push compared.
    Checking,
    /// `./mach try fuzzy` run: its login, then the push.
    Pushing,
    /// Pushed: its link known.
    Pushed,
}

/// The page and code that mach's login (Auth0's device code) waits for.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Login {
    pub url: String,
    pub code: String,
}

/// Where Lando has a push: its instance, and its job.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Lando {
    pub instance: String,
    pub job: u64,
}

/// Where try push `p` is, said for its comment: `try push: mach checks the
/// tasks`.
pub fn state_said(p: &Push) -> String {
    match (p.step, &p.login) {
        (Step::Preparing, _) => "try push: its worktree is being made".into(),
        (Step::Checking, _) => "try push: mach checks the tasks".into(),
        (Step::Pushing, Some(_)) => "try push: mach waits for your login".into(),
        (Step::Pushing, None) => "try push: mach pushes".into(),
        (Step::Pushed, _) => format!("try push: {}", progress_said(p)),
    }
}

/// How far try push `p`, pushed and followed, is, said: Lando's state of
/// it until Lando pushes it to try, then its tasks on Treeherder.
pub fn progress_said(p: &Push) -> String {
    match (&p.revision, &p.seen, &p.lando_status) {
        (Some(_), Some(seen), _) => seen.said(),
        (Some(_), None, _) => "pushed, its tasks not on Treeherder yet".into(),
        (None, _, Some(status)) => format!("Lando has it: {}", status.to_lowercase().replace('_', " ")),
        (None, _, None) if p.trouble.is_some() => "pushed, Lando not read yet".into(),
        (None, _, None) => "pushed, Lando not looked at yet".into(),
    }
}

/// The comments whose try push is under way, by (change, id), each with
/// the push's run: locked as those the agent works on are.
pub fn locks(db: &Db) -> Vec<((String, String), String)> {
    db.live_runs()
        .into_iter()
        .filter(|r| r.state == RunState::Started)
        .filter_map(|r| r.push.as_ref().map(|p| ((p.change.clone(), p.id.clone()), r.id.clone())))
        .collect()
}

/// Under the lock: the try push of `accepted`, the tasks the review of work
/// `work` accepted, each to run `rebuild` times, begun as a run of its own;
/// its id. It pushes the stack as it is now, up to the patch of the comment
/// that asked, or every patch for a comment on the stack. `launch` starts
/// it.
pub fn begin(db: &mut Db, work: &str, accepted: &[Task], rebuild: Option<u32>) -> Result<String> {
    let r = db.run(work).cloned().ok_or_else(|| Error(format!("no run {work}")))?;
    let i = crate::ask::info(&r).ok_or_else(|| Error(format!("run {work} is not an ask")))?;
    let asked = i.notes.first().ok_or_else(|| Error("the try push was asked about no comment: nothing is pushed".into()))?;
    if db.note(&asked.change, &asked.id).is_none_or(|n| n.text.is_empty()) {
        return err("the comment that asked for the try push is closed: nothing is pushed");
    }
    let (rev, s) = db.head(&r.stack).map(|(rev, s)| (rev, s.clone())).ok_or_else(|| Error(format!("no stack {}", r.stack)))?;
    let (patches, whole) = match &i.scope {
        crate::ask::Scope::Stack => (s.patches.len(), true),
        crate::ask::Scope::Patch(c) => {
            let k = s
                .patches
                .iter()
                .position(|p| db.patch(p).change == *c)
                .ok_or_else(|| Error("the patch of the comment that asked is no longer in the stack: nothing is pushed".into()))?;
            (k + 1, false)
        }
    };
    if patches == 0 {
        return err("the stack has no patch to push: nothing is pushed");
    }
    let labels: Vec<String> = accepted.iter().map(|t| t.label.clone()).collect();
    let lowest = scheduled(labels.len(), rebuild) > THRESHOLD;
    let id = crate::db::unique();
    db.add_run(Run {
        id: id.clone(),
        what: WHAT.into(),
        stack: r.stack.clone(),
        state: RunState::Started,
        request: format!("a try push of {}", tasks_said(labels.len())),
        starter: Some(std::process::id()),
        push: Some(Box::new(Push {
            change: asked.change.clone(),
            id: asked.id.clone(),
            work: work.into(),
            rev,
            patches,
            whole,
            labels,
            rebuild,
            lowest,
            ..Default::default()
        })),
        ..Default::default()
    })?;
    Ok(id)
}

/// The program the pushes this process starts run as mach, when one is
/// set: the tests' stand-in, passed on as `$STACKED_MACH`.
static MACH: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

/// Runs `program` as mach from now on, in the pushes this process starts:
/// the tests set their stand-in here, as they share their process.
pub fn stand_in(program: &Path) {
    *MACH.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(program.to_path_buf());
}

/// Starts try push `id` away from here: `stacked try-push ID`, a process of
/// its own, in a session of its own, so that neither mach nor what it runs
/// (ssh, git) has a terminal to ask anything on, and its process group is
/// its own, which stopping it stops, mach with it; told to keep what is
/// yours where this one does, and the tests' stand-in for mach, if they set
/// one.
pub fn launch(db_path: &Path, id: &str) -> Result<()> {
    let exe = crate::ask::stacked_exe()?;
    let db_abs = db_path.canonicalize().map_err(|e| Error(format!("{}: {e}", db_path.display())))?;
    let mut c = Command::new(&exe);
    c.arg("-f").arg(&db_abs).arg("try-push").arg(id);
    c.current_dir(db_abs.parent().unwrap_or(Path::new("/")));
    c.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    crate::home::get().pass_on(&mut c);
    if let Some(mach) = MACH.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone() {
        c.env("STACKED_MACH", mach);
    }
    // Between fork and exec, the child alone calls setsid, which is safe
    // there: it allocates nothing and touches no lock.
    unsafe {
        std::os::unix::process::CommandExt::pre_exec(&mut c, || {
            libc::setsid();
            Ok(())
        });
    }
    let child = c.spawn().map_err(|e| Error(format!("{}: {e}", exe.display())))?;
    let pid = child.id();
    crate::ask::keep_started(child);
    // Its pid written, for a stop to find it: unless it wrote it itself, or
    // is past needing one.
    Db::edit(db_path, |db| {
        if let Some(r) = db.run(id).filter(|r| r.state == RunState::Started && r.pid.is_none()).cloned()
            && r.push.as_ref().is_some_and(|p| p.step != Step::Pushed)
        {
            db.add_run(Run { pid: Some(pid), ..r })?;
        }
        Ok(())
    })
}

/// Ends try push `id`, which `launch` could not start, with why.
pub fn not_started(db_path: &Path, id: &str, why: &str) {
    let said = format!("The try push was not made: {why}. Nothing was pushed.");
    let _ = end(db_path, id, RunState::Failed, why, &|_| said.clone());
}

/// Why a try push was not made: for its run's summary, short, and for the
/// reply on its comment, said in full.
struct Failure {
    why: String,
    said: String,
}

impl Failure {
    /// Not made, `why`, before anything could be pushed.
    fn before(why: impl Into<String>) -> Failure {
        let why = why.into();
        Failure {
            said: format!("The try push was not made: {why}. Nothing was pushed."),
            why,
        }
    }
}

impl From<Error> for Failure {
    fn from(e: Error) -> Failure {
        Failure::before(e.0)
    }
}

/// Pushes try push `id`, in the process `launch` started: the stack's
/// patches as commits in its worktree, the commits mach would push checked
/// to be those alone, `./mach try fuzzy --no-push` run and what it would
/// push compared with what was accepted, then `./mach try fuzzy`; mach's
/// lines in the run's stream as they come, its login's page and code kept
/// as it waits for them. Pushed, its link is a reply on the comment that
/// asked, which stays locked; else the reply says why it was not, and the
/// comment unlocks.
pub fn push(db_path: &Path, id: &str) -> Result<()> {
    // Its pid, for a stop to find it; nothing done if it is over already
    // (stopped as it started).
    let started = Db::edit(db_path, |db| {
        let r = db.run(id).cloned().ok_or_else(|| Error(format!("no run {id}")))?;
        if r.state != RunState::Started || r.push.is_none() {
            return Ok(None);
        }
        db.add_run(Run { pid: Some(std::process::id()), ..r.clone() })?;
        Ok(Some(r))
    })?;
    let Some(r) = started else { return Ok(()) };
    let scratch = Scratch::new(db_path, id);
    let pushed = pushing(db_path, &r, &scratch);
    drop(scratch);
    match pushed {
        Ok(()) => pushed_to_try(db_path, id),
        Err(f) => end(db_path, id, RunState::Failed, &f.why, &|_| f.said.clone()),
    }
}

/// The work of `push`, up to mach saying it pushed: why it was not made,
/// if it was not.
fn pushing(db_path: &Path, r: &Run, scratch: &Scratch) -> std::result::Result<(), Failure> {
    let id = r.id.as_str();
    let p = r.push.clone().ok_or_else(|| Failure::before("the run is not a try push"))?;
    let db = Db::load(db_path)?;
    let s = db.node(p.rev)?.clone();
    let (repo, base) = crate::ops::base_of(&s)?;
    let patches: Vec<&crate::db::Patch> = s.patches.iter().take(p.patches).map(|id| db.patch(id)).collect();
    let mbox = crate::mbox::format_mbox(&patches);
    let (cfg, _) = crate::config::of(&repo);
    let dir = crate::ask::agents_dir(&repo, &s.name, cfg.agent.worktrees.as_deref())?;
    // Its worktree: the stack's patches as commits on its base, as far as
    // the comment's.
    let at = take_worktree(db_path, id, &dir)?;
    let step = |text: String| record(db_path, id, STEP, &text);
    if at.join(".git").exists() {
        let _ = crate::git::scrub(&at);
    }
    crate::git::ready(&repo, &at, &base, &mbox, p.patches, &step)?;
    // The commits mach would push: the stack's patches, and nothing else.
    let remotes = official_remotes(&at)?;
    step(format!("git rev-list --count HEAD --not {}", remote_args(&remotes).join(" ")));
    if let Some(why) = commits_problem(&at, &base, p.patches, &remotes) {
        return Err(Failure::before(why));
    }
    let n = p.patches;
    say(db_path, id, &format!("mach would push the stack's {} alone, on its base", if n == 1 { "patch".to_string() } else { format!("{n} patches") }));
    let program = program(&at).map_err(Failure::before)?;
    // Past the threshold, the preset of the lowest priority.
    let presets = match p.lowest {
        true => Some(scratch.presets().map_err(|e| Failure::before(format!("the presets file for mach could not be written: {e}")))?),
        false => None,
    };
    // What it would push, checked first.
    update(db_path, id, |p| p.step = Step::Checking)?;
    let args = mach_args(&p, true);
    step(command_said(&args));
    let ran = run_mach(db_path, id, &program, &at, &args, presets.as_deref(), &mut |_| {}).map_err(Failure::before)?;
    if let Some(q) = asked(&ran.lines) {
        return Err(asked_failure(&q));
    }
    if ran.code != Some(0) {
        let why = format!("`./mach try fuzzy --no-push` failed, {}", exit_said(ran.code));
        return Err(Failure {
            said: with_lines(&format!("The try push was not made: {why}."), &ran.lines, "Nothing was pushed."),
            why,
        });
    }
    let Some(config) = calculated(&ran.lines) else {
        let why = "`./mach try fuzzy --no-push` did not say what it would push (its try_task_config.json)".to_string();
        return Err(Failure {
            said: with_lines(&format!("The try push was not made: {why}."), &ran.lines, "Nothing was pushed."),
            why,
        });
    };
    let differs = differences(&config, &p);
    if !differs.is_empty() {
        return Err(Failure {
            why: format!("`./mach try fuzzy --no-push` would push other than what you accepted: {}", differs.join(" ")),
            said: format!(
                "The try push was not made: `./mach try fuzzy --no-push` would push other than what you accepted. {} Nothing was pushed: reply to say what to select instead, and ask for the try push again, for the agent to select the tasks again.",
                differs.join(" ")
            ),
        });
    }
    say(db_path, id, "what mach would push is what you accepted");
    // The push, its login's page and code kept as mach waits for them, its
    // link once it says it.
    update(db_path, id, |p| p.step = Step::Pushing)?;
    let args = mach_args(&p, false);
    step(command_said(&args));
    let mut page: Option<String> = None;
    let mut linked = false;
    let ran = run_mach(db_path, id, &program, &at, &args, presets.as_deref(), &mut |line| {
        if let Some(url) = login_page(line) {
            page = Some(url);
        }
        if let (Some(code), Some(url)) = (login_code(line), page.clone()) {
            let _ = update(db_path, id, |p| p.login = Some(Login { url, code }));
        }
        if let Some(link) = pushed_link(line)
            && !linked
        {
            linked = true;
            let lando = lando_of(&link);
            let _ = update(db_path, id, |p| {
                p.link = Some(link);
                p.lando = lando;
                p.login = None;
                p.step = Step::Pushed;
            });
        }
    })
    .map_err(Failure::before)?;
    if linked {
        return Ok(());
    }
    // Not pushed, as far as stacked knows.
    if let Some(q) = asked(&ran.lines) {
        return Err(asked_failure(&q));
    }
    let maybe = ran.lines.iter().any(|l| l.trim_start().starts_with("Submitting"));
    let ending = match maybe {
        true => "mach had started to send the push to Lando, which may have it: stacked does not know where it is.",
        false => "Nothing was pushed.",
    };
    let how = match ran.code {
        Some(0) => "exited without saying where the push is".to_string(),
        code => format!("failed, {}, before it said it pushed", exit_said(code)),
    };
    Err(Failure {
        why: format!("`./mach try fuzzy` {how}"),
        said: with_lines(&format!("The try push failed: `./mach try fuzzy` {how}."), &ran.lines, ending),
    })
}

/// Try push `id`, pushed: its link a reply on the comment that asked, a web
/// link, and the comment kept locked while the push is followed.
fn pushed_to_try(db_path: &Path, id: &str) -> Result<()> {
    Db::edit(db_path, |db| {
        let r = db.run(id).cloned().ok_or_else(|| Error(format!("no run {id}")))?;
        let Some(p) = r.push.clone().filter(|_| r.state == RunState::Started) else { return Ok(()) };
        let Some(link) = p.link.clone() else { return Ok(()) };
        reply(db, &p, &|n| pushed_said(&p, &link, crate::refs::next_of(n)))?;
        db.add_run(Run {
            pid: None,
            starter: None,
            summary: format!("pushed to try: {link}"),
            push: Some(Box::new(Push { login: None, step: Step::Pushed, ..*p })),
            ..r
        })?;
        Ok(())
    })
}

/// The reply saying that try push `p` was made, its link reference `k`.
fn pushed_said(p: &Push, link: &str, k: usize) -> String {
    let mut how = String::new();
    if let Some(n) = p.rebuild {
        how += &format!(", each to run {n} times");
    }
    if p.lowest {
        how += ", at the lowest priority";
    }
    let text = format!(
        "stacked pushed {} to try, with {}{how}. Treeherder shows the push: @r{k}.",
        tasks_said(p.labels.len()),
        patches_said(p.patches, p.whole)
    );
    crate::refs::attach(&text, &[Pointing { n: Some(k), to: Pointer::Link(link.to_string()) }])
}

/// Stops try push `id`: once pushed, following it, which cannot stop it in
/// CI; before mach said it pushed, the push, its process stopped, mach with
/// it. The comment unlocks, and a reply on it says so, with the push's link
/// once there is one. What came of it, said for the status line.
pub fn stop(db_path: &Path, id: &str) -> Result<String> {
    let r = Db::load(db_path)?.run(id).cloned().ok_or_else(|| Error(format!("no run {id}")))?;
    let Some(p) = r.push.clone() else { return err(format!("run {id} is not a try push")) };
    if r.state != RunState::Started {
        return Ok("the try push is over already".into());
    }
    if p.step != Step::Pushed
        && let Some(pid) = r.pid
    {
        crate::ask::stop_group(pid);
    }
    // Where it is now: pushed meanwhile, perhaps.
    let r = Db::load(db_path)?.run(id).cloned().ok_or_else(|| Error(format!("no run {id}")))?;
    let Some(p) = r.push.clone().filter(|_| r.state == RunState::Started) else {
        return Ok(format!("the try push was over before it could be stopped: {}", r.summary));
    };
    match &p.link {
        Some(link) => {
            let said = |n: &Note| {
                let k = crate::refs::next_of(n);
                let text = format!("stacked stopped following the try push: it goes on in CI, and Treeherder shows it: @r{k}.");
                crate::refs::attach(&text, &[Pointing { n: Some(k), to: Pointer::Link(link.clone()) }])
            };
            end(db_path, id, RunState::Done, "stopped following", &said)?;
            Ok("stopped following the try push: it goes on in CI, and the comment is unlocked".into())
        }
        None => {
            let maybe = Db::load(db_path)?.events(id).iter().any(|e| e.get("text").and_then(Value::as_str).is_some_and(|t| t.trim_start().starts_with("Submitting")));
            let ending = match maybe {
                true => "mach had started to send the push to Lando, which may have it: stacked does not know where it is.",
                false => "nothing was pushed.",
            };
            let said = format!("The try push was stopped while mach ran, before it said it pushed: {ending}");
            end(db_path, id, RunState::Failed, "stopped before mach said it pushed", &|_| said.clone())?;
            Ok(format!("the try push was stopped: {ending}"))
        }
    }
}

/// The try pushes whose process is gone before mach said it pushed, as
/// `db` has them, ended as failed, each with a reply saying so: how many.
pub fn orphans(db_path: &Path, db: &Db) -> usize {
    let gone: Vec<String> = db
        .live_runs()
        .into_iter()
        .filter(|r| r.state == RunState::Started && r.push.as_ref().is_some_and(|p| p.step != Step::Pushed))
        .filter(|r| match (r.pid, r.starter) {
            (Some(pid), _) => !crate::ask::alive(pid),
            (None, Some(starter)) => !crate::ask::alive(starter),
            (None, None) => now() - r.time > 120,
        })
        .map(|r| r.id.clone())
        .collect();
    let said = "The try push stopped before mach said it pushed: the process pushing it is gone, and stacked does not know whether mach sent the push.";
    gone.iter()
        .filter(|id| end(db_path, id, RunState::Failed, "the process pushing it is gone", &|_| said.to_string()).is_ok())
        .count()
}

// -- following the push ------------------------------------------------------- //

/// Where Treeherder answers: its pages, and its API under `/api`.
const TREEHERDER: &str = "https://treeherder.mozilla.org";

/// How long a followed push goes between looks, in seconds: a few minutes.
const LOOK_EVERY: i64 = 180;

/// How many looks in a row that find nothing of a push pending or running,
/// and nothing new, end it: what is missing then never shows.
const QUIET: u32 = 2;

/// The seconds between looks, when the tests set them.
static EVERY: std::sync::Mutex<Option<i64>> = std::sync::Mutex::new(None);

/// Looks at the pushes followed every `secs` seconds from now on, in this
/// process: the tests'.
pub fn look_every(secs: i64) {
    *EVERY.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(secs);
}

fn every() -> i64 {
    EVERY.lock().unwrap_or_else(std::sync::PoisonError::into_inner).unwrap_or(LOOK_EVERY)
}

/// Whether run `r` is a try push followed that is due a look at `now`:
/// pushed, and not looked at for a few minutes.
pub fn due(r: &Run, now: i64) -> bool {
    r.state == RunState::Started && r.push.as_ref().is_some_and(|p| p.step == Step::Pushed && now - p.looked >= every())
}

/// The try pushes followed that are due a look, of `db`: their runs.
pub fn due_now(db: &Db) -> Vec<String> {
    let now = now();
    db.live_runs().into_iter().filter(|r| due(r, now)).map(|r| r.id.clone()).collect()
}

/// A look at each try push followed that is due one (`follow`): what came
/// of them, said for the status line.
pub fn follow_due(db_path: &Path) -> Vec<String> {
    let Ok(db) = Db::load(db_path) else { return vec![] };
    due_now(&db).iter().filter_map(|id| follow(db_path, id).ok().flatten()).collect()
}

/// What answers the GETs that follow pushes, in place of the web, when set:
/// the tests', which never reach Lando nor Treeherder.
pub type Get = Box<dyn Fn(&str) -> std::result::Result<String, String> + Send>;

static WEB: std::sync::Mutex<Option<Get>> = std::sync::Mutex::new(None);

/// Has `get` answer the GETs that follow pushes; with none, the web again.
pub fn web_stand_in(get: Option<Get>) {
    *WEB.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = get;
}

/// What `url` answers, as JSON: a plain GET of a public page (Lando's,
/// Treeherder's), with nothing of yours sent, by curl. The tests' stand-in,
/// when one is set; never the web from a process that keeps nothing, as
/// the tests' own.
fn get(url: &str) -> std::result::Result<Value, String> {
    let body = {
        let web = WEB.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        match web.as_ref() {
            Some(get) => get(url)?,
            None if crate::home::get().dir == Path::new(crate::home::NOWHERE) => {
                return Err("Lando and Treeherder are not reached from a process that keeps nothing, as the tests' own".into());
            }
            None => {
                let out = Command::new("curl")
                    .args(["-fsSL", "--max-time", "30", "-A", "stacked", "-H", "Accept: application/json"])
                    .arg(url)
                    .stdin(Stdio::null())
                    .output()
                    .map_err(|e| format!("curl: {e}"))?;
                if !out.status.success() {
                    return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
                }
                String::from_utf8_lossy(&out.stdout).into_owned()
            }
        }
    };
    serde_json::from_str(&body).map_err(|_| format!("{url} answered no JSON"))
}

/// Where Lando's instance `instance` answers: as `.lando.ini` at `dir`
/// says it (`api_domain` in its section whose `instance_id` is that, or
/// named so when it names none), as mach reads it; else Lando's own, which
/// stacked knows (`lando-prod-2025`, lando.moz.tools).
pub fn lando_api(dir: Option<&Path>, instance: &str) -> Option<String> {
    let text = dir.and_then(|d| std::fs::read_to_string(d.join(".lando.ini")).ok()).unwrap_or_default();
    // Each section's name, instance_id and api_domain.
    let mut sections: Vec<(String, Option<String>, Option<String>)> = Vec::new();
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            sections.push((name.trim().to_string(), None, None));
            continue;
        }
        let (Some((key, value)), Some(s)) = (line.split_once('='), sections.last_mut()) else { continue };
        match key.trim() {
            "instance_id" => s.1 = Some(value.trim().to_string()),
            "api_domain" => s.2 = Some(value.trim().to_string()),
            _ => {}
        }
    }
    let known = sections.into_iter().find_map(|(name, id, api)| (id.as_deref().unwrap_or(&name) == instance).then_some(api).flatten());
    known.or_else(|| (instance == "lando-prod-2025").then(|| "lando.moz.tools".to_string()))
}

/// What one look at a push found.
#[derive(Debug, PartialEq)]
enum Looked {
    /// Lando does not have it on try yet: its state there.
    Lando(String),
    /// Lando did not push it to try: its state, why it says, and its page.
    NotPushed { status: String, error: String, page: Option<String> },
    /// On try, as its revision: its push on Treeherder, if Treeherder has
    /// it, and its tasks as Treeherder shows them.
    OnTry { revision: String, push: Option<u64>, seen: Option<Seen> },
    /// Lando or Treeherder could not be read: why.
    Unread(String),
}

/// A look at try push `p`, pushed: on Lando, until it pushes it to try;
/// then on Treeherder, its push, then its tasks. `dir` is where its
/// `.lando.ini` is.
fn look(p: &Push, dir: Option<&Path>) -> Looked {
    let revision = match &p.revision {
        Some(r) => r.clone(),
        None => {
            let Some(l) = &p.lando else {
                return Looked::Unread("where Lando has the push is not known: its link names no Lando job".into());
            };
            let Some(api) = lando_api(dir, &l.instance) else {
                return Looked::Unread(format!("stacked does not know where Lando's {} answers: its .lando.ini does not say", l.instance));
            };
            let job = match get(&format!("https://{api}/landing_jobs/{}/", l.job)) {
                Ok(job) => job,
                Err(e) => return Looked::Unread(format!("Lando could not be read: {e}")),
            };
            let status = job.get("status").and_then(Value::as_str).unwrap_or("").to_string();
            match status.as_str() {
                "LANDED" => match job.get("commit_id").and_then(Value::as_str).filter(|c| !c.is_empty()) {
                    Some(c) => c.to_string(),
                    None => return Looked::Unread("Lando says it pushed it, and names no revision".into()),
                },
                "FAILED" | "CANCELLED" | "ABORTED" => {
                    return Looked::NotPushed {
                        status,
                        error: job.get("error").and_then(Value::as_str).unwrap_or("").trim().to_string(),
                        page: job.get("url").and_then(Value::as_str).and_then(|u| crate::refs::link(u).ok()),
                    };
                }
                _ => return Looked::Lando(status),
            }
        }
    };
    let push = match p.treeherder {
        Some(push) => push,
        None => match get(&format!("{TREEHERDER}/api/project/try/push/?revision={revision}")) {
            Ok(v) => match v.pointer("/results/0/id").and_then(Value::as_u64) {
                Some(push) => push,
                None => return Looked::OnTry { revision, push: None, seen: None },
            },
            Err(e) => return Looked::Unread(format!("Treeherder could not be read: {e}")),
        },
    };
    match jobs(push) {
        Ok(rows) => {
            let seen = seen_of(&rows, p, &revision);
            Looked::OnTry { revision, push: Some(push), seen: Some(seen) }
        }
        Err(e) => Looked::Unread(format!("Treeherder could not be read: {e}")),
    }
}

/// The jobs of push `push` on Treeherder, each by its properties' names,
/// page after page.
fn jobs(push: u64) -> std::result::Result<Vec<Map<String, Value>>, String> {
    let mut url = Some(format!("{TREEHERDER}/api/jobs/?push_id={push}&count=2000"));
    let mut out = Vec::new();
    // Bounded: a push of fewer than 100,000 jobs.
    for _ in 0..50 {
        let Some(at) = url.take() else { break };
        let page = get(&at)?;
        let names: Vec<String> = page.get("job_property_names").and_then(Value::as_array).into_iter().flatten().filter_map(|n| n.as_str().map(String::from)).collect();
        for row in page.get("results").and_then(Value::as_array).into_iter().flatten() {
            let Some(values) = row.as_array() else { continue };
            out.push(names.iter().cloned().zip(values.iter().cloned()).collect());
        }
        url = page.get("next").and_then(Value::as_str).filter(|n| n.starts_with(TREEHERDER)).map(String::from);
    }
    Ok(out)
}

/// Try push `p`'s tasks as the Treeherder jobs `rows` of its push, revision
/// `revision`, show them: each task's last run (its retry), those of the
/// labels it pushed counted, each label as many times as it runs, those it
/// does not show as many times not seen; the others' failures (what the push
/// ran for them) kept apart.
pub fn seen_of(rows: &[Map<String, Value>], p: &Push, revision: &str) -> Seen {
    let text = |r: &Map<String, Value>, k: &str| r.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let number = |r: &Map<String, Value>, k: &str| r.get(k).and_then(Value::as_u64).unwrap_or(0);
    // Each task's last run.
    let mut last: std::collections::BTreeMap<String, &Map<String, Value>> = std::collections::BTreeMap::new();
    for r in rows {
        let task = text(r, "task_id");
        if last.get(&task).is_none_or(|was| number(was, "retry_id") <= number(r, "retry_id")) {
            last.insert(task, r);
        }
    }
    let shown = |r: &Map<String, Value>| Shown {
        label: text(r, "job_type_name"),
        result: text(r, "result"),
        link: format!("{TREEHERDER}/jobs?repo=try&revision={revision}&selectedTaskRun={}.{}", text(r, "task_id"), number(r, "retry_id")),
    };
    let mut seen = Seen { going: last.is_empty(), ..Default::default() };
    let mut of: std::collections::HashMap<String, Vec<&Map<String, Value>>> = std::collections::HashMap::new();
    for r in last.values() {
        let completed = text(r, "state") == "completed";
        if !completed {
            seen.going = true;
        }
        let label = text(r, "job_type_name");
        if p.labels.contains(&label) {
            of.entry(label).or_default().push(r);
        } else if completed && text(r, "result") != "success" {
            seen.besides.push(shown(r));
        }
    }
    let runs = p.rebuild.unwrap_or(1) as usize;
    for label in &p.labels {
        let these = of.get(label).map(Vec::as_slice).unwrap_or(&[]);
        seen.total += runs.max(these.len());
        if these.len() < runs {
            seen.unseen.push((label.clone(), runs - these.len()));
        }
        for r in these.iter().filter(|r| text(r, "state") == "completed") {
            seen.done += 1;
            let result = text(r, "result");
            *seen.results.entry(result.clone()).or_default() += 1;
            if result != "success" {
                seen.failed.push(shown(r));
            }
        }
    }
    seen.besides.sort_by(|a, b| a.label.cmp(&b.label));
    seen
}

/// One look at try push `id`, followed, if it is due one: taken under the
/// lock, so that no other look is made meanwhile, then Lando and Treeherder
/// read away from it, then what they say kept. Its stream says what
/// changed. Over, its summary lands for your review, as an answer only's
/// reply does, and the comment unlocks. What came of it, for the status
/// line, when it is over.
pub fn follow(db_path: &Path, id: &str) -> Result<Option<String>> {
    let now = now();
    let taken = Db::edit(db_path, |db| {
        let Some(r) = db.run(id).cloned().filter(|r| due(r, now)) else { return Ok(None) };
        let Some(mut p) = r.push.clone() else { return Ok(None) };
        p.looked = now;
        db.add_run(Run { push: Some(p.clone()), ..r })?;
        Ok(Some(p))
    })?;
    let Some(p) = taken else { return Ok(None) };
    let looked = look(&p, p.worktree.as_deref().map(Path::new));
    Db::edit(db_path, |db| {
        let r = db.run(id).cloned().ok_or_else(|| Error(format!("no run {id}")))?;
        let Some(mut p) = r.push.clone().filter(|_| r.state == RunState::Started && r.push.as_ref().is_some_and(|p| p.step == Step::Pushed)) else {
            return Ok(None);
        };
        let mut said: Vec<String> = Vec::new();
        let mut over: Option<(String, String, String)> = None;
        if !matches!(looked, Looked::Unread(_)) {
            p.trouble = None;
        }
        match looked {
            Looked::Lando(status) => {
                if p.lando_status.as_deref() != Some(status.as_str()) {
                    said.push(format!("Lando has the push: {status}"));
                    p.lando_status = Some(status);
                }
            }
            Looked::NotPushed { status, error, page } => {
                let error = error.trim_end_matches('.');
                let why = if error.is_empty() { status.clone() } else { format!("{status}: {error}") };
                said.push(format!("Lando did not push it to try: {why}"));
                let summary = format!("Lando did not push it to try: {why}");
                let news = format!("the try push did not get to try, as Lando says {why}: its summary waits for your review");
                let first = crate::refs::next_of(&db.note(&p.change, &p.id).cloned().unwrap_or_default());
                over = Some((summary, news, not_pushed_said(&why, page.as_deref(), first)));
            }
            Looked::Unread(why) => {
                if p.trouble.as_deref() != Some(why.as_str()) {
                    said.push(format!("{why}: stacked looks again every few minutes"));
                    p.trouble = Some(why);
                }
            }
            Looked::OnTry { revision, push, seen } => {
                if p.revision.is_none() {
                    said.push(format!("Lando pushed it to try, as revision {}", &revision[..12.min(revision.len())]));
                    p.revision = Some(revision);
                    p.lando_status = Some("LANDED".into());
                }
                if p.treeherder.is_none() && push.is_some() {
                    said.push("Treeherder has the push".into());
                    p.treeherder = push;
                }
                if let Some(seen) = seen {
                    let quiet = !seen.going && p.seen.as_ref() == Some(&seen);
                    p.quiet = if quiet { p.quiet + 1 } else { 0 };
                    if p.seen.as_ref().map(Seen::said) != Some(seen.said()) {
                        said.push(seen.said());
                    }
                    let done = (seen.done == seen.total && seen.unseen.is_empty()) || p.quiet >= QUIET;
                    if done {
                        let link = p.link.clone().unwrap_or_default();
                        let first = crate::refs::next_of(&db.note(&p.change, &p.id).cloned().unwrap_or_default());
                        over = Some((format!("done: {}", results_said(&seen)), format!("the try push is done: {}: its summary waits for your review", results_said(&seen)), summary_said(&p, &seen, &link, first)));
                    }
                    p.seen = Some(seen);
                }
            }
        }
        for line in &said {
            db.add_event(id, serde_json::json!({"type": "text", "text": format!("stacked: {line}")}))?;
        }
        match over {
            // Over: its summary held for your review, as an answer only's
            // reply is; the comment unlocks.
            Some((summary, news, text)) => {
                let held = crate::held::Held {
                    at: now,
                    replies: vec![crate::held::Reply {
                        change: p.change.clone(),
                        id: p.id.clone(),
                        text,
                        tag: "try".into(),
                        ..Default::default()
                    }],
                    ..Default::default()
                };
                db.add_run(Run { state: RunState::Done, summary, held: Some(held), pid: None, starter: None, push: Some(p), ..r })?;
                Ok(Some(news))
            }
            None => {
                db.add_run(Run { push: Some(p), ..r })?;
                Ok(None)
            }
        }
    })
}

/// How the tasks of a push seen as `seen` ended, said: `27 success, 2
/// testfailed, 1 not seen on Treeherder`.
fn results_said(seen: &Seen) -> String {
    let mut by: Vec<(&String, &usize)> = seen.results.iter().collect();
    by.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    let mut out: Vec<String> = by.into_iter().map(|(r, n)| format!("{} {r}", thousands(*n))).collect();
    let still = seen.total - seen.done - seen.unseen_count().min(seen.total - seen.done);
    if still > 0 {
        out.push(format!("{} not done", thousands(still)));
    }
    if seen.unseen_count() > 0 {
        out.push(format!("{} not seen on Treeherder", thousands(seen.unseen_count())));
    }
    if out.is_empty() {
        out.push("none seen".into());
    }
    out.join(", ")
}

/// `items`, said: `a`, `a and b`, `a, b and c`.
fn and(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// The summary of try push `p`, done, as `seen` shows its tasks: its link,
/// the counts of its tasks by result, those not seen on Treeherder apart,
/// then each that failed, with its link, and what the push ran for them
/// that failed; each link a reference numbered from `first`.
fn summary_said(p: &Push, seen: &Seen, link: &str, first: usize) -> String {
    let mut refs: Vec<Pointing> = Vec::new();
    let mut named = |url: &str| -> String {
        let k = first + refs.len();
        refs.push(Pointing { n: Some(k), to: Pointer::Link(url.to_string()) });
        format!("@r{k}")
    };
    let push = named(link);
    let tasks = if seen.total == 1 { "task" } else { "tasks" };
    let of = match p.rebuild {
        Some(n) => format!("Of its {} {tasks}, the {} you accepted each run {n} times,", thousands(seen.total), thousands(p.labels.len())),
        None => format!("Of its {} {tasks},", thousands(seen.total)),
    };
    let mut by: Vec<(&String, &usize)> = seen.results.iter().collect();
    by.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    let shows: Vec<String> = by.into_iter().map(|(r, n)| format!("{} as {r}", thousands(*n))).collect();
    let mut text = format!("The try push is done. Treeherder shows it: {push}.\n\n");
    text += &match shows.is_empty() {
        true => format!("{of} Treeherder shows none done"),
        false => format!("{of} Treeherder shows {}", and(&shows)),
    };
    let still = seen.total - seen.done - seen.unseen_count().min(seen.total - seen.done);
    if still > 0 {
        text += &format!("; {} never ended", thousands(still));
    }
    if seen.unseen_count() > 0 {
        let n = seen.unseen_count();
        text += &format!("; {} {} not seen on Treeherder", thousands(n), if n == 1 { "was" } else { "were" });
    }
    text += ".";
    if !seen.failed.is_empty() {
        text += "\n\nThese failed:\n";
        for f in &seen.failed {
            text += &format!("\n- `{}` ({}): {}", f.label, f.result, named(&f.link));
        }
    }
    if !seen.besides.is_empty() {
        text += "\n\nThe push ran these for them too, and they failed:\n";
        for f in &seen.besides {
            text += &format!("\n- `{}` ({}): {}", f.label, f.result, named(&f.link));
        }
    }
    if !seen.unseen.is_empty() {
        let listed: Vec<String> = seen
            .unseen
            .iter()
            .map(|(l, n)| match p.rebuild {
                Some(runs) => format!("`{l}` ({n} of its {runs} runs)"),
                None => format!("`{l}`"),
            })
            .collect();
        text += &format!("\n\nNot seen on Treeherder, which shows no task that was never scheduled: {}.", listed.join(", "));
    }
    crate::refs::attach(&text, &refs)
}

/// The summary of a try push that Lando did not push to try, `why`, its
/// page on Lando, if it said one, a reference numbered `first`.
fn not_pushed_said(why: &str, page: Option<&str>, first: usize) -> String {
    match page {
        Some(page) => {
            let text = format!("The try push did not get to try: Lando says {why}. None of its tasks ran. Lando shows it: @r{first}.");
            crate::refs::attach(&text, &[Pointing { n: Some(first), to: Pointer::Link(page.to_string()) }])
        }
        None => format!("The try push did not get to try: Lando says {why}. None of its tasks ran."),
    }
}

/// Ends try push `id` as `state` says, `why` its summary, `said` the reply
/// on the comment that asked, if it is open, given that comment: the
/// comment unlocks. Nothing, if it is over already.
fn end(db_path: &Path, id: &str, state: RunState, why: &str, said: &dyn Fn(&Note) -> String) -> Result<()> {
    Db::edit(db_path, |db| {
        let r = db.run(id).cloned().ok_or_else(|| Error(format!("no run {id}")))?;
        let Some(p) = r.push.clone().filter(|_| r.state == RunState::Started) else { return Ok(()) };
        reply(db, &p, said)?;
        db.add_run(Run {
            state,
            summary: why.to_string(),
            pid: None,
            starter: None,
            push: Some(Box::new(Push { login: None, ..*p })),
            ..r
        })?;
        Ok(())
    })
}

/// `said`, given the comment that asked for try push `p`, a reply of the
/// try push's on it, if it is open.
fn reply(db: &mut Db, p: &Push, said: &dyn Fn(&Note) -> String) -> Result<()> {
    let Some(n) = db.note(&p.change, &p.id).filter(|n| !n.text.is_empty()).cloned() else { return Ok(()) };
    let text = append_turn(&n.text, Role::Agent, &said(&n), Some("try"));
    db.set_note(Note { text, time: now(), ..n })
}

/// Try push `id`'s push changed by `change`, under the lock: unless it is
/// over.
fn update(db_path: &Path, id: &str, change: impl FnOnce(&mut Push)) -> Result<()> {
    Db::edit(db_path, |db| {
        let r = db.run(id).cloned().ok_or_else(|| Error(format!("no run {id}")))?;
        let Some(mut p) = r.push.clone().filter(|_| r.state == RunState::Started) else { return Ok(()) };
        change(&mut p);
        db.add_run(Run { push: Some(p), ..r })?;
        Ok(())
    })
}

/// The type of the events that say what a push runs, as an ask's starting
/// commands are: `$ command` in its stream.
const STEP: &str = "stacked";

/// Appends `text` to run `id`'s stream, as an event of type `kind`.
fn record(db_path: &Path, id: &str, kind: &str, text: &str) {
    let event = serde_json::json!({"type": kind, "text": text});
    let _ = Db::append(db_path, &[Record::Event { run: id.into(), event }]);
}

/// What stacked itself says in run `id`'s stream: `stacked: …`.
fn say(db_path: &Path, id: &str, text: &str) {
    record(db_path, id, "text", &format!("stacked: {text}"));
}

/// The first free worktree of `dir` for pushes (`try`, `try-2`…), taken
/// for try push `id`: none other being pushed from it. Written as its
/// worktree, so that no other push takes it.
fn take_worktree(db_path: &Path, id: &str, dir: &Path) -> Result<PathBuf> {
    let mut k = 1;
    loop {
        let at = crate::ask::slot(dir, TRY, k);
        k += 1;
        let path = at.to_string_lossy().into_owned();
        let taken = Db::edit(db_path, |db| {
            let busy = db.live_runs().iter().any(|r| {
                r.id != id
                    && r.state == RunState::Started
                    && r.push.as_ref().is_some_and(|p| p.step != Step::Pushed && p.worktree.as_deref() == Some(path.as_str()))
            });
            if busy {
                return Ok(false);
            }
            let r = db.run(id).cloned().ok_or_else(|| Error(format!("no run {id}")))?;
            let Some(mut p) = r.push.clone() else { return err(format!("run {id} is not a try push")) };
            p.worktree = Some(path.clone());
            db.add_run(Run { push: Some(p), ..r })?;
            Ok(true)
        })?;
        if taken {
            return Ok(at);
        }
    }
}

/// The program a push runs as mach in worktree `at`: the one
/// `$STACKED_MACH` names, as the tests' stand-in is passed on; else the
/// worktree's own `./mach`. Never that from a process that keeps nothing,
/// as the tests' own: there, only a stand-in runs.
fn program(at: &Path) -> std::result::Result<PathBuf, String> {
    let given = std::env::var_os("STACKED_MACH").filter(|p| !p.is_empty()).map(PathBuf::from);
    program_of(at, given, crate::home::get().dir == Path::new(crate::home::NOWHERE))
}

/// As `program`, the stand-in `given` if any, and whether this process
/// keeps nothing (`nowhere`) said.
fn program_of(at: &Path, given: Option<PathBuf>, nowhere: bool) -> std::result::Result<PathBuf, String> {
    match given {
        Some(p) => Ok(p),
        None if nowhere => Err("mach is not run from a process that keeps nothing, as the tests' own: only a stand-in for it is".into()),
        None => Ok(at.join("mach")),
    }
}

/// The remotes that the checkout at `dir` fetches from that mach takes as
/// Firefox's own, by name, as mach's git support decides it
/// (mozversioncontrol's `get_mozilla_upstream_remotes`): those fetching
/// from github.com/mozilla-firefox/, and, in a git-cinnabar clone, from
/// hg.mozilla.org, but its try.
pub fn official_remotes(dir: &Path) -> Result<Vec<String>> {
    let out = crate::git::git(dir, &["remote", "-v"])?;
    let cinnabar = is_cinnabar(dir);
    let mut names: Vec<String> = Vec::new();
    for line in out.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        let [name, url, action, ..] = parts[..] else { continue };
        if action != "(fetch)" {
            continue;
        }
        let hg = cinnabar && url.contains("hg.mozilla.org") && !url.ends_with("hg.mozilla.org/try") && !url.ends_with("hg.mozilla.org/try/");
        let github = url.contains("github.com/mozilla-firefox/") || url.contains("github.com:mozilla-firefox/");
        if (hg || github) && !names.iter().any(|n| n == name) {
            names.push(name.to_string());
        }
    }
    Ok(names)
}

/// Whether the checkout at `dir` is a git-cinnabar clone, as mach tells:
/// not when it has the first commit of Firefox's own git repository; else
/// when it has refs of git-cinnabar's.
fn is_cinnabar(dir: &Path) -> bool {
    if crate::git::git(dir, &["cat-file", "-e", "2ca566cd74d5d0863ba7ef0529a4f88b2823eb43^{commit}"]).is_ok() {
        return false;
    }
    crate::git::git(dir, &["for-each-ref", "--count=1", "refs/cinnabar"]).is_ok_and(|o| !o.trim().is_empty())
}

/// How git is told the remotes `names`, as mach tells it what it leaves
/// out of a push: `--remotes=NAME` each; with none, `--remotes`, every
/// remote's.
pub fn remote_args(names: &[String]) -> Vec<String> {
    match names {
        [] => vec!["--remotes".into()],
        names => names.iter().map(|n| format!("--remotes={n}")).collect(),
    }
}

/// The remotes `names`, said: `origin, the remote mach takes as
/// Firefox's`.
fn remotes_said(names: &[String]) -> String {
    match names {
        [] => "any remote (mach takes none as Firefox's, and so goes by them all)".into(),
        [one] => format!("{one}, the remote mach takes as Firefox's"),
        many => format!("any of {}, the remotes mach takes as Firefox's", many.join(", ")),
    }
}

/// Why mach would not push a stack's patches alone on base `base` in the
/// checkout at `dir`, if it would not: as it pushes every commit that none
/// of the remotes it takes as Firefox's has, the base must be on one of
/// them. Asked before the review closes, the patches not yet commits.
pub fn base_problem(dir: &Path, base: &str) -> Option<String> {
    let names = match official_remotes(dir) {
        Ok(names) => names,
        Err(e) => return Some(e.0),
    };
    if names.is_empty() && crate::git::git(dir, &["remote"]).is_ok_and(|o| o.trim().is_empty()) {
        return Some("the repository has no remote, and mach pushes every commit that no remote has: it would push the stack's whole history".into());
    }
    let mut args = vec!["rev-list".to_string(), "--count".into(), base.to_string(), "--not".into()];
    args.extend(remote_args(&names));
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let n: usize = match crate::git::git(dir, &args) {
        Ok(out) => out.trim().parse().unwrap_or(0),
        Err(e) => return Some(e.0),
    };
    let short = &base[..12.min(base.len())];
    (n > 0).then(|| {
        format!(
            "the stack's base, commit {short}, is not on {}: mach would push {n} commit{} of the base's besides the stack's patches. Fetch that remote (`git fetch`), or put the stack on a commit that it has",
            remotes_said(&names),
            if n == 1 { "" } else { "s" }
        )
    })
}

/// Why the commits mach would push from HEAD at `dir` are not the `n`
/// patches on top of `base` alone, if they are not, `names` the remotes it
/// takes as Firefox's.
fn commits_problem(dir: &Path, base: &str, n: usize, names: &[String]) -> Option<String> {
    if let Some(why) = base_problem(dir, base) {
        return Some(why);
    }
    let mut args = vec!["rev-list".to_string(), "--count".into(), "HEAD".into(), "--not".into()];
    args.extend(remote_args(names));
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let count: usize = match crate::git::git(dir, &args) {
        Ok(out) => out.trim().parse().unwrap_or(0),
        Err(e) => return Some(e.0),
    };
    let patches = |k: usize| if k == 1 { "1 patch".to_string() } else { format!("{k} patches") };
    match count {
        c if c == n => None,
        c if c > n => Some(format!("mach would push {c} commits, more than the stack's {}", patches(n))),
        c => Some(format!("mach would push {c} of the stack's {}: the others are on a remote that mach takes as Firefox's already", patches(n))),
    }
}

/// What mach is run with: `try fuzzy`, the tasks in one query, each task
/// an exact term of its own (`^LABEL$`), the terms joined by fzf's OR
/// (`^a$ | ^b$`), so that fzf runs once; the labels as the task graph has
/// them, chunk numbers and all (`--show-chunk-numbers`), from every task it
/// has (`--all-tasks`); how many times each runs; past the threshold, the
/// preset of the lowest priority; `--no-push` to see what it would push.
pub fn mach_args(p: &Push, check: bool) -> Vec<String> {
    let mut out: Vec<String> = vec!["try".into(), "fuzzy".into()];
    if check {
        out.push("--no-push".into());
    }
    out.extend(["--show-chunk-numbers".to_string(), "--all-tasks".to_string()]);
    if let Some(n) = p.rebuild {
        out.extend(["--rebuild".to_string(), n.to_string()]);
    }
    if p.lowest {
        out.extend(["--preset".to_string(), LOWEST.to_string()]);
    }
    let terms: Vec<String> = p.labels.iter().map(|l| format!("^{l}$")).collect();
    out.extend(["-q".to_string(), terms.join(" | ")]);
    out
}

/// mach's command line `args`, said in the stream: `./mach try fuzzy …`,
/// each argument quoted as a shell would need it, a query of more than two
/// terms cut after them, with how many it has.
fn command_said(args: &[String]) -> String {
    let mut out = vec!["./mach".to_string()];
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "-q" {
            let q = it.next().map(String::as_str).unwrap_or("");
            let terms: Vec<&str> = q.split(" | ").collect();
            match terms.len() {
                n if n > 2 => out.push(format!("-q {} (one query, a term for each of the {} tasks)", quoted(&format!("{} | {} | …", terms[0], terms[1])), thousands(n))),
                _ => out.push(format!("-q {}", quoted(q))),
            }
            continue;
        }
        out.push(quoted(a));
    }
    out.join(" ")
}

/// What mach wrote, each line, and its exit code (none when it was
/// killed).
struct Ran {
    lines: Vec<String>,
    code: Option<i32>,
}

/// Runs `program` as mach in worktree `at` with `args`: its stdin closed,
/// so that it reads nothing; its output not held back (Python's, by
/// `PYTHONUNBUFFERED`); `presets` the presets file it reads, if any. Each
/// line it writes, out or error, is in run `id`'s stream as it comes, and
/// `seen` is told it.
fn run_mach(db_path: &Path, id: &str, program: &Path, at: &Path, args: &[String], presets: Option<&Path>, seen: &mut dyn FnMut(&str)) -> std::result::Result<Ran, String> {
    let mut c = Command::new(program);
    c.args(args).current_dir(at).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    c.env("PYTHONUNBUFFERED", "1");
    if let Some(file) = presets {
        c.env("MACH_TRY_PRESET_PATHS", file);
    }
    let mut child = c.spawn().map_err(|e| format!("{} could not be run: {e}", program.display()))?;
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    let mut readers = Vec::new();
    let out: Option<Box<dyn std::io::Read + Send>> = child.stdout.take().map(|o| Box::new(o) as Box<dyn std::io::Read + Send>);
    let err: Option<Box<dyn std::io::Read + Send>> = child.stderr.take().map(|e| Box::new(e) as Box<dyn std::io::Read + Send>);
    for pipe in [out, err].into_iter().flatten() {
        let tx = tx.clone();
        readers.push(std::thread::spawn(move || {
            let mut reader = std::io::BufReader::new(pipe);
            let mut buf = Vec::new();
            loop {
                buf.clear();
                match reader.read_until(b'\n', &mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        let _ = tx.send(String::from_utf8_lossy(&buf).into_owned());
                    }
                }
            }
        }));
    }
    drop(tx);
    let mut lines = Vec::new();
    for line in rx {
        let line = plain(&line);
        if line.trim().is_empty() {
            continue;
        }
        record(db_path, id, "text", &line);
        seen(&line);
        lines.push(line);
    }
    for r in readers {
        let _ = r.join();
    }
    let status = child.wait().map_err(|e| format!("{}: {e}", program.display()))?;
    Ok(Ran { lines, code: status.code() })
}

/// `line` as the stream shows it: the colours of a terminal's escapes taken
/// out, and what a terminal cannot show.
fn plain(line: &str) -> String {
    let mut out = String::new();
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' && chars.peek() == Some(&'[') {
            chars.next();
            for d in chars.by_ref() {
                if ('\x40'..='\x7e').contains(&d) {
                    break;
                }
            }
            continue;
        }
        out.push(c);
    }
    crate::claude::clean(out.trim_end_matches(['\n', '\r']))
}

/// How mach exited, said: `exiting with 1`, or `killed`.
fn exit_said(code: Option<i32>) -> String {
    code.map_or("killed".into(), |c| format!("exiting with {c}"))
}

/// A reply of `head`, then the last lines mach wrote, `lines`, up to three
/// in a block of their own, then `tail`.
fn with_lines(head: &str, lines: &[String], tail: &str) -> String {
    let last: Vec<&str> = lines.iter().map(String::as_str).filter(|l| !l.trim().is_empty()).rev().take(3).collect();
    match last.is_empty() {
        true => format!("{head} {tail}"),
        false => format!("{head} Its last lines:\n\n```text\n{}\n```\n\n{tail}", last.into_iter().rev().collect::<Vec<_>>().join("\n")),
    }
}

/// The try_task_config.json that `./mach try fuzzy --no-push` says it
/// would push, after its "Calculated try_task_config.json:" line.
pub fn calculated(lines: &[String]) -> Option<Value> {
    let at = lines.iter().position(|l| l.trim() == "Calculated try_task_config.json:")?;
    let rest = lines[at + 1..].join("\n");
    serde_json::Deserializer::from_str(&rest).into_iter::<Value>().next()?.ok()
}

/// How what mach would push, `config` (its try_task_config.json), differs
/// from what try push `p` pushes: a sentence for each difference, of the
/// tasks, how many times each runs, and the priority; none when they are
/// the same.
pub fn differences(config: &Value, p: &Push) -> Vec<String> {
    let ttc = config.pointer("/parameters/try_task_config");
    let tasks: Vec<String> = ttc
        .and_then(|t| t.get("tasks"))
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let listed = |these: &[&String]| these.iter().map(|l| format!("`{l}`")).collect::<Vec<_>>().join(", ");
    let mut out = Vec::new();
    let missing: Vec<&String> = p.labels.iter().filter(|l| !tasks.contains(l)).collect();
    if !missing.is_empty() {
        let these = if missing.len() == 1 { "This task you accepted is" } else { "These tasks you accepted are" };
        out.push(format!("{these} not in what mach would push, as the task graph it reads does not have {}: {}.", if missing.len() == 1 { "it" } else { "them" }, listed(&missing)));
    }
    let extra: Vec<&String> = tasks.iter().filter(|t| !p.labels.contains(t)).collect();
    if !extra.is_empty() {
        let these = if extra.len() == 1 { "this task" } else { "these tasks" };
        out.push(format!("What mach would push holds {these} besides: {}.", listed(&extra)));
    }
    let times = |n: Option<u64>| n.map_or("once".to_string(), |n| format!("{n} times"));
    let rebuild = ttc.and_then(|t| t.get("rebuild")).and_then(Value::as_u64);
    let want = p.rebuild.map(u64::from);
    if rebuild != want {
        out.push(format!("What mach would push runs each task {}, not {}.", times(rebuild), times(want)));
    }
    let priority = |p: Option<&str>| p.map_or("at try's own priority".to_string(), |p| format!("at the {p} priority"));
    let got = ttc.and_then(|t| t.get("priority")).and_then(Value::as_str);
    let want = p.lowest.then_some("lowest");
    if got != want {
        out.push(format!("What mach would push goes {}, not {}.", priority(got), priority(want)));
    }
    out
}

/// The question mach asked on its terminal, if it stopped on one: its stdin
/// closed, Python's `input()` reads nothing there and raises EOFError. The
/// question as mach wrote it, up to its choices (`[Y/n]`); empty when it is
/// not found.
pub fn asked(lines: &[String]) -> Option<String> {
    if !lines.iter().any(|l| l.contains("EOFError") || l.contains("EOF when reading a line")) {
        return None;
    }
    Some(lines.iter().rev().find_map(|l| question_in(l)).unwrap_or_default())
}

/// The question in `line`, if it holds one: up to its choices, `… ? [Y/n]`.
fn question_in(line: &str) -> Option<String> {
    let at = line.find("? [")?;
    let end = at + line[at..].find(']')? + 1;
    Some(line[..end].trim().to_string())
}

/// What a try push not made because mach asked `question` says: the
/// question quoted, and what to change for mach not to ask it.
fn asked_failure(question: &str) -> Failure {
    let what = match question.is_empty() {
        true => "mach asked a question on its terminal that stacked cannot answer".to_string(),
        false => format!("mach asked a question that stacked cannot answer, \"{question}\""),
    };
    let advice = if question.contains("@mozilla.com") {
        "mach asks it when Pernosco is asked for (MOZ_USE_PERNOSCO set, or --pernosco) and `ssh -G hg.mozilla.org` names no user of yours: set your hg.mozilla.org user in ~/.ssh/config (`Host hg.mozilla.org`, then `User` and your address), or unset MOZ_USE_PERNOSCO, then ask for the try push again."
    } else if question.contains("fzf") {
        "mach asks it when it finds no fzf: install fzf, or run `./mach try fuzzy` once in a terminal of yours and say yes there, then ask for the try push again."
    } else {
        "Run `./mach try fuzzy` once in a terminal of yours to answer it there, then ask for the try push again."
    };
    Failure {
        why: what.clone(),
        said: format!("The try push was not made: {what}, and stopped, as stacked gives it nothing to read. Nothing was pushed. {advice}"),
    }
}

/// The page mach's login asks you to open, in `line` if it says it:
/// "1. On your computer or mobile device navigate to: URL".
pub fn login_page(line: &str) -> Option<String> {
    let (_, url) = line.split_once("navigate to:")?;
    crate::refs::link(url.trim()).ok()
}

/// The code mach's login asks you to enter, in `line` if it says it: "2.
/// Enter the following code: CODE".
pub fn login_code(line: &str) -> Option<String> {
    let (_, code) = line.split_once("Enter the following code:")?;
    Some(code.trim().to_string()).filter(|c| !c.is_empty() && !c.contains(char::is_whitespace))
}

/// The push's link on Treeherder, in `line` if it holds one: as mach says
/// it once it pushed, ` https://treeherder.mozilla.org/jobs?repo=try&…`.
pub fn pushed_link(line: &str) -> Option<String> {
    let at = line.find("https://treeherder.mozilla.org/")?;
    let url: String = line[at..].chars().take_while(|c| !c.is_whitespace()).collect();
    crate::refs::link(&url).ok()
}

/// Where Lando has the push of link `link`, as its query says it:
/// `landoInstance` and `landoCommitID`.
pub fn lando_of(link: &str) -> Option<Lando> {
    let (_, query) = link.split_once('?')?;
    let param = |name: &str| query.split('&').find_map(|kv| kv.split_once('=').filter(|(k, _)| *k == name).map(|(_, v)| v.to_string()));
    Some(Lando {
        instance: param("landoInstance")?,
        job: param("landoCommitID")?.parse().ok()?,
    })
}

/// A folder of stacked's own beside the database, `stacked-try/ID`, for
/// what one push gives mach: removed once the push is done with it,
/// whatever came of it, and `stacked-try` with it when nothing else is in
/// it.
struct Scratch(PathBuf);

impl Scratch {
    fn new(db_path: &Path, id: &str) -> Scratch {
        Scratch(db_path.with_file_name("stacked-try").join(id))
    }

    /// The presets file mach reads in place of yours and Firefox's, which
    /// `MACH_TRY_PRESET_PATHS` names: the preset of the lowest priority
    /// alone, as JSON, which reads as YAML.
    fn presets(&self) -> std::io::Result<PathBuf> {
        std::fs::create_dir_all(&self.0)?;
        let file = self.0.join("try_presets.yml");
        let preset = serde_json::json!({
            LOWEST: {
                "selector": "fuzzy",
                "description": "stacked's: the lowest priority, as mach pushes past its threshold once told yes",
                "try_config_params": {"try_task_config": {"priority": "lowest"}}
            }
        });
        std::fs::write(&file, format!("{}\n", serde_json::to_string_pretty(&preset).unwrap_or_default()))?;
        Ok(file)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
        if let Some(parent) = self.0.parent() {
            let _ = std::fs::remove_dir(parent);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn the_tasks_are_read_from_the_answer_each_label_once() {
        let a = answer(serde_json::json!({
            "replies": {"c1": "Three tasks."},
            "tasks": [
                {"label": " test-linux2404-64/opt-xpcshell-1 ", "why": "It runs the tests of netwerk."},
                {"label": "build-linux64/debug", "why": {"text": "The build the tests need.", "refs": [{"url": "https://example.com/build"}]}},
                "source-test-mozlint-eslint",
                {"label": "test-linux2404-64/opt-xpcshell-1", "why": "Again."},
                {"label": "two words", "why": "Not a label."},
                {"label": "", "why": "Nothing."},
                {"why": "No label at all."},
                7
            ],
            "rebuild": 3
        }));
        let (tasks, rebuild) = tasks_of(Some(&a));
        let labels: Vec<&str> = tasks.iter().map(|t| t.label.as_str()).collect();
        assert_eq!(labels, ["test-linux2404-64/opt-xpcshell-1", "build-linux64/debug", "source-test-mozlint-eslint"]);
        assert_eq!(tasks[0].why, "It runs the tests of netwerk.", "the first reason kept");
        assert!(tasks[1].why.starts_with("The build the tests need.") && tasks[1].why.contains("```refs\nhttps://example.com/build\n```"), "{}", tasks[1].why);
        assert_eq!((tasks[2].why.as_str(), &tasks[2].answer), ("", &None));
        assert_eq!(rebuild, Some(3));
    }

    #[test]
    fn a_rebuild_is_from_two_to_twenty_and_none_is_none() {
        let of = |v: Value| tasks_of(Some(&answer(serde_json::json!({"tasks": [], "rebuild": v})))).1;
        assert_eq!(of(serde_json::json!(1)), None, "once is no rebuild");
        assert_eq!(of(serde_json::json!(0)), None);
        assert_eq!(of(serde_json::json!(20)), Some(20));
        assert_eq!(of(serde_json::json!(50)), Some(20), "as many as mach takes");
        assert_eq!(of(serde_json::json!("three")), None);
        assert_eq!(tasks_of(None), (vec![], None));
        assert_eq!(scheduled(400, Some(3)), 1200, "as push.py counts them");
        assert_eq!(scheduled(1001, None), 1001);
        assert_eq!([0, 999, 1000, 1240, 1234567].map(thousands), ["0", "999", "1,000", "1,240", "1,234,567"]);
    }

    #[test]
    fn the_prompt_asks_for_what_the_comment_asks_exact_labels_and_why_pushing_nothing() {
        let p = prompt("the stack's patches", "Read the stack.", "You may read the web.", Some(Path::new("/tmp/0123456789ab/full-task-graph.json")));
        for said in [
            "as few or as many as they say",
            "exact label",
            "a chunked test is a task for each chunk",
            "no label is a pattern",
            "Say why each is relevant",
            "Do NOT push anything, nor run `./mach`",
            "Do NOT modify anything",
            "target-tasks.json",
        ] {
            assert!(p.contains(said), "{said:?} in {p}");
        }
        assert!(!p.contains("minimal"), "{p}");
        assert!(!prompt("x", "", "", None).contains("target-tasks.json"), "without curl, no address to read");
        let end = answer_said("End your reply with a JSON object");
        assert!(end.contains("\"tasks\"") && end.contains("\"rebuild\"") && end.contains("\"replies\""), "{end}");
    }

    #[test]
    fn with_curl_each_label_is_checked_against_the_task_graph_downloaded_once() {
        let graph = Path::new("/tmp/stacked-todo/0123456789ab/full-task-graph.json");
        let p = prompt("the stack's patches", "Read the stack.", "You may read the web.", Some(graph));
        // Beside target-tasks.json's address, the same decision task's full
        // task graph, and what it holds.
        let index = "https://firefox-ci-tc.services.mozilla.com/api/index/v1/task/gecko.v2.mozilla-central.latest.taskgraph.decision/artifacts/public";
        for said in [
            format!("a JSON list at {index}/target-tasks.json, and autoland's"),
            format!(
                "The full task graph of mozilla-central's latest push, every task `./mach try fuzzy --all-tasks` can select, \
                 is the JSON object at {index}/full-task-graph.json, whose keys are the tasks' labels"
            ),
            "check each label you select against it before you answer".to_string(),
            // Once, into its scratch file, by its full path, and read there.
            format!("It is large: download it once into `{}`, a scratch file in a folder of your own, the one place you may write", graph.display()),
            format!("`curl -sL --compressed -o {}` with its address", graph.display()),
            format!("read that copy: `jq '[\"LABEL\", …] - keys' {}` prints the labels you list that the graph does not have", graph.display()),
            "If the download fails, say so in your answer, and answer without it.".to_string(),
        ] {
            assert!(p.contains(&said), "{said:?} in {p}");
        }
        assert!(p.find("target-tasks.json") < p.find("full-task-graph.json"), "{p}");
        // A path sh would split, quoted in the commands.
        let p = prompt("x", "", "", Some(Path::new("/tmp/my plans/0123456789ab/full-task-graph.json")));
        assert!(p.contains("`curl -sL --compressed -o '/tmp/my plans/0123456789ab/full-task-graph.json'` with its address"), "{p}");
        assert!(p.contains("`jq '[\"LABEL\", …] - keys' '/tmp/my plans/0123456789ab/full-task-graph.json'`"), "{p}");
        // Without curl, nothing of it.
        let p = prompt("the stack's patches", "Read the stack.", "", None);
        for not in ["full-task-graph.json", "check each label", "download", "place you may write"] {
            assert!(!p.contains(not), "{not:?} in {p}");
        }
    }

    #[test]
    fn what_a_round_asked_again_is_told_and_your_answers_in_the_thread() {
        let tasks = vec![
            Task { label: "a".into(), why: "For a.".into(), answer: Some("Not needed.".into()) },
            Task { label: "b".into(), why: String::new(), answer: None },
        ];
        let said = chosen_said(&tasks, Some(4), &|k| (k == 0).then_some(false));
        assert!(said.contains("- `a` (rejected): For a.\n  The user answered: Not needed."), "{said}");
        assert!(said.contains("- `b` (not marked): no reason given"), "{said}");
        assert!(said.ends_with("Each was to run 4 times."), "{said}");
        assert_eq!(answers_turn(&tasks).as_deref(), Some("On the tasks the agent selected:\n- `a`: Not needed."));
        assert_eq!(answers_turn(&tasks[1..]), None);
    }

    fn push(labels: &[&str], rebuild: Option<u32>, lowest: bool) -> Push {
        Push {
            labels: labels.iter().map(|l| l.to_string()).collect(),
            rebuild,
            lowest,
            patches: 1,
            ..Default::default()
        }
    }

    #[test]
    fn mach_is_run_with_the_tasks_in_one_query_each_an_exact_term_and_said_short() {
        let p = push(&["build-linux64/debug", "test-linux2404-64/opt-xpcshell-1"], Some(3), true);
        let args = mach_args(&p, true);
        assert_eq!(&args[args.len() - 2..], ["-q", "^build-linux64/debug$ | ^test-linux2404-64/opt-xpcshell-1$"], "one argument, fzf's OR between the terms");
        assert_eq!(
            args.join(" "),
            "try fuzzy --no-push --show-chunk-numbers --all-tasks --rebuild 3 --preset stacked-lowest -q ^build-linux64/debug$ | ^test-linux2404-64/opt-xpcshell-1$"
        );
        assert_eq!(mach_args(&push(&["a"], None, false), false).join(" "), "try fuzzy --show-chunk-numbers --all-tasks -q ^a$");
        assert_eq!(
            command_said(&args),
            "./mach try fuzzy --no-push --show-chunk-numbers --all-tasks --rebuild 3 --preset stacked-lowest -q '^build-linux64/debug$ | ^test-linux2404-64/opt-xpcshell-1$'"
        );
        let many = push(&["a", "b", "c", "d"], None, false);
        assert_eq!(command_said(&mach_args(&many, false)), "./mach try fuzzy --show-chunk-numbers --all-tasks -q '^a$ | ^b$ | …' (one query, a term for each of the 4 tasks)");
    }

    #[test]
    fn what_mach_would_push_is_compared_with_what_was_accepted() {
        let config = |tasks: &[&str], rebuild: Option<u64>, priority: Option<&str>| {
            let mut ttc = serde_json::json!({"env": {"TRY_SELECTOR": "fuzzy"}, "tasks": tasks});
            if let Some(n) = rebuild {
                ttc["rebuild"] = n.into();
            }
            if let Some(p) = priority {
                ttc["priority"] = p.into();
            }
            serde_json::json!({"parameters": {"optimize_target_tasks": false, "try_task_config": ttc}, "version": 2})
        };
        let p = push(&["b", "a"], Some(2), true);
        assert!(differences(&config(&["a", "b"], Some(2), Some("lowest")), &p).is_empty(), "the same, sorted");
        assert_eq!(
            differences(&config(&["a", "c", "d"], None, None), &p),
            [
                "This task you accepted is not in what mach would push, as the task graph it reads does not have it: `b`.",
                "What mach would push holds these tasks besides: `c`, `d`.",
                "What mach would push runs each task once, not 2 times.",
                "What mach would push goes at try's own priority, not at the lowest priority.",
            ]
        );
        let q = push(&["a"], None, false);
        assert_eq!(
            differences(&config(&["a"], Some(5), Some("low")), &q),
            ["What mach would push runs each task 5 times, not once.", "What mach would push goes at the low priority, not at try's own priority."]
        );
        // Read from what `--no-push` writes, after its line.
        let lines: Vec<String> = ["Commit message:", "Fuzzy query=^a$", "Calculated try_task_config.json:", "{", "    \"parameters\": {\"try_task_config\": {\"tasks\": [\"a\"]}},", "    \"version\": 2", "}", "trailing words"]
            .map(String::from)
            .to_vec();
        assert_eq!(calculated(&lines), Some(serde_json::json!({"parameters": {"try_task_config": {"tasks": ["a"]}}, "version": 2})));
        assert_eq!(calculated(&lines[..2]), None);
    }

    #[test]
    fn a_question_mach_stopped_on_its_login_and_its_link_are_read_from_what_it_writes() {
        let lines = |l: &[&str]| l.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let asked_it = lines(&["Pernosco requires a Mozilla e-mail address to view its reports.", "Do you have an @mozilla.com address? [Y/n]: ", "EOFError: EOF when reading a line"]);
        assert_eq!(asked(&asked_it).as_deref(), Some("Do you have an @mozilla.com address? [Y/n]"));
        assert_eq!(asked(&lines(&["Would you like to proceed anyway? [Y/n]: Error running mach:", "EOFError"])).as_deref(), Some("Would you like to proceed anyway? [Y/n]"));
        assert_eq!(asked(&lines(&["EOFError: EOF when reading a line"])).as_deref(), Some(""), "a question not found");
        assert_eq!(asked(&lines(&["Do you have an @mozilla.com address? [Y/n]: "])), None, "not stopped on it");
        assert!(asked_failure("Do you have an @mozilla.com address? [Y/n]").said.contains("~/.ssh/config"));
        assert!(asked_failure("Could not detect fzf, install it now? [y/n]").said.contains("install fzf"));
        assert_eq!(
            login_page("1. On your computer or mobile device navigate to: https://auth.example.com/activate?user_code=AB-CD").as_deref(),
            Some("https://auth.example.com/activate?user_code=AB-CD")
        );
        assert_eq!(login_code("2. Enter the following code: AB-CD").as_deref(), Some("AB-CD"));
        assert_eq!(login_page("navigate to: somewhere"), None);
        let link = "https://treeherder.mozilla.org/jobs?repo=try&landoInstance=lando-prod-new&landoCommitID=4242";
        assert_eq!(pushed_link(&format!(" {link}")).as_deref(), Some(link));
        assert_eq!(pushed_link("remote:   https://treeherder.mozilla.org/#/jobs?repo=try&revision=abc").as_deref(), Some("https://treeherder.mozilla.org/#/jobs?repo=try&revision=abc"));
        assert_eq!(lando_of(link), Some(Lando { instance: "lando-prod-new".into(), job: 4242 }));
        assert_eq!(lando_of("https://treeherder.mozilla.org/#/jobs?repo=try&revision=abc"), None);
        assert_eq!(plain("\x1b[33mwarning\x1b[0m: it\r\n"), "warning: it");
    }

    #[test]
    fn mach_runs_only_a_stand_in_in_a_process_that_keeps_nothing() {
        let at = Path::new("/w");
        assert!(program_of(at, None, true).unwrap_err().contains("only a stand-in for it is"));
        assert_eq!(program_of(at, Some("/fake".into()), true), Ok(PathBuf::from("/fake")));
        assert_eq!(program_of(at, None, false), Ok(PathBuf::from("/w/mach")));
        // This process is one: no home of its own was set up.
        assert!(program(at).is_err() || std::env::var_os("STACKED_MACH").is_some());
    }

    /// A Treeherder job as `jobs` reads one: by its properties' names.
    fn row(label: &str, state: &str, result: &str, task: &str, retry: u64) -> Map<String, Value> {
        serde_json::json!({"job_type_name": label, "state": state, "result": result, "task_id": task, "retry_id": retry}).as_object().unwrap().clone()
    }

    #[test]
    fn a_push_s_tasks_are_counted_as_treeherder_shows_them_each_by_its_last_run() {
        let p = push(&["a", "b", "c"], Some(2), false);
        let rows = [
            row("Gecko Decision Task", "completed", "success", "d", 0),
            // a: twice, as it runs twice; its first run retried, then passed.
            row("a", "completed", "retry", "a1", 0),
            row("a", "completed", "success", "a1", 1),
            row("a", "completed", "testfailed", "a2", 0),
            // b: once of its two runs, still running.
            row("b", "running", "unknown", "b1", 0),
            // c: never; a build the push ran for them, busted.
            row("build-linux64/debug", "completed", "busted", "x", 0),
        ];
        let seen = seen_of(&rows, &p, "rev");
        assert_eq!((seen.total, seen.done, seen.going), (6, 2, true));
        assert_eq!(seen.results, [("success".to_string(), 1), ("testfailed".to_string(), 1)].into_iter().collect());
        assert_eq!(seen.failed, [Shown { label: "a".into(), result: "testfailed".into(), link: "https://treeherder.mozilla.org/jobs?repo=try&revision=rev&selectedTaskRun=a2.0".into() }]);
        assert_eq!(seen.besides.iter().map(|s| (s.label.as_str(), s.result.as_str())).collect::<Vec<_>>(), [("build-linux64/debug", "busted")]);
        assert_eq!(seen.unseen, [("b".to_string(), 1), ("c".to_string(), 2)]);
        assert_eq!(seen.said(), "2 of 6 tasks done, 1 failed");
        // Nothing shown yet: still to come.
        assert!(seen_of(&[], &p, "rev").going);
    }

    #[test]
    fn a_push_done_is_summed_up_its_failures_and_its_tasks_never_seen_with_their_links() {
        let mut p = push(&["a", "b", "c"], None, false);
        p.link = Some("https://treeherder.mozilla.org/jobs?repo=try&landoInstance=i&landoCommitID=1".into());
        let rows = [row("a", "completed", "success", "a1", 0), row("b", "completed", "exception", "b1", 0), row("w", "completed", "busted", "w1", 0)];
        let seen = seen_of(&rows, &p, "r");
        assert_eq!(results_said(&seen), "1 exception, 1 success, 1 not seen on Treeherder");
        let said = summary_said(&p, &seen, p.link.as_deref().unwrap(), 3);
        assert_eq!(
            said,
            "The try push is done. Treeherder shows it: @r3.\n\nOf its 3 tasks, Treeherder shows 1 as exception and 1 as success; 1 was not seen on Treeherder.\n\nThese failed:\n\n- `b` (exception): @r4\n\nThe push ran these for them too, and they failed:\n\n- `w` (busted): @r5\n\nNot seen on Treeherder, which shows no task that was never scheduled: `c`.\n\n```refs\nr3 https://treeherder.mozilla.org/jobs?repo=try&landoInstance=i&landoCommitID=1\nr4 https://treeherder.mozilla.org/jobs?repo=try&revision=r&selectedTaskRun=b1.0\nr5 https://treeherder.mozilla.org/jobs?repo=try&revision=r&selectedTaskRun=w1.0\n```"
        );
        // Each run twice: said so, and the runs not seen counted.
        let p = push(&["a"], Some(2), false);
        let seen = seen_of(&[row("a", "completed", "success", "a1", 0)], &p, "r");
        assert!(summary_said(&p, &seen, "https://treeherder.mozilla.org/x", 1).contains("Of its 2 tasks, the 1 you accepted each run 2 times, Treeherder shows 1 as success; 1 was not seen on Treeherder.\n\nNot seen on Treeherder, which shows no task that was never scheduled: `a` (1 of its 2 runs)."));
        assert_eq!(
            not_pushed_said("FAILED: it does not apply", Some("https://lando.moz.tools/landings/7"), 2),
            "The try push did not get to try: Lando says FAILED: it does not apply. None of its tasks ran. Lando shows it: @r2.\n\n```refs\nr2 https://lando.moz.tools/landings/7\n```"
        );
    }

    #[test]
    fn where_lando_answers_as_the_checkout_s_lando_ini_says_and_a_push_s_progress_said() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join(".lando.ini"),
            "[autoformat]\nenabled = True\n\n[lando-prod]\ninstance_id = lando-prod\napi_domain = api.lando.example\n\n[lando-prod-new]\ninstance_id = lando-prod-2025\napi_domain = lando.example\n# a comment\n[other]\napi_domain = other.example\n",
        )
        .unwrap();
        let dir = Some(tmp.path());
        assert_eq!(lando_api(dir, "lando-prod-2025").as_deref(), Some("lando.example"));
        assert_eq!(lando_api(dir, "lando-prod").as_deref(), Some("api.lando.example"));
        assert_eq!(lando_api(dir, "other").as_deref(), Some("other.example"), "named so, with no instance_id");
        assert_eq!(lando_api(dir, "nowhere"), None);
        assert_eq!(lando_api(None, "lando-prod-2025").as_deref(), Some("lando.moz.tools"), "the one stacked knows");
        let mut p = push(&["a"], None, false);
        p.step = Step::Pushed;
        assert_eq!(state_said(&p), "try push: pushed, Lando not looked at yet");
        p.lando_status = Some("IN_PROGRESS".into());
        assert_eq!(state_said(&p), "try push: Lando has it: in progress");
        p.revision = Some("r".into());
        assert_eq!(progress_said(&p), "pushed, its tasks not on Treeherder yet");
        p.seen = Some(Seen { total: 30, done: 12, failed: vec![Shown::default()], ..Default::default() });
        assert_eq!(progress_said(&p), "12 of 30 tasks done, 1 failed");
    }

    #[test]
    fn lando_and_treeherder_are_not_reached_from_a_process_that_keeps_nothing() {
        // This process is one, and sets no stand-in.
        assert_eq!(
            get("https://lando.moz.tools/landing_jobs/1/"),
            Err("Lando and Treeherder are not reached from a process that keeps nothing, as the tests' own".to_string())
        );
    }

    /// A repository in `dir`, its first commit made, its id.
    fn repo(dir: &Path) -> String {
        let git = |args: &[&str]| {
            let out = Command::new("git")
                .args(args)
                .current_dir(dir)
                .env("GIT_AUTHOR_NAME", "A")
                .env("GIT_AUTHOR_EMAIL", "a@example.com")
                .env("GIT_COMMITTER_NAME", "C")
                .env("GIT_COMMITTER_EMAIL", "c@example.com")
                .output()
                .unwrap();
            assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };
        git(&["init", "-q", "-b", "main"]);
        git(&["-c", "commit.gpgsign=false", "commit", "-q", "--allow-empty", "-m", "base"]);
        git(&["rev-parse", "HEAD"])
    }

    #[test]
    fn the_remotes_mach_takes_as_firefox_s_and_a_base_none_of_them_has() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let base = repo(dir);
        let git = |args: &[&str]| crate::git::git(dir, args).unwrap();
        assert_eq!(official_remotes(dir).unwrap(), Vec::<String>::new());
        assert!(base_problem(dir, &base).unwrap().starts_with("the repository has no remote"));
        git(&["remote", "add", "mine", "https://example.com/firefox"]);
        git(&["remote", "add", "origin", "https://github.com/mozilla-firefox/firefox"]);
        git(&["remote", "add", "ssh", "git@github.com:mozilla-firefox/firefox.git"]);
        git(&["remote", "add", "hg", "hg::https://hg.mozilla.org/mozilla-central"]);
        // Not a git-cinnabar clone: hg.mozilla.org is no remote of Firefox's.
        assert_eq!(official_remotes(dir).unwrap(), ["origin", "ssh"]);
        assert_eq!(remote_args(&official_remotes(dir).unwrap()), ["--remotes=origin", "--remotes=ssh"]);
        assert_eq!(remote_args(&[]), ["--remotes"]);
        let short = &base[..12];
        assert_eq!(
            base_problem(dir, &base).as_deref(),
            Some(format!("the stack's base, commit {short}, is not on any of origin, ssh, the remotes mach takes as Firefox's: mach would push 1 commit of the base's besides the stack's patches. Fetch that remote (`git fetch`), or put the stack on a commit that it has").as_str())
        );
        // Another remote's having it is not enough.
        git(&["update-ref", "refs/remotes/mine/main", &base]);
        assert!(base_problem(dir, &base).is_some());
        git(&["update-ref", "refs/remotes/ssh/main", &base]);
        assert_eq!(base_problem(dir, &base), None);
        // A git-cinnabar clone: hg.mozilla.org's remotes too, but try.
        git(&["update-ref", "refs/cinnabar/metadata", &base]);
        git(&["remote", "add", "try", "hg::ssh://hg.mozilla.org/try"]);
        assert_eq!(official_remotes(dir).unwrap(), ["hg", "origin", "ssh"], "as git lists them");
        // The commits mach would push, the patches alone or more.
        let names = official_remotes(dir).unwrap();
        assert_eq!(commits_problem(dir, &base, 0, &names), None);
        assert_eq!(
            commits_problem(dir, &base, 1, &names).as_deref(),
            Some("mach would push 0 of the stack's 1 patch: the others are on a remote that mach takes as Firefox's already")
        );
    }
}
