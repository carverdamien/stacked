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

use crate::claude::{allowed_tools, format_event, parse_claude_replies, parse_review_notes};
use crate::config::Workspace;
use crate::db::{
    Db, Error, Moved, Note, Patch, Queued, Record, Result, Run, RunState, Stack, Stage, Way, err,
    now, stack_change,
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
/// they were sent in: they go again as they were asked, the agent shown
/// what run `was` brought back, if it landed.
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
/// the whole stack if `whole`: they wait as ready, together, and go as soon
/// as nothing stands in their way. What they wait on, if they could not go
/// at once.
pub fn send(db_path: &Path, name: &str, notes: &[(String, String)], way: Way, whole: bool) -> Result<Option<String>> {
    queue(db_path, name, notes, way, whole)?;
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

/// Puts `notes` of stack `name` on the agent's queue, in `way`, together:
/// `launch` starts them when nothing stands in their way. The editor
/// launches elsewhere than where it draws: making a worktree takes time.
/// With `whole`, a patch's comments are sent to the whole stack: the agent
/// may change any patch, add patches anywhere, reorder them.
pub fn queue(db_path: &Path, name: &str, notes: &[(String, String)], way: Way, whole: bool) -> Result<()> {
    if notes.is_empty() {
        return err("no comment to send");
    }
    check_ahead(db_path, name);
    Db::edit(db_path, |db| {
        applies(db, name)?;
        enqueue(db, notes, way, whole, Stage::Ready, None)
    })
}

/// Sends the work run `id` holds back to the agent, as what you wrote in
/// its review asks (`held::send_back`): another round, from that work's
/// stack, whose own work lands to review in its place. What was sent,
/// said. `launch` starts it.
pub fn send_back(db_path: &Path, name: &str, id: &str, marks: &std::collections::BTreeMap<String, bool>) -> Result<String> {
    check_ahead(db_path, name);
    Db::edit(db_path, |db| {
        applies(db, name)?;
        let round = crate::held::send_back(db, id, marks)?;
        enqueue(db, &round.notes, round.way, round.whole, Stage::Ready, Some(id))?;
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
/// together, in a batch of their own; from the stack of landed work `on`,
/// if a round sent back from it.
fn enqueue(db: &mut Db, notes: &[(String, String)], way: Way, whole: bool, stage: Stage, on: Option<&str>) -> Result<()> {
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
            }),
            failed: None,
            ..n
        })?;
    }
    Ok(())
}

