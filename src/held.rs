//! Work held for review: the agent's, and the reviewers'. When an ask
//! lands, nothing changes under you: what it brought back is held in its
//! run, in the queue of work to review, oldest first. It may hold a change
//! of the stack, replies to the comments it was asked about, and comments
//! it opened; or nothing, while one of those comments is still open, for
//! its review to close it or keep it. Reviews pulled from Phabricator are
//! held the same way, in their pull's run: the reviewers' new comments,
//! their replies, and their comments edited on Phabricator since they were
//! filed. Each part is accepted or rejected, the change of the stack as
//! one; once every part is, the review closes: what was accepted is
//! applied, and the rest is dropped for good. A pull's parts are not
//! rejected: closing its review files every comment it pulled, a reply to
//! a thread you closed reopening that thread. Until then, the stack is not
//! edited by hand if the agent changed it, and the comments it answered
//! are not written on. Replies that are all their work holds may also be
//! taken at once, one by one, out of its review, unless that work goes on
//! from work under review: the last one taken, the review closes, and the
//! comments the ask was about stay open, as any comment does. Work that
//! brought nothing back has no part: it waits until you close its review,
//! which closes the comments it was asked about or keeps them.
//!
//! Your own edits by hand land the same way, in a run of their own
//! (`hand`), to review before they change anything. An edit made in the
//! review of landed work replaces it: that work is rejected, and the work
//! with your edit in it lands in its place, its replies and comments as
//! they were.

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

use crate::db::{Db, Error, Note, Patch, Result, Run, err, now};
use crate::thread::{Role, Turn, all_turns, append_agent, append_turn, render_thread};

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
    /// A lint job's: what the linter did, patch by patch, and what it
    /// reported that it did not fix, its own lines; shown in the review, to
    /// read. With no change of the stack, it is all the work holds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lint: Option<crate::lint::Report>,
    /// The tools the agent could not use, each refused, allowed for this
    /// work or allowed from now on, as the review marks it: with them, work
    /// lands for review though the agent failed or brought nothing back.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<crate::refusals::Refusal>,
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
    /// wrote it, or the agent's, the effort its run was asked at on it; your
    /// answer to it after, if you wrote one. A reviewer's edit is their edit
    /// of their turn as it was, which it redacts (`edited`).
    pub fn after(&self, text: &str) -> String {
        let text = match (&self.reviewer, &self.was) {
            (Some(who), Some(was)) => edited(text, who, was, &self.text),
            (Some(who), None) => append_turn(text, Role::Reviewer, &self.text, Some(who)),
            (None, _) => append_agent(text, &self.text, &self.tag, self.effort_said()),
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

    /// It, said: `the agent's reply`, `ann's reply`, `ann's edit`; the
    /// agent's, its run asked at an effort, `the agent's reply (max)`.
    pub fn said(&self) -> String {
        let what = if self.was.is_some() { "edit" } else { "reply" };
        crate::thread::named_at(&format!("{}'s {what}", self.who()), self.effort_said())
    }

    /// The name of the effort the agent's run was asked at, if it is the
    /// agent's and its run had one.
    pub fn effort_said(&self) -> Option<&'static str> {
        self.effort.filter(|_| self.reviewer.is_none()).map(crate::db::Effort::name)
    }
}

