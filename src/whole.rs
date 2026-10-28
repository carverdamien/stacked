//! The files a patch changes, whole, as the middle shows them after `3`:
//! each file as the patch leaves it, the lines it removes in their place,
//! the lines in no hunk between its hunks' lines. The files are read from
//! the check's own index (`check::versions`: no checkout, the repository
//! only read), once for each patch, as a patch never changes, in a thread of
//! their own: the screen does not wait on them.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use crate::check::Sides;
use crate::db::Patch;
use crate::diff::FileDiff;

/// Each file a patch changes, before and after it, by the path it names
/// after.
pub type Files = HashMap<String, Sides>;

/// What reading a patch's files came to: the files, or why they could not
/// be read.
pub type Read = std::result::Result<Arc<Files>, String>;

/// The most lines a file shown whole may have, on either side: a longer
/// one is shown as its hunks, a note saying so.
pub const MOST_LINES: usize = 20_000;

/// The most lines a patch's files shown whole may have together: a file
/// that would take them past it is shown as its hunks, a note saying so.
pub const MOST_IN_PATCH: usize = 50_000;

/// A line of a file shown whole: one in no hunk, by its numbers before and
/// after the patch, and its text, the origin's space not in it; or line
/// `line` of hunk `hunk`, as the hunks show it.
#[derive(Debug, Clone, PartialEq)]
pub enum Line<'a> {
    Outside { old_no: usize, new_no: usize, text: &'a str },
    In { hunk: usize, line: usize },
}

/// Whether file `fd`'s diff is of a file that is not text: git's word that
/// it differs, or its binary patch.
pub fn binary(fd: &FileDiff) -> bool {
    fd.header.iter().any(|l| l.starts_with("Binary files ") || l.starts_with("GIT binary patch"))
}

/// File `fd`'s lines whole, from `sides`, the file before and after the
/// patch: its hunks' lines in their place, and between them the lines in
/// no hunk, as the file after the patch has them (as the file before has
/// them, for a file the patch deletes). None for a file that is not text,
/// and for one that does not fit its hunks, as a side shorter than they
/// say would not.
pub fn lines<'a>(fd: &'a FileDiff, sides: &'a Sides) -> Option<Vec<Line<'a>>> {
    if binary(fd) {
        return None;
    }
    let deleted = fd.header.iter().any(|l| l.starts_with("deleted file mode"));
    // The side the lines in no hunk are read from, and whether it is the
    // new one.
    let (from, new) = match (&sides.new, &sides.old) {
        (Some(n), _) => (n, true),
        (None, Some(o)) if deleted => (o, false),
        _ => return None,
    };
    let mut out = Vec::new();
    // The next line of each side not shown yet, from 1.
    let (mut old_at, mut new_at) = (1usize, 1usize);
    // The lines `old_no` and `new_no` on, `n` of them, in no hunk.
    let gap = |out: &mut Vec<Line<'a>>, old_no: usize, new_no: usize, n: usize| -> Option<()> {
        for k in 0..n {
            let no = if new { new_no + k } else { old_no + k };
            let text = from.get(no.checked_sub(1)?)?;
            out.push(Line::Outside { old_no: old_no + k, new_no: new_no + k, text });
        }
        Some(())
    };
    for (h, hunk) in fd.hunks.iter().enumerate() {
        let first = hunk.lines.first()?;
        // As many lines on each side as its header says: git's `\` line
        // counts on neither. A side with none starts after its line
        // `start` (`@@ -5,0 +6,2 @@` adds two lines after line 5).
        let old_n = hunk.lines.iter().filter(|l| matches!(l.origin, ' ' | '-')).count();
        let new_n = hunk.lines.iter().filter(|l| matches!(l.origin, ' ' | '+')).count();
        let old_first = if old_n > 0 { first.old_no } else { first.old_no + 1 };
        let new_first = if new_n > 0 { first.new_no } else { first.new_no + 1 };
        // The lines between it and the hunk before: the same on both sides.
        let (old_gap, new_gap) = (old_first.checked_sub(old_at)?, new_first.checked_sub(new_at)?);
        if old_gap != new_gap {
            return None;
        }
        gap(&mut out, old_at, new_at, new_gap)?;
        out.extend((0..hunk.lines.len()).map(|line| Line::In { hunk: h, line }));
        old_at = old_first + old_n;
        new_at = new_first + new_n;
    }
    // After the last hunk, the rest of the file: as long on the other side,
    // when it is there.
    let rest = (from.len() + 1).checked_sub(if new { new_at } else { old_at })?;
    let other = if new { sides.old.as_ref().map(|o| (o.len() + 1).checked_sub(old_at)) } else { None };
    if other.is_some_and(|n| n != Some(rest)) {
        return None;
    }
    gap(&mut out, old_at, new_at, rest)?;
    Some(out)
}

