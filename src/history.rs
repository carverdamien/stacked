//! A stack's history, as the editor shows it and the agent is told it:
//! everything that happened to the stack (its edits, the moves of its head,
//! every record of its comments, the runs that landed), and what each was
//! in full.

use std::collections::{HashMap, HashSet};

use crate::db::{Db, Note, Record, RunState, Stack, local_time, stack_change};
use crate::thread::{Role, last_reply_way, last_role};

/// A line of the stack's history, and where it takes you.
#[derive(Debug, Clone)]
pub struct Entry {
    /// The record it is.
    pub rev: usize,
    pub time: i64,
    pub text: String,
    pub to: Goto,
}

#[derive(Debug, Clone)]
pub enum Goto {
    Stack(usize),
    Note { change: String, id: String },
    Run(String),
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "es" }
}

fn short(text: &str, n: usize) -> String {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() <= n {
        text
    } else {
        format!("{}…", text.chars().take(n.saturating_sub(1)).collect::<String>())
    }
}

/// How the agent's reply says it was asked, from its fence's tag.
pub fn way_said(tag: &str) -> &'static str {
    match tag {
        "change" => "change",
        "answer" => "answer only",
        "test" => "build and test",
        _ => "earlier",
    }
}

/// Everything that happened to stack `name`, newest first.
pub fn entries(db: &Db, name: &str) -> Vec<Entry> {
    let mut changes: HashSet<String> = HashSet::new();
    changes.insert(stack_change(name));
    for (_, s) in db.tree(name) {
        for id in &s.patches {
            changes.insert(db.patch(id).change.clone());
        }
    }
    let mut last: HashMap<(String, String), &Note> = HashMap::new();
    let mut out = Vec::new();
    for (i, r) in db.records.iter().enumerate() {
        let rev = i + 1;
        match r {
            Record::Stack(s) if s.name == name => out.push(Entry {
                rev,
                time: s.time,
                text: format!("@{rev} · {} · {} patch{}", s.op, s.patches.len(), plural(s.patches.len())),
                to: Goto::Stack(rev),
            }),
            Record::Head(h) if h.name == name => out.push(Entry {
                rev,
                time: h.time,
                text: format!("@{rev} · {} → {}", h.op, h.rev.map_or("deleted".into(), |r| format!("@{r}"))),
                to: Goto::Stack(h.rev.unwrap_or(rev)),
            }),
            Record::Comment(n) if changes.contains(&n.change) => {
                let key = (n.change.clone(), n.id.clone());
                let what = note_change(last.get(&key).copied(), n);
                let at = match n.about {
                    Some(about) => format!("on the history, @{about}"),
                    None if n.on_stack() => "on the stack".to_string(),
                    None => format!("{}: {}", n.file, short(&n.preview, 40)),
                };
                out.push(Entry {
                    rev,
                    time: n.time,
                    text: format!("@{rev} · {what} · {at}"),
                    to: Goto::Note {
                        change: n.change.clone(),
                        id: n.id.clone(),
                    },
                });
                last.insert(key, n);
            }
            Record::Run(run) if run.stack == name && run.state != RunState::Started => out.push(Entry {
                rev,
                time: run.time,
                text: format!("@{rev} · {} {:?} · {}", run.what, run.state, short(&run.summary, 80)).to_lowercase(),
                to: Goto::Run(run.id.clone()),
            }),
            _ => {}
        }
    }
    out.reverse();
    out
}

