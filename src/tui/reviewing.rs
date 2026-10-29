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

use std::collections::BTreeMap;
use std::ops::Range;

use super::*;
use crate::db::Run;
use crate::held::{Held as Work, Part};
use crate::history::Diffed;

/// What the review's right pane shows of a part: what it is, said; its
/// lines; and the lines that are the part, the first to show first.
type View = (String, Vec<Vec<(String, Style)>>, Range<usize>);

/// The stack diff of the change, as the right pane shows it: what it is,
/// said; its lines. For each of its entries: the lines it takes; where
/// Enter on it goes, the patch it is of (by its entry among the stack
/// operations) and, on a line of what changed in it, the line of that
/// patch's mail it is; whether it is a line of what changed, and one
/// added or taken out. And where each entry of the stack operations is.
#[derive(Default)]
struct DiffView {
    said: String,
    lines: Vec<Vec<(String, Style)>>,
    spans: Vec<Range<usize>>,
    goes: Vec<Option<(usize, Option<usize>)>>,
    code: Vec<bool>,
    changed: Vec<bool>,
    of_entry: Vec<usize>,
}

impl DiffView {
    /// Where the cursor goes in it from entry `entry` of the stack
    /// operations: on the first line that changed of that patch, else on
    /// the patch; from none, the section's title, on the first line that
    /// changed of any.
    fn first_change(&self, entry: Option<usize>) -> usize {
        match entry.and_then(|e| self.of_entry.get(e).copied()) {
            Some(f) => (f + 1..self.code.len()).take_while(|&k| self.code[k]).find(|&k| self.changed[k]).unwrap_or(f),
            None => self.changed.iter().position(|&c| c).unwrap_or(0),
        }
    }
}

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
    /// Each part marked: accepted, or rejected. As saved, and since.
    pub marks: BTreeMap<String, bool>,
    /// The comments' marks when the change of the stack was rejected,
    /// rejecting them with it: they come back if it is accepted again.
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
}

/// What you wrote in the review a line of its list is: a comment you opened
/// there, by (change, id); or your answer to the work's reply `0`.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Yours {
    Comment(String, String),
    Answer(usize),
}

/// A line of the review's list: what it says, how, and the part it is of
/// (none, a section's title); for the change's, its place in the diff.
pub(super) struct Line {
    text: String,
    style: Style,
    part: Option<Part>,
    entry: Option<usize>,
    /// The comment the ask was about it is, by its place among them: kept,
    /// or closed with the review.
    asking: Option<usize>,
    /// What you wrote in the review it is: to see where it would be, to
    /// write again, or to take back.
    yours: Option<Yours>,
}

impl Line {
    /// Whether the cursor goes on it: a part, a comment that asked, or what
    /// you wrote; not a section's title.
    fn selectable(&self) -> bool {
        self.part.is_some() || self.asking.is_some() || self.yours.is_some()
    }
}

/// How a review closes, for what that asks again.
#[derive(Clone, Copy, PartialEq)]
enum Closing {
    Accepting,
    Rejecting,
    SendingBack,
}

