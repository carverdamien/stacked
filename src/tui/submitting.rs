//! A patch sent to Phabricator as work in progress, from its menu, or its
//! title, summary and parent alone to its revision, open but not in work:
//! what is sent worked out first, away from the screen, from what the
//! database holds of Phabricator, what it lacks fetched into it first, the
//! bottom line saying so (`submit::lacking`), nothing sent yet. For a new
//! revision, a dialog then says what is sent and where. For a revision
//! there already, a screen of its own shows what replaces what: what is
//! sent and where, said, how long ago what it shows of the revision was
//! fetched, as the Phabricator line says it, and, to a revision not in
//! work, that the code is not sent when it differs; the title and summary,
//! Phabricator's then what is sent; what else changes (the parent, the
//! bug, the status); and
//! the code, when a new diff is sent, in the view of the interdiffs that 1
//! sets (`views_dialog`): compared as two diffs, as the view of what
//! differs from Phabricator draws its code for the revision and the patch
//! (`Compared`), Phabricator's diff and the one sent as an interdiff, or,
//! the two on other bases, each file as each leaves it; compared as the
//! code, each file as Phabricator's latest diff leaves it, then as the patch
//! does, both on the patches below it, as the middle draws a patch
//! (`change::change_stops`), Phabricator's diff named with how long ago it
//! was fetched. Side by side, what is said over the code is side by side
//! too. Esc opens the screen's menu, which sends it, as it shows it,
//! nothing read of Phabricator first, or sends nothing; a right click opens
//! it too. While HEAD is on the stack, a patch whose commit in the
//! workspace names a revision it does not is held back, its item grayed,
//! before anything is worked out. Built with the `moz-phab` feature.

use super::change::{Near, change_stops, code_view};
use super::compared::{diff_rows, message_rows, row, said_rows};
use super::interdiffs::{At, Seen, Two, half_of, paired};
use super::revisions::{Shown as CodeShown, bare, files_drawn, files_heading, interdiff_drawn, named_interdiff, shown_of, shown_said};
use super::*;
use crate::ops::CodePlace;
use crate::phab::age_said;
use crate::submit::{Prepared, and};

/// What the bar says while a submit is read, or sent.
pub(super) const SUBMITTING: &str = "submitting";

/// How the review names the diff a submit sends, against Phabricator's.
const SENT: &str = "what is sent";

/// The item of the review's menu that opens the dialog of the views of the
/// interdiffs, for its code.
pub(super) const VIEW_OF_CODE: &str = "The view of the code…";

/// A submit read, waiting on you: its dialog open, for a new revision; or
/// its review, on its screen.
pub struct Submitting {
    pub ready: Prepared,
    /// What the review says over the code, as rows (`review_rows`), and the
    /// first row of each of its lines, a line folded into more than one;
    /// the lines it draws, those rows then the code (`review_lines`), each
    /// with what it shows, to be found again drawn otherwise
    /// (`interdiffs::Seen`); what they were drawn for: the width, the view
    /// of the interdiffs, every line or only what the patches changed, and
    /// how long ago what they show of the revision was fetched, as they say
    /// it (`phab::age_said`); and, drawn again, the line the cursor
    /// went on and the place it came with (`interdiffs::locate`), so that
    /// switching back and forth it goes back where it was.
    rows: Vec<Row>,
    starts: Vec<usize>,
    lines: Vec<Parts>,
    at: Vec<Seen>,
    built: Option<(usize, InterLayout, InterLines, Option<String>)>,
    kept: Option<(usize, At)>,
    /// The line the cursor is on, and the first shown.
    pub cursor: usize,
    top: usize,
    /// The screen it was opened from, to go back to.
    back: Screen,
}

impl Submitting {
    fn new(ready: Prepared, back: Screen) -> Submitting {
        Submitting {
            ready,
            rows: Vec::new(),
            starts: Vec::new(),
            lines: Vec::new(),
            at: Vec::new(),
            built: None,
            kept: None,
            cursor: 0,
            top: 0,
            back,
        }
    }

    /// What its lines read, as drawn last: for the tests.
    pub fn texts(&self) -> Vec<String> {
        texts(&self.lines)
    }
}

