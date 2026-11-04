# stacked

stacked is an editor for stacks of patches, made to work on them with an
AI agent. A stack is a series of commits on a base, each patch building on
the ones below it, the way you send a change for review one patch at a
time. In stacked you read the patches, comment on their lines as a
reviewer would, and ask the agent (Claude Code's `claude`, unless you name
another) to answer a comment or to change the patches. The agent works on
its own, in a git worktree of its own, never in your checkout, and what it
brings back waits for your review before anything changes. Every patch,
edit, comment and answer is kept in one file that is only ever appended
to, so any edit can be undone and the whole history read back. stacked
also works with Phabricator, as Mozilla uses it with moz-phab: it shows
each revision's status, pulls the reviewers' comments, and submits a patch
as work in progress.

It runs in a terminal, in a browser, or in a window of its own (StackED),
with the same screen and the same keys in all three.

## Installing

stacked is written in Rust and built with cargo. It needs git, and, to ask
the agent, `claude` on your PATH.

```sh
rustup target add wasm32-unknown-unknown
cargo install --git https://github.com/carverdamien/stacked
```

In a clone of this repository, `cargo install --path .` does the same, and
`cargo build --release` builds `target/release/stacked` to run in place.
The wasm target is for the browser front end, whose page is compiled to
WebAssembly; without it, the build stops and says what to do.

### Features

stacked has five cargo features, all on by default:

| Feature    | What it adds |
|------------|--------------|
| `web`      | `stacked web`, the editor in a browser (it needs the wasm target) |
| `app`      | `stacked app`, StackED, the editor in a window of its own |
| `demo`     | `stacked demo`, the tour |
| `moz-phab` | Phabricator: statuses, reviews pulled, revisions imported, patches submitted |
| `try-push` | try pushes of Firefox's CI, their tasks selected by the agent |

To leave some out, turn the defaults off and name the ones to keep. This
builds stacked with the tour and Phabricator, but without the browser,
StackED or the try push:

```sh
cargo install --path . --no-default-features --features demo,moz-phab
```

### StackED as an app

`stacked app` opens StackED from a terminal. To start it as you start any
other application, make it an app of your system:

```sh
stacked app --bundle
```

On macOS, this makes StackED.app in ~/Applications, to drag to the Dock.
It holds a copy of stacked, so make it again after you update stacked. On
Linux and the other desktops that follow freedesktop.org, it adds
StackED's launcher and icon to the desktop's applications; the launcher
runs stacked where it is, so make it again if you move stacked.

On Linux, StackED loads the desktop's libraries (X11 or Wayland, OpenGL,
D-Bus) when it starts, so building it needs no extra package. A desktop
usually has them all; [docs/ai/install.md](docs/ai/install.md) lists the
Debian and Ubuntu packages for one that lacks some.

### Completion in zsh

```sh
mkdir -p ~/.zfunc && stacked completions zsh > ~/.zfunc/_stacked
# then, in ~/.zshrc, before compinit runs: fpath=(~/.zfunc $fpath)
```

TAB then completes stacked's commands and options, and the stacks,
patches, comments and runs of your database.

## Quick start

### Take the tour

```sh
stacked demo
```

The tour is the best way to learn stacked. It plays a game of tic-tac-toe
in a file, one patch per move, and walks you through 14 short sections,
the most useful first: reading a stack, commenting on a line, asking the
agent (a stand-in that needs no network), reviewing what it brings back,
the reading mode, the views of the code, the history, and moving and
dropping patches. It takes about fifteen minutes. Each step says what to
press and waits for you; Tab does the step for you, and ctrl-c leaves.
The tour keeps everything in memory and writes only one setting, so that
the next time it starts on the list of its sections.

### On your own project

In a git checkout, make a stack of the commits on top of main, and open
the editor:

```sh
stacked import bug123 main..HEAD
stacked
```

