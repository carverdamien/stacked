//! Asking claude, which is sending it comments. The user's checkout is never
//! touched: every ask works in a worktree of its own.
//!
//! A comment is written first, then sent, in one of three ways (`Way`):
//! claude may change the code, answer only, or build and test. Where the
//! comment is sets what claude may change (`Scope`): a comment on the stack,
//! the whole stack; a comment on a patch or on one of its lines, only that
//! patch, unless it is sent to the whole stack. Comments sent together go
//! in one ask.
//!
//! A sent comment waits as ready, and goes as soon as nothing stands in its
//! way, as in self-review: one changing ask at a time on a stack, answer-only
//! asks together, a build and test once no change is running on its stack,
//! one at a time of every stack, as builds take the machine. `launch` starts
//! what can go; it runs when a comment is sent and when an
//! ask lands, so what waits goes on its own. A comment may be batched
//! instead, as in self-review too: it waits for you to send the batch, and
//! then goes with the rest asked the same way, in one ask (the changes, and
//! the answers unless they fan out, an ask each). A lint job of a stack
//! takes its turn in the same queue as a change does, one at a time of
//! every stack (`lint`): the changes asked after it wait for it.
//!
//! Every ask works in a git worktree of the stack's own, never in the user's
//! checkout, the stack up to its place applied. A change works in
//! `.stacked/NAME/agent` (`agent-2`… when it is busy, or holds what an agent
//! left unfinished), made the first time and taken again by the next, or
//! made ahead by the editor. For the stack, all its patches as commits; for
//! a patch, the stack up to it: the patches below as commits and the patch
//! on top, the patches above to read only. A patch change
//! checks, with `stacked fits`, that the patches above still apply on it; if
//! they would not, claude changes nothing and the comment is promoted to the
//! stack. A change may run the linter of the stack's checkout, when it has
//! one, on the files it changes (`lint::Linter::rule`), and its prompt
//! says so. An answer works in `.stacked/NAME/answer-only-agent`, made and
//! taken as a change's is, and is let run nothing that changes anything; so
//! does the selection of a try push's tasks. A build and test works in
//! `.stacked/NAME/build`: kept for the next build to build on, what a build
//! made there left as it is.
//!
//! An ask that a stacked of before started in the user's checkout, as it
//! could then, is read as it was written, and lands, or is found dead, as
//! any other: the checkout is left as the agent left it, never removed nor
//! cleaned.
//!
//! When claude exits, `stacked land` files what it did: a patch change slots
//! into the stack as it is by then, the comments on lines a split moved
//! following them; replies land as turns of their threads; an answer may open
//! comments of its own within the scope, as a review does. Everything is in
//! the database: the run as it starts, each line claude writes, and what came
//! of it.
//!
//! An ask you stop (`stop`) lands at once, as failed, its agent killed if it
//! will not exit; one whose agent is gone without landing it lands as
//! failed too, once seen (`land_orphans`). The wrappers of the asks a
//! process starts are waited for once they have exited (`reap`), so that
//! none is left a zombie, which would look as if its agent still ran.
//!
//! Asks start one at a time, of every process working on the database:
//! under the launch lock (`launch_lock`), from reading the queue to the
//! spawn of their agents, the worktree each takes made or moved meanwhile.
//! Whatever else stacked does to its own worktrees is done under it too:
//! filing what an ask did there, landing one left behind, making the
//! editor's ahead and removing them, a lint job's and a try push's made
//! ready. It is never held while an agent works. It is taken before the
//! database's lock, never under it.

use std::collections::{HashMap, HashSet};
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::claude::{allowed_tools, format_event, last_object, parse_claude_replies, parse_review_notes};
use crate::config::Workspace;
use crate::db::{
    Db, Effort, Error, Moved, Note, Patch, Queued, Record, Result, Run, RunState, Stack, Stage, Way,
    err, now, stack_change,
};
use crate::git;
use crate::held::Held;
use crate::lint::Linter;
use crate::mbox::{Draft, format_mbox, parse_mbox};
use crate::ops::{Written, base_of, plan_checked};
use crate::thread::{Role, agent_open, append_agent, fenced, last_reply_way, last_role, thread_turns};
use crate::view::{MSG_PATH, find_line, mode, note_lines, title_key};

/// What an ask may change: the whole stack, or the patch of this change.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    Stack,
    Patch(String),
}

/// The scope a comment is in.
pub fn scope_of(n: &Note) -> Scope {
    if n.on_stack() {
        Scope::Stack
    } else {
        Scope::Patch(n.change.clone())
    }
}

/// The scope an ask of `n` has: the one it is in, or the whole stack when
/// it was sent so.
fn asked_scope(n: &Note) -> Scope {
    match &n.queued {
        Some(q) if q.stack => Scope::Stack,
        _ => scope_of(n),
    }
}

/// The scope run `id` was started with, if it is an ask.
pub fn run_scope(db: &Db, id: &str) -> Option<Scope> {
    light(db.run(id)?).map(|l| l.scope)
}

/// The effort run `r` was asked at, if it is an ask that had one: read from
/// its run without the rest, as it is read on every redraw. None for an
/// ask made before efforts were picked, which went at the agent's own.
pub fn effort_of(r: &Run) -> Option<Effort> {
    #[derive(Deserialize)]
    struct Asked {
        #[serde(default)]
        effort: Option<Effort>,
    }
    Asked::deserialize(r.ask.as_ref()?).ok()?.effort
}

/// The name of the effort run `r` was asked at, as `effort_of` reads it.
pub fn effort_said(r: &Run) -> Option<&'static str> {
    effort_of(r).map(Effort::name)
}

/// What run `r` is, as Runs… and `stacked runs` list it: `answer`, `pull`;
/// an ask asked at an effort with it, as the agent's turns are headed
/// (`answer(max)`).
pub fn what_said(r: &Run) -> String {
    match effort_said(r) {
        Some(e) => format!("{}({e})", r.what),
        None => r.what.clone(),
    }
}

/// A comment claude is shown, by the label the prompt gives it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Shown {
    pub label: String,
    pub change: String,
    pub id: String,
}

/// All an ask is started with, kept in its run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Info {
    pub way: Way,
    pub scope: Scope,
    /// The stack the ask started from.
    pub from: usize,
    pub notes: Vec<Shown>,
    #[serde(default)]
    pub base: String,
    #[serde(default)]
    pub repo: String,
    /// The worktree it works in, one of the stack's own once it is taken.
    /// An ask that a stacked of before started in the user's checkout has
    /// none, for an answer, or the checkout itself (`repo`), for a change;
    /// what that stacked also wrote of where the checkout had been is not
    /// read.
    #[serde(default)]
    pub worktree: Option<String>,
    /// Its worktree is one of the stack's own, kept once it lands for the
    /// next ask to take, while it holds nothing an agent left unfinished.
    #[serde(default)]
    pub kept: bool,
    /// For a patch: its id when the ask started, and where it was.
    #[serde(default)]
    pub patch: Option<(String, usize)>,
    /// Each commit made in the worktree, bottom first, and the patch it is.
    #[serde(default)]
    pub commits: Vec<(String, String)>,
    #[serde(default)]
    pub prompt: String,
    /// A round sent back from the review of landed work, or a follow-up
    /// asked from it (`follow`): that work's run, whose stack it starts from
    /// rather than the stack at `from`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on: Option<String>,
    /// With `on`: a follow-up that only reads, asked from that work's
    /// review, which stays waiting for your review: what it brings back
    /// comes into that review (`held::come_in`), not into one of its own.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub follow: bool,
    /// A guess: the landed work, by its run, whose change waits for your
    /// review and which it goes on from, as if you accept it. Accepted, it
    /// guessed right and goes on from the stack as any ask; rejected, sent
    /// back or edited by hand, it is dropped and starts again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
    /// The batch it was sent in, where it goes back in the queue if dropped.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub batch: String,
    /// Dropped, its guess wrong: its worktree is free to be taken again.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dropped: bool,
    /// Asked again: the run before it, dropped. What it brought back, if it
    /// landed, is shown to the agent to reuse what still holds; what it was
    /// given for this work, it is given again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub was: Option<String>,
    /// The folder of its own its agent writes in, outside its worktree,
    /// removed once it lands: a change to the whole stack's, for its rebase
    /// plan; a try push's selection's that may run curl, for its copy of the
    /// task graph.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub todo: Option<String>,
    /// The effort the agent is asked at (`--effort`), the one its comments
    /// were queued at; none for an ask made before efforts were picked,
    /// which goes at the agent's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<Effort>,
    /// What it may use for this work, besides what its kind of ask is
    /// given: what the review of the work it goes on with, sent back or
    /// asked again, allowed for this work of what the agent was refused
    /// (`refusals::carried`). Never saved in any settings.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed: Vec<String>,
    /// What it was let use, as claude was given it (`--allowedTools`): what
    /// it was refused is read against it as it lands.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub given: String,
}

/// The stack ask `i` works on: the stack at `i.from`; or, a round sent
/// back from the review of landed work, or a guess going on from landed
/// work that waits for review, the stack that work makes.
fn started_from(db: &Db, i: &Info) -> Result<Stack> {
    let s = db.node(i.from)?.clone();
    let Some(c) = i.on.as_deref().or(i.after.as_deref()).and_then(|on| db.run(on)?.held.as_ref()?.stack.clone()) else {
        return Ok(s);
    };
    Ok(Stack {
        patches: c.patches,
        base: c.base.or(s.base.clone()),
        ..s
    })
}

pub fn info(r: &Run) -> Option<Info> {
    serde_json::from_value(r.ask.clone()?).ok()
}

/// What deciding what goes needs of an ask: read from its run without the
/// rest (its prompt, its commits), as it is read on every redraw.
#[derive(Debug, Clone, Deserialize)]
struct Light {
    way: Way,
    scope: Scope,
    notes: Vec<Shown>,
}

fn light(r: &Run) -> Option<Light> {
    Light::deserialize(r.ask.as_ref()?).ok()
}

/// What telling a follow-up needs of an ask: read from its run without the
/// rest, as it is read on every redraw of a review.
#[derive(Debug, Clone, Deserialize)]
struct FollowLight {
    way: Way,
    notes: Vec<Shown>,
    #[serde(default)]
    on: Option<String>,
    #[serde(default)]
    follow: bool,
    #[serde(default)]
    effort: Option<Effort>,
}

/// A follow-up of landed work that is going: waiting its turn, or with the
/// agent (its run); how it was asked, at what effort, and about which
/// comments, by (change, id).
#[derive(Debug, Clone, PartialEq)]
pub struct Following {
    pub run: Option<String>,
    pub way: Way,
    pub effort: Option<Effort>,
    pub notes: Vec<(String, String)>,
}

/// The follow-ups asked from the review of landed work `work` that are
/// going: those with the agent, the first started first, then those
/// waiting their turn, a batch each.
pub fn follow_ups_going(db: &Db, work: &str) -> Vec<Following> {
    let mut out = Vec::new();
    for r in db.live_runs().into_iter().filter(|r| r.state == RunState::Started) {
        let Some(f) = r.ask.as_ref().and_then(|a| FollowLight::deserialize(a).ok()) else { continue };
        if f.follow && f.on.as_deref() == Some(work) {
            out.push(Following {
                run: Some(r.id.clone()),
                way: f.way,
                effort: f.effort,
                notes: f.notes.into_iter().map(|sh| (sh.change, sh.id)).collect(),
            });
        }
    }
    let mut waiting: Vec<(String, Following)> = Vec::new();
    for n in db.open_notes() {
        let Some(q) = n.queued.as_ref().filter(|q| q.follow && !q.batched() && q.on.as_deref() == Some(work)) else { continue };
        let at = (n.change.clone(), n.id.clone());
        match waiting.iter_mut().find(|(b, _)| *b == q.batch) {
            Some((_, f)) => f.notes.push(at),
            None => waiting.push((
                q.batch.clone(),
                Following {
                    run: None,
                    way: q.way,
                    effort: q.effort,
                    notes: vec![at],
                },
            )),
        }
    }
    waiting.sort_by(|a, b| a.0.cmp(&b.0));
    out.extend(waiting.into_iter().map(|(_, f)| f));
    out
}

/// The comments the follow-ups of landed work `work` that are going are
/// asked about.
pub fn following(db: &Db, work: &str) -> HashSet<(String, String)> {
    follow_ups_going(db, work).into_iter().flat_map(|f| f.notes).collect()
}

/// Why a follow-up stopped as the review of the work it follows up closed
/// failed, as its run and its comments say it.
pub const FOLLOW_CLOSED: &str = "you stopped the agent when you closed the review of the work it follows up";

/// Why a follow-up stopped as the work it follows up went back to the agent
/// failed, as its run and its comments say it.
pub const FOLLOW_SENT: &str = "you stopped the agent when you sent back the work it follows up";

/// The follow-ups of landed work `work` that are going, stopped, `why`, as
/// that work's review closes or the work goes back to the agent, under the
/// lock: those waiting their turn taken off the queue, their comments as
/// they were; those with the agent stopped as an answer left behind by an
/// accepted change is (`stop_left`): said first in their stream, their ask
/// landed as failed, saying `why` on their comments, then their process
/// group sent SIGTERM, which ends it. How many.
pub fn stop_follow_ups(db: &mut Db, work: &str, why: &str) -> Result<usize> {
    let going = follow_ups_going(db, work);
    for f in &going {
        let Some(id) = &f.run else {
            for (change, id) in &f.notes {
                if let Some(n) = db.note(change, id).cloned() {
                    db.set_note(Note { queued: None, ..n })?;
                }
            }
            continue;
        };
        let Some(r) = db.run(id).cloned() else { continue };
        let Some(mut i) = info(&r) else { continue };
        i.dropped = true;
        db.add_event(id, serde_json::json!({"type": STOP}))?;
        db.add_run(Run {
            state: RunState::Failed,
            summary: why.into(),
            ask: Some(serde_json::to_value(&i).unwrap()),
            ..r.clone()
        })?;
        mark_failed(db, &i, why)?;
        if let Some(dir) = &i.todo {
            drop_plan(Path::new(dir), &plans_busy(db, id));
        }
        if let Some(pid) = r.pid {
            signal(pid, libc::SIGTERM);
        }
    }
    Ok(going.len())
}

/// Whether claude may be sent `n`: the last word is yours or a reviewer's,
/// or claude's why it moved the comment to the stack, where it waits to be
/// sent again.
pub fn sendable(n: &Note) -> bool {
    n.queued.is_none()
        && match last_role(&n.text) {
            Some(Role::Agent) => last_reply_way(&n.text).as_deref() == Some("moved"),
            Some(_) => true,
            None => false,
        }
}

/// Whether `n` may be sent to build and test: a comment on the stack, or on
/// a patch as a whole (its title), not on a line.
pub fn can_test(db: &Db, n: &Note) -> bool {
    n.on_stack()
        || (n.file == MSG_PATH
            && db.latest_version(&n.change).is_some_and(|p| title_key(p) == n.line))
}

/// Whether `n` may be asked as a try push: as it may be sent to build and
/// test (`can_test`), a comment on the stack, or on a patch as a whole (its
/// title), never one on a line or a file; nor one on the stack's history,
/// which is asked about to answer only. Its push is of the patches up to
/// its own, or of all of them for the stack's.
#[cfg(feature = "try-push")]
pub fn can_try(db: &Db, n: &Note) -> bool {
    n.about.is_none() && can_test(db, n)
}

/// Why a comment on a line or a file is not asked as a try push, as every
/// way of asking one says it: the comment's own choices do not offer it.
#[cfg(feature = "try-push")]
pub const TRY_OF_A_PATCH: &str = "A try push is asked of a patch as a whole or of the stack, not of a line or a file: \
comment on the patch's title or on the stack.";

/// Whether `n` may be asked in `way`, of those the comment's own choices
/// have: a build and test, and a try push, on the stack or a patch as a
/// whole, a try push not on the history.
pub fn fits_way(db: &Db, n: &Note, way: Way) -> bool {
    match way {
        Way::Test => can_test(db, n),
        #[cfg(feature = "try-push")]
        Way::Try => can_try(db, n),
        _ => true,
    }
}

/// Is run `r` still going: started, and its process there, or about to be?
pub fn busy(r: &Run) -> bool {
    r.state == RunState::Started && r.pid.is_none_or(alive)
}

/// Whether process `pid` is there, and has not exited. A child of this
/// process that has exited is gone, though `kill` still finds it until
/// it is waited for (a zombie): an ask's wrapper this process started is
/// waited for now (`STARTED`); any other child is left for whoever started
/// it to wait for, its exit kept for them.
pub fn alive(pid: u32) -> bool {
    let Some(p) = process(pid) else { return false };
    if let Some(running) = wrapper_running(pid) {
        return running;
    }
    // A child of this process: exited or not, asked without waiting for it
    // (WNOWAIT). The pid is said only when it has exited.
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    let options = libc::WEXITED | libc::WNOHANG | libc::WNOWAIT;
    if unsafe { libc::waitid(libc::P_PID, p as libc::id_t, &mut info, options) } == 0 {
        return unsafe { info.si_pid() } == 0;
    }
    // Not this process's child: there, or not.
    unsafe { libc::kill(p, 0) == 0 }
}

/// Pid `pid` as the system takes it, if it can be a process of an ask's:
/// never 0 nor 1, nor a number `kill` and `waitpid` would read as a group.
fn process(pid: u32) -> Option<i32> {
    i32::try_from(pid).ok().filter(|&p| p > 1)
}

/// The wrappers of the asks this process started (`start`), each kept
/// until it has exited and been waited for: so that none is left a zombie,
/// which `kill` finds as if its agent still ran.
static STARTED: std::sync::Mutex<Vec<std::process::Child>> = std::sync::Mutex::new(Vec::new());

fn started() -> std::sync::MutexGuard<'static, Vec<std::process::Child>> {
    STARTED.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// `child`, a process this one started for a run (a try push's, a lint
/// job's), kept to wait for once it has exited, as an ask's wrapper is.
pub(crate) fn keep_started(child: std::process::Child) {
    started().push(child);
}

/// The stacked that a run started from here calls back, or that runs it:
/// the tests' own binary when they set one (`stand_in`), else this one.
pub(crate) fn stacked_exe() -> Result<PathBuf> {
    match CLAUDE.lock().unwrap().clone() {
        Some((_, stacked)) => Ok(stacked),
        None => std::env::current_exe().map_err(|e| Error(e.to_string())),
    }
}

/// The process group of `pid` stopped, as an ask's agent is: sent SIGTERM,
/// then SIGKILL if it is not gone within a few seconds. Whether it is gone.
/// A try push's, a lint job's.
pub(crate) fn stop_group(pid: u32) -> bool {
    signal(pid, libc::SIGTERM);
    if gone_within(pid, TERM_GRACE) {
        return true;
    }
    signal(pid, libc::SIGKILL);
    gone_within(pid, KILL_GRACE)
}

/// Whether the wrapper `pid` this process started still runs, if it is
/// one: waited for, and let go, once it has exited.
fn wrapper_running(pid: u32) -> Option<bool> {
    let mut started = started();
    let k = started.iter().position(|c| c.id() == pid)?;
    if matches!(started[k].try_wait(), Ok(None)) {
        return Some(true);
    }
    // Waited for already: its handle gives back what it kept of the exit.
    let _ = started.swap_remove(k).wait();
    Some(false)
}

/// Waits for the wrappers this process started that have exited: wherever
/// orphans are landed, as on each launch tick of the editor, whichever
/// front-end runs it.
fn reap() {
    started().retain_mut(|c| matches!(c.try_wait(), Ok(None)));
}

/// What leaving the editor says of the asks still running: a line for
/// each, as `stacked runs` says it, and where its agent works: its
/// worktree; for an answer a stacked of before started in the user's
/// checkout, that checkout.
pub fn still_running(db: &Db) -> Vec<String> {
    db.live_runs()
        .iter()
        .filter(|r| running(r))
        .map(|r| {
            let words = r.request.split_whitespace().collect::<Vec<_>>().join(" ");
            let asked = match words.chars().count() {
                0 => String::new(),
                n if n > 60 => format!(" \"{}…\"", words.chars().take(59).collect::<String>()),
                _ => format!(" \"{words}\""),
            };
            // A try push: where it pushes from, once its worktree is taken.
            #[cfg(feature = "try-push")]
            if let Some(w) = r.push.as_ref().and_then(|p| p.worktree.as_deref()) {
                return format!("{} {} {}{asked} pushes to try from {w}", r.id, r.what, r.stack);
            }
            // A lint job: where it lints, once its worktree is taken.
            if let Some(j) = &r.lint {
                return match &j.worktree {
                    Some(w) => format!("{} {} {}{asked} lints in {w}", r.id, r.what, r.stack),
                    None => format!("{} {} {}{asked} is being started, its worktree being made", r.id, r.what, r.stack),
                };
            }
            let at = match info(r) {
                Some(Info { worktree: Some(w), .. }) => format!("works in {w}"),
                Some(i) if r.pid.is_some() && !i.repo.is_empty() => format!("works in {}", i.repo),
                _ => "is being started, its worktree being made".to_string(),
            };
            format!("{} {} {}{asked} {at}", r.id, what_said(r), r.stack)
        })
        .collect()
}

/// Is run `r` running: started, and its process there.
pub fn running(r: &Run) -> bool {
    r.state == RunState::Started
        && match r.pid {
            Some(pid) => alive(pid),
            // Being started: its worktree made.
            None => r.starter.is_some_and(alive),
        }
}

/// The asks of stack `name` still going, with what each was started with.
fn asks_going<'a>(db: &'a Db, name: &str) -> Vec<(&'a Run, Light)> {
    db.live_runs()
        .into_iter()
        .filter(|r| r.stack == name && busy(r))
        .filter_map(|r| Some((r, light(r)?)))
        .collect()
}

/// The comments the agent is working on, to how it was sent them and the
/// run.
pub fn with_agent(db: &Db) -> HashMap<(String, String), (Way, String)> {
    let mut out = HashMap::new();
    for r in db.live_runs().into_iter().filter(|r| busy(r)) {
        if let Some(i) = light(r) {
            for n in &i.notes {
                out.insert((n.change.clone(), n.id.clone()), (i.way, r.id.clone()));
            }
        }
    }
    // A comment whose try push is under way is locked as one the agent
    // works on, until the push is over or no longer followed.
    #[cfg(feature = "try-push")]
    for (at, run) in crate::try_push::locks(db) {
        out.entry(at).or_insert((Way::Try, run));
    }
    out
}

/// Whether what is asked in `way` only reads: an answer, or a try push's
/// selection. Such asks run together, and alongside the changes when
/// chained.
pub fn reads(way: Way) -> bool {
    match way {
        Way::Answer => true,
        #[cfg(feature = "try-push")]
        Way::Try => true,
        _ => false,
    }
}

/// What a comment sent on stack `name` in `way` would wait on, if anything.
pub fn waits_on(db: &Db, name: &str, way: Way) -> Option<String> {
    waits_for(db, name, way, None)
}

/// What the reconcile with HEAD of stack `name`, waiting its turn, waits
/// for, if anything: what goes before it in the queue, as `claim` takes
/// it, not what was asked after it. None when none waits.
pub fn reconcile_waits_on(db: &Db, name: &str) -> Option<String> {
    let r = crate::reconcile::pending(db, name).filter(|r| r.state == RunState::Started)?;
    waits_for(db, name, Way::Change, Some(&r.id))
}

/// What the pull from Phabricator of stack `name`, under way, waits for, if
/// anything: what goes before it in the queue, as `claim` takes it, not
/// what was asked after it, which waits for it. Nothing: its turn has come,
/// and it is being made, unless it still reads Phabricator. None when no
/// pull of it is under way.
#[cfg(feature = "moz-phab")]
pub fn pull_waits_on(db: &Db, name: &str) -> Option<String> {
    let r = crate::review::pending(db, name)?;
    waits_for(db, name, Way::Change, Some(&r.id))
}

/// The key of run `id`'s turn in the queue, a lint job's, a reconcile
/// with HEAD's or a pull from Phabricator's: `b` and when it was asked, as
/// a batch's key is, so that `claim` takes the turns in the order they
/// were asked.
fn turn_key(id: &str) -> String {
    format!("b{}", id.trim_start_matches("pull-"))
}

/// What `waits_on` says: for what is asked now, with `mine` none; for
/// `mine`, the run of a lint job, a reconcile with HEAD or a pull waiting
/// in the queue already, what goes before it there, as `claim` takes it:
/// what was asked after it waits for it, and does not hold it back, unless
/// it is under way already (a lint job running, a reconcile made and
/// waiting for your review).
fn waits_for(db: &Db, name: &str, way: Way, mine: Option<&str>) -> Option<String> {
    let going = asks_going(db, name);
    let chain = chains(&agent_of(db, name));
    // Asked before `mine`: in the queue ahead of it; anything but it, for
    // what is asked now.
    let ahead = |id: &str| mine.is_none_or(|m| id != m && turn_key(id) < turn_key(m));
    // A lint job takes its turn as a change does: a change waits for it, a
    // build and test for one that runs, and, unchained, an answer.
    if let Some(r) = crate::lint::pending(db, name)
        && let Some(j) = r.lint.as_deref()
        && mine != Some(r.id.as_str())
        && (j.step == crate::lint::Step::Running || ahead(&r.id))
        && (way == Way::Change || (way == Way::Test && j.step == crate::lint::Step::Running) || (!chain && reads(way)))
    {
        return Some("once the linter's work on the stack lands".into());
    }
    // A reconcile with HEAD, as a change: a change waits until it is
    // reviewed; an answer goes at once.
    if way == Way::Change
        && let Some(r) = crate::reconcile::pending(db, name)
        && mine != Some(r.id.as_str())
        && (r.state != RunState::Started || ahead(&r.id))
    {
        return Some(match r.state {
            RunState::Started => "once the reconcile with HEAD before it is made and you have reviewed it".into(),
            _ => "once you have reviewed the reconcile with HEAD".into(),
        });
    }
    // A pull from Phabricator under way, as a change: a change goes once it
    // is made, from the stack it makes.
    #[cfg(feature = "moz-phab")]
    if way == Way::Change
        && let Some(r) = crate::review::pending(db, name)
        && ahead(&r.id)
    {
        return Some("once the pull from Phabricator before it is made".into());
    }
    // Chained, an answer runs alongside a change, on the stack as it is.
    let change = going.iter().find(|(_, i)| i.way == Way::Change).filter(|_| !(chain && reads(way)));
    if let Some((_, i)) = change {
        let on = match &i.scope {
            Scope::Stack => "the stack".to_string(),
            Scope::Patch(c) => format!("patch {}", position(db, name, c).map_or("?".into(), |p| (p + 1).to_string())),
        };
        return Some(format!("once the agent's change on {on} lands"));
    }
    // A change goes from the stack as it is once the change before it is
    // reviewed; chained, at once, from the stack that change makes.
    if way == Way::Change
        && !chain
        && let Some(r) = crate::held::change_waiting(db, name)
        && let Some(c) = r.held.as_ref().and_then(|h| h.stack.as_ref())
    {
        return Some(format!("once you have reviewed {} of {}", whose_change(r), c.of));
    }
    match way {
        Way::Change if !chain && going.iter().any(|(_, i)| reads(i.way)) => Some("once the running answers land".into()),
        // One build and test at a time, of every stack.
        Way::Test => testing(db).map(|r| match r.stack == name {
            true => "once the running build and test lands".to_string(),
            false => format!("once the running build and test of {} lands", r.stack),
        }),
        _ => None,
    }
}

/// The agent's settings for stack `name`: its repository's checkout's, as
/// an ask of it starts with.
fn agent_of(db: &Db, name: &str) -> crate::config::Agent {
    let dir = db.stack(name).ok().and_then(|s| s.repo.clone()).map_or_else(|| PathBuf::from("."), PathBuf::from);
    crate::config::of(&dir).0.agent
}

/// Whether, with `agent`'s settings, a change or an answer goes on from the
/// change waiting for your review, guessing you accept it: unless the
/// setting says not.
fn chains(agent: &crate::config::Agent) -> bool {
    agent.chain
}

/// What the next change or answer of stack `name` goes on from, chained:
/// the last landed work whose change of the stack waits for your review.
fn guessed_from(db: &Db, name: &str) -> Option<String> {
    crate::held::queue(db, name).into_iter().rev().find(|r| r.held.as_ref().is_some_and(|h| h.stack.is_some())).map(|r| r.id.clone())
}

/// The work run `r` goes on from, guessing you accept it, while that work
/// waits for your review: it is reviewed first. An ask's, a reconcile with
/// HEAD's (`reconcile::after`), or a pull's (`review::after`).
pub fn guessing(db: &Db, r: &Run) -> Option<String> {
    let after = info(r).and_then(|i| i.after).or_else(|| crate::reconcile::after(r).map(String::from)).or_else(|| pull_after(r))?;
    db.run(&after).is_some_and(|w| crate::held::waiting(db, w)).then_some(after)
}

/// The work pull `r` goes on from, if it is a pull chained on some.
pub(crate) fn pull_after(r: &Run) -> Option<String> {
    #[cfg(feature = "moz-phab")]
    return crate::review::after(r).map(String::from);
    #[cfg(not(feature = "moz-phab"))]
    return {
        let _ = r;
        None
    };
}

/// Whether, with the agent's settings for stack `name`, the edits of it go
/// on from the change waiting for your review, guessing you accept it.
pub fn chained(db: &Db, name: &str) -> bool {
    chains(&agent_of(db, name))
}

