# The editor

The left pane is the current stack: its header (`≡ bug123 · 3 patches`),
which is the stack's own place, then the patches' titles, bottom first, with
how many comments wait on an answer. When stacked runs in a checkout of the
stack's repository, a line among them says where its HEAD is (its branch,
else its commit): on the stack, `▶ HEAD main` after the patches its
commits since the stack's base are (a commit is a patch when its diff is,
whatever its message), under the header at the base; on another base,
under the header too, `▶ HEAD main: off the stack`. HEAD's commits that are
the stack's patches but whose message is not are marked, in the column
before the patch's number: `≠` when the commit's message says something
other than the patch's, and, with moz-phab, `!` when it names a
Differential Revision the patch does not name, none or another, which wins
over `≠` (Move HEAD, below, says what each locks). While there is a `!`,
HEAD's line ends `· locked below patch 2`, folded under itself where the
pane is too narrow for it.

HEAD forked from the stack on its base, with commits the stack does not
have (yours, or main pulled since, or the stack moved on since HEAD was
put there), is drawn as a graph:

```
● 1 [R]✓ Bug 1 - first part
≠ 2 [W]✗ Bug 2 - second part
├○ Fix the thing
│▶ HEAD main
└● 3      Bug 3 - third part
 ● 4      Bug 4 - fourth part
```

First the trunk the two share, the patches whose commits HEAD has, each
`●` or its mark; then the shorter of the two branches, then the longer:
the first one drawn starts `├`, its next lines `│`, the other starts `└`.
The stack's patches are `●`, every one of them, its rows as ever; the
commits HEAD alone has are `○`, by their subject, three rows at most, the
third `… 5 more commits` when there are more; and HEAD's line comes after
HEAD's branch. The cursor goes over the `○` rows: they are drawn only.
HEAD's line itself is a line the cursor stops on: its menu (Enter, Space)
has `Move HEAD…`, and `?` there says where HEAD is, its marks, where it is
locked, and which of Move HEAD's cases applies (`HEAD is as stacked left
it: Move HEAD replaces it with the stack as it is now.`), or why none does,
and then that `Move HEAD…` shows why, and what you can do (below).
On a patch marked, `?` says first what its mark means, what it locks or
grays, and how it clears; the bottom line says only that it does (`?: what
the ! on patch 2 means`). With more patches than the pane has room for,
HEAD's line stays in sight: on the pane's last line while its place is
further down (`↓ HEAD main`), under the header while it is further up
(`↑ HEAD main`), the arrow saying that patches go on between. It is looked
at again as the stack changes, and every few seconds. `stacked head NAME`
says the same.
`Move HEAD…`, in the stack's menu, each patch's and HEAD's line's, moves
that checkout along the stack: ↑↓ place HEAD's line (`↕ HEAD`), the
checkout as it is meanwhile, the patches drawn as they are, and Enter moves
it there, Esc leaving it where it was. In the browser and in StackED, the
mouse over a patch places HEAD's line under it, and a click on HEAD's
line, or on the patch it is under, moves it there; and HEAD's line
dragged with the mouse, its button held, is moved as `Move HEAD…` moves
it, put down where the button is let go over the patches (In a browser,
above). HEAD is
then detached at the stack's base with the patches up to there applied as
commits, those the checkout has already, message too, taken as they are;
the branch it was on stays where it was. Not while files git tracks have
changes, and only as below. `stacked head NAME --to N` moves it to N
patches (0, the base).

With the `moz-phab` feature (on by default), a patch whose message names a
revision (its `Differential Revision:` trailer, Phabricator below) has that
revision's status on Phabricator after its number, a letter in brackets,
expanded and listing files alike: `[D]` Draft, `[W]` Changes Planned, `[R]`
Needs Review, `[C]` Needs Revision, `[A]` Accepted, `[L]` Closed (landed),
`[X]` Abandoned, and `[?]` a status stacked does not know (`  2 [W] Bug 2 -
add b`). The Phabricator line over the patch's message says it in full
(below). Right after the letter, a mark says whether Phabricator has this
exact version of the patch, as that line says it: `✓` where the line says
`in sync` (the revision's latest diff does what the patch does, its title
and summary are those a submit sends, and a submit would not change its
parent), `✗` where it says what differs, the diff, the title or summary,
or the parent (`  2 [W]✓ Bug 2 - add b`, `  3 [R]✗ Bug 3 - add c`). The
mark is blank while what differs is not fetched and compared yet, or when
Phabricator does not have the revision; the pane and the line read it in
one place, so that they never disagree. `?` in the patches pane says what
the two marks mean. The statuses are a column of their own, as wide as
the widest of the stack's, so that the titles start together: a patch
that names no revision, or whose revision's status is not fetched yet,
has its column blank, and while no status is known there is no column at
all; the marks are one more cell of it, there once any patch shows one.
Expanded, the pane is wider by the column, up to half the screen, so that
each title keeps the room it had and folds as it did. The tags at the end
of a patch's first line (`[2]`, and `✓` or `3/7` in the reading mode) are
where they were: the reading mode's `✓` there says that all of the
patch's lines are read, not that Phabricator has this version. A revision
is a draft when its status is Draft: one in Changes Planned that was never
sent for review, as `Submit to Phabricator as WIP` makes one, is `[W]`,
though Phabricator says it is a draft too (`isDraft`, as it says of every
revision it has not sent to reviewers yet).

The letters and the marks are what stacked stored the last time it read
Phabricator, in the database (One file, only appended to, below), and the
Phabricator line says how long ago that was (below). stacked reads
Phabricator only when you ask, never by itself, not even as the editor
opens: a stack none of whose revisions was ever fetched has no letter at
all. `Fetch the latest from Phabricator`, in the menu of the stack's
header, in each patch's own `Phabricator` menu and in the menu of its
Phabricator line (below), reads every revision the stack's patches name,
away from the screen, over Conduit with the token a pull uses: all of
them in one search (differential.revision.search; one for each
Phabricator, should their links name more than one), then the rest of
what the lines say (below), and stores it. Meanwhile the top bar says
`fetching…`, and the bottom line `fetching the latest from
Phabricator…`, then what was fetched (`fetched the latest of 3 revisions
from Phabricator`, `fetched the latest of D12 from Phabricator`); one
fetch goes at a time (`already fetching the latest from Phabricator`). A
submit and a pull read Phabricator too, and store what they read
(below). A fetch that fails stores nothing of what it read, so that what
is stored stays as it was, and the bottom line says why (`The stack's
revisions could not be fetched from Phabricator, so what is stored of
them stays as it was: …`): what Phabricator answered, a status other
than 200 with the first line of the page that came with it, cut short
(`differential.revision.search: Phabricator answered HTTP 502 with a page
starting "<html>"`), and a page that is not JSON by its first line too.
When Phabricator turns away the requests of this machine (HTTP 429, too
many requests), the fetch is tried once, and the bottom line says so
plainly, in the same words (`… stays as it was: Phabricator is turning
away requests from this machine for now (HTTP 429, too many requests)`);
nothing waits to read it again, as nothing reads it by itself: you fetch
again when you choose. What else you ask for is tried once too, a
submit, a pull of the stack, of one revision, or into a new stack, and,
turned away, says so plainly, a pull storing nothing of Phabricator then,
its run failed. Nothing is
read while reviews come from nowhere
(`reviews.source = "none"`, Settings below), when no fetch is offered,
nor in the tour. In the browser (`stacked web`) and in StackED, the
same.

A commit of the checkout is a patch of the stack when its diff is, but its
message may say otherwise. stacked never writes a commit's message into the
stack, but as `Reconcile with HEAD` (below) asks it: the banner, the
reviews and the menus are as they are whatever the commits' messages say. Two messages are the same when they differ only by
white space at the end of a line or by empty lines in a row, which `git am`
makes one as HEAD is moved. With the `moz-phab` feature (on by default),
the trailer moz-phab writes to name the revision, the line of the message's
last paragraph that Reading (below) says is that trailer (`Differential
Revision: https://phabricator.services.mozilla.com/D123`), is told apart:
a commit that names a revision its patch does not name, none or another,
as `moz-phab submit` leaves it in your checkout, is locked (below), and
while a patch and its commit differ by the revision they name, HEAD is not
in sync with the stack, so that it does not follow the stack as a review
closes. Built without the feature, the trailer is a line of the message as
any other.

