//! What the middle pane draws, flattened into rows: a patch (its message,
//! then each file's hunks, the comments under the lines they are on), or the
//! stack (its patches, then the stack's own comments); and the side-by-side
//! pairing of a patch's rows. After self-review's.

use std::collections::{HashMap, HashSet};

use crate::db::{Note, Patch};
use crate::diff::parse_diff;
use crate::thread::{Role, all_turns, thread_turns};
use crate::view::{MSG_PATH, message_keys, mode};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Kind {
    File,
    Hunk,
    Ctx,
    Add,
    Del,
    #[default]
    Meta,
    Msg,
    Comment,
    /// A quiet line of a comment: one of claude's stream, under the comment
    /// it is working on; where the comment stands, when its first line has
    /// no room for it; the comment, collapsed.
    Stream,
    Blank,
}

impl Kind {
    /// A line a comment can be on.
    pub fn commentable(self) -> bool {
        matches!(self, Kind::Add | Kind::Del | Kind::Ctx | Kind::Msg)
    }
}

/// Who wrote the comment a row is part of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum By {
    You,
    Agent,
    Reviewer,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Row {
    pub kind: Kind,
    pub text: String,
    /// The line's key, on a line a comment can be on.
    pub key: Option<String>,
    /// The comment's id, on its rows.
    pub note: Option<String>,
    /// Who wrote it: its comment's opener on its head, the turn's author on
    /// a turn's rows.
    pub by: Option<By>,
    /// The first row of a comment.
    pub head: bool,
    /// The row naming who said a turn of a comment's thread, over what they
    /// said.
    pub who: bool,
    /// The turn of its comment's thread a row shows, of all its turns
    /// (those deleted too), from 0.
    pub turn: Option<usize>,
    pub path: Option<String>,
    /// The line's text, kept with a comment on it.
    pub preview: Option<String>,
    /// The hunk a hunk's header and lines are of: its id.
    pub hunk: Option<String>,
    /// The line of the patch's message it is, from 0.
    pub msg_at: Option<usize>,
    /// The line of the patch's diff it is, from 0.
    pub diff_at: Option<usize>,
    /// The line of the file, as the patch leaves it, it is (or where a line
    /// removed was), from 1.
    pub new_no: Option<usize>,
    /// A changed line marked read, in the reading mode: drawn dim.
    pub read: bool,
    /// A changed line that needs no reading, in the reading mode: empty or
    /// white space alone, as the settings have it. Drawn dim, and counted
    /// in nothing.
    pub free: bool,
    /// A line of a turn of a comment, as markdown reads it: what it says
    /// after the bar's indent, piece by piece, each with what it is.
    pub pieces: crate::markdown::Line,
    /// A reference a turn of the comment attached, under the comment:
    /// Enter previews its lines.
    pub reference: Option<crate::refs::Ref>,
    /// A comment a reference points at, under the comment pointing:
    /// Enter goes to it.
    pub points: Option<(String, String)>,
    /// The numbers of the comment's references its text names on this
    /// line: Enter goes to the one, or picks among them.
    pub names: Vec<usize>,
    /// A piece of a line too wide for the pane, after its first: the same
    /// line, folded, which counts once.
    pub cont: bool,
}

impl Row {
    /// Whether it is a line under a comment for one of its references.
    pub fn is_reference(&self) -> bool {
        self.reference.is_some() || self.points.is_some()
    }
}

/// What begins each piece of a line of code folded, after the first, in
/// place of the line's `+`, `-` or space.
pub const FOLDED: char = '↪';

/// A diff line, `text`, its `+`, `-` or space first, in pieces of at most
/// `w` characters: each after the first is `↪` and what comes next. As it
/// is when it fits, or the pane is too narrow to fold it.
fn fold_code(text: &str, w: usize) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= w || w < 10 {
        return vec![text.to_string()];
    }
    let mut out = vec![chars[..w].iter().collect::<String>()];
    let mut at = w;
    while at < chars.len() {
        let end = (at + w - 1).min(chars.len());
        out.push(format!("{FOLDED}{}", chars[at..end].iter().collect::<String>()));
        at = end;
    }
    out
}

