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
//! (`check::first_failure`). What each operation needs follows from the two
//! stacks alone, the one the change went on from and the one it makes, both
//! fixed once the work lands: it is worked out then, by the landing, and
//! stored with the work (`Needs`, `Held::needs`), for each operation
//! rejected alone, every other one standing: the operations whose patches
//! then no longer apply, and those that go with it. The review decides from
//! what is stored, at once, with no check: rejecting an operation is refused
//! while one that stands needs it, and accepting one while one it needs is
//! rejected (`Ops::reject`, `Ops::accept`). An operation whose patch as it
//! was no longer applies on what the others leave goes with the operation
//! that makes it so: the two are rejected, or accepted, together, once
//! asked. Two rejections together may, rarely, break a patch that neither
//! breaks alone: closing checks the stack the operations accepted leave
//! (`Ops::closing`), which keeps what is filed right. A change with no
//! operation to mark, every patch and the base left as they were, is marked
//! as a whole (`Ops::whole`), as every change once was; and so is the
//! change of work that holds nothing worked out for it (work that landed
//! before stacked worked it out, or whose stacks it could not be worked out
//! for: `of_review`, `whole_why`).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

use serde::{Deserialize, Serialize};

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

/// Where the operations read the patches they name: the database; or the
/// patches taken from it (`Ops::pool`), for a check made away from the
/// screen, where there is no database.
pub trait Patches {
    /// Patch `id`.
    fn get(&self, id: &str) -> &Patch;
}

impl Patches for Db {
    fn get(&self, id: &str) -> &Patch {
        self.patch(id)
    }
}

impl Patches for HashMap<String, Patch> {
    fn get(&self, id: &str) -> &Patch {
        &self[id]
    }
}

/// The operations of a change, from a stack to the stack it makes.
#[derive(Clone, Debug, PartialEq)]
pub struct Ops {
    /// Each of them, in the order of the stack diff's lines; none when the
    /// change is marked as one (`as_one`).
    pub list: Vec<Op>,
    slots: Vec<Slot>,
    /// The base before, and after.
    bases: (Option<String>, Option<String>),
    /// The stack's name and repository, to check the stacks they leave.
    name: String,
    repo: Option<String>,
    /// Whether the change is marked as one, though it has operations to
    /// mark: nothing is stored of what each needs (`as_one`).
    one: bool,
}

/// The operations from stack `before` to `after`, as the stack diff says
/// them (`history::stacks_diff`), each on the line it has there: a patch is
/// known across its versions by its change; one out of the order the others
/// keep is moved.
pub fn of<P: Patches + ?Sized>(db: &P, before: &Stack, after: &Stack) -> Ops {
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
    let old: Vec<&Patch> = before.patches.iter().map(|id| db.get(id)).collect();
    let new: Vec<&Patch> = after.patches.iter().map(|id| db.get(id)).collect();
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
        one: false,
    }
}

/// The stack the change of `h` makes from stack `from`: its patches, on its
/// base, else on `from`'s.
fn made_on(from: &Stack, h: &Held) -> Option<Stack> {
    let c = h.stack.as_ref()?;
    Some(Stack {
        patches: c.patches.clone(),
        base: c.base.clone().or(from.base.clone()),
        ..from.clone()
    })
}

/// The operations of the change work `h` of run `r` holds, from its stack
/// as it is: none for work that changes nothing of the stack, or a stack
/// gone.
pub fn of_work(db: &Db, r: &Run, h: &Held) -> Option<Ops> {
    let now = db.stack(&r.stack).ok()?;
    Some(of(db, now, &made_on(now, h)?))
}

/// The operations of the change work `h` of run `r` holds, as its review is
/// to see them, worked out as it lands: from the stack as it is; or,
/// chained on other work that waits for your review (an ask asked after it,
/// a reconcile with HEAD or a pull from Phabricator made on it), from the
/// stack that work makes, which the stack is once that work is accepted,
/// and only then is this one reviewed.
pub fn of_landed(db: &Db, r: &Run, h: &Held) -> Option<Ops> {
    let after = crate::ask::info(r)
        .and_then(|i| i.after)
        .or_else(|| crate::reconcile::after(r).map(String::from))
        .or_else(|| crate::ask::pull_after(r));
    let from = match after {
        Some(a) => crate::ask::work_stack(db, &a).ok()?,
        None => db.stack(&r.stack).ok()?.clone(),
    };
    Some(of(db, &from, &made_on(&from, h)?))
}

/// What each operation of the change work `h` of run `r` holds needs,
/// worked out as the work lands (`Ops::needs`), the checks reading only the
/// repository: none for work that changes nothing of the stack.
pub fn needs_of(db: &Db, r: &Run, h: &Held) -> Option<Needs> {
    let ops = of_landed(db, r, h)?;
    Some(ops.needs(db))
}

/// The operations of the change work `h` of run `r` holds, as its review
/// marks them: each on its own, once what each needs is stored for the
/// change from the stack as it is (`Held::needs`); else the change as one
/// (`Ops::as_one`), as before stacked marked each, and so is a change with
/// no operation to mark (`Ops::whole`). None for work that changes nothing
/// of the stack.
pub fn of_review(db: &Db, r: &Run, h: &Held) -> Option<Ops> {
    let ops = of_work(db, r, h)?;
    if ops.whole() || h.needs.as_ref().is_some_and(|n| n.fits(&ops)) {
        return Some(ops);
    }
    Some(ops.as_one())
}

/// What the review says of work that landed before stacked worked out, as
/// work lands, what each operation of its change needs: its change is one.
pub const WHOLE_BEFORE: &str = "This work landed before stacked worked out what each operation needs, so its change is accepted or rejected as a whole.";