/// The runs going on from landed work `id`, guessing it is accepted, that
/// still work or wait for your review.
fn guesses_on(db: &Db, id: &str) -> Vec<Run> {
    db.live_runs()
        .into_iter()
        .filter(|r| info(r).is_some_and(|i| i.after.as_deref() == Some(id) && !i.dropped))
        .filter(|r| r.state == RunState::Started || crate::held::waiting(db, r))
        .cloned()
        .collect()
}

/// Landed work `id` accepted, its change now the stack at `rev`: what went
/// on from it guessed right, and goes on from that stack as any ask does.
pub fn guessed_right(db: &mut Db, id: &str, rev: usize) -> Result<()> {
    for r in guesses_on(db, id) {
        let Some(mut i) = info(&r) else { continue };
        (i.from, i.after) = (rev, None);
        db.add_run(Run {
            ask: Some(serde_json::to_value(&i).unwrap()),
            ..r
        })?;
    }
    #[cfg(feature = "moz-phab")]
    crate::review::followed(db, id, rev)?;
    crate::reconcile::followed(db, id, rev)
}

/// Run `r`, of ask `i`, dropped (`why`): its agent stopped if it still
/// works, what it brought back thrown away unseen, its comments back in the
/// queue as they were sent, to be asked again.
fn drop_run(db: &mut Db, r: &Run, mut i: Info, why: String) -> Result<()> {
    i.dropped = true;
    let running = r.state == RunState::Started;
    // Landed, its review closes: what it allowed from now on is saved, as
    // when a review closes, and where it cannot be, allowed for this work
    // instead, which the work asked again is given.
    let mut v = db.review(&r.id).cloned().unwrap_or_default();
    let settled = if running { crate::refusals::Settled::default() } else { crate::refusals::settle(db, r, &mut v.marks, true) };
    db.add_run(Run {
        state: if running { RunState::Failed } else { r.state },
        summary: format!("{why}{}", settled.said(i.way)),
        ask: Some(serde_json::to_value(&i).unwrap()),
        ..r.clone()
    })?;
    if running {
        // Sent SIGTERM only, under the lock: the run is landed already.
        if let Some(pid) = r.pid {
            signal(pid, libc::SIGTERM);
        }
    } else {
        db.set_review(&r.id, crate::held::Review { closed: true, ..v })?;
    }
    // Landed, what it brought back is shown to the agent asked again; and,
    // landed or not, what this work may use goes on with it.
    ask_again(db, &i, Some(r.id.clone()))
}

/// Landed work `id` rejected, sent back or edited by hand (`why`): what
/// went on from it guessed wrong. Each is dropped, and asked again; and so
/// with what went on from it. How many.
pub fn guessed_wrong(db: &mut Db, id: &str, why: &str) -> Result<usize> {
    let mut n = 0;
    for r in guesses_on(db, id) {
        let Some(i) = info(&r) else { continue };
        drop_run(db, &r, i, format!("dropped: the work it went on from was {why}; asked again"))?;
        n += 1 + guessed_wrong(db, &r.id, "dropped")?;
    }
    // A reconcile with HEAD gone on from it, or a pull: dropped, and made
    // again at its next turn.
    #[cfg(feature = "moz-phab")]
    {
        n += crate::review::dropped(db, id, why)?;
    }
    Ok(n + crate::reconcile::dropped(db, id, why)?)
}

/// What rejecting landed work `id`, or sending it back, drops: what went on
/// from it, and from that; still working, and waiting for your review.
pub fn guesses_after(db: &Db, id: &str) -> (usize, usize) {
    let mut out = (0, 0);
    for r in guesses_on(db, id) {
        let (running, waiting) = guesses_after(db, &r.id);
        let own = usize::from(r.state == RunState::Started);
        out = (out.0 + own + running, out.1 + (1 - own) + waiting);
    }
    #[cfg(feature = "moz-phab")]
    {
        out.1 += crate::review::going_on_from(db, id).len();
    }
    (out.0, out.1 + crate::reconcile::going_on_from(db, id).len())
}

/// The asks of stack `name` that only read (answers, and a try push's
/// selection), but run `but`, still running or waiting for your review:
/// made on the stack as it was. Not a follow-up asked from the review of
/// landed work, which reads the stack that work makes, and whose work comes
/// into that review.
fn answers_in_flight(db: &Db, name: &str, but: &str) -> Vec<Run> {
    db.live_runs()
        .into_iter()
        .filter(|r| r.stack == name && r.id != but && info(r).is_some_and(|i| reads(i.way) && !i.dropped && !i.follow))
        .filter(|r| r.state == RunState::Started || crate::held::waiting(db, r))
        .cloned()
        .collect()
}

/// The answers made on stack `name` before the change of the work run `id`
/// holds, that accepting that change leaves behind, chained: the asks that
/// only read (answers, and a try push's selection) running, or landed and
/// waiting for your review, made on the stack as it was. The review of the
/// change marks each keep, restart or stop before it closes with the change
/// accepted (`leave_answers`). None unchained: a change then waits for the
/// answers running, and accepting it touches none of them.
pub fn answers_left(db: &Db, name: &str, id: &str) -> Vec<Run> {
    if !chains(&agent_of(db, name)) {
        return vec![];
    }
    answers_in_flight(db, name, id)
}

/// The answers `answers`, made on the stack before a change now accepted,
/// each as the change's review `marks` it (`held::answer_mark`): kept, it
/// goes on as it is, or waits for your review as it is, on the stack it was
/// asked on; restarted, it is dropped and asked again on the stack as it is
/// now, shown what it brought back if it landed; stopped, its agent is
/// stopped and its ask landed as failed, or its work is thrown away unseen,
/// and its comments stay open, not asked again (`stop_left`). One not
/// marked is kept. What came of them, said, a count each.
pub fn leave_answers(db: &mut Db, answers: &[Run], marks: &std::collections::BTreeMap<String, bool>) -> Result<Vec<String>> {
    use crate::held::AnswerMark;
    let (mut again, mut kept, mut stopped) = (0, 0, 0);
    for r in answers {
        let Some(i) = info(r) else { continue };
        match crate::held::answer_mark(marks, &r.id) {
            Some(AnswerMark::Restart) => {
                drop_run(db, r, i, "dropped: an edit of the stack was accepted meanwhile; asked again".into())?;
                again += 1;
            }
            Some(AnswerMark::Stop) => {
                stop_left(db, r, i)?;
                stopped += 1;
            }
            Some(AnswerMark::Keep) | None => kept += 1,
        }
    }
    let said = |n: usize, what: &str| match n {
        1 => format!("1 answer-only ask {what}"),
        n => format!("{n} answer-only asks {what}"),
    };
    Ok([(again, "asked again"), (kept, "kept"), (stopped, "stopped")].into_iter().filter(|(n, _)| *n > 0).map(|(n, what)| said(n, what)).collect())
}

/// What the summary of an answer stopped running as the change made on the
/// stack after it is accepted says, and its comments: why its ask failed.
pub const STOPPED_LEFT: &str = "you stopped the agent when you accepted an edit of the stack";

/// Answer run `r`, of ask `i`, made on the stack before a change, stopped as
/// that change is accepted. Running, its agent is stopped as `stop` stops
/// it, under the lock: said first in its stream, its ask landed as failed
/// and its comments said so, then its process group sent SIGTERM, which
/// ends it; its worktree free to be taken again. Landed, its work is thrown
/// away unseen, as a rejected one's is: its review closed, what it allowed
/// from now on saved, as a review closing saves it. Either way, its
/// comments stay open, and are not asked again.
fn stop_left(db: &mut Db, r: &Run, mut i: Info) -> Result<()> {
    if r.state == RunState::Started {
        i.dropped = true;
        db.add_event(&r.id, serde_json::json!({"type": STOP}))?;
        db.add_run(Run {
            state: RunState::Failed,
            summary: STOPPED_LEFT.into(),
            ask: Some(serde_json::to_value(&i).unwrap()),
            ..r.clone()
        })?;
        mark_failed(db, &i, STOPPED_LEFT)?;
        if let Some(pid) = r.pid {
            signal(pid, libc::SIGTERM);
        }
        return Ok(());
    }
    let mut v = db.review(&r.id).cloned().unwrap_or_default();
    let settled = crate::refusals::settle(db, r, &mut v.marks, true);
    db.add_run(Run {
        summary: format!("reviewed: thrown away unseen, as you stopped it when you accepted an edit of the stack{}", settled.said(i.way)),
        ..r.clone()
    })?;
    db.set_review(&r.id, crate::held::Review { closed: true, ..v })?;
    Ok(())
}

/// The comments run `r` was asked about, by (change, id), read from its
/// run without the rest, as it is read on every redraw: none for a run
/// that is no ask.
pub fn asked_about(r: &Run) -> Vec<(String, String)> {
    light(r).map(|l| l.notes.into_iter().map(|sh| (sh.change, sh.id)).collect()).unwrap_or_default()
}

/// Whether run `r` is a try push's selection, read from its run without
/// the rest: the asks that only read are answers but for those.
pub fn selects_tasks(r: &Run) -> bool {
    #[cfg(feature = "try-push")]
    return light(r).is_some_and(|l| l.way == Way::Try);
    #[cfg(not(feature = "try-push"))]
    return {
        let _ = r;
        false
    };
}

/// The builds and tests of stack `name` going: of the stack as it is, which
/// accepting a change leaves behind. Not asked again; you may stop them.
pub fn builds_going(db: &Db, name: &str) -> Vec<Run> {
    db.live_runs()
        .into_iter()
        .filter(|r| r.stack == name && busy(r) && info(r).is_some_and(|i| i.way == Way::Test))
        .cloned()
        .collect()
}

/// The comments ask `i` was about back in the queue, ready, in the batch
/// they were sent in: they go again as they were asked, at the effort they
/// were asked at, the agent shown what run `was` brought back, if it
/// landed, and given what that run was given for this work.
fn ask_again(db: &mut Db, i: &Info, was: Option<String>) -> Result<()> {
    let batch = if i.batch.is_empty() { format!("b{}", crate::db::unique()) } else { i.batch.clone() };
    for sh in &i.notes {
        let Some(n) = db.note(&sh.change, &sh.id).cloned() else { continue };
        let stack = i.scope == Scope::Stack && !n.on_stack();
        db.set_note(Note {
            queued: Some(Queued {
                way: i.way,
                batch: batch.clone(),
                stack,
                stage: Stage::Ready,
                on: i.on.clone(),
                was: was.clone(),
                effort: i.effort,
                follow: i.follow,
            }),
            failed: None,
            ..n
        })?;
    }
    Ok(())
}

/// The stack landed work run `id` makes: its change of the stack on the
/// stack it is of, whether it still waits for review or not.
pub fn work_stack(db: &Db, id: &str) -> Result<Stack> {
    let r = db.run(id).ok_or_else(|| Error(format!("no run {id}")))?;
    let c = r.held.as_ref().and_then(|h| h.stack.clone()).ok_or_else(|| Error(format!("run {id} holds no change of the stack")))?;
    let s = db.stack(&r.stack)?.clone();
    Ok(Stack {
        patches: c.patches,
        base: c.base.or(s.base.clone()),
        ..s
    })
}

/// What goes on a stack as `claim` starts batches.
struct Going {
    /// A change is going; and how many answers.
    change: bool,
    answers: usize,
    /// A change waits for what is going, before the answers behind it.
    change_waits: bool,
    /// A change waits for your review.
    reviewing: bool,
    /// Changes go on from the change under review, guessing; answers run
    /// alongside them, on the stack as it is.
    chain: bool,
    /// The work under review they go on from: the last landed.
    tail: Option<String>,
    /// A lint job of the stack waits its turn ahead: the changes after it
    /// go once it has landed.
    lint: bool,
    /// A reconcile with HEAD waits its turn ahead, or for your review: the
    /// changes after it go once it is accepted or rejected.
    reconciling: bool,
    /// A pull from Phabricator is ahead, reading Phabricator, waiting its
    /// turn, or to be made once the lock is given back: the changes after it
    /// go once it is made.
    pulling: bool,
}

/// The build and test in flight, of whichever stack: one at a time, as
/// builds take the machine.
fn testing(db: &Db) -> Option<&Run> {
    db.live_runs().into_iter().find(|r| busy(r) && light(r).is_some_and(|i| i.way == Way::Test))
}

/// Why stack `name` cannot be edited by hand now, if it cannot: the agent
/// is changing it, from the stack as it took it, or is about to go on from
/// work you sent back, from the stack as that work left it; a lint job or
/// a pull from Phabricator waits its turn or is under way; so nothing else
/// may change the stack meanwhile. Or a change of it waits for your review.
pub fn blocked(db: &Db, name: &str) -> Option<String> {
    match changer(db, name) {
        Some(Changer::Agent(on)) => {
            return Some(format!(
                "the agent is changing {on}: the stack can be edited by hand again once you have reviewed the agent's work"
            ));
        }
        Some(Changer::Round) => {
            return Some(
                "the work you sent back waits to go back to the agent: the stack can be edited by hand again once you have reviewed the agent's new work"
                    .into(),
            );
        }
        Some(Changer::Lint(said)) => {
            let r = crate::lint::pending(db, name);
            let then = if r.and_then(|r| r.lint.as_deref()).is_some_and(|j| j.stopping) {
                "once the linter is stopped, in a few seconds"
            } else {
                "once the linter is done and you have reviewed what it fixed, if it fixed anything"
            };
            return Some(format!(
                "the linter {said} ({}): the stack can be edited by hand again {then}",
                r.map(|r| r.request.as_str()).unwrap_or_default()
            ));
        }
        #[cfg(feature = "moz-phab")]
        Some(Changer::Pull(said, asked)) => {
            return Some(format!(
                "a pull from Phabricator {said} ({asked}): the stack can be edited by hand again once the pull is made and you have reviewed its change of the stack, if it makes one"
            ));
        }
        None => {}
    }
    let r = crate::held::change_waiting(db, name)?;
    let of = r.held.as_ref()?.stack.as_ref()?.of.clone();
    Some(format!(
        "{} of {of} waits for your review: the stack can be edited by hand again once you have reviewed it",
        whose_change(r)
    ))
}

/// What changes stack `name` from the stack as it was, its work to land:
/// the agent, at a place (`patch 2`, `the stack`); or, soon, a round sent
/// back to it; or a lint job, where it is said (`lints patch 2 of 5`,
/// `waits its turn to lint the stack`); or a pull from Phabricator, where it
/// is said (`is reading Phabricator`, `waits its turn`, `is being made`),
/// with what it was asked (`the stack's revisions, with their parents and
/// those stacked on top`).
enum Changer {
    Agent(String),
    Round,
    Lint(String),
    #[cfg(feature = "moz-phab")]
    Pull(&'static str, String),
}

/// What changes stack `name` now, if anything does: the agent changing it
/// from the stack as it took it, a round sent back about to, a lint job
/// waiting its turn or running, or a pull from Phabricator from when it is
/// asked to when it is filed, reading Phabricator, waiting its turn or
/// being made, as an edit is (`review::make`).
fn changer(db: &Db, name: &str) -> Option<Changer> {
    if let Some((_, i)) = asks_going(db, name).into_iter().find(|(_, i)| i.way == Way::Change) {
        return Some(Changer::Agent(match &i.scope {
            Scope::Stack => "the stack".to_string(),
            Scope::Patch(c) => format!("patch {}", position(db, name, c).map_or("?".into(), |p| (p + 1).to_string())),
        }));
    }
    if let Some(j) = crate::lint::pending(db, name).and_then(|r| r.lint.as_deref()) {
        return Some(Changer::Lint(j.said()));
    }
    if round_waiting(db, name) {
        return Some(Changer::Round);
    }
    // A pull reads Phabricator first; then it waits its turn while what goes
    // before it holds it back, as its banner line's menu says; else its
    // turn has come, and it is made.
    #[cfg(feature = "moz-phab")]
    if let Some(r) = crate::review::pending(db, name) {
        let said = crate::review::phase(db, r).map_or("waits its turn", crate::review::Phase::said);
        return Some(Changer::Pull(said, r.request.clone()));
    }
    None
}

/// What changes stack `name` from the stack as it was, its work still to
/// land, said, if anything does: `the agent is changing patch 2`, work you
/// sent back waiting to go back to the agent, `the linter lints patch 2 of
/// 5`, `a pull from Phabricator is reading Phabricator`, `… waits its
/// turn`, `… is being made`. Nothing else may change the stack until that
/// work lands, or its change would be made from a stack that is no longer.
pub fn changing(db: &Db, name: &str) -> Option<String> {
    Some(match changer(db, name)? {
        Changer::Agent(on) => format!("the agent is changing {on}"),
        Changer::Round => "the work you sent back waits to go back to the agent".into(),
        Changer::Lint(said) => format!("the linter {said}"),
        #[cfg(feature = "moz-phab")]
        Changer::Pull(said, _) => format!("a pull from Phabricator {said}"),
    })
}

/// Whether a round sent back from the review of stack `name`'s landed work,
/// one that may change the code, waits its turn.
fn round_waiting(db: &Db, name: &str) -> bool {
    let waits = |n: &Note| n.queued.as_ref().is_some_and(|q| q.on.is_some() && q.way == Way::Change && !q.batched());
    db.stack_notes(name).into_iter().any(waits) || queued_off_stacks(db).into_iter().any(|(s, n)| s == name && waits(n))
}

/// The comments waiting their turn in a round sent back from landed work
/// that are on a patch no stack holds: one that work adds, which the round
/// goes on from, written on in its review. Each with the stack of that
/// work, which `stack_notes` does not find them on.
fn queued_off_stacks(db: &Db) -> Vec<(String, &Note)> {
    let stacks = db.stacks();
    let held: HashSet<&str> = stacks.iter().flat_map(|s| s.patches.iter().map(|id| db.patch(id).change.as_str())).collect();
    let own: HashSet<String> = stacks.iter().map(|s| stack_change(&s.name)).collect();
    db.open_notes()
        .into_iter()
        .filter(|n| !held.contains(n.change.as_str()) && !own.contains(&n.change))
        .filter_map(|n| {
            let on = n.queued.as_ref().filter(|q| !q.batched())?.on.as_deref()?;
            Some((db.run(on)?.stack.clone(), n))
        })
        .collect()
}

/// Whose change of the stack run `r` holds, said: `your edit by hand`,
/// `the agent's change`.
pub fn whose_change(r: &Run) -> &'static str {
    if crate::held::by_hand(r) {
        "your edit by hand"
    } else if crate::held::linted(r) {
        "the linter's change"
    } else if crate::reconcile::is(r) {
        "the reconcile with HEAD"
    } else if crate::held::pulled(r) {
        "the pull's change"
    } else {
        "the agent's change"
    }
}

/// Where the patch of `change` is in stack `name`, from 0.
fn position(db: &Db, name: &str, change: &str) -> Option<usize> {
    db.stack(name)
        .ok()?
        .patches
        .iter()
        .position(|id| db.patch(id).change == change)
}

/// For the tests: the gate a test sets, `$STACKED_TEST_GATE`, a folder.
/// This process says it is there, with a file named after its pid, and
/// waits until `go` is there too, so that the processes a test starts send
/// at the same instant. Nothing, unset.
fn at_the_gate() {
    if let Some(gate) = std::env::var_os("STACKED_TEST_GATE").map(PathBuf::from) {
        let _ = std::fs::write(gate.join(std::process::id().to_string()), "");
        until_go(&gate);
    }
}

/// For the tests: the folder `$STACKED_TEST_HOLD` names, if set. A start
/// that has taken its worktree says so there (`start-RUN`), then waits
/// until `go` is there too, holding the launch lock as a slow start would
/// (`held_at_start`); a process that finds the launch lock held says so
/// there (`waits-PID`), then waits for it (`launch_lock`).
fn test_hold() -> Option<PathBuf> {
    std::env::var_os("STACKED_TEST_HOLD").map(PathBuf::from)
}

/// A start of run `id` held midway, its worktree taken, while the tests say
/// (`test_hold`).
fn held_at_start(id: &str) {
    if let Some(hold) = test_hold() {
        let _ = std::fs::write(hold.join(format!("start-{id}")), "");
        until_go(&hold);
    }
}

