# stacked

A stack editor, in the terminal. Patches, the stacks they make and every
edit made to them, comments on them, and what the agent (claude) and
Phabricator said: all in one file that is only ever appended to (its
header aside), so that any edit can be undone and the history read back. A
comment can be asked of the agent, which answers it or changes the
patches; what it brings back waits for your review. Your checkout is not
touched unless you ask: every ask of the agent runs in a worktree of its
own, never in your checkout.

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

The five features below are on by default, so that a build has them all.
`--no-default-features` builds stacked with none of them, and `--features`
then names those to keep: `cargo install --path . --no-default-features
--features demo,moz-phab`, for instance, builds stacked with the tour and
Phabricator, but without the editor in a browser, StackED or the try push.

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

The `try-push` feature adds the try push of Firefox's CI (Try push,
below): a comment asked as a try push has the agent select the CI tasks it
asks for, each with why, and those tasks wait for your review, to accept,
reject or answer; those you accept are pushed to try with `./mach try
fuzzy`, its link a reply on the comment, and the push is followed on Lando
and Treeherder until its tasks are done, its summary a reply for your
review. Built without it, stacked has none
of it, but reads a database and a settings' file that a build with it
wrote: an ask of a try push is read as one to answer only, the try push's
list of tools is passed over, and so is what a push's run keeps (it reads
as a run of its own, `try-push`).

The `app` feature adds StackED, the editor in a window of its own
(`stacked app`, below). Its window is drawn by eframe (egui), on OpenGL,
its dialogs are rfd's, and its fonts are the system's, as fontdb finds
them: the build fetches them all, and on macOS and on Linux it needs
nothing more. `stacked app --bundle` then makes StackED an app of the
system's: StackED.app on macOS, for the Dock; elsewhere, a launcher among
the desktop's applications.

On Linux, StackED loads what it needs of the system only when it starts,
and a Linux desktop has all of it: X11's libraries or Wayland's,
xkbcommon's and OpenGL's (EGL or GLX), for its windows (eframe); D-Bus's
library and the desktop portal, for the Open and Save dialogs (rfd), else
zenity, which also shows the alerts; and DejaVu Sans Mono with DejaVu
Sans, for the text, else egui's own Hack. On a Debian or Ubuntu system
that lacks them, this installs them all:

```sh
sudo apt install libx11-xcb1 libxcursor1 libxi6 libxrandr2 libxkbcommon-x11-0 \
  libwayland-client0 libwayland-egl1 libegl1 libgl1 \
  libdbus-1-3 xdg-desktop-portal xdg-desktop-portal-gtk zenity fonts-dejavu-core
```

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
stacked app --bundle               # StackED as an app: StackED.app for the Dock on macOS, a launcher elsewhere
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
stacked send bug123 4f1c2a --effort max   # at claude's --effort max, rather than high
stacked stop 1761650000000-4242    # the agent of that run (stacked runs lists them) stopped: its ask lands as failed at once
stacked reply bug123 4f1c2a and keep the title
stacked moz-phab pull bug123       # reviewers' comments from Phabricator, held for your review (moz-phab)
stacked moz-phab import D123       # a new stack of D123, those it stacks on and those on it (moz-phab)
stacked export bug123 | git am     # or put it anywhere by hand
stacked trim new.jsonl --from @40  # a new database, the history before @40 left out
stacked config > ~/.stacked/config.toml   # every setting, to start a settings file from (once ~/.stacked is made)
stacked config editor hx           # one written in the settings file; KEY alone reads it
stacked config --unset editor      # ... taken out of it
stacked config --local editor vim  # ... in this checkout's own settings, over the file's here
stacked setup agent bug123         # what each kind of ask may use, one list of tools, checked line by line, for its checkout
stacked setup agent bug123 --tui   # ... on a screen, a box for each kind of ask on each tool, and the rules reviews allowed, to keep or take out
stacked setup lint eslint --fix    # the linter of this checkout, which the stack's menu runs: Lint the stack (eslint --fix)
stacked setup lint                 # ... the one in use, and where it comes from (./mach lint --fix where ./mach is)
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
trim) or reach Phabricator says that it is not in the demo.

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
are listed there; and `stacked web` ends once each page is told (a few
seconds at most, for a page that reads nothing). A ctrl-c while `$EDITOR`
runs there goes to `$EDITOR` alone.

The server runs the editor as the terminal does, drawn into a buffer of the
page's size, and sends the page the rows that changed (server-sent events);
the page sends its keys and clicks back. Keys that come together are drawn
once, after the last, and each is done on the screen as the keys before it
left it, as in a terminal, which draws the screen after each key: Home,
then Space at once, opens the stack's menu with the stack's comments to
ask about, as a right click on the stack's header does. What the page does
is wasm
(`web/client.rs`: the rows made HTML, the keys and clicks made lines for
the server), carried by fifty lines of JS (`web/index.html`).

## In a window of its own: StackED

`stacked app` (built by default, with the `app` feature:
`--no-default-features --features demo,moz-phab,web` builds stacked without
it) opens StackED, the editor in a window of its own, with no terminal and
no browser: the same editor, the same keys, drawn as in a terminal. The
window paints the editor's screen as a terminal would, a grid of cells in
their colours, bold and italic, underlined or struck through, and draws
the lines of boxes itself, so that they meet from row to row. Its text is
in the system's monospace font, as fontdb finds it where the system keeps
its fonts: on macOS Menlo, with Apple Symbols for the few characters Menlo
lacks; on Linux DejaVu Sans Mono, which Menlo was made from, with DejaVu
Sans for those (else Noto Sans Symbols2 or Noto Sans Symbols); each with
its own bold and italics. Where neither is found, the text is in Hack,
egui's own font, its bold drawn twice a pixel apart and its italics
slanted. ⌘+ and ⌘− (ctrl elsewhere) make the text larger and smaller, ⌘0
brings it back, and the window holds as many cells as fit in it, under its
menu bar.

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
and does nothing elsewhere. On macOS ⌘ is never the editor's: ⌘Q quits
StackED and ⌘H hides it, as the application menu has them, which is the
system's (below); ⌘C and ⌘V copy and paste; and the other ⌘ keys do
nothing. Option types the characters it makes (é, ∂), as Terminal does.
StackED's menus take no key, on any system: every other key goes to the
editor, as in a terminal.

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
the editor in the terminal lists them.

StackED has many windows, on every system, and each has a menu bar at its
top, egui's own, the same in every window and on every system, with one
menu, File: New…, Open…, Open Recent, Close Window and Quit StackED. It is
used with the mouse: none of its items has a shortcut, on any system, and
the bar takes no key, so that every key goes to the editor. Open… shows the
system's Open dialog, where any number of databases (.jsonl files) can be
picked: each opens in a new window, even one open in another window
already. New… shows the system's Save dialog, then makes an empty database
where it says and opens it. New… never replaces a file: the dialog offers
to, but a file there already is said in an alert and nothing is made
(delete it first, outside StackED). The dialogs are rfd's: on macOS
AppKit's Open and Save panels, and the system's alerts; on Linux the
desktop portal's file chooser, asked over D-Bus, else zenity's, and
zenity's alerts, which are also written on stderr. While a dialog is open,
the windows go on, and the items that open another are greyed out. Open
Recent lists the eight databases opened last, the latest first, whether
StackED opened them or stacked's editor did, in a terminal or in a browser
(`stacked web`); one no longer there is taken off the list, and Clear Menu
empties it. Each window has an editor of its own, which runs as if started
in its database's folder: HEAD is the HEAD of the checkout there, and
`$EDITOR` runs there; only the first window of `stacked app` runs where it
was started. Close Window, like the window's close button, closes the
window and quits its editor, as ctrl-c does. Closing the last window quits
StackED, as Quit StackED does: every window's editor leaves, and is waited
for. The databases opened last are kept, with the windows StackED has open
when opened as an app (below), in ~/.stacked/state.json, never in the
settings' file (`$STACKED_HOME` names another folder than ~/.stacked, and
`$STACKED_APP_STATE` another file). StackED and a terminal's stacked may
change that file at once: each change is made under a lock, on the file
state.json.lock beside it, so that neither loses what the other wrote.
StackED kept the file in ~/Library/Application Support/StackED/state.json
before; your first run of this stacked moves it (Settings, below).

There is no Edit menu: to copy, select the text with the mouse, then press
the system's copy key (⌘C on macOS, ctrl-c elsewhere); to paste, press its
paste key (⌘V, ctrl-v elsewhere) where you write. On macOS, the menu bar at
the top of the screen is the system's, not StackED's: the application menu,
as winit gives it (About, Hide with ⌘H, Quit with ⌘Q); and, only while an
Open or Save panel is open, an Edit menu, so that ⌘Z, ⇧⌘Z, ⌘X, ⌘C, ⌘V and
⌘A do in the panel's text fields what they do in any Mac app, such as the
name in the Save panel and the folder typed after ⇧⌘G (Go to Folder):
AppKit gives those keys to a text field only through the menu bar's items,
and egui's copy and paste do not reach the panel.

`stacked app --bundle` makes StackED an app of the system's. On macOS, it
makes StackED.app in ~/Applications (`--bundle DIR` makes it in DIR), for
the Dock and the Finder: drag it to the Dock to keep it there. It holds a
copy of this stacked, so make it again once stacked is updated; made again,
it replaces the StackED.app there, but nothing else of that name.
Elsewhere, on Linux and the other desktops that follow freedesktop.org, it
makes StackED's launcher: a file
`io.github.carverdamien.stacked.desktop` in ~/.local/share/applications
(in `$XDG_DATA_HOME/applications` if `$XDG_DATA_HOME` is set, and in
DIR/applications for `--bundle DIR`), and StackED's icon, drawn at each
size of the icon theme's, in ~/.local/share/icons/hicolor/NxN/apps beside
it. StackED is then among the desktop's applications. The launcher runs
this stacked where it is, as `stacked app --launched`, not in a terminal:
make it again if you move stacked. Made again, it replaces StackED's
launcher there, but nothing else of that name. Either way, stacked prints
where it made it, and says what to do next. The launcher holds:

```
[Desktop Entry]
Type=Application
Version=1.5
Name=StackED
GenericName=Stack editor
Comment=Review a stack of patches, and have an agent edit it
TryExec=/home/me/.cargo/bin/stacked
Exec="/home/me/.cargo/bin/stacked" app --launched
Icon=io.github.carverdamien.stacked
Terminal=false
Categories=Development;RevisionControl;
Keywords=git;patch;stack;review;
StartupNotify=true
StartupWMClass=stacked
```

Opened from the Dock, the Finder or the launcher, StackED has no terminal,
nor what a terminal's shell sets. It first takes what your login shell
sets (`$SHELL -ilc`, else the shell the user database names for you, given
ten seconds): `PATH`, to find git and claude with, `EDITOR`,
`STACKED_FILE`. It then opens again the windows that were open when it was
last quit, but those whose database is no longer there. With none to open
again, it opens the database `$STACKED_FILE` or the settings' `file`
names, if either does, and else an empty window, its menu bar there: File
→ Open… opens one. What goes wrong is said in an alert. `$EDITOR` has no
terminal to run in there, so it runs in a terminal window of its own:
StackED writes the command in a script of its own, which removes itself as
it runs. On macOS, a window of Terminal's runs it: the script is a
.command file, and `open -a Terminal` runs it, so that macOS asks for no
permission to control Terminal. On Linux, the terminal runs it with sh:
the one `$TERMINAL` names, else x-terminal-emulator, else the first there
is of gnome-terminal, ptyxis, kgx, konsole, xfce4-terminal, mate-terminal,
tilix, terminator, alacritty, kitty, foot, wezterm and xterm. With none,
the status line says so, and the editor does not run. StackED goes on once
the editor exits; closing that window before it does changes nothing. An
editor with a window of its own (`code --wait`, `zed --wait`, `open -W -n
-a TextEdit`, `gedit --wait`) opens that window instead.

StackED's icon is drawn rather than kept as a picture (`icon.rs`): an S of
white sheets of paper, stacked along the letter, each a little further
along than the one under it so that their edges show, fanned where the S
turns, on a black rounded square. Small, as in the Finder's lists, the
sheets' edges are left out, and the S is plain white. It is the windows'
icon, and the Dock's on macOS, whether StackED is started from a terminal
or as an app; StackED.app's icon file is drawn from it at every size macOS
asks for, and so is the launcher's icon, at every size of the icon theme's.

The editor runs on a thread of its own, drawn into a buffer of the window's
size, as `stacked web` runs it for a page; the window (eframe: egui, on
OpenGL) paints what the buffer holds, under its menu bar, and sends the
editor its keys, clicks and size (`app.rs`). Each window is a viewport of
eframe's (`app/windows.rs`): the first is eframe's root viewport, which
eframe cannot do without, and the others paint themselves. The first
window closed while others are open takes over the latest of them, its
editor, its title, its place and its size; on Wayland, which lets no
program place its windows, the window that stays shows up where the closed
one was. The menu bar is egui's (`app/menu.rs`), the dialogs rfd's
(`app/dialogs.rs`), and what StackED is to each system (StackED.app, its
launcher, the terminal `$EDITOR` runs in, and the Edit menu macOS's panels
need) is in one module (`launcher.rs`).

## The editor

The left pane is the current stack: its header (`≡ bug123 · 3 patches`),
which is the stack's own place, then the patches' titles, bottom first, with
how many comments wait on an answer. When stacked runs in a checkout of the
stack's repository, a line among them says where its HEAD is (its branch,
else its commit): on the stack, `▶ HEAD main` after the patches its
commits since the stack's base are (a commit is a patch when its diff is,
whatever its message), under the header at the base; on another base,
under the header too, `▶ HEAD main: off the stack`. HEAD's commits that are
the stack's patches but whose message is not are marked, in the column
before the patch's number: `≠` when the commit's message says something
other than the patch's, and, with moz-phab, `!` when it names a
Differential Revision the patch does not name, none or another, which wins
over `≠` (Move HEAD, below, says what each locks). While there is a `!`,
HEAD's line ends `· locked below patch 2`, folded under itself where the
pane is too narrow for it.

HEAD forked from the stack on its base, with commits the stack does not
have (yours, or main pulled since, or the stack moved on since HEAD was
put there), is drawn as a graph:

```
● 1 [R]✓ Bug 1 - first part
≠ 2 [W]✗ Bug 2 - second part
├○ Fix the thing
│▶ HEAD main
└● 3      Bug 3 - third part
 ● 4      Bug 4 - fourth part
```

First the trunk the two share, the patches whose commits HEAD has, each
`●` or its mark; then the shorter of the two branches, then the longer:
the first one drawn starts `├`, its next lines `│`, the other starts `└`.
The stack's patches are `●`, every one of them, its rows as ever; the
commits HEAD alone has are `○`, by their subject, three rows at most, the
third `… 5 more commits` when there are more; and HEAD's line comes after
HEAD's branch. The cursor goes over the `○` rows: they are drawn only.
HEAD's line itself is a line the cursor stops on: its menu (Enter, Space)
has `Move HEAD…`, and `?` there says where HEAD is, its marks, where it is
locked, and which of Move HEAD's cases applies (`HEAD is as stacked left
it: Move HEAD replaces it with the stack as it is now.`), or why none does,
and then that `Move HEAD…` shows why, and what you can do (below).
On a patch marked, `?` says first what its mark means, what it locks or
grays, and how it clears; the bottom line says only that it does (`?: what
the ! on patch 2 means`). With more patches than the pane has room for,
HEAD's line stays in sight: on the pane's last line while its place is
further down (`↓ HEAD main`), under the header while it is further up
(`↑ HEAD main`), the arrow saying that patches go on between. It is looked
at again as the stack changes, and every few seconds. `stacked head NAME`
says the same.
`Move HEAD…`, in the stack's menu, each patch's and HEAD's line's, moves
that checkout along the stack: ↑↓ place HEAD's line (`↕ HEAD`), the
checkout as it is meanwhile, the patches drawn as they are, and Enter moves
it there, Esc leaving it where it was. HEAD is
then detached at the stack's base with the patches up to there applied as
commits, those the checkout has already, message too, taken as they are;
the branch it was on stays where it was. Not while files git tracks have
changes, and only as below. `stacked head NAME --to N` moves it to N
patches (0, the base).

With the `moz-phab` feature (on by default), a patch whose message names a
revision (its `Differential Revision:` trailer, Phabricator below) has that
revision's status on Phabricator after its number, a letter in brackets,
expanded and listing files alike: `[D]` Draft, `[W]` Changes Planned, `[R]`
Needs Review, `[C]` Needs Revision, `[A]` Accepted, `[L]` Closed (landed),
`[X]` Abandoned, and `[?]` a status stacked does not know (`  2 [W] Bug 2 -
add b`). The Phabricator line over the patch's message says it in full
(below). Right after the letter, a mark says whether Phabricator has this
exact version of the patch, as that line says it: `✓` where the line says
`in sync` (the revision's latest diff does what the patch does, its title
and summary are those a submit sends, and a submit would not change its
parent), `✗` where it says what differs, the diff, the title or summary,
or the parent (`  2 [W]✓ Bug 2 - add b`, `  3 [R]✗ Bug 3 - add c`). The
mark is blank while what differs is not read and compared yet, or when
Phabricator does not have the revision; the pane and the line read it in
one place, so that they never disagree. `?` in the patches pane says what
the two marks mean. The statuses are a column of their own, as wide as
the widest of the stack's, so that the titles start together: a patch
that names no revision, or whose revision's status is not read yet, has
its column blank, and while no status is known there is no column at
all; the marks are one more cell of it, there once any patch shows one.
Expanded, the pane is wider by the column, up to half the screen, so that
each title keeps the room it had and folds as it did. The tags at the end
of a patch's first line (`[2]`, and `✓` or `3/7` in the reading mode) are
where they were: the reading mode's `✓` there says that all of the
patch's lines are read, not that Phabricator has this version. A revision
is a draft when its status is Draft: one in Changes Planned that was never
sent for review, as `Submit to Phabricator as WIP` makes one, is `[W]`,
though Phabricator says it is a draft too (`isDraft`, as it says of every
revision it has not sent to reviewers yet). The
statuses are read away from the screen, over Conduit with the token a pull
uses, all of the stack's revisions in one search
(differential.revision.search; one for each Phabricator, should their
links name more than one): as the stack is shown, again when the
revisions its patches name change and right after a submit, and every three
minutes while the editor runs. They are kept as long as it runs, and
written nowhere. A read that fails leaves them as they were, and the bottom
line says so once (`The statuses of the stack's revisions could not be read
from Phabricator: …`), then again only after a read that worked. The
message names what Phabricator answered: a status other than 200 with the
first line of the page that came with it, cut short
(`differential.revision.search: Phabricator answered HTTP 502 with a page
starting "<html>"`), and a page that is not JSON by its first line too.
When Phabricator turns away the requests of this machine (HTTP 429, too
many requests), nothing more is read away from the screen for a while,
neither the statuses nor what the Phabricator lines read (below): for as
long as Phabricator's `Retry-After` says, when it says a time to come, else
for five minutes, then twice as long at each refusal in a row, up to half
an hour; a read that works ends the row. The bottom line says so, in the
place of the message above, with when stacked reads Phabricator again
(`Phabricator is turning away requests from this machine for now (HTTP
429): stacked reads it again at 14:32.`), and the statuses and the lines
keep what they had meanwhile. What you ask for is tried all the same,
once: a submit, a pull, the pull of one revision, an import; turned away
too, each says so plainly (`Phabricator is turning away requests from this
machine for now (HTTP 429, too many requests)`). The wait lasts as long as
the editor runs, each editor waiting on its own, and nothing of it is
written. Nothing is
read while reviews come from nowhere (`reviews.source = "none"`, Settings
below), nor in the tour. In the browser (`stacked web`) and in StackED, the
same.

A commit of the checkout is a patch of the stack when its diff is, but its
message may say otherwise. stacked never writes a commit's message into the
stack, but as `Reconcile with HEAD` (below) asks it: the banner, the
reviews and the menus are as they are whatever the commits' messages say. Two messages are the same when they differ only by
white space at the end of a line or by empty lines in a row, which `git am`
makes one as HEAD is moved. With the `moz-phab` feature (on by default),
the trailer moz-phab writes to name the revision, the line of the message's
last paragraph that Reading (below) says is that trailer (`Differential
Revision: https://phabricator.services.mozilla.com/D123`), is told apart:
a commit that names a revision its patch does not name, none or another,
as `moz-phab submit` leaves it in your checkout, is locked (below), and
while a patch and its commit differ by the revision they name, HEAD is not
in sync with the stack, so that it does not follow the stack as a review
closes. Built without the feature, the trailer is a line of the message as
any other.

Move HEAD replaces the commits of your checkout with the stack's, and
those commits might be yours: commits the stack does not have, or ones
amended outside stacked. So stacked remembers where it left HEAD: the
commit HEAD is at right after stacked last moved it, by Move HEAD or by
HEAD following a review, in that checkout on that stack, each move
replacing it, in the database (One file, only appended to, below). Move
HEAD replaces commits only when HEAD is exactly that commit, whatever the
stack has become since (its patches edited, reworded, the commits of HEAD
then other than the stack's): stacked made them; or when HEAD is on the
stack, its commits the stack's first patches, with nothing above them and
the same messages. Else `Move HEAD…` moves nothing: it is offered all
the same, drawn as any item, and chosen, it opens a view of why it does not
move HEAD, and of what you can do in your checkout so that it can (below);
`?` on it says so. `--to` is refused with the reason (`HEAD has commits
stacked did not make, as 1 commit after patch 2 is not the stack's: Move
HEAD moves HEAD only when HEAD is on the stack with no marks, or as
stacked last left it`). Everything else of HEAD is read from the commits,
by their diffs, never from what was remembered.

A commit that names a revision its patch does not name is never rewritten
nor dropped: everything up to and including the last such commit is kept
as it is, and HEAD goes nowhere below it. Moving HEAD, ↑ and Home stop at
it, a click below it says why (`HEAD is locked below patch 2: the commit
of patch 2 names D34, which the patch does not name`), and `--to` there is
refused with that reason. From it up to the stack's top, HEAD may go
anywhere: the stack's patches apply on it, as those commits have their
patches' diffs. `stacked head NAME` lists the patches whose message
differs from its commit's (`the message of patch 3 differs from its
commit's: "Bug 3 - other words"`; with moz-phab, when the two differ by the
revision they name, or would but for it, `patch 3 names no revision in the
stack and D3 in your workspace's commit`), then where HEAD is locked, and
why `--to` would be refused, when it would.

The view `Move HEAD…` opens while it would not move HEAD is only to read,
a screen of its own laid out as the review of what the agent brings back is
(below). On the left, first, where HEAD is and why Move HEAD does not move
it (`HEAD of your checkout, on main at 9f8e7d6c5b4a, is at patch 2 of s,
then 1 commit not in the stack.`); then each thing that keeps it from
moving HEAD, a line each, under a title for each kind: each commit only
your checkout has, after the stack's patches it has, by its subject (the
first ten, then `○ … 3 more commits`); each commit with its patch's diff
but another message, `≠`, then each that names a revision its patch does
not name, `!`, by its patch; or HEAD on another base. On the right, the
line the cursor is on: the commit as `git show` shows it, drawn as the
middle draws a patch (a diff of more than 2,000 lines said, not drawn); the
patch's message then the commit's, compared where their lines wrap aside,
as the reviews compare two messages; or HEAD and the stack's base, how
they stand (the commit they share and how many commits each has since, or
none shared), with HEAD's latest commits. Under it, stacked's comment says
why that keeps Move HEAD from moving HEAD, and what you can do in your
checkout, the git commands written out, their commits named: the commits
only your checkout has put on a branch first when HEAD is on none (`git
branch my-work 9f8e7d6c5b4a`, any name not taken), then HEAD back on the
stack's last commit there, or on its base (`git switch --detach
1a2b3c4d5e6f`), the branch HEAD was on keeping them; a commit marked given
to its patch by Reconcile with HEAD (below), or reworded in your checkout
to the patch's message (`git rebase -i 1a2b3c4d5e6f^` and `reword` on its
line, or `git commit --amend` for HEAD's own commit); HEAD on another base
switched to the stack's base, its commits first put on a branch when it is
on none, or kept where it is, Move HEAD left alone. stacked runs none of
them. What the view shows of the commits is read away from the screen as
it opens, `reading your checkout…` on the top bar meanwhile. As HEAD is
looked at again, every few seconds, the view follows it: once nothing
keeps Move HEAD from moving HEAD, it says so, and `Move HEAD…` moves HEAD
again. ↑↓ go from line to line, → (or Enter) into the pane on the right
to scroll it, → again to see it wide, the list out of sight, ← back, and
Esc goes back where the view was opened from; `?` says so. In the browser
(`stacked web`) and in StackED, the same.

`Reconcile with HEAD`, in the menu of each patch marked and of HEAD's
line, takes the commits' messages into the stack: each patch marked, `≠` or
`!`, takes its commit's whole message as a new version, all of them in one
edit stacked makes, with no agent. It joins the queue as an edit of the
agent's does (Asking the agent, below), and is made at its turn, at once:
from the stack the edits before it make, chained on the last of them that
waits for your review, as chained edits are. It then waits for your review
in the banner, in turn (`⟳ Review a change of patch 2, reconciled with
HEAD: its commits' messages`); while it waits its turn, the banner says so
(`◌ Reconcile with HEAD, waiting its turn: the commit messages of patch 2
taken from HEAD`), and Enter on that line opens its menu, which says what
it waits for. Its `Drop the waiting reconcile with HEAD…`, there, on
HEAD's menu and the patch marked's, and Delete on its line, takes it out
of the queue once asked, as if it was never asked: nothing changes, the
marks stay, and the edits asked after it no longer wait for it (What the
agent brings back waits for your review, below). Edits
asked after it do not start before it is accepted or rejected, and then go
on from the stack that leaves (`ready (may change the code), it goes once
you have reviewed the reconcile with HEAD`); answers go at once, as ever.
The work it went on from accepted, it goes on from the stack that made;
rejected, sent back or edited by hand, it is dropped and made again at its
next turn, from the stack as it then is. One at a time: while one is under
way, the item is grayed, its menu saying why, as it is when no commit is
marked or the stack cannot change.

Its review shows what it takes from your checkout: the stack operations
(`reword  2 Bug 2 - …`) and each new version's interdiff, its message
compared with the patch's where their lines wrap aside (History, below).
Under them, `stacked's comment, to read`, nothing to accept or reject,
says what to run in your checkout should you reject it, to give the
commits the stack's messages instead, from the top down: `git commit
--amend` for HEAD's own commit, `git rebase -i COMMIT^` for one below,
marked `reword`, each reworded to its patch's message as the diff shows
it. Accepted, the patches have their commits' messages, and the marks are
gone; rejected, nothing changes in the stack, and the marks stay.

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

With the `moz-phab` feature, a patch that names a revision has the
Phabricator line over its message, in every view of the code, reading or
not, its hunks or each file whole:

```
Phabricator: D123456 · Needs Review · the diff differs · 2 new comments to pull
✉ message
   Bug 123 - the parser skips the blank lines
