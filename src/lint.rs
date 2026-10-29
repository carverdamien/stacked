//! Linting a stack: the editor's `Lint the stack (CMD)`, on the stack's
//! header, runs the linter of the stack's checkout on every patch, patch by
//! patch from the bottom, away from the screen; what it fixes lands for your
//! review as one change, as an edit by hand does. stacked runs the linter
//! itself, never the agent: a lint job is a run of its own (`Run::lint`),
//! run by `stacked lint-job RUN`, a process of its own, in a session of its
//! own (no terminal to ask anything on), at a low priority, its stdin
//! closed, in a worktree of the stack's own, `.stacked/NAME/lint` (under
//! `agent.worktrees` when that is set), kept for the next job: never in your
//! checkout.
//!
//! The linter is a checkout's own setting (`stacked setup lint`, its
//! `.stacked/local.toml`'s `[lint] command`): a program and its arguments,
//! split as a shell splits words and run with no shell, the files to lint
//! after them. Unset, a checkout whose top has `./mach` lints with `./mach
//! lint --fix`, nothing written; set empty, lint is off.
//!
//! Patch by patch: for each patch, from the bottom, the worktree holds the
//! stack up to it, with the fixes kept so far, and the linter is run on the
//! files of the patch that are there. What it changes is folded into the
//! patch, its new version, so that each fix lands in the earliest patch it
//! shows in; but only once every patch above, as stored, is seen to still
//! apply on it, in turn, in an index of the check's own (the repository only
//! read). At the first patch whose fixes would keep one above from applying,
//! or that add or delete a file, change one that is not text or its mode, or
//! touch a file it renames, none of its fixes is kept and the phase stops:
//! with that patch and those above as stored, the linter is run once at the
//! top, on their files, and what it changes goes in a patch of its own at
//! the end, `Lint fixes to fold into the stack`, with a comment on it, yours,
//! ready to send but not sent, asking the agent to fold them in.
//!
//! The linter's exit says whether it reported something it did not fix:
//! what it wrote then is kept with the work, for the review to list. With
//! nothing fixed and nothing reported, nothing lands; with nothing fixed and
//! something reported, that alone lands, to read. A lint job takes its turn
//! as an edit of the agent's does (`ask::claim`): while it waits or runs, the
//! stack is not edited by hand, and the commit messages' sync makes no new
//! version of a patch; an edit asked meanwhile waits for it, then goes on
//! from what it made, chained, as edits do. One lint job runs at a time, of
//! every stack of the database.

use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

use crate::db::{Db, Error, Note, Patch, Record, Result, Run, RunState, err, now};
use crate::held::{Change, Held};
use crate::mbox::Draft;

/// What a lint job's run is, as `Run::what` says it.
pub const WHAT: &str = crate::db::LINT;

/// The worktrees lint jobs run in, of the stack's own: `lint`, `lint-2`…
/// under `.stacked/NAME`, beside the agents'.
pub const KIND: &str = "lint";

/// Firefox's linter, the command of a checkout whose top has `./mach` and
/// whose own settings set none.
pub const MACH: &str = "./mach lint --fix";

/// Why a stack is not linted when its checkout has no linter.
pub const NO_LINTER: &str = "No linter is set for this checkout: `stacked setup lint COMMAND` sets one.";

/// Why a stack is not linted when its checkout's linter is turned off.
pub const OFF: &str = "Lint is turned off for this checkout: `stacked setup lint COMMAND` sets a linter again.";

/// Why a stack with no patch is not linted.
pub const EMPTY: &str = "The stack has no patch to lint.";

/// The title of the patch that holds, at the end of the stack, the fixes
/// that could not be folded into their patches.
pub const ON_TOP: &str = "Lint fixes to fold into the stack";

/// What the comment on that patch asks, ready to send.
pub const FOLD: &str = "Fold these lint fixes into the patches they belong to, keeping every patch applying, then drop this patch.";

/// What a lint job that fixed nothing and reported nothing came to.
pub const NOTHING: &str = "the linter found nothing to fix, and reported nothing";

/// How many of the lines a linter wrote are kept with the work, for the
/// review to show: its stream has them all.
const KEPT_LINES: usize = 200;

// -- the setting --------------------------------------------------------------- //

/// The linter of a checkout, as its own settings have it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Linter {
    /// The command its local file sets.
    Set(String),
    /// `./mach lint --fix`: `./mach` is at its top, and its local file sets
    /// no linter.
    Mach,
    /// Turned off by its local file: `command = ""`.
    Off,
    /// None: no `./mach`, and no setting.
    None,
}

impl Linter {
    /// The linter of the checkout `dir` is in: its local file's, else
    /// `./mach lint --fix` where `./mach` is at its top. Nothing is written.
    pub fn of(dir: &Path) -> Linter {
        let (cfg, _) = crate::config::of(dir);
        match cfg.lint.command {
            Some(c) if c.is_empty() => Linter::Off,
            Some(c) => Linter::Set(c),
            None if has_mach(dir) => Linter::Mach,
            None => Linter::None,
        }
    }

    /// The command it runs, if there is one.
    pub fn command(&self) -> Option<&str> {
        match self {
            Linter::Set(c) => Some(c),
            Linter::Mach => Some(MACH),
            Linter::Off | Linter::None => None,
        }
    }

    /// Why nothing is linted with it, if nothing is.
    pub fn why_not(&self) -> Option<&'static str> {
        match self {
            Linter::Off => Some(OFF),
            Linter::None => Some(NO_LINTER),
            Linter::Set(_) | Linter::Mach => None,
        }
    }
}

/// Whether the checkout `dir` is in has `./mach` at its top, as Firefox's
/// does.
fn has_mach(dir: &Path) -> bool {
    crate::git::toplevel(dir).is_ok_and(|top| Path::new(&top).join("mach").is_file())
}

/// `command` split into words as a shell splits them: at white space, but
/// within single or double quotes, a backslash keeping the character after
/// it. Nothing else of a shell's is read: no variable, no pipe, no glob.
pub fn words(command: &str) -> std::result::Result<Vec<String>, String> {
    let unclosed = |q: char| format!("{command:?} has a {q} that nothing closes");
    let mut out = Vec::new();
    let mut word = String::new();
    // A word started, empty or not: `''` is one.
    let mut started = false;
    let mut chars = command.chars();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                started = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(c) => word.push(c),
                        None => return Err(unclosed('\'')),
                    }
                }
            }
            '"' => {
                started = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some(c @ ('"' | '\\' | '$' | '`')) => word.push(c),
                            Some('\n') => {}
                            Some(c) => {
                                word.push('\\');
                                word.push(c);
                            }
                            None => return Err(unclosed('"')),
                        },
                        Some(c) => word.push(c),
                        None => return Err(unclosed('"')),
                    }
                }
            }
            '\\' => {
                started = true;
                match chars.next() {
                    Some('\n') | None => {}
                    Some(c) => word.push(c),
                }
            }
            c if c.is_whitespace() => {
                if started {
                    out.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            c => {
                started = true;
                word.push(c);
            }
        }
    }
    if started {
        out.push(word);
    }
    Ok(out)
}

/// `words` written as one command that `words` splits back into them: each
/// as it is where a shell would read it so, else between single quotes.
pub fn quoted(words: &[String]) -> String {
    let one = |w: &str| match !w.is_empty() && w.chars().all(|c| c.is_ascii_alphanumeric() || "-_./=:,+@%^".contains(c)) {
        true => w.to_string(),
        false => format!("'{}'", w.replace('\'', "'\\''")),
    };
    words.iter().map(|w| one(w)).collect::<Vec<_>>().join(" ")
}

// -- the job ------------------------------------------------------------------- //

