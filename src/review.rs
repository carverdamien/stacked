//! A pull from Phabricator: of every revision the stack's patches name, of
//! one patch's alone, or of the revision a new stack is pulled from, read
//! with the revisions they stack on that the stack does not hold, and those
//! stacked on top; their patches and their reviewers' comments, as one job,
//! held for one review, as the agent's work is (`held`).
//!
//! What is read is stored first, as a fetch stores it (`phab::store_read`);
//! then the pull waits its turn as an edit of the agent's does
//! (`ask::claim`): at its turn, it is made (`make`), from the stack the
//! edits before it make, chained on the last of them waiting for your
//! review as edits are (`Job::after`), on a copy of the database with no
//! lock held, what each operation of its change needs worked out there
//! too, then filed, and it waits for your review in turn. Its change of
//! the stack takes, as a new version of a patch, its
//! revision's latest diff, or its title and summary, where they are no
//! version of the patch stacked knows; adds the revisions the stack does
//! not hold, a parent below the patch of the revision it is the parent of,
//! one stacked on another above that one's patch; and makes again on the
//! pulled versions a patch above them that no longer applies, its
//! conflicts marked where they disagree, so that the stack it holds still
//! applies; its review closes with the change accepted only once no
//! conflict marked is left in it. What it read that changes nothing is
//! said in its review: a revision that landed or was abandoned, and the
//! parents Phabricator gives that the stack's order disagrees with, which
//! is kept.
//!
//! The comments: a remote thread new is a comment opened on the line it is
//! on, in the patch as the pull leaves it (or the patch's title), the
//! replies pulled with it turns of it; a reply to a thread there already, a
//! reply to it. None is rejected: closing the review files each in its
//! thread, a reply to a thread you closed reopening it. A comment pulled is
//! known by the thread that holds it, open or closed, or by the pull
//! waiting for review that holds it, so pulling again holds only what is
//! new. A comment edited on Phabricator since it was filed is held as an
//! edit of its turn: accepted, it is the reviewer's latest turn, the turn
//! it edits redacted. Each version of a comment has an id of its own, so
//! that one pulled before is known by any of them.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::db::{Db, Error, Note, Patch, Result, Run, RunState, Stack, err, now, stack_change};
use crate::held::{Added, Change, Closed, Held, Reply, Review, Taken, Took};
use crate::mbox::Draft;
use crate::phab::{self, PullRead, Reached, Read, Remote, Wants};
use crate::thread::{Role, append_turn, fenced};
use crate::view::{MSG_PATH, note_lines, title_key};

/// What a pull's run is, as `Run::what` says it.
pub const WHAT: &str = "pull";

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
    let patches: Vec<Patch> = s.patches.iter().map(|id| db.patch(id).clone()).collect();
    hold_on(db, name, &patches, pulled)
}

