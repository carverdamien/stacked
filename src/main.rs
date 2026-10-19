use std::io::{IsTerminal, Read};
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand, ValueEnum};

use stacked::ask;
use stacked::db::{Db, Error, Note, Patch, Record, Result, Way, err, local_time as time, short_id, stack_change};
use stacked::mbox::{Draft, base_commit, format_mbox, parse_mail, parse_mbox};
use stacked::ops;
use stacked::thread::{Role, append_turn, replace_last_turn, thread_turns};
use stacked::tui::{Busy, Show, note_state, shows};
use stacked::view::{MSG_PATH, find_line, fold, note_lines, title_key};
use stacked::{git, review, tree};

/// A stack editor. Patches, stacks and every edit made to them, comments
/// and what the agent (claude) and Phabricator said: all in one file that is only ever
/// appended to. The worktree is not touched unless `apply` is asked to.
#[derive(Parser)]
#[command(version)]
struct Cli {
    /// The database: else $STACKED_FILE, else the settings' `file`, else
    /// ./stacked.jsonl.
    #[arg(short, long, global = true)]
    file: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

/// Which comments `comments` lists, as the editor's Show… picks them.
#[derive(Clone, Copy, ValueEnum)]
enum ShowArg {
    All,
    /// Not sent, and not answered.
    ToSend,
    /// The agent's answer is their last word.
    ToRead,
    /// ... from an ask that could only answer.
    ToReadOnly,
    /// ... from an ask that could change the code.
    ToReadWrite,
    /// Ready to go to the agent, or with it.
    InProgress,
    /// Their ask failed, or they moved to the stack.
    Failed,
    /// On a line their patch no longer has.
    Gone,
}

impl ShowArg {
    fn show(self) -> Show {
        match self {
            ShowArg::All => Show::All,
            ShowArg::ToSend => Show::ToSend,
            ShowArg::ToRead => Show::ToRead,
            ShowArg::ToReadOnly => Show::ToReadAnswer,
            ShowArg::ToReadWrite => Show::ToReadChange,
            ShowArg::InProgress => Show::InProgress,
            ShowArg::Failed => Show::Failed,
            ShowArg::Gone => Show::Gone,
        }
    }
}

#[derive(Subcommand)]
enum Cmd {
    /// Open the editor: `stacked FILE` is `stacked open FILE`, and
    /// `stacked` alone opens the default database.
    Open { file: Option<PathBuf> },
    /// The editor in a browser: a server on this machine, and the browser
    /// opened on it. The cursor follows the mouse: a click is Enter, a right
    /// click Space. Built with `--features web`.
    /// A tour of the editor, in sections, the most useful first, on a game
    /// of tic-tac-toe played in a file, a move a patch. It writes nothing on
    /// disk but `[demo] seen` in the settings: opened again, it starts on
    /// the list of its sections, to start at any of them. Built with
    /// `--features demo`.
    #[cfg(feature = "demo")]
    Demo,
    #[cfg(feature = "web")]
    Web {
        /// The port of 127.0.0.1 to listen on; else any free one.
        #[arg(long, default_value_t = 0)]
        port: u16,
        /// Only say where it is: do not open the browser.
        #[arg(long)]
        no_open: bool,
    },
    /// The stacks: name, patches, the stack it is on.
    Stacks,
    /// Make an empty stack, on a commit of the repository here.
    New {
        name: String,
        /// The commit its patches will apply to; else HEAD.
        #[arg(long)]
        base: Option<String>,
    },
    /// Put commits on top of a stack, making it if need be: RANGE's
    /// (`main..HEAD`, or `main` for that) from the repository here, else
    /// the mail on stdin (`git format-patch --stdout`). Only if they apply.
    Import {
        name: String,
        range: Option<String>,
        /// Make them the whole stack. A patch with the title of one the
        /// stack held is a new version of it.
        #[arg(long)]
        replace: bool,
        /// The commit mail on stdin applies to, when it does not say
        /// (`format-patch --base` makes it say).
        #[arg(long)]
        base: Option<String>,
    },
    /// Set the commit a stack applies to, in the repository here: only if
    /// its patches apply there.
    Base { name: String, rev: String },
    /// Put one patch on a stack, making it if need be: a mail, or a diff on
    /// stdin and -m. Only if it applies.
    Add {
        name: String,
        /// The message, when stdin is a bare diff.
        #[arg(short, long)]
        message: Option<String>,
        /// Where to put it, counting from 1 at the bottom; else on top.
        #[arg(long)]
        at: Option<usize>,
        /// The commit a new stack is on; else HEAD.
        #[arg(long)]
        base: Option<String>,
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
    /// Edit a stack as `git rebase -i` would; or one patch, in $EDITOR: as
    /// mail, or one of its files as the patch leaves it (--path), the patch
    /// made again from what is saved.
    Edit {
        name: String,
        patch: Option<String>,
        /// The line, from 1, to open the editor at.
        #[arg(long, requires = "patch")]
        line: Option<usize>,
        /// A file the patch changes, to edit as the patch leaves it.
        #[arg(long, requires = "patch")]
        path: Option<String>,
        /// Where the file is edited: `tmp`, a copy of it alone; `pwd`, this
        /// checkout, moved to the patch for the edit, then back; `worktree`,
        /// the stack's own, .stacked/NAME in its repository. Else where the
        /// stack's menus last picked.
        #[arg(long = "in", value_name = "WHERE", requires = "path", value_parser = ["tmp", "pwd", "worktree"])]
        place: Option<String>,
        /// What to start from: an edit not taken before, as the file it was
        /// kept in (tmp), or the commit (pwd, worktree).
        #[arg(long, requires = "path")]
        from: Option<String>,
    },
    /// Move a patch to another position, counting from 1 at the bottom.
    Move { name: String, patch: String, to: usize },
    /// Take patches out of a stack. They stay in the database.
    Drop {
        name: String,
        #[arg(required = true)]
        patches: Vec<String>,
    },
    /// Every edit and every move of a head, oldest first: a stack's, or
    /// all. `runs` has the asks, applies and pulls.
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
    /// Comment on a stack, on one of its patches as a whole, or on a line
    /// of one. Its short id is printed, for `send` and `reply`. Where a
    /// comment is, is what the agent may change when it is asked: the stack,
    /// or that patch.
    Comment {
        name: String,
        /// The patch: its position from 1, or its id or change; else the
        /// comment is on the stack.
        #[arg(long)]
        patch: Option<String>,
        /// A line of the patch, as FILE:LINE with LINE as the diff has it,
        /// `+`, `-` or a space first: `a.txt:+TWO`.
        #[arg(long, requires = "patch")]
        line: Option<String>,
        /// A record of the stack's history (`stacked log` lists them), as
        /// @12: a question on it, for the agent to answer only.
        #[arg(long, conflicts_with = "patch")]
        about: Option<String>,
        #[arg(required = true)]
        text: Vec<String>,
    },
    /// A stack's comments, its own then each patch's, or one patch's: each
    /// its short id, where it is, where it stands, and its thread.
    Comments {
        name: String,
        #[arg(long)]
        patch: Option<String>,
        #[arg(long, value_enum)]
        show: Option<ShowArg>,
    },
    /// Reply to a comment, by its short id.
    Reply {
        name: String,
        id: String,
        /// Write what you said last again, rather than add to it: only when
        /// the last word is yours, and not while the agent works on it. A
        /// comment waiting its turn leaves the agent's queue, to be sent
        /// again.
        #[arg(long)]
        edit: bool,
        #[arg(required = true)]
        text: Vec<String>,
    },
    /// Ask the agent about comments, by their short ids, or --all that can
    /// be sent. It may change the code where they are, unless told to
    /// answer only, or to build and test. It runs on its own: its answer
    /// lands as a turn of each thread.
    Send {
        name: String,
        ids: Vec<String>,
        /// Every comment that can be sent: the stack's and each patch's, or
        /// `--patch`'s.
        #[arg(long, conflicts_with = "ids")]
        all: bool,
        #[arg(long, requires = "all")]
        patch: Option<String>,
        /// The agent only answers: nothing changes.
        #[arg(long, conflicts_with = "build_and_test")]
        answer_only: bool,
        /// The agent builds and tests, in a workspace of its choosing: on the
        /// stack's comments and patches' as a whole.
        #[arg(long)]
        build_and_test: bool,
        /// A patch's comments, sent to the whole stack: the agent may change
        /// any patch, add patches anywhere, reorder them, as for the stack's
        /// own comments, not only the patch they are on.
        #[arg(long, conflicts_with_all = ["answer_only", "build_and_test"])]
        whole_stack: bool,
        /// Wait for them to be answered, and print the threads.
        #[arg(long)]
        wait: bool,
    },
    /// The runs, oldest first: asks, applies and pulls, and what came of
    /// them.
    Runs { name: Option<String> },
    /// What a run's agent wrote.
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
    /// The lines of the database passed over, if any; and whether each
    /// stack applies.
    Check,
    /// Write a new database: this one, the start of its history left out.
    /// What there was before --from is the new one's start, and everything
    /// from it on follows; without --from, only what there is now. This one
    /// is only read.
    Trim {
        new: PathBuf,
        /// The first rev of the history kept, as @12.
        #[arg(long)]
        from: Option<String>,
    },
    /// The settings, as they are: every one, in the form of the file they
    /// are read from (~/.stacked.toml, or $STACKED_CONFIG), to start one
    /// from: `stacked config > ~/.stacked.toml`.
    Config,
    /// An ask's prompt (for the ask's wrapper).
    #[command(hide = true)]
    Prompt { id: String },
    /// Append stdin to a run's events (for the ask's wrapper).
    #[command(hide = true)]
    Record { id: String },
    /// File what a finished ask did (for the ask's wrapper).
    #[command(hide = true)]
    Land { id: String },
    /// Whether the patches above still apply on what an ask changing a
    /// patch has in its worktree (for the agent).
    #[command(hide = true)]
    Fits { id: String },
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

fn parse_rev(s: &str) -> Result<usize> {
    s.trim_start_matches('@')
        .parse()
        .map_err(|_| Error(format!("{s:?} is not a rev (as @12)")))
}

fn here() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// `rev` (else HEAD) of the repository here, and where that repository is:
/// what a new stack is checked against.
fn commit_here(rev: Option<&str>) -> Result<(String, String)> {
    let repo = git::toplevel(&here())
        .map_err(|_| Error("not in a git repository: a stack's patches are checked to apply to a commit of one".into()))?;
    Ok((git::rev_parse(&here(), rev.unwrap_or("HEAD"))?, repo))
}


/// The patch of stack `name` that `spec` names.
fn patch_of(db: &Db, name: &str, spec: &str) -> Result<Patch> {
    let s = db.stack(name)?;
    Ok(db.patch(&s.patches[db.resolve(s, spec)?]).clone())
}

/// `FILE:LINE` of patch `p`: its file, the side (`+`, `-` or a space), and
/// the line's text. FILE is the first of the patch's files that a colon
/// ends.
fn line_of(p: &Patch, spec: &str) -> Result<String> {
    let lines = note_lines(p);
    for (i, _) in spec.match_indices(':') {
        let (file, rest) = (&spec[..i], &spec[i + 1..]);
        if !lines.iter().any(|l| l.file == file) {
            continue;
        }
        let (side, text) = match rest.chars().next() {
            Some(c @ ('+' | '-' | ' ')) => (c, &rest[1..]),
            _ => (' ', rest),
        };
        return find_line(&lines, file, side, text)
            .ok_or_else(|| Error(format!("{file} has no line {}{text:?} in this patch", side.to_string().trim())));
    }
    err(format!("{spec:?} names none of this patch's files: FILE:LINE, as a.txt:+TWO"))
}

/// Where comment `n` of stack `name` is, said.
fn where_on(db: &Db, name: &str, n: &Note) -> String {
    if n.about.is_some() {
        return format!("the history, {}", n.preview);
    }
    if n.on_stack() {
        return "the stack".into();
    }
    let at = db
        .stack(name)
        .ok()
        .and_then(|s| s.patches.iter().position(|id| db.patch(id).change == n.change))
        .map_or("a patch no longer in the stack".to_string(), |i| format!("patch {}", i + 1));
    let p = db.versions(&n.change).last().map(|p| (*p).clone());
    if n.file == MSG_PATH && p.is_some_and(|p| title_key(&p) == n.line) {
        format!("{at}, as a whole")
    } else {
        format!("{at} · {}: {}", n.file, n.preview)
    }
}

fn print_note(db: &Db, name: &str, n: &Note, busy: &Busy) {
    let state = note_state(db, name, n, busy);
    let state = if state.is_empty() { String::new() } else { format!("\t{state}") };
    println!("{}\t{}{state}", short_id(&n.change, &n.id), where_on(db, name, n));
    for t in thread_turns(&n.text) {
        let who = match t.role {
            Role::You => "you",
            Role::Agent => "agent",
            Role::Reviewer => t.tag.as_deref().unwrap_or("reviewer"),
        };
        for (k, line) in t.body.lines().enumerate() {
            if k == 0 {
                println!("  {who}: {line}");
            } else {
                println!("    {line}");
            }
        }
    }
}

/// The comments of stack `name`, its own then each patch's, or patch
/// `spec`'s; each with whether its line is gone from its patch.
fn comments_of(db: &Db, name: &str, spec: Option<&str>) -> Result<Vec<(Note, bool)>> {
    let s = db.stack(name)?;
    let mut subjects: Vec<(String, Option<Patch>)> = Vec::new();
    match spec {
        Some(spec) => {
            let p = patch_of(db, name, spec)?;
            subjects.push((p.change.clone(), Some(p)));
        }
        None => {
            subjects.push((stack_change(name), None));
            for id in &s.patches {
                let p = db.patch(id).clone();
                subjects.push((p.change.clone(), Some(p)));
            }
        }
    }
    let mut out = Vec::new();
    for (change, p) in subjects {
        let keys: std::collections::HashSet<String> =
            p.as_ref().map(|p| note_lines(p).into_iter().map(|l| l.key).collect()).unwrap_or_default();
        for n in db.notes(&change) {
            let gone = p.is_some() && !keys.contains(&n.line);
            out.push((n.clone(), gone));
        }
    }
    Ok(out)
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
        #[cfg(feature = "demo")]
        Cmd::Demo => stacked::demo::run()?,
        #[cfg(feature = "web")]
        Cmd::Web { port, no_open } => {
            let bound = stacked::web::bind(port)?;
            let url = bound.url();
            eprintln!("stacked: the editor is at {url} · quit it there, or ctrl-c here");
            if !no_open {
                stacked::web::open_browser(&url);
            }
            bound.serve(path)?;
        }
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
        Cmd::New { name, base } => {
            let (base, repo) = commit_here(base.as_deref())?;
            Db::edit(path, |db| db.new_stack(&name, &base, &repo))?;
        }
        Cmd::Import { name, range, replace, base } => {
            let (mail, base) = match (&range, base) {
                (Some(r), _) => {
                    let (mail, base) = git::format_patch(&here(), r)?;
                    (mail, Some(base))
                }
                (None, given) => {
                    let mail = stdin()?;
                    let base = match given {
                        Some(rev) => Some(commit_here(Some(&rev))?.0),
                        None => base_commit(&mail),
                    };
                    (mail, base)
                }
            };
            let drafts = parse_mbox(&mail)?;
            if drafts.is_empty() {
                return err("no commits to import");
            }
            if base.is_none() && Db::load(path)?.head(&name).is_none() {
                return err(
                    "the mail does not say what it applies to: give --base REV, or make it with `git format-patch --base=REV`",
                );
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
        Cmd::Add { name, message, at, base } => {
            let text = stdin()?;
            let draft = match message {
                Some(m) => Draft {
                    message: m.trim().to_string(),
                    diff: text,
                    ..Default::default()
                },
                None => parse_mail(&text)?,
            };
            let new = match Db::load(path)?.head(&name) {
                Some(_) => None,
                None => Some(commit_here(base.as_deref())?),
            };
            Db::edit(path, |db| {
                if let Some((base, repo)) = &new
                    && db.head(&name).is_none()
                {
                    db.new_stack(&name, base, repo)?;
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
        Cmd::Edit { name, patch: None, .. } => eprintln!("stacked: {}", ops::edit_stack(path, &name)?),
        Cmd::Edit { name, patch: Some(p), line, path: None, .. } => {
            let db = Db::load(path)?;
            let at = db.resolve(db.stack(&name)?, &p)?;
            eprintln!("stacked: {}", ops::edit_patch(path, &name, at, line)?);
        }
        Cmd::Edit { name, patch: Some(p), line, path: Some(f), place, from } => {
            let db = Db::load(path)?;
            let at = db.resolve(db.stack(&name)?, &p)?;
            let place = ops::Place::named(place.as_deref().or(db.choice(&name, "edit")).unwrap_or("tmp"));
            let again = match (&from, &place) {
                (None, _) => None,
                (Some(from), ops::Place::Copy) => {
                    Some(ops::Kept::Text(std::fs::read_to_string(from).map_err(|e| Error(format!("{from}: {e}")))?))
                }
                (Some(commit), place) => Some(ops::Kept::Commit {
                    place: place.clone(),
                    commit: commit.clone(),
                }),
            };
            match ops::edit_code(path, &name, at, &f, line.unwrap_or(1), &place, again)? {
                ops::Edited::Done(said) => eprintln!("stacked: {said}"),
                ops::Edited::Unchanged => eprintln!("stacked: saved as it was: nothing changed"),
                ops::Edited::Refused { why, kept: ops::Kept::Text(text) } => {
                    let short = f.rsplit('/').next().unwrap_or(&f);
                    let keep = std::env::temp_dir().join(format!("stacked-edit-{}-{short}", std::process::id()));
                    std::fs::write(&keep, text).map_err(|e| Error(format!("{}: {e}", keep.display())))?;
                    return err(format!(
                        "{why}; nothing changed. Your edit is kept in {k}: `stacked edit {name} {p} --path {f} --from {k}` takes it up again",
                        k = keep.display()
                    ));
                }
                ops::Edited::Refused { why, kept: ops::Kept::Commit { place, commit } } => {
                    return err(format!(
                        "{why}; nothing changed. Your edit is kept in commit {short}: `stacked edit {name} {p} --path {f} --in {w} --from {commit}` takes it up again",
                        short = &commit[..12.min(commit.len())],
                        w = place.name(),
                    ));
                }
            }
        }
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
        Cmd::Comment { name, patch, line, about: Some(rev), text } => {
            let _ = (patch, line);
            let rev = parse_rev(&rev)?;
            let db = Db::load(path)?;
            db.stack(&name)?;
            let Some(e) = stacked::history::entries(&db, &name).into_iter().find(|e| e.rev == rev) else {
                return err(format!(
                    "@{rev} is not in the history of {name}, the records that changed it: `stacked log {name}` lists its edits"
                ));
            };
            let note = Note {
                change: stack_change(&name),
                id: format!("h{}", stacked::db::unique()),
                text: text.join(" "),
                preview: e.text.chars().take(80).collect(),
                about: Some(rev),
                ..Default::default()
            };
            let short = short_id(&note.change, &note.id);
            Db::edit(path, |db| db.set_note(note))?;
            println!("{short}");
        }
        Cmd::Comment { name, patch, line, about: None, text } => {
            let text = text.join(" ");
            let db = Db::load(path)?;
            db.stack(&name)?;
            let note = match (&patch, &line) {
                (None, _) => Note {
                    change: stack_change(&name),
                    id: format!("s{}", stacked::db::unique()),
                    text,
                    ..Default::default()
                },
                (Some(spec), None) => {
                    let p = patch_of(&db, &name, spec)?;
                    Note {
                        change: p.change.clone(),
                        id: format!("p{}", stacked::db::unique()),
                        line: title_key(&p),
                        text,
                        file: MSG_PATH.into(),
                        preview: p.title().chars().take(80).collect(),
                        ..Default::default()
                    }
                }
                (Some(spec), Some(l)) => {
                    let p = patch_of(&db, &name, spec)?;
                    let key = line_of(&p, l)?;
                    // A comment on the line already: this is a turn of it.
                    match db.note(&p.change, &key) {
                        Some(n) => Note {
                            text: append_turn(&n.text, Role::You, &text, None),
                            failed: None,
                            ..n.clone()
                        },
                        None => {
                            let at = note_lines(&p).into_iter().find(|nl| nl.key == key).unwrap();
                            Note {
                                change: p.change.clone(),
                                id: key.clone(),
                                line: key,
                                text,
                                file: at.file,
                                preview: fold(&at.text).chars().take(80).collect(),
                                ..Default::default()
                            }
                        }
                    }
                }
            };
            let short = short_id(&note.change, &note.id);
            Db::edit(path, |db| db.set_note(note))?;
            println!("{short}");
        }
        Cmd::Comments { name, patch, show } => {
            let db = Db::load(path)?;
            let busy = ask::with_agent(&db);
            let show = show.map_or(Show::All, ShowArg::show);
            for (n, gone) in comments_of(&db, &name, patch.as_deref())? {
                if shows(show, &n, &busy, gone) {
                    print_note(&db, &name, &n, &busy);
                }
            }
        }
        Cmd::Reply { name, id, edit, text } => {
            let text = text.join(" ");
            let off = Db::edit(path, |db| {
                let n = db.find_note(&name, &id)?.clone();
                if !edit {
                    db.set_note(Note {
                        text: append_turn(&n.text, Role::You, &text, None),
                        failed: None,
                        ..n
                    })?;
                    return Ok(None);
                }
                if ask::with_agent(db).contains_key(&(n.change.clone(), n.id.clone())) {
                    return err("the agent is working on it: wait for it to land");
                }
                let Some(again) = replace_last_turn(&n.text, &text) else {
                    return err("the last word is not yours: reply instead");
                };
                if again == n.text {
                    return Ok(None);
                }
                let off = n.queued.is_some().then(|| short_id(&n.change, &n.id));
                db.set_note(Note {
                    text: again,
                    failed: None,
                    queued: None,
                    ..n
                })?;
                Ok(off)
            })?;
            if let Some(short) = off {
                println!("{short}\tedited: off the agent's queue, until it is sent again");
            }
        }
        Cmd::Send { name, ids, all, patch, answer_only, build_and_test, whole_stack, wait } => {
            let way = match (answer_only, build_and_test) {
                (_, true) => Way::Test,
                (true, _) => Way::Answer,
                _ => Way::Change,
            };
            let db = Db::load(path)?;
            let since = db.records.len();
            let busy = ask::with_agent(&db);
            let picked: Vec<Note> = if all {
                comments_of(&db, &name, patch.as_deref())?
                    .into_iter()
                    .map(|(n, _)| n)
                    .filter(|n| ask::sendable(n) && !busy.contains_key(&(n.change.clone(), n.id.clone())))
                    .filter(|n| way != Way::Test || ask::can_test(&db, n))
                    .collect()
            } else if ids.is_empty() {
                return err("name the comments to send by their short ids (`stacked comments NAME` lists them), or --all");
            } else {
                ids.iter().map(|id| db.find_note(&name, id).cloned()).collect::<Result<_>>()?
            };
            if picked.is_empty() {
                return err("no comment to send");
            }
            // Comments go together only from one place: the stack, or one
            // patch; to the whole stack, all together.
            let mut batches: Vec<(ask::Scope, Vec<(String, String)>)> = Vec::new();
            for n in &picked {
                let scope = if whole_stack { ask::Scope::Stack } else { ask::scope_of(n) };
                let at = (n.change.clone(), n.id.clone());
                match batches.iter_mut().find(|(s, _)| *s == scope) {
                    Some((_, b)) => b.push(at),
                    None => batches.push((scope, vec![at])),
                }
            }
            let said = if whole_stack { stacked::db::WHOLE } else { way.said() };
            for (_, batch) in &batches {
                let waits = ask::send(path, &name, batch, way, whole_stack)?;
                for (c, i) in batch {
                    match &waits {
                        None => println!("{}\tsent ({said})", short_id(c, i)),
                        Some(w) => println!("{}\tready ({said}), it goes {w}", short_id(c, i)),
                    }
                }
            }
            if wait {
                let sent: Vec<(String, String)> = picked.iter().map(|n| (n.change.clone(), n.id.clone())).collect();
                loop {
                    std::thread::sleep(std::time::Duration::from_millis(200));
                    let db = Db::load(path)?;
                    ask::land_orphans(path, &db);
                    let db = Db::load(path)?;
                    let busy = ask::with_agent(&db);
                    let going = sent
                        .iter()
                        .any(|k| busy.contains_key(k) || db.note(&k.0, &k.1).is_some_and(|n| n.queued.is_some()));
                    if going {
                        continue;
                    }
                    // What came of it: every comment written since, the
                    // sent ones where they are now (a split moves them with
                    // their line), and those an answer opened.
                    let mut failed = 0;
                    for n in db.stack_notes(&name) {
                        if db.note_history(&n.change, &n.id).last().is_some_and(|(rev, _)| *rev > since) {
                            failed += n.failed.is_some() as usize;
                            print_note(&db, &name, n, &busy);
                        }
                    }
                    if failed > 0 {
                        return err(format!("{failed} of them failed"));
                    }
                    break;
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
        Cmd::Trim { new, from } => {
            let from = from.as_deref().map(parse_rev).transpose()?;
            println!("{}", stacked::trim::trim(path, &new, from)?.said(&new));
        }
        Cmd::Config => {
            let p = stacked::config::path();
            let state = if p.exists() { "read from" } else { "none in" };
            println!("# {state} {}", p.display());
            for w in stacked::config::warnings() {
                println!("# wrong: {w}");
            }
            print!("{}", stacked::config::get().render());
        }
        Cmd::Check => {
            let db = Db::load(path)?;
            for w in &db.warnings {
                println!("{w}");
            }
            if db.warnings.is_empty() {
                println!("{} records, none passed over", db.records.len());
            }
            for s in db.stacks() {
                match db.verify(s) {
                    Ok(()) => println!("{}: applies", s.name),
                    Err(e) => println!("{}: {}", s.name, e.0.trim_end_matches("; nothing changed")),
                }
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
        Cmd::Fits { id } => println!("{}", ask::fits(path, &id)?),
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
        .or_else(|| stacked::config::get().file.clone())
        .unwrap_or_else(|| PathBuf::from("stacked.jsonl"));
    // What is wrong with the settings is said: by the editor in its bar,
    // here for the commands, but those asks run by themselves.
    let quiet = matches!(
        cli.cmd,
        Some(Cmd::Open { .. } | Cmd::Prompt { .. } | Cmd::Record { .. } | Cmd::Land { .. } | Cmd::Fits { .. } | Cmd::Config) | None
    );
    if !quiet {
        for w in stacked::config::warnings() {
            eprintln!("stacked: {w}");
        }
    }
    let res = match cli.cmd {
        Some(Cmd::Open { .. }) | None => stacked::tui::run(&path),
        Some(cmd) => run(&path, cmd),
    };
    if let Err(e) = res {
        die(&e.0);
    }
}
