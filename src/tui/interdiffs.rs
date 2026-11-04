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
//! one of the message between words. Or the change from the old version
//! to the new one as a patch of its own, in the views of the code
//! (`change.rs`). The dialog of 1 (`views_dialog.rs`) sets what is
//! compared, the two diffs or the code, the view it is drawn in, and the
//! lines each view draws: every line, or only what the patches changed
//! (`kept_lines`, `change::changed_rows`), each place lines are left out a
//! line saying how many (`left_out`). The view set, and the lines, hold for
//! every interdiff while the editor is open, the settings'
//! `view.interdiff` and `view.interdiff_lines` those they start with; a
//! pane narrower than two sides need (`view.split_width`, as the code
//! view's) draws its interdiffs as interdiffs meanwhile, the change
//! unified, and says so. Switching from one view to another, or to other
//! lines, a screen keeps its place (`At`).

use super::change::{Changed, Near, change_said, code_stops, code_view, message_stops};
use super::*;
use crate::config::InterLines;
use crate::ops::{InterLine, Part};
use crate::rows::FOLDED;

/// What the views of an interdiff say once, at the top of its code, while
/// they draw only what the patches changed: what is left out, and how the
/// places they were are marked (`kept_lines`, `left_out`).
pub(super) const LEFT_OUT: &str = "Only what the patches changed is drawn. The lines both versions have alike, the context lines of their diffs and their index lines are left out, and where lines are left out, a line says how many. Two hunk headers that differ only in their numbers are drawn once, as the new version has them.";

/// What an interdiff says in place of its lines, drawn with only what the
/// patches changed, when none of them is kept (`kept_lines`).
pub(super) const NOTHING_LEFT: &str = "The two versions differ only in context, line numbers or index lines, which are left out.";

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
/// said of them or a heading. A stop of the change as a patch of its own
/// shows the line of the interdiff nearest it (`change::Near`); and, as its
/// own, the lines of the change it shows, each by the place of its patch's
/// line among what is drawn and its line of the change's diff, the place of
/// its patch's line, and whether a line of it is taken out or put in.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct Stop {
    pub lines: Vec<Parts>,
    pub shows: Vec<usize>,
    pub rows: Vec<(usize, usize)>,
    pub slot: Option<usize>,
    pub changed: bool,
}

impl Stop {
    /// Lines `lines`, showing `shows`.
    pub(super) fn new(lines: Vec<Parts>, shows: Vec<usize>) -> Stop {
        Stop { lines, shows, ..Default::default() }
    }
}

/// What a line drawn of interdiffs shows (`Stop::shows`, `Stop::rows`), for
/// a screen to find its place again once it draws them otherwise.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct Seen {
    pub shows: Vec<usize>,
    pub rows: Vec<(usize, usize)>,
}

impl From<&Stop> for Seen {
    fn from(s: &Stop) -> Seen {
        Seen { shows: s.shows.clone(), rows: s.rows.clone() }
    }
}

/// Where a screen of interdiffs is, to be found again in another view of
/// them: the line of what is drawn its cursor's stop shows, the one it is
/// on of two; and on a line of the change as a patch of its own, that line
/// (`Stop::rows`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct At {
    pub line: usize,
    pub row: Option<(usize, usize)>,
}

/// What a view says once, at the top of an interdiff's code, of how to read
/// it, `said`, folded `w` wide after `indent` columns: a stop that shows no
/// line.
pub(super) fn said_stop(said: &str, w: usize, indent: usize) -> Stop {
    let lines = wrap(said, w.max(20)).into_iter().map(|l| vec![(" ".repeat(indent), Style::new()), (l, color::meta())]).collect();
    Stop::new(lines, vec![])
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
pub(super) fn cut(parts: &[(String, Style)], w: usize) -> (Parts, usize) {
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
pub(super) fn half_of(w: usize) -> usize {
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
            Some(s) if !first => {
                s.lines.push(parts);
                s.shows.extend(shows);
                s.shows.sort_unstable();
                s.shows.dedup();
            }
            _ => out.push(Stop::new(vec![parts], shows.collect())),
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
    let mut out: Vec<Stop> = vec![Stop::new(vec![heading(heads, w, indent)], vec![])];
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

/// What line `l` says after the interdiff's sign: its version's line.
fn after_sign(l: &InterLine) -> &str {
    l.text.get(1..).unwrap_or_default()
}

/// What line `l` of a version says after the interdiff's sign, and after
/// its own sign too, which its column says instead.
fn after_signs(l: &InterLine) -> &str {
    let at = if own_sign(l).is_some() { 2 } else { 1 };
    l.text.get(at..).unwrap_or_default()
}

/// Line `k` of `lines`, as a column per version matches it with the other
/// version's (`Key`), the two versions on one base or not (`one_base`).
fn key<'a>(lines: &[&'a InterLine], k: usize, one_base: bool) -> Key<'a> {
    let l = lines[k];
    let line = l.text.get(1..).unwrap_or_default();
    match (l.part, own_sign(l)) {
        (Part::Line, Some(c)) => Key::Line(one_base && c == '+', &line[1..]),
        (Part::Hunk, _) => Key::Hunk(ranges(line).map_or(line, |(_, rest)| rest)),
        (Part::Mail, _) => Key::Mail(k),
        _ => Key::Head(line),
    }
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
            out.push(Stop::new(vec![draw(cols, text, mine == theirs)], vec![i]));
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
        Stop::new(vec![draw(cols, text, agree)], vec![k, m])
    };
    let mut out: Vec<Stop> = Vec::new();
    let mut k = 0;
    while k < lines.len() {
        let l = lines[k];
        match l.text.chars().next() {
            // Both have it, and do the same with it.
            Some(' ') => {
                let col = own_sign(l).map_or(Col::Has, Col::Sign);
                out.push(Stop::new(vec![draw([col, col], agreeing(l), true)], vec![k]));
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
                let (a, b): (Vec<Key>, Vec<Key>) = (olds.iter().map(|&i| key(lines, i, one_base)).collect(), news.iter().map(|&i| key(lines, i, one_base)).collect());
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
                out.push(Stop::new(vec![vec![(" ".repeat(indent), Style::new()), (l.text.clone(), diff_style(&l.text))]], vec![k]));
                k += 1;
            }
        }
    }
    out
}

/// What git derives in a file's header, which only what the patches
/// changed leaves out though the two versions' differ: its index line, and
/// its similarity, as the screen of what differs from Phabricator leaves
/// them out of the diffs it compares (`revisions::bare`).
const DERIVED: [&str; 3] = ["index ", "similarity index ", "dissimilarity index "];

/// An interdiff's lines from its first hunk's header, as its views draw
/// them with only what the patches changed (`kept_lines`): its lines, but
/// two hunk's headers of the two versions that differ only in their
/// numbers, which are one line, the new version's, as a header both have
/// (` @@`), each version's lines before its own of the two over it, and
/// those after under it; of each, the lines of the interdiff it stands
/// for, by their places there, one or the two headers; whether each is
/// kept; and the run of lines each version has alone it is in, as drawn,
/// none for a line both have, or the interdiff's own.
pub(super) struct Kept {
    pub lines: Vec<InterLine>,
    pub of: Vec<Vec<usize>>,
    pub kept: Vec<bool>,
    pub runs: Vec<Option<usize>>,
}

