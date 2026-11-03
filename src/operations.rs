//! The operations of a change of the stack, each an item of its review: a
//! line for each patch of the stack the change makes (`add`, `modify`,
//! `reword` when its message alone changed, `move`, or `noop` as it was), a
//! line for each patch it removes (`remove`), and one for the base when it
//! rebases. Each is accepted, rejected or to review, as the review's marks
//! hold it (`op CHANGE`, by its patch's change; `op base`), but a `noop`,
//! which changes nothing and needs no mark.
//!
//! The stack the operations leave (`Ops::leave`) holds each patch as the
//! change leaves it where its operation stands (accepted, or to review),
//! else as it was, where it was. An operation B needs an operation A when
//! B's patch does not apply, in the stack as the operations stand, without
//! A: told by the check that applies a stack's patches
//! (`check::first_failure`), as the operations stand, and nothing kept but
//! what that check keeps of each stack it applied. Rejecting an operation
//! is refused while one that stands needs it, and accepting one while one
//! it needs is rejected (`Ops::reject`, `Ops::accept`), so that the stack
//! as they stand always applies. An operation whose patch as it was no
//! longer applies on what the others leave goes with the operation that
//! makes it so: the two are rejected, or accepted, together, once asked. A
//! change with no operation to mark, every patch and the base left as they
//! were, is marked as a whole (`Ops::whole`), as every change once was.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::db::{Db, Patch, Result, Run, Stack};
use crate::held::Held;

/// What an operation does to the stack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The stack rebased on another base.
    Base,
    /// A patch added.
    Add,
    /// A patch's new version.
    Modify,
    /// A patch's new version, its message alone changed.
    Reword,
    /// A patch moved, as it was.
    Move,
    /// A patch removed.
    Remove,
    /// A patch left as it was: no operation, and no mark.
    Noop,
}

impl Kind {
    /// It in a word, as the review's list says it: `modify`, `remove`.
    pub fn word(self) -> &'static str {
        match self {
            Kind::Base => "base",
            Kind::Add => "add",
            Kind::Modify => "modify",
            Kind::Reword => "reword",
            Kind::Move => "move",
            Kind::Remove => "remove",
            Kind::Noop => "noop",
        }
    }
}

/// An operation of a change of the stack.
#[derive(Clone, Debug, PartialEq)]
pub struct Op {
    pub kind: Kind,
    /// As the review's marks name it: `op CHANGE`, `op base`.
    pub key: String,
    /// Its entry among the lines of the stack diff, what changed in each
    /// patch aside: its line of the review's list.
    pub entry: usize,
    /// The place it names, from 1: in the stack the change makes, or, for a
    /// patch removed, in the stack before; 0 for the base.
    pub at: usize,
    /// A new version, or a new message, that the change moves too.
    pub moved: bool,
}

impl Op {
    /// Whether it is marked: all but a patch left as it was.
    pub fn marked(&self) -> bool {
        self.kind != Kind::Noop
    }
}

/// The mark, in a review, of the base's operation.
pub const BASE: &str = "op base";

/// The mark, in a review, of the operation on the patch of `change`.
pub fn key_of(change: &str) -> String {
    format!("op {change}")
}

/// Whether `key` is a review's mark of an operation.
pub fn is_key(key: &str) -> bool {
    key.starts_with("op ")
}

/// Where a patch goes in the stacks the operations leave, the order of the
/// two stacks kept: one both hold where neither moves it (its operation
/// gives its version); one the stack before holds where the change takes
/// it away, removed or moved elsewhere (there while its operation is
/// rejected); one the stack after holds where the change puts it, added or
/// moved there (there while its operation stands).
#[derive(Clone, Debug, PartialEq)]
enum Slot {
    Kept { key: String, old: String, new: String },
    Was { key: String, old: String },
    Is { key: String, new: String },
}

/// A patch of a stack the operations leave: its id; the operation whose
/// patch it is; whether as that operation leaves it, else as it was; and
/// its slot.
#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    pub id: String,
    pub key: String,
    pub new: bool,
    slot: usize,
}

/// The operations of a change, from a stack to the stack it makes.
#[derive(Clone, Debug, PartialEq)]
pub struct Ops {
    /// Each of them, in the order of the stack diff's lines.
    pub list: Vec<Op>,
    slots: Vec<Slot>,
    /// The base before, and after.
    bases: (Option<String>, Option<String>),
    /// The stack's name and repository, to check the stacks they leave.
    name: String,
    repo: Option<String>,
}

/// The operations from stack `before` to `after`, as the stack diff says
/// them (`history::stacks_diff`), each on the line it has there: a patch is
/// known across its versions by its change; one out of the order the others
/// keep is moved.
pub fn of(db: &Db, before: &Stack, after: &Stack) -> Ops {
    let mut list: Vec<Op> = Vec::new();
    let mut slots = Vec::new();
    let mut entry = 0;
    let op = |list: &mut Vec<Op>, entry: &mut usize, kind: Kind, key: String, at: usize, moved: bool| {
        list.push(Op { kind, key, entry: *entry, at, moved });
        *entry += 1;
    };
    if before.base != after.base {
        op(&mut list, &mut entry, Kind::Base, BASE.into(), 0, false);
    }
    // A line of its own, for no operation: a review's stacks share theirs.
    if before.repo != after.repo {
        entry += 1;
    }
    let old: Vec<&Patch> = before.patches.iter().map(|id| db.patch(id)).collect();
    let new: Vec<&Patch> = after.patches.iter().map(|id| db.patch(id)).collect();
    let olds: Vec<&str> = old.iter().map(|p| p.change.as_str()).collect();
    let news: Vec<&str> = new.iter().map(|p| p.change.as_str()).collect();
    // A new version, its message alone changed: reworded, as the stack
    // diff says it.
    let version = |o: &Patch, p: &Patch| match (o.id == p.id, o.message != p.message && crate::ops::same_effect(&o.diff, &p.diff)) {
        (true, _) => Kind::Noop,
        (false, true) => Kind::Reword,
        (false, false) => Kind::Modify,
    };
    for d in similar::capture_diff_slices(similar::Algorithm::Myers, &olds, &news) {
        let (tag, o, n) = d.as_tag_tuple();
        if tag == similar::DiffTag::Equal {
            for (i, j) in o.zip(n) {
                let key = key_of(news[j]);
                op(&mut list, &mut entry, version(old[i], new[j]), key.clone(), j + 1, false);
                slots.push(Slot::Kept {
                    key,
                    old: old[i].id.clone(),
                    new: new[j].id.clone(),
                });
            }
            continue;
        }
        // Removed where they were; moved, where they are now.
        for i in o {
            let key = key_of(olds[i]);
            if !news.contains(&olds[i]) {
                op(&mut list, &mut entry, Kind::Remove, key.clone(), i + 1, false);
            }
            slots.push(Slot::Was { key, old: old[i].id.clone() });
        }
        for j in n {
            let key = key_of(news[j]);
            let (kind, moved) = match olds.iter().position(|c| *c == news[j]) {
                None => (Kind::Add, false),
                Some(i) => match version(old[i], new[j]) {
                    Kind::Noop => (Kind::Move, false),
                    k => (k, true),
                },
            };
            op(&mut list, &mut entry, kind, key.clone(), j + 1, moved);
            slots.push(Slot::Is { key, new: new[j].id.clone() });
        }
    }
    // Nothing changed: the stack diff says so first, on a line of its own.
    if before.repo == after.repo && list.iter().all(|o| o.kind == Kind::Noop) {
        for o in &mut list {
            o.entry += 1;
        }
    }
    Ops {
        list,
        slots,
        bases: (before.base.clone(), after.base.clone()),
        name: before.name.clone(),
        repo: before.repo.clone(),
    }
}