/// A line of a patch's message, `text`, folded into lines of at most `w`
/// characters, between words: its indent kept, and each line after the
/// first under its text, a bullet's under what follows the bullet; a word
/// longer than a line broken where the line ends. As it is when it fits,
/// or the pane is too narrow to fold it.
fn fold_prose(text: &str, w: usize) -> Vec<String> {
    let indent: String = text.chars().take_while(|c| *c == ' ').collect();
    let rest = &text[indent.len()..];
    // `- `, `* `, `+ `, `1. `, `2) `: what follows it is where the lines go.
    let bullet = {
        let digits = rest.chars().take_while(char::is_ascii_digit).count();
        let mark = match rest[digits..].chars().next() {
            Some('-' | '*' | '+') if digits == 0 => 1,
            Some('.' | ')') if digits > 0 => digits + 1,
            _ => 0,
        };
        if mark > 0 && rest[mark..].starts_with(' ') { mark + 1 } else { 0 }
    };
    let hang = indent.chars().count() + bullet;
    if text.chars().count() <= w || w < hang + 10 {
        return vec![text.to_string()];
    }
    let (first, under) = (w - indent.chars().count(), w - hang);
    let mut out: Vec<String> = Vec::new();
    let mut line = String::new();
    let room = |out: &Vec<String>| if out.is_empty() { first } else { under };
    for word in rest.split_whitespace() {
        let mut word = word.to_string();
        loop {
            let (len, wl) = (line.chars().count(), word.chars().count());
            if (if line.is_empty() { wl } else { len + 1 + wl }) <= room(&out) {
                if !line.is_empty() {
                    line.push(' ');
                }
                line.push_str(&word);
                break;
            }
            if !line.is_empty() {
                out.push(std::mem::take(&mut line));
                continue;
            }
            // Longer than a line on its own: broken where the line ends.
            let r = room(&out);
            out.push(word.chars().take(r).collect());
            word = word.chars().skip(r).collect();
            if word.is_empty() {
                break;
            }
        }
    }
    if !line.is_empty() || out.is_empty() {
        out.push(line);
    }
    let pad = " ".repeat(hang);
    out.into_iter().enumerate().map(|(k, l)| if k == 0 { format!("{indent}{l}") } else { format!("{pad}{l}") }).collect()
}

/// `pieces`, a line of a comment's text, its mentions marked as what the
/// comment has (`nums`, its references' numbers) or not; and the numbers
/// of those it names that are there.
fn marked(pieces: crate::markdown::Line, nums: &[usize]) -> (crate::markdown::Line, Vec<usize>) {
    use crate::markdown::Mark;
    let (mut out, mut names) = (Vec::new(), Vec::new());
    for (s, m) in pieces {
        // Not in code, nor in a link: there, it is what it says.
        let ms = match m {
            Mark::Plain | Mark::Strong | Mark::Emph | Mark::Heading => crate::refs::mentions(&s),
            _ => vec![],
        };
        if ms.is_empty() {
            out.push((s, m));
            continue;
        }
        let mut at = 0;
        for mention in ms {
            if mention.at.start > at {
                out.push((s[at..mention.at.start].to_string(), m));
            }
            let holds = crate::refs::holds(&mention, nums);
            out.push((s[mention.at.clone()].to_string(), if holds { Mark::Mention } else { Mark::Broken }));
            if holds {
                match &mention.names {
                    crate::refs::Names::All => names.extend_from_slice(nums),
                    crate::refs::Names::Some(ns) => names.extend(ns),
                    crate::refs::Names::Bad => {}
                }
            }
            at = mention.at.end;
        }
        if at < s.len() {
            out.push((s[at..].to_string(), m));
        }
    }
    names.sort_unstable();
    names.dedup();
    (out, names)
}

impl Row {
    fn new(kind: Kind, text: impl Into<String>) -> Row {
        Row {
            kind,
            text: text.into(),
            ..Default::default()
        }
    }
}

