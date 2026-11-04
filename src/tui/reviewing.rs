//! The review of what landed: what the agent brought back, or the reviews
//! a pull brought from Phabricator. On the left, its parts, in
//! three sections: the stack operations its change makes, a line for each
//! patch; the comments it opened; its replies. On the right, the part the
//! cursor is on as it would be: the stack diff of the change, or the
//! comment in its place, only to look at. Each part is accepted or
//! rejected, the change of the stack as one; then the review is saved to go
//! on later, thrown away, or closed, what was accepted applied. Work that
//! brought nothing back has no part: it is reviewed for the comments its
//! ask was about, to close or keep.
//!
//! The tools the agent could not use are a section of their own, each
//! item refused (Delete), allowed for this work, or allowed from now on
//! (Enter), as its menu picks too; on the right, what the agent said of it,
//! the calls it tried under that. Work that holds one lands though it failed
//! or brought nothing back, to allow a tool and send it back.
//!
//! So are the answers made on the stack before the change, chained, that
//! accepting the change leaves behind, as the review finds them when it
//! opens: each running or waiting for your review, marked keep (Enter),
//! restart, or stop (Delete), as its menu picks too, before the review
//! closes with the change accepted; on the right, the comment it answers.
//! An answer asked once the review is open is none of them: it goes on as
//! it is.

use std::collections::BTreeMap;
use std::ops::Range;
use std::path::PathBuf;

use super::*;
use crate::db::Run;
use crate::held::{AnswerMark, Goes, Held as Work, Part};
use crate::history::Diffed;
use crate::operations::Ops;
use crate::refusals::{Allow, Refusal};
#[cfg(feature = "try-push")]
use crate::try_push::thousands;

/// What the review's right pane shows of a part: what it is, said; its
/// lines; and the lines that are the part, the first to show first.
type View = (String, Vec<Vec<(String, Style)>>, Range<usize>);

/// What leaving the review of a pull's comments says: they wait for it.
pub(super) const PULL_WAITS: &str = "the comments pulled wait for your review: closing it files every one of them";

/// The review's item that sends the work back to the agent, from the
/// dialog it opens.
pub(super) const SEND_BACK: &str = "Send it back to the agent…";

/// How the work goes back, as the review's menu says under what you wrote.
const SENT_HOW: &str = "Send it back to the agent… opens a dialog that sets what the agent may do, and its effort: a permission that changes the stack rejects this work, and the agent's new work lands in its place; one that only reads asks a follow-up, this work staying waiting for your review, and what the follow-up brings back comes into this review.";

/// What the review's section of the answers made on the stack before the
/// change is called, as its heading says it, and its line of what Esc says.
pub(super) const ANSWERS: &str = "answers made on the stack before the change";

/// What marking a patch the change leaves as it was says: it has no mark.
pub(super) const NOTHING_TO_MARK: &str = "the change leaves this patch as it is: there is nothing to accept or reject";

/// What the status line says while the stack the operations accepted leave
/// is checked, away from the screen, before the review closes.
pub(super) const CHECKING_CLOSE: &str = "checking the stack the operations you accepted leave…";

/// What it says when the review changed while that stack was checked: what
/// was checked is not what would close.
pub(super) const CLOSE_CHANGED: &str = "the review changed while the stack the operations you accepted leave was being checked, so nothing changed: close it again";

/// A close of the review asked, waiting for its check of the stack the
/// operations accepted leave, made away from the screen: the review as it
/// was when it was asked, its run and its marks; the parts not marked, as
/// `rest` says; whether the build and test of the stack as it was is
/// stopped too; and whether it was asked from the banner, the review not
/// open.
pub(super) struct ToClose {
    review: Reviewing,
    rest: Option<bool>,
    stop: bool,
    banner: bool,
}

/// Where an answer made on the stack before the change stands, as its line
/// of the review says it: running, or landed and waiting for your review,
/// to mark; or, since the review opened, failed, or reviewed, with nothing
/// to mark.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Going {
    Running,
    Waiting,
    Failed,
    Reviewed,
}

impl Going {
    /// Whether an answer standing so is to mark: running, or waiting for
    /// your review.
    fn markable(self) -> bool {
        matches!(self, Going::Running | Going::Waiting)
    }

    /// It in a word, as the answer's line says it after its mark, the list
    /// being narrow: `running`, `landed` (waiting for your review),
    /// `failed`, `reviewed`.
    fn word(self) -> &'static str {
        match self {
            Going::Running => "running",
            Going::Waiting => "landed",
            Going::Failed => "failed",
            Going::Reviewed => "reviewed",
        }
    }

    /// Why an answer standing so has nothing to mark, said, if it has not:
    /// `this answer failed since the review opened: nothing to mark`.
    fn unmarkable(self) -> Option<&'static str> {
        match self {
            Going::Failed => Some("this answer failed since the review opened: nothing to mark"),
            Going::Reviewed => Some("this answer was reviewed since the review opened: nothing to mark"),
            _ => None,
        }
    }
}

/// What mark `m` does, once the change is accepted, to an answer made on
/// the stack before it, standing as `going`: said after the mark's word, in
/// the box of its menu and on the status line.
fn answer_does(m: AnswerMark, going: Going) -> &'static str {
    match (m, going) {
        (AnswerMark::Keep, Going::Running) => "the answer goes on, and lands for your review on the stack it was asked on",
        (AnswerMark::Keep, _) => "the answer waits for your review as it is",
        (AnswerMark::Restart, Going::Running) => "the agent is stopped, and the answer is asked again on the stack the change makes",
        (AnswerMark::Restart, _) => "the answer is asked again on the stack the change makes, shown what it brought back",
        (AnswerMark::Stop, Going::Running) => "the agent is stopped, and the comment stays open, not asked again",
        (AnswerMark::Stop, _) => "what the answer brought back is thrown away unseen, and the comment stays open, not asked again",
    }
}

/// What an answer's mark says, as the list shows it: `✓ keep` as a part
/// accepted is marked, `✗ stop` as one rejected, `↻ restart` with no colour
/// of its own, and `·` while it is not marked.
fn answer_mark_of(m: Option<AnswerMark>) -> (&'static str, Style) {
    match m {
        Some(AnswerMark::Keep) => ("✓ keep", color::add()),
        Some(AnswerMark::Stop) => ("✗ stop", color::del()),
        Some(AnswerMark::Restart) => ("↻ restart", Style::new()),
        None => ("·", Style::new()),
    }
}

/// The stack diff of the change, as the right pane shows it: what it is,
/// said; the diff, and its lines drawn. Its stops of the cursor, a line of
/// the diff each, or, its interdiffs side by side, each pair of the two
/// versions' lines, or, in a view of the change, each line of it
/// (`interdiffs::stack_stops`; a heading over an interdiff is drawn, and no
/// stop), the cursor on the one its line of the diff puts it on
/// (`Reviewing::entry`, `keys`), whatever the view: for each, the lines
/// drawn it takes; what it shows (`interdiffs::Seen`); where Enter on it
/// goes, the patch it is of (by its entry among the stack operations) and,
/// on a line of what changed in it, the line of that patch's mail it is,
/// the new version's where it shows two; whether it is a line of what
/// changed, and one added or taken out. And the line of the diff each
/// entry of the stack operations is.
#[derive(Default)]
struct DiffView {
    said: String,
    diff: Vec<Diffed>,
    lines: Vec<Vec<(String, Style)>>,
    spans: Vec<Range<usize>>,
    seen: Vec<interdiffs::Seen>,
    keys: Vec<usize>,
    goes: Vec<Option<(usize, Option<usize>)>>,
    code: Vec<bool>,
    changed: Vec<bool>,
    of_entry: Vec<usize>,
}

/// Where the lines that put the cursor on a line of the change as a patch
/// of its own start (`DiffView::keys`): past any line of the diff.
const CHANGE_KEYS: usize = usize::MAX / 2;

impl DiffView {
    /// The stop the line of the diff `k` puts the cursor on: the one that
    /// shows it; a line of the change, the one it is the key of; past them
    /// all, the last.
    fn stop_of(&self, k: usize) -> usize {
        let last = self.keys.len().saturating_sub(1);
        match k >= CHANGE_KEYS {
            true => self.keys.iter().position(|&key| key == k),
            false => self.seen.iter().position(|s| s.shows.contains(&k)),
        }
        .unwrap_or(last)
    }

    /// The line of the diff that puts the cursor on stop `i`: the first it
    /// shows; of a line of the change, a key of its own, by its patch and
    /// its place among the lines of its change.
    fn line_of(&self, i: usize) -> usize {
        self.keys.get(i).copied().unwrap_or(0)
    }

    /// Where the cursor goes in it from entry `entry` of the stack
    /// operations: on the first line that changed of that patch, else on
    /// the patch; from none, the section's title, on the first line that
    /// changed of any. The line of the diff that puts it there.
    fn first_change(&self, entry: Option<usize>) -> usize {
        let stop = match entry.and_then(|e| self.of_entry.get(e).copied()) {
            Some(f) => {
                let f = self.stop_of(f);
                (f + 1..self.code.len()).take_while(|&k| self.code[k]).find(|&k| self.changed[k]).unwrap_or(f)
            }
            None => self.changed.iter().position(|&c| c).unwrap_or(0),
        };
        self.line_of(stop)
    }
}

/// What the two versions of each interdiff of the stack diff are named in
/// the review of what landed, as the views of an interdiff name them: the
/// patch as the stack has it, and as the work leaves it.
pub(super) const AS_IT_IS: (&str, &str) = ("the stack's version", "the work's version");

/// The row of `rows` that is line `at` of their patch: of its message, or
/// of its diff (a line of a file's header without a row of its own, the row
/// before it).
fn row_of(rows: &[Row], at: crate::ops::MailLine) -> usize {
    let own = |r: &Row| r.note.is_none();
    match at {
        crate::ops::MailLine::Message(i) => rows.iter().position(|r| own(r) && r.msg_at == Some(i)),
        crate::ops::MailLine::Diff(j) => rows.iter().rposition(|r| own(r) && r.diff_at.is_some_and(|d| d <= j)),
    }
    .unwrap_or(0)
}

/// Where the review's cursor is in the stack the work makes, for an edit
/// by hand from there: the patch, by its place; the line of its mail the
/// cursor is on, from 1, if it is on one; and the file and line, from 1,
/// of the code it is on, as the patch leaves it, if it is.
struct WorkPlace {
    at: usize,
    mail: Option<usize>,
    code: Option<(String, usize)>,
}

impl WorkPlace {
    /// Patch `p`, at place `at`, the cursor on `row` of its rows, if on one:
    /// what of it the row is, as the middle has it of its own rows.
    fn of_row(at: usize, p: &Patch, row: Option<&Row>) -> WorkPlace {
        WorkPlace {
            at,
            mail: row.and_then(|r| mail_of(p, r)),
            code: row.and_then(code_of),
        }
    }
}

/// A review being made: of what run `run` brought back.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Reviewing {
    pub run: String,
    /// Each part marked: accepted, or rejected; each operation of the
    /// change of the stack on its own. As saved, and since.
    pub marks: BTreeMap<String, bool>,
    /// The comments' marks when every operation of the change of the stack
    /// was rejected, rejecting them with it: they come back once one is
    /// accepted again.
    before: Option<BTreeMap<String, bool>>,
    /// The line of the list the cursor is on, and the first one shown.
    pub sel: usize,
    top: usize,
    /// Whether the cursor is in the other pane, to scroll it, and how far;
    /// and whether that pane is wide, the list out of sight.
    pub in_main: bool,
    scroll: usize,
    pub wide: bool,
    /// The replies' marks when the comment they answer was marked to close,
    /// accepting them with it: they come back if it is kept again.
    asked_before: BTreeMap<String, Option<bool>>,
    /// On a patch of the stack the work makes, looked at closer: the row
    /// the cursor is on, the first row shown, and how wide they were drawn.
    row: usize,
    row_top: usize,
    main_w: usize,
    /// On the stack diff, looked at closer: the entry the cursor is on.
    entry: usize,
    /// HEAD of your checkout picked to stay where it is as the review
    /// closes, rather than follow the stack: it follows unless so.
    pub stay: bool,
    /// Whether, from there, the cursor is in the patch of that entry, as the
    /// work leaves it, a row at a time; and the line of its mail to put the
    /// cursor on once it is drawn, the one Enter went to from the stack diff.
    pub in_patch: bool,
    goto: Option<usize>,
    /// Where what the review allows from now on is saved: the stack's
    /// checkout's own settings; else why it cannot be. Looked at as the
    /// review opens, when its work holds a tool the agent could not use.
    pub local: Option<std::result::Result<PathBuf, String>>,
    /// The answers made on the stack before the change that accepting it
    /// leaves behind, by their runs, as the review found them when it
    /// opened (`ask::answers_left`): each to mark keep, restart or stop, as
    /// `marks` hold it, while it runs or waits for your review. An answer
    /// asked since is none of them, and goes on as it is.
    pub answers: Vec<String>,
}

/// What you wrote in the review a line of its list is: a comment you opened
/// there, by (change, id); your answer to the work's reply `0`; or your
/// answer to why the agent selected a try push's task `0`.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Yours {
    Comment(String, String),
    Answer(usize),
    #[cfg(feature = "try-push")]
    TaskAnswer(usize),
    ToolAnswer(usize),
}

/// A line of the review's list: what it says, in parts, each in its own
/// style, and the part it is of (none, a section's title); for the
/// change's, its place in the diff.
pub(super) struct Line {
    parts: Parts,
    part: Option<Part>,
    entry: Option<usize>,
    /// The comment the ask was about it is, by its place among them: kept,
    /// or closed with the review.
    asking: Option<usize>,
    /// What you wrote in the review it is: to see where it would be, to
    /// write again, or to take back.
    yours: Option<Yours>,
    /// What a run of the linter reported and did not fix, by its place
    /// among those the work holds: to read.
    report: Option<usize>,
    /// Of a reconcile with HEAD's work, stacked's comment: what to run in
    /// your checkout should you reject it, to read.
    stacked: bool,
    /// A tool the agent could not use, by its place among those the work
    /// holds: refused, or allowed for this work or from now on.
    tool: Option<usize>,
    /// An answer made on the stack before the change, by its run: kept,
    /// restarted or stopped once the change is accepted.
    answer: Option<String>,
    /// A follow-up asked from the review that is going, by its place among
    /// them: to see what it asks, and how far the agent is.
    following: Option<usize>,
    /// What a build and test asked as a follow-up found: to read.
    found: Option<Found>,
    /// What a pull from Phabricator found that changes nothing, by its
    /// place among what it found: to read.
    noted: Option<usize>,
}

/// What a build and test asked as a follow-up from the review found, a
/// line of the review's list: its reply `0` among the work's; or what
/// follow-up `0` of the work found of a patch, its finding `1`.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Found {
    Reply(usize),
    Patch(usize, usize),
}

impl Line {
    /// A line saying `parts`, of no part: a section's title, until said
    /// otherwise.
    fn new(parts: Parts) -> Line {
        Line {
            parts,
            part: None,
            entry: None,
            asking: None,
            yours: None,
            report: None,
            stacked: false,
            tool: None,
            answer: None,
            following: None,
            found: None,
            noted: None,
        }
    }

    /// Whether the cursor goes on it: a part, a comment that asked, what
    /// you wrote, what the linter reported, stacked's comment, a tool the
    /// agent could not use, an answer made on the stack before the change,
    /// a follow-up going, what a build and test asked as one found, or what
    /// a pull found; not a section's title.
    fn selectable(&self) -> bool {
        self.part.is_some()
            || self.asking.is_some()
            || self.yours.is_some()
            || self.report.is_some()
            || self.stacked
            || self.tool.is_some()
            || self.answer.is_some()
            || self.following.is_some()
            || self.found.is_some()
            || self.noted.is_some()
    }

    /// What it is, to find it again once the list changes under it (a
    /// follow-up landing): the part, the comment, what you wrote, the
    /// report, the tool, the answer, the follow-up or what was found it is.
    fn key(&self) -> Option<String> {
        if let Some(p) = self.part {
            return Some(format!("{}{}", p.key(), self.entry.map_or(String::new(), |e| format!(" entry {e}"))));
        }
        let key = match (&self.asking, &self.yours, &self.report, &self.tool, &self.answer, &self.following, &self.found) {
            (Some(k), ..) => format!("asking {k}"),
            (_, Some(y), ..) => format!("yours {y:?}"),
            (_, _, Some(k), ..) => format!("report {k}"),
            (_, _, _, Some(k), ..) => format!("tool {k}"),
            (_, _, _, _, Some(id), ..) => format!("answer {id}"),
            (_, _, _, _, _, Some(k), _) => format!("following {k}"),
            (_, _, _, _, _, _, Some(f)) => format!("found {f:?}"),
            _ if self.stacked => "stacked".to_string(),
            _ => {
                let k = self.noted?;
                format!("noted {k}")
            }
        };
        Some(key)
    }
}

/// How a review closes, for what that asks again, its change accepted
/// whole, in part, or rejected: or how the work goes back to the agent
/// while it stays open, followed up.
#[derive(Clone, Copy, PartialEq)]
enum Closing {
    Accepting,
    Partly,
    Rejecting,
    SendingBack,
    FollowingUp,
}

/// Where a follow-up going stands, said: with the agent, or waiting its
/// turn.
fn following_said(f: &ask::Following) -> &'static str {
    if f.run.is_some() { "with the agent" } else { "waiting its turn" }
}

/// What a follow-up going is, said, as the review's line of it says it:
/// how it was asked and at what effort, then how many comments.
fn following_what(f: &ask::Following) -> String {
    let at = f.effort.map_or(String::new(), |e| format!(", at {}", e.name()));
    let n = f.notes.len();
    format!("{}{at} · {n} comment{}", f.way.said(), if n == 1 { "" } else { "s" })
}

/// What a part's mark says, as the list shows it.
fn mark_of(m: Option<bool>) -> (&'static str, Style) {
    match m {
        Some(true) => ("✓", color::add()),
        Some(false) => ("✗", color::del()),
        None => ("·", Style::new()),
    }
}

/// What marking operation `key` of `ops` accepted (`accept`), or rejected,
/// asks first when operations `with` go with it: whether to mark them all,
/// and why they go with it; the menu on keeping them as they are.
fn with_ask(ops: &Ops, key: &str, with: &[crate::operations::With], accept: bool) -> Menu {
    let others: Vec<String> = with.iter().map(|w| w.key.clone()).collect();
    let mut keys = vec![key.to_string()];
    keys.extend(others.iter().cloned());
    let (doing, done, yes) = match accept {
        true => ("accept", "Accepted", "Accept them"),
        false => ("reject", "Rejected", "Reject them"),
    };
    let title = format!("{doing} {} with {}?", ops.said(key), ops.listed(&others));
    let mut m = Menu::ask(title, Item::new(yes, Action::MarkOpsWith(keys, accept)), "Keep them as they are", true);
    m.note = Some(format!("{} {done} alone, {} would leave a stack that does not apply.", ops.with_all_said(with), ops.said(key)));
    m
}

/// A line of the stack diff, as one of the stack operations: what is done
/// to each patch.
fn operation(d: &Diffed) -> Option<(String, Style)> {
    Some(match d {
        Diffed::Base { what, was, is } => (format!("{what}  {was} → {is}"), color::hunk()),
        Diffed::Patch { at, title, added: true, .. } => (format!("add     {at} {title}"), color::add()),
        Diffed::Patch {
            at, title, reworded: true, was, ..
        } => (format!("reword  {at} {title}{}", was.map_or(String::new(), |w| format!(", from {w}"))), color::hunk()),
        Diffed::Patch {
            at, title, changed: true, was, ..
        } => (format!("modify  {at} {title}{}", was.map_or(String::new(), |w| format!(", from {w}"))), color::hunk()),
        Diffed::Patch { at, title, was: Some(w), .. } => (format!("move    {at} {title}, from {w}"), Style::new()),
        Diffed::Patch { at, title, .. } => (format!("noop    {at} {title}"), color::dim()),
        Diffed::Dropped { was, title } => (format!("remove  {was} {title}"), color::del()),
        _ => return None,
    })
}

/// Reviewer's edit `rp` of their comment, which said `was`, or that
/// comment read again with the suggested edit Phabricator gives with it
/// (`Reply::anew`), as its review shows it over its thread, `w` wide: under
/// a heading saying which, the comment as it was filed, then as it is now,
/// compared as two versions of a message are (`compared::message_rows`),
/// what only each has marked; a line between them and the thread.
fn edit_lines(rp: &crate::held::Reply, was: &str, w: usize) -> Vec<Vec<(String, Style)>> {
    // Each line as it ends aside: Phabricator's may end with a carriage
    // return.
    let plain = |s: &str| s.lines().map(str::trim_end).collect::<Vec<_>>().join("\n").trim().to_string();
    let heading = match rp.is_anew() {
        true => format!("{}'s comment as it was filed, then with the suggested edit that stacked now reads from Phabricator", rp.who()),
        false => format!("{}'s comment as it was filed, then as it was edited on Phabricator", rp.who()),
    };
    let mut rows = vec![super::compared::row(Kind::File, heading)];
    super::compared::message_rows(&mut rows, &plain(was), &plain(&rp.text), w);
    rows.push(super::compared::row(Kind::Blank, ""));
    rows.iter().map(|row| row_parts(row, false)).collect()
}

/// What a pull's operation on patch `n` (from 1) takes from Phabricator,
/// `t`, said as the review's right pane says it under the operation, a
/// sentence each: where the patch comes from, and why; its conflicts
/// marked, and what to do with them, or that they are `resolved` by hand
/// since.
fn taken_said(t: &crate::held::Taken, n: usize, resolved: bool) -> Vec<String> {
    use crate::held::Added;
    let d = &t.d;
    let mut out = Vec::new();
    let as_uploaded = "its latest diff, with the author and the message of its commit as uploaded, or its title and summary where Phabricator has them otherwise";
    match &t.added {
        Some(Added::Parent(of)) => out.push(format!("{d} is the parent of {of} on Phabricator, open, and the stack does not hold it: the pull adds it under the patch of {of}, as patch {n}, {as_uploaded}.")),
        Some(Added::Child(on)) => out.push(format!("{d} is stacked on {on} on Phabricator, open, and the stack does not hold it: the pull adds it over the patch of {on}, as patch {n}, {as_uploaded}.")),
        Some(Added::Asked) => out.push(format!("{d} is the revision this stack is pulled from: the pull adds it as patch {n}, {as_uploaded}.")),
        None => match (t.diff, t.message) {
            (true, true) => out.push(format!("The latest diff of {d}, and its title and summary, are no version of patch {n} that stacked knows: they came from elsewhere, and the pull takes them as patch {n}'s new version.")),
            (true, false) => out.push(format!("The latest diff of {d} is no version of patch {n} that stacked knows: it came from elsewhere, and the pull takes it as patch {n}'s new version, its message as it was.")),
            (false, true) => out.push(format!("The title and summary of {d} are no version of patch {n}'s message that stacked knows: they came from elsewhere, and the pull takes them as its message, its code as it was.")),
            (false, false) => {}
        },
    }
    if let Some(v) = t.since {
        out.push(format!(
            "Patch {n} changed in the stack since version {v}, the last diff of {d} stacked knows: the pull takes Phabricator's all the same, and what this version had since version {v}, which the pulled one lacks, is under this."
        ));
    }
    let markers = format!("<<<<<<< {} … >>>>>>>", crate::ops::PULL_OURS);
    match (&t.remade, t.diff) {
        (Some(files), true) if !files.is_empty() && resolved => out.push(format!(
            "The latest diff of {d} does not apply on the patches under patch {n}: the pull made it again on them, its conflicts marked in {}, which are resolved by hand since.",
            crate::head::and(files)
        )),
        (Some(files), false) if !files.is_empty() && resolved => out.push(format!(
            "Patch {n} as it was no longer applies on the pulled version under it: the pull made it again on it, its conflicts marked in {}, which are resolved by hand since.",
            crate::head::and(files)
        )),
        (Some(files), true) if !files.is_empty() => out.push(format!(
            "The latest diff of {d} does not apply on the patches under patch {n}: it is made again on them, its conflicts marked in {} ({markers}), to resolve. The review does not close with this operation accepted while they are marked: reject it, resolve them by hand in the patch from here, or send the pull back to the agent.",
            crate::head::and(files)
        )),
        (Some(_), true) => out.push(format!("The latest diff of {d} does not apply on the patches under patch {n} as it is: it is made again on them, its files merged with no conflict, to look over.")),
        (Some(files), false) if !files.is_empty() => out.push(format!(
            "Patch {n} as it was no longer applies on the pulled version under it: this new version is patch {n} made again on it, its conflicts marked in {} ({markers}), to resolve. The review does not close with this operation accepted while they are marked: reject it, which takes the operation that breaks it along, resolve them by hand in the patch from here, or send the pull back to the agent.",
            crate::head::and(files)
        )),
        (Some(_), false) => out.push(format!("Patch {n} as it was no longer applies on the pulled version under it: this new version is patch {n} made again on it, its files merged with no conflict, to look over.")),
        (None, _) => {}
    }
    out
}