Move HEAD replaces the commits of your checkout with the stack's, and
those commits might be yours: commits the stack does not have, or ones
amended outside stacked. So stacked remembers where it left HEAD: the
commit HEAD is at right after stacked last moved it, by Move HEAD or by
HEAD following a review, in that checkout on that stack, each move
replacing it, in the database (One file, only appended to, below). Move
HEAD replaces commits only when HEAD is exactly that commit, whatever the
stack has become since (its patches edited, reworded, the commits of HEAD
then other than the stack's): stacked made them; or when HEAD is on the
stack, its commits the stack's first patches, with nothing above them and
the same messages. Else `Move HEAD…` moves nothing: it is offered all
the same, drawn as any item, and chosen, it opens a view of why it does not
move HEAD, and of what you can do in your checkout so that it can (below);
`?` on it says so. `--to` is refused with the reason (`HEAD has commits
stacked did not make, as 1 commit after patch 2 is not the stack's: Move
HEAD moves HEAD only when HEAD is on the stack with no marks, or as
stacked last left it`). Everything else of HEAD is read from the commits,
by their diffs, never from what was remembered.

A commit that names a revision its patch does not name is never rewritten
nor dropped: everything up to and including the last such commit is kept
as it is, and HEAD goes nowhere below it. Moving HEAD, ↑ and Home stop at
it, a click below it says why (`HEAD is locked below patch 2: the commit
of patch 2 names D34, which the patch does not name`), and `--to` there is
refused with that reason. From it up to the stack's top, HEAD may go
anywhere: the stack's patches apply on it, as those commits have their
patches' diffs. `stacked head NAME` lists the patches whose message
differs from its commit's (`the message of patch 3 differs from its
commit's: "Bug 3 - other words"`; with moz-phab, when the two differ by the
revision they name, or would but for it, `patch 3 names no revision in the
stack and D3 in your workspace's commit`), then where HEAD is locked, and
why `--to` would be refused, when it would.

The view `Move HEAD…` opens while it would not move HEAD is only to read,
a screen of its own laid out as the review of what the agent brings back is
(below). On the left, first, where HEAD is and why Move HEAD does not move
it (`HEAD of your checkout, on main at 9f8e7d6c5b4a, is at patch 2 of s,
then 1 commit not in the stack.`); then each thing that keeps it from
moving HEAD, a line each, under a title for each kind: each commit only
your checkout has, after the stack's patches it has, by its subject (the
first ten, then `○ … 3 more commits`); each commit with its patch's diff
but another message, `≠`, then each that names a revision its patch does
not name, `!`, by its patch; or HEAD on another base. On the right, the
line the cursor is on: the commit as `git show` shows it, drawn as the
middle draws a patch (a diff of more than 2,000 lines said, not drawn); the
patch's message then the commit's, compared where their lines wrap aside,
as the reviews compare two messages; or HEAD and the stack's base, how
they stand (the commit they share and how many commits each has since, or
none shared), with HEAD's latest commits. Under it, stacked's comment says
why that keeps Move HEAD from moving HEAD, and what you can do in your
checkout, the git commands written out, their commits named: the commits
only your checkout has put on a branch first when HEAD is on none (`git
branch my-work 9f8e7d6c5b4a`, any name not taken), then HEAD back on the
stack's last commit there, or on its base (`git switch --detach
1a2b3c4d5e6f`), the branch HEAD was on keeping them; a commit marked given
to its patch by Reconcile with HEAD (below), or reworded in your checkout
to the patch's message (`git rebase -i 1a2b3c4d5e6f^` and `reword` on its
line, or `git commit --amend` for HEAD's own commit); HEAD on another base
switched to the stack's base, its commits first put on a branch when it is
on none, or kept where it is, Move HEAD left alone. stacked runs none of
them. What the view shows of the commits is read away from the screen as
it opens, `reading your checkout…` on the top bar meanwhile. As HEAD is
looked at again, every few seconds, the view follows it: once nothing
keeps Move HEAD from moving HEAD, it says so, and `Move HEAD…` moves HEAD
again. ↑↓ go from line to line, → (or Enter) into the pane on the right
to scroll it, → again to see it wide, the list out of sight, ← back, and
Esc goes back where the view was opened from; `?` says so. In the browser
(`stacked web`) and in StackED, the same; and the mouse over a line of the
list marks it, underlined, and picks nothing, the pane on the right going
on showing the line picked, scrolled as it was: a click picks the line,
the pane on the right showing it from its first line, and a second click
on it is Enter, into that pane. The wheel over the list scrolls it, the
line picked staying picked, in sight or not, until a key.

`Reconcile with HEAD`, in the menu of each patch marked and of HEAD's
line, takes the commits' messages into the stack: each patch marked, `≠` or
`!`, takes its commit's whole message as a new version, all of them in one
edit stacked makes, with no agent. It joins the queue as an edit of the
agent's does (Asking the agent, below), and is made at its turn, at once:
from the stack the edits before it make, chained on the last of them that
waits for your review, as chained edits are. It then waits for your review
in the banner, in turn (`⟳ Review a change of patch 2, reconciled with
HEAD: its commits' messages`); while it waits its turn, the banner says so
(`◌ Reconcile with HEAD, waiting its turn: the commit messages of patch 2
taken from HEAD`), and Enter on that line opens its menu, which says what
it waits for. Its `Drop the waiting reconcile with HEAD…`, there, on
HEAD's menu and the patch marked's, and Delete on its line, takes it out
of the queue once asked, as if it was never asked: nothing changes, the
marks stay, and the edits asked after it no longer wait for it (What the
agent brings back waits for your review, below). Edits
asked after it do not start before it is accepted or rejected, and then go
on from the stack that leaves (`ready (may change the code), it goes once
you have reviewed the reconcile with HEAD`); answers go at once, as ever.
The work it went on from accepted, it goes on from the stack that made;
rejected, accepted in part, sent back or edited by hand, it is dropped and made again at its
next turn, from the stack as it then is. One at a time: while one is under
way, the item is grayed, its menu saying why, as it is when no commit is
marked or the stack cannot change.

