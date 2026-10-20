//! A stack's history, as the editor shows it and the agent is told it: the
//! records that changed the stack (its edits, the moves of its head), each
//! with the stack it was on before and after, and the stack diff from one
//! to the other: the patches added, dropped, moved, given a new version,
//! and what changed in each new version.

use std::collections::HashSet;

use crate::db::{Db, Note, Patch, Record, Stack, local_time};
use crate::thread::{Role, all_turns, last_mine, last_mine_at, last_reply_way, last_role, thread_turns};

/// A record that changed the stack, and where it took it.
#[derive(Debug, Clone)]
pub struct Entry {
    /// The record it is.
    pub rev: usize,
    pub time: i64,
    /// What it was: the edit, or the head's move (undo, redo, select,
    /// delete).
    pub op: String,
    /// Its line: its rev, what it changed in short, as its stack diff marks
    /// it (`+1 −1 ~1 ↕1`), what it was, and where a move went.
    pub text: String,
    /// The stack before it and after it, by rev; none for no stack, not
    /// made yet or deleted.
    pub from: Option<usize>,
    pub to: Option<usize>,
    /// The tree of edits' lanes on its line: an edit's own mark on its
    /// branch's lane, as the tree draws it; for a move of the head, the
    /// lanes going on past it. As wide as every entry's.
    pub lanes: String,
    /// The lanes going on below it: for what is drawn under it.
    pub under: String,
}

impl Entry {
    /// Where it took the stack: from which to which.
    pub fn span(&self) -> String {
        match (self.from, self.to) {
            (None, Some(to)) => format!("the stack made, @{to}"),
            (Some(from), None) => format!("the stack @{from} → deleted"),
            (Some(from), Some(to)) => format!("the stack @{from} → @{to}"),
            (None, None) => "no stack".into(),
        }
    }
}

/// The widest the lanes are drawn: eight branches open at once.
const LANES: usize = 16;

/// How the agent's reply says it was asked, from its fence's tag.
pub fn way_said(tag: &str) -> &'static str {
    match tag {
        "change" => "change",
        "answer" => "answer only",
        "test" => "build and test",
        _ => "earlier",
    }
}

/// What stack `rev` holds, as far as a change of it goes; none for no
/// stack.
fn held(db: &Db, rev: Option<usize>) -> Option<(&[String], Option<&str>, Option<&str>)> {
    let s = db.node(rev?).ok()?;
    Some((&s.patches, s.base.as_deref(), s.repo.as_deref()))
}

/// The records that changed stack `name`, oldest first: its edits, and the
/// moves of its head (undo, redo, select, delete) that took it to another
/// stack than the one it was. Comments, runs and the rest change nothing of
/// it; nor does a landed ask made a branch, the stack having moved on
/// meanwhile, as its name is put straight back.
fn steps(db: &Db, name: &str) -> Vec<Entry> {
    let mut out = Vec::new();
    // The stack the name is on, as the records go.
    let mut on: Option<usize> = None;
    let mut rev = 0;
    while rev < db.records.len() {
        rev += 1;
        let (to, time, op, moved) = match db.rev(rev) {
            Some(Record::Stack(s)) if s.name == name => {
                // Made from elsewhere than where the stack is, then the
                // stack put back where it was.
                let back = matches!(db.rev(rev + 1), Some(Record::Head(h)) if h.name == name && h.rev == on);
                if s.parent != on && back {
                    rev += 1;
                    continue;
                }
                (Some(rev), s.time, s.op.clone(), false)
            }
            Some(Record::Head(h)) if h.name == name => (h.rev, h.time, h.op.clone(), true),
            _ => continue,
        };
        let from = std::mem::replace(&mut on, to);
        if held(db, from) == held(db, to) {
            continue;
        }
        // What it changed first, so that it shows however long the op.
        let mut text = format!("@{rev}");
        let what = summary(&stack_diff(db, from, to, false));
        if !what.is_empty() {
            text += &format!(" {what}");
        }
        text += &format!(" · {op}");
        if moved {
            text += &format!(" → {}", to.map_or("deleted".into(), |r| format!("@{r}")));
        }
        out.push(Entry {
            rev,
            time,
            op,
            text,
            from,
            to,
            lanes: String::new(),
            under: String::new(),
        });
    }
    out
}

