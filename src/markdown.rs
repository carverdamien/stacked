//! Markdown in comments, as the editor shows it: what each piece of a turn's
//! text is (code, bold, a list's bullet…), the editor choosing how each
//! looks. The markdown comments use: paragraphs, `#` headings, `-`, `*`,
//! `+` and `1.` lists, `>` quotes, code blocks between fences, tables,
//! rules; and in a line, `code`, **bold**, *italics* and [links](to). A line
//! breaks where the text breaks it, as comments are written, and again
//! where it is too wide; a code block's lines keep their spaces, cut only
//! where too wide; a table's cells fold within their columns.

/// What a piece of text is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    Plain,
    /// `code`, in a line.
    Code,
    /// **bold**.
    Strong,
    /// *italics*.
    Emph,
    /// A link's text; and where it goes, after it.
    Link,
    Url,
    /// A heading's text.
    Heading,
    /// The bar left of a quote.
    Quote,
    /// A list item's bullet, or its number.
    Bullet,
    /// A line of a code block.
    Block,
    /// A rule.
    Rule,
    /// A mention of references a comment has, `@r1`.
    Mention,
    /// A mention of references it does not have.
    Broken,
}

/// A line as drawn: its pieces, each with what it is.
pub type Line = Vec<(String, Mark)>;

/// `text`, a turn of a comment, as lines `width` wide.
pub fn render(text: &str, width: usize) -> Vec<Line> {
    let width = width.max(10);
    let lines: Vec<&str> = text.lines().collect();
    let mut out: Vec<Line> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let t = line.trim_start();
        i += 1;
        // A code block: its lines to its fence, or to the end, as they are.
        if let Some((c, n)) = opens(t) {
            let end = (i..lines.len()).find(|&k| closes(lines[k].trim_start(), c, n)).unwrap_or(lines.len());
            out.extend(block(&lines[i..end], width));
            i = end + 1;
            continue;
        }
        // A table: a header, a row of dashes, then rows, as long as they
        // have pipes.
        if let Some((rows, aligns, used)) = table(&lines[i - 1..]) {
            match draw_table(&rows, &aligns, width) {
                Some(drawn) => out.extend(drawn),
                // Too narrow even folded: as it is written.
                None => out.extend(lines[i - 1..i - 1 + used].iter().flat_map(|l| fold(&inline(l), width, &[], &[]))),
            }
            i += used - 1;
            continue;
        }
        if t.is_empty() {
            out.push(Vec::new());
        } else if is_rule(t) {
            out.push(vec![("─".repeat(width.min(40)), Mark::Rule)]);
        } else if let Some(rest) = heading(t) {
            let pieces = marked(inline(rest), Mark::Heading);
            out.extend(fold(&pieces, width, &[], &[]));
        } else if let Some(rest) = t.strip_prefix('>') {
            let rest = rest.strip_prefix(' ').unwrap_or(rest);
            let bar = [("▏ ".to_string(), Mark::Quote)];
            out.extend(fold(&inline(rest), width, &bar, &bar));
        } else if let Some((marker, rest)) = item(t) {
            // Nested by its indent; its lines after the first under its text.
            let indent = " ".repeat(line.len() - t.len());
            let first = [(indent.clone(), Mark::Plain), (marker.clone(), Mark::Bullet), (" ".to_string(), Mark::Plain)];
            let next = [(" ".repeat(indent.len() + marker.chars().count() + 1), Mark::Plain)];
            out.extend(fold(&inline(rest), width, &first, &next));
        } else {
            out.extend(fold(&inline(line), width, &[], &[]));
        }
    }
    out
}

/// What `text` says first, in a line: its first line that is not blank nor
/// a code block's fence, without what marks it up.
pub fn first_line(text: &str) -> String {
    text.lines().map(str::trim).find(|l| !l.is_empty() && opens(l).is_none()).map(plain).unwrap_or_default()
}

/// `text`, a line, without what marks it up: for a comment said in a line,
/// where it is listed or folded. Where a link goes is left out.
pub fn plain(text: &str) -> String {
    let t = text.trim_start();
    let rest = heading(t)
        .or_else(|| t.strip_prefix('>').map(str::trim_start))
        .or_else(|| item(t).map(|(_, rest)| rest))
        .unwrap_or(text);
    inline(rest).into_iter().filter(|(_, m)| *m != Mark::Url).map(|(s, _)| s).collect()
}

