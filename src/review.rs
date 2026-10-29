//! Reviews from Phabricator, pulled into the database as work held for
//! review, as the agent's is (`held`): a remote thread new, a comment opened
//! on the line it is on (or the patch's title), the replies pulled with it
//! turns of it; a reply to a thread there already, a reply to it. Accepted,
//! they are filed in their threads; rejected, dropped. Every comment pulled
//! is remembered, with its thread or in the work held, so pulling again
//! holds only what is new. A comment edited on Phabricator since it was
//! filed is held as an edit of its turn: accepted, the turn says what the
//! comment says now. Each version of a comment has an id of its own, so
//! that one pulled before is known by any of them.

use std::collections::{HashMap, HashSet};

use crate::db::{Db, Note, Result, Run, RunState, now, stack_change};
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

/// The remote comments pulled before on the patch of `change`, of stack
/// `name`, by the id of each version pulled; each to the comment whose
/// thread it is in, (change, id).
#[derive(Default)]
struct Before {
    /// Filed in a thread open: on the patch, or moved from it to the
    /// stack's comments.
    filed: HashMap<String, (String, String)>,
    /// Held by a pull, and whether its review is still open: reviewed, and
    /// rejected or its thread closed since, it is not pulled again.
    held: HashMap<String, ((String, String), bool)>,
}

fn pulled_before(db: &Db, name: &str, change: &str) -> Before {
    let mut before = Before::default();
    let moved: Vec<&Note> = db
        .notes(&stack_change(name))
        .into_iter()
        .filter(|n| n.moved.as_ref().is_some_and(|m| m.change == change))
        .collect();
    for n in db.notes(change).into_iter().chain(moved.iter().copied()) {
        for r in &n.remote {
            before.filed.insert(r.clone(), (n.change.clone(), n.id.clone()));
        }
    }
    let moved: HashSet<(&str, &str)> = moved.iter().map(|n| (n.change.as_str(), n.id.as_str())).collect();
    for r in db.runs().into_iter().filter(|r| crate::held::pulled(r)) {
        let Some(h) = r.held.as_ref() else { continue };
        let open = crate::held::waiting(db, r);
        let mut hold = |id: &String, at: (&String, &String)| {
            let e = before.held.entry(id.clone()).or_insert(((at.0.clone(), at.1.clone()), false));
            e.1 |= open;
        };
        for n in h.opened.iter().filter(|n| n.change == change) {
            for id in &n.remote {
                hold(id, (&n.change, &n.id));
            }
        }
        for rp in h.replies.iter().filter(|rp| rp.change == change || moved.contains(&(rp.change.as_str(), rp.id.as_str()))) {
            if let Some(id) = &rp.remote {
                hold(id, (&rp.change, &rp.id));
            }
        }
    }
    before
}

