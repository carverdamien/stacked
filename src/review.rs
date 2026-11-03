//! Reviews from Phabricator, of every revision the stack's patches name or
//! of one patch's alone, pulled into the database as work held for review,
//! as the agent's is (`held`): a remote thread new, a comment opened on the
//! line it is on (or the patch's title), the replies pulled with it turns
//! of it; a reply to a thread there already, a reply to it. None is
//! rejected: closing the review files each in its thread, a reply to a
//! thread you closed reopening it. A comment pulled is known by the thread
//! that holds it, open or closed, or by the pull waiting for review that
//! holds it, so pulling again holds only what is new. A comment edited on
//! Phabricator since it was filed is held as an edit of its turn: accepted,
//! it is the reviewer's latest turn, the turn it edits redacted. Each
//! version of a comment has an id of its own, so that one pulled before is
//! known by any of them.

use std::collections::{HashMap, HashSet};

use crate::db::{Db, Note, Result, Run, RunState, now, stack_change};
use crate::held::{Closed, Held, Reply};
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

/// What holds the remote comments of the patch of `change`, of stack
/// `name`, already, by the id of each version: a thread, open or closed
/// since, or a pull whose review is still open; each to the comment whose
/// thread it is in, or would be in, (change, id). A comment nothing holds
/// is pulled, whatever a pull did with it before: none is rejected now,
/// and one a stacked of before rejected comes back once.
#[derive(Default)]
struct Before {
    /// In a thread open: on the patch, or moved from it to the stack's
    /// comments.
    filed: HashMap<String, (String, String)>,
    /// In a thread closed since, as it was when closed: the thread, by its
    /// place in `shut`.
    closed: HashMap<String, usize>,
    /// The threads closed since, on the patch or moved from it to the
    /// stack's comments, each where it was last open: as it was when
    /// closed, and the rev of the record that closed it.
    shut: Vec<Closed>,
    /// Each place one of those threads was at, to it in `shut`.
    places: HashMap<(String, String), usize>,
    /// Held by a pull whose review is still open: pulled once that review
    /// closes, if no thread holds it then.
    waiting: HashMap<String, (String, String)>,
}

impl Before {
    /// Where the thread closed since that holds remote comment `id` is, if
    /// one does.
    fn closed_at(&self, id: &str) -> Option<(String, String)> {
        self.closed.get(id).map(|&k| place(&self.shut[k].note))
    }
}

/// Where comment `n` is: its change and id.
fn place(n: &Note) -> (String, String) {
    (n.change.clone(), n.id.clone())
}

/// The comment reply `rp` goes to: its change and id.
fn place_of(rp: &Reply) -> (String, String) {
    (rp.change.clone(), rp.id.clone())
}

