# stacked

A stack editor, in the terminal. Patches, the stacks they make and every
edit made to them, comments on them, and what the agent (claude) and
Phabricator said: all in one file that is only ever appended to, so that
any edit can be undone and the history read back. A comment can be asked of
the agent, which answers it or changes the patches; what it brings back
waits for your review. Your checkout is not touched unless you ask.

What follows are the notes on using it that do not belong in its `--help`.
Press `?` in the editor: it says what can be done where the cursor is.

## Install

```sh
cargo install --git https://github.com/carverdamien/stacked
```

or, in a clone of this repository, `cargo install --path .`; or run it in
place with `target/release/stacked` after `cargo build --release`. It needs
`git`, an agent for asks (`claude`, unless the settings name another), and
`curl` for Phabricator.

With the editor in a browser too (`stacked web`, below), built with the
`web` feature; its browser half is compiled to wasm, with Rust's
wasm32-unknown-unknown target:

```sh
rustup target add wasm32-unknown-unknown
cargo install --path . --features web
```

With a tour of it, to learn it by (`stacked demo`, below), built with the
`demo` feature: `cargo install --path . --features demo` (or
`--features web,demo` for both).

## Running

```sh
stacked import bug123 main..HEAD   # the commits main..HEAD as stack bug123
git format-patch --stdout main | stacked import bug123 --base main   # or mail
stacked new bug124 --base main     # an empty stack, to add patches to
stacked stacks.jsonl               # open the editor on that database
stacked                            # ... on $STACKED_FILE, the settings' file, else ./stacked.jsonl
stacked show bug123                # its patches, bottom first
stacked show bug123 2 --path a.txt # a file as patch 2 leaves it, no checkout made
stacked edit bug123                # as `git rebase -i`: reorder, drop, reword, edit
stacked edit bug123 2 --path a.txt --line 12   # a file as patch 2 leaves it, at line 12
stacked edit bug123 2 --path a.txt --in worktree   # ... in the stack's own worktree, .stacked/bug123/user
stacked edit bug123 2 --line 30    # patch 2 as mail, at line 30
stacked review bug123 --accept-all # an edit lands for review first: accepted, it goes in
stacked comment bug123 rebase onto origin/main        # a comment on the stack
stacked comment bug123 --patch 2 split this in two    # on a patch as a whole
stacked comment bug123 --patch 2 --line a.txt:+TWO why?   # on one of its lines
stacked comments bug123            # every comment: short id, where, state, thread
stacked comments bug123 --show closed    # the closed ones, as each was
stacked send bug123 4f1c2a --wait  # the agent works on it; its answer lands in the thread
stacked send bug123 --all --answer-only   # every comment that can be sent
stacked send bug123 4f1c2a --later   # in the stack's batch, to go with the rest
stacked send bug123 --batch --fan-out   # the batch: edits to one agent, answers an agent each
stacked reply bug123 4f1c2a and keep the title
stacked pull bug123                # reviewers' comments from Phabricator, held for your review
stacked apply bug123 --branch bug123   # only now does the worktree change
stacked export bug123 | git am     # or put it anywhere by hand
stacked trim new.jsonl --from @40  # a new database, the history before @40 left out
stacked config > ~/.stacked.toml   # every setting, to start a settings file from
stacked config editor hx           # one written in the settings file; KEY alone reads it
stacked config --unset editor      # ... taken out of it
stacked config --local editor vim  # ... in this checkout's own settings, over the file's here
stacked setup agent bug123         # which of the agent's tools every ask may use, asked one by one, for its checkout
stacked setup agent bug123 --tui   # ... on a screen, a box for each
```

`-f FILE` points any command at a database. A first argument that is not a
command is the database to open: `stacked FILE` is `stacked open FILE`.

## A tour

`stacked demo` (built with `--features demo`) teaches the editor with a game
of tic-tac-toe played in a file, board.txt, one patch per move. It starts
from an empty board and an empty stack, and plays five moves as patches.
Each patch shows the whole board, so the moves before it can be seen. O's
fourth move is a mistake, and X's fifth move wins because of it. The tour
has 14 sections, the most useful first, and takes about fifteen minutes:

1. Read a stack and its patches.
2. Comment on a line: O's mistake.
3. Ask the agent to change the code: asked about the whole stack, because
   the fix changes X's winning move too, a stand-in for the agent rewrites
   both moves (O blocks X, then X blocks O), and you can watch it write.
   Asked about patch 4 alone, the stand-in says that the fix needs more
   than that patch; accepting that answer moves your comment to the stack.
4. Review what the agent did: its work waits, the banner says so, and Esc
   opens the banner; Enter there opens the review, its change of the
   stack patch by patch and its reply, each accepted with Enter, then
   closed.
5. Reply to the agent with a question, asked answer only, its answer
   accepted at once with Enter on the comment.
6. Keep track of what you have read, in reading.
7. Switch how the code is drawn: unified, side by side, compact.
8. The history of the stack, and the versions of a patch.
9. Move a patch: stacked refuses, because each patch only applies to the
   board that the patches before it leave; Esc stops moving it.
10. Drop a patch, undo the drop, and see the tree of edits.
11. Ask the agent to add a patch: a comment on the stack asks for O's next
    move, and the stand-in adds it as patch 6, once its review accepts it.
12. See what the agent did, in Runs….
13. Where help is (`?`), and how to change the keys.
14. Start on a project of yours. At the end, the tour shows the board as
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
the comments on the stack). Scrolled past, a patch's title stays on the
middle's first line, the rest under it, in every view of the code; a click
on it goes back to it. The right pane lists the comments the middle
shows, under a header of its own; with none, it is not there. The top bar
names the file and the stack, and on its right says how far the stack's
review is (Reading, below, says how).

There are five keys, and two of self-review's (Copied from self-review-rs,
below, says what it is); the settings can make any of them other keys, and
the help then names those:

