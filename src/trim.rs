//! A new database from an old one, the start of its history left out: what
//! there was before a rev is the new file's start, as if it had always been
//! so, and every record from that rev on follows, its revs made the new
//! file's. The old file is only read; the new one is written whole, read
//! back, and checked to hold every stack as the old one does.
//!
//! The start holds each stack as it was (and any earlier state a later
//! record names: the parent of a later edit, where an undo goes, where an
//! ask started), the patches they need, the comments as they stood, the
//! read marks standing, each stack's filter, and the runs still going (with
//! what their agent had written). Runs that had landed, earlier states no
//! one names, earlier versions of comments: that is the history left out.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::io::Write;
use std::path::Path;

use crate::db::{Db, Error, Head, Mark, Note, Record, Result, Run, RunState, Stack, VERSION, err, now};

/// What a trim made.
#[derive(Debug, Clone, PartialEq)]
pub struct Trimmed {
    /// The first rev of the old file's history kept.
    pub from: usize,
    /// How many records the new file has, and the old had.
    pub records: usize,
    pub was: usize,
    pub stacks: Vec<String>,
    /// The lines of the old file kept that could not be read, left out.
    pub broken: usize,
}

impl Trimmed {
    pub fn said(&self, dst: &Path) -> String {
        let mut said = format!(
            "wrote {}: {} records, of {}; the history from @{} on, before it its start; stacks {}",
            dst.display(),
            self.records,
            self.was,
            self.from,
            if self.stacks.is_empty() { "none".to_string() } else { self.stacks.join(", ") }
        );
        if self.broken > 0 {
            said += &format!("; {} lines that could not be read left out", self.broken);
        }
        said
    }
}

/// The new file's records, as they are made: what each old rev is now.
struct New {
    records: Vec<Record>,
    revs: HashMap<usize, usize>,
    patches: HashSet<String>,
}

impl New {
    /// Adds `rec`, the old file's `old` if it was one of its: its new rev.
    fn push(&mut self, rec: Record, old: Option<usize>) -> usize {
        self.records.push(rec);
        let rev = self.records.len();
        if let Some(o) = old {
            self.revs.insert(o, rev);
        }
        rev
    }

    fn rev(&self, old: usize) -> Option<usize> {
        self.revs.get(&old).copied()
    }

    /// Adds the patches `ids` of `db` the new file does not have yet.
    fn patches(&mut self, db: &Db, ids: &[String]) {
        for id in ids {
            if self.patches.insert(id.clone()) {
                self.records.push(Record::Patch(db.patch(id).clone()));
            }
        }
    }
}

/// The revs a record names.
fn named(rec: &Record) -> Vec<usize> {
    match rec {
        Record::Stack(s) => s.parent.into_iter().collect(),
        Record::Head(h) => h.rev.into_iter().collect(),
        Record::Run(r) => {
            let mut out: Vec<usize> = r.rev.into_iter().collect();
            if let Some(from) = r.ask.as_ref().and_then(|a| a.get("from")).and_then(|f| f.as_u64()) {
                out.push(from as usize);
            }
            out
        }
        Record::Read(m) => m.undo.into_iter().collect(),
        _ => vec![],
    }
}

/// The patches a run's ask names: the one it changes, the commits it made.
fn run_patches(r: &Run) -> Vec<String> {
    // Those of its change waiting for review.
    let mut out: Vec<String> = r.held.iter().flat_map(|h| h.stack.iter()).flat_map(|c| c.patches.clone()).collect();
    let Some(a) = &r.ask else { return out };
    if let Some(id) = a.get("patch").and_then(|p| p.get(0)).and_then(|p| p.as_str()) {
        out.push(id.to_string());
    }
    for c in a.get("commits").and_then(|c| c.as_array()).into_iter().flatten() {
        if let Some(id) = c.get(1).and_then(|p| p.as_str()) {
            out.push(id.to_string());
        }
    }
    out
}

/// `r` with its revs the new file's.
fn run_now(r: &Run, new: &New) -> Run {
    let mut r = r.clone();
    r.rev = r.rev.and_then(|old| new.rev(old));
    if let Some(serde_json::Value::Object(a)) = &mut r.ask
        && let Some(from) = a.get("from").and_then(|f| f.as_u64())
        && let Some(to) = new.rev(from as usize)
    {
        a.insert("from".into(), to.into());
    }
    r
}