/// Waits until `go` is in the folder `at`, a test's: a few minutes at most.
pub(crate) fn until_go(at: &Path) {
    let started = std::time::Instant::now();
    while !at.join("go").exists() && started.elapsed() < std::time::Duration::from_secs(180) {
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

/// Sends `notes` (change, id) of stack `name` to the agent, in `way`, to
/// the whole stack if `whole`, at `effort`: they wait as ready, together,
/// and go as soon as nothing stands in their way. What they wait on, if
/// they could not go at once.
pub fn send(db_path: &Path, name: &str, notes: &[(String, String)], way: Way, whole: bool, effort: Effort) -> Result<Option<String>> {
    at_the_gate();
    queue(db_path, name, notes, way, whole, effort)?;
    launch(db_path)?;
    let db = Db::load(db_path)?;
    let waiting = notes
        .iter()
        .any(|(c, i)| db.note(c, i).and_then(|n| n.queued.as_ref()).is_some_and(|q| !q.batched()));
    Ok(if waiting {
        Some(waits_on(&db, name, way).unwrap_or_else(|| "its turn".into()))
    } else {
        None
    })
}

/// Puts `notes` of stack `name` on the agent's queue, in `way`, together,
/// at `effort`: `launch` starts them when nothing stands in their way, at
/// that effort whenever that is. The editor launches elsewhere than where
/// it draws: making a worktree takes time. With `whole`, a patch's
/// comments are sent to the whole stack: the agent may change any patch,
/// add patches anywhere, reorder them.
pub fn queue(db_path: &Path, name: &str, notes: &[(String, String)], way: Way, whole: bool, effort: Effort) -> Result<()> {
    if notes.is_empty() {
        return err("no comment to send");
    }
    check_ahead(db_path, name);
    Db::edit(db_path, |db| {
        applies(db, name)?;
        enqueue(db, notes, way, whole, Stage::Ready, None, Some(effort), false)
    })
}

/// The way the work run `id` holds was asked in, if it is an ask's: the
/// way it goes back to the agent in.
pub fn way_of(db: &Db, id: &str) -> Option<Way> {
    light(db.run(id)?).map(|l| l.way)
}

/// Sends the work run `id` holds back to the agent, as what you wrote in
/// its review asks (`held::send_back`): another round, from that work's
/// stack, whose own work lands to review in its place; in the way the work
/// was asked, of the whole stack as far as its comments reach, unless `how`
/// says otherwise (the editor's dialog: a change, of the whole stack or
/// not); at `effort`, else at the effort the work was asked at. What was
/// sent, said. `launch` starts it.
pub fn send_back(
    db_path: &Path,
    name: &str,
    id: &str,
    marks: &std::collections::BTreeMap<String, bool>,
    effort: Option<Effort>,
    how: Option<(Way, bool)>,
) -> Result<String> {
    check_ahead(db_path, name);
    Db::edit(db_path, |db| {
        applies(db, name)?;
        let round = crate::held::send_back(db, id, marks)?;
        let (way, whole) = how.unwrap_or((round.way, round.whole));
        enqueue(db, &round.notes, way, whole, Stage::Ready, Some(id), effort.or(round.effort), false)?;
        let n = round.notes.len();
        let said = if whole && way == Way::Change { crate::db::WHOLE } else { way.said() };
        let mut out = format!("sent back to the agent ({said}): {n} comment{} to ask again", if n == 1 { "" } else { "s" });
        match round.dropped {
            0 => {}
            1 => out += "; 1 edit that went on from it asked again",
            k => out += &format!("; {k} edits that went on from it asked again"),
        }
        match round.stopped {
            0 => {}
            1 => out += "; the follow-up asked from its review stopped",
            k => out += &format!("; the {k} follow-ups asked from its review stopped"),
        }
        if !round.allowed.is_empty() {
            out += &format!("; {ALLOWED_SAID}: {}", round.allowed.join(", "));
        }
        out += &round.settled.said(round.way);
        Ok(out)
    })
}

/// Asks a follow-up that only reads, in `way` (an answer, a build and
/// test, a try push's selection), at `effort`, from the review of the work
/// run `id` holds, as what you wrote there asks (`held::follow_up`): that
/// work stays waiting for your review, its marks as they are, and what the
/// follow-up brings back comes into its review once it lands. It reads the
/// stack that work makes. Its comments are asked together, with the whole
/// stack in view when they are on more than one place, as an answer of
/// several is. What was asked, said. `launch` starts it.
pub fn follow_up(db_path: &Path, name: &str, id: &str, marks: &std::collections::BTreeMap<String, bool>, way: Way, effort: Effort) -> Result<String> {
    if !reads(way) && way != Way::Test {
        return err("a follow-up only reads: an answer, a build and test, or a try push's selection");
    }
    check_ahead(db_path, name);
    Db::edit(db_path, |db| {
        applies(db, name)?;
        let round = crate::held::follow_up(db, id, marks)?;
        let places: HashSet<Scope> = round.notes.iter().filter_map(|(c, i)| db.note(c, i)).map(scope_of).collect();
        let whole = way == Way::Answer && places.len() > 1;
        enqueue(db, &round.notes, way, whole, Stage::Ready, Some(id), Some(effort), true)?;
        let n = round.notes.len();
        let mut out = format!(
            "asked as a follow-up ({}): {n} comment{}; what the agent brings back comes into this review",
            way.said(),
            if n == 1 { "" } else { "s" }
        );
        if !round.allowed.is_empty() {
            out += &format!("; {ALLOWED_SAID}: {}", round.allowed.join(", "));
        }
        out += &round.settled.said(round.way);
        Ok(out)
    })
}

/// Whether stack `name` applies, checked with no lock held (it takes a
/// moment on a big repository): under it, the check is remembered.
fn check_ahead(db_path: &Path, name: &str) {
    if let Ok(db) = Db::load(db_path)
        && let Ok(s) = db.stack(name)
    {
        let _ = db.verify(s);
    }
}

/// Claude works on the stack as it applies: one that does not, or cannot
/// be checked, cannot be sent anything.
fn applies(db: &Db, name: &str) -> Result<()> {
    let s = db.stack(name)?.clone();
    db.verify(&s)
        .map_err(|e| Error(format!("cannot send: {}", e.0.trim_end_matches("; nothing changed"))))
}

/// Under the lock: `notes` on the agent's queue, at `stage`, in `way`,
/// together, in a batch of their own, to be asked at `effort`; from the
/// stack of landed work `on`, if a round sent back from it, or, `follow`, a
/// follow-up asked from its review, whose replies held there it is asked
/// on.
#[allow(clippy::too_many_arguments)]
fn enqueue(db: &mut Db, notes: &[(String, String)], way: Way, whole: bool, stage: Stage, on: Option<&str>, effort: Option<Effort>, follow: bool) -> Result<()> {
    if notes.is_empty() {
        return err("no comment to send");
    }
    if whole && way == Way::Test {
        return err("a build and test is not asked of the whole stack");
    }
    #[cfg(feature = "try-push")]
    if whole && way == Way::Try {
        return err("a try push is not asked of the whole stack: a comment on the stack pushes all of its patches");
    }
    let batch = format!("b{}", crate::db::unique());
    let busy = with_agent(db);
    let mut scope = None;
    for (change, id) in notes {
        let Some(n) = db.note(change, id).cloned() else {
            return err("no such comment");
        };
        if busy.contains_key(&(change.clone(), id.clone())) {
            return err("the agent is already working on it");
        }
        let broken = crate::refs::broken_in(&n);
        if !broken.is_empty() {
            return err(format!(
                "{} names no reference attached to the comment: attach one, or write it otherwise, before the agent is asked",
                broken.join(", ")
            ));
        }
        // A follow-up is asked on the replies the review of its work holds.
        if crate::held::reply_waiting(db, change, id).is_some_and(|(run, _)| !follow || on != Some(run.as_str())) {
            return err("the agent's reply to it waits for your review: review it first");
        }
        if way == Way::Test && !can_test(db, &n) {
            return err("only a comment on the stack or on a patch as a whole can be sent to build and test");
        }
        if way != Way::Answer && n.about.is_some() {
            return err("a comment on the history is asked about to answer only: it changes nothing");
        }
        #[cfg(feature = "try-push")]
        if way == Way::Try && !can_try(db, &n) {
            return err(TRY_OF_A_PATCH);
        }
        let reach = if whole { Scope::Stack } else { scope_of(&n) };
        match &scope {
            None => scope = Some(reach),
            Some(s) if *s != reach => return err("comments sent together must be on one patch, or all on the stack"),
            _ => {}
        }
        db.set_note(Note {
            queued: Some(Queued {
                way,
                batch: batch.clone(),
                stack: whole && !n.on_stack(),
                stage,
                on: on.map(String::from),
                was: None,
                effort,
                follow,
            }),
            failed: None,
            ..n
        })?;
    }
    Ok(())
}

/// Puts `notes` of stack `name` in its batch, to be asked later in `way`
/// (to the whole stack if `whole`), at `effort`: each waits there until
/// you send the batch. Nothing is started.
pub fn batch(db_path: &Path, name: &str, notes: &[(String, String)], way: Way, whole: bool, effort: Effort) -> Result<()> {
    if notes.is_empty() {
        return err("no comment to batch");
    }
    Db::edit(db_path, |db| {
        for at in notes {
            // Sent with the stack's batch: one of its comments.
            if !db.stack_notes(name).iter().any(|n| (&n.change, &n.id) == (&at.0, &at.1)) {
                return err(format!("that comment is not on {name}"));
            }
            enqueue(db, std::slice::from_ref(at), way, whole, Stage::Batch, None, Some(effort), false)?;
        }
        Ok(())
    })
}

/// Stack `name`'s batched comments asked in `way`, in the order of the
/// stack's comments: each with whether it goes to the whole stack.
pub fn batched(db: &Db, name: &str, way: Way) -> Vec<((String, String), bool)> {
    db.stack_notes(name)
        .into_iter()
        .filter_map(|n| {
            let q = n.queued.as_ref().filter(|q| q.batched() && q.way == way)?;
            Some(((n.change.clone(), n.id.clone()), q.stack))
        })
        .collect()
}

/// The effort stack `name`'s batch of what is asked in `way` was asked at:
/// the highest its comments were batched at, if any was.
pub fn batched_effort(db: &Db, name: &str, way: Way) -> Option<Effort> {
    db.stack_notes(name)
        .into_iter()
        .filter_map(|n| n.queued.as_ref().filter(|q| q.batched() && q.way == way)?.effort)
        .max()
}

/// Whether what is asked in `way` goes from the batch each comment in an
/// ask of its own, whatever is picked: a build and test, of its own place;
/// a try push, of the patches up to its own place.
pub fn each_on_its_own(way: Way) -> bool {
    match way {
        Way::Test => true,
        #[cfg(feature = "try-push")]
        Way::Try => true,
        _ => false,
    }
}

/// The comments of a batch that were not sent, each with why: they stay
/// comments, out of the batch, their ask failed with that reason.
pub type Refused = Vec<((String, String), String)>;

/// Sends stack `name`'s batch of what is asked in `way`: its comments turn
/// ready together, as one ask, or with `fan` (as builds and tests always
/// do, and try pushes, each of its own place) each as an ask of its own.
/// Together, comments from more than one place (the stack, or a patch) go
/// to the whole stack, as do those batched so. Each ask goes at `effort`;
/// else at the effort its comments were batched at, the highest of them for
/// an ask of several. `launch` starts them. How many were sent, and those
/// refused: a try push of a line, batched as an older stacked let it be.
pub fn release_batch(db_path: &Path, name: &str, way: Way, fan: bool, effort: Option<Effort>) -> Result<(usize, Refused)> {
    check_ahead(db_path, name);
    Db::edit(db_path, |db| {
        let all = batched(db, name, way);
        if all.is_empty() {
            return Ok((0, Vec::new()));
        }
        applies(db, name)?;
        let own = |db: &Db, (c, i): &(String, String)| db.note(c, i).and_then(|n| n.queued.as_ref()?.effort);
        // A try push of a line, batched as an older stacked let it be: out
        // of the batch as it goes, a comment still, its ask failed saying
        // why; the rest go.
        #[cfg_attr(not(feature = "try-push"), allow(unused_mut))]
        let mut refused: Refused = Vec::new();
        if fan || each_on_its_own(way) {
            for (at, whole) in &all {
                #[cfg(feature = "try-push")]
                if way == Way::Try
                    && let Some(n) = db.note(&at.0, &at.1).filter(|n| !can_try(db, n)).cloned()
                {
                    db.set_note(Note { queued: None, failed: Some(TRY_OF_A_PATCH.into()), ..n })?;
                    refused.push((at.clone(), TRY_OF_A_PATCH.into()));
                    continue;
                }
                let at_effort = effort.or(own(db, at));
                enqueue(db, std::slice::from_ref(at), way, *whole, Stage::Ready, None, at_effort, false)?;
            }
        } else {
            let places: HashSet<Scope> = all.iter().filter_map(|((c, i), _)| db.note(c, i)).map(scope_of).collect();
            let whole = places.len() > 1 || all.iter().any(|(_, whole)| *whole);
            let notes: Vec<(String, String)> = all.iter().map(|(at, _)| at.clone()).collect();
            let at_effort = effort.or(batched_effort(db, name, way));
            enqueue(db, &notes, way, whole, Stage::Ready, None, at_effort, false)?;
        }
        Ok((all.len() - refused.len(), refused))
    })
}

/// Puts comment (change, id) back on the agent's queue as it was, `q`:
/// taken off while it was edited, and left as it was. Not if it is on the
/// queue again already, or with the agent.
pub fn requeue(db_path: &Path, change: &str, id: &str, q: Queued) -> Result<()> {
    Db::edit(db_path, |db| {
        let Some(n) = db.note(change, id).cloned() else { return Ok(()) };
        if n.queued.is_some() || with_agent(db).contains_key(&(change.to_string(), id.to_string())) {
            return Ok(());
        }
        db.set_note(Note { queued: Some(q), ..n })
    })
}

/// Takes comment (change, id) back from the agent's queue, where it waits
/// its turn, unsent: the comment stays open, as it is. What it was queued
/// as, if it waited its turn; none if it did not, the agent having taken it
/// meanwhile, or it being in the batch.
pub fn take_back(db_path: &Path, change: &str, id: &str) -> Result<Option<Queued>> {
    Db::edit(db_path, |db| {
        let Some(n) = db.note(change, id).filter(|n| n.queued.as_ref().is_some_and(|q| !q.batched())).cloned() else {
            return Ok(None);
        };
        let was = n.queued.clone();
        db.set_note(Note { queued: None, ..n })?;
        Ok(was)
    })
}

/// How many edits of stack `name` wait their turn behind run `id`, a lint
/// job or a reconcile with HEAD waiting its turn, asked after it: each an
/// ask of comments together, as `claim` takes them in their turn (a round
/// sent back goes first, before it).
pub fn edits_after(db: &Db, name: &str, id: &str) -> usize {
    let key = format!("b{id}");
    let batches: HashSet<&str> = db
        .stack_notes(name)
        .into_iter()
        .filter_map(|n| n.queued.as_ref())
        .filter(|q| !q.batched() && q.way == Way::Change && q.on.is_none() && q.batch > key)
        .map(|q| q.batch.as_str())
        .collect();
    batches.len()
}

/// The other comments of stack `name` waiting their turn in the agent's
/// queue in batch `batch`, asked together with comment (change, id).
pub fn asked_with(db: &Db, name: &str, batch: &str, change: &str, id: &str) -> usize {
    db.stack_notes(name)
        .iter()
        .filter(|n| (n.change.as_str(), n.id.as_str()) != (change, id) && n.queued.as_ref().is_some_and(|q| !q.batched() && q.batch == batch))
        .count()
}

/// Takes `notes` off claude's queue, unsent.
pub fn unsend(db_path: &Path, notes: &[(String, String)]) -> Result<()> {
    Db::edit(db_path, |db| {
        for (c, i) in notes {
            if let Some(n) = db.note(c, i).cloned() {
                db.set_note(Note { queued: None, ..n })?;
            }
        }
        Ok(())
    })
}

thread_local! {
    /// How many launch locks this thread holds (`launch_lock`): the first
    /// holds the file's lock, those taken under it hold it as it is.
    static LAUNCHING: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// The launch lock, held until dropped (`launch_lock`): the file locked, by
/// the first this thread took; none by those taken under it. It stays with
/// the thread that took it.
pub struct LaunchLock {
    file: Option<std::fs::File>,
    _thread: std::marker::PhantomData<*const ()>,
}

impl Drop for LaunchLock {
    fn drop(&mut self) {
        if let Some(f) = &self.file {
            use std::os::fd::AsRawFd;
            unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_UN) };
        }
        LAUNCHING.with(|l| l.set(l.get().saturating_sub(1)));
    }
}

impl LaunchLock {
    /// The lock handed to `c`, a process to start that goes on with what it
    /// is held for, away from here: its file left open in that process, the
    /// lock is held until that process is done, as the kernel keeps a lock
    /// while any copy of its file is open. Whether it could be: not by a
    /// lock taken under another of this thread's, which holds no file.
    fn hand_to(&self, c: &mut Command) -> bool {
        use std::os::fd::AsRawFd;
        let Some(f) = &self.file else { return false };
        let fd = f.as_raw_fd();
        // Between fork and exec, the child alone keeps the file open across
        // its exec: fcntl allocates nothing and touches no lock.
        unsafe {
            std::os::unix::process::CommandExt::pre_exec(c, move || {
                let flags = libc::fcntl(fd, libc::F_GETFD);
                if flags < 0 || libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        true
    }

    /// Given up by this process with its lock left as it is: held by the
    /// processes it was handed to (`hand_to`) until they are done, else given
    /// back now, as no copy of its file is left open.
    fn leave(mut self) {
        self.file.take();
    }
}

/// Where the launch lock of the database at `db_path` is: a file of its
/// own beside it, `NAME.launch.lock`, by the database's real path, so that
/// every process finds the same one however it names the database.
pub fn launch_lock_path(db_path: &Path) -> PathBuf {
    let real = db_path.canonicalize().unwrap_or_else(|_| db_path.to_path_buf());
    let name = real.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    real.with_file_name(format!("{name}.launch.lock"))
}

/// The launch lock of the database at `db_path`, held until what this
/// gives is dropped: one thread at a time, of every process working on the
/// database, starts asks, from reading the queue to the spawn of their
/// agents, and does anything else to stacked's own worktrees, so that no
/// two touch them at once. A flock on its own file (`launch_lock_path`),
/// apart from the database's lock, so that the agents running meanwhile go
/// on writing their streams; the kernel gives it back however the process
/// ends. A thread that holds it takes it again as it is. A database kept in
/// memory, as the demo's, has no worktree: nothing is locked.
///
/// It comes before the database's lock: asked for by a thread holding that
/// one, it could wait for ever on a process that holds it and waits for the
/// database. That is a bug, a panic where debug assertions are on, as in
/// the tests, else an error.
pub fn launch_lock(db_path: &Path) -> Result<LaunchLock> {
    if crate::db::holds_lock() {
        let why = format!(
            "{}: the launch lock asked for under the database's lock, which is taken after it",
            db_path.display()
        );
        if cfg!(debug_assertions) {
            panic!("{why}");
        }
        return Err(Error(why));
    }
    let first = LAUNCHING.with(|l| {
        l.set(l.get() + 1);
        l.get() == 1
    });
    // Its count given back if it goes wrong, as it is dropped.
    let mut held = LaunchLock {
        file: None,
        _thread: std::marker::PhantomData,
    };
    #[cfg(feature = "demo")]
    if crate::memory::is_file(db_path) {
        return Ok(held);
    }
    if !first {
        return Ok(held);
    }
    let path = launch_lock_path(db_path);
    let io = |e: std::io::Error| Error(format!("{}: {e}", path.display()));
    let f = std::fs::File::options().create(true).append(true).open(&path).map_err(io)?;
    use std::os::fd::AsRawFd;
    // Found held, said so where the tests ask (`test_hold`), before it is
    // waited for.
    if let Some(hold) = test_hold() {
        if unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            held.file = Some(f);
            return Ok(held);
        }
        let _ = std::fs::write(hold.join(format!("waits-{}", std::process::id())), "");
    }
    loop {
        if unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX) } == 0 {
            break;
        }
        let e = std::io::Error::last_os_error();
        if e.kind() != std::io::ErrorKind::Interrupted {
            return Err(io(e));
        }
    }
    held.file = Some(f);
    Ok(held)
}

/// How many times a launch makes the pulls from Phabricator whose turn
/// came, then claims what waited behind them: a pull whose stack changed
/// as it was made is made again, and once these are spent waits for the
/// next launch.
const PULL_ROUNDS: usize = 4;

/// Starts every ask that may go now, and the lint job whose turn it is;
/// the runs started. Under the launch lock, every one of them: its
/// worktree taken and made ready, and its agent spawned. A pull from
/// Phabricator whose turn came is made then, the database's lock given
/// back (`review::make`), and what waited behind it claimed in turn.
pub fn launch(db_path: &Path) -> Result<Vec<String>> {
    if !Db::exists(db_path) {
        return Ok(vec![]);
    }
    let _launching = launch_lock(db_path)?;
    let mut started = Vec::new();
    let mut rounds = 0;
    loop {
        let (claimed, lints, pulls) = Db::edit(db_path, claim)?;
        for id in &claimed {
            if let Err(e) = start(db_path, id) {
                fail_start(db_path, id, &e.0)?;
            }
        }
        for id in &lints {
            if let Err(e) = crate::lint::start(db_path, id) {
                crate::lint::not_started(db_path, id, &e.0);
            }
        }
        started.extend(claimed.into_iter().chain(lints));
        if pulls.is_empty() || rounds == PULL_ROUNDS {
            break;
        }
        rounds += 1;
        #[cfg(feature = "moz-phab")]
        for (id, after) in pulls {
            crate::review::make(db_path, &id, after)?;
        }
    }
    Ok(started)
}

/// What `claim` goes through, in their turn: a batch of comments, by its
/// key; a lint job, a reconcile with HEAD or a pull from Phabricator
/// waiting, by its run, and its stack; a pull with whether it still reads
/// Phabricator.
enum Turn {
    Batch(String),
    Lint(String, String),
    Reconcile(String, String),
    #[cfg_attr(not(feature = "moz-phab"), allow(dead_code))]
    Pull(String, String, bool),
}

/// What `claim` comes to: the asks begun, the lint jobs running, and the
/// pulls from Phabricator whose turn came, each with the landed work it
/// goes on from, chained, to make once the lock is given back.
type Claimed = (Vec<String>, Vec<String>, Vec<(String, Option<String>)>);

/// Under the lock: the ready batches that may go now, each a run begun,
/// its comments taken off the queue; and the lint job whose turn it is, if
/// one's is, running from now on. A lint job takes its turn as a change
/// does, among the batches, the first asked first: one at a time of every
/// stack. So does a reconcile with HEAD, made at once at its turn, chained
/// as a change is; the changes after it wait until it is reviewed. So does
/// a pull from Phabricator, made once the lock is given back, the changes
/// after it waiting until it is made. Their run ids: the asks', the lint
/// job's, and the pulls'.
fn claim(db: &mut Db) -> Result<Claimed> {
    // The ready batches: stack, way, scope, comments, the landed work a
    // round sent back starts from, the run asked before it, the effort they
    // were sent at (the highest, should they differ), and whether they are
    // a follow-up asked from the review of that work.
    let mut order: Vec<String> = Vec::new();
    type Batch = (String, Way, Scope, Vec<Note>, Option<String>, Option<String>, Option<Effort>, bool);
    let mut batches: HashMap<String, Batch> = HashMap::new();
    // A try push of a line, queued as an older stacked let it be: refused
    // as it would go, a comment still, its ask failed saying why.
    #[cfg(feature = "try-push")]
    let mut refused: Vec<Note> = Vec::new();
    // Each stack's comments; then those of a round sent back on a patch no
    // stack holds yet, one the work it goes on from adds, with the stack of
    // that work.
    let stacks: Vec<(String, Vec<&Note>)> = db.stacks().into_iter().map(|s| (s.name.clone(), db.stack_notes(&s.name))).collect();
    let mut off: Vec<(String, Vec<&Note>)> = Vec::new();
    for (name, n) in queued_off_stacks(db) {
        match off.iter_mut().find(|(s, _)| *s == name) {
            Some((_, ns)) => ns.push(n),
            None => off.push((name, vec![n])),
        }
    }
    for (name, notes) in stacks.into_iter().chain(off) {
        for n in notes {
            let Some(q) = n.queued.as_ref().filter(|q| !q.batched()) else { continue };
            #[cfg(feature = "try-push")]
            if q.way == Way::Try && !can_try(db, n) {
                refused.push(n.clone());
                continue;
            }
            let entry = batches.entry(q.batch.clone()).or_insert_with(|| {
                order.push(q.batch.clone());
                (name.clone(), q.way, asked_scope(n), Vec::new(), q.on.clone(), q.was.clone(), q.effort, q.follow && q.on.is_some())
            });
            entry.3.push(n.clone());
            entry.6 = entry.6.max(q.effort);
        }
    }
    #[cfg(feature = "try-push")]
    for n in refused {
        db.set_note(Note { queued: None, failed: Some(TRY_OF_A_PATCH.into()), ..n })?;
    }
    // A round sent back first, as it goes on from the work just reviewed,
    // the stack still that work's; then the rest, oldest first, the lint
    // jobs waiting among them, each by when it was asked.
    let mut turns: Vec<Turn> = order.into_iter().map(Turn::Batch).collect();
    turns.extend(crate::lint::waiting(db).into_iter().map(|(id, name)| Turn::Lint(id, name)));
    turns.extend(crate::reconcile::waiting(db).into_iter().map(|(id, name)| Turn::Reconcile(id, name)));
    #[cfg(feature = "moz-phab")]
    turns.extend(crate::review::waiting(db).into_iter().map(|(id, name, reading)| Turn::Pull(id, name, reading)));
    // A batch's key is `b` and when it was made; a lint job's id, a
    // reconcile's, and a pull's after `pull-`, when it was asked.
    let key = |t: &Turn| match t {
        Turn::Batch(b) => (batches[b].4.is_none(), b.clone()),
        Turn::Lint(id, _) | Turn::Reconcile(id, _) | Turn::Pull(id, ..) => (true, turn_key(id)),
    };
    turns.sort_by_key(key);
    let mut started = Vec::new();
    let mut lints = Vec::new();
    #[cfg_attr(not(feature = "moz-phab"), allow(unused_mut))]
    let mut pulls = Vec::new();
    // Per stack: what is going, as the batches are started; whether a change
    // waits for its review; and, chained, what the next goes on from. Of
    // every stack: whether a build and test is going, and a lint job.
    let mut state: HashMap<String, Going> = HashMap::new();
    let mut test = testing(db).is_some();
    let mut linting = crate::lint::running(db);
    let going_of = |db: &Db, name: &str| {
        let going = asks_going(db, name);
        let lint_runs = crate::lint::pending(db, name).and_then(|r| r.lint.as_deref()).is_some_and(|j| j.step == crate::lint::Step::Running);
        Going {
            change: going.iter().any(|(_, i)| i.way == Way::Change) || lint_runs,
            answers: going.iter().filter(|(_, i)| reads(i.way)).count(),
            change_waits: false,
            reviewing: crate::held::change_waiting(db, name).is_some(),
            chain: chains(&agent_of(db, name)),
            tail: guessed_from(db, name),
            lint: false,
            reconciling: crate::reconcile::reviewing(db, name),
            pulling: false,
        }
    };
    for t in &turns {
        let b = match t {
            Turn::Batch(b) => b,
            // A lint job, as a change: once nothing changes the stack and no
            // change of it waits for review, nor, unchained, does an answer
            // run; and while no other lint job runs.
            Turn::Lint(id, name) => {
                let st = state.entry(name.clone()).or_insert_with(|| going_of(db, name));
                if !linting && !st.change && !st.lint && !st.reviewing && !st.reconciling && !st.pulling && (st.chain || st.answers == 0) {
                    crate::lint::claim(db, id)?;
                    linting = true;
                    st.change = true;
                    lints.push(id.clone());
                } else {
                    // The changes behind it wait for it, and, unchained,
                    // the answers behind them.
                    st.lint = true;
                    st.change_waits |= st.chain || !st.reviewing;
                }
                continue;
            }
            // A reconcile with HEAD, as a change: made at once, chained on
            // the change under review, if any. The changes behind it wait
            // until it is reviewed, made or not.
            Turn::Reconcile(id, name) => {
                let st = state.entry(name.clone()).or_insert_with(|| going_of(db, name));
                if !st.change && !st.lint && !st.reconciling && !st.pulling && (st.chain || (!st.reviewing && st.answers == 0)) {
                    let after = if st.chain { st.tail.clone() } else { None };
                    crate::reconcile::make(db, id, after)?;
                    st.reviewing = true;
                } else {
                    st.change_waits |= st.chain || !st.reviewing;
                }
                st.reconciling = true;
                continue;
            }
            // A pull from Phabricator, as a change: its turn come, made once
            // the lock is given back (`review::make`), chained on the change
            // under review, if any; not while it still reads Phabricator,
            // its first phase, though it holds its place in the queue from
            // when it is asked. What goes after it waits until it is made,
            // as behind a pull waiting its turn; then its change, if it
            // makes one, waits for your review, and the changes behind it go
            // on from it, chained, as from any change.
            #[cfg(feature = "moz-phab")]
            Turn::Pull(id, name, reading) => {
                let st = state.entry(name.clone()).or_insert_with(|| going_of(db, name));
                if !reading && !st.change && !st.lint && !st.reconciling && !st.pulling && (st.chain || (!st.reviewing && st.answers == 0)) {
                    let after = if st.chain { st.tail.clone() } else { None };
                    pulls.push((id.clone(), after));
                }
                st.change_waits |= st.chain || !st.reviewing;
                st.pulling = true;
                continue;
            }
            #[cfg(not(feature = "moz-phab"))]
            Turn::Pull(..) => continue,
        };
        let (name, way, scope, notes, on, was, effort, follow) = batches[b].clone();
        let st = state.entry(name.clone()).or_insert_with(|| going_of(db, &name));
        let go = match way {
            // A change has the stack to itself, from the stack as the change
            // before it left it, reviewed; chained, while that change waits
            // for review, from the stack it makes, as if accepted. One waiting
            // for what runs holds back the answers behind it, so it goes next.
            // Not before a lint job asked before it.
            Way::Change => {
                if !st.change && !st.lint && !st.reconciling && !st.pulling && (st.chain || (!st.reviewing && st.answers == 0)) {
                    st.change = true;
                    true
                } else {
                    st.change_waits |= st.chain || !st.reviewing;
                    false
                }
            }
            // Chained, an answer runs alongside the changes, on the stack as
            // it is: accepting a change asks it again. So does a try push's
            // selection, which reads only too.
            Way::Answer => {
                let go = st.chain || (!st.change && !st.change_waits);
                if go {
                    st.answers += 1;
                }
                go
            }
            #[cfg(feature = "try-push")]
            Way::Try => {
                let go = st.chain || (!st.change && !st.change_waits);
                if go {
                    st.answers += 1;
                }
                go
            }
            // A build and test, while no change runs on its stack, and no
            // other build and test anywhere: of the stack as it is.
            Way::Test => {
                let go = !st.change && !test;
                if go {
                    test = true;
                }
                go
            }
        };
        if !go {
            continue;
        }
        // Chained, a change goes on from the work under review.
        let after = if st.chain && on.is_none() && way == Way::Change { st.tail.clone() } else { None };
        let Some((from, _)) = db.head(&name) else { continue };
        let id = crate::db::unique();
        let shown = notes
            .iter()
            .enumerate()
            .map(|(i, n)| Shown {
                label: format!("c{}", i + 1),
                change: n.change.clone(),
                id: n.id.clone(),
            })
            .collect();
        // What this work may use for itself, as the work it goes on with
        // was given and its review allowed: the run asked before it, else
        // the work it was sent back from.
        let allowed = was.as_deref().or(on.as_deref()).map(|w| crate::refusals::carried(db, w)).unwrap_or_default();
        let info = Info {
            way,
            scope,
            from,
            notes: shown,
            base: String::new(),
            repo: String::new(),
            worktree: None,
            kept: false,
            patch: None,
            commits: vec![],
            prompt: String::new(),
            on,
            follow,
            after,
            batch: b.clone(),
            dropped: false,
            was,
            todo: None,
            effort,
            allowed,
            given: String::new(),
        };
        db.add_run(Run {
            id: id.clone(),
            what: way.name().into(),
            stack: name.clone(),
            state: RunState::Started,
            request: notes.first().map(|n| first_line(&n.text)).unwrap_or_default(),
            ask: Some(serde_json::to_value(&info).unwrap()),
            starter: Some(std::process::id()),
            ..Default::default()
        })?;
        for n in notes {
            db.set_note(Note { queued: None, ..n })?;
        }
        started.push(id);
    }
    Ok((started, lints, pulls))
}

fn first_line(text: &str) -> String {
    thread_turns(text)
        .last()
        .map(|t| t.body.lines().next().unwrap_or("").to_string())
        .unwrap_or_default()
}

/// A run that could not start: failed, and its comments with it. The
/// folder made for it, if one was (its rebase plan's, or its copy of the
/// task graph's), goes. One landed meanwhile (stopped as its worktree was
/// made) stays as it landed.
fn fail_start(db_path: &Path, id: &str, why: &str) -> Result<()> {
    Db::edit(db_path, |db| {
        let Some(r) = db.run(id).cloned().filter(|r| r.state == RunState::Started) else { return Ok(()) };
        if let Some(i) = info(&r) {
            mark_failed(db, &i, why)?;
            if let Some(dir) = &i.todo {
                drop_plan(Path::new(dir), &plans_busy(db, id));
            }
        }
        db.add_run(Run {
            state: RunState::Failed,
            summary: format!("could not start: {why}"),
            pid: None,
            ..r
        })?;
        Ok(())
    })
}

fn mark_failed(db: &mut Db, i: &Info, why: &str) -> Result<()> {
    for sh in &i.notes {
        if let Some(n) = db.note(&sh.change, &sh.id).cloned() {
            db.set_note(Note {
                failed: Some(why.into()),
                ..n
            })?;
        }
    }
    Ok(())
}

/// A comment's place in the stack, as the prompt says it.
fn place(db: &Db, s: &Stack, n: &Note) -> String {
    if let Some(rev) = n.about {
        return format!("on the stack's history, about @{rev} ({})", n.preview);
    }
    if n.on_stack() {
        return match &n.moved {
            Some(m) => format!("on the stack (moved there from {})", where_was(db, s, m)),
            None => "on the stack".into(),
        };
    }
    let at = s
        .patches
        .iter()
        .position(|id| db.patch(id).change == n.change)
        .map_or("a patch no longer in the stack".to_string(), |i| format!("patch {}", i + 1));
    if n.file == MSG_PATH {
        format!("{at}, its message, line `{}`", n.preview)
    } else if crate::diff::is_file_key(&n.line) {
        format!("{at}, the file {} as a whole", n.file)
    } else {
        format!("{at}, {} line `{}`", n.file, n.preview)
    }
}

fn where_was(db: &Db, s: &Stack, m: &Moved) -> String {
    let at = s
        .patches
        .iter()
        .position(|id| db.patch(id).change == m.change)
        .map_or("a patch".to_string(), |i| format!("patch {}", i + 1));
    match crate::diff::is_file_key(&m.line) {
        true => format!("{at}, the file {} as a whole", m.file),
        false => format!("{at}, {} line `{}`", m.file, m.preview),
    }
}

/// For comments on the stack's history: the history, oldest first, and
/// each record asked about in full; with the commands that show more.
/// Nothing for others.
fn history_part(db: &Db, i: &Info, s: &Stack, cmd: &str) -> String {
    let revs: Vec<usize> = i
        .notes
        .iter()
        .filter_map(|sh| db.note(&sh.change, &sh.id)?.about)
        .collect();
    if revs.is_empty() {
        return String::new();
    }
    let name = &s.name;
    let mut entries = crate::history::entries(db, name);
    entries.reverse();
    // The latest of a long history.
    let skip = entries.len().saturating_sub(300);
    let mut p = format!(
        "\nThe stack's history, oldest first: each record that changed it (an edit, an undo, a redo, a \
         select), by its rev, and what it changed in short (+ patches added, − dropped, ~ given a new \
         version, ↕ moved){}:\n",
        if skip > 0 { format!(" ({skip} older ones left out)") } else { String::new() }
    );
    for e in &entries[skip..] {
        p.push_str(&format!("  {} {}\n", crate::db::local_time(e.time), e.text));
    }
    p.push_str(
        "\nA record asked about comes with its stack diff: the stack after it, bottom first, each patch \
         by its place and marked (+ added, ~ a new version, what changed in its mail indented under it, \
         ↕ moved, from where it `was`; unmarked, as it was), and each patch dropped, marked −.\n",
    );
    for rev in revs {
        p.push_str(&format!("\nWhat @{rev} was:\n"));
        for l in crate::history::about(db, name, rev).into_iter().take(400) {
            p.push_str(&format!("  {l}\n"));
        }
    }
    p.push_str(&format!(
        "\nMore of it: `{cmd} log {name}` (every edit, and the stack each was made from), \
         `{cmd} tree {name}`, `{cmd} export {name} --rev @N` (the stack as it was at @N, as mail), \
         `{cmd} versions {name} PATCH` and `{cmd} interdiff {name} PATCH --from ID` (a patch's \
         versions, and what changed between them), `{cmd} comments {name}`. PATCH is a position \
         from 1, or an id. Answer about the history as asked; the comments you open go on lines \
         of the stack as it is now.\n"
    ));
    p
}

/// What stacked always gives an ask to answer only, `cmd` being stacked on
/// its database: it reads the code and the stack, and changes nothing.
/// Claude's tools that read (`GIVEN`), git's commands that only read
/// (`claude::GIT_READS`), the commands that only read (`claude::ONLY_READ`),
/// the options of theirs that write a file or run a program refused
/// (`refused_of`), and stacked's own that read the stack. What else it may
/// run is the settings' (`agent.answer_tools`: curl and jq unless set, to
/// read the web), each command of a pipe needing its own.
fn answer_given(cmd: &str) -> String {
    let mut tools: Vec<String> = GIVEN.map(String::from).to_vec();
    tools.extend(crate::claude::GIT_READS.iter().map(|s| format!("Bash(git {s} *)")));
    tools.extend(crate::claude::ONLY_READ.iter().map(|c| format!("Bash({c} *)")));
    tools.extend(["export", "log", "tree", "versions", "interdiff", "show", "comments"].map(|s| format!("Bash({cmd} {s} *)")));
    tools.join(",")
}

/// What stacked refuses an ask in `way`, whatever its lists let it run, a
/// rule each, as claude takes them (`--disallowedTools`): to every ask but
/// a build and test, which runs what the project needs, what would make the
/// commands it may run run another program or delete files
/// (`claude::refused_tools`); to an ask that only reads, an answer or a try
/// push's selection, what would make those it is given write a file or run
/// a program, too (`claude::read_refused`).
pub(crate) fn refused_of(way: Way) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    if way != Way::Test {
        out.extend(crate::claude::refused_tools().split(',').map(String::from));
    }
    if reads(way) {
        out.extend(crate::claude::read_refused().split(',').map(String::from));
    }
    out
}

