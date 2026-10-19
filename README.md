# stacked

Usage notes on `stacked`, a stack editor, that do not belong in its
`--help`. Run `cargo test` in `stacked/` after any change to the tool. Press
`?` in the editor: it says what can be done where the cursor is.

## Install

```sh
cargo install --path stacked
```

or run it in place with `stacked/target/release/stacked` after
`cargo build --release` in `stacked/`. It needs `git`, an agent for asks
(`claude`, unless the settings name another), and `curl` for Phabricator.

With the editor in a browser too (`stacked web`, below), built with the
`web` feature; its browser half is compiled to wasm, with Rust's
wasm32-unknown-unknown target:

```sh
rustup target add wasm32-unknown-unknown
cargo install --path stacked --features web
```

With a tour of it, to learn it by (`stacked demo`, below), built with the
`demo` feature: `cargo install --path stacked --features demo` (or
`--features web,demo` for both).

## Running

```sh
stacked import bug123 main..HEAD   # the commits main..HEAD as stack bug123
git format-patch --stdout main | stacked import bug123 --base main   # or mail
stacked new bug124 --base main     # an empty stack, to add patches to
stacked stacks.jsonl               # open the editor on that database
stacked                            # ... on $STACKED_FILE, the settings' file, else ./stacked.jsonl
stacked show bug123                # its patches, bottom first
stacked edit bug123                # as `git rebase -i`: reorder, drop, reword, edit
stacked edit bug123 2 --path a.txt --line 12   # a file as patch 2 leaves it, at line 12
stacked edit bug123 2 --path a.txt --in worktree   # ... in the stack's own worktree, .stacked/bug123
stacked edit bug123 2 --line 30    # patch 2 as mail, at line 30
stacked comment bug123 rebase onto origin/main        # a comment on the stack
stacked comment bug123 --patch 2 split this in two    # on a patch as a whole
stacked comment bug123 --patch 2 --line a.txt:+TWO why?   # on one of its lines
stacked comments bug123            # every comment: short id, where, state, thread
stacked send bug123 4f1c2a --wait  # the agent works on it; its answer lands in the thread
stacked send bug123 --all --answer-only   # every comment that can be sent
stacked reply bug123 4f1c2a and keep the title
stacked pull bug123                # reviewers' comments from Phabricator
stacked apply bug123 --branch bug123   # only now does the worktree change
stacked export bug123 | git am     # or put it anywhere by hand
stacked trim new.jsonl --from @40  # a new database, the history before @40 left out
stacked config > ~/.stacked.toml   # every setting, to start a settings file from
```

`-f FILE` points any command at a database. A first argument that is not a
command is the database to open: `stacked FILE` is `stacked open FILE`.

## A tour

`stacked demo` (built with `--features demo`) teaches the editor with a game
of tic-tac-toe played in a file, board.txt, one patch per move. It starts
from an empty board and an empty stack, and plays five moves as patches.
Each patch shows the whole board, so the moves before it can be seen. O's
fourth move is a mistake, and X's fifth move wins because of it. The tour
has 13 sections, the most useful first, and takes about fifteen minutes:

1. Read a stack and its patches.
2. Comment on a line: O's mistake.
3. Ask the agent to change the code: asked about the whole stack, because
   the fix changes X's winning move too, a stand-in for the agent rewrites
   both moves (O blocks X, then X blocks O), and you can watch it write.
   Asked about patch 4 alone, the stand-in says that the fix needs more
   than that patch, and your comment moves to the stack.
4. Reply to the agent with a question, asked answer only.
5. Keep track of what you have read, in reading.
6. Switch how the code is drawn: unified, side by side, compact.
7. The history of the stack, and the versions of a patch.
8. Move a patch: stacked refuses, because each patch only applies to the
   board that the patches before it leave; Esc stops moving it.
9. Drop a patch, undo the drop, and see the tree of edits.
10. Ask the agent to add a patch: a comment on the stack asks for O's next
    move, and the stand-in adds it as patch 6.
11. See what the agent did, in Runs….
12. Where help is (`?`), and how to change the keys.
13. Start on a project of yours. At the end, the tour shows the board as
    the stack's patches leave it.

Each step says what to press, and why, and waits for you to do it: Tab does
the step for you, and ctrl-c leaves the tour. The first time it is opened,
the tour writes `[demo] seen = true` in the settings file (below), the rest
of the file as it was. From then on it starts on the list of its sections:
↑↓ pick one and Enter starts it, on a new game played up to there, the steps
before it done for you in a moment. Esc starts at the beginning. During
that tour, shift-Tab lists the sections again, to go to any of them.

