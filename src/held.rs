//! Work held for review: the agent's, and the reviewers'. When an ask
//! lands, nothing changes under you: what it brought back is held in its
//! run, in the queue of work to review, oldest first. It may hold a change
//! of the stack, replies to the comments it was asked about, and comments
//! it opened; or nothing, while one of those comments is still open, for
//! its review to close it or keep it. Reviews pulled from Phabricator are
//! held the same way, in their pull's run: the reviewers' new comments,
//! their replies, their comments edited on Phabricator since they were
//! filed, and those filed without the suggested edit Phabricator gives with
//! them, read again with it. Each part is accepted or rejected, the change
//! of the stack operation by operation (`operations`), as what each
//! operation needs, worked out as the work landed, is stored with it
//! (`Held::needs`); work with nothing stored has its change marked as a
//! whole. Once every part is marked, the review closes: what was accepted
//! is applied, the change whole or in part, and the rest is dropped for
//! good. A pull's parts are not rejected: closing its review files every
//! comment it pulled, a reply to a thread you closed reopening that thread.
//! Until then, the stack is not edited by hand if the agent changed it, and
//! the comments it answered are not written on. Replies that are all their
//! work holds may also be taken at once, one by one, out of its review,
//! unless that work goes on from work under review: the last one taken, the
//! review closes, and the comments the ask was about stay open, as any
//! comment does. Work that brought nothing back has no part: it waits until
//! you close its review, which closes the comments it was asked about or
//! keeps them.
//!
//! A change of the stack accepted leaves behind the answers made on the
//! stack before it, running or waiting for your review, chained: its
//! review marks each keep, restart or stop (`AnswerMark`), and closes with
//! the change accepted only once each is marked. Rejected or sent back,
//! the change leaves the stack as it is, and the answers go on as they are.
//!
//! Your own edits by hand land the same way, in a run of their own
//! (`hand`), to review before they change anything. An edit made in the
//! review of landed work replaces it: that work is rejected, and the work
//! with your edit in it lands in its place, its replies and comments as
//! they were.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::db::{Db, Error, Note, Patch, Result, Run, Stack, err, now};
use crate::thread::{Role, Turn, all_turns, append_agent_on, append_turn, render_thread};

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
    /// A try push ask's: the tasks it selected, each to accept or reject,
    /// those accepted pushed to try as the review closes.
    #[cfg(feature = "try-push")]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tasks: Vec<crate::try_push::Task>,
    /// How many times each of those tasks is to run, when it asked for
    /// more than once (mach's `--rebuild`).
    #[cfg(feature = "try-push")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rebuild: Option<u32>,
    /// A bug's draft's: the bug the agent drafted, to read, its form opened
    /// in your browser as the review closes (`bug`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bug: Option<Box<crate::bug::Draft>>,
    /// A lint job's: what the linter did, patch by patch, and what it
    /// reported that it did not fix, its own lines; shown in the review, to
    /// read. With no change of the stack, it is all the work holds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lint: Option<Box<crate::lint::Report>>,
    /// The tools the agent could not use, each refused, allowed for this
    /// work or allowed from now on, as the review marks it: with them, work
    /// lands for review though the agent failed or brought nothing back.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<crate::refusals::Refusal>,
    /// The follow-ups that only read, asked from its review, that came into
    /// it, the first landed first: what each was asked about, and a build
    /// and test's findings on each patch, to read. Their replies are among
    /// the replies above, a build and test's to read (`Reply::read`); the
    /// comments an answer opened, among those opened; a try push's tasks,
    /// among the tasks; what they could not use, among the tools.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub follow_ups: Vec<FollowUp>,
    /// What each operation of its change of the stack needs, worked out as
    /// it landed (`operations::Needs`), for the stacks it was worked out
    /// for: its review decides from it, each operation marked on its own.
    /// None for work that landed before stacked worked it out, or that
    /// changes nothing of the stack: its change is marked as a whole.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub needs: Option<crate::operations::Needs>,
}

/// What the change of the stack a pull from Phabricator holds takes from
/// it, a patch at a time (`Taken`), and what the pull found that changes
/// nothing, a sentence each, to read: a revision that landed or was
/// abandoned, a parent Phabricator gives that the stack's order disagrees
/// with, a revision not added, a diff not taken, and why. Kept with the
/// pull's job (`review::Job::took`), which its review reads (`took`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Took {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub took: Vec<Taken>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub found: Vec<String>,
}

/// What a patch of a pull's change takes from Phabricator: the patch, by
/// its change; its revision, `D123` (none for a patch of the stack with no
/// revision, made again on what the pull changed under it); its revision's
/// latest diff, as its code, and its title and summary, as its message;
/// added, how the pull came to it; the version of the patch the last diff
/// of its revision stacked knows was, when the patch changed in the stack
/// since; and, made again on the pulled versions under it, which it no
/// longer applied on as it was, the files whose conflicts it marks, none
/// when they merged.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Taken {
    pub change: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub d: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub diff: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub message: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub added: Option<Added>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remade: Option<Vec<String>>,
}

/// How a pull came to a revision it adds: the one asked, of a new stack;
/// the parent of revision `D123`; stacked on revision `D123`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Added {
    Asked,
    Parent(String),
    Child(String),
}

impl Taken {
    /// What it takes, said after its operation's line in the review: `from
    /// D123's latest diff`, `D11, the parent of D12`, `D125, stacked on
    /// D124`, `no longer applies on the pulled version`; its conflicts
    /// `resolved` by hand since, so.
    pub fn said(&self, resolved: bool) -> String {
        let mut out: Vec<String> = Vec::new();
        match (&self.added, self.diff, self.message) {
            (Some(Added::Parent(of)), ..) => out.push(format!("{}, the parent of {of}", self.d)),
            (Some(Added::Child(on)), ..) => out.push(format!("{}, stacked on {on}", self.d)),
            (Some(Added::Asked), ..) => out.push(self.d.clone()),
            (None, true, _) => out.push(format!("from {}'s latest diff", self.d)),
            (None, false, true) => out.push(format!("from {}'s title and summary", self.d)),
            (None, false, false) => {}
        }
        let pulled = self.diff;
        match &self.remade {
            Some(m) if !m.is_empty() && resolved => out.push("its conflicts resolved by hand".into()),
            Some(m) if !m.is_empty() && pulled => out.push("its conflicts with the patches under it marked".into()),
            Some(m) if !m.is_empty() => out.push("no longer applies on the pulled version".into()),
            Some(_) if pulled => out.push("made again on the patches under it".into()),
            Some(_) => out.push("made again on the pulled version".into()),
            None => {}
        }
        out.join(", ")
    }

    /// Whether it holds conflicts marked, to resolve.
    pub fn marked(&self) -> bool {
        self.remade.as_ref().is_some_and(|m| !m.is_empty())
    }
}

/// What the change pull `r` holds takes from Phabricator, and what the pull
/// found that changes nothing (`Took`): none for any other work, nor for a
/// pull that changes nothing of the stack.
pub fn took(r: &Run) -> Option<&Took> {
    #[cfg(feature = "moz-phab")]
    return r.pull.as_ref()?.took.as_ref();
    #[cfg(not(feature = "moz-phab"))]
    return {
        let _ = r;
        None
    };
}

/// What the change pull `r` holds takes from Phabricator for the patch of
/// `change`, if it takes something.
pub fn taken<'a>(r: &'a Run, change: &str) -> Option<&'a Taken> {
    took(r)?.took.iter().find(|t| t.change == change)
}

/// Whether patch `p` holds a conflict a pull marked, to resolve: a line
/// its diff adds is the marker over the patches under the conflict
/// (`ops::PULL_OURS`). A review does not close with such a patch in the
/// stack.
pub fn marked(p: &Patch) -> bool {
    let marker = format!("+<<<<<<< {}", crate::ops::PULL_OURS);
    p.diff.lines().any(|l| l.trim_end() == marker)
}

/// A follow-up that only reads, asked from the review of the work it came
/// into once it landed.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct FollowUp {
    /// Its run.
    pub run: String,
    /// How the agent was asked: to answer, to build and test, or for a try
    /// push's tasks.
    pub way: crate::db::Way,
    /// The effort it was asked at, if it was asked at one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<crate::db::Effort>,
    /// The model it was asked with, by its id, if it was asked with one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// The comments it was asked about, by (change, id).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub asked: Vec<(String, String)>,
    /// A build and test's: what it found of each patch, as the comments a
    /// build and test opens on the patches' titles, to read in the review,
    /// never filed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub found: Vec<Note>,
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
    /// Pulled, a reviewer's comment edited on Phabricator since it was
    /// filed: what it said, as its turn of the thread says it until the
    /// edit is accepted. Accepted, the edit is their latest turn, and that
    /// turn is redacted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub was: Option<String>,
    /// Pulled with what it said (`was`), not an edit: the same comment, the
    /// same version on Phabricator, filed without the suggested edit
    /// Phabricator gives with it, and read again with it now
    /// (`phab::anew`). Accepted, it takes the place of their turn as an
    /// edit does, said for what it is, as no one edited it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub anew: bool,
    /// Pulled, to a thread you closed: that thread as it was when you
    /// closed it, kept by the pull, which read it from the history, so that
    /// its review shows it, and accepting reopens it, without reading the
    /// history again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed: Option<Closed>,
    /// Your answer to it, written in its review: it follows it in its
    /// thread, and the work goes back to the agent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
    /// The agent's, its run asked at an effort: that effort, which its turn
    /// in the thread says. None for a reviewer's, and for one an ask made
    /// before efforts were picked brought back.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<crate::db::Effort>,
    /// The agent's, its run asked with a model: that model's id, which its
    /// turn in the thread says. None for a reviewer's, and for one an ask
    /// made before models were picked brought back.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// An answer-only ask's: the stack it was asked on, by its rev, which
    /// its turn's fence names, its heading saying how many edits old that
    /// stack is while the stack has changed since. None for another ask's,
    /// a reviewer's, one of before, and a follow-up's on the stack a change
    /// waiting for your review makes, which is no state of the stack yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asked_on: Option<usize>,
    /// To read, not to mark: a build and test's, asked as a follow-up from
    /// the review of the work it came into, which shows it with what that
    /// build and test found. Filed in its thread as the review closes,
    /// after the reply before it there, unless the work's change, or that
    /// reply, is rejected.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub read: bool,
}

/// A thread you closed, as a pull keeps it with a reviewer's reply to it:
/// as it was when you closed it, where it was last open (on its patch, or
/// moved to the stack's comments), and the rev of the record that closed
/// it, its latest then. Written since, the thread's latest record is
/// another: then it is read again from the history.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Closed {
    pub rev: usize,
    pub note: Note,
}