/// The history of stack `name`, newest first: the records that changed it,
/// the tree of edits' lanes beside them.
pub fn entries(db: &Db, name: &str) -> Vec<Entry> {
    let mut out = steps(db, name);
    out.reverse();
    draw_lanes(db, name, &mut out);
    out
}

/// The tree of edits' lanes beside `entries`, newest first: each edit
/// drawn on its branch's lane as the tree draws it, the lanes passing every
/// move of the head; all as wide as the widest, and no wider than `LANES`.
/// An edit left out (a branch the stack was never on) is passed over: what
/// was edited from it joins what it was edited from.
fn draw_lanes(db: &Db, name: &str, entries: &mut [Entry]) {
    let head = db.head(name).map(|(r, _)| r);
    let shown: HashSet<usize> = entries.iter().filter(|e| e.to == Some(e.rev)).map(|e| e.rev).collect();
    let joins = |mut at: Option<usize>| {
        while let Some(r) = at.filter(|r| !shown.contains(r)) {
            at = db.node(r).ok().and_then(|s| s.parent);
        }
        at
    };
    let mut lanes = crate::tree::Lanes::default();
    for e in entries.iter_mut() {
        e.lanes = match db.rev(e.rev) {
            Some(Record::Stack(s)) if e.to == Some(e.rev) => lanes.node(e.rev, joins(s.parent), Some(e.rev) == head),
            _ => lanes.passing(),
        };
        e.under = lanes.passing();
    }
    let width = entries
        .iter()
        .map(|e| e.lanes.chars().count().max(e.under.chars().count()))
        .max()
        .unwrap_or(0)
        .min(LANES);
    let fit = |s: &str| -> String {
        let chars: Vec<char> = s.trim_end().chars().collect();
        if chars.len() > width {
            let mut cut: String = chars[..width.saturating_sub(2)].iter().collect();
            cut.push_str("… ");
            cut
        } else {
            format!("{}{}", chars.iter().collect::<String>(), " ".repeat(width - chars.len()))
        }
    };
    for e in entries {
        e.lanes = fit(&e.lanes);
        e.under = fit(&e.under);
    }
}

/// A line of a stack diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Diffed {
    /// The stack made: there was none before.
    Made,
    /// The stack deleted: there is none after.
    Deleted,
    /// Nothing changed.
    Same,
    /// Its base, or its repository (`what`), changed: from, to.
    Base { what: &'static str, was: String, is: String },
    /// Patch `at` (from 1) of the stack after: added, a new version (its
    /// message alone changed, `reworded`), moved from `was`, or as it was.
    Patch {
        at: usize,
        title: String,
        added: bool,
        changed: bool,
        reworded: bool,
        was: Option<usize>,
    },
    /// Patch `was` (from 1) of the stack before, not in the one after.
    Dropped { was: usize, title: String },
    /// A line of what changed in the new version above it: its mail and
    /// the one before it, diffed.
    Code(String),
}

impl Diffed {
    /// As it is written: a patch's place, its mark (`+` added, `~` a new
    /// version, `↕` moved, none as it was) and its title; `−` and the
    /// title of one dropped; what changed in a new version indented under
    /// it.
    pub fn text(&self) -> String {
        match self {
            Diffed::Made => "the stack made".into(),
            Diffed::Deleted => "the stack deleted".into(),
            Diffed::Same => "nothing changed".into(),
            Diffed::Base { what, was, is } => format!("{what} {was} → {is}"),
            Diffed::Patch {
                at,
                title,
                added,
                changed,
                was,
                ..
            } => {
                let mark = if *added {
                    '+'
                } else if *changed {
                    '~'
                } else if was.is_some() {
                    '↕'
                } else {
                    ' '
                };
                let was = was.map_or(String::new(), |w| format!(", was {w}"));
                format!("{at:>2} {mark} {title}{was}")
            }
            Diffed::Dropped { was, title } => format!("   − {title}, was {was}"),
            Diffed::Code(l) => format!("     {l}"),
        }
    }

    /// Whether it is kept where only what changed is said: all but a patch
    /// as it was.
    pub fn is_change(&self) -> bool {
        !matches!(
            self,
            Diffed::Patch {
                added: false,
                changed: false,
                was: None,
                ..
            }
        )
    }
}

