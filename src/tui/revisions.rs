//! What Phabricator has of the stack's revisions, as stacked stored it the
//! last time it read Phabricator (`db::Revision`): the status of each
//! patch's revision, which the patches pane shows after the patch's number;
//! and the Phabricator line over the message of the patch the middle
//! shows, which says its revision's status by name, whether Phabricator's
//! latest diff and the revision's title and summary are this version's,
//! its parent the one a submit sets, how many of the revision's comments
//! are not pulled yet, and how long ago that was fetched. Space on the
//! line, or its menu, shows what differs on a screen of its own,
//! Phabricator's version then this one as an interdiff compares two
//! versions. Whether each patch is in sync so, the patches pane marks right
//! after its status (`✓`, `✗`), from the same reading as its line
//! (`Synced`); and the patch's `Phabricator` menu offers, for a revision
//! open but not in work, the update of its title, summary and parent alone,
//! as that reading says what differs (`App::submit_item`). The patch's own
//! `Phabricator` menu, and the line's, hold every item of the patch about
//! Phabricator (`App::phab_items`): its submit, what differs, the pull of
//! its reviews alone, and Fetch the latest from Phabricator, which reads
//! every revision the stack's patches name, away from the screen, and
//! stores it (`phab::fetch_latest`). Nothing of Phabricator is read by
//! itself: only that, a submit, which reads its revision again once it is
//! sent, and a pull. What differs in the code of each patch is worked out away from
//! the screen, from what is stored, one patch at a time, the one shown
//! first, and kept in memory as long as the editor runs. Not while reviews
//! come from nowhere, nor in the tour. Built with the `moz-phab` feature.

use super::compared::{ONLY_WRAPPED, row};
use super::*;
use crate::db::State;
use crate::ops::other_bases;
use crate::phab::Status;
use crate::submit::{Below, Parent};

/// What the top bar says while Phabricator is fetched.
pub(super) const FETCHING: &str = "fetching";

/// The item of the menus that fetches the latest from Phabricator.
pub(super) const FETCH: &str = "Fetch the latest from Phabricator";

/// The item of the stack's menu that pulls the stack from Phabricator.
pub(super) const PULL: &str = "Pull from Phabricator";

/// What the bottom line says when a pull is asked while one goes.
const PULLING_ALREADY: &str = "already pulling from Phabricator: one pull at a time";

/// What the Phabricator line says of a revision nothing of which is stored
/// yet.
const NOT_FETCHED: &str = "not fetched yet: Fetch the latest from Phabricator, in its menu";

/// What the Phabricator line says of the files when they cannot be read as
/// the patches below leave them.
const AS_TEXT: &str = "The files could not be read as the patches below this one leave them, so this is Phabricator's latest diff against this version's diff, as text.";

/// What the code of patch `n` (from 1) is compared on, said: each version
/// applied on the stack's base for the first, else on the patches below it.
fn below(n: usize) -> String {
    match n {
        1 => "both on the stack's base".to_string(),
        _ => format!("both on the patches below patch {n}"),
    }
}

/// How long before `now` it was that `then` was, in seconds since 1970, as
/// the Phabricator line says how long ago what it says was fetched: `just
/// now` under a minute, then in minutes, then in hours, `yesterday` from a
/// day to two, then in days. A time to come, as a clock set otherwise
/// gives, is just now.
fn age_said(then: i64, now: i64) -> String {
    let secs = now.saturating_sub(then).max(0);
    let ago = |n: i64, unit: &str| format!("fetched {n} {unit}{} ago", if n == 1 { "" } else { "s" });
    match secs {
        s if s < 60 => "fetched just now".into(),
        s if s < 3600 => ago(s / 60, "minute"),
        s if s < 86_400 => ago(s / 3600, "hour"),
        s if s < 2 * 86_400 => "fetched yesterday".into(),
        s => ago(s / 86_400, "day"),
    }
}

/// What differs in the code between a revision's latest diff and a version
/// of its patch: nothing, as they do the same; or the code from the one to
/// the other, as a diff, and what that diff is when it is not each file as
/// both leave it.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Code {
    Same,
    Differs(String, Option<String>),
}

/// What differs between a version of a patch and its revision, once all of
/// it is known: the code, as a diff, and what it is; the title and summary,
/// Phabricator's then this version's; none of either where it is the same.
/// And the parent a submit gives the revision, by the submit's own rule
/// (`submit::parent_with`), which differs where a submit sets one; with
/// the parents the revision has, by name.
struct Differs<'a> {
    code: Option<(&'a String, &'a Option<String>)>,
    message: Option<(String, String)>,
    parent: Parent,
    parents_now: Vec<String>,
}

impl Differs<'_> {
    /// Whether the parent differs: a submit sets one.
    fn parent_differs(&self) -> bool {
        matches!(self.parent, Parent::Set { .. })
    }

    /// Whether nothing differs: Phabricator has this exact version, and a
    /// submit would set no parent.
    fn none(&self) -> bool {
        self.code.is_none() && self.message.is_none() && !self.parent_differs()
    }

    /// What the Phabricator line says of it.
    fn said(&self) -> &'static str {
        match (self.code.is_some(), self.message.is_some(), self.parent_differs()) {
            (false, false, false) => "in sync",
            (true, false, false) => "the diff differs",
            (false, true, false) => "the title or summary differs",
            (false, false, true) => "the parent differs",
            (true, true, false) => "the diff and the title or summary differ",
            (true, false, true) => "the diff and the parent differ",
            (false, true, true) => "the title or summary and the parent differ",
            (true, true, true) => "the diff, the title or summary, and the parent differ",
        }
    }

    /// What is the same, in sync, said, of revision `d` and `this`, the
    /// version of the patch: the code, the title and summary, and the
    /// parent, the one a submit sets, or why a submit sets none, as the
    /// submit says it.
    fn same_said(&self, d: &str, this: &str) -> String {
        let code = format!("the latest diff of {d} does what {this} does");
        let message = format!("the title and summary of {d} are those of this version");
        match &self.parent {
            Parent::Already { id, below } => format!("{code}, {message}, and its parent is {id}, the revision of patch {below}."),
            Parent::Nothing(why) => format!("{code}, and {message}. {why}"),
            Parent::Set { .. } => format!("{code}, and {message}."),
        }
    }

    /// The parent, when a submit sets one, said: the parents the revision
    /// has on Phabricator, then the one a submit sets.
    fn parent_said(&self) -> Option<String> {
        let Parent::Set { id, below, .. } = &self.parent else { return None };
        let now = match self.parents_now.as_slice() {
            [] => "Parent: none".to_string(),
            [one] => format!("Parent: {one}"),
            all => format!("Parents: {}", crate::submit::and(all)),
        };
        Some(format!("{now} on Phabricator; a submit sets {id}, the revision of patch {below}."))
    }
}

/// Whether Phabricator has the exact version of a patch, read once
/// (`App::synced`) for all that says it: the Phabricator line over the
/// patch's message, in words, and the patches pane, a mark after the
/// revision's status (`mark`); so that the two never disagree.
enum Synced<'a> {
    /// Phabricator does not have the revision: the line says so, alone.
    NotFound,
    /// Not known yet: the revision, its latest diff raw, or the revision of
    /// the patch below, not stored yet, or the comparison of its code with
    /// this version not worked out yet. The line has `…` in its place.
    Unknown,
    /// Known: what differs, or that nothing does.
    Known(Differs<'a>),
}

impl Synced<'_> {
    /// What the patches pane marks after the revision's status: `✓` where
    /// the line says in sync, `✗` where it says what differs; none, a
    /// blank, where it says neither.
    fn mark(&self) -> Option<&'static str> {
        match self {
            Synced::Known(x) if x.none() => Some("✓"),
            Synced::Known(_) => Some("✗"),
            Synced::NotFound | Synced::Unknown => None,
        }
    }
}

/// What the code of a version of a patch is compared with its revision's
/// latest diff from, away from the screen: Phabricator's diff, the
/// version's, the diffs of the patches below it, the stack's repository and
/// base, when it has them, and the patch's place in the stack.
struct Job {
    raw: String,
    mine: String,
    below: Vec<String>,
    pad: Option<(PathBuf, String)>,
    at: usize,
}

/// What differs in the code between `raw`, Phabricator's latest diff of the
/// patch at `at`, and `mine`, the patch as it is: nothing when the two do
/// the same (`ops::same_effect`), or leave each file the same, both applied
/// on the patches below it, `below`, on `pad`'s base in its repository, as
/// a submit's check takes them; else the code from one to the other, each
/// file as each leaves it (`submit::files_diff`). Where the files cannot be
/// read so, the two diffs diffed as text, said.
fn compare(job: Job) -> Code {
    if crate::ops::same_effect(&job.raw, &job.mine) {
        return Code::Same;
    }
    let below: Vec<&str> = job.below.iter().map(String::as_str).collect();
    match job.pad.and_then(|(repo, base)| crate::submit::files_diff(&repo, &base, &below, &job.raw, &job.mine)) {
        Some(None) => Code::Same,
        Some(Some(code)) => Code::Differs(code, None),
        None => Code::Differs(crate::submit::diffs_diffed(job.at, &job.raw, &job.mine), Some(AS_TEXT.into())),
    }
}

/// The title and summary of a revision, `title` and `summary` as its search
/// found them, and as patch `p` would send them (`submit::message_against`):
/// none when they are the same to the letter.
fn message_against(title: &str, summary: &str, p: &Patch, trailer: &str) -> Option<(String, String)> {
    crate::submit::message_against(title, summary, p, trailer)
}

/// Whether two versions of a title and summary, `before` and `after`, have
/// the same words, and differ only in where their lines wrap, or in their
/// white space (`messages::diff`).
fn only_wrapped(before: &str, after: &str) -> bool {
    before != after && crate::messages::diff(before, after).runs.iter().all(|r| r.same)
}

