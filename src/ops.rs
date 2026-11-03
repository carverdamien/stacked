//! Edits to a stack, as the commands and the UI make them.

use std::collections::HashMap;
use std::ops::Range;

use crate::db::{Db, Error, Patch, Result, Stack, err, new_change};
use crate::mbox::{Draft, format_mail};

/// `git`'s idea of who is writing, without the timestamp it ends with.
pub fn default_author() -> Option<String> {
    let out = std::process::Command::new("git")
        .args(["var", "GIT_AUTHOR_IDENT"])
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    let ident = String::from_utf8(out.stdout).ok()?;
    let end = ident.rfind('>')?;
    out.status.success().then(|| ident[..=end].to_string())
}

/// A new patch, a change of its own.
pub fn new_patch(d: Draft) -> Patch {
    let author = d.author.or_else(default_author);
    Patch::new(new_change(&d.message), author, d.date, d.message, d.diff)
}

/// A new version of `old`.
pub fn revise(old: &Patch, d: Draft) -> Patch {
    Patch::new(old.change.clone(), d.author, d.date, d.message, d.diff)
}

/// What stacked wrote of a stack into the worktree where the agent changes
/// it: the commit it made of each patch, as that commit's patch (`commits`,
/// by the commit's sha, the patch's id), and that commit read back as the
/// agent's commits are, what git gives back of the patch (`drafts`, by the
/// patch's id). Empty for drafts read from commits stacked did not make, as
/// an import's are.
#[derive(Debug, Clone, Default)]
pub struct Written {
    pub commits: HashMap<String, String>,
    pub drafts: HashMap<String, Draft>,
}

/// The patches `drafts` are, given the ones they replace: a draft from a
/// commit stacked made of a patch (`written`) replaces that patch; else one
/// with the effect of a patch's diff (`same_effect`) replaces it, its
/// message alone changed if any (of several with that effect, the one with
/// its title); else one with its title; else it is new. A patch's title is
/// its own, or the one git gave back of it. A draft takes the message and
/// the author of the patch it replaces, as stored, where they are what git
/// gave back of them (`as_stored`). A draft with the message, the author
/// and the effect of the patch it replaces is that patch, unchanged, its
/// diff as stored: a rebase that only moved the patch's lines, or that made
/// its commit again as it rewords another, makes no new version of it. Any
/// other draft is a new version of the patch it replaces. A diff of
/// nothing is no patch's own. Nothing is stored, and nothing is checked:
/// `plan_checked` checks the patches kept so.
fn plan_drafts(olds: &[Patch], drafts: Vec<Draft>, written: &Written) -> Vec<Patch> {
    let mut left: Vec<&Patch> = olds.iter().collect();
    let mut out = Vec::new();
    for d in drafts {
        let from_commit = d
            .commit
            .as_ref()
            .and_then(|c| written.commits.get(c))
            .and_then(|id| left.iter().position(|o| &o.id == id));
        // Its code as it was, whatever its commit: reworded, or rebased.
        let code = |o: &Patch| !d.diff.trim().is_empty() && same_effect(&o.diff, &d.diff);
        // Its title as stored, or as git gave it back.
        let titled = |o: &Patch| o.title() == d.title() || written.drafts.get(&o.id).is_some_and(|w| w.title() == d.title());
        let at = from_commit
            .or_else(|| left.iter().position(|o| code(o) && titled(o)))
            .or_else(|| left.iter().position(|o| code(o)))
            .or_else(|| left.iter().position(|o| titled(o)));
        out.push(match at {
            Some(i) => {
                let old = left.remove(i);
                let d = as_stored(old, d, written);
                if unchanged(old, &d) { old.clone() } else { revise(old, d) }
            }
            None => new_patch(d),
        });
    }
    out
}

/// Draft `d` of patch `old`, its author the patch's if it has none; and its
/// message, and its author, the patch's as stored where they are what git
/// gave back of the patch's own (`written`): what the agent left as it was.
/// git does not give every message back as stored: `git am` keeps no empty
/// line twice in a row, no white space at the end of a line and no carriage
/// return, and takes a prefix and runs of spaces out of a title;
/// format-patch quotes a name that holds a comma or a dot. A message or an
/// author changed otherwise, if only in its white space, is the draft's.
pub(crate) fn as_stored(old: &Patch, mut d: Draft, written: &Written) -> Draft {
    if d.author.is_none() {
        d.author = old.author.clone();
    }
    if let Some(w) = written.drafts.get(&old.id) {
        if d.message == w.message {
            d.message = old.message.clone();
        }
        if d.author == w.author {
            d.author = old.author.clone();
        }
    }
    d
}

/// Whether draft `d` is patch `old` as it was: the same message, the same
/// author, and a diff of the same effect, however git writes it now and
/// whatever date the draft has (`git am` gives a patch with none the time
/// it ran, and a rebase may give a commit a new one).
fn unchanged(old: &Patch, d: &Draft) -> bool {
    d.message == old.message && d.author == old.author && same_effect(&old.diff, &d.diff)
}

/// Whether diffs `a` and `b` have the same effect, whatever else their
/// text says: the same files in the same order, with the same paths and
/// modes, and the same hunks, each with the same lines (kept, added,
/// removed). Where a hunk starts, the heading git writes after its second
/// `@@`, the blobs an `index` line names, the similarity of a rename and
/// the white space a header's line ends with do not count. So a commit
/// reworded, rebased as it was, or moved down its files by a change below
/// it, has the effect it had; one whose hunks git splits or merges
/// otherwise has another.
pub fn same_effect(a: &str, b: &str) -> bool {
    effect(a).eq(effect(b))
}

/// The lines of diff `d` that make its effect, as `same_effect` compares
/// them: a file's header without its `index` line and its similarity, each
/// line of it without the white space it ends with (git ends `--- a/x y`
/// with a tab, as the name holds a space); a hunk's header as `@@` alone,
/// numbers and heading left out; a hunk's lines as they are, an empty one
/// the empty context line that git applies it as.
fn effect(d: &str) -> impl Iterator<Item = &str> {
    let mut in_hunk = false;
    d.split_terminator('\n').filter_map(move |l| {
        if l.starts_with("diff --git ") {
            in_hunk = false;
        } else if l.starts_with("@@") {
            in_hunk = true;
            return Some("@@");
        } else if in_hunk {
            return Some(if l.is_empty() { " " } else { l });
        }
        header_line(l)
    })
}

/// A line of a file's header as `effect` takes it: without the white space
/// it ends with; none for an `index` line or a similarity, which git
/// derives.
fn header_line(l: &str) -> Option<&str> {
    let l = l.trim_end();
    let derived = ["index ", "similarity index ", "dissimilarity index "].iter().any(|p| l.starts_with(p));
    (!derived).then_some(l)
}

/// Whether diffs `a` and `b` make the same changes, whatever context git
/// wrote around them: the same files, in any order, each with the same
/// header as `same_effect` takes it, and the same runs of changed lines,
/// each the lines it removes and adds one after another, with git's `\ No
/// newline at end of file` after one of them, as they are. How many lines
/// of context there are, and which, where each hunk starts, and how git
/// splits or merges the hunks, do not count. So a diff moz-phab sent with
/// more context than stacked keeps still makes the changes of the version
/// it was made of once a rebase has changed lines of that context, though
/// it no longer applies on the patches below.
pub fn same_changes(a: &str, b: &str) -> bool {
    changes(a) == changes(b)
}

/// A file of a diff as `same_changes` compares it: its header, as `effect`
/// takes it, and its runs of changed lines, each its lines as they are.
#[derive(Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
struct FileChanges<'a> {
    header: Vec<&'a str>,
    runs: Vec<Vec<&'a str>>,
}

/// The files of diff `d` as `same_changes` compares them, sorted, what
/// comes before the first one a file too. In a hunk, a line removed or
/// added goes in the run of those right before it, or starts one, as git's
/// `\ No newline at end of file` does after one of them; a context line
/// ends a run, as a hunk's header does, and neither is kept (an empty line
/// in a hunk is git's empty context line, as `effect` takes it), nor git's
/// `\ No newline at end of file` after a context line: more context may
/// reach the end of a file that has none, where less does not.
fn changes(d: &str) -> Vec<FileChanges<'_>> {
    let mut files = vec![FileChanges::default()];
    let (mut in_hunk, mut in_run) = (false, false);
    for l in d.split_terminator('\n') {
        if l.starts_with("diff --git ") {
            files.push(FileChanges::default());
            in_hunk = false;
        } else if l.starts_with("@@") {
            (in_hunk, in_run) = (true, false);
            continue;
        }
        let Some(file) = files.last_mut() else { continue };
        if !in_hunk {
            file.header.extend(header_line(l));
            continue;
        }
        let changed = l.starts_with(['-', '+']) || (in_run && l.starts_with('\\'));
        if changed && in_run && let Some(run) = file.runs.last_mut() {
            run.push(l);
        } else if changed {
            file.runs.push(vec![l]);
        }
        in_run = changed;
    }
    files.sort();
    files
}

/// As `plan_drafts`, each patch it keeps with its diff as stored checked
/// on `on`, a repository and the commit the drafts apply to: the patches
/// applied in turn in the check's own index (`check::astray`), a kept patch
/// must leave its files as its draft does, and so as the commit the draft
/// was read from has them. One that does not is its draft's new version
/// instead (its message and author as `as_stored` gives them): its diff,
/// written before its lines moved, applied at another copy of them, or a
/// hunk moved to lines like its own, as git puts a hunk where its lines
/// are nearest to where its header says. Then the check goes on up the
/// stack. Where nothing can be checked, every patch kept so is its draft's
/// new version.
pub fn plan_checked(
    olds: &[Patch],
    drafts: Vec<Draft>,
    written: &Written,
    on: Option<(&std::path::Path, &str)>,
) -> Vec<Patch> {
    let theirs = drafts.clone();
    let mut planned = plan_drafts(olds, drafts, written);
    // Kept, a patch has its diff as stored, not its draft's.
    let kept = |planned: &[Patch], k: usize| planned[k].diff != theirs[k].diff;
    let taken = |old: &Patch, k: usize| revise(old, as_stored(old, theirs[k].clone(), written));
    while (0..planned.len()).any(|k| kept(&planned, k)) {
        let astray = on.ok_or_else(|| Error("nothing to check on".into())).and_then(|(repo, base)| {
            let mine: Vec<&str> = planned.iter().map(|p| p.diff.as_str()).collect();
            let drafts: Vec<&str> = theirs.iter().map(|d| d.diff.as_str()).collect();
            crate::check::astray(repo, base, &mine, &drafts)
        });
        match astray {
            Ok(None) => break,
            Ok(Some(k)) if kept(&planned, k) => planned[k] = taken(&planned[k], k),
            _ => {
                for k in 0..planned.len() {
                    if kept(&planned, k) {
                        planned[k] = taken(&planned[k], k);
                    }
                }
            }
        }
    }
    planned
}

/// As `plan_checked`, the patches stored: their ids.
pub fn match_drafts(
    db: &mut Db,
    olds: &[Patch],
    drafts: Vec<Draft>,
    written: &Written,
    on: Option<(&std::path::Path, &str)>,
) -> Result<Vec<String>> {
    plan_checked(olds, drafts, written, on)
        .into_iter()
        .map(|p| db.add_patch(p))
        .collect()
}

/// Puts `drafts` on top of `name`, making it if need be. With `replace`,
/// they are the whole stack instead, and a draft with the effect of a
/// patch's diff, or its title, is a new version of that patch, or that
/// patch itself when its message and author are the same too, and its
/// diff as stored does on the stack's new base what the draft does
/// (`plan_checked`): so importing a stack again after rebasing it, or
/// rewording its commits, keeps each patch's history, and a patch the
/// rebase only moved down its files keeps its version.
pub fn import(
    db: &mut Db,
    name: &str,
    drafts: Vec<Draft>,
    replace: bool,
    base: Option<(String, String)>,
) -> Result<usize> {
    let (mut patches, mut on) = match db.head(name) {
        Some((_, s)) => (s.patches.clone(), s.base.clone().zip(s.repo.clone())),
        None => (vec![], None),
    };
    let n = drafts.len();
    let olds: Vec<Patch> = if replace {
        std::mem::take(&mut patches).iter().map(|id| db.patch(id).clone()).collect()
    } else {
        vec![]
    };
    if replace || on.is_none() {
        on = base.or(on);
    }
    // Replacing the stack, the drafts apply to its base.
    let at = on.as_ref().filter(|_| replace).map(|(b, r)| (std::path::Path::new(r.as_str()), b.as_str()));
    patches.extend(match_drafts(db, &olds, drafts, &Written::default(), at)?);
    let op = if replace {
        format!("import {n} patches, replacing")
    } else {
        format!("import {n} patches")
    };
    let (base, repo) = on.unzip();
    db.set_stack_on(name, patches, base, repo, &op)
}

/// That stack `name` may be edited by hand now: not while the agent
/// changes it. Every edit of the user's asks first; the agent's own do not.
pub fn by_hand(db: &Db, name: &str) -> Result<()> {
    match crate::ask::blocked(db, name) {
        Some(why) => err(why),
        None => Ok(()),
    }
}

/// That stack `name` may be edited by hand in $EDITOR now: as `by_hand`
/// says, unless the edit is of `work`, the landed work whose change waits
/// for your review.
pub fn may_edit(db: &Db, name: &str, work: Option<&str>) -> Result<()> {
    if work.is_some_and(|w| crate::held::change_waiting(db, name).is_some_and(|r| r.id == w)) {
        return Ok(());
    }
    by_hand(db, name)
}

/// What an edit by hand of stack `name` starts from: the stack as it is,
/// or, with `work`, as the landed work run `work` holds would make it.
pub fn edited_from(db: &Db, name: &str, work: Option<&str>) -> Result<Stack> {
    let s = db.stack(name)?.clone();
    let Some(id) = work else { return Ok(s) };
    let c = db
        .run(id)
        .filter(|r| r.stack == name && crate::held::waiting(db, r))
        .and_then(|r| r.held.as_ref()?.stack.clone())
        .ok_or_else(|| Error(format!("run {id} holds no change of {name} that waits for review")))?;
    Ok(Stack {
        patches: c.patches,
        base: c.base.or(s.base.clone()),
        ..s
    })
}