Its review shows what it takes from your checkout: the stack operations
(`reword  2 Bug 2 - …`) and each new version's interdiff, its message
compared with the patch's where their lines wrap aside (History, below).
Under them, `stacked's comment, to read`, nothing to accept or reject,
says what to run in your checkout should you reject it, to give the
commits the stack's messages instead, from the top down: `git commit
--amend` for HEAD's own commit, `git rebase -i COMMIT^` for one below,
marked `reword`, each reworded to its patch's message as the diff shows
it. Accepted, the patches have their commits' messages, and the marks are
gone; accepted in part, only those whose operations are accepted, the
others keeping their marks; rejected, nothing changes in the stack, and
the marks stay.

The middle shows what the left pane's cursor is on: a patch (its message,
then its diff, the comments under the
lines they are on), from a file's name when the cursor is on one of the
files the left pane lists under the patch (the arrows, below, say how), or,
on the header, the stack itself (its patches, then
the comments on the stack). Scrolled past, a patch's title stays on the
middle's first line, the rest under it, in every view of the code; a click
on it goes back to it. The right pane lists the comments the middle
shows, under a header of its own; with none, it is not there. The top bar
names the file and the stack, and on its right says how far the stack's
review is (Reading, below, says how).

The place the cursor is in, while it holds more than it shows (lines above
the first it shows, or below the last), has a scroll bar on its right
edge: a column a character wide, as high as its rows that scroll (those
under its header, where it has one), its track a light line, `│`, as the
lines between the panes are, and on it the thumb, a block, `█`, both in the
colour of its border, `meta` (the comment being written: its frame's,
`note`; Settings, below), so that the thumb stands out by its shape alone.
The thumb is as long as the share of what the place holds that it shows,
rounded, a row at least, and a row short of the track, which then says
that there is more; it is at the track's top while the view is at its first
line, at its bottom once its last line is shown (or the view is past it,
as a text on a screen of its own scrolls until its last line is the first
shown), and else where the view is between, a row from either end where
the track has the room. The patches pane counts by its lines, a patch's
title folded the lines it takes, the comments pane and a menu by their
comments and items. The place is the pane the cursor is in, of the middle
split in two the half it is in (the search of the middle, `/`, puts the
cursor in the middle); the banner, while the cursor is on it; the patches,
while a patch or HEAD is moved there; a box open over the screen, a menu
longer than its box, what `?` says, a reference's preview, or the comment
being written, taller than its box, the panes under it drawing none; the
agent's stream read in place, the bar beside its lines; and on a screen of
its own the part of it the cursor is in: the list of the tree of edits,
the stacks, the runs, the versions or the history, History…'s stack diff,
the review's list and its pane on the right, what a submit sends, the list
and the pane on the right of why Move HEAD does not move HEAD, a text or
interdiffs on a screen of their own, a transcript, what the search found.
There is none in the ask dialog, nor while a line is typed in a box, which
do not scroll. The places the cursor is not in draw nothing new, nor does a
place that fits.

A place with a line on its right draws the bar in that line, losing no
width: the patches, the middle beside the comments, the left half of the
middle split in two, the list of a screen of its own beside its pane (the
tree's, the stacks', the versions', History…'s, the review's, that of why
Move HEAD does not move HEAD, the search's); and every box, in its right
border, its corners as they are. A pane on the right edge of the screen
draws it in its own last column, which its lines then leave to it: the
comments, the middle and its right half while the comments are hidden, the
pane on the right of a screen of its own, a screen of one pane (Runs…, a
transcript, a text, what a submit sends, a list alone, the screen too
narrow for its pane beside it), and the banner, in the screen's last
column. The code, the messages, the comments in the middle and the lines
of the stack diffs and of interdiffs are folded a column short of their
pane, and leave that column blank already; a line cut at the pane's edge
is cut a column sooner: the comments pane's, a list's, a transcript's, a
text's, the banner's, and in the middle a hunk's header or a file's path
too long for it. The cursor's line is lit as far as the bar, and what a
box holds is as it was. In StackED, a pane's text selected leaves out the
bar's column. The bar follows the view however it moves: by the keys, by
the wheel (a pane the wheel scrolled away from what it picked shows where
its view is), and by a search jumping. In the browser (`stacked web`) and
in StackED, the same, as they draw the same screen.

With the `moz-phab` feature, a patch that names a revision has the
Phabricator line over its message, in every view of the code, reading or
not, its hunks or each file whole:

```
Phabricator: D123456 · Needs Review · the diff differs · 2 new comments to pull · fetched 2 hours ago
✉ message
   Bug 123 - the parser skips the blank lines