Apart from that setting, it writes nothing on disk: its database and its
repository are kept in memory, and its patches are applied there as `git
apply` would apply them. Anything that would write on disk (`$EDITOR`,
apply, trim) or reach Phabricator says that it is not in the demo.

## In a browser

`stacked web` (built with `--features web`) serves the editor on this
machine and opens the browser on it: the same editor, the same keys, drawn
in the page as in a terminal. The cursor follows the mouse: what is under it
is picked, as the arrows would (a patch of the list, a line, a comment, an
item of a list or of a menu; a patch being moved goes there), so a click is
Enter on it, and a right click Space, its menu. Outside a menu, a click
closes it; writing a comment, or reading what `?` says, the mouse moves
nothing. The wheel is ↑↓, and a paste types what is pasted, into a comment
being written. `--port N` listens on port N
(else any free one), `--no-open` only says where it is.

It listens on 127.0.0.1 only, and only a page holding the token in the
address it opened (a new one each time) may see it or drive it: a page from
elsewhere could otherwise send it keys. The editor runs where `stacked web`
was started: `$EDITOR`, for what is edited in it, opens there. Quit is
quit, the page saying so; ctrl-c there stops it too.

The server runs the editor as the terminal does, drawn into a buffer of the
page's size, and sends the page the rows that changed (server-sent events);
the page sends its keys and clicks back. What the page does is wasm
(`web/client.rs`: the rows made HTML, the keys and clicks made lines for
the server), carried by fifty lines of JS (`web/index.html`).

## The editor

The left pane is the current stack: its header (`≡ bug123 · 3 patches`),
which is the stack's own place, then the patches' titles, bottom first, with
how many comments wait on an answer. The middle shows what the left pane's
cursor is on: a patch (its message, then its diff, the comments under the
lines they are on), or, on the header, the stack itself (its patches, then
the comments on the stack). The right pane lists the comments the middle
shows, under a header of its own; with none, it is not there.

There are five keys, and two of self-review's; the settings can make any of
them other keys, and the help then names those:

- The arrows move. ↑↓ go through the patches (the header is above the
  first), the lines of the patch, or the comments (their header above the
  first); pgup, pgdn, home and end go further. ← and → go from pane to
  pane: ← on the patches hides them, for the middle to have the width, and
  ← again brings them back; → on the comments hides them the same way, the
  bar saying how many there are.
- Enter does what is to be done where the cursor is: on a patch, read it; on
  a line, comment on it; on a comment, reply, or, while the agent works on
  it, open what it is writing; in the comments pane, go to the comment.
  Elsewhere it is the menu.
- Space is the menu: all that can be done where the cursor is, then undo,
  redo, `Runs…` and quit, which every menu ends with. The stack's items
  (comment on it, ask about all, `Show…`, the stack as a list, the tree,
  the history, Phabricator, apply, switch, delete) are on the left header
  only; a patch's on a patch; a line's on a line; a comment's on a comment.
  ↑↓ and enter; ↑ from the top is Quit. What a read-only stack cannot do is
  shown dimmed, and the menu says why. An item may have choices, radio
  buttons after its label (`[x] tmp/file  [ ] pwd`): ← → pick one, a click
  on one picks it, and Enter does the item with the one picked, which is
  kept for the stack in the database.
- `?` says what can be done where the cursor is, as `Press Enter to create a
  comment on this line.`, and where the arrows go. The next key closes it
  and does what it does, so the key it names can be pressed straight away.
- Esc goes back: out of a menu, a list, a stream.
- `1` cycles the views of the code, as in self-review: unified, side by
  side, and compact, the two sides each a pane of its own with no gaps,
  scrolled together or one alone (← → turn which, and past the old side or
  the new one go on to the pane beyond). Two sides need 60 columns.
- `2` switches between commenting and reading (below).

Nothing else is a key. Going through the comments pane does not move the
patch: the comment is marked where it is if the patch shows it, else its
thread is shown over the top or the bottom of the patch, as it is above or
below. Enter goes to it.

Moving a patch is ↑↓ to place it, enter to put it there, after the menu's
`Move this patch…`. If the patch does not apply at that place, stacked
refuses, and the patch is still being moved: the status line says why, and
that ↑↓ place it elsewhere and Esc leaves it where it was. Dropping a patch, deleting a stack or a comment,
applying, and stopping an ask ask first, in a menu of their own; for what
cannot be undone, the menu starts on `Keep it`. ctrl-c quits, as it does
everywhere.