/// A lint job, as its run keeps it (`Run::lint`), each step written as it
/// goes.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Job {
    /// The command it runs, as the checkout's settings gave it when it was
    /// asked.
    pub command: String,
    #[serde(default)]
    pub step: Step,
    /// The stack it lints, by its rev, once it runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rev: Option<usize>,
    /// The worktree it runs in, once taken.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<String>,
    /// Where it is, as it runs: 0 its worktree being made; the patch it
    /// lints, from 1; past the last, the top of the stack. And how many
    /// patches the stack has.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub at: usize,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub of: usize,
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

/// Where a lint job is: waiting its turn, or running.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Step {
    #[default]
    Waiting,
    Running,
}

impl Job {
    /// Where it is, said: `waits its turn`, `lints patch 2 of 5`.
    pub fn said(&self) -> String {
        match (self.step, self.at) {
            (Step::Waiting, _) => "waits its turn to lint the stack".into(),
            (Step::Running, 0) => "makes its worktree, to lint the stack".into(),
            (Step::Running, at) if at <= self.of => format!("lints patch {at} of {}", self.of),
            (Step::Running, _) => "lints the top of the stack".into(),
        }
    }
}

/// Run `r`'s lint job, if it is one still under way.
fn job_of(r: &Run) -> Option<&Job> {
    r.lint.as_deref().filter(|_| r.state == RunState::Started)
}

/// The lint job of stack `name` under way, waiting or running, if one is.
pub fn pending<'a>(db: &'a Db, name: &str) -> Option<&'a Run> {
    db.live_runs().into_iter().find(|r| r.stack == name && job_of(r).is_some())
}

/// Whether a lint job runs, of any stack of the database.
pub fn running(db: &Db) -> bool {
    db.live_runs().into_iter().any(|r| job_of(r).is_some_and(|j| j.step == Step::Running))
}

/// The lint jobs waiting their turn, the first asked first: each its run,
/// and its stack.
pub fn waiting(db: &Db) -> Vec<(String, String)> {
    db.live_runs()
        .into_iter()
        .filter(|r| job_of(r).is_some_and(|j| j.step == Step::Waiting))
        .map(|r| (r.id.clone(), r.stack.clone()))
        .collect()
}

/// The linter of stack `name`'s checkout, as `Linter::of` reads it.
pub fn linter_of(db: &Db, name: &str) -> Linter {
    match db.stack(name).ok().and_then(|s| s.repo.as_deref()) {
        Some(repo) => Linter::of(Path::new(repo)),
        None => Linter::None,
    }
}

/// Stack `name` linted: a lint job queued, to take its turn as an edit of
/// the agent's does (`ask::launch` starts it). Not on a stack that cannot
/// change, nor while it may not be edited by hand, nor without a linter.
/// What is said of it.
pub fn ask(db_path: &Path, name: &str) -> Result<String> {
    // Whether it applies, checked with no lock held (it takes a moment on a
    // big repository): under it, the check is remembered.
    if let Ok(db) = Db::load(db_path)
        && let Ok(s) = db.stack(name)
    {
        let _ = db.verify(s);
    }
    Db::edit(db_path, |db| {
        let s = db.stack(name)?.clone();
        db.writable(name)?;
        crate::ops::by_hand(db, name)?;
        if s.patches.is_empty() {
            return err(EMPTY);
        }
        db.verify(&s).map_err(|e| Error(format!("the stack is not linted: {}", e.0.trim_end_matches("; nothing changed"))))?;
        let linter = linter_of(db, name);
        let Some(command) = linter.command().map(String::from) else {
            return err(linter.why_not().unwrap_or(NO_LINTER));
        };
        if words(&command).map_err(Error)?.is_empty() {
            return err(OFF);
        }
        db.add_run(Run {
            id: crate::db::unique(),
            what: WHAT.into(),
            stack: name.into(),
            state: RunState::Started,
            request: command.clone(),
            lint: Some(Box::new(Job {
                command: command.clone(),
                ..Default::default()
            })),
            ..Default::default()
        })?;
        Ok(format!(
            "the linter is to lint the stack, {command}, patch by patch, away from the screen: what it fixes lands for your review, and the banner says where it is"
        ))
    })
}

/// Under the lock, from `ask::claim`: lint job `id`'s turn come, it is
/// running from now on, of the stack as it is now, started by this process
/// until its own says its pid.
pub(crate) fn claim(db: &mut Db, id: &str) -> Result<()> {
    let r = db.run(id).cloned().ok_or_else(|| Error(format!("no run {id}")))?;
    let mut j = r.lint.clone().ok_or_else(|| Error(format!("run {id} is not a lint job")))?;
    let Some((rev, n)) = db.head(&r.stack).map(|(rev, s)| (rev, s.patches.len())) else {
        return db
            .add_run(Run {
                state: RunState::Failed,
                summary: format!("the stack {} is gone: nothing was linted", r.stack),
                ..r
            })
            .map(|_| ());
    };
    (j.step, j.rev, j.of, j.at) = (Step::Running, Some(rev), n, 0);
    db.add_run(Run {
        lint: Some(j),
        starter: Some(std::process::id()),
        ..r
    })
    .map(|_| ())
}

/// The program the lint jobs this process starts run in place of the
/// linter, when one is set: the tests' stand-in, passed on as
/// `$STACKED_LINT`.
static STAND_IN: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

/// Runs `program` in place of the linter from now on, in the lint jobs this
/// process starts, its arguments as the command gives them: the tests set
/// their stand-in here, as they share their process.
pub fn stand_in(program: &Path) {
    *STAND_IN.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(program.to_path_buf());
}

/// Starts lint job `id`, claimed, away from here: `stacked lint-job ID`, a
/// process of its own, in a session of its own, so that neither the linter
/// nor what it runs has a terminal to ask anything on, at a low priority;
/// told to keep what is yours where this one does, the agents' rebase plans
/// too, and the tests' stand-ins, for the linter and for claude, if they set
/// them (what the job starts once it lands runs with them).
pub fn start(db_path: &Path, id: &str) -> Result<()> {
    let exe = crate::ask::stacked_exe()?;
    let db_abs = db_path.canonicalize().map_err(|e| Error(format!("{}: {e}", db_path.display())))?;
    let mut c = Command::new(&exe);
    c.arg("-f").arg(&db_abs).arg("lint-job").arg(id);
    c.current_dir(db_abs.parent().unwrap_or(Path::new("/")));
    c.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    crate::home::get().pass_on(&mut c);
    c.env("STACKED_TODO", crate::ask::plans(&db_abs));
    if let Some(p) = STAND_IN.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone() {
        c.env("STACKED_LINT", p);
    }
    if let Some(claude) = crate::ask::claude_stand_in() {
        c.env("STACKED_CLAUDE", claude);
    }
    // Between fork and exec, the child alone calls setsid and nice, which
    // allocate nothing and touch no lock.
    unsafe {
        std::os::unix::process::CommandExt::pre_exec(&mut c, || {
            libc::setsid();
            libc::nice(10);
            Ok(())
        });
    }
    let child = c.spawn().map_err(|e| Error(format!("{}: {e}", exe.display())))?;
    let pid = child.id();
    crate::ask::keep_started(child);
    // Its pid written, for an orphan to be found: unless it wrote it itself,
    // or is over.
    Db::edit(db_path, |db| {
        if let Some(r) = db.run(id).filter(|r| r.state == RunState::Started && r.pid.is_none() && job_of(r).is_some()).cloned() {
            db.add_run(Run { pid: Some(pid), ..r })?;
        }
        Ok(())
    })
}

/// Ends lint job `id`, which could not be started, with why.
pub fn not_started(db_path: &Path, id: &str, why: &str) {
    let _ = end(db_path, id, RunState::Failed, &format!("could not start: {why}"));
}

