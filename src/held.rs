//! Work held for review: the agent's, and the reviewers'. When an ask
//! lands, nothing changes under you: what it brought back is held in its
//! run, in the queue of work to review, oldest first. It may hold a change
//! of the stack, replies to the comments it was asked about, and comments
//! it opened. Reviews pulled from Phabricator are held the same way, in
//! their pull's run: the reviewers' new comments, and their replies. Each
//! part is accepted or rejected, the change of the stack as one; once every
//! part is, the review closes: what was accepted is applied, and the rest
//! is dropped for good. Until then, the stack is not edited by hand if the
//! agent changed it, and the comments it answered are not written on.
//!
//! Your own edits by hand land the same way, in a run of their own
//! (`hand`), to review before they change anything. An edit made in the
//! review of landed work replaces it: that work is rejected, and the work
//! with your edit in it lands in its place, its replies and comments as
//! they were.

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

use crate::db::{Db, Error, Note, Patch, Result, Run, err, now};
use crate::thread::{Role, append_turn};

/// What an ask brought back, held until it is reviewed.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Held {
    /// When it landed: the queue goes by it.
    pub at: i64,
    /// Its change of the stack, if it changed it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stack: Option<Change>,
    /// Its replies to the comments it was asked about.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub replies: Vec<Reply>,
    /// The comments it opened: an answer's, a build and test's findings.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub opened: Vec<Note>,
    /// What was edited of it by hand before its review closed, each said:
    /// an edit by hand's own, and yours of the agent's work.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edits: Vec<String>,
}

/// A change of the stack held: the stack it makes from the one the ask
/// started from, which nothing else changes until the review closes.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Change {
    pub patches: Vec<String>,
    pub base: Option<String>,
    /// What the history calls it: `agent: patch 2`.
    pub op: String,
    /// What it changes, said: `patch 2`, `the stack`.
    pub of: String,
    /// The stack it was made from, when the ask's is not it: an edit by
    /// hand's. Only that stack takes it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<usize>,
}

/// A reply held, to comment (change, id): the agent's, or, pulled from
/// Phabricator, a reviewer's.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Reply {
    pub change: String,
    pub id: String,
    pub text: String,
    /// Its fence's tag, the way the agent was asked: `change`, `answer`,
    /// `test`.
    pub tag: String,
    /// Why the comment needs more than its patch, the agent says: accepted,
    /// the comment moves to the stack's comments.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub promote: Option<String>,
    /// Pulled: the reviewer who wrote it, and the remote comment it is,
    /// kept with its thread so that it is not pulled again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote: Option<String>,
    /// Your answer to it, written in its review: it follows it in its
    /// thread, and the work goes back to the agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
}

impl Reply {
    /// Thread `text` with this reply as its last turn: the reviewer's who
    /// wrote it, or the agent's; your answer to it after, if you wrote one.
    pub fn after(&self, text: &str) -> String {
        let text = match &self.reviewer {
            Some(who) => append_turn(text, Role::Reviewer, &self.text, Some(who)),
            None => append_turn(text, Role::Agent, &self.text, Some(&self.tag)),
        };
        match &self.answer {
            Some(a) => append_turn(&text, Role::You, a, None),
            None => text,
        }
    }

    /// Who wrote it, as a reply's owner is said: `the agent`, `ann`.
    pub fn who(&self) -> &str {
        self.reviewer.as_deref().unwrap_or("the agent")
    }
}

/// Whether run `r` is a pull of reviews from Phabricator: the work it
/// holds, the reviewers'.
pub fn pulled(r: &Run) -> bool {
    r.what == "pull"
}

/// Whether run `r` holds an edit by hand: yours, for your review.
pub fn by_hand(r: &Run) -> bool {
    r.what == "hand"
}

/// The stack the change of work `r` was made from, which alone takes it.
fn made_from(r: &Run, h: &Held) -> Option<usize> {
    h.stack.as_ref().and_then(|c| c.from).or_else(|| crate::ask::info(r).map(|i| i.from))
}

