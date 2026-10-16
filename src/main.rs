use std::io::{IsTerminal, Read};
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand, ValueEnum};

use stacked::ask::{self, Kind};
use stacked::db::{Db, Error, Note, Record, Result, err};
use stacked::mbox::{Draft, base_commit, format_mbox, parse_mail, parse_mbox};
use stacked::ops::{self, edit_text, strip_comments};
use stacked::todo::{self, Action};
use stacked::{git, review, tree, view};

/// A stack editor. Patches, stacks and every edit made to them, comments
/// and what claude and Phabricator said: all in one file that is only ever
/// appended to. The worktree is not touched unless `apply` is asked to.
#[derive(Parser)]
#[command(version)]
struct Cli {
    /// The database: else $STACKED_FILE, else ./stacked.jsonl.
    #[arg(short, long, global = true)]
    file: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Clone, Copy, ValueEnum)]
enum AskKind {
    Edit,
    Rebase,
    Review,
    Test,
}

#[derive(Subcommand)]
enum Cmd {
    /// Open the editor: `stacked FILE` is `stacked open FILE`, and
    /// `stacked` alone opens the default database.
    Open { file: Option<PathBuf> },
    /// The stacks: name, patches, the stack it is on.
    Stacks,
    /// Make an empty stack.
    New { name: String },
    /// Put commits on top of a stack, making it if need be: RANGE's
    /// (`main..HEAD`, or `main` for that) from the repository here, else
    /// the mail on stdin (`git format-patch --stdout`).
    Import {
        name: String,
        range: Option<String>,
        /// Make them the whole stack. A patch with the title of one the
        /// stack held is a new version of it.
        #[arg(long)]
        replace: bool,
    },
    /// Set the commit a stack applies to, in the repository here.
    Base { name: String, rev: String },
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
    Export {
        name: String,
        patches: Vec<String>,
        /// The stack as it was at this rev, rather than now.
        #[arg(long)]
        rev: Option<String>,
    },
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
    /// Every edit, head moved and run, oldest first: a stack's, or all.
    Log { name: Option<String> },
    /// A stack's tree of edits, newest first.
    Tree { name: String },
    /// Go back to the stack this one was edited from.
    Undo { name: String },
    /// Go forward again, to the stack edited from this one last left.
    Redo { name: String },
    /// Make the stack at a rev of a tree (`tree`, `log`) the one to edit.
    Select { rev: String },
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
    /// Comment on a patch, as a whole (its title), or list its comments.
    Comment {
        name: String,
        patch: String,
        text: Option<String>,
    },
    /// Ask claude to edit, rebase, review or test a stack. It runs on its
    /// own; what came of it lands in the database.
    Ask {
        name: String,
        kind: AskKind,
        /// What to do, for an edit; what to look for, for a review or a
        /// test.
        request: Vec<String>,
        /// For a rebase: the commit to go onto.
        #[arg(long)]
        onto: Option<String>,
        /// The patch it is about.
        #[arg(long)]
        patch: Option<String>,
        /// Wait for it to land, and print what came of it.
        #[arg(long)]
        wait: bool,
    },
    /// The runs, oldest first: asks, applies and pulls, and what came of
    /// them.
    Runs { name: Option<String> },
    /// What a run's claude wrote.
    Transcript { id: String },
    /// Pull a stack's reviews from Phabricator.
    Pull { name: String },
    /// Put a stack into the worktree here: its base checked out and its
    /// patches committed, detached or on --branch. Only with nothing
    /// uncommitted.
    Apply {
        name: String,
        #[arg(long)]
        branch: Option<String>,
    },
    /// The lines of the database passed over, if any.
    Check,
    /// An ask's prompt (for the ask's wrapper).
    #[command(hide = true)]
    Prompt { id: String },
    /// Append stdin to a run's events (for the ask's wrapper).
    #[command(hide = true)]
    Record { id: String },
    /// File what a finished ask did (for the ask's wrapper).
    #[command(hide = true)]
    Land { id: String },
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

pub fn time(t: i64) -> String {
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

fn here() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
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
        drafts.push(match action {
            Action::Pick | Action::Drop => None,
            Action::Reword => {
                let message = strip_comments(&edit_text(&p.message, ".txt")?);
                if message.is_empty() {
                    return err(format!("the message of {} was emptied; nothing changed", &id[..8]));
                }
                Some(Draft {
                    message,
                    ..Draft::from(p)
                })
            }
            Action::Edit => Some(parse_mail(&edit_text(&ops::render(p), ".patch")?)?),
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
        if db.set_stack(name, patches, &op)? == rev {
            eprintln!("stacked: nothing changed");
        }
        Ok(())
    })
}

fn edit_patch(path: &Path, name: &str, spec: &str) -> Result<()> {
    let db = Db::load(path)?;
    let s = db.stack(name)?;
    let at = db.resolve(s, spec)?;
    let id = s.patches[at].clone();
    let draft = parse_mail(&edit_text(&ops::render(db.patch(&id)), ".patch")?)?;
    Db::edit(path, |db| {
        if db.stack(name)?.patches.get(at) != Some(&id) {
            return err(format!("{name} changed while it was being edited; nothing changed"));
        }
        ops::replace(db, name, at, draft, "edit").map(|_| ())
    })
}

fn print_run(r: &stacked::db::Run) {
    let state = format!("{:?}", r.state).to_lowercase();
    let req = if r.request.is_empty() {
        String::new()
    } else {
        format!(" \"{}\"", r.request.split_whitespace().collect::<Vec<_>>().join(" "))
    };
    println!("{}\t{}\t{} {}{req}\t{state}\t{}", r.id, time(r.time), r.what, r.stack, r.summary);
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
        Cmd::Import { name, range, replace } => {
            let (mail, base) = match &range {
                Some(r) => {
                    let (mail, base) = git::format_patch(&here(), r)?;
                    (mail, Some(base))
                }
                None => {
                    let mail = stdin()?;
                    let base = base_commit(&mail);
                    (mail, base)
                }
            };
            let drafts = parse_mbox(&mail)?;
            if drafts.is_empty() {
                return err("no commits to import");
            }
            let on = match base {
                Some(b) => Some((b, git::toplevel(&here())?)),
                None => None,
            };
            Db::edit(path, |db| ops::import(db, &name, drafts, replace, on))?;
        }
        Cmd::Base { name, rev } => {
            let base = git::rev_parse(&here(), &rev)?;
            let repo = git::toplevel(&here())?;
            Db::edit(path, |db| {
                let s = db.stack(&name)?.clone();
                let op = format!("base {}", &base[..12]);
                db.set_stack_on(&name, s.patches, Some(base), Some(repo), &op)
            })?;
        }
        Cmd::Add { name, message, at } => {
            let text = stdin()?;
            let draft = match message {
                Some(m) => Draft {
                    message: m.trim().to_string(),
                    diff: text,
                    ..Default::default()
                },
                None => parse_mail(&text)?,
            };
            Db::edit(path, |db| {
                if db.head(&name).is_none() {
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
        Cmd::Export { name, patches, rev } => {
            let db = Db::load(path)?;
            let s = match rev {
                Some(r) => {
                    let s = db.node(parse_rev(&r)?)?;
                    if s.name != name {
                        return err(format!("@{r} is a stack of {}, not {name}", s.name));
                    }
                    s
                }
                None => db.stack(&name)?,
            };
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
                && !db.names().contains(&n.as_str())
            {
                return err(format!("no stack {n}"));
            }
            for (rev, r) in db.log(name.as_deref()) {
                match r {
                    Record::Stack(s) => println!(
                        "@{rev}\t{}\t{}\t{} patches\t{}{}",
                        time(s.time),
                        s.name,
                        s.patches.len(),
                        s.op,
                        s.parent.map_or(String::new(), |p| format!(" (from @{p})"))
                    ),
                    Record::Head(h) => println!(
                        "@{rev}\t{}\t{}\t→ {}\t{}",
                        time(h.time),
                        h.name,
                        h.rev.map_or("deleted".into(), |r| format!("@{r}")),
                        h.op
                    ),
                    _ => {}
                }
            }
        }
        Cmd::Tree { name } => {
            let db = Db::load(path)?;
            if !db.names().contains(&name.as_str()) {
                return err(format!("no stack {name}"));
            }
            for l in tree::graph(&db, &name) {
                let s = db.node(l.rev)?;
                println!("{}@{}\t{}\t{} patches\t{}", l.lanes, l.rev, time(s.time), s.patches.len(), s.op);
            }
        }
        Cmd::Undo { name } => {
            Db::edit(path, |db| db.undo(&name))?;
        }
        Cmd::Redo { name } => {
            Db::edit(path, |db| db.redo(&name))?;
        }
        Cmd::Select { rev } => {
            let rev = parse_rev(&rev)?;
            Db::edit(path, |db| db.select(rev))?;
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
        Cmd::Comment { name, patch, text } => {
            let db = Db::load(path)?;
            let s = db.stack(&name)?;
            let p = db.patch(&s.patches[db.resolve(s, &patch)?]).clone();
            match text {
                None => {
                    for n in db.notes(&p.change) {
                        println!("{} · {}: {}\n{}\n", n.id, n.file, n.preview, n.text);
                    }
                }
                Some(t) => {
                    let key = view::title_key(&p);
                    Db::edit(path, |db| {
                        let text = match db.note(&p.change, &key) {
                            Some(n) => format!("{}\n{t}", n.text),
                            None => t.clone(),
                        };
                        db.set_note(Note {
                            change: p.change.clone(),
                            id: key.clone(),
                            line: key.clone(),
                            text,
                            file: view::MSG_PATH.into(),
                            preview: p.title().into(),
                            ..Default::default()
                        })
                    })?;
                }
            }
        }
        Cmd::Ask { name, kind, request, onto, patch, wait } => {
            let db = Db::load(path)?;
            let focus = match &patch {
                Some(p) => {
                    let s = db.stack(&name)?;
                    Some(s.patches[db.resolve(s, p)?].clone())
                }
                None => None,
            };
            let kind = match kind {
                AskKind::Edit => Kind::Edit,
                AskKind::Rebase => Kind::Rebase,
                AskKind::Review => Kind::Review,
                AskKind::Test => Kind::Test,
            };
            let request = request.join(" ");
            let id = ask::start(
                path,
                &ask::Spec {
                    kind,
                    stack: &name,
                    request: &request,
                    focus: focus.as_deref(),
                    onto: onto.as_deref(),
                },
            )?;
            println!("{id}");
            if wait {
                loop {
                    std::thread::sleep(std::time::Duration::from_millis(200));
                    let db = Db::load(path)?;
                    let r = db.run(&id).unwrap();
                    if r.state != stacked::db::RunState::Started {
                        print_run(r);
                        break;
                    }
                    if !ask::running(r) && r.pid.is_some() {
                        print_run(&ask::land(path, &id, Some("killed"))?);
                        break;
                    }
                }
            }
        }
        Cmd::Runs { name } => {
            let db = Db::load(path)?;
            ask::land_orphans(path, &db);
            let db = Db::load(path)?;
            for r in db.runs() {
                if name.as_ref().is_none_or(|n| *n == r.stack) {
                    print_run(r);
                }
            }
        }
        Cmd::Transcript { id } => {
            let db = Db::load(path)?;
            for l in ask::tail(&db, &id, usize::MAX) {
                println!("{l}");
            }
            if let Some(r) = db.run(&id)
                && !r.output.is_empty()
            {
                println!("\n{}", r.output);
            }
        }
        Cmd::Pull { name } => print_run(&review::pull(path, &name)?),
        Cmd::Apply { name, branch } => print_run(&ops::apply_stack(path, &name, branch.as_deref())?),
        Cmd::Check => {
            let db = Db::load(path)?;
            for w in &db.warnings {
                println!("{w}");
            }
            if db.warnings.is_empty() {
                println!("{} records, none passed over", db.records.len());
            }
        }
        Cmd::Prompt { id } => {
            let db = Db::load(path)?;
            let info = db
                .run(&id)
                .and_then(ask::info)
                .ok_or_else(|| Error(format!("no ask {id}")))?;
            print!("{}", info.prompt);
        }
        Cmd::Record { id } => ask::record(path, &id, std::io::stdin().lock())?,
        Cmd::Land { id } => print_run(&ask::land(path, &id, None)?),
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
