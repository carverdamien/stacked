//! A patch's lines as comments are put on them: each keyed by its text, as
//! self-review keys them, under its change, so a comment stays on a line
//! the next version of the patch keeps.

use std::collections::HashMap;

use crate::db::Patch;
use crate::diff::{line_key, parse_diff};

/// Pseudo-path the message is shown under.
pub const MSG_PATH: &str = "✉ message";

/// The source key of a change's lines: the same for each of its versions.
pub fn mode(change: &str) -> String {
    format!("patch:{change}")
}

/// Whitespace folded: how a note quoting a line is matched to it.
pub fn fold(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A line a note can quote, and its key.
#[derive(Debug, Clone, PartialEq)]
pub struct NoteLine {
    pub file: String,
    pub origin: char,
    /// The line without its origin, whitespace folded.
    pub text: String,
    pub key: String,
    /// Its line number in the new version.
    pub new_no: usize,
}

/// The keys of the message's lines.
pub fn message_keys(p: &Patch) -> Vec<String> {
    let m = mode(&p.change);
    let mut counts: HashMap<&str, usize> = HashMap::new();
    p.message
        .split('\n')
        .map(|line| {
            let ord = counts.entry(line).or_insert(0);
            let key = line_key(&m, MSG_PATH, ' ', line, *ord);
            *ord += 1;
            key
        })
        .collect()
}

/// Every line of `p` a note can quote: its message's, then its diff's.
pub fn note_lines(p: &Patch) -> Vec<NoteLine> {
    let mut out: Vec<NoteLine> = p
        .message
        .split('\n')
        .zip(message_keys(p))
        .map(|(line, key)| NoteLine {
            file: MSG_PATH.into(),
            origin: ' ',
            text: fold(line),
            key,
            new_no: 0,
        })
        .collect();
    for fd in parse_diff(&p.diff, &mode(&p.change)) {
        for h in fd.hunks {
            for l in h.lines {
                out.push(NoteLine {
                    file: fd.path.clone(),
                    origin: l.origin,
                    text: fold(&l.text.chars().skip(1).collect::<String>()),
                    key: l.key,
                    new_no: l.new_no,
                });
            }
        }
    }
    out
}

/// The key of the line `text` is, on `side` if it says, in `file`.
pub fn find_line(lines: &[NoteLine], file: &str, side: char, text: &str) -> Option<String> {
    let want = fold(text);
    let mut found = None;
    for l in lines.iter().filter(|l| l.file == file && l.text == want) {
        if l.origin == side {
            return Some(l.key.clone());
        }
        found = found.or(Some(l.key.clone()));
    }
    found
}

/// The key of the title line: where a comment on the whole patch goes.
pub fn title_key(p: &Patch) -> String {
    message_keys(p).swap_remove(0)
}
