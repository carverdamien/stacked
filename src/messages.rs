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
//!
//! Compared so, two versions are also a diff of their lines (`diff`): a
//! section of one whose words are all of one section of the other, and
//! all its words, is the same in both, however each wraps it; so are the
//! empty lines between two sections, when the break they are is in both;
//! the rest differs, each version with its own lines.

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

/// A section of a message as read: its lines, its tokens, and the break
/// before it, a token too, which every section but the first has.
struct Section {
    lines: Range<usize>,
    tokens: Range<usize>,
    brk: Option<usize>,
}

/// A message as read: its lines, its tokens, each where it is, and its
/// sections.
struct Read<'a> {
    lines: Vec<&'a str>,
    tokens: Vec<(Token<'a>, At)>,
    sections: Vec<Section>,
}

/// Message `m` read.
fn read(m: &str) -> Read<'_> {
    let lines: Vec<&str> = m.lines().collect();
    let mut out = Vec::new();
    let mut secs = Vec::new();
    for (k, (range, code)) in sections(&lines).into_iter().enumerate() {
        // The break before it, on the first empty line after the last.
        let brk = (k > 0).then(|| {
            let first = (0..range.start).rev().take_while(|&i| lines[i].trim().is_empty()).last().unwrap_or(range.start);
            out.push((Token::Break, (first, 0, 0)));
            out.len() - 1
        });
        let start = out.len();
        let last = range.end - 1;
        for j in range.clone() {
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
        secs.push(Section {
            lines: range,
            tokens: start..out.len(),
            brk,
        });
    }
    Read {
        lines,
        tokens: out,
        sections: secs,
    }
}

/// Messages `ours` and `theirs` compared: the lines of each, what each has
/// that the other has not marked (`Marked`).
pub fn compare(ours: &str, theirs: &str) -> (Vec<Marked>, Vec<Marked>) {
    let d = diff(ours, theirs);
    (d.ours, d.theirs)
}

/// Two versions of a message compared (`compare`), and as a diff of their
/// lines: the lines of each, marked; and all of them in runs, in order,
/// each the same in both as compared, or not.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Diff {
    pub ours: Vec<Marked>,
    pub theirs: Vec<Marked>,
    pub runs: Vec<Run>,
}

/// A run of lines of two versions of a message, from where it starts to
/// where it ends in each: a section the same in both, or the empty lines
/// between two, the same break (`same`); or what is between two such, the
/// lines that differ, none in one of them where the other has lines alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    pub ours: Range<usize>,
    pub theirs: Range<usize>,
    pub same: bool,
}

/// What a message's lines are, in order, as a diff of two versions takes
/// them: a section, its lines and its tokens; or empty lines, before the
/// first section, between two (and the break they are), or after the last.
enum Segment {
    Section(Range<usize>, Range<usize>),
    Gap(Range<usize>, Option<usize>),
}

impl Segment {
    fn lines(&self) -> Range<usize> {
        match self {
            Segment::Section(l, _) | Segment::Gap(l, _) => l.clone(),
        }
    }
}

/// The segments of message `r`, which hold each of its lines once.
fn segments(r: &Read) -> Vec<Segment> {
    let mut out = Vec::new();
    let mut at = 0;
    for s in &r.sections {
        if s.lines.start > at {
            out.push(Segment::Gap(at..s.lines.start, s.brk));
        }
        out.push(Segment::Section(s.lines.clone(), s.tokens.clone()));
        at = s.lines.end;
    }
    if at < r.lines.len() {
        out.push(Segment::Gap(at..r.lines.len(), None));
    }
    out
}

/// Messages `ours` and `theirs` compared, word by word as `compare` does,
/// and as a diff of their lines (`Diff`).
pub fn diff(ours: &str, theirs: &str) -> Diff {
    let (a, b) = (read(ours), read(theirs));
    let ta: Vec<Token> = a.tokens.iter().map(|(t, _)| *t).collect();
    let tb: Vec<Token> = b.tokens.iter().map(|(t, _)| *t).collect();
    let (mut ma, mut mb) = (vec![false; ta.len()], vec![false; tb.len()]);
    // Each token of ours that theirs has too, as the one of theirs it is.
    let mut to = vec![None; ta.len()];
    for op in similar::capture_diff_slices(similar::Algorithm::Myers, &ta, &tb) {
        if op.tag() == similar::DiffTag::Equal {
            for (i, j) in op.old_range().zip(op.new_range()) {
                to[i] = Some(j);
            }
            continue;
        }
        for k in op.old_range() {
            ma[k] = true;
        }
        for k in op.new_range() {
            mb[k] = true;
        }
    }
    Diff {
        runs: runs(&a, &b, &to),
        ours: marked(&a.lines, &a.tokens, &ma),
        theirs: marked(&b.lines, &b.tokens, &mb),
    }
}

