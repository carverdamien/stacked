//! A commit message's long lines wrapped at a width, as the menus offer to
//! wrap them: each paragraph of prose of its body that has a line longer
//! than the width is filled again, greedily, as many words on each line as
//! the width lets, its words in their order, a gap of white space one
//! space. The message is cut into sections as `messages` cuts it
//! (`messages::sections`): at its empty lines, a fenced block whole, a
//! section with a line of code being code. Never wrapped: the title, the
//! first line, however long; a section of code; a quoted line (`> `); the
//! trailers, the last section from its first `Key: value` line on
//! (`head::trailer`); and a paragraph whose lines all fit, none of them
//! joined to another. A list item (`- `, `* `, `+ `, `1. `, `1) `) is a
//! paragraph of its own: filled, it keeps its marker, and its next lines
//! are indented under its text, by three spaces at most, as four would make
//! them code. A word longer than the width stays whole, on a line of its
//! own. A word that would start a line as something else than prose (a
//! list's marker, a quote, a fence, a shell's prompt, or `#`, which git
//! reads as a comment) goes on the line before, with the word before it; a
//! line of such a word alone stays as it is. So the message wrapped is the
//! one it came from as `messages::compare` reads them, the same words in
//! the same order, in the same sections: only where its lines break
//! changes.

use std::ops::Range;

/// A message wrapped: its new text, and how many of its lines of prose, of
/// the paragraphs that may be filled, were longer than the width.
#[derive(Debug, Clone, PartialEq)]
pub struct Wrapped {
    pub message: String,
    pub long: usize,
}

/// A paragraph of prose that may be filled: its lines; where its words
/// start on the first, past a list item's marker; what its first line
/// starts with once filled, its indent and its marker if it has one; and
/// what each of its next lines starts with.
struct Para {
    lines: Range<usize>,
    from: usize,
    first: String,
    next: String,
}

/// Message `m` with the long lines of its prose wrapped at `width` columns,
/// as this module says; none when that changes nothing.
pub fn wrapped(m: &str, width: usize) -> Option<Wrapped> {
    let w = refill(m, width)?;
    // The two the same as `compare` reads them, else nothing is wrapped:
    // rather that than a word lost, or a paragraph read as code.
    let (a, b) = crate::messages::compare(m, &w.message);
    if a.iter().chain(&b).any(|l| !l.words.is_empty() || l.broken) {
        return None;
    }
    Some(w)
}

/// As `wrapped`, but unchecked: what filling the paragraphs of `m` makes.
fn refill(m: &str, width: usize) -> Option<Wrapped> {
    let lines: Vec<&str> = m.split('\n').collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let (mut long, mut changed) = (0, false);
    let mut at = 0;
    for p in paragraphs(&lines) {
        out.extend(lines[at..p.lines.start].iter().map(|l| l.to_string()));
        let own = &lines[p.lines.clone()];
        let over = own.iter().filter(|l| cols(l.trim_end()) > width).count();
        long += over;
        match (over > 0).then(|| fill(&lines, &p, width)).filter(|f| f.iter().map(String::as_str).ne(own.iter().copied())) {
            Some(filled) => {
                changed = true;
                out.extend(filled);
            }
            None => out.extend(own.iter().map(|l| l.to_string())),
        }
        at = p.lines.end;
    }
    out.extend(lines[at..].iter().map(|l| l.to_string()));
    changed.then(|| Wrapped { message: out.join("\n"), long })
}

/// What a menu says of the messages `long` names, to wrap at `width`
/// columns: each patch's place, from 0, and how many of its lines of prose
/// are longer than the width. `Patch 2's message has 3 lines of prose
/// longer than 72 columns, patch 4's has 1 and patch 5's has 2.`
pub fn said(long: &[(usize, usize)], width: usize) -> String {
    let lines = |n: usize| if n == 1 { "1 line".to_string() } else { format!("{n} lines") };
    let each: Vec<String> = long
        .iter()
        .enumerate()
        .map(|(i, &(k, n))| match i {
            0 => format!("Patch {}'s message has {} of prose longer than {width} columns", k + 1, lines(n)),
            _ => format!("patch {}'s has {n}", k + 1),
        })
        .collect();
    match each.as_slice() {
        [] => String::new(),
        [one] => format!("{one}."),
        [rest @ .., last] => format!("{} and {last}.", rest.join(", ")),
    }
}

