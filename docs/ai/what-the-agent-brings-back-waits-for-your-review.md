# What the agent brings back waits for your review

When an ask lands, nothing changes under you while you read or write.
What the agent brought back is held, in a queue of work to review, the
first landed first; what a pull from Phabricator brings waits in it too,
its change of the stack, if it makes one, and the reviewers' comments
(Phabricator, below). One landed ask may hold three kinds of parts: its
change of the stack, if it changed it; its replies to the comments it was
asked about; and the comments it opened (an answer's, or a build and
test's findings on each patch's title). A try push's ask holds a fourth,
the tasks it selected (Try push, below). An ask that brought none of them
back waits for your review all the same while a comment it was asked
about is still open: its review has no part, only those comments, to
close or keep. If every comment it was asked about was closed or deleted
meanwhile, it is not held, and its run says that the agent brought
nothing back. You accept or reject each part, but the comments of a pull
from Phabricator, which are all accepted (Phabricator, below). A
comment is one part. The change of the stack is a part for each of its
operations, each patch's and the base's, accepted or rejected on its own
as far as what each needs to apply allows, which is worked out as the
work lands (below); the change of work that landed before stacked worked
that out is one part, accepted or rejected whole. Rejecting every
operation of the change rejects the replies that go with it too, since
each says what the agent changed. Once every part is accepted or
rejected, the review closes. What was accepted is applied then, the
change of the stack as its operations accepted leave the stack; what was
rejected is dropped for good, though `Runs…` still shows what the agent
said and did.