/// What the review says of work whose operations' needs were worked out for
/// the stack as it was when it landed, which has changed since.
pub const WHOLE_CHANGED: &str =
    "What each operation needs was worked out for the stack as it was when this work landed, and the stack has changed since, so its change is accepted or rejected as a whole.";

/// Why the change work `h` of run `r` holds is marked as one, though it has
/// operations to mark, said as its review says it: nothing stored of what
/// each needs, as it landed before stacked worked it out; it could not be
/// worked out, and why; or what is stored is for other stacks. None when
/// each is marked on its own, or the change has none to mark.
pub fn whole_why(db: &Db, r: &Run, h: &Held) -> Option<String> {
    let ops = of_work(db, r, h)?;
    if ops.whole() {
        return None;
    }
    match &h.needs {
        None => Some(WHOLE_BEFORE.into()),
        Some(n) => match &n.failed {
            Some(why) => Some(format!(
                "What each operation needs could not be worked out when this work landed, so its change is accepted or rejected as a whole: {}.",
                why.trim_end_matches('.')
            )),
            None if n.of != ops.digest() => Some(WHOLE_CHANGED.into()),
            None => None,
        },
    }
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
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct With {
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub because: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub by: Vec<String>,
}

/// What each operation of a change needs, worked out once, as the work
/// lands (`Ops::needs`), and stored with it (`Held::needs`): for the stack
/// the change makes, every operation standing, each operation marked
/// rejected alone (`Alone`). An operation nothing needs, and nothing goes
/// with, has no entry. What the review decides from it holds for the whole
/// review: the two stacks it follows from are fixed once the work lands.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Needs {
    /// The operations it was worked out for (`Ops::digest`): those of
    /// another change, or of the same from another stack, are not these.
    #[serde(default)]
    pub of: String,
    /// Why it could not be worked out, if it could not: then nothing else is
    /// known of them, and the change is marked as a whole.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failed: Option<String>,
    /// Each operation that something needs, or that something goes with,
    /// rejected alone.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alone: Vec<Alone>,
}

/// Operation `key` rejected alone, every other one standing: those that
/// go with it, its patch as it was, or one of theirs, applying only on what
/// they leave as they were; the operations whose patches then no longer
/// apply, which need it; and the patch as it was that nothing makes apply,
/// if one is found.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Alone {
    #[serde(default)]
    pub key: String,
    /// The operations whose patches, as the change leaves them (or left as
    /// they were), no longer apply once it is rejected with what goes with
    /// it, the lowest first: each the first found not to apply once those
    /// before it are rejected too, with what each takes with it, as you
    /// would reject them first. One left as it was, which cannot be
    /// rejected, is the last seen.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub needed_by: Vec<String>,
    /// The operations that go with it, the nearest under the patch as it
    /// was that needs them first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub with: Vec<With>,
    /// The operation of the patch, as it was, that no other operation
    /// rejected with it makes apply.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stuck: Option<String>,
}

impl Needs {
    /// What is stored of operation `key` rejected alone: none for one that
    /// nothing needs, and that nothing goes with.
    pub fn alone(&self, key: &str) -> Option<&Alone> {
        self.alone.iter().find(|a| a.key == key)
    }

    /// Whether it was worked out, and for operations `ops`.
    pub fn fits(&self, ops: &Ops) -> bool {
        self.failed.is_none() && self.of == ops.digest()
    }

    /// Whether operation `by`'s patch was found not to apply once operation
    /// `key` is rejected.
    fn needs(&self, key: &str, by: &str) -> bool {
        self.alone(key).is_some_and(|a| a.needed_by.iter().any(|n| n == by))
    }
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
    /// was and the base too, or is marked as one (`as_one`): it is marked as
    /// a whole (`stack`), as a stacked of before marked every change.
    pub fn whole(&self) -> bool {
        self.marked().next().is_none()
    }

    /// The change marked as one, though it has operations to mark, as before
    /// stacked marked each on its own: with nothing stored of what each
    /// needs, none is marked alone. Its lines are no operations then.
    pub fn as_one(self) -> Ops {
        Ops {
            list: vec![],
            one: true,
            ..self
        }
    }

