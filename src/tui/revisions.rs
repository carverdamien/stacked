//! What Phabricator has of the stack's revisions: the status of each
//! patch's revision, which the patches pane shows after the patch's number,
//! read in one search of all the revisions the stack's patches name; and
//! the Phabricator line over the message of the patch the middle shows,
//! which says its revision's status by name, whether Phabricator's latest
//! diff and the revision's title and summary are this version's, and how
//! many of the revision's comments are not pulled yet, what differs shown
//! under it with Space. All read away from the screen: the statuses as the
//! stack is shown, again as those revisions change and right after a
//! submit, and every few minutes while the editor runs; the latest diff and
//! the comments of the patch shown alone, once each time the statuses are
//! read, and again after a submit or a pull, kept by revision, its diff
//! kept by id. Kept in memory as long as the editor runs, nothing written;
//! a read that fails leaves what was known as it was. Not while reviews
//! come from nowhere, nor in the tour. Built with the `moz-phab` feature.

use super::submitting::{code_rows, message_rows, row, said_rows};
use super::*;
use crate::phab::{Latest, Searched, Status};

/// How long the statuses go between reads while the stack's revisions are
/// the same: a few minutes, as a try push followed does between looks.
const EVERY: Duration = Duration::from_secs(180);

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

/// What differs between a version of a patch and its revision, once both
/// are known: the code, as a diff, and what it is; the title and summary,
/// Phabricator's then this version's. None of either where it is the same.
struct Differs<'a> {
    code: Option<(&'a String, &'a Option<String>)>,
    message: Option<(String, String)>,
}

