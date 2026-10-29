//! Reconcile with HEAD: the patches whose commits in your checkout are
//! marked, `≠` or `!` (`head::Mark`), each take their commit's whole
//! message as a new version, in one edit stacked makes, with no agent: the
//! only way a commit's message goes into the stack. It is asked from the
//! menu of a patch marked, or of HEAD's row, and joins the queue as an edit
//! of the agent's does (`ask::claim`): at its turn, it is made at once, from
//! the stack the edits before it make, chained on the last of them waiting
//! for your review as edits are (`Job::after`), and it waits for your
//! review in turn. Edits asked after it do not start before it is
//! accepted or rejected: they then go on from the stack that decision
//! leaves. Answers go at once, as ever. The work it chained on accepted,
//! it goes on from the stack that made; rejected, it is dropped, and made
//! again at its next turn.
//!
//! Its review shows what it takes from your checkout, each new version's
//! message compared with the patch's where their lines wrap aside, and a
//! comment of stacked's says what to run in your checkout should you
//! reject it: the commits reworded to the stack's messages there instead.
//! Accepted, the marks are gone; rejected, nothing changes in the stack,
//! and the marks stay.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::db::{Db, Error, Result, Run, RunState, err, now};
use crate::head::{At, patches_said};
use crate::held::{Change, Held, Review};
use crate::mbox::Draft;

/// What a reconcile's run is, as `Run::what` says it.
pub const WHAT: &str = "reconcile";

/// What a reconcile takes, as it was asked: from the commits of HEAD of
/// the checkout whose top is `checkout`, each one's message. Made, the
/// landed work it went on from, guessing it is accepted, while that work
/// waits for your review: its change is made from the stack that work
/// makes, and reviewed after it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Job {
    pub takes: Vec<Take>,
    pub checkout: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
}

/// The message a patch takes: its change, its place (from 0) as it was
/// asked, the commit of HEAD's whose message it takes and that message,
/// as taken; and whether the commit is HEAD itself.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Take {
    pub change: String,
    pub at: usize,
    pub commit: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub head: bool,
}

/// Whether run `r` is a reconcile's.
pub fn is(r: &Run) -> bool {
    r.what == WHAT
}

/// The reconcile of stack `name` under way, if there is one: waiting its
/// turn, or made and waiting for your review. One at a time.
pub fn pending<'a>(db: &'a Db, name: &str) -> Option<&'a Run> {
    db.live_runs().into_iter().find(|r| r.stack == name && is(r))
}

/// Whether the reconcile of stack `name` is made and waits for your
/// review: edits asked meanwhile wait for it.
pub fn reviewing(db: &Db, name: &str) -> bool {
    pending(db, name).is_some_and(|r| r.state != RunState::Started)
}

/// The reconciles waiting their turn, the first asked first: each its run,
/// and its stack.
pub fn waiting(db: &Db) -> Vec<(String, String)> {
    db.live_runs()
        .into_iter()
        .filter(|r| is(r) && r.state == RunState::Started)
        .map(|r| (r.id.clone(), r.stack.clone()))
        .collect()
}

/// What `pending` says of a reconcile under way, `r`: why another is not
/// asked now.
fn under_way(r: &Run) -> String {
    match r.state {
        RunState::Started => "Reconcile with HEAD waits its turn already: one at a time".into(),
        _ => "Reconcile with HEAD waits for your review already: review it first".into(),
    }
}

/// Why Reconcile with HEAD is not asked of stack `name` now, if it is not:
/// it cannot change, or one is under way.
pub fn not_now(db: &Db, name: &str) -> Option<String> {
    if let Err(e) = db.writable(name) {
        return Some(e.0);
    }
    pending(db, name).map(under_way)
}

