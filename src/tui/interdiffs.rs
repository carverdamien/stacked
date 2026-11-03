//! The views of an interdiff, which `1` picks wherever one is shown
//! (`App::inter_screen`), as it picks the views of the code in the middle.
//! As an interdiff, as stacked always drew it, git range-diff's diff of two
//! diffs (`inter_parts`): each line a sign saying which version has it (a
//! space for both, `-` for the old one alone, `+` for the new one alone),
//! then that version's own sign. In a column per version, as git's
//! combined diff draws a merge (`columns`): before each line, what the old
//! version's patch does with it, then what the new one's does, `+` it adds
//! the line, `-` it removes it, a space it leaves it as it is; `=` the
//! version has the line, a header or a line of its message; `·` the
//! version does not have it. Both say once, at the top of the code, how to
//! read them, naming the versions as the screen names them
//! (`interdiff_said`, `columns_said`). Or side by side, after
//! moz-phab-diff's split view: the old version on the left and the new one
//! on the right, each half under a heading saying what it is, the
//! interdiff's lines paired as the code view's side by side pairs a diff's
//! (`rows::split_lines`). Each cell is its version's line as a diff draws
//! it, its own sign once: a line of the two diffs in the colour of its own
//! sign, what changed in it as its own version's diff pairs it in the
//! colours for words (`InterLine::words`); a line of the mail around them,
//! its headers and its message, as the interdiff draws it, compared across
//! the two halves. In a column per version and side by side, the lines both
//! versions have, and do the same with, are dim, so that what differs
//! stands out; the interdiff's hunks are as it cuts them, their headers
//! across. Side by side, a line too wide folds in its half, as the code
//! view's side by side folds its lines: one of code where the half ends,
//! one of the message between words. The view picked holds for every
//! interdiff while the editor is open, the settings' `view.interdiff` the
//! one they start in; a pane narrower than two sides need
//! (`view.split_width`, as the code view's) draws its interdiffs as
//! interdiffs meanwhile, and says so.

use super::*;
use crate::ops::{InterLine, Part};
use crate::rows::FOLDED;

impl InterLayout {
    /// Each, in the order the menu of 1 lists them.
    pub const ALL: [InterLayout; 3] = [InterLayout::Interdiff, InterLayout::Columns, InterLayout::Sides];

    /// As the menu of 1 lists it: how it reads.
    pub fn item(self) -> &'static str {
        match self {
            InterLayout::Interdiff => "Interdiff: a sign for which version has the line, then its own sign",
            InterLayout::Columns => "A column per version: what each one's patch does with the line",
            InterLayout::Sides => "Side by side: the old version on the left, the new one on the right",
        }
    }
}

/// What the menu of 1 is called on an interdiff's screen.
pub(super) const INTER_MENU: &str = "the view of the interdiffs";

/// The heading of the menu of 1's section of what the interdiff draws:
/// the two versions' diffs, compared, in each of the views of the section
/// (`InterLayout`).
pub(super) const INTERDIFF_SECTION: &str = "the interdiff: the two versions' diffs, compared";

/// The two versions an interdiff compares, as the screen names them, the
/// old one first; and whether both are on one base (`ops::other_bases`):
/// in a column per version, a line of the base that only one version's
/// diff shows is one the other leaves as it is, on one base, and one the
/// other is not known to have, on two.
#[derive(Debug, Clone, Copy)]
pub(super) struct Two<'a> {
    pub old: &'a str,
    pub new: &'a str,
    pub one_base: bool,
}

/// What the view as an interdiff says once, at the top of its code, of how
/// to read it: what each of the two signs says, of versions `two`.
pub(super) fn interdiff_said(two: &Two) -> String {
    let Two { old, new, .. } = two;
    format!(
        "The first sign of each line says which version has it: a space for both, - for {old} alone, + for {new} alone. The second is that version's own sign: + for a line its patch adds, - for one it removes, a space for one it leaves as it is."
    )
}

/// What the view in a column per version says once, at the top of its
/// code, of what each column is, of versions `two`.
pub(super) fn columns_said(two: &Two) -> String {
    let Two { old, new, .. } = two;
    format!(
        "The first column is {old}, the second {new}. In each, + says that the version's patch adds the line, - that it removes it, a space that it leaves it as it is, = that the version has the line, a header or a line of its message, and · that the version does not have the line."
    )
}

/// A stop of the cursor in an interdiff's lines, as drawn: its lines in
/// parts; and the lines of what is drawn it shows, by their places there
/// (one, or two, the old version's then the new one's), none for what is
/// said of them or a heading.
pub(super) type Stop = (Vec<Parts>, Vec<usize>);

/// What a view says once, at the top of an interdiff's code, of how to read
/// it, `said`, folded `w` wide after `indent` columns: a stop that shows no
/// line.
fn said_stop(said: &str, w: usize, indent: usize) -> Stop {
    let lines = wrap(said, w.max(20)).into_iter().map(|l| vec![(" ".repeat(indent), Style::new()), (l, color::meta())]).collect();
    (lines, vec![])
}

/// Characters, each in its style, back into parts, those of a style
/// together.
fn joined(cs: &[(char, Style)]) -> Parts {
    let mut out: Parts = Vec::new();
    for &(c, st) in cs {
        match out.last_mut() {
            Some((text, was)) if *was == st => text.push(c),
            _ => out.push((c.to_string(), st)),
        }
    }
    out
}

/// `parts` cut to their first `w` characters, and how many characters
/// they hold then.
fn cut(parts: &[(String, Style)], w: usize) -> (Parts, usize) {
    let mut out = Vec::new();
    let mut n = 0;
    for (text, st) in parts {
        let taken: String = text.chars().take(w - n).collect();
        n += taken.chars().count();
        out.push((taken, *st));
        if n == w {
            break;
        }
    }
    (out, n)
}

/// Ranges of a line's characters counted from its interdiff's sign, as
/// `InterLine` gives them, counted from the version's line after it.
fn unsigned(w: &[(usize, usize)]) -> Vec<(usize, usize)> {
    w.iter().map(|&(s, e)| (s.saturating_sub(1), e.saturating_sub(1))).collect()
}

/// The two halves of a line side by side `w` wide, as the code view's
/// side by side has them: each this wide, the old one's from its start, a
/// line between them after a column, the new one's after the line.
fn half_of(w: usize) -> usize {
    w.saturating_sub(3) / 2
}

