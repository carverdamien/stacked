//! The reading mode's marks, after self-review's view 3 (copied, not
//! shared): Space marks read the innermost block of code around the cursor
//! with lines not read yet, and pressed again, the block around that; Enter
//! takes back the latest mark there. Where a file's blocks cannot be found,
//! and once its blocks are all read, the block is the hunk.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::blocks::{FileBlocks, Side};
use crate::db::Mark;
use crate::rows::{Kind, Row};

/// Each file's code blocks, by path: None for a file they could not be
/// found in.
pub type Blocks = HashMap<String, Option<Arc<FileBlocks>>>;

/// What Space marks read: every changed line of a block, which is what the
/// mark holds, the lines of them it reads anew, and the block, said.
#[derive(Debug, Clone, PartialEq)]
pub struct Target {
    pub keys: HashSet<String>,
    pub new: HashSet<String>,
    pub what: String,
}

/// A patch's rows as the reading mode sees them: their blocks, the lines
/// read, and the marks that read them, oldest first.
pub struct Reading<'a> {
    pub rows: &'a [Row],
    pub blocks: &'a Blocks,
    pub read: &'a HashSet<String>,
    pub marks: &'a [(usize, &'a Mark)],
    /// Whether changed lines that are empty or white space alone need
    /// reading too: when not, no mark is needed on them, and nothing
    /// counts them; a mark of the block they are in still holds them.
    pub whitespace: bool,
}

fn changed(r: &Row) -> bool {
    matches!(r.kind, Kind::Add | Kind::Del)
}