/// What stacked always gives an ask that changes the stack, `cmd` being
/// stacked on its database: git's history commands and the commands that
/// read (`allowed_tools`), none that reaches the network, and stacked's
/// own, to check that the patches above still apply, and to read the
/// stack; then the checkout's linter, `linter`, if it has one that a narrow
/// rule lets run (`Linter::rule`), to check the files the change changes.
fn change_given(cmd: &str, linter: &Linter) -> String {
    let mut out = format!("{},Bash({cmd} fits *),Bash({cmd} export *)", allowed_tools());
    if let Some(rule) = linter.rule() {
        out.push(',');
        out.push_str(&rule);
    }
    out
}

/// What stacked always gives the asks, said, a line each, as the setup
/// shows it beside what the settings add: what it is, whether answer-only
/// asks have it (`answer_given`), and whether asks that change the stack
/// do (`change_given`). The last, the checkout's linter, the setup says of
/// the checkout it is run in, naming its command (`setup::linter_said`).
pub const GIVEN_SAID: [(&str, bool, bool); 7] = [
    ("Read, Grep, Glob, LS, and git show, log and diff", true, true),
    ("commands that only read: grep, rg, cat, ls, diff…, and git blame, ls-files, rev-parse…", true, false),
    ("commands that read: grep, rg, find, cat, sed, sort…", false, true),
    ("git's other history commands (status, add, commit, reset, rebase -i…), and git config to read", false, true),
    ("stacked's commands that read the stack: export, show, log, tree, versions, interdiff, comments", true, false),
    ("stacked's fits and export; none of these reaches the network", false, true),
    (LINTER_GIVEN, false, true),
];

/// What the setup says of the linter that asks that change the stack are
/// given, the checkout's, where it does not know the checkout.
pub const LINTER_GIVEN: &str = "the checkout's linter, when it has one, to run on the files a change changes";

/// What an ask that changes the stack is told of the checkout's linter,
/// `command`, when it is given it: that it may run it on the files it
/// changes.
fn linter_told(command: &str) -> String {
    format!(" This checkout's linter is `{command}`: you may run it on the files you change, to check them.")
}

/// What an ask in `way` may use, `cmd` being stacked on its database, as
/// claude takes it (`--allowedTools`): what stacked gives that kind of ask
/// (for a change, with the rule that lets it run `linter`, the checkout's
/// linter), then what the settings `agent` add for that kind
/// (`change_tools`, `answer_tools`, `try_tools`), then what reviews allowed
/// it from now on, in a list of its own (`change_accepted`,
/// `answer_accepted`, `test_accepted`, `try_accepted`), then what they add
/// for every ask (`tools`). A build and test runs what the project needs:
/// Bash at large, and every ask's. A try push's selection is given what an
/// answer is: it reads only, and is not given the linter.
pub fn tools_for(way: Way, cmd: &str, agent: &crate::config::Agent, linter: &Linter) -> String {
    let (mut tools, own): (String, &[String]) = match way {
        Way::Change => (change_given(cmd, linter), &agent.change_tools),
        Way::Answer => (answer_given(cmd), &agent.answer_tools),
        Way::Test => (format!("Bash,{}", allowed_tools()), &[]),
        #[cfg(feature = "try-push")]
        Way::Try => (answer_given(cmd), &agent.try_tools),
    };
    // Its answer given as structured output: with the tool claude gives it
    // with.
    if agent.schema {
        tools.push_str(",StructuredOutput");
    }
    for t in own.iter().chain(agent.accepted(way)).chain(&agent.tools) {
        tools.push(',');
        tools.push_str(t);
    }
    tools
}

/// `tools`, as claude takes them, with `more` after them, those it has
/// not already.
fn with_more(tools: String, more: &[String]) -> String {
    let had: Vec<String> = tools.split(',').map(String::from).collect();
    let mut out = tools;
    for t in more.iter().filter(|t| !had.contains(t)) {
        out.push(',');
        out.push_str(t);
    }
    out
}

/// What the stream and the status say of what an ask may use for its work
/// alone, before the rules.
pub const ALLOWED_SAID: &str = "allowed for this work";

/// What an ask to answer only is told of the web: that it may read it with
/// curl, when its tools let it run curl; and with the tools of the MCP
/// servers it is let use.
const READ_THE_WEB: &str = "You may read the web with `curl` to answer (a bug on Bugzilla, a \
revision on Phabricator, a page of documentation), what it reads through `jq` if need be, and use \
the tools of MCP servers you are let use: only to read, never to send anything of this repository \
anywhere.";
const READ_WITH_TOOLS: &str = "You may use the tools of MCP servers you are let use to answer (a bug \
on Bugzilla, a revision on Phabricator): only to read, never to send anything of this repository \
anywhere.";

/// A thread as a conversation, for claude. A reviewer's comment edited on
/// Phabricator is said so: the turn of its edit by what it edits, the turn
/// it replaces as redacted by the edit below it. One read again with the
/// suggested edit Phabricator gives with it is said for what it is, no
/// edit: the turn it replaces as redacted by that comment, read with it.
fn transcript(text: &str) -> String {
    let turns = thread_turns(text);
    turns
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let who = match t.role {
                Role::You => "The user".to_string(),
                Role::Agent => "You (the agent, earlier)".to_string(),
                Role::Reviewer => {
                    let name = t.tag.as_deref().unwrap_or("someone");
                    let mut who = format!("{name} (a reviewer, on Phabricator)");
                    if t.edit.is_some() || t.redacted {
                        who += &format!(", {}", t.named(i));
                    }
                    if t.redacted {
                        who += &match crate::thread::replacing(&turns, i).is_some_and(|n| n.anew) {
                            true => " (redacted: stacked had not read the suggested edit Phabricator gives with this comment; the turn below, the same comment read with it, replaces it)".to_string(),
                            false => format!(" (redacted: {name} edited it on Phabricator, and the edit below replaces it)"),
                        };
                    }
                    who
                }
            };
            // What it points at, said rather than as its block.
            let (body, pointers) = crate::refs::split(&t.body);
            let pointers: Vec<String> = pointers
                .iter()
                .map(|p| {
                    let name = p.n.map_or(String::new(), |k| format!("r{k}, "));
                    match &p.to {
                        crate::refs::Pointer::Lines(r) => format!("{name}{}", r.said()),
                        crate::refs::Pointer::Comment { change, id } => format!("{name}the comment [{}]", crate::db::short_id(change, id)),
                        crate::refs::Pointer::Link(url) => format!("{name}the web link {url}"),
                    }
                })
                .collect();
            match pointers.is_empty() {
                true => format!("{who}: {body}"),
                false => format!("{who}: {body}\n(pointing at: {})", pointers.join("; ")),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The most lines of a file attached to a comment the agent is shown.
const ATTACHED_LINES: usize = 200;

/// What is attached to comment `n`, as the agent is told it: each reference
/// by its name, a comment as it stands, open or closed, lines as stack `s`
/// has them, a web link as it is, not to be fetched. Nothing if nothing
/// is.
fn attached_part(db: &Db, s: &Stack, n: &Note) -> String {
    if n.attached.is_empty() {
        return String::new();
    }
    let mut out = "Attached to it, as what it says names them (@r1; @r{1,2} several; @r* all), for what they say; \
                   they are not asked of you:\n"
        .to_string();
    for a in &n.attached {
        match &a.to {
            crate::refs::To::Comment { change, id } => match crate::refs::find_comment(db, &s.name, change, id) {
                Some((c, open)) => {
                    let state = if open { "open" } else { "closed" };
                    out += &format!("  {} [{}] {} ({state}):\n", a.name(), crate::db::short_id(&c.change, &c.id), place(db, s, &c));
                    for l in transcript(&c.text).lines() {
                        out += &format!("    {l}\n");
                    }
                }
                None => out += &format!("  {}: a comment that is gone\n", a.name()),
            },
            crate::refs::To::Lines(l) => {
                let Some(r) = crate::refs::Ref::parse(l) else { continue };
                out += &format!("  {}: {}:\n", a.name(), r.said());
                let Some(text) = lines_text(db, s, &r) else {
                    out += "    (the file is not there)\n";
                    continue;
                };
                let all: Vec<&str> = text.lines().collect();
                let wanted: Vec<usize> = match r.whole() {
                    true => (1..=all.len()).collect(),
                    false => r.ranges.iter().flat_map(|&(x, y)| x..=y.min(all.len())).collect(),
                };
                for &k in wanted.iter().take(ATTACHED_LINES) {
                    out += &format!("    {k:>5} │ {}\n", all[k - 1]);
                }
                if wanted.len() > ATTACHED_LINES {
                    out += &format!("    … {} lines more\n", wanted.len() - ATTACHED_LINES);
                }
            }
            crate::refs::To::Link(url) => out += &format!("  {}: a web link, {url} ({LINK_UNREAD})\n", a.name()),
        }
    }
    out
}

/// What the agent is told of a web link attached to a comment.
const LINK_UNREAD: &str = "you need not fetch it, and may not be able to: your tools may not reach the network";

/// The file reference `r` is of, as it says: as a patch of stack `s`
/// leaves it, as the stack's base has it, or as the whole stack leaves it.
fn lines_text(db: &Db, s: &Stack, r: &crate::refs::Ref) -> Option<String> {
    use crate::refs::At;
    match r.at {
        At::Patch(n) if (1..=s.patches.len()).contains(&n) => crate::ops::file_at(db, s, n - 1, &r.file).ok().flatten(),
        At::Patch(_) => None,
        At::Top if !s.patches.is_empty() => crate::ops::file_at(db, s, s.patches.len() - 1, &r.file).ok().flatten(),
        _ => {
            let (repo, base) = crate::ops::base_of(s).ok()?;
            git::git(&repo, &["show", &format!("{base}:{}", r.file)]).ok()
        }
    }
}

/// What the agent is told of the comments it opens: one issue each, as
/// many on one line as it has issues there.
const ONE_ISSUE: &str = "Each comment you open is about one issue: a line with two issues gets two \
comments, each on that line.";

const KEEP_TITLES: &str = "Keep each commit's title unless asked to change it: a patch is \
followed across its versions by its title. When you are done, everything must be committed \
and HEAD must be the top. You cannot build the project or run its tests here; reason from the \
code. Do not push or fetch, nor stash. Nothing you run here reaches the network: git's settings \
are not yours to set (`git -c`, `git config` but to read them), and what would run another \
program is refused (`git rebase --exec`, `find -exec`, `rg --pre`).";

/// The most lines of a dropped change's stack diff a prompt shows.
const BEFORE_LINES: usize = 300;

/// What ask `i`, asked again, is told of what run `was` brought back before
/// it was dropped, the stack having changed: its replies and the comments it
/// opened; for a change, what it changed, its stack diff from the stack it
/// went on from, and how to read the stack it made.
fn before(db: &Db, i: &Info, was: &str, name: &str, cmd: &str) -> String {
    // Nothing brought back: nothing to tell.
    if db.run(was).and_then(|r| r.held.as_ref()).is_none_or(Held::is_empty) {
        return String::new();
    }
    let out = match i.way {
        Way::Answer => " This was asked before, on the stack as it was before an edit of it was \
                        accepted; what was answered then is below. Answer again on the stack as it is \
                        now: keep what still holds, and say what the edit changes."
            .to_string(),
        #[cfg(feature = "try-push")]
        Way::Try => " This was asked before, on the stack as it was before an edit of it was \
                     accepted; what was selected then is below. Select the tasks again on the stack \
                     as it is now: keep what still holds, and say what the edit changes."
            .to_string(),
        _ => format!(
            " This was asked before, going on from an edit that was then dropped; what was \
             made then is below, and `{cmd} export {name} --work {was}` prints the stack it \
             made. Make it again on the stack as it is now, keeping what still applies."
        ),
    };
    out + &brought(db, i, was, name, cmd, true)
}

/// What run `was` brought back, for ask `i` to go on from or make again: its
/// replies (`replies`: unless their threads hold them already), the comments
/// it opened, and for a change, what it changed, as a stack diff from the
/// stack it went on from.
fn brought(db: &Db, i: &Info, was: &str, name: &str, cmd: &str, replies: bool) -> String {
    let Some((r, h)) = db.run(was).and_then(|r| Some((r, r.held.as_ref()?))) else { return String::new() };
    let mut out = String::new();
    for sh in i.notes.iter().filter(|_| replies) {
        if let Some(rp) = h.replies.iter().find(|rp| rp.change == sh.change && rp.id == sh.id) {
            out += &format!("\n\nWhat was answered then to {}:\n{}", sh.label, rp.text.trim_end());
        }
    }
    for n in &h.opened {
        out += &format!("\n\nA comment opened then, on {}: {}", n.preview, crate::thread::thread_turns(&n.text).last().map_or("", |t| t.body.as_str()));
    }
    // A try push's: the tasks selected then, as the review marked them, and
    // what the user answered to why each was.
    #[cfg(feature = "try-push")]
    {
        let marks = db.review(was).map(|v| v.marks.clone()).unwrap_or_default();
        let mark = |k: usize| marks.get(&crate::held::Part::Task(k).key()).copied();
        out += &crate::try_push::chosen_said(&h.tasks, h.rebuild, &mark);
    }
    // A change: what it changed, from the stack it went on from.
    if let (Some(_), Some(old)) = (&h.stack, info(r))
        && let (Ok(start), Ok(made)) = (started_from(db, &old), work_stack(db, was))
    {
        let lines: Vec<String> = crate::history::stacks_diff(db, Some(&start), Some(&made), true).iter().map(crate::history::Diffed::text).collect();
        out += "\n\nWhat was changed then, as a stack diff:\n";
        out += &lines.iter().take(BEFORE_LINES).cloned().collect::<Vec<_>>().join("\n");
        if lines.len() > BEFORE_LINES {
            out += &format!("\n… and {} lines more: `{cmd} export {name} --work {was}` has it all", lines.len() - BEFORE_LINES);
        }
    }
    out
}

/// The prompt for an ask: what it may do, the stack, the comments, and
/// what to answer with. An answer is told it may read the web with curl if
/// `curl` says it is let run curl; a change, that it may run the
/// checkout's linter, `linter`, its command, if it is given it.
#[allow(clippy::too_many_arguments)]
fn prompt(db: &Db, i: &Info, s: &Stack, cmd: &str, run: &str, structured: bool, curl: bool, linter: Option<&str>) -> String {
    let web = if curl { READ_THE_WEB } else { READ_WITH_TOOLS };
    let lint = linter.map(linter_told).unwrap_or_default();
    // How it answers: as structured output, which claude checks against
    // what this asks for (`answer_schema`); else as JSON it ends with.
    let (end, only, said) = match structured {
        true => (
            "Give your answer as the structured output asked of you",
            "Give your answer as the structured output asked of you",
            "in your answer",
        ),
        false => ("End your reply with a JSON object", "Output ONLY a JSON object", "in the JSON you end with"),
    };
    let n = s.patches.len();
    let k = i.patch.as_ref().map(|(_, k)| *k);
    // The stack it started from: at its rev; a guess's, or a round's sent
    // back, as the work it goes on from makes it.
    let at = i.after.as_ref().or(i.on.as_ref()).map_or(format!("--rev {}", i.from), |w| format!("--work {w}"));
    let export = format!("{cmd} export {} {at}", s.name);
    let mut p = match (i.way, &i.scope) {
        (Way::Change, Scope::Stack) => format!(
            "You are changing a stack of patches, as the comments below ask. The current \
             directory is a git worktree made for you: the stack is the commits {base}..HEAD, \
             each commit a patch, listed below bottom first. You may change any of them: \
             edit files and `git commit --amend` for HEAD, `git reset` and `git commit` to \
             split, a new commit where a new patch belongs. To change, reorder, squash or \
             drop commits below HEAD: (1) write the plan, with your file-editing tools, to \
             the rebase todo file named at the end: a `pick`, `edit`, `squash`, `fixup` or \
             `drop` line (`pick SHA TITLE`) for every commit of {base}..HEAD, bottom first, \
             a commit left out being dropped, no `exec` line (a plan that runs a command is \
             refused); (2) run a plain `git rebase -i {base}`, which \
             takes that plan; (3) at each `edit` stop, change the files and `git commit \
             --amend` (`-m` or `-F` for a new message: `reword` keeps the old one), then \
             `git rebase --continue`. No editor opens, and none is to be set: a command \
             that starts by setting a variable (`GIT_SEQUENCE_EDITOR=…`) is refused. A \
             commit's sha changes when it or a commit below it does: after a rebase, find \
             the commits by their titles. If a rebase conflicts and you cannot resolve it, \
             `git rebase --abort`, and say so. Do not touch \
             commits below {base}. To rebase the stack onto another commit, write the plan \
             of its picks, then `git rebase -i --onto COMMIT {base}`, and say onto what: \
             \"base\": COMMIT's full sha, {said}. \
             {KEEP_TITLES}{lint}",
            base = i.base
        ),
        (Way::Change, Scope::Patch(_)) => format!(
            "You are changing one patch of a stack, patch {k} of {n}, as the comments below \
             ask. The current directory is a git worktree made for you: the commits \
             {base}..HEAD are the stack up to that patch, the patches below it and the patch \
             itself on top, at HEAD. You may change only this patch: amend HEAD, or split it \
             into several commits in its place (its title stays on the first). Do not touch \
             the commits below it. The patches above it are not here: you may read them, not \
             change them: `{export}` prints the whole stack as mail. Before you finish, run \
             `{cmd} fits {run}`: it says whether the patches above still apply on your change. \
             If they would not, or if what is asked needs another patch to change too, change \
             nothing: `git reset --hard {orig}` puts the patch back as it was, and answer \
             with \"promote\" saying why, so the comment goes to the stack. {KEEP_TITLES}{lint}",
            k = k.map_or(0, |k| k + 1),
            base = i.base,
            orig = i.commits.last().map_or("HEAD", |c| c.0.as_str())
        ),
        (Way::Answer, Scope::Stack) => format!(
            "You are answering comments on a stack of patches. The current directory is a git \
             worktree made for you: the stack is the commits {base}..HEAD, each commit a \
             patch, listed below bottom first; `git show` each to read it. Do NOT modify \
             anything. You cannot build the project or run its tests; reason from the code. \
             {web} Besides your answers, you may open comments of your own on lines \
             of any patch, as a careful reviewer would, without repeating what is already said. {ONE_ISSUE}",
            base = i.base
        ),
        (Way::Answer, Scope::Patch(_)) => format!(
            "You are answering comments on one patch of a stack, patch {k} of {n}. The current \
             directory is a git worktree made for you: the commits {base}..HEAD are the stack \
             up to that patch, the patch itself at HEAD; `git show` to read them; `{export}` \
             prints the whole stack, the patches above included. Do NOT modify anything. You \
             cannot build the project or run its tests; reason from the code. {web} \
             Besides your answers, you may open comments of your own on lines of this patch, as \
             a careful reviewer would, without repeating what is already said. {ONE_ISSUE}",
            k = k.map_or(0, |k| k + 1),
            base = i.base
        ),
        (Way::Test, scope) => {
            let what = match scope {
                Scope::Stack => format!("the stack's patches: the commits {}..HEAD, each commit a patch", i.base),
                Scope::Patch(_) => format!(
                    "patch {} with the patches below it: the commits {}..HEAD, the patch at HEAD; `{export}` \
                     prints the whole stack, the patches above included",
                    k.map_or(0, |k| k + 1),
                    i.base
                ),
            };
            format!(
                "You are building and testing {what}. The current directory is a git worktree of \
                 the stack's own, for its builds: build there, by your own rules for this \
                 repository, and run the tests that matter for what the patches change. It is \
                 kept for the next build, with what you build in it: build on what is already \
                 there. Do not change the files git tracks, nor commit: you change nothing in the stack, \
                 and what you find is your answer. Never use the user's current worktree, nor \
                 change a branch of the user's. Do not push."
            )
        }
        // The CI tasks of a try push, selected as the comments ask: pushed
        // once the user accepts them, by stacked.
        #[cfg(feature = "try-push")]
        (Way::Try, scope) => {
            let what = match scope {
                Scope::Stack => format!("the stack's patches, listed below bottom first, which apply to commit {}", i.base),
                Scope::Patch(_) => format!(
                    "patch {k} of {n} with the patches below it, listed below with the others, bottom first, \
                     which apply to commit {}: the patches above it are not pushed",
                    i.base,
                    k = k.map_or(0, |k| k + 1)
                ),
            };
            let here = format!(
                "The current directory is a git worktree made for you: the commits {}..HEAD are \
                 what is pushed, each commit a patch; `git show` to read them; `{export}` prints \
                 the whole stack.",
                i.base
            );
            // Let run curl: where it downloads the task graph, in its folder
            // of its own.
            let graph = i.todo.as_deref().filter(|_| curl).map(|d| Path::new(d).join(crate::try_push::GRAPH));
            crate::try_push::prompt(&what, &here, web, graph.as_deref())
        }
    };
    // A follow-up: what it brought back waits for the user's review as it
    // is, the stack it makes in the worktree. What it changed, and the
    // comments it opened, shown; its replies are in their threads below, as
    // they would be.
    if let Some(on) = i.on.as_ref().filter(|_| i.follow) {
        let changed = db.run(on).and_then(|r| r.held.as_ref()).is_some_and(|h| h.stack.is_some());
        let made = if changed {
            format!(", which the current directory holds as that work leaves it: `{cmd} export {} --work {on}` prints it", s.name)
        } else {
            String::new()
        };
        p.push_str(&match i.way {
            Way::Test => format!(
                " This is a follow-up: the user reviews the work you brought back, which waits for their review as it is, \
                 and asks you, in the comments below, to build and test the stack that work makes{made}. The threads \
                 below hold your replies, and what the user answered to them."
            ),
            #[cfg(feature = "try-push")]
            Way::Try => format!(
                " This is a follow-up: the user reviews the work you brought back, which waits for their review as it is, \
                 and asks you, in the comments below, to select the CI tasks of a try push of the stack that work \
                 makes{made}. The threads below hold your replies, and what the user answered to them."
            ),
            _ => format!(
                " This is a follow-up: the user reviews the work you brought back, which waits for their review as it is, \
                 and asks about it in the comments below, on the stack that work makes{made}. The threads below hold \
                 your replies, and what the user answered to them."
            ),
        });
        p.push_str(&brought(db, i, on, &s.name, cmd, false));
    }
    // A pull from Phabricator's change sent back: the worktree holds the
    // stack it makes, the conflicts it marked in it, which the comment below
    // hands over, as the lint job hands its fixes over.
    let from_pull = i.on.as_ref().filter(|_| !i.follow).and_then(|on| db.run(on)).is_some_and(crate::held::pulled);
    if let Some(on) = i.on.as_ref().filter(|_| from_pull) {
        p.push_str(&format!(
            " This starts from the stack a pull from Phabricator makes, which the worktree holds: the pull took the latest diffs \
             of revisions as new versions of their patches, or as new patches, and made again on them the patches that no longer \
             applied, their conflicts marked in their files, as the comment below says; `{cmd} export {} --work {on}` prints that \
             stack. Rework those patches as it asks.",
            s.name
        ));
    }
    // Another round: what it brought back was sent back. What it changed
    // then, and the comments it opened, shown, as to a round asked again;
    // its replies are in their threads.
    if let Some(on) = i.on.as_ref().filter(|_| !i.follow && !from_pull) {
        let changed = db.run(on).and_then(|r| r.held.as_ref()).is_some_and(|h| h.stack.is_some());
        p.push_str(&match i.way {
            Way::Change if changed => format!(
                " This is another round: the user reviewed the change you made, which the worktree holds \
                 as you left it, and sent it back with the comments below: change it as they ask, from \
                 there. What you changed then is below, and `{cmd} export {} --work {on}` prints the \
                 stack you made.",
                s.name
            ),
            #[cfg(feature = "try-push")]
            Way::Try => " This is another round: the user reviewed the tasks you selected, and asks again in \
                         the comments below: select them again as they ask, keeping those that still hold."
                .to_string(),
            _ => " This is another round: the user read what you answered, and asks again in the comments below."
                .to_string(),
        });
        p.push_str(&brought(db, i, on, &s.name, cmd, false));
    }
    // Asked again, the work landed before dropped: shown, to reuse.
    if let Some(was) = &i.was {
        p.push_str(&before(db, i, was, &s.name, cmd));
    }
    p.push_str("\n\nThe stack, bottom first:\n");
    for (at, id) in s.patches.iter().enumerate() {
        let mark = if Some(at) == k { " ← this patch" } else { "" };
        p.push_str(&format!("  {} {}{mark}\n", at + 1, db.patch(id).title()));
    }
    p.push_str("\nThe comments, each a thread, the last word the user's or a reviewer's:\n");
    // A follow-up's, with the replies the review of its work holds in them,
    // and the user's answers to those.
    let review = i.on.as_deref().filter(|_| i.follow).and_then(|on| {
        let h = db.run(on)?.held.clone()?;
        Some((h, db.review(on).map(|v| v.taken.clone()).unwrap_or_default()))
    });
    for sh in &i.notes {
        let Some(n) = db.note(&sh.change, &sh.id) else { continue };
        let text = match &review {
            Some((h, taken)) => h.thread_but(&n.change, &n.id, &n.text, taken),
            None => n.text.clone(),
        };
        p.push_str(&format!("\n[{}] {}:\n{}\n", sh.label, place(db, s, n), transcript(&text)));
        p.push_str(&attached_part(db, s, n));
        p.push_str(&format!("(The references you attach to your reply to it are numbered from r{} on.)\n", crate::refs::next_of(n)));
    }
    p.push_str(&history_part(db, i, s, cmd));
    p.push_str(&match i.way {
        Way::Change => match i.scope {
            Scope::Stack => format!(
                "\n{end}: {{\"replies\": {{label: your reply to that comment, saying what you \
                 changed}}}}, labels as above ([c1] is \"c1\"); with \"base\": the commit's sha, \
                 when you rebased the stack onto another."
            ),
            Scope::Patch(_) => format!(
                "\n{end}: {{\"replies\": {{label: your reply to that comment, saying what you \
                 changed}}}}, labels as above ([c1] is \"c1\"); with \"promote\": why, when the \
                 comments need more than this patch."
            ),
        },
        Way::Answer => format!(
            "\n{only}: {{\"replies\": {{label: your answer}}, \"comments\": [{{\"patch\": its \
             number, \"file\": the path as the diff names it, \"side\": \"+\" for an added line, \
             \"-\" for a removed one, \" \" for context, \"line\": that line's exact text without \
             its leading +/-/space, or \"\" for the file as a whole, \"text\": your comment, one \
             issue}}]}}, the comments optional, as many on one line as it has issues."
        ),
        Way::Test => format!(
            "\n{end}: {{\"replies\": {{label: what you found, with the commands run}}, \
             \"patches\": {{its number: what building and testing it found}}}}."
        ),
        #[cfg(feature = "try-push")]
        Way::Try => crate::try_push::answer_said(end),
    });
    p.push_str(REFS);
    p.push_str(&not_allowed(said));
    // What it was refused before and may use now, for this work.
    if !i.allowed.is_empty() {
        p.push_str(&format!(" You may use, for this work, what you were refused before: {}.", crate::refusals::listed(&i.allowed)));
    }
    p
}

/// What an ask is told of a tool it is not let use: that nothing it would
/// have reached said no, so that it says not allowed, not that a service
/// refused; that it is not to stop there, but to work around it; and to
/// say, `said` saying where (`in your answer`), for each tool refused, why
/// it needed it and how it worked around it, for the user to review.
fn not_allowed(said: &str) -> String {
    format!(
        " A tool you are not let use is refused before it runs (\"Claude requested permissions to use …, \
         but you haven't granted it yet\"): nothing it would have reached was asked. Do not stop there: work \
         around the refusal with what you may use, and finish what you were asked as well as you can. Say so \
         in what you write: that the tool was not allowed, naming it (`mcp__moz__get_bugzilla_bug`, `Bash: \
         curl …`), never that the service behind it refused, failed or would not let you read. And list each \
         tool you were refused {said}, as \"refused\": [{{\"tool\": the tool as you called it \
         (`mcp__moz__get_bugzilla_bug`, `Bash: searchfox-cli --define Foo`), \"why\": why you needed it, and \
         how you worked around it}}]: the user reviews each, and may let you use it."
    )
}

/// What the agent's answer to ask `i`, on stack `s`, is to be, as a JSON
/// Schema: what its prompt asks for, given back as structured output
/// (`--json-schema`), which claude checks before it is done. Its replies by
/// the comments' labels, a build and test's findings by the patches'
/// numbers; what is written a string, or one with the references it
/// attaches: lines of a file, a comment, a web link.
fn answer_schema(i: &Info, s: &Stack) -> Value {
    use serde_json::json;
    let refs = json!({
        "type": "array",
        "items": {
            "type": "object",
            "properties": {
                "file": {"type": "string"},
                "lines": {"type": "string"},
                "patch": {"type": "integer"},
                "comment": {"type": "string"},
                "url": {"type": "string"}
            },
            "additionalProperties": false
        }
    });
    let written = json!({"anyOf": [
        {"type": "string"},
        {
            "type": "object",
            "properties": {"text": {"type": "string"}, "refs": refs.clone()},
            "required": ["text"],
            "additionalProperties": false
        }
    ]});
    let keyed = |keys: Vec<String>| {
        let props: Map<String, Value> = keys.into_iter().map(|k| (k, written.clone())).collect();
        json!({"type": "object", "properties": props, "additionalProperties": false})
    };
    let mut props = Map::new();
    props.insert("replies".into(), keyed(i.notes.iter().map(|sh| sh.label.clone()).collect()));
    // Each tool it was refused: as it called it, and why it needed it and
    // how it worked around it.
    let refused = json!({
        "type": "object",
        "properties": {"tool": {"type": "string"}, "why": written.clone()},
        "required": ["tool", "why"],
        "additionalProperties": false
    });
    props.insert("refused".into(), json!({"type": "array", "items": refused}));
    match (i.way, &i.scope) {
        (Way::Change, Scope::Stack) => {
            props.insert("base".into(), json!({"type": "string"}));
        }
        (Way::Change, Scope::Patch(_)) => {
            props.insert("promote".into(), json!({"type": "string"}));
        }
        (Way::Answer, _) => {
            let comment = json!({
                "type": "object",
                "properties": {
                    "patch": {"type": "integer"},
                    "file": {"type": "string"},
                    "side": {"enum": ["+", "-", " "]},
                    "line": {"type": "string"},
                    "text": {"type": "string"},
                    "refs": refs
                },
                "required": ["patch", "file", "line", "text"],
                "additionalProperties": false
            });
            props.insert("comments".into(), json!({"type": "array", "items": comment}));
        }
        (Way::Test, _) => {
            props.insert("patches".into(), keyed((1..=s.patches.len()).map(|n| n.to_string()).collect()));
        }
        // A try push's tasks, each its label and why, and how many times
        // each runs.
        #[cfg(feature = "try-push")]
        (Way::Try, _) => {
            for (key, schema) in crate::try_push::schema(&written) {
                props.insert(key.into(), schema);
            }
        }
    }
    json!({"type": "object", "properties": props, "required": ["replies"], "additionalProperties": false})
}

/// What an ask is told of the references it may attach to what it writes:
/// lines of a file, a comment of the stack, a web link, the stack first.
const REFS: &str = " To point at the code you mean, in the stack or out of it, anything you write \
there may be an object instead of a string, {\"text\": what you write, \"refs\": [...]}, and a \
comment you open may have \"refs\" too: each {\"file\": its path, \"lines\": \"120-140,200-210\" \
(none, the whole file), \"patch\": a patch's number, for the file as that patch leaves it; 0, as \
the stack's base has it; none, as the whole stack leaves it}; or, to point at a comment of the \
stack, {\"comment\": its label \
above ([c1] is \"c1\"), or its id, as `stacked comments` lists it}; or, to point at a page of the \
web, {\"url\": \"https://example.com/a/b\" (its whole address, https or http)}. Point at the stack \
first: when the code or the comments of the stack say what you mean, point at those lines of a \
file, as a patch, the base or the whole stack leaves it, or at that comment, rather than at a page \
of the web. A web link is for what nothing in the stack, the repository or its comments says (a \
bug, a specification, a page of documentation). The user reads each under what you wrote, and \
goes there. Each takes a number, in the order you give them: in a reply, from the one its comment \
above says; in a comment you open, from r1. In what you write, @r1 names reference r1 of the \
comment you write on, @r{1,2} several, @r* all of them: those attached to it, and those you \
attach.";

/// The program run as claude, when not `$STACKED_CLAUDE`'s or `claude`,
/// and the stacked an ask calls back (`prompt`, `record`, `land`), when not
/// this one: the editor's tests set them here, as they run the editor in a
/// process of their own and cannot each set the environment they share.
static CLAUDE: std::sync::Mutex<Option<(PathBuf, PathBuf)>> = std::sync::Mutex::new(None);

/// Runs `claude` as claude from now on, the asks calling `stacked` back.
pub fn stand_in(claude: &Path, stacked: &Path) {
    *CLAUDE.lock().unwrap() = Some((claude.to_path_buf(), stacked.to_path_buf()));
}

/// The program the tests run as claude, if they set one (`stand_in`): what
/// a process this one starts is told to run as claude in turn
/// (`$STACKED_CLAUDE`), for the asks it starts.
pub(crate) fn claude_stand_in() -> Option<PathBuf> {
    CLAUDE.lock().unwrap().as_ref().map(|(claude, _)| claude.clone())
}

/// Has the git commands `c` runs make commits and tags unsigned, whatever
/// the user's git config says: settings added to those of `GIT_CONFIG_COUNT`.
fn unsigned(c: &mut Command) {
    let n: usize = std::env::var("GIT_CONFIG_COUNT").ok().and_then(|n| n.parse().ok()).unwrap_or(0);
    c.env("GIT_CONFIG_COUNT", (n + 2).to_string());
    for (k, (key, value)) in [("commit.gpgsign", "false"), ("tag.gpgsign", "false")].into_iter().enumerate() {
        c.env(format!("GIT_CONFIG_KEY_{}", n + k), key);
        c.env(format!("GIT_CONFIG_VALUE_{}", n + k), value);
    }
}

/// Claude's lines, stdout and stderr, go to `stacked record`, then a line
/// saying how it exited; then `stacked land` files what it did.
const WRAPPER: &str = r#"trap '' HUP INT
id="$1"; cwd="$2"; exe="$3"; db="$4"; shift 4
cd "$cwd" || exit 1
{ "${STACKED_CLAUDE:-claude}" -p "$("$exe" -f "$db" prompt "$id")" "$@" < /dev/null 2>&1
  echo "{\"type\":\"exit\",\"code\":$?}"; } | "$exe" -f "$db" record "$id"
"$exe" -f "$db" land "$id" > /dev/null 2>&1
"#;

/// What the agent has, as it says when it starts in `dir`, run as `cfg`
/// says (the settings there), in its first event (`system/init`): its
/// tools, and its MCP servers by name. It is stopped then, before it is
/// asked anything.
pub fn agent_tools(dir: &Path, cfg: &crate::config::Config) -> Result<(Vec<String>, Vec<String>)> {
    let claude = match CLAUDE.lock().unwrap().clone() {
        Some((claude, _)) => claude.to_string_lossy().into_owned(),
        None => cfg.agent_command(),
    };
    let mut c = Command::new(&claude);
    c.args(["-p", "Say OK.", "--output-format", "stream-json", "--verbose"]);
    c.args(cfg.agent_args());
    c.current_dir(dir).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    let mut child = c.spawn().map_err(|e| Error(format!("{claude}: {e}")))?;
    let out = child.stdout.take().ok_or_else(|| Error(format!("{claude}: no output")))?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in std::io::BufReader::new(out).lines().map_while(|l| l.ok()) {
            if let Ok(v) = serde_json::from_str::<Value>(&line)
                && v.get("type").and_then(Value::as_str) == Some("system")
                && v.get("subtype").and_then(Value::as_str) == Some("init")
            {
                let _ = tx.send(v);
                return;
            }
        }
    });
    let init = rx.recv_timeout(std::time::Duration::from_secs(60));
    let _ = child.kill();
    let _ = child.wait();
    let init = init.map_err(|_| Error(format!("{claude} said nothing of its tools: it did not start, or not as claude does")))?;
    let names = |key: &str, field: Option<&str>| -> Vec<String> {
        init.get(key)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|t| match field {
                Some(f) => t.get(f).and_then(Value::as_str),
                None => t.as_str(),
            })
            .map(String::from)
            .collect()
    };
    Ok((names("tools", None), names("mcp_servers", Some("name"))))
}