/// What of interdiff `lines`, from its first hunk's header, its views draw
/// with only what the patches changed (`Kept`), its versions on one base or
/// not (`one_base`). Kept: a line of the two diffs that one version adds or
/// removes and the other does not, its own sign `+` or `-`; git's `\` line
/// that one version alone has, and the line of that version it is about;
/// a line of a file's header that differs, but what git derives
/// (`DERIVED`), as when a file comes or goes; a line of the mail around the
/// diffs that differs, the messages compared; and the headers one of those
/// is under in its version's diff (`InterLine::under`), its hunk's and the
/// line naming its file, its `diff --git`, where the interdiff has them,
/// though it leaves out lines between them, a header both versions have
/// for a line of either. Two hunk's headers of a run of lines the versions
/// have each alone that differ only in their numbers, which a column per
/// version matches as one line (`Key::Hunk`), are one, the new version's,
/// kept so too. Left out: the lines both versions have alike, the
/// interdiff's own context; the context lines of the two diffs, whichever
/// version has them; what git derives; the interdiff's own hunks' headers;
/// and a hunk's header, or a file's, with nothing kept under it.
pub(super) fn kept_lines(lines: &[&InterLine], one_base: bool) -> Kept {
    // The lines in the order drawn, each the lines of the interdiff it
    // stands for: a run of lines each version has alone cut at each two
    // hunk's headers that are one, the lines of each version before its
    // own over the one, in their order, and those after under it.
    let mut order: Vec<Vec<usize>> = Vec::new();
    let mut k = 0;
    while k < lines.len() {
        if !lines[k].text.starts_with(['-', '+']) {
            order.push(vec![k]);
            k += 1;
            continue;
        }
        let start = k;
        while lines.get(k).is_some_and(|l| l.text.starts_with(['-', '+'])) {
            k += 1;
        }
        let olds: Vec<usize> = (start..k).filter(|&i| lines[i].text.starts_with('-')).collect();
        let news: Vec<usize> = (start..k).filter(|&i| lines[i].text.starts_with('+')).collect();
        let (a, b): (Vec<Key>, Vec<Key>) = (olds.iter().map(|&i| key(lines, i, one_base)).collect(), news.iter().map(|&i| key(lines, i, one_base)).collect());
        let mut pairs: Vec<(usize, usize)> = Vec::new();
        for op in similar::capture_diff_slices(similar::Algorithm::Myers, &a, &b) {
            let (tag, x, y) = op.as_tag_tuple();
            if tag == similar::DiffTag::Equal {
                pairs.extend(x.zip(y).filter(|&(i, _)| matches!(a[i], Key::Hunk(_))).map(|(i, j)| (olds[i], news[j])));
            }
        }
        // The first line of each version not drawn yet.
        let mut from = (start, start);
        for (o, n) in pairs.into_iter().chain([(k, k)]) {
            let before = |i: &usize| match lines[*i].text.starts_with('-') {
                true => (from.0..o).contains(i),
                false => (from.1..n).contains(i),
            };
            order.extend((start..k).filter(before).map(|i| vec![i]));
            if o < k {
                order.push(vec![o, n]);
            }
            from = (o + 1, n + 1);
        }
    }
    let drawn: Vec<InterLine> = order
        .iter()
        .map(|of| match of.as_slice() {
            &[_, n] => InterLine { text: format!(" {}", &lines[n].text[1..]), part: Part::Hunk, ..Default::default() },
            _ => lines[of[0]].clone(),
        })
        .collect();
    // The versions a line is of, by its sign: both, the old one, the new
    // one; none, the interdiff's own.
    let versions = |l: &InterLine| -> &'static [usize] {
        match l.text.chars().next() {
            Some(' ') => &[0, 1],
            Some('-') => &[0],
            Some('+') => &[1],
            _ => &[],
        }
    };
    let mut kept = vec![false; drawn.len()];
    // Each `\` line both versions have, or the interdiff's own, with the
    // lines it is about: kept with them.
    let mut about: Vec<(usize, Vec<usize>)> = Vec::new();
    let mut last: [Option<usize>; 2] = [None, None];
    for (e, l) in drawn.iter().enumerate() {
        let (of, line, differs) = (versions(l), after_sign(l), l.text.starts_with(['-', '+']));
        match l.part {
            Part::Line if line.starts_with('\\') => {
                let on: Vec<usize> = of.iter().filter_map(|&v| last[v]).collect();
                match differs {
                    true => {
                        kept[e] = true;
                        for a in on {
                            kept[a] = true;
                        }
                    }
                    false => about.push((e, on)),
                }
            }
            Part::Line => {
                kept[e] = differs && line.starts_with(['+', '-']);
                for &v in of {
                    last[v] = Some(e);
                }
            }
            Part::Head => kept[e] = differs && !DERIVED.iter().any(|d| line.starts_with(d)),
            Part::Hunk => {}
            Part::Mail if l.text.starts_with('\\') => about.push((e, e.checked_sub(1).into_iter().collect())),
            Part::Mail => kept[e] = differs,
        }
    }
    for (e, on) in about {
        kept[e] = on.iter().any(|&a| kept[a]);
    }
    // The headers of the lines kept: each line drawn that is a header of a
    // version's diff, by the version and its line there; then of each line
    // kept, those it is under there, where they are drawn.
    let mut heads: HashMap<(usize, usize), usize> = HashMap::new();
    for (e, of) in order.iter().enumerate() {
        for &i in of {
            let l = lines[i];
            if l.part == Part::Hunk || (l.part == Part::Head && after_sign(l).starts_with("diff --git ")) {
                for (v, u) in l.under.iter().enumerate() {
                    if let Some(u) = u {
                        heads.insert((v, u.at), e);
                    }
                }
            }
        }
    }
    let mut need = vec![false; drawn.len()];
    for (e, of) in order.iter().enumerate() {
        if !kept[e] || !matches!(drawn[e].part, Part::Line | Part::Head) {
            continue;
        }
        for (v, u) in of.iter().flat_map(|&i| lines[i].under.iter().enumerate()) {
            for h in u.iter().flat_map(|u| [u.file, u.hunk]).flatten() {
                if let Some(&at) = heads.get(&(v, h)) {
                    need[at] = true;
                }
            }
        }
    }
    for (e, needed) in need.into_iter().enumerate() {
        kept[e] |= needed;
    }
    let mut runs = Vec::with_capacity(drawn.len());
    let mut n = 0;
    for (e, l) in drawn.iter().enumerate() {
        let alone = l.text.starts_with(['-', '+']);
        if alone && (e == 0 || !drawn[e - 1].text.starts_with(['-', '+'])) {
            n += 1;
        }
        runs.push(alone.then_some(n));
    }
    Kept { lines: drawn, of: order, kept, runs }
}

/// Lines `n` left out of an interdiff, or of the change, drawn with only
/// what the patches changed, as the line marking where they were says it.
pub(super) fn left_out_said(n: usize) -> String {
    match n {
        1 => "⋯ 1 line is left out here.".into(),
        n => format!("⋯ {} lines are left out here.", crate::paint::thousands(n)),
    }
}

/// Where `n` lines are left out, drawn with only what the patches changed:
/// a line saying so (`left_out_said`), dim, after `indent` columns; a stop
/// that shows no line.
pub(super) fn left_out(n: usize, indent: usize) -> Stop {
    Stop::new(vec![vec![(" ".repeat(indent), Style::new()), (left_out_said(n), color::dim())]], vec![])
}

/// Stops `stops` drawn of lines `k` (`kept_lines`), with only what the
/// patches changed: each that shows lines, none of them kept, left out,
/// each run of those one line saying how many (`left_out`), after `indent`
/// columns; of the stops of a run of lines each version has alone
/// (`Kept::runs`), those it keeps together, under the line saying how many
/// of its others are left out, with those before it. Each kept showing the
/// lines of the interdiff its own stand for, by their places among what is
/// drawn, the first at `first`. A stop that shows no line, a heading or
/// what is said, kept.
fn kept_stops(stops: Vec<Stop>, k: &Kept, first: usize, indent: usize) -> Vec<Stop> {
    let keeps = |s: &Stop| s.shows.is_empty() || s.shows.iter().any(|&e| k.kept[e]);
    let run = |s: &Stop| s.shows.first().and_then(|&e| k.runs[e]);
    let shown = |s: Stop| Stop { shows: s.shows.iter().flat_map(|&e| k.of[e].iter().map(|i| first + i)).collect(), ..s };
    let mut out = Vec::new();
    let mut gone = 0;
    let mut stops = stops.into_iter().peekable();
    while let Some(s) = stops.next() {
        let mut all = vec![s];
        if let Some(r) = run(&all[0]) {
            while let Some(next) = stops.next_if(|t| run(t) == Some(r)) {
                all.push(next);
            }
        }
        let (kept, left): (Vec<Stop>, Vec<Stop>) = all.into_iter().partition(keeps);
        gone += left.len();
        if !kept.is_empty() && gone > 0 {
            out.push(left_out(std::mem::take(&mut gone), indent));
        }
        out.extend(kept.into_iter().map(shown));
    }
    if gone > 0 {
        out.push(left_out(gone, indent));
    }
    out
}

