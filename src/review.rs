//! Reviews from Phabricator, pulled into the database as work held for
//! review, as the agent's is (`held`): a remote thread new, a comment opened
//! on the line it is on (or the patch's title), the replies pulled with it
//! turns of it; a reply to a thread there already, a reply to it. Accepted,
//! they are filed in their threads; rejected, dropped. Every comment pulled
//! is remembered, with its thread or in the work held, so pulling again
//! holds only what is new.

use std::collections::HashMap;

use crate::db::{Db, Note, Result, Run, RunState, now};
use crate::held::{Held, Reply};
use crate::phab::{self, Remote};
use crate::thread::{Role, append_turn, fenced};
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

/// The remote comments on the patch of `change` pulled before, each to the
/// comment whose thread it is in: filed in a thread, or held by a pull,
/// waiting for review or reviewed (rejected, it is not pulled again).
fn pulled_before(db: &Db, change: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for n in db.notes(change) {
        for r in &n.remote {
            out.insert(r.clone(), n.id.clone());
        }
    }
    for h in db.runs().into_iter().filter(|r| crate::held::pulled(r)).filter_map(|r| r.held.as_ref()) {
        for n in h.opened.iter().filter(|n| n.change == change) {
            for r in &n.remote {
                out.entry(r.clone()).or_insert_with(|| n.id.clone());
            }
        }
        for rp in h.replies.iter().filter(|rp| rp.change == change) {
            if let Some(r) = &rp.remote {
                out.entry(r.clone()).or_insert_with(|| rp.id.clone());
            }
        }
    }
    out
}

/// What `pulled` brings that is new on `name`'s patches, held: each remote
/// thread new, a comment opened, the replies pulled with it turns of it;
/// each reply to a thread there already, or held already, a reply to it.
pub fn hold(db: &Db, name: &str, pulled: &phab::Pulled) -> Result<Held> {
    let s = db.stack(name)?;
    let mut held = Held::default();
    for rc in &pulled.revisions {
        let Some(p) = s.patches.iter().map(|id| db.patch(id)).find(|p| p.change == rc.source).cloned() else {
            continue;
        };
        let lines = note_lines(&p);
        let mut thread_of = pulled_before(db, &p.change);
        // The threads this pull opens: by each remote comment in them, their
        // place in what it holds.
        let mut opened: HashMap<String, usize> = HashMap::new();
        for c in &rc.comments {
            if thread_of.contains_key(&c.phid) || opened.contains_key(&c.phid) {
                continue;
            }
            let parent = c.reply_to.as_ref();
            if let Some(k) = parent.and_then(|r| opened.get(r)).copied() {
                let n = &mut held.opened[k];
                n.text = append_turn(&n.text, Role::Reviewer, &c.text, Some(&c.author));
                n.remote.push(c.phid.clone());
                opened.insert(c.phid.clone(), k);
            } else if let Some(id) = parent.and_then(|r| thread_of.get(r)).cloned() {
                held.replies.push(Reply {
                    change: p.change.clone(),
                    id: id.clone(),
                    text: c.text.clone(),
                    reviewer: Some(c.author.clone()),
                    remote: Some(c.phid.clone()),
                    ..Default::default()
                });
                thread_of.insert(c.phid.clone(), id);
            } else {
                held.opened.push(new_thread(&p, &lines, c));
                opened.insert(c.phid.clone(), held.opened.len() - 1);
            }
        }
    }
    Ok(held)
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
        text: fenced(&format!("reviewer {}", c.author), c.text.trim()),
        file,
        preview: preview.trim().chars().take(80).collect(),
        by: Some(c.author.clone()),
        remote: vec![c.phid.clone()],
        time: now(),
        ..Default::default()
    }
}

/// Files what pulling the reviews of `name`'s patches, on `revisions`
/// revisions, `fetched`, as run `id`: what is new held in it, to review.
pub fn file(db: &mut Db, name: &str, id: &str, revisions: usize, fetched: &std::result::Result<phab::Pulled, String>) -> Result<Run> {
    let of = format!("{revisions} revision{}", if revisions == 1 { "" } else { "s" });
    let (state, summary, held) = match fetched {
        Ok(pulled) => {
            let h = hold(db, name, pulled)?;
            let n = h.opened.iter().map(|n| n.remote.len()).sum::<usize>() + h.replies.len();
            let summary = match n {
                0 => format!("nothing new from {of}"),
                1 => format!("1 new comment from {of}: it waits for your review"),
                _ => format!("{n} new comments from {of}: they wait for your review"),
            };
            (RunState::Done, summary, (n > 0).then(|| Held { at: now(), ..h }))
        }
        Err(e) => (RunState::Failed, e.clone(), None),
    };
    let run = Run {
        id: id.into(),
        what: "pull".into(),
        stack: name.into(),
        state,
        summary,
        held,
        ..Default::default()
    };
    db.add_run(run.clone())?;
    Ok(run)
}

/// Pulls the reviews of `name`'s patches, the result a run of its own,
/// holding what is new for review.
pub fn pull(db_path: &std::path::Path, name: &str) -> Result<Run> {
    if crate::config::get().reviews.source == crate::config::Source::None {
        return crate::db::err(format!(
            "reviews come from nowhere: reviews.source is \"none\" in {}",
            crate::config::path().display()
        ));
    }
    let revs = revisions(&Db::load(db_path)?, name)?;
    let id = format!("pull-{}", crate::db::unique());
    let fetched = if revs.is_empty() {
        Err("no patch of the stack has a Differential Revision".to_string())
    } else {
        phab::pull(&revs)
    };
    Db::edit(db_path, |db| file(db, name, &id, revs.len(), &fetched))
}