/// The rows of the review of `p` over its code, folded to `w`: what is sent
/// and where, said, and that the code does not change when it does not,
/// that it is not sent when it differs but the revision is not in work, or
/// why a new diff is sent of a latest diff that makes the changes of a
/// version of the patch; how long ago what is shown of the revision was
/// fetched, `age` (`fetched 11 minutes ago`), as the Phabricator line says
/// it; then only what changes: the title and summary, Phabricator's then
/// what is sent; the parent and the bug, before then after, and the
/// status; when a new diff is sent, the heading of the code and what the
/// code is, `code` (`Compared::said`).
fn review_rows(p: &Prepared, w: usize, code: Option<(&str, &str)>, age: Option<&str>) -> Vec<Row> {
    let mut rows = vec![row(Kind::File, "what is sent")];
    let n = p.number();
    let id = p.revision.as_ref().map_or("the revision".to_string(), |r| r.id.clone());
    match p.kept_said() {
        Some(kept) => said_rows(&mut rows, Kind::Meta, &kept, w),
        None if !p.new_diff => said_rows(&mut rows, Kind::Meta, &format!("The code does not change: the latest diff of {id} is patch {n} as it is now."), w),
        None => {}
    }
    if let Some(said) = p.changes_said() {
        said_rows(&mut rows, Kind::Meta, &said, w);
    }
    for s in p.said() {
        said_rows(&mut rows, Kind::Meta, &s, w);
    }
    // What the review stands on: the revision as it was fetched, nothing
    // read again before it is sent.
    if let (Some(r), Some(age)) = (&p.revision, age) {
        said_rows(&mut rows, Kind::Meta, &format!("What is shown of {} here was {age}.", r.id), w);
    }
    // The title and summary, as an interdiff compares two messages.
    let before = p.message_before().unwrap_or_default();
    let after = p.message_after();
    if before != after {
        rows.push(row(Kind::Blank, ""));
        rows.push(row(Kind::File, "the title and summary: Phabricator's, then what is sent"));
        message_rows(&mut rows, &before, &after, w);
    }
    // What else changes, before then after; the status, always.
    rows.push(row(Kind::Blank, ""));
    rows.push(row(Kind::File, "what else changes"));
    let none = |l: &[String]| if l.is_empty() { "none".to_string() } else { and(l) };
    if p.parent_changes() {
        diff_rows(&mut rows, &format!("-parent: {}", none(&p.parents_now)), &[], w);
        diff_rows(&mut rows, &format!("+parent: {}", p.parent_after()), &[], w);
    }
    if let (Some(r), Some(b)) = (&p.revision, &p.bug)
        && r.bug != *b
    {
        let was = if r.bug.is_empty() { "none" } else { r.bug.as_str() };
        diff_rows(&mut rows, &format!("-bug: {was}"), &[], w);
        diff_rows(&mut rows, &format!("+bug: {b}"), &[], w);
    }
    match &p.revision {
        Some(r) if r.in_work() && r.status != "changes-planned" => {
            diff_rows(&mut rows, &format!("-status: {}", p.status_was()), &[], w);
            diff_rows(&mut rows, "+status: Changes Planned", &[], w);
        }
        _ => diff_rows(&mut rows, &format!(" status: {}", p.status_change()), &[], w),
    }
    // The code's heading, and what it is.
    if let Some((heading, said)) = code {
        rows.push(row(Kind::Blank, ""));
        rows.push(row(Kind::File, heading));
        said_rows(&mut rows, Kind::Meta, said, w);
        rows.push(row(Kind::Blank, ""));
    }
    rows
}

/// The code of the review of a submit, `code`, each file as Phabricator's
/// latest diff leaves it, then as the patch does, both on the patches below
/// it (`Prepared::code`); and the two diffs it compares, as the view of what
/// differs from Phabricator compares them for the revision and the patch:
/// Phabricator's side, its latest diff, or, known by its changes alone, the
/// version of the patch whose changes it makes, as the review says at its
/// top (`submit::Known::by_changes`); the other, the diff the submit sends
/// (`Prepared::diff`). How the first is named in the views (`D13 on
/// Phabricator, fetched 11 minutes ago`, with how long ago it was fetched,
/// `age`, as the review says it; `version 1 of patch 3`), and in what is
/// said (`the latest diff of D13`); what the views show of them as two
/// diffs (`revisions::shown_of`); and, where that is an interdiff, the two
/// versions it compares and their interdiff (`revisions::named_interdiff`),
/// each diff without what git derives in it (`revisions::bare`), none
/// where it is each file.
struct Compared<'a> {
    code: &'a str,
    old: String,
    said: String,
    shown: CodeShown<'a>,
    two_of: Option<(Patch, Patch, Vec<ops::InterLine>)>,
}