/// `code`, each file of change `change` as one version leaves it, then as
/// the other does, as a diff (`submit::files_diff`), the two side by side
/// `w` wide, as the code view puts a patch's files side by side: the old
/// file's lines on the left and the new one's on the right, each as the
/// middle draws it (`row_parts`), the lines both have dim, as an interdiff
/// side by side has them (`interdiffs::paired`); the files' names and the
/// hunks' headers across both halves. Each line drawn with the lines of
/// `code` it shows, by their places there.
fn files_side_by_side(change: &str, code: &str, w: usize) -> Vec<(Parts, Vec<usize>)> {
    let fold = (w.saturating_sub(3) / 2).saturating_sub(1);
    let all = patch_rows(&Patch::new(change.to_string(), None, None, String::new(), code.to_string()), &[], fold, &Deco::default());
    // Its message's rows, empty, left out.
    let from = all.iter().position(|r| r.kind == Kind::Blank).map_or(0, |k| k + 1);
    let rows: Vec<Row> = all.into_iter().skip(from).collect();
    let cells: Vec<Parts> = rows
        .iter()
        .map(|r| match r.kind {
            Kind::Ctx => row_parts(r, false).into_iter().map(|(text, st)| (text, st.add_modifier(Modifier::DIM))).collect(),
            _ => row_parts(r, false),
        })
        .collect();
    // The line of `code` each row is, the one before's where it says none.
    let mut at = 0;
    let lines: Vec<usize> = rows
        .iter()
        .map(|r| {
            at = r.diff_at.unwrap_or(at);
            at
        })
        .collect();
    interdiffs::paired(&rows, &cells, w, 0).into_iter().map(|(parts, cells, _)| (parts, cells.iter().map(|&i| lines[i]).collect())).collect()
}

/// Diff `d` without what git derives in it, of which Phabricator's raw
/// diffs have nothing: a file's `index` line and its similarity, the white
/// space ending a line of a file's header, and the heading git writes after
/// a hunk's second `@@`. As the view of what differs compares a revision's
/// latest diff with the patch's, so that only what the two do, and where,
/// is told apart.
fn bare(d: &str) -> String {
    let mut in_hunk = false;
    let mut out = String::new();
    for l in d.split_inclusive('\n') {
        let line = l.strip_suffix('\n').unwrap_or(l);
        if line.starts_with("diff --git ") {
            in_hunk = false;
        }
        if let Some(rest) = line.strip_prefix("@@") {
            // `@@ -1,3 +1,3 @@ fn main() {`: up to its second `@@`.
            in_hunk = true;
            let end = rest.find("@@").map_or(line.len(), |k| k + 4);
            out.push_str(&line[..end]);
            out.push('\n');
        } else if in_hunk {
            out.push_str(l);
        } else {
            let header = line.trim_end();
            if !["index ", "similarity index ", "dissimilarity index "].iter().any(|p| header.starts_with(p)) {
                out.push_str(header);
                out.push('\n');
            }
        }
    }
    out
}

/// What the view of what differs from Phabricator shows of the code: of a
/// revision's latest diff and a version of its patch (`App::shown_code`).
#[derive(Debug, Clone, Copy, PartialEq)]
enum Shown<'a> {
    /// Nothing: the latest diff does what the version does.
    Same,
    /// The two diffs, as an interdiff, each without what git derives in it
    /// (`bare`); and whether the latest diff is on another base, its files
    /// not read as the patches below leave them.
    Interdiff(bool),
    /// Each file as the latest diff leaves it, then as the version does,
    /// both on the patches below, as the Phabricator line compared them:
    /// the latest diff is on another base (`other_bases`).
    Files(&'a str),
}

/// How many comments are not pulled yet, new and edited, and how many
/// suggested edits of comments filed without them (`review::not_pulled`),
/// as the Phabricator line says it.
fn comments_said((new, edited, anew): (usize, usize, usize)) -> String {
    let n = |k: usize, what: &str| format!("{k} {what}{}", if k == 1 { "" } else { "s" });
    let what: Vec<String> = [(new, "new comment"), (edited, "edited comment"), (anew, "suggested edit")].into_iter().filter(|(k, _)| *k > 0).map(|(k, what)| n(k, what)).collect();
    match what.is_empty() {
        true => "no new comments".into(),
        false => format!("{} to pull", crate::head::and(&what)),
    }
}

/// `text` folded between words into lines of at most `w` characters, each
/// after the first three spaces in, under the first; as it is when it fits,
/// or the pane is too narrow to fold it.
fn fold_line(text: &str, w: usize) -> Vec<String> {
    if text.chars().count() <= w || w < 20 {
        return vec![text.to_string()];
    }
    let mut out: Vec<String> = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let room = if out.is_empty() { w } else { w - 3 };
        if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > room {
            out.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        out.push(line);
    }
    out.into_iter().enumerate().map(|(k, l)| if k == 0 { l } else { format!("   {l}") }).collect()
}

/// What the editor keeps in memory of Phabricator's revisions, all else
/// being stored: what differs in the code between a revision's latest
/// diff, as stored, and a version of its patch, on the patches below it,
/// by what it was worked out from (`compare_key`); and the one being worked
/// out.
#[derive(Default)]
pub(super) struct Comparisons {
    compared: HashMap<String, Code>,
    comparing: Option<String>,
    /// How many times what the line may say changed as a comparison was
    /// worked out: the middle's rows are built again each time.
    seq: u64,
    /// The change of the patch the submit under way sends: its revision
    /// fetched again once the submit is done (`App::fetch_sent`).
    sent: Option<String>,
}

impl App {
    /// The revisions the current stack's patches name, `D123` and its link
    /// (`phab::named_by`): each once, by link.
    fn revisions_named(&self) -> Vec<(String, String)> {
        self.current().map_or_else(Vec::new, |s| crate::phab::named_by(&self.db, s))
    }

    /// Whether this editor reads Phabricator, and shows what is stored of
    /// it: not while reviews come from nowhere, nor in the tour; nor from a
    /// process that keeps nothing, as the tests' own, without a stand-in.
    fn reads_phabricator(&self) -> bool {
        if self.cfg.reviews.source == crate::config::Source::None {
            return false;
        }
        #[cfg(feature = "demo")]
        if crate::memory::is_file(&self.path) {
            return false;
        }
        crate::phab::statuses_read(&self.path)
    }

    /// What is stored of the state on Phabricator of the revision at
    /// `link` (`db::State`): none before any read stored it.
    fn state(&self, link: &str) -> Option<&State> {
        self.db.revision_state(link).map(|(s, _)| s)
    }

    /// The raw latest diff of the revision at `link`, as stored: none while
    /// it is not, or the revision has none.
    fn raw(&self, link: &str) -> Option<&str> {
        let s = self.state(link).filter(|s| !s.missing && s.diff != 0)?;
        self.db.raw_diff(link, s.diff)
    }

    /// When what the Phabricator line of the revision at `link` says was
    /// fetched, in seconds since 1970: of its state and its comments, of
    /// those stored, the one read longest ago, each as of the read that
    /// stored it, or of the latest fetch that found it as it was stored.
    /// None while its state is not stored: the line says that it is not
    /// fetched yet.
    fn fetched_at(&self, link: &str) -> Option<i64> {
        let (_, state) = self.db.revision_state(link)?;
        let fetched = self.db.fetched(link);
        let at = |t: i64| fetched.map_or(t, |f| f.max(t));
        let comments = self.db.revision_comments(link).map(|(_, t)| at(t));
        Some(comments.map_or(at(state), |c| c.min(at(state))))
    }

    /// The status of patch `id`'s revision, as stored: none when it names
    /// none, nothing of it is stored, Phabricator does not have it, or the
    /// editor does not read Phabricator.
    pub(super) fn revision_status(&self, id: &str) -> Option<Status> {
        if !self.reads_phabricator() {
            return None;
        }
        let (_, link) = crate::phab::revision_of(&self.db.patch(id).message)?;
        self.state(&link).filter(|s| !s.missing).map(|s| Status::from_value(&s.status))
    }