```

It says the revision; its status, by its full name: Draft, Changes
Planned, Needs Review, Needs Revision, Accepted, Landed (Closed, as
Phabricator names it), Abandoned, or `status unknown`; whether Phabricator
has this version of the patch, `in sync` when the revision's latest diff
does what the patch does (the same effect, or each file the same once both
are applied on the patches below it, as a submit's check takes them: the
line keeps those two strict rules, and not the submit's third, as a diff of
the same changes made on the patches below as they were before, which no
longer applies on them, is not what would land), its
title and summary are to the letter those a submit sends (`WIP: `
first or not, as the revision's title is; the summary without the lines
naming a revision; the white space at their end aside), where their lines
wrap too, as Phabricator's are what lands: a message wrapped again since
it was submitted is not in sync until it is submitted again; and a submit
would not change its parent, by the submit's own rule (Submitting a
patch, below): the revision of the patch below, when that is open, is its
parent already, or a submit sets none (for the bottom patch, on a patch
with no revision or one naming the same revision, on a revision closed or
that Phabricator does not have), so that once the stack is reordered, a
revision moved is not in sync until it is submitted again; else what
differs, listed: `the diff differs`, `the title or summary differs`, `the
parent differs`, `the diff and the title or summary differ`, `the diff and
the parent differ`, `the title or summary and the parent differ`, or `the
diff, the title or summary, and the parent differ`, always against the
patch as it is, never its older versions; and how many of the revision's comments are not
pulled yet: `2 new comments to pull`, `1 edited comment to pull` (edited on
Phabricator since it was filed), `no new comments`. Those are counted from
what the database holds as it stands, never from its history: a comment is
pulled when a comment open holds it, on the patch or moved to the stack's,
or when a pull's review waiting holds it. The line does not count the
comments of a thread you closed, though a pull brings the new ones: without
the history, a new reply or edit there is not told from the comments filed
in it before you closed it. What is not read yet is `…` (`Phabricator: D123 ·
Needs Review · …`), whether it is in sync too while the revision of the
patch below is not; a revision Phabricator does not have is said so,
alone (`Phabricator: D123 · not found on Phabricator`). Whether each patch
is in sync, the patches pane marks too, after its revision's status (`✓`,
`✗`, above), from the same reading as its line. The line is a row
of the patch, over its message's header, and folds between words where
the middle is narrower; it is not pinned, and scrolls away with the rows,
the title pinned in its place. The cursor comes to a patch on its
message's header, the line over it in sight; ↑ goes onto it. There, Space
shows what differs on a screen of its own, titled `D12 on Phabricator,
then patch 2 as it is`: Phabricator's version of the patch, the
revision's title and summary as a submit compares them over its latest
diff, then the patch as it is, as an interdiff, the way `Versions of this
patch…` shows two versions. The screen first says what it shows: the
parent first, when a submit would set one, the revision's parents on
Phabricator then the one a submit sets (`Parent: D10 on Phabricator; a
submit sets D11, the revision of patch 2.`, `Parent: none` for a revision
with none; a parent that no patch of the stack names is named by a search
of its own). The
title and summary are compared where their lines wrap aside, what only
each has marked; when they differ only in where their lines wrap, or in
their white space, the screen says so, and each paragraph written
otherwise is there as each version writes it, Phabricator's lines taken
out and this version's put in, no word in the colours for words. The code
is the two diffs, as an interdiff, what git derives in them left out of
both, as Phabricator's raw diffs have none of it (the `index` lines, the
heading after a hunk's `@@`): Phabricator's lines taken out, this
version's put in. When the latest diff is on another base than this
version (the two read a file before the patch otherwise: a line at the
same place is not the same, the same hunk is at another place, or one
makes a file the other finds there), the code is instead each file as
that diff leaves it, then as this version leaves it, both on the patches
below it, as the line compares them, and the screen says so; where the
files cannot be read so, the code is the interdiff all the same, and the
screen says that too. When the code is the same, the screen shows the
title and summary alone; both the same, the parent alone differing, it
shows nothing under what it says. ↑↓ (pgup pgdn home end) scroll it, and
Esc goes back to the line. In sync, Space says so on the bottom line,
naming the parent (`Phabricator has this exact version of patch 3: the
latest diff of D12 does what this version does, the title and summary of
D12 are those of this version, and its parent is D11, the revision of
patch 2.`), or saying why a submit sets none, as a submit says it (`…
Patch 1 is at the bottom of the stack, so D11 gets no parent.`), and
opens nothing; the screen says the same, in sync (`Nothing differs: …`);
while what differs is not read yet, Space says that. Space there marks nothing read,
even in the reading mode, and opens no menu: Enter opens it, titled `D12
on Phabricator`. Its first items are the revision's: `What differs from
D12 on Phabricator…` opens the same screen, grayed, the menu saying why, while the revision is not
read and compared yet, or Phabricator does not have it; and `Pull the
reviews of D12 from Phabricator` pulls the reviews of that revision alone
(Phabricator, below). The patch's menu in the patches pane has both too,
after its submit item: `Submit to Phabricator as WIP`, or, for a revision
open but not in work, the update of its title, summary and parent alone,
which the line's comparison names, offers or grays (`Update D12 on
Phabricator: its title and summary`, Submitting a patch, below). `?`
says what the line says, and that the patches pane marks the same, what
Space shows there, and what the menu has.

What the lines say is read away from the screen, with the token the
statuses use, for every revision the stack's patches name, as the stack is
shown, so that each patch's line is whole as soon as the patch is: after
the statuses' search, the revisions' parents (edge.search) and their
latest diffs (differential.diff.search, the newest first, page after page
until each revision has its newest), each in one call for all of them;
the parents that are none of the stack's revisions, of a revision a
submit would set another, named in one search of their own
(differential.revision.search); then each revision in turn, that of the
patch the middle shows first: its latest diff, raw
(differential.getrawdiff), unless it was read before, and its comments
(transaction.search). Those two are read only for a revision not read yet,
or modified since it was read, as the statuses' search says when each was
last modified (its `dateModified`): what was read of the others is kept,
and only their parents are read again, as a revision's parents also change
when another revision is edited to take it as its child. A submit forgets
what was read of the revision it sends to, and a pull what was read of the
revisions it reads, so that those are read again in the next round.
Phabricator says that a revision was modified at each change to it, a new
comment among them, but not when one of its comments is edited or removed:
the line counts such a comment once the revision changes otherwise, or
once a pull reads it. One read of Phabricator after another, the statuses' and
this: the calls for all of the revisions again each time the statuses are
read, every three minutes, and right after a submit and after a pull, and
those of each revision as above. For 15 revisions none of which was
modified, a round is three calls, as a rule: the statuses' search,
edge.search and differential.diff.search; one modified adds its
transaction.search, and its getrawdiff when its latest diff is a new one.
All of it is kept in memory by revision as long as the editor runs, and
the raw diff by its id, not read again while it is the latest. What
differs in the code of each patch is worked out away from the screen too,
one patch at a time, the one the middle shows first, once for each version
of the patch on the patches below it and each latest diff. A read that
fails keeps what was known, and the bottom line says nothing more than the
statuses' read says, but when Phabricator turns this machine away: then
nothing is read away from the screen for a while, and the bottom line says
until when (above). Nothing is read, and no line drawn, while reviews come
from nowhere, nor in the tour. In the browser and in StackED, the same.

There are six keys, four more for the middle: three of self-review's
(Copied from self-review-rs, below, says what it is) and one of stacked's
own; and `o`, on a line of a commit message. The settings can make any of
them other keys, and the help then names those:

- The arrows move. ↑↓ go through the patches (the header is above the
  first), the lines of the patch, or the comments (their header above the
  first); pgup, pgdn, home and end go further. A pane longer than the
  screen stays as it is while the cursor moves within it, and scrolls
  only once the cursor would go past its top line or its bottom one, as
  far as it takes to show what the cursor goes to whole (a patch whose
  title folds, all of its lines): from the top line going up, down to the
  bottom line going down. ← and → go from pane to
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
  of the others. ↑ from a patch, or from HEAD's row, to the patch above
  picks it in the same way, its files listed in place of the others, and
  goes to the last of them, so that ↑ goes back up the lines that ↓ came
  down; when the patch above has no file to list, ↑ goes to that patch
  itself. Enter or → on a file goes into the middle there, and a click on
  a file goes to it. While the cursor is in the middle, the pane shows the
  file the cursor is in as picked, and ← comes back to that file. While
  one of a patch's files is picked, the patch's own lines, once scrolled
  past, stay on the pane's top lines, drawn as they are in their place:
  its number, its revision's status and mark when it has them, and its
  title, folded as it folds there. The files scroll under them, and the
  pane moves only once the cursor would go past the line under them or
  the bottom one. In a pane too short for all of them and a line for the
  file, the first of them stay, as many as leave that line (HEAD's line,
  when it is pinned, keeping its own), and none in a pane of one line. ↑
  above the first file shows them in their place again, the cursor on the
  patch, and a click on them goes to the patch itself. ← again
  hides the patches, for the middle to have the width, and ←
  from the middle brings them back expanded. → on the comments makes them
  compact, a line for each (who, and where it is); → again hides them, the
  bar saying how many there are; and → from the middle brings them back
  expanded, each with the first line of what was said last. → from the
  middle opens the comments pane whatever the patch has, the cursor on its
  header when it lists nothing: the header says what the filter is and that
  it lists nothing (`all · nothing listed`), and the line under it why
  (`This patch has no comments.`, `The filter shows the comments to review,
  and hides this patch's only comment: Filter comments…, on this header,
  picks another.`, `The filter shows the closed comments, and this patch
  has none: …`), as `?` does; Enter or Space on the header is its menu, the
  filter's item there. With nothing on the patch for it to show, the pane
  goes again as ← leaves it, so that the middle keeps its width, and → opens
  it again; one the filter empties stays, as there are comments to show.
- Enter does what is to be done where the cursor is, reading or commenting:
  on a patch, read it; on a file the patches pane lists, read the patch
  there; on a line, comment on it; on a comment, reply, or, while the agent
  works on it, open what it is writing; in the comments pane, go to the
  comment. Elsewhere it is the menu.
- Space is the menu: all that can be done where the cursor is, then undo,
  redo, `Runs…` and quit, which every menu ends with. The stack's items
  (comment on it, ask about all, `Filter comments…`, the stack as a list,
  the tree, the history, Phabricator, `Move HEAD…`, switch, delete) are on
  the left header only, but `Filter comments…`, which a patch's menu has
  too; a patch's on a patch; a line's on a line; a comment's on a comment.
  ↑↓ and enter; ↑ from the top is Quit. On the Phabricator line over a
  patch's message (with moz-phab, above), Space shows what differs from
  Phabricator, on a screen of its own, and Enter is the menu. What a
  read-only stack cannot do is shown dimmed, and the menu says why. An
  item may have choices, radio
  buttons after its label (`[x] tmp/file  [ ] pwd`): ← → pick one, and
  stop at either end rather than go round, so that ← held ends on the first
  and → held on the last; a click
  on one picks it, and Enter does the item with the one picked, which is
  kept for the stack in the database. Every `Ask agent…` item opens the
  ask dialog rather than asking from the menu (Asking the agent, below),
  and so does a review's `Send it back to the agent…`. An item that
  sends again what was asked before ends with a dropdown of the effort it
  goes at, `Effort [high]`: `Send it now` and the batch's items. On an
  item with more than one
  dropdown, ← → pick in the one underlined, which is the first until Tab
  goes to the next one; shift-Tab goes back, as the bottom bar says
  (`←→: to whom · tab: effort`). A click picks in the dropdown it is on:
  the choice clicked, or, on `Effort [high]`, the next level, round from
  `max` to `low`.
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
  works on the comment, it is `Stop the agent…`; while the comment's ask
  waits its turn, it is `Take it back from the agent's queue…`, which takes
  the ask out of the queue and leaves the comment open rather than closing
  it; and while the comment is in the batch, it is `Remove it from the
  batch`. On a patch, it
  is `Drop this patch…`; on the stack's header, it is `Delete this
  stack…`; on a file the patches pane lists under a patch, it drops the
  patch's changes to that file (Editing a patch by hand, below). In the
  review of landed work, it is `Mark as rejected` on a part, `Close it with
  the review` on a comment the ask was about, and `Take it back` or `Take
  your answer back` on what you wrote there; on a part of a pull's, which
  is not rejected, it does nothing, and the status line says why. On the
  banner, it rejects all that remains of the landed work picked and closes
  its review, as the review's `Reject all remaining and Close` does, but
  for a pull's, which it does not reject; on a comment's line, it
  does what it does on the comment; on the stack's lint job, it is `Drop
  the waiting lint job…`, or `Stop the linter…` once it runs; on a
  reconcile with HEAD waiting its turn, it is `Drop the waiting reconcile
  with HEAD…`. On the runs, it stops an ask that runs, and drops a lint
  job or a reconcile with HEAD that waits its turn. An item ending with `…` asks first, and so does Delete; Delete also
  asks before it takes back what you wrote in a review, and before it
  rejects landed work from the banner. On a comment closed, it does
  nothing, and the status line says so. In the reading mode, on the code
  in the middle, it takes a mark back (Reading, below). It does nothing in a menu,
  nor in History…, the tree and the versions; and where what is typed is
  read (the comment editor, a line typed in a box, `/`'s bar, the search's
  own screen, the setup's `Add a command…`), Delete and Backspace edit it.
  The bottom bar says what Delete does where it does something (`delete:
  close it`), and so does `?`.
- `o`, on a line of a commit message, opens its link in your browser, as
  the line's item in its menu does: `Open https://example.org/a in your
  browser`, for a web link written in the line (`https://` or `http://`,
  what ends a sentence after it or a bracket around it left out; between
  angle brackets, `<https://…>`, all they hold; in the menu, a long one is
  shortened to fit the screen); `Open "the docs" in your browser`, for a
  markdown link, `[the docs](https://…)`, named by its text, its link up
  to the `)` that closes it (a text that is a link itself, or holds one,
  is not said: the item says the link it goes to); and, with the
  `moz-phab` feature, `Open bug 1234567 in your browser`, for a bug the
  line names (`Bug 123`, `bug123`, `b=123`, in any case), `bug` a word of
  its own, so that `debug 7` names none, and the rest as moz-phab reads a
  bug in a title (a submit reads a title as moz-phab does, `debug 7` its
  bug 7), on Bugzilla (`reviews.bugzilla`, Settings, below),
  and `Open D123 in your browser`, for the revision its `Differential
  Revision:` trailer names (the settings' `reviews.trailer`), at the
  trailer's link, or, a bare `D123`, on the settings' Phabricator
  (`reviews.url`). A bug written in a web link is the link's, not one of
  its own, as is one in a markdown link's text, and the trailer's link is
  one link, the revision. On a line with
  several, `o` opens the menu of them, `the links of this line`; on a line
  of the message with none, the status line says so; anywhere else, it does
  nothing. A right click on the line opens its menu, with the items; a
  click never opens the browser. A link opens away from the screen, with
  the program a comment's web link opens with (`$STACKED_OPEN`, else the
  system's own: Writing comments, below), and the status line says what
  came of it (`opened https://bugzilla.mozilla.org/show_bug.cgi?id=1234567
  in your browser`). It is so wherever the cursor is on a line of a
  message: the patch's message in the middle, commenting or reading; in
  the review of landed work, a patch looked at closer, and the lines of the
  messages in the interdiffs of the stack diff (one the new version takes
  out, the old version's); in History…'s stack diff, the same; and in what
  a submit sends to Phabricator, the lines of the title and summary, both
  sides' side by side. The versions of a patch have their cursor on a
  version: no line of theirs is under a cursor. The bottom bar says it where it opens
  something (`o: open bug 1234567`, `o: open the link`, `o: open a link`),
  and so does `?`; `open_link` in `[keys]` (Settings, below) makes it
  another key. A line of what a comment says has the same items in its
  menu (Writing comments, below), but `o` opens nothing there. In the
  browser (`stacked web`) and in StackED, the same.
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
  body, which are compared word by word, where their lines wrap aside
  (History, below). Made
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
  it. Esc, then `Ask agent…`, opens the ask dialog, which saves the comment
  and asks the agent; what it makes waits for your review, as any of its
  work does.
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
the terminal's own colour, so that no comment reads as code. The agent's
name says the effort its run was asked at, when it was asked at one
(`▎ agent(max):`, `└ agent's comment (max):`): Asking the agent, below.
A reviewer's
comment edited on Phabricator is a turn of its own, headed by what it
edits (`ann's edit of the comment that said "Hi Da":`), and the turn it
edits stays, headed `ann (redacted):` (Phabricator, below). The colours
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

The menu on a line of what was said has an item for each link the line
has, to open it in your browser, as a line of a commit message has (`o`,
The editor, above), in a section of their own after the comment's items:
`Open https://example.org/a in your browser` for a web link, `Open "the
docs" in your browser` for a markdown link, `[the docs](https://…)`,
named by its text, and, with the `moz-phab` feature, `Open bug 1234567 in
your browser` for a bug the line names and `Open D123 in your browser`
for a revision a `Differential Revision:` line names. A line folded to
fit the pane is one line: each of its pieces has all of its links. A line
of a code block has none, and neither does the line that heads the
comment (`└ comment:`) nor the name of who said a turn. A comment
collapsed has the links of the first line it says; in the comments pane
expanded, a comment has those of the line said under it, the first line
of what was said last, and none when the pane is compact. `o` opens
nothing there: it opens the links of a line of a commit message. In the
browser (`stacked web`) and in StackED, the same.

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
to read before you write. The box grows to show the whole thread, up to
half the pane. When the thread is longer, ↑↓ (pgup, pgdn) scroll it down to
its last line, and the box's foot and the bottom bar say so (`↑↓: read the
last comment closed here`); the wheel and the trackpad scroll it too
wherever they are ↑↓ (in the browser, in StackED, and in a terminal that
sends ↑↓ for them). When the whole thread shows, there is nothing to
scroll, and neither says ↑↓. The first thing you type takes its place.
What you write is the comment; the closed one stays as it was.
ctrl-a and ctrl-e go to the start and the end of the line. ctrl-k cuts the
rest of the line, or at its end the line break, and ctrl-k again adds what
it cuts to that; ctrl-y puts what was cut last where the cursor is, in this
comment or in the next one. ctrl-← and ctrl-→ go a word back and a word on,
as alt-← and alt-→ (alt-b and alt-f) do; a word is what is between blanks.
On a Mac, ctrl-← and ctrl-→ switch desktops unless that is turned off in
the keyboard settings, so use alt (option) there. Esc
asks what to do with the comment: `Save it`, `Ask agent…`, which opens
the ask dialog, to save the comment first and then ask the agent about
it, now or later (Asking the agent, below), `Attach a reference…`
(below), `Throw it away`, `Keep writing`. The menu opens under the
comment (above it when there is no room under), and the comment stays
where it was written, to read again before it is saved; so does the
dialog. What the agent may do there is as it is from a comment's own
`Ask agent…`: change its patch or the whole stack for a patch's comment,
the stack for the stack's; build and test, and try push, which is built
with the `try-push` feature, for the stack's or a patch's as a whole, as
you write it, reply to it or write it again. The dialog's Cancel, and
Esc there, go back to writing the comment, as it was. Esc
asks even when nothing changed, so that what you wrote earlier can be sent
as it is: as nothing would be saved or thrown away, the menu is then `Ask
agent…`, `Leave it as it is`, `Keep writing`; the dialog's title, and `?`
on the item, say which way it goes, saving first or asking about the
comment as it is. Changed, what you wrote earlier (your reply, a turn written
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
program `$STACKED_OPEN` names, given the link as its one argument; else as
the system opens links, which the `open` crate knows: with `open` on
macOS, and elsewhere with `xdg-open`, else gio's, gnome-open or KDE's, the
first there is. The status line says what was opened, or why it could not
be. StackED and `stacked web` open it the same way, on the machine stacked
runs on. A mention
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
agent…` saves it and asks it again, as its dialog sets). In $EDITOR it opens
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
  see the whole project as the patch leaves it. Only while `Move HEAD…`
  would move HEAD there and no commit is locked (The editor, above): else
  `pwd` is grayed, ← → go past it, the menu says why, and an edit there,
  as the stack's menus last picked it or from the command line, is
  refused with the reason (`this checkout is not offered for edits by
  hand: HEAD is locked below patch 2: …; nothing changed`).
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
long; code, as an interdiff reads it (History, below): a fenced block
(```` ``` ```` or `~~~`), its empty lines and all, and a paragraph with a
line indented by a tab or by four spaces or more, or with a line starting
with a `$ ` prompt; a quoted line (`> `); the trailers, the last
paragraph from its first `Key: value` line on (`Reviewed-by: ann`,
`Differential Revision: …`); and a paragraph whose lines all fit, its short
lines not joined. So nothing but where the lines break changes: the
interdiff of the edit's review, which compares the two where their lines
wrap aside, has each paragraph wrapped there once, as the new version
writes it, none of its lines a change (History, below). The width is the settings' `view.message_width`, 72
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

Every item that asks the agent opens the ask dialog rather than asking
from the menu: a comment's `Ask agent…` (`Ask agent, answer only…` on a
question on the history), `Ask agent about all…` on the left header (the
stack's comments) or on the comments pane's header (each comment the list
shows), the comment editor's `Ask agent…` (above), and, in the review of
the agent's work, `Send it back to the agent…`, whose dialog has Send Now
and Cancel for its buttons (What the agent brings back waits for your
review, below). Its title says what is asked about; under it are two
lines, then three buttons:

```
┌─ ask the agent about the comment on patch 1 · a.txt: TWO ────┐
│ Permission: [ — ]                                            │
│   Not set yet: ← or → sets answer only, then steps to change │
│   patch and change stack. Enter opens the list of them.      │
│                                                              │
│ Effort: [ — ]                                                │
│   Not set yet: ← or → sets low, then steps to medium, high,  │
│   xhigh and max. Enter opens the list of them.               │
│                                                              │
│ < Ask Now >   < Ask Later >   < Cancel >                     │
│                                                              │
│ Ask Now and Ask Later are grayed until the permission and    │
│ the effort are set.                                          │
└──────────────────────────────────────────────────────────────┘
```

Both lines are unset as the dialog opens, every time: nothing set in a
dialog is kept for the next one, for the stack or between runs, so that no
ask goes as the one before it went by mistake. On a line, ← or → sets its
first option, `answer only` or `low`, whichever the arrow, so that a stray
key never sets the most permissive or the most expensive one; then → steps
to the next option and ← to the one before, and neither goes round past
either end, as the menus' dropdowns do: ← held ends on the first, and →
held on the last.

Enter on a line opens the list of its options, every one the line offers
here, in their order, just under the line and over the dialog, framed as
a menu is, the cursor on the one set, or on the first while the line is
unset:

```
┌─ ask the agent about the comment on patch 1 · a.txt: TWO ────┐
│ Permission: [ — ]                                            │
│   Not set ye┌──────────────┐nswer only, then steps to change │
│   patch and │ answer only  │ter opens the list of them.      │
│             │ change patch │                                 │
│ Effort: [ — │ change stack │                                 │
│   Not set ye└──────────────┘ow, then steps to medium, high,  │
│   xhigh and max. Enter opens the list of them.               │
│                                                              │
│ < Ask Now >   < Ask Later >   < Cancel >                     │
│                                                              │
│ Ask Now and Ask Later are grayed until the permission and    │
│ the effort are set.                                          │
└──────────────────────────────────────────────────────────────┘
```

In the list, ↑↓ go from option to option, round past either end as in a
menu, and Home and End go to the first and the last; Enter sets the line
to the option the cursor is on, and Esc leaves the line as it was. Either
closes the list alone: the dialog stays open, the cursor on that line.
While the list is open, the other keys of the dialog do nothing; the
bottom bar and `?` say what Enter and Esc do there. ↑↓ go from line to
line and to the buttons, where ← → go from one button to the next, past
those grayed, and Enter presses the one the cursor is on. A click on a
line opens its list, as Enter does, and a click on an option of the list
sets the line to it; while the list is open, a click anywhere else closes
the list alone, as Esc does there, and the dialog stays open. A click on
a button presses it, and a click out of the dialog cancels it, as Esc and
Cancel do: nothing is asked, and from the comment editor you are back to
writing the comment. Ask Now and Ask Later are grayed until both lines
are set, and the dialog says which one is missing. Ask Later puts the ask
in the stack's batch (below). Ask Now is grayed as well while the agent
cannot be reached, and the dialog says why:

- `Ask Now is grayed: claude is not found on the PATH.` The program the
  asks run as claude (`$STACKED_CLAUDE`, else `agent.command`, else
  `claude`) is looked for as the dialog opens: a name in each folder of
  the PATH, a path as it is.
- `Ask Now is grayed while stacked checks that the network reaches
  api.anthropic.com.` As the dialog opens, stacked makes a TCP connection,
  away from the screen, to where claude connects first: the proxy that
  `HTTPS_PROXY` (or `https_proxy`) names, when one is set; else the host
  of `ANTHROPIC_BASE_URL`, when it is set; else `api.anthropic.com`, port
  443. Nothing is sent over it, and it is given 2 seconds. What it found
  holds for 30 seconds, so that dialogs opened one after the other do not
  check again.
- `Ask Now is grayed: the network does not reach api.anthropic.com.` No
  connection was made in time. Ask Later still puts the ask in the batch.
  Where the check is wrong though claude works, `stacked config
  agent.reach_check false` turns it off, as the dialog says too.

The tour checks neither: its agent is a stand-in, in memory.

The permission, what the agent may do, offers what applies where the
agent is asked, in this order; the line under the permission says what
the one set lets the agent do. Of the comments listed (`Ask agent about
all…`), those that can be asked so are asked: that line says how many,
and Ask Now and Ask Later are grayed while none can be.

- `answer only`: read-only, enforced by its tools: it reads the code and
  the stack, and, unless its list says otherwise (`agent.answer_tools`,
  below), it may read the web with `curl` (a bug on Bugzilla, a revision on
  Phabricator, a page of documentation), through `jq` if need be. To read,
  stacked gives it `Read`, `Grep`, `Glob` and `LS`; the commands that only
  read: `grep`, `rg`, `cat`, `head`, `tail`, `wc`, `ls`, `find`, `cut`,
  `tr`, `stat`, `which`, `dirname`, `basename`, `realpath`, `diff`, `cmp`
  and `comm`; git's commands that only read: `git show`, `log`, `diff`,
  `ls-files`, `rev-parse`, `blame`, `cat-file`, `ls-tree`, `merge-base`
  and `describe`; and stacked's commands that read the stack. The options
  of these that write a file or run a program are refused, whatever its
  lists allow: git's `--output`, and, as to every ask but a build and
  test, `find -exec`, `rg --pre` and `rg --hostname-bin`. It is not given
  `git grep`, `sort` or `file`, because their short options can be
  bundled: `sort -uo FILE` is `sort -u -o FILE`, which overwrites FILE, and
  `git grep -nOCMD` runs CMD as its pager, so that no rule can refuse the
  option that writes a file or runs a program. It searches with `grep`,
  `rg` and `Grep` instead. Should its list give them all the same, their
  options that write or run a program are refused where they stand alone:
  `sort -o`, `file -C` (`--compile`), `git grep -O`
  (`--open-files-in-pager`, however shortened). Nor is it given `sed`,
  `awk`, `uniq` or `tree`, which can write files or run programs however
  they are called. The review of its work never offers it any of these
  (The tools the agent could not use, below). It may
  also use what your own claude settings allow, and what its list and
  stacked's `agent.tools` add (the stack's checkout's own, if it sets them:
  Settings, below), your MCP servers' tools among them. What it was not let use
  is said in its stream (`⛔ not allowed: mcp__moz__get_bug: …`) and in its
  run's summary, and each tool it could not use is an item of the review of
  its work, to allow for this work or from now on (The tools the agent
  could not use, below); every ask, a change's too, is told not to stop at
  a refusal but to work around it, to say so in what it writes, the tool
  not allowed, never that the service behind it (Bugzilla, Phabricator)
  refused, and to say why it needed each tool and how it worked around
  it. It works in a worktree
  of the stack's own for answers (below), never in your checkout, the stack
  up to its place applied, as a change's worktree is: it reads the patches
  there with `git show`, and the whole stack with `stacked export`. Only the
  reply comes back. An
  answer may open comments of its own on lines within the place, as a
  review does, each about one issue (a line with two issues gets two
  comments, both on it): the asking comment then says `opened 4 comments, 1 with your
  replies`, and its menu goes to them, or closes it and them. A question
  on the history has this permission alone.
- `change patch`, on a patch's comment: it may change the code within the
  comment's place, its patch and no other; its reply says what changed.
  It may run the checkout's linter on the files it changes, to check them
  (Linting, below).
- `change stack`: a change, with the whole stack in its worktree: it may
  change any patch, add patches anywhere, reorder them. It is how the
  stack's own comments change the stack; on a patch's comment, the comment
  stays on its line, and its reply goes in its thread. On the command
  line, `stacked send NAME ID --whole-stack`.
- `build and test`, on the stack's comments and a patch's as a whole: it may
  run any command, in the stack's build worktree (below), by its own rules
  (CLAUDE.md), never the user's worktree; it changes nothing in the stack.
  What it found comes back in the reply and on each patch's title, saying which
  stack it tested.
- `try push`, built with the `try-push` feature, on the stack's comments
  and a patch's as a whole, as `build and test` is, never on a line or a
  file, nor on the history: it selects the CI tasks the comment asks for,
  for a try push of the patches up to the comment's (all of them, for the
  stack's), as few or as many as the comment says, each by its exact label
  and with why it is relevant. It reads as an answer reads, where an
  answer works, and is given what an answer is given and its own list
  (`agent.try_tools`); it pushes nothing and changes nothing. What it
  selects waits for your review, and stacked pushes what you accept (Try
  push, below).

How hard the agent works on an ask is its effort, as claude's `--effort`
takes it: `low`, `medium`, `high`, `xhigh` or `max`, the dialog's second
line. What sends again what was asked before ends with the effort it goes
at, `Effort [high]`: `Send it now` and the batch's items (below), each at
the effort it was asked at unless another is picked there. On an item
with choices of its own, Tab goes to
the effort, shift-Tab goes back, and ← → pick in the one the menu is on.
Neither goes round past its end: ← held ends on `low`, and → held on
`max`. A click on `Effort [high]` picks the next level, round from `max`
to `low`. What is picked there is that item's alone: no other ask starts
at it, and nothing is kept between runs. The items that open something to
write first, `Ask agent about this, answer only…` on the history and `Ask
the agent to make a stack…`, pick no effort: the dialog of what you write
there does.

An ask goes at the effort set for it, whenever its turn comes: the
comment keeps that effort while it waits, ready or in the batch, and the
ask keeps it once it starts. claude is given `--effort LEVEL` after the
settings' `agent.args`, in place of an `--effort` they give, and the
stream's first lines say so (`starting: claude -p … --permission-mode
acceptEdits --effort high, in …`). A comment in the batch keeps the effort
it was asked at: its `Send it now` starts at that effort, and each of the
batch's items at the highest effort of the comments it sends. An ask asked
again goes at the effort it had: a change dropped because the work it went
on from was rejected, an answer restarted as a change is accepted, a
comment put back on the queue as it was. Work sent back from its review
goes at the effort its dialog sets, unset as the dialog opens, as every
ask's is, whatever the work was asked at; from the command line, it goes
at the effort it was asked at, unless `--effort` says otherwise (below).
An ask queued by an older stacked has no effort of its own: it goes at
the agent's default, or at the settings' `--effort`.

What the agent brings back says the effort it was asked at wherever the
agent or its work is named, so that work done at `max` is told from work
done at `low`. Each turn of the agent's in a thread is headed with it,
`agent(max):`, in the middle, in the previews of the comments pane, in a
review, in the browser and in StackED; the comments pane says `agent(max)`
as who opened a comment the agent opened, and before the agent's last word
(`agent(max): It is a shout.`), and the comment's head says `agent's
comment (max)`. A turn is named with it (`Edit the agent's reply (max)`,
`the agent's reply (max): edit it`, `you edited the agent's reply (max)`
in the comment's history), and a comment's state says it after how it was
asked (`with the agent (answer only, at max)`, `agent replied (answer
only, at max)`, `the agent's reply (max) waits for your review`). The
banner says it of what landed, before what was asked (`⟳ Review 1 reply,
asked at max: "why so loud?"`), the review's title after whose work it is
(`the agent's work (max): 1 reply · asked "why so loud?"`), as the menu of
a comment its reply waits on names it (`Review the agent's work (max)…`),
`Runs…` and `stacked runs` after what each run is (`answer(max) s "why so
loud?"`), `stacked landed` after what is left of the work to review, a
column of its own (`asked at max`), and `stacked comments`, the search and
the history's comments as the editor heads the turns (`agent(max): It is a
shout.`); the transcript's first lines give it as claude was given it
(`--effort max`). The effort goes with each turn, on its fence (One file,
only appended to, below): a reply or a comment the agent opened says it
however long its work waits for review, and once accepted; so does a reply
that moves a comment to the stack's comments, and a try push's replies, at
the effort its tasks were selected at. A turn written before efforts were
picked, or by an ask that went at the agent's own, says nothing of it, and
shows as it always did. What the agent is told of the earlier turns of a
thread says none of it either.

The heading of each turn an answer-only ask brought back, its reply or a
comment it opened, also says how old the stack it was asked on is, while
the thread is open and the stack has changed since: `agent(max), on the
stack 2 edits ago:` (`on the stack 1 edit ago` for one). The edits are
counted along the tree of edits (The tree of edits, below), from the state
the answer was asked on to the state the stack is on now. Once the stack
is on a state that was not edited from that one (that edit undone, say,
and another made in its place), the heading says `agent(max), on another
version of the stack:`. On the stack it was asked on, it says nothing
more; nor does a closed thread, a turn of any other ask, a change's, a
build and test's or a try push's, nor one written before. Only the heading
in the thread says it, in the middle and in a review's right pane: the
comments pane, the search, the history, `$EDITOR` and `stacked comments`
say who wrote each turn as ever. So an answer kept as a change of the stack
is accepted (below) shows that it answered the stack as it was, for you to
ask it again on the stack as it is if need be. The stack an answer was
asked on goes with its turns, on their fence (One file, only appended to,
below).

On the command line, `stacked send` takes `--effort LEVEL`. Without it, an
ask goes at `high`, and
`--batch` sends each comment at the effort it was batched at, an ask of
several at the highest of theirs. `stacked review --send-back` takes
`--effort` too; without it, the work goes back at the effort it was asked
at. A level claude does not take is refused, with the levels it takes
said, and nothing is sent.

What each kind of ask may use is stacked's to give and yours to add to.
stacked always gives each what it needs: a change, git's history commands,
the commands that read, and its own `stacked fits` and `stacked export`,
none of which reaches the network, so that a change cannot touch a remote
(below), and the checkout's linter, when it has one, to run on the files
it changes (Linting, below); an answer, and a try push's selection,
`Read`, `Grep`, `Glob`, `LS`, the commands that only read and git's that
only read (`answer only`, above), and stacked's commands that read the
stack; a build and test, any command. What else
each may use is a list of
the settings' (Settings, below): `agent.change_tools` for the asks that
change the stack, nothing unless set; `agent.answer_tools` for answer-only
asks, `curl` and `jq` unless set, as answers could always run them before
there were lists; with the `try-push` feature, `agent.try_tools` for the
asks that select a try push's tasks, `curl` and `jq` unless set; then
what the reviews of the agent's work allowed from now on for that kind of
ask, a list of its own (`agent.change_accepted`, `agent.answer_accepted`,
`agent.test_accepted`, `agent.try_accepted`: The tools the agent could not
use, below); and `agent.tools` for every ask, builds and tests too.
`stacked setup agent` sets those lists up with you, as one list of tools
with a box for each kind of ask. Every ask is refused, too, what the stack's
checkout's claude settings of yours deny (`permissions.deny` of
`.claude/settings.local.json`), wherever it works: claude refuses it in the
checkout, and stacked has it refused in the agents' worktrees too, which do
not have that file, as git does not track it (`--disallowedTools`). What
the tracked `.claude/settings.json` denies is in the worktrees already.
Every ask but a build and test, which runs what the project needs, is
refused what would make the commands it may run run another program or
delete files (`git rebase --exec`, `find -exec`, `find -delete`, `rg
--pre`, `rg --hostname-bin`, `sort --compress-program`, however shortened,
`--co`), whatever its lists let it run. An answer and a try push's
selection are refused, too, the options that make what they are given
write a file or run a program (`answer only`, above).

No ask waits for a setup: the first goes at once, with stacked's defaults
(`curl` and `jq` for answers and for a try push's selection, nothing more
for changes), and what an ask could not use lands for your review with its
work. `stacked setup agent` sets the lists up with you whenever you want
it to. A settings' file of a stacked of before that says the setup was
done (`agent.setup`) is read all the same, that setting passed over:
`stacked config --unset agent.setup` takes it out. The tour's stand-in for
the agent needs no list.

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

An ask goes now, or later, as in self-review's batch: the dialog's Ask
Now, or its Ask Later, each time, as nothing of it is kept. Asked later, a
comment waits in the stack's batch, at the permission and the effort its
dialog set, marked `◇` and said `batched (may change the code)`; nothing
is started. Its menu has `Send it now (…)`, alone, as it was batched, and
`Remove it from the batch`; written again, it stays in the batch, to go as
it is then. The banner sends the batch (below), as you pick: its edits to a
single agent, in one ask, of the whole stack when they are on more than one
patch or one was batched so; its answers to a single agent too, in one ask
with the whole stack in view, or fanned out, an agent each, all at once;
each build and test on its own, and each try push on its own too (Try push,
below). Each of these items ends with the effort it sends at, the highest
its comments were batched at unless another is picked. On the command
line, `stacked send NAME ID --later` batches (how, as `send` says it:
`--answer-only` and the rest), and `stacked send NAME --batch` sends the
batch, the answers an agent each with `--fan-out`, and the try pushes the
editor batched, each on its own.

Every ask works in a worktree of its own, never in your checkout. A
worktree is a checkout of every file, which costs time and space: so the
stack has worktrees of its own, `.stacked/NAME/agent` for changes,
`.stacked/NAME/answer-only-agent` for answers and for the selection of a
try push's tasks, and `.stacked/NAME/build` for builds and tests, beside
yours (`.stacked/NAME/user`), out of git's sight. An ask takes the first
that no running ask is in and that holds nothing uncommitted, moves it to
what it is asked about, and leaves it there when it lands, for the next
ask to take: only what differs is written. One busy, the next ask takes
`agent-2`, `agent-3`. One an agent left work unfinished in is left as it
is, for you to look at; the ask says so when it fails (`it is left in
…/.stacked/NAME/agent`), and the next takes another. Looking at a
checkout, to tell whether it holds anything uncommitted, stacked's git
takes no lock it can do without (`GIT_OPTIONAL_LOCKS=0`): an ask that
starts in a worktree as another ask's start looks at it, or a git command
of yours in your checkout as the editor looks at HEAD, never finds the
index locked by that look. A setting of
`[agent]` (the stack's checkout's own, if it sets it) says when each is
made: `worktree` for a change, and `answer_only_worktree` for an answer,
each `ondemand` or `precreate`:

- `ondemand`, unless set: the stack's own worktree, made the first time an
  ask needs it, and kept until you remove it (`git worktree remove
  .stacked/NAME/agent`).
- `precreate`: the same, made when the editor opens the stack, and each
  stack it switches to, away from the screen, so that no ask waits for it;
  removed when the editor exits, away from it too, but those an agent is
  working in or left anything uncommitted in.

A stacked of before also took `pwd`, your checkout itself. No ask works
there now: a settings' file that still says `pwd` is read as `ondemand`,
and stacked says so where it says what is wrong with the settings
(`agent.answer_only_worktree = "pwd" is no longer supported: asks run in a
worktree of their own; read as "ondemand"`); `stacked config` refuses to
write it. An ask that a stacked of before started in your checkout still
lands, or is found dead, as any other: your checkout is left as the agent
left it, never removed nor cleaned.

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
on, and goes on its own when that lands; `Take it back from the agent's
queue…`, in its menu, and Delete on it, take its ask out of the queue once
asked, the comment staying open (What the agent brings back waits for your
review, below). Nothing is asked on a stack that
does not apply. A lint job of the stack takes its turn as a change does,
the changes asked after it waiting for it (Linting, below); so does a
reconcile with HEAD (The editor, above), the changes asked after it
waiting until it is reviewed.

Chained (`agent.chain`, on unless set to false), a change
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
the changes, on the stack as it is. Accepting a change leaves behind the
answers made on the stack before it, still running or waiting for your
review: the review of the change lists them, and you mark each keep,
restart or stop before that review can close with the change accepted
(What the agent brings back waits for your review, below). Kept, an
answer goes on, or waits for your review, as it is, on the stack it was
asked on, and the heading of its reply says how many edits old that stack
is (below). Restarted, it is asked again on the stack the change makes.
Stopped, its agent is stopped, or what it brought back is thrown away
unseen, and its comment stays open, not asked again. Asked again, an ask
whose work had landed is shown that work, to reuse what still holds: its
replies, the comments it opened, and, for a change, what it changed. A
build and test is not asked again: it goes on, of the stack as it was,
unless you stop it as you close the review. Closing a review, or sending
the work back, says first what becomes of the asks it leaves behind, and
offers to stop a build and test going. Unchained, a change waits for the
answers running, and accepting it leaves those waiting for your review as
they are: its review lists none.

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
nothing in it has changed for a day; so is the folder a try push's
selection downloads the task graph into (Try push, below), should its ask
never have landed. `$STACKED_TODO` names another folder to keep the plans
in, in place of `$TMPDIR/stacked-todo`.

What a change may run reaches no network, unless what its list
(`agent.change_tools`) adds does, or the checkout's linter does: stacked
ticks nothing in that list's column of the setup, and the setup says
which of the agent's tools can reach the network (Bash at large,
`WebFetch`, `WebSearch`), as a change that reaches it could touch a
remote. It may run git's history
commands (`status`, `log`, `show`, `diff`, `add`, `restore`, `reset`,
`commit`, `cherry-pick`, `apply`, `checkout`, `switch`), `git rebase` with
its plan (`-i`, and `--continue`, `--skip`, `--abort`), git's settings to
read them (`git config --get`, `--list`), commands that read (`grep`,
`rg`, `find`, `cat`, `sed`, `sort` and the like), and the checkout's
linter, when it has one, on the files it changes (Linting, below). It may
not set git's
settings (`git -c`, `git config` otherwise), as one can run a program, and
stacked has claude refuse what would make the rest run another program
(`--disallowedTools`: `git rebase --exec`, `find -exec`, `rg --pre`, `rg
--hostname-bin`, `sort --compress-program`, however shortened), as
claude's own refusals win over what it allows. The
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
and `Move it back to patch 2` are in its menu, the second grayed while the
stack does not hold that patch. Comments on lines follow a
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
gives them), agent replied (changed, answered, tested), each with how it
was asked and at what effort (`with the agent (answer only, at max)`,
`agent replied (change, at max)`), failed (and why), moved. Until the
agent writes its first line, one line says what is being run to start
it, as it runs: `starting: git worktree add --detach … BASE` (the
stack's own worktree, made the first time, which takes a while in a big
repository) or `starting: git switch --detach BASE, in WORKTREE` (taken
again), then `starting: git am --keep-non-patch, 3 patches` (the stack, up
to what it is asked about); then `starting: claude -p … --permission-mode
… --effort LEVEL, in WORKTREE`, the ask's own worktree, whatever its kind:
never your checkout. They stay
at the top of its stream, as `$ git worktree add …`, in the stream, in
`Runs…` and in `stacked transcript RUN`. Enter on a comment the agent is
working on opens its stream in place; esc folds it, and so does the agent
being done, the bottom line saying so. The comments pane marks each (`●` with the agent, `◇`
batched, `◌` ready, `◆` its reply waits for your review, `✓` replied, `✗`
failed, `↑` moved), its header counts them (`4 comments · 1 with the agent ·
1 to review · 2 to read`), and the top bar too (`agent ●2 · batch:3 ·
ready:1`).
`Filter comments…`, on either header and on a patch's menu, picks what
both panes show, each with its count: all, to send, to review, to read
(any, from an answer, from a change), in progress, failed or moved, gone
(on a line the patch no longer has), closed, none. The counts are of the
comments the middle shows, which the comments pane lists, the patch's or
the stack's own, as the menu's title says, from either header as from a
patch's menu, that patch's (`filter comments, counted on patch 3`,
`filter comments, counted on the stack's own comments`). What is picked
is kept per stack, in the file, and filters every patch's comments and the
stack's own alike. Under `closed`, the comments pane lists those closed, as
the middle shows them, its header counting them (`1 closed comment`); it
is there for them though no comment is open, and → reaches it. A comment you are done
with is closed, from its menu (`Close this comment…`): it leaves the
comments shown. When the cursor is on the comment closed, the cursor goes
to the line the comment was on (a line of the code or of the message, the
file's name, or the patch's title), on the same line of the screen; a
comment on the stack, or on a line the patch no longer has, leaves it on
the row over the comment. A comment that `Filter comments…` hides does
the same. When
a comment is closed from elsewhere (the comments pane, the banner, another
stacked), the cursor stays where it is, as it does whenever other comments
come, go or change: hidden, moved to the stack, answered, written on, or
opened again. The comments pane then picks the next comment, or the one
before it when the comment closed was the last; with none left to list, its
cursor is on its header, which says why. A comment closed is not
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
as patches.

The agent runs in a process group of its own. Quitting the editor leaves it
running, and what it does lands on its own; leaving (ctrl-c, or `Quit`)
lists the asks still running, each as `stacked runs` says it and where its
agent works: its worktree. A worktree with work the agent did not commit, or
that could not be filed, is left in place, and the run's summary says
where; others are kept for the next ask. `Runs…` lists every ask and pull,
each ask with the effort it was asked at (`answer(max) s "why so loud?"`),
as `stacked runs` does, and shows what the agent wrote.

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
test's findings on each patch's title). A try push's ask holds a fourth,
the tasks it selected (Try push, below). An ask that brought none of them
back waits for your review all the same while a comment it was asked
about is still open: its review has no part, only those comments, to
close or keep. If every comment it was asked about was closed or deleted
meanwhile, it is not held, and its run says that the agent brought
nothing back. You accept or reject each part, but those of a pull from
Phabricator, which are all accepted (Phabricator, below). A
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
nothing back waits until you close it. Work that holds a tool the agent
could not use waits for your review too, though the agent failed or
brought nothing back (The tools the agent could not use, below).

In the editor, a banner under the top bar says what waits on you on the
stack, in a line: what waits for your review (`⟳ 2 landed asks wait for
your review · the next: a change of patch 2, 1 reply, asked at max: "…"`,
the effort the agent was asked at before what it was asked; edits by
hand, lints and reconciles with HEAD counted apart), a reconcile with HEAD
waiting its turn (`Reconcile with HEAD waits its turn`), a pull
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
not in (`picked`), not reversed. First, `read only, to review`: `⟳ Review …` for what landed and changes nothing (an answer, an
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
agent, and at what effort, `with the agent (answer only, at max)`, the last
line it wrote). ↑↓ pick one, and Enter reviews what
landed, or goes to the comment; on the batch, it opens the banner's menu,
all of it, which Space opens from any line: what landed, to review; the
batch, to
send (`Send 2 edits to a single agent`, `Send 1 answer only to  [x] a single
agent  [ ] fan out`, `Send 1 build and test, each on its own`); every
comment the agent can be asked about, in the order they were written, a
box checked for those in the batch, which Enter checks (the comment goes in
the batch as it was last asked, else as the stack's asks are) or unchecks
(it is taken out); then the comments, to go to. ↓ past the last line goes
to the stack's header, and Esc goes back where the cursor was, the banner
closed again. A comment you are done with, closed, leaves it.

What waits its turn in the agent's queue is taken out of it from its line
of the banner, with Delete or the banner's menu, as from its own menus,
each asking first what it takes out and what stays; the status line then
says what was done, and the queue goes on with what waits after it. A
comment's ask waiting its turn (`◌`): `Take it back from the agent's
queue…`, in the comment's menu, and Delete on the comment and on its line,
asks `take the edit ask on patch 2's comment out of the queue? The comment
stays open` (or the answer-only ask, the build and test ask, the try push
ask), saying under it how many comments asked together with it still wait
their turn; the banner's menu has `Take back the ask on patch 2 · a.txt:
TWO · "…"…` after `Go to` the comment. Taken back, the
comment stays open, as it is, unsent, and the cursor on the banner
follows its line. One of a round you sent back from a review no longer
goes back to the agent with the work: taken back, the last of the round,
nothing goes back to the agent, and the work stays as you sent it back,
its review closed and its change not applied; edits by hand no longer
wait for it. One of a follow-up you asked from a review is no longer
asked about in it: taken back, the last of it, nothing is asked, and the
work waits for your review as it did. The stack's lint job waiting its
turn: `Drop the waiting lint job…`, in the banner's menu after `Read what the linter
writes`, which Enter on its line does, and in the stack's menu, asks
`drop the lint job that waits its turn? Nothing was linted yet`. The
stack's reconcile with HEAD waiting its turn: Enter on its line opens its
menu, which says what it waits for, with `Drop the waiting reconcile with
HEAD…`, which the banner's menu, HEAD's, the patch marked's and the runs
have too; it asks `drop the reconcile with HEAD that waits its turn?
Nothing was changed yet`. Dropped, either is as if it was never asked:
nothing is linted or taken from HEAD, nothing changes in the stack, the
marks stay, and the edits asked after it no longer wait for it (the
question says how many), so that they go in their turn. What runs keeps
its `Stop…` items.

A review lists the parts on the left, in three sections, under the
comments the agent was asked about, for its work, on top (`asking
comments`): each is `✓ keep`, its line in green, as it is when the review
opens, or `✗ close`, its line in red, done with: Enter turns one to the
other, as Space's `Close it with the
review` and `Keep it open` do, and the review closes those to close as it
closes, once their replies are in their threads (`Filter comments…`,
closed, lists them, to reopen). Marked to close, a comment's replies,
which go with it, are accepted; kept again, they are as they were. With no part, as when
the agent brought nothing back, the list has only those comments, and
`Close review` closes or keeps them as marked. The sections are the stack
operations of the change (a line for each patch: `noop`, `add`, `modify`,
`reword` when its message alone changed, `move`, `remove`, and the base
when it is rebased), the comments it opened, and its replies. Each part is
marked as it stands: `✓` and green once accepted, `✗` and red once
rejected, `·` while it is to review, green and red being the colours of
the lines a patch adds and removes (`added` and `removed` in the
settings). The title of the stack operations says how the change is
marked, `stack operations · accepted` with the word in green, `rejected`
in red, or `to review` as the rest of the title. Each of its lines starts
with that mark, and the operation after it keeps the colour of its kind:
`add` green, `modify`, `reword` and the base magenta, `remove` red, `noop`
dim, and `move` as any text. Under the cursor, a marked line is in the
cursor's colours, its mark too, as every line of the list is. On the right
is the part the cursor is on, as it would be:
the stack diff of the change, from the stack as it is, each new version's
interdiff under it, its message compared word by word, where its lines
wrap aside (History, below), so that a paragraph the work only
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
(`replies: ✓ 1 accepted, · 1 to review`). Each count is marked as the
list marks a part: `✓` and green for what is accepted, allowed or kept,
`✗` and red for what is rejected, refused or to close, and `·` for what is
to review or noted. Everything else on those lines, the labels, the
commas, and `·` with its count, is dim, not in the yellow of what a menu
says under its items, which is hard to tell from the green. The other
lines there, such as `You wrote 1 comment in this review…`, stay yellow,
as what every other menu says does. Esc then asks what to do: `Save` keeps
the marks in the file, for the review to go on from them when you open it
again; `Discard` throws away the marks made since the review was opened;
`Close review`, once every part is marked, applies what is accepted;
`Accept all remaining and Close` and `Reject all remaining and Close`, in
a section of their own, mark what is left and close once you say so (the
question says what is left, the menu on `Keep reviewing`), while
something is left: once every part is marked, they are grayed. Over them,
`Asking comment  [x] keep  [ ] close` picks whether the comments the agent
was asked about are kept open or closed as the review closes, every one,
picked as the review has them: ← picks `keep`, → picks `close`, and Enter
picks the other, the menu staying. Under it, while HEAD of your checkout
can move safely to the
stack as the review leaves it, `HEAD  [x] sync  [ ] no sync` picks whether
HEAD moves there once the change is accepted: detached at the stack's
top, the branch it was on as it was, as `Move HEAD…` moves it, the status
line saying where it went. HEAD can move safely when it was in sync with
the stack (on its top, each patch a commit with the patch's message, its
`Differential Revision:` trailer too, no commit above), and nothing is
uncommitted in the checkout. `sync` is
picked unless you picked otherwise in this review, and the pick is not
kept for the stack. A reply that goes with a change of the stack
opens that review, on the reply, from its comment (Enter, or `Review the
agent's work…` in its menu).

The review of a change of the stack, chained (Asking the agent, above),
lists in a section of its own, `answers made on the stack before the
change`, the answers that accepting it leaves behind: the answer-only
asks, and a try push's selections, running or landed and waiting for your
review, as the review finds them when it opens. An answer asked once the
review is open is none of them: it goes on, on the stack as it is. Each
is a line: its mark, then `running`, or `landed` while it waits for your
review, then what it was asked about, where its comment is and how that
comment starts (`·         running · a.txt: LINE 39 · why so loud?`). There
is no default: `·` says it is not marked yet, and each is marked, for when
the change is accepted:

- `✓ keep`, in green: the answer goes on as it is, on the stack it was
  asked on, and lands for your review as any answer does; landed, it
  waits for your review as it is. Its reply's heading then says how many
  edits old the stack it answered is (Asking the agent, above).
- `↻ restart`, with no colour of its own: it is asked again on the stack
  the change makes, its agent stopped first if it still works, and shown
  what it brought back if it landed, to reuse what still holds.
- `✗ stop`, in red: its agent is stopped, as `Stop the agent…` stops it,
  its ask landed as failed and said so on its comment; landed, what it
  brought back is thrown away unseen, as a rejected answer's is. Either
  way, its comment stays open, and is not asked again.

Enter keeps the answer the cursor is on, Delete stops it, and Space has
its menu: the three marks as boxes, the one it has checked, each saying
what it does (`Kept: the answer goes on, and lands for your review on the
stack it was asked on`), Enter on one picking it, the menu staying. On any
line, Space also has `Mark all answers kept`, `Mark all answers restarted`
and `Mark all answers stopped`. On an answer, the right pane shows the
comment it answers, its reply after it once it waits for your review, then
how it is marked and what that does. One that fails, or is reviewed, while
the review is open says so, dim, and needs no mark; one running that lands
keeps its mark, which then goes for the work it brought back. The marks
count only once the change is accepted: rejected or sent back, the change
leaves the stack as it is, and the answers go on as they are. Until each
answer is marked, the review does not close with its change accepted:
`Close review` and `Accept all remaining and Close` are grayed, and so is
`Reject all remaining and Close` while the change is marked accepted, Esc
saying what they wait for (`Not marked yet: the stack · and the stack?
(waiting for your review)`) under how they are marked (`answers made on
the stack before the change: ✓ 1 kept, · 1 to mark`). `Accept all
remaining and Close` marks none of them. Closing with the change accepted
asks first, saying what becomes of each (`Closing this review applies its
change. Of the answers made on the stack before it, 1 running goes on, and
lands for your review on the stack it was asked on; 1 waiting for your
review is thrown away unseen, its comment left open, not asked again.`),
in place of what closing asked again before; and the status line, then
the run, says what came of them (`1 answer-only ask kept`, `… asked
again`, `… stopped`). The review of an edit by hand, of a reconcile with
HEAD and of a lint job lists them the same. A build and test is none of
them: it goes on, of the stack as it was, and closing offers to stop it,
as ever.

The review of a pull from Phabricator rejects nothing: each part shows as
accepted (`✓`), Enter, Delete and Space mark none of them (Delete says
`a comment pulled from Phabricator cannot be rejected: close the review to
file it`), and Esc offers `Close review`, which files every comment it
pulled, with `Save` and `Discard` to leave it waiting for you; there is no
`Accept all remaining and Close` nor `Reject all remaining and Close`.

What you write in the review of the agent's work goes back to the agent
with that work. In the patch as the work leaves it, ↑↓ move from line to line,
and Enter writes a comment on the line in the inline editor, as in the
main view: on a comment, what you said last written again if the last word
is yours, a reply if not, and your answer to the agent's reply if this work
replied to it. The same holds on a patch the work adds, or splits a patch
into, which the stack holds only once the work is accepted, and in the
thread of a comment the ask was about that someone else opened, a
reviewer or the agent: what you write there is yours, as a comment you
open is. Where Enter writes a new comment, on a line or on a file's
name, Space has `Comment on this line` (`Comment on this file`), which
writes the same comment, here or in $EDITOR as the item's choice picks, as
in the main view. On a reply in the list, Space has `Reply to it`, your answer
to follow the reply in its thread (`Edit your answer to it` once written).
Nothing written there asks the agent at once, and the list shows it under
`what you wrote, to send back`: on one of its lines, the right pane shows
it where it would be, Enter writes it again, and Space has `Take it back`
(`Take your answer back`), for it not to go back with the work: a comment
you opened there is closed, and a comment the ask was about is as it was
before you wrote in it in the review. With any
of it, the work is neither accepted nor rejected:
Esc has `Send it back to the agent…` in place of closing the review. It
opens the ask dialog (Asking the agent, above) on the work it sends back,
its title saying so, what the work holds and what was asked (`send back
the agent's work (max): a change of patch 1, 1 reply · asked "why so
loud?"`). The dialog has the same two lines, unset as it opens, and the
same keys, list and clicks; its buttons are Send Now and Cancel. Send Now
is grayed until both lines are set, and while the agent cannot be
reached, the dialog saying why as it says it of Ask Now (`Send Now is
grayed: claude is not found on the PATH.`). Cancel, Esc and a click out of
the dialog send nothing, and the review is as it was. The permissions
offered are those that apply to every comment that goes back, as each
one's own dialog offers them: to answer, always; to change the patch,
when they are all on one patch; to change the stack, unless one is a
question on the history; and, when they are all on one place, the stack
or a patch as a whole, to build and test, and for a try push. The line
under the permission says what the one set does with the work, and how
many comments go back (`1 comment goes back to the agent.`).

A permission that changes the stack, `change patch` or `change stack`,
sends the work back. The work is rejected, its change not applied; its
replies go in their threads, your answers after them, and the comments it
opened go as you marked them. Then the comments it was asked about whose
last word is yours, and what you wrote in its review, are asked again, in
a round that starts from the stack this work makes, asked as the dialog
set it, which may be otherwise than the work was asked: a patch's work
sent back to change the stack is asked of the whole stack. The agent's
worktree holds the change the work made, and the agent is told it is
another round. As a round asked again after its work was dropped is told,
it is told what it changed in the round sent back (a stack diff from the
stack that round went on from) and the comments it opened then, and
`stacked export --work` prints the stack it made for it; its replies are
in their threads already. Its session is not the one before: claude
starts afresh each round. The round goes at the effort the dialog set.
That round's work lands for review in place of the one sent back. A
comment it is asked about on a patch the work sent back added stays
there, open; should the stack never take that patch, the round's work
rejected, or accepted without it, the comment moves to the stack's own
comments as that review closes, open still, `moved from a patch · c.txt:
cee` on its first line (`Move it back` grayed while the stack does not
hold that patch), and the review says so. The round goes before anything
else waiting for the agent, and the stack is not edited by hand until you
have reviewed its work: nothing changes the stack it goes on from.

A permission that only reads, `answer only`, `build and test` or `try
push`, asks a follow-up, and the work is not rejected: it stays waiting
for your review, its review open and its marks as they are. The follow-up
is asked about what you wrote in the review, and the comments the work
was asked about whose last word is yours, each thread as it would be with
the work's replies and your answers in it: those are not in the threads
before the review closes. It is asked on the stack the work makes,
which `stacked export --work` prints: an answer's worktree holds that
stack, and so does the stack's build worktree for a build and test. It is
told that the work waits for your review, what the work changed and the
comments it opened. What goes back with the comment that asked first
(your answers to why a try push's tasks were selected, and what the
review allows of the tools the agent could not use) goes as your answer
after that comment's reply, where the work going back would put it in the
thread. What the follow-up brings back comes into the same review once it
lands, and nothing of it waits for a review of its own:

- An answer's replies are replies of the review, in its list after the
  work's, each in its thread after your answer, for you to read and mark
  as any reply; the comments it opens are among those the work opened.
- A build and test's report is a section of its own, `what the build and
  test found, to read`: its reply to each comment, by where the comment
  is, and what it found of each patch, by the patch's title, each on the
  right as it would be, with nothing to accept or reject. Its replies go
  in their threads as the review closes, after the reply before them
  there, unless the work's change, or that reply, is rejected; what it
  found of each patch is not filed. Space on a reply of it has `Reply to
  it`, as on any reply.
- A try push's tasks are tasks of the review, to accept or reject as a
  try push's tasks are, but those the work holds already, which keep their
  marks; its replies are replies of the review. Closing the review with
  tasks accepted pushes them as the work's own try push would; the work
  being no try push, as the follow-up would, from its comment.

Your answer to a reply that a reply of the follow-up follows went with the
follow-up: it is no longer under `what you wrote, to send back`, and Space
grays `Edit your answer to it` there. Then you go on reviewing the work:
accept it, reject it, or send it back again. A follow-up that fails, or
brings nothing back for a comment, leaves your last word there, to send
back again or to take back.

While the follow-up is with the agent, or waits its turn, the review says
so, on top, in a section of its own (`the follow-up you asked, with the
agent`), its line marked as the comments pane marks a comment with the
agent (`●`) or waiting its turn (`◌`), and the right pane lists the
comments it asks about and the last lines the agent wrote. What went with
it is not under `what you wrote, to send back`, nor written again or taken
back meanwhile, and `Send it back to the agent…` is grayed until it lands,
the menu saying why. Closing the review then asks first, saying that the
follow-up is stopped. Closed, the follow-up's agent is stopped as `Stop
the agent…` stops it, its ask landed as failed, its comments saying `ask
failed: you stopped the agent when you closed the review of the work it
follows up`, and what it would bring back thrown away; the status line
says `1 follow-up stopped`. The follow-up may use what this work was
allowed for itself, as every round of it may, and what the review allows
from now on is saved as the follow-up is asked, as when the work goes
back.

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
brought back`), the effort it was asked at, if it was asked at one (`asked
at max`), and each part of it; then what a build and test asked as a
follow-up found, a line each (`found`), and each follow-up going, how it
was asked and where it stands (`following`). `stacked review NAME [RUN]
--accept-all` (or `--reject-all`) closes the review of that ask, the first
landed if no run is given: the parts not marked yet are accepted (or
rejected), and what is accepted is applied; `--close-asking` closes the
comments the agent was asked about too. A pull's review is not rejected:
`--reject-all` refuses it, and `--accept-all` files every comment it
pulled. Accepting a change, chained, `--accept-all` goes by the marks its
review saved for the answers made on the stack before the change, and
marks none of them: while one is not marked keep, restart or stop, it
refuses, naming each by its run, where it stands and what it was asked
about, and saying to mark each in the review of that work in the editor,
then to close that review there or save it. So do `stacked send NAME ID
--wait --accept`, and `stacked edit … --accept`, whose edit then waits for
your review. `--reject-all` and `--send-back` leave those answers as they
are. In the review of landed work RUN,
`stacked comment NAME --patch N --line … --work RUN TEXT` writes a comment
on the patch as that work leaves it, and `stacked reply NAME ID --answer
TEXT` answers the agent's reply to comment ID; then `stacked review NAME
--send-back` sends the work back to the agent with them, at the effort it
was asked at unless `--effort LEVEL` says otherwise (`--accept-all`
refuses while you wrote in it). It goes back as a round of the work's own
kind, the work rejected, whatever its kind: the editor's dialog is where
another permission is picked, and a follow-up asked. A follow-up asked
from the editor that is still going is stopped as the review closes from
the command line, and as the work goes back, what went with it going back
with the work. `stacked send NAME ID --wait`
says what waits for your review once it lands; `--wait --accept` accepts
all of it as soon as it lands, for a script.

## The tools the agent could not use

claude refuses a tool the agent is not let use before it runs, and its
result lists each call it refused. Each refusal is an item of the review
of the work, by the rule that would have let the call run: one item a
rule, with the calls that needed it. Five calls of `searchfox-cli` are one
item, `Bash(searchfox-cli *)`, which lists the calls. A plain program's
rule is `Bash(PROGRAM *)`. A broad program's is narrower: for `./mach` and
`git`, what it is asked to do (a refused `./mach try fuzzy --no-push -q …`
is `Bash(./mach try fuzzy *)`, `git log --oneline` is `Bash(git log *)`);
for `sh`, `bash` and `python`, the script it runs (`Bash(bash
./tools/run.sh *)`, `Bash(python3 -m pytest *)`); for `npx`, the package
it runs, with its scope or version (`npx eslint --fix` is `Bash(npx eslint
*)`, `npx @scope/pkg@2 x` is `Bash(npx @scope/pkg@2 *)`), or, after `-p
PKG`, the command it runs (`Bash(npx -p typescript tsc *)`); for `cargo`,
its subcommand (`cargo clippy --fix` is `Bash(cargo clippy *)`). The
options that npx and cargo are given before what they run stay in the
narrow rule as written, so that the rule lets the call: `npx -y eslint .`
is `Bash(npx -y eslint *)`, and `cargo +nightly --locked clippy` is
`Bash(cargo +nightly --locked clippy *)`. The options read this way are
npx's `-y`, `--yes`, `-q`, `--quiet` and `-p` (`--package`), and cargo's
toolchain (`+nightly`) and global options: `-q`, `--quiet`, `-v`, `-vv`,
`--verbose`, `--locked`, `--offline`, `--frozen`, `--color`, `--config`,
`-C` and `-Z`. After any other option, which could take the word after it
as its value, the rule is the wide one alone (`Bash(npx *)`, `Bash(cargo
*)`), as it is for `npx` or `cargo` with nothing after it. Each command of
a pipe is matched on its own, as claude matches it: of `curl … | jq .`,
only the command that was not let run is an item. An MCP server's tool is
its rule, `mcp__SERVER__TOOL`; another tool is its name (`WebFetch`).
Only the calls claude refused are items, never what the agent says it
would have liked. Work that holds an item lands for your review though
the agent failed or brought nothing back, so that a tool can be allowed
and the work sent back.

Every ask is told not to stop at a refusal: to work around it with what it
may use, and to finish what it was asked as well as it can; and to say,
for each tool it was refused, why it needed it and how it worked around
it (`"refused"` in its answer, structured output or JSON). What it said is
its comment on the item. A refusal the agent did not explain is an item
all the same, its comment saying that the agent did not say why.

The review lists the items in a section of their own, `tools the agent
could not use`, a line each: `✗ Bash(searchfox-cli *) · refused`, `✓
Bash(./mach try *) · for this work`, `✓ WebFetch · from now on`, `·
Edit · noted`. On an item, the right pane shows the agent's comment on it,
then the calls the agent tried, then where the item stands and, when the
item is not offered, why. Each item is in one of three states:

- refused, the default: the agent is not let use the tool. Delete refuses
  an item.
- allowed for this work: every round of this work may use the tool, until
  its review closes. That is the rounds sent back from its review, the
  follow-ups asked from it, and the rounds stacked starts by itself for
  that work: an answer restarted as a chained edit is accepted, an edit
  asked again once the work it went on from is rejected. claude is given
  the rule with the other tools the ask may use, and the rule is never
  saved. The stream of each such round starts by saying so (`✓ allowed
  for this work: Bash(searchfox-cli *)`), and so does the status line once
  the work is sent back. The rule ends with the review, whether the work
  is accepted or rejected.
- allowed from now on: Enter allows an item from now on, as accepting it.
  The rule is saved once the work is sent back or followed up (first, so
  that the next round may use it) or once its review closes, whether the work is
  accepted or rejected, or stacked drops the work to ask it again (its run
  saying so). It is saved only in the stack's checkout's own
  settings, `.stacked/local.toml`, never in `~/.stacked/config.toml`, nor
  in any other file of the checkout (claude's settings, the repository's
  files). It goes in a list of its own for the kind of ask the work was:
  `agent.change_accepted`, `agent.answer_accepted`, `agent.test_accepted`
  for builds and tests, and, with the `try-push` feature,
  `agent.try_accepted` for a try push's selection. That list is added to
  the tools of that kind of ask alone, after its own list, so that it never
  hides the lists `stacked setup agent` writes, global or local.

Space has the item's menu: the three states as boxes, the one the item is
in checked, each naming the rule (`Refused: Bash(searchfox-cli *)`,
`Allowed for this work: Bash(searchfox-cli *)`, `Allowed from now on:
Bash(searchfox-cli *), saved in /src/firefox/.stacked/local.toml`); Enter
on one picks it, the menu staying. Under them, a broad program's other
rules, to allow the item with instead: `Widen the rule to Bash(./mach try
*)`, `Widen the rule to Bash(./mach *)`, then `Narrow the rule to …` back.
Then `Reply to it`: your answer to what the agent said of the tool, which
goes back to the agent with the work, as any answer written in a review
does. Where the checkout's own settings cannot be written, git tracking
`.stacked/local.toml` or the stack's checkout being no checkout any more,
`Allowed from now on` is grayed, the menu and the right pane saying why,
and Enter allows the item for this work; should a rule allowed from now on
fail to be saved as the work goes back, it is allowed for this work
instead, and the status line says so.

Noted items are said, and not offered: no state but refused, as no allow
would let the call run. They are what the checkout's claude settings deny
(`.claude/settings.local.json`, `.claude/settings.json`), a deny winning
over any allow; what stacked refuses every ask but builds and tests (`find
-exec`, `rg --pre`, `rg --hostname-bin`…), and what it refuses an
answer-only ask and a try push's selection (`git diff --output`, `sort
-o`, `git grep -O`, `file -C`); what changes things, to an answer-only ask
and to a try push's selection, which only read (`Edit`, `Write`, `rm`,
`git commit`, `sed -i`, a redirection to a file); to those two again, a
rule that is never offered them, whatever the call that needed it, so
that a refused `sed -n 1,5p f` is noted as `sed -i` is; a command the
shell runs itself (`cd` elsewhere); and a call whose rule the ask was
given already, which claude refused for a reason of its own. The rules
never offered those two are those of `sort`, `file` and `git grep`, whose
short options can be bundled, so that no rule refuses the one that
writes a file or runs a program (`sort -uo FILE`, `git grep -nOCMD`); and
the rules that can write files or run programs: those of `sed`, `awk`
(`gawk`, `mawk`, `nawk`), `uniq`, `tree`, `xargs`, `env`, `command`,
`perl`, `python`, `python3`, `ruby`, `node`, the shells (`sh`, `bash`,
`zsh`) and the programs that write files (`rm`, `mv`, `cp`, `tee`…), and
git's but for its commands that only read (`git show`, `log`, `diff`,
`ls-files`, `rev-parse`, `blame`, `cat-file`, `ls-tree`, `merge-base` and
`describe`), `Bash(git *)` among them, which a refused `git -C x log`
would need. No item offered to an answer-only ask or a try push's
selection widens to one of them either. An ask that changes the stack is
offered these rules, as before.

With a tool allowed, for this work or from now on, Esc has `Send it back
to the agent…` though you wrote nothing in the review: the agent tries
again with the tool, which is how a failed attempt goes on. The work goes
back as when you wrote in its review, from the ask dialog, and a turn of
yours in the thread of the comment that asked says what you allowed and
what you answered (`On the tools you could not use:`, then ``-
`Bash(searchfox-cli *)`: you may use it now, for this work.``); asked as a
follow-up that only reads, it is your answer after that comment's reply,
where the work going back would put it in the thread. Before it goes, the
dialog says, under its buttons, what this work may use from the next
round on, or from the follow-up on, and what is saved for good, and
where. Closing the review asks first when it saves a rule, saying what is
saved and where, and that what this work was allowed for itself ends
there.

On the command line, `stacked landed NAME` lists each item of a work on a
line of its own: `tool`, its rule, its state (`refused`, `allowed for this
work`, `allowed from now on` or `noted`) and the first call it tried, a
tab between each. `stacked review NAME --send-back`, `--accept-all` and
`--reject-all` go by the states saved in the review. The editor marks the
items, in the terminal, in the browser and in StackED alike.

## Try push

What follows is built by default, with the `try-push` feature:
`--no-default-features --features app,demo,moz-phab,web` builds stacked
without it (Install). A comment on the stack, or on a patch as a whole
(its title), asks which of Firefox's CI tasks to run on a try push: `Run
the tests a.txt touches, on Linux and Windows`, or `as many as you see
fit`. A comment on a line or a file is not asked as a try push, as it is
not sent to build and test: its `Ask agent…` dialog has no `try push`. One that an
older stacked batched or queued as a try push is refused as it would go,
from the banner's batch, from `stacked send NAME --batch` or from the
queue, saying why: `A try push is asked of a patch as a whole or of the
stack, not of a line or a file: comment on the patch's title or on the
stack.` It stays a comment, its ask failed with that reason, and the rest
of the batch goes. Asked as a try push (`Ask agent…`, its permission set
to `try push`, then Ask Now; or Ask Later, in the batch, whose `Send 2 try
pushes, each on its own` sends each
in an ask of its own), the agent
selects the tasks the comment asks for, as few or as many as it says, each
by its exact label in the task graph (`test-linux2404-64/opt-xpcshell-1`;
a chunked test is a task for each chunk, its label ending with the chunk's
number, and no label is a pattern), with why it is relevant to what the
patches change and to what the comment asks; and, when the comment asks
for repeated runs (an intermittent failure), how many times each runs,
from 2 to 20, as mach's `--rebuild` takes it. It reads as an answer does,
where an answer works, with the tools of its own list (`agent.try_tools`,
Settings, below), and pushes nothing, nor runs `./mach`. When it may run
curl, it is told where the labels of the tasks of mozilla-central's latest
push are listed (its `target-tasks.json`), and autoland's; and where that
push's full task graph is (its `full-task-graph.json`), whose keys are the
labels of every task `./mach try fuzzy --all-tasks` can select, to check
each label it selects against it before it answers. The graph is large:
the agent downloads it once, into a folder of the ask's own that its
prompt names, the one place it may write, and reads that copy with jq, all
its labels in one go. The folder is beside the rebase plans' folders
(`$TMPDIR/stacked-todo/…`, Asking the agent, above), outside your checkout
and any worktree, so that neither shows anything new; stacked lets claude
write there (`--add-dir`), and removes it once the ask lands, however it
ends: done, failed, stopped, or found with its agent gone. Two asks at
once each have their own. If the download fails, the agent says so in its
answer, and answers without it. It gives the tasks with its replies, as
structured output, else in the JSON it ends with, whatever prose is around
it; a label given twice is taken once, and one with white space in it,
which could not select its task alone, is left out.

What it selects lands for your review, as any of its work does: the banner
says so (`the next: 6 tasks to push to try, 1 reply`), in its section of
read-only work. The review lists the tasks on the left, under `tasks to
push to try` (`· each runs 3 times`, when they run more than once), each
marked as a part is: `·` to review, `✓` accepted, `✗` rejected. Enter
accepts the task the cursor is on, or puts it back to review; Space has
`Mark as accepted` and `Mark as rejected`, `Mark all tasks accepted` and
`Mark all tasks rejected`; Delete rejects it. On the right is why the
agent selected it, as the agent's comment under the task's label. Space's
`Reply to it` answers that comment: your answer shows under it, and on the
left under `what you wrote, to send back`, where Enter writes it again and
Delete takes it back. With an answer of yours, the work is not accepted:
Esc has `Send it back to the agent…`, whose dialog sets how the comment
that asked is asked again, your answers after the agent's reply in its
thread, as a turn of yours (`On the tasks the agent selected:`, then each
task's label with what you said). Asked as a try push again, a follow-up
that only reads, the work stays waiting for your review, its marks as
they are: the follow-up is told the tasks the work selected, each as you
marked it, with why, and selects them again, and what it selects comes
into the same review, but the tasks the work holds already, which keep
their marks. Asked with a permission that changes the stack, the work is
rejected, and the round's work lands for review in its place.

Closing the review with tasks accepted is where they are pushed to try,
with the patches up to the comment's (`patch 1 alone`, `patches 1 to 3`,
or every patch of the stack, for a comment on the stack): the question
before closing says which, and that `./mach try fuzzy --no-push` checks
first that it selects them. Past the 1,000 tasks mach pushes without
asking (push.py's threshold, each task counted as many times as it runs),
it says so too: `These 1,240 tasks are more than the 1,000 mach pushes
without asking: they would run at the lowest priority, as mach does when
you say yes.` Closing with no task accepted pushes nothing, as a
rejection does: the question says so. The comment that asked stays open,
even marked to close with the review: the push's replies go there.

mach pushes every commit of HEAD that none of the remotes it takes as
Firefox's has (those fetching from github.com/mozilla-firefox/, and, in a
git-cinnabar clone, from hg.mozilla.org but its try; with none of those,
every remote), so the stack's base must be on one of them, for the push to
hold the stack's patches alone. A base that is not keeps the review open,
as the question and the status line say: `the stack's base, commit
1a2b3c4d5e6f, is not on origin, the remote mach takes as Firefox's: mach
would push 3 commits of the base's besides the stack's patches. Fetch that
remote (`git fetch`), or put the stack on a commit that it has.`

The push is made away from the screen, by a stacked of its own (`stacked
try-push RUN`, which the editor starts), from a worktree of the stack's
own, `.stacked/NAME/try` (`try-2`… while another push is made from it; under
`agent.worktrees` when that is set): the stack's patches as commits on its
base, up to the comment's. The commits mach would push are checked there
to be those patches alone. Then `./mach try fuzzy --no-push` runs, the
tasks in one query, each its own exact term, fzf's OR between them (`-q
'^a$ | ^b$'`, which fzf is given whole, so that it runs once), the labels
with their chunk numbers (`--show-chunk-numbers`), from every task of the graph
(`--all-tasks`), with `--rebuild N` when each runs more than once; what it
says it would push (its try_task_config.json) is compared with what you
accepted: the tasks, how many times each runs, and the priority. Anything
that differs is said, and nothing is pushed: `This task you accepted is
not in what mach would push, as the task graph it reads does not have it:
`test-linux2404-64/opt-not-a-task-1`.` Then `./mach try fuzzy` pushes, the
same way. Past the threshold, mach is given the lowest priority
beforehand, as it sets it itself once told yes: a preset of stacked's
(`--preset stacked-lowest`), in a presets file of its own that
`MACH_TRY_PRESET_PATHS` names, in `stacked-try/RUN` beside the database,
which is gone once the push is done with it. Your own presets are not
read then, and nothing is written in ~/.mozbuild by stacked.

mach reads nothing: its stdin is closed, and the push runs in a session of
its own, with no terminal, so that nothing mach runs (ssh, git) asks
anything on yours either. A question mach asks stops it there, before
anything is pushed (each of them comes before the push), and the reply
says so, the question quoted, with what to change: `mach asked a question that stacked cannot answer, "Do you have an
@mozilla.com address? [Y/n]", and stopped, as stacked gives it nothing to
read. Nothing was pushed.` That one comes with Pernosco asked for
(`MOZ_USE_PERNOSCO` set, or `--pernosco`) when `ssh -G hg.mozilla.org` names
no user of yours: set your hg.mozilla.org user in ~/.ssh/config, or unset
`MOZ_USE_PERNOSCO`. mach asks the other when it finds no fzf: install fzf,
or run `./mach try fuzzy` once in a terminal, where it asks to install it.

What mach writes is the comment's stream, as an agent's is, each command
stacked runs as `$ …`: under the comment, the last lines, and `Read what
mach writes for the try push` on its menu, all of them. The comment is
locked meanwhile, as one the agent works on (`try push: mach checks the
tasks`, `try push: mach waits for your login`, `try push: pushed`). Its
login (Auth0's device code) shows where you see it: the page to open and
the code to enter in the status line (`mach waits for you to log in to
push to try: open https://… and enter the code ABCD-EFGH`) and in the
stream, and the comment's menu has `Open the login page…`, which shows the
page's link, for Enter to open it in your browser. mach opens the page in
the browser itself too, unless its `try.nobrowser` setting is on; once you
are logged in, it caches its token in ~/.mozbuild, which stacked never
reads, and asks for no login until it expires.

Pushed, the push's link is a reply on the comment that asked, a web link
under it: `stacked pushed the 3 tasks you accepted to try, with patch 1
alone. Treeherder shows the push: @r1.` The comment stays locked while the
push is followed; its menu has `Open the try push on Treeherder…` and
`Stop following the try push…`, which cannot stop the push in CI: it
unlocks the comment, a reply saying `stacked stopped following the try
push: it goes on in CI, and Treeherder shows it: @r2.`, the link again.
Before mach says it pushed, the menu has `Stop the try push…`, which stops
mach (`nothing was pushed`, unless mach had started to send the push to
Lando, which it says). A push that fails says why in a reply, with mach's
last lines, and the comment unlocks: reply, and ask for the try push
again, for the agent to select the tasks again.

The push is followed while stacked runs, the editor looking at it every
few minutes, through what Lando and Treeherder show anyone, plain GETs of
their public pages, nothing of yours sent: Lando's job, at the address
`.lando.ini` gives its instance (`https://lando.moz.tools/landing_jobs/ID/`),
until Lando says it pushed it to try, and as which revision; then
Treeherder's push of that revision, then its jobs, page after page. All it
saw is kept in the database: the next editor picks it up from there, after
a restart too. Each change is a line of the comment's stream (`stacked:
Lando pushed it to try, as revision abcdef012345`, `stacked: 12 of 30
tasks done, 1 failed`), and the banner tracks it, in a section of its own,
`try pushes, followed` (`⇡ Try push of patch 1 · "Run the tests…": 12 of
30 tasks done, 1 failed`), and in its first line (`1 try push: 12 of 30
tasks done, 1 failed`); Enter there goes to the comment, Delete stops
following it. Its tasks are those you accepted, each as many times as it
runs, each by its last run (a retry's). It is done once every one of them
is, or once two looks in a row find nothing of the push pending or running
and nothing new: a task Treeherder never shows then, as it shows no task
that was never scheduled, is counted as not seen on Treeherder.

Done, its summary lands as the agent's answers do, for your review, in the
banner's read-only section, and the comment unlocks: `The try push is
done. Treeherder shows it: @r2. Of its 30 tasks, Treeherder shows 27 as
success and 2 as testfailed; 1 was not seen on Treeherder.`, then each task
that failed, with its link on Treeherder (`- `test-linux2404-64/opt-xpcshell-1`
(testfailed): @r3`), the tasks the push ran for them that failed (a build,
the decision task), and those not seen on Treeherder; each link a web link
under the reply. As an answer only's reply, it is taken at once from the
comment, Enter there putting it in the thread, or reviewed with the rest.
A push that Lando does not push to try (`FAILED`, `CANCELLED`, `ABORTED`)
lands a summary saying so, with what Lando says and its page there.

## Linting

`Lint the stack (./mach lint --fix)`, on the stack's header (Space there),
lints every patch of the stack with the linter of the stack's checkout,
away from the screen; what the linter fixes lands for your review, as an
edit by hand does. Nothing else lints: no patch's menu, no command, and
nothing is linted unless you choose it.

The linter is the checkout's own setting, `[lint] command` in its
`.stacked/local.toml`, which `stacked setup lint COMMAND…`, run in the
checkout, writes: a program and its arguments (`stacked setup lint eslint
--fix`), the files to lint after them. A lint job runs it itself, not the
agent, and with no shell: the command is split into words as a shell
splits them, quotes and all, and nothing else of a shell's is read, no
pipe and no variable. A checkout whose top has `./mach`, as Firefox's has,
and whose own settings set no linter, lints with `./mach lint --fix`,
nothing written. `stacked setup lint --off` writes `command = ""`: lint is
off there, `./mach` or not, until `stacked setup lint COMMAND…` sets a
linter again, or `stacked config --local --unset lint.command` takes the
setting out. With no command, `stacked setup lint` prints the linter in
use and says where it comes from, the checkout's own settings or the
default of `./mach`; with none, it says how to set one, and fails. The
setting is never written in `~/.stacked/config.toml`, nor read from it,
which says so if it sets one; `stacked config lint.command VALUE` refuses,
with `--local` too, and says that `stacked setup lint` writes it, while
`stacked config lint.command` reads it as any setting.

An ask that changes the stack may run the linter too, on the files it
changes, to check them: it is given the linter as the checkout's settings
have it when the ask starts, and no other kind of ask is (an answer and a
try push's selection only read; a build and test runs any command
already). It is given the rule the review of a refused call of the linter
would offer first (The tools the agent could not use, above), so that no
call of it is refused: `Bash(./mach lint *)` for `./mach lint --fix`, with
or without `--fix`, on any files; for a linter the checkout sets, its
program, with what it is asked to do for a broad program (`Bash(eslint *)`
for `eslint --fix`, `Bash(python3 -m black *)` for `python3 -m black`,
`Bash(npx eslint *)` for `npx eslint --fix`, `Bash(cargo clippy *)` for
`cargo clippy --fix`), so that the ask may not run another package with
npx, nor `cargo run` or `cargo install`. A linter that no narrow rule lets
run is not given: a broad program asked nothing narrower, whose rule would
let it run anything (`sh -c …`, `./mach --verbose lint`, `npx` or `cargo`
alone), or one whose program is in quotes. The ask's prompt names the
command: ``This checkout's linter is `./mach lint --fix`: you may run it
on the files you change, to check them.`` With no linter, or lint off,
nothing is given and nothing is said. `stacked setup agent` names the
linter, among what stacked always gives, in the change column (Settings,
below).

The item names the command in use; with none, it reads `Lint the stack`,
grayed, and the menu says why: `No linter is set for this checkout:
`stacked setup lint COMMAND` sets one.` It is grayed too, the menu saying
why, where lint is turned off, while the stack may not be edited by hand
(the agent changing it, a change of it waiting for your review), on a
read-only stack, and while a lint job of the stack waits or runs. So it is
in the browser and in StackED, as in the terminal.

The lint job runs in a process of its own (`stacked lint-job RUN`), in a
session of its own, at a low priority (`nice`), the linter's stdin closed,
so that nothing it runs has a terminal to ask anything on; and in a
worktree of the stack's own, `.stacked/NAME/lint` (under `agent.worktrees`
when that is set), kept for the next job, never in your checkout. It goes
patch by patch, from the bottom: the worktree holds the stack up to the
patch, with the fixes kept so far, and the linter is run as `COMMAND
FILE…` on the patch's files that are there. What it changes is folded into
the patch, as its new version, its message and author as they were, so
that each fix lands in the earliest patch it shows in; but only once each
patch above, as stored, is seen to still apply on it, in turn, in an index
of the check's own (the repository only read), and the new version leaves
its files as the linter did. At the first patch whose fixes would keep a
patch above from applying, none of its fixes is kept, and the patches from
it on are not linted one by one: with them as they are, the linter is run
once at the top of the stack, on their files, and what it changes goes in
a patch of its own at the end, `Lint fixes to fold into the stack`, by
you, its message saying which command made them, on which patches' files,
and why they were not folded in. A fix that adds or deletes a file, or
changes a file that is not text, a file's mode, or a file its patch
renames, is not folded in either: it stops the patches' run there the same
way, and goes in that patch at the end. That patch comes with a comment on
it, yours, ready to send but not sent: `Fold these lint fixes into the
patches they belong to, keeping every patch applying, then drop this
patch.` Asked of the whole stack, as usual, the agent folds them in.

What lands is one change of the stack, for your review: the new versions
of the patches that took fixes, and the patch at the end if there is one,
its comment one the work opened. The history calls it, as the banner and
the review do, `lint fixes in patches 1, 2 and 4, and a patch on top from
patch 5 on (./mach lint --fix)`. The linter's exit says whether it
reported something it did not fix: each run of it that exits otherwise
than with 0 is listed with the landed work, in the review under `what the
linter reported, to read`, a line for each (the patch it ran at, its files
and how it exited), what it wrote shown on the right; and under it in
`stacked landed NAME`. With nothing fixed and nothing reported, nothing
lands, and the status line says so. With nothing fixed and something
reported, that alone lands, read only, in the banner's section of what
changes nothing: its review is to read, then close. The rest is as for an
edit by hand: accepted, it goes in; rejected, nothing changes; edited by
hand from its review, it lands again with your edit.

A lint job takes its turn as a change of the agent's does, in the same
queue: once nothing changes the stack and no change of it waits for review;
and one at a time of every stack of the database, another waiting its turn.
While it waits or runs, the stack is not edited by hand, the menus saying
why (`the linter lints patch 2 of 5 (./mach lint --fix): …`), as while
the agent changes the stack. An edit asked of the
agent meanwhile waits for it (`ready (may change the code), it goes once
the linter's work on the stack lands`), then goes on from the stack the
linter made, without waiting for its review, as chained edits go on from
one another (Asking the agent, above): the linter's work rejected, it is
asked again. Answers go at once, as ever. The banner shows the job in a
section of its own, `the linter`, with where it is (`● The linter lints
patch 2 of 5 (./mach lint --fix)`, the last line it wrote after it; `◌ The
linter waits its turn to lint the stack`); Enter there shows what it
writes, all of which its run keeps (`Runs…`, `stacked transcript RUN`). A
job whose process is gone, killed, lands as failed once seen, as an ask
whose agent is gone does, and the stack is free again.

A linter that hangs does not keep the stack blocked. Delete on the job's
line of the banner, or `Stop the linter…` on the banner's menu, asks
first, then stops the linter as an ask's agent is stopped: the job's
process group, its process and the linter with what that runs, is sent
SIGTERM, then SIGKILL if it is not gone within a few seconds, and the job
lands at once, as stopped; the banner says `The linter is being stopped`
meanwhile. Nothing changes in the stack: the fixes folded so far are
thrown away, nothing lands for review, and the status line says so. The
stack can be edited by hand again, and the edits that waited for the
linter go. A job waiting its turn is dropped the same way, `Drop the
waiting lint job…`, on the banner's menu and the stack's, asking `drop the
lint job that waits its turn? Nothing was linted yet`: nothing is linted,
and the edits asked after it no longer wait for it. Delete on the job in `Runs…`,
or its menu there, does the same, and so does `stacked stop RUN`, the run
as `stacked runs` lists it. What the linter left in the worktree is thrown
away as the next job starts, so that the next job starts clean.

`$STACKED_LINT` names a program run in place of the linter's own, given
the command's arguments and the files: the tests' stand-in. From a process
that keeps nothing, as the tests' own, only that runs, never the command,
so never a `./mach`.

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
message, as two messages are compared, cut into sections (History, below),
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
  titles, then the bodies of their messages, are compared word by word,
  where their lines wrap aside. A message is cut into sections at its
  empty lines, a fenced block (```` ``` ````, `~~~`) staying whole, its
  empty lines and all; a section is code when one of its lines is a line
  of a fenced block, its fences too, a line indented by a tab or by four
  spaces or more, or a line that starts with a shell's prompt, `$ `, after
  any indent. In prose, the end of a line is a space as any other, and a
  gap of white space is one gap however long, so that a paragraph wrapped
  otherwise is the same. In code, the end of every line counts, as does
  the white space within a line, its indent too; only the white space that
  ends a line is not read. Each message is read so on its own, and a
  section that is code in one and prose in the other is not the same
  either. The break between two sections is a word of its own. A section
  the same in both, a paragraph only wrapped otherwise or ending its lines
  in other white space, is there once, as the new version writes it, among
  the lines left as they were: none of its lines is a change. A section
  that differs is the old version's lines taken out, then the new one's put
  in, each version with its own line breaks: what only one of them has is
  in the colours of `removed_word` and `added_word`, the words both have
  are drawn as any text, each line's sign is in the colour of its line,
  and a break only one has is marked, `↵` at the end of a line of code,
  `¶` on an empty line. Code is compared line by line,
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
  rev and time of each; a closed one's too, from `Filter comments…`,
  `closed`.

`stacked log`, `tree`, `versions`, `interdiff` and `runs` say the same on
the command line.

### Asking the agent about the history

On a record of `History…`, Space, `Ask agent about this, answer only…`
writes a question under it (`why was it dropped?`, `what changed since?`),
and `Ask agent, answer only…`, in the menu Esc opens, asks it from the ask
dialog, at the effort set there: answer only, the dialog's one permission
here, so the agent changes nothing. It is told the
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
last, StackED above; and the searches of the middle, The editor above; not
how the agent is asked, nor at what effort, which each ask dialog sets
anew, Asking the agent above);
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
  `answer_only_worktree`, when the worktree a change works in, and the one
  an answer works in, is made: `ondemand` or `precreate` (Asking the agent,
  above; `ondemand` unless set), as every ask runs in a worktree of its
  own, never in your checkout (a file of before that says `pwd` is read as
  `ondemand`, and stacked says so); `model`. What each kind of ask may use
  besides what stacked gives it (Asking the agent, above), as claude names
  the tools for `--allowedTools`: all of an MCP server's
  (`mcp__bugzilla__*`; there is no way to name every server's at once), one
  of its tools (`mcp__bugzilla__get_bug`), a command (`Bash(jq *)`, each
  command of a pipe needing its own). `change_tools`, for the asks that
  change the stack: nothing unless set. `answer_tools`, for answer-only
  asks: `["Bash(curl *)", "Bash(jq *)"]` unless set, which answers could
  always run before there were lists; taken out, an answer is no longer
  told it may read the web with curl. `try_tools` (built with the
  `try-push` feature; read either way, so one file serves both builds),
  for the asks that select a try push's tasks: `["Bash(curl *)", "Bash(jq
  *)"]` unless set, as for answers. `tools`, for every ask, builds and
  tests too, as before there were lists: an answer may use them too, so give
  only tools that read. `change_accepted`, `answer_accepted`,
  `test_accepted` and `try_accepted`: what the reviews of the agent's work
  allowed from now on for each kind of ask, added to its tools (The tools
  the agent could not use, above), which a review writes in a checkout's own
  settings alone.

  `stacked setup agent [NAME]` sets the lists up with you. It starts the agent in the stack's repository
  (else where it is run), where it has your MCP servers, and stops it as
  soon as it has said what tools it has. The lists are shown as one list
  of tools, a row each, with a box for each list under the columns'
  headings: `read only`, for answer-only asks (`answer_tools`); `change`,
  for the asks that change the stack (`change_tools`); and, built with the
  `try-push` feature, `try push`, for the asks that select a try push's
  tasks (`try_tools`). The boxes are ticked as the settings say, else as
  stacked has it: nothing for changes; for answers, `curl` and `jq`, and
  what the checkout's own claude settings allow, as you allow it yourself,
  but for what changes things; for try pushes, as the column for answers
  starts, as both only read, for you to check before the first try push.
  First comes
  what stacked always gives, which does not change: `always` in the column
  of each kind of ask that has it, greyed, with nothing to tick. Its last
  line is the checkout's linter, which changes are given (Linting, above),
  named with the rule that lets it run: `this checkout's linter, ./mach
  lint --fix, to run on the files a change changes (Bash(./mach lint *))`;
  or, with none given, why: `the checkout's linter, when it has one: this
  checkout has none`, `lint is turned off in this checkout`, or that no
  narrow rule lets it run. Then the
  list holds what the agent says it has (each MCP server, its tools all at
  once or one by one, and the agent's other tools, but those stacked gives
  every ask, said `one of the agent's tools`); what the checkout's own
  claude settings allow (`permissions.allow` of
  `.claude/settings.local.json`, yours, then of `.claude/settings.json`;
  their `deny` is left alone), each rule as written, but for a command's
  older form (below), said `allowed in .claude/settings.local.json` (or in
  both files, if both allow it): claude applies them when it runs in the
  checkout, but every ask runs in a worktree of the stack's own, which does
  not have the first, as git does not track it; what the checkout's skills let run, unticked,
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
  things too (rewriting history, filing a bug); an answer runs in a worktree
  of its own, never in your checkout. Its change
  box gives the skills to changes, written in `change_tools`. Whether what
  stacked refuses still holds against what a skill's allowed-tools let it
  run, claude's own text does not say, so stacked does not claim it. What
  the checkout's claude settings of yours deny is said above the list,
  `denied in .claude/settings.local.json, refused to every ask`, each rule
  once however it is written, to read, not to tick: every ask is refused it
  (Asking the agent, above). A tool of `tools` shows ticked in every
  column; taken out of one, it leaves `tools` and goes on in the other
  lists alone. A command or a tool of your own is added as a row, typed as
  claude writes a permission rule: a tool (`WebFetch`), a tool and what it
  may do between parentheses (`Bash(make check *)`,
  `WebFetch(domain:example.com)`), all of an MCP server's tools
  (`mcp__SERVER__*`, `mcp__SERVER`) or one (`mcp__SERVER__TOOL`); what does
  not read as one is refused, and said why (`make check is not a tool's
  name: a command is let run as Bash(make check)`). The row comes last,
  said `added here`, neither box ticked, the cursor on it, for you to tick
  what should use it; a rule that a row lets use already, however it is
  written, is that row, as it is. Saving writes each list, and `tools` if it
  changed, in that checkout's own settings (below; with `--global`, in
  `~/.stacked/config.toml`, for the checkouts without their own), the rest
  of the file as it was. Outside a checkout, only `--global` can be
  written.

  Above the list, what the reviews of the agent's work allowed from now on,
  as the checkout's own settings say it, under `Allowed from now on in
  reviews of the agent's work, in …/.stacked/local.toml: untick a rule to
  take it out`: a row a rule, its one box ticked, and the kind of ask it is
  for (`Bash(searchfox-cli *)   for answer-only asks`). Space unticks a
  row, and saving takes that rule out of its list, in the checkout's own
  settings, with `--global` too; the bottom line says what is left of each
  list a rule was taken out of. Asked line by line, each is asked in turn
  (`Keep Bash(searchfox-cli *), allowed from now on in a review, for
  answer-only asks? [Y/n]`).

  Asked line by line, the setup shows the list, a line a row as the screen
  draws it, to keep as it is (Enter) or to go through a row at a time. Each
  row is one question for every kind of ask, `Which asks may use Bash(jq
  *), stacked's default for answers? [R: read only, c: change, b: both, n:
  neither]`, the capital letter being what Enter keeps; built with the
  `try-push` feature, `read only` is for answers and try pushes both, as
  both only read, and `both` for every kind (the screen's own column tells
  try pushes apart); an MCP server's
  tools are asked all at once, or each in turn (`e`). Then it asks for a
  command or a tool to add, until a line is left empty, and asks the same
  question of each. With `--tui`, the setup is a screen of its own, with what saving writes, in the file
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
  out. Given up on by claude, the ask fails and says so. `reach_check`,
  whether the ask dialog checks, as it opens, that the network reaches the
  host claude talks to, and grays Ask Now while it does not (Asking the
  agent, above; on unless set): `false` turns that check off, for a setup
  where it is wrong though claude works, a proxy the check does not go
  through, say. The program run as claude is looked for all the same.
- `[reviews]` (built with the `moz-phab` feature, on by default; read
  either way, so one file serves both builds): `source`, `"phabricator"`
  or `"none"` (no pulling, no importing, no statuses of revisions read);
  `url`, the Phabricator a bare
  `D123` trailer is on; `arcrc`, where the tokens are; `token`, one for
  `url` instead; `trailer`, the trailer naming a revision
  (`Differential Revision`); `bugzilla`, the Bugzilla that a bug a commit
  message names opens on in your browser (`https://bugzilla.mozilla.org`;
  one that is not a web link to a site keeps it, and is said).
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
  `delete`, `find`, `view`, `read`, `whole`, `open_link`, `quit`); `search`, the
  search of the middle while its bar is open (`find_next`, Tab, and
  `find_previous`, shift-Tab). `keep_stack` and `keep_workspace`, the keys
  of the sync of the commit messages, went with it: a settings' file that
  still names them is told so (`keys.keep_stack is no longer supported: the
  sync of the commit messages it picked in is gone; it is passed over`),
  `stacked config` refuses to write them, and `stacked config --unset`
  takes them out. `delete` is `["delete", "backspace"]`
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
  typed, whatever the keys, and the bottom bar names their keys as they
  are there (`enter: a new line · esc: close`).
