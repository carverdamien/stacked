# Searching

`Search…`, in every menu, opens the search on a screen of its own. On the
left, at the top, where it looks, a button (`‹ comment ›`) that ← → turn
to `all`, `patch`, `comment` or `history` (a click does too, as → does;
the mouse going over it turns nothing), and what you
type; under it, what is found, appended as it is found, in a thread of its
own: the screen never waits on it, and what you type starts it again. At
the bottom, how many it found, and whether it is still looking. On the
right, the match picked in its context: its rows in its patch (or the
stack), the lines around it in its file, or its record of the history,
what you look for lit. ↑↓ pick a match. Enter goes there: to the comment,
to the patch at its line (a file's line its diff does not show: the
patch's nearest line of that file, or with each file whole, that line), or
to the record in the history. Esc goes back where the search was opened.
In the browser and in StackED, the mouse over a match marks it,
underlined, and picks nothing, the right going on showing the match
picked in its context: a click picks the match, and a second click on it
goes there, as Enter does. The wheel over what is found scrolls it, the
match picked staying picked, in sight or not, until a key.

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