/// The paragraphs of prose of message `lines` that may be filled, in their
/// order: of each section of prose, its lines but the title, the lines
/// kept as they are (`kept`) and, in the last section, the trailers; each
/// list item one, with the lines after it indented more than it is.
fn paragraphs(lines: &[&str]) -> Vec<Para> {
    let sections = crate::messages::sections(lines);
    let last = sections.len().saturating_sub(1);
    let mut out = Vec::new();
    for (k, (range, code)) in sections.into_iter().enumerate() {
        if code {
            continue;
        }
        // The title, the first section's first line.
        let start = if k == 0 { range.start + 1 } else { range.start };
        let end = match k == last {
            true => (start..range.end).find(|&i| crate::head::trailer(lines[i])).unwrap_or(range.end),
            false => range.end,
        };
        let goes_on = |l: &str| !kept(l) && item(l).is_none();
        let mut i = start;
        while i < end {
            let line = lines[i];
            let indent = indent_of(line);
            let mut j = i + 1;
            if kept(line) {
                i = j;
                continue;
            }
            match item(line) {
                Some(marker) => {
                    while j < end && goes_on(lines[j]) && indent_of(lines[j]).len() > indent.len() {
                        j += 1;
                    }
                    let first = format!("{indent}{marker} ");
                    let under = cols(&first).min(3);
                    // Its next lines indented more than it, as they must be
                    // to be its own, and less than four spaces.
                    if spaces(indent) && under > indent.len() {
                        let from = indent.len() + marker.len();
                        out.push(Para { lines: i..j, from, first, next: " ".repeat(under) });
                    }
                }
                None => {
                    while j < end && goes_on(lines[j]) {
                        j += 1;
                    }
                    if spaces(indent) {
                        out.push(Para { lines: i..j, from: 0, first: indent.into(), next: indent.into() });
                    }
                }
            }
            i = j;
        }
    }
    out
}

/// Paragraph `p` of message `lines` filled to `width` columns: its words
/// (`words`), each line as full as the width lets it, the first starting
/// as `p.first` says and the next ones as `p.next` does; each line ending
/// as its last did, with a carriage return or without.
fn fill(lines: &[&str], p: &Para, width: usize) -> Vec<String> {
    let own = &lines[p.lines.clone()];
    let text = std::iter::once(&own[0][p.from..]).chain(own[1..].iter().copied());
    let end = if own[own.len() - 1].ends_with('\r') { "\r" } else { "" };
    let mut out = Vec::new();
    let (mut line, mut w, mut bare) = (p.first.clone(), cols(&p.first), true);
    for word in words(text) {
        let n = cols(&word);
        if !bare && w + 1 + n > width {
            out.push(std::mem::replace(&mut line, p.next.clone()) + end);
            (w, bare) = (cols(&p.next), true);
        }
        if !bare {
            line.push(' ');
            w += 1;
        }
        line.push_str(&word);
        w += n;
        bare = false;
    }
    out.push(line + end);
    out
}

/// The words of `text`, each that would start a line as something else
/// than prose (`misread`) joined to the one before it: what a line holds
/// whole once filled.
fn words<'a>(text: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for w in text.flat_map(str::split_whitespace) {
        match out.last_mut() {
            Some(last) if misread(w) => {
                last.push(' ');
                last.push_str(w);
            }
            _ => out.push(w.to_string()),
        }
    }
    out
}

/// Whether `word`, starting a line, would make it something else than a
/// line of prose: a list item, a quote, a fence, a command after a shell's
/// prompt, or a line git reads as a comment.
fn misread(word: &str) -> bool {
    marker(word) || word == "$" || word.starts_with(['>', '#']) || word.starts_with("```") || word.starts_with("~~~")
}

/// Whether `word` is a list item's marker: `-`, `*`, `+`, or a number of one
/// to nine digits and `.` or `)`.
fn marker(word: &str) -> bool {
    matches!(word, "-" | "*" | "+") || word.strip_suffix(['.', ')']).is_some_and(|n| (1..=9).contains(&n.len()) && n.bytes().all(|b| b.is_ascii_digit()))
}

/// The marker of `line` if it is a list item: after its indent, a marker
/// (`marker`), then white space, then its text.
fn item(line: &str) -> Option<&str> {
    let rest = line.trim_start();
    let (word, after) = rest.split_at(rest.find(char::is_whitespace)?);
    (marker(word) && !after.trim().is_empty()).then_some(word)
}