/// The fence a code block opens with, if `t` (a line, its indent left out)
/// is one: three backticks or tildes, or more, then what language it is.
fn opens(t: &str) -> Option<(char, usize)> {
    let c = t.chars().next().filter(|c| *c == '`' || *c == '~')?;
    let n = t.chars().take_while(|x| *x == c).count();
    (n >= 3 && !(c == '`' && t[n..].contains('`'))).then_some((c, n))
}

/// Whether `t` closes the code block opened with `n` of `c`.
fn closes(t: &str, c: char, n: usize) -> bool {
    let t = t.trim_end();
    t.chars().count() >= n && t.chars().all(|x| x == c)
}

/// Whether `t` is a rule: three of `-`, `*` or `_` or more, spaces between.
fn is_rule(t: &str) -> bool {
    let s: String = t.chars().filter(|c| !c.is_whitespace()).collect();
    let Some(c) = s.chars().next() else { return false };
    matches!(c, '-' | '*' | '_') && s.chars().count() >= 3 && s.chars().all(|x| x == c)
}

/// A heading's text, if `t` is one: `#` to `######`, then a space.
fn heading(t: &str) -> Option<&str> {
    let n = t.chars().take_while(|c| *c == '#').count();
    if !(1..=6).contains(&n) {
        return None;
    }
    let rest = &t[n..];
    (rest.is_empty() || rest.starts_with(' ')).then(|| rest.trim().trim_end_matches('#').trim_end())
}

/// A list item's marker, as it is drawn (`•`, `1.`), and its text, if `t`
/// is one.
fn item(t: &str) -> Option<(String, &str)> {
    for b in ['-', '*', '+'] {
        if let Some(rest) = t.strip_prefix(b).and_then(|r| r.strip_prefix(' ')) {
            return Some(("•".into(), rest.trim_start()));
        }
    }
    let digits = t.chars().take_while(char::is_ascii_digit).count();
    if (1..=9).contains(&digits) {
        let rest = &t[digits..];
        for end in ['.', ')'] {
            if let Some(rest) = rest.strip_prefix(end).and_then(|r| r.strip_prefix(' ')) {
                return Some((format!("{}{end}", &t[..digits]), rest.trim_start()));
            }
        }
    }
    None
}

/// `pieces` with what is plain marked `m`; what is marked otherwise keeps it.
fn marked(pieces: Line, m: Mark) -> Line {
    pieces.into_iter().map(|(s, x)| (s, if x == Mark::Plain { m } else { x })).collect()
}

/// `s` as `m` after the pieces of `out`: with the last one, if it is `m` too.
fn push(out: &mut Line, s: &str, m: Mark) {
    if s.is_empty() {
        return;
    }
    match out.last_mut() {
        Some((t, last)) if *last == m => t.push_str(s),
        _ => out.push((s.to_string(), m)),
    }
}