/// The operations of the change work `h` of run `r` holds, from its stack
/// as it is: none for work that changes nothing of the stack, or a stack
/// gone.
pub fn of_work(db: &Db, r: &Run, h: &Held) -> Option<Ops> {
    let c = h.stack.as_ref()?;
    let now = db.stack(&r.stack).ok()?;
    let next = Stack {
        patches: c.patches.clone(),
        base: c.base.clone().or(now.base.clone()),
        ..now.clone()
    };
    Some(of(db, now, &next))
}

/// Why marking an operation alone is refused.
#[derive(Clone, Debug, PartialEq)]
pub enum Refused {
    /// Rejecting: the patch of operation `by`, which stands (or is a patch
    /// left as it was), does not apply without `on`, those rejected.
    Needed { by: String, on: Vec<String> },
    /// Accepting: the patch of operation `op` applies only with `on`, which
    /// is rejected.
    Needs { op: String, on: String },
    /// Accepting: the patch of operation `by`, which stands (or is a patch
    /// left as it was), does not apply once `on` are accepted.
    Breaks { by: String, on: Vec<String> },
    /// The patch of operation `key`, as it leaves it (`new`) or as it was,
    /// does not apply on what the others leave, and no other operation
    /// marked with it would make it apply.
    Stuck { key: String, new: bool },
    /// The stack cannot be checked: why.
    Unchecked(String),
}

/// An operation that goes with the one marked, marked with it: its key;
/// the operation whose patch as it was does not apply otherwise; and the
/// operations that make it so.
#[derive(Clone, Debug, PartialEq)]
pub struct With {
    pub key: String,
    pub because: String,
    pub by: Vec<String>,
}

impl Ops {
    /// Operation `key`.
    pub fn op(&self, key: &str) -> Option<&Op> {
        self.list.iter().find(|o| o.key == key)
    }

    /// The operation of entry `entry` of the stack diff, if it is one.
    pub fn by_entry(&self, entry: usize) -> Option<&Op> {
        self.list.iter().find(|o| o.entry == entry)
    }

    /// Those that are marked: all but the patches left as they were.
    pub fn marked(&self) -> impl Iterator<Item = &Op> {
        self.list.iter().filter(|o| o.marked())
    }

    /// How `marks` mark operation `o`: its own mark; else, as a stacked of
    /// before marked a change, the change's as a whole (`stack`). None for a
    /// patch left as it was.
    pub fn mark(&self, marks: &BTreeMap<String, bool>, o: &Op) -> Option<bool> {
        if !o.marked() {
            return None;
        }
        marks.get(&o.key).or_else(|| marks.get(&crate::held::Part::Stack.key())).copied()
    }

    /// How many `marks` accept, reject, and leave to review.
    pub fn counts(&self, marks: &BTreeMap<String, bool>) -> (usize, usize, usize) {
        let of = |m: Option<bool>| self.marked().filter(|o| self.mark(marks, o) == m).count();
        (of(Some(true)), of(Some(false)), of(None))
    }

    /// Whether the change has no operation to mark, every patch left as it
    /// was and the base too: it is marked as a whole (`stack`), as a stacked
    /// of before marked every change.
    pub fn whole(&self) -> bool {
        self.marked().next().is_none()
    }

    /// The marks the change takes: each operation's; with none, the
    /// change's as a whole.
    pub fn keys(&self) -> Vec<String> {
        match self.whole() {
            true => vec![crate::held::Part::Stack.key()],
            false => self.marked().map(|o| o.key.clone()).collect(),
        }
    }

    /// How `marks` mark the change, its marks (`keys`) counted: how many
    /// accept, reject, and leave to review.
    pub fn tally(&self, marks: &BTreeMap<String, bool>) -> (usize, usize, usize) {
        if !self.whole() {
            return self.counts(marks);
        }
        match marks.get(&crate::held::Part::Stack.key()) {
            Some(true) => (1, 0, 0),
            Some(false) => (0, 1, 0),
            None => (0, 0, 1),
        }
    }

    /// The operations that stand as `marks` mark them, by their keys: those
    /// accepted, those to review, and the patches left as they were.
    pub fn standing(&self, marks: &BTreeMap<String, bool>) -> BTreeSet<String> {
        self.list.iter().filter(|o| self.mark(marks, o) != Some(false)).map(|o| o.key.clone()).collect()
    }

    /// The stack the operations `stands` leave, the others rejected: each
    /// patch where it goes, as its operation leaves it or as it was; and the
    /// base.
    pub fn leave(&self, stands: &BTreeSet<String>) -> (Vec<Placed>, Option<String>) {
        let mut out = Vec::new();
        for (slot, s) in self.slots.iter().enumerate() {
            match s {
                Slot::Kept { key, old, new } => {
                    let as_new = old == new || stands.contains(key);
                    out.push(Placed {
                        id: if as_new { new } else { old }.clone(),
                        key: key.clone(),
                        new: as_new,
                        slot,
                    });
                }
                Slot::Was { key, old } if !stands.contains(key) => out.push(Placed {
                    id: old.clone(),
                    key: key.clone(),
                    new: false,
                    slot,
                }),
                Slot::Is { key, new } if stands.contains(key) => out.push(Placed {
                    id: new.clone(),
                    key: key.clone(),
                    new: true,
                    slot,
                }),
                _ => {}
            }
        }
        let rebased = self.list.iter().any(|o| o.kind == Kind::Base) && stands.contains(BASE);
        (out, if rebased { self.bases.1.clone() } else { self.bases.0.clone() })
    }

