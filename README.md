# stacked

A stack editor, in the terminal. Patches, the stacks they make and every
edit made to them, comments on them, and what the agent (claude) and
Phabricator said: all in one file that is only ever appended to (its
header aside), so that any edit can be undone and the history read back. A
comment can be asked of the agent, which answers it or changes the
patches; what it brings back waits for your review. Your checkout is not
touched unless you ask.

What follows are the notes on using it that do not belong in its `--help`.
Press `?` in the editor: it says what can be done where the cursor is.

## Install

```sh
rustup target add wasm32-unknown-unknown
cargo install --git https://github.com/carverdamien/stacked
```

or, in a clone of this repository, `cargo install --path .`; or run it in
place with `target/release/stacked` after `cargo build --release`. It needs
`git`, and an agent for asks (`claude`, unless the settings name another).

The four features below are on by default, so that a build has them all.
`--no-default-features` builds stacked with none of them, and `--features`
then names those to keep: `cargo install --path . --no-default-features
--features demo,moz-phab`, for instance, builds stacked with the tour and
Phabricator, but without the editor in a browser or StackED.

The `web` feature adds the editor in a browser (`stacked web`, below). Its
browser half is compiled to wasm, with Rust's wasm32-unknown-unknown
target, which `rustup target add` installs (above). Without that target,
the build stops and says to add it, or to build stacked without `web`, as
`--no-default-features --features app,demo,moz-phab` does.

The `demo` feature adds a tour of the editor, to learn it by (`stacked
demo`, below).

The `moz-phab` feature adds Phabricator (below): the reviews pulled from
its revisions, stacks imported from them, and a patch submitted to it as
work in progress, over Conduit with the token moz-phab (or arc) keeps,
which `curl` calls. With it, the reading mode
takes the trailer moz-phab writes in a message as an empty line, which
needs no reading (Reading). Built without it, stacked has none of it, but
reads a database that holds reviews pulled.

The `app` feature adds StackED, the editor in a window of its own
(`stacked app`, below). Its window is drawn by eframe (egui), on OpenGL,
which the build fetches: on macOS and on Linux, the build needs nothing
more. On Linux, eframe loads X11's or Wayland's libraries, xkbcommon's and
OpenGL's (EGL or GLX) only when StackED starts, and a Linux desktop has
them. On macOS, `stacked app --bundle` then makes StackED.app, for the
Dock.

### Completion in zsh

`stacked completions zsh` prints stacked's completion for zsh. Keep it
where zsh finds its functions:

```sh
mkdir -p ~/.zfunc && stacked completions zsh > ~/.zfunc/_stacked
# and in ~/.zshrc, before compinit runs: fpath=(~/.zfunc $fpath)
```

or have ~/.zshrc read it, once compinit has run: `source <(stacked
completions zsh)`. TAB then offers stacked's commands and their options,
each with what it does, the values an option lists (`--show`'s), files
where a file goes, and from the database (`-f`'s, else the one stacked
opens) its stacks, a stack's patches with their titles, its comments (to
reply to, or send) and the agent's runs. The function asks stacked each
time (`stacked __complete`), so it is never out of date with it.

## Running

```sh
stacked import bug123 main..HEAD   # the commits main..HEAD as stack bug123
git format-patch --stdout main | stacked import bug123 --base main   # or mail
stacked new bug124 --base main     # an empty stack, to add patches to
stacked stacks.jsonl               # open the editor on that database
stacked                            # ... on $STACKED_FILE, the settings' file, else ./stacked.jsonl
stacked app                        # ... in a window of its own: StackED (app)
stacked app --bundle               # StackED.app, in ~/Applications, for the Dock (macOS)
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
stacked comment bug123 --patch 2 --path a.txt why here?   # on one of its files, as a whole
stacked comments bug123            # every comment: short id, where, state, thread
stacked comments bug123 --show closed    # the closed ones, as each was
stacked send bug123 4f1c2a --wait  # the agent works on it; its answer lands in the thread
stacked send bug123 --all --answer-only   # every comment that can be sent
stacked send bug123 4f1c2a --later   # in the stack's batch, to go with the rest
stacked send bug123 --batch --fan-out   # the batch: edits to one agent, answers an agent each
stacked send bug123 4f1c2a --effort max   # at claude's --effort max, rather than the editor's last pick
stacked stop 1761650000000-4242    # the agent of that run (stacked runs lists them) stopped: its ask lands as failed at once
stacked reply bug123 4f1c2a and keep the title
stacked moz-phab pull bug123       # reviewers' comments from Phabricator, held for your review (moz-phab)
stacked moz-phab import D123       # a new stack of D123, those it stacks on and those on it (moz-phab)
stacked apply bug123 --branch bug123   # only now does the worktree change
stacked export bug123 | git am     # or put it anywhere by hand
stacked trim new.jsonl --from @40  # a new database, the history before @40 left out
stacked config > ~/.stacked/config.toml   # every setting, to start a settings file from (once ~/.stacked is made)
stacked config editor hx           # one written in the settings file; KEY alone reads it
stacked config --unset editor      # ... taken out of it
stacked config --local editor vim  # ... in this checkout's own settings, over the file's here
stacked setup agent bug123         # what each kind of ask may use, one list of tools, checked line by line, for its checkout
stacked setup agent bug123 --tui   # ... on a screen, a read-only box and a change box for each, as the editor shows it before the first ask
```

`-f FILE` points any command at a database. A first argument that is not a
command is the database to open, if it names one: a file there, or a path
to one (`./bug123`, `bug123.jsonl`); `stacked FILE` is then `stacked open
FILE`. Any other word is a command mistyped: stacked says so, and which
command is near it (`stacked shwo`: `show`), rather than open a new
database of that name.

## A tour

`stacked demo` (built by default, with the `demo` feature:
`--no-default-features --features app,moz-phab,web` builds stacked without
it) teaches the editor with a game of tic-tac-toe played in a file,
board.txt, one patch per move. The tour starts from an empty board and an
empty stack, and plays five moves as patches. Each patch shows the whole
board, so the moves before it can be seen. O's fourth move is a mistake,
and X's fifth move wins because of it. The tour has 14 sections, the most
useful first, and takes about fifteen minutes:

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
7. Pick how the code is drawn in the menu of `1`: unified, side by side,
   compact, or unified split in two; and each file whole rather than its
   hunks.
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

`stacked web` (built by default, with the `web` feature:
`--no-default-features --features app,demo,moz-phab` builds stacked without
it) serves the editor on this machine and opens the browser on it: the
same editor, the same keys, drawn in the page as in a terminal. Over the
side panes, the patches and the comments, the mouse marks the line under
it, underlined, and picks nothing: what is picked stays so (reversed in
the pane the cursor is in, grey in the other) until a click picks the
line, and a second click on it is Enter. Elsewhere the cursor follows the
mouse: what is under it is picked, as the arrows would (a line of the
patch, an item of a list or of a menu; a patch being moved goes there), so
a click is Enter on it. A right click is Space, its menu. A menu opened
with a click opens next to the click: the top left corner of its box is
just below and to the right of the cell clicked, or, near the right edge or
the bottom of the page, the menu opens to the left of that cell or above
it, to be whole on the page. The menu keeps that place while it is open,
and when the page is resized it moves only as much as it must to stay
whole. A menu opened with a key opens in the middle, as in a terminal.
Outside a menu, a click is Esc for the menu: it closes, and the comment it
asks about, if any, goes on being written. Writing a comment, a click
outside its box, with either button, is Esc as well, and picks nothing of
what is under it: the box closes when nothing is written in it, else the
menu Esc opens asks what to do with the comment. That menu opens next to
the click too, even over the comment, and a click outside the menu goes
back to the comment, with what is written in it. Opened with Esc, the
menu opens under the comment, or above it, so that the comment stays in
sight. A click in the box puts the cursor where the
click is. Writing a comment, or reading what `?` says, the mouse going by
moves nothing. The wheel is ↑↓, and a paste types what is pasted, into a
comment being written. The page follows the mouse only while it is the
active one: while another window or another tab has the focus, the mouse
going over the page marks and picks nothing, and the line it marked is
underlined no longer. A click and the wheel act on the page all the same.
When the page has the focus again, what is under the mouse is marked or
picked at once, if the mouse went over the page in the meantime and is
still there. The click that gives the page the focus back thus acts as any
other click does, whether the browser sends the click before the focus or
after: a click on a line of the patch is Enter on that line. `--port N`
listens on port N (else any free one), `--no-open` only says where it is.

It listens on 127.0.0.1 only, and only a page holding the token in the
address it opened (a new one each time) may see it or drive it: a page from
elsewhere could otherwise send it keys. The editor runs where `stacked web`
was started: `$EDITOR`, for what is edited in it, opens there. Quit is
quit, and the page says so. A ctrl-c there, a kill, or that terminal
closing quits too. Either way, as when the editor in a terminal is left,
the agents' worktrees made ahead are removed, and the asks still running
are listed there. A ctrl-c while `$EDITOR` runs there goes to `$EDITOR`
alone.

The server runs the editor as the terminal does, drawn into a buffer of the
page's size, and sends the page the rows that changed (server-sent events);
the page sends its keys and clicks back. What the page does is wasm
(`web/client.rs`: the rows made HTML, the keys and clicks made lines for
the server), carried by fifty lines of JS (`web/index.html`).

## In a window of its own: StackED

`stacked app` (built by default, with the `app` feature:
`--no-default-features --features demo,moz-phab,web` builds stacked without
it) opens StackED, the editor in a window of its own, with no terminal and
no browser: the same editor, the same keys, drawn as in a terminal. The
window paints the editor's screen as a terminal would, a grid of cells in
their colours, bold and italic, underlined or struck through, and draws
the lines of boxes itself, so that they meet from row to row. On macOS
its text is in Menlo, the system's monospace font, with Apple Symbols for
the few characters Menlo lacks; elsewhere it is in Hack, egui's own font,
its bold drawn twice a pixel apart and its italics slanted. ⌘+ and ⌘−
(ctrl elsewhere) make the text larger and smaller, ⌘0 brings it back, and
the window holds as many cells as fit in it.

The mouse works as in the browser (above): over the patches and the
comments it marks the line under it, and a click picks the line, a second
click on it being Enter; elsewhere the cursor follows it, so that a click
is Enter on what is under it. A right click is Space, its menu, and so is
ctrl-click on macOS. A menu opened with a click opens next to the click,
as in the browser, and stays whole in the window; a menu opened with a key
opens in the middle. Writing a comment, a click outside its box is Esc,
and a click in the box puts the cursor where the click is. The wheel and
the trackpad are ↑↓. Each window
follows the mouse only while it is the active one, as the page does: over
a window in the background, the mouse marks and picks nothing, but a
click and the wheel act there all the same. A paste (⌘V on
macOS, ctrl-v elsewhere) types what is pasted where text is being written,
and does nothing elsewhere. On macOS ⌘ is StackED's own (⌘Q quits it, ⌘H
hides it, ⌘W closes the window), and option types the characters it makes
(é, ∂), as Terminal does.

A drag selects the text it goes over, as in a terminal, but kept to the
pane it starts in, or to the box (a menu, a preview, a comment being
written): the lines from where it starts to where it ends, or with option
held (alt elsewhere) a block of them, their columns only. ⌘C copies it
(ctrl-c elsewhere, which quits only when nothing is selected), each line
without the blanks at its end. A click or a key for the editor lets it go.
A click is a press let go where it was pressed, so that a drag is not one;
the cursor does not follow the mouse while its button is down.

StackED runs where `stacked app` was started, as the editor in the terminal
does: it opens the database the editor would (`-f`, `$STACKED_FILE`, the
settings' `file`, else ./stacked.jsonl), and HEAD is the HEAD of the
checkout there. `$EDITOR`, for what is edited in it, opens in that
terminal (one window's at a time), and the window says that it waits until
the editor exits; an editor with a window of its own
(`editor = "code --wait"` in the settings) opens that window instead. Quit
(ctrl-c) closes the window, and closing the window quits its editor;
either way, the agent's asks still running are listed in the terminal, as
the editor in the terminal lists them. Elsewhere than on macOS, StackED
then exits.

On macOS, StackED has many windows, as a Mac app does, and the menu bar's
File menu: New… (⌘N), Open… (⌘O), Open Recent and Close Window (⌘W).
Open… shows the system's Open panel, where any number of databases (.jsonl
files) can be picked: each opens in a new window, even one open in another
window already. New… shows the Save panel, then makes an empty database
where it says and opens it. New… never replaces a file: the panel offers
to, but a file there already is said in an alert and nothing is made
(delete it first, outside StackED). Open Recent lists the eight databases
opened last, the latest first, whether StackED opened them or stacked's
editor did, in a terminal or in a browser (`stacked web`); one no longer
there is taken off the list, and Clear Menu empties it. Each window has an
editor of its own, which runs as if started in its database's folder: HEAD
is the HEAD of the checkout there, and `$EDITOR` runs there; only the first
window of `stacked app` runs where it was started. Close Window, like the
window's close button, closes the window and quits its editor, as ctrl-c
does. Closing the last window leaves StackED running, its menu bar there,
until ⌘Q quits it, and each window's editor with it. The databases opened
last are kept, with the windows StackED.app has open, in
~/.stacked/state.json, never in the settings' file (`$STACKED_HOME` names
another folder than ~/.stacked, and `$STACKED_APP_STATE` another file).
StackED and a terminal's stacked may change that file at once: each change
is made under a lock, on the file state.json.lock beside it, so that
neither loses what the other wrote. StackED kept the file in
~/Library/Application Support/StackED/state.json before; your first run of
this stacked moves it (Settings, below).

The menu bar also has an Edit menu, after File, with Undo, Redo, Cut, Copy,
Paste and Select All. Its items act on the panels' text fields, such as the
name in the Save panel and the folder typed after ⇧⌘G (Go to Folder). While
a panel is open, ⌘Z, ⇧⌘Z, ⌘X, ⌘C, ⌘V and ⌘A do in its fields what they do
in any Mac app, so that a name can be pasted. Outside the panels, the Edit
menu's items have no shortcuts and are greyed out, and StackED's windows
keep those keys: ⌘C still copies the text selected, and ⌘V still types the
clipboard where text is being written.

On macOS, `stacked app --bundle` makes StackED.app in ~/Applications
(`--bundle DIR` makes it in DIR), for the Dock and the Finder: drag it to
the Dock to keep it there. It holds a copy of this stacked, so make it again
once stacked is updated; made again, it replaces the StackED.app there, but
nothing else of that name. Opened from the Dock or the Finder, StackED has
no terminal, nor what a terminal's shell sets. It first takes what your
login shell sets (`$SHELL -ilc`, given ten seconds): `PATH`, to find git and
claude with, `EDITOR`, `STACKED_FILE`. It then opens again the windows that
were open when it was last quit, but those whose database is no longer
there. With none to open again, it opens the database `$STACKED_FILE` or
the settings' `file` names, if either does, and else no window at all, only
its menu bar: File → Open… opens one. What goes wrong is said in an alert.
`$EDITOR` has no terminal to run in there, so it runs in a window of
Terminal's: StackED writes the command in a script of its own (a .command
file, which removes itself as it runs) and has `open -a Terminal` run it,
so that macOS asks for no permission to control Terminal. StackED goes on
once the editor exits; closing that window before it does changes nothing.
An editor with a window of its own (`code --wait`, `zed --wait`, `open -W
-n -a TextEdit`) opens that window instead.

StackED's icon is drawn rather than kept as a picture (`icon.rs`): an S of
white sheets of paper, stacked along the letter, each a little further
along than the one under it so that their edges show, fanned where the S
turns, on a black rounded square. Small, as in the Finder's lists, the
sheets' edges are left out, and the S is plain white. It is the window's
icon, and the Dock's on macOS, whether StackED is started from a terminal
or from StackED.app, whose icon file is drawn from it at every size macOS
asks for.

The editor runs on a thread of its own, drawn into a buffer of the window's
size, as `stacked web` runs it for a page; the window (eframe: egui, on
OpenGL) paints what the buffer holds, and sends the editor its keys, clicks
and size (`app.rs`). On macOS each window is a viewport of eframe's, which
paints itself, and eframe's root viewport is hidden (`app/windows.rs`); the
File and Edit menus, the panels and the alerts are AppKit's, called through
objc2 (`app/menu.rs`).

## The editor

The left pane is the current stack: its header (`≡ bug123 · 3 patches`),
which is the stack's own place, then the patches' titles, bottom first, with
how many comments wait on an answer. When stacked runs in a checkout of the
stack's repository, a line among them says where its HEAD is: `▶ HEAD main`
after the patches its commits since the stack's base are (a commit is a
patch when its diff is, whatever its message), `+2` when two commits above
them are not the stack's; under the header at the base, or when it is not
on the stack (`▶ HEAD main: off the stack`). With more patches than the
pane has room for, the line stays in sight: on the pane's last line while
its place is further down (`↓ HEAD main`), under the header while it is
further up (`↑ HEAD main`), the arrow saying that patches go on between.
It is looked at again as the stack changes, and every few seconds.
`stacked head NAME` says the same.
`Move HEAD…`, in the stack's menu and each patch's, moves that checkout
along the stack: ↑↓ place HEAD's line (`↕ HEAD`), the checkout as it is
meanwhile, and Enter moves it there, Esc leaving it where it was. HEAD is
then detached at the stack's base with the patches up to there applied as
commits, those the checkout has already, message too, taken as they are;
the branch it was on stays where it was. Not while files git tracks have
changes, nor while the agent works in the checkout. `stacked head NAME
--to N` moves it to N patches (0, the base).

A commit of the checkout is a patch of the stack when its diff is, but its
message may say otherwise. While HEAD is on the stack and the messages of
some patches differ from their commits', the banner (What the agent brings
back waits for your review, below) says so before anything else: its line
first (`✉ the commit messages of patches 2 and 3 differ from your
workspace's: sync them first`), and, open, a section of its own on top,
`commit messages, to sync first`, with `✉ Sync the commit messages of
patches 2 and 3: the stack and your workspace differ`; its menu has `Sync
the commit messages of patches 2 and 3…` first. Until they are synced, no
landed work is reviewed: the banner still lists it, its items grayed, and
Enter on it says to sync the commit messages first, as Delete there does,
and Enter on a comment whose reply waits; a comment's `Accept the agent's
reply` and `Review the agent's work…` are grayed, its menu saying why. An
edit by hand lands and waits there. Asks still run and land as they do,
and a review open before the messages came to differ still closes. While
the agent makes its changes in your checkout (`agent.worktree = "pwd"`),
HEAD is the agent's, and nothing is to sync. Nothing else of it is said on
the main screen: the patches pane and HEAD's line say nothing of messages,
and no patch's menu syncs one.