impl<'a> Compared<'a> {
    fn of(p: &'a Prepared, code: &'a str, age: Option<&str>) -> Compared<'a> {
        let n = p.number();
        let id = p.revision.as_ref().map_or("the revision".to_string(), |r| r.id.clone());
        let raw = p.revision.as_ref().map_or("", |r| r.raw.as_str());
        let phabricator = match age {
            Some(age) => format!("{id} on Phabricator, {age}"),
            None => format!("{id} on Phabricator"),
        };
        let (theirs, old, said) = match &p.known {
            Some(k) if k.by_changes => (k.diff.as_str(), format!("version {} of patch {n}", k.version), format!("version {} of patch {n}", k.version)),
            _ => (raw, phabricator, format!("the latest diff of {id}")),
        };
        let shown = shown_of(theirs, &p.diff, Some((code, p.code_files)));
        let two_of = matches!(shown, CodeShown::Interdiff(_)).then(|| {
            let (was, is) = (Patch::new(p.change.clone(), None, None, String::new(), bare(theirs)), Patch::new(p.change.clone(), None, None, String::new(), bare(&p.diff)));
            let inter = named_interdiff(&was, &is, (&old, SENT));
            (was, is, inter)
        });
        Compared { code, old, said, shown, two_of }
    }

    /// The heading of the code, in view `v`, saying which comparison it is,
    /// side by side whose is on which side; and what the review says of
    /// it: compared as two diffs, as the view of what differs from
    /// Phabricator says it (`revisions::shown_said`); as the code, what the
    /// code of `p` is.
    fn heading(&self, p: &Prepared, v: InterLayout) -> (String, String) {
        let (sides, n) = (v.sides(), p.number());
        match (v.change(), self.shown) {
            (true, _) => {
                let id = p.revision.as_ref().map_or("the revision".to_string(), |r| r.id.clone());
                let said = format!("Each file as the latest diff of {id} leaves it, then as patch {n} leaves it, both on the patches below patch {n}.");
                (files_heading(sides, SENT), p.code_said.clone().unwrap_or(said))
            }
            (false, CodeShown::Interdiff(_)) if sides => (format!("the code: the two diffs, Phabricator's on the left, {SENT} on the right"), shown_said(&self.shown, &self.said, n)),
            (false, CodeShown::Interdiff(_)) => (format!("the code: the two diffs, Phabricator's then {SENT}"), shown_said(&self.shown, &self.said, n)),
            (false, _) => (files_heading(sides, SENT), shown_said(&self.shown, &self.said, n)),
        }
    }

    /// The code drawn `w` wide in view `v` of the interdiffs, with only what
    /// the patches changed if `only`. Compared as two diffs, as the view of
    /// what differs from Phabricator draws it: their interdiff, as an
    /// interdiff, in a column per version or side by side
    /// (`revisions::interdiff_drawn`), the two on one base unless the
    /// review says they are not; or each file as each leaves them, as it is,
    /// or side by side (`revisions::files_drawn`). Compared as the code, as
    /// the middle draws a patch, unified, side by side or compact
    /// (`change::change_stops`), on one base nothing left out. Each line with
    /// what it shows, from `first` on: the lines of the interdiff, or of
    /// the code where there is none, by their places there; and, of a file,
    /// its lines of the code (`Seen::rows`). A line of the code shows the
    /// line of the interdiff nearest it (`ops::code_places`), as on the view
    /// of what differs, so that each view finds the cursor's place again.
    fn lines(&self, change: &str, v: InterLayout, only: bool, w: usize, first: usize) -> Vec<(Parts, Seen)> {
        if v.change() {
            let near = match &self.two_of {
                Some((was, is, inter)) => Near::new(&ops::code_places(&inter.iter().collect::<Vec<_>>(), was, is), first, first),
                None => code_near(self.code, first),
            };
            let c = crate::change::Change { diff: self.code.to_string(), dim: Default::default(), rebased: false, files: Default::default() };
            return change_stops(&c, code_view(v), (false, only), w, 0, 0, &near)
                .into_iter()
                .flat_map(|s| {
                    let seen = Seen::from(&s);
                    s.lines.into_iter().map(move |l| (l, seen.clone()))
                })
                .collect();
        }
        if let (CodeShown::Interdiff(drifted), Some((_, _, inter))) = (self.shown, &self.two_of) {
            let two = Two { old: &self.old, new: SENT, one_base: !drifted };
            return interdiff_drawn(inter, v, &two, None, (false, only), w, first);
        }
        files_drawn(change, self.code, v == InterLayout::Sides, w)
            .into_iter()
            .map(|(parts, shows)| {
                let seen = Seen { shows: shows.iter().map(|j| first + j).collect(), rows: shows.iter().map(|&j| (0, j)).collect() };
                (parts, seen)
            })
            .collect()
    }
}