/// Ends lint job `id` as `state` says, `why` its summary, nothing landed:
/// unless it is over already.
fn end(db_path: &Path, id: &str, state: RunState, why: &str) -> Result<()> {
    Db::edit(db_path, |db| {
        let Some(r) = db.run(id).filter(|r| r.state == RunState::Started).cloned() else { return Ok(()) };
        db.add_run(Run {
            state,
            summary: why.to_string(),
            pid: None,
            starter: None,
            ..r
        })
        .map(|_| ())
    })
}

/// The lint jobs whose process is gone before they landed, as `db` has
/// them, ended as failed: how many.
pub fn orphans(db_path: &Path, db: &Db) -> usize {
    let gone: Vec<String> = db
        .live_runs()
        .into_iter()
        .filter(|r| job_of(r).is_some_and(|j| j.step == Step::Running))
        .filter(|r| match (r.pid, r.starter) {
            (Some(pid), _) => !crate::ask::alive(pid),
            (None, Some(starter)) => !crate::ask::alive(starter),
            (None, None) => now() - r.time > 120,
        })
        .map(|r| r.id.clone())
        .collect();
    let said = "the process linting the stack is gone: nothing landed";
    gone.iter().filter(|id| end(db_path, id, RunState::Failed, said).is_ok()).count()
}

/// Runs lint job `id`, in the process `start` started: the stack linted
/// patch by patch in its worktree, what came of it landed; then what waited
/// for it goes.
pub fn run(db_path: &Path, id: &str) -> Result<()> {
    // Its pid, for an orphan to be found; nothing done if it is over
    // already.
    let started = Db::edit(db_path, |db| {
        let r = db.run(id).cloned().ok_or_else(|| Error(format!("no run {id}")))?;
        if !job_of(&r).is_some_and(|j| j.step == Step::Running) {
            return Ok(None);
        }
        db.add_run(Run {
            pid: Some(std::process::id()),
            ..r.clone()
        })?;
        Ok(Some(r))
    })?;
    let Some(r) = started else { return Ok(()) };
    let done = linting(db_path, &r);
    land(db_path, id, done)?;
    crate::ask::launch(db_path)?;
    Ok(())
}

/// The work of `run`: the stack lint job `r` lints, linted in its
/// worktree, the linter's lines in its stream as they come; what came of
/// it, checked to apply; or why it could not be.
fn linting(db_path: &Path, r: &Run) -> std::result::Result<Linted, String> {
    let id = r.id.as_str();
    let job = r.lint.as_deref().cloned().ok_or("the run is not a lint job")?;
    let db = Db::load(db_path).map_err(|e| e.0)?;
    let rev = job.rev.ok_or("the lint job has no stack to lint")?;
    let s = db.node(rev).map_err(|e| e.0)?.clone();
    let (repo, base) = crate::ops::base_of(&s).map_err(|e| e.0)?;
    let patches: Vec<Patch> = s.patches.iter().map(|p| db.patch(p).clone()).collect();
    let words = words(&job.command)?;
    let (cfg, _) = crate::config::of(&repo);
    let dir = crate::ask::agents_dir(&repo, &s.name, cfg.agent.worktrees.as_deref()).map_err(|e| e.0)?;
    let at = take_worktree(db_path, id, &dir).map_err(|e| e.0)?;
    let (program, args) = program(&words, &at)?;
    let step = |text: String| record(db_path, id, STEP, &text);
    // Its own, kept from the job before: what that left thrown away.
    if at.join(".git").exists() {
        let _ = crate::git::scrub(&at);
    }
    crate::git::ready(&repo, &at, &base, "", 0, &step).map_err(|e| e.0)?;
    let mut linter = |at: &Path, files: &[String], place: usize| -> std::result::Result<Ran, String> {
        let _ = update(db_path, id, |j| j.at = place + 1);
        step(format!("{} {}", job.command, quoted(files)));
        run_linter(db_path, id, &program, &args, files, at)
    };
    let said = |text: &str| say(db_path, id, text);
    let linted = lint_stack(&repo, &base, &patches, &at, &job.command, &mut linter, &said)?;
    // The stack it makes applies, as any stack must.
    let all: Vec<&Patch> = linted.patches.iter().collect();
    crate::check::stack(&s, &all).map_err(|e| format!("the stack with the linter's fixes does not apply: {}", e.0.trim_end_matches("; nothing changed")))?;
    Ok(linted)
}

/// What lint job `id` came to, `done`, landed, unless the job is over
/// already: its fixes held for your review, as one change of the stack, with
/// a comment on the patch on top if there is one; or what it reported alone,
/// to read; or nothing, said in its run's summary; or why it failed.
fn land(db_path: &Path, id: &str, done: std::result::Result<Linted, String>) -> Result<()> {
    Db::edit(db_path, |db| {
        let Some(r) = db.run(id).filter(|r| r.state == RunState::Started).cloned() else { return Ok(()) };
        let (state, summary, held) = match landed(db, &r, done) {
            Ok((summary, held)) => (RunState::Done, summary, held),
            Err(why) => (RunState::Failed, why, None),
        };
        db.add_run(Run {
            state,
            summary,
            held,
            pid: None,
            starter: None,
            ..r
        })
        .map(|_| ())
    })
}

/// Under the lock: what lint job `r` landed with `done`, its summary and
/// the work held, if any; or why nothing landed. The stack must be as the
/// job took it: it is not edited meanwhile, but if it is, nothing lands.
fn landed(db: &mut Db, r: &Run, done: std::result::Result<Linted, String>) -> std::result::Result<(String, Option<Held>), String> {
    let l = done?;
    let job = r.lint.as_deref().ok_or("the run is not a lint job")?;
    let (head, s) = db.head(&r.stack).map(|(rev, s)| (rev, s.clone())).ok_or_else(|| format!("the stack {} is gone: nothing landed", r.stack))?;
    if job.rev != Some(head) {
        return Err("the stack changed while the linter ran: nothing landed, and the stack can be linted again".into());
    }
    let mut ids = Vec::new();
    for p in &l.patches {
        ids.push(db.add_patch(p.clone()).map_err(|e| e.0)?);
    }
    if ids != s.patches {
        let mut opened = Vec::new();
        if let Some(top) = l.patches.last().filter(|_| l.report.on_top) {
            opened.push(Note {
                change: top.change.clone(),
                id: format!("p{}", crate::db::unique()),
                line: crate::view::title_key(top),
                text: crate::thread::yours(FOLD),
                file: crate::view::MSG_PATH.into(),
                preview: top.title().chars().take(80).collect(),
                time: now(),
                ..Default::default()
            });
        }
        let op = l.report.said();
        let held = Held {
            at: now(),
            stack: Some(Change {
                patches: ids,
                base: s.base.clone(),
                op: op.clone(),
                of: l.report.of(),
                from: Some(head),
            }),
            opened,
            edits: vec![op.clone()],
            lint: Some(l.report),
            ..Default::default()
        };
        return Ok((format!("landed: {op}"), Some(held)));
    }
    if !l.report.errors.is_empty() {
        let summary = format!("the linter fixed nothing, and reported what it did not fix: {}", l.report.errors_said());
        let held = Held {
            at: now(),
            lint: Some(l.report),
            ..Default::default()
        };
        return Ok((summary, Some(held)));
    }
    Ok((NOTHING.into(), None))
}

/// Lint job `id`'s job changed by `change`, under the lock: unless it is
/// over.
fn update(db_path: &Path, id: &str, change: impl FnOnce(&mut Job)) -> Result<()> {
    Db::edit(db_path, |db| {
        let Some(r) = db.run(id).filter(|r| job_of(r).is_some()).cloned() else { return Ok(()) };
        let Some(mut j) = r.lint.clone() else { return Ok(()) };
        change(&mut j);
        db.add_run(Run { lint: Some(j), ..r }).map(|_| ())
    })
}

