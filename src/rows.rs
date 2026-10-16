//! A patch flattened into the rows the right pane draws: its message, then
//! each file's hunks, the comments under the lines they are on; and the
//! side-by-side pairing of those rows. After self-review's.

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
    Claude,
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

/// `text` cut into lines of at most `width` characters.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(10);
    let mut out = Vec::new();
    for line in text.lines() {
        let chars: Vec<char> = line.chars().collect();
        if chars.is_empty() {
            out.push(String::new());
        }
        for chunk in chars.chunks(width) {
            out.push(chunk.iter().collect());
        }
    }
    out
}

fn by_of(n: &Note) -> By {
    match n.by.as_deref() {
        None => By::You,
        Some("claude") => By::Claude,
        Some(_) => By::Reviewer,
    }
}

/// The rows of a comment, its turns folded to `width`.
pub fn comment_rows(n: &Note, width: usize) -> Vec<Row> {
    let by = by_of(n);
    let head = match (&by, n.by.as_deref()) {
        (By::You, _) => "comment".to_string(),
        (By::Claude, _) => "claude's note".to_string(),
        (_, Some(who)) => format!("{who}'s comment"),
        _ => unreachable!(),
    };
    let row = |text: String, head: bool| Row {
        note: Some(n.id.clone()),
        by: Some(by.clone()),
        head,
        ..Row::new(Kind::Comment, text)
    };
    let mut out = vec![row(format!("   └ {head}:"), true)];
    for t in thread_turns(&n.text) {
        let who = match t.role {
            Role::You => "you",
            Role::Claude => "claude",
            Role::Reviewer => t.tag.as_deref().unwrap_or("reviewer"),
        };
        out.push(row(format!("     {who}:"), false));
        for line in wrap(&t.body, width.saturating_sub(7)) {
            out.push(row(format!("       {line}"), false));
        }
    }
    out
}

/// The rows of `p`, with `notes` under their lines and those whose line is
/// gone at the end.
pub fn patch_rows(p: &Patch, notes: &[&Note], width: usize) -> Vec<Row> {
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
                rows.extend(comment_rows(n, width));
            }
        }
    };
    rows.push(Row::new(Kind::File, MSG_PATH));
    let keys = message_keys(p);
    for (text, key) in p.message.split('\n').zip(keys) {
        line(
            &mut rows,
            Row {
                key: Some(key),
                path: Some(MSG_PATH.into()),
                preview: Some(text.trim().chars().take(80).collect()),
                ..Row::new(Kind::Msg, format!("   {text}"))
            },
        );
    }
    rows.push(Row::new(Kind::Blank, ""));
    for fd in parse_diff(&p.diff, &mode(&p.change)) {
        rows.push(Row::new(Kind::File, fd.path.clone()));
        for h in &fd.header[1..] {
            if !h.starts_with("--- ") && !h.starts_with("+++ ") {
                rows.push(Row::new(Kind::Meta, h.clone()));
            }
        }
        for hunk in &fd.hunks {
            rows.push(Row::new(Kind::Hunk, hunk.header.clone()));
            for l in &hunk.lines {
                let kind = match l.origin {
                    '+' => Kind::Add,
                    '-' => Kind::Del,
                    ' ' => Kind::Ctx,
                    _ => Kind::Meta,
                };
                line(
                    &mut rows,
                    Row {
                        key: (kind != Kind::Meta).then(|| l.key.clone()),
                        path: Some(fd.path.clone()),
                        preview: Some(l.text.chars().skip(1).collect::<String>().trim().chars().take(80).collect()),
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
            rows.extend(comment_rows(n, width));
        }
    }
    rows
}

/// One screen line of the side-by-side layout, as row indices: the old and
/// new columns, or a row drawn across the full width.
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