    /// The mark the patches pane shows right after the status of patch
    /// `id`'s revision: `✓` where Phabricator has this exact version of the
    /// patch, `✗` where anything differs, as its Phabricator line says it,
    /// from the same reading (`synced`). None, a blank, while that is not
    /// known, or Phabricator does not have the revision; and when the patch
    /// names no revision, or the editor does not read Phabricator.
    pub(super) fn sync_mark(&self, id: &str) -> Option<&'static str> {
        if !self.reads_phabricator() {
            return None;
        }
        let p = self.db.patch(id);
        let (_, link) = crate::phab::revision_of(&p.message)?;
        self.synced(p, &link).mark()
    }

    /// Whether what Phabricator has is being fetched, or compared with a
    /// patch.
    pub(super) fn reading_phabricator(&self) -> bool {
        self.bg.doing.contains(&FETCHING) || self.comparisons.comparing.is_some()
    }

    // -- the Phabricator line over the patch's message ---------------------- //

    /// What the middle's rows are built again for, as the Phabricator line
    /// changes, beyond what the database holds: how many times a comparison
    /// was worked out, and how long ago what the line of the patch shown
    /// says was fetched, as the line says it, so that it is said again as
    /// it is drawn.
    pub(super) fn line_key(&self) -> String {
        let link = self.shown_patch().and_then(|p| crate::phab::revision_of(&p.message)).map(|(_, link)| link);
        let age = link.and_then(|l| self.fetched_at(&l)).map(|t| age_said(t, crate::db::now()));
        format!("{}:{}", self.comparisons.seq, age.unwrap_or_default())
    }

    /// What the code of patch `p`, the one picked, is compared with its
    /// revision's latest diff (at `link`) by: that diff, the version, the
    /// patches below it and the stack's base. None while that diff is not
    /// stored.
    fn compare_key(&self, p: &Patch, link: &str) -> Option<String> {
        self.raw(link)?;
        let diff = self.state(link)?.diff;
        let s = self.current()?;
        let at = s.patches.iter().position(|id| *id == p.id)?;
        Some(format!("{link}\0{diff}\0{}\0{}\0{}", p.id, s.patches[..at].join(","), s.base.as_deref().unwrap_or("")))
    }

    /// What comparing the code of patch `p` with its revision's latest diff
    /// (at `link`), as stored, takes, away from the screen.
    fn compare_job(&self, p: &Patch, link: &str) -> Option<Job> {
        let raw = self.raw(link)?.to_string();
        let s = self.current()?;
        let at = s.patches.iter().position(|id| *id == p.id)?;
        Some(Job {
            raw,
            mine: p.diff.clone(),
            below: s.patches[..at].iter().map(|id| self.db.patch(id).diff.clone()).collect(),
            pad: s.repo.clone().zip(s.base.clone()).map(|(repo, base)| (PathBuf::from(repo), base)),
            at,
        })
    }

    /// What the Phabricator lines of the stack's patches need, started away
    /// from the screen when it is due: what differs in the code of each
    /// patch, worked out from what is stored, once for each version of it on
    /// the patches below it and each latest diff, the one the middle shows
    /// first (`check_compare`). One comparison at a time; nothing read of
    /// Phabricator. Not while the editor does not read Phabricator.
    pub(super) fn check_line(&mut self) {
        if !self.reads_phabricator() {
            return;
        }
        self.check_compare();
    }

    /// The revision at `link`, the one of a patch below another, as it is
    /// stored, for the parent's rule (`submit::parent_with`): not known
    /// while nothing of it is.
    fn below(&self, link: &str) -> std::result::Result<Below, ()> {
        match self.state(link) {
            Some(s) if s.missing => Ok(Below::Missing),
            Some(s) => {
                let status = Status::from_value(&s.status);
                Ok(Below::Found { phid: s.phid.clone(), open: status.open(), status: status.name().to_string() })
            }
            None => Err(()),
        }
    }

    /// What differs in the code of the next patch due a comparison,
    /// worked out away from the screen (`next_compare`), one at a time.
    fn check_compare(&mut self) {
        if self.comparisons.comparing.is_some() {
            return;
        }
        let Some((key, job)) = self.next_compare() else { return };
        self.comparisons.comparing = Some(key.clone());
        self.spawn(move || Done::Compared(key, compare(job)));
    }

    /// The next patch of the current stack whose code is due a comparison
    /// with its revision's latest diff, once that is stored: the one the
    /// middle shows first, then the others, bottom first, each compared
    /// once until it, the patches below it, or the diff changes
    /// (`compare_key`). What it is compared by, and what that takes.
    fn next_compare(&self) -> Option<(String, Job)> {
        let s = self.current()?;
        let shown = self.patch_id().filter(|_| !self.on_stack);
        shown.iter().chain(&s.patches).find_map(|id| {
            let p = self.db.patch(id);
            let (_, link) = crate::phab::revision_of(&p.message)?;
            let key = self.compare_key(p, &link).filter(|k| !self.comparisons.compared.contains_key(k))?;
            Some((key, self.compare_job(p, &link)?))
        })
    }

    /// What differs in the code, worked out from `key`, kept; then what the
    /// lines need next.
    pub(super) fn code_compared(&mut self, key: String, code: Code) {
        self.comparisons.comparing = None;
        self.comparisons.compared.insert(key, code);
        self.comparisons.seq += 1;
        self.check_line();
    }

    /// What differs between patch `p` and its revision at `link`, once what
    /// is stored of the revision, its latest diff raw, the comparison of its
    /// code with this version, and what a submit gives it as its parent
    /// (`parent_of`) are known.
    fn differs(&self, p: &Patch, link: &str) -> Option<Differs<'_>> {
        let s = self.state(link).filter(|s| !s.missing)?;
        let code = self.comparisons.compared.get(&self.compare_key(p, link)?)?;
        Some(Differs {
            code: match code {
                Code::Same => None,
                Code::Differs(code, said) => Some((code, said)),
            },
            message: message_against(&s.title, &s.summary, p, &self.cfg.reviews.trailer),
            parent: self.parent_of(p, s)?,
            parents_now: s.parents.iter().map(|(_, d)| d.clone()).collect(),
        })
    }

    /// The parent a submit gives the revision of patch `p`, `s` what is
    /// stored of it, its parents with the rest, by the submit's own rule
    /// (`submit::parent_with`), the revision of the patch below as it is
    /// stored: none while that is not known.
    fn parent_of(&self, p: &Patch, s: &State) -> Option<Parent> {
        let stack = self.current()?;
        let at = stack.patches.iter().position(|id| *id == p.id)?;
        let (d, _) = crate::phab::revision_of(&p.message)?;
        let parents: Vec<String> = s.parents.iter().map(|(phid, _)| phid.clone()).collect();
        crate::submit::parent_with(&self.db, stack, at, Some((d.as_str(), parents.as_slice())), |_, below| self.below(below)).ok()
    }

    /// Whether Phabricator has the exact version of patch `p`, of its
    /// revision at `link` (`Synced`): the one place the Phabricator line,
    /// its view of what differs, its menu and `?`, and the patches pane's
    /// mark read it from. Not found, when the last read of it did not find
    /// it; else what differs (`differs`), once all of that is known.
    fn synced(&self, p: &Patch, link: &str) -> Synced<'_> {
        if self.state(link).is_some_and(|s| s.missing) {
            return Synced::NotFound;
        }
        self.differs(p, link).map_or(Synced::Unknown, Synced::Known)
    }

    /// The Phabricator line of patch `p`, of revision `d` at `link`, `sync`
    /// whether Phabricator has this version (`synced`): the revision; its
    /// status, by name; whether its latest diff, title and summary are this
    /// version's, and its parent the one a submit sets; how many of its
    /// comments are not pulled yet; then how long ago that was fetched
    /// (`fetched_at`), computed as it is drawn. What is not known yet is
    /// `…`, once for those in a row; a revision the last read did not find,
    /// said so, with how long ago; one nothing of which is stored, that it
    /// is not fetched yet, and where to fetch it.
    fn line_said(&self, p: &Patch, d: &str, link: &str, sync: &Synced) -> String {
        let Some(at) = self.fetched_at(link) else { return format!("Phabricator: {d} · {NOT_FETCHED}") };
        let age = age_said(at, crate::db::now());
        let sync = match sync {
            Synced::NotFound => return format!("Phabricator: {d} · not found on Phabricator · {age}"),
            Synced::Unknown => None,
            Synced::Known(x) => Some(x.said().to_string()),
        };
        let status = self.state(link).map(|s| Status::from_value(&s.status).name().to_string());
        let comments = self.db.revision_comments(link).map(|(c, _)| {
            let all: Vec<crate::phab::Remote> = c.list.iter().map(crate::phab::remote_of).collect();
            comments_said(crate::review::not_pulled(&self.db, &self.stack, &p.change, &all))
        });
        let mut out = format!("Phabricator: {d}");
        let mut unknown = false;
        for field in [status, sync, comments] {
            match field {
                Some(f) => {
                    out += &format!(" · {f}");
                    unknown = false;
                }
                None if !unknown => {
                    out += " · …";
                    unknown = true;
                }
                None => {}
            }
        }
        out + &format!(" · {age}")
    }

    /// The rows over patch `p`'s message, `w` wide, the middle showing it:
    /// the Phabricator line, folded between words where the pane is
    /// narrower. None when the patch names no revision, or the editor does
    /// not read Phabricator.
    pub(super) fn line_rows(&self, p: &Patch, w: usize) -> Vec<Row> {
        if !self.reads_phabricator() {
            return Vec::new();
        }
        let Some((d, link)) = crate::phab::revision_of(&p.message) else { return Vec::new() };
        fold_line(&self.line_said(p, &d, &link, &self.synced(p, &link)), w)
            .into_iter()
            .enumerate()
            .map(|(k, text)| Row { cont: k > 0, phab: true, ..row(Kind::Meta, text) })
            .collect()
    }

    /// What the view of what differs, `x`, between patch `p` and its
    /// revision at `link` shows of the code (`Shown`): nothing when the
    /// revision's latest diff does what the patch does; else the two diffs
    /// as an interdiff, but when the latest is on another base
    /// (`other_bases`) and the files were read as the patches below leave
    /// them, those files, as the line compared them.
    fn shown_code<'a>(&'a self, p: &Patch, link: &str, x: &Differs<'a>) -> Shown<'a> {
        let raw = self.raw(link).unwrap_or_default();
        match x.code {
            None => Shown::Same,
            Some((files, None)) if other_bases(raw, &p.diff) => Shown::Files(files),
            Some((_, unread)) => Shown::Interdiff(unread.is_some() && other_bases(raw, &p.diff)),
        }
    }

    /// The view of what differs, `x`, between patch `p`, the one picked,
    /// and its revision `d` at `link`: its title, and its lines `w` wide,
    /// each with the lines of what the view shows that it shows
    /// (`InterText`): what is said first, then the interdiff's, then the
    /// code's, as the view draws them as an interdiff, each by its place.
    /// What it shows, said first, the parent first when a submit sets one,
    /// then Phabricator's version of the patch, the revision's title and
    /// summary as a submit compares them (`message_against`) over its
    /// latest diff, then the patch as it is, as two versions of a patch are
    /// compared (`ops::letter_interdiff_lines`, the title and summary where
    /// their lines wrap aside, to the letter), the code as an interdiff of
    /// the two diffs, each without what git derives in it (`bare`); or, the
    /// latest diff on another base, each file as each leaves it, under the
    /// title and summary (`shown_code`). The code the same, the title and
    /// summary alone; both the same, nothing under what it says. In sync,
    /// only that nothing differs, the parent named. In view `v`, side by
    /// side, the interdiff's halves headed with what each version is, and
    /// each file as each leaves it side by side too (`files_side_by_side`).
    /// In a view of the change, the code is the change from the latest diff,
    /// on the commit it is on, to this version (`phab_change`), read from
    /// the repository away from the screen; where it cannot be read, the
    /// interdiff, as an interdiff, said why. All else from what is stored,
    /// nothing read.
    fn view_of(&self, p: &Patch, d: &str, link: &str, x: &Differs, w: usize, v: InterLayout) -> InterDrawn {
        let n = self.sel + 1;
        let title = format!("{d} on Phabricator, then patch {n} as it is");
        let (theirs_said, mine_said) = (format!("{d} on Phabricator"), format!("patch {n} as it is"));
        let mut said: Vec<String> = Vec::new();
        let mut inter: Vec<ops::InterLine> = Vec::new();
        let mut files: Option<&str> = None;
        // In a view of the change, the change from the latest diff, on the
        // commit it is on, to this version; where it cannot be read, the
        // view as an interdiff, said why.
        let (mut v, mut change) = (v, None);
        // The latest diff on another base, its files not read: the two
        // diffs compared all the same, as the screen says.
        let mut drifted = false;
        if x.none() {
            said.push(format!("Nothing differs: {}", x.same_said(d, &format!("this version of patch {n}"))));
        } else {
            said.extend(x.parent_said());
            let shown = self.shown_code(p, link, x);
            drifted = shown == Shown::Interdiff(true);
            said.push(match &x.message {
                Some((before, after)) if only_wrapped(before, after) => format!(
                    "The title and summary of {d} are compared with those a submit of this version sends, where their lines wrap aside. {ONLY_WRAPPED} Each version's lines are shown as it writes them."
                ),
                Some(_) => format!("The title and summary of {d} are compared with those a submit of this version sends, where their lines wrap aside."),
                None => format!("The title and summary of {d} are those of this version."),
            });
            // Phabricator's version, then this one: over the diffs the
            // interdiff compares, or, where it compares none, this version's
            // under both.
            let raw = self.raw(link).unwrap_or_default();
            let (theirs, mine) = match shown {
                Shown::Interdiff(_) => (bare(raw), bare(&p.diff)),
                _ => (p.diff.clone(), p.diff.clone()),
            };
            let (before, after) = x.message.clone().unwrap_or_else(|| (p.message.clone(), p.message.clone()));
            let (was, is) = (Patch { message: before, diff: theirs, ..p.clone() }, Patch { message: after, diff: mine, ..p.clone() });
            inter = ops::letter_interdiff_lines(&was, &is);
            // Its two first lines name the two versions.
            if let [a, b, ..] = inter.as_mut_slice()
                && a.text.starts_with("--- ")
                && b.text.starts_with("+++ ")
            {
                a.text = format!("--- {theirs_said}");
                b.text = format!("+++ {mine_said}");
            }
            if v.change() && shown != Shown::Same {
                let refs: Vec<&ops::InterLine> = inter.iter().collect();
                match self.phab_change(p, link, &refs, (&was, &is), [&theirs_said, &mine_said]) {
                    (super::change::Changed::Cannot(why) | super::change::Changed::Failed(why), _) => {
                        said.push(format!("The change cannot be drawn: {why}."));
                        v = InterLayout::Interdiff;
                    }
                    read => change = Some(read),
                }
            }
            said.push(match shown {
                Shown::Same => format!("The latest diff of {d} does what this version does."),
                // In a view of the change, what it says over its code.
                _ if change.is_some() => String::new(),
                Shown::Interdiff(false) => format!("The code is shown as an interdiff of the latest diff of {d} and this version's diff."),
                Shown::Interdiff(true) => format!(
                    "The latest diff of {d} is on another base than this version, and the files could not be read as the patches below this one leave them, so the code is shown as an interdiff of the two diffs all the same."
                ),
                Shown::Files(_) => format!(
                    "The latest diff of {d} is on another base than this version, so the code shown is each file as that diff leaves it, then as this version leaves it, {}.",
                    below(n)
                ),
            });
            if let Shown::Files(code) = shown {
                files = Some(code);
            }
        }
        said.retain(|l| !l.is_empty());
        let said = wrap(&said.join(" "), w.saturating_sub(2).max(10));
        let only = |k: usize| interdiffs::Seen { shows: vec![k], rows: vec![] };
        let mut out: Vec<Parts> = said.iter().map(|l| vec![(format!(" {l}"), color::meta())]).collect();
        let mut at: Vec<interdiffs::Seen> = (0..said.len()).map(only).collect();
        // The places of what follows, from there on.
        let mut id = said.len();
        // Lines of a diff as an interdiff, folded to the width under their
        // signs, each piece with its line, the first of them at `from`.
        let folded = |all: &[ops::InterLine], from: usize, out: &mut Vec<Parts>, at: &mut Vec<interdiffs::Seen>| {
            for (j, parts) in inter_parts(&all.iter().collect::<Vec<_>>()).iter().enumerate() {
                for piece in fold_parts(parts, w, 1) {
                    out.push(piece);
                    at.push(only(from + j));
                }
            }
        };
        if !inter.is_empty() {
            out.push(vec![(String::new(), Style::new())]);
            at.push(only(id));
            id += 1;
            // In the view of the interdiffs the screen draws: what says how
            // to read it first, as an interdiff or in a column per version,
            // folded to the width under the first sign; side by side, a
            // heading, its halves folded already. The two versions on one
            // base unless the screen says that they are not. In a view of the
            // change, the title and summary compared, then the change from
            // the latest diff, on the commit it is on, to this version, on
            // the patches below it.
            let two = interdiffs::Two { old: &theirs_said, new: &mine_said, one_base: !drifted };
            let refs: Vec<&ops::InterLine> = inter.iter().collect();
            for stop in interdiffs::view_stops(&refs, v, &two, change.as_ref(), self.change_whole, w, 0) {
                let seen = interdiffs::Seen { shows: stop.shows.iter().map(|j| id + j).collect(), rows: stop.rows.clone() };
                for parts in stop.lines {
                    // Side by side, its halves folded already; a line of the
                    // change fits.
                    for piece in if v == InterLayout::Sides { vec![parts] } else { fold_parts(&parts, w, 1) } {
                        out.push(piece);
                        at.push(seen.clone());
                    }
                }
            }
            id += inter.len();
        }
        if let Some(code) = files.filter(|_| change.is_none()) {
            out.push(vec![(String::new(), Style::new())]);
            // A diff of the files, one sign before each line: as an
            // interdiff and in a column per version, as it is.
            let heading = match v {
                InterLayout::Sides => " the code: Phabricator's on the left, this version's on the right",
                _ => " the code: Phabricator's, then this version's",
            };
            out.push(vec![(heading.to_string(), color::file())]);
            at.extend([only(id), only(id + 1)]);
            id += 2;
            if v == InterLayout::Sides {
                for (parts, shows) in files_side_by_side(&p.change, code, w) {
                    out.push(parts);
                    at.push(interdiffs::Seen { shows: shows.iter().map(|k| id + k).collect(), rows: vec![] });
                }
            } else {
                folded(&code.lines().map(ops::InterLine::from).collect::<Vec<_>>(), id, &mut out, &mut at);
            }
        }
        (title, out, at)
    }

    /// The change from the latest diff of the revision at `link` to patch
    /// `p` as it is, named `names` (`App::changed`): that diff on the commit
    /// it is on (`phab_on`), the patch on the stack's base with the patches
    /// below it; with the lines of their interdiff, `lines`, of versions
    /// `was` and `is` as it compares them, nearest each line of it.
    fn phab_change(&self, p: &Patch, link: &str, lines: &[&ops::InterLine], (was, is): (&Patch, &Patch), names: [&str; 2]) -> (super::change::Changed, super::change::Near) {
        let near = super::change::Near::new(&ops::code_places(lines, was, is), 0, 0);
        let mine = self.current().ok_or_else(|| "no stack is shown".to_string()).and_then(|s| {
            let at = s.patches.iter().position(|id| *id == p.id).unwrap_or_default();
            self.on_stack(s, at, names[1])
        });
        (self.changed(self.phab_on(link, names[0]), mine, names), near)
    }

    /// The latest diff of the revision at `link`, named `name`, on the
    /// commit it is on as Phabricator names it (`State::base`), in the
    /// repository of the stack shown: as git names it, else as git-cinnabar
    /// has the Mercurial changeset (`App::commit_named`). Why not, where
    /// Phabricator said none, or the repository has not that commit.
    fn phab_on(&self, link: &str, name: &str) -> std::result::Result<super::change::On, String> {
        let s = self.current().ok_or_else(|| "no stack is shown".to_string())?;
        let repo = s.repo.clone().ok_or_else(|| format!("the stack has no repository to read the files of {name} in"))?;
        let state = self.state(link).filter(|s| !s.missing && s.diff != 0).ok_or_else(|| format!("{name} has no diff stored"))?;
        let raw = self.raw(link).ok_or_else(|| format!("{name} has no diff stored"))?;
        if state.base.is_empty() {
            return Err(format!("the commit {name} is on is not known: Phabricator said none, or it was fetched before stacked read that"));
        }
        let base = self.commit_named(&repo, &state.base, name)?;
        Ok(super::change::On { repo, base, below: vec![], id: format!("{link}#{}", state.diff), raw: Some(raw.to_string()) })
    }

    /// Why the screen of what differs from Phabricator cannot draw the
    /// change from the revision's latest diff to the patch as it is: the
    /// bases of the two not read (`phab_on`, `on_stack`). None where it can.
    pub(super) fn differs_why(&self) -> Option<String> {
        let p = self.shown_patch()?;
        let (d, link) = crate::phab::revision_of(&p.message)?;
        let s = self.current()?;
        let at = s.patches.iter().position(|id| *id == p.id)?;
        let mine = format!("patch {} as it is", self.sel + 1);
        self.phab_on(&link, &format!("{d} on Phabricator")).and_then(|_| self.on_stack(s, at, &mine)).err()
    }

    /// What differs between the patch the middle shows and its revision on
    /// Phabricator, as its view shows it (`view_of`), its lines `w` wide in
    /// view `v`: its title, its lines, and the lines of what the view shows
    /// each shows. Or, nothing of it known, or Phabricator not having the
    /// revision, why, for the status line.
    pub(super) fn differs_view(&self, w: usize, v: InterLayout) -> std::result::Result<InterDrawn, String> {
        let n = self.sel + 1;
        let Some(p) = self.shown_patch() else { return Err("No patch is shown, so nothing of it is compared with Phabricator.".into()) };
        let Some((d, link)) = crate::phab::revision_of(&p.message) else {
            return Err(format!("Patch {n} names no revision, so nothing of it is compared with Phabricator."));
        };
        match self.synced(p, &link) {
            Synced::NotFound => Err(format!("Phabricator has no revision {d}, so nothing of it is compared with patch {n}.")),
            Synced::Known(x) => Ok(self.view_of(p, &d, &link, &x, w, v)),
            Synced::Unknown => Err(format!("What differs between patch {n} and {d} on Phabricator is not known yet: the line says it once {d} is fetched and compared.")),
        }
    }

    /// What differs between the patch the middle shows and its revision on
    /// Phabricator, on a screen of its own, Esc going back where the cursor
    /// was (`differs_view`): from the Phabricator line's menu and the
    /// patch's, and Space on the line. Not known, the status line says why.
    pub(super) fn open_differs(&mut self) {
        if let Err(why) = self.open_inter(Inter::Differs) {
            self.status = why;
        }
    }

    /// Space on the Phabricator line: what differs, on a screen of its own
    /// (`open_differs`). With nothing that differs, the status line says
    /// that Phabricator has this exact version, the parent named, and
    /// nothing opens; with what differs not known yet, it says so.
    pub(super) fn line_space(&mut self) {
        let Some(p) = self.shown_patch() else { return };
        let Some((d, link)) = crate::phab::revision_of(&p.message) else { return };
        let n = self.sel + 1;
        let same = match self.synced(p, &link) {
            Synced::Known(x) if x.none() => Some(x.same_said(&d, "this version")),
            _ => None,
        };
        match same {
            Some(same) => self.status = format!("Phabricator has this exact version of patch {n}: {same}"),
            None => self.open_differs(),
        }
    }

    /// The items about the revision of the patch the middle shows: what
    /// differs from it on Phabricator, on a screen of its own, grayed while
    /// that is not known; and a pull of it, with its parents, and, as the
    /// item's choice picks, the revisions stacked on it (`pull_choices`).
    /// Why the first is grayed, if it is, for the menu to say. No item,
    /// while reviews come from nowhere, or the patch names no revision.
    fn revision_items(&self, items: &mut Vec<Item>) -> Option<String> {
        if self.cfg.reviews.source == crate::config::Source::None {
            return None;
        }
        let p = self.shown_patch()?;
        let (d, link) = crate::phab::revision_of(&p.message)?;
        let why = match self.synced(p, &link) {
            Synced::Known(_) => None,
            Synced::NotFound => Some(format!("Phabricator has no revision {d}.")),
            Synced::Unknown => Some(format!("{d} is not read from Phabricator and compared with this patch yet.")),
        };
        items.push(Item::new(format!("What differs from {d} on Phabricator…"), Action::Differs).when(why.is_none()));
        let change = p.change.clone();
        items.push(Item::new(format!("Pull {d} from Phabricator"), Action::PullPatch(change.clone(), false)).with(self.pull_choices(&change)));
        why.map(|why| format!("What differs from {d} on Phabricator is grayed: {why}"))
    }

    /// What the pull of the revision of the patch of `change` pulls with
    /// it, as radio buttons: the revisions it stacks on that the stack does
    /// not hold; or those, and the revisions stacked on it. Kept for the
    /// stack, as the menus' choices are.
    fn pull_choices(&self, change: &str) -> Choices {
        let list = vec![
            Choice::new(
                "parents",
                "with its parents",
                "with the revisions it stacks on that the stack does not hold, down to one it holds or the first that is not open",
                Some(Action::PullPatch(change.to_string(), false)),
            ),
            Choice::new(
                "children",
                "and the revisions on it",
                "with the revisions it stacks on that the stack does not hold, and the revisions stacked on it, while one is open",
                Some(Action::PullPatch(change.to_string(), true)),
            ),
        ];
        Choices::new("pull", "with", list, self.db.choice(&self.stack, "pull"))
    }

    /// Every item of the patch the middle shows about Phabricator, in the
    /// patch's `Phabricator` menu (`phab_menu`) and in the menu of its
    /// Phabricator line alike: its submit (`submit_item`), what differs from
    /// its revision and the pull of it (`revision_items`),
    /// and Fetch the latest from Phabricator. Why those grayed are, for the
    /// menu to say: HEAD holding the submit back, or a reason of its own,
    /// then what differs. None while reviews come from nowhere.
    pub(super) fn phab_items(&self, items: &mut Vec<Item>) -> Vec<String> {
        if self.cfg.reviews.source == crate::config::Source::None {
            return Vec::new();
        }
        let held = self.submit_held(self.sel);
        let (item, submit_why) = self.submit_item(self.read_only().is_none() && held.is_none());
        items.push(item);
        let mut why: Vec<String> = held.into_iter().chain(submit_why).collect();
        why.extend(self.revision_items(items));
        items.push(Item::new(FETCH, Action::Fetch));
        why
    }

    /// The patch's own `Phabricator` menu, from its menu in the patches
    /// pane: every item of the patch picked about Phabricator
    /// (`phab_items`), titled for its revision, `D123 on Phabricator`, or
    /// `Phabricator` for a patch that names none yet; why each grayed item
    /// is, under them, the stack being read-only first.
    pub(super) fn phab_menu(&self) -> Menu {
        let mut items = Vec::new();
        let mut why: Vec<String> = self.read_only().map(String::from).into_iter().collect();
        why.extend(self.phab_items(&mut items));
        let title = self.line_title().unwrap_or_else(|| "Phabricator".into());
        Menu::new(title, items, (!why.is_empty()).then(|| why.join("\n")))
    }

    /// The item of the patch's `Phabricator` menu, and of the menu of its
    /// Phabricator line, that sends the patch the middle shows to
    /// Phabricator, by the status of its revision, as stored, and by
    /// what its Phabricator line knows (`synced`); offered when `can` (the
    /// stack can be changed, and HEAD does not hold the patch back). With
    /// it, why it is grayed, when that is for a reason of its own, for the
    /// menu to say. `Submit to Phabricator as WIP` for a patch that names
    /// no revision, for a revision in work (Changes Planned, a draft), and
    /// while the status is not stored, is one stacked does not know, or for
    /// a revision Phabricator does not have: what the submit reads decides
    /// then. For a revision open but not in work, in its place, the update
    /// of its title, summary and parent alone, named for what differs:
    /// grayed when nothing but the diff differs, or nothing, as no diff is
    /// sent to it, and while that is not read and compared yet. For a
    /// revision that landed or was abandoned, `Submit to Phabricator as
    /// WIP` grayed, as nothing is sent to it.
    pub(super) fn submit_item(&self, can: bool) -> (Item, Option<String>) {
        let wip = |why: Option<String>| (Item::new("Submit to Phabricator as WIP", Action::Submit).when(can && why.is_none()), why);
        let Some(p) = self.shown_patch() else { return wip(None) };
        let Some((d, link)) = crate::phab::revision_of(&p.message) else { return wip(None) };
        let Some(status) = self.revision_status(&p.id) else { return wip(None) };
        match status {
            Status::Published => return wip(Some(format!("{d} has landed, so stacked sends it nothing."))),
            Status::Abandoned => return wip(Some(format!("{d} was abandoned, so stacked sends it nothing."))),
            Status::ChangesPlanned | Status::Draft | Status::Unknown => return wip(None),
            Status::NeedsReview | Status::NeedsRevision | Status::Accepted => {}
        }
        let n = self.sel + 1;
        let label = |what: &str| format!("Update {d} on Phabricator: its {what}");
        let all = label("title, summary and parent");
        let is = status.name();
        let (label, why) = match self.synced(p, &link) {
            Synced::Known(x) => match (x.message.is_some(), x.parent_differs()) {
                (true, true) => (all, None),
                (true, false) => (label("title and summary"), None),
                (false, true) => (label("parent"), None),
                (false, false) if x.code.is_some() => (
                    all,
                    Some(format!(
                        "{d} is {is}, so stacked sends it no new diff, as only a revision in work gets one, and its title, summary and parent are those of patch {n} already."
                    )),
                ),
                (false, false) => (all, Some(format!("{d} is in sync with patch {n}, so there is nothing to send."))),
            },
            Synced::Unknown | Synced::NotFound => (
                all,
                Some(format!(
                    "{d} is {is}, so only its title, summary and parent would be sent, and they are not read from Phabricator and compared with patch {n} yet."
                )),
            ),
        };
        (Item::new(label, Action::SubmitMeta).when(can && why.is_none()), why)
    }

    /// The title of the Phabricator line's menu, and of the patch's
    /// `Phabricator` menu: its revision, on Phabricator.
    pub(super) fn line_title(&self) -> Option<String> {
        let (d, _) = crate::phab::revision_of(&self.shown_patch()?.message)?;
        Some(format!("{d} on Phabricator"))
    }

    // -- reading Phabricator, when asked --------------------------------- //

    /// Fetch the latest from Phabricator, from a menu: every revision the
    /// stack's patches name, read away from the screen and stored
    /// (`phab::fetch_latest`), the bottom line saying so meanwhile, then
    /// what came of it. One fetch at a time; nothing to fetch, said.
    pub fn fetch_from_phabricator(&mut self) {
        if self.bg.doing.contains(&FETCHING) {
            self.status = "already fetching the latest from Phabricator".into();
            return;
        }
        if self.revisions_named().is_empty() {
            self.status = "No patch of the stack names a revision, so there is nothing to fetch from Phabricator.".into();
            return;
        }
        self.status = "fetching the latest from Phabricator…".into();
        self.fetch(None);
    }

    /// The revisions of the stack read from Phabricator and stored, away
    /// from the screen (`phab::fetch_latest`): every one its patches name,
    /// or revision `only` alone, `D123` and its link, which a submit just
    /// sent to.
    fn fetch(&mut self, only: Option<(String, String)>) {
        self.bg.doing.push(FETCHING);
        let (path, name) = (self.path.clone(), self.stack.clone());
        self.spawn(move || {
            let links: Option<Vec<String>> = only.as_ref().map(|(_, link)| vec![link.clone()]);
            Done::Fetched(crate::phab::fetch_latest(&path, &name, links.as_deref()), only.map(|(d, _)| d))
        });
    }

    /// What a fetch came to, `after` the revision it read again after a
    /// submit, if it was that: the database read again, and what the lines
    /// need next. The bottom line says what was fetched; or, nothing stored,
    /// why, Phabricator turning this machine away said plainly. After a
    /// submit, it keeps what the submit came to, and adds why a fetch that
    /// failed failed.
    pub(super) fn fetched(&mut self, said: std::result::Result<String, crate::phab::Error>, after: Option<String>) {
        if let Some(k) = self.bg.doing.iter().position(|d| *d == FETCHING) {
            self.bg.doing.remove(k);
        }
        self.reload();
        match (said, after) {
            (Ok(said), None) => self.status = said,
            (Ok(_), Some(_)) => {}
            (Err(e), None) => self.status = format!("The stack's revisions could not be fetched from Phabricator, so what is stored of them stays as it was: {e}"),
            (Err(e), Some(d)) => {
                self.status = format!("{} {d} could not be fetched again from Phabricator after the submit, so its line shows it as it was read before: {e}", self.status)
            }
        }
        self.check_line();
    }

    /// The submit under way sends the patch of change `change`: its
    /// revision fetched again once the submit is done (`fetch_sent`).
    pub(super) fn sends(&mut self, change: &str) {
        self.comparisons.sent = Some(change.to_string());
    }

    /// Right after a submit: the database read again, for the link of a new
    /// revision; then the revision of the patch it sent, as the patch names
    /// it now, fetched again, alone, so that its line shows what Phabricator
    /// has once the submit is done, not what was read before it. Not while
    /// the editor does not read Phabricator.
    pub(super) fn fetch_sent(&mut self) {
        let sent = self.comparisons.sent.take();
        self.reload();
        if !self.reads_phabricator() {
            return;
        }
        let Some(change) = sent else { return };
        let p = self.current().and_then(|s| s.patches.iter().map(|id| self.db.patch(id)).find(|p| p.change == change));
        if let Some(named) = p.and_then(|p| crate::phab::revision_of(&p.message)) {
            self.fetch(Some(named));
        }
    }

    /// A pull from Phabricator, as `asked` asks: of every revision the
    /// stack's patches name, or of the patch's whose change it names, with
    /// their parents and, as asked, those stacked on them. Its run written at
    /// once, queued as an edit is, so that the stack is not edited by hand
    /// from now on (`review::ask`); then its read of Phabricator, away from
    /// the screen (`review::read`), the banner saying where it is as it
    /// reads, what it read stored, and what it brings waiting for your
    /// review once it is made. One pull at a time, and not while one of the
    /// stack is under way.
    pub(super) fn pull_from_phabricator(&mut self, asked: review::Asked) {
        if self.bg.doing.contains(&"pulling") {
            self.status = PULLING_ALREADY.into();
            return;
        }
        let name = self.stack.clone();
        // Of one patch: its revision, said from the start.
        let alone = match &asked {
            review::Asked::Patch { change, .. } => self.current().and_then(|s| {
                let p = s.patches.iter().map(|id| self.db.patch(id)).find(|p| p.change == *change)?;
                crate::phab::revision_of(&p.message).map(|(d, _)| d)
            }),
            _ => None,
        };
        let reading = match review::ask(&self.path, &name, asked) {
            Ok(r) => r,
            Err(e) => {
                self.status = e.0;
                return;
            }
        };
        self.reload();
        self.bg.doing.push("pulling");
        self.status = match &alone {
            Some(d) => format!("pulling {d} from Phabricator…"),
            None => "pulling from Phabricator…".into(),
        };
        self.bg.pull = Some((reading.run.clone(), alone.map_or(review::Pulling::Starting, review::Pulling::Alone)));
        let (path, tx) = (self.path.clone(), self.bg.tx.clone());
        self.spawn(move || {
            // Where it is, as it goes: the banner says.
            let mut say = |p| {
                let _ = tx.send(Done::Pulling(p));
            };
            let said = match review::read(&path, reading, &mut say) {
                Ok(r) => Db::load(&path).map_or_else(|_| r.summary.clone(), |db| review::said(&db, &r)),
                Err(e) => e.0,
            };
            Done::Said("pulling", said)
        });
    }

    /// A new stack pulled from Phabricator, from the revision `text` names:
    /// made at once, empty, on HEAD, with its pull's run, and shown
    /// (`review::ask_new`); then the pull's read of Phabricator, away from the
    /// screen (`review::read`), its banner saying where it is, the stack moved
    /// onto the commit the bottom revision is on, and what the pull brings
    /// landing into it for review; then its review open. One pull at a time.
    pub(super) fn pull_new_stack(&mut self, text: &str) {
        let Some((rev, _)) = crate::phab::revision_named(text) else { return };
        if self.bg.doing.contains(&"pulling") {
            self.status = PULLING_ALREADY.into();
            return;
        }
        let reading = match review::ask_new(&self.path, &self.checkout, text, None) {
            Ok(r) => r,
            Err(e) => {
                self.status = e.0;
                return;
            }
        };
        self.reload();
        let name = reading.stack.clone();
        self.switch_stack(&name);
        self.bg.doing.push("pulling");
        self.status = format!("pulling {rev} from Phabricator, with the revisions it stacks on and those stacked on it…");
        self.bg.pull = Some((reading.run.clone(), review::Pulling::Alone(rev)));
        let (path, tx) = (self.path.clone(), self.bg.tx.clone());
        self.spawn(move || {
            let mut say = |p| {
                let _ = tx.send(Done::Pulling(p));
            };
            let pulled = review::read(&path, reading, &mut say).and_then(|r| {
                let said = review::said(&Db::load(&path)?, &r);
                Ok((r.id.clone(), said))
            });
            Done::PulledStack(name, pulled.map_err(|e| e.0))
        });
    }

    /// What pulling new stack `name` came to (`pull_new_stack`): the stack
    /// shown, the pull's review open once it waits for your review; else why
    /// nothing was pulled, the stack staying, empty.
    pub(super) fn pulled_stack(&mut self, name: String, pulled: std::result::Result<(String, String), String>) {
        self.bg.doing.retain(|d| *d != "pulling");
        self.bg.pull = None;
        self.reload();
        match pulled {
            Ok((run, said)) => {
                self.switch_stack(&name);
                if self.db.run(&run).is_some_and(|r| crate::held::waiting(&self.db, r)) {
                    self.open_review(&run, None);
                }
                self.status = said;
            }
            Err(e) => self.status = e,
        }
    }

    /// The item that drops the stack's pull from Phabricator waiting its
    /// turn, asking first, if one waits.
    pub(super) fn pull_drop_item(&self) -> Option<Item> {
        let r = review::pending(&self.db, &self.stack)?;
        let r = self.pull_waiting(&r.id)?;
        Some(Item::new("Drop the waiting pull from Phabricator…", Action::AskDropPull(r.id.clone())))
    }

    /// What pull `run`, under way, does, said: reading Phabricator, waiting
    /// its turn and what for, or being made. None once it is not under way.
    pub(super) fn pull_said(&self, run: &str) -> Option<String> {
        let r = self.db.run(run)?;
        review::under_way_said(&self.db, r).map(|said| format!("The pull from Phabricator {said}"))
    }

    /// The stack's pull under way, if one is, and where it is: read again
    /// only once the database changes, or another stack is shown, as the
    /// banner and the bar ask on every redraw. A pull whose read is cut short
    /// meanwhile is told so as its run lands, a few seconds later.
    pub(super) fn pull_under_way(&self) -> Option<(String, review::Phase)> {
        let key = (self.db.changes(), self.stack.clone());
        if let Some((k, p)) = &*self.pull_memo.borrow()
            && *k == key
        {
            return p.clone();
        }
        let p = review::pending(&self.db, &self.stack).and_then(|r| Some((r.id.clone(), review::phase(&self.db, r)?)));
        *self.pull_memo.borrow_mut() = Some((key, p.clone()));
        p
    }

    /// Where pull `run` is, if it is under way: the stack's, as last read
    /// (`pull_under_way`); another's, read now.
    pub(super) fn phase_of(&self, run: &str) -> Option<review::Phase> {
        match self.pull_under_way() {
            Some((id, p)) if id == run => Some(p),
            _ => self.db.run(run).and_then(|r| review::phase(&self.db, r)),
        }
    }

    /// Pull `run`, if it waits its turn: read, and not being made.
    fn pull_waiting(&self, run: &str) -> Option<&crate::db::Run> {
        self.db.run(run).filter(|_| self.phase_of(run) == Some(review::Phase::Waiting))
    }

    /// What Delete does on pull `run`, under way, as on its line of the
    /// banner and in the runs: waiting its turn, its drop, asked first;
    /// reading Phabricator or being made, nothing, and why. None once it is
    /// not under way.
    pub(super) fn pull_deleting(&self, run: &str) -> Option<std::result::Result<Action, String>> {
        Some(match self.phase_of(run)? {
            review::Phase::Waiting => Ok(Action::AskDropPull(run.into())),
            review::Phase::Reading => Err(review::READING_NOT_DROPPED.into()),
            review::Phase::Making => Err("the pull from Phabricator is being made, so it can no longer be dropped: what it brings waits for your review once it is made".into()),
        })
    }

    /// The menu of pull `run` waiting its turn, which Enter on its line of
    /// the banner opens: what it waits for, and its drop. None once it no
    /// longer waits: reading Phabricator still, or being made.
    pub(super) fn pull_menu(&self, run: &str) -> Option<Menu> {
        let r = self.pull_waiting(run)?;
        let items = vec![Item::new("Drop the waiting pull from Phabricator…", Action::AskDropPull(run.into()))];
        let mut m = Menu::new("pull from Phabricator, waiting its turn", items, None);
        m.note = self.pull_said(&r.id).map(|said| format!("{said}."));
        Some(m)
    }

    /// What dropping pull `run`, waiting its turn, asks first: that nothing
    /// was changed, and what is then of the edits asked after it. None once
    /// it no longer waits.
    pub(super) fn drop_pull_ask(&self, run: &str) -> Option<Menu> {
        let r = self.pull_waiting(run)?;
        let mut said = vec!["It is taken off the queue: what it read of Phabricator stays stored, and nothing changes in the stack.".to_string()];
        said.extend(super::banner::edits_after_said(&self.db, &r.stack, run.trim_start_matches("pull-")));
        let yes = Item::new("Drop it", Action::DropPull(run.into()));
        let mut m = Menu::ask("drop the pull from Phabricator that waits its turn? Nothing was held yet", yes, "Keep it", true);
        m.note = Some(said.join("\n"));
        Some(m)
    }

    /// Pull `run`, waiting its turn, dropped: the status line says so, and
    /// what waited behind it goes.
    pub(super) fn drop_pull(&mut self, run: &str) {
        match review::drop_waiting(&self.path, run) {
            Ok((_, dropped)) => {
                self.reload();
                self.status = match dropped {
                    true => review::DROPPED.into(),
                    false => "the pull from Phabricator no longer waits its turn: it was made meanwhile".into(),
                };
                self.launch();
            }
            Err(e) => self.status = e.0,
        }
    }

    /// What `?` says on the Phabricator line: what the line says, where it
    /// is read from and when, what Space does there, and what the menu has.
    pub(super) fn line_help(&self) -> Vec<String> {
        let Some(p) = self.shown_patch() else { return Vec::new() };
        let Some((d, link)) = crate::phab::revision_of(&p.message) else { return Vec::new() };
        let n = self.sel + 1;
        let mut out = vec![format!(
            "This line says what Phabricator has of patch {n}: its revision, {d}; the status of {d}, by its name; whether {d} is in sync with this version; how many of the comments of {d} are not pulled yet; and how long ago that was fetched. In sync, the latest diff of {d} does what this version does; the title and summary of {d} are exactly those a submit sends, where their lines wrap too; and a submit would not change the parent of {d}, which a submit sets to the revision of the patch below, when that is open. What this line says is what stacked stored the last time it read Phabricator: stacked reads nothing of it by itself. Fetch the latest from Phabricator, in this line's menu, reads every revision of the stack again; a submit reads its revision, and a pull the revisions it pulls. Where something is not fetched yet, the line says … in its place. The patches pane says the same after the status of each patch's revision: ✓ in sync, ✗ when something differs."
        )];
        out.push(match self.synced(p, &link) {
            Synced::NotFound => format!("Phabricator has no revision {d}, so nothing of it is compared with this patch, and Space only says so."),
            Synced::Known(x) if x.none() => "Phabricator has this exact version: Space says so on the bottom line, and opens nothing.".into(),
            // What Space shows, of what differs, the parent first.
            Synced::Known(x) => {
                let mut shown: Vec<String> = Vec::new();
                if x.parent_differs() {
                    shown.push(format!("the parent of {d} on Phabricator, then the one a submit sets"));
                }
                if x.message.is_some() {
                    shown.push(format!("the title and summary of {d}, then those a submit of this version sends, compared where their lines wrap aside"));
                }
                if x.code.is_some() {
                    shown.push(match self.shown_code(p, &link, &x) {
                        Shown::Files(_) => format!("each file as the latest diff of {d} leaves it, then as this version leaves it, {}, as that diff is on another base", below(n)),
                        _ => format!("the latest diff of {d}, then this version's diff, as an interdiff"),
                    });
                }
                let shown = match shown.split_last() {
                    Some((last, rest)) if !rest.is_empty() => format!("{}; and {last}", rest.join("; ")),
                    _ => shown.concat(),
                };
                format!("Press Space to see what differs from {d} on Phabricator, on a screen of its own: {shown}. Esc there comes back to this line.")
            }
            Synced::Unknown => format!(
                "Press Space to see what differs from {d} on Phabricator, on a screen of its own, once {d} is fetched and compared: until then, Space says that it is not known yet."
            ),
        });
        out.push(format!(
            "Press Enter for all that can be done here: the submit of this patch, what differs from {d} on Phabricator, the pull of {d} with its parents, or with the revisions stacked on it too, as its choice picks, and Fetch the latest from Phabricator."
        ));
        out
    }

    /// What the bottom bar says on the Phabricator line.
    pub(super) fn line_hint(&self) -> String {
        let space = "space: what differs from Phabricator";
        let keys = match (self.reading, self.whole_files) {
            (false, false) => "1: view · 2: read · 3: whole",
            (false, true) => "1: view · 2: read · 3: hunks",
            (true, false) => "1: view · 2: comment · 3: whole",
            (true, true) => "1: view · 2: comment · 3: hunks",
        };
        format!("{space} · enter: menu · {keys} · ?: what can I do here")
    }
}

