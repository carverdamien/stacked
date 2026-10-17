//! Code blocks, for marking a patch read a block at a time (the reading
//! mode), after self-review's (copied, not shared):
//! from a tree-sitter grammar where the file's language has one, else from
//! its braces.

use std::collections::{HashMap, HashSet};

use crate::diff::FileDiff;

/// The grammar for a file, by its extension.
fn language(path: &str) -> Option<tree_sitter::Language> {
    let ext = path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase())?;
    Some(match ext.as_str() {
        "rs" => tree_sitter_rust::LANGUAGE.into(),
        "c" => tree_sitter_c::LANGUAGE.into(),
        "h" | "cc" | "cpp" | "cxx" | "hh" | "hpp" | "hxx" | "mm" | "inl" => {
            tree_sitter_cpp::LANGUAGE.into()
        }
        "py" | "pyi" | "pyw" => tree_sitter_python::LANGUAGE.into(),
        "js" | "jsm" | "mjs" | "cjs" | "jsx" | "sys.mjs" => tree_sitter_javascript::LANGUAGE.into(),
        "ts" | "mts" | "cts" => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        "tsx" => tree_sitter_typescript::LANGUAGE_TSX.into(),
        "java" => tree_sitter_java::LANGUAGE.into(),
        "go" => tree_sitter_go::LANGUAGE.into(),
        _ => return None,
    })
}

/// A body that only restates its parent: the block after a Python `def`, a
/// class's member list. Its parent is the block.
fn is_body(kind: &str) -> bool {
    kind == "block"
        || kind.ends_with("_body")
        || kind == "body"
        || kind.ends_with("declaration_list")
        || kind == "compound_statement"
        || kind == "statement_block"
}

/// The blocks a grammar finds in `lines` as `(first, last)` line numbers
/// (from 1): every named node over more than one line, a node with the same
/// lines as its parent once, and a body ending with its parent left to the
/// parent. None when the file is no language this knows, or nothing in it
/// spans lines.
pub fn grammar_blocks(path: &str, lines: &[String]) -> Option<Vec<(usize, usize)>> {
    let lang = language(path)?;
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&lang).ok()?;
    let text = lines.join("\n");
    let tree = parser.parse(&text, None)?;
    let mut spans: HashSet<(usize, usize)> = HashSet::new();
    let mut cursor = tree.walk();
    let mut stack = vec![(tree.root_node(), None::<(usize, usize)>)];
    while let Some((node, parent)) = stack.pop() {
        let (start, end) = (node.start_position(), node.end_position());
        // A node taking its line's newline ends at the start of the next.
        let last = if end.column == 0 && end.row > start.row {
            end.row - 1
        } else {
            end.row
        };
        let span = (start.row + 1, last + 1);
        let mut keep = node.is_named() && span.1 > span.0 && node.kind() != "comment";
        if node.parent().is_none() {
            keep = false;
        }
        if let Some(p) = parent
            && keep
            && is_body(node.kind())
            && span.1 == p.1
            && span.0 <= p.0 + 1
        {
            keep = false;
        }
        if keep {
            spans.insert(span);
        }
        let here = if keep { Some(span) } else { parent };
        for child in node.children(&mut cursor) {
            stack.push((child, here));
        }
    }
    if spans.is_empty() {
        return None;
    }
    let mut out: Vec<(usize, usize)> = spans.into_iter().collect();
    out.sort_by_key(|&(s, e)| (e, std::cmp::Reverse(s)));
    Some(out)
}

/// A file's blocks: its grammar's, else its braces'.
pub fn code_blocks(path: &str, lines: &[String]) -> Option<Vec<(usize, usize)>> {
    if language(path).is_some() {
        return grammar_blocks(path, lines);
    }
    brace_blocks(lines)
}