/// The tools every ask is given, whatever it is: they only read.
pub const GIVEN: [&str; 4] = ["Read", "Grep", "Glob", "LS"];

/// Tools that change things: let answer-only asks use them, and an answer
/// may change things too.
pub const CHANGING: [&str; 7] = ["Bash", "Edit", "MultiEdit", "Write", "NotebookEdit", "Task", "KillShell"];

/// The worktrees of a stack's own that changes, answers, and builds and
/// tests work in: `agent`, `agent-2`…, `answer-only-agent`…, `build` under
/// `.stacked/NAME`.
pub const AGENT: &str = "agent";
pub const ANSWERING: &str = "answer-only-agent";
pub const BUILD: &str = "build";

/// Where stack `name`'s agents' worktrees are: `.stacked/NAME` in its
/// repository, else under `under` (the settings' `agent.worktrees`), in a
/// folder of the repository's own.
pub(crate) fn agents_dir(repo: &Path, name: &str, under: Option<&Path>) -> Result<PathBuf> {
    match under {
        Some(d) => {
            let top = git::toplevel(repo)?;
            let tag = sha1_smol::Sha1::from(&top).digest().to_string();
            let short = Path::new(&top).file_name().map_or(String::new(), |n| n.to_string_lossy().into_owned());
            Ok(d.join(format!("{short}-{}", &tag[..8])).join(name))
        }
        None => git::stack_dir(repo, name),
    }
}

/// `kind`'s worktrees in `dir`, as named: KIND, then KIND-2, KIND-3…
pub(crate) fn slot(dir: &Path, kind: &str, k: usize) -> PathBuf {
    dir.join(if k == 1 { kind.to_string() } else { format!("{kind}-{k}") })
}

/// The first of `kind`'s worktrees in `dir` that no other running ask is
/// in and that holds nothing an agent left unfinished, taken for run `id`:
/// written as its worktree in the database, so that no other takes it. One
/// not there yet is made by `git::ready`.
fn take_slot(db_path: &Path, id: &str, dir: &Path, kind: &str) -> Result<PathBuf> {
    // What an agent left unfinished stays as it is, to look at; what a
    // build made, untracked, is the next build's to build on.
    let left = |at: &Path| if kind == BUILD { git::unfinished_tracked(at) } else { git::unfinished(at) };
    let mut k = 1;
    loop {
        let at = slot(dir, kind, k);
        k += 1;
        if at.join(".git").exists() && left(&at).unwrap_or(true) && !scrub_dropped(db_path, &at) {
            continue;
        }
        let path = at.to_string_lossy().into_owned();
        let taken = Db::edit(db_path, |db| {
            let busy = db
                .live_runs()
                .iter()
                .filter(|r| r.state == RunState::Started && r.id != id)
                .any(|r| info(r).and_then(|i| i.worktree).as_deref() == Some(path.as_str()));
            if busy {
                return Ok(false);
            }
            let run = db.run(id).cloned().ok_or_else(|| Error(format!("no run {id}")))?;
            let mut i = info(&run).ok_or_else(|| Error(format!("run {id} is not an ask")))?;
            i.worktree = Some(path.clone());
            i.kept = true;
            db.add_run(Run {
                ask: Some(serde_json::to_value(&i).unwrap()),
                ..run
            })?;
            Ok(true)
        })?;
        if taken {
            return Ok(at);
        }
    }
}

/// Whether worktree `at` holds only what a dropped guess left there, its
/// agent gone: then that is thrown away, and the worktree is free again.
fn scrub_dropped(db_path: &Path, at: &Path) -> bool {
    let path = at.to_string_lossy().into_owned();
    let Ok(db) = Db::load(db_path) else { return false };
    let last = db
        .runs()
        .into_iter()
        .filter(|r| info(r).and_then(|i| i.worktree).as_deref() == Some(path.as_str()))
        .max_by_key(|r| r.time);
    last.is_some_and(|r| info(r).is_some_and(|i| i.dropped) && !r.pid.is_some_and(alive)) && git::scrub(at).is_ok()
}

/// The kinds of worktree the agent's settings have made when the editor
/// opens a stack, and removed when it exits.
pub fn precreated(agent: &crate::config::Agent) -> Vec<&'static str> {
    let mut kinds = Vec::new();
    if agent.worktree == Workspace::Precreate {
        kinds.push(AGENT);
    }
    if agent.answer_only_worktree == Workspace::Precreate {
        kinds.push(ANSWERING);
    }
    kinds
}

/// Stack `name`'s first worktree of each of `kinds` made, if it is not
/// there, the whole stack applied in it: for the asks to come not to wait
/// for it; `under`, the settings' `agent.worktrees`. Where they are, to
/// remove once the editor exits. Made under the launch lock, as an ask's
/// worktree is.
pub fn precreate(db_path: &Path, name: &str, kinds: &[&str], under: Option<&Path>) -> Result<(PathBuf, PathBuf)> {
    let _launching = launch_lock(db_path)?;
    let db = Db::load(db_path)?;
    let s = db.stack(name)?;
    let (repo, base) = base_of(s)?;
    let dir = agents_dir(&repo, name, under)?;
    let patches: Vec<&Patch> = s.patches.iter().map(|p| db.patch(p)).collect();
    let mbox = format_mbox(&patches);
    for kind in kinds {
        let at = slot(&dir, kind, 1);
        if !at.join(".git").exists() {
            git::ready(&repo, &at, &base, &mbox, patches.len(), &|_| {})?;
        }
    }
    Ok((repo, dir))
}

/// A stack's agents' worktrees made ahead: its repository, their folder,
/// and their kinds.
pub type Ahead = (PathBuf, PathBuf, Vec<&'static str>);

/// The worktrees made ahead, `made`, removed, away from the caller, which
/// need not wait: but those an ask is working in, and, as git refuses to
/// remove them, those with anything uncommitted. Under the launch lock:
/// what is free is read under it, and the shell that removes them is
/// handed it (`LaunchLock::hand_to`), which it holds until it is done,
/// however soon the caller goes on, or exits.
pub fn forget(db_path: &Path, made: &[Ahead]) {
    let Ok(launching) = launch_lock(db_path) else { return };
    let busy: HashSet<String> = Db::load(db_path)
        .map(|db| {
            db.live_runs()
                .iter()
                .filter(|r| r.state == RunState::Started)
                .filter_map(|r| info(r)?.worktree)
                .collect()
        })
        .unwrap_or_default();
    for (repo, dir, kinds) in made {
        let mut gone: Vec<PathBuf> = Vec::new();
        for kind in kinds {
            for k in 1.. {
                let at = slot(dir, kind, k);
                if !at.join(".git").exists() {
                    break;
                }
                if !busy.contains(&*at.to_string_lossy()) {
                    gone.push(at);
                }
            }
        }
        if gone.is_empty() {
            continue;
        }
        let mut c = Command::new("sh");
        c.arg("-c")
            .arg("for w; do git worktree remove \"$w\"; done; git worktree prune")
            .arg("sh")
            .args(&gone)
            .current_dir(repo)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        std::os::unix::process::CommandExt::process_group(&mut c, 0);
        // Taken under another of this thread's, the lock is not this one's
        // to hand on: they are removed here, while it is held.
        if !launching.hand_to(&mut c) {
            let _ = c.status();
            continue;
        }
        if let Ok(child) = c.spawn() {
            keep_started(child);
        }
    }
    launching.leave();
}

/// Whether rebase plan `text` may be given git as its todo: none of its
/// lines runs a command (`exec`, `x`), which could reach the network. The
/// line that would, said, if one does.
pub fn check_plan(text: &str) -> Result<()> {
    for (k, line) in text.lines().enumerate() {
        let word = line.split_whitespace().next().unwrap_or("");
        if matches!(word, "exec" | "x") {
            return err(format!(
                "the plan's line {} runs a command (`{}`), which is not allowed: the rebase is not started",
                k + 1,
                line.trim()
            ));
        }
    }
    Ok(())
}

/// The folder the agents' rebase plans are kept in, as the binary set it
/// as it starts (`plans_in`).
static PLANS: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// Where the agents' rebase plans are kept, as the environment `var` names
/// it: the folder `$STACKED_TODO` names, else `stacked-todo` in the
/// temporary directory, which every stacked shares. One set to nothing is
/// not set, and one not from the root is taken from where stacked runs.
pub fn plans_from(var: impl Fn(&str) -> Option<std::ffi::OsString>) -> PathBuf {
    match var("STACKED_TODO").filter(|v| !v.is_empty()) {
        Some(v) => std::path::absolute(Path::new(&v)).unwrap_or_else(|_| PathBuf::from(v)),
        None => std::env::temp_dir().join("stacked-todo"),
    }
}

/// From now on, the agents' rebase plans are kept in `dir`, unless that was
/// set already: by the binary, as it starts, before any ask does. Whether
/// they are now.
pub fn plans_in(dir: PathBuf) -> bool {
    PLANS.set(dir).is_ok()
}

/// Where the asks on database `db` keep their rebase plans: where the
/// binary set it as it started; else beside the database. Only the tests'
/// own processes are never set up so: their plans are kept in the test's
/// own folder, and they never touch the one that every stacked shares.
pub(crate) fn plans(db: &Path) -> PathBuf {
    PLANS.get().cloned().unwrap_or_else(|| db.with_file_name("stacked-todo"))
}

/// The file in a plan's folder that names, in full, the git directory the
/// folder is for: the folder's own name is a hash of that path, which
/// cannot be read back.
const GITDIR: &str = "gitdir";

/// How long a folder that names no git directory, as a stacked of before
/// made them and as a try push's selection's is, is kept once nothing in
/// it has changed: a day.
const UNNAMED_KEPT: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);

/// Whether `name` is a plan folder's: the first 12 hex digits of a SHA-1.
fn plan_name(name: &str) -> bool {
    name.len() == 12 && name.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// The folder in `base` for the rebase plans of git directory `gitdir`,
/// made if it is not there, and the file in it that names that git
/// directory written. Its name is the first 12 hex digits of the SHA-1 of
/// the git directory's path, as self-review names its own.
fn plan_folder(base: &Path, gitdir: &str) -> Result<PathBuf> {
    let dir = base.join(crate::diff::sha1_12(gitdir));
    let say = |e: std::io::Error| Error(format!("{}: {e}", dir.display()));
    std::fs::create_dir_all(&dir).map_err(say)?;
    std::fs::write(dir.join(GITDIR), format!("{gitdir}\n")).map_err(say)?;
    Ok(dir)
}

/// The git directory plan folder `dir` is for, as its `gitdir` file names
/// it. A file that names one whose hash is not the folder's name, as when
/// something else wrote over it, names none.
fn plan_gitdir(dir: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(dir.join(GITDIR)).ok()?;
    let gitdir = text.trim();
    let named = crate::diff::sha1_12(gitdir);
    (dir.file_name() == Some(std::ffi::OsStr::new(&named))).then(|| PathBuf::from(gitdir))
}

/// Whether a rebase is under way in git directory `gitdir`.
fn rebasing(gitdir: &Path) -> bool {
    ["rebase-merge", "rebase-apply"].iter().any(|f| gitdir.join(f).exists())
}

/// When anything in folder `dir` last changed: the folder itself, or one of
/// the files in it.
fn last_changed(dir: &Path) -> Option<std::time::SystemTime> {
    let mut last = std::fs::metadata(dir).and_then(|m| m.modified()).ok()?;
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        if let Ok(t) = e.metadata().and_then(|m| m.modified()) {
            last = last.max(t);
        }
    }
    Some(last)
}

/// The plan folders of the asks still running on `db`, but run `id`'s.
fn plans_busy(db: &Db, id: &str) -> HashSet<PathBuf> {
    db.live_runs()
        .iter()
        .filter(|r| r.state == RunState::Started && r.id != id)
        .filter_map(|r| info(r)?.todo.map(PathBuf::from))
        .collect()
}

/// The plan folders in `base` that no rebase can want any more, removed:
/// each folder whose git directory is gone, as its `gitdir` file names it;
/// and each that names none, as a stacked of before made them and as a try
/// push's selection's copy of the task graph is kept, once nothing in it
/// has changed for a day (`now` being the time). Never a folder that one
/// of the asks still running (`busy`) writes in: a folder whose git
/// directory is there is kept, rebase or not.
fn prune_plans(base: &Path, busy: &HashSet<PathBuf>, now: std::time::SystemTime) {
    let Ok(entries) = std::fs::read_dir(base) else {
        return;
    };
    for e in entries.flatten() {
        let dir = e.path();
        let named = e.file_name().to_str().is_some_and(plan_name);
        if !named || !e.file_type().is_ok_and(|t| t.is_dir()) || busy.contains(&dir) {
            continue;
        }
        let gone = match plan_gitdir(&dir) {
            Some(gitdir) => matches!(gitdir.try_exists(), Ok(false)),
            None => last_changed(&dir).is_some_and(|t| now.duration_since(t).is_ok_and(|d| d > UNNAMED_KEPT)),
        };
        if gone {
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
}

/// Plan folder `dir`, its ask landed, removed: unless a rebase may still
/// want it, one of the asks still running (`busy`) writing its plan there,
/// or a rebase under way in its git directory.
fn drop_plan(dir: &Path, busy: &HashSet<PathBuf>) {
    let named = dir.file_name().and_then(|n| n.to_str()).is_some_and(plan_name);
    if !named || busy.contains(dir) || plan_gitdir(dir).is_some_and(|g| rebasing(&g)) {
        return;
    }
    let _ = std::fs::remove_dir_all(dir);
}

/// Where a change to the stack writes its rebase plan, for the worktree (or
/// checkout) `dir`: in `base`, the folder for its git directory, outside
/// that git directory, which claude may not write in, as self-review has
/// it; a plan an earlier ask left there gone. The folders whose worktree is
/// gone are removed first, but those of the asks still running (`busy`).
fn rebase_todo(base: &Path, dir: &Path, busy: &HashSet<PathBuf>) -> Result<PathBuf> {
    let gitdir = git::git(dir, &["rev-parse", "--absolute-git-dir"])?;
    prune_plans(base, busy, std::time::SystemTime::now());
    let todo = plan_folder(base, gitdir.trim())?.join("REBASE_TODO");
    let _ = std::fs::remove_file(&todo);
    Ok(todo)
}

/// The folder of its own that run `id` of database `db`, a try push's
/// selection, downloads the task graph into, made empty: in `base`, beside
/// the rebase plans' folders, and named as they are, so that it goes as
/// they go, once the ask lands, however it ends, and, should nothing land
/// it, once nothing in it has changed for a day. Each ask has its own: two
/// at once each download their own copy.
#[cfg(feature = "try-push")]
fn graph_folder(base: &Path, db: &Path, id: &str) -> Result<PathBuf> {
    let dir = base.join(crate::diff::sha1_12(&format!("try push {} {id}", db.display())));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| Error(format!("{}: {e}", dir.display())))?;
    Ok(dir)
}

/// Starts claimed run `id`: its worktree made, its prompt written, claude
/// started.
fn start(db_path: &Path, id: &str) -> Result<()> {
    let db = Db::load(db_path)?;
    let run = db.run(id).cloned().ok_or_else(|| Error(format!("no run {id}")))?;
    // Landed already, stopped before it started: nothing to start.
    if run.state != RunState::Started {
        return Ok(());
    }
    let mut i = info(&run).ok_or_else(|| Error(format!("run {id} is not an ask")))?;
    let s = started_from(&db, &i)?;
    let (repo, base) = base_of(&s)?;
    // A repository kept in memory, as the demo's: answered there, by the
    // stand-in it names, not by claude in a worktree on disk.
    #[cfg(feature = "demo")]
    if crate::memory::is_repo(&repo) {
        return crate::memory::agent(db_path, id);
    }
    i.base = base.clone();
    i.repo = repo.to_string_lossy().into_owned();
    // Each command it runs to start, in its stream as it goes: what the
    // comment says while the agent has not written yet.
    let step = |text: String| {
        let event = serde_json::json!({"type": STARTING, "text": text});
        let _ = Db::append(db_path, &[Record::Event { run: id.into(), event }]);
    };
    // What it may use for this work alone, said first in its stream.
    if !i.allowed.is_empty() {
        let event = serde_json::json!({"type": ALLOWED, "text": i.allowed.join(", ")});
        let _ = Db::append(db_path, &[Record::Event { run: id.into(), event }]);
    }
    let upto = match &i.scope {
        Scope::Stack => s.patches.len(),
        Scope::Patch(c) => {
            let k = s
                .patches
                .iter()
                .position(|p| db.patch(p).change == *c)
                .ok_or_else(|| Error("its patch is no longer in the stack".into()))?;
            i.patch = Some((s.patches[k].clone(), k));
            k + 1
        }
    };
    // Where it works: a worktree of the stack's own, never the user's
    // checkout, the stack up to its place applied. A change takes one of
    // the changes'; an answer, and a try push's selection, one of the
    // answers'; a build and test, one of the builds'. The settings of the
    // stack's checkout, read now, say where the agents' worktrees go, and
    // what the agent is.
    let (cfg, _) = crate::config::of(&repo);
    let agent = &cfg.agent;
    let patches: Vec<&Patch> = s.patches[..upto].iter().map(|p| db.patch(p)).collect();
    let mbox = format_mbox(&patches);
    let kind = match i.way {
        Way::Change => AGENT,
        Way::Answer => ANSWERING,
        Way::Test => BUILD,
        #[cfg(feature = "try-push")]
        Way::Try => ANSWERING,
    };
    let dir = agents_dir(&repo, &s.name, agent.worktrees.as_deref())?;
    let cwd = take_slot(db_path, id, &dir, kind)?;
    held_at_start(id);
    git::ready(&repo, &cwd, &base, &mbox, upto, &step)?;
    i.worktree = Some(cwd.to_string_lossy().into_owned());
    i.kept = true;
    if i.way == Way::Change {
        let shas: Vec<String> = git::git(&cwd, &["rev-list", "--reverse", &format!("{base}..HEAD")])?
            .lines()
            .map(String::from)
            .collect();
        if shas.len() != upto {
            return err(format!("{upto} patches made {} commits: is one empty?", shas.len()));
        }
        i.commits = shas.into_iter().zip(s.patches[..upto].iter().cloned()).collect();
    }
    let exe = stacked_exe()?;
    let db_abs = db_path.canonicalize().map_err(|e| Error(e.to_string()))?;
    let cmd = format!("{} -f {}", exe.display(), db_abs.display());
    // A change may run the linter of the stack's checkout, as the
    // checkout's settings have it now, on the files it changes; no other
    // kind of ask is given it.
    let linter = match i.way {
        Way::Change => Linter::of(&repo),
        _ => Linter::None,
    };
    // What it may use: what stacked gives its kind of ask, and what the
    // settings add, for its kind and for every ask; then what it may use
    // for this work alone. Kept, to read what it is refused against.
    let tools = with_more(tools_for(i.way, &cmd, agent, &linter), &i.allowed);
    i.given = tools.clone();
    let curl = tools.split(',').any(|t| t == "Bash(curl *)");
    let plans = plans(&db_abs);
    // A try push's selection that may run curl downloads the task graph, to
    // check its labels against, into a folder of its own that its prompt
    // names: beside the rebase plans' folders, outside your checkout and
    // any worktree, kept in the run as theirs is, to remove once it lands.
    #[cfg(feature = "try-push")]
    if i.way == Way::Try && curl {
        i.todo = Some(graph_folder(&plans, &db_abs, id)?.to_string_lossy().into_owned());
    }
    // Told of the linter it is given, by its command.
    let lints = linter.rule().and(linter.command());
    i.prompt = prompt(&db, &i, &s, &cmd, id, agent.schema, curl, lints);
    // A change to the stack rewrites what is below HEAD with a plan it
    // writes, which a plain `git rebase -i` takes: claude refuses a command
    // that sets an editor. Its plan's folder is kept in the run, to remove
    // once it lands.
    let todo = match (i.way, &i.scope) {
        (Way::Change, Scope::Stack) => Some(rebase_todo(&plans, &cwd, &plans_busy(&db, id))?),
        _ => None,
    };
    if let Some(todo) = &todo {
        i.prompt += &format!("\n\nThe rebase todo file: {}", todo.display());
        i.todo = todo.parent().map(|d| d.to_string_lossy().into_owned());
    }
    let permission = if reads(i.way) { "default" } else { "acceptEdits" };
    let mut run = Run {
        ask: Some(serde_json::to_value(&i).unwrap()),
        ..run
    };
    let landed = Db::edit(db_path, |db| {
        // Unless it was landed meanwhile, taken for one left behind: then
        // the folder made for it goes.
        if db.run(id).is_some_and(|r| r.state == RunState::Started) {
            db.add_run(run.clone())?;
            Ok(false)
        } else {
            if let Some(dir) = &i.todo {
                drop_plan(Path::new(dir), &plans_busy(db, id));
            }
            Ok(true)
        }
    })?;
    if landed {
        return err("it was landed while its worktree was made");
    }
    let mut c = Command::new("sh");
    c.arg("-c").arg(WRAPPER).arg("sh").arg(id).arg(&cwd).arg(&exe).arg(&db_abs);
    c.args(["--output-format", "stream-json", "--verbose"]);
    c.args(["--permission-mode", permission, "--allowedTools", &tools]);
    // The plan checked (one that runs a command is refused), then copied in
    // as the rebase's todo, the messages kept: no editor opens.
    if let Some(todo) = &todo {
        let quoted = |p: &Path| format!("'{}'", p.display().to_string().replace('\'', "'\\''"));
        c.env("GIT_SEQUENCE_EDITOR", format!("{} __plan {}", quoted(&exe), quoted(todo)));
        c.env("GIT_EDITOR", "true");
    }
    // Its folder of its own, its rebase plan's or its copy of the task
    // graph's, is claude's to write in, as the worktree is.
    if let Some(dir) = &i.todo {
        c.arg("--add-dir").arg(dir);
    }
    // What would run another program, or delete files, of what a change may
    // run, and of what an ask that reads may be let run: refused, to every
    // ask but a build and test, which runs what the project needs; and to
    // an ask that only reads, what would make what it is given write a file
    // or run a program. And to every ask, what the checkout's claude
    // settings of yours deny, as claude does in the checkout: a worktree of
    // the stack's own has no file of yours, git tracking none.
    let mut refused = refused_of(i.way);
    refused.extend(crate::setup::denied(&repo));
    if !refused.is_empty() {
        c.args(["--disallowedTools", &refused.join(",")]);
    }
    // Its answer asked as structured output, checked against what this ask
    // asks for; else read from what it writes.
    if agent.schema {
        c.args(["--json-schema", &answer_schema(&i, &s).to_string()]);
    }
    // What the settings add: a model, arguments of the user's; then the
    // effort the ask goes at, in place of one they give.
    let args = at_effort(cfg.agent_args(), i.effort);
    c.args(&args);
    c.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    // The program run as claude: the tests' stand-in, else $STACKED_CLAUDE,
    // the settings' or claude.
    let claude = match CLAUDE.lock().unwrap().clone() {
        Some((claude, _)) => claude.to_string_lossy().into_owned(),
        None => cfg.agent_command(),
    };
    let extra: String = args.iter().map(|a| format!(" {a}")).collect();
    step(format!("{claude} -p … --permission-mode {permission}{extra}, in {}", cwd.display()));
    c.env("STACKED_CLAUDE", claude);
    // The stacked it calls back keeps what is yours where this one does:
    // nowhere, in the tests' own processes. It keeps the agents' rebase
    // plans where this one does too.
    crate::home::get().pass_on(&mut c);
    c.env("STACKED_TODO", &plans);
    // The agent's commits are read back as patches, then gone: not signed,
    // which would want the user's key for each.
    unsigned(&mut c);
    std::os::unix::process::CommandExt::process_group(&mut c, 0);
    let child = c.spawn().map_err(|e| Error(format!("sh: {e}")))?;
    let pid = child.id();
    run.pid = Some(pid);
    // Kept, to wait for once it has exited (`reap`), before its pid is
    // written for anyone to ask after.
    started().push(child);
    let stopped = Db::edit(db_path, |db| {
        // Stopped as it started, by a stop that could not know its pid: its
        // agent is stopped here. Else, unless it already landed (claude may
        // be that quick), its pid is written.
        let stopped = db.events(id).iter().any(|e| e.get("type").and_then(Value::as_str) == Some(STOP));
        if !stopped && db.run(id).is_some_and(|r| r.state == RunState::Started) {
            db.add_run(run)?;
        }
        Ok(stopped)
    })?;
    if stopped {
        signal(pid, libc::SIGKILL);
    }
    Ok(())
}

/// `args`, the arguments the settings add to claude, with `--effort` at
/// `effort`, the ask's, after them, in place of any `--effort` of theirs;
/// as they are for an ask with none.
fn at_effort(args: Vec<String>, effort: Option<Effort>) -> Vec<String> {
    let Some(effort) = effort else { return args };
    let mut out = Vec::new();
    let mut args = args.into_iter();
    while let Some(a) = args.next() {
        if a == "--effort" {
            args.next();
        } else if !a.starts_with("--effort=") {
            out.push(a);
        }
    }
    out.extend(["--effort".to_string(), effort.name().to_string()]);
    out
}

/// Appends each line of `input` to run `id`'s events as it comes: JSON as
/// it is, any other line as `{"type": "text", "text": ...}`.
pub fn record(db_path: &Path, id: &str, input: impl BufRead) -> Result<()> {
    for line in input.lines() {
        let line = line.map_err(|e| Error(e.to_string()))?;
        if line.trim().is_empty() {
            continue;
        }
        let event = serde_json::from_str::<Value>(&line)
            .ok()
            .filter(Value::is_object)
            .unwrap_or_else(|| serde_json::json!({"type": "text", "text": line}));
        Db::append(db_path, &[Record::Event { run: id.into(), event }])?;
    }
    Ok(())
}

/// The type of the event a run's stream gets as you stop its agent, before
/// the agent is sent anything: whatever lands the ask then says that you
/// stopped it, rather than that it was killed.
const STOP: &str = "stop";

/// How an agent you stopped exited, as `exit_code` says it.
const STOPPED: &str = "stopped";

/// How run `id`'s agent exited: as its stream says, else `exit` (it was
/// killed), else unknown. Once you stopped it, it was stopped, unless it
/// exited before that, or was done as you stopped it.
fn exit_code(db: &Db, id: &str, exit: Option<&str>) -> String {
    let events = db.events(id);
    let last = |kind: &str| events.iter().rposition(|e| e.get("type").and_then(Value::as_str) == Some(kind));
    match (last("exit"), last(STOP)) {
        (Some(x), Some(s)) if x > s && str_of(events[x].get("code")) != "0" => STOPPED.into(),
        (Some(x), _) => str_of(events[x].get("code")),
        (None, Some(_)) => STOPPED.into(),
        (None, None) => exit.map_or_else(|| "unknown".into(), String::from),
    }
}

/// What the agent answered last: the structured output of its stream's
/// `result`, as JSON, when it gave one; else what that `result` says.
fn result_text(events: &[&Value]) -> String {
    let results = || events.iter().rev().filter(|v| v.get("type").and_then(Value::as_str) == Some("result"));
    if let Some(o) = results().find_map(|v| v.get("structured_output").filter(|o| o.is_object())) {
        return o.to_string();
    }
    results().find_map(|v| v.get("result").and_then(Value::as_str).map(String::from)).unwrap_or_default()
}

/// How the agent's stream ended, when it ended in an error: as its
/// `result` says, said.
fn ended_badly(events: &[&Value]) -> Option<String> {
    let r = events.iter().rev().find(|v| v.get("type").and_then(Value::as_str) == Some("result"))?;
    if r.get("is_error").and_then(Value::as_bool) != Some(true) {
        return None;
    }
    Some(match r.get("subtype").and_then(Value::as_str).unwrap_or("") {
        "error_max_structured_output_retries" => "its answer never fit what it was asked for, tried again and again".into(),
        "error_max_turns" => "it ran out of turns".into(),
        "" => "it said it failed".into(),
        other => other.to_string(),
    })
}

/// What the run of ask `i` was refused, as its stream `events` say, as the
/// items of its review (`refusals::items`): read against what it was given,
/// what stacked refuses it (`refused_of`), and what the checkout's claude
/// settings deny; the agent's word on each, from its answer `output`.
fn refusals_of(events: &[&Value], i: &Info, output: &str) -> Vec<crate::refusals::Refusal> {
    let given: Vec<String> = i.given.split(',').filter(|t| !t.is_empty()).map(String::from).collect();
    let refused = refused_of(i.way);
    let denied = if i.repo.is_empty() { vec![] } else { crate::refusals::denied_in(Path::new(&i.repo)) };
    crate::refusals::items(events, i.way, &given, &refused, &denied, &crate::refusals::said_of(output))
}

/// How an ask in `way` may be let use more, said after what it was not let
/// use: in the review of its work, each tool allowed for this work or from
/// now on; the list of the settings for its kind, which `stacked setup
/// agent` sets; for a build and test, which runs any command already, every
/// ask's.
fn allow_more(way: Way) -> &'static str {
    match way {
        Way::Change => {
            "the review of its work lists each, to allow for this work or from now on; agent.change_tools lets asks that change the stack use more, and `stacked setup agent` sets it; your claude settings can allow more too"
        }
        Way::Answer => {
            "the review of its work lists each, to allow for this work or from now on; agent.answer_tools lets answer-only asks use more, and `stacked setup agent` sets it; your claude settings can allow more too"
        }
        Way::Test => {
            "the review of its work lists each, to allow for this work or from now on; agent.tools lets every ask use more, builds and tests too, and `stacked config agent.tools` sets it; your claude settings can allow more too"
        }
        #[cfg(feature = "try-push")]
        Way::Try => {
            "the review of its work lists each, to allow for this work or from now on; agent.try_tools lets the asks that select a try push's tasks use more, and `stacked setup agent` sets it; your claude settings can allow more too"
        }
    }
}