/// The stack diff from stack `from` to stack `to` (revs; none for no
/// stack): what is said of the whole, then the stack after, bottom first,
/// each patch marked with how it came from the stack before, those dropped
/// where they were; with `code`, under each new version what changed in
/// it. A patch is known across its versions by its change; one out of the
/// order the others keep is moved, not one only shifted by what was added
/// or dropped below it.
pub fn stack_diff(db: &Db, from: Option<usize>, to: Option<usize>, code: bool) -> Vec<Diffed> {
    let before = from.and_then(|r| db.node(r).ok());
    let after = to.and_then(|r| db.node(r).ok());
    stacks_diff(db, before, after, code)
}

/// As `stack_diff`, from stack `before` to `after`, either of them one that
/// is not in the database: what accepting the agent's change would make.
pub fn stacks_diff(db: &Db, before: Option<&Stack>, after: Option<&Stack>, code: bool) -> Vec<Diffed> {
    let mut out = Vec::new();
    match (before, after) {
        (None, Some(_)) => out.push(Diffed::Made),
        (Some(_), None) => out.push(Diffed::Deleted),
        (Some(b), Some(a)) => {
            let say = |s: &Option<String>, n: usize| s.as_deref().map_or("none".to_string(), |s| s.chars().take(n).collect());
            if b.base != a.base {
                out.push(Diffed::Base {
                    what: "base",
                    was: say(&b.base, 12),
                    is: say(&a.base, 12),
                });
            }
            if b.repo != a.repo {
                out.push(Diffed::Base {
                    what: "repository",
                    was: say(&b.repo, usize::MAX),
                    is: say(&a.repo, usize::MAX),
                });
            }
        }
        (None, None) => {}
    }
    let patches = |s: Option<&Stack>| -> Vec<&Patch> { s.map_or(vec![], |s| s.patches.iter().map(|id| db.patch(id)).collect()) };
    let (old, new) = (patches(before), patches(after));
    let olds: Vec<&str> = old.iter().map(|p| p.change.as_str()).collect();
    let news: Vec<&str> = new.iter().map(|p| p.change.as_str()).collect();
    for op in similar::capture_diff_slices(similar::Algorithm::Myers, &olds, &news) {
        let (tag, o, n) = op.as_tag_tuple();
        if tag == similar::DiffTag::Equal {
            for (i, j) in o.zip(n) {
                put(&mut out, j, new[j], Some(old[i]), None, code);
            }
            continue;
        }
        // Dropped where they were; moved, where they are now.
        for i in o {
            if !news.contains(&olds[i]) {
                out.push(Diffed::Dropped {
                    was: i + 1,
                    title: old[i].title().to_string(),
                });
            }
        }
        for j in n {
            let i = olds.iter().position(|c| *c == news[j]);
            put(&mut out, j, new[j], i.map(|i| old[i]), i, code);
        }
    }
    if !out.iter().any(Diffed::is_change) {
        out.insert(0, Diffed::Same);
    }
    out
}

/// Patch `j` (from 0) of a stack diff's stack after, `p`, coming from
/// `old` in the stack before (moved, from its patch `was`); with `code`,
/// what changed in it.
fn put(out: &mut Vec<Diffed>, j: usize, p: &Patch, old: Option<&Patch>, was: Option<usize>, code: bool) {
    let changed = old.is_some_and(|o| o.id != p.id);
    // Its message alone changed: reworded.
    let reworded = changed && old.is_some_and(|o| o.message != p.message && crate::ops::same_diff(&o.diff, &p.diff));
    out.push(Diffed::Patch {
        at: j + 1,
        title: p.title().to_string(),
        added: old.is_none(),
        changed,
        reworded,
        was: was.map(|i| i + 1),
    });
    if let (true, true, Some(o)) = (code, changed, old) {
        let text = crate::ops::interdiff(o, p);
        out.extend(
            text.lines()
                .skip_while(|l| l.starts_with("--- ") || l.starts_with("+++ "))
                .map(|l| Diffed::Code(l.to_string())),
        );
    }
}