#[cfg(test)]
mod tests {
    use super::super::compared::message_rows;
    use super::*;

    #[test]
    fn a_title_is_in_work_as_moz_phab_reads_it() {
        use crate::submit::title_in_work as in_work;
        for t in ["WIP: Bug 1 - one", "wip: Bug 1", "WIP Bug 1", "WIP", "Wip:x"] {
            assert!(in_work(t), "{t:?}");
        }
        for t in ["Bug 1 - WIP: one", "WIPE out", "WI", ""] {
            assert!(!in_work(t), "{t:?}");
        }
    }

    #[test]
    fn the_title_and_summary_are_compared_as_a_submit_sends_them_in_work_or_not_as_the_revision_s_title_is() {
        let p = |message: &str| Patch::new("c".into(), None, None, message.into(), String::new());
        // A revision's title and summary, as stored.
        let searched = |title: &str, summary: &str| (title.to_string(), summary.to_string());
        let message_against = |s: &(String, String), p: &Patch, trailer: &str| message_against(&s.0, &s.1, p, trailer);
        let trailer = "Differential Revision";
        let mine = p("Bug 1 - one r?alice\n\nThe parser reads the whole file\nbefore it checks the header.\n\nDifferential Revision: https://p.example/D1");
        // In work, or not: the title as moz-phab writes it, either way; the
        // summary line for line, the trailer left out, the white space at
        // its end aside.
        let summary = "The parser reads the whole file\nbefore it checks the header.";
        assert_eq!(message_against(&searched("WIP: Bug 1 - one r?alice", summary), &mine, trailer), None);
        assert_eq!(message_against(&searched("Bug 1 - one r?alice", summary), &mine, trailer), None);
        assert_eq!(message_against(&searched("Bug 1 - one r?alice", &format!("{summary}\n\n")), &mine, trailer), None);
        // The same words, their lines wrapped otherwise, as a message wrapped
        // again since it was submitted is: they differ, Phabricator's then
        // this version's, as a submit would send them again.
        let (before, after) = message_against(&searched("Bug 1 - one r?alice", "The parser reads the whole file before it checks the header."), &mine, trailer).unwrap();
        assert_eq!(before, "Bug 1 - one r?alice\n\nThe parser reads the whole file before it checks the header.");
        assert_eq!(after, "Bug 1 - one r?alice\n\nThe parser reads the whole file\nbefore it checks the header.");
        // Other white space too: within a line, ending one, a carriage
        // return ending one.
        for other in [
            "The parser reads the whole  file\nbefore it checks the header.",
            "The parser reads the whole file \nbefore it checks the header.",
            "The parser reads the whole file\r\nbefore it checks the header.",
        ] {
            assert!(message_against(&searched("Bug 1 - one r?alice", other), &mine, trailer).is_some(), "{other:?}");
        }
        assert!(message_against(&searched("Bug 1 -  one r?alice", summary), &mine, trailer).is_some(), "the title's");
        // A word of the summary otherwise: Phabricator's, then this version's.
        let (before, after) = message_against(&searched("Bug 1 - one r?alice", "The parser reads the entire file before it checks the header."), &mine, trailer).unwrap();
        assert_eq!(before, "Bug 1 - one r?alice\n\nThe parser reads the entire file before it checks the header.");
        assert_eq!(after, "Bug 1 - one r?alice\n\nThe parser reads the whole file\nbefore it checks the header.");
        // The title otherwise, in work: compared in work.
        let (before, after) = message_against(&searched("WIP: Bug 1 - two", summary), &mine, trailer).unwrap();
        assert_eq!((before.lines().next(), after.lines().next()), (Some("WIP: Bug 1 - two"), Some("WIP: Bug 1 - one r?alice")));
    }