/// A line of code in `parts`, its sign first, in pieces of at most `w`
/// characters, as the middle folds a line of code (`rows::fold_code`):
/// each after the first `↪` and what comes next, the `↪` in the colour of
/// the line's sign. As it is when it fits, or the half is too narrow to
/// fold it.
fn fold_code(parts: &[(String, Style)], w: usize) -> Vec<Parts> {
    let chars: Vec<(char, Style)> = parts.iter().flat_map(|(text, st)| text.chars().map(move |c| (c, *st))).collect();
    if chars.len() <= w || w < 10 {
        return vec![parts.to_vec()];
    }
    let lead = chars[0].1;
    let mut out = vec![joined(&chars[..w])];
    let mut at = w;
    while at < chars.len() {
        let end = (at + w - 1).min(chars.len());
        let mut piece = vec![(FOLDED, lead)];
        piece.extend_from_slice(&chars[at..end]);
        out.push(joined(&piece));
        at = end;
    }
    out
}

/// Interdiff line `l` as its version's cell side by side: its line after
/// the interdiff's sign, as a diff draws it. A line of the diffs
/// (`InterLine::code`) in the colour of its own sign, what changed in it as
/// its own version's diff pairs it in the colours for words
/// (`InterLine::words`); a line of the message compared with the other
/// version's as the interdiff draws it (`marked_parts`); any other, the
/// mail's headers, in its side's colour as the interdiff colours it, what
/// changed in it in the colours for words, `across`, as the interdiff pairs
/// it with the line across (`rows::word_spans`). A line both versions have
/// dim, no colour for words on it.
fn cell(l: &InterLine, across: &[(usize, usize)]) -> Parts {
    let mut chars = l.text.chars();
    let sign = chars.next().unwrap_or(' ');
    let line = chars.as_str();
    let (side, word) = match sign {
        '+' => (color::add(), color::add_word()),
        _ => (color::del(), color::del_word()),
    };
    let parts = match &l.own {
        _ if l.code() => {
            let word = if line.starts_with('+') { color::add_word() } else { color::del_word() };
            let words = if sign == ' ' { vec![] } else { unsigned(&l.words) };
            split_words(line, &words, diff_style(line), word)
        }
        Some((own, broken)) => marked_parts(line, &unsigned(own), *broken, word),
        None if sign == ' ' => vec![(line.to_string(), Style::new())],
        None => split_words(line, &unsigned(across), side, word),
    };
    match sign {
        ' ' => parts.into_iter().map(|(text, st)| (text, st.add_modifier(Modifier::DIM))).collect(),
        _ => parts,
    }
}

/// `cell`, or `~` dim where its side has nothing there, cut and padded to
/// `w` columns after `parts`: the room after it the terminal's own, though
/// it ends in a colour for words.
fn put_cell(parts: &mut Parts, cell: Option<&Parts>, w: usize) {
    match cell {
        Some(c) => {
            let (c, n) = cut(c, w);
            parts.extend(c);
            parts.push((" ".repeat(w - n), Style::new()));
        }
        None => parts.push((fit("~", w), color::dim())),
    }
}

/// `cells`, the pieces of the lines of a diff in parts, side by side `w`
/// wide after `indent` columns, as the code view puts a diff's rows side by
/// side: paired as `rows` say where each goes (`rows::split_lines`), its
/// own row there by its place, `Kind::Del` on the old side, `Kind::Add` on
/// the new side, `Kind::Ctx` on both, any other kind across both halves,
/// and a piece after its line's first one `Row::cont`. Each line drawn,
/// with the places of the cells it draws, and whether it starts a line of
/// the diff, rather than going on with the pieces of the line before it.
pub(super) fn paired(rows: &[Row], cells: &[Parts], w: usize, indent: usize) -> Vec<(Parts, Vec<usize>, bool)> {
    let half = half_of(w);
    let lead = || vec![(" ".repeat(indent), Style::new())];
    split_lines(rows)
        .0
        .into_iter()
        .map(|s| {
            let mut parts = lead();
            if let Some(i) = s.span {
                parts.extend(cut(&cells[i], w).0);
                return (parts, vec![i], !rows[i].cont);
            }
            put_cell(&mut parts, s.left.map(|i| &cells[i]), half);
            parts.push((" │".into(), color::meta()));
            put_cell(&mut parts, s.right.map(|i| &cells[i]), half);
            let mut at: Vec<usize> = s.left.into_iter().chain(s.right).collect();
            at.dedup();
            let first = at.iter().any(|&i| !rows[i].cont);
            (parts, at, first)
        })
        .collect()
}

/// The heading of an interdiff side by side, `w` wide after `indent`
/// columns: what each version is, `heads`, over its half.
pub(super) fn heading(heads: (&str, &str), w: usize, indent: usize) -> Parts {
    let half = half_of(w);
    vec![
        (" ".repeat(indent), Style::new()),
        (fit(heads.0, half), color::file()),
        (" │".into(), color::meta()),
        (fit(heads.1, half), color::file()),
    ]
}

/// The lines of the pieces `drawn` of a diff side by side (`paired`), a
/// stop of the cursor for each line of the diff, or for each pair of lines
/// of the two versions: its lines, folded pieces and all, and the lines of
/// the diff it shows, by their places there, each cell being of line
/// `of[i]`.
fn stops_of(drawn: Vec<(Parts, Vec<usize>, bool)>, of: &[usize]) -> Vec<Stop> {
    let mut out: Vec<Stop> = Vec::new();
    for (parts, at, first) in drawn {
        let shows = at.iter().map(|&i| of[i]);
        match out.last_mut() {
            Some((lines, was)) if !first => {
                lines.push(parts);
                was.extend(shows);
                was.sort_unstable();
                was.dedup();
            }
            _ => out.push((vec![parts], shows.collect())),
        }
    }
    out
}