- `[lint]`: `command`, the linter of a checkout, which the stack's `Lint the
  stack` runs, and which asks that change the stack may run on the files
  they change (Linting, above): a checkout's own setting alone, read only
  from its `.stacked/local.toml`, which `stacked setup lint` writes; `""`,
  lint turned off there; unset, `./mach lint --fix` where `./mach` is at the
  checkout's top. `stacked config` shows it as a comment, so that what it
  prints, kept as `~/.stacked/config.toml`, sets no linter there.
- `[demo]`: `seen`, written by `stacked demo` the first time it is opened:
  the tour then starts on the list of its sections. `seen = false` shows
  the first tour again.

A file stacked cannot read, or with a setting it does not know, is not
used at all: the editor's bar says why, as every command does. A setting
with a wrong value keeps its default, and that is said too.

The settings were once in `~/.stacked.toml`, and StackED's state in
~/Library/Application Support/StackED/state.json. The first run of yours
that finds either, with nothing in its place in ~/.stacked, moves it there
and says so on stderr (StackED opened as an app, from the Dock or from its
launcher, says so in an alert);
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
(an ask, a lint job, a pull) with what came of it, every line the agent
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
  (```` ```claude ````, as the first files have them, reads the same) and
  the way they were asked (```` ```agent answer ````), then, when their run
  was asked at an effort, `effort` and its name (```` ```agent answer
  effort max ````), a turn of before having none; then, an answer-only
  ask's, `stack` and the rev of the stack it was asked on (```` ```agent
  answer effort max stack 12 ````), which its heading counts the edits
  since, by the stacks' index alone, no record read; and
  a reviewer's ```` ```reviewer ```` and their name; a reviewer's turn an
  edit of it replaces has `redacted` after the name (```` ```reviewer ann
  redacted ````), and their edit `edit` and the first line of the comment it
  edits (```` ```reviewer ann edit Hi Da ````), both when it is an edit
  redacted in its turn; a turn holding fences of its own (a
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
  each reply (an answer-only ask's with the rev of the stack it was asked
  on), each comment it opened. Nothing of it is applied before its
  review closes. Bringing nothing back, it holds none of them, and is held
  all the same while a comment it was asked about is still open. A
  follow-up asked from a review is queued as the comments it asks about,
  each naming the work it follows up (`on`) and saying it is one
  (`follow`); landed, its run holds nothing for review, and the work's run
  is written again with what it brought: its replies after the work's (a
  build and test's marked `read`), the comments it opened, a try push's
  tasks, the tools it could not use, and, among the work's follow-ups,
  which comments it was asked about and what a build and test found of
  each patch. A lint
  job's run holds the command it runs and where it stands, waiting its turn
  or running, and the stack it lints once it runs; landed, what it brought
  back, as an ask's does, with what the linter reported. A review
  record holds the marks saved for a run's review (each answer made on the
  stack before its change, kept, restarted or stopped, by that answer's
  run: `keep RUN`, `restart RUN`, `stop RUN`),
  the replies taken already, and whether it is closed; the last one is
  what stands.