/// Thread `text` with the comment of reviewer `who` that said `was` saying
/// `now`, as Phabricator has it since they edited it: their edit of it, as
/// their last turn, its heading quoting what it edits (`ann's edit of the
/// comment that said "Hi Da"`). Their turn that said it, the latest version
/// of their comment, stays as it was, for what answered it to read as it
/// was written, redacted. That turn gone (rewound, say), `now` is their
/// last turn, and nothing is redacted.
fn edited(text: &str, who: &str, was: &str, now: &str) -> String {
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
    turns.push(Turn {
        role: Role::Reviewer,
        body: now.trim().to_string(),
        tag: Some(who.to_string()),
        deleted: false,
        edit,
        redacted: false,
        effort: None,
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
    /// the comments opened, a try push's tasks, the replies.
    pub fn parts(&self) -> Vec<Part> {
        let mut out = Vec::new();
        if self.stack.is_some() {
            out.push(Part::Stack);
        }
        out.extend((0..self.opened.len()).map(Part::Opened));
        #[cfg(feature = "try-push")]
        out.extend((0..self.tasks.len()).map(Part::Task));
        out.extend((0..self.replies.len()).map(Part::Reply));
        out
    }

    pub fn is_empty(&self) -> bool {
        self.stack.is_none() && self.replies.is_empty() && self.opened.is_empty() && self.no_tasks() && self.lint.is_none()
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
        let replies: Vec<&Reply> = self.replies.iter().enumerate().filter(|(k, _)| !taken.contains(k)).map(|(_, rp)| rp).collect();
        let edits = replies.iter().filter(|rp| rp.was.is_some()).count();
        if replies.len() > edits {
            out.push(count(replies.len() - edits, "reply", "replies"));
        }
        if edits > 0 {
            out.push(count(edits, "edited comment", "edited comments"));
        }
        if !self.opened.is_empty() {
            out.push(count(self.opened.len(), "opened comment", "opened comments"));
        }
        // A try push's tasks first, as they are what it brings.
        #[cfg(feature = "try-push")]
        if !self.tasks.is_empty() {
            out.insert(0, count(self.tasks.len(), "task to push to try", "tasks to push to try"));
        }
        // What the linter reported that it did not fix, last.
        if self.lint.as_ref().is_some_and(|l| !l.errors.is_empty()) {
            out.push("what the linter reported".to_string());
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
        crate::ask::promote(db, name, &n, why, rp.effort)?;
    }
    Ok(reopened)
}

/// Whether the replies of the work run `r` holds may be taken at once, out
/// of its review: they are all it holds, with no change of the stack nor
/// comment opened that they go with, and it does not go on from work that
/// waits for review, which is reviewed first. If not, why.
pub fn can_take(db: &Db, r: &Run) -> Result<()> {
    let Some(h) = &r.held else { return err(format!("run {} holds nothing to review", r.id)) };
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
/// applied. A pull's parts are not rejected.
pub fn save(db: &mut Db, id: &str, marks: &BTreeMap<String, bool>) -> Result<()> {
    let (r, h, mut v) = under_review(db, id)?;
    if pulled(&r) && rejects(&h, marks) {
        return err(FILED);
    }
    v.marks = marks.clone();
    db.set_review(id, v).map(|_| ())
}

/// What refusing to reject a comment pulled from Phabricator says: each is
/// filed as its pull's review closes.
pub const FILED: &str = "a comment pulled from Phabricator cannot be rejected: close the review to file it";

/// Whether `marks` rejects a part of work `h`.
fn rejects(h: &Held, marks: &BTreeMap<String, bool>) -> bool {
    h.parts().iter().any(|p| marks.get(&p.key()) == Some(&false))
}

/// The marks of the review of the work run `r` holds, as saved; of a
/// pull's, every part accepted, as none is rejected: closing its review
/// files each comment it pulled. Marks a stacked of before saved
/// rejecting some are passed over.
pub fn marks(db: &Db, r: &Run) -> BTreeMap<String, bool> {
    let mut marks = db.review(&r.id).map(|v| v.marks.clone()).unwrap_or_default();
    if pulled(r) {
        marks.extend(r.held.iter().flat_map(Held::parts).map(|p| (p.key(), true)));
    }
    marks
}

/// What you wrote in the review of work `r`: the comments written there,
/// still open, and your answers to its replies, to the reasons of a try
/// push's tasks, and to what it said of the tools it could not use. A
/// comment written there is one you opened in it, or one whose thread you
/// wrote in there, whoever opened it (a reviewer, the agent); on the stack
/// or one of its patches, or on a patch the work adds (`added_changes`),
/// which the stack holds only once the work is accepted. With any, the
/// work is not accepted: it goes back to the agent.
pub fn yours_in(db: &Db, r: &Run) -> (Vec<(String, String)>, usize) {
    let mut notes = db.stack_notes(&r.stack);
    for change in added_changes(db, r) {
        notes.extend(db.notes(&change));
    }
    let opened = notes
        .into_iter()
        .filter(|n| n.review.as_deref() == Some(r.id.as_str()))
        .map(|n| (n.change.clone(), n.id.clone()))
        .collect();
    let answers = r.held.as_ref().map_or(0, |h| {
        h.replies.iter().filter(|rp| rp.answer.is_some()).count() + h.task_answers() + h.tools.iter().filter(|t| t.answer.is_some()).count()
    });
    (opened, answers)
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
/// its review: empty, it takes it back.
pub fn answer(db: &mut Db, id: &str, k: usize, text: &str) -> Result<()> {
    let (r, mut h, _) = under_review(db, id)?;
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
/// how, whether of the whole stack, and at what effort, the work's own.
#[derive(Debug, Clone, PartialEq)]
pub struct Round {
    pub notes: Vec<(String, String)>,
    pub way: crate::db::Way,
    pub whole: bool,
    pub effort: Option<crate::db::Effort>,
    /// How many edits that went on from it are asked again.
    pub dropped: usize,
    /// What the next round may use for this work, of what the agent was
    /// refused; and what the review saved for good, or could not.
    pub allowed: Vec<String>,
    pub settled: crate::refusals::Settled,
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
    // A tool allowed is reason enough: the agent tries again with it.
    let mut marks = marks.clone();
    let (work, always) = crate::refusals::allowed(&h.tools, &marks);
    if opened.is_empty() && answers == 0 && work.is_empty() && always.is_empty() {
        return err("you wrote nothing in its review to send back, nor allowed a tool the agent could not use: accept or reject it");
    }
    // What is allowed from now on is saved for the next round to use, once
    // nothing stops the work going back. Where it cannot be, it is allowed
    // for this work instead, the agent told so.
    let early = (!always.is_empty() && crate::refusals::local_of(db, &r).is_err()).then(|| crate::refusals::settle(db, &r, &mut marks, true));
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
    #[cfg_attr(not(feature = "try-push"), allow(unused_mut))]
    let mut said: Vec<String> = Vec::new();
    #[cfg(feature = "try-push")]
    {
        said.extend(crate::try_push::answers_turn(&h.tasks));
        v.marks.extend(marks.iter().filter(|(k, _)| matches!(Part::of_key(k), Some(Part::Task(_)))).map(|(k, m)| (k.clone(), *m)));
    }
    said.extend(crate::refusals::turn(&h.tools, &marks));
    if !said.is_empty()
        && let Some(n) = asking_open(db, &r).first().map(|(_, n)| (*n).clone())
    {
        db.set_note(Note { text: append_turn(&n.text, Role::You, &said.join("\n\n"), None), ..n })?;
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
    let settled = match early {
        Some(s) => s,
        None => crate::refusals::settle(db, &r, &mut marks, true),
    };
    v.marks.retain(|k, _| !crate::refusals::is_key(k));
    v.marks.extend(marks.iter().filter(|(k, _)| crate::refusals::is_key(k)).map(|(k, m)| (k.clone(), *m)));
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
        summary: format!("reviewed: sent back to the agent, {}{}", count(n, "comment to ask again", "comments to ask again"), settled.said(i.way)),
        ..r
    })?;
    Ok(Round {
        notes,
        way: i.way,
        whole,
        effort: i.effort,
        dropped,
        allowed: crate::refusals::carried(db, id),
        settled,
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

/// The comments the ask of run `r` was about that are still open, neither
/// closed nor gone, each with its place among them.
pub fn asking_open<'a>(db: &'a Db, r: &Run) -> Vec<(usize, &'a Note)> {
    asking_of(r).into_iter().enumerate().filter_map(|(k, (c, i))| Some((k, db.note(&c, &i)?))).collect()
}

/// Closes run `id`'s review: each part as marked in `marks`, those left
/// unmarked accepted or rejected as `rest` says (with none, every part must
/// be marked). Rejecting the change of the stack rejects its replies and
/// comments too. What is accepted is applied, the change of the stack only
/// if the stack is still the one the agent took; the rest is dropped. A
/// pull's parts are all accepted, a mark or `rest` rejecting any refused:
/// each comment is filed, a reviewer's reply to a thread you closed
/// reopening it. What came of it, said.
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
    // A pull's: every comment it pulled is filed, none rejected.
    let rest = match pulled(&r) {
        true if rest == Some(false) || rejects(&h, marks) => return err(FILED),
        true => Some(true),
        false => rest,
    };
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
    // on its lines following them, then the comments opened. A reviewer's
    // reply to a thread you closed reopens it.
    let mut reopened = 0;
    for (k, rp) in h.replies.iter().enumerate() {
        if all.get(&Part::Reply(k).key()) == Some(&true) && apply_reply(db, &r.stack, rp)? {
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
    if let Some((c, true)) = change {
        let s = db.stack(&r.stack)?.clone();
        let olds: Vec<Patch> = s.patches.iter().map(|p| db.patch(p).clone()).collect();
        let base = c.base.clone().or(s.base.clone());
        rev = Some(db.set_stack_on(&r.stack, c.patches.clone(), base, s.repo.clone(), &c.op)?);
        crate::ask::follow_lines(db, &olds, &c.patches)?;
    }
    // A comment the ask was about, kept open, on a patch the stack held
    // neither before nor holds now (one that work it went on from added,
    // the stack never taking it), stays in sight: on the stack's own
    // comments, moved there from where it was.
    let held_after = held_changes(db, &r.stack);
    let mut moved = 0;
    for (c, i) in asking_of(&r) {
        if held_before.contains(&c) || held_after.contains(&c) || c == crate::db::stack_change(&r.stack) {
            continue;
        }
        if let Some(n) = db.note(&c, &i).cloned() {
            crate::ask::to_stack(db, &r.stack, &n)?;
            moved += 1;
        }
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
    // The tools the agent could not use as you marked them: what you allowed
    // from now on saved, accepted or rejected; what you allowed for this
    // work ends here.
    v.marks.retain(|k, _| !crate::refusals::is_key(k));
    v.marks.extend(marks.iter().filter(|(k, _)| crate::refusals::is_key(k)).map(|(k, m)| (k.clone(), *m)));
    let settled = crate::refusals::settle(db, &r, &mut v.marks, false);
    v.closed = true;
    db.set_review(id, v.clone())?;
    // Nothing brought back: no part to say, but the tools it could not use.
    // What the linter reported and fixed nothing of is only read.
    let mut said = match (h.is_empty(), said(&h, &v.marks)) {
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
    db.add_run(Run {
        summary: format!("reviewed: {said}"),
        rev,
        ..r
    })?;
    Ok(said)
}

/// The tasks of the try push work `h` that `marks` accepts, those it does
/// not mark as `rest` says, in their order.
#[cfg(feature = "try-push")]
pub fn accepted_tasks(h: &Held, marks: &BTreeMap<String, bool>, rest: Option<bool>) -> Vec<crate::try_push::Task> {
    h.tasks.iter().enumerate().filter(|(k, _)| marks.get(&Part::Task(*k).key()).copied().or(rest) == Some(true)).map(|(_, t)| t.clone()).collect()
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
    let replies = |edits: bool| (0..h.replies.len()).filter(|&k| h.replies[k].was.is_some() == edits).map(Part::Reply).collect::<Vec<_>>();
    #[cfg(feature = "try-push")]
    let tasks: Vec<Part> = (0..h.tasks.len()).map(Part::Task).collect();
    #[cfg(not(feature = "try-push"))]
    let tasks: Vec<Part> = Vec::new();
    for (one, many, parts) in [
        ("task", "tasks", tasks),
        ("opened comment", "opened comments", (0..h.opened.len()).map(Part::Opened).collect::<Vec<_>>()),
        ("reply", "replies", replies(false)),
        ("edited comment", "edited comments", replies(true)),
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
        let rev = Db::load(&path).unwrap().last_rev();
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