fn str_of(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) | None => String::new(),
        Some(o) => o.to_string(),
    }
}

/// What claude replied to each comment: `replies`, else labels at the top.
fn replies_of(text: &str) -> Map<String, Value> {
    match last_object(text).and_then(|o| o.get("replies").and_then(Value::as_object).cloned()) {
        Some(r) => r,
        None => parse_claude_replies(text).unwrap_or_default(),
    }
}

/// What landing an ask did, before it is a run.
struct Landed {
    ok: bool,
    summary: String,
    /// Leave the worktree made for it: it holds work not filed.
    keep: bool,
    /// What it brought back, held for review.
    held: Held,
}

impl Landed {
    /// What claude brought back, held for review.
    fn held(held: Held) -> Landed {
        let summary = if held.is_empty() {
            crate::held::NOTHING_BACK.to_string()
        } else {
            format!("its work waits for your review: {}", held.said())
        };
        Landed {
            ok: true,
            summary,
            keep: false,
            held,
        }
    }

    fn failed(summary: impl Into<String>, keep: bool) -> Landed {
        Landed {
            ok: false,
            summary: summary.into(),
            keep,
            held: Held::default(),
        }
    }
}

/// Files what a finished ask did; once, however often it is called. `exit`
/// says how claude exited when its events cannot (it was killed). Then
/// starts what was waiting behind it. Under the launch lock, both: what it
/// did is read from its worktree, which is cleaned up after. What it would
/// file is made first on a copy, under the launch lock too; what each
/// operation of its change needs is then worked out from that copy with no
/// lock held (`operations::needs_of`), as its checks only read the
/// repository and may take long on a big one, and stored with it as it is
/// filed (`held::needs_filed`).
pub fn land(db_path: &Path, id: &str, exit: Option<&str>) -> Result<Run> {
    let dry = {
        let _launching = launch_lock(db_path)?;
        dry_run(db_path, id, exit)
    };
    let needs = dry.and_then(|(db, r, h)| crate::operations::needs_of(&db, &r, &h));
    let _launching = launch_lock(db_path)?;
    let run = file_landed(db_path, id, exit, Some(needs))?;
    launch(db_path)?;
    Ok(run)
}

/// What landing run `id` would file, made on a copy of the database, with
/// no lock of the database held, nothing of the copy written: what takes
/// long (the stack made checked to apply, on a big repository) is then
/// remembered, and under the lock it is only written. The copy, the run as
/// it started, and what it would hold; none when it would file nothing of
/// its own (it failed, or is filed already).
fn dry_run(db_path: &Path, id: &str, exit: Option<&str>) -> Option<(Db, Run, Held)> {
    let mut dry = Db::load(db_path).ok()?;
    let started = dry.run(id).cloned().filter(|r| r.state == RunState::Started)?;
    let i = info(&started)?;
    if exit_code(&dry, id, exit) != "0" {
        return None;
    }
    let output = result_text(&dry.events(id));
    let landed = file(&mut dry, &started, &i, &output).ok()?;
    Some((dry, started, landed.held))
}

/// What `land` does, but start what waits. `worked`, what each operation of
/// its change needs, worked out before (an outer `None` when it was not:
/// then made here, under the launch lock held, as a landing of an agent
/// gone, or stopped, is).
fn file_landed(db_path: &Path, id: &str, exit: Option<&str>, worked: Option<Option<crate::operations::Needs>>) -> Result<Run> {
    let worked = match worked {
        Some(needs) => needs,
        None => dry_run(db_path, id, exit).and_then(|(db, r, h)| crate::operations::needs_of(&db, &r, &h)),
    };
    // Where it worked, to leave as it should be once it is filed.
    let mut after: Option<Info> = None;
    let run = Db::edit(db_path, |db| {
        let Some(started) = db.run(id).cloned() else {
            return err(format!("no run {id}"));
        };
        // Under the lock, so two landings cannot both file it.
        if started.state != RunState::Started {
            return Ok(started);
        }
        let Some(i) = info(&started) else {
            return err(format!("run {id} is not an ask"));
        };
        after = Some(i.clone());
        let events = db.events(id);
        let code = exit_code(db, id, exit);
        let output = result_text(&events);
        // What it was not let use, each an item of its review, so that it
        // may be allowed: then it waits for your review, though it failed
        // or brought nothing back.
        let refusals = refusals_of(&events, &i, &output);
        let mut landed = if code != "0" {
            let said: Vec<String> = events
                .iter()
                .filter(|e| e.get("type").and_then(Value::as_str) == Some("text"))
                .map(|e| str_of(e.get("text")))
                .collect();
            let keep = left_work(&i);
            let said = said.join("\n");
            let mut summary = match (code.as_str(), said.trim()) {
                (STOPPED, "") => STOPPED_SAID.to_string(),
                (STOPPED, said) => format!("{STOPPED_SAID}: {said}"),
                (code, said) => format!("the agent failed ({code}): {said}"),
            };
            if let Some(why) = ended_badly(&events) {
                summary += &format!("; {why}");
            }
            if let Some(w) = i.worktree.as_deref().filter(|_| keep) {
                summary += &format!("; its work is left in {w}");
            }
            Landed::failed(summary, keep)
        } else {
            // Whatever goes wrong filing it is what came of it: a run left
            // started would be landed again and again.
            file(db, &started, &i, &output).unwrap_or_else(|e| {
                let at = i.worktree.as_ref().map_or(String::new(), |w| format!("; its work is left in {w}"));
                Landed::failed(format!("could not file what the agent did: {}{at}", e.0), true)
            })
        };
        landed.held.tools = refusals;
        // What each operation of its change needs, worked out before with no
        // lock held: stored with it, if it is for that change from that stack.
        if landed.held.stack.is_some() {
            landed.held.needs = crate::held::needs_filed(db, &started, &landed.held, worked);
        }
        let tools = landed.held.tools.len();
        // Bringing nothing back, it waits for your review all the same while
        // a comment it was asked about is still open: to close or keep.
        let open = crate::held::asking_open(db, &started).len();
        let waits = (landed.ok && (!landed.held.is_empty() || open > 0)) || tools > 0;
        match (landed.ok, landed.held.is_empty()) {
            (true, false) if tools > 0 => landed.summary = format!("its work waits for your review: {}", landed.held.said()),
            (true, true) if tools > 0 || open > 0 => {
                let with = match tools {
                    0 => String::new(),
                    1 => ", with 1 tool the agent could not use".to_string(),
                    n => format!(", with {n} tools the agent could not use"),
                };
                landed.summary = format!("{}: its work waits for your review{with}", crate::held::nothing_back(open, i.notes.len()));
            }
            _ => {}
        }
        if tools > 0 {
            let rules: Vec<&str> = landed.held.tools.iter().map(crate::refusals::Refusal::rule).collect();
            landed.summary += &format!("; the agent was not allowed to use {}: {}", rules.join(", "), allow_more(i.way));
        }
        if !landed.ok {
            mark_failed(db, &i, &landed.summary)?;
        }
        if landed.keep {
            after = None;
        }
        // A follow-up's: into the review of the work it follows up, while
        // that work waits for your review; held on its own only once that
        // review is closed.
        let came = crate::held::come_in(db, &started, &i, &landed.held, landed.ok)?;
        if let Some(said) = &came {
            landed.summary = match landed.ok {
                true => said.clone(),
                false => format!("{}; {said}", landed.summary),
            };
        }
        // Held for review: in the queue, unless it brought nothing back and
        // no comment it was asked about is still open, or it came into the
        // review of the work it follows up.
        let held = (waits && came.is_none()).then(|| Held { at: now(), ..landed.held });
        let run = Run {
            state: if landed.ok { RunState::Done } else { RunState::Failed },
            summary: landed.summary,
            output,
            pid: None,
            held,
            ..started
        };
        db.add_run(run.clone())?;
        // Its folder of its own goes, failed or not, stopped or found dead,
        // under the lock: its rebase plan's, which an ask that takes its
        // worktree next makes again, or its copy of the task graph's.
        if let Some(dir) = &i.todo {
            drop_plan(Path::new(dir), &plans_busy(db, id));
        }
        Ok(run)
    })?;
    if let Some(i) = after {
        drop_scratch(&i);
    }
    Ok(run)
}

/// Whether the agent left work unfinished where run `i` worked: anything
/// uncommitted.
fn left_work(i: &Info) -> bool {
    let Some(w) = i.worktree.as_deref() else {
        return false;
    };
    let w = Path::new(w);
    if i.way == Way::Test {
        // What a build made, untracked, is not work: the next build's.
        git::unfinished_tracked(w).unwrap_or(false)
    } else {
        git::unfinished(w).unwrap_or(false)
    }
}

/// Where run `i` worked, once it is filed and nothing is left unfinished
/// there: a worktree a stacked of before made for it alone, removed. One
/// of the stack's own is kept for the next ask; and the user's checkout,
/// which a stacked of before let a change work in, is never removed.
fn drop_scratch(i: &Info) {
    let Some(w) = i.worktree.as_deref().map(Path::new) else {
        return;
    };
    if !i.kept && w != Path::new(&i.repo) {
        git::remove_scratch(Path::new(&i.repo), w);
    }
}

/// The comment the agent names `name` in what it points at: a label of
/// ask `i`'s comments (`c1`), or the id of one of stack `stack`'s as
/// `stacked comments` lists it.
fn named_comment(db: &Db, i: &Info, stack: &str, name: &str) -> Option<(String, String)> {
    let name = name.trim().trim_start_matches('[').trim_end_matches(']');
    if let Some(sh) = i.notes.iter().find(|sh| sh.label == name) {
        return Some((sh.change.clone(), sh.id.clone()));
    }
    db.find_note(stack, name).ok().map(|n| (n.change.clone(), n.id.clone()))
}

/// The stack an answer-only ask `i` was asked on, by its rev, as its turns
/// name it: none for another ask, nor for one asked on the stack a change
/// waiting for your review makes (a follow-up asked from that review),
/// which is no state of the stack.
fn asked_on(db: &Db, i: &Info) -> Option<usize> {
    let on_change = i.on.as_deref().and_then(|on| db.run(on)?.held.as_ref()?.stack.as_ref()).is_some();
    (i.way == Way::Answer && !on_change).then_some(i.from)
}

/// Claude's replies to the comments it was sent, held: turns `way` names,
/// at the effort the ask was asked at; an answer-only ask's naming the
/// stack it was asked on (`asked_on`).
fn held_replies(db: &Db, i: &Info, replies: &Map<String, Value>) -> Vec<crate::held::Reply> {
    let stack = db.node(i.from).map(|s| s.name.clone()).unwrap_or_default();
    i.notes
        .iter()
        .filter(|sh| db.note(&sh.change, &sh.id).is_some())
        .filter_map(|sh| {
            Some(crate::held::Reply {
                change: sh.change.clone(),
                id: sh.id.clone(),
                text: replies.get(&sh.label).map(|v| {
                    // Numbered after the comment's own references, as the
                    // prompt said: nothing could be attached meanwhile.
                    let first = db.note(&sh.change, &sh.id).map_or(1, crate::refs::next_of);
                    crate::refs::written_with(v, first, &|c| named_comment(db, i, &stack, c))
                })?,
                tag: i.way.name().into(),
                effort: i.effort,
                asked_on: asked_on(db, i),
                ..Default::default()
            })
        })
        .collect()
}

/// What claude brought back, held for review; nothing of it is applied.
fn file(db: &mut Db, run: &Run, i: &Info, result: &str) -> Result<Landed> {
    let replies = replies_of(result);
    match (i.way, &i.scope) {
        (Way::Change, Scope::Patch(_)) => land_patch(db, run, i, result, &replies),
        (Way::Change, Scope::Stack) => land_stack(db, run, i, result, &replies),
        (Way::Answer, _) => Ok(Landed::held(Held {
            replies: held_replies(db, i, &replies),
            opened: opened_notes(db, run, i, result)?,
            ..Default::default()
        })),
        (Way::Test, _) => Ok(Landed::held(Held {
            replies: held_replies(db, i, &replies),
            opened: test_notes(db, run, i, result)?,
            ..Default::default()
        })),
        // The tasks it selected, found whatever prose is around its answer,
        // each to accept or reject.
        #[cfg(feature = "try-push")]
        (Way::Try, _) => {
            let (tasks, rebuild) = crate::try_push::tasks_of(last_object(result).as_ref());
            Ok(Landed::held(Held {
                replies: held_replies(db, i, &replies),
                tasks,
                rebuild,
                ..Default::default()
            }))
        }
    }
}

/// A patch change landing, held: the patch's new version, or the pieces of
/// it, in its place in the stack; or, when claude said it needs more than
/// the patch, the comments to move to the stack's comments.
fn land_patch(db: &mut Db, run: &Run, i: &Info, result: &str, replies: &Map<String, Value>) -> Result<Landed> {
    let (old_id, k) = i.patch.clone().ok_or_else(|| Error("no patch".into()))?;
    if let Some(why) = last_object(result).and_then(|o| o.get("promote").map(|v| str_of(Some(v)))).filter(|w| !w.is_empty()) {
        let mut held = held_replies(db, i, replies);
        // Each comment moves with its reply, or alone.
        for sh in &i.notes {
            match held.iter_mut().find(|r| r.change == sh.change && r.id == sh.id) {
                Some(r) => r.promote = Some(why.clone()),
                None if db.note(&sh.change, &sh.id).is_some() => held.push(crate::held::Reply {
                    change: sh.change.clone(),
                    id: sh.id.clone(),
                    text: String::new(),
                    tag: i.way.name().into(),
                    promote: Some(why.clone()),
                    effort: i.effort,
                    ..Default::default()
                }),
                None => {}
            }
        }
        return Ok(Landed::held(Held {
            replies: held,
            ..Default::default()
        }));
    }
    let wt = PathBuf::from(i.worktree.as_deref().unwrap_or_default());
    if git::unfinished(&wt)? {
        return Ok(Landed::failed(
            format!("the agent left work uncommitted, or a rebase half done: it is left in {}", wt.display()),
            true,
        ));
    }
    let below: Vec<&str> = i.commits[..k].iter().map(|c| c.0.as_str()).collect();
    let parent = below.last().copied().unwrap_or(i.base.as_str()).to_string();
    let now_below = git::git(&wt, &["rev-list", "--reverse", &format!("{}..{parent}", i.base)]).unwrap_or_default();
    if now_below.lines().collect::<Vec<_>>() != below {
        return Ok(Landed::failed(format!("the agent changed the patches below patch {}", k + 1), true));
    }
    let pieces = pieces_since(db, &wt, &parent, &old_id, i)?;
    if pieces.is_empty() {
        return Ok(Landed::failed(format!("nothing is left of patch {}", k + 1), true));
    }
    // A round sent back slots into the stack the work it goes on from
    // makes; any other, into the stack as it is.
    let now = db.stack(&run.stack)?.clone();
    let s = if i.on.is_some() || i.after.is_some() { started_from(db, i)? } else { now.clone() };
    let Some(at) = s.patches.iter().position(|p| *p == old_id) else {
        return Ok(Landed::failed(
            format!("patch {} changed while the agent worked on it", k + 1),
            true,
        ));
    };
    let ids: Vec<String> = pieces.iter().map(|p| p.id.clone()).collect();
    let replies = held_replies(db, i, replies);
    let mut patches = s.patches.clone();
    patches.splice(at..=at, ids.iter().cloned());
    // Nothing that accepting it would change of the stack as it is.
    if patches == now.patches {
        return Ok(Landed::held(Held {
            replies,
            ..Default::default()
        }));
    }
    for p in &pieces {
        db.add_patch(p.clone())?;
    }
    // Checked now, as it would be when accepted: nothing else changes the
    // stack until then.
    db.verify(&Stack {
        patches: patches.clone(),
        ..s.clone()
    })?;
    let op = match pieces.len() {
        1 => format!("agent: patch {}", at + 1),
        n => format!("agent: patch {} split in {n}", at + 1),
    };
    Ok(Landed::held(Held {
        stack: Some(crate::held::Change {
            patches,
            base: s.base.clone(),
            op,
            of: format!("patch {}", at + 1),
            from: None,
        }),
        replies,
        ..Default::default()
    }))
}

/// The patches the commits after `parent` in `wt` are: the one keeping its
/// commit, its diff or its title, else the first, a new version of
/// `old_id`, or `old_id` itself when nothing but where its lines are has
/// changed and its diff as stored leaves its files as that commit does
/// (`plan_checked`), its message and author as stored where the agent left
/// them as git gave them back (`written`); the rest new.
fn pieces_since(db: &Db, wt: &Path, parent: &str, old_id: &str, i: &Info) -> Result<Vec<Patch>> {
    let drafts = read_back(wt, &format!("{parent}..HEAD"))?;
    let written = written(wt, i);
    let old = db.patch(old_id).clone();
    let mut pieces = plan_checked(std::slice::from_ref(&old), drafts, &written, Some((wt, parent)));
    // Neither its commit, its diff nor its title kept: the first commit is
    // the patch's new version all the same, the ask being on that patch.
    if !pieces.iter().any(|p| p.change == old.change)
        && let Some(first) = pieces.first_mut()
    {
        *first = crate::ops::revise(&old, crate::ops::as_stored(&old, Draft::from(&*first), &written));
    }
    Ok(pieces)
}

/// The commits of `range` in `wt` read back as patches: their mail, as
/// `git format-patch` writes it.
fn read_back(wt: &Path, range: &str) -> Result<Vec<Draft>> {
    parse_mbox(&git::git(wt, &["format-patch", "--stdout", "--no-signature", "--keep-subject", range])?)
}

/// What stacked wrote into `wt` for ask `i` to change (`Written`): the
/// commit it made of each patch, and that commit read back as the agent's
/// are, so what git gives back of the patch's message and author. Should
/// git no longer have those commits, only which patch each was is known.
fn written(wt: &Path, i: &Info) -> Written {
    let commits: HashMap<String, String> = i.commits.iter().cloned().collect();
    let made = match i.commits.last() {
        Some((top, _)) => read_back(wt, &format!("{}..{top}", i.base)).unwrap_or_default(),
        None => vec![],
    };
    let drafts = made
        .into_iter()
        .filter_map(|d| Some((commits.get(d.commit.as_deref()?)?.clone(), d)))
        .collect();
    Written { commits, drafts }
}

/// A stack change landing, held: the commits read back as a stack made from
/// the one the ask started from, which nothing else changes until the
/// review closes.
fn land_stack(db: &mut Db, run: &Run, i: &Info, result: &str, replies: &Map<String, Value>) -> Result<Landed> {
    let wt = PathBuf::from(i.worktree.as_deref().unwrap_or_default());
    if git::unfinished(&wt)? {
        return Ok(Landed::failed(
            format!("the agent left work uncommitted, or a rebase half done: it is left in {}", wt.display()),
            true,
        ));
    }
    let _ = run;
    let from = db.node(i.from)?.clone();
    // A round sent back goes on from the stack the work it was sent back
    // from makes: its patches the ones the commits are new versions of.
    let start = started_from(db, i)?;
    // Rebased, the stack is on the commit claude says; else on its base,
    // which must still be under it.
    let said = last_object(result).and_then(|o| o.get("base").map(|v| str_of(Some(v)))).filter(|b| !b.is_empty());
    let base = match &said {
        Some(b) => match git::git(&wt, &["rev-parse", "--verify", "--quiet", &format!("{b}^{{commit}}")]) {
            Ok(sha) => sha.trim().to_string(),
            Err(_) => return Ok(Landed::failed(format!("the agent rebased onto {b}, which is not a commit"), true)),
        },
        None => i.base.clone(),
    };
    if git::git(&wt, &["merge-base", "--is-ancestor", &base, "HEAD"]).is_err() {
        return Ok(Landed::failed(
            format!("the stack is no longer on {}, and the agent did not say onto what it went", &base[..12.min(base.len())]),
            true,
        ));
    }
    let drafts = read_back(&wt, &format!("{base}..HEAD"))?;
    let olds: Vec<Patch> = start.patches.iter().map(|id| db.patch(id).clone()).collect();
    let planned = plan_checked(&olds, drafts, &written(&wt, i), Some((&wt, &base)));
    let ids: Vec<String> = planned.iter().map(|p| p.id.clone()).collect();
    let replies = held_replies(db, i, replies);
    // Nothing changed of the stack it went on from: a guess's, the stack the
    // work under review makes; else the stack at `from`, which the change a
    // round carries is not.
    let was = if i.after.is_some() { &start } else { &from };
    let head_base = was.base.clone().unwrap_or_else(|| i.base.clone());
    if ids == was.patches && base == head_base {
        return Ok(Landed::held(Held {
            replies,
            ..Default::default()
        }));
    }
    let next = Stack {
        patches: ids.clone(),
        base: Some(base.clone()),
        parent: Some(i.from),
        op: if base == head_base {
            "agent: the stack".into()
        } else {
            format!("agent: rebase onto {}", &base[..12])
        },
        time: now(),
        ..from.clone()
    };
    crate::check::stack(&next, &planned.iter().collect::<Vec<_>>())?;
    for p in &planned {
        db.add_patch(p.clone())?;
    }
    Ok(Landed::held(Held {
        stack: Some(crate::held::Change {
            patches: ids,
            base: Some(base),
            op: next.op,
            of: "the stack".into(),
            from: None,
        }),
        replies,
        ..Default::default()
    }))
}