- The arrows move. ↑↓ go through the patches (the header is above the
  first), the lines of the patch, or the comments (their header above the
  first); pgup, pgdn, home and end go further. ← and → go from pane to
  pane, and past the side panes cycle what they show: expanded, compact,
  hidden. ← on the expanded patches makes them compact (each its number
  and the lines it adds and removes, `+12 −3`, in a narrow pane), ← again
  hides them, for the middle to have the width, and ← from the middle
  brings them back expanded (each its number and title, whole, folded
  under itself when the pane is too narrow). → on the comments does the
  same: compact, a line for each (who, and where it is); hidden, the bar
  saying how many there are; expanded again, each with the first line of
  what was said last.
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
- Esc goes back: out of a menu, a list, a stream. In the panes, it goes to
  the banner first, when there is one (while a comment is open, or work
  waits), from wherever the cursor is; Esc there comes back where it was.
  Without a banner, it
  goes up, from the comments to the middle, to the patches, to the stack's
  header. In the comment editor, Esc asks what to do with the comment.
- Every pane keeps where its cursor was, while the editor is open: the
  middle and the comments of each patch (whichever its version, so an edit
  keeps the line) and of the stack, the patches of each stack, the history
  of each stack, the runs, each transcript, each review. Coming back to
  one, the cursor is where it was, on what it was on if that is still
  there. The tree of edits, the stacks and the versions open on where the
  stack is, as before.
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

A comment shows under what it is on: `└ comment:` (or `└ agent's comment:`,
`└ ann's comment:`), in the colour of who opened it, then each turn of its
thread, a bar at its left and its author's name in the author's colour
(yours amber, the agent's violet, a reviewer's blue), and what they said in
the terminal's own colour, so that no comment reads as code. The colours
are settings (`[colors]`, below).