- A reconcile with HEAD's run holds what it takes, each patch's change,
  its commit and that commit's message, and the work it went on from, if it
  was chained; made, its change of the stack, as an ask's.
- A moved record says where stacked left HEAD of a checkout on a stack
  (`{"t":"moved","stack":"s","checkout":"/path/to/top","commit":"…"}`):
  the commit HEAD was at right after Move HEAD, or HEAD following a review,
  moved it. The last one of a stack and checkout is what stands. A file a
  stacked of before wrote has none, and reads as if stacked had left HEAD
  nowhere.
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
  they need, what is chosen and marked read, where stacked last left HEAD
  of each checkout), and how the indexes stood; a checkpoint of before,
  with no index of where stacked left HEAD, reads as one with none.
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
settings'. Every call of Conduit is made by curl, which is given the token
and the call's parameters on its standard input, never on its command
line, where any process of this machine could read them; what went wrong
is said as Phabricator answered it (The editor, above): a status other
than 200, a page that is not JSON, or its turning away the requests of
this machine (HTTP 429). While it reads them, the banner says which revision it reads
(`⇣ pulling the reviews from Phabricator: D12, revision 2 of 3…`), and the
top bar `pulling…`; once it is done, the bottom line says what came of it
(`1 new comment from 3 revisions: it waits for your review`), or what went
wrong. In the editor, `Pull the reviews of D12 from Phabricator`, on the
menu of a patch's Phabricator line (The editor, above) and on the patch's
own in the patches pane, reads that patch's revision alone, as the stack's
pull reads each: the banner says so (`⇣ pulling the reviews from
Phabricator: D12…`), the bottom line says what came of it (`2 new comments
from D12: they wait for your review`), what it brings is held as a pull's,
and the line is read again once it is done. As the stack's pull, it is not
offered while reviews come from nowhere, and not done in the tour; one
pull goes at a time (`already pulling the reviews`). What is new waits for
your review, in the queue of work the agent's
asks land in (below): nothing is filed before you close that review. A
pull's parts are all accepted, never rejected: closing its review files
every comment it pulled (`Close review`, or `stacked review NAME RUN
--accept-all`; `--reject-all` refuses). Each
new thread is a comment opened, on its line, or on the patch's title for
one on the whole revision, its replies pulled with it turns of it; a reply
to a thread there already is a reply to it, the reviewer's turn (`ann's
reply waits for your review`, `Accept ann's reply`). The banner counts the
pulls apart (`1 pull from Phabricator waits for your review`), and the
review says what was pulled. Pulling again holds only what is new: a
comment is passed over when a thread holds it, open or closed (as it was
when you closed it, read from the history, which a pull reads), or when a
pull whose review is still open holds it; a comment some of which still
waits in a pull's review is pulled once that review closes. Nothing
remembers a rejection: a comment a stacked of before let you reject, which
no thread holds, comes back once, with the next pull, and is filed as its
review closes; a reply or an edit as well.

