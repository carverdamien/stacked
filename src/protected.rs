//! The patches that should not change, because of their revision's status
//! on Phabricator as it was last fetched: Accepted, as a change of the
//! patch needs review again, or Landed, as it is in the tree already
//! (`protecting`, the one place that says which). A stack written to hold
//! another version of such a patch, its diff or its message differing,
//! changes it (`changes`); dropped, moved, or left as it is while patches
//! change around it, it does not change. The database refuses such a write
//! unless the patch was confirmed for the one operation that writes it
//! (`Db::confirm`), its refusal an error of its own kind that names the
//! patches (`Refused`): the editor asks first in a dialog where `accept` is
//! typed, the command line on its terminal. Without the `moz-phab` feature,
//! or with reviews coming from nowhere, no patch is protected.

use std::collections::HashSet;

use crate::db::{Db, Patch, Stack};

/// What protects a patch: its revision, `D123`, and the link its trailer
/// gives; that revision's status as the last read of Phabricator stored it,
/// as the database writes it (`accepted`); and when that was fetched, in
/// seconds since 1970 (`phab::fetched_at`).
#[derive(Debug, Clone, PartialEq)]
pub struct Guard {
    pub d: String,
    pub link: String,
    pub status: String,
    pub fetched: Option<i64>,
}

impl Guard {
    /// Its status as the dialog names it, after the patch and its revision
    /// (`is Accepted on Phabricator`), and why the patch should not change
    /// (`a change needs review again`).
    pub fn said(&self) -> (&'static str, &'static str) {
        protecting(&self.status).unwrap_or(("should not change", "its status says so"))
    }
}

/// A patch that should not change that a write would change, or that an
/// edit by hand may change: the version the stack holds, by its id; its
/// place in the stack, from 0; and what protects it.
#[derive(Debug, Clone, PartialEq)]
pub struct Changed {
    pub id: String,
    pub at: usize,
    pub guard: Guard,
}

impl Changed {
    /// Its line, as the dialog and the command line say it: `Patch 3, D123,
    /// is Accepted on Phabricator: a change needs review again.`
    pub fn line(&self) -> String {
        let (is, why) = self.guard.said();
        format!("Patch {}, {}, {is}: {why}.", self.at + 1, self.guard.d)
    }
}

/// A write of stack `stack` refused, as it would change `patches`, which
/// should not change, none of them confirmed for it, nothing written: the
/// error of its own kind that `Db::edit_confirmed` gives back. Or, asked
/// before $EDITOR opens, the patches an edit by hand may change.
#[derive(Debug, Clone, PartialEq)]
pub struct Refused {
    pub stack: String,
    pub patches: Vec<Changed>,
}

/// What status `status` of a patch's revision, as the database writes it
/// (`accepted`), says of the patch when the patch should not change: how
/// the patch's line names it, after the patch and its revision, and why
/// the patch should not change. None for every other status, whose patches
/// change as any do. The one place that says which statuses protect a
/// patch.
pub fn protecting(status: &str) -> Option<(&'static str, &'static str)> {
    match status {
        "accepted" => Some(("is Accepted on Phabricator", "a change needs review again")),
        "published" => Some(("is Landed", "it is in the tree already")),
        _ => None,
    }
}

/// What protects patch `p`, if something does: the revision its trailer
/// names (`phab::revision_of`), whose status as last fetched is one that
/// protects it (`protecting`). None without the `moz-phab` feature, with
/// reviews coming from nowhere (`reviews.source = "none"`), for a patch
/// that names no revision, and for one whose revision nothing is fetched of
/// yet.
pub fn guard_of(db: &Db, p: &Patch) -> Option<Guard> {
    #[cfg(feature = "moz-phab")]
    {
        if crate::config::get().reviews.source == crate::config::Source::None {
            return None;
        }
        let (d, link) = crate::phab::revision_of(&p.message)?;
        let (state, _) = db.revision_state(&link)?;
        protecting(&state.status)?;
        let status = state.status.clone();
        let fetched = crate::phab::fetched_at(db, &link);
        Some(Guard { d, link, status, fetched })
    }
    #[cfg(not(feature = "moz-phab"))]
    {
        let _ = (db, p);
        None
    }
}