/// Reviewer's reply `rp` to a comment you closed, said: `ann's reply to a
/// comment you closed`, `ann's edit of a comment you closed`, `ann's
/// suggested edit of a comment you closed`.
fn closed_said(rp: &crate::held::Reply) -> String {
    let to = if rp.was.is_some() { "of" } else { "to" };
    format!("{} {to} a comment you closed", rp.said())
}

/// A comment's first words.
pub(super) fn first_words(text: &str) -> String {
    thread_turns(text).first().map(|t| crate::markdown::first_line(&t.body)).unwrap_or_default()
}

/// Stacked's comment in the review of reconcile `job`, as the right pane
/// shows it, `w` wide: what to run in your checkout should you reject it,
/// each sentence folded between words. All of it is the part.
fn stacked_view(job: &crate::reconcile::Job, w: usize) -> View {
    let mut lines: Vec<Vec<(String, Style)>> = Vec::new();
    for (k, said) in crate::reconcile::comment(job).iter().enumerate() {
        if k > 0 {
            lines.push(vec![(String::new(), Style::new())]);
        }
        lines.extend(wrap(said, w.max(20)).into_iter().map(|l| vec![(l, Style::new())]));
    }
    let n = lines.len();
    (" stacked's comment: if you reject it, what to run in your checkout".into(), lines, 0..n)
}

/// What a run of the linter of the lint job's work `l` reported and did
/// not fix, `e`, as the right pane shows it, `w` wide: where it ran and how
/// it exited, said; then each line it wrote, as it wrote it, a line too long
/// for the pane going on under it. All of it is the part.
fn report_view(l: &crate::lint::Report, e: &crate::lint::Errors, w: usize) -> View {
    let said = format!(" {} at {}: what it reported, and did not fix", l.command, e.said(l.stopped.as_ref().map(|s| s.at)));
    let w = w.max(20);
    let mut lines: Vec<Vec<(String, Style)>> = Vec::new();
    for text in &e.lines {
        let chars: Vec<char> = text.chars().collect();
        if chars.is_empty() {
            lines.push(vec![(String::new(), Style::new())]);
        }
        for piece in chars.chunks(w) {
            lines.push(vec![(piece.iter().collect(), Style::new())]);
        }
    }
    if lines.is_empty() {
        lines.push(vec![("It wrote nothing.".to_string(), color::dim())]);
    }
    let n = lines.len();
    (said, lines, 0..n)
}

impl App {
    /// What run `run` brought back.
    pub(super) fn work(&self, run: &str) -> Option<(&Run, &Work)> {
        let r = self.db.run(run)?;
        Some((r, r.held.as_ref()?))
    }

    /// The parts of what is reviewed still to resolve: the replies taken
    /// already left out.
    fn review_parts(&self) -> Vec<Part> {
        let Some(r) = self.reviewing.as_ref().and_then(|rv| self.db.run(&rv.run)) else { return vec![] };
        crate::held::parts_left(&self.db, r)
    }

    /// Opens the review of what run `run` brought back, as `show_review`
    /// does.
    pub(super) fn open_review(&mut self, run: &str, part: Option<Part>) {
        self.show_review(run, part);
    }

    /// Shows the review of what run `run` brought back, with its marks as
    /// last saved; the cursor on `part`, else where it was when the review
    /// was left, else on the first not marked.
    pub(super) fn show_review(&mut self, run: &str, part: Option<Part>) {
        let Some((r, _)) = self.work(run) else { return };
        // Gone on from work under review, guessing: that one is reviewed first.
        if ask::guessing(&self.db, r).is_some() {
            self.status = "this work goes on from the work landed before it, which waits for your review: review that one first".into();
            return;
        }
        let marks = crate::held::marks(&self.db, r);
        self.reviewing = Some(Reviewing {
            run: run.into(),
            marks,
            local: self.local_of(run),
            answers: self.answers_of(run),
            ..Default::default()
        });
        self.screen = Screen::Review;
        self.on_banner = false;
        let lines = self.review_lines();
        let marks = self.reviewing.as_ref().map(|r| r.marks.clone()).unwrap_or_default();
        let was = self.review_was(run).filter(|&k| lines.get(k).is_some_and(|l| l.part.is_some()));
        let at = match part {
            Some(p) => lines.iter().position(|l| l.part == Some(p)),
            None => was.or_else(|| lines.iter().position(|l| l.part.is_some_and(|p| !self.part_marked(&marks, p)))),
        };
        // No part left: on the first tool the agent could not use, else on
        // the first comment that asked.
        let at = at
            .or_else(|| lines.iter().position(|l| l.part.is_some()))
            .or_else(|| lines.iter().position(|l| l.tool.is_some()))
            .or_else(|| lines.iter().position(Line::selectable))
            .unwrap_or(0);
        if let Some(rv) = &mut self.reviewing {
            rv.sel = at;
        }
        // Where HEAD is, looked at again, and whether the checkout is clean:
        // HEAD in sync with the stack may follow it as the review closes.
        self.look_at_head();
    }

    /// The stack as it is, and as accepting the change would make it.
    pub(super) fn review_stacks(&self) -> Option<(Stack, Stack)> {
        let rv = self.reviewing.as_ref()?;
        let (r, h) = self.work(&rv.run)?;
        let c = h.stack.as_ref()?;
        let now = self.db.stack(&r.stack).ok()?.clone();
        let next = Stack {
            patches: c.patches.clone(),
            base: c.base.clone().or(now.base.clone()),
            ..now.clone()
        };
        Some((now, next))
    }

    /// The operations of the change reviewed, from the stack as it is, as
    /// the review marks them (`operations::of_review`): each on its own, as
    /// what each needs is stored with the work; else the change as one.
    /// None for work that changes nothing of the stack.
    pub(super) fn review_ops(&self) -> Option<Ops> {
        let rv = self.reviewing.as_ref()?;
        let (r, h) = self.work(&rv.run)?;
        crate::operations::of_review(&self.db, r, h)
    }

    /// What each operation of the change reviewed needs, as stored when the
    /// work landed (`Held::needs`): what marking each on its own is decided
    /// from, at once, with no check.
    fn review_needs(&self) -> Option<&crate::operations::Needs> {
        let rv = self.reviewing.as_ref()?;
        self.work(&rv.run)?.1.needs.as_ref()
    }

    /// Why the change reviewed is marked as a whole, though it has
    /// operations to mark (`operations::whole_why`), as the right pane and
    /// `?` say it: none when each is marked on its own.
    pub(super) fn review_whole_why(&self) -> Option<String> {
        let rv = self.reviewing.as_ref()?;
        let (r, h) = self.work(&rv.run)?;
        crate::operations::whole_why(&self.db, r, h)
    }

    /// What the work of run `r`, a pull's, takes from Phabricator for the
    /// patch that line `d` of the stack diff to `next` is of, said after its
    /// line, if it takes something.
    fn taken_line(&self, r: &Run, next: &Stack, d: &Diffed) -> Option<String> {
        let Diffed::Patch { at, .. } = d else { return None };
        let t = self.op_taken(r, next, *at)?;
        Some(t.said(self.resolved(next, *at))).filter(|s| !s.is_empty())
    }

    /// Whether patch `at` (from 1) of `next`, the stack a pull makes, holds
    /// no conflict the pull marked: none was, or they were resolved by hand
    /// since.
    fn resolved(&self, next: &Stack, at: usize) -> bool {
        next.patches.get(at.wrapping_sub(1)).is_some_and(|id| !crate::held::marked(self.db.patch(id)))
    }

