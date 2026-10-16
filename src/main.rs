use std::io::{IsTerminal, Read};
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};

use stacked::db::{Db, Error, Result, err};
use stacked::mbox::{format_mbox, parse_mail, parse_mbox};
use stacked::ops;
use stacked::todo::{self, Action};

/// A stack editor. Patches and stacks live in one file that is only ever
/// appended to, so every edit to a stack can be looked back on and undone.
/// The worktree is never touched: patches come in as `git format-patch`
/// mail and go out as mail for `git am`.
#[derive(Parser)]
#[command(version)]
struct Cli {
    /// The database: else $STACKED_FILE, else ./stacked.jsonl.
    #[arg(short, long, global = true)]
    file: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Open the editor: `stacked FILE` is `stacked open FILE`, and
    /// `stacked` alone opens the default database.
    Open { file: Option<PathBuf> },
    /// The stacks: name, patches, last edit.
    Stacks,
    /// Make an empty stack.
    New { name: String },
    /// Put the mail on stdin (`git format-patch --stdout`) on top of a
    /// stack, making it if need be.
    Import {
        name: String,
        /// Make the mail the whole stack. A patch with the title of one the
        /// stack held is a new version of it.
        #[arg(long)]
        replace: bool,
    },
    /// Put one patch on a stack: a mail, or a diff on stdin and -m.
    Add {
        name: String,
        /// The message, when stdin is a bare diff.
        #[arg(short, long)]
        message: Option<String>,
        /// Where to put it, counting from 1 at the bottom; else on top.
        #[arg(long)]
        at: Option<usize>,
    },
    /// A stack's patches, bottom first; or one patch as mail.
    Show { name: String, patch: Option<String> },
    /// A stack's patches, or some of them, as mail for `git am`.
    Export { name: String, patches: Vec<String> },
    /// Edit a stack as `git rebase -i` would; or one patch, in $EDITOR.
    Edit { name: String, patch: Option<String> },
    /// Move a patch to another position, counting from 1 at the bottom.
    Move { name: String, patch: String, to: usize },
    /// Take patches out of a stack. They stay in the database.
    Drop {
        name: String,
        #[arg(required = true)]
        patches: Vec<String>,
    },
    /// The edits made to a stack, or to every stack, oldest first.
    Log { name: Option<String> },
    /// Put a stack back as it was before its last edit.
    Undo { name: String },
    /// Put a stack back as it was at a rev from `log`.
    Restore { name: String, rev: String },
    /// Delete a stack. `undo` brings it back.
    Delete { name: String },
    /// Every version of a patch, oldest first.
    Versions { name: String, patch: String },
    /// What changed between two versions of a patch: by default, the one in
    /// the stack and the one before it.
    Interdiff {
        name: String,
        patch: String,
        /// The older version's id.
        #[arg(long)]
        from: Option<String>,
    },
}

fn die(msg: &str) -> ! {
    eprintln!("stacked: {msg}");
    std::process::exit(1)
}

fn stdin() -> Result<String> {
    if std::io::stdin().is_terminal() {
        return err("expected mail or a diff on stdin");
    }
    let mut s = String::new();
    std::io::stdin()
        .read_to_string(&mut s)
        .map_err(|e| Error(format!("stdin: {e}")))?;
    Ok(s)
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

fn time(t: i64) -> String {
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let t = t as libc::time_t;
    unsafe { libc::localtime_r(&t, &mut tm) };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min
    )
}

fn parse_rev(s: &str) -> Result<usize> {
    s.trim_start_matches('@')
        .parse()
        .map_err(|_| Error(format!("{s:?} is not a rev (as @12)")))
}