/// What holds the remote comments of the patch of `change` already
/// (`Before`). The threads closed since are read from the history, as a
/// pull reads them.
fn pulled_before(db: &Db, name: &str, change: &str) -> Before {
    let mut before = Before::default();
    let stack = stack_change(name);
    let from_here = |n: &Note| n.moved.as_ref().is_some_and(|m| m.change == change);
    let moved: Vec<&Note> = db.notes(&stack).into_iter().filter(|n| from_here(n)).collect();
    for n in db.notes(change).into_iter().chain(moved.iter().copied()) {
        for r in &n.remote {
            before.filed.insert(r.clone(), place(n));
        }
    }
    // The threads closed since: the patch's, and those moved from it to the
    // stack's comments. One moved there and closed, or moved back and
    // closed, was at both places: it is where it was closed last. One whose
    // comments an open comment holds, moved with its lines to another patch
    // say, is not closed.
    let mut closed = db.deleted_notes(name, change);
    closed.extend(db.deleted_notes(name, &stack).into_iter().filter(|(_, n)| from_here(n)));
    closed.sort_by_key(|(rev, _)| *rev);
    let open: HashSet<&str> = db.stack_notes(name).into_iter().flat_map(|n| n.remote.iter().map(String::as_str)).collect();
    let mut threads: HashMap<(String, String), usize> = HashMap::new();
    for (rev, n) in closed {
        if n.remote.is_empty() || n.remote.iter().any(|r| open.contains(r.as_str())) {
            continue;
        }
        let origin = n.moved.as_ref().map_or_else(|| place(&n), |m| (m.change.clone(), m.id.clone()));
        let k = *threads.entry(origin).or_insert(before.shut.len());
        before.places.insert(place(&n), k);
        for r in &n.remote {
            before.closed.insert(r.clone(), k);
        }
        let thread = Closed { rev, note: n };
        match before.shut.get_mut(k) {
            Some(was) => *was = thread,
            None => before.shut.push(thread),
        }
    }
    // The pulls whose review is still open: where each comment they hold
    // goes, on the patch or in a thread moved from it.
    let ours: HashSet<(String, String)> = moved.iter().copied().map(place).chain(before.places.keys().cloned()).collect();
    for r in db.live_runs().into_iter().filter(|r| crate::held::pulled(r) && crate::held::waiting(db, r)) {
        let Some(h) = r.held.as_ref() else { continue };
        for n in h.opened.iter().filter(|n| n.change == change) {
            for id in &n.remote {
                before.waiting.insert(id.clone(), place(n));
            }
        }
        for rp in h.replies.iter().filter(|rp| rp.change == change || ours.contains(&place_of(rp))) {
            if let Some(id) = &rp.remote {
                before.waiting.insert(id.clone(), place_of(rp));
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
/// there is not counted, nor one of a thread closed: without the history,
/// a new reply or edit there is not told from the comments filed in it
/// before it was closed, though a pull brings the new ones.
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
/// each reply to a thread there already, open or closed since, or held
/// already, a reply to it; each comment filed and edited on Phabricator
/// since, an edit of its turn. A reply or an edit in a thread closed since
/// keeps the thread as it was when closed, for its review to show it and
/// accepting it to reopen it. A comment some of which waits for review in a
/// pull is held once that review closes.
pub fn hold(db: &Db, name: &str, pulled: &phab::Pulled) -> Result<Held> {
    let s = db.stack(name)?;
    let mut held = Held::default();
    for rc in &pulled.revisions {
        let Some(p) = s.patches.iter().map(|id| db.patch(id)).find(|p| p.change == rc.source).cloned() else {
            continue;
        };
        let lines = note_lines(&p);
        let before = pulled_before(db, name, &p.change);
        // The thread each remote comment is in, or would be in, by any of
        // its versions: a thread open, one closed since where it is, or where
        // a pull waiting for review holds it.
        let mut thread_of: HashMap<String, (String, String)> = before.waiting.clone();
        thread_of.extend(before.closed.iter().map(|(r, &k)| (r.clone(), place(&before.shut[k].note))));
        thread_of.extend(before.filed.clone());
        // The threads this pull opens: by each remote comment in them, their
        // place in what it holds.
        let mut opened: HashMap<String, usize> = HashMap::new();
        for c in &rc.comments {
            let versions: Vec<&String> = std::iter::once(&c.phid).chain(c.earlier.iter().map(|v| &v.phid)).collect();
            let waits = versions.iter().any(|v| before.waiting.contains_key(*v));
            let filed = before.filed.contains_key(&c.phid) || before.closed.contains_key(&c.phid);
            if waits || filed || opened.contains_key(&c.phid) {
                continue;
            }
            // Into the thread at `at`; closed since, into it where it is, kept
            // with the reply as it was when closed.
            let reply = |at: &(String, String), was: Option<&String>| {
                let closed = before.places.get(at).map(|&k| before.shut[k].clone());
                let (change, id) = closed.as_ref().map_or_else(|| at.clone(), |t| place(&t.note));
                Reply {
                    change,
                    id,
                    text: c.text.clone(),
                    reviewer: Some(c.author.clone()),
                    remote: Some(c.phid.clone()),
                    was: was.cloned(),
                    closed,
                    ..Default::default()
                }
            };
            // Edited since it was filed, its thread open or closed since: as it
            // was last filed, its turn says.
            let filed_as = c.earlier.iter().find_map(|v| before.filed.get(&v.phid).cloned().or_else(|| before.closed_at(&v.phid)).map(|at| (v, at)));
            if let Some((v, at)) = filed_as {
                let rp = reply(&at, Some(&v.text));
                thread_of.extend(versions.iter().map(|v| ((*v).clone(), place_of(&rp))));
                held.replies.push(rp);
                continue;
            }
            let parent = c.reply_to.as_ref();
            if let Some(k) = parent.and_then(|r| opened.get(r)).copied() {
                let n = &mut held.opened[k];
                n.text = append_turn(&n.text, Role::Reviewer, &c.text, Some(&c.author));
                n.remote.push(c.phid.clone());
                opened.extend(versions.iter().map(|v| ((*v).clone(), k)));
            } else if let Some(at) = parent.and_then(|r| thread_of.get(r)).cloned() {
                let rp = reply(&at, None);
                thread_of.extend(versions.iter().map(|v| ((*v).clone(), place_of(&rp))));
                held.replies.push(rp);
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
    file_from(db, name, id, &of, fetched)
}

/// As `file`, the revisions read said as `of`: `3 revisions`, or `D12`
/// for a pull of one patch's revision alone.
fn file_from(db: &mut Db, name: &str, id: &str, of: &str, fetched: &std::result::Result<phab::Pulled, String>) -> Result<Run> {
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
    /// Reading revision `rev`, the revision of one patch, pulled alone.
    Alone(String),
}

impl Pulling {
    /// What the pull is doing, said: `pulling the reviews from Phabricator:
    /// D12, revision 2 of 3`; of one patch's revision alone, `pulling the
    /// reviews from Phabricator: D12`.
    pub fn said(&self) -> String {
        match self {
            Pulling::Starting => "pulling the reviews from Phabricator".into(),
            Pulling::Reading { rev, at, of } => format!("pulling the reviews from Phabricator: {rev}, revision {at} of {of}"),
            Pulling::Alone(rev) => format!("pulling the reviews from Phabricator: {rev}"),
        }
    }
}

/// Pulls the reviews of `name`'s patches, the result a run of its own,
/// holding what is new for review; the comments of each revision read
/// stored too, as the Phabricator line counts them (`phab::store_pulled`).
pub fn pull(db_path: &std::path::Path, name: &str) -> Result<Run> {
    pull_saying(db_path, name, &mut |_| {})
}

/// As `pull`, `say` told where it is as it goes: each revision as it starts
/// reading it.
pub fn pull_saying(db_path: &std::path::Path, name: &str, say: &mut dyn FnMut(Pulling)) -> Result<Run> {
    pull_of(db_path, name, None, say)
}

/// As `pull_saying`, of the revision of the patch of `change` alone, as
/// the editor's Phabricator line and the patch's menu pull it: that
/// revision read, as a pull of the whole stack reads each, what is new on
/// it held for review the same way.
pub fn pull_patch_saying(db_path: &std::path::Path, name: &str, change: &str, say: &mut dyn FnMut(Pulling)) -> Result<Run> {
    pull_of(db_path, name, Some(change), say)
}

/// The pull of the revisions of `name`'s patches, those of the patch of
/// change `only` alone when given; `say` told where it is as it goes.
fn pull_of(db_path: &std::path::Path, name: &str, only: Option<&str>, say: &mut dyn FnMut(Pulling)) -> Result<Run> {
    if crate::config::get().reviews.source == crate::config::Source::None {
        return crate::db::err(format!(
            "reviews come from nowhere: reviews.source is \"none\" in {}",
            crate::config::path().display()
        ));
    }
    let mut revs = revisions(&Db::load(db_path)?, name)?;
    if let Some(change) = only {
        revs.retain(|(c, _, _)| c == change);
    }
    let id = format!("pull-{}", crate::db::unique());
    let fetched = match (revs.is_empty(), only) {
        (true, None) => Err("no patch of the stack has a Differential Revision".to_string()),
        (true, Some(_)) => Err("the patch has no Differential Revision, or is no longer in the stack".to_string()),
        (false, None) => {
            let of = revs.len();
            phab::pull(&revs, &mut |k| {
                say(Pulling::Reading {
                    rev: revs[k].1.clone(),
                    at: k + 1,
                    of,
                })
            })
        }
        (false, Some(_)) => phab::pull(&revs, &mut |k| say(Pulling::Alone(revs[k].1.clone()))),
    };
    let of = match (only, revs.as_slice()) {
        (Some(_), [(_, rev, _)]) => rev.clone(),
        _ => format!("{} revision{}", revs.len(), if revs.len() == 1 { "" } else { "s" }),
    };
    // What it read stored too: each revision's comments, as the Phabricator
    // line counts those not pulled yet.
    Db::edit(db_path, |db| {
        if let Ok(pulled) = &fetched {
            phab::store_pulled(db, &revs, pulled)?;
        }
        file_from(db, name, &id, &of, &fetched)
    })
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
        // In the thread closed, b2 edited since and a reply to it, b3: not
        // counted either, though a pull brings them.
        let mut since = comments.to_vec();
        since[4] = remote("b2v2", Some("b1"), &["b2"]);
        since.push(remote("b3", Some("b2v2"), &[]));
        assert_eq!(not_pulled(&db, "s", "c", &since), (4, 1));
        // The pull's review closed, w1 filed nowhere, as a stacked of before
        // let you reject it: told as new, as a pull brings it again.
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
        // One patch's revision, pulled alone: it alone, named.
        assert_eq!(Pulling::Alone("D12".into()).said(), "pulling the reviews from Phabricator: D12");
    }
}