A comment the reviewer edited on Phabricator since it was filed (each edit
makes a version of it, with an id of its own) is held as an edit of their
turn (`ann's edit waits for your review`, `Accept ann's edit`). Its review
shows the comment as it was filed, then as it is now, one above the other,
compared word by word where their lines wrap aside, as two versions of a
message are (`ann's comment as it was filed, then as it was edited on
Phabricator`), then the thread as it would be. Accepted, the edit is the
reviewer's latest turn, headed by what it edits: `ann's edit of the comment
that said "Hi Da"`, the comment's first line, cut with `…` past 60
characters. The turn it edits, the latest version of the comment, stays as
it was, for what answered it to read as it was written, and is redacted:
headed `ann (redacted)` in the thread, `ann's comment (redacted)` where a
turn is named, and told to the agent as replaced by the edit below it. A
later edit redacts the latest version's turn the same way, and is added
after the rest. With its turn gone (rewound), the edit is the reviewer's
last turn, and nothing is redacted.

A new reply or edit in a thread you closed is pulled and held, with the
thread as it was when you closed it. Its review shows that thread, its
line saying `(closed)` after where the comment is, and says that
accepting it reopens the comment (`ann's reply to a comment you closed, as
it would be in its thread: accepting it reopens the comment`);
`stacked landed` says `to a comment you closed: accepting it reopens the
comment`. Accepted, as the review closes or taken at once, it reopens the
comment as `Reopen it` does, as it was when closed (as its history has it
last, when it was written since), its reply or edit in it (`reviewed: 1
reply accepted; 1 comment reopened`); until then the comment stays closed.
It is not pulled again: the pull's review holds it until it closes, and
the thread then. One moved to the stack's comments and closed there
reopens there; one open elsewhere since (moved with its lines, or back
from the stack's comments) takes the reply where it is open, and nothing
is reopened. Replies and edits to a thread moved to the stack's comments
go to it there. A comment removed on Phabricator stays in its thread as it was. A
reviewer's comment can be asked about like any.

A line of a commit message that names a bug (`Bug 123`), or the revision
(`Differential Revision:`), opens it in your browser: `o` there, or the
line's `Open bug 123 in your browser` (The editor, above), the bug on
Bugzilla, `reviews.bugzilla` (Settings, below). A line of what a comment
says opens them from its menu too (Writing comments, above).

The patches pane shows the status of each patch's revision after its number
(`[W]`: The editor, above), read over Conduit with the same token, all of
the stack's revisions in one search; and the Phabricator line over the
message of the patch the middle shows says it by name, whether the
revision's latest diff and its title and summary are the patch's, its
parent the one a submit sets, and how many of its comments are not pulled
yet, all of it read for every patch of the stack as it is shown. Space there shows what differs on
a screen of its own, and its menu pulls the reviews of that revision alone
(The editor, above).

### Submitting a patch

`Submit to Phabricator as WIP`, on a patch's menu (not while reviews come
from nowhere, `reviews.source = "none"`), sends that patch alone to
Phabricator as work in progress, as `moz-phab submit --wip` sends a single
commit: in Changes Planned, a status that asks no one for a review. In the
browser (`stacked web`) and in StackED, the same. Nothing is sent before
it is read and seen. First, away from the screen, the patch's revision is
read: the one its trailer names (`Differential Revision:`), or none, and
the submit makes a new one. A revision there already in work, Changes
Planned or a draft, as moz-phab takes it, is sent the patch as work in
progress; one open but not in work, Needs Review, Needs Revision or
Accepted, only the patch's title, summary and parent (below); one that
landed or was abandoned refuses (`D123 is Closed, not open: only an open
revision is updated, and nothing was sent.`). In work, its latest diff
must be a version of the
patch the database holds, the one in the stack or an earlier one: of the
same effect, or leaving the same files once both are applied on the
patches below, so that a diff moz-phab sent with more context is still the
version it was made from. When neither rule finds one, a third looks for
the version whose changes it makes: file by file, the same lines removed
and added one after another, whatever lines of context are around them and
wherever its hunks start. moz-phab sends a diff with more context than
stacked keeps, so once a rebase has changed lines of that context, the
diff no longer applies on the patches below, though its changes are that
version's. A diff known by this rule is never the patch as it is, as it
was made on the patches below as they were before: a new diff of the
patch is sent, made on them as they are now. Else it came from elsewhere,
and nothing is sent
(`D123's latest diff is not a version of patch 3 that stacked knows: it
came from elsewhere, and nothing was sent.`). And while HEAD is on the
stack, a patch whose commit in your workspace names a revision the patch
does not, none or another, as after `moz-phab submit` there, whatever else
the two messages say, is not submitted before anything is read: its menu grays the item and says to
give the patch that revision first, so that no second revision is made
(`Patch 3 names no revision in the stack and D34 in your workspace's
commit: give the patch D34 first, with Reconcile with HEAD on its menu, so
that no second revision is made.`); the other patches, and other differences, are not held back.