/// Edits `name` through a todo list: the editor runs with no lock held, so
/// the stack is checked to be as it was before anything is written.
fn edit_stack(path: &Path, name: &str) -> Result<()> {
    let db = Db::load(path)?;
    let s = db.stack(name)?;
    let (rev, _) = db.head(name).unwrap();
    let lines = match todo::parse(&edit_text(&todo::render(&db, s), ".todo")?)? {
        Some(l) => todo::resolve(&db, l)?,
        None => {
            eprintln!("stacked: the list was emptied; nothing changed");
            return Ok(());
        }
    };
    let mut drafts = Vec::new();
    for (action, id) in &lines {
        let p = db.patch(id);
        let text = match action {
            Action::Pick | Action::Drop => None,
            Action::Reword => Some(edit_text(&p.message, ".txt")?),
            Action::Edit => Some(edit_text(&ops::render(p), ".patch")?),
        };
        drafts.push(match (action, text) {
            (Action::Reword, Some(m)) => {
                let message = strip_comments(&m);
                if message.is_empty() {
                    return err(format!("the message of {} was emptied; nothing changed", &id[..8]));
                }
                let mut d = stacked::mbox::Draft::from(p);
                d.message = message;
                Some(d)
            }
            (Action::Edit, Some(t)) => Some(parse_mail(&t)?),
            _ => None,
        });
    }
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
                    let new = db.add_patch(ops::revise(&old, d))?;
                    if new != old.id {
                        verbs.push(format!("{} {}", if *action == Action::Edit { "edit" } else { "reword" }, &id[..8]));
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
        let before = db.head(name).unwrap().0;
        if db.set_stack(name, patches, &op)? == before {
            eprintln!("stacked: nothing changed");
        }
        Ok(())
    })
}