/// Each line of `code`, a diff of files, by its file and its line in each
/// version of the file (`ops::CodePlace`): a file's header at its top, a
/// hunk's header at its first lines, a line taken out in the old version,
/// one put in in the new one, one kept in both. As `change::Near` finds the
/// line of `code` nearest a row of the change drawn from it, the first at
/// place `first` among what is drawn, a row of no file the first.
fn code_near(code: &str, first: usize) -> Near {
    let mut places: Vec<[Option<CodePlace>; 2]> = vec![[None, None]; code.lines().count()];
    for fd in crate::diff::parse_diff(code, "") {
        let at = |line: usize, gone: bool| Some(CodePlace { path: fd.path.clone(), line, gone });
        for j in 0..fd.header.len() {
            if let Some(p) = places.get_mut(fd.at + j) {
                *p = [at(0, false), at(0, false)];
            }
        }
        for h in &fd.hunks {
            if let Some(p) = places.get_mut(h.at) {
                *p = [at(h.lines.first().map_or(0, |l| l.old_no), false), at(h.new_start, false)];
            }
            for l in &h.lines {
                if let Some(p) = places.get_mut(l.at) {
                    *p = match l.origin {
                        '-' => [at(l.old_no, true), None],
                        '+' => [None, at(l.new_no, false)],
                        _ => [at(l.old_no, false), at(l.new_no, false)],
                    };
                }
            }
        }
    }
    Near::new(&places, first, first)
}

/// The review of `p` drawn `w` wide, its code in view `v` of the
/// interdiffs, with only what the patches changed if `only`, saying how
/// long ago what it shows of the revision was fetched, `age`: what it says
/// over the code (`review_rows`), a row each, or, side by side where the
/// view is side by side, as the code view pairs a diff's rows
/// (`interdiffs::paired`), a line of the title and summary, or of what
/// else changes, beside the one it replaces; then the code
/// (`Compared::lines`).
/// Its rows, the first row of each of their lines, and the lines drawn,
/// each with what it shows: the lines its rows are, by their places among
/// the review's, then the lines of the code.
type Drawn = (Vec<Row>, Vec<usize>, Vec<Parts>, Vec<Seen>);

fn review_lines(p: &Prepared, v: InterLayout, only: bool, w: usize, age: Option<&str>) -> Drawn {
    let sides = v.sides();
    let fold = if sides { half_of(w) } else { w };
    let compared = p.code.as_deref().map(|code| Compared::of(p, code, age));
    let said = compared.as_ref().map(|c| c.heading(p, v));
    let rows = review_rows(p, fold.saturating_sub(1), said.as_ref().map(|(h, s)| (h.as_str(), s.as_str())), age);
    // Each row's line, a row folded on with the line it goes on with.
    let mut starts = Vec::new();
    let mut place = Vec::with_capacity(rows.len());
    for (i, r) in rows.iter().enumerate() {
        if !r.cont || starts.is_empty() {
            starts.push(i);
        }
        place.push(starts.len() - 1);
    }
    let cells: Vec<Parts> = rows.iter().map(|r| row_parts(r, false)).collect();
    let (mut lines, mut at) = (Vec::new(), Vec::new());
    match sides {
        true => {
            for (parts, cells, _) in paired(&rows, &cells, w, 0) {
                let mut shows: Vec<usize> = cells.iter().map(|&i| place[i]).collect();
                shows.dedup();
                lines.push(parts);
                at.push(Seen { shows, rows: vec![] });
            }
        }
        false => {
            for (i, parts) in cells.into_iter().enumerate() {
                lines.push(parts);
                at.push(Seen { shows: vec![place[i]], rows: vec![] });
            }
        }
    }
    if let Some(c) = &compared {
        for (parts, seen) in c.lines(&p.change, v, only, w, starts.len()) {
            lines.push(parts);
            at.push(seen);
        }
    }
    (rows, starts, lines, at)
}

