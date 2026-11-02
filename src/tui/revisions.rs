//! What Phabricator has of the stack's revisions: the status of each
//! patch's revision, which the patches pane shows after the patch's number,
//! read in one search of all the revisions the stack's patches name; and
//! the Phabricator line over the message of the patch the middle shows,
//! which says its revision's status by name, whether Phabricator's latest
//! diff and the revision's title and summary are this version's, its
//! parent the one a submit sets, and how many of the revision's comments
//! are not pulled yet. Space on the line, or its menu, shows what differs
//! on a screen of its own, Phabricator's version then this one as an
//! interdiff compares two versions; the menu pulls the reviews of that
//! revision alone, too. Whether each patch is in sync so, the patches pane
//! marks right after its status (`✓`, `✗`), from the same reading as its
//! line (`Synced`); and the patch's menu offers, for a revision open but not
//! in work, the update of its title, summary and parent alone, as that
//! reading says what differs (`App::submit_item`). All read away from the
//! screen: the statuses as the stack is shown, again as those revisions
//! change and right after a submit, and every few minutes while the editor
//! runs; then the parents and the latest diffs of every revision the
//! stack's patches name, in a few calls for all of them, each time the
//! statuses are read, and again after a pull, and the comments and the
//! latest diff, raw, of each revision modified since it was read, as its
//! search says, or that a submit or a pull touched, kept by revision, its
//! diff kept by id; and what differs in the code of each patch, one at a
//! time, the one shown first. Kept in memory as long as the editor runs,
//! nothing written; a read that fails leaves what was known as it was.
//! While Phabricator turns this machine away (HTTP 429), nothing is read
//! away from the screen for a while, longer at each refusal in a row, the
//! bottom line saying until when (`Statuses::turned_away`). Not while
//! reviews come from nowhere, nor in the tour. Built with the `moz-phab`
//! feature.

use super::compared::{ONLY_WRAPPED, row};
use super::*;
use crate::diff::{FileDiff, Hunk};
use crate::phab::{Latest, Searched, Status, Wanted};
use crate::submit::{Below, Parent};

/// How long the statuses go between reads while the stack's revisions are
/// the same: a few minutes, as a try push followed does between looks.
const EVERY: Duration = Duration::from_secs(180);

/// How long the reads away from the screen wait once Phabricator turns this
/// machine away (HTTP 429), when it does not say how long: at first, and
/// at most, however many times in a row it does.
const FIRST_WAIT: Duration = Duration::from_secs(300);
const LONGEST_WAIT: Duration = Duration::from_secs(1800);

/// How long the reads away from the screen wait after the `n`th time in a
/// row (from 1) that Phabricator turned this machine away: as long as it
/// asked, `asked`, if it did; else five minutes, twice as long each time
/// after, up to half an hour.
fn wait(asked: Option<Duration>, n: u32) -> Duration {
    asked.unwrap_or_else(|| FIRST_WAIT.saturating_mul(1 << n.saturating_sub(1).min(3)).min(LONGEST_WAIT))
}

/// What the Phabricator line says of what differs in the code when the
/// files cannot be read as the patches below leave them.
const AS_TEXT: &str = "The files could not be read as the patches below this one leave them, so this is Phabricator's latest diff against this version's diff, as text.";

/// What the code of patch `n` (from 1) is compared on, said: each version
/// applied on the stack's base for the first, else on the patches below it.
fn below(n: usize) -> String {
    match n {
        1 => "both on the stack's base".to_string(),
        _ => format!("both on the patches below patch {n}"),
    }
}