impl Reading<'_> {
    /// Whether row `r` is a changed line that needs reading.
    fn needs(&self, r: &Row) -> bool {
        changed(r) && (self.whitespace || !crate::diff::white_space(&r.text))
    }

    /// The changed lines that need no reading, by key: white space alone,
    /// unless they need it. A line folded is white space alone when no
    /// piece of it needs reading.
    fn free(&self) -> HashSet<&str> {
        if self.whitespace {
            return HashSet::new();
        }
        let needed: HashSet<&str> = self.rows.iter().filter(|r| self.needs(r)).filter_map(|r| r.key.as_deref()).collect();
        self.rows
            .iter()
            .filter(|r| changed(r))
            .filter_map(|r| r.key.as_deref())
            .filter(|k| !needed.contains(k))
            .collect()
    }

    /// Whether the line keyed `k` needs reading and is not read.
    fn unread(&self, k: &str, free: &HashSet<&str>) -> bool {
        !self.read.contains(k) && !free.contains(k)
    }

    /// The diff line row `i` is, or the one the comment it is part of is
    /// on.
    pub fn line_at(&self, mut i: usize) -> Option<usize> {
        loop {
            let r = self.rows.get(i)?;
            if matches!(r.kind, Kind::Add | Kind::Del | Kind::Ctx) {
                return Some(i);
            }
            if !matches!(r.kind, Kind::Comment | Kind::Stream) || i == 0 {
                return None;
            }
            i -= 1;
        }
    }

    /// The changed lines of the hunk row `i` is the header or a line of.
    fn hunk_keys(&self, i: usize) -> HashSet<String> {
        let Some(h) = self.rows.get(i).and_then(|r| r.hunk.as_ref()) else {
            return HashSet::new();
        };
        self.rows
            .iter()
            .filter(|r| r.hunk.as_ref() == Some(h) && changed(r))
            .filter_map(|r| r.key.clone())
            .collect()
    }

    /// The changed lines of the file whose header is row `i`.
    fn file_keys(&self, i: usize) -> HashSet<String> {
        self.rows[i + 1..]
            .iter()
            .take_while(|r| r.kind != Kind::File)
            .filter(|r| changed(r))
            .filter_map(|r| r.key.clone())
            .collect()
    }

    fn file_blocks(&self, at: usize) -> Option<&Arc<FileBlocks>> {
        self.blocks.get(self.rows[at].path.as_deref()?)?.as_ref()
    }

    /// The blocks around line row `at` with changes in them, innermost
    /// first; None when its file's blocks were not found.
    fn chain(&self, at: usize) -> Option<Vec<&crate::blocks::Block>> {
        let fb = self.file_blocks(at)?;
        let key = self.rows[at].key.as_deref().unwrap_or("");
        Some(fb.chain(key).into_iter().map(|b| &fb.blocks[b]).collect())
    }

    /// What Space at row `i` marks read: the innermost block around its
    /// line with lines not read, else its hunk if it has some; on a hunk's
    /// header the hunk, on a file's the file. None when all there is read.
    /// A mark on a line stays in its hunk: a block reaching past it, as a
    /// long function does, is read there alone.
    pub fn to_mark(&self, i: usize) -> Option<Target> {
        let row = self.rows.get(i)?;
        let free = self.free();
        let whole = |keys: HashSet<String>, what: String| {
            let new: HashSet<String> = keys.iter().filter(|k| self.unread(k, &free)).cloned().collect();
            (!new.is_empty()).then_some(Target { keys, new, what })
        };
        let Some(at) = self.line_at(i) else {
            return match row.kind {
                Kind::Hunk => whole(self.hunk_keys(i), "this hunk".into()),
                Kind::File => whole(self.file_keys(i), "this file".into()),
                _ => None,
            };
        };
        let hunk = self.hunk_keys(at);
        for b in self.chain(at).unwrap_or_default() {
            let inside: HashSet<String> = b.changed.intersection(&hunk).cloned().collect();
            if inside.iter().any(|k| self.unread(k, &free)) {
                let old = if b.side == Side::Old { " as they were" } else { "" };
                let part = if inside.len() < b.changed.len() { ", in this hunk" } else { "" };
                // A run of code between empty lines is lines, not a block.
                let what = if b.run { "the lines" } else { "the block of lines" };
                return whole(inside, format!("{what} {}–{}{old}{part}", b.start, b.end));
            }
        }
        whole(hunk, "this hunk".into())
    }

    /// The mark Enter at row `i` takes back: the latest holding its line or
    /// a line of the innermost block around it (of its hunk, where its file
    /// has no blocks; of the hunk or the file a header heads); with the
    /// lines no other mark holds, which it leaves not read.
    pub fn to_unmark(&self, i: usize) -> Option<(usize, HashSet<String>)> {
        let row = self.rows.get(i)?;
        let near: HashSet<String> = match self.line_at(i) {
            None => match row.kind {
                Kind::Hunk => self.hunk_keys(i),
                Kind::File => self.file_keys(i),
                _ => return None,
            },
            Some(at) => {
                // Its file's blocks known: the innermost around it with
                // changes in its hunk, those changes alone, as a mark holds
                // them; or the line alone when it is in none, as self-review
                // does. With none known, marks are a hunk's.
                let hunk = self.hunk_keys(at);
                let mut near = match self.chain(at) {
                    Some(chain) => chain
                        .iter()
                        .map(|b| b.changed.intersection(&hunk).cloned().collect::<HashSet<String>>())
                        .find(|inside| !inside.is_empty())
                        .unwrap_or_default(),
                    None => hunk,
                };
                near.extend(self.rows[at].key.clone());
                near
            }
        };
        let (k, (rev, mark)) = self
            .marks
            .iter()
            .enumerate()
            .rev()
            .find(|(_, (_, m))| m.keys.iter().any(|key| near.contains(key)))?;
        let others: HashSet<&String> = self
            .marks
            .iter()
            .enumerate()
            .filter(|(n, _)| *n != k)
            .flat_map(|(_, (_, m))| m.keys.iter())
            .collect();
        let free = self.free();
        Some((*rev, mark.keys.iter().filter(|key| !others.contains(key) && !free.contains(key.as_str())).cloned().collect()))
    }

    /// The first changed line not read after row `i`, or before it: of
    /// those that need reading; a line folded, at its first piece.
    pub fn next_unread(&self, i: usize, forward: bool) -> Option<usize> {
        let free = self.free();
        let unread = |j: &usize| {
            let r = &self.rows[*j];
            changed(r) && !r.cont && r.key.as_deref().is_some_and(|k| self.unread(k, &free))
        };
        if forward {
            (i + 1..self.rows.len()).find(unread)
        } else {
            (0..i.min(self.rows.len())).rev().find(unread)
        }
    }

    /// How many changed lines that need reading are read, of how many: a
    /// line folded, once.
    pub fn progress(&self) -> (usize, usize) {
        let keys: HashSet<&String> = self.rows.iter().filter(|r| self.needs(r)).filter_map(|r| r.key.as_ref()).collect();
        (keys.iter().filter(|k| self.read.contains(**k)).count(), keys.len())
    }
}