/// What view `v` of an interdiff of versions `two` says once, at the top of
/// its code, of how to read it, with only what the patches changed: as an
/// interdiff, and in a column per version, what it says ever, then what is
/// left out (`LEFT_OUT`); side by side, that alone.
fn said_left_out(v: InterLayout, two: &Two) -> String {
    match v {
        InterLayout::Columns => format!("{} {LEFT_OUT}", columns_said(two)),
        InterLayout::Sides => LEFT_OUT.into(),
        _ => format!("{} {LEFT_OUT}", interdiff_said(two)),
    }
}

/// One interdiff's stops in view `v` with only what the patches changed,
/// as `interdiff_stops` draws it, `lines` its lines, the first `names` of
/// them naming the versions: what the view says of how to read it and what
/// is left out (`said_left_out`); then, as an interdiff, the two lines
/// naming the versions, as ever, in a column per version none, and side by
/// side the heading standing for them; then the lines kept, each run of
/// those left out one line saying how many (`kept_stops`). With none of
/// its lines kept, what says so (`NOTHING_LEFT`) alone.
fn only_changed_stops(lines: &[&InterLine], names: usize, v: InterLayout, two: &Two, w: usize, indent: usize) -> Vec<Stop> {
    let k = kept_lines(&lines[names..], two.one_base);
    if !k.kept.contains(&true) {
        return vec![said_stop(NOTHING_LEFT, w, indent)];
    }
    let refs: Vec<&InterLine> = k.lines.iter().collect();
    let lead = |parts: Parts| -> Parts { std::iter::once((" ".repeat(indent), Style::new())).chain(parts).collect() };
    let mut out = vec![said_stop(&said_left_out(v, two), w, indent)];
    let stops = match v {
        InterLayout::Columns => columns(&refs, two.one_base, indent),
        InterLayout::Sides => {
            let mut sides = side_by_side(&refs, (two.old, two.new), w, indent);
            // The heading stands for the two lines it names.
            if !sides.is_empty() {
                out.push(Stop { shows: (0..names).collect(), ..sides.remove(0) });
            }
            sides
        }
        _ => {
            for (j, parts) in inter_parts(&lines[..names]).into_iter().enumerate() {
                out.push(Stop::new(vec![lead(parts)], vec![j]));
            }
            inter_parts(&refs).into_iter().enumerate().map(|(e, parts)| Stop::new(vec![lead(parts)], vec![e])).collect()
        }
    };
    out.extend(kept_stops(stops, &k, names, indent));
    out
}

/// One interdiff's stops in view `v`, `lines` as `ops` gives them, the two
/// lines naming the versions over its first hunk first, `w` wide with
/// `indent` columns before each line, of versions `two`: as an interdiff,
/// under what says how to read it, each line as `inter_parts` draws it; in
/// a column per version, under what says what each column is, the two
/// lines naming the versions left out (`columns`); side by side, those left
/// out too, each half under a heading of its version (`side_by_side`). In
/// a view of the change, as an interdiff: `view_stops` draws the change.
/// With `only`, only what the patches changed (`only_changed_stops`). Each
/// stop with the lines of `lines` it shows.
pub(super) fn interdiff_stops(lines: &[&InterLine], v: InterLayout, two: &Two, only: bool, w: usize, indent: usize) -> Vec<Stop> {
    let names = lines.iter().take_while(|l| l.text.starts_with("--- ") || l.text.starts_with("+++ ")).count();
    let shifted = |stops: Vec<Stop>| -> Vec<Stop> { stops.into_iter().map(|s| Stop { shows: s.shows.iter().map(|k| k + names).collect(), ..s }).collect() };
    let block = w.saturating_sub(indent);
    if only {
        return only_changed_stops(lines, names, v, two, block, indent);
    }
    match v {
        InterLayout::Columns => {
            let mut out = vec![said_stop(&columns_said(two), block, indent)];
            out.extend(shifted(columns(&lines[names..], two.one_base, indent)));
            out
        }
        // The heading stands for the two lines it names.
        InterLayout::Sides => {
            let mut out = shifted(side_by_side(&lines[names..], (two.old, two.new), block, indent));
            if let Some(s) = out.first_mut() {
                s.shows = (0..names).collect();
            }
            out
        }
        _ => {
            let mut out = vec![said_stop(&interdiff_said(two), block, indent)];
            for (k, parts) in inter_parts(lines).into_iter().enumerate() {
                let mut drawn = vec![(" ".repeat(indent), Style::new())];
                drawn.extend(parts);
                out.push(Stop::new(vec![drawn], vec![k]));
            }
            out
        }
    }
}

/// One interdiff's stops in view `v` (`interdiff_stops`); in a view of the
/// change, the change of its patch as `change` has it, with the lines of
/// the interdiff nearest each of its lines, each file whole with `whole`
/// (`change::one_change_stops`), and without it, as an interdiff. With
/// `only`, only what the patches changed, in either.
pub(super) fn view_stops(lines: &[&InterLine], v: InterLayout, two: &Two, change: Option<&(Changed, Near)>, (whole, only): (bool, bool), w: usize, indent: usize) -> Vec<Stop> {
    match change {
        Some((c, near)) if v.change() => super::change::one_change_stops(lines, v, two, (c, near), (whole, only), w, indent),
        _ => interdiff_stops(lines, v, two, only, w, indent),
    }
}