impl Reply {
    /// Thread `text` with this reply as its last turn: the reviewer's who
    /// wrote it, or the agent's, the model and the effort its run was asked
    /// with on it, and
    /// an answer-only ask's the stack it was asked on; your answer to it
    /// after, if you wrote one. A reviewer's edit is their edit of their
    /// turn as it was, which it redacts (`edited`), as their comment read
    /// again with its suggested edit (`anew`) is that comment so read.
    pub fn after(&self, text: &str) -> String {
        let text = match (&self.reviewer, &self.was) {
            (Some(who), Some(was)) => edited(text, who, was, &self.text, self.anew),
            (Some(who), None) => append_turn(text, Role::Reviewer, &self.text, Some(who)),
            (None, _) => append_agent_on(text, &self.text, &self.tag, &self.asked_with(), self.asked_on),
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

    /// It, said: `the agent's reply`, `ann's reply`, `ann's edit`; their
    /// comment read again with its suggested edit, `ann's suggested edit`;
    /// the agent's, its run asked with a model and at an effort, `the
    /// agent's reply (claude-opus-5-5, max)`.
    pub fn said(&self) -> String {
        let what = match (self.is_edit(), self.is_anew()) {
            (true, _) => "edit",
            (_, true) => "suggested edit",
            _ => "reply",
        };
        crate::thread::named_at(&format!("{}'s {what}", self.who()), &self.asked_with())
    }

    /// Whether it is a reviewer's edit, on Phabricator, of their comment
    /// filed: what it said then kept with it (`was`).
    pub fn is_edit(&self) -> bool {
        self.was.is_some() && !self.anew
    }

    /// Whether it is a reviewer's comment filed without the suggested edit
    /// Phabricator gives with it, read again with it (`anew`): not an edit,
    /// what it said then kept with it all the same.
    pub fn is_anew(&self) -> bool {
        self.was.is_some() && self.anew
    }

    /// The model and the effort the agent's run was asked with, if it is
    /// the agent's, as far as its run had them.
    pub fn asked_with(&self) -> crate::thread::At {
        match self.reviewer {
            None => crate::thread::At::new(self.model.as_deref(), self.effort.map(crate::db::Effort::name)),
            Some(_) => crate::thread::At::default(),
        }
    }
}

/// Thread `text` with the comment of reviewer `who` that said `was` saying
/// `now`, as Phabricator has it since they edited it: their edit of it, as
/// their last turn, its heading quoting what it edits (`ann's edit of the
/// comment that said "Hi Da"`); or, `anew`, as stacked reads it since, with
/// the suggested edit Phabricator gives with it, no one having edited it
/// (`ann's comment that said "Hi Da", with its suggested edit`). Their turn
/// that said it, the latest version of their comment, stays as it was, for
/// what answered it to read as it was written, redacted. That turn gone
/// (rewound, say), `now` is their last turn, and nothing is redacted.
fn edited(text: &str, who: &str, was: &str, now: &str, anew: bool) -> String {
    // Each line as it ends aside: Phabricator's may end with a carriage
    // return, a turn's lines do not.
    let plain = |s: &str| s.lines().map(str::trim_end).collect::<Vec<_>>().join("\n").trim().to_string();
    let was = plain(was);
    let mut turns = all_turns(text);
    let at = turns
        .iter()
        .rposition(|t| t.role == Role::Reviewer && !t.deleted && !t.redacted && t.tag.as_deref() == Some(who) && plain(&t.body) == was);
    let edit = at.map(|i| {
        turns[i].redacted = true;
        turns[i].body.lines().next().unwrap_or("").trim().to_string()
    });
    let anew = anew && edit.is_some();
    turns.push(Turn {
        role: Role::Reviewer,
        body: now.trim().to_string(),
        tag: Some(who.to_string()),
        deleted: false,
        edit,
        anew,
        redacted: false,
        effort: None,
        model: None,
        stack: None,
    });
    render_thread(&turns)
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

/// Whether run `r` is a lint job's: the linter's work, its fixes of the
/// stack and what it reported.
pub fn linted(r: &Run) -> bool {
    r.what == crate::db::LINT
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
/// holds its replies and comments as they were, and the patches that should
/// not change that were confirmed for it (`confirm`). The new run. What each
/// operation of its change needs is worked out once it is filed, no lock
/// held (`work_out`): none of what that work held, which was another
/// change's.
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
        needs: None,
        ..h.clone()
    };
    db.add_run(Run {
        id: id.clone(),
        held: Some(held),
        summary: String::new(),
        rev: None,
        ..r.clone()
    })?;
    // The patches that should not change you typed accept for before an
    // edit by hand of yours that this work was: they go on with it.
    if !v.confirmed.is_empty() {
        db.set_review(&id, Review { confirmed: v.confirmed.clone(), ..Default::default() })?;
    }
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
    /// A task a try push ask selected, by its place among them.
    #[cfg(feature = "try-push")]
    Task(usize),
    Reply(usize),
}

impl Part {
    /// As a review's marks name it.
    pub fn key(self) -> String {
        match self {
            Part::Stack => "stack".into(),
            Part::Opened(k) => format!("opened {k}"),
            #[cfg(feature = "try-push")]
            Part::Task(k) => format!("task {k}"),
            Part::Reply(k) => format!("reply {k}"),
        }
    }

    pub fn of_key(key: &str) -> Option<Part> {
        let k = |s: &str| s.parse().ok();
        match key.split_once(' ') {
            None if key == "stack" => Some(Part::Stack),
            Some(("opened", n)) => k(n).map(Part::Opened),
            #[cfg(feature = "try-push")]
            Some(("task", n)) => k(n).map(Part::Task),
            Some(("reply", n)) => k(n).map(Part::Reply),
            _ => None,
        }
    }
}

/// The review of work held, as saved: each part marked accepted (true) or
/// rejected; the replies taken already into their threads; whether it is
/// closed; and the patches that should not change (`protected`) that you
/// typed `accept` for before $EDITOR opened on the edit by hand it is of, by
/// the versions the stack held then, which closing it changes with no
/// one asked again (`confirm`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Review {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub marks: BTreeMap<String, bool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub taken: Vec<usize>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub closed: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub confirmed: Vec<String>,
}

impl Held {
    /// Its parts in the order they are reviewed: the change of the stack,
    /// the comments opened, a try push's tasks, the replies; not those to
    /// read, a build and test's asked as a follow-up.
    pub fn parts(&self) -> Vec<Part> {
        let mut out = Vec::new();
        if self.stack.is_some() {
            out.push(Part::Stack);
        }
        out.extend((0..self.opened.len()).map(Part::Opened));
        #[cfg(feature = "try-push")]
        out.extend((0..self.tasks.len()).map(Part::Task));
        out.extend((0..self.replies.len()).filter(|k| !self.replies[*k].read).map(Part::Reply));
        out
    }

    /// Whether a build and test asked as a follow-up brought something into
    /// it, to read: a reply, or what it found of a patch.
    pub fn found(&self) -> bool {
        self.replies.iter().any(|rp| rp.read) || self.follow_ups.iter().any(|f| !f.found.is_empty())
    }

    /// The replies to comment (change, id), by their places, the first
    /// first: the work's own, then its follow-ups'.
    pub fn replies_to(&self, change: &str, id: &str) -> Vec<usize> {
        (0..self.replies.len()).filter(|&k| self.replies[k].change == change && self.replies[k].id == id).collect()
    }

    /// Whether reply `k` is the last one to its comment: the one that
    /// answers what was said before it there, and the one to answer.
    pub fn last_reply(&self, k: usize) -> bool {
        self.replies.get(k).is_some_and(|rp| self.replies_to(&rp.change, &rp.id).last() == Some(&k))
    }

    /// Thread `text` of comment (change, id) as it would be once its replies
    /// held here are in it, but those `taken` already, which it holds: each
    /// in turn, and your answer after each.
    pub fn thread_but(&self, change: &str, id: &str, text: &str, taken: &[usize]) -> String {
        self.replies_to(change, id).into_iter().filter(|k| !taken.contains(k)).fold(text.to_string(), |t, k| self.replies[k].after(&t))
    }

    /// Thread `text` of the comment reply `k` is to as it would be once
    /// that reply is in it: after the replies before it there, but those
    /// `taken` already, each with your answer after it.
    pub fn thread_upto(&self, k: usize, text: &str, taken: &[usize]) -> String {
        let Some(rp) = self.replies.get(k) else { return text.to_string() };
        self.replies_to(&rp.change, &rp.id)
            .into_iter()
            .take_while(|&j| j <= k)
            .filter(|j| !taken.contains(j))
            .fold(text.to_string(), |t, j| self.replies[j].after(&t))
    }

    /// The comments its follow-ups were asked about.
    pub fn followed(&self) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = Vec::new();
        for at in self.follow_ups.iter().flat_map(|f| f.asked.iter()) {
            if !out.contains(at) {
                out.push(at.clone());
            }
        }
        out
    }

    pub fn is_empty(&self) -> bool {
        self.stack.is_none() && self.replies.is_empty() && self.opened.is_empty() && self.no_tasks() && self.lint.is_none() && self.bug.is_none()
    }

    /// Whether it holds no task of a try push's: it never does, built
    /// without the `try-push` feature.
    fn no_tasks(&self) -> bool {
        #[cfg(feature = "try-push")]
        return self.tasks.is_empty();
        #[cfg(not(feature = "try-push"))]
        return true;
    }

    /// What it holds, said: `a change of patch 2, 2 replies`.
    pub fn said(&self) -> String {
        self.said_but(&[])
    }

    /// What it holds but its replies `taken`, said.
    fn said_but(&self, taken: &[usize]) -> String {
        let mut out = Vec::new();
        if let Some(c) = &self.stack {
            out.push(format!("a change of {}", c.of));
        }
        let replies: Vec<&Reply> = self.replies.iter().enumerate().filter(|(k, rp)| !taken.contains(k) && !rp.read).map(|(_, rp)| rp).collect();
        let edits = replies.iter().filter(|rp| rp.is_edit()).count();
        let anew = replies.iter().filter(|rp| rp.is_anew()).count();
        if replies.len() > edits + anew {
            out.push(count(replies.len() - edits - anew, "reply", "replies"));
        }
        if edits > 0 {
            out.push(count(edits, "edited comment", "edited comments"));
        }
        if anew > 0 {
            out.push(count(anew, "suggested edit", "suggested edits"));
        }
        if !self.opened.is_empty() {
            out.push(count(self.opened.len(), "opened comment", "opened comments"));
        }
        // A try push's tasks first, as they are what it brings.
        #[cfg(feature = "try-push")]
        if !self.tasks.is_empty() {
            out.insert(0, count(self.tasks.len(), "task to push to try", "tasks to push to try"));
        }
        // A bug's draft first too, as it is what it brings.
        if self.bug.is_some() {
            out.insert(0, "a bug to file".to_string());
        }
        // What the linter reported that it did not fix, and what a build
        // and test asked as a follow-up found, last: to read.
        if self.lint.as_ref().is_some_and(|l| !l.errors.is_empty()) {
            out.push("what the linter reported".to_string());
        }
        if self.found() {
            out.push(FOUND.to_string());
        }
        if !self.tools.is_empty() {
            out.push(count(self.tools.len(), "tool the agent could not use", "tools the agent could not use"));
        }
        out.join(", ")
    }

    /// Whether it holds a tool the agent could not use that may be allowed.
    pub fn offers_tools(&self) -> bool {
        self.tools.iter().any(crate::refusals::Refusal::offered)
    }
}

fn count(n: usize, one: &str, many: &str) -> String {
    if n == 1 { format!("1 {one}") } else { format!("{n} {many}") }
}

/// What a build and test asked as a follow-up brings into the review, as
/// what the work holds says it.
pub const FOUND: &str = "what the build and test found";

/// Why your answer to a reply is not written: a later reply follows it in
/// its thread.
pub const LATER_REPLY: &str = "a later reply follows this one in its thread, and answered what you said after it: answer the last one";

/// Why what you wrote on a comment in the review is not written again nor
/// taken back: the follow-up you asked works on it.
pub const FOLLOWING: &str = "the follow-up you asked from this review works on this comment: what it brings back comes into the review";

/// Whether what run `r` brought back waits for review.
pub fn waiting(db: &Db, r: &Run) -> bool {
    r.held.is_some() && !db.review(&r.id).is_some_and(|v| v.closed)
}

/// The runs of stack `name` whose work waits for review, the first landed
/// first.
pub fn queue<'a>(db: &'a Db, name: &str) -> Vec<&'a Run> {
    let mut out: Vec<&Run> = db.live_runs().into_iter().filter(|r| r.stack == name && waiting(db, r)).collect();
    out.sort_by_key(|r| r.held.as_ref().map_or(0, |h| h.at));
    out
}

/// The run whose change of stack `name` waits for review, if one does.
pub fn change_waiting<'a>(db: &'a Db, name: &str) -> Option<&'a Run> {
    queue(db, name).into_iter().find(|r| r.held.as_ref().is_some_and(|h| h.stack.is_some()))
}

/// The parts of the work run `r` holds still to accept or reject: those
/// of a reply taken already left out.
pub fn parts_left(db: &Db, r: &Run) -> Vec<Part> {
    let Some(h) = &r.held else { return vec![] };
    let taken = db.review(&r.id).map(|v| v.taken.clone()).unwrap_or_default();
    h.parts().into_iter().filter(|p| !matches!(p, Part::Reply(k) if taken.contains(k))).collect()
}

/// What is left to review of the work run `r` holds, said: its parts not
/// taken already (`a change of patch 2, 1 reply`); else the comments its
/// ask was about that are still open (`1 asking comment to close or
/// keep`), and whether the agent brought nothing back.
pub fn left(db: &Db, r: &Run) -> String {
    let Some(h) = &r.held else { return String::new() };
    let taken = db.review(&r.id).map(|v| v.taken.clone()).unwrap_or_default();
    let parts = h.said_but(&taken);
    if !parts.is_empty() {
        return parts;
    }
    let mut out = Vec::new();
    let open = asking_open(db, r).len();
    if open > 0 {
        out.push(format!("{} to close or keep", count(open, "asking comment", "asking comments")));
    }
    if h.is_empty() {
        out.push("nothing brought back".to_string());
    }
    if out.is_empty() {
        out.push("nothing left to accept or reject".to_string());
    }
    out.join(", ")
}

/// What work that holds nothing is said to be.
pub const NOTHING_BACK: &str = "the agent brought nothing back";

/// What an ask that brought nothing back says of it, `open` of the `asked`
/// comments it was asked about still open: `the agent brought nothing
/// back, and the comment it was asked about is still open`.
pub fn nothing_back(open: usize, asked: usize) -> String {
    let still = match (open, asked) {
        (0, _) => return NOTHING_BACK.into(),
        (1, 1) => "the comment it was asked about is still open".to_string(),
        (o, a) if o == a => format!("the {a} comments it was asked about are still open"),
        (1, a) => format!("1 of the {a} comments it was asked about is still open"),
        (o, a) => format!("{o} of the {a} comments it was asked about are still open"),
    };
    format!("{NOTHING_BACK}, and {still}")
}