    /// The first patch of `left` that does not apply, each on those before
    /// it, to `base`, by place; none when they all do. Why it cannot be
    /// told, when the stack cannot be checked.
    fn fails(&self, db: &Db, left: &[Placed], base: &Option<String>) -> Result<Option<usize>> {
        let (Some(repo), Some(base)) = (&self.repo, base) else {
            let s = Stack {
                name: self.name.clone(),
                patches: vec![],
                base: base.clone(),
                repo: self.repo.clone(),
                parent: None,
                op: String::new(),
                time: 0,
            };
            crate::check::can_check(&s)?;
            return Ok(None);
        };
        let patches: Vec<&Patch> = left.iter().map(|p| db.patch(&p.id)).collect();
        crate::check::first_failure(Path::new(repo), base, &patches)
    }

    /// Whether the stack the operations `stands` leave applies: if not, the
    /// first patch that does not.
    pub fn failing(&self, db: &Db, stands: &BTreeSet<String>) -> Result<Option<Placed>> {
        let (left, base) = self.leave(stands);
        Ok(self.fails(db, &left, &base)?.map(|k| left[k].clone()))
    }

    /// What stands in the way of rejecting operation `key` alone, of those
    /// that stand (`stands`): the first patch that does not apply without
    /// it, none when the stack still applies.
    pub fn needer(&self, db: &Db, stands: &BTreeSet<String>, key: &str) -> Result<Option<Placed>> {
        let mut without = stands.clone();
        without.remove(key);
        self.failing(db, &without)
    }

    /// The first slot of operation `key`: where it starts to change the
    /// stack; none for the base, which changes all of it.
    fn first_slot(&self, key: &str) -> Option<usize> {
        self.slots.iter().position(|s| match s {
            Slot::Kept { key: k, .. } | Slot::Was { key: k, .. } | Slot::Is { key: k, .. } => k == key,
        })
    }

    /// The operations marked, but `but`, that change what is under the patch
    /// placed `q`, of those `of` says: the nearest first, the base last.
    fn under(&self, q: &Placed, of: impl Fn(&Op) -> bool, but: &[String]) -> Vec<String> {
        let mut below: Vec<(usize, String)> = self
            .marked()
            .filter(|o| of(o) && !but.contains(&o.key) && o.key != q.key)
            .filter_map(|o| match o.kind {
                Kind::Base => Some((0, o.key.clone())),
                _ => self.first_slot(&o.key).filter(|s| *s < q.slot).map(|s| (s + 1, o.key.clone())),
            })
            .collect();
        below.sort_by(|a, b| b.0.cmp(&a.0));
        below.into_iter().map(|(_, k)| k).collect()
    }

    /// Whether operation `c`, marked otherwise than it stands, as `stands`
    /// then are, gets further than the patch placed `q`, the first that did
    /// not apply: the stack they leave applies; or it holds `q` as it was
    /// placed, and every patch up to it applies; or the first that does not
    /// is `c`'s own (`new`, as `c` leaves it, else as it was), which then
    /// needs more marked with it, as a patch of a chain of them does.
    fn further(&self, db: &Db, stands: &BTreeSet<String>, q: &Placed, c: &str, new: bool) -> Result<bool> {
        let (left, base) = self.leave(stands);
        let at = left.iter().position(|p| p.slot == q.slot && p.id == q.id);
        Ok(match self.fails(db, &left, &base)? {
            None => true,
            Some(k) => at.is_some_and(|at| at < k) || (left[k].key == c && left[k].new == new),
        })
    }

    /// Operation `key`, which stands among `stands`, rejected: alone, or
    /// with the operations that go with it (each said, to ask first), the
    /// stack as they then stand applying; or why not. Refused while an
    /// operation that stands needs it, or a patch left as it was does: its
    /// patch, as it stands, does not apply without it. Its own patch as it
    /// was, or one rejected already, not applying on what the others leave,
    /// the operation under it that makes it so goes with it.
    pub fn reject(&self, db: &Db, stands: &BTreeSet<String>, key: &str) -> std::result::Result<Vec<With>, Refused> {
        let unchecked = |e: crate::db::Error| Refused::Unchecked(e.0);
        let mut gone = vec![key.to_string()];
        let mut with = Vec::new();
        loop {
            let now: BTreeSet<String> = stands.iter().filter(|k| !gone.contains(k)).cloned().collect();
            let Some(q) = self.failing(db, &now).map_err(unchecked)? else { return Ok(with) };
            if q.new {
                return Err(Refused::Needed { by: q.key, on: gone });
            }
            let mut found = None;
            for c in self.under(&q, |o| now.contains(&o.key), &gone) {
                let mut then = now.clone();
                then.remove(&c);
                if self.further(db, &then, &q, &c, false).map_err(unchecked)? {
                    found = Some(c);
                    break;
                }
            }
            let Some(c) = found else { return Err(Refused::Stuck { key: q.key, new: false }) };
            with.push(With {
                key: c.clone(),
                because: q.key.clone(),
                by: vec![c.clone()],
            });
            gone.push(c);
        }
    }

    /// Operation `key`, rejected, accepted: alone, or with the operations
    /// rejected whose patches as they were no longer apply on it (each said,
    /// to ask first), the stack as they then stand applying; or why not.
    /// Refused while its patch needs an operation that is rejected, or a
    /// patch that stands would no longer apply.
    pub fn accept(&self, db: &Db, stands: &BTreeSet<String>, key: &str) -> std::result::Result<Vec<With>, Refused> {
        let unchecked = |e: crate::db::Error| Refused::Unchecked(e.0);
        let mut coming = vec![key.to_string()];
        let mut with = Vec::new();
        loop {
            let now: BTreeSet<String> = stands.iter().cloned().chain(coming.iter().cloned()).collect();
            let Some(q) = self.failing(db, &now).map_err(unchecked)? else { return Ok(with) };
            match (q.new, coming.contains(&q.key)) {
                // Its patch, or one coming with it, needs an operation that
                // is rejected: which one, accepted too, would let it apply.
                (true, true) => {
                    for y in self.under(&q, |o| !now.contains(&o.key), &coming) {
                        let mut then = now.clone();
                        then.insert(y.clone());
                        if self.further(db, &then, &q, &y, true).map_err(unchecked)? {
                            return Err(Refused::Needs { op: q.key, on: y });
                        }
                    }
                    return Err(Refused::Stuck { key: q.key, new: true });
                }
                (true, false) => return Err(Refused::Breaks { by: q.key, on: coming }),
                // One rejected, its patch as it was no longer applying: it
                // comes too.
                (false, _) if self.op(&q.key).is_some_and(Op::marked) => {
                    with.push(With {
                        key: q.key.clone(),
                        because: q.key.clone(),
                        by: coming.clone(),
                    });
                    coming.push(q.key);
                }
                (false, _) => return Err(Refused::Stuck { key: q.key, new: false }),
            }
        }
    }