/// Reconcile with HEAD asked of stack `name`, HEAD of the checkout at
/// `here`: each patch whose commit is marked to take its commit's message.
/// Queued as an edit is (`ask::launch` makes it at its turn). Not of a
/// stack that cannot change, nor while one is under way, nor with no
/// commit marked. The run.
pub fn ask(db_path: &Path, name: &str, here: &Path) -> Result<String> {
    let db = Db::load(db_path)?;
    let s = db.stack(name)?.clone();
    let Some(h) = crate::head::find(&db, name, here)? else {
        return err(format!("{} is not a checkout of {name}'s repository", here.display()));
    };
    let on = match &h.at {
        At::Stack(on) if !on.marks.is_empty() => on,
        _ => return err(NOTHING),
    };
    let takes: Vec<Take> = on
        .marks
        .iter()
        .filter_map(|(k, message, _)| {
            Some(Take {
                change: db.patch(s.patches.get(*k)?).change.clone(),
                at: *k,
                commit: on.commits[*k].clone(),
                message: message.clone(),
                head: on.commits[*k] == h.commit,
            })
        })
        .collect();
    let of = patches_said(&takes.iter().map(|t| t.at).collect::<Vec<_>>());
    let checkout = h.dir.to_string_lossy().into_owned();
    Db::edit(db_path, |db| {
        if let Some(why) = not_now(db, name) {
            return err(why);
        }
        let id = crate::db::unique();
        db.add_run(Run {
            id: id.clone(),
            what: WHAT.into(),
            stack: name.into(),
            state: RunState::Started,
            request: format!("the commit messages of {of} taken from HEAD"),
            reconcile: Some(Box::new(Job { takes, checkout, after: None })),
            ..Default::default()
        })?;
        Ok(id)
    })
}

/// Why there is nothing to reconcile.
pub const NOTHING: &str = "no commit of HEAD's is marked: each has its patch's message";

/// Under the lock, from `ask::claim`: reconcile `id`'s turn come, made at
/// once, from the stack the work `after` makes, if it is chained on that
/// work, else from the stack as it is. Each patch it takes a message for
/// that is there, its message another, a new version; then it waits for
/// your review. With none, it lands with nothing to review, as said.
pub(crate) fn make(db: &mut Db, id: &str, after: Option<String>) -> Result<()> {
    let r = db.run(id).cloned().ok_or_else(|| Error(format!("no run {id}")))?;
    let job = r.reconcile.clone().ok_or_else(|| Error(format!("run {id} is not a reconcile")))?;
    let Some((rev, now_s)) = db.head(&r.stack).map(|(rev, s)| (rev, s.clone())) else {
        let summary = format!("the stack {} is gone: nothing was taken", r.stack);
        return db.add_run(Run { state: RunState::Failed, summary, ..r }).map(|_| ());
    };
    // The work it goes on from gone meanwhile: from the stack as it is.
    let (from, after) = match after.and_then(|a| Some((crate::ask::work_stack(db, &a).ok()?, a))) {
        Some((s, a)) => (s, Some(a)),
        None => (now_s, None),
    };
    let mut patches = from.patches.clone();
    let (mut done, mut places) = (Vec::new(), Vec::new());
    for t in &job.takes {
        let Some(k) = patches.iter().position(|p| db.patch(p).change == t.change) else { continue };
        let old = db.patch(&patches[k]).clone();
        if crate::head::same_message(&old.message, &t.message) {
            continue;
        }
        let new = db.add_patch(crate::ops::revise(&old, Draft { message: t.message.clone(), ..Draft::from(&old) }))?;
        done.push(format!("edit {} as {}", &old.id[..8], &new[..8]));
        patches[k] = new;
        places.push(k);
    }
    if places.is_empty() {
        let summary = "nothing to take from HEAD: each patch has its commit's message already".to_string();
        return db.add_run(Run { state: RunState::Done, summary, ..r }).map(|_| ());
    }
    places.sort_unstable();
    let of = patches_said(&places);
    let held = Held {
        at: now(),
        stack: Some(Change {
            patches,
            base: from.base.clone(),
            op: format!("take the commit messages of {of} from HEAD: {}", done.join("; ")),
            of: of.clone(),
            from: after.is_none().then_some(rev),
        }),
        ..Default::default()
    };
    db.add_run(Run {
        state: RunState::Done,
        summary: format!("the commit messages of {of} taken from HEAD, to review"),
        held: Some(held),
        reconcile: Some(Box::new(Job { after, ..*job })),
        ..r
    })
    .map(|_| ())
}

/// The reconciles waiting for your review that go on from landed work
/// `id`, guessing it is accepted.
pub(crate) fn going_on_from<'a>(db: &'a Db, id: &str) -> Vec<&'a Run> {
    db.live_runs()
        .into_iter()
        .filter(|r| is(r) && crate::held::waiting(db, r) && after(r) == Some(id))
        .collect()
}

