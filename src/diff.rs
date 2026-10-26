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

/// Stable anchor for a comment on a file as a whole, on its name: there as
/// long as the patch touches the file.
pub fn file_key(mode: &str, path: &str) -> String {
    format!("{mode}\0{path}\0file")
}

/// Whether `key` is a file's, as `file_key` makes them, not a line's.
pub fn is_file_key(key: &str) -> bool {
    key.ends_with("\0file") && key.matches('\0').count() == 2
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

/// A path as git writes it in a header: bare, or quoted as C would,
/// `"a/caf\303\251"`. `sided`: it has the `a/` or `b/` of its side.
pub fn unquote(p: &str, sided: bool) -> String {
    let Some(inner) = p.strip_prefix('"').and_then(|s| s.strip_suffix('"')) else {
        return p.to_string();
    };
    let mut out = Vec::new();
    let b = inner.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'\\' || i + 1 == b.len() {
            out.push(b[i]);
            i += 1;
            continue;
        }
        if let Some(o) = b.get(i + 1..i + 4).filter(|o| o.iter().all(|d| (b'0'..=b'7').contains(d))) {
            out.push(u8::from_str_radix(std::str::from_utf8(o).unwrap(), 8).unwrap_or(b'?'));
            i += 4;
            continue;
        }
        out.push(match b[i + 1] {
            b'a' => 7,
            b'b' => 8,
            b't' => b'\t',
            b'n' => b'\n',
            b'v' => 11,
            b'f' => 12,
            b'r' => b'\r',
            c => c,
        });
        i += 2;
    }
    let s = String::from_utf8_lossy(&out).into_owned();
    match s.get(..2) {
        Some("a/") | Some("b/") if sided => s[2..].to_string(),
        _ => s,
    }
}

/// One side of a `diff --git` line as written, bare or quoted, its
/// `prefix` (`a/`, `b/`) taken off; none if it has not that prefix.
fn sided(p: &str, prefix: &str) -> Option<String> {
    let p = if p.starts_with('"') { unquote(p, false) } else { p.to_string() };
    p.strip_prefix(prefix).map(String::from)
}

/// The path what follows `diff --git ` names, as git writes it: both
/// sides', bare or quoted, when they are the same path, as for any file not
/// renamed, one holding ` b/` too; else the new side's.
pub fn header_path(rest: &str) -> String {
    let rest = rest.trim_end();
    if rest.len() % 2 == 1 {
        let half = (rest.len() - 1) / 2;
        if rest.as_bytes()[half] == b' '
            && let (Some(a), Some(b)) = (rest.get(..half), rest.get(half + 1..))
            && let (Some(a), Some(b)) = (sided(a, "a/"), sided(b, "b/"))
            && a == b
        {
            return a;
        }
    }
    // Renamed: the new side, quoted, else after the first ` b/`.
    if rest.ends_with('"')
        && let Some(k) = rest.rfind(" \"b/")
        && let Some(b) = sided(&rest[k + 1..], "b/")
    {
        return b;
    }
    rest.split_once(" b/").map_or(rest, |(_, b)| b).to_string()
}

/// The path a `--- ` or `+++ ` line names, unquoted, its side's `a/` or
/// `b/` taken off, what follows a tab left out; none for `/dev/null`.
pub fn side_path(line: &str) -> Option<String> {
    let p = line.strip_prefix("--- ").or_else(|| line.strip_prefix("+++ "))?;
    let p = p.split('\t').next().unwrap_or(p);
    if p == "/dev/null" {
        return None;
    }
    Some(match p.starts_with('"') {
        true => unquote(p, true),
        false => p.strip_prefix("a/").or_else(|| p.strip_prefix("b/")).unwrap_or(p).to_string(),
    })
}

