use std::io::{IsTerminal, Read};
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand, ValueEnum};

use stacked::ask;
use stacked::db::{Db, Error, Note, Patch, Record, Result, Way, err, local_time as time, short_id, stack_change};
use stacked::mbox::{Draft, base_commit, format_mbox, parse_mail, parse_mbox};
use stacked::ops;
use stacked::thread::{Role, all_turns, append_turn, edit_turn, replace_last_turn, rewind_from};
use stacked::tui::{Busy, Show, note_state, shows, where_on};
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
    /// The agent's reply to them waits for your review.
    ToReview,
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
    /// Closed, each as it was.
    #[value(alias = "deleted")]
    Closed,
}

impl ShowArg {
    fn show(self) -> Show {
        match self {
            ShowArg::All => Show::All,
            ShowArg::ToSend => Show::ToSend,
            ShowArg::ToReview => Show::ToReview,
            ShowArg::ToRead => Show::ToRead,
            ShowArg::ToReadOnly => Show::ToReadAnswer,
            ShowArg::ToReadWrite => Show::ToReadChange,
            ShowArg::InProgress => Show::InProgress,
            ShowArg::Failed => Show::Failed,
            ShowArg::Gone => Show::Gone,
            ShowArg::Closed => Show::Closed,
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
        /// Make them the whole stack. A patch with the diff or the title of
        /// one the stack held is a new version of it.
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
    /// A stack's patches, bottom first; or one patch as mail; or a file as
    /// the patch leaves it (--path), whether the stack changes it or not.
    Show {
        name: String,
        patch: Option<String>,
        /// A file, as the patch leaves it.
        #[arg(long, requires = "patch")]
        path: Option<String>,
        /// The stack as it was at this rev, rather than now.
        #[arg(long)]
        rev: Option<String>,
        /// The stack as landed work RUN makes it, waiting for your review or
        /// not (`landed` lists what waits).
        #[arg(long, conflicts_with = "rev")]
        work: Option<String>,
    },
    /// A stack's patches, or some of them, as mail for `git am`.
    Export {
        name: String,
        patches: Vec<String>,
        /// The stack as it was at this rev, rather than now.
        #[arg(long)]
        rev: Option<String>,
        /// The stack as landed work RUN makes it, waiting for your review or
        /// not.
        #[arg(long, conflicts_with = "rev")]
        work: Option<String>,
    },
    /// Edit a stack as `git rebase -i` would; or one patch, in $EDITOR: as
    /// mail, or one of its files as the patch leaves it (--path), the patch
    /// made again from what is saved. The edit lands for your review, as
    /// the agent's work does (`landed`, `review`), unless --accept takes it
    /// at once.
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
        /// the stack's own, .stacked/NAME/user in its repository. Else where the
        /// stack's menus last picked.
        #[arg(long = "in", value_name = "WHERE", requires = "path", value_parser = ["tmp", "pwd", "worktree"])]
        place: Option<String>,
        /// What to start from: an edit not taken before, as the file it was
        /// kept in (tmp), or the commit (pwd, worktree).
        #[arg(long, requires = "path")]
        from: Option<String>,
        /// Should a later patch no longer apply on the edit, its file opens
        /// next in $EDITOR, as that patch would leave it, with conflict
        /// markers where the two disagree, to resolve by hand: one conflict
        /// after another, until the stack applies with all of it.
        #[arg(long, requires = "path")]
        resolve: bool,
        /// Accept the edit at once, as `review --accept-all` would: it goes
        /// in the stack.
        #[arg(long)]
        accept: bool,
        /// Edit what landed work RUN (as `landed` lists it), under review,
        /// makes of the stack, rather than the stack: that work is
        /// rejected, and the edit lands in its place, with its replies and
        /// the comments it opened.
        #[arg(long, value_name = "RUN")]
        work: Option<String>,
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
        /// In the review of the agent's landed work RUN (as `landed` lists
        /// it): on the patch as that work leaves it. The work then goes
        /// back to the agent with it (`review --send-back`), not accepted.
        #[arg(long, value_name = "RUN", requires = "patch")]
        work: Option<String>,
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
    /// Reply to a comment, by its short id. Or change its thread: one turn
    /// written again, or the conversation rewound; N counts the turns as
    /// `comments` lists them, from 1 for the comment itself.
    Reply {
        name: String,
        id: String,
        /// Write what you said last again, rather than add to it: only when
        /// the last word is yours, and not while the agent works on it. A
        /// comment waiting its turn leaves the agent's queue, to be sent
        /// again.
        #[arg(long, conflicts_with = "rewind")]
        edit: bool,
        /// With --edit, the turn to write again, whoever said it.
        #[arg(long, value_name = "N", requires = "edit")]
        turn: Option<usize>,
        /// Rewind the conversation from turn N if it is yours, else from
        /// your turn above it: TEXT takes the place of that turn and of
        /// every turn after it.
        #[arg(long, value_name = "N")]
        rewind: Option<usize>,
        /// Answer the agent's reply to it that waits for your review, in that
        /// review: the work goes back to the agent with your answer after
        /// its reply (`review --send-back`), not accepted.
        #[arg(long, conflicts_with_all = ["edit", "rewind"])]
        answer: bool,
        #[arg(required = true)]
        text: Vec<String>,
    },
    /// Ask the agent about comments, by their short ids, or --all that can
    /// be sent. It may change the code where they are, unless told to
    /// answer only, or to build and test. It runs on its own: its answer
    /// lands as a turn of each thread. With --later, they wait in the
    /// stack's batch instead, until --batch sends it.
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
        /// The agent builds and tests, in the stack's build worktree,
        /// .stacked/NAME/build: on the stack's comments and patches' as a
        /// whole.
        #[arg(long)]
        build_and_test: bool,
        /// A patch's comments, sent to the whole stack: the agent may change
        /// any patch, add patches anywhere, reorder them, as for the stack's
        /// own comments, not only the patch they are on.
        #[arg(long, conflicts_with_all = ["answer_only", "build_and_test"])]
        whole_stack: bool,
        /// Batched, not sent: they wait in the stack's batch until it is
        /// sent, by --batch or from the editor's banner.
        #[arg(long, conflicts_with_all = ["wait", "batch"])]
        later: bool,
        /// The stack's batch sent: its changes to one agent, in one ask (to
        /// the whole stack when they are on more than one patch); its
        /// answers to one agent too, unless --fan-out; each build and test
        /// on its own.
        #[arg(long, conflicts_with_all = ["ids", "all", "answer_only", "build_and_test", "whole_stack"])]
        batch: bool,
        /// With --batch, each answer to an agent of its own, all at once.
        #[arg(long, requires = "batch")]
        fan_out: bool,
        /// Wait for them to be answered, and print the threads.
        #[arg(long)]
        wait: bool,
        /// With --wait, accept what the agent brings back as soon as it
        /// lands, as `review --accept-all` does: its change of the stack,
        /// its replies, the comments it opens.
        #[arg(long, requires = "wait")]
        accept: bool,
    },
    /// What waits for your review on a stack, the first landed first: what
    /// each ask brought back, and the reviews each pull brought from
    /// Phabricator; and what each holds.
    Landed { name: String },
    /// Close the review of what landed, an ask's work or a pull's reviews
    /// (by its run, as `landed` lists it, else the first landed): accept all
    /// of it that is
    /// not marked yet, or reject it. What is accepted is applied; what is
    /// rejected is dropped for good. Rejecting a change of the stack
    /// rejects its replies too.
    Review {
        name: String,
        run: Option<String>,
        #[arg(long, conflicts_with_all = ["reject_all", "send_back"], required_unless_present_any = ["reject_all", "send_back"])]
        accept_all: bool,
        #[arg(long, conflicts_with = "send_back")]
        reject_all: bool,
        /// With what you wrote in its review (`comment --work`, `reply
        /// --answer`), the agent's work goes back to it: rejected, and asked
        /// again, from the stack that work makes, its new work landing to
        /// review in its place.
        #[arg(long)]
        send_back: bool,
        /// Close the comments the agent was asked about too, done with,
        /// once their replies are in their threads.
        #[arg(long)]
        close_asking: bool,
    },
    /// The runs, oldest first: asks, applies and pulls, and what came of
    /// them.
    Runs { name: Option<String> },
    /// What a run's agent wrote.
    Transcript { id: String },
    /// Pull a stack's reviews from Phabricator: what is new waits for your
    /// review, as the agent's work does (`landed`, `review`).
    Pull { name: String },
    /// Put a stack into the worktree here: its base checked out and its
    /// patches committed, detached or on --branch. Only with nothing
    /// uncommitted.
    Apply {
        name: String,
        #[arg(long)]
        branch: Option<String>,
    },
    /// Where HEAD of the checkout stacked runs in is, against stack NAME: at
    /// its base, at one of its patches (its commits since the base being the
    /// stack's patches, by their diff), or elsewhere.
    Head {
        name: String,
        /// HEAD moved there: detached at the stack's base with its first N
        /// patches applied (0, the base), the commits the checkout has that
        /// are those patches taken as they are; the branch it was on as it
        /// was. Not while files git tracks have changes.
        #[arg(long, value_name = "N", conflicts_with_all = ["messages_to_stack", "messages_to_checkout"])]
        to: Option<usize>,
        /// The messages of HEAD's commits that are the stack's patches by
        /// their diff, but not by their message, taken as those patches' own
        /// (PATCH as `show` names it; every one whose message differs if
        /// none): an edit by hand, landing for your review.
        #[arg(long, num_args = 0.., value_name = "PATCH", conflicts_with = "messages_to_checkout")]
        messages_to_stack: Option<Vec<String>>,
        /// The other way: those commits made again with the stack's
        /// messages, and every one above them to HEAD, their files as they
        /// were; the branch HEAD is on moved to them.
        #[arg(long, num_args = 0.., value_name = "PATCH")]
        messages_to_checkout: Option<Vec<String>>,
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
    /// from: `stacked config > ~/.stacked.toml`; those the checkout's own
    /// settings set (.stacked/local.toml at its top) are said so. With KEY,
    /// its value; with VALUE too, KEY written in that file, the rest of it
    /// as it was.
    Config {
        /// A setting, as the file names it: `editor`, `view.layout`,
        /// `keys.down`.
        key: Option<String>,
        /// Its value; for a list, each of its values (`keys.down down j`).
        values: Vec<String>,
        /// Takes KEY out of the file: it is then as it would be without.
        #[arg(long, requires = "key", conflicts_with = "values")]
        unset: bool,
        /// The checkout's own settings alone, in .stacked/local.toml at its
        /// top, which override the others there: printed, or KEY read,
        /// written or taken out there.
        #[arg(long)]
        local: bool,
    },
    /// Sets something of stacked up, asking you: `agent`, the tools every
    /// ask may use.
    Setup {
        #[command(subcommand)]
        what: SetupWhat,
    },
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

#[derive(Subcommand)]
enum SetupWhat {
    /// The tools every ask may use besides stacked's own: the agent is
    /// started (and stopped at once) in the stack's repository, else here,
    /// where it has your MCP servers, and says what it has; you let every
    /// ask use each, or not, and what you let it is written as agent.tools
    /// in that checkout's own settings, .stacked/local.toml at its top,
    /// which override the others there.
    Agent {
        name: Option<String>,
        /// On a screen of its own, a box for each tool, rather than a
        /// question each.
        #[arg(long)]
        tui: bool,
        /// Written in the settings' file (~/.stacked.toml, or
        /// $STACKED_CONFIG), for every checkout without its own, rather
        /// than in the checkout's own.
        #[arg(long)]
        global: bool,
    },
}

/// What `stacked setup agent` works with: the directory the agent is asked
/// in, as the settings there have it run; the settings' tools now, and the
/// file they are in, if one sets them; the file they are written in, and
/// how.
struct SetUp<'a> {
    dir: &'a Path,
    cfg: &'a stacked::config::Config,
    was: Vec<String>,
    from: Option<PathBuf>,
    to: PathBuf,
    save: &'a dyn Fn(&[String]) -> std::result::Result<(), String>,
}

/// Says so when `key`, written in (or taken out of) the settings' file
/// `written`, is not the one used in the checkout `dir` is in: that
/// checkout's own settings set it.
fn overridden(dir: &Path, key: &str, written: &Path) {
    if let Some(l) = stacked::config::set_in(dir, key).filter(|f| f != written) {
        eprintln!(
            "stacked: {} sets {key} too, and is the one used in its checkout (`stacked config --local --unset {key}` there takes it out)",
            l.display()
        );
    }
}

/// What `tools` are said as: each, or none.
fn listed(tools: &[String]) -> String {
    if tools.is_empty() { "none".to_string() } else { tools.join(", ") }
}

/// As `setup_agent`, on a screen of its own: a box for each tool, the
/// settings' value now and as it is to be.
fn setup_agent_tui(up: SetUp) -> Result<()> {
    eprintln!("stacked: asking the agent, in {}, what tools it has…", up.dir.display());
    let (tools, servers) = stacked::ask::agent_tools(up.dir, up.cfg)?;
    let from = up.from.as_ref().map(|f| f.display().to_string());
    let s = stacked::setup::Setup::new(&tools, &servers, &up.was, from.as_deref(), &up.to.display().to_string());
    match stacked::setup::run(s, &format!("the agent's, in {}", up.dir.display()))? {
        Some(chosen) => {
            (up.save)(&chosen).map_err(Error)?;
            eprintln!("stacked: agent.tools written in {}: {}", up.to.display(), listed(&chosen));
        }
        None => eprintln!("stacked: left; nothing written"),
    }
    Ok(())
}

/// Asks the agent what tools it has in its directory, then you, on stdin,
/// which every ask may use: an MCP server's all at once or one by one,
/// then each other tool stacked does not give. What you let it is written
/// as agent.tools, what you did not taken out.
fn setup_agent(up: SetUp) -> Result<()> {
    use std::io::BufRead;
    let dir = up.dir;
    let (tools, servers) = stacked::ask::agent_tools(dir, up.cfg)?;
    let mut chosen: Vec<String> = up.was.clone();
    let mut lines = std::io::stdin().lock().lines();
    let mut answer = |q: &str| -> String {
        eprint!("{q} ");
        lines.next().and_then(|l| l.ok()).unwrap_or_default().trim().to_ascii_lowercase()
    };
    let keep = |chosen: &mut Vec<String>, tool: &str, yes: bool| {
        chosen.retain(|t| t != tool);
        if yes {
            chosen.push(tool.to_string());
        }
    };
    eprintln!(
        "stacked: the agent, in {}, has {} tools; every ask has {} already, and the commands stacked lets each kind of ask run.",
        dir.display(),
        tools.len(),
        stacked::ask::GIVEN.join(", ")
    );
    match &up.from {
        Some(f) => eprintln!("Now, agent.tools in {}: {}.", f.display(), listed(&chosen)),
        None => eprintln!("Now, agent.tools: {}.", listed(&chosen)),
    }
    eprintln!("What you let every ask use is written in {}.", up.to.display());
    // An MCP server's tools, all at once or one by one.
    let mut names: Vec<String> = servers.clone();
    for t in &tools {
        if let Some(s) = t.strip_prefix("mcp__").and_then(|r| r.split_once("__")).map(|(s, _)| s.to_string())
            && !names.contains(&s)
        {
            names.push(s);
        }
    }
    for server in &names {
        let prefix = format!("mcp__{server}__");
        let its: Vec<&String> = tools.iter().filter(|t| t.starts_with(&prefix)).collect();
        let all = format!("{prefix}*");
        let short: Vec<&str> = its.iter().map(|t| &t[prefix.len()..]).collect();
        eprintln!("The MCP server {server}: {}.", if short.is_empty() { "no tools".into() } else { short.join(", ") });
        match answer(&format!("Let every ask use all of its tools ({all})? [y/N/e: each]")).as_str() {
            "y" | "yes" => {
                chosen.retain(|t| !t.starts_with(&prefix));
                chosen.push(all);
            }
            "e" | "each" => {
                chosen.retain(|t| *t != all);
                for t in its {
                    let yes = matches!(answer(&format!("  {t}? [y/N]")).as_str(), "y" | "yes");
                    keep(&mut chosen, t, yes);
                }
            }
            _ => chosen.retain(|t| !t.starts_with(&prefix)),
        }
    }
    // Any other tool stacked does not give every ask.
    for t in tools.iter().filter(|t| !t.starts_with("mcp__") && !stacked::ask::GIVEN.contains(&t.as_str())) {
        let warn = if stacked::ask::CHANGING.contains(&t.as_str()) {
            " It changes things: an answer could too."
        } else {
            ""
        };
        let yes = matches!(answer(&format!("Let every ask use {t}?{warn} [y/N]")).as_str(), "y" | "yes");
        keep(&mut chosen, t, yes);
    }
    (up.save)(&chosen).map_err(Error)?;
    eprintln!("stacked: agent.tools written in {}: {}", up.to.display(), listed(&chosen));
    Ok(())
}

/// Whether the process `pid` is gone: exited (reaped now, if it was this
/// one's child, as an ask's started here is), or not there at all.
fn gone(pid: u32) -> bool {
    let mut status = 0;
    match unsafe { libc::waitpid(pid as i32, &mut status, libc::WNOHANG) } {
        0 => false,
        p if p == pid as i32 => true,
        _ => !ask::alive(pid),
    }
}

fn parse_rev(s: &str) -> Result<usize> {
    s.trim_start_matches('@')
        .parse()
        .map_err(|_| Error(format!("{s:?} is not a rev (as @12)")))
}

/// Stack `name` as it was at `rev`, else as it is.
fn stack_at<'a>(db: &'a Db, name: &str, rev: Option<&str>) -> Result<&'a stacked::db::Stack> {
    let Some(r) = rev else { return db.stack(name) };
    let s = db.node(parse_rev(r)?)?;
    if s.name != name {
        return err(format!("@{} is a stack of {}, not {name}", r.trim_start_matches('@'), s.name));
    }
    Ok(s)
}