/// The comments whose reply waits for review: to the run, and where the
/// reply is in what it holds.
pub fn replies_waiting(db: &Db) -> HashMap<(String, String), (String, usize)> {
    let mut out = HashMap::new();
    for r in db.live_runs() {
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
    db.live_runs().into_iter().find_map(|r| {
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

/// Where a reply held goes, as the database stands, its history not read:
/// what the review shows of it, and where accepting it files it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Goes<'a> {
    /// Into its thread, open: where it is now, moved since or not.
    Open(&'a Note),
    /// A reviewer's, into a thread you closed, which accepting it reopens:
    /// the thread as the pull kept it, if it did.
    Reopens(Option<&'a Note>),
    /// Nowhere: its thread is gone, or never was.
    Nowhere,
}

/// Where reply `rp`, of work on stack `name`, goes (`Goes`).
pub fn goes<'a>(db: &'a Db, name: &str, rp: &'a Reply) -> Goes<'a> {
    if let Some(n) = db.note(&rp.change, &rp.id) {
        return Goes::Open(n);
    }
    // The agent's: no comment is closed while the agent has it, nor while
    // its reply waits for review.
    if rp.reviewer.is_none() {
        return Goes::Nowhere;
    }
    // Open elsewhere: moved since, with its lines, to the stack's comments
    // or back. Known by its first comment, which the thread holds wherever
    // it is: as the pull kept the thread, else as its id names it.
    let kept = rp.closed.as_ref().map(|c| &c.note);
    let first = kept.and_then(|n| n.remote.first()).map(String::as_str).or_else(|| rp.id.strip_prefix("phab:"));
    if let Some(n) = first.and_then(|f| holding(db, name, f)) {
        return Goes::Open(n);
    }
    match kept {
        Some(n) => Goes::Reopens(Some(n)),
        // Closed, though the pull kept no copy of it: held while it was
        // open, or by a stacked of before.
        None if db.note_rev(&rp.change, &rp.id).is_some() => Goes::Reopens(None),
        None => Goes::Nowhere,
    }
}

/// The open comment of stack `name` that holds remote comment `remote`, if
/// one does.
fn holding<'a>(db: &'a Db, name: &str, remote: &str) -> Option<&'a Note> {
    db.stack_notes(name).into_iter().find(|n| n.remote.iter().any(|r| r == remote))
}

/// Comment (change, id) as its history has it last open, if it ever was:
/// the whole history read.
fn last_open(db: &Db, change: &str, id: &str) -> Option<Note> {
    db.note_history(change, id).into_iter().rev().find(|(_, n)| !n.text.is_empty()).map(|(_, n)| n.clone())
}

/// The thread of reviewer's reply `rp`, of work on stack `name`, which you
/// closed, reopened as `Reopen it` reopens a comment: as it was when
/// closed, not asked again. As the pull kept it, unless the thread was
/// written since: then as its history has it last open, which is read
/// only here, as you accept the reply. Open elsewhere by then, that one,
/// not reopened. The thread, and whether it reopened it; none if it never
/// was.
fn reopen(db: &mut Db, name: &str, rp: &Reply) -> Result<Option<(Note, bool)>> {
    let kept = rp.closed.as_ref().filter(|c| db.note_rev(&rp.change, &rp.id) == Some(c.rev)).map(|c| c.note.clone());
    let Some(was) = kept.or_else(|| last_open(db, &rp.change, &rp.id)) else { return Ok(None) };
    if let Some(n) = was.remote.first().and_then(|f| holding(db, name, f)) {
        return Ok(Some((n.clone(), false)));
    }
    let n = Note { queued: None, ..was };
    db.set_note(n.clone())?;
    Ok(Some((n, true)))
}

/// Reply `rp` put into its thread, as the agent's turn or the reviewer's,
/// the comment moved to stack `name`'s comments if it needs more than its
/// patch. A reviewer's reply to a thread you closed reopens it first
/// (`reopen`). Whether it reopened one.
fn apply_reply(db: &mut Db, name: &str, rp: &Reply) -> Result<bool> {
    let says = !rp.text.trim().is_empty();
    let open = match goes(db, name, rp) {
        Goes::Open(n) => Some(n.clone()),
        Goes::Reopens(_) if says => None,
        // Gone meanwhile, or nothing to say in it: nothing to answer.
        _ => return Ok(false),
    };
    let (mut n, reopened) = match open {
        Some(n) => (n, false),
        None => match reopen(db, name, rp)? {
            Some(at) => at,
            None => return Ok(false),
        },
    };
    if says {
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
        crate::ask::promote(db, name, &n, why, &rp.asked_with())?;
    }
    Ok(reopened)
}

/// Whether the replies of the work run `r` holds may be taken at once, out
/// of its review: they are all it holds, with no change of the stack nor
/// comment opened that they go with, and it does not go on from work that
/// waits for review, which is reviewed first. If not, why.
pub fn can_take(db: &Db, r: &Run) -> Result<()> {
    let Some(h) = &r.held else { return err(format!("run {} holds nothing to review", r.id)) };
    if h.stack.is_some() && pulled(r) {
        return err("this reply goes with the patches pulled with it: review them together");
    }
    if h.stack.is_some() {
        return err("this reply goes with the agent's change of the stack: review them together");
    }
    if !h.opened.is_empty() {
        let opened = if pulled(r) { "the new comments pulled with it" } else { "the comments the agent opened" };
        return err(format!("this reply goes with {opened}: review them together"));
    }
    if !h.no_tasks() {
        return err("this reply goes with the tasks the agent selected for a try push: review them together");
    }
    if h.offers_tools() {
        return err("this reply goes with the tools the agent could not use: review them together");
    }
    if crate::ask::guessing(db, r).is_some() {
        return err("this reply goes on from the work landed before it, which waits for your review: review that one first");
    }
    if !h.follow_ups.is_empty() || !crate::ask::follow_ups_going(db, &r.id).is_empty() {
        return err("this reply goes with a follow-up you asked from its review: review them together");
    }
    Ok(())
}

/// Takes reply `k` of what run `id` brought back into its thread at once,
/// accepted, out of the review, if `can_take` lets it: a reviewer's reply
/// to a thread you closed reopens it. The last reply taken, the review
/// closes: the comments the ask was about stay open, as any comment does.
pub fn take(db: &mut Db, id: &str, k: usize) -> Result<()> {
    let (r, h, mut v) = under_review(db, id)?;
    can_take(db, &r)?;
    let rp = h.replies.get(k).ok_or_else(|| Error(format!("run {id} holds no reply {k}")))?;
    if v.taken.contains(&k) {
        return Ok(());
    }
    apply_reply(db, &r.stack, rp)?;
    v.taken.push(k);
    v.marks.insert(Part::Reply(k).key(), true);
    v.closed = (0..h.replies.len()).all(|j| v.taken.contains(&j));
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
/// applied. A pull's comments are not rejected.
pub fn save(db: &mut Db, id: &str, marks: &BTreeMap<String, bool>) -> Result<()> {
    let (r, h, mut v) = under_review(db, id)?;
    if pulled(&r) && rejects(&h, marks) {
        return err(FILED);
    }
    v.marks = marks.clone();
    db.set_review(id, v).map(|_| ())
}

/// Keeps with run `id`'s review `ids`, patches that should not change
/// (`protected`), by the versions the stack holds, which you typed `accept`
/// for before $EDITOR opened on the edit by hand it holds: closing the
/// review changes them with no one asked again, as that confirmation was
/// for that edit, and its review's close is what writes it.
pub fn confirm(db: &mut Db, id: &str, ids: &[String]) -> Result<()> {
    let (_, _, mut v) = under_review(db, id)?;
    for i in ids {
        if !v.confirmed.contains(i) {
            v.confirmed.push(i.clone());
        }
    }
    db.set_review(id, v).map(|_| ())
}

/// What refusing to reject a comment pulled from Phabricator says: each is
/// filed as its pull's review closes.
pub const FILED: &str = "a comment pulled from Phabricator cannot be rejected: close the review to file it";

/// Whether `marks` rejects a comment of work `h`, a part other than its
/// change of the stack, whose operations a pull's review marks as any.
fn rejects(h: &Held, marks: &BTreeMap<String, bool>) -> bool {
    h.parts().iter().filter(|p| **p != Part::Stack).any(|p| marks.get(&p.key()) == Some(&false))
}

/// The marks of the review of the work run `r` holds, as saved; of a
/// pull's, every comment accepted, as none is rejected: closing its review
/// files each comment it pulled. Its change of the stack's operations are
/// marked as any change's. Marks a stacked of before saved rejecting a
/// comment are passed over. The change of the stack marked as a whole, as
/// a stacked of before marked it (`stack`), each of its operations has
/// that mark, once each is marked on its own (`operations::of_review`); a
/// change marked as one keeps it.
pub fn marks(db: &Db, r: &Run) -> BTreeMap<String, bool> {
    let mut marks = db.review(&r.id).map(|v| v.marks.clone()).unwrap_or_default();
    if pulled(r) {
        marks.extend(r.held.iter().flat_map(Held::parts).filter(|p| *p != Part::Stack).map(|p| (p.key(), true)));
    }
    if let Some(&m) = marks.get(&Part::Stack.key())
        && let Some(ops) = r.held.as_ref().and_then(|h| crate::operations::of_review(db, r, h)).filter(|o| !o.whole())
    {
        marks.remove(&Part::Stack.key());
        if !marks.keys().any(|k| crate::operations::is_key(k)) {
            marks.extend(ops.marked().map(|o| (o.key.clone(), m)));
        }
    }
    marks
}

/// What you wrote in the review of work `r`, to send back: the comments
/// written there, still open, that no reply held in the review answers;
/// and your answers to its replies, each to the last reply of its comment,
/// to the reasons of a try push's tasks, and to what it said of the tools
/// it could not use. A comment written there is one you opened in it, or
/// one whose thread you wrote in there, whoever opened it (a reviewer, the
/// agent); on the stack or one of its patches, or on a patch the work adds
/// (`added_changes`), which the stack holds only once the work is accepted.
/// What went with a follow-up of it that is going is with the agent, and
/// none of it. With any, the work is not accepted: it goes back to the
/// agent.
pub fn yours_in(db: &Db, r: &Run) -> (Vec<(String, String)>, usize) {
    yours_but(db, r, &crate::ask::following(db, &r.id))
}

/// As `yours_in`, but for the comments `busy` holds, those a follow-up
/// going is asked about.
fn yours_but(db: &Db, r: &Run, busy: &HashSet<(String, String)>) -> (Vec<(String, String)>, usize) {
    let h = r.held.as_ref();
    let mut notes = db.stack_notes(&r.stack);
    for change in added_changes(db, r) {
        notes.extend(db.notes(&change));
    }
    let opened = notes
        .into_iter()
        .filter(|n| n.review.as_deref() == Some(r.id.as_str()))
        .filter(|n| h.is_none_or(|h| h.replies_to(&n.change, &n.id).is_empty()))
        .map(|n| (n.change.clone(), n.id.clone()))
        .filter(|at| !busy.contains(at))
        .collect();
    let answers = h.map_or(0, |h| answered(h, busy).len() + h.task_answers() + h.tools.iter().filter(|t| t.answer.is_some()).count());
    (opened, answers)
}

/// The replies of work `h` your answer to goes back to the agent with, by
/// their places: each the last reply of its comment, which no follow-up
/// going (`busy`) is asked about. Your answer to a reply a later one
/// follows went with the follow-up that brought that one.
pub fn answered(h: &Held, busy: &HashSet<(String, String)>) -> Vec<usize> {
    (0..h.replies.len())
        .filter(|&k| {
            let rp = &h.replies[k];
            rp.answer.is_some() && h.last_reply(k) && !busy.contains(&(rp.change.clone(), rp.id.clone()))
        })
        .collect()
}

/// The comments of the review of work `r`, still open: those its ask was
/// about, those written in its review, and those its follow-ups were asked
/// about, each once.
fn in_review(db: &Db, r: &Run) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = asking_of(r);
    let mut notes = db.stack_notes(&r.stack);
    for change in added_changes(db, r) {
        notes.extend(db.notes(&change));
    }
    out.extend(notes.into_iter().filter(|n| n.review.as_deref() == Some(r.id.as_str())).map(|n| (n.change.clone(), n.id.clone())));
    out.extend(r.held.iter().flat_map(Held::followed));
    let mut seen: Vec<(String, String)> = Vec::new();
    for at in out {
        if !seen.contains(&at) && db.note(&at.0, &at.1).is_some() {
            seen.push(at);
        }
    }
    seen
}

/// What goes back to the agent with the comment of the review of work `h`
/// that asked first, as a turn of yours after its reply: your answers to
/// why a try push's tasks were selected, then what the review allows of
/// the tools the agent could not use, as `marks` mark them, and what you
/// answered to them. None for none.
fn said_back(h: &Held, marks: &BTreeMap<String, bool>) -> Option<String> {
    #[cfg_attr(not(feature = "try-push"), allow(unused_mut))]
    let mut said: Vec<String> = Vec::new();
    #[cfg(feature = "try-push")]
    said.extend(crate::try_push::answers_turn(&h.tasks));
    said.extend(crate::refusals::turn(&h.tools, marks));
    (!said.is_empty()).then(|| said.join("\n\n"))
}

/// The comments of the review of work `r` that go back to the agent, as
/// their threads would be once its replies held are in them: those its ask
/// was about, those written in its review and those its follow-ups were
/// asked about (`in_review`) whose last word would be yours or a
/// reviewer's; and the first that asked, while something goes back with it
/// (`said_back`, as `marks` mark the tools). Not one a follow-up going is
/// asked about (`busy`), nor one waiting its turn.
pub fn going_back(db: &Db, r: &Run, marks: &BTreeMap<String, bool>, busy: &HashSet<(String, String)>) -> Vec<(String, String)> {
    let Some(h) = &r.held else { return vec![] };
    let taken = db.review(&r.id).map(|v| v.taken.clone()).unwrap_or_default();
    // Waiting its turn in a follow-up of this work, not counted as busy: as
    // it would be once that follow-up is stopped, off the queue.
    let queued = |n: &Note| n.queued.clone().filter(|q| !(q.follow && q.on.as_deref() == Some(r.id.as_str())));
    let mut out: Vec<(String, String)> = Vec::new();
    let first = asking_open(db, r).first().map(|(_, n)| (*n).clone());
    if said_back(h, marks).is_some()
        && let Some(n) = &first
        && queued(n).is_none()
        && !busy.contains(&(n.change.clone(), n.id.clone()))
    {
        out.push((n.change.clone(), n.id.clone()));
    }
    for at in in_review(db, r) {
        if out.contains(&at) || busy.contains(&at) {
            continue;
        }
        let Some(n) = db.note(&at.0, &at.1) else { continue };
        let would = Note {
            text: h.thread_but(&n.change, &n.id, &n.text, &taken),
            queued: queued(n),
            ..n.clone()
        };
        if crate::ask::sendable(&would) {
            out.push(at);
        }
    }
    out
}

/// The changes of the patches the stack that work `r` makes holds and its
/// stack does not, in the order the work has them: those the work adds, or
/// splits a patch into, and those a round sent back carries from the work
/// it goes on from. None for work that does not change the stack.
fn added_changes(db: &Db, r: &Run) -> Vec<String> {
    let Some(c) = r.held.as_ref().and_then(|h| h.stack.as_ref()) else { return vec![] };
    let held = held_changes(db, &r.stack);
    let mut out: Vec<String> = Vec::new();
    for id in &c.patches {
        let change = &db.patch(id).change;
        if !held.contains(change) && !out.contains(change) {
            out.push(change.clone());
        }
    }
    out
}

/// The changes of the patches stack `name` holds: none for no stack.
fn held_changes(db: &Db, name: &str) -> std::collections::HashSet<String> {
    db.stack(name).map(|s| s.patches.iter().map(|id| db.patch(id).change.clone()).collect()).unwrap_or_default()
}

/// Takes back what you wrote in the review of run `id` on comment (change,
/// `note`), for it not to go back to the agent with the work: one you
/// opened there is closed; a thread from before the review (yours, a
/// reviewer's, the agent's) is as it was then, its history read for it.
/// Whether it was a thread from before.
pub fn take_back(db: &mut Db, id: &str, change: &str, note: &str) -> Result<bool> {
    let n = db.note(change, note).cloned().ok_or_else(|| Error("the comment is gone".into()))?;
    if crate::ask::following(db, id).contains(&(change.to_string(), note.to_string())) {
        return err(FOLLOWING);
    }
    let before = db
        .note_history(change, note)
        .into_iter()
        .rev()
        .map(|(_, n)| n)
        .find(|n| n.review.as_deref() != Some(id))
        .filter(|n| !n.text.is_empty())
        .cloned();
    match before {
        Some(was) => db.set_note(Note { time: now(), ..was }).map(|_| true),
        None => db.set_note(Note { text: String::new(), ..n }).map(|_| false),
    }
}

impl Held {
    /// How many of a try push's tasks you answered the reason of, in their
    /// review: none, built without the `try-push` feature.
    fn task_answers(&self) -> usize {
        #[cfg(feature = "try-push")]
        return self.tasks.iter().filter(|t| t.answer.is_some()).count();
        #[cfg(not(feature = "try-push"))]
        return 0;
    }
}

/// Writes `text`, your answer to reply `k` of the work run `id` holds, in
/// its review: empty, it takes it back. Not to a reply a later one
/// follows, which answered what you said after it; nor while a follow-up
/// you asked from the review works on its comment.
pub fn answer(db: &mut Db, id: &str, k: usize, text: &str) -> Result<()> {
    let (r, mut h, _) = under_review(db, id)?;
    if k < h.replies.len() && !h.last_reply(k) {
        return err(LATER_REPLY);
    }
    if h.replies.get(k).is_some_and(|rp| crate::ask::following(db, id).contains(&(rp.change.clone(), rp.id.clone()))) {
        return err(FOLLOWING);
    }
    let rp = h.replies.get_mut(k).ok_or_else(|| Error(format!("run {id} holds no reply {k}")))?;
    rp.answer = Some(text.trim().to_string()).filter(|t| !t.is_empty());
    db.add_run(Run { held: Some(h), ..r }).map(|_| ())
}

/// Writes `text`, your answer to why the agent selected task `k` of the
/// try push run `id` holds, in its review: empty, it takes it back.
#[cfg(feature = "try-push")]
pub fn answer_task(db: &mut Db, id: &str, k: usize, text: &str) -> Result<()> {
    let (r, mut h, _) = under_review(db, id)?;
    let t = h.tasks.get_mut(k).ok_or_else(|| Error(format!("run {id} holds no task {k}")))?;
    t.answer = Some(text.trim().to_string()).filter(|t| !t.is_empty());
    db.add_run(Run { held: Some(h), ..r }).map(|_| ())
}

/// Writes `text`, your answer to what the agent said of tool `k` it could
/// not use, of the work run `id` holds, in its review: empty, it takes it
/// back.
pub fn answer_tool(db: &mut Db, id: &str, k: usize, text: &str) -> Result<()> {
    let (r, mut h, _) = under_review(db, id)?;
    let t = h.tools.get_mut(k).ok_or_else(|| Error(format!("run {id} holds no tool {k}")))?;
    t.answer = Some(text.trim().to_string()).filter(|t| !t.is_empty());
    db.add_run(Run { held: Some(h), ..r }).map(|_| ())
}

/// What goes back to the agent from a review: the comments to ask again,
/// how, whether of the whole stack, and at what effort and with what
/// model, the work's own.
#[derive(Debug, Clone, PartialEq)]
pub struct Round {
    pub notes: Vec<(String, String)>,
    pub way: crate::db::Way,
    pub whole: bool,
    pub effort: Option<crate::db::Effort>,
    pub model: Option<String>,
    /// How many edits that went on from it are asked again.
    pub dropped: usize,
    /// How many follow-ups asked from its review were stopped, as it went
    /// back.
    pub stopped: usize,
    /// What the next round may use for this work, of what the agent was
    /// refused; and what the review saved for good, or could not.
    pub allowed: Vec<String>,
    pub settled: crate::refusals::Settled,
}

/// Rejects the work run `id` holds, to send it back to the agent as what
/// you wrote in its review asks: its change is not applied, but the next
/// round goes on from it; its replies go in their threads, your answers
/// after them; the comments it opened, as `marks` says (those not marked,
/// kept). A follow-up asked from its review that is still going is
/// stopped, what it was asked going back with the work. What to ask again:
/// the comments of its ask whose last word is yours now, those you opened
/// in its review, and those its follow-ups were asked about.
pub fn send_back(db: &mut Db, id: &str, marks: &BTreeMap<String, bool>) -> Result<Round> {
    let (r, h, mut v) = under_review(db, id)?;
    if crate::ask::guessing(db, &r).is_some() {
        return err("this work goes on from the work landed before it, which waits for your review: review that one first");
    }
    if pulled(&r) {
        return send_back_pull(db, id);
    }
    let i = crate::ask::info(&r).ok_or_else(|| Error(format!("run {id} is not the agent's work: there is no one to send it back to")))?;
    // What went with a follow-up going goes back too: it is stopped.
    let none = HashSet::new();
    let (opened, answers) = yours_but(db, &r, &none);
    // A tool allowed is reason enough: the agent tries again with it.
    let mut marks = marks.clone();
    let (work, always) = crate::refusals::allowed(&h.tools, &marks);
    if opened.is_empty() && answers == 0 && work.is_empty() && always.is_empty() {
        return err("you wrote nothing in its review to send back, nor allowed a tool the agent could not use: accept or reject it");
    }
    if going_back(db, &r, &marks, &none).is_empty() {
        return err("none of what you wrote in its review is open to send back");
    }
    // What is allowed from now on is saved for the next round to use, once
    // nothing stops the work going back. Where it cannot be, it is allowed
    // for this work instead, the agent told so.
    let early = (!always.is_empty() && crate::refusals::local_of(db, &r).is_err()).then(|| crate::refusals::settle(db, &r, &mut marks, true));
    let stopped = crate::ask::stop_follow_ups(db, id, crate::ask::FOLLOW_SENT)?;
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
    // A try push's: your answers to the reasons of its tasks; and the tools
    // the agent could not use, those you allowed and those you answered.
    // Each a turn of yours in the thread of the first comment that asked,
    // still open, as one: with it, that comment is asked again. How you
    // marked each, the next round is told, and given what you allowed.
    #[cfg(feature = "try-push")]
    v.marks.extend(marks.iter().filter(|(k, _)| matches!(Part::of_key(k), Some(Part::Task(_)))).map(|(k, m)| (k.clone(), *m)));
    if let Some(said) = said_back(&h, &marks)
        && let Some(n) = asking_open(db, &r).first().map(|(_, n)| (*n).clone())
    {
        db.set_note(Note { text: append_turn(&n.text, Role::You, &said, None), ..n })?;
    }
    let mut notes: Vec<(String, String)> = Vec::new();
    for at in in_review(db, &r) {
        if !notes.contains(&at) && db.note(&at.0, &at.1).is_some_and(crate::ask::sendable) {
            notes.push(at);
        }
    }
    if notes.is_empty() {
        return err("none of what you wrote in its review is open to send back");
    }
    let settled = match early {
        Some(s) => s,
        None => crate::refusals::settle(db, &r, &mut marks, true),
    };
    v.marks.retain(|k, _| !crate::refusals::is_key(k));
    v.marks.extend(marks.iter().filter(|(k, _)| crate::refusals::is_key(k)).map(|(k, m)| (k.clone(), *m)));
    // Asked together, as they were, unless they reach past the place the
    // ask was about.
    let places: HashSet<crate::ask::Scope> = notes.iter().filter_map(|(c, i)| db.note(c, i)).map(crate::ask::scope_of).collect();
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
        summary: format!("reviewed: sent back to the agent, {}{}", count(n, "comment to ask again", "comments to ask again"), settled.said(i.way)),
        ..r
    })?;
    Ok(Round {
        notes,
        way: i.way,
        whole,
        effort: i.effort,
        model: i.model.clone(),
        dropped,
        stopped,
        allowed: crate::refusals::carried(db, id),
        settled,
    })
}

/// The patches of `patches`, a stack the review of a pull would leave, that
/// hold a conflict it marked, said with what to do, when one does: the
/// review does not close with them.
pub fn marked_said(db: &Db, patches: &[String]) -> Option<String> {
    let at: Vec<usize> = patches.iter().enumerate().filter(|(_, id)| marked(db.patch(id))).map(|(k, _)| k).collect();
    match at.as_slice() {
        [] => None,
        [k] => Some(format!(
            "patch {} ({}) holds the conflicts the pull marked where it does not apply on the patches under it: reject its operation, resolve them by hand in the patch, or send the pull back to the agent",
            k + 1,
            db.patch(&patches[*k]).title()
        )),
        _ => Some(format!(
            "{} hold the conflicts the pull marked where they do not apply on the patches under them: reject their operations, resolve them by hand in the patches, or send the pull back to the agent",
            crate::head::patches_said(&at)
        )),
    }
}

/// What goes back to the agent from the review of a pull whose change
/// holds patches whose conflicts it marked: the comment of yours that hands
/// them over, as the lint job hands its fixes over, on the patch whose
/// conflicts are marked, or on the stack for more than one; its text says
/// which patches no longer apply on the pulled versions, the files their
/// conflicts are marked in, and what to do. The patches, by their places
/// in the stack the pull makes, from 0, and the comment; none without a
/// conflict marked.
pub fn handed_over(db: &Db, r: &Run, h: &Held) -> Option<(Vec<usize>, Note)> {
    let c = h.stack.as_ref()?;
    let at: Vec<usize> = c.patches.iter().enumerate().filter(|(_, p)| marked(db.patch(p))).map(|(k, _)| k).collect();
    let files = |k: usize| taken(r, &db.patch(&c.patches[k]).change).and_then(|t| t.remade.clone()).unwrap_or_default();
    let markers = format!("`<<<<<<< {}`, `=======` and `>>>>>>>`", crate::ops::PULL_OURS);
    let note = match at.as_slice() {
        [] => return None,
        [k] => {
            let p = db.patch(&c.patches[*k]);
            let fs = files(*k);
            let shown = if fs.is_empty() { String::new() } else { format!(", in {}", crate::head::and(&fs)) };
            Note {
                change: p.change.clone(),
                id: format!("p{}", crate::db::unique()),
                line: crate::view::title_key(p),
                file: crate::view::MSG_PATH.into(),
                preview: p.title().chars().take(80).collect(),
                text: crate::thread::yours(&format!(
                    "This patch no longer applies on the versions pulled from Phabricator under it: its conflicts are marked in its files{shown}, as {markers}. Resolve them, keeping what this patch changes, and take every marker out, so that every patch of the stack applies."
                )),
                time: now(),
                ..Default::default()
            }
        }
        _ => {
            let each: Vec<String> = at
                .iter()
                .map(|&k| {
                    let fs = files(k);
                    let p = db.patch(&c.patches[k]);
                    match fs.is_empty() {
                        true => format!("patch {} ({})", k + 1, p.title()),
                        false => format!("patch {} ({}), in {}", k + 1, p.title(), crate::head::and(&fs)),
                    }
                })
                .collect();
            Note {
                change: crate::db::stack_change(&r.stack),
                id: format!("p{}", crate::db::unique()),
                text: crate::thread::yours(&format!(
                    "These patches no longer apply on the versions pulled from Phabricator under them, their conflicts marked in their files, as {markers}: {}. Resolve each, keeping what each patch changes, and take every marker out, so that every patch of the stack applies.",
                    each.join("; ")
                )),
                time: now(),
                ..Default::default()
            }
        }
    };
    Some((at, note))
}

/// The work of pull `id` sent back, its change holding patches whose
/// conflicts it marked: to an agent asked to rework them, starting from the
/// stack the pull makes. Its review closes: its comments filed, as closing
/// files them, those of a patch the stack does not hold left to be pulled
/// again; its change not applied. Then the comment of yours that hands the
/// conflicts over (`handed_over`) is opened, for the round to ask the agent
/// about: as a change of the stack, from the work.
fn send_back_pull(db: &mut Db, id: &str) -> Result<Round> {
    let (r, h, mut v) = under_review(db, id)?;
    let Some((at, note)) = handed_over(db, &r, &h) else {
        return err("no patch of this pull's change holds a conflict it marked: there is nothing to send back to the agent, so accept or reject it");
    };
    let mut reopened = 0;
    for (k, rp) in h.replies.iter().enumerate() {
        if !v.taken.contains(&k) && apply_reply(db, &r.stack, rp)? {
            reopened += 1;
        }
    }
    let held_now = held_changes(db, &r.stack);
    for n in &h.opened {
        if held_now.contains(&n.change) || n.on_stack() {
            db.set_note(Note { time: now(), ..n.clone() })?;
        }
    }
    db.set_note(note.clone())?;
    v.marks.insert(Part::Stack.key(), false);
    v.closed = true;
    db.set_review(id, v)?;
    let dropped = crate::ask::guessed_wrong(db, id, "sent back")?;
    let mut said = format!("reviewed: its change sent back to the agent, to rework {}", crate::head::patches_said(&at));
    if !h.opened.is_empty() || !h.replies.is_empty() {
        said += "; the comments it pulled filed";
    }
    if reopened > 0 {
        said += &format!("; {}", count(reopened, "comment reopened", "comments reopened"));
    }
    db.add_run(Run { summary: said, ..r })?;
    Ok(Round {
        notes: vec![(note.change.clone(), note.id.clone())],
        way: crate::db::Way::Change,
        whole: false,
        effort: None,
        model: None,
        dropped,
        stopped: 0,
        allowed: Vec::new(),
        settled: crate::refusals::Settled::default(),
    })
}

/// What a follow-up that only reads asks, from the review of the work run
/// `id` holds, which stays waiting for your review, its marks as they are:
/// the comments that go back (`going_back`), as their threads would be. The
/// follow-up reads the stack that work makes. What goes back with the
/// first comment that asked (`said_back`) goes in its thread as the work
/// going back would put it there: your answer after its last reply held
/// here, or, with none, a turn of yours in it. What the review allows from
/// now on is saved first, as when the work goes back, where it cannot be
/// allowed for this work instead; and the marks of the tools and of a try
/// push's tasks are saved, for the follow-up to read. The comments, the
/// way the work was asked, at what effort, and what the follow-up may use
/// for this work.
pub fn follow_up(db: &mut Db, id: &str, marks: &BTreeMap<String, bool>) -> Result<Round> {
    let (r, mut h, mut v) = under_review(db, id)?;
    if crate::ask::guessing(db, &r).is_some() {
        return err("this work goes on from the work landed before it, which waits for your review: review that one first");
    }
    let i = crate::ask::info(&r).ok_or_else(|| Error(format!("run {id} is not the agent's work: there is no one to ask")))?;
    let mut marks = marks.clone();
    let notes = going_back(db, &r, &marks, &crate::ask::following(db, id));
    if notes.is_empty() {
        return err("none of what you wrote in its review is open to send back");
    }
    let settled = crate::refusals::settle(db, &r, &mut marks, true);
    if let Some(said) = said_back(&h, &marks)
        && let Some(n) = asking_open(db, &r).first().map(|(_, n)| (*n).clone())
    {
        match h.replies_to(&n.change, &n.id).into_iter().rfind(|k| !v.taken.contains(k)) {
            Some(k) => {
                let rp = &mut h.replies[k];
                rp.answer = Some(match rp.answer.take() {
                    Some(a) => format!("{a}\n\n{said}"),
                    None => said,
                });
            }
            None => db.set_note(Note {
                text: append_turn(&n.text, Role::You, &said, None),
                review: Some(id.to_string()),
                ..n
            })?,
        }
        // Said, as your answer or a turn of yours: no longer to send back.
        #[cfg(feature = "try-push")]
        for t in &mut h.tasks {
            t.answer = None;
        }
        for t in &mut h.tools {
            t.answer = None;
        }
        db.add_run(Run {
            held: Some(h.clone()),
            ..r.clone()
        })?;
    }
    v.marks.retain(|k, _| !crate::refusals::is_key(k));
    v.marks.extend(marks.iter().filter(|(k, _)| crate::refusals::is_key(k)).map(|(k, m)| (k.clone(), *m)));
    #[cfg(feature = "try-push")]
    v.marks.extend(marks.iter().filter(|(k, _)| matches!(Part::of_key(k), Some(Part::Task(_)))).map(|(k, m)| (k.clone(), *m)));
    db.set_review(id, v)?;
    Ok(Round {
        notes,
        way: i.way,
        whole: false,
        effort: i.effort,
        model: i.model.clone(),
        dropped: 0,
        stopped: 0,
        allowed: crate::refusals::carried(db, id),
        settled,
    })
}

/// What follow-up run `f`, of ask `i`, brought back (`brought`), put into
/// the review of the work it follows up, while that work waits for your
/// review: its replies after the work's, a build and test's to read; the
/// comments an answer opened, among those the work opened; what a build
/// and test found of each patch, to read; a try push's tasks, but those the
/// work holds already, each to accept or reject; and the tools it could not
/// use, but those the work holds already. Landed `ok` or not: failed, its
/// tools alone come in. What came in, said, for the follow-up's run; none
/// if it is no follow-up, if the work no longer waits for your review, or
/// if, failed, it brought nothing that comes in.
pub fn come_in(db: &mut Db, f: &Run, i: &crate::ask::Info, brought: &Held, ok: bool) -> Result<Option<String>> {
    let Some(on) = i.on.as_deref().filter(|_| i.follow) else { return Ok(None) };
    let Some(w) = db.run(on).cloned().filter(|w| waiting(db, w)) else { return Ok(None) };
    let mut h = w.held.clone().unwrap_or_default();
    // What came in, as a work of its own would say it.
    let mut came = Held::default();
    if ok {
        let read = i.way == crate::db::Way::Test;
        came.replies = brought.replies.iter().map(|rp| Reply { read, ..rp.clone() }).collect();
        h.replies.extend(came.replies.iter().cloned());
        let found = if read { brought.opened.clone() } else { vec![] };
        if !read {
            came.opened = brought.opened.clone();
            h.opened.extend(came.opened.iter().cloned());
        }
        #[cfg(feature = "try-push")]
        {
            for t in &brought.tasks {
                if !h.tasks.iter().any(|x| x.label == t.label) {
                    h.tasks.push(t.clone());
                    came.tasks.push(t.clone());
                }
            }
            h.rebuild = h.rebuild.max(brought.rebuild);
        }
        let follow = FollowUp {
            run: f.id.clone(),
            way: i.way,
            effort: i.effort,
            model: i.model.clone(),
            asked: i.notes.iter().map(|sh| (sh.change.clone(), sh.id.clone())).collect(),
            found,
        };
        came.follow_ups.push(follow.clone());
        h.follow_ups.push(follow);
    }
    for t in &brought.tools {
        if !h.tools.iter().any(|x| x.rule() == t.rule()) {
            h.tools.push(t.clone());
            came.tools.push(t.clone());
        }
    }
    if !ok && came.tools.is_empty() {
        return Ok(None);
    }
    db.add_run(Run { held: Some(h), ..w })?;
    let said = came.said();
    Ok(Some(match (ok, said.is_empty()) {
        (true, true) => "the agent brought nothing back into the review of the work it follows up".to_string(),
        (true, false) => format!("its work came into the review of the work it follows up: {said}"),
        (false, _) => format!("the tools it could not use came into the review of the work it follows up: {said}"),
    }))
}

/// The mark, in a review, of the comment the ask was about at place `k`
/// among them: true to close it with the review; else it is kept.
pub fn asking_key(k: usize) -> String {
    format!("asking {k}")
}

/// What becomes of an answer made on the stack before a change once the
/// change is accepted, as the change's review marks it (`answer_mark`):
/// kept, it goes on, or waits for your review, as it is, on the stack it
/// was asked on; restarted, it is asked again on the stack the change
/// makes; stopped, its agent is stopped, or its work thrown away unseen,
/// and its comments stay open, not asked again (`ask::leave_answers`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnswerMark {
    Keep,
    Restart,
    Stop,
}