/// The type of the events that say what a lint job runs, as an ask's
/// starting commands are: `$ command` in its stream.
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

/// The first free worktree of `dir` for lint jobs (`lint`, `lint-2`…),
/// taken for lint job `id`: no other running there. Written as its
/// worktree, so that no other job takes it.
fn take_worktree(db_path: &Path, id: &str, dir: &Path) -> Result<PathBuf> {
    let mut k = 1;
    loop {
        let at = crate::ask::slot(dir, KIND, k);
        k += 1;
        let path = at.to_string_lossy().into_owned();
        let taken = Db::edit(db_path, |db| {
            let busy = db
                .live_runs()
                .iter()
                .any(|r| r.id != id && job_of(r).is_some_and(|j| j.step == Step::Running && j.worktree.as_deref() == Some(path.as_str())));
            if busy {
                return Ok(false);
            }
            let r = db.run(id).cloned().ok_or_else(|| Error(format!("no run {id}")))?;
            let Some(mut j) = r.lint.clone() else { return err(format!("run {id} is not a lint job")) };
            j.worktree = Some(path.clone());
            db.add_run(Run { lint: Some(j), ..r })?;
            Ok(true)
        })?;
        if taken {
            return Ok(at);
        }
    }
}

/// The program a lint job runs in worktree `at` for the command `words`,
/// and its arguments: the one `$STACKED_LINT` names, as the tests' stand-in
/// is passed on, given the command's arguments; else the command's program,
/// a path in the worktree when it names one (`./mach`), else as `PATH`
/// finds it. Never the command's own from a process that keeps nothing, as
/// the tests' own: there, only a stand-in runs.
fn program(words: &[String], at: &Path) -> std::result::Result<(PathBuf, Vec<String>), String> {
    let given = std::env::var_os("STACKED_LINT").filter(|p| !p.is_empty()).map(PathBuf::from);
    program_of(words, at, given, crate::home::get().dir == Path::new(crate::home::NOWHERE))
}

/// As `program`, the stand-in `given` if any, and whether this process
/// keeps nothing (`nowhere`) said.
fn program_of(words: &[String], at: &Path, given: Option<PathBuf>, nowhere: bool) -> std::result::Result<(PathBuf, Vec<String>), String> {
    let (first, rest) = words.split_first().ok_or("the lint command is empty")?;
    match given {
        Some(p) => Ok((p, rest.to_vec())),
        None if nowhere => Err("a linter is not run from a process that keeps nothing, as the tests' own: only a stand-in for it is".into()),
        None if first.contains('/') => Ok((if Path::new(first).is_absolute() { PathBuf::from(first) } else { at.join(first) }, rest.to_vec())),
        None => Ok((PathBuf::from(first), rest.to_vec())),
    }
}

/// What a run of the linter came to: its exit code (none when it was
/// killed), and each line it wrote, out or error.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Ran {
    pub code: Option<i32>,
    pub lines: Vec<String>,
}

/// Runs `program` as the linter in worktree `at`, with `args`, then
/// `files`: its stdin closed, so that it reads nothing; its output not held
/// back (Python's, by `PYTHONUNBUFFERED`, as mach's). Each line it writes is
/// in run `id`'s stream as it comes.
fn run_linter(db_path: &Path, id: &str, program: &Path, args: &[String], files: &[String], at: &Path) -> std::result::Result<Ran, String> {
    let mut c = Command::new(program);
    c.args(args).args(files).current_dir(at).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    c.env("PYTHONUNBUFFERED", "1");
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
        lines.push(line);
    }
    for r in readers {
        let _ = r.join();
    }
    let status = child.wait().map_err(|e| format!("{}: {e}", program.display()))?;
    Ok(Ran { code: status.code(), lines })
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

// -- what it came to ----------------------------------------------------------- //

/// What a lint job did, kept with its work: the patches its fixes were
/// folded into, where the phase stopped and why, whether a patch on top
/// holds the fixes from there on, and what each run of the linter reported
/// that it did not fix.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Report {
    pub command: String,
    /// The patches that took fixes, by place from 0.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fixed: Vec<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stopped: Option<Stopped>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub on_top: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<Errors>,
}

/// Where the phase of a lint job stopped: the patch whose fixes could not
/// be kept, by place from 0, and why.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Stopped {
    pub at: usize,
    pub why: String,
}

/// What a run of the linter reported that it did not fix, as its exit
/// says: where it ran (a patch, by place from 0; none, the top of the
/// stack), on which files, how it exited, and the lines it wrote.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Errors {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<usize>,
    pub files: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<i32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lines: Vec<String>,
}

impl Errors {
    /// Where it ran and how it exited, said: `patch 2 · a.txt, b.txt ·
    /// exited with 1`.
    pub fn said(&self, stopped: Option<usize>) -> String {
        let at = match (self.at, stopped) {
            (Some(k), _) => format!("patch {}", k + 1),
            (None, Some(k)) => format!("the top of the stack, from patch {} on", k + 1),
            (None, None) => "the top of the stack".into(),
        };
        let exit = self.code.map_or("killed".to_string(), |c| format!("exited with {c}"));
        format!("{at} · {} · {exit}", self.files.join(", "))
    }
}

impl Report {
    /// What its change of the stack is, as the history says it: `lint
    /// fixes in patches 1, 2 and 4, and a patch on top from patch 5 on
    /// (./mach lint --fix)`.
    pub fn said(&self) -> String {
        let in_patches = (!self.fixed.is_empty()).then(|| crate::head::patches_said(&self.fixed));
        let on_top = self.stopped.as_ref().filter(|_| self.on_top).map(|s| format!("a patch on top from patch {} on", s.at + 1));
        let what = match (in_patches, on_top) {
            (Some(p), Some(t)) => format!("in {p}, and {t}"),
            (Some(p), None) => format!("in {p}"),
            (None, Some(t)) => format!("in {t}"),
            (None, None) => "in no patch".into(),
        };
        format!("lint fixes {what} ({})", self.command)
    }

    /// What its change of the stack changes, said: `patches 1 and 2, and a
    /// patch on top`.
    pub fn of(&self) -> String {
        match (self.fixed.is_empty(), self.on_top) {
            (false, true) => format!("{}, and a patch on top", crate::head::patches_said(&self.fixed)),
            (false, false) => crate::head::patches_said(&self.fixed),
            (true, _) => "a patch on top".into(),
        }
    }