/// What the bottom line says while what submitting the patch picked, patch
/// `n`, sends is worked out away from the screen: that what `db` lacks for
/// it, `lacking`, each `D123` and its link (`submit::lacking`), is fetched
/// first, and why: nothing of it is stored yet, or what is, is not all
/// that a submit needs. Else what is worked out: the update of revision
/// `meta`, or the submit of the patch. Nothing is sent yet.
fn working_said(db: &crate::db::Db, lacking: &[(String, String)], meta: Option<&str>, n: usize) -> String {
    if !lacking.is_empty() {
        let names: Vec<String> = lacking.iter().map(|(d, _)| d.clone()).collect();
        let of = if names.len() == 1 { "it" } else { "them" };
        let why = match lacking.iter().all(|(_, link)| db.revision_state(link).is_none()) {
            true => format!("nothing of {of} is stored yet"),
            false => format!("what is stored of {of} is not all that a submit needs"),
        };
        return format!("fetching {} from Phabricator first, as {why}: nothing is sent yet", and(&names));
    }
    match meta {
        Some(d) => format!("working out what updating {d} sends: nothing is sent yet"),
        None => format!("working out what submitting patch {n} sends: nothing is sent yet"),
    }
}

impl App {
    /// Why the patch at `at` is not to be submitted now, if it is not:
    /// HEAD, as last seen for the stack as it is, has its commit naming a
    /// revision the patch does not (`submit::held_by_head`). None when HEAD
    /// is elsewhere, or not known for the stack as it is: then nothing is
    /// guessed.
    pub(super) fn submit_held(&self, at: usize) -> Option<String> {
        let rev = self.db.head(&self.stack).map(|(r, _)| r);
        let seen = self.head.as_ref().filter(|h| h.name == self.stack && h.rev == rev && !h.stale)?;
        crate::submit::held_by_head(seen.head.as_ref()?, at)
    }

    /// Submit to Phabricator as WIP, from the patch's menu, or the update
    /// of its revision's title, summary and parent alone (`meta`): what
    /// sending the patch picked sends, worked out away from the screen from
    /// what the database holds of Phabricator, what it lacks fetched first,
    /// the bottom line saying so (`working_said`); nothing is sent. What is
    /// stored decides which it is: the revision's status. Not while its
    /// commit in the workspace names a revision it does not.
    pub(super) fn submit_patch(&mut self, meta: bool) {
        if self.bg.doing.contains(&SUBMITTING) {
            self.status = "a patch is being submitted to Phabricator already: submit this one once that is done".into();
            return;
        }
        let at = self.sel;
        if let Some(why) = self.submit_held(at) {
            self.status = format!("{why} Nothing was sent.");
            return;
        }
        self.bg.doing.push(SUBMITTING);
        let revision = self.shown_patch().and_then(|p| crate::phab::revision_of(&p.message)).map(|(d, _)| d);
        let lacking = self.current().map_or_else(Vec::new, |s| crate::submit::lacking(&self.db, s, at));
        self.status = working_said(&self.db, &lacking, revision.as_deref().filter(|_| meta), at + 1);
        let (path, name) = (self.path.clone(), self.stack.clone());
        self.spawn(move || Done::Submitting(crate::submit::prepare(&path, &name, at).map(Box::new)));
    }