For a new revision, a dialog then says what is sent and where (`Patch 3 is
sent to Phabricator, at https://phabricator.services.mozilla.com, as a new
revision, with the status Changes Planned.`), with its title and its
parent. For a revision there already, a screen of its own shows what
replaces what, for your review: what is sent and where; then, of what
changes, the title and summary, Phabricator's then what is sent, compared
as an interdiff compares two messages (History, above: a paragraph
only wrapped otherwise once, as it is sent; a section that differs,
Phabricator's lines then those sent, what only one has in the colours for
words; and when nothing differs but where their lines wrap, or their
white space, that is said, and each paragraph written otherwise is there
as each writes it, Phabricator's lines then those sent, no word in the
colours for words); the parent and the bug, before then after, and the
status; and the code, each file as the latest diff leaves it, then as the
patch does, both on the patches below it, a file that only one of them
touches too. When the code is the patch's already, the screen says so at
its top. When the latest diff was known by the third rule, the screen says
at its top why a new diff is sent (`The latest diff of D123 makes the same
changes as version 2 of patch 3, but it was made on the patches below
patch 3 as they were before, and it no longer applies on them as they are
now, so a new diff of patch 3 is sent, made on them.`), and the code is
then each file as that version leaves it, then as the patch does, both on
the patches below it as they are now. ↑↓ (pgup pgdn home end) scroll, and 1 shows the code side by
side, or unified again, the lines of the title and summary that differ
side by side too, Phabricator's on the left, coloured the same way.
Enter, or `Send it to Phabricator` in its menu, sends it; Esc, or
`Cancel: send nothing`, sends nothing. Before anything
is sent, the revision is read again: if its latest diff, its status, its
title, summary or bug, or its parents changed meanwhile, nothing is (`D123
changed on Phabricator since you reviewed it, so nothing was sent. Submit
again to review it as it is now.`).

Only what changed is sent, as moz-phab sends it: a new diff of the patch
(differential.creatediff, its commit's author, message, parent and tree
set with differential.setdiffproperty), unless the latest diff is the patch as it
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
revision's parents are then left as they are). The Phabricator line takes
the parent by this same rule: it differs exactly where a submit would set
one (The editor, above). A submit as work in
progress sets no reviewers, as `moz-phab submit --wip` sets none: those
the title names (`r=alice,bob!`, `r?#build`) stay in the title, written
as moz-phab writes them, and nothing of the revision's reviewers is read,
said or changed. While it is sent, the top bar says `submitting…`; then
the bottom line says what came of it, and what was done and what was not
when a call failed partway (`D123 has the new diff of patch 3 now. D123
stays Changes Planned. But the parent of D123 was not set to D122: …`). A new revision's link goes into the patch, as a
new version taken at once, which the history says (`submitted patch 3 as
D124, in work`). Its trailer goes where moz-phab puts one: at the end of
the message's last paragraph when that paragraph is trailers
(`Reviewed-by: …`), else after an empty line. The workspace's commits are
not touched: the patch's commit there then has a message other than the
stack's, and `Move HEAD…` writes the stack's message, the trailer with it,
there, while HEAD is where stacked last left it. When HEAD is
on the stack and has the patch's commit, the diff is said to be that
commit, made on the commit below it (on the stack's base for the bottom
patch), with that commit's tree, as moz-phab says it. Else the diff is
said to be the patch's own sha1, made on the stack's base, and its tree,
the files as the patch leaves them on the patches below it, is written in
a check index of its own, the repository only read; when that tree cannot
be written, nothing is sent, and the bottom line says why. The diff is in
the repository the checkout's `.arcconfig` names (`repository.callsign`;
a revision there already keeps its own).