/// How many of `comments`, those of a revision on Phabricator, the patch of
/// `change` in stack `name` has not got yet, as the Phabricator line over
/// its message counts them: those new, then those edited since they were
/// filed. Told from what the database holds as it stands, its history never
/// read: the comments open (the change's, and the stack's own, as those
/// moved there), the threads of the change, open or closed, by their ids
/// (a thread pulled is `phab:` and the id of its first comment as it was
/// pulled), and the pulls that wait for your review. A comment that waits
/// there is not counted, nor one of a thread closed, which a pull passes
/// over; one rejected in a pull's review is, as only that pull's record, in
/// the history, holds it.
pub fn not_pulled(db: &Db, name: &str, change: &str, comments: &[Remote]) -> (usize, usize) {
    let stack = stack_change(name);
    let on_stack = db.notes(&stack);
    // Filed in a thread open: on the patch, or on the stack. A comment's id
    // is Phabricator's own, so none of another revision's is taken for it.
    let filed: HashSet<&str> = db.notes(change).into_iter().chain(on_stack.iter().copied()).flat_map(|n| n.remote.iter().map(String::as_str)).collect();
    // Held by a pull whose review is open.
    let mut waiting: HashSet<&str> = HashSet::new();
    for r in db.live_runs().into_iter().filter(|r| r.stack == name && crate::held::pulled(r) && crate::held::waiting(db, r)) {
        let Some(h) = r.held.as_ref() else { continue };
        waiting.extend(h.opened.iter().flat_map(|n| n.remote.iter().map(String::as_str)));
        waiting.extend(h.replies.iter().filter_map(|rp| rp.remote.as_deref()));
    }
    let versions = |c: &Remote| std::iter::once(c.phid.clone()).chain(c.earlier.iter().map(|v| v.phid.clone())).collect::<Vec<String>>();
    // The comment each version is of, to go up a thread to its first.
    let of: HashMap<String, &Remote> = comments.iter().flat_map(|c| versions(c).into_iter().map(move |v| (v, c))).collect();
    fn first<'a>(of: &HashMap<String, &'a Remote>, c: &'a Remote) -> &'a Remote {
        let mut at = c;
        for _ in 0..of.len() {
            match at.reply_to.as_ref().and_then(|p| of.get(p)) {
                Some(up) => at = up,
                None => break,
            }
        }
        at
    }
    // A thread of the change filed once, and closed since: its id known, no
    // comment open by it, on the patch nor moved to the stack.
    let closed = |c: &Remote| {
        versions(c).iter().any(|v| {
            let id = format!("phab:{v}");
            let moved = on_stack.iter().any(|n| n.moved.as_ref().is_some_and(|m| m.change == change && m.id == id));
            db.note_rev(change, &id).is_some() && db.note(change, &id).is_none() && !moved
        })
    };
    let (mut new, mut edited) = (0, 0);
    for c in comments {
        let all = versions(c);
        if all.iter().any(|v| waiting.contains(v.as_str())) || filed.contains(c.phid.as_str()) {
            continue;
        }
        if c.earlier.iter().any(|v| filed.contains(v.phid.as_str())) {
            edited += 1;
        } else if !closed(first(&of, c)) {
            new += 1;
        }
    }
    (new, edited)
}