    /// What working out what is sent came to: the database read again, for
    /// what was fetched first; then the dialog of a new revision, the review
    /// of a revision there already; or, said, why nothing is sent, or that
    /// nothing is to be.
    pub(super) fn submit_read(&mut self, read: std::result::Result<Box<Prepared>, String>) {
        self.bg.doing.retain(|d| *d != SUBMITTING);
        self.reload();
        self.check_line();
        let p = match read {
            Ok(p) => *p,
            Err(why) => {
                self.status = why;
                return;
            }
        };
        if p.nothing() {
            self.status = p.nothing_said();
            return;
        }
        let n = p.number();
        if p.stack != self.stack {
            self.status = format!(
                "what submitting patch {n} of {} sends was read, but you are on {} now, so nothing was sent: submit it again from its menu",
                p.stack, self.stack
            );
            return;
        }
        // Not over what is being written, typed or moved.
        if self.writing().is_some() || !matches!(self.mode, Mode::Normal | Mode::Menu(_) | Mode::Help { .. }) {
            self.status = format!(
                "what submitting patch {n} sends was read, but something is being written or moved, so nothing was sent: submit it again once that is done"
            );
            return;
        }
        match &p.revision {
            None => {
                let items = vec![
                    Item::new("Send it to Phabricator", Action::SendSubmit),
                    Item::new("Cancel: send nothing", Action::CancelSubmit),
                ];
                let mut m = Menu::new(format!("send patch {n} to Phabricator as a new revision?"), items, None);
                m.note = Some(p.said().join("\n"));
                self.status.clear();
                self.mode = Mode::Menu(m);
                self.submitting = Some(Box::new(Submitting::new(p, self.screen)));
            }
            Some(_) => {
                // The bar says how to send it, and how not to.
                self.status.clear();
                self.mode = Mode::Normal;
                self.submitting = Some(Box::new(Submitting::new(p, self.screen)));
                self.screen = Screen::Submit;
            }
        }
    }

    /// The submit seen, sent, away from the screen, as it was seen: nothing
    /// read of Phabricator first, nor after.
    pub(super) fn submit_send(&mut self) {
        let Some(sub) = self.submitting.take() else { return };
        if self.screen == Screen::Submit {
            self.screen = sub.back;
        }
        if self.bg.doing.contains(&SUBMITTING) {
            self.status = "a patch is being submitted to Phabricator already, so nothing was sent: submit this one again once that is done".into();
            return;
        }
        // HEAD seen meanwhile with the patch's commit naming another
        // revision: not sent either.
        if sub.ready.stack == self.stack
            && let Some(why) = self.submit_held(sub.ready.at)
        {
            self.status = format!("{why} Nothing was sent.");
            return;
        }
        self.bg.doing.push(SUBMITTING);
        self.status = match sub.ready.revision.as_ref().filter(|_| !sub.ready.in_work()) {
            Some(r) => format!("updating {} on Phabricator…", r.id),
            None => format!("sending patch {} to Phabricator…", sub.ready.number()),
        };
        // The commit it makes is the workspace's when HEAD of the checkout
        // stacked runs in has it.
        let (path, here) = (self.path.clone(), self.checkout.clone());
        self.spawn(move || Done::Said(SUBMITTING, crate::submit::send(&path, &sub.ready, &here)));
    }

    /// The submit left, nothing sent.
    pub(super) fn submit_cancel(&mut self) {
        if let Some(sub) = self.submitting.take()
            && self.screen == Screen::Submit
        {
            self.screen = sub.back;
        }
        self.status = "nothing was sent to Phabricator".into();
    }

    /// Whether the review of a submit is on the screen with code to show,
    /// a new diff of the patch being sent: the code the dialog of the views
    /// of the interdiffs sets the view of (`App::inter_screen`).
    pub(super) fn submit_code(&self) -> bool {
        self.screen == Screen::Submit && self.submitting.as_ref().is_some_and(|s| s.ready.code.is_some())
    }

    /// The review's own keys: ↑↓ and the rest scroll; Esc opens its menu,
    /// which sends it or not (`submit_items`); Enter, as Space, does
    /// nothing. 1, its code there, opens the dialog of the views of the
    /// interdiffs, as on every screen of them.
    pub(super) fn submit_key(&mut self, k: KeyEvent) -> Effect {
        if k.code == KeyCode::Esc {
            self.menu_here();
            return Effect::None;
        }
        let page = self.page();
        let Some(sub) = self.submitting.as_mut() else {
            self.screen = Screen::Main;
            return Effect::None;
        };
        let last = sub.lines.len().saturating_sub(1);
        sub.cursor = match k.code {
            KeyCode::Down => (sub.cursor + 1).min(last),
            KeyCode::Up => sub.cursor.saturating_sub(1),
            KeyCode::PageDown => (sub.cursor + page).min(last),
            KeyCode::PageUp => sub.cursor.saturating_sub(page),
            KeyCode::Home => 0,
            KeyCode::End => last,
            _ => sub.cursor,
        };
        Effect::None
    }

