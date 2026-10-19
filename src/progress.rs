//! How far the review of a stack is: the lines left to review, the comments
//! left to close, and how long the rest takes at your pace.
//!
//! Your pace is what you did in the time you spent: the lines you marked
//! read and the comments you closed, over the time the database was written
//! to, the pauses left out (an hour or more with nothing written). The stack
//! has a pace too, what came to it in the time spent since it had patches:
//! the lines new to it, and the comments opened on it. Its lines are done
//! when you read them faster than they come, in what is left over the
//! difference; its comments likewise; the rest is done when both are.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::db::{Db, Note, Patch, Record, stack_change};

/// An hour or more with nothing written to the database is a pause: it is
/// not time spent.
pub const IDLE: i64 = 3600;

/// Each patch's changed lines, by its id: a patch never changes, so they
/// are kept.
pub type Memo = HashMap<String, Arc<Vec<String>>>;

/// Where the review of a stack is.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Progress {
    /// The changed lines of its patches not marked read.
    pub lines: usize,
    /// The comments open on it.
    pub comments: usize,
    /// How long the rest takes, in seconds of time spent: none while your
    /// pace is not known, infinite while what comes outpaces it.
    pub eta: Option<f64>,
}

impl Progress {
    /// What the top bar says of it, the longest first, then shorter ones
    /// for less room.
    pub fn said(&self) -> Vec<String> {
        let (l, c) = (self.lines, self.comments);
        if l == 0 && c == 0 {
            return vec!["nothing left to review".into(), "all reviewed".into()];
        }
        let eta = match self.eta {
            None => "ETA unknown".to_string(),
            Some(t) if t.is_infinite() => "ETA never".to_string(),
            Some(t) => format!("ETA {}", duration(t)),
        };
        let (mut long, mut short) = (Vec::new(), Vec::new());
        if l > 0 {
            long.push(format!("{l} line{} to review", plural(l)));
            short.push(format!("{l} line{}", plural(l)));
        }
        if c > 0 {
            long.push(format!("{c} comment{} to close", plural(c)));
            short.push(format!("{c} comment{}", plural(c)));
        }
        long.push(eta.clone());
        short.push(eta.clone());
        vec![long.join(" · "), short.join(" · "), eta]
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// `secs` said short, in minutes, then hours: `45m`, `1h 20m`, `12h`.
pub fn duration(secs: f64) -> String {
    let m = ((secs / 60.0).round() as u64).max(1);
    match m {
        ..60 => format!("{m}m"),
        60..600 if !m.is_multiple_of(60) => format!("{}h {}m", m / 60, m % 60),
        _ => format!("{}h", (m + 30) / 60),
    }
}

/// The changed lines of patch `p`, by key, as the reading mode marks them.
pub fn changed_keys(p: &Patch) -> Vec<String> {
    crate::diff::parse_diff(&p.diff, &crate::view::mode(&p.change))
        .iter()
        .flat_map(|f| f.hunks.iter())
        .flat_map(|h| h.lines.iter())
        .filter(|l| l.origin != ' ')
        .map(|l| l.key.clone())
        .collect()
}

fn keys_of(db: &Db, memo: &mut Memo, id: &str) -> Arc<Vec<String>> {
    memo.entry(id.to_string()).or_insert_with(|| Arc::new(changed_keys(db.patch(id)))).clone()
}

/// The time spent from `from` on, in seconds: the gaps between `times`, in
/// order, the pauses left out.
pub fn active(times: &[i64], from: i64) -> i64 {
    times.windows(2).filter(|w| w[0] >= from).map(|w| w[1] - w[0]).filter(|&d| d < IDLE).sum()
}

/// How long `left` takes when `done` go a second and `come` come: none if
/// none were ever done, infinite if they come as fast as they go.
pub fn time_to(left: usize, done: Option<f64>, come: f64) -> Option<f64> {
    if left == 0 {
        return Some(0.0);
    }
    let done = done?;
    Some(if done <= come { f64::INFINITY } else { left as f64 / (done - come) })
}

/// When both are done: never if either never is, not known if either is
/// not, else when the later is.
pub fn both(a: Option<f64>, b: Option<f64>) -> Option<f64> {
    if a.is_some_and(f64::is_infinite) || b.is_some_and(f64::is_infinite) {
        return Some(f64::INFINITY);
    }
    Some(a?.max(b?))
}

/// When record `r` was written, if it says.
fn time_of(r: &Record) -> Option<i64> {
    match r {
        Record::Stack(s) => Some(s.time),
        Record::Head(h) => Some(h.time),
        Record::Comment(n) => Some(n.time),
        Record::Run(r) => Some(r.time),
        Record::Read(m) => Some(m.time),
        _ => None,
    }
}

/// What came to stack `name` since it had patches (since it was last made,
/// if it was deleted): when that was, how many lines were new to it then,
/// and every change it had, its own comments' too.
struct Growth {
    from: i64,
    lines: usize,
    changes: HashSet<String>,
}

fn growth(db: &Db, name: &str, memo: &mut Memo) -> Growth {
    let made = db
        .records
        .iter()
        .rposition(|r| matches!(r, Record::Head(h) if h.name == name && h.rev.is_none()))
        .map_or(0, |at| at + 1);
    let tree: Vec<_> = db.tree(name).into_iter().filter(|(rev, s)| *rev > made && !s.patches.is_empty()).collect();
    let mut changes = HashSet::from([stack_change(name)]);
    let Some((_, first)) = tree.first() else {
        return Growth {
            from: i64::MAX,
            lines: 0,
            changes,
        };
    };
    // Each patch once, with the first version of the stack it is in.
    let (mut ids, mut each) = (HashSet::new(), Vec::new());
    for (k, (_, s)) in tree.iter().enumerate() {
        for id in &s.patches {
            if ids.insert(id.as_str()) {
                changes.insert(db.patch(id).change.clone());
                each.push((k, keys_of(db, memo, id)));
            }
        }
    }
    let (mut keys, mut lines) = (HashSet::new(), 0);
    for (k, ks) in &each {
        for key in ks.iter() {
            if keys.insert(key.as_str()) && *k > 0 {
                lines += 1;
            }
        }
    }
    Growth {
        from: first.time,
        lines,
        changes,
    }
}

/// A comment opened (or opened again) or closed: when, and on what change.
struct Flow<'a> {
    time: i64,
    change: &'a str,
    closed: bool,
}

/// Every comment opened and closed, oldest first. A comment moved, closed
/// where it was as it opens where it goes (a patch split, or the agent
/// moving it to the stack and back), is neither.
fn flows(db: &Db) -> Vec<Flow<'_>> {
    let mut last: HashMap<(&str, &str), &Note> = HashMap::new();
    // When, the comment as it was open, and whether it closed.
    let mut all: Vec<(i64, &Note, bool)> = Vec::new();
    for r in &db.records {
        let Record::Comment(n) = r else { continue };
        let was = last.insert((&n.change, &n.id), n).filter(|w| !w.text.is_empty());
        match (was, n.text.is_empty()) {
            (None, false) => all.push((n.time, n, false)),
            (Some(w), true) => all.push((n.time, w, true)),
            _ => {}
        }
    }
    let points = |a: &Note, b: &Note| a.moved.as_ref().is_some_and(|m| m.change == b.change && m.id == b.id);
    let moved = |open: &Note, closed: &Note| open.text == closed.text || points(open, closed) || points(closed, open);
    let mut opens: HashMap<i64, Vec<usize>> = HashMap::new();
    for (k, (t, _, closed)) in all.iter().enumerate() {
        if !closed {
            opens.entry(*t).or_default().push(k);
        }
    }
    let mut paired = vec![false; all.len()];
    for k in 0..all.len() {
        let (t, closed_note, closed) = all[k];
        if !closed {
            continue;
        }
        let open = (t - 1..=t + 1)
            .flat_map(|u| opens.get(&u).into_iter().flatten())
            .copied()
            .find(|&o| !paired[o] && moved(all[o].1, closed_note));
        if let Some(o) = open {
            paired[o] = true;
            paired[k] = true;
        }
    }
    all.into_iter()
        .zip(paired)
        .filter(|(_, p)| !p)
        .map(|((time, n, closed), _)| Flow {
            time,
            change: &n.change,
            closed,
        })
        .collect()
}