    /// The operations `marks` reject, said as the history says them, each
    /// by its word and its place, as the review lists it: `modify 5`,
    /// `remove 2`, `base`.
    pub fn rejected_said(&self, marks: &BTreeMap<String, bool>) -> String {
        let each: Vec<String> = self
            .marked()
            .filter(|o| self.mark(marks, o) == Some(false))
            .map(|o| match o.kind {
                Kind::Base => "base".to_string(),
                k => format!("{} {}", k.word(), o.at),
            })
            .collect();
        each.join(", ")
    }

    /// Operation `key`, said as what is marked: `patch 5's operation`, `the
    /// removal of patch 2`, `the new base`.
    pub fn said(&self, key: &str) -> String {
        match self.op(key) {
            Some(o) if o.kind == Kind::Base => "the new base".into(),
            Some(o) if o.kind == Kind::Remove => format!("the removal of patch {}", o.at),
            Some(o) => format!("patch {}'s operation", o.at),
            None => "an operation gone".into(),
        }
    }

    /// The patch of operation `key`, as the change leaves it (`new`) or as it
    /// was, said as a sentence starts with it: `patch 5's new version`,
    /// `patch 3, which the change adds,`, `patch 5 as it was`.
    pub fn patch_said(&self, key: &str, new: bool) -> String {
        let Some(o) = self.op(key) else { return "a patch gone".into() };
        let n = o.at;
        match (o.kind, new) {
            (Kind::Noop, _) => format!("patch {n}, which the change leaves as it is,"),
            (Kind::Add, _) => format!("patch {n}, which the change adds,"),
            (Kind::Move, true) => format!("patch {n} at its new place"),
            (Kind::Move, false) => format!("patch {n} at its old place"),
            (Kind::Base, true) => "the new base".into(),
            (Kind::Base, false) => "the base as it was".into(),
            (_, true) => format!("patch {n}'s new version"),
            (_, false) => format!("patch {n} as it was"),
        }
    }

    /// What operation `key` puts in the stack, as what another patch
    /// applies only on: `on patch 4's new version`, `without patch 2, which
    /// the change removes`, `on the new base`.
    pub fn on_said(&self, key: &str) -> String {
        let Some(o) = self.op(key) else { return "on an operation gone".into() };
        let n = o.at;
        match o.kind {
            Kind::Base => "on the new base".into(),
            Kind::Add => format!("with patch {n}, which the change adds"),
            Kind::Move => format!("with patch {n} at its new place"),
            Kind::Remove => format!("without patch {n}, which the change removes"),
            _ => format!("on patch {n}'s new version"),
        }
    }

    /// What operation `key` leaves in the stack once rejected, as what
    /// another patch applies only on: `on patch 4 as it was`, `with patch 2,
    /// which the change removes`, `on the base as it was`.
    pub fn off_said(&self, key: &str) -> String {
        let Some(o) = self.op(key) else { return "without an operation gone".into() };
        let n = o.at;
        match o.kind {
            Kind::Base => "on the base as it was".into(),
            Kind::Add => format!("without patch {n}, which the change adds"),
            Kind::Move => format!("with patch {n} at its old place"),
            Kind::Remove => format!("with patch {n}, which the change removes"),
            _ => format!("on patch {n} as it was"),
        }
    }

    /// Operations `keys`, said one after another: `patch 4's operation and
    /// the removal of patch 2`; more than three, the first two and how many
    /// more (`patch 19's operation, patch 18's operation and 17 other
    /// operations`).
    pub fn listed(&self, keys: &[String]) -> String {
        let each: Vec<String> = keys.iter().map(|k| self.said(k)).collect();
        if each.len() > 3 {
            return format!("{}, {} and {} other operations", each[0], each[1], each.len() - 2);
        }
        match each.split_last() {
            None => String::new(),
            Some((last, [])) => last.clone(),
            Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
        }
    }

    /// Why operations `with` go with the one marked, said: a sentence each
    /// (`with_said`); more than three, the first three and how many more.
    pub fn with_all_said(&self, with: &[With]) -> String {
        let why: Vec<String> = with.iter().take(3).map(|w| self.with_said(w)).collect();
        match with.len() {
            n if n > 3 => format!("{} And so on, for {} more.", why.join(" "), n - 3),
            _ => why.join(" "),
        }
    }

    /// Why marking operation `key` alone (`accept`ing it, or rejecting it)
    /// is refused, said for the status line: `patch 5's new version applies
    /// only on patch 4's new version: reject patch 5's operation first`.
    pub fn refused_said(&self, key: &str, accept: bool, r: &Refused) -> String {
        let doing = if accept { "accepted" } else { "rejected" };
        let alone = format!("so {} cannot be {doing} alone", self.said(key));
        let noop = |k: &str| self.op(k).is_some_and(|o| !o.marked());
        match r {
            Refused::Needed { by, on } => {
                let needs = match on.as_slice() {
                    [one] => format!("{} applies only {}", self.patch_said(by, true), self.on_said(one)),
                    _ => format!("{} does not apply without {}", self.patch_said(by, true), self.listed(on)),
                };
                match noop(by) {
                    true => format!("{needs}, {alone}"),
                    false => format!("{needs}: reject {} first", self.said(by)),
                }
            }
            Refused::Needs { op, on } => format!("{} applies only {}: accept {} first", self.patch_said(op, true), self.on_said(on), self.said(on)),
            Refused::Breaks { by, on } => {
                let breaks = match on.as_slice() {
                    [one] => format!("{} applies only {}", self.patch_said(by, true), self.off_said(one)),
                    _ => format!("{} does not apply once {} are accepted", self.patch_said(by, true), self.listed(on)),
                };
                match noop(by) {
                    true => format!("{breaks}, {alone}"),
                    false => format!("{breaks}: reject {} first", self.said(by)),
                }
            }
            Refused::Stuck { key: q, new } => format!("{} does not apply on what the other operations leave in the stack, {alone}", self.patch_said(q, *new)),
            Refused::Unchecked(why) => format!("{} cannot be {doing} alone, as the stack cannot be checked: {why}", self.said(key)),
        }
    }