/// Lands `patches`, what an edit by hand made of stack `name` (`of`, what
/// it changes), for your review: in place of `work`, if it edits it. What
/// came of it: nothing, if they are the patches it started from, `was`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn land(db: &mut Db, name: &str, was: &[String], patches: Vec<String>, op: &str, of: &str, said: &str, work: Option<&str>) -> Result<Edited> {
    if patches == was {
        return Ok(Edited::Unchanged("nothing changed".into()));
    }
    let run = crate::held::hold_by_hand(db, name, patches, op, of, said, work)?;
    Ok(Edited::Landed { said: said.into(), run })
}

/// What an edit by hand came to, `done`, as it lands: once it is filed,
/// before its review opens, what each operation of its change needs worked
/// out and stored with it, with no lock held (`held::work_out`). Should the
/// database not be read or written then, nothing is stored, and its change
/// is marked as a whole.
fn filed(path: &std::path::Path, done: Result<Edited>) -> Result<Edited> {
    if let Ok(Edited::Landed { run, .. }) = &done {
        let _ = crate::held::work_out(path, run);
    }
    done
}

/// `patches` with `d` the new version of the one at `at`, stored, and the
/// edit as the history says it; none when it is the same.
pub(crate) fn revised(db: &mut Db, patches: &[String], at: usize, d: Draft) -> Result<Option<(Vec<String>, String)>> {
    let old = db.patch(&patches[at]).clone();
    let id = db.add_patch(revise(&old, d))?;
    if id == old.id {
        return Ok(None);
    }
    let mut out = patches.to_vec();
    out[at] = id.clone();
    Ok(Some((out, format!("edit {} as {}", &old.id[..8], &id[..8]))))
}

/// `c`, the conflict an edit of `work` has, if it is of landed work.
fn of_work(c: Option<Box<Conflict>>, work: Option<&str>) -> Option<Box<Conflict>> {
    c.map(|c| {
        Box::new(Conflict {
            work: work.map(String::from),
            ..*c
        })
    })
}

/// Puts `d` into `name` at `at` (0 is the bottom), or on top.
pub fn add(db: &mut Db, name: &str, d: Draft, at: Option<usize>) -> Result<String> {
    let mut patches = db.stack(name)?.patches.clone();
    let at = at.unwrap_or(patches.len());
    if at > patches.len() {
        return err(format!("{name} has {} patches", patches.len()));
    }
    let title = d.title().to_string();
    let id = db.add_patch(new_patch(d))?;
    patches.insert(at, id.clone());
    db.set_stack(name, patches, &format!("add {}: {title}", &id[..8]))?;
    Ok(id)
}

/// Moves the patch at `from` to `to`, both counted from 0 at the bottom.
pub fn move_patch(db: &mut Db, name: &str, from: usize, to: usize) -> Result<usize> {
    let mut patches = db.stack(name)?.patches.clone();
    if from >= patches.len() || to >= patches.len() {
        return err(format!("{name} has {} patches", patches.len()));
    }
    let id = patches.remove(from);
    patches.insert(to, id.clone());
    db.set_stack(name, patches, &format!("move {} from {} to {}", &id[..8], from + 1, to + 1))
}

pub fn drop_patches(db: &mut Db, name: &str, at: &[usize]) -> Result<usize> {
    let s = db.stack(name)?;
    let dropped: Vec<&str> = at.iter().map(|&i| &s.patches[i][..8]).collect();
    let op = format!("drop {}", dropped.join(" "));
    let patches = (0..s.patches.len())
        .filter(|i| !at.contains(i))
        .map(|i| s.patches[i].clone())
        .collect();
    db.set_stack(name, patches, &op)
}

/// Makes `d` the new version of the patch at `at`, at once: not an edit
/// by hand, which lands for review.
pub fn replace(db: &mut Db, name: &str, at: usize, d: Draft, verb: &str) -> Result<usize> {
    let mut patches = db.stack(name)?.patches.clone();
    let old = db.patch(&patches[at]).clone();
    let id = db.add_patch(revise(&old, d))?;
    if id == old.id {
        return Ok(db.head(name).unwrap().0);
    }
    patches[at] = id.clone();
    db.set_stack(name, patches, &format!("{verb} {} as {}", &old.id[..8], &id[..8]))
}

/// The patch as it is edited and shown: its mail.
pub fn render(p: &Patch) -> String {
    format_mail(&Draft::from(p), "")
}

/// What changed from `old` to `new`, their mails diffed, as
/// `interdiff_lines` has it: as text.
pub fn interdiff(old: &Patch, new: &Patch) -> String {
    inter(old, new).0
}

/// A line of an interdiff (`interdiff_lines`), as it is written, its sign
/// first. On a line of the message taken out or put in, what of it the
/// other version has not, compared word by word (`messages::compare`): ranges of its characters, the sign the first of
/// them; and whether a break only it has follows it. The rest of such a
/// line is what both versions say.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InterLine {
    pub text: String,
    pub own: Option<(Vec<(usize, usize)>, bool)>,
}

impl From<&str> for InterLine {
    fn from(text: &str) -> InterLine {
        InterLine { text: text.into(), own: None }
    }
}

/// What changed from `old` to `new`, a line at a time: their mails diffed,
/// with three lines around each change. Their headers, and what follows
/// the `---` before the diff, line by line. Their titles, then the bodies
/// of their messages, each with the other's, compared word by word where
/// their lines wrap aside (`messages::diff`): a section the same in
/// both, though wrapped otherwise or ending its lines in other white space,
/// is there once, as the new version writes it, unchanged; written
/// otherwise, it is around a change of its own, though none of its lines
/// is one. The sections that differ are the old version's lines taken out,
/// then the new one's put in, each with its own line breaks, what only one
/// has marked (`InterLine`). With the title and the body the same, the two
/// mails diffed line by line.
pub fn interdiff_lines(old: &Patch, new: &Patch) -> Vec<InterLine> {
    inter(old, new).1
}

/// What changed from `old` to `new`, as `interdiff_lines` has it, their
/// messages compared to the letter: when they have the same words, and
/// differ only in where their lines wrap or in their white space, each
/// section written otherwise is there as each version writes it, the old
/// one's lines taken out and the new one's put in, nothing of them marked,
/// as every word of them is in both. As the Phabricator line's view
/// compares a revision with its patch, their titles and summaries in sync
/// only to the letter.
pub fn letter_interdiff_lines(old: &Patch, new: &Patch) -> Vec<InterLine> {
    inter_as(old, new, true).1
}

/// The interdiff of `old` and `new` (`interdiff_lines`): as text, and a
/// line at a time.
fn inter(old: &Patch, new: &Patch) -> (String, Vec<InterLine>) {
    inter_as(old, new, false)
}

/// As `inter`, the messages compared to the letter if `letter`
/// (`letter_interdiff_lines`).
fn inter_as(old: &Patch, new: &Patch, letter: bool) -> (String, Vec<InterLine>) {
    let (a, b) = (render(old), render(new));
    let names = (&old.id[..8], &new.id[..8]);
    match (Mail::of(&a, old), Mail::of(&b, new)) {
        (Some(x), Some(y)) if (x.title, x.body) != (y.title, y.body) => {
            // The same words, only wrapped or spaced otherwise.
            let wrapped = letter && [(x.title, y.title), (x.body, y.body)].iter().all(|(p, q)| crate::messages::diff(p, q).runs.iter().all(|r| r.same));
            hunks(&mail_rows(&x, &y, wrapped), names)
        }
        _ => {
            let text = similar::TextDiff::from_lines(&a, &b).unified_diff().context_radius(3).header(names.0, names.1).to_string();
            let lines = text.lines().map(InterLine::from).collect();
            (text, lines)
        }
    }
}

/// A patch's mail (`render`) in its parts, each its lines with their
/// newlines: its headers before the title; the title, its `Subject:`
/// line; the empty line under it; the body of the message; the rest, from
/// the `---` before the diff.
struct Mail<'a> {
    head: &'a str,
    title: &'a str,
    blank: &'a str,
    body: &'a str,
    rest: &'a str,
}

impl<'a> Mail<'a> {
    /// `mail`, patch `p`'s, in its parts: none if it is not as `render`
    /// writes it.
    fn of(mail: &'a str, p: &Patch) -> Option<Mail<'a>> {
        let starts: Vec<usize> = std::iter::once(0).chain(mail.match_indices('\n').map(|(i, _)| i + 1)).collect();
        let start = |k: usize| starts.get(k).copied().unwrap_or(mail.len());
        let line = |k: usize| &mail[start(k)..start(k + 1)];
        let subject = mail.lines().position(|l| l.starts_with("Subject: "))?;
        let sep = mail.lines().count().checked_sub(p.diff.lines().count() + 1)?;
        (subject + 2 <= sep && line(subject + 1) == "\n" && line(sep) == "---\n").then(|| Mail {
            head: &mail[..start(subject)],
            title: line(subject),
            blank: line(subject + 1),
            body: &mail[start(subject + 2)..start(sep)],
            rest: &mail[start(sep)..],
        })
    }
}

/// A line of an interdiff before its hunks are cut (`hunks`): its sign;
/// the line, with its newline if it has one; how many lines of the old
/// mail and of the new it stands for; whether it is a line the same in
/// both as it is written, of those around a change that are cut to the
/// three nearest; and what of it only its version has (`InterLine`).
struct Row {
    sign: char,
    line: String,
    old: usize,
    new: usize,
    kept: bool,
    own: Option<(Vec<(usize, usize)>, bool)>,
}

/// The lines of the interdiff of mails `a` and `b`, before its hunks are
/// cut: their headers line by line; their titles, then their bodies, as
/// messages are compared, each section written otherwise there as each
/// writes it when `wrapped` (`by_message`); the empty line under the
/// title, which both have; the rest line by line.
fn mail_rows(a: &Mail, b: &Mail, wrapped: bool) -> Vec<Row> {
    let mut rows = Vec::new();
    by_line(&mut rows, a.head, b.head);
    by_message(&mut rows, a.title, b.title, wrapped);
    rows.push(Row {
        sign: ' ',
        line: b.blank.into(),
        old: 1,
        new: 1,
        kept: true,
        own: None,
    });
    by_message(&mut rows, a.body, b.body, wrapped);
    by_line(&mut rows, a.rest, b.rest);
    rows
}

/// Lines `a` of the old mail and `b` of the new, diffed line by line.
fn by_line(rows: &mut Vec<Row>, a: &str, b: &str) {
    for c in similar::TextDiff::from_lines(a, b).iter_all_changes() {
        let (sign, old, new) = match c.tag() {
            similar::ChangeTag::Equal => (' ', 1, 1),
            similar::ChangeTag::Delete => ('-', 1, 0),
            similar::ChangeTag::Insert => ('+', 0, 1),
        };
        rows.push(Row {
            sign,
            line: c.value().into(),
            old,
            new,
            kept: sign == ' ',
            own: None,
        });
    }
}

/// Lines `a` of the old mail and `b` of the new, a part of each one's
/// message, compared as messages are (`messages::diff`): a run the same
/// in both, the new one's lines, kept as they are when both write it so;
/// a run that differs, the old one's lines taken out and the new one's
/// put in, what only one has marked. When `wrapped`, as the two messages
/// differ only in where their lines wrap or in their white space, a run
/// the same in both but written otherwise is as one that differs, nothing
/// of it marked.
fn by_message(rows: &mut Vec<Row>, a: &str, b: &str, wrapped: bool) {
    let d = crate::messages::diff(a, b);
    let (la, lb): (Vec<&str>, Vec<&str>) = (a.split_inclusive('\n').collect(), b.split_inclusive('\n').collect());
    // What a line has that the other version has not, counted from its sign.
    let own = |m: Option<&crate::messages::Marked>| {
        Some(m.map_or((vec![], false), |m| (m.words.iter().map(|&(s, e)| (s + 1, e + 1)).collect(), m.broken)))
    };
    let row = |sign: char, line: &str, old: usize, new: usize, own: Option<(Vec<(usize, usize)>, bool)>| Row {
        sign,
        line: line.into(),
        old,
        new,
        kept: false,
        own,
    };
    for run in d.runs {
        let (x, y) = (la.get(run.ours.clone()).unwrap_or_default(), lb.get(run.theirs.clone()).unwrap_or_default());
        if run.same && x == y {
            rows.extend(y.iter().map(|l| Row { kept: true, ..row(' ', l, 1, 1, None) }));
        } else if run.same && !wrapped {
            // Written otherwise: once, as the new version writes it.
            rows.extend(y.iter().enumerate().map(|(k, l)| row(' ', l, if k == 0 { x.len() } else { 0 }, 1, None)));
        } else {
            rows.extend(run.ours.zip(x).map(|(i, l)| row('-', l, 1, 0, own(d.ours.get(i)))));
            rows.extend(run.theirs.zip(y).map(|(j, l)| row('+', l, 0, 1, own(d.theirs.get(j)))));
        }
    }
}

/// `rows` as a unified diff, `names` the old mail's and the new one's: the
/// lines kept as they are around each change cut to the three nearest, as
/// `similar` cuts them, those of two changes further apart in two hunks.
/// As text, and a line at a time.
fn hunks(rows: &[Row], names: (&str, &str)) -> (String, Vec<InterLine>) {
    const AROUND: usize = 3;
    // Each run of lines kept as they are, and each other line.
    let mut ops: Vec<(bool, Range<usize>)> = Vec::new();
    for (i, r) in rows.iter().enumerate() {
        match ops.last_mut() {
            Some((true, run)) if r.kept => run.end = i + 1,
            _ => ops.push((r.kept, i..i + 1)),
        }
    }
    if let Some((true, run)) = ops.first_mut() {
        run.start = run.end - run.len().min(AROUND);
    }
    if let Some((true, run)) = ops.last_mut() {
        run.end = run.start + run.len().min(AROUND);
    }
    let mut groups: Vec<Vec<(bool, Range<usize>)>> = Vec::new();
    let mut pending = Vec::new();
    for (kept, run) in ops {
        if kept && run.len() > 2 * AROUND {
            pending.push((true, run.start..run.start + AROUND));
            groups.push(std::mem::take(&mut pending));
            pending.push((true, run.end - AROUND..run.end));
            continue;
        }
        pending.push((kept, run));
    }
    if !matches!(&pending[..], [] | [(true, _)]) {
        groups.push(pending);
    }
    // Where each row starts, in the old mail and in the new.
    let (mut old, mut new) = (vec![0], vec![0]);
    for r in rows {
        old.push(old[old.len() - 1] + r.old);
        new.push(new[new.len() - 1] + r.new);
    }
    // As a hunk's header has it: its first line, from 1, and how many.
    let range = |from: usize, to: usize| match to - from {
        0 => format!("{from},0"),
        1 => format!("{}", from + 1),
        n => format!("{},{n}", from + 1),
    };
    // A line written, ended with a newline.
    fn put(out: &mut (String, Vec<InterLine>), text: String, own: Option<(Vec<(usize, usize)>, bool)>) {
        out.0.push_str(&text);
        out.0.push('\n');
        out.1.push(InterLine { text, own });
    }
    let mut out = (String::new(), Vec::new());
    for (k, g) in groups.iter().enumerate() {
        let (from, to) = (g[0].1.start, g[g.len() - 1].1.end);
        if k == 0 {
            put(&mut out, format!("--- {}", names.0), None);
            put(&mut out, format!("+++ {}", names.1), None);
        }
        put(&mut out, format!("@@ -{} +{} @@", range(old[from], old[to]), range(new[from], new[to])), None);
        for r in &rows[from..to] {
            let bare = r.line.strip_suffix('\n').unwrap_or(&r.line);
            let bare = bare.strip_suffix('\r').unwrap_or(bare);
            if r.line.ends_with(['\n', '\r']) {
                // Written as it is, its own newline ending it.
                out.0.push(r.sign);
                out.0.push_str(&r.line);
                out.1.push(InterLine {
                    text: format!("{}{bare}", r.sign),
                    own: r.own.clone(),
                });
            } else {
                put(&mut out, format!("{}{bare}", r.sign), r.own.clone());
                put(&mut out, "\\ No newline at end of file".into(), None);
            }
        }
    }
    out
}

/// A line of a patch's mail, as what it is of the patch: a line of its
/// message, or of its diff; from 0.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MailLine {
    Message(usize),
    Diff(usize),
}

