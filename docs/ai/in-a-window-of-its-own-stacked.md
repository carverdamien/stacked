# In a window of its own: StackED

`stacked app` (built by default, with the `app` feature:
`--no-default-features --features demo,moz-phab,try-push,web` builds
stacked without it) opens StackED, the editor in a window of its own, with
no terminal and
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
the trackpad do what the browser's wheel does: while the mouse is in use,
they scroll what is under the mouse, and after a key they are ↑↓ where
the cursor is. They turn over the cell the mouse was last seen over, a
line for each 40 points a trackpad turns; a wheel that turns in pages
scrolls by pages, a page being the height of what it scrolls less a line,
and is pgup and pgdn after a key. Each window
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
the cursor does not follow the mouse while its button is down. A drag
that starts on HEAD's row (`▶ HEAD`) selects nothing: it drags HEAD, as
in the browser (above), each cell the mouse goes over with the button
down sent to the editor, then where the button is let go; the editor
tells the window where HEAD's row is with each screen it sends.

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
editor its keys, clicks, wheel and size (`app.rs`). Each window is a viewport of
eframe's (`app/windows.rs`): the first is eframe's root viewport, which
eframe cannot do without, and the others paint themselves. The first
window closed while others are open takes over the latest of them, its
editor, its title, its place and its size; on Wayland, which lets no
program place its windows, the window that stays shows up where the closed
one was. The menu bar is egui's (`app/menu.rs`), the dialogs rfd's
(`app/dialogs.rs`), and what StackED is to each system (StackED.app, its
launcher, the terminal `$EDITOR` runs in, and the Edit menu macOS's panels
need) is in one module (`launcher.rs`).