```

It says the revision; its status, by its full name: Draft, Changes
Planned, Needs Review, Needs Revision, Accepted, Landed (Closed, as
Phabricator names it), Abandoned, or `status unknown`; whether Phabricator
has this version of the patch, `in sync` when the revision's latest diff
does what the patch does (the same effect, or each file the same once both
are applied on the patches below it, as a submit's check takes them: the
line keeps those two strict rules, and not the submit's third, as a diff of
the same changes made on the patches below as they were before, which no
longer applies on them, is not what would land), its
title and summary are to the letter those a submit sends (`WIP: `
first or not, as the revision's title is; the summary without the lines
naming a revision; the white space at their end aside), where their lines
wrap too, as Phabricator's are what lands: a message wrapped again since
it was submitted is not in sync until it is submitted again; and a submit
would not change its parent, by the submit's own rule (Submitting a
patch, below): the revision of the patch below, when that is open, is its
parent already, or a submit sets none (for the bottom patch, on a patch
with no revision or one naming the same revision, on a revision closed or
that Phabricator does not have), so that once the stack is reordered, a
revision moved is not in sync until it is submitted again; else what
differs, listed: `the diff differs`, `the title or summary differs`, `the
parent differs`, `the diff and the title or summary differ`, `the diff and
the parent differ`, `the title or summary and the parent differ`, or `the
diff, the title or summary, and the parent differ`, always against the
patch as it is, never its older versions; and how many of the revision's comments are not
pulled yet: `2 new comments to pull`, `1 edited comment to pull` (edited on
Phabricator since it was filed), `2 suggested edits to pull` (filed without
the suggested edit Phabricator gives with them, which a pull reads now:
Phabricator, below), `no new comments`. Those are counted from
what the database holds as it stands, never from its history: a comment is
pulled when a comment open holds it, on the patch or moved to the stack's,
or when a pull's review waiting holds it. The line does not count the
comments of a thread you closed, though a pull brings the new ones: without
the history, a new reply or edit there is not told from the comments filed
in it before you closed it. Last, it says how long ago what it says was
fetched, worked out as it is drawn: `fetched just now` under a minute,
then `fetched 3 minutes ago`, `fetched 2 hours ago`, `fetched yesterday`
from a day to two, `fetched 4 days ago`, so that a state read long ago is
never taken for one read now. Of what the line says, the part read
longest ago tells it: a submit reads the revision's state but not its
comments, as of when it read it, a pull reads all of it, as a fetch
does, and a fetch that found the revision as it was stored counts as a
read of all of it. What is not known yet is `…` (`Phabricator: D123 · Needs
Review · … · fetched 2 hours ago`): whether it is in sync while the
revision's latest diff, or the revision of the patch below, is not
fetched yet, or while what differs is worked out; its comments while only
a submit read it. A revision Phabricator does not have is said so, with
how long ago (`Phabricator: D123 · not found on Phabricator · fetched just
now`); one nothing of which is stored yet says so, and where to fetch it
(`Phabricator: D123 · not fetched yet: Fetch the latest from Phabricator,
in its menu`). Whether each patch is in sync, the patches pane marks too,
after its revision's status (`✓`, `✗`, above), from the same reading as
its line. The line is a row
of the patch, over its message's header, and folds between words where
the middle is narrower; it is not pinned, and scrolls away with the rows,
the title pinned in its place. The cursor comes to a patch on its
message's header, the line over it in sight; ↑ goes onto it. There, Space
shows what differs on a screen of its own, titled `D12 on Phabricator,
then patch 2 as it is`: Phabricator's version of the patch, the
revision's title and summary as a submit compares them over its latest
diff, then the patch as it is, as an interdiff, the way `Versions of this
patch…` shows two versions, in the view of the interdiffs that `1` picks
there (below), the two versions named `D12 on Phabricator` and `patch 2
as it is`: the interdiff, or the change from the latest diff, on the
commit it is on, to the patch as it is, as a patch of its own. The screen first says what it shows: the
parent first, when a submit would set one, the revision's parents on
Phabricator then the one a submit sets (`Parent: D10 on Phabricator; a
submit sets D11, the revision of patch 2.`, `Parent: none` for a revision
with none; a parent that no patch of the stack names is named by a search
of its own). The
title and summary are compared where their lines wrap aside, what only
each has marked; when they differ only in where their lines wrap, or in
their white space, the screen says so, and each paragraph written
otherwise is there as each version writes it, Phabricator's lines taken
out and this version's put in, no word in the colours for words. The code
is the two diffs, as an interdiff, what git derives in them left out of
both, as Phabricator's raw diffs have none of it (the `index` lines, the
heading after a hunk's `@@`): Phabricator's lines taken out, this
version's put in. When the latest diff is on another base than this
version (the two read a file before the patch otherwise: a line at the
same place is not the same, the same hunk is at another place, or one
makes a file the other finds there), the code is instead each file as
that diff leaves it, then as this version leaves it, both on the patches
below it, as the line compares them, and the screen says so: a diff of
the files under `the code: Phabricator's, then this version's`, as an
interdiff and in a column per version; side by side, under `the code:
Phabricator's on the left, this version's on the right`, each file as
Phabricator's diff leaves it beside the same file as this version leaves
it, as the code view's side by side draws a patch, the lines both have
dim. Where the files cannot be read so, the code is the interdiff all the
same, and the screen says that too: in a column per version, the two
versions are then on two bases (below). When the code is the same, the
screen shows the
title and summary alone; both the same, the parent alone differing, it
shows nothing under what it says. ↑↓ (pgup pgdn home end) scroll it, `1`
opens the menu of the views of its interdiff (below), and Esc goes back
to the line. In sync, Space says so on the bottom line,
naming the parent (`Phabricator has this exact version of patch 3: the
latest diff of D12 does what this version does, the title and summary of
D12 are those of this version, and its parent is D11, the revision of
patch 2.`), or saying why a submit sets none, as a submit says it (`…
Patch 1 is at the bottom of the stack, so D11 gets no parent.`), and
opens nothing; the screen says the same, in sync (`Nothing differs: …`);
while what differs is not known yet, Space says that it is not, and that
the line says it once the revision is fetched and compared. Space there
marks nothing read, even in the reading mode, and opens no menu: Enter
opens it, titled `D12 on Phabricator`. Its first items are every item of
the patch about Phabricator, as the patch's own `Phabricator` menu holds
them (below): the patch's submit, `Submit to Phabricator as WIP`, or, for
a revision open but not in work, the update of its title, summary and
parent alone, which the line's comparison names, offers or grays
(`Update D12 on Phabricator: its title and summary`, Submitting a patch,
below); `What differs from D12 on Phabricator…`, which opens the same
screen, grayed while the revision is not read and compared yet, or
Phabricator does not have it; `Pull D12 from Phabricator`, which pulls
that revision with its parents, or with the revisions stacked on it too,
as its choice picks (Phabricator, below); and `Fetch the latest from
Phabricator` (above).
In the patches pane, a patch's menu has those items in a menu of their
own, its `Phabricator` item, which opens a menu titled for the revision,
`D12 on Phabricator`, or `Phabricator` for a patch that names none yet,
which holds its submit and the fetch alone; none of them is on the
patch's menu itself. An item grayed says why under the menu, as every
item does. `?` says what the line says, and that the patches pane marks
the same, what Space shows there, and what the menu has.

What the lines say is read by `Fetch the latest from Phabricator`, with
the token the statuses use, for every revision the stack's patches name:
after the statuses' search, the revisions' parents (edge.search) and
their latest diffs (differential.diff.search, the newest first, page
after page until each revision has its newest), each in one call for all
of them, with the commit each latest diff is on as Phabricator names it
(its `base` ref), stored, for the change from it to the patch as a patch
of its own (`1`, below); the parents that are none of the stack's revisions, of a
revision a submit would set another, named in one search of their own
(differential.revision.search), unless what is stored names them already,
as a parent's number never changes; then each revision in turn, bottom
first: its latest diff, raw (differential.getrawdiff), unless that diff
is stored, and its comments (transaction.search). The comments are read
only for a revision never fetched, or modified since they were stored, as
the statuses' search says when each was last modified (its
`dateModified`): what is stored of the others is kept, and only their
parents are read again, as a revision's parents also change when another
revision is edited to take it as its child. Phabricator says that a
revision was modified at each change to it, a new comment among them, but
not when one of its comments is edited or removed: the line counts such a
comment once the revision changes otherwise, or once a pull reads it. For
15 revisions none of which was modified, a fetch is three calls, as a
rule: the statuses' search, edge.search and differential.diff.search; one
modified adds its transaction.search, and its getrawdiff when its latest
diff is a new one. Once all of it is read, it is stored, under the
database's lock, as records of their own (One file, only appended to,
below): of each revision, only what differs from what is stored, its
state, its comments, or both, and its raw latest diff once for each diff
id; a revision found as it was stored is not written again, and one small
record says when the fetch read every revision. A fetch that fails
partway stores nothing. A submit stores what it reads of its revision
before anything is sent, and what it reads of it again before it sends
(Submitting a patch, below); once it is done, it fetches that revision
again, alone, its comments with it whatever its search says, so that its
line shows what Phabricator has once the submit is done, not what was
read before it; should that fetch fail, the bottom line says so after
what the submit came to. A pull stores what it reads of each revision as
a fetch stores it: its state and its comments, with when the revision
was last modified as its search said, each only where it is not what is
stored, the raw diffs it read, and that it read every one of them; the
next fetch reads none of it again while the revision is not modified
since. What
differs in the code of each patch is worked out away from the screen,
from what is stored, one patch at a time, the one the middle shows first,
once for each version of the patch on the patches below it and each
latest diff, and kept in memory as long as the editor runs: an editor
opened again on the database shows each line as it is stored, the code
compared again, and reads nothing of Phabricator. Nothing is read, and no
line drawn, while reviews come from nowhere, nor in the tour. In the
browser and in StackED, the same.

There are six keys, four more for the middle, and `o`, on a line of a
commit message. Of the four, `1`, `2` and `/` are self-review's (Copied
from self-review-rs, below, says what self-review is), and `3` is
stacked's own; `1` is also the key of the views of the interdiffs, on the
screens that show them. The settings can make any of them other keys, and
the help then names those:

- The arrows move. ↑↓ go through the patches (the header is above the
  first), the lines of the patch, or the comments (their header above the
  first); pgup, pgdn, home and end go further. A pane longer than the
  screen stays as it is while the cursor moves within it, and scrolls
  only once the cursor would go past its top line or its bottom one, as
  far as it takes to show what the cursor goes to whole (a patch whose
  title folds, all of its lines): from the top line going up, down to the
  bottom line going down. ← and → go from pane to
  pane, and past the side panes cycle what they show; in the unified view
  split in two (`1`, below), each half of the middle is a pane of its own
  for them. ← on the expanded
  patches (each its number and title, whole, folded under itself when the
  pane is too narrow) lists the files of the patch picked under it, in the
  order the middle shows them: a line for each file, its path, cut to its
  end when it is too long (`…/tui/banner.rs`), and, when the path leaves
  room for them, the lines the patch adds and removes in it (`+12 −3`).
  In the reading mode (Reading, below), each file says instead how many
  of its changed lines are left to mark read (`7 left`), or `✓` once all
  of them are read, and its path is cut, if it must be, to leave room for
  that. The pane is then as wide as the paths need, from as wide as
  expanded to a third of the screen, and keeps that width from patch to
  patch. ↑↓ go through the patches and those files as one
  list: on a file, the middle shows it at once, its name at the top; ↑
  above the first file goes back to the patch itself, from its start, and
  ↓ under the last picks the next patch, whose files are listed in place
  of the others. ↑ from a patch, or from HEAD's row, to the patch above
  picks it in the same way, its files listed in place of the others, and
  goes to the last of them, so that ↑ goes back up the lines that ↓ came
  down; when the patch above has no file to list, ↑ goes to that patch
  itself. Enter or → on a file goes into the middle there, and a click on
  a file goes to it. While the cursor is in the middle, the pane shows the
  file the cursor is in as picked, and ← comes back to that file. While
  one of a patch's files is picked, the patch's own lines, once scrolled
  past, stay on the pane's top lines, drawn as they are in their place:
  its number, its revision's status and mark when it has them, and its
  title, folded as it folds there. The files scroll under them, and the
  pane moves only once the cursor would go past the line under them or
  the bottom one. In a pane too short for all of them and a line for the
  file, the first of them stay, as many as leave that line (HEAD's line,
  when it is pinned, keeping its own), and none in a pane of one line. ↑
  above the first file shows them in their place again, the cursor on the
  patch, and a click on them goes to the patch itself. ← again
  hides the patches, for the middle to have the width, and ←
  from the middle brings them back expanded. → on the comments makes them
  compact, a line for each (who, and where it is); → again hides them, the
  bar saying how many there are; and → from the middle brings them back
  expanded, each with the first line of what was said last. → from the
  middle opens the comments pane whatever the patch has, the cursor on its
  header when it lists nothing: the header says what the filter is and that
  it lists nothing (`all · nothing listed`), and the line under it why
  (`This patch has no comments.`, `The filter shows the comments to review,
  and hides this patch's only comment: Filter comments…, on this header,
  picks another.`, `The filter shows the closed comments, and this patch
  has none: …`), as `?` does; Enter or Space on the header is its menu, the
  filter's item there. With nothing on the patch for it to show, the pane
  goes again as ← leaves it, so that the middle keeps its width, and → opens
  it again; one the filter empties stays, as there are comments to show.
- Enter does what is to be done where the cursor is, reading or commenting:
  on a patch, read it; on a file the patches pane lists, read the patch
  there; on a line, comment on it; on a comment, reply, or, while the agent
  works on it, open what it is writing; in the comments pane, go to the
  comment. Elsewhere it is the menu.
- Space is the menu: all that can be done where the cursor is, then undo,
  redo, `Runs…` and quit, which every menu ends with. The stack's items
  (comment on it, ask about all, `Filter comments…`, the stack as a list,
  the tree, the history, Phabricator, `Move HEAD…`, switch, delete) are on
  the left header only, but `Filter comments…`, which a patch's menu has
  too; a patch's on a patch; a line's on a line; a comment's on a comment.
  ↑↓ and enter; ↑ from the top is Quit. On the Phabricator line over a
  patch's message (with moz-phab, above), Space shows what differs from
  Phabricator, on a screen of its own, and Enter is the menu. What a
  read-only stack cannot do is shown dimmed, and the menu says why. An
  item may have choices, radio
  buttons after its label (`[x] tmp/file  [ ] pwd`): ← → pick one, and
  stop at either end rather than go round, so that ← held ends on the first
  and → held on the last; a click
  on one picks it, and Enter does the item with the one picked, which is
  kept for the stack in the database. Every `Ask agent…` item opens the
  ask dialog rather than asking from the menu (Asking the agent, below),
  and so does a review's `Send it back to the agent…`. An item that
  sends again what was asked before ends with a dropdown of the effort it
  goes at, `Effort [high]`: `Send it now` and the batch's items. On an
  item with more than one
  dropdown, ← → pick in the one underlined, which is the first until Tab
  goes to the next one; shift-Tab goes back, as the bottom bar says
  (`←→: to whom · tab: effort`). A click picks in the dropdown it is on:
  the choice clicked, or, on `Effort [high]`, the next level, round from
  `max` to `low`.
- `?` says what can be done where the cursor is, as `Press Enter to create a
  comment on this line.`, and where the arrows go. The next key closes it
  and does what it does, so the key it names can be pressed straight away.
  With no stack, it says nothing: the menu has nothing but Quit.
- Esc goes back: out of a menu, a list, a stream. In the panes, it goes to
  the banner first, when there is one (while a comment is open, or work
  waits), from wherever the cursor is; Esc there comes back where it was.
  Without a banner, it
  goes up, from the comments to the middle, to the patches (from a file
  they list, to its patch first), to the stack's
  header. In the comment editor, Esc asks what to do with the comment.
- Delete, and Backspace too (a Mac's delete key is Backspace, and its
  forward delete is fn-delete), does what the menu's item that closes,
  drops, stops or rejects what the cursor is on does. On a comment, in the
  middle or the comments pane, it is `Close this comment…`, or `Close it
  and the comments it opened…` when its answer opened some; while the agent
  works on the comment, it is `Stop the agent…`; while the comment's ask
  waits its turn, it is `Take it back from the agent's queue…`, which takes
  the ask out of the queue and leaves the comment open rather than closing
  it; and while the comment is in the batch, it is `Remove it from the
  batch`. On a patch, it
  is `Drop this patch…`; on the stack's header, it is `Delete this
  stack…`; on a file the patches pane lists under a patch, it drops the
  patch's changes to that file (Editing a patch by hand, below). In the
  review of landed work, it is `Mark as rejected` on a part, `Close it with
  the review` on a comment the ask was about, and `Take it back` or `Take
  your answer back` on what you wrote there; on a comment of a pull's,
  which is not rejected, it does nothing, and the status line says why,
  and on an operation of a pull's change it marks it rejected, as on any.
  On the banner, it rejects all that remains of the landed work picked
  and closes its review, as the review's `Reject all remaining and Close`
  does: of a pull's, its change alone, its comments filed, and nothing of
  a pull that brought comments alone; on a comment's line, it
  does what it does on the comment; on the stack's lint job, it is `Drop
  the waiting lint job…`, or `Stop the linter…` once it runs; on a
  reconcile with HEAD waiting its turn, it is `Drop the waiting reconcile
  with HEAD…`; on a pull from Phabricator waiting its turn, `Drop the
  waiting pull from Phabricator…`, and, reading Phabricator or being
  made, nothing, the status line saying why. On the runs, it stops an ask
  that
  runs, and drops a lint job, a reconcile with HEAD or a pull from
  Phabricator that waits its turn. An item ending with `…` asks first, and so does Delete; Delete also
  asks before it takes back what you wrote in a review, and before it
  rejects landed work from the banner. On a comment closed, it does
  nothing, and the status line says so. In the reading mode, on the code
  in the middle, it takes a mark back (Reading, below). It does nothing in a menu,
  nor in History…, the tree and the versions; and where what is typed is
  read (the comment editor, a line typed in a box, `/`'s bar, the search's
  own screen, the setup's `Add a command…`), Delete and Backspace edit it.
  The bottom bar says what Delete does where it does something (`delete:
  close it`), and so does `?`.
- `o`, on a line of a commit message, opens its link in your browser, as
  the line's item in its menu does: `Open https://example.org/a in your
  browser`, for a web link written in the line (`https://` or `http://`,
  what ends a sentence after it or a bracket around it left out; between
  angle brackets, `<https://…>`, all they hold; in the menu, a long one is
  shortened to fit the screen); `Open "the docs" in your browser`, for a
  markdown link, `[the docs](https://…)`, named by its text, its link up
  to the `)` that closes it (a text that is a link itself, or holds one,
  is not said: the item says the link it goes to); and, with the
  `moz-phab` feature, `Open bug 1234567 in your browser`, for a bug the
  line names (`Bug 123`, `bug123`, `b=123`, in any case), `bug` a word of
  its own, so that `debug 7` names none, and the rest as moz-phab reads a
  bug in a title (a submit reads a title as moz-phab does, `debug 7` its
  bug 7), on Bugzilla (`reviews.bugzilla`, Settings, below),
  and `Open D123 in your browser`, for the revision its `Differential
  Revision:` trailer names (the settings' `reviews.trailer`), at the
  trailer's link, or, a bare `D123`, on the settings' Phabricator
  (`reviews.url`). A bug written in a web link is the link's, not one of
  its own, as is one in a markdown link's text, and the trailer's link is
  one link, the revision. On a line with
  several, `o` opens the menu of them, `the links of this line`; on a line
  of the message with none, the status line says so; anywhere else, it does
  nothing. A right click on the line opens its menu, with the items; a
  click never opens the browser. A link opens away from the screen, with
  the program a comment's web link opens with (`$STACKED_OPEN`, else the
  system's own: Writing comments, below), and the status line says what
  came of it (`opened https://bugzilla.mozilla.org/show_bug.cgi?id=1234567
  in your browser`). It is so wherever the cursor is on a line of a
  message: the patch's message in the middle, commenting or reading; in
  the review of landed work, a patch looked at closer, and the lines of the
  messages in the interdiffs of the stack diff (one the new version takes
  out, the old version's); in History…'s stack diff, the same; and in what
  a submit sends to Phabricator, the lines of the title and summary, both
  sides' side by side. The versions of a patch have their cursor on a
  version: no line of theirs is under a cursor. The bottom bar says it where it opens
  something (`o: open bug 1234567`, `o: open the link`, `o: open a link`),
  and so does `?`; `open_link` in `[keys]` (Settings, below) makes it
  another key. A line of what a comment says has the same items in its
  menu (Writing comments, below), but `o` opens nothing there. In the
  browser (`stacked web`) and in StackED, the same.
- Every pane keeps where its cursor was, while the editor is open: the
  middle and the comments of each patch (whichever its version, so an edit
  keeps the line) and of the stack, the patches of each stack, the history
  of each stack, the runs, each transcript, each review; each half of the
  middle split in two keeps its own. Coming back to
  one, the cursor is where it was, on what it was on if that is still
  there, else on the closest row there is (for a comment closed or hidden
  meanwhile, the line it was on). The tree of edits, the stacks and the
  versions open on where the stack is, as before.
- `1` opens the menu of the views of the code, the one the code is drawn
  in marked `●`: Enter or a click picks one, and Esc leaves the view as it
  is. Unified and side by side are self-review's; compact draws the two
  sides each in a pane of its own with no gaps, scrolled together or one
  alone (← → turn which, and past the old side or the new one go on to
  the pane beyond); unified split in two is below. Every view but unified
  needs a middle of 60 columns (`split_width`). Picked on a narrower
  middle, a view makes its room, hiding as little as it can: it makes the
  comments compact, else hides them, else hides the patches (listing
  their files gives no room), the comments as they were, then compact,
  then hidden too. The status line says what was hidden and how to show
  it again (`the patches are hidden to make room for side by side: ← out
  of the middle shows them again`), and what was hidden stays so once the
  view is left. On a screen too narrow even with both side panes hidden,
  the view's item in the menu says so, and the view is not taken. A middle
  made too narrow later (a side pane shown again, the window made
  narrower) shows the code unified, and the top bar says that the view is
  too narrow (`· side-by-side: too narrow`) until there is room: picking
  the view again makes it. In
  every view, a line removed and the line added in its place (paired as the
  two sides pair them), when the two are almost the same, at least half of
  what they say, show what changed in them in the colours for words
  (`removed_word`, `added_word`), the rest of them in the lines' own. So do
  the lines of an interdiff, what changed from one version of a patch to the
  next (`Versions…`), and of the stack diffs (`History…`, the edits' `What
  this edit changed…`, the review of the agent's work), paired by their
  first sign; but not the lines of the message in them, its title and its
  body, which are compared word by word, where their lines wrap aside
  (History, below). Made
  shorter than the cursor's line, the screen keeps the cursor's line in
  sight, on its last line, in every view.
- On a screen that shows interdiffs, `1` opens the menu of their views,
  titled `the view of the interdiffs`: on the versions of a patch;
  History…, on its history and on its stack diff; the review of what
  landed, while its right pane shows the stack diff of its change (the
  cursor on its stack operations, or in the stack diff), not on a comment
  nor on a patch looked at closer; and the screens of what an edit
  changed (History, below) and of what differs from Phabricator (above).
  The menu is in sections, each what it draws under a heading, then the
  views it is drawn in, the heading no item: Enter, a click and ↑↓ pass it
  by. Its first section is the interdiff, `the interdiff: the two
  versions' diffs, compared`, in three views, each item saying how it
  reads: `Interdiff: a sign for which version has the line, then its own
  sign`, `A column per version: what each one's patch does with the line`,
  and `Side by side: the old version on the left, the new one on the
  right`. Its second is the change, `the change: each file from the old
  version to the new one, as a patch of its own`, in the code view's own
  views (below): `Unified: - for a line only the old version's code has, +
  for one only the new one's has`, `Side by side: the old version's code on
  the left, the new one's on the right`, and `Side by side, compact: each
  side's lines one after another, no gap where the other has more`. Its
  third is the lines each of those views draws, `the lines: in each view,
  every line or only what the patches changed`: `Every line: the context,
  the index lines and what the change of base made too`, as stacked always
  drew them, and `Only what the patches changed: no context, no index
  line, nothing the change of base made` (below), the ones drawn marked
  `●` as the view is, Enter or a click picking them as a view.
  The view the interdiffs are drawn in is marked `●`; Enter or a click
  picks one, and Esc leaves the view as it is; `?` on an item says what it
  draws. The view picked holds for every interdiff, on every screen, as
  long as the editor is open, the status line saying how it reads (`each
  interdiff is drawn in a column per version: what each one's patch does
  with each line, the old version's first`); the settings'
  `view.interdiff` is the one they start in (Settings, below), as an
  interdiff unless set. The screen keeps its place: the line its cursor is
  on, else its top line, a row showing two lines, the old version's and
  the new one's, being on the line it was on of the two, else on the old
  version's; switching back, it is on the line it was on before. Every
  other key does there what it does in each view: the arrows move or
  scroll, Esc goes back, Enter on the review's stack diff goes to the line
  in its patch as the work leaves it (of a row of two, the new version's),
  and `o` opens the links of the line of a message the cursor is on (of a
  row of two, both versions'). The bottom bar says `1: view` there, `?`
  says what `1` does, and the top bar says the view when it is not the
  first (`· interdiffs in columns`, `· interdiffs side-by-side`, `· change
  unified`, `· change side-by-side`, `· change side-by-side, compact`). The
  versions are named as the screen names them: `D12 on Phabricator` and
  `patch 2 as it is` on the screen of what differs from Phabricator; `v2`
  and `v3` on the versions of a patch; `@4` and `@12`, the stacks before
  and after, in History…'s stack diffs and on the screen of what an edit
  changed; `the stack's version` and `the work's version` in the review of
  what landed; and `version 3` and `this version`, in the review of a
  pull, for what a patch had since the version a pull takes.

  As an interdiff, as stacked always drew it, git range-diff's diff of two
  diffs: each line's first sign says which version has it, a space for
  both, `-` for the old one alone, `+` for the new one alone; its second
  is that version's own sign. The code says so once, over the interdiff,
  or, in a stack diff, over its first (`The first sign of each line says
  which version has it: a space for both, - for D12 on Phabricator alone,
  + for patch 2 as it is alone. The second is that version's own sign: +
  for a line its patch adds, - for one it removes, a space for one it
  leaves as it is.`); the lines are as they ever were.

  In a column per version, as git's combined diff draws a merge (`git diff
  --cc`): two columns before each line, the old version's first, each
  saying what that version's patch does with the line: `+` it adds it, `-`
  it removes it, a space it leaves it as it is; `=` the version has the
  line, a header or a line of its message, which no sign of a diff marks;
  `·` the version does not have it. The code says so once, over the
  interdiff or the stack diff's first (`The first column is D12 on
  Phabricator, the second patch 2 as it is. In each, + says that the
  version's patch adds the line, - that it removes it, a space that it
  leaves it as it is, = that the version has the line, a header or a line
  of its message, and · that the version does not have the line.`). Each
  line is worked out from the interdiff's: a line both versions have, what
  each does with it, the same (`  ` for a line both leave, `--` one both
  remove, `++` one both add, `==` a header both have). A run of lines that
  one version alone has, between two that both have, holds those of the
  two versions that are one line: the same text after their own signs,
  matched in their order, a diff of the run's lines of each version,
  never across a line both have, which would move a line past it. On one
  base, a line a patch adds (`+`) is never one of the base (a space or
  `-`), which is another line there; on two bases (below), it is the line
  the other's base has, moved there, as when part of the patch went into
  a patch below it, or came out of one. Such a line is drawn once, where
  the old version has it, what each does with it (` -x`: the old version
  leaves it, the new one removes it; `+ x`, on two bases: the old version
  adds it, the new one has it before its patch).
  Two hunk's headers that differ only in their ranges are one header, both
  versions' ranges in it, the old one's first (`@@ -15,7 +15,7 → -17,7
  +17,7 @@`). The rest of the run, the old version's first: a line the one
  version adds, which the other does not have (`+·`, `·+`); a line of the
  base the one version's diff alone shows, keeping or removing it, which
  the other leaves as it is when the two are on one base (`- `), and is
  not known to have when they are not (`-·`): on two bases, as when a
  patch below changed between the two versions, or when the screen of what
  differs from Phabricator says that the latest diff is on another base,
  a line of the base of one is not a line of the other's, and the views
  say `·`. Two diffs are on two bases when what each says of the files
  both change, before the patch, differs (a line at the same place is
  another, the same hunk starts elsewhere, or one makes a file the other
  finds there), as the screen of what differs from Phabricator tells
  them; each version's diff is read so only when a line of the base shows
  in one alone. A header, the title and the body of the message, the
  files' headers and git's `\` line: `=` where a version has it, `·` where
  it does not. The interdiff's own hunks' headers are as the interdiff
  cuts them, alone on their lines. Its colours: a line both versions have
  and do the same with dim, in its own sign's colour as a diff draws it,
  so that what differs stands out; any other line as any text, each column
  in its own colour (`+` as a line added, `-` as a line removed, `·` dim,
  `=` and a space as any text), but a line of the message compared with
  the other version's, word by word, where their lines wrap aside, its
  words only one version has in the colours for words (`removed_word` the
  old version's, `added_word` the new one's), as an interdiff compares two
  messages (History, below); a header the two have otherwise (`Date:`),
  what changed in it so too, as an interdiff pairs it; and a line one
  version adds beside the one the other adds in its place (`+·` then
  `·+`), the two almost the same, at least half of what they say, what
  changed in each in the colours for words, as the interdiff pairs them.

  Side by side, after moz-phab-diff's split view: the old version on the
  left and the new one on the right, each half under a heading of its
  version, over each interdiff (`D12 on Phabricator │patch 2 as it is`),
  the interdiff's lines paired as the code view's side by side pairs a
  diff's (`rows.rs`, copied from self-review-rs): a line both versions
  have on both sides of one row; a run of lines the old one has beside the
  run the new one has after it, row by row; a line only one has alone in
  its row, `~` dim on the other side. The interdiff's hunks are as it cuts
  them, their headers across both halves. Each cell is its version's line
  as a diff draws it, its own sign once, with no sign of the interdiff: a
  line of the diffs in the colour of its own sign, as the code view
  colours a diff's lines (`added`, `removed`, a hunk's header in its own);
  a line of the mail, its headers and its message, as the interdiff draws
  it. A row both versions have is dim, in its own colours, so that what
  differs stands out. In each half, a line its version's own diff removes
  and the one it adds in its place, almost the same, show what changed in
  them in the colours for words, as the code view pairs a patch's lines,
  however much of the version's hunk the interdiff shows; a line of the
  message is compared across the two halves instead, word by word, as the
  interdiff compares two messages. A line too wide folds in its half, a
  column short of it, as the code view's side by side folds its lines: a
  line of code where the half ends, each piece after the first `↪` in
  place of its sign, and any other between words, a bullet's pieces under
  what follows the bullet. Side by side needs a pane of 60 columns, as the
  code view's side by side needs a middle so wide (`view.split_width`): a
  pane narrower than that draws its interdiffs as interdiffs meanwhile,
  the view picked kept. Picked on such a pane, side by side is the view
  all the same, of every pane wide enough, and the status line says why
  this one does not draw it (`each interdiff is drawn side by side where
  its pane has 60 columns: this pane has 59, so its interdiffs are drawn
  as interdiffs until it is wider`); `?` on its item says so before it is
  picked; and while the pane is narrower, the top bar says `· interdiffs
  side-by-side: too narrow`, and `?` says why the interdiffs are drawn as
  interdiffs. Such panes are the review's right pane and History…'s stack
  diff on a screen of 100 columns or fewer (→ again, in the review, gives
  its pane the screen's width), the versions' on one of 80 or fewer, and
  a screen of its own under 60. In the browser (`stacked web`) and in
  StackED, the same.

  The change, the second section: the change from the old version to the
  new one as a patch of its own, each file either version touches as the
  old version leaves it on its own base, then as the new one leaves it on
  its own, as a diff; nothing is made again on another base. It is drawn
  as the code view draws a patch: unified; side by side, its rows paired,
  `~` dim where a side has nothing; compact, the old side's lines in the
  left column and the new side's in the right, each side's one after
  another, with no gap where the other has more, the two scrolled
  together (← → keep their meaning on these screens). The code says so
  once, over the change, or over a stack diff's first (`The code is the
  change from v2 to v3 as a patch of its own, each file as v2 leaves it on
  its base, then as v3 leaves it on its own: - for a line only v2 has, +
  for one only v3 has. Where the messages differ, their lines are over the
  code, - for those of v2, + for those of v3.`). It is of the code: the
  title and the message are compared as the interdiff compares them, the
  interdiff's lines of the mail over the code, as an interdiff draws them,
  where they differ. Each version's base is what its screen knows of it:
  on the versions of a patch, the stack that first held each version, its
  base commit and the patches below it there; in History…'s stack diffs,
  and on the screen of what an edit changed, the stacks before and after;
  in the review of what landed, the stack as it is and as the work leaves
  it; in the review of a pull, the version a pull takes on the stack that
  first held it, and this version on the stack as it is; on the screen of
  what differs from Phabricator, the revision's latest diff on the commit
  it is on, as Phabricator names it (Phabricator, below), as git names it,
  else, a Mercurial changeset, as git-cinnabar has it (`git cinnabar
  hg2git`), and the patch as it is on the stack's base, the patches below
  it applied. The files are read from the repository alone, as `3` reads a
  patch's files whole, in an index of stacked's own, never a checkout, away
  from the screen (`Reading the files of both versions…` meanwhile), two
  changes at a time, each once while the editor is open. Where a base
  cannot be read (a stack with no base, a commit the repository lacks, a
  version never in a stack, a latest diff whose commit Phabricator did not
  say, or stacked did not read yet), the section's items cannot be picked,
  and the menu says why (`The change cannot be drawn here: the commit D12
  on Phabricator is on is not known: Phabricator said none, or it was
  fetched before stacked read that.`); the first section keeps working.
  Where a version does not apply on its base, the change says why, the
  interdiff after it, as an interdiff (`The change cannot be drawn: v2 does
  not apply on its base, 0123456789ab, with the patches below it. Here is
  the interdiff instead.`); two versions that leave each file they touch
  the same say so (`The two versions leave each file they touch the
  same.`).

  Where the two bases differ in a file either version touches, the code
  also says what dim is (`The two are on other bases: a line drawn dim is
  one the change of base made, not either patch, one taken out that the
  base of v2 has and the base of v3 has not, or one put in that only the
  base of v3 has. A file only the change of base touches is left out.`),
  and a line of the change is drawn dim, over its own colours, where the
  change of base made it: a line taken out that the old version keeps of
  its base and the new base has not, while the new version takes out
  nothing the new base has in its place; a line put in that the new
  version keeps of its base and the old base has not, while the old
  version takes out nothing the old base had there. Each version's files
  are matched line by line with its base's (a file with the one it is
  renamed or copied from), and each file of one base with the other's, a
  diff of their lines, each run of lines one base has and the other has
  not a place the change of base changed. Every line either patch puts in,
  or takes out, keeps its colours; so does a line where a patch took out
  what the change of base changed (the old version took out `x`, the new
  base has `X` there, and the new version keeps it), as which of them made
  it cannot be told. On one base, nothing is dim. A line moved into a patch
  below, or out of one, is no change, both versions leaving the file the
  same; a file the change of base adds, deletes or renames is each file as
  each version leaves it, by its path, the base's lines dim and each
  version's own as ever; a file one version touches and the other does
  not, as that other leaves it on its base. A file only the change of base
  touches is left out.

  `3` there, the settings' `whole` key as in the middle, shows each file of
  the change whole, rather than its hunks, the lines in no hunk dim as the
  middle draws them, then its hunks again, held while the editor is open:
  the bottom bar says `3: whole` (`3: hunks`), `?` says so, and the top bar
  `· whole files`. Side by side and compact need a pane of 60 columns, as
  the code view does (`view.split_width`): a narrower one draws the change
  unified meanwhile, the status line, `?` on the item, `?` and the top bar
  saying so as for the interdiff (`the change is drawn side by side where
  its pane has 60 columns: this pane has 59, so it is drawn unified until
  the pane is wider`, `· change side-by-side: too narrow`). From the
  interdiff to the change, and back, the screen keeps its place: each line
  of the change has the line of the interdiff nearest it, by its file and
  its line as each version leaves it, a line a version keeps before one it
  takes out there; from the interdiff, the cursor goes on the first line of
  the change nearest its line; from the change, on that line of the
  interdiff; from one view of the change to another, on the same line of
  it; and back where it came from, on the line it was on. A change read
  once the screen is drawn, the cursor goes on to the line it was found
  for. Enter on a line of the change in the review's stack diff goes to the
  line of its patch the line of the interdiff nearest it goes to.

  The lines, the third section: every line, or only what the patches
  changed, in every view of both sections, on every screen of interdiffs,
  as long as the editor is open; the settings' `view.interdiff_lines` is
  what they start with (Settings, below), every line unless set. Picked,
  the status line says what is drawn (`only what the patches changed is
  drawn: of each interdiff, no line both versions have alike, no context
  line and no index line, and of the change, no line the change of base
  made; a line says how many are left out at each place`; `every line is
  drawn: …`), the top bar says `· only what changed`, `?` says what is left
  out and that `1` draws every line again, and `?` on each item what it
  draws; the bottom bar names `1: view`, as for the views. In the
  interdiff's views, as an interdiff, in a column per version and side by
  side, what is drawn is the lines where the two versions' own `+` and `-`
  lines differ; the lines of a file's header that differ, as where a file
  comes or goes, but its `index` line and its similarity, which git derives
  (the screen of what differs from Phabricator leaves them out of the diffs
  it compares); the messages compared, each line of the mail that differs;
  and over those, the hunk's header and the line naming the file (its
  `diff --git`) each is under in its version's diff, where the interdiff
  has them, though it leaves out the lines between, never a header of
  other lines. Left out: the lines both versions have alike, the
  interdiff's own context; every context line of the two diffs, a line
  whose own sign is a space, whichever version has it; the index lines;
  the interdiff's own hunks' headers; and a header with nothing kept under
  it. Two hunk headers of the two versions that differ only in their
  numbers, as when a change of base moved them, are drawn once, as a header
  both have, with the new version's numbers, each version's lines before
  its own over it (in a column per version `==@@ -5,6 +5,8 @@`, not the
  line with both ranges); two that differ in what follows their numbers
  too are each kept over its own lines. Each place lines are left out, a
  line says how many, dim (`⋯ 12 lines are left out here.`, `⋯ 1 line is
  left out here.`), counting the lines the view draws there: side by side,
  a row of the two versions' lines is one; of a run of lines each version
  has alone, those it keeps are together, under the line counting its
  others with those left out before it, as `-+old` over `++new`. The code
  says what is left out once, after how to read the view, side by side on
  its own over the heading (`Only what the patches changed is drawn. The
  lines both versions have alike, the context lines of their diffs and
  their index lines are left out, and where lines are left out, a line
  says how many. Two hunk headers that differ only in their numbers are
  drawn once, as the new version has them.`); an interdiff with none of
  its lines kept says so in their place (`The two versions differ only in
  context, line numbers or index lines, which are left out.`). In the
  change's views, unified, side by side and compact, the lines the change
  of base made, those drawn dim, are left out, and so are a hunk, and a
  file, with nothing else; each place says how many as the interdiff's
  do, counting the lines of the change, a run's left out over the lines it
  keeps; the code says so rather than what dim is (`The two are on other
  bases. Only what the patches changed is drawn: the lines the change of
  base made, not either patch, are left out, …, and so is a hunk or a file
  with nothing else; where lines are left out, a line says how many.`); a
  change left with nothing says so (`The two versions differ only in what
  the change of base made, which is left out.`); on one base, nothing is
  left out there. The screen keeps its place from every line to only what
  the patches changed and back, as from one view to another: the line its
  cursor is on, else, a line left out, the line kept nearest it, of two as
  near the one after; back, the line it was on. The lines left out are not
  drawn at all, so the arrows and the wheel go over what is. In the
  browser (`stacked web`) and in StackED, the same.
- Unified split in two shows the patch picked twice, side by side, each
  half a unified view of its own with a line between them, to read one
  place of the patch while going through another: a function's definition
  in one half and its callers, searched, in the other. Each half has its
  own cursor and scroll, its own search (`/`), its own reading (`2`) and
  its own hunks or files whole (`3`); `1`, `2`, `3` and `/` act on the
  half the cursor is in, and the other half draws its cursor as a pane
  draws what it has picked (`picked`). ← and → go from half to half as
  from pane to pane: from the patches to the left half, then to the right
  half, then to the comments, and back; Esc in the right half goes to the
  left one, as ← does. The left half is beside the patches and the right
  one beside the comments: what a side pane does in the middle (a file
  gone to, a comment gone to, the search opened there), it does in the
  half beside it. A click in a half, or the mouse over it, puts the cursor
  there, on the row under it. Comments, replies and read marks are made
  from the cursor's half, and Enter, Space and the menu act on its row;
  what is written shows in both halves, and what is read in each half
  that is reading. Another patch picked shows in both halves, each where
  it was on that patch, else from its top. Another view picked keeps the
  half the cursor is in as the middle, its cursor and its reading and
  files whole with it. `view.layout = "unified-split"` starts in it
  (Settings, below).
- `2` switches between commenting and reading (below).
- `3` switches between the hunks and each file whole: every file the patch
  changes as the patch leaves it, the lines it removes in their place, with
  no hunk's header and nothing left out. The lines of the hunks are the
  rows the hunks have, keyed as they are, so that comments, read marks and
  references are on the same lines both ways. A line in no hunk is drawn
  dim, as self-review draws the lines it adds around a hunk, and takes no
  comment and no read mark: Enter and Space there open the menu, as on any
  line no comment goes on, and the status line says why. Side by side, the
  old file is whole on the left and the new one on the right, lined up as
  the hunks are; compact, each in its pane. A file the patch adds is all
  added, one it deletes all removed, one it renames whole under its new
  name. The files are read as the reading mode reads them, from an index of
  stacked's own (no checkout, the repository only read), once for each
  patch and away from the screen: meanwhile the hunks show, and the bar
  says `whole files: reading…`. A file that is not text shows as its diff
  says it; a file of more than 20000 lines, and one that would take the
  patch past 50000 lines shown whole, show as their hunks, a line over them
  saying so.
- `1`, `2` and `3` go together: each view of the code works in commenting
  and in reading, with the hunks or with each file whole, and split in
  two, each half on its own. Pressed in any
  order, they leave the cursor on its line, on its line of the screen (as
  far down as the rows over it reach). Into each file whole, a hunk's
  header leaves the cursor on the hunk's first line; back to the hunks, a
  line in no hunk leaves it on the closest line of its file in a hunk, by
  their numbers, a tie going to the line above, and the status line says
  so.
- `/` searches what the middle shows, as in self-review, whatever pane the
  cursor is in. What you type goes in the bottom bar. As you type, the
  cursor goes to the first line that holds what you typed, from the line
  it was on down (with none there, from the top), and when that line is
  out of sight, the middle scrolls to put it a third of the way down. The
  matches in sight are lit, reversed from how their lines are drawn; those
  of the line the cursor is on are bold and underlined too. The bar says
  which match the cursor is on, of how many (`match 2 of 5`), and when the
  search started again at the top or at the bottom; or it says that
  nothing matches. Tab goes to the next match, and shift-Tab to the one
  before (`find_next` and `find_previous` in `[keys]`, Settings, below).
  Enter ends the search on the match, the cursor in the middle.
  Esc ends it with the cursor back where it was, in the pane it was in,
  and the middle scrolled as it was; so does Enter when nothing matches,
  and the status line says so. A line matches when it holds what you
  type, whatever its case, as in self-review and in the search's own
  screen (Searching, below): plain text, the blanks at its ends left out.
  A line of code is searched without its `+`, `-` or space, and a line
  folded as the one line it is, the cursor going to the piece that holds
  the match; the rest is searched as the middle shows it: the files'
  names, the hunks' headers, the message, the comments. The search works
  in every view of the code, reading or not, and with the hunks or with
  each file whole (`3`); split in two, in the half the cursor is in, and
  Esc puts that half back where it was. It searches the lines the middle
  shows, so that
  with each file whole it finds the lines no hunk has too, and the cursor
  goes to them. The searches that Enter ends on a match are kept, the
  latest first, twenty at most, in `state.json` (Settings, below), so that
  the next run has them too. `/` opens with the last one in the bar,
  selected, its matches lit: what you type takes its place, Tab goes on to
  its next match, and Enter stays on the match the cursor is on; ↑ and ↓
  go through the searches before it. While the bar is open, Enter, Esc, ↑
  and ↓ are those keys whatever the settings make them, and every key but
  the two going from match to match is typed or edits what is typed:
  Space, `?`, `1`, `2` and `3` too.

Nothing else is a key. Going through the comments pane does not move the
patch: the comment is marked where it is if the patch shows it, else its
thread is shown over the top or the bottom of the patch, as it is above or
below. Enter goes to it.

A line too wide for the middle is folded, as comments are: a line of the
message between words, its indent kept and a bullet's lines under what
follows the bullet; a line of code where the pane ends, each piece after
the first `↪` in place of its `+`, `-` or space. Side by side, each side
folds in its half; split in two, each half as wide as it is. A line folded
is still one line: the cursor goes
through its pieces, its comments go under its last piece, reading marks
and counts it once, and side by side its pieces go beside the line the
other side has there.

Moving a patch is ↑↓ to place it, enter to put it there, after the menu's
`Move this patch…`. If the patch does not apply at that place, stacked
refuses, and the patch is still being moved: the status line says why, and
that ↑↓ place it elsewhere and Esc leaves it where it was. Dropping a patch, deleting a stack or a comment,
applying, and stopping an ask ask first, in a menu of their own; for what
cannot be undone, the menu starts on `Keep it`. ctrl-c quits, as it does
everywhere.

The screen never waits on work done elsewhere: an ask is started (its
worktree made) away from it, and so are pulls, applies, a new database
written, and the check of a stack someone else made; the bar says
`pulling…`, `applying…`, `checking…` meanwhile, and what came of it when it
is done. While the agent writes, only what it writes is drawn again.