For a revision open but not in work (Needs Review, Needs Revision,
Accepted), the patch's menu has, in the place of `Submit to Phabricator as
WIP`, an item that sends the revision the patch's title, summary and
parent alone, named for what differs as the Phabricator line says it (The
editor, above): `Update D123 on Phabricator: its title and summary`,
`Update D123 on Phabricator: its parent`, or `Update D123 on Phabricator:
its title, summary and parent`. It sends no new diff, as only a revision
in work gets one, and no status, so the revision stays as it is. The
title is written as a submit in work writes it, but for `WIP: `, which
goes first only when the revision's title has it now, as the Phabricator
line compares it; it goes with the summary and the bug, where they differ,
in one edit (differential.revision.edit); then the parent, in an edit of
its own, by the same rule as a submit in work's. As a submit in work, it
reads and changes no reviewers: those the title names stay in it, written
as moz-phab writes them. The revision's latest diff is left as it is,
whatever version of the patch it is, and its repository is not read. The
item is grayed, and the menu says why, when only the diff differs (`D123
is Needs Review, so stacked sends it no new diff, as only a revision in
work gets one, and its title, summary and parent are those of patch 3
already.`), when nothing differs (`D123 is in sync with patch 3, so there
is nothing to send.`), and while what differs is not read and compared
yet (`D123 is Needs Review, so only its title, summary and parent would be
sent, and they are not read from Phabricator and compared with patch 3
yet.`). `?` on the item says what it sends.

Chosen, it reads the revision away from the screen, as a submit does
(`reading Phabricator to show what updating D123 sends: nothing is sent
yet`), then shows what is sent on the same screen as a submit's review:
what is sent and where (`D123, at
https://phabricator.services.mozilla.com/D123, gets the title and summary
of patch 3, and stays Needs Review.`), then the title and summary
compared, the parent and the bug before and after, and the status, said to
stay as it is (`status: stays Needs Review`). It shows no code. When the
code differs too, the screen says so at its top, and why it is not sent
(`The code of patch 3 is not sent: D123 is Needs Review, not in work, and
only a revision in work gets a new diff.`); else that the code does not
change. Enter, or `Send it to Phabricator` in its menu, sends it, the
bottom line saying `updating D123 on Phabricator…` meanwhile; Esc, or
`Cancel: send nothing`, sends nothing. Before anything is sent, the
revision is read again, as for a submit in work: if its latest diff, its
status, its title, summary or bug, or its parents changed meanwhile,
nothing is. When nothing differs, nothing is sent, and the bottom line
says so. Once it is sent, the bottom line says what came of it, as after a
submit (`D123 has the title and summary of patch 3 now.`), and the
statuses and the Phabricator lines are read again. While the
revision's status is not read yet, or is one stacked does not know, the
menu has `Submit to Phabricator as WIP`, and the status the submit reads
decides what is sent. For a revision that landed or was abandoned, `Submit
to Phabricator as WIP` is grayed, and the menu says why (`D123 has landed,
so stacked sends it nothing.`, `D123 was abandoned, so stacked sends it
nothing.`).

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
  patch leaves them, and the tree, for a submit.
- `ops.rs`: edits to a stack, `$EDITOR` (the patch as mail, its code); the
  interdiff of two versions of a patch.
- `git.rs`: what stacked asks of git: mail from a range, the stack's
  worktrees and the agents', a checkout to edit a patch in.
- `ask.rs`, `claude.rs`: asking the agent about comments, from the queue to
  landing, and the folders of the agent's rebase plans; claude's command
  line, its stream and its replies.
- `reach.rs`: whether the agent can be reached, for the ask dialog's Ask
  Now: its program found, and the network reaching the host claude talks
  to, a stand-in answering for the tests.
- `held.rs`: what an ask brought back, held for your review; work sent
  back, and the follow-ups that only read asked from its review, what they
  bring back coming into it (`come_in`); and the marks of the answers made
  on the stack before a change, kept, restarted or stopped as the change
  is accepted (`ask.rs` does what each mark says).
- `head.rs`: where HEAD of your checkout is against a stack, its commits
  marked, and HEAD moved along it, only as stacked left it or with no mark;
  what the view of why Move HEAD does not move HEAD reads of the checkout.
- `reconcile.rs`: Reconcile with HEAD, the commits' messages taken into the
  stack, queued and reviewed as an edit is.
- `messages.rs`: two versions of a commit message compared word by word,
  where their lines wrap aside, and so as a diff of their lines, for the
  interdiffs and the submit's review to colour; and what
  each section of a message says, for the reading marks to hold.
- `wrap.rs`: a commit message's long lines wrapped at a width, its
  paragraphs of prose filled again, for the menus to offer.
- `links.rs`: the links a line of a commit message or of a comment's text
  has, to open in your browser: its web links, markdown's too, and, with
  the `moz-phab` feature, the bugs it names and the revision its trailer
  names.
- `review.rs`, `phab.rs`, `submit.rs`: Phabricator, built with the
  `moz-phab` feature (on by default): the reviews pulled, the stacks
  imported, a patch submitted as work in progress, or its title, summary
  and parent alone to a revision in review, the statuses of
  revisions read, and what the Phabricator line over a patch's message
  reads and counts; Conduit called by curl, the token on its standard
  input, what Phabricator answered said, and its refusals of this machine
  (HTTP 429) an error of their own.
- `try_push.rs`: the try push, built with the `try-push` feature (on by
  default): what an ask of a try push is told and answers with, its tasks
  read from its answer, and what a round asked again is told of them; the
  push of the tasks a review accepts (its run's `push`), made by `stacked
  try-push` in the stack's worktree for it, its base checked against the
  remotes mach takes as Firefox's, `./mach try fuzzy --no-push` compared
  with what was accepted, then the push, mach's lines in its stream, its
  login and its link kept, its replies, the lock of its comment, and its
  stop; then the push followed on Lando and Treeherder (`follow`), its
  tasks counted as Treeherder shows them, and its summary, held for
  review once it is done.
- `lint.rs`: linting a stack: the linter of a checkout (its own setting,
  else `./mach lint --fix` where `./mach` is), its command split as a
  shell splits words, and the rule that lets a change run it; a lint job
  (its run's `lint`), its turn in the queue
  taken as a change's (`ask::claim`), run by `stacked lint-job` in the
  stack's lint worktree, patch by patch, each patch's fixes folded into it
  once the patches above still apply, the rest in a patch at the end with a
  comment to send; what the linter reported; what lands for your review;
  and a job whose process is gone.
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
  list with a box for each kind: what stacked always gives, the checkout's
  linter named; where what it lists comes from (the agent,
  the checkout's claude settings, its skills and its instructions), which
  rules mean the same and are one row, what the checkout's claude settings
  of yours deny, refused to every ask, what the reviews allowed from now
  on, to keep or take out, on a screen (`--tui`) or line by line, and what
  saving writes.
- `refusals.rs`: the tools the agent could not use, as items of the
  review of its work: the rule each call refused needs, narrow for a broad
  program and wider to pick, the narrowest also the rule that lets a change
  run the checkout's linter; one item a rule, what is noted and why, the
  agent's word on each; their three states as the review's marks; what this
  work may use for itself, round after round; and what is saved from now
  on, in the checkout's own settings.
- `editor.rs`: the inline comment editor.
- `paint.rs`: colours, and drawing text and boxes.
- `tui.rs`: the editor, and the work it does away from the screen; what a
  click is on; the views of the code, the middle split in two among them,
  each half its own (`Half`). `tui/reviewing.rs` and `tui/searching.rs`:
  its screens for reviewing what the agent brought back, and for
  searching;
  `tui/submitting.rs`, what a submit to Phabricator sends, said in a
  dialog, or reviewed on a screen of its own, before it is sent;
  `tui/revisions.rs`, the status on Phabricator of each patch's revision,
  which the patches pane shows, and the Phabricator line over the message
  of the patch the middle shows, what differs on a screen of its own, the
  pull of its reviews alone, the submit item of a patch's menu as its
  revision's status has it, and when each is read, which revisions are
  read again, and how long nothing is read once Phabricator turns this
  machine away; `tui/compared.rs`, two
  versions of a text compared word by word where their lines wrap aside,
  as rows: a title and summary in the review of a submit, a reviewer's
  comment edited on Phabricator in the review of a pull, a commit's
  message against its patch's; and a diff, or a commit, as the middle
  draws a patch; `tui/head_why.rs`, the view of why Move HEAD does not
  move HEAD, and what you can do in your checkout so that it can;
  `tui/banner.rs`, the banner of the agent's work; `tui/places.rs`, where
  each pane's cursor was, each half's of the middle split in two;
  `tui/finding.rs`, the search of the middle (`/`); `tui/ask_dialog.rs`,
  the dialog every `Ask agent…` item opens, its permission and effort set
  each time, then Ask Now, Ask Later or Cancel; and a review's `Send it
  back to the agent…`, then Send Now or Cancel.
- `web.rs`, `web/client.rs`, `web/index.html`, `build.rs`: `stacked web`,
  its server, its wasm and its page; the build script compiles the wasm,
  or, where Rust lacks the wasm target, stops the build and says what to
  run.
- `app.rs`: `stacked app`, StackED: the editor on a thread of its own, and
  the window that paints its screen and sends it what is done there; the
  fonts the system has for it, as fontdb finds them.
  `app/windows.rs`: its windows, on every system, one for each database
  opened, the first in eframe's root viewport, and what the File menu's
  items do; `app/menu.rs`, the menu bar at the top of each window (egui's),
  its one menu, File, used with the mouse; `app/dialogs.rs`, the
  system's Open and Save dialogs and its alerts (rfd's), asked on a thread
  of their own. `icon.rs`: StackED's icon, drawn at any size.
  `launcher.rs`: what StackED is to each system, made by `stacked app
  --bundle` and read when the system opens it: StackED.app on macOS, its
  launcher and icon on the desktops that follow freedesktop.org, the
  terminal `$EDITOR` runs in on each, and the Edit menu macOS's panels
  need; the one place that tells the systems apart. `bundle.rs`:
  StackED opened as an app, with no terminal: the login shell's
  environment taken, and `$EDITOR` run in a terminal window of its own.
- `demo.rs`, `memory.rs`: `stacked demo`, the tour; and the database and
  repository it keeps in memory, with a stand-in for the agent.

## Tests

Run `cargo test` after any change. With every feature on by default,
`cargo test` runs every test: the browser's, the tour's, Phabricator's,
StackED's and the try push's too (the web feature needs the wasm target,
as in Install).
`cargo test --no-default-features` runs the minimal set, the tests of
stacked built with no feature, and needs no wasm target; `--features` then
adds the tests of each feature named. No test runs claude: a script stands
in for it; nor a linter of yours, nor any `./mach`: a script stands in for
the linter; none reaches Phabricator: its revisions and their comments are
read by stand-ins, and what a submit sends is answered by one, which keeps
every call made to it, so that a test can see that a refusal wrote
nothing, and which, as Phabricator does, makes no revision of a diff whose
commit has no tree; the statuses of a stack's revisions, and what the
Phabricator line reads, are answered by one set for the test's database
alone, as every editor reads the statuses of the stack it shows, so that no
test's stand-in answers another's editors; and
in a test's own process, which keeps nothing, an editor with no stand-in
reads none. How a call of Conduit is made is seen with a stand-in for
curl, a script that `sh` runs, which keeps the arguments it is given and
what it reads, and answers as the test says; and with curl itself,
sending to a server that the test runs on the loopback (127.0.0.1), so
that nothing leaves the machine. Nor does the ask dialog reach the network: whether it reaches
the agent's host is answered by a stand-in set for the test's database
alone (reached, not reached, or only once the test says), and in a test's
own process an editor with no stand-in checks nothing. None opens a window, nor a browser: a script stands in
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
shares. In `tests/cli.rs` and `tests/try_push.rs`, whose asks a run of
the binary starts (a try push's keeps its copy of the task graph there),
the function that gives every run of the binary its home gives it a
folder of the test's as `$STACKED_TODO` too. The tests'
own processes keep the plans beside their database, in the test's own
folder, and the asks they start are told the same. In a folder of their
own, the tests in `ask.rs` show which plan folders are removed: one whose
git directory is gone, and one that names no git directory once nothing in
it has changed for a day; not one whose git directory is there, rebase or
not, nor one that an ask still running writes its plan in. They show a
landed ask's folder removed, but not while a rebase is under way in its git
directory, nor while another ask still running writes its plan there.
`tests/cli.rs` shows the folder gone once an agent's change to the whole
stack lands; `tests/try_push.rs`, a try push's selection's, one for each
ask, gone once it lands or is stopped.

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

No ask waits for a setup, so no test sets one up first. In
`tests/cli.rs`, the test's home holds a settings' file of its own
(`[agent]` alone), which a test adds to; the editor's tests share settings
of their own (`settings()` in `tests/tui.rs`), the defaults. The tests of
the first ask start from settings of before that say the setup was done
(`agent.setup`), to see that setting passed over.

With `STACKED_TEST_CHECKPOINTS=1` set, every write writes a checkpoint, and
every database read from one is read whole too and the two compared, a
difference a panic: run the suite so too after a change to the database,
or to what reads it, as `STACKED_TEST_CHECKPOINTS=1 cargo test`. The tests
in `db.rs` show where stacked left HEAD kept per stack and checkout, each
move replacing it and the same again writing nothing, read from a
checkpoint as when read whole, and a checkpoint of before, with no index
of them, read all the same, as one with none; and the edits since a state
of a stack counted along its tree of edits by the index of the stacks
alone (another version once an edit is undone and another made in its
place, nothing for what is no state of it), read from a checkpoint as
when read whole, its history not read.

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
answers, opens comments, tests), every ask in a worktree of its own (your
checkout's HEAD, branch and files as they were while the agent works and
after; a settings' file of before that says `pwd` read as `ondemand`, and
said; `stacked config` refusing it; the asks a stacked of before started in
your checkout landing, or found dead, the checkout left as the agent left
it, never removed; no `apply`, the applies of before listed), a split
following its comments, a comment
moved to the stack, a patch's comment asked of the whole stack adding a
patch on top, a comment written with `comment --work` on a patch the
agent's work adds keeping that work from being accepted or rejected, sent
back with it and asked of the next round, on that patch, which accepted,
holds the patch, the comment and its reply, work that brought nothing back held for review while a
comment it was asked about is open (and not once every one is closed), the
last reply taken at once closing its work's review with its comment open
or not, a reply going with a comment the agent opened not taken at once, a
reply written again, a
reference no longer named detached by `stacked reply --edit` and the
agent's reference by `--edit --turn` (said so), and a rewind keeping one
never named, a web link attached told the agent as one it need not fetch
(the prompt asking it to point at the stack first) and listed with its
comment, the agent's own link kept in its reply (one not of the web left
out, with its number), asks waiting their turn, an answer made on the
stack before a change accepted asked again on the stack it makes once the
change's review marks it restart (and given what it was allowed for this
work, what it allowed from now on saved), an agent stopped with
`stacked stop` (by the start of its run's id, the shell offering only the
runs that run) and killed as it will not exit on SIGTERM, its ask landed
at once as failed and said so on its comment and in its transcript, a
second stop refused, the effort each ask goes
at (`--effort`, else `high`, an effort an editor of before kept in the
test's home no longer read; the settings' own taken out; a level claude does not
take refused, nothing sent; a batch at the effort its comments were
batched at, and work sent back at the effort it was asked at unless
`--effort` says otherwise; the effort an ask was asked at said by
`stacked landed`, `stacked runs`, `stacked comments` and its transcript,
on the comment its reply waits on, on its reply and on the comment it
opened, and kept on the fence as the thread is written again, a turn of
before as ever), a rebase, a patch whose lines a new version of
a patch below moved keeping its version and its diff as stored when the
agent's rebase rewords another patch (the stack diff marking only the
reworded one), the agent's change of such a patch, its commit left as it
was or made again, landing with nothing to accept, the patches below and
above the one the agent's rebase rewords keeping their versions, their
messages and authors as stored, when git gives those back otherwise
(empty lines in a row, under the title as a sync of before had taken them
from the checkout, white space at the end of a line or of the
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
refused and taken up again; in this checkout, refused while Move HEAD
would not move HEAD, and taken once stacked has put HEAD on the stack),
`stacked head` listing the patches whose message differs from its
commit's, with moz-phab what each side names when they differ by the
Differential Revision, then why `--to` would be refused, the flags that
synced them once refused as unknown, `stacked head NAME --to N` moving
HEAD only as stacked last left it or on the stack with no mark, where it
left HEAD remembered from one run to the next, the stack as it is now put
there whatever it has become, and refused with why otherwise (a commit of
yours on top, one reworded outside stacked), and, with moz-phab, never
below a commit that names a revision its patch does not, every commit up
to it kept as it is, a revision named outside stacked refused, trimming, the settings (in ~/.stacked, or the
folder `$STACKED_HOME` names, the folder made by the first written, a file
`$STACKED_CONFIG` names over it; the files of before moved in by a run of
yours, said once, never over a file there, and not by the shell's
completion; a checkout's own over them, an ask on a stack having its
checkout's wherever stacked runs, one git tracks not read; and what each
kind of ask may use, set up with the agent, line by line, a row of the
one list at a time for every kind, from what it has, the checkout's claude
settings and instructions, in the checkout's own or with `--global`, a
command saved the newer way, though your claude settings or the
checkout's own list write it the older, and your claude settings never
written; each kind given its own list and every ask's, and the
refusal of a tool naming its kind's list; what your claude settings deny
in the checkout refused to a change, an answer and a build and test, and
an answer refused too what would make the commands it is given write a
file or run a program, a change not; the
first ask sent at once, with no setup; the calls an ask was refused landing
as items of the review of its work, one a rule, a failed run's too; a tool
allowed from now on saved in the checkout's own list for its kind alone,
sent back or closed, and one allowed for this work given to every round of
it until its review closes; the setup listing what reviews allowed, to
take out), Phabricator's comments
held for review then filed, none rejected (none twice, but those a review
of before rejected, back once; an edit held as an edit of its turn, then
the reviewer's latest turn, the turn it edits redacted; a reply or an edit
in a thread you closed held with the thread as it was, `stacked landed`
saying so, and reopening it once accepted, on the stack's comments for one
moved there, as its history has it last when written since), a stack
imported from Phabricator's revisions (by
number or link, named or not), Phabricator's commands there only when
built with it, and
what does not apply being refused, edited again, or read-only. `tests/tui.rs` draws the editor into a buffer and presses keys:
that no letter but 1, 2, 3 and / does anything, keys mapped by the settings
(the bottom bar naming them, but as they are where what is typed is read),
Delete and Backspace as the menus' items that close, drop, stop or reject
(a comment closed, with those its answer opened, its agent stopped and
its ask landed at once as failed, its ask taken
back from the queue once asked, the comment open, or taken out of the batch at once; a patch dropped and the stack
deleted; a file's changes dropped from its patch for review, refused when
a later patch needs them, a rename, an added file and a deleted one
dropped whole, and the patch's drop offered for its only file; in a
review, a part rejected, a comment closed with it, what you wrote taken
back; landed work rejected and a comment closed from the banner; an ask
stopped from the runs, landed at once too; what waits its turn taken out
of the queue, the confirmation saying what is taken out and what stays,
the status line what was done: an edit ask from its line of the banner,
the banner's menu and its comment in the middle, the comment staying open
and the one asked with it still waiting, the cursor on the banner
following its line; a round sent back, the work staying as you sent it
back, its review closed and the stack as it was; a reconcile with HEAD
from its line, its menu (Enter there, saying what it waits for), HEAD's,
the patch marked's, the banner's and the runs, as if it was never asked,
the marks still there, and the edit asked after it, which no longer waits
for it, going once the change before it lands; each asking first where its item does, Esc then
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
menus by place, what `?` says where, writing in every pane, the keys in
a patch looked at closer in a review going from the line Enter went to
in its stack diff, though nothing was drawn since, the stack's menu
with the stack's comments to ask about after Home, or a right click on
its header, with nothing drawn since, the comment
closed last on a line shown dimmed in the box until you write (↑↓ said only
when it is longer than the box, which ↑↓ and pgdn then scroll down to its
last line, the box above the bottom bar), the regions it
says it drew (its panes, then the boxes over them, inside their borders:
what StackED's selections keep to), the first ask going at once with no
setup, at the effort its dialog set, given stacked's defaults, the setup's old
mark in the checkout's own settings passed over; the tools an answer was
refused as items of its review (one a rule, the calls under the agent's
comment on the right, a broad program's rule widened from the menu, what
changes things noted), each refused by Delete, allowed from now on by
Enter and for this work from the menu, its three boxes naming the rule
and the file; followed up to answer only with a tool allowed though
nothing is written, the dialog saying what the work may use and what is
saved, the follow-up given both and its stream saying so, the comment told
in your answer after its reply, the follow-up's reply in the same review,
and what the work was allowed for itself ending as its review closes, the
close saying so; what is allowed from now on saved closing,
accepted or rejected, in the checkout's own list for answers alone, not
offered where git tracks that file; and a reply on an item sending the
work back; in the review of the agent's work, a comment on a patch it adds
(Enter on that patch's line of the stack diff), and what you write in a
thread the agent or a reviewer opened that it was asked about, each listed
with what you wrote, keeping the work from being accepted or rejected,
taken back (that thread as it was before the review), and sent back, the
next round asked about it, on that patch of the stack it goes on from;
that round rejected, the comment on the patch it added moved to the
stack's own comments, open, `Move it back` grayed; work sent back from
its dialog to change its patch, rejected, its round landing in its place,
asked of that patch, and that round's work sent back to change the
stack, from a patch's comment, the next round asked of the whole stack;
followed up to answer only, the work staying for your review, its change
still marked accepted, the review saying the follow-up runs and the
cursor on it, what you wrote gone with it, the follow-up asked on the
stack the work makes with the work's reply and your answer in its thread,
its reply coming into the same review after the work's, your answer to
the reply before it grayed, nothing waiting for a review of its own, then
the review closing as ever, the replies in their thread in turn; followed
up to build and test from a comment on the stack, what it found a section
to read (its reply, and what it found of a patch, each on the right, Enter
marking nothing), the review closing with its reply in the thread after
your answer and nothing filed on the patch's title; the review closed
while the follow-up runs, its menu saying so, closing asking first, the
follow-up stopped, its ask landed as failed and said so on its comment,
nothing of it waiting for review; a comment asked
about and its stream and answer shown in place, the stream folded in the
step that reads its landing, as the banner says what landed (its line
read first cut short, as it is being written, then whole), its agent
stopped from its
menu (the ask landed at once as failed, said so, its stream folded, the
comment's menu whole again, and the agent's wrapper, a child of the
tests' process, waited for), an ask done leaving no zombie once the
editor looks again, an ask whose agent is gone without landing it landed
at the next look (its process gone, or a zombie the editor did not start,
left for whoever started it to wait for), a build and test stopped as a
review closes landed at once though it had no process yet, the review's
word kept, the ask dialog (every `Ask agent…` item opening it with the
permissions that apply there: a line's, a patch's as a whole, the
stack's, a question's on the history, the comments listed with how many
each asks about, the comment editor's, changed or as it is; both lines
unset, Ask Now and Ask Later grayed and the dialog saying what is
missing; ← and → setting each line's first option from unset, then
stepping on and stopping at either end; Enter opening the list of a
line's options under it, on the one set or on the first, ↑↓ going round
it, Home and End to its ends, ← → doing nothing in it, Enter picking an
option and Esc leaving the line as it was, either closing the list alone,
the dialog open on that line, and Ask Now and Ask Later grayed until both
lines are set; ↑↓ going round to the buttons, ← → there past those
grayed; Ask Now saving what changed first and asking at the permission
and the effort set, the stand-in given that `--effort` and the tools of
that way; Ask Later putting it in the batch; Cancel and Esc asking
nothing, back to writing the comment; a second dialog unset, nothing
kept for the stack; clicks opening a line's list, picking an option of
it, closing the list alone from anywhere else, the mouse over an option
taking the list's cursor there, pressing a button, cancelling out of the
box; Ask Now
grayed while the network is checked, saying so, and while it is not
reached, saying how to turn the check off, Ask Later going all the same,
a dialog opened a moment later not checking again; reached, Ask Now
asking; `agent.reach_check` off, nothing checked; the items that open
something to write first picking no effort; the review's `Send it back to
the agent…` opening it on the work it sends back, its title saying so,
nothing set, the permissions those of the comment going back, its
buttons Send Now and Cancel, Send Now grayed until both lines are set and
pressed grayed saying why, the line under the permission saying what each
does with the work, `?` what Send Now does as the permission set does it,
Cancel and a click out of it sending nothing, the review as it was, a
second dialog unset; Send Now grayed, saying why, while the network does
not reach the agent, nothing sent), the effort an ask goes at
(the stand-in started at the effort the dialog set, the settings' own
taken out; a batched comment, sent now or with the batch, at the effort
it was batched at, one waiting its turn at its own, one queued by an
older stacked at the agent's default; work sent back at the effort its
dialog sets, the work asked at low, its item no dropdown of its own; the
effort the agent was asked at shown wherever its work is
named: its comment's state with the agent and replied to, in the middle
and the banner, the banner's landed line, `Runs…` and the transcript, the
review's title, the comment it opened and its reply in the review, then
their turns in the middle, the comments pane, the search, the turn's name
in its menu and in the editor's title, the comment's history, a reply
added keeping each turn's mark, and a thread of before as it always
was), the screen never waiting on
the agent, redrawing not slowing as the agent writes (a frame timed as the
time the editor ran to read and draw it, not by the clock, which counts
too the time a busy machine gives to other work), the filter kept in
the file, `Filter comments…` on a patch's menu as on both headers, each
counting the comments the middle shows, as its title says (from the
comments pane's header, the patch's or the stack's own), the filter
picked from one patch's filtering every patch's, → opening the comments
pane of a patch with none, and of the stack with none of its own, its
header saying the filter and that it lists nothing, the line under it and
`?` why, its menu there with the filter, ← putting it away, the middle as
wide as it was, and → bringing it back, put away by hand too; the line's
texts for a filter hiding a patch's only comment or the stack's, and for
none closed, the pane staying then, or going; a comment closed and none
open, the pane listing it under `closed` and → reaching it, the views of the code
(the menu of 1, the view drawn marked,
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
the message in an interdiff compared word by word (the words put in its title as words, the words it keeps as
any text, its sign as its line's; in the review of the agent's work, a
paragraph the work only wraps otherwise no change, the cursor on the
first line of code that changed, and a comment written from a line of it
there on that line of the new version; accepted, its lines read as they
were, Delete there taking back the mark that read them and Space reading
them anew, and the history changing no line of it; a word changed in a
paragraph wrapped otherwise the one word coloured in each; a command
wrapped otherwise a change, its break marked; the diff after the message
painted as words as ever), the submit's review comparing the title and
summary so, side by side too, and saying at its top why a new diff is
sent when the latest diff makes the patch's changes on the patches below
as they were before a rebase, no code under it, then sending one,
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
the stack, the row over it; hidden by `Filter comments…`, from the agent's
stream too), and leaving the cursor elsewhere where it is (closed from the
comments pane, which picks the next comment or the one before at the end,
from the banner, by another stacked; hidden by `Filter comments…` from
either header, moved to the stack, reopened), a turn written again in its
place taking the cursor back to it, what you said last written again (off the queue meanwhile;
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
it, the links of a line of a commit message (a web link, two on a line
picked from their menu, none on a line of the message said and nothing
done on a line of code; a markdown link named by its text, and one
between angle brackets; with moz-phab, the bug a title names on Bugzilla
and on the settings' own, the trailer's revision at its link, and a bare
one on the settings' Phabricator, none without it; without moz-phab, the
web links alone) opened by the stand-in from the line's menu, from the
menu of a right click on it and with `o` (which the settings make another
key), a click opening none, in the middle, reading too, in the review of
landed work (a line of an interdiff in the stack diff, one taken out too,
and a patch looked at closer), in History…'s stack diff and in what a
submit sends (side by side too), the links of a line of what a comment
says opened from its menu, after the comment's items, the same items (a
web link, a markdown link by its text, one between angle brackets, and,
with moz-phab, a bug and a revision on the settings' Bugzilla and
Phabricator), none on a line of a code block nor on the comment's
heading, a line folded one line from each of its pieces, a comment
collapsed the first line it says, the comments pane expanded the line it
says under the comment (none compact, none when that line is code), and
`o` opening none there, a stack made with none there (imported from the workspace, a
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
it as the agent's work does, their review rejecting nothing (no Delete, no
item, nothing to reject as it closes), a reply to a comment you closed
shown with the comment as it was, reopening it once the review closes, an
edit shown as it was filed and as it is now over its thread, then headed
by what it edits, the turn it edits redacted; replies taken with Enter, the last one
closing their work's review, its comments left open; Enter opening the
review of work that opened comments too, and saying to review first the
edit that work goes on from; and work that brought nothing back reviewed
for its comment alone), the review's list saying how each part stands
(the title of the stack operations with its word in green once accepted,
in red once rejected, in the title's colour to review, and each of its
lines marked `✓` in green, `✗` in red or `·` as any text, the operation
after the mark in its kind's colour, `modify`, `noop`, `remove` and `add`
each its own; a line under the cursor in the cursor's colours, its mark
too, and picked in its own; a comment that asked `✓ keep` in green and
`✗ close` in red, cell by cell) and Esc's summary of it (each count
marked so, in green or red, what is to review and the rest dim, and what
else the menu says, `You wrote 1 answer…`, in the note's colour), the
answers made on the stack before a change in its review (one landed and
one running, a section of their own, each `·` until it is marked, saying
whether it runs or landed and what it was asked about; `Close review`
and `Accept all remaining and Close` grayed until each is marked, `Reject
all remaining and Close` not, Esc saying which are not marked yet; the
right pane, the bottom bar and `?` on one; Enter keeping one, Delete
stopping one, Space's three boxes, the one picked checked, the menu
staying, and its items marking them all; the one running landing while
the review is open, its line saying so, its mark kept; kept, the closing saying so, the
one running going on and landing on the stack it was asked on, its reply's
heading in its open thread saying `on the stack 1 edit ago`; restarted,
each asked again on the stack the change made, the one that had landed
shown what it brought back; stopped, the agent stopped and its ask landed
as failed, said so on its comment and in its stream, the landed work
thrown away unseen, both comments open and nothing asked again; the change
rejected, each going on as it was; unchained, none listed, the answer
waiting as it was, its heading saying how old its stack is all the same;
`stacked review --accept-all` refusing, naming the answer and where to
mark it, then accepting once it is marked keep and the review saved), the
heading of an answer's turn counting the edits since the stack it was
asked on (1, then 2; `on another version of the stack` once an edit is
undone and another made in its place; nothing in a closed thread), a commit message that differs from its patch's
holding back nothing (none of it in the banner, landed work reviewed and a
reply taken at once meanwhile, the stack and the commit each keeping its
own message, s and w doing nothing), HEAD shown where it was while stacked
moves it and where it went once the move is over, a message whose empty
lines in a row `git am` made one leaving HEAD in sync, HEAD forked from
the stack drawn as a graph in each of its cases (the trunk, then the
shorter branch, then the longer, HEAD above the whole stack, the stack
moved on since, far ahead on the same base with no trunk, the commits HEAD
alone has capped at three rows, the stack's patches all of them, and HEAD
on another base off the stack), the cursor going over the commits HEAD
alone has to HEAD's row and the patches, the marks of HEAD's commits in
their margin, `≠` and `!`, HEAD's row ending `· locked below patch 2`,
folded, `?` on each mark saying what it means, what it locks or grays and
how it clears, the bottom line saying only that `?` does, and on HEAD's
row saying where HEAD is, its marks, the lock and which case of Move HEAD
applies, or why none does and where to see why, its menu with Move HEAD
not grayed, a click on it and Delete there doing nothing, `Move HEAD…`
moving HEAD only as stacked last left it or on the stack with no mark,
else offered all the same, `?` on it saying what it does, and opening the
view of why (a commit of yours on top, one reworded outside stacked), HEAD
as it was, and moving it after a restart from what the database
remembers, the stack moved on since, its place never below a commit that
names a revision its patch does not (↑, Home and a click stopping there,
`?` saying why), the view of why Move HEAD does not move HEAD (a commit
above the stack, HEAD on its branch, then on none, shown whole with what
to do; a fork with commits only the checkout has, each, the first ten
then how many more, forked at the base too; a commit marked `≠`, HEAD's
own amended; one marked `!`, with moz-phab; HEAD on another base, sharing
no commit, under the base, forked from below it; several at once, each
with the others to deal with too; each line's comment and its commands,
their commits named, none of them run; the view following HEAD once the
checkout changes, then saying that nothing keeps Move HEAD from moving
HEAD; the arrows, a click, `?`, Space and Esc there, from the stack's
header too), HEAD following
a review remembered where it went, Reconcile with HEAD (on the menus of
the patches marked and of HEAD's row, not of another, `?` saying what it
does; made at once, waiting for review in the banner, grayed while it is
under way; its review saying what it takes, stacked's comment saying what
to run from the top down, `git commit --amend` for HEAD's own commit and
`git rebase -i` for one below; accepted, the messages taken and the marks
gone; rejected, nothing changed and the marks there; asked while the
agent changes the stack, waiting its turn, the banner saying so and its
menu, Enter there, what it waits for, made once that change lands, going on from it, then from the
stack its acceptance made, an edit asked after it waiting until it is
reviewed and an answer going at once; going on from an edit by hand
rejected, dropped and made again from the stack as it is; grayed with no
commit marked), this checkout grayed for edits by hand
meanwhile (dim, ← → going past it and no further than either end, a
click picking nothing, the menu and
`?` saying why, an edit there as the stack's menus last picked it
refused), the comment editor hiding
nothing in each view of the code, each pane keeping where its cursor
was, the patches pane longer than the screen scrolling only once the
cursor would leave it, as far as the cursor needs (down to the end and up
again, a page at a time, Home and End, titles folded, files listed, ↑
from a patch onto the last file of the patch above and ↓ back again, from
the top line too, the lines of a patch whose files are listed pinned on
the top lines once scrolled past (drawn as in their place, its title
folded, its revision's status and mark, reading too; the files scrolling
under them, the pane still while the cursor moves in the lines under
them; ↑ above the first file and Esc showing them in their place, ↓
under the last picking the next patch; the mouse over them underlining
them, a click on them going to the patch; a pane too small for them and
a file; HEAD's line in sight with them), HEAD's rows and its line pinned
to either edge, a patch
and HEAD being moved, HEAD moved, a comment on the patch written from a
file, the cursor in the middle, the screen smaller and larger, wider and
narrower, the stack losing patches, and another stack shown, its pane as
if never drawn) and
the comments pane likewise, the patch's title kept at the top of the
middle, the patches pane's
states (expanded, files, hidden) and its files (listed under the patch
picked, gone through with the arrows past their ends, over the same lines
up as down, ↑ on a patch or on HEAD's row going to the last file of the
patch above, reading too, or to that patch itself when it has no file to
list, gone into with Enter and →, clicked, and picked from the middle,
where ← comes back to the file; reading, each saying the lines left to
mark read as marks are made and taken back, those of white space alone
counted only as the settings say, a file with no newline at its end ticked
once read whole, its patch too, the pane keeping its width from patch to
patch), the
statuses of the revisions there (with moz-phab, each status after its
patch's number, a letter listing files and expanded alike, `[?]` for one
stacked does not know, no words; none before it is read, nor for a patch
naming no revision or one Phabricator does not have; the titles starting
together, those folded folding as they did, the pane wider by the column,
and the tags where they were; right after the letter, `✓` in sync and `✗`
for a diff, a title or summary, or a parent that differs, listing files
and expanded alike, each patch's mark the one its Phabricator line says,
patch by patch, a revision two patches name marked for each, as a patch
changes and as a reordered stack is submitted again; a blank while what
differs is not read, no cell for the marks while no patch shows one and
one more once one does, the pane a cell wider, a revision named since
blank while the others keep theirs; none for a patch naming no revision
or one Phabricator does not have; `?` saying what the marks mean, and in
the reading mode that the `✓` at a line's end is another, nothing of
them while there is no column; the stack's revisions read in one search,
no other call made but the Phabricator lines', not again while they are
the same, again once its patches name others and right after a submit, a
new revision's status shown at once; Changes Planned shown as such though
Phabricator says it is a draft; a read that fails keeping those known and
saying so once, again only after one that worked; Phabricator turning the
machine away, two minutes asked: the bottom line saying until when, in the
place of what a read that fails says, the statuses and the lines keeping
what they had, nothing read while the wait goes on, though the stack's
revisions change, and a pull of one revision made all the same, nothing
read after it; a second asked, all read again once it is over, and a read
that works making the next refusal the first again, five minutes; none
read while reviews come from nowhere; built without moz-phab, none shown,
nor a Phabricator line), the Phabricator line (with moz-phab, over the message's header,
the cursor coming to the header: the revision alone and `…` before
anything is read; in sync; the diff differing, the title or summary, the
parent, and those together, listed plainly; a summary wrapped otherwise, every word the same, differing: Space
showing the lines of both versions, Phabricator's taken out and this
version's put in, no word in the colours for words, and saying so, `?`
saying that in sync is to the letter and what Space shows, the submit's
review showing them so too, and in sync once it is submitted; a patch
submitted and unchanged since in sync, Phabricator
giving back its title and summary as they were sent, white space ending
a line and all; a status stacked does not know, and a revision
Phabricator does not have; the comments to pull, none once a pull holds
them for review, nor once they are filed, one edited and one new once
the revision is read
again, none once their thread is closed; over the message in every view
of the code, in each half of the middle split in two, reading and with
each file whole; none over a patch naming no revision, nor while reviews
come from nowhere, nor without a stand-in in the tests' own process;
scrolled away with the rows, the title pinned in its place; Space on it
opening what differs on a screen of its own, its title naming both
versions and what it shows said first: the code as an interdiff of the
two diffs, what git derives in them left out, and the title and summary
compared where their lines wrap aside, what only each has marked in the
colours for words; Esc going back to the line; in sync, Space saying so,
the parent named, and opening nothing; not read yet, Space saying that; reading, Space
marking nothing read and Delete taking no mark back; Enter opening the
menu, titled for the revision, what differs and the pull of its reviews
first, and the bar and `?` saying what Space does and what the menu has;
the patch's menu in the patches pane offering both too, after its submit;
what differs grayed, the menu saying why, while it is not read yet, and
for a revision Phabricator does not have; in sync, the screen saying that
nothing differs, the parent with the rest, Esc going back to the patch; neither item while reviews
come from nowhere; a latest diff on another base, each file as each
leaves it, said, or the interdiff all the same, said too, where the
files cannot be read so; the reviews of one patch's revision pulled
alone, from the line's menu and from the patch's, that revision read and
no other, the banner naming it, one pull at a time, what it brings held
for review as a pull's, every revision read again after it, and the stack's
pull still reading every revision; every revision of the stack read as
it is shown, the parents and the latest diffs of all of them in one call
each, by their PHIDs, then each, that of the patch shown first, each
patch's line whole as soon as it is shown, nothing read for it, nor as
patches are shown again, all of it read again each round (the statuses
read again, a pull), a raw diff not read again while it is the latest; a
revision whose search says when it was last modified not read again
while that stays the same, only its parents and its latest diff read,
with the others'; one modified since read again, alone, its new comment
counted; one submitted, or pulled alone, read again, alone, though its
search says it was not modified since; a
read that fails keeping what was known and saying nothing more than the
statuses' read, a revision never read `…` but for its status; the
parent: a reordered stack's revisions differing by their parents alone,
nothing read for it, Space and the menu's item showing the parent first,
Phabricator's then the one a submit sets, and nothing under it, `?`
saying so, a submit setting it and nothing else, then in sync, its
parents read again with the rest, Space naming the parent; none
differing at the bottom of the stack, over a patch with no revision, a
landed revision, or a patch naming the same revision, Space saying why a
submit sets none as a submit does, no parent named; a parent that is
none of the stack's revisions named in a search of its own, where a
submit sets another; `…` while the revision below is not read; the
parent with the code, the title or summary, or both, listed plainly, the
view showing the parent first, then the code; the patch's menu submitting
as its revision's status has it: `Submit to Phabricator as WIP` while the
status is not read, and for a revision in work; grayed for one that
landed, the menu saying why; for one in review, the update of its title,
summary and parent alone in its place, grayed while what differs is not
read and compared, saying so, then offered for a summary that differs,
`?` saying what it sends, its review saying at its top that the code is
not sent and why, the status staying as it is, no code under it, Enter
sending the summary alone, no diff, no Changes Planned and no reviewers,
the bottom line saying what came of it, and once read again, the diff
alone differing, the update grayed, the menu saying why), and the comments pane's states (expanded,
compact, hidden) and the compact view of the code as they were. With the `web` feature, `tests/web.rs`
serves the editor and asks it over HTTP as a browser would: the token
needed, the page, the wasm, the rows as they change, a key and a click,
the wheel's ↑ on a patch, listing files, going to the last file of the
patch above, and the editor left, the page told, the worktree made ahead
removed (Quit in the page; `stacked web` itself sent SIGINT, SIGTERM or
SIGHUP, the first while `$EDITOR` runs left to `$EDITOR`, the second
waiting for it), and two
`stacked web` at once, each putting its database, named from where it
runs, on the list StackED's Open Recent shows, the ask dialog (opened with
nothing set from `Ask agent about all…`, clicked, its permission set by
clicks on its line and on an option of the list that opens under it, then
stepped by → and back by ←, its effort picked in the list Enter opens, ↑
and ↓ going round it, its lines set by → alone too, Ask Now grayed as
claude is not found, saying so, Ask Later putting the comments in the
batch; Esc closing it, a second one unset), the effort of `Send it
now`, picked there alone, ← → no further than `low` and `max` and a click
on it going round from `max` to `low`, the agent's turns headed with the
effort it was asked at and its landed work named with it, in the banner
and its review, a turn of before as ever, a message's long lines wrapped
from its patch's menu, the edit landing for review, what waits its turn
(a reconcile with HEAD, a lint
job, an edit ask) taken out of the queue from the banner with Delete, each
asked first, → opening the comments pane of a patch with none, saying why,
its menu there, and ← putting it away, a comment on a patch the agent's
work adds, and what you write in a thread the agent opened that it was
asked about, written in its review, listed with what you wrote and
sending the work back (Esc's `Send it back to the agent…` opening the ask
dialog, nothing set, Send Now and Cancel its buttons), Esc's summary in a
review sent with its count of
the tools refused in red, then of those allowed in green, the rest dim,
in the runs of the row the page draws, `Move HEAD…` on the stack's menu
not grayed while it would not move HEAD, and the view of why opened from
it, then left, a link of a line of a message opened from the
menu of a right click on it and with `o` (the status line saying it is
opening it while the stand-in is held, then that it opened it), and one
of a line of a comment from its menu, by a stand-in, and the comment
closed last on a line, longer than its box in a small page, scrolled
down to its last line with
↓, the wheel's ↓ and pgdn, and, with
moz-phab, the status of a patch's revision after its number, a letter,
then its mark (`✗`, its diff otherwise) on the screen its Phabricator
line says so, the next patch's title after the column all the same,
expanded and listing files, a title folded there, and the Phabricator line
over its message, Space on it showing what
differs on a screen of its own, Esc going back to the line, read by a
stand-in for its database, and the reviews of its revision pulled alone
from its menu, an item clicked, by a stand-in too; and
`web.rs` tests the rows and the lines both ways, with the client's own tests
(`web/client.rs`, compiled to the machine as well as to wasm), which also
test the mouse followed only while the page has the focus: the cell where
the mouse was last seen while the page did not have the focus said at once
when the page has it again (unless the mouse left the page), so that the
click that gives the page the focus back acts on that cell, whether the
browser sends the click before the focus or after. With
the `app` feature, `tests/app.rs` runs StackED's editor as its window runs
it, with no window: nothing drawn before the window's size, a key, a
click, ↑ on a patch, listing files, going to the last file of the patch
above, as the wheel's ↑ does, the mouse over a patch then gone, a window
too small, the wait while `$EDITOR` runs (an editor that edits nothing,
set in the test's settings), quit, the command there, a link of a line of
a message opened from the
menu of a right click on it and with `o` (the status line saying it is
opening it while the stand-in is held, then that it opened it), and one
of a line of a comment from its menu, by a stand-in, the comment closed
last on a line, longer
than its box in a small window, scrolled down to its last line with ↓ and
pgdn, as the wheel's ↓ do, and a database
that cannot be read said before any window; two editors in one process,
as two windows, each in its own
folder (HEAD said in the checkout's, none elsewhere); a message's long
lines wrapped from its patch's menu, the edit landing for review; what
waits its turn (a reconcile with HEAD, a lint job, an edit ask) taken out
of the queue from the banner with Delete, each asked first; → opening the
comments pane of a patch with none, saying why, its menu there, and ←
putting it away; a comment on a patch the agent's work adds, written in
its review, listed with what you wrote and sending the work back, Esc's
`Send it back to the agent…` opening the ask dialog, nothing set, Send Now
and Cancel its buttons; Esc's
summary in a review, its count of the tools refused in red, then of
those allowed in green, the rest dim, cell by cell of the screen the
window paints; `Move HEAD…` on the stack's menu drawn as any item while
it would not move HEAD, and the view of why opened from it, then left; the
ask dialog opened with nothing set from a click on `Ask agent about all…`,
Ask Now and Ask Later drawn dim, Ask Now as claude is not found, which it
says, a click on its permission's line opening the list of its options
and a click on one of them picking it, Enter on its effort's line opening
that line's list and Enter again picking low, and a click on Ask Later
putting the comment in the batch; the agent's turns headed
with the effort it was asked at, and its landed work named with it, in
the banner and its review, a turn of before as ever; with
moz-phab, the status of a patch's revision after its number, a letter,
then its mark (`✗`, its diff otherwise) on the screen its Phabricator
line says so, the next patch's title after the column all the same,
expanded and listing files, a title folded there, and the Phabricator line
over its message, Space on it showing what
differs on a screen of its own, Esc going back to the line, read by a
stand-in for its database, and the reviews of its revision pulled alone
from its menu, an item clicked, by a stand-in too; the File menu, its
dialogs answered by the test: Open… opening each database picked in a
window of its own (one picked twice, one that cannot be read said, the
dialog cancelled), New… making an empty database that stacked reads and
never replacing a file there (its bytes the same), and Open Recent taking
a database deleted off the menu and off the state kept, then Clear Menu;
and, elsewhere than on macOS, `stacked app --bundle` making StackED's
launcher in the test's own data folder (`$XDG_DATA_HOME`, else the test's
home's ~/.local/share, else the folder named), the file the desktop
reads, this stacked in its Exec line and its icon at every size, made
again, and something else of its name left alone.
`app.rs` tests what the window paints of a screen (the colours, reversed
and dim, the faces, wide characters, the lines of boxes and blocks), what
it makes of keys, clicks, the wheel (while a comment is written too) and
pastes, and the window itself, run
by egui with no window: the grid's size sent, the screen painted, what is
done sent, the wait for `$EDITOR`, and closing, and text dragged over lit
and copied, kept to its pane (a block with alt, a drag past the grid up to
its edge, a key letting it go), and the mouse followed only while the
window is the active one; and the fonts, the system's monospace font
where it has one (Menlo where macOS keeps it, its own bold and italics,
and Apple Symbols; DejaVu Sans Mono where Linux keeps it), else Hack, each
face holding every character stacked draws where a font for symbols is
found. `app/menu.rs` tests the menu bar, run by egui with no window: File
its only menu, its items with no shortcut beside them on either system;
ctrl-n, ctrl-o, ctrl-w and ctrl-q, and ⌘ with them on macOS, left as they
came, nothing picked; Tab, Enter and Space taking no item; an item
clicked once its menu opens, Open… asking the dialogs on their thread (a
stand-in's), Open Recent read as the menu opens, Close Window and Quit
StackED. `app/windows.rs` tests StackED's
windows, run by egui with no window as eframe runs them: the first's empty
with no database, its menu bar there, then a window for each database
opened (one twice), the first in the root's, each telling its own editor
its size under the bar and following the mouse only while it is the
active one, what is done in a menu open not sent the editor, ctrl-n,
ctrl-o, ctrl-w and ctrl-q sent the editor on either system and ⌘ with them
neither the editor's nor the menu's on macOS, no shortcut in the File
menu, egui's own text font drawing all the bar and the empty window show,
a window closed telling its editor to leave, the root's closed taking over
the latest other (its title, its place and its size), the last closed
letting StackED quit, its window kept, and Quit keeping the windows open,
from the File menu too, and waiting for each editor to leave. `state.rs` tests what is kept
between runs: the recent databases, the latest first, once each and eight
at most, cleared; the searches of the middle, the latest first, once each
and twenty at most, kept beside them; the effort a stacked of before kept
beside them passed over, the rest read as it is, and gone once the file
is written; those deleted taken off both lists; a file that cannot be
read; writers at once, each keeping what it wrote (with no lock, some
would not); a file of another kind (the null device) keeping nothing; and
the editor's database kept nowhere in a test's own process.
`launcher.rs` tests, on every system, when stacked is opened as macOS
opens StackED.app and as the launcher opens it, StackED.app's Info.plist,
the launcher's file (and what its Exec line escapes), the launcher and its
icon made in a data folder of the test's and made again, the terminal
found (the one `$TERMINAL` names, with its own words, else the first of
the list on `$PATH`, a file there that does not run passed over), and an
edit run in a terminal window, a stand-in for x-terminal-emulator given
`-e`, sh and the script, a terminal that fails at once said, and no
terminal found said, nothing run; on macOS, it tests that the Edit menu
put on the menu bar for the panels has the titles and shortcuts it has in
any Mac app, with actions that AppKit's text fields and windows answer. On
macOS, `tests/app.rs` makes StackED.app as macOS reads an app
(its Info.plist, its signature, its icon at every size, the stacked in it,
which runs), makes it again, and leaves alone something else of its name.
`bundle.rs` tests the login shell's environment read (a shell that never
ends given up, the shell the user's), an edit in a window of its own
waited for (in its folder, it failing, its window closed before it exits),
the editors with windows of their own, and the script a terminal is given
to run it (for its owner alone, removing itself as it runs the edit,
removed if the terminal cannot be opened); `refs.rs`, that what is not a
web link is never handed to the system's way of opening links;
`icon.rs`, what the icon shows where, at its largest (the sheets' edges)
and at its smallest (a plain white S), and that it is grey all over. With
the `demo` feature, `tests/demo.rs` takes the tour, each step done as Tab
does it: the
stand-in's fix, the board at its end, nothing made on disk; the answer
to the tour's question taken at once with Enter, the tour going on then;
and the views taught with 3, picked in the menu of 1 and making their
room on a middle too narrow, the step waiting for the hunks again;
`memory.rs` tests its diffs applied as git applies them. With the
`try-push` feature, `tests/try_push.rs` drives the editor as
`tests/tui.rs` does, a stand-in of its own for claude: a comment asked as
a try push, `try push` among the comment's choices of how, its ask in a
worktree of its own, your checkout as it was while it works and after, not
given the checkout's linter as a change is, its prompt
asking for the tasks the comment asks for, as few or as many as it says,
exact labels, why each, nothing pushed or changed; its tasks landing in
the banner as read-only work, listed in the review, why each was selected
shown as the agent's comment, marked by Enter and from the menu, answered
there and followed up as a try push, the work staying for your review,
the answer yours after the agent's reply as the thread would be, the
thread itself as it was, the follow-up told what was selected before as
you marked it, and its tasks coming into the same review, but the one the
work held already, which keeps its mark; closing with tasks accepted
saying what it pushes,
with none accepted that nothing is pushed, and past mach's 1,000 tasks,
each task counted as many times as it runs, that they would run at the
lowest priority; the first try push going at once, with no setup, the
setup's old mark passed over; and a try push batched, sent from the banner on
its own, and with the batch that `stacked send NAME --batch` sends. The
push runs a stand-in for mach (`$STACKED_MACH`, which a stacked that keeps
nothing, as the tests' own, needs, never running a mach of a worktree's),
the repository's remote of Firefox's a name and a ref, never fetched: the
push checked with `--no-push`, then made, mach's stdin closed and its
output not held back, the tasks in one query, the commits it would push
the patch alone; its login's page and code in the status line and the
comment's stream, the page opened from the comment's menu; its link a
reply, a web link, the comment locked until you stop following it, which
says so with the link; past the threshold, both runs given the preset of
the lowest priority from a presets file of stacked's own, gone once the
push is done; a check that would push another task, a question mach asks
(quoted, with what to change), and a push that fails (mach's last lines)
each said in a reply, nothing pushed, the comment unlocked; a push
stopped as mach waits for the login; a base none of the remotes of
Firefox's has, or no remote at all, keeping the review open, mach never
run. The push followed answers to a stand-in for Lando and Treeherder
(`try_push::web_stand_in`, which a process that keeps nothing, as the
tests' own, needs, never reaching the web): Lando's state of it in the
banner, then its tasks as Treeherder shows them, in the banner's section
of try pushes and its first line, and in the stream, the pages read as
Lando's and Treeherder's APIs have them; done, a retry counted by its last
run, its summary held for review (the counts by result, each task that
failed with its link, what the push ran for them that failed, each a web
link), taken from the comment as an answer only's is, the comment
unlocked; the tasks Treeherder never shows counted as not seen once
nothing runs any more; a push Lando did not push to try summed up so; and
a push followed picked up by the next editor from where the database has
it. `try_push.rs` tests its tasks read from the answer (each label once,
the first reason kept, one with a space or none left out, a reason with
references), how many times each runs, from 2 to 20, the prompt, and what
a round asked again is told; mach's arguments, the tasks in one query, and
how the stream says them; what `--no-push` says it would push, read and
compared (the tasks, the runs, the priority); a question mach stopped on,
its login, its link and Lando's job read from what it writes; that a
process keeping nothing runs only a stand-in for mach, and reaches
neither Lando nor Treeherder; the remotes mach takes as Firefox's (on
GitHub, and hg.mozilla.org's but try in a git-cinnabar clone), a base none
of them has, and the commits mach would push; and a push's tasks counted as
Treeherder shows them (each by its last run, each run of a rebuild, those
not seen), its summary, and where Lando answers, as `.lando.ini` says;
`ask.rs`, that a try push's ask is given what an answer is and its own
list, not the linter a change is given, and is asked for its tasks and
their runs; `setup.rs`, the column of
try pushes, ticked at first as the read-only one, its own once ticked, in
what saving writes, and line by line `read only` for both read-only kinds;
`refusals.rs`, what changes things noted for a try push's selection, as
for an answer, and what can write files or run programs, what bundles its
short options, and what stacked refuses it as an answer, too;
`config.rs`, `try_tools` written and read back, and read built without
the feature too. `tests/lint.rs` drives the
editor as `tests/tui.rs` does, a stand-in run in place of the linter
(`lint::stand_in`, `$STACKED_LINT` to the job's process, which keeps
nothing and so runs no other) that takes out the white space lines end
with and reports the lines it cannot fix, exiting with 1, and a stand-in
for claude: `Lint the stack` on the stack's header alone, grayed with why
where there is no linter, where lint is off, while an edit by hand waits
for review, and on a read-only stack, and named once set; a checkout with
`./mach` at its top linting with `./mach lint --fix`, the stand-in given
mach's arguments, nothing written; each patch linted at its place, on its
files, in the stack's lint worktree, its stdin closed, the checkout not
touched, each fix folded into the earliest patch it shows in, the change
landing for review as one, named so in the banner, the review and the
history; a fix that would keep the patch above from applying stopping the
patches' run, the linter run once at the top, its fixes in a patch at the
end with your comment on it, ready to send and sent to no one; nothing
landing when the linter fixes and reports nothing, said quietly; what it
reports listed in the review and by `stacked landed`, and landing alone,
read only, to read and close; an edit asked while the linter runs
waiting for it, the stack not edited by hand meanwhile (the menus and the
commands saying why), an answer going at once, then the edit going on from the stack the linter made;
one lint job at a time of every stack, the next waiting its turn; a job
whose process is gone landed as failed, the stack free again; and a
linter that hangs stopped from the banner, asked first, and by `stacked
stop`, the job landing as stopped with nothing landed and the stack as it
was, the stack free again and the edit that waited for it going, and a
job waiting its turn dropped the same way, asked first, by Delete on its
line of the banner and from the banner's menu, where it is next to `Read
what the linter writes`, and the stack's, the edit asked after it going
then while another stack's linter still runs.
`tests/cli.rs` sets the linter up: in the checkout's own settings alone,
kept out of git's sight, never in the settings' file, which is passed over
and said if it sets one; printed with where it comes from; turned off,
and that kept over the default of `./mach`; `stacked config` refusing to
write it but reading it; refused in a settings file git tracks and outside
a checkout. It shows a change, of the stack or of a patch, given the
checkout's linter, with `./mach` at its top or one set over it, as the
rule a refused call of it is offered with, after what stacked gives and
before what the settings add, and its prompt naming the command; an
answer and a build and test not given it, nor a change with lint off or
no linter; and `stacked setup agent` naming it, or saying the checkout has
none. `tests/web.rs` and `tests/app.rs` show the item in the
browser and in StackED, and also that a reviewer's edit is headed by what
it edits, the turn it edits redacted, and that a pull's review rejects
nothing there either. `lint.rs` tests a command split as a shell's
words and written back, only a stand-in run from a process that keeps
nothing, what the history says of the fixes, a patch's sections spliced
with those written again, and the patches' run itself on a repository of
its own: each fix in the earliest patch it shows in, a fix that would keep
a patch above from applying stopping it with the rest in a patch at the
end (its message wrapped), a clean stack left as it is with what is
reported kept, and a fix that adds a file going at the end. In
`ask.rs`, an answer and a try push's selection are given the commands
that only read and git's that only read, `Grep` too, not `sort`, `file`
nor `git grep`, whose short options can be bundled, nor `sed`, `awk`,
`uniq`, `tree` or git at large; they are refused the options that write a
file or run a program, long or shortened (`git log --output`, `file
--compile`, `git grep --open`), none that only reads, and none twice. A
change is given what it was, and refused, as every ask but a build and
test, `rg --hostname-bin` and `sort --co` too; a build and test is refused
nothing. The
stream's thinking is tested in `ask.rs`, with the prompt asking the agent
to point at the stack before the web and its answer's schema taking a web
link, the ask's effort given to claude after the settings' arguments, in
place of theirs, a child process that has exited not taken for alive
though `kill` still finds it, its exit left for whoever started it, the
wrapper of an ask the process started waited for once it has exited, and
an ask whose agent you stopped landing as stopped, unless the agent was
done as you stopped it, and the thread the agent is told, a reviewer's edit
and the turn it redacts in plain words; what is said of landed work left
to review is tested in `held.rs`, with a reviewer's edit as their latest
turn, the turn it edits redacted, again on a later edit, nothing redacted
when that turn is gone; and a reviewer's reply to a comment you closed
reopening it once accepted, as the review closes, taken at once or sent
back with the agent's work, as the pull kept it, as its history has it
last when written since, filed where it is open since rather than
reopened, read from a checkpoint without its history read, while the
agent's goes nowhere and a pull's parts are not rejected; and what a
follow-up brings back coming into the review of the work it follows up:
an answer's reply after the work's, your answer to the reply before it
gone with it, the thread as it would be holding each in turn, the tools
it could not use but those the work holds; a build and test's reply to
read, no part, what it found of each patch with the follow-up; nothing
from one failed with nothing to bring, nor once the work's review is
closed. Those in
`thread.rs` show the fences of a reviewer's edit and of a turn redacted
read and written again, their headings, the first line an edit quotes cut
short with `…`, and a name alone as ever; and the effort on the fence of
the agent's turn, after the way, read, headed and named with it, kept as
the thread is written again, a reply added, a turn edited or one holding
a code block, a turn of before read and written as it was, a comment the
agent opened at an effort, and a reviewer named `effort` a name as ever;
the stack an answer was asked on, last on its fence, after the effort or
alone, read as the way and the effort as ever, kept as the thread is
written again, a word after `stack` that is no rev read as part of the
way, and how old that stack is said; in `ask.rs`, the thread the agent is
told says none of it. The
tests in `check.rs` show
the check's index holds only what the patches touch, that it writes nothing
to the repository (nor does the tree a patch leaves, written for a
submit), how a patch's files are read before and after it, and where a
diff of the same effect as its draft does otherwise (git applying
it at another copy of its lines, it not applying), the drafts not
applying an error;
those in `git.rs`, that stacked's looks at a checkout, its worktree or
yours, take no lock of its index (a file touched as it was, the index
left as it was, where git's own status writes it again);
those in `ops.rs`, that two code edits saved at once, on two threads, each
make their diff in a folder of their own and leave nothing in the
temporary directory, which diffs have the same effect (a hunk moved down
its file, under another heading or none, other blobs, a rename's
similarity, a header's line ending in a tab or not, an empty line in a
hunk for an empty context line) and which do not (another line added,
removed or kept, a file more or fewer, the files in another order, a mode
changed, a hunk split in two), which diffs make the same changes (more
context, as `-U5` and `-U10` write it, merging the hunks; a hunk split in
two or merged, starting elsewhere, under another heading, with other lines
kept around it; an empty line in a hunk for an empty context line; the
files in another order; more context reaching the end of a file that has
no newline there) and which do not (another line added or removed, white
space a line added ends with, a file more or fewer, a mode changed, a line
added with no newline at its end or with one, a run of changed lines split
in two by a line kept between them, or split across two files otherwise),
and that a draft with its patch's message,
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
by line as they were; and compared to the letter, as the view of what
differs from Phabricator compares them, a paragraph only wrapped
otherwise each version's lines, nothing of them marked, but where a word
changed too, as ever; those in `submit.rs`, that a latest diff with ten
lines of context, as moz-phab sends it, is the patch as it is on the base
it was made on, and nothing is sent; that once upstream has changed a line
within those ten and not within three, and the stack is rebased onto it,
the diff is known by its changes alone, never as the patch as it is, the
review saying why, and a new diff is sent, which a submit after it finds
to be the patch as it is; that once the patch is edited too, its code is
compared from the version whose changes the diff makes; that a latest
diff of other changes is still refused, nothing written; that a revision
in review, accepted or needing revision is sent its title and summary
alone in one edit, `WIP: ` first only where its title has it, the
reviewers its title names kept and none read, its status kept, no diff
made and its repository not read; its parent alone, in an edit of its
own, when only that differs; and nothing when only its diff differs, said
so, its latest diff left as it is whatever version it is, one known by
its changes alone with no new diff said to be sent, nor once it changed
on Phabricator since it was read, retitled or accepted; and that a
revision landed or abandoned is refused, nothing read but the revision and
nothing written; those
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
with what it lets an answer do; the rules the reviews allowed from now on,
a row each, unticked and ticked again, what saving takes out of their
list; the linter changes are given, named where the checkout is known,
with the rule that lets it run, or why none is given (none, lint off, no
narrow rule), on the screen and line by line alike; and the setup asked
line by line, a question a row for every kind.
Those in `refusals.rs` show a command line cut into the commands claude
matches apart, quotes and `$(…)` kept whole; a command's rule, its program
for a plain one, narrower for a broad one with the wider ones to pick,
what claude passes over first passed over, none for a shell's own; npx's
package and cargo's subcommand, past the options before them, kept as
written so that the rule lets the call, and the wide rule alone after an
option not known or with nothing after the options; the narrowest, none
for a broad program asked nothing narrower; the rule that lets a change
run the checkout's linter, the one a refused call of it is offered with,
which lets each call of it on files run, and not another package of
npx's nor another subcommand of cargo's; a call of it that a run of
before was refused, not given it, an item offered with that rule, npx or
cargo alone offered with the wide rule only, and one refused though given
it noted as allowed already; how a
rule lets a call (the older prefix form, a pattern, an MCP server's tools,
WebFetch's domain); the calls refused as items, one a rule, each with its
calls and the agent's word, a pipe's command given already left out; what
is noted (denied, refused by stacked, changing things for an answer, given
already), and that one call that needs it offered offers it; that an
answer is offered no rule of what bundles its short options (`sort`,
`file`, `git grep`) nor of what can write files or run programs whatever
the call (`sed -n`, `awk`, `uniq -c`, `env`, `xargs`, `python3 -m`, git at
large, what git does that changes things or does not only read), each
noted, nor such a rule to widen to, what stacked refuses it alone noted
as that (`git diff --output`), and what it refuses every ask but builds
and tests as that (`rg --hostname-bin`), while a change is offered each,
with its wider rules, as before, but for `rg --hostname-bin` and `sort
--co`, now refused it; that a command an answer is given, `rg`, is no
item where another command of its pipe needs one, and alone is noted as
allowed already; that a run of before, not given `git blame` nor `rg`, is
offered them, `git blame`'s rule never widened to the whole of git, and
`git grep` noted all the same; the three
states and the rule picked in the review's marks, and the turn the agent is
told; what the agent said of each tool, read from its answer; and what is
saved from now on, where, and why not outside a checkout. Those in
`search.rs`, which lines of a file a hunk shows
already; those in `rows.rs`, a web link's line under its comment cut to
the pane, its start kept, each row of what a comment says holding the
line it draws when that is prose (each piece of a line folded the same,
none in a code block nor on the comment's heading, a comment collapsed
the first line it says), each file whole keeping the rows of its hunks'
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
the width messages are wrapped at, one too narrow refused, the Bugzilla a
bug opens on taken when it is a web link to a site, else said and not
written, the keys listed by the context they are read in, and the sync's
keys of before said to be gone where a file names them, refused to
`stacked config` and taken out by its `--unset`; those in
`wrap.rs`, a message's
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
what the search of the middle keeps for itself refused to its own,
the sync's keys of before, keep_stack and keep_workspace, said to be gone
and passed over, and o, the key that
opens a link, read on the screens, made another key, and clashing there
alone; those in `links.rs`, the links of a line of a message: its web
links as written, less what ends a sentence or a bracket around them,
none with no site, with another scheme or with a word running into it;
a markdown link named by its text, its markup left out (a text that is
a link, or holds one, said as the link it goes to), its link up to the
`)` that closes it, its own brackets kept, a title after it not of it,
two with nothing between them two, none to no web link; one between
angle brackets whole, none with a space in it;
with moz-phab, the bugs it names, `bug` a word of its own (none in `debug
7`, which a title submitted still reads as moz-phab does), the rest as
moz-phab reads them, none in a web link nor in a markdown link's text,
on the settings' Bugzilla, and the revision its trailer names, at
its link or bare on the settings' Phabricator, as the settings name the
trailer; without moz-phab, its web links alone; those in
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
those in `head.rs`, which commits are marked, `!` for one that names a
revision its patch does not, whatever else differs, and `≠` for a
message other than the patch's, empty lines in a row one as `git am`
leaves them, what `stacked head` says of each, when Move HEAD replaces
commits (as stacked left them, or on the stack with no mark) and why it
does not, and that it keeps every commit up to the last one marked `!`,
and those after it that are their patches, HEAD going nowhere below it;
and, without moz-phab, the trailer compared as any line;
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
a test's own process. Those in `phab.rs` show a revision's status read from
its value alone, whatever Phabricator says of drafts, with its letter and
its name; the statuses of revisions read in one search, those it does not
find not known, each with its title, summary and PHID, and a search
refused said; an editor's read by the stand-in for its database alone, one
search for each Phabricator the links name, never from a process that
keeps nothing without one; and what the Phabricator lines read of the
revisions, in a few calls: the parents of all of them, and their latest
diffs, the newest first, each in one call, page after page, the diffs
until each revision has its newest; the parents that are none of them
named in one search, only of the revisions a submit sets one, a parent it
does not find left unnamed, none searched with none to name; then each
revision in the order given, its diff raw unless it is the one read
before, its comments from every page of its transactions, their authors
and lines not read; a revision with no diff passed over, and nothing more
read after a call that fails; a revision not modified since it was read,
its latest diff the same, not read again, but for its parents, and one
modified since, given a new diff, never read, or whose modification is
not known or was forgotten, read as before; by the stand-in for the
editor's database, the calls of each Phabricator on their own, and a read
turned away said as such, whatever its calls make of it. They show curl
given the token and the call's parameters on its standard input, none of
them in its arguments; a call turned away (HTTP 429) an error of its own,
which says so, with the wait Phabricator asks for, in seconds or until a
date, or none, after an answer that says to go on too; another status
said with the first line of its page, cut short, or that the page is
empty, as a page that is not JSON is; Conduit's own error and curl's said
as before; and curl itself, to a server on the loopback, sending the
token in the form of its request, and saying a 429, a gateway's error page
and Conduit's answer. Those in `review.rs` show the comments not
pulled yet counted from the comments open, the threads by their ids and
the pulls waiting: those filed or waiting not counted, one edited since
it was filed counted as edited, a thread closed not counted, nor a new
reply or edit there, but for one moved to the stack's comments, another
stack's pull not this one's, and one a review of before rejected counted,
as a pull brings it again; and where a pull is said, one patch's revision
pulled alone too. Those in
`tui/revisions.rs` show when the statuses are due a read: never read, other
revisions, a submit ended since, or three minutes gone by; how long
nothing is read once Phabricator turns the machine away: as long as it
asks, two minutes say, else five minutes, then ten, twice as long each
time in a row up to thirty, a time it asks taken as it is and counted,
and five minutes again once a read worked; a title in work
as moz-phab reads it; the title and summary compared to the letter as a
submit sends them, in work or not as the revision's title is, the white
space at their end aside, their lines wrapped otherwise or other white
space in them differing; what a submit sent, given back as it was, in
sync, its reviewers, its trailer, white space ending a line, a fenced
block or carriage returns and all; a title and summary only wrapped
otherwise, or only spaced otherwise, showing the lines of both versions,
nothing marked, said so, and one with a word changed too showing the
paragraph wrapped otherwise once; a title and summary only wrapped or
spaced otherwise told from one with a word changed; a diff bare of what
git derives in it, as Phabricator's raw diffs are, the lines of its hunks
as they are; two diffs on other bases told by the files they read before
the patch, a line otherwise at the same place, the same hunk elsewhere, a
file one makes and the other finds, but not by more of a file around a
hunk, nor by another file; the tour pulling nothing from Phabricator, a
patch's revision alone neither; the comments to pull and what differs
said in plain words; the patches pane's mark `✓` exactly where the line
says in sync and `✗` wherever it says what differs, the parent set,
already its parent or none set, and none while that is not known or for a
revision not found; and the line folded between words, its pieces under
its first. Those in `tui.rs` show, among others, a line of a menu's note
drawn in parts folded at the spaces its text folds at, each character in
its part's colour, but never between a mark and the count after it.

## Not done yet

- Turns deleted by stacked's first way of deleting one (```` ```deleted ````)
  are still read, only to be left out. Once no database holds them, that
  code goes, and with it the split of a thread's turns between those that
  stand and all of them.
- A stack-wide review runs one agent; one per patch is not done.
- An agent other than claude must speak claude's command line and stream,
  and take `--json-schema`, unless `agent.schema` is off.
- Only git mail is read: no `hg export`.
- A patch is submitted to Phabricator one at a time, as work in progress
  alone (Changes Planned), and so with no reviewers; a revision in review
  gets its title, summary and parent alone: a stack at once, a revision
  sent for review with the reviewers its title names, a new diff of a
  revision in review, a file that is not text, and each file whole as the
  diff's context (so that reviewers can see more of it) are still
  moz-phab's, once `stacked head NAME --to N` (or `Move HEAD…`) has put
  the stack in your checkout.
- A database is not opened by dropping it on StackED's icon, nor by
  double-clicking it in the Finder or in a file manager (the launcher names
  no kind of file), and there is no Window menu. Its text is not yet a
  setting: it is Menlo on macOS, DejaVu Sans Mono on Linux (else Hack), at
  14 points before ⌘+ and ⌘−, which change it in every window at once.
  The editor the settings name for
  `$EDITOR` is read where StackED runs (where `stacked app` was started,
  the home folder opened as an app), not in each window's checkout.

## License

MIT: see [LICENSE](LICENSE).
