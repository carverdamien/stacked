use std::io::{IsTerminal, Read};
use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand, ValueEnum};

use stacked::ask;
use stacked::db::{Db, Error, Note, Patch, Record, Result, Way, err, local_time as time, short_id, stack_change};
use stacked::mbox::{Draft, base_commit, format_mbox, parse_mail, parse_mbox};
use stacked::ops;
use stacked::thread::{Role, all_turns, append_turn, edit_turn, replace_last_turn, rewind_from, yours};
use stacked::tui::{Busy, Show, note_state, shows, where_on};
use stacked::view::{MSG_PATH, find_line, fold, note_lines, title_key};
#[cfg(feature = "moz-phab")]
use stacked::review;
use stacked::{git, tree};

/// A stack editor. Patches, stacks and every edit made to them, comments
/// and what the agent (claude) and Phabricator said: all in one file that is only ever
/// appended to. Your checkout is not touched unless you ask: every ask of the agent
/// runs in a worktree of its own.
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

/// A shell `completions` completes stacked in.
#[derive(Clone, Copy, ValueEnum)]
enum Shell {
    Zsh,
}

/// Which comments `comments` lists, as the editor's Filter comments… picks
/// them.
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
    /// A tour of the editor, in sections, the most useful first, on a game
    /// of tic-tac-toe played in a file, a move a patch. It writes nothing on
    /// disk but `[demo] seen` in the settings: opened again, it starts on
    /// the list of its sections, to start at any of them. `stacked demo` is
    /// built by default, with the `demo` feature: `--no-default-features
    /// --features app,moz-phab,try-push,web` builds stacked without it.
    #[cfg(feature = "demo")]
    Demo,
    /// The editor in a browser: a server on this machine, and the browser
    /// opened on it. The cursor follows the mouse: a click is Enter, a right
    /// click Space; writing a comment, a click outside its box is Esc. With
    /// a port the settings fix, the next launch gives the tab already open
    /// 2 seconds to come back before it opens a new one. Closing the last
    /// tab quits, unless a tab comes back within 2 seconds, as a reload
    /// does. A terminal `$EDITOR` opens in the page, which sends it every
    /// key but those with ⌘, unless the settings' web.editor is "terminal":
    /// then it opens where `stacked web` was started, as an editor with a
    /// window of its own opens that window. `stacked web` is built by
    /// default, with the `web` feature: `--no-default-features --features
    /// app,demo,moz-phab,try-push` builds stacked without it.
    #[cfg(feature = "web")]
    Web {
        /// The port of 127.0.0.1 to listen on this time, 0 for any free
        /// one, with a new token. Without it, the settings' web.port says
        /// which: "random" (unless set), any free one each time;
        /// "fixed-random", any free one the first time, which stacked writes
        /// in the settings as web.kept_port and listens on each time after;
        /// or a port from 1 to 65535. A port in use is said, and no other is
        /// tried.
        #[arg(long)]
        port: Option<u16>,
        /// Only say where it is: do not open the browser.
        #[arg(long)]
        no_open: bool,
    },
    /// StackED: the editor in a window of its own, drawn as in a terminal,
    /// the mouse as in `stacked web`: the cursor follows it, a click is
    /// Enter, a right click Space; writing a comment, a click outside its
    /// box is Esc. `$EDITOR` opens where it was started. The window's File
    /// menu opens more databases, each in a window of its own. `stacked app`
    /// is built by default, with the `app` feature: `--no-default-features
    /// --features demo,moz-phab,try-push,web` builds stacked without it.
    #[cfg(feature = "app")]
    App {
        /// StackED made an app of the system's: on macOS, StackED.app in DIR
        /// (else ~/Applications), for the Dock and the Finder, this stacked
        /// copied into it with StackED's icon, so made again once stacked is
        /// updated; elsewhere, StackED's launcher in DIR/applications and its
        /// icon in DIR/icons (DIR else $XDG_DATA_HOME, else ~/.local/share),
        /// running this stacked, for the desktop's applications. Opened, it
        /// opens again the windows open when it was last quit, else the
        /// database $STACKED_FILE or the settings name, else an empty window:
        /// File → Open… opens one.
        #[arg(long, value_name = "DIR", num_args = 0..=1)]
        bundle: Option<Option<PathBuf>>,
    },
    /// The completion of stacked's commands for a shell, zsh's: kept as
    /// `_stacked` in a folder of `$fpath`, or read by `source <(stacked
    /// completions zsh)` once compinit has run. TAB then offers the
    /// commands and their options, and the stacks, patches, comments and
    /// runs of the database.
    Completions { shell: Shell },
    /// What the shell's completion offers for the words after `stacked`,
    /// the last being typed: for the function `completions` prints.
    #[command(name = "__complete", hide = true)]
    Complete {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        words: Vec<String>,
    },
    /// The rebase plan PLAN an ask wrote, given git as its todo TODO once
    /// checked: the sequence editor of its rebases. A plan that runs a
    /// command is refused, the rebase not started.
    #[command(name = "__plan", hide = true)]
    Plan { plan: PathBuf, todo: PathBuf },
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
        /// one the stack held is a new version of it, or that patch itself
        /// when its message, its author and its diff's lines are the same,
        /// wherever its hunks start, and its diff as stored still leaves the
        /// files as the commit does.
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
    /// all. `runs` has the asks and the pulls.
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
        /// A file the patch changes, as its diff names it: the comment is on
        /// the file as a whole, on its name.
        #[arg(long, requires = "patch", conflicts_with = "line")]
        path: Option<String>,
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
        /// on its own, and each try push the editor batched.
        #[arg(long, conflicts_with_all = ["ids", "all", "answer_only", "build_and_test", "whole_stack"])]
        batch: bool,
        /// With --batch, each answer to an agent of its own, all at once.
        #[arg(long, requires = "batch")]
        fan_out: bool,
        /// How hard the agent works on them, as claude's --effort takes it:
        /// low, medium, high, xhigh or max. Unless given, high; with
        /// --batch, the effort each comment was batched at.
        #[arg(long, value_name = "LEVEL", value_parser = effort_arg())]
        effort: Option<stacked::db::Effort>,
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
    /// Phabricator; and what is left of each to review: its parts, or the
    /// comments the agent was asked about, to close or keep.
    Landed { name: String },
    /// Close the review of what landed, an ask's work or a pull's reviews
    /// (by its run, as `landed` lists it, else the first landed): accept all
    /// of it that is
    /// not marked yet, or reject it, each operation of a change of the
    /// stack not marked yet too. What is accepted is applied, a change of
    /// the stack as the operations accepted leave it; what is rejected is
    /// dropped for good. Rejecting every operation of a change of the stack
    /// rejects its replies too. A pull's comments are not rejected: closing
    /// its review files every comment it pulled, and --reject-all rejects
    /// its change of the stack alone, refused for a pull that brought
    /// comments alone. Accepting a change of
    /// the stack waits until each answer made on the stack before it is
    /// marked keep, restart or stop, in the review of that work in the
    /// editor: --accept-all marks none of them.
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
        /// With --send-back, how hard the agent works on it, as claude's
        /// --effort takes it: low, medium, high, xhigh or max. Unless given,
        /// the effort the work was asked at.
        #[arg(long, value_name = "LEVEL", value_parser = effort_arg())]
        effort: Option<stacked::db::Effort>,
        /// Close the comments the agent was asked about too, done with,
        /// once their replies are in their threads.
        #[arg(long)]
        close_asking: bool,
    },
    /// The runs, oldest first: asks and pulls, and what came of them.
    Runs { name: Option<String> },
    /// What a run's agent wrote.
    Transcript { id: String },
    /// Stop the agent of an ask that runs, by its run (as `runs` lists it,
    /// or the start of its id): sent SIGTERM, then SIGKILL if it has not
    /// exited a few seconds later, its ask lands at once, as failed, and
    /// what waited behind it goes. A lint job's linter is stopped the same
    /// way: the job lands at once, as stopped, and nothing changes in the
    /// stack. A lint job waiting its turn is dropped.
    Stop { id: String },
    /// Phabricator, over Conduit with moz-phab's token: `pull`, a stack's
    /// revisions and their reviews; `import`, a new stack of its revisions.
    /// `stacked moz-phab` is built by default, with the `moz-phab` feature:
    /// `--no-default-features --features app,demo,try-push,web` builds
    /// stacked without it.
    #[cfg(feature = "moz-phab")]
    MozPhab {
        #[command(subcommand)]
        what: MozPhabWhat,
    },
    /// Where HEAD of the checkout stacked runs in is, against stack NAME: at
    /// its base, at one of its patches (its commits since the base being the
    /// stack's patches, by their diff), or elsewhere; the patches whose
    /// message differs from its commit's; and where --to may not go, and
    /// why it may not move HEAD.
    Head {
        name: String,
        /// HEAD moved there: detached at the stack's base with its first N
        /// patches applied (0, the base), the commits the checkout has that
        /// are those patches taken as they are; the branch it was on as it
        /// was. Only when HEAD is where stacked last left it, or on the
        /// stack, its commits the stack's first patches with nothing above
        /// them and the same messages; never below a commit that names a
        /// revision its patch does not, which is kept as it is, with every
        /// commit under it. Not while files git tracks have changes.
        #[arg(long, value_name = "N")]
        to: Option<usize>,
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
    /// are read from (~/.stacked/config.toml, or $STACKED_CONFIG; with
    /// $STACKED_HOME, config.toml in that folder), to start one from:
    /// `stacked config > ~/.stacked/config.toml`; those the checkout's own
    /// settings set (.stacked/local.toml at its top) are said so. With KEY,
    /// its value; with VALUE too, KEY written in that file (made, with its
    /// folder, if it is not there), the rest of it as it was.
    ///
    /// Every ask of the agent runs in a worktree of its own, never in your
    /// checkout: agent.worktree, for a change, and agent.answer_only_worktree,
    /// for an answer, say when it is made, `ondemand` or `precreate`.
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
    /// Sets something of stacked up: `agent`, what each kind of ask may
    /// use, asking you; `lint`, the linter of this checkout.
    Setup {
        #[command(subcommand)]
        what: SetupWhat,
    },
    /// An ask's prompt; a try push's selection's once stacked has listed
    /// the tasks mach can push (for the ask's wrapper).
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
    /// Push the tasks a try push's review accepted to try (for the editor,
    /// which starts it as the review closes).
    #[cfg(feature = "try-push")]
    #[command(name = "try-push", hide = true)]
    TryPush { id: String },
    /// Lint a stack as its lint job says, patch by patch, and land what the
    /// linter fixed (for the editor, which starts it when its turn comes).
    #[command(name = "lint-job", hide = true)]
    LintJob { id: String },
}