/// As `hold`, on the patches `patches` of stack `name`: those the pull's
/// change leaves, the comments anchored on them, each revision's on the
/// patch of the change its comments are of (`RevisionComments::source`).
fn hold_on(db: &Db, name: &str, patches: &[Patch], pulled: &phab::Pulled) -> Result<Held> {
    let mut held = Held::default();
    for rc in &pulled.revisions {
        let Some(p) = patches.iter().find(|p| p.change == rc.source).cloned() else {
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

/// Files the comments a pull read of `name`'s patches, on `revisions`
/// revisions, `fetched`, as run `id`, a pull that brings no patch: what is
/// new held in it, to review; or, `fetched` an error, the pull failed.
pub fn file(db: &mut Db, name: &str, id: &str, revisions: usize, fetched: &std::result::Result<phab::Pulled, String>) -> Result<Run> {
    let of = format!("{revisions} revision{}", if revisions == 1 { "" } else { "s" });
    let (state, summary, held) = match fetched {
        Ok(pulled) => {
            let h = hold(db, name, pulled)?;
            let (new, edited) = comments_counted(&h);
            let summary = summary_of(&Brought { new, edited, ..Default::default() }, &of);
            (RunState::Done, summary, (new + edited > 0).then(|| Held { at: now(), ..h }))
        }
        Err(e) => (RunState::Failed, e.clone(), None),
    };
    let run = Run {
        id: id.into(),
        what: WHAT.into(),
        stack: name.into(),
        state,
        summary,
        held,
        ..Default::default()
    };
    db.add_run(run.clone())?;
    Ok(run)
}

/// How many of the comments `h` holds are new, and how many are edits of
/// comments filed: each new thread's comments, each reply.
fn comments_counted(h: &Held) -> (usize, usize) {
    let edited = h.replies.iter().filter(|rp| rp.was.is_some()).count();
    (h.opened.iter().map(|n| n.remote.len()).sum::<usize>() + h.replies.len() - edited, edited)
}

/// What a pull brings, counted, as what came of it says it: new versions of
/// the stack's patches, new patches, new comments and edited ones; and the
/// patches it made again on the pulled versions with their conflicts
/// marked, by their places, from 0.
#[derive(Debug, Clone, Default)]
struct Brought {
    versions: usize,
    added: usize,
    new: usize,
    edited: usize,
    marked: Vec<usize>,
}

/// `n` of `one`, or of `many`.
fn count(n: usize, one: &str, many: &str) -> String {
    if n == 1 { format!("1 {one}") } else { format!("{n} {many}") }
}

/// What came of a pull that read `of` (`3 revisions`, `D12`) and brought
/// `b`, said: `1 new version, 1 new patch and 2 new comments from 3
/// revisions: they wait for your review`; `nothing new from 3 revisions`.
/// The patches whose conflicts it marked after it.
fn summary_of(b: &Brought, of: &str) -> String {
    let mut what: Vec<String> = Vec::new();
    for (n, one, many) in [
        (b.versions, "new version", "new versions"),
        (b.added, "new patch", "new patches"),
        (b.new, "new comment", "new comments"),
        (b.edited, "edited comment", "edited comments"),
    ] {
        if n > 0 {
            what.push(count(n, one, many));
        }
    }
    let total = b.versions + b.added + b.new + b.edited;
    let mut out = match total {
        0 => return format!("nothing new from {of}"),
        1 => format!("{} from {of}: it waits for your review", crate::submit::and(&what)),
        _ => format!("{} from {of}: they wait for your review", crate::submit::and(&what)),
    };
    match b.marked.len() {
        0 => {}
        1 => out += &format!("; {} no longer applies on the pulled versions: its conflicts are marked, to resolve", crate::head::patches_said(&b.marked)),
        _ => out += &format!("; {} no longer apply on the pulled versions: their conflicts are marked, to resolve", crate::head::patches_said(&b.marked)),
    }
    out
}

/// Where a pull is, as the banner says it while it reads Phabricator.
#[derive(Debug, Clone, PartialEq)]
pub enum Pulling {
    /// Finding the revisions to read: the stack's, their parents, and those
    /// stacked on top.
    Starting,
    /// Reading revision `rev`, the `at`th of `of`, from 1.
    Reading { rev: String, at: usize, of: usize },
    /// Finding the revisions to read, from revision `rev`: one patch's, or
    /// a new stack's.
    Alone(String),
}

impl Pulling {
    /// What the pull is doing, said: `pulling from Phabricator: D12,
    /// revision 2 of 3`; from one revision, `pulling from Phabricator:
    /// D12`.
    pub fn said(&self) -> String {
        match self {
            Pulling::Starting => "pulling from Phabricator".into(),
            Pulling::Reading { rev, at, of } => format!("pulling from Phabricator: {rev}, revision {at} of {of}"),
            Pulling::Alone(rev) => format!("pulling from Phabricator: {rev}"),
        }
    }
}

/// What a pull is asked to pull.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Asked {
    /// Every revision the stack's patches name, with the revisions they
    /// stack on that the stack does not hold, and those stacked on top.
    Stack,
    /// The revision of the patch of `change`, with the revisions it stacks
    /// on that the stack does not hold; with `children`, and those stacked
    /// on it.
    Patch { change: String, children: bool },
    /// Revision `D123`, for a new stack: with the revisions it stacks on,
    /// and those stacked on it.
    New(String),
}

/// A pull, as its run keeps it from when it is asked to when it is made:
/// what it was asked; what it read of Phabricator, each revision in the
/// order it read them (`phab::Read`), their raw diffs stored; made chained,
/// the landed work it went on from, guessing it is accepted, while that
/// work waits for your review: its change is made from the stack that work
/// makes, and reviewed after it; and, made with a change of the stack, what
/// each patch of it takes from Phabricator and what the pull found that
/// changes nothing (`held::Took`), which its review shows.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Job {
    pub asked: Asked,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub read: Vec<Read>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub took: Option<Took>,
}

/// Whether run `r` is a pull's.
pub fn is(r: &Run) -> bool {
    crate::held::pulled(r)
}

/// What a pull asked, as its run's request says it: `the stack's
/// revisions, with their parents and those stacked on top`; `D12, with its
/// parents`.
fn asked_said(asked: &Asked, d: &str) -> String {
    match asked {
        Asked::Stack => "the stack's revisions, with their parents and those stacked on top".into(),
        Asked::Patch { children: false, .. } => format!("{d}, with its parents"),
        Asked::Patch { children: true, .. } | Asked::New(_) => format!("{d}, with its parents and the revisions stacked on it"),
    }
}

/// The pull of stack `name` waiting its turn, if one does: one at a time.
pub fn pending<'a>(db: &'a Db, name: &str) -> Option<&'a Run> {
    db.live_runs().into_iter().find(|r| r.stack == name && is(r) && r.state == RunState::Started && r.pull.is_some())
}

/// The pulls waiting their turn, the first asked first: each its run, and
/// its stack.
pub fn waiting(db: &Db) -> Vec<(String, String)> {
    db.live_runs()
        .into_iter()
        .filter(|r| is(r) && r.state == RunState::Started && r.pull.is_some())
        .map(|r| (r.id.clone(), r.stack.clone()))
        .collect()
}

/// What refusing another pull of a stack whose pull waits its turn says.
pub const ONE_AT_A_TIME: &str = "a pull from Phabricator of this stack waits its turn already: one at a time";

/// Why stack `name` is not pulled now, if it is not: a pull of it waits its
/// turn already.
pub fn not_now(db: &Db, name: &str) -> Option<String> {
    pending(db, name).map(|_| ONE_AT_A_TIME.to_string())
}

/// What came of pull `r`, said: once made, its summary; waiting its turn,
/// that it does, and what for.
pub fn said(db: &Db, r: &Run) -> String {
    match r.state {
        RunState::Started => format!(
            "the pull from Phabricator waits its turn: it is made {}, and what it brings then waits for your review",
            crate::ask::pull_waits_on(db, &r.stack).unwrap_or_else(|| "next".into())
        ),
        _ => r.summary.clone(),
    }
}

/// What refusing a pull says while reviews come from nowhere.
fn nowhere(what: &str) -> Error {
    Error(format!("{what} come from nowhere: reviews.source is \"none\" in {}", crate::config::path().display()))
}