/// Whether `line` of a paragraph of prose is kept as it is: quoted, `>`
/// after its indent; or one word alone that would make a line starting
/// with it something else than prose (`misread`), as it would once filled,
/// more words after it there.
fn kept(line: &str) -> bool {
    let mut words = line.split_whitespace();
    line.trim_start().starts_with('>') || (words.next().is_some_and(misread) && words.next().is_none())
}

/// The white space `line` starts with.
fn indent_of(line: &str) -> &str {
    &line[..line.len() - line.trim_start().len()]
}

/// Whether `indent` is spaces alone, as the lines of a paragraph filled are
/// indented: of one indented otherwise, a tab among its spaces, nothing is
/// filled.
fn spaces(indent: &str) -> bool {
    indent.bytes().all(|b| b == b' ')
}

/// The columns `text` takes on a screen: two for a wide character.
fn cols(text: &str) -> usize {
    ratatui::text::Span::raw(text).width()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Message `lines`, a line each.
    fn message(lines: &[&str]) -> String {
        lines.join("\n")
    }

    /// Whether `a` and `b` say the same as `messages::compare` reads them:
    /// nothing of either marked.
    fn same(a: &str, b: &str) -> bool {
        let (a, b) = crate::messages::compare(a, b);
        a.iter().chain(&b).all(|l| l.words.is_empty() && !l.broken)
    }

    /// `m` wrapped at `width`, seen to say what `m` says and to have nothing
    /// more to wrap: its text, and its count of long lines.
    fn checked(m: &str, width: usize) -> (String, usize) {
        let w = wrapped(m, width).unwrap_or_else(|| panic!("nothing to wrap at {width} in {m:?}"));
        assert!(same(m, &w.message), "not the same at {width}:\n{m}\n--\n{}", w.message);
        assert_eq!(wrapped(&w.message, width), None, "more to wrap at {width}:\n{}", w.message);
        (w.message, w.long)
    }

    #[test]
    fn a_long_paragraph_of_prose_is_filled_to_the_width() {
        let one = message(&[
            "Bug 1 - fix the parser",
            "",
            "The parser read the whole file before it checked the header, which made it slow on big files.",
        ]);
        let (text, long) = checked(&one, 40);
        let want = ["Bug 1 - fix the parser", "", "The parser read the whole file before it", "checked the header, which made it slow", "on big files."];
        assert_eq!(text, message(&want));
        assert_eq!(long, 1);
        // Lines broken by hand, too long: filled again, a gap of white space
        // one space, the white space ending a line gone.
        let two = message(&["Bug 1 - fix the parser", "", "The parser read the whole file before it checked", "the   header,  which made it slow on   big files.   "]);
        assert_eq!(checked(&two, 40), (message(&want), 2));
        // Wider, more words on each line.
        let (text, _) = checked(&one, 72);
        assert_eq!(text.lines().nth(2), Some("The parser read the whole file before it checked the header, which made"));
        assert_eq!(text.lines().nth(3), Some("it slow on big files."));
    }

    #[test]
    fn a_paragraph_within_the_width_is_left_as_it_is_its_short_lines_unjoined() {
        let fine = message(&["Bug 1 - two", "", "Short lines,", "not joined,   even with  gaps.", "", "Another paragraph, as short."]);
        assert_eq!(wrapped(&fine, 40), None);
        // Beside a paragraph that is wrapped, it is not touched either.
        let m = format!("{fine}\n\n{}", ["word"; 12].join(" "));
        let (text, long) = checked(&m, 40);
        assert_eq!(text, format!("{fine}\n\n{}\n{}", ["word"; 8].join(" "), ["word"; 4].join(" ")));
        assert_eq!(long, 1);
        // A line exactly as wide as the width is not longer.
        assert_eq!(wrapped(&format!("Bug 1\n\n{}", "x".repeat(40)), 40), None);
    }

    #[test]
    fn a_list_item_keeps_its_marker_and_its_next_lines_go_under_its_text() {
        let m = message(&[
            "Bug 1 - list",
            "",
            "The parser now:",
            "- reads the header first, then the rest of the file",
            "- stops",
            "  early",
            "1. counts the lines of the file before it reads any of them",
            "2) and goes on with another line that is long enough",
            "* stars",
            "+ and pluses too, an item as any other one is",
        ]);
        let (text, long) = checked(&m, 40);
        let want = [
            "Bug 1 - list",
            "",
            "The parser now:",
            "- reads the header first, then the rest",
            "  of the file",
            "- stops",
            "  early",
            "1. counts the lines of the file before",
            "   it reads any of them",
            "2) and goes on with another line that is",
            "   long enough",
            "* stars",
            "+ and pluses too, an item as any other",
            "  one is",
        ];
        assert_eq!(text, message(&want));
        assert_eq!(long, 4);
        // An item's lines are its own: those of the next item, or of the
        // prose after it, are not joined to it.
        let m = message(&["Bug 1", "", "- an item that is long enough to be wrapped here", "- short", "Prose, not under the item."]);
        let (text, _) = checked(&m, 40);
        assert_eq!(text, message(&["Bug 1", "", "- an item that is long enough to be", "  wrapped here", "- short", "Prose, not under the item."]));
    }

    #[test]
    fn the_next_lines_of_an_item_are_indented_by_three_spaces_at_most_as_four_would_be_code() {
        let m = message(&["Bug 1 - nested", "", "- outer", "  - an inner item that goes on past forty columns", "10. a tenth item that goes on past forty columns"]);
        let (text, _) = checked(&m, 40);
        let want = [
            "Bug 1 - nested",
            "",
            "- outer",
            "  - an inner item that goes on past",
            "   forty columns",
            "10. a tenth item that goes on past forty",
            "   columns",
        ];
        assert_eq!(text, message(&want));
        // An item indented by three spaces has no room under it: it is left
        // as it is.
        assert_eq!(wrapped("Bug 1\n\n   - an item indented by three spaces that goes on and on", 40), None);
    }

    #[test]
    fn a_word_longer_than_the_width_stays_whole_on_a_line_of_its_own() {
        let link = "https://bugzilla.mozilla.org/show_bug.cgi?id=1234567";
        let m = format!("Bug 1 - link\n\nSee {link} for more.");
        let (text, long) = checked(&m, 40);
        assert_eq!(text, format!("Bug 1 - link\n\nSee\n{link}\nfor more."));
        assert_eq!(long, 1);
        // Alone on its line already, it leaves nothing to wrap.
        assert_eq!(wrapped(&format!("Bug 1 - link\n\n{link}"), 40), None);
        assert_eq!(wrapped(&format!("Bug 1 - link\n\n- {link}"), 40), None);
    }

    #[test]
    fn code_is_left_as_it_is_a_fenced_block_an_indented_command_and_a_prompt() {
        let long = "a line far longer than forty columns, as code may be";
        for m in [
            // Indented by four spaces, or by a tab: its paragraph is code.
            message(&["Bug 1", "", "Run it so:", &format!("    ./mach build {long}")]),
            message(&["Bug 1", "", &format!("\t{long}")]),
            message(&["Bug 1", "", &format!("Before the command, {long},"), "    ./mach build", "and after it."]),
            // A fenced block, an empty line in it, and the prose touching it.
            message(&["Bug 1", "", &format!("Its prose: {long}"), "```", long, "", long, "```"]),
            message(&["Bug 1", "", "~~~", long, "", long]),
            // A shell's prompt, after an indent too.
            message(&["Bug 1", "", &format!("To see it fail, {long}:"), "$ ./mach test"]),
            message(&["Bug 1", "", &format!("  $ ./mach test {long}")]),
        ] {
            assert_eq!(wrapped(&m, 40), None, "{m}");
        }
        // The prose after a fenced block is wrapped, the block as it was.
        let m = message(&["Bug 1", "", "```", long, "", "```", "", long]);
        let (text, _) = checked(&m, 40);
        assert_eq!(text, message(&["Bug 1", "", "```", long, "", "```", "", "a line far longer than forty columns, as", "code may be"]));
    }

    #[test]
    fn the_title_and_the_trailers_are_left_as_they_are() {
        let title = "Bug 1 - a title much longer than forty columns, as titles are";
        let revision = "Differential Revision: https://phabricator.services.mozilla.com/D123456";
        let m = message(&[title, "", "Reviewed-by: someone with a very long name <someone@example.com>", revision]);
        assert_eq!(wrapped(&m, 40), None);
        // A body right under the title: the title alone is left.
        let m = message(&[title, "and a body under it, with no empty line between them"]);
        let (text, _) = checked(&m, 40);
        assert_eq!(text, message(&[title, "and a body under it, with no empty line", "between them"]));
        // Empty lines before it: the title is the first line that is not.
        let m = message(&["", title, "", "and a body under it, with an empty line between them"]);
        let (text, _) = checked(&m, 40);
        assert_eq!(text, message(&["", title, "", "and a body under it, with an empty line", "between them"]));
        // Prose before the trailers, in their paragraph: wrapped, the
        // trailers after it as they were, a line each.
        let m = message(&[title, "", "This paragraph is long enough to be wrapped at forty.", revision, "Reviewed-by: ann"]);
        let (text, long) = checked(&m, 40);
        assert_eq!(text, message(&[title, "", "This paragraph is long enough to be", "wrapped at forty.", revision, "Reviewed-by: ann"]));
        assert_eq!(long, 1);
        // A line like a trailer, in a paragraph other than the last: prose.
        let m = message(&[title, "", "Note: a line like a trailer, but in a paragraph before the last.", "", "The end."]);
        let (text, _) = checked(&m, 40);
        assert_eq!(text.lines().nth(2), Some("Note: a line like a trailer, but in a"));
    }

    #[test]
    fn a_quoted_line_is_left_as_it_is_the_prose_around_it_filled_on_its_own() {
        let m = message(&[
            "Bug 1 - quote",
            "",
            "As the bug says, and it says so at length:",
            "> the parser crashes on an empty file, every single time",
            "which this fixes for every file there is, empty or not.",
        ]);
        let (text, long) = checked(&m, 40);
        let want = [
            "Bug 1 - quote",
            "",
            "As the bug says, and it says so at",
            "length:",
            "> the parser crashes on an empty file, every single time",
            "which this fixes for every file there",
            "is, empty or not.",
        ];
        assert_eq!(text, message(&want));
        assert_eq!(long, 2, "the quoted line is not prose to wrap");
    }

    #[test]
    fn a_word_that_would_start_a_line_as_something_else_goes_on_the_line_before() {
        // Each word would start the second line: a list's marker, a quote, a
        // prompt, a fence, what git reads as a comment.
        for word in ["-", "*", "+", "1.", "23)", ">", ">>", "$", "```", "~~~", "#1234"] {
            let m = format!("Bug 1\n\naaaa bbbb cccc dddd eeee ffff gggg hhhh {word} iiii");
            let (text, _) = checked(&m, 40);
            assert_eq!(text, format!("Bug 1\n\naaaa bbbb cccc dddd eeee ffff gggg\nhhhh {word} iiii"), "{word:?}");
        }
        // Words like them, that do not make a line something else, start
        // one as any word does.
        for word in ["-x", "1.5", "e.g.", "$x", "a#b"] {
            let m = format!("Bug 1\n\naaaa bbbb cccc dddd eeee ffff gggg hhhh {word} iiii");
            let (text, _) = checked(&m, 40);
            assert_eq!(text, format!("Bug 1\n\naaaa bbbb cccc dddd eeee ffff gggg hhhh\n{word} iiii"), "{word:?}");
        }
    }

    #[test]
    fn what_is_wrapped_says_what_it_said_as_compare_reads_it_and_leaves_nothing_to_wrap() {
        let m = message(&[
            "Bug 1 - all at once, with a title longer than forty columns",
            "",
            "A paragraph of prose, long enough to wrap at every width tried here, with   gaps   in it",
            "and a second line.",
            "",
            "- an item, long enough to wrap at forty columns",
            "- another, with a link https://example.com/a/very/long/path/to/somewhere",
            "  under it",
            "",
            "> a quoted line that is longer than forty columns, kept",
            "",
            "```",
            "a fenced line that is longer than forty columns, kept",
            "",
            "```",
            "",
            "    an indented line that is longer than forty columns",
            "",
            "Prose again: it costs $ 5 - or # 6 - and > 7, 1. and 2) are words here.",
            "",
            "Reviewed-by: someone with a long name <someone@example.com>",
        ]);
        for width in [20, 30, 40, 50, 60, 72] {
            let (text, _) = checked(&m, width);
            for (old, new) in [(0, 0), (9, text.lines().position(|l| l.starts_with("> ")).unwrap())] {
                assert_eq!(m.lines().nth(old), text.lines().nth(new), "at {width}");
            }
            assert!(text.ends_with("\n\nReviewed-by: someone with a long name <someone@example.com>"), "at {width}:\n{text}");
        }
        assert_eq!(checked(&m, 40).1, 4);
        // A message whose lines end with a carriage return keeps it on each
        // line, those wrapped too.
        let m = message(&["Bug 1\r", "\r", "The parser read the whole file before it checked the header.\r"]);
        let (text, _) = checked(&m, 40);
        assert_eq!(text, message(&["Bug 1\r", "\r", "The parser read the whole file before it\r", "checked the header.\r"]));
        // And its last line ending, as it was.
        let (text, _) = checked(&format!("{m}\n"), 40);
        assert!(text.ends_with("header.\r\n"), "{text:?}");
    }

    #[test]
    fn a_line_of_such_a_word_alone_stays_as_it_is_and_cuts_its_paragraph() {
        // Filled, `-` would start a list item there, and `$` a command.
        for word in ["-", "*", "1.", "$"] {
            let m = message(&["Bug 1", "", "Before it.", word, "a paragraph after it that is long enough to be wrapped"]);
            let (text, _) = checked(&m, 40);
            let want = ["Bug 1", "", "Before it.", word, "a paragraph after it that is long enough", "to be wrapped"];
            assert_eq!(text, message(&want), "{word:?}");
        }
    }

    #[test]
    fn messages_made_up_of_lines_of_every_kind_are_filled_to_the_same_words_once_and_for_all() {
        // Unchecked, what is filled says what the message said, as `compare`
        // reads them, and leaves nothing to fill: `wrapped`'s check is never
        // what keeps a message right.
        let words = [
            "the", "parser", "reads", "a", "file", "-", "*", "+", "1.", "2)", ">", "$", "#12", "```", "~~~", "e.g.", "Key:", "語語",
            "https://example.com/a/path/that/goes/on/and/on/past/the/width",
        ];
        let mut seed: u64 = 7;
        let mut next = |n: usize| {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (seed >> 33) as usize % n
        };
        let mut filled = 0;
        for _ in 0..3000 {
            let mut lines = vec!["Bug 1 - a title".to_string(), String::new()];
            for _ in 0..next(12) {
                let said: Vec<&str> = (0..1 + next(14)).map(|_| words[next(words.len())]).collect();
                let said = said.join(if next(4) == 0 { "   " } else { " " });
                lines.push(match next(11) {
                    0 => String::new(),
                    1 => format!("- {said}"),
                    2 => format!("  {said}"),
                    3 => format!("> {said}"),
                    4 => format!("    {said}"),
                    5 => "```".into(),
                    6 => format!("Reviewed-by: {said}"),
                    7 => format!("12. {said}"),
                    8 => words[next(words.len())].into(),
                    _ => said,
                });
            }
            let m = lines.join("\n");
            for width in [20, 40, 72] {
                let Some(w) = refill(&m, width) else { continue };
                filled += 1;
                assert!(same(&m, &w.message), "not the same at {width}:\n{m}\n--\n{}", w.message);
                assert_eq!(refill(&w.message, width), None, "more to fill at {width}:\n{m}\n--\n{}", w.message);
            }
        }
        assert!(filled > 1000, "only {filled} filled");
    }

    #[test]
    fn a_wide_character_takes_two_columns() {
        let m = format!("Bug 1\n\n{}", ["語語"; 10].join(" "));
        let (text, long) = checked(&m, 20);
        assert_eq!(long, 1);
        assert_eq!(text, format!("Bug 1\n\n{}\n{}\n{}", ["語語"; 4].join(" "), ["語語"; 4].join(" "), ["語語"; 2].join(" ")));
    }

    #[test]
    fn the_menus_say_how_many_lines_of_which_patches_are_longer_than_the_width() {
        assert_eq!(said(&[(1, 3)], 72), "Patch 2's message has 3 lines of prose longer than 72 columns.");
        assert_eq!(said(&[(0, 1)], 72), "Patch 1's message has 1 line of prose longer than 72 columns.");
        assert_eq!(said(&[(1, 3), (3, 1)], 72), "Patch 2's message has 3 lines of prose longer than 72 columns and patch 4's has 1.");
        assert_eq!(
            said(&[(1, 3), (3, 1), (4, 2)], 60),
            "Patch 2's message has 3 lines of prose longer than 60 columns, patch 4's has 1 and patch 5's has 2."
        );
    }
}