/// What the rows say beyond the patch and its comments' text: by comment
/// id, where each stands, on its first line, and the lines of claude's
/// stream under one claude is working on; in the reading mode, the lines
/// read.
#[derive(Debug, Default, Clone)]
pub struct Deco {
    pub tags: HashMap<String, String>,
    pub streams: HashMap<String, Vec<String>>,
    pub read: Option<HashSet<String>>,
    /// Whether, reading, changed lines that are empty or white space alone
    /// need reading too.
    pub whitespace: bool,
    /// The comments collapsed by hand, by id: a quiet line each.
    pub collapsed: HashSet<String>,
    /// The turns of a comment being written in the editor, by its id: from
    /// the first, to the one after the last, of all its turns. The editor
    /// shows them, and its rows do not.
    pub writing: HashMap<String, (usize, usize)>,
    /// The comments references point at, by change and id, as their line
    /// under the comment pointing says them.
    pub pointed: HashMap<(String, String), String>,
}

/// `3/10`: how many of `keys` are read, of how many.
fn read_of<'a>(keys: impl IntoIterator<Item = &'a String>, read: &HashSet<String>) -> String {
    let (mut n, mut all) = (0, 0);
    for k in keys {
        all += 1;
        n += read.contains(k) as usize;
    }
    format!("{n}/{all}")
}


pub fn by_of(n: &Note) -> By {
    match n.by.as_deref() {
        None => By::You,
        Some("agent" | "claude") => By::Agent,
        Some(_) => By::Reviewer,
    }
}

/// What a turn of a comment's thread has at its left, in its author's
/// colour: what they said is in the terminal's own, so that no comment
/// reads as code.
pub const BAR: char = '▎';

/// The rows of a comment, its turns folded to `width`, what `deco` says of
/// it on its first line and under it.
pub fn comment_rows(n: &Note, width: usize, deco: &Deco) -> Vec<Row> {
    let by = by_of(n);
    let who = match (&by, n.by.as_deref()) {
        (By::You, _) => "comment".to_string(),
        (By::Agent, _) => "agent's comment".to_string(),
        (_, Some(who)) => format!("{who}'s comment"),
        _ => unreachable!(),
    };
    let row = |kind: Kind, text: String, head: bool| Row {
        note: Some(n.id.clone()),
        by: Some(by.clone()),
        head,
        ..Row::new(kind, text)
    };
    // Collapsed: one quiet line, how it starts and how many turns it has.
    if deco.collapsed.contains(&n.id) {
        let turns = thread_turns(&n.text);
        let first = turns.first().map(|t| crate::markdown::first_line(&t.body)).unwrap_or_default();
        let more = if turns.len() > 1 { format!(" · {} turns", turns.len()) } else { String::new() };
        let head = format!("   ▸ {who}: ");
        let room = width.saturating_sub(head.chars().count() + more.chars().count()).max(10);
        let first = if first.chars().count() > room {
            first.chars().take(room - 1).chain(['…']).collect()
        } else {
            first
        };
        return vec![row(Kind::Stream, format!("{head}{first}{more}"), true)];
    }
    // Where it stands, on its first line; under it when that is too long.
    let tag = deco.tags.get(&n.id).map_or(String::new(), |t| format!(" · {t}"));
    let head = format!("   └ {who}{tag}:");
    let mut out = Vec::new();
    if head.chars().count() <= width.max(20) {
        out.push(row(Kind::Comment, head, true));
    } else {
        out.push(row(Kind::Comment, format!("   └ {who}:"), true));
        for part in tag.trim_start_matches(" · ").split(" · ") {
            out.push(row(Kind::Stream, format!("     {part}"), false));
        }
    }
    let hidden = deco.writing.get(&n.id).copied().unwrap_or((usize::MAX, usize::MAX));
    // The numbers of its references, which mentions in it name.
    let nums = crate::refs::numbers(n);
    // The references the turns attach, each once, by the turn and its author.
    let mut refs: Vec<(usize, By, crate::refs::Pointing)> = Vec::new();
    for (i, t) in all_turns(&n.text).into_iter().enumerate() {
        // Deleted the first way: shown nowhere. Written in the editor: shown
        // there only.
        if t.deleted || (hidden.0..hidden.1).contains(&i) {
            continue;
        }
        let (name, author) = match t.role {
            Role::You => ("you", By::You),
            Role::Agent => ("agent", By::Agent),
            Role::Reviewer => (t.tag.as_deref().unwrap_or("reviewer"), By::Reviewer),
        };
        // Each turn its author's: their name, then what they said, the bar
        // left of both.
        let of = |r: Row| Row {
            turn: Some(i),
            by: Some(author.clone()),
            ..r
        };
        out.push(Row {
            who: true,
            ..of(row(Kind::Comment, format!("     {BAR} {name}:"), false))
        });
        // What they said, read as markdown, the references it attaches kept
        // for under the comment.
        let (body, attached) = crate::refs::split(&t.body);
        for r in attached {
            if !refs.iter().any(|(_, _, had)| *had == r) {
                refs.push((i, author.clone(), r));
            }
        }
        for pieces in crate::markdown::render(&body, width.saturating_sub(9)) {
            let (pieces, names) = marked(pieces, &nums);
            let line: String = pieces.iter().map(|(s, _)| s.as_str()).collect();
            out.push(Row {
                pieces,
                names,
                ..of(row(Kind::Comment, format!("     {BAR}   {line}"), false))
            });
        }
    }
    // Under the comment, a line for each reference attached to it, by its
    // name, then for each a turn points at: Enter previews lines, and goes
    // to a comment.
    let attached = n.attached.iter().filter_map(|a| {
        let p = match &a.to {
            crate::refs::To::Comment { change, id } => crate::refs::Pointer::Comment {
                change: change.clone(),
                id: id.clone(),
            },
            crate::refs::To::Lines(_) => crate::refs::Pointer::Lines(a.lines()?),
        };
        Some((None, By::You, format!("{} ", a.name()), p))
    });
    let pointed = refs.into_iter().map(|(i, author, p)| (Some(i), author, p.n.map_or(String::new(), |k| format!("r{k} ")), p.to));
    for (turn, author, name, p) in attached.collect::<Vec<_>>().into_iter().chain(pointed) {
        let (said, reference, points) = match p {
            crate::refs::Pointer::Lines(r) => (format!("{name}⇢ {}", r.said()), Some(r), None),
            crate::refs::Pointer::Comment { change, id } => {
                let what = deco.pointed.get(&(change.clone(), id.clone())).map_or("a comment that is gone", String::as_str);
                (format!("{name}↗ {what}"), None, Some((change, id)))
            }
        };
        out.push(Row {
            turn,
            by: Some(author),
            pieces: vec![(said.clone(), crate::markdown::Mark::Link)],
            reference,
            points,
            ..row(Kind::Comment, format!("     {BAR}   {said}"), false)
        });
    }
    for line in deco.streams.get(&n.id).into_iter().flatten() {
        let line: String = line.chars().take(width.saturating_sub(7)).collect();
        out.push(row(Kind::Stream, format!("     ⋯ {line}"), false));
    }
    out
}