    #[test]
    fn a_title_and_summary_given_back_as_a_submit_sent_them_are_in_sync() {
        let trailer = "Differential Revision";
        let link = "Differential Revision: https://p.example/D1";
        for message in [
            format!("Bug 1 - one\n\n{link}"),
            "Bug 1 - one, its title alone".to_string(),
            // Reviewers moz-phab writes otherwise; white space ending a line,
            // a command, a fenced block and its empty line.
            format!("bug 1 - one r=alice,bob! r?#build\n\nThe parser reads the whole file\nbefore it checks the header.  \n\n    $ ./mach build --foo\n\n```\nfn a() {{}}\n\nfn b() {{}}\n```\n\n{link}"),
            // The trailer between two paragraphs, empty lines after it all.
            format!("Bug 1 - one\n\nA paragraph.\n\n{link}\n\nAnother, after the trailer.\n\n\n"),
            // Lines that end in a carriage return.
            format!("WIP: Bug 1 - one\r\n\r\nLines that end\r\nin a carriage return.\r\n\r\n{link}\r\n"),
        ] {
            let p = Patch::new("c".into(), None, None, message.clone(), String::new());
            // What a submit sends (`submit::prepare_with`), as Phabricator
            // keeps it and gives it back.
            let sent = (crate::submit::wip_title(p.title()).title, crate::submit::summary_of(&p.message, trailer));
            assert_eq!(message_against(&sent.0, &sent.1, &p, trailer), None, "{message:?}");
        }
    }

