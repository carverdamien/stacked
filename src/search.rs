//! Searching what stacked holds: the comments, the patches of a stack
//! (their messages and their diffs), the files a patch touches as it leaves
//! them, and the stack's history. Away from the screen, in a thread of its
//! own: each match is sent as it is found, what is in sight first, and the
//! search stops as soon as another is wanted. A line matches when it holds
//! what is searched for, whatever its case.

use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Sender;

use crate::db::{Db, Note, Patch, stack_change};
use crate::thread::{Role, all_turns};

/// What a search looks in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    All,
    Patch,
    Comment,
    History,
}

impl Scope {
    pub const ALL: [Scope; 4] = [Scope::All, Scope::Patch, Scope::Comment, Scope::History];

    pub fn name(self) -> &'static str {
        match self {
            Scope::All => "all",
            Scope::Patch => "patch",
            Scope::Comment => "comment",
            Scope::History => "history",
        }
    }

    /// The next in `ALL`, or the one before.
    pub fn step(self, forward: bool) -> Scope {
        let k = Scope::ALL.iter().position(|s| *s == self).unwrap_or(0);
        let n = Scope::ALL.len();
        Scope::ALL[if forward { (k + 1) % n } else { (k + n - 1) % n }]
    }
}

/// Where a match is, to go to.
#[derive(Debug, Clone, PartialEq)]
pub enum Place {
    /// A turn of a comment's thread, of all its turns.
    Comment { change: String, id: String, turn: usize },
    /// A line of a patch's message, from 0.
    Message { change: String, line: usize },
    /// A line of a patch's diff, from 0.
    Diff { change: String, at: usize },
    /// A line of a file as patch `change` leaves it, from 1.
    File { change: String, path: String, line: usize },
    /// A record of the stack's history.
    History { rev: usize },
}

/// A match: where it is, said short, and its line.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub place: Place,
    pub at: String,
    pub line: String,
    /// For a file's: the lines around it, from that line number.
    pub context: Option<(usize, Vec<String>)>,
}

/// What a search is, and what is in sight where it was asked from.
#[derive(Debug, Clone)]
pub struct Query {
    pub scope: Scope,
    pub text: String,
    pub stack: String,
    /// The patch the middle shows, by place; none for the stack itself.
    pub patch: Option<usize>,
    /// The comments in sight: those the comments pane shows, then the
    /// others of the stack the filter shows.
    pub shown: Vec<(String, String)>,
    pub filtered: Vec<(String, String)>,
}

/// The most matches a search sends.
pub const MOST: usize = 500;

/// What a search sends: its generation, then a match; nothing once it is
/// done.
pub type Sent = (u64, Option<Found>);

/// Runs search `q` on the database at `path`, sending what it finds, the
/// nearest first, to `tx` as generation `generation`; it stops once
/// `wanted` is another generation. Done, it says so: `(generation, None)`.
pub fn run(path: &Path, q: &Query, generation: u64, wanted: &AtomicU64, tx: &Sender<Sent>) {
    let mut s = Search {
        q: q.text.trim().to_lowercase(),
        generation,
        wanted,
        tx,
        sent: 0,
    };
    if !s.q.is_empty()
        && let Ok(db) = Db::load(path)
    {
        s.all(&db, q);
    }
    let _ = tx.send((generation, None));
}

struct Search<'a> {
    q: String,
    generation: u64,
    wanted: &'a AtomicU64,
    tx: &'a Sender<Sent>,
    sent: usize,
}