/// Holds `patches`, what an edit by hand makes of stack `name`, for your
/// review: nothing changes before it is accepted. `op` is what the history
/// calls it, `of` what it changes, and `said` what the edit was. Made from
/// the stack as it is; or, editing the work run `work` holds, from the
/// stack that work makes, in its place: that work is rejected, and the new
/// holds its replies and comments as they were. The new run.
pub fn hold_by_hand(db: &mut Db, name: &str, patches: Vec<String>, op: &str, of: &str, said: &str, work: Option<&str>) -> Result<String> {
    let id = crate::db::unique();
    let Some(work) = work else {
        let (from, s) = db.head(name).map(|(rev, s)| (rev, s.clone())).ok_or_else(|| Error(format!("no stack {name}")))?;
        let held = Held {
            at: now(),
            stack: Some(Change {
                patches,
                base: s.base.clone(),
                op: op.into(),
                of: of.into(),
                from: Some(from),
            }),
            edits: vec![said.into()],
            ..Default::default()
        };
        db.add_run(Run {
            id: id.clone(),
            what: "hand".into(),
            stack: name.into(),
            state: crate::db::RunState::Done,
            request: said.into(),
            held: Some(held),
            ..Default::default()
        })?;
        return Ok(id);
    };
    let (r, h, mut v) = under_review(db, work)?;
    let c = h.stack.clone().ok_or_else(|| Error(format!("run {work} holds no change of the stack to edit")))?;
    let mut edits = h.edits.clone();
    edits.push(said.into());
    let held = Held {
        stack: Some(Change {
            patches,
            op: format!("{}; {op}", c.op),
            of: if c.of == of { c.of.clone() } else { "the stack".into() },
            from: made_from(&r, &h),
            ..c
        }),
        edits,
        ..h.clone()
    };
    db.add_run(Run {
        id: id.clone(),
        held: Some(held),
        summary: String::new(),
        rev: None,
        ..r.clone()
    })?;
    v.closed = true;
    v.marks.insert(Part::Stack.key(), false);
    db.set_review(work, v)?;
    db.add_run(Run {
        summary: format!("reviewed: rejected for your edit of it by hand, run {id}"),
        ..r
    })?;
    crate::ask::guessed_wrong(db, work, "edited by hand")?;
    Ok(id)
}

/// A part of the work held, to accept or reject.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Part {
    Stack,
    Opened(usize),
    Reply(usize),
}

impl Part {
    /// As a review's marks name it.
    pub fn key(self) -> String {
        match self {
            Part::Stack => "stack".into(),
            Part::Opened(k) => format!("opened {k}"),
            Part::Reply(k) => format!("reply {k}"),
        }
    }

    pub fn of_key(key: &str) -> Option<Part> {
        let k = |s: &str| s.parse().ok();
        match key.split_once(' ') {
            None if key == "stack" => Some(Part::Stack),
            Some(("opened", n)) => k(n).map(Part::Opened),
            Some(("reply", n)) => k(n).map(Part::Reply),
            _ => None,
        }
    }
}

/// The review of work held, as saved: each part marked accepted (true) or
/// rejected; the replies taken already into their threads; and whether it
/// is closed.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Review {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub marks: BTreeMap<String, bool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub taken: Vec<usize>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub closed: bool,
}

impl Held {
    /// Its parts in the order they are reviewed: the change of the stack,
    /// the comments opened, the replies.
    pub fn parts(&self) -> Vec<Part> {
        let mut out = Vec::new();
        if self.stack.is_some() {
            out.push(Part::Stack);
        }
        out.extend((0..self.opened.len()).map(Part::Opened));
        out.extend((0..self.replies.len()).map(Part::Reply));
        out
    }

    pub fn is_empty(&self) -> bool {
        self.stack.is_none() && self.replies.is_empty() && self.opened.is_empty()
    }

    /// What it holds, said: `a change of patch 2, 2 replies`.
    pub fn said(&self) -> String {
        let mut out = Vec::new();
        if let Some(c) = &self.stack {
            out.push(format!("a change of {}", c.of));
        }
        if !self.replies.is_empty() {
            out.push(count(self.replies.len(), "reply", "replies"));
        }
        if !self.opened.is_empty() {
            out.push(count(self.opened.len(), "opened comment", "opened comments"));
        }
        out.join(", ")
    }
}

