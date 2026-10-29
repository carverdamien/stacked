//! How far the review of a stack is: the lines left to review, of all it
//! changes, and the comments left to close, as the stack is now.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::db::{Db, Mark, Patch};

/// Each patch's changed lines, by its id: each line's key, and whether it
/// is blank (`changed_lines`). A patch never changes, so they are kept.
pub type Memo = HashMap<String, Arc<Vec<(String, bool)>>>;

/// Where the review of a stack is.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Progress {
    /// The changed lines of its patches not marked read.
    pub lines: usize,
    /// The changed lines of its patches, read or not.
    pub total: usize,
    /// The comments open on it.
    pub comments: usize,
}

impl Progress {
    /// What the top bar says of it, the longest first, then shorter ones
    /// for less room: the lines left of them all, and the comments open.
    pub fn said(&self) -> Vec<String> {
        let (l, t, c) = (self.lines, self.total, self.comments);
        if l == 0 && c == 0 {
            return vec!["nothing left to review".into(), "all reviewed".into()];
        }
        let (mut long, mut short) = (Vec::new(), Vec::new());
        if l > 0 {
            long.push(format!("{l}/{t} line{} to review", plural(t)));
            short.push(format!("{l}/{t} line{}", plural(t)));
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
    changed_lines(p).into_iter().map(|(k, _)| k).collect()
}

/// The lines of patch `p` to read: the changed lines of its diff, its +
/// and − lines, which are the lines Space marks (git's `\ No newline at
/// end of file` is not one of them), then the lines of its message, its
/// title first; each one's key, and whether it is blank, which a line
/// needs no reading for unless the settings say white space does: a line
/// of the diff when it is white space alone, a line of the message as
/// `blank_lines` says.
pub fn changed_lines(p: &Patch) -> Vec<(String, bool)> {
    let diff = crate::diff::parse_diff(&p.diff, &crate::view::mode(&p.change))
        .iter()
        .flat_map(|f| f.hunks.iter())
        .flat_map(|h| h.lines.iter())
        .filter(|l| matches!(l.origin, '+' | '-'))
        .map(|l| (l.key.clone(), crate::diff::white_space(&l.text)))
        .collect::<Vec<_>>();
    let message = crate::view::message_keys(p).into_iter().zip(blank_lines(&p.message));
    diff.into_iter().chain(message).collect()
}

/// Each line of `message`, a patch's: whether the reading mode takes it as
/// a blank line, which needs no reading unless the settings say white space
/// does (`view.whitespace_needs_review`). A line is blank when it is empty
/// or white space alone, or, with the moz-phab feature, when it is the
/// trailer moz-phab writes (`revision_trailers`). This is the one place
/// that says which lines of a message are blank: the counts, the reading
/// mode's marks and the rows all ask here.
pub fn blank_lines(message: &str) -> Vec<bool> {
    let trailers = revision_trailers(message);
    message.split('\n').enumerate().map(|(i, l)| l.trim().is_empty() || trailers.contains(&i)).collect()
}

/// The lines of `message`, a patch's, from 0, that are the trailer moz-phab
/// writes to name the patch's revision (`revision_trailer`), in the
/// message's last paragraph, after an empty line, where trailers go, and so
/// never in the title's paragraph. This is the one place that says which
/// lines of a message are that trailer: the reading mode asks here, and so
/// does the sync of a patch's message with its commit's (`head`).
#[cfg(feature = "moz-phab")]
pub fn revision_trailers(message: &str) -> Vec<usize> {
    let lines: Vec<&str> = message.split('\n').collect();
    let empty = |l: &&str| l.trim().is_empty();
    // The first line of the last paragraph, past the end when that
    // paragraph is the title's.
    let last = lines.iter().rposition(|l| !empty(l)).unwrap_or(0);
    let start = lines[..last].iter().rposition(empty).map_or(lines.len(), |e| e + 1);
    (start..lines.len()).filter(|&i| revision_trailer(lines[i])).collect()
}

/// Without the moz-phab feature, no line of a message is moz-phab's
/// trailer: the line naming the revision is a line as any other.
#[cfg(not(feature = "moz-phab"))]
pub fn revision_trailers(_: &str) -> Vec<usize> {
    Vec::new()
}

/// Whether `line` is the trailer moz-phab writes to name the patch's
/// revision, as `Differential Revision:
/// https://phabricator.services.mozilla.com/D123456`. Trimmed, the line
/// must be exactly `Differential Revision: ` and a link, `http://` or
/// `https://`, a host, and a path ending in `/D` and the revision's number:
/// `Differential Revision: https?://[^/\s]+(/\S*)?/D[0-9]+`, from its
/// start to its end, in the case written here.
#[cfg(feature = "moz-phab")]
fn revision_trailer(line: &str) -> bool {
    let link = line.trim().strip_prefix("Differential Revision: ");
    let Some(rest) = link.and_then(|l| l.strip_prefix("https://").or_else(|| l.strip_prefix("http://"))) else {
        return false;
    };
    // The host, and the revision after the last slash.
    let Some((before, last)) = rest.rsplit_once('/') else {
        return false;
    };
    let host = before.split('/').next().unwrap_or_default();
    let number = last.strip_prefix('D').unwrap_or_default();
    !host.is_empty() && !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()) && !rest.contains(char::is_whitespace)
}

fn keys_of(db: &Db, memo: &mut Memo, id: &str) -> Arc<Vec<(String, bool)>> {
    memo.entry(id.to_string()).or_insert_with(|| Arc::new(changed_lines(db.patch(id)))).clone()
}

/// The sections of patch `p`'s message, each its key
/// (`view::section_keys`) and the keys of its lines.
fn sections_of(p: &Patch) -> Vec<(String, Vec<String>)> {
    let lines = crate::view::message_keys(p);
    crate::view::section_keys(p)
        .into_iter()
        .map(|(at, k)| (k, at.filter_map(|i| lines.get(i).cloned()).collect()))
        .collect()
}

/// The lines of patch `p` read, by key: those marked read on its change
/// (`Db::read_keys`), and each line of a section of its message that a
/// mark holds the key of (`view::section_keys`). So a section of the
/// message that a new version only wraps otherwise, or ends its lines in
/// other white space, as an interdiff shows the same, is read in the new
/// version as it was in the old: only what changed is to read again.
/// This is what the counts, the reading mode's marks and the rows read.
pub fn read_keys(db: &Db, p: &Patch) -> HashSet<String> {
    let mut read = db.read_keys(&p.change);
    let more: Vec<String> = sections_of(p).into_iter().filter(|(k, _)| read.contains(k)).flat_map(|(_, lines)| lines).collect();
    read.extend(more);
    read
}

/// The marks on patch `p`'s change, oldest first (`Db::marks`), each as the
/// lines it reads in `p`: what it holds, a section's key (`read_keys`) as
/// each line of that section of `p`'s message.
pub fn marks(db: &Db, p: &Patch) -> Vec<(usize, Mark)> {
    let sections = sections_of(p);
    db.marks(&p.change)
        .into_iter()
        .map(|(rev, m)| {
            let mut keys: Vec<String> = m.keys.iter().filter(|k| !crate::view::is_section_key(k)).cloned().collect();
            keys.extend(sections.iter().filter(|(k, _)| m.keys.contains(k)).flat_map(|(_, lines)| lines.iter().cloned()));
            (rev, Mark { keys, ..m.clone() })
        })
        .collect()
}

/// `keys`, lines of patch `p` that a mark is to hold, with the key of each
/// section of `p`'s message (`view::section_keys`) whose lines they hold
/// all of: the mark reads that section in any version of the patch that
/// has it, however that version wraps it.
pub fn with_sections(p: &Patch, mut keys: Vec<String>) -> Vec<String> {
    let held: HashSet<&String> = keys.iter().collect();
    let more: Vec<String> = sections_of(p)
        .into_iter()
        .filter(|(_, lines)| !lines.is_empty() && lines.iter().all(|k| held.contains(k)))
        .map(|(k, _)| k)
        .collect();
    keys.extend(more);
    keys
}

/// Where the review of stack `name` is; `memo` keeps the patches' changed
/// lines. Those that are blank (`changed_lines`) are left to review only
/// when `whitespace` says they need it.
pub fn progress(db: &Db, name: &str, memo: &mut Memo, whitespace: bool) -> Progress {
    let Some((_, stack)) = db.head(name) else {
        return Progress::default();
    };
    let (mut lines, mut total) = (0, 0);
    for id in &stack.patches {
        let read = read_keys(db, db.patch(id));
        for (k, _) in keys_of(db, memo, id).iter().filter(|(_, blank)| whitespace || !blank) {
            total += 1;
            if !read.contains(k) {
                lines += 1;
            }
        }
    }
    Progress {
        lines,
        total,
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

    /// As `patch`, its message `message`.
    fn patch_saying(change: &str, lines: &[&str], message: &str) -> Patch {
        let p = patch(change, lines);
        Patch::new(p.change, None, None, message.into(), p.diff)
    }

    /// The trailer moz-phab writes.
    const TRAILER: &str = "Differential Revision: https://phabricator.services.mozilla.com/D123456";

    /// A message that ends with the trailer moz-phab writes.
    fn trailed() -> String {
        format!("Bug 1 - a\n\nWhy it is so.\n\n{TRAILER}")
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
        progress(&db_of(recs), "s", &mut Memo::new(), false)
    }

    #[test]
    fn the_bar_says_what_is_left_of_all_then_less() {
        let p = Progress { lines: 1, total: 4, comments: 2 };
        assert_eq!(p.said(), ["1/4 lines to review · 2 comments to close", "1/4 lines · 2 comments"]);
        let p = Progress { lines: 3, total: 3, comments: 0 };
        assert_eq!(p.said(), ["3/3 lines to review", "3/3 lines"]);
        let p = Progress { lines: 1, total: 1, comments: 0 };
        assert_eq!(p.said(), ["1/1 line to review", "1/1 line"]);
        // All read: no lines to count, however many there are.
        let p = Progress { lines: 0, total: 5, comments: 1 };
        assert_eq!(p.said(), ["1 comment to close", "1 comment"]);
        let p = Progress { lines: 0, total: 5, comments: 0 };
        assert_eq!(p.said(), ["nothing left to review", "all reviewed"]);
        assert_eq!(Progress::default().said(), ["nothing left to review", "all reviewed"]);
    }

    #[test]
    fn what_is_left_is_the_lines_not_read_and_the_comments_open() {
        let a = patch("a", &["one", "two", "three", "four"]);
        let start = [Record::Patch(a.clone()), stack("s", &[&a], 1000)];
        // Its 4 lines, and its message's title.
        assert_eq!(of(&start), Progress { lines: 5, total: 5, comments: 0 });
        // 2 lines read; a comment open on the patch, one on the stack.
        let recs = [&start[..], &[read(&a, 2, 1600), note("a", "n", "why?", 1700), note(&stack_change("s"), "m", "and?", 1800)]].concat();
        assert_eq!(of(&recs), Progress { lines: 3, total: 5, comments: 2 });
        // A new version keeps the mark on the line it did not change; a
        // comment closed.
        let a2 = patch("a", &["one", "TWO", "three", "four"]);
        let recs = [&recs[..], &[Record::Patch(a2.clone()), stack("s", &[&a2], 1900), note("a", "n", "", 2000)]].concat();
        assert_eq!(of(&recs), Progress { lines: 4, total: 5, comments: 1 });
        // A patch more: its lines to review, of the stack's.
        let b = patch("b", &["five", "six"]);
        let recs = [&recs[..], &[Record::Patch(b.clone()), stack("s", &[&a2, &b], 2100), read(&b, 1, 2200)]].concat();
        assert_eq!(of(&recs), Progress { lines: 6, total: 8, comments: 1 });
    }

    #[test]
    fn lines_of_white_space_alone_are_left_to_review_only_when_the_settings_say_so() {
        let a = patch("a", &["one", "", "  \t", "two"]);
        let recs = [Record::Patch(a.clone()), stack("s", &[&a], 1000)];
        let db = db_of(&recs);
        let p = progress(&db, "s", &mut Memo::new(), false);
        assert_eq!((p.lines, p.total), (3, 3), "the empty line and the blank one need no review, nor count among the lines");
        let p = progress(&db, "s", &mut Memo::new(), true);
        assert_eq!((p.lines, p.total), (5, 5));
        // The same memo serves either setting.
        let mut memo = Memo::new();
        assert_eq!(progress(&db, "s", &mut memo, false).lines, 3);
        assert_eq!(progress(&db, "s", &mut memo, true).lines, 5);
        assert_eq!(changed_lines(&a).iter().filter(|(_, blank)| *blank).count(), 2);
    }

    #[test]
    fn no_newline_at_end_of_file_is_not_a_line_to_review() {
        // The file's last line has no newline, before the patch and after
        // it: under each side's, git writes `\ No newline at end of file`.
        let diff = "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1,2 +1,2 @@\n one\n-two\n\\ No newline at end of file\n+TWO\n\\ No newline at end of file\n";
        let a = Patch::new("a".into(), None, None, "Bug - a".into(), diff.into());
        let lines = crate::diff::parse_diff(&a.diff, &crate::view::mode(&a.change)).remove(0).hunks.remove(0).lines;
        assert_eq!(lines.iter().filter(|l| l.origin == '\\').count(), 2);
        // What Space marks: the line removed, the line added, and the title.
        let marked: Vec<String> = lines.iter().filter(|l| matches!(l.origin, '+' | '-')).map(|l| l.key.clone()).chain(crate::view::message_keys(&a)).collect();
        assert_eq!(changed_keys(&a), marked);
        let start = [Record::Patch(a.clone()), stack("s", &[&a], 1000)];
        assert_eq!(of(&start), Progress { lines: 3, total: 3, comments: 0 });
        // All of them read: nothing is left to review.
        let all = Record::Read(Mark {
            change: a.change.clone(),
            keys: marked,
            undo: None,
            time: 1600,
        });
        let p = of(&[&start[..], &[all]].concat());
        assert_eq!(p, Progress { lines: 0, total: 3, comments: 0 });
        assert_eq!(p.said(), ["nothing left to review", "all reviewed"]);
    }

    #[cfg(feature = "moz-phab")]
    #[test]
    fn the_trailer_moz_phab_writes_needs_no_review_as_an_empty_line_unless_the_settings_say_so() {
        let a = patch_saying("a", &["one", "two"], &trailed());
        assert_eq!(blank_lines(&trailed()), [false, true, false, true, true]);
        let start = [Record::Patch(a.clone()), stack("s", &[&a], 1000)];
        // Its 2 lines, and of its message the title and the line of prose.
        assert_eq!(of(&start), Progress { lines: 4, total: 4, comments: 0 });
        // Those read, the empty lines and the trailer not: nothing is left.
        let message = crate::view::message_keys(&a);
        let mut keys: Vec<String> = changed_keys(&a).into_iter().take(2).collect();
        keys.extend([message[0].clone(), message[2].clone()]);
        let all = Record::Read(Mark {
            change: a.change.clone(),
            keys,
            undo: None,
            time: 1600,
        });
        let recs = [&start[..], &[all]].concat();
        let p = of(&recs);
        assert_eq!(p, Progress { lines: 0, total: 4, comments: 0 });
        assert_eq!(p.said(), ["nothing left to review", "all reviewed"]);
        // When the settings say white space needs review, the trailer is
        // left to review, as the two empty lines are.
        let p = progress(&db_of(&recs), "s", &mut Memo::new(), true);
        assert_eq!((p.lines, p.total), (3, 7));
    }

    #[cfg(feature = "moz-phab")]
    #[test]
    fn the_trailer_is_blank_whatever_phabricator_it_names() {
        let last = |message: &str| *blank_lines(message).last().unwrap();
        for link in [
            "https://phabricator.services.mozilla.com/D123456",
            "https://phabricator-dev.allizom.org/D7",
            "http://localhost:8080/D1",
            "https://example.org/phab/D42",
        ] {
            assert!(last(&format!("Bug 1 - a\n\nWhy.\n\nDifferential Revision: {link}")), "{link}");
        }
        // White space around it; after another trailer of its paragraph;
        // the message ending with empty lines.
        assert!(last(&format!("Bug 1 - a\n\n  {TRAILER}\t")));
        assert_eq!(blank_lines(&format!("Bug 1 - a\n\nReviewed-by: ann\n{TRAILER}")), [false, true, false, true]);
        assert_eq!(blank_lines(&format!("Bug 1 - a\n\n{TRAILER}\n\n")), [false, true, true, true, true]);
    }

    #[cfg(feature = "moz-phab")]
    #[test]
    fn a_line_like_the_trailer_but_not_it_is_a_line_to_review() {
        let last = |message: &str| *blank_lines(message).last().unwrap();
        for line in [
            // Something said after the link.
            "Differential Revision: https://phabricator.services.mozilla.com/D123456 (landed)",
            // A link that does not end in `/D` and a number.
            "Differential Revision: https://phabricator.services.mozilla.com/D123456/",
            "Differential Revision: https://phabricator.services.mozilla.com/123456",
            "Differential Revision: https://phabricator.services.mozilla.com/D12a",
            "Differential Revision: https://phabricator.services.mozilla.com/D",
            "Differential Revision: https://phabricator.services.mozilla.com",
            "Differential Revision: https:///D123456",
            "Differential Revision: ftp://phabricator.services.mozilla.com/D123456",
            // No link.
            "Differential Revision:",
            "Differential Revision: D123456",
            // Not as moz-phab writes it.
            "differential revision: https://phabricator.services.mozilla.com/D123456",
            "Differential Revision:  https://phabricator.services.mozilla.com/D123456",
            "See Differential Revision: https://phabricator.services.mozilla.com/D123456",
        ] {
            assert!(!last(&format!("Bug 1 - a\n\nWhy.\n\n{line}")), "{line}");
        }
        // The trailer in the middle of the message, in the title's paragraph,
        // and as the title.
        assert_eq!(blank_lines(&format!("Bug 1 - a\n\n{TRAILER}\n\nWhy.")), [false, true, false, true, false]);
        assert_eq!(blank_lines(&format!("Bug 1 - a\n{TRAILER}")), [false, false]);
        assert_eq!(blank_lines(TRAILER), [false]);
    }

    #[cfg(not(feature = "moz-phab"))]
    #[test]
    fn without_moz_phab_the_trailer_is_a_line_to_review() {
        assert_eq!(blank_lines(&trailed()), [false, true, false, true, false]);
        let a = patch_saying("a", &["one", "two"], &trailed());
        let start = [Record::Patch(a.clone()), stack("s", &[&a], 1000)];
        assert_eq!(of(&start), Progress { lines: 5, total: 5, comments: 0 });
    }

    #[test]
    fn a_paragraph_a_new_version_only_wraps_otherwise_is_read_as_it_was() {
        let wrapped = "Bug 1 - a\n\nThe parser reads the whole file\nbefore it checks the header.\n\nThat is all.";
        let a = patch_saying("a", &["one"], wrapped);
        let start = [Record::Patch(a.clone()), stack("s", &[&a], 1000)];
        // Its line, the title and three lines of prose.
        assert_eq!(of(&start), Progress { lines: 5, total: 5, comments: 0 });
        // Read whole: the mark holds the key of each section of the
        // message, as it holds its lines.
        let keys = with_sections(&a, changed_keys(&a));
        let sections: Vec<String> = crate::view::section_keys(&a).into_iter().map(|(_, k)| k).collect();
        assert_eq!(sections.len(), 3);
        assert!(sections.iter().all(|k| keys.contains(k) && crate::view::is_section_key(k)), "{keys:?}");
        let mark = |p: &Patch, keys: Vec<String>, time: i64| Record::Read(Mark {
            change: p.change.clone(),
            keys,
            undo: None,
            time,
        });
        let read = [&start[..], &[mark(&a, keys, 1100)]].concat();
        assert_eq!(of(&read).lines, 0);
        // A new version that only wraps the paragraph otherwise: nothing
        // left to read, as its interdiff shows nothing of it changed.
        let rewrapped = wrapped.replace("whole file\nbefore", "whole\nfile before").replace("header.", "header.  ");
        let b = patch_saying("a", &["one"], &rewrapped);
        let recs = [&read[..], &[Record::Patch(b.clone()), stack("s", &[&b], 1200)]].concat();
        assert_eq!(of(&recs), Progress { lines: 0, total: 5, comments: 0 });
        let db = db_of(&recs);
        let lines = crate::view::message_keys(&b);
        assert!(lines.iter().all(|k| read_keys(&db, &b).contains(k)), "each line of it read");
        // The mark, as it reads the new version: its lines, no section's key.
        let marks = marks(&db, &b);
        assert!(lines.iter().all(|k| marks[0].1.keys.contains(k)) && !marks[0].1.keys.iter().any(|k| crate::view::is_section_key(k)));
        // A word of it changed: its two lines to read again, as its
        // interdiff shows them both changed; the rest read.
        let reworded = rewrapped.replace("whole", "entire");
        let c = patch_saying("a", &["one"], &reworded);
        let recs = [&recs[..], &[Record::Patch(c.clone()), stack("s", &[&c], 1300)]].concat();
        assert_eq!(of(&recs), Progress { lines: 2, total: 5, comments: 0 });
        // A mark of a paragraph alone holds its section, no other.
        let held = with_sections(&c, vec![crate::view::message_keys(&c)[0].clone()]);
        assert_eq!(held.len(), 2);
        // A mark holding no section's key, as those made before marks
        // held them: only the lines it holds, the paragraph's two to read.
        let old = [&start[..], &[mark(&a, changed_keys(&a), 1100), Record::Patch(b.clone()), stack("s", &[&b], 1200)]].concat();
        assert_eq!(of(&old).lines, 2);
    }

    #[test]
    fn a_line_of_the_diff_saying_what_the_trailer_says_is_a_line_to_review() {
        let a = patch_saying("a", &[TRAILER], &trailed());
        let start = [Record::Patch(a.clone()), stack("s", &[&a], 1000)];
        assert!(!changed_lines(&a)[0].1, "the line added");
        let message = if cfg!(feature = "moz-phab") { 2 } else { 3 };
        assert_eq!(of(&start).total, 1 + message);
    }
}
