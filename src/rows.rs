//! What the middle pane draws, flattened into rows: a patch (its message,
//! then each file's hunks, the comments under the lines they are on), or the
//! stack (its patches, then the stack's own comments); and the side-by-side
//! pairing of a patch's rows. After self-review's.

use std::collections::{HashMap, HashSet};

use crate::db::{Note, Patch};
use crate::diff::parse_diff;
use crate::thread::{Role, thread_turns};
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
    /// A line of claude's stream, under the comment it is working on.
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
    pub by: Option<By>,
    /// The first row of a comment.
    pub head: bool,
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

/// `text` cut into lines of at most `width` characters, between words
/// where it can be.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(10);
    let mut out = Vec::new();
    for line in text.lines() {
        let mut rest: Vec<char> = line.chars().collect();
        if rest.is_empty() {
            out.push(String::new());
        }
        while !rest.is_empty() {
            if rest.len() <= width {
                out.push(rest.iter().collect());
                break;
            }
            // The last space that leaves something on the line.
            let cut = match rest[..=width].iter().rposition(|c| *c == ' ') {
                Some(at) if at > 0 => at,
                _ => width,
            };
            out.push(rest[..cut].iter().collect::<String>().trim_end().to_string());
            rest.drain(..cut);
            while rest.first() == Some(&' ') {
                rest.remove(0);
            }
        }
    }
    out
}

pub fn by_of(n: &Note) -> By {
    match n.by.as_deref() {
        None => By::You,
        Some("agent" | "claude") => By::Agent,
        Some(_) => By::Reviewer,
    }
}

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
    for t in thread_turns(&n.text) {
        let who = match t.role {
            Role::You => "you",
            Role::Agent => "agent",
            Role::Reviewer => t.tag.as_deref().unwrap_or("reviewer"),
        };
        out.push(row(Kind::Comment, format!("     {who}:"), false));
        for line in wrap(&t.body, width.saturating_sub(7)) {
            out.push(row(Kind::Comment, format!("       {line}"), false));
        }
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
/// gone at the end.
pub fn patch_rows(p: &Patch, notes: &[&Note], width: usize, deco: &Deco) -> Vec<Row> {
    let mut on: HashMap<&str, Vec<&Note>> = HashMap::new();
    for n in notes {
        on.entry(n.line.as_str()).or_default().push(n);
    }
    let mut placed: HashSet<&str> = HashSet::new();
    let mut rows = Vec::new();
    let mut line = |rows: &mut Vec<Row>, row: Row| {
        let key = row.key.clone();
        rows.push(row);
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
    let changed = |hunks: &[crate::diff::Hunk]| -> Vec<String> {
        hunks
            .iter()
            .flat_map(|h| h.lines.iter())
            .filter(|l| l.origin == '+' || l.origin == '-')
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
                let read = matches!(kind, Kind::Add | Kind::Del)
                    && deco.read.as_ref().is_some_and(|r| r.contains(&l.key));
                line(
                    &mut rows,
                    Row {
                        key: (kind != Kind::Meta).then(|| l.key.clone()),
                        path: Some(fd.path.clone()),
                        preview: Some(l.text.chars().skip(1).collect::<String>().trim().chars().take(80).collect()),
                        hunk: Some(hunk.hid.clone()),
                        read,
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
    fn wraps_between_words() {
        let w = super::wrap("This needs more than the patch: patch 2 says two too", 20);
        assert_eq!(w, ["This needs more than", "the patch: patch 2", "says two too"]);
        assert_eq!(super::wrap("averyveryverylongwordindeed", 10), ["averyveryv", "erylongwor", "dindeed"]);
        assert_eq!(super::wrap("a\n\nb", 10), ["a", "", "b"]);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SplitLine {
    pub left: Option<usize>,
    pub right: Option<usize>,
    pub span: Option<usize>,
}

/// Pairs unified rows into side-by-side lines: a removal run and the
/// addition run after it zipped into replacement pairs. Returns the lines
/// and a row -> line index.
pub fn split_lines(rows: &[Row]) -> (Vec<SplitLine>, HashMap<usize, usize>) {
    let mut lines = Vec::new();
    let mut i = 0;
    while i < rows.len() {
        if matches!(rows[i].kind, Kind::Add | Kind::Del) {
            let (mut dels, mut adds) = (Vec::new(), Vec::new());
            while i < rows.len() && rows[i].kind == Kind::Del {
                dels.push(i);
                i += 1;
            }
            while i < rows.len() && rows[i].kind == Kind::Add {
                adds.push(i);
                i += 1;
            }
            for n in 0..dels.len().max(adds.len()) {
                lines.push(SplitLine {
                    left: dels.get(n).copied(),
                    right: adds.get(n).copied(),
                    span: None,
                });
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