fn count(n: usize, one: &str, many: &str) -> String {
    if n == 1 { format!("1 {one}") } else { format!("{n} {many}") }
}

/// Whether what run `r` brought back waits for review.
pub fn waiting(db: &Db, r: &Run) -> bool {
    r.held.is_some() && !db.review(&r.id).is_some_and(|v| v.closed)
}

/// The runs of stack `name` whose work waits for review, the first landed
/// first.
pub fn queue<'a>(db: &'a Db, name: &str) -> Vec<&'a Run> {
    let mut out: Vec<&Run> = db.runs().into_iter().filter(|r| r.stack == name && waiting(db, r)).collect();
    out.sort_by_key(|r| r.held.as_ref().map_or(0, |h| h.at));
    out
}

/// The run whose change of stack `name` waits for review, if one does.
pub fn change_waiting<'a>(db: &'a Db, name: &str) -> Option<&'a Run> {
    queue(db, name).into_iter().find(|r| r.held.as_ref().is_some_and(|h| h.stack.is_some()))
}

/// The comments whose reply waits for review: to the run, and where the
/// reply is in what it holds.
pub fn replies_waiting(db: &Db) -> HashMap<(String, String), (String, usize)> {
    let mut out = HashMap::new();
    for r in db.runs() {
        let Some(h) = r.held.as_ref().filter(|_| waiting(db, r)) else { continue };
        let taken = db.review(&r.id).map(|v| v.taken.clone()).unwrap_or_default();
        for (k, rp) in h.replies.iter().enumerate() {
            if !taken.contains(&k) {
                out.insert((rp.change.clone(), rp.id.clone()), (r.id.clone(), k));
            }
        }
    }
    out
}

/// The run holding a reply to comment (change, id) that waits for review,
/// and where the reply is in what it holds.
pub fn reply_waiting(db: &Db, change: &str, id: &str) -> Option<(String, usize)> {
    db.runs().into_iter().find_map(|r| {
        let h = r.held.as_ref().filter(|_| waiting(db, r))?;
        let taken = db.review(&r.id).map(|v| v.taken.clone()).unwrap_or_default();
        let k = h.replies.iter().enumerate().position(|(k, rp)| rp.change == change && rp.id == id && !taken.contains(&k))?;
        Some((r.id.clone(), k))
    })
}

/// What run `id` brought back, and its review as saved.
fn under_review(db: &Db, id: &str) -> Result<(Run, Held, Review)> {
    let r = db.run(id).cloned().ok_or_else(|| Error(format!("no run {id}")))?;
    let h = r.held.clone().ok_or_else(|| Error(format!("run {id} holds nothing to review")))?;
    let v = db.review(id).cloned().unwrap_or_default();
    if v.closed {
        return err(format!("run {id} is reviewed already"));
    }
    Ok((r, h, v))
}

/// Reply `rp` put into its thread, as the agent's turn or the reviewer's,
/// the comment moved to stack `name`'s comments if it needs more than its
/// patch.
fn apply_reply(db: &mut Db, name: &str, rp: &Reply) -> Result<()> {
    // Gone meanwhile: nothing to answer.
    let Some(mut n) = db.note(&rp.change, &rp.id).cloned() else { return Ok(()) };
    if !rp.text.trim().is_empty() {
        let mut remote = n.remote.clone();
        remote.extend(rp.remote.clone());
        // The agent's answer ends a failed ask; a reviewer's reply does not.
        let failed = if rp.reviewer.is_some() { n.failed.clone() } else { None };
        n = Note {
            text: rp.after(&n.text),
            remote,
            failed,
            ..n
        };
        db.set_note(n.clone())?;
    }
    if let Some(why) = &rp.promote {
        crate::ask::promote(db, name, &n, why)?;
    }
    Ok(())
}