impl Differs<'_> {
    /// Whether nothing differs: Phabricator has this exact version.
    fn none(&self) -> bool {
        self.code.is_none() && self.message.is_none()
    }

    /// What the Phabricator line says of it.
    fn said(&self) -> &'static str {
        match (self.code.is_some(), self.message.is_some()) {
            (false, false) => "in sync",
            (true, false) => "the diff differs",
            (false, true) => "the title or summary differs",
            (true, true) => "the diff and the title or summary differ",
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

/// Whether title `t` says the revision is in work, as moz-phab reads it,
/// and as `submit::wip_title` takes it off: `WIP` first, in any case, then
/// `:`, a space or nothing.
fn in_work(t: &str) -> bool {
    let c: Vec<char> = t.chars().collect();
    c.len() >= 3 && c[..3].iter().collect::<String>().eq_ignore_ascii_case("wip") && matches!(c.get(3), None | Some(':' | ' '))
}

/// The title and summary of revision `s`, as its search found them, and as
/// patch `p` would send them, as a submit reads them (`submit::wip_title`,
/// `submit::summary_of`, the lines naming a revision left out of both): its
/// title `WIP: ` first, or not, as the revision's is. None when they are
/// the same to the letter, the white space at their end aside, as a
/// submit compares them before it sends them: where their lines wrap too,
/// as that is what lands, so that a message wrapped again since it was
/// submitted differs until it is submitted again. Phabricator gives a
/// title and summary back as a submit sent them, over Conduit, to the
/// letter: nothing else is to be allowed for.
fn message_against(s: &Searched, p: &Patch, trailer: &str) -> Option<(String, String)> {
    let before = format!("{}\n\n{}", s.title, crate::submit::summary_of(&format!("\n{}", s.summary), trailer)).trim_end().to_string();
    let wip = crate::submit::wip_title(p.title()).title;
    let title = match in_work(&s.title) {
        true => wip,
        false => wip.strip_prefix("WIP: ").unwrap_or(&wip).to_string(),
    };
    let after = format!("{title}\n\n{}", crate::submit::summary_of(&p.message, trailer)).trim_end().to_string();
    (before != after).then_some((before, after))
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
    /// How many times the statuses were read, or a submit or a pull ended:
    /// what the Phabricator line read before is read again once shown.
    round: u64,
    /// The latest diff and the comments of each revision the line was
    /// drawn for, by link, as last read; and the round each was last read
    /// in, whether that worked or not.
    latest: HashMap<String, Latest>,
    tried: HashMap<String, u64>,
    /// The revision whose latest diff and comments are being read: its
    /// link.
    reading_latest: Option<String>,
    /// What differs in the code between a revision's latest diff and a
    /// version of its patch, by what it was worked out from
    /// (`compare_key`); and the one being worked out.
    compared: HashMap<String, Code>,
    comparing: Option<String>,
    /// The patches whose line shows what differs under it, by change.
    unfolded: HashSet<String>,
    /// How many times what the line may say changed: the middle's rows
    /// are built again each time.
    seq: u64,
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
    /// way, nor while the editor does not read Phabricator. What the
    /// Phabricator line read before is read again with them.
    pub(super) fn check_statuses(&mut self) {
        if self.statuses.reading || !self.reads_phabricator() {
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

    /// Right after a submit: the database read again, for the link of a new
    /// revision, and the statuses with it, at once, or once the read under
    /// way is done.
    pub(super) fn statuses_again(&mut self) {
        self.reload();
        self.statuses.stale = true;
        self.check_statuses();
        self.check_line();
    }

    /// Right after a pull: the latest diff and the comments of the patch
    /// shown read again, those pulled in the database now.
    pub(super) fn line_again(&mut self) {
        self.statuses.round += 1;
        self.check_line();
    }

    /// What a read of the statuses came to: those read, each revision read
    /// that Phabricator did not give known no more, and known to be missing;
    /// or, failed, those known kept, and why said, unless the last read
    /// failed too. A submit ended meanwhile, they are read again at once.
    /// Then what the Phabricator line needs next.
    pub(super) fn statuses_read(&mut self, read: std::result::Result<HashMap<String, Searched>, String>) {
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
                self.statuses.seq += 1;
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

    /// Whether what Phabricator has is being read, or compared with the
    /// patch shown.
    pub(super) fn reading_phabricator(&self) -> bool {
        self.statuses.reading || self.statuses.reading_latest.is_some() || self.statuses.comparing.is_some()
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

    /// What the Phabricator line of the patch shown needs, started away
    /// from the screen when it is due: its revision's latest diff and
    /// comments, read once each round, once its search has found its PHID;
    /// then what differs in the code, worked out once for each version of
    /// the patch, on the patches below it. One read and one comparison at a
    /// time. Not while the editor does not read Phabricator.
    pub(super) fn check_line(&mut self) {
        if !self.reads_phabricator() {
            return;
        }
        let Some(id) = self.patch_id().filter(|_| !self.on_stack) else { return };
        let p = self.db.patch(&id);
        let Some((d, link)) = crate::phab::revision_of(&p.message) else { return };
        let round = self.statuses.round;
        if self.statuses.reading_latest.is_none()
            && self.statuses.tried.get(&link) != Some(&round)
            && let Some(s) = self.statuses.known.get(&link)
        {
            self.statuses.tried.insert(link.clone(), round);
            self.statuses.reading_latest = Some(link.clone());
            let had = self.statuses.latest.get(&link).map(|l| (l.diff, l.raw.clone()));
            let (db, phid, at) = (self.path.clone(), s.phid.clone(), link.clone());
            self.spawn(move || {
                let read = crate::phab::latest(&db, &d, &phid, &at, had);
                Done::Latest(at, read)
            });
        }
        if self.statuses.comparing.is_none()
            && let Some(key) = self.compare_key(p, &link)
            && !self.statuses.compared.contains_key(&key)
            && let Some(job) = self.compare_job(p, &link)
        {
            self.statuses.comparing = Some(key.clone());
            self.spawn(move || Done::Compared(key, compare(job)));
        }
    }

    /// What a read of revision `link`'s latest diff and comments came to:
    /// kept; or, failed, what was known kept as it was, nothing said, as
    /// the statuses' reads say what is wrong. Then what the line needs next.
    pub(super) fn latest_read(&mut self, link: String, read: std::result::Result<Latest, String>) {
        self.statuses.reading_latest = None;
        if let Ok(l) = read {
            self.statuses.latest.insert(link, l);
            self.statuses.seq += 1;
        }
        self.check_line();
    }

    /// What differs in the code, worked out from `key`, kept; then what the
    /// line needs next.
    pub(super) fn code_compared(&mut self, key: String, code: Code) {
        self.statuses.comparing = None;
        self.statuses.compared.insert(key, code);
        self.statuses.seq += 1;
        self.check_line();
    }

    /// Whether what differs between patch `p` and its revision at `link`
    /// is shown under the Phabricator line: Space showed it, and something
    /// differs.
    fn unfolded(&self, p: &Patch, link: &str) -> bool {
        self.statuses.unfolded.contains(&p.change) && self.differs(p, link).is_some_and(|x| !x.none())
    }

    /// What differs between patch `p` and its revision at `link`, once the
    /// revision's search, its latest diff and the comparison of its code
    /// with this version are known.
    fn differs(&self, p: &Patch, link: &str) -> Option<Differs<'_>> {
        let s = self.statuses.known.get(link)?;
        let code = self.statuses.compared.get(&self.compare_key(p, link)?)?;
        Some(Differs {
            code: match code {
                Code::Same => None,
                Code::Differs(code, said) => Some((code, said)),
            },
            message: message_against(s, p, &self.cfg.reviews.trailer),
        })
    }

    /// The Phabricator line of patch `p`, of revision `d` at `link`, `x`
    /// what differs, if it is known: the revision; its status, by name;
    /// whether its latest diff, title and summary are this version's; how
    /// many of its comments are not pulled yet. What is not known yet is
    /// `…`, once for those in a row; a revision the search did not find,
    /// said so, and nothing else.
    fn line_said(&self, p: &Patch, d: &str, link: &str, x: Option<&Differs>) -> String {
        if self.statuses.missing.contains(link) && !self.statuses.known.contains_key(link) {
            return format!("Phabricator: {d} · not found on Phabricator");
        }
        let status = self.statuses.known.get(link).map(|s| s.status.name().to_string());
        let sync = x.map(|x| x.said().to_string());
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
    /// narrower; then, Space having shown it, what differs. None when the
    /// patch names no revision, or the editor does not read Phabricator.
    pub(super) fn line_rows(&self, p: &Patch, w: usize) -> Vec<Row> {
        if !self.reads_phabricator() {
            return Vec::new();
        }
        let Some((d, link)) = crate::phab::revision_of(&p.message) else { return Vec::new() };
        let x = self.differs(p, &link);
        let mut rows: Vec<Row> = fold_line(&self.line_said(p, &d, &link, x.as_ref()), w)
            .into_iter()
            .enumerate()
            .map(|(k, text)| Row { cont: k > 0, ..row(Kind::Meta, text) })
            .collect();
        if self.statuses.unfolded.contains(&p.change)
            && let Some(x) = x.filter(|x| !x.none())
        {
            rows.extend(self.differs_rows(p, &d, &x, w));
        }
        for r in &mut rows {
            r.phab = true;
        }
        rows
    }

    /// What differs, `x`, between patch `p`, the one picked, and its
    /// revision `d`, as rows `w` wide under the Phabricator line, as a
    /// submit's review shows them: the title and summary, Phabricator's then
    /// this version's, compared where their lines wrap aside, or each
    /// version's lines when only where they wrap differs (`message_rows`);
    /// then the code, each file as the latest diff leaves it, then as this
    /// version does, both on the patches below it. An empty row after, over
    /// the message.
    fn differs_rows(&self, p: &Patch, d: &str, x: &Differs, w: usize) -> Vec<Row> {
        let mut rows = Vec::new();
        let n = self.sel + 1;
        if let Some((before, after)) = &x.message {
            rows.push(row(Kind::File, "the title and summary: Phabricator's, then this version's"));
            message_rows(&mut rows, before, after, w);
        }
        if let Some((code, said)) = x.code {
            if !rows.is_empty() {
                rows.push(row(Kind::Blank, ""));
            }
            rows.push(row(Kind::File, "the code: Phabricator's, then this version's"));
            let said = said.clone().unwrap_or_else(|| format!("Each file as the latest diff of {d} leaves it, then as this version of patch {n} leaves it, {}.", below(n)));
            said_rows(&mut rows, Kind::Meta, &said, w);
            rows.push(row(Kind::Blank, ""));
            rows.extend(code_rows(&p.change, code, w));
        }
        if rows.last().is_none_or(|r| r.kind != Kind::Blank) {
            rows.push(row(Kind::Blank, ""));
        }
        rows
    }

    /// Space on the Phabricator line, or on what it shows under it: what
    /// differs shown under the line, or hidden again. With nothing that
    /// differs, the status line says that Phabricator has this exact
    /// version; with what differs not known yet, it says so.
    pub(super) fn toggle_line(&mut self) {
        let Some(p) = self.shown_patch().cloned() else { return };
        let Some((d, link)) = crate::phab::revision_of(&p.message) else { return };
        let n = self.sel + 1;
        // Shown: hidden again. Shown once, and nothing differs since: as
        // if it were not.
        let shown = self.unfolded(&p, &link);
        if self.statuses.unfolded.remove(&p.change) && shown {
            self.statuses.seq += 1;
            return;
        }
        if self.statuses.missing.contains(&link) && !self.statuses.known.contains_key(&link) {
            self.status = format!("Phabricator has no revision {d}, so nothing of it is compared with patch {n}.");
            return;
        }
        match self.differs(&p, &link).map(|x| x.none()) {
            None => {
                self.status = format!("What differs between patch {n} and {d} on Phabricator is not known yet: the line says it once {d} is read and compared.");
            }
            Some(true) => {
                self.status = format!(
                    "Phabricator has this exact version of patch {n}: the latest diff of {d} does what this version does, and the title and summary of {d} are those of this version."
                );
            }
            Some(false) => {
                self.statuses.unfolded.insert(p.change.clone());
                self.statuses.seq += 1;
            }
        }
    }

    /// What `?` says on the Phabricator line, or on what it shows under it:
    /// what the line says, and what Space does there.
    pub(super) fn line_help(&self) -> Vec<String> {
        let Some(p) = self.shown_patch() else { return Vec::new() };
        let Some((d, link)) = crate::phab::revision_of(&p.message) else { return Vec::new() };
        let n = self.sel + 1;
        let mut out = vec![format!(
            "This line says what Phabricator has of patch {n}: its revision, {d}; the status of {d}, by its name; whether the latest diff of {d}, and its title and summary, are those of this version (in sync), the diff by what it does, the title and summary exactly as a submit sends them, where their lines wrap too; and how many of the comments of {d} are not pulled yet. What this line says is read from Phabricator, away from the screen, for the patch the middle shows: every three minutes, and again after a submit or a pull. Where something is not read yet, the line says … in its place."
        )];
        let unfolded = self.unfolded(p, &link);
        let missing = self.statuses.missing.contains(&link) && !self.statuses.known.contains_key(&link);
        out.push(match self.differs(p, &link) {
            _ if missing => format!("Phabricator has no revision {d}, so nothing of it is compared with this patch, and Space only says so."),
            Some(_) if unfolded => "Under this line is what differs: press Space to hide it.".into(),
            Some(x) if x.none() => "Phabricator has this exact version: Space says so, and shows nothing under this line.".into(),
            // What Space shows, of what differs.
            Some(x) => {
                let message = "the title and summary, Phabricator's then this version's".to_string();
                let code = match x.code {
                    Some((_, Some(_))) => format!("the latest diff of {d} against this version's diff, as text, since the files could not be read"),
                    _ => format!("each file as the latest diff of {d} leaves it, then as this version leaves it, {}", below(n)),
                };
                let shown = match (x.message.is_some(), x.code.is_some()) {
                    (true, true) => format!("{message}; and {code}"),
                    (true, false) => message,
                    _ => code,
                };
                format!("Press Space to show what differs under this line: {shown}. Space again hides it.")
            }
            None => "Press Space to show what differs under this line, once it is read and compared.".into(),
        });
        out.push("Press Enter for all that can be done here.".into());
        out
    }

    /// What the bottom bar says on the Phabricator line, or on what it
    /// shows under it.
    pub(super) fn line_hint(&self) -> String {
        let unfolded = self.shown_patch().is_some_and(|p| crate::phab::revision_of(&p.message).is_some_and(|(_, link)| self.unfolded(p, &link)));
        let space = if unfolded { "space: hide what differs" } else { "space: show what differs" };
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
    fn a_title_is_in_work_as_moz_phab_reads_it() {
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
        let searched = |title: &str, summary: &str| Searched { status: Status::NeedsReview, title: title.into(), summary: summary.into(), phid: String::new() };
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
        let differs = |c: bool, m: bool| Differs { code: c.then_some((&code.0, &code.1)), message: if m { message.clone() } else { None } };
        assert_eq!(
            [differs(false, false).said(), differs(true, false).said(), differs(false, true).said(), differs(true, true).said()],
            ["in sync", "the diff differs", "the title or summary differs", "the diff and the title or summary differ"]
        );
        assert!(differs(false, false).none() && !differs(true, false).none());
    }
}