/// Whether version `now` of a patch changes version `was` of it: their
/// diffs or their messages differ. Another author, another date, another
/// place in the stack are no change.
pub fn differs(was: &Patch, now: &Patch) -> bool {
    was.diff != now.diff || was.message != now.message
}

/// The patches of a stack holding `was` that should not change, that its
/// holding `now` instead would change, by their versions and places in
/// `was`, in order: each that `now` does not hold, whose change `now` holds
/// in another version that differs from it (`differs`). One that `now`
/// holds as it is, anywhere, does not change, nor does one it does not
/// hold at all, dropped. Only the patches the write replaces are looked at,
/// so that a write that replaces none costs a look at the ids alone.
pub fn changes(db: &Db, was: &[String], now: &[String]) -> Vec<Changed> {
    let kept: HashSet<&str> = now.iter().map(String::as_str).collect();
    let mut out = Vec::new();
    for (at, id) in was.iter().enumerate() {
        if kept.contains(id.as_str()) {
            continue;
        }
        let old = db.patch(id);
        let Some(new) = now.iter().map(|n| db.patch(n)).find(|n| n.change == old.change) else { continue };
        if !differs(old, new) {
            continue;
        }
        if let Some(guard) = guard_of(db, old) {
            out.push(Changed { id: id.clone(), at, guard });
        }
    }
    out
}

/// The patches of the stack `name` is on that should not change, of those
/// an edit by hand may change: each that holds the change of one of the
/// patches at `places` of `from`, the stack the edit starts from (the
/// stack as it is, or as landed work would make it), by its version and
/// place in the stack as it is. The edit not made yet, each may change, or
/// not.
pub fn of_edit(db: &Db, name: &str, from: &Stack, places: std::ops::Range<usize>) -> Vec<Changed> {
    let Ok(s) = db.stack(name) else { return vec![] };
    // To the top, past its end.
    let end = places.end.min(from.patches.len());
    let edited: HashSet<&str> = from.patches[places.start.min(end)..end].iter().map(|id| db.patch(id).change.as_str()).collect();
    s.patches
        .iter()
        .enumerate()
        .filter(|(_, id)| edited.contains(db.patch(id).change.as_str()))
        .filter_map(|(at, id)| Some(Changed { id: id.clone(), at, guard: guard_of(db, db.patch(id))? }))
        .collect()
}

/// Of the patches that should not change that a stack holding `was` would
/// change holding `now`, those a pull brings to what Phabricator has of
/// them, by their versions in `was`: the new version's diff is its
/// revision's latest, as the read stored it raw, or the patch's own, and
/// its message is the revision's title and summary as a submit sends them
/// (`submit::message_against`), or the patch's own. A pull takes what
/// Phabricator has, so that those change with no one asked; the others it
/// changes, a patch it made again on the pulled versions under it say, are
/// asked about as any.
pub fn from_phabricator(db: &Db, was: &[String], now: &[String]) -> Vec<String> {
    #[cfg(feature = "moz-phab")]
    {
        let trailer = &crate::config::get().reviews.trailer;
        changes(db, was, now)
            .into_iter()
            .filter(|c| {
                let old = db.patch(&c.id);
                let Some(new) = now.iter().map(|n| db.patch(n)).find(|n| n.change == old.change) else { return false };
                let Some((state, _)) = db.revision_state(&c.guard.link) else { return false };
                let raw = (state.diff != 0).then(|| db.raw_diff(&c.guard.link, state.diff)).flatten();
                let diff = new.diff == old.diff || raw == Some(new.diff.as_str());
                let message = new.message == old.message || crate::submit::message_against(&state.title, &state.summary, new, trailer).is_none();
                diff && message
            })
            .map(|c| c.id)
            .collect()
    }
    #[cfg(not(feature = "moz-phab"))]
    {
        let _ = (db, was, now);
        vec![]
    }
}

/// How long before `now` it was that `then` was, as the Phabricator line
/// says how long ago what it shows was fetched, without its `fetched`:
/// `11 minutes ago`, `just now`, `yesterday`.
fn ago(then: i64, now: i64) -> String {
    #[cfg(feature = "moz-phab")]
    {
        let said = crate::phab::age_said(then, now);
        said.strip_prefix("fetched ").map(String::from).unwrap_or(said)
    }
    #[cfg(not(feature = "moz-phab"))]
    {
        let _ = (then, now);
        String::new()
    }
}