/// Takes reply `k` of what run `id` brought back into its thread at once,
/// accepted, out of the review; not one going with a change of the stack,
/// which is reviewed with it. With nothing left to review, the review
/// closes.
pub fn take(db: &mut Db, id: &str, k: usize) -> Result<()> {
    let (r, h, mut v) = under_review(db, id)?;
    if h.stack.is_some() {
        return err("this reply goes with the agent's change of the stack: review them together");
    }
    if crate::ask::guessing(db, &r).is_some() {
        return err("this reply goes on from the work landed before it, which waits for your review: review that one first");
    }
    let rp = h.replies.get(k).ok_or_else(|| Error(format!("run {id} holds no reply {k}")))?;
    if v.taken.contains(&k) {
        return Ok(());
    }
    apply_reply(db, &r.stack, rp)?;
    v.taken.push(k);
    v.marks.insert(Part::Reply(k).key(), true);
    v.closed = h.opened.is_empty() && (0..h.replies.len()).all(|j| v.taken.contains(&j));
    let closed = v.closed;
    db.set_review(id, v.clone())?;
    if closed {
        db.add_run(Run {
            summary: format!("reviewed: {}", said(&h, &v.marks)),
            ..r
        })?;
    }
    Ok(())
}

/// Saves the marks of run `id`'s review, to take up again: nothing is
/// applied.
pub fn save(db: &mut Db, id: &str, marks: &BTreeMap<String, bool>) -> Result<()> {
    let (_, _, mut v) = under_review(db, id)?;
    v.marks = marks.clone();
    db.set_review(id, v).map(|_| ())
}

/// What you wrote in the review of work `r`: the comments you opened
/// there, still open, and your answers to its replies. With any, the work
/// is not accepted: it goes back to the agent.
pub fn yours_in(db: &Db, r: &Run) -> (Vec<(String, String)>, usize) {
    let opened = db
        .stack_notes(&r.stack)
        .into_iter()
        .filter(|n| n.review.as_deref() == Some(r.id.as_str()) && n.by.is_none())
        .map(|n| (n.change.clone(), n.id.clone()))
        .collect();
    let answers = r.held.as_ref().map_or(0, |h| h.replies.iter().filter(|rp| rp.answer.is_some()).count());
    (opened, answers)
}

/// Writes `text`, your answer to reply `k` of the work run `id` holds, in
/// its review: empty, it takes it back.
pub fn answer(db: &mut Db, id: &str, k: usize, text: &str) -> Result<()> {
    let (r, mut h, _) = under_review(db, id)?;
    let rp = h.replies.get_mut(k).ok_or_else(|| Error(format!("run {id} holds no reply {k}")))?;
    rp.answer = Some(text.trim().to_string()).filter(|t| !t.is_empty());
    db.add_run(Run { held: Some(h), ..r }).map(|_| ())
}

/// What goes back to the agent from a review: the comments to ask again,
/// how, and whether of the whole stack.
#[derive(Debug, Clone, PartialEq)]
pub struct Round {
    pub notes: Vec<(String, String)>,
    pub way: crate::db::Way,
    pub whole: bool,
    /// How many edits that went on from it are asked again.
    pub dropped: usize,
}