/// `p`'s diff, parsed: once per patch, as a patch never changes, and the
/// rows of one are built again as comments come and go.
pub fn parsed(p: &Patch) -> std::sync::Arc<Vec<crate::diff::FileDiff>> {
    use std::sync::{Arc, Mutex};
    static PARSED: Mutex<Option<HashMap<String, Arc<Vec<crate::diff::FileDiff>>>>> = Mutex::new(None);
    let mut all = PARSED.lock().unwrap();
    all.get_or_insert_with(HashMap::new)
        .entry(p.id.clone())
        .or_insert_with(|| Arc::new(parse_diff(&p.diff, &mode(&p.change))))
        .clone()
}

/// The rows of `p`, with `notes` under their lines and those whose line is
/// gone at the end. A line too wide for `width` is folded: the message's
/// between words, the code's where the pane ends; its comments under its
/// last piece.
pub fn patch_rows(p: &Patch, notes: &[&Note], width: usize, deco: &Deco) -> Vec<Row> {
    let mut on: HashMap<&str, Vec<&Note>> = HashMap::new();
    for n in notes {
        on.entry(n.line.as_str()).or_default().push(n);
    }
    let mut placed: HashSet<&str> = HashSet::new();
    let mut rows = Vec::new();
    let mut line = |rows: &mut Vec<Row>, row: Row| {
        let key = row.key.clone();
        let pieces = match row.kind {
            Kind::Msg => fold_prose(&row.text, width),
            Kind::Add | Kind::Del | Kind::Ctx => fold_code(&row.text, width),
            _ => vec![row.text.clone()],
        };
        for (k, text) in pieces.into_iter().enumerate() {
            rows.push(Row { text, cont: k > 0, ..row.clone() });
        }
        if let Some(k) = key
            && let Some(ns) = on.get(k.as_str())
        {
            for n in ns {
                placed.insert(n.id.as_str());
                rows.extend(comment_rows(n, width, deco));
            }
        }
    };
    rows.push(Row::new(Kind::File, MSG_PATH));
    let keys = message_keys(p);
    for (i, (text, key)) in p.message.split('\n').zip(keys).enumerate() {
        line(
            &mut rows,
            Row {
                key: Some(key),
                path: Some(MSG_PATH.into()),
                preview: Some(text.trim().chars().take(80).collect()),
                msg_at: Some(i),
                ..Row::new(Kind::Msg, format!("   {text}"))
            },
        );
    }
    rows.push(Row::new(Kind::Blank, ""));
    // A changed line that needs no reading: white space alone.
    let free = |l: &crate::diff::Line| deco.read.is_some() && !deco.whitespace && crate::diff::white_space(&l.text);
    let changed = |hunks: &[crate::diff::Hunk]| -> Vec<String> {
        hunks
            .iter()
            .flat_map(|h| h.lines.iter())
            .filter(|l| (l.origin == '+' || l.origin == '-') && !free(l))
            .map(|l| l.key.clone())
            .collect()
    };
    for fd in parsed(p).iter() {
        let mut label = fd.path.clone();
        if let Some(read) = &deco.read {
            label += &format!("   read {} lines", read_of(&changed(&fd.hunks), read));
        }
        rows.push(Row {
            path: Some(fd.path.clone()),
            diff_at: Some(fd.at),
            new_no: Some(1),
            ..Row::new(Kind::File, label)
        });
        for (j, h) in fd.header.iter().enumerate().skip(1) {
            if !h.starts_with("--- ") && !h.starts_with("+++ ") {
                rows.push(Row {
                    diff_at: Some(fd.at + j),
                    ..Row::new(Kind::Meta, h.clone())
                });
            }
        }
        for hunk in &fd.hunks {
            let mut label = hunk.header.clone();
            if let Some(read) = &deco.read {
                label += &format!("   read {} lines", read_of(&changed(std::slice::from_ref(hunk)), read));
            }
            rows.push(Row {
                path: Some(fd.path.clone()),
                hunk: Some(hunk.hid.clone()),
                diff_at: Some(hunk.at),
                new_no: Some(hunk.new_start.max(1)),
                ..Row::new(Kind::Hunk, label)
            });
            for l in &hunk.lines {
                let kind = match l.origin {
                    '+' => Kind::Add,
                    '-' => Kind::Del,
                    ' ' => Kind::Ctx,
                    _ => Kind::Meta,
                };
                let changed_line = matches!(kind, Kind::Add | Kind::Del);
                let read = changed_line && deco.read.as_ref().is_some_and(|r| r.contains(&l.key));
                line(
                    &mut rows,
                    Row {
                        key: (kind != Kind::Meta).then(|| l.key.clone()),
                        path: Some(fd.path.clone()),
                        preview: Some(l.text.chars().skip(1).collect::<String>().trim().chars().take(80).collect()),
                        hunk: Some(hunk.hid.clone()),
                        read,
                        free: changed_line && free(l),
                        diff_at: Some(l.at),
                        new_no: Some(l.new_no.max(1)),
                        ..Row::new(kind, l.text.clone())
                    },
                );
            }
        }
        rows.push(Row::new(Kind::Blank, ""));
    }
    let gone: Vec<&&Note> = notes.iter().filter(|n| !placed.contains(n.id.as_str())).collect();
    if !gone.is_empty() {
        rows.push(Row::new(Kind::File, "comments on lines this version does not have"));
        for n in gone {
            rows.push(Row::new(Kind::Meta, format!("   {}: {}", n.file, n.preview)));
            rows.extend(comment_rows(n, width, deco));
        }
    }
    rows
}