/// The runs of lines of `a` and `b`, given what of `a`'s tokens `b` has
/// (`to`): a section of `a` is the same as the section of `b` that has its
/// tokens, all of them and no other; empty lines between two sections are
/// the same as those of `b` that are the same break; the empty lines
/// before the first section, or after the last, the same as `b`'s there,
/// if it has some. Between two such, what each has is a run that differs.
fn runs(a: &Read, b: &Read, to: &[Option<usize>]) -> Vec<Run> {
    let (sa, sb) = (segments(a), segments(b));
    // The segment of `b` each of its tokens is in.
    let mut of = vec![0; b.tokens.len()];
    for (k, s) in sb.iter().enumerate() {
        match s {
            Segment::Section(_, t) => t.clone().for_each(|i| of[i] = k),
            Segment::Gap(_, Some(i)) => of[*i] = k,
            Segment::Gap(_, None) => {}
        }
    }
    let mut pairs: Vec<(usize, usize)> = Vec::new();
    for (k, s) in sa.iter().enumerate() {
        let other = match s {
            Segment::Section(_, t) => t.clone().next().and_then(|x| to[x]).map(|y| of[y]).filter(|&j| match &sb[j] {
                Segment::Section(_, u) => u.len() == t.len() && t.clone().zip(u.clone()).all(|(x, y)| to[x] == Some(y)),
                Segment::Gap(..) => false,
            }),
            Segment::Gap(_, Some(x)) => to[*x].map(|y| of[y]),
            Segment::Gap(l, None) if l.start == 0 => matches!(sb.first(), Some(Segment::Gap(m, None)) if m.start == 0).then_some(0),
            Segment::Gap(_, None) => matches!(sb.last(), Some(Segment::Gap(_, None))).then(|| sb.len() - 1),
        };
        // In order in both, as the tokens are.
        if let Some(j) = other
            && pairs.last().is_none_or(|&(_, was)| j > was)
        {
            pairs.push((k, j));
        }
    }
    let at = |s: &[Segment], k: usize, n: usize| s.get(k).map_or(n, |x| x.lines().start);
    let (na, nb) = (a.lines.len(), b.lines.len());
    let mut out = Vec::new();
    let (mut i, mut j) = (0, 0);
    for (p, q) in pairs.into_iter().chain([(sa.len(), sb.len())]) {
        let (ours, theirs) = (at(&sa, i, na)..at(&sa, p, na), at(&sb, j, nb)..at(&sb, q, nb));
        if !ours.is_empty() || !theirs.is_empty() {
            out.push(Run { ours, theirs, same: false });
        }
        if let (Some(x), Some(y)) = (sa.get(p), sb.get(q)) {
            out.push(Run {
                ours: x.lines(),
                theirs: y.lines(),
                same: true,
            });
        }
        (i, j) = (p + 1, q + 1);
    }
    out
}

