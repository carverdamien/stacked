# Reading

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