/// A stack diff's stops, as the screens draw it in view `v`, `w` wide: each
/// of its lines in parts, as `diff_parts` draws it, a stop of its own; but
/// its interdiffs, under the line of their patch, after the indent
/// `Diffed::text` gives them, in a column per version (`columns`) or side
/// by side (`side_by_side`, each under a heading of the two versions), a
/// stop for each line drawn of their versions. In a view of the change, the
/// lines of each interdiff comparing its patch's messages
/// (`change::message_stops`), then the change of its patch, as `changes`
/// has it by the place of the patch's line (`change::code_stops`), each
/// file whole with `whole`. As an interdiff, in a column per version and in
/// a view of the change, what says how to read them once, over the first;
/// what dim is, where a change read is on other bases. With `only`, only
/// what the patches changed, in each (`kept_lines`, `kept_stops`), what is
/// left out said once over the first too, side by side as well; an
/// interdiff with none of its lines kept saying so. The two versions named
/// `names`, each patch's on one base or not as its line says
/// (`Diffed::Patch::rebased`). Each stop with the lines of `diff` it shows.
pub(super) fn stack_stops(
    diff: &[Diffed],
    v: InterLayout,
    (old, new): (&str, &str),
    w: usize,
    changes: &HashMap<usize, (Changed, Near)>,
    (whole, only): (bool, bool),
) -> Vec<Stop> {
    let indent = 1 + Diffed::Code(Default::default()).text().chars().count();
    let block = w.saturating_sub(indent);
    let lines = diff_lines(diff);
    let plain = match v {
        InterLayout::Columns | InterLayout::Sides => vec![],
        _ => diff_parts(diff),
    };
    let rebased_change = changes.values().any(|(c, _)| matches!(c, Changed::Read(c) if c.rebased));
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
            out.push(Stop::new(vec![plain.get(k).cloned().unwrap_or_else(|| vec![lines[k].clone()])], vec![k]));
            k += 1;
            continue;
        }
        let two = Two { old, new, one_base: !rebased };
        let code: Vec<&InterLine> = diff[start..k].iter().filter_map(|d| if let Diffed::Code(l) = d { Some(l) } else { None }).collect();
        let at = |stops: Vec<Stop>| -> Vec<Stop> { stops.into_iter().map(|s| Stop { shows: s.shows.iter().map(|j| start + j).collect(), ..s }).collect() };
        // The interdiff in `view`, as an interdiff, in a column per version or
        // side by side; with only what the patches changed, its lines drawn
        // so, as an interdiff after the indent `diff_parts` gives them.
        let drawn = |view: InterLayout| -> Vec<Stop> {
            if only {
                let kept = kept_lines(&code, two.one_base);
                if !kept.kept.contains(&true) {
                    return vec![said_stop(NOTHING_LEFT, block, indent)];
                }
                let refs: Vec<&InterLine> = kept.lines.iter().collect();
                let stops = match view {
                    InterLayout::Columns => columns(&refs, two.one_base, indent),
                    InterLayout::Sides => side_by_side(&refs, (old, new), block, indent),
                    _ => inter_parts(&refs).into_iter().enumerate().map(|(e, parts)| Stop::new(vec![indented(parts, indent)], vec![e])).collect(),
                };
                return kept_stops(stops, &kept, start, indent);
            }
            match view {
                InterLayout::Columns => at(columns(&code, two.one_base, indent)),
                InterLayout::Sides => at(side_by_side(&code, (old, new), block, indent)),
                _ => (start..k).map(|j| Stop::new(vec![plain[j].clone()], vec![j])).collect(),
            }
        };
        match v {
            InterLayout::Interdiff | InterLayout::Columns | InterLayout::Sides => {
                let how = match v {
                    InterLayout::Interdiff => Some(interdiff_said(&two)),
                    InterLayout::Columns => Some(columns_said(&two)),
                    _ => None,
                };
                let say = match only {
                    true => Some(said_left_out(v, &two)),
                    false => how,
                };
                if let Some(say) = say
                    && !std::mem::replace(&mut said, true)
                {
                    out.push(said_stop(&say, block, indent));
                }
                out.extend(drawn(v));
            }
            _ => {
                let how = code_view(v);
                if !std::mem::replace(&mut said, true) {
                    out.push(said_stop(&change_said(&two, how, rebased_change, only), block, indent));
                }
                // The change of the patch whose line is over its code.
                match start.checked_sub(1).and_then(|p| changes.get(&p).map(|c| (p, c))) {
                    Some((p, (c, near))) => {
                        if matches!(c, Changed::Read(_) | Changed::Reading) {
                            out.extend(message_stops(&code, start, indent));
                        }
                        out.extend(code_stops(c, how, (near, p), (whole, only), (w, indent), || drawn(InterLayout::Interdiff)));
                    }
                    None => out.extend(drawn(InterLayout::Interdiff)),
                }
            }
        }
    }
    out
}

/// Line `parts` of an interdiff under its patch in a stack diff, after
/// `indent` columns, as `diff_parts` draws it: the indent the start of its
/// first part.
fn indented(mut parts: Parts, indent: usize) -> Parts {
    if let Some((text, _)) = parts.first_mut() {
        text.insert_str(0, &" ".repeat(indent));
    }
    parts
}

/// Of the lines drawn, `seen`, the one where place `at` is: the first that
/// shows its line of the change, else its line of what is drawn, else the
/// first that shows the line nearest that, of two as near the one past it,
/// as a line left out is found, drawn with only what the patches changed;
/// none, the last.
pub(super) fn find(seen: &[Seen], at: At) -> usize {
    // How far from the line a line drawn is, one before it the farther of
    // two as far.
    let far = |s: &Seen| s.shows.iter().map(|&l| (l.abs_diff(at.line), l < at.line)).min();
    at.row
        .and_then(|r| seen.iter().position(|s| s.rows.contains(&r)))
        .or_else(|| seen.iter().position(|s| s.shows.contains(&at.line)))
        .or_else(|| seen.iter().enumerate().filter_map(|(i, s)| far(s).map(|d| (d, i))).min().map(|(_, i)| i))
        .unwrap_or(seen.len().saturating_sub(1))
}

/// Where line `i` of those drawn, `seen`, is (`At`): the first line of what
/// is drawn it shows, the old version's of two, none (a heading) the next
/// one's; and its first line of the change. Kept, `kept`, where a switch
/// of view put line `i`, the place it came with: so that switching back and
/// forth, the cursor on the new version's line of two, or on a line of the
/// change, goes back to that line; and that drawn again as the change is
/// read, the cursor goes on to it.
pub(super) fn locate(seen: &[Seen], i: usize, kept: Option<(usize, At)>) -> Option<At> {
    let here = seen.get(i)?;
    if let Some((k, at)) = kept
        && k == i
    {
        return Some(at);
    }
    let line = seen.get(i..)?.iter().find_map(|s| s.shows.first().copied())?;
    Some(At { line, row: here.rows.first().copied() })
}