The stacks live in a database file, `./stacked.jsonl` unless you name
another (`stacked FILE`, `-f FILE`, `$STACKED_FILE`, or the `file`
setting). The editor can also make a stack itself: with no stack yet,
Space offers to import the branch you are on, to import a revision from
Phabricator, or to ask the agent to make one.

From there, a typical round goes like this:

1. **Read a patch.** The left pane lists the patches, bottom first. ↑↓
   pick one, and the middle shows it: its message, then its diff. → goes
   into the middle, and ↑↓ go through its lines.
2. **Write a comment.** On a line, press Enter: a small editor opens under
   the line. Write, then press Esc, which asks what to do: `Save it`,
   `Ask agent…`, `Attach a reference…`, `Throw it away` or `Keep writing`.
   A comment can also go on a file as a whole, on a patch (its title in
   the left pane), or on the whole stack (its header).
3. **Ask the agent.** `Ask agent…` opens the ask dialog. Set the
   permission, what the agent may do, and the effort, how hard it works,
   then press `Ask Now`. Both start unset every time, so that no ask goes
   the way the last one went by mistake. The agent's progress shows under
   the comment while you go on working.
4. **Review what comes back.** When the agent is done, a banner under the
   top bar says that its work waits for your review. Esc goes to the
   banner, and Enter on the work opens its review. Mark each part accepted
   or rejected, then close the review: what you accepted is applied then,
   and not before.
5. **Move HEAD.** None of this touches your checkout. When you want the
   stack there, to build or run it, `Move HEAD…` on the stack's menu
   (Space on its header) moves your checkout's HEAD along the stack: ↑↓
   to the patch you want, then Enter.

Wherever you are, `?` says what you can do there.

### From the command line

The command line does much of the same, for scripts and quick looks:

```sh
stacked show bug123                                 # the patches, bottom first
stacked comment bug123 --patch 2 split this in two  # prints the comment's short id
stacked comments bug123                             # every comment: its id, where it is, its thread
stacked send bug123 4f1c2a --answer-only --wait     # ask the agent, and wait for its answer
stacked landed bug123                               # what waits for your review
stacked review bug123 --accept-all                  # accept it
stacked head bug123 --to 2                          # this checkout's HEAD at patch 2
stacked export bug123 | git am                      # the stack as mail, to apply anywhere
```

`stacked --help` lists the rest.

## The main ideas

### Stacks and patches

A stack is a named series of patches on a base commit; a patch is a
commit's message, author and diff. A stack only goes in the database once
its patches apply cleanly, each on the ones below it, so a move that would
not apply is refused, and stacked says why. The check uses an index of its
own: your working tree, your index and the repository's objects are never
touched.

You can move and drop patches, edit a patch in your `$EDITOR` (as mail, or
one of its files as the patch leaves it), or edit the whole stack as a
list, as `git rebase -i` does. Every edit makes a new state of the stack.
Undo and redo move between the states, and the tree of edits keeps every
branch, so nothing is lost. What you edit in `$EDITOR` waits for your
review, as the agent's work does; moves, drops, undo and redo apply at
once.

### Comments and threads

A comment starts a thread. You, the agent and the reviewers on Phabricator
each add turns to it, each in a colour of their own, written in markdown.
You can reply, write any turn again (to trim the agent's answer down to
what you need, say), rewind a conversation, attach references (other
comments, lines of a file, web links), and close a comment once you are
done with it. Closed comments are not lost: `Filter comments…` shows them,
and they can be reopened.

Where a comment is says what it is about, and so what the agent may
change. A comment on a patch, or on one of its lines, is about that patch:
the agent may change that patch alone, and split it, unless you ask it of
the whole stack. A comment on the stack lets the agent change the whole
stack: rebase it, reorder it, add patches.

### Asking the agent

The ask dialog sets two things every time. The permission is one of:

- **answer only**: the agent reads the code and the stack, and may read the
  web with curl, but changes nothing: the tools it is given only read. It
  may open comments of its own on lines, as a reviewer would.
- **change patch**: it may change the patch the comment is on, and no
  other.
- **change stack**: it may change any patch, add patches anywhere and
  reorder them.