/// Whether the statuses of revisions `named` are due a read at `now`, as
/// `s` says they were read: never, or of other revisions; or a submit
/// ended since; or a few minutes ago.
fn due(s: &Statuses, named: &[(String, String)], now: Instant) -> bool {
    s.stale || s.asked.as_ref().is_none_or(|(asked, when)| asked != named || now.duration_since(*when) >= EVERY)
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
    /// Not known yet: the revision, its latest diff and its parents, the
    /// comparison of its code with this version, or the revision of the
    /// patch below, not read yet. The line has `…` in its place.
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

/// The title and summary of revision `s`, as its search found them, and as
/// patch `p` would send them, as a submit reads them (`submit::wip_title`,
/// `submit::summary_of`, the lines naming a revision left out of both): its
/// title `WIP: ` first, or not, as the revision's is
/// (`submit::title_against`, which a submit to a revision not in work
/// sends). None when they are the same to the letter, the white space at
/// their end aside, as a submit compares them before it sends them: where
/// their lines wrap too, as that is what lands, so that a message wrapped
/// again since it was submitted differs until it is submitted again.
/// Phabricator gives a title and summary back as a submit sent them, over
/// Conduit, to the letter: nothing else is to be allowed for.
fn message_against(s: &Searched, p: &Patch, trailer: &str) -> Option<(String, String)> {
    let before = format!("{}\n\n{}", s.title, crate::submit::summary_of(&format!("\n{}", s.summary), trailer)).trim_end().to_string();
    let title = crate::submit::title_against(p.title(), &s.title);
    let after = format!("{title}\n\n{}", crate::submit::summary_of(&p.message, trailer)).trim_end().to_string();
    (before != after).then_some((before, after))
}

/// Whether two versions of a title and summary, `before` and `after`, have
/// the same words, and differ only in where their lines wrap, or in their
/// white space (`messages::diff`).
fn only_wrapped(before: &str, after: &str) -> bool {
    before != after && crate::messages::diff(before, after).runs.iter().all(|r| r.same)
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

/// Whether diffs `theirs` and `mine` of a patch are on two bases, as what
/// each says of the files both change, as they are before the patch,
/// tells: a line both have at the same place there, kept or taken out, is
/// another in one; a hunk the same in both, its lines and all, starts
/// elsewhere in one; or one makes the file, and the other finds it there.
/// On one base, both read each file as it is there, at the same places.
fn other_bases(theirs: &str, mine: &str) -> bool {
    let (a, b) = (crate::diff::parse_diff(theirs, ""), crate::diff::parse_diff(mine, ""));
    let made = |f: &FileDiff| f.header.iter().any(|l| l.starts_with("new file mode"));
    // The lines of the file before the patch, by their number there.
    let before = |f: &FileDiff| -> HashMap<usize, String> {
        let kept = f.hunks.iter().flat_map(|h| &h.lines).filter(|l| matches!(l.origin, ' ' | '-'));
        kept.map(|l| (l.old_no, l.text.get(1..).unwrap_or_default().to_string())).collect()
    };
    let start = |h: &Hunk| h.lines.first().map(|l| l.old_no);
    fn lines(h: &Hunk) -> Vec<&str> {
        h.lines.iter().map(|l| l.text.as_str()).collect()
    }
    a.iter().any(|fa| {
        let Some(fb) = b.iter().find(|f| f.path == fa.path) else { return false };
        let (x, y) = (before(fa), before(fb));
        made(fa) != made(fb)
            || x.iter().any(|(k, t)| y.get(k).is_some_and(|u| u != t))
            || fa.hunks.iter().any(|ha| fb.hunks.iter().any(|hb| lines(ha) == lines(hb) && start(ha) != start(hb)))
    })
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

/// How many comments are not pulled yet, new and edited (`review::not_pulled`), as
/// the Phabricator line says it.
fn comments_said((new, edited): (usize, usize)) -> String {
    let n = |k: usize, what: &str| format!("{k} {what} comment{}", if k == 1 { "" } else { "s" });
    match (new, edited) {
        (0, 0) => "no new comments".into(),
        (k, 0) => format!("{} to pull", n(k, "new")),
        (0, e) => format!("{} to pull", n(e, "edited")),
        (k, e) => format!("{} and {} to pull", n(k, "new"), n(e, "edited")),
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

/// What Phabricator has of the stack's revisions, as last read, and their
/// reads.
#[derive(Default)]
pub(super) struct Statuses {
    /// Each revision's, by its link, as its search found it: its status,
    /// title, summary and PHID.
    known: HashMap<String, Searched>,
    /// The revisions the last search that worked asked for and did not
    /// find, by link.
    missing: HashSet<String>,
    /// The revisions read last, `D123` and its link, and when that read
    /// started.
    asked: Option<(Vec<(String, String)>, Instant)>,
    /// A read is under way.
    pub(super) reading: bool,
    /// To be read again as soon as no read is under way: a submit ended.
    stale: bool,
    /// A read failed and the status line said so: not again until one
    /// works.
    failed: bool,
    /// Phabricator turning this machine away (HTTP 429): how many times in
    /// a row, and until when nothing is read away from the screen. None
    /// once a read works.
    away: Option<(u32, Instant)>,
    /// How many times the statuses were read, or a pull ended: what the
    /// Phabricator lines read is read again each time.
    round: u64,
    /// The latest diff, the comments and the parents of each revision the
    /// stack's patches name, by link, as last read; the round they were
    /// last read in, whether that worked or not; and whether they are being
    /// read.
    latest: HashMap<String, Latest>,
    read_in: Option<u64>,
    reading_latest: bool,
    /// The revisions, by link, that a submit or a pull touched while they
    /// were being read: what that read brings of them is forgotten as it
    /// comes (`App::forget`).
    forgot: HashSet<String>,
    /// The revision the submit under way sends to, by link, if it names
    /// one; and those the pull under way reads: forgotten once each is
    /// done.
    sending: Option<String>,
    pulling: Vec<String>,
    /// What differs in the code between a revision's latest diff and a
    /// version of its patch, on the patches below it, by what it was worked
    /// out from (`compare_key`); and the one being worked out.
    compared: HashMap<String, Code>,
    comparing: Option<String>,
    /// How many times what the line may say changed: the middle's rows
    /// are built again each time.
    seq: u64,
}

impl Statuses {
    /// Phabricator turned this machine away at `now`, asking to wait
    /// `asked`, if it did: how long nothing is read away from the screen,
    /// from then, by how many times in a row it did since a read worked
    /// (`wait`).
    fn turned_away(&mut self, asked: Option<Duration>, now: Instant) -> Duration {
        let n = self.away.map_or(0, |(n, _)| n) + 1;
        let wait = wait(asked, n);
        self.away = Some((n, now + wait));
        wait
    }

    /// Whether nothing is read away from the screen at `now`, Phabricator
    /// having turned this machine away.
    fn waiting(&self, now: Instant) -> bool {
        self.away.is_some_and(|(_, until)| now < until)
    }
}

impl App {
    /// The revisions the current stack's patches name, `D123` and its link
    /// (`phab::revision_of`): each once, by link.
    fn revisions_named(&self) -> Vec<(String, String)> {
        let Some(s) = self.current() else { return Vec::new() };
        let mut named: Vec<(String, String)> = s.patches.iter().filter_map(|id| crate::phab::revision_of(&self.db.patch(id).message)).collect();
        named.sort_by(|a, b| a.1.cmp(&b.1));
        named.dedup_by(|a, b| a.1 == b.1);
        named
    }

    /// Whether this editor reads Phabricator for what it shows: not while
    /// reviews come from nowhere, nor in the tour; nor from a process that
    /// keeps nothing, as the tests' own, without a stand-in.
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

    /// The statuses of the revisions the current stack's patches name, read
    /// away from the screen, when they are due: never read, or for other
    /// revisions (the stack shown, or its patches naming others), a submit
    /// ended since, or a few minutes gone by. Not while a read is under
    /// way, theirs or the revisions' (`check_latest`), one read of
    /// Phabricator after another, nor while Phabricator turns this machine
    /// away, nor while the editor does not read Phabricator. What the
    /// Phabricator lines read is read again after them.
    pub(super) fn check_statuses(&mut self) {
        if self.statuses.reading || self.statuses.reading_latest || self.statuses.waiting(Instant::now()) || !self.reads_phabricator() {
            return;
        }
        let named = self.revisions_named();
        if named.is_empty() || !due(&self.statuses, &named, Instant::now()) {
            return;
        }
        self.statuses.asked = Some((named.clone(), Instant::now()));
        self.statuses.reading = true;
        self.statuses.stale = false;
        self.statuses.round += 1;
        let db = self.path.clone();
        self.spawn(move || Done::Statuses(crate::phab::statuses(&db, &named)));
    }

    /// Right after a submit: what was read of the revision it sent to
    /// forgotten (`forget`); the database read again, for the link of a new
    /// revision, and the statuses with it, at once, or once the read under
    /// way is done; then what the Phabricator lines read.
    pub(super) fn statuses_again(&mut self) {
        let sent: Vec<String> = self.statuses.sending.take().into_iter().collect();
        self.forget(&sent);
        self.reload();
        self.statuses.stale = true;
        self.check_statuses();
        self.check_line();
    }

    /// Right after a pull: what was read of the revisions it read forgotten
    /// (`forget`); then the parents and the latest diffs of every revision
    /// read again, and the comments of those forgotten or modified since,
    /// those pulled in the database now.
    pub(super) fn line_again(&mut self) {
        let pulled = std::mem::take(&mut self.statuses.pulling);
        self.forget(&pulled);
        self.statuses.round += 1;
        self.check_line();
    }

    /// The submit under way sends version `id` of a patch: the revision it
    /// names, if it does, forgotten once the submit is done
    /// (`statuses_again`).
    pub(super) fn sends(&mut self, id: &str) {
        self.statuses.sending = crate::phab::revision_of(&self.db.patch(id).message).map(|(_, link)| link);
    }

    /// What was read of the revisions at `links`, which a submit or a pull
    /// touched, forgotten as not read since they were last modified: their
    /// comments and latest diff are read again in the next round, whatever
    /// their search says, and so, should they come in the read under way,
    /// in the round after it.
    fn forget(&mut self, links: &[String]) {
        for link in links {
            if let Some(l) = self.statuses.latest.get_mut(link) {
                l.modified = None;
            }
            if self.statuses.reading_latest {
                self.statuses.forgot.insert(link.clone());
            }
        }
    }

    /// Phabricator turned this machine away (HTTP 429) as it was read away
    /// from the screen, asking to wait `asked`, if it did: nothing is read
    /// so until the wait is over (`Statuses::turned_away`), and the bottom
    /// line says when that is.
    fn turned_away(&mut self, asked: Option<Duration>) {
        let wait = self.statuses.turned_away(asked, Instant::now());
        let at = local_time(crate::db::now() + wait.as_secs() as i64);
        let at = at.rsplit(' ').next().unwrap_or_default();
        self.status = format!("Phabricator is turning away requests from this machine for now (HTTP 429): stacked reads it again at {at}.");
    }

    /// What a read of the statuses came to: those read, each revision read
    /// that Phabricator did not give known no more, and known to be missing,
    /// and no wait any more; or, failed, those known kept, and why said,
    /// unless the last read failed too; or, Phabricator turning this machine
    /// away, those known kept, and nothing read until the wait is over, the
    /// bottom line saying when that is, the statuses read then. A submit
    /// ended meanwhile, they are read again at once. Then what the
    /// Phabricator line needs next.
    pub(super) fn statuses_read(&mut self, read: std::result::Result<HashMap<String, Searched>, crate::phab::Error>) {
        self.statuses.reading = false;
        let asked = self.statuses.asked.as_ref().map(|(a, _)| a.clone()).unwrap_or_default();
        match read {
            Ok(found) => {
                for (_, link) in &asked {
                    self.statuses.known.remove(link);
                    self.statuses.missing.remove(link);
                    if !found.contains_key(link) {
                        self.statuses.missing.insert(link.clone());
                    }
                }
                self.statuses.known.extend(found);
                self.statuses.failed = false;
                self.statuses.away = None;
                self.statuses.seq += 1;
            }
            Err(crate::phab::Error::TurnedAway(asked)) => {
                self.statuses.stale = true;
                self.turned_away(asked);
            }
            Err(e) if !self.statuses.failed => {
                self.statuses.failed = true;
                let kept = asked.iter().any(|(_, link)| self.statuses.known.contains_key(link));
                self.status = match kept {
                    true => format!("The statuses of the stack's revisions could not be read from Phabricator again, so the patches pane shows them as they were: {e}"),
                    false => format!("The statuses of the stack's revisions could not be read from Phabricator: {e}"),
                };
            }
            Err(_) => {}
        }
        if self.statuses.stale {
            self.check_statuses();
        }
        self.check_line();
    }

    /// The status of patch `id`'s revision, as last read: none when it
    /// names none, or its status is not known yet.
    pub(super) fn revision_status(&self, id: &str) -> Option<Status> {
        if self.statuses.known.is_empty() {
            return None;
        }
        let (_, link) = crate::phab::revision_of(&self.db.patch(id).message)?;
        self.statuses.known.get(&link).map(|s| s.status)
    }

    /// The mark the patches pane shows right after the status of patch
    /// `id`'s revision: `✓` where Phabricator has this exact version of the
    /// patch, `✗` where anything differs, as its Phabricator line says it,
    /// from the same reading (`synced`). None, a blank, while that is not
    /// known, or Phabricator does not have the revision; and when the patch
    /// names no revision.
    pub(super) fn sync_mark(&self, id: &str) -> Option<&'static str> {
        let p = self.db.patch(id);
        let (_, link) = crate::phab::revision_of(&p.message)?;
        self.synced(p, &link).mark()
    }

    /// Whether what Phabricator has is being read, or compared with a
    /// patch.
    pub(super) fn reading_phabricator(&self) -> bool {
        self.statuses.reading || self.statuses.reading_latest || self.statuses.comparing.is_some()
    }

    // -- the Phabricator line over the patch's message ---------------------- //

    /// What the middle's rows are built again for, as the Phabricator line
    /// changes: how many times what it may say changed.
    pub(super) fn line_seq(&self) -> u64 {
        self.statuses.seq
    }

    /// What the code of patch `p`, the one picked, is compared with its
    /// revision's latest diff (at `link`) by: that diff, the version, the
    /// patches below it and the stack's base. None before that diff is read.
    fn compare_key(&self, p: &Patch, link: &str) -> Option<String> {
        let l = self.statuses.latest.get(link)?;
        let s = self.current()?;
        let at = s.patches.iter().position(|id| *id == p.id)?;
        Some(format!("{link}\0{}\0{}\0{}\0{}", l.diff, p.id, s.patches[..at].join(","), s.base.as_deref().unwrap_or("")))
    }

    /// What comparing the code of patch `p` with its revision's latest diff
    /// (at `link`) takes, away from the screen.
    fn compare_job(&self, p: &Patch, link: &str) -> Option<Job> {
        let l = self.statuses.latest.get(link)?;
        let s = self.current()?;
        let at = s.patches.iter().position(|id| *id == p.id)?;
        Some(Job {
            raw: l.raw.clone(),
            mine: p.diff.clone(),
            below: s.patches[..at].iter().map(|id| self.db.patch(id).diff.clone()).collect(),
            pad: s.repo.clone().zip(s.base.clone()).map(|(repo, base)| (PathBuf::from(repo), base)),
            at,
        })
    }

    /// What the Phabricator lines of the stack's patches need, started away
    /// from the screen when it is due: the latest diff, the comments and
    /// the parents of every revision the patches name, read once each
    /// round, once the statuses' search has found their PHIDs
    /// (`check_latest`); then what differs in the code of each patch,
    /// worked out once for each version of it on the patches below it, the
    /// one the middle shows first (`check_compare`). One read and one
    /// comparison at a time. Not while the editor does not read
    /// Phabricator.
    pub(super) fn check_line(&mut self) {
        if !self.reads_phabricator() {
            return;
        }
        self.check_latest();
        self.check_compare();
    }

    /// The revision at `link`, the one of a patch below another, as the
    /// statuses' search found it, for the parent's rule
    /// (`submit::parent_with`): not known while it is not read.
    fn below(&self, link: &str) -> std::result::Result<Below, ()> {
        match self.statuses.known.get(link) {
            Some(s) => Ok(Below::Found { phid: s.phid.clone(), open: s.status.open(), status: s.status.name().to_string() }),
            None if self.statuses.missing.contains(link) => Ok(Below::Missing),
            None => Err(()),
        }
    }

    /// The revisions the current stack's patches name that the statuses'
    /// search found, as the Phabricator lines read them (`phab::Wanted`):
    /// that of the patch the middle shows first, then the others, bottom
    /// first, each once; each with when it was last modified, as its search
    /// says, what was read of it before, and whether its parents are to be
    /// named, as a submit of a patch naming it would set it one, whatever
    /// parents it has.
    fn wanted(&self) -> Vec<Wanted> {
        let Some(s) = self.current() else { return Vec::new() };
        let shown = self.patch_id().filter(|_| !self.on_stack).and_then(|id| s.patches.iter().position(|p| *p == id));
        let none: &[String] = &[];
        let mut out: Vec<Wanted> = Vec::new();
        for at in shown.into_iter().chain(0..s.patches.len()) {
            let p = self.db.patch(&s.patches[at]);
            let Some((d, link)) = crate::phab::revision_of(&p.message) else { continue };
            let Some(found) = self.statuses.known.get(&link).filter(|f| !f.phid.is_empty()) else { continue };
            let sets = crate::submit::parent_with(&self.db, s, at, Some((d.as_str(), none)), |_, below| self.below(below));
            let sets = matches!(sets, Ok(Parent::Set { .. }));
            match out.iter_mut().find(|w| w.link == link) {
                Some(w) => w.name_parents |= sets,
                None => {
                    let had = self.statuses.latest.get(&link).cloned();
                    out.push(Wanted { d, link, phid: found.phid.clone(), modified: found.modified, had, name_parents: sets });
                }
            }
        }
        out
    }

    /// The latest diff, the comments and the parents of every revision the
    /// current stack's patches name (`wanted`), read away from the screen
    /// in a few calls for all of them (`phab::latests`), once each round,
    /// the comments and the latest diff only of those modified since they
    /// were read, each revision kept as soon as it is read. Not while the
    /// statuses are read, nor while the revisions are, for a round before,
    /// nor while Phabricator turns this machine away.
    fn check_latest(&mut self) {
        let st = &self.statuses;
        if st.reading || st.reading_latest || st.read_in == Some(st.round) || st.waiting(Instant::now()) {
            return;
        }
        let wanted = self.wanted();
        if wanted.is_empty() {
            return;
        }
        self.statuses.read_in = Some(self.statuses.round);
        self.statuses.reading_latest = true;
        self.statuses.forgot.clear();
        let (db, tx) = (self.path.clone(), self.bg.tx.clone());
        self.spawn(move || {
            let mut got = |link: &str, l: Latest| {
                let _ = tx.send(Done::Latest(link.to_string(), l));
            };
            Done::LatestAll(crate::phab::latests(&db, &wanted, &mut got).err())
        });
    }

    /// What differs in the code of the next patch due a comparison,
    /// worked out away from the screen (`next_compare`), one at a time.
    fn check_compare(&mut self) {
        if self.statuses.comparing.is_some() {
            return;
        }
        let Some((key, job)) = self.next_compare() else { return };
        self.statuses.comparing = Some(key.clone());
        self.spawn(move || Done::Compared(key, compare(job)));
    }

    /// The next patch of the current stack whose code is due a comparison
    /// with its revision's latest diff, once that is read: the one the
    /// middle shows first, then the others, bottom first, each compared
    /// once until it, the patches below it, or the diff changes
    /// (`compare_key`). What it is compared by, and what that takes.
    fn next_compare(&self) -> Option<(String, Job)> {
        let s = self.current()?;
        let shown = self.patch_id().filter(|_| !self.on_stack);
        shown.iter().chain(&s.patches).find_map(|id| {
            let p = self.db.patch(id);
            let (_, link) = crate::phab::revision_of(&p.message)?;
            let key = self.compare_key(p, &link).filter(|k| !self.statuses.compared.contains_key(k))?;
            Some((key, self.compare_job(p, &link)?))
        })
    }

    /// Revision `link`'s latest diff, comments and parents, read: kept, in
    /// place of what was known of it, but forgotten as not read since it
    /// was last modified, if a submit or a pull touched it meanwhile. Then
    /// what the lines need next.
    pub(super) fn latest_read(&mut self, link: String, mut l: Latest) {
        if self.statuses.forgot.contains(&link) {
            l.modified = None;
        }
        self.statuses.latest.insert(link, l);
        self.statuses.seq += 1;
        self.check_line();
    }

    /// The revisions of a round read, and no wait any more; or their read
    /// stopped at a call that `failed`, what was known of the others kept
    /// as it was, nothing said, as the statuses' reads say what is wrong,
    /// but for Phabricator turning this machine away: then nothing is read
    /// until the wait is over, the bottom line saying when that is, and the
    /// revisions are read then. Then what is due next: the statuses, should
    /// a submit have ended meanwhile, and what the lines need.
    pub(super) fn latests_done(&mut self, failed: Option<crate::phab::Error>) {
        self.statuses.reading_latest = false;
        match failed {
            None => self.statuses.away = None,
            Some(crate::phab::Error::TurnedAway(asked)) => {
                self.statuses.read_in = None;
                self.turned_away(asked);
            }
            Some(_) => {}
        }
        self.check_statuses();
        self.check_line();
    }

    /// What differs in the code, worked out from `key`, kept; then what the
    /// lines need next.
    pub(super) fn code_compared(&mut self, key: String, code: Code) {
        self.statuses.comparing = None;
        self.statuses.compared.insert(key, code);
        self.statuses.seq += 1;
        self.check_line();
    }

    /// Whether the search did not find revision `link`, and nothing is
    /// known of it.
    fn not_found(&self, link: &str) -> bool {
        self.statuses.missing.contains(link) && !self.statuses.known.contains_key(link)
    }

    /// What differs between patch `p` and its revision at `link`, once the
    /// revision's search, its latest diff and its parents, the comparison
    /// of its code with this version, and what a submit gives it as its
    /// parent (`parent_of`) are known.
    fn differs(&self, p: &Patch, link: &str) -> Option<Differs<'_>> {
        let s = self.statuses.known.get(link)?;
        let l = self.statuses.latest.get(link)?;
        let code = self.statuses.compared.get(&self.compare_key(p, link)?)?;
        Some(Differs {
            code: match code {
                Code::Same => None,
                Code::Differs(code, said) => Some((code, said)),
            },
            message: message_against(s, p, &self.cfg.reviews.trailer),
            parent: self.parent_of(p, l)?,
            parents_now: l.parents.iter().map(|(_, d)| d.clone()).collect(),
        })
    }

    /// The parent a submit gives the revision of patch `p`, `l` what was
    /// read of it, its parents with the rest, by the submit's own rule
    /// (`submit::parent_with`), the revision of the patch below as the
    /// statuses' search found it: none while that is not known.
    fn parent_of(&self, p: &Patch, l: &Latest) -> Option<Parent> {
        let s = self.current()?;
        let at = s.patches.iter().position(|id| *id == p.id)?;
        let (d, _) = crate::phab::revision_of(&p.message)?;
        let parents: Vec<String> = l.parents.iter().map(|(phid, _)| phid.clone()).collect();
        crate::submit::parent_with(&self.db, s, at, Some((d.as_str(), parents.as_slice())), |_, below| self.below(below)).ok()
    }

    /// Whether Phabricator has the exact version of patch `p`, of its
    /// revision at `link` (`Synced`): the one place the Phabricator line,
    /// its view of what differs, its menu and `?`, and the patches pane's
    /// mark read it from. Not found, when the search did not find it and
    /// nothing is known of it; else what differs (`differs`), once all of
    /// that is known.
    fn synced(&self, p: &Patch, link: &str) -> Synced<'_> {
        if self.not_found(link) {
            return Synced::NotFound;
        }
        self.differs(p, link).map_or(Synced::Unknown, Synced::Known)
    }

    /// The Phabricator line of patch `p`, of revision `d` at `link`, `sync`
    /// whether Phabricator has this version (`synced`): the revision; its
    /// status, by name; whether its latest diff, title and summary are this
    /// version's, and its parent the one a submit sets; how many of its
    /// comments are not pulled yet. What is not known yet is `…`, once for
    /// those in a row; a revision the search did not find, said so, and
    /// nothing else.
    fn line_said(&self, p: &Patch, d: &str, link: &str, sync: &Synced) -> String {
        let sync = match sync {
            Synced::NotFound => return format!("Phabricator: {d} · not found on Phabricator"),
            Synced::Unknown => None,
            Synced::Known(x) => Some(x.said().to_string()),
        };
        let status = self.statuses.known.get(link).map(|s| s.status.name().to_string());
        let comments = self.statuses.latest.get(link).map(|l| comments_said(crate::review::not_pulled(&self.db, &self.stack, &p.change, &l.comments)));
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
        out
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
        let raw = self.statuses.latest.get(link).map_or("", |l| l.raw.as_str());
        match x.code {
            None => Shown::Same,
            Some((files, None)) if other_bases(raw, &p.diff) => Shown::Files(files),
            Some((_, unread)) => Shown::Interdiff(unread.is_some() && other_bases(raw, &p.diff)),
        }
    }

    /// The view of what differs, `x`, between patch `p`, the one picked,
    /// and its revision `d` at `link`: its title, and its lines `w` wide.
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
    /// only that nothing differs, the parent named.
    fn view_of(&self, p: &Patch, d: &str, link: &str, x: &Differs, w: usize) -> (String, Vec<Parts>) {
        let n = self.sel + 1;
        let title = format!("{d} on Phabricator, then patch {n} as it is");
        let mut said: Vec<String> = Vec::new();
        let mut lines: Vec<Parts> = Vec::new();
        // A line of a diff, folded to the width under its sign.
        let folded = |all: &[ops::InterLine]| -> Vec<Parts> {
            let refs: Vec<&ops::InterLine> = all.iter().collect();
            inter_parts(&refs).iter().flat_map(|parts| fold_parts(parts, w, 1)).collect()
        };
        if x.none() {
            said.push(format!("Nothing differs: {}", x.same_said(d, &format!("this version of patch {n}"))));
        } else {
            said.extend(x.parent_said());
            let shown = self.shown_code(p, link, x);
            said.push(match &x.message {
                Some((before, after)) if only_wrapped(before, after) => format!(
                    "The title and summary of {d} are compared with those a submit of this version sends, where their lines wrap aside. {ONLY_WRAPPED} Each version's lines are shown as it writes them."
                ),
                Some(_) => format!("The title and summary of {d} are compared with those a submit of this version sends, where their lines wrap aside."),
                None => format!("The title and summary of {d} are those of this version."),
            });
            said.push(match shown {
                Shown::Same => format!("The latest diff of {d} does what this version does."),
                Shown::Interdiff(false) => format!("The code is shown as an interdiff of the latest diff of {d} and this version's diff."),
                Shown::Interdiff(true) => format!(
                    "The latest diff of {d} is on another base than this version, and the files could not be read as the patches below this one leave them, so the code is shown as an interdiff of the two diffs all the same."
                ),
                Shown::Files(_) => format!(
                    "The latest diff of {d} is on another base than this version, so the code shown is each file as that diff leaves it, then as this version leaves it, {}.",
                    below(n)
                ),
            });
            // Phabricator's version, then this one: over the diffs the
            // interdiff compares, or, where it compares none, this version's
            // under both.
            let raw = self.statuses.latest.get(link).map_or("", |l| l.raw.as_str());
            let (theirs, mine) = match shown {
                Shown::Interdiff(_) => (bare(raw), bare(&p.diff)),
                _ => (p.diff.clone(), p.diff.clone()),
            };
            let (before, after) = x.message.clone().unwrap_or_else(|| (p.message.clone(), p.message.clone()));
            let mut inter = ops::letter_interdiff_lines(&Patch { message: before, diff: theirs, ..p.clone() }, &Patch { message: after, diff: mine, ..p.clone() });
            // Its two first lines name the two versions.
            if let [a, b, ..] = inter.as_mut_slice()
                && a.text.starts_with("--- ")
                && b.text.starts_with("+++ ")
            {
                a.text = format!("--- {d} on Phabricator");
                b.text = format!("+++ patch {n} as it is");
            }
            if !inter.is_empty() {
                lines.push(vec![(String::new(), Style::new())]);
                lines.extend(folded(&inter));
            }
            if let Shown::Files(code) = shown {
                lines.push(vec![(String::new(), Style::new())]);
                lines.push(vec![(" the code: Phabricator's, then this version's".to_string(), color::file())]);
                lines.extend(folded(&code.lines().map(ops::InterLine::from).collect::<Vec<_>>()));
            }
        }
        let mut out: Vec<Parts> = wrap(&said.join(" "), w.saturating_sub(2).max(10)).into_iter().map(|l| vec![(format!(" {l}"), color::meta())]).collect();
        out.extend(lines);
        (title, out)
    }

    /// What differs between the patch the middle shows and its revision on
    /// Phabricator, as its view shows it (`view_of`), its lines as wide as
    /// the screen: its title and its lines. Or, nothing of it known, or
    /// Phabricator not having the revision, why, for the status line.
    fn differs_view(&self) -> std::result::Result<(String, Vec<Parts>), String> {
        let n = self.sel + 1;
        let Some(p) = self.shown_patch() else { return Err("No patch is shown, so nothing of it is compared with Phabricator.".into()) };
        let Some((d, link)) = crate::phab::revision_of(&p.message) else {
            return Err(format!("Patch {n} names no revision, so nothing of it is compared with Phabricator."));
        };
        match self.synced(p, &link) {
            Synced::NotFound => Err(format!("Phabricator has no revision {d}, so nothing of it is compared with patch {n}.")),
            Synced::Known(x) => Ok(self.view_of(p, &d, &link, &x, self.body_w)),
            Synced::Unknown => Err(format!("What differs between patch {n} and {d} on Phabricator is not known yet: the line says it once {d} is read and compared.")),
        }
    }

    /// What differs between the patch the middle shows and its revision on
    /// Phabricator, on a screen of its own, Esc going back where the cursor
    /// was (`differs_view`): from the Phabricator line's menu and the
    /// patch's, and Space on the line. Not known, the status line says why.
    pub(super) fn open_differs(&mut self) {
        match self.differs_view() {
            Ok((title, lines)) => self.open_parts(title, lines),
            Err(why) => self.status = why,
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

    /// The items about the revision of the patch the middle shows, on the
    /// Phabricator line's menu and on the patch's in the patches pane: what
    /// differs from it on Phabricator, on a screen of its own, grayed while
    /// that is not known; and a pull of its reviews alone. Why the first is
    /// grayed, if it is, for the menu to say. No item, while reviews come
    /// from nowhere, or the patch names no revision.
    pub(super) fn revision_items(&self, items: &mut Vec<Item>) -> Option<String> {
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
        items.push(Item::new(format!("Pull the reviews of {d} from Phabricator"), Action::PullPatch(p.change.clone())));
        why.map(|why| format!("What differs from {d} on Phabricator is grayed: {why}"))
    }

    /// The item of the patch's menu that sends the patch the middle shows
    /// to Phabricator, by the status of its revision, as last read, and by
    /// what its Phabricator line knows (`synced`); offered when `can` (the
    /// stack can be changed, and HEAD does not hold the patch back). With
    /// it, why it is grayed, when that is for a reason of its own, for the
    /// menu to say. `Submit to Phabricator as WIP` for a patch that names
    /// no revision, for a revision in work (Changes Planned, a draft), and
    /// while the status is not read, is one stacked does not know, or for a
    /// revision Phabricator does not have: what the submit reads decides
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

    /// The title of the Phabricator line's menu: its revision, on
    /// Phabricator.
    pub(super) fn line_title(&self) -> Option<String> {
        let (d, _) = crate::phab::revision_of(&self.shown_patch()?.message)?;
        Some(format!("{d} on Phabricator"))
    }

    /// The reviews pulled from Phabricator, away from the screen: of every
    /// revision the stack's patches name, or of the patch of change `only`'s
    /// alone; the banner saying where the pull is as it goes, what it brings
    /// held for review, the Phabricator line read again once it is done.
    /// One pull at a time.
    pub(super) fn pull_reviews(&mut self, only: Option<String>) {
        if self.bg.doing.contains(&"pulling") {
            self.status = "already pulling the reviews".into();
            return;
        }
        let name = self.stack.clone();
        // Of one patch: its revision, said from the start.
        let alone = only.as_ref().and_then(|change| {
            let s = self.current()?;
            let p = s.patches.iter().map(|id| self.db.patch(id)).find(|p| p.change == *change)?;
            crate::phab::revision_of(&p.message)
        });
        // What it reads, forgotten once it is done (`line_again`).
        self.statuses.pulling = match (&only, &alone) {
            (None, _) => self.revisions_named().into_iter().map(|(_, link)| link).collect(),
            (Some(_), alone) => alone.iter().map(|(_, link)| link.clone()).collect(),
        };
        let alone = alone.map(|(d, _)| d);
        self.bg.doing.push("pulling");
        self.status = match &alone {
            Some(d) => format!("pulling the reviews of {d} from Phabricator…"),
            None => "pulling the reviews from Phabricator…".into(),
        };
        self.bg.pull = Some((name.clone(), alone.map_or(review::Pulling::Starting, review::Pulling::Alone)));
        let (path, tx) = (self.path.clone(), self.bg.tx.clone());
        self.spawn(move || {
            // Where it is, as it goes: the banner says.
            let mut say = |p| {
                let _ = tx.send(Done::Pulling(p));
            };
            let pulled = match &only {
                Some(change) => review::pull_patch_saying(&path, &name, change, &mut say),
                None => review::pull_saying(&path, &name, &mut say),
            };
            Done::Said(
                "pulling",
                match pulled {
                    Ok(r) => r.summary,
                    Err(e) => e.0,
                },
            )
        });
    }

    /// What `?` says on the Phabricator line: what the line says, what
    /// Space does there, and what the menu has.
    pub(super) fn line_help(&self) -> Vec<String> {
        let Some(p) = self.shown_patch() else { return Vec::new() };
        let Some((d, link)) = crate::phab::revision_of(&p.message) else { return Vec::new() };
        let n = self.sel + 1;
        let mut out = vec![format!(
            "This line says what Phabricator has of patch {n}: its revision, {d}; the status of {d}, by its name; whether {d} is in sync with this version; and how many of the comments of {d} are not pulled yet. In sync, the latest diff of {d} does what this version does; the title and summary of {d} are exactly those a submit sends, where their lines wrap too; and a submit would not change the parent of {d}, which a submit sets to the revision of the patch below, when that is open. What this line says is read from Phabricator, away from the screen, for every patch of the stack: as the stack is shown, every three minutes, and again after a submit or a pull. Where something is not read yet, the line says … in its place. The patches pane says the same after the status of each patch's revision: ✓ in sync, ✗ when something differs."
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
                "Press Space to see what differs from {d} on Phabricator, on a screen of its own, once {d} is read and compared: until then, Space says that it is not known yet."
            ),
        });
        out.push(format!("Press Enter for all that can be done here: what differs from {d} on Phabricator, and a pull of the reviews of {d} alone."));
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
    fn the_statuses_are_due_a_read_when_never_read_for_other_revisions_after_a_submit_or_every_few_minutes() {
        let named = |ds: &[&str]| ds.iter().map(|d| (d.to_string(), format!("https://p.example/{d}"))).collect::<Vec<_>>();
        let then = Instant::now();
        assert!(due(&Statuses::default(), &named(&["D1"]), then), "never read");
        let s = Statuses { asked: Some((named(&["D1", "D2"]), then)), ..Default::default() };
        assert!(!due(&s, &named(&["D1", "D2"]), then + Duration::from_secs(179)), "read under three minutes ago");
        assert!(due(&s, &named(&["D1", "D2"]), then + EVERY), "three minutes on");
        assert!(due(&s, &named(&["D1", "D3"]), then), "other revisions");
        assert!(due(&s, &named(&["D1"]), then), "fewer");
        let s = Statuses { stale: true, ..s };
        assert!(due(&s, &named(&["D1", "D2"]), then), "a submit ended since");
        assert_eq!(EVERY, Duration::from_secs(180));
    }

    #[test]
    fn turned_away_nothing_is_read_for_as_long_as_phabricator_asks_else_five_minutes_doubling_up_to_thirty() {
        let then = Instant::now();
        let minutes = |m: u64| Duration::from_secs(60 * m);
        // Two minutes asked: not read before they are over, read once they
        // are.
        let mut s = Statuses::default();
        assert!(!s.waiting(then));
        assert_eq!(s.turned_away(Some(Duration::from_secs(120)), then), Duration::from_secs(120));
        assert!(s.waiting(then) && s.waiting(then + Duration::from_secs(119)), "for two minutes");
        assert!(!s.waiting(then + Duration::from_secs(120)), "not after");
        // Nothing asked: five minutes, then ten, twice as long each time in
        // a row, up to thirty; a time asked in between taken as it is, and
        // counted.
        let mut s = Statuses::default();
        let mut at = then;
        let mut waits = Vec::new();
        for asked in [None, None, Some(Duration::from_secs(90)), None, None, None] {
            let w = s.turned_away(asked, at);
            assert!(s.waiting(at + w - Duration::from_secs(1)) && !s.waiting(at + w));
            waits.push(w);
            at += w;
        }
        assert_eq!(waits, [minutes(5), minutes(10), Duration::from_secs(90), minutes(30), minutes(30), minutes(30)]);
        assert_eq!((wait(None, 1), wait(None, 2), wait(None, 3), wait(None, 4), wait(None, 40)), (minutes(5), minutes(10), minutes(20), minutes(30), minutes(30)));
        // A read that works: back to five minutes at the next.
        s.away = None;
        assert!(!s.waiting(at));
        assert_eq!(s.turned_away(None, at), minutes(5));
    }

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
        let searched = |title: &str, summary: &str| Searched { status: Status::NeedsReview, title: title.into(), summary: summary.into(), phid: String::new(), modified: None };
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
            let sent = Searched {
                status: Status::ChangesPlanned,
                title: crate::submit::wip_title(p.title()).title,
                summary: crate::submit::summary_of(&p.message, trailer),
                phid: String::new(),
                modified: None,
            };
            assert_eq!(message_against(&sent, &p, trailer), None, "{message:?}");
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

    #[test]
    fn two_diffs_are_on_other_bases_when_they_read_a_file_otherwise_before_the_patch() {
        let diff = |start: usize, lines: &str| format!("diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -{start},3 +{start},3 @@\n{lines}");
        let mine = diff(10, " line 10\n-line 11\n+LINE 11\n line 12\n");
        // On one base: the same lines at the same places, whatever each
        // changes; more of the file around them in one; another file.
        assert!(!other_bases(&mine, &mine));
        assert!(!other_bases(&diff(10, " line 10\n-line 11\n+Line 11\n line 12\n"), &mine));
        let wider = "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -9,5 +9,5 @@\n line 9\n line 10\n-line 11\n+Line 11\n line 12\n line 13\n";
        assert!(!other_bases(wider, &mine));
        assert!(!other_bases(&mine.replace("a.txt", "b.txt"), &mine));
        // The same hunk elsewhere, as lines were added above it below the
        // patch; another line at the same place, as one was changed there.
        assert!(other_bases(&diff(14, " line 10\n-line 11\n+LINE 11\n line 12\n"), &mine));
        assert!(other_bases(&diff(10, " line 10\n-line 11\n+Line 11\n LINE 12\n"), &mine));
        // A file the one makes, which the other finds there.
        let made = "diff --git a/a.txt b/a.txt\nnew file mode 100644\n--- /dev/null\n+++ b/a.txt\n@@ -0,0 +1 @@\n+LINE 11\n";
        assert!(other_bases(made, &mine) && other_bases(&mine, made));
    }

    #[cfg(feature = "demo")]
    #[test]
    fn the_tour_pulls_nothing_from_phabricator_a_patch_s_revision_alone_neither() {
        assert!(reaches_phabricator(&Action::Pull) && reaches_phabricator(&Action::PullPatch("c".into())));
        // What differs shows what was read, and reads nothing.
        assert!(!reaches_phabricator(&Action::Differs));
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
        assert_eq!(comments_said((0, 0)), "no new comments");
        assert_eq!(comments_said((1, 0)), "1 new comment to pull");
        assert_eq!(comments_said((2, 0)), "2 new comments to pull");
        assert_eq!(comments_said((0, 1)), "1 edited comment to pull");
        assert_eq!(comments_said((3, 2)), "3 new comments and 2 edited comments to pull");
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