impl App {
    /// The view a pane `w` wide draws its interdiffs in: the one picked,
    /// but, while the pane is too narrow for two sides (`view.split_width`,
    /// as the code view's side by side needs), as interdiffs, side by side
    /// picked, and the change unified, side by side or compact picked.
    pub(super) fn inter_in(&self, w: usize) -> InterLayout {
        match self.inter {
            InterLayout::Sides if w < self.cfg.view.split_width => InterLayout::Interdiff,
            InterLayout::ChangeSides | InterLayout::ChangeCompact if w < self.cfg.view.split_width => InterLayout::ChangeUnified,
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

    /// Why the screen cannot draw the change of what it shows, the bases of
    /// its versions not read: the versions of a patch, each on the stack
    /// that first held it; History…'s stack diff, and what an edit changed,
    /// on the stacks before and after; the review's stack diff, on the stack
    /// as it is and as the work leaves it; what differs from Phabricator,
    /// the revision's latest diff on the commit it is on. None where it can,
    /// or shows no interdiff.
    pub(super) fn change_why(&self) -> Option<String> {
        let node = |rev: Option<usize>| rev.and_then(|r| self.db.node(r).ok());
        let at = |rev: Option<usize>| rev.map_or(String::new(), |r| format!("@{r}"));
        match self.screen {
            Screen::Versions => {
                let list = self.versions_list();
                let (p, _) = list.get(self.pick)?;
                let (before, _) = self.pick.checked_sub(1).and_then(|i| list.get(i)).filter(|_| !self.whole)?;
                let (old, new) = (format!("v{}", self.pick), format!("v{}", self.pick + 1));
                self.on_first(before, &old).and_then(|_| self.on_first(p, &new)).err()
            }
            Screen::History => {
                let e = self.hist_entry()?;
                self.stacks_why(&[(node(e.from), &at(e.from)), (node(e.to), &at(e.to))])
            }
            Screen::Review => {
                let (now, next) = self.review_stacks()?;
                self.stacks_why(&[(Some(&now), super::reviewing::AS_IT_IS.0), (Some(&next), super::reviewing::AS_IT_IS.1)])
            }
            Screen::Text => match self.text.as_ref()?.inter.as_ref()?.of {
                #[cfg(feature = "moz-phab")]
                Inter::Differs => self.differs_why(),
                Inter::Changed(rev) => {
                    let s = self.db.node(rev).ok()?;
                    self.stacks_why(&[(node(s.parent), &at(s.parent)), (Some(s), &at(Some(rev)))])
                }
            },
            _ => None,
        }
    }

    /// Whether the pane the screen draws its interdiffs in, as last drawn,
    /// is too narrow for them side by side; never drawn, it is not.
    pub(super) fn inter_narrow(&self) -> bool {
        let w = self.inter_w.get();
        w > 0 && w < self.cfg.view.split_width
    }

    /// View `v`, set in the dialog of 1: every interdiff drawn so from now
    /// on, while the editor is open, on every screen; this one keeping its
    /// place (`keep_inter_place`). The status line says how the view reads
    /// (`inter_status`).
    pub(super) fn pick_inter(&mut self, v: InterLayout) {
        let kept = self.keep_inter_place();
        self.inter = v;
        self.find_inter_place(kept);
        self.status = self.inter_status(v);
    }

    /// What the status line says of view `v` as it is set: how it reads,
    /// or, side by side on a pane too narrow for it, that this pane draws
    /// its interdiffs as interdiffs until it has the room, the change
    /// unified.
    pub(super) fn inter_status(&self, v: InterLayout) -> String {
        let (need, has) = (self.cfg.view.split_width, self.inter_w.get());
        let narrow = self.inter_narrow();
        match v {
            InterLayout::Interdiff => "each interdiff is drawn as an interdiff: a sign for which version has each line, then that version's own sign".into(),
            InterLayout::Columns => "each interdiff is drawn in a column per version: what each one's patch does with each line, the old version's first".into(),
            InterLayout::Sides if narrow => format!(
                "each interdiff is drawn side by side where its pane has {need} columns: this pane has {has}, so its interdiffs are drawn as interdiffs until it is wider"
            ),
            InterLayout::Sides => "each interdiff is drawn side by side: the old version on the left, the new one on the right, the lines both have dim".into(),
            InterLayout::ChangeUnified => "the change from the old version to the new one is drawn as a patch of its own, unified: each file as the old version leaves it, then as the new one leaves it".into(),
            InterLayout::ChangeSides if narrow => format!(
                "the change is drawn side by side where its pane has {need} columns: this pane has {has}, so it is drawn unified until the pane is wider"
            ),
            InterLayout::ChangeSides => "the change from the old version to the new one is drawn as a patch of its own, side by side: the old version's files on the left, the new one's on the right".into(),
            InterLayout::ChangeCompact if narrow => format!(
                "the change is drawn side by side and compact where its pane has {need} columns: this pane has {has}, so it is drawn unified until the pane is wider"
            ),
            InterLayout::ChangeCompact => "the change from the old version to the new one is drawn as a patch of its own, side by side and compact: each side's lines one after another, with no gap where the other has more".into(),
        }
    }

    /// Lines `l`, set in the dialog of 1: every interdiff drawn with them
    /// from now on, every line or only what the patches changed, in every
    /// view, while the editor is open, on every screen; this one keeping its
    /// place (`keep_inter_place`), the line its cursor is on, else the
    /// nearest line kept (`find`). The status line says what is drawn
    /// (`lines_status`).
    pub(super) fn pick_lines(&mut self, l: InterLines) {
        let kept = self.keep_inter_place();
        self.inter_lines = l;
        self.find_inter_place(kept);
        self.status = App::lines_status(l);
    }

    /// What the status line says of lines `l` as they are set.
    pub(super) fn lines_status(l: InterLines) -> String {
        match l {
            InterLines::Every => "every line is drawn: of each interdiff, the lines both versions have alike, the context lines and the index lines too, and of the change, the lines the change of base made, dim".into(),
            InterLines::Changed => "only what the patches changed is drawn: of each interdiff, no line both versions have alike, no context line and no index line, and of the change, no line the change of base made; a line says how many are left out at each place".into(),
        }
    }

    /// Whether the interdiffs are drawn with only what the patches changed.
    pub(super) fn only_changed(&self) -> bool {
        self.inter_lines == InterLines::Changed
    }

    /// Where the screen of interdiffs is, kept before it is drawn otherwise
    /// (in another view, each file of the change whole or not, with other
    /// lines): History…'s stack diff, the place of its cursor's line, on its
    /// line of the pane, found again as it is drawn; the review's stack
    /// diff, the place of its cursor's stop, and how far under the pane's
    /// top it is, for `find_inter_place`. The text's is found again as it is
    /// drawn (`text_again`).
    pub(super) fn keep_inter_place(&mut self) -> Option<(At, usize)> {
        if self.screen == Screen::History
            && let Some(at) = locate(&self.hist_of, self.hist_cursor, self.hist_line)
        {
            self.hist_keep = Some((at, self.hist_cursor.saturating_sub(self.hist_top)));
        }
        self.review_place()
    }

    /// The place `kept` the review's stack diff was at (`keep_inter_place`)
    /// found again, drawn otherwise; History…'s stack diff, and the text,
    /// drawn again.
    pub(super) fn find_inter_place(&mut self, kept: Option<(At, usize)>) {
        self.hist_built = None;
        self.review_found(kept);
        if self.screen == Screen::Text {
            self.text_again(self.body_w);
        }
    }

    /// What `?` says of 1 on an interdiff's screen; drawing only what the
    /// patches changed, what is left out, and how to draw every line again;
    /// side by side, on a pane too narrow for it, why the interdiffs here
    /// are drawn as interdiffs, or the change unified; in a view of the
    /// change, what 3 does there. Nothing elsewhere.
    pub(super) fn inter_help(&self) -> Vec<String> {
        if !self.inter_screen() {
            return vec![];
        }
        let mut out = vec![
            "Press 1 to pick how the interdiffs are drawn: as interdiffs, a sign for which version has each line before that version's own sign; in a column per version, what each one's patch does with each line; or side by side, the old version on the left and the new one on the right, the lines both have dim. Or how the change from the old version to the new one is drawn as a patch of its own: unified, side by side, or side by side and compact. And whether every line is drawn, or only what the patches changed.".to_string(),
        ];
        if self.only_changed() {
            out.push("Only what the patches changed is drawn: of each interdiff, the lines both versions have alike, the context lines and the index lines are left out, and of the change, the lines the change of base made; where lines are left out, a line says how many. Press 1 to draw every line again.".into());
        }
        let need = self.cfg.view.split_width;
        if self.inter_narrow() {
            match self.inter {
                InterLayout::Sides => out.push(format!("This pane is under {need} columns, too narrow for side by side: its interdiffs are drawn as interdiffs until the pane has room.")),
                InterLayout::ChangeSides => out.push(format!("This pane is under {need} columns, too narrow for the change side by side: it is drawn unified until the pane has room.")),
                InterLayout::ChangeCompact => out.push(format!("This pane is under {need} columns, too narrow for the change side by side and compact: it is drawn unified until the pane has room.")),
                _ => {}
            }
        }
        if self.change_drawn() {
            out.push(match self.change_whole {
                true => "Press 3 to see each file of the change as its hunks again.".into(),
                false => "Press 3 to see each file of the change whole rather than its hunks.".into(),
            });
        }
        out
    }

    /// The keys the bottom bar names on a screen of interdiffs: 1, for
    /// their view; drawing the change, 3, for each of its files whole, or
    /// as its hunks again. None elsewhere.
    pub(super) fn inter_keys(&self) -> String {
        match (self.inter_screen(), self.change_drawn(), self.change_whole) {
            (false, ..) => String::new(),
            (true, true, false) => "1: view · 3: whole · ".into(),
            (true, true, true) => "1: view · 3: hunks · ".into(),
            (true, false, _) => "1: view · ".into(),
        }
    }

    /// What the top bar says of the interdiffs on their screens, drawn
    /// otherwise than as interdiffs: in columns, or side by side, as the
    /// code view's side by side says it, too narrow so; the change, in the
    /// view of the code it is drawn in, its files whole; and drawn with
    /// only what the patches changed.
    pub(super) fn inter_bar(&self) -> String {
        if !self.inter_screen() {
            return String::new();
        }
        let narrow = if self.inter_narrow() { ": too narrow" } else { "" };
        let view = match self.inter {
            InterLayout::Interdiff => String::new(),
            InterLayout::Columns => " · interdiffs in columns".into(),
            InterLayout::Sides => format!(" · interdiffs side-by-side{narrow}"),
            InterLayout::ChangeUnified => " · change unified".into(),
            InterLayout::ChangeSides => format!(" · change side-by-side{narrow}"),
            InterLayout::ChangeCompact => format!(" · change side-by-side, compact{narrow}"),
        };
        let whole = if self.change_drawn() && self.change_whole { " · whole files" } else { "" };
        let only = if self.only_changed() { " · only what changed" } else { "" };
        view + whole + only
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
        stops.iter().flat_map(|s| s.lines.iter().map(|l| l.iter().map(|(t, _)| t.as_str()).collect::<String>().trim_end().to_string())).collect()
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
        let shows: Vec<Vec<usize>> = stops.iter().map(|s| s.shows.clone()).collect();
        assert_eq!(shows, [vec![], vec![0], vec![1], vec![2, 3], vec![4], vec![5], vec![6], vec![7, 9], vec![8, 10]]);
        // A line only one version has: alone in its row, `~` dim on the
        // other side.
        let all = lines(&["@@ -1 +1,2 @@", " ---", "  a", "++b"]);
        let stops = side_by_side(&all.iter().collect::<Vec<_>>(), ("old", "new"), 53, 0);
        assert_eq!(said(&stops)[4..], [row("~", "+b")]);
        assert!(stops[4].lines[0].contains(&(fit("~", 25), color::dim())), "{:?}", stops[4]);
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
        let row = |k: usize| stops[k].lines[0].clone();
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
        let parts = &stops[4].lines[0];
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
        assert_eq!(stops[3].shows, [2, 3], "one stop, both pieces");
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
        let shows: Vec<Vec<usize>> = stops.iter().map(|s| s.shows.clone()).collect();
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
        let shows: Vec<Vec<usize>> = stops.iter().map(|s| s.shows.clone()).collect();
        assert_eq!(shows, [vec![0], vec![1], vec![2, 4], vec![3, 5], vec![6]]);
        assert_eq!((style_at(&stops[2].lines[0], 0), style_at(&stops[2].lines[0], 1), style_at(&stops[3].lines[0], 1)), (color::add(), Style::new(), color::add()));
        // On one base, a line a patch adds is another than the line of the
        // base: each alone, a line of the base both leave as it is.
        assert_eq!(said(&columns(&refs, true, 0)), ["==---", "  a", "+·b", "  c", "  b", "·+c", "  d"]);
    }

    #[test]
    fn in_a_column_per_version_what_both_do_alike_is_dim_and_a_line_each_adds_in_place_of_the_other_s_in_the_colours_for_words() {
        let all = drifted();
        let stops = columns(&all.iter().collect::<Vec<_>>(), true, 2);
        let row = |k: usize| stops[k].lines[0].clone();
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
        let stops = interdiff_stops(&refs, InterLayout::Interdiff, &two, false, 400, 0);
        assert_eq!(said(&stops[..1]), [interdiff_said(&two)]);
        assert!(interdiff_said(&two).contains("- for v2 alone, + for v3 alone"), "{}", interdiff_said(&two));
        assert_eq!(said(&stops[1..]), ["--- 1234abcd", "+++ 5678ef01", "@@ -5,4 +5,4 @@", " ---", "  a", "-+b", "++B"]);
        // In a column per version: what each column is, the lines naming
        // the versions left out.
        let stops = interdiff_stops(&refs, InterLayout::Columns, &two, false, 400, 0);
        assert_eq!(said(&stops[..1]), [columns_said(&two)]);
        assert!(columns_said(&two).starts_with("The first column is v2, the second v3."), "{}", columns_said(&two));
        assert_eq!(said(&stops[1..3]), ["@@ -5,4 +5,4 @@", "==---"]);
        assert_eq!(stops[1].shows, [2], "each line by its place among them all");
        // Side by side: a heading of the two instead.
        let stops = interdiff_stops(&refs, InterLayout::Sides, &two, false, 43, 0);
        assert_eq!(said(&stops[..1]), [beside(0, "v2", 20, "v3")]);
        // A stack diff: said once, over its first interdiff, the patches'
        // lines as ever.
        let patch = |at: usize| Diffed::Patch { at, title: format!("Bug {at}"), added: false, changed: true, reworded: false, rebased: false, was: None };
        let mut diff = vec![patch(1)];
        diff.extend(all[2..].iter().cloned().map(Diffed::Code));
        diff.push(patch(2));
        diff.extend(all[2..].iter().cloned().map(Diffed::Code));
        for (v, said_once) in [(InterLayout::Interdiff, interdiff_said(&two)), (InterLayout::Columns, columns_said(&two))] {
            let stops = stack_stops(&diff, v, ("v2", "v3"), 400, &HashMap::new(), (false, false));
            let lines = said(&stops);
            assert_eq!(lines.iter().filter(|l| l.trim() == said_once).count(), 1, "{lines:#?}");
            assert_eq!(lines[1].trim(), said_once, "{lines:#?}");
            assert_eq!((lines[0].trim(), stops[1].shows.len()), ("1 ~ Bug 1", 0));
        }
        let stops = stack_stops(&diff, InterLayout::Sides, ("v2", "v3"), 400, &HashMap::new(), (false, false));
        assert_eq!(said(&stops).iter().filter(|l| l.trim().starts_with("v2 ")).count(), 2, "a heading over each");
    }

    #[test]
    fn of_the_lines_drawn_the_one_showing_a_line_is_found_again() {
        // A heading, a line, two side by side over two pieces, a line.
        let seen = |all: &[&[usize]]| -> Vec<Seen> { all.iter().map(|s| Seen { shows: s.to_vec(), rows: vec![] }).collect() };
        let at = seen(&[&[], &[0], &[2, 4], &[2, 4], &[5]]);
        let line = |line: usize| At { line, row: None };
        assert_eq!((find(&at, line(4)), find(&at, line(3)), find(&at, line(9))), (2, 2, 4));
        // The line of each: the old version's of two, a heading the next
        // one's; but the one a switch found there, kept.
        assert_eq!((locate(&at, 0, None), locate(&at, 3, None)), (Some(line(0)), Some(line(2))));
        assert_eq!((locate(&at, 3, Some((3, line(4)))), locate(&at, 2, Some((3, line(4))))), (Some(line(4)), Some(line(2))));
        // Lines of the change: found by their line of it first, else by the
        // line of the interdiff nearest them; kept, a line of the change
        // the switch came from, though no line drawn here shows it.
        let mut change = seen(&[&[], &[1], &[1], &[3]]);
        change[1].rows = vec![(0, 7)];
        change[2].rows = vec![(0, 8)];
        assert_eq!(find(&change, At { line: 1, row: Some((0, 8)) }), 2);
        assert_eq!(find(&change, At { line: 1, row: None }), 1);
        assert_eq!(locate(&change, 2, None), Some(At { line: 1, row: Some((0, 8)) }));
        let inter = seen(&[&[0], &[1], &[3]]);
        assert_eq!(locate(&inter, 1, Some((1, At { line: 1, row: Some((0, 8)) }))), Some(At { line: 1, row: Some((0, 8)) }));
    }

    /// The interdiff of two versions of a patch saying `message`, as `ops`
    /// makes it, each version's diff of f.txt its index line, then its
    /// hunks, each its header and its lines.
    fn versions(message: &str, old: (&str, &[(&str, &[&str])]), new: (&str, &[(&str, &[&str])])) -> Vec<InterLine> {
        let diff = |(index, hunks): (&str, &[(&str, &[&str])])| -> String {
            let mut d = format!("diff --git a/f.txt b/f.txt\nindex {index} 100644\n--- a/f.txt\n+++ b/f.txt\n");
            for (head, lines) in hunks {
                d += &format!("{head}\n");
                for l in *lines {
                    d += &format!("{l}\n");
                }
            }
            d
        };
        let patch = |d: String| crate::db::Patch::new("a".into(), Some("A <a@b>".into()), None, message.into(), d);
        crate::ops::interdiff_lines(&patch(diff(old)), &patch(diff(new)))
    }

    /// A patch of f.txt's two versions, on two bases a commit apart: the new
    /// base has two lines more over the patch's hunks, and its line 18
    /// otherwise; each version makes line 5 `FIVE`, and line 20 its own.
    fn rebased() -> Vec<InterLine> {
        let five = [" line 2", " line 3", " line 4", "-line 5", "+FIVE", " line 6", " line 7", " line 8"];
        let old = [" line 17", " line 18", " line 19", "-line 20", "+LINE 20", " line 21", " line 22", " line 23"];
        let new = [" line 17", " line 18, moved", " line 19", "-line 20", "+LINE 20, again", " line 21", " line 22", " line 23"];
        versions(
            "Bug 1 - five and twenty",
            ("1111111..2222222", &[("@@ -2,7 +2,7 @@ fn top()", &five), ("@@ -17,7 +17,7 @@ fn bottom()", &old)]),
            ("3333333..4444444", &[("@@ -4,7 +4,7 @@ fn top()", &five), ("@@ -19,7 +19,7 @@ fn bottom()", &new)]),
        )
    }

    /// The lines drawn of interdiff `all` in view `v` with only what the
    /// patches changed, as `said` reads them, what the view says of how to
    /// read it left out.
    fn only(all: &[InterLine], v: InterLayout, one_base: bool) -> Vec<String> {
        let refs: Vec<&InterLine> = all.iter().collect();
        let two = Two { old: "v2", new: "v3", one_base };
        said(&interdiff_stops(&refs, v, &two, true, 2000, 0)[1..])
    }

    #[test]
    fn with_only_what_the_patches_changed_a_rebase_onto_a_new_commit_is_its_one_line_that_changed() {
        let all = rebased();
        // As an interdiff: 29 lines under the two naming the versions, of
        // which an index line, two hunk headers whose numbers moved, and a
        // context line, each version's; a line each version makes its own.
        assert_eq!(all.len(), 31, "{:#?}", all.iter().map(|l| &l.text).collect::<Vec<_>>());
        let lines = only(&all, InterLayout::Interdiff, false);
        // The two lines naming the versions as ever; then the file's name,
        // the header of the hunk that changed, as the new version has it, as
        // both versions have it; and the line each makes its own; each place
        // lines are left out, how many: 26 of the 29.
        assert_eq!(
            lines[2..],
            [
                "⋯ 3 lines are left out here.",
                " diff --git a/f.txt b/f.txt",
                "⋯ 12 lines are left out here.",
                " @@ -19,7 +19,7 @@ fn bottom()",
                "⋯ 5 lines are left out here.",
                "-+LINE 20",
                "++LINE 20, again",
                "⋯ 3 lines are left out here.",
            ]
        );
        assert!(lines[0].starts_with("--- ") && lines[1].starts_with("+++ "), "{lines:#?}");
        // In a column per version, as each column says the line is.
        assert_eq!(
            only(&all, InterLayout::Columns, false),
            [
                "⋯ 3 lines are left out here.",
                "==diff --git a/f.txt b/f.txt",
                "⋯ 12 lines are left out here.",
                "==@@ -19,7 +19,7 @@ fn bottom()",
                "⋯ 5 lines are left out here.",
                "+·LINE 20",
                "·+LINE 20, again",
                "⋯ 3 lines are left out here.",
            ]
        );
        // Side by side, under the heading of the two, each line of two
        // versions one row: the index lines one, and the context lines.
        let row = |left: &str, right: &str| beside(0, left, 998, right);
        assert_eq!(
            only(&all, InterLayout::Sides, false),
            [
                row("v2", "v3"),
                "⋯ 3 lines are left out here.".to_string(),
                row("diff --git a/f.txt b/f.txt", "diff --git a/f.txt b/f.txt"),
                "⋯ 11 lines are left out here.".to_string(),
                row("@@ -19,7 +19,7 @@ fn bottom()", "@@ -19,7 +19,7 @@ fn bottom()"),
                "⋯ 4 lines are left out here.".to_string(),
                row("+LINE 20", "+LINE 20, again"),
                "⋯ 3 lines are left out here.".to_string(),
            ]
        );
        // The marks dim; the header kept as a line both have.
        let refs: Vec<&InterLine> = all.iter().collect();
        let two = Two { old: "v2", new: "v3", one_base: false };
        let stops = interdiff_stops(&refs, InterLayout::Interdiff, &two, true, 2000, 0);
        assert!(stops[3].lines[0].contains(&("⋯ 3 lines are left out here.".to_string(), color::dim())), "{:?}", stops[3]);
        assert_eq!(stops[3].shows, Vec::<usize>::new(), "a mark shows no line");
        // Each line kept shows the lines of the interdiff it stands for: the
        // header both of the two, the new version's last.
        let at = |text: &str| stops.iter().find(|s| said(std::slice::from_ref(*s)) == [text]).map(|s| s.shows.clone()).unwrap_or_else(|| panic!("{text}"));
        assert_eq!(at(" @@ -19,7 +19,7 @@ fn bottom()"), [19, 20]);
        assert_eq!((at("-+LINE 20"), at("++LINE 20, again")), (vec![26], vec![27]));
    }

    #[test]
    fn with_only_what_the_patches_changed_a_hunk_s_header_is_kept_only_over_its_own_lines() {
        // One base: the first hunk the same, the second's line its own in
        // each version. The interdiff has the first hunk's header, within
        // three lines of the index lines, and not the second's.
        let one = [" one", "-two", "+TWO", " three"];
        let (old, new) = ([" line 19", " line 20", "-line 21", "+LINE 21", " line 22"], [" line 19", " line 20", "-line 21", "+LINE 21, again", " line 22"]);
        let all = versions(
            "Bug 6 - twenty-one",
            ("1111111..2222222", &[("@@ -1,3 +1,3 @@", &one), ("@@ -20,4 +20,4 @@", &old)]),
            ("1111111..3333333", &[("@@ -1,3 +1,3 @@", &one), ("@@ -20,4 +20,4 @@", &new)]),
        );
        let texts: Vec<&str> = all.iter().map(|l| l.text.as_str()).collect();
        assert!(texts.contains(&" @@ -1,3 +1,3 @@") && !texts.contains(&" @@ -20,4 +20,4 @@"), "{texts:#?}");
        // The file's name over its line, though lines between are left out
        // of the interdiff; no hunk's header, as the first is not its own.
        assert_eq!(
            only(&all, InterLayout::Interdiff, true)[2..],
            ["⋯ 3 lines are left out here.", " diff --git a/f.txt b/f.txt", "⋯ 9 lines are left out here.", "-+LINE 21", "++LINE 21, again", "⋯ 1 line is left out here."]
        );
    }

    #[test]
    fn with_only_what_the_patches_changed_a_change_of_the_patches_below_leaves_out_the_context_it_changed() {
        // Patch 1, below, made line 10 `TEN`, then `TEN, louder`, and put a
        // line in over it; patch 2 puts a line in under it, its own each time.
        let old = [" line 8", " line 9", " TEN", "+eleven and a half", " line 11", " line 12", " line 13"];
        let new = [" line 8", " line 9", " TEN, louder", "+ten and a half", " line 11", " line 12", " line 13"];
        let all = versions("Bug 2 - and a half", ("aaaaaaa..bbbbbbb", &[("@@ -8,6 +8,7 @@", &old)]), ("ccccccc..ddddddd", &[("@@ -9,6 +9,7 @@", &new)]));
        // The context line patch 1 changed left out with the rest: the two
        // lines that differ of the run together, under the mark of the
        // others.
        assert_eq!(
            only(&all, InterLayout::Interdiff, false)[2..],
            [
                "⋯ 3 lines are left out here.",
                " diff --git a/f.txt b/f.txt",
                "⋯ 4 lines are left out here.",
                " @@ -9,6 +9,7 @@",
                "⋯ 4 lines are left out here.",
                "-+eleven and a half",
                "++ten and a half",
                "⋯ 3 lines are left out here.",
            ]
        );
        assert_eq!(only(&all, InterLayout::Columns, false)[4..7], ["⋯ 4 lines are left out here.", "+·eleven and a half", "·+ten and a half"]);
        let sides = only(&all, InterLayout::Sides, false);
        assert_eq!(sides[5..7], ["⋯ 3 lines are left out here.".to_string(), beside(0, "+eleven and a half", 998, "+ten and a half")]);
    }

    #[test]
    fn with_only_what_the_patches_changed_on_one_base_two_hunk_headers_that_differ_only_in_their_numbers_are_one() {
        // The new version puts in a line more: its index line, and the length
        // of its hunk, otherwise.
        let old = [" line 5", " line 6", " line 7", "+new one", " line 8", " line 9", " line 10"];
        let new = [" line 5", " line 6", " line 7", "+new one", "+new two", " line 8", " line 9", " line 10"];
        let all = versions("Bug 3 - new", ("1111111..2222222", &[("@@ -5,6 +5,7 @@", &old)]), ("1111111..3333333", &[("@@ -5,6 +5,8 @@", &new)]));
        assert_eq!(
            only(&all, InterLayout::Interdiff, true)[2..],
            [
                "⋯ 3 lines are left out here.",
                " diff --git a/f.txt b/f.txt",
                "⋯ 4 lines are left out here.",
                " @@ -5,6 +5,8 @@",
                "⋯ 4 lines are left out here.",
                "++new two",
                "⋯ 3 lines are left out here.",
            ]
        );
        // In a column per version, the header once, as a header both have,
        // with the new version's numbers: not the line with both ranges.
        let columns = only(&all, InterLayout::Columns, true);
        assert!(columns.contains(&"==@@ -5,6 +5,8 @@".to_string()) && !columns.iter().any(|l| l.contains('→')), "{columns:#?}");
        // As every line is drawn: the two headers, and the line with both
        // ranges.
        let refs: Vec<&InterLine> = all.iter().collect();
        let two = Two { old: "v2", new: "v3", one_base: true };
        let every = said(&interdiff_stops(&refs, InterLayout::Columns, &two, false, 2000, 0));
        assert!(every.contains(&"==@@ -5,6 +5,7 → -5,6 +5,8 @@".to_string()), "{every:#?}");
    }

    #[test]
    fn with_only_what_the_patches_changed_the_messages_compared_and_a_file_one_version_alone_touches_are_kept() {
        let diff = "diff --git a/f.txt b/f.txt\nindex 1111111..2222222 100644\n--- a/f.txt\n+++ b/f.txt\n@@ -1,3 +1,3 @@\n a\n-b\n+B\n c\n";
        let head = "diff --git a/g.txt b/g.txt\nnew file mode 100644\nindex 0000000..5555555\n--- /dev/null\n+++ b/g.txt\n";
        let g = format!("{head}@@ -0,0 +1,2 @@\n+gee\n+whiz\n\\ No newline at end of file\n");
        let old = crate::db::Patch::new("a".into(), Some("A <a@b>".into()), None, "Bug 4 - one\n\nBecause.".into(), diff.into());
        let new = crate::db::Patch::new("a".into(), Some("A <a@b>".into()), None, "Bug 4 - one, reworded\n\nBecause.".into(), format!("{diff}{g}"));
        let all = crate::ops::interdiff_lines(&old, &new);
        let lines = only(&all, InterLayout::Interdiff, true);
        // The title as each version has it; the file the new version alone
        // makes, each line of it but what git derives, git's `\` line too,
        // the index line counted with what is left out over it.
        let at = lines.iter().position(|l| l.starts_with("-Subject: ")).unwrap_or_else(|| panic!("{lines:#?}"));
        assert_eq!(lines[at..at + 2], ["-Subject: Bug 4 - one", "+Subject: Bug 4 - one, reworded"]);
        let at = lines.iter().position(|l| l.starts_with("+diff --git")).unwrap_or_else(|| panic!("{lines:#?}"));
        assert!(lines[at - 1].starts_with("⋯ ") && lines[at - 1].ends_with(" lines are left out here."), "{lines:#?}");
        assert_eq!(
            lines[at..],
            ["+diff --git a/g.txt b/g.txt", "+new file mode 100644", "+--- /dev/null", "++++ b/g.txt", "+@@ -0,0 +1,2 @@", "++gee", "++whiz", "+\\ No newline at end of file"]
        );
        // Of f.txt, which both versions change alike, nothing.
        assert!(!lines.iter().any(|l| l.contains("f.txt") || l.contains("-b") || l.contains("+B")), "{lines:#?}");
    }

    #[test]
    fn with_only_what_the_patches_changed_an_interdiff_with_nothing_left_says_so() {
        // The same patch on a new base: its index line and its hunk's numbers
        // otherwise, nothing it does.
        let five = [" line 2", " line 3", " line 4", "-line 5", "+FIVE", " line 6", " line 7", " line 8"];
        let all = versions("Bug 5 - five", ("1111111..2222222", &[("@@ -2,7 +2,7 @@", &five)]), ("3333333..4444444", &[("@@ -4,7 +4,7 @@", &five)]));
        let refs: Vec<&InterLine> = all.iter().collect();
        let two = Two { old: "v2", new: "v3", one_base: false };
        for v in [InterLayout::Interdiff, InterLayout::Columns, InterLayout::Sides] {
            let stops = interdiff_stops(&refs, v, &two, true, 2000, 0);
            assert_eq!(said(&stops), [NOTHING_LEFT], "{v:?}");
            assert_eq!(NOTHING_LEFT, "The two versions differ only in context, line numbers or index lines, which are left out.");
        }
        // In a stack diff, under its patch's line, whatever the view, what
        // is left out said once over the first interdiff.
        let patch = |at: usize| Diffed::Patch { at, title: format!("Bug {at}"), added: false, changed: true, reworded: false, rebased: true, was: None };
        let mut diff = vec![patch(1)];
        diff.extend(all[2..].iter().cloned().map(Diffed::Code));
        diff.push(patch(2));
        diff.extend(rebased()[2..].iter().cloned().map(Diffed::Code));
        for v in [InterLayout::Interdiff, InterLayout::Columns, InterLayout::Sides] {
            let lines = said(&stack_stops(&diff, v, ("@4", "@12"), 2000, &HashMap::new(), (false, true)));
            let trimmed: Vec<&str> = lines.iter().map(|l| l.trim()).collect();
            assert_eq!(trimmed[0], "1 ~ Bug 1", "{v:?}");
            assert!(trimmed[1].ends_with(LEFT_OUT) && trimmed[2] == NOTHING_LEFT && trimmed[3] == "2 ~ Bug 2", "{v:?}: {trimmed:#?}");
            assert_eq!(trimmed.iter().filter(|l| l.ends_with(LEFT_OUT)).count(), 1, "{v:?}");
            assert!(trimmed.contains(&"⋯ 3 lines are left out here."), "{v:?}: {trimmed:#?}");
        }
    }

    #[test]
    fn with_only_what_the_patches_changed_each_view_says_what_is_left_out_once_at_the_top() {
        let all = rebased();
        let refs: Vec<&InterLine> = all.iter().collect();
        let two = Two { old: "v2", new: "v3", one_base: false };
        // As an interdiff and in a column per version, after how to read
        // them; side by side, on its own, over the heading.
        for (v, first) in [(InterLayout::Interdiff, interdiff_said(&two)), (InterLayout::Columns, columns_said(&two)), (InterLayout::Sides, String::new())] {
            let stops = interdiff_stops(&refs, v, &two, true, 2000, 0);
            let said_first = said(&stops[..1]).join(" ");
            assert_eq!(said_first, format!("{first} {LEFT_OUT}").trim(), "{v:?}");
            // With every line, nothing of it.
            let every = interdiff_stops(&refs, v, &two, false, 2000, 0);
            assert!(!said(&every).join(" ").contains(LEFT_OUT), "{v:?}");
        }
        assert_eq!(
            LEFT_OUT,
            "Only what the patches changed is drawn. The lines both versions have alike, the context lines of their diffs and their index lines are left out, and where lines are left out, a line says how many. Two hunk headers that differ only in their numbers are drawn once, as the new version has them."
        );
    }

    #[test]
    fn of_the_lines_drawn_a_line_left_out_is_found_on_the_nearest_kept() {
        // Lines 0 and 9 kept, those between left out, under a mark.
        let seen = |all: &[&[usize]]| -> Vec<Seen> { all.iter().map(|s| Seen { shows: s.to_vec(), rows: vec![] }).collect() };
        let at = seen(&[&[0], &[], &[9]]);
        let line = |line: usize| At { line, row: None };
        // The nearest; of two as near, the one past it.
        assert_eq!([1, 4, 5, 8].map(|l| find(&at, line(l))), [0, 0, 2, 2]);
        assert_eq!(find(&seen(&[&[0], &[], &[8]]), line(4)), 2);
        // Past the last, the last kept; none kept, the last drawn.
        assert_eq!((find(&at, line(20)), find(&seen(&[&[], &[]]), line(3))), (2, 1));
    }
}