Until its review closes, a change of the stack keeps the stack as the
agent took it: the stack is not edited by hand, as while the agent works.
Chained (`agent.chain`, on unless set to false; Asking the agent, above),
the next ask that may change the code does not wait for that review: it
goes at once, from the stack that change makes, as if you accept it, and
what it brings back is reviewed after that change.
Unchained, that ask waits for the review (`ready (may change the code),
it goes once you have reviewed the agent's
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
waiting its turn (`Reconcile with HEAD waits its turn`), the stack's
pull from Phabricator, as it reads Phabricator, read by this editor, the
revision it reads (`pulling from Phabricator: D12, revision 2 of 3…`),
else `the pull from Phabricator is reading Phabricator`, then waiting its
turn (`the pull from Phabricator waits its turn`), or being made (`the
pull from Phabricator is being made`), what the asks batched (`3
batched asks`), what the agent works on (`the agent works on 1 ask`), the
comments that wait their turn (`1 comment waits its turn`), and how many
comments are open (`5 open comments`); it is there while any of it is. Landing writes nothing
on the bottom line. Esc goes to the banner from wherever the cursor is in
the panes, and ↑ from the stack's header goes there too; a click on it as
well, but not the mouse going by. With the cursor on it, the banner opens,
a line for each, in sections under their titles, and the cursor is in
none of the panes: what each has picked shows as in a pane the cursor is
not in (`picked`), not reversed. First, `read only, to review`: `⟳ Review …` for what landed and changes nothing (an answer, an
ask that brought nothing back, a pull that brought comments alone), each
said by what is left of it to review, the first landed first, then `◌ … after
the edit under review` for those going on from it: what a pull brings
waits there once it is filed, or under `edits, to review` when it changes
the stack. `edits, to review`: the change of the stack you review now,
then those going on from it, which you review after it, a pull's among
them; then a reconcile with HEAD waiting its turn, and the stack's pull
from Phabricator, from when it is asked to when it is filed, each phase
said as it is (`⇣ The pull from Phabricator, reading Phabricator: the
stack's revisions, with their parents and those stacked on top`, `⇣ The
pull from Phabricator, reading D12, revision 2 of 3: …` as this editor
reads it, `◌ The pull from Phabricator, waiting its turn: …`, `● The
pull from Phabricator, being made: …`). `open comments`: while there is a batch, `◇ Send the
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
Nothing was changed yet`. The stack's pull from Phabricator waiting its
turn: Enter on its line opens its menu, which says what it waits for,
with `Drop the waiting pull from Phabricator…`, which the banner's menu,
the stack's and the runs have too; it asks `drop the pull from
Phabricator that waits its turn? Nothing was held yet`, what it read of
Phabricator staying stored. Dropped, each is as if it was never asked:
nothing is linted, taken from HEAD or pulled, nothing changes in the
stack, the marks stay, and the edits asked after it no longer wait for it
(the question says how many), so that they go in their turn. What runs keeps
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
settings). Each stack operation is a part of its own, marked so; a `noop`
changes nothing, and has no mark. The title of the stack operations
counts how they are marked, each count after its mark and in its colour,
those to review in the title's own: `stack operations · ✓ 3 accepted, ✗ 1
rejected, · 1 to review`, those it has; the list cuts what is too long
for it. A change that leaves every patch as it was, and the base too, has
no operation to mark: it is marked as a whole, its title saying `stack
operations · accepted` with the word in green, `rejected` in red, or `to
review` as the rest of the title. So is the change of work with nothing
stored of what each operation needs (below), every line of its operations
starting with the change's mark, none of its own (`   · modify  4 Bug 4
- line 35`, `   · noop    1 Bug 1 - line 5`): Enter, Delete and Space on
any of them mark the change as a whole. The line of each operation starts with
its mark, and the operation after it keeps the colour of its kind: `add`
green, `modify`, `reword` and the base magenta, `remove` red, `noop` dim,
and `move` as any text. Under the cursor, a marked line is in the
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
part the cursor is on, or puts it back to review if accepted, and Delete
rejects it, as Space's `Mark as rejected` does. On the title of the stack
operations, Enter accepts every one of them, or, every one accepted, puts
each back to review, and Delete rejects every one. Space has `Mark as
accepted` or `Mark as rejected` (on the title, for every operation); and,
each kind apart, `Mark all operations accepted` and `… rejected` on every
other line, for the stack operations, `Mark all opened
comments accepted` and `… rejected` for the comments the work opened,
`Mark all replies accepted` and `… rejected` for its replies. Rejecting
every operation of the change marks the comments rejected too, and
accepting one again puts their marks back as they were; a comment cannot
be accepted while every operation is rejected. Esc says what is marked,
by kind of part (`stack operations: ✓ 3 accepted, · 1 to review`,
`replies: ✓ 1 accepted, · 1 to review`). Each count is marked as the
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

An operation needs another when its patch does not apply, in the stack
as the operations stand, without the other: without a `modify`'s or a
`reword`'s, that patch as it was; without an `add`'s, that patch absent;
without a `remove`'s, that patch still there; without a `move`'s, that
patch at its old place; without the base's, the base as it was. Whether
a patch applies is told by the check every stack passes before it goes
in the database (Only patches that apply go in, below). What each
operation needs follows from two stacks alone, the one the change went on
from and the one it makes, which are fixed once the work lands: it is
worked out then, by the landing, and stored with the work (One file, only
appended to, below), for the stack the change makes, every operation
standing, and each operation rejected alone: the operations that go with
it (its patch as it was, or one of theirs, applying only on what they
leave as they were), worked out from the bottom of the stack up, and the
operations whose patches then no longer apply, which need it, from the
top down, each found taken as rejected too, as you would reject it first,
to see past it; each worked out from what was found of those before it,
so that a chain of them is a few checks each. An agent's change works it
out as it lands: what the agent did is read from its worktree under the
launch lock, made on a copy of the database, then what each operation
needs is worked out from that copy with no lock held, and stored as the
work is filed, if the stacks it was worked out for are still those (else
why not is stored). A lint job's is worked out once the linter is done,
before its work is filed; an edit by hand's, once it is filed, before its
review opens; a reconcile with HEAD's as it is made, its operations
taking messages alone, which nothing needs, with no check; a pull from
Phabricator's as it is made, on a copy of the database with no lock
held, before it is filed (Phabricator, below). An operation
whose rejection leaves every diff of the stack as it was, as a `reword`'s
does, is told with no check. The review then decides from what is
stored, at once: the right pane's sentences, Enter, Delete and Space, and
the questions about operations that go together check nothing. The stack
as the operations stand, those accepted and those to review, applies as
far as what is stored tells, each operation rejected alone: two
rejections together may, rarely, leave a patch that applies on neither,
which closing finds (below).
Rejecting an operation is refused while one that is accepted or to review
needs it, the status line saying which (`patch 5's new version applies
only on patch 4's new version: reject patch 5's operation first`), and so
is rejecting one that a patch the change leaves as it was needs (`patch
6, which the change leaves as it is, applies only on patch 4's new
version, so patch 4's operation cannot be rejected alone`): operations
are rejected from the leaves inward. Accepting one is refused while one
it needs is rejected (`patch 5's new version applies only on patch 4's
new version: accept patch 4's operation first`). A `reword` changes no
diff, so nothing needs it. When an operation's patch as it was would no
longer apply on what the others leave, as a patch made again on the new
version of the one under it, the operation that makes it so goes with
it: rejecting the one asks first whether to reject both (`reject patch 5's
operation with patch 4's operation?`, the question saying why: `Patch 5 as
it was applies only on patch 4 as it was. Rejected alone, patch 5's
operation would leave a stack that does not apply.`), with `Reject them`
and `Keep them as they are`, and accepting the other again asks the same.
On an operation, the right pane shows the stack diff at its patch, as on
any line of the stack operations, and under the operation's line, while
the cursor is in the list, what it needs and what needs it, a sentence
each: `Patch 5's new version applies only on patch 4's new version.`, then
`Nothing else needs this operation: it can be rejected alone.` or `Patch 5's
new version applies only on patch 4's new version: reject patch 5's
operation before this one.`; rejected, `Nothing it needs is rejected: it
can be accepted alone.` or what to accept before it. On a `noop`, it says
that the change leaves the patch as it is, with nothing to accept or
reject.

Work with nothing stored of what each operation of its change needs is
not worked out as it is reviewed, nor stored later: its change is one
part, accepted or rejected as a whole, as before stacked marked each
operation, and its review says why, on the stack operations' right pane,
under the line the cursor is on (over the stack diff on their title), and
in `?`: `This work landed before stacked worked out what each operation
needs, so its change is accepted or rejected as a whole.`; for work whose
landing could not work it out, `What each operation needs could not be
worked out when this work landed, so its change is accepted or rejected
as a whole: ` and why; for work whose stack changed since it landed, `What
each operation needs was worked out for the stack as it was when this
work landed, and the stack has changed since, so its change is accepted
or rejected as a whole.` (its change can then only be rejected, as
ever). Nothing is checked as it is reviewed, and closing it checks
nothing of its own: the stack its change makes was checked as it landed.

Closing, the stack becomes the stack as the operations accepted leave it:
each patch whose operation is accepted as the change leaves it, every
other as it was, where it was, and the base as its operation is marked.
Accepted in part, that stack is checked first, away from the screen: the
status line says `checking the stack the operations you accepted leave…`
meanwhile, the keys going as ever (the cursor moves, the right pane
follows it), and the review closes once it is seen to apply. If it does
not, as when `Reject all remaining and Close` would reject an operation
that one accepted needs, or two rejections together leave a patch that
applies on neither, the review stays open, nothing done, the status line
saying which patch does not apply, as git says it, and what it needs: the
operation rejected nearest under it whose acceptance would let it apply,
or, a patch as it was, the operation accepted whose rejection would
(`the stack the operations you accepted leave does not apply: patch 5
(…) does not apply to the patches below it (…); patch 5's new version
needs patch 4's operation, which is rejected; nothing changed`). A mark
changed while that stack is checked calls the close off, nothing done:
what was checked is not what would close (`the review changed while the
stack the operations you accepted leave was being checked, so nothing
changed: close it again`). Closing again while one is checked takes its
place. Accepted whole, or rejected, closing checks no stack of its own:
the stack the change makes was checked as the work landed, and rejected,
the stack stays as it is. `stacked review --accept-all` and
`--reject-all` check the stack the operations accepted leave the same
before they take the database's lock, and refuse as the review does.
Accepted in part, the history names the operations rejected, by their
word and place as the list says them (`agent: the stack; rejected:
modify 5`), and so does the run (`reviewed: its change accepted in part, 3
operations accepted and 1 rejected (modify 5)`). A change accepted in part
is not the stack that the chained edits going on from it went on from:
they are dropped and asked again, as for a rejection (Asking the agent,
above), and closing says so first (`Accepting this change in part throws
away the 1 edit that went on from it, as if you accepted it whole: 1
waiting for your review is thrown away. It is asked again from the stack
the operations you accepted leave.`). Once any operation is accepted, the
stack changes: the answers made on the stack before the change go as they
are marked (below), and HEAD of your checkout may follow the stack.

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
count only once the change is accepted, wholly or in part: rejected, every
operation of it, or sent back, the change leaves the stack as it is, and
the answers go on as they are. Until each answer is marked, the review
does not close with its change accepted:
`Close review` and `Accept all remaining and Close` are grayed, and so is
`Reject all remaining and Close` while an operation is marked accepted, Esc
saying what they wait for (`Not marked yet: the stack · and the stack?
(waiting for your review)`) under how they are marked (`answers made on
the stack before the change: ✓ 1 kept, · 1 to mark`). `Accept all
remaining and Close` marks none of them. Closing with the change accepted
asks first, saying what becomes of each (`Closing this review applies its
change. Of the answers made on the stack before it, 1 running goes on, and
lands for your review on the stack it was asked on; 1 waiting for your
review is thrown away unseen, its comment left open, not asked again.`;
accepted in part, `Closing this review applies its change in part.`),
in place of what closing asked again before; and the status line, then
the run, says what came of them (`1 answer-only ask kept`, `… asked
again`, `… stopped`). The review of an edit by hand, of a reconcile with
HEAD and of a lint job lists them the same. A build and test is none of
them: it goes on, of the stack as it was, and closing offers to stop it,
as ever.

The review of a pull from Phabricator rejects none of its comments: each
shows as accepted (`✓`), Enter, Delete and Space mark none of them
(Delete says `a comment pulled from Phabricator cannot be rejected: close
the review to file it`), and Esc offers `Close review`, which files every
comment it pulled, with `Save` and `Discard` to leave it waiting for you.
Its change of the stack, when it makes one, is reviewed as any change,
operation by operation, from what was worked out as it was made, each
operation saying after it what it takes from Phabricator; Esc then offers
`Accept all remaining and Close` and `Reject all remaining and Close` for
those operations, the comments filed whatever becomes of them; with no
change, there are neither. A patch whose conflicts the pull marked does
not go in: closing with its operation standing is refused, and `Send it
back to the agent…` is offered (Phabricator, below).

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
marked them, and its change is to review again, each of its operations. The review goes on there,
its title saying what you edited (`then you edited a.txt in Bug 1`). The
review of an edit by hand has the same, to edit it again. On the command
line, `stacked edit NAME PATCH --work RUN` edits the patch as that landed
work leaves it.

On the command line, `stacked landed NAME` lists the queue: each landed ask
by its run, what is left of it to review (with no part left, the comments
the agent was asked about: `1 asking comment to close or keep, nothing
brought back`), the effort it was asked at, if it was asked at one (`asked
at max`), and each part of it, the change's line saying `accepted` or
`rejected` once every one of its operations is, and else how many are
marked each way, once one is (`operations: 1 rejected, 3 to review`); then
what a build and test asked as a
follow-up found, a line each (`found`), and each follow-up going, how it
was asked and where it stands (`following`). `stacked review NAME [RUN]
--accept-all` (or `--reject-all`) closes the review of that ask, the first
landed if no run is given: the parts not marked yet are accepted (or
rejected), each operation of its change not marked yet too, and what is
accepted is applied, the change as the operations accepted leave the
stack; a stack that does not apply is refused, saying why, and nothing
changes. There is no option for one operation: the review in the editor
marks each, and saved, those marks stand. `--close-asking` closes the
comments the agent was asked about too. A pull's comments are not
rejected: `--accept-all` files every comment it pulled, and so does
`--reject-all`, which rejects its change of the stack alone, and is
refused for a pull that brought comments alone. Accepting a change, chained, `--accept-all` goes by the marks its
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