/// A stack diff in short: how many patches were added, dropped, given a
/// new version, moved (`+1 −1 ~1 ↕1`); a new base, a new repository.
pub fn summary(diff: &[Diffed]) -> String {
    let mut n = [0; 4];
    let mut also = Vec::new();
    for d in diff {
        match d {
            Diffed::Patch { added: true, .. } => n[0] += 1,
            Diffed::Patch { changed, was, .. } => {
                n[2] += *changed as usize;
                n[3] += was.is_some() as usize;
            }
            Diffed::Dropped { .. } => n[1] += 1,
            Diffed::Base { what, .. } => also.push(format!("a new {what}")),
            _ => {}
        }
    }
    let mut parts: Vec<String> = ["+", "−", "~", "↕"]
        .iter()
        .zip(n)
        .filter(|(_, k)| *k > 0)
        .map(|(mark, k)| format!("{mark}{k}"))
        .collect();
    parts.extend(also);
    parts.join(" ")
}

/// What the edit that made stack `rev` changed: the stack diff from the
/// stack it was edited from.
pub fn what_changed(db: &Db, rev: usize) -> Vec<Diffed> {
    db.node(rev).map_or(vec![], |s| stack_diff(db, s.parent, Some(rev), true))
}

/// What a comment's record was, next to the one before it.
pub fn note_change(before: Option<&Note>, n: &Note) -> String {
    let Some(b) = before else {
        return match n.by.as_deref() {
            None => "written".into(),
            Some("agent" | "claude") => "opened by the agent".into(),
            Some(who) => format!("pulled: {who}'s comment"),
        };
    };
    if n.text.is_empty() {
        return "closed".into();
    }
    if b.text.is_empty() {
        return "reopened".into();
    }
    if n.change != b.change || n.id != b.id {
        return "moved".into();
    }
    match (&n.queued, &b.queued) {
        (Some(q), was) if q.batched() && was.as_ref().is_none_or(|w| !w.batched()) => return format!("batched ({})", q.said()),
        (Some(q), was) if !q.batched() && was.as_ref().is_none_or(|w| w.batched()) => {
            return format!("sent to the agent ({})", q.said());
        }
        (None, Some(was)) if was.batched() && n.text == b.text => return "taken out of the batch".into(),
        (None, Some(_)) if n.text == b.text => return "taken by the agent".into(),
        _ => {}
    }
    if n.failed.is_some() && b.failed.is_none() {
        return "ask failed".into();
    }
    if n.text != b.text {
        let (now, then) = (all_turns(&n.text), all_turns(&b.text));
        // Rewound from one of your turns: the turns before it as they were,
        // your text in its place, the turns after it gone.
        if let Some(k) = now.len().checked_sub(1)
            && then.len() > now.len()
            && now[..k] == then[..k]
            && now[k].role == Role::You
            && then[k].role == Role::You
        {
            return format!("you rewound the conversation from {}", then[k].name(k));
        }
        // One turn written again, where it is: which, and whose. Your last
        // written again is said below.
        if now.len() == then.len()
            && let [i] = (0..now.len()).filter(|&i| now[i] != then[i]).collect::<Vec<_>>()[..]
            && last_mine_at(&n.text) != Some(i)
        {
            return format!("you edited {}", now[i].name(i));
        }
        // What you said last written again, not added to: a reply to your
        // own turn goes on a line of its own.
        if let (Some(now), Some(then)) = (last_mine(&n.text), last_mine(&b.text))
            && thread_turns(&n.text).len() == thread_turns(&b.text).len()
            && !now.starts_with(&format!("{then}\n"))
        {
            return "you edited it".into();
        }
        return match last_role(&n.text) {
            Some(Role::Agent) => format!("agent replied ({})", way_said(&last_reply_way(&n.text).unwrap_or_default())),
            Some(Role::Reviewer) => "a reviewer replied".into(),
            _ => "you replied".into(),
        };
    }
    "changed".into()
}