impl AnswerMark {
    pub const ALL: [AnswerMark; 3] = [AnswerMark::Keep, AnswerMark::Restart, AnswerMark::Stop];

    /// It, in a word, as the review's list says it: `keep`, `restart`,
    /// `stop`; and as its mark in the review's marks starts.
    pub fn word(self) -> &'static str {
        match self {
            AnswerMark::Keep => "keep",
            AnswerMark::Restart => "restart",
            AnswerMark::Stop => "stop",
        }
    }
}

/// The mark, in a review, saying that the answer of run `run`, made on the
/// stack before the change, is marked `m`: true.
pub fn answer_key(m: AnswerMark, run: &str) -> String {
    format!("{} {run}", m.word())
}

/// How `marks` mark the answer of run `run`, if they do.
pub fn answer_mark(marks: &BTreeMap<String, bool>, run: &str) -> Option<AnswerMark> {
    AnswerMark::ALL.into_iter().find(|m| marks.get(&answer_key(*m, run)) == Some(&true))
}

/// The answer of run `run` marked `m` in `marks`, in place of how it was.
pub fn set_answer_mark(marks: &mut BTreeMap<String, bool>, run: &str, m: AnswerMark) {
    for other in AnswerMark::ALL {
        marks.remove(&answer_key(other, run));
    }
    marks.insert(answer_key(m, run), true);
}