/// The `{ }` blocks of a file as `(open, close)` line numbers (from 1), or
/// None when it has no braces or they do not balance. A best-effort lexer:
/// braces in strings, character literals and comments (`//`, `/* */`, and
/// `#` followed by a space or `!`) are skipped. A `'` only opens a string
/// when it closes on the same line, so a Rust lifetime does not swallow the
/// rest of it.
pub fn brace_blocks(lines: &[String]) -> Option<Vec<(usize, usize)>> {
    let mut blocks = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    // None, or what closes the string/comment we are in.
    let mut state: Option<String> = None;
    for (no, line) in lines.iter().enumerate() {
        let no = no + 1;
        let c: Vec<char> = line.chars().collect();
        let n = c.len();
        let starts = |i: usize, s: &str| {
            let s: Vec<char> = s.chars().collect();
            i + s.len() <= n && c[i..i + s.len()] == s[..]
        };
        let mut i = 0;
        while i < n {
            let ch = c[i];
            match state.as_deref() {
                Some("/*") => {
                    if starts(i, "*/") {
                        state = None;
                        i += 2;
                    } else {
                        i += 1;
                    }
                    continue;
                }
                Some(s @ ("\"\"\"" | "'''")) => {
                    if starts(i, s) {
                        state = None;
                        i += 3;
                    } else {
                        i += if ch == '\\' { 2 } else { 1 };
                    }
                    continue;
                }
                Some(q @ ("`" | "\"" | "'")) => {
                    if ch == '\\' {
                        i += 2;
                        continue;
                    }
                    if q.starts_with(ch) {
                        state = None;
                    }
                    i += 1;
                    continue;
                }
                _ => {}
            }
            if starts(i, "//") {
                break;
            }
            if starts(i, "/*") {
                state = Some("/*".into());
                i += 2;
                continue;
            }
            if ch == '#' && (i + 1 == n || c[i + 1] == ' ' || c[i + 1] == '!') {
                break;
            }
            if starts(i, "\"\"\"") || starts(i, "'''") {
                state = Some(c[i..i + 3].iter().collect());
                i += 3;
                continue;
            }
            if ch == '`' || ch == '"' || (ch == '\'' && c[i + 1..].contains(&'\'')) {
                state = Some(ch.to_string());
                i += 1;
                continue;
            }
            if ch == '{' {
                stack.push(no);
            } else if ch == '}' {
                let open = stack.pop()?;
                blocks.push((open, no));
            }
            i += 1;
        }
        // An unterminated quote ends with its line.
        if matches!(state.as_deref(), Some("\"") | Some("'")) {
            state = None;
        }
    }
    if !stack.is_empty() || blocks.is_empty() {
        return None;
    }
    Some(blocks)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Side {
    New,
    Old,
}

/// A code block, or a run of top-level code between empty lines. `side` says
/// which version of the file `start`/`end` number: the new one, or the old
/// one for a block the change removes whole. `changed` holds the keys of every
/// changed diff line in it, nested blocks' included.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Block {
    pub start: usize,
    pub end: usize,
    pub side: Side,
    pub parent: Option<usize>,
    pub depth: usize,
    pub changed: HashSet<String>,
    pub run: bool,
}

/// The blocks of one file of a diff, and each diff line's innermost one.
#[derive(Debug, Clone)]
pub struct FileBlocks {
    pub blocks: Vec<Block>,
    pub line_block: HashMap<String, usize>,
}

impl FileBlocks {
    /// The blocks around the diff line `key`, innermost first, keeping only
    /// those with changes in them.
    pub fn chain(&self, key: &str) -> Vec<usize> {
        let mut out = Vec::new();
        let mut b = self.line_block.get(key).copied();
        while let Some(i) = b {
            if !self.blocks[i].changed.is_empty() {
                out.push(i);
            }
            b = self.blocks[i].parent;
        }
        out
    }
}

/// Blocks for `(start, end)` spans, each given its innermost enclosing span
/// as parent; indices start at `offset`.
fn nest(
    spans: &[(usize, usize)],
    side: Side,
    runs: &[(usize, usize)],
    offset: usize,
) -> Vec<Block> {
    let mut sorted = spans.to_vec();
    sorted.sort_by_key(|&(s, e)| (s, std::cmp::Reverse(e)));
    let runs: HashSet<(usize, usize)> = runs.iter().copied().collect();
    let mut blocks: Vec<Block> = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    for (start, end) in sorted {
        while let Some(&top) = stack.last() {
            if blocks[top].end < start {
                stack.pop();
            } else {
                break;
            }
        }
        let parent = stack.last().copied();
        let depth = parent.map_or(0, |p| blocks[p].depth + 1);
        blocks.push(Block {
            start,
            end,
            side,
            parent: parent.map(|p| p + offset),
            depth,
            changed: HashSet::new(),
            run: runs.contains(&(start, end)),
        });
        stack.push(blocks.len() - 1);
    }
    blocks
}