/// Pulls from Phabricator what `asked` asks of stack `name` in the database
/// at `db_path`, `say` told where it is as it reads: the revisions read
/// (`phab::read_pull`), with the revisions they stack on that the stack
/// does not hold, and, of the stack's top or of the patch asked with
/// them, those stacked on them; what is read stored, as a fetch stores it;
/// then the pull queued, as an edit is, and made if its turn has come
/// (`ask::launch`). The run, made or waiting its turn. Nothing is stored
/// or queued when the read fails, Phabricator turning this machine away
/// said plainly; nor while reviews come from nowhere, nor while a pull of
/// the stack waits its turn.
pub fn pull(db_path: &Path, name: &str, asked: Asked, say: &mut dyn FnMut(Pulling)) -> Result<Run> {
    if crate::config::get().reviews.source == crate::config::Source::None {
        return Err(nowhere("reviews"));
    }
    let db = Db::load(db_path)?;
    let s = db.stack(name)?.clone();
    if let Some(why) = not_now(&db, name) {
        return err(why);
    }
    // The revisions the stack's patches name, bottom first, each once.
    let mut named: Vec<(String, String)> = Vec::new();
    for id in &s.patches {
        if let Some((d, l)) = phab::revision_of(&db.patch(id).message)
            && !named.iter().any(|(_, x)| *x == l)
        {
            named.push((d, l));
        }
    }
    let (wanted, up) = match &asked {
        Asked::Stack => (named.clone(), named.last().map(|(_, l)| l.clone())),
        Asked::Patch { change, children } => {
            let p = s.patches.iter().map(|id| db.patch(id)).find(|p| p.change == *change);
            let one: Vec<(String, String)> = p.and_then(|p| phab::revision_of(&p.message)).into_iter().collect();
            let up = one.first().filter(|_| *children).map(|(_, l)| l.clone());
            (one, up)
        }
        Asked::New(_) => return err("a new stack is pulled from Phabricator with `stacked moz-phab import`"),
    };
    if wanted.is_empty() {
        return err(match asked {
            Asked::Stack => "no patch of the stack has a Differential Revision",
            _ => "the patch has no Differential Revision, or is no longer in the stack",
        });
    }
    // What the stack holds: its own revisions; chained, those the change
    // waiting for your review that it would go on from makes it hold too.
    let mut held: Vec<String> = named.iter().map(|(_, l)| l.clone()).collect();
    if crate::ask::chained(&db, name)
        && let Some(w) = crate::held::queue(&db, name).into_iter().rev().find(|r| r.held.as_ref().is_some_and(|h| h.stack.is_some()))
        && let Ok(ws) = crate::ask::work_stack(&db, &w.id)
    {
        held.extend(ws.patches.iter().filter_map(|id| phab::revision_of(&db.patch(id).message)).map(|(_, l)| l));
    }
    let d = wanted.first().map(|(d, _)| d.clone()).unwrap_or_default();
    say(match &asked {
        Asked::Patch { .. } => Pulling::Alone(d.clone()),
        _ => Pulling::Starting,
    });
    let read = read_with(db_path, &db, Some(&s), &Wants { asked: wanted, held, up }, say)?;
    drop(db);
    let id = format!("pull-{}", crate::db::unique());
    let job = Job { asked, read: read.revisions.clone(), after: None, took: None };
    Db::edit(db_path, |db| {
        if let Some(why) = not_now(db, name) {
            return err(why);
        }
        phab::store_read(db, &read)?;
        db.add_run(Run {
            id: id.clone(),
            what: WHAT.into(),
            stack: name.into(),
            state: RunState::Started,
            request: asked_said(&job.asked, &d),
            pull: Some(Box::new(job.clone())),
            ..Default::default()
        })
        .map(|_| ())
    })?;
    crate::ask::launch(db_path)?;
    let db = Db::load(db_path)?;
    db.run(&id).cloned().ok_or_else(|| Error(format!("run {id} is gone")))
}

/// `stacked moz-phab import`, and the editor's `Pull from Phabricator…` with
/// no stack, or under `New stack…`: the revision `text` names (`D123`,
/// `123`, or its link) pulled from Phabricator into a new stack of the
/// database at `db_path`, in the checkout at `at`, named `name`, else after
/// the revision as no stack is; `say` told where it is as it reads. With the
/// revisions it stacks on, down to the first that is not open, and those
/// stacked on it while one is open; refused, nothing made, when a stack
/// holds any of them already, or the work waiting for your review on a
/// stack adds one (`held_where`), or when the bottom one is on a commit the
/// repository does not have. The stack is made empty, on the commit the
/// bottom one is on, and the pull lands into it for review, as a change
/// that adds each patch: accepting it makes them. The run, made or waiting
/// its turn.
pub fn import(db_path: &Path, at: &Path, text: &str, name: Option<&str>, say: &mut dyn FnMut(Pulling)) -> Result<Run> {
    if crate::config::get().reviews.source == crate::config::Source::None {
        return Err(nowhere("revisions"));
    }
    let Some((rev, link)) = phab::revision_named(text) else {
        return err(format!("{text:?} names no revision: give D123, 123, or its link"));
    };
    let db = Db::load(db_path)?;
    if let Some(n) = name
        && let Some(why) = crate::import::name_wrong(&db, n)
    {
        return err(why);
    }
    let link = match link {
        Some(l) => l,
        None => format!("{}/{rev}", phabricator_at(at).ok_or_else(|| Error(format!("{rev} names no Phabricator: give its link, or set reviews.url")))?),
    };
    if let Some(why) = held_elsewhere(&db, &rev, &link) {
        return err(why);
    }
    let Ok(repo) = crate::git::toplevel(at) else {
        return err("not in a git repository: a stack is pulled into the checkout stacked runs in");
    };
    say(Pulling::Alone(rev.clone()));
    let read = read_with(db_path, &db, None, &Wants { asked: vec![(rev.clone(), link.clone())], held: vec![], up: Some(link) }, say)?;
    if let Some(why) = read.revisions.iter().find_map(|r| held_elsewhere(&db, &r.d, &r.link)) {
        return err(why);
    }
    let Some(asked) = read.revisions.first() else {
        return err(format!("Phabricator has no revision {rev}"));
    };
    // The bottom one: the last parent the walk down found.
    let bottom = read.revisions.iter().rev().find(|r| matches!(r.reached, Reached::Parent(_))).unwrap_or(asked);
    let Some(base) = bottom.base.clone() else {
        return err(format!("{} does not say what commit it is on", bottom.d));
    };
    let Ok(base) = crate::git::rev_parse(at, &base) else {
        return err(format!("{} is on {}, which this repository does not have: fetch it first", bottom.d, &base[..12.min(base.len())]));
    };
    drop(db);
    let id = format!("pull-{}", crate::db::unique());
    let job = Job { asked: Asked::New(rev.clone()), read: read.revisions.clone(), after: None, took: None };
    Db::edit(db_path, |db| {
        let name = match name {
            Some(n) => n.trim().to_string(),
            None => crate::import::free_name(db, &rev),
        };
        if let Some(why) = crate::import::name_wrong(db, &name) {
            return err(why);
        }
        if let Some(why) = job.read.iter().find_map(|r| held_elsewhere(db, &r.d, &r.link)) {
            return err(why);
        }
        db.new_stack(&name, &base, &repo)?;
        phab::store_read(db, &read)?;
        db.add_run(Run {
            id: id.clone(),
            what: WHAT.into(),
            stack: name.clone(),
            state: RunState::Started,
            request: asked_said(&job.asked, &rev),
            pull: Some(Box::new(job.clone())),
            ..Default::default()
        })
        .map(|_| ())
    })?;
    crate::ask::launch(db_path)?;
    let db = Db::load(db_path)?;
    db.run(&id).cloned().ok_or_else(|| Error(format!("run {id} is gone")))
}