/// An interdiff side by side, `lines` from its first hunk's header (the
/// two lines naming the versions over it left out), `w` wide after
/// `indent` columns: under a heading of what each version is, `heads`, the
/// old version on the left and the new one on the right, paired as the
/// code view pairs a diff's lines (`paired`): a line both have on both
/// sides of its row, dim; a run of lines the old one has beside the run the
/// new one has after it, row by row; a line only one has alone in its row;
/// the hunks' headers across both halves. Each cell its version's line
/// (`cell`), folded in its half a column short of it, as the code view
/// folds its lines: one of code where the half ends (`fold_code`), any
/// other between words, a bullet's pieces under what follows the bullet
/// (`fold_parts`, `rows::hang`). A stop of the cursor for the heading,
/// which shows no line, then for each pair of lines.
pub(super) fn side_by_side(lines: &[&InterLine], heads: (&str, &str), w: usize, indent: usize) -> Vec<Stop> {
    let fold = half_of(w).saturating_sub(1);
    // The lines of the mail around the diffs paired across, as
    // `inter_parts` pairs them; those of the diffs, and of the message,
    // with none.
    let texts: Vec<&str> = lines.iter().map(|l| if l.own.is_some() || l.code() { " " } else { l.text.as_str() }).collect();
    let across = crate::rows::word_spans(&texts);
    let (mut rows, mut cells, mut of) = (Vec::new(), Vec::new(), Vec::new());
    for (k, l) in lines.iter().enumerate() {
        let kind = match l.text.chars().next() {
            Some(' ') => Kind::Ctx,
            Some('-') => Kind::Del,
            Some('+') => Kind::Add,
            _ => Kind::Hunk,
        };
        let pieces = match kind {
            // A hunk's header, and git's `\` line: across, as the interdiff
            // draws them.
            Kind::Hunk => vec![vec![(l.text.clone(), diff_style(&l.text))]],
            _ if l.code() => fold_code(&cell(l, &across[k]), fold),
            _ => fold_parts(&cell(l, &across[k]), fold, crate::rows::hang(&l.text[1..])),
        };
        for (j, piece) in pieces.into_iter().enumerate() {
            rows.push(Row {
                kind,
                cont: j > 0,
                ..Default::default()
            });
            cells.push(piece);
            of.push(k);
        }
    }
    let mut out: Vec<Stop> = vec![(vec![heading(heads, w, indent)], vec![])];
    out.extend(stops_of(paired(&rows, &cells, w, indent), &of));
    out
}

/// What a version does with a line, as its column marks it in the view of
/// a column per version: its patch adds it, removes it or leaves it as it
/// is, its own sign (`+`, `-`, a space); it has the line, a header or a
/// line of its message (`=`); or it does not have the line (`·`).
#[derive(Debug, Clone, Copy, PartialEq)]
enum Col {
    Sign(char),
    Has,
    Lacks,
}

impl Col {
    /// As its column draws it: a sign in the colour of a line added or
    /// removed, `=` as any text, `·` dim.
    fn drawn(self) -> (String, Style) {
        match self {
            Col::Sign('+') => ("+".into(), color::add()),
            Col::Sign('-') => ("-".into(), color::del()),
            Col::Sign(c) => (c.to_string(), Style::new()),
            Col::Has => ("=".into(), Style::new()),
            Col::Lacks => ("·".into(), color::dim()),
        }
    }
}