/// Line number -> the smallest of `blocks[range]` holding it, for 1..=size.
#[allow(clippy::needless_range_loop)]
fn innermost(blocks: &[Block], range: std::ops::Range<usize>, size: usize) -> Vec<Option<usize>> {
    let mut at = vec![None; size + 2];
    let mut order: Vec<usize> = range.collect();
    order.sort_by_key(|&i| blocks[i].start as i64 - blocks[i].end as i64);
    for i in order {
        let b = &blocks[i];
        for no in b.start.max(1)..=b.end.min(size) {
            at[no] = Some(i);
        }
    }
    at
}

/// `fd`'s blocks from the new version of the file, and the old one for
/// blocks the change removes whole; None when no blocks are found in the new
/// version. Top-level code splits into blocks at the new file's empty lines. A
/// removed line belongs to the block at its place in the new file, unless it
/// is part of an old block removed whole.
pub fn file_blocks(
    fd: &FileDiff,
    new_lines: Option<&[String]>,
    old_lines: Option<&[String]>,
) -> Option<FileBlocks> {
    let new_lines = new_lines.filter(|l| !l.is_empty())?;
    let spans = code_blocks(&fd.path, new_lines)?;
    let size = new_lines.len();
    let mut inside = vec![false; size + 2];
    for &(s, e) in &spans {
        for flag in &mut inside[s..=e] {
            *flag = true;
        }
    }
    let mut runs = Vec::new();
    let mut first = None;
    for no in 1..=size + 1 {
        let plain = no <= size && !inside[no] && !new_lines[no - 1].trim().is_empty();
        if plain && first.is_none() {
            first = Some(no);
        } else if !plain && let Some(f) = first.take() {
            runs.push((f, no - 1));
        }
    }
    let all: Vec<(usize, usize)> = spans.iter().chain(runs.iter()).copied().collect();
    let mut blocks = nest(&all, Side::New, &runs, 0);
    let n_new = blocks.len();
    let new_at = innermost(&blocks, 0..n_new, size);

    let removed: HashSet<usize> = fd
        .hunks
        .iter()
        .flat_map(|h| h.lines.iter())
        .filter(|l| l.origin == '-')
        .map(|l| l.old_no)
        .collect();
    let old_spans: Vec<(usize, usize)> = old_lines
        .filter(|l| !l.is_empty())
        .and_then(|l| code_blocks(&fd.path, l))
        .unwrap_or_default()
        .into_iter()
        .filter(|&(a, b)| (a..=b).all(|no| removed.contains(&no)))
        .collect();
    blocks.extend(nest(&old_spans, Side::Old, &[], n_new));
    let n_all = blocks.len();
    let old_size = blocks[n_new..].iter().map(|b| b.end).max().unwrap_or(0);
    let old_at = innermost(&blocks, n_new..n_all, old_size);

    let new_block = |no: usize| -> Option<usize> {
        if no > 0 && no <= size {
            new_at[no]
        } else {
            None
        }
    };
    // Where something removed before new line `pos` belongs: the innermost
    // { } block it sat strictly inside, else the run of top-level code around
    // it, else nowhere.
    let removed_home = |blocks: &[Block], pos: usize| -> Option<usize> {
        let mut b = new_block(pos);
        while let Some(i) = b {
            let blk = &blocks[i];
            if blk.run || !(blk.start < pos && pos <= blk.end) {
                b = blk.parent;
            } else {
                break;
            }
        }
        if b.is_some() {
            return b;
        }
        for n in [pos, pos.wrapping_sub(1)] {
            if let Some(i) = new_block(n)
                && blocks[i].run
            {
                return Some(i);
            }
        }
        None
    };

    // A block removed whole hangs off the new block at the place it was.
    let mut first_line: HashMap<usize, usize> = HashMap::new();
    for h in &fd.hunks {
        for l in &h.lines {
            if l.origin == '-' {
                first_line.entry(l.old_no).or_insert(l.new_no);
            }
        }
    }
    for i in n_new..n_all {
        if blocks[i].parent.is_none() {
            let pos = *first_line.get(&blocks[i].start).unwrap_or(&0);
            let home = removed_home(&blocks, pos);
            blocks[i].parent = home.filter(|&h| !blocks[h].run);
        }
    }
    for i in n_new..n_all {
        let (mut d, mut p) = (0, blocks[i].parent);
        while let Some(j) = p {
            d += 1;
            p = blocks[j].parent;
        }
        blocks[i].depth = d;
    }

    let mut line_block = HashMap::new();
    for h in &fd.hunks {
        for l in &h.lines {
            let b = if l.origin == '-' && l.old_no < old_at.len() && old_at[l.old_no].is_some() {
                old_at[l.old_no]
            } else if l.origin == '-' {
                removed_home(&blocks, l.new_no)
            } else {
                new_block(l.new_no)
            };
            let Some(mut b) = b else { continue };
            line_block.insert(l.key.clone(), b);
            if l.origin == '+' || l.origin == '-' {
                loop {
                    blocks[b].changed.insert(l.key.clone());
                    match blocks[b].parent {
                        Some(p) => b = p,
                        None => break,
                    }
                }
            }
        }
    }
    Some(FileBlocks { blocks, line_block })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::parse_diff;

    fn lines(s: &str) -> Vec<String> {
        s.lines().map(String::from).collect()
    }

    #[test]
    fn finds_balanced_braces_skipping_strings_and_comments() {
        let src = lines("fn a() {\n  let s = \"{\";\n  // }\n  if x {\n  }\n}\n");
        assert_eq!(brace_blocks(&src), Some(vec![(4, 5), (1, 6)]));
        assert_eq!(brace_blocks(&lines("a {\n")), None);
        assert_eq!(brace_blocks(&lines("no braces\n")), None);
        // A lifetime is not a string.
        assert_eq!(
            brace_blocks(&lines("fn f<'a>(x: &'a u8) {\n}\n")),
            Some(vec![(1, 2)])
        );
        // # comments, and /* */ across lines.
        assert_eq!(
            brace_blocks(&lines("# {\n/* {\n } */\nx {\n}\n")),
            Some(vec![(4, 5)])
        );
    }

    #[test]
    fn grammars_find_blocks_braces_cannot() {
        let py = lines(
            "def f(x):\n    if x:\n        return 1\n    return 2\n\nclass C:\n    def m(self):\n        pass\n",
        );
        let spans = grammar_blocks("a.py", &py).unwrap();
        assert!(spans.contains(&(1, 4)), "the function: {spans:?}");
        assert!(spans.contains(&(2, 3)), "its if");
        assert!(
            spans.contains(&(6, 8)) && spans.contains(&(7, 8)),
            "the class and its method"
        );
        assert!(!spans.contains(&(2, 4)), "the def's body is the def");
        assert_eq!(brace_blocks(&py), None);
        let rs = lines("fn a() {\n    if x {\n        y();\n    }\n}\n");
        assert_eq!(code_blocks("a.rs", &rs), Some(vec![(2, 4), (1, 5)]));
        assert_eq!(
            code_blocks("a.webidl", &lines("interface X {\n};\n")),
            Some(vec![(1, 2)])
        );
        assert_eq!(code_blocks("notes.txt", &lines("one\ntwo\n")), None);
        let c = lines("#include <a.h>\n#include <b.h>\n\nint f(void) {\n  return 0;\n}\n");
        assert_eq!(
            code_blocks("a.c", &c),
            Some(vec![(4, 6)]),
            "an #include is one line"
        );
    }

    #[test]
    fn nests_blocks_and_runs_and_collects_changed_lines() {
        let new = lines("top\n\nfn a() {\n  one\n  if x {\n    two\n  }\n}\n");
        let diff = "diff --git a/f.rs b/f.rs\n--- a/f.rs\n+++ b/f.rs\n@@ -1,8 +1,8 @@\n top\n \n fn a() {\n-  uno\n+  one\n   if x {\n-    dos\n+    two\n   }\n }\n";
        let fd = &parse_diff(diff, "m")[0];
        let fb = file_blocks(fd, Some(&new), None).unwrap();
        let two = fd.hunks[0]
            .lines
            .iter()
            .find(|l| l.text == "+    two")
            .unwrap();
        let chain = fb.chain(&two.key);
        let spans: Vec<_> = chain
            .iter()
            .map(|&i| (fb.blocks[i].start, fb.blocks[i].end))
            .collect();
        assert_eq!(spans, [(5, 7), (3, 8)]);
        assert_eq!(fb.blocks[chain[0]].changed.len(), 2);
        assert_eq!(fb.blocks[chain[1]].changed.len(), 4);
        // "top" is a run of top-level code with no change in it.
        let top = fd.hunks[0].lines.iter().find(|l| l.text == " top").unwrap();
        assert!(fb.chain(&top.key).is_empty());
    }
}