/// `text`, a line of markdown, as pieces: code, bold, italics, links.
pub fn inline(text: &str) -> Line {
    let cs: Vec<char> = text.chars().collect();
    let mut out: Line = Vec::new();
    let mut plain = String::new();
    let mut i = 0;
    let flush = |out: &mut Line, plain: &mut String| {
        push(out, plain, Mark::Plain);
        plain.clear();
    };
    while i < cs.len() {
        let c = cs[i];
        // A character escaped is itself.
        if c == '\\' && cs.get(i + 1).is_some_and(|n| n.is_ascii_punctuation()) {
            plain.push(cs[i + 1]);
            i += 2;
            continue;
        }
        if c == '`' {
            let n = run(&cs, i, '`');
            if let Some(end) = find_run(&cs, i + n, '`', n) {
                flush(&mut out, &mut plain);
                let code: String = cs[i + n..end].iter().collect();
                // One space either side of it, as markdown has it, left out.
                let code = match code.strip_prefix(' ').and_then(|c| c.strip_suffix(' ')) {
                    Some(inner) if !inner.trim().is_empty() => inner.to_string(),
                    _ => code,
                };
                push(&mut out, &code, Mark::Code);
                i = end + n;
            } else {
                plain.extend(&cs[i..i + n]);
                i += n;
            }
            continue;
        }
        if c == '*' || c == '_' {
            let n = run(&cs, i, c).min(2);
            let before = i.checked_sub(1).map(|b| cs[b]);
            // Opens when it is followed by a word, and, an underscore, not
            // within one (snake_case).
            let opens = cs.get(i + n).is_some_and(|x| !x.is_whitespace()) && (c == '*' || !before.is_some_and(char::is_alphanumeric));
            if opens && let Some(end) = closing(&cs, i + n, c, n) {
                flush(&mut out, &mut plain);
                let inner: String = cs[i + n..end].iter().collect();
                for (s, m) in marked(inline(&inner), if n == 2 { Mark::Strong } else { Mark::Emph }) {
                    push(&mut out, &s, m);
                }
                i = end + n;
            } else {
                plain.extend(&cs[i..i + n]);
                i += n;
            }
            continue;
        }
        if c == '['
            && let Some((text_end, url_end)) = link(&cs, i)
        {
            flush(&mut out, &mut plain);
            let label: String = cs[i + 1..text_end].iter().collect();
            let url: String = cs[text_end + 2..url_end].iter().collect();
            for (s, m) in marked(inline(&label), Mark::Link) {
                push(&mut out, &s, m);
            }
            if url.trim() != label.trim() && !url.trim().is_empty() {
                push(&mut out, &format!(" ({})", url.trim()), Mark::Url);
            }
            i = url_end + 1;
            continue;
        }
        // <https://…>: a link to itself.
        if c == '<'
            && let Some(end) = cs[i + 1..].iter().position(|x| *x == '>').map(|e| i + 1 + e)
        {
            let inner: String = cs[i + 1..end].iter().collect();
            if (inner.starts_with("http://") || inner.starts_with("https://")) && !inner.contains(char::is_whitespace) {
                flush(&mut out, &mut plain);
                push(&mut out, &inner, Mark::Link);
                i = end + 1;
                continue;
            }
        }
        plain.push(c);
        i += 1;
    }
    flush(&mut out, &mut plain);
    out
}

/// How many `c` there are in a row from `i`.
fn run(cs: &[char], i: usize, c: char) -> usize {
    cs[i..].iter().take_while(|x| **x == c).count()
}

/// Where, from `from`, a run of exactly `n` of `c` starts.
fn find_run(cs: &[char], from: usize, c: char, n: usize) -> Option<usize> {
    let mut i = from;
    while i < cs.len() {
        if cs[i] == c {
            let k = run(cs, i, c);
            if k == n {
                return Some(i);
            }
            i += k;
        } else {
            i += 1;
        }
    }
    None
}

/// Where, from `from`, the `n` of `c` that close emphasis opened with them
/// are: after a word, and, an underscore, not within one; none past a code
/// span's start, so as not to end within it.
fn closing(cs: &[char], from: usize, c: char, n: usize) -> Option<usize> {
    let mut i = from;
    while i < cs.len() {
        if cs[i] == '`' {
            let k = run(cs, i, '`');
            i = find_run(cs, i + k, '`', k).map_or(i + k, |e| e + k);
            continue;
        }
        if cs[i] == c {
            let k = run(cs, i, c);
            let after_word = i > from && !cs[i - 1].is_whitespace();
            let word_after = cs.get(i + k).is_some_and(|x| x.is_alphanumeric());
            if k >= n && after_word && (c == '*' || !word_after) {
                return Some(i);
            }
            i += k;
        } else {
            i += 1;
        }
    }
    None
}

/// A link at `i` (`[`): where its text ends (`]`) and its address does
/// (`)`), if it is one.
fn link(cs: &[char], i: usize) -> Option<(usize, usize)> {
    let text_end = cs[i + 1..].iter().position(|x| *x == ']').map(|e| i + 1 + e)?;
    if cs.get(text_end + 1) != Some(&'(') || text_end == i + 1 {
        return None;
    }
    let url_end = cs[text_end + 2..].iter().position(|x| *x == ')').map(|e| text_end + 2 + e)?;
    Some((text_end, url_end))
}

/// How a table's column is aligned, as the colons of its row of dashes say.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Align {
    Left,
    Center,
    Right,
}