    /// Whether the change is marked as one, though it has operations to mark
    /// (`as_one`).
    pub fn one(&self) -> bool {
        self.one
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

    /// What they are, for what is worked out of them (`Needs::of`) to be
    /// known as theirs: their repository, the bases before and after, and
    /// each slot, by its operation and the patches it holds, which are their
    /// contents. The same change from the same stack is the same.
    pub fn digest(&self) -> String {
        let mut s = format!("{:?}\0{:?}\0{:?}", self.repo, self.bases.0, self.bases.1);
        for slot in &self.slots {
            s.push('\0');
            s.push_str(&match slot {
                Slot::Kept { key, old, new } => format!("kept {key} {old} {new}"),
                Slot::Was { key, old } => format!("was {key} {old}"),
                Slot::Is { key, new } => format!("is {key} {new}"),
            });
        }
        sha1_smol::Sha1::from(s).digest().to_string()
    }

    /// The patches they name, each version, taken from `db`: for a check
    /// made away from the screen, where there is no database.
    pub fn pool(&self, db: &Db) -> HashMap<String, Patch> {
        let mut out = HashMap::new();
        for s in &self.slots {
            let ids = match s {
                Slot::Kept { old, new, .. } => vec![old, new],
                Slot::Was { old, .. } => vec![old],
                Slot::Is { new, .. } => vec![new],
            };
            for id in ids {
                out.entry(id.clone()).or_insert_with(|| db.patch(id).clone());
            }
        }
        out
    }

    /// The first patch of `left` that does not apply, each on those before
    /// it, to `base`, by place; none when they all do. Why it cannot be
    /// told, when the stack cannot be checked.
    fn fails<P: Patches + ?Sized>(&self, db: &P, left: &[Placed], base: &Option<String>) -> Result<Option<usize>> {
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
        let patches: Vec<&Patch> = left.iter().map(|p| db.get(&p.id)).collect();
        crate::check::first_failure(Path::new(repo), base, &patches)
    }

    /// Whether the stack the operations `stands` leave applies: if not, the
    /// first patch that does not.
    pub fn failing<P: Patches + ?Sized>(&self, db: &P, stands: &BTreeSet<String>) -> Result<Option<Placed>> {
        let (left, base) = self.leave(stands);
        Ok(self.fails(db, &left, &base)?.map(|k| left[k].clone()))
    }

    /// The first slot of operation `key`: where it starts to change the
    /// stack; none for the base, which changes all of it.
    fn first_slot(&self, key: &str) -> Option<usize> {
        self.slots.iter().position(|s| match s {
            Slot::Kept { key: k, .. } | Slot::Was { key: k, .. } | Slot::Is { key: k, .. } => k == key,
        })
    }

    /// How deep in the stack operation `o` starts to change it: the base
    /// under every patch, then each by its first slot.
    fn depth(&self, o: &Op) -> usize {
        match o.kind {
            Kind::Base => 0,
            _ => self.first_slot(&o.key).map_or(0, |s| s + 1),
        }
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
    fn further<P: Patches + ?Sized>(&self, db: &P, stands: &BTreeSet<String>, q: &Placed, c: &str, new: bool) -> Result<bool> {
        let (left, base) = self.leave(stands);
        let at = left.iter().position(|p| p.slot == q.slot && p.id == q.id);
        Ok(match self.fails(db, &left, &base)? {
            None => true,
            Some(k) => at.is_some_and(|at| at < k) || (left[k].key == c && left[k].new == new),
        })
    }

    /// Whether rejecting operation `key` leaves every diff of the stack as
    /// it is: its patch's new version has the old one's diff, where it was,
    /// as a reword's has. Nothing needs it then, and nothing goes with it,
    /// told with no check.
    fn same_diffs<P: Patches + ?Sized>(&self, db: &P, key: &str) -> bool {
        self.slots.iter().any(|s| matches!(s, Slot::Kept { key: k, old, new } if k == key && db.get(old).diff == db.get(new).diff))
    }

    /// What each operation needs, worked out as the work lands, to be
    /// stored with it: the stack the change makes checked first; then, for
    /// each operation marked rejected alone, every other one standing, what
    /// goes with it, from the bottom of the stack up (`going_with`), and what
    /// needs it, from the top down (`needed`), each using what was found of
    /// those worked out before it, so that a chain of them is a few checks
    /// each, not one for every link (`Alone`). An operation whose rejection
    /// leaves every diff as it is is told with no check (`same_diffs`), and a
    /// change of only such needs none at all, as a reconcile with HEAD's,
    /// which takes messages alone. Its checks only read the repository; what
    /// each stack checked came to is kept for the process
    /// (`check::first_failure`). Why it could not be worked out, as it is
    /// stored, should a check fail.
    pub fn needs<P: Patches + ?Sized>(&self, db: &P) -> Needs {
        let of = self.digest();
        let failed = |why: String| Needs {
            of: of.clone(),
            failed: Some(why),
            alone: vec![],
        };
        let all: BTreeSet<String> = self.list.iter().map(|o| o.key.clone()).collect();
        let mut checked: Vec<&Op> = self.marked().filter(|o| !self.same_diffs(db, &o.key)).collect();
        if checked.is_empty() {
            return Needs { of, failed: None, alone: vec![] };
        }
        match self.failing(db, &all) {
            Err(e) => return failed(e.0),
            Ok(Some(q)) => return failed(format!("the stack the change makes does not apply: {} does not apply on the patches below it", self.patch_said(&q.key, q.new))),
            Ok(None) => {}
        }
        checked.sort_by_key(|o| self.depth(o));
        let mut with: HashMap<String, Vec<With>> = HashMap::new();
        let mut stuck: HashMap<String, String> = HashMap::new();
        for o in &checked {
            match self.going_with(db, &all, &o.key, &with) {
                Ok((w, s)) => {
                    with.insert(o.key.clone(), w);
                    if let Some(s) = s {
                        stuck.insert(o.key.clone(), s);
                    }
                }
                Err(e) => return failed(e.0),
            }
        }
        let mut needed: HashMap<String, Vec<String>> = HashMap::new();
        let mut takes: HashMap<String, Vec<String>> = HashMap::new();
        for o in checked.iter().rev() {
            match self.needed(db, &all, &o.key, &with, &takes) {
                Ok((n, t)) => {
                    needed.insert(o.key.clone(), n);
                    takes.insert(o.key.clone(), t);
                }
                Err(e) => return failed(e.0),
            }
        }
        let alone = self
            .marked()
            .filter_map(|o| {
                let a = Alone {
                    key: o.key.clone(),
                    needed_by: needed.remove(&o.key).unwrap_or_default(),
                    with: with.remove(&o.key).unwrap_or_default(),
                    stuck: stuck.remove(&o.key),
                };
                (!a.needed_by.is_empty() || !a.with.is_empty() || a.stuck.is_some()).then_some(a)
            })
            .collect();
        Needs { of, failed: None, alone }
    }

    /// What goes with operation `key` rejected alone, every other operation
    /// of `all` standing: while its patch as it was, or that of one going
    /// with it, is the first that does not apply, the operation under it
    /// that makes it so, the nearest first, and what goes with that one
    /// (`with`, those under it worked out first), its patch as it was
    /// needing them too. And the operation of the patch as it was that none
    /// makes apply, if one is found.
    fn going_with<P: Patches + ?Sized>(&self, db: &P, all: &BTreeSet<String>, key: &str, with: &HashMap<String, Vec<With>>) -> Result<(Vec<With>, Option<String>)> {
        let mut gone = vec![key.to_string()];
        let mut out: Vec<With> = Vec::new();
        loop {
            let now: BTreeSet<String> = all.iter().filter(|k| !gone.contains(k)).cloned().collect();
            let Some(q) = self.failing(db, &now)? else { return Ok((out, None)) };
            // As the change leaves it, or left as it was: what needs it,
            // which `needed` finds.
            if q.new || !gone.contains(&q.key) {
                return Ok((out, None));
            }
            let mut found = None;
            for c in self.under(&q, |o| now.contains(&o.key), &gone) {
                let mut then = now.clone();
                then.remove(&c);
                if self.further(db, &then, &q, &c, false)? {
                    found = Some(c);
                    break;
                }
            }
            let Some(c) = found else { return Ok((out, Some(q.key))) };
            out.push(With {
                key: c.clone(),
                because: q.key.clone(),
                by: vec![c.clone()],
            });
            gone.push(c.clone());
            for w in with.get(&c).into_iter().flatten() {
                if !gone.contains(&w.key) {
                    gone.push(w.key.clone());
                    out.push(w.clone());
                }
            }
        }
    }

    /// What needs operation `key`, rejected with what goes with it (`with`),
    /// every other operation of `all` standing: each operation whose patch,
    /// as the change leaves it (or left as it was), is then the first that
    /// does not apply, above it, the lowest first, seen past by taking it as
    /// rejected too, with all it takes with it (`takes`, those above worked
    /// out first), as you would reject it first. A patch as it was that does
    /// not apply, or one left as it was, which cannot be rejected, is the
    /// last seen. And all it takes with it so: what goes with it, what needs
    /// it, and what those take.
    fn needed<P: Patches + ?Sized>(
        &self,
        db: &P,
        all: &BTreeSet<String>,
        key: &str,
        with: &HashMap<String, Vec<With>>,
        takes: &HashMap<String, Vec<String>>,
    ) -> Result<(Vec<String>, Vec<String>)> {
        let mut gone: Vec<String> = std::iter::once(key.to_string()).chain(with.get(key).into_iter().flatten().map(|w| w.key.clone())).collect();
        let floor = self.first_slot(key);
        let mut needed_by = Vec::new();
        loop {
            let now: BTreeSet<String> = all.iter().filter(|k| !gone.contains(k)).cloned().collect();
            let Some(q) = self.failing(db, &now)? else { break };
            if !q.new || floor.is_some_and(|f| q.slot < f) {
                break;
            }
            needed_by.push(q.key.clone());
            if !self.op(&q.key).is_some_and(Op::marked) || gone.contains(&q.key) {
                break;
            }
            gone.push(q.key.clone());
            let more: Vec<String> = takes.get(&q.key).into_iter().flatten().cloned().chain(with.get(&q.key).into_iter().flatten().map(|w| w.key.clone())).collect();
            for k in more {
                if !gone.contains(&k) {
                    gone.push(k);
                }
            }
        }
        Ok((needed_by, gone[1..].to_vec()))
    }

    /// Operation `key`, which stands among `stands`, rejected, as what is
    /// stored of it says (`needs`): alone, or with the operations that go
    /// with it and still stand (each said, to ask first); or why not. Refused
    /// while an operation that stands needs it, or a patch left as it was
    /// does, the lowest first; or while its patch as it was applies on
    /// nothing the others would leave. Nothing is checked: two rejections
    /// that break a patch together, though neither does alone, are found as
    /// the review closes.
    pub fn reject(&self, needs: &Needs, stands: &BTreeSet<String>, key: &str) -> std::result::Result<Vec<With>, Refused> {
        if let Some(why) = &needs.failed {
            return Err(Refused::Unchecked(why.clone()));
        }
        let Some(a) = needs.alone(key) else { return Ok(vec![]) };
        if let Some(by) = a.needed_by.iter().find(|n| stands.contains(*n)) {
            let going = a.with.iter().map(|w| w.key.clone()).filter(|k| stands.contains(k));
            let on = std::iter::once(key.to_string()).chain(going).collect();
            return Err(Refused::Needed { by: by.clone(), on });
        }
        if let Some(k) = &a.stuck {
            return Err(Refused::Stuck { key: k.clone(), new: false });
        }
        Ok(a.with.iter().filter(|w| stands.contains(&w.key)).cloned().collect())
    }

    /// Operation `key`, rejected, accepted, as what is stored says
    /// (`needs`): alone, or with the operations rejected whose patches as
    /// they were apply only on what it, or one coming with it, leaves as it
    /// was (each said, to ask first); or why not. Refused while its patch, or
    /// that of one coming with it, needs an operation that is rejected, the
    /// nearest under it named. Taken from the bottom of the stack up, as a
    /// check finds them.
    pub fn accept(&self, needs: &Needs, stands: &BTreeSet<String>, key: &str) -> std::result::Result<Vec<With>, Refused> {
        if let Some(why) = &needs.failed {
            return Err(Refused::Unchecked(why.clone()));
        }
        let mut order: Vec<&Op> = self.marked().collect();
        order.sort_by_key(|o| self.depth(o));
        let mut coming = vec![key.to_string()];
        let mut with = Vec::new();
        loop {
            let now: BTreeSet<String> = stands.iter().cloned().chain(coming.iter().cloned()).collect();
            let mut comes = None;
            for o in &order {
                if coming.contains(&o.key) {
                    let rejected = order.iter().filter(|a| !now.contains(&a.key) && needs.needs(&a.key, &o.key));
                    if let Some(a) = rejected.max_by_key(|a| self.depth(a)) {
                        return Err(Refused::Needs {
                            op: o.key.clone(),
                            on: a.key.clone(),
                        });
                    }
                } else if !now.contains(&o.key) && needs.alone(&o.key).is_some_and(|x| x.with.iter().any(|w| coming.contains(&w.key))) {
                    comes = Some(o.key.clone());
                    break;
                }
            }
            let Some(z) = comes else { return Ok(with) };
            with.push(With {
                key: z.clone(),
                because: z.clone(),
                by: coming.clone(),
            });
            coming.push(z);
        }
    }

    /// Whether the stack the operations `stands` leave applies, as closing
    /// checks it before anything is done: if not, why, as git says it of the
    /// first patch that does not (`check::applies`), then what that patch
    /// needs (`needs_said`). Its checks only read the repository: the review
    /// makes it away from the screen.
    pub fn closing<P: Patches + ?Sized>(&self, db: &P, stands: &BTreeSet<String>) -> Result<Option<String>> {
        let (left, base) = self.leave(stands);
        let Some(k) = self.fails(db, &left, &base)? else { return Ok(None) };
        let patches: Vec<&Patch> = left.iter().map(|p| db.get(&p.id)).collect();
        let git = match (&self.repo, &base) {
            (Some(repo), Some(base)) => crate::check::applies(Path::new(repo), base, &patches).err().map(|e| e.0),
            _ => None,
        };
        let said = git.unwrap_or_else(|| format!("patch {} ({}) does not apply", k + 1, patches[k].title()));
        Ok(Some(match self.needs_said(db, stands, &left[k])? {
            Some(n) => format!("{said}; {n}"),
            None => said,
        }))
    }

    /// What the patch placed `q`, the first of the stack the operations
    /// `stands` leave not to apply, needs, said: as the change leaves it (or
    /// left as it was), the operation rejected nearest under it whose
    /// acceptance gets further (`patch 5's new version needs patch 4's
    /// operation, which is rejected`); as it was, the operation standing
    /// nearest under it whose rejection does (`patch 5 as it was needs patch
    /// 4's operation rejected too`). None when no one operation does.
    fn needs_said<P: Patches + ?Sized>(&self, db: &P, stands: &BTreeSet<String>, q: &Placed) -> Result<Option<String>> {
        for c in self.under(q, |o| stands.contains(&o.key) != q.new, &[]) {
            let mut then = stands.clone();
            match q.new {
                true => then.insert(c.clone()),
                false => then.remove(&c),
            };
            if self.further(db, &then, q, &c, q.new)? {
                return Ok(Some(match q.new {
                    true => format!("{} needs {}, which is rejected", self.patch_said(&q.key, true), self.said(&c)),
                    false => format!("{} needs {} rejected too", self.patch_said(&q.key, false), self.said(&c)),
                }));
            }
        }
        Ok(None)
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
    /// them and what is stored says (`needs`), a sentence each: what it
    /// needs, then what needs it, or, once it is rejected, whether it can be
    /// accepted alone.
    pub fn pane_said(&self, needs: &Needs, marks: &BTreeMap<String, bool>, key: &str) -> Vec<String> {
        let Some(o) = self.op(key) else { return vec![] };
        let stands = self.standing(marks);
        let mut out = Vec::new();
        if !o.marked() {
            out.push("The change leaves this patch as it is: there is nothing to accept or reject.".to_string());
        }
        // What it needs: each operation that stands, of which its patch,
        // standing, is the first found not to apply once that one alone is
        // rejected, nothing going with that one.
        if stands.contains(key) {
            for x in self.marked().filter(|x| x.key != key && stands.contains(&x.key)) {
                if let Some(a) = needs.alone(&x.key)
                    && a.with.is_empty()
                    && a.needed_by.iter().find(|n| stands.contains(*n)).is_some_and(|n| n == key)
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
            say(match self.accept(needs, &stands, key) {
                Ok(with) if with.is_empty() => "Nothing it needs is rejected: it can be accepted alone".to_string(),
                Ok(with) => {
                    let goes: Vec<String> = with.iter().map(|w| w.key.clone()).collect();
                    format!("{} So this operation is accepted only with {}", self.with_all_said(&with), self.listed(&goes))
                }
                Err(Refused::Needs { op, on }) => format!("{} applies only {}: accept {} before this one", patch(&op, true), self.on_said(&on), self.said(&on)),
                Err(Refused::Stuck { key: q, new }) => format!("{} does not apply on what the other operations leave in the stack, {alone}", patch(&q, new)),
                Err(Refused::Unchecked(why)) => format!("Whether this operation can be accepted alone cannot be told, as the stack cannot be checked: {why}"),
                Err(Refused::Needed { .. }) => String::new(),
            });
            return out;
        }
        let alone = "so this operation cannot be rejected alone";
        say(match self.reject(needs, &stands, key) {
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
            Err(Refused::Needs { .. }) => String::new(),
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
        let needs = ops.needs(&db);
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
        assert_eq!(ops.reject(&needs, &all, "op llllllllllll"), Err(Refused::Needed { by: "op nnnnnnnnnnnn".into(), on: vec!["op llllllllllll".into()] }));
        assert_eq!(
            ops.refused_said("op llllllllllll", false, &ops.reject(&needs, &all, "op llllllllllll").unwrap_err()),
            "patch 3, which the change adds, applies only on patch 2's new version: reject patch 3's operation first"
        );
        // 4 first, then 2: alone each time.
        assert_eq!(ops.reject(&needs, &all, "op nnnnnnnnnnnn"), Ok(vec![]));
        let mut no4 = all.clone();
        no4.remove("op nnnnnnnnnnnn");
        assert_eq!(ops.reject(&needs, &no4, "op llllllllllll"), Ok(vec![]));
        // Accepting 4 while 2 is rejected: it needs 2.
        no4.remove("op llllllllllll");
        assert_eq!(ops.accept(&needs, &no4, "op nnnnnnnnnnnn"), Err(Refused::Needs { op: "op nnnnnnnnnnnn".into(), on: "op llllllllllll".into() }));
        assert_eq!(ops.accept(&needs, &no4, "op llllllllllll"), Ok(vec![]));
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
        let needs = ops.needs(&db);
        let all = ops.standing(&BTreeMap::new());
        // 1 is needed by 2's new version; 2, rejected alone, leaves 2 as it
        // was on 1's new version, which it does not apply on: 1 goes with it.
        assert_eq!(ops.reject(&needs, &all, "op kkkkkkkkkkkk"), Err(Refused::Needed { by: "op llllllllllll".into(), on: vec!["op kkkkkkkkkkkk".into()] }));
        let with = ops.reject(&needs, &all, "op llllllllllll").unwrap();
        assert_eq!(with, [With { key: "op kkkkkkkkkkkk".into(), because: "op llllllllllll".into(), by: vec!["op kkkkkkkkkkkk".into()] }]);
        assert_eq!(ops.with_said(&with[0]), "Patch 2 as it was applies only on patch 1 as it was.");
        // Both rejected, accepting 1 again brings 2 with it, as 2 as it was
        // does not apply on it.
        let none = ops.standing(&[("op kkkkkkkkkkkk".to_string(), false), ("op llllllllllll".to_string(), false)].into());
        let with = ops.accept(&needs, &none, "op kkkkkkkkkkkk").unwrap();
        assert_eq!(with.iter().map(|w| w.key.as_str()).collect::<Vec<_>>(), ["op llllllllllll"]);
        assert_eq!(ops.accept(&needs, &none, "op llllllllllll"), Err(Refused::Needs { op: "op llllllllllll".into(), on: "op kkkkkkkkkkkk".into() }));
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
        let needs = ops.needs(&db);
        on_their_lines(&db, &ops, &before, &after);
        let moved: Vec<&Op> = ops.marked().collect();
        assert_eq!((moved.len(), moved[0].kind), (1, Kind::Move));
        let ids = |stands: &BTreeSet<String>| ops.leave(stands).0.into_iter().map(|p| p.id).collect::<Vec<_>>();
        assert_eq!(ids(&ops.standing(&BTreeMap::new())), after.patches);
        assert_eq!(ids(&ops.standing(&[(moved[0].key.clone(), false)].into())), before.patches);
        assert_eq!(ops.reject(&needs, &ops.standing(&BTreeMap::new()), &moved[0].key), Ok(vec![]));
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
        let needs = ops.needs(&db);
        on_their_lines(&db, &ops, &before, &after);
        assert_eq!(ops.list.iter().map(|o| o.kind).collect::<Vec<_>>(), [Kind::Base, Kind::Modify]);
        let all = ops.standing(&BTreeMap::new());
        assert_eq!(ops.leave(&all).1.as_deref(), Some(on.as_str()));
        assert_eq!(ops.leave(&ops.standing(&[(BASE.to_string(), false)].into())).1.as_deref(), Some(base.as_str()));
        let refused = ops.reject(&needs, &all, BASE).unwrap_err();
        assert_eq!(ops.refused_said(BASE, false, &refused), "patch 1's new version applies only on the new base: reject patch 1's operation first");
        // Patch 1 as it was, on the new base: it goes with the base's.
        let with = ops.reject(&needs, &all, "op kkkkkkkkkkkk").unwrap();
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
        let needs = ops.needs(&db);
        assert_eq!(ops.marked().count(), 20);
        let all = ops.standing(&BTreeMap::new());
        let said = ops.pane_said(&needs, &BTreeMap::new(), &key_of(&changes[19]));
        assert_eq!(said, ["Nothing else needs this operation: it can be rejected alone."]);
        for c in &changes {
            assert_eq!(ops.reject(&needs, &all, &key_of(c)), Ok(vec![]));
        }
        // Every other one rejected, the stack still applies.
        let marks: BTreeMap<String, bool> = changes.iter().step_by(2).map(|c| (key_of(c), false)).collect();
        assert_eq!(ops.failing(&db, &ops.standing(&marks)).unwrap(), None);
    }

    #[test]
    fn of_twenty_operations_each_needing_the_one_under_it_and_each_patch_as_it_was_the_one_under_it_as_it_was_all_go_together() {
        // Three lines apart: each patch has the one under it in sight.
        let (_r, _t, db, ops, changes) = every_one(20, 63, |k| 3 * (k + 1));
        let needs = ops.needs(&db);
        let key = |k: usize| key_of(&changes[k]);
        let all = ops.standing(&BTreeMap::new());
        // The top's, rejected, takes every one under it with it, the nearest
        // first.
        let with = ops.reject(&needs, &all, &key(19)).unwrap();
        assert_eq!(with.iter().map(|w| w.key.clone()).collect::<Vec<_>>(), (0..19).rev().map(key).collect::<Vec<_>>());
        assert_eq!(ops.with_said(&with[0]), "Patch 20 as it was applies only on patch 19 as it was.");
        assert_eq!(
            ops.with_all_said(&with),
            "Patch 20 as it was applies only on patch 19 as it was. Patch 19 as it was applies only on patch 18 as it was. Patch 18 as it was applies only on patch 17 as it was. And so on, for 16 more."
        );
        assert_eq!(ops.listed(&with.iter().map(|w| w.key.clone()).collect::<Vec<_>>()), "patch 19's operation, patch 18's operation and 17 other operations");
        // One in the middle: the one above it needs it.
        let Err(Refused::Needed { by, on }) = ops.reject(&needs, &all, &key(9)) else { panic!() };
        assert_eq!((by, on.len()), (key(10), 10));
        // Every one rejected: the bottom's, accepted, brings every one above
        // it; the top's needs the one under it.
        let none = ops.standing(&changes.iter().map(|c| (key_of(c), false)).collect());
        assert_eq!(ops.accept(&needs, &none, &key(0)).unwrap().len(), 19);
        assert_eq!(ops.accept(&needs, &none, &key(19)), Err(Refused::Needs { op: key(19), on: key(18) }));
    }

    #[test]
    fn a_stack_that_cannot_be_checked_marks_none_of_its_operations_alone() {
        let p1 = patch("kkkkkkkkkkkk", "one", diff("f.txt", "a\n", "b\n"));
        let p1b = patch("kkkkkkkkkkkk", "one", diff("f.txt", "a\n", "c\n"));
        let (_t, db) = db_with(&[p1.clone(), p1b.clone()]);
        let before = Stack { base: None, repo: None, ..stack("/nowhere", "x", &[&p1]) };
        let after = Stack { patches: vec![p1b.id.clone()], ..before.clone() };
        let ops = of(&db, &before, &after);
        let needs = ops.needs(&db);
        let all = ops.standing(&BTreeMap::new());
        let Err(Refused::Unchecked(why)) = ops.reject(&needs, &all, "op kkkkkkkkkkkk") else { panic!() };
        assert!(why.contains("is read-only"), "{why}");
    }

    #[test]
    fn what_each_operation_needs_is_worked_out_once_and_kept_as_stored_for_these_operations_alone() {
        let (_r, repo, base) = repo(&[("f.txt", &text(12, &[]))]);
        let f = |changed: &[(usize, &str)], was: &[(usize, &str)]| diff("f.txt", &text(12, was), &text(12, changed));
        let p1 = patch("kkkkkkkkkkkk", "one", f(&[(1, "a")], &[]));
        let p2 = patch("llllllllllll", "two", f(&[(1, "a"), (5, "b")], &[(1, "a")]));
        let p1b = patch("kkkkkkkkkkkk", "one", f(&[(1, "a"), (4, "x")], &[]));
        let p2b = patch("llllllllllll", "two", f(&[(1, "a"), (4, "x"), (5, "b")], &[(1, "a"), (4, "x")]));
        let (_t, db) = db_with(&[p1.clone(), p2.clone(), p1b.clone(), p2b.clone()]);
        let ops = of(&db, &stack(&repo, &base, &[&p1, &p2]), &stack(&repo, &base, &[&p1b, &p2b]));
        let needs = ops.needs(&db);
        // Patch 2's new version needs patch 1's; patch 2 as it was applies
        // only on patch 1 as it was.
        assert_eq!(
            needs,
            Needs {
                of: ops.digest(),
                failed: None,
                alone: vec![
                    Alone {
                        key: "op kkkkkkkkkkkk".into(),
                        needed_by: vec!["op llllllllllll".into()],
                        ..Default::default()
                    },
                    Alone {
                        key: "op llllllllllll".into(),
                        with: vec![With { key: "op kkkkkkkkkkkk".into(), because: "op llllllllllll".into(), by: vec!["op kkkkkkkkkkkk".into()] }],
                        ..Default::default()
                    },
                ],
            }
        );
        // Stored as JSON and read back, the same.
        let stored = serde_json::to_string(&needs).unwrap();
        assert_eq!(serde_json::from_str::<Needs>(&stored).unwrap(), needs);
        assert!(needs.fits(&ops));
        // The same change from another stack, or another change, is not
        // what it was worked out for.
        let other = of(&db, &stack(&repo, &base, &[&p1]), &stack(&repo, &base, &[&p1b, &p2b]));
        assert!(!needs.fits(&other));
        assert!(!Needs { failed: Some("why".into()), ..needs.clone() }.fits(&ops));
        // Deciding from it checks nothing.
        let made = crate::check::made_here();
        let all = ops.standing(&BTreeMap::new());
        assert!(ops.reject(&needs, &all, "op kkkkkkkkkkkk").is_err());
        assert_eq!(ops.reject(&needs, &all, "op llllllllllll").unwrap().len(), 1);
        assert!(!ops.pane_said(&needs, &BTreeMap::new(), "op llllllllllll").is_empty());
        assert_eq!(crate::check::made_here(), made, "no check");
    }

    #[test]
    fn a_change_of_messages_alone_needs_no_check_to_work_out_that_nothing_needs_any() {
        let (_r, repo, base) = repo(&[("f.txt", &text(12, &[]))]);
        let f = |changed: &[(usize, &str)], was: &[(usize, &str)]| diff("f.txt", &text(12, was), &text(12, changed));
        let p1 = patch("kkkkkkkkkkkk", "one", f(&[(1, "a")], &[]));
        let p2 = patch("llllllllllll", "two", f(&[(1, "a"), (5, "b")], &[(1, "a")]));
        let reworded = |p: &Patch, title: &str| Patch::new(p.change.clone(), None, None, title.into(), p.diff.clone());
        let (p1b, p2b) = (reworded(&p1, "one, said better"), reworded(&p2, "two, said better"));
        let (_t, db) = db_with(&[p1.clone(), p2.clone(), p1b.clone(), p2b.clone()]);
        let ops = of(&db, &stack(&repo, &base, &[&p1, &p2]), &stack(&repo, &base, &[&p1b, &p2b]));
        assert_eq!(ops.list.iter().map(|o| o.kind).collect::<Vec<_>>(), [Kind::Reword, Kind::Reword]);
        let made = crate::check::made_here();
        let needs = ops.needs(&db);
        assert_eq!(crate::check::made_here(), made, "no check");
        assert_eq!((needs.failed.as_deref(), needs.alone.len()), (None, 0));
        assert!(needs.fits(&ops));
    }

    /// The file of `two_rejections_…`: `A1` to `A30`, `marks` giving lines
    /// their own (`(3, "P3")`), and the block X, Y, Z (its Y as `y` says)
    /// after each line `blocks` names.
    fn lettered(marks: &[(usize, &str)], blocks: &[usize], y: &str) -> String {
        let mut out = String::new();
        for k in 1..=30 {
            match marks.iter().find(|(at, _)| *at == k) {
                Some((_, m)) => out += &format!("{m}\n"),
                None => out += &format!("A{k}\n"),
            }
            if blocks.contains(&k) {
                out += &format!("X\n{y}\nZ\n");
            }
        }
        out
    }

    /// As `diff`, with one line of context around each change.
    fn diff_one(name: &str, was: &str, is: &str) -> String {
        let tmp = tempfile::tempdir().unwrap();
        let (a, b) = (tmp.path().join("a"), tmp.path().join("b"));
        std::fs::write(&a, was).unwrap();
        std::fs::write(&b, is).unwrap();
        let out = std::process::Command::new("git").args(["diff", "--no-index", "--no-color", "-U1"]).arg(&a).arg(&b).output().unwrap();
        let text = String::from_utf8(out.stdout).unwrap();
        let body: String = text.lines().skip_while(|l| !l.starts_with("@@")).map(|l| format!("{l}\n")).collect();
        format!("diff --git a/{name} b/{name}\n--- a/{name}\n+++ b/{name}\n{body}")
    }

    #[test]
    fn two_rejections_that_together_break_a_patch_neither_breaks_alone_are_found_by_closing_s_check() {
        let (_r, repo, base) = repo(&[("f.txt", &lettered(&[], &[], "Y"))]);
        // Patches 1 and 2, made again, each add the block X, Y, Z; patch 3's
        // new version changes its Y, which it finds in either.
        let (p_old, q_old, r_old) = ([(3, "P3")], [(3, "P3"), (15, "Q15")], [(3, "P3"), (15, "Q15"), (28, "R28")]);
        let p1 = patch("kkkkkkkkkkkk", "one", diff("f.txt", &lettered(&[], &[], "Y"), &lettered(&p_old, &[], "Y")));
        let p2 = patch("llllllllllll", "two", diff("f.txt", &lettered(&p_old, &[], "Y"), &lettered(&q_old, &[], "Y")));
        let p3 = patch("mmmmmmmmmmmm", "three", diff("f.txt", &lettered(&q_old, &[], "Y"), &lettered(&r_old, &[], "Y")));
        let p1b = patch("kkkkkkkkkkkk", "one", diff("f.txt", &lettered(&[], &[], "Y"), &lettered(&p_old, &[10], "Y")));
        let p2b = patch("llllllllllll", "two", diff("f.txt", &lettered(&p_old, &[10], "Y"), &lettered(&q_old, &[10, 20], "Y")));
        let p3b = patch("mmmmmmmmmmmm", "three", diff_one("f.txt", &lettered(&q_old, &[10, 20], "Y"), &lettered(&r_old, &[10, 20], "Y").replace("A20\nX\nY\n", "A20\nX\nY5\n")));
        let (_t, db) = db_with(&[p1.clone(), p2.clone(), p3.clone(), p1b.clone(), p2b.clone(), p3b.clone()]);
        let ops = of(&db, &stack(&repo, &base, &[&p1, &p2, &p3]), &stack(&repo, &base, &[&p1b, &p2b, &p3b]));
        let needs = ops.needs(&db);
        // Rejected alone, none breaks another: nothing is stored.
        assert_eq!((needs.failed.as_deref(), needs.alone.len()), (None, 0), "{needs:?}");
        let all = ops.standing(&BTreeMap::new());
        assert_eq!(ops.reject(&needs, &all, "op kkkkkkkkkkkk"), Ok(vec![]));
        let mut one = all.clone();
        one.remove("op kkkkkkkkkkkk");
        assert_eq!(ops.reject(&needs, &one, "op llllllllllll"), Ok(vec![]));
        // Each alone, the stack applies; both, patch 3's new version finds no
        // Y to change, and closing says so, and what it needs.
        assert_eq!(ops.closing(&db, &one).unwrap(), None);
        let mut both = one.clone();
        both.remove("op llllllllllll");
        let said = ops.closing(&db, &both).unwrap().unwrap();
        assert!(said.starts_with("patch 3 (three) does not apply to the patches below it ("), "{said}");
        assert!(said.ends_with("); patch 3's new version needs patch 2's operation, which is rejected"), "{said}");
    }
}
