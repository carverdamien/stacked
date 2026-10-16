//! A patch as self-review's rows: its message and diff, the comments on its
//! lines among them, for the unified and side-by-side views.

use std::collections::{HashMap, HashSet};

use self_review::asks::{NoteLine, fold};
use self_review::diff::{FileDiff, line_key, parse_diff};
use self_review::git::Source;
use self_review::model::{Comment, Opened, Show};
use self_review::rows::{BuildArgs, Busy, ChunkCache, MSG_PATH, Rows, build_rows, line_keys};
use self_review::store::{Comments, ReviewedStore};

use crate::db::{Db, Note, Patch};

/// The source key of a change's lines: the same for each of its versions,
/// so a comment stays on a line the next version keeps.
pub fn mode(change: &str) -> String {
    format!("patch:{change}")
}

pub fn source(p: &Patch) -> Source {
    Source {
        key: mode(&p.change),
        label: p.title().to_string(),
        diff_args: vec![],
        new_ref: String::new(),
        message: p.message.clone(),
        alt_keys: vec![],
    }
}

pub fn files(p: &Patch) -> HashMap<String, Vec<FileDiff>> {
    let m = mode(&p.change);
    HashMap::from([(m.clone(), parse_diff(&p.diff, &m))])
}

/// Every line of `p` a note can quote: its message's, then its diff's.
pub fn note_lines(p: &Patch) -> Vec<NoteLine> {
    let m = mode(&p.change);
    let mut out = Vec::new();
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for line in p.message.split('\n') {
        let ord = counts.entry(line).or_insert(0);
        out.push(NoteLine {
            file: MSG_PATH.into(),
            origin: ' ',
            text: fold(line),
            key: line_key(&m, MSG_PATH, ' ', line, *ord),
            new_no: 0,
        });
        *ord += 1;
    }
    for fd in files(p).remove(&m).unwrap_or_default() {
        for h in fd.hunks {
            for l in h.lines {
                out.push(NoteLine {
                    file: fd.path.clone(),
                    origin: l.origin,
                    text: fold(&l.text[l.origin.len_utf8().min(l.text.len())..]),
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
    line_key(&mode(&p.change), MSG_PATH, ' ', p.title(), 0)
}

/// A note as self-review's comment, under the key self-review files it at.
pub fn comment(n: &Note) -> (String, Comment) {
    let mut c = Comment::new(&n.text, &mode(&n.change), &n.file, &n.preview);
    c.created = n.time;
    c.updated = n.time;
    if n.id == n.line {
        return (n.id.clone(), c);
    }
    c.opened = Some(Opened {
        review: n.id.split(':').next().unwrap_or("").to_string(),
        anchor: (!n.line.is_empty()).then(|| n.line.clone()),
        guidance: String::new(),
        by: n.by.clone().unwrap_or_else(|| "claude".into()),
        remote: n.remote.first().cloned(),
    });
    (n.id.clone(), c)
}

pub fn comments(db: &Db, p: &Patch) -> Comments {
    db.notes(&p.change).into_iter().map(comment).collect()
}

/// The rows of `p` folded to `width`, comments among them.
pub fn rows(db: &Db, p: &Patch, width: usize) -> Rows {
    let sources = [source(p)];
    let files = files(p);
    let data = comments(db, p);
    let keys: HashSet<String> = line_keys(&sources, &files);
    let args = BuildArgs {
        files_by_mode: &files,
        data: &data,
        expand: &HashMap::new(),
        sources: &sources,
        reviewed: &ReviewedStore::default(),
        pending_only: false,
        busy: &Busy::new(),
        show: Show::All,
        reviews: &[],
        line_keys: &keys,
        progress: None,
    };
    build_rows(&args, &mut |_, _| None, &mut ChunkCache::default(), width).rows
}