/// What line `m` (from 0) of `p`'s mail, as `render` writes it, is of `p`,
/// as `mail_line` has it the other way: its headers and `Subject:` its
/// title, the message's first line; its body the rest of the message; the
/// `---` before the diff, and the diff's lines, the diff's.
pub fn line_of_mail(p: &Patch, m: usize) -> MailLine {
    let mail = render(p);
    let lines: Vec<&str> = mail.lines().collect();
    let diff_at = lines.len().saturating_sub(p.diff.lines().count());
    if m + 1 >= diff_at {
        return MailLine::Diff(m.saturating_sub(diff_at));
    }
    let subject = lines.iter().position(|l| l.starts_with("Subject: ")).unwrap_or(0);
    if m <= subject {
        return MailLine::Message(0);
    }
    // The body after a blank line, the blank lines it started with gone.
    let rest = p.message.split_once('\n').map_or("", |(_, b)| b);
    let lead = rest.len() - rest.trim_start_matches('\n').len();
    let at = if m == subject + 1 { 1 } else { 1 + lead + (m - subject - 2) };
    MailLine::Message(at.min(p.message.split('\n').count().saturating_sub(1)))
}

/// Where each line of an interdiff is in the new version's mail, from 0,
/// `lines` as a stack diff has them (`interdiff`'s, its two first lines
/// gone): a line kept or added, the line it is; one taken out, the line of
/// the new mail where it was; a hunk's header, its first line.
pub fn interdiff_places(lines: &[&str]) -> Vec<usize> {
    let mut at = 0;
    lines
        .iter()
        .map(|l| {
            if let Some(head) = l.strip_prefix("@@ ") {
                // `@@ -a,b +c,d @@`: d lines of the new mail from line c.
                let new = head.split(' ').find_map(|w| w.strip_prefix('+')).unwrap_or("1");
                let (start, n) = new.split_once(',').unwrap_or((new, "1"));
                let start: usize = start.parse().unwrap_or(1);
                at = if n == "0" { start } else { start.saturating_sub(1) };
                return at;
            }
            match l.chars().next() {
                Some(' ' | '+') => {
                    at += 1;
                    at - 1
                }
                Some('\\') => at.saturating_sub(1),
                _ => at,
            }
        })
        .collect()
}

/// The previous version of `p`, if it has one.
pub fn previous<'a>(db: &'a Db, p: &Patch) -> Option<&'a Patch> {
    let versions = db.versions(&p.change);
    let i = versions.iter().position(|v| v.id == p.id)?;
    i.checked_sub(1).map(|i| versions[i])
}

/// File `path` as the patch at `at` of stack `s` leaves it, whether the
/// stack changes it or not: read from an index of the check's own, no
/// checkout made. Its text; None where it is not there, or is not text.
pub fn file_at(db: &Db, s: &Stack, at: usize, path: &str) -> Result<Option<String>> {
    let (repo, base) = base_of(s)?;
    let diffs: Vec<&str> = s.patches[..=at].iter().map(|id| db.patch(id).diff.as_str()).collect();
    let pad = crate::check::Pad::new(&repo, &base, &diffs, &[path])?;
    if let Err(k) = pad.apply_each(&diffs) {
        return err(format!("patch {} of {} does not apply on the patches below it", k + 1, s.name));
    }
    pad.text(path)
}

/// The repository and base a stack applies to, or why it has none.
pub fn base_of(s: &Stack) -> Result<(std::path::PathBuf, String)> {
    match (&s.repo, &s.base) {
        (Some(r), Some(b)) => Ok((r.into(), b.clone())),
        _ => err(format!(
            "{} has no base commit: import it from a range (`stacked import {} main..HEAD`) or set one (`stacked base {} REV`)",
            s.name, s.name, s.name
        )),
    }
}

/// The lines `with_error` puts at the top of what is edited again.
const ERROR: &str = "# stacked: ";

/// `text` to edit again, `why` it was not taken said at its top.
fn with_error(why: &str, text: &str, then: &str) -> String {
    let mut out: String = why.lines().map(|l| format!("{ERROR}{l}\n")).collect();
    out.push_str(&format!("{ERROR}{then}\n"));
    out.push_str(&strip_errors(text));
    out
}

/// `text` without the lines `with_error` put in it.
fn strip_errors(text: &str) -> String {
    text.lines()
        .filter(|l| !l.starts_with(ERROR))
        .map(|l| format!("{l}\n"))
        .collect()
}

/// Edits stack `name` as `git rebase -i` would, in $EDITOR: its todo list,
/// then each message or patch it says to; or, with `work`, the stack the
/// landed work run `work` would make. What it comes to is checked to
/// apply; if it does not, the list comes back with why at its top, and a
/// patch edited before comes back as it was left. The editor runs with no
/// lock held, so the stack is checked to be as it was before anything is
/// written. It lands for your review. What came of it. The editor runs in
/// `here`, the folder stacked runs in.
pub fn edit_stack(path: &std::path::Path, name: &str, work: Option<&str>, here: &std::path::Path) -> Result<Edited> {
    use crate::todo::{self, Action};
    let db = Db::load(path)?;
    db.writable(name)?;
    may_edit(&db, name, work)?;
    let s = edited_from(&db, name, work)?;
    let (rev, _) = db.head(name).unwrap();
    // What was last made of each message or patch edited.
    let mut texts: HashMap<String, String> = HashMap::new();
    let mut list = todo::render(&db, &s);
    /// The list as read, and the message or patch made of each line, if any.
    type Plan = (Vec<(Action, String)>, Vec<Option<Draft>>);
    let (lines, drafts) = loop {
        let edited = edit_text(&list, ".todo", here)?;
        let Some(lines) = todo::parse(&strip_errors(&edited))? else {
            return Ok(Edited::Unchanged("the list was emptied; nothing changed".into()));
        };
        // The stack taken meanwhile, by the agent or a pull from Phabricator
        // say: the list waits in the editor, why at its top.
        if let Err(e) = Db::load(path).and_then(|now| may_edit(&now, name, work)) {
            list = with_error(&e.0, &edited, "save again then, or empty the list to give up");
            continue;
        }
        let plan = || -> Result<Plan> {
            let lines = todo::resolve(&db, lines)?;
            let mut drafts = Vec::new();
            for (action, id) in &lines {
                let p = db.patch(id);
                drafts.push(match action {
                    Action::Pick | Action::Drop => None,
                    Action::Reword => {
                        let was = texts.get(id).cloned().unwrap_or_else(|| p.message.clone());
                        let message = strip_comments(&edit_text(&was, ".txt", here)?);
                        texts.insert(id.clone(), message.clone());
                        if message.is_empty() {
                            return err(format!("the message of {} was emptied", &id[..8]));
                        }
                        Some(Draft {
                            message,
                            ..Draft::from(p)
                        })
                    }
                    Action::Edit => {
                        let was = texts.get(id).cloned().unwrap_or_else(|| render(p));
                        let text = strip_errors(&edit_text(&was, ".patch", here)?);
                        texts.insert(id.clone(), text.clone());
                        Some(crate::mbox::parse_mail(&text)?)
                    }
                });
            }
            let patches: Vec<Patch> = lines
                .iter()
                .zip(&drafts)
                .filter(|((a, _), _)| *a != Action::Drop)
                .map(|((_, id), d)| match d {
                    Some(d) => revise(db.patch(id), d.clone()),
                    None => db.patch(id).clone(),
                })
                .collect();
            crate::check::stack(&s, &patches.iter().collect::<Vec<_>>())?;
            Ok((lines, drafts))
        };
        match plan() {
            Ok(p) => break p,
            Err(e) => list = with_error(&e.0, &edited, "change the list and save, or empty it to give up"),
        }
    };
    filed(path, Db::edit(path, |db| {
        may_edit(db, name, work)?;
        if db.head(name).map(|(r, _)| r) != Some(rev) || edited_from(db, name, work)?.patches != s.patches {
            return err(format!("{name} changed while it was being edited; nothing changed"));
        }
        let mut patches = Vec::new();
        let mut verbs = Vec::new();
        for ((action, id), draft) in lines.iter().zip(drafts) {
            match (action, draft) {
                (Action::Drop, _) => verbs.push(format!("drop {}", &id[..8])),
                (_, Some(d)) => {
                    let old = db.patch(id).clone();
                    let new = db.add_patch(revise(&old, d))?;
                    if new != old.id {
                        let verb = if *action == Action::Edit { "edit" } else { "reword" };
                        verbs.push(format!("{verb} {}", &id[..8]));
                    }
                    patches.push(new);
                }
                (_, None) => patches.push(id.clone()),
            }
        }
        let op = if verbs.is_empty() {
            "reorder".to_string()
        } else {
            verbs.join(", ")
        };
        let said = format!("the stack edited as a list: {op}");
        land(db, name, &s.patches, patches, &op, "the stack", &said, work)
    }))
}

/// The line (from 1) of `p`'s mail, as it is edited, that `row` of its
/// rows is: a line of its message or of its diff; else the first.
pub fn mail_line(p: &Patch, row: &crate::rows::Row) -> usize {
    let mail = render(p);
    let lines: Vec<&str> = mail.lines().collect();
    if let Some(d) = row.diff_at {
        // The diff is the mail's end.
        return lines.len().saturating_sub(p.diff.lines().count()) + d + 1;
    }
    let Some(m) = row.msg_at else { return 1 };
    let subject = lines.iter().position(|l| l.starts_with("Subject: ")).unwrap_or(0);
    if m == 0 {
        return subject + 1;
    }
    // The body follows the subject and a blank line, the blank lines that
    // started it gone.
    let rest = p.message.split_once('\n').map_or("", |(_, r)| r);
    let blank = rest.len() - rest.trim_start_matches('\n').len();
    if m <= blank {
        subject + 2
    } else {
        subject + 3 + (m - 1 - blank)
    }
}

/// Edits the patch at `at` of stack `name` in the editor, as mail, opened
/// at `line` if one is given; with `work`, as the landed work run `work`
/// would make it. If it no longer applies, or cannot be read, it comes
/// back with why at its top. It lands for your review. What came of it.
/// The editor runs in `here`, the folder stacked runs in.
pub fn edit_patch(
    path: &std::path::Path,
    name: &str,
    at: usize,
    line: Option<usize>,
    work: Option<&str>,
    here: &std::path::Path,
) -> Result<Edited> {
    let db = Db::load(path)?;
    db.writable(name)?;
    may_edit(&db, name, work)?;
    let s = edited_from(&db, name, work)?;
    let Some(old) = s.patches.get(at).map(|id| db.patch(id).clone()) else {
        return err(format!("{name} has {} patches", s.patches.len()));
    };
    let mut text = render(&old);
    let mut line = line;
    let draft = loop {
        let edited = strip_errors(&edit_file(&text, "edit.patch", line, here)?);
        if edited.trim().is_empty() {
            return Ok(Edited::Unchanged("the patch was emptied; nothing changed".into()));
        }
        // The stack taken meanwhile, by the agent or a pull from Phabricator
        // say: the edit waits in the editor, why at its top.
        if let Err(e) = Db::load(path).and_then(|now| may_edit(&now, name, work)) {
            text = with_error(&e.0, &edited, "save again then, or empty the file to give up");
            continue;
        }
        let tried = crate::mbox::parse_mail(&edited).and_then(|d| {
            let new = revise(&old, d.clone());
            let mut patches: Vec<&Patch> = s.patches.iter().map(|id| db.patch(id)).collect();
            patches[at] = &new;
            crate::check::stack(&s, &patches).map(|_| d)
        });
        match tried {
            Ok(d) => break d,
            Err(e) => {
                text = with_error(&e.0, &edited, "fix it and save, or empty the file to give up");
                // Where it was, below what was said at the top.
                line = line.map(|l| l + text.lines().take_while(|l| l.starts_with(ERROR)).count());
            }
        }
    };
    filed(path, Db::edit(path, |db| {
        may_edit(db, name, work)?;
        let now = edited_from(db, name, work)?;
        if now.patches.get(at) != Some(&old.id) {
            return err(format!("{name} changed while it was being edited; nothing changed"));
        }
        match revised(db, &now.patches, at, draft)? {
            None => Ok(Edited::Unchanged("nothing changed".into())),
            Some((patches, op)) => land(db, name, &now.patches, patches, &op, &format!("patch {}", at + 1), &format!("edited {}", old.title()), work),
        }
    }))
}