/// The cells of a table's row: between its pipes, trimmed. A pipe escaped,
/// or in code, is the cell's own.
fn cells(line: &str) -> Vec<String> {
    let t = line.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let t = if t.ends_with('|') && !t.ends_with("\\|") { &t[..t.len() - 1] } else { t };
    let mut out = Vec::new();
    let (mut cur, mut code, mut escaped) = (String::new(), false, false);
    for c in t.chars() {
        match c {
            '|' if escaped => {
                cur.pop();
                cur.push('|');
            }
            '|' if !code => out.push(std::mem::take(&mut cur).trim().to_string()),
            _ => {
                if c == '`' {
                    code = !code;
                }
                cur.push(c);
            }
        }
        escaped = c == '\\';
    }
    out.push(cur.trim().to_string());
    out
}

/// How a cell of a table's row of dashes aligns its column, if it is one:
/// dashes, a colon at either end or both.
fn align_of(cell: &str) -> Option<Align> {
    let inner = cell.trim_start_matches(':').trim_end_matches(':');
    if inner.is_empty() || !inner.chars().all(|c| c == '-') {
        return None;
    }
    Some(match (cell.starts_with(':'), cell.ends_with(':')) {
        (true, true) => Align::Center,
        (false, true) => Align::Right,
        _ => Align::Left,
    })
}

/// The table `lines` start with, if they start with one: its rows of cells,
/// the header first; how its columns align; and how many lines it takes.
fn table(lines: &[&str]) -> Option<(Vec<Vec<String>>, Vec<Align>, usize)> {
    let head = lines.first()?;
    if !head.contains('|') {
        return None;
    }
    let aligns: Vec<Align> = cells(lines.get(1)?).iter().map(|c| align_of(c)).collect::<Option<_>>()?;
    let header = cells(head);
    if header.len() != aligns.len() {
        return None;
    }
    let mut rows = vec![header];
    let mut used = 2;
    while let Some(l) = lines.get(used).filter(|l| !l.trim().is_empty() && l.contains('|')) {
        rows.push(cells(l));
        used += 1;
    }
    Some((rows, aligns, used))
}

/// Columns as wide as `natural` asks, shrunk to `room` when it is less: the
/// narrow ones as they are, the room left shared by the others. None when
/// each cannot have three characters.
fn fit_columns(natural: &[usize], room: usize) -> Option<Vec<usize>> {
    if natural.iter().sum::<usize>() <= room {
        return Some(natural.to_vec());
    }
    if room < 3 * natural.len() {
        return None;
    }
    let mut widths = natural.to_vec();
    let (mut left, mut open): (usize, Vec<usize>) = (room, (0..natural.len()).collect());
    loop {
        let share = left / open.len();
        let (fit, wide): (Vec<usize>, Vec<usize>) = open.iter().partition(|&&c| natural[c] <= share);
        if fit.is_empty() {
            for (k, &c) in wide.iter().enumerate() {
                widths[c] = share + usize::from(k < left % wide.len());
            }
            return Some(widths);
        }
        left -= fit.iter().map(|&c| natural[c]).sum::<usize>();
        open = wide;
    }
}

/// Table `rows` drawn `width` wide: its columns aligned as `aligns` says,
/// `│` between them, its header bold with a rule under it; each cell folded
/// within its column when the table is wider than that. None when it
/// cannot fit even so.
fn draw_table(rows: &[Vec<String>], aligns: &[Align], width: usize) -> Option<Vec<Line>> {
    let n = aligns.len();
    let wide = |l: &[(String, Mark)]| l.iter().map(|(s, _)| s.chars().count()).sum::<usize>();
    let pieces: Vec<Vec<Line>> = rows
        .iter()
        .enumerate()
        .map(|(r, row)| {
            (0..n)
                .map(|c| {
                    let cell = inline(row.get(c).map_or("", String::as_str));
                    if r == 0 { marked(cell, Mark::Strong) } else { cell }
                })
                .collect()
        })
        .collect();
    let natural: Vec<usize> = (0..n).map(|c| pieces.iter().map(|row| wide(&row[c])).max().unwrap_or(0).max(1)).collect();
    let widths = fit_columns(&natural, width.checked_sub(3 * (n - 1))?)?;
    let mut out = Vec::new();
    for (r, row) in pieces.iter().enumerate() {
        let folded: Vec<Vec<Line>> = (0..n).map(|c| fold(&row[c], widths[c], &[], &[])).collect();
        for k in 0..folded.iter().map(Vec::len).max().unwrap_or(1) {
            let mut line: Line = Vec::new();
            for c in 0..n {
                if c > 0 {
                    push(&mut line, " │ ", Mark::Rule);
                }
                let cell = folded[c].get(k).cloned().unwrap_or_default();
                let pad = widths[c].saturating_sub(wide(&cell));
                let (before, after) = match aligns[c] {
                    Align::Left => (0, pad),
                    Align::Right => (pad, 0),
                    Align::Center => (pad / 2, pad - pad / 2),
                };
                push(&mut line, &" ".repeat(before), Mark::Plain);
                for (s, m) in cell {
                    push(&mut line, &s, m);
                }
                if c + 1 < n {
                    push(&mut line, &" ".repeat(after), Mark::Plain);
                }
            }
            out.push(line);
        }
        if r == 0 {
            let rule: Vec<String> = widths.iter().map(|w| "─".repeat(*w)).collect();
            out.push(vec![(rule.join("─┼─"), Mark::Rule)]);
        }
    }
    Some(out)
}