fn strip_comments(text: &str) -> String {
    text.lines()
        .filter(|l| !l.starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

fn edit_patch(path: &Path, name: &str, spec: &str) -> Result<()> {
    let db = Db::load(path)?;
    let s = db.stack(name)?;
    let at = db.resolve(s, spec)?;
    let id = s.patches[at].clone();
    let draft = parse_mail(&edit_text(&ops::render(db.patch(&id)), ".patch")?)?;
    Db::edit(path, |db| {
        let s = db.stack(name)?;
        if s.patches.get(at) != Some(&id) {
            return err(format!("{name} changed while it was being edited; nothing changed"));
        }
        ops::replace(db, name, at, draft, "edit").map(|_| ())
    })
}

fn run(path: &Path, cmd: Cmd) -> Result<()> {
    match cmd {
        Cmd::Open { .. } => unreachable!(),
        Cmd::Stacks => {
            let db = Db::load(path)?;
            for s in db.stacks() {
                let (rev, _) = db.head(&s.name).unwrap();
                println!(
                    "{}\t{} patches\t@{rev} {} {}",
                    s.name,
                    s.patches.len(),
                    time(s.time),
                    s.op
                );
            }
        }
        Cmd::New { name } => {
            Db::edit(path, |db| db.new_stack(&name))?;
        }
        Cmd::Import { name, replace } => {
            let drafts = parse_mbox(&stdin()?)?;
            if drafts.is_empty() {
                return err("no mail on stdin");
            }
            Db::edit(path, |db| ops::import(db, &name, drafts, replace))?;
        }
        Cmd::Add { name, message, at } => {
            let text = stdin()?;
            let draft = match message {
                Some(m) => stacked::mbox::Draft {
                    message: m.trim().to_string(),
                    diff: text,
                    ..Default::default()
                },
                None => parse_mail(&text)?,
            };
            Db::edit(path, |db| {
                if db.stack(&name).is_err() {
                    db.new_stack(&name)?;
                }
                ops::add(db, &name, draft, at.map(|n| n.saturating_sub(1)))
            })?;
        }
        Cmd::Show { name, patch: None } => {
            let db = Db::load(path)?;
            let s = db.stack(&name)?;
            for (i, id) in s.patches.iter().enumerate() {
                let p = db.patch(id);
                println!("{}\t{}\t{}\t{}", i + 1, &p.change[..8], &id[..8], p.title());
            }
        }
        Cmd::Show { name, patch: Some(spec) } => {
            let db = Db::load(path)?;
            let s = db.stack(&name)?;
            print!("{}", ops::render(db.patch(&s.patches[db.resolve(s, &spec)?])));
        }
        Cmd::Export { name, patches } => {
            let db = Db::load(path)?;
            let s = db.stack(&name)?;
            let mut at: Vec<usize> = (0..s.patches.len()).collect();
            if !patches.is_empty() {
                at = patches.iter().map(|p| db.resolve(s, p)).collect::<Result<_>>()?;
                at.sort();
            }
            let ps: Vec<_> = at.iter().map(|&i| db.patch(&s.patches[i])).collect();
            print!("{}", format_mbox(&ps));
        }
        Cmd::Edit { name, patch: None } => edit_stack(path, &name)?,
        Cmd::Edit { name, patch: Some(p) } => edit_patch(path, &name, &p)?,
        Cmd::Move { name, patch, to } => Db::edit(path, |db| {
            let from = db.resolve(db.stack(&name)?, &patch)?;
            ops::move_patch(db, &name, from, to.saturating_sub(1)).map(|_| ())
        })?,
        Cmd::Drop { name, patches } => Db::edit(path, |db| {
            let s = db.stack(&name)?;
            let at = patches.iter().map(|p| db.resolve(s, p)).collect::<Result<Vec<_>>>()?;
            ops::drop_patches(db, &name, &at).map(|_| ())
        })?,
        Cmd::Log { name } => {
            let db = Db::load(path)?;
            if let Some(n) = &name
                && db.head(n).is_none()
            {
                return err(format!("no stack {n}"));
            }
            for (rev, s) in db.log(name.as_deref()) {
                println!(
                    "@{rev}\t{}\t{}\t{} patches\t{}",
                    time(s.time),
                    s.name,
                    s.patches.len(),
                    s.op
                );
            }
        }
        Cmd::Undo { name } => {
            Db::edit(path, |db| db.undo(&name))?;
        }
        Cmd::Restore { name, rev } => {
            let rev = parse_rev(&rev)?;
            Db::edit(path, |db| db.restore(&name, rev))?;
        }
        Cmd::Delete { name } => {
            Db::edit(path, |db| db.delete_stack(&name))?;
        }
        Cmd::Versions { name, patch } => {
            let db = Db::load(path)?;
            let s = db.stack(&name)?;
            let p = db.patch(&s.patches[db.resolve(s, &patch)?]);
            for v in db.versions(&p.change) {
                let mark = if v.id == p.id { "*" } else { " " };
                println!("{mark} {}\t{}", &v.id[..8], v.title());
            }
        }
        Cmd::Interdiff { name, patch, from } => {
            let db = Db::load(path)?;
            let s = db.stack(&name)?;
            let new = db.patch(&s.patches[db.resolve(s, &patch)?]);
            let old = match from {
                Some(f) => db.find_patch(&f)?,
                None => match ops::previous(&db, new) {
                    Some(o) => o,
                    None => return err(format!("{} has no earlier version", &new.id[..8])),
                },
            };
            print!("{}", ops::interdiff(old, new));
        }
    }
    Ok(())
}

fn main() {
    // A first argument that is not a command is the file to open.
    let mut argv: Vec<String> = std::env::args().collect();
    let commands: Vec<String> = <Cli as clap::CommandFactory>::command()
        .get_subcommands()
        .map(|c| c.get_name().to_string())
        .chain(["help".to_string()])
        .collect();
    let mut i = 1;
    while i < argv.len() && argv[i].starts_with('-') {
        i += if argv[i] == "-f" || argv[i] == "--file" { 2 } else { 1 };
    }
    if i < argv.len() && !commands.contains(&argv[i]) {
        argv.insert(i, "open".into());
    }
    let cli = Cli::parse_from(argv);
    let mut file = cli.file;
    if let Some(Cmd::Open { file: Some(f) }) = &cli.cmd {
        file = Some(f.clone());
    }
    let path = file
        .or_else(|| std::env::var_os("STACKED_FILE").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("stacked.jsonl"));
    let res = match cli.cmd {
        Some(Cmd::Open { .. }) | None => stacked::tui::run(&path),
        Some(cmd) => run(&path, cmd),
    };
    if let Err(e) = res {
        die(&e.0);
    }
}
