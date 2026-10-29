//! The status on Phabricator of each patch's revision, which the patches
//! pane shows after the patch's number: read away from the screen, in one
//! search of all the revisions the stack's patches name, as the stack is
//! shown, again as those revisions change and right after a submit, and
//! every few minutes while the editor runs. Kept in memory as long as it
//! runs, nothing written; a read that fails leaves them as they were. Not
//! while reviews come from nowhere, nor in the tour. Built with the
//! `moz-phab` feature.

use super::*;
use crate::phab::Status;

/// How long the statuses go between reads while the stack's revisions are
/// the same: a few minutes, as a try push followed does between looks.
const EVERY: Duration = Duration::from_secs(180);

/// Whether the statuses of revisions `named` are due a read at `now`, as
/// `s` says they were read: never, or of other revisions; or a submit
/// ended since; or a few minutes ago.
fn due(s: &Statuses, named: &[(String, String)], now: Instant) -> bool {
    s.stale || s.asked.as_ref().is_none_or(|(asked, when)| asked != named || now.duration_since(*when) >= EVERY)
}

/// The statuses of the revisions, as last read, and their reads.
#[derive(Default)]
pub(super) struct Statuses {
    /// Each revision's, by its link.
    known: HashMap<String, Status>,
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

    /// The statuses of the revisions the current stack's patches name, read
    /// away from the screen, when they are due: never read, or for other
    /// revisions (the stack shown, or its patches naming others), a submit
    /// ended since, or a few minutes gone by. Not while a read is under
    /// way, while reviews come from nowhere, nor in the tour; nor from a
    /// process that keeps nothing, as the tests' own, without a stand-in.
    pub(super) fn check_statuses(&mut self) {
        if self.statuses.reading || self.cfg.reviews.source == crate::config::Source::None {
            return;
        }
        #[cfg(feature = "demo")]
        if crate::memory::is_file(&self.path) {
            return;
        }
        if !crate::phab::statuses_read(&self.path) {
            return;
        }
        let named = self.revisions_named();
        if named.is_empty() || !due(&self.statuses, &named, Instant::now()) {
            return;
        }
        self.statuses.asked = Some((named.clone(), Instant::now()));
        self.statuses.reading = true;
        self.statuses.stale = false;
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
    }

    /// What a read of the statuses came to: those read, each revision read
    /// that Phabricator did not give known no more; or, failed, those known
    /// kept, and why said, unless the last read failed too. A submit ended
    /// meanwhile, they are read again at once.
    pub(super) fn statuses_read(&mut self, read: std::result::Result<HashMap<String, Status>, String>) {
        self.statuses.reading = false;
        let asked = self.statuses.asked.as_ref().map(|(a, _)| a.clone()).unwrap_or_default();
        match read {
            Ok(found) => {
                for (_, link) in &asked {
                    self.statuses.known.remove(link);
                }
                self.statuses.known.extend(found);
                self.statuses.failed = false;
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
    }

    /// The status of patch `id`'s revision, as last read: none when it
    /// names none, or its status is not known yet.
    pub(super) fn revision_status(&self, id: &str) -> Option<Status> {
        if self.statuses.known.is_empty() {
            return None;
        }
        let (_, link) = crate::phab::revision_of(&self.db.patch(id).message)?;
        self.statuses.known.get(&link).copied()
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
}