    /// What the work of run `r`, a pull's, takes from Phabricator for patch
    /// `at` (from 1) of `next`, the stack it makes, if it takes something.
    fn op_taken<'a>(&self, r: &'a Run, next: &Stack, at: usize) -> Option<&'a crate::held::Taken> {
        let id = next.patches.get(at.checked_sub(1)?)?;
        crate::held::taken(r, &self.db.patch(id).change)
    }

    /// Of a pull's new version `t` that takes Phabricator's for a patch
    /// changed in the stack since, what the patch, as it is in `now`, had
    /// since that version, which the pulled one lacks: the interdiff from
    /// that version to the patch as it is, `w` wide, under a sentence's
    /// lines; side by side, under a heading of the two versions. None
    /// without it.
    fn since_lines(&self, now: &Stack, t: &crate::held::Taken, w: usize) -> Vec<Vec<(String, Style)>> {
        let Some(v) = t.since else { return vec![] };
        let versions = self.db.versions(&t.change);
        let (Some(was), Some(mine)) = (versions.get(v.saturating_sub(1)), now.patches.iter().map(|id| self.db.patch(id)).find(|p| p.change == t.change)) else {
            return vec![];
        };
        let lines = crate::ops::interdiff_lines(was, mine);
        let refs: Vec<&crate::ops::InterLine> = lines.iter().filter(|l| !l.text.starts_with("--- ") && !l.text.starts_with("+++ ")).collect();
        let mut out = vec![vec![(String::new(), Style::new())], vec![(format!("      version {v}, then this version, as the stack has it:"), color::file())]];
        let version = format!("version {v}");
        let two = interdiffs::Two { old: &version, new: "this version", one_base: crate::ops::one_base(&lines, &was.diff, &mine.diff) };
        let view = self.inter_in(self.inter_w.get());
        // The change: version `v` on the stack that first held it, this one
        // on the stack as it is.
        let change = view.change().then(|| {
            let near = super::change::Near::new(&crate::ops::code_places(&refs, was, mine), 0, 0);
            let at = now.patches.iter().position(|id| *id == mine.id).unwrap_or_default();
            (self.changed(self.on_first(was, &version), self.on_stack(now, at, "this version"), [&version, "this version"]), near)
        });
        let stops = interdiffs::view_stops(&refs, view, &two, change.as_ref(), (self.change_whole, self.only_changed()), w.saturating_sub(1), 6);
        out.extend(stops.iter().flat_map(|s| &s.lines).flat_map(|parts| fold_parts(parts, w.saturating_sub(1), 7)));
        out
    }

    /// The operation of the review's line `line`, of `ops`: one of the stack
    /// operations' lines but their title.
    fn line_op<'a>(ops: &'a Ops, line: &Line) -> Option<&'a crate::operations::Op> {
        line.part.filter(|p| *p == Part::Stack).and(line.entry).and_then(|e| ops.by_entry(e))
    }

    /// The operation of the review's line the cursor is on, if it is one:
    /// its key, and whether it is marked (not a patch left as it was).
    pub(super) fn review_op(&self) -> Option<(String, bool)> {
        let rv = self.reviewing.as_ref()?;
        let ops = self.review_ops()?;
        let o = Self::line_op(&ops, self.review_lines().get(rv.sel)?)?;
        Some((o.key.clone(), o.marked()))
    }

    /// Whether `marks` reject the change of the stack: every one of its
    /// operations, the comments of the work with it.
    fn change_rejected(&self, marks: &BTreeMap<String, bool>) -> bool {
        self.review_ops().is_some_and(|ops| matches!(ops.tally(marks), (0, n, 0) if n > 0))
    }

    /// Whether closing the review, the parts not marked yet as `rest`
    /// says, accepts the change of the stack, wholly or in part: an
    /// operation of it is accepted.
    fn change_taken(&self, marks: &BTreeMap<String, bool>, rest: Option<bool>) -> bool {
        self.review_ops().is_some_and(|ops| {
            let (yes, _, open) = ops.tally(marks);
            yes > 0 || (open > 0 && rest == Some(true))
        })
    }

    /// Whether closing the review, the parts not marked yet as `rest` says,
    /// accepts the change of the stack in part: some of its operations, not
    /// all.
    fn change_in_part(&self, marks: &BTreeMap<String, bool>, rest: Option<bool>) -> bool {
        self.review_ops().is_some_and(|ops| {
            let (yes, no, open) = ops.tally(marks);
            let (yes, no) = match rest {
                Some(true) => (yes + open, no),
                Some(false) => (yes, no + open),
                None => (yes, no),
            };
            yes > 0 && no > 0
        })
    }

    /// Whether part `p` of what is reviewed is marked, as `marks` mark it:
    /// the change of the stack once each of its operations is.
    fn part_marked(&self, marks: &BTreeMap<String, bool>, p: Part) -> bool {
        match (p, self.review_ops()) {
            (Part::Stack, Some(ops)) => ops.tally(marks).2 == 0,
            _ => marks.contains_key(&p.key()),
        }
    }

    /// The comments the ask of what is reviewed was about, still open, each
    /// with its place among them.
    fn asking_open(&self) -> Vec<(usize, Note)> {
        let Some((r, _)) = self.reviewing.as_ref().and_then(|rv| self.work(&rv.run)) else { return vec![] };
        crate::held::asking_open(&self.db, r).into_iter().map(|(k, n)| (k, n.clone())).collect()
    }

    /// Whether the work reviewed is a pull's, whose comments are all
    /// accepted: none of the comments it pulled is rejected. Its change of
    /// the stack's operations are marked as any change's.
    pub(super) fn review_pulled(&self) -> bool {
        self.reviewing.as_ref().and_then(|rv| self.db.run(&rv.run)).is_some_and(crate::held::pulled)
    }

    /// Whether part `p` of the work reviewed is accepted as it is, with
    /// nothing to mark: a comment of a pull's.
    pub(super) fn part_fixed(&self, p: Part) -> bool {
        p != Part::Stack && self.review_pulled()
    }

    /// The patches the stack would hold as the review of a pull leaves it,
    /// the operations of its change marked as `marks` mark them, those not
    /// marked as `rest` says (to review, with none), when one of them holds
    /// a conflict the pull marked: why the review does not close so, said.
    /// Its change marked as a whole, the patches it makes, once it is
    /// accepted.
    fn marked_closing(&self, marks: &BTreeMap<String, bool>, rest: Option<bool>) -> Option<String> {
        if !self.review_pulled() {
            return None;
        }
        let ops = self.review_ops()?;
        let patches: Vec<String> = match ops.whole() {
            true => {
                let rv = self.reviewing.as_ref()?;
                if marks.get(&Part::Stack.key()).copied().or(rest) != Some(true) {
                    return None;
                }
                self.work(&rv.run)?.1.stack.as_ref()?.patches.clone()
            }
            false => {
                let mut marks = marks.clone();
                if let Some(m) = rest {
                    for o in ops.marked() {
                        marks.entry(o.key.clone()).or_insert(m);
                    }
                }
                let (left, _) = ops.leave(&ops.standing(&marks));
                left.into_iter().map(|p| p.id).collect()
            }
        };
        crate::held::marked_said(&self.db, &patches)
    }

    /// Whether the work reviewed is the agent's, to send back to it with
    /// what you write in its review: not a pull's, nor an edit by hand's.
    pub(super) fn review_writable(&self) -> bool {
        self.reviewing.as_ref().and_then(|rv| self.db.run(&rv.run)).is_some_and(|r| crate::ask::info(r).is_some() && !crate::held::pulled(r))
    }

    /// Whether the right pane shows the stack diff of the change: the
    /// list's cursor on the stack operations.
    pub(super) fn review_on_diff(&self) -> bool {
        let Some(rv) = &self.reviewing else { return false };
        self.review_lines()
            .get(rv.sel)
            .is_some_and(|l| {
                l.asking.is_none()
                    && l.yours.is_none()
                    && l.report.is_none()
                    && !l.stacked
                    && l.tool.is_none()
                    && l.answer.is_none()
                    && l.following.is_none()
                    && l.found.is_none()
                    && matches!(l.part, Some(Part::Stack) | None)
            })
            && self.review_stacks().is_some()
    }

    /// The stack diff of the change, `w` wide, as the right pane shows it,
    /// its interdiffs in the view the pane draws them in, as last drawn.
    fn review_diff(&self, w: usize) -> Option<DiffView> {
        let (now, next) = self.review_stacks()?;
        let (_, h) = self.reviewing.as_ref().and_then(|rv| self.work(&rv.run))?;
        let diff = crate::history::stacks_diff(&self.db, Some(&now), Some(&next), true);
        let of = h.stack.as_ref().map_or(String::new(), |c| c.of.clone());
        let mut v = DiffView {
            said: format!(" the change of {of}: what accepting it does to the stack"),
            ..Default::default()
        };
        // Where Enter on each line of the diff goes: the patch the lines of
        // code under it are of, by its entry among the stack operations,
        // which have every entry but those lines.
        let mut goes: Vec<Option<(usize, Option<usize>)>> = Vec::new();
        let mut under = None;
        for d in &diff {
            if !matches!(d, Diffed::Code(_)) {
                under = matches!(d, Diffed::Patch { .. }).then_some(v.of_entry.len());
                v.of_entry.push(goes.len());
            }
            goes.push(under.map(|e| (e, None)));
        }
        // Each interdiff's lines: where each is in its patch's mail.
        let mut k = 0;
        while k < diff.len() {
            let start = k;
            while let Some(Diffed::Code(_)) = diff.get(k) {
                k += 1;
            }
            if k == start {
                k += 1;
                continue;
            }
            let code: Vec<&str> = diff[start..k].iter().map(|d| if let Diffed::Code(l) = d { l.text.as_str() } else { "" }).collect();
            for (j, at) in crate::ops::interdiff_places(&code).into_iter().enumerate() {
                if let Some((e, _)) = goes[start + j] {
                    goes[start + j] = Some((e, Some(at)));
                }
            }
        }
        // Each stop, folded to the pane; a heading drawn, and no stop. In a
        // view of the change, each patch's, as the stack has it and as the
        // work leaves it.
        let view = self.inter_in(self.inter_w.get());
        let changes = self.stack_changes(&diff, (Some(&now), Some(&next)), [AS_IT_IS.0, AS_IT_IS.1], view);
        // A line of what changed taken out or put in; drawn with only what
        // the patches changed, not a hunk's header, which stands there for
        // the lines under it, as both versions have it.
        let only = self.only_changed();
        let changed = |k: &usize| matches!(&diff[*k], Diffed::Code(l) if (l.text.starts_with('+') || l.text.starts_with('-')) && !(only && l.part == crate::ops::Part::Hunk));
        let mut ordinal: HashMap<usize, usize> = HashMap::new();
        for stop in interdiffs::stack_stops(&diff, view, AS_IT_IS, w.saturating_sub(1), &changes, (self.change_whole, only)) {
            let start = v.lines.len();
            v.lines.extend(stop.lines.iter().flat_map(|parts| fold_parts(parts, w.saturating_sub(1), 6)));
            let (Some(&first), Some(&last)) = (stop.shows.first(), stop.shows.last()) else { continue };
            v.spans.push(start..v.lines.len());
            v.goes.push(goes[last]);
            let key = match stop.slot {
                Some(slot) => {
                    let n = ordinal.entry(slot).or_default();
                    *n += 1;
                    CHANGE_KEYS.saturating_add(slot.saturating_mul(1 << 20)).saturating_add(*n)
                }
                None => first,
            };
            v.keys.push(key);
            v.code.push(stop.slot.is_some() || stop.shows.iter().all(|&k| matches!(diff[k], Diffed::Code(_))));
            v.changed.push(if stop.slot.is_some() { stop.changed } else { stop.shows.iter().any(changed) });
            v.seen.push(interdiffs::Seen::from(&stop));
        }
        v.diff = diff;
        Some(v)
    }

    /// In the stack diff looked at closer, where the cursor is
    /// (`interdiffs::At`), and how far under the right pane's top the first
    /// line of its stop is, as last drawn: for `review_found` to put it
    /// there again once the stack diff is drawn otherwise. None elsewhere.
    pub(super) fn review_place(&self) -> Option<(interdiffs::At, usize)> {
        let rv = self.reviewing.as_ref().filter(|rv| self.screen == Screen::Review && rv.in_main && !rv.in_patch)?;
        let v = self.review_diff(self.inter_w.get().saturating_sub(1)).filter(|_| self.review_on_diff())?;
        let i = v.stop_of(rv.entry);
        let kept = self.review_kept.filter(|k| k.0 == rv.entry).map(|(_, at, ..)| (i, at));
        let at = interdiffs::locate(&v.seen, i, kept)?;
        Some((at, v.spans.get(i)?.start.saturating_sub(rv.scroll)))
    }

    /// The cursor of the stack diff looked at closer on place `kept`
    /// again, drawn otherwise, the first line of its stop as far under the
    /// right pane's top as it was (`review_place`); kept, for a switch back
    /// to find the place it came from.
    pub(super) fn review_found(&mut self, kept: Option<(interdiffs::At, usize)>) {
        let Some((at, below)) = kept else { return };
        let Some(v) = self.review_diff(self.inter_w.get().saturating_sub(1)) else { return };
        let i = interdiffs::find(&v.seen, at);
        let ended = self.changes.ended();
        if let Some(rv) = self.reviewing.as_mut() {
            rv.entry = v.line_of(i);
            if let Some(s) = v.spans.get(i) {
                rv.scroll = s.start.saturating_sub(below);
            }
            self.review_kept = Some((rv.entry, at, below, ended));
        }
    }

    /// The review's stack diff drawn again as a change of it was read
    /// since a switch of view: the cursor on the place it came with, again
    /// (`review_found`), as long as it was not moved since.
    pub(super) fn review_found_again(&mut self) {
        let Some((entry, at, below, _)) = self.review_kept.filter(|k| k.3 != self.changes.ended()) else { return };
        match self.reviewing.as_ref().is_some_and(|rv| rv.entry == entry) {
            true => self.review_found(Some((at, below))),
            false => self.review_kept = None,
        }
    }

    /// The patch of the stack the work makes that the list's line the cursor
    /// is on is about, with its place, if it is one of its patches.
    pub(super) fn review_patch(&self) -> Option<(usize, Patch)> {
        let rv = self.reviewing.as_ref()?;
        let entry = self.review_lines().get(rv.sel)?.entry?;
        self.entry_patch(entry)
    }

    /// The patch of the stack the work makes that entry `entry` of the stack
    /// operations is, with its place: none for the base, or a patch dropped.
    fn entry_patch(&self, entry: usize) -> Option<(usize, Patch)> {
        let (now, next) = self.review_stacks()?;
        match crate::history::stacks_diff(&self.db, Some(&now), Some(&next), false).get(entry)? {
            Diffed::Patch { at, .. } => Some((at - 1, self.db.patch(next.patches.get(at - 1)?).clone())),
            _ => None,
        }
    }

    /// On a patch of the stack the work makes looked at closer: its place,
    /// the patch, and the row the cursor is on (until it is drawn, the one
    /// Enter went to from the stack diff).
    pub(super) fn review_row(&self) -> Option<(usize, Patch, Row)> {
        let rv = self.reviewing.as_ref().filter(|rv| rv.in_main && rv.in_patch)?;
        let (at, p) = self.review_patch()?;
        let rows = self.review_rows(&p, rv.main_w);
        let i = match rv.goto {
            Some(m) => row_of(&rows, crate::ops::line_of_mail(&p, m)),
            None => rv.row.min(rows.len().saturating_sub(1)),
        };
        let row = rows.into_iter().nth(i)?;
        Some((at, p, row))
    }

    /// The line of a commit message the review's cursor is on, as it is
    /// written: on a patch looked at closer, a line of its message; on the
    /// stack diff looked at closer, a line of the message part of an
    /// interdiff (`message_line_of`), side by side one of each version's,
    /// each once. None elsewhere.
    pub(super) fn review_message_lines(&self) -> Vec<String> {
        let Some(rv) = self.reviewing.as_ref().filter(|rv| rv.in_main) else { return Vec::new() };
        if rv.in_patch {
            let line = self.review_row().and_then(|(_, p, row)| {
                let at = row.msg_at.filter(|_| row.kind == Kind::Msg && row.note.is_none())?;
                p.message.split('\n').nth(at).map(String::from)
            });
            return line.into_iter().collect();
        }
        let (Some((_, next)), Some(v)) = (self.review_stacks(), self.review_diff(80).filter(|_| self.review_on_diff())) else { return Vec::new() };
        let mut out: Vec<String> = Vec::new();
        for &k in v.seen.get(v.stop_of(rv.entry)).into_iter().flat_map(|s| &s.shows) {
            if let Some(line) = message_line_of(&self.db, &v.diff, &next, k).filter(|l| !out.contains(l)) {
                out.push(line);
            }
        }
        out
    }

    /// The comment of its own that Enter writes on row `row` of patch `p`,
    /// looked at closer: on a line, or a file's name, that has no comment of
    /// the work's nor of yours.
    fn line_comment(&self, p: &Patch, row: &Row) -> Option<Target> {
        let key = row.key.clone().filter(|_| row.note.is_none() && row.kind.commentable())?;
        if self.db.note(&p.change, &key).is_some_and(|n| self.review_shows(n)) {
            return None;
        }
        let (file, preview) = (row.path.clone().unwrap_or_default(), row.preview.clone().unwrap_or_default());
        Some(Target::Line {
            change: p.change.clone(),
            key,
            file,
            preview,
        })
    }

    /// The comment of its own that Enter writes where the cursor is, if it
    /// writes one there: on the row of a patch looked at closer, in the
    /// review of the agent's work.
    pub(super) fn review_line_comment(&self) -> Option<Target> {
        if !self.review_writable() {
            return None;
        }
        let (_, p, row) = self.review_row()?;
        self.line_comment(&p, &row)
    }

    /// Where the review's cursor is in the stack the work makes, for an edit
    /// by hand from there. On a patch looked at closer, the row it is on. On
    /// the stack diff, where Enter goes from the line the cursor is on: from
    /// a line of what changed in a patch, that line as the work leaves the
    /// patch; from the patch's own line, the patch alone. Else (on the list,
    /// or on a line of the stack diff of no patch: the base, a patch
    /// dropped) the patch of the list's line, alone.
    fn work_place(&self) -> Option<WorkPlace> {
        let rv = self.reviewing.as_ref()?;
        if let Some((at, p, row)) = self.review_row() {
            return Some(WorkPlace::of_row(at, &p, Some(&row)));
        }
        if rv.in_main
            && !rv.in_patch
            && self.review_on_diff()
            && let Some(v) = self.review_diff(80)
            && let Some((entry, m)) = v.goes.get(v.stop_of(rv.entry)).copied().flatten()
            && let Some((at, p)) = self.entry_patch(entry)
        {
            let row = m.and_then(|m| {
                let rows = self.review_rows(&p, rv.main_w);
                let i = row_of(&rows, crate::ops::line_of_mail(&p, m));
                rows.into_iter().nth(i)
            });
            return Some(WorkPlace::of_row(at, &p, row.as_ref()));
        }
        self.review_patch().map(|(at, p)| WorkPlace::of_row(at, &p, None))
    }

    /// Patch `p` as the work leaves it, a row at a time, `w` wide, the
    /// comments of the work on its lines under them, and no other comment of
    /// the patch: those the ask was about, with the work's reply and your
    /// answer to it, as their threads would be; those the work opened, where
    /// they would be; those you wrote in its review. The turn written again
    /// in the editor in the editor alone.
    fn review_rows(&self, p: &Patch, w: usize) -> Vec<Row> {
        let mut notes: Vec<Note> = Vec::new();
        if let Some((r, h)) = self.reviewing.as_ref().and_then(|rv| self.work(&rv.run)) {
            let taken = self.db.review(&r.id).map(|v| v.taken.clone()).unwrap_or_default();
            notes = self
                .db
                .notes(&p.change)
                .into_iter()
                .filter(|n| self.review_shows(n))
                .map(|n| Note {
                    text: h.thread_but(&n.change, &n.id, &n.text, &taken),
                    ..n.clone()
                })
                .collect();
            notes.extend(h.opened.iter().filter(|n| n.change == p.change).cloned());
        }
        let mut deco = Deco {
            pointed: self.pointed_of(&notes),
            ages: self.ages_of(&notes),
            ..Default::default()
        };
        if let Some((id, turns)) = self.written_turns() {
            deco.writing.insert(id, turns);
        }
        patch_rows(p, &notes.iter().collect::<Vec<_>>(), w, &deco)
    }

    /// Whether comment `n` is one of the work reviewed: one its ask was
    /// about, one you wrote in its review, whoever opened its thread, or one
    /// a follow-up asked from the review was asked about.
    pub(super) fn review_shows(&self, n: &Note) -> bool {
        let Some((r, h)) = self.reviewing.as_ref().and_then(|rv| self.work(&rv.run)) else { return false };
        let at = (n.change.clone(), n.id.clone());
        crate::held::asking_of(r).contains(&at) || n.review.as_deref() == Some(r.id.as_str()) || h.followed().contains(&at)
    }

    /// What you wrote in the review, counted: the comments written there,
    /// and your answers to the work's replies.
    pub(super) fn review_yours(&self) -> (usize, usize) {
        let Some((r, _)) = self.reviewing.as_ref().and_then(|rv| self.work(&rv.run)) else { return (0, 0) };
        let (opened, answers) = crate::held::yours_in(&self.db, r);
        (opened.len(), answers)
    }

    /// Enter on the row the cursor is on, a patch looked at closer, as on a
    /// line of the main view: a comment on its line; on a comment, what you
    /// said last written again if the last word is yours, else a reply; on
    /// one the work replied to, your answer to its reply.
    fn review_comment(&mut self) {
        if !self.review_writable() {
            self.status = "only the agent's work goes back to the agent with your comments: this work is not the agent's".into();
            return;
        }
        let Some(rv) = self.reviewing.clone() else { return };
        let Some((_, p, row)) = self.review_row() else { return };
        let line = row.key.clone().filter(|_| row.note.is_none() && row.kind.commentable());
        // The comment on the row, or yours on its line if it is one of the
        // work's; else a comment of its own there.
        let Some(id) = row.note.clone().or_else(|| line.filter(|k| self.db.note(&p.change, k).is_some_and(|n| self.review_shows(n)))) else {
            if let Some(target) = self.line_comment(&p, &row) {
                self.write(target, Focus::Diff, "");
            }
            return;
        };
        // Your answer to the last reply to it, a follow-up's if one replied.
        let replied = self.work(&rv.run).and_then(|(_, h)| h.replies_to(&p.change, &id).last().copied());
        match (replied, self.db.note(&p.change, &id).map(|n| last_mine(&n.text).is_some())) {
            (Some(k), _) => self.review_answer(k),
            (None, Some(true)) => self.edit_mine(p.change, id, Focus::Diff),
            (None, Some(false)) => self.write(Target::Reply { change: p.change, id }, Focus::Diff, ""),
            // Not written yet: one the work opened.
            (None, None) => {
                self.status = "the work opened this comment: it is accepted or rejected in the list, with the comments the work opened".into();
            }
        }
    }

    /// Your answer to reply `k` of the work, written in the review: again,
    /// if you wrote one.
    pub(super) fn review_answer(&mut self, k: usize) {
        let Some(run) = self.reviewing.as_ref().map(|rv| rv.run.clone()) else { return };
        // Not to a reply a later one follows, nor while the follow-up you
        // asked works on its comment.
        if let Some((_, h)) = self.work(&run)
            && let Some(rp) = h.replies.get(k)
        {
            if !h.last_reply(k) {
                self.status = crate::held::LATER_REPLY.into();
                return;
            }
            if ask::following(&self.db, &run).contains(&(rp.change.clone(), rp.id.clone())) {
                self.status = crate::held::FOLLOWING.into();
                return;
            }
        }
        let was = self.work(&run).and_then(|(_, h)| h.replies.get(k)?.answer.clone()).unwrap_or_default();
        self.write(Target::Answer { run, k }, Focus::Diff, &was);
    }

    /// Your answer to why the agent selected task `k` of the try push work,
    /// written in the review: again, if you wrote one. The cursor goes on
    /// that task, for its comment to show where the answer is written.
    #[cfg(feature = "try-push")]
    pub(super) fn review_task_answer(&mut self, k: usize) {
        let Some(run) = self.reviewing.as_ref().map(|rv| rv.run.clone()) else { return };
        let was = self.work(&run).and_then(|(_, h)| h.tasks.get(k)?.answer.clone()).unwrap_or_default();
        if let Some(at) = self.review_lines().iter().position(|l| l.part == Some(Part::Task(k)))
            && let Some(rv) = self.reviewing.as_mut()
        {
            (rv.sel, rv.in_main, rv.in_patch) = (at, false, false);
        }
        self.write(Target::TaskAnswer { run, k }, Focus::Diff, &was);
    }

    /// What you wrote in the review on the list's line the cursor is on, if
    /// it is.
    pub(super) fn review_yours_line(&self) -> Option<Yours> {
        let rv = self.reviewing.as_ref()?;
        self.review_lines().get(rv.sel)?.yours.clone()
    }

    /// Your answer to what the agent said of tool `k` it could not use,
    /// written in the review: again, if you wrote one. The cursor goes on
    /// that tool, for its comment to show where the answer is written.
    pub(super) fn review_tool_answer(&mut self, k: usize) {
        let Some(run) = self.reviewing.as_ref().map(|rv| rv.run.clone()) else { return };
        let was = self.work(&run).and_then(|(_, h)| h.tools.get(k)?.answer.clone()).unwrap_or_default();
        if let Some(at) = self.review_lines().iter().position(|l| l.tool == Some(k))
            && let Some(rv) = self.reviewing.as_mut()
        {
            (rv.sel, rv.in_main, rv.in_patch) = (at, false, false);
        }
        self.write(Target::ToolAnswer { run, k }, Focus::Diff, &was);
    }

    /// What you wrote in the review, `y`, written again: what you said last
    /// in your comment, or your answer to the work's reply.
    pub(super) fn review_write_yours(&mut self, y: Yours) {
        match y {
            Yours::Comment(change, id) => self.edit_mine(change, id, Focus::Diff),
            Yours::Answer(k) => self.review_answer(k),
            #[cfg(feature = "try-push")]
            Yours::TaskAnswer(k) => self.review_task_answer(k),
            Yours::ToolAnswer(k) => self.review_tool_answer(k),
        }
    }

    /// What you wrote in the review, `y`, taken back: your comment closed, a
    /// thread you wrote in there as it was before the review, or your answer
    /// to the work's reply gone. It does not go back to the agent with the
    /// work; the cursor stays on a line still there.
    pub(super) fn review_take_back(&mut self, y: Yours) {
        let Some(run) = self.reviewing.as_ref().map(|rv| rv.run.clone()) else { return };
        let done = match y {
            Yours::Comment(change, id) => self.edit(|db| crate::held::take_back(db, &run, &change, &id)),
            Yours::Answer(k) => self.edit(|db| crate::held::answer(db, &run, k, "")).map(|_| false),
            #[cfg(feature = "try-push")]
            Yours::TaskAnswer(k) => self.edit(|db| crate::held::answer_task(db, &run, k, "")).map(|_| false),
            Yours::ToolAnswer(k) => self.edit(|db| crate::held::answer_tool(db, &run, k, "")).map(|_| false),
        };
        let Some(before) = done else { return };
        self.status = match before {
            true => "taken back: the comment is as it was before this review, and does not go back to the agent with the work".into(),
            false => "taken back: it does not go back to the agent with the work".into(),
        };
        let lines = self.review_lines();
        if let Some(rv) = self.reviewing.as_mut()
            && !lines.get(rv.sel).is_some_and(Line::selectable)
        {
            let before = lines.iter().enumerate().take(rv.sel + 1).rev().find(|(_, l)| l.selectable()).map(|(i, _)| i);
            rv.sel = before.or_else(|| lines.iter().position(Line::selectable)).unwrap_or(0);
        }
    }

    /// What Delete asks first on what you wrote in the review: taking it
    /// back, which its menu's item does at once.
    pub(super) fn review_take_back_ask(&self) -> Option<Menu> {
        // A comment the ask was about is a thread from before the review:
        // what you wrote in it there alone is taken back.
        let asked = |c: &str, i: &str| {
            let r = self.reviewing.as_ref().and_then(|rv| self.db.run(&rv.run));
            r.is_some_and(|r| crate::held::asking_of(r).iter().any(|(ac, ai)| ac == c && ai == i))
        };
        let (title, yes) = match self.review_yours_line()? {
            Yours::Comment(c, i) if asked(&c, &i) => (
                "take back what you wrote on this comment in this review? the comment is as it was before the review, and does not go back to the agent with the work",
                "Take it back",
            ),
            Yours::Comment(..) => ("take your comment back? it does not go back to the agent with the work", "Take it back"),
            Yours::Answer(_) => ("take your answer back? it does not go back to the agent with the work", "Take your answer back"),
            #[cfg(feature = "try-push")]
            Yours::TaskAnswer(_) => ("take your answer back? it does not go back to the agent with the work", "Take your answer back"),
            Yours::ToolAnswer(_) => ("take your answer back? it does not go back to the agent with the work", "Take your answer back"),
        };
        Some(Menu::ask(title, Item::new(yes, Action::TakeYoursBack), "Keep it", true))
    }

    /// What Delete does on the review's line the cursor is on, as its menu
    /// has it: what you wrote taken back, once asked; a comment the ask was
    /// about closed with the review; a part rejected. Nothing where that is
    /// so already, as the status line says.
    pub(super) fn review_deleting(&self) -> Option<std::result::Result<Action, String>> {
        let rv = self.reviewing.as_ref()?;
        let lines = self.review_lines();
        let line = lines.get(rv.sel)?;
        if line.yours.is_some() {
            return Some(Ok(Action::AskTakeYoursBack));
        }
        if let Some(k) = line.asking {
            return Some(match rv.marks.get(&crate::held::asking_key(k)) == Some(&true) {
                true => Err("this comment closes with the review already".into()),
                false => Ok(Action::CloseAsking(k, true)),
            });
        }
        // A tool the agent could not use: refused, unless it is, or is
        // noted, not offered.
        if let Some(k) = line.tool {
            let t = self.tool_item(k)?;
            return Some(match (&t.noted, crate::refusals::state(&rv.marks, k)) {
                (Some(_), _) => Err("this tool is noted, not offered: it stays refused".into()),
                (None, Allow::Refused) => Err("this tool is refused already".into()),
                _ => Ok(Action::ToolState(k, Allow::Refused)),
            });
        }
        // An answer made on the stack before the change: stopped, unless it
        // is marked so, or has nothing to mark.
        if let Some(id) = &line.answer {
            let going = self.answer_items().into_iter().find(|(k, _)| k == id).map_or(Going::Reviewed, |(_, g)| g);
            return Some(match (going.unmarkable(), crate::held::answer_mark(&rv.marks, id)) {
                (Some(why), _) => Err(why.into()),
                (None, Some(AnswerMark::Stop)) => Err("this answer is marked stop already".into()),
                (None, _) => Ok(Action::MarkAnswer(id.clone(), AnswerMark::Stop)),
            });
        }
        let p = line.part?;
        if self.part_fixed(p) {
            return Some(Err(crate::held::FILED.into()));
        }
        // An operation of the change, by its own mark; their title, the
        // change as a whole, every one rejected or not.
        if p == Part::Stack
            && let Some(ops) = self.review_ops()
        {
            return Some(match Self::line_op(&ops, line) {
                Some(o) if !o.marked() => Err(NOTHING_TO_MARK.into()),
                Some(o) if ops.mark(&rv.marks, o) == Some(false) => Err("this operation is rejected already".into()),
                Some(_) => Ok(Action::MarkPart(false)),
                None => match ops.tally(&rv.marks) {
                    (0, _, 0) => Err("this part is rejected already".into()),
                    _ => Ok(Action::MarkPart(false)),
                },
            });
        }
        Some(match rv.marks.get(&p.key()) {
            Some(false) => Err("this part is rejected already".into()),
            _ => Ok(Action::MarkPart(false)),
        })
    }

    /// The review of what run `run` brought back, as it was saved: its
    /// marks, the cursor on its first line.
    fn review_of(&self, run: &str) -> Reviewing {
        Reviewing {
            run: run.into(),
            marks: self.db.run(run).map(|r| crate::held::marks(&self.db, r)).unwrap_or_default(),
            local: self.local_of(run),
            answers: self.answers_of(run),
            ..Default::default()
        }
    }

    /// The answers made on the stack before the change of the work run
    /// `run` holds, that accepting it leaves behind, by their runs, as the
    /// review finds them as it opens: running, or landed and waiting for
    /// your review (`ask::answers_left`). None for work that does not change
    /// the stack, nor unchained.
    fn answers_of(&self, run: &str) -> Vec<String> {
        match self.work(run) {
            Some((r, h)) if h.stack.is_some() => ask::answers_left(&self.db, &r.stack, run).into_iter().map(|a| a.id).collect(),
            _ => vec![],
        }
    }

    /// The answers of the review `rv`, made on the stack before the change,
    /// each with where it stands now: running or waiting for your review, to
    /// mark; else failed, or reviewed, since the review opened.
    fn answers_in(&self, rv: &Reviewing) -> Vec<(String, Going)> {
        if rv.answers.is_empty() {
            return vec![];
        }
        let live: HashMap<&str, &Run> = self.db.live_runs().into_iter().map(|r| (r.id.as_str(), r)).collect();
        rv.answers
            .iter()
            .map(|id| {
                let going = match live.get(id.as_str()) {
                    Some(r) if r.state == RunState::Started => Going::Running,
                    Some(_) => Going::Waiting,
                    // No longer either: its run as it ended, read again.
                    None if self.db.run(id).is_some_and(|r| r.state == RunState::Failed) => Going::Failed,
                    None => Going::Reviewed,
                };
                (id.clone(), going)
            })
            .collect()
    }

    /// The answers of the review, made on the stack before the change, each
    /// with where it stands now (`answers_in`).
    pub(super) fn answer_items(&self) -> Vec<(String, Going)> {
        self.reviewing.as_ref().map(|rv| self.answers_in(rv)).unwrap_or_default()
    }

    /// The answers of review `rv`, made on the stack before the change,
    /// running or waiting for your review, that it does not mark yet: the
    /// review does not close with the change accepted while one is.
    fn unmarked_in(&self, rv: &Reviewing) -> Vec<String> {
        self.answers_in(rv).into_iter().filter(|(id, g)| g.markable() && crate::held::answer_mark(&rv.marks, id).is_none()).map(|(id, _)| id).collect()
    }

    /// Whether closing the review, the parts not marked yet as `rest` says,
    /// accepts its change while an answer made on the stack before it is not
    /// marked: then it does not close.
    fn waits_for_answers(&self, rest: Option<bool>) -> bool {
        let Some(rv) = &self.reviewing else { return false };
        self.change_taken(&rv.marks, rest) && !self.unmarked_in(rv).is_empty()
    }

    /// The answer of run `id`, made on the stack before the change, marked
    /// `m` in the review, for when the change is accepted: the status line
    /// says what that does; or, no longer running nor waiting for your
    /// review, why nothing is marked.
    pub(super) fn mark_answer(&mut self, id: &str, m: AnswerMark) {
        let Some(going) = self.answer_items().into_iter().find(|(a, _)| a == id).map(|(_, g)| g) else { return };
        if let Some(why) = going.unmarkable() {
            self.status = why.into();
            return;
        }
        if let Some(rv) = self.reviewing.as_mut() {
            crate::held::set_answer_mark(&mut rv.marks, id, m);
        }
        self.status = format!("marked {}: once the change is accepted, {}", m.word(), answer_does(m, going));
    }

    /// Every answer made on the stack before the change, running or waiting
    /// for your review, marked `m`, as the review's menu marks them all.
    pub(super) fn mark_answers(&mut self, m: AnswerMark) {
        let items: Vec<String> = self.answer_items().into_iter().filter(|(_, g)| g.markable()).map(|(id, _)| id).collect();
        let n = items.len();
        if let Some(rv) = self.reviewing.as_mut() {
            for id in &items {
                crate::held::set_answer_mark(&mut rv.marks, id, m);
            }
        }
        self.status = match n {
            0 => "no answer made on the stack before the change is running nor waiting for your review: nothing to mark".into(),
            1 => format!("the answer made on the stack before the change is marked {}", m.word()),
            n => format!("the {n} answers made on the stack before the change are marked {}", m.word()),
        };
    }

    /// Where the review of what run `run` brought back saves what it allows
    /// from now on, or why it cannot: looked at only when that work holds a
    /// tool the agent could not use.
    fn local_of(&self, run: &str) -> Option<std::result::Result<PathBuf, String>> {
        let (r, h) = self.work(run)?;
        h.offers_tools().then(|| crate::refusals::local_of(&self.db, r))
    }

    /// Where the review saves what it allows from now on, or why it cannot.
    pub(super) fn tool_local(&self) -> std::result::Result<PathBuf, String> {
        let Some(rv) = &self.reviewing else { return Err("no review is open".into()) };
        match &rv.local {
            Some(l) => l.clone(),
            None => self.db.run(&rv.run).map_or(Err(format!("run {} is gone", rv.run)), |r| crate::refusals::local_of(&self.db, r)),
        }
    }

    /// Tool `k` the agent could not use, of the work reviewed.
    fn tool_item(&self, k: usize) -> Option<&Refusal> {
        let rv = self.reviewing.as_ref()?;
        self.work(&rv.run)?.1.tools.get(k)
    }

    /// The kind of ask the work reviewed was asked in: whose list a tool
    /// allowed from now on goes in.
    fn review_way(&self) -> Option<Way> {
        ask::way_of(&self.db, &self.reviewing.as_ref()?.run)
    }

    /// Tool `k` the agent could not use marked `a` in the review, the status
    /// line saying so; from now on, only where it can be saved.
    pub(super) fn tool_state(&mut self, k: usize, a: Allow) {
        let Some(t) = self.tool_item(k).cloned() else { return };
        if let Some(why) = &t.noted {
            self.status = format!("this tool is noted, not offered: {why}");
            return;
        }
        let local = self.tool_local();
        let kind = self.review_way().map_or("this kind of ask", crate::refusals::kind_said);
        let Some(rv) = self.reviewing.as_mut() else { return };
        let rule = crate::refusals::rule_of(&t, &rv.marks, k).to_string();
        let (said, done) = match (a, &local) {
            (Allow::Always, Err(why)) => (format!("{rule} cannot be allowed from now on: {why}; the rule can be allowed for this work"), false),
            (Allow::Always, Ok(at)) => (
                format!("{rule} is allowed from now on: the rule is saved in {}, for {kind}, once the work is sent back or its review closes", at.display()),
                true,
            ),
            (Allow::Work, _) => (format!("{rule} is allowed for this work: every round of this work may use the rule until the work's review closes; nothing is saved"), true),
            (Allow::Refused, _) => (format!("{rule} is refused"), true),
        };
        if done {
            crate::refusals::set_state(&mut rv.marks, k, a);
        }
        self.status = said;
    }

    /// Tool `k` the agent could not use allowed with its rule `j` instead,
    /// narrower or wider, marked as it was.
    pub(super) fn tool_rule(&mut self, k: usize, j: usize) {
        let Some(t) = self.tool_item(k).cloned() else { return };
        let Some(rv) = self.reviewing.as_mut() else { return };
        crate::refusals::set_picked(&mut rv.marks, k, &t, j);
        let rule = crate::refusals::rule_of(&t, &rv.marks, k).to_string();
        self.status = format!("the item's rule is now {rule}, and the item is {}", crate::refusals::state(&rv.marks, k).said());
    }

    /// Enter on tool `k` the agent could not use: allowed from now on, where
    /// that can be saved; else allowed for this work, the status line saying
    /// why. A tool noted is not offered.
    fn tool_enter(&mut self, k: usize) {
        match self.tool_local() {
            Ok(_) => self.tool_state(k, Allow::Always),
            Err(why) => {
                self.tool_state(k, Allow::Work);
                if self.tool_item(k).is_some_and(Refusal::offered) {
                    self.status += &format!("; allowed from now on is not offered: {why}");
                }
            }
        }
    }

    /// What Delete does on the banner's line of what run `run` brought
    /// back: asks to reject every part of it not marked yet and close its
    /// review, as the review's "Reject all remaining and Close" does. Not
    /// while it goes on from work landed before it, which is reviewed
    /// first, nor with no part left that is not marked, as that item is
    /// not: why is said.
    pub(super) fn reject_work_here(&self, run: &str) -> std::result::Result<Action, String> {
        let Some((r, h)) = self.work(run) else { return Err("this work is gone".into()) };
        // A pull's comments are not rejected: what of it is, its change.
        if crate::held::pulled(r) && h.stack.is_none() {
            return Err(crate::held::FILED.into());
        }
        if ask::guessing(&self.db, r).is_some() {
            return Err("this work goes on from the work landed before it, which waits for your review: review that one first".into());
        }
        let rv = self.review_of(run);
        // The change of that work, by its operations, as its review marks
        // them: each on its own, or the change as one.
        let ops = r.held.as_ref().and_then(|h| crate::operations::of_review(&self.db, r, h));
        let marked = crate::held::parts_left(&self.db, r).iter().all(|p| match (p, &ops) {
            (Part::Stack, Some(ops)) => ops.tally(&rv.marks).2 == 0,
            _ => rv.marks.contains_key(&p.key()),
        });
        let taken = ops.as_ref().is_some_and(|ops| ops.tally(&rv.marks).0 > 0);
        if marked {
            return Err(self.say_keys("nothing of this work is left to reject: press Enter to review it, and close the review there".into()));
        }
        // Its change marked accepted, wholly or in part, it closes accepted:
        // once each answer made on the stack before it is marked.
        if taken && !self.unmarked_in(&rv).is_empty() {
            return Err(self.say_keys(
                "its change is marked accepted, and an answer made on the stack before it is not marked keep, restart or stop: press Enter to review it, and mark each there".into(),
            ));
        }
        Ok(Action::AskRejectWork(run.into()))
    }

    /// What rejecting every part not marked yet of what run `run` brought
    /// back, then closing its review, asks first from the banner, as the
    /// review asks it: the parts not marked yet, and what closing asks
    /// again, said; the menu on keeping it for review.
    pub(super) fn ask_reject_work(&mut self, run: &str) {
        let was = self.reviewing.replace(self.review_of(run));
        let mut said = vec![format!(
            "Not marked yet: {}. The review closes then: what you rejected is dropped for good, and what you accepted is applied.",
            self.review_left().join(", ")
        )];
        // Its change, accepted or rejected as marked, else rejected.
        said.extend(self.review_again(self.closing_as(Some(false))));
        said.extend(self.follow_stopped(Closing::Rejecting));
        // A try push's: what closing pushes, of the tasks accepted already.
        #[cfg(feature = "try-push")]
        said.extend(self.push_said(Some(false)));
        said.extend(self.tools_note(Closing::Rejecting));
        self.reviewing = was;
        let yes = Item::new("Reject them and close", Action::RejectWork(run.into()));
        let mut m = Menu::ask("reject every part of this work not marked yet, then close its review?", yes, "Keep it for review", true);
        m.note = Some(said.join("\n"));
        self.mode = Mode::Menu(m);
    }

    /// The review of what run `run` brought back closed from the banner,
    /// every part not marked yet rejected, as from the review; the main
    /// screen staying, closed or not (the status line says why not). Its
    /// change accepted in part, as its review marks it, once the stack the
    /// operations accepted leave is seen to apply, checked away from the
    /// screen first, as the review's close checks it.
    pub(super) fn reject_work(&mut self, run: &str) {
        let rv = self.review_of(run);
        if let Some((ops, stands)) = crate::held::closing_stands(&self.db, run, &rv.marks, Some(false)) {
            self.check_closing(
                ToClose {
                    review: rv,
                    rest: Some(false),
                    stop: false,
                    banner: true,
                },
                ops,
                stands,
            );
            return;
        }
        self.reviewing = Some(rv);
        self.review_close_now(Some(false), false);
        self.reviewing = None;
        self.screen = Screen::Main;
    }

    /// The work sent back to the agent, with what you wrote in its review:
    /// another round, from it, in `way`, of the whole stack if `whole`, at
    /// `effort`, as its dialog set them; the review left.
    pub(super) fn review_send_back(&mut self, way: Way, whole: bool, effort: Effort) {
        let Some(rv) = self.reviewing.clone() else { return };
        let sent = ask::send_back(&self.path, &self.stack, &rv.run, &rv.marks, Some(effort), Some((way, whole)));
        self.reload();
        match sent {
            Ok(said) => {
                self.status = said;
                self.leave_review();
                self.launch();
            }
            Err(e) => self.status = e.0,
        }
    }

    /// A follow-up that only reads asked about what you wrote in the review,
    /// in `way`, at `effort`, as its dialog set them: the work stays waiting
    /// for your review, its marks as they are, and the review stays open,
    /// what the follow-up brings back to come into it.
    pub(super) fn review_follow_up(&mut self, way: Way, effort: Effort) {
        let Some(rv) = self.reviewing.clone() else { return };
        let asked = ask::follow_up(&self.path, &self.stack, &rv.run, &rv.marks, way, effort);
        self.reload();
        match asked {
            Ok(said) => {
                self.status = said;
                // On the follow-up's line, which says it goes.
                if let Some(at) = self.review_lines().iter().position(|l| l.following.is_some())
                    && let Some(rv) = self.reviewing.as_mut()
                {
                    (rv.sel, rv.in_main, rv.in_patch) = (at, false, false);
                }
                self.launch();
            }
            Err(e) => self.status = e.0,
        }
    }

    /// What the review's line the cursor is on is, to find it again once
    /// the list changes under it: what landed since, a follow-up's work
    /// coming in, adds lines to it, and takes the follow-up's away.
    pub(super) fn review_line_key(&self) -> Option<String> {
        let rv = self.reviewing.as_ref()?;
        self.review_lines().get(rv.sel)?.key()
    }

    /// The review's cursor back on the line `key` names, if the list still
    /// has it; else, its line gone, on the line before it that the cursor
    /// can be on.
    pub(super) fn review_line_back(&mut self, key: Option<String>) {
        let Some(key) = key else { return };
        let lines = self.review_lines();
        let Some(rv) = self.reviewing.as_mut() else { return };
        if let Some(at) = lines.iter().position(|l| l.key().as_deref() == Some(key.as_str())) {
            rv.sel = at;
        } else if !lines.get(rv.sel).is_some_and(Line::selectable) {
            let before = lines.iter().enumerate().take(rv.sel + 1).rev().find(|(_, l)| l.selectable()).map(|(i, _)| i);
            rv.sel = before.or_else(|| lines.iter().position(Line::selectable)).unwrap_or(0);
        }
    }

    /// The follow-ups asked from the review that are going: waiting their
    /// turn, or with the agent.
    pub(super) fn review_following(&self) -> Vec<ask::Following> {
        self.reviewing.as_ref().map(|rv| ask::follow_ups_going(&self.db, &rv.run)).unwrap_or_default()
    }

    /// What sending the work reviewed back does besides, as the dialog that
    /// sends it says under its buttons: with a permission that `changes`
    /// the stack, the edits that went on from it asked again, the follow-ups
    /// going stopped, and what the next round may use; with one that only
    /// reads, what the follow-up may use.
    pub(super) fn sending_back_said(&self, changes: bool) -> Vec<String> {
        let how = if changes { Closing::SendingBack } else { Closing::FollowingUp };
        let mut out: Vec<String> = Vec::new();
        if changes {
            out.extend(self.review_again(Closing::SendingBack));
            out.extend(self.follow_stopped(Closing::SendingBack));
        }
        out.extend(self.tools_note(how));
        out
    }

    /// What closing the review, or sending the work back (`how`), does to
    /// the follow-ups asked from it that are going, said: each is stopped,
    /// and what it would bring back thrown away. None while none goes.
    fn follow_stopped(&self, how: Closing) -> Option<String> {
        let going = self.review_following();
        let doing = if how == Closing::SendingBack { "Sending this work back" } else { "Closing the review" };
        let from = if how == Closing::SendingBack { "from its review" } else { "from it" };
        match going.len() {
            0 => None,
            1 => Some(format!(
                "{doing} stops the follow-up you asked {from}, which is {}: what it would bring back is thrown away, and its comments say that you stopped it.",
                following_said(&going[0])
            )),
            n => Some(format!(
                "{doing} stops the {n} follow-ups you asked {from}: what they would bring back is thrown away, and their comments say that you stopped them."
            )),
        }
    }

    /// The review's list: on top, the comments that asked, each kept or
    /// closed with the review; then its three sections, each part a line,
    /// the change of the stack a line for each patch; then, of a lint job's
    /// work, what the linter reported and did not fix, a line for each run
    /// of it that did.
    pub(super) fn review_lines(&self) -> Vec<Line> {
        let mut out = Vec::new();
        let Some(rv) = &self.reviewing else { return out };
        let Some((r, h)) = self.work(&rv.run) else { return out };
        let line = |text: String, style: Style| Line::new(vec![(text, style)]);
        // A follow-up asked from the review, going: first, as what comes of
        // it comes into the review, and closing the review stops it. Marked
        // as the comments pane marks a comment with the agent, and one
        // waiting its turn.
        let going = ask::follow_ups_going(&self.db, &rv.run);
        if !going.is_empty() {
            let heading = match going.as_slice() {
                [f] => format!(" the follow-up you asked, {}", following_said(f)),
                _ => " the follow-ups you asked".to_string(),
            };
            out.push(line(heading, color::file()));
            for (k, f) in going.iter().enumerate() {
                let mark = if f.run.is_some() { "●" } else { "◌" };
                out.push(Line {
                    following: Some(k),
                    ..line(format!("   {mark} {}", following_what(f)), Style::new())
                });
            }
        }
        let asking = self.asking_open();
        if !asking.is_empty() {
            out.push(line((if asking.len() == 1 { " asking comment" } else { " asking comments" }).into(), color::file()));
            // Each kept, marked and coloured as a part accepted is; each to
            // close, as one rejected is.
            for (k, n) in asking {
                let at = if n.on_stack() { "the stack".to_string() } else { n.where_said() };
                let (m, st) = match rv.marks.get(&crate::held::asking_key(k)) {
                    Some(true) => ("✗ close", color::del()),
                    _ => ("✓ keep ", color::add()),
                };
                out.push(Line {
                    asking: Some(k),
                    ..line(format!("   {m} {at} · {}", first_words(&n.text)), st)
                });
            }
        }
        let parts = self.review_parts();
        let mark = |p: Part| rv.marks.get(&p.key()).copied();
        let title = |text: &str| line(text.into(), color::file());
        if let Some(((now, next), ops)) = self.review_stacks().zip(self.review_ops()) {
            // How the operations are marked: in their title, a count of each
            // mark, in that mark's colour (`mark_of`), those to review in the
            // title's; and before each operation, its own mark, the operation
            // in the colour of its kind. A patch left as it was has none. The
            // change marked as one, the title says its mark, and every line
            // has it.
            let mut parts: Parts = vec![(" stack operations · ".into(), color::file())];
            match ops.whole() {
                // No operation to mark: the change as a whole, its word in
                // its mark's colour.
                true => parts.push(match mark(Part::Stack) {
                    Some(true) => ("accepted".into(), color::add()),
                    Some(false) => ("rejected".into(), color::del()),
                    None => ("to review".into(), color::file()),
                }),
                false => {
                    let (yes, no, open) = ops.counts(&rv.marks);
                    let counts = [(yes, "accepted", color::add(), "✓"), (no, "rejected", color::del(), "✗"), (open, "to review", color::file(), "·")];
                    for (k, (n, word, st, sign)) in counts.into_iter().filter(|(n, ..)| *n > 0).enumerate() {
                        if k > 0 {
                            parts.push((", ".into(), color::file()));
                        }
                        parts.push((format!("{sign} {n} {word}"), st));
                    }
                }
            }
            out.push(Line {
                part: Some(Part::Stack),
                ..Line::new(parts)
            });
            let diff = crate::history::stacks_diff(&self.db, Some(&now), Some(&next), false);
            for (k, d) in diff.iter().enumerate() {
                if let Some((mut text, style)) = operation(d) {
                    let (m, st) = match ops.by_entry(k) {
                        Some(o) if o.marked() => mark_of(ops.mark(&rv.marks, o)),
                        None if ops.one() => mark_of(mark(Part::Stack)),
                        _ => (" ", Style::new()),
                    };
                    // A pull's: what it takes from Phabricator, said after it.
                    if let Some(said) = self.taken_line(r, &next, d) {
                        text += &format!(", {said}");
                    }
                    out.push(Line {
                        part: Some(Part::Stack),
                        entry: Some(k),
                        ..Line::new(vec![("   ".into(), Style::new()), (m.into(), st), (" ".into(), Style::new()), (text, style)])
                    });
                }
            }
            // What the pull found that changes nothing: to read.
            if let Some(found) = crate::held::took(r).map(|t| &t.found).filter(|f| !f.is_empty()) {
                out.push(title(" what the pull found, to read"));
                for (k, said) in found.iter().enumerate() {
                    out.push(Line {
                        noted: Some(k),
                        ..line(format!("   {}", short(said, 120)), Style::new())
                    });
                }
            }
        }
        let opened: Vec<Part> = parts.iter().copied().filter(|p| matches!(p, Part::Opened(_))).collect();
        if !opened.is_empty() {
            out.push(title(" comments opened"));
            for p in opened {
                let Part::Opened(k) = p else { continue };
                let n = &h.opened[k];
                let (m, st) = mark_of(mark(p));
                out.push(Line {
                    part: Some(p),
                    ..line(format!("   {m} {} · {}", n.where_said(), first_words(&n.text)), st)
                });
            }
        }
        // A try push's tasks, each by its label, how many times each runs
        // said over them.
        #[cfg(feature = "try-push")]
        {
            let tasks: Vec<Part> = parts.iter().copied().filter(|p| matches!(p, Part::Task(_))).collect();
            if !tasks.is_empty() {
                let each = h.rebuild.map_or(String::new(), |n| format!(" · each runs {n} times"));
                out.push(title(&format!(" tasks to push to try{each}")));
                for p in tasks {
                    let Part::Task(k) = p else { continue };
                    let (m, st) = mark_of(mark(p));
                    out.push(Line {
                        part: Some(p),
                        ..line(format!("   {m} {}", h.tasks[k].label), st)
                    });
                }
            }
        }
        let replies: Vec<Part> = parts.iter().copied().filter(|p| matches!(p, Part::Reply(_))).collect();
        if !replies.is_empty() {
            out.push(title(" replies"));
            for p in replies {
                let Part::Reply(k) = p else { continue };
                let rp = &h.replies[k];
                let on = |n: &Note| if n.on_stack() { "the stack".to_string() } else { n.where_said() };
                let at = match crate::held::goes(&self.db, &r.stack, rp) {
                    Goes::Open(n) => on(n),
                    // A reviewer's, to a comment you closed: where it was.
                    Goes::Reopens(Some(n)) => format!("{} (closed)", on(n)),
                    Goes::Reopens(None) => "a comment you closed".to_string(),
                    Goes::Nowhere => "a comment deleted".to_string(),
                };
                let (m, st) = mark_of(mark(p));
                let edit = if rp.was.is_some() { format!("{}: ", rp.said()) } else { String::new() };
                out.push(Line {
                    part: Some(p),
                    ..line(format!("   {m} {at} · {edit}{}", crate::markdown::first_line(&rp.text)), st)
                });
            }
        }
        // What a build and test asked as a follow-up found, to read, nothing
        // to accept or reject: its replies, where each goes, then what it
        // found of each patch, by its title.
        if h.found() {
            out.push(title(&format!(" {}, to read", crate::held::FOUND)));
            let taken = self.db.review(&r.id).map(|v| v.taken.clone()).unwrap_or_default();
            for (k, rp) in h.replies.iter().enumerate().filter(|(k, rp)| rp.read && !taken.contains(k)) {
                let at = self.db.note(&rp.change, &rp.id).map_or("a comment deleted".to_string(), |n| if n.on_stack() { "the stack".to_string() } else { n.where_said() });
                out.push(Line {
                    found: Some(Found::Reply(k)),
                    ..line(format!("   {at} · {}", crate::markdown::first_line(&rp.text)), Style::new())
                });
            }
            for (j, f) in h.follow_ups.iter().enumerate() {
                for (i, n) in f.found.iter().enumerate() {
                    let said = crate::thread::thread_turns(&n.text).first().map(|t| crate::markdown::first_line(&t.body)).unwrap_or_default();
                    out.push(Line {
                        found: Some(Found::Patch(j, i)),
                        ..line(format!("   {} · {said}", n.preview), Style::new())
                    });
                }
            }
        }
        // The tools the agent could not use, each by the rule that would let
        // it, as the review marks it, in a word or two after it, the list
        // being narrow: refused, allowed for this work or from now on; those
        // noted, not offered.
        if !h.tools.is_empty() {
            out.push(title(&format!(" {}", crate::refusals::HEADING)));
            for (k, t) in h.tools.iter().enumerate() {
                let rule = crate::refusals::rule_of(t, &rv.marks, k);
                let (m, said, st) = match (&t.noted, crate::refusals::state(&rv.marks, k)) {
                    (Some(_), _) => ("·", "noted", color::dim()),
                    (None, Allow::Refused) => ("✗", "refused", color::del()),
                    (None, Allow::Work) => ("✓", "for this work", color::add()),
                    (None, Allow::Always) => ("✓", "from now on", color::add()),
                };
                out.push(Line {
                    tool: Some(k),
                    ..line(format!("   {m} {rule} · {said}"), st)
                });
            }
        }
        // The answers made on the stack before the change, each marked and
        // coloured for when the change is accepted, kept as a part accepted
        // is, stopped as one rejected is; then, in a word, whether it runs or
        // landed and waits for your review, the list being narrow; then what
        // it was asked about. One neither any more, since the review opened,
        // says so, dim: nothing to mark.
        let answers = self.answer_items();
        if !answers.is_empty() {
            out.push(title(&format!(" {ANSWERS}")));
            for (id, going) in answers {
                let about = self.db.run(&id).map_or(String::new(), |a| crate::held::answer_about(&self.db, a));
                let (m, st) = match going.markable() {
                    true => answer_mark_of(crate::held::answer_mark(&rv.marks, &id)),
                    false => ("·", color::dim()),
                };
                out.push(Line {
                    answer: Some(id),
                    ..line(format!("   {m:<9} {} · {about}", going.word()), st)
                });
            }
        }
        // What the linter reported and did not fix: each run of it that did,
        // to read, nothing to accept or reject.
        if let Some(l) = h.lint.as_ref().filter(|l| !l.errors.is_empty()) {
            out.push(title(" what the linter reported, to read"));
            let stopped = l.stopped.as_ref().map(|s| s.at);
            for (k, e) in l.errors.iter().enumerate() {
                out.push(Line {
                    report: Some(k),
                    ..line(format!("   {}", e.said(stopped)), Style::new())
                });
            }
        }
        // A reconcile with HEAD's: stacked's comment, what to run in your
        // checkout should you reject it, to read.
        if self.work(&rv.run).is_some_and(|(r, _)| r.reconcile.is_some()) {
            out.push(title(" stacked's comment, to read"));
            out.push(Line {
                stacked: true,
                ..line("   what to run if you reject it".into(), Style::new())
            });
        }
        // What you wrote in it, which sends it back to the agent: each line
        // to see where it would be, write again, or take back.
        let mut yours: Vec<(String, Yours)> = Vec::new();
        if let Some((r, _)) = self.work(&rv.run) {
            let (opened, _) = crate::held::yours_in(&self.db, r);
            // What you said last there: the comment, or your reply to it.
            for n in opened.iter().filter_map(|(c, i)| self.db.note(c, i)) {
                let said = last_mine(&n.text).unwrap_or_default();
                let text = format!("   ✎ {} · {}", n.where_said(), crate::markdown::first_line(&said));
                yours.push((text, Yours::Comment(n.change.clone(), n.id.clone())));
            }
        }
        // Your answers each to the last reply of its comment, not one gone
        // with a follow-up still going.
        let busy = ask::following(&self.db, &rv.run);
        for k in crate::held::answered(h, &busy) {
            let rp = &h.replies[k];
            let at = self.db.note(&rp.change, &rp.id).map_or("a comment".to_string(), |n| n.where_said());
            let text = format!("   ✎ your answer on {at} · {}", crate::markdown::first_line(rp.answer.as_deref().unwrap_or("")));
            yours.push((text, Yours::Answer(k)));
        }
        #[cfg(feature = "try-push")]
        for (k, t) in h.tasks.iter().enumerate().filter(|(_, t)| t.answer.is_some()) {
            let text = format!("   ✎ your answer on {} · {}", t.label, crate::markdown::first_line(t.answer.as_deref().unwrap_or("")));
            yours.push((text, Yours::TaskAnswer(k)));
        }
        for (k, t) in h.tools.iter().enumerate().filter(|(_, t)| t.answer.is_some()) {
            let rule = crate::refusals::rule_of(t, &rv.marks, k);
            let text = format!("   ✎ your answer on {rule} · {}", crate::markdown::first_line(t.answer.as_deref().unwrap_or("")));
            yours.push((text, Yours::ToolAnswer(k)));
        }
        if !yours.is_empty() {
            out.push(title(" what you wrote, to send back"));
            for (text, y) in yours {
                out.push(Line {
                    yours: Some(y),
                    ..line(text, color::you())
                });
            }
        }
        out
    }

    /// What the part the cursor is on would be, `w` wide: the stack diff of
    /// the change, or the comment in its place; and the lines that are the
    /// part, the first to show first.
    fn review_view(&self, line: Option<&Line>, w: usize) -> View {
        // The list empty: no part left, nor a comment that asked still open.
        let Some((rv, line)) = self.reviewing.as_ref().zip(line) else {
            return (" nothing is left to accept or reject: press Esc, then choose Close review".into(), vec![], 0..0);
        };
        let Some((r, h)) = self.work(&rv.run) else { return (String::new(), vec![], 0..0) };
        // A comment that asked: as it is now.
        if let Some(k) = line.asking
            && let Some((_, n)) = self.asking_open().into_iter().find(|(j, _)| *j == k)
        {
            let said = match rv.marks.get(&crate::held::asking_key(k)) {
                Some(true) => " the comment that asked, as it is: closed with the review",
                _ => " the comment that asked, as it is: kept open",
            };
            return self.comment_view(r, &n, said.into(), w);
        }
        // What you wrote: where it would be.
        if let Some(y) = &line.yours {
            return self.yours_view(r, h, y, w);
        }
        // A follow-up going: what it asks, and how far the agent is.
        if let Some(k) = line.following {
            return self.following_view(&rv.run, k, w);
        }
        // What a build and test asked as a follow-up found: to read.
        if let Some(f) = &line.found {
            return self.found_view(r, h, f, w);
        }
        // What the linter reported and did not fix: its lines, to read.
        if let Some(l) = h.lint.as_ref()
            && let Some(e) = line.report.and_then(|k| l.errors.get(k))
        {
            return report_view(l, e, w);
        }
        // Stacked's comment on a reconcile with HEAD: its sentences, to read.
        if line.stacked
            && let Some(job) = r.reconcile.as_deref()
        {
            return stacked_view(job, w);
        }
        // A tool the agent could not use: what it said of it, as its comment.
        if let Some(k) = line.tool {
            return self.tool_view(h, k, w);
        }
        // An answer made on the stack before the change: the comment it
        // answers, and what each mark does.
        if let Some(id) = &line.answer {
            return self.answer_view(r, id, w);
        }
        // What a pull found that changes nothing: its sentence, to read.
        if let Some(said) = line.noted.and_then(|k| crate::held::took(r)?.found.get(k)) {
            let lines: Vec<Vec<(String, Style)>> = wrap(said, w.max(20)).into_iter().map(|l| vec![(l, Style::new())]).collect();
            let n = lines.len();
            return (" what the pull found: it changes nothing of the stack".into(), lines, 0..n);
        }
        match line.part {
            Some(Part::Stack) | None => {
                let Some(mut v) = self.review_diff(w) else {
                    return ("the agent changed nothing of the stack".into(), vec![], 0..0);
                };
                // Where the patch of the list's line is, among every stop.
                let at = line.entry.and_then(|e| v.of_entry.get(e)).map(|&k| v.stop_of(k));
                let first = at.map_or(0, |k| v.spans[k].start);
                // On an operation, the cursor in the list: what it needs, and
                // what needs it, said under its line, over what changed in its
                // patch, as what is stored of them says. Not once the cursor is
                // in the stack diff, which goes from entry to entry as it is.
                // The change marked as one, though it has operations: why,
                // under the line, or over the diff on their title.
                let indent = "      ";
                let said_lines = |said: &[String]| -> Vec<Vec<(String, Style)>> {
                    let w = w.saturating_sub(indent.len()).max(20);
                    said.iter().flat_map(|s| wrap(s, w)).map(|l| vec![(format!("{indent}{l}"), color::note())]).collect()
                };
                if let Some(k) = at.filter(|_| !rv.in_main)
                    && let Some(ops) = self.review_ops()
                    && let Some(o) = Self::line_op(&ops, line)
                    && let Some(needs) = self.review_needs()
                {
                    let mut under = said_lines(&ops.pane_said(needs, &rv.marks, &o.key));
                    // A pull's: what it takes from Phabricator, and why; and,
                    // the patch changed in the stack since, what this
                    // version had since that the pulled one lacks.
                    if let Some((now, next)) = self.review_stacks()
                        && let Some(t) = self.op_taken(r, &next, o.at)
                    {
                        under.extend(said_lines(&taken_said(t, o.at, self.resolved(&next, o.at))));
                        under.extend(self.since_lines(&now, t, w));
                    }
                    let end = v.spans[k].end.min(v.lines.len());
                    let n = under.len();
                    v.lines.splice(end..end, under);
                    return (v.said, v.lines, first..end + n);
                }
                if !rv.in_main
                    && line.part == Some(Part::Stack)
                    && let Some(why) = self.review_whole_why()
                {
                    let under = said_lines(&[why]);
                    let end = at.map_or(0, |k| v.spans[k].end.min(v.lines.len()));
                    let n = under.len();
                    v.lines.splice(end..end, under);
                    return (v.said, v.lines, first.min(end)..end + n);
                }
                (v.said, v.lines, first..first)
            }
            // A try push's task: why the agent selected it, as its comment.
            #[cfg(feature = "try-push")]
            Some(Part::Task(k)) => self.task_view(h, k, w),
            Some(p) => {
                let (n, said) = match p {
                    Part::Reply(k) => {
                        let rp = &h.replies[k];
                        // A reviewer's, to a comment you closed: its thread as
                        // it was when closed, as the pull kept it.
                        let (n, closed) = match crate::held::goes(&self.db, &r.stack, rp) {
                            Goes::Open(n) => (n, false),
                            Goes::Reopens(Some(n)) => (n, true),
                            Goes::Reopens(None) => return (format!(" {}: accepting it reopens the comment", closed_said(rp)), vec![], 0..0),
                            Goes::Nowhere => return ("its comment was deleted: nothing to answer".into(), vec![], 0..0),
                        };
                        // After the replies before it there, a follow-up's.
                        let taken = self.db.review(&r.id).map(|v| v.taken.clone()).unwrap_or_default();
                        let text = h.thread_upto(k, &n.text, &taken);
                        let said = match &rp.promote {
                            Some(why) => format!(" its reply, as it would be in its thread; then it moves to the stack's comments: {why}"),
                            None if closed => format!(" {}, as it would be in its thread: accepting it reopens the comment", closed_said(rp)),
                            None if rp.is_anew() => format!(" {}'s comment, with the suggested edit that stacked now reads from Phabricator, as it would be in its thread", rp.who()),
                            None if rp.was.is_some() => format!(" {}'s comment, edited on Phabricator, as it would be in its thread", rp.who()),
                            None => " its reply, as it would be in its thread".to_string(),
                        };
                        (Note { text, ..n.clone() }, said)
                    }
                    Part::Opened(k) if crate::held::pulled(r) => (h.opened[k].clone(), " a reviewer's comment, where it would be".to_string()),
                    Part::Opened(k) => (h.opened[k].clone(), " a comment it opened, where it would be".to_string()),
                    Part::Stack => unreachable!(),
                    #[cfg(feature = "try-push")]
                    Part::Task(_) => unreachable!(),
                };
                let (said, mut lines, at) = self.comment_view(r, &n, said, w);
                // A reviewer's comment edited on Phabricator, or read again
                // with its suggested edit: as it was filed, then as it is now,
                // over its thread.
                if let Part::Reply(k) = p
                    && let Some(was) = &h.replies[k].was
                {
                    let both = edit_lines(&h.replies[k], was, w);
                    let n = both.len();
                    lines.splice(at.start..at.start, both);
                    return (said, lines, at.start..at.end + n);
                }
                (said, lines, at)
            }
        }
    }

    /// Comment `n` of the work of run `r` where it is, `w` wide, `said`
    /// above it; and the lines that are its own.
    fn comment_view(&self, r: &Run, n: &Note, said: String, w: usize) -> View {
        let deco = Deco {
            pointed: self.pointed_of([n]),
            ages: self.ages_of([n]),
            ..Default::default()
        };
        let rows = if n.on_stack() {
            let Ok(s) = self.db.stack(&r.stack) else { return (said, vec![], 0..0) };
            let titles: Vec<String> = s.patches.iter().map(|id| self.db.patch(id).title().to_string()).collect();
            stack_rows(&format!("the stack {}", r.stack), &titles, &[n], w, &deco)
        } else {
            // On a patch of the stack; or on one the work adds, as the work
            // makes it (the linter's patch on top); or on one neither holds,
            // a work sent back added and this one dropped, as its latest
            // version has it.
            let p = self
                .db
                .stack(&r.stack)
                .ok()
                .and_then(|s| s.patches.iter().map(|id| self.db.patch(id)).find(|p| p.change == n.change))
                .or_else(|| {
                    let (_, next) = self.review_stacks()?;
                    next.patches.iter().map(|id| self.db.patch(id)).find(|p| p.change == n.change)
                })
                .or_else(|| self.db.latest_version(&n.change));
            let Some(p) = p else { return (said, vec![], 0..0) };
            patch_rows(p, &[n], w, &deco)
        };
        let head = rows.iter().position(|row| row.head && row.note.as_deref() == Some(&n.id)).unwrap_or(0);
        let end = rows.iter().rposition(|row| row.note.as_deref() == Some(&n.id)).map_or(head, |i| i + 1);
        let lines = rows.iter().map(|row| row_parts(row, false)).collect();
        (said, lines, head..end)
    }

    /// What you wrote in the review, `y`, where it would be, `w` wide: your
    /// comment on its patch as the work leaves it, the work's comments with
    /// it; your answer after the reply, in its thread. And the lines that
    /// are its own.
    fn yours_view(&self, r: &Run, h: &Work, y: &Yours, w: usize) -> View {
        match y {
            Yours::Comment(change, id) => {
                let said = " your comment, where it is on the patch as the work leaves it".to_string();
                let on_work = self
                    .review_stacks()
                    .and_then(|(_, next)| next.patches.iter().map(|p| self.db.patch(p)).find(|p| p.change == *change).cloned());
                let Some(p) = on_work else {
                    // Not on a patch the work leaves: as it is.
                    let Some(n) = self.db.note(change, id).cloned() else { return (said, vec![], 0..0) };
                    return self.comment_view(r, &n, said, w);
                };
                let rows = self.review_rows(&p, w);
                let head = rows.iter().position(|row| row.head && row.note.as_deref() == Some(id.as_str())).unwrap_or(0);
                let end = rows.iter().rposition(|row| row.note.as_deref() == Some(id.as_str())).map_or(head, |i| i + 1);
                (said, rows.iter().map(|row| row_parts(row, false)).collect(), head..end)
            }
            Yours::Answer(k) => {
                let Some(rp) = h.replies.get(*k) else { return (String::new(), vec![], 0..0) };
                let Some(n) = self.db.note(&rp.change, &rp.id) else {
                    return ("its comment was deleted: nothing to answer".into(), vec![], 0..0);
                };
                let taken = self.db.review(&r.id).map(|v| v.taken.clone()).unwrap_or_default();
                let n = Note {
                    text: h.thread_upto(*k, &n.text, &taken),
                    ..n.clone()
                };
                self.comment_view(r, &n, " your answer, after the work's reply, as it would be in its thread".into(), w)
            }
            #[cfg(feature = "try-push")]
            Yours::TaskAnswer(k) => self.task_view(h, *k, w),
            Yours::ToolAnswer(k) => self.tool_view(h, *k, w),
        }
    }

    /// Follow-up `k` going, of the review of run `run`, `w` wide: the
    /// comments it asks about, each by where it is and how it starts; then,
    /// with the agent, the last lines the agent wrote; then what closing the
    /// review does to it. All of it is the part.
    fn following_view(&self, run: &str, k: usize, w: usize) -> View {
        let going = ask::follow_ups_going(&self.db, run);
        let Some(f) = going.get(k) else { return (" this follow-up is no longer going".into(), vec![], 0..0) };
        let w = w.max(20);
        let mut lines: Vec<Vec<(String, Style)>> = Vec::new();
        fn said_in(lines: &mut Vec<Vec<(String, Style)>>, said: &str, w: usize, st: Style) {
            lines.extend(wrap(said, w).into_iter().map(|l| vec![(l, st)]));
        }
        let at = f.effort.map_or(String::new(), |e| format!(", at {}", e.name()));
        said_in(&mut lines, &format!("The follow-up asks the agent about these comments, {}{at}:", f.way.said()), w, Style::new());
        for (c, i) in &f.notes {
            if let Some(n) = self.db.note(c, i) {
                let place = if n.on_stack() { "the stack".to_string() } else { n.where_said() };
                said_in(&mut lines, &format!("  {place} · {}", first_words(&n.text)), w, Style::new());
            }
        }
        lines.push(vec![(String::new(), Style::new())]);
        match &f.run {
            Some(id) => {
                said_in(&mut lines, "The agent works on the follow-up. The last lines it wrote:", w, color::dim());
                for l in ask::tail(&self.db, id, 12) {
                    said_in(&mut lines, &format!("  {l}"), w, Style::new());
                }
            }
            None => said_in(&mut lines, "The follow-up waits its turn in the agent's queue: nothing is asked yet.", w, color::dim()),
        }
        lines.push(vec![(String::new(), Style::new())]);
        said_in(
            &mut lines,
            "Closing the review stops the follow-up: what it would bring back is thrown away, and its comments say that you stopped it.",
            w,
            color::note(),
        );
        let n = lines.len();
        (format!(" the follow-up you asked, {}: what it brings back comes into this review", following_said(f)), lines, 0..n)
    }

    /// What a build and test asked as a follow-up found, `f`, of work `h`
    /// of run `r`, `w` wide: its reply, as it would be in its thread, after
    /// the replies before it there; or what it found of a patch, on the
    /// patch's title. And the lines that are its own.
    fn found_view(&self, r: &Run, h: &Work, f: &Found, w: usize) -> View {
        match f {
            Found::Reply(k) => {
                let Some(rp) = h.replies.get(*k) else { return (String::new(), vec![], 0..0) };
                let Some(n) = self.db.note(&rp.change, &rp.id) else {
                    return ("its comment was deleted: nothing to read there".into(), vec![], 0..0);
                };
                let taken = self.db.review(&r.id).map(|v| v.taken.clone()).unwrap_or_default();
                let n = Note {
                    text: h.thread_upto(*k, &n.text, &taken),
                    ..n.clone()
                };
                self.comment_view(r, &n, format!(" {}, as it would be in its thread: to read", crate::held::FOUND), w)
            }
            Found::Patch(j, i) => {
                let Some(n) = h.follow_ups.get(*j).and_then(|f| f.found.get(*i)) else { return (String::new(), vec![], 0..0) };
                self.comment_view(r, n, format!(" {} of this patch: to read, never filed", crate::held::FOUND), w)
            }
        }
    }

    /// Tool `k` the agent could not use, of work `h`, `w` wide: why the agent
    /// needed it and how it worked around it, as its comment on the tool, by
    /// its rule; your answer to it after, if you wrote one; then the calls it
    /// tried, and where the tool stands. And the lines that are the comment.
    fn tool_view(&self, h: &Work, k: usize, w: usize) -> View {
        let (Some(t), Some(rv)) = (h.tools.get(k), self.reviewing.as_ref()) else { return (String::new(), vec![], 0..0) };
        let rule = crate::refusals::rule_of(t, &rv.marks, k).to_string();
        let why = if t.why.trim().is_empty() { "The agent did not say why it needed this tool." } else { t.why.as_str() };
        let mut text = crate::thread::append_agent("", why, "tool", self.reviewed_at());
        if let Some(a) = &t.answer {
            text = append_turn(&text, Role::You, a, None);
        }
        let n = Note {
            change: stack_change(&self.stack),
            id: format!("tool {k}"),
            text,
            by: Some("agent".into()),
            ..Default::default()
        };
        let said = match (&t.noted, &t.answer) {
            (Some(_), _) => " why the agent needed this tool, as its comment: noted, it is not offered",
            (None, Some(_)) => " why the agent needed this tool, and your answer, which sends the work back to the agent",
            (None, None) => " why the agent needed this tool, as its comment: a reply to it sends the work back to the agent",
        };
        let mut rows = vec![Row {
            kind: Kind::File,
            text: rule.clone(),
            ..Default::default()
        }];
        rows.extend(crate::rows::comment_rows(&n, w, &Deco::default()));
        let end = rows.len();
        let mut lines: Vec<Vec<(String, Style)>> = rows.iter().map(|row| row_parts(row, false)).collect();
        let blank = || vec![(String::new(), Style::new())];
        lines.push(blank());
        let tried = if t.calls.len() == 1 { "The call the agent tried, which was refused:" } else { "The calls the agent tried, each refused:" };
        lines.push(vec![(tried.to_string(), color::dim())]);
        for c in &t.calls {
            lines.extend(wrap(c, w.saturating_sub(2).max(20)).into_iter().map(|l| vec![(format!("  {l}"), Style::new())]));
        }
        lines.push(blank());
        let kind = self.review_way().map_or("this kind of ask", crate::refusals::kind_said);
        let mut stands = match (&t.noted, crate::refusals::state(&rv.marks, k)) {
            (Some(why), _) => vec![format!("This tool is not offered: {why}.")],
            (None, Allow::Refused) => vec![format!("{rule} is refused: the agent is not let use it.")],
            (None, Allow::Work) => vec![format!("{rule} is allowed for this work: every round of this work may use the tool until the work's review closes, and nothing is saved.")],
            (None, Allow::Always) => vec![match self.tool_local() {
                Ok(at) => format!("{rule} is allowed from now on: once the work is sent back or its review closes, the rule is saved in {}, for {kind}.", at.display()),
                Err(why) => format!("{rule} is marked allowed from now on, which cannot be saved: {why}."),
            }],
        };
        if t.offered() {
            if let Err(why) = self.tool_local() {
                stands.push(format!("Allowed from now on is not offered: {why}."));
            }
            let at = crate::refusals::picked(&rv.marks, k, t);
            let others: Vec<&str> = t.rules.iter().enumerate().filter(|(j, _)| *j != at).map(|(_, r)| r.as_str()).collect();
            if !others.is_empty() {
                stands.push(format!("The item's menu can allow the tool with another rule instead: {}.", others.join(", ")));
            }
        }
        for said in stands {
            lines.extend(wrap(&said, w.max(20)).into_iter().map(|l| vec![(l, color::note())]));
        }
        (said.to_string(), lines, 1..end)
    }

    /// The answer of run `id`, made on the stack before the change of the
    /// work of run `r`, `w` wide: the comment it was asked about, where it
    /// is, with its reply after it as it would be in its thread once it
    /// waits for your review; then how it is marked, and what that does
    /// once the change is accepted. And the lines that are the comment.
    fn answer_view(&self, r: &Run, id: &str, w: usize) -> View {
        let (Some(rv), Some(a)) = (self.reviewing.as_ref(), self.db.run(id)) else { return (String::new(), vec![], 0..0) };
        let going = self.answer_items().into_iter().find(|(k, _)| k == id).map_or(Going::Reviewed, |(_, g)| g);
        let what = if ask::selects_tasks(a) { "a try push's selection" } else { "an answer" };
        let said = match going {
            Going::Running => format!(" {what} running, asked on the stack before the change: the comment it answers"),
            Going::Waiting => format!(" {what} waiting for your review, asked on the stack before the change: its reply as it would be in its thread"),
            Going::Failed => format!(" {what} that failed since the review opened: nothing to mark"),
            Going::Reviewed => format!(" {what} reviewed since the review opened: nothing to mark"),
        };
        let note = ask::asked_about(a).first().and_then(|(c, i)| self.db.note(c, i)).cloned();
        let (said, mut lines, at) = match note {
            Some(n) => {
                // Landed: its reply after the comment, as it would be.
                let reply = a
                    .held
                    .as_ref()
                    .filter(|_| going == Going::Waiting)
                    .and_then(|h| h.replies.iter().find(|rp| rp.change == n.change && rp.id == n.id));
                let n = match reply {
                    Some(rp) => Note { text: rp.after(&n.text), ..n },
                    None => n,
                };
                self.comment_view(r, &n, said, w)
            }
            None => (said, vec![], 0..0),
        };
        let stands: Vec<String> = match (going.markable(), crate::held::answer_mark(&rv.marks, id)) {
            (false, _) => vec!["The answer is no longer running nor waiting for your review, so it needs no mark.".into()],
            (true, mark) => vec![
                match mark {
                    Some(m) => format!("Marked {}: once the change is accepted, {}.", m.word(), answer_does(m, going)),
                    None => "Not marked yet: Enter keeps the answer, Delete stops it, and Space has its menu, to restart it too. The review closes with the change accepted only once each answer made on the stack before it is marked.".into(),
                },
                "The mark counts only once the change is accepted: rejected or sent back, the change leaves the stack as it is, and the answer goes on as it is.".into(),
            ],
        };
        // Said right under the comment, in sight with it.
        let mut under = vec![vec![(String::new(), Style::new())]];
        for said in stands {
            under.extend(wrap(&said, w.max(20)).into_iter().map(|l| vec![(l, color::note())]));
        }
        let n = under.len();
        let end = at.end.min(lines.len());
        lines.splice(end..end, under);
        (said, lines, at.start.min(end)..end + n)
    }

    /// Why the agent selected task `k` of the try push work `h`, `w` wide,
    /// as its comment on the task, by its label; your answer to it after,
    /// if you wrote one. And the lines that are the comment.
    #[cfg(feature = "try-push")]
    fn task_view(&self, h: &Work, k: usize, w: usize) -> View {
        let Some(t) = h.tasks.get(k) else { return (String::new(), vec![], 0..0) };
        let why = if t.why.trim().is_empty() { "The agent did not say why." } else { t.why.as_str() };
        let mut text = crate::thread::append_agent("", why, "try", self.reviewed_at());
        if let Some(a) = &t.answer {
            text = append_turn(&text, Role::You, a, None);
        }
        let n = Note {
            change: stack_change(&self.stack),
            id: format!("task {k}"),
            text,
            by: Some("agent".into()),
            ..Default::default()
        };
        let said = match &t.answer {
            Some(_) => " why the agent selected this task, and your answer, which sends the work back to the agent",
            None => " why the agent selected this task, as its comment: a reply to it sends the work back to the agent",
        };
        let mut rows = vec![Row {
            kind: Kind::File,
            text: t.label.clone(),
            ..Default::default()
        }];
        rows.extend(crate::rows::comment_rows(&n, w, &Deco::default()));
        let end = rows.len();
        (said.to_string(), rows.iter().map(|row| row_parts(row, false)).collect(), 1..end)
    }

    /// The review: its list on the left, what the cursor's part would be on
    /// the right.
    pub(super) fn draw_review(&mut self, buf: &mut Buffer, body: Rect) -> Option<(u16, u16)> {
        self.review_found_again();
        let (w, h) = (body.width as usize, body.height as usize);
        let lines = self.review_lines();
        let rv = self.reviewing.clone()?;
        let list_w = if w < 60 { w } else { (w * 2 / 5).max(30) };
        let list = Rect::new(body.x, body.y, list_w as u16, body.height);
        // Wide, the part the cursor is on has it all, the list out of sight.
        let wide = rv.in_main && rv.wide;
        let rows = h.saturating_sub(1);
        // How wide the pane on the right draws the stack diff's interdiffs:
        // too narrow for both panes, wide, as → shows it.
        self.inter_w.set(if wide || w < 60 { w } else { w - list_w - 1 });
        if !wide {
            // What is left of it to review: not the replies taken already.
            let title = self.work(&rv.run).map_or(String::new(), |(r, h)| {
                let left = crate::held::left(&self.db, r);
                if crate::held::pulled(r) {
                    return format!(" pulled from Phabricator: {left}");
                }
                if crate::held::by_hand(r) {
                    return format!(" your edit by hand: {left} · {}", h.edits.join(", then "));
                }
                // A reconcile with HEAD's: what it takes from your checkout.
                if crate::reconcile::is(r) {
                    return format!(" the reconcile with HEAD: {left} · {}", r.request);
                }
                // The linter's: what it did, said as the history says it, or
                // what it reported alone.
                if crate::held::linted(r) {
                    let did = match h.stack.as_ref() {
                        Some(_) => h.edits.join(", then "),
                        None => format!("{} fixed nothing", r.request),
                    };
                    return format!(" the linter's work: {left} · {did}");
                }
                let asked = if r.request.is_empty() { String::new() } else { format!(" · asked \"{}\"", r.request) };
                let edited = if h.edits.is_empty() { String::new() } else { format!(" · then you {}", h.edits.join(", then ")) };
                // The effort it was asked at, first, where the list's width
                // leaves it in sight.
                let whose = crate::thread::named_at("the agent's work", ask::effort_said(r));
                format!(" {whose}: {left}{asked}{edited}")
            });
            put(buf, list, 0, 0, list_w, &fit(&title, list_w), color::header());
            // The line the cursor is on in sight; scrolled by the wheel,
            // moved by the lines it turned, as far as the last line on the
            // bottom one, and since, until a key, where the wheel left it.
            self.scrolls_in(list, Wheeled::ReviewList);
            let mut top = rv.top;
            if let Some(n) = self.turned(Wheeled::ReviewList) {
                top = moved(top, n, lines.len().saturating_sub(rows));
            }
            if !self.free(Wheeled::ReviewList) {
                if rv.sel < top {
                    top = rv.sel;
                } else if rv.sel >= top + rows {
                    top = rv.sel + 1 - rows;
                }
            }
            // Longer than the list, its scroll bar beside the lines under its
            // title: in the line on its right, else, the list alone on the
            // screen, in its last column.
            let tw = self.pane_bar(Wheeled::ReviewList, list, buf.area, (1, rows), Shown { top, rows, all: lines.len() });
            for (y, (i, l)) in lines.iter().enumerate().skip(top).take(rows).enumerate() {
                // The line the cursor is on: each of its parts in the cursor's
                // colours, whatever its own, a mark too; picked, the cursor in
                // the other pane, each in its own, as `picked` paints them.
                let drawn = |st: Style| match (i == rv.sel, rv.in_main) {
                    (false, _) => st,
                    (true, true) => st.patch(color::picked()),
                    (true, false) => on_cursor(st),
                };
                let parts: Parts = l.parts.iter().map(|(text, st)| (text.clone(), drawn(*st))).collect();
                put_parts(buf, list, 0, y + 1, tw, &parts);
                spot_in(&self.spots, list, (0, y + 1, list_w, 1), Spot::ReviewLine(i));
            }
            if let Some(r) = &mut self.reviewing {
                r.top = top;
            }
            if w < 60 {
                return None;
            }
            for y in 0..body.height {
                buf.set_string(body.x + list_w as u16, body.y + y, "│", color::meta());
            }
        }
        let (main_x, main_w) = if wide { (0, w) } else { (list_w + 1, w - list_w - 1) };
        let main = Rect::new(body.x + main_x as u16, body.y, main_w as u16, body.height);
        let mut head = color::header();
        if rv.in_main {
            head = head.add_modifier(Modifier::UNDERLINED);
        }
        // Scrolled by the wheel: by the lines it turned, as far as the arrows
        // take it; and since, until a key, where the wheel left it, the
        // cursor in sight or not, the part the list's cursor is on wherever
        // it is.
        self.scrolls_in(main, Wheeled::ReviewMain);
        let turned = self.turned(Wheeled::ReviewMain);
        let free = self.free(Wheeled::ReviewMain);
        // A patch of the stack the work makes, looked at closer: as the work
        // leaves it, a row at a time, the cursor on one, what is written on
        // it under it.
        if rv.in_main
            && rv.in_patch
            && let Some((at, p)) = self.review_patch()
        {
            let pw = main_w.saturating_sub(1);
            let all = self.review_rows(&p, pw);
            let editing = self.writing().is_some_and(|e| e.pane == Focus::Diff);
            let edit_h = if editing { self.editor_height(main, Focus::Diff) } else { 0 };
            let room = rows.saturating_sub(edit_h).max(1);
            // Gone to from the stack diff: the cursor on the line Enter was on
            // there, a third of the way down.
            let (cur, mut row_top) = match rv.goto {
                Some(m) => {
                    let cur = row_of(&all, crate::ops::line_of_mail(&p, m));
                    (cur, cur.saturating_sub(room / 3))
                }
                None => (rv.row.min(all.len().saturating_sub(1)), rv.row_top),
            };
            let last = all.len().saturating_sub(room);
            if let Some(n) = turned {
                row_top = moved(row_top, n, last);
            }
            // Written on, the box goes under the line or the comment, as in
            // the main view, in sight.
            let under = editing.then(|| self.editor_anchor_in(&all).unwrap_or(cur));
            let shown = under.unwrap_or(cur);
            let mut top = row_top.min(shown);
            if shown >= top + room {
                top = shown + 1 - room;
            }
            if free && !editing {
                top = row_top.min(last);
            }
            if let Some(r) = &mut self.reviewing {
                (r.row, r.row_top, r.main_w, r.goto) = (cur, top, pw, None);
            }
            // Longer than the pane, its scroll bar beside the rows under its
            // title, in its last column, which its rows, built a column short
            // of the pane, leave blank.
            self.pane_bar(Wheeled::ReviewMain, main, buf.area, (1, rows), Shown { top, rows: room, all: all.len() });
            put(buf, main, 0, 0, main_w, &fit(&format!(" patch {} as the work leaves it: {}", at + 1, p.title()), main_w), head);
            let (mut y, mut cursor) = (1, None);
            for (i, row) in all.iter().enumerate().skip(top) {
                if y > rows {
                    break;
                }
                put_parts(buf, main, 0, y, main_w, &row_parts(row, i == cur && !editing));
                y += 1;
                if Some(i) == under {
                    cursor = self.draw_editor(buf, main, y, Focus::Diff);
                    y += edit_h;
                }
            }
            return cursor;
        }
        let (said, view, at) = self.review_view(lines.get(rv.sel), main_w.saturating_sub(1));
        put(buf, main, 0, 0, main_w, &fit(&said, main_w), head);
        // Where the part is, unless scrolled by hand, or by the wheel. Your
        // answer to its reply being written: the box under the thread, in
        // sight, what follows the thread below it.
        let edit_h = self.editor_height(main, Focus::Diff);
        let mut scroll = if rv.in_main || free { rv.scroll } else { at.start.saturating_sub(rows / 3) };
        if edit_h > 0 {
            scroll = scroll.max((at.end + edit_h).saturating_sub(rows)).min(at.start);
        }
        // The stack diff looked at closer: the cursor on an entry, in sight.
        let picked = (rv.in_main && self.review_on_diff())
            .then(|| self.review_diff(main_w.saturating_sub(1)))
            .flatten()
            .and_then(|v| v.spans.get(v.stop_of(rv.entry)).cloned());
        if let Some(n) = turned {
            let last = if picked.is_some() { view.len().saturating_sub(rows) } else { view.len().saturating_sub(1) };
            scroll = moved(scroll, n, last);
        }
        if let Some(c) = picked.as_ref().filter(|_| !free) {
            scroll = scroll.min(c.start).max(c.end.saturating_sub(rows));
        }
        let scroll = scroll.min(view.len().saturating_sub(1));
        if let Some(r) = &mut self.reviewing {
            r.scroll = scroll;
        }
        // Longer than the pane, or scrolled past its first line, its scroll
        // bar beside the lines under its title, in its last column, which
        // its lines, built a column short of the pane, leave blank.
        self.pane_bar(Wheeled::ReviewMain, main, buf.area, (1, rows), Shown { top: scroll, rows, all: view.len() });
        for (i, parts) in view.iter().enumerate().skip(scroll) {
            let y = 1 + i - scroll + if i >= at.end { edit_h } else { 0 };
            if y > rows {
                break;
            }
            match &picked {
                Some(c) if c.contains(&i) => put_parts(buf, main, 0, y, main_w, &on_cursor_parts(parts)),
                _ => put_parts(buf, main, 0, y, main_w, parts),
            }
        }
        if edit_h == 0 {
            return None;
        }
        self.draw_editor(buf, main, 1 + at.end.saturating_sub(scroll), Focus::Diff)
    }

    /// A key in the review: ↑↓ through its parts (or the other pane, → in
    /// it, ← out: on the stack diff, Enter goes to the line in its patch as
    /// the work leaves it, to comment on it), Enter to accept, or put back
    /// to review what is accepted (Space's menu rejects), Esc to save, throw
    /// away or close it.
    pub(super) fn review_key(&mut self, k: KeyEvent) -> Effect {
        let lines = self.review_lines();
        let page = self.page();
        // A patch looked at closer: how many rows it has; gone to from the
        // stack diff and not drawn since, the row Enter went to there, which
        // the keys move from, as the drawing would put the cursor on it.
        let looked = self.reviewing.as_ref().is_some_and(|rv| rv.in_main && rv.in_patch);
        let goto = self.reviewing.as_ref().and_then(|rv| rv.goto);
        let rows = self.review_patch().filter(|_| looked).map(|(_, p)| {
            let rows = self.review_rows(&p, self.reviewing.as_ref().map_or(80, |rv| rv.main_w));
            (rows.len(), goto.map(|m| row_of(&rows, crate::ops::line_of_mail(&p, m))))
        });
        let (patch_rows, went) = (rows.map(|(n, _)| n), rows.and_then(|(_, at)| at));
        // The stack diff: where → from the list puts the cursor in it; looked
        // at closer, where Enter on each of its entries goes.
        let in_main = self.reviewing.as_ref().is_some_and(|rv| rv.in_main);
        let on_diff = in_main && !looked && self.review_on_diff();
        let diff = (on_diff || (!in_main && k.code == KeyCode::Right)).then(|| self.review_diff(80)).flatten();
        let entry = self.reviewing.as_ref().and_then(|rv| lines.get(rv.sel)).and_then(|l| l.entry);
        let first = diff.as_ref().map_or(0, |v| v.first_change(entry));
        let stops = diff.filter(|_| on_diff);
        // A pull's comments: all accepted, nothing to mark.
        let pulled = self.review_pulled();
        // The operation of the line the cursor is on, if it is one; those of
        // the change, and whether every one is accepted, for their title.
        let op = self.review_op();
        let ops = self.review_ops();
        let op_keys: Vec<String> = ops.iter().flat_map(|o| o.keys()).collect();
        let all_accepted = self.reviewing.as_ref().zip(ops.as_ref()).is_some_and(|(rv, o)| matches!(o.tally(&rv.marks), (y, 0, 0) if y > 0));
        let Some(rv) = self.reviewing.as_mut() else {
            self.screen = Screen::Main;
            return Effect::None;
        };
        let parts: Vec<usize> = lines.iter().enumerate().filter(|(_, l)| l.selectable()).map(|(i, _)| i).collect();
        let step = |from: usize, down: bool, n: usize| -> usize {
            let mut at = parts.iter().position(|&i| i == from).unwrap_or(0);
            for _ in 0..n {
                at = if down { (at + 1).min(parts.len().saturating_sub(1)) } else { at.saturating_sub(1) };
            }
            parts.get(at).copied().unwrap_or(from)
        };
        match (rv.in_main, k.code) {
            (_, KeyCode::Esc) => {
                self.mode = Mode::Menu(self.review_dialog());
            }
            // On a patch looked at closer, Enter writes on the line, and ↑↓
            // go from row to row.
            (true, KeyCode::Enter) if patch_rows.is_some() => self.review_comment(),
            (true, code @ (KeyCode::Up | KeyCode::Down | KeyCode::PageUp | KeyCode::PageDown | KeyCode::Home | KeyCode::End))
                if patch_rows.is_some() =>
            {
                if let Some(at) = went {
                    (rv.row, rv.goto) = (at, None);
                }
                let last = patch_rows.unwrap_or(1).saturating_sub(1);
                rv.row = match code {
                    KeyCode::Down => (rv.row + 1).min(last),
                    KeyCode::Up => rv.row.saturating_sub(1),
                    KeyCode::PageDown => (rv.row + page).min(last),
                    KeyCode::PageUp => rv.row.saturating_sub(page),
                    KeyCode::Home => 0,
                    _ => last,
                };
            }
            // On the stack diff looked at closer, ↑↓ go from line to line, and
            // Enter to the line in its patch as the work leaves it: the cursor
            // on it there, to comment on it.
            (true, KeyCode::Enter) if stops.is_some() => {
                let to = stops.as_ref().and_then(|v| v.goes.get(v.stop_of(rv.entry)).copied().flatten());
                match to.and_then(|(e, m)| Some((lines.iter().position(|l| l.entry == Some(e))?, m))) {
                    Some((sel, m)) => {
                        rv.sel = sel;
                        rv.in_patch = true;
                        (rv.row, rv.row_top, rv.goto) = (0, 0, m);
                    }
                    None => self.status = "this line is of a patch dropped, or of the base: there is no line of a patch to go to".into(),
                }
            }
            // From stop to stop: a line of the diff, or, side by side, a
            // pair of the two versions' lines.
            (true, code @ (KeyCode::Up | KeyCode::Down | KeyCode::PageUp | KeyCode::PageDown | KeyCode::Home | KeyCode::End))
                if stops.is_some() =>
            {
                if let Some(v) = &stops {
                    let (at, last) = (v.stop_of(rv.entry), v.keys.len().saturating_sub(1));
                    rv.entry = v.line_of(match code {
                        KeyCode::Down => (at + 1).min(last),
                        KeyCode::Up => at.saturating_sub(1),
                        KeyCode::PageDown => (at + page).min(last),
                        KeyCode::PageUp => at.saturating_sub(page),
                        KeyCode::Home => 0,
                        _ => last,
                    });
                }
            }
            (_, KeyCode::Enter) => {
                if let Some(y) = lines.get(rv.sel).and_then(|l| l.yours.clone()) {
                    // What you wrote: written again.
                    self.review_write_yours(y);
                } else if let Some(k) = lines.get(rv.sel).and_then(|l| l.asking) {
                    // A comment that asked: closed with the review, or kept.
                    let close = rv.marks.get(&crate::held::asking_key(k)) != Some(&true);
                    self.mark_asking(k, close);
                } else if let Some(k) = lines.get(rv.sel).and_then(|l| l.tool) {
                    // A tool the agent could not use: allowed from now on.
                    self.tool_enter(k);
                } else if let Some(id) = lines.get(rv.sel).and_then(|l| l.answer.clone()) {
                    // An answer made on the stack before the change: kept.
                    self.mark_answer(&id, AnswerMark::Keep);
                } else if let Some(p) = lines.get(rv.sel).and_then(|l| l.part).filter(|p| !pulled || *p == Part::Stack) {
                    // An operation of the change: accepted, or back to
                    // review; their title, every one of them.
                    match (p, op) {
                        (Part::Stack, Some((_, false))) => self.status = NOTHING_TO_MARK.into(),
                        (Part::Stack, Some((key, true))) if rv.marks.get(&key) == Some(&true) => {
                            rv.marks.remove(&key);
                        }
                        (Part::Stack, Some((key, true))) => self.mark_op(&key, true),
                        (Part::Stack, None) if all_accepted => {
                            for key in op_keys {
                                rv.marks.remove(&key);
                            }
                        }
                        _ if rv.marks.get(&p.key()) == Some(&true) => {
                            rv.marks.remove(&p.key());
                        }
                        _ => self.mark(p, true),
                    }
                }
            }
            // → into the pane on the right, to scroll it, or the stack diff
            // from the first line that changed of the patch the cursor is on;
            // → again, wide, the list out of sight; ← back. Too narrow for
            // both side by side, it is wide at once.
            (false, KeyCode::Right) => {
                rv.in_main = true;
                rv.in_patch = false;
                rv.wide = self.body_w < 60;
                (rv.row, rv.row_top, rv.entry) = (0, 0, first);
            }
            (true, KeyCode::Right) => rv.wide = true,
            (true, KeyCode::Left) if rv.wide && self.body_w >= 60 => rv.wide = false,
            // From the patch, back to the stack diff, its cursor where it was.
            (true, KeyCode::Left) if rv.in_patch => rv.in_patch = false,
            (true, KeyCode::Left) => {
                rv.in_main = false;
                rv.wide = false;
            }
            (false, code) => {
                rv.sel = match code {
                    KeyCode::Down => step(rv.sel, true, 1),
                    KeyCode::Up => step(rv.sel, false, 1),
                    KeyCode::PageDown => step(rv.sel, true, page),
                    KeyCode::PageUp => step(rv.sel, false, page),
                    KeyCode::Home => parts.first().copied().unwrap_or(0),
                    KeyCode::End => parts.last().copied().unwrap_or(0),
                    _ => rv.sel,
                };
            }
            (true, code) => {
                rv.scroll = match code {
                    KeyCode::Down => rv.scroll + 1,
                    KeyCode::Up => rv.scroll.saturating_sub(1),
                    KeyCode::PageDown => rv.scroll + page,
                    KeyCode::PageUp => rv.scroll.saturating_sub(page),
                    KeyCode::Home => 0,
                    _ => rv.scroll,
                };
            }
        }
        Effect::None
    }

    /// Part `p` marked accepted, or rejected: the change of the stack, every
    /// one of its operations at once (`mark_ops`). A comment is not accepted
    /// while the change it goes with is rejected, every operation of it.
    pub(super) fn mark(&mut self, p: Part, accept: bool) {
        if !accept && self.part_fixed(p) {
            self.status = crate::held::FILED.into();
            return;
        }
        if p == Part::Stack {
            let keys = self.review_ops().map_or_else(|| vec![Part::Stack.key()], |o| o.keys());
            self.mark_ops(&keys, accept);
            return;
        }
        let whose = self.reviewing.as_ref().and_then(|rv| self.db.run(&rv.run)).map_or("the agent's change", ask::whose_change);
        if accept && self.reviewing.as_ref().is_some_and(|rv| self.change_rejected(&rv.marks)) {
            self.status = format!("{whose} of the stack is rejected, and the comments with it: accept the change first");
            return;
        }
        if let Some(rv) = self.reviewing.as_mut() {
            rv.marks.insert(p.key(), accept);
        }
    }

    /// Operations `keys` of the change marked accepted, or rejected, as
    /// they are: what they need is not looked at. Every operation rejected,
    /// the comments of the work are rejected with them; one accepted again,
    /// they are as they were.
    fn mark_ops(&mut self, keys: &[String], accept: bool) {
        // A pull's comments are the reviewers': filed whatever becomes of
        // its change.
        let pulled = self.review_pulled();
        let comments: Vec<Part> = self.review_parts().into_iter().filter(|p| *p != Part::Stack && !pulled).collect();
        let Some(was) = self.reviewing.as_ref().map(|rv| self.change_rejected(&rv.marks)) else { return };
        let mut marks = self.reviewing.as_ref().map(|rv| rv.marks.clone()).unwrap_or_default();
        for k in keys {
            marks.insert(k.clone(), accept);
        }
        let now = self.change_rejected(&marks);
        let Some(rv) = self.reviewing.as_mut() else { return };
        if now && !was {
            rv.before = Some(comments.iter().filter_map(|c| Some((c.key(), *marks.get(&c.key())?))).collect());
            for c in &comments {
                marks.insert(c.key(), false);
            }
        }
        if was && !now {
            let before = rv.before.take().unwrap_or_default();
            for c in &comments {
                match before.get(&c.key()) {
                    Some(m) => marks.insert(c.key(), *m),
                    None => marks.remove(&c.key()),
                };
            }
        }
        rv.marks = marks;
    }

    /// Operation `key` of the change marked accepted, or rejected, as what
    /// it needs allows, as what is stored of it says, at once: nothing is
    /// checked. Rejecting is refused while an operation that is accepted or
    /// to review needs it, and accepting while one it needs is rejected: the
    /// status line says which. Its patch as it was, or one rejected already,
    /// not applying once it is marked, the operations that go with it are
    /// marked with it, once asked.
    pub(super) fn mark_op(&mut self, key: &str, accept: bool) {
        let (Some(rv), Some(ops)) = (self.reviewing.as_ref(), self.review_ops()) else { return };
        let Some(o) = ops.op(key) else { return };
        if !o.marked() {
            self.status = NOTHING_TO_MARK.into();
            return;
        }
        let now = ops.mark(&rv.marks, o);
        if now == Some(accept) {
            self.status = format!("this operation is {} already", if accept { "accepted" } else { "rejected" });
            return;
        }
        // One to review accepted: the stack stands as it did.
        if accept && now.is_none() {
            self.mark_ops(&[key.to_string()], true);
            return;
        }
        let Some(needs) = self.review_needs() else { return };
        let stands = ops.standing(&rv.marks);
        let plan = if accept { ops.accept(needs, &stands, key) } else { ops.reject(needs, &stands, key) };
        match plan {
            Ok(with) if with.is_empty() => self.mark_ops(&[key.to_string()], accept),
            Ok(with) => self.mode = Mode::Menu(with_ask(&ops, key, &with, accept)),
            Err(r) => self.status = ops.refused_said(key, accept, &r),
        }
    }

    /// Operations `keys`, the one marked and those that go with it, marked
    /// accepted, or rejected, together, as the question before asked, from
    /// what is stored of what each needs: whether the stack they leave
    /// applies is checked as the review closes.
    pub(super) fn mark_ops_with(&mut self, keys: &[String], accept: bool) {
        self.mark_ops(keys, accept);
    }

    /// The comment the ask was about at place `k` marked to close with the
    /// review (`close`), or to keep. Closed, its replies go with it: they
    /// are accepted (while the change they go with is not rejected), and
    /// are as they were once it is kept again.
    pub(super) fn mark_asking(&mut self, k: usize, close: bool) {
        let Some((c, i)) = self.reviewing.as_ref().and_then(|rv| self.db.run(&rv.run)).and_then(|r| crate::held::asking_of(r).get(k).cloned()) else {
            return;
        };
        let parts = self.review_parts();
        let Some((_, h)) = self.reviewing.as_ref().and_then(|rv| self.work(&rv.run)) else { return };
        let replies: Vec<Part> = h
            .replies
            .iter()
            .enumerate()
            .filter(|(_, rp)| rp.change == c && rp.id == i)
            .map(|(j, _)| Part::Reply(j))
            .filter(|p| parts.contains(p))
            .collect();
        let rejected = self.reviewing.as_ref().is_some_and(|rv| self.change_rejected(&rv.marks));
        let Some(rv) = self.reviewing.as_mut() else { return };
        if close {
            rv.marks.insert(crate::held::asking_key(k), true);
            if !rejected {
                for p in &replies {
                    let was = rv.marks.get(&p.key()).copied();
                    rv.asked_before.entry(p.key()).or_insert(was);
                    rv.marks.insert(p.key(), true);
                }
            }
        } else {
            rv.marks.remove(&crate::held::asking_key(k));
            for p in &replies {
                match rv.asked_before.remove(&p.key()) {
                    Some(Some(m)) => {
                        rv.marks.insert(p.key(), m);
                    }
                    Some(None) => {
                        rv.marks.remove(&p.key());
                    }
                    None => {}
                }
            }
        }
    }

    /// The review's keys, where the cursor is: the list, or the pane on its
    /// right, wide or not.
    pub(super) fn review_hint(&self) -> String {
        let enter = self.review_enter();
        // With what you wrote in it, the work goes back to the agent.
        let esc = if self.review_yours() > (0, 0) { "esc: send back, save or discard" } else { "esc: save, discard or close" };
        // What Delete does to the list's line the cursor is on, wherever the
        // cursor is, as the menu does.
        let delete = self.delete_hint().map_or(String::new(), |d| format!("{d} · "));
        // On a line of a message that has links: the key that opens them.
        let open = self.open_hint();
        // The stack diff of the change on the right: the view of its
        // interdiffs.
        let view = self.inter_keys();
        match &self.reviewing {
            Some(rv) if rv.in_main && rv.in_patch && self.review_patch().is_some() => {
                let enter = if self.review_writable() { "enter: comment · " } else { "" };
                let side = if rv.wide { "←: narrower" } else { "→: wider · ←: back to the stack diff" };
                format!("↑↓: a line · {enter}{open}{delete}{side} · {esc}")
            }
            Some(rv) if rv.in_main && self.review_on_diff() => {
                let side = if rv.wide { "←: narrower" } else { "→: wider · ←: back to the list" };
                format!("↑↓: a line · enter: go to it in the patch · {open}{delete}{side} · {view}{esc}")
            }
            Some(rv) if rv.in_main && rv.wide => format!("↑↓: scroll · {enter} · {delete}←: narrower · {esc}"),
            Some(rv) if rv.in_main => format!("↑↓: scroll · {enter} · {delete}→: wider · ←: back to the list · {esc}"),
            _ => format!("↑↓ choose · {enter} · {delete}→: look closer · space: menu · {view}{esc}"),
        }
    }

    /// What Enter does on the review's line the cursor is on, said.
    pub(super) fn review_enter(&self) -> &'static str {
        let Some(rv) = &self.reviewing else { return "enter: accept" };
        let lines = self.review_lines();
        let Some(line) = lines.get(rv.sel) else { return "enter: accept" };
        if line.yours.is_some() {
            return "enter: edit it";
        }
        if line.report.is_some() {
            return "enter: nothing, what the linter reported is to read";
        }
        if line.stacked {
            return "enter: nothing, stacked's comment is to read";
        }
        if let Some(k) = line.following {
            return match self.review_following().get(k).is_some_and(|f| f.run.is_some()) {
                true => "enter: nothing, the follow-up runs",
                false => "enter: nothing, the follow-up waits its turn",
            };
        }
        if line.found.is_some() {
            return "enter: nothing, what the build and test found is to read";
        }
        if let Some(k) = line.tool {
            return match self.tool_item(k) {
                Some(t) if !t.offered() => "enter: nothing, this tool is noted, not offered",
                _ if self.tool_local().is_ok() => "enter: allow it from now on",
                _ => "enter: allow it for this work",
            };
        }
        if let Some(id) = &line.answer {
            return match self.answer_items().into_iter().find(|(k, _)| k == id) {
                Some((_, g)) if g.markable() => "enter: keep it",
                _ => "enter: nothing, this answer has nothing to mark",
            };
        }
        // An operation of the change, by its own mark; their title, by
        // theirs, every one accepted or not.
        let ops = self.review_ops();
        let op = ops.as_ref().and_then(|o| Self::line_op(o, line));
        let all = ops.as_ref().is_some_and(|o| matches!(o.tally(&rv.marks), (y, 0, 0) if y > 0));
        match (line.asking, line.part) {
            (Some(k), _) if rv.marks.get(&crate::held::asking_key(k)) == Some(&true) => "enter: keep it open",
            (Some(_), _) => "enter: close it with the review",
            (None, Some(p)) if self.part_fixed(p) => "enter: nothing, what was pulled is filed as the review closes",
            (None, Some(Part::Stack)) => match op {
                Some(o) if !o.marked() => "enter: nothing, the change leaves this patch as it is",
                Some(o) if rv.marks.get(&o.key) == Some(&true) => "enter: to review again",
                Some(_) => "enter: accept",
                None if all => "enter: to review again",
                None => "enter: accept",
            },
            (None, Some(p)) if rv.marks.get(&p.key()) == Some(&true) => "enter: to review again",
            _ => "enter: accept",
        }
    }

    /// Every reply of the review marked accepted, or rejected; or, not
    /// `replies`, every comment the work opened.
    pub(super) fn mark_all(&mut self, replies: bool, accept: bool) {
        let these: Vec<Part> = self
            .review_parts()
            .into_iter()
            .filter(|p| match p {
                Part::Reply(_) => replies,
                Part::Opened(_) => !replies,
                Part::Stack => false,
                #[cfg(feature = "try-push")]
                Part::Task(_) => false,
            })
            .collect();
        for p in these {
            self.mark(p, accept);
        }
    }

    /// Every task of the try push work reviewed marked accepted, or
    /// rejected.
    #[cfg(feature = "try-push")]
    pub(super) fn mark_tasks(&mut self, accept: bool) {
        for p in self.review_parts().into_iter().filter(|p| matches!(p, Part::Task(_))) {
            self.mark(p, accept);
        }
    }

    /// What is marked, by kind of part: a line for each, in parts. Each
    /// count is marked as the list marks a part (`mark_of`): what is
    /// accepted, allowed or kept, `✓` in the colour of a part accepted;
    /// what is rejected, refused or to close, `✗` in that of one rejected;
    /// what is to review or noted, `·`. The rest is dim rather than in the
    /// note's colour, which reads too close to the green.
    pub(super) fn review_summary(&self) -> Vec<Parts> {
        let Some(rv) = &self.reviewing else { return vec![] };
        let parts = self.review_parts();
        let mark = |p: &Part| rv.marks.get(&p.key()).copied();
        let rest = color::dim();
        // What a part, or a count of them, stands at, marked; neither
        // accepted nor rejected, in the rest's style.
        let marked = |m: Option<bool>, what: String| {
            let (sign, st) = mark_of(m);
            (format!("{sign} {what}"), if m.is_some() { st } else { rest })
        };
        // A line: what it counts, then each count but none, commas between.
        let counted = |of: &str, counts: &[(usize, &str, Option<bool>)]| -> Parts {
            let mut line = vec![(format!("{of}: "), rest)];
            for (k, &(n, word, m)) in counts.iter().filter(|(n, ..)| *n > 0).enumerate() {
                if k > 0 {
                    line.push((", ".into(), rest));
                }
                line.push(marked(m, format!("{n} {word}")));
            }
            line
        };
        let mut out: Vec<Parts> = Vec::new();
        // Nothing brought back: only the comments that asked, to say.
        if self.work(&rv.run).is_some_and(|(_, h)| h.is_empty()) {
            out.push(vec![(crate::held::NOTHING_BACK.to_string(), rest)]);
        }
        // What the linter reported and did not fix: read, not marked.
        if self.work(&rv.run).is_some_and(|(_, h)| h.lint.as_ref().is_some_and(|l| !l.errors.is_empty())) {
            out.push(vec![("what the linter reported: to read, nothing to accept or reject".to_string(), rest)]);
        }
        if self.work(&rv.run).is_some_and(|(r, _)| r.reconcile.is_some()) {
            out.push(vec![("stacked's comment: to read, nothing to accept or reject".to_string(), rest)]);
        }
        if let Some((_, h)) = self.work(&rv.run).filter(|(_, h)| !h.tools.is_empty()) {
            let of = |a: Allow| h.tools.iter().enumerate().filter(|(k, t)| t.offered() && crate::refusals::state(&rv.marks, *k) == a).count();
            let noted = h.tools.iter().filter(|t| !t.offered()).count();
            out.push(counted(
                crate::refusals::HEADING,
                &[
                    (of(Allow::Always), Allow::Always.said(), Some(true)),
                    (of(Allow::Work), Allow::Work.said(), Some(true)),
                    (of(Allow::Refused), Allow::Refused.said(), Some(false)),
                    (noted, "noted", None),
                ],
            ));
        }
        // The comments that asked: kept, as a part accepted; to close, as one
        // rejected.
        let asking = self.asking_open();
        if !asking.is_empty() {
            let closing = asking.iter().filter(|(k, _)| rv.marks.get(&crate::held::asking_key(*k)) == Some(&true)).count();
            let of = if asking.len() == 1 { "asking comment" } else { "asking comments" };
            out.push(counted(of, &[(closing, "to close", Some(false)), (asking.len() - closing, "kept", Some(true))]));
        }
        // The stack operations, each marked on its own: a count of each mark;
        // with none to mark, the change as a whole.
        if parts.contains(&Part::Stack) {
            match self.review_ops().filter(|o| !o.whole()) {
                Some(ops) => {
                    let (yes, no, open) = ops.counts(&rv.marks);
                    out.push(counted("stack operations", &[(yes, "accepted", Some(true)), (no, "rejected", Some(false)), (open, "to review", None)]));
                }
                None => {
                    let m = mark(&Part::Stack);
                    let said = match m {
                        Some(true) => "accepted",
                        Some(false) => "rejected",
                        None => "to review",
                    };
                    out.push(vec![("the change of the stack: ".into(), rest), marked(m, said.into())]);
                }
            }
        }
        // The answers made on the stack before the change: kept, as a part
        // accepted is counted; stopped, as one rejected; restarted, and those
        // not marked yet, as neither; those no longer going, which need no
        // mark, as neither too.
        let answers = self.answer_items();
        if !answers.is_empty() {
            let of = |m: Option<AnswerMark>| answers.iter().filter(|(id, g)| g.markable() && crate::held::answer_mark(&rv.marks, id) == m).count();
            let now = |going: Going| answers.iter().filter(|(_, g)| *g == going).count();
            let counts = [
                (of(Some(AnswerMark::Keep)), "✓", "kept", color::add()),
                (of(Some(AnswerMark::Restart)), "↻", "restarted", rest),
                (of(Some(AnswerMark::Stop)), "✗", "stopped", color::del()),
                (of(None), "·", "to mark", rest),
                (now(Going::Failed), "·", "failed", rest),
                (now(Going::Reviewed), "·", "reviewed", rest),
            ];
            let mut line = vec![(format!("{ANSWERS}: "), rest)];
            for (k, (n, sign, word, st)) in counts.into_iter().filter(|(n, ..)| *n > 0).enumerate() {
                if k > 0 {
                    line.push((", ".into(), rest));
                }
                line.push((format!("{sign} {n} {word}"), st));
            }
            out.push(line);
        }
        // Each kind of part apart: a try push's tasks, the comments
        // opened, the replies.
        let group = |p: &Part| match p {
            Part::Opened(_) => 0,
            Part::Reply(_) => 1,
            #[cfg(feature = "try-push")]
            Part::Task(_) => 2,
            Part::Stack => 3,
        };
        for (what, of) in [("tasks", 2), ("comments opened", 0), ("replies", 1)] {
            let these: Vec<&Part> = parts.iter().filter(|p| group(p) == of).collect();
            if these.is_empty() {
                continue;
            }
            let count = |m: Option<bool>| these.iter().filter(|p| mark(p) == m).count();
            out.push(counted(what, &[(count(Some(true)), "accepted", Some(true)), (count(Some(false)), "rejected", Some(false)), (count(None), "to review", None)]));
        }
        out
    }

    /// What Esc asks in the review: what is marked, said; then save the
    /// marks, throw away those made since it was opened, or close it.
    pub(super) fn review_dialog(&self) -> Menu {
        // Sent back from the dialog the item opens, which sets how; not
        // while a follow-up asked from the review goes.
        let run = self.reviewing.as_ref().map(|rv| rv.run.clone()).unwrap_or_default();
        let going = self.review_following();
        let back = Item::new(SEND_BACK, Action::AskAgent(AskOf::SendBack(run.clone()))).when(going.is_empty());
        let grayed = |m: &mut Menu| {
            if !going.is_empty() {
                let said = format!("{SEND_BACK} is grayed while the follow-up you asked from this review goes: what it brings back comes into this review once it lands.");
                m.note = Some(m.note.take().map_or(said.clone(), |n| format!("{n}\n{said}")));
            }
        };
        // What you wrote in it: it goes back to the agent, neither accepted
        // nor rejected.
        let (comments, answers) = self.review_yours();
        if comments + answers > 0 {
            let items = vec![
                back,
                Item::new("Save", Action::ReviewSave),
                Item::new("Discard", Action::ReviewDiscard),
                Item::line(),
                Item::new("Keep reviewing", Action::Cancel),
            ];
            let mut m = Menu::new("your review of the agent's work", items, None);
            let mut what = Vec::new();
            // A try push's: your answers to why it selected its tasks, apart.
            #[cfg(feature = "try-push")]
            let tasks = self.reviewing.as_ref().and_then(|rv| self.work(&rv.run)).map_or(0, |(_, h)| h.tasks.iter().filter(|t| t.answer.is_some()).count());
            #[cfg(not(feature = "try-push"))]
            let tasks = 0;
            let tools = self.reviewing.as_ref().and_then(|rv| self.work(&rv.run)).map_or(0, |(_, h)| h.tools.iter().filter(|t| t.answer.is_some()).count());
            for (n, one, many) in [
                (comments, "1 comment", "comments"),
                (answers.saturating_sub(tasks + tools), "1 answer to the agent's replies", "answers to the agent's replies"),
                (tasks, "1 answer to why the agent selected a task", "answers to why the agent selected its tasks"),
                (tools, "1 answer to what the agent said of a tool it could not use", "answers to what the agent said of the tools it could not use"),
            ] {
                match n {
                    0 => {}
                    1 => what.push(one.to_string()),
                    n => what.push(format!("{n} {many}")),
                }
            }
            m.note = Some(format!(
                "You wrote {} in this review, so the work is not accepted: it goes back to the agent, which is asked again, starting from this work.\n{SENT_HOW}",
                what.join(" and "),
            ));
            grayed(&mut m);
            m.say_parts(self.review_summary());
            return m;
        }
        let parts = self.review_parts();
        let done = self.reviewing.as_ref().is_some_and(|rv| parts.iter().all(|p| self.part_marked(&rv.marks, *p)));
        // Closing with the change accepted waits for each answer made on the
        // stack before it to be marked.
        let waits = |rest: Option<bool>| self.waits_for_answers(rest);
        // A pull's: not with a patch whose conflicts it marked in the stack.
        let marks = self.reviewing.as_ref().map(|rv| rv.marks.clone()).unwrap_or_default();
        let conflicts = |rest: Option<bool>| self.marked_closing(&marks, rest);
        // Every part marked, nothing remains for the others to mark.
        let mut items = vec![
            Item::new("Save", Action::ReviewSave),
            Item::new("Discard", Action::ReviewDiscard),
            Item::new("Close review", if self.review_close_menu().is_some() { Action::AskClose } else { Action::ReviewClose(None) })
                .when(done && !waits(None) && conflicts(None).is_none()),
        ];
        // A tool allowed: the work may go back to the agent, to try again
        // with it, though you wrote nothing, as its dialog sets.
        let tools = self.review_writable() && self.tools_allowed();
        if tools {
            items.push(back);
        }
        // A pull's change holding a patch whose conflicts it marked: an agent
        // may rework it, from the stack the pull makes.
        if conflicts(None).is_some() {
            items.push(Item::new(SEND_BACK, Action::AskAgent(AskOf::SendBack(run.clone()))));
        }
        // What remains, accepted or rejected together, a section of its
        // own; over it, whether the comments that asked close with it, and
        // whether HEAD follows the stack.
        items.push(Item::line());
        items.extend(self.asking_row());
        items.extend(self.head_row());
        // A pull's with no change: every part accepted already, none to
        // reject; with one, what remains is its operations.
        if !self.review_pulled() || self.review_ops().is_some() {
            items.extend([
                Item::new("Accept all remaining and Close", Action::AskReviewClose(true)).when(!done && !waits(Some(true)) && conflicts(Some(true)).is_none()),
                Item::new("Reject all remaining and Close", Action::AskReviewClose(false)).when(!done && !waits(Some(false)) && conflicts(Some(false)).is_none()),
                Item::line(),
            ]);
        }
        items.push(Item::new("Keep reviewing", Action::Cancel));
        let run = self.reviewing.as_ref().and_then(|rv| self.db.run(&rv.run));
        let of = match run {
            Some(r) if crate::held::pulled(r) && r.held.as_ref().is_some_and(|h| h.stack.is_some()) => "the pull from Phabricator",
            Some(r) if crate::held::pulled(r) => "the comments pulled",
            Some(r) if crate::held::by_hand(r) => "your edit by hand",
            Some(r) if crate::held::linted(r) => "the linter's work",
            Some(r) if crate::reconcile::is(r) => "the reconcile with HEAD",
            _ => "the agent's work",
        };
        let mut m = Menu::new(format!("your review of {of}"), items, None);
        m.say_parts(self.review_summary());
        // A closing grayed for the conflicts a pull marked: which, and what
        // to do.
        if let Some(why) = conflicts(None).or_else(|| conflicts(Some(true))) {
            let said = format!("The review closes with the change accepted only once no patch of it holds a conflict the pull marked: {why}.");
            m.note = Some(m.note.take().map_or(said.clone(), |n| format!("{n}\n{said}")));
        }
        // A closing grayed for the answers not marked yet: which they are,
        // under the rest.
        if (done && waits(None)) || (!done && (waits(Some(true)) || waits(Some(false)))) {
            let said = self.answers_wait_said();
            m.note = Some(m.note.take().map_or(said.clone(), |n| format!("{n}\n{said}")));
        }
        // A follow-up going: closing the review stops it.
        if let Some(f) = going.first() {
            let said = match going.len() {
                1 => format!(
                    "The follow-up you asked from this review is {}: what it brings back comes into this review once it lands. Closing the review stops it.",
                    following_said(f)
                ),
                n => format!("The {n} follow-ups you asked from this review go: what they bring back comes into this review once they land. Closing the review stops them."),
            };
            m.note = Some(m.note.take().map_or(said.clone(), |n| format!("{n}\n{said}")));
        }
        if tools {
            grayed(&mut m);
        }
        m
    }

    /// Why closing the review with the change accepted is grayed, while an
    /// answer made on the stack before the change is not marked: which are
    /// not, each by what it was asked about and where it stands.
    fn answers_wait_said(&self) -> String {
        let unmarked = self.reviewing.as_ref().map(|rv| self.unmarked_in(rv)).unwrap_or_default();
        let each: Vec<String> = unmarked
            .iter()
            .filter_map(|id| self.db.run(id))
            .map(|a| format!("{} ({})", crate::held::answer_about(&self.db, a), crate::held::answer_state(a)))
            .collect();
        format!(
            "The review closes with the change accepted only once each answer made on the stack before it is marked keep, restart or stop. Not marked yet: {}.",
            each.join("; ")
        )
    }

    /// The row picking whether the comments that asked are kept open or
    /// closed as the review closes, every one of them: picked as the review
    /// has them, `close` if every one is marked to close. None without
    /// such comments.
    fn asking_row(&self) -> Option<Item> {
        let rv = self.reviewing.as_ref()?;
        let asking = self.asking_open();
        if asking.is_empty() {
            return None;
        }
        let close = asking.iter().all(|(k, _)| rv.marks.get(&crate::held::asking_key(*k)) == Some(&true));
        let (label, these) = if asking.len() == 1 { ("Asking comment", "the comment") } else { ("Asking comments", "the comments") };
        let list = vec![
            Choice::new("keep", "keep", format!("{these} kept open when the review closes"), None),
            Choice::new(
                "close",
                "close",
                format!("{these} closed when the review closes, done with, and the replies that go with them accepted"),
                None,
            ),
        ];
        let choices = Choices::new("asking", "whether they close", list, Some(if close { "close" } else { "keep" }));
        Some(Item::new(label, Action::AskingAll).with(choices))
    }

    /// The row picking whether HEAD of your checkout moves to the top of
    /// the stack as the review leaves it, once its change is accepted:
    /// offered only when HEAD can move there safely; `sync` unless you
    /// picked otherwise in this review.
    fn head_row(&self) -> Option<Item> {
        let rv = self.reviewing.as_ref()?;
        if !self.head_can_follow() {
            return None;
        }
        let list = vec![
            Choice::new(
                "sync",
                "sync",
                "HEAD moves to the top of the stack as the review leaves it, detached there; the branch it is on stays as it is",
                None,
            ),
            Choice::new("stay", "no sync", "HEAD stays where it is", None),
        ];
        let choices = Choices::new("head", "whether HEAD follows the stack", list, Some(if rv.stay { "stay" } else { "sync" }));
        Some(Item::new("HEAD", Action::HeadSync).with(choices))
    }

    /// Whether HEAD of your checkout can move safely to the stack as the
    /// review leaves it: the work changes the stack, its change not
    /// rejected; HEAD was in sync with the stack as it is, when last looked
    /// at (on its top, each patch a commit with the patch's message, the
    /// revision it names too, no commit above), and nothing is uncommitted
    /// there.
    fn head_can_follow(&self) -> bool {
        let Some(rv) = &self.reviewing else { return false };
        let Some((now, _)) = self.review_stacks() else { return false };
        if self.change_rejected(&rv.marks) {
            return false;
        }
        let rev = self.db.head(&self.stack).map(|(r, _)| r);
        let Some(seen) = self.head.as_ref().filter(|h| h.name == self.stack && h.rev == rev) else { return false };
        let Some(h) = &seen.head else { return false };
        h.in_sync(now.patches.len()) && seen.clean == Some(true)
    }

    /// HEAD picked to stay where it is as the review closes (`stay`), or to
    /// follow the stack, as the review's menu picks it.
    pub(super) fn head_stays(&mut self, stay: bool) {
        if let Some(rv) = &mut self.reviewing {
            rv.stay = stay;
        }
    }

    /// Every comment that asked marked to close with the review (`close`),
    /// or to keep, as the review's menu picks it.
    pub(super) fn mark_all_asking(&mut self, close: bool) {
        let asking: Vec<usize> = self.asking_open().into_iter().map(|(k, _)| k).collect();
        for k in asking {
            self.mark_asking(k, close);
        }
    }

    /// What closing the review with every part not marked yet accepted, or
    /// rejected, asks first: those parts, said, and what closing does; the
    /// menu on Keep reviewing.
    pub(super) fn review_close_ask(&self, accept: bool) -> Menu {
        let (title, yes, then) = match accept {
            true => ("accept every part not marked yet?", "Accept them and close", "what you accepted is applied"),
            false => (
                "reject every part not marked yet?",
                "Reject them and close",
                "what you rejected is dropped for good, and what you accepted is applied",
            ),
        };
        let mut said = vec![format!("Not marked yet: {}. The review closes then: {then}.", self.review_left().join(", "))];
        // Its change, accepted or rejected as marked, else as the rest.
        let how = self.closing_as(Some(accept));
        said.extend(self.review_again(how));
        let builds = (how != Closing::Rejecting).then(|| self.review_builds()).flatten();
        let stop = builds.is_some();
        said.extend(builds);
        said.extend(self.follow_stopped(Closing::Accepting));
        // A try push's: what closing pushes.
        #[cfg(feature = "try-push")]
        said.extend(self.push_said(Some(accept)));
        said.extend(self.tools_note(how));
        self.close_choices(title, yes, Some(accept), said, stop)
    }

    /// What closing the review with every part marked asks first, if it asks
    /// something again, leaves a build and test behind, or decides what a
    /// try push pushes; the menu on Keep reviewing.
    pub(super) fn review_close_menu(&self) -> Option<Menu> {
        self.reviewing.as_ref()?;
        let how = self.closing_as(None);
        let mut said: Vec<String> = self.review_again(how).into_iter().collect();
        let builds = (how != Closing::Rejecting).then(|| self.review_builds()).flatten();
        let stop = builds.is_some();
        said.extend(builds);
        said.extend(self.follow_stopped(Closing::Accepting));
        #[cfg(feature = "try-push")]
        said.extend(self.push_said(None));
        said.extend(self.tools_note(how));
        (!said.is_empty()).then(|| self.close_choices("close the review?", "Close it", None, said, stop))
    }

    /// How closing the review, the parts not marked yet as `rest` says,
    /// goes for its change of the stack: accepted whole, accepted in part,
    /// or rejected. Of work that changes nothing of the stack, as `rest`
    /// says.
    fn closing_as(&self, rest: Option<bool>) -> Closing {
        let Some(rv) = &self.reviewing else { return Closing::Rejecting };
        if self.review_ops().is_none() {
            return if rest == Some(true) { Closing::Accepting } else { Closing::Rejecting };
        }
        match (self.change_taken(&rv.marks, rest), self.change_in_part(&rv.marks, rest)) {
            (true, true) => Closing::Partly,
            (true, false) => Closing::Accepting,
            (false, _) => Closing::Rejecting,
        }
    }

    /// Whether the review allows a tool the agent could not use, for this
    /// work or from now on.
    fn tools_allowed(&self) -> bool {
        let Some(rv) = &self.reviewing else { return false };
        self.work(&rv.run).is_some_and(|(_, h)| {
            let (work, always) = crate::refusals::allowed(&h.tools, &rv.marks);
            !work.is_empty() || !always.is_empty()
        })
    }

    /// What the review does with the tools the agent could not use as it
    /// closes `how`, said: sent back, what the next round may use for this
    /// work, and what is saved for good, in the checkout's own settings;
    /// closed, accepted or rejected, what is saved for good, and that what
    /// this work was allowed for itself ends. None for nothing.
    fn tools_note(&self, how: Closing) -> Option<String> {
        let rv = self.reviewing.as_ref()?;
        let (r, h) = self.work(&rv.run)?;
        let (mut work, always) = crate::refusals::allowed(&h.tools, &rv.marks);
        // What the work was given for itself already goes on too.
        let mut given = ask::info(r).map(|i| i.allowed).unwrap_or_default();
        given.retain(|g| !work.contains(g));
        given.append(&mut work);
        let work = given;
        let kind = self.review_way().map_or("this kind of ask", crate::refusals::kind_said);
        let mut out = Vec::new();
        let saved = |how: &str, rules: &[String]| match self.tool_local() {
            Ok(at) => format!("{how} saves for good, in {}, for {kind}: {}.", at.display(), crate::refusals::listed(rules)),
            Err(why) => format!("{} cannot be saved for good: {why}.", crate::refusals::listed(rules)),
        };
        match how {
            Closing::SendingBack | Closing::FollowingUp => {
                let mut may = work.clone();
                if !always.is_empty() && self.tool_local().is_err() {
                    may.extend(always.iter().cloned());
                }
                if !may.is_empty() {
                    out.push(match how {
                        Closing::SendingBack => format!("This work may use, from the next round until its review closes: {}.", crate::refusals::listed(&may)),
                        _ => format!("This work may use, from the follow-up on until its review closes: {}.", crate::refusals::listed(&may)),
                    });
                }
                if !always.is_empty() {
                    let doing = if how == Closing::SendingBack { "Sending it back" } else { "Asking the follow-up" };
                    out.push(match self.tool_local() {
                        Ok(_) => saved(doing, &always),
                        Err(why) => format!("{} cannot be saved for good, so it is allowed for this work instead: {why}.", crate::refusals::listed(&always)),
                    });
                }
            }
            Closing::Accepting | Closing::Partly | Closing::Rejecting => {
                if !always.is_empty() {
                    out.push(saved("Closing the review", &always));
                }
                if !work.is_empty() {
                    out.push(format!("What this work was allowed for itself ends with its review: {}.", crate::refusals::listed(&work)));
                }
            }
        }
        (!out.is_empty()).then(|| out.join("\n"))
    }

    /// What closing the review of a try push's work does with its tasks,
    /// those not marked yet as `rest` says, if the work has tasks: none
    /// accepted, nothing is pushed; else which are pushed to try, and how,
    /// those past mach's threshold at the lowest priority, as mach runs them
    /// once told yes.
    #[cfg(feature = "try-push")]
    pub(super) fn push_said(&self, rest: Option<bool>) -> Option<String> {
        let rv = self.reviewing.as_ref()?;
        let (r, h) = self.work(&rv.run)?;
        if h.tasks.is_empty() {
            return None;
        }
        let accepted = crate::held::accepted_tasks(h, &rv.marks, rest);
        if accepted.is_empty() {
            return Some("No task is accepted: closing the review pushes nothing to try.".into());
        }
        let n = accepted.len();
        let tasks = crate::try_push::tasks_said(n);
        // Pushed as the ask that selected them: the work's own, or the try
        // push asked as a follow-up from its review.
        let what = match crate::try_push::asker(&self.db, r).map(|i| i.scope) {
            Some(Scope::Patch(c)) => {
                let at = self.db.stack(&r.stack).ok().and_then(|s| s.patches.iter().position(|p| self.db.patch(p).change == c));
                match at {
                    Some(k) => crate::try_push::patches_said(k + 1, false),
                    None => "the patches up to its own".to_string(),
                }
            }
            _ => crate::try_push::patches_said(0, true),
        };
        // mach would push more than the stack's patches on its base: the
        // review stays open until that is fixed.
        if let Some(why) = self.push_base_problem() {
            return Some(format!("Closing the review with {tasks} keeps it open, as nothing could be pushed: {why}."));
        }
        let mut out = format!(
            "Closing the review pushes {tasks} to try, with {what}: first `./mach try fuzzy --no-push`, to see that it selects them, then the push."
        );
        let total = crate::try_push::scheduled(n, h.rebuild);
        if total > crate::try_push::THRESHOLD {
            let these = match h.rebuild {
                Some(k) => format!("These {} tasks, each run {k} times, are {} tasks,", thousands(n), thousands(total)),
                None => format!("These {} tasks are", thousands(n)),
            };
            out += &format!(
                " {these} more than the {} mach pushes without asking: they would run at the lowest priority, as mach does when you say yes.",
                thousands(crate::try_push::THRESHOLD)
            );
        }
        // The comment that asked, marked to close: open all the same.
        let asking = self.asking_open();
        if asking.iter().any(|(k, _)| rv.marks.get(&crate::held::asking_key(*k)) == Some(&true)) {
            out += " The comment that asked stays open all the same: the push's link goes there as a reply.";
        }
        Some(out)
    }

    /// The name of the effort the work reviewed was asked at, which heads
    /// the agent's words the review shows as comments of its own (why it
    /// selected a task, what it said of a tool it could not use): none for
    /// work of before, nor for what is not the agent's.
    fn reviewed_at(&self) -> Option<&'static str> {
        let rv = self.reviewing.as_ref()?;
        ask::effort_said(self.db.run(&rv.run)?)
    }

    /// A question before closing the review, the parts not marked as `rest`
    /// says: `yes`; when `stop`, `yes` also stopping the build and test of the
    /// stack as it was; or keep reviewing, picked. `said`, its note.
    fn close_choices(&self, title: &str, yes: &str, rest: Option<bool>, said: Vec<String>, stop: bool) -> Menu {
        let mut items = vec![Item::new(yes, Action::ReviewClose(rest))];
        if stop {
            items.push(Item::new(format!("{yes}, and stop the build"), Action::ReviewCloseStop(rest)));
        }
        items.push(Item::new("Keep reviewing", Action::Cancel));
        let mut m = Menu::new(title, items, None);
        m.sel = m.items.len() - 1;
        m.note = Some(said.join("\n"));
        m
    }

    /// What closing the review as `how` does to the asks it leaves behind,
    /// said: its change accepted, to the answers made on the stack before
    /// it, each as marked (`answers_closing`); rejected, or the work sent
    /// back, the edits that went on from it, asked again; accepted in part,
    /// both. None for nothing, or for work with no change of the stack.
    fn review_again(&self, how: Closing) -> Option<String> {
        let rv = self.reviewing.as_ref()?;
        self.work(&rv.run).filter(|(_, h)| h.stack.is_some())?;
        if how == Closing::Accepting {
            return self.answers_closing(false);
        }
        let answers = if how == Closing::Partly { self.answers_closing(true) } else { None };
        let (running, waiting) = ask::guesses_after(&self.db, &rv.run);
        let n = running + waiting;
        if n == 0 {
            return answers;
        }
        let is = |k: usize| if k == 1 { "is" } else { "are" };
        let mut how_each = Vec::new();
        if running > 0 {
            how_each.push(format!("{running} running {} stopped", is(running)));
        }
        if waiting > 0 {
            how_each.push(format!("{waiting} waiting for your review {} thrown away", is(waiting)));
        }
        let each = how_each.join(", and ");
        let they = if n == 1 { "It is" } else { "They are" };
        let edits = if n == 1 { "1 edit".to_string() } else { format!("{n} edits") };
        let dropped = match how {
            Closing::Rejecting => format!(
                "Rejecting this change throws away the {edits} that went on from it, as if you accepted it: {each}. {they} asked again from the stack as it is."
            ),
            Closing::Partly => format!(
                "Accepting this change in part throws away the {edits} that went on from it, as if you accepted it whole: {each}. {they} asked again from the stack the operations you accepted leave."
            ),
            _ => format!("Sending this work back throws away the {edits} that went on from it: {each}. {they} asked again once the agent's new work lands."),
        };
        Some(match answers {
            Some(a) => format!("{a}\n{dropped}"),
            None => dropped,
        })
    }

    /// What closing the review with its change accepted, whole or `in_part`,
    /// does to the answers made on the stack before it, running or waiting
    /// for your review, as each is marked: kept, restarted and stopped, a
    /// count each, said. None without any.
    fn answers_closing(&self, in_part: bool) -> Option<String> {
        let rv = self.reviewing.as_ref()?;
        let items: Vec<(String, Going)> = self.answer_items().into_iter().filter(|(_, g)| g.markable()).collect();
        let of = |m: AnswerMark, going: Going| items.iter().filter(|(id, g)| *g == going && crate::held::answer_mark(&rv.marks, id) == Some(m)).count();
        let one = |n: usize, one: &'static str, many: &'static str| if n == 1 { one } else { many };
        let mut each = Vec::new();
        let n = of(AnswerMark::Keep, Going::Running);
        if n > 0 {
            each.push(format!(
                "{n} running {} on, and {} for your review on the stack {} asked on",
                one(n, "goes", "go"),
                one(n, "lands", "land"),
                one(n, "it was", "they were")
            ));
        }
        let n = of(AnswerMark::Keep, Going::Waiting);
        if n > 0 {
            each.push(format!("{n} waiting for your review {} as {}", one(n, "stays", "stay"), one(n, "it is", "they are")));
        }
        let n = of(AnswerMark::Restart, Going::Running);
        if n > 0 {
            each.push(format!("{n} running {} stopped, and asked again on the stack the change makes", one(n, "is", "are")));
        }
        let n = of(AnswerMark::Restart, Going::Waiting);
        if n > 0 {
            each.push(format!(
                "{n} waiting for your review {} asked again on the stack the change makes, shown what {} brought back",
                one(n, "is", "are"),
                one(n, "it", "they")
            ));
        }
        let n = of(AnswerMark::Stop, Going::Running);
        if n > 0 {
            each.push(format!("{n} running {} stopped, {} left open, not asked again", one(n, "is", "are"), one(n, "its comment", "their comments")));
        }
        let n = of(AnswerMark::Stop, Going::Waiting);
        if n > 0 {
            each.push(format!(
                "{n} waiting for your review {} thrown away unseen, {} left open, not asked again",
                one(n, "is", "are"),
                one(n, "its comment", "their comments")
            ));
        }
        let applies = if in_part { "applies its change in part" } else { "applies its change" };
        (!each.is_empty()).then(|| format!("Closing this review {applies}. Of the answers made on the stack before it, {}.", each.join("; ")))
    }

    /// The build and test of the stack as it was, going, that accepting a
    /// change leaves behind, said.
    fn review_builds(&self) -> Option<String> {
        (!ask::builds_going(&self.db, &self.stack).is_empty())
            .then(|| "A build and test of the stack as it was is going: it is not asked again, and you may stop it.".to_string())
    }

    /// The parts not marked yet, said by kind: the change of the stack, or
    /// some of its operations (`2 stack operations`), `2 opened comments`,
    /// `1 reply`.
    fn review_left(&self) -> Vec<String> {
        let Some(rv) = &self.reviewing else { return vec![] };
        let left: Vec<Part> = self.review_parts().into_iter().filter(|p| !self.part_marked(&rv.marks, *p)).collect();
        let mut out = Vec::new();
        if left.contains(&Part::Stack) {
            out.push(match self.review_ops().map(|o| o.tally(&rv.marks)) {
                Some((0, 0, _)) | None => "the change of the stack".to_string(),
                Some((_, _, 1)) => "1 stack operation".to_string(),
                Some((_, _, n)) => format!("{n} stack operations"),
            });
        }
        let opened = left.iter().filter(|p| matches!(p, Part::Opened(_))).count();
        let replies = left.iter().filter(|p| matches!(p, Part::Reply(_))).count();
        #[cfg(feature = "try-push")]
        let tasks = left.iter().filter(|p| matches!(p, Part::Task(_))).count();
        #[cfg(not(feature = "try-push"))]
        let tasks = 0;
        for (n, one, many) in [(tasks, "task", "tasks"), (opened, "opened comment", "opened comments"), (replies, "reply", "replies")] {
            match n {
                0 => {}
                1 => out.push(format!("1 {one}")),
                n => out.push(format!("{n} {many}")),
            }
        }
        out
    }

    /// The menu of the part the cursor is on, in the review.
    pub(super) fn review_items(&self, items: &mut Vec<Item>) {
        let lines = self.review_lines();
        let Some(rv) = &self.reviewing else { return };
        let rejected = self.change_rejected(&rv.marks);
        let ops = self.review_ops();
        // On a line of a message, each of its links, to open in your browser:
        // after the comment on the line, as in the middle; else after the
        // rest, Space then Enter doing what it does without them.
        let mut links = Some(self.link_items());
        // On a line of a patch looked at closer where Enter writes a comment
        // of its own: that comment, here or in $EDITOR, as in the middle.
        if self.review_writable()
            && let Some((_, p, row)) = self.review_row()
            && self.line_comment(&p, &row).is_some()
        {
            let label = if row.kind == Kind::File { "Comment on this file" } else { "Comment on this line" };
            items.push(Item::new(label, Action::CommentLine).with(self.write_choices(Action::CommentLine, Action::CommentInEditor)));
            items.extend(links.take().unwrap_or_default());
            items.push(Item::line());
        }
        // What you wrote: written again, or taken back.
        if let Some(y) = lines.get(rv.sel).and_then(|l| l.yours.as_ref()) {
            let (edit, back) = match y {
                Yours::Comment(..) => ("Edit it", "Take it back"),
                Yours::Answer(_) | Yours::ToolAnswer(_) => ("Edit your answer to it", "Take your answer back"),
                #[cfg(feature = "try-push")]
                Yours::TaskAnswer(_) => ("Edit your answer to it", "Take your answer back"),
            };
            items.push(Item::new(edit, Action::EditYours));
            items.push(Item::new(back, Action::TakeYoursBack));
        }
        // A reply of the agent's, or one a build and test asked as a
        // follow-up brought, to read: answered, the work goes back to it.
        // Not one a later reply follows in its thread, which answered what
        // you said after it; nor while a follow-up works on its comment.
        let reply = lines.get(rv.sel).and_then(|l| match (l.part, l.found.clone()) {
            (Some(Part::Reply(k)), _) | (_, Some(Found::Reply(k))) => Some(k),
            _ => None,
        });
        if self.review_writable()
            && let Some(k) = reply
            && let Some((_, h)) = self.work(&rv.run)
        {
            let answered = h.replies.get(k).is_some_and(|rp| rp.answer.is_some());
            let busy = h.replies.get(k).is_some_and(|rp| ask::following(&self.db, &rv.run).contains(&(rp.change.clone(), rp.id.clone())));
            let item = Item::new(if answered { "Edit your answer to it" } else { "Reply to it" }, Action::Answer(k));
            items.push(item.when(h.last_reply(k) && !busy));
        }
        // A try push's task: why the agent selected it, answered, the work
        // goes back to the agent.
        #[cfg(feature = "try-push")]
        if self.review_writable()
            && let Some(Part::Task(k)) = lines.get(rv.sel).and_then(|l| l.part)
        {
            let answered = self.work(&rv.run).is_some_and(|(_, h)| h.tasks.get(k).is_some_and(|t| t.answer.is_some()));
            items.push(Item::new(if answered { "Edit your answer to it" } else { "Reply to it" }, Action::AnswerTask(k)));
        }
        // A tool the agent could not use: refused, allowed for this work or
        // allowed from now on, a box each, the one it is checked, each naming
        // the rule, from now on the file it is saved in; then the other rules
        // it may be allowed with; then your answer to what the agent said.
        if let Some(k) = lines.get(rv.sel).and_then(|l| l.tool)
            && let Some(t) = self.tool_item(k)
        {
            if t.offered() {
                let rule = crate::refusals::rule_of(t, &rv.marks, k);
                let now = crate::refusals::state(&rv.marks, k);
                let local = self.tool_local();
                let always = match &local {
                    Ok(at) => format!("Allowed from now on: {rule}, saved in {}", at.display()),
                    Err(_) => format!("Allowed from now on: {rule}"),
                };
                items.push(Item::new(format!("Refused: {rule}"), Action::ToolState(k, Allow::Refused)).checked(now == Allow::Refused));
                items.push(Item::new(format!("Allowed for this work: {rule}"), Action::ToolState(k, Allow::Work)).checked(now == Allow::Work));
                items.push(Item::new(always, Action::ToolState(k, Allow::Always)).checked(now == Allow::Always).when(local.is_ok()));
                let at = crate::refusals::picked(&rv.marks, k, t);
                if t.rules.len() > 1 {
                    items.push(Item::line());
                    for (j, r) in t.rules.iter().enumerate().filter(|(j, _)| *j != at) {
                        let how = if j > at { "Widen the rule to" } else { "Narrow the rule to" };
                        items.push(Item::new(format!("{how} {r}"), Action::ToolRule(k, j)));
                    }
                }
                items.push(Item::line());
            }
            if self.review_writable() {
                items.push(Item::new(if t.answer.is_some() { "Edit your answer to it" } else { "Reply to it" }, Action::AnswerTool(k)));
            }
        }
        // An answer made on the stack before the change: kept, restarted or
        // stopped once the change is accepted, a box each, the one it is
        // marked checked, each saying what it does.
        if let Some(id) = lines.get(rv.sel).and_then(|l| l.answer.as_ref())
            && let Some((_, going)) = self.answer_items().into_iter().find(|(k, _)| k == id).filter(|(_, g)| g.markable())
        {
            let now = crate::held::answer_mark(&rv.marks, id);
            for (m, word) in [(AnswerMark::Keep, "Kept"), (AnswerMark::Restart, "Restarted"), (AnswerMark::Stop, "Stopped")] {
                items.push(Item::new(format!("{word}: {}", answer_does(m, going)), Action::MarkAnswer(id.clone(), m)).checked(now == Some(m)));
            }
            items.push(Item::line());
        }
        // A comment that asked: closed with the review, or kept.
        if let Some(k) = lines.get(rv.sel).and_then(|l| l.asking) {
            let closing = rv.marks.get(&crate::held::asking_key(k)) == Some(&true);
            items.push(Item::new("Close it with the review", Action::CloseAsking(k, true)).when(!closing));
            items.push(Item::new("Keep it open", Action::CloseAsking(k, false)).when(closing));
        }
        // A pull's comments: all accepted, none rejected, nothing to mark;
        // its operations, as any.
        let marked = !self.review_pulled();
        let line = lines.get(rv.sel);
        let op = ops.as_ref().zip(line).and_then(|(o, l)| Self::line_op(o, l));
        if let Some(p) = line.and_then(|l| l.part).filter(|p| marked || *p == Part::Stack) {
            let may_accept = p == Part::Stack || !rejected;
            // An operation of the change, by its own mark; their title, the
            // change as a whole, by theirs: every one accepted, or rejected,
            // or neither. A patch left as it was has none.
            let now = match (p, &ops, op) {
                (Part::Stack, Some(_), Some(o)) if !o.marked() => None,
                (Part::Stack, Some(ops), Some(o)) => Some(ops.mark(&rv.marks, o)),
                (Part::Stack, Some(ops), None) => match ops.tally(&rv.marks) {
                    (_, 0, 0) => Some(Some(true)),
                    (0, _, 0) => Some(Some(false)),
                    _ => Some(None),
                },
                _ => Some(rv.marks.get(&p.key()).copied()),
            };
            match now {
                Some(Some(true)) => items.push(Item::new("Mark as rejected", Action::MarkPart(false))),
                Some(Some(false)) => items.push(Item::new("Mark as accepted", Action::MarkPart(true)).when(may_accept)),
                Some(None) => {
                    items.push(Item::new("Mark as accepted", Action::MarkPart(true)).when(may_accept));
                    items.push(Item::new("Mark as rejected", Action::MarkPart(false)));
                }
                None => {}
            }
        }
        // Every operation of the change at once, but on their title, which
        // marks them all as it is.
        let on_title = line.is_some_and(|l| l.part == Some(Part::Stack) && l.entry.is_none());
        if !on_title && ops.as_ref().is_some_and(|o| !o.whole()) {
            items.push(Item::new("Mark all operations accepted", Action::MarkOps(true)));
            items.push(Item::new("Mark all operations rejected", Action::MarkOps(false)));
        }
        // The comments it opened, and its replies, each kind apart.
        let parts: Vec<Part> = self.review_parts().into_iter().filter(|_| marked).collect();
        if parts.iter().any(|p| matches!(p, Part::Opened(_))) {
            items.push(Item::new("Mark all opened comments accepted", Action::MarkOpened(true)).when(!rejected));
            items.push(Item::new("Mark all opened comments rejected", Action::MarkOpened(false)));
        }
        #[cfg(feature = "try-push")]
        if parts.iter().any(|p| matches!(p, Part::Task(_))) {
            items.push(Item::new("Mark all tasks accepted", Action::MarkTasks(true)));
            items.push(Item::new("Mark all tasks rejected", Action::MarkTasks(false)));
        }
        if parts.iter().any(|p| matches!(p, Part::Reply(_))) {
            items.push(Item::new("Mark all replies accepted", Action::MarkReplies(true)).when(!rejected));
            items.push(Item::new("Mark all replies rejected", Action::MarkReplies(false)));
        }
        // The answers made on the stack before the change, all marked at
        // once, for when the change is accepted.
        if self.answer_items().iter().any(|(_, g)| g.markable()) {
            items.push(Item::new("Mark all answers kept", Action::MarkAnswers(AnswerMark::Keep)));
            items.push(Item::new("Mark all answers restarted", Action::MarkAnswers(AnswerMark::Restart)));
            items.push(Item::new("Mark all answers stopped", Action::MarkAnswers(AnswerMark::Stop)));
        }
        // Edited by hand from here, the stack the work makes, where the
        // cursor is, as in the middle: the file of the code the cursor is
        // on, opened at that line; the patch the cursor is in, as mail,
        // opened at the cursor's line of it, else at its top; the stack as
        // a list.
        if self.read_only().is_none() && self.review_stacks().is_some() {
            items.push(Item::line());
            if let Some(w) = self.work_place() {
                if let Some((file, line)) = w.code {
                    let short = file.rsplit('/').next().unwrap_or(&file).to_string();
                    items.push(Item::new(format!("Edit {short} here in $EDITOR"), Action::EditWorkFile(w.at, file, line)).with(self.edit_choices()));
                }
                items.push(Item::new("Edit this patch in $EDITOR", Action::EditWorkPatch(w.at, w.mail)));
            }
            items.push(Item::new("Edit the stack as a list in $EDITOR", Action::EditWorkList));
        }
        if let Some(links) = links {
            items.push(Item::line());
            items.extend(links);
        }
    }

    /// The edit by hand `action` asks for in the review, of the stack the
    /// work makes: opened at the line the menu found the cursor on.
    pub(super) fn work_edit(&self, action: &Action) -> Option<Effect> {
        let run = self.reviewing.as_ref()?.run.clone();
        let edit = match action {
            Action::EditWorkList => WorkEdit::List,
            Action::EditWorkPatch(at, line) => WorkEdit::Mail { at: *at, line: *line },
            Action::EditWorkFile(at, file, line) => WorkEdit::Code {
                at: *at,
                file: file.clone(),
                line: *line,
                again: None,
            },
            _ => return None,
        };
        Some(Effect::EditWork { run, edit })
    }

    /// The marks made so far in the review of `run`, for the work that takes
    /// its place: all but those of its change of the stack's operations,
    /// which is new; the comments as they were before every one of those was
    /// rejected, if each was.
    pub(super) fn carried_marks(&self, run: &str) -> BTreeMap<String, bool> {
        let (mut marks, before) = match &self.reviewing {
            Some(rv) if rv.run == run => (rv.marks.clone(), rv.before.clone()),
            _ => (self.db.review(run).map(|v| v.marks.clone()).unwrap_or_default(), None),
        };
        let ops = self.db.run(run).and_then(|r| crate::operations::of_review(&self.db, r, r.held.as_ref()?));
        let rejected = match &ops {
            Some(ops) => matches!(ops.tally(&marks), (0, n, 0) if n > 0),
            None => marks.get(&Part::Stack.key()) == Some(&false),
        };
        marks.retain(|k, _| !crate::operations::is_key(k) && *k != Part::Stack.key());
        if rejected {
            marks = before.unwrap_or_default();
        }
        marks
    }

    /// The part the cursor is on, in the review.
    pub(super) fn review_part(&self) -> Option<Part> {
        let rv = self.reviewing.as_ref()?;
        self.review_lines().get(rv.sel).and_then(|l| l.part)
    }

    /// Leaves the review, saved or not, for the main screen.
    pub(super) fn leave_review(&mut self) {
        self.keep_review();
        self.reviewing = None;
        self.screen = Screen::Main;
    }

    /// The review's marks saved, to go on from them later.
    pub(super) fn review_save(&mut self) {
        let Some(rv) = self.reviewing.clone() else { return };
        // A pull's with no change: nothing marked, all of it filed as it
        // closes.
        let pulled = self.review_pulled() && self.review_ops().is_none();
        if self.edit(|db| crate::held::save(db, &rv.run, &rv.marks)).is_some() {
            self.status = match pulled {
                // A pull's: nothing marked, all of it filed as it closes.
                true => PULL_WAITS.into(),
                false => "your marks are saved: the review goes on from them when you open it again".into(),
            };
            self.leave_review();
        }
    }

    /// The review closed, as `review_close_now` closes it, the parts not
    /// marked as `rest` says; `stop`, the build and test of the stack as it
    /// was stopped too. Its change accepted in part, operation by operation,
    /// only once the stack the operations accepted leave is seen to apply,
    /// checked away from the screen first (`held::closing_stands`), the
    /// status line saying so meanwhile (`CHECKING_CLOSE`).
    pub(super) fn review_close(&mut self, rest: Option<bool>, stop: bool) {
        let Some(rv) = self.reviewing.clone() else { return };
        if let Some((ops, stands)) = crate::held::closing_stands(&self.db, &rv.run, &rv.marks, rest) {
            self.check_closing(ToClose { review: rv, rest, stop, banner: false }, ops, stands);
            return;
        }
        self.review_close_now(rest, stop);
    }

    /// Close `c` waiting for its check of the stack the operations accepted
    /// leave (`ops`, `stands` standing), made away from the screen, on the
    /// patches taken from the database (`Ops::pool`): what it comes to is
    /// read as the screen next looks (`closing_checked`). Another close asked
    /// meanwhile takes its place.
    fn check_closing(&mut self, c: ToClose, ops: Ops, stands: std::collections::BTreeSet<String>) {
        let pool = ops.pool(&self.db);
        self.bg.closes += 1;
        let n = self.bg.closes;
        self.bg.closing = Some((n, c));
        self.status = CHECKING_CLOSE.into();
        self.spawn(move || Done::Closing(n, ops.closing(&pool, &stands).map_err(|e| e.0)));
    }

    /// What the check of the stack the operations accepted leave came to,
    /// for close `n`: once it applies, the review closed as asked; else why
    /// not, nothing changed. Thrown away for a close another took the place
    /// of; and nothing done if the review changed meanwhile, its marks or
    /// itself, as what was checked is not what would close.
    pub(super) fn closing_checked(&mut self, n: usize, found: std::result::Result<Option<String>, String>) {
        let Some((_, c)) = self.bg.closing.take_if(|(k, _)| *k == n) else { return };
        let now = match c.banner {
            true => self.reviewing.is_none().then(|| self.review_of(&c.review.run)),
            false => self.reviewing.clone(),
        };
        if !now.is_some_and(|rv| rv.run == c.review.run && rv.marks == c.review.marks) {
            self.status = CLOSE_CHANGED.into();
            return;
        }
        match found {
            Ok(None) if c.banner => {
                self.reviewing = Some(c.review);
                self.review_close_now(c.rest, c.stop);
                self.reviewing = None;
                self.screen = Screen::Main;
            }
            Ok(None) => self.review_close_now(c.rest, c.stop),
            Ok(Some(why)) => self.status = crate::held::refused_closing(&why),
            Err(e) => self.status = format!("the stack the operations you accepted leave could not be checked: {e}; nothing changed"),
        }
    }

    /// The review closed: what is accepted applied, the rest dropped; the
    /// parts not marked accepted or rejected as `rest` says; `stop`, the
    /// build and test of the stack as it was stopped too, once it is closed.
    /// Its change accepted, HEAD of your checkout follows the stack to its
    /// top, if it can safely and you did not pick otherwise.
    fn review_close_now(&mut self, rest: Option<bool>, stop: bool) {
        let builds = if stop { ask::builds_going(&self.db, &self.stack) } else { vec![] };
        self.review_closed(rest);
        if self.reviewing.is_none() && !builds.is_empty() {
            for r in builds {
                self.stop(r.id, false);
            }
            self.status += "; the build and test of the stack as it was is being stopped, and lands as failed";
        }
    }

    /// The review closed, as `review_close_now` closes it, nothing stopped.
    fn review_closed(&mut self, rest: Option<bool>) {
        let Some(rv) = self.reviewing.clone() else { return };
        // Its change accepted, wholly or in part: the stack changes.
        let accepted = self.change_taken(&rv.marks, rest);
        let follow = accepted && !rv.stay && self.head_can_follow();
        // A try push's tasks accepted, pushed once the review closes: not
        // on a base that mach would push more than the patches on, which
        // the review stays open to fix. The comment that asked stays open,
        // the push's link to go there.
        #[cfg(feature = "try-push")]
        let (pushed, rebuild) = self
            .work(&rv.run)
            .map(|(_, h)| (crate::held::accepted_tasks(h, &rv.marks, rest), h.rebuild))
            .unwrap_or_default();
        #[allow(unused_mut)]
        let mut marks = rv.marks.clone();
        #[cfg(feature = "try-push")]
        if !pushed.is_empty() {
            if let Some(why) = self.push_base_problem() {
                self.status = format!("the review stays open: {why}");
                return;
            }
            marks.retain(|k, _| !k.starts_with("asking "));
        }
        // HEAD to follow: moving from now on, before the stack takes the
        // change, which HEAD has only once it has moved; from the commit it
        // was seen in sync with the stack at.
        let seen = self.head_here().map(|h| h.commit.clone()).unwrap_or_default();
        let moving = follow && self.head_to_move();
        let stack = self.stack.clone();
        let closed = self.edit(|db| {
            // An answer asked since the review opened is none of its items:
            // it goes on as it is, kept, the change accepted.
            let mut marks = marks.clone();
            if accepted {
                for a in ask::answers_left(db, &stack, &rv.run) {
                    if !rv.answers.contains(&a.id) && crate::held::answer_mark(&marks, &a.id).is_none() {
                        crate::held::set_answer_mark(&mut marks, &a.id, AnswerMark::Keep);
                    }
                }
            }
            let said = crate::held::close(db, &rv.run, &marks, rest)?;
            #[cfg(feature = "try-push")]
            if !pushed.is_empty() {
                let id = crate::try_push::begin(db, &rv.run, &pushed, rebuild)?;
                return Ok((said, Some(id)));
            }
            Ok((said, None::<String>))
        });
        if let Some((said, push)) = closed {
            self.status = format!("reviewed: {said}");
            if let Some(id) = push {
                self.start_try_push(&id);
            }
            self.leave_review();
            if moving {
                self.head_follows(format!("reviewed: {said}; "), seen);
            } else if follow {
                self.status = format!("reviewed: {said}; HEAD is being moved already");
            }
            // What waited on the review may go.
            self.launch();
        } else if moving {
            // Not closed: HEAD stays where it is.
            self.bg.doing.retain(|d| *d != MOVING_HEAD);
        }
    }

    /// Try push `id`, begun as the review closed, started away from here:
    /// the status line says so, or why it could not be, then the push
    /// ended.
    #[cfg(feature = "try-push")]
    fn start_try_push(&mut self, id: &str) {
        match crate::try_push::launch(&self.path, id) {
            Ok(()) => self.status += "; the try push has started: the comment that asked shows what mach writes",
            Err(e) => {
                crate::try_push::not_started(&self.path, id, &e.0);
                self.status += &format!("; the try push could not start: {}", e.0);
            }
        }
        self.reload();
    }

    #[cfg(not(feature = "try-push"))]
    fn start_try_push(&mut self, _: &str) {}

    /// Why mach would push more than the stack's patches on its base, if it
    /// would: the review of a try push's work is not closed with tasks
    /// accepted then.
    #[cfg(feature = "try-push")]
    pub(super) fn push_base_problem(&self) -> Option<String> {
        let s = self.current()?;
        let (repo, base) = crate::ops::base_of(s).ok()?;
        crate::try_push::base_problem(&repo, &base)
    }

    /// The landed asks waiting for review on this stack, the first landed
    /// first: their runs.
    pub(super) fn queue(&self) -> Vec<String> {
        crate::held::queue(&self.db, &self.stack).into_iter().map(|r| r.id.clone()).collect()
    }
}