Enter on that line, or its item, opens the sync of the commit messages. On
the left is each patch whose message differs, with what it keeps: `to
pick`, `stacked`, `workspace`, or `nothing to pick` (below). On the right
are the two messages of the patch the cursor is on, the stack's on the left
and your workspace's on the right, or, where the pane is narrower than
`view.split_width` (Settings, below), one over the other, the stack's
first: each with its own line breaks, a line longer than its column folded,
`↪` before each piece after the first. Over them, a note says what the
patch has to pick and what its pick does. The two are compared word by
word, where their lines wrap aside. A message is cut into sections at its
empty lines, a fenced block (```` ``` ````, `~~~`) staying whole, its empty
lines and all; a section is code when one of its lines is a line of a
fenced block, its fences too, a line indented by a tab or by four spaces or
more, or a line that starts with a shell's prompt, `$ `, after any indent.
In prose, the end of a line is a space as any other, and a gap of white
space is one gap however long, so that a paragraph wrapped otherwise is the
same. In code, the end of every line counts, as does the white space within
a line, its indent too; only the white space that ends a line is not read,
so that a command split over two lines in one message and on one line in
the other is not the same. Each message is read so on its own, and a
section that is code in one and prose in the other is not the same either.
The break between two sections is a word of its own. The words both have,
in order, are drawn as any text; what only the stack's has, in the colours
of `removed_word`, and what only your workspace's has, in those of
`added_word`; a break only one of them has is marked on its line, `↵` at
the end of a line of code, `¶` on an empty line. Two messages that differ
only in how they wrap are listed all the same, drawn as any text: HEAD is
not in sync. White space at the end of a line is no difference, as it is
not for HEAD.

`s` keeps the stack's message for the patch the cursor is on, and `w` your
workspace's (`keep_stack` and `keep_workspace` in `[keys]`, Settings,
below); so do the sync's menu, `Keep the stack's message` and `Keep your
workspace's message`, and a click on the message to keep. ↑↓ pick another
patch; → goes into the messages to scroll them, → again shows them wide,
the list out of sight, and ← comes back. Enter closes the sync once every
patch has its pick, asking first, `Keep picking` picked, what it does said:
`Patches 2 and 3 take your workspace's message. The commit of patch 5
takes the stack's message. The stack takes the new versions of its patches
at once: your picks are their review. 3 commits are made again in your
workspace, the files as they were, and main moves to them.` Before that,
Enter says which patches are still to pick, and the menu grays its `Sync
the commit messages…`, saying so too. Esc leaves the sync, the picks kept
for as long as stacked runs (not in the file), and the banner's line opens
it again. While it is open, the list follows the stack and HEAD as they
change, and a pick goes with a patch whose messages no longer differ, or
whose commit's message changed.

Closing puts the message kept on each side that lacks it, all at once, or
nothing at all. The patch takes it where its own message differs: a new
version, in the stack at once, with no review of its own, as your pick is
its review; the history says it is the sync's (`sync the commit messages
of patch 2 with your workspace: edit 1a2b3c4d as 5e6f7a8b`). A change of
the stack waiting for review, made from the stack as it was, goes on from
the stack the sync leaves, as does what goes on from it: a version it kept
of one of those patches is the new one, and a version of its own that said
what the stack's said takes the new message too, so that the change can
still be accepted. The commit takes it where its own message differs: the
commits from the first of those up to HEAD are made again, each on the one
before, their files, authors and dates as they were (`git commit-tree`,
signed as git says), and the branch HEAD is on, else HEAD, moves to them.
Nothing changes if the stack or HEAD changed since the sync listed them,
which it then lists as they are; nor if a commit is to be made again and
files git tracks have changes, or an agent works in the checkout; nor if a
patch is to change while the agent changes the stack, or while work sent
back waits to go back to it: the sync says why. HEAD is then in sync with
the stack, when it was at its top, and the banner's line is gone.

With the `moz-phab` feature (on by default), the trailer moz-phab writes to
name the revision, the line of the message's last paragraph that Reading
(below) says is that trailer (`Differential Revision:
https://phabricator.services.mozilla.com/D123`), is not lost. Two messages
that differ by that trailer alone, one naming a revision and the other
none, as after `moz-phab submit`, have nothing to pick: the message kept is
the one that names it, as it is, and the side that names none takes it
(`The stack takes the Differential Revision D123 from your workspace's
commit.`). A message picked that names no revision, the other naming one,
takes the other's trailer, at the end of its paragraph of trailers, as
after `Reviewed-by: ann`, or after an empty line, as moz-phab puts it. Of
two that name each another, the one picked keeps its own, and the note says
which the pick drops (`Keeping the stack's message drops D34, which your
workspace's names.`). While a patch and its commit differ by the revision
they name, HEAD is not in sync with the stack, so that it does not follow
the stack as a review closes, which would write the stack's message over a
revision only the commit names. Built without the feature, the trailer is a
line of the message as any other, to pick with the rest.

`stacked head NAME` lists the patches whose message differs from its
commit's (`the message of patch 3 differs from its commit's: "Bug 3 -
other words"`; with moz-phab, `patch 3 names no revision in the stack and
D3 in your workspace's commit`), then says that the commit messages are
synced in the editor, from the first line of its banner: there alone.

The middle shows what the left pane's cursor is on: a patch (its message,
then its diff, the comments under the
lines they are on), from a file's name when the cursor is on one of the
files the left pane lists under the patch (the arrows, below, say how), or,
on the header, the stack itself (its patches, then
the comments on the stack). Scrolled past, a patch's title stays on the
middle's first line, the rest under it, in every view of the code; a click
on it goes back to it. The right pane lists the comments the middle
shows, under a header of its own; with none, it is not there. The top bar
names the file and the stack, and on its right says how far the stack's
review is (Reading, below, says how).

There are six keys, and four more for the middle: three of self-review's
(Copied from self-review-rs, below, says what it is) and one of stacked's
own. The settings can make any of them other keys, and the help then names
those:

- The arrows move. ↑↓ go through the patches (the header is above the
  first), the lines of the patch, or the comments (their header above the
  first); pgup, pgdn, home and end go further. ← and → go from pane to
  pane, and past the side panes cycle what they show; in the unified view
  split in two (`1`, below), each half of the middle is a pane of its own
  for them. ← on the expanded
  patches (each its number and title, whole, folded under itself when the
  pane is too narrow) lists the files of the patch picked under it, in the
  order the middle shows them: a line for each file, its path, cut to its
  end when it is too long (`…/tui/banner.rs`), and, when the path leaves
  room for them, the lines the patch adds and removes in it (`+12 −3`).
  In the reading mode (Reading, below), each file says instead how many
  of its changed lines are left to mark read (`7 left`), or `✓` once all
  of them are read, and its path is cut, if it must be, to leave room for
  that. The pane is then as wide as the paths need, from as wide as
  expanded to a third of the screen, and keeps that width from patch to
  patch. ↑↓ go through the patches and those files as one
  list: on a file, the middle shows it at once, its name at the top; ↑
  above the first file goes back to the patch itself, from its start, and
  ↓ under the last picks the next patch, whose files are listed in place
  of the others. Enter or → on a file goes into the middle there, and a
  click on a file goes to it. While the cursor is in the middle, the pane
  shows the file the cursor is in as picked, and ← comes back to that
  file. ← again hides the patches, for the middle to have the width, and ←
  from the middle brings them back expanded. → on the comments makes them
  compact, a line for each (who, and where it is); → again hides them, the
  bar saying how many there are; and → from the middle brings them back
  expanded, each with the first line of what was said last.
- Enter does what is to be done where the cursor is, reading or commenting:
  on a patch, read it; on a file the patches pane lists, read the patch
  there; on a line, comment on it; on a comment, reply, or, while the agent
  works on it, open what it is writing; in the comments pane, go to the
  comment. Elsewhere it is the menu.
- Space is the menu: all that can be done where the cursor is, then undo,
  redo, `Runs…` and quit, which every menu ends with. The stack's items
  (comment on it, ask about all, `Show…`, the stack as a list, the tree,
  the history, Phabricator, apply, switch, delete) are on the left header
  only; a patch's on a patch; a line's on a line; a comment's on a comment.
  ↑↓ and enter; ↑ from the top is Quit. What a read-only stack cannot do is
  shown dimmed, and the menu says why. An item may have choices, radio
  buttons after its label (`[x] tmp/file  [ ] pwd`): ← → pick one, a click
  on one picks it, and Enter does the item with the one picked, which is
  kept for the stack in the database. An item that asks the agent ends
  with a dropdown of the effort it asks at, `Effort [high]` (Asking the
  agent, below), after its choices if it has some. On an item with more
  than one dropdown, ← → pick in the one underlined, which is the first
  until Tab goes to the next one; shift-Tab goes back, as the bottom bar
  says (`←→: how · tab: effort`). A click picks in the dropdown it is on:
  the choice clicked, or, on `Effort [high]`, the next level.
- `?` says what can be done where the cursor is, as `Press Enter to create a
  comment on this line.`, and where the arrows go. The next key closes it
  and does what it does, so the key it names can be pressed straight away.
  With no stack, it says nothing: the menu has nothing but Quit.
- Esc goes back: out of a menu, a list, a stream. In the panes, it goes to
  the banner first, when there is one (while a comment is open, or work
  waits), from wherever the cursor is; Esc there comes back where it was.
  Without a banner, it
  goes up, from the comments to the middle, to the patches (from a file
  they list, to its patch first), to the stack's
  header. In the comment editor, Esc asks what to do with the comment.
- Delete, and Backspace too (a Mac's delete key is Backspace, and its
  forward delete is fn-delete), does what the menu's item that closes,
  drops, stops or rejects what the cursor is on does. On a comment, in the
  middle or the comments pane, it is `Close this comment…`, or `Close it
  and the comments it opened…` when its answer opened some; while the agent
  works on the comment, it is `Stop the agent…`; while the comment waits
  its turn, it is `Take it back from the agent's queue`; and while the
  comment is in the batch, it is `Remove it from the batch`. On a patch, it
  is `Drop this patch…`; on the stack's header, it is `Delete this
  stack…`; on a file the patches pane lists under a patch, it drops the
  patch's changes to that file (Editing a patch by hand, below). In the
  review of landed work, it is `Mark as rejected` on a part, `Close it with
  the review` on a comment the ask was about, and `Take it back` or `Take
  your answer back` on what you wrote there. On the banner, it rejects all
  that remains of the landed work picked and closes its review, as the
  review's `Reject all remaining and Close` does; on a comment's line, it
  does what it does on the comment. On the runs, it stops an ask that
  runs. An item ending with `…` asks first, and so does Delete; Delete also
  asks before it takes back what you wrote in a review, and before it
  rejects landed work from the banner. On a comment closed, it does
  nothing, and the status line says so. In the reading mode, on the code
  in the middle, it takes a mark back (Reading, below). It does nothing in a menu,
  nor in History…, the tree and the versions; and where what is typed is
  read (the comment editor, a line typed in a box, `/`'s bar, the search's
  own screen, the setup's `Add a command…`), Delete and Backspace edit it.
  The bottom bar says what Delete does where it does something (`delete:
  close it`), and so does `?`.
- Every pane keeps where its cursor was, while the editor is open: the
  middle and the comments of each patch (whichever its version, so an edit
  keeps the line) and of the stack, the patches of each stack, the history
  of each stack, the runs, each transcript, each review; each half of the
  middle split in two keeps its own. Coming back to
  one, the cursor is where it was, on what it was on if that is still
  there, else on the closest row there is (for a comment closed or hidden
  meanwhile, the line it was on). The tree of edits, the stacks and the
  versions open on where the stack is, as before.
- `1` opens the menu of the views of the code, the one the code is drawn
  in marked `●`: Enter or a click picks one, and Esc leaves the view as it
  is. Unified and side by side are self-review's; compact draws the two
  sides each in a pane of its own with no gaps, scrolled together or one
  alone (← → turn which, and past the old side or the new one go on to
  the pane beyond); unified split in two is below. Every view but unified
  needs a middle of 60 columns (`split_width`). Picked on a narrower
  middle, a view makes its room, hiding as little as it can: it makes the
  comments compact, else hides them, else hides the patches (listing
  their files gives no room), the comments as they were, then compact,
  then hidden too. The status line says what was hidden and how to show
  it again (`the patches are hidden to make room for side by side: ← out
  of the middle shows them again`), and what was hidden stays so once the
  view is left. On a screen too narrow even with both side panes hidden,
  the view's item in the menu says so, and the view is not taken. A middle
  made too narrow later (a side pane shown again, the window made
  narrower) shows the code unified, and the top bar says that the view is
  too narrow (`· side-by-side: too narrow`) until there is room: picking
  the view again makes it. In
  every view, a line removed and the line added in its place (paired as the
  two sides pair them), when the two are almost the same, at least half of
  what they say, show what changed in them in the colours for words
  (`removed_word`, `added_word`), the rest of them in the lines' own. So do
  the lines of an interdiff, what changed from one version of a patch to the
  next (`Versions…`), and of the stack diffs (`History…`, the edits' `What
  this edit changed…`, the review of the agent's work), paired by their
  first sign; but not the lines of the message in them, its title and its
  body, which are compared as the sync of the commit messages compares two
  messages (History, below). Made
  shorter than the cursor's line, the screen keeps the cursor's line in
  sight, on its last line, in every view.
- Unified split in two shows the patch picked twice, side by side, each
  half a unified view of its own with a line between them, to read one
  place of the patch while going through another: a function's definition
  in one half and its callers, searched, in the other. Each half has its
  own cursor and scroll, its own search (`/`), its own reading (`2`) and
  its own hunks or files whole (`3`); `1`, `2`, `3` and `/` act on the
  half the cursor is in, and the other half draws its cursor as a pane
  draws what it has picked (`picked`). ← and → go from half to half as
  from pane to pane: from the patches to the left half, then to the right
  half, then to the comments, and back; Esc in the right half goes to the
  left one, as ← does. The left half is beside the patches and the right
  one beside the comments: what a side pane does in the middle (a file
  gone to, a comment gone to, the search opened there), it does in the
  half beside it. A click in a half, or the mouse over it, puts the cursor
  there, on the row under it. Comments, replies and read marks are made
  from the cursor's half, and Enter, Space and the menu act on its row;
  what is written shows in both halves, and what is read in each half
  that is reading. Another patch picked shows in both halves, each where
  it was on that patch, else from its top. Another view picked keeps the
  half the cursor is in as the middle, its cursor and its reading and
  files whole with it. `view.layout = "unified-split"` starts in it
  (Settings, below).
- `2` switches between commenting and reading (below).
- `3` switches between the hunks and each file whole: every file the patch
  changes as the patch leaves it, the lines it removes in their place, with
  no hunk's header and nothing left out. The lines of the hunks are the
  rows the hunks have, keyed as they are, so that comments, read marks and
  references are on the same lines both ways. A line in no hunk is drawn
  dim, as self-review draws the lines it adds around a hunk, and takes no
  comment and no read mark: Enter and Space there open the menu, as on any
  line no comment goes on, and the status line says why. Side by side, the
  old file is whole on the left and the new one on the right, lined up as
  the hunks are; compact, each in its pane. A file the patch adds is all
  added, one it deletes all removed, one it renames whole under its new
  name. The files are read as the reading mode reads them, from an index of
  stacked's own (no checkout, the repository only read), once for each
  patch and away from the screen: meanwhile the hunks show, and the bar
  says `whole files: reading…`. A file that is not text shows as its diff
  says it; a file of more than 20000 lines, and one that would take the
  patch past 50000 lines shown whole, show as their hunks, a line over them
  saying so.
- `1`, `2` and `3` go together: each view of the code works in commenting
  and in reading, with the hunks or with each file whole, and split in
  two, each half on its own. Pressed in any
  order, they leave the cursor on its line, on its line of the screen (as
  far down as the rows over it reach). Into each file whole, a hunk's
  header leaves the cursor on the hunk's first line; back to the hunks, a
  line in no hunk leaves it on the closest line of its file in a hunk, by
  their numbers, a tie going to the line above, and the status line says
  so.
- `/` searches what the middle shows, as in self-review, whatever pane the
  cursor is in. What you type goes in the bottom bar. As you type, the
  cursor goes to the first line that holds what you typed, from the line
  it was on down (with none there, from the top), and when that line is
  out of sight, the middle scrolls to put it a third of the way down. The
  matches in sight are lit, reversed from how their lines are drawn; those
  of the line the cursor is on are bold and underlined too. The bar says
  which match the cursor is on, of how many (`match 2 of 5`), and when the
  search started again at the top or at the bottom; or it says that
  nothing matches. Tab goes to the next match, and shift-Tab to the one
  before (`find_next` and `find_previous` in `[keys]`, Settings, below).
  Enter ends the search on the match, the cursor in the middle.
  Esc ends it with the cursor back where it was, in the pane it was in,
  and the middle scrolled as it was; so does Enter when nothing matches,
  and the status line says so. A line matches when it holds what you
  type, whatever its case, as in self-review and in the search's own
  screen (Searching, below): plain text, the blanks at its ends left out.
  A line of code is searched without its `+`, `-` or space, and a line
  folded as the one line it is, the cursor going to the piece that holds
  the match; the rest is searched as the middle shows it: the files'
  names, the hunks' headers, the message, the comments. The search works
  in every view of the code, reading or not, and with the hunks or with
  each file whole (`3`); split in two, in the half the cursor is in, and
  Esc puts that half back where it was. It searches the lines the middle
  shows, so that
  with each file whole it finds the lines no hunk has too, and the cursor
  goes to them. The searches that Enter ends on a match are kept, the
  latest first, twenty at most, in `state.json` (Settings, below), so that
  the next run has them too. `/` opens with the last one in the bar,
  selected, its matches lit: what you type takes its place, Tab goes on to
  its next match, and Enter stays on the match the cursor is on; ↑ and ↓
  go through the searches before it. While the bar is open, Enter, Esc, ↑
  and ↓ are those keys whatever the settings make them, and every key but
  the two going from match to match is typed or edits what is typed:
  Space, `?`, `1`, `2` and `3` too.

Nothing else is a key. Going through the comments pane does not move the
patch: the comment is marked where it is if the patch shows it, else its
thread is shown over the top or the bottom of the patch, as it is above or
below. Enter goes to it.

A line too wide for the middle is folded, as comments are: a line of the
message between words, its indent kept and a bullet's lines under what
follows the bullet; a line of code where the pane ends, each piece after
the first `↪` in place of its `+`, `-` or space. Side by side, each side
folds in its half; split in two, each half as wide as it is. A line folded
is still one line: the cursor goes
through its pieces, its comments go under its last piece, reading marks
and counts it once, and side by side its pieces go beside the line the
other side has there.

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

## Making a stack in the editor

With no stack yet, the menu (Space, or Enter) has the ways to make one;
with stacks, the stack's own menu has them under `New stack…`:

- `Ask the agent to make a stack…`: named after the branch checked out, as
  no stack is, the stack is made empty, on HEAD of the checkout stacked
  runs in, and a comment on it opens, to say what the agent should make of
  it. `Ask agent` saves the comment and asks the agent; what it makes waits
  for your review, as any of its work does.
- `Import from the workspace…`: the branch checked out, on its upstream
  (else `origin/HEAD`, else main or master: `feature on origin/main: 3
  commits`); `Another branch…`, each on its own; or `A range…`, typed, and
  said as it is typed (`unknown ref`, `0 commits: nothing to import`, `2
  commits`), taken only once it holds a commit. The staged changes, and the
  unstaged ones, when there are, may each be checked to go on top of what
  ends at HEAD, a patch of their own (`Staged changes`, `Unstaged changes`).
- `Import from Phabricator…` (built with the `moz-phab` feature, on by
  default): a revision, `D123` (or `123`) or its link, read away from the
  screen with the revisions it stacks on, down to the first that is not
  open, and those stacked on it while one is open; each its latest diff,
  with the message and author of its commit as uploaded. They go on the
  commit the bottom one is on, which the repository must have (`D11 is on
  0123456789ab, which this repository does not have: fetch it first`).
  `stacked moz-phab import D123` makes the same stack from the command
  line, at once, named after the revision as no stack is, or `--name
  NAME`.

An import is seen before it is made: its patches, bottom first, the commit
they go on, and the name of the stack it makes, after the branch or the
revision, as no stack is (`Name it otherwise…`). `Import it as NAME` makes
it, and shows it.

## Writing comments

A comment shows under what it is on: `└ comment:` (or `└ agent's comment:`,
`└ ann's comment:`), in the colour of who opened it, then each turn of its
thread, a bar at its left and its author's name in the author's colour
(yours amber, the agent's violet, a reviewer's blue), and what they said in
the terminal's own colour, so that no comment reads as code. The colours
are settings (`[colors]`, below).