/// The Phabricator a bare `D123` is on, `https://phabricator.example.com`,
/// pulled into the checkout at `at`: the settings' (`reviews.url`), else the
/// one its `.arcconfig` names. None for neither.
fn phabricator_at(at: &Path) -> Option<String> {
    let url = crate::config::get().reviews.url.clone().or_else(|| crate::submit::arcconfig(at).uri)?;
    Some(url.trim_end_matches('/').to_string())
}

/// Why revision `d`, at `link`, is not pulled into a new stack, if a stack
/// holds it already (`held_where`).
fn held_elsewhere(db: &Db, d: &str, link: &str) -> Option<String> {
    held_where(db, d, &|_, l| l == link)
}

/// As `held_elsewhere`, revision `d` known by its link if given, else by
/// its `D123` on any Phabricator: what the editor says as it is typed.
pub fn held_by(db: &Db, d: &str, link: Option<&str>) -> Option<String> {
    match link {
        Some(l) => held_elsewhere(db, d, l),
        None => held_where(db, d, &|x, _| x == d),
    }
}

/// Why revision `d`, as `named` knows it by its `D123` and its link, is not
/// pulled into a new stack, if a stack holds it already: which patch of
/// which stack it is; or which it would be, once the work waiting for your
/// review on a stack that adds it is accepted, as a pull into a new stack
/// waits for it.
fn held_where(db: &Db, d: &str, named: &dyn Fn(&str, &str) -> bool) -> Option<String> {
    let at = |patches: &[String]| patches.iter().position(|id| phab::revision_of(&db.patch(id).message).is_some_and(|(x, l)| named(&x, &l)));
    if let Some(why) = db.stacks().into_iter().find_map(|s| Some(format!("{d} is patch {} of {}: pull it there", at(&s.patches)? + 1, s.name))) {
        return Some(why);
    }
    db.live_runs().into_iter().filter(|r| crate::held::waiting(db, r)).find_map(|r| {
        let k = at(&r.held.as_ref()?.stack.as_ref()?.patches)?;
        Some(format!("{d} would be patch {} of {} once the work waiting for your review there is accepted: review that work first", k + 1, r.stack))
    })
}

/// What a pull reads of Phabricator for stack `s` (none for a new stack),
/// as `wants` asks (`phab::read_pull`), the database at `db_path` read as
/// `db`: whether a revision's raw diff is a version of the patch that names
/// it, which stacked knows (`submit::known`), and the raw diffs stored, not
/// read again. What went wrong, said; Phabricator turning this machine away
/// said plainly.
fn read_with(db_path: &Path, db: &Db, s: Option<&Stack>, wants: &Wants, say: &mut dyn FnMut(Pulling)) -> Result<PullRead> {
    let at = |link: &str| s.and_then(|s| s.patches.iter().position(|id| phab::revision_of(&db.patch(id).message).is_some_and(|(_, l)| l == link)));
    let known = |link: &str, raw: &str| -> Option<bool> {
        let s = s?;
        Some(crate::submit::known(db, s, at(link)?, raw).is_some())
    };
    let stored = |link: &str, id: u64| db.raw_diff(link, id).map(String::from);
    phab::read_pull(db_path, wants, &known, &stored, &mut |d, k, n| say(Pulling::Reading { rev: d.into(), at: k, of: n })).map_err(|e| Error(e.to_string()))
}

/// From `ask::launch`, under the launch lock, its turn come (`ask::claim`):
/// pull `id` made, from the stack the work `after` makes, if it is chained
/// on that work, else from the stack as it is (`plan`): the new versions,
/// the patches added, the patches above made again on what changed under
/// them. That, and what each operation of the change it makes needs
/// (`operations::Ops::needs`), are worked out on a copy of the database,
/// with no lock held, as their checks read the repository and may take
/// long on a big one. Then, under the lock, it is filed: its patches, the
/// comments of each revision held, anchored on the patches as the pull
/// leaves them, and what each operation needs (`held::needs_filed`); it
/// waits for your review, or, with nothing new, lands with nothing to
/// review, as said. Whether it was filed: not when the stack, or the work
/// it goes on from, changed meanwhile, as it is made again then, at its
/// next turn. A pull that cannot be made fails, saying why.
pub(crate) fn make(db_path: &Path, id: &str, after: Option<String>) -> Result<bool> {
    let mut dry = Db::load(db_path)?;
    let Some(r) = dry.run(id).cloned().filter(|r| r.state == RunState::Started) else { return Ok(true) };
    let Some(job) = r.pull.as_deref().cloned() else { return Ok(true) };
    let Some((rev, now_s)) = dry.head(&r.stack).map(|(rev, s)| (rev, s.clone())) else {
        return failed(db_path, id, format!("the stack {} is gone: nothing was pulled", r.stack));
    };
    // The work it goes on from gone meanwhile: from the stack as it is.
    let (from, after) = match after.and_then(|a| Some((crate::ask::work_stack(&dry, &a).ok()?, a))) {
        Some((s, a)) => (s, Some(a)),
        None => (now_s, None),
    };
    let planned = match plan(&mut dry, &from, &job) {
        Ok(p) => p,
        Err(e) => return failed(db_path, id, format!("the pull could not be made: {}", e.0)),
    };
    let next = Stack { patches: planned.patches.clone(), ..from.clone() };
    let needs = (planned.patches != from.patches).then(|| crate::operations::of(&dry, &from, &next).needs(&dry));
    let patches: Vec<Patch> = planned.patches.iter().map(|p| dry.patch(p).clone()).collect();
    drop(dry);
    Db::edit(db_path, |db| {
        let Some(r) = db.run(id).cloned().filter(|r| r.state == RunState::Started) else { return Ok(true) };
        // Made from what changed meanwhile: made again.
        let still = match &after {
            Some(a) => db.run(a).is_some_and(|w| crate::held::waiting(db, w)),
            None => db.head(&r.stack).map(|(rev, _)| rev) == Some(rev),
        };
        if !still {
            return Ok(false);
        }
        for p in &patches {
            db.add_patch(p.clone())?;
        }
        file_made(db, r, job.clone(), &planned, &patches, &from, rev, after.clone(), needs.clone())?;
        Ok(true)
    })
}

