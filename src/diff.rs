//! The diff model: files, hunks and lines, each line keyed by its content so
//! comments survive line numbers moving.

use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    /// ' ' context, '+' add, '-' del.
    pub origin: char,
    /// The full line, origin character included.
    pub text: String,
    /// A stable, content-based anchor for comments.
    pub key: String,
    /// Line numbers on each side; on the side a line is missing from, where
    /// it would sit -- the number of the line that follows it there.
    pub old_no: usize,
    pub new_no: usize,
    /// Its line in the diff's text, from 0.
    pub at: usize,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Hunk {
    pub header: String,
    /// Stable id: mode\0path\0old_start\0new_start.
    pub hid: String,
    /// Content-based id for the "reviewed" mark.
    pub hkey: String,
    pub new_start: usize,
    /// Its header's line in the diff's text, from 0.
    pub at: usize,
    /// New-side line numbers of the first and last present (ctx/+) lines.
    pub first_new: usize,
    pub last_new: usize,
    pub lines: Vec<Line>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FileDiff {
    /// The source key ('staged', 'unstaged', 'commit:...').
    pub mode: String,
    pub path: String,
    pub header: Vec<String>,
    pub hunks: Vec<Hunk>,
    /// Its `diff --git` line in the diff's text, from 0; its header's lines
    /// follow it.
    pub at: usize,
}

impl FileDiff {
    /// What identifies both versions of the file: the blob ids on its
    /// `index` line, else (a mode change, say) its hunks.
    pub fn signature(&self) -> String {
        match self.header.iter().find_map(|l| l.strip_prefix("index ")) {
            Some(ids) => format!("{}\0{ids}", self.path),
            None => {
                let mut parts = vec![self.path.clone()];
                for h in &self.hunks {
                    parts.push(h.header.clone());
                    parts.extend(h.lines.iter().map(|l| l.text.clone()));
                }
                sha1_12(&parts.join("\n"))
            }
        }
    }
}

/// Whether line `text`, its origin first, is white space alone, or
/// nothing: a changed one of those needs no review, unless the settings
/// say it does (`view.whitespace_needs_review`).
pub fn white_space(text: &str) -> bool {
    text.chars().skip(1).all(char::is_whitespace)
}

pub fn sha1_12(text: &str) -> String {
    let mut h = sha1_smol::Sha1::new();
    h.update(text.as_bytes());
    h.digest().to_string()[..12].to_string()
}

/// Content-based anchor so comments survive line-number drift.
pub fn line_key(mode: &str, path: &str, origin: char, content: &str, ordinal: usize) -> String {
    format!("{mode}\0{path}\0{origin}\0{ordinal}\0{}", sha1_12(content))
}

/// Stable anchor for a comment on a whole section.
pub fn section_key(mode: &str) -> String {
    format!("{mode}\0\0section")
}

/// Content-based id for the "reviewed" mark, from the hunk's +/- lines.
pub fn hunk_key(mode: &str, path: &str, signature: &str, ordinal: usize) -> String {
    format!("{mode}\0{path}\0{ordinal}\0{}", sha1_12(signature))
}

/// `@@ -(\d+)(?:,\d+)? \+(\d+)(?:,(\d+))? @@`: the old and new starts.
fn hunk_starts(line: &str) -> Option<(usize, usize)> {
    let rest = line.strip_prefix("@@ -")?;
    let (old, rest) = rest.split_once(' ')?;
    let old: usize = old.split(',').next()?.parse().ok()?;
    let rest = rest.strip_prefix('+')?;
    let (new, rest) = rest.split_once(' ')?;
    let mut parts = new.split(',');
    let start: usize = parts.next()?.parse().ok()?;
    if let Some(n) = parts.next() {
        n.parse::<usize>().ok()?;
    }
    rest.starts_with("@@").then_some((old, start))
}

/// Parses a unified diff into the files of one source.
pub fn parse_diff(text: &str, mode: &str) -> Vec<FileDiff> {
    let mut files: Vec<FileDiff> = Vec::new();
    let mut counts: HashMap<(char, String), usize> = HashMap::new();
    let mut in_hunk = false;
    let (mut new_no, mut old_no) = (0usize, 0usize);

    for (at, line) in text.lines().enumerate() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            let path = match line.split_once(" b/") {
                Some((_, p)) => p.to_string(),
                None => rest.to_string(),
            };
            files.push(FileDiff {
                mode: mode.into(),
                path,
                header: vec![line.into()],
                hunks: Vec::new(),
                at,
            });
            in_hunk = false;
            counts.clear();
            continue;
        }
        let Some(current) = files.last_mut() else {
            continue;
        };
        if line.starts_with("@@") {
            let (old_start, new_start) = hunk_starts(line).unwrap_or((0, 0));
            current.hunks.push(Hunk {
                header: line.into(),
                new_start,
                hid: format!("{mode}\0{}\0{old_start}\0{new_start}", current.path),
                at,
                ..Default::default()
            });
            new_no = new_start;
            old_no = old_start;
            in_hunk = true;
            continue;
        }
        if !in_hunk {
            current.header.push(line.into());
            continue;
        }
        let origin = line.chars().next().unwrap_or(' ');
        let content: String = line.chars().skip(1).collect();
        let slot = (origin, content.clone());
        let ordinal = *counts.get(&slot).unwrap_or(&0);
        counts.insert(slot, ordinal + 1);
        let key = line_key(mode, &current.path, origin, &content, ordinal);
        let hunk = current.hunks.last_mut().unwrap();
        hunk.lines.push(Line {
            origin,
            text: line.into(),
            key,
            old_no,
            new_no,
            at,
        });
        if origin != '-' {
            if hunk.last_new == 0 {
                hunk.first_new = new_no;
            }
            hunk.last_new = new_no;
            new_no += 1;
        }
        if origin != '+' {
            old_no += 1;
        }
    }

    for fd in &mut files {
        let mut hcounts: HashMap<String, usize> = HashMap::new();
        for h in &mut fd.hunks {
            let sig = h
                .lines
                .iter()
                .filter(|l| l.origin == '+' || l.origin == '-')
                .map(|l| l.text.as_str())
                .collect::<Vec<_>>()
                .join("\n");
            let ordinal = *hcounts.get(&sig).unwrap_or(&0);
            hcounts.insert(sig.clone(), ordinal + 1);
            h.hkey = hunk_key(&fd.mode, &fd.path, &sig, ordinal);
        }
    }
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIFF: &str = "diff --git a/f.txt b/f.txt\nindex 1..2 100644\n--- a/f.txt\n+++ b/f.txt\n\
@@ -1,3 +1,3 @@\n a\n-b\n+B\n c\n@@ -10,2 +10,3 @@\n x\n+y\n+y\n z\n";

    #[test]
    fn parses_files_hunks_and_numbers() {
        let files = parse_diff(DIFF, "unstaged");
        assert_eq!(files.len(), 1);
        let f = &files[0];
        assert_eq!(f.path, "f.txt");
        assert_eq!(f.header.len(), 4);
        assert_eq!(f.hunks.len(), 2);
        let h = &f.hunks[0];
        assert_eq!((h.first_new, h.last_new), (1, 3));
        let nos: Vec<_> = h
            .lines
            .iter()
            .map(|l| (l.origin, l.old_no, l.new_no))
            .collect();
        assert_eq!(nos, [(' ', 1, 1), ('-', 2, 2), ('+', 3, 2), (' ', 3, 3)]);
        // Identical lines get distinct keys by ordinal.
        let ys: Vec<_> = f.hunks[1]
            .lines
            .iter()
            .filter(|l| l.origin == '+')
            .map(|l| &l.key)
            .collect();
        assert_ne!(ys[0], ys[1]);
    }

    #[test]
    fn keys_are_stable() {
        // sha1("b")[:12] is e9d71f5ee7c9, as hashlib computes it.
        assert_eq!(
            line_key("unstaged", "f.txt", '-', "b", 0),
            "unstaged\0f.txt\0-\00\0e9d71f5ee7c9"
        );
        assert_eq!(section_key("staged"), "staged\0\0section");
    }

    #[test]
    fn hunk_keys_ignore_context() {
        let a = parse_diff(DIFF, "m");
        let wider = DIFF.replace("@@ -1,3 +1,3 @@\n a\n", "@@ -1,3 +1,3 @@\n z\n a\n");
        let b = parse_diff(&wider, "m");
        assert_eq!(a[0].hunks[0].hkey, b[0].hunks[0].hkey);
    }
}