/// Drops the changes the patch at `at` of stack `name` makes to `file`:
/// that file's part of its diff gone whole, a rename's, a file's it adds
/// or deletes, as any. The patch's new version, its message and author as
/// they were, lands for your review as an edit by hand does, once the
/// patches after it are seen to still apply on it; else nothing changes,
/// and why is the error. A patch changing nothing else is not left empty:
/// it is to be dropped instead. What came of it.
pub fn drop_file(path: &std::path::Path, name: &str, at: usize, file: &str) -> Result<Edited> {
    let db = Db::load(path)?;
    db.writable(name)?;
    by_hand(&db, name)?;
    let s = db.stack(name)?.clone();
    let Some(old) = s.patches.get(at).map(|id| db.patch(id).clone()) else {
        return err(format!("{name} has {} patches", s.patches.len()));
    };
    let files = crate::diff::parse_diff(&old.diff, "");
    let Some(k) = files.iter().position(|f| f.path == file) else {
        return err(format!("{} does not change {file}", old.title()));
    };
    let diff = with_section(&old.diff, k, None);
    if !diff.lines().any(|l| l.starts_with("diff --git ")) {
        return err(format!("{} changes nothing but {file}: drop the patch instead; nothing changed", old.title()));
    }
    let draft = Draft { diff, ..Draft::from(&old) };
    let new = revise(&old, draft.clone());
    let mut all: Vec<&Patch> = s.patches.iter().map(|id| db.patch(id)).collect();
    all[at] = &new;
    crate::check::stack(&s, &all)?;
    let said = format!("dropped the changes to {file} from {}", old.title());
    filed(path, Db::edit(path, |db| {
        by_hand(db, name)?;
        let now = db.stack(name)?.clone();
        if now.patches.get(at) != Some(&old.id) {
            return err(format!("{name} changed meanwhile; nothing changed"));
        }
        match revised(db, &now.patches, at, draft)? {
            None => Ok(Edited::Unchanged("nothing changed".into())),
            Some((patches, op)) => land(db, name, &now.patches, patches, &op, &format!("patch {}", at + 1), &said, None),
        }
    }))
}

/// Wraps the long lines of the messages of the patches at `places` (from
/// 0) of stack `name` at `width` columns, as `wrap::wrapped` does: their
/// new versions, their diffs and authors as they were, land together for
/// your review, as an edit by hand does, the history calling it `wrap the
/// long lines of the messages of patches 2 and 3 at 72 columns`. Of those,
/// a patch with nothing to wrap is left as it is; none having any, nothing
/// changes. What came of it.
pub fn wrap_messages(path: &std::path::Path, name: &str, places: &[usize], width: usize) -> Result<Edited> {
    let mut places = places.to_vec();
    places.sort_unstable();
    places.dedup();
    filed(path, Db::edit(path, |db| {
        db.writable(name)?;
        by_hand(db, name)?;
        let was = db.stack(name)?.patches.clone();
        let mut patches = was.clone();
        let mut done = Vec::new();
        for &at in &places {
            let Some(id) = was.get(at) else {
                return err(format!("{name} has {} patches", was.len()));
            };
            let old = db.patch(id).clone();
            let Some(w) = crate::wrap::wrapped(&old.message, width) else { continue };
            if let Some((p, _)) = revised(db, &patches, at, Draft { message: w.message, ..Draft::from(&old) })? {
                patches = p;
                done.push(at);
            }
        }
        let messages = |n: usize| if n == 1 { "message" } else { "messages" };
        if done.is_empty() {
            let has = if places.len() == 1 { "has" } else { "have" };
            let of = crate::head::patches_said(&places);
            return Ok(Edited::Unchanged(format!("nothing changed: the {} of {of} {has} no long lines to wrap at {width} columns", messages(places.len()))));
        }
        let of = crate::head::patches_said(&done);
        let op = format!("wrap the long lines of the {} of {of} at {width} columns", messages(done.len()));
        let said = format!("wrapped the long lines of the {} of {of} at {width} columns", messages(done.len()));
        land(db, name, &was, patches, &op, &of, &said, None)
    }))
}

/// What came of an edit by hand.
#[derive(Debug, Clone, PartialEq)]
pub enum Edited {
    /// It landed, held for your review in run `run`: what it was, said.
    Landed { said: String, run: String },
    /// Nothing changed: why, said.
    Unchanged(String),
    /// The stack does not apply with it (why): the edit, kept to edit
    /// again; and, when a later patch no longer applies on it, the
    /// conflict to resolve by hand.
    Refused { why: String, kept: Kept, conflict: Option<Box<Conflict>> },
}

/// A later patch that no longer applies once patches below it are edited,
/// and the file of it to resolve by hand.
#[derive(Debug, Clone, PartialEq)]
pub struct Conflict {
    /// The stack's patches when it was edited: nothing is written if they
    /// changed since.
    pub patches: Vec<String>,
    /// The new versions of patches, by place from 0: the one edited by
    /// hand first, then those resolved since.
    pub edits: Vec<(usize, Draft)>,
    /// The patch that no longer applies on them, by place from 0.
    pub at: usize,
    /// Its file to resolve, as the patch names it after, and before.
    pub file: String,
    pub from: String,
    /// The file as the patches below leave it, with the edits.
    pub ours: String,
    /// What to resolve: the patch's change merged into `ours`, conflict
    /// markers where the two disagree; or, `left`, what was left of it.
    pub text: String,
    pub left: bool,
    /// What the edit by hand was, said.
    pub said: String,
    /// The landed work it edits, under review, if it edits one.
    pub work: Option<String>,
}

/// An edit not taken, kept to edit again.
#[derive(Debug, Clone, PartialEq)]
pub enum Kept {
    /// The file as it was left, edited as a copy.
    Text(String),
    /// A commit of the patch with the edit in it, made where it was edited.
    Commit { place: Place, commit: String },
}

/// Where a file of a patch is edited by hand.
#[derive(Debug, Clone, PartialEq)]
pub enum Place {
    /// A copy of the file alone, in a temporary folder.
    Copy,
    /// The checkout of the stack's repository at this path (where stacked
    /// runs): moved to the patch for the edit, then back where it was.
    Checkout(std::path::PathBuf),
    /// The stack's own worktree, `.stacked/NAME/user` in its repository, made
    /// the first time and kept.
    Worktree,
}

impl Place {
    /// As the menus' choice and `--in` name it.
    pub fn name(&self) -> &'static str {
        match self {
            Place::Copy => "tmp",
            Place::Checkout(_) => "pwd",
            Place::Worktree => "worktree",
        }
    }

    /// The place `name` is, a checkout being `here`, the one stacked runs
    /// in.
    pub fn named(name: &str, here: &std::path::Path) -> Place {
        match name {
            "pwd" => Place::Checkout(here.to_path_buf()),
            "worktree" => Place::Worktree,
            _ => Place::Copy,
        }
    }
}

/// Edits `file` as the patch at `at` of stack `name` leaves it, in the
/// editor at `line`, where `place` says: a copy of the file alone, or a
/// checkout of the stack up to the patch. `again`, an edit not taken
/// before, is where it starts from (and a commit, where it was made).
/// What the edit makes of the patch lands for your review once the stack
/// is seen to apply with it. With `work`, the patch is as the landed work
/// run `work` would make it. A copy is edited with the editor in `here`,
/// the folder stacked runs in.
#[allow(clippy::too_many_arguments)]
pub fn edit_code(
    path: &std::path::Path,
    name: &str,
    at: usize,
    file: &str,
    line: usize,
    place: &Place,
    again: Option<Kept>,
    work: Option<&str>,
    here: &std::path::Path,
) -> Result<Edited> {
    match (place, again) {
        (_, Some(Kept::Commit { place, commit })) => edit_in(path, name, at, file, line, &place, Some(commit), work),
        (Place::Copy, Some(Kept::Text(text))) => edit_copy(path, name, at, file, line, Some(text), work, here),
        (Place::Copy, None) => edit_copy(path, name, at, file, line, None, work, here),
        (place, _) => edit_in(path, name, at, file, line, place, None, work),
    }
}

/// Edits `file` as the patch at `at` of stack `name` leaves it, as a copy
/// alone, in the editor at `line`, from `text` if given (an edit refused
/// before), else the file. The patch's part for that file is made again
/// from what is saved. The editor runs in `here`.
#[allow(clippy::too_many_arguments)]
fn edit_copy(
    path: &std::path::Path,
    name: &str,
    at: usize,
    file: &str,
    line: usize,
    text: Option<String>,
    work: Option<&str>,
    here: &std::path::Path,
) -> Result<Edited> {
    let db = Db::load(path)?;
    db.writable(name)?;
    may_edit(&db, name, work)?;
    let s = edited_from(&db, name, work)?;
    let (repo, base) = base_of(&s)?;
    let Some(old) = s.patches.get(at).map(|id| db.patch(id).clone()) else {
        return err(format!("{name} has {} patches", s.patches.len()));
    };
    let files = crate::diff::parse_diff(&old.diff, "");
    let Some(k) = files.iter().position(|f| f.path == file) else {
        return err(format!("{} does not change {file}", old.title()));
    };
    let fd = &files[k];
    let from = from_of(fd);
    let patches: Vec<&Patch> = s.patches[..=at].iter().map(|id| db.patch(id)).collect();
    let (before, after) = crate::check::texts(&repo, &base, &patches, &from, file)?;
    let Some(after) = after else {
        return err(format!("{} deletes {file}, or it is not text: edit the patch as mail instead", old.title()));
    };
    let start = text.unwrap_or_else(|| after.clone());
    let short = file.rsplit('/').next().unwrap_or(file);
    let edited = edit_file(&start, short, Some(line), here)?;
    if edited == after {
        return Ok(Edited::Unchanged("saved as it was: nothing changed".into()));
    }
    let section = file_section(&repo, fd, &from, before.as_deref(), &edited)?;
    let diff = with_section(&old.diff, k, section.as_deref());
    if !diff.lines().any(|l| l.starts_with("diff --git ")) {
        return Ok(Edited::Refused {
            why: format!("{} would change nothing: drop it instead", old.title()),
            kept: Kept::Text(edited),
            conflict: None,
        });
    }
    let draft = Draft {
        diff,
        ..Draft::from(&old)
    };
    let new = revise(&old, draft.clone());
    let said = format!("edited {file} in {}", old.title());
    let mut all: Vec<&Patch> = s.patches.iter().map(|id| db.patch(id)).collect();
    all[at] = &new;
    if let Err(e) = crate::check::stack(&s, &all) {
        return Ok(Edited::Refused {
            why: e.0.trim_end_matches("; nothing changed").to_string(),
            kept: Kept::Text(edited),
            conflict: of_work(conflict_of(&db, &s, vec![(at, draft)], &said), work),
        });
    }
    filed(path, Db::edit(path, |db| {
        // The agent took the stack meanwhile: yours is kept, to take up again.
        if let Err(e) = may_edit(db, name, work) {
            return Ok(Edited::Refused {
                why: e.0,
                kept: Kept::Text(edited.clone()),
                conflict: None,
            });
        }
        let now = edited_from(db, name, work)?;
        if now.patches.get(at) != Some(&old.id) {
            return err(format!("{name} changed while it was being edited; nothing changed"));
        }
        match revised(db, &now.patches, at, draft)? {
            None => Ok(Edited::Unchanged("nothing changed".into())),
            Some((patches, op)) => land(db, name, &now.patches, patches, &op, &format!("patch {}", at + 1), &said, work),
        }
    }))
}

/// Edits the patch at `at` of stack `name` in a checkout of the stack up
/// to it, `file` opened in the editor at `line`: the stack's worktree, or
/// the checkout `place` names, which goes back where it was after. The
/// checkout is put at the stack's base with the patches up to it applied,
/// or at `again`, the commit an edit not taken left. What the edit changes
/// there (the files it tracks, and the files new since it started) is the
/// patch's new version, its message and author as they were.
#[allow(clippy::too_many_arguments)]
fn edit_in(
    path: &std::path::Path,
    name: &str,
    at: usize,
    file: &str,
    line: usize,
    place: &Place,
    again: Option<String>,
    work: Option<&str>,
) -> Result<Edited> {
    use crate::git::git;
    let db = Db::load(path)?;
    db.writable(name)?;
    may_edit(&db, name, work)?;
    let s = edited_from(&db, name, work)?;
    let (repo, base) = base_of(&s)?;
    let Some(old) = s.patches.get(at).map(|id| db.patch(id).clone()) else {
        return err(format!("{name} has {} patches", s.patches.len()));
    };
    let dir = match place {
        Place::Checkout(d) => {
            here_edits(&db, name, d)?;
            crate::git::checkout_of(&repo, d)?
        }
        // Made the first time under the launch lock, as the agents' are: the
        // repository's worktrees made and pruned one at a time.
        Place::Worktree => {
            let _launching = crate::ask::launch_lock(path)?;
            crate::git::kept_worktree(&repo, name, &base)?
        }
        Place::Copy => return err("a copy is not a checkout"),
    };
    // Nothing of yours to lose, and no switching midway.
    if crate::git::unfinished_tracked(&dir)? {
        return err(format!("{} has uncommitted changes: commit or stash them first; nothing changed", dir.display()));
    }
    let back = crate::git::head_of(&dir)?;
    let before = crate::git::untracked(&dir)?;
    let taken = (|| {
        match &again {
            Some(c) => {
                git(&dir, &["switch", "--quiet", "--detach", c])?;
            }
            None => {
                git(&dir, &["switch", "--quiet", "--detach", &base])?;
                let patches: Vec<&Patch> = s.patches[..=at].iter().map(|id| db.patch(id)).collect();
                crate::git::am(&dir, &crate::mbox::format_mbox(&patches))?;
            }
        }
        run_editor(&dir.join(file), Some(line), &dir)?;
        take_edit(&db, &s, at, &old, &dir, &before)
    })();
    // An edit that failed midway leaves nothing behind; a checkout of
    // yours goes back where it was.
    if taken.is_err() {
        crate::git::discard(&dir, &before);
    }
    if let Place::Checkout(_) = place {
        crate::git::discard(&dir, &before);
        crate::git::back_to(&dir, &back)?;
    }
    let kept = |commit: String| Kept::Commit { place: place.clone(), commit };
    match taken? {
        Taken::Unchanged => Ok(Edited::Unchanged("saved as it was: nothing changed".into())),
        Taken::Refused { why, commit, conflict } => Ok(Edited::Refused {
            why,
            kept: kept(commit),
            conflict: of_work(conflict, work),
        }),
        Taken::Ready { draft, said, commit } => filed(path, Db::edit(path, |db| {
            if let Err(e) = may_edit(db, name, work) {
                return Ok(Edited::Refused {
                    why: e.0,
                    kept: kept(commit.clone()),
                    conflict: None,
                });
            }
            // Edited meanwhile: yours is kept, to take up again.
            let now = edited_from(db, name, work);
            let Some(now) = now.ok().filter(|now| now.patches.get(at) == Some(&old.id)) else {
                return Ok(Edited::Refused {
                    why: format!("{name} changed while it was being edited"),
                    kept: kept(commit.clone()),
                    conflict: None,
                });
            };
            match revised(db, &now.patches, at, draft.clone())? {
                None => Ok(Edited::Unchanged("nothing changed".into())),
                Some((patches, op)) => land(db, name, &now.patches, patches, &op, &format!("patch {}", at + 1), &said, work),
            }
        })),
    }
}