/// The comments on lines that their own change's version in `now` no
/// longer has go to the patch of `now` that has their line, as after a
/// split or a merge. How many moved.
pub(crate) fn follow_lines(db: &mut Db, olds: &[Patch], now: &[String]) -> Result<usize> {
    let patches: Vec<Patch> = now.iter().map(|id| db.patch(id).clone()).collect();
    let keys: Vec<(String, HashSet<String>)> = patches
        .iter()
        .map(|p| (p.change.clone(), crate::view::note_keys(p)))
        .collect();
    let mut moved = 0;
    for old in olds {
        let own = keys.iter().find(|(c, _)| *c == old.change);
        let notes: Vec<Note> = db.notes(&old.change).into_iter().cloned().collect();
        for n in notes {
            if n.line.is_empty() || own.is_some_and(|(_, k)| k.contains(&n.line)) {
                continue;
            }
            // A key is the change's, then the line's: put it under each other.
            let Some((_, rest)) = n.line.split_once('\0') else { continue };
            for (change, k) in &keys {
                if *change == old.change {
                    continue;
                }
                let key = format!("{}\0{rest}", mode(change));
                if k.contains(&key) {
                    let id = if n.id == n.line { key.clone() } else { n.id.clone() };
                    db.set_note(Note {
                        change: change.clone(),
                        id,
                        line: key,
                        ..n.clone()
                    })?;
                    db.set_note(Note {
                        text: String::new(),
                        ..n.clone()
                    })?;
                    moved += 1;
                    break;
                }
            }
        }
    }
    Ok(moved)
}

/// Moves comment `n` to stack `name`'s own comments, `why` claude's last
/// turn, at the effort its ask was asked at: it needs more than its patch.
/// It waits there to be sent again.
pub fn promote(db: &mut Db, name: &str, n: &Note, why: &str, effort: Option<Effort>) -> Result<()> {
    let text = append_agent(&n.text, &format!("This needs more than the patch: {why}"), "moved", effort.map(Effort::name));
    moved_to_stack(db, name, n, text)
}

/// Moves comment `n` to stack `name`'s own comments as it is, its thread
/// unchanged: on a patch the stack does not hold, it would be in sight
/// nowhere. Where it was is kept with it, as for one promoted.
pub(crate) fn to_stack(db: &mut Db, name: &str, n: &Note) -> Result<()> {
    moved_to_stack(db, name, n, n.text.clone())
}

/// Comment `n` on stack `name`'s own comments, its thread `text` there,
/// where it was kept with it; closed where it was.
fn moved_to_stack(db: &mut Db, name: &str, n: &Note, text: String) -> Result<()> {
    let moved = Moved {
        change: n.change.clone(),
        id: n.id.clone(),
        line: n.line.clone(),
        file: n.file.clone(),
        preview: n.preview.clone(),
    };
    db.set_note(Note {
        change: stack_change(name),
        id: format!("moved:{}", crate::db::short_id(&n.change, &n.id)),
        line: String::new(),
        text,
        moved: Some(moved),
        queued: None,
        failed: None,
        ..n.clone()
    })?;
    db.set_note(Note {
        text: String::new(),
        ..n.clone()
    })
}

/// Moves a promoted comment back where it came from.
pub fn move_back(db: &mut Db, n: &Note) -> Result<()> {
    let Some(m) = n.moved.clone() else {
        return err("it was not moved");
    };
    db.set_note(Note {
        change: m.change,
        id: m.id,
        line: m.line,
        file: m.file,
        preview: m.preview,
        moved: None,
        queued: None,
        failed: None,
        ..n.clone()
    })?;
    db.set_note(Note {
        text: String::new(),
        ..n.clone()
    })
}

/// The comments an answer opened, to file: on the lines they quote, within
/// the ask's scope, each opened by the comment that asked.
fn opened_notes(db: &Db, run: &Run, i: &Info, result: &str) -> Result<Vec<Note>> {
    let notes = parse_review_notes(result).unwrap_or_default();
    // The patches by their places in the stack it was asked on: a
    // follow-up's, the stack the work it follows up makes.
    let from = started_from(db, i)?;
    let asked = i.notes.first().map(|n| (n.change.clone(), n.id.clone()));
    let mut out = Vec::new();
    for (n, note) in notes.iter().enumerate() {
        let at = note
            .get("patch")
            .and_then(|v| v.as_u64().or_else(|| v.as_str()?.parse().ok()))
            .map(|n| n as usize);
        let Some(id) = at.and_then(|a| a.checked_sub(1)).and_then(|a| from.patches.get(a)) else {
            continue;
        };
        let p = db.patch(id).clone();
        if let Scope::Patch(c) = &i.scope
            && *c != p.change
        {
            continue;
        }
        let file = str_of(note.get("file"));
        let text = str_of(note.get("line"));
        let side = str_of(note.get("side")).chars().next().unwrap_or(' ');
        // No line, a file of the patch: on the file as a whole.
        let whole = text.trim().is_empty().then(|| crate::view::file_keys(&p).into_iter().find(|(f, _)| *f == file)).flatten();
        let (line, file, text) = match (whole, find_line(&note_lines(&p), &file, side, &text)) {
            (Some((f, k)), _) => (k, f.clone(), f),
            (None, Some(k)) => (k, file, text),
            (None, None) => (title_key(&p), MSG_PATH.to_string(), text),
        };
        out.push(Note {
            change: p.change.clone(),
            id: format!("agent:{}:{n}", run.id),
            line,
            text: fenced(
                &agent_open(Way::Answer.name(), i.effort.map(Effort::name), asked_on(db, i)),
                &crate::refs::attach(
                    str_of(note.get("text")).trim(),
                    &crate::refs::resolved(crate::refs::of_json(note.get("refs")), Some(1), &|c| named_comment(db, i, &from.name, c)),
                ),
            ),
            file,
            preview: text.chars().take(80).collect(),
            by: Some("agent".into()),
            opened_by: asked.clone(),
            time: now(),
            ..Default::default()
        });
    }
    Ok(out)
}

/// A build and test's findings, to file on each patch's title, opened by
/// the comment that asked.
fn test_notes(db: &Db, run: &Run, i: &Info, result: &str) -> Result<Vec<Note>> {
    let found = last_object(result)
        .and_then(|o| o.get("patches").and_then(Value::as_object).cloned())
        .unwrap_or_default();
    // The patches by their places in the stack it tested: a follow-up's,
    // the stack the work it follows up makes, which is no state of the
    // stack yet, but of that work.
    let from = started_from(db, i)?;
    let tested = match i.on.as_deref().filter(|_| i.follow) {
        Some(_) if from.patches != db.node(i.from)?.patches || from.base != db.node(i.from)?.base => "tested on the stack this work makes".to_string(),
        _ => format!("tested @{}", i.from),
    };
    let asked = i.notes.first().map(|n| (n.change.clone(), n.id.clone()));
    let mut out = Vec::new();
    for (at, id) in from.patches.iter().enumerate() {
        let Some(v) = found.get(&(at + 1).to_string()) else { continue };
        let p = db.patch(id).clone();
        if let Scope::Patch(c) = &i.scope
            && *c != p.change
        {
            continue;
        }
        out.push(Note {
            change: p.change.clone(),
            id: format!("agent:{}:test:{}", run.id, at + 1),
            line: title_key(&p),
            text: {
                let (found, refs) = crate::refs::text_and_refs(v);
                let refs = crate::refs::resolved(refs, Some(1), &|c| named_comment(db, i, &from.name, c));
                let open = agent_open(Way::Test.name(), i.effort.map(Effort::name), None);
                fenced(&open, &crate::refs::attach(&format!("{} ({tested})", found.trim()), &refs))
            },
            file: MSG_PATH.into(),
            preview: p.title().chars().take(80).collect(),
            by: Some("agent".into()),
            opened_by: asked.clone(),
            time: now(),
            ..Default::default()
        });
    }
    Ok(out)
}

/// For claude, while it changes a patch: whether the patches above still
/// apply on what is in its worktree now.
pub fn fits(db_path: &Path, id: &str) -> Result<String> {
    let db = Db::load(db_path)?;
    let run = db.run(id).ok_or_else(|| Error(format!("no run {id}")))?;
    let i = info(run).ok_or_else(|| Error(format!("run {id} is not an ask")))?;
    let (Some((old_id, k)), Some(wt)) = (i.patch.clone(), i.worktree.clone()) else {
        return err("this ask is not changing a patch");
    };
    let wt = PathBuf::from(wt);
    let parent = i.commits[..k].last().map_or(i.base.clone(), |c| c.0.clone());
    let pieces = pieces_since(&db, &wt, &parent, &old_id, &i)?;
    let from = started_from(&db, &i)?;
    let mut patches: Vec<Patch> = from.patches[..k].iter().map(|p| db.patch(p).clone()).collect();
    patches.extend(pieces);
    patches.extend(from.patches[k + 1..].iter().map(|p| db.patch(p).clone()));
    crate::check::stack(&from, &patches.iter().collect::<Vec<_>>())
        .map(|_| "fits: the patches above still apply on this change".into())
        .map_err(|e| Error(format!("does not fit: {}", e.0.trim_end_matches("; nothing changed"))))
}

/// Lands every ask whose claude is gone without its wrapper landing it (it
/// was killed), or that never started; how many. So too a try push, a lint
/// job and a pull from Phabricator whose process is gone, the pull's read
/// cut short (`review::orphans`). The wrappers this process started that
/// have exited are waited for first (`reap`). Under the launch lock, the
/// database read again under it: no ask is being started meanwhile, so one
/// whose agent's pid is not written yet was left so by a starter that is
/// gone, not by one about to write it. `db`, read before, only says whether
/// anything is going, to look at.
pub fn land_orphans(db_path: &Path, db: &Db) -> usize {
    reap();
    if !db.live_runs().iter().any(|r| r.state == RunState::Started) {
        return 0;
    }
    let Ok(_launching) = launch_lock(db_path) else { return 0 };
    let Ok(read) = Db::load(db_path) else { return 0 };
    let db = &read;
    let stale: Vec<String> = db
        .live_runs()
        .into_iter()
        .filter(|r| r.ask.is_some() && r.state == RunState::Started)
        .filter(|r| match (r.pid, r.starter) {
            (Some(pid), _) => !alive(pid),
            (None, Some(starter)) => !alive(starter),
            (None, None) => now() - r.time > 120,
        })
        .map(|r| r.id.clone())
        .collect();
    stale.iter().filter(|id| land(db_path, id, Some("killed")).is_ok()).count() + pushes_orphaned(db_path, db) + crate::lint::orphans(db_path, db) + pulls_orphaned(db_path, db)
}

/// The try pushes whose process is gone before mach said it pushed, ended
/// as failed: how many. None, built without the `try-push` feature.
fn pushes_orphaned(db_path: &Path, db: &Db) -> usize {
    #[cfg(feature = "try-push")]
    return crate::try_push::orphans(db_path, db);
    #[cfg(not(feature = "try-push"))]
    {
        let _ = (db_path, db);
        0
    }
}

/// The pulls from Phabricator whose read was cut short, the process
/// reading it gone, landed as failed: how many. None, built without the
/// `moz-phab` feature.
fn pulls_orphaned(db_path: &Path, db: &Db) -> usize {
    #[cfg(feature = "moz-phab")]
    return crate::review::orphans(db_path, db);
    #[cfg(not(feature = "moz-phab"))]
    {
        let _ = (db_path, db);
        0
    }
}

/// The type of the events a run's stream starts with: the commands run to
/// start it, before its agent writes.
const STARTING: &str = "stacked";

/// The type of the event that says, first in its stream, what an ask may
/// use for its work alone.
const ALLOWED: &str = "allowed";

/// What run `id` is doing as it starts, while its agent has written
/// nothing yet: the command it runs, as one line.
pub fn starting(db: &Db, id: &str) -> Option<String> {
    let last = db.events(id).pop()?;
    (last.get("type").and_then(Value::as_str) == Some(STARTING)).then(|| str_of(last.get("text")))
}

/// The last `n` lines of what run `id`'s agent wrote, as a transcript:
/// read from the end, as it is on every redraw while the agent writes. The
/// commands run to start it come first, as `$ command`.
///
/// A turn of the agent's ends with a result, but the ask only with its
/// exit: the agent may end a turn to wait on what it started (a build in
/// the background) and go on once it is done. A result is the agent's
/// finishing only when its exit follows; one it went on after, it waited
/// there; and one that is the last, while it runs, it waits.
pub fn tail(db: &Db, id: &str, n: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    // Its thinking counted as it goes: of the counts in a row, the last.
    let mut counted = false;
    // What came after, read from the end: its exit, or more of its work.
    let (mut exit, mut more) = (false, false);
    let going = db.run(id).is_some_and(running);
    for v in db.events(id).into_iter().rev() {
        if out.len() >= n {
            break;
        }
        let count = v.get("type").and_then(Value::as_str) == Some("system")
            && v.get("subtype").and_then(Value::as_str) == Some("thinking_tokens");
        if count && counted {
            continue;
        }
        let lines = match v.get("type").and_then(Value::as_str) {
            Some("text") => vec![str_of(v.get("text"))],
            Some(STARTING) => vec![format!("$ {}", str_of(v.get("text")))],
            Some(ALLOWED) => vec![format!("✓ {ALLOWED_SAID}: {}", str_of(v.get("text")))],
            Some(STOP) => vec![format!("■ {STOPPED_SAID}")],
            Some("exit") => vec![format!("exit {}", str_of(v.get("code")))],
            Some("result") if more || (!exit && going) => {
                let said = if more { "◷ the agent waited" } else { "◷ the agent waits" };
                let words = str_of(v.get("result"));
                let mut lines = vec![match words.lines().map(str::trim).find(|l| !l.is_empty()) {
                    Some(first) => format!("{said}: {first}"),
                    None => said.to_string(),
                }];
                lines.extend(crate::claude::denials(v).into_iter().map(|d| format!("⛔ not allowed: {d}")));
                lines
            }
            _ => format_event(v),
        };
        if v.get("type").and_then(Value::as_str) == Some("exit") {
            exit = true;
        } else if works(v) {
            more = true;
        }
        if !lines.is_empty() {
            counted = count;
        }
        out.extend(lines.into_iter().rev());
    }
    out.truncate(n);
    out.reverse();
    out
}

/// Whether stream event `v` is of the agent's work: what it says or does,
/// thinks, or the end of one of its turns; not its exit, nor what its
/// session, or stacked, says of it.
fn works(v: &Value) -> bool {
    match v.get("type").and_then(Value::as_str) {
        Some("assistant" | "user" | "result") => true,
        Some("system") => v.get("subtype").and_then(Value::as_str) == Some("thinking_tokens"),
        _ => false,
    }
}

/// How long a stopped ask's agent is given to exit once sent SIGTERM,
/// before it is killed (SIGKILL); and then, to be gone.
const TERM_GRACE: std::time::Duration = std::time::Duration::from_secs(3);
const KILL_GRACE: std::time::Duration = std::time::Duration::from_secs(2);

/// What the summary of an ask you stopped starts with.
const STOPPED_SAID: &str = "you stopped the agent";

/// Stops run `id`'s agent, and lands its ask as failed, at once: said
/// first in its stream, so that whatever lands it says that you stopped
/// it; then its process group (its wrapper, claude and all) sent SIGTERM
/// and given a few seconds to exit, its wrapper waited for if this process
/// started it, and killed if it has not exited; then the ask landed. What
/// waits behind it is not started here, but by the caller (`launch`), away
/// from what it does. The run as it is then, and whether you stopped it:
/// one that had landed already, or whose agent was done as it was
/// stopped, stays as it landed.
pub fn stop(db_path: &Path, id: &str) -> Result<(Run, bool)> {
    // A lint job: the linter stopped, or the job dropped from the queue.
    if Db::load(db_path)?.run(id).is_some_and(|r| r.lint.is_some()) {
        return crate::lint::stop(db_path, id);
    }
    // Said under the lock, its wrapper's pid read with it: an ask whose
    // wrapper starts after this sees it said, and stops its agent itself.
    let (landed, pid) = Db::edit(db_path, |db| {
        let r = db.run(id).cloned().ok_or_else(|| Error(format!("no run {id}")))?;
        if r.ask.is_none() {
            return err(format!("run {id} is not an ask nor a lint job: only an ask's agent, or the linter, can be stopped"));
        }
        if r.state != RunState::Started {
            return Ok((Some(r), None));
        }
        db.add_event(id, serde_json::json!({"type": STOP}))?;
        Ok((None, r.pid))
    })?;
    if let Some(r) = landed {
        return Ok((r, false));
    }
    if let Some(pid) = pid {
        signal(pid, libc::SIGTERM);
        if !gone_within(pid, TERM_GRACE) {
            signal(pid, libc::SIGKILL);
            gone_within(pid, KILL_GRACE);
        }
    }
    // Landed under the launch lock, once its agent is gone, as any landing:
    // not while waiting for it, which its wrapper's own landing could be.
    let _launching = launch_lock(db_path)?;
    let run = file_landed(db_path, id, Some("killed"), None)?;
    let stopped = run.state == RunState::Failed && run.summary.starts_with(STOPPED_SAID);
    Ok((run, stopped))
}

/// Sends `sig` to the process group of ask wrapper `pid`: the wrapper,
/// claude and all.
fn signal(pid: u32, sig: libc::c_int) {
    if let Some(p) = process(pid) {
        unsafe { libc::kill(-p, sig) };
    }
}