/// Pull `id`, still waiting its turn, failed, `why` its summary: filed.
fn failed(db_path: &Path, id: &str, why: String) -> Result<bool> {
    Db::edit(db_path, |db| {
        if let Some(r) = db.run(id).cloned().filter(|r| r.state == RunState::Started) {
            db.add_run(Run { state: RunState::Failed, summary: why, ..r })?;
        }
        Ok(true)
    })
}

/// Under the lock: pull `r`, as `job` asked it, made as `planned` plans it
/// from stack `from` (`patches`, those it leaves, stored), the stack at
/// `rev` unless it goes on from the work `after`: the comments of each
/// revision held, anchored on those patches; what each operation of its
/// change needs, `needs`, worked out before, stored with it if it is for
/// that change (`held::needs_filed`); and its run filed, waiting for your
/// review, or done with nothing new.
#[allow(clippy::too_many_arguments)]
fn file_made(db: &mut Db, r: Run, job: Job, planned: &Planned, patches: &[Patch], from: &Stack, rev: usize, after: Option<String>, needs: Option<crate::operations::Needs>) -> Result<()> {
    let pulled = phab::Pulled {
        revisions: job
            .read
            .iter()
            .enumerate()
            .filter_map(|(i, rd)| {
                Some(phab::RevisionComments {
                    revision: rd.d.clone(),
                    source: planned.changes.get(&i)?.clone(),
                    comments: rd.comments.clone(),
                })
            })
            .collect(),
    };
    let h = hold_on(db, &r.stack, patches, &pulled)?;
    let (new, edited) = comments_counted(&h);
    let changed = planned.patches != from.patches;
    let brought = Brought {
        new,
        edited,
        ..match changed {
            true => planned.brought.clone(),
            false => Brought::default(),
        }
    };
    let summary = summary_of(&brought, &planned.from);
    let change = changed.then(|| Change {
        patches: planned.patches.clone(),
        base: from.base.clone(),
        op: planned.op.clone(),
        of: planned.of.clone(),
        from: after.is_none().then_some(rev),
    });
    let any = change.is_some() || new + edited > 0;
    let took = change.as_ref().map(|_| planned.took.clone());
    let held = any.then(|| Held {
        at: now(),
        stack: change,
        opened: h.opened,
        replies: h.replies,
        ..Default::default()
    });
    let r = Run {
        state: RunState::Done,
        summary,
        pull: Some(Box::new(Job { after, took, ..job })),
        ..r
    };
    // What each operation needs, for the change filed from that stack.
    let held = held.map(|h| Held {
        needs: crate::held::needs_filed(db, &r, &h, needs),
        ..h
    });
    db.add_run(Run { held, ..r }).map(|_| ())
}

/// A patch of the stack a pull makes, as it is planned: the patch; its
/// place in the stack the pull goes from, when it was there; the revision
/// it is of, by its place among those the pull read, if it read it; and
/// what it takes from Phabricator, if anything.
#[derive(Debug, Clone)]
struct Slot {
    patch: Patch,
    was: Option<usize>,
    read: Option<usize>,
    taken: Option<Taken>,
}

/// The stack a pull makes: its patches, stored; what each takes from
/// Phabricator and what the pull found that changes nothing (`Took`); what
/// the history calls it, and what it changes, said; what it brings,
/// counted; where it is from, said (`3 revisions`, `D12`, `D11..D13`); and,
/// by the place of each revision read, the change of its patch there, for
/// its comments.
struct Planned {
    patches: Vec<String>,
    took: Took,
    op: String,
    of: String,
    brought: Brought,
    from: String,
    changes: HashMap<usize, String>,
}

