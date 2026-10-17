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
/// title of a patch it replaces is a new version of it; else it is new.
/// Nothing is stored.
pub fn plan_drafts(olds: &[Patch], drafts: Vec<Draft>, by_commit: &HashMap<String, String>) -> Vec<Patch> {
    let mut left: Vec<&Patch> = olds.iter().collect();
    let mut out = Vec::new();
    for d in drafts {
        let from_commit = d
            .commit
            .as_ref()
            .and_then(|c| by_commit.get(c))
            .and_then(|id| left.iter().position(|o| &o.id == id));
        let at = from_commit.or_else(|| left.iter().position(|o| o.title() == d.title()));
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
/// they are the whole stack instead, and a draft with the title of a patch
/// it held is a new version of that patch: so importing a stack again after
/// rebasing it keeps each patch's history.
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

/// Makes `d` the new version of the patch at `at`.
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

/// The previous version of `p`, if it has one.
pub fn previous<'a>(db: &'a Db, p: &Patch) -> Option<&'a Patch> {
    let versions = db.versions(&p.change);
    let i = versions.iter().position(|v| v.id == p.id)?;
    i.checked_sub(1).map(|i| versions[i])
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
        id: format!("apply-{}-{}", crate::db::now(), std::process::id()),
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
/// then each message or patch it says to. What it comes to is checked to
/// apply; if it does not, the list comes back with why at its top, and a
/// patch edited before comes back as it was left. The editor runs with no
/// lock held, so the stack is checked to be as it was before anything is
/// written. What came of it.
pub fn edit_stack(path: &std::path::Path, name: &str) -> Result<String> {
    use crate::todo::{self, Action};
    let db = Db::load(path)?;
    db.writable(name)?;
    let s = db.stack(name)?.clone();
    let (rev, _) = db.head(name).unwrap();
    // What was last made of each message or patch edited.
    let mut texts: HashMap<String, String> = HashMap::new();
    let mut list = todo::render(&db, &s);
    /// The list as read, and the message or patch made of each line, if any.
    type Plan = (Vec<(Action, String)>, Vec<Option<Draft>>);
    let (lines, drafts) = loop {
        let edited = edit_text(&list, ".todo")?;
        let Some(lines) = todo::parse(&strip_errors(&edited))? else {
            return Ok("the list was emptied; nothing changed".into());
        };
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
        if db.head(name).map(|(r, _)| r) != Some(rev) {
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
        Ok(if db.set_stack(name, patches, &op)? == rev {
            "nothing changed".to_string()
        } else {
            op
        })
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
/// at `line` if one is given. If it no longer applies, or cannot be read,
/// it comes back with why at its top. What came of it.
pub fn edit_patch(path: &std::path::Path, name: &str, at: usize, line: Option<usize>) -> Result<String> {
    let db = Db::load(path)?;
    db.writable(name)?;
    let s = db.stack(name)?.clone();
    let old = db.patch(&s.patches[at]).clone();
    let mut text = render(&old);
    let mut line = line;
    let draft = loop {
        let edited = strip_errors(&edit_file(&text, "edit.patch", line)?);
        if edited.trim().is_empty() {
            return Ok("the patch was emptied; nothing changed".into());
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
        if db.stack(name)?.patches.get(at) != Some(&old.id) {
            return err(format!("{name} changed while it was being edited; nothing changed"));
        }
        let before = db.head(name).unwrap().0;
        Ok(if replace(db, name, at, draft, "edit")? == before {
            "nothing changed".to_string()
        } else {
            format!("edited {}", old.title())
        })
    })
}

/// What came of editing a patch's code.
#[derive(Debug, Clone, PartialEq)]
pub enum Edited {
    /// The new version is in the stack: what it was, said.
    Done(String),
    /// Saved as it was: nothing changed.
    Unchanged,
    /// The stack does not apply with it (why): the text edited, kept to
    /// edit again.
    Refused { why: String, text: String },
}

/// Edits `file` as the patch at `at` of stack `name` leaves it, in the
/// editor at `line`, from `text` if given (an edit refused before), else
/// the file. The patch's part for that file is made again from what is
/// saved, and the new version goes in the stack once the stack is seen to
/// apply with it.
pub fn edit_code(
    path: &std::path::Path,
    name: &str,
    at: usize,
    file: &str,
    line: usize,
    text: Option<String>,
) -> Result<Edited> {
    let db = Db::load(path)?;
    db.writable(name)?;
    let s = db.stack(name)?.clone();
    let (repo, base) = base_of(&s)?;
    let old = db.patch(&s.patches[at]).clone();
    let files = crate::diff::parse_diff(&old.diff, "");
    let Some(k) = files.iter().position(|f| f.path == file) else {
        return err(format!("{} does not change {file}", old.title()));
    };
    let fd = &files[k];
    let from = fd
        .header
        .iter()
        .find_map(|l| l.strip_prefix("rename from ").or_else(|| l.strip_prefix("copy from ")))
        .map_or(file.to_string(), String::from);
    let patches: Vec<&Patch> = s.patches[..=at].iter().map(|id| db.patch(id)).collect();
    let (before, after) = crate::check::texts(&repo, &base, &patches, &from, file)?;
    let Some(after) = after else {
        return err(format!("{} deletes {file}, or it is not text: edit the patch as mail instead", old.title()));
    };
    let start = text.unwrap_or_else(|| after.clone());
    let short = file.rsplit('/').next().unwrap_or(file);
    let edited = edit_file(&start, short, Some(line))?;
    if edited == after {
        return Ok(Edited::Unchanged);
    }
    let section = file_section(&repo, fd, &from, before.as_deref(), &edited)?;
    let diff = with_section(&old.diff, k, section.as_deref());
    if !diff.lines().any(|l| l.starts_with("diff --git ")) {
        return Ok(Edited::Refused {
            why: format!("{} would change nothing: drop it instead", old.title()),
            text: edited,
        });
    }
    let new = revise(
        &old,
        Draft {
            diff,
            ..Draft::from(&old)
        },
    );
    let mut all: Vec<&Patch> = s.patches.iter().map(|id| db.patch(id)).collect();
    all[at] = &new;
    if let Err(e) = crate::check::stack(&s, &all) {
        return Ok(Edited::Refused {
            why: e.0.trim_end_matches("; nothing changed").to_string(),
            text: edited,
        });
    }
    let draft = Draft::from(&new);
    Db::edit(path, |db| {
        if db.stack(name)?.patches.get(at) != Some(&old.id) {
            return err(format!("{name} changed while it was being edited; nothing changed"));
        }
        replace(db, name, at, draft, "edit")?;
        Ok(Edited::Done(format!("edited {file} in {}", old.title())))
    })
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
    let cfg = crate::config::get();
    let editor = cfg.editor();
    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg(cfg.editing(line))
        .arg("editor")
        .arg(&path)
        .status()
        .map_err(|e| Error(format!("{editor}: {e}")))?;
    let out = std::fs::read_to_string(&path).map_err(|e| Error(e.to_string()));
    let _ = std::fs::remove_dir_all(&dir);
    if !status.success() {
        return err(format!("{editor} failed; nothing changed"));
    }
    out
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