/// Record `rev` of stack `name`'s history said in full, as the agent is
/// told it: when, what it was, from which stack to which, and the stack
/// diff, each new version's interdiff under it.
pub fn about(db: &Db, name: &str, rev: usize) -> Vec<String> {
    let Some(e) = steps(db, name).into_iter().find(|e| e.rev == rev) else {
        return vec![format!("@{rev} is not in the history of {name}")];
    };
    let mut out = vec![format!("@{rev} · {} · {}: {}", local_time(e.time), e.op, e.span())];
    out.extend(stack_diff(db, e.from, e.to, true).iter().map(Diffed::text));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{Head, Run, RunState, VERSION, stack_change};

    fn patch(change: &str, title: &str, line: &str) -> Patch {
        let diff = format!("diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -1 +1 @@\n-x\n+{line}\n");
        Patch::new(change.into(), None, None, title.into(), diff)
    }

    fn stack(patches: &[&Patch], parent: Option<usize>, op: &str) -> Record {
        Record::Stack(Stack {
            name: "s".into(),
            patches: patches.iter().map(|p| p.id.clone()).collect(),
            base: Some("base".into()),
            repo: Some("repo".into()),
            parent,
            op: op.into(),
            time: 0,
        })
    }

    fn head(rev: Option<usize>, op: &str) -> Record {
        Record::Head(Head {
            name: "s".into(),
            rev,
            op: op.into(),
            time: 0,
        })
    }

    /// A database of `records`, its header @1: the first of them is @2.
    fn db(records: Vec<Record>) -> Db {
        let mut bytes = String::new();
        for r in std::iter::once(Record::Stacked { version: VERSION }).chain(records) {
            bytes += &serde_json::to_string(&r).unwrap();
            bytes.push('\n');
        }
        let db = Db::from_bytes(std::path::Path::new("t.jsonl"), bytes.as_bytes()).unwrap();
        assert!(db.warnings.is_empty(), "{:?}", db.warnings);
        db
    }

    fn texts(diff: &[Diffed]) -> Vec<String> {
        diff.iter().filter(|d| !matches!(d, Diffed::Code(_))).map(Diffed::text).collect()
    }

    #[test]
    fn a_new_version_whose_message_alone_changed_is_reworded() {
        let a1 = patch("a", "A", "a1");
        // Its message other, and an index line its diff did not have.
        let diff = a1.diff.replace("--- a/f", "index 1..2 100644\n--- a/f");
        let a2 = Patch::new("a".into(), None, None, "A, said otherwise".into(), diff);
        let a3 = patch("a", "A", "a3");
        let d = db(vec![Record::Patch(a1.clone()), Record::Patch(a2.clone()), Record::Patch(a3.clone())]);
        let s = |p: &Patch| Stack {
            name: "s".into(),
            patches: vec![p.id.clone()],
            base: None,
            repo: None,
            parent: None,
            op: String::new(),
            time: 0,
        };
        let diff = stacks_diff(&d, Some(&s(&a1)), Some(&s(&a2)), false);
        assert!(matches!(diff[..], [Diffed::Patch { changed: true, reworded: true, .. }]), "{diff:?}");
        let diff = stacks_diff(&d, Some(&s(&a1)), Some(&s(&a3)), false);
        assert!(matches!(diff[..], [Diffed::Patch { changed: true, reworded: false, .. }]), "{diff:?}");
    }

    #[test]
    fn the_history_is_what_changed_the_stack() {
        let (a1, b1, a2) = (patch("a", "A", "a1"), patch("b", "B", "b1"), patch("a", "A", "a2"));
        let d = db(vec![
            Record::Patch(a1.clone()),             // @2
            Record::Patch(b1.clone()),             // @3
            stack(&[&a1, &b1], None, "import"),    // @4
            Record::Comment(Note {                 // @5
                change: stack_change("s"),
                id: "s1".into(),
                text: "why?".into(),
                ..Default::default()
            }),
            stack(&[&b1], Some(4), "drop"),        // @6
            Record::Patch(a2.clone()),             // @7
            // A landed ask made from @4, a branch: the stack put back.
            stack(&[&a2, &b1], Some(4), "agent: the stack"), // @8
            head(Some(6), "the agent's edit @8 is a branch"),  // @9
            Record::Run(Run {                      // @10
                id: "r".into(),
                stack: "s".into(),
                state: RunState::Done,
                ..Default::default()
            }),
            head(Some(4), "undo drop"),            // @11
            head(Some(8), "select @8"),            // @12
            head(Some(8), "select @8"),            // @13: where it is already
            stack(&[&b1, &a2], Some(8), "move"),   // @14
            head(None, "delete"),                  // @15
        ]);
        let all = entries(&d, "s");
        let revs: Vec<usize> = all.iter().map(|e| e.rev).collect();
        assert_eq!(revs, [15, 14, 12, 11, 6, 4]);
        let text: Vec<&str> = all.iter().map(|e| e.text.as_str()).collect();
        assert_eq!(
            text,
            [
                "@15 −2 · delete → deleted",
                "@14 ↕1 · move",
                "@12 ~1 · select @8 → @8",
                "@11 +1 · undo drop → @4",
                "@6 −1 · drop",
                "@4 +2 · import",
            ]
        );
        let at = |rev: usize| all.iter().find(|e| e.rev == rev).unwrap();
        assert_eq!((at(12).from, at(12).to), (Some(4), Some(8)));
        assert_eq!(at(15).span(), "the stack @14 → deleted");
        assert_eq!(at(4).span(), "the stack made, @4");
        // The branch left out, what was edited from it joins what it was
        // edited from: the move and the drop both from the import.
        let lanes: Vec<&str> = all.iter().map(|e| e.lanes.as_str()).collect();
        assert_eq!(lanes, ["    ", "○   ", "│   ", "│   ", "│ ○ ", "○ ┘ "]);
        // Told in full: the select, a new version, what changed in it.
        let told = about(&d, "s", 12);
        assert!(told[0].ends_with("select @8: the stack @4 → @8"), "{told:?}");
        assert!(told.contains(&" 1 ~ A".to_string()) && told.contains(&"     -+a1".to_string()), "{told:?}");
        assert!(told.contains(&"     ++a2".to_string()) && told.contains(&" 2   B".to_string()), "{told:?}");
        assert_eq!(about(&d, "s", 5), ["@5 is not in the history of s"]);
    }

    #[test]
    fn a_stack_diff_follows_each_patch_by_its_change() {
        let (a1, b1, c1) = (patch("a", "A", "a1"), patch("b", "B", "b1"), patch("c", "C", "c1"));
        let (a2, d1) = (patch("a", "A", "a2"), patch("d", "D", "d1"));
        let all = [&a1, &b1, &c1, &a2, &d1];
        let mut records: Vec<Record> = all.iter().map(|p| Record::Patch((*p).clone())).collect();
        // @7 to @11.
        records.push(stack(&[&a1, &b1, &c1], None, "import"));
        records.push(stack(&[&a2, &c1, &d1], Some(7), "edit"));
        records.push(stack(&[&c1, &a1, &b1], Some(7), "move"));
        records.push(stack(&[&b1, &c1], Some(7), "drop"));
        records.push(Record::Stack(Stack {
            base: Some("another".into()),
            ..match stack(&[&a1, &b1, &c1], Some(7), "rebase") {
                Record::Stack(s) => s,
                _ => unreachable!(),
            }
        }));
        let d = db(records);
        // A new version, one dropped where it was, one kept, one added.
        let diff = stack_diff(&d, Some(7), Some(8), true);
        assert_eq!(texts(&diff), [" 1 ~ A", "   − B, was 2", " 2   C", " 3 + D"]);
        assert!(diff.contains(&Diffed::Code("-+a1".into())) && diff.contains(&Diffed::Code("++a2".into())), "{diff:?}");
        assert_eq!(summary(&diff), "+1 −1 ~1");
        assert!(!stack_diff(&d, Some(7), Some(8), false).iter().any(|l| matches!(l, Diffed::Code(_))));
        // Moved, the others keeping their order.
        let diff = stack_diff(&d, Some(7), Some(9), true);
        assert_eq!(texts(&diff), [" 1 ↕ C, was 3", " 2   A", " 3   B"]);
        assert_eq!(summary(&diff), "↕1");
        // Shifted by a drop below them: not moved.
        let diff = stack_diff(&d, Some(7), Some(10), true);
        assert_eq!(texts(&diff), ["   − A, was 1", " 1   B", " 2   C"]);
        assert_eq!(summary(&diff), "−1");
        // A new base, the patches as they were.
        let diff = stack_diff(&d, Some(7), Some(11), true);
        assert_eq!(texts(&diff)[0], "base base → another");
        assert_eq!(summary(&diff), "a new base");
        // Made, deleted; nothing changed.
        assert_eq!(texts(&stack_diff(&d, None, Some(7), true))[..2], ["the stack made", " 1 + A"]);
        assert_eq!(texts(&stack_diff(&d, Some(10), None, true)), ["the stack deleted", "   − B, was 1", "   − C, was 2"]);
        assert_eq!(texts(&stack_diff(&d, Some(7), Some(7), true))[0], "nothing changed");
        // The tree's: from the stack it was edited from.
        assert_eq!(what_changed(&d, 10), stack_diff(&d, Some(7), Some(10), true));
    }
}