/// Rejects the work run `id` holds, to send it back to the agent as what
/// you wrote in its review asks: its change is not applied, but the next
/// round goes on from it; its replies go in their threads, your answers
/// after them; the comments it opened, as `marks` says (those not marked,
/// kept). What to ask again: the comments of its ask whose last word is
/// yours now, and those you opened in its review.
pub fn send_back(db: &mut Db, id: &str, marks: &BTreeMap<String, bool>) -> Result<Round> {
    let (r, h, mut v) = under_review(db, id)?;
    if crate::ask::guessing(db, &r).is_some() {
        return err("this work goes on from the work landed before it, which waits for your review: review that one first");
    }
    let i = crate::ask::info(&r).ok_or_else(|| Error(format!("run {id} is not the agent's work: there is no one to send it back to")))?;
    let (opened, answers) = yours_in(db, &r);
    if opened.is_empty() && answers == 0 {
        return err("you wrote nothing in its review to send back: accept or reject it");
    }
    for (k, rp) in h.replies.iter().enumerate() {
        if !v.taken.contains(&k) {
            apply_reply(db, &r.stack, rp)?;
        }
    }
    for (k, n) in h.opened.iter().enumerate() {
        if marks.get(&Part::Opened(k).key()) != Some(&false) {
            db.set_note(Note { time: now(), ..n.clone() })?;
        }
    }
    let mut notes: Vec<(String, String)> = Vec::new();
    for at in i.notes.iter().map(|sh| (sh.change.clone(), sh.id.clone())).chain(opened) {
        if !notes.contains(&at) && db.note(&at.0, &at.1).is_some_and(crate::ask::sendable) {
            notes.push(at);
        }
    }
    if notes.is_empty() {
        return err("none of what you wrote in its review is open to send back");
    }
    // Asked together, as they were, unless they reach past the place the
    // ask was about.
    let places: std::collections::HashSet<crate::ask::Scope> = notes.iter().filter_map(|(c, i)| db.note(c, i)).map(crate::ask::scope_of).collect();
    let whole = match &i.scope {
        crate::ask::Scope::Stack => notes.iter().any(|(c, _)| !c.starts_with("stack:")),
        place => places.len() > 1 || places.iter().any(|p| p != place),
    };
    v.marks.insert(Part::Stack.key(), false);
    v.closed = true;
    db.set_review(id, v)?;
    let dropped = crate::ask::guessed_wrong(db, id, "sent back")?;
    let n = notes.len();
    db.add_run(Run {
        summary: format!("reviewed: sent back to the agent, {}", count(n, "comment to ask again", "comments to ask again")),
        ..r
    })?;
    Ok(Round {
        notes,
        way: i.way,
        whole,
        dropped,
    })
}

/// The mark, in a review, of the comment the ask was about at place `k`
/// among them: true to close it with the review; else it is kept.
pub fn asking_key(k: usize) -> String {
    format!("asking {k}")
}

/// The comments the ask of run `r` was about, by (change, id): an agent's
/// work's; none for a pull's, or an edit by hand's.
pub fn asking_of(r: &Run) -> Vec<(String, String)> {
    crate::ask::info(r).map(|i| i.notes.into_iter().map(|sh| (sh.change, sh.id)).collect()).unwrap_or_default()
}

/// Closes run `id`'s review: each part as marked in `marks`, those left
/// unmarked accepted or rejected as `rest` says (with none, every part must
/// be marked). Rejecting the change of the stack rejects its replies and
/// comments too. What is accepted is applied, the change of the stack only
/// if the stack is still the one the agent took; the rest is dropped. What
/// came of it, said.
pub fn close(db: &mut Db, id: &str, marks: &BTreeMap<String, bool>, rest: Option<bool>) -> Result<String> {
    close_with(db, id, marks, rest, false)
}