What was said is read as markdown, as comments write it: `**bold**` is
bold, `*italics*` italic, `# headings` bold, a `[link](to)` underlined with
where it goes after it, dimmed; `-`, `*` and `+` items are bulleted (`•`),
numbered ones keep their number, each item's lines under its text; a `>`
quote has a bar at its left; a code block between fences shows its lines as
they are, its fences left out, as one block; `` `code` `` loses its
backticks; both are in the colour of the settings for code, `code`
(`[colors]`, below), light grey on a dark grey; a `---` line is a rule. A
table (a header of cells between pipes, a row of dashes under it, `:` for
how a column aligns, then its rows) has its columns aligned, `│` between
them, its header bold with a rule under it; wider than the pane, its cells
fold within their columns, and too narrow even so, it shows as written. A line breaks where the text breaks it, as comments are
written, and again where it is too wide. Where a comment is said in a line
(listed, folded), its markup is left out. The editor shows what was written
as it was written.

Comments are written where they will show, in an editor inline: a comment
on the stack under the left pane's header, on a patch under its title in
the left pane, on a line under the line, on a file as a whole under its
name (Enter on the file's name in the middle, `Comment on this file` in its
menu), a reply under its comment, in the middle or in the comments pane. A
comment on a file stays on it as long as the patch changes the file, and is
said to be on it as a whole (`patch 1 · a.txt, as a whole`); the agent is
told so, and one it opens with no line (`"line": ""`) is on its file as a
whole too. In the middle, a reply goes under the last
turn of the comment, its references under the box (a turn written again
goes in its place, the turns after it under the box), so that the last
reply and the references are in sight both. If the agent starts on the
comment meanwhile (asked from the command line, say), its stream goes under
the box too, over the references. The editor's box hides nothing: what was
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
asks what to do with the comment: `Save it`, `Ask agent`, which saves it
first and then asks the agent about it, `Attach a reference…` (below),
`Throw it away`, `Keep writing`; under the ask, `Send it`, `[x] now  [ ]
later`, picks when it asks (later, the comment waits in the stack's batch:
Asking the agent, below). The menu opens under the comment (above it when
there is no room under), and the comment stays where it was written, to
read again before it is saved. How the agent is asked is the choice of `Ask
agent`, `[x] change  [ ] the whole stack  [ ] answer only  [ ] build and
test` (the whole stack for a patch's comment, build and test for the
stack's or a patch's as a whole), as it is of a comment's own `Ask agent`:
one pick, kept for the stack. Its line ends with the effort the agent is
asked at, `Effort [high]`, which Tab goes to (Asking the agent, below). Esc
asks even when nothing changed, so that what you wrote earlier can be sent
as it is: as nothing would be saved or thrown away, the menu is then `Ask
agent` (the same choices), `Leave it as it is`, `Keep writing`; `?` says
which way `Ask agent` goes, saving first or asking about the comment as it
is. Changed, what you wrote earlier (your reply, a turn written
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

A comment may have references attached to it: other comments of the
stack, open or closed, lines of a file, and web links. In the editor, Esc,
then `Attach a reference…`, lists `Lines of a file…`, `A web link…` and
the stack's comments, those closed after those open. Lines are written as
a `refs` line says them, `a.txt:120-140,200-210 @2` (no lines, `a.txt @2`,
the whole file; `@0` the base; no `@`, the whole stack); for a comment on a
line, they start with its file, as its patch leaves it. A web link is
written whole on a line of its own, as lines are, `https://example.com/a/b`:
only an address that starts with `https://` or `http://` and names a site
is taken, and the status line says why another is not. Attach a web link
for what nothing in the stack says; the agent is asked to do the same
(below).
Each reference takes a number, never given twice in the comment (not even
once its reference is detached), and what you write names it: `@r1`,
several `@r{1,2,3}` (or `@r{2-4}`), all of them `@r*`. They show under what
you write (written in the middle on a comment there, as its lines under
the editor, those attached meanwhile with them; else in the editor, under
the text), and under the comment once it is saved, a line each: `r1 ↗
patch 2 · a.txt: TWO · "why so loud?"`, `r2 ⇢ a.txt:2-3 · as patch 1 leaves
it`, `r3 ⇗ https://example.com/a/b` (a link too long for the pane is cut
there, its start kept). Enter on a comment's goes to that comment (closed,
it opens as it was); on lines, it previews them; on a link, it shows the
link whole in a box, and Enter there opens it in your browser, with the
program `$STACKED_OPEN` names, else with `open` on macOS and `xdg-open`
elsewhere: the program is given the link as its one argument. The status
line says what was opened, or why it could not be. StackED and `stacked
web` open it the same way, on the machine stacked runs on. A mention
naming no reference attached is drawn in the warning colour, and the agent
is not asked until it names one: the menu says why, and `stacked send`
refuses it. `Detach a reference…` takes one off, but not one that another
turn of the comment names. The references stay with the comment as its
turns change, and go with it to the agent each time it is asked: each
comment as it stands, said open or closed (closed, as it was last), each
file's lines as the version says, each link as it is, which the agent is
told it need not fetch and may not be able to, as its tools may keep it
off the network. The agent reads them for what they say; they are not
asked of it, and they close only as any comment does. `stacked comments`
lists them under the comment. Only the editor attaches them: `stacked
comment` and `stacked reply` do not.

A reference is named by a mention in a turn of the comment, whoever wrote
it (you, the agent, a reviewer), and by one in the agent's reply waiting
for your review, as that reply goes in once accepted. When a rewind or an
edit takes away the last mention of a reference you attached, that
reference is detached, as `Detach a reference…` would: it is no longer
shown under the comment, nor in the editor's lists, and the agent is no
longer told it. The status line says so: `r2 is no longer named: it is
detached` (`stacked reply --edit` and `--rewind` print it too). A
reference that nothing named (attached for what it says, or named by
nothing yet) stays as it is.

In a comment's thread, a mention is drawn as a link, and one naming no
reference the comment has in the warning colour, whoever wrote it. Enter
on a line that names references goes to the one it names: a comment,
lines previewed, or a link shown whole; naming several, it asks which.
(Enter on such a line no longer opens your comment to write in: Enter on
its line, or its menu, does.)

A reply written in $EDITOR (`Reply`, or `Comment on this line` on a line
that has a comment) opens under the conversation it answers, each turn a
line starting with `#` under who said it, the cursor on the line after the
last, where you write: the editor is told the line as it takes it. Under it
are the comment's references, a line each as the comment shows them, to
name in what you write, then the rules. Lines starting with `#` are dropped
when you save, and with nothing written, nothing is saved.

When the last word of a comment is yours, enter on it opens what you said
last, in place, to edit or add to (a reply would only add to it), the turns
before it as they were; enter on its line does too. Its menu has the same
first, `Edit your reply` (`Edit your comment` when no one has answered it),
here or in $EDITOR; `Reply` adds to it without showing it. Not while the
agent works on it. A comment waiting its turn
leaves the agent's queue while it is edited, so that it is not asked as it
was: its changes thrown away, or left as it is, it goes back on, as it was
asked and at the same effort; edited, it waits to be asked again (or `Ask
agent` saves it and asks it again). In $EDITOR it opens
as a reply does: the conversation above what you said, the cursor on it,
the comment's references under it. Only the lines stacked wrote are
dropped when you save; any other line is yours, `#` or not. The comment's
history says `you edited it`. On the command line: `stacked reply NAME ID
--edit TEXT`.

