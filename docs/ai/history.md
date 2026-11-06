# History

The file holds every stack, every version of every patch, every turn of
every comment and every run; the editor shows them where they belong:

- `Tree of edits…`, on the stack's header: every state of the stack, the
  one being edited marked; enter edits from another. Beside each, its
  patches and what its edit changed; Space, `What this edit changed…` in
  full: its stack diff (below), from the stack it was edited from, on a
  screen of its own, its interdiffs in the view `1` sets there, every
  line or only what the patches changed (The editor, above), or each
  patch's change from the stack it was edited
  from to the one it made, as a patch of its own, the two stacks named
  `@10` and `@12`, its top line kept from one view to another.
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
  is not taken for moved. → goes into it, ↑↓ scroll it, ← goes back; `1`,
  on the history or in the stack diff, picks the view of its interdiffs,
  every line or only what the patches changed (The editor, above), or of
  each patch's change from the stack before to
  the one after, as a patch of its own, each version on the stack it is
  in, the two stacks named `@4` and `@12`, the cursor on its line. It is only to look at: nothing is edited, commented on or
  asked from it, nor undone or redone from History…; esc leaves it, for
  the stack as it is.

  ```
   the stack @4 → @12
   edit e80b0801 as e35c91e0 · 2026-10-17 19:50
    1 ~ Bug 1 - parse skips blank lines, total does not overflow (reworded)
        The first sign of each line says which version has it: a space for
        both, - for @4 alone, + for @12 alone. The second is that version's
        own sign: + for a line its patch adds, - for one it removes, a space
        for one it leaves as it is.
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
  `stacked interdiff` are made the same way. Each line of one is also told
  apart by what part of its version's mail it is, the mail's headers and
  message, or under the `---`, a file's header, a hunk's header or a line
  of a hunk; and, a line of a hunk taken out or put in, what changed in it
  as its own version's diff pairs it with the line removed or added in its
  place: what the views of the interdiffs draw (The editor, above).
- `Versions of this patch…`, on a patch: every version, oldest first, each
  with the edit that first had it; beside it what changed from the version
  before, in the view of the interdiffs `1` sets there, every line or
  only what the patches changed (The editor, above), the two versions
  named as the list names them (`v2`, `v3`): the
  interdiff, or the change from the one to the other as a patch of its
  own, each on the stack that first held it; or the version whole. `Put this version back in the stack` is an edit like
  any.
- `History of this comment…`, on a comment: every state it was in, with the
  rev and time of each; a closed one's too, from `Filter comments…`,
  `closed`.

In the browser and in StackED, the mouse over a line of the tree's list,
of History…'s (a record, or a comment under it, which marks the record
with it) or of the versions' marks it, underlined, and picks nothing, so
that what is beside the list goes on showing the line picked: a click
picks the line, what is beside following (History…'s stack diff from its
first line, as the arrows show another record's), a second click on it is
Enter, and a right click opens its menu, as Space does. The wheel over the
list scrolls it, the line picked staying picked, in sight or not, until a
key.

`stacked log`, `tree`, `versions`, `interdiff` and `runs` say the same on
the command line.

## Asking the agent about the history

On a record of `History…`, Space, `Ask agent about this, answer only…`
writes a question under it (`why was it dropped?`, `what changed since?`),
and `Ask agent, answer only…`, in the menu Esc opens, asks it from the ask
dialog, with the model and at the effort set there: answer only, the
dialog's one permission here, so the agent changes nothing. It is told the
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