/// As `close`, the comments the ask was about that `marks` marks to close
/// closed too, done with, once their replies are in their threads; with
/// `asking`, every one of them.
pub fn close_with(db: &mut Db, id: &str, marks: &BTreeMap<String, bool>, rest: Option<bool>, asking: bool) -> Result<String> {
    let (r, h, mut v) = under_review(db, id)?;
    // Gone on from work under review, guessing: that one is reviewed first.
    if crate::ask::guessing(db, &r).is_some() {
        return err("this work goes on from the work landed before it, which waits for your review: review that one first");
    }
    // What you wrote in its review is for the agent: it goes back to it.
    let (opened, answers) = yours_in(db, &r);
    if !opened.is_empty() || answers > 0 {
        return err("you wrote in its review: it is not accepted nor rejected, but sent back to the agent");
    }
    let mut all = BTreeMap::new();
    for p in h.parts() {
        if matches!(p, Part::Reply(k) if v.taken.contains(&k)) {
            continue;
        }
        let Some(m) = marks.get(&p.key()).copied().or(rest) else {
            return err("not every part is marked yet: mark each, or accept or reject all that remain");
        };
        all.insert(p.key(), m);
    }
    let change = h.stack.as_ref().map(|c| (c, all.get(&Part::Stack.key()) == Some(&true)));
    if let Some((_, false)) = change {
        for (key, m) in all.iter_mut() {
            if key != "stack" {
                *m = false;
            }
        }
    }
    let from = made_from(&r, &h);
    if let Some((_, true)) = change
        && db.head(&r.stack).map(|(rev, _)| rev) != from
    {
        return err("the stack changed since this change was made from it: it can only be rejected");
    }
    // The replies where their comments are, then the change, the comments
    // on its lines following them, then the comments opened.
    for (k, rp) in h.replies.iter().enumerate() {
        if all.get(&Part::Reply(k).key()) == Some(&true) {
            apply_reply(db, &r.stack, rp)?;
        }
    }
    // Done with, the comments that asked are closed: a closed comment does
    // not follow the lines of the change.
    let mut closed = 0;
    for (k, (c, i)) in asking_of(&r).into_iter().enumerate() {
        if !asking && marks.get(&asking_key(k)) != Some(&true) {
            continue;
        }
        if let Some(n) = db.note(&c, &i).cloned() {
            db.set_note(Note { text: String::new(), ..n })?;
            closed += 1;
        }
    }
    let mut rev = r.rev;
    if let Some((c, true)) = change {
        let s = db.stack(&r.stack)?.clone();
        let olds: Vec<Patch> = s.patches.iter().map(|p| db.patch(p).clone()).collect();
        let base = c.base.clone().or(s.base.clone());
        rev = Some(db.set_stack_on(&r.stack, c.patches.clone(), base, s.repo.clone(), &c.op)?);
        crate::ask::follow_lines(db, &olds, &c.patches)?;
    }
    // What went on from it, guessing it would be accepted: right, or wrong.
    // Its change accepted, the answers made on the stack before it are asked
    // again, on the stack as it is now.
    let mut again = Vec::new();
    match (change, rev) {
        (Some((_, true)), Some(at)) => {
            crate::ask::guessed_right(db, id, at)?;
            let n = crate::ask::reask_answers(db, &r.stack, id)?;
            if n > 0 {
                again.push(count(n, "answer-only ask asked again", "answer-only asks asked again"));
            }
        }
        (Some((_, false)), _) => {
            let n = crate::ask::guessed_wrong(db, id, "rejected")?;
            if n > 0 {
                again.push(count(n, "edit that went on from it asked again", "edits that went on from it asked again"));
            }
        }
        _ => {}
    }
    for (k, n) in h.opened.iter().enumerate() {
        if all.get(&Part::Opened(k).key()) == Some(&true) {
            db.set_note(Note { time: now(), ..n.clone() })?;
        }
    }
    v.marks.extend(all);
    v.marks.extend(marks.iter().filter(|(k, _)| k.starts_with("asking ")).map(|(k, m)| (k.clone(), *m)));
    v.closed = true;
    db.set_review(id, v.clone())?;
    let mut said = said(&h, &v.marks);
    if closed > 0 {
        said += &format!("; {}", count(closed, "asking comment closed", "asking comments closed"));
    }
    for a in again {
        said += &format!("; {a}");
    }
    db.add_run(Run {
        summary: format!("reviewed: {said}"),
        rev,
        ..r
    })?;
    Ok(said)
}

/// What a review made of the work held, by its marks: `its change
/// accepted; 2 replies accepted, 1 rejected`.
pub fn said(h: &Held, marks: &BTreeMap<String, bool>) -> String {
    let mark = |p: Part| marks.get(&p.key()).copied();
    let mut out = Vec::new();
    if h.stack.is_some() {
        out.push(match mark(Part::Stack) {
            Some(true) => "its change accepted".to_string(),
            _ => "its change rejected".to_string(),
        });
    }
    for (one, many, parts) in [
        ("opened comment", "opened comments", (0..h.opened.len()).map(Part::Opened).collect::<Vec<_>>()),
        ("reply", "replies", (0..h.replies.len()).map(Part::Reply).collect()),
    ] {
        let yes = parts.iter().filter(|p| mark(**p) == Some(true)).count();
        let no = parts.len() - yes;
        let of = |n| count(n, one, many);
        match (yes, no) {
            (0, 0) => {}
            (y, 0) => out.push(format!("{} accepted", of(y))),
            (0, n) => out.push(format!("{} rejected", of(n))),
            (y, n) => out.push(format!("{} accepted, {n} rejected", of(y))),
        }
    }
    out.join("; ")
}