/// Whether `key` is a review's mark of an answer made on the stack before
/// the change.
pub fn is_answer_key(key: &str) -> bool {
    AnswerMark::ALL.iter().any(|m| key.strip_prefix(m.word()).is_some_and(|rest| rest.starts_with(' ')))
}

/// What the answer of run `r` was asked about, as a review's list says a
/// comment: where its first comment is (`a.txt: TWO`, `the stack`), then
/// the first words of that comment; `and 2 more comments` after, when it
/// was asked about more. Read from its run without the rest, as it is read
/// on every redraw.
pub fn answer_about(db: &Db, r: &Run) -> String {
    let notes = crate::ask::asked_about(r);
    let Some((change, id)) = notes.first() else { return "no comment".into() };
    let mut out = match db.note(change, id) {
        Some(n) => {
            let at = if n.on_stack() { "the stack".to_string() } else { n.where_said() };
            let words = crate::thread::thread_turns(&n.text).first().map(|t| crate::markdown::first_line(&t.body)).unwrap_or_default();
            format!("{at} · {words}")
        }
        None => "a comment closed since".into(),
    };
    match notes.len() {
        1 => {}
        2 => out += " · and 1 more comment",
        n => out += &format!(" · and {} more comments", n - 1),
    }
    out
}

/// Whether the answer of run `r` is running, else landed and waiting for
/// your review, said: `running`, `waiting for your review`.
pub fn answer_state(r: &Run) -> &'static str {
    if r.state == crate::db::RunState::Started { "running" } else { "waiting for your review" }
}

/// What refusing to close a review with its change accepted says, while
/// `unmarked`, answers made on the stack before the change, are not marked
/// keep, restart or stop: each by its run, where it stands, and what it was
/// asked about; and where to mark them.
fn unmarked_said(db: &Db, unmarked: &[&Run]) -> String {
    let each: Vec<String> = unmarked.iter().map(|r| format!("{} ({}, on {})", r.id, answer_state(r), answer_about(db, r))).collect();
    let (n, is) = match unmarked.len() {
        1 => ("1 answer".to_string(), "is"),
        n => (format!("{n} answers"), "are"),
    };
    format!(
        "{n} made on the stack before this change {is} not marked keep, restart or stop: {}. Mark each in the review of this work in the editor, then close the review there, or save it",
        each.join("; ")
    )
}

/// The comments the ask of run `r` was about, by (change, id): an agent's
/// work's; none for a pull's, or an edit by hand's.
pub fn asking_of(r: &Run) -> Vec<(String, String)> {
    crate::ask::info(r).map(|i| i.notes.into_iter().map(|sh| (sh.change, sh.id)).collect()).unwrap_or_default()
}

/// The comments the ask of run `r` was about that are still open, neither
/// closed nor gone, each with its place among them.
pub fn asking_open<'a>(db: &'a Db, r: &Run) -> Vec<(usize, &'a Note)> {
    asking_of(r).into_iter().enumerate().filter_map(|(k, (c, i))| Some((k, db.note(&c, &i)?))).collect()
}