- **build and test**: it may run any command, in a worktree kept for
  builds, and says what it found; it changes nothing in the stack.
- **try push**, with the `try-push` feature: it selects tasks of Firefox's
  CI for a try push (below).

The dialog offers only those that fit where the comment is. The effort is
claude's `--effort`: low, medium, high, xhigh or max. `Ask Later`, in
place of `Ask Now`, puts the ask in the stack's batch, and the banner
sends the batch when you are ready: several comments to one agent, or the
answers to an agent each.

Every ask runs in a git worktree of the stack's own, under `.stacked/` at
the top of your repository (out of `git status`'s sight), never in your
checkout. The agent runs in the background: quitting the editor leaves it
working, and what it brings back lands on its own. `Runs…`, at the end of
every menu, lists every ask with what the agent wrote, and
`Stop the agent…` stops one.

What each kind of ask may use is stacked's to give and yours to add to.
`stacked setup agent` shows one list of tools with a box for each kind of
ask (`--tui` shows it on a screen of its own). You need not run it first:
when the agent is refused a tool, the refusal becomes an item in the
review of its work, where you can leave it refused, allow it for this
work, or allow it from now on (saved in the checkout's
`.stacked/local.toml`), and send the work back for the agent to try again.

### Reviewing the agent's work

What an ask brings back waits in a queue, the first landed first, with
the reviews pulled from Phabricator. A review lists its parts on the
left: the stack operations of its change (each patch added, modified,
reworded, moved or removed), the comments it opened, and its replies. The
part under the cursor shows on the right as it would be, a change as the
diff of the stack it makes, each new version of a patch compared with the
one before. Enter accepts a part, and Space has the rest, `Mark as
rejected` among them. Esc then offers to close the review, which applies
what you accepted, or to save your marks for later.

Each stack operation is marked on its own, so you can accept part of a
change: the stack then takes the operations you accepted, and keeps the
other patches as they were. Rejecting an operation is refused while
another one still needs it, its patch not applying without it, and the
status line says which one to reject first. What each operation needs is
worked out once, as the work lands, so each mark is decided at once;
closing a review in part checks the stack you leave before anything
changes. Work that landed before stacked worked that out is reviewed as a
whole, as its review says.

Until the review of a change closes, you cannot edit the stack by hand. A
reply that came alone needs no review screen: Enter on its comment takes
it into the thread.

If you write in a review, a comment on a line of the work or an answer to
one of its replies, the work is neither accepted nor rejected: Esc offers
`Send it back to the agent…`. With a permission that changes the stack,
the agent tries again, on the stack the work makes, and its new work
takes the old one's place; with one that only reads, it answers a
follow-up, which comes into the same review.

### Chained edits

A change does not wait for you to review the change before it. It starts
at once, from the stack that change makes, as if you had accepted it, and
its work is reviewed after that one's. If you reject the earlier work, or
send it back, what went on from it is dropped, and asked again from the
stack as it then is. Accepting a change also asks what becomes of the
answers made on the stack before it: keep each, restart it on the new
stack, or stop it. `stacked config agent.chain false` turns chaining off.

### HEAD and your checkout

When stacked runs in a checkout of the stack's repository, the left pane
shows where that checkout's HEAD is against the stack (`▶ HEAD main`). A
commit whose message differs from its patch's is marked `≠`; one that
names another Phabricator revision than its patch is marked `!`, and HEAD
never moves below it. `Reconcile with HEAD` takes those messages into the
stack, as an edit for your review.

`Move HEAD…` detaches HEAD at the stack's base, with the patches up to the
one you pick applied as commits, and leaves the branch it was on where it
was; `stacked head NAME --to N` does the same from the command line. It
never replaces commits of yours: it moves HEAD only while HEAD is on the
stack (its commits the stack's patches, with their messages, and nothing
above them) or exactly where stacked last left it, and while no file git
tracks has changes. When it cannot, `Move HEAD…` shows why, and the git
commands you can run so that it can. Once you accept a change, its review
can also move HEAD to the new top of the stack, when HEAD was in sync with
it.

### The database

Everything lives in one file, one JSON object per line: the patches,
every state of every stack, the comments, the agent's runs and what it
wrote, the read marks, and what was read from Phabricator. A line once
written is never changed, the header aside, so a crash can only cut the
last line short. Each record is named by its place in the file, its rev,
such as `@12`. Several editors and commands can share the file at once:
each write holds its lock.

The editor shows the history in `History…`, `Tree of edits…` and
`Versions of this patch…`; `stacked log`, `tree`, `versions` and
`interdiff` say the same on the command line. The file only grows:
`stacked trim NEW --from @N` writes a new, shorter one, the history before
`@N` left out. A stack whose base commit is not in the repository (the
database copied to another machine, say) is read-only until
`stacked base NAME REV` gives it a base.

## Three front ends

The terminal, the browser and StackED run the same editor, with the same
keys.

- **The terminal**: `stacked`, or `stacked FILE`.
- **A browser**: `stacked web` serves the editor on this machine and opens
  your browser on it. It listens on 127.0.0.1 only, and only the page with
  the token in the address it opened can see it or drive it. `--port N`
  picks the port, and `--no-open` only says where it is. Without
  `--port`, the setting `web.port` picks it: `"random"`, any free port each
  time (unless set), `"fixed-random"`, any free port the first time, which
  stacked writes in your settings and listens on every time after, or a
  port number; a port in use is said, and `stacked web` does not start.
  With `"fixed-random"` or a port number, the next launch gives the tab
  already open 2 seconds to come back before it opens a new one, and
  either way, closing the editor's last tab quits `stacked web`.
  While you are not looking at the page, it shows your system's banners
  when work of any stack lands for your review, a run fails or a try push
  is done, if you let the browser show them when it asks, once
  (`web.notifications = false` turns them off), and rings a soft bell,
  once you have clicked or typed in the page (`web.bell = false` turns it
  off).
  A terminal `$EDITOR` opens in the page itself, in a terminal of `stacked
  web`'s own that gets every key you type there but those with ⌘
  (`web.editor = "terminal"` opens it in the terminal where you started
  `stacked web` instead); an editor with a window of its own opens that
  window.
- **StackED**: `stacked app` opens the editor in a window of its own, with
  no terminal and no browser. Its File menu opens more databases, each in a
  window of its own, and remembers the ones opened last. `$EDITOR` opens in
  the terminal StackED was started from, or, opened as an app, in a
  terminal window of its own.

In the browser and in StackED the mouse works too: a click picks or opens
what is under it, a right click opens its menu, and the wheel scrolls the
pane under it. After a key, the wheel moves the cursor, as the arrows do,
until the mouse moves again. Dragging HEAD's line onto a patch, and
letting go there, moves your checkout's HEAD there, as `Move HEAD…` does.

## Phabricator

With the `moz-phab` feature, stacked reads the revision each patch's
`Differential Revision:` trailer names, over Conduit, with the token
moz-phab (or arc) keeps in `~/.arcrc`; curl makes the calls.

stacked reads Phabricator only when you ask, never by itself:
`Fetch the latest from Phabricator`, on the stack's menu or in a patch's
`Phabricator` menu, reads every revision of the stack and stores what it
found in the database; a pull reads it too, and a submit what the
database lacks of its revision. Each patch then
shows its revision's status after its number, a letter (`[R]` Needs
Review, `[A]` Accepted, `[W]` Changes Planned, and so on), then `✓` when
Phabricator has this exact version of the patch, or `✗` when something
differs; or `↻` once you have submitted it since it was last fetched, as
what is shown of it is from before your submit, until you fetch it again.
A line over the patch's message says it in full, and how long ago it was
fetched:

```
Phabricator: D123456 · Needs Review · the diff differs · 2 new comments to pull · fetched 2 hours ago
```

Space on that line shows what differs, and Enter opens its menu.

A patch whose revision is Accepted or Landed, as last fetched, should not
change: whatever would give it another version (an undo or a redo, a
version put back, a review closed, an edit by hand, asked about before
`$EDITOR` opens) opens a dialog first, which names each such patch and its
status, and changes them only once you type `accept` there; on the command
line, stacked asks on the terminal, and with no terminal it refuses. Moving
or dropping such a patch asks nothing, and nor does a pull that brings it
to Phabricator's own latest diff.

- **Pulling.** `Pull from Phabricator`, on the stack's menu, or `stacked
  moz-phab pull NAME`, brings in what changed there: each revision's
  latest diff, title and summary that are no version of its patch stacked
  knows, as new versions; the open revisions the stack stacks on and does
  not hold, and those stacked on its top, as new patches; and the
  reviewers' new comments, replies and edits. All of it waits for your
  review, as the agent's work does, each operation of the stack accepted
  or rejected on its own; a patch above that no longer applies is made
  again with its conflicts marked, to resolve, to reject, or to send back
  to the agent. Closing that review files the comments as threads, to
  reply to or to ask the agent about. A reviewer's suggested edit comes
  with their comment, as a diff of the lines it replaces, from a
  Phabricator that gives suggested edits, as Mozilla's does; from one that
  does not, a comment that is a suggested edit alone comes in saying so,
  with its link. `Pull D123 from Phabricator`, in a
  patch's `Phabricator` menu, pulls that patch's revision with its
  parents, or with those stacked on it too.
- **A new stack.** `Pull from Phabricator…`, under `New stack…`, or
  `stacked moz-phab import D123`, pulls a revision, with the revisions it
  stacks on and those stacked on it, into a new stack, for your review.
- **Submitting.** `Submit to Phabricator as WIP`, in a patch's
  `Phabricator` menu, sends that one patch as work in progress (Changes
  Planned, which asks no one for a review), once you have seen what it
  sends. What you see is worked out from what was last fetched of the
  revision, and says how long ago that was (`fetched 11 minutes ago`):
  nothing is read again before it is sent, nor after, so fetch the latest
  first when that matters; once it is sent, the revision is marked `↻`
  until you fetch it again. It is sent in the writes moz-phab makes, four
  at most. For a revision already in review, the menu offers to update its
  title, summary and parent alone. Sending a whole stack for review, with
  its reviewers, is still moz-phab's work: put the stack in your checkout
  with `Move HEAD…`, and run moz-phab there.

## Try push

With the `try-push` feature, a comment on the stack, or on a patch as a
whole, can ask for a try push of Firefox's CI in plain words: "Run the
tests a.txt touches, on Linux and Windows". Asked with the `try push`
permission, the agent selects the tasks, each by its exact label and with
why it matters; it pushes nothing itself. The tasks wait for your review,
to accept or reject each. Closing the review pushes the accepted ones to
try with `./mach try fuzzy`, from a worktree of the stack's own, once a
run with `--no-push` has checked that mach selects exactly those. The
push's link comes back as a reply to the comment, stacked follows the push
on Lando and Treeherder, and a summary of its results lands for your
review once its tasks are done.

## Linting

`Lint the stack`, on the stack's menu, runs your checkout's linter over the
stack, patch by patch from the bottom, in a worktree of the stack's own.
In a Firefox checkout, the linter is `./mach lint --fix`; elsewhere,
`stacked setup lint COMMAND` sets one. Each fix goes into the patch it
belongs to. The fixes that cannot go in without keeping a patch above from
applying go in a patch of their own on top, with a comment, ready to send,
that asks the agent to fold them in. All of it lands for your review, as
an edit does.

## Keys

| Key | What it does |
|-----|--------------|
| ↑ ↓ | move through the patches, the lines of a patch, the comments; pgup, pgdn, home and end go further |
| ← → | go from pane to pane |
| Enter | the main thing to do where the cursor is: read a patch, comment on a line, reply to a comment |
| Space | the menu: everything that can be done where the cursor is |
| `?` | say what can be done here |
| Esc | go back; in the panes, go to the banner of what waits for you |
| Delete or Backspace | close, drop, stop or reject what the cursor is on (it asks first) |
| `1` | pick a view of the code: unified, side by side, compact, or unified split in two; where interdiffs are shown, a dialog sets what is compared, the two diffs or the code (the change from the old version to the new one as a patch of its own), how it is drawn (as an interdiff, in columns or side by side; the code unified, side by side or compact), and whether every line is drawn or only what the patches changed, the context, the index lines and what a change of base made left out, each place saying how many |
| `2` | switch between commenting and reading, where Space marks code read |
| `3` | switch between the hunks and each file whole |
| `/` | search what the middle shows; Tab and shift-Tab go from match to match |
| `o` | open the link on a line of a commit message |
| ctrl-c | quit |

The settings' `[keys]` can make any of them other keys; `?` and the bottom
bar then name yours.

## Settings

The settings are in `~/.stacked/config.toml`, and a checkout's own
`.stacked/local.toml` overrides them in that checkout. `stacked config`
reads and writes them, checks each value before it writes it, and keeps
the rest of the file as it was, comments too:

```sh
stacked config                      # every setting, as it is, and what is wrong with the file
stacked config editor hx            # write one
stacked config editor               # read one
stacked config --unset editor       # take it out
stacked config --local editor vim   # write one in this checkout's own settings
```

The main ones:

- `file`, the database opened when none is named, and `editor`, the editor
  for comments, patches and code (`hx {file}:{line}`; else `$VISUAL`,
  `$EDITOR`, then vi).
- `[agent]`: the program run as the agent and its arguments, the tools each
  kind of ask may use, where the agents' worktrees go, and `chain`.
- `[reviews]`: Phabricator's address, token and trailer, and Bugzilla's
  address; `source = "none"` turns Phabricator off.
- `[view]`: the layout and the panes at the start, the view of the
  interdiffs, the reading mode, and the width long message lines are
  wrapped at.
- `[colors]`: every colour of the screen, written as git writes colours
  (`stacked config colors.you 208`).
- `[keys]`: the keys of each thing the editor does.
- `[lint]`: the checkout's linter, which `stacked setup lint` writes.

`$STACKED_HOME` names another folder than ~/.stacked, and
`$STACKED_CONFIG` another settings file.

## Building and testing

```sh
cargo build                        # every feature: the web feature needs the wasm target
cargo test                         # every test, of every feature
cargo test --no-default-features   # stacked with no feature: no wasm target needed
```

No test runs claude, a linter of yours or `./mach`, and none reaches
Phabricator or the network: stand-ins answer for them. No test touches
your own `~/.stacked` either.

The full check of a change is clippy over five feature sets, then the test
suites over the same five, and once more with `STACKED_TEST_CHECKPOINTS=1`,
which writes a checkpoint at every write and checks each database read
from a checkpoint against the same database read whole:

```sh
cargo clippy --all-targets --no-default-features --features web,demo,moz-phab,try-push
cargo clippy --all-targets --no-default-features --features app
cargo clippy --all-targets --no-default-features
cargo clippy --all-targets --no-default-features --features try-push
cargo clippy --all-targets --no-default-features --features web,demo,moz-phab,app,try-push

cargo test --no-default-features --features web,demo,moz-phab,try-push
STACKED_TEST_CHECKPOINTS=1 cargo test --no-default-features --features web,demo,moz-phab,try-push
cargo test --no-default-features
cargo test --no-default-features --features try-push
cargo test --no-default-features --features web,demo,moz-phab,app,try-push
cargo test --no-default-features --features app
```

## The full specification

This README is an introduction. The full specification of stacked, every
behaviour, every text the screen shows, every setting, the database's
format, the layout of the code and what each test checks, is in
[docs/ai/](docs/ai/README.md). It is written for the AI agents that work
on stacked, and they keep it up to date with every change, so it is long
and exact rather than easy to read: start here, and go there for the exact
rule.

## License

MIT: see [LICENSE](LICENSE).