/// What the stack `from` becomes, pulled as job `job` read Phabricator: of
/// each patch whose revision is open, its revision's latest diff, and its
/// title and summary, where they are no version of the patch stacked knows
/// (`version_of`), as its new version; each revision read that the stack
/// does not hold, open, a patch added where it goes (below the patch of
/// the revision it is the parent of, above that of the one it is stacked
/// on), of its latest diff, with the message and author of its commit as
/// uploaded; then each patch made to apply on those under it (`settle`).
/// Where that cannot be, what keeps it from applying is not taken, and it
/// is planned again without it. A stack that cannot be checked takes no
/// patch: only the comments come.
fn plan(db: &mut Db, from: &Stack, job: &Job) -> Result<Planned> {
    let trailer = crate::config::get().reviews.trailer.clone();
    let olds: Vec<Patch> = from.patches.iter().map(|id| db.patch(id).clone()).collect();
    let read_at = |link: &str| job.read.iter().position(|r| r.link == link);
    let pad = crate::check::can_check(from).ok().map(|(r, b)| (r.to_string(), b.to_string()));
    let mut not_taken: Vec<usize> = Vec::new();
    let mut undone: Vec<String> = Vec::new();
    let (slots, skipped) = loop {
        let mut skipped: Vec<String> = Vec::new();
        let mut slots: Vec<Slot> = olds
            .iter()
            .enumerate()
            .map(|(k, p)| Slot {
                patch: p.clone(),
                was: Some(k),
                read: phab::revision_of(&p.message).and_then(|(_, l)| read_at(&l)),
                taken: None,
            })
            .collect();
        if pad.is_some() {
            for (k, slot) in slots.iter_mut().enumerate() {
                let Some(i) = slot.read else { continue };
                let rd = &job.read[i];
                if !rd.status().open() {
                    continue;
                }
                let (diff, message, since) = version_of(db, from, k, rd, &trailer, not_taken.contains(&i));
                if diff.is_none() && message.is_none() {
                    continue;
                }
                let old = slot.patch.clone();
                let d = Draft {
                    author: if diff.is_some() { rd.author.clone().or_else(|| old.author.clone()) } else { old.author.clone() },
                    message: message.clone().unwrap_or_else(|| old.message.clone()),
                    diff: diff.clone().unwrap_or_else(|| old.diff.clone()),
                    ..Draft::from(&old)
                };
                let new = crate::ops::revise(&old, d);
                if new.id == old.id {
                    continue;
                }
                slot.taken = Some(Taken {
                    change: old.change.clone(),
                    d: rd.d.clone(),
                    diff: diff.is_some(),
                    message: message.is_some(),
                    since,
                    ..Default::default()
                });
                slot.patch = new;
            }
            // The revisions the stack does not hold, each where it goes.
            for (i, rd) in job.read.iter().enumerate() {
                if !rd.status().open() || not_taken.contains(&i) || slots.iter().any(|s| s.read == Some(i)) {
                    continue;
                }
                let of = |d: &str| slots.iter().position(|s| s.read.is_some_and(|j| job.read[j].d == d));
                let (at, added) = match &rd.reached {
                    Reached::Parent(child) => (of(child.as_str()), Added::Parent(child.clone())),
                    Reached::Child(under) => (of(under.as_str()).map(|k| k + 1), Added::Child(under.clone())),
                    Reached::Asked => (slots.is_empty().then_some(0), Added::Asked),
                };
                let Some(at) = at else {
                    skipped.push(match &rd.reached {
                        Reached::Parent(child) => format!("{} is not added: {child}, the revision it is the parent of, is not in the stack.", rd.d),
                        Reached::Child(under) => format!("{} is not added: {under}, the revision it is stacked on, is not in the stack.", rd.d),
                        Reached::Asked => format!("{} is not added: it is no longer in the stack.", rd.d),
                    });
                    continue;
                };
                let Some(p) = added_patch(db, rd, &trailer) else {
                    skipped.push(format!("{} is not added: it has no diff on Phabricator.", rd.d));
                    continue;
                };
                slots.insert(
                    at,
                    Slot {
                        taken: Some(Taken {
                            change: p.change.clone(),
                            d: rd.d.clone(),
                            diff: true,
                            added: Some(added),
                            ..Default::default()
                        }),
                        patch: p,
                        was: None,
                        read: Some(i),
                    },
                );
            }
        }
        let Some((repo, base)) = &pad else { break (slots, skipped) };
        match settle(db, Path::new(repo), base, &olds, job, &mut slots) {
            Ok(()) => break (slots, skipped),
            Err((reads, why)) => {
                let new: Vec<usize> = reads.into_iter().filter(|i| !not_taken.contains(i)).collect();
                undone.push(why);
                // Nothing more to take away: none of what keeps it from
                // applying is the pull's, and nothing of its code is taken.
                if new.is_empty() {
                    not_taken.extend(0..job.read.len());
                } else {
                    not_taken.extend(new);
                }
            }
        }
    };
    for s in &slots {
        db.add_patch(s.patch.clone())?;
    }
    // What the pull found that changes nothing: the revisions that landed
    // or were abandoned; the parents the stack's order disagrees with.
    let mut found: Vec<String> = Vec::new();
    for (k, s) in slots.iter().enumerate() {
        let (Some(i), Some(_)) = (s.read, s.was) else { continue };
        let rd = &job.read[i];
        match rd.status() {
            phab::Status::Published => found.push(format!("{}, the revision of patch {}, has landed: the pull leaves patch {} as it is.", rd.d, k + 1, k + 1)),
            phab::Status::Abandoned => found.push(format!("{}, the revision of patch {}, was abandoned: the pull leaves patch {} as it is.", rd.d, k + 1, k + 1)),
            _ => {}
        }
    }
    for (k, s) in slots.iter().enumerate() {
        let Some(i) = s.read else { continue };
        let rd = &job.read[i];
        let under = (0..k).rev().find(|j| slots[*j].read.is_some());
        for (phid, d) in &rd.parents {
            let Some(j) = slots.iter().position(|t| t.read.is_some_and(|x| job.read[x].phid == *phid)) else { continue };
            if Some(j) != under {
                found.push(format!(
                    "On Phabricator, {}, patch {}, is stacked on {d}, patch {}, which is not the patch under it: the stack keeps its order.",
                    rd.d,
                    k + 1,
                    j + 1
                ));
            }
        }
    }
    found.extend(skipped);
    found.extend(undone);
    // What the history calls it, what it changes, and what it brings.
    let mut ops: Vec<String> = Vec::new();
    let mut places: Vec<usize> = Vec::new();
    let mut brought = Brought::default();
    for (k, s) in slots.iter().enumerate() {
        let Some(t) = &s.taken else { continue };
        places.push(k);
        let word = match (&t.added, s.was.map(|w| &olds[w])) {
            (Some(_), _) => "add",
            (None, Some(old)) if old.diff == s.patch.diff || crate::ops::same_effect(&old.diff, &s.patch.diff) => "reword",
            _ => "modify",
        };
        let of = if t.d.is_empty() { String::new() } else { format!(" ({})", t.d) };
        ops.push(format!("{word} {}{of}", k + 1));
        match (&t.added, t.diff || t.message) {
            (Some(_), _) => brought.added += 1,
            (None, true) => brought.versions += 1,
            (None, false) => {}
        }
        if t.remade.as_ref().is_some_and(|m| !m.is_empty()) {
            brought.marked.push(k);
        }
    }
    let changes: HashMap<usize, String> = slots.iter().filter_map(|s| Some((s.read?, s.patch.change.clone()))).collect();
    let from_said = match (&job.asked, job.read.as_slice()) {
        (_, [one]) => one.d.clone(),
        (Asked::New(_), _) => {
            let ds: Vec<&str> = slots.iter().filter_map(|s| s.read).map(|i| job.read[i].d.as_str()).collect();
            match ds.as_slice() {
                [first, .., last] => format!("{first}..{last}"),
                [one] => one.to_string(),
                [] => "no revision".into(),
            }
        }
        (_, all) => count(all.len(), "revision", "revisions"),
    };
    Ok(Planned {
        patches: slots.iter().map(|s| s.patch.id.clone()).collect(),
        took: Took {
            took: slots.iter().filter_map(|s| s.taken.clone()).collect(),
            found,
        },
        op: format!("pull from Phabricator: {}", ops.join(", ")),
        of: crate::head::patches_said(&places),
        brought,
        from: from_said,
        changes,
    })
}