Any turn of a thread can be written again, whoever wrote it. With the
cursor on one of its lines in the middle, the menu's first item is `Edit
the agent's reply` (or `Edit your comment`, `Edit ann's reply`), here or in
$EDITOR: trim the agent's answer down to what you need, and the agent is
told it as you left it the next time it is asked. While a turn is written
again, the editor shows it, and the thread above the editor no longer
does. In $EDITOR, it is in its place in the conversation: the turns
before it above it, those after it under it, as lines starting with `#`,
a turn not yours under who said it. The editor shows the turn's text
alone; the references it attaches (the agent's) are shown with the
comment's meanwhile, and the turn keeps them once saved. Only one it
named, and that nothing names any more, is taken out, as the status line
says. One it attached without naming it counts as named by its turn, and
stays as long as the turn does.

A conversation can also be rewound: `Rewind the conversation from your
reply` (or `from your comment`), on a turn of yours, or on the agent's or
a reviewer's to rewind from your turn above it. The editor opens empty for
your new text, and under it, every line starting with `#`, is what goes
once you save: your turn and every turn after it, with the references the
agent's turns attach. Your text then takes the
place of all of it: the turns before your turn stay as they were, and your
text is the last word, to ask the agent again; a reference you attached
that only the turns gone named is detached. Only the lines stacked wrote
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
  patch as it was. Each diff is made in a temporary folder of its own,
  which is removed once the diff is made, whatever happens: two edits saved
  at once, in two of StackED's windows, never mix their files.
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

On a file the patches pane lists under a patch, Delete drops the patch's
changes to that file, once asked: the question names the file and the
patch, and says that the patches above it are checked. The patch without
that file's part of its diff, its message and author as they were, is its
new version, and it lands as an edit by hand does, once the patches above
it are seen to still apply on it; if one does not, nothing changes, and
the status line says why. A rename, and a file the patch adds or deletes,
goes whole: the file is then as it was before the patch. On the only file
of a patch, the patch would be left empty: stacked says so, and offers
`Drop this patch…` instead.

A patch whose message has lines of prose longer than 72 columns has `Wrap
the long lines of this patch's message at 72 columns` in its menu, after
`Edit this patch in $EDITOR`; the stack's header has `Wrap the long lines
of 2 messages at 72 columns`, for every patch whose message has some, as
one edit. The menu's note says how many lines of which patches are longer
(`Patch 2's message has 3 lines of prose longer than 72 columns and patch
4's has 1.`). Each paragraph of prose with a line longer than the width is
filled again: as many words on each line as the width lets, the words in
their order, a gap of white space one space. A list item (`- `, `* `, `+ `,
`1. `, `1) `) is a paragraph of its own: it keeps its marker, and its next
lines go under its text, indented by three spaces at most (four would make
them code). A word longer than the width, a link say, stays whole, on a
line of its own; a word that would make the line it starts something else
(a list's marker, `>`, `$`, a fence, or `#`, which git reads as a comment)
goes on the line before. Never wrapped: the title, the first line, however
long; code, as the sync of the commit messages reads it: a fenced block
(```` ``` ```` or `~~~`), its empty lines and all, and a paragraph with a
line indented by a tab or by four spaces or more, or with a line starting
with a `$ ` prompt; a quoted line (`> `); the trailers, the last
paragraph from its first `Key: value` line on (`Reviewed-by: ann`,
`Differential Revision: …`); and a paragraph whose lines all fit, its short
lines not joined. So nothing but where the lines break changes: the sync of
the commit messages, which compares them where their lines wrap aside,
finds the two the same, and so does the interdiff of the edit's review,
where each paragraph wrapped is there once, as the new version writes it,
none of its lines a change (History, below). The width is the settings' `view.message_width`, 72
unless set (Settings, below). The new versions, their diffs and authors as
they were, land for your review as an edit by hand does (below), the
history calling the edit `wrap the long lines of the messages of patches 2
and 4 at 72 columns`. On a read-only stack, and while the stack is not to
be edited by hand, the items are offered but cannot be chosen, and the
menu says why. No command wraps messages: the editor's menus do, in the
terminal, in the browser and in StackED.

An edit by hand does not go in the stack at once: it lands, as the agent's
work does (What the agent brings back waits for your review, below), and
its review opens as soon as it is saved: the stack operations it makes
(`modify  1 …`), and beside them what it does to the stack, to accept or
reject. Accepted, it goes in, as the edit of the stack it is (`edit … as …`
in the history); rejected, nothing changes. Left for later, the banner says
so (`⟳ 1 edit by hand waits for your review`), and as with the agent's
change, the stack is not edited by hand again before it is reviewed. So it
is of `Edit … here in $EDITOR`, `Edit this patch in $EDITOR`, `Edit the
stack as a list in $EDITOR`, the conflicts resolved after them, a file's
changes dropped with Delete, and the long lines of messages wrapped; moving, dropping,
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
choice, picked with ← →, kept for the stack, and the same as that of the
comment editor's `Ask agent`:

- `change`: it may change the code within the comment's place; its reply
  says what changed.
- `the whole stack`, on a patch's comment: a change, with the whole stack in
  its worktree, as for the stack's own comments: it may change any patch,
  add patches anywhere, reorder them. The comment stays on its line, and
  its reply goes in its thread. On the command line, `stacked send NAME ID
  --whole-stack`.
- `answer only`: read-only, enforced by its tools: it reads the code and
  the stack, and, unless its list says otherwise (`agent.answer_tools`,
  below), it may read the web with `curl` (a bug on Bugzilla, a revision on
  Phabricator, a page of documentation), through `jq` if need be. It may
  also use what your own claude settings allow, and what its list and
  stacked's `agent.tools` add (the stack's checkout's own, if it sets them:
  Settings, below), your MCP servers' tools among them: running in the
  repository, it has the servers you added there. What it was not let use
  is said in its stream (`⛔ not allowed: mcp__moz__get_bug: …`) and in its
  run's summary, with the list that lets its kind of ask use more
  (`agent.answer_tools`, `agent.change_tools`; for a build and test,
  `agent.tools`), to allow it if it should be; every ask, a change's too,
  is told to say so in what it writes, the tool not allowed, never that the
  service behind it (Bugzilla, Phabricator) refused. No worktree is made for
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

How hard the agent works on an ask is its effort, as claude's `--effort`
takes it: `low`, `medium`, `high`, `xhigh` or `max`. Every item that asks
the agent ends with the effort it asks at, `Effort [high]`: the comment
editor's `Ask agent` (and `Ask agent, answer only`, for a question on the
history), a comment's own `Ask agent`, `Ask agent about all`, `Send it now`,
the batch's items (below) and `Send it back to the agent`, in a review. The
items that open something to write first, `Ask agent about this, answer
only…` on the history and `Ask the agent to make a stack…`, end with
`Effort [high]` too: the menu of what you write then starts at the effort
picked there.
On an item with choices of its own, Tab goes to the effort, shift-Tab
goes back, and ← → pick in the one the menu is on; a click on `Effort
[high]` picks the next level. The effort picked last is where every next
menu starts, in every stack: stacked keeps it between runs, in
`~/.stacked/state.json` (Settings, below), and it is `high` until one is
picked. The tour keeps none.

An ask goes at the effort its item picked, whenever its turn comes: the
comment keeps that effort while it waits, ready or in the batch, and the
ask keeps it once it starts. claude is given `--effort LEVEL` after the
settings' `agent.args`, in place of an `--effort` they give, and the
stream's first lines say so (`starting: claude -p … --permission-mode
acceptEdits --effort high, in …`). A comment in the batch keeps the effort
it was asked at: its `Send it now` starts at that effort, and each of the
batch's items at the highest effort of the comments it sends. An ask asked
again goes at the effort it had: a change dropped because the work it went
on from was rejected, an answer asked again once a change is accepted, a
comment put back on the queue as it was. Work sent back from its review
starts at the effort it was asked at as well, unless another is picked on
`Send it back to the agent`. An ask queued by an older stacked has no
effort of its own: it goes at the agent's default, or at the settings'
`--effort`.

On the command line, `stacked send` takes `--effort LEVEL`. Without it, an
ask goes at the effort the editor picked last (`high` until one is), and
`--batch` sends each comment at the effort it was batched at, an ask of
several at the highest of theirs. `stacked review --send-back` takes
`--effort` too; without it, the work goes back at the effort it was asked
at. A level claude does not take is refused, with the levels it takes
said, and nothing is sent.

What each kind of ask may use is stacked's to give and yours to add to.
stacked always gives each what it needs: a change, git's history commands,
the commands that read, and its own `stacked fits` and `stacked export`,
none of which reaches the network, so that a change cannot touch a remote
(below); an answer, `Read`, `Grep`, `Glob`, `LS`, `git show`, `log` and
`diff`, and stacked's commands that read the stack; a build and test, any
command. What else each may use is a list of the settings' (Settings,
below): `agent.change_tools` for the asks that change the stack, nothing
unless set; `agent.answer_tools` for answer-only asks, `curl` and `jq`
unless set, as answers could always run them before there were lists; and
`agent.tools` for every ask, builds and tests too. `stacked setup agent`
sets the two lists up with you, as one list of tools with a box for each
kind of ask. Every ask is refused, too, what the stack's
checkout's claude settings of yours deny (`permissions.deny` of
`.claude/settings.local.json`), wherever it works: claude refuses it in the
checkout, and stacked has it refused in the agents' worktrees too, which do
not have that file, as git does not track it (`--disallowedTools`). What
the tracked `.claude/settings.json` denies is in the worktrees already.

No ask goes before that setup is saved, in the settings that apply to the
stack's checkout (the settings' file, or the checkout's own): `agent.setup`
says it is. In the editor, asking (`Ask agent`, `Ask agent about all`,
`Send it now`, sending the batch, sending work back from its review) opens
the setup first, the screen `stacked setup agent --tui` shows, over the
editor; while the agent is asked what tools it has, a box says so, and the
editor goes on. Saving the setup writes it in the stack's checkout's own
settings, `.stacked/local.toml`, and the ask goes; Esc sends nothing, and
the bottom line says why: sending again opens the setup again. On the
command line, `stacked send` (but `--later`, which only batches) and
`stacked review --send-back` refuse, saying to run `stacked setup agent
NAME` first. Once the setup is saved, it is not asked again, unless a later
stacked's setup is a new version of it (`agent.setup` says which was
saved). The tour's stand-in for the agent needs none.

Whatever it writes, a reply or a comment it opens, the agent may attach
references to it: lines of a file, a union of ranges (`120-140,200-210`),
or the whole file, as a patch leaves the file, as the stack's base has it,
or as the whole stack leaves it, which reaches the files the stack does
not touch too. Under the comment, a line for each: `⇢ a.txt:2-3, 10 · as
patch 1 leaves it`, `⇢ a.txt · as patch 1 leaves it` for the whole file,
which previews whole, its numbers plain. Enter on it previews those lines in a box, numbered, `⋯` between the
ranges; Enter again goes there: to the line in its patch, when the patch's
diff has it, else to the file whole, at that line, as it reads (Esc comes
back). In the box and in the file whole, the numbers of the lines it
points at are marked, drawn reversed. They are kept in the agent's turn as a `refs` block, a reference a
line (`a.txt:2-3,10 @1`; `a.txt @1` the whole file; `@0` the base, no `@`
the whole stack; a web link as it is, which is never read as a path), which
`stacked comments` shows and the agent reads back in its thread. The agent
may point at another comment of the stack too, by its label in the ask or
its id as `stacked comments` lists it: under what it wrote, `↗` and where
that comment is, Enter going there. It may point at a page of the web,
`{"url": "https://…"}`: under what it wrote, `⇗` and the link, which Enter
shows whole and then opens, as one you attached. The prompt asks it to
point at the stack first: whenever the code or the comments say what it
means, at those lines of a file, as a patch, the base or the whole stack
leaves it, or at that comment. A web link is for what nothing in the
stack, the repository or its comments says: a bug, a specification, a page
of documentation. The
references it attaches take numbers too, after those of the comment it
writes on, in the order it gives them (the prompt says the first free
one; a comment it opens numbers its own from `r1`), and one it gives that
is no reference (a comment that is none, lines that are none, a link not
of the web) leaves its number out, so that what names the others still
does. In what it writes, and in what you write after, `@r4`
names one of them as `@r1` names one you attached. They go with their
turn when a rewind takes it; when the turn is written again, it keeps
them, but one it named whose last mention the edit took away (Writing
comments, above). A turn deleted by stacked's first way of deleting one
counts nowhere: its references are not the comment's (not shown, not
listed in $EDITOR, not told the agent), and its mentions name nothing.

An ask goes now, or later, as in self-review's batch: under `Ask agent`,
`Send it`, `[x] now  [ ] later`, picks when, kept for the stack, and Enter
on it asks as the ask over it does, at the effort of the ask's line, which
then says so (`Ask agent later`). Asked later, a comment waits in the
stack's batch, marked `◇` and said `batched (may change the code)`; nothing
is started. Its menu has `Send it now (…)`, alone, as it was batched, and
`Remove it from the batch`; written again, it stays in the batch, to go as
it is then. The banner sends the batch (below), as you pick: its edits to a
single agent, in one ask, of the whole stack when they are on more than one
patch or one was batched so; its answers to a single agent too, in one ask
with the whole stack in view, or fanned out, an agent each, all at once;
each build and test on its own. Each of these items ends with the effort it
sends at, the highest its comments were batched at unless another is
picked. On the command line, `stacked send NAME ID
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

Chained (`agent.chain`, on unless changes work in your checkout), a change
does not wait for the review of the change before it: it goes at once, from
the stack that change makes, as if you accept it, one change running at a
time. What it brings back is reviewed after the work it went on from: the
banner lists it after that work, `◌ … after the edit under review`, and its
review does not open before (`stacked review NAME RUN` refuses too).
Accepted, that work makes it an ask of the stack as any other. Rejected,
sent back or edited by hand, what went on from it is dropped: its agent is
stopped if it still works, what it brought back is thrown away unseen, and
its comments go back to their turn, to start again from the stack as it
then is (a round sent back goes first). It reads the stack it went on from
with `stacked export NAME --work RUN` and `stacked show NAME --work RUN`,
the stack a landed work makes. An answer-only ask runs at once, alongside
the changes, on the stack as it is: accepting a change asks again the
answers made on the stack before it, still running or waiting for your
review. Asked again, an ask whose work had landed is shown that work, to
reuse what still holds: its replies, the comments it opened, and, for a
change, what it changed. A build and test is not asked again: it goes on,
of the stack as it was, unless you stop it as you close the review.
Closing a review, or sending the work back, says first what that asks
again, and offers to stop a build and test going.

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

To change commits below the top of the stack, the agent writes its rebase
plan (a `pick`, `edit`, `squash`, `fixup` or `drop` line for each commit)
to a file its prompt names, then runs a plain `git rebase -i`. The file is
`$TMPDIR/stacked-todo/…/REBASE_TODO`, one for each worktree, outside the
git directory, which claude may not write in; stacked lets claude write
there (`--add-dir`), has `GIT_SEQUENCE_EDITOR` copy the plan in, and sets
`GIT_EDITOR` to `true`, so that no editor opens. That is how self-review has
its agent rebase. Claude refuses a command that starts by setting a
variable (`GIT_SEQUENCE_EDITOR=… git rebase -i`), as it refuses any command
that no rule allows.

The plan's folder is named after the worktree's git directory: its name is
the first 12 hex digits of the SHA-1 of that directory's path, and a file
in it, `gitdir`, names the directory in full, as the hash cannot be read
back. The folder is removed once the ask lands, failed or not, unless a
rebase is still under way in its git directory, or another ask that is
running writes its plan there. Each time an ask that may change the whole
stack starts, stacked also removes every folder whose git directory is
gone, unless an ask that is running writes its plan there. A folder that
names no git directory, as an older stacked made them, is removed once
nothing in it has changed for a day. `$STACKED_TODO` names another folder
to keep the plans in, in place of `$TMPDIR/stacked-todo`.

What a change may run reaches no network, unless what its list
(`agent.change_tools`) adds does: stacked ticks nothing in that list's
column of the setup, and the setup says which of the agent's tools can
reach the network (Bash at large, `WebFetch`, `WebSearch`), as a change
that reaches it could touch a remote. It may run git's history
commands (`status`, `log`, `show`, `diff`, `add`, `restore`, `reset`,
`commit`, `cherry-pick`, `apply`, `checkout`, `switch`), `git rebase` with
its plan (`-i`, and `--continue`, `--skip`, `--abort`), git's settings to
read them (`git config --get`, `--list`), and commands that read (`grep`,
`rg`, `find`, `cat`, `sed`, `sort` and the like). It may not set git's
settings (`git -c`, `git config` otherwise), as one can run a program, and
stacked has claude refuse what would make the rest run another program
(`--disallowedTools`: `git rebase --exec`, `find -exec`, `rg --pre`, `sort
--compress-program`), as claude's own refusals win over what it allows. The
plan is checked before git takes it: one with an `exec` line is refused, and
the rebase is not started. Claude's rules are what it is let do, not a
wall around what it runs: for that, claude's sandbox.

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

When a change lands, stacked reads the agent's commits back as mail (`git
format-patch`) and matches each commit to the patch it comes from: by the
commit stacked made of that patch, if the agent left that commit as it
was; else by the effect of its diff and its title, then by that effect
alone, then by its title alone. A commit matched so is a new version of
its patch, unless it has the patch's message and author and a diff with
the same effect: then it is the patch itself, unchanged, its diff kept as
stored, and no new version is made, whatever date the commit has. Two
diffs have the same effect when they change the same files, in the same
order, with the same paths and modes, in the same hunks, each hunk
holding the same lines, kept, added and removed. What git writes
differently once lines have moved does not count: the line numbers in a
hunk's header, the heading git writes after its second `@@`, the blobs an
`index` line names, and the white space a header's line ends with. So
when the agent rebases the stack to reword patch 2, patch 3 keeps its
version even when a later version of patch 1 moved the lines that patch 3
changes, and git now writes patch 3's hunks with other numbers: only
patch 2 is a new version (`~`). A commit with a line added, removed or
kept that its patch does not have, a file more or fewer, a mode changed,
or its hunks split or merged otherwise, is a new version.

git does not give every message back as stored: `git am`, which puts the
stack into the agent's worktree, keeps no empty line twice in a row, no
white space at the end of a line and no carriage return, and takes a
prefix and runs of spaces out of a title; `git format-patch` quotes a
name that holds a comma or a dot. So stacked also reads back the commits
it made of the patches, as it reads the agent's, and a commit whose
message or author is what git gave back of its patch's has the patch's
own, as stored: the agent left it as it was. The title git gave back
finds the patch as its own does. A message the agent changed otherwise,
if only in its white space, is the commit's: a new version of the patch,
reworded when its diff has the same effect. So when the agent rewords
patch 2 of a stack whose messages have two empty lines under their
titles, the patches below it and above it keep their versions, and only
patch 2 is a new version.

A diff kept as stored is checked before the change lands. git puts a
hunk where the hunk's lines are nearest to the line its header names, so
a diff stored before its lines moved could go to another copy of the
same lines in the file, and a hunk the agent moved to lines like its own
would go back to where it was. So stacked applies the patches it would
land, the diffs kept as stored and the new versions, in turn on the
commit that the agent's commits start from, in an index of the check's
own, nothing written in the repository; beside it, in another such index,
it applies the agent's commits as it read them back. After each patch
whose diff is kept as stored, the files that the patch touches must be
the same in both. A patch whose files are not the same is a new version
after all, its diff the agent's commit's, and the check goes on up the
stack from there. When the check cannot be made, every patch that would
keep its diff as stored is a new version instead. `stacked import NAME
RANGE --replace` matches the commits it imports to the stack's patches in
the same way, their messages and authors compared as they are, as
stacked did not make those commits, and checks the patches it keeps
against those commits on the stack's base.

Where it stands shows on the comment's first line, or under it when that is
too long: ready (and on what), with the agent (and the last lines of its
stream under the comment's turns, over its references: what it says and
runs, and while it thinks, one line
counting it, `✻ thinking · ~688 tokens`, with the words of it when claude
gives them), agent replied (changed, answered, tested), failed (and why),
moved. Until the agent writes its first line, one line says what is being
run to start it, as it runs: `starting: git worktree add --detach … BASE`
(the stack's own worktree, made the first time, which takes a while in a
big repository) or `starting: git switch --detach BASE, in WORKTREE` (taken
again), then `starting: git am --keep-non-patch, 3 patches` (the stack, up
to what it is asked about); then `starting: claude -p … --permission-mode
… --effort LEVEL, in WORKTREE` (or the repository, for an answer as it is
by default, or a build and test). They stay
at the top of its stream, as `$ git worktree add …`, in the stream, in
`Runs…` and in `stacked transcript RUN`. Enter on a comment the agent is
working on opens its stream in place; esc folds it, and so does the agent
being done, the bottom line saying so. The comments pane marks each (`●` with the agent, `◇`
batched, `◌` ready, `◆` its reply waits for your review, `✓` replied, `✗`
failed, `↑` moved), its header counts them (`4 comments · 1 with the agent ·
1 to review · 2 to read`), and the top bar too (`agent ●2 · batch:3 ·
ready:1`).
`Show…`, on either header, picks what both panes show, each with its
count: all, to send, to review, to read (any, from an answer, from a change), in
progress, failed or moved, gone (on a line the patch no longer has),
closed, none. It is kept per stack, in the file. A comment you are done
with is closed, from its menu (`Close this comment…`): it leaves the
comments shown. When the cursor is on the comment closed, the cursor goes
to the line the comment was on (a line of the code or of the message, the
file's name, or the patch's title), on the same line of the screen; a
comment on the stack, or on a line the patch no longer has, leaves it on
the row over the comment. A comment that `Show…` hides does the same. When
a comment is closed from elsewhere (the comments pane, the banner, another
stacked), the cursor stays where it is, as it does whenever other comments
come, go or change: hidden, moved to the stack, answered, written on, or
opened again. The comments pane then picks the next comment, or the one
before it when the comment closed was the last. A comment closed is not
lost, as the file only grows: `closed` shows each
as it was, `closed @12` on its first line, its menu (which Enter opens) to
`Reopen it` or to read its history. The agent's comments moved to the stack
are not closed, nor what is moved back. The stack's menu, on its header,
has `Close all comments…`, every open comment of the stack, its own and its
patches': it asks first, saying how many it closes (`close all 3 open
comments of the stack?`), and how many stay open, those with the agent or
whose reply waits for your review, as no comment is closed while so.

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

`Stop the agent…`, in the menu of a comment the agent works on, and on an
ask that runs in `Runs…` (Delete does the same in both places), asks
first. Once you say so, the agent's process group is sent SIGTERM, and
SIGKILL if the agent has not exited three seconds later; the ask then
lands as failed at once, and what waited behind it goes. Meanwhile the bar
says `stopping the agent…`; once the ask has landed, the status line says
that the agent was stopped and its ask landed as failed. The comment is
free again: its stream is folded, the comment is marked failed (`✗`,
`ask failed: you stopped the agent`), and its menu offers everything
again. The run's summary says that you stopped the agent, and its stream
ends with `■ you stopped the agent`. `stacked stop RUN` does the same from
the command line, RUN being a run as `stacked runs` lists it, or the start
of its id. A build and test stopped as a review closes lands the same way.

An ask whose agent is gone without landing it (killed by something other
than stacked) lands as failed on its own: the editor looks for such asks
every few seconds, `stacked send --wait` as it waits, and `stacked runs`
before it lists the runs. The editor also waits for each process it
started for an ask once that process has exited, so that none is left a
zombie, which would look as if its agent still ran.

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
test's findings on each patch's title). An ask that brought none of them
back waits for your review all the same while a comment it was asked
about is still open: its review has no part, only those comments, to
close or keep. If every comment it was asked about was closed or deleted
meanwhile, it is not held, and its run says that the agent brought
nothing back. You accept or reject each part. A
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
before you take its reply. When the work the reply came with holds
nothing but replies, Enter on the comment takes the reply into its thread
at once, accepted, out of the review (its menu's `Accept the agent's
reply`), and the review closes by itself once the last of those replies
is taken. The comments the agent was asked about stay open, as any comment
does, to reply to or to close. A reply that goes with a change of the
stack, or with comments the agent opened, is reviewed with them: Enter on
the comment opens the review of that work, on the reply. A reply of work
that goes on from an edit under review waits until that edit is reviewed.
No other review closes by itself: the review of an ask that brought
nothing back waits until you close it.

In the editor, a banner under the top bar says what waits on you on the
stack, in a line: first, while the commit messages of some patches differ
from their commits' in your workspace, that they are to sync before
anything else (`✉ the commit messages of patch 2 differ from your
workspace's: sync them first`, The editor, above); what waits for your
review (`⟳ 2 landed asks wait for
your review · the next: a change of patch 2, 1 reply, asked "…"`), a pull
from Phabricator under way and the revision it reads (`pulling the reviews
from Phabricator: D12, revision 2 of 3…`), what the asks batched (`3
batched asks`), what the agent works on (`the agent works on 1 ask`), the
comments that wait their turn (`1 comment waits its turn`), and how many
comments are open (`5 open comments`); it is there while any of it is. Landing writes nothing
on the bottom line. Esc goes to the banner from wherever the cursor is in
the panes, and ↑ from the stack's header goes there too; a click on it as
well, but not the mouse going by. With the cursor on it, the banner opens,
a line for each, in sections under their titles, and the cursor is in
none of the panes: what each has picked shows as in a pane the cursor is
not in (`picked`), not reversed. First, while there is one, `commit
messages, to sync first`: `✉ Sync the commit messages of patch 2: the
stack and your workspace differ`, which Enter opens the sync of, and until
then no line of what landed opens. `read only, to
review`: `⟳ Review …` for what landed and changes nothing (an answer, an
ask that brought nothing back, the comments pulled from Phabricator), each
said by what is left of it to review, the first landed first, then `◌ … after
the edit under review` for those going on from it, then, while a pull from
Phabricator is under way, `⇣ Pulling the reviews from Phabricator: D12,
revision 2 of 3…`: what it brings waits there once it is done. `edits, to
review`: the
change of the stack you review now, then those going on from it, which you
review after it. `open comments`: while there is a batch, `◇ Send the
batch… 2 edits, 1 answer only`; then every comment open on the stack, the
most recently updated first, marked as the comments pane marks it (`●` with the agent,
`◇` batched, `◌` waiting its turn, `◆` its reply waiting for review, `✓`
answered…), with where it is, how it starts, and where it stands (with the
agent, the last line it wrote). ↑↓ pick one, and Enter opens the sync of
the commit messages, reviews what landed (once they are synced), or goes to
the comment; on the batch, it opens the banner's menu, all of
it, which Space opens from any line: the sync of the commit messages, while
there is one, its items for what landed grayed until then; what landed, to review; the batch, to
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
are accepted; kept again, they are as they were. With no part, as when
the agent brought nothing back, the list has only those comments, and
`Close review` closes or keeps them as marked. The sections are the stack
operations of the change (a line for each patch: `noop`, `add`, `modify`,
`reword` when its message alone changed, `move`, `remove`, and the base
when it is rebased), the comments it opened, and its replies. On the right
is the part the cursor is on, as it would be:
the stack diff of the change, from the stack as it is, each new version's
interdiff under it, its message compared as the sync of the commit
messages compares two (History, below), so that a paragraph the work only
wraps otherwise is no change, nor a line to read again; or the comment
where it would be, the reply as the last turn of its thread. → goes into
it to scroll, → again makes it wide, the list out of sight, and ← narrows
it, then comes back to the list (too narrow for both side by side, →
shows it wide at once). On the stack
operations, → goes into the stack diff, the cursor on the first line that
changed of the patch the cursor was on: ↑↓ move it from line to line, and
Enter on a line of a patch, or of what changed in it, shows that patch as
the work leaves it, a line at a time, the cursor on that line, and the
work's comments under their lines, no other comment of the patch: those it
was asked about, its replies in their threads, those it opened, and yours
written in the review; ← goes back to the stack diff. Enter accepts the
part the cursor is on, or puts it back to review if accepted (rejecting is
Space's); every line of the stack
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
`Accept all remaining and Close` and `Reject all remaining and Close`, in
a section of their own, mark what is left and close once you say so (the
question says what is left, the menu on `Keep reviewing`), while
something is left: once every part is marked, they are grayed. Over them,
`Asking comment  [x] keep  [ ] close` picks whether the comments the agent
was asked about are kept open or closed as the review closes, every one,
picked as the review has them: ← → or Enter pick the other, the menu
staying. Under it, while HEAD of your checkout can move safely to the
stack as the review leaves it, `HEAD  [x] sync  [ ] no sync` picks whether
HEAD moves there once the change is accepted: detached at the stack's
top, the branch it was on as it was, as `Move HEAD…` moves it, the status
line saying where it went. HEAD can move safely when it was in sync with
the stack (on its top, each patch a commit with the patch's message, its
`Differential Revision:` trailer too, no commit above), nothing is
uncommitted in the checkout, and no agent works
there or makes its changes there (`agent.worktree = "pwd"`). `sync` is
picked unless you picked otherwise in this review, and the pick is not
kept for the stack. A reply that goes with a change of the stack
opens that review, on the reply, from its comment (Enter, or `Review the
agent's work…` in its menu).

What you write in the review of the agent's work sends that work back to
the agent. In the patch as the work leaves it, ↑↓ move from line to line,
and Enter writes a comment on the line in the inline editor, as in the
main view: on a comment, what you said last written again if the last word
is yours, a reply if not, and your answer to the agent's reply if this work
replied to it. Where Enter writes a new comment, on a line or on a file's
name, Space has `Comment on this line` (`Comment on this file`), which
writes the same comment, here or in $EDITOR as the item's choice picks, as
in the main view. On a reply in the list, Space has `Reply to it`, your answer
to follow the reply in its thread (`Edit your answer to it` once written).
Nothing written there asks the agent at once, and the list shows it under
`what you wrote, to send back`: on one of its lines, the right pane shows
it where it would be, Enter writes it again, and Space has `Take it back`
(`Take your answer back`), for it not to go back with the work. With any
of it, the work is not accepted:
Esc has `Send it back to the agent` in place of closing the review. The
work is rejected, its change not applied; its replies go in their threads,
your answers after them, and the comments it opened go as you marked them.
Then the comments it was asked about whose last word is yours, and what you
wrote in its review, are asked again, in a round that starts from the stack
this work makes: the agent's worktree holds the change it made, and the
agent is told it is another round. As a round asked again after its work
was dropped is told, it is told what it changed in the round sent back (a
stack diff from the stack that round went on from) and the comments it
opened then, and `stacked export --work` prints the stack it made for it;
its replies are in their threads already. Its session is not the one
before: claude starts afresh each round. The round goes at the effort the
work was asked at, which `Send it back to the agent` ends with, unless
another is picked there. That round's work lands for review in
place of the one sent back. The round goes before anything else waiting
for the agent, and the stack is not edited by hand until you have reviewed
its work: nothing changes the stack it goes on from.

The change of the stack is edited by hand from its review, as the work
leaves the stack, where the cursor is, as in the main view. On a line of a
file, in the patch looked at closer or in the stack diff (where the line
is the one Enter goes to), Space has `Edit a.txt here in $EDITOR`, which
opens the file as the patch leaves it at that line; on a line removed, the
file opens at the line after where that line was. Where it is edited is
the item's choice, as for `Edit … here in $EDITOR`. `Edit this patch in
$EDITOR` opens the patch the cursor is in as mail, at the line of its
message or of its diff that the cursor is on, else at its top; on the
list, the patch is the one of the list's line. `Edit the stack as a list
in $EDITOR` is there too. An edit that is not taken opens again at the
same file and line with `Edit it again`.
Saved, that work is rejected, and the work with your edit in it lands in
its place: its replies and the comments it opened go with it, as you had
marked them, and its change is to review again. The review goes on there,
its title saying what you edited (`then you edited a.txt in Bug 1`). The
review of an edit by hand has the same, to edit it again. On the command
line, `stacked edit NAME PATCH --work RUN` edits the patch as that landed
work leaves it.

On the command line, `stacked landed NAME` lists the queue: each landed ask
by its run, what is left of it to review (with no part left, the comments
the agent was asked about: `1 asking comment to close or keep, nothing
brought back`), and each part of it. `stacked review NAME [RUN]
--accept-all` (or `--reject-all`) closes the review of that ask, the first
landed if no run is given: the parts not marked yet are accepted (or
rejected), and what is accepted is applied; `--close-asking` closes the
comments the agent was asked about too. In the review of landed work RUN,
`stacked comment NAME --patch N --line … --work RUN TEXT` writes a comment
on the patch as that work leaves it, and `stacked reply NAME ID --answer
TEXT` answers the agent's reply to comment ID; then `stacked review NAME
--send-back` sends the work back to the agent with them, at the effort it
was asked at unless `--effort LEVEL` says otherwise (`--accept-all`
refuses while you wrote in it). `stacked send NAME ID --wait`
says what waits for your review once it lands; `--wait --accept` accepts
all of it as soon as it lands, for a script.

## Reading

`2` switches to reading, as self-review's view of what is left to review:
in the middle, on the code, Space marks the block at the cursor read and
goes on to the next line not read, and Delete (or Backspace) marks it
unread again (no menu in the code meanwhile: the other panes still have
theirs). On a comment's rows (its head, its turns, the agent's stream, its
references), the keys are those of commenting: Space is the comment's
menu, Delete closes the comment, and nothing is marked read there; the
margin then shows no mark to make. Enter does what it does when
commenting: on a line it comments, on a comment it replies, on a reference
it goes there, and where it has nothing else to do, it opens the menu.
Going on from above
the middle line of the screen, the cursor goes down to that line where it
is, the code staying where it is, but no further than the middle line:
the code comes up from there. From the middle line or below it, the
cursor stays on its line of the screen and the code comes up under it.
So it is in every view, past the end of the patch if it takes that. A block is the
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
hunk's header reads the hunk, on a file's the file. Delete takes back the
latest mark holding the line or its block, in this hunk, and says so when
nothing there is marked. The
margin shows what Space would mark (`○→✓`), or else what Delete would unmark
(`✓→○`), and `✓` beside each line read, drawn dim; hunks, files, the
patches on the left and the top bar count what is read (`reading 5/11`),
and each file listed under a patch on the left counts what is left of it
(`7 left`).

A patch's message is read as its code is, its lines lines to review too,
counted on its header (`✉ message   read 1/3 lines`) and with the rest: a
paragraph at a time, its lines between empty ones, its title a paragraph of
its own; Space on a line of it marks its paragraph, once more the whole
message, as Space on its header does; Delete takes back the latest mark
there. Its empty lines need no reading, as a changed line of white space
does not. A line of it reworded in a new version is to read again; a
paragraph that a new version only wraps otherwise, or whose lines end in
other white space, is read as it was, as its interdiff shows it the same
(History, below). For that, a mark holding every line of a section of the
message, as the sync of the commit messages cuts a message into sections,
holds the section too, by what it says however it is wrapped: each
version of the patch that has the section has it read, and Delete on a
line of it there takes that mark back.

With the `moz-phab` feature (on by default), the trailer moz-phab writes to
name the patch's revision needs no reading either, as an empty line needs
none. That trailer is a line of the message's last paragraph, after an
empty line, that is exactly, once trimmed, `Differential Revision: `, then
`http://` or `https://`, a host, and a path ending in `/D` and the
revision's number, as in `Differential Revision:
https://phabricator.services.mozilla.com/D123456`. A line that only comes
close is a line to read: one that says more than the link, one with no link
or a link that ends otherwise, the same line in the title's paragraph or in
the middle of the message, and a line of the diff that says it. Built
without the feature, the trailer is a line to read as any other.

A changed line that is empty or white space alone needs no review, nor do
the message's empty lines and moz-phab's trailer, unless
`whitespace_needs_review` (in `[view]`) is `true`: Space marks no such line
as one to read, the cursor going on to the next line not read passes it
over, no count of lines read or left to review holds it, and it is drawn
dim, with no check beside it. A mark of the block it is in still holds it,
so with the setting `true` it is read as the rest of its block is. A line
whose code is re-indented is not one of them: it has code to read.

The line git writes under a file's last line when that line has no
newline, `\ No newline at end of file`, is not a changed line: Space marks
no such line, and no count of lines read or left to review holds it, so
that a patch read whole shows its `✓` whatever its files end with.

Marks are records in the file, kept by the lines' content: a new version of
the patch keeps them on the lines it did not change, and what it changed is
to read again. With each file whole (`3`), the marks are on the same lines,
by the same keys, and a block is read in its hunk as it is in the hunks: a
line in no hunk takes no mark, and Space there goes on to the next line not
read, the status line saying why. The blocks are found from the files as
each patch leaves them, in an index of stacked's own (as the check does),
in a thread: the screen does not wait.

On the right of the top bar, how far the review of the stack is: the lines
its patches change that are not read yet, of all the lines they change,
and the comments open on it (`24/120 lines to review · 3 comments to
close`). With less room, less of it (`24/120 lines · 3 comments`); with
all read and no comment open, `nothing left to review`.

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
patch's nearest line of that file, or with each file whole, that line), or
to the record in the history. Esc goes back where the search was opened.

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

To search only what the middle shows, without leaving it, press `/` (The
editor, above): the cursor goes from match to match in the middle itself.

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

  What changed in a new version's mail, its interdiff, is the two mails
  diffed, with three lines around each change. Their headers, and their
  diffs after the `---`, are diffed line by line, a line almost the same as
  the one it took the place of showing what changed in it as words. Their
  titles, then the bodies of their messages, are compared as the sync of
  the commit messages compares two messages (The editor, above). A section
  the same in both, a paragraph only wrapped otherwise or ending its lines
  in other white space, is there once, as the new version writes it, among
  the lines left as they were: none of its lines is a change. A section
  that differs is the old version's lines taken out, then the new one's put
  in, each version with its own line breaks: what only one of them has is
  in the colours of `removed_word` and `added_word`, the words both have
  are drawn as any text, each line's sign is in the colour of its line,
  and a break only one has is marked as the sync marks it, `↵` at the end
  of a line of code, `¶` on an empty line. Code is compared line by line,
  so that a command split over two lines in one version and on one line in
  the other is a change. With the title and the body the same, the mails
  are diffed line by line. The interdiffs of the review of the agent's
  work, of `What this edit changed…`, of `Versions of this patch…` and of
  `stacked interdiff` are made the same way.
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
and `Ask agent, answer only`, in the menu Esc opens, asks it at the effort
its line ends with: answer only, so the agent changes nothing. It is told the
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

`~/.stacked/config.toml` holds the settings, in stacked's own folder,
beside what stacked keeps between runs (`state.json`: the databases opened
last, StackED above; the searches of the middle, The editor above; and the
effort last picked in a menu, Asking the agent below);
`$STACKED_HOME` names another folder than ~/.stacked, and `$STACKED_CONFIG`
another file. A checkout's own settings, `.stacked/local.toml`, override
them there (below); each is optional, and `stacked config` prints them all
as they are, in the file's form, with what is wrong with the file. As `git
config` does, `stacked config KEY` prints one (`view.layout`, `keys.down`:
each value on a line of its own), `stacked config KEY VALUE…` writes it in
the file (several values for a list: `stacked config keys.down down j`),
the file and its folder made if they are not there, and `stacked config
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
  every run, but an `--effort` they give, which the ask's own effort takes
  the place of (Asking the agent, above); `worktree` and
  `answer_only_worktree`, where a change and an
  answer work: `pwd`, `ondemand` or `precreate` (Asking the agent, above;
  `ondemand` and `pwd` unless set); `model`. What each kind of ask may use
  besides what stacked gives it (Asking the agent, above), as claude names
  the tools for `--allowedTools`: all of an MCP server's
  (`mcp__bugzilla__*`; there is no way to name every server's at once), one
  of its tools (`mcp__bugzilla__get_bug`), a command (`Bash(jq *)`, each
  command of a pipe needing its own). `change_tools`, for the asks that
  change the stack: nothing unless set. `answer_tools`, for answer-only
  asks: `["Bash(curl *)", "Bash(jq *)"]` unless set, which answers could
  always run before there were lists; taken out, an answer is no longer
  told it may read the web with curl. `tools`, for every ask, builds and
  tests too, as before there were lists: an answer may use them too, so give
  only tools that read. `setup`, the setup of the two lists done, as the
  version of it that was: until it is, no ask goes (Asking the agent,
  above).

  `stacked setup agent [NAME]` sets the two lists up with you, and writes
  `setup` once you save them. It starts the agent in the stack's repository
  (else where it is run), where it has your MCP servers, and stops it as
  soon as it has said what tools it has. Both lists are shown as one list
  of tools, a row each, with two boxes under the columns' headings:
  `read only`, for answer-only asks (`answer_tools`), and `change`, for
  the asks that change the stack (`change_tools`). The boxes are ticked as
  the settings say, else as stacked has it: nothing for changes; for
  answers, `curl` and `jq`, and what the checkout's own claude settings
  allow, as you allow it yourself, but for what changes things. First comes
  what stacked always gives, which does not change: `always` in the column
  of each kind of ask that has it, greyed, with nothing to tick. Then the
  list holds what the agent says it has (each MCP server, its tools all at
  once or one by one, and the agent's other tools, but those stacked gives
  every ask, said `one of the agent's tools`); what the checkout's own
  claude settings allow (`permissions.allow` of
  `.claude/settings.local.json`, yours, then of `.claude/settings.json`;
  their `deny` is left alone), each rule as written, but for a command's
  older form (below), said `allowed in .claude/settings.local.json` (or in
  both files, if both allow it): claude applies them when it runs in the
  checkout, but an agent working in a worktree of the stack's own does not
  have the first, which git does not track; what the checkout's skills let run, unticked,
  as several skills change things (they commit, rewrite history, file
  bugs); and the programs the checkout's instructions to the agent name,
  unticked, as stacked cannot know what a program does. A skill's rules
  are the `allowed-tools` of the header of its
  `.claude/skills/NAME/SKILL.md`, on that line (a space or a comma between
  each) or listed under it, each as written, but for a command's older
  form, said `allowed by the skill NAME` (or by the skills that allow it,
  several of them); what is not a rule (`[Agent]`) is left out, and a skill a link
  takes out of the checkout is not read. Those instructions are `CLAUDE.md`,
  `.claude/CLAUDE.md`, `AGENTS.md` and `AGENT.md` at the checkout's top, and
  what a `CLAUDE.md` imports (`@path`), one level down, inside the checkout:
  a link or an import out of it, as to `~/.claude`, is not read. A program
  is the first word of what they say in code: a span, a command of a block
  of shell (past its prompt, `$ `, and what it sets first, `A=b`), or a line
  of an unmarked block. It is listed as `Bash(PROGRAM *)`, said `named in
  AGENTS.md` (or in each of the files that name it), if it is there to run,
  on PATH or in the checkout (`./mach`);
  a block of shell's that is not on PATH is listed too, said `named in
  AGENTS.md, not found on PATH`, to install and tick if it should be. What
  stacked gives already (git, grep, rg, cat…), shells and their builtins
  (cd, echo…) and what stacked refuses are left out, of the instructions,
  the skills and the claude settings alike. Last come what the settings let that
  none of these lists, said `in the settings`, to keep or take out.

  Rules that mean the same to claude are one row: a command's older prefix
  form, `Bash(searchfox-cli:*)`, and the newer, `Bash(searchfox-cli *)`,
  which claude matches alike (the command alone, or followed by a space and
  more); all of an MCP server's tools, `mcp__SERVER` and `mcp__SERVER__*`;
  and a rule that several places give. The row says where each comes from
  (`allowed in .claude/settings.local.json; allowed by the skill
  spec-trace; named in AGENTS.md`), and its boxes are ticked as for any
  row: as the settings say, else as stacked has it for any of those
  places. A command is shown and saved the newer way, which reads better,
  however its places write it: `Bash(searchfox-cli *)`, in each list it is
  ticked for. A `Bash(cmd:*)` that stacked's own settings hold (`tools`,
  `answer_tools`, `change_tools`) is rewritten so the next time the setup
  is saved, as claude reads the two alike; your claude settings are never
  written. All of an MCP server's tools are written as the settings write
  them, if they do, else as all their places do, if they agree, else
  `mcp__SERVER__*`. Any other rule is shown and saved as it is written.
  Nothing else is the same: a narrower rule is never merged into a wider
  one (`Bash(./mach file-info:*)`, shown `Bash(./mach file-info *)`, and
  `Bash(./mach *)` are two rows), nor the command alone into the command
  and more (`Bash(make)` and `Bash(make *)`), nor the older form of a
  command that holds a `*` into the newer (claude reads the `*` of
  `Bash(git * push:*)` as it is written, so that rule stays as it is).

  Of the agent's tools, the list says which can reach the network, for a
  change (Bash at large, `WebFetch`, `WebSearch`), and which change things,
  for an answer (Bash at large, `Edit`, `Write`…). The row of claude's
  `Skill` tool, `claude's tool for the checkout's skills`, is there whether
  the agent says it has the tool or not, unticked unless the settings say:
  its read-only box gives answer-only asks the skills, written in
  `answer_tools` as `Skill`, and the lines under it say what that lets in:
  a skill may run what its own allowed-tools let it, commands that change
  things too (rewriting history, filing a bug), and an answer runs in your
  checkout itself unless `answer_only_worktree` says otherwise. Its change
  box gives the skills to changes, written in `change_tools`. Whether what
  stacked refuses still holds against what a skill's allowed-tools let it
  run, claude's own text does not say, so stacked does not claim it. What
  the checkout's claude settings of yours deny is said above the list,
  `denied in .claude/settings.local.json, refused to every ask`, each rule
  once however it is written, to read, not to tick: every ask is refused it
  (Asking the agent, above). A tool of `tools` shows ticked in both
  columns; taken out of one, it leaves `tools` and goes on in the other
  list alone. A command or a tool of your own is added as a row, typed as
  claude writes a permission rule: a tool (`WebFetch`), a tool and what it
  may do between parentheses (`Bash(make check *)`,
  `WebFetch(domain:example.com)`), all of an MCP server's tools
  (`mcp__SERVER__*`, `mcp__SERVER`) or one (`mcp__SERVER__TOOL`); what does
  not read as one is refused, and said why (`make check is not a tool's
  name: a command is let run as Bash(make check)`). The row comes last,
  said `added here`, neither box ticked, the cursor on it, for you to tick
  what should use it; a rule that a row lets use already, however it is
  written, is that row, as it is. Saving writes both lists, `tools` if it
  changed, and `setup`, in that checkout's own settings (below; with
  `--global`, in `~/.stacked/config.toml`, for the checkouts without their
  own), the rest of the file as it was; saved unchanged, the setup is done
  all the same. Outside a checkout, only `--global` can be written.

  Asked line by line, the setup shows the list, a line a row as the screen
  draws it, to keep as it is (Enter) or to go through a row at a time. Each
  row is one question for both kinds of ask, `Which asks may use Bash(jq
  *), stacked's default for answers? [R: read only, c: change, b: both, n:
  neither]`, the capital letter being what Enter keeps; an MCP server's
  tools are asked all at once, or each in turn (`e`). Then it asks for a
  command or a tool to add, until a line is left empty, and asks the same
  question of each. With `--tui`, and in the editor before the first ask,
  the setup is a screen of its own, with what saving writes, in the file
  it writes, at the bottom; once the list scrolls past the columns'
  heading, the heading stays on its first line. ↑↓ pick a row; ← and →, or
  Tab and shift-Tab, the column; Space ticks the box of the row picked in
  that column, or not. In the browser and in StackED, a click on a box
  ticks it or not, a click on `+ add a command or a tool…` opens the line
  to type one in, and the mouse over a box picks its row and its column.
  `a` (or Enter on `+ add a command or a tool…`) opens that line too, and
  `s` saves. Esc (or `q`) is as in the comment editor: with something
  changed, a menu, `Save it`, `Throw it away`, `Keep choosing` (Esc there
  keeps choosing too); with nothing changed, out, nothing written. Ctrl-c
  leaves at once, nothing written.

  `worktrees`, where the agents' worktrees go, else `.stacked/` at the top
  of the stack's repository. `schema`, whether the agent gives its answer
  as structured output, checked against what the ask asks for (claude's
  `--json-schema`: its replies by the comments' labels, an answer's
  comments, a build and test's findings by the patches' numbers; on
  unless set): off for an agent that does not take it, its answer is read
  from what it writes, the JSON it ends with, the prose around it left
  out. Given up on by claude, the ask fails and says so.
- `[reviews]` (built with the `moz-phab` feature, on by default; read
  either way, so one file serves both builds): `source`, `"phabricator"`
  or `"none"` (no pulling, no importing); `url`, the Phabricator a bare
  `D123` trailer is on; `arcrc`, where the tokens are; `token`, one for
  `url` instead; `trailer`, the trailer naming a revision
  (`Differential Revision`).
- `[view]`: `layout` at the start (`unified`, `side-by-side`, `compact`,
  `unified-split`);
  `reading`, to start reading; `whitespace_needs_review`, whether changed
  lines that are empty or white space alone, the message's empty lines and
  moz-phab's trailer need review in the reading mode (`false`: they need no
  mark, and no count holds them); `patches`, the
  patches pane at the start, `"expanded"`, `"files"` (the files of the
  patch picked listed under it; `"compact"`, as versions before the files
  wrote it, is read as them) or `"hidden"`; `comments`, the comments pane
  at the start, `"expanded"`, `"compact"` or `"hidden"` (for both, `true`
  and `false`, as before, are expanded and hidden); `stream_lines`, the
  agent's lines under a comment;
  `split_width`, the columns two sides, or two halves, need; and
  `message_width`, the columns the menus wrap a commit message's long
  lines at (Editing a patch by hand, above): 72 unless set, and 20 at
  least.
- `[colors]`: what paints each part of the screen, as git writes a colour
  (`git config color.diff.new "green bold"`): a foreground, then a
  background, each a name (`black`, `red`, `green`, `yellow`, `blue`,
  `magenta`, `cyan`, `white`, any of these after `bright`, `default`,
  `normal`), a number from 0 to 255, or `#rrggbb`; and any of `bold`, `dim`,
  `italic`, `ul`, `blink`, `reverse`, `strike`. `added` (`green`), `removed`
  (`red`), `added_word` (`green 22`) and `removed_word` (`red 52`, on a
  dark tint): what changed in a line almost the same as the one paired with it,
  `hunk` (`magenta`), `file` (`cyan bold`: a file's name, the
  titles of boxes, the history's records), `meta` (`blue`: index lines,
  borders), `header` (`cyan bold`: each pane's first row); `you` (`172`),
  `agent` (`134`) and `reviewer` (`32`): the comments and turns of each,
  their bar and name; `note` (`yellow`: what a menu says under its items, the
  tour's box, the comment editor's frame, a run going); `bar` (`brightwhite
  blue`), `warn` (`black yellow`: the top bar when the stack cannot change),
  `status` (`black brightwhite`: the bottom line), `cursor` (`reverse`: the
  line, the item or the pick the cursor is on, in the terminal's own colours
  reversed, white on a dark theme; the cursor looks the same on every line
  whatever the line's own colours, and a line read is not dim under it, but
  what changed in a line almost the same as its pair keeps its tint under
  the cursor), `picked` (`normal 238`: what a pane has picked while the
  cursor is in another, on a grey background, the cursor's own pane drawing
  its pick as `cursor` paints it) and `code` (`252
  236`: code in what a comment says, `code` in a line and code blocks, light
  grey on a dark grey). `stacked config colors.you 208` sets one.
- `[keys]`: for each thing a key does, its keys: `down = ["down", "j"]`,
  `menu = "m"`, `find_next = "ctrl-n"`. Each thing is read in one
  context, where the editor reads its keys, and the keys are checked
  context by context: two things of one context on one key clash, which
  is said as what else is wrong is (below), and the first does it; one key
  can do a thing in each context, each read where it is. The contexts, as
  `stacked config` lists them: `screens`, the screens and the menus over
  them, wherever nothing is typed (`up`, `down`, `left`, `right`,
  `page_up`, `page_down`, `home`, `end`, `enter`, `menu`, `help`, `back`,
  `delete`, `find`, `view`, `read`, `whole`, `quit`); `search`, the
  search of the middle while its bar is open (`find_next`, Tab, and
  `find_previous`, shift-Tab); and `sync`, the sync of the commit
  messages with no menu over it (`keep_stack`, s, and `keep_workspace`,
  w), read there before the keys of the screens, so that a key of both
  does the sync's there: `keep_stack = "left"` and `keep_workspace =
  "right"` keep the message on the side of the arrow, and ← → then do
  nothing else in the sync. `delete` is `["delete", "backspace"]`
  unless the settings say otherwise, as a Mac's delete key is Backspace;
  `delete = "d"` makes it d alone, and Delete and Backspace then do
  nothing on the screens. The search keeps for itself Esc, Enter, ↑
  and ↓, ctrl-c, and the keys that type or edit what is typed, and
  neither of its own keys can be one of those. So `menu = "tab"` makes Tab the menu on the
  screens, and Tab still goes to the next match in the search. In a menu,
  Tab and shift-Tab go from one dropdown of an item to the next (The
  editor, above), unless a setting gives them to something on the screens.
  The comment
  editor, a line typed in a box, and the search's own screen read what is
  typed, whatever the keys.
- `[demo]`: `seen`, written by `stacked demo` the first time it is opened:
  the tour then starts on the list of its sections. `seen = false` shows
  the first tour again.

A file stacked cannot read, or with a setting it does not know, is not
used at all: the editor's bar says why, as every command does. A setting
with a wrong value keeps its default, and that is said too.

The settings were once in `~/.stacked.toml`, and StackED's state in
~/Library/Application Support/StackED/state.json. The first run of yours
that finds either, with nothing in its place in ~/.stacked, moves it there
and says so on stderr (StackED opened from the Dock says so in an alert);
a link moves as a link, to the same file. A run that finds both reads the
one in ~/.stacked, never writes over it, and says each time that the other
is no longer read. Nothing is moved when `$STACKED_HOME`, or the file's own
override, names where it is to be; nor by what stacked runs itself (an
ask's `stacked land`, the shell's completion), which reads the file where
it is until a run of yours moves it.

A checkout's own settings, `.stacked/local.toml` at its top (beside the
stacks' worktrees, out of git's sight), override `~/.stacked/config.toml` there:
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
`~/.stacked/config.toml` that the checkout's own sets too is said to be the
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
written is never changed, but the header: patches, every state of every
stack, which state each stack is on, comments and their replies, every run
(an ask, an apply, a pull) with what came of it, every line the agent
wrote while it worked, what each stack's comments pane shows, and what is
read. A record is named by its place among the records, its rev: `@12`.
The first line, the header, is `@1`: it says the file's format, and is 128
bytes long, padded with spaces.

- A patch is stored once, by the sha1 of its content. An edited patch is a
  new patch with the same change, so `versions` and `interdiff` can follow a
  patch through its versions.
- A stack record is the whole stack after an edit, with the stack it was
  edited from as its parent. A head record moves a name to another stack:
  undo, redo, a stack picked from the tree, a delete.
- A comment record is the whole comment as it is now; its earlier records
  are its history. The agent's turns are fenced ```` ```agent ````
  (```` ```claude ````, as the first files have them, reads the same), and
  a reviewer's ```` ```reviewer ````; a turn holding fences of its own (a
  code block) is fenced with one backtick more than they have, and only a
  line of as many backticks ends it. A turn of yours is as you wrote it,
  unless a line of it would read as one of those fences: then it is fenced
  too, ```` ```you ````, so that it stays yours whole. A
  turn deleted by stacked's first way of deleting one is fenced ````
  ```deleted ````, each line of it quoted with `>`: it is read only to be
  left out, and nothing writes one any more. A
  comment collapsed, or expanded, is a record naming it, apart from its
  history. A read mark holds the lines it read; taking it back is a record
  naming it.
- An ask's run, once landed, holds what it brought back for review: the
  stack its change makes (its patches stored already, by their content),
  each reply, each comment it opened. Nothing of it is applied before its
  review closes. Bringing nothing back, it holds none of them, and is held
  all the same while a comment it was asked about is still open. A review
  record holds the marks saved for a run's review,
  the replies taken already, and whether it is closed; the last one is
  what stands.
- A crash can only cut the last line short. A line that cannot be read is
  passed over with a warning (`stacked check` lists them, by line) and
  keeps its rev, so every rev after it stays the same. The next write ends
  a cut line with a newline; nothing is ever cut off the file.
- Every write holds the file's lock (flock), so two editors, the CLI and
  landing asks can share it.
- A file an older stacked wrote, its header shorter, is written again once,
  the first time something is written to it: the same lines under the new
  header, renamed over the old file, every rev the same. Not while an ask
  is going, as its process could be an older stacked's, which reads no
  file of a newer format; then the next write does it. An editor with the
  old file open reads the new one whole, and a process waiting for the
  lock opens the new file once it has the old one's lock. An older stacked
  does not read the new file.
- The header points at the last checkpoint: a line of its own, which
  takes no rev, saying where in the file the records are that the state
  stands on (each stack's head, the stacks one undo and one redo go to,
  the comments open, the asks going or waiting for your review and what
  they need, what is chosen and marked read), and how the indexes stood.
  stacked reads the header, that checkpoint, the records it says, and the
  lines after it: not the whole file. What needs the history (an older
  stack, the log and the tree, the versions of a patch, a comment's
  history, the comments closed, the runs done with and what their agent
  wrote) reads the whole file the first time it is asked.
- A checkpoint is written, then the header pointed at it, once the file
  has grown four times the last one's length past it (and at least
  256 KB), and when a stack's head moves. A crash in between leaves the
  header pointing at the one before. A header or a checkpoint that does
  not check out (half written, pointing past the end, its sum wrong) is
  passed over, and the file read whole; a header half written is written
  again.

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

What follows is built by default, with the `moz-phab` feature:
`--no-default-features --features app,demo,web` builds stacked without it
(Install). `stacked moz-phab import REVISION` makes a new stack of a
revision, `D123`, `123` or its link, as `Import from Phabricator…` does in
the editor (Making a stack in the editor): `imported 2 patches from
D11..D12 as D12`.

`stacked moz-phab pull NAME`, or the menu's `Pull reviews from
Phabricator`, reads the revision in
each patch's `Differential Revision:` trailer (the settings can name
another) over Conduit, with the token moz-phab keeps in `~/.arcrc`, or the
settings'. While it reads them, the banner says which revision it reads
(`⇣ pulling the reviews from Phabricator: D12, revision 2 of 3…`), and the
top bar `pulling…`; once it is done, the bottom line says what came of it
(`1 new comment from 3 revisions: it waits for your review`), or what went
wrong. What is new waits for your review, in the queue of work the agent's
asks land in (below): nothing is filed before you accept it. Each
new thread is a comment opened, on its line, or on the patch's title for
one on the whole revision, its replies pulled with it turns of it; a reply
to a thread there already is a reply to it, the reviewer's turn (`ann's
reply waits for your review`, `Accept ann's reply`). The banner counts the
pulls apart (`1 pull from Phabricator waits for your review`), and the
review says what was pulled. Pulling again holds only what is new: what was
pulled before, waiting, accepted or rejected, is passed over, so a comment
you rejected does not come back. A comment the reviewer edited on
Phabricator since it was filed (each edit makes a version of it, with an id
of its own) is held as an edit of their turn (`ann's edit waits for your
review`, `Accept ann's edit`): accepted, the edit takes the place of that
turn, wherever it is in the thread, or is the reviewer's last turn when
that one is gone. An edit you rejected does not come back, though a later
one does; a comment some of which still waits in a pull's review is pulled
once that review closes, and the comments of a thread you closed are passed
over. Replies and edits to a thread moved to the stack's comments go to it
there. A comment removed on Phabricator stays in its thread as it was. A
reviewer's comment can be asked about like any.

### Submitting a patch

`Submit to Phabricator as WIP`, on a patch's menu (not while reviews come
from nowhere, `reviews.source = "none"`), sends that patch alone to
Phabricator as work in progress, as `moz-phab submit --wip` sends a single
commit: in Changes Planned, a status that asks no one for a review. In the
browser (`stacked web`) and in StackED, the same. Nothing is sent before
it is read and seen. First, away from the screen, the patch's revision is
read: the one its trailer names (`Differential Revision:`), or none, and
the submit makes a new one. A revision there already must be in work,
Changes Planned or a draft, as moz-phab takes it: any other status refuses
(`D123 is Needs Review, not Changes Planned: only a revision in work is
updated, and nothing was sent.`). Its latest diff must be a version of the
patch the database holds, the one in the stack or an earlier one: of the
same effect, or leaving the same files once both are applied on the
patches below, so that a diff moz-phab sent with more context is still the
version it was made from. Else it came from elsewhere, and nothing is sent
(`D123's latest diff is not a version of patch 3 that stacked knows: it
came from elsewhere, and nothing was sent.`). And while HEAD is on the
stack, a patch whose commit in your workspace names a revision the patch
does not, none or another, as after `moz-phab submit` there, is not
submitted before anything is read: its menu grays the item and says to
sync the commit messages first, so that no second revision is made
(`Patch 3 names no revision in the stack and D34 in your workspace's
commit: sync the commit messages first, so that no second revision is
made.`); the other patches, and other differences, are not held back.

For a new revision, a dialog then says what is sent and where (`Patch 3 is
sent to Phabricator, at https://phabricator.services.mozilla.com, as a new
revision, with the status Changes Planned.`), with its title and its
parent. For a revision there already, a screen of its own shows what
replaces what, for your review: what is sent and where; then, of what
changes, the title and summary, Phabricator's then what is sent, compared
as the sync of the commit messages compares two messages (a paragraph
only wrapped otherwise once, as it is sent; a section that differs,
Phabricator's lines then those sent, what only one has in the colours for
words); the parent and the bug, before then after, and the status; and the
code, each file as the latest diff leaves it, then as the patch does, both
on the patches below it, a file that only one of them touches too. When
the code is the patch's already, the screen says so at its top. ↑↓ (pgup
pgdn home end) scroll, and 1 shows the code side by side, or unified
again, the lines of the title and summary that differ side by side too,
Phabricator's on the left, coloured the same way. Enter, or `Send it to
Phabricator` in its menu, sends it; Esc, or `Cancel: send nothing`,
sends nothing. Before anything
is sent, the revision is read again: if its latest diff, its status, its
title, summary or bug, or its parents changed meanwhile, nothing is (`D123
changed on Phabricator since you reviewed it, so nothing was sent. Submit
again to review it as it is now.`).

Only what changed is sent, as moz-phab sends it: a new diff of the patch
(differential.creatediff, its commit's author and message set with
differential.setdiffproperty), unless the latest diff is the patch as it
is; the title (`WIP: ` and the patch's first line, its reviewers and its
bug written as moz-phab writes them), the summary (the rest of the
message, without the line naming the revision) and the bug (`Bug 123` in
the title), where they differ; and Changes Planned, kept, or put back after
a new diff, which Phabricator takes as a request for review
(differential.revision.edit). When the latest diff is the patch as it is
and nothing else changes, nothing is sent, and the bottom line says so.
Then, in an edit of its own, so that a refusal undoes none of the rest:
the parent, the revision of the patch below, when it is open (none for
the bottom patch, or when the patch below has no revision, and a
revision's parents are then left as they are). A submit as work in
progress sets no reviewers, as `moz-phab submit --wip` sets none: those
the title names (`r=alice,bob!`, `r?#build`) stay in the title, written
as moz-phab writes them, and nothing of the revision's reviewers is read,
said or changed. While it is sent, the top bar says `submitting…`; then
the bottom line says what came of it, and what was done and what was not
when a call failed partway (`D123 has the new diff of patch 3 now. D123
stays Changes Planned. But the parent of D123 was not set to D122: …`). A new revision's link goes into the patch, its
trailer at the end of the message, as a new version taken at once, which
the history says (`submitted patch 3 as D124, in work`). The workspace's
commits are not touched: when HEAD is on the stack, the banner's first
line then says that the patch's message differs from its commit's, and
the sync of the commit messages (The editor, above) puts the trailer on
the commit, the message naming the revision being the one kept. The diff
is said to be on the stack's base, in the repository the checkout's
`.arcconfig` names (`repository.callsign`; a revision there already keeps
its own).

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
together, the filter of the comments pane, and what a range is on (a
branch on its upstream, else the remote's HEAD; a range typed, said as it
is typed: `import.rs`). A fix to one may belong in the other.

## Layout of the code

- `main.rs`: the command line; the library does the rest.
- `db.rs`: the file, its records and its lock.
- `check.rs`: whether patches apply, before they go in; the files as a
  patch leaves them.
- `ops.rs`: edits to a stack, `$EDITOR` (the patch as mail, its code), apply;
  the interdiff of two versions of a patch.
- `git.rs`: what stacked asks of git: mail from a range, the stack's
  worktrees and the agents', a checkout to edit a patch in.
- `ask.rs`, `claude.rs`: asking the agent about comments, from the queue to
  landing, and the folders of the agent's rebase plans; claude's command
  line, its stream and its replies.
- `held.rs`: what an ask brought back, held for your review.
- `head.rs`: where HEAD of your checkout is against a stack, HEAD moved
  along it, and the sync of the commit messages, both sides at once.
- `messages.rs`: two versions of a commit message compared word by word,
  where their lines wrap aside, for the sync to colour; and so as a diff
  of their lines, for the interdiffs and the submit's review; and what
  each section of a message says, for the reading marks to hold.
- `wrap.rs`: a commit message's long lines wrapped at a width, its
  paragraphs of prose filled again, for the menus to offer.
- `review.rs`, `phab.rs`, `submit.rs`: Phabricator, built with the
  `moz-phab` feature (on by default): the reviews pulled, the stacks
  imported, a patch submitted as work in progress.
- `import.rs`: what a stack made in the editor is imported from: a range of
  the checkout, its uncommitted changes on top if asked.
- `mbox.rs`: reading `git format-patch` mail, writing mail for `git am`.
- `rows.rs`, `view.rs`, `diff.rs`, `thread.rs`: the patch or the stack as
  rows, line keys, comment threads.
- `panes.rs`: the compact layout's two panes.
- `whole.rs`: each file of a patch whole, as `3` shows it: how its hunks go
  in it, and its files read once for each patch, in a thread of their own.
- `reading.rs`, `blocks.rs`: the reading mode's marks, by blocks of code.
- `progress.rs`: how far the review of a stack is.
- `tree.rs`: the tree of edits as a graph, and the lanes the history draws.
- `history.rs`: a stack's history, what changed it, and stack diffs: as the
  editor shows them and the agent is told them.
- `trim.rs`: a new database, the start of the history left out.
- `todo.rs`: the `git rebase -i` style list.
- `search.rs`: searching what stacked holds, in a thread of its own.
- `config.rs`, `keys.rs`: the settings (`~/.stacked/config.toml`, and a
  checkout's own over it), and the keys they map, each in its context.
- `home.rs`: where stacked keeps what is yours, ~/.stacked, as the binary
  works it out as it starts (nowhere, in the tests' own processes), and
  the files of before moved into it.
- `state.rs`: the databases opened last, by StackED and by stacked's
  editor, StackED's windows to open again, and the searches of the middle,
  kept between runs under a lock.
- `setup.rs`: `stacked setup agent`, what each kind of ask may use, one
  list with a box for each kind: where what it lists comes from (the agent,
  the checkout's claude settings, its skills and its instructions), which
  rules mean the same and are one row, what the checkout's claude settings
  of yours deny, refused to every ask, on a screen (`--tui`, and the
  editor's before the first ask) or line by line, and what saving writes.
- `editor.rs`: the inline comment editor.
- `paint.rs`: colours, and drawing text and boxes.
- `tui.rs`: the editor, and the work it does away from the screen; what a
  click is on; the views of the code, the middle split in two among them,
  each half its own (`Half`). `tui/reviewing.rs`, `tui/syncing.rs` and
  `tui/searching.rs`: its screens for reviewing what the agent brought
  back, for syncing the commit messages, and for searching;
  `tui/submitting.rs`, what a submit to Phabricator sends, said in a
  dialog, or reviewed on a screen of its own, before it is sent;
  `tui/banner.rs`, the banner of the agent's work; `tui/places.rs`, where
  each pane's cursor was, each half's of the middle split in two;
  `tui/finding.rs`, the search of the middle (`/`).
- `web.rs`, `web/client.rs`, `web/index.html`, `build.rs`: `stacked web`,
  its server, its wasm and its page; the build script compiles the wasm,
  or, where Rust lacks the wasm target, stops the build and says what to
  run.
- `app.rs`: `stacked app`, StackED: the editor on a thread of its own, and
  the window that paints its screen and sends it what is done there.
  `app/windows.rs`: its windows on macOS, one for each database opened,
  and what the File menu's items do; `app/menu.rs`, the File menu itself,
  its panels and its alerts, and the Edit menu (AppKit's).
  `icon.rs`: StackED's icon, drawn at any size. `bundle.rs`:
  StackED.app (macOS), made by `stacked app --bundle`, and opened from
  the Dock.
- `demo.rs`, `memory.rs`: `stacked demo`, the tour; and the database and
  repository it keeps in memory, with a stand-in for the agent.

## Tests

Run `cargo test` after any change. With every feature on by default,
`cargo test` runs every test: the browser's, the tour's, Phabricator's and
StackED's too (the web feature needs the wasm target, as in Install).
`cargo test --no-default-features` runs the minimal set, the tests of
stacked built with no feature, and needs no wasm target; `--features` then
adds the tests of each feature named. No test runs claude: a script stands
in for it; none reaches Phabricator: its revisions and their comments are
read by stand-ins, and what a submit sends is answered by one, which keeps
every call made to it, so that a test can see that a refusal wrote
nothing; none opens a window, nor a browser: a script stands in
for the program that opens a web link, and only writes down the links it
is given. No sleep decides what a test sees:
when a test looks at the agent's stand-in at work, the stand-in goes on
working until the test lets it finish, however busy the machine is.

Nor does any test reach what is yours to stacked: ~/.stacked,
~/.stacked.toml, StackED's state in ~/Library/Application Support. In each
test file, one function gives every run of the binary a folder of the
test's as `$STACKED_HOME`. The tests' own processes are never set up as
the binary's start sets one up: their settings' file and their state are
the null device, so that they read no settings but the defaults and keep
nothing, and the asks they start are told the same. `tests/cli.rs` checks
the first: stacked, run as the tests run it in a home holding the files of
before, reads its settings from the test's folder and leaves that home as
it was. `tests/tui.rs` and `home.rs` check the second. The tests of the
move, and of the settings in ~/.stacked, give stacked a home of the
test's.

Nor does any test write in, or remove anything from, the folder of the
agents' rebase plans in the temporary directory, which every stacked
shares. In `tests/cli.rs`, the function that gives every run of the binary
its home gives it a folder of the test's as `$STACKED_TODO` too. The tests'
own processes keep the plans beside their database, in the test's own
folder, and the asks they start are told the same. In a folder of their
own, the tests in `ask.rs` show which plan folders are removed: one whose
git directory is gone, and one that names no git directory once nothing in
it has changed for a day; not one whose git directory is there, rebase or
not, nor one that an ask still running writes its plan in. They show a
landed ask's folder removed, but not while a rebase is under way in its git
directory, nor while another ask still running writes its plan there.
`tests/cli.rs` shows the folder gone once an agent's change to the whole
stack lands.

Nor does any test leave anything else in the temporary directory, which
every process shares. In each test file, the function that gives every run
of the binary its home gives it another folder of the test's as `$TMPDIR`,
so that what stacked writes in the temporary directory (an edit it could
not take, kept for `--from`) is removed with the test's folder. The
editor's tests write their stand-ins, for claude and for the program that
opens a web link (with the links it was given), in the folder Cargo gives
the tests for their files. The folder of a test that asks the agent is
removed only once its asks are over, with no process working in the folder
any more: an ask the test left going would write in the folder as it is
removed, and leave some of it.

Every test that asks the agent has the setup of what each kind of ask may
use done, as a user's is once it is saved, so that its asks go. In
`tests/cli.rs`, the test's home holds a settings' file that says so
(`[agent] setup = 1`), and a test that names a settings' file of its own
(`$STACKED_CONFIG`) says so in it too; the editor's tests share settings
of their own that say so (`settings()` in `tests/tui.rs`). The tests of the
setup itself, and of an ask before it, start from settings that do not: a
file of their own, or, for the editor's, the stack's checkout's own
settings, `agent.setup = 0`, over the tests' own.

With `STACKED_TEST_CHECKPOINTS=1` set, every write writes a checkpoint, and
every database read from one is read whole too and the two compared, a
difference a panic: run the suite so too after a change to the database,
or to what reads it, as `STACKED_TEST_CHECKPOINTS=1 cargo test`.

`tests/cli.rs` builds a repo and drives the binary. It covers a round trip
through `git am`, a stack imported again (a patch whose lines a new
version of a patch below moved keeping its own version, and its diff as
stored; a patch whose hunk moved to another copy of its lines a new
version), `stacked interdiff` of a patch reworded from the list that only
wraps a paragraph otherwise (the paragraph as the new version wraps it,
no line taken out or put in), the tree, recovery from broken lines, comments on the
stack, a patch and a line, every way of asking about them (a script stands
in for the agent and rewrites commits with git, with a plain `git rebase
-i` that takes the plan written to the todo file the prompt names, and
none without a plan or with one that runs a command, claude told what to
refuse; runs `stacked fits`,
answers, opens comments, tests), a split following its comments, a comment
moved to the stack, a patch's comment asked of the whole stack adding a
patch on top, work that brought nothing back held for review while a
comment it was asked about is open (and not once every one is closed), the
last reply taken at once closing its work's review with its comment open
or not, a reply going with a comment the agent opened not taken at once, a
reply written again, a
reference no longer named detached by `stacked reply --edit` and the
agent's reference by `--edit --turn` (said so), and a rewind keeping one
never named, a web link attached told the agent as one it need not fetch
(the prompt asking it to point at the stack first) and listed with its
comment, the agent's own link kept in its reply (one not of the web left
out, with its number), asks waiting their turn, an agent stopped with
`stacked stop` (by the start of its run's id, the shell offering only the
runs that run) and killed as it will not exit on SIGTERM, its ask landed
at once as failed and said so on its comment and in its transcript, a
second stop refused, the effort each ask goes
at (`--effort`, else the one the editor picked last, kept in the test's
home, else `high`; the settings' own taken out; a level claude does not
take refused, nothing sent; a batch at the effort its comments were
batched at, and work sent back at the effort it was asked at unless
`--effort` says otherwise), a rebase, a patch whose lines a new version of
a patch below moved keeping its version and its diff as stored when the
agent's rebase rewords another patch (the stack diff marking only the
reworded one), the agent's change of such a patch, its commit left as it
was or made again, landing with nothing to accept, the patches below and
above the one the agent's rebase rewords keeping their versions, their
messages and authors as stored, when git gives those back otherwise
(empty lines in a row, under the title as a sync of the user's had taken
them from the checkout, white space at the end of a line or of the
title, a first paragraph of two lines, carriage returns, a prefix or
runs of spaces in the title, a newline and empty lines at the end, a
line of three dashes, a line `git am` takes for the author, a name
format-patch quotes), a message the agent changed in its white space
alone a new version, reworded, a patch with such a message that the
agent left as it was landing with nothing to accept, and one whose code
it changed keeping its message as stored, a patch with two
copies of its lines in its file a new version when its diff as stored
would go to the other copy (lines added between the two by a patch
below) and when the agent moved its hunk to the other copy, the stack not
edited by hand while the agent changes it, failures kept to look at, comments staying on their line
across versions, a patch edited by hand (as mail and as code, at a line,
refused and taken up again), `stacked head` listing the patches whose
message differs from its commit's, with moz-phab what each side names when
they differ by the Differential Revision alone, then that the editor syncs
them, the flags that synced them before refused as unknown, trimming, the settings (in ~/.stacked, or the
folder `$STACKED_HOME` names, the folder made by the first written, a file
`$STACKED_CONFIG` names over it; the files of before moved in by a run of
yours, said once, never over a file there, and not by the shell's
completion; a checkout's own over them, an ask on a stack having its
checkout's wherever stacked runs, one git tracks not read; and what each
kind of ask may use, set up with the agent, line by line, a row of the
one list at a time for both kinds, from what it has, the checkout's claude
settings and instructions, in the checkout's own or with `--global`, a
command saved the newer way, though your claude settings or the
checkout's own list write it the older, and your claude settings never
written; each kind given its own list and every ask's, and the
refusal of a tool naming its kind's list; what your claude settings deny
in the checkout refused to a change, an answer and a build and test; no
ask sent before the setup is saved, and every ask once it is), Phabricator's comments
held for review then filed (none twice, none rejected again, an edit held
as an edit of its turn), a stack imported from Phabricator's revisions (by
number or link, named or not), Phabricator's commands there only when
built with it, apply, and
what does not apply being refused, edited again, or read-only. `tests/tui.rs` draws the editor into a buffer and presses keys:
that no letter but 1, 2, 3 and / does anything, keys mapped by the settings,
Delete and Backspace as the menus' items that close, drop, stop or reject
(a comment closed, with those its answer opened, its agent stopped and
its ask landed at once as failed, taken
back from the queue or out of the batch; a patch dropped and the stack
deleted; a file's changes dropped from its patch for review, refused when
a later patch needs them, a rename, an added file and a deleted one
dropped whole, and the patch's drop offered for its only file; in a
review, a part rejected, a comment closed with it, what you wrote taken
back; landed work rejected and a comment closed from the banner; an ask
stopped from the runs, landed at once too; each asking first where its item does, Esc then
doing nothing; on a comment closed, the status line saying so), their key
a setting, and nothing done in a menu, History…, the tree, the versions or
the search, nor in the comment editor and `/`'s bar, where they edit what is
typed, the search of the middle (as it is typed, from any pane; Tab and
shift-Tab from match to match and round past either end; Esc back where the
cursor was, the middle scrolled as it was, a click out of the bar too; Enter on the match,
or back with nothing matched, said; the next `/` with the last search,
selected, what is typed taking its place, ↑↓ through those before; case
ignored, a line of code without its sign; a comment, the message and a
file's name; side by side and compact; split in two, the cursor's half
alone, the other's lines not lit, Esc putting that half back; reading; a
line folded matched as
one, the cursor on the piece of its match; the matches lit; its key the
settings', and those from match to match, Tab a key of the screens' too,
its others as they are; with each file whole, a line in no hunk found and
gone to, 3 typed while the bar is open, 3 after Enter, and the files read
while it is open searched again, Esc going back to the row found again),
the
menus by place, what `?` says where, writing in every pane, the regions it
says it drew (its panes, then the boxes over them, inside their borders:
what StackED's selections keep to), the first ask opening the setup of
what each kind of ask may use (the agent asked what it has away from the
screen, Esc sending nothing, a box ticked with a click or by its column
with the arrows and Space, saving writing the checkout's own settings
and sending, the next ask going at once), a comment asked
about and its stream and answer shown in place, its agent stopped from its
menu (the ask landed at once as failed, said so, its stream folded, the
comment's menu whole again, and the agent's wrapper, a child of the
tests' process, waited for), an ask done leaving no zombie once the
editor looks again, an ask whose agent is gone without landing it landed
at the next look (its process gone, or a zombie the editor did not start,
left for whoever started it to wait for), a build and test stopped as a
review closes landed at once though it had no process yet, the review's
word kept, the effort an ask goes at
(`Ask agent` saving what changed first, and asking about what did not as
it is; the effort at the end of its line, picked with Tab, ← → and a
click, as `?` and the bar say, the other dropdown picked each way too; the
stand-in started at the effort picked, `high` until one is, the settings'
own taken out; the effort picked where the next menus start, kept nowhere
in a test's own process; a batched comment, sent now or with the batch,
at the effort it was batched at, one waiting its turn at its own, one
queued by an older stacked at the agent's default; work sent back at the
effort it was asked at, unless another is picked), the screen never waiting on
the agent, redrawing not slowing as the agent writes (a frame timed as the
time the editor ran to read and draw it, not by the clock, which counts
too the time a busy machine gives to other work), the filter kept in
the file, the views of the code (the menu of 1, the view drawn marked,
Enter or a click picking one and Esc none, its key a setting, each view
drawn; a view picked on a middle too narrow making its room, the comments
compact, else hidden, else the patches hidden too, as little as it can,
said and left so once the view is; on a screen too narrow even then, the
item saying so and the view not taken; split in two: both halves drawn,
each a region of its own, each its own cursor and scroll, search, reading
and files whole, and its place on each patch, the other half's cursor as a
pick, ← → from half to half and pane to pane, a click and the mouse taking
the cursor into a half, a patch clicked taking it into the left one, a
comment and a read mark made from the cursor's half and shown in both,
another view keeping the cursor's half, a middle too narrow drawn as one
and said, and the layout a setting), what changed in a line almost the same as the
one removed painted as words (and lines quite other, not), in the code, in
a version's interdiff and in the history's stack diff, none of the room after
a word last in its line, and the words keeping their own colours under the
cursor, in the code and in the history's stack diff, the rest of their line
in the cursor's (`paint.rs` tests the colours for words so: whatever they
become, they must look otherwise than their line's, and than the cursor's),
the message in an interdiff compared as the sync of the commit messages
compares two (the words put in its title as words, the words it keeps as
any text, its sign as its line's; in the review of the agent's work, a
paragraph the work only wraps otherwise no change, the cursor on the
first line of code that changed, and a comment written from a line of it
there on that line of the new version; accepted, its lines read as they
were, Delete there taking back the mark that read them and Space reading
them anew, and the history changing no line of it; a word changed in a
paragraph wrapped otherwise the one word coloured in each; a command
wrapped otherwise a change, its break marked; the diff after the message
painted as words as ever), the submit's review comparing the title and
summary so, side by side too,
the cursor looking the same on every line, as `cursor` paints it, whatever
the line's own colours (a line added, a line removed, a hunk's header, a
file's name, each line of a comment, a line read, the patch picked in its
pane), and the patch picked as `picked` paints it once the cursor is in the
middle (`paint.rs` tests the cursor so, over each colour, dim or reversed), reading by hunks and by a Rust file's blocks
(the cursor going down to the middle line, then the code coming up under
it, in each view; Enter commenting there, Delete taking a mark back, and
a comment's rows keeping the keys of commenting, Space its menu, marking
nothing read; with moz-phab, the trailer moz-phab writes at the end of a
message passed over by Space, the margin asking no mark on it, the patch
ticked and nothing left to review once all but the trailer is read, and
the trailer counted when the settings say white space needs review), each
file whole (its files read away from the screen,
the status saying so; no hunk's header, each line removed in its place,
the lines between the hunks in none, keyless and dim, the cursor looking
the same on them as on any line; a file added, deleted, renamed, not text;
a file as its patch leaves it, the patches under it applied, and nothing
written in the repository; the rows of the hunks' lines, keys and all, as
in the hunks; a comment on such a line shown both ways, written either
way on the same key, and none on a line in no hunk, the menu and the status
line saying why; a line in no hunk folded, two sides keeping its piece, and
taking no read mark; the files listed in the patches pane gone to; the key
a setting; on the stack's own view, said, and on another screen, no key;
with no base to read the files on, the hunks under a line saying why),
the cursor through 1, 2 and 3 (on its line and its line of the screen,
through each view, split in two too; back
to the hunks from a line in no hunk, on the closest line in a hunk, ties
above; a hunk's header, whole, on its first line), and the twelve ways the
three keys combine (unified, side by side or compact, commenting or
reading, the hunks or each file whole: the view drawn, the old side beside
the new, a comment shown, a
read mark made on the hunks' keys and taken back, the cursor kept),
editing a patch by hand at the cursor (a line added after git's
`\ No newline at end of file` opened at its own number, and a reference
to that number going to that line), and from the review of landed work
where its cursor is (on the list, a line of the stack diff, a row of the
patch looked at closer: a line removed opened at the line after it, a
comment or the message with no file to edit, an edit not taken opened
again at the same line, and the menu writing a comment where Enter writes
one), the history screens, the branches
History… draws and the stack diff it shows beside each change, the agent
asked about a record of the history, a new database started from it, a
moved comment and an answer's comments, a patch's comment asked of the
whole stack, every comment of the stack closed at once but those that
cannot be, a comment closed taking the cursor on it to the line it was on,
on its line of the screen (from its menu on its first row and on a
reference, by Delete on a turn; in each view of the code, split in two each
half on its own, with each file whole, on a line folded, reading too; on
the stack, the row over it; hidden by `Show…`, from the agent's stream too),
and leaving the cursor elsewhere where it is (closed from the comments
pane, which picks the next comment or the one before at the end, from the
banner, by another stacked; hidden by `Show…` from either header, moved to
the stack, reopened), a turn written again in its place taking the cursor
back to it, what you said last written again (off the queue meanwhile;
left as it was, asked as it is or left so), a reply written in $EDITOR
under its conversation, its references under it, and a turn written again
there in its place, the editor in the middle between the last reply and the
references, the agent's stream between them too, under the editor when one
is open on the comment (Enter on the stream still opening it, Enter on a
reference still previewing it, and the cursor staying on its line as the
stream grows and goes), the references a rewind or an edit no longer names detached
(the status line saying so; those never named, those the agent attached
without naming them, and those the reply waiting for review names, kept;
the next one attached taking a number never given), the agent's reply
written again as its text alone,
keeping its references but the one it no longer names, a turn deleted the
first way counting nowhere, a web link attached from the menu (what is not
one refused, the status line saying why) and named, its line under the
comment cut to the pane, shown whole in its box (folded there when longer)
and opened by the stand-in, never a browser, the status line saying so,
the agent's own link given through the stand-in for claude, both listed
in $EDITOR, and the link detached by a rewind of the only turn that names
it, a stack made with none there (imported from the workspace, a
range typed and refused until it holds a commit, the changes staged on top;
from Phabricator, its revisions read by a stand-in; or by the agent), a
comment moved to the stack
taking the cursor with it, the mouse (a side pane's line marked under it and picked by a click,
elsewhere the cursor following it, a click as
Enter, a right click's menu, a menu's item; a menu opened with a click
next to the click, to its left or above it by the screen's edges, whole on
a screen made smaller, and where it was once a box in it is checked, a menu
opened with a key in the middle; writing a comment in the middle
in each view of the code, in the patches pane, in the comments pane, or
between a reply and its references, a click outside the box as Esc,
picking nothing, the menu that click opens next to it, over the comment
too, the one Esc opens beside the comment, a click outside either going
back to the
comment, and a click in the box putting the cursor there), the comments pane's previews,
moving a patch, the long lines of messages wrapped (a patch's menu
offering it only when its message has some at the settings' width, the
note saying how many lines of which patches; the edit landing for review,
the stack as it was until it is accepted, then the new messages, as the
history says; the stack's header wrapping every message that has some, as
one edit; offered but not to be chosen, the menu saying why, on a
read-only stack and while an edit by hand waits for review), searching (what is in sight first, the patches then the
files they touch, the history), the agents' worktrees made ahead as the
stack's checkout says, the banner of the agent's work (Esc to it and
back, open a line for each, Enter on each; the reviews pulled waiting in
it as the agent's work does; replies taken with Enter, the last one
closing their work's review, its comments left open; Enter opening the
review of work that opened comments too, and saying to review first the
edit that work goes on from; and work that brought nothing back reviewed
for its comment alone), the sync of the commit messages (its line first in
the banner, in a section of its own, and first in the banner's line and
menu, gone once they are synced; no landed work opened, rejected or taken
meanwhile, from the banner, its menu, a comment's or an edit by hand
landing, and opened after; a review open before closing all the same, HEAD
staying; nothing of it in the patches pane nor in a patch's menu or the
stack's; its patches listed with what each keeps, picked by key, from its
menu and by a click, and its keys the settings', read there before the
screens'; what is left to pick said; closing asked first, saying what
goes where, then both ways at once: new versions in the stack, the
commits made again on the same files, HEAD in sync after; a change waiting
for review going on from the new versions, its own versions of a patch
taking the message too, and accepted after; Esc keeping the picks; nothing
changed when HEAD changed meanwhile, the pick of a message no longer there
dropped, nor with a change not committed, nor a patch changed while the
agent changes the stack, a commit still taking the stack's message then;
the messages side by side, then one over the other on a narrower pane,
what one has and the other has not coloured, a paragraph wrapped otherwise
as any text and a break one alone has marked; with moz-phab, nothing to
pick of a revision one side alone names, the other taking it, a revision
carried over to the message picked that names none, and of two, the pick
keeping its own and the sync saying which it drops; without moz-phab, the
trailer a line to pick as any other), the comment editor hiding
nothing in each view of the code, each pane keeping where its cursor
was, the patch's title kept at the top of the middle, the patches pane's
states (expanded, files, hidden) and its files (listed under the patch
picked, gone through with the arrows past their ends, gone into with
Enter and →, clicked, and picked from the middle, where ← comes back to
the file; reading, each saying the lines left to mark read as marks are
made and taken back, those of white space alone counted only as the
settings say, a file with no newline at its end ticked once read whole,
its patch too, the pane keeping its width from patch to patch), and the
comments pane's states (expanded, compact, hidden) and the compact view
of the code as they were. With the `web` feature, `tests/web.rs`
serves the editor and asks it over HTTP as a browser would: the token
needed, the page, the wasm, the rows as they change, a key and a click,
and the editor left, the worktree made ahead removed (Quit in the page;
`stacked web` itself sent SIGINT, SIGTERM or SIGHUP, the first while
`$EDITOR` runs left to `$EDITOR`, the second waiting for it), and two
`stacked web` at once, each putting its database, named from where it
runs, on the list StackED's Open Recent shows, the effort picked in a
menu kept in the state file of its home, where the next `stacked web`
starts, and a message's long lines wrapped from its patch's menu, the
edit landing for review; and
`web.rs` tests the rows and the lines both ways, with the client's own tests
(`web/client.rs`, compiled to the machine as well as to wasm), which also
test the mouse followed only while the page has the focus: the cell where
the mouse was last seen while the page did not have the focus said at once
when the page has it again (unless the mouse left the page), so that the
click that gives the page the focus back acts on that cell, whether the
browser sends the click before the focus or after. With
the `app` feature, `tests/app.rs` runs StackED's editor as its window runs
it, with no window: nothing drawn before the window's size, a key, a click,
the mouse over a patch then gone, a window too small, the wait while
`$EDITOR` runs (an editor that edits nothing, set in the test's settings),
quit, the command there, and a database that cannot be read said before
any window; two editors in one process, as two windows, each in its own
folder (HEAD said in the checkout's, none elsewhere); a message's long
lines wrapped from its patch's menu, the edit landing for review; and the File menu,
its panels answered by the test: Open… opening each database picked in a
window of its own (one picked twice, one that cannot be read said, the
panel cancelled), New… making an empty database that stacked reads and
never replacing a file there (its bytes the same), and Open Recent taking
a database deleted off the menu and off the state kept, then Clear Menu.
`app.rs` tests what the window paints of a screen (the colours, reversed
and dim, the faces, wide characters, the lines of boxes and blocks), what
it makes of keys, clicks, the wheel and pastes, and the window itself, run
by egui with no window: the grid's size sent, the screen painted, what is
done sent, the wait for `$EDITOR`, and closing, and text dragged over lit
and copied, kept to its pane (a block with alt, a drag past the grid up to
its edge, a key letting it go), and the mouse followed only while the
window is the active one. `app/windows.rs` tests StackED's windows,
run by egui with no window as eframe runs them: the root's window hidden,
a window for each database opened (one twice), each telling its own editor
its size and following the mouse only while it is the active one, a
window closed telling its editor to leave, the last closed
leaving StackED running, and Quit keeping the windows open for StackED.app
and waiting for each editor to leave. `state.rs` tests what is kept
between runs: the recent databases, the latest first, once each and eight
at most, cleared; the searches of the middle, the latest first, once each
and twenty at most, kept beside them; the effort last picked, kept beside
them too, one this stacked does not know read as none, the rest as it is;
those deleted taken off both lists; a file that cannot be
read; writers at once, each keeping what it wrote (with no lock, some
would not); a file of another kind (the null device) keeping nothing; and
the editor's database kept nowhere in a test's own process. On macOS,
`app.rs` tests that the fonts hold every character
stacked draws, `app/menu.rs` that the File menu's target is a class the
Objective-C runtime takes, with a method for each item's action, and that
the Edit menu's items have the titles and shortcuts they have in any Mac
app, with actions that AppKit's text fields and windows answer, and
`tests/app.rs` makes StackED.app as macOS reads an app
(its Info.plist, its signature, its icon at every size, the stacked in it,
which runs), makes it again, and leaves alone something else of its name;
`bundle.rs` tests when stacked is opened as the app, the login shell's
environment read (a shell that never ends given up), an edit in a window
of its own waited for (in its folder, it failing, its window closed before
it exits), and the script Terminal is given to run it (for its owner
alone, removing itself as it runs the edit, removed if Terminal cannot be
opened); `icon.rs`, what the icon shows where, at its largest (the sheets' edges)
and at its smallest (a plain white S), and that it is grey all over. With
the `demo` feature, `tests/demo.rs` takes the tour, each step done as Tab
does it: the
stand-in's fix, the board at its end, nothing made on disk; the answer
to the tour's question taken at once with Enter, the tour going on then;
and the views taught with 3, picked in the menu of 1 and making their
room on a middle too narrow, the step waiting for the hunks again;
`memory.rs` tests its diffs applied as git applies them. The
stream's thinking is tested in `ask.rs`, with the prompt asking the agent
to point at the stack before the web and its answer's schema taking a web
link, the ask's effort given to claude after the settings' arguments, in
place of theirs, a child process that has exited not taken for alive
though `kill` still finds it, its exit left for whoever started it, the
wrapper of an ask the process started waited for once it has exited, and
an ask whose agent you stopped landing as stopped, unless the agent was
done as you stopped it; what is said of landed work left to review is tested
in `held.rs`. The
tests in `check.rs` show
the check's index holds only what the patches touch, that it writes nothing
to the repository, how a patch's files are read before and after it, and
where a diff of the same effect as its draft does otherwise (git applying
it at another copy of its lines, it not applying), the drafts not
applying an error;
those in `ops.rs`, that two code edits saved at once, on two threads, each
make their diff in a folder of their own and leave nothing in the
temporary directory, which diffs have the same effect (a hunk moved down
its file, under another heading or none, other blobs, a rename's
similarity, a header's line ending in a tab or not, an empty line in a
hunk for an empty context line) and which do not (another line added,
removed or kept, a file more or fewer, the files in another order, a mode
changed, a hunk split in two), and that a draft with its patch's message,
author and effect is that patch, its diff as stored, whatever its date,
while one with another message, author or effect is a new version, as is
one that nothing can be checked on, and that a draft with the message and
the author git gave back of its patch's has them as stored, from its
commit as it was or made again, its code changed (the title git gave
back finding its patch) or nothing to check on, while one whose message
changed otherwise, if only in its white space, or with another author,
is a new version with the draft's message or author; and they show the
interdiff of two versions: a paragraph only wrapped otherwise no line
taken out or put in, there once as the new version wraps it, each line
where it is in the new mail, a word changed in it the one word marked,
each version's lines taken out and put in, a command wrapped otherwise a
change, its break marked, the diff after the `---` the same lines as the
mails diffed line by line, its hunks of its own when far from the
message, and with the title and the body the same, the mails diffed line
by line as they were; those
in `history.rs`, what the history leaves out (comments, runs, a move to
where the stack is, a branch put straight back), how a stack diff follows
each patch by its change, a move told from a shift, and a new version
reworded when its message alone changed, its hunk moved under another
heading, one that only wraps its message otherwise changing no line of
it and one changing a word marking that word alone. Those in
`setup.rs` show the setup screen's one list, a read-only box and a change
box a row, what stacked always gives marked in its columns, Space ticking
the box of the cursor's column, ←→ and Tab moving between them, a click
ticking the box under it, and the columns' heading kept on the first line
once the list scrolls; a server's tools let all at once or one by one, a
tool of every ask's taken out of one kind; what each list starts ticked
as, the settings winning; which rules mean the same (a command's older
prefix form and the newer, a rule from several places), each one row
saying every place, a command shown and saved the newer way, agent.tools
rewritten so too, and which do not (a narrower rule and a wider, the command alone, a
`*` read as written); the rules a command added is checked against, and
its row, neither box ticked; what the checkout's claude settings allow and
deny, what its skills let run (their headers' every form, a rule several
allow, what is no rule left out), and what its instructions name (a
program on PATH, one not, a builtin and a tool stacked gives left out),
nothing read out of the checkout; the row of claude's tool for skills,
with what it lets an answer do; and the setup asked line by line, a
question a row for both kinds; those in `search.rs`, which lines of a file a hunk shows
already; those in `rows.rs`, a web link's line under its comment cut to
the pane, its start kept, each file whole keeping the rows of its hunks'
lines, a file too long and one past what a patch shows whole shown as
their hunks, a note saying so, and a row found again in the rows built
otherwise (whole or in hunks, folded to another width), a line in no hunk
as the closest line in one, ties above, and as comments go (a closed
comment's row on its line; one on a line the patch no longer has, or the
row saying which line that was, on the closest row over them; the rows
under them, there still); those in `whole.rs`, how a file's
hunks go in it whole (a hunk with no line on one side, git's `\ No newline
at end of file`), a file added, deleted, renamed or not text, and one that
does not fit its hunks, not shown whole; those in `progress.rs`, that a patch whose file has no newline
at its end leaves nothing to review once its lines removed and added and
its message are read (git's `\ No newline at end of file` is not a line
to read), and, with moz-phab, that a patch whose message ends with the
trailer moz-phab writes leaves nothing to review once all of it but the
trailer is read, the trailer left to review as the empty lines are when
the settings say white space needs review, which lines are that trailer
(its link to any host) and which are not (a line that says more, a link
that ends otherwise, no link, the line in the title's paragraph or in the
middle of the message), that a line of the diff saying the same is a line
to review, that without moz-phab the trailer is a line to review, and that
a paragraph a new version only wraps otherwise is read as it was, once a
mark holds its section, its lines to read again once a word of it
changes, and a mark that holds no section's key holding its lines alone;
those in `reading.rs`, Space and Delete on that trailer marking and
unmarking the message as on an empty line, the next line to read found
past the trailer, and the trailer drawn as a line that needs nothing, or
without moz-phab a line to read; those in `config.rs`, a file of settings over another, a
checkout's own read and written unless git tracks it, the patches
pane's `compact`, as versions before the files wrote it, read as them,
the width messages are wrapped at, one too narrow refused, and the keys
listed by the context they are read in; those in `wrap.rs`, a message's
long lines wrapped: a paragraph of prose filled to the width (lines broken
by hand, gaps of white space), one within it left as it is, its short
lines unjoined, a list's items each a paragraph under its marker (three
spaces at most), a word longer than the width alone on its line, code (a
fenced block with an empty line, an indented command, a `$ ` line), the
title, the trailers and a quoted line left as they are, a word that would
start a line as something else kept on the line before and such a word
alone on its line kept so, a wide character two columns, what the menus
say, and messages made up of lines of every kind wrapped to the same words
as `messages.rs` compares them, with nothing more to wrap after; those in `keys.rs`,
the keys checked context by context (the defaults with no clash, one key
in two contexts no clash, two things of one context on one key said),
what the search of the middle keeps for itself refused to its own, and
the sync's own keys, s and w, read in the sync alone, given ← and → of the
screens with no clash, and given one key both, said; those in
`messages.rs`, two versions of a commit message compared where their lines
wrap aside: a paragraph wrapped otherwise the same, a word changed marked
in each, two words in a row marked as one stretch, a paragraph split or
joined marking the break alone, white space that ends a line not read in
prose nor in code, the trailer naming the revision a line as any other
(another revision, its link alone; one alone naming it, its line and the
break before it), a command wrapped otherwise not the same while the
prose over it is, a fenced block one section of code, its empty lines and
all, a line of code (indented, after a tab, a prompt) making its whole
paragraph code, the white space within a line of code counting, its
indent too, and a section of code in one version and prose in the other
not the same; and the two as a diff of their lines: a paragraph wrapped
otherwise a run the same in both, each with its own lines; one with a
word changed a run that differs, the word alone marked; a paragraph split,
one put in or taken out, a command wrapped otherwise, runs that differ;
empty lines before every section or after the same as the other's there;
and what a section says the same however it wraps, code line by line;
those in `head.rs`, which messages differ by the revision
alone and which have nothing to pick, where a trailer is added, the
message kept as picked (the other's revision taken when it names none, of
two the other's dropped, of one naming none the one that names it,
whatever is picked), what a sync's plan changes on each side and how many
commits it makes again, and, without moz-phab, the trailer compared and
kept as any line;
those in `refs.rs`, which references a rewind or an edit detaches, which
stay (named by another turn, the agent's or by `@r*`, by the reply waiting
for review, or never named), the agent's turn written again keeping its
own, a turn deleted the first way counting nowhere, and no number given
twice, a reference detached by hand or gone with its turn; and a web link
read from its `refs` line and never as a path, kept in the database while
the comments saved before read as they did, given by the agent as
`{"url"}` only when it is of the web, named and detached as any
reference, and opened by the program that opens links, or said why not.
The tests in
`diff.rs` show each line's numbers on each side, git's
`\ No newline at end of file` counting on neither (under the old side's
last line, under the new side's, under both, or under a line both sides
have), and show that the keys of those lines, which comments and read marks
are on, are as they were when git's line counted on both sides. Those in
`blocks.rs` show that a line added after git's `\ No newline at end of file`
is in the block its own number is in, and those in `phab.rs` show that a
reviewer's comment on such a line is put on that line, found by its number
as by its text. Those in
`home.rs` show where stacked keeps what is yours (the home's folder, the
one `$STACKED_HOME` names, a file named over it, nowhere with neither);
the files of before moved in once by a run of yours and said, never over a
file there, a link moved as a link, and nothing moved that an override
names the place of; what stacked runs told the same places; and nowhere in
a test's own process.

## Not done yet

- Turns deleted by stacked's first way of deleting one (```` ```deleted ````)
  are still read, only to be left out. Once no database holds them, that
  code goes, and with it the split of a thread's turns between those that
  stand and all of them.
- How apply picks a branch, which belongs to publishing: the editor applies
  on no branch, `stacked apply --branch` on one.
- A stack-wide review runs one agent; one per patch is not done.
- An agent other than claude must speak claude's command line and stream,
  and take `--json-schema`, unless `agent.schema` is off.
- Only git mail is read: no `hg export`.
- A patch is submitted to Phabricator one at a time, as work in progress
  alone (Changes Planned), and so with no reviewers: a stack at once, a
  revision sent for review with the reviewers its title names, a file
  that is not text, and each file whole as the diff's context (so that
  reviewers can see more of it) are still moz-phab's, after `stacked
  apply`.
- StackED's windows are on macOS only: elsewhere it has one, on one
  database, and no File or Edit menu. A database is not opened by dropping
  it on StackED's icon or by double-clicking it in the Finder, and there is
  no Window menu. Its text is not yet a setting: it is Menlo (Hack
  elsewhere), at 14 points before ⌘+ and ⌘−, which change it in every
  window at once. The editor the settings name for `$EDITOR` is read where
  StackED runs (where `stacked app` was started, the home folder from the
  Dock), not in each window's checkout.

## License

MIT: see [LICENSE](LICENSE).