    /// The review: its title, then its lines from the top shown, the
    /// cursor's lit. Drawn again where the width, the view of the
    /// interdiffs or their lines changed (`review_lines`), or how long ago
    /// what it shows of the revision was fetched, as it says it, worked out
    /// as it is drawn, so that it grows while the review stays open; the
    /// cursor on the place it was on (`interdiffs::locate`,
    /// `interdiffs::find`), as far under the top as it was.
    pub(super) fn draw_submit(&mut self, buf: &mut Buffer, area: Rect) {
        let (w, h) = (area.width as usize, area.height as usize);
        // Scrolled by the wheel: by the lines it turned, as far as the last
        // on the bottom line; and since, until a key, where the wheel left
        // it, the cursor's line in sight or not.
        self.scrolls_in(area, Wheeled::Submit);
        let (turned, free) = (self.turned(Wheeled::Submit), self.free(Wheeled::Submit));
        // Its lines a column short of the screen, which its scroll bar
        // takes: the pane its code is drawn in, too narrow for two sides or
        // not (`inter_in`), where it has code. Without code, there is no
        // view to set: what it says is unified.
        let lw = w.saturating_sub(1);
        let has_code = self.submit_code();
        if has_code {
            self.inter_w.set(lw);
        }
        let v = if has_code { self.inter_in(lw) } else { InterLayout::Interdiff };
        let only = self.only_changed();
        let Some(sub) = self.submitting.as_mut() else { return };
        let age = sub.ready.fetched.map(|t| age_said(t, crate::db::now()));
        let key = (lw, v, self.inter_lines, age);
        if sub.built.as_ref() != Some(&key) {
            let was = sub.built.as_ref().and_then(|_| interdiffs::locate(&sub.at, sub.cursor, sub.kept));
            let under = sub.cursor.saturating_sub(sub.top);
            (sub.rows, sub.starts, sub.lines, sub.at) = review_lines(&sub.ready, key.1, only, lw, key.3.as_deref());
            sub.built = Some(key);
            sub.kept = was.map(|place| (interdiffs::find(&sub.at, place), place));
            if let Some((at, _)) = sub.kept {
                (sub.cursor, sub.top) = (at, at.saturating_sub(under));
            }
        }
        let r = sub.ready.revision.as_ref().map_or(String::new(), |r| format!("{}: ", r.id));
        put(buf, area, 0, 0, w, &fit(&format!(" {r}what is sent to Phabricator, for your review"), w), color::file());
        let all = sub.lines.len();
        sub.cursor = sub.cursor.min(all.saturating_sub(1));
        let rows_h = h.saturating_sub(1).max(1);
        if let Some(n) = turned {
            sub.top = moved(sub.top, n, all.saturating_sub(rows_h));
        }
        if !free {
            if sub.cursor < sub.top {
                sub.top = sub.cursor;
            } else if sub.cursor >= sub.top + rows_h {
                sub.top = sub.cursor + 1 - rows_h;
            }
        }
        // Longer than the screen, its scroll bar beside the lines under its
        // title, in the screen's last column, which its lines leave blank.
        let shown = Shown { top: sub.top, rows: rows_h, all };
        for (y, k) in (sub.top..all).take(rows_h).enumerate() {
            match k == sub.cursor {
                true => put_parts(buf, area, 0, y + 1, lw, &on_cursor_parts(&sub.lines[k])),
                false => put_parts(buf, area, 0, y + 1, lw, &sub.lines[k]),
            }
        }
        self.pane_bar(Wheeled::Submit, area, buf.area, (1, rows_h), shown);
    }

    /// What the review's bar says: Esc for its menu, which sends it or
    /// not; on a line of the title and summary that has links, how to open
    /// them; with code, 1 for its view.
    pub(super) fn submit_hint(&self) -> String {
        let open = self.open_hint();
        let view = if self.submit_code() { "1: view · " } else { "" };
        format!("↑↓: scroll · esc: menu · {open}{view}?: what can I do here")
    }