/// The path a line of a file's header gives its new side: `+++ b/X`,
/// `rename to X`, `copy to X`, bare or quoted; none for another line, or
/// `/dev/null`.
fn new_side(line: &str) -> Option<String> {
    if line.starts_with("+++ ") {
        return side_path(line);
    }
    let p = line.strip_prefix("rename to ").or_else(|| line.strip_prefix("copy to "))?;
    Some(unquote(p, false))
}

/// Which file of a diff a line is in, the diff read a line at a time: the
/// path its `diff --git` line names, as its header says it, before its
/// first hunk.
#[derive(Debug, Default)]
pub struct FileOf {
    pub path: String,
    /// Whether the line read last is of the file's header.
    pub header: bool,
}

impl FileOf {
    /// Line `line` read: whether a file starts there.
    pub fn read(&mut self, line: &str) -> bool {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            self.path = header_path(rest);
            self.header = true;
            return true;
        }
        if self.header {
            if line.starts_with("@@") {
                self.header = false;
            } else if let Some(p) = new_side(line) {
                self.path = p;
            }
        }
        false
    }
}

/// The paths of the files `diff` changes, as their headers name them.
pub fn paths(diff: &str) -> Vec<String> {
    let mut f = FileOf::default();
    let mut out: Vec<String> = Vec::new();
    for line in diff.lines() {
        if f.read(line) {
            out.push(String::new());
        }
        if f.header
            && let Some(last) = out.last_mut()
        {
            last.clone_from(&f.path);
        }
    }
    out
}

/// Parses a unified diff into the files of one source.
pub fn parse_diff(text: &str, mode: &str) -> Vec<FileDiff> {
    let mut files: Vec<FileDiff> = Vec::new();
    let mut counts: HashMap<(char, String), usize> = HashMap::new();
    let mut in_hunk = false;
    let (mut new_no, mut old_no) = (0usize, 0usize);

    for (at, line) in text.lines().enumerate() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            let path = header_path(rest);
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
            // Its new side's path, as the header says it after its first
            // line: quoted, or renamed.
            if let Some(p) = new_side(line) {
                current.path = p;
            }
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

    #[test]
    fn a_files_path_is_read_quoted_holding_spaces_renamed_or_gone() {
        // As git writes them: a name it quotes, one holding " b/" (its
        // header lines ending in a tab), one renamed, one gone.
        let diff = "diff --git \"a/caf\\303\\251.txt\" \"b/caf\\303\\251.txt\"\nnew file mode 100644\nindex 0000000..5626abf\n--- /dev/null\n+++ \"b/caf\\303\\251.txt\"\n@@ -0,0 +1 @@\n+one\n\
diff --git a/plan b/x.txt b/plan b/x.txt\nnew file mode 100644\nindex 0000000..f719efd\n--- /dev/null\n+++ b/plan b/x.txt\t\n@@ -0,0 +1 @@\n+two\n\
diff --git a/old.txt b/new.txt\nsimilarity index 100%\nrename from old.txt\nrename to new.txt\n\
diff --git a/gone.txt b/gone.txt\ndeleted file mode 100644\n--- a/gone.txt\n+++ /dev/null\n@@ -1 +0,0 @@\n-x\n";
        let files = parse_diff(diff, "m");
        let want = ["café.txt", "plan b/x.txt", "new.txt", "gone.txt"];
        assert_eq!(files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(), want);
        assert_eq!(paths(diff), want);
        // A line added that reads as a header is the file's still.
        let added = "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1 +1,2 @@\n a\n+++ b/other.txt\n";
        assert_eq!(paths(added), ["a.txt"]);
        assert_eq!(parse_diff(added, "m")[0].hunks[0].lines.len(), 2);
        // Renamed from a name git quotes; the sides of a line, as written.
        assert_eq!(header_path("\"a/caf\\303\\251.txt\" b/cafe.txt"), "cafe.txt");
        assert_eq!(side_path("--- \"a/sp\\tace\""), Some("sp\tace".to_string()));
        assert_eq!(side_path("+++ /dev/null"), None);
    }

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