Comments are written where they will show, in an editor inline: a comment
on the stack under the left pane's header, on a patch under its title in
the left pane, on a line under the line, a reply under its comment, in the
middle or in the comments pane. The editor's box hides nothing: what was
under the line goes on after the box, in every view of the code. The pane
being written in takes half the width meanwhile. Typing and the arrows
edit, and enter breaks the line. On a line where no comment is open but one
was closed (or a patch's title), the box opens on the one closed last there,
dimmed, its thread as it was (`you: why so loud?`, `agent: It is a shout.`),
to read before you write: ↑↓ (pgup, pgdn) scroll it, and the first thing you
type takes its place. What you write is the comment; the closed one stays
as it was.
ctrl-a and ctrl-e go to the start and the end of the line. ctrl-k cuts the
rest of the line, or at its end the line break, and ctrl-k again adds what
it cuts to that; ctrl-y puts what was cut last where the cursor is, in this
comment or in the next one. ctrl-← and ctrl-→ go a word back and a word on,
as alt-← and alt-→ (alt-b and alt-f) do; a word is what is between blanks.
On a Mac, ctrl-← and ctrl-→ switch desktops unless that is turned off in
the keyboard settings, so use alt (option) there. Esc
asks what to do with the comment: `Save it`, `Save it and ask agent`, `Throw it
away`, `Keep writing`; above the ask, `Send it`, `[x] now  [ ] later`, picks
when it asks (later, the comment waits in the stack's batch: Asking the
agent, below). The menu opens under the comment (above it when
there is no room under), and the comment stays where it was written, to
read again before it is saved. How the agent is asked is the choice of `Save it and
ask agent`, `[x] change  [ ] the whole stack  [ ] answer only  [ ] build and
test` (the whole stack for a patch's comment, build and test for the
stack's or a patch's as a whole), as it is of a comment's `Ask agent`: one
pick, kept for the stack. Esc asks even when nothing changed, so that what
you wrote earlier can be sent as it is: as nothing would be saved or thrown
away, the menu is then `Ask agent` (the same choice), `Leave it as it is`,
`Keep writing`. Changed, what you wrote earlier (your reply, a turn written
again) has `Throw the changes away` rather than `Throw it away`: the
comment stays as it was. With nothing written (or,
when you rewind a conversation, nothing but the lines stacked wrote), Esc
just closes the editor, as nothing would be saved. Left as it was, the
editor closes too when the agent cannot be asked from it: the stack is
read-only, or the last word is not yours (the agent's reply, written
again, is still the agent's word). Where a
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
was: its changes thrown away, or left as it is, it goes back on; edited, it
waits to be asked again (or `Save it and ask agent`). In $EDITOR it is only what you
said, nothing dropped, a line starting with `#` as any. The comment's
history says `you edited it`. On the command line: `stacked reply NAME ID
--edit TEXT`.

Any turn of a thread can be written again, whoever wrote it. With the
cursor on one of its lines in the middle, the menu's first item is `Edit
the agent's reply` (or `Edit your comment`, `Edit ann's reply`), here or in
$EDITOR: trim the agent's answer down to what you need, and the agent is
told it as you left it the next time it is asked. While a turn is written
again, the editor shows it, and the thread above the editor no longer
does.

A conversation can also be rewound: `Rewind the conversation from your
reply` (or `from your comment`), on a turn of yours, or on the agent's or
a reviewer's to rewind from your turn above it. The editor opens empty for
your new text, and under it, every line starting with `#`, is what goes
once you save: your turn and every turn after it. Your text then takes the
place of all of it: the turns before your turn stay as they were, and your
text is the last word, to ask the agent again. Only the lines stacked wrote
are dropped when you save; any other line is yours, `#` or not. In
$EDITOR, `#` lines at the top say these rules too. Leave nothing but
stacked's lines, or throw the text away, and nothing changes. A thread the
agent or a reviewer opened has no turn of yours to rewind from at its
start: `Edit the agent's reply` is the way there. Your last word is
written again with `Edit your reply`, not rewound. None of this is done
while the agent works on the comment, or while its reply waits for your
review. The comment's history says which turn was edited, or that the
conversation was rewound. On the command line, N counts the turns as
`stacked comments` lists them, from 1 for the comment itself: `stacked
reply NAME ID --edit --turn N TEXT`, `stacked reply NAME ID --rewind N
TEXT`.

## Editing a patch by hand

On a line of code, the menu's `Edit a.txt here in $EDITOR` opens the file
as the patch leaves it, at that line. Where it is edited is the item's
choice, `[x] tmp/file  [ ] pwd  [ ] .stacked/NAME/user`, kept for the stack:

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
- `.stacked/NAME/user`: your worktree of the stack, at the top of its
  repository, made the first time and kept (`.stacked/` goes in the
  repository's `info/exclude`, out of `git status`'s sight). Each edit moves
  it to its patch; your checkout is not touched. The first one checks out
  every file; the next ones only what differs. The agents' worktrees are
  beside it (below); one made before they were, `.stacked/NAME` itself, is
  moved to `.stacked/NAME/user` the first time it is needed.

In a checkout, what the edit changes there is the patch's new version: the
files git tracks, and the files new since the edit started (files git
ignores, and the ones that were there before, are left out). Its message
and author stay as they were. Closed without saving, nothing changes. The
new version lands for your review (below) once the stack is seen to apply
with it. If the patches
above no longer apply on it, it is not taken, and nothing is lost: `Edit it
again` opens it as you left it, or `Throw the edit away`. On the command
line, `stacked edit NAME PATCH --path FILE --line N` does the same, `--in
tmp|pwd|worktree` saying where (else where the stack's menu last picked); an
edit not taken is kept in a file (`tmp`) or a commit, and `--from FILE` or
`--from COMMIT` takes it up again.

When a patch above no longer applies on your edit, its conflict can be
resolved by hand instead: `Resolve the conflict in patch 3 by hand` opens
the file of patch 3 that no longer applies, in $EDITOR, as patch 3 would
leave it on your edit, at its first conflict. `<<<<<<< your edit`,
`=======` and `>>>>>>> patch 3` mark where the two disagree, as `git
merge-file` marks them (with `||||||| before your edit` too when git's
`merge.conflictStyle` is diff3 or zdiff3). It is a copy of the file alone,
wherever your edit was made. Resolve it, take the markers out, and save:
what you save is patch 3's new version of that file, its message and
author as they were. Saved with the markers in, it comes back to resolve
again. Should another file of patch 3, or a patch above it, then not apply,
it is offered next, one conflict after another; a patch whose change merges
with yours without a conflict opens with no markers, to look over and save.
Once the stack applies, your edit and each patch resolved land together, as
one change of the stack (`edit … as …, resolve … as …` in its history).
`Throw the edit away` throws all of it away, and `Edit it again` goes back
to your edit, what you resolved thrown away. A file the patch above adds or
deletes, or one that is not text, is not offered: edit it again, or edit
that patch as mail. On the command line, `--resolve` opens each conflict in
turn after the edit; left unresolved, the edit is kept as above, and
`--from … --resolve` takes it up again.

`Edit this patch in $EDITOR`, there or on the patch, opens the patch as
mail at the line the cursor is on: its message, its diff. It comes back
with why at its top if it does not apply, as an edit by hand does.

An edit by hand does not go in the stack at once: it lands, as the agent's
work does (What the agent brings back waits for your review, below), and
its review opens as soon as it is saved: the stack operations it makes
(`modify  1 …`), and beside them what it does to the stack, to accept or
reject. Accepted, it goes in, as the edit of the stack it is (`edit … as …`
in the history); rejected, nothing changes. Left for later, the banner says
so (`⟳ 1 edit by hand waits for your review`), and as with the agent's
change, the stack is not edited by hand again before it is reviewed. So it
is of `Edit … here in $EDITOR`, `Edit this patch in $EDITOR`, `Edit the
stack as a list in $EDITOR` and the conflicts resolved after them; moving, dropping,
undo and redo change the stack at once. On the command line, `stacked edit`
says the run the edit landed as: `stacked review NAME RUN --accept-all`
takes it, and `stacked edit … --accept` takes it at once.

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
  its reply goes in its thread. On the command line, `stacked send NAME ID
  --whole-stack`.
- `answer only`: read-only, enforced by its tools: it reads the code and
  the stack, and it may read the web with `curl` (a bug on Bugzilla, a
  revision on Phabricator, a page of documentation), through `jq` if need
  be. It may also use what your own claude settings allow, and the tools
  stacked's `agent.tools` names (the stack's checkout's own, if it sets
  them: Settings, below), your MCP servers' among them: running in
  the repository, it has the servers you added there. What it was not let
  use is said in its stream (`⛔ not allowed: mcp__moz__get_bug: …`) and in
  its run's summary, to allow it if it should be. No worktree is made for
  it, which in a big repository takes a while, unless the settings say so
  (below): as self-review's answers do, it runs in the stack's repository as
  it is checked out, and is told that this need not be the stack. It reads
  the stack through stacked: `stacked
  export`, `stacked show NAME N`, and `stacked show NAME N --path FILE`, a
  file as patch N leaves it, whether the stack changes it or not, read
  without a checkout. Only the reply comes back. An
  answer may open comments of its own on lines within the place, as a
  review does, each about one issue (a line with two issues gets two
  comments, both on it): the asking comment then says `opened 4 comments, 1 with your
  replies`, and its menu goes to them, or closes it and them.
- `build and test`, on the stack's comments and a patch's as a whole: it may
  run any command, in the stack's build worktree (below), by its own rules
  (CLAUDE.md), never the user's worktree; it changes nothing in the stack.
  What it found comes back in the reply and on each patch's title, saying which
  stack it tested.

An ask goes now, or later, as in self-review's batch: above `Ask agent`
(and `Save it and ask agent`), `Send it`, `[x] now  [ ] later`, picks when,
kept for the stack, and Enter on it asks as the ask under it does, which
then says so (`Ask agent later`). Asked later, a comment waits in the
stack's batch, marked `◇` and said `batched (may change the code)`; nothing
is started. Its menu has `Send it now (…)`, alone, as it was batched, and
`Remove it from the batch`; written again, it stays in the batch, to go as
it is then. The banner sends the batch (below), as you pick: its edits to a
single agent, in one ask, of the whole stack when they are on more than one
patch or one was batched so; its answers to a single agent too, in one ask
with the whole stack in view, or fanned out, an agent each, all at once;
each build and test on its own. On the command line, `stacked send NAME ID
--later` batches (how, as `send` says it: `--answer-only` and the rest), and
`stacked send NAME --batch` sends the batch, the answers an agent each with
`--fan-out`.

Where an agent works costs time and space: a worktree is a checkout of
every file. So the stack has worktrees of its own, `.stacked/NAME/agent`
for changes, `.stacked/NAME/answer-only-agent` for answers and
`.stacked/NAME/build` for builds and tests, beside yours
(`.stacked/NAME/user`), out of git's sight. An ask takes the first
that no running ask is in and that holds nothing uncommitted, moves it to
what it is asked about, and leaves it there when it lands, for the next
ask to take: only what differs is written. One busy, the next ask takes
`agent-2`, `agent-3`. One an agent left work unfinished in is left as it
is, for you to look at; the ask says so when it fails (`it is left in
…/.stacked/NAME/agent`), and the next takes another. Where each kind of ask
works is a setting of `[agent]` (the stack's checkout's own, if it sets
it): `worktree` for a change, and `answer_only_worktree` for an answer,
each `pwd`, `ondemand` or `precreate`:

- `ondemand`, a change's unless set: the stack's own worktree, made the
  first time an ask needs it, and kept until you remove it (`git worktree
  remove .stacked/NAME/agent`).
- `precreate`: the same, made when the editor opens the stack, and each
  stack it switches to, away from the screen, so that no ask waits for it;
  removed when the editor exits, away from it too, but those an agent is
  working in or left anything uncommitted in.
- `pwd`, an answer's unless set: the stack's repository as it is checked
  out. For an answer, as it is. For a change, moved to the stack, the agent
  working in your checkout, then back where it was once it lands; not while
  it has changes to files git tracks. While it works, leave the checkout to
  it.

A build and test always works in `.stacked/NAME/build`, made the first
time and kept until you remove it (`git worktree remove --force
.stacked/NAME/build`), with what each build made in it, for the next build
to build on: only changes to the files git tracks, which a build should not
leave, keep it from the next build, which then takes `build-2`.

`agent.worktrees` puts the agents' worktrees elsewhere than `.stacked/`: in
that folder, under one of each repository's own.

Several go at once, by self-review's rule: one changing ask at a time on a
stack, answer-only asks together, a build and test once no change runs on
its stack, one at a time of every stack, as builds take the machine (a
build asked on another stack meanwhile says it goes `once the running build
and test of s lands`); a change waiting holds back the answers asked after
it. An asked comment that cannot go yet waits as ready, says what it waits
on, and goes on its own when that lands. Nothing is asked on a stack that
does not apply.

While the agent changes a stack, the stack is not edited by hand: the
agent's change is made from the stack as it took it. The menus' edits of
the stack and its patches are off meanwhile, and the menu says why:
editing a patch or one of its files, editing the stack as a list, moving,
dropping, undo and redo, picking a state from the tree or a version,
deleting the stack. The same commands refuse on the command line.
Comments are written as ever, except those the agent works on: their
`Reply` and edits wait for the agent's reply to land, as the reply is to
what was asked. An edit by hand open in $EDITOR when the agent takes the
stack is not lost: a file's edit is kept, to `Edit it again` later; a
patch's mail, or the stack's list, opens again with why at its top. A
comment waiting its turn is off the agent's queue while you reply to it,
and goes back on with your reply.

Changes are checked when they land, like any edit. A patch's change slots
into the stack in its place. Before it finishes,
the agent runs `stacked fits`, which says whether the patches above still
apply on its change; if they would not, or the comment needs more than its
patch, it changes nothing and the comment moves to the stack's comments,
with the agent's why as its last turn and `moved from patch 2 · a.txt: TWO`
on its first line, to be asked again from there (asked of the whole stack
from the start, it would not have had to). The cursor goes with it if it
was on it, and the bottom line says so. `Go to where it came from`
and `Move it back to patch 2` are in its menu. Comments on lines follow a
split or merged patch to the piece that has their line. A rebase says onto
what (the agent adds `"base"` to its answer): the stack's base moves there.

Where it stands shows on the comment's first line, or under it when that is
too long: ready (and on what), with the agent (and the last lines of its
stream under it: what it says and runs, and while it thinks, one line
counting it, `✻ thinking · ~688 tokens`, with the words of it when claude
gives them), agent replied (changed, answered, tested), failed (and why),
moved. Until the agent writes its first line, one line says what is being
run to start it, as it runs: `starting: git worktree add --detach … BASE`
(the stack's own worktree, made the first time, which takes a while in a
big repository) or `starting: git switch --detach BASE, in WORKTREE` (taken
again), then `starting: git am --keep-non-patch, 3 patches` (the stack, up
to what it is asked about); then `starting: claude -p … --permission-mode
…, in WORKTREE` (or the repository, for an answer as it is by default, or a
build and test). They stay
at the top of its stream, as `$ git worktree add …`, in the stream, in
`Runs…` and in `stacked transcript RUN`. Enter on a comment the agent is
working on opens its stream in place; esc folds it. The comments pane marks each (`●` with the agent, `◇`
batched, `◌` ready, `◆` its reply waits for your review, `✓` replied, `✗`
failed, `↑` moved), its header counts them (`4 comments · 1 with the agent ·
1 to review · 2 to read`), and the top bar too (`agent ●2 · batch:3 ·
ready:1`).
`Show…`, on either header, picks what both panes show, each with its
count: all, to send, to review, to read (any, from an answer, from a change), in
progress, failed or moved, gone (on a line the patch no longer has),
closed, none. It is kept per stack, in the file. A comment you are done
with is closed, from its menu (`Close this comment…`): it leaves the
comments shown. It is not lost, as the file only grows: `closed` shows each
as it was, `closed @12` on its first line, its menu (which Enter opens) to
`Reopen it` or to read its history. The agent's comments moved to the stack
are not closed, nor what is moved back.

A comment you no longer need to read can be collapsed by hand: its menu's
`Collapse this comment` shows it in the middle as one quiet line, which
says how the comment starts and how many turns it has (`▸ comment: why so
loud? · 3 turns`). The cursor still goes on that line, and the comments
pane lists the comment as before. Press Enter on the line, or choose
`Expand this comment` in its menu, to show the comment whole again; writing
on it (a reply, an edit) expands it too. A comment stays collapsed until it
is expanded, opened again or not.

The commits made in the agent's worktree, stacked's and the agent's, are
not signed, whatever git's `commit.gpgsign` says: they are only read back
as patches. `stacked apply`'s commits are yours, signed as git says.

The agent runs in a process group of its own. Quitting the editor leaves it
running, and what it does lands on its own; leaving (ctrl-c, or `Quit`)
lists the asks still running, each as `stacked runs` says it and where its
agent works: its worktree, or the repository. A worktree with work the agent
did not commit, or that could not be filed, is left in place, and the run's
summary says where; others are removed once landed. `Runs…` lists every
ask, apply and pull, and shows what the agent wrote.

`STACKED_CLAUDE` names a program to run as the agent, over the settings'
`agent.command`. The tests use it. Whatever it is, it is given claude's
arguments (`-p PROMPT --output-format stream-json …`) and must answer as
claude does.

## What the agent brings back waits for your review

When an ask lands, nothing changes under you while you read or write.
What the agent brought back is held, in a queue of work to review, the
first landed first; the reviews pulled from Phabricator wait in it too
(Phabricator, below). One landed ask may hold three kinds of parts: its
change of the stack, if it changed it; its replies to the comments it was
asked about; and the comments it opened (an answer's, or a build and
test's findings on each patch's title). You accept or reject each part. A
comment is one part, and the change of the stack is one part, taken whole
or not at all. Rejecting the change of the stack rejects the replies that
go with it too, since each says what the agent changed. Once every part is
accepted or rejected, the review closes. What was accepted is applied
then; what was rejected is dropped for good, though `Runs…` still shows
what the agent said and did.

Until its review closes, a change of the stack keeps the stack as the
agent took it: the stack is not edited by hand, as while the agent works,
and the next ask that may change the code waits for that review
(`ready (may change the code), it goes once you have reviewed the agent's
change of patch 2`). Asks to answer only, or to build and test, do not
wait. A comment whose reply waits says so on its first line (`the agent's
reply waits for your review`), and is not written on, nor asked again,
before you take its reply. Enter on it takes the reply into its thread at
once, accepted, out of the review (its menu's `Accept the agent's reply`).
A reply that goes with a change of the stack is reviewed with that change.

In the editor, a banner under the top bar says what waits on you on the
stack, in a line: what waits for your review (`⟳ 2 landed asks wait for
your review · the next: a change of patch 2, 1 reply, asked "…"`), what
the asks batched (`3 batched asks`), what the agent works on (`the agent
works on 1 ask`), the comments that wait their turn (`1 comment waits its
turn`), and how many comments are open (`5 open comments`); it is there
while any of it is. Landing writes nothing
on the bottom line. Esc goes to the banner from wherever the cursor is in
the panes, and ↑ from the stack's header goes there too; a click on it as
well, but not the mouse going by. With the cursor on it, the banner opens,
a line for each: on top, `⟳ Review …` for what landed, the first landed
first; under it, while there is a batch, `◇ Send the batch… 2 edits, 1
answer only`; then every comment open on the stack, the most recently
updated first, marked as the comments pane marks it (`●` with the agent,
`◇` batched, `◌` waiting its turn, `◆` its reply waiting for review, `✓`
answered…), with where it is, how it starts, and where it stands (with the
agent, the last line it wrote). ↑↓ pick one, and Enter reviews what landed,
or goes to the comment; on the batch, it opens the banner's menu, all of
it, which Space opens from any line: what landed, to review; the batch, to
send (`Send 2 edits to a single agent`, `Send 1 answer only to  [x] a single
agent  [ ] fan out`, `Send 1 build and test, each on its own`); every
comment the agent can be asked about, in the order they were written, a
box checked for those in the batch, which Enter checks (the comment goes in
the batch as it was last asked, else as the stack's asks are) or unchecks
(it is taken out); then the comments, to go to. ↓ past the last line goes
to the stack's header, and Esc goes back where the cursor was, the banner
closed again. A comment you are done with, closed, leaves it.

A review lists the parts on the left, in three sections, under the
comments the agent was asked about, for its work, on top (`asking
comments`): each is `keep`, as it is when the review opens, or `close`,
done with: Enter turns one to the other, as Space's `Close it with the
review` and `Keep it open` do, and the review closes those to close as it
closes, once their replies are in their threads (`Show…`, closed, lists
them, to reopen). Marked to close, a comment's replies, which go with it,
are accepted; kept again, they are as they were. The sections are the stack
operations of the change (a line for each patch: `noop`, `add`, `modify`,
`move`, `remove`, and the base when it is rebased), the comments it opened,
and its replies. On the right is the part the cursor is on, as it would be:
the stack diff of the change, from the stack as it is, each new version's
interdiff under it; or the comment where it would be, the reply as the
last turn of its thread. → goes into it to scroll, → again makes it wide,
the list out of sight, and ← narrows it, then comes back to the list (too
narrow for both side by side, → shows it wide at once). On a line of the
stack operations, → shows that patch as the work leaves it instead, a line
at a time, the comments under their lines, the work's replies in their
threads. Enter accepts the part the cursor is on, or puts it back to
review if accepted (rejecting is Space's); every line of the stack
operations is the one part of the change. Space has `Mark as
accepted` or `Mark as rejected`; and, each kind apart, `Mark all opened
comments accepted` and `… rejected` for the comments the work opened,
`Mark all replies accepted` and `… rejected` for its replies. Marking the change
rejected marks the comments rejected too, and marking it accepted again
puts their marks back as they were; a comment cannot be accepted while
the change is rejected. Esc says what is marked, by kind of part
(`replies: 1 accepted, 1 to review`), and asks what to do: `Save` keeps
the marks in the file, for the review to go on from them when you open it
again; `Discard` throws away the marks made since the review was opened;
`Close review`, once every part is marked, applies what is accepted;
`Accept all remaining and Close` and `Reject all remaining and Close`
mark what is left and close once you say so (the question says what is
left, the menu on `Keep reviewing`), while something is left: once every
part is marked, they are grayed. A reply that goes with a change of the stack
opens that review, on the reply, from its comment (Enter, or `Review the
agent's work…` in its menu).

What you write in the review of the agent's work sends that work back to
the agent. In the patch as the work leaves it, ↑↓ move from line to line,
and Enter writes a comment on the line in the inline editor, as in the
main view: on a comment, what you said last written again if the last word
is yours, a reply if not, and your answer to the agent's reply if this work
replied to it. On a reply in the list, Space has `Reply to it`, your answer
to follow the reply in its thread (`Edit your answer to it` once written).
Nothing written there asks the agent at once, and the list shows it under
`what you wrote, to send back`. With any of it, the work is not accepted:
Esc has `Send it back to the agent` in place of closing the review. The
work is rejected, its change not applied; its replies go in their threads,
your answers after them, and the comments it opened go as you marked them.
Then the comments it was asked about whose last word is yours, and what you
wrote in its review, are asked again, in a round that starts from the stack
this work makes: the agent's worktree holds the change it made, and the
agent is told it is another round. That round's work lands for review in
place of the one sent back.

The change of the stack is edited by hand from its review, as the work
leaves the stack: on a line of the stack operations, Space has `Edit this
patch in $EDITOR` (the patch of that line, as mail), `Edit a.txt in
$EDITOR` for each of its files (where it is edited is the item's choice, as
for `Edit … here in $EDITOR`), and `Edit the stack as a list in $EDITOR`.
Saved, that work is rejected, and the work with your edit in it lands in
its place: its replies and the comments it opened go with it, as you had
marked them, and its change is to review again. The review goes on there,
its title saying what you edited (`then you edited a.txt in Bug 1`). The
review of an edit by hand has the same, to edit it again. On the command
line, `stacked edit NAME PATCH --work RUN` edits the patch as that landed
work leaves it.

On the command line, `stacked landed NAME` lists the queue: each landed ask
by its run, what it holds, and each part of it. `stacked review NAME [RUN]
--accept-all` (or `--reject-all`) closes the review of that ask, the first
landed if no run is given: the parts not marked yet are accepted (or
rejected), and what is accepted is applied; `--close-asking` closes the
comments the agent was asked about too. In the review of landed work RUN,
`stacked comment NAME --patch N --line … --work RUN TEXT` writes a comment
on the patch as that work leaves it, and `stacked reply NAME ID --answer
TEXT` answers the agent's reply to comment ID; then `stacked review NAME
--send-back` sends the work back to the agent with them (`--accept-all`
refuses while you wrote in it). `stacked send NAME ID --wait`
says what waits for your review once it lands; `--wait --accept` accepts
all of it as soon as it lands, for a script.

## Reading

`2` switches to reading, as self-review's view of what is left to review:
in the middle, Space marks the block of code at the cursor read and goes on
to the next line not read, and Enter marks it unread again (no menu in the
code meanwhile: the other panes still have theirs). A block is the
innermost block of code around the line with lines not read yet, from a
tree-sitter grammar (Rust, C, C++, Python, JavaScript, TypeScript, Java,
Go) or else the file's braces: on a line whose block is read, the block
around it, and once those are read, the hunk. A block's first line (its
header) and its last (its footer) are its own: Space on either marks it
whole. A line of more than one block is the innermost's, and of two as deep
(`} else {`, which ends one and starts the next) the one it ends. Inside a
block, empty lines split its code into runs of lines, its inner blocks each
one piece of a run: each run is a block of its own, so Space on a line of
one marks its run, then the block around it. A mark never reaches past the
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

On the right of the top bar, how far the review of the stack is: the lines
its patches change that are not read yet, the comments open on it, and how
long the rest takes at your pace (`120 lines to review · 3 comments to
close · ETA 1h 20m`). With less room, less of it (`120 lines · 3 comments ·
ETA 1h 20m`, then `ETA 1h 20m`); with all read and no comment open,
`nothing left to review`. Your pace is the lines you marked read and the
comments you closed, in the time you spent: the time the file was written
to, each hour or more with nothing written to it left out, as a pause. What
comes to the stack slows it down: the lines new to it since its first
patches went in (a patch added, what a new version of one changes) and the
comments opened on it (yours, the agent's, reviewers'), in the time spent
since. Its lines are done when you read them faster than they come, its
comments when you close them faster than they open, and the rest when both
are: the time it says is time spent, the pauses left out, not a time of
day. A comment moved (a patch split, or the agent moving it to the stack and
back) is neither closed nor opened. It says `ETA unknown` while lines are
left and you never marked one read, or comments are left and you never
closed one; `ETA never` while either comes as fast as you go.

## Searching

`Search…`, in every menu, opens the search on a screen of its own. On the
left, at the top, where it looks, a button (`‹ comment ›`) that ← → turn
to `all`, `patch`, `comment` or `history` (a click does too), and what you
type; under it, what is found, appended as it is found, in a thread of its
own: the screen never waits on it, and what you type starts it again. At
the bottom, how many it found, and whether it is still looking. On the
right, the match picked in its context: its rows in its patch (or the
stack), the lines around it in its file, or its record of the history,
what you look for lit. ↑↓ pick a match. Enter goes there: to the comment,
to the patch at its line (a file's line its diff does not show: the
patch's nearest line of that file), or to the record in the history. Esc
goes back where the search was opened.

Where it looks is where it was opened, and what is in sight there comes
first. From the comments pane, the comments: those the pane shows, then
the stack's others the filter shows, then all the rest, those the filter
hides and other stacks'. From the middle, the patches: the one shown, then
the stack's others, their messages and their diffs; then the files the
patch touches, every line its diff does not show, as it leaves them, read
without a checkout. From the history, its records and their stack diffs.
From anywhere else, everywhere. A line matches when it holds what you
type, whatever its case; the first 500 matches are listed.

The searches are remembered while the editor runs, newest first: what you
type is followed, dim, by the search before that starts the same way, and
Tab takes it.

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
  it and are left out. Each says what it changed in
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
  rev and time of each; a closed one's too, from `Show…`, `closed`.

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

`~/.stacked.toml` (or the file `$STACKED_CONFIG` names) holds the settings,
which a checkout's own, `.stacked/local.toml`, overrides there (below);
each is optional, and `stacked config` prints them all as they are, in the
file's form, with what is wrong with the file. As `git config` does,
`stacked config KEY` prints one (`view.layout`, `keys.down`: each value on a
line of its own), `stacked config KEY VALUE…` writes it in the file (several
values for a list: `stacked config keys.down down j`), and `stacked config
--unset KEY` takes it out. The rest of the file stays as it was, comments
too. What is written is checked first: a setting there is not, a value of
the wrong kind, or one stacked would not take is refused, with why, and
nothing is written; nor anything in a file stacked cannot read. The
settings:

- `file`: the database opened when none is named, nor `$STACKED_FILE`.
- `editor`: the editor, for comments, patches, the stack as a list and the
  code, with `{file}` and `{line}` where they go (`hx {file}:{line}`); else
  `$VISUAL`, `$EDITOR`, vi, told the line as each takes it.
- `[agent]`: `command`, the program run as the agent; `args`, added to
  every run; `worktree` and `answer_only_worktree`, where a change and an
  answer work: `pwd`, `ondemand` or `precreate` (Asking the agent, above;
  `ondemand` and `pwd` unless set); `model`; `tools`, tools every ask may use besides its own, as
  claude names them for `--allowedTools`: all of an MCP server's
  (`mcp__bugzilla__*`; there is no way to name every server's at once), one
  of its tools (`mcp__bugzilla__get_bug`), a command (`Bash(jq *)`, each
  command of a pipe needing its own). An answer may use them too, so give
  only tools that read. `stacked setup agent [NAME]` writes it with you: it
  starts the agent in the stack's repository (else where it is run), where
  it has your MCP servers, and stops it as soon as it has said what tools it
  has; then it asks, for each MCP server, whether every ask may use all its
  tools (`mcp__moz__*`), none, or which one by one, and for each other tool
  stacked does not give every ask, whether it may, saying of those that
  change things (`Bash`, `Edit`, `Write`…) that an answer could change
  things too. No answer is no. What you let it is added to `tools`, what
  you did not taken out, in that checkout's own settings (below; with
  `--global`, in `~/.stacked.toml`, for the checkouts without their own),
  the rest of the file as it was; it says first which file `tools` is in
  now, and which it writes. Outside a checkout, only `--global` can be
  written. With `--tui`, on a screen of its own: `tools` as the settings
  have it now, and the file it is in, at the top; a box for each MCP server
  (all its tools, those to come too) and for each of its tools, then for
  each other tool, and for what the settings let that the agent does not
  say it has (`Bash(jq *)`, a server not there now), to keep or take out;
  and what `tools` is to be, in the file it writes, at the bottom. Space
  lets a tool or not, `s` saves, esc leaves with nothing written.
  `worktrees`, where the agents' worktrees go, else `.stacked/` at the top
  of the stack's repository.
- `[reviews]`: `source`, `"phabricator"` or `"none"` (no pulling); `url`,
  the Phabricator a bare `D123` trailer is on; `arcrc`, where the tokens
  are; `token`, one for `url` instead; `trailer`, the trailer naming a
  revision (`Differential Revision`).
- `[view]`: `layout` at the start (`unified`, `side-by-side`, `compact`);
  `reading`, to start reading; `patches` and `comments`, the side panes at
  the start, `"expanded"`, `"compact"` or `"hidden"` (`true` and `false`,
  as before, are expanded and hidden); `stream_lines`, the agent's lines
  under a comment; and
  `split_width`, the columns two sides need.
- `[colors]`: what paints each part of the screen, as git writes a colour
  (`git config color.diff.new "green bold"`): a foreground, then a
  background, each a name (`black`, `red`, `green`, `yellow`, `blue`,
  `magenta`, `cyan`, `white`, any of these after `bright`, `default`,
  `normal`), a number from 0 to 255, or `#rrggbb`; and any of `bold`, `dim`,
  `italic`, `ul`, `blink`, `reverse`, `strike`. `added` (`green`), `removed`
  (`red`), `hunk` (`magenta`), `file` (`cyan bold`: a file's name, the
  titles of boxes, the history's records), `meta` (`blue`: index lines,
  borders), `header` (`cyan bold`: each pane's first row); `you` (`172`),
  `agent` (`134`) and `reviewer` (`32`): the comments and turns of each,
  their bar and name; `note` (`yellow`: what a menu says under its items, the
  tour's box, the comment editor's frame, a run going); `bar` (`brightwhite
  blue`), `warn` (`black yellow`: the top bar when the stack cannot change),
  `status` (`black brightwhite`: the bottom line) and `picked` (`normal
  238`: what a pane has picked while the cursor is in another, on a grey
  background, the cursor's own pane reversing its pick). `stacked config
  colors.you 208` sets one.
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

A checkout's own settings, `.stacked/local.toml` at its top (beside the
stacks' worktrees, out of git's sight), override `~/.stacked.toml` there:
what it sets is its own, a list whole and a table's settings each (`[keys]
down` alone, the other keys as they were); what it does not set, or sets
wrong, is the other file's. The settings used are those of the checkout
stacked runs in, but for what a stack's agent does (`[agent]`: its tools,
where it works, the program run): those are of the stack's checkout,
wherever stacked runs, and read when an ask starts, so that a change
counts from the next ask, even with the editor open. `stacked config --local KEY
VALUE…` writes one there, `stacked config --local --unset KEY` takes it
out, `stacked config --local KEY` reads it, and `stacked config --local`
prints the file. `stacked config` names both files and marks each setting
the checkout's own sets (`# .stacked/local.toml`); one written in
`~/.stacked.toml` that the checkout's own sets too is said to be the
checkout's there. A `.stacked/local.toml` that git tracks is neither read
nor written, and that is said: it came with the repository, and a setting
can name a program to run (`agent.command`, `editor`).

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
says why, in yellow (`colors.warn`). `stacked base NAME REV`, from the repository, gives it
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
  turn deleted by stacked's first way of deleting one is fenced ````
  ```deleted ````, each line of it quoted with `>`: it is read only to be
  left out, and nothing writes one any more. A
  comment collapsed, or expanded, is a record naming it, apart from its
  history. A read mark holds the lines it read; taking it back is a record
  naming it.
- An ask's run, once landed, holds what it brought back for review: the
  stack its change makes (its patches stored already, by their content),
  each reply, each comment it opened. Nothing of it is applied before its
  review closes. A review record holds the marks saved for a run's review,
  the replies taken already, and whether it is closed; the last one is
  what stands.
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
settings'. What is new waits for your review, in the queue of work the
agent's asks land in (below): nothing is filed before you accept it. Each
new thread is a comment opened, on its line, or on the patch's title for
one on the whole revision, its replies pulled with it turns of it; a reply
to a thread there already is a reply to it, the reviewer's turn (`ann's
reply waits for your review`, `Accept ann's reply`). The banner counts the
pulls apart (`1 pull from Phabricator waits for your review`), and the
review says what was pulled. Pulling again holds only what is new: what was
pulled before, waiting, accepted or rejected, is passed over, so a comment
you rejected does not come back. A reviewer's comment can be asked about
like any.

## Applying

`apply`, or the menu's `Apply the stack to the worktree…`, is the only thing
that changes a worktree. It checks out the stack's base, detached or on
`--branch` (made, or reset there), then `git am`s the patches. It refuses if
anything is uncommitted, and if a patch does not apply it aborts and leaves
the worktree at the base. Like every run, it lands in the database. The
editor asks in plain words before it applies, on no branch; a branch is the
command line's.

## Copied from self-review-rs

stacked started from self-review-rs, a reviewer of your own changes in the
terminal (the worktree's, or a stack's, `main..HEAD`), whose comments claude
is asked about as stacked's are. It lives in
[my-moz-tools](https://github.com/carverdamien/my-moz-tools), beside
self-review, the Python tool it rewrites in Rust; "as in self-review" here
means both.

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

- `main.rs`: the command line; the library does the rest.
- `db.rs`: the file, its records and its lock.
- `check.rs`: whether patches apply, before they go in; the files as a
  patch leaves them.
- `ops.rs`: edits to a stack, `$EDITOR` (the patch as mail, its code), apply.
- `git.rs`: what stacked asks of git: mail from a range, the stack's
  worktrees and the agents', a checkout to edit a patch in.
- `ask.rs`, `claude.rs`: asking the agent about comments, from the queue to
  landing; claude's command line, its stream and its replies.
- `held.rs`: what an ask brought back, held for your review.
- `review.rs`, `phab.rs`: Phabricator.
- `mbox.rs`: reading `git format-patch` mail, writing mail for `git am`.
- `rows.rs`, `view.rs`, `diff.rs`, `thread.rs`: the patch or the stack as
  rows, line keys, comment threads.
- `panes.rs`: the compact layout's two panes.
- `reading.rs`, `blocks.rs`: the reading mode's marks, by blocks of code.
- `progress.rs`: how far the review of a stack is, and how long the rest
  takes at your pace.
- `tree.rs`: the tree of edits as a graph, and the lanes the history draws.
- `history.rs`: a stack's history, what changed it, and stack diffs: as the
  editor shows them and the agent is told them.
- `trim.rs`: a new database, the start of the history left out.
- `todo.rs`: the `git rebase -i` style list.
- `search.rs`: searching what stacked holds, in a thread of its own.
- `config.rs`, `keys.rs`: the settings (`~/.stacked.toml`, and a
  checkout's own over it), and the keys they map.
- `setup.rs`: `stacked setup agent --tui`, the agent's tools on a screen.
- `editor.rs`: the inline comment editor.
- `paint.rs`: colours, and drawing text and boxes.
- `tui.rs`: the editor, and the work it does away from the screen; what a
  click is on. `tui/reviewing.rs` and `tui/searching.rs`: its screens for
  reviewing what the agent brought back, and for searching;
  `tui/banner.rs`, the banner of the agent's work; `tui/places.rs`, where
  each pane's cursor was.
- `web.rs`, `web/client.rs`, `web/index.html`, `build.rs`: `stacked web`,
  its server, its wasm and its page; the build script compiles the wasm.
- `demo.rs`, `memory.rs`: `stacked demo`, the tour; and the database and
  repository it keeps in memory, with a stand-in for the agent.

## Tests

Run `cargo test` after any change; `cargo test --features web,demo` runs
the browser's and the tour's too (the web feature needs the wasm target, as
in Install). No test runs claude: a script stands in for it.

`tests/cli.rs` builds a repo and drives the binary. It covers a round trip
through `git am`, the tree, recovery from broken lines, comments on the
stack, a patch and a line, every way of asking about them (a script stands
in for the agent and rewrites commits with git, runs `stacked fits`,
answers, opens comments, tests), a split following its comments, a comment
moved to the stack, a patch's comment asked of the whole stack adding a
patch on top, a reply written again, asks waiting their turn, a rebase, the stack not
edited by hand while the agent changes it, failures kept to look at, comments staying on their line
across versions, a patch edited by hand (as mail and as code, at a line,
refused and taken up again), trimming, the settings (a checkout's own over
them, an ask on a stack having its checkout's wherever stacked runs, one
git tracks not read; and the tools every ask may use, set up with the
agent, in the checkout's own or with `--global`), Phabricator's comments
held for review then filed (none twice, none rejected again), apply, and
what does not apply being refused, edited again, or read-only. `tests/tui.rs` draws the editor into a buffer and presses keys:
that no letter but 1 and 2 does anything, keys mapped by the settings, the
menus by place, what `?` says where, writing in every pane, a comment asked
about and its stream and answer shown in place, the screen never waiting on
the agent, redrawing not slowing as the agent writes, the filter kept in
the file, the three views, reading by hunks and by a Rust file's blocks,
editing a patch by hand at the cursor, the history screens, the branches
History… draws and the stack diff it shows beside each change, the agent
asked about a record of the history, a new database started from it, a
moved comment and an answer's comments, a patch's comment asked of the
whole stack, what you said last written again (off the queue meanwhile;
left as it was, asked as it is or left so), a comment moved to the stack
taking the cursor with it, the mouse (the cursor following it, a click as
Enter, a right click's menu, a menu's item), the comments pane's previews,
moving a patch, searching (what is in sight first, the patches then the
files they touch, the history), the agents' worktrees made ahead as the
stack's checkout says, the banner of the agent's work (Esc to it and
back, open a line for each, Enter on each; the reviews pulled waiting in
it as the agent's work does), the comment editor hiding
nothing in each view of the code, each pane keeping where its cursor
was, the patch's title kept at the top of the middle, and the side panes'
modes (expanded, compact, hidden). With `--features web`, `tests/web.rs`
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
follows each patch by its change, a move told from a shift. Those in
`setup.rs` show the setup screen's boxes, a server's tools let all at once
or one by one; those in `search.rs`, which lines of a file a hunk shows
already; those in `config.rs`, a file of settings over another, and a
checkout's own read and written unless git tracks it.

## Not done yet

- Turns deleted by stacked's first way of deleting one (```` ```deleted ````)
  are still read, only to be left out. Once no database holds them, that
  code goes, and with it the split of a thread's turns between those that
  stand and all of them.
- How apply picks a branch, which belongs to publishing: the editor applies
  on no branch, `stacked apply --branch` on one.
- A stack-wide review runs one agent; one per patch is not done.
- An agent other than claude must speak claude's command line and stream.
- Only git mail is read: no `hg export`.
- Nothing submits to Phabricator: `stacked apply`, then moz-phab.

## License

MIT: see [LICENSE](LICENSE).