    /// Where the linter reported what it did not fix, said: `at patch 2
    /// and at the top of the stack`.
    pub fn errors_said(&self) -> String {
        let at: Vec<String> = self
            .errors
            .iter()
            .map(|e| match e.at {
                Some(k) => format!("at patch {}", k + 1),
                None => "at the top of the stack".into(),
            })
            .collect();
        match at.as_slice() {
            [] => "nothing".into(),
            [one] => one.clone(),
            [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
        }
    }

    /// Run `ran` of the linter, at place `at` (none, the top) on `files`,
    /// noted: what it wrote kept, if it reported what it did not fix.
    fn note(&mut self, at: Option<usize>, files: &[String], ran: &Ran) {
        if ran.code == Some(0) {
            return;
        }
        let mut lines: Vec<String> = ran.lines.iter().take(KEPT_LINES).cloned().collect();
        if ran.lines.len() > KEPT_LINES {
            lines.push(format!("… and {} lines more: the run's stream has them all", ran.lines.len() - KEPT_LINES));
        }
        self.errors.push(Errors {
            at,
            files: files.to_vec(),
            code: ran.code,
            lines,
        });
    }
}

/// What linting a stack made of it: its patches, the new versions of those
/// that took fixes in their place, and the patch on top if one holds the
/// rest; and what was done, said.
#[derive(Debug, Clone, PartialEq)]
pub struct Linted {
    pub patches: Vec<Patch>,
    pub report: Report,
}

// -- patch by patch ------------------------------------------------------------ //

/// A run of the linter, in a worktree, on files of it: by place (a patch's
/// from 0, past the last the top of the stack), what it came to.
pub type Linting<'a> = dyn FnMut(&Path, &[String], usize) -> std::result::Result<Ran, String> + 'a;

/// Stack `patches`, on `base` of `repo`, linted in worktree `at` (put at
/// `base` first), patch by patch from the bottom, `lint` running the linter
/// (`command`) on the files of each that are there, with the stack up to it
/// and the fixes kept so far; `say` told what is done, for the stream. Each
/// patch's fixes are folded into it, as its new version, once every patch
/// above as stored is seen to still apply on it; at the first patch whose
/// fixes cannot be kept so, none is, and the linter runs once more at the
/// top, those patches as stored, on their files: what it changes goes in a
/// patch on top. What it came to; or why it could not be done.
pub fn lint_stack(repo: &Path, base: &str, patches: &[Patch], at: &Path, command: &str, lint: &mut Linting<'_>, say: &dyn Fn(&str)) -> std::result::Result<Linted, String> {
    let n = patches.len();
    let mut kept: Vec<Patch> = patches.to_vec();
    let mut report = Report {
        command: command.into(),
        ..Default::default()
    };
    let mut stopped: Option<usize> = None;
    for k in 0..n {
        am(at, &patches[k]).map_err(|e| format!("patch {} does not apply in the lint worktree: {}", k + 1, e.0))?;
        let files = existing(at, &patches[k].diff);
        if files.is_empty() {
            say(&format!("patch {} of {n}: none of its files is there to lint", k + 1));
            continue;
        }
        let ran = lint(at, &files, k)?;
        report.note(Some(k), &files, &ran);
        let changed = changes(at).map_err(|e| e.0)?;
        if changed.is_empty() {
            say(&format!("patch {} of {n}: the linter changed nothing", k + 1));
            continue;
        }
        match fold(repo, base, &kept, patches, k, at, &changed) {
            Ok(new) => {
                amend(at).map_err(|e| e.0)?;
                say(&format!("patch {} of {n}: the linter's fixes are folded into it", k + 1));
                kept[k] = new;
                report.fixed.push(k);
            }
            Err(why) => {
                discard(at).map_err(|e| e.0)?;
                say(&format!("patch {} of {n}: none of the linter's fixes is kept, as {why}; from there on, they go in a patch on top", k + 1));
                report.stopped = Some(Stopped { at: k, why });
                stopped = Some(k);
                break;
            }
        }
    }
    let mut out = kept;
    if let Some(k) = stopped {
        for (j, p) in patches.iter().enumerate().skip(k + 1) {
            am(at, p).map_err(|e| format!("patch {} does not apply in the lint worktree: {}", j + 1, e.0))?;
        }
        let mut files: Vec<String> = Vec::new();
        for p in &patches[k..] {
            for f in existing(at, &p.diff) {
                if !files.contains(&f) {
                    files.push(f);
                }
            }
        }
        if !files.is_empty() {
            let ran = lint(at, &files, n)?;
            report.note(None, &files, &ran);
            if !changes(at).map_err(|e| e.0)?.is_empty() {
                let diff = staged(at).map_err(|e| e.0)?;
                out.push(on_top(command, k, n, &report.stopped.as_ref().map(|s| s.why.clone()).unwrap_or_default(), diff));
                report.on_top = true;
                say(&format!("the top of the stack: the linter's fixes from patch {} on are in a patch on top", k + 1));
            } else {
                say("the top of the stack: the linter changed nothing");
            }
            discard(at).map_err(|e| e.0)?;
        }
    }
    Ok(Linted { patches: out, report })
}

/// The patch on top that holds `diff`, the fixes the linter (`command`)
/// made at the top of the stack on the files of patches `k` (from 0) to the
/// last of `n`, those of patch `k` not kept for `why`: titled so, its body
/// saying where they come from and what to do with them, by you.
fn on_top(command: &str, k: usize, n: usize, why: &str, diff: String) -> Patch {
    let from = match n - k {
        1 => format!("patch {n}"),
        2 => format!("patches {} and {n}", k + 1),
        _ => format!("patches {} to {n}", k + 1),
    };
    let body = format!(
        "`{command}` made these fixes at the top of the stack, on the files of {from}. Its fixes for patch {} were not folded into it, as {why}, so none was folded in from patch {} on. Fold each into the patch it belongs to, keeping every patch applying, then drop this patch.",
        k + 1,
        k + 1
    );
    let message = format!("{ON_TOP}\n\n{body}");
    let message = crate::wrap::wrapped(&message, 72).map_or(message, |w| w.message);
    crate::ops::new_patch(Draft {
        message,
        diff,
        ..Default::default()
    })
}

/// The new version of patch `k` (from 0) of `kept` (the stack, its patches
/// below `k` with the fixes kept so far), with the linter's fixes in
/// worktree `at` folded in, `changed` there: its diff with the sections of
/// the files fixed as git writes them now, the others as stored. Only once
/// every patch above, as `stored`, still applies on it, in turn, in an
/// index of the check's own on `base` of `repo` (the repository only read),
/// and it leaves the files fixed as the linter did. Why not, else: the
/// fixes add or delete a file, change one that is not text or its mode,
/// touch a file the patch renames or copies, or would keep a patch above
/// from applying.
fn fold(repo: &Path, base: &str, kept: &[Patch], stored: &[Patch], k: usize, at: &Path, changed: &[Changed]) -> std::result::Result<Patch, String> {
    for c in changed {
        match c.status {
            'M' => {}
            'A' => return Err(format!("they add {}", c.path)),
            'D' => return Err(format!("they delete {}", c.path)),
            _ => return Err(format!("they change what {} is", c.path)),
        }
        if c.old_mode != c.new_mode {
            return Err(format!("they change the mode of {}", c.path));
        }
        if c.binary {
            return Err(format!("they change {}, which is not text", c.path));
        }
    }
    let p = &kept[k];
    let secs = sections(&p.diff);
    for c in changed {
        if secs.iter().any(|s| s.path == c.path && s.moved()) {
            return Err(format!("they are to {}, which the patch renames or copies", c.path));
        }
    }
    let mut fresh = Vec::new();
    for c in changed {
        fresh.push((c.path.clone(), section_now(at, &c.path).map_err(|e| e.0)?));
    }
    let diff = spliced(&secs, &fresh);
    if !diff.lines().any(|l| l.starts_with("diff --git ")) {
        return Err("they would leave it changing nothing".into());
    }
    let new = crate::ops::revise(p, Draft { diff, ..Draft::from(p) });
    let below: Vec<&str> = kept[..k].iter().map(|p| p.diff.as_str()).chain([new.diff.as_str()]).collect();
    let above: Vec<&str> = stored[k + 1..].iter().map(|p| p.diff.as_str()).collect();
    let every: Vec<&str> = below.iter().chain(&above).copied().collect();
    let also: Vec<&str> = changed.iter().map(|c| c.path.as_str()).collect();
    let pad = crate::check::Pad::new(repo, base, &every, &also).map_err(|e| e.0)?;
    if pad.apply_each(&below).is_err() {
        return Err("they could not be written as a new version of it that applies".into());
    }
    for c in changed {
        let made = pad.text(&c.path).map_err(|e| e.0)?;
        if made.as_deref() != Some(index_text(at, &c.path).map_err(|e| e.0)?.as_str()) {
            return Err(format!("written as a new version of it, they would not leave {} as the linter did", c.path));
        }
    }
    if let Err(j) = pad.apply_each(&above) {
        return Err(format!("they would keep patch {} from applying", k + 2 + j));
    }
    Ok(new)
}

/// A file's part of a diff: the path its header names, after; and its text,
/// from its `diff --git` line on.
#[derive(Debug, Clone, PartialEq)]
struct Section {
    path: String,
    text: String,
}

impl Section {
    /// Whether it renames or copies its file.
    fn moved(&self) -> bool {
        self.text.lines().take_while(|l| !l.starts_with("@@")).any(|l| l.starts_with("rename from ") || l.starts_with("copy from "))
    }
}

/// The sections of `diff`, each a file's, in its order; what comes before
/// the first, if anything, a section of no path.
fn sections(diff: &str) -> Vec<Section> {
    let mut f = crate::diff::FileOf::default();
    let mut out: Vec<Section> = Vec::new();
    for line in diff.split_inclusive('\n') {
        let bare = line.trim_end_matches(['\n', '\r']);
        if f.read(bare) || out.is_empty() {
            out.push(Section {
                path: String::new(),
                text: String::new(),
            });
        }
        let last = out.last_mut().unwrap();
        if f.header {
            last.path.clone_from(&f.path);
        }
        last.text.push_str(line);
    }
    out
}

/// The diff `secs` make, each section of a file `fresh` holds in its place
/// (gone, when that holds nothing for it), those of files they have none of
/// after them, in `fresh`'s order.
fn spliced(secs: &[Section], fresh: &[(String, String)]) -> String {
    let mut out = String::new();
    let now = |path: &str| fresh.iter().find(|(p, _)| p == path).map(|(_, t)| t.as_str());
    for s in secs {
        match now(&s.path).filter(|_| !s.path.is_empty()) {
            Some(text) => out.push_str(text),
            None => out.push_str(&s.text),
        }
    }
    for (path, text) in fresh {
        if !secs.iter().any(|s| s.path == *path) {
            out.push_str(text);
        }
    }
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// The paths patch `diff` names, after, that are files in worktree `at`:
/// the files of it there are to lint, in its order, each once.
fn existing(at: &Path, diff: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for p in crate::diff::paths(diff) {
        if !p.is_empty() && !out.contains(&p) && std::fs::symlink_metadata(at.join(&p)).is_ok_and(|m| m.is_file()) {
            out.push(p);
        }
    }
    out
}

/// A file the linter changed in the worktree, as git has it once every
/// change is added: its path, how it changed (`M`, `A`, `D`, `T`), its
/// modes before and after, and whether git takes it for something not
/// text.
#[derive(Debug, Clone, PartialEq)]
struct Changed {
    path: String,
    status: char,
    old_mode: String,
    new_mode: String,
    binary: bool,
}

/// git at worktree `at`, a lint worktree of stacked's own: its commits
/// unsigned and no hook of the repository's run; a diff written as `git am`
/// reads one, `a/` and `b/` before its paths, whatever the settings say.
fn wgit(at: &Path, args: &[&str]) -> Result<String> {
    wgit_in(at, args, None)
}

/// As `wgit`, `stdin` given to git.
fn wgit_in(at: &Path, args: &[&str], stdin: Option<&str>) -> Result<String> {
    let mut all = vec![
        "-c",
        "commit.gpgsign=false",
        "-c",
        "core.hooksPath=/dev/null",
        "-c",
        "diff.noprefix=false",
        "-c",
        "diff.mnemonicPrefix=false",
        "-c",
        "diff.relative=false",
        "-c",
        "color.ui=false",
    ];
    all.extend(args);
    crate::git::git_in(at, &all, stdin)
}

/// Patch `p` applied in worktree `at` as a commit on its HEAD, as `git am`
/// takes it; none of it if it does not apply.
fn am(at: &Path, p: &Patch) -> Result<()> {
    let mbox = crate::mbox::format_mbox(&[p]);
    if let Err(e) = wgit_in(at, &["am", "--quiet", "--keep-non-patch"], Some(&mbox)) {
        let _ = wgit(at, &["am", "--abort"]);
        return Err(e);
    }
    Ok(())
}

/// What the linter changed in worktree `at`, every change added: each file,
/// as git has it against HEAD.
fn changes(at: &Path) -> Result<Vec<Changed>> {
    wgit(at, &["add", "-A"])?;
    let raw = wgit(at, &["diff", "--cached", "--raw", "-z", "--no-renames", "HEAD"])?;
    let num = wgit(at, &["diff", "--cached", "--numstat", "-z", "--no-renames", "HEAD"])?;
    let binary: Vec<&str> = num.split('\0').filter_map(|e| e.strip_prefix("-\t-\t")).collect();
    let mut out = Vec::new();
    let mut fields = raw.split('\0');
    while let Some(meta) = fields.next() {
        let Some(meta) = meta.strip_prefix(':') else { continue };
        let Some(path) = fields.next() else { break };
        let m: Vec<&str> = meta.split(' ').collect();
        let [old_mode, new_mode, _, _, status] = m[..] else { continue };
        out.push(Changed {
            path: path.to_string(),
            status: status.chars().next().unwrap_or('?'),
            old_mode: old_mode.into(),
            new_mode: new_mode.into(),
            binary: binary.contains(&path),
        });
    }
    Ok(out)
}

/// The section of file `path` of the diff the patch on HEAD of worktree
/// `at` makes with what is added there: the patch with the linter's fixes,
/// as git writes it. Empty when the file is as it was before the patch.
fn section_now(at: &Path, path: &str) -> Result<String> {
    wgit(
        at,
        &[
            "--literal-pathspecs",
            "diff",
            "--cached",
            "--no-renames",
            "--no-color",
            "--no-ext-diff",
            "--no-textconv",
            "--src-prefix=a/",
            "--dst-prefix=b/",
            "HEAD~1",
            "--",
            path,
        ],
    )
}

/// Every change added in worktree `at`, against HEAD, as git writes a
/// patch of them: binary files too.
fn staged(at: &Path) -> Result<String> {
    wgit(
        at,
        &["diff", "--cached", "--binary", "--no-renames", "--no-color", "--no-ext-diff", "--no-textconv", "--src-prefix=a/", "--dst-prefix=b/", "HEAD"],
    )
}

/// What the index of worktree `at` holds at `path`, as text: the blob as
/// it is, no filter of git's run on it.
fn index_text(at: &Path, path: &str) -> Result<String> {
    wgit(at, &["cat-file", "blob", &format!(":{path}")])
}

/// What is added in worktree `at` made part of its HEAD's commit.
fn amend(at: &Path) -> Result<()> {
    wgit(at, &["commit", "--quiet", "--amend", "--no-edit", "--no-verify"]).map(|_| ())
}

/// What the linter changed in worktree `at` thrown away: the worktree as
/// its HEAD has it.
fn discard(at: &Path) -> Result<()> {
    wgit(at, &["reset", "--quiet", "--hard", "HEAD"])?;
    wgit(at, &["clean", "-fdq"]).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_is_split_as_a_shell_splits_its_words() {
        let w = |s: &str| words(s).unwrap();
        assert_eq!(w("./mach lint --fix"), ["./mach", "lint", "--fix"]);
        assert_eq!(w("  lint  'a b'  \"c \\\"d\\\"\" e\\ f ''"), ["lint", "a b", "c \"d\"", "e f", ""]);
        // Nothing else of a shell's: a pipe, a variable, a glob are words.
        assert_eq!(w("lint | tee $OUT *.js"), ["lint", "|", "tee", "$OUT", "*.js"]);
        assert!(words("lint 'open").unwrap_err().contains("has a ' that nothing closes"));
        assert!(words("lint \"open").is_err());
        assert!(w("   ").is_empty());
        // Written back so that it splits into them again.
        for each in [vec!["./mach", "lint", "--fix"], vec!["my lint", "it's", "", "a\"b", "$X"]] {
            let each: Vec<String> = each.into_iter().map(String::from).collect();
            assert_eq!(words(&quoted(&each)).unwrap(), each, "{}", quoted(&each));
        }
        assert_eq!(quoted(&["./mach".into(), "lint".into(), "--fix".into()]), "./mach lint --fix");
        assert_eq!(quoted(&["my lint".into(), "it's".into()]), "'my lint' 'it'\\''s'");
    }

    #[test]
    fn only_a_stand_in_runs_from_a_process_that_keeps_nothing() {
        let at = Path::new("/w/.stacked/s/lint");
        let words: Vec<String> = ["./mach", "lint", "--fix"].map(String::from).to_vec();
        let stand_in = PathBuf::from("/tests/lint.sh");
        // The stand-in in the command's place, its arguments as they are.
        assert_eq!(program_of(&words, at, Some(stand_in.clone()), true).unwrap(), (stand_in, vec!["lint".to_string(), "--fix".to_string()]));
        // Else, from the tests' own processes, nothing.
        assert!(program_of(&words, at, None, true).unwrap_err().contains("only a stand-in for it is"));
        // Elsewhere: a path in the worktree, or one found on PATH.
        assert_eq!(program_of(&words, at, None, false).unwrap().0, at.join("./mach"));
        let eslint: Vec<String> = ["eslint", "--fix"].map(String::from).to_vec();
        assert_eq!(program_of(&eslint, at, None, false).unwrap().0, PathBuf::from("eslint"));
        assert!(program_of(&[], at, None, false).is_err());
    }

    #[test]
    fn the_history_says_where_the_fixes_went() {
        let mut r = Report {
            command: MACH.into(),
            fixed: vec![0, 1, 3],
            ..Default::default()
        };
        assert_eq!(r.said(), "lint fixes in patches 1, 2 and 4 (./mach lint --fix)");
        assert_eq!(r.of(), "patches 1, 2 and 4");
        r.stopped = Some(Stopped { at: 4, why: "they would keep patch 6 from applying".into() });
        r.on_top = true;
        assert_eq!(r.said(), "lint fixes in patches 1, 2 and 4, and a patch on top from patch 5 on (./mach lint --fix)");
        assert_eq!(r.of(), "patches 1, 2 and 4, and a patch on top");
        r.fixed.clear();
        assert_eq!(r.said(), "lint fixes in a patch on top from patch 5 on (./mach lint --fix)");
        assert_eq!(r.of(), "a patch on top");
        // What it reported, kept when it exited otherwise than with 0.
        r.note(Some(1), &["a.txt".into()], &Ran { code: Some(0), lines: vec!["fine".into()] });
        assert!(r.errors.is_empty());
        r.note(Some(1), &["a.txt".into()], &Ran { code: Some(1), lines: vec!["a.txt:2: bad".into()] });
        r.note(None, &["a.txt".into(), "b.txt".into()], &Ran { code: None, lines: vec![] });
        assert_eq!(r.errors_said(), "at patch 2 and at the top of the stack");
        assert_eq!(r.errors[0].said(Some(4)), "patch 2 · a.txt · exited with 1");
        assert_eq!(r.errors[1].said(Some(4)), "the top of the stack, from patch 5 on · a.txt, b.txt · killed");
    }

    #[test]
    fn a_patch_s_sections_are_spliced_with_those_written_now() {
        let a = "diff --git a/a.txt b/a.txt\nindex 1..2 100644\n--- a/a.txt\n+++ b/a.txt\n@@ -1 +1 @@\n-one\n+ONE\n";
        let b = "diff --git a/b.txt b/b.txt\nnew file mode 100644\nindex 0..3\n--- /dev/null\n+++ b/b.txt\n@@ -0,0 +1 @@\n+bee\n";
        let moved = "diff --git a/o.txt b/n.txt\nsimilarity index 90%\nrename from o.txt\nrename to n.txt\n";
        let secs = sections(&format!("{a}{b}{moved}"));
        assert_eq!(secs.iter().map(|s| s.path.as_str()).collect::<Vec<_>>(), ["a.txt", "b.txt", "n.txt"]);
        assert_eq!((secs[0].moved(), secs[2].moved()), (false, true));
        let b2 = "diff --git a/b.txt b/b.txt\nnew file mode 100644\nindex 0..4\n--- /dev/null\n+++ b/b.txt\n@@ -0,0 +1 @@\n+BEE\n";
        let c = "diff --git a/c.txt b/c.txt\nindex 5..6 100644\n--- a/c.txt\n+++ b/c.txt\n@@ -1 +1 @@\n-x \n+x\n";
        // b.txt written now in its place, c.txt after the rest; a.txt as it
        // was, and one whose file the fixes put back as before, gone.
        let fresh = [("b.txt".to_string(), b2.to_string()), ("c.txt".to_string(), c.to_string()), ("n.txt".to_string(), String::new())];
        assert_eq!(spliced(&secs, &fresh), format!("{a}{b2}{c}"));
        assert_eq!(spliced(&secs, &[]), format!("{a}{b}{moved}"));
    }

    /// git at `dir`, as a test runs it, its author and committer its own.
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

    /// A repository whose base has `files`, then a commit for each of
    /// `commits` (the files it writes, whole): its folder, the base, and the
    /// commits as stacked keeps them, as patches.
    fn stack(files: &[(&str, &str)], commits: &[&[(&str, &str)]]) -> (tempfile::TempDir, PathBuf, String, Vec<Patch>) {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().canonicalize().unwrap().join("repo");
        std::fs::create_dir(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["config", "commit.gpgsign", "false"]);
        git(&repo, &["config", "user.name", "C O Mitter"]);
        git(&repo, &["config", "user.email", "committer@example.com"]);
        for (f, text) in files {
            std::fs::write(repo.join(f), text).unwrap();
        }
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-qm", "base"]);
        let base = git(&repo, &["rev-parse", "HEAD"]).trim().to_string();
        for (k, c) in commits.iter().enumerate() {
            for (f, text) in c.iter() {
                std::fs::write(repo.join(f), text).unwrap();
            }
            git(&repo, &["add", "."]);
            git(&repo, &["commit", "-qm", &format!("Bug {} - patch {}", k + 1, k + 1)]);
        }
        let mail = git(&repo, &["format-patch", "--stdout", "--keep-subject", &format!("{base}..HEAD")]);
        let patches = crate::mbox::parse_mbox(&mail).unwrap().into_iter().map(crate::ops::new_patch).collect();
        (tmp, repo, base, patches)
    }

    /// The runs of a linter: by place, and the files of each.
    type Calls = Vec<(usize, Vec<String>)>;

    /// A stand-in for a linter: each line of each file given loses the
    /// white space it ends with; a line holding `NOT-LINTABLE` is reported,
    /// and the run exits with 1. Its runs kept in `calls`.
    fn strip(calls: &std::cell::RefCell<Calls>) -> impl FnMut(&Path, &[String], usize) -> std::result::Result<Ran, String> + '_ {
        move |at: &Path, files: &[String], place: usize| {
            calls.borrow_mut().push((place, files.to_vec()));
            let mut ran = Ran { code: Some(0), lines: vec![] };
            for f in files {
                let text = std::fs::read_to_string(at.join(f)).unwrap();
                for (no, l) in text.lines().enumerate() {
                    if l.contains("NOT-LINTABLE") {
                        ran.lines.push(format!("{f}:{}: NOT-LINTABLE cannot be fixed", no + 1));
                        ran.code = Some(1);
                    }
                }
                let fixed: String = text.lines().map(|l| format!("{}\n", l.trim_end())).collect();
                std::fs::write(at.join(f), fixed).unwrap();
            }
            Ok(ran)
        }
    }

    /// The stack `patches` linted with `strip`, in a worktree beside the
    /// repository: what it came to, the runs of the linter, and the worktree.
    fn linted(repo: &Path, base: &str, patches: &[Patch]) -> (std::result::Result<Linted, String>, Calls, PathBuf) {
        let at = repo.with_file_name("lint");
        crate::git::ready(repo, &at, base, "", 0, &|_| {}).unwrap();
        let calls = std::cell::RefCell::new(Vec::new());
        let done = lint_stack(repo, base, patches, &at, "lint --fix", &mut strip(&calls), &|_| {});
        (done, calls.into_inner(), at)
    }

    /// What `patches` leave of file `f`, applied in turn on `base` of `repo`.
    fn leave(repo: &Path, base: &str, patches: &[Patch], f: &str) -> String {
        let diffs: Vec<&str> = patches.iter().map(|p| p.diff.as_str()).collect();
        let pad = crate::check::Pad::new(repo, base, &diffs, &[f]).unwrap();
        assert_eq!(pad.apply_each(&diffs), Ok(()));
        pad.text(f).unwrap().unwrap_or_default()
    }

    #[test]
    fn each_fix_lands_in_the_earliest_patch_it_shows_in() {
        let lines: String = (1..=20).map(|n| format!("line {n}\n")).collect();
        let one = lines.replace("line 2\n", "TWO  \n");
        let two = one.replace("line 18\n", "EIGHTEEN \n");
        let (_tmp, repo, base, patches) = stack(&[("a.txt", &lines)], &[&[("a.txt", &one)], &[("a.txt", &two), ("b.txt", "bee  \n")]]);
        let (done, calls, at) = linted(&repo, &base, &patches);
        let l = done.unwrap();
        // Each patch linted at its place, on its files.
        assert_eq!(calls, [(0, vec!["a.txt".to_string()]), (1, vec!["a.txt".to_string(), "b.txt".to_string()])]);
        assert_eq!(l.report.fixed, [0, 1]);
        assert_eq!((l.report.stopped.clone(), l.report.on_top), (None, false));
        assert_eq!(l.patches.len(), 2);
        // Line 2's fix in patch 1, which wrote it; line 18's and b.txt's in
        // patch 2: as the linter left them, at each place.
        assert_eq!(leave(&repo, &base, &l.patches[..1], "a.txt"), one.replace("TWO  \n", "TWO\n"));
        assert_eq!(leave(&repo, &base, &l.patches, "a.txt"), two.replace("TWO  \n", "TWO\n").replace("EIGHTEEN \n", "EIGHTEEN\n"));
        assert_eq!(leave(&repo, &base, &l.patches, "b.txt"), "bee\n");
        // Patch 2's new version does not hold patch 1's fix again; each keeps
        // its change and message, a new version of its own.
        assert!(!l.patches[1].diff.contains("TWO"), "{}", l.patches[1].diff);
        for (new, old) in l.patches.iter().zip(&patches) {
            assert_eq!((&new.change, &new.message, &new.author), (&old.change, &old.message, &old.author));
            assert_ne!(new.id, old.id);
        }
        // The worktree holds the stack linted, nothing else.
        assert_eq!(git(&at, &["status", "--porcelain"]), "");
    }

    #[test]
    fn a_fix_that_would_keep_a_patch_above_from_applying_stops_the_phase() {
        let lines: String = (1..=10).map(|n| format!("line {n}\n")).collect();
        // Patch 1 writes line 2 with white space at its end; patch 2 changes
        // line 3, its context line 2 as patch 1 wrote it.
        let one = lines.replace("line 2\n", "TWO  \n").replace("line 9\n", "NINE \n");
        let two = one.replace("line 3\n", "THREE\n");
        let three = two.replace("line 7\n", "SEVEN \n");
        let (_tmp, repo, base, patches) = stack(&[("a.txt", &lines)], &[&[("a.txt", &one)], &[("a.txt", &two)], &[("a.txt", &three)]]);
        let (done, calls, at) = linted(&repo, &base, &patches);
        let l = done.unwrap();
        // Patch 1 linted at its place, its fixes not kept; then the top, on
        // the files of every patch from patch 1 on.
        assert_eq!(calls, [(0, vec!["a.txt".to_string()]), (3, vec!["a.txt".to_string()])]);
        assert!(l.report.fixed.is_empty());
        let stopped = l.report.stopped.clone().unwrap();
        assert_eq!((stopped.at, stopped.why.as_str()), (0, "they would keep patch 2 from applying"));
        assert!(l.report.on_top);
        assert_eq!(l.report.said(), "lint fixes in a patch on top from patch 1 on (lint --fix)");
        // The patches as stored, then the patch on top, which leaves the file
        // as the linter left it at the top.
        assert_eq!(l.patches[..3], patches[..]);
        let top = &l.patches[3];
        assert_eq!(top.title(), ON_TOP);
        // Its body, wrapped at 72 columns, read as one paragraph.
        assert!(top.message.lines().all(|l| l.chars().count() <= 72), "{}", top.message);
        let body = top.message.lines().skip(2).collect::<Vec<_>>().join(" ");
        assert_eq!(
            body,
            "`lint --fix` made these fixes at the top of the stack, on the files of patches 1 to 3. Its fixes for patch 1 were not folded into it, as they would keep patch 2 from applying, so none was folded in from patch 1 on. Fold each into the patch it belongs to, keeping every patch applying, then drop this patch."
        );
        let fixed: String = three.lines().map(|l| format!("{}\n", l.trim_end())).collect();
        assert_eq!(leave(&repo, &base, &l.patches, "a.txt"), fixed);
        assert_eq!(git(&at, &["status", "--porcelain"]), "");
    }

    #[test]
    fn a_clean_stack_is_left_as_it_is_and_what_is_reported_is_kept() {
        let lines: String = (1..=10).map(|n| format!("line {n}\n")).collect();
        let one = lines.replace("line 2\n", "two\n");
        let two = one.replace("line 8\n", "eight NOT-LINTABLE\n");
        let (_tmp, repo, base, patches) = stack(&[("a.txt", &lines)], &[&[("a.txt", &one)], &[("a.txt", &two)]]);
        let (done, calls, _at) = linted(&repo, &base, &patches);
        let l = done.unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(l.patches, patches, "nothing to fix");
        assert!(l.report.fixed.is_empty() && l.report.stopped.is_none() && !l.report.on_top);
        // Patch 2's run reported the line it could not fix.
        assert_eq!(l.report.errors.len(), 1);
        assert_eq!(l.report.errors[0].at, Some(1));
        assert_eq!(l.report.errors[0].lines, ["a.txt:8: NOT-LINTABLE cannot be fixed"]);
    }

    #[test]
    fn a_fix_that_adds_a_file_is_not_folded_but_goes_on_top() {
        let (_tmp, repo, base, patches) = stack(&[("a.txt", "a\n")], &[&[("a.txt", "a \n")], &[("b.txt", "b\n")]]);
        let at = repo.with_file_name("lint");
        crate::git::ready(&repo, &at, &base, "", 0, &|_| {}).unwrap();
        // A linter that leaves a file of its own, and fixes a.txt.
        let mut lint = |at: &Path, files: &[String], _: usize| -> std::result::Result<Ran, String> {
            if files.iter().any(|f| f == "a.txt") {
                std::fs::write(at.join("a.txt"), "a\n").unwrap();
                std::fs::write(at.join("lint.log"), "linted\n").unwrap();
            }
            Ok(Ran { code: Some(0), lines: vec![] })
        };
        let l = lint_stack(&repo, &base, &patches, &at, "lint", &mut lint, &|_| {}).unwrap();
        let stopped = l.report.stopped.clone().unwrap();
        assert_eq!((stopped.at, stopped.why.as_str()), (0, "they add lint.log"));
        // Both on top: the file it added, and a.txt's fix.
        assert_eq!(l.patches.len(), 3);
        let top = &l.patches[2].diff;
        assert!(top.contains("+++ b/lint.log") && top.contains("+++ b/a.txt"), "{top}");
    }
}