/// The landed work reconcile `r` goes on from, if it is chained on some.
pub(crate) fn after(r: &Run) -> Option<&str> {
    r.reconcile.as_ref()?.after.as_deref()
}

/// Landed work `id` accepted, its change now the stack at `rev`: the
/// reconciles that went on from it go on from that stack.
pub(crate) fn followed(db: &mut Db, id: &str, rev: usize) -> Result<()> {
    for r in going_on_from(db, id).into_iter().cloned().collect::<Vec<_>>() {
        let (Some(mut h), Some(mut job)) = (r.held.clone(), r.reconcile.clone()) else { continue };
        if let Some(c) = h.stack.as_mut() {
            c.from = Some(rev);
        }
        job.after = None;
        db.add_run(Run {
            held: Some(h),
            reconcile: Some(job),
            ..r
        })?;
    }
    Ok(())
}

/// Landed work `id` rejected, sent back or edited by hand (`why`): the
/// reconciles that went on from it dropped, each asked again, to be made
/// at its next turn from the stack that leaves. How many.
pub(crate) fn dropped(db: &mut Db, id: &str, why: &str) -> Result<usize> {
    let runs: Vec<Run> = going_on_from(db, id).into_iter().cloned().collect();
    for r in &runs {
        let v = db.review(&r.id).cloned().unwrap_or_default();
        db.set_review(&r.id, Review { closed: true, ..v })?;
        db.add_run(Run {
            summary: format!("dropped: the work it went on from was {why}; asked again"),
            ..r.clone()
        })?;
        let job = r.reconcile.as_deref().map(|j| Job { after: None, ..j.clone() });
        db.add_run(Run {
            id: crate::db::unique(),
            what: WHAT.into(),
            stack: r.stack.clone(),
            state: RunState::Started,
            request: r.request.clone(),
            reconcile: job.map(Box::new),
            ..Default::default()
        })?;
    }
    Ok(runs.len())
}

/// What stacked's comment in the review of reconcile `job` says: should
/// you reject it, what to run in your checkout to give its commits the
/// stack's messages instead, a line for each, from the top down, each a
/// command and what to do then.
pub fn comment(job: &Job) -> Vec<String> {
    let mut out = vec![
        "If you reject this, the stack keeps its messages and the marks stay. To give your checkout the stack's messages instead, reword these commits there, from the top down, each to its patch's message as the stack has it: the diff shows it."
            .to_string(),
    ];
    let mut takes: Vec<&Take> = job.takes.iter().collect();
    takes.sort_by_key(|t| std::cmp::Reverse(t.at));
    for t in takes {
        let short = &t.commit[..12.min(t.commit.len())];
        out.push(match t.head {
            true => format!("Patch {}, commit {short}, HEAD itself: `git commit --amend`, then write the stack's message of patch {}.", t.at + 1, t.at + 1),
            false => format!(
                "Patch {}, commit {short}: `git rebase -i {short}^`, mark the commit `reword`, then write the stack's message of patch {}.",
                t.at + 1,
                t.at + 1
            ),
        });
    }
    out.push(format!("Run them in {}. Once the messages are the same, the marks are gone.", job.checkout));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn its_comment_says_what_to_run_from_the_top_down_the_top_commit_amended() {
        let take = |at: usize, commit: &str, head: bool| Take {
            change: format!("c{at}"),
            at,
            commit: commit.into(),
            message: "m".into(),
            head,
        };
        let job = Job {
            takes: vec![take(0, "1a2b3c4d5e6f7a8b", false), take(2, "9f8e7d6c5b4a3f2e", true)],
            checkout: "/src".into(),
            after: None,
        };
        assert_eq!(
            comment(&job)[1..],
            [
                "Patch 3, commit 9f8e7d6c5b4a, HEAD itself: `git commit --amend`, then write the stack's message of patch 3.",
                "Patch 1, commit 1a2b3c4d5e6f: `git rebase -i 1a2b3c4d5e6f^`, mark the commit `reword`, then write the stack's message of patch 1.",
                "Run them in /src. Once the messages are the same, the marks are gone.",
            ]
        );
    }
}