/// What a part's mark says, as the list shows it.
fn mark_of(m: Option<bool>) -> (&'static str, Style) {
    match m {
        Some(true) => ("✓", color::add()),
        Some(false) => ("✗", color::del()),
        None => ("·", Style::new()),
    }
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

/// A comment's first words.
pub(super) fn first_words(text: &str) -> String {
    thread_turns(text).first().map(|t| crate::markdown::first_line(&t.body)).unwrap_or_default()
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
    /// does; not while the commit messages of the stack and of your
    /// workspace differ, which are synced before any landed work is
    /// reviewed, whatever opens it.
    pub(super) fn open_review(&mut self, run: &str, part: Option<Part>) {
        if let Some(why) = self.sync_first() {
            self.status = why;
            return;
        }
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
        let marks = self.db.review(run).map(|v| v.marks.clone()).unwrap_or_default();
        self.reviewing = Some(Reviewing {
            run: run.into(),
            marks,
            ..Default::default()
        });
        self.screen = Screen::Review;
        self.on_banner = false;
        let lines = self.review_lines();
        let marks = self.reviewing.as_ref().map(|r| r.marks.clone()).unwrap_or_default();
        let was = self.review_was(run).filter(|&k| lines.get(k).is_some_and(|l| l.part.is_some()));
        let at = match part {
            Some(p) => lines.iter().position(|l| l.part == Some(p)),
            None => was.or_else(|| lines.iter().position(|l| l.part.is_some_and(|p| !marks.contains_key(&p.key())))),
        };
        // No part left: on the first comment that asked.
        let at = at.or_else(|| lines.iter().position(|l| l.part.is_some())).or_else(|| lines.iter().position(Line::selectable)).unwrap_or(0);
        if let Some(rv) = &mut self.reviewing {
            rv.sel = at;
        }
        // Where HEAD is, looked at again, and whether the checkout is clean:
        // HEAD in sync with the stack may follow it as the review closes.
        self.look_at_head();
    }

    /// The stack as it is, and as accepting the change would make it.
    fn review_stacks(&self) -> Option<(Stack, Stack)> {
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

    /// The comments the ask of what is reviewed was about, still open, each
    /// with its place among them.
    fn asking_open(&self) -> Vec<(usize, Note)> {
        let Some((r, _)) = self.reviewing.as_ref().and_then(|rv| self.work(&rv.run)) else { return vec![] };
        crate::held::asking_open(&self.db, r).into_iter().map(|(k, n)| (k, n.clone())).collect()
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
        self.review_lines().get(rv.sel).is_some_and(|l| l.asking.is_none() && l.yours.is_none() && matches!(l.part, Some(Part::Stack) | None))
            && self.review_stacks().is_some()
    }

    /// The stack diff of the change, `w` wide, as the right pane shows it.
    fn review_diff(&self, w: usize) -> Option<DiffView> {
        let (now, next) = self.review_stacks()?;
        let (_, h) = self.reviewing.as_ref().and_then(|rv| self.work(&rv.run))?;
        let diff = crate::history::stacks_diff(&self.db, Some(&now), Some(&next), true);
        let of = h.stack.as_ref().map_or(String::new(), |c| c.of.clone());
        let mut v = DiffView {
            said: format!(" the change of {of}: what accepting it does to the stack"),
            ..Default::default()
        };
        // The patch the lines of code under it are of, by its entry among
        // the stack operations, which have every entry but those lines.
        let mut under = None;
        for (k, (d, parts)) in diff.iter().zip(diff_parts(&diff)).enumerate() {
            let start = v.lines.len();
            v.lines.extend(fold_parts(&parts, w.saturating_sub(1), 6));
            v.spans.push(start..v.lines.len());
            let code = matches!(d, Diffed::Code(_));
            if !code {
                under = matches!(d, Diffed::Patch { .. }).then_some(v.of_entry.len());
                v.of_entry.push(k);
            }
            v.goes.push(under.map(|e| (e, None)));
            v.code.push(code);
            v.changed.push(matches!(d, Diffed::Code(l) if l.text.starts_with('+') || l.text.starts_with('-')));
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
                if let Some((e, _)) = v.goes[start + j] {
                    v.goes[start + j] = Some((e, Some(at)));
                }
            }
        }
        Some(v)
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
    /// interdiff (`message_line_of`). None elsewhere.
    pub(super) fn review_message_line(&self) -> Option<String> {
        let rv = self.reviewing.as_ref().filter(|rv| rv.in_main)?;
        if rv.in_patch {
            let (_, p, row) = self.review_row()?;
            let at = row.msg_at.filter(|_| row.kind == Kind::Msg && row.note.is_none())?;
            return p.message.split('\n').nth(at).map(String::from);
        }
        if !self.review_on_diff() {
            return None;
        }
        let (now, next) = self.review_stacks()?;
        let diff = crate::history::stacks_diff(&self.db, Some(&now), Some(&next), true);
        message_line_of(&self.db, &diff, &next, rv.entry.min(diff.len().saturating_sub(1)))
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
            && let Some((entry, m)) = v.goes.get(rv.entry.min(v.goes.len().saturating_sub(1))).copied().flatten()
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
        if let Some((_, h)) = self.reviewing.as_ref().and_then(|rv| self.work(&rv.run)) {
            notes = self
                .db
                .notes(&p.change)
                .into_iter()
                .filter(|n| self.review_shows(n))
                .map(|n| match h.replies.iter().find(|rp| rp.change == n.change && rp.id == n.id) {
                    Some(rp) => Note { text: rp.after(&n.text), ..n.clone() },
                    None => n.clone(),
                })
                .collect();
            notes.extend(h.opened.iter().filter(|n| n.change == p.change).cloned());
        }
        let mut deco = Deco {
            pointed: self.pointed_of(&notes),
            ..Default::default()
        };
        if let Some((id, turns)) = self.written_turns() {
            deco.writing.insert(id, turns);
        }
        patch_rows(p, &notes.iter().collect::<Vec<_>>(), w, &deco)
    }

    /// Whether comment `n` is one of the work reviewed: one its ask was
    /// about, or one you wrote in its review.
    pub(super) fn review_shows(&self, n: &Note) -> bool {
        let Some((r, _)) = self.reviewing.as_ref().and_then(|rv| self.work(&rv.run)) else { return false };
        crate::held::asking_of(r).iter().any(|(c, i)| *c == n.change && *i == n.id) || (n.review.as_deref() == Some(r.id.as_str()) && n.by.is_none())
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
        let replied = self.work(&rv.run).and_then(|(_, h)| h.replies.iter().position(|rp| rp.change == p.change && rp.id == id));
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
        let was = self.work(&run).and_then(|(_, h)| h.replies.get(k)?.answer.clone()).unwrap_or_default();
        self.write(Target::Answer { run, k }, Focus::Diff, &was);
    }

    /// What you wrote in the review on the list's line the cursor is on, if
    /// it is.
    pub(super) fn review_yours_line(&self) -> Option<Yours> {
        let rv = self.reviewing.as_ref()?;
        self.review_lines().get(rv.sel)?.yours.clone()
    }

    /// What you wrote in the review, `y`, written again: what you said last
    /// in your comment, or your answer to the work's reply.
    pub(super) fn review_write_yours(&mut self, y: Yours) {
        match y {
            Yours::Comment(change, id) => self.edit_mine(change, id, Focus::Diff),
            Yours::Answer(k) => self.review_answer(k),
        }
    }

    /// What you wrote in the review, `y`, taken back: your comment closed, or
    /// your answer to the work's reply gone. It does not go back to the agent
    /// with the work; the cursor stays on a line still there.
    pub(super) fn review_take_back(&mut self, y: Yours) {
        let Some(run) = self.reviewing.as_ref().map(|rv| rv.run.clone()) else { return };
        let done = match y {
            Yours::Comment(change, id) => self.edit(|db| {
                let n = db.note(&change, &id).cloned().ok_or_else(|| crate::db::Error("the comment is gone".into()))?;
                db.set_note(Note { text: String::new(), ..n }).map(|_| ())
            }),
            Yours::Answer(k) => self.edit(|db| crate::held::answer(db, &run, k, "")),
        };
        if done.is_none() {
            return;
        }
        self.status = "taken back: it does not go back to the agent with the work".into();
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
        let (title, yes) = match self.review_yours_line()? {
            Yours::Comment(..) => ("take your comment back? it does not go back to the agent with the work", "Take it back"),
            Yours::Answer(_) => ("take your answer back? it does not go back to the agent with the work", "Take your answer back"),
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
        let p = line.part?;
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
            marks: self.db.review(run).map(|v| v.marks.clone()).unwrap_or_default(),
            ..Default::default()
        }
    }

    /// What Delete does on the banner's line of what run `run` brought
    /// back: asks to reject every part of it not marked yet and close its
    /// review, as the review's "Reject all remaining and Close" does. Not
    /// before the commit messages are synced, nor while it goes on from
    /// work landed before it, which is reviewed first, nor with no part
    /// left that is not marked, as that item is not: why is said.
    pub(super) fn reject_work_here(&self, run: &str) -> std::result::Result<Action, String> {
        let Some((r, _)) = self.work(run) else { return Err("this work is gone".into()) };
        if let Some(why) = self.sync_first() {
            return Err(why);
        }
        if ask::guessing(&self.db, r).is_some() {
            return Err("this work goes on from the work landed before it, which waits for your review: review that one first".into());
        }
        let marks = self.review_of(run).marks;
        if crate::held::parts_left(&self.db, r).iter().all(|p| marks.contains_key(&p.key())) {
            return Err(self.say_keys("nothing of this work is left to reject: press Enter to review it, and close the review there".into()));
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
        let accepting = self.reviewing.as_ref().and_then(|rv| rv.marks.get(&Part::Stack.key()).copied()) == Some(true);
        said.extend(self.review_again(if accepting { Closing::Accepting } else { Closing::Rejecting }));
        self.reviewing = was;
        let yes = Item::new("Reject them and close", Action::RejectWork(run.into()));
        let mut m = Menu::ask("reject every part of this work not marked yet, then close its review?", yes, "Keep it for review", true);
        m.note = Some(said.join("\n"));
        self.mode = Mode::Menu(m);
    }

    /// The review of what run `run` brought back closed from the banner,
    /// every part not marked yet rejected, as from the review; the main
    /// screen staying, closed or not (the status line says why not). Not
    /// before the commit messages are synced.
    pub(super) fn reject_work(&mut self, run: &str) {
        if let Some(why) = self.sync_first() {
            self.status = why;
            return;
        }
        self.reviewing = Some(self.review_of(run));
        self.review_close(Some(false));
        self.reviewing = None;
        self.screen = Screen::Main;
    }

    /// The work sent back to the agent, with what you wrote in its review:
    /// another round, from it, at `effort`, else at the effort the work was
    /// asked at; the review left.
    pub(super) fn review_send_back(&mut self, effort: Option<Effort>) {
        let Some(rv) = self.reviewing.clone() else { return };
        if self.set_up_first(super::Then::SendBack(effort)) {
            return;
        }
        let sent = ask::send_back(&self.path, &self.stack, &rv.run, &rv.marks, effort);
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

    /// The review's list: on top, the comments that asked, each kept or
    /// closed with the review; then its three sections, each part a line,
    /// the change of the stack a line for each patch.
    pub(super) fn review_lines(&self) -> Vec<Line> {
        let mut out = Vec::new();
        let Some(rv) = &self.reviewing else { return out };
        let Some((_, h)) = self.work(&rv.run) else { return out };
        let asking = self.asking_open();
        if !asking.is_empty() {
            out.push(Line {
                text: (if asking.len() == 1 { " asking comment" } else { " asking comments" }).into(),
                style: color::file(),
                part: None,
                entry: None,
                asking: None,
                yours: None,
            });
            for (k, n) in asking {
                let at = if n.on_stack() { "the stack".to_string() } else { n.where_said() };
                let (m, st) = match rv.marks.get(&crate::held::asking_key(k)) {
                    Some(true) => ("close", color::del()),
                    _ => ("keep ", Style::new()),
                };
                out.push(Line {
                    text: format!("   {m} {at} · {}", first_words(&n.text)),
                    style: st,
                    part: None,
                    entry: None,
                    asking: Some(k),
                    yours: None,
                });
            }
        }
        let parts = self.review_parts();
        let mark = |p: Part| rv.marks.get(&p.key()).copied();
        let title = |text: &str| Line {
            text: text.into(),
            style: color::file(),
            part: None,
            entry: None,
            asking: None,
            yours: None,
        };
        if let Some((now, next)) = self.review_stacks() {
            let said = match mark(Part::Stack) {
                Some(true) => "accepted",
                Some(false) => "rejected",
                None => "to review",
            };
            out.push(Line {
                text: format!(" stack operations · {said}"),
                style: color::file(),
                part: Some(Part::Stack),
                entry: None,
                asking: None,
                yours: None,
            });
            let diff = crate::history::stacks_diff(&self.db, Some(&now), Some(&next), false);
            for (k, d) in diff.iter().enumerate() {
                if let Some((text, style)) = operation(d) {
                    out.push(Line {
                        text: format!("   {text}"),
                        style,
                        part: Some(Part::Stack),
                        entry: Some(k),
                        asking: None,
                        yours: None,
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
                    text: format!("   {m} {} · {}", n.where_said(), first_words(&n.text)),
                    style: st,
                    part: Some(p),
                    entry: None,
                    asking: None,
                    yours: None,
                });
            }
        }
        let replies: Vec<Part> = parts.iter().copied().filter(|p| matches!(p, Part::Reply(_))).collect();
        if !replies.is_empty() {
            out.push(title(" replies"));
            for p in replies {
                let Part::Reply(k) = p else { continue };
                let rp = &h.replies[k];
                let at = match self.db.note(&rp.change, &rp.id) {
                    Some(n) if n.on_stack() => "the stack".to_string(),
                    Some(n) => n.where_said(),
                    None => "a comment deleted".to_string(),
                };
                let (m, st) = mark_of(mark(p));
                let edit = if rp.was.is_some() { format!("{}: ", rp.said()) } else { String::new() };
                out.push(Line {
                    text: format!("   {m} {at} · {edit}{}", crate::markdown::first_line(&rp.text)),
                    style: st,
                    part: Some(p),
                    entry: None,
                    asking: None,
                    yours: None,
                });
            }
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
        for (k, rp) in h.replies.iter().enumerate().filter(|(_, rp)| rp.answer.is_some()) {
            let at = self.db.note(&rp.change, &rp.id).map_or("a comment".to_string(), |n| n.where_said());
            let text = format!("   ✎ your answer on {at} · {}", crate::markdown::first_line(rp.answer.as_deref().unwrap_or("")));
            yours.push((text, Yours::Answer(k)));
        }
        if !yours.is_empty() {
            out.push(title(" what you wrote, to send back"));
            for (text, y) in yours {
                out.push(Line {
                    text,
                    style: color::you(),
                    part: None,
                    entry: None,
                    asking: None,
                    yours: Some(y),
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
        match line.part {
            Some(Part::Stack) | None => {
                let Some(v) = self.review_diff(w) else {
                    return ("the agent changed nothing of the stack".into(), vec![], 0..0);
                };
                // Where the patch of the list's line is, among every entry.
                let first = line.entry.and_then(|e| v.of_entry.get(e)).map_or(0, |&k| v.spans[k].start);
                (v.said, v.lines, first..first)
            }
            Some(p) => {
                let (n, said) = match p {
                    Part::Reply(k) => {
                        let rp = &h.replies[k];
                        let Some(n) = self.db.note(&rp.change, &rp.id) else {
                            return ("its comment was deleted: nothing to answer".into(), vec![], 0..0);
                        };
                        let text = rp.after(&n.text);
                        let said = match &rp.promote {
                            Some(why) => format!(" its reply, as it would be in its thread; then it moves to the stack's comments: {why}"),
                            None if rp.was.is_some() => format!(" {}'s comment, edited on Phabricator, as it would be in its thread", rp.who()),
                            None => " its reply, as it would be in its thread".to_string(),
                        };
                        (Note { text, ..n.clone() }, said)
                    }
                    Part::Opened(k) if crate::held::pulled(r) => (h.opened[k].clone(), " a reviewer's comment, where it would be".to_string()),
                    Part::Opened(k) => (h.opened[k].clone(), " a comment it opened, where it would be".to_string()),
                    Part::Stack => unreachable!(),
                };
                self.comment_view(r, &n, said, w)
            }
        }
    }

    /// Comment `n` of the work of run `r` where it is, `w` wide, `said`
    /// above it; and the lines that are its own.
    fn comment_view(&self, r: &Run, n: &Note, said: String, w: usize) -> View {
        let deco = Deco {
            pointed: self.pointed_of([n]),
            ..Default::default()
        };
        let rows = if n.on_stack() {
            let Ok(s) = self.db.stack(&r.stack) else { return (said, vec![], 0..0) };
            let titles: Vec<String> = s.patches.iter().map(|id| self.db.patch(id).title().to_string()).collect();
            stack_rows(&format!("the stack {}", r.stack), &titles, &[n], w, &deco)
        } else {
            let p = self
                .db
                .stack(&r.stack)
                .ok()
                .and_then(|s| s.patches.iter().map(|id| self.db.patch(id)).find(|p| p.change == n.change));
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
                let n = Note { text: rp.after(&n.text), ..n.clone() };
                self.comment_view(r, &n, " your answer, after the work's reply, as it would be in its thread".into(), w)
            }
        }
    }

    /// The review: its list on the left, what the cursor's part would be on
    /// the right.
    pub(super) fn draw_review(&mut self, buf: &mut Buffer, body: Rect) -> Option<(u16, u16)> {
        let (w, h) = (body.width as usize, body.height as usize);
        let lines = self.review_lines();
        let rv = self.reviewing.clone()?;
        let list_w = if w < 60 { w } else { (w * 2 / 5).max(30) };
        let list = Rect::new(body.x, body.y, list_w as u16, body.height);
        // Wide, the part the cursor is on has it all, the list out of sight.
        let wide = rv.in_main && rv.wide;
        let rows = h.saturating_sub(1);
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
                let asked = if r.request.is_empty() { String::new() } else { format!(" · asked \"{}\"", r.request) };
                let edited = if h.edits.is_empty() { String::new() } else { format!(" · then you {}", h.edits.join(", then ")) };
                format!(" the agent's work: {left}{asked}{edited}")
            });
            put(buf, list, 0, 0, list_w, &fit(&title, list_w), color::header());
            let mut top = rv.top;
            if rv.sel < top {
                top = rv.sel;
            } else if rv.sel >= top + rows {
                top = rv.sel + 1 - rows;
            }
            for (y, (i, l)) in lines.iter().enumerate().skip(top).take(rows).enumerate() {
                let mut st = l.style;
                if i == rv.sel {
                    st = if rv.in_main { st.patch(color::picked()) } else { on_cursor(st) };
                }
                put(buf, list, 0, y + 1, list_w, &fit(&l.text, list_w), st);
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
            let (cur, row_top) = match rv.goto {
                Some(m) => {
                    let cur = row_of(&all, crate::ops::line_of_mail(&p, m));
                    (cur, cur.saturating_sub(room / 3))
                }
                None => (rv.row.min(all.len().saturating_sub(1)), rv.row_top),
            };
            // Written on, the box goes under the line or the comment, as in
            // the main view, in sight.
            let under = editing.then(|| self.editor_anchor_in(&all).unwrap_or(cur));
            let shown = under.unwrap_or(cur);
            let mut top = row_top.min(shown);
            if shown >= top + room {
                top = shown + 1 - room;
            }
            if let Some(r) = &mut self.reviewing {
                (r.row, r.row_top, r.main_w, r.goto) = (cur, top, pw, None);
            }
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
        // Where the part is, unless scrolled by hand. Your answer to its reply
        // being written: the box under the thread, in sight, what follows the
        // thread below it.
        let edit_h = self.editor_height(main, Focus::Diff);
        let mut scroll = if rv.in_main { rv.scroll } else { at.start.saturating_sub(rows / 3) };
        if edit_h > 0 {
            scroll = scroll.max((at.end + edit_h).saturating_sub(rows)).min(at.start);
        }
        // The stack diff looked at closer: the cursor on an entry, in sight.
        let picked = (rv.in_main && self.review_on_diff())
            .then(|| self.review_diff(main_w.saturating_sub(1)))
            .flatten()
            .and_then(|v| v.spans.get(rv.entry.min(v.spans.len().saturating_sub(1))).cloned());
        if let Some(c) = &picked {
            scroll = scroll.min(c.start).max(c.end.saturating_sub(rows));
        }
        let scroll = scroll.min(view.len().saturating_sub(1));
        if let Some(r) = &mut self.reviewing {
            r.scroll = scroll;
        }
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
        // A patch looked at closer: how many rows it has.
        let looked = self.reviewing.as_ref().is_some_and(|rv| rv.in_main && rv.in_patch);
        let patch_rows = self
            .review_patch()
            .filter(|_| looked)
            .map(|(_, p)| self.review_rows(&p, self.reviewing.as_ref().map_or(80, |rv| rv.main_w)).len());
        // The stack diff: where → from the list puts the cursor in it; looked
        // at closer, where Enter on each of its entries goes.
        let in_main = self.reviewing.as_ref().is_some_and(|rv| rv.in_main);
        let on_diff = in_main && !looked && self.review_on_diff();
        let diff = (on_diff || (!in_main && k.code == KeyCode::Right)).then(|| self.review_diff(80)).flatten();
        let entry = self.reviewing.as_ref().and_then(|rv| lines.get(rv.sel)).and_then(|l| l.entry);
        let first = diff.as_ref().map_or(0, |v| v.first_change(entry));
        let goes = diff.filter(|_| on_diff).map(|v| v.goes);
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
            (true, KeyCode::Enter) if goes.is_some() => {
                let to = goes.as_ref().and_then(|g| g.get(rv.entry).copied().flatten());
                match to.and_then(|(e, m)| Some((lines.iter().position(|l| l.entry == Some(e))?, m))) {
                    Some((sel, m)) => {
                        rv.sel = sel;
                        rv.in_patch = true;
                        (rv.row, rv.row_top, rv.goto) = (0, 0, m);
                    }
                    None => self.status = "this line is of a patch dropped, or of the base: there is no line of a patch to go to".into(),
                }
            }
            (true, code @ (KeyCode::Up | KeyCode::Down | KeyCode::PageUp | KeyCode::PageDown | KeyCode::Home | KeyCode::End))
                if goes.is_some() =>
            {
                let last = goes.as_ref().map_or(0, |g| g.len().saturating_sub(1));
                rv.entry = match code {
                    KeyCode::Down => (rv.entry + 1).min(last),
                    KeyCode::Up => rv.entry.saturating_sub(1),
                    KeyCode::PageDown => (rv.entry + page).min(last),
                    KeyCode::PageUp => rv.entry.saturating_sub(page),
                    KeyCode::Home => 0,
                    _ => last,
                };
            }
            (_, KeyCode::Enter) => {
                if let Some(y) = lines.get(rv.sel).and_then(|l| l.yours.clone()) {
                    // What you wrote: written again.
                    self.review_write_yours(y);
                } else if let Some(k) = lines.get(rv.sel).and_then(|l| l.asking) {
                    // A comment that asked: closed with the review, or kept.
                    let close = rv.marks.get(&crate::held::asking_key(k)) != Some(&true);
                    self.mark_asking(k, close);
                } else if let Some(p) = lines.get(rv.sel).and_then(|l| l.part) {
                    match rv.marks.get(&p.key()) {
                        Some(true) => {
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

    /// Part `p` marked accepted, or rejected. The change of the stack
    /// rejected rejects the comments of the work with it; accepted again,
    /// they are as they were. A comment is not accepted while the change it
    /// goes with is rejected.
    pub(super) fn mark(&mut self, p: Part, accept: bool) {
        let comments: Vec<Part> = self.review_parts().into_iter().filter(|p| *p != Part::Stack).collect();
        let Some(rv) = self.reviewing.as_mut() else { return };
        let rejected = rv.marks.get(&Part::Stack.key()) == Some(&false);
        if p == Part::Stack {
            if !accept && !rejected {
                rv.before = Some(comments.iter().filter_map(|c| Some((c.key(), *rv.marks.get(&c.key())?))).collect());
                for c in &comments {
                    rv.marks.insert(c.key(), false);
                }
            }
            if accept && rejected {
                let before = rv.before.take().unwrap_or_default();
                for c in &comments {
                    match before.get(&c.key()) {
                        Some(m) => rv.marks.insert(c.key(), *m),
                        None => rv.marks.remove(&c.key()),
                    };
                }
            }
        } else if accept && rejected {
            self.status = "the agent's change of the stack is rejected, and the comments with it: accept the change first".into();
            return;
        }
        rv.marks.insert(p.key(), accept);
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
        let Some(rv) = self.reviewing.as_mut() else { return };
        let rejected = rv.marks.get(&Part::Stack.key()) == Some(&false);
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
        match &self.reviewing {
            Some(rv) if rv.in_main && rv.in_patch && self.review_patch().is_some() => {
                let enter = if self.review_writable() { "enter: comment · " } else { "" };
                let side = if rv.wide { "←: narrower" } else { "→: wider · ←: back to the stack diff" };
                format!("↑↓: a line · {enter}{open}{delete}{side} · {esc}")
            }
            Some(rv) if rv.in_main && self.review_on_diff() => {
                let side = if rv.wide { "←: narrower" } else { "→: wider · ←: back to the list" };
                format!("↑↓: a line · enter: go to it in the patch · {open}{delete}{side} · {esc}")
            }
            Some(rv) if rv.in_main && rv.wide => format!("↑↓: scroll · {enter} · {delete}←: narrower · {esc}"),
            Some(rv) if rv.in_main => format!("↑↓: scroll · {enter} · {delete}→: wider · ←: back to the list · {esc}"),
            _ => format!("↑↓ choose · {enter} · {delete}→: look closer · space: menu · {esc}"),
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
        match (line.asking, line.part) {
            (Some(k), _) if rv.marks.get(&crate::held::asking_key(k)) == Some(&true) => "enter: keep it open",
            (Some(_), _) => "enter: close it with the review",
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
            })
            .collect();
        for p in these {
            self.mark(p, accept);
        }
    }

    /// What is marked, by kind of part: a line for each.
    pub(super) fn review_summary(&self) -> String {
        let Some(rv) = &self.reviewing else { return String::new() };
        let parts = self.review_parts();
        let mark = |p: &Part| rv.marks.get(&p.key()).copied();
        let mut out = Vec::new();
        // Nothing brought back: only the comments that asked, to say.
        if self.work(&rv.run).is_some_and(|(_, h)| h.is_empty()) {
            out.push(crate::held::NOTHING_BACK.to_string());
        }
        let asking = self.asking_open();
        if !asking.is_empty() {
            let closing = asking.iter().filter(|(k, _)| rv.marks.get(&crate::held::asking_key(*k)) == Some(&true)).count();
            let mut said = Vec::new();
            for (n, word) in [(closing, "to close"), (asking.len() - closing, "kept")] {
                if n > 0 {
                    said.push(format!("{n} {word}"));
                }
            }
            out.push(format!("{}: {}", if asking.len() == 1 { "asking comment" } else { "asking comments" }, said.join(", ")));
        }
        if parts.contains(&Part::Stack) {
            out.push(format!(
                "the change of the stack: {}",
                match mark(&Part::Stack) {
                    Some(true) => "accepted",
                    Some(false) => "rejected",
                    None => "to review",
                }
            ));
        }
        for (what, of) in [("comments opened", 0), ("replies", 1)] {
            let these: Vec<&Part> = parts.iter().filter(|p| matches!((of, p), (0, Part::Opened(_)) | (1, Part::Reply(_)))).collect();
            if these.is_empty() {
                continue;
            }
            let count = |m: Option<bool>| these.iter().filter(|p| mark(p) == m).count();
            let mut said = Vec::new();
            for (n, word) in [(count(Some(true)), "accepted"), (count(Some(false)), "rejected"), (count(None), "to review")] {
                if n > 0 {
                    said.push(format!("{n} {word}"));
                }
            }
            out.push(format!("{what}: {}", said.join(", ")));
        }
        out.join("\n")
    }

    /// What Esc asks in the review: what is marked, said; then save the
    /// marks, throw away those made since it was opened, or close it.
    pub(super) fn review_dialog(&self) -> Menu {
        // What you wrote in it: it goes back to the agent, neither accepted
        // nor rejected.
        let (comments, answers) = self.review_yours();
        if comments + answers > 0 {
            let back = if self.review_again(Closing::SendingBack).is_some() { Action::AskSendBack } else { Action::SendBack };
            let items = vec![
                Item::new("Send it back to the agent", back).with(self.effort_choices(self.review_effort())),
                Item::new("Save", Action::ReviewSave),
                Item::new("Discard", Action::ReviewDiscard),
                Item::line(),
                Item::new("Keep reviewing", Action::Cancel),
            ];
            let mut m = Menu::new("your review of the agent's work", items, None);
            let mut what = Vec::new();
            for (n, one, many) in [(comments, "1 comment", "comments"), (answers, "1 answer to the agent's replies", "answers to the agent's replies")] {
                match n {
                    0 => {}
                    1 => what.push(one.to_string()),
                    n => what.push(format!("{n} {many}")),
                }
            }
            m.note = Some(format!(
                "You wrote {} in this review, so the work is not accepted: it goes back to the agent, which is asked again, starting from this work.\n{}",
                what.join(" and "),
                self.review_summary()
            ));
            return m;
        }
        let parts = self.review_parts();
        let done = self.reviewing.as_ref().is_some_and(|rv| parts.iter().all(|p| rv.marks.contains_key(&p.key())));
        // Every part marked, nothing remains for the others to mark.
        let mut items = vec![
            Item::new("Save", Action::ReviewSave),
            Item::new("Discard", Action::ReviewDiscard),
            Item::new("Close review", if self.review_close_menu().is_some() { Action::AskClose } else { Action::ReviewClose(None) }).when(done),
            // What remains, accepted or rejected together, a section of its
            // own; over it, whether the comments that asked close with it,
            // and whether HEAD follows the stack.
            Item::line(),
        ];
        items.extend(self.asking_row());
        items.extend(self.head_row());
        items.extend([
            Item::new("Accept all remaining and Close", Action::AskReviewClose(true)).when(!done),
            Item::new("Reject all remaining and Close", Action::AskReviewClose(false)).when(!done),
            Item::line(),
            Item::new("Keep reviewing", Action::Cancel),
        ]);
        let run = self.reviewing.as_ref().and_then(|rv| self.db.run(&rv.run));
        let of = match run {
            Some(r) if crate::held::pulled(r) => "the comments pulled",
            Some(r) if crate::held::by_hand(r) => "your edit by hand",
            _ => "the agent's work",
        };
        let mut m = Menu::new(format!("your review of {of}"), items, None);
        m.note = Some(self.review_summary());
        m
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
    /// there; and no agent works in your checkout, nor makes its changes
    /// there (`agent.worktree = "pwd"`), which moves HEAD itself.
    fn head_can_follow(&self) -> bool {
        let Some(rv) = &self.reviewing else { return false };
        let Some((now, _)) = self.review_stacks() else { return false };
        if rv.marks.get(&Part::Stack.key()) == Some(&false) {
            return false;
        }
        let rev = self.db.head(&self.stack).map(|(r, _)| r);
        let Some(seen) = self.head.as_ref().filter(|h| h.name == self.stack && h.rev == rev) else { return false };
        let Some(h) = &seen.head else { return false };
        h.in_sync(now.patches.len()) && seen.clean == Some(true) && !crate::head::agent_in(&self.db, &h.dir) && !ask::changes_in_checkout(&self.db, &self.stack)
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
        let accepting = self.reviewing.as_ref().and_then(|rv| rv.marks.get(&Part::Stack.key()).copied()).unwrap_or(accept);
        said.extend(self.review_again(if accepting { Closing::Accepting } else { Closing::Rejecting }));
        let builds = accepting.then(|| self.review_builds()).flatten();
        let stop = builds.is_some();
        said.extend(builds);
        self.close_choices(title, yes, Some(accept), said, stop)
    }

    /// What closing the review with every part marked asks first, if it asks
    /// something again or leaves a build and test behind; the menu on Keep
    /// reviewing.
    pub(super) fn review_close_menu(&self) -> Option<Menu> {
        let rv = self.reviewing.as_ref()?;
        let accepting = rv.marks.get(&Part::Stack.key()) == Some(&true);
        let mut said: Vec<String> = self.review_again(if accepting { Closing::Accepting } else { Closing::Rejecting }).into_iter().collect();
        let builds = accepting.then(|| self.review_builds()).flatten();
        let stop = builds.is_some();
        said.extend(builds);
        (!said.is_empty()).then(|| self.close_choices("close the review?", "Close it", None, said, stop))
    }

    /// What sending the work back asks first: what went on from it, asked
    /// again; the menu on Keep reviewing. It goes at the effort the menu
    /// before picked, unless another is picked here.
    pub(super) fn review_send_back_ask(&self) -> Menu {
        let back = Item::new("Send it back", Action::SendBack).with(self.effort_choices(self.at_effort.or(self.review_effort())));
        let mut m = Menu::ask("send the work back to the agent?", back, "Keep reviewing", true);
        m.note = self.review_again(Closing::SendingBack);
        m
    }

    /// The effort the work reviewed was asked at, which it goes back to the
    /// agent at unless another is picked: none for work of before.
    fn review_effort(&self) -> Option<Effort> {
        let rv = self.reviewing.as_ref()?;
        ask::info(self.db.run(&rv.run)?)?.effort
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

    /// What closing the review as `how` asks again, said: its change
    /// accepted, the answer-only asks made on the stack before it; rejected,
    /// or the work sent back, the edits that went on from it. None for
    /// nothing, or for work with no change of the stack.
    fn review_again(&self, how: Closing) -> Option<String> {
        let rv = self.reviewing.as_ref()?;
        self.work(&rv.run).filter(|(_, h)| h.stack.is_some())?;
        let (running, waiting) = match how {
            Closing::Accepting => ask::answers_reasked(&self.db, &self.stack, &rv.run),
            _ => ask::guesses_after(&self.db, &rv.run),
        };
        let n = running + waiting;
        if n == 0 {
            return None;
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
        Some(match how {
            Closing::Accepting => format!(
                "Closing this review applies its change. The {} made on the stack before it {} asked again: {each}.",
                if n == 1 { "1 answer-only ask".to_string() } else { format!("{n} answer-only asks") },
                is(n)
            ),
            Closing::Rejecting => format!(
                "Rejecting this change throws away the {edits} that went on from it, as if you accepted it: {each}. {they} asked again from the stack as it is."
            ),
            Closing::SendingBack => format!(
                "Sending this work back throws away the {edits} that went on from it: {each}. {they} asked again once the agent's new work lands."
            ),
        })
    }

    /// The build and test of the stack as it was, going, that accepting a
    /// change leaves behind, said.
    fn review_builds(&self) -> Option<String> {
        (!ask::builds_going(&self.db, &self.stack).is_empty())
            .then(|| "A build and test of the stack as it was is going: it is not asked again, and you may stop it.".to_string())
    }

    /// The parts not marked yet, said by kind: the change of the stack,
    /// `2 opened comments`, `1 reply`.
    fn review_left(&self) -> Vec<String> {
        let Some(rv) = &self.reviewing else { return vec![] };
        let left: Vec<Part> = self.review_parts().into_iter().filter(|p| !rv.marks.contains_key(&p.key())).collect();
        let mut out = Vec::new();
        if left.contains(&Part::Stack) {
            out.push("the change of the stack".to_string());
        }
        let opened = left.iter().filter(|p| matches!(p, Part::Opened(_))).count();
        let replies = left.iter().filter(|p| matches!(p, Part::Reply(_))).count();
        for (n, one, many) in [(opened, "opened comment", "opened comments"), (replies, "reply", "replies")] {
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
        let rejected = rv.marks.get(&Part::Stack.key()) == Some(&false);
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
                Yours::Answer(_) => ("Edit your answer to it", "Take your answer back"),
            };
            items.push(Item::new(edit, Action::EditYours));
            items.push(Item::new(back, Action::TakeYoursBack));
        }
        // A reply of the agent's: answered, the work goes back to it.
        if self.review_writable()
            && let Some(Part::Reply(k)) = lines.get(rv.sel).and_then(|l| l.part)
        {
            let answered = self.work(&rv.run).is_some_and(|(_, h)| h.replies.get(k).is_some_and(|rp| rp.answer.is_some()));
            items.push(Item::new(if answered { "Edit your answer to it" } else { "Reply to it" }, Action::Answer(k)));
        }
        // A comment that asked: closed with the review, or kept.
        if let Some(k) = lines.get(rv.sel).and_then(|l| l.asking) {
            let closing = rv.marks.get(&crate::held::asking_key(k)) == Some(&true);
            items.push(Item::new("Close it with the review", Action::CloseAsking(k, true)).when(!closing));
            items.push(Item::new("Keep it open", Action::CloseAsking(k, false)).when(closing));
        }
        if let Some(p) = lines.get(rv.sel).and_then(|l| l.part) {
            let may_accept = p == Part::Stack || !rejected;
            match rv.marks.get(&p.key()) {
                Some(true) => items.push(Item::new("Mark as rejected", Action::MarkPart(false))),
                Some(false) => items.push(Item::new("Mark as accepted", Action::MarkPart(true)).when(may_accept)),
                None => {
                    items.push(Item::new("Mark as accepted", Action::MarkPart(true)).when(may_accept));
                    items.push(Item::new("Mark as rejected", Action::MarkPart(false)));
                }
            }
        }
        // The comments it opened, and its replies, each kind apart.
        let parts = self.review_parts();
        if parts.iter().any(|p| matches!(p, Part::Opened(_))) {
            items.push(Item::new("Mark all opened comments accepted", Action::MarkOpened(true)).when(!rejected));
            items.push(Item::new("Mark all opened comments rejected", Action::MarkOpened(false)));
        }
        if parts.iter().any(|p| matches!(p, Part::Reply(_))) {
            items.push(Item::new("Mark all replies accepted", Action::MarkReplies(true)).when(!rejected));
            items.push(Item::new("Mark all replies rejected", Action::MarkReplies(false)));
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
    /// its place: all but its change of the stack's, which is new; the
    /// comments as they were before that change was rejected, if it was.
    pub(super) fn carried_marks(&self, run: &str) -> BTreeMap<String, bool> {
        let (mut marks, before) = match &self.reviewing {
            Some(rv) if rv.run == run => (rv.marks.clone(), rv.before.clone()),
            _ => (self.db.review(run).map(|v| v.marks.clone()).unwrap_or_default(), None),
        };
        if marks.remove(&Part::Stack.key()) == Some(false) {
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
        if self.edit(|db| crate::held::save(db, &rv.run, &rv.marks)).is_some() {
            self.status = "your marks are saved: the review goes on from them when you open it again".into();
            self.leave_review();
        }
    }

    /// The review closed: what is accepted applied, the rest dropped; the
    /// parts not marked accepted or rejected as `rest` says. Its change
    /// accepted, HEAD of your checkout follows the stack to its top, if it
    /// can safely and you did not pick otherwise.
    pub(super) fn review_close(&mut self, rest: Option<bool>) {
        let Some(rv) = self.reviewing.clone() else { return };
        let accepted = rv.marks.get(&Part::Stack.key()).copied().or(rest) == Some(true);
        let follow = accepted && !rv.stay && self.head_can_follow();
        // HEAD to follow: moving from now on, before the stack takes the
        // change, which HEAD has only once it has moved; the sync of the
        // commit messages waits meanwhile.
        let moving = follow && self.head_to_move();
        if let Some(said) = self.edit(|db| crate::held::close(db, &rv.run, &rv.marks, rest)) {
            self.status = format!("reviewed: {said}");
            self.leave_review();
            if moving {
                let n = self.patch_count();
                self.head_moves(n, format!("reviewed: {said}; "));
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

    /// The landed asks waiting for review on this stack, the first landed
    /// first: their runs.
    pub(super) fn queue(&self) -> Vec<String> {
        crate::held::queue(&self.db, &self.stack).into_iter().map(|r| r.id.clone()).collect()
    }
}