/// `items`, the last after `and`: `3, 4 and 5`.
fn and(items: &[String]) -> String {
    match items.split_last() {
        None => String::new(),
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
    }
}

impl Refused {
    /// The patches it names, by the versions the stack holds: what typing
    /// `accept` confirms.
    pub fn ids(&self) -> Vec<String> {
        self.patches.iter().map(|c| c.id.clone()).collect()
    }

    /// Whether it names one patch alone.
    fn one(&self) -> bool {
        self.patches.len() == 1
    }

    /// The patches, by their numbers: `patch 3`, `patches 3 and 5`.
    fn named(&self) -> String {
        let numbers: Vec<String> = self.patches.iter().map(|c| (c.at + 1).to_string()).collect();
        match self.one() {
            true => format!("patch {}", and(&numbers)),
            false => format!("patches {}", and(&numbers)),
        }
    }

    /// What the dialog says on its top: that the operation changes them,
    /// or, asked before $EDITOR opens, that the edit may (`hand`).
    pub fn title(&self, hand: bool) -> String {
        let n = self.patches.len();
        let what = if self.one() { "a patch".to_string() } else { format!("{n} patches") };
        match hand {
            true => format!("this edit may change {what} that should not change"),
            false => format!("this changes {what} that should not change"),
        }
    }

    /// A line for each patch, as the dialog lists them.
    pub fn lines(&self) -> Vec<String> {
        self.patches.iter().map(Changed::line).collect()
    }

    /// How long before `now` what protects them was fetched: `That status
    /// was fetched from Phabricator 11 minutes ago.`; of several, fetched
    /// at once or not, each revision's then.
    pub fn fetched(&self, now: i64) -> String {
        let ages: Vec<(String, String)> = self.patches.iter().filter_map(|c| Some((c.guard.d.clone(), ago(c.guard.fetched?, now)))).collect();
        match ages.as_slice() {
            [] => String::new(),
            [(_, age)] if self.one() => format!("That status was fetched from Phabricator {age}."),
            [(_, age), rest @ ..] if rest.iter().all(|(_, a)| a == age) => format!("Those statuses were fetched from Phabricator {age}."),
            all => {
                let each: Vec<String> = all.iter().map(|(d, age)| format!("{d}'s {age}")).collect();
                format!("Those statuses were fetched from Phabricator: {}.", and(&each))
            }
        }
    }

    /// What the dialog says to type, over its field.
    pub fn type_said(&self) -> String {
        match self.one() {
            true => format!("Type accept to change {} anyway.", self.named()),
            false => "Type accept to change them anyway.".into(),
        }
    }