    #[test]
    fn a_title_and_summary_only_wrapped_otherwise_show_the_lines_of_both_versions_said_so() {
        let rows = |before: &str, after: &str| {
            let mut rows = Vec::new();
            message_rows(&mut rows, before, after, 80);
            rows
        };
        let texts = |rows: &[Row]| rows.iter().map(|r| r.text.clone()).collect::<Vec<_>>();
        let said = |rows: &[Row]| rows.iter().take_while(|r| r.kind == Kind::Meta).map(|r| r.text.trim().to_string()).collect::<Vec<_>>();
        let before = "WIP: Bug 1 - one\n\nThe parser reads the whole file before it checks the header.\n\nBecause.";
        let after = "WIP: Bug 1 - one\n\nThe parser reads the whole file\nbefore it checks the header.\n\nBecause.";
        // Wrapped otherwise, and nothing else: said first; the title and
        // `Because.` once; the paragraph as each version writes it,
        // Phabricator's lines then this version's, nothing of them marked, as
        // every word of them is in both.
        let only = rows(before, after);
        let over = said(&only);
        assert_eq!(over.join(" "), "Both versions have the same words: they differ only in where their lines wrap, or in their white space.");
        assert!(over.len() > 1, "folded to the width: {over:?}");
        let lines = &only[over.len()..];
        assert_eq!(
            texts(lines),
            [" WIP: Bug 1 - one", " ", "-The parser reads the whole file before it checks the header.", "+The parser reads the whole file", "+before it checks the header.", " ", " Because."]
        );
        assert!(lines.iter().all(|r| r.words.is_empty() && !r.broken), "{lines:#?}");
        assert!(lines.iter().filter(|r| r.kind != Kind::Ctx).all(|r| r.compared && matches!(r.kind, Kind::Add | Kind::Del)), "{lines:#?}");
        // White space otherwise alone, its lines wrapped where they were: the
        // same, both versions' lines.
        let spaced = rows("Bug 1\n\nThe parser  reads it.\r\nThen it checks.", "Bug 1\n\nThe parser reads it.\nThen it checks.");
        let lines = &spaced[said(&spaced).len()..];
        assert!(!said(&spaced).is_empty());
        assert_eq!(texts(lines), [" Bug 1", " ", "-The parser  reads it.", "-Then it checks.", "+The parser reads it.", "+Then it checks."]);
        // A word otherwise too: nothing said, the paragraph wrapped otherwise
        // once, as this version writes it, the word's own lines Phabricator's
        // then this version's.
        let mixed = rows(before, &after.replace("Because.", "Since."));
        assert_eq!(
            texts(&mixed),
            [" WIP: Bug 1 - one", " ", " The parser reads the whole file", " before it checks the header.", " ", "-Because.", "+Since."]
        );
    }