/// What a pull takes of revision `rd` for the patch at `k` of stack `from`:
/// its latest diff, raw, when it is no version of the patch stacked knows
/// (`submit::known`), unless it is not to be taken (`skip`); its title and
/// summary, as a message (`phab_message`), when no version of the patch
/// would send them (`submit::message_against`); and, its diff taken, when
/// the patch changed since the last diff of the revision stacked knows,
/// the version of the patch that diff is.
fn version_of(db: &Db, from: &Stack, k: usize, rd: &Read, trailer: &str, skip: bool) -> (Option<String>, Option<String>, Option<usize>) {
    let p = db.patch(&from.patches[k]);
    let raw = (rd.diff != 0).then(|| db.raw_diff(&rd.link, rd.diff)).flatten().filter(|r| r.lines().any(|l| l.starts_with("diff ")));
    let diff = raw.filter(|_| !skip).filter(|raw| crate::submit::known(db, from, k, raw).is_none()).map(String::from);
    let since = diff
        .as_ref()
        .and_then(|_| rd.older.iter().find_map(|id| crate::submit::known(db, from, k, db.raw_diff(&rd.link, *id)?)))
        .filter(|kn| !kn.current)
        .map(|kn| kn.version);
    let known = rd.title.trim().is_empty() || db.versions(&p.change).iter().any(|v| crate::submit::message_against(&rd.title, &rd.summary, v, trailer).is_none());
    let message = (!known).then(|| phab_message(rd, trailer));
    (diff, message, since)
}

/// The message revision `rd` gives its patch: its title, without the `WIP: `
/// a submit in work puts first, then its summary, then the trailer naming
/// it.
fn phab_message(rd: &Read, trailer: &str) -> String {
    let t = rd.title.trim();
    let title = match crate::submit::title_in_work(t) {
        true => t.get(3..).unwrap_or_default().trim_start_matches([':', ' ']),
        false => t,
    };
    let mut out = title.to_string();
    let summary = rd.summary.trim();
    if !summary.is_empty() {
        out += &format!("\n\n{summary}");
    }
    out + &format!("\n\n{trailer}: {}", rd.link)
}

/// The patch revision `rd`, which the stack does not hold, is added as: its
/// latest diff, with the author of its commit as uploaded; and that
/// commit's message, the trailer naming the revision put in unless it has
/// it, while the revision's title and summary are what that message sends
/// (`submit::message_against`), else those (`phab_message`), so that the
/// next pull finds its message a version stacked knows. None when it has
/// no diff.
fn added_patch(db: &Db, rd: &Read, trailer: &str) -> Option<Patch> {
    let raw = db.raw_diff(&rd.link, rd.diff).filter(|r| rd.diff != 0 && r.lines().any(|l| l.starts_with("diff ")))?;
    let made = |message: String| {
        crate::ops::new_patch(Draft {
            author: rd.author.clone(),
            message,
            diff: raw.to_string(),
            ..Default::default()
        })
    };
    if let Some(m) = &rd.message {
        let named = format!("{trailer}:");
        let p = made(match m.lines().any(|l| l.starts_with(&named)) {
            true => m.clone(),
            false => format!("{m}\n\n{named} {}", rd.link),
        });
        if rd.title.trim().is_empty() || crate::submit::message_against(&rd.title, &rd.summary, &p, trailer).is_none() {
            return Some(p);
        }
    }
    Some(made(phab_message(rd, trailer)))
}

/// The diff Phabricator has of the patch in slot `s`: its revision's latest,
/// raw, if the pull read it.
fn phab_diff(db: &Db, job: &Job, s: &Slot) -> Option<String> {
    let rd = &job.read[s.read?];
    db.raw_diff(&rd.link, rd.diff).map(String::from)
}

/// Each patch of `slots` made to apply on those under it, from the bottom,
/// on `base` in the repository at `repo`: one that does not, made again on
/// them (`ops::rebased`), as it was made on what was under it (the stack as
/// it was, `olds`, for a patch of its own; Phabricator's diffs under it, for
/// one pulled), its conflicts marked where they disagree. Where that
/// cannot be: the revisions read, by their places, whose diffs keep it from
/// applying, not to take, and why, said.
fn settle(db: &Db, repo: &Path, base: &str, olds: &[Patch], job: &Job, slots: &mut [Slot]) -> std::result::Result<(), (Vec<usize>, String)> {
    let mut remade: HashSet<usize> = HashSet::new();
    loop {
        let ps: Vec<&Patch> = slots.iter().map(|s| &s.patch).collect();
        let pulled_under = |slots: &[Slot], k: usize| -> Vec<usize> { slots[..k].iter().filter(|s| s.taken.as_ref().is_some_and(|t| t.diff)).filter_map(|s| s.read).collect() };
        let k = match crate::check::first_failure(repo, base, &ps) {
            Ok(None) => return Ok(()),
            Ok(Some(k)) => k,
            Err(e) => return Err((pulled_under(slots, slots.len()), format!("The pull takes no diff: the stack cannot be checked: {}.", e.0))),
        };
        let n = k + 1;
        let pulled = slots[k].taken.as_ref().filter(|t| t.diff).map(|t| t.d.clone());
        let fail = |why: String| -> (Vec<usize>, String) {
            match (&pulled, slots[k].read) {
                (Some(d), Some(i)) => (vec![i], format!("{d}'s latest diff is not taken: it does not apply on the patches under patch {n}, and {why}.")),
                _ => {
                    let under = pulled_under(slots, k);
                    let ds: Vec<String> = under.iter().map(|i| format!("{}'s latest diff", job.read[*i].d)).collect();
                    let (is, it) = if ds.len() == 1 { ("is", "it") } else { ("are", "them") };
                    (under, format!("{} {is} not taken: patch {n} ({}) would no longer apply on {it}, and {why}.", crate::submit::and(&ds), slots[k].patch.title()))
                }
            }
        };
        if !remade.insert(k) {
            return Err(fail("made again on them, it does not apply either".into()));
        }
        let was: Vec<String> = match (&pulled, slots[k].was) {
            (None, Some(w)) => olds[..w].iter().map(|p| p.diff.clone()).collect(),
            _ => slots[..k].iter().map(|s| phab_diff(db, job, s).unwrap_or_else(|| s.patch.diff.clone())).collect(),
        };
        let ours: Vec<&str> = slots[..k].iter().map(|s| s.patch.diff.as_str()).collect();
        let was: Vec<&str> = was.iter().map(String::as_str).collect();
        let theirs = match &pulled {
            Some(d) => format!("{d}'s latest diff"),
            None => format!("patch {n}"),
        };
        match crate::ops::rebased(repo, base, &ours, &was, &slots[k].patch.diff, [crate::ops::PULL_OURS, crate::ops::PULL_BASE, &theirs]) {
            Ok((diff, marked)) => {
                let old = slots[k].patch.clone();
                slots[k].patch = crate::ops::revise(&old, Draft { diff, ..Draft::from(&old) });
                let d = slots[k].read.map(|i| job.read[i].d.clone()).unwrap_or_default();
                let t = slots[k].taken.get_or_insert_with(|| Taken {
                    change: old.change.clone(),
                    d,
                    ..Default::default()
                });
                t.remade = Some(marked);
            }
            Err(why) => return Err(fail(why)),
        }
    }
}