/// Writes at `dst` (which must not be there) the database at `src` with its
/// history before rev `from` left out, `from` the end (the last rev and
/// one) to keep only what is there now.
pub fn trim(src: &Path, dst: &Path, from: Option<usize>) -> Result<Trimmed> {
    if dst.exists() {
        return err(format!("{} is there already: a trim writes a new file", dst.display()));
    }
    let bytes = std::fs::read(src).map_err(|e| Error(format!("{}: {e}", src.display())))?;
    let all = Db::from_bytes(src, &bytes)?;
    let len = all.last_rev();
    let from = from.unwrap_or(len + 1);
    if from < 1 || from > len + 1 {
        return err(format!("@{from} is not a rev of {} (its last is @{len})", src.display()));
    }
    // The state just before `from`: the first `from - 1` lines read.
    let cut = bytes
        .iter()
        .enumerate()
        .filter(|(_, b)| **b == b'\n')
        .nth(from.saturating_sub(2))
        .map_or(0, |(i, _)| i + 1);
    let before = if from == 1 { Db::from_bytes(src, &[])? } else { Db::from_bytes(src, &bytes[..cut])? };
    let later = &all.records()[from - 1..];

    // The runs kept: going at the cut, or with records after it.
    let mut kept_runs: BTreeSet<String> = later
        .iter()
        .filter_map(|r| match r {
            Record::Run(r) => Some(r.id.clone()),
            Record::Event { run, .. } => Some(run.clone()),
            _ => None,
        })
        .collect();
    for r in before.runs() {
        if r.state == RunState::Started || crate::held::waiting(&before, r) {
            kept_runs.insert(r.id.clone());
        }
    }
    // The earlier states something kept names: later records, kept runs.
    let mut wanted: BTreeSet<usize> = later.iter().flat_map(named).filter(|&r| r < from).collect();
    for id in &kept_runs {
        if let Some(r) = before.run(id) {
            wanted.extend(named(&Record::Run(r.clone())).into_iter().filter(|&r| r < from));
        }
    }
    let heads: BTreeMap<String, Option<usize>> = before
        .names()
        .into_iter()
        .map(|n| (n.to_string(), before.head(n).map(|(r, _)| r)))
        .collect();
    let mut nodes: BTreeSet<usize> = heads.values().flatten().copied().collect();
    nodes.extend(wanted.iter().filter(|&&r| matches!(before.rev(r), Some(Record::Stack(_)))));

    let mut new = New {
        records: vec![Record::Stacked { version: VERSION }],
        revs: HashMap::new(),
        patches: HashSet::new(),
    };
    // The stacks, in the order they were made, a parent that is kept kept
    // as one; then each name moved where it was.
    let mut last_of: HashMap<String, usize> = HashMap::new();
    for &old in &nodes {
        let Ok(s) = before.node(old) else { continue };
        new.patches(&before, &s.patches);
        let parent = s.parent.and_then(|p| new.rev(p));
        let op = if heads.get(&s.name) == Some(&Some(old)) {
            format!("{} (the start of a trimmed history)", s.op)
        } else {
            s.op.clone()
        };
        new.push(
            Record::Stack(Stack {
                parent,
                op,
                ..s.clone()
            }),
            Some(old),
        );
        last_of.insert(s.name.clone(), old);
    }
    for (name, head) in &heads {
        let Some(&last) = last_of.get(name) else { continue };
        if *head != Some(last) {
            new.push(
                Record::Head(Head {
                    name: name.clone(),
                    rev: head.and_then(|h| new.rev(h)),
                    op: "the start of a trimmed history".into(),
                    time: now(),
                }),
                None,
            );
        }
    }
    // The comments as they stood, on what is kept.
    let mut changes: HashSet<String> = heads.keys().map(|n| crate::db::stack_change(n)).collect();
    for id in &new.patches {
        changes.insert(before.patch(id).change.clone());
    }
    for r in later {
        match r {
            Record::Patch(p) => {
                changes.insert(p.change.clone());
            }
            Record::Stack(s) => {
                changes.insert(crate::db::stack_change(&s.name));
            }
            _ => {}
        }
    }
    let mut notes: Vec<&Note> = changes.iter().flat_map(|c| before.notes(c)).collect();
    notes.sort_by_key(|n| before.note_history(&n.change, &n.id).last().map_or(0, |(r, _)| *r));
    for n in notes {
        new.push(Record::Comment(n.clone()), None);
        // Collapsed, it stays so.
        if before.collapsed(&n.change, &n.id) {
            new.push(
                Record::Collapse {
                    change: n.change.clone(),
                    id: n.id.clone(),
                    collapsed: true,
                },
                None,
            );
        }
    }
    // Each stack's filter and choices; the read marks standing.
    for name in heads.keys() {
        if let Some(show) = before.view(name) {
            new.push(
                Record::View {
                    stack: name.clone(),
                    show: show.to_string(),
                },
                None,
            );
        }
        for (of, pick) in before.choices(name) {
            new.push(
                Record::Choice {
                    stack: name.clone(),
                    of: of.to_string(),
                    pick: pick.to_string(),
                },
                None,
            );
        }
    }
    let mut marked: Vec<(usize, &Mark)> = changes.iter().flat_map(|c| before.marks(c)).collect();
    marked.sort_by_key(|(r, _)| *r);
    for (old, m) in marked {
        new.push(Record::Read(m.clone()), Some(old));
    }
    // The runs kept, as they stood, and what their agent had written.
    for id in &kept_runs {
        let Some(r) = before.run(id) else { continue };
        new.patches(&before, &run_patches(r));
        new.push(Record::Run(run_now(r, &new)), None);
        // Its review as saved, what it holds waiting.
        if let Some(review) = before.review(id) {
            new.push(
                Record::Review {
                    run: id.clone(),
                    review: review.clone(),
                },
                None,
            );
        }
        for e in before.events(id) {
            new.push(
                Record::Event {
                    run: id.clone(),
                    event: e.clone(),
                },
                None,
            );
        }
    }
    // Then the history kept, its revs the new file's.
    let mut broken = 0;
    for (k, rec) in later.iter().enumerate() {
        let old = from + k;
        let rec = match rec {
            Record::Stacked { .. } => continue,
            Record::Broken => {
                broken += 1;
                continue;
            }
            Record::Patch(p) => {
                if !new.patches.insert(p.id.clone()) {
                    continue;
                }
                Record::Patch(p.clone())
            }
            Record::Stack(s) => {
                new.patches(&all, &s.patches);
                Record::Stack(Stack {
                    parent: s.parent.and_then(|p| new.rev(p)),
                    ..s.clone()
                })
            }
            Record::Head(h) => Record::Head(Head {
                rev: match h.rev {
                    Some(r) => Some(new.rev(r).ok_or_else(|| Error(format!("@{old} names @{r}, which is not kept")))?),
                    None => None,
                },
                ..h.clone()
            }),
            Record::Run(r) => {
                new.patches(&all, &run_patches(r));
                Record::Run(run_now(r, &new))
            }
            Record::Read(m) => match m.undo {
                Some(u) => match new.rev(u) {
                    Some(to) => Record::Read(Mark { undo: Some(to), ..m.clone() }),
                    // A mark left out is taken back already.
                    None => continue,
                },
                None => Record::Read(m.clone()),
            },
            Record::Event { run, .. } if !kept_runs.contains(run) => continue,
            other => other.clone(),
        };
        new.push(rec, Some(old));
    }

    // Written whole, then read back: every stack as it was, or no file.
    let mut text = String::new();
    for r in &new.records {
        text.push_str(&serde_json::to_string(r).map_err(|e| Error(e.to_string()))?);
        text.push('\n');
    }
    let mut f = std::fs::File::options()
        .write(true)
        .create_new(true)
        .open(dst)
        .map_err(|e| Error(format!("{}: {e}", dst.display())))?;
    f.write_all(text.as_bytes())
        .and_then(|_| f.sync_all())
        .map_err(|e| Error(format!("{}: {e}", dst.display())))?;
    let check = || -> Result<Vec<String>> {
        let made = Db::load(dst)?;
        if let Some(w) = made.warnings.first() {
            return err(format!("it does not read back: {w}"));
        }
        let mut names = Vec::new();
        for s in all.stacks() {
            let ok = made.stack(&s.name).is_ok_and(|m| m.patches == s.patches && m.base == s.base);
            if !ok {
                return err(format!("{} is not as it was", s.name));
            }
            names.push(s.name.clone());
        }
        if made.stacks().len() != names.len() {
            return err("it has stacks the old file does not");
        }
        Ok(names)
    };
    match check() {
        Ok(stacks) => Ok(Trimmed {
            from,
            records: new.records.len(),
            was: len,
            stacks,
            broken,
        }),
        Err(e) => {
            let _ = std::fs::remove_file(dst);
            err(format!("trimming {} failed: {}; nothing written", src.display(), e.0))
        }
    }
}
