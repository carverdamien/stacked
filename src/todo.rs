//! A stack as a list to edit, as `git rebase -i` gives one: a line per
//! patch, bottom first, to reorder, drop, reword or edit.

use crate::db::{Db, Error, Result, Stack, err};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Action {
    Pick,
    /// Edit the message.
    Reword,
    /// Edit the message and the diff.
    Edit,
    Drop,
}

pub fn render(db: &Db, s: &Stack) -> String {
    let mut out = String::new();
    for id in &s.patches {
        let p = db.patch(id);
        out.push_str(&format!("pick {} {}\n", &id[..8], p.title()));
    }
    out.push_str(&format!(
        "\n# Stack {}, bottom first: each patch applies on top of the ones above it.\n\
         #\n\
         # p, pick <patch>   keep the patch\n\
         # r, reword <patch> keep the patch, but edit its message\n\
         # e, edit <patch>   keep the patch, but edit its message and diff\n\
         # d, drop <patch>   drop the patch\n\
         #\n\
         # Lines can be reordered. Removing a line drops its patch; removing\n\
         # them all changes nothing. A patch dropped earlier, or from another\n\
         # stack, can be picked by its id: `stacked versions` lists them.\n",
        s.name
    ));
    out
}

/// The lines of `text`, each an action and a patch id prefix. `None` when
/// there are none, as when every line was removed.
pub fn parse(text: &str) -> Result<Option<Vec<(Action, String)>>> {
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut words = line.split_whitespace();
        let verb = words.next().unwrap();
        let action = match verb {
            "p" | "pick" => Action::Pick,
            "r" | "reword" => Action::Reword,
            "e" | "edit" => Action::Edit,
            "d" | "drop" => Action::Drop,
            _ => return err(format!("line {}: no action {verb:?}", i + 1)),
        };
        let Some(id) = words.next() else {
            return err(format!("line {}: {verb} what?", i + 1));
        };
        out.push((action, id.to_string()));
    }
    Ok((!out.is_empty()).then_some(out))
}

/// The lines with their prefixes made full patch ids.
pub fn resolve(db: &Db, lines: Vec<(Action, String)>) -> Result<Vec<(Action, String)>> {
    let mut out: Vec<(Action, String)> = Vec::new();
    for (action, prefix) in lines {
        let id = db.find_patch(&prefix)?.id.clone();
        if action != Action::Drop && out.iter().any(|(a, o)| *a != Action::Drop && *o == id) {
            return Err(Error(format!("patch {prefix} is picked twice")));
        }
        out.push((action, id));
    }
    Ok(out)
}