    /// The dialog's button that changes them: `Change it`, `Change them`.
    pub fn change_label(&self) -> &'static str {
        if self.one() { "Change it" } else { "Change them" }
    }

    /// Each patch said in a few words, for a line of its own: `patch 3
    /// (D123) is Accepted on Phabricator, and patch 5 (D125) is Landed`.
    fn statuses(&self) -> String {
        let each: Vec<String> = self.patches.iter().map(|c| format!("patch {} ({}) {}", c.at + 1, c.guard.d, c.guard.said().0)).collect();
        and(&each)
    }

    /// The refusal said in a line, the error's message: which patches
    /// should not change, and why each.
    pub fn said(&self) -> String {
        let why: Vec<String> = self
            .patches
            .iter()
            .map(|c| {
                let (is, why) = c.guard.said();
                format!("{} {is}, and {why}", c.guard.d)
            })
            .collect();
        format!("{} should not change, so nothing changed: {}", self.named(), why.join("; "))
    }

    /// As `said`, from a command run with no terminal to ask on: and that
    /// on one, it asks.
    pub fn said_off_terminal(&self) -> String {
        let them = if self.one() { self.named() } else { "them".into() };
        format!("{}; on a terminal, stacked asks you to type accept to change {them} anyway", self.said())
    }

    /// What the bottom line says once you did not type accept, nothing
    /// written: of an edit by hand asked before $EDITOR opens (`hand`), that
    /// it was not made.
    pub fn not_typed(&self, hand: bool) -> String {
        let done = if hand { "the edit was not made" } else { "nothing changed" };
        format!("{done}, as accept was not typed: {}", self.statuses())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Patch `at`, of revision `d`, `status` as fetched at `fetched`.
    #[cfg(feature = "moz-phab")]
    fn changed(at: usize, d: &str, status: &str, fetched: i64) -> Changed {
        Changed {
            id: format!("id{at}"),
            at,
            guard: Guard { d: d.into(), link: format!("https://phabricator.example.com/{d}"), status: status.into(), fetched: Some(fetched) },
        }
    }

    #[test]
    fn accepted_and_landed_alone_protect_a_patch() {
        assert_eq!(protecting("accepted"), Some(("is Accepted on Phabricator", "a change needs review again")));
        assert_eq!(protecting("published"), Some(("is Landed", "it is in the tree already")));
        for other in ["draft", "changes-planned", "needs-review", "needs-revision", "abandoned", "unknown", ""] {
            assert_eq!(protecting(other), None, "{other}");
        }
    }

    #[cfg(feature = "moz-phab")]
    #[test]
    fn a_refusal_names_each_patch_its_status_what_that_means_and_how_long_ago_it_was_fetched() {
        let now = 100_000;
        let one = Refused { stack: "s".into(), patches: vec![changed(2, "D123", "accepted", now - 11 * 60)] };
        assert_eq!(one.title(false), "this changes a patch that should not change");
        assert_eq!(one.title(true), "this edit may change a patch that should not change");
        assert_eq!(one.lines(), ["Patch 3, D123, is Accepted on Phabricator: a change needs review again."]);
        assert_eq!(one.fetched(now), "That status was fetched from Phabricator 11 minutes ago.");
        assert_eq!(one.type_said(), "Type accept to change patch 3 anyway.");
        assert_eq!(one.change_label(), "Change it");
        assert_eq!(one.said(), "patch 3 should not change, so nothing changed: D123 is Accepted on Phabricator, and a change needs review again");
        assert_eq!(
            one.said_off_terminal(),
            "patch 3 should not change, so nothing changed: D123 is Accepted on Phabricator, and a change needs review again; on a terminal, stacked asks you to type accept to change patch 3 anyway"
        );
        assert_eq!(one.not_typed(false), "nothing changed, as accept was not typed: patch 3 (D123) is Accepted on Phabricator");
        assert_eq!(one.not_typed(true), "the edit was not made, as accept was not typed: patch 3 (D123) is Accepted on Phabricator");
        let two = Refused {
            stack: "s".into(),
            patches: vec![changed(2, "D123", "accepted", now - 11 * 60), changed(4, "D125", "published", now - 11 * 60)],
        };
        assert_eq!(two.title(false), "this changes 2 patches that should not change");
        assert_eq!(two.lines(), ["Patch 3, D123, is Accepted on Phabricator: a change needs review again.", "Patch 5, D125, is Landed: it is in the tree already."]);
        assert_eq!(two.fetched(now), "Those statuses were fetched from Phabricator 11 minutes ago.");
        assert_eq!(two.type_said(), "Type accept to change them anyway.");
        assert_eq!(two.change_label(), "Change them");
        assert_eq!(
            two.said(),
            "patches 3 and 5 should not change, so nothing changed: D123 is Accepted on Phabricator, and a change needs review again; D125 is Landed, and it is in the tree already"
        );
        assert_eq!(two.not_typed(false), "nothing changed, as accept was not typed: patch 3 (D123) is Accepted on Phabricator and patch 5 (D125) is Landed");
        assert_eq!(two.ids(), ["id2", "id4"]);
        // Fetched at other times: each revision's.
        let apart = Refused {
            stack: "s".into(),
            patches: vec![changed(2, "D123", "accepted", now - 11 * 60), changed(4, "D125", "published", now - 30 * 3600)],
        };
        assert_eq!(apart.fetched(now), "Those statuses were fetched from Phabricator: D123's 11 minutes ago and D125's yesterday.");
    }
}
