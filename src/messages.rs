//! Two versions of a commit message compared word by word, where their
//! lines wrap aside: what each has that the other has not, which the sync
//! of the commit messages colours. A message is cut into sections at its
//! empty lines, a fenced block (```, ~~~) staying whole, its empty lines
//! and all. A section is code when one of its lines is: a line of a fenced
//! block, the fences too; a line indented by a tab, or by four spaces or
//! more; a line that starts with a shell's prompt, `$ `, after any indent.
//! In prose, the end of a line is a space as any other, and a gap of white
//! space is one however long, so that a paragraph wrapped otherwise is the
//! same. In code, the end of every line counts, as the white space within
//! a line does, its indent too: only the white space that ends a line is
//! not read. Each version is read so on its own, and a section that is
//! code in one and prose in the other is not the same in both. The break
//! between two sections is a word of its own.

use std::ops::Range;

/// A line of a message, as compared with the other version: its text;
/// what of it the other has not, as ranges of its characters, from where
/// each starts to where it ends; and whether the break after it is one the
/// other has not: on a line of code, its end; on an empty line, the break
/// between two sections it is.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Marked {
    pub text: String,
    pub words: Vec<(usize, usize)>,
    pub broken: bool,
}

/// What a message is made of, compared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum Token<'a> {
    /// A word of prose: what is between two gaps of white space.
    Word(&'a str),
    /// A piece of a line of code, as it is: a word, or white space within
    /// the line, its indent too.
    Code(&'a str),
    /// The end of a line of code, but of its section's last.
    End,
    /// The break between two sections: one empty line or more.
    Break,
}

/// Where a token is: its line, and its characters there, from where it
/// starts to where it ends; the end of a line, after its last character
/// but white space; a break, on its first empty line.
type At = (usize, usize, usize);

/// The sections of a message of `lines`, each its lines and whether it is
/// code, as this module says them.
fn sections(lines: &[&str]) -> Vec<(Range<usize>, bool)> {
    // The lines of fenced blocks, their fences too: a fence is closed by
    // one of its kind, else the block goes on to the end.
    let mut fenced = vec![false; lines.len()];
    let mut open: Option<&str> = None;
    for (i, l) in lines.iter().enumerate() {
        let t = l.trim_start();
        match open {
            Some(f) => {
                fenced[i] = true;
                if t.starts_with(f) {
                    open = None;
                }
            }
            None if t.starts_with("```") || t.starts_with("~~~") => {
                fenced[i] = true;
                open = Some(&t[..3]);
            }
            None => {}
        }
    }
    let empty = |i: usize| !fenced[i] && lines[i].trim().is_empty();
    let code = |i: usize| fenced[i] || lines[i].starts_with('\t') || lines[i].starts_with("    ") || lines[i].trim_start().starts_with("$ ");
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if empty(i) {
            i += 1;
            continue;
        }
        let start = i;
        while i < lines.len() && !empty(i) {
            i += 1;
        }
        out.push((start..i, (start..i).any(code)));
    }
    out
}

/// The pieces of `text` that `split` says are words, each as its bytes and
/// its characters, from where it starts to where it ends; white space
/// itself, when `space`, a piece of its own.
fn pieces(text: &str, space: bool) -> Vec<(Range<usize>, (usize, usize))> {
    let mut out = Vec::new();
    // The piece going on: where it started, in bytes and in characters, and
    // whether it is white space.
    let mut piece: Option<(usize, usize, bool)> = None;
    let mut n = 0;
    for (b, c) in text.char_indices() {
        let blank = c.is_whitespace();
        match piece {
            Some((_, _, was)) if was == blank => {}
            Some((start, from, was)) => {
                if space || !was {
                    out.push((start..b, (from, n)));
                }
                piece = Some((b, n, blank));
            }
            None => piece = Some((b, n, blank)),
        }
        n += 1;
    }
    if let Some((start, from, was)) = piece
        && (space || !was)
    {
        out.push((start..text.len(), (from, n)));
    }
    out
}