/// The pulls waiting for your review that go on from landed work `id`,
/// guessing it is accepted.
pub(crate) fn going_on_from<'a>(db: &'a Db, id: &str) -> Vec<&'a Run> {
    db.live_runs()
        .into_iter()
        .filter(|r| is(r) && crate::held::waiting(db, r) && after(r) == Some(id))
        .collect()
}

/// The landed work pull `r` goes on from, if it is chained on some.
pub(crate) fn after(r: &Run) -> Option<&str> {
    r.pull.as_ref()?.after.as_deref()
}

/// Landed work `id` accepted, its change now the stack at `rev`: the pulls
/// that went on from it go on from that stack.
pub(crate) fn followed(db: &mut Db, id: &str, rev: usize) -> Result<()> {
    for r in going_on_from(db, id).into_iter().cloned().collect::<Vec<_>>() {
        let (Some(mut h), Some(mut job)) = (r.held.clone(), r.pull.clone()) else { continue };
        if let Some(c) = h.stack.as_mut() {
            c.from = Some(rev);
        }
        job.after = None;
        db.add_run(Run {
            held: Some(h),
            pull: Some(job),
            ..r
        })?;
    }
    Ok(())
}

/// Landed work `id` rejected, sent back or edited by hand (`why`): the
/// pulls that went on from it dropped, each made again at its next turn,
/// from what it read, from the stack that leaves. How many.
pub(crate) fn dropped(db: &mut Db, id: &str, why: &str) -> Result<usize> {
    let runs: Vec<Run> = going_on_from(db, id).into_iter().cloned().collect();
    for r in &runs {
        let v = db.review(&r.id).cloned().unwrap_or_default();
        db.set_review(&r.id, Review { closed: true, ..v })?;
        db.add_run(Run {
            summary: format!("dropped: the work it went on from was {why}; made again"),
            ..r.clone()
        })?;
        db.add_run(Run {
            id: format!("pull-{}", crate::db::unique()),
            what: WHAT.into(),
            stack: r.stack.clone(),
            state: RunState::Started,
            request: r.request.clone(),
            pull: r.pull.as_deref().map(|j| Box::new(Job { after: None, took: None, ..j.clone() })),
            ..Default::default()
        })?;
    }
    Ok(runs.len())
}

/// What a pull you dropped before its turn came to: nothing held.
pub const DROPPED: &str = "you dropped the pull from Phabricator before its turn: what it read is stored, and nothing of it waits for your review";

/// Pull `id`, waiting its turn, dropped from the queue as if it was never
/// asked: it ends as failed, `DROPPED` its summary, nothing changed in the
/// stack. One made meanwhile stays as it is. The run as it is then, and
/// whether it was dropped. What waited behind it is started by the caller
/// (`ask::launch`).
pub fn drop_waiting(db_path: &Path, id: &str) -> Result<(Run, bool)> {
    Db::edit(db_path, |db| {
        let r = db.run(id).cloned().ok_or_else(|| Error(format!("no run {id}")))?;
        if !is(&r) || r.pull.is_none() {
            return err(format!("run {id} is not a pull from Phabricator"));
        }
        if r.state != RunState::Started {
            return Ok((r, false));
        }
        let r = Run {
            state: RunState::Failed,
            summary: DROPPED.into(),
            ..r
        };
        db.add_run(r.clone())?;
        Ok((r, true))
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
        assert_eq!(Pulling::Starting.said(), "pulling from Phabricator");
        let reading = Pulling::Reading {
            rev: "D12".into(),
            at: 2,
            of: 3,
        };
        assert_eq!(reading.said(), "pulling from Phabricator: D12, revision 2 of 3");
        // From one patch's revision, or a new stack's: it, named.
        assert_eq!(Pulling::Alone("D12".into()).said(), "pulling from Phabricator: D12");
    }

    #[test]
    fn what_a_pull_brings_is_said_counted_its_conflicts_marked_last() {
        let b = |versions, added, new, edited, marked: &[usize]| Brought { versions, added, new, edited, marked: marked.to_vec() };
        assert_eq!(summary_of(&b(0, 0, 0, 0, &[]), "3 revisions"), "nothing new from 3 revisions");
        assert_eq!(summary_of(&b(0, 0, 1, 0, &[]), "D12"), "1 new comment from D12: it waits for your review");
        assert_eq!(summary_of(&b(0, 0, 2, 1, &[]), "1 revision"), "2 new comments and 1 edited comment from 1 revision: they wait for your review");
        assert_eq!(
            summary_of(&b(1, 1, 2, 0, &[2]), "3 revisions"),
            "1 new version, 1 new patch and 2 new comments from 3 revisions: they wait for your review; patch 3 no longer applies on the pulled versions: its conflicts are marked, to resolve"
        );
        assert_eq!(
            summary_of(&b(2, 0, 0, 0, &[1, 2]), "D11..D13"),
            "2 new versions from D11..D13: they wait for your review; patches 2 and 3 no longer apply on the pulled versions: their conflicts are marked, to resolve"
        );
    }
}