/// What the edit that made stack `rev` changed, from the stack it was made
/// from: patches added, dropped, moved, given a new version (and, under it,
/// indented, what changed in each). Its first line says which edit.
pub fn what_changed(db: &Db, rev: usize) -> Vec<String> {
    let Ok(s) = db.node(rev) else { return vec![] };
    let mut out = vec![format!("@{rev} · {} · {}", local_time(s.time), s.op)];
    let Some(p) = s.parent.and_then(|p| db.node(p).ok()) else {
        out.push(format!("the stack as it was made: {} patch{}", s.patches.len(), plural(s.patches.len())));
        return out;
    };
    let at = |st: &Stack, change: &str| st.patches.iter().position(|id| db.patch(id).change == change);
    let mut changed = false;
    if p.base != s.base {
        out.push(format!("a new base: {}", s.base.as_deref().unwrap_or("none")));
        changed = true;
    }
    for (i, id) in s.patches.iter().enumerate() {
        let new = db.patch(id);
        match at(p, &new.change) {
            None => out.push(format!("+ {} added, at {}", new.title(), i + 1)),
            Some(j) => {
                let old = db.patch(&p.patches[j]);
                if old.id != new.id {
                    out.push(format!("~ {} has a new version", new.title()));
                    for l in crate::ops::interdiff(old, new).lines() {
                        out.push(format!("    {l}"));
                    }
                }
                if j != i {
                    out.push(format!("↕ {} moved from {} to {}", new.title(), j + 1, i + 1));
                }
            }
        }
        changed = true;
    }
    for id in &p.patches {
        let old = db.patch(id);
        if at(s, &old.change).is_none() {
            out.push(format!("− {} dropped", old.title()));
            changed = true;
        }
    }
    if !changed {
        out.push("nothing changed".into());
    }
    out
}

/// What a comment's record was, next to the one before it.
pub fn note_change(before: Option<&Note>, n: &Note) -> String {
    let Some(b) = before else {
        return match n.by.as_deref() {
            None => "written".into(),
            Some("agent" | "claude") => "opened by the agent".into(),
            Some(who) => format!("pulled: {who}'s comment"),
        };
    };
    if n.text.is_empty() {
        return "deleted".into();
    }
    if n.change != b.change || n.id != b.id {
        return "moved".into();
    }
    if n.queued.is_some() && b.queued.is_none() {
        return format!("sent to the agent ({})", n.queued.as_ref().unwrap().way.said());
    }
    if n.queued.is_none() && b.queued.is_some() && n.text == b.text {
        return "taken by the agent".into();
    }
    if n.failed.is_some() && b.failed.is_none() {
        return "ask failed".into();
    }
    if n.text != b.text {
        return match last_role(&n.text) {
            Some(Role::Agent) => format!("agent replied ({})", way_said(&last_reply_way(&n.text).unwrap_or_default())),
            Some(Role::Reviewer) => "a reviewer replied".into(),
            _ => "you replied".into(),
        };
    }
    "changed".into()
}

/// Record `rev` of stack `name`'s history said in full, as the agent is
/// told it: what an edit changed, where a head moved, what a comment said
/// then, what came of a run.
pub fn about(db: &Db, name: &str, rev: usize) -> Vec<String> {
    match db.rev(rev) {
        Some(Record::Stack(_)) => what_changed(db, rev),
        Some(Record::Head(h)) => vec![format!(
            "@{rev} · {} · {}: the stack {name} moved to {}",
            local_time(h.time),
            h.op,
            h.rev.map_or("nothing (deleted)".into(), |r| format!("@{r}"))
        )],
        Some(Record::Comment(n)) => {
            let before = db
                .note_history(&n.change, &n.id)
                .into_iter()
                .take_while(|(r, _)| *r < rev)
                .last()
                .map(|(_, b)| b);
            let mut out = vec![format!("@{rev} · {} · a comment {}", local_time(n.time), note_change(before, n))];
            out.extend(n.text.lines().map(|l| format!("    {l}")));
            out
        }
        Some(Record::Run(r)) => {
            let mut out = vec![format!(
                "@{rev} · {} · {} {:?}: {}",
                local_time(r.time),
                r.what,
                r.state,
                r.summary
            )];
            if !r.request.is_empty() {
                out.push(format!("    asked: {}", r.request));
            }
            out.extend(r.output.lines().take(40).map(|l| format!("    {l}")));
            out
        }
        _ => vec![format!("@{rev} is not in the history of {name}")],
    }
}