/// Message `m` read: its lines, and its tokens, each where it is.
fn read(m: &str) -> (Vec<&str>, Vec<(Token<'_>, At)>) {
    let lines: Vec<&str> = m.lines().collect();
    let mut out = Vec::new();
    for (k, (range, code)) in sections(&lines).into_iter().enumerate() {
        // The break before it, on the first empty line after the last.
        if k > 0 {
            let first = (0..range.start).rev().take_while(|&i| lines[i].trim().is_empty()).last().unwrap_or(range.start);
            out.push((Token::Break, (first, 0, 0)));
        }
        let last = range.end - 1;
        for j in range {
            if code {
                let kept = lines[j].trim_end();
                for (bytes, (from, to)) in pieces(kept, true) {
                    out.push((Token::Code(&kept[bytes]), (j, from, to)));
                }
                if j < last {
                    let n = kept.chars().count();
                    out.push((Token::End, (j, n, n)));
                }
            } else {
                for (bytes, (from, to)) in pieces(lines[j], false) {
                    out.push((Token::Word(&lines[j][bytes]), (j, from, to)));
                }
            }
        }
    }
    (lines, out)
}

/// Messages `ours` and `theirs` compared: the lines of each, what each has
/// that the other has not marked (`Marked`).
pub fn compare(ours: &str, theirs: &str) -> (Vec<Marked>, Vec<Marked>) {
    let ((la, ta), (lb, tb)) = (read(ours), read(theirs));
    let a: Vec<Token> = ta.iter().map(|(t, _)| *t).collect();
    let b: Vec<Token> = tb.iter().map(|(t, _)| *t).collect();
    let (mut ma, mut mb) = (vec![false; a.len()], vec![false; b.len()]);
    for op in similar::capture_diff_slices(similar::Algorithm::Myers, &a, &b) {
        if op.tag() == similar::DiffTag::Equal {
            continue;
        }
        for k in op.old_range() {
            ma[k] = true;
        }
        for k in op.new_range() {
            mb[k] = true;
        }
    }
    (marked(&la, &ta, &ma), marked(&lb, &tb, &mb))
}

/// `lines` as marked: the tokens `marks` says are its own, of `tokens`. Of
/// two words of prose in a line, both its own, the gap between them too.
fn marked(lines: &[&str], tokens: &[(Token, At)], marks: &[bool]) -> Vec<Marked> {
    let mut out: Vec<Marked> = lines.iter().map(|l| Marked { text: l.to_string(), ..Default::default() }).collect();
    for (k, ((t, (line, from, to)), &own)) in tokens.iter().zip(marks).enumerate() {
        if !own {
            continue;
        }
        match t {
            Token::End | Token::Break => out[*line].broken = true,
            Token::Word(_) | Token::Code(_) => {
                out[*line].words.push((*from, *to));
                if let Token::Word(_) = t
                    && let Some((Token::Word(_), (next, start, _))) = tokens.get(k + 1)
                    && marks.get(k + 1) == Some(&true)
                    && next == line
                {
                    out[*line].words.push((*to, *start));
                }
            }
        }
    }
    for l in &mut out {
        l.words.sort_unstable();
        let mut merged: Vec<(usize, usize)> = Vec::new();
        for &(s, e) in &l.words {
            match merged.last_mut() {
                Some((_, end)) if s <= *end => *end = (*end).max(e),
                _ => merged.push((s, e)),
            }
        }
        l.words = merged;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A line marked: its place, its words marked, and whether its break is.
    type Line = (usize, Vec<(usize, usize)>, bool);

    /// What of each version is marked, line by line: only the lines that
    /// have some.
    fn marks(ours: &str, theirs: &str) -> (Vec<Line>, Vec<Line>) {
        let (a, b) = compare(ours, theirs);
        let some = |m: Vec<Marked>| {
            m.into_iter()
                .enumerate()
                .filter(|(_, l)| !l.words.is_empty() || l.broken)
                .map(|(i, l)| (i, l.words, l.broken))
                .collect::<Vec<_>>()
        };
        (some(a), some(b))
    }

    /// The words marked in a line, as they read.
    fn said(l: &Marked) -> Vec<String> {
        let chars: Vec<char> = l.text.chars().collect();
        l.words.iter().map(|&(s, e)| chars[s..e].iter().collect()).collect()
    }

    #[test]
    fn a_paragraph_wrapped_otherwise_is_the_same_in_both() {
        let ours = "Bug 1 - two\n\nThe parser reads the whole file\nbefore it checks the header.";
        let theirs = "Bug 1 - two\n\nThe parser reads the whole\nfile before it checks\nthe   header.";
        assert_eq!(marks(ours, theirs), (vec![], vec![]));
        // Each keeps its own lines.
        let (a, b) = compare(ours, theirs);
        assert_eq!((a.len(), b.len()), (4, 5));
        assert_eq!(b[3].text, "file before it checks");
    }

    #[test]
    fn a_word_changed_is_marked_in_each_and_the_gap_between_two_words_marked_too() {
        let (a, b) = compare("Bug 1 - the cat sat down", "Bug 1 - the dog sat down");
        assert_eq!((said(&a[0]), said(&b[0])), (vec!["cat".to_string()], vec!["dog".to_string()]));
        // Two words in a row: one stretch, the gap between them with them.
        let (a, b) = compare("Bug 1 - the big cat sat", "Bug 1 - the sat");
        assert_eq!((said(&a[0]), said(&b[0])), (vec!["big cat".to_string()], vec![]));
        // Words of two lines: each line's own.
        let (a, _) = compare("Bug 1\n\nbig\ncat sat", "Bug 1\n\nsat");
        assert_eq!((said(&a[2]), said(&a[3])), (vec!["big".to_string()], vec!["cat".to_string()]));
    }

    #[test]
    fn a_paragraph_split_or_joined_marks_the_break_alone() {
        let split = "Bug 1 - two\n\nFirst this.\n\nThen that.";
        let joined = "Bug 1 - two\n\nFirst this. Then that.";
        // The empty line the split has, marked; every word the same.
        assert_eq!(marks(split, joined), (vec![(3, vec![], true)], vec![]));
        assert_eq!(marks(joined, split), (vec![], vec![(3, vec![], true)]));
        // More empty lines than one between them: one break all the same.
        assert_eq!(marks(split, "Bug 1 - two\n\n\n\nFirst this.\n\nThen that."), (vec![], vec![]));
    }

    #[test]
    fn white_space_that_ends_a_line_is_not_read_in_prose_nor_in_code() {
        assert_eq!(marks("Bug 1 - two  \n\nBecause.\t\n", "Bug 1 - two\n\nBecause."), (vec![], vec![]));
        let code = "Bug 1\n\n    let x = 1;   \n    let y = 2;";
        assert_eq!(marks(code, "Bug 1\n\n    let x = 1;\n    let y = 2;"), (vec![], vec![]));
    }

    #[test]
    fn the_trailer_naming_the_revision_is_a_line_as_any_other() {
        let with = |d: &str| format!("Bug 1 - two\n\nBecause.\n\nDifferential Revision: https://phabricator.services.mozilla.com/{d}");
        // Another revision: its link alone.
        let (a, b) = compare(&with("D1"), &with("D2"));
        let link = |d: &str| vec![format!("https://phabricator.services.mozilla.com/{d}")];
        assert_eq!((said(&a[4]), said(&b[4])), (link("D1"), link("D2")));
        // Only one names it: its line, and the break before it.
        let (a, b) = compare("Bug 1 - two\n\nBecause.", &with("D1"));
        assert!(a.iter().all(|l| l.words.is_empty() && !l.broken), "{a:?}");
        assert!(b[3].broken && b[3].words.is_empty(), "{b:?}");
        assert_eq!(said(&b[4]), [b[4].text.clone()]);
    }

    #[test]
    fn a_command_wrapped_otherwise_is_not_the_same() {
        let one = "Bug 1 - build\n\nBuild it so:\n\n$ ./mach build --foo";
        let two = "Bug 1 - build\n\nBuild it so:\n\n$ ./mach build\n  --foo";
        let (a, b) = compare(one, two);
        // The space before --foo, in the one; the end of the line and the
        // indent, in the other: neither all the same.
        assert_eq!((a[4].words.clone(), a[4].broken), (vec![(14, 15)], false));
        assert_eq!((b[4].words.clone(), b[4].broken), (vec![], true));
        assert_eq!(b[5].words, [(0, 2)]);
        // The prose over it, wrapped otherwise, is still the same.
        let (a, b) = compare(&one.replace("Build it so:", "Build\nit so:"), one);
        assert!(a[2].words.is_empty() && a[3].words.is_empty() && b[2].words.is_empty(), "{a:?}\n{b:?}");
    }

    #[test]
    fn a_fenced_block_is_one_section_of_code_its_empty_lines_and_all() {
        let m = "Bug 1 - two\n\nRun:\n\n```\nfn a() {}\n\nfn b() {}\n```\n\nThen.";
        let lines: Vec<&str> = m.lines().collect();
        assert_eq!(sections(&lines), [(0..1, false), (2..3, false), (4..9, true), (10..11, false)]);
        // Unclosed, it goes on to the end; ~~~ fences as ``` do.
        let lines: Vec<&str> = "Bug 1\n\n~~~\na\n\n```\nb".lines().collect();
        assert_eq!(sections(&lines), [(0..1, false), (2..7, true)]);
        // The empty line inside it is a line of code: taken out, the block
        // is not the same.
        let (a, _) = compare(m, &m.replace("fn a() {}\n\nfn b()", "fn a() {}\nfn b()"));
        assert!(a.iter().any(|l| l.broken) && a.iter().all(|l| l.words.is_empty()), "{a:?}");
    }

    #[test]
    fn a_line_of_code_makes_its_whole_paragraph_code() {
        let m = "Bug 1 - two\n\nTo see it, run\n    $ make check\nand read what it says.";
        let lines: Vec<&str> = m.lines().collect();
        assert_eq!(sections(&lines), [(0..1, false), (2..5, true)]);
        // Wrapped otherwise, the paragraph is not the same, though prose
        // wrapped so would be.
        let other = "Bug 1 - two\n\nTo see it,\nrun\n    $ make check\nand read what it says.";
        let (a, b) = compare(m, other);
        assert!(a[2].words == [(10, 11)] && b[2].broken && b[3].words.is_empty(), "{a:?}\n{b:?}");
        // A tab, or a prompt alone, makes it code too.
        for line in ["\tmake check", "$ make check", "  $ make check"] {
            let lines = ["Run", line, "now"];
            assert_eq!(sections(&lines), [(0..3, true)], "{line:?}");
        }
        assert_eq!(sections(&["Run", "  make check", "now"]), [(0..3, false)]);
    }

    #[test]
    fn white_space_within_a_line_of_code_counts() {
        let (a, b) = compare("Bug 1\n\n    let x = 1;", "Bug 1\n\n    let x  = 1;");
        assert_eq!((a[2].words.clone(), b[2].words.clone()), (vec![(9, 10)], vec![(9, 11)]));
        // Its indent too.
        let (a, b) = compare("Bug 1\n\n    let x = 1;", "Bug 1\n\n      let x = 1;");
        assert_eq!((a[2].words.clone(), b[2].words.clone()), (vec![(0, 4)], vec![(0, 6)]));
    }

    #[test]
    fn a_section_of_code_in_one_and_prose_in_the_other_is_not_the_same() {
        let (a, b) = compare("Bug 1\n\nmake check", "Bug 1\n\n    make check");
        assert_eq!((said(&a[2]), said(&b[2])), (vec!["make check".to_string()], vec!["    make check".to_string()]));
    }
}