/// The code blocks of `patches`' last, each of its files' from the file as
/// it is before and after it: on `base` in `repo`, the patches below
/// applied. Its files' blocks are None where they cannot be found, all of
/// them if the patches cannot be applied.
pub fn find(repo: &std::path::Path, base: &str, patches: &[&crate::db::Patch]) -> Blocks {
    let Some(last) = patches.last() else {
        return Blocks::new();
    };
    let files = crate::diff::parse_diff(&last.diff, &crate::view::mode(&last.change));
    let sides = crate::check::versions(repo, base, patches).unwrap_or_default();
    files
        .iter()
        .map(|fd| {
            let s = sides.get(&fd.path);
            let blocks = s.and_then(|s| {
                crate::blocks::file_blocks(fd, s.new.as_deref(), s.old.as_deref()).map(Arc::new)
            });
            (fd.path.clone(), blocks)
        })
        .collect()
}

/// The code blocks of the patches read, by `key`: those found, and those
/// being looked for, in threads of their own, as self-review does; the
/// screen does not wait on them.
#[derive(Default)]
pub struct Finder {
    found: HashMap<String, Arc<Blocks>>,
    going: HashMap<String, std::thread::JoinHandle<Blocks>>,
}

/// What blocks are looked for in: a repository, a base, and the patches
/// up to the one whose blocks they are.
pub type Job = (std::path::PathBuf, String, Vec<crate::db::Patch>);

/// What a stack's patch `k` is read against: its base and the patches up
/// to it, which is what its blocks depend on.
pub fn key(base: Option<&str>, ids: &[String]) -> String {
    format!("{}\0{}", base.unwrap_or(""), ids.join("\0"))
}

impl Finder {
    /// The blocks of `key`, if found by now.
    pub fn peek(&self, key: &str) -> Option<Arc<Blocks>> {
        self.found.get(key).cloned()
    }

    /// The blocks of `key`, if found; else they are looked for with what
    /// `job` gives (if not already), and, with nothing to look in, there
    /// are none.
    pub fn get(&mut self, key: &str, job: impl FnOnce() -> Option<Job>) -> Option<Arc<Blocks>> {
        if let Some(b) = self.found.get(key) {
            return Some(b.clone());
        }
        if let Some(h) = self.going.get(key) {
            if !h.is_finished() {
                return None;
            }
            let b = Arc::new(self.going.remove(key).unwrap().join().unwrap_or_default());
            self.found.insert(key.into(), b.clone());
            return Some(b);
        }
        match job() {
            None => {
                let b = Arc::new(Blocks::new());
                self.found.insert(key.into(), b.clone());
                Some(b)
            }
            Some((repo, base, patches)) => {
                let h = std::thread::spawn(move || {
                    let refs: Vec<&crate::db::Patch> = patches.iter().collect();
                    find(&repo, &base, &refs)
                });
                self.going.insert(key.into(), h);
                None
            }
        }
    }

    /// Whether blocks are being looked for.
    pub fn busy(&self) -> bool {
        !self.going.is_empty()
    }