/// Where the review of stack `name` is; `memo` keeps the patches' changed
/// lines.
pub fn progress(db: &Db, name: &str, memo: &mut Memo) -> Progress {
    let Some((_, stack)) = db.head(name) else {
        return Progress::default();
    };
    let mut lines = 0;
    for id in &stack.patches {
        let read = db.read_keys(&db.patch(id).change);
        lines += keys_of(db, memo, id).iter().filter(|k| !read.contains(*k)).count();
    }
    let comments = db.stack_notes(name).len();
    // The time spent: in all, and since the stack had patches.
    let mut times: Vec<i64> = db.records.iter().filter_map(time_of).filter(|&t| t > 0).collect();
    times.sort_unstable();
    let grown = growth(db, name, memo);
    let (spent, since) = (active(&times, i64::MIN) as f64, active(&times, grown.from) as f64);
    // Your pace: the lines you ever marked read, the comments you closed.
    let marked: HashSet<&str> = db
        .records
        .iter()
        .filter_map(|r| match r {
            Record::Read(m) => Some(m.change.as_str()),
            _ => None,
        })
        .collect();
    let read: usize = marked.iter().map(|c| db.read_keys(c).len()).sum();
    let flows = flows(db);
    let closed = flows.iter().filter(|f| f.closed).count();
    let pace = |n: usize| (spent > 0.0 && n > 0).then(|| n as f64 / spent);
    // The stack's: the lines new to it, the comments opened on it.
    let opened = flows.iter().filter(|f| !f.closed && f.time >= grown.from && grown.changes.contains(f.change)).count();
    let come = |n: usize| if since > 0.0 { n as f64 / since } else { 0.0 };
    let eta = both(time_to(lines, pace(read), come(grown.lines)), time_to(comments, pace(closed), come(opened)));
    Progress { lines, comments, eta }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{Head, Mark, Moved, Stack, VERSION};
    use std::path::Path;

    /// A database of `recs`, as the file would hold them.
    fn db_of(recs: &[Record]) -> Db {
        let mut bytes = Vec::new();
        for r in std::iter::once(&Record::Stacked { version: VERSION }).chain(recs) {
            bytes.extend(serde_json::to_vec(r).unwrap());
            bytes.push(b'\n');
        }
        let db = Db::from_bytes(Path::new("db.jsonl"), &bytes).unwrap();
        assert!(db.warnings.is_empty(), "{:?}", db.warnings);
        db
    }

    /// A patch of change `change` adding `lines` to its own file.
    fn patch(change: &str, lines: &[&str]) -> Patch {
        let added: String = lines.iter().map(|l| format!("+{l}\n")).collect();
        let diff = format!("diff --git a/{change}.txt b/{change}.txt\n--- a/{change}.txt\n+++ b/{change}.txt\n@@ -0,0 +1,{} @@\n{added}", lines.len());
        Patch::new(change.into(), None, None, format!("Bug - {change}"), diff)
    }

    fn stack(name: &str, patches: &[&Patch], time: i64) -> Record {
        Record::Stack(Stack {
            name: name.into(),
            patches: patches.iter().map(|p| p.id.clone()).collect(),
            base: None,
            repo: None,
            parent: None,
            op: "import".into(),
            time,
        })
    }

    /// The first `n` lines of `p` marked read at `time`.
    fn read(p: &Patch, n: usize, time: i64) -> Record {
        Record::Read(Mark {
            change: p.change.clone(),
            keys: changed_keys(p).into_iter().take(n).collect(),
            undo: None,
            time,
        })
    }

    fn note(change: &str, id: &str, text: &str, time: i64) -> Record {
        Record::Comment(Note {
            change: change.into(),
            id: id.into(),
            text: text.into(),
            time,
            ..Default::default()
        })
    }

    fn of(recs: &[Record]) -> Progress {
        progress(&db_of(recs), "s", &mut Memo::new())
    }

    /// Whether `eta` is `secs`, give or take what floats round.
    fn near(eta: Option<f64>, secs: f64) -> bool {
        eta.is_some_and(|t| (t - secs).abs() < 1e-6)
    }

    #[test]
    fn the_time_spent_leaves_the_pauses_out() {
        assert_eq!(active(&[0, 10, 4000, 4010], i64::MIN), 20, "3990 seconds is a pause");
        assert_eq!(active(&[0, 3599, 7199], i64::MIN), 3599, "an hour is a pause, a second less is not");
        assert_eq!(active(&[0, 10, 30], 10), 20, "from the second on");
        assert_eq!(active(&[5], i64::MIN), 0);
    }

    #[test]
    fn what_is_left_over_what_goes_and_comes() {
        assert_eq!(time_to(0, None, 1.0), Some(0.0), "nothing left is done, pace or not");
        assert_eq!(time_to(3, None, 0.0), None, "nothing done yet: not known");
        assert_eq!(time_to(3, Some(0.5), 0.25), Some(12.0));
        assert_eq!(time_to(3, Some(0.5), 0.5), Some(f64::INFINITY), "they come as fast as they go");
        assert_eq!(both(Some(5.0), Some(7.0)), Some(7.0));
        assert_eq!(both(Some(5.0), None), None);
        assert_eq!(both(None, Some(f64::INFINITY)), Some(f64::INFINITY), "never, known or not");
    }

    #[test]
    fn durations_are_said_short() {
        assert_eq!(duration(10.0), "1m");
        assert_eq!(duration(2700.0), "45m");
        assert_eq!(duration(4800.0), "1h 20m");
        assert_eq!(duration(7200.0), "2h");
        assert_eq!(duration(45000.0), "13h");
    }

    #[test]
    fn the_bar_says_what_is_left_then_less() {
        let p = Progress {
            lines: 1,
            comments: 2,
            eta: Some(4800.0),
        };
        assert_eq!(p.said(), ["1 line to review · 2 comments to close · ETA 1h 20m", "1 line · 2 comments · ETA 1h 20m", "ETA 1h 20m"]);
        let p = Progress {
            lines: 3,
            comments: 0,
            eta: None,
        };
        assert_eq!(p.said()[0], "3 lines to review · ETA unknown");
        let p = Progress {
            comments: 1,
            eta: Some(f64::INFINITY),
            ..p
        };
        assert_eq!(p.said()[0], "3 lines to review · 1 comment to close · ETA never");
        assert_eq!(Progress::default().said(), ["nothing left to review", "all reviewed"]);
    }

    #[test]
    fn the_eta_is_known_once_lines_are_read_and_the_pauses_do_not_count() {
        let a = patch("a", &["one", "two", "three", "four"]);
        let start = [Record::Patch(a.clone()), stack("s", &[&a], 1000)];
        assert_eq!(
            of(&start),
            Progress {
                lines: 4,
                comments: 0,
                eta: None
            }
        );
        // 2 lines in 600 seconds: 2 more in 600.
        let recs = [&start[..], &[read(&a, 2, 1600)]].concat();
        assert_eq!(of(&recs).lines, 2);
        assert!(near(of(&recs).eta, 600.0), "{:?}", of(&recs));
        // Back after two hours: 1 more line, in 300 seconds more spent.
        let recs = [&recs[..], &[note("x", "n", "a note", 8800), read(&a, 3, 9100)]].concat();
        assert_eq!(of(&recs).lines, 1);
        assert!(near(of(&recs).eta, 300.0), "{:?}", of(&recs));
    }

    #[test]
    fn lines_new_to_the_stack_slow_it_down() {
        let a = patch("a", &["one", "two", "three", "four"]);
        let b = patch("b", &["bee"]);
        let a2 = patch("a", &["one", "TWO", "three", "four"]);
        let recs = [
            Record::Patch(a.clone()),
            Record::Patch(b.clone()),
            Record::Patch(a2.clone()),
            stack("s", &[&a], 1000),
            // A patch more: 1 line new; a new version: 1 line new.
            stack("s", &[&a, &b], 1100),
            stack("s", &[&a2, &b], 1200),
            read(&a2, 4, 1600),
        ];
        // 4 lines read in 600 seconds, 2 new: 2 lines a 600 seconds, 1 left.
        let p = of(&recs);
        assert_eq!(p.lines, 1);
        assert!(near(p.eta, 300.0), "{p:?}");
        // As fast as they are read, they are never done.
        let c = patch("c", &["see", "sea"]);
        let recs = [&recs[..], &[Record::Patch(c.clone()), stack("s", &[&a2, &b, &c], 1600)]].concat();
        assert_eq!(of(&recs).eta, Some(f64::INFINITY));
    }

    #[test]
    fn a_stack_made_again_grows_from_then() {
        let a = patch("a", &["one", "two"]);
        let b = patch("b", &["bee", "honey"]);
        let recs = [
            Record::Patch(a.clone()),
            Record::Patch(b.clone()),
            stack("s", &[&a], 1000),
            Record::Head(Head {
                name: "s".into(),
                rev: None,
                op: "delete".into(),
                time: 1100,
            }),
            stack("s", &[&b], 1200),
            read(&b, 1, 1800),
        ];
        // None new since it was made again: 1 left, at a line a 800 seconds.
        assert!(near(of(&recs).eta, 800.0), "{:?}", of(&recs));
    }

    #[test]
    fn comments_closed_against_those_opened() {
        let a = patch("a", &["one"]);
        let recs = [
            Record::Patch(a.clone()),
            stack("s", &[&a], 1000),
            read(&a, 1, 1000),
            // Elsewhere, before: 3 closed.
            note("z", "1", "x", 900),
            note("z", "2", "y", 900),
            note("z", "3", "z", 900),
            note("z", "1", "", 1100),
            note("z", "2", "", 1200),
            note("z", "3", "", 1300),
            // On the stack: 2 opened, 1 left.
            note("a", "n", "why?", 1400),
            note(&stack_change("s"), "m", "and?", 1500),
            note("a", "n", "", 1800),
        ];
        // 4 closed in 900 seconds; 2 opened in 800.
        let p = of(&recs);
        assert_eq!((p.lines, p.comments), (0, 1));
        let (done, come) = (4.0 / 900.0, 2.0 / 800.0);
        assert!(near(p.eta, 1.0 / (done - come)), "{p:?}");
    }

    #[test]
    fn a_comment_moved_is_neither_closed_nor_opened() {
        let a = patch("a", &["one"]);
        let b = patch("b", &["bee"]);
        let moved = Record::Comment(Note {
            change: stack_change("s"),
            id: "moved:1".into(),
            text: "why?\n\nmore".into(),
            moved: Some(Moved {
                change: "a".into(),
                id: "n".into(),
                ..Default::default()
            }),
            time: 1500,
            ..Default::default()
        });
        let recs = [
            Record::Patch(a.clone()),
            Record::Patch(b.clone()),
            stack("s", &[&a, &b], 1000),
            note("a", "n", "why?", 1100),
            note("a", "k", "this?", 1200),
            // Split: on another patch, the same.
            note("b", "k", "this?", 1300),
            note("a", "k", "", 1300),
            // To the stack.
            moved,
            note("a", "n", "", 1501),
        ];
        let db = db_of(&recs);
        let flows = flows(&db);
        let said: Vec<(i64, &str, bool)> = flows.iter().map(|f| (f.time, f.change, f.closed)).collect();
        assert_eq!(said, [(1100, "a", false), (1200, "a", false)]);
    }
}