/// What `pulled` brings that is new on `name`'s patches, held: each remote
/// thread new, a comment opened, the replies pulled with it turns of it;
/// each reply to a thread there already, or held already, a reply to it;
/// each comment filed and edited on Phabricator since, an edit of its turn.
/// A comment some of which waits for review in a pull is held once that
/// review closes.
pub fn hold(db: &Db, name: &str, pulled: &phab::Pulled) -> Result<Held> {
    let s = db.stack(name)?;
    let mut held = Held::default();
    for rc in &pulled.revisions {
        let Some(p) = s.patches.iter().map(|id| db.patch(id)).find(|p| p.change == rc.source).cloned() else {
            continue;
        };
        let lines = note_lines(&p);
        let before = pulled_before(db, name, &p.change);
        // The thread each remote comment is in, or would be in, filed or
        // held, by any of its versions.
        let mut thread_of: HashMap<String, (String, String)> = before.held.iter().map(|(r, (at, _))| (r.clone(), at.clone())).collect();
        thread_of.extend(before.filed.clone());
        // The threads this pull opens: by each remote comment in them, their
        // place in what it holds.
        let mut opened: HashMap<String, usize> = HashMap::new();
        for c in &rc.comments {
            let versions: Vec<&String> = std::iter::once(&c.phid).chain(c.earlier.iter().map(|v| &v.phid)).collect();
            let waits = versions.iter().any(|v| before.held.get(*v).is_some_and(|(_, open)| *open));
            if waits || before.filed.contains_key(&c.phid) || before.held.contains_key(&c.phid) || opened.contains_key(&c.phid) {
                continue;
            }
            let reply = |at: &(String, String), was: Option<&String>| Reply {
                change: at.0.clone(),
                id: at.1.clone(),
                text: c.text.clone(),
                reviewer: Some(c.author.clone()),
                remote: Some(c.phid.clone()),
                was: was.cloned(),
                ..Default::default()
            };
            // Edited since it was filed: as it was last filed, its turn says.
            if let Some((v, at)) = c.earlier.iter().find_map(|v| before.filed.get(&v.phid).map(|at| (v, at.clone()))) {
                held.replies.push(reply(&at, Some(&v.text)));
                thread_of.extend(versions.iter().map(|v| ((*v).clone(), at.clone())));
                continue;
            }
            // Rejected as it was, or its thread closed since: not again.
            if c.earlier.iter().any(|v| before.held.contains_key(&v.phid)) {
                continue;
            }
            let parent = c.reply_to.as_ref();
            if let Some(k) = parent.and_then(|r| opened.get(r)).copied() {
                let n = &mut held.opened[k];
                n.text = append_turn(&n.text, Role::Reviewer, &c.text, Some(&c.author));
                n.remote.push(c.phid.clone());
                opened.extend(versions.iter().map(|v| ((*v).clone(), k)));
            } else if let Some(at) = parent.and_then(|r| thread_of.get(r)).cloned() {
                held.replies.push(reply(&at, None));
                thread_of.extend(versions.iter().map(|v| ((*v).clone(), at.clone())));
            } else {
                held.opened.push(new_thread(&p, &lines, c));
                opened.extend(versions.iter().map(|v| ((*v).clone(), held.opened.len() - 1)));
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
            let edited = h.replies.iter().filter(|rp| rp.was.is_some()).count();
            let new = h.opened.iter().map(|n| n.remote.len()).sum::<usize>() + h.replies.len() - edited;
            let count = |n: usize, one: &str| match n {
                0 => None,
                1 => Some(format!("1 {one} comment")),
                n => Some(format!("{n} {one} comments")),
            };
            let what: Vec<String> = [count(new, "new"), count(edited, "edited")].into_iter().flatten().collect();
            let summary = match new + edited {
                0 => format!("nothing new from {of}"),
                1 => format!("{} from {of}: it waits for your review", what.join(" and ")),
                _ => format!("{} from {of}: they wait for your review", what.join(" and ")),
            };
            (RunState::Done, summary, (new + edited > 0).then(|| Held { at: now(), ..h }))
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

/// Where a pull is, as the banner says it while it goes.
#[derive(Debug, Clone, PartialEq)]
pub enum Pulling {
    /// Finding the revisions of the stack's patches.
    Starting,
    /// Reading revision `rev`, the `at`th of `of`, from 1.
    Reading { rev: String, at: usize, of: usize },
}

impl Pulling {
    /// What the pull is doing, said: `pulling the reviews from Phabricator:
    /// D12, revision 2 of 3`.
    pub fn said(&self) -> String {
        match self {
            Pulling::Starting => "pulling the reviews from Phabricator".into(),
            Pulling::Reading { rev, at, of } => format!("pulling the reviews from Phabricator: {rev}, revision {at} of {of}"),
        }
    }
}

/// Pulls the reviews of `name`'s patches, the result a run of its own,
/// holding what is new for review.
pub fn pull(db_path: &std::path::Path, name: &str) -> Result<Run> {
    pull_saying(db_path, name, &mut |_| {})
}

/// As `pull`, `say` told where it is as it goes: each revision as it starts
/// reading it.
pub fn pull_saying(db_path: &std::path::Path, name: &str, say: &mut dyn FnMut(Pulling)) -> Result<Run> {
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
        let of = revs.len();
        phab::pull(&revs, &mut |k| {
            say(Pulling::Reading {
                rev: revs[k].1.clone(),
                at: k + 1,
                of,
            })
        })
    };
    Db::edit(db_path, |db| file(db, name, &id, revs.len(), &fetched))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_comments_not_pulled_yet_are_told_from_the_comments_open_the_threads_and_the_pulls_waiting() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("db.jsonl");
        Db::create(&path).unwrap();
        let remote = |phid: &str, reply_to: Option<&str>, earlier: &[&str]| Remote {
            phid: phid.into(),
            text: format!("{phid} says"),
            reply_to: reply_to.map(String::from),
            earlier: earlier.iter().map(|p| crate::phab::Version { phid: p.to_string(), text: String::new() }).collect(),
            ..Default::default()
        };
        let note = |change: &str, id: &str, remote: &[&str], text: &str| Note {
            change: change.into(),
            id: id.into(),
            text: text.into(),
            remote: remote.iter().map(|r| r.to_string()).collect(),
            ..Default::default()
        };
        // On Phabricator: a thread filed and open, a1 and its reply a2
        // pulled with it, a3 a reply since; a thread filed and closed since,
        // b1 and b2; a comment edited since it was filed, e1, once e0; one
        // that waits in a pull's review, w1; a new thread, n1 and its reply
        // n2; and a thread moved to the stack's comments, m1, and a reply
        // to it since, m2.
        let comments = [
            remote("a1", None, &[]),
            remote("a2", Some("a1"), &[]),
            remote("a3", Some("a2"), &[]),
            remote("b1", None, &[]),
            remote("b2", Some("b1"), &[]),
            remote("e1", None, &["e0"]),
            remote("w1", None, &[]),
            remote("n1", None, &[]),
            remote("n2", Some("n1"), &[]),
            remote("m1", None, &[]),
            remote("m2", Some("m1"), &[]),
        ];
        let pull = |id: &str, stack: &str, opened: Note| Run {
            id: id.into(),
            what: "pull".into(),
            stack: stack.into(),
            state: RunState::Done,
            held: Some(Held { opened: vec![opened], ..Default::default() }),
            ..Default::default()
        };
        Db::edit(&path, |db| {
            db.set_note(note("c", "phab:a1", &["a1", "a2"], "Why?"))?;
            db.set_note(note("c", "phab:b1", &["b1", "b2"], "Why not?"))?;
            db.set_note(note("c", "phab:b1", &["b1", "b2"], ""))?;
            db.set_note(note("c", "phab:e0", &["e0"], "Fine."))?;
            db.set_note(note("c", "phab:m1", &["m1"], ""))?;
            db.set_note(Note {
                moved: Some(crate::db::Moved { change: "c".into(), id: "phab:m1".into(), ..Default::default() }),
                ..note(&stack_change("s"), "moved:m1", &["m1"], "Moved.")
            })?;
            db.add_run(pull("pull-1", "s", note("c", "phab:w1", &["w1"], "Hm.")))?;
            // Another stack's pull, holding n1: not this one's.
            db.add_run(pull("pull-2", "t", note("c", "phab:n1", &["n1"], "New.")))?;
            Ok(())
        })
        .unwrap();
        let db = Db::load(&path).unwrap();
        // New: a3, n1, n2 and m2; edited: e1. Neither those filed, nor b2 in
        // its thread closed, nor w1 waiting.
        assert_eq!(not_pulled(&db, "s", "c", &comments), (4, 1));
        assert_eq!(not_pulled(&db, "s", "c", &comments[..3]), (1, 0), "a3 alone");
        assert_eq!(not_pulled(&db, "s", "c", &[]), (0, 0));
        // The pull's review closed, w1 rejected: told as new, as only the
        // history holds it.
        Db::edit(&path, |db| db.set_review("pull-1", crate::held::Review { closed: true, ..Default::default() }).map(|_| ())).unwrap();
        let db = Db::load(&path).unwrap();
        assert_eq!(not_pulled(&db, "s", "c", &comments), (5, 1));
    }

    #[test]
    fn where_a_pull_is_is_said_whole() {
        assert_eq!(Pulling::Starting.said(), "pulling the reviews from Phabricator");
        let reading = Pulling::Reading {
            rev: "D12".into(),
            at: 2,
            of: 3,
        };
        assert_eq!(reading.said(), "pulling the reviews from Phabricator: D12, revision 2 of 3");
    }
}