The screen never waits on work done elsewhere: an ask is started (its
worktree made) away from it, and so are pulls, applies, a new database
written, and the check of a stack someone else made; the bar says
`pulling…`, `applying…`, `checking…` meanwhile, and what came of it when it
is done. While the agent writes, only what it writes is drawn again.

## Writing comments

Comments are written where they will show, in an editor inline: a comment
on the stack under the left pane's header, on a patch under its title in
the left pane, on a line under the line, a reply under its comment, in the
middle or in the comments pane. The pane being written in takes half the
width meanwhile. Typing and the arrows edit, enter breaks the line, and esc
asks what to do with it: `Save it`, `Save it and ask agent`, `Throw it
away`, `Keep writing`. How the agent is asked is the choice of `Save it and
ask agent`, `[x] change  [ ] the whole stack  [ ] answer only  [ ] build and
test` (the whole stack for a patch's comment, build and test for the
stack's or a patch's as a whole), as it is of a comment's `Ask agent`: one
pick, kept for the stack. Esc with nothing written just closes it. Where a
comment is written is a choice too, kept for the stack: `Comment on this
line`, `Reply` and `Edit your reply` each have `[x] here  [ ] in $EDITOR`.
Nothing is typed at the bottom: the bottom line only says things.

When the last word of a comment is yours, enter on it opens what you said
last, in place, to edit or add to (a reply would only add to it), the turns
before it as they were; enter on its line does too. Its menu has the same
first, `Edit your reply` (`Edit your comment` when no one has answered it),
here or in $EDITOR; `Reply` adds to it without showing it. Not while the
agent works on it. A comment waiting its turn
leaves the agent's queue while it is edited, so that it is not asked as it
was: thrown away, or left as it was, it goes back on; edited, it waits to
be asked again (or `Save it and ask agent`). In $EDITOR it is only what you
said, nothing dropped, a line starting with `#` as any. The comment's
history says `you edited it`. On the command line: `stacked reply NAME ID
--edit TEXT`.

## Editing a patch by hand

On a line of code, the menu's `Edit a.txt here in $EDITOR` opens the file
as the patch leaves it, at that line. Where it is edited is the item's
choice, `[x] tmp/file  [ ] pwd  [ ] .stacked/NAME`, kept for the stack:

- `tmp/file`: a copy of the file alone, in a temporary folder. It is quick,
  but the editor sees nothing around it: no project for a language server's
  go to definition. What is saved makes the patch's part for that file again
  (the file before the patch and as saved, diffed by git), the rest of the
  patch as it was.
- `pwd`: the checkout stacked runs in, when it is one of the stack's
  repository and nothing it tracks has changes. It is put at the stack's
  base with the patches up to this one applied (as commits, not signed),
  the editor runs there, and then the checkout goes back to the branch or
  commit it was on. The editor, its language server, and your build there
  see the whole project as the patch leaves it.