/// Closes run `id`'s review: each part as marked in `marks`, those left
/// unmarked accepted or rejected as `rest` says (with none, every part must
/// be marked), the change of the stack operation by operation. Rejecting
/// every operation of the change rejects its replies and comments too.
/// What is accepted is applied, the change of the stack only if the stack
/// is still the one the agent took: whole, or in part, as the stack its
/// operations accepted leave, which must apply, what went on from it then
/// dropped and asked again, as for a rejection; the rest is dropped. A
/// pull's parts are all accepted, a mark or `rest` rejecting any refused:
/// each comment is filed, a reviewer's reply to a thread you closed
/// reopening it. Its change accepted, chained, wholly or in part, each
/// answer made on the stack before it, running or waiting for your review,
/// goes as `marks` mark it, kept, restarted or stopped (`answer_mark`), and
/// none may be left unmarked: `rest` marks none of them. What came of it,
/// said.
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
    // A pull's: every comment it pulled is filed, none rejected; its change
    // of the stack, by its operations, as any. With no change, nothing of it
    // is rejected.
    let pull = pulled(&r);
    if pull && (rejects(&h, marks) || (rest == Some(false) && h.stack.is_none())) {
        return err(FILED);
    }
    // The change of the stack, by its operations, each as marked or as
    // `rest` says; with none to mark, nothing stored of what each needs, or
    // its stack gone, as a whole.
    let ops = crate::operations::of_review(db, &r, &h).filter(|o| !o.whole());
    let mut all = BTreeMap::new();
    for p in h.parts() {
        if matches!(p, Part::Reply(k) if v.taken.contains(&k)) {
            continue;
        }
        let unmarked = || err("not every part is marked yet: mark each, or accept or reject all that remain");
        if let (Part::Stack, Some(ops)) = (p, &ops) {
            for o in ops.marked() {
                let Some(m) = ops.mark(marks, o).or(rest) else { return unmarked() };
                all.insert(o.key.clone(), m);
            }
            continue;
        }
        if pull && p != Part::Stack {
            all.insert(p.key(), true);
            continue;
        }
        let Some(m) = marks.get(&p.key()).copied().or(rest) else { return unmarked() };
        all.insert(p.key(), m);
    }
    let taken = h.stack.as_ref().map(|_| match &ops {
        Some(ops) => match ops.counts(&all) {
            (_, 0, _) => Accepted::Whole,
            (0, _, _) => Accepted::Nothing,
            _ => Accepted::Part,
        },
        None if all.get(&Part::Stack.key()) == Some(&true) => Accepted::Whole,
        None => Accepted::Nothing,
    });
    let change = h.stack.as_ref().zip(taken);
    // Every operation rejected, the agent's replies and comments with them;
    // a pull's comments are the reviewers', filed all the same, and an
    // experiment's say what it found, as they are marked.
    if let Some((_, Accepted::Nothing)) = change
        && !pull
        && !crate::ask::experiment(&r)
    {
        for (key, m) in all.iter_mut() {
            if !crate::operations::is_key(key) && key != "stack" {
                *m = false;
            }
        }
    }
    let accepted = matches!(change, Some((_, Accepted::Whole | Accepted::Part)));
    let from = made_from(&r, &h);
    if accepted && db.head(&r.stack).map(|(rev, _)| rev) != from {
        return err("the stack changed since this change was made from it: it can only be rejected");
    }
    // Accepted in part: the stack the operations accepted leave, which must
    // apply, before anything is done. The editor checks it first, away from
    // its screen, and so does `stacked review` before it takes the lock
    // (`closing_check`): here, what that check found is only read again.
    let part = match (change, &ops) {
        (Some((c, Accepted::Part)), Some(ops)) => {
            let stands = ops.standing(&all);
            let (left, base) = ops.leave(&stands);
            let s = db.stack(&r.stack)?;
            let made = Stack {
                patches: left.into_iter().map(|p| p.id).collect(),
                base,
                ..s.clone()
            };
            if let Err(e) = db.verify(&made) {
                return err(format!("the stack the operations you accepted leave does not apply: {}", e.0));
            }
            Some((made, format!("{}; rejected: {}", c.op, ops.rejected_said(&all))))
        }
        _ => None,
    };
    // A patch that holds a conflict a pull marked does not go in.
    let taking: Option<&Vec<String>> = match (change, &part) {
        (Some((c, Accepted::Whole)), _) => Some(&c.patches),
        (Some((_, Accepted::Part)), Some((s, _))) => Some(&s.patches),
        _ => None,
    };
    if let Some(why) = taking.and_then(|p| marked_said(db, p)) {
        return err(format!("{why}; nothing changed"));
    }
    // Its change accepted, chained, wholly or in part: the answers made on
    // the stack before it, running or waiting for your review, each go as
    // marked, kept, restarted or stopped. None goes unmarked: nothing is
    // done before each is.
    let left = match accepted {
        true => crate::ask::answers_left(db, &r.stack, id),
        false => vec![],
    };
    let unmarked: Vec<&Run> = left.iter().filter(|a| answer_mark(marks, &a.id).is_none()).collect();
    if !unmarked.is_empty() {
        return err(unmarked_said(db, &unmarked));
    }
    // The replies where their comments are, then the change, the comments
    // on its lines following them, then the comments opened. A reviewer's
    // reply to a thread you closed reopens it. A build and test's, asked as
    // a follow-up, read: after the reply before it in its thread, if that
    // one goes in, and the change is not rejected.
    let mut reopened = 0;
    let mut filed: HashMap<(String, String), bool> = HashMap::new();
    let rejected = matches!(change, Some((_, Accepted::Nothing))) && !crate::ask::experiment(&r);
    for (k, rp) in h.replies.iter().enumerate() {
        let at = (rp.change.clone(), rp.id.clone());
        let goes = match rp.read {
            true => !rejected && filed.get(&at).copied().unwrap_or(true),
            false => all.get(&Part::Reply(k).key()) == Some(&true) || v.taken.contains(&k),
        };
        filed.insert(at, goes);
        if goes && !v.taken.contains(&k) && apply_reply(db, &r.stack, rp)? {
            reopened += 1;
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
    let held_before = held_changes(db, &r.stack);
    // Accepted whole, the stack the change makes; in part, the stack its
    // operations accepted leave, the history naming those rejected.
    let made = match (change, part) {
        (Some((c, Accepted::Whole)), _) => Some((c.patches.clone(), c.base.clone(), c.op.clone())),
        (Some((_, Accepted::Part)), Some((s, op))) => Some((s.patches, s.base, op)),
        _ => None,
    };
    if let Some((patches, base, op)) = made {
        let s = db.stack(&r.stack)?.clone();
        let olds: Vec<Patch> = s.patches.iter().map(|p| db.patch(p).clone()).collect();
        let base = base.or(s.base.clone());
        // Of the patches that should not change, those you typed accept for
        // before $EDITOR opened on this edit by hand, and those a pull
        // brings to what Phabricator has of them, as a pull takes what
        // Phabricator has, change with no one asked again. The write is
        // refused for any other, the editor and the command line asking.
        db.confirm(v.confirmed.iter().cloned());
        if pull {
            let own = crate::protected::from_phabricator(db, &s.patches, &patches);
            db.confirm(own);
        }
        rev = Some(db.set_stack_on(&r.stack, patches.clone(), base, s.repo.clone(), &op)?);
        crate::ask::follow_lines(db, &olds, &patches)?;
    }
    // A comment the ask was about, or a follow-up, kept open, on a patch the
    // stack held neither before nor holds now (one that work it went on
    // from added, the stack never taking it), stays in sight: on the stack's
    // own comments, moved there from where it was.
    let held_after = held_changes(db, &r.stack);
    let mut moved = 0;
    let mut kept = asking_of(&r);
    let followed: Vec<(String, String)> = h.followed().into_iter().filter(|at| !kept.contains(at)).collect();
    kept.extend(followed);
    for (c, i) in kept {
        if held_before.contains(&c) || held_after.contains(&c) || c == crate::db::stack_change(&r.stack) {
            continue;
        }
        if let Some(n) = db.note(&c, &i).cloned() {
            crate::ask::to_stack(db, &r.stack, &n)?;
            moved += 1;
        }
    }
    // What went on from it, guessing it would be accepted: right, or wrong.
    // Its change accepted, the answers made on the stack before it go as
    // marked: kept, asked again on the stack as it is now, or stopped.
    // Accepted in part, it is not the stack what went on from it went on
    // from: that is dropped and asked again, as for a rejection.
    let mut again = Vec::new();
    let wrong = |db: &mut Db, why: &str, again: &mut Vec<String>| -> Result<()> {
        let n = crate::ask::guessed_wrong(db, id, why)?;
        if n > 0 {
            again.push(count(n, "edit that went on from it asked again", "edits that went on from it asked again"));
        }
        Ok(())
    };
    match (change, rev) {
        (Some((_, Accepted::Whole)), Some(at)) => {
            crate::ask::guessed_right(db, id, at)?;
            again.extend(crate::ask::leave_answers(db, &left, marks)?);
        }
        (Some((_, Accepted::Part)), _) => {
            wrong(db, "accepted in part", &mut again)?;
            again.extend(crate::ask::leave_answers(db, &left, marks)?);
        }
        (Some((_, Accepted::Nothing)), _) => wrong(db, "rejected", &mut again)?,
        _ => {}
    }
    // A pull's new threads, on a patch the stack holds now: one on a patch it
    // added that the stack does not take is pulled again with it, next time.
    for (k, n) in h.opened.iter().enumerate() {
        if all.get(&Part::Opened(k).key()) == Some(&true) && (!pull || held_after.contains(&n.change) || n.on_stack()) {
            db.set_note(Note { time: now(), ..n.clone() })?;
        }
    }
    // Each operation as it went; the change as a whole too, unless it went
    // in part, as a stacked of before reads it.
    v.marks.retain(|k, _| !crate::operations::is_key(k) && k != "stack");
    match change.map(|(_, t)| t) {
        Some(Accepted::Whole) => v.marks.insert(Part::Stack.key(), true),
        Some(Accepted::Nothing) => v.marks.insert(Part::Stack.key(), false),
        _ => None,
    };
    v.marks.extend(all);
    v.marks.extend(marks.iter().filter(|(k, _)| k.starts_with("asking ")).map(|(k, m)| (k.clone(), *m)));
    // The answers made on the stack before it, as they went: those of the
    // marks saved that went nowhere left out.
    v.marks.retain(|k, _| !is_answer_key(k));
    v.marks.extend(left.iter().filter_map(|a| Some((answer_key(answer_mark(marks, &a.id)?, &a.id), true))));
    // The tools the agent could not use as you marked them: what you allowed
    // from now on saved, accepted or rejected; what you allowed for this
    // work ends here.
    v.marks.retain(|k, _| !crate::refusals::is_key(k));
    v.marks.extend(marks.iter().filter(|(k, _)| crate::refusals::is_key(k)).map(|(k, m)| (k.clone(), *m)));
    let settled = crate::refusals::settle(db, &r, &mut v.marks, false);
    // A follow-up asked from the review that is still going: stopped, what
    // it would bring back thrown away unseen, its comments saying so, once
    // what the review takes is in them.
    let stopped = crate::ask::stop_follow_ups(db, id, crate::ask::FOLLOW_CLOSED)?;
    v.closed = true;
    db.set_review(id, v.clone())?;
    // Nothing brought back: no part to say, but the tools it could not use.
    // What the linter reported and fixed nothing of is only read.
    let taken_said = change.and_then(|(_, t)| (t == Accepted::Part).then(|| ops.as_ref().map(|o| part_said(o, &v.marks)))).flatten();
    let mut said = match (h.is_empty(), said_of(&h, &v.marks, taken_said)) {
        (true, s) if s.is_empty() => NOTHING_BACK.to_string(),
        (true, s) => format!("{NOTHING_BACK}; {s}"),
        (false, s) if s.is_empty() && h.lint.is_some() => "what the linter reported, read".to_string(),
        (false, s) => s,
    };
    if reopened > 0 {
        said += &format!("; {}", count(reopened, "comment reopened", "comments reopened"));
    }
    if let Some(i) = crate::ask::info(&r) {
        said += &settled.said(i.way);
    }
    if closed > 0 {
        said += &format!("; {}", count(closed, "asking comment closed", "asking comments closed"));
    }
    if moved > 0 {
        let what = count(moved, "asking comment", "asking comments");
        said += &format!("; {what} moved to the stack's comments, as the stack does not hold {}", if moved == 1 { "its patch" } else { "their patches" });
    }
    for a in again {
        said += &format!("; {a}");
    }
    if stopped > 0 {
        said += &format!("; {}", count(stopped, "follow-up stopped", "follow-ups stopped"));
    }
    db.add_run(Run {
        summary: format!("reviewed: {said}"),
        rev,
        ..r
    })?;
    Ok(said)
}

/// Accepted in part, operation by operation, as closing run `id`'s review
/// with `marks`, those not marked as `rest` says, would accept its change:
/// its operations, and those that would stand. None for any other close,
/// which checks no stack of its own: accepted whole, the stack the change
/// makes was checked as the work landed; rejected, the stack stays as it
/// is; marked as a whole, its change is one. A pull's change as any.
pub fn closing_stands(db: &Db, id: &str, marks: &BTreeMap<String, bool>, rest: Option<bool>) -> Option<(crate::operations::Ops, BTreeSet<String>)> {
    let r = db.run(id)?;
    let ops = crate::operations::of_review(db, r, r.held.as_ref()?).filter(|o| !o.whole())?;
    let mut all = BTreeMap::new();
    for o in ops.marked() {
        all.insert(o.key.clone(), ops.mark(marks, o).or(rest)?);
    }
    match ops.counts(&all) {
        (yes, no, _) if yes > 0 && no > 0 => {
            let stands = ops.standing(&all);
            Some((ops, stands))
        }
        _ => None,
    }
}

/// What closing run `id`'s review with `marks`, those not marked as `rest`
/// says, checks first, before any lock is taken: accepted in part, whether
/// the stack the operations accepted leave applies (`Ops::closing`); if
/// not, the refusal, saying why (`refused_closing`). None when it applies,
/// or when the close checks no stack of its own (`closing_stands`).
pub fn closing_check(db: &Db, id: &str, marks: &BTreeMap<String, bool>, rest: Option<bool>) -> Result<Option<String>> {
    let Some((ops, stands)) = closing_stands(db, id, marks, rest) else { return Ok(None) };
    Ok(ops.closing(db, &stands)?.map(|why| refused_closing(&why)))
}

/// What closing refuses with, the stack the operations accepted leave not
/// applying: why (`Ops::closing`), and that nothing changed.
pub fn refused_closing(why: &str) -> String {
    format!("the stack the operations you accepted leave does not apply: {why}; nothing changed")
}

/// Why what each operation of a change needs, worked out before the work
/// was filed, is not stored with it: nothing was, the work not read then.
const NOT_READ: &str = "the work could not be read before it was filed";

/// Why what each operation of a change needs, worked out before the work
/// was filed, is not stored with it: it is for other stacks.
const NOT_THESE: &str = "the stack it went on from changed as it landed";

/// What is stored of what each operation of the change `h` of run `r`
/// needs, as the work is filed, under the lock: `worked`, worked out before
/// with no lock held, if it is for that change from that stack; else why
/// not, so that the change is marked as a whole. None for work that changes
/// nothing of the stack. Nothing is checked here.
pub fn needs_filed(db: &Db, r: &Run, h: &Held, worked: Option<crate::operations::Needs>) -> Option<crate::operations::Needs> {
    let of = crate::operations::of_landed(db, r, h)?.digest();
    let failed = |why: &str| crate::operations::Needs {
        of: of.clone(),
        failed: Some(why.into()),
        alone: vec![],
    };
    Some(match worked {
        Some(n) if n.of == of => n,
        Some(_) => failed(NOT_THESE),
        None => failed(NOT_READ),
    })
}

/// What each operation of the change run `id` holds needs, worked out and
/// stored with it (`Held::needs`), unless what is stored is for that change
/// already: as an edit by hand lands, once it is filed, before its review
/// opens. Worked out with no lock held, as its checks read the repository
/// and may take long on a big one; stored under the lock, if the run still
/// holds that change, waiting for your review. Why it could not be worked
/// out is stored as well: the change is marked as a whole then.
pub fn work_out(path: &Path, id: &str) -> Result<()> {
    let db = Db::load(path)?;
    let Some(r) = db.run(id) else { return Ok(()) };
    let Some(h) = r.held.as_ref() else { return Ok(()) };
    let Some(ops) = crate::operations::of_landed(&db, r, h) else { return Ok(()) };
    if h.needs.as_ref().is_some_and(|n| n.of == ops.digest()) {
        return Ok(());
    }
    let needs = ops.needs(&db);
    Db::edit(path, |db| {
        let Some(r) = db.run(id).cloned() else { return Ok(()) };
        let Some(mut h) = r.held.clone() else { return Ok(()) };
        let now = crate::operations::of_landed(db, &r, &h).map(|o| o.digest());
        if !waiting(db, &r) || now.as_deref() != Some(needs.of.as_str()) || h.needs.as_ref() == Some(&needs) {
            return Ok(());
        }
        h.needs = Some(needs);
        db.add_run(Run { held: Some(h), ..r }).map(|_| ())
    })
}

/// The tasks of the try push work `h` that `marks` accepts, those it does
/// not mark as `rest` says, in their order.
#[cfg(feature = "try-push")]
pub fn accepted_tasks(h: &Held, marks: &BTreeMap<String, bool>, rest: Option<bool>) -> Vec<crate::try_push::Task> {
    h.tasks.iter().enumerate().filter(|(k, _)| marks.get(&Part::Task(*k).key()).copied().or(rest) == Some(true)).map(|(_, t)| t.clone()).collect()
}

/// How much of a change of the stack a review takes: every operation,
/// some of them, or none.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Accepted {
    Whole,
    Part,
    Nothing,
}

