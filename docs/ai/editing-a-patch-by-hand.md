# Editing a patch by hand

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
reject, each new version's interdiff in the view `1` sets there (The
editor, above). Accepted, it goes in, as the edit of the stack it is (`edit … as …`
in the history); rejected, nothing changes. Left for later, the banner says
so (`⟳ 1 edit by hand waits for your review`), and as with the agent's
change, the stack is not edited by hand again before it is reviewed. So it
is of `Edit … here in $EDITOR`, `Edit this patch in $EDITOR`, `Edit the
stack as a list in $EDITOR`, the conflicts resolved after them, a file's
changes dropped with Delete, and the long lines of messages wrapped; moving, dropping,
undo and redo change the stack at once. On the command line, `stacked edit`
says the run the edit landed as: `stacked review NAME RUN --accept-all`
takes it, and `stacked edit … --accept` takes it at once.

With the `moz-phab` feature, an edit by hand that may change a patch that
should not change, its revision Accepted or Landed on Phabricator, asks
before $EDITOR opens, `accept` to type (Phabricator, below): the patch as
mail asks of itself; a file of it, of itself alone, as the patches above it
that it only rebases do not change; a conflict resolved by hand, of the
patch it is in; the stack as a list, of every patch. Typed, $EDITOR opens, and closing the review the edit lands
for changes them with no one asked again; cancelled, the editor does not
open, and nothing changes. On the command line, `stacked edit … --accept`
asks on its terminal, as it takes the edit.

The editor is told the line: `+LINE` for vi, vim, nvim, nano, emacs or kak,
`FILE:LINE` for hx, zed or subl, `--goto FILE:LINE` for code; any other is
given the file alone, unless the settings' `editor` says where `{file}` and
`{line}` go.
