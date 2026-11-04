# In a browser

`stacked web` (built by default, with the `web` feature:
`--no-default-features --features app,demo,moz-phab,try-push` builds
stacked without it) serves the editor on this machine and opens the
browser on it: the
same editor, the same keys, drawn in the page as in a terminal. Over the
side panes, the patches and the comments, and over the lists (the
review's; those of the screens of their own, History…'s, a record's
comments with it, the tree's, the stacks', the runs' and the versions';
what the search found, and the list of the view of why Move HEAD does not
move HEAD), the mouse marks the line under it, underlined, and picks
nothing: what is picked stays so (reversed in the pane the cursor is in,
grey in the other), and so does what the pane beside shows of the line
picked, until a click picks the line, and a second click on it is Enter.
A line marked on one screen is marked no longer on another, until the
mouse moves there. Elsewhere the cursor follows the
mouse: what is under it is picked, as the arrows would (a line of the
patch, an item of a menu; a patch being moved goes there), so
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
moves nothing. A paste types what is pasted, into a comment being written.

HEAD being moved goes under the patch the mouse is over (under the
stack's header, at the base), and a click on HEAD's own row (`↕ HEAD`)
puts it down there, as Enter does; a click on the patch it is under does
too. HEAD's row (`▶ HEAD`) can be dragged: pressed, then moved over
another cell with the button down, it moves HEAD as `Move HEAD…` does,
which shows why in a view of its own when it would not move HEAD. HEAD
then follows the mouse over the patches, as when it is moved with the
keys, and letting go of the button over the patches puts it down where it
is; over a place below its last commit marked `!`, the bottom line says
why, as for a click there, and HEAD is still being moved. Let go
anywhere else, or after Esc, HEAD is left where it was. Pressed and let
go on HEAD's row with no drag, it is the click it was, done as the button
is let go: HEAD's row picked, a second time its menu. Nothing else is
dragged: a press anywhere else is a click at once, and the mouse moved
with the button down is the mouse going by.

The wheel scrolls what is under the mouse while the mouse is in use: from
the mouse going over another cell, or a click, until the next key, a key
typed into a comment too. The view of the pane or of the box under the
mouse moves by the lines the wheel turned, a line for each notch, and
stops at its first and its last lines, where the arrows stop it. In
Firefox, each notch of a mouse's wheel is a line, whatever the lines
Firefox says for it; a turn said in pixels, a trackpad's and any wheel's
in Chrome and Safari, is a line for each 40 pixels. Where the
cursor follows the mouse (the middle, the half of the middle split in two
under the mouse, the banner, a menu longer than its box, History…'s stack
diff), the cursor goes on the line then under the mouse. A side pane, the
patches or the comments, and a list (above) keep what they picked, in
sight or not, and the line then under the mouse is marked. So does a pane whose cursor the mouse does not
move: the review's pane on the right, and what a submit sends. A text, a
transcript, a reference's preview, and what `?` says when it is taller
than its box, have no cursor: they only scroll. Each place stays where the
wheel left it until the next key, which brings what it picked back into
sight. The box of a comment being written scrolls when its text is taller
than the box, its cursor kept in the box, on the nearest of its lines. Out
of that box, as the mouse going by moves nothing there, the wheel does
nothing; nor does it out of a menu, of what `?` says or of a reference's
preview, nor while the ask dialog is open or a line is typed, nor over
what does not scroll, such as the top bar or the bottom line. While a
patch or HEAD is being moved, the wheel scrolls the patches alone, what is
moved going under the mouse; while the agent's stream is read in place,
the wheel over the middle scrolls the stream. After a key, until the mouse
moves or clicks again, the wheel is ↑↓ where the cursor is, a line for
each notch, as those keys are.

The page follows the mouse only while it is the
active one: while another window or another tab has the focus, the mouse
going over the page marks and picks nothing, and the line it marked is
underlined no longer. A click and the wheel act on the page all the same.
When the page has the focus again, what is under the mouse is marked or
picked at once, if the mouse went over the page in the meantime and is
still there. The click that gives the page the focus back thus acts as any
other click does, whether the browser sends the click before the focus or
after: a click on a line of the patch is Enter on that line. `--port N`
listens on port N for that run, any free one for 0; without it, the
settings' `web.port` says which (Settings, below): any free one each time
unless set, the one `"fixed-random"` took the first time and wrote in the
settings, whatever the database, or the one it names. A port in use is
said in a line, `port 8080 of 127.0.0.1 is in use`, and `stacked web` does
not start, trying no other. `--no-open` only says where it is.

It listens on 127.0.0.1 only, and only a page holding the token in the
address it opened (a new one each time, whatever the port) may see it or
drive it: a page from elsewhere could otherwise send it keys. The page's
tab shows StackED's icon
(In a window of its own: StackED, below), 32 pixels square, or 64 on a
screen of high density, each size drawn once and written as a PNG by
stacked itself, with no eframe (`icon.rs`); the page asks for it with the
token, as for all else, and `/favicon.ico`, which a browser may ask for on
its own, without the token, is refused. The editor runs where `stacked web`
was started: `$EDITOR`, for what is edited in it, opens there. Quit is
quit, and the page says so. A ctrl-c there, a kill, or that terminal
closing quits too. Either way, as when the editor in a terminal is left,
the agents' worktrees made ahead are removed, and the asks still running
are listed there; and `stacked web` ends once each page is told (a few
seconds at most, for a page that reads nothing). A ctrl-c while `$EDITOR`
runs there goes to `$EDITOR` alone.

The server runs the editor as the terminal does, drawn into a buffer of the
page's size, and sends the page the rows that changed (server-sent events);
the page sends its keys, its clicks and its wheel back, the wheel with the
cell under the mouse and the lines it turned, and with the left button
down, each cell the mouse goes over, then the cell where the button is
let go. Keys that come together are drawn
once, after the last, and each is done on the screen as the keys before it
left it, as in a terminal, which draws the screen after each key: Home,
then Space at once, opens the stack's menu with the stack's comments to
ask about, as a right click on the stack's header does. What the page does
is wasm
(`web/client.rs`: the rows made HTML, the keys, the clicks and the wheel
made lines for the server, the wheel's small turns in pixels kept until
they make a notch, 40 pixels, and each turn in lines or in pages a notch,
whatever its size), carried by fifty lines of JS (`web/index.html`),
which reads the wheel's mode before its turn: Firefox says a mouse's
notches in lines only to a page that reads the mode first. The
editor knows whether the mouse is in use, and what the wheel scrolls
under it (`tui/wheel.rs`), as it does for StackED; and what a press on
HEAD's row is, a click or a drag (`tui/drag.rs`).