/// Whether a file of stack `name` may be edited by hand in the checkout
/// at `here`, which is moved to the patch for the edit and back: only while
/// Move HEAD would move HEAD there, and no commit of HEAD's is marked `!`
/// (`head::Moves::edits_here`); else refused, with why.
fn here_edits(db: &Db, name: &str, here: &std::path::Path) -> Result<()> {
    let Some((h, m)) = crate::head::moves_of(db, name, here)? else { return Ok(()) };
    if m.edits_here() {
        return Ok(());
    }
    let why = h.lock_said().or(m.case.err()).unwrap_or_default();
    err(format!("this checkout is not offered for edits by hand: {why}; nothing changed"))
}

/// What an edit in a checkout came to, before anything is written.
enum Taken {
    Unchanged,
    /// Not taken (why); the commit holding it; the conflict a later patch
    /// has with it, to resolve by hand.
    Refused { why: String, commit: String, conflict: Option<Box<Conflict>> },
    /// The patch's new version, what it changed said, and the commit
    /// holding it.
    Ready { draft: Draft, said: String, commit: String },
}

/// The edit made in the checkout at `dir`, as the patch at `at`: what it
/// changed in the files git tracks there, and the files new since
/// `before`, put in the patch's commit, then read back from it, with the
/// patch's message and author as they were; and whether the stack applies
/// with it.
fn take_edit(
    db: &Db,
    s: &Stack,
    at: usize,
    old: &Patch,
    dir: &std::path::Path,
    before: &std::collections::BTreeSet<String>,
) -> Result<Taken> {
    use crate::git::git;
    git(dir, &["add", "--update"])?;
    let new: Vec<String> = crate::git::untracked(dir)?.difference(before).cloned().collect();
    if !new.is_empty() {
        let mut args = vec!["add", "--"];
        args.extend(new.iter().map(String::as_str));
        git(dir, &args)?;
    }
    let changed = git(dir, &["diff", "--cached", "--name-only"])?;
    let changed: Vec<&str> = changed.lines().filter(|l| !l.is_empty()).collect();
    if changed.is_empty() {
        return Ok(Taken::Unchanged);
    }
    git(dir, &["-c", "commit.gpgsign=false", "commit", "--quiet", "--amend", "--no-edit", "--no-verify", "--allow-empty"])?;
    let commit = crate::git::rev_parse(dir, "HEAD")?;
    let mail = git(dir, &["format-patch", "-1", "--stdout", "--no-signature", "--keep-subject", "HEAD"])?;
    let diff = crate::mbox::parse_mbox(&mail)?.into_iter().next().map(|d| d.diff).unwrap_or_default();
    let draft = Draft { diff, ..Draft::from(old) };
    if !draft.diff.lines().any(|l| l.starts_with("diff --git ")) {
        return Ok(Taken::Refused {
            why: format!("{} would change nothing: drop it instead", old.title()),
            commit,
            conflict: None,
        });
    }
    let new = revise(old, draft.clone());
    if new.id == old.id {
        return Ok(Taken::Unchanged);
    }
    let said = format!("edited {} in {}", said_files(&changed), old.title());
    let mut all: Vec<&Patch> = s.patches.iter().map(|id| db.patch(id)).collect();
    all[at] = &new;
    if let Err(e) = crate::check::stack(s, &all) {
        return Ok(Taken::Refused {
            why: e.0.trim_end_matches("; nothing changed").to_string(),
            commit,
            conflict: conflict_of(db, s, vec![(at, draft)], &said),
        });
    }
    Ok(Taken::Ready { draft, said, commit })
}