/// Stack `name` at `rev`, or as landed work `work` (a run, or the start of
/// its id) makes it; else as it is.
fn stack_of(db: &Db, name: &str, rev: Option<&str>, work: Option<&str>) -> Result<stacked::db::Stack> {
    let Some(w) = work else { return Ok(stack_at(db, name, rev)?.clone()) };
    let runs: Vec<String> = db
        .run_ids()
        .filter(|id| id.starts_with(w) && db.run(id).is_some_and(|r| r.stack == name))
        .map(str::to_string)
        .collect();
    match runs.as_slice() {
        [id] => ask::work_stack(db, id),
        [] => err(format!("no run {w} on {name}")),
        _ => err(format!("{w} names more than one run: give more of it")),
    }
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

fn print_note(db: &Db, name: &str, n: &Note, busy: &Busy) {
    print_thread(db, name, n, &note_state(db, name, n, busy));
}

/// `n`'s short id, where it is and, if any, `state`; then its thread.
fn print_thread(db: &Db, name: &str, n: &Note, state: &str) {
    let state = if state.is_empty() { String::new() } else { format!("\t{state}") };
    println!("{}\t{}{state}", short_id(&n.change, &n.id), where_on(db, name, n));
    for line in stacked::thread::said_lines(&n.text) {
        println!("  {line}");
    }
    for a in &n.attached {
        println!("  {}", attached_line(db, name, a));
    }
}

/// Reference `a`, attached to a comment of stack `name`, as a line: a
/// comment by its id, where it is, and whether it is closed; lines as a
/// `refs` line says them.
fn attached_line(db: &Db, name: &str, a: &stacked::refs::Attached) -> String {
    match &a.to {
        stacked::refs::To::Comment { change, id } => match stacked::refs::find_comment(db, name, change, id) {
            Some((c, open)) => {
                let closed = if open { "" } else { " (closed)" };
                format!("{} ↗ {}\t{}{closed}", a.name(), short_id(&c.change, &c.id), where_on(db, name, &c))
            }
            None => format!("{} ↗ a comment that is gone", a.name()),
        },
        stacked::refs::To::Lines(l) => format!("{} ⇢ {}", a.name(), a.lines().map_or(l.clone(), |r| r.said())),
    }
}

/// The deleted comments of stack `name`, its own then each patch's, or
/// patch `spec`'s: each as it was, with the rev that closed it.
fn closed_of(db: &Db, name: &str, spec: Option<&str>) -> Result<Vec<(usize, Note)>> {
    let s = db.stack(name)?;
    let changes: Vec<String> = match spec {
        Some(spec) => vec![patch_of(db, name, spec)?.change.clone()],
        None => std::iter::once(stack_change(name)).chain(s.patches.iter().map(|id| db.patch(id).change.clone())).collect(),
    };
    Ok(changes.iter().flat_map(|c| db.deleted_notes(name, c)).collect())
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

/// The places (from 0) of stack `name`'s patches `specs` names, as `show`
/// takes them.
fn places_of(path: &Path, name: &str, specs: &[String]) -> Result<Vec<usize>> {
    let db = Db::load(path)?;
    let s = db.stack(name)?;
    specs.iter().map(|spec| db.resolve(s, spec)).collect()
}

/// The run of the landed work on stack `name` that `run` starts, waiting
/// for review, if one is named.
fn work_of(db: &Db, name: &str, run: Option<&str>) -> Result<Option<String>> {
    let Some(run) = run else { return Ok(None) };
    let queue: Vec<String> = stacked::held::queue(db, name).into_iter().map(|r| r.id.clone()).collect();
    match queue.iter().filter(|id| id.starts_with(run)).collect::<Vec<_>>().as_slice() {
        [id] => Ok(Some((*id).clone())),
        [] => err(format!("no work on {name} waits for review as {run}: `stacked landed {name}` lists what does")),
        _ => err(format!("{run} names more than one run: give more of it")),
    }
}

/// Says what an edit by hand of stack `name` came to: landed, waiting for
/// your review, or with `accept` accepted at once; or nothing changed.
fn edited(path: &Path, name: &str, done: ops::Edited, accept: bool) -> Result<()> {
    match done {
        ops::Edited::Landed { said, run } if accept => {
            let reviewed = Db::edit(path, |db| stacked::held::close(db, &run, &Default::default(), Some(true)))?;
            // What waited on the review may go.
            ask::launch(path)?;
            eprintln!("stacked: {said}: {reviewed}");
        }
        ops::Edited::Landed { said, run } => eprintln!(
            "stacked: {said}: it waits for your review, as run {run}: `stacked review {name} {run} --accept-all` takes it, `stacked landed {name}` says what it holds"
        ),
        ops::Edited::Unchanged(why) => eprintln!("stacked: {why}"),
        ops::Edited::Refused { why, .. } => return err(why),
    }
    Ok(())
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
            Db::edit(path, |db| {
                if db.head(&name).is_some() {
                    ops::by_hand(db, &name)?;
                }
                ops::import(db, &name, drafts, replace, on)
            })?;
        }
        Cmd::Base { name, rev } => {
            let base = git::rev_parse(&here(), &rev)?;
            let repo = git::toplevel(&here())?;
            Db::edit(path, |db| {
                ops::by_hand(db, &name)?;
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
                ops::by_hand(db, &name)?;
                ops::add(db, &name, draft, at.map(|n| n.saturating_sub(1)))
            })?;
        }
        Cmd::Show { name, patch: None, rev, work, .. } => {
            let db = Db::load(path)?;
            let s = stack_of(&db, &name, rev.as_deref(), work.as_deref())?;
            for (i, id) in s.patches.iter().enumerate() {
                let p = db.patch(id);
                println!("{}\t{}\t{}\t{}", i + 1, &p.change[..8], &id[..8], p.title());
            }
        }
        Cmd::Show { name, patch: Some(spec), path: None, rev, work } => {
            let db = Db::load(path)?;
            let s = stack_of(&db, &name, rev.as_deref(), work.as_deref())?;
            print!("{}", ops::render(db.patch(&s.patches[db.resolve(&s, &spec)?])));
        }
        Cmd::Show { name, patch: Some(spec), path: Some(f), rev, work } => {
            let db = Db::load(path)?;
            let s = stack_of(&db, &name, rev.as_deref(), work.as_deref())?;
            let at = db.resolve(&s, &spec)?;
            match ops::file_at(&db, &s, at, &f)? {
                Some(text) => print!("{text}"),
                None => return err(format!("as patch {} leaves it, {f} is not there, or is not text", at + 1)),
            }
        }
        Cmd::Export { name, patches, rev, work } => {
            let db = Db::load(path)?;
            let s = stack_of(&db, &name, rev.as_deref(), work.as_deref())?;
            let mut at: Vec<usize> = (0..s.patches.len()).collect();
            if !patches.is_empty() {
                at = patches.iter().map(|p| db.resolve(&s, p)).collect::<Result<_>>()?;
                at.sort();
            }
            let ps: Vec<_> = at.iter().map(|&i| db.patch(&s.patches[i])).collect();
            print!("{}", format_mbox(&ps));
        }
        Cmd::Edit { name, patch: None, accept, work, .. } => {
            let work = work_of(&Db::load(path)?, &name, work.as_deref())?;
            edited(path, &name, ops::edit_stack(path, &name, work.as_deref())?, accept)?
        }
        Cmd::Edit { name, patch: Some(p), line, path: None, accept, work, .. } => {
            let db = Db::load(path)?;
            let work = work_of(&db, &name, work.as_deref())?;
            let at = db.resolve(&ops::edited_from(&db, &name, work.as_deref())?, &p)?;
            edited(path, &name, ops::edit_patch(path, &name, at, line, work.as_deref())?, accept)?;
        }
        Cmd::Edit { name, patch: Some(p), line, path: Some(f), place, from, resolve, accept, work } => {
            let db = Db::load(path)?;
            let work = work_of(&db, &name, work.as_deref())?;
            let at = db.resolve(&ops::edited_from(&db, &name, work.as_deref())?, &p)?;
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
            let mut done = ops::edit_code(path, &name, at, &f, line.unwrap_or(1), &place, again, work.as_deref())?;
            // One conflict after another, until one is left unresolved.
            while resolve
                && let ops::Edited::Refused { kept, conflict: Some(c), .. } = &done
                && !c.left
            {
                done = ops::resolve(path, &name, c, kept.clone())?;
            }
            let (why, kept, conflict) = match done {
                ops::Edited::Refused { why, kept, conflict } => (why, kept, conflict),
                done => return edited(path, &name, done, accept),
            };
            let again = if resolve { " --resolve" } else { "" };
            let then = match conflict {
                Some(c) if !resolve => format!(
                    "; with --resolve, {} of patch {} opens next, to resolve the conflict by hand",
                    c.file,
                    c.at + 1
                ),
                _ => String::new(),
            };
            match kept {
                ops::Kept::Text(text) => {
                    let short = f.rsplit('/').next().unwrap_or(&f);
                    let keep = std::env::temp_dir().join(format!("stacked-edit-{}-{short}", std::process::id()));
                    std::fs::write(&keep, text).map_err(|e| Error(format!("{}: {e}", keep.display())))?;
                    return err(format!(
                        "{why}; nothing changed. Your edit is kept in {k}: `stacked edit {name} {p} --path {f} --from {k}{again}` takes it up again{then}",
                        k = keep.display()
                    ));
                }
                ops::Kept::Commit { place, commit } => {
                    return err(format!(
                        "{why}; nothing changed. Your edit is kept in commit {short}: `stacked edit {name} {p} --path {f} --in {w} --from {commit}{again}` takes it up again{then}",
                        short = &commit[..12.min(commit.len())],
                        w = place.name(),
                    ));
                }
            }
        }
        Cmd::Move { name, patch, to } => Db::edit(path, |db| {
            ops::by_hand(db, &name)?;
            let from = db.resolve(db.stack(&name)?, &patch)?;
            ops::move_patch(db, &name, from, to.saturating_sub(1)).map(|_| ())
        })?,
        Cmd::Drop { name, patches } => Db::edit(path, |db| {
            ops::by_hand(db, &name)?;
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
            Db::edit(path, |db| {
                ops::by_hand(db, &name)?;
                db.undo(&name)
            })?;
        }
        Cmd::Redo { name } => {
            Db::edit(path, |db| {
                ops::by_hand(db, &name)?;
                db.redo(&name)
            })?;
        }
        Cmd::Select { rev } => {
            let rev = parse_rev(&rev)?;
            Db::edit(path, |db| {
                let name = db.node(rev)?.name.clone();
                ops::by_hand(db, &name)?;
                db.select(rev)
            })?;
        }
        Cmd::Delete { name } => {
            Db::edit(path, |db| {
                ops::by_hand(db, &name)?;
                db.delete_stack(&name)
            })?;
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
        Cmd::Comment { name, patch, line, about: Some(rev), text, .. } => {
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
        Cmd::Comment { name, patch, line, about: None, text, work } => {
            let text = text.join(" ");
            let db = Db::load(path)?;
            db.stack(&name)?;
            let work = work_of(&db, &name, work.as_deref())?;
            // In a review, the patch as the work leaves it.
            let patch_of = |db: &Db, name: &str, spec: &str| -> Result<Patch> {
                let s = ops::edited_from(db, name, work.as_deref())?;
                Ok(db.patch(&s.patches[db.resolve(&s, spec)?]).clone())
            };
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
                    // A comment on the line already: this is a turn of it,
                    // not while the agent works on it.
                    match db.note(&p.change, &key) {
                        Some(n) if ask::with_agent(&db).contains_key(&(n.change.clone(), n.id.clone())) => {
                            return err("the agent is working on the comment on that line: wait for it to land");
                        }
                        Some(n) if stacked::held::reply_waiting(&db, &n.change, &n.id).is_some() => {
                            return err("the agent's reply to the comment on that line waits for your review: review it first");
                        }
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
            let note = Note { review: work.or(note.review), ..note };
            let short = short_id(&note.change, &note.id);
            Db::edit(path, |db| db.set_note(note))?;
            println!("{short}");
        }
        Cmd::Comments { name, patch, show: Some(ShowArg::Closed) } => {
            let db = Db::load(path)?;
            for (rev, n) in closed_of(&db, &name, patch.as_deref())? {
                print_thread(&db, &name, &n, &format!("closed @{rev}"));
            }
        }
        Cmd::Comments { name, patch, show } => {
            let db = Db::load(path)?;
            let busy = ask::with_agent(&db);
            let held = stacked::held::replies_waiting(&db);
            let show = show.map_or(Show::All, ShowArg::show);
            for (n, gone) in comments_of(&db, &name, patch.as_deref())? {
                if shows(show, &n, &busy, &held, gone) {
                    print_note(&db, &name, &n, &busy);
                }
            }
        }
        Cmd::Reply { name, id, answer: true, text, .. } => {
            let text = text.join(" ");
            Db::edit(path, |db| {
                let n = db.find_note(&name, &id)?.clone();
                let Some((run, k)) = stacked::held::reply_waiting(db, &n.change, &n.id) else {
                    return err("no reply of the agent's to it waits for your review: reply to it instead");
                };
                stacked::held::answer(db, &run, k, &text)
            })?;
            eprintln!("stacked: your answer is in its review: the work goes back to the agent with it (`stacked review {name} --send-back`)");
        }
        Cmd::Reply { name, id, edit, turn, rewind, text, .. } => {
            let text = text.join(" ");
            let off = Db::edit(path, |db| {
                let n = db.find_note(&name, &id)?.clone();
                // Not while the agent works on it: its reply is to what it
                // was asked.
                if ask::with_agent(db).contains_key(&(n.change.clone(), n.id.clone())) {
                    return err("the agent is working on it: wait for it to land");
                }
                if stacked::held::reply_waiting(db, &n.change, &n.id).is_some() {
                    return err("the agent's reply to it waits for your review: review it first");
                }
                // Which turn, from 1, as `comments` lists them: where it is
                // among all the turns, those deleted the first way too.
                let at = |k: usize| {
                    let listed: Vec<usize> = all_turns(&n.text).iter().enumerate().filter(|(_, t)| !t.deleted).map(|(i, _)| i).collect();
                    match k.checked_sub(1).and_then(|k| listed.get(k)) {
                        Some(&i) => Ok(i),
                        None => err(format!("there is no turn {k}: the thread has {}", listed.len())),
                    }
                };
                if !edit && rewind.is_none() {
                    db.set_note(Note {
                        text: append_turn(&n.text, Role::You, &text, None),
                        failed: None,
                        ..n
                    })?;
                    return Ok(None);
                }
                let again = match (rewind, turn) {
                    (Some(k), _) => {
                        let Some(from) = rewind_from(&n.text, at(k)?) else {
                            return err(format!("turn {k} has no turn of yours there or above it: `--edit --turn {k}` writes it again"));
                        };
                        stacked::thread::rewind(&n.text, from, &text).ok_or_else(|| Error("nothing to write: the conversation is as it was".into()))?
                    }
                    (None, Some(k)) => {
                        let Some(again) = edit_turn(&n.text, at(k)?, &text) else {
                            return err(format!("turn {k} cannot be written again"));
                        };
                        again
                    }
                    (None, None) => {
                        let Some(again) = replace_last_turn(&n.text, &text) else {
                            return err("the last word is not yours: reply instead");
                        };
                        again
                    }
                };
                if again == n.text {
                    return Ok(None);
                }
                // Off the agent's queue, until it is sent again; in the
                // batch, it stays there, to go as it is now.
                let off = n.queued.as_ref().is_some_and(|q| !q.batched()).then(|| short_id(&n.change, &n.id));
                db.set_note(Note {
                    text: again,
                    failed: None,
                    queued: n.queued.clone().filter(|q| q.batched()),
                    ..n
                })?;
                Ok(off)
            })?;
            if let Some(short) = off {
                println!("{short}\tedited: off the agent's queue, until it is sent again");
            }
        }
        Cmd::Send { name, ids, all, patch, answer_only, build_and_test, whole_stack, later, batch, fan_out, wait, accept } => {
            let way = match (answer_only, build_and_test) {
                (_, true) => Way::Test,
                (true, _) => Way::Answer,
                _ => Way::Change,
            };
            let db = Db::load(path)?;
            let since = db.last_rev();
            let busy = ask::with_agent(&db);
            let picked: Vec<Note> = if batch {
                db.stack(&name)?;
                db.stack_notes(&name).into_iter().filter(|n| n.queued.as_ref().is_some_and(|q| q.batched())).cloned().collect()
            } else if all {
                comments_of(&db, &name, patch.as_deref())?
                    .into_iter()
                    .map(|(n, _)| n)
                    .filter(|n| ask::sendable(n) && !busy.contains_key(&(n.change.clone(), n.id.clone())))
                    .filter(|n| stacked::held::reply_waiting(&db, &n.change, &n.id).is_none())
                    .filter(|n| way != Way::Test || ask::can_test(&db, n))
                    .collect()
            } else if ids.is_empty() {
                return err("name the comments to send by their short ids (`stacked comments NAME` lists them), or --all");
            } else {
                ids.iter().map(|id| db.find_note(&name, id).cloned()).collect::<Result<_>>()?
            };
            if picked.is_empty() {
                return err(if batch { "the batch is empty: `send --later` puts comments in it" } else { "no comment to send" });
            }
            let ats: Vec<(String, String)> = picked.iter().map(|n| (n.change.clone(), n.id.clone())).collect();
            if later {
                ask::batch(path, &name, &ats, way, whole_stack)?;
                let said = if whole_stack { stacked::db::WHOLE } else { way.said() };
                for (c, i) in &ats {
                    println!("{}\tbatched ({said})", short_id(c, i));
                }
                return Ok(());
            }
            if batch {
                for way in [Way::Change, Way::Answer, Way::Test] {
                    let sent = ask::batched(&db, &name, way);
                    if ask::release_batch(path, &name, way, fan_out)? == 0 {
                        continue;
                    }
                    ask::launch(path)?;
                    let db = Db::load(path)?;
                    let busy = ask::with_agent(&db);
                    for ((c, i), _) in &sent {
                        let Some(n) = db.note(c, i) else { continue };
                        match (&n.queued, busy.get(&(c.clone(), i.clone()))) {
                            (Some(q), _) => {
                                let w = ask::waits_on(&db, &name, q.way).unwrap_or_else(|| "its turn".into());
                                println!("{}\tready ({}), it goes {w}", short_id(c, i), q.said());
                            }
                            (None, Some((way, run))) => {
                                let whole = *way == Way::Change && !n.on_stack() && ask::run_scope(&db, run) == Some(ask::Scope::Stack);
                                println!("{}\tsent ({})", short_id(c, i), if whole { stacked::db::WHOLE } else { way.said() });
                            }
                            (None, None) => println!("{}\tsent ({})", short_id(c, i), way.said()),
                        }
                    }
                }
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
            for (_, these) in batches.iter().filter(|_| !batch) {
                let waits = ask::send(path, &name, these, way, whole_stack)?;
                for (c, i) in these {
                    match &waits {
                        None => println!("{}\tsent ({said})", short_id(c, i)),
                        Some(w) => println!("{}\tready ({said}), it goes {w}", short_id(c, i)),
                    }
                }
            }
            if wait {
                let sent = ats;
                // The runs asking about what was sent.
                let ours = |r: &stacked::db::Run| {
                    ask::info(r).is_some_and(|i| i.notes.iter().any(|sh| sent.contains(&(sh.change.clone(), sh.id.clone()))))
                };
                // The processes of the asks about it: until each is gone,
                // what landing does after filing (the checkout moved back,
                // the next asks started) is not done.
                let mut pids: Vec<u32> = Vec::new();
                loop {
                    std::thread::sleep(std::time::Duration::from_millis(200));
                    let db = Db::load(path)?;
                    ask::land_orphans(path, &db);
                    // What is going, and what landed, as one look at the
                    // database sees them: an ask landing meanwhile is in
                    // neither, and is seen the next time.
                    let db = Db::load(path)?;
                    for r in db.live_runs().into_iter().filter(|r| ours(r) && r.state == stacked::db::RunState::Started) {
                        if let Some(pid) = r.pid.filter(|p| !pids.contains(p)) {
                            pids.push(pid);
                        }
                    }
                    let busy = ask::with_agent(&db);
                    // A change waiting for the review of the one before it
                    // goes once you have reviewed that: nothing to wait for.
                    let reviewing = stacked::held::change_waiting(&db, &name).is_some();
                    let going = sent.iter().any(|k| {
                        busy.contains_key(k)
                            || db
                                .note(&k.0, &k.1)
                                .is_some_and(|n| n.queued.as_ref().is_some_and(|q| !(q.batched() || reviewing && q.way == Way::Change)))
                    });
                    // Accepted as soon as landed: what waited on its review
                    // goes.
                    if accept {
                        let landed: Vec<String> = stacked::held::queue(&db, &name).into_iter().filter(|r| ours(r)).map(|r| r.id.clone()).collect();
                        for id in &landed {
                            Db::edit(path, |db| stacked::held::close(db, id, &Default::default(), Some(true)))?;
                        }
                        if !landed.is_empty() {
                            ask::launch(path)?;
                            continue;
                        }
                    }
                    pids.retain(|&pid| !gone(pid));
                    if going || !pids.is_empty() {
                        continue;
                    }
                    for r in stacked::held::queue(&db, &name).into_iter().filter(|r| ours(r)) {
                        let said = r.held.as_ref().map(|h| h.said()).unwrap_or_default();
                        println!("{}\twaits for your review: {said}", r.id);
                    }
                    // What came of it: every comment written since, the
                    // sent ones where they are now (a split moves them with
                    // their line), and those an answer opened.
                    let mut failed = 0;
                    for n in db.stack_notes(&name) {
                        if db.note_rev(&n.change, &n.id).is_some_and(|rev| rev > since) {
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
        Cmd::Landed { name } => {
            let db = Db::load(path)?;
            db.stack(&name)?;
            for r in stacked::held::queue(&db, &name) {
                let Some(h) = &r.held else { continue };
                let review = db.review(&r.id).cloned().unwrap_or_default();
                // Gone on from the work before it, guessing: reviewed after it.
                let after = ask::guessing(&db, r).map_or(String::new(), |on| format!("\tgoes on from {on}, which waits for your review"));
                println!("{}\t{}\t{}{after}", r.id, time(h.at), h.said());
                for p in h.parts() {
                    if matches!(p, stacked::held::Part::Reply(k) if review.taken.contains(&k)) {
                        continue;
                    }
                    let mark = match review.marks.get(&p.key()) {
                        Some(true) => "\taccepted",
                        Some(false) => "\trejected",
                        None => "",
                    };
                    let what = match p {
                        stacked::held::Part::Stack => h.stack.as_ref().map_or(String::new(), |c| format!("change\t{}", c.op)),
                        stacked::held::Part::Reply(k) => {
                            let rp = &h.replies[k];
                            format!("reply\t{}\t{}", short_id(&rp.change, &rp.id), rp.text.lines().next().unwrap_or(""))
                        }
                        stacked::held::Part::Opened(k) => {
                            let n = &h.opened[k];
                            let first = stacked::thread::thread_turns(&n.text).first().map(|t| t.body.lines().next().unwrap_or("").to_string());
                            format!("opened\t{}: {}\t{}", n.file, n.preview, first.unwrap_or_default())
                        }
                    };
                    println!("  {what}{mark}");
                }
            }
        }
        Cmd::Review { name, run, send_back: true, .. } => {
            let db = Db::load(path)?;
            let id = match work_of(&db, &name, run.as_deref())? {
                Some(id) => id,
                None => stacked::held::queue(&db, &name).first().map(|r| r.id.clone()).ok_or_else(|| Error(format!("nothing on {name} waits for your review")))?,
            };
            let marks = db.review(&id).map(|v| v.marks.clone()).unwrap_or_default();
            let said = ask::send_back(path, &name, &id, &marks)?;
            ask::launch(path)?;
            println!("{id}\t{said}");
        }
        Cmd::Review { name, run, accept_all, close_asking, .. } => {
            let said = Db::edit(path, |db| {
                let queue: Vec<String> = stacked::held::queue(db, &name).into_iter().map(|r| r.id.clone()).collect();
                let id = match &run {
                    Some(run) => match queue.iter().filter(|id| id.starts_with(run.as_str())).collect::<Vec<_>>().as_slice() {
                        [id] => (*id).clone(),
                        [] => return err(format!("no work on {name} waits for review as {run}: `stacked landed {name}` lists what does")),
                        _ => return err(format!("{run} names more than one run: give more of it")),
                    },
                    None => queue.first().cloned().ok_or_else(|| Error(format!("nothing on {name} waits for your review")))?,
                };
                let marks = db.review(&id).map(|v| v.marks.clone()).unwrap_or_default();
                let said = stacked::held::close_with(db, &id, &marks, Some(accept_all), close_asking)?;
                Ok(format!("{id}\treviewed: {said}"))
            })?;
            // What waited on the review may go.
            ask::launch(path)?;
            println!("{said}");
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
        Cmd::Config { key: None, local: false, .. } => {
            let p = stacked::config::path();
            let state = if p.exists() { "read from" } else { "none in" };
            println!("# {state} {}", p.display());
            // One that is there and not read: what is wrong says why.
            if let Some(l) = stacked::config::local_path(&here()) {
                if stacked::config::read_local(&l) {
                    println!("# then from {}, this checkout's own, over it", l.display());
                } else if !l.exists() {
                    println!("# none in {}, where this checkout's own would be", l.display());
                }
            }
            for w in stacked::config::warnings() {
                println!("# wrong: {w}");
            }
            let local = stacked::config::local_keys(&here());
            print!("{}", stacked::config::mark_local(&stacked::config::get().render(), &local));
        }
        Cmd::Config { key: None, local: true, .. } => {
            let l = stacked::config::local_file(&here()).map_err(Error)?;
            match std::fs::read_to_string(&l) {
                Ok(text) => {
                    println!("# {}, this checkout's own settings, over {}", l.display(), stacked::config::path().display());
                    let at = format!("{}:", l.display());
                    for w in stacked::config::warnings().iter().filter(|w| w.starts_with(&at)) {
                        println!("# wrong: {w}");
                    }
                    print!("{text}");
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => println!("# none in {}", l.display()),
                Err(e) => return err(format!("{}: {e}", l.display())),
            }
        }
        Cmd::Config { key: Some(key), values, unset, local: true } => {
            let dir = here();
            if unset {
                let l = stacked::config::unset_local(&dir, &key).map_err(Error)?;
                eprintln!("stacked: {key} taken out of {}", l.display());
            } else if values.is_empty() {
                let Some(now) = stacked::config::local_value(&dir, &key).map_err(Error)? else {
                    let l = stacked::config::local_file(&dir).map_err(Error)?;
                    return err(format!("{key} is not set in {}", l.display()));
                };
                for v in now {
                    println!("{v}");
                }
            } else {
                let l = stacked::config::set_local(&dir, &key, &values).map_err(Error)?;
                eprintln!("stacked: {key} written in {}", l.display());
            }
        }
        Cmd::Config { key: Some(key), values, unset, local: false } => {
            let p = stacked::config::path();
            if unset {
                stacked::config::unset(&p, &key).map_err(Error)?;
                eprintln!("stacked: {key} taken out of {}", p.display());
                overridden(&here(), &key, &p);
            } else if values.is_empty() {
                let now = stacked::config::value_of(stacked::config::get(), &key).map_err(Error)?;
                if now.is_empty() {
                    return err(format!("{key} is not set"));
                }
                for v in now {
                    println!("{v}");
                }
            } else {
                stacked::config::set(&p, &key, &values).map_err(Error)?;
                eprintln!("stacked: {key} written in {}", p.display());
                overridden(&here(), &key, &p);
            }
        }
        Cmd::Setup { what: SetupWhat::Agent { name, tui, global } } => {
            let dir = match name {
                Some(n) => ops::base_of(Db::load(path)?.stack(&n)?)?.0,
                None => here(),
            };
            // Where what you let it is written: refused before the agent is
            // asked, if it cannot be.
            let to = match global {
                true => stacked::config::path(),
                false => stacked::config::local_to_write(&dir).map_err(Error)?,
            };
            let (cfg, wrong) = stacked::config::of(&dir);
            for w in wrong.iter().filter(|w| !stacked::config::warnings().contains(w)) {
                eprintln!("stacked: {w}");
            }
            // The tools now: those used there; or the settings' file's own.
            let (was, from) = match global {
                true => {
                    let set = stacked::config::file_sets(&to, "agent.tools");
                    (stacked::config::Config::load(&to).0.agent.tools, set.then(|| to.clone()))
                }
                false => (cfg.agent.tools.clone(), stacked::config::set_in(&dir, "agent.tools")),
            };
            let save = |chosen: &[String]| match global {
                true => stacked::config::set(&to, "agent.tools", chosen),
                false => stacked::config::set_local(&dir, "agent.tools", chosen).map(|_| ()),
            };
            let up = SetUp { dir: &dir, cfg: &cfg, was, from, to: to.clone(), save: &save };
            if tui {
                setup_agent_tui(up)?;
            } else {
                setup_agent(up)?;
            }
            if global {
                overridden(&dir, "agent.tools", &to);
            }
        }
        Cmd::Head { name, messages_to_stack: Some(specs), .. } => {
            let places = places_of(path, &name, &specs)?;
            let done = stacked::head::messages_to_stack(path, &name, &here(), &places)?;
            edited(path, &name, done, false)?;
        }
        Cmd::Head { name, messages_to_checkout: Some(specs), .. } => {
            let places = places_of(path, &name, &specs)?;
            println!("{}", stacked::head::messages_to_checkout(path, &name, &here(), &places)?);
        }
        Cmd::Head { name, to: Some(n), .. } => println!("{}", stacked::head::move_to(path, &name, &here(), n)?),
        Cmd::Head { name, .. } => {
            let db = Db::load(path)?;
            ops::base_of(db.stack(&name)?)?;
            let Some(h) = stacked::head::find(&db, &name, &here())? else {
                return err(format!("{} is not a checkout of {name}'s repository", here().display()));
            };
            println!("HEAD, {}, is {}", h.git_said(), h.said(&name));
            if let stacked::head::At::Stack(on) = &h.at {
                for (k, m) in &on.messages {
                    println!("the message of patch {} differs here: \"{}\"", k + 1, m.lines().next().unwrap_or(""));
                }
            }
        }
        Cmd::Check => {
            let db = Db::load(path)?;
            for w in &db.warnings {
                println!("{w}");
            }
            if db.warnings.is_empty() {
                println!("{} records, none passed over", db.last_rev());
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
        Some(Cmd::Open { .. } | Cmd::Prompt { .. } | Cmd::Record { .. } | Cmd::Land { .. } | Cmd::Fits { .. } | Cmd::Config { .. }) | None
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