/// Whether `cmd` is one that stacked runs itself, never you: an ask's
/// (`prompt`, `record`, `land`, `fits`), git's (`__plan`), the shell's
/// completion, a try push's, and a lint job's.
fn internal(cmd: &Option<Cmd>) -> bool {
    #[cfg(feature = "try-push")]
    if matches!(cmd, Some(Cmd::TryPush { .. })) {
        return true;
    }
    matches!(
        cmd,
        Some(Cmd::Prompt { .. } | Cmd::Record { .. } | Cmd::Land { .. } | Cmd::Fits { .. } | Cmd::Plan { .. } | Cmd::Complete { .. } | Cmd::LintJob { .. })
    )
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

#[cfg(feature = "moz-phab")]
#[derive(Subcommand)]
enum MozPhabWhat {
    /// Pull a new stack from Phabricator: revision REVISION (D123, 123, or
    /// its link) with the revisions it stacks on, down to the first that is
    /// not open, and those stacked on it while one is open; none that a
    /// stack holds already. The stack is made empty, on the commit the
    /// bottom one's diff is on, which the repository here must have, and
    /// the pull lands into it for your review, a patch of each revision's
    /// latest diff, with its commit's message as uploaded while the
    /// revision's title and summary are what that message says, else
    /// those, and the reviewers' comments: `stacked review NAME RUN
    /// --accept-all` makes the patches. Prints the run.
    Import {
        /// D123, 123, or the revision's link.
        revision: String,
        /// The stack's name; else the revision's (D123), or the first of
        /// D123-2, D123-3, … that no stack has.
        #[arg(long)]
        name: Option<String>,
    },
    /// Pull a stack from Phabricator: every revision its patches name, with
    /// the revisions they stack on that it does not hold, and those stacked
    /// on top; each revision's latest diff, title and summary where they are
    /// no version of its patch stacked knows, and the reviewers' comments.
    /// It is queued as an edit is, and what it brings waits for your review,
    /// as the agent's work does (`landed`, `review`). Prints the run.
    Pull { name: String },
}

#[derive(Subcommand)]
enum SetupWhat {
    /// What each kind of ask may use besides what stacked gives it, a list
    /// each: answer-only asks (agent.answer_tools), asks that change the
    /// stack (agent.change_tools), and, built with the try-push feature,
    /// asks that select a try push's tasks (agent.try_tools). They are
    /// shown as one list of tools, each with a box for each kind: read
    /// only, change, try push; above them, what stacked always gives each,
    /// among it the checkout's linter, which changes may run, named by its
    /// command. The agent is started (and stopped at once)
    /// in the stack's repository, else here, where it has your MCP servers,
    /// and says what tools it has. The boxes are ticked as the settings
    /// say, else as stacked has it: nothing for changes; for answers, curl
    /// and jq, and what the checkout's own claude settings allow; for try
    /// pushes, as for answers. What the checkout's skills let run, and the
    /// programs its instructions to the agent name, are listed too,
    /// unticked. Rules that mean the same, as Bash(cmd:*) and Bash(cmd *),
    /// are one row, which says where each comes from; a command is shown
    /// and saved as Bash(cmd *), which reads better, an older form in
    /// stacked's own settings rewritten so. What the checkout's claude
    /// settings of yours deny is said, refused to every ask. You check
    /// them, add to them, and save them; no ask waits for it, each going
    /// with stacked's defaults until it is saved. They are written in that
    /// checkout's own settings, .stacked/local.toml at its top, which
    /// override the others there. What the reviews of the agent's work
    /// allowed from now on, of the tools it could not use, is listed too,
    /// from that file, each rule to keep or take out there.
    Agent {
        name: Option<String>,
        /// On a screen of its own, a box for each kind of ask and each
        /// tool, rather than asked line by line.
        #[arg(long)]
        tui: bool,
        /// Written in the settings' file (~/.stacked/config.toml, or
        /// $STACKED_CONFIG), for every checkout without its own, rather
        /// than in the checkout's own.
        #[arg(long)]
        global: bool,
    },
    /// The linter of the checkout it is run in, which the editor's `Lint
    /// the stack` runs on a stack of it, and which the asks that change one
    /// may run on the files they change: COMMAND, a program and its
    /// arguments, the files of each patch after them, run by stacked with
    /// no shell. Written in that checkout's own settings alone,
    /// .stacked/local.toml at its top (`[lint] command`), never in the
    /// settings' file. Unset, a checkout whose top has ./mach lints with
    /// `./mach lint --fix`, nothing written. With no COMMAND, the linter in
    /// use is printed, and where it comes from. This lints nothing: the
    /// stack's menu does.
    Lint {
        /// The linter: a program, then its arguments (`eslint --fix`),
        /// split again as a shell splits words when it is run.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, value_name = "COMMAND")]
        command: Vec<String>,
        /// Lint turned off in this checkout, ./mach or not: `command = ""`
        /// written.
        #[arg(long, conflicts_with = "command")]
        off: bool,
    },
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

/// The setup asked line by line (`setup::ask_lines`): the answers read
/// from stdin, what is asked and said written on stderr, after how many
/// `tools` the agent in `dir` has, and where the setup is saved, `to`.
fn setup_lines(s: &mut stacked::setup::Setup, dir: &Path, to: &Path, tools: usize) -> stacked::setup::Chosen {
    use std::io::BufRead;
    eprintln!(
        "stacked: the agent, in {}, has {tools} tools. What each kind of ask may use is saved in {}; Enter keeps what is ticked.",
        dir.display(),
        to.display()
    );
    let mut lines = std::io::stdin().lock().lines();
    stacked::setup::ask_lines(
        s,
        &mut |q| {
            eprint!("{q} ");
            lines.next().and_then(|l| l.ok()).unwrap_or_default().trim().to_string()
        },
        &mut |l| eprintln!("{l}"),
    )
}

/// What `--effort` takes: a level claude takes, as it names them; any
/// other is refused, with the levels it takes said.
fn effort_arg() -> impl clap::builder::TypedValueParser<Value = stacked::db::Effort> {
    use clap::builder::TypedValueParser;
    clap::builder::PossibleValuesParser::new(stacked::db::Effort::NAMES).map(|s: String| stacked::db::Effort::parse(&s).unwrap_or_default())
}

/// Whether the process `pid` is gone: exited (waited for now, if it is
/// the wrapper of an ask started here), or not there at all.
fn gone(pid: u32) -> bool {
    !ask::alive(pid)
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
/// `refs` line says them; a web link whole.
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
        stacked::refs::To::Link(url) => format!("{} ⇗ {url}", a.name()),
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
            p.as_ref().map(stacked::view::note_keys).unwrap_or_default();
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
    // An ask asked at an effort, with it: `answer(max)`.
    println!("{}\t{}\t{} {}{req}\t{state}\t{}", r.id, time(r.time), ask::what_said(r), r.stack, r.summary);
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
            // Not taken, as it changes a patch that should not change: the
            // edit waits for your review all the same.
            let reviewed = edit_asking(path, |db| stacked::held::close(db, &run, &Default::default(), Some(true))).map_err(|e| {
                Error(format!("{}; your edit waits for your review, as run {run}: `stacked review {name} {run} --accept-all` takes it", e.0))
            })?;
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

/// Runs `edit` on the database at `path`, as `Db::edit` does. A write of a
/// stack it makes refused, as it would change patches that should not
/// change (`stacked::protected`): on a terminal, what the editor's dialog
/// says is said there and asked, and once `accept` is typed, to the letter,
/// `edit` runs again with those patches confirmed, for the one operation it
/// makes; anything else typed, or with no terminal to ask on, the refusal
/// is what went wrong, nothing changed, which says why and which patches.
fn edit_asking<T>(path: &Path, mut edit: impl FnMut(&mut Db) -> Result<T>) -> Result<T> {
    let mut confirmed: Vec<String> = Vec::new();
    loop {
        let r = match Db::edit_confirmed(path, &confirmed, &mut edit) {
            Ok(out) => return Ok(out),
            Err(stacked::db::Failed::Error(e)) => return Err(e),
            Err(stacked::db::Failed::Protected(r)) => r,
        };
        if !(std::io::stdin().is_terminal() && std::io::stderr().is_terminal()) {
            return err(r.said_off_terminal());
        }
        eprintln!("stacked: {}.", r.title(false));
        for l in r.lines() {
            eprintln!("{l}");
        }
        let fetched = r.fetched(stacked::db::now());
        if !fetched.is_empty() {
            eprintln!("{fetched}");
        }
        eprint!("{}: ", r.type_said().trim_end_matches('.'));
        let mut answer = String::new();
        std::io::stdin().read_line(&mut answer).map_err(|e| Error(format!("stdin: {e}")))?;
        if answer.trim_end_matches(['\n', '\r']) != "accept" {
            return err(r.not_typed(false));
        }
        // Asked again, should another write refuse others now: those typed
        // for go on being confirmed.
        confirmed.extend(r.ids());
    }
}

fn run(path: &Path, cmd: Cmd) -> Result<()> {
    match cmd {
        Cmd::Open { .. } => unreachable!(),
        #[cfg(feature = "demo")]
        Cmd::Demo => stacked::demo::run()?,
        #[cfg(feature = "web")]
        Cmd::Web { port, no_open } => {
            let tokens = stacked::home::get().web_token();
            let (bound, unkept) = stacked::web::listen(port, &stacked::config::get().web, &stacked::config::path(), &tokens)?;
            if let Some(why) = unkept {
                eprintln!("stacked: {why}");
            }
            stacked::web::catch_signals();
            // Where it is said as it serves, with the token kept for the
            // next launch, or once the tab of the launch before came back or
            // did not.
            bound.serve(path, !no_open)?;
        }
        #[cfg(feature = "app")]
        Cmd::App { bundle: Some(dir) } => {
            let made = stacked::launcher::make(dir)?;
            println!("{}", made.at.display());
            eprintln!("stacked: {}", made.said);
        }
        #[cfg(feature = "app")]
        Cmd::App { bundle: None } => stacked::app::run(path)?,
        Cmd::Plan { plan, todo } => {
            let text = std::fs::read_to_string(&plan)
                .map_err(|e| Error(format!("no plan in {}: {e}; write it there first", plan.display())))?;
            ask::check_plan(&text)?;
            std::fs::write(&todo, text).map_err(|e| Error(format!("{}: {e}", todo.display())))?;
        }
        Cmd::Completions { shell: Shell::Zsh } => print!("{}", stacked::complete::ZSH),
        Cmd::Complete { words } => {
            let mut cli = <Cli as clap::CommandFactory>::command();
            cli.build();
            print!("{}", stacked::complete::complete(&cli, &words, path).lines());
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
            edit_asking(path, |db| {
                if db.head(&name).is_some() {
                    ops::by_hand(db, &name)?;
                }
                ops::import(db, &name, drafts.clone(), replace, on.clone())
            })?;
        }
        Cmd::Base { name, rev } => {
            let base = git::rev_parse(&here(), &rev)?;
            let repo = git::toplevel(&here())?;
            edit_asking(path, |db| {
                ops::by_hand(db, &name)?;
                let s = db.stack(&name)?.clone();
                let op = format!("base {}", &base[..12]);
                db.set_stack_on(&name, s.patches, Some(base.clone()), Some(repo.clone()), &op)
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
            edit_asking(path, |db| {
                if let Some((base, repo)) = &new
                    && db.head(&name).is_none()
                {
                    db.new_stack(&name, base, repo)?;
                }
                ops::by_hand(db, &name)?;
                ops::add(db, &name, draft.clone(), at.map(|n| n.saturating_sub(1)))
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
            edited(path, &name, ops::edit_stack(path, &name, work.as_deref(), &here())?, accept)?
        }
        Cmd::Edit { name, patch: Some(p), line, path: None, accept, work, .. } => {
            let db = Db::load(path)?;
            let work = work_of(&db, &name, work.as_deref())?;
            let at = db.resolve(&ops::edited_from(&db, &name, work.as_deref())?, &p)?;
            edited(path, &name, ops::edit_patch(path, &name, at, line, work.as_deref(), &here())?, accept)?;
        }
        Cmd::Edit { name, patch: Some(p), line, path: Some(f), place, from, resolve, accept, work } => {
            let db = Db::load(path)?;
            let work = work_of(&db, &name, work.as_deref())?;
            let at = db.resolve(&ops::edited_from(&db, &name, work.as_deref())?, &p)?;
            let place = ops::Place::named(place.as_deref().or(db.choice(&name, "edit")).unwrap_or("tmp"), &here());
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
            let mut done = ops::edit_code(path, &name, at, &f, line.unwrap_or(1), &place, again, work.as_deref(), &here())?;
            // One conflict after another, until one is left unresolved.
            while resolve
                && let ops::Edited::Refused { kept, conflict: Some(c), .. } = &done
                && !c.left
            {
                done = ops::resolve(path, &name, c, kept.clone(), &here())?;
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
        Cmd::Move { name, patch, to } => edit_asking(path, |db| {
            ops::by_hand(db, &name)?;
            let from = db.resolve(db.stack(&name)?, &patch)?;
            ops::move_patch(db, &name, from, to.saturating_sub(1)).map(|_| ())
        })?,
        Cmd::Drop { name, patches } => edit_asking(path, |db| {
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
            edit_asking(path, |db| {
                ops::by_hand(db, &name)?;
                db.undo(&name)
            })?;
        }
        Cmd::Redo { name } => {
            edit_asking(path, |db| {
                ops::by_hand(db, &name)?;
                db.redo(&name)
            })?;
        }
        Cmd::Select { rev } => {
            let rev = parse_rev(&rev)?;
            edit_asking(path, |db| {
                let name = db.node(rev)?.name.clone();
                ops::by_hand(db, &name)?;
                db.select(rev)
            })?;
        }
        Cmd::Delete { name } => {
            edit_asking(path, |db| {
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
                text: yours(&text.join(" ")),
                preview: e.text.chars().take(80).collect(),
                about: Some(rev),
                ..Default::default()
            };
            let short = short_id(&note.change, &note.id);
            Db::edit(path, |db| db.set_note(note))?;
            println!("{short}");
        }
        Cmd::Comment { name, patch, line, path: on_file, about: None, text, work } => {
            let text = text.join(" ");
            let db = Db::load(path)?;
            db.stack(&name)?;
            let work = work_of(&db, &name, work.as_deref())?;
            // In a review, the patch as the work leaves it.
            let patch_of = |db: &Db, name: &str, spec: &str| -> Result<Patch> {
                let s = ops::edited_from(db, name, work.as_deref())?;
                Ok(db.patch(&s.patches[db.resolve(&s, spec)?]).clone())
            };
            let note = match (&patch, &line, &on_file) {
                (None, ..) => Note {
                    change: stack_change(&name),
                    id: format!("s{}", stacked::db::unique()),
                    text: yours(&text),
                    ..Default::default()
                },
                (Some(spec), None, None) => {
                    let p = patch_of(&db, &name, spec)?;
                    Note {
                        change: p.change.clone(),
                        id: format!("p{}", stacked::db::unique()),
                        line: title_key(&p),
                        text: yours(&text),
                        file: MSG_PATH.into(),
                        preview: p.title().chars().take(80).collect(),
                        ..Default::default()
                    }
                }
                (Some(spec), line, file) => {
                    let p = patch_of(&db, &name, spec)?;
                    // On a line, or on a file as a whole, its name: its key,
                    // its file, and what shows it.
                    let (key, file, preview, on) = match line {
                        Some(l) => {
                            let key = line_of(&p, l)?;
                            let at = note_lines(&p).into_iter().find(|nl| nl.key == key).unwrap();
                            (key, at.file, fold(&at.text).chars().take(80).collect::<String>(), "that line")
                        }
                        None => {
                            let want = file.as_deref().unwrap_or_default();
                            let Some((path, key)) = stacked::view::file_keys(&p).into_iter().find(|(f, _)| f == want) else {
                                return err(format!("patch {spec} does not change {want}"));
                            };
                            (key, path.clone(), path, "that file")
                        }
                    };
                    // A comment there already: this is a turn of it, not
                    // while the agent works on it.
                    match db.note(&p.change, &key) {
                        Some(n) if ask::with_agent(&db).contains_key(&(n.change.clone(), n.id.clone())) => {
                            return err(format!("the agent is working on the comment on {on}: wait for it to land"));
                        }
                        Some(n) if stacked::held::reply_waiting(&db, &n.change, &n.id).is_some() => {
                            return err(format!("the agent's reply to the comment on {on} waits for your review: review it first"));
                        }
                        Some(n) => Note {
                            text: append_turn(&n.text, Role::You, &text, None),
                            failed: None,
                            ..n.clone()
                        },
                        None => Note {
                            change: p.change.clone(),
                            id: key.clone(),
                            line: key,
                            text: yours(&text),
                            file,
                            preview,
                            ..Default::default()
                        },
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
            let (short, off, detached) = Db::edit(path, |db| {
                let n = db.find_note(&name, &id)?.clone();
                let short = short_id(&n.change, &n.id);
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
                    return Ok((short, false, vec![]));
                }
                // A turn written again keeps the references it attaches.
                let again = match (rewind, turn) {
                    (Some(k), _) => {
                        let Some(from) = rewind_from(&n.text, at(k)?) else {
                            return err(format!("turn {k} has no turn of yours there or above it: `--edit --turn {k}` writes it again"));
                        };
                        stacked::thread::rewind(&n.text, from, &text).ok_or_else(|| Error("nothing to write: the conversation is as it was".into()))?
                    }
                    (None, Some(k)) => {
                        let i = at(k)?;
                        let was = all_turns(&n.text).into_iter().nth(i).map(|t| t.body).unwrap_or_default();
                        let Some(again) = edit_turn(&n.text, i, &stacked::refs::kept(&was, &text)) else {
                            return err(format!("turn {k} cannot be written again"));
                        };
                        again
                    }
                    (None, None) => {
                        let was = stacked::thread::last_mine(&n.text).unwrap_or_default();
                        let Some(again) = replace_last_turn(&n.text, &stacked::refs::kept(&was, &text)) else {
                            return err("the last word is not yours: reply instead");
                        };
                        again
                    }
                };
                if again == n.text {
                    return Ok((short, false, vec![]));
                }
                // Off the agent's queue, until it is sent again; in the
                // batch, it stays there, to go as it is now.
                let off = n.queued.as_ref().is_some_and(|q| !q.batched());
                let mut note = Note {
                    text: again,
                    failed: None,
                    queued: n.queued.clone().filter(|q| q.batched()),
                    ..n.clone()
                };
                // What it no longer names is detached.
                let waiting = stacked::refs::waiting(db, &n.change, &n.id);
                let detached = stacked::refs::settle(&n, &mut note, waiting.as_deref());
                db.set_note(note)?;
                Ok((short, off, detached))
            })?;
            if off {
                println!("{short}\tedited: off the agent's queue, until it is sent again");
            }
            if !detached.is_empty() {
                println!("{short}\t{}", stacked::refs::detached_said(&detached));
            }
        }
        Cmd::Send { name, ids, all, patch, answer_only, build_and_test, whole_stack, later, batch, fan_out, effort, wait, accept } => {
            let way = match (answer_only, build_and_test) {
                (_, true) => Way::Test,
                (true, _) => Way::Answer,
                _ => Way::Change,
            };
            // Unless given, high: nothing picked before is kept.
            let at = effort.unwrap_or_default();
            let db = Db::load(path)?;
            let since = db.end_rev();
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
                ask::batch(path, &name, &ats, way, whole_stack, at)?;
                let said = if whole_stack { stacked::db::WHOLE } else { way.said() };
                for (c, i) in &ats {
                    println!("{}\tbatched ({said})", short_id(c, i));
                }
                return Ok(());
            }
            let mut not_sent = 0;
            if batch {
                // A try push batched in the editor too, built with the
                // try-push feature: each on its own.
                let ways = [
                    Way::Change,
                    Way::Answer,
                    Way::Test,
                    #[cfg(feature = "try-push")]
                    Way::Try,
                ];
                for way in ways {
                    let sent = ask::batched(&db, &name, way);
                    let (n, refused) = ask::release_batch(path, &name, way, fan_out, effort)?;
                    // Those it refused, said: each stays a comment, its ask
                    // failed.
                    for ((c, i), why) in &refused {
                        println!("{}\tnot sent ({}): {why}", short_id(c, i), way.said());
                    }
                    not_sent += refused.len();
                    if n == 0 {
                        continue;
                    }
                    ask::launch(path)?;
                    let db = Db::load(path)?;
                    let busy = ask::with_agent(&db);
                    for ((c, i), _) in sent.iter().filter(|(at, _)| !refused.iter().any(|(r, _)| r == at)) {
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
                let waits = ask::send(path, &name, these, way, whole_stack, at)?;
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
                // what landing does after filing (the next asks started) is
                // not done.
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
                            edit_asking(path, |db| stacked::held::close(db, id, &Default::default(), Some(true)))?;
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
                        println!("{}\twaits for your review: {}", r.id, stacked::held::left(&db, r));
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
            // What the batch refused fails the command, as said above; waited
            // for, it is counted with the comments whose ask failed.
            if not_sent > 0 {
                return err(format!("{not_sent} of them {} not sent", if not_sent == 1 { "was" } else { "were" }));
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
                // The agent's: the effort it was asked at.
                let at = ask::effort_said(r).map_or(String::new(), |e| format!("\tasked at {e}"));
                println!("{}\t{}\t{}{at}{after}", r.id, time(h.at), stacked::held::left(&db, r));
                // A pull's comments are not marked: each is filed as its
                // review closes. Its change's operations are, as any.
                let pulled = stacked::held::pulled(r);
                // The change's operations, each marked on its own: as one
                // when every one is marked the same, else counted. Marked
                // as a whole, nothing stored of what each needs, as one.
                let ops = stacked::operations::of_review(&db, r, h);
                for p in stacked::held::parts_left(&db, r) {
                    let counted = match (p, &ops) {
                        (stacked::held::Part::Stack, Some(ops)) => Some(match ops.tally(&review.marks) {
                            (y, 0, 0) if y > 0 => Some(true),
                            (0, n, 0) if n > 0 => Some(false),
                            (0, 0, _) => None,
                            (y, n, open) => {
                                let each: Vec<String> = [(y, "accepted"), (n, "rejected"), (open, "to review")].iter().filter(|(k, _)| *k > 0).map(|(k, w)| format!("{k} {w}")).collect();
                                println!("  change\t{}\toperations: {}", h.stack.as_ref().map_or("", |c| c.op.as_str()), each.join(", "));
                                continue;
                            }
                        }),
                        _ => None,
                    };
                    let marked = !pulled || p == stacked::held::Part::Stack;
                    let mark = match counted.unwrap_or_else(|| review.marks.get(&p.key()).copied()).filter(|_| marked) {
                        Some(true) => "\taccepted",
                        Some(false) => "\trejected",
                        None => "",
                    };
                    let what = match p {
                        stacked::held::Part::Stack => h.stack.as_ref().map_or(String::new(), |c| format!("change\t{}", c.op)),
                        stacked::held::Part::Reply(k) => {
                            let rp = &h.replies[k];
                            let what = match (rp.is_edit(), rp.is_anew()) {
                                (true, _) => "edited",
                                (_, true) => "suggested edit",
                                _ => "reply",
                            };
                            // To a comment you closed, which accepting it reopens.
                            let closed = match stacked::held::goes(&db, &name, rp) {
                                stacked::held::Goes::Reopens(_) => "\tto a comment you closed: accepting it reopens the comment",
                                _ => "",
                            };
                            format!("{what}\t{}\t{}{closed}", short_id(&rp.change, &rp.id), rp.text.lines().next().unwrap_or(""))
                        }
                        stacked::held::Part::Opened(k) => {
                            let n = &h.opened[k];
                            let first = stacked::thread::thread_turns(&n.text).first().map(|t| t.body.lines().next().unwrap_or("").to_string());
                            format!("opened\t{}\t{}", n.where_said(), first.unwrap_or_default())
                        }
                        #[cfg(feature = "try-push")]
                        stacked::held::Part::Task(k) => {
                            let t = &h.tasks[k];
                            format!("task\t{}\t{}", t.label, stacked::refs::split(&t.why).0.lines().next().unwrap_or(""))
                        }
                    };
                    println!("  {what}{mark}");
                }
                // The tools the agent could not use, each as the review marks
                // it, and a call it tried.
                for (k, t) in h.tools.iter().enumerate() {
                    let state = match t.noted {
                        Some(_) => "noted".to_string(),
                        None => stacked::refusals::state(&review.marks, k).said().to_string(),
                    };
                    let rule = stacked::refusals::rule_of(t, &review.marks, k);
                    println!("  tool\t{rule}\t{state}\t{}", t.calls.first().map_or("", String::as_str));
                }
                // What a build and test asked as a follow-up from the review
                // found, to read: its replies, then what it found of each
                // patch; and the follow-ups going, whose work comes into the
                // review once they land.
                for rp in h.replies.iter().filter(|rp| rp.read) {
                    println!("  found\t{}\t{}", short_id(&rp.change, &rp.id), rp.text.lines().next().unwrap_or(""));
                }
                for n in h.follow_ups.iter().flat_map(|f| f.found.iter()) {
                    let first = stacked::thread::thread_turns(&n.text).first().map(|t| t.body.lines().next().unwrap_or("").to_string());
                    println!("  found\t{}\t{}", n.preview, first.unwrap_or_default());
                }
                for f in ask::follow_ups_going(&db, &r.id) {
                    let at = if f.run.is_some() { "with the agent" } else { "waiting its turn" };
                    println!("  following\t{}\t{at}\t{} comment{}", f.way.said(), f.notes.len(), if f.notes.len() == 1 { "" } else { "s" });
                }
                // What the linter reported and did not fix, to read: where
                // each run of it that did ran, then its lines.
                if let Some(l) = &h.lint {
                    let stopped = l.stopped.as_ref().map(|s| s.at);
                    for e in &l.errors {
                        println!("  reported\t{} ({})", e.said(stopped), l.command);
                        for line in &e.lines {
                            println!("    {line}");
                        }
                    }
                }
            }
        }
        Cmd::Review { name, run, send_back: true, effort, .. } => {
            let db = Db::load(path)?;
            let id = match work_of(&db, &name, run.as_deref())? {
                Some(id) => id,
                None => stacked::held::queue(&db, &name).first().map(|r| r.id.clone()).ok_or_else(|| Error(format!("nothing on {name} waits for your review")))?,
            };
            let marks = db.review(&id).map(|v| v.marks.clone()).unwrap_or_default();
            let said = ask::send_back(path, &name, &id, &marks, effort, None)?;
            ask::launch(path)?;
            println!("{id}\t{said}");
        }
        Cmd::Review { effort: Some(_), .. } => {
            return err("--effort goes with --send-back: accepting or rejecting the work asks the agent nothing");
        }
        Cmd::Review { name, run, accept_all, close_asking, .. } => {
            let queued = |db: &Db| -> Result<String> {
                let queue: Vec<String> = stacked::held::queue(db, &name).into_iter().map(|r| r.id.clone()).collect();
                Ok(match &run {
                    Some(run) => match queue.iter().filter(|id| id.starts_with(run.as_str())).collect::<Vec<_>>().as_slice() {
                        [id] => (*id).clone(),
                        [] => return err(format!("no work on {name} waits for review as {run}: `stacked landed {name}` lists what does")),
                        _ => return err(format!("{run} names more than one run: give more of it")),
                    },
                    None => queue.first().cloned().ok_or_else(|| Error(format!("nothing on {name} waits for your review")))?,
                })
            };
            // Accepted in part, the stack the operations accepted leave is
            // checked first, with no lock held, as the editor checks it away
            // from its screen: refused, with why, nothing changed. Under the
            // lock, what that check found is only read again.
            {
                let db = Db::load(path)?;
                let id = queued(&db)?;
                let marks = db.run(&id).map(|r| stacked::held::marks(&db, r)).unwrap_or_default();
                if let Some(refused) = stacked::held::closing_check(&db, &id, &marks, Some(accept_all))? {
                    return err(refused);
                }
            }
            let said = edit_asking(path, |db| {
                let id = queued(db)?;
                // A pull's: every part accepted, none rejected.
                let marks = db.run(&id).map(|r| stacked::held::marks(db, r)).unwrap_or_default();
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
        Cmd::Stop { id } => {
            // The run of that id, else the one it starts.
            let db = Db::load(path)?;
            let named: Vec<String> = match db.run(&id) {
                Some(_) => vec![id.clone()],
                None => db.run_ids().filter(|r| r.starts_with(id.as_str())).map(str::to_string).collect(),
            };
            let id = match named.as_slice() {
                [one] => one.clone(),
                [] => return err(format!("no run {id}: `stacked runs` lists them")),
                _ => return err(format!("{id} names more than one run: give more of it")),
            };
            drop(db);
            let (run, stopped) = ask::stop(path, &id)?;
            // What waited behind it goes.
            ask::launch(path)?;
            if !stopped {
                let state = format!("{:?}", run.state).to_lowercase();
                let what = if stacked::held::linted(&run) { "the lint job is over" } else { "its ask landed" };
                return err(format!("{id} is not running: {what}, {state}: {}", run.summary));
            }
            print_run(&run);
        }
        #[cfg(feature = "moz-phab")]
        Cmd::MozPhab { what: MozPhabWhat::Import { revision, name } } => {
            let r = review::import(path, &here(), &revision, name.as_deref(), &mut |_| {})?;
            let summary = review::said(&Db::load(path)?, &r);
            print_run(&stacked::db::Run { summary, ..r });
        }
        #[cfg(feature = "moz-phab")]
        Cmd::MozPhab { what: MozPhabWhat::Pull { name } } => {
            let r = review::pull(path, &name, review::Asked::Stack, &mut |_| {})?;
            let summary = review::said(&Db::load(path)?, &r);
            print_run(&stacked::db::Run { summary, ..r });
        }
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
        // The linter is written by `stacked setup lint` alone, in a
        // checkout's own settings; read as any setting is.
        Cmd::Config { key: Some(key), values, unset: false, .. } if !values.is_empty() && stacked::config::local_only(&key) => {
            return err(format!(
                "{key} is written by `stacked setup lint COMMAND…`, in the checkout's own settings (.stacked/local.toml): `stacked config` reads it; nothing written"
            ));
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
            // The lists now, and whether a file sets each: as they are used
            // there; or the settings' file's own.
            let kinds = stacked::setup::Kind::ALL;
            let (agent, set) = match global {
                true => (stacked::config::Config::load(&to).0.agent, kinds.map(|k| stacked::config::file_sets(&to, k.key()))),
                false => (cfg.agent.clone(), kinds.map(|k| stacked::config::set_in(&dir, k.key()).is_some())),
            };
            let top = git::toplevel(&dir).ok().map(PathBuf::from);
            let path_var = std::env::var_os("PATH").unwrap_or_default();
            // What the reviews allowed from now on, the checkout's own.
            let start = stacked::setup::Start::new(&agent, set, top.as_deref(), &path_var).accepted_in(&dir);
            if tui {
                eprintln!("stacked: asking the agent, in {}, what tools it has…", dir.display());
            }
            let (tools, servers) = ask::agent_tools(&dir, &cfg)?;
            let said = format!("the agent's tools, in {}", dir.display());
            let mut s = stacked::setup::Setup::new(&tools, &servers, &start, &to.display().to_string(), &said);
            let chosen = match tui {
                true => stacked::setup::run(s)?,
                false => Some(setup_lines(&mut s, &dir, &to, tools.len())),
            };
            let Some(chosen) = chosen else {
                eprintln!("stacked: left; nothing written");
                return Ok(());
            };
            let settings = chosen.settings();
            match global {
                true => stacked::config::set_all(&to, &settings),
                false => stacked::config::set_local_all(&dir, &settings).map(|_| ()),
            }
            .map_err(Error)?;
            eprintln!("stacked: the setup is saved in {}: {}", to.display(), chosen.said());
            // What the reviews allowed that you took out: in the checkout's
            // own settings, where they are, with --global too.
            if !chosen.accepted.is_empty() {
                let at = stacked::config::set_local_all(&dir, &chosen.accepted).map_err(Error)?;
                eprintln!("stacked: taken out of what reviews allowed from now on, in {}: {}", at.display(), chosen.accepted_said());
            }
            if global {
                for (key, _) in &settings {
                    overridden(&dir, key, &to);
                }
            }
        }
        Cmd::Setup { what: SetupWhat::Lint { command, off } } => {
            let dir = here();
            if off {
                let l = stacked::config::set_local(&dir, "lint.command", &[String::new()]).map_err(Error)?;
                eprintln!(
                    "stacked: lint is turned off in this checkout: command = \"\" is written in {}, and `Lint the stack` is grayed on its stacks (`stacked setup lint COMMAND…` sets a linter again)",
                    l.display()
                );
            } else if command.is_empty() {
                // The linter in use, and where it comes from.
                let l = stacked::config::local_file(&dir).map_err(Error)?;
                match stacked::lint::Linter::of(&dir) {
                    stacked::lint::Linter::Set(c) => {
                        println!("{c}");
                        eprintln!("stacked: this checkout's linter, as {} sets it (`stacked setup lint --off` turns lint off)", l.display());
                    }
                    stacked::lint::Linter::Mach => {
                        println!("{}", stacked::lint::MACH);
                        eprintln!(
                            "stacked: this checkout's linter by default, as ./mach is at its top and {} sets none; nothing is written (`stacked setup lint COMMAND…` sets another, `--off` turns lint off)",
                            l.display()
                        );
                    }
                    stacked::lint::Linter::Off => {
                        eprintln!("stacked: lint is turned off in this checkout, as {} sets command = \"\" (`stacked setup lint COMMAND…` sets a linter)", l.display());
                    }
                    stacked::lint::Linter::None => {
                        return err("no linter is set for this checkout: `stacked setup lint COMMAND…` sets one, a program and its arguments, the files to lint after them");
                    }
                }
            } else {
                let c = stacked::lint::quoted(&command);
                let l = stacked::config::set_local(&dir, "lint.command", std::slice::from_ref(&c)).map_err(Error)?;
                eprintln!("stacked: the linter of this checkout is `{c}`, written in {}: the editor's `Lint the stack ({c})` runs it on a stack of this checkout", l.display());
            }
        }
        Cmd::LintJob { id } => stacked::lint::run(path, &id)?,
        Cmd::Head { name, to: Some(n) } => println!("{}", stacked::head::move_to(path, &name, &here(), n)?),
        Cmd::Head { name, to: None } => {
            let db = Db::load(path)?;
            let s = db.stack(&name)?;
            ops::base_of(s)?;
            let Some((h, moves)) = stacked::head::moves_of(&db, &name, &here())? else {
                return err(format!("{} is not a checkout of {name}'s repository", here().display()));
            };
            println!("HEAD, {}, is {}", h.git_said(), h.said(&name));
            // Each commit marked: what the two name, when they differ by the
            // revision, or would but for it; else the commit's first line.
            if let stacked::head::At::Stack(on) = &h.at {
                for (k, theirs, m) in &on.marks {
                    println!("{}", stacked::head::mark_said(*k, &db.patch(&s.patches[*k]).message, theirs, m));
                }
            }
            // Where Move HEAD may not go, and why it may not move HEAD.
            if let Some(lock) = h.lock_said() {
                println!("{lock}");
            }
            if let Err(why) = &moves.case {
                println!("--to N is refused: {why}");
            }
        }
        Cmd::Check => {
            let db = Db::load(path)?;
            for w in &db.warnings {
                println!("{w}");
            }
            if db.warnings.is_empty() {
                println!("{} records, none passed over", db.end_rev());
            }
            for s in db.stacks() {
                match db.verify(s) {
                    Ok(()) => println!("{}: applies", s.name),
                    Err(e) => println!("{}: {}", s.name, e.0.trim_end_matches("; nothing changed")),
                }
            }
        }
        Cmd::Prompt { id } => print!("{}", ask::prompt_for(path, &id)?),
        Cmd::Record { id } => ask::record(path, &id, std::io::stdin().lock())?,
        Cmd::Land { id } => print_run(&ask::land(path, &id, None)?),
        Cmd::Fits { id } => println!("{}", ask::fits(path, &id)?),
        #[cfg(feature = "try-push")]
        Cmd::TryPush { id } => stacked::try_push::push(path, &id)?,
    }
    Ok(())
}

/// StackED's app, opened by the system (from the Dock or the Finder on
/// macOS, from its launcher elsewhere): StackED with what the user's login
/// shell sets, opening again the windows that were open when it was last
/// quit, else one on the database `$STACKED_FILE` or the settings name,
/// else an empty one (`app::dock`). What goes wrong is said in an alert, as
/// there is no terminal to say it in.
#[cfg(feature = "app")]
fn opened_as_app() {
    use stacked::bundle;
    if let Some(env) = bundle::login_env(&bundle::login_shell(), std::time::Duration::from_secs(10)) {
        bundle::adopt(env);
    }
    // A path the settings give is from the home folder; each window's
    // editor runs in its own database's folder.
    if let Some(home) = std::env::var_os("HOME") {
        let _ = std::env::set_current_dir(home);
    }
    // What is to be said of the files of before, with no terminal to say
    // it in: in alerts, each a sentence of its own.
    let said = settle(true)
        .into_iter()
        .map(|s| {
            let mut chars = s.chars();
            chars.next().map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect())
        })
        .collect();
    let settings = stacked::config::Config::load(&stacked::config::path()).0;
    let named = std::env::var_os("STACKED_FILE").map(PathBuf::from).or(settings.file);
    // No terminal for $EDITOR: a terminal window of its own for it.
    bundle::set_no_terminal();
    if let Err(e) = stacked::app::dock(named, said) {
        stacked::app::alert(&e.0);
        std::process::exit(1);
    }
}

/// Where stacked keeps what is yours (`home`), and where the agents keep
/// their rebase plans, as its environment says, set for the whole process
/// before anything reads them: the one place that works the first out from
/// `$HOME`. What is to be said of the files of before, moved into it if
/// this run is yours (`by_you`).
fn settle(by_you: bool) -> Vec<String> {
    let (home, said) = stacked::home::resolve(|k| std::env::var_os(k), by_you);
    stacked::home::set(home);
    stacked::ask::plans_in(stacked::ask::plans_from(|k| std::env::var_os(k)));
    said
}

/// Whether `word`, first on the command line and no command, names a
/// database to open: a file there, or a path to one (`./x`, `x.jsonl`).
fn names_database(word: &str) -> bool {
    Path::new(word).is_file() || word.contains('/') || word.ends_with(".jsonl")
}

fn main() {
    // Opened as the system opens StackED's app: StackED, as an app.
    #[cfg(feature = "app")]
    if std::env::current_exe().is_ok_and(|exe| stacked::launcher::opened(&exe, &std::env::args().collect::<Vec<_>>())) {
        return opened_as_app();
    }
    // A first argument that is not a command is the file to open, if it
    // names one: a file there, or a path to one. Any other word is a command
    // mistyped, which clap says, rather than a new database of its name.
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
    if i < argv.len() && !commands.contains(&argv[i]) && names_database(&argv[i]) {
        argv.insert(i, "open".into());
    }
    let cli = Cli::parse_from(argv);
    // The files of before moved by a run of yours, which says so; not by
    // those stacked runs itself (an ask's, git's, the shell's completion),
    // which could not say so.
    let by_you = !internal(&cli.cmd);
    for said in settle(by_you) {
        eprintln!("stacked: {said}");
    }
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
    let quiet = internal(&cli.cmd) || matches!(cli.cmd, Some(Cmd::Open { .. } | Cmd::Config { .. }) | None);
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