/// What a version's line is matched with the other version's by, in a
/// column per version: a line of a file by its text, and, the two versions
/// on one base, by whether a patch adds it (`true`), for a line one adds is
/// then never a line of the base the other keeps or removes; on two, the
/// line one adds is the one the other's base has, moved there, as when
/// part of a patch went into one below it, or out of it. A hunk's header by
/// what follows its ranges; a file's header, or git's `\` line, as it is; a
/// line of the mail around the diffs with none, by its own place.
#[derive(Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum Key<'a> {
    Line(bool, &'a str),
    Hunk(&'a str),
    Head(&'a str),
    Mail(usize),
}

/// Hunk header `head`, as a diff writes it (`@@ -1,3 +1,4 @@ fn f()`): its
/// two ranges (`-1,3 +1,4`), and what follows its second `@@`.
fn ranges(head: &str) -> Option<(&str, &str)> {
    let rest = head.strip_prefix("@@ ")?;
    let end = rest.find(" @@")?;
    Some((&rest[..end], &rest[end + 3..]))
}

/// Line `l`'s own sign, a line of a hunk of its version's diff: `+`, `-`
/// or a space; none for git's `\` line, and any other line.
fn own_sign(l: &InterLine) -> Option<char> {
    l.text.get(1..)?.chars().next().filter(|c| l.part == Part::Line && matches!(c, ' ' | '-' | '+'))
}

/// What line `l` of a version says after the interdiff's sign, and after
/// its own sign too, which its column says instead.
fn after_signs(l: &InterLine) -> &str {
    let at = if own_sign(l).is_some() { 2 } else { 1 };
    l.text.get(at..).unwrap_or_default()
}

/// An interdiff in a column per version, `lines` from its first hunk's
/// header (the two lines naming the versions over it left out), each line
/// after `indent` columns, as git's combined diff draws a merge: before
/// each line, what the old version does with it, then the new one (`Col`),
/// worked out from the interdiff's lines. A line both have: what each one's
/// diff says it does, the same. A run of lines one version alone has: those
/// of the two versions that are one line (`Key`), matched in their order (a
/// diff of the two versions' lines of the run), not across a line both
/// have, where a line would move past it, one line drawn where the old
/// version has it, what each one's diff says it does, a hunk's header of
/// the two giving both versions' ranges (`@@ -1,3 +1,3 → -2,3 +2,3 @@`).
/// The rest, between them, the old version's first: what the one version
/// does, and of the other, a line the one adds it does not have, and a line
/// of the base the one keeps or removes, one the other's diff does not
/// show, the other leaves as it is where both are on one base
/// (`one_base`), and is not known to have else. A line of the mail around
/// the diffs, had by each or not. A line both versions have, doing the same
/// with it, dim, as a diff draws it; any other as any text, a line of the
/// message compared with the other version's as the interdiff marks it
/// (`marked_parts`), a header with what changed in it as the interdiff
/// pairs it across, and a line the one version adds beside the one the
/// other adds there, the two almost the same, what changed in each in the
/// colours for words, as the interdiff pairs them (`rows::word_spans`). The
/// interdiff's own hunks' headers across. A stop for each line, with the
/// lines of the interdiff it shows, the old version's first.
pub(super) fn columns(lines: &[&InterLine], one_base: bool, indent: usize) -> Vec<Stop> {
    let texts: Vec<&str> = lines.iter().map(|l| if l.own.is_some() || l.code() { " " } else { l.text.as_str() }).collect();
    let across = crate::rows::word_spans(&texts);
    let draw = |cols: [Col; 2], text: Parts, agree: bool| -> Parts {
        let mut out = vec![(" ".repeat(indent), Style::new())];
        out.extend(cols.map(Col::drawn));
        out.extend(text);
        match agree {
            true => out.into_iter().map(|(t, st)| (t, st.add_modifier(Modifier::DIM))).collect(),
            false => out,
        }
    };
    // A line both versions have, doing the same with it: as a diff draws
    // it, in its own sign's colour; a hunk's header in its own.
    let agreeing = |l: &InterLine| -> Parts {
        let st = match l.part {
            Part::Line => diff_style(&l.text[1..]),
            Part::Hunk => color::hunk(),
            _ => Style::new(),
        };
        vec![(after_signs(l).to_string(), st)]
    };
    // What the version that has line `l` alone does with it, then what the
    // other does.
    let cols_of = |l: &InterLine| -> (Col, Col) {
        match own_sign(l) {
            Some('+') => (Col::Sign('+'), Col::Lacks),
            Some(c) => (Col::Sign(c), if one_base { Col::Sign(' ') } else { Col::Lacks }),
            None => (Col::Has, Col::Lacks),
        }
    };
    // Those of each version alone, between two lines the same: the old
    // version's first, what changed in a line the one adds and the one the
    // other adds there in the colours for words.
    let alone = |(olds, news): (Vec<usize>, Vec<usize>)| -> Vec<Stop> {
        let adds: Vec<usize> = olds.iter().chain(&news).copied().filter(|&i| own_sign(lines[i]) == Some('+')).collect();
        let paired: Vec<String> = adds.iter().map(|&i| format!("{}{}", &lines[i].text[..1], after_signs(lines[i]))).collect();
        let spans = crate::rows::word_spans(&paired.iter().map(String::as_str).collect::<Vec<_>>());
        let mut out = Vec::new();
        for (i, old) in olds.iter().map(|&i| (i, true)).chain(news.iter().map(|&j| (j, false))) {
            let l = lines[i];
            let words = adds.iter().position(|&n| n == i).map_or(vec![], |at| unsigned(&spans[at]));
            let word = if old { color::del_word() } else { color::add_word() };
            let text = match (&l.own, l.part) {
                (Some((own, broken)), _) => marked_parts(after_signs(l), &unsigned(own), *broken, word),
                (None, Part::Hunk) => vec![(after_signs(l).to_string(), color::hunk())],
                (None, Part::Mail) => split_words(after_signs(l), &unsigned(&across[i]), Style::new(), word),
                _ => split_words(after_signs(l), &words, Style::new(), word),
            };
            let (mine, theirs) = cols_of(l);
            let cols = if old { [mine, theirs] } else { [theirs, mine] };
            out.push((vec![draw(cols, text, mine == theirs)], vec![i]));
        }
        out
    };
    // The same line of the two versions: what each does with it, as each
    // one's diff says; a hunk's header giving both versions' ranges.
    let both = |k: usize, m: usize| -> Stop {
        let (a, b) = (lines[k], lines[m]);
        let cols = match (own_sign(a), own_sign(b)) {
            (Some(c), Some(d)) => [Col::Sign(c), Col::Sign(d)],
            _ => [Col::Has, Col::Has],
        };
        let two = match (a.part, ranges(&a.text[1..]), ranges(&b.text[1..])) {
            (Part::Hunk, Some((x, rest)), Some((y, _))) if x != y => Some(format!("@@ {x} → {y} @@{rest}")),
            _ => None,
        };
        let (text, agree) = match two {
            Some(t) => (vec![(t, color::hunk())], false),
            None if cols[0] == cols[1] => (agreeing(a), true),
            None => (vec![(after_signs(a).to_string(), Style::new())], false),
        };
        (vec![draw(cols, text, agree)], vec![k, m])
    };
    let key = |k: usize| -> Key {
        let l = lines[k];
        let line = l.text.get(1..).unwrap_or_default();
        match (l.part, own_sign(l)) {
            (Part::Line, Some(c)) => Key::Line(one_base && c == '+', &line[1..]),
            (Part::Hunk, _) => Key::Hunk(ranges(line).map_or(line, |(_, rest)| rest)),
            (Part::Mail, _) => Key::Mail(k),
            _ => Key::Head(line),
        }
    };
    let mut out: Vec<Stop> = Vec::new();
    let mut k = 0;
    while k < lines.len() {
        let l = lines[k];
        match l.text.chars().next() {
            // Both have it, and do the same with it.
            Some(' ') => {
                let col = own_sign(l).map_or(Col::Has, Col::Sign);
                out.push((vec![draw([col, col], agreeing(l), true)], vec![k]));
                k += 1;
            }
            // A run of lines one version alone has: those of the two that
            // are one line, one.
            Some('-' | '+') => {
                let start = k;
                while lines.get(k).is_some_and(|l| l.text.starts_with(['-', '+'])) {
                    k += 1;
                }
                let olds: Vec<usize> = (start..k).filter(|&i| lines[i].text.starts_with('-')).collect();
                let news: Vec<usize> = (start..k).filter(|&i| lines[i].text.starts_with('+')).collect();
                let (a, b): (Vec<Key>, Vec<Key>) = (olds.iter().map(|&i| key(i)).collect(), news.iter().map(|&i| key(i)).collect());
                let mut apart: (Vec<usize>, Vec<usize>) = (vec![], vec![]);
                for op in similar::capture_diff_slices(similar::Algorithm::Myers, &a, &b) {
                    let (tag, x, y) = op.as_tag_tuple();
                    if tag != similar::DiffTag::Equal {
                        apart.0.extend(x.map(|i| olds[i]));
                        apart.1.extend(y.map(|j| news[j]));
                        continue;
                    }
                    out.extend(alone(std::mem::take(&mut apart)));
                    out.extend(x.zip(y).map(|(i, j)| both(olds[i], news[j])));
                }
                out.extend(alone(apart));
            }
            // The interdiff's own hunk's header, and git's `\` line: across,
            // as the interdiff draws them.
            _ => {
                out.push((vec![vec![(" ".repeat(indent), Style::new()), (l.text.clone(), diff_style(&l.text))]], vec![k]));
                k += 1;
            }
        }
    }
    out
}

/// One interdiff's stops in view `v`, `lines` as `ops` gives them, the two
/// lines naming the versions over its first hunk first, `w` wide with
/// `indent` columns before each line, of versions `two`: as an interdiff,
/// under what says how to read it, each line as `inter_parts` draws it; in
/// a column per version, under what says what each column is, the two
/// lines naming the versions left out (`columns`); side by side, those left
/// out too, each half under a heading of its version (`side_by_side`). Each
/// stop with the lines of `lines` it shows.
pub(super) fn interdiff_stops(lines: &[&InterLine], v: InterLayout, two: &Two, w: usize, indent: usize) -> Vec<Stop> {
    let names = lines.iter().take_while(|l| l.text.starts_with("--- ") || l.text.starts_with("+++ ")).count();
    let shifted = |stops: Vec<Stop>| -> Vec<Stop> { stops.into_iter().map(|(drawn, shows)| (drawn, shows.into_iter().map(|k| k + names).collect())).collect() };
    let block = w.saturating_sub(indent);
    match v {
        InterLayout::Interdiff => {
            let mut out = vec![said_stop(&interdiff_said(two), block, indent)];
            for (k, parts) in inter_parts(lines).into_iter().enumerate() {
                let mut drawn = vec![(" ".repeat(indent), Style::new())];
                drawn.extend(parts);
                out.push((vec![drawn], vec![k]));
            }
            out
        }
        InterLayout::Columns => {
            let mut out = vec![said_stop(&columns_said(two), block, indent)];
            out.extend(shifted(columns(&lines[names..], two.one_base, indent)));
            out
        }
        // The heading stands for the two lines it names.
        InterLayout::Sides => {
            let mut out = shifted(side_by_side(&lines[names..], (two.old, two.new), block, indent));
            if let Some((_, shows)) = out.first_mut() {
                *shows = (0..names).collect();
            }
            out
        }
    }
}

/// A stack diff's stops, as the screens draw it in view `v`, `w` wide: each
/// of its lines in parts, as `diff_parts` draws it, a stop of its own; but
/// its interdiffs, under the line of their patch, after the indent
/// `Diffed::text` gives them, in a column per version (`columns`) or side
/// by side (`side_by_side`, each under a heading of the two versions), a
/// stop for each line drawn of their versions. As an interdiff and in a
/// column per version, what says how to read them once, over the first.
/// The two versions named `names`, each patch's on one base or not as its
/// line says (`Diffed::Patch::rebased`). Each stop with the lines of
/// `diff` it shows.
pub(super) fn stack_stops(diff: &[Diffed], v: InterLayout, (old, new): (&str, &str), w: usize) -> Vec<Stop> {
    let indent = 1 + Diffed::Code(Default::default()).text().chars().count();
    let block = w.saturating_sub(indent);
    let lines = diff_lines(diff);
    let plain = match v {
        InterLayout::Interdiff => diff_parts(diff),
        _ => vec![],
    };
    let mut out = Vec::new();
    let (mut said, mut rebased) = (false, false);
    let mut k = 0;
    while k < diff.len() {
        if let Diffed::Patch { rebased: r, .. } = &diff[k] {
            rebased = *r;
        }
        let start = k;
        while matches!(diff.get(k), Some(Diffed::Code(_))) {
            k += 1;
        }
        if k == start {
            out.push((vec![plain.get(k).cloned().unwrap_or_else(|| vec![lines[k].clone()])], vec![k]));
            k += 1;
            continue;
        }
        let two = Two { old, new, one_base: !rebased };
        let code: Vec<&InterLine> = diff[start..k].iter().filter_map(|d| if let Diffed::Code(l) = d { Some(l) } else { None }).collect();
        let at = |stops: Vec<Stop>| -> Vec<Stop> { stops.into_iter().map(|(drawn, shows)| (drawn, shows.into_iter().map(|j| start + j).collect())).collect() };
        match v {
            InterLayout::Interdiff => {
                if !std::mem::replace(&mut said, true) {
                    out.push(said_stop(&interdiff_said(&two), block, indent));
                }
                out.extend((start..k).map(|j| (vec![plain[j].clone()], vec![j])));
            }
            InterLayout::Columns => {
                if !std::mem::replace(&mut said, true) {
                    out.push(said_stop(&columns_said(&two), block, indent));
                }
                out.extend(at(columns(&code, two.one_base, indent)));
            }
            InterLayout::Sides => out.extend(at(side_by_side(&code, (old, new), block, indent))),
        }
    }
    out
}

/// Of lines drawn, each with the lines of what is drawn that its stop of
/// the cursor shows, `at` (`Stop`): the first that shows line `line`, else
/// the first that shows one past it; none, the last.
pub(super) fn line_at(at: &[Vec<usize>], line: usize) -> usize {
    at.iter()
        .position(|a| a.contains(&line))
        .or_else(|| at.iter().position(|a| a.iter().any(|&l| l > line)))
        .unwrap_or(at.len().saturating_sub(1))
}

/// The line of what is drawn that line `i` of those drawn is at, `at`
/// saying what each shows (`line_at`): the first it shows, the old
/// version's of two; none, a heading's, the next one's. Kept, `kept`, the
/// line that a switch of view found line `i` at, which it shows too: so
/// that switching back and forth, the cursor on the new version's line of
/// two goes back to that line.
pub(super) fn line_of(at: &[Vec<usize>], i: usize, kept: Option<(usize, usize)>) -> Option<usize> {
    match kept {
        Some((k, line)) if k == i && at.get(i).is_some_and(|a| a.contains(&line)) => Some(line),
        _ => at.get(i..)?.iter().find_map(|a| a.first().copied()),
    }
}

impl App {
    /// The view a pane `w` wide draws its interdiffs in: the one picked,
    /// but as interdiffs, side by side picked, while the pane is too narrow
    /// for two sides (`view.split_width`, as the code view's side by side
    /// needs).
    pub(super) fn inter_in(&self, w: usize) -> InterLayout {
        match self.inter {
            InterLayout::Sides if w < self.cfg.view.split_width => InterLayout::Interdiff,
            v => v,
        }
    }

    /// Whether the screen shows interdiffs, for 1 to pick their view: the
    /// versions of a patch, History…, the review of what landed while its
    /// right pane shows the stack diff of its change, and what differs from
    /// Phabricator or what an edit changed, on a screen of its own.
    pub(super) fn inter_screen(&self) -> bool {
        match self.screen {
            Screen::Versions | Screen::History => true,
            Screen::Review => self.review_on_diff(),
            Screen::Text => self.text.as_ref().is_some_and(|t| t.inter.is_some()),
            _ => false,
        }
    }

    /// The menu of 1 on an interdiff's screen, in sections, each what it
    /// draws under a heading, then the views it is drawn in: the interdiff,
    /// in each of its views, each saying how it reads, the one picked
    /// marked and picked.
    pub(super) fn inter_menu(&self) -> Menu {
        let mut items = vec![Item::heading(INTERDIFF_SECTION)];
        items.extend(InterLayout::ALL.iter().map(|&v| {
            let mark = if v == self.inter { "● " } else { "  " };
            Item::new(format!("{mark}{}", v.item()), Action::InterView(v))
        }));
        let mut m = Menu::new(INTER_MENU, items, None);
        m.sel = m.items.iter().position(|i| i.action == Action::InterView(self.inter)).unwrap_or(0);
        m
    }

    /// Whether the pane the screen draws its interdiffs in, as last drawn,
    /// is too narrow for them side by side; never drawn, it is not.
    pub(super) fn inter_narrow(&self) -> bool {
        let w = self.inter_w.get();
        w > 0 && w < self.cfg.view.split_width
    }

    /// View `v`, picked in the menu of 1: every interdiff drawn so from now
    /// on, while the editor is open, on every screen; this one keeping its
    /// place, the line its cursor is on, else its top line, on its line of
    /// the screen. The status line says how the view reads, or, side by
    /// side on a pane too narrow for it, that this pane draws its
    /// interdiffs as interdiffs until it has the room.
    pub(super) fn pick_inter(&mut self, v: InterLayout) {
        // History…'s stack diff: on the line its cursor was on, found again
        // once it is drawn in the view.
        if self.screen == Screen::History
            && let Some(line) = line_of(&self.hist_of, self.hist_cursor, self.hist_line)
        {
            self.hist_keep = Some((line, self.hist_cursor.saturating_sub(self.hist_top)));
        }
        let was = self.review_diff_scroll();
        self.inter = v;
        self.hist_built = None;
        self.review_scroll_kept(was);
        if self.screen == Screen::Text {
            self.text_again(self.body_w);
        }
        let need = self.cfg.view.split_width;
        self.status = match v {
            InterLayout::Interdiff => "each interdiff is drawn as an interdiff: a sign for which version has each line, then that version's own sign".into(),
            InterLayout::Columns => "each interdiff is drawn in a column per version: what each one's patch does with each line, the old version's first".into(),
            InterLayout::Sides if self.inter_narrow() => format!(
                "each interdiff is drawn side by side where its pane has {need} columns: this pane has {}, so its interdiffs are drawn as interdiffs until it is wider",
                self.inter_w.get()
            ),
            InterLayout::Sides => "each interdiff is drawn side by side: the old version on the left, the new one on the right, the lines both have dim".into(),
        };
    }

    /// What `?` says of 1 on an interdiff's screen; side by side, on a pane
    /// too narrow for it, why the interdiffs here are drawn as interdiffs.
    /// Nothing elsewhere.
    pub(super) fn inter_help(&self) -> Vec<String> {
        if !self.inter_screen() {
            return vec![];
        }
        let mut out = vec![
            "Press 1 to pick how the interdiffs are drawn: as interdiffs, a sign for which version has each line before that version's own sign; in a column per version, what each one's patch does with each line; or side by side, the old version on the left and the new one on the right, the lines both have dim.".to_string(),
        ];
        if self.inter == InterLayout::Sides && self.inter_narrow() {
            out.push(format!(
                "This pane is under {} columns, too narrow for side by side: its interdiffs are drawn as interdiffs until the pane has room.",
                self.cfg.view.split_width
            ));
        }
        out
    }

    /// What the top bar says of the interdiffs on their screens, drawn
    /// otherwise than as interdiffs: in columns, or side by side, as the
    /// code view's side by side says it, too narrow so.
    pub(super) fn inter_bar(&self) -> String {
        match (self.inter, self.inter_screen()) {
            (InterLayout::Columns, true) => " · interdiffs in columns".into(),
            (InterLayout::Sides, true) if self.inter_narrow() => " · interdiffs side-by-side: too narrow".into(),
            (InterLayout::Sides, true) => " · interdiffs side-by-side".into(),
            _ => String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An interdiff's lines from their texts, each of the diffs once past
    /// the line `---`: a hunk's header, a file's, or a line of a hunk, as
    /// `ops` marks them.
    fn lines(texts: &[&str]) -> Vec<InterLine> {
        let past = texts.iter().position(|t| *t == " ---").unwrap_or(texts.len());
        let part = |k: usize, t: &str| match &t[1..] {
            _ if k <= past || t.starts_with("@@") => Part::Mail,
            l if l.starts_with("@@") => Part::Hunk,
            l if ["diff --git", "index ", "--- ", "+++ "].iter().any(|h| l.starts_with(h)) => Part::Head,
            _ => Part::Line,
        };
        texts.iter().enumerate().map(|(k, t)| InterLine { part: part(k, t), ..InterLine::from(*t) }).collect()
    }

    /// The text of each line drawn, the spaces at its end left out.
    fn said(stops: &[Stop]) -> Vec<String> {
        stops.iter().flat_map(|(ls, _)| ls.iter().map(|l| l.iter().map(|(t, _)| t.as_str()).collect::<String>().trim_end().to_string())).collect()
    }

    /// A line side by side, as `said` reads it: `left` in its half, `half`
    /// wide, after `indent` columns, a column, the line between the
    /// halves, then `right`.
    fn beside(indent: usize, left: &str, half: usize, right: &str) -> String {
        format!("{}{left:half$} │{right}", " ".repeat(indent)).trim_end().to_string()
    }

    /// The style the character `at` of line `parts` is drawn in.
    fn style_at(parts: &Parts, at: usize) -> Style {
        let mut n = 0;
        let cell = parts.iter().find(|(t, _)| {
            n += t.chars().count();
            n > at
        });
        cell.map(|(_, st)| *st).unwrap()
    }

    #[test]
    fn side_by_side_pairs_the_interdiff_s_lines_as_the_code_view_pairs_a_diff_s() {
        let all = lines(&[
            "@@ -1,9 +1,9 @@",
            " From: A <a@b>",
            "-Subject: Bug 1 - old",
            "+Subject: Bug 1 - new",
            " ---",
            " @@ -1,3 +1,3 @@",
            "  a",
            "--b",
            "-+B",
            "++Bee",
            "+ c",
        ]);
        let refs: Vec<&InterLine> = all.iter().collect();
        // 53 wide: each half 25, a column and the line between them.
        let stops = side_by_side(&refs, ("old", "new"), 53, 0);
        let row = |left: &str, right: &str| beside(0, left, 25, right);
        // A heading; the hunk's header across; a line both have on both
        // sides of its row; a run taken out beside the run put in, row by
        // row, each line as its own version has it, its own sign once.
        assert_eq!(
            said(&stops),
            [
                row("old", "new"),
                "@@ -1,9 +1,9 @@".to_string(),
                row("From: A <a@b>", "From: A <a@b>"),
                row("Subject: Bug 1 - old", "Subject: Bug 1 - new"),
                row("---", "---"),
                row("@@ -1,3 +1,3 @@", "@@ -1,3 +1,3 @@"),
                row(" a", " a"),
                row("-b", "+Bee"),
                row("+B", " c"),
            ]
        );
        // Each stop with the lines it shows: none for the heading, both of a
        // pair.
        let shows: Vec<Vec<usize>> = stops.iter().map(|(_, s)| s.clone()).collect();
        assert_eq!(shows, [vec![], vec![0], vec![1], vec![2, 3], vec![4], vec![5], vec![6], vec![7, 9], vec![8, 10]]);
        // A line only one version has: alone in its row, `~` dim on the
        // other side.
        let all = lines(&["@@ -1 +1,2 @@", " ---", "  a", "++b"]);
        let stops = side_by_side(&all.iter().collect::<Vec<_>>(), ("old", "new"), 53, 0);
        assert_eq!(said(&stops)[4..], [row("~", "+b")]);
        assert!(stops[4].0[0].contains(&(fit("~", 25), color::dim())), "{:?}", stops[4]);
    }

    #[test]
    fn side_by_side_each_cell_is_drawn_as_its_version_s_diff_draws_it_the_lines_both_have_dim() {
        let mut all = lines(&["@@ -5,4 +5,4 @@", " ---", "  a", " - x = 1;", "-+ x = 2;", "++ x = 3;"]);
        // What changed in each line put in or taken out, as its own
        // version's diff pairs it with the line removed before it: counted
        // from the interdiff's sign.
        all[4].words = vec![(7, 8)];
        all[5].words = vec![(7, 8)];
        let refs: Vec<&InterLine> = all.iter().collect();
        // Halves of 20: the old one from column 0, the new one from 22.
        let stops = side_by_side(&refs, ("old", "new"), 43, 0);
        let row = |k: usize| stops[k].0[0].clone();
        // A line both have: in its own colour, dim, on both sides.
        let shared = row(4);
        assert_eq!((style_at(&shared, 0), style_at(&shared, 22)), (color::del().add_modifier(Modifier::DIM), color::del().add_modifier(Modifier::DIM)));
        // Lines that differ: each in its own sign's colour, with no sign of
        // the interdiff, what its own diff changed in the colour for words.
        let differs = row(5);
        let text: String = differs.iter().map(|(t, _)| t.as_str()).collect();
        assert_eq!(text.trim_end(), beside(0, "+ x = 2;", 20, "+ x = 3;"));
        assert_eq!((style_at(&differs, 0), style_at(&differs, 6)), (color::add(), color::add_word()));
        assert_eq!((style_at(&differs, 22), style_at(&differs, 28)), (color::add(), color::add_word()));
        // The headings in the colour of a file's name; the line between the
        // halves in the borders'.
        let heading = row(0);
        assert_eq!((style_at(&heading, 0), style_at(&heading, 21), style_at(&heading, 22)), (color::file(), color::meta(), color::file()));
    }

    #[test]
    fn side_by_side_a_line_of_the_message_is_compared_across_as_the_interdiff_compares_it() {
        let mut all = lines(&["@@ -1,3 +1,3 @@", " Subject: Bug 1", " ", "-The parser reads the whole file", "+The parser reads the entire file", " ---"]);
        // What only each version has, as `messages` marks it.
        all[3].own = Some((vec![(22, 27)], false));
        all[4].own = Some((vec![(22, 28)], false));
        let stops = side_by_side(&all.iter().collect::<Vec<_>>(), ("old", "new"), 83, 0);
        let parts = &stops[4].0[0];
        let plain = Style::new();
        // Each side its version's line, the words only it has in the colours
        // for words, the rest as any text.
        assert!(parts.contains(&("whole".to_string(), color::del_word())) && parts.contains(&("entire".to_string(), color::add_word())), "{parts:?}");
        assert!(parts.contains(&("The parser reads the ".to_string(), plain)), "{parts:?}");
        assert_eq!(said(&stops)[4], beside(0, "The parser reads the whole file", 40, "The parser reads the entire file"));
    }

    #[test]
    fn side_by_side_a_line_too_wide_folds_in_its_half() {
        let all = lines(&["@@ -1 +1 @@", " ---", "-+let total = price * quantity * rate;", "++let total = price * quantity;"]);
        let refs: Vec<&InterLine> = all.iter().collect();
        // Halves of 20, each line folded at 19: code where the half ends,
        // `↪` before what comes next; the pieces of one beside the other's.
        let stops = side_by_side(&refs, ("old", "new"), 43, 2);
        assert_eq!(
            said(&stops)[3..],
            [beside(2, "+let total = price ", 20, "+let total = price "), beside(2, "↪* quantity * rate;", 20, "↪* quantity;")]
        );
        assert_eq!(stops[3].1, [2, 3], "one stop, both pieces");
        // A line of the message, between words, a bullet's pieces under
        // what follows the bullet.
        let all = lines(&["@@ -1 +1 @@", "-- the parser skips blank lines", "+- the parser skips empty lines", " ---"]);
        let stops = side_by_side(&all.iter().collect::<Vec<_>>(), ("old", "new"), 43, 0);
        assert_eq!(said(&stops)[2..4], [beside(0, "- the parser skips", 20, "- the parser skips"), beside(0, "  blank lines", 20, "  empty lines")]);
    }

    /// The interdiff the next tests draw in a column per version: the title
    /// reworded; a hunk moved down a line; a line both remove; one each
    /// adds in place of the other's; one only the old version's diff shows,
    /// which the new one removes; and one only the old version's diff
    /// shows, which the new one's does not.
    fn drifted() -> Vec<InterLine> {
        lines(&[
            "@@ -1,14 +1,14 @@",
            " From: A <a@b>",
            "-Subject: Bug 1 - old",
            "+Subject: Bug 1 - new",
            " ---",
            "-@@ -1,4 +1,4 @@ fn f()",
            "+@@ -2,4 +2,4 @@ fn f()",
            "  function f() {",
            " -  const a = 1;",
            "-+  const a = 2;",
            "-   x();",
            "-   z();",
            "++  const a = 3;",
            "+-  x();",
        ])
    }

    #[test]
    fn in_a_column_per_version_each_line_says_what_each_version_s_patch_does_with_it() {
        let all = drifted();
        let refs: Vec<&InterLine> = all.iter().collect();
        let stops = columns(&refs, true, 0);
        // The interdiff's hunk's header across; a header both have, `=` in
        // each column; the title each has alone, `·` in the other's; the
        // hunk's headers that differ in their numbers alone, one line with
        // both ranges; a line both leave, both remove; one each adds alone;
        // the line the old version keeps and the new one removes, one line;
        // and one the old version's diff alone shows, on one base, a line
        // the new one leaves as it is.
        assert_eq!(
            said(&stops),
            [
                "@@ -1,14 +1,14 @@",
                "==From: A <a@b>",
                "=·Subject: Bug 1 - old",
                "·=Subject: Bug 1 - new",
                "==---",
                "==@@ -1,4 +1,4 → -2,4 +2,4 @@ fn f()",
                "  function f() {",
                "--  const a = 1;",
                "+·  const a = 2;",
                "·+  const a = 3;",
                " -  x();",
                "    z();",
            ]
        );
        let shows: Vec<Vec<usize>> = stops.iter().map(|(_, s)| s.clone()).collect();
        assert_eq!(shows, [vec![0], vec![1], vec![2], vec![3], vec![4], vec![5, 6], vec![7], vec![8], vec![9], vec![12], vec![10, 13], vec![11]]);
        // On two bases, a line the old version's diff alone shows is one the
        // new one is not known to have.
        assert_eq!(said(&columns(&refs, false, 0))[11], " ·  z();");
    }

    #[test]
    fn in_a_column_per_version_on_two_bases_a_line_one_adds_and_the_other_s_base_has_is_one_line() {
        // A line the old version adds, which the new one's base has, as when
        // it went into a patch below; and one the old version's base has,
        // which the new one adds, as when it came out of it.
        let all = lines(&[" ---", "  a", "-+b", "- c", "+ b", "++c", "  d"]);
        let refs: Vec<&InterLine> = all.iter().collect();
        // On two bases, each one line: what each version's patch does with
        // it, in its own colour.
        let stops = columns(&refs, false, 0);
        assert_eq!(said(&stops), ["==---", "  a", "+ b", " +c", "  d"]);
        let shows: Vec<Vec<usize>> = stops.iter().map(|(_, s)| s.clone()).collect();
        assert_eq!(shows, [vec![0], vec![1], vec![2, 4], vec![3, 5], vec![6]]);
        assert_eq!((style_at(&stops[2].0[0], 0), style_at(&stops[2].0[0], 1), style_at(&stops[3].0[0], 1)), (color::add(), Style::new(), color::add()));
        // On one base, a line a patch adds is another than the line of the
        // base: each alone, a line of the base both leave as it is.
        assert_eq!(said(&columns(&refs, true, 0)), ["==---", "  a", "+·b", "  c", "  b", "·+c", "  d"]);
    }

    #[test]
    fn in_a_column_per_version_what_both_do_alike_is_dim_and_a_line_each_adds_in_place_of_the_other_s_in_the_colours_for_words() {
        let all = drifted();
        let stops = columns(&all.iter().collect::<Vec<_>>(), true, 2);
        let row = |k: usize| stops[k].0[0].clone();
        let dim = |st: Style| st.add_modifier(Modifier::DIM);
        // After the indent, the two columns, then the line: a line both
        // remove, dim, in the colour of a line removed.
        let removed = row(7);
        assert_eq!((style_at(&removed, 2), style_at(&removed, 3), style_at(&removed, 6)), (dim(color::del()), dim(color::del()), dim(color::del())));
        // The line each adds alone: its column in the colour of a line
        // added, the other's `·` dim, the line as any text, what changed in
        // each in the colours for words.
        let (old, new) = (row(8), row(9));
        assert_eq!((style_at(&old, 2), style_at(&old, 3), style_at(&old, 4)), (color::add(), color::dim(), Style::new()));
        assert_eq!((style_at(&old, 16), style_at(&new, 16)), (color::del_word(), color::add_word()));
        // A line the two do otherwise: each sign in its own colour.
        let otherwise = row(10);
        assert_eq!((style_at(&otherwise, 2), style_at(&otherwise, 3), style_at(&otherwise, 6)), (Style::new(), color::del(), Style::new()));
        // The headers' marks: `=` as any text, dim where both have the line.
        assert_eq!((style_at(&row(1), 2), style_at(&row(2), 2), style_at(&row(2), 3)), (dim(Style::new()), Style::new(), color::dim()));
    }

    #[test]
    fn one_interdiff_says_how_to_read_it_once_at_the_top_of_its_code_naming_its_versions() {
        let mut all = lines(&["--- 1234abcd", "+++ 5678ef01", "@@ -5,4 +5,4 @@", " ---", "  a", "-+b", "++B"]);
        all[0].part = Part::Mail;
        let refs: Vec<&InterLine> = all.iter().collect();
        let two = Two { old: "v2", new: "v3", one_base: true };
        // As an interdiff: what the two signs say, then the lines as ever.
        let stops = interdiff_stops(&refs, InterLayout::Interdiff, &two, 400, 0);
        assert_eq!(said(&stops[..1]), [interdiff_said(&two)]);
        assert!(interdiff_said(&two).contains("- for v2 alone, + for v3 alone"), "{}", interdiff_said(&two));
        assert_eq!(said(&stops[1..]), ["--- 1234abcd", "+++ 5678ef01", "@@ -5,4 +5,4 @@", " ---", "  a", "-+b", "++B"]);
        // In a column per version: what each column is, the lines naming
        // the versions left out.
        let stops = interdiff_stops(&refs, InterLayout::Columns, &two, 400, 0);
        assert_eq!(said(&stops[..1]), [columns_said(&two)]);
        assert!(columns_said(&two).starts_with("The first column is v2, the second v3."), "{}", columns_said(&two));
        assert_eq!(said(&stops[1..3]), ["@@ -5,4 +5,4 @@", "==---"]);
        assert_eq!(stops[1].1, [2], "each line by its place among them all");
        // Side by side: a heading of the two instead.
        let stops = interdiff_stops(&refs, InterLayout::Sides, &two, 43, 0);
        assert_eq!(said(&stops[..1]), [beside(0, "v2", 20, "v3")]);
        // A stack diff: said once, over its first interdiff, the patches'
        // lines as ever.
        let patch = |at: usize| Diffed::Patch { at, title: format!("Bug {at}"), added: false, changed: true, reworded: false, rebased: false, was: None };
        let mut diff = vec![patch(1)];
        diff.extend(all[2..].iter().cloned().map(Diffed::Code));
        diff.push(patch(2));
        diff.extend(all[2..].iter().cloned().map(Diffed::Code));
        for (v, said_once) in [(InterLayout::Interdiff, interdiff_said(&two)), (InterLayout::Columns, columns_said(&two))] {
            let stops = stack_stops(&diff, v, ("v2", "v3"), 400);
            let lines = said(&stops);
            assert_eq!(lines.iter().filter(|l| l.trim() == said_once).count(), 1, "{lines:#?}");
            assert_eq!(lines[1].trim(), said_once, "{lines:#?}");
            assert_eq!((lines[0].trim(), stops[1].1.len()), ("1 ~ Bug 1", 0));
        }
        let stops = stack_stops(&diff, InterLayout::Sides, ("v2", "v3"), 400);
        assert_eq!(said(&stops).iter().filter(|l| l.trim().starts_with("v2 ")).count(), 2, "a heading over each");
    }

    #[test]
    fn of_the_lines_drawn_the_one_showing_a_line_is_found_again() {
        // A heading, a line, two side by side over two pieces, a line.
        let at = [vec![], vec![0], vec![2, 4], vec![2, 4], vec![5]];
        assert_eq!((line_at(&at, 4), line_at(&at, 3), line_at(&at, 9)), (2, 2, 4));
        // The line of each: the old version's of two, a heading the next
        // one's; but the one a switch found there, kept.
        assert_eq!((line_of(&at, 0, None), line_of(&at, 3, None)), (Some(0), Some(2)));
        assert_eq!((line_of(&at, 3, Some((3, 4))), line_of(&at, 2, Some((3, 4)))), (Some(4), Some(2)));
    }
}
