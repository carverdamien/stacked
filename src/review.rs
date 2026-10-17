//! Reviews from Phabricator, filed in the database: a comment thread per
//! remote thread, on the line it is on (or the patch's title), each reply a
//! turn of it. A thread remembers the remote comments it holds, so pulling
//! again adds only what is new.

use std::collections::{HashMap, HashSet};

use crate::db::{Db, Note, Result, Run, RunState, now};
use crate::phab::{self, Remote};
use crate::thread::{FENCE_CLOSE, REVIEWER_OPEN, Role, append_turn};
use crate::view::{MSG_PATH, note_lines, title_key};

/// The revisions of `name`'s patches: (change, D123, link).
pub fn revisions(db: &Db, name: &str) -> Result<Vec<(String, String, String)>> {
    let s = db.stack(name)?;
    Ok(s.patches
        .iter()
        .map(|id| db.patch(id))
        .filter_map(|p| phab::revision_of(&p.message).map(|(d, url)| (p.change.clone(), d, url)))
        .collect())
}

/// Files `pulled`'s comments on `name`'s patches; how many were new.
pub fn file(db: &mut Db, name: &str, pulled: &phab::Pulled) -> Result<usize> {
    let s = db.stack(name)?.clone();
    let mut added = 0;
    for rc in &pulled.revisions {
        let Some(p) = s
            .patches
            .iter()
            .map(|id| db.patch(id))
            .find(|p| p.change == rc.source)
            .cloned()
        else {
            continue;
        };
        let lines = note_lines(&p);
        // Each remote comment already filed, to the thread it is in.
        let mut thread_of: HashMap<String, String> = HashMap::new();
        for n in db.notes(&p.change) {
            for r in &n.remote {
                thread_of.insert(r.clone(), n.id.clone());
            }
        }
        let seen: HashSet<String> = thread_of.keys().cloned().collect();
        for c in &rc.comments {
            if seen.contains(&c.phid) {
                continue;
            }
            let parent = c.reply_to.as_ref().and_then(|r| thread_of.get(r)).cloned();
            let note = match parent.and_then(|id| db.note(&p.change, &id).cloned()) {
                Some(n) => {
                    let mut remote = n.remote.clone();
                    remote.push(c.phid.clone());
                    Note {
                        text: append_turn(&n.text, Role::Reviewer, &c.text, Some(&c.author)),
                        remote,
                        ..n
                    }
                }
                None => new_thread(&p, &lines, c),
            };
            thread_of.insert(c.phid.clone(), note.id.clone());
            db.set_note(note)?;
            added += 1;
        }
    }
    Ok(added)
}

fn new_thread(p: &crate::db::Patch, lines: &[crate::view::NoteLine], c: &Remote) -> Note {
    let (line, file, preview) = match phab::anchor(lines, c) {
        Some(k) => (
            k,
            c.path.clone().unwrap_or_default(),
            c.line_text.clone().unwrap_or_default(),
        ),
        None => (title_key(p), MSG_PATH.into(), p.title().to_string()),
    };
    Note {
        change: p.change.clone(),
        id: format!("phab:{}", c.phid),
        line,
        text: format!("{REVIEWER_OPEN} {}\n{}\n{FENCE_CLOSE}", c.author, c.text.trim()),
        file,
        preview: preview.trim().chars().take(80).collect(),
        by: Some(c.author.clone()),
        remote: vec![c.phid.clone()],
        time: now(),
        ..Default::default()
    }
}

/// Pulls the reviews of `name`'s patches into the database, the result a
/// run of its own.
pub fn pull(db_path: &std::path::Path, name: &str) -> Result<Run> {
    if crate::config::get().reviews.source == crate::config::Source::None {
        return crate::db::err(format!(
            "reviews come from nowhere: reviews.source is \"none\" in {}",
            crate::config::path().display()
        ));
    }
    let revs = revisions(&Db::load(db_path)?, name)?;
    let id = format!("pull-{}-{}", now(), std::process::id());
    let fetched = if revs.is_empty() {
        Err("no patch of the stack has a Differential Revision".to_string())
    } else {
        phab::pull(&revs)
    };
    Db::edit(db_path, |db| {
        let (state, summary) = match &fetched {
            Ok(pulled) => {
                let n = file(db, name, pulled)?;
                (RunState::Done, format!("{n} new comments from {} revisions", revs.len()))
            }
            Err(e) => (RunState::Failed, e.clone()),
        };
        let run = Run {
            id: id.clone(),
            what: "pull".into(),
            stack: name.into(),
            state,
            summary,
            ..Default::default()
        };
        db.add_run(run.clone())?;
        Ok(run)
    })
}
