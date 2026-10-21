//! Edits to a stack, as the commands and the UI make them.

use std::collections::HashMap;

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

/// The patches `drafts` are, given the ones they replace: a draft from a
/// commit in `by_commit` is that patch unless it changed; else one with the
/// diff of a patch it replaces is a new version of it, its message alone
/// changed if any (of several with that diff, the one with its title);
/// else one with its title; else it is new. A diff of nothing is no
/// patch's own. Nothing is stored.
pub fn plan_drafts(olds: &[Patch], drafts: Vec<Draft>, by_commit: &HashMap<String, String>) -> Vec<Patch> {
    let mut left: Vec<&Patch> = olds.iter().collect();
    let mut out = Vec::new();
    for d in drafts {
        let from_commit = d
            .commit
            .as_ref()
            .and_then(|c| by_commit.get(c))
            .and_then(|id| left.iter().position(|o| &o.id == id));
        // Its code as it was, whatever its commit: reworded, or rebased.
        let code = |o: &Patch| !d.diff.trim().is_empty() && same_diff(&o.diff, &d.diff);
        let at = from_commit
            .or_else(|| left.iter().position(|o| code(o) && o.title() == d.title()))
            .or_else(|| left.iter().position(|o| code(o)))
            .or_else(|| left.iter().position(|o| o.title() == d.title()));
        out.push(match at {
            Some(i) => {
                let old = left.remove(i);
                let mut d = d;
                if d.author.is_none() {
                    d.author = old.author.clone();
                }
                revise(old, d)
            }
            None => new_patch(d),
        });
    }
    out
}

/// Whether diffs `a` and `b` make the same change, whatever blobs their
/// `index` lines name and wherever their hunks start: a commit reworded,
/// rebased as it was, or moved down its files by a change below it.
pub fn same_diff(a: &str, b: &str) -> bool {
    fn change(d: &str) -> impl Iterator<Item = std::borrow::Cow<'_, str>> {
        d.lines().filter(|l| !l.starts_with("index ")).map(unplaced)
    }
    change(a).eq(change(b))
}

/// Line `l` of a diff, a hunk's header without where the hunk starts:
/// `@@ -336,6 +336,43 @@ f()` as `@@ -,6 +,43 @@ f()`.
fn unplaced(l: &str) -> std::borrow::Cow<'_, str> {
    let Some((ranges, rest)) = l.strip_prefix("@@ ").and_then(|r| r.split_once(" @@")) else {
        return std::borrow::Cow::Borrowed(l);
    };
    let ranges: Vec<String> = ranges
        .split(' ')
        .map(|r| {
            let (side, at) = r.split_at(r.len().min(1));
            at.split_once(',').map_or(side.to_string(), |(_, n)| format!("{side},{n}"))
        })
        .collect();
    std::borrow::Cow::Owned(format!("@@ {} @@{rest}", ranges.join(" ")))
}

/// As `plan_drafts`, the patches stored: their ids.
pub fn match_drafts(
    db: &mut Db,
    olds: &[Patch],
    drafts: Vec<Draft>,
    by_commit: &HashMap<String, String>,
) -> Result<Vec<String>> {
    plan_drafts(olds, drafts, by_commit)
        .into_iter()
        .map(|p| db.add_patch(p))
        .collect()
}

/// Puts `drafts` on top of `name`, making it if need be. With `replace`,
/// they are the whole stack instead, and a draft with the diff or the title
/// of a patch it held is a new version of that patch: so importing a stack
/// again after rebasing it, or rewording its commits, keeps each patch's
/// history.
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
    patches.extend(match_drafts(db, &olds, drafts, &HashMap::new())?);
    if replace || on.is_none() {
        on = base.or(on);
    }
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