    #[test]
    fn a_diff_is_bare_of_what_git_derives_in_it_as_phabricator_s_raw_diffs_are() {
        // As git writes it: blobs, a similarity, a hunk's heading, a header's
        // line ending in a tab. As Phabricator gives it back: none of those.
        let git = "diff --git a/a.txt b/a.txt\nindex 1234567..89abcde 100644\n--- a/a.txt\n+++ b/a.txt\n@@ -1,3 +1,3 @@ fn main() {\n one\n-two\n+TWO\n three\n\
                   diff --git a/old.txt b/new.txt\nsimilarity index 90%\nrename from old.txt\nrename to new.txt\n--- a/old.txt\n+++ b/new.txt\t\n@@ -1 +1 @@\n-x\n+y\n";
        let phab = "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1,3 +1,3 @@\n one\n-two\n+TWO\n three\n\
                    diff --git a/old.txt b/new.txt\nrename from old.txt\nrename to new.txt\n--- a/old.txt\n+++ b/new.txt\n@@ -1 +1 @@\n-x\n+y\n";
        assert_eq!(bare(git), phab);
        assert_eq!(bare(phab), phab);
        // The lines of a hunk as they are, those that read as a header too,
        // and the white space ending them; git's line under a last line
        // with no newline.
        let hunk = "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1,2 +1,2 @@\n-index 1..2\n+index 3..4  \n similarity index 1%\n\\ No newline at end of file";
        assert_eq!(bare(hunk), hunk);
    }