/// The files of patches as they are read: a repository, a base, and the
/// patches up to the one whose files they are.
pub type Job = (PathBuf, String, Vec<Patch>);

/// The files the last of `patches` changes, before and after it, read on
/// `base` in `repo` with the patches below applied.
pub fn read(repo: &std::path::Path, base: &str, patches: &[&Patch]) -> Read {
    crate::check::versions(repo, base, patches).map(Arc::new).map_err(|e| e.0)
}

/// The files of the patches shown whole, by key (`reading::key`, as their
/// blocks are kept): those read, kept for as long as the editor runs, and
/// those being read, each in a thread of its own.
#[derive(Default)]
pub struct Reader {
    read: HashMap<String, Read>,
    going: HashMap<String, std::thread::JoinHandle<Read>>,
}

impl Reader {
    /// Whether the files of `key` are read, or being read.
    pub fn known(&self, key: &str) -> bool {
        self.read.contains_key(key) || self.going.contains_key(key)
    }

    /// Whether the files of `key` are being read.
    pub fn going(&self, key: &str) -> bool {
        self.going.get(key).is_some_and(|h| !h.is_finished())
    }

    /// The files of `key`, if read, or why they could not be.
    pub fn peek(&self, key: &str) -> Option<Read> {
        self.read.get(key).cloned()
    }

    /// The files of `key`, if read by now, or why they could not be; else
    /// they are read with what `job` gives, if they are not being read
    /// already, and None meanwhile. With nothing to read them from, `job`
    /// says why.
    pub fn get(&mut self, key: &str, job: impl FnOnce() -> std::result::Result<Job, String>) -> Option<Read> {
        if let Some(r) = self.read.get(key) {
            return Some(r.clone());
        }
        if let Some(h) = self.going.get(key) {
            if !h.is_finished() {
                return None;
            }
            let r = self.going.remove(key).unwrap().join().unwrap_or_else(|_| Err("reading them failed".into()));
            self.read.insert(key.into(), r.clone());
            return Some(r);
        }
        match job() {
            Err(why) => {
                self.read.insert(key.into(), Err(why.clone()));
                Some(Err(why))
            }
            Ok((repo, base, patches)) => {
                let h = std::thread::spawn(move || read(&repo, &base, &patches.iter().collect::<Vec<_>>()));
                self.going.insert(key.into(), h);
                None
            }
        }
    }