/// Puts `notes` of stack `name` in its batch, to be asked later in `way`
/// (to the whole stack if `whole`): each waits there until you send the
/// batch. Nothing is started.
pub fn batch(db_path: &Path, name: &str, notes: &[(String, String)], way: Way, whole: bool) -> Result<()> {
    if notes.is_empty() {
        return err("no comment to batch");
    }
    Db::edit(db_path, |db| {
        for at in notes {
            // Sent with the stack's batch: one of its comments.
            if !db.stack_notes(name).iter().any(|n| (&n.change, &n.id) == (&at.0, &at.1)) {
                return err(format!("that comment is not on {name}"));
            }
            enqueue(db, std::slice::from_ref(at), way, whole, Stage::Batch, None)?;
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

/// Sends stack `name`'s batch of what is asked in `way`: its comments turn
/// ready together, as one ask, or with `fan` (as builds and tests always
/// do) each as an ask of its own. Together, comments from more than one
/// place (the stack, or a patch) go to the whole stack, as do those
/// batched so. `launch` starts them. How many were sent.
pub fn release_batch(db_path: &Path, name: &str, way: Way, fan: bool) -> Result<usize> {
    check_ahead(db_path, name);
    Db::edit(db_path, |db| {
        let all = batched(db, name, way);
        if all.is_empty() {
            return Ok(0);
        }
        applies(db, name)?;
        if fan || way == Way::Test {
            for (at, whole) in &all {
                enqueue(db, std::slice::from_ref(at), way, *whole, Stage::Ready, None)?;
            }
        } else {
            let places: HashSet<Scope> = all.iter().filter_map(|((c, i), _)| db.note(c, i)).map(scope_of).collect();
            let whole = places.len() > 1 || all.iter().any(|(_, whole)| *whole);
            let notes: Vec<(String, String)> = all.iter().map(|(at, _)| at.clone()).collect();
            enqueue(db, &notes, way, whole, Stage::Ready, None)?;
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
    // The ready batches: stack, way, scope, comments, and the landed work a
    // round sent back starts from.
    let mut order: Vec<String> = Vec::new();
    type Batch = (String, Way, Scope, Vec<Note>, Option<String>, Option<String>);
    let mut batches: HashMap<String, Batch> = HashMap::new();
    for s in db.stacks() {
        for n in db.stack_notes(&s.name) {
            let Some(q) = n.queued.as_ref().filter(|q| !q.batched()) else { continue };
            let entry = batches.entry(q.batch.clone()).or_insert_with(|| {
                order.push(q.batch.clone());
                (s.name.clone(), q.way, asked_scope(n), Vec::new(), q.on.clone(), q.was.clone())
            });
            entry.3.push(n.clone());
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
        let (name, way, scope, notes, on, was) = batches[b].clone();
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

/// A run that could not start: failed, and its comments with it.
fn fail_start(db_path: &Path, id: &str, why: &str) -> Result<()> {
    Db::edit(db_path, |db| {
        let Some(r) = db.run(id).cloned() else { return Ok(()) };
        if let Some(i) = info(&r) {
            mark_failed(db, &i, why)?;
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
    format!("{at}, {} line `{}`", m.file, m.preview)
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

/// What an ask to answer only may run, `cmd` being stacked on its
/// database: it reads the code and the stack, changes nothing, and may read
/// the web with curl (a bug, a revision, a page of documentation), what it
/// reads through jq: each command of a pipe must be let run.
fn answer_tools(cmd: &str) -> String {
    format!(
        "Read,Grep,Glob,LS,Bash(git show *),Bash(git log *),Bash(git diff *),Bash({cmd} export *),\
         Bash({cmd} log *),Bash({cmd} tree *),Bash({cmd} versions *),Bash({cmd} interdiff *),\
         Bash({cmd} show *),Bash({cmd} comments *),Bash(curl *),Bash(jq *)"
    )
}

/// What an ask to answer only is told of the web.
const READ_THE_WEB: &str = "You may read the web with `curl` to answer (a bug on Bugzilla, a \
revision on Phabricator, a page of documentation), what it reads through `jq` if need be, and use \
the tools of MCP servers you are let use: only to read, never to send anything of this repository \
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
/// has them. Nothing if nothing is.
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
                let wanted: Vec<usize> = r.ranges.iter().flat_map(|&(x, y)| x..=y.min(all.len())).collect();
                for &k in wanted.iter().take(ATTACHED_LINES) {
                    out += &format!("    {k:>5} │ {}\n", all[k - 1]);
                }
                if wanted.len() > ATTACHED_LINES {
                    out += &format!("    … {} lines more\n", wanted.len() - ATTACHED_LINES);
                }
            }
        }
    }
    out
}

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
code. Do not push or fetch, nor stash.";

/// The most lines of a dropped change's stack diff a prompt shows.
const BEFORE_LINES: usize = 300;

/// What ask `i`, asked again, is told of what run `was` brought back before
/// it was dropped, the stack having changed: its replies and the comments it
/// opened; for a change, what it changed, its stack diff from the stack it
/// went on from, and how to read the stack it made.
fn before(db: &Db, i: &Info, was: &str, name: &str, cmd: &str) -> String {
    let Some((r, h)) = db.run(was).and_then(|r| Some((r, r.held.as_ref()?))) else { return String::new() };
    let mut out = match i.way {
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
    for sh in &i.notes {
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
/// what to answer with.
fn prompt(db: &Db, i: &Info, s: &Stack, cmd: &str, run: &str) -> String {
    let n = s.patches.len();
    let k = i.patch.as_ref().map(|(_, k)| *k);
    // The stack it started from: at its rev; a guess's, as the work it goes
    // on from makes it.
    let at = i.after.as_ref().map_or(format!("--rev {}", i.from), |w| format!("--work {w}"));
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
             edit files and `git commit --amend`, `git commit --fixup` then \
             `GIT_SEQUENCE_EDITOR=true git rebase -i --autosquash {base}`, `git rebase -i` \
             with a GIT_SEQUENCE_EDITOR script to reorder, drop or squash, `git reset` and \
             `git commit` to split, a new commit where a new patch belongs. Do not touch \
             commits below {base}. To rebase the stack onto another commit, `git rebase \
             --onto COMMIT {base}`, and say onto what: \"base\": COMMIT's full sha, in the \
             JSON you end with. {KEEP_TITLES}",
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
             {READ_THE_WEB} Besides your answers, you may open comments of your own on lines \
             of any patch, as a careful reviewer would, without repeating what is already said. {ONE_ISSUE}",
            base = i.base
        ),
        (Way::Answer, Scope::Patch(_)) if i.worktree.is_some() => format!(
            "You are answering comments on one patch of a stack, patch {k} of {n}. The current \
             directory is a git worktree made for you: the commits {base}..HEAD are the stack \
             up to that patch, the patch itself at HEAD; `git show` to read them; `{export}` \
             prints the whole stack, the patches above included. Do NOT modify anything. You \
             cannot build the project or run its tests; reason from the code. {READ_THE_WEB} \
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
             {READ_THE_WEB} Besides your answers, you may open comments of your own on lines \
             of any patch, as a careful reviewer would, without repeating what is already said. {ONE_ISSUE}",
            base = i.base
        ),
        (Way::Answer, Scope::Patch(_)) => format!(
            "You are answering comments on one patch of a stack, patch {k} of {n}, listed below \
             with the others, bottom first; they apply to commit {base}. {HERE} `{show} {k}` \
             prints patch {k}, `{show} {k} --path FILE` a file as it leaves it, whether the \
             stack changes it or not, and `{export}` the whole stack, the patches above \
             included. Do NOT modify anything: no file, no commit, no branch. You cannot build \
             the project or run its tests; reason from the code. {READ_THE_WEB} Besides your \
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
    // Another round: what it brought back was sent back.
    if i.on.is_some() {
        p.push_str(match i.way {
            Way::Change => {
                " This is another round: the user reviewed the change you made, which the worktree holds \
                 as you left it, and sent it back with the comments below: change it as they ask, from \
                 there."
            }
            _ => " This is another round: the user read what you answered, and asks again in the comments below.",
        });
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
    p.push_str(match i.way {
        Way::Change => match i.scope {
            Scope::Stack => {
                "\nEnd your reply with a JSON object: {\"replies\": {label: your reply to that \
                 comment, saying what you changed}}, labels as above ([c1] is \"c1\"); with \
                 \"base\": the commit's sha, when you rebased the stack onto another."
            }
            Scope::Patch(_) => {
                "\nEnd your reply with a JSON object: {\"replies\": {label: your reply to that \
                 comment, saying what you changed}}, labels as above ([c1] is \"c1\"); with \
                 \"promote\": why, when the comments need more than this patch."
            }
        },
        Way::Answer => {
            "\nOutput ONLY a JSON object: {\"replies\": {label: your answer}, \"comments\": \
             [{\"patch\": its number, \"file\": the path as the diff names it, \"side\": \"+\" \
             for an added line, \"-\" for a removed one, \" \" for context, \"line\": that \
             line's exact text without its leading +/-/space, \"text\": your comment, one \
             issue}]}, the comments optional, as many on one line as it has issues."
        }
        Way::Test => {
            "\nEnd your reply with a JSON object: {\"replies\": {label: what you found, with \
             the commands run}, \"patches\": {its number: what building and testing it found}}."
        }
    });
    p.push_str(REFS);
    p
}

/// What an ask is told of the references it may attach to what it writes.
const REFS: &str = " To point at the code you mean, in the stack or out of it, anything you write \
there may be an object instead of a string, {\"text\": what you write, \"refs\": [...]}, and a \
comment you open may have \"refs\" too: each {\"file\": its path, \"lines\": \"120-140,200-210\", \
\"patch\": a patch's number, for the file as that patch leaves it; 0, as the stack's base has it; \
none, as the whole stack leaves it}; or, to point at a comment of the stack, {\"comment\": its label \
above ([c1] is \"c1\"), or its id, as `stacked comments` lists it}. The user reads each under what \
you wrote, and goes there. Each takes a number, in the order you give them: in a reply, from the \
one its comment above says; in a comment you open, from r1. In what you write, @r1 names \
reference r1 of the comment you write on, @r{1,2} several, @r* all of them: those attached to \
it, and those you attach.";

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

/// Tools that change things: let every ask use them, and an answer may
/// change things too.
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
    i.prompt = prompt(&db, &i, &s, &cmd, id);
    let cwd = i.worktree.clone().map_or(repo.clone(), PathBuf::from);
    let mut tools = match i.way {
        Way::Change => format!("{},Bash({cmd} fits *),Bash({cmd} export *)", allowed_tools()),
        Way::Answer => answer_tools(&cmd),
        // Building and testing runs what the project needs: Bash at large.
        Way::Test => format!("Bash,{}", allowed_tools()),
    };
    // The settings' own, for every ask: an MCP server's tools, a command.
    for t in &agent.tools {
        tools.push(',');
        tools.push_str(t);
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
        // Unless it was landed meanwhile, taken for one left behind.
        if db.run(id).is_some_and(|r| r.state == RunState::Started) {
            db.add_run(run.clone())?;
            Ok(false)
        } else {
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
    // What the settings add: a model, arguments of the user's.
    c.args(cfg.agent_args());
    c.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    // The program run as claude: the tests' stand-in, else $STACKED_CLAUDE,
    // the settings' or claude.
    let claude = match CLAUDE.lock().unwrap().clone() {
        Some((claude, _)) => claude.to_string_lossy().into_owned(),
        None => cfg.agent_command(),
    };
    let extra: String = cfg.agent_args().iter().map(|a| format!(" {a}")).collect();
    step(format!("{claude} -p … --permission-mode {permission}{extra}, in {}", cwd.display()));
    c.env("STACKED_CLAUDE", claude);
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

/// What the agent said last: the `result` of its stream.
fn result_text(events: &[&Value]) -> String {
    events
        .iter()
        .rev()
        .filter(|v| v.get("type").and_then(Value::as_str) == Some("result"))
        .find_map(|v| v.get("result").and_then(Value::as_str).map(String::from))
        .unwrap_or_default()
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

fn str_of(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) | None => String::new(),
        Some(o) => o.to_string(),
    }
}

/// The JSON object `text` ends with.
fn last_object(text: &str) -> Option<Map<String, Value>> {
    let (i, j) = (text.find('{')?, text.rfind('}')?);
    if j < i {
        return None;
    }
    serde_json::from_str::<Value>(&text[i..=j]).ok()?.as_object().cloned()
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
            "the agent brought nothing back".to_string()
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
        if !refused.is_empty() {
            landed.summary += &format!(
                "; the agent was not allowed to use {}: your claude settings, or stacked's agent.tools, allow more",
                refused.join(", ")
            );
        }
        if !landed.ok {
            mark_failed(db, &i, &landed.summary)?;
        }
        if landed.keep {
            after = None;
        }
        // Held for review: in the queue, unless it brought nothing back.
        let held = (landed.ok && !landed.held.is_empty()).then(|| Held { at: now(), ..landed.held });
        let run = Run {
            state: if landed.ok { RunState::Done } else { RunState::Failed },
            summary: landed.summary,
            output,
            pid: None,
            held,
            ..started
        };
        db.add_run(run.clone())?;
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
        .map(|p| (p.change.clone(), note_lines(p).into_iter().map(|l| l.key).collect()))
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
        let (line, file) = match find_line(&note_lines(&p), &file, side, &text) {
            Some(k) => (k, file),
            None => (title_key(&p), MSG_PATH.to_string()),
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
    fn an_answer_may_read_the_web_and_changes_nothing() {
        let t = answer_tools("stacked -f db");
        assert!(t.contains("Bash(curl *)") && t.contains("Read") && t.contains("Bash(stacked -f db comments *)"), "{t}");
        for no in ["Edit", "Write", "Bash(git commit", "Bash(git push", "Bash(git rebase", "Bash,"] {
            assert!(!t.contains(no), "{no} in {t}");
        }
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
}