    #[cfg(feature = "demo")]
    #[test]
    fn the_tour_pulls_nothing_from_phabricator_a_patch_s_revision_alone_neither() {
        assert!(reaches_phabricator(&Action::Pull) && reaches_phabricator(&Action::PullPatch("c".into(), true)));
        // Nor does it fetch the latest.
        assert!(reaches_phabricator(&Action::Fetch));
        // What differs shows what is stored, and reads nothing; the menu of
        // a patch's items opens all the same.
        assert!(!reaches_phabricator(&Action::Differs) && !reaches_phabricator(&Action::PhabMenu));
    }

    #[test]
    fn how_long_ago_the_line_was_fetched_is_said_in_minutes_hours_and_days() {
        let now = 1_762_150_000;
        let said = |secs: i64| age_said(now - secs, now);
        assert_eq!([said(0), said(59), said(-30)], ["fetched just now"; 3], "a time to come is just now");
        assert_eq!([said(60), said(119), said(180), said(3599)], ["fetched 1 minute ago", "fetched 1 minute ago", "fetched 3 minutes ago", "fetched 59 minutes ago"]);
        assert_eq!([said(3600), said(7200), said(86_399)], ["fetched 1 hour ago", "fetched 2 hours ago", "fetched 23 hours ago"]);
        assert_eq!([said(86_400), said(2 * 86_400 - 1)], ["fetched yesterday", "fetched yesterday"]);
        assert_eq!([said(2 * 86_400), said(40 * 86_400)], ["fetched 2 days ago", "fetched 40 days ago"]);
    }

    #[test]
    fn a_title_and_summary_only_wrapped_otherwise_are_told_from_those_that_differ() {
        let wrapped = "Bug 1 - one\n\nThe parser reads the whole file\nbefore it checks the header.";
        assert!(only_wrapped("Bug 1 - one\n\nThe parser reads the whole file before it checks the header.", wrapped));
        assert!(only_wrapped("Bug 1 -  one\n\nThe parser reads the whole file\nbefore it checks the header.", wrapped));
        assert!(!only_wrapped(wrapped, wrapped));
        assert!(!only_wrapped("Bug 1 - one\n\nThe parser reads the entire file before it checks the header.", wrapped));
    }

    #[test]
    fn the_comments_not_pulled_are_said_new_then_edited() {
        assert_eq!(comments_said((0, 0, 0)), "no new comments");
        assert_eq!(comments_said((1, 0, 0)), "1 new comment to pull");
        assert_eq!(comments_said((2, 0, 0)), "2 new comments to pull");
        assert_eq!(comments_said((0, 1, 0)), "1 edited comment to pull");
        assert_eq!(comments_said((3, 2, 0)), "3 new comments and 2 edited comments to pull");
        // Comments filed read with their suggested edits, apart from edits.
        assert_eq!(comments_said((0, 0, 2)), "2 suggested edits to pull");
        assert_eq!(comments_said((1, 1, 1)), "1 new comment, 1 edited comment and 1 suggested edit to pull");
    }

    #[test]
    fn the_line_folds_between_words_its_pieces_under_its_first() {
        let line = "Phabricator: D123456 · Needs Review · the diff differs · 2 new comments to pull";
        assert_eq!(fold_line(line, 100), [line]);
        let folded = fold_line(line, 40);
        assert_eq!(folded, ["Phabricator: D123456 · Needs Review ·", "   the diff differs · 2 new comments to", "   pull"]);
        assert!(folded.iter().all(|l| l.chars().count() <= 40), "{folded:?}");
        // Too narrow to fold: as it is.
        assert_eq!(fold_line(line, 15), [line]);
    }

    #[test]
    fn what_differs_is_said_in_plain_words() {
        let code = (String::new(), None);
        let message = Some((String::new(), String::new()));
        let set = Parent::Set { id: "D11".into(), phid: "PHID-DREV-11".into(), below: 1 };
        let differs = |c: bool, m: bool, p: bool| Differs {
            code: c.then_some((&code.0, &code.1)),
            message: if m { message.clone() } else { None },
            parent: if p { set.clone() } else { Parent::Already { id: "D11".into(), below: 1 } },
            parents_now: vec![],
        };
        let all = [(false, false, false), (true, false, false), (false, true, false), (false, false, true), (true, true, false), (true, false, true), (false, true, true), (true, true, true)];
        assert_eq!(
            all.map(|(c, m, p)| differs(c, m, p).said()),
            [
                "in sync",
                "the diff differs",
                "the title or summary differs",
                "the parent differs",
                "the diff and the title or summary differ",
                "the diff and the parent differ",
                "the title or summary and the parent differ",
                "the diff, the title or summary, and the parent differ",
            ]
        );
        assert!(differs(false, false, false).none() && !differs(true, false, false).none() && !differs(false, false, true).none());
        // A submit setting none: in sync, whatever parents it has.
        let nothing = Differs { parent: Parent::Nothing("Patch 1 is at the bottom of the stack, so D12 gets no parent.".into()), ..differs(false, false, false) };
        assert!(nothing.none() && !nothing.parent_differs());
        // In sync, the parent named, or why a submit sets none.
        assert_eq!(
            differs(false, false, false).same_said("D12", "this version"),
            "the latest diff of D12 does what this version does, the title and summary of D12 are those of this version, and its parent is D11, the revision of patch 1."
        );
        assert_eq!(
            nothing.same_said("D12", "this version"),
            "the latest diff of D12 does what this version does, and the title and summary of D12 are those of this version. Patch 1 is at the bottom of the stack, so D12 gets no parent."
        );
        // A submit setting one: the parents the revision has, by name, then
        // the one a submit sets.
        let now = |names: &[&str]| Differs { parents_now: names.iter().map(|n| n.to_string()).collect(), ..differs(false, false, true) };
        assert_eq!(now(&[]).parent_said().as_deref(), Some("Parent: none on Phabricator; a submit sets D11, the revision of patch 1."));
        assert_eq!(now(&["D10"]).parent_said().as_deref(), Some("Parent: D10 on Phabricator; a submit sets D11, the revision of patch 1."));
        assert_eq!(now(&["D10", "D9"]).parent_said().as_deref(), Some("Parents: D10 and D9 on Phabricator; a submit sets D11, the revision of patch 1."));
        assert_eq!(differs(true, true, false).parent_said(), None);
    }

    #[test]
    fn the_patches_pane_marks_in_sync_and_what_differs_as_the_line_says_them() {
        let code = (String::new(), None);
        let message = Some((String::new(), String::new()));
        let differs = |c: bool, m: bool, parent: Parent| Differs {
            code: c.then_some((&code.0, &code.1)),
            message: if m { message.clone() } else { None },
            parent,
            parents_now: vec![],
        };
        let set = || Parent::Set { id: "D11".into(), phid: "PHID-DREV-11".into(), below: 1 };
        let already = || Parent::Already { id: "D11".into(), below: 1 };
        let nothing = || Parent::Nothing("Patch 1 is at the bottom of the stack, so D12 gets no parent.".into());
        // ✓ where the line says in sync, ✗ wherever it says what differs,
        // for every one of them.
        for (c, m) in [(false, false), (true, false), (false, true), (true, true)] {
            for parent in [set(), already(), nothing()] {
                let x = differs(c, m, parent);
                let said = x.said();
                assert_eq!(Synced::Known(x).mark(), Some(if said == "in sync" { "✓" } else { "✗" }), "{said}");
            }
        }
        // Neither while it is not known, nor for a revision not found: a blank.
        assert_eq!((Synced::Unknown.mark(), Synced::NotFound.mark()), (None, None));
    }
}