/// The rows of the stack as a whole: what it is, its patches, then its own
/// comments, as many as `notes`.
pub fn stack_rows(about: &str, titles: &[String], notes: &[&Note], width: usize, deco: &Deco) -> Vec<Row> {
    let mut rows = vec![Row::new(Kind::File, about)];
    for (i, t) in titles.iter().enumerate() {
        rows.push(Row::new(Kind::Meta, format!("  {:>2} {t}", i + 1)));
    }
    rows.push(Row::new(Kind::Blank, ""));
    if notes.is_empty() {
        rows.push(Row::new(Kind::Meta, "  no comments on the stack itself"));
        return rows;
    }
    rows.push(Row::new(Kind::File, "comments on the stack"));
    for n in notes {
        if n.about.is_some() {
            rows.push(Row::new(Kind::Meta, format!("   about {}", n.preview)));
        }
        rows.extend(comment_rows(n, width, deco));
    }
    rows
}

/// One screen line of the side-by-side layout, as row indices: the old and
/// new columns, or a row drawn across the full width.
#[cfg(test)]
mod tests {
    #[test]
    fn a_turn_wraps_between_words() {
        let said = |text: &str, w: usize| -> Vec<String> {
            crate::markdown::render(text, w).iter().map(|l| l.iter().map(|(s, _)| s.as_str()).collect()).collect()
        };
        assert_eq!(said("This needs more than the patch: patch 2 says two too", 20), ["This needs more than", "the patch: patch 2", "says two too"]);
        assert_eq!(said("averyveryverylongwordindeed", 10), ["averyveryv", "erylongwor", "dindeed"]);
        assert_eq!(said("a\n\nb", 10), ["a", "", "b"]);
    }