- `.stacked/NAME`: the stack's own worktree, at the top of its repository,
  made the first time and kept (`.stacked/` goes in the repository's
  `info/exclude`, out of `git status`'s sight). Each edit moves it to its
  patch; your checkout is not touched. The first one checks out every file;
  the next ones only what differs.

In a checkout, what the edit changes there is the patch's new version: the
files git tracks, and the files new since the edit started (files git
ignores, and the ones that were there before, are left out). Its message
and author stay as they were. Closed without saving, nothing changes. The
new version goes in once the stack is seen to apply with it. If the patches
above no longer apply on it, it is not taken, and nothing is lost: `Edit it
again` opens it as you left it, or `Throw the edit away`. On the command
line, `stacked edit NAME PATCH --path FILE --line N` does the same, `--in
tmp|pwd|worktree` saying where (else where the stack's menu last picked); an
edit not taken is kept in a file (`tmp`) or a commit, and `--from FILE` or
`--from COMMIT` takes it up again.

`Edit this patch in $EDITOR`, there or on the patch, opens the patch as
mail at the line the cursor is on: its message, its diff. It comes back
with why at its top if it does not apply, as an edit by hand does.

The editor is told the line: `+LINE` for vi, vim, nvim, nano, emacs or kak,
`FILE:LINE` for hx, zed or subl, `--goto FILE:LINE` for code; any other is
given the file alone, unless the settings' `editor` says where `{file}` and
`{line}` go.

## Asking the agent

The agent (claude, unless the settings name another) only works on
comments: write one where it belongs, read it again, then ask. Where a
comment is, is what the agent may change:

- A comment on the stack: the whole stack. Rebasing, reviewing, testing and
  adding patches are stack comments: `rebase onto origin/main`, asked.
- A comment on a patch, as a whole or on a line: only that patch, which the
  agent may split in place. Its worktree is the stack up to that patch;
  the patches above it, it may read with `stacked export`, not change.
  Unless it is asked of the whole stack (below): when you know already that
  the answer needs more than the patch (another patch changed, a new one on
  top), as a comment on the stack would.

A comment's menu asks with `Ask agent`, and `Ask agent about all` on the
left header (the stack's comments) or the comments pane's header (what the
list shows) asks about each comment listed. How it asks is the item's
choice, picked with ← →, kept for the stack, and the same as `Save it and
ask agent`'s:

- `change`: it may change the code within the comment's place; its reply
  says what changed.
- `the whole stack`, on a patch's comment: a change, with the whole stack in
  its worktree, as for the stack's own comments: it may change any patch,
  add patches anywhere, reorder them. The comment stays on its line, and
  its reply lands in its thread. On the command line, `stacked send NAME ID
  --whole-stack`.
- `answer only`: read-only, enforced by its tools; only the reply lands. An
  answer may open comments of its own on lines within the place, as a
  review does: the asking comment then says `opened 4 comments, 1 with your
  replies`, and its menu goes to them, or deletes it and them.
- `build and test`, on the stack's comments and a patch's as a whole: it may
  run any command, in a workspace of its choosing by its own rules
  (CLAUDE.md), never the user's worktree; it changes nothing in the stack.
  What it found lands in the reply and on each patch's title, saying which
  stack it tested.

Several go at once, by self-review's rule: one changing ask at a time on a
stack, answer-only asks together, a build and test once no change runs, one
at a time; a change waiting holds back the answers asked after it. An asked
comment that cannot go yet waits as ready, says what it waits on, and goes
on its own when that lands. Nothing is asked on a stack that does not apply.

Changes are checked when they land, like any edit. A patch's change slots
into the stack as it is by then; if the patch itself was edited meanwhile,
the ask fails and the comment waits to be asked again. Before it finishes,
the agent runs `stacked fits`, which says whether the patches above still
apply on its change; if they would not, or the comment needs more than its
patch, it changes nothing and the comment moves to the stack's comments,
with the agent's why as its last turn and `moved from patch 2 · a.txt: TWO`
on its first line, to be asked again from there (asked of the whole stack
from the start, it would not have had to). The cursor goes with it if it
was on it, and the bottom line says so. `Go to where it came from`
and `Move it back to patch 2` are in its menu. Comments on lines follow a
split or merged patch to the piece that has their line. A stack's change
made from a stack you have edited since is a branch of the tree of edits,
and yours stays where it is. A rebase says onto what (the agent adds
`"base"` to its answer): the stack's base moves there.

Where it stands shows on the comment's first line, or under it when that is
too long: ready (and on what), with the agent (and the last lines of its
stream under it: what it says and runs, and while it thinks, one line
counting it, `✻ thinking · ~688 tokens`, with the words of it when claude
gives them), agent replied (changed, answered, tested), failed (and why),
moved. Enter on a comment the agent is working on opens its stream in
place; esc folds it. The comments pane marks each (`●` with the agent, `◌`
ready, `✓` replied, `✗` failed, `↑` moved), its header counts them (`4
comments · 1 with the agent · 2 to read`), and the top bar too (`agent ●2 ·
ready:1`). `Show…`, on either header, picks what both panes show, each with
its count: all, to send, to read (any, from an answer, from a change), in
progress, failed or moved, gone (on a line the patch no longer has), none.
It is kept per stack, in the file.

The commits made in the agent's worktree, stacked's and the agent's, are
not signed, whatever git's `commit.gpgsign` says: they are only read back
as patches. `stacked apply`'s commits are yours, signed as git says.

The agent runs in a process group of its own. Quitting the editor leaves it
running, and what it does lands on its own. A worktree with work the agent
did not commit, or that could not be filed, is left in place, and the run's
summary says where; others are removed once landed. `Runs…` lists every
ask, apply and pull, and shows what the agent wrote.

`STACKED_CLAUDE` names a program to run as the agent, over the settings'
`agent.command`. The tests use it. Whatever it is, it is given claude's
arguments (`-p PROMPT --output-format stream-json …`) and must answer as
claude does.

## Reading

`2` switches to reading, as self-review's view of what is left to review:
in the middle, Space marks the block of code at the cursor read and goes on
to the next line not read, and Enter marks it unread again (no menu in the
code meanwhile: the other panes still have theirs). A block is the
innermost block of code around the line with lines not read yet, from a
tree-sitter grammar (Rust, C, C++, Python, JavaScript, TypeScript, Java,
Go) or else the file's braces: on a line whose block is read, the block
around it, and once those are read, the hunk. A mark never reaches past the
hunk the cursor is in: a block over several hunks, as a long function or a
namespace is, is read in this hunk alone, the margin and the bar saying
`in this hunk`. Files with no blocks are read a hunk at a time; Space on a
hunk's header reads the hunk, on a file's the file. Enter takes back the
latest mark holding the line or its block, in this hunk. The
margin shows what Space would mark (`○→✓`), or else what Enter would unmark
(`✓→○`), and `✓` beside each line read, drawn dim; hunks, files, the
patches on the left and the top bar count what is read (`reading 5/11`).

Marks are records in the file, kept by the lines' content: a new version of
the patch keeps them on the lines it did not change, and what it changed is
to read again. The blocks are found from the files as each patch leaves
them, in an index of stacked's own (as the check does), in a thread: the
screen does not wait.

## History

The file holds every stack, every version of every patch, every turn of
every comment and every run; the editor shows them where they belong:

- `Tree of edits…`, on the stack's header: every state of the stack, the
  one being edited marked; enter edits from another. Beside each, its
  patches and what its edit changed; Space, `What this edit changed…` in
  full: its stack diff (below), from the stack it was edited from.
- `History…`, on the stack's header: what changed the stack, newest first:
  its edits, and the moves of its head (undo, redo, select, delete) that
  took it to another stack. Comments, runs and the rest change nothing of
  it and are left out; so is an ask that landed as a branch, the stack
  having moved on (the tree of edits has it). Each says what it changed in
  short (`+1` added, `−1` dropped, `~1` a new version, `↕1` moved), then
  what it was. Beside them, the tree of edits' lanes: each edit on its
  branch's lane, marked as the tree marks it (`●` where the stack is, `○`
  the others, `┘` where a branch leaves the state it was edited from), and
  the lanes going on past each move of the head, so which branch an edit
  is on, and where an undo jumped, can be read in the order it all
  happened. Enter goes to the stack it made, in the tree. Space, `Ask agent
  about this, answer only…` (below), or `Start a new database here…`
  (further below).

  ```
   ●     2026-10-17 19:50 @12 ~1 · edit e80b0801 as e35c91e0
   │     2026-10-17 19:50 @10 +1 · undo drop 1e346401 → @4
   │ ○   2026-10-17 19:50 @8 −1 · drop 1e346401
   │ │   2026-10-17 19:50 @7 ↕1 · undo move 1e346401 from 2 to 1 → @4
   │ │ ○ 2026-10-17 19:50 @6 ↕1 · move 1e346401 from 2 to 1
   ○ ┘ ┘ 2026-10-17 19:50 @4 +2 · import 2 patches
  ```

  Beside the history, the stack diff of the record picked, from the stack
  before it to the one after: the stack after, bottom first, each patch by
  its place and marked (`+` added, `~` a new version with what changed in
  its mail under it, `↕` moved, from where it was; unmarked, as it was),
  and each patch dropped, marked `−`, where it was. A patch is known across
  its versions, so one only shifted by what was added or dropped below it
  is not taken for moved. → goes into it, ↑↓ scroll it, ← goes back. It is
  only to look at: nothing is edited, commented on or asked from it, nor
  undone or redone from History…; esc leaves it, for the stack as it is.

  ```
   the stack @4 → @12
   edit e80b0801 as e35c91e0 · 2026-10-17 19:50
    1 ~ Bug 1 - parse skips blank lines, total does not overflow (reworded)
        @@ -1,6 +1,6 @@
         From: A <a@b>
         Date: Sat, 17 Oct 2026 19:50:52 +0200
        -Subject: Bug 1 - parse skips blank lines, total does not overflow
        +Subject: Bug 1 - parse skips blank lines, total does not overflow (reworded)
        …
        @@ -26,7 +26,7 @@
         +    let mut sum = 0u64;
              for i in items {
         -        sum += i;
        -+        sum += *i as u64;
        ++        sum += u64::from(*i);
              }
    2   Bug 2 - a readme
  ```
- `Versions of this patch…`, on a patch: every version, oldest first, each
  with the edit that first had it; beside it what changed from the version
  before, or the version whole. `Put this version back in the stack` is an
  edit like any.
- `History of this comment…`, on a comment: every state it was in, with the
  rev and time of each.

`stacked log`, `tree`, `versions`, `interdiff` and `runs` say the same on
the command line.

### Asking the agent about the history

On a record of `History…`, Space, `Ask agent about this, answer only…`
writes a question under it (`why was it dropped?`, `what changed since?`),
and esc asks it: answer only, so the agent changes nothing. It is told the
stack's history, oldest first, and that record in full: when, what it was,
from which stack to which, and its stack diff, each new version's
interdiff in it. It may run stacked's read-only commands to look further: `log`, `tree`, `export --rev @N`, `versions`, `interdiff`,
`show`, `comments`. Its answer lands in the thread, and it may open comments
of its own on lines of the stack as it is now, as any answer may.

The question is a comment on the stack, `about @7 −1 · drop …` among its
comments; in `History…` it shows under its record with where it stands, and
the record's menu goes to it. Its own menu only asks to answer, and goes
back to the record (`Go to @7 in the history`). On the command line:
`stacked comment NAME --about @7 why?`, then `stacked send NAME ID
--answer-only`.

## Trimming the history

`stacked trim NEW --from @N`, or `Start a new database here…` on an entry of
`History…`, writes a new database: the history from `@N` on, and before it,
as its start, what there was then. The database it is made from is only
read; NEW must not be there (the editor names it `db-from-N.jsonl`, by the
old one). Without `--from`, only what there is now is kept.

The start holds each stack as it was, and any earlier state a later record
names (the parent of a later edit, where a later undo goes, where an ask
started), the patches they need, the comments as they stood, the read marks
standing, each stack's filter, and the runs still going with what their
agent had written. Runs that had landed, earlier states no one names, the
earlier versions of comments and patches: that is what is left out. Every
record from `@N` on follows, its revs made the new file's. The new file is
written whole, then read back: every stack must be as in the old one, or
nothing is written.

## Settings

`~/.stacked.toml` (or the file `$STACKED_CONFIG` names) holds the settings;
each is optional, and `stacked config` prints them all as they are, in the
file's form, with what is wrong with the file:

- `file`: the database opened when none is named, nor `$STACKED_FILE`.
- `editor`: the editor, for comments, patches, the stack as a list and the
  code, with `{file}` and `{line}` where they go (`hx {file}:{line}`); else
  `$VISUAL`, `$EDITOR`, vi, told the line as each takes it.
- `[agent]`: `command`, the program run as the agent; `args`, added to
  every run; `model`; `worktrees`, where the asks' worktrees go.
- `[reviews]`: `source`, `"phabricator"` or `"none"` (no pulling); `url`,
  the Phabricator a bare `D123` trailer is on; `arcrc`, where the tokens
  are; `token`, one for `url` instead; `trailer`, the trailer naming a
  revision (`Differential Revision`).
- `[view]`: `layout` at the start (`unified`, `side-by-side`, `compact`);
  `reading`, to start reading; `patches` and `comments`, the panes shown at
  the start; `stream_lines`, the agent's lines under a comment; and
  `split_width`, the columns two sides need.
- `[keys]`: for each thing a key does (`up`, `down`, `left`, `right`,
  `page_up`, `page_down`, `home`, `end`, `enter`, `menu`, `help`, `back`,
  `view`, `read`, `quit`), its keys: `down = ["down", "j"]`, `menu = "m"`.
  The comment editor reads what is typed, whatever the keys.
- `[demo]`: `seen`, written by `stacked demo` the first time it is opened:
  the tour then starts on the list of its sections. `seen = false` shows
  the first tour again.

A file stacked cannot read, or with a setting it does not know, is not
used at all: the editor's bar says why, as every command does. A setting
with a wrong value keeps its default, and that is said too.

## Only patches that apply go in

A stack goes in the database only once its patches are seen to apply
cleanly, each on the ones before it, to its base: what an import, an add, a
move, a drop, an edit by hand, a todo list, a new base or the agent's edit
would make. If they do not, nothing is written and the error says which
patch failed and where (`patch 3 (...) does not apply to the patches below
it (patch failed: a.txt:12)`). Undo, redo and picking from the tree only go
to a stack that applies too. A patch with no diff is refused, as `git am`
cannot apply it.

An edit by hand that does not apply comes back in `$EDITOR` with why at its
top, as `git rebase -i` does: put it right, or empty the file to give up.
So does a todo list, with the patches already edited as you left them.

The check never touches the worktree, nor the repository's index or object
store. Each patch is applied with `git apply --cached` to an index of the
check's own, holding only the files the patches touch, and what it writes
goes to an object store of its own. On a repository of 300,000 files, a
move in a stack of ten patches takes 0.2s, and one refused 0.4s: a failure
is checked again against the whole base before it is believed.

A stack with no base (a stack from before the check), or whose repository
or base commit is not there (the database copied to another machine, or
the repository moved), cannot be checked, and so is read-only: comments can
still be added, Phabricator pulled, the patches read and exported, but
nothing changes the stack and the agent is asked nothing. The editor's bar
says why, in yellow. `stacked base NAME REV`, from the repository, gives it
a base it applies to, and it can change again. `stacked check` says, for
each stack, whether it applies.

## One file, only appended to

Everything is in the database, one JSON object per line, and a line once
written is never changed: patches, every state of every stack, which state
each stack is on, comments and their replies, every run (an ask, an apply,
a pull) with what came of it, every line the agent wrote while it worked,
what each stack's comments pane shows, and what is read. A record is named
by its line number, its rev: `@12`.

- A patch is stored once, by the sha1 of its content. An edited patch is a
  new patch with the same change, so `versions` and `interdiff` can follow a
  patch through its versions.
- A stack record is the whole stack after an edit, with the stack it was
  edited from as its parent. A head record moves a name to another stack:
  undo, redo, a stack picked from the tree, a delete.
- A comment record is the whole comment as it is now; its earlier records
  are its history. The agent's turns are fenced ```` ```agent ````
  (```` ```claude ````, as the first files have them, reads the same). A
  read mark holds the lines it read; taking it back is a record naming it.
- A crash can only cut the last line short. A line that cannot be read is
  passed over with a warning (`stacked check` lists them) and keeps its line
  number, so every rev after it stays the same. The next write ends a cut
  line with a newline; nothing is ever cut off the file.
- Every write holds the file's lock (flock), so two editors, the CLI and
  landing asks can share it.

The file only grows; `stacked trim` makes a new, shorter one. An ask's
events include the agent's tool output, so a long ask can add a few
megabytes.

## The tree of edits

Every edit makes a new stack whose parent is the stack edited. Undo moves
back to the parent, and redo forward to the child you came back from. An
edit made after an undo starts a branch, and the old branch stays:

```
$ stacked tree bug123
● @10   2026-10-16 22:31   3 patches   drop ae318fd6
│ ○ @8  2026-10-16 22:31   4 patches   move f32a1147 from 2 to 1
○ ┘ @6  2026-10-16 22:31   4 patches   import 4 patches
```

`select @8`, or enter on it in the editor's `Tree of edits…`, goes back to
the other branch. Deleting a stack moves its name to nothing, and `undo`
brings it back.

## Phabricator

`pull`, or the menu's `Pull reviews from Phabricator`, reads the revision in
each patch's `Differential Revision:` trailer (the settings can name
another) over Conduit, with the token moz-phab keeps in `~/.arcrc`, or the
settings'. Each thread is a comment on its line, or on the patch's title for
one on the whole revision. A reply is a turn of its thread. Pulling again
adds only what is new. A reviewer's comment can be asked about like any.

## Applying

`apply`, or the menu's `Apply the stack to the worktree…`, is the only thing
that changes a worktree. It checks out the stack's base, detached or on
`--branch` (made, or reset there), then `git am`s the patches. It refuses if
anything is uncommitted, and if a patch does not apply it aborts and leaves
the worktree at the base. Like every run, it lands in the database. The
editor asks in plain words before it applies, on no branch; a branch is the
command line's.

## Copied from self-review-rs

stacked does not depend on self-review-rs. It has its own copies of
`diff.rs` (lines keyed by their text, so a comment, or a read mark, stays on
a line the next version keeps), `claude.rs` (allowed tools, reading replies
and stream events) and `blocks.rs` (code blocks, from tree-sitter grammars
or braces). It also takes the Conduit half of `phab.rs`, the thread fences
of `thread.rs`, the side-by-side pairing of `rows.rs`, the compact layout of
`nav.rs` (`panes.rs`), the reading view's marks of `review.rs`
(`reading.rs`), the inline editor (`editor.rs`), the rule for which asks go
together, and the filter of the comments pane. A fix to one may belong in
the other.

## Layout of the code

- `db.rs`: the file, its records and its lock.
- `check.rs`: whether patches apply, before they go in; the files as a
  patch leaves them.
- `ops.rs`: edits to a stack, `$EDITOR` (the patch as mail, its code), apply.
- `ask.rs`: asking the agent about comments, from the queue to landing.
- `review.rs`, `phab.rs`: Phabricator.
- `mbox.rs`: reading `git format-patch` mail, writing mail for `git am`.
- `rows.rs`, `view.rs`, `diff.rs`, `thread.rs`: the patch or the stack as
  rows, line keys, comment threads.
- `panes.rs`: the compact layout's two panes.
- `reading.rs`, `blocks.rs`: the reading mode's marks, by blocks of code.
- `tree.rs`: the tree of edits as a graph, and the lanes the history draws.
- `history.rs`: a stack's history, what changed it, and stack diffs: as the
  editor shows them and the agent is told them.
- `trim.rs`: a new database, the start of the history left out.
- `todo.rs`: the `git rebase -i` style list.
- `config.rs`, `keys.rs`: the settings, and the keys they map.
- `editor.rs`: the inline comment editor.
- `paint.rs`: colours, and drawing text and boxes.
- `tui.rs`: the editor, and the work it does away from the screen; what a
  click is on.
- `web.rs`, `web/client.rs`, `web/index.html`, `build.rs`: `stacked web`,
  its server, its wasm and its page; the build script compiles the wasm.
- `demo.rs`, `memory.rs`: `stacked demo`, the tour; and the database and
  repository it keeps in memory, with a stand-in for the agent.

## Tests

`tests/cli.rs` builds a repo and drives the binary. It covers a round trip
through `git am`, the tree, recovery from broken lines, comments on the
stack, a patch and a line, every way of asking about them (a script stands
in for the agent and rewrites commits with git, runs `stacked fits`,
answers, opens comments, tests), a split following its comments, a comment
moved to the stack, a patch's comment asked of the whole stack adding a
patch on top, a reply written again, asks waiting their turn, a rebase, a branch when the
stack moved on (and left out of the history), failures kept to look at, comments staying on their line
across versions, a patch edited by hand (as mail and as code, at a line,
refused and taken up again), trimming, the settings, filing Phabricator
threads, apply, and what does not apply being refused, edited again, or
read-only. `tests/tui.rs` draws the editor into a buffer and presses keys:
that no letter but 1 and 2 does anything, keys mapped by the settings, the
menus by place, what `?` says where, writing in every pane, a comment asked
about and its stream and answer shown in place, the screen never waiting on
the agent, redrawing not slowing as the agent writes, the filter kept in
the file, the three views, reading by hunks and by a Rust file's blocks,
editing a patch by hand at the cursor, the history screens, the branches
History… draws and the stack diff it shows beside each change, the agent
asked about a record of the history, a new database started from it, a
moved comment and an answer's comments, a patch's comment asked of the
whole stack, what you said last written again (off the queue meanwhile),
a comment moved to the stack taking the cursor with it, the mouse (the
cursor following it, a click as Enter, a right click's menu, a menu's item),
the comments
pane's previews, and moving a patch. With `--features web`, `tests/web.rs`
serves the editor and asks it over HTTP as a browser would: the token
needed, the page, the wasm, the rows as they change, a key and a click; and
`web.rs` tests the rows and the lines both ways, with the client's own tests
(`web/client.rs`, compiled to the machine as well as to wasm). With `--features
demo`, `tests/demo.rs` takes the tour, each step done as Tab does it: the
stand-in's fix, the board at its end, nothing made on disk; `memory.rs` tests its diffs applied as git applies them. The
stream's thinking is tested in `ask.rs`. The tests in `check.rs` show
the check's index holds only what the patches touch, that it writes nothing
to the repository, and how a patch's files are read before and after it;
those in `history.rs`, what the history leaves out (comments, runs, a move
to where the stack is, a branch put straight back) and how a stack diff
follows each patch by its change, a move told from a shift.

## Not done yet

- A deleted comment cannot be seen again: the editor and `stacked comments`
  skip it, and Undo is only for the stack. Its text is still in the file, as
  the file only grows. To build: `deleted` in `Show…`, each as it was, with
  `History of this comment…` and `Bring it back` in its menu, and `stacked
  comments NAME --show deleted`.
- `stacked config` only prints every setting. To build, as `git config`
  does: `stacked config KEY` prints one (`view.layout`), `stacked config KEY
  VALUE` writes it in the settings file, the rest of the file as it was and
  the value checked first, and `--unset KEY` takes it out.
- When an ask lands, the screen changes on its own: the stack moves to the
  agent's edit if you have not edited since. self-review-rs now holds
  changes until `R`; should stacked do the same?
- How apply picks a branch, which belongs to publishing: the editor applies
  on no branch, `stacked apply --branch` on one.
- A stack-wide review runs one agent; one per patch is not done.
- An agent other than claude must speak claude's command line and stream.
- Only git mail is read: no `hg export`.
- Nothing submits to Phabricator: `stacked apply`, then moz-phab.