impl Search<'_> {
    /// Whether to go on: still wanted, and not too many found.
    fn on(&self) -> bool {
        self.wanted.load(Ordering::Relaxed) == self.generation && self.sent < MOST
    }

    fn matches(&self, line: &str) -> bool {
        line.to_lowercase().contains(&self.q)
    }

    fn send(&mut self, f: Found) -> bool {
        if !self.on() {
            return false;
        }
        self.sent += 1;
        self.tx.send((self.generation, Some(f))).is_ok()
    }

    /// Everything the scope looks in, in sight first.
    fn all(&mut self, db: &Db, q: &Query) {
        let Ok(s) = db.stack(&q.stack) else { return };
        let patches: Vec<&Patch> = s.patches.iter().map(|id| db.patch(id)).collect();
        let here = q.patch.filter(|&k| k < patches.len());
        let mut seen: HashSet<(String, String)> = HashSet::new();
        let (comments, code, history) = match q.scope {
            Scope::All => (true, true, true),
            Scope::Patch => (false, true, false),
            Scope::Comment => (true, false, false),
            Scope::History => (false, false, true),
        };
        if comments {
            self.notes(db, q, &q.shown, &mut seen);
        }
        if code && let Some(k) = here {
            self.patch(&patches, k);
        }
        if comments {
            self.notes(db, q, &q.filtered, &mut seen);
        }
        if code {
            for k in (0..patches.len()).filter(|&k| Some(k) != here) {
                if !self.on() {
                    return;
                }
                self.patch(&patches, k);
            }
            self.files(s, &patches, here);
        }
        if history {
            self.history(db, &q.stack);
        }
        if comments {
            let rest: Vec<(String, String)> = db.stack_notes(&q.stack).iter().map(|n| (n.change.clone(), n.id.clone())).collect();
            self.notes(db, q, &rest, &mut seen);
            for other in db.stacks().iter().filter(|o| o.name != q.stack) {
                let theirs: Vec<(String, String)> =
                    db.stack_notes(&other.name).iter().map(|n| (n.change.clone(), n.id.clone())).collect();
                self.notes(db, q, &theirs, &mut seen);
            }
        }
    }

    /// The comments `which`, but those `seen`: each line of each turn.
    fn notes(&mut self, db: &Db, q: &Query, which: &[(String, String)], seen: &mut HashSet<(String, String)>) {
        for (change, id) in which {
            if !self.on() {
                return;
            }
            if !seen.insert((change.clone(), id.clone())) {
                continue;
            }
            let Some(n) = db.note(change, id) else { continue };
            let place = where_note(db, &q.stack, n);
            for (turn, t) in all_turns(&n.text).into_iter().enumerate() {
                if t.deleted {
                    continue;
                }
                let who = match t.role {
                    Role::You => "you".to_string(),
                    Role::Agent => crate::thread::agent_at(t.agent_effort()),
                    Role::Reviewer => t.tag.clone().unwrap_or_else(|| "reviewer".into()),
                };
                let hits: Vec<String> = t.body.lines().filter(|l| self.matches(l)).map(|l| l.trim().to_string()).collect();
                for line in hits {
                    let f = Found {
                        place: Place::Comment {
                            change: n.change.clone(),
                            id: n.id.clone(),
                            turn,
                        },
                        at: format!("{place} · {who}"),
                        line,
                        context: None,
                    };
                    if !self.send(f) {
                        return;
                    }
                }
            }
        }
    }

    /// Patch `k`'s message and diff.
    fn patch(&mut self, patches: &[&Patch], k: usize) {
        let p = patches[k];
        for (i, line) in p.message.lines().enumerate() {
            if self.matches(line) {
                let f = Found {
                    place: Place::Message {
                        change: p.change.clone(),
                        line: i,
                    },
                    at: format!("patch {} · message", k + 1),
                    line: line.trim().to_string(),
                    context: None,
                };
                if !self.send(f) {
                    return;
                }
            }
        }
        let mut file = crate::diff::FileOf::default();
        for (i, line) in p.diff.lines().enumerate() {
            file.read(line);
            if self.matches(line) {
                let f = Found {
                    place: Place::Diff {
                        change: p.change.clone(),
                        at: i,
                    },
                    at: format!("patch {} · {}", k + 1, file.path),
                    line: line.to_string(),
                    context: None,
                };
                if !self.send(f) {
                    return;
                }
            }
        }
    }

    /// The files patch `here` touches, all of them, as it leaves them (the
    /// whole stack's, as the stack leaves them, when no patch is shown): the
    /// lines its diff does not show.
    fn files(&mut self, s: &crate::db::Stack, patches: &[&Patch], here: Option<usize>) {
        let Some(top) = here.or(patches.len().checked_sub(1)) else { return };
        let Ok((repo, base)) = crate::ops::base_of(s) else { return };
        let below = &patches[..=top];
        let touching: Vec<&Patch> = match here {
            Some(k) => vec![patches[k]],
            None => below.to_vec(),
        };
        let mut paths: Vec<(String, &Patch)> = Vec::new();
        for p in &touching {
            for fd in crate::diff::parse_diff(&p.diff, "") {
                if fd.header.iter().any(|l| l.starts_with("deleted file mode ")) {
                    continue;
                }
                if !paths.iter().any(|(q, _)| *q == fd.path) {
                    paths.push((fd.path, p));
                }
            }
        }
        let diffs: Vec<&str> = below.iter().map(|p| p.diff.as_str()).collect();
        let also: Vec<&str> = paths.iter().map(|(p, _)| p.as_str()).collect();
        let Ok(pad) = crate::check::Pad::new(&repo, &base, &diffs, &also) else { return };
        if !pad.apply(&diffs.concat()) {
            return;
        }
        for (path, p) in paths {
            if !self.on() {
                return;
            }
            let Ok(Some(text)) = pad.text(&path) else { continue };
            let lines: Vec<&str> = text.lines().collect();
            let shown = shown_lines(&p.diff, &path);
            let k = patches.iter().position(|q| q.id == p.id).unwrap_or(top);
            for (i, line) in lines.iter().enumerate() {
                let no = i + 1;
                if shown.iter().any(|(a, b)| (*a..*b).contains(&no)) || !self.matches(line) {
                    continue;
                }
                let from = i.saturating_sub(6);
                let to = (i + 7).min(lines.len());
                let f = Found {
                    place: Place::File {
                        change: p.change.clone(),
                        path: path.clone(),
                        line: no,
                    },
                    at: format!("patch {} · {path}:{no}", k + 1),
                    line: line.to_string(),
                    context: Some((from + 1, lines[from..to].iter().map(|l| l.to_string()).collect())),
                };
                if !self.send(f) {
                    return;
                }
            }
        }
    }

    /// Each record of the stack's history, newest first: its line, then its
    /// stack diff.
    fn history(&mut self, db: &Db, name: &str) {
        for e in crate::history::entries(db, name) {
            if !self.on() {
                return;
            }
            let mut lines = vec![e.text.clone()];
            lines.extend(crate::history::stack_diff(db, e.from, e.to, true).iter().map(|d| d.text()));
            let hits: Vec<String> =
                lines.iter().flat_map(|l| l.lines()).filter(|l| self.matches(l)).map(|l| l.trim().to_string()).collect();
            for line in hits {
                let f = Found {
                    place: Place::History { rev: e.rev },
                    at: format!("@{} · {}", e.rev, crate::db::local_time(e.time)),
                    line,
                    context: None,
                };
                if !self.send(f) {
                    return;
                }
            }
        }
    }
}