    #[test]
    fn a_line_of_code_too_wide_is_folded_where_the_pane_ends() {
        use super::fold_code;
        assert_eq!(fold_code("+short", 20), ["+short"]);
        let line = format!("+{}", "abcdefghij".repeat(3));
        let pieces = fold_code(&line, 12);
        assert_eq!(pieces, ["+abcdefghija", "↪bcdefghijab", "↪cdefghij"]);
        // Each as wide as the pane at most, and all of the line in them.
        assert!(pieces.iter().all(|p| p.chars().count() <= 12));
        let back: String = pieces.iter().enumerate().map(|(k, p)| if k == 0 { p.clone() } else { p.chars().skip(1).collect() }).collect();
        assert_eq!(back, line);
        // Too narrow to fold: as it is.
        assert_eq!(fold_code(&line, 9), std::slice::from_ref(&line));
    }

    #[test]
    fn a_line_of_the_message_too_wide_is_folded_between_words() {
        use super::fold_prose;
        assert_eq!(fold_prose("   Short.", 40), ["   Short."]);
        assert_eq!(
            fold_prose("   This summary was typed as one long line on Phabricator", 24),
            ["   This summary was", "   typed as one long", "   line on Phabricator"]
        );
        // A bullet's lines under what follows the bullet.
        assert_eq!(fold_prose("   - an item that goes on and on", 20), ["   - an item that", "     goes on and on"]);
        // A word longer than a line: broken where the line ends.
        assert_eq!(
            fold_prose("   see https://example.com/a/very/long/path/indeed", 20),
            ["   see", "   https://example.c", "   om/a/very/long/pa", "   th/indeed"]
        );
        // Too narrow to fold: as it is.
        assert_eq!(fold_prose("   - x y z", 12), ["   - x y z"]);
    }