    /// Waits for every search to end.
    pub fn wait(&mut self) {
        for (key, h) in self.going.drain() {
            self.found.insert(key, Arc::new(h.join().unwrap_or_default()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Patch;
    use crate::rows::{Deco, patch_rows};

    fn lines(s: &str) -> Vec<String> {
        s.lines().map(String::from).collect()
    }

    /// A patch changing two lines of a function, one of them in an `if`:
    /// its rows and blocks.
    fn patch() -> (Vec<Row>, Blocks) {
        let new = lines("top\n\nfn a() {\n  one\n  if x {\n    two\n  }\n}\n");
        let diff = "diff --git a/f.rs b/f.rs\n--- a/f.rs\n+++ b/f.rs\n@@ -1,8 +1,8 @@\n top\n \n fn a() {\n-  uno\n+  one\n   if x {\n-    dos\n+    two\n   }\n }\n";
        let p = Patch::new("c".into(), None, None, "t".into(), diff.into());
        let fd = &crate::diff::parse_diff(diff, &crate::view::mode("c"))[0];
        let fb = crate::blocks::file_blocks(fd, Some(&new), None).map(Arc::new);
        let rows = patch_rows(&p, &[], 80, &Deco::default());
        (rows, Blocks::from([("f.rs".to_string(), fb)]))
    }

    fn row(rows: &[Row], text: &str) -> usize {
        rows.iter().position(|r| r.text == text).unwrap()
    }

    #[test]
    fn space_reads_a_block_then_the_one_around_it_then_the_hunk() {
        let (rows, blocks) = patch();
        let two = row(&rows, "+    two");
        let mut read = HashSet::new();
        let marks: Vec<(usize, &Mark)> = vec![];
        let r = Reading { rows: &rows, blocks: &blocks, read: &read, marks: &marks, whitespace: false };
        let t = r.to_mark(two).unwrap();
        assert_eq!((t.what.as_str(), t.new.len()), ("the block of lines 5–7", 2));
        read.extend(t.keys);
        let r = Reading { rows: &rows, blocks: &blocks, read: &read, marks: &marks, whitespace: false };
        let t = r.to_mark(two).unwrap();
        assert_eq!((t.what.as_str(), t.keys.len(), t.new.len()), ("the block of lines 3–8", 4, 2));
        read.extend(t.keys);
        let r = Reading { rows: &rows, blocks: &blocks, read: &read, marks: &marks, whitespace: false };
        assert_eq!(r.to_mark(two), None, "all read");
        assert_eq!(r.progress(), (4, 4));
        // With no blocks to go by, the hunk.
        let none = Blocks::from([("f.rs".to_string(), None)]);
        let unread = HashSet::new();
        let r = Reading { rows: &rows, blocks: &none, read: &unread, marks: &marks, whitespace: false };
        assert_eq!(r.to_mark(two).map(|t| (t.what, t.new.len())), Some(("this hunk".into(), 4)));
        assert_eq!(r.next_unread(0, true), Some(row(&rows, "-  uno")));
    }

    #[test]
    fn a_block_s_header_reads_it_whole_and_its_code_between_empty_lines_a_run_at_a_time() {
        let old = "fn a() {\n    let x = 1;\n    let y = 3;\n\n    v();\n}\n";
        let new = old.replace("y = 3", "y = 2").replace("v()", "w()");
        let d = similar::TextDiff::from_lines(old, &new).unified_diff().context_radius(20).header("a/f.rs", "b/f.rs").to_string();
        let diff = format!("diff --git a/f.rs b/f.rs\n{d}");
        let p = Patch::new("c".into(), None, None, "t".into(), diff.clone());
        let fd = &crate::diff::parse_diff(&diff, &crate::view::mode("c"))[0];
        let fb = crate::blocks::file_blocks(fd, Some(&lines(&new)), None).map(Arc::new);
        let blocks = Blocks::from([("f.rs".to_string(), fb)]);
        let rows = patch_rows(&p, &[], 80, &Deco::default());
        let mut read = HashSet::new();
        // On its first lines, their run first, then the function.
        let x = row(&rows, "     let x = 1;");
        let r = Reading { rows: &rows, blocks: &blocks, read: &read, marks: &[], whitespace: false };
        let run = r.to_mark(x).unwrap();
        assert_eq!((run.what.as_str(), run.new.len()), ("the lines 2–3", 2));
        read.extend(run.keys);
        let r = Reading { rows: &rows, blocks: &blocks, read: &read, marks: &[], whitespace: false };
        assert_eq!(r.to_mark(x).map(|t| (t.what, t.new.len())), Some(("the block of lines 1–6".into(), 2)));
        // On its header, the function whole, at once.
        let header = row(&rows, " fn a() {");
        let none = HashSet::new();
        let r = Reading { rows: &rows, blocks: &blocks, read: &none, marks: &[], whitespace: false };
        assert_eq!(r.to_mark(header).map(|t| (t.what, t.new.len())), Some(("the block of lines 1–6".into(), 4)));
    }

    #[test]
    fn a_block_over_two_hunks_is_read_a_hunk_at_a_time() {
        // A function changed near its top and near its end, far enough apart
        // to make two hunks.
        let body: Vec<String> = (1..=14).map(|n| format!("  line{n}\n")).collect();
        let old = format!("fn a() {{\n  one\n  two\n{}  three\n  four\n}}\n", body.concat());
        let new = old.replace("  two\n", "  TWO\n").replace("  three\n", "  THREE\n");
        let d = similar::TextDiff::from_lines(&old, &new).unified_diff().context_radius(3).header("a/f.rs", "b/f.rs").to_string();
        let diff = format!("diff --git a/f.rs b/f.rs\n{d}");
        let p = Patch::new("c".into(), None, None, "t".into(), diff.clone());
        let fd = &crate::diff::parse_diff(&diff, &crate::view::mode("c"))[0];
        assert_eq!(fd.hunks.len(), 2, "{diff}");
        let fb = crate::blocks::file_blocks(fd, Some(&lines(&new)), None).map(Arc::new);
        let blocks = Blocks::from([("f.rs".to_string(), fb)]);
        let rows = patch_rows(&p, &[], 80, &Deco::default());
        let (top, end) = (row(&rows, "+  TWO"), row(&rows, "+  THREE"));
        // Space in the first hunk reads the function there alone.
        let mut read = HashSet::new();
        let r = Reading { rows: &rows, blocks: &blocks, read: &read, marks: &[], whitespace: false };
        let first = r.to_mark(top).unwrap();
        assert_eq!((first.what.as_str(), first.new.len()), ("the block of lines 2–18, in this hunk", 2));
        read.extend(first.keys.clone());
        let r = Reading { rows: &rows, blocks: &blocks, read: &read, marks: &[], whitespace: false };
        assert_eq!(r.progress(), (2, 4), "the other hunk is not read");
        assert_eq!(r.to_mark(top), None, "its hunk is read");
        let second = r.to_mark(end).unwrap();
        assert_eq!(second.new.len(), 2);
        // Enter in the first hunk takes back its own mark, not the one made
        // after it in the other hunk.
        read.extend(second.keys.clone());
        let m1 = Mark { keys: first.keys.into_iter().collect(), ..Default::default() };
        let m2 = Mark { keys: second.keys.into_iter().collect(), ..Default::default() };
        let marks = vec![(3, &m1), (5, &m2)];
        let r = Reading { rows: &rows, blocks: &blocks, read: &read, marks: &marks, whitespace: false };
        assert_eq!(r.to_unmark(top).map(|(rev, gone)| (rev, gone.len())), Some((3, 2)));
        assert_eq!(r.to_unmark(end).map(|(rev, _)| rev), Some(5));
    }

    #[test]
    fn lines_of_white_space_alone_need_no_mark_unless_the_settings_say_so() {
        // A function given a line of code, then an empty line and a blank one.
        let old = "fn a() {\n  one\n}\n";
        let new = "fn a() {\n  one\n  two\n\n  \n}\n";
        let d = similar::TextDiff::from_lines(old, new).unified_diff().context_radius(20).header("a/f.rs", "b/f.rs").to_string();
        let diff = format!("diff --git a/f.rs b/f.rs\n{d}");
        let p = Patch::new("c".into(), None, None, "t".into(), diff);
        let none = Blocks::from([("f.rs".to_string(), None)]);
        let rows = patch_rows(&p, &[], 80, &Deco::default());
        let two = row(&rows, "+  two");
        let unread = HashSet::new();
        let r = Reading { rows: &rows, blocks: &none, read: &unread, marks: &[], whitespace: false };
        assert_eq!(r.progress(), (0, 1), "only the line of code needs reading");
        let t = r.to_mark(two).unwrap();
        assert_eq!((t.what.as_str(), t.keys.len(), t.new.len()), ("this hunk", 3, 1), "the mark holds the hunk's lines, one of them new to read");
        // That line read: nothing is left there, nor below.
        let read: HashSet<String> = rows[two].key.iter().cloned().collect();
        let r = Reading { rows: &rows, blocks: &none, read: &read, marks: &[], whitespace: false };
        assert_eq!(r.progress(), (1, 1));
        assert_eq!(r.to_mark(two), None);
        assert_eq!(r.next_unread(0, true), None, "the empty line and the blank one are passed over");
        // Enter, the hunk marked: the line of code alone is left not read.
        let m = Mark { keys: t.keys.iter().cloned().collect(), ..Default::default() };
        let marks = vec![(4, &m)];
        let all: HashSet<String> = t.keys.clone();
        let r = Reading { rows: &rows, blocks: &none, read: &all, marks: &marks, whitespace: false };
        assert_eq!(r.to_unmark(two).map(|(rev, gone)| (rev, gone.len())), Some((4, 1)));
        // When they need reading, they are read as any other line.
        let r = Reading { rows: &rows, blocks: &none, read: &read, marks: &[], whitespace: true };
        assert_eq!(r.progress(), (1, 3));
        assert_eq!(r.to_mark(two).map(|t| t.new.len()), Some(2));
        assert_eq!(r.next_unread(0, true), Some(row(&rows, "+")));
        // The hunk's and the file's counts, reading, leave them out too, and
        // they are drawn as lines that need nothing.
        let deco = Deco { read: Some(HashSet::new()), ..Default::default() };
        let rows = patch_rows(&p, &[], 80, &deco);
        assert_eq!(rows.iter().filter(|r| r.text.ends_with("   read 0/1 lines")).count(), 2, "{rows:#?}");
        assert_eq!(rows.iter().filter(|r| r.free).map(|r| r.text.as_str()).collect::<Vec<_>>(), ["+", "+  "]);
        let deco = Deco { read: Some(HashSet::new()), whitespace: true, ..Default::default() };
        let rows = patch_rows(&p, &[], 80, &deco);
        assert_eq!(rows.iter().filter(|r| r.text.ends_with("   read 0/3 lines")).count(), 2);
        assert!(!rows.iter().any(|r| r.free));
    }

    #[test]
    fn a_line_folded_is_one_line_to_read() {
        // Two lines added: one too wide for the pane, its first piece white
        // space alone, its code in the next; and one short.
        let deep = format!("{}code", " ".repeat(40));
        let diff = format!("diff --git a/f.txt b/f.txt\n--- a/f.txt\n+++ b/f.txt\n@@ -0,0 +1,2 @@\n+{deep}\n+short\n");
        let p = Patch::new("c".into(), None, None, "t".into(), diff);
        let rows = patch_rows(&p, &[], 30, &Deco::default());
        let lines: Vec<usize> = (0..rows.len()).filter(|&i| rows[i].kind == Kind::Add).collect();
        assert_eq!(lines.len(), 3, "{rows:#?}");
        assert!(rows[lines[1]].cont && !rows[lines[2]].cont);
        let none = Blocks::new();
        let r = Reading { rows: &rows, blocks: &none, read: &HashSet::new(), marks: &[], whitespace: false };
        assert_eq!(r.progress(), (0, 2), "the line folded counted once, its code in its second piece");
        assert_eq!(r.next_unread(0, true), Some(lines[0]));
        assert_eq!(r.next_unread(lines[0], true), Some(lines[2]), "past its second piece, to the next line");
        let read = HashSet::from([rows[lines[0]].key.clone().unwrap()]);
        let r = Reading { rows: &rows, blocks: &none, read: &read, marks: &[], whitespace: false };
        assert_eq!(r.progress(), (1, 2));
    }

    #[test]
    fn enter_takes_back_the_latest_mark_around_the_line() {
        let (rows, blocks) = patch();
        let two = row(&rows, "+    two");
        let r0 = Reading { rows: &rows, blocks: &blocks, read: &HashSet::new(), marks: &[], whitespace: false };
        let inner = r0.to_mark(two).unwrap();
        let read1: HashSet<String> = inner.keys.clone();
        let r1 = Reading { rows: &rows, blocks: &blocks, read: &read1, marks: &[], whitespace: false };
        let outer = r1.to_mark(two).unwrap();
        let m1 = Mark { keys: inner.keys.iter().cloned().collect(), ..Default::default() };
        let m2 = Mark { keys: outer.keys.iter().cloned().collect(), ..Default::default() };
        let marks = vec![(5, &m1), (9, &m2)];
        let read: HashSet<String> = m1.keys.iter().chain(&m2.keys).cloned().collect();
        let r = Reading { rows: &rows, blocks: &blocks, read: &read, marks: &marks, whitespace: false };
        // The outer mark, the latest: the lines only it holds go unread.
        let (rev, gone) = r.to_unmark(two).unwrap();
        assert_eq!((rev, gone.len()), (9, 2));
        let marks = vec![(5, &m1)];
        let r = Reading { rows: &rows, blocks: &blocks, read: &read1, marks: &marks, whitespace: false };
        assert_eq!(r.to_unmark(two).map(|(rev, g)| (rev, g.len())), Some((5, 2)));
        // Nothing marked on the other line's block: the outer block's
        // innermost changed block is the function, which holds `one`.
        let one = row(&rows, "+  one");
        let only_if = vec![];
        let r = Reading { rows: &rows, blocks: &blocks, read: &read1, marks: &only_if, whitespace: false };
        assert_eq!(r.to_unmark(one), None);
        // A line in no block with changes takes back only a mark that holds
        // it; with no blocks known, the hunk's.
        let top = row(&rows, " top");
        let hunk = Mark { keys: r0.hunk_keys(two).into_iter().collect(), ..Default::default() };
        let marks = vec![(3, &hunk)];
        let r = Reading { rows: &rows, blocks: &blocks, read: &read, marks: &marks, whitespace: false };
        assert_eq!(r.to_unmark(top), None);
        let none = Blocks::from([("f.rs".to_string(), None)]);
        let r = Reading { rows: &rows, blocks: &none, read: &read, marks: &marks, whitespace: false };
        assert_eq!(r.to_unmark(top).map(|(rev, _)| rev), Some(3));
    }
}