/// What changed from `old` to `new`, their mails diffed.
pub fn interdiff(old: &Patch, new: &Patch) -> String {
    let (a, b) = (render(old), render(new));
    similar::TextDiff::from_lines(&a, &b)
        .unified_diff()
        .context_radius(3)
        .header(&old.id[..8], &new.id[..8])
        .to_string()
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

/// Puts stack `name` into the worktree of its repository: its base checked
/// out, its patches applied as commits, on `branch` if named. The one thing
/// stacked does to a worktree, and only when asked. The result is a run.
pub fn apply_stack(
    db_path: &std::path::Path,
    name: &str,
    branch: Option<&str>,
) -> Result<crate::db::Run> {
    use crate::db::{Run, RunState};
    let db = Db::load(db_path)?;
    let s = db.stack(name)?;
    let (rev, _) = db.head(name).unwrap();
    let (repo, base) = base_of(s)?;
    let patches: Vec<_> = s.patches.iter().map(|id| db.patch(id)).collect();
    let applied = crate::git::apply(&repo, &base, &crate::mbox::format_mbox(&patches), branch);
    let (state, summary) = match &applied {
        Ok(head) => (
            RunState::Done,
            format!(
                "@{rev} applied in {}: {} at {head}",
                repo.display(),
                branch.unwrap_or("detached HEAD")
            ),
        ),
        Err(e) => (RunState::Failed, e.0.clone()),
    };
    let run = Run {
        id: format!("apply-{}", crate::db::unique()),
        what: "apply".into(),
        stack: name.into(),
        state,
        summary,
        rev: Some(rev),
        ..Default::default()
    };
    Db::edit(db_path, |db| db.add_run(run.clone()))?;
    applied.map(|_| run)
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
/// written. It lands for your review. What came of it.
pub fn edit_stack(path: &std::path::Path, name: &str, work: Option<&str>) -> Result<Edited> {
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
        let edited = edit_text(&list, ".todo")?;
        let Some(lines) = todo::parse(&strip_errors(&edited))? else {
            return Ok(Edited::Unchanged("the list was emptied; nothing changed".into()));
        };
        // The agent took the stack meanwhile: the list waits in the editor.
        if let Err(e) = Db::load(path).and_then(|now| may_edit(&now, name, work)) {
            list = with_error(&e.0, &edited, "save again once the agent's work has landed, or empty the list to give up");
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
                        let message = strip_comments(&edit_text(&was, ".txt")?);
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
                        let text = strip_errors(&edit_text(&was, ".patch")?);
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
    Db::edit(path, |db| {
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
    })
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
pub fn edit_patch(path: &std::path::Path, name: &str, at: usize, line: Option<usize>, work: Option<&str>) -> Result<Edited> {
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
        let edited = strip_errors(&edit_file(&text, "edit.patch", line)?);
        if edited.trim().is_empty() {
            return Ok(Edited::Unchanged("the patch was emptied; nothing changed".into()));
        }
        // The agent took the stack meanwhile: the edit waits in the editor.
        if let Err(e) = Db::load(path).and_then(|now| may_edit(&now, name, work)) {
            text = with_error(&e.0, &edited, "save again once the agent's work has landed, or empty the file to give up");
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
    Db::edit(path, |db| {
        may_edit(db, name, work)?;
        let now = edited_from(db, name, work)?;
        if now.patches.get(at) != Some(&old.id) {
            return err(format!("{name} changed while it was being edited; nothing changed"));
        }
        match revised(db, &now.patches, at, draft)? {
            None => Ok(Edited::Unchanged("nothing changed".into())),
            Some((patches, op)) => land(db, name, &now.patches, patches, &op, &format!("patch {}", at + 1), &format!("edited {}", old.title()), work),
        }
    })
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

    /// The place `name` is, a checkout being the one stacked runs in.
    pub fn named(name: &str) -> Place {
        match name {
            "pwd" => Place::Checkout(std::env::current_dir().unwrap_or_default()),
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
/// run `work` would make it.
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
) -> Result<Edited> {
    match (place, again) {
        (_, Some(Kept::Commit { place, commit })) => edit_in(path, name, at, file, line, &place, Some(commit), work),
        (Place::Copy, Some(Kept::Text(text))) => edit_copy(path, name, at, file, line, Some(text), work),
        (Place::Copy, None) => edit_copy(path, name, at, file, line, None, work),
        (place, _) => edit_in(path, name, at, file, line, place, None, work),
    }
}

/// Edits `file` as the patch at `at` of stack `name` leaves it, as a copy
/// alone, in the editor at `line`, from `text` if given (an edit refused
/// before), else the file. The patch's part for that file is made again
/// from what is saved.
fn edit_copy(
    path: &std::path::Path,
    name: &str,
    at: usize,
    file: &str,
    line: usize,
    text: Option<String>,
    work: Option<&str>,
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
    let edited = edit_file(&start, short, Some(line))?;
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
    Db::edit(path, |db| {
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
    })
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
        Place::Checkout(d) => crate::git::checkout_of(&repo, d)?,
        Place::Worktree => crate::git::kept_worktree(&repo, name, &base)?,
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
        run_editor(&dir.join(file), Some(line), Some(&dir))?;
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
        Taken::Ready { draft, said, commit } => Db::edit(path, |db| {
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
        }),
    }
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
    use std::process::{Command, Stdio};
    let dir = std::env::temp_dir().join(format!("stacked-merge-{}", crate::db::unique()));
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
    let label = format!("patch {}", at + 1);
    cmd.args(["-L", OURS, "-L", BASE, "-L", &label]).args(&files).current_dir(repo).stderr(Stdio::null());
    let out = if written { cmd.output().ok() } else { None };
    let _ = std::fs::remove_dir_all(&dir);
    // The number of conflicts, 127 at most; else git could not merge.
    let out = out.filter(|o| o.status.code().is_some_and(|c| (0..=127).contains(&c)))?;
    String::from_utf8(out.stdout).ok()
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
/// kept, comes back with it.
pub fn resolve(path: &std::path::Path, name: &str, c: &Conflict, kept: Kept) -> Result<Edited> {
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
    let edited = edit_file(&c.text, short, Some(line))?;
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
    Db::edit(path, |db| {
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
    })
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
    let dir = std::env::temp_dir().join(format!("stacked-diff-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| Error(e.to_string()))?;
    let (a, b) = (dir.join("a"), dir.join("b"));
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
    let _ = std::fs::remove_dir_all(&dir);
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

/// Opens `text` in the editor and gives back what was saved.
pub fn edit_text(text: &str, suffix: &str) -> Result<String> {
    edit_file(text, &format!("edit{suffix}"), None)
}

/// Opens `text` in the editor as a file named `name`, at `line` (from 1)
/// if one is given, and gives back what was saved. The editor is the
/// settings', else $VISUAL, $EDITOR or vi, told the line as it takes it.
pub fn edit_file(text: &str, name: &str, line: Option<usize>) -> Result<String> {
    let dir = std::env::temp_dir().join(format!("stacked-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| Error(e.to_string()))?;
    let path = dir.join(name);
    std::fs::write(&path, text).map_err(|e| Error(e.to_string()))?;
    let ran = run_editor(&path, line, None);
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

/// Runs the editor on `file`, at `line` if given, in `dir` if given (else
/// where stacked runs): an error if it says it failed.
fn run_editor(file: &std::path::Path, line: Option<usize>, dir: Option<&std::path::Path>) -> Result<()> {
    if let Some(done) = STAND_IN.with(|s| s.borrow_mut().as_mut().map(|edit| edit(file, line))) {
        return done;
    }
    let cfg = crate::config::get();
    let editor = cfg.editor();
    let mut cmd = std::process::Command::new("sh");
    cmd.arg("-c").arg(cfg.editing(line)).arg("editor").arg(file);
    if let Some(dir) = dir {
        cmd.current_dir(dir);
    }
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

    #[test]
    fn a_diff_moved_down_its_file_by_a_change_below_is_the_same_change() {
        let at = |start: usize| {
            format!("diff --git a/f b/f\nindex 1..2 100644\n--- a/f\n+++ b/f\n@@ -{start},6 +{start},7 @@ fn f()\n a\n b\n c\n+d\n e\n f\n g\n")
        };
        // A line more above it, from a patch below: the same change.
        assert!(same_diff(&at(336), &at(337)));
        assert!(same_diff(&at(336), &at(336).replace("index 1..2", "index 3..4")));
        // Its lines, its sizes, or the function it is in, other: not.
        assert!(!same_diff(&at(336), &at(336).replace("+d", "+D")));
        assert!(!same_diff(&at(336), &at(336).replace(",7 @@", ",8 @@")));
        assert!(!same_diff(&at(336), &at(336).replace("fn f()", "fn g()")));
        // A draft so moved, its title other, is its patch's new version.
        let old = Patch::new("a".into(), Some("T <t@x>".into()), None, "Bug XXXXXXX - A\n".into(), at(336));
        let planned = plan_drafts(std::slice::from_ref(&old), vec![draft("Bug 2084269 - A", &at(337))], &HashMap::new());
        assert_eq!(planned[0].change, "a");
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
        let planned = plan_drafts(&olds, drafts, &HashMap::new());
        let changes: Vec<&str> = planned.iter().map(|p| p.change.as_str()).collect();
        assert_eq!(changes[..3], ["c", "a", "b"]);
        assert!(!["a", "b", "c", "e"].contains(&changes[3]), "{changes:?}");
        assert_eq!(planned[0].id, olds[2].id);
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
}