    #[test]
    fn a_patch_s_lines_too_wide_are_folded_each_piece_the_same_line() {
        use super::{Deco, Kind, Row, patch_rows};
        let diff = format!("diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1 +1 @@\n-old\n+{}\n", "x".repeat(50));
        let message = "Bug 1 - a title\n\nA summary typed as one long line, as Phabricator has them.";
        let p = crate::db::Patch::new("c".into(), None, None, message.into(), diff);
        let rows = patch_rows(&p, &[], 30, &Deco::default());
        let long: Vec<&Row> = rows.iter().filter(|r| r.kind == Kind::Add).collect();
        assert_eq!(long.len(), 2, "{rows:#?}");
        assert_eq!((long[0].cont, long[1].cont, long[0].key.is_some()), (false, true, true));
        assert_eq!((&long[0].key, &long[0].diff_at), (&long[1].key, &long[1].diff_at));
        assert!(long[1].text.starts_with('↪'), "{}", long[1].text);
        let msg: Vec<&str> = rows.iter().filter(|r| r.msg_at == Some(2)).map(|r| r.text.as_str()).collect();
        assert_eq!(msg, ["   A summary typed as one long", "   line, as Phabricator has", "   them."]);
        // A comment on it: under its last piece.
        let key = long[0].key.clone().unwrap();
        let n = crate::db::Note {
            change: "c".into(),
            id: "n".into(),
            line: key.clone(),
            text: "Why so long?".into(),
            ..Default::default()
        };
        let rows = patch_rows(&p, &[&n], 30, &Deco::default());
        let at = rows.iter().position(|r| r.note.as_deref() == Some("n")).unwrap();
        assert!(rows[at - 1].cont && rows[at - 1].key.as_deref() == Some(key.as_str()), "{rows:#?}");
    }

    #[test]
    fn side_by_side_a_line_folded_is_paired_as_one() {
        use super::{Kind, Row, split_lines};
        let r = |kind: Kind, cont: bool| Row { kind, cont, ..Default::default() };
        // Two lines removed, the first in three pieces; two added.
        let rows = vec![r(Kind::Del, false), r(Kind::Del, true), r(Kind::Del, true), r(Kind::Del, false), r(Kind::Add, false), r(Kind::Add, false)];
        let (lines, _) = split_lines(&rows);
        let pairs: Vec<(Option<usize>, Option<usize>)> = lines.iter().map(|l| (l.left, l.right)).collect();
        assert_eq!(pairs, [(Some(0), Some(4)), (Some(1), None), (Some(2), None), (Some(3), Some(5))]);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SplitLine {
    pub left: Option<usize>,
    pub right: Option<usize>,
    pub span: Option<usize>,
}

/// Pairs unified rows into side-by-side lines: a removal run and the
/// addition run after it zipped into replacement pairs, line by line, the
/// pieces of a line folded beside the other's, the fewer padded. Returns
/// the lines and a row -> line index.
pub fn split_lines(rows: &[Row]) -> (Vec<SplitLine>, HashMap<usize, usize>) {
    let mut lines = Vec::new();
    let mut i = 0;
    // The run of `kind` from `i` on, a line each: its rows, its pieces.
    let run = |kind: Kind, i: &mut usize| {
        let mut out: Vec<Vec<usize>> = Vec::new();
        while *i < rows.len() && rows[*i].kind == kind {
            match out.last_mut() {
                Some(l) if rows[*i].cont => l.push(*i),
                _ => out.push(vec![*i]),
            }
            *i += 1;
        }
        out
    };
    while i < rows.len() {
        if matches!(rows[i].kind, Kind::Add | Kind::Del) {
            let dels = run(Kind::Del, &mut i);
            let adds = run(Kind::Add, &mut i);
            for n in 0..dels.len().max(adds.len()) {
                let (l, r) = (dels.get(n).map_or(&[][..], Vec::as_slice), adds.get(n).map_or(&[][..], Vec::as_slice));
                for k in 0..l.len().max(r.len()) {
                    lines.push(SplitLine {
                        left: l.get(k).copied(),
                        right: r.get(k).copied(),
                        span: None,
                    });
                }
            }
            continue;
        }
        if rows[i].kind == Kind::Ctx {
            lines.push(SplitLine {
                left: Some(i),
                right: Some(i),
                span: None,
            });
        } else {
            lines.push(SplitLine {
                span: Some(i),
                ..Default::default()
            });
        }
        i += 1;
    }
    let mut index = HashMap::new();
    for (n, line) in lines.iter().enumerate() {
        for idx in [line.span, line.left, line.right].into_iter().flatten() {
            index.entry(idx).or_insert(n);
        }
    }
    (lines, index)
}

/// The row a split line's cursor addresses: the new side where there is one.
pub fn split_row(line: &SplitLine) -> usize {
    line.span.or(line.right).or(line.left).unwrap_or(0)
}
