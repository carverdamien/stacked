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
pub fn match_drafts(
    db: &mut Db,
    olds: &[Patch],
    drafts: Vec<Draft>,
    by_commit: &HashMap<String, String>,
) -> Result<Vec<String>> {
    let mut left: Vec<&Patch> = olds.iter().collect();
    let mut out = Vec::new();
    for d in drafts {
        let from_commit = d
            .commit
            .as_ref()
            .and_then(|c| by_commit.get(c))
            .and_then(|id| left.iter().position(|o| &o.id == id));
        let at = from_commit.or_else(|| left.iter().position(|o| o.title() == d.title()));
        let p = match at {
            Some(i) => {
                let old = left.remove(i);
                let mut d = d;
                if d.author.is_none() {
                    d.author = old.author.clone();
                }
                revise(old, d)
            }
            None => new_patch(d),
        };
        out.push(db.add_patch(p)?);
    }
    Ok(out)
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

/// Opens `text` in $VISUAL or $EDITOR and gives back what was saved.
pub fn edit_text(text: &str, suffix: &str) -> Result<String> {
    let dir = std::env::temp_dir().join(format!("stacked-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| Error(e.to_string()))?;
    let path = dir.join(format!("edit{suffix}"));
    std::fs::write(&path, text).map_err(|e| Error(e.to_string()))?;
    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "vi".into());
    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("{editor} \"$@\""))
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