/// Where comment `n` is, said short, from stack `name`'s side: on the stack,
/// on one of its patches, or in another stack.
fn where_note(db: &Db, name: &str, n: &Note) -> String {
    for s in db.stacks() {
        let mine = s.name == name;
        let theirs = |what: String| if mine { what } else { format!("{} · {what}", s.name) };
        if n.change == stack_change(&s.name) {
            return theirs("the stack".into());
        }
        if let Some(k) = s.patches.iter().position(|id| db.patch(id).change == n.change) {
            return theirs(format!("patch {}", k + 1));
        }
    }
    "a patch no longer in a stack".into()
}

/// The lines of `path`, as `diff` leaves it, that its hunks show: each
/// hunk's new lines, from and to (not counted).
fn shown_lines(diff: &str, path: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut file = crate::diff::FileOf::default();
    for line in diff.lines() {
        if file.read(line) || file.path != path {
            continue;
        }
        let Some(head) = line.strip_prefix("@@ ") else { continue };
        let Some(new) = head.split_whitespace().find(|w| w.starts_with('+')) else { continue };
        let mut parts = new[1..].split(',');
        let start: usize = parts.next().and_then(|n| n.parse().ok()).unwrap_or(0);
        let count: usize = parts.next().and_then(|n| n.parse().ok()).unwrap_or(1);
        out.push((start, start + count));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lines_a_hunk_shows_are_its_new_ones() {
        let diff = "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1,3 +1,4 @@\n one\n+two\n three\n four\n@@ -20 +21 @@\n-x\n+y\ndiff --git a/b.txt b/b.txt\n@@ -1 +1 @@\n-b\n+B\n";
        assert_eq!(shown_lines(diff, "a.txt"), [(1, 5), (21, 22)]);
        assert_eq!(shown_lines(diff, "b.txt"), [(1, 2)]);
        assert!(shown_lines(diff, "c.txt").is_empty());
        // A path git quotes, one holding " b/".
        let named = "diff --git \"a/caf\\303\\251.txt\" \"b/caf\\303\\251.txt\"\n--- \"a/caf\\303\\251.txt\"\n+++ \"b/caf\\303\\251.txt\"\n@@ -1 +1 @@\n-x\n+y\ndiff --git a/plan b/x.txt b/plan b/x.txt\n--- a/plan b/x.txt\t\n+++ b/plan b/x.txt\t\n@@ -2 +2 @@\n-x\n+y\n";
        assert_eq!(shown_lines(named, "café.txt"), [(1, 2)]);
        assert_eq!(shown_lines(named, "plan b/x.txt"), [(2, 3)]);
    }

    #[test]
    fn the_scope_steps_round() {
        assert_eq!(Scope::All.step(true), Scope::Patch);
        assert_eq!(Scope::All.step(false), Scope::History);
        assert_eq!(Scope::History.step(true), Scope::All);
    }
}
