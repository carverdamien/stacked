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

A key types what the keyboard makes of it, as in a terminal. On a Mac, a
character Option makes is typed as it is, as Terminal.app types it by
default: ∂ for Option-D, `[`, `{`, `|` and `~`, which French and German
keyboards type with Option, `@` on a German one, and `…` and `€` on any;
in a comment, in a search and in the editor run in the page alike, which
is given the character alone, with no Esc before it. Option is alt only
where it makes no character, the key typing its own (d for Option-D),
which the page tells for a letter or a digit by where the key is on the
keyboard; on any other key, what Option types is its own. Elsewhere, alt
is alt, as terminals take it, Esc first for the editor run in the page;
and a character AltGr makes is typed as it is, without the ctrl and alt
that Windows says AltGr with, which stay where AltGr makes none. A dead
key (Option-E, before the letter it accents) types nothing: the browser
keeps it, and the next key types the character the two make (é for
Option-E, then E). With a key that types no character, the arrows,
Backspace, Enter and the others, Option and alt are alt, as ever, and the
editor run in the page is given xterm's forms for them (`ESC [ 1 ; 3 D`
for Option-←): in a comment, where Option-B and Option-F type characters
on a Mac (∫ and ƒ on a US keyboard), Option-← and Option-→ go a word back
and on.

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
address it opened (a new one each time, but with a port the settings fix,
below) may see it or drive it: a page from elsewhere could otherwise send
it keys. The page's
tab shows StackED's icon
(In a window of its own: StackED, below), 32 pixels square, or 64 on a
screen of high density, each size drawn once and written as a PNG by
stacked itself, with no eframe (`icon.rs`); the page asks for it with the
token, as for all else, and `/favicon.ico`, which a browser may ask for on
its own, without the token, is refused.