/// A change of the stack accepted in part, by its operations as `marks`
/// mark them, said as what a review made of it: `its change accepted in
/// part, 3 operations accepted and 1 rejected (remove 2)`.
fn part_said(ops: &crate::operations::Ops, marks: &BTreeMap<String, bool>) -> String {
    let (yes, no, _) = ops.counts(marks);
    let accepted = count(yes, "operation", "operations");
    format!("its change accepted in part, {accepted} accepted and {no} rejected ({})", ops.rejected_said(marks))
}

/// What a review made of the work held, by its marks: `its change
/// accepted; 2 replies accepted, 1 rejected`.
pub fn said(h: &Held, marks: &BTreeMap<String, bool>) -> String {
    said_of(h, marks, None)
}

/// As `said`, its change accepted in part said `part`, as `part_said`
/// says it.
fn said_of(h: &Held, marks: &BTreeMap<String, bool>, part: Option<String>) -> String {
    let mark = |p: Part| marks.get(&p.key()).copied();
    let mut out = Vec::new();
    if h.stack.is_some() {
        out.push(match (part, mark(Part::Stack)) {
            (Some(part), _) => part,
            (None, Some(true)) => "its change accepted".to_string(),
            (None, _) => "its change rejected".to_string(),
        });
    }
    // Not a build and test's, asked as a follow-up, which was to read; each
    // as what it is, a reply, an edit, or a comment read again with its
    // suggested edit.
    let replies = |of: fn(&Reply) -> bool| (0..h.replies.len()).filter(|&k| !h.replies[k].read && of(&h.replies[k])).map(Part::Reply).collect::<Vec<_>>();
    #[cfg(feature = "try-push")]
    let tasks: Vec<Part> = (0..h.tasks.len()).map(Part::Task).collect();
    #[cfg(not(feature = "try-push"))]
    let tasks: Vec<Part> = Vec::new();
    for (one, many, parts) in [
        ("task", "tasks", tasks),
        ("opened comment", "opened comments", (0..h.opened.len()).map(Part::Opened).collect::<Vec<_>>()),
        ("reply", "replies", replies(|rp| rp.was.is_none())),
        ("edited comment", "edited comments", replies(Reply::is_edit)),
        ("suggested edit", "suggested edits", replies(Reply::is_anew)),
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
    if h.found() {
        out.push(format!("{FOUND}, read"));
    }
    if let Some(t) = tools_said(h, marks) {
        out.push(t);
    }
    out.join("; ")
}

/// What a review made of the tools the agent could not use, by its marks:
/// `2 tools allowed from now on, 1 refused, 1 noted`. None without any.
pub fn tools_said(h: &Held, marks: &BTreeMap<String, bool>) -> Option<String> {
    use crate::refusals::{Allow, state};
    let noted = h.tools.iter().filter(|t| !t.offered()).count();
    let of = |a: Allow| h.tools.iter().enumerate().filter(|(k, t)| t.offered() && state(marks, *k) == a).count();
    let parts: Vec<(usize, &str)> = [(of(Allow::Always), "allowed from now on"), (of(Allow::Work), "allowed for this work"), (of(Allow::Refused), "refused"), (noted, "noted")]
        .into_iter()
        .filter(|(n, _)| *n > 0)
        .collect();
    let ((n, first), rest) = parts.split_first()?;
    let mut out = format!("{} {first}", count(*n, "tool", "tools"));
    for (n, what) in rest {
        out += &format!(", {n} {what}");
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::thread::fenced;

    #[test]
    fn an_edit_on_phabricator_is_the_reviewer_s_latest_turn_and_the_turn_it_edits_is_redacted() {
        let edit = |was: &str, now: &str| Reply {
            text: now.into(),
            reviewer: Some("ann".into()),
            was: Some(was.into()),
            ..Default::default()
        };
        let ann = |text: &str| fenced("reviewer ann", text);
        let turns = |text: &str| all_turns(text).iter().enumerate().map(|(i, t)| (t.name(i), t.body.clone())).collect::<Vec<_>>();
        let named = |pairs: &[(&str, &str)]| pairs.iter().map(|(n, b)| (n.to_string(), b.to_string())).collect::<Vec<_>>();
        // Nothing after her comment: it stays, redacted, her edit after it
        // quoting it.
        let once = edit("Hi Da", "Hi Damien").after(&ann("Hi Da"));
        assert_eq!(once, "```reviewer ann redacted\nHi Da\n```\n```reviewer ann edit Hi Da\nHi Damien\n```");
        // Edited again: the latest version's turn redacted in its turn, the
        // new edit quoting it.
        let twice = edit("Hi Damien", "Hi Damien!").after(&once);
        assert_eq!(
            turns(&twice),
            named(&[
                ("ann's comment (redacted)", "Hi Da"),
                ("ann's edit of the comment that said \"Hi Da\" (redacted)", "Hi Damien"),
                ("ann's edit of the comment that said \"Hi Damien\"", "Hi Damien!"),
            ])
        );
        // Your reply after it: it stays after what it answered, the edit
        // after it.
        let text = [ann("Hi Da"), "Hi ann! My name is Damien".to_string()].join("\n");
        let replied = edit("Hi Da", "Hi Damien!").after(&text);
        assert_eq!(
            turns(&replied),
            named(&[
                ("ann's comment (redacted)", "Hi Da"),
                ("your reply", "Hi ann! My name is Damien"),
                ("ann's edit of the comment that said \"Hi Da\"", "Hi Damien!"),
            ])
        );
        // Phabricator's lines end with a carriage return, a turn's do not;
        // bob's turn saying the same is not hers.
        let text = [ann("Fine.\nBut why?"), "Because.".to_string(), fenced("reviewer bob", "Fine.\nBut why?")].join("\n");
        assert_eq!(
            edit("Fine.\r\nBut why?", "Fine, but why?").after(&text),
            "```reviewer ann redacted\nFine.\nBut why?\n```\nBecause.\n```reviewer bob\nFine.\nBut why?\n```\n```reviewer ann edit Fine.\nFine, but why?\n```"
        );
        // Its turn gone, rewound away say: the edit is ann's last turn,
        // nothing redacted.
        assert_eq!(edit("Never said.", "Said now.").after(&text), format!("{text}\n```reviewer ann\nSaid now.\n```"));
        // Said as what it is.
        let reply = Reply {
            reviewer: Some("ann".into()),
            ..Default::default()
        };
        assert_eq!((edit("a", "b").said(), reply.said(), Reply::default().said()), ("ann's edit".into(), "ann's reply".into(), "the agent's reply".into()));
        let h = Held {
            replies: vec![edit("a", "b"), reply],
            ..Default::default()
        };
        assert_eq!(h.said(), "1 reply, 1 edited comment");
        let marks = [("reply 0".to_string(), true), ("reply 1".to_string(), false)].into();
        assert_eq!(said(&h, &marks), "1 reply rejected; 1 edited comment accepted");
    }

    #[test]
    fn a_comment_read_again_with_its_suggested_edit_takes_the_place_of_its_turn_said_for_what_it_is() {
        // ann's comment filed without the suggested edit Phabricator gives
        // with it, read again with it: no one edited it, though it takes the
        // place of her turn as an edit does.
        let anew = |was: &str, now: &str| Reply {
            text: now.into(),
            reviewer: Some("ann".into()),
            was: Some(was.into()),
            anew: true,
            ..Default::default()
        };
        let now = "Shout less.\n\nann suggests this edit of line 2:\n```diff\n-LINE 2\n+line 2\n```";
        let text = anew("Shout less.", now).after(&fenced("reviewer ann", "Shout less."));
        assert_eq!(text, format!("```reviewer ann redacted\nShout less.\n```\n{}", fenced("reviewer ann anew Shout less.", now)));
        let names: Vec<String> = all_turns(&text).iter().enumerate().map(|(i, t)| t.name(i)).collect();
        assert_eq!(names, ["ann's comment (redacted)", "ann's comment that said \"Shout less.\", with its suggested edit"]);
        // Said so wherever it is named, and counted apart from edits.
        let edit = Reply { anew: false, ..anew("a", "b") };
        let reply = Reply {
            reviewer: Some("ann".into()),
            ..Default::default()
        };
        assert_eq!((anew("a", "b").said(), edit.said()), ("ann's suggested edit".into(), "ann's edit".into()));
        let h = Held {
            replies: vec![anew("a", "b"), edit, reply, anew("c", "d")],
            ..Default::default()
        };
        assert_eq!(h.said(), "1 reply, 1 edited comment, 2 suggested edits");
        let marks: BTreeMap<String, bool> = (0..4).map(|k| (format!("reply {k}"), true)).collect();
        assert_eq!(said(&h, &marks), "1 reply accepted; 1 edited comment accepted; 2 suggested edits accepted");
    }

    /// A database at `path` holding ann's thread `phab:a1` on change `c`,
    /// your answer in it, closed since; and pull `run` holding `replies`,
    /// each kept with that thread as it was when closed. The thread as it
    /// was.
    fn closed_thread(path: &std::path::Path, run: &str, replies: Vec<Reply>) -> Note {
        let open = Note {
            change: "c".into(),
            id: "phab:a1".into(),
            line: "+x".into(),
            text: format!("{}\nBecause.", fenced("reviewer ann", "Why?")),
            by: Some("ann".into()),
            remote: vec!["a1".into()],
            ..Default::default()
        };
        Db::create(path).unwrap();
        Db::edit(path, |db| {
            db.set_note(open.clone())?;
            db.set_note(Note { text: String::new(), ..open.clone() })?;
            let kept = Closed {
                rev: db.note_rev("c", "phab:a1").unwrap(),
                note: db.note_history("c", "phab:a1")[0].1.clone(),
            };
            let replies = replies.into_iter().map(|rp| Reply { closed: Some(kept.clone()), ..rp }).collect();
            pull_run(db, run, replies)?;
            Ok(kept.note)
        })
        .unwrap()
    }

    #[test]
    fn what_a_follow_up_brings_back_comes_into_the_review_of_the_work_it_follows_up() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("db.jsonl");
        Db::create(&path).unwrap();
        let reply = |text: &str| Reply {
            change: "c".into(),
            id: "n".into(),
            text: text.into(),
            tag: "answer".into(),
            ..Default::default()
        };
        let tool = |rule: &str| crate::refusals::Refusal {
            rules: vec![rule.into()],
            ..Default::default()
        };
        let asked = |way: &str| -> crate::ask::Info {
            let notes = serde_json::json!([{"label": "c1", "change": "c", "id": "n"}]);
            serde_json::from_value(serde_json::json!({"way": way, "scope": "stack", "from": 0, "notes": notes, "on": "w", "follow": true})).unwrap()
        };
        let follow = |id: &str| Run { id: id.into(), ..Default::default() };
        let work = |db: &Db| db.run("w").unwrap().held.clone().unwrap();
        Db::edit(&path, |db| {
            // The work: a reply, which you answered in its review, and a
            // tool the agent could not use.
            let held = Held {
                at: now(),
                replies: vec![Reply { answer: Some("Why?".into()), ..reply("Seen.") }],
                tools: vec![tool("Bash(rg *)")],
                ..Default::default()
            };
            db.add_run(Run { id: "w".into(), what: "answer".into(), stack: "s".into(), state: crate::db::RunState::Done, held: Some(held), ..Default::default() })?;
            // An answer's: its reply after the work's, the tools it could
            // not use but those the work holds.
            let brought = Held { replies: vec![reply("Because.")], tools: vec![tool("Bash(rg *)"), tool("WebFetch")], ..Default::default() };
            let said = come_in(db, &follow("f1"), &asked("answer"), &brought, true)?;
            assert_eq!(said.as_deref(), Some("its work came into the review of the work it follows up: 1 reply, 1 tool the agent could not use"));
            let h = work(db);
            assert_eq!(h.replies.iter().map(|rp| rp.text.as_str()).collect::<Vec<_>>(), ["Seen.", "Because."]);
            assert_eq!(h.tools.iter().map(crate::refusals::Refusal::rule).collect::<Vec<_>>(), ["Bash(rg *)", "WebFetch"]);
            assert_eq!((h.follow_ups.len(), h.follow_ups[0].run.as_str(), h.follow_ups[0].asked.clone()), (1, "f1", vec![("c".to_string(), "n".to_string())]));
            // Your answer went with it: a later reply follows, and answered
            // it; the thread as it would be has each in turn.
            assert_eq!((h.last_reply(0), h.last_reply(1), answered(&h, &HashSet::new())), (false, true, vec![]));
            let thread: Vec<String> = all_turns(&h.thread_but("c", "n", "Loud?", &[])).into_iter().map(|t| t.body).collect();
            assert_eq!(thread, ["Loud?", "Seen.", "Why?", "Because."]);
            // A build and test's: its reply to read, not a part, and what it
            // found of each patch with the follow-up.
            let found = Note {
                change: "p".into(),
                id: "agent:f2:test:1".into(),
                text: fenced("agent test", "Builds."),
                ..Default::default()
            };
            let brought = Held { replies: vec![reply("All pass.")], opened: vec![found.clone()], ..Default::default() };
            let said = come_in(db, &follow("f2"), &asked("test"), &brought, true)?;
            assert_eq!(said.as_deref(), Some(format!("its work came into the review of the work it follows up: {FOUND}").as_str()));
            let h = work(db);
            assert!(h.replies[2].read && h.found() && h.opened.is_empty());
            assert_eq!((h.follow_ups[1].found.clone(), h.parts()), (vec![found], vec![Part::Reply(0), Part::Reply(1)]));
            assert_eq!(h.said(), format!("2 replies, {FOUND}, 2 tools the agent could not use"));
            // Failed with nothing that comes in: nothing comes in; nor once
            // the review of the work is closed.
            assert_eq!(come_in(db, &follow("f3"), &asked("answer"), &Held::default(), false)?, None);
            db.set_review("w", Review { closed: true, ..Default::default() })?;
            assert_eq!(come_in(db, &follow("f4"), &asked("answer"), &brought, true)?, None);
            assert_eq!(work(db).follow_ups.len(), 2);
            Ok(())
        })
        .unwrap();
    }

    /// Pull `run` of stack `s`, holding `replies`, waiting for review.
    fn pull_run(db: &mut Db, run: &str, replies: Vec<Reply>) -> Result<()> {
        db.add_run(Run {
            id: run.into(),
            what: "pull".into(),
            stack: "s".into(),
            state: crate::db::RunState::Done,
            held: Some(Held { at: now(), replies, ..Default::default() }),
            ..Default::default()
        })
        .map(|_| ())
    }

    /// ann's reply `text`, Phabricator's comment `remote`, to thread
    /// `phab:a1` of change `c`.
    fn ann_says(text: &str, remote: &str) -> Reply {
        Reply {
            change: "c".into(),
            id: "phab:a1".into(),
            text: text.into(),
            reviewer: Some("ann".into()),
            remote: Some(remote.into()),
            ..Default::default()
        }
    }

    #[test]
    fn a_reviewer_s_reply_to_a_comment_you_closed_reopens_it_as_it_was_once_accepted() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("db.jsonl");
        let open = closed_thread(&path, "pull-1", vec![ann_says("Still?", "a2")]);
        let db = Db::load(&path).unwrap();
        let rp = &db.run("pull-1").unwrap().held.as_ref().unwrap().replies[0];
        // Where it goes: into the thread as it was when closed, reopened.
        assert_eq!(goes(&db, "s", rp), Goes::Reopens(Some(&open)));
        // The agent's to it would go nowhere: none is closed while the agent
        // has it, nor while its reply waits.
        let agent_s = Reply { reviewer: None, ..rp.clone() };
        assert_eq!(goes(&db, "s", &agent_s), Goes::Nowhere);
        drop(db);
        // A pull's parts are not rejected, a mark or the rest.
        let reject = [("reply 0".to_string(), false)].into();
        assert_eq!(Db::edit(&path, |db| close(db, "pull-1", &reject, None)).unwrap_err().0, FILED);
        assert_eq!(Db::edit(&path, |db| close(db, "pull-1", &Default::default(), Some(false))).unwrap_err().0, FILED);
        assert_eq!(Db::edit(&path, |db| save(db, "pull-1", &reject)).unwrap_err().0, FILED);
        // Closed, every part accepted: the thread reopened as it was, not
        // asked again, then her reply its last turn, its comment's id kept.
        let said = Db::edit(&path, |db| close(db, "pull-1", &Default::default(), None)).unwrap();
        assert_eq!(said, "1 reply accepted; 1 comment reopened");
        let db = Db::load(&path).unwrap();
        let n = db.note("c", "phab:a1").unwrap();
        assert_eq!(n.text, format!("{}\nBecause.\n{}", fenced("reviewer ann", "Why?"), fenced("reviewer ann", "Still?")));
        assert_eq!((n.remote.clone(), n.queued.clone(), n.line.as_str()), (vec!["a1".to_string(), "a2".to_string()], None, "+x"));
        // Reopened, then replied to: as `Reopen it` reopens it, then as her
        // reply is filed in a thread open.
        let history = db.note_history("c", "phab:a1");
        let last: Vec<&str> = history.iter().rev().take(2).map(|(_, n)| n.text.as_str()).collect();
        assert_eq!(last, [n.text.as_str(), open.text.as_str()]);
    }

    #[test]
    fn a_reply_taken_at_once_or_sent_back_with_the_work_reopens_the_comment_too() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("db.jsonl");
        closed_thread(&path, "pull-1", vec![ann_says("Still?", "a2")]);
        // Taken at once: reopened, the review closed with it.
        Db::edit(&path, |db| take(db, "pull-1", 0)).unwrap();
        let db = Db::load(&path).unwrap();
        assert!(db.note("c", "phab:a1").unwrap().text.ends_with(&fenced("reviewer ann", "Still?")));
        assert!(db.review("pull-1").is_some_and(|v| v.closed));
        drop(db);
        // Sent back with the agent's work, its replies filed: one of a
        // reviewer's to a thread you closed reopens it there too, the
        // agent's to the comment it was asked about in its thread.
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("db.jsonl");
        closed_thread(&path, "pull-1", vec![]);
        Db::edit(&path, |db| {
            let kept = Closed {
                rev: db.note_rev("c", "phab:a1").unwrap(),
                note: db.note_history("c", "phab:a1")[0].1.clone(),
            };
            db.set_note(Note {
                change: "c".into(),
                id: "+y".into(),
                line: "+y".into(),
                text: "Why y?".into(),
                ..Default::default()
            })?;
            let agent_s = Reply {
                change: "c".into(),
                id: "+y".into(),
                text: "Because of x.".into(),
                tag: "answer".into(),
                answer: Some("And z?".into()),
                ..Default::default()
            };
            db.add_run(Run {
                id: "work-1".into(),
                what: "answer".into(),
                stack: "s".into(),
                state: crate::db::RunState::Done,
                ask: Some(serde_json::json!({"way": "answer", "scope": "stack", "from": 1, "notes": [{"label": "1", "change": "c", "id": "+y"}]})),
                held: Some(Held {
                    at: now(),
                    replies: vec![agent_s, Reply { closed: Some(kept), ..ann_says("Still?", "a2") }],
                    ..Default::default()
                }),
                ..Default::default()
            })
            .map(|_| ())
        })
        .unwrap();
        let round = Db::edit(&path, |db| send_back(db, "work-1", &Default::default())).unwrap();
        assert_eq!(round.notes, [("c".to_string(), "+y".to_string())]);
        let db = Db::load(&path).unwrap();
        assert!(db.note("c", "phab:a1").unwrap().text.ends_with(&fenced("reviewer ann", "Still?")));
        assert!(db.note("c", "+y").unwrap().text.ends_with("```agent answer\nBecause of x.\n```\nAnd z?"));
    }

    #[test]
    fn a_comment_written_since_the_pull_is_read_again_and_one_open_elsewhere_is_not_reopened() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("db.jsonl");
        let open = closed_thread(&path, "pull-1", vec![ann_says("Still?", "a2")]);
        // Reopened since, written to, closed again: as its history has it
        // last open, your reply in.
        Db::edit(&path, |db| {
            let more = Note { text: format!("{}\nAnd then?", open.text), ..open.clone() };
            db.set_note(more.clone())?;
            db.set_note(Note { text: String::new(), ..more })
        })
        .unwrap();
        assert_eq!(Db::edit(&path, |db| close(db, "pull-1", &Default::default(), None)).unwrap(), "1 reply accepted; 1 comment reopened");
        let n = Db::load(&path).unwrap().note("c", "phab:a1").unwrap().clone();
        assert_eq!(n.text, format!("{}\nAnd then?\n{}", open.text, fenced("reviewer ann", "Still?")));
        // Open again since: the reply goes in it, nothing reopened.
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("db.jsonl");
        let open = closed_thread(&path, "pull-1", vec![ann_says("Still?", "a2")]);
        Db::edit(&path, |db| db.set_note(open.clone())).unwrap();
        assert_eq!(Db::edit(&path, |db| close(db, "pull-1", &Default::default(), None)).unwrap(), "1 reply accepted");
        assert!(Db::load(&path).unwrap().note("c", "phab:a1").unwrap().text.ends_with(&fenced("reviewer ann", "Still?")));
        // Moved to the stack's comments since, open there: the reply goes
        // there, the thread not reopened on its patch.
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("db.jsonl");
        let open = closed_thread(&path, "pull-1", vec![ann_says("Still?", "a2")]);
        Db::edit(&path, |db| {
            db.set_note(Note {
                change: crate::db::stack_change("s"),
                id: "moved:x".into(),
                line: String::new(),
                moved: Some(crate::db::Moved { change: "c".into(), id: "phab:a1".into(), ..Default::default() }),
                ..open.clone()
            })
        })
        .unwrap();
        assert_eq!(Db::edit(&path, |db| close(db, "pull-1", &Default::default(), None)).unwrap(), "1 reply accepted");
        let db = Db::load(&path).unwrap();
        assert!(db.note("c", "phab:a1").is_none(), "not reopened where it was");
        let moved = db.note(&crate::db::stack_change("s"), "moved:x").unwrap();
        assert!(moved.text.ends_with(&fenced("reviewer ann", "Still?")) && moved.remote.contains(&"a2".to_string()), "{moved:?}");
    }

    #[test]
    fn read_from_a_checkpoint_the_review_shows_the_thread_closed_as_the_pull_kept_it() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("db.jsonl");
        let open = closed_thread(&path, "pull-1", vec![ann_says("Still?", "a2")]);
        // A head moved: a checkpoint, which reads no comment closed.
        let stack = crate::db::Stack {
            name: "s".into(),
            patches: vec![],
            base: None,
            repo: None,
            parent: None,
            op: "new".into(),
            time: 0,
        };
        Db::append(&path, &[crate::db::Record::Stack(stack)]).unwrap();
        let rev = Db::load(&path).unwrap().end_rev();
        Db::edit(&path, |db| db.restore_head("s", Some(rev), "again".into()).map(|_| ())).unwrap();
        let db = Db::load(&path).unwrap();
        assert_eq!(db.read_from_checkpoint(), (true, false));
        // Where the reply goes, as the review shows it: the thread as the
        // pull kept it, the history not read.
        let rp = &db.run("pull-1").unwrap().held.as_ref().unwrap().replies[0];
        assert_eq!(goes(&db, "s", rp), Goes::Reopens(Some(&open)));
        assert_eq!(db.read_from_checkpoint(), (true, false), "nothing has read the whole history");
        drop(db);
        // Accepted, reopened from it.
        assert_eq!(Db::edit(&path, |db| close(db, "pull-1", &Default::default(), None)).unwrap(), "1 reply accepted; 1 comment reopened");
        assert!(Db::load(&path).unwrap().note("c", "phab:a1").unwrap().text.ends_with(&fenced("reviewer ann", "Still?")));
    }

    #[test]
    fn what_is_left_of_work_is_said_without_what_was_taken() {
        // Its replies taken at once: said no more.
        let h = Held {
            replies: vec![Reply::default(), Reply::default()],
            ..Default::default()
        };
        assert_eq!((h.said_but(&[1]), h.said_but(&[0, 1])), ("1 reply".to_string(), String::new()));
        // Nothing brought back: which of the comments asked about are open.
        assert_eq!(nothing_back(0, 1), "the agent brought nothing back");
        assert_eq!(nothing_back(1, 1), "the agent brought nothing back, and the comment it was asked about is still open");
        assert_eq!(nothing_back(2, 2), "the agent brought nothing back, and the 2 comments it was asked about are still open");
        assert_eq!(nothing_back(1, 3), "the agent brought nothing back, and 1 of the 3 comments it was asked about is still open");
        assert_eq!(nothing_back(2, 3), "the agent brought nothing back, and 2 of the 3 comments it was asked about are still open");
    }
}