/// Whether the process group of ask wrapper `pid` is gone within `within`:
/// none of its processes left but those that exited, the wrapper waited
/// for if this process started it. Looked at every few milliseconds.
fn gone_within(pid: u32, within: std::time::Duration) -> bool {
    let Some(p) = process(pid) else { return true };
    let start = std::time::Instant::now();
    loop {
        let _ = wrapper_running(pid);
        if unsafe { libc::kill(-p, 0) } != 0 {
            // The wrapper too, should it have exited since.
            let _ = wrapper_running(pid);
            return true;
        }
        if start.elapsed() >= within {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{Record, VERSION};
    use crate::thread::append_turn;

    #[test]
    fn a_rebase_plan_that_runs_a_command_is_refused() {
        assert!(check_plan("pick abc one\n# a note\nfixup def two\n\ndrop 123 three\nbreak\n").is_ok());
        let e = check_plan("pick abc one\nexec curl example.com | sh\n").unwrap_err();
        assert!(e.0.contains("line 2 runs a command (`exec curl example.com | sh`)"), "{}", e.0);
        assert!(check_plan("  x make\n").is_err(), "x is exec");
    }

    /// A database holding run `r`'s stream, `events`.
    fn streamed(events: &[Value]) -> Db {
        let mut bytes = serde_json::to_string(&Record::Stacked { version: VERSION }).unwrap() + "\n";
        for e in events {
            bytes += &serde_json::to_string(&Record::Event { run: "r".into(), event: e.clone() }).unwrap();
            bytes.push('\n');
        }
        Db::from_bytes(Path::new("t.jsonl"), bytes.as_bytes()).unwrap()
    }

    #[test]
    fn the_answer_is_asked_as_structured_output_of_what_each_ask_asks_for() {
        let s = Stack {
            name: "s".into(),
            patches: vec!["p1".into(), "p2".into()],
            base: None,
            repo: None,
            parent: None,
            op: String::new(),
            time: 0,
        };
        let i = |way: Way, scope: Scope| -> Info {
            let notes = serde_json::json!([{"label": "c1", "change": "x", "id": "1"}, {"label": "c2", "change": "x", "id": "2"}]);
            serde_json::from_value(serde_json::json!({"way": way, "scope": scope, "from": 0, "notes": notes})).unwrap()
        };
        let keys = |v: &Value| v["properties"].as_object().unwrap().keys().cloned().collect::<Vec<_>>();
        // Replies by the comments' labels, nothing else.
        let change = answer_schema(&i(Way::Change, Scope::Stack), &s);
        assert_eq!((keys(&change), change["required"].clone()), (vec!["replies".to_string(), "refused".into(), "base".into()], serde_json::json!(["replies"])));
        assert_eq!(keys(&change["properties"]["replies"]), ["c1", "c2"]);
        assert_eq!(change["properties"]["replies"]["additionalProperties"], false);
        assert_eq!(keys(&answer_schema(&i(Way::Change, Scope::Patch("x".into())), &s)), ["replies", "refused", "promote"]);
        // Each tool it was refused, every ask's: as it called it, and why.
        let refused = &change["properties"]["refused"]["items"];
        assert_eq!((refused["required"].clone(), refused["properties"]["why"]["anyOf"][1]["required"].clone()), (serde_json::json!(["tool", "why"]), serde_json::json!(["text"])));
        // An answer's comments; a build and test's findings by the patches'
        // numbers.
        let answer = answer_schema(&i(Way::Answer, Scope::Stack), &s);
        assert_eq!(answer["properties"]["comments"]["items"]["required"], serde_json::json!(["patch", "file", "line", "text"]));
        let test = answer_schema(&i(Way::Test, Scope::Stack), &s);
        assert_eq!(keys(&test["properties"]["patches"]), ["1", "2"]);
        // What is written: a string, or one with its references.
        let written = &test["properties"]["replies"]["properties"]["c1"]["anyOf"];
        assert_eq!((written[0]["type"].clone(), written[1]["required"].clone()), (serde_json::json!("string"), serde_json::json!(["text"])));
    }

    /// A try push's ask: given what an answer is, and its own list; asked
    /// for its tasks, each its label and why, and how many times each runs;
    /// reading only, it runs as answers do, and from the batch each on its
    /// own.
    #[cfg(feature = "try-push")]
    #[test]
    fn a_try_push_ask_reads_only_with_its_own_list_and_is_asked_for_its_tasks() {
        let s = Stack {
            name: "s".into(),
            patches: vec!["p1".into()],
            base: None,
            repo: None,
            parent: None,
            op: String::new(),
            time: 0,
        };
        let i: Info = serde_json::from_value(serde_json::json!({"way": "try", "scope": "stack", "from": 0, "notes": [{"label": "c1", "change": "x", "id": "1"}]})).unwrap();
        let schema = answer_schema(&i, &s);
        let keys: Vec<&String> = schema["properties"].as_object().unwrap().keys().collect();
        assert_eq!(keys, ["replies", "refused", "tasks", "rebuild"]);
        let task = &schema["properties"]["tasks"]["items"];
        assert_eq!(task["required"], serde_json::json!(["label", "why"]));
        assert_eq!(task["properties"]["why"]["anyOf"][1]["required"], serde_json::json!(["text"]), "why written as anything is, with references");
        assert_eq!((schema["properties"]["rebuild"]["minimum"].clone(), schema["properties"]["rebuild"]["maximum"].clone()), (serde_json::json!(2), serde_json::json!(20)));
        let agent = crate::config::Agent {
            answer_tools: vec!["Bash(curl *)".into()],
            try_tools: vec!["mcp__moz__*".into()],
            ..Default::default()
        };
        let cmd = "stacked -f db";
        let tools = tools_for(Way::Try, cmd, &agent, &Linter::None);
        assert!(tools.starts_with(&answer_given(cmd)), "what an answer is given: {tools}");
        assert!(tools.contains("mcp__moz__*") && !tools.contains("Bash(curl *)"), "its own list, not the answers': {tools}");
        assert!(allow_more(Way::Try).contains("; agent.try_tools lets the asks that select a try push's tasks use more"));
        assert!(reads(Way::Try) && each_on_its_own(Way::Try) && !reads(Way::Test));
        assert_eq!(Way::Try.said(), "try push");
    }

    #[test]
    fn the_answer_is_its_structured_output_when_it_gives_one() {
        let said = serde_json::json!({"type": "result", "result": "Done: {\"replies\": {\"c1\": \"as said\"}}"});
        let structured = serde_json::json!({"type": "result", "result": "", "structured_output": {"replies": {"c1": "as given"}}});
        assert_eq!(replies_of(&result_text(&[&said]))["c1"], "as said");
        assert_eq!(replies_of(&result_text(&[&said, &structured]))["c1"], "as given");
        let failed = serde_json::json!({"type": "result", "is_error": true, "subtype": "error_max_structured_output_retries"});
        assert_eq!(ended_badly(&[&failed]).as_deref(), Some("its answer never fit what it was asked for, tried again and again"));
        assert_eq!(ended_badly(&[&said]), None);
    }

    #[test]
    fn replies_are_found_after_prose_holding_braces() {
        let text = "Left: `Provider.${this.name}` @r1.\n\n```json\n{\"replies\": {\"c1\": {\"text\": \"Done.\"}}}\n```";
        assert_eq!(replies_of(text)["c1"]["text"], "Done.");
        assert!(last_object("no object { here").is_none());
        // Prose after it, braces in it too: still the answer.
        let after = format!("{text}\nThen `${{more}}`, as fn f() {{}} does.");
        assert_eq!(replies_of(&after)["c1"]["text"], "Done.");
    }

    #[test]
    fn leaving_says_where_each_agent_still_running_works() {
        let run = |id: &str, what: &str, pid: Option<u32>, state: RunState, ask: Value| Run {
            id: id.into(),
            what: what.into(),
            stack: "s".into(),
            state,
            pid,
            request: format!("why\n{id}?"),
            ask: Some(ask),
            ..Default::default()
        };
        let ask = |worktree: Option<&str>| serde_json::json!({"way": "change", "scope": "stack", "from": 0, "notes": [], "repo": "/repo", "worktree": worktree});
        let me = Some(std::process::id());
        let runs = [
            run("r1", "change", me, RunState::Started, ask(Some("/repo/.stacked/s/agent"))),
            // An answer, in the repository as it is.
            run("r2", "answer", me, RunState::Started, ask(None)),
            // Its process gone, or landed: not running.
            run("r3", "change", Some(i32::MAX as u32), RunState::Started, ask(Some("/w"))),
            run("r4", "change", me, RunState::Done, ask(Some("/w"))),
            // Being started, where it works not known yet.
            Run { starter: me, ..run("r5", "change", None, RunState::Started, ask(None)) },
        ];
        let mut bytes = serde_json::to_string(&Record::Stacked { version: VERSION }).unwrap() + "\n";
        for r in runs {
            bytes += &(serde_json::to_string(&Record::Run(r)).unwrap() + "\n");
        }
        let db = Db::from_bytes(Path::new("t.jsonl"), bytes.as_bytes()).unwrap();
        assert_eq!(
            still_running(&db),
            [
                "r1 change s \"why r1?\" works in /repo/.stacked/s/agent",
                "r2 answer s \"why r2?\" works in /repo",
                "r5 change s \"why r5?\" is being started, its worktree being made",
            ]
        );
    }

    #[test]
    fn the_stream_starts_with_the_commands_run_to_start_the_agent() {
        let step = |t: &str| serde_json::json!({"type": STARTING, "text": t});
        let mut events = vec![step("git worktree add --detach /tmp/stacked-r 750b26ffa4ae")];
        // While the agent has written nothing, what it is doing is the last
        // command, as one line.
        assert_eq!(starting(&streamed(&events), "r").as_deref(), Some("git worktree add --detach /tmp/stacked-r 750b26ffa4ae"));
        events.push(step("git am --keep-non-patch, 3 patches"));
        assert_eq!(starting(&streamed(&events), "r").as_deref(), Some("git am --keep-non-patch, 3 patches"));
        // Once it writes, it is started; the commands stay at the top of
        // what it wrote.
        events.push(serde_json::json!({"type": "system", "subtype": "init", "model": "m"}));
        let db = streamed(&events);
        assert_eq!(starting(&db, "r"), None);
        assert_eq!(
            tail(&db, "r", usize::MAX),
            ["$ git worktree add --detach /tmp/stacked-r 750b26ffa4ae", "$ git am --keep-non-patch, 3 patches", "● session started · m"]
        );
        assert_eq!(starting(&streamed(&[]), "r"), None);
    }

    #[test]
    fn the_stream_says_the_agent_is_thinking() {
        // As claude writes it, its thinking kept from the stream: counted,
        // a block with no words in it.
        let count = |n: u64| serde_json::json!({"type": "system", "subtype": "thinking_tokens", "estimated_tokens": n});
        let mut events = vec![
            serde_json::json!({"type": "system", "subtype": "dev_intent", "kind": "android_app"}),
            serde_json::json!({"type": "system", "subtype": "init", "model": "claude-opus-5-5"}),
        ];
        events.extend([50, 200, 350].map(count));
        // Thinking still: its count as it is now, one line.
        let db = streamed(&events);
        assert_eq!(tail(&db, "r", 10), ["● session started · claude-opus-5-5", "✻ thinking · ~350 tokens"]);
        events.extend([500, 688].map(count));
        events.extend([
            serde_json::json!({"type": "assistant", "message": {"content": [{"type": "thinking", "thinking": "", "signature": "x"}]}}),
            serde_json::json!({"type": "assistant", "message": {"content": [{"type": "tool_use", "name": "Bash", "input": {"command": "git status --short"}}]}}),
            serde_json::json!({"type": "user", "message": {"content": [{"type": "tool_result", "content": "clean"}]}}),
            count(120),
            count(240),
            serde_json::json!({"type": "assistant", "message": {"content": [{"type": "text", "text": "Nothing changed."}]}}),
            serde_json::json!({"type": "result", "result": "{}"}),
            serde_json::json!({"type": "exit", "code": 0}),
        ]);
        let db = streamed(&events);
        assert_eq!(
            tail(&db, "r", 20),
            [
                "● session started · claude-opus-5-5",
                "✻ thinking · ~688 tokens",
                "⚙ Bash: git status --short",
                "  ↳ clean",
                "✻ thinking · ~240 tokens",
                "Nothing changed.",
                "✓ the agent finished",
                "exit 0",
            ]
        );
        assert_eq!(tail(&db, "r", 3), ["Nothing changed.", "✓ the agent finished", "exit 0"]);
    }

    #[test]
    fn a_turn_ended_while_the_agent_runs_on_waits_and_only_its_exit_finishes() {
        let say = |text: &str| serde_json::json!({"type": "assistant", "message": {"content": [{"type": "text", "text": text}]}});
        let result = |text: &str| serde_json::json!({"type": "result", "result": text});
        // As `streamed`, its run still going: its process there.
        let going = |events: &[Value]| {
            let run = Run {
                id: "r".into(),
                state: RunState::Started,
                pid: Some(std::process::id()),
                ..Default::default()
            };
            let mut bytes = serde_json::to_string(&Record::Stacked { version: VERSION }).unwrap() + "\n";
            bytes += &(serde_json::to_string(&Record::Run(run)).unwrap() + "\n");
            for e in events {
                bytes += &(serde_json::to_string(&Record::Event { run: "r".into(), event: e.clone() }).unwrap() + "\n");
            }
            Db::from_bytes(Path::new("t.jsonl"), bytes.as_bytes()).unwrap()
        };
        // Its build started in the background, its turn over: it waits, its
        // process running on.
        let mut events = vec![say("Building."), result("\nThe build is still running.\nI will test once it ends.")];
        assert_eq!(tail(&going(&events), "r", 2), ["Building.", "◷ the agent waits: The build is still running."]);
        // Gone, nothing after it: it finished there.
        assert_eq!(tail(&streamed(&events), "r", 1), ["✓ the agent finished"]);
        // Woken by the build's end, it went on, then finished.
        events.extend([say("Built; 3 tests pass."), result("{}"), serde_json::json!({"type": "exit", "code": 0})]);
        assert_eq!(
            tail(&going(&events), "r", 5),
            ["Building.", "◷ the agent waited: The build is still running.", "Built; 3 tests pass.", "✓ the agent finished", "exit 0"]
        );
    }

    #[test]
    fn the_agent_is_told_to_point_at_the_stack_before_the_web() {
        // A web link is one of the references it may give: in the prompt,
        // as an example, and in its answer's schema.
        assert!(REFS.contains("or, to point at a page of the web, {\"url\": \"https://example.com/a/b\" (its whole address, https or http)}"), "{REFS}");
        let s = Stack {
            name: "s".into(),
            patches: vec!["p1".into()],
            base: None,
            repo: None,
            parent: None,
            op: String::new(),
            time: 0,
        };
        let notes = serde_json::json!([{"label": "c1", "change": "x", "id": "1"}]);
        let i: Info = serde_json::from_value(serde_json::json!({"way": Way::Answer, "scope": Scope::Stack, "from": 0, "notes": notes})).unwrap();
        let schema = answer_schema(&i, &s);
        let given = &schema["properties"]["replies"]["properties"]["c1"]["anyOf"][1]["properties"]["refs"]["items"]["properties"];
        assert_eq!(given.as_object().unwrap().keys().collect::<Vec<_>>(), ["file", "lines", "patch", "comment", "url"]);
        assert_eq!(schema["properties"]["comments"]["items"]["properties"]["refs"]["items"]["properties"]["url"]["type"], "string");
        // Told to point at the stack first, at lines or a comment, and at the
        // web only for what nothing of the stack's says.
        let first = REFS.find("Point at the stack first: when the code or the comments of the stack say what you mean, point at those lines").expect(REFS);
        assert!(first > REFS.find("{\"url\"").unwrap(), "{REFS}");
        assert!(
            REFS[first..].contains(
                "or at that comment, rather than at a page of the web. A web link is for what nothing in the stack, the repository or its comments says"
            ),
            "{REFS}"
        );
        // A link it gave before, said as one in the thread it is told.
        let text = append_turn("why?", Role::Agent, "See @r1.\n\n```refs\nr1 https://example.com/a\n```", Some("answer"));
        assert_eq!(transcript(&text), "The user: why?\nYou (the agent, earlier): See @r1.\n(pointing at: r1, the web link https://example.com/a)");
    }

    #[test]
    fn each_kind_of_ask_may_use_what_stacked_gives_it_then_what_the_settings_add() {
        let cmd = "stacked -f db";
        let mine = crate::config::Agent {
            change_tools: vec!["Bash(make check *)".into()],
            answer_tools: vec!["Bash(lint *)".into(), crate::setup::SKILL.into()],
            tools: vec!["mcp__moz__*".into()],
            schema: false,
            ..Default::default()
        };
        let none = crate::config::Agent { schema: false, ..Default::default() };
        // An answer is given what reads the code and the stack, nothing that
        // changes anything; curl and jq are no longer its own.
        let given = answer_given(cmd);
        assert!(given.contains("Read") && given.contains("Bash(stacked -f db comments *)"), "{given}");
        for no in ["Edit", "Write", "Bash(git commit", "Bash(git push", "Bash(git rebase", "Bash,", "curl", "jq"] {
            assert!(!given.contains(no), "{no} in {given}");
        }
        // Then its list, then every ask's; its list being curl and jq unless
        // the settings say otherwise, as they were before there were lists.
        // The skills, let answers, are theirs alone.
        assert_eq!(tools_for(Way::Answer, cmd, &mine, &Linter::None), format!("{given},Bash(lint *),Skill,mcp__moz__*"));
        assert_eq!(tools_for(Way::Answer, cmd, &none, &Linter::None), format!("{given},Bash(curl *),Bash(jq *)"));
        // A change: git's history, what reads, stacked's fits and export; then
        // its own list, none unless set, then every ask's.
        let change = format!("{},Bash({cmd} fits *),Bash({cmd} export *)", allowed_tools());
        assert_eq!(tools_for(Way::Change, cmd, &mine, &Linter::None), format!("{change},Bash(make check *),mcp__moz__*"));
        assert_eq!(tools_for(Way::Change, cmd, &none, &Linter::None), change);
        // A build and test: Bash at large, and every ask's alone.
        assert_eq!(tools_for(Way::Test, cmd, &mine, &Linter::None), format!("Bash,{},mcp__moz__*", allowed_tools()));
        // Asked for structured output, the tool it answers with comes after
        // what stacked gives.
        let schema = tools_for(Way::Answer, cmd, &crate::config::Agent::default(), &Linter::None);
        assert_eq!(schema, format!("{given},StructuredOutput,Bash(curl *),Bash(jq *)"));
    }

    #[test]
    fn an_ask_that_only_reads_may_run_the_commands_that_only_read_their_options_that_write_refused() {
        let cmd = "stacked -f db";
        let none = crate::config::Agent { schema: false, ..Default::default() };
        let tools_of = |way: Way| -> Vec<String> { tools_for(way, cmd, &none, &Linter::None).split(',').map(String::from).collect() };
        // An answer: the commands that only read, and git's that do.
        let answer = tools_of(Way::Answer);
        let reads = [
            "grep", "rg", "cat", "head", "tail", "wc", "ls", "find", "cut", "tr", "stat", "which", "dirname", "basename", "realpath", "diff", "cmp", "comm",
            "git show", "git log", "git diff", "git ls-files", "git rev-parse", "git blame", "git cat-file", "git ls-tree", "git merge-base", "git describe",
        ];
        for read in reads {
            assert!(answer.contains(&format!("Bash({read} *)")), "{read} in {answer:?}");
        }
        assert!(answer.iter().any(|t| t == "Grep"), "{answer:?}");
        // Not what bundles its short options, so that no rule refuses the
        // one that writes a file or runs a program; nor what can write files
        // or run programs, whatever the call; nor git at large, nor what it
        // does that changes things.
        for no in [
            "sort", "file", "git grep", "sed", "awk", "uniq", "tree", "xargs", "tee", "perl", "python3", "sh", "bash", "git commit", "git config", "git push",
        ] {
            assert!(!answer.iter().any(|t| t.starts_with(&format!("Bash({no} "))), "{no} in {answer:?}");
        }
        assert!(!answer.iter().any(|t| t == "Bash(git *)" || t == "Bash"), "{answer:?}");
        // A try push's selection, as an answer.
        #[cfg(feature = "try-push")]
        assert_eq!(tools_of(Way::Try), answer);
        // Refused what would make those write a file or run a program, and
        // the bundling ones, should a list give them by hand, on top of what
        // every ask but a build and test is refused; none twice.
        let stacked = crate::claude::refused_tools();
        let refused = refused_of(Way::Answer);
        assert_eq!(refused.join(","), format!("{stacked},{}", crate::claude::read_refused()));
        let reads_only = ["Bash(git *--output*)", "Bash(sort *-o*)", "Bash(file *-C*)", "Bash(file *--co*)", "Bash(git grep *-O*)", "Bash(git grep *--op*)"];
        assert_eq!(crate::claude::read_refused(), reads_only.join(","));
        assert!(reads_only.iter().all(|r| !stacked.split(',').any(|s| s == *r)), "{stacked}");
        #[cfg(feature = "try-push")]
        assert_eq!(refused_of(Way::Try), refused);
        // What they refuse: each call that writes a file or runs a program,
        // its long option shortened too; no call that only reads, of those
        // given or of those a list may give.
        let refuses = |call: &str| refused.iter().any(|r| crate::refusals::lets(r, "Bash", &Value::Null, Some(call)));
        for writes in [
            "sort -o out.txt a.txt",
            "sort --output=out.txt a.txt",
            "sort --co=sh a.txt",
            "git diff --output=x.diff",
            "git log -p --output x.diff",
            "git show HEAD --output=x.diff",
            "git grep -O less Foo",
            "git grep -Oless Foo",
            "git grep --open-files-in-pager=vim Foo",
            "git grep --open=vim Foo",
            "file -C -m magic",
            "file --compile -m magic",
            "find . -exec rm {} ;",
            "rg --pre cat Foo",
            "rg --hostname-bin=./x --hyperlink-format=default Foo",
        ] {
            assert!(refuses(writes), "{writes}");
        }
        for reads in ["sort -u a.txt", "sort -k2 -n --check a.txt", "git diff HEAD~1", "git log --oneline -5", "git grep -n -o Foo", "file -b a.txt", "rg -n Foo src", "find . -name x"] {
            assert!(!refuses(reads), "{reads}");
        }
        // A change: given what it was, which lets it write with sort, sed
        // and the rest, and refused what every ask but a build and test is,
        // rg's hostname program and sort's shortened one now among them. A
        // build and test: nothing.
        let change = tools_of(Way::Change);
        assert_eq!(change.join(","), format!("{},Bash({cmd} fits *),Bash({cmd} export *)", allowed_tools()));
        assert!(change.iter().any(|t| t == "Bash(sed *)") && !change.iter().any(|t| t == "Bash(diff *)" || t == "Bash(git grep *)"), "{change:?}");
        assert_eq!(refused_of(Way::Change).join(","), stacked);
        let changes = |call: &str| refused_of(Way::Change).iter().any(|r| crate::refusals::lets(r, "Bash", &Value::Null, Some(call)));
        assert!(changes("rg --hostname-bin=./x Foo") && changes("sort --co=sh a.txt") && !changes("sort -o out.txt a.txt"));
        assert!(refused_of(Way::Test).is_empty());
    }

    #[test]
    fn a_change_is_given_the_checkout_s_linter_and_no_other_ask_is() {
        let cmd = "stacked -f db";
        let agent = crate::config::Agent { change_tools: vec!["Bash(make check *)".into()], schema: false, ..Default::default() };
        let change = format!("{},Bash({cmd} fits *),Bash({cmd} export *)", allowed_tools());
        // ./mach at the top of the checkout: its lint, with or without
        // --fix, after what stacked gives and before what the settings add.
        assert_eq!(tools_for(Way::Change, cmd, &agent, &Linter::Mach), format!("{change},Bash(./mach lint *),Bash(make check *)"));
        // One the checkout sets: as narrow as a refusal of it is offered.
        let set = |c: &str| tools_for(Way::Change, cmd, &agent, &Linter::Set(c.into()));
        assert_eq!(set("eslint --fix"), format!("{change},Bash(eslint *),Bash(make check *)"));
        assert_eq!(set("python3 -m black"), format!("{change},Bash(python3 -m black *),Bash(make check *)"));
        assert_eq!(set("npx eslint --fix"), format!("{change},Bash(npx eslint *),Bash(make check *)"));
        assert_eq!(set("cargo clippy --fix"), format!("{change},Bash(cargo clippy *),Bash(make check *)"));
        // One no narrow rule lets run, lint off, or no linter: nothing.
        let wide = ["sh -c 'eslint --fix \"$@\"' sh", "./mach --verbose lint", "npx", "cargo"].map(|c| Linter::Set(c.into()));
        for none in wide.into_iter().chain([Linter::Off, Linter::None]) {
            assert_eq!(tools_for(Way::Change, cmd, &agent, &none), format!("{change},Bash(make check *)"), "{none:?}");
        }
        // An answer, a try push's selection, a build and test: never.
        let ways = [Way::Answer, Way::Test, #[cfg(feature = "try-push")] Way::Try];
        for way in ways {
            let tools = tools_for(way, cmd, &agent, &Linter::Mach);
            assert_eq!(tools, tools_for(way, cmd, &agent, &Linter::None), "{way:?}");
            assert!(!tools.contains("lint"), "{way:?}: {tools}");
        }
        // What the prompt of a change given it says.
        assert_eq!(
            linter_told(crate::lint::MACH),
            " This checkout's linter is `./mach lint --fix`: you may run it on the files you change, to check them."
        );
    }

    #[test]
    fn the_ask_s_effort_goes_after_the_settings_arguments_in_place_of_theirs() {
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        // The settings' arguments as they are, then the ask's effort.
        assert_eq!(at_effort(args(&["--model", "opus"]), Some(Effort::Max)), args(&["--model", "opus", "--effort", "max"]));
        // The settings' own effort, either way it is written, taken out.
        let theirs = args(&["--effort", "low", "--verbose", "--effort=medium"]);
        assert_eq!(at_effort(theirs.clone(), Some(Effort::Xhigh)), args(&["--verbose", "--effort", "xhigh"]));
        // An ask of before, with none: the settings' as they are.
        assert_eq!(at_effort(theirs.clone(), None), theirs);
        // The levels as claude takes them, the least first, high unless
        // picked.
        assert_eq!(Effort::ALL.map(Effort::name), ["low", "medium", "high", "xhigh", "max"]);
        assert_eq!((Effort::default(), Effort::parse("xhigh"), Effort::parse("huge")), (Effort::High, Some(Effort::Xhigh), None));
        assert_eq!(Effort::listed(), "low, medium, high, xhigh or max");
        assert!(Effort::Low < Effort::Max);
    }

    #[test]
    fn the_agent_is_told_the_thread_as_it_stands() {
        // Its turn trimmed, it is told it as you left it; rewound from your
        // question, it is told only the new one.
        let text = append_turn("why?", Role::Agent, "Because, at length.", Some("answer"));
        let text = append_turn(&text, Role::You, "and here?", None);
        let text = crate::thread::edit_turn(&text, 1, "Because.").unwrap();
        assert_eq!(transcript(&text), "The user: why?\nYou (the agent, earlier): Because.\nThe user: and here?");
        let text = crate::thread::rewind(&text, 0, "why, really?").unwrap();
        assert_eq!(transcript(&text), "The user: why, really?");
        // A turn deleted the first way is told to no one.
        assert_eq!(transcript("why?\n```deleted agent answer\n> Because.\n```\nand here?"), "The user: why?\nThe user: and here?");
        // The effort its earlier turns were asked at is not told: they are
        // told as ever.
        let text = append_agent("why?", "Because.", "answer", Some("max"));
        let text = append_agent(&append_turn(&text, Role::You, "and here?", None), "Here too.", "change", Some("low"));
        assert!(text.contains("```agent answer effort max\n") && text.contains("```agent change effort low\n"), "{text}");
        assert_eq!(transcript(&text), "The user: why?\nYou (the agent, earlier): Because.\nThe user: and here?\nYou (the agent, earlier): Here too.");
    }

    #[test]
    fn the_agent_is_told_a_reviewer_s_edit_or_their_comment_read_anew_and_the_turn_it_redacts_in_plain_words() {
        // ann's comment, your reply, her edit of it on Phabricator: her
        // comment as it was, then what answered it, then the edit.
        let text = [
            fenced("reviewer ann redacted", "Hi Da"),
            "Hi ann! My name is Damien".to_string(),
            fenced("reviewer ann edit Hi Da", "Hi Damien!"),
        ]
        .join("\n");
        assert_eq!(
            transcript(&text),
            "ann (a reviewer, on Phabricator), ann's comment (redacted: ann edited it on Phabricator, and the edit below replaces it): Hi Da\n\
             The user: Hi ann! My name is Damien\n\
             ann (a reviewer, on Phabricator), ann's edit of the comment that said \"Hi Da\": Hi Damien!"
        );
        // Her comment read again with the suggested edit Phabricator gives
        // with it, which no one edited: told for what it is.
        let now = "Hi Da\n\nann suggests this edit of line 2:\n```diff\n-hi\n+Hi\n```";
        let text = [fenced("reviewer ann redacted", "Hi Da"), fenced("reviewer ann anew Hi Da", now)].join("\n");
        assert_eq!(
            transcript(&text),
            format!(
                "ann (a reviewer, on Phabricator), ann's comment (redacted: stacked had not read the suggested edit Phabricator gives with this comment; the turn below, the same comment read with it, replaces it): Hi Da\n\
                 ann (a reviewer, on Phabricator), ann's comment that said \"Hi Da\", with its suggested edit: {now}"
            )
        );
    }

    /// A git directory of the test's own, `tmp/NAME/.git`: its path.
    fn git_dir(tmp: &Path, name: &str) -> String {
        let dir = tmp.join(name).join(".git");
        std::fs::create_dir_all(&dir).unwrap();
        dir.to_string_lossy().into_owned()
    }

    #[test]
    fn a_plan_s_folder_is_pruned_once_its_git_directory_is_gone() {
        // The plans' folder is the test's own: never the temporary
        // directory's, which every stacked shares.
        let dir = tempfile::tempdir().unwrap();
        let (tmp, base) = (dir.path(), dir.path().join("plans"));
        let live = plan_folder(&base, &git_dir(tmp, "live")).unwrap();
        let rebasing = plan_folder(&base, &git_dir(tmp, "rebasing")).unwrap();
        std::fs::create_dir(tmp.join("rebasing/.git/rebase-merge")).unwrap();
        let dead = plan_folder(&base, &git_dir(tmp, "dead")).unwrap();
        std::fs::write(dead.join("REBASE_TODO"), "pick 0123abc one\n").unwrap();
        let running = plan_folder(&base, &git_dir(tmp, "running")).unwrap();
        for gone in ["dead", "running"] {
            std::fs::remove_dir_all(tmp.join(gone)).unwrap();
        }
        // Each folder names its git directory in full, as its name cannot.
        assert_eq!(plan_gitdir(&live), Some(tmp.join("live/.git")));
        // A folder that a stacked of before made names none; nor does one
        // whose file names a git directory that is not the folder's.
        let before = base.join("0123456789ab");
        std::fs::create_dir(&before).unwrap();
        let written_over = base.join("ba9876543210");
        std::fs::create_dir(&written_over).unwrap();
        std::fs::write(written_over.join(GITDIR), format!("{}\n", tmp.join("dead/.git").display())).unwrap();
        assert_eq!(plan_gitdir(&written_over), None);
        // What is not a plan's folder is never touched.
        let other = base.join("notes");
        std::fs::create_dir(&other).unwrap();
        std::fs::write(base.join("0123456789ac"), "a file\n").unwrap();
        // The folder whose git directory is gone goes; not one whose git
        // directory is there, rebase or not; not one that an ask still
        // running writes its plan in, though its git directory is gone.
        let busy = HashSet::from([running.clone()]);
        let now = std::time::SystemTime::now();
        prune_plans(&base, &busy, now);
        assert!(!dead.exists());
        for kept in [&live, &rebasing, &running, &before, &written_over, &other, &base.join("0123456789ac")] {
            assert!(kept.exists(), "{} is kept", kept.display());
        }
        // The folders that name no git directory go once nothing in them
        // has changed for a day.
        prune_plans(&base, &busy, now + std::time::Duration::from_secs(25 * 60 * 60));
        assert!(!before.exists() && !written_over.exists());
        for kept in [&live, &rebasing, &running, &other] {
            assert!(kept.exists(), "{} is kept", kept.display());
        }
        // Its ask landed, the folder whose git directory is gone goes too.
        prune_plans(&base, &HashSet::new(), now);
        assert!(!running.exists() && live.exists() && rebasing.exists());
        // An ask's rebase step, starting, makes the folder of its worktree's
        // git directory, a plan an earlier ask left there gone.
        let repo = tmp.join("repo");
        std::fs::create_dir(&repo).unwrap();
        git::git(&repo, &["init", "-q"]).unwrap();
        let gitdir = repo.canonicalize().unwrap().join(".git");
        let folder = base.join(crate::diff::sha1_12(&gitdir.to_string_lossy()));
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(folder.join("REBASE_TODO"), "drop 0123abc one\n").unwrap();
        let todo = rebase_todo(&base, &repo, &HashSet::new()).unwrap();
        assert_eq!(todo, folder.join("REBASE_TODO"));
        assert!(!todo.exists() && plan_gitdir(&folder) == Some(gitdir));
    }

    #[test]
    fn a_landed_ask_s_plan_goes_unless_a_rebase_may_still_want_it() {
        // The plans' folder and the database are the test's own.
        let dir = tempfile::tempdir().unwrap();
        let (tmp, base) = (dir.path(), dir.path().join("plans"));
        let db_path = tmp.join("db.jsonl");
        let started = |id: &str, plan: &Path| Run {
            id: id.into(),
            state: RunState::Started,
            ask: Some(serde_json::json!({"way": Way::Change, "scope": Scope::Stack, "from": 0, "notes": [], "todo": plan})),
            ..Default::default()
        };
        // Three folders, each for a git directory that is still there.
        let alone = plan_folder(&base, &git_dir(tmp, "alone")).unwrap();
        let shared = plan_folder(&base, &git_dir(tmp, "shared")).unwrap();
        let rebasing = plan_folder(&base, &git_dir(tmp, "rebasing")).unwrap();
        std::fs::create_dir(tmp.join("rebasing/.git/rebase-apply")).unwrap();
        Db::edit(&db_path, |db| {
            for (id, plan) in [("r1", &alone), ("r2", &shared), ("r3", &shared), ("r4", &rebasing)] {
                db.add_run(started(id, plan))?;
            }
            Ok(())
        })
        .unwrap();
        // Landed, failed or not, an ask's folder goes, though its worktree
        // is still there.
        assert_eq!(land(&db_path, "r1", Some("1")).unwrap().state, RunState::Failed);
        assert!(!alone.exists());
        // Not while another ask still running writes its plan there.
        land(&db_path, "r2", Some("1")).unwrap();
        assert!(shared.exists());
        land(&db_path, "r3", Some("1")).unwrap();
        assert!(!shared.exists());
        // Nor while a rebase is under way in its git directory.
        land(&db_path, "r4", Some("1")).unwrap();
        assert!(rebasing.exists());
    }

    #[test]
    fn the_plans_are_kept_where_the_environment_says() {
        // `$STACKED_TODO`, from where stacked runs; else the temporary
        // directory's, which every stacked shares.
        let named = PathBuf::from("/tmp/plans");
        assert_eq!(plans_from(|k| (k == "STACKED_TODO").then(|| named.clone().into())), named);
        assert!(plans_from(|_| Some("plans".into())).is_absolute());
        assert_eq!(plans_from(|_| None), std::env::temp_dir().join("stacked-todo"));
        assert_eq!(plans_from(|_| Some("".into())), std::env::temp_dir().join("stacked-todo"));
        // In the tests' own processes, which the binary never set up: beside
        // the database.
        assert_eq!(plans(Path::new("/tmp/t/db.jsonl")), Path::new("/tmp/t/stacked-todo"));
    }

    /// Waits until child `pid` has exited, without waiting for it: a
    /// zombie, until whoever started it waits for it.
    fn exited(pid: u32) {
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        assert_eq!(unsafe { libc::waitid(libc::P_PID, pid, &mut info, libc::WEXITED | libc::WNOWAIT) }, 0);
    }

    #[test]
    fn a_child_that_has_exited_is_not_alive_its_exit_left_to_who_started_it() {
        let mut running = Command::new("sleep").arg("60").spawn().unwrap();
        assert!(alive(running.id()));
        running.kill().unwrap();
        exited(running.id());
        // A zombie: `kill` finds it, but it is not alive.
        assert_eq!(unsafe { libc::kill(running.id() as i32, 0) }, 0, "a zombie");
        assert!(!alive(running.id()));
        // Its exit is still there for who started it, who waits for it.
        assert!(running.try_wait().unwrap().is_some(), "not waited for by alive");
        // Never 0, 1, or a pid read as a group.
        assert!(!alive(0) && !alive(1) && !alive(u32::MAX));
    }

    #[test]
    fn a_wrapper_started_here_is_waited_for_once_it_has_exited() {
        // As `start` keeps the wrapper it starts.
        let (one, two) = (Command::new("true").spawn().unwrap(), Command::new("true").spawn().unwrap());
        let (a, b) = (one.id(), two.id());
        started().extend([one, two]);
        exited(a);
        exited(b);
        assert_eq!(unsafe { libc::kill(a as i32, 0) }, 0, "a zombie");
        // Asked after: not alive, and waited for, gone.
        assert!(!alive(a));
        assert_eq!(unsafe { libc::kill(a as i32, 0) }, -1, "waited for");
        // Not asked after: waited for all the same, as a launch looks.
        reap();
        assert_eq!(unsafe { libc::kill(b as i32, 0) }, -1, "waited for");
        assert!(!started().iter().any(|c| c.id() == a || c.id() == b));
    }

    #[test]
    fn an_agent_you_stopped_lands_said_stopped_unless_it_was_done() {
        let stop = serde_json::json!({"type": STOP});
        let exit = |code: i32| serde_json::json!({"type": "exit", "code": code});
        let code = |events: &[Value], given: Option<&str>| exit_code(&streamed(events), "r", given);
        // Gone without a word: as said, else unknown.
        assert_eq!(code(&[], Some("killed")), "killed");
        assert_eq!(code(&[], None), "unknown");
        // Stopped: so, however it went, and whatever is said of it.
        assert_eq!(code(std::slice::from_ref(&stop), Some("killed")), STOPPED);
        assert_eq!(code(&[stop.clone(), exit(143)], None), STOPPED);
        // Done as you stopped it, or gone before: as it exited.
        assert_eq!(code(&[stop.clone(), exit(0)], None), "0");
        assert_eq!(code(&[exit(1), stop.clone()], None), "1");
        // Its stream says so.
        let init = serde_json::json!({"type": "system", "subtype": "init", "model": "m"});
        assert_eq!(tail(&streamed(&[init, stop]), "r", 10), ["● session started · m", "■ you stopped the agent"]);
    }

    #[test]
    fn the_launch_lock_is_held_by_one_thread_at_a_time_and_again_by_the_one_holding_it() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("db.jsonl");
        Db::edit(&path, |db| db.set_view("s", "all")).unwrap();
        // Its own file, beside the database.
        let first = launch_lock(&path).unwrap();
        assert!(tmp.path().join("db.jsonl.launch.lock").exists());
        // Taken again under it, as a landing starts what waited: held as it
        // is, and still held once that is given back.
        drop(launch_lock(&path).unwrap());
        // The database's lock is taken under it, as a launch's claim is.
        Db::edit(&path, |db| db.set_view("s", "none")).unwrap();
        // Another thread waits for it to be given back.
        let started = std::time::Instant::now();
        let (took, taken) = std::sync::mpsc::channel();
        let other = {
            let path = path.clone();
            std::thread::spawn(move || {
                let held = launch_lock(&path).map(|_| ());
                took.send(()).unwrap();
                held
            })
        };
        assert!(taken.recv_timeout(std::time::Duration::from_millis(300)).is_err(), "it waits while it is held");
        drop(first);
        taken.recv().unwrap();
        other.join().unwrap().unwrap();
        assert!(started.elapsed() >= std::time::Duration::from_millis(300));
    }

    #[test]
    #[should_panic(expected = "the launch lock asked for under the database's lock")]
    fn the_launch_lock_asked_for_under_the_database_s_lock_is_a_bug() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("db.jsonl");
        let _ = Db::edit(&path, |db| {
            db.set_view("s", "all")?;
            launch_lock(&path).map(|_| ())
        });
    }
}