    /// Waits for every read to end.
    pub fn wait(&mut self) {
        for (key, h) in self.going.drain() {
            self.read.insert(key, h.join().unwrap_or_else(|_| Err("reading them failed".into())));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::parse_diff;

    fn lines_of(s: &str) -> Vec<String> {
        s.lines().map(String::from).collect()
    }

    /// What `lines` makes of `diff` and the file before and after it: each
    /// line in no hunk as `=N text`, each of a hunk as the diff writes it.
    fn shown(diff: &str, old: Option<&str>, new: Option<&str>) -> Option<Vec<String>> {
        let fd = &parse_diff(diff, "m")[0];
        let sides = Sides { old: old.map(lines_of), new: new.map(lines_of) };
        let ls = lines(fd, &sides)?;
        Some(
            ls.iter()
                .map(|l| match l {
                    Line::Outside { new_no, text, .. } => format!("={new_no} {text}"),
                    Line::In { hunk, line } => fd.hunks[*hunk].lines[*line].text.clone(),
                })
                .collect(),
        )
    }

    #[test]
    fn a_file_whole_has_its_lines_in_no_hunk_between_its_hunks() {
        let old = "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n";
        let new = "1\nTWO\n3\n4\n5\n6\n7\n8\nNINE\n10\n";
        let diff = "diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -1,3 +1,3 @@\n 1\n-2\n+TWO\n 3\n@@ -8,3 +8,3 @@\n 8\n-9\n+NINE\n 10\n";
        assert_eq!(
            shown(diff, Some(old), Some(new)).unwrap(),
            [" 1", "-2", "+TWO", " 3", "=4 4", "=5 5", "=6 6", "=7 7", " 8", "-9", "+NINE", " 10"]
        );
        // The file's end, after the last hunk; its start, before the first.
        let diff = "diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -4,3 +4,3 @@\n 4\n-5\n+FIVE\n 6\n";
        let new = old.replace("5\n", "FIVE\n");
        let got = shown(diff, Some(old), Some(&new)).unwrap();
        assert_eq!(got[..4], ["=1 1", "=2 2", "=3 3", " 4"]);
        assert_eq!(got[6..], [" 6", "=7 7", "=8 8", "=9 9", "=10 10"]);
        assert_eq!(got.len(), 11, "ten lines and the one removed");
    }

    #[test]
    fn a_file_added_or_deleted_is_its_one_hunk_and_one_renamed_whole_is_all_outside() {
        let added = "diff --git a/f b/f\nnew file mode 100644\n--- /dev/null\n+++ b/f\n@@ -0,0 +1,2 @@\n+a\n+b\n";
        assert_eq!(shown(added, None, Some("a\nb\n")).unwrap(), ["+a", "+b"]);
        let deleted = "diff --git a/f b/f\ndeleted file mode 100644\n--- a/f\n+++ /dev/null\n@@ -1,2 +0,0 @@\n-a\n-b\n";
        assert_eq!(shown(deleted, Some("a\nb\n"), None).unwrap(), ["-a", "-b"]);
        // Renamed, and not changed: no hunk, every line outside.
        let renamed = "diff --git a/f b/g\nsimilarity index 100%\nrename from f\nrename to g\n";
        assert_eq!(shown(renamed, Some("a\nb\n"), Some("a\nb\n")).unwrap(), ["=1 a", "=2 b"]);
        // Not text: as its hunks.
        let binary = "diff --git a/f b/f\nindex 1..2 100644\nBinary files a/f and b/f differ\n";
        assert_eq!(shown(binary, None, None), None);
        assert_eq!(shown(binary, Some("a\n"), Some("a\n")), None, "git says it is not text");
    }

    #[test]
    fn a_hunk_with_no_line_on_a_side_and_no_newline_at_the_end_fit_the_file() {
        // `-U0`: two lines added after line 2, none removed; line 4 removed,
        // none added.
        let old = "1\n2\n3\n4\n5\n";
        let new = "1\n2\nx\ny\n3\n5\n";
        let diff = "diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -2,0 +3,2 @@\n+x\n+y\n@@ -4 +5,0 @@\n-4\n";
        assert_eq!(shown(diff, Some(old), Some(new)).unwrap(), ["=1 1", "=2 2", "+x", "+y", "=5 3", "-4", "=6 5"]);
        // git's `\` line, under the last line of each side.
        let diff = "diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -2,2 +2,2 @@\n 2\n-3\n\\ No newline at end of file\n+THREE\n\\ No newline at end of file\n";
        let got = shown(diff, Some("1\n2\n3"), Some("1\n2\nTHREE")).unwrap();
        assert_eq!(got, ["=1 1", " 2", "-3", "\\ No newline at end of file", "+THREE", "\\ No newline at end of file"]);
    }

    #[test]
    fn a_file_that_does_not_fit_its_hunks_is_not_shown_whole() {
        let diff = "diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -4,3 +4,3 @@\n 4\n-5\n+FIVE\n 6\n";
        // Shorter than the hunk says, or longer on one side than the other.
        assert_eq!(shown(diff, Some("1\n2\n"), Some("1\n2\n")), None);
        assert_eq!(shown(diff, Some("1\n2\n3\n4\n5\n6\n"), Some("1\n2\n3\n4\nFIVE\n6\n7\n")), None);
    }
}