    /// The lines of the title and summary the review's cursor is on, each as
    /// it is written, its pieces joined and its sign taken off: one, or side
    /// by side, those of the two sides. None elsewhere in the review.
    pub(super) fn submit_message_lines(&self) -> Vec<String> {
        let Some(sub) = self.submitting.as_ref() else { return Vec::new() };
        let rows = &sub.rows;
        // Under their heading, as `review_rows` writes it, up to the empty
        // row after them; what is said over them (`message_rows`) aside.
        let heading = "the title and summary: Phabricator's, then what is sent";
        let Some(head) = rows.iter().position(|r| r.kind == Kind::File && r.text == heading) else { return Vec::new() };
        let end = rows[head..].iter().position(|r| r.kind == Kind::Blank).map_or(rows.len(), |k| head + k);
        // The first row of each line of the review the cursor's line draws.
        let under: Vec<usize> = sub.at.get(sub.cursor).map_or(Vec::new(), |s| s.shows.iter().filter_map(|&l| sub.starts.get(l).copied()).collect());
        let mut out: Vec<String> = Vec::new();
        for first in under.into_iter().filter(|&i| head < i && i < end && rows[i].kind != Kind::Meta) {
            let last = (first + 1..end).find(|&k| !rows[k].cont).unwrap_or(end);
            let line: String = rows[first..last].iter().map(|r| r.text.strip_prefix(crate::rows::FOLDED).filter(|_| r.cont).unwrap_or(r.text.as_str())).collect();
            let mut text = line.chars();
            text.next();
            let text = text.as_str().to_string();
            if !out.contains(&text) {
                out.push(text);
            }
        }
        out
    }

    /// What `?` says on the review: for a revision not in work, that no
    /// code is sent; Esc, and a right click, for the menu that sends it, as
    /// the review shows it, nothing read first, or not; and, with code, 1
    /// for its view.
    pub(super) fn submit_help(&self) -> Vec<String> {
        let ready = self.submitting.as_ref().map(|s| &s.ready);
        let id = ready.and_then(|p| p.revision.as_ref()).map_or("the revision".to_string(), |r| r.id.clone());
        let what = match ready.filter(|p| !p.in_work()) {
            Some(p) => format!(
                "This is what is sent to Phabricator, for you to review before it is: at the top, what is sent and where, and how long ago what is shown of {id} was fetched; then, of what changes, the title and summary, Phabricator's then what is sent; and the parent and the bug, before then after, and the status, which stays as it is. No code is sent: {id} is {}, not in work, and only a revision in work gets a new diff.",
                p.status_was()
            ),
            None => format!(
                "This is what is sent to Phabricator, for you to review before it is: at the top, what is sent and where, and how long ago what is shown of {id} was fetched; then, of what changes, the title and summary, Phabricator's then what is sent; the parent and the bug, before then after, and the status; and the code, each file as the latest diff of {id} leaves it, then as the patch leaves it, both on the patches below it."
            ),
        };
        let mut out = vec![
            what,
            format!(
                "Press Esc for the menu, or click with the right button: Send it to Phabricator sends it as this review shows it, against {id} as it was fetched, nothing read of Phabricator first; Cancel: send nothing sends nothing."
            ),
        ];
        out.push(match self.submit_code() {
            true => "Press ↑↓ (pgup pgdn home end) to scroll, and 1 to pick how the code is drawn: compared as two diffs, Phabricator's and the one sent, as an interdiff, in a column per version or side by side, as what differs from Phabricator draws them; or as the code, each file as both leave it, unified, side by side, or side by side and compact. Side by side, the title and summary, and what else changes, are side by side too.".into(),
            false => "Press ↑↓ (pgup pgdn home end) to scroll.".into(),
        });
        out
    }

    /// The review's menu: send it, or send nothing; with code, the dialog
    /// of its view.
    pub(super) fn submit_items(&self, items: &mut Vec<Item>) {
        items.push(Item::new("Send it to Phabricator", Action::SendSubmit));
        items.push(Item::new("Cancel: send nothing", Action::CancelSubmit));
        if self.submit_code() {
            items.push(Item::line());
            items.push(Item::new(VIEW_OF_CODE, Action::SubmitViews));
        }
    }
}
