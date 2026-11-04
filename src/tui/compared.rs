//! Rows of two versions of a text compared word by word, where their lines
//! wrap aside (`messages::diff`), each line a row of a diff, its sign
//! first, what only its version has marked: as the review of a submit shows
//! a title and summary, the review of a pull a reviewer's comment edited on
//! Phabricator, and the view of why Move HEAD does not move HEAD a commit's
//! message against its patch's. And the rows of a commit, as the middle
//! draws a patch, drawn only.

use super::*;

/// A row of `kind` saying `text`.
pub(super) fn row(kind: Kind, text: impl Into<String>) -> Row {
    Row {
        kind,
        text: text.into(),
        ..Default::default()
    }
}

/// `text`, a sentence, as rows of `kind` folded to `w`, each indented.
pub(super) fn said_rows(rows: &mut Vec<Row>, kind: Kind, text: &str, w: usize) {
    for (k, piece) in wrap(text, w.saturating_sub(3).max(10)).into_iter().enumerate() {
        rows.push(Row {
            cont: k > 0,
            ..row(kind, format!("   {piece}"))
        });
    }
}

/// A line of a diff, its sign first, as rows folded to `w`, what changed
/// in it, `words`, kept on each piece: as the middle folds code.
pub(super) fn diff_rows(rows: &mut Vec<Row>, line: &str, words: &[(usize, usize)], w: usize) {
    let kind = match line.chars().next() {
        Some('+') => Kind::Add,
        Some('-') => Kind::Del,
        _ => Kind::Ctx,
    };
    let chars: Vec<char> = line.chars().collect();
    let w = w.max(10);
    let mut at = 0;
    let mut first = true;
    while first || at < chars.len() {
        // After the first, each piece is `↪` and what comes next.
        let room = if first { w } else { w - 1 };
        let end = (at + room).min(chars.len());
        let lead = usize::from(!first);
        let mut text: String = if first { String::new() } else { crate::rows::FOLDED.to_string() };
        text.extend(&chars[at..end]);
        let words = words
            .iter()
            .filter_map(|&(s, e)| {
                let (s, e) = (s.max(at), e.min(end));
                (s < e).then(|| (s - at + lead, e - at + lead))
            })
            .collect();
        rows.push(Row {
            cont: !first,
            words,
            ..row(kind, text)
        });
        at = end;
        first = false;
    }
}

/// A line of the title and summary, `l`, compared with the other
/// version's (`messages::diff`), after its sign, as rows folded to `w`:
/// what only its version has marked on each piece, as an interdiff marks
/// it, and a break only it has on the last.
fn compared_rows(rows: &mut Vec<Row>, sign: char, l: &crate::messages::Marked, w: usize) {
    let words: Vec<(usize, usize)> = l.words.iter().map(|&(s, e)| (s + 1, e + 1)).collect();
    let from = rows.len();
    diff_rows(rows, &format!("{sign}{}", l.text), &words, w);
    for r in &mut rows[from..] {
        r.compared = true;
    }
    if let Some(r) = rows.last_mut() {
        r.broken = l.broken;
    }
}

/// What the rows of two versions of a title and summary say first when
/// they differ in nothing but where their lines wrap, or their white space;
/// and the Phabricator line's view of what differs, too.
pub(super) const ONLY_WRAPPED: &str = "Both versions have the same words: they differ only in where their lines wrap, or in their white space.";

/// Two versions of a title and summary, `before` then `after`, as rows
/// folded to `w`, compared as an interdiff compares two messages, where
/// their lines wrap aside (`messages::diff`): a section the same in both
/// once, as `after` has it; one that differs, the lines of `before` then
/// those of `after`, what only each has marked. When they differ in
/// nothing else than where their lines wrap, or their white space, that
/// is said first, and each section written otherwise is there as each
/// version writes it, the lines of `before` then those of `after`, nothing
/// marked, as every word is in both: what differs in sight, rather than
/// one version alone.
pub(super) fn message_rows(rows: &mut Vec<Row>, before: &str, after: &str, w: usize) {
    let d = crate::messages::diff(before, after);
    // Each version's lines as it writes them, the white space ending each
    // too, a carriage return as well; the same lines as `messages` reads.
    let (wrote_a, wrote_b): (Vec<&str>, Vec<&str>) = (before.split('\n').collect(), after.split('\n').collect());
    let only_wrapped = before != after && d.runs.iter().all(|r| r.same);
    if only_wrapped {
        said_rows(rows, Kind::Meta, ONLY_WRAPPED, w);
    }
    for run in &d.runs {
        let (was, is) = (d.ours.get(run.ours.clone()).unwrap_or_default(), d.theirs.get(run.theirs.clone()).unwrap_or_default());
        let written_alike = wrote_a.get(run.ours.clone()) == wrote_b.get(run.theirs.clone());
        if run.same && (written_alike || !only_wrapped) {
            for l in is {
                diff_rows(rows, &format!(" {}", l.text), &[], w);
            }
            continue;
        }
        for (sign, lines) in [('-', was), ('+', is)] {
            for l in lines {
                compared_rows(rows, sign, l, w);
            }
        }
    }
}

/// Commit `c` of the checkout, read whole, as the rows the middle draws a
/// patch in, folded to `w`: its message, then each file's name, its hunks
/// and their lines. Drawn only: no row is a line of a patch, so none takes
/// a comment nor a read mark, nor is found again as one.
pub(super) fn commit_rows(c: &crate::head::Commit, w: usize) -> Vec<Row> {
    let p = Patch::new(c.sha.clone(), None, None, c.message.clone(), c.diff.clone());
    patch_rows(&p, &[], w, &Deco::default()).into_iter().map(drawn).collect()
}

/// Row `r` of a patch's, drawn only: what it says and how, none of what
/// makes it a line of the patch.
fn drawn(r: Row) -> Row {
    Row {
        kind: r.kind,
        text: r.text,
        cont: r.cont,
        words: r.words,
        ..Default::default()
    }
}
