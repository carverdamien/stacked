//! How far the review of a stack is: the lines left to review and the
//! comments left to close, as the stack is now.

use std::collections::HashMap;
use std::sync::Arc;

use crate::db::{Db, Patch};

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
}

impl Progress {
    /// What the top bar says of it, the longest first, then shorter ones
    /// for less room.
    pub fn said(&self) -> Vec<String> {
        let (l, c) = (self.lines, self.comments);
        if l == 0 && c == 0 {
            return vec!["nothing left to review".into(), "all reviewed".into()];
        }
        let (mut long, mut short) = (Vec::new(), Vec::new());
        if l > 0 {
            long.push(format!("{l} line{} to review", plural(l)));
            short.push(format!("{l} line{}", plural(l)));
        }
        if c > 0 {
            long.push(format!("{c} comment{} to close", plural(c)));
            short.push(format!("{c} comment{}", plural(c)));
        }
        vec![long.join(" · "), short.join(" · ")]
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
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
    Progress {
        lines,
        comments: db.stack_notes(name).len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{Mark, Note, Record, Stack, VERSION, stack_change};
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

    #[test]
    fn the_bar_says_what_is_left_then_less() {
        let p = Progress { lines: 1, comments: 2 };
        assert_eq!(p.said(), ["1 line to review · 2 comments to close", "1 line · 2 comments"]);
        let p = Progress { lines: 3, comments: 0 };
        assert_eq!(p.said(), ["3 lines to review", "3 lines"]);
        let p = Progress { lines: 0, comments: 1 };
        assert_eq!(p.said(), ["1 comment to close", "1 comment"]);
        assert_eq!(Progress::default().said(), ["nothing left to review", "all reviewed"]);
    }

    #[test]
    fn what_is_left_is_the_lines_not_read_and_the_comments_open() {
        let a = patch("a", &["one", "two", "three", "four"]);
        let start = [Record::Patch(a.clone()), stack("s", &[&a], 1000)];
        assert_eq!(of(&start), Progress { lines: 4, comments: 0 });
        // 2 lines read; a comment open on the patch, one on the stack.
        let recs = [&start[..], &[read(&a, 2, 1600), note("a", "n", "why?", 1700), note(&stack_change("s"), "m", "and?", 1800)]].concat();
        assert_eq!(of(&recs), Progress { lines: 2, comments: 2 });
        // A new version keeps the mark on the line it did not change; a
        // comment closed.
        let a2 = patch("a", &["one", "TWO", "three", "four"]);
        let recs = [&recs[..], &[Record::Patch(a2.clone()), stack("s", &[&a2], 1900), note("a", "n", "", 2000)]].concat();
        assert_eq!(of(&recs), Progress { lines: 3, comments: 1 });
    }
}