/// A code block's `lines` as they are, a space at either side, each as
/// wide as the widest, so that they read as one block; cut where too wide.
fn block(lines: &[&str], width: usize) -> Vec<Line> {
    let parts: Vec<String> = lines
        .iter()
        .flat_map(|l| if l.is_empty() { vec![String::new()] } else { cut(l, width.saturating_sub(2).max(1)) })
        .collect();
    let wide = parts.iter().map(|l| l.chars().count()).max().unwrap_or(0);
    parts.into_iter().map(|l| vec![(format!(" {l}{} ", " ".repeat(wide - l.chars().count())), Mark::Block)]).collect()
}

/// `line` cut every `width` characters, its spaces as they are.
fn cut(line: &str, width: usize) -> Vec<String> {
    let cs: Vec<char> = line.chars().collect();
    cs.chunks(width).map(|c| c.iter().collect()).collect()
}

/// `pieces` folded between words to lines `width` wide: the first after
/// `first`, the others after `next`. A word wider than a line is cut.
fn fold(pieces: &[(String, Mark)], width: usize, first: &[(String, Mark)], next: &[(String, Mark)]) -> Vec<Line> {
    let wide = |l: &[(String, Mark)]| l.iter().map(|(s, _)| s.chars().count()).sum::<usize>();
    let mut out = Vec::new();
    let mut line: Line = first.to_vec();
    let mut used = wide(first);
    let mut words = false;
    let mut space: Option<(String, Mark)> = None;
    // Words and the spaces between them, each with its piece's mark.
    let mut tokens: Vec<(String, Mark, bool)> = Vec::new();
    for (s, m) in pieces {
        let mut cur = String::new();
        let mut blank = false;
        for ch in s.chars() {
            if !cur.is_empty() && ch.is_whitespace() != blank {
                tokens.push((std::mem::take(&mut cur), *m, blank));
            }
            blank = ch.is_whitespace();
            cur.push(ch);
        }
        if !cur.is_empty() {
            tokens.push((cur, *m, blank));
        }
    }
    for (tok, m, blank) in tokens {
        if blank {
            if words {
                space = Some((tok, m));
            }
            continue;
        }
        let sw = space.as_ref().map_or(0, |(s, _)| s.chars().count());
        let mut word: Vec<char> = tok.chars().collect();
        if words && used + sw + word.len() > width {
            out.push(std::mem::replace(&mut line, next.to_vec()));
            used = wide(next);
            space = None;
        }
        if let Some((s, sm)) = space.take() {
            push(&mut line, &s, sm);
            used += sw;
        }
        // Wider than a line of its own: cut where the line ends.
        while used + word.len() > width {
            let room = width.saturating_sub(used).max(1);
            push(&mut line, &word[..room].iter().collect::<String>(), m);
            out.push(std::mem::replace(&mut line, next.to_vec()));
            used = wide(next);
            word.drain(..room);
        }
        push(&mut line, &word.iter().collect::<String>(), m);
        used += word.len();
        words = true;
    }
    out.push(line);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn said(lines: &[Line]) -> Vec<String> {
        lines.iter().map(|l| l.iter().map(|(s, _)| s.as_str()).collect()).collect()
    }

    #[test]
    fn a_line_is_read_for_code_bold_italics_and_links() {
        use Mark::*;
        assert_eq!(
            inline("Use `ns*http` with **care**, *really*, see [bug 1](https://b/1)."),
            [
                ("Use ".to_string(), Plain),
                ("ns*http".into(), Code),
                (" with ".into(), Plain),
                ("care".into(), Strong),
                (", ".into(), Plain),
                ("really".into(), Emph),
                (", see ".into(), Plain),
                ("bug 1".into(), Link),
                (" (https://b/1)".into(), Url),
                (".".into(), Plain),
            ]
        );
        // Not markup: a word's underscores, a lone star, an escape, code's own.
        assert_eq!(inline("snake_case_name, 2 * 3, \\*x\\*"), [("snake_case_name, 2 * 3, *x*".to_string(), Plain)]);
        assert_eq!(inline("``a `b` c``"), [("a `b` c".to_string(), Code)]);
        assert_eq!(inline("**`code` bold**"), [("code".to_string(), Code), (" bold".into(), Strong)]);
        assert_eq!(inline("<https://x.org/a>"), [("https://x.org/a".to_string(), Link)]);
        assert_eq!(inline("**open"), [("**open".to_string(), Plain)]);
        assert_eq!(plain("- **Bold** `x` and [y](z)"), "Bold x and y");
        assert_eq!(first_line("\n```rust\nlet `a` = 1;\n```"), "let a = 1;");
    }

    #[test]
    fn a_table_s_columns_are_aligned_and_fold_where_too_wide() {
        let text = "| Name | Level | Note |\n|:---|---:|:---:|\n| `nsHttp` | 5 | on |\n| Login crypto | 12 | a \\| b |\nafter";
        let lines = render(text, 60);
        assert_eq!(
            said(&lines),
            [
                "Name         │ Level │ Note",
                "─────────────┼───────┼──────",
                "nsHttp       │     5 │  on",
                "Login crypto │    12 │ a | b",
                "after",
            ]
        );
        assert_eq!(lines[0][0], ("Name".to_string(), Mark::Strong));
        assert_eq!(lines[2][0], ("nsHttp".to_string(), Mark::Code));
        // Too wide: each cell folded within its column, the narrow ones as
        // they are.
        let wide = "| a | b |\n|---|---|\n| x | one two three four five six |";
        assert_eq!(said(&render(wide, 16)), ["a │ b", "──┼─────────────", "x │ one two", "  │ three four", "  │ five six"]);
        // Too narrow even so: as it is written.
        assert_eq!(said(&render("|a|b|c|d|\n|-|-|-|-|", 10)), ["|a|b|c|d|", "|-|-|-|-|"]);
        // Pipes, but no row of dashes: no table.
        assert_eq!(said(&render("a | b\nc | d", 20)), ["a | b", "c | d"]);
    }

    #[test]
    fn blocks_are_lists_quotes_headings_rules_and_code() {
        let text = "# Title\n\nIntro.\n- one\n- two is long enough to fold under its text\n  1. nested\n> quoted\n---\n```rust\nfn  main() {}\n```\nend";
        let lines = render(text, 24);
        assert_eq!(
            said(&lines),
            [
                "Title",
                "",
                "Intro.",
                "• one",
                "• two is long enough to",
                "  fold under its text",
                "  1. nested",
                "▏ quoted",
                "────────────────────────",
                " fn  main() {} ",
                "end",
            ]
        );
        assert_eq!(lines[0], [("Title".to_string(), Mark::Heading)]);
        assert_eq!(lines[3][1], ("•".to_string(), Mark::Bullet));
        assert_eq!(lines[9], [(" fn  main() {} ".to_string(), Mark::Block)]);
        // A code block's lines as wide as its widest, a space either side; a
        // line too wide cut, its spaces kept; an open block goes to the end.
        assert_eq!(said(&render("```\na\nbcd\n```", 20)), [" a   ", " bcd "]);
        assert_eq!(said(&render("```\n  0123456789abc\n", 10)), ["   012345 ", " 6789abc  "]);
        // A word wider than a line is cut where the line ends.
        assert_eq!(said(&render("0123456789abcdef", 10)), ["0123456789", "abcdef"]);
    }
}
