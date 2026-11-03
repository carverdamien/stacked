# Writing comments

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
