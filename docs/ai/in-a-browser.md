# In a browser

`stacked web` (built by default, with the `web` feature:
`--no-default-features --features app,demo,moz-phab,try-push` builds
stacked without it) serves the editor on this machine and opens the
browser on it: the
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
elsewhere could otherwise send it keys. The page's tab shows StackED's icon
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
the page sends its keys and clicks back. Keys that come together are drawn
once, after the last, and each is done on the screen as the keys before it
left it, as in a terminal, which draws the screen after each key: Home,
then Space at once, opens the stack's menu with the stack's comments to
ask about, as a right click on the stack's header does. What the page does
is wasm
(`web/client.rs`: the rows made HTML, the keys and clicks made lines for
the server), carried by fifty lines of JS (`web/index.html`).