/// The sections of message `m`, each its lines and what it says as
/// compared: two sections say the same when their words are the same, in
/// the same order, however each wraps them (`compare`), and are code in
/// both or prose in both.
pub fn sections_said(m: &str) -> Vec<(Range<usize>, String)> {
    let r = read(m);
    r.sections
        .iter()
        .map(|s| {
            let said: Vec<String> = r.tokens[s.tokens.clone()]
                .iter()
                .map(|(t, _)| match t {
                    Token::Word(w) => format!("w{w}"),
                    Token::Code(c) => format!("c{c}"),
                    Token::End => "e".into(),
                    Token::Break => "b".into(),
                })
                .collect();
            (s.lines.clone(), said.join("\0"))
        })
        .collect()
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

    /// The runs of a diff, each as its lines in ours and in theirs, and
    /// whether it is the same in both.
    fn runs_of(ours: &str, theirs: &str) -> Vec<(Range<usize>, Range<usize>, bool)> {
        diff(ours, theirs).runs.into_iter().map(|r| (r.ours, r.theirs, r.same)).collect()
    }

    #[test]
    fn a_paragraph_wrapped_otherwise_is_a_run_the_same_in_both_each_with_its_lines() {
        let ours = "Bug 1 - two\n\nThe parser reads the whole file\nbefore it checks the header.\n\nThat is all.";
        let theirs = "Bug 1 - two\n\nThe parser reads the whole\nfile before it checks\nthe   header.  \n\nThat is all.";
        // The title, the empty line under it, the paragraph, the empty line
        // after it, the last: each the same.
        assert_eq!(runs_of(ours, theirs), [(0..1, 0..1, true), (1..2, 1..2, true), (2..4, 2..5, true), (4..5, 5..6, true), (5..6, 6..7, true)]);
        // The same message: the same runs, line for line.
        assert!(runs_of(ours, ours).iter().all(|(a, b, same)| a == b && *same));
        // More empty lines between two paragraphs: the same break.
        assert!(runs_of(ours, &ours.replace("header.\n\n", "header.\n\n\n")).iter().all(|r| r.2));
    }

    #[test]
    fn a_paragraph_with_a_word_changed_is_a_run_that_differs_its_word_marked() {
        let ours = "Bug 1 - two\n\nThe parser reads the whole file\nbefore it checks the header.\n\nThat is all.";
        let theirs = "Bug 1 - two\n\nThe parser reads the entire\nfile before it checks\nthe header.\n\nThat is all.";
        let d = diff(ours, theirs);
        let runs: Vec<_> = d.runs.iter().map(|r| (r.ours.clone(), r.theirs.clone(), r.same)).collect();
        assert_eq!(runs, [(0..1, 0..1, true), (1..2, 1..2, true), (2..4, 2..5, false), (4..5, 5..6, true), (5..6, 6..7, true)]);
        // Only the word changed is marked: the others are in both.
        let marked = |m: &[Marked]| m.iter().flat_map(said).collect::<Vec<_>>();
        assert_eq!((marked(&d.ours), marked(&d.theirs)), (vec!["whole".to_string()], vec!["entire".to_string()]));
    }

    #[test]
    fn a_paragraph_split_a_section_put_in_or_a_command_wrapped_otherwise_differs() {
        // Split: the two lines and the break between them, against the one.
        let split = "Bug 1 - two\n\nFirst this.\n\nThen that.";
        let joined = "Bug 1 - two\n\nFirst this. Then that.";
        assert_eq!(runs_of(split, joined), [(0..1, 0..1, true), (1..2, 1..2, true), (2..5, 2..3, false)]);
        // A paragraph put in at the end: its line, and the break before it.
        let trailed = format!("{joined}\n\nDifferential Revision: https://phabricator.services.mozilla.com/D1");
        assert_eq!(runs_of(joined, &trailed), [(0..1, 0..1, true), (1..2, 1..2, true), (2..3, 2..3, true), (3..3, 3..5, false)]);
        assert_eq!(runs_of(&trailed, joined), [(0..1, 0..1, true), (1..2, 1..2, true), (2..3, 2..3, true), (3..5, 3..3, false)]);
        // A command wrapped otherwise: code, not the same.
        let one = "Bug 1 - build\n\n$ ./mach build --foo";
        let two = "Bug 1 - build\n\n$ ./mach build\n  --foo";
        assert_eq!(runs_of(one, two), [(0..1, 0..1, true), (1..2, 1..2, true), (2..3, 2..4, false)]);
    }

    #[test]
    fn empty_lines_before_every_section_or_after_are_the_same_as_the_other_s_there() {
        assert_eq!(runs_of("\nBug 1\n\n", "\n\nBug 1\n\n\n"), [(0..1, 0..2, true), (1..2, 2..3, true), (2..3, 3..5, true)]);
        // Only one has some: they differ.
        assert_eq!(runs_of("Bug 1\n\n", "Bug 1"), [(0..1, 0..1, true), (1..2, 1..1, false)]);
        // Nothing in either.
        assert_eq!(runs_of("", ""), []);
        assert_eq!(runs_of("", "Bug 1"), [(0..0, 0..1, false)]);
    }

    #[test]
    fn what_a_section_says_is_its_words_however_it_wraps_them() {
        let said = |m: &str| sections_said(m).into_iter().map(|(_, s)| s).collect::<Vec<_>>();
        let wrapped = said("Bug 1 - two\n\nThe parser reads the whole file\nbefore it checks the header.");
        assert_eq!(wrapped, said("Bug 1 - two\n\nThe parser reads the whole\nfile before   it checks\nthe header.  "));
        assert_ne!(wrapped, said("Bug 1 - two\n\nThe parser reads the entire file\nbefore it checks the header."));
        // Its lines, from 0.
        let lines: Vec<Range<usize>> = sections_said("Bug 1\n\nA\nB\n\n\nC").into_iter().map(|(l, _)| l).collect();
        assert_eq!(lines, [0..1, 2..4, 6..7]);
        // Code, line by line; prose and code never the same.
        assert_ne!(said("Bug 1\n\n$ ./mach build --foo"), said("Bug 1\n\n$ ./mach build\n  --foo"));
        assert_ne!(said("Bug 1\n\nmake check"), said("Bug 1\n\n    make check"));
        assert_eq!(said("Bug 1\n\n    let x = 1;   "), said("Bug 1\n\n    let x = 1;"));
    }
}