A terminal editor runs in the page itself, unless the settings say
otherwise (`web.editor`, Settings, below): `$EDITOR`, for what is edited
in it (the settings' `editor`, else `$VISUAL`, else `$EDITOR`, else vi),
runs as the terminal runs it, the same command on the same file in the
same folder, but in a terminal of `stacked web`'s own, a pseudo-terminal,
in a session of its own whose controlling terminal it is, told
`TERM=xterm-256color`, and neither `COLUMNS` nor `LINES`, which would say
another size, its size the screen's, the last a page sent (`pty.rs`). Its
screen takes the place of stacked's in every page, nothing said over it:
what it writes is read on a thread of its own and made a screen by
vt100, as a terminal shows it, each cell's character, a wide one over two
cells, its colours (the page's own, one of 16, of 256, or RGB), its bold,
dim, italic, underline and inverse, and the cursor where the editor puts
it, none while it hides it; and that screen is sent the pages as
stacked's own is, the rows that changed, about 60 times a second at most,
the first time whole: its size, then `t`, which tells the page that the
screen is the editor's until the next size, then its rows. A page that
comes meanwhile is sent it whole at once. An editor that writes nothing has
its blank screen shown after a quarter of a second, so that the page shows
where its keys go. What the editor asks of its terminal is answered as
xterm answers it: what terminal it is, a VT100 with advanced video (DA, as
libvterm answers it), and, asked for its version, libvterm's (`>0;100;0`),
by which vim reports the mouse as SGR does; whether it is well and where
its cursor is (DSR); and its colours, the page's own (OSC 10 and 11), by
which an editor tells that the page is dark. Nothing else is answered.

What the pages send meanwhile goes to the editor, made the bytes xterm
sends: a character as UTF-8, ctrl with a letter its control byte (and
with `@ [ \ ] ^ _ / ?`, space and 2 to 8 the one xterm gives), alt as Esc
first (a character Option or AltGr made comes without alt: above); Enter a
carriage return, Tab a tab, Shift-Tab `ESC [ Z`, Backspace
DEL (BS with ctrl), Esc ESC; the arrows, Home and End as `ESC [ A`, or as
`ESC O A` once the editor asks for the cursor keys' application mode,
Insert, Delete, Page Up and Down and F5 to F12 as `ESC [ 2 ~`, F1 to F4 as
`ESC O P`, each, with a modifier, as CSI with xterm's parameter for it
(`ESC [ 1 ; 5 A` ctrl-↑, `ESC [ 15 ; 2 ~` shift-F5). While the screen is
the editor's, the page sends every key it has, ctrl ones included (ctrl-c
is the editor's then, not Quit), and F1 to F12 and Insert too, which are
the browser's otherwise; those with ⌘ stay the browser's, for copy and
paste. A paste is its text, sent as it is (`p HEX`, its UTF-8): each of its
new lines a carriage return, as xterm pastes them, every control character
but a tab dropped, Esc among them, and bracketed (`ESC [ 200 ~`, then `ESC
[ 201 ~`) when the editor asks for it. A new size resizes the editor's
terminal, which the system tells the editor, and its screen. The mouse is
the editor's only once it asks for it (`:set mouse=a` in vim), and only
what it asks for, presses, releases, the mouse moved with a button down or
anywhere: a click pressed, then let go, the right button let go at once,
as the page does not say when it is, the wheel a press of its button for
each line it turned, the mouse moved with the left button down, and the
mouse going by; reported as SGR's (`ESC [ < 0 ; X ; Y M`), else in xterm's
first encoding, as the editor asks. Else the mouse does nothing there.

When the editor exits, its terminal is closed, and the server sends each
page the screen whole, stacked's own again; the bottom line says what
came of it (`nothing changed`), or why the editor failed (`/path/to/noedit
failed; nothing changed`), as for an editor that cannot start or a
terminal that cannot be made for it (`vim: no terminal could be made for
it in the page (…); nothing changed`), nothing changed either way. An
edit that asks the editor again, a patch that no longer applies with why
at its top, opens it in the page again. A page that comes while it runs is
told none of what the editor learned before it came, as ever. When
`stacked web` goes while the editor runs, by a ctrl-c where it was started
(which the editor does not hear), a kill, that terminal closing, or its
last tab closed (below), the editor is hung up on as its terminal goes:
SIGHUP, then SIGCONT, sent to its process group and to the one its
terminal has in the foreground, its terminal closed, which the system says
to it too; and what is left of them 2 seconds after is killed, so that
nothing of it is left running.

With `web.editor = "terminal"`, the editor runs where `stacked web` was
started: `$EDITOR`, for what is edited in it, opens there. So does an
editor with a window of its own (code, zed, gvim and the like, by the
program its command runs), which opens that window, whatever the
settings say. Meanwhile the page says so over its screen, darkened, as
StackED's window does: `The text is open in vim, in the terminal `stacked
web` was started from if vim runs in one.`, then on a line of its own
`stacked goes on once vim exits.`, vim being the program the editor's
command runs, as StackED names it (the last path piece of its first word),
`the editor` when there is none. The server says it to every page before
it runs the editor, and at once to a page that comes while it runs, which
says it over a screen still empty, as nothing else is read until the
editor exits. Then, whether the editor ran or could not run, the server
sends each page the screen whole, which takes the place of what was said
over it; the bottom line says what came of it (`nothing changed`), or why
the editor failed (`/path/to/noedit failed; nothing changed`). What the
pages do meanwhile is dropped, as StackED drops what is done in its window,
and nothing says so: keys, clicks, the wheel, drags, the mouse going over
the screen or leaving it, pastes, a notice clicked; but a new size, so that
the screen sent after fits the page as it is then. The server drops it,
for every page, one of an older stacked too: what it reads from a page
from when it tells the pages that the editor runs to when it sends them
the screen after is dropped; what it read before, or reads after, is done.
The page sends nothing meanwhile but its size, and the mouse is over what
is said, not over the screen.

Quit is quit, and the page says so. A ctrl-c where `stacked web` was
started, a kill, or that terminal closing quits too, and so does closing
the editor's last tab (below). Either way, as when the editor in a terminal is left,
the agents' worktrees made ahead are removed, and the asks still running
are listed there; and `stacked web` ends once each page is told (a few
seconds at most, for a page that reads nothing). A ctrl-c while `$EDITOR`
runs in that terminal goes to `$EDITOR` alone.

With a port the settings fix, `"fixed-random"` or a number, the address is
kept for the next launch: `stacked web` keeps its token, for that port
alone, in `web-token.json` beside `state.json` (where `$STACKED_HOME`, or
`$STACKED_APP_STATE`, puts the state; nowhere where the state is kept
nowhere), a file that only you may read or write, written whole beside it
then renamed into place, and never in the settings' file, which you may
share, nor in a database. The next launch on that port listens with that
token and opens no tab at once: it says `a tab already open on port 51234
has 2 seconds to come back, else a new tab is opened on a new address`, and
gives the tab already open on that address 2 seconds, one constant, to
come back, a request of its page with that token, but its lines (its
events asked for again, as the page asks for them each second while
stacked is gone). Come back, it says `the tab already open came back`, then
where the editor is, as ever, and opens nothing. Else it takes a new token,
which it keeps in the place of the other, says `no tab came back within 2
seconds, so the address has a new token, and a tab with the old one is
refused`, then where the editor is, and opens the browser there, as a
launch with no token kept yet does at once; a page with the old token is
refused from then on. With `--no-open` it waits and renews the token all
the same, opening nothing (`…, else the address is given a new token`).
With `"random"`, or `--port`, the token is new each launch, and the file is
neither read nor written: a token kept there stays as it was, unused, and a
launch on its port fixed again waits for its tab, as above. A file the
token cannot be kept in is said (`the token of this address cannot be kept
in …, so the next launch opens a new tab: …`), and the launch goes on.

When stacked quits, or the page's events are lost, the page says so over
its screen, darkened, as while `$EDITOR` runs in the terminal: `stacked has
quit.`, then on a line of its own `This tab waits for stacked web to start
again on this address, then goes on by itself.`; or, its events lost with no word from
the editor, `stacked cannot be reached.`, then `This tab waits for stacked
web to answer again on this address, then goes on by itself.`. The browser
asks for the events again each second, at the same address with the same
token, saying the launch it had them from (the events' `Last-Event-ID`);
what the page does meanwhile is not sent. Once stacked answers, it sends
the screen whole, which takes the place of what was said over it, and the
page says its size: the wasm and the page's script stay as they were
loaded, unless the stacked that answers is of another build (its page or
its wasm differs, newer or older), whose page is then loaded in the tab.
Refused, as stacked started again with a new token, the page says `stacked
web started again with a new address.`, then `This tab can be closed.`, and
asks no more. A launch that keeps no address never listens on the same one
again: once it quits, its page says `stacked has quit.`, then `The next
stacked web opens a tab of its own, so this tab can be closed.`, and asks
no more. A tab that comes back from another launch while it is not what you
look at shows a banner, as the notices below do, titled `stacked`:
`stacked web is running again: click to switch to this tab.`; a click on
it brings the tab forward, and does nothing more. With `web.notifications =
false`, or the browser's permission refused, nothing is shown.

Closing the editor's tab quits `stacked web`: the page says it goes when it
is hidden for good (`pagehide`: closed, loaded again, or left for another
page), in a line of its own sent as a beacon, its events closed. Once the
last page with its events open has said so, and no page asks for its
events within 2 seconds, `stacked web` says `the last tab of the editor was
closed, and no tab came back within 2 seconds, so stacked web quits`, and
quits as Quit does. A reload, which says it goes and comes back at once,
quits nothing; nor does closing one tab of several while another stays. A
page cut off without saying it goes, as when the machine sleeps or the
browser puts its tab aside, quits nothing: it comes back, as above. Nor
does anything quit so before a page ever came: the first launch's tab
counts as soon as it asks for its events.

While the page is not what you look at, hidden behind another tab or
window, or in a window that does not have the focus, it shows the editor's
notices through the browser's notifications, which macOS shows as banners:
of every stack of the database, not only the one shown, the work that lands
and waits for your review, said as the banner says it, an ask's change or
answer (`A landed ask waits for your review: 1 reply, asked "Why add
b?".`), a follow-up's work come into the review of the work it follows up
(`A follow-up landed: its work came into the review of the work it follows
up: 1 reply.`), a lint of the stack (`A lint of the stack waits for your
review: what the linter reported.`), a reconcile with HEAD (`A reconcile
with HEAD waits for your review: …`) and a pull from Phabricator made (`A
pull from Phabricator waits for your review: …`), and work landed after an
edit under review said so (`… waits for your review after an edit under
review: …`); a run that fails, said as the bottom line says it (`The
agent's change failed: the agent failed (1): rate limited.`, `The linter
could not lint the stack s: …`, `The reconcile with HEAD failed: …`, `The
pull from Phabricator failed: …`, `The try push was not made: …`; failed
with a tool the agent could not use, `… Its work waits for your review.`
after it), but one you stopped or dropped yourself, in this editor or in
any other (an agent stopped, or dropped as the work it went on from was
rejected, sent back or edited by hand, the linter stopped, what waited its
turn dropped, a try push stopped); and a try push followed that is over,
done on Treeherder with how many of its tasks failed (`The try push is
done: 1 of its 2 tasks failed. Its summary waits for your review.`; none
failed, `none of its 2 tasks failed`; and how many never ended or were not
seen on Treeherder after it), or not pushed to try (`The try push did not
get to try, as Lando says FAILED: …. Its summary waits for your review.`).
Each is titled with its stack, `stacked: bug-2084854`, its body cut at 300
characters, and is of a run that was going when the editor read the
database before, and landed since: a page is never told of what was there
when it came, as `stacked web` started or later. On screen, nothing is
shown: the editor's own banner and bottom line say it. Two pages hidden
show one banner for each notice, the second taking the place of the first.
A click on one brings the page forward and closes it; then, on the main
screen with nothing open on it, the editor shows the notice's stack and
opens the review of what it tells, as Enter on its line of the banner does
(for a follow-up, the review of the work it follows up), or, for a
failure, says it again in the bottom line; with a menu, a dialog or a
comment being written open, or on any other screen, nothing more is done,
so that what is open is never left for a notice. The browser asks once
whether the page may show them, on the first click or key that reaches it
(but Esc, or a modifier alone, such as the ⌘ that each ⌘ shortcut, ⌘-Tab,
⌘-L, ⌘-W or ⌘-1, sends first: browsers do not let a page ask on those),
as a page may ask only
then; once you answer, never again. It keeps your answer for the page's
address, its port included: with `web.port = "random"`, a new port each
launch, it asks at each launch, and with `"fixed-random"` or a port, once.
Refused, or in a browser without notifications, nothing is shown, and the
page works as ever. `web.notifications = false` (Settings, below) sends
none.

The tab's icon counts the notices that came while the page was not what you
look at, whether the browser showed their banners or not: StackED's icon
with a red badge in its top right corner, the count in white, `9+` past 9,
so that it stays legible on a tab. The page draws it on a canvas from the
plain icon, at both of the icon's sizes, 32 and 64 pixels, the badge's
circle five eighths of the icon wide, ringed with the page's background,
`#1e1e1e`, to stand apart from the icon's white sheets; its red is
`#d32f2f`. The notice of a tab come back (above) counts too. A notice that
comes while you look at the page counts nothing. Once you look at the page
again, shown in the window that has the focus, the count is 0 and the plain
icon comes back. An installed app of the page has its own badge where the
browser has one (`navigator.setAppBadge`): it says the count too, and is
cleared with it; a browser without one, such as Firefox, has the tab's icon
alone. A browser that draws no icon a page changes, as Safari may, shows
the plain icon; nothing else says the count. There is no setting for it.

With what the notices tell, while the page is not what you look at, as for
the banners, the page rings a bell, once `web.bell` (Settings, below) gives
it a level above 0: unless set, it is 0, and the bell never rings. At 100,
the bell is one soft strike of a small bell, about
a second long, which the page makes itself with the browser's Web Audio
API, from no sound file. Its partials are a thin hemispherical shell's, as
a desk bell's are: A5, 880 Hz, and 2.828 and 5.423 times it (2489 and 4772
Hz), at 0.15, 0.045 and 0.015 of full scale, each struck in 4 ms, then
dying away by 60 dB in 1.2, 0.6 and 0.3 seconds, the higher the sooner; at
its loudest, −14 dBFS. At a lower level each peak is scaled by the square of
the level's fraction, as loudness is heard: 50 strikes at a quarter of
those peaks, 12 dB under them, about half as loud to the ear; 10 at a
hundredth, faint. What the editor learned at once rings it once,
however many notices it makes; and a bell still sounding, for 1.2 seconds,
is not struck again. It rings whether the page shows the banners or not:
refused by the browser, in a browser without notifications, or with
`web.notifications = false`. `web.bell = 0` silences it. The notice of a tab come
back, `stacked web is running again: click to switch to this tab.`
(above), rings nothing, as it tells nothing the editor learned. A browser
lets a page make sound only once a click or a key reached it: the page
makes its audio on its first click or key (but Esc), and starts it there,
and again on
each click or key after until it has started once, as a browser may take a
key for no such key (⌘ alone, as ⌘-Tab sends it). Until then the bell is
silent, and nothing says so, as nothing says that the banners wait for
that click or key either. A page loaded again, as when a stacked of
another build answers (above), needs its first click or key again for the
bell, though the browser keeps its permission for the banners. Between two
strikes, the page's audio is suspended; a strike the browser holds back,
as one may until the page is shown again, is not struck once it comes more
than a second late, nor once the page is looked at. Nothing in StackED or
in the terminal rings.

The Configure screen (The editor) has a test button, `< Test >`, on the
rows of `web.notifications` and `web.bell`. In the page, the first sends
the pages a test notice, `This is a test notice from stacked.`, titled
`stacked`, which each page shows as the system's banner even while you look
at it, counted on no icon; if the browser was never asked whether the page
may show banners, it is asked then, and the banner shown once you allow
it; refused, nothing shows. The second has the pages ring the bell once, at
the level in effect, or at the one being typed for `web.bell` when it is
clicked while that line is open, even while you look at the page. Each is
grayed, and Enter or a click on it says why on the bottom line: in the
terminal and in StackED, which show no notices and ring no bell (`No page
shows the test notice here: this is stacked in a terminal, …`, `StackED
shows no notices: they are stacked web's, shown in its page. …`); with no
page open (`No stacked web page is open, so there is nothing to show the
notice in.`); and, for the bell, at the level 0, which never rings (`The
bell's level is 0, so it never rings: set web.bell above 0 first.`). The
server reads its settings as they are each time it tells the pages
something, so that a change written on the Configure screen counts at once.

The server runs the editor as the terminal does, drawn into a buffer of the
page's size, and sends the page the rows that changed (server-sent events),
after the notices of what it learned meanwhile (`tui/notices.rs`), then,
once for them all, the bell (`b LEVEL`, a line of its own, its level from
1 to 100), so that a page sent the screen that says what landed was sent
its notice and told to ring: the notices' lines unless `web.notifications`
is false, the bell unless `web.bell` is 0, the settings read as they are
each time. A test button's press is a notice's line or the bell's with
`test` last (`n RUN US TITLE US BODY US test`, `b LEVEL test`), which the
page shows or rings even while it is looked at. The server decides, and
the page does as told. A page
that comes is sent none of what the editor learns as it comes, the
database read first, its notices sent to the pages there before. While
`$EDITOR` runs in the page, the editor's thread runs the editor's
terminal, lent the pages and the screen's size (`web.rs` `in_page`): it
reads what the pages send, and what the terminal's reader read of the
editor, from the one queue it reads the pages from, a terminal's by its
number, so that what one wrote, read late, is not taken for the next's;
the pages' lines are read as ever, and nothing more is shared under the
lock below. While `$EDITOR` runs where `stacked web` was started, or in a
window of its own, the editor's thread reads nothing: the threads that
answer the pages' events share with it, under one lock, the pages that
came and that it has not read yet, and what says that the editor runs, so
that a page that comes then is told at once, and one come just before the
editor starts, which the editor's thread does not know yet, is told as it
starts. The threads that read the pages' lines read them under that lock
too, which the pages are told under that the editor runs and sent the
screen after under: a line read once a page could have that screen is
done.
Under that lock too: the token a request needs, which the launch renews
under it while it waits for the tab of the launch before, so that a page
of that tab either came back before or is refused after; the pages'
events open, each counted from when it comes until its page closes it, a
thread of its own reading it for that, and when a page last said it goes,
by which the editor's thread knows its last page gone; and whether the
editor is left, a page's events that come after told so at once.
Each page's events start with which stacked sends them (`v BUILD LAUNCH
KEPT`): the build of the page and its wasm, the launch, and whether that
launch keeps its address; and with how long the browser waits before it
asks for them again, once lost (a second), and the launch as their id,
which the browser says as it asks again.
A page of an older stacked does nothing with a notice, nor with the bell,
nor with what says that the editor runs, nor with `t`, as with any line it
does not know; the notices' lines are as they were, so that it shows the
banners as ever. The page sends
its keys, its clicks and its wheel back, the wheel
with the cell under the mouse and the lines it turned, and with the left
button down, each cell the mouse goes over, then the cell where the button
is let go; a notice clicked, by its run; and, while the screen is the
editor's own, its pastes as text. Keys that come together are drawn
once, after the last, and each is done on the screen as the keys before it
left it, as in a terminal, which draws the screen after each key: Home,
then Space at once, opens the stack's menu with the stack's comments to
ask about, as a right click on the stack's header does. What the page does
is wasm
(`web/client.rs`: the rows made HTML, the notices what the page shows of
them, the bell's line the bell, `$EDITOR` running the words said over the
screen, `t` what says that the screen is the editor's own, which stacked
sends the lines what the page keeps of it, the keys, F1 to F12 and Insert
among them while the screen is the editor's, a character Option made on a
Mac, or AltGr elsewhere, as it is, told from the key's own by the key's
code (`KeyD`), a paste the keys that type it
or then its text, the clicks and the wheel made lines for the server, the
wheel's small turns in pixels kept until they make a notch, 40 pixels, and
each turn in lines or in pages a notch, whatever its size), carried by a
hundred and ninety lines of JS (`web/index.html`), which tells whether the
page is on a Mac (`navigator.userAgentData`'s platform, else
`navigator.platform`); which measures the page's cell, a row made the
whole device pixels nearest to its line (`--row`) and its height read
back as laid out, so that the mouse's cells are where the rows are, again
at most once a frame as the window is resized, as the page moves to a
screen of another density (a query for its `resolution`, as no `resize`
may come), and as a font loads, saying its size only when the cells that
fit it changed, or, looked at again, when another tab's size was said
since, never in answer to the server's; which reads the
wheel's mode before its turn: Firefox says a mouse's notches in lines only to a
page that reads the mode first; which shows the notices, and asks the
browser's permission for them; which rings the bell, its audio made on
the first click or key; and which says over the screen that
stacked is gone, while the browser asks for the lines again, and says the
page goes as it is hidden for good. The
editor knows whether the mouse is in use, and what the wheel scrolls
under it (`tui/wheel.rs`), as it does for StackED; and what a press on
HEAD's row is, a click or a drag (`tui/drag.rs`).