/// `a.txt`, `a.txt and b.txt`, `a.txt, b.txt and c.txt`.
fn said_files(files: &[&str]) -> String {
    match files {
        [] => String::new(),
        [one] => one.to_string(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// `patch 3`, `patches 3 and 4`: the places of `edits`, from 1.
fn said_places(edits: &[(usize, Draft)]) -> String {
    let mut at: Vec<usize> = edits.iter().map(|(a, _)| a + 1).collect();
    at.sort_unstable();
    at.dedup();
    let named: Vec<String> = at.iter().map(usize::to_string).collect();
    let named: Vec<&str> = named.iter().map(String::as_str).collect();
    format!("{} {}", if at.len() == 1 { "patch" } else { "patches" }, said_files(&named))
}

/// The path a file of a diff had before it: as its rename or copy says,
/// else its own.
fn from_of(fd: &crate::diff::FileDiff) -> String {
    fd.header
        .iter()
        .find_map(|l| l.strip_prefix("rename from ").or_else(|| l.strip_prefix("copy from ")))
        .map_or(fd.path.clone(), String::from)
}

/// The parts of `diff`, a file each, from its `diff --git` line on.
fn parts_of(diff: &str) -> Vec<String> {
    let mut parts: Vec<String> = Vec::new();
    for line in diff.split_inclusive('\n') {
        if line.starts_with("diff --git ") {
            parts.push(String::new());
        }
        if let Some(p) = parts.last_mut() {
            p.push_str(line);
        }
    }
    parts
}

/// The patches `olds` with `edits` made: at each place, its new version.
fn with_edits(olds: &[&Patch], edits: &[(usize, Draft)]) -> Vec<Patch> {
    let mut news: Vec<Patch> = olds.iter().map(|p| (*p).clone()).collect();
    for (at, d) in edits {
        news[*at] = revise(olds[*at], d.clone());
    }
    news
}

/// How conflict markers name two sides of a conflict resolved by hand:
/// what the edits leave, and what was there before them. The third is the
/// later patch's, `patch N`.
const OURS: &str = "your edit";
const BASE: &str = "before your edit";

/// What keeps stack `s` from applying with `edits` (the first of them the
/// edit by hand), when it is a later patch that no longer applies on them,
/// and a file of it that can be resolved by hand: one the patch changes,
/// neither adds nor deletes, and that is text on every side. `said`, what
/// the edit by hand was.
fn conflict_of(db: &Db, s: &Stack, edits: Vec<(usize, Draft)>, said: &str) -> Option<Box<Conflict>> {
    use crate::check::Pad;
    let (repo, base) = base_of(s).ok()?;
    let olds: Vec<&Patch> = s.patches.iter().map(|id| db.patch(id)).collect();
    let news = with_edits(&olds, &edits);
    let diffs: Vec<&str> = olds.iter().map(|p| p.diff.as_str()).chain(news.iter().map(|p| p.diff.as_str())).collect();
    // The patches as edited, in turn, to the first that does not apply,
    // and the first of its files that does not: as the edits leave it
    // below the patch, the file is `ours`.
    let pad = Pad::new(&repo, &base, &diffs, &[]).ok()?;
    let at = news.iter().position(|p| !pad.apply(&p.diff))?;
    if at <= edits.first()?.0 {
        return None;
    }
    let k = parts_of(&news[at].diff).iter().position(|d| !pad.would_apply(d))?;
    let fd = crate::diff::parse_diff(&news[at].diff, "").into_iter().nth(k)?;
    if fd.header.iter().any(|l| l.starts_with("new file mode ") || l.starts_with("deleted file mode ")) {
        return None;
    }
    let from = from_of(&fd);
    let ours = pad.text(&from).ok()??;
    // The patch as it was, on the patches below as they were.
    let old = Pad::new(&repo, &base, &diffs, &[]).ok()?;
    if old.apply_each(&olds[..at].iter().map(|p| p.diff.as_str()).collect::<Vec<_>>()).is_err() {
        return None;
    }
    let was = old.text(&from).ok()??;
    if !old.apply(&olds[at].diff) {
        return None;
    }
    let theirs = old.text(&fd.path).ok()??;
    let text = merge(&repo, &ours, &was, &theirs, at)?;
    Some(Box::new(Conflict {
        patches: s.patches.clone(),
        edits,
        at,
        file: fd.path,
        from,
        ours,
        text,
        left: false,
        said: said.into(),
        work: None,
    }))
}

/// `theirs`, the file as patch `at` leaves it, merged into `ours` from
/// `was`, as `git merge-file` merges them: conflict markers where the two
/// disagree, in the style the repository's git is set to
/// (`merge.conflictStyle`). None if git could not merge them.
fn merge(repo: &std::path::Path, ours: &str, was: &str, theirs: &str, at: usize) -> Option<String> {
    merge_as(repo, ours, was, theirs, [OURS, BASE, &format!("patch {}", at + 1)]).map(|(text, _)| text)
}

/// How a pull's conflict markers name the patches under a patch it makes
/// again on the versions it pulled, as the pull leaves them, and as they
/// were; the third, the patch's (`patch 3`) or the revision's diff it
/// pulled (`D12's latest diff`).
pub const PULL_OURS: &str = "the stack below, as the pull leaves it";
pub const PULL_BASE: &str = "the stack below, as it was";

/// As `merge`, the markers labelled `labels`: what `ours` is, what `was`
/// is, and what `theirs` is. The merged text, and whether it holds a
/// conflict marked.
fn merge_as(repo: &std::path::Path, ours: &str, was: &str, theirs: &str, labels: [&str; 3]) -> Option<(String, bool)> {
    use std::process::{Command, Stdio};
    use std::sync::atomic::{AtomicUsize, Ordering};
    // A folder for each merge, named as all that stacked makes in the
    // temporary directory is: the process's id first, then a count.
    static MERGES: AtomicUsize = AtomicUsize::new(0);
    let n = MERGES.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("stacked-merge-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).ok()?;
    let sides = [("ours", ours), ("was", was), ("theirs", theirs)];
    let files: Vec<std::path::PathBuf> = sides.iter().map(|(n, _)| dir.join(n)).collect();
    let written = sides.iter().zip(&files).all(|((_, text), f)| std::fs::write(f, text).is_ok());
    let mut cmd = Command::new("git");
    cmd.args(["merge-file", "-p"]);
    match crate::git::git(repo, &["config", "--get", "merge.conflictStyle"]).unwrap_or_default().trim() {
        "diff3" => {
            cmd.arg("--diff3");
        }
        "zdiff3" => {
            cmd.arg("--zdiff3");
        }
        _ => {}
    }
    cmd.args(["-L", labels[0], "-L", labels[1], "-L", labels[2]]).args(&files).current_dir(repo).stderr(Stdio::null());
    let out = if written { cmd.output().ok() } else { None };
    let _ = std::fs::remove_dir_all(&dir);
    // The number of conflicts, 127 at most; else git could not merge.
    let out = out.filter(|o| o.status.code().is_some_and(|c| (0..=127).contains(&c)))?;
    let conflicts = out.status.code().is_some_and(|c| c > 0);
    Some((String::from_utf8(out.stdout).ok()?, conflicts))
}

/// Diff `diff`, of a patch made on the diffs `was` applied in turn on
/// `base` in the repository at `repo`, made again on the diffs `ours`
/// instead, as a pull makes a patch above the versions it pulled: each file
/// of it that applies on `ours` as it is, as it is; each other merged as a
/// conflict resolved by hand is (`merge_as`), the file as `ours` leaves it,
/// as `was` left it, and as the diff left it on `was`, conflict markers
/// labelled `labels` where they disagree, its part of the diff made again
/// from that. The new diff, and the files it marks a conflict in, none when
/// each merged; or why it cannot be made so, said: a file that does not
/// apply which it adds, deletes, renames or copies, or that is not text, or
/// the diff not applying on `was` either. Nothing is written but in a pad
/// of the check's own (`check::Pad`): the repository is only read.
#[cfg(feature = "moz-phab")]
pub(crate) fn rebased(repo: &std::path::Path, base: &str, ours: &[&str], was: &[&str], diff: &str, labels: [&str; 3]) -> std::result::Result<(String, Vec<String>), String> {
    use crate::check::Pad;
    let all: Vec<&str> = ours.iter().chain(was).chain([&diff]).copied().collect();
    let now = Pad::new(repo, base, &all, &[]).map_err(|e| e.0)?;
    if now.apply_each(ours).is_err() {
        return Err("the patches under it do not apply on the stack's base".into());
    }
    let then = Pad::new(repo, base, &all, &[]).map_err(|e| e.0)?;
    if then.apply_each(was).is_err() {
        return Err("the patches it was made on cannot be read".into());
    }
    // What comes before the first file, then each file.
    let head: String = diff.split_inclusive('\n').take_while(|l| !l.starts_with("diff --git ")).collect();
    let parts = parts_of(diff);
    let files = crate::diff::parse_diff(diff, "");
    if parts.len() != files.len() {
        return Err("its files cannot be told apart".into());
    }
    let mut out = head;
    let mut marked = Vec::new();
    for (part, fd) in parts.iter().zip(&files) {
        if now.would_apply(part) {
            out.push_str(part);
            continue;
        }
        let whole = fd.header.iter().any(|l| {
            ["new file mode ", "deleted file mode ", "rename from ", "copy from ", "Binary files ", "GIT binary patch"]
                .iter()
                .any(|h| l.starts_with(h))
        });
        if whole {
            return Err(format!("its conflict in {} cannot be marked, as it adds, deletes, renames or copies the file, or the file is not text", fd.path));
        }
        let from = from_of(fd);
        let not_text = || format!("its conflict in {} cannot be marked, as the file is not text", fd.path);
        let ours_text = now.text(&from).map_err(|e| e.0)?.ok_or_else(not_text)?;
        let was_text = then.text(&from).map_err(|e| e.0)?.ok_or_else(not_text)?;
        if !then.apply(part) {
            return Err(format!("its change of {} does not apply on what it was made on either", fd.path));
        }
        let theirs_text = then.text(&fd.path).map_err(|e| e.0)?.ok_or_else(not_text)?;
        let (merged, conflicts) = merge_as(repo, &ours_text, &was_text, &theirs_text, labels).ok_or_else(|| format!("git could not merge its change of {}", fd.path))?;
        if conflicts {
            marked.push(fd.path.clone());
        }
        let section = file_section(repo, fd, &from, Some(&ours_text), &merged).map_err(|e| e.0)?;
        out.push_str(section.as_deref().unwrap_or(""));
    }
    if !out.lines().any(|l| l.starts_with("diff --git ")) {
        return Err("made again, it would change nothing".into());
    }
    Ok((out, marked))
}

/// Whether `text` still holds a conflict marker `merge` wrote for patch
/// `at`.
fn has_markers(text: &str, at: usize) -> bool {
    let marks = [format!("<<<<<<< {OURS}"), format!("||||||| {BASE}"), format!(">>>>>>> patch {}", at + 1)];
    text.lines().any(|l| marks.iter().any(|m| l.trim_end() == m))
}

/// Resolves conflict `c` by hand, in the editor: the file of the patch
/// that no longer applies, as that patch would leave it on the edits,
/// conflict markers where the two disagree. What is saved is the patch's
/// new version of the file. Once the stack applies with every edit, they
/// go in it at once, as one change; else the conflict comes back, to
/// resolve again, or the next one does. `kept`, the edit by hand as it was
/// kept, comes back with it. The editor runs in `here`, the folder stacked
/// runs in.
pub fn resolve(path: &std::path::Path, name: &str, c: &Conflict, kept: Kept, here: &std::path::Path) -> Result<Edited> {
    let work = c.work.as_deref();
    let db = Db::load(path)?;
    db.writable(name)?;
    may_edit(&db, name, work)?;
    let s = edited_from(&db, name, work)?;
    if s.patches != c.patches {
        return err(format!("{name} changed while it was being edited; nothing changed"));
    }
    let (repo, _) = base_of(&s)?;
    let olds: Vec<&Patch> = s.patches.iter().map(|id| db.patch(id)).collect();
    let patch = with_edits(&olds, &c.edits).swap_remove(c.at);
    let line = c.text.lines().position(|l| l.starts_with("<<<<<<< ")).map_or(1, |i| i + 1);
    let short = c.file.rsplit('/').next().unwrap_or(&c.file);
    let edited = edit_file(&c.text, short, Some(line), here)?;
    let again = |why: String| Edited::Refused {
        why,
        kept: kept.clone(),
        conflict: Some(Box::new(Conflict {
            text: edited.clone(),
            left: true,
            ..c.clone()
        })),
    };
    if has_markers(&edited, c.at) {
        return Ok(again(format!("{} still has conflict markers: take them out once you have resolved it", c.file)));
    }
    let files = crate::diff::parse_diff(&patch.diff, "");
    let Some(k) = files.iter().position(|f| f.path == c.file) else {
        return err(format!("patch {} no longer changes {}; nothing changed", c.at + 1, c.file));
    };
    let section = file_section(&repo, &files[k], &c.from, Some(&c.ours), &edited)?;
    let diff = with_section(&patch.diff, k, section.as_deref());
    if !diff.lines().any(|l| l.starts_with("diff --git ")) {
        return Ok(again(format!("patch {} ({}) would change nothing: drop it instead", c.at + 1, patch.title())));
    }
    let draft = Draft {
        diff,
        ..Draft::from(&patch)
    };
    let mut edits = c.edits.clone();
    match edits.iter_mut().find(|(at, _)| *at == c.at) {
        Some((_, d)) => *d = draft,
        None => edits.push((c.at, draft)),
    }
    let news = with_edits(&olds, &edits);
    if let Err(e) = crate::check::stack(&s, &news.iter().collect::<Vec<_>>()) {
        return Ok(Edited::Refused {
            why: e.0.trim_end_matches("; nothing changed").to_string(),
            kept: kept.clone(),
            conflict: of_work(conflict_of(&db, &s, edits, &c.said), work),
        });
    }
    let said = format!("{}, and resolved {} by hand", c.said, said_places(&edits[1..]));
    filed(path, Db::edit(path, |db| {
        // The agent took the stack meanwhile: what you resolved is kept, to
        // take up again.
        if let Err(e) = may_edit(db, name, work) {
            return Ok(again(e.0));
        }
        if edited_from(db, name, work)?.patches != c.patches {
            return err(format!("{name} changed while it was being edited; nothing changed"));
        }
        let (patches, op) = put_edits(db, &c.patches, &edits)?;
        land(db, name, &c.patches, patches, &op, &said_places(&edits), &said, work)
    }))
}

/// `patches` with each of `edits` the new version of the patch at its
/// place, stored, as one change: the first edited by hand, the others
/// resolved. The edit as the history says it.
fn put_edits(db: &mut Db, patches: &[String], edits: &[(usize, Draft)]) -> Result<(Vec<String>, String)> {
    let mut patches = patches.to_vec();
    let mut verbs = Vec::new();
    for (k, (at, d)) in edits.iter().enumerate() {
        let old = db.patch(&patches[*at]).clone();
        let id = db.add_patch(revise(&old, d.clone()))?;
        if id != old.id {
            let verb = if k == 0 { "edit" } else { "resolve" };
            verbs.push(format!("{verb} {} as {}", &old.id[..8], &id[..8]));
            patches[*at] = id;
        }
    }
    Ok((patches, verbs.join(", ")))
}

/// The part of a diff for one file, `fd` as it was, made again for `new`:
/// its header with the blob ids of what it is now, and the hunks from
/// `old` (the file before the patch, at `from`; None for a file the patch
/// adds). None when it no longer changes the file at all.
fn file_section(repo: &std::path::Path, fd: &crate::diff::FileDiff, from: &str, old: Option<&str>, new: &str) -> Result<Option<String>> {
    use std::process::{Command, Stdio};
    // A folder for each diff, gone once the diff is made, whatever happens:
    // two edits finishing at once, in two of StackED's windows, each diff
    // files of their own.
    let dir = crate::check::Scratch::new("diff")?;
    let (a, b) = (dir.path().join("a"), dir.path().join("b"));
    std::fs::write(&b, new).map_err(|e| Error(e.to_string()))?;
    let old_file = match old {
        Some(t) => {
            std::fs::write(&a, t).map_err(|e| Error(e.to_string()))?;
            a.clone()
        }
        None => std::path::PathBuf::from("/dev/null"),
    };
    let out = Command::new("git")
        .args(["diff", "--no-index", "--no-color", "--no-ext-diff", "-U3"])
        .arg(&old_file)
        .arg(&b)
        .current_dir(repo)
        .stderr(Stdio::null())
        .output()
        .map_err(|e| Error(format!("git: {e}")))?;
    let hash = |text: &str| -> Result<String> {
        let mut child = Command::new("git")
            .args(["hash-object", "--stdin"])
            .current_dir(repo)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| Error(format!("git: {e}")))?;
        use std::io::Write;
        child.stdin.take().unwrap().write_all(text.as_bytes()).map_err(|e| Error(e.to_string()))?;
        let out = child.wait_with_output().map_err(|e| Error(e.to_string()))?;
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    };
    let new_id = hash(new)?;
    let old_id = match old {
        Some(t) => hash(t)?,
        None => "0".repeat(40),
    };
    drop(dir);
    // 0: the same; 1: they differ; else git could not say.
    let hunks: Vec<String> = match out.status.code() {
        Some(0) => vec![],
        Some(1) => String::from_utf8_lossy(&out.stdout)
            .lines()
            .skip_while(|l| !l.starts_with("@@"))
            .map(String::from)
            .collect(),
        _ => return err("git diff could not compare the file as it was and as it is"),
    };
    let moved = fd.header.iter().any(|l| {
        ["rename from ", "copy from ", "old mode ", "new file mode ", "deleted file mode "]
            .iter()
            .any(|h| l.starts_with(h))
    });
    if hunks.is_empty() && !moved {
        return Ok(None);
    }
    let mode = fd
        .header
        .iter()
        .find_map(|l| l.strip_prefix("new file mode ").or_else(|| l.strip_prefix("new mode ")))
        .map(String::from)
        .or_else(|| {
            fd.header
                .iter()
                .find_map(|l| l.strip_prefix("index "))
                .and_then(|r| r.split_once(' ').map(|(_, m)| m.to_string()))
        });
    let mut out = vec![fd.header[0].clone()];
    for l in &fd.header[1..] {
        if l.starts_with("index ") || l.starts_with("similarity index ") || l.starts_with("dissimilarity index ") {
            continue;
        }
        if l.starts_with("--- ") || l.starts_with("+++ ") {
            continue;
        }
        out.push(l.clone());
    }
    let same_mode = !fd.header.iter().any(|l| l.starts_with("new file mode ") || l.starts_with("old mode "));
    out.push(match (&mode, same_mode) {
        (Some(m), true) => format!("index {old_id}..{new_id} {m}"),
        _ => format!("index {old_id}..{new_id}"),
    });
    if !hunks.is_empty() {
        let minus = fd.header.iter().find(|l| l.starts_with("--- ")).cloned().unwrap_or_else(|| {
            if old.is_none() { "--- /dev/null".into() } else { format!("--- a/{from}") }
        });
        let plus = fd.header.iter().find(|l| l.starts_with("+++ ")).cloned().unwrap_or_else(|| format!("+++ b/{}", fd.path));
        out.push(minus);
        out.push(plus);
        out.extend(hunks);
    }
    Ok(Some(out.into_iter().map(|l| l + "\n").collect()))
}

/// `diff` with the part of its file `k` (from 0) made `section`, or gone.
fn with_section(diff: &str, k: usize, section: Option<&str>) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut head = String::new();
    for line in diff.split_inclusive('\n') {
        if line.starts_with("diff --git ") {
            parts.push(String::new());
        }
        match parts.last_mut() {
            Some(p) => p.push_str(line),
            None => head.push_str(line),
        }
    }
    let mut out = head;
    for (i, p) in parts.iter().enumerate() {
        if i == k {
            out.push_str(section.unwrap_or(""));
        } else {
            out.push_str(p);
        }
    }
    out
}

/// Opens `text` in the editor and gives back what was saved. The editor
/// runs in `here`, the folder stacked runs in.
pub fn edit_text(text: &str, suffix: &str, here: &std::path::Path) -> Result<String> {
    edit_file(text, &format!("edit{suffix}"), None, here)
}

/// The files opened in the editor so far: each in a folder of its own, as
/// each of StackED's windows may have one open at the same time.
static EDITED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Opens `text` in the editor as a file named `name`, at `line` (from 1)
/// if one is given, and gives back what was saved. The editor is the
/// settings', else $VISUAL, $EDITOR or vi, told the line as it takes it; it
/// runs in `here`, the folder stacked runs in.
pub fn edit_file(text: &str, name: &str, line: Option<usize>, here: &std::path::Path) -> Result<String> {
    let n = EDITED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("stacked-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| Error(e.to_string()))?;
    let path = dir.join(name);
    std::fs::write(&path, text).map_err(|e| Error(e.to_string()))?;
    let ran = run_editor(&path, line, here);
    let out = std::fs::read_to_string(&path).map_err(|e| Error(e.to_string()));
    let _ = std::fs::remove_dir_all(&dir);
    ran?;
    out
}

/// What edits a file in place of the editor, told the file and the line it
/// is opened at.
pub type Editing = Box<dyn FnMut(&std::path::Path, Option<usize>) -> Result<()>>;

thread_local! {
    /// What edits files in place of the editor on this thread, when one is
    /// set: the tests', which never run one, as `ask::stand_in` is for the
    /// agent.
    static STAND_IN: std::cell::RefCell<Option<Editing>> = const { std::cell::RefCell::new(None) };
}

/// Edits the files this thread opens with `edit` rather than the editor;
/// with none, with the editor again.
pub fn stand_in(edit: Option<Editing>) {
    STAND_IN.with(|s| *s.borrow_mut() = edit);
}

/// Runs the editor on `file`, at `line` if given, in `dir`: an error if it
/// says it failed.
fn run_editor(file: &std::path::Path, line: Option<usize>, dir: &std::path::Path) -> Result<()> {
    if let Some(done) = STAND_IN.with(|s| s.borrow_mut().as_mut().map(|edit| edit(file, line))) {
        return done;
    }
    let cfg = crate::config::get();
    let editor = cfg.editor();
    // Opened as an app (from the Dock, from its launcher), stacked has no
    // terminal: the editor runs in a terminal window of its own, unless it
    // opens a window of its own.
    #[cfg(feature = "app")]
    if crate::bundle::no_terminal() && !crate::bundle::own_window(&editor) {
        return crate::bundle::edit_in_window(&cfg.editing(line), file, dir, &crate::launcher::terminal)
            .map_err(|e| Error(format!("{editor} {}", e.0)));
    }
    let mut cmd = std::process::Command::new("sh");
    cmd.arg("-c").arg(cfg.editing(line)).arg("editor").arg(file);
    // A folder stacked could not tell (its own gone) is left as it is.
    if !dir.as_os_str().is_empty() {
        cmd.current_dir(dir);
    }
    // One editor at a time in the terminal: StackED's windows share the one
    // `stacked app` was started from.
    static TERMINAL: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _terminal = TERMINAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let status = cmd.status().map_err(|e| Error(format!("{editor}: {e}")))?;
    if !status.success() {
        return err(format!("{editor} failed; nothing changed"));
    }
    Ok(())
}

/// `text` without its `#` lines, trimmed.
pub fn strip_comments(text: &str) -> String {
    text.lines()
        .filter(|l| !l.starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(title: &str, diff: &str) -> Draft {
        Draft {
            commit: None,
            author: Some("T <t@x>".into()),
            date: None,
            message: format!("{title}\n"),
            diff: diff.into(),
        }
    }

    fn adds(line: &str) -> String {
        format!("diff --git a/f b/f\nindex 1..2 100644\n--- a/f\n+++ b/f\n@@ -0,0 +1 @@\n+{line}\n")
    }

    /// A patch of change `a` saying `message`, adding `line` to f.
    fn saying(message: &str, line: &str) -> Patch {
        Patch::new("a".into(), Some("T <t@x>".into()), None, message.into(), adds(line))
    }

    /// The lines of an interdiff, as written.
    fn texts(lines: &[InterLine]) -> Vec<&str> {
        lines.iter().map(|l| l.text.as_str()).collect()
    }

    /// What of `l` only its version has, as it reads.
    fn own_said(l: &InterLine) -> Vec<String> {
        let chars: Vec<char> = l.text.chars().collect();
        l.own.iter().flat_map(|(w, _)| w.iter().map(|&(s, e)| chars[s..e].iter().collect())).collect()
    }

    const WRAPPED: &str = "Bug 1 - two\n\nThe parser reads the whole file\nbefore it checks the header.\n\nThat is all.\n";
    const REWRAPPED: &str = "Bug 1 - two\n\nThe parser reads the whole\nfile before it checks\nthe header.  \n\nThat is all.\n";

    #[test]
    fn a_paragraph_only_wrapped_otherwise_is_no_line_taken_out_or_put_in() {
        let (old, new) = (saying(WRAPPED, "x"), saying(REWRAPPED, "x"));
        let lines = interdiff_lines(&old, &new);
        // Once, as the new version writes it, among the lines around it,
        // and none of them a change: the old version's two lines of the
        // mail, the new one's three.
        assert_eq!(
            texts(&lines),
            [
                &format!("--- {}", &old.id[..8]),
                &format!("+++ {}", &new.id[..8]),
                "@@ -1,8 +1,9 @@",
                " From: T <t@x>",
                " Subject: Bug 1 - two",
                " ",
                " The parser reads the whole",
                " file before it checks",
                " the header.  ",
                " ",
                " That is all.",
                " ---",
            ]
        );
        assert!(lines.iter().all(|l| l.own.is_none()), "{lines:?}");
        // As text, the same lines.
        assert_eq!(interdiff(&old, &new).lines().collect::<Vec<_>>(), texts(&lines));
        // Each where it is in the new version's mail.
        let mail: Vec<String> = render(&new).lines().map(String::from).collect();
        let body: Vec<&str> = texts(&lines)[2..].to_vec();
        for (l, at) in body.iter().zip(interdiff_places(&body)).skip(1) {
            assert_eq!(&l[1..], mail[at], "{body:?}");
        }
    }

    #[test]
    fn to_the_letter_a_paragraph_only_wrapped_otherwise_is_each_version_s_lines_nothing_marked() {
        let (old, new) = (saying(WRAPPED, "x"), saying(REWRAPPED, "x"));
        let lines = letter_interdiff_lines(&old, &new);
        // The old version's two lines taken out, the new one's three put in,
        // as each writes them, nothing of them marked; the title and the last
        // paragraph once, among the lines around them.
        let changed: Vec<&InterLine> = lines.iter().filter(|l| l.own.is_some()).collect();
        let said: Vec<&str> = changed.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(
            said,
            ["-The parser reads the whole file", "-before it checks the header.", "+The parser reads the whole", "+file before it checks", "+the header.  "]
        );
        assert!(changed.iter().all(|l| l.own == Some((vec![], false))), "{changed:?}");
        assert!(texts(&lines).contains(&" Subject: Bug 1 - two") && texts(&lines).contains(&" That is all."), "{lines:?}");
        // A word otherwise too: as `interdiff_lines` has it, the paragraph
        // wrapped otherwise once, as the new version writes it.
        let (old, new) = (saying(WRAPPED, "x"), saying(&REWRAPPED.replace("That is all.", "That is not all."), "x"));
        assert_eq!(letter_interdiff_lines(&old, &new), interdiff_lines(&old, &new));
        // The same messages: the mails diffed line by line, as ever.
        let (old, new) = (saying(WRAPPED, "x"), saying(WRAPPED, "y"));
        assert_eq!(letter_interdiff_lines(&old, &new), interdiff_lines(&old, &new));
    }

    #[test]
    fn a_word_changed_in_a_paragraph_wrapped_otherwise_is_marked_alone() {
        let (old, new) = (saying(WRAPPED, "x"), saying(&REWRAPPED.replace("whole", "entire"), "x"));
        let lines = interdiff_lines(&old, &new);
        let changed: Vec<&InterLine> = lines.iter().filter(|l| l.own.is_some()).collect();
        // The old version's two lines taken out, the new one's three put in,
        // each as it wraps them.
        let said: Vec<&str> = changed.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(
            said,
            ["-The parser reads the whole file", "-before it checks the header.", "+The parser reads the entire", "+file before it checks", "+the header.  "]
        );
        // What only one has: the word, nothing else.
        let own: Vec<Vec<String>> = changed.iter().map(|l| own_said(l)).collect();
        assert_eq!(own, [vec!["whole".to_string()], vec![], vec!["entire".to_string()], vec![], vec![]]);
        // The title and the last paragraph, the same: no line of them taken
        // out or put in.
        assert!(texts(&lines).contains(&" Subject: Bug 1 - two") && texts(&lines).contains(&" That is all."), "{lines:?}");
    }

    #[test]
    fn a_command_wrapped_otherwise_is_taken_out_and_put_in() {
        let one = "Bug 1 - build\n\nBuild it so:\n\n$ ./mach build --foo\n";
        let (old, new) = (saying(one, "x"), saying(&one.replace("build --foo", "build\n  --foo"), "x"));
        let lines = interdiff_lines(&old, &new);
        let changed: Vec<(&str, Vec<String>, bool)> =
            lines.iter().filter_map(|l| l.own.as_ref().map(|(_, broken)| (l.text.as_str(), own_said(l), *broken))).collect();
        // The space before `--foo` in the one; the end of the line, marked,
        // and the indent, in the other.
        assert_eq!(
            changed,
            [
                ("-$ ./mach build --foo", vec![" ".to_string()], false),
                ("+$ ./mach build", vec![], true),
                ("+  --foo", vec!["  ".to_string()], false)
            ]
        );
        // The prose over it, the same.
        assert!(texts(&lines).contains(&" Build it so:"), "{lines:?}");
    }

    #[test]
    fn the_diff_after_the_message_is_diffed_line_by_line_as_ever() {
        // The message and the diff changed: the diff's lines, from the
        // `---` on, as the mails diffed line by line have them.
        let before = |p: &Patch, q: &Patch| {
            let text = similar::TextDiff::from_lines(&render(p), &render(q)).unified_diff().context_radius(3).to_string();
            text.lines().skip_while(|l| *l != " ---").map(String::from).collect::<Vec<_>>()
        };
        let (old, new) = (saying(WRAPPED, "the parser"), saying(&REWRAPPED.replace("whole", "entire"), "the new parser"));
        let lines = interdiff_lines(&old, &new);
        let after: Vec<String> = lines.iter().map(|l| l.text.clone()).skip_while(|l| l != " ---").collect();
        assert_eq!(after, before(&old, &new));
        assert!(after.contains(&"-+the parser".to_string()) && after.contains(&"++the new parser".to_string()), "{after:?}");
        // Nothing of them marked as the message's lines are: their words
        // are paired as a diff's (`rows::word_spans`).
        assert!(lines.iter().skip_while(|l| l.text != " ---").all(|l| l.own.is_none()));
        // With no line of the diff near the message, its hunks of its own.
        let far = |n: usize| (1..=n).map(|k| format!("+line {k}\n")).collect::<String>();
        let long = |message: &str, last: &str| {
            let diff = format!("diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -0,0 +1,21 @@\n{}+{last}\n", far(20));
            Patch::new("a".into(), None, None, message.into(), diff)
        };
        let (old, new) = (long(WRAPPED, "end"), long(&WRAPPED.replace("whole", "entire"), "END"));
        let text = interdiff(&old, &new);
        let hunks: Vec<&str> = text.lines().filter(|l| l.starts_with("@@")).collect();
        assert_eq!(hunks, ["@@ -1,7 +1,7 @@", "@@ -29,4 +29,4 @@"], "{text}");
        assert!(text.contains("\n-+end\n++END\n"), "{text}");
    }

    #[test]
    fn without_a_change_to_the_title_or_the_body_the_mails_are_diffed_as_ever() {
        let whole = |p: &Patch, q: &Patch| {
            similar::TextDiff::from_lines(&render(p), &render(q)).unified_diff().context_radius(3).header(&p.id[..8], &q.id[..8]).to_string()
        };
        // Its diff changed, its author too, its message not.
        let (old, new) = (saying(WRAPPED, "x"), saying(WRAPPED, "y"));
        let other = Patch::new("a".into(), Some("U <u@x>".into()), None, WRAPPED.into(), adds("y"));
        for (p, q) in [(&old, &new), (&old, &other)] {
            assert_eq!(interdiff(p, q), whole(p, q));
            let lines = interdiff_lines(p, q);
            assert_eq!(texts(&lines), whole(p, q).lines().collect::<Vec<_>>());
            assert!(lines.iter().all(|l| l.own.is_none()));
        }
        // A message that only gains empty lines at its ends: its mail the same.
        let spaced = saying(&format!("{}\n\n", WRAPPED.replacen("\n\n", "\n\n\n", 1)), "y");
        assert_eq!(interdiff(&old, &spaced), whole(&old, &spaced));
    }

    /// A diff of two files: f, `d` added in a hunk at line `start`, under
    /// the heading `fn f()`; then g, a line changed.
    fn hunk_at(start: usize) -> String {
        format!(
            "diff --git a/f b/f\nindex 1..2 100644\n--- a/f\n+++ b/f\n@@ -{start},6 +{start},7 @@ fn f()\n a\n b\n c\n+d\n e\n f\n g\n\
             diff --git a/g b/g\nindex 3..4 100644\n--- a/g\n+++ b/g\n@@ -1 +1 @@\n-x\n+y\n"
        )
    }

    #[test]
    fn a_diff_moved_down_its_file_by_a_change_below_has_the_same_effect() {
        let at = hunk_at(336);
        // Lines more above it, from a patch below: its hunk starts lower,
        // under another heading or none, and its blobs are others.
        assert!(same_effect(&at, &hunk_at(348)));
        assert!(same_effect(&at, &at.replace("@@ fn f()", "@@ fn helper()")));
        assert!(same_effect(&at, &at.replace(" @@ fn f()", " @@")));
        assert!(same_effect(&at, &at.replace("index 1..2 100644", "index 5..6 100644")));
        assert!(same_effect(&at, &at.replace("index 3..4 100644\n", "")));
        // A header's line ending in white space or not, as git ends `---`
        // and `+++` with a tab for a name holding a space; an empty line in
        // a hunk, the empty context line git applies it as; the similarity
        // of a rename.
        let spaced = |end: &str| format!("diff --git a/f g b/f g\n--- a/f g{end}\n+++ b/f g{end}\n@@ -1,3 +1,3 @@\n a\n-b\n+B\n \n");
        assert!(same_effect(&spaced("\t"), &spaced("")));
        assert!(same_effect(&spaced(""), &spaced("").replace("+B\n \n", "+B\n\n")));
        let renamed = |n: u32| format!("diff --git a/f b/h\nsimilarity index {n}%\nrename from f\nrename to h\nindex 1..2 100644\n--- a/f\n+++ b/h\n@@ -1 +1 @@\n-x\n+y\n");
        assert!(same_effect(&renamed(90), &renamed(92)));
        // A line added, removed or kept other (it would apply elsewhere),
        // white space a line added ends with too.
        assert!(!same_effect(&at, &at.replace("+d", "+D")));
        assert!(!same_effect(&at, &at.replace("-x", "-X")));
        assert!(!same_effect(&at, &at.replace(" e\n", " E\n")));
        assert!(!same_effect(&at, &at.replace("+d\n", "+d \n")));
        // A file fewer, one more, or the two in another order.
        let (f, g) = at.split_at(at.find("diff --git a/g").unwrap());
        assert!(!same_effect(&at, f));
        assert!(!same_effect(&at, &format!("{at}diff --git a/h b/h\nnew file mode 100644\nindex 0..5\n--- /dev/null\n+++ b/h\n@@ -0,0 +1 @@\n+h\n")));
        assert!(!same_effect(&at, &format!("{g}{f}")));
        // A file's mode changed too, or a file added with another mode.
        assert!(!same_effect(&at, &at.replace("index 1..2 100644\n", "old mode 100644\nnew mode 100755\nindex 1..2\n")));
        let added = |mode: &str| format!("diff --git a/h b/h\nnew file mode {mode}\nindex 0..5\n--- /dev/null\n+++ b/h\n@@ -0,0 +1 @@\n+h\n");
        assert!(!same_effect(&added("100644"), &added("100755")));
        // The same lines, its hunk split in two.
        let merged = "diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -1,8 +1,10 @@\n a\n+b\n c\n d\n e\n f\n g\n+h\n i\n j\n";
        let split = "diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -1,4 +1,5 @@\n a\n+b\n c\n d\n@@ -5,4 +6,5 @@\n e\n f\n g\n+h\n i\n j\n";
        assert!(!same_effect(merged, split));
        // A draft so moved, its title other, is its patch's new version.
        let old = Patch::new("a".into(), Some("T <t@x>".into()), None, "Bug XXXXXXX - A\n".into(), at.clone());
        let planned = plan_drafts(std::slice::from_ref(&old), vec![draft("Bug 2084269 - A", &hunk_at(348))], &Written::default());
        assert_eq!(planned[0].change, "a");
    }

    /// A diff of f, from thirty lines to the same with lines 10 and 20
    /// changed and a line put in after line 21, as git writes it with `n`
    /// lines of context (`-U5`, `-U10`): with three, in two hunks; with five
    /// or more, in one, as the context merges them.
    fn with_context(n: usize) -> String {
        let old: String = (1..=30).map(|k| format!("line {k}\n")).collect();
        let new = old.replace("line 10\n", "LINE 10\n").replace("line 20\n", "LINE 20\n").replace("line 21\n", "line 21\nnew\n");
        let d = similar::TextDiff::from_lines(&old, &new);
        let hunks = d.unified_diff().context_radius(n).header("a/f", "b/f").to_string();
        format!("diff --git a/f b/f\nindex 1..2 100644\n{hunks}")
    }

    /// A diff of f removing line a and adding line b, `gap` between them.
    fn gapped(gap: &str) -> String {
        format!("diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -1,2 +1,2 @@\n-a\n{gap}+b\n")
    }

    #[test]
    fn a_diff_with_more_context_or_its_hunks_otherwise_makes_the_same_changes() {
        // More context, as moz-phab sends: the hunks merged by it, which
        // `same_effect` tells apart.
        let three = with_context(3);
        assert_eq!(three.matches("\n@@ ").count(), 2, "{three}");
        for n in [5, 10] {
            let wide = with_context(n);
            assert_eq!(wide.matches("\n@@ ").count(), 1, "{wide}");
            assert!(same_changes(&three, &wide) && !same_effect(&three, &wide), "{wide}");
        }
        // A hunk split in two by its context, or merged.
        let merged = "diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -1,8 +1,10 @@\n a\n+b\n c\n d\n e\n f\n g\n+h\n i\n j\n";
        let split = "diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -1,4 +1,5 @@\n a\n+b\n c\n d\n@@ -5,4 +6,5 @@\n e\n f\n g\n+h\n i\n j\n";
        assert!(same_changes(merged, split));
        // Its hunk starting elsewhere, under another heading, other blobs or
        // none named, other lines kept around it.
        let at = hunk_at(336);
        assert!(same_changes(&at, &hunk_at(348)));
        assert!(same_changes(&at, &at.replace("@@ fn f()", "@@ fn helper()")));
        assert!(same_changes(&at, &at.replace("index 1..2 100644\n", "")));
        assert!(same_changes(&at, &at.replace(" e\n", " E\n")));
        // An empty line in a hunk, the empty context line git applies it as.
        assert!(same_changes(&gapped(" \n"), &gapped("\n")));
        // The files in another order.
        let (f, g) = at.split_at(at.find("diff --git a/g").unwrap());
        let swapped = format!("{g}{f}");
        assert!(same_changes(&at, &swapped) && !same_effect(&at, &swapped));
        // More context reaching the end of a file that has no newline there,
        // which git says after the last line.
        let end = |hunk: &str| format!("diff --git a/e b/e\n--- a/e\n+++ b/e\n{hunk}");
        let near = end("@@ -1,3 +1,3 @@\n 1\n-2\n+two\n 3\n");
        let far = end("@@ -1,5 +1,5 @@\n 1\n-2\n+two\n 3\n 4\n 5\n\\ No newline at end of file\n");
        assert!(same_changes(&near, &far));
    }

    #[test]
    fn a_diff_with_another_line_file_mode_or_end_of_file_makes_other_changes() {
        let at = hunk_at(336);
        // A line added or removed otherwise, white space a line added ends
        // with too.
        assert!(!same_changes(&at, &at.replace("+d", "+D")));
        assert!(!same_changes(&at, &at.replace("-x", "-X")));
        assert!(!same_changes(&at, &at.replace("+d\n", "+d \n")));
        // A file more, or fewer.
        let (f, _) = at.split_at(at.find("diff --git a/g").unwrap());
        assert!(!same_changes(&at, f));
        assert!(!same_changes(&at, &format!("{at}diff --git a/h b/h\nnew file mode 100644\nindex 0..5\n--- /dev/null\n+++ b/h\n@@ -0,0 +1 @@\n+h\n")));
        // A file's mode changed too, or a file added with another mode.
        assert!(!same_changes(&at, &at.replace("index 1..2 100644\n", "old mode 100644\nnew mode 100755\nindex 1..2\n")));
        let added = |mode: &str| format!("diff --git a/h b/h\nnew file mode {mode}\nindex 0..5\n--- /dev/null\n+++ b/h\n@@ -0,0 +1 @@\n+h\n");
        assert!(!same_changes(&added("100644"), &added("100755")));
        // A line added with no newline at its end, or with one.
        let eof = "diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -1,2 +1,2 @@\n a\n-b\n+B\n\\ No newline at end of file\n";
        assert!(!same_changes(eof, &eof.replace("\\ No newline at end of file\n", "")));
        // A run split in two by a line kept between its lines, an empty one
        // too.
        assert!(!same_changes(&gapped(""), &gapped(" x\n")));
        assert!(!same_changes(&gapped(""), &gapped("\n")));
        // The same lines changed one after another, but a run of them split
        // across two files otherwise.
        let two = |f: &str, g: &str| format!("diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -1,2 +1,2 @@\n a\n{f}diff --git a/g b/g\n--- a/g\n+++ b/g\n@@ -1,2 +1,2 @@\n c\n{g}");
        assert!(!same_changes(&two("-b\n+B\n", "-d\n+D\n"), &two("-b\n", "+B\n-d\n+D\n")));
    }

    #[test]
    fn a_draft_with_the_message_author_and_effect_of_its_patch_is_that_patch_as_stored() {
        let old = Patch::new("a".into(), Some("T <t@x>".into()), Some("Mon, 2 Mar 2026 10:00:00 +0000".into()), "A\n".into(), hunk_at(336));
        // Moved down its file under another heading, its commit made again
        // at another time: the patch, its id and its diff as they were.
        let moved = Draft {
            date: Some("Tue, 3 Mar 2026 11:00:00 +0000".into()),
            ..draft("A", &hunk_at(348).replace("@@ fn f()", "@@ fn helper()"))
        };
        let planned = plan_drafts(std::slice::from_ref(&old), vec![moved.clone()], &Written::default());
        assert_eq!(planned, std::slice::from_ref(&old));
        // From the commit it was given, read back as git writes it now: the
        // same.
        let written = Written {
            commits: HashMap::from([("c1".to_string(), old.id.clone())]),
            ..Default::default()
        };
        let from = Draft { commit: Some("c1".into()), ..moved.clone() };
        assert_eq!(plan_drafts(std::slice::from_ref(&old), vec![from], &written), std::slice::from_ref(&old));
        // Its message other, its author, or its effect: its new version,
        // with the diff as git writes it now.
        for d in [
            Draft { message: "A, said otherwise\n".into(), ..moved.clone() },
            Draft { author: Some("U <u@x>".into()), ..moved.clone() },
            draft("A", &hunk_at(336).replace("+d", "+D")),
        ] {
            let p = plan_drafts(std::slice::from_ref(&old), vec![d.clone()], &Written::default()).remove(0);
            assert!(p.change == "a" && p.id != old.id && p.diff == d.diff, "{d:?}");
        }
        // With nothing to check its diff as stored on, it is its draft's
        // new version too.
        let p = plan_checked(std::slice::from_ref(&old), vec![moved.clone()], &Written::default(), None).remove(0);
        assert!(p.change == "a" && p.id != old.id && p.diff == moved.diff, "{p:?}");
    }

    #[test]
    fn a_draft_with_the_diff_of_a_patch_is_its_new_version_reworded() {
        let old = |change: &str, title: &str, diff: &str| {
            Patch::new(change.into(), Some("T <t@x>".into()), None, format!("{title}\n"), diff.into())
        };
        let olds = [old("a", "A", &adds("x")), old("b", "B", &adds("y")), old("c", "C", &adds("x")), old("e", "E", "")];
        let drafts = vec![
            // Of A and C, the same diff: the one with its title.
            draft("C", &adds("x")),
            // Its message other, its blobs too: A, reworded.
            draft("A, said otherwise", &adds("x").replace("index 1..2", "index 3..4")),
            // Its code other, its title kept: B.
            draft("B", &adds("z")),
            // A diff of nothing, its title other: not E.
            draft("F", ""),
        ];
        let planned = plan_drafts(&olds, drafts, &Written::default());
        let changes: Vec<&str> = planned.iter().map(|p| p.change.as_str()).collect();
        assert_eq!(changes[..3], ["c", "a", "b"]);
        assert!(!["a", "b", "c", "e"].contains(&changes[3]), "{changes:?}");
        assert_eq!(planned[0].id, olds[2].id);
    }

    #[test]
    fn a_draft_with_what_git_gave_back_of_its_patch_s_message_and_author_has_them_as_stored() {
        // Two empty lines under the title, two spaces at its end, and a name
        // that format-patch quotes: git gives them back otherwise from the
        // commit stacked made of the patch, c1.
        let old = Patch::new("a".into(), Some("Thor, A. <t@x>".into()), None, "A  \n\n\nOne.\n\nTwo.".into(), adds("x"));
        let back = Draft {
            author: Some("\"Thor, A.\" <t@x>".into()),
            message: "A\n\nOne.\n\nTwo.".into(),
            ..draft("A", &adds("x"))
        };
        let written = Written {
            commits: HashMap::from([("c1".to_string(), old.id.clone())]),
            drafts: HashMap::from([(old.id.clone(), Draft { commit: Some("c1".into()), ..back.clone() })]),
        };
        // That commit as it was, or made again as a rebase that rewords
        // another makes it: the patch itself, as stored.
        for commit in ["c1", "c2"] {
            let d = Draft { commit: Some(commit.into()), ..back.clone() };
            assert_eq!(plan_drafts(std::slice::from_ref(&old), vec![d], &written), std::slice::from_ref(&old), "{commit}");
        }
        // Its code changed, its commit made again: found by the title git
        // gave back, its new version, with the message and the author as
        // stored.
        let d = Draft { commit: Some("c2".into()), diff: adds("y"), ..back.clone() };
        let p = plan_drafts(std::slice::from_ref(&old), vec![d], &written).remove(0);
        assert!(p.change == "a" && p.diff == adds("y") && p.message == old.message && p.author == old.author, "{p:?}");
        // Its diff as stored kept, then nothing to check it on: its draft's
        // new version, with the message and the author as stored too.
        let moved = Draft { diff: adds("x").replace("index 1..2", "index 3..4"), ..back.clone() };
        let p = plan_checked(std::slice::from_ref(&old), vec![moved.clone()], &written, None).remove(0);
        assert!(p.change == "a" && p.diff == moved.diff && p.message == old.message && p.author == old.author, "{p:?}");
        // Its message changed otherwise, if only in its white space (an empty
        // line more, spaces at the end of a line, as a commit made with its
        // message verbatim may have them): its new version, with that
        // message, the author as stored.
        for message in ["A\n\nOne, said otherwise.\n\nTwo.", "A\n\nOne.\n\n\nTwo.", "A\n\nOne.  \n\nTwo."] {
            let d = Draft { message: message.into(), ..back.clone() };
            let p = plan_drafts(std::slice::from_ref(&old), vec![d], &written).remove(0);
            assert!(p.change == "a" && p.id != old.id && p.message == message && p.author == old.author, "{p:?}");
        }
        // Another author: its new version, with that author, the message as
        // stored.
        let d = Draft { author: Some("U <u@x>".into()), ..back.clone() };
        let p = plan_drafts(std::slice::from_ref(&old), vec![d], &written).remove(0);
        assert!(p.change == "a" && p.id != old.id && p.message == old.message && p.author.as_deref() == Some("U <u@x>"), "{p:?}");
    }

    #[test]
    fn a_line_of_an_interdiff_is_where_it_is_in_the_new_version() {
        let patch = |message: &str, line: &str| Patch::new("a".into(), Some("T <t@x>".into()), None, message.into(), adds(line));
        let (old, new) = (patch("A\n\nBody.\n", "x"), patch("A, said otherwise\n\nBody.\n", "y"));
        let text = interdiff(&old, &new);
        let lines: Vec<&str> = text.lines().skip_while(|l| l.starts_with("--- ") || l.starts_with("+++ ")).collect();
        let places = interdiff_places(&lines);
        let mail: Vec<String> = render(&new).lines().map(String::from).collect();
        // A line kept, or added, is that line of the new mail.
        for (l, at) in lines.iter().zip(&places) {
            if let Some(s) = l.strip_prefix(' ').or_else(|| l.strip_prefix('+')) {
                assert_eq!(mail[*at], s, "{l:?} in {lines:?}");
            }
        }
        // Each is a line of the patch: of its message, of its diff; one
        // taken out, the line in its place.
        let of = |s: &str| lines.iter().zip(&places).find(|(l, _)| **l == s).map(|(_, at)| line_of_mail(&new, *at));
        // As `mail_line` has it the other way, for the patch's own lines:
        // not the mail's headers, nor the `---` before the diff.
        let own = |at: usize| !(mail[at].starts_with("From: ") || mail[at].starts_with("Date: ") || mail[at] == "---");
        for (l, at) in lines.iter().zip(&places).filter(|(l, at)| (l.starts_with(' ') || l.starts_with('+')) && own(**at)) {
            let rows = crate::rows::patch_rows(&new, &[], 80, &crate::rows::Deco::default());
            let row = match line_of_mail(&new, *at) {
                MailLine::Message(i) => rows.iter().find(|r| r.msg_at == Some(i)),
                MailLine::Diff(j) => rows.iter().find(|r| r.diff_at == Some(j)),
            };
            if let Some(row) = row {
                assert_eq!(mail_line(&new, row), at + 1, "{l:?}");
            }
        }
        assert_eq!(of("-Subject: A"), Some(MailLine::Message(0)));
        assert_eq!(of("+Subject: A, said otherwise"), Some(MailLine::Message(0)));
        assert_eq!(of(" Body."), Some(MailLine::Message(2)));
        assert_eq!(of("-+x"), Some(MailLine::Diff(5)));
        assert_eq!(of("++y"), Some(MailLine::Diff(5)));
    }

    #[test]
    fn two_code_edits_at_once_each_diff_in_a_folder_of_their_own() {
        // A repository for git to run in, as an edit's has.
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path();
        let init = std::process::Command::new("git").args(["init", "-q"]).current_dir(repo).status().unwrap();
        assert!(init.success());
        let fd = crate::diff::parse_diff("diff --git a/f b/f\nindex 1..2 100644\n--- a/f\n+++ b/f\n@@ -1 +1 @@\n-one\n+two\n", "")
            .remove(0);
        // What this process has made in the temporary directory for diffs:
        // an entry with another id first is another process's.
        let ours = || -> std::collections::BTreeSet<String> {
            let mine = format!("stacked-diff-{}-", std::process::id());
            std::fs::read_dir(std::env::temp_dir())
                .unwrap()
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n.starts_with(&mine))
                .collect()
        };
        let before = ours();
        // Two edits of the file saved at once, each many times over: each
        // diff is of its own edit, and none fails for the other's files
        // written over or removed.
        let start = std::sync::Barrier::new(2);
        std::thread::scope(|s| {
            for side in ["left", "right"] {
                let (fd, start) = (&fd, &start);
                s.spawn(move || {
                    start.wait();
                    for k in 0..25 {
                        let saved = format!("{side} {k}\n");
                        let section = file_section(repo, fd, "f", Some("one\n"), &saved).unwrap().unwrap();
                        assert!(section.ends_with(&format!("@@ -1 +1 @@\n-one\n+{saved}")), "{section}");
                    }
                });
            }
        });
        // Each folder is gone once its diff is made.
        let left: Vec<String> = ours().difference(&before).cloned().collect();
        assert!(left.is_empty(), "left in {}: {left:?}", std::env::temp_dir().display());
    }
}