    /// Why operation `w.key` goes with the one marked, as a sentence:
    /// `Patch 5 as it was applies only on patch 4 as it was.`
    pub fn with_said(&self, w: &With) -> String {
        let was = self.patch_said(&w.because, false);
        sentence(&match w.by.as_slice() {
            [c] => format!("{was} applies only {}", self.off_said(c)),
            by => format!("{was} does not apply once {} are accepted", self.listed(by)),
        })
    }

    /// What the review's right pane says of operation `key`, as `marks` mark
    /// them, a sentence each: what it needs, then what needs it, or, once it
    /// is rejected, whether it can be accepted alone.
    pub fn pane_said(&self, db: &Db, marks: &BTreeMap<String, bool>, key: &str) -> Vec<String> {
        let Some(o) = self.op(key) else { return vec![] };
        let stands = self.standing(marks);
        let mut out = Vec::new();
        if !o.marked() {
            out.push("The change leaves this patch as it is: there is nothing to accept or reject.".to_string());
        }
        // What it needs: the operations that stand without which its patch
        // is the first that does not apply. Only those under its patch, or
        // the base's, can be: the stack up to it is the same without the
        // others.
        if stands.contains(key) {
            let (left, _) = self.leave(&stands);
            let at = left.iter().find(|p| p.key == key && p.new).map_or(0, |p| p.slot);
            let under = |x: &Op| x.kind == Kind::Base || self.first_slot(&x.key).is_some_and(|s| s < at);
            for x in self.marked().filter(|x| x.key != key && stands.contains(&x.key) && under(x)) {
                if let Ok(Some(q)) = self.needer(db, &stands, &x.key)
                    && q.key == key
                    && q.new
                {
                    out.push(sentence(&format!("{} applies only {}", self.patch_said(key, true), self.on_said(&x.key))));
                }
            }
        }
        if !o.marked() {
            return out;
        }
        let noop = |k: &str| self.op(k).is_some_and(|o| !o.marked());
        // Of a patch as it is in the stack, said as a sentence starts.
        let patch = |k: &str, new: bool| self.patch_said(k, new);
        let mut say = |said: String| {
            if !said.is_empty() {
                out.push(sentence(&said));
            }
        };
        if !stands.contains(key) {
            let alone = "so this operation cannot be accepted alone";
            say(match self.accept(db, &stands, key) {
                Ok(with) if with.is_empty() => "Nothing it needs is rejected: it can be accepted alone".to_string(),
                Ok(with) => {
                    let goes: Vec<String> = with.iter().map(|w| w.key.clone()).collect();
                    format!("{} So this operation is accepted only with {}", self.with_all_said(&with), self.listed(&goes))
                }
                Err(Refused::Needs { op, on }) => format!("{} applies only {}: accept {} before this one", patch(&op, true), self.on_said(&on), self.said(&on)),
                Err(Refused::Breaks { by, on }) => {
                    let breaks = match on.as_slice() {
                        [one] => format!("{} applies only {}", patch(&by, true), self.off_said(one)),
                        _ => format!("{} does not apply once {} are accepted", patch(&by, true), self.listed(&on)),
                    };
                    match noop(&by) {
                        true => format!("{breaks}, {alone}"),
                        false => format!("{breaks}: reject {} before accepting this one", self.said(&by)),
                    }
                }
                Err(Refused::Stuck { key: q, new }) => format!("{} does not apply on what the other operations leave in the stack, {alone}", patch(&q, new)),
                Err(Refused::Unchecked(why)) => format!("Whether this operation can be accepted alone cannot be told, as the stack cannot be checked: {why}"),
                Err(Refused::Needed { .. }) => String::new(),
            });
            return out;
        }
        let alone = "so this operation cannot be rejected alone";
        say(match self.reject(db, &stands, key) {
            Ok(with) if with.is_empty() => "Nothing else needs this operation: it can be rejected alone".to_string(),
            Ok(with) => {
                let goes: Vec<String> = with.iter().map(|w| w.key.clone()).collect();
                format!("{} So this operation is rejected only with {}", self.with_all_said(&with), self.listed(&goes))
            }
            Err(Refused::Needed { by, on }) => {
                let needs = match on.as_slice() {
                    [one] => format!("{} applies only {}", patch(&by, true), self.on_said(one)),
                    _ => format!("{} does not apply without {}", patch(&by, true), self.listed(&on)),
                };
                match noop(&by) {
                    true => format!("{needs}, {alone}"),
                    false => format!("{needs}: reject {} before this one", self.said(&by)),
                }
            }
            Err(Refused::Stuck { key: q, new }) => format!("{} does not apply on what the other operations leave in the stack, {alone}", patch(&q, new)),
            Err(Refused::Unchecked(why)) => format!("Whether this operation can be rejected alone cannot be told, as the stack cannot be checked: {why}"),
            Err(Refused::Needs { .. } | Refused::Breaks { .. }) => String::new(),
        });
        out
    }
}

/// `said` as a sentence: its first letter a capital, a full stop at its
/// end.
fn sentence(said: &str) -> String {
    let mut c = said.chars();
    let mut out = match c.next() {
        Some(first) => first.to_uppercase().chain(c).collect::<String>(),
        None => String::new(),
    };
    if !out.ends_with('.') {
        out.push('.');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Patch;

    fn git_ok(dir: &Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_AUTHOR_NAME", "a")
            .env("GIT_AUTHOR_EMAIL", "a@b")
            .env("GIT_COMMITTER_NAME", "a")
            .env("GIT_COMMITTER_EMAIL", "a@b")
            .output()
            .unwrap();
        assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8(out.stdout).unwrap()
    }

    /// A repository whose base holds `files`, each `name` of `lines`; its
    /// path and base.
    fn repo(files: &[(&str, &str)]) -> (tempfile::TempDir, String, String) {
        let tmp = tempfile::tempdir().unwrap();
        let r = tmp.path().canonicalize().unwrap();
        git_ok(&r, &["init", "-q", "-b", "main"]);
        git_ok(&r, &["config", "commit.gpgsign", "false"]);
        for (name, text) in files {
            std::fs::write(r.join(name), text).unwrap();
        }
        git_ok(&r, &["add", "."]);
        git_ok(&r, &["commit", "-qm", "base"]);
        let base = git_ok(&r, &["rev-parse", "HEAD"]).trim().to_string();
        (tmp, r.display().to_string(), base)
    }

    /// The diff of `name` from `was` to `is`, as git writes it.
    fn diff(name: &str, was: &str, is: &str) -> String {
        let tmp = tempfile::tempdir().unwrap();
        let (a, b) = (tmp.path().join("a"), tmp.path().join("b"));
        std::fs::write(&a, was).unwrap();
        std::fs::write(&b, is).unwrap();
        let out = std::process::Command::new("git").args(["diff", "--no-index", "--no-color"]).arg(&a).arg(&b).output().unwrap();
        let text = String::from_utf8(out.stdout).unwrap();
        let body: String = text.lines().skip_while(|l| !l.starts_with("@@")).map(|l| format!("{l}\n")).collect();
        format!("diff --git a/{name} b/{name}\n--- a/{name}\n+++ b/{name}\n{body}")
    }

    /// Lines `from..=to` of a file, `L{n}`, each changed to `{n}{mark}` for
    /// those in `changed`.
    fn text(n: usize, changed: &[(usize, &str)]) -> String {
        (1..=n)
            .map(|k| match changed.iter().find(|(at, _)| *at == k) {
                Some((_, m)) => format!("{k}{m}\n"),
                None => format!("L{k}\n"),
            })
            .collect()
    }

    fn db_with(patches: &[Patch]) -> (tempfile::TempDir, Db) {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("db.jsonl");
        Db::create(&path).unwrap();
        let mut db = Db::load(&path).unwrap();
        for p in patches {
            db.add_patch(p.clone()).unwrap();
        }
        (tmp, db)
    }

    fn stack(repo: &str, base: &str, ids: &[&Patch]) -> Stack {
        Stack {
            name: "s".into(),
            patches: ids.iter().map(|p| p.id.clone()).collect(),
            base: Some(base.into()),
            repo: Some(repo.into()),
            parent: None,
            op: String::new(),
            time: 0,
        }
    }

    fn patch(change: &str, title: &str, diff: String) -> Patch {
        Patch::new(change.into(), None, None, title.into(), diff)
    }

    #[test]
    fn each_line_of_the_stack_diff_is_an_operation_and_the_stacks_they_leave_keep_the_order() {
        let (_r, repo, base) = repo(&[("f.txt", &text(30, &[]))]);
        let f = |changed: &[(usize, &str)], was: &[(usize, &str)]| diff("f.txt", &text(30, was), &text(30, changed));
        // 1 changes line 2, 2 line 10, 3 line 20; the change gives 2 a new
        // version, removes 3, and adds 4 over it, changing line 12, which 2's
        // new version is in the context of.
        let p1 = patch("kkkkkkkkkkkk", "one", f(&[(2, "a")], &[]));
        let p2 = patch("llllllllllll", "two", f(&[(2, "a"), (10, "b")], &[(2, "a")]));
        let p3 = patch("mmmmmmmmmmmm", "three", f(&[(2, "a"), (10, "b"), (20, "c")], &[(2, "a"), (10, "b")]));
        let p2b = patch("llllllllllll", "two", f(&[(2, "a"), (10, "B")], &[(2, "a")]));
        let p4 = patch("nnnnnnnnnnnn", "four", f(&[(2, "a"), (10, "B"), (12, "d")], &[(2, "a"), (10, "B")]));
        let (_t, db) = db_with(&[p1.clone(), p2.clone(), p3.clone(), p2b.clone(), p4.clone()]);
        let before = stack(&repo, &base, &[&p1, &p2, &p3]);
        let after = stack(&repo, &base, &[&p1, &p2b, &p4]);
        let ops = of(&db, &before, &after);
        let said: Vec<(Kind, usize, usize)> = ops.list.iter().map(|o| (o.kind, o.at, o.entry)).collect();
        assert_eq!(said, [(Kind::Noop, 1, 0), (Kind::Modify, 2, 1), (Kind::Remove, 3, 2), (Kind::Add, 3, 3)]);
        // The entries are the stack diff's own.
        on_their_lines(&db, &ops, &before, &after);
        assert_eq!(crate::history::stacks_diff(&db, Some(&before), Some(&after), false).len(), 4);
        // Each left as marked: all of it is the stack after; none, before.
        let all: BTreeSet<String> = ops.list.iter().map(|o| o.key.clone()).collect();
        let ids = |stands: &BTreeSet<String>| ops.leave(stands).0.into_iter().map(|p| p.id).collect::<Vec<_>>();
        assert_eq!(ids(&all), after.patches);
        let noop: BTreeSet<String> = [ops.list[0].key.clone()].into();
        assert_eq!(ids(&noop), before.patches);
        // 3 kept, the rest taken: 3 back where it was, 4 over it.
        let mut kept = all.clone();
        kept.remove(&key_of("mmmmmmmmmmmm"));
        assert_eq!(ids(&kept), [p1.id.clone(), p2b.id.clone(), p3.id.clone(), p4.id.clone()]);
        // Every one of them applies, and the check says which does not.
        assert_eq!(ops.failing(&db, &all).unwrap(), None);
        assert_eq!(ops.failing(&db, &kept).unwrap(), None);
        // 2 as it was under 4's new version, which applies only on 2's.
        let mut old2 = all.clone();
        old2.remove(&key_of("llllllllllll"));
        let q = ops.failing(&db, &old2).unwrap().unwrap();
        assert_eq!((q.key.as_str(), q.new), ("op nnnnnnnnnnnn", true));
        assert_eq!(ops.reject(&db, &all, "op llllllllllll"), Err(Refused::Needed { by: "op nnnnnnnnnnnn".into(), on: vec!["op llllllllllll".into()] }));
        assert_eq!(
            ops.refused_said("op llllllllllll", false, &ops.reject(&db, &all, "op llllllllllll").unwrap_err()),
            "patch 3, which the change adds, applies only on patch 2's new version: reject patch 3's operation first"
        );
        // 4 first, then 2: alone each time.
        assert_eq!(ops.reject(&db, &all, "op nnnnnnnnnnnn"), Ok(vec![]));
        let mut no4 = all.clone();
        no4.remove("op nnnnnnnnnnnn");
        assert_eq!(ops.reject(&db, &no4, "op llllllllllll"), Ok(vec![]));
        // Accepting 4 while 2 is rejected: it needs 2.
        no4.remove("op llllllllllll");
        assert_eq!(ops.accept(&db, &no4, "op nnnnnnnnnnnn"), Err(Refused::Needs { op: "op nnnnnnnnnnnn".into(), on: "op llllllllllll".into() }));
        assert_eq!(ops.accept(&db, &no4, "op llllllllllll"), Ok(vec![]));
        assert_eq!(ops.rejected_said(&[("op llllllllllll".to_string(), false), ("op mmmmmmmmmmmm".to_string(), false)].into()), "modify 2, remove 3");
    }

    #[test]
    fn a_patch_as_it_was_that_no_longer_applies_goes_with_the_operation_under_it() {
        let (_r, repo, base) = repo(&[("f.txt", &text(12, &[]))]);
        let f = |changed: &[(usize, &str)], was: &[(usize, &str)]| diff("f.txt", &text(12, was), &text(12, changed));
        // 2 changes line 5, its context lines 2 to 8 in sight; 1's new
        // version changes line 4 too, and 2's new version is 2 rebased on it.
        let p1 = patch("kkkkkkkkkkkk", "one", f(&[(1, "a")], &[]));
        let p2 = patch("llllllllllll", "two", f(&[(1, "a"), (5, "b")], &[(1, "a")]));
        let p1b = patch("kkkkkkkkkkkk", "one", f(&[(1, "a"), (4, "x")], &[]));
        let p2b = patch("llllllllllll", "two", f(&[(1, "a"), (4, "x"), (5, "b")], &[(1, "a"), (4, "x")]));
        let (_t, db) = db_with(&[p1.clone(), p2.clone(), p1b.clone(), p2b.clone()]);
        let ops = of(&db, &stack(&repo, &base, &[&p1, &p2]), &stack(&repo, &base, &[&p1b, &p2b]));
        let all = ops.standing(&BTreeMap::new());
        // 1 is needed by 2's new version; 2, rejected alone, leaves 2 as it
        // was on 1's new version, which it does not apply on: 1 goes with it.
        assert_eq!(ops.reject(&db, &all, "op kkkkkkkkkkkk"), Err(Refused::Needed { by: "op llllllllllll".into(), on: vec!["op kkkkkkkkkkkk".into()] }));
        let with = ops.reject(&db, &all, "op llllllllllll").unwrap();
        assert_eq!(with, [With { key: "op kkkkkkkkkkkk".into(), because: "op llllllllllll".into(), by: vec!["op kkkkkkkkkkkk".into()] }]);
        assert_eq!(ops.with_said(&with[0]), "Patch 2 as it was applies only on patch 1 as it was.");
        // Both rejected, accepting 1 again brings 2 with it, as 2 as it was
        // does not apply on it.
        let none = ops.standing(&[("op kkkkkkkkkkkk".to_string(), false), ("op llllllllllll".to_string(), false)].into());
        let with = ops.accept(&db, &none, "op kkkkkkkkkkkk").unwrap();
        assert_eq!(with.iter().map(|w| w.key.as_str()).collect::<Vec<_>>(), ["op llllllllllll"]);
        assert_eq!(ops.accept(&db, &none, "op llllllllllll"), Err(Refused::Needs { op: "op llllllllllll".into(), on: "op kkkkkkkkkkkk".into() }));
    }

    /// Whether each operation of `ops` is on the line of the stack diff
    /// from `before` to `after` that says it: its kind, and its place.
    fn on_their_lines(db: &Db, ops: &Ops, before: &Stack, after: &Stack) {
        use crate::history::Diffed;
        let diffed = crate::history::stacks_diff(db, Some(before), Some(after), false);
        for o in &ops.list {
            let line = &diffed[o.entry];
            let fits = match (o.kind, line) {
                (Kind::Base, Diffed::Base { what: "base", .. }) => true,
                (Kind::Remove, Diffed::Dropped { was, .. }) => *was == o.at,
                (Kind::Add, Diffed::Patch { at, added: true, .. }) => *at == o.at,
                (Kind::Reword, Diffed::Patch { at, reworded: true, was, .. }) => *at == o.at && was.is_some() == o.moved,
                (Kind::Modify, Diffed::Patch { at, changed: true, reworded: false, was, .. }) => *at == o.at && was.is_some() == o.moved,
                (Kind::Move, Diffed::Patch { at, changed: false, was: Some(_), .. }) => *at == o.at,
                (Kind::Noop, Diffed::Patch { at, added: false, changed: false, was: None, .. }) => *at == o.at,
                _ => false,
            };
            assert!(fits, "{o:?} on {line:?}");
        }
    }

    #[test]
    fn a_move_rejected_leaves_its_patch_at_its_old_place() {
        let (_r, repo, base) = repo(&[("f.txt", "a\n"), ("g.txt", "b\n")]);
        let x = patch("kkkkkkkkkkkk", "x", diff("f.txt", "a\n", "A\n"));
        let y = patch("llllllllllll", "y", diff("g.txt", "b\n", "B\n"));
        let (_t, db) = db_with(&[x.clone(), y.clone()]);
        let (before, after) = (stack(&repo, &base, &[&x, &y]), stack(&repo, &base, &[&y, &x]));
        let ops = of(&db, &before, &after);
        on_their_lines(&db, &ops, &before, &after);
        let moved: Vec<&Op> = ops.marked().collect();
        assert_eq!((moved.len(), moved[0].kind), (1, Kind::Move));
        let ids = |stands: &BTreeSet<String>| ops.leave(stands).0.into_iter().map(|p| p.id).collect::<Vec<_>>();
        assert_eq!(ids(&ops.standing(&BTreeMap::new())), after.patches);
        assert_eq!(ids(&ops.standing(&[(moved[0].key.clone(), false)].into())), before.patches);
        assert_eq!(ops.reject(&db, &ops.standing(&BTreeMap::new()), &moved[0].key), Ok(vec![]));
    }

    #[test]
    fn a_patch_made_again_on_the_new_base_needs_it() {
        // The base moved on, a line in sight of the patch changed: the
        // patch made again on it.
        let (_r, repo, base) = repo(&[("f.txt", &text(9, &[]))]);
        std::fs::write(Path::new(&repo).join("f.txt"), text(9, &[(3, "z")])).unwrap();
        git_ok(Path::new(&repo), &["commit", "-qam", "on"]);
        let on = git_ok(Path::new(&repo), &["rev-parse", "HEAD"]).trim().to_string();
        let old = patch("kkkkkkkkkkkk", "one", diff("f.txt", &text(9, &[]), &text(9, &[(5, "a")])));
        let new = patch("kkkkkkkkkkkk", "one", diff("f.txt", &text(9, &[(3, "z")]), &text(9, &[(3, "z"), (5, "a")])));
        let (_t, db) = db_with(&[old.clone(), new.clone()]);
        let (before, after) = (stack(&repo, &base, &[&old]), stack(&repo, &on, &[&new]));
        let ops = of(&db, &before, &after);
        on_their_lines(&db, &ops, &before, &after);
        assert_eq!(ops.list.iter().map(|o| o.kind).collect::<Vec<_>>(), [Kind::Base, Kind::Modify]);
        let all = ops.standing(&BTreeMap::new());
        assert_eq!(ops.leave(&all).1.as_deref(), Some(on.as_str()));
        assert_eq!(ops.leave(&ops.standing(&[(BASE.to_string(), false)].into())).1.as_deref(), Some(base.as_str()));
        let refused = ops.reject(&db, &all, BASE).unwrap_err();
        assert_eq!(ops.refused_said(BASE, false, &refused), "patch 1's new version applies only on the new base: reject patch 1's operation first");
        // Patch 1 as it was, on the new base: it goes with the base's.
        let with = ops.reject(&db, &all, "op kkkkkkkkkkkk").unwrap();
        assert_eq!(with.iter().map(|w| w.key.as_str()).collect::<Vec<_>>(), [BASE]);
        assert_eq!(ops.with_said(&with[0]), "Patch 1 as it was applies only on the base as it was.");
    }

    /// The changes of `n` patches, twelve letters from k to z each.
    fn changes(n: usize) -> Vec<String> {
        let l = |i: usize| (b'k' + i as u8) as char;
        (0..n).map(|k| format!("{}{}{}", "k".repeat(10), l(k / 16), l(k % 16))).collect()
    }

    /// A stack of `n` patches on a file of `lines` lines, patch `k` (from 0)
    /// changing line `at(k)` to `{line}a`; and the change of the stack giving
    /// each a new version that changes it to `{line}N` instead, on the new
    /// versions under it. The repository, the database, the operations, and
    /// the patches' changes.
    fn every_one(n: usize, lines: usize, at: impl Fn(usize) -> usize) -> (tempfile::TempDir, tempfile::TempDir, Db, Ops, Vec<String>) {
        let (r, repo, base) = repo(&[("f.txt", &text(lines, &[]))]);
        let changes = changes(n);
        let made = |mark: &'static str| -> Vec<Patch> {
            (0..n)
                .map(|k| {
                    let was: Vec<(usize, &str)> = (0..k).map(|j| (at(j), mark)).collect();
                    let mut is = was.clone();
                    is.push((at(k), mark));
                    patch(&changes[k], &format!("patch {}", k + 1), diff("f.txt", &text(lines, &was), &text(lines, &is)))
                })
                .collect()
        };
        let (old, new) = (made("a"), made("N"));
        let (t, db) = db_with(&old.iter().chain(&new).cloned().collect::<Vec<_>>());
        let ops = of(&db, &stack(&repo, &base, &old.iter().collect::<Vec<_>>()), &stack(&repo, &base, &new.iter().collect::<Vec<_>>()));
        (r, t, db, ops, changes)
    }

    #[test]
    fn of_twenty_operations_none_needing_another_each_is_rejected_alone() {
        let (_r, _t, db, ops, changes) = every_one(20, 210, |k| 10 * (k + 1));
        assert_eq!(ops.marked().count(), 20);
        let all = ops.standing(&BTreeMap::new());
        let said = ops.pane_said(&db, &BTreeMap::new(), &key_of(&changes[19]));
        assert_eq!(said, ["Nothing else needs this operation: it can be rejected alone."]);
        for c in &changes {
            assert_eq!(ops.reject(&db, &all, &key_of(c)), Ok(vec![]));
        }
        // Every other one rejected, the stack still applies.
        let marks: BTreeMap<String, bool> = changes.iter().step_by(2).map(|c| (key_of(c), false)).collect();
        assert_eq!(ops.failing(&db, &ops.standing(&marks)).unwrap(), None);
    }

    #[test]
    fn of_twenty_operations_each_needing_the_one_under_it_and_each_patch_as_it_was_the_one_under_it_as_it_was_all_go_together() {
        // Three lines apart: each patch has the one under it in sight.
        let (_r, _t, db, ops, changes) = every_one(20, 63, |k| 3 * (k + 1));
        let key = |k: usize| key_of(&changes[k]);
        let all = ops.standing(&BTreeMap::new());
        // The top's, rejected, takes every one under it with it, the nearest
        // first.
        let with = ops.reject(&db, &all, &key(19)).unwrap();
        assert_eq!(with.iter().map(|w| w.key.clone()).collect::<Vec<_>>(), (0..19).rev().map(key).collect::<Vec<_>>());
        assert_eq!(ops.with_said(&with[0]), "Patch 20 as it was applies only on patch 19 as it was.");
        assert_eq!(
            ops.with_all_said(&with),
            "Patch 20 as it was applies only on patch 19 as it was. Patch 19 as it was applies only on patch 18 as it was. Patch 18 as it was applies only on patch 17 as it was. And so on, for 16 more."
        );
        assert_eq!(ops.listed(&with.iter().map(|w| w.key.clone()).collect::<Vec<_>>()), "patch 19's operation, patch 18's operation and 17 other operations");
        // One in the middle: the one above it needs it.
        let Err(Refused::Needed { by, on }) = ops.reject(&db, &all, &key(9)) else { panic!() };
        assert_eq!((by, on.len()), (key(10), 10));
        // Every one rejected: the bottom's, accepted, brings every one above
        // it; the top's needs the one under it.
        let none = ops.standing(&changes.iter().map(|c| (key_of(c), false)).collect());
        assert_eq!(ops.accept(&db, &none, &key(0)).unwrap().len(), 19);
        assert_eq!(ops.accept(&db, &none, &key(19)), Err(Refused::Needs { op: key(19), on: key(18) }));
    }

    #[test]
    fn a_stack_that_cannot_be_checked_marks_none_of_its_operations_alone() {
        let p1 = patch("kkkkkkkkkkkk", "one", diff("f.txt", "a\n", "b\n"));
        let p1b = patch("kkkkkkkkkkkk", "one", diff("f.txt", "a\n", "c\n"));
        let (_t, db) = db_with(&[p1.clone(), p1b.clone()]);
        let before = Stack { base: None, repo: None, ..stack("/nowhere", "x", &[&p1]) };
        let after = Stack { patches: vec![p1b.id.clone()], ..before.clone() };
        let ops = of(&db, &before, &after);
        let all = ops.standing(&BTreeMap::new());
        let Err(Refused::Unchecked(why)) = ops.reject(&db, &all, "op kkkkkkkkkkkk") else { panic!() };
        assert!(why.contains("is read-only"), "{why}");
    }
}
