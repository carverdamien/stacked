# Tests

The full check of a change to stacked, before it is merged, is clippy with
`--all-targets` over five feature sets, `web,demo,moz-phab,try-push`,
`app`, none, `try-push` and all five, then the test suites over the same
five, and once more for `web,demo,moz-phab,try-push` with
`STACKED_TEST_CHECKPOINTS=1` (below). The [README](../../README.md)'s
Building and testing gives the same commands, in the same order; a change
to them is made in both:

```sh
cargo clippy --all-targets --no-default-features --features web,demo,moz-phab,try-push
cargo clippy --all-targets --no-default-features --features app
cargo clippy --all-targets --no-default-features
cargo clippy --all-targets --no-default-features --features try-push
cargo clippy --all-targets --no-default-features --features web,demo,moz-phab,app,try-push

cargo test --no-default-features --features web,demo,moz-phab,try-push
STACKED_TEST_CHECKPOINTS=1 cargo test --no-default-features --features web,demo,moz-phab,try-push
cargo test --no-default-features
cargo test --no-default-features --features try-push
cargo test --no-default-features --features web,demo,moz-phab,app,try-push
cargo test --no-default-features --features app
```

Run `cargo test` after any change. With every feature on by default,
`cargo test` runs every test: the browser's, the tour's, Phabricator's,
StackED's and the try push's too (the web feature needs the wasm target,
as in Install).
`cargo test --no-default-features` runs the minimal set, the tests of
stacked built with no feature, and needs no wasm target; `--features` then
adds the tests of each feature named. No test runs claude: a script stands
in for it; nor a linter of yours, nor any `./mach`: a script stands in for
the linter; none reaches Phabricator: its revisions and their comments are
read by stand-ins, and what a submit fetches first and sends is answered
by one, which keeps every call made to it, so that a test can see that a
review read nothing, that a send made its writes alone, and that a
refusal wrote nothing, and which, as Phabricator does, makes no revision
of a diff whose commit has no tree; what a fetch reads of a stack's revisions is answered
by one set for the test's database alone, so that no test's stand-in
answers another's editors; and in a test's own process, which keeps
nothing, an editor with no stand-in reads none, and shows nothing of what
is stored. How a call of Conduit is made is seen with a stand-in for
curl, a script that `sh` runs, which keeps the arguments it is given and
what it reads, and answers as the test says; and with curl itself,
sending to a server that the test runs on the loopback (127.0.0.1), so
that nothing leaves the machine. Nor does the ask dialog reach the network: whether it reaches
the agent's host is answered by a stand-in set for the test's database
alone (reached, not reached, or only once the test says), and in a test's
own process an editor with no stand-in checks nothing. None opens a window, nor a browser: a script stands in
for the program that opens a web link, and only writes down the links it
is given. No sleep decides what a test sees:
when a test looks at the agent's stand-in at work, the stand-in goes on
working until the test lets it finish, however busy the machine is.

Nor does any test reach what is yours to stacked: ~/.stacked,
~/.stacked.toml, StackED's state in ~/Library/Application Support. In each
test file, one function gives every run of the binary a folder of the
test's as `$STACKED_HOME`. The tests' own processes are never set up as
the binary's start sets one up: their settings' file and their state are
the null device, so that they read no settings but the defaults and keep
nothing, and the asks they start are told the same. `tests/cli.rs` checks
the first: stacked, run as the tests run it in a home holding the files of
before, reads its settings from the test's folder and leaves that home as
it was. `tests/tui.rs` and `home.rs` check the second. The tests of the
move, and of the settings in ~/.stacked, give stacked a home of the
test's.

Nor does any test write in, or remove anything from, the folder of the
agents' rebase plans in the temporary directory, which every stacked
shares. In `tests/cli.rs` and `tests/try_push.rs`, whose asks a run of
the binary starts (a try push's keeps the tasks mach can push and its copy
of the task graph there),
the function that gives every run of the binary its home gives it a
folder of the test's as `$STACKED_TODO` too. The tests'
own processes keep the plans beside their database, in the test's own
folder, and the asks they start are told the same. In a folder of their
own, the tests in `ask.rs` show which plan folders are removed: one whose
git directory is gone, and one that names no git directory once nothing in
it has changed for a day; not one whose git directory is there, rebase or
not, nor one that an ask still running writes its plan in. They show a
landed ask's folder removed, but not while a rebase is under way in its git
directory, nor while another ask still running writes its plan there.
`tests/cli.rs` shows the folder gone once an agent's change to the whole
stack lands; `tests/try_push.rs`, a try push's selection's, one for each
ask, gone once it lands or is stopped.

Nor does any test leave anything else in the temporary directory, which
every process shares. In each test file, the function that gives every run
of the binary its home gives it another folder of the test's as `$TMPDIR`,
so that what stacked writes in the temporary directory (an edit it could
not take, kept for `--from`) is removed with the test's folder. The
editor's tests write their stand-ins, for claude and for the program that
opens a web link (with the links it was given), in the folder Cargo gives
the tests for their files. The folder of a test that asks the agent is
removed only once its asks are over, with no process working in the folder
any more: an ask the test left going would write in the folder as it is
removed, and leave some of it.

No ask waits for a setup, so no test sets one up first. In
`tests/cli.rs`, the test's home holds a settings' file of its own
(`[agent]` alone), which a test adds to; the editor's tests share settings
of their own (`settings()` in `tests/tui.rs`), the defaults. The tests of
the first ask start from settings of before that say the setup was done
(`agent.setup`), to see that setting passed over.

With `STACKED_TEST_CHECKPOINTS=1` set, every write writes a checkpoint, and
every database read from one is read whole too and the two compared, a
difference a panic: run the suite so too after a change to the database,
or to what reads it, as `STACKED_TEST_CHECKPOINTS=1 cargo test`. The tests
in `db.rs` show where stacked left HEAD kept per stack and checkout, each
move replacing it and the same again writing nothing, read from a
checkpoint as when read whole, and a checkpoint of before, with no index
of them, read all the same, as one with none; what is read of Phabricator
stored only where it differs from what is stored, each part of a revision
on its own, its state, its comments, a raw diff once for each id, a fetch
always, what stands each part's latest, a revision's PHID named, read from
a checkpoint as when read whole, the raw diff no state names not read
with it, and a checkpoint of before, with no index of them, read as one
with none; a state saying whether its revision is a draft and its
repository, one stored before saying neither; a repository read of
Phabricator stored once, found by its PHID or by its callsign, read from
a checkpoint as when read whole, a record of its own; a submit of a
revision a record of its own, by its link, with when, standing while no
read of the revision is stored after it, whatever their times, read from a
checkpoint as when read whole, a read after it, of its state, standing no
more, no record taken out, and a submit again standing again; a trim of
what there is now keeping no submit a read came after, and a trim from a
cut keeping the one that stood there, with when, a read after the cut
coming after it, as in the old file; a checkpoint of before with no index
of the submits read as one with none, a submit stored after it; and the edits since a state
of a stack counted along its tree of edits by the index of the stacks
alone (another version once an edit is undone and another made in its
place, nothing for what is no state of it), read from a checkpoint as
when read whole, its history not read. They show a reader that catches
up, reads the file anew, or reads it whole as a trim does, while a writer
writes its record in two halves under its lock, a pause between them
(a hook of the tests'): the reader waits, and reads the record whole,
never half of it; and a thread that holds the lock to write reading the
file again under it, with no shared lock asked for, which would wait on
the thread for ever.

The tests of the patches that should not change (Phabricator, above), with
moz-phab, store their revisions' statuses in the test's database, as a
fetch stores them, and reach no Phabricator. In `protected.rs`, Accepted and
Landed alone protect a patch, and the refusal says each patch, its
revision, its status and what that means, how long ago their statuses were
fetched, at once or apart, what to type, the button, and what the bottom
line says once accept is not typed. In `db.rs`, a write that changes such a
patch, its diff or its message alone, is refused as an error of its own
kind naming it, nothing written, `Db::edit` saying it as its error, the
first write of an edit refused leaving the next unmade, both patches of one
write named in order; confirmed for another patch, refused all the same;
confirmed for exactly those, written, for that edit alone, the next edit of
its new version refused again; one Changes Planned or Needs Review changed
at once; such a patch moved, its version as it was, dropped, given another
author alone, and its stack deleted, with no one asked; undo, redo and a
stack picked from the tree refused as any write, then written once
confirmed, a delete undone changing none; and a patch brought to its
revision's latest diff as stored raw, or to its title and summary, or to
both, told apart from one with another diff or another message. In
`tests/tui.rs`, for Accepted and Landed each: a move and a drop ask
nothing, nor do they of Changes Planned and Needs Review; undo and redo,
a version put back from the versions of a patch, and a stack picked in the
tree of edits open the dialog, which stays with `accep`, `Accept`, `
accept` or `accept ` typed, Enter and a click on its Change button doing
nothing but saying why, nothing written; Esc cancels it, the bottom line
saying why, nothing written; `accept` writes it, for that one operation, the
next undo asking again; Changes Planned and Needs Review write at once. An
edit by hand asks before $EDITOR opens, of the patch as mail, of a file
of it, of the stack as a list, and from the review of landed work;
cancelled, $EDITOR does not open; typed, it opens, the edit lands, and its
review closes with no one asked again; a file of the patch under it asks
nothing, its review closing so too, the patch above as it was; an edit of
a file not taken, as the patch above it no longer applies on it, asks
nothing, and its conflict resolved by hand asks of that patch, cancelled
giving back the edit's menu, then nothing more, its review closing so too;
Changes Planned and Needs Review give $EDITOR at once. A review of a change of such a patch, closed,
asks first, the review staying open while cancelled; from the banner too,
the main screen staying, the review closed from there once accept is typed.
The agent's work on the patch under it closes with no one asked, the patch
above as it was, or rebased unchanged, a version whose hunk is a line down;
one that changes a line of its context asks, as one that changes the patch
itself does, the review staying open while cancelled. In `db.rs`, such a
patch rebased so is written with no one asked, and one rebased with a line
of its own, a line of its context, or its message written otherwise is
refused; in `protected.rs`, what is rebased unchanged and what is not, a
binary file's change being so only as the same text. In `tests/cli.rs`,
`send --accept` takes the agent's work on the patch under it, its lines
moved, with no terminal to ask on, and refuses its work on that patch,
which waits for review and is taken on a terminal once accept is typed.
A pull that brings such a patch to its revision's latest diff closes with
no one asked; one that makes it again on the pulled version under it asks,
Accepted or Landed. The dialog draws its title, each patch's line, how long
ago their statuses were fetched, apart, what to type, the field with the
terminal's cursor after what is typed, the buttons, and what keeps Change
them grayed, gone once accept is typed; the mouse over a grayed Change
them leaves the cursor in the field, over it once typed puts it there, and
a click on it changes them; a click out of the dialog, and one on Cancel,
cancel it; ↓ goes to the buttons, ← stays on Cancel while Change them is
grayed, and Enter there cancels. In `tests/cli.rs`, `stacked undo` of such
a patch is refused with no terminal, saying why and which patch, nothing
written; with reviews coming from nowhere, undone at once; on a terminal
of its own (a pseudo-terminal), it asks there, `Accept` typed refused, and
`accept` typed undone.

The tests of the launch lock, in `ask.rs`, show it on a file of its own
beside the database, taken again by the thread that holds it, the
database's lock taken under it, another thread waiting until it is given
back; and asked for under the database's lock, a panic, as the lock
order is broken. In `tests/cli.rs`, two `stacked send`s each send a
comment at the same instant, twelve times: each process waits at a gate
the test sets (`$STACKED_TEST_GATE`), and the test lets both go together
once both are there; every other time on a stack of its own, whose
worktrees the asks make, else on one whose worktrees they take again. The
first start is then held midway, its worktree taken, the launch lock its
own (`$STACKED_TEST_HOLD`, a hook of the tests'), and the other process is
seen waiting for that lock, with no second start under way, before both
are let go. Every ask starts and lands, none fails, and each comment is
answered.

`tests/cli.rs` builds a repo and drives the binary. It covers a round trip
through `git am`, a stack imported again (a patch whose lines a new
version of a patch below moved keeping its own version, and its diff as
stored; a patch whose hunk moved to another copy of its lines a new
version), `stacked interdiff` of a patch reworded from the list that only
wraps a paragraph otherwise (the paragraph as the new version wraps it,
no line taken out or put in), the tree, recovery from broken lines, comments on the
stack, a patch and a line, every way of asking about them (a script stands
in for the agent and rewrites commits with git, with a plain `git rebase
-i` that takes the plan written to the todo file the prompt names, and
none without a plan or with one that runs a command, claude told what to
refuse; runs `stacked fits`,
answers, opens comments, tests), every ask in a worktree of its own (your
checkout's HEAD, branch and files as they were while the agent works and
after; a settings' file of before that says `pwd` read as `ondemand`, and
said; `stacked config` refusing it; the asks a stacked of before started in
your checkout landing, or found dead, the checkout left as the agent left
it, never removed; no `apply`, the applies of before listed), a split
following its comments, a comment
moved to the stack, a patch's comment asked of the whole stack adding a
patch on top, a comment written with `comment --work` on a patch the
agent's work adds keeping that work from being accepted or rejected, sent
back with it and asked of the next round, on that patch, which accepted,
holds the patch, the comment and its reply, work that brought nothing back held for review while a
comment it was asked about is open (and not once every one is closed), the
last reply taken at once closing its work's review with its comment open
or not, a reply going with a comment the agent opened not taken at once, a
reply written again, a
reference no longer named detached by `stacked reply --edit` and the
agent's reference by `--edit --turn` (said so), and a rewind keeping one
never named, a web link attached told the agent as one it need not fetch
(the prompt asking it to point at the stack first) and listed with its
comment, the agent's own link kept in its reply (one not of the web left
out, with its number), asks waiting their turn, an answer made on the
stack before a change accepted asked again on the stack it makes once the
change's review marks it restart (and given what it was allowed for this
work, what it allowed from now on saved), an agent stopped with
`stacked stop` (by the start of its run's id, the shell offering only the
runs that run) and killed as it will not exit on SIGTERM, its ask landed
at once as failed and said so on its comment and in its transcript, a
second stop refused, the effort each ask goes
at (`--effort`, else `high`, an effort an editor of before kept in the
test's home no longer read; the settings' own taken out; a level claude does not
take refused, nothing sent; a batch at the effort its comments were
batched at, and work sent back at the effort it was asked at unless
`--effort` says otherwise; the model each ask goes with (`--model`, else
none of its own; a batch with the model its comments were batched with,
or the one given; work sent back with the model it was asked with unless
`--model` says otherwise; an id that is not one of the dialog's refused,
the ids said, nothing sent; `--model` refused in a review without
`--send-back`), and the model said by `stacked landed` and `stacked
runs`; the effort an ask was asked at said by
`stacked landed`, `stacked runs`, `stacked comments` and its transcript,
on the comment its reply waits on, on its reply and on the comment it
opened, and kept on the fence as the thread is written again, a turn of
before as ever), a rebase, a patch whose lines a new version of
a patch below moved keeping its version and its diff as stored when the
agent's rebase rewords another patch (the stack diff marking only the
reworded one), the agent's change of such a patch, its commit left as it
was or made again, landing with nothing to accept, the patches below and
above the one the agent's rebase rewords keeping their versions, their
messages and authors as stored, when git gives those back otherwise
(empty lines in a row, under the title as a sync of before had taken them
from the checkout, white space at the end of a line or of the
title, a first paragraph of two lines, carriage returns, a prefix or
runs of spaces in the title, a newline and empty lines at the end, a
line of three dashes, a line `git am` takes for the author, a name
format-patch quotes), a message the agent changed in its white space
alone a new version, reworded, a patch with such a message that the
agent left as it was landing with nothing to accept, and one whose code
it changed keeping its message as stored, a patch with two
copies of its lines in its file a new version when its diff as stored
would go to the other copy (lines added between the two by a patch
below) and when the agent moved its hunk to the other copy, the stack not
edited by hand while the agent changes it, what each operation of the
agent's change of the stack needs worked out as it lands and stored with
it (patch 2 made again, and patch 3's new version adding a line after
it: patch 2's operation needed by patch 3's, nothing needing patch 3's,
for the operations of the change from the stack as it is, each marked on
its own), failures kept to look at, comments staying on their line
across versions, a patch edited by hand (as mail and as code, at a line,
refused and taken up again; in this checkout, refused while Move HEAD
would not move HEAD, and taken once stacked has put HEAD on the stack),
`stacked head` listing the patches whose message differs from its
commit's, with moz-phab what each side names when they differ by the
Differential Revision, then why `--to` would be refused, the flags that
synced them once refused as unknown, `stacked head NAME --to N` moving
HEAD only as stacked last left it or on the stack with no mark, where it
left HEAD remembered from one run to the next, the stack as it is now put
there whatever it has become, and refused with why otherwise (a commit of
yours on top, one reworded outside stacked), and, with moz-phab, never
below a commit that names a revision its patch does not, every commit up
to it kept as it is, a revision named outside stacked refused, trimming, the settings (in ~/.stacked, or the
folder `$STACKED_HOME` names, the folder made by the first written, a file
`$STACKED_CONFIG` names over it; the files of before moved in by a run of
yours, said once, never over a file there, and not by the shell's
completion; a checkout's own over them, an ask on a stack having its
checkout's wherever stacked runs, one git tracks not read; and what each
kind of ask may use, set up with the agent, line by line, a row of the
one list at a time for every kind, from what it has, the checkout's claude
settings and instructions, in the checkout's own or with `--global`, a
command saved the newer way, though your claude settings or the
checkout's own list write it the older, and your claude settings never
written; each kind given its own list and every ask's, and the
refusal of a tool naming its kind's list; what your claude settings deny
in the checkout refused to a change, an answer and a build and test, and
an answer refused too what would make the commands it is given write a
file or run a program, a change not; the
first ask sent at once, with no setup; the calls an ask was refused landing
as items of the review of its work, one a rule, a failed run's too; a tool
allowed from now on saved in the checkout's own list for its kind alone,
sent back or closed, and one allowed for this work given to every round of
it until its review closes; the setup listing what reviews allowed, to
take out), Phabricator's comments
held for review then filed, none rejected (none twice, but those a review
of before rejected, back once; an edit held as an edit of its turn, then
the reviewer's latest turn, the turn it edits redacted; a reply or an edit
in a thread you closed held with the thread as it was, `stacked landed`
saying so, and reopening it once accepted, on the stack's comments for one
moved there, as its history has it last when written since), a new stack
pulled from Phabricator's revisions (by number or link, named or not,
made empty, on the bottom one's base once read, the pull's review
accepted from the command line making its patches, a revision a stack
holds, or that the work waiting for your review adds, refused before
Phabricator is read, one on a commit the repository lacks refused, its
run failed saying so,
nothing of Phabricator stored, the stack made as it was asked staying,
empty, on HEAD),
Phabricator's commands there only when built with it, and
what does not apply being refused, edited again, or read-only. `tests/tui.rs` draws the editor into a buffer and presses keys:
that no letter but 1, 2, 3 and / does anything, keys mapped by the settings
(the bottom bar naming them, but as they are where what is typed is read),
Delete and Backspace as the menus' items that close, drop, stop or reject
(a comment closed, with those its answer opened, its agent stopped and
its ask landed at once as failed, its ask taken
back from the queue once asked, the comment open, or taken out of the batch at once; a patch dropped and the stack
deleted; a file's changes dropped from its patch for review, refused when
a later patch needs them, a rename, an added file and a deleted one
dropped whole, and the patch's drop offered for its only file; in a
review, a part rejected, a comment closed with it, what you wrote taken
back; landed work rejected and a comment closed from the banner; an ask
stopped from the runs, landed at once too; what waits its turn taken out
of the queue, the confirmation saying what is taken out and what stays,
the status line what was done: an edit ask from its line of the banner,
the banner's menu and its comment in the middle, the comment staying open
and the one asked with it still waiting, the cursor on the banner
following its line; a round sent back, the work staying as you sent it
back, its review closed and the stack as it was; a reconcile with HEAD
from its line, its menu (Enter there, saying what it waits for), HEAD's,
the patch marked's, the banner's and the runs, as if it was never asked,
the marks still there, and the edit asked after it, which no longer waits
for it, going once the change before it lands; each asking first where its item does, Esc then
doing nothing; on a comment closed, the status line saying so), their key
a setting, and nothing done in a menu, History…, the tree, the versions or
the search, nor in the comment editor and `/`'s bar, where they edit what is
typed, the search of the middle (as it is typed, from any pane; Tab and
shift-Tab from match to match and round past either end; Esc back where the
cursor was, the middle scrolled as it was, a click out of the bar too; Enter on the match,
or back with nothing matched, said; the next `/` with the last search,
selected, what is typed taking its place, ↑↓ through those before; case
ignored, a line of code without its sign; a comment, the message and a
file's name; side by side and compact; split in two, the cursor's half
alone, the other's lines not lit, Esc putting that half back; reading; a
line folded matched as
one, the cursor on the piece of its match; the matches lit; its key the
settings', and those from match to match, Tab a key of the screens' too,
its others as they are; with each file whole, a line in no hunk found and
gone to, 3 typed while the bar is open, 3 after Enter, and the files read
while it is open searched again, Esc going back to the row found again),
the
menus by place, what `?` says where, writing in every pane, the keys in
a patch looked at closer in a review going from the line Enter went to
in its stack diff, though nothing was drawn since, the stack's menu
with the stack's comments to ask about after Home, or a right click on
its header, with nothing drawn since, the comment
closed last on a line shown dimmed in the box until you write (↑↓ said only
when it is longer than the box, which ↑↓ and pgdn then scroll down to its
last line, the box above the bottom bar), the regions it
says it drew (its panes, then the boxes over them, inside their borders:
what StackED's selections keep to), the first ask going at once with no
setup, at the effort its dialog set, given stacked's defaults, the setup's old
mark in the checkout's own settings passed over; the tools an answer was
refused as items of its review (one a rule, the calls under the agent's
comment on the right, a broad program's rule widened from the menu, what
changes things noted), each refused by Delete, allowed from now on by
Enter and for this work from the menu, its three boxes naming the rule
and the file; followed up to answer only with a tool allowed though
nothing is written, the dialog saying what the work may use and what is
saved, the follow-up given both and its stream saying so, the comment told
in your answer after its reply, the follow-up's reply in the same review,
and what the work was allowed for itself ending as its review closes, the
close saying so; what is allowed from now on saved closing,
accepted or rejected, in the checkout's own list for answers alone, not
offered where git tracks that file; and a reply on an item sending the
work back; in the review of the agent's work, a comment on a patch it adds
(Enter on that patch's line of the stack diff), and what you write in a
thread the agent or a reviewer opened that it was asked about, each listed
with what you wrote, keeping the work from being accepted or rejected,
taken back (that thread as it was before the review), and sent back, the
next round asked about it, on that patch of the stack it goes on from;
that round rejected, the comment on the patch it added moved to the
stack's own comments, open, `Move it back` grayed; work sent back from
its dialog to change its patch, rejected, its round landing in its place,
asked of that patch, and that round's work sent back to change the
stack, from a patch's comment, the next round asked of the whole stack;
followed up to answer only, the work staying for your review, its change
still marked accepted, the review saying the follow-up runs and the
cursor on it, what you wrote gone with it, the follow-up asked on the
stack the work makes with the work's reply and your answer in its thread,
its reply coming into the same review after the work's, your answer to
the reply before it grayed, nothing waiting for a review of its own, then
the review closing as ever, the replies in their thread in turn; followed
up to build and test from a comment on the stack, what it found a section
to read (its reply, and what it found of a patch, each on the right, Enter
marking nothing), the review closing with its reply in the thread after
your answer and nothing filed on the patch's title; the review closed
while the follow-up runs, its menu saying so, closing asking first, the
follow-up stopped, its ask landed as failed and said so on its comment,
nothing of it waiting for review; a comment asked
about and its stream and answer shown in place, the stream folded in the
step that reads its landing, as the banner says what landed (its line
read first cut short, as a writer holding no lock would leave it, then
whole), its agent stopped from its
menu (the ask landed at once as failed, said so, its stream folded, the
comment's menu whole again, and the agent's wrapper, a child of the
tests' process, waited for), an ask done leaving no zombie once the
editor looks again, an ask whose agent is gone without landing it landed
at the next look (its process gone, or a zombie the editor did not start,
left for whoever started it to wait for), a build and test stopped as a
review closes landed at once though it had no process yet, the review's
word kept, the ask dialog (every `Ask agent…` item opening it with the
permissions that apply there: a line's, a patch's as a whole, the
stack's, a question's on the history, the comments listed with how many
each asks about, the comment editor's, changed or as it is; both lines
unset, Ask Now and Ask Later grayed and the dialog saying what is
missing; ← and → setting each line's first option from unset, then
stepping on and stopping at either end; Enter opening the list of a
line's options under it, on the one set or on the first, ↑↓ going round
it, Home and End to its ends, ← → doing nothing in it, Enter picking an
option and Esc leaving the line as it was, either closing the list alone,
the dialog open on that line, and Ask Now and Ask Later grayed until the
three lines are set, the note naming every one unset, in the lines'
order; the model's line of the full ids, in the list's order; ↑↓ going
round to the buttons, ← → there past those grayed; Ask Now saving what
changed first and asking at the permission, the model and the effort set,
the stand-in given that `--model`, that `--effort` and the tools of that
way; the settings' `ask_model` and `ask_effort` set as the dialog opens,
changed there as any, the ask going with them; Ask Later putting it in the batch; Cancel and Esc asking
nothing, back to writing the comment; a second dialog unset, nothing
kept for the stack; clicks opening a line's list, picking an option of
it, closing the list alone from anywhere else, the mouse over an option
taking the list's cursor there, pressing a button, cancelling out of the
box; Ask Now
grayed while the network is checked, saying so, and while it is not
reached, saying how to turn the check off, Ask Later going all the same,
a dialog opened a moment later not checking again; reached, Ask Now
asking; `agent.reach_check` off, nothing checked; the items that open
something to write first picking no effort; the review's `Send it back to
the agent…` opening it on the work it sends back, its title saying so,
nothing set, the permissions those of the comment going back, its
buttons Send Now and Cancel, Send Now grayed until the three lines are set and
pressed grayed saying why, the line under the permission saying what each
does with the work, `?` what Send Now does as the permission set does it,
Cancel and a click out of it sending nothing, the review as it was, a
second dialog unset; Send Now grayed, saying why, while the network does
not reach the agent, nothing sent), the model and the effort an ask goes with
(the stand-in started with the model and at the effort the dialog set, the settings' own
taken out; a batched comment, sent now or with the batch, with the model and at the effort
it was batched with, or another its dropdown picks, one waiting its turn with its own, one queued by an
older stacked with the agent's default; work sent back with the model and at the effort its
dialog sets, the work asked at low, its item no dropdown of its own; the
model and the effort the agent was asked with shown wherever its work is
named: its comment's state with the agent and replied to, in the middle
and the banner, the banner's landed line, `Runs…` and the transcript, the
review's title, the comment it opened and its reply in the review, then
their turns in the middle, the comments pane, the search, the turn's name
in its menu and in the editor's title, the comment's history, a reply
added keeping each turn's mark, and a thread of before as it always
was), the screen never waiting on
the agent, redrawing not slowing as the agent writes (a frame timed as the
time the editor ran to read and draw it, not by the clock, which counts
too the time a busy machine gives to other work), the filter kept in
the file, `Filter comments…` on a patch's menu as on both headers, each
counting the comments the middle shows, as its title says (from the
comments pane's header, the patch's or the stack's own), the filter
picked from one patch's filtering every patch's, → opening the comments
pane of a patch with none, and of the stack with none of its own, its
header saying the filter and that it lists nothing, the line under it and
`?` why, its menu there with the filter, ← putting it away, the middle as
wide as it was, and → bringing it back, put away by hand too; the line's
texts for a filter hiding a patch's only comment or the stack's, and for
none closed, the pane staying then, or going; a comment closed and none
open, the pane listing it under `closed` and → reaching it, the views of the code
(the menu of 1, the view drawn marked,
Enter or a click picking one and Esc none, its key a setting, each view
drawn; a view picked on a middle too narrow making its room, the comments
compact, else hidden, else the patches hidden too, as little as it can,
said and left so once the view is; on a screen too narrow even then, the
item saying so and the view not taken; split in two: both halves drawn,
each a region of its own, each its own cursor and scroll, search, reading
and files whole, and its place on each patch, the other half's cursor as a
pick, ← → from half to half and pane to pane, a click and the mouse taking
the cursor into a half, a patch clicked taking it into the left one, a
comment and a read mark made from the cursor's half and shown in both,
another view keeping the cursor's half, a middle too narrow drawn as one
and said, and the layout a setting), the views of the interdiffs (the
dialog of 1 on the versions of a patch, History… and its stack diff, the
screen of what an edit changed, the review's stack diff and, with
moz-phab, what differs from Phabricator and the review of what a submit
sends: Compare, View and Lines as drawn, each its value in brackets as
the ask dialog draws one set (`[the two diffs]`) and the line under it,
then Done and Cancel,
opening on the choices as they are, those set elsewhere too, ↑↓ from line
to line and to the buttons, round past either end, Home and End, ← → from
button to button, never round; ← → stepping a value, never round past
either end, the screen following at once, the line under it, the status
line and the top bar saying so; Compare set, View keeping side by side,
else taking the other list's first, both ways; Enter opening a line's
list under it from the column of its value, framed as a menu is, on the
value set, under the cursor, ↑↓ round past either end, Home and End, ← →
nothing, Enter setting the value and closing it, the screen following,
Esc closing it and setting nothing, the bar saying so, on each line;
Cancel putting back the view and the lines the dialog opened on, the
cursor on its line, the status line saying nothing; Done keeping them, on
every screen, the status line saying how the view reads, else what lines
are drawn, else nothing; Esc, Esc closing the list first, and a click out
of the box putting back what it opened on, a click in it on nothing doing
nothing; `?` on each line the value set in whole words, what Enter and ←
→ do there and what Done and Cancel do, in the list what Enter sets, on a
button what it does; the mouse as in the ask dialog, over a line or a
button the cursor there, a click on a line opening its list, over a value
the list's cursor there, a click on one setting it, one anywhere else
closing the list alone, a click on Cancel and on Done pressing it; the
bar `1: view`, 1 doing nothing on the tree nor on a reply in the review,
the code's in the middle still, and its key the settings'; as an interdiff, what its two
signs say once over the code, the versions named as the list names them,
its lines as ever; in a column per version, what each column is, a line
both remove dim in the colour of a line removed, the line each version
adds alone its column in the colour of a line added and the other's `·`
dim, what changed in each in the colours for words, the title each has
alone `=` and the other's column `·`, its words compared as words, the top
bar saying so; side by side, the old version from column 31 and the new
one from 76, each under a heading of its version, a line both have dim
in its own colour, the lines that differ in their own colours with no
sign of the interdiff, what each version's own diff changed in a line in
the colours for words, the title compared across, the top bar saying so;
on a pane too narrow, side by side set all the same, the line under the
view in the dialog saying so in short and `?` there in whole, the pane
drawing its interdiffs as interdiffs, the status line, the top bar and
`?` saying why, then side by side once the pane is wider, the line under
the view as ever; the settings' `view.interdiff` starting them side by
side, the dialog opening on it, and the view set holding on every
screen, from the versions to History… and back; the cursor kept on its line through each view, on the
new version's line and on the old one's, back as an interdiff on the line
it was on, `o` side by side opening the link of the new version's line of
the message beside the old one's, and a text's top line kept through each
view; in the review of the agent's work, its stack diff in each view, said
once over its first interdiff, the versions named as the review names
them, the cursor on its line, Enter side by side on a row of two going to
the new version's line in its patch as the work leaves it; with moz-phab,
what differs from Phabricator in a column per version and side by side,
the two versions named as the title names them, Esc back on the line,
each file of a latest diff on another base side by side, and in a column
per version the interdiff all the same where the files cannot be read so,
its hunks' headers that differ in their ranges one line with both ranges,
a line of each base the other's diff does not show `·` in the other's
column), the change from one version to another as a patch of its own
(the code, which the dialog's Compare sets, on the versions of a patch,
on History…'s history and its stack diff, on the screen of what an edit
changed, in the review and, with moz-phab, on what differs from
Phabricator, the code can be set there (and drawn there both from a
latest diff on its parent's own commit, read on it alone, and from one on
the commit under the whole series, read over the patches below it, which
does not apply on that commit alone); from v2 to v3, on one
base, what it is said once, the messages' lines over it, then a.txt as v2
leaves it, then as v3 leaves it, `-line 39 was`, `+line 39 said`, nothing
dim, the status line, the bar `1: view · 3: whole` and the top bar saying
so, side by side the two on one row, compact too; across a change of
base, in History…'s stack diff of an edit giving both patches new
versions, what dim is said once, patch 1's change on one base, nothing of
it dim, c.txt in it, patch 2's the lines the change of base made in place
of line 25 dim, its own line 35 not, line 28, which the old version took
out and the new one keeps as the new base has it, not, c.txt, which only
the change of base touches, left out, and as an interdiff as ever; `3`
each file of it whole, the lines in no hunk dim, the bar `3: hunks` and
the top bar `· whole files`, then its hunks again, nothing drawing the
interdiff, and the settings' `whole` key doing it, 3 not; the settings'
`change-side-by-side` starting it side by side, the dialog opening on
it, unified on the versions' pane at 80 columns and `?` saying why, the
line under the view in the dialog saying so in short and `?` there in
whole, on History…'s at 100 the top bar saying so, compact set there the
status line; the
cursor from the interdiff's `++line 39 said` on the change's `+line 39
said`, side by side on its row, back on its line as an interdiff, and from
a line of the change no line of the interdiff is, on the nearest, and back
on it; in the review, the code can be set there, the cursor from the interdiff's
`-+TWO` on the change's `-TWO`, ↓ from line to line, Enter going to the
line of the patch as the work leaves it; with moz-phab, Phabricator saying
no commit for the latest diff, the code not to be compared, the dialog
saying why under its buttons and `?` on Compare, → setting nothing and
the status line saying why, the code dim in the list and Enter on it
setting nothing, then, a fetch reading the commit, the change from the diff
on it to the patch as it is, on one base, nothing dim), the lines the
views of the interdiffs draw (the dialog's Lines, every line set, `?` on
only what changed in its list saying what it draws, the bar saying what
Enter sets; only what the patches changed set, the view as it was, the status line saying what is
drawn; as an interdiff, what it is said after how to read it, the titles
and the messages as each version says them, the file's name, the line
each version says its own, `⋯ 5 lines are left out here.` and three times
`⋯ 1 line is left out here.`, no context line, index line or hunk the two
have alike; the top bar `· only what changed`, the bottom bar `1: view`,
`?` saying what is left out and that 1 draws every line again; the
dialog opening on it, the view as it was; in a column per version the same,
said after what each column is; side by side said over the heading, a
pair of lines one row; on History…'s stack diff too; every line again,
the status line saying so, the context lines back, the top bar and `?`
as ever; the settings' `view.interdiff_lines` starting them so, the
dialog opening on it, and with `view.interdiff` both read from a file; across a
change of the patches below, patch 2's interdiff in a column per version
with every line its context lines and index lines, and with only what the
patches changed the line it no longer takes out, `-·line 28`, its own
`·-line 35` and `·+THIRTY-FIVE` under the new version's header, never one
with both ranges, the context patch 1 changed left out, and patch 1's,
on one base, its new lines and c.txt; the change, unified, what is left
out said rather than what dim is, patch 1's on one base whole, patch 2's
lines the change of base made in place of line 25 left out, `⋯ 2 lines
are left out here.` dim in their place, its own as ever, side by side
and compact the same, and every line again dim; the cursor in History…'s
stack diff on a line kept there still, and back, on a context line on the
nearest line kept, and back on its own, through another view too, and a
text's top line kept, else the nearest; in the review, the cursor on its
line, what is left out said, ↓ from line kept to line kept), what changed in a
line almost the same as the
one removed painted as words (and lines quite other, not), in the code, in
a version's interdiff and in the history's stack diff, none of the room after
a word last in its line, and the words keeping their own colours under the
cursor, in the code and in the history's stack diff, the rest of their line
in the cursor's (`paint.rs` tests the colours for words so: whatever they
become, they must look otherwise than their line's, and than the cursor's),
the message in an interdiff compared word by word (the words put in its title as words, the words it keeps as
any text, its sign as its line's; in the review of the agent's work, a
paragraph the work only wraps otherwise no change, the cursor on the
first line of code that changed, and a comment written from a line of it
there on that line of the new version; accepted, its lines read as they
were, Delete there taking back the mark that read them and Space reading
them anew, and the history changing no line of it; a word changed in a
paragraph wrapped otherwise the one word coloured in each; a command
wrapped otherwise a change, its break marked; the diff after the message
painted as words as ever), the submit's review comparing the title and
summary so, side by side too, and saying at its top why a new diff is
sent when the latest diff makes the patch's changes on the patches below
as they were before a rebase, no code under it, then sending one; how
long ago what it shows of the revision was fetched, said as the
Phabricator line says it, at its top and on Phabricator's diff, from a
fetch stored eleven minutes ago, nothing read, then from one found as
stored a moment ago, just now, and a minute ago as the review stays open
and the minute goes by, nothing read nor written; a submit whose fetch
first is turned away (HTTP 429) sending nothing, said as such, no call
made after the one turned away, nothing stored; its
code drawn in the view the dialog of 1 sets there: compared as two diffs,
as the view of what differs from Phabricator draws its code, the
interdiff of the latest diff and the diff sent, named so, a sign before
each version's own sign, the columns' marks, the two halves, only what
the patches changed leaving out the lines both have alike; on another
base, each file as each leaves it, said as that view says it, as it is
and side by side, the code views as ever; the files not read so, the
interdiff all the same, said so, only what the patches changed leaving
out what the move of the base made, the line each version says its own
kept, in each view; compared as the code, unified, side by side and
compact as the middle draws a patch, every line drawn; the cursor kept
on its line from view to view and back, from a line of the interdiff to
the line of the code it is and back, the title and summary side by side
with the code, the top bar saying the view; Esc opening its menu and Esc in it closing it, Enter and Space
doing nothing, a right click opening the menu next to the cell clicked
and a left click nothing; that menu `Send it to Phabricator`, `Cancel:
send nothing` and `The view of the code…`, which opens the dialog, `?` on
it saying what it does, and no Search…, Runs… nor Quit; Send and Cancel
there sending it and sending nothing as Enter and Esc did; the bottom bar
and `?` saying Esc for the menu and 1 for the view, no Enter, no Space;
and with no code, no 1 in the bottom bar, 1 doing nothing,
the cursor looking the same on every line, as `cursor` paints it, whatever
the line's own colours (a line added, a line removed, a hunk's header, a
file's name, each line of a comment, a line read, the patch picked in its
pane), and the patch picked as `picked` paints it once the cursor is in the
middle (`paint.rs` tests the cursor so, over each colour, dim or reversed), reading by hunks and by a Rust file's blocks
(the cursor going down to the middle line, then the code coming up under
it, in each view; Enter commenting there, Delete taking a mark back, and
a comment's rows keeping the keys of commenting, Space its menu, marking
nothing read; with moz-phab, the trailer moz-phab writes at the end of a
message passed over by Space, the margin asking no mark on it, the patch
ticked and nothing left to review once all but the trailer is read, and
the trailer counted when the settings say white space needs review), each
file whole (its files read away from the screen,
the status saying so; no hunk's header, each line removed in its place,
the lines between the hunks in none, keyless and dim, the cursor looking
the same on them as on any line; a file added, deleted, renamed, not text;
a file as its patch leaves it, the patches under it applied, and nothing
written in the repository; the rows of the hunks' lines, keys and all, as
in the hunks; a comment on such a line shown both ways, written either
way on the same key, and none on a line in no hunk, the menu and the status
line saying why; a line in no hunk folded, two sides keeping its piece, and
taking no read mark; the files listed in the patches pane gone to; the key
a setting; on the stack's own view, said, and on another screen, no key;
with no base to read the files on, the hunks under a line saying why),
the cursor through 1, 2 and 3 (on its line and its line of the screen,
through each view, split in two too; back
to the hunks from a line in no hunk, on the closest line in a hunk, ties
above; a hunk's header, whole, on its first line), and the twelve ways the
three keys combine (unified, side by side or compact, commenting or
reading, the hunks or each file whole: the view drawn, the old side beside
the new, a comment shown, a
read mark made on the hunks' keys and taken back, the cursor kept),
editing a patch by hand at the cursor (a line added after git's
`\ No newline at end of file` opened at its own number, and a reference
to that number going to that line), and from the review of landed work
where its cursor is (on the list, a line of the stack diff, a row of the
patch looked at closer: a line removed opened at the line after it, a
comment or the message with no file to edit, an edit not taken opened
again at the same line, and the menu writing a comment where Enter writes
one), the history screens, the branches
History… draws and the stack diff it shows beside each change, the agent
asked about a record of the history, a new database started from it, a
moved comment and an answer's comments, a patch's comment asked of the
whole stack, every comment of the stack closed at once but those that
cannot be, a comment closed taking the cursor on it to the line it was on,
on its line of the screen (from its menu on its first row and on a
reference, by Delete on a turn; in each view of the code, split in two each
half on its own, with each file whole, on a line folded, reading too; on
the stack, the row over it; hidden by `Filter comments…`, from the agent's
stream too), and leaving the cursor elsewhere where it is (closed from the
comments pane, which picks the next comment or the one before at the end,
from the banner, by another stacked; hidden by `Filter comments…` from
either header, moved to the stack, reopened), a turn written again in its
place taking the cursor back to it, what you said last written again (off the queue meanwhile;
left as it was, asked as it is or left so), a reply written in $EDITOR
under its conversation, its references under it, and a turn written again
there in its place, the editor in the middle between the last reply and the
references, the agent's stream between them too, under the editor when one
is open on the comment (Enter on the stream still opening it, Enter on a
reference still previewing it, and the cursor staying on its line as the
stream grows and goes), the references a rewind or an edit no longer names detached
(the status line saying so; those never named, those the agent attached
without naming them, and those the reply waiting for review names, kept;
the next one attached taking a number never given), the agent's reply
written again as its text alone,
keeping its references but the one it no longer names, a turn deleted the
first way counting nowhere, a web link attached from the menu (what is not
one refused, the status line saying why) and named, its line under the
comment cut to the pane, shown whole in its box (folded there when longer)
and opened by the stand-in, never a browser, the status line saying so,
the agent's own link given through the stand-in for claude, both listed
in $EDITOR, and the link detached by a rewind of the only turn that names
it, the links of a line of a commit message (a web link, two on a line
picked from their menu, none on a line of the message said and nothing
done on a line of code; a markdown link named by its text, and one
between angle brackets; with moz-phab, the bug a title names on Bugzilla
and on the settings' own, the trailer's revision at its link, and a bare
one on the settings' Phabricator, none without it; without moz-phab, the
web links alone) opened by the stand-in from the line's menu, from the
menu of a right click on it and with `o` (which the settings make another
key), a click opening none, in the middle, reading too, in the review of
landed work (a line of an interdiff in the stack diff, one taken out too,
and a patch looked at closer), in History…'s stack diff and in what a
submit sends (side by side too), the links of a line of what a comment
says opened from its menu, after the comment's items, the same items (a
web link, a markdown link by its text, one between angle brackets, and,
with moz-phab, a bug and a revision on the settings' Bugzilla and
Phabricator), none on a line of a code block nor on the comment's
heading, a line folded one line from each of its pieces, a comment
collapsed the first line it says, the comments pane expanded the line it
says under the comment (none compact, none when that line is code), and
`o` opening none there, a stack made with none there (imported from the workspace, a
range typed and refused until it holds a commit, the changes staged on top;
pulled from Phabricator, its revisions read by a stand-in; or by the agent), a
comment moved to the stack
taking the cursor with it, the mouse (a side pane's line marked under it and picked by a click,
elsewhere the cursor following it, a click as
Enter, a right click's menu, a menu's item; a menu opened with a click
next to the click, to its left or above it by the screen's edges, whole on
a screen made smaller, and where it was once a box in it is checked, a menu
opened with a key in the middle; writing a comment in the middle
in each view of the code, in the patches pane, in the comments pane, or
between a reply and its references, a click outside the box as Esc,
picking nothing, the menu that click opens next to it, over the comment
too, the one Esc opens beside the comment, a click outside either going
back to the
comment, and a click in the box putting the cursor there), the wheel
(while the mouse is in use: over the patches pane, by whole patches, the
cursor's pane not moved, the patch picked kept out of sight and the line
under the mouse marked until a key brings the pick back into sight; over
the comments pane, by whole comments, the same; over the middle, unified,
side by side, compact and split in two, by the lines turned, the cursor on
the row then under the mouse, in the half under it, the other half as it
was; over the banner open, a menu longer than its box and History…'s
stack diff, the cursor under the mouse; over History…'s list, the tree and
what the search found, what is picked kept, the line under the mouse
marked, a click on History…'s picking it and a second going to the tree
with the tree's own pick in sight; over the text of what an edit changed, a transcript, a reference's
preview, what `?` says taller than its box, the review's pane on the right
and the pane on the right of why Move HEAD does not move HEAD, each only
scrolled, the cursor where it was; over the box of a comment written
taller than the box, the cursor kept in it, typing there (and the
editor's own test, `editor.rs`, of the box scrolled: nothing before it
is drawn, the cursor on the nearest of its lines at its column, as far as
its ends, what was closed here scrolled as ↑↓ scroll it); over the
patches while a patch is moved, the patch going under the mouse; each as
far as its ends and no further; the patches past HEAD's line among them
a line at a time, as a mouse's notches come in the browser, as far as
notches of several lines and pages go; the middle compact, over the side
that ends first, to the other side's end; by pages too, a page the middle's height
less a line; nothing out of a box open, over the middle while a patch is
moved, nor over the top bar or the bottom line; after a key, ↑↓ where
the cursor is, wherever the mouse is, pgdn for a page, until the mouse
moves again), HEAD with the mouse (being moved, put down by a click on
its own row once the mouse over a patch put it under the patch; its row
pressed and let go with no drag, the click it was, HEAD's row picked,
then its menu; pressed and dragged, moved as `Move HEAD…` moves it,
under the patch the mouse is over, at the base over the stack's header,
and put down where the button is let go over the patches; let go over
the middle or off the screen, left where it was; Esc while it is
dragged, left where it was, the mouse then going by and the button let
go dropping nothing; dragged where Move HEAD would not move HEAD, the
view of why opened, as `Move HEAD…` opens it; with moz-phab, let go below
its last commit marked `!`, said why as a click there says it, HEAD
being moved still; and a press on a patch's row a click at once, a drag
from there the mouse going by), the mouse going over the search's scope
turning nothing, and a click on it turning it, the review's list under
the mouse as the patches are (a line marked, underlined, nothing picked,
the right pane as it was; a click picking it, the right pane following, a
second click Enter, the operation accepted, a right click its menu; the
wheel over the list scrolling it, what is picked kept out of sight, the
line under the mouse marked, a key bringing what is picked back into
sight), and so the other lists: History…'s (a record marked, nothing
picked, the stack diff beside it as it was, scrolled as it was; a click
picking it, its stack diff following from its first line, a right click
its menu, a second click Enter, the stack it made picked in the tree, the
record marked on the history marked no longer there), the runs' (a run
marked, the screen as it was; a click picking it, a second Enter, what
its agent wrote), the list of why Move HEAD does not move HEAD (a line
marked, the pane on the right as it was, scrolled as it was; a click
picking it, the pane showing it from its first line, a second click Enter,
into that pane) and what the search found (a match marked, its context as
it was; a click picking it, the context following, a second click Enter,
the patch at its line), each scrolled by the wheel, what is picked kept
out of sight, the line under the mouse marked, a key bringing what is
picked back into sight, the scroll bar (only in the
place the cursor is in, and only while it holds more than it shows: the
patches pane longer than it is high, its bar in the line on its right
beside its rows under the header, the header's row keeping the line, a
thumb as long as the share it shows, at the top on the first patch,
between on the fourteenth, a row further on the next, at the bottom on the
last, and a row from it a line from it, none once the cursor is in the
middle, nor in the middle that the patch fits in, nor on a screen tall
enough for all of the patches; the comments pane on the screen's right
edge, the banner over the panes, its bar in its last column beside its
rows under the header, a comment's line cut a column sooner, that column
in none of the regions StackED selects text in, at the bottom on the last
comment, then, the cursor in the middle, the middle's bar in the line
between it and the comments, the comment's line whole again and its column
the pane's again, and the comments hidden, the middle's in its own last
column; the middle split in two, the bar of the half the cursor is in, the
left one's in the line between the halves, at its top and at its end, the
right one's in the screen's last column, the other half drawing none; the
middle side by side and compact, at the top and at the bottom; a menu
longer than its box, its bar in the box's right border beside its items,
its corners as they were, at the top, and at the bottom on Quit; a menu
that fits with none; what `?` says taller than its box, its bar in its
border, the middle under it drawing none; the comment being written taller
than its box, its bar in the box's right border beside its text, at the
bottom, then between as the cursor goes up, and a reference's preview, at
the top; the banner open, its bar in the screen's last column beside its
rows, not its own line, at the top and on its last comment at the bottom,
the panes under it drawing none; History…'s list, in the line on its
right, at the top and on the oldest record at the bottom, and its stack
diff, the cursor in it, in the screen's last column, at the top and at its
end at the bottom, the list's line as ever; what the search found, in the
line on its right beside the matches alone; the review's pane on the
right, no bar while the cursor is in the list, one once it is in the pane;
a transcript on its end, at the bottom; a text on a screen of its own,
what an edit changed, its bar in the screen's last column beside its lines
under the title, at the top, between halfway down, at the bottom with its
last line the first shown, and none on a screen tall enough for all of it;
the bar following the view, the wheel scrolling the patches pane to its
end, the patch picked out of sight, the bar at the bottom, and a key
bringing both back, the search of the middle jumping down the patch, the
bar with it, and Esc putting both back; the tests that read a pane's
lines, or the cursor's row, to the pane's edge reading them up to the
bar's column, not into it; and the tests in `tui/scrollbar.rs` of the
thumb, its length and its place: a tenth, a half and all but a line shown,
at the first line, halfway and at the last, a line from either end a row
from it, a share too small for a row a row, a track of a row, a view past
its last line, and whether a place holds more than it shows, scrolled past
its first line too), the comments pane's previews,
moving a patch, the long lines of messages wrapped (a patch's menu
offering it only when its message has some at the settings' width, the
note saying how many lines of which patches; the edit landing for review,
the stack as it was until it is accepted, then the new messages, as the
history says; the stack's header wrapping every message that has some, as
one edit; offered but not to be chosen, the menu saying why, on a
read-only stack and while an edit by hand waits for review), searching (what is in sight first, the patches then the
files they touch, the history), the agents' worktrees made ahead as the
stack's checkout says, the banner of the agent's work (Esc to it and
back, open a line for each, Enter on each; the comments pulled waiting in
it as the agent's work does, their review rejecting none of them (no
Delete, no item, nothing to reject as it closes), a reply to a comment you closed
shown with the comment as it was, reopening it once the review closes, an
edit shown as it was filed and as it is now over its thread, then headed
by what it edits, the turn it edits redacted; replies taken with Enter, the last one
closing their work's review, its comments left open; Enter opening the
review of work that opened comments too, and saying to review first the
edit that work goes on from; and work that brought nothing back reviewed
for its comment alone), the review's list saying how each part stands
(the title of the stack operations counting each mark, `✓ 3 accepted` in
green, `✗ 3 rejected` in red, `· 3 to review` in the title's colour, and
each operation's line marked `✓` in green, `✗` in red or `·` as any text,
the patch left as it was with no mark, the operation after the mark in
its kind's colour, `modify`, `noop`, `remove` and `add`
each its own; a line under the cursor in the cursor's colours, its mark
too, and picked in its own; a comment that asked `✓ keep` in green and
`✗ close` in red, cell by cell) and Esc's summary of it (each count
marked so, in green or red, what is to review and the rest dim, and what
else the menu says, `You wrote 1 answer…`, in the note's colour), the
answers made on the stack before a change in its review (one landed and
one running, a section of their own, each `·` until it is marked, saying
whether it runs or landed and what it was asked about; `Close review`
and `Accept all remaining and Close` grayed until each is marked, `Reject
all remaining and Close` not, Esc saying which are not marked yet; the
right pane, the bottom bar and `?` on one; Enter keeping one, Delete
stopping one, Space's three boxes, the one picked checked, the menu
staying, and its items marking them all; the one running landing while
the review is open, its line saying so, its mark kept; kept, the closing saying so, the
one running going on and landing on the stack it was asked on, its reply's
heading in its open thread saying `on the stack 1 edit ago`; restarted,
each asked again on the stack the change made, the one that had landed
shown what it brought back; stopped, the agent stopped and its ask landed
as failed, said so on its comment and in its stream, the landed work
thrown away unseen, both comments open and nothing asked again; the change
rejected, each going on as it was; unchained, none listed, the answer
waiting as it was, its heading saying how old its stack is all the same;
`stacked review --accept-all` refusing, naming the answer and where to
mark it, then accepting once it is marked keep and the review saved), the
stack operations of a change of five patches each marked on its own
(patch 4's new version changing a line in sight of patch 5's new version:
rejecting patch 4's operation refused, the status line saying that patch
5's new version applies only on patch 4's new version, to reject patch 5's
operation first, and its menu refusing the same; patch 5's, then patch
4's, each rejected alone, then the rest accepted, the stack holding
neither new version, the history and the run naming those two rejected;
patch 3's, a leaf, rejected alone, the stack holding patch 3 as it was
and the others' new versions; a reword rejected alone; accepting patch
5's while patch 4's is rejected refused, then patch 4's and patch 5's
accepted, and the change accepted whole; every operation accepted, put
back to review and rejected from their title, and from Space's items on
any other line, the title and Esc counting them, and `Reject all
remaining and Close` rejecting the one not marked yet; rejecting all that
remains refused while patch 5's, accepted, needs patch 4's, nothing
changed and the review still open; a change accepted in part dropping the
edit that went on from it, closing saying so first, the edit asked again;
the marks saved with the review, by each patch's change, and there once it
is opened again; the right pane's sentences under the operation's line,
what it needs and what needs it, nothing to mark on a patch left as it
was, and, rejected, whether it can be accepted alone, none of them once
the cursor is in the stack diff; patch 5 made again on patch 4's new
version, so that patch 5 as it was no longer applies there: rejecting
patch 5's operation asking first to reject patch 4's with it, saying why,
Esc keeping them as they were, and accepting patch 4's again asking the
same; on the command line, `--reject-all` rejecting every operation, and,
patch 5's marked accepted in the review saved, refusing with the stack's
reason, nothing changed, then `--accept-all` taking every operation not
marked yet, patch 5's marked rejected; closing in part, each of those,
waiting for its check of the stack the operations accepted leave, the
status line saying `checking the stack the operations you accepted
leave…` until the editor reads what it came to; what each operation needs
stored as the work landed (patch 4's, needed by patch 5's new version),
and the right pane, Delete refused, Delete and Enter taken and Enter
refused decided from it at once, no check made on the screen's thread,
`?` saying when it was worked out and what closing checks; an edit by
hand of patch 3, as mail in $EDITOR, landing with what each operation
needs stored, its review marking its one operation on its own; work with
nothing stored, as a stacked of before landed it, reviewed whole, its
title saying its mark and every line of its operations that mark, none of
their own, the right pane on the title and on a line, and `?`, saying
why, Delete and Enter marking the change whole, no check made on the
screen's thread, and, what could not be worked out stored, the right pane
saying so and why, then closed, the change taken whole; three patches
whose new versions, the first two, each add the same block, the third's
changing that block with one line of context, which finds it in either:
nothing stored, each of the first two rejected alone, then closing
refused, saying that patch 3 does not apply and that patch 3's new
version needs patch 2's operation, which is rejected, nothing changed and
the review open, then patch 2's accepted again and the review closed in
part; and closing in part waiting for its check, the cursor moving and the
right pane following it meanwhile, a mark changed meanwhile calling the
close off (`the review changed while the stack the operations you accepted
leave was being checked, so nothing changed: close it again`), nothing
changed, then closed again, once checked), the
heading of an answer's turn counting the edits since the stack it was
asked on (1, then 2; `on another version of the stack` once an edit is
undone and another made in its place; nothing in a closed thread), a commit message that differs from its patch's
holding back nothing (none of it in the banner, landed work reviewed and a
reply taken at once meanwhile, the stack and the commit each keeping its
own message, s and w doing nothing), HEAD shown where it was while stacked
moves it and where it went once the move is over, a message whose empty
lines in a row `git am` made one leaving HEAD in sync, HEAD forked from
the stack drawn as a graph in each of its cases (the trunk, then the
shorter branch, then the longer, HEAD above the whole stack, the stack
moved on since, far ahead on the same base with no trunk, the commits HEAD
alone has capped at three rows, the stack's patches all of them, and HEAD
on another base off the stack), the cursor going over the commits HEAD
alone has to HEAD's row and the patches, the marks of HEAD's commits in
their margin, `≠` and `!`, HEAD's row ending `· locked below patch 2`,
folded, `?` on each mark saying what it means, what it locks or grays and
how it clears, the bottom line saying only that `?` does, and on HEAD's
row saying where HEAD is, its marks, the lock and which case of Move HEAD
applies, or why none does and where to see why, its menu with Move HEAD
not grayed, a click on it and Delete there doing nothing, `Move HEAD…`
moving HEAD only as stacked last left it or on the stack with no mark,
else offered all the same, `?` on it saying what it does, and opening the
view of why (a commit of yours on top, one reworded outside stacked), HEAD
as it was, and moving it after a restart from what the database
remembers, the stack moved on since, its place never below a commit that
names a revision its patch does not (↑, Home and a click stopping there,
`?` saying why), the view of why Move HEAD does not move HEAD (a commit
above the stack, HEAD on its branch, then on none, shown whole with what
to do; a fork with commits only the checkout has, each, the first ten
then how many more, forked at the base too; a commit marked `≠`, HEAD's
own amended; one marked `!`, with moz-phab; HEAD on another base, sharing
no commit, under the base, forked from below it; several at once, each
with the others to deal with too; each line's comment and its commands,
their commits named, none of them run; the view following HEAD once the
checkout changes, then saying that nothing keeps Move HEAD from moving
HEAD; the arrows, a click, `?`, Space and Esc there, from the stack's
header too), HEAD following
a review remembered where it went, Reconcile with HEAD (on the menus of
the patches marked and of HEAD's row, not of another, `?` saying what it
does; made at once, waiting for review in the banner, grayed while it is
under way, what each of its operations needs stored as it is made, with
no check, nothing needing any; its review saying what it takes, each
operation marked on its own, stacked's comment saying what
to run from the top down, `git commit --amend` for HEAD's own commit and
`git rebase -i` for one below; accepted, the messages taken and the marks
gone; rejected, nothing changed and the marks there; asked while the
agent changes the stack, waiting its turn, the banner saying so and its
menu, Enter there, what it waits for, made once that change lands, going on from it, then from the
stack its acceptance made, an edit asked after it waiting until it is
reviewed and an answer going at once; going on from an edit by hand
rejected, dropped and made again from the stack as it is; grayed with no
commit marked), this checkout grayed for edits by hand
meanwhile (dim, ← → going past it and no further than either end, a
click picking nothing, the menu and
`?` saying why, an edit there as the stack's menus last picked it
refused), the comment editor hiding
nothing in each view of the code, each pane keeping where its cursor
was, the patches pane longer than the screen scrolling only once the
cursor would leave it, as far as the cursor needs (down to the end and up
again, a page at a time, Home and End, titles folded, files listed, ↑
from a patch onto the last file of the patch above and ↓ back again, from
the top line too, the lines of a patch whose files are listed pinned on
the top lines once scrolled past (drawn as in their place, its title
folded, its revision's status and mark, reading too; the files scrolling
under them, the pane still while the cursor moves in the lines under
them; ↑ above the first file and Esc showing them in their place, ↓
under the last picking the next patch; the mouse over them underlining
them, a click on them going to the patch; a pane too small for them and
a file; HEAD's line in sight with them), HEAD's rows and its line pinned
to either edge, a patch
and HEAD being moved, HEAD moved, a comment on the patch written from a
file, the cursor in the middle, the screen smaller and larger, wider and
narrower, the stack losing patches, and another stack shown, its pane as
if never drawn) and
the comments pane likewise, the patch's title kept at the top of the
middle, the patches pane's
states (expanded, files, hidden) and its files (listed under the patch
picked, gone through with the arrows past their ends, over the same lines
up as down, ↑ on a patch or on HEAD's row going to the last file of the
patch above, reading too, or to that patch itself when it has no file to
list, gone into with Enter and →, clicked, and picked from the middle,
where ← comes back to the file; reading, each saying the lines left to
mark read as marks are made and taken back, those of white space alone
counted only as the settings say, a file with no newline at its end ticked
once read whole, its patch too, the pane keeping its width from patch to
patch), the tags at the end of each patch's first line (commenting, its
comments, as long as they are, each title folding in the room its own tag
leaves; reading, two columns, the same on every line, each right-aligned, two
cells between them: its comments, as wide as the widest, blank on a patch
with none, then what is read, `✓`, in part, or not at all, as wide as the
widest a patch can show, `39/40`, or `9/10` of a patch of ten lines; a
patch being moved taking them with it, listing files the same, the files
under the patch as they were, and commenting again as before; the column
of what is read keeping its place, the comments theirs and the titles
their room, as lines are marked read, with Space and in the file, up to
`✓`, though no patch shows a count that wide any longer; no column for the
comments, nor room for it, while no patch has one waiting, there on every
line once one has, gone again once the agent answers it; in a narrow
pane, each title folded under itself in the room the columns leave, a
word longer than it cut on the first line, the columns whole), the
statuses of the revisions there (with moz-phab, each status after its
patch's number, a letter listing files and expanded alike, `[?]` for one
stacked does not know, no words; none before it is fetched, however long
the editor goes on, nothing read by itself, nor for a patch naming no
revision or one Phabricator does not have; the titles starting
together, those folded folding as they did, the pane wider by the column,
and the tags where they were; right after the letter, `✓` in sync and `✗`
for a diff, a title or summary, or a parent that differs, listing files
and expanded alike, each patch's mark the one its Phabricator line says,
patch by patch, a revision two patches name marked for each, as a patch
changes and as a reordered stack is submitted again; a blank while what
differs is not fetched, no cell for the marks while no patch shows one and
one more once one does, the pane a cell wider, a revision named since
blank while the others keep theirs; none for a patch naming no revision
or one Phabricator does not have; `?` saying what the marks mean, and in
the reading mode that the `✓` at a line's end is another, nothing of
them while there is no column; the stack's revisions fetched in one
search when asked, no other call made but the rest of the fetch, not again
as the editor goes on, nor once its patches name others, until a fetch is
asked again; a submit fetching nothing once it is done, all it needed
stored and nothing read, its revision's letter as it was fetched before
it, its mark `↻` and its line saying that it was submitted since, the
bottom line saying so, a new revision not fetched yet, `↻` with no letter,
until a fetch, which shows Changes Planned as such though Phabricator
says it is a draft; a submit's `↻` in the cell of its revision's `✗`, the
pane as wide, its line saying it in words, the status and the comments
as fetched, the other patches' marks as they were, the marks agreeing
with the lines, `?` in the pane and on the line saying what it means, a
review of a submit of it again saying when it was submitted, nothing
read, an editor opened again on the database showing `↻` and the same
line, nothing read, and a fetch bringing back `✓`, Phabricator giving
back what was sent; a new revision's `↻` in a stack none of whose
revisions is fetched, no letter for any patch and the column the marks'
alone, the pane two cells wider, `?` saying what it means, the fetch a
submit of it again makes first bringing its letter and `✓`, nothing to
send, then its summary submitted, `↻` again, and a pull of it storing what
it read after that, `✓` again, nothing new; the line and the review of a
submit saying that the revision was submitted 2 minutes ago, after it was
last fetched, 11 minutes ago, `?` on the review saying so, nothing read; a fetch that fails keeping
what is stored and saying why, each time it is asked, a first one showing
no status; none read nor offered while reviews come from nowhere; built
without moz-phab, none shown, nor a Phabricator line), the Phabricator
line (with moz-phab, over the message's header, the cursor coming to the
header: before anything is fetched, the revision, and that it is not
fetched yet, with where to fetch it; once it is, how long ago, `fetched
just now`, last; in sync; the diff differing, the title or summary, the
parent, and those together, listed plainly; a summary wrapped otherwise, every word the same, differing: Space
showing the lines of both versions, Phabricator's taken out and this
version's put in, no word in the colours for words, and saying so, `?`
saying that in sync is to the letter and what Space shows, the submit's
review showing them so too, and in sync once it is submitted and
fetched; a patch submitted and unchanged since in sync once fetched, its
line saying that it was submitted since what it shows was fetched until
then, Phabricator
giving back its title and summary as they were sent, white space ending
a line and all; a status stacked does not know, and a revision
Phabricator does not have; the comments to pull, none once a pull holds
them for review, nor once they are filed, one edited and one new once
the revision is fetched
again, none once their thread is closed; over the message in every view
of the code, in each half of the middle split in two, reading and with
each file whole, folded in three in the narrowest, how long ago with it;
none over a patch naming no revision, nor while reviews come from nowhere,
nor without a stand-in in the tests' own process, though what was fetched
is stored;
scrolled away with the rows, the title pinned in its place; Space on it
opening what differs on a screen of its own, its title naming both
versions and what it shows said first, then what the interdiff's two
signs say, the versions named as the title names them: the code as an
interdiff of the two diffs, what git derives in them left out, and the
title and summary
compared where their lines wrap aside, what only each has marked in the
colours for words; Esc going back to the line; in sync, Space saying so,
the parent named, and opening nothing; not fetched yet, Space saying that; reading, Space
marking nothing read and Delete taking no mark back; Enter opening the
menu, titled for the revision, every item of the patch about Phabricator
first, its submit, what differs, the pull of its revision and the fetch,
and the bar and `?` saying what Space does and what the menu has; the
patch's own `Phabricator` menu offering those, the patch's menu none of
them; what differs grayed, the menu saying why, while it is not fetched yet, and
for a revision Phabricator does not have; in sync, the screen saying that
nothing differs, the parent with the rest, Esc going back to the patch; neither item while reviews
come from nowhere; a latest diff on another base, each file as each
leaves it, said, or the interdiff all the same, said too, where the
files cannot be read so; one patch's revision pulled with its parents,
which is that revision alone, the stack holding the one under it, from
the line's menu and from the patch's `Phabricator` menu, that revision's
diffs and comments read and no other's, after its parent's search, the
banner naming it, one pull at a time, its diff, which no version of the
patch is, a new version, and its comments, held for review as a pull's,
what it read stored, the line saying no comments to pull, a pull of it
again going on from that change and bringing nothing new, and the stack's
pull reading every revision, an older title taken as a new version;
every revision of the stack fetched, the parents and the latest diffs of
all of them in one call each, by their PHIDs, then each, bottom first,
each patch's line whole as soon as it is shown, nothing read for it, nor
as patches are shown again; the stack's pull reading, after their search,
the parents of all of them and what is stacked on the top, then each, its
diffs, its raw latest diff only where it is not stored, and its comments,
with their authors' names, storing it as a fetch does, the line of a
revision whose new diff it read in sync, its comments held for review
none to pull, and an older title a new version; all of it read again
once it is fetched again, a raw diff not read again while it is stored,
the one the pull read among them; a revision whose search says when it was last
modified not read again while that stays the same, only its parents and
its latest diff read, with the others', and no record of it written again,
only the fetch's; one modified since read again, alone, its new comment
counted; one submitted, all it needed stored, nothing read, before the
send nor after it;
one commented on and pulled alone, read whole by the pull, its raw diff
stored already, and not by the next fetch, as the pull stored its
comments with when it was modified; a fetch that fails partway
storing nothing and saying why, the line not fetched yet, the next one
storing it all, one failing later keeping what is stored, a revision
never fetched not fetched yet; a fetch turned away (HTTP 429) said
plainly, tried once, its first call turned away and nothing asked after
it, nothing written, the statuses and the lines as they were, nothing
waited out nor read by itself after it, a pull asked tried all the same,
turned away too, said plainly, its run failed, nothing of Phabricator
written, and a fetch after it reading all of it at once;
what a fetch reads stored as records of their own, a revision's state, its
comments, its raw latest diff, the fetch, and an editor opened again on
that database showing the lines, the letters and the marks as they are
stored, and what differs from the raw diff stored, making no call of
Phabricator; how long ago it was fetched, said as the line is drawn,
yesterday, two hours ago, three minutes ago, then just now, and a minute
ago as the minute goes by, nothing written nor read; the patch's
`Phabricator` menu titled for its revision, or `Phabricator` for a patch
naming none, with its submit and the fetch, its items not on the patch's
menu itself, `?` saying what it opens and what the fetch does, the fetch
one at a time, the bars saying it is fetching, the line's menu offering
the same items directly, the stack's menu its own `Phabricator` menu,
titled for the stack, `?` saying what it opens, the fetch in it after the
pull, neither of them on the stack's menu itself; every `Phabricator`
menu made alike, the stack's, a patch's and its line's, the same entry, a
line then `Phabricator`, titles ending ` on Phabricator`, the line's
items and title those of the patch's menu, and in each the items in one
order, the opening of the revision in your browser first, the submit,
what differs, the pull, the drop of the waiting pull, the fetch last; a
patch's `Phabricator` menu opening its revision in your browser first,
`Open D123 in your browser`, its link exactly, by the stand-in that opens
links, never a browser, the bottom line saying so, offered while the stack
is read-only, nothing fetched and nothing written, and neither in the
stack's menu nor in that of a patch naming no revision; the stack's pull waiting its turn dropped from the
stack's `Phabricator` menu, between the pull and the fetch, and not from
a patch's; what a
submit fetches first, nothing of its revision stored, the bottom line
saying so, stored as a fetch stores it, its state, whether it is a draft
and its repository with it, its raw diff, its comments, and when, though
nothing is sent; reviewed again, nothing read; retitled on Phabricator,
then sent, nothing read, the summary sent as the review showed it, and
nothing stored of what was sent; and what a pull
reads, its comments with no time of when the revision was modified, as
its search says none, its title and summary, which no version of the
patch has, a new version, and, accepted and pulled again, nothing new,
nothing of the revision written again, only that it was read; a stack
pulled from Phabricator: a revision given a diff from elsewhere taken as
a new version, its parent the stack does not hold added below it and the
revision stacked on the top added on top, each with the author and the
message of its commit as uploaded, and a reviewer's comment, in one
review, each operation said with what it takes and marked on its own,
the stack the four once it is accepted, the comment filed, and pulled
again nothing new; from a Phabricator that gives no suggested edits, a
suggested edit alone, an inline comment with no words, counted by the
Phabricator line, pulled and filed as any comment, a thread of its own on
its first line saying so, that this Phabricator gives none, with its link
at the diff it was made on, another, answering a comment, pulled into its
thread, one removed neither counted nor pulled, and the first edited on
Phabricator to have words, counted and held as an edit of it, its turn
redacted; suggested edits as Mozilla's Phabricator gives them, the
stand-in for Phabricator giving a diff's lines too: a suggested edit
alone filed by a stacked of before saying it could not be read, and a
comment filed with its words alone by a pull from a Phabricator that gave
none, both read anew once their suggested edits are given, counted by the
Phabricator line and by the pull apart from a comment edited on
Phabricator meanwhile, held, their review listing each for what it is
and showing a comment read anew as it was filed, then with its suggested
edit, said so, never as edited, the edit as edited on Phabricator, and
each accepted as it closes, saying so of each kind: each then the diff of
the lines it replaces, as they are, after its words, in the place of the
turn it replaces, redacted, a comment read anew headed for what it is,
the edit as an edit; a new
one on the old side of its diff, on the line the patch removes, saying
so, a reply to it by its id on Phabricator pulled into its thread, and
another pulled since too, none counted after; and each diff drawn in its
thread as a block of code, each line as it is, its sign first, no list's
bullet nor fence, and the turn read anew headed for what it is; a pull
whose read of a diff's lines fails, failed, saying why, nothing of
Phabricator stored, the stack free, turned away there said plainly, then
read, a line the diff holds read, one it does not hold said not read; a version stacked knows bringing nothing, a revision
retitled elsewhere rewording its patch, known since; both changed,
Phabricator's taken, the review showing what the patch had since the
last version stacked knew, the patch above made again on it, its
conflicts marked, closing with it refused, in the editor and under the
lock, saying why, `Send it back to the agent…` offered with a permission
to change; rejecting that patch's operation taking the one that breaks it
along, the review closing then, the stack as it was; the conflicts
resolved by hand from the review, the edit a pull's still, closing
accepted; a leaf's add rejected alone, from what was stored of what each
operation needs, its comment not filed, closing in part checked away from
the screen, and pulled again, both back; more than one open parent, or
more than one open revision stacked on one where the pull goes up,
refused, its run failed saying which, nothing stored nor held, the stack
free, the pull of a patch's revision with its parents alone going all the
same; a pull turned away saying so plainly, its run failed saying so, no
record of Phabricator written, the stack free, from the editor too; a
patch's revision pulled with its parents, then
with those stacked on it too, as its item's choice picks, kept for the
stack; a pull waiting its turn as an edit does behind the agent's change,
one at a time, its line of the banner and its menu saying what it waits
for, dropped from its line and from the runs, asked first, else made on
the stack that change makes, reviewed after it, what each operation
needs worked out from that stack, its review marking each on its own once
that change is accepted; the stack not edited by hand while a pull reads
Phabricator, asked in the editor, its run written at once and reading,
nothing read yet, the read held as it starts (`review::hold_reading`, as
`review::hold_making` below holds a making, each read saying so in a
folder), the stack's menu and a patch's graying their edits and saying
why, Delete on the patch saying why, `stacked drop`, `move`, `undo` and
`delete` refusing, saying why, another pull of the stack refused, then,
let go, the pull read, made and filed, the stack free once its review
closes; a pull whose read is cut short, the process reading it gone: the
stack free at once, its run landed as failed once seen (`land_orphans`),
saying so, once, nothing of Phabricator stored, and, its read let go
after all, reading for nothing and saying why, then pulled again; a pull
saying each phase as it is, behind the agent's change under review,
unchained, in another process than the editor's: reading Phabricator,
then waiting its turn, then, that change reviewed, being made, held
midway, a reconcile with HEAD asked after it waiting for it, its line of
the banner, the banner's own line, Enter there, Delete there and what
changes the stack saying each, the drop offered only while it waits its
turn, what the pull waits for counting nothing asked after it; the stack
not edited by hand while a pull waits its turn, unchained, behind the
change under review, then while it is made, held midway
(`review::hold_making`, a hook of the tests' for one
database's pulls, as the editor's tests share their process: planned on
its copy and not filed yet, the launch lock held, each making says so in
a folder, then waits there until the test lets it go), the stack's menu
and a patch's graying their edits and saying why, Delete on the patch
saying why, `stacked drop`, `move`, `undo` and `delete` refusing, saying
why, then the pull filed once, its change waiting for your review, and
the stack free once that review closes; a chained pull whose base work is
accepted whole while it is made filed once, on the stack that work made,
chained no more, what each operation needs stored from that stack, its
review marking each on its own; one whose base work is rejected
meanwhile made again from the stack as it then is, its review showing
what it pulled onto that stack; one whose stack a write of the test's own
moves while it is made, past what refuses edits by hand, made again, and
filed after on the stack as it then is; the stack's list open in $EDITOR
as a pull is made, saved, coming back with why at its top, emptied,
nothing changed; a new stack pulled from a revision typed,
refused until it names one, made empty, shown, then moved onto the
bottom one's base, named after it, its parent and the one stacked on it added, the review open,
the commit's message as uploaded, or Phabricator's title and summary for
one whose summary was written otherwise since, a revision a stack holds
refused as it is typed, one on a commit the repository lacks refused, its
stack made at once, empty, on HEAD, and shown, staying so, its run
failed, nothing of Phabricator stored; the banner saying where a pull is
as it reads, its line among the edits, and why one that failed did, its
run failed; the
parent: a reordered stack's revisions differing by their parents alone,
nothing read for it, Space and the menu's item showing the parent first,
Phabricator's then the one a submit sets, and nothing under it, `?`
saying so, a submit setting it and nothing else, in one write, nothing
read, the line saying that it was submitted since until a fetch, the
pane's mark `↻`, the marks agreeing with the lines, then in sync, its
parents fetched with the rest, Space naming the parent; none
differing at the bottom of the stack, over a patch with no revision, a
landed revision, or a patch naming the same revision, Space saying why a
submit sets none as a submit does, no parent named; a parent that is
none of the stack's revisions named in a search of its own, where a
submit sets another, and as it was stored since; `…` while the revision
below is not fetched; the parent with the code, the title or summary, or
both, listed plainly, the view showing the parent first, then the code;
the patch's `Phabricator` menu submitting as its revision's status has
it: `Submit to Phabricator as WIP` while the status is not fetched, and
for a revision in work; grayed for one that
landed, the menu saying why; for one in review, the update of its title,
summary and parent alone in its place, grayed while what differs is not
known, as its revision has no diff yet, saying so, then offered for a
summary that differs,
`?` saying what it sends, its review saying at its top that the code is
not sent and why, the status staying as it is, no code under it, so no
view to set: 1, Enter and Space doing nothing, the bottom bar naming no
1, its menu no view of the code; `Send it to Phabricator` in that menu
sending the summary alone, no diff, no Changes Planned and no reviewers,
nothing read, the bottom line saying what came of it, the line saying
that it was submitted since what it shows was fetched, and once it is
fetched, the diff alone differing, the
update grayed, the menu saying why), and the comments pane's states (expanded,
compact, hidden) and the compact view of the code as they were; and the
notices of `stacked web`, kept once it asks for them (`keep_notices`): of
every stack, an answer asked at max landed for review on the stack not
shown, a change failed, a lint of the stack reporting what it did not fix,
and a follow-up come into the review of the work it follows up, each its
run, its stack in its title and its body as `stacked web` sends it; none
for what landed before the editor opened, the work waiting for review and
the failure, none for an answer you stopped nor for one dropped, and none
left once they are taken; the follow-up's clicked, the review of the work
it follows up open; and a notice clicked with a menu open, nothing done,
then with nothing open, its stack shown and the review of its work open,
the failure's on the review, nothing done, then on the main screen, its
stack shown and why it failed said again in the bottom line, and one of a
run that is not there, nothing. With the `web` feature, `tests/web.rs`
serves the editor and asks it over HTTP as a browser would: the token
needed, the page, the wasm, StackED's icon at the two sizes the page links
for its tab, asked for as the page asks, with the token, and refused
without it, as `/favicon.ico` is, the rows as they change, a key and a click,
1 on the versions of a patch opening the dialog of the views of the
interdiffs, a click on the view opening its list and one on side by side
there setting it, the version drawn so at once, and the change from a
version to the next, the code set so on Compare and kept with Done, drawn
as a patch of its own once its files are read, and only what the patches
changed, set so on Lines and kept with Done, the lines left out said where
they were, with moz-phab the review of what a submit sends, its menu
opened by a right click, its view of the code opening the dialog, the
code set there by clicks in its list drawn unified at once and kept with
Done, and Cancel: send nothing, in the menu Esc opens, sending nothing,
HEAD's row pressed, dragged
onto patch 1 and let go there, `stacked web` run in the checkout, HEAD
put down under patch 1, a line of the review's list marked by the mouse
over it, underlined in the runs of its row and not reversed, then picked
by a click, reversed, and so a run of `Runs…`, opened from a menu,
the wheel's ↑ after a key, sent with the cell under the mouse, on a patch,
listing files, going to the last file of the patch above, whatever is
under the mouse, and the editor left, the page told, the worktree made ahead
removed (Quit in the page; `stacked web` itself sent SIGINT, SIGTERM or
SIGHUP, the first while `$EDITOR` runs in the terminal, as the settings
say, left to `$EDITOR`, the second waiting for it), and two
`stacked web` at once, each putting its database, named from where it
runs, on the list StackED's Open Recent shows, the port `stacked web`
listens on, a free one found for a test by binding 0 first, as the code
does, and all tried again on another, up to ten times, should another
process take that one before `stacked web` listens on it, as it then says,
so that only stacked can fail them (any free one each time, unset or `"random"`, the one taken before
held meanwhile, nothing written in the settings, the page's events saying
the address kept for no launch after, and no token kept; with `"fixed-random"`,
any free one the first time, written in the settings beside `port` as
`kept_port`, the rest of the file as it was, then that one on another
database, with the token the first kept for it, its tab given 2 seconds to
come back, none there, then a new token, said so, `--no-open` opening
nothing, the first one's refused there, the new one kept beside the state,
nothing written in the databases nor in `state.json`; a port set listened
on, the port kept passed over though it is in use, its token kept for it,
and `--port` giving that very port a token of its own, nothing waited for;
`--port` going over a port set
that is in use, 0 any free one, and over `"fixed-random"` keeping nothing,
nor any token, the one kept as it was;
a port in use, set, kept or given, said in a line, `stacked web` exiting
with 1, the settings as they were; and the port `"fixed-random"` took, in
a file where it would not read back, its table's header written `[ web
]`, said, and listened on all the same, the file as it was), the address
of a launch on a port the settings set (no token kept yet, one of its own
kept for it beside the state, in a file of mode 0600, the browser opened
at once by a stand-in for its opener, `$STACKED_OPEN`, nothing said before
where the editor is; the next launch there, the tab asking for its events
again as soon as it listens, saying the launch it had them from
(`Last-Event-ID`): come back, said so, the first one's token kept and
served, no new tab opened, the tab told first which launch sends its
events (`v BUILD LAUNCH KEPT`, the build the same, the launch another,
its address kept), with the browser to ask again a second after losing
them (`retry: 1000`, the launch as their `id`), then that stacked web runs
again (`n stacked-web US stacked US stacked web is running again: click to
switch to this tab.`), then the screen whole; with no tab back, the next
launch there waiting 2 seconds, no less, then taking a new token, kept in
the first one's place, said so, a new tab opened on it, and the first
one's refused, its tab's events too; each tried again on another port should
another process take it first, or the tab come back later than 2
seconds), a tab come back from another launch told that stacked web runs
again, before the screen, with no bell, whether it says that launch as
the browser does or in the address of a page loaded again (`l`), none told
for the same launch, nor with `web.notifications = false`; the last tab closed, saying
it goes (`g`) and its events closed, `stacked web` quitting as Quit does,
no sooner than 2 seconds after, saying why, the worktree made ahead
removed; a tab loaded again, saying it goes and back at once, quitting
nothing past the 2 seconds, and served (tried again should it come back
later); a page saying it goes before any came, then one cut off without
saying it goes, quitting nothing, the page served once it comes back; and
of two tabs, one closed quitting nothing, the other served, then the other
closed quitting, the ask dialog (opened with
nothing set from `Ask agent about all…`, clicked, its permission set by
clicks on its line and on an option of the list that opens under it, then
stepped by → and back by ←, its model and its effort picked in the lists Enter opens, ↑
and ↓ going round it, its lines set by → alone too, Ask Now grayed as
claude is not found, saying so, Ask Later putting the comments in the
batch; Esc closing it, a second one unset), the effort of `Send it
now`, picked there alone, ← → no further than `low` and `max` and a click
on it going round from `max` to `low`, the agent's turns headed with the
effort it was asked at and its landed work named with it, in the banner
and its review, a turn of before as ever, a message's long lines wrapped
from its patch's menu, the edit landing for review, what waits its turn
(a reconcile with HEAD, a lint
job, an edit ask) taken out of the queue from the banner with Delete, each
asked first, → opening the comments pane of a patch with none, saying why,
its menu there, and ← putting it away, a comment on a patch the agent's
work adds, and what you write in a thread the agent opened that it was
asked about, written in its review, listed with what you wrote and
sending the work back (Esc's `Send it back to the agent…` opening the ask
dialog, nothing set, Send Now and Cancel its buttons), Esc's summary in a
review sent with its count of
the tools refused in red, then of those allowed in green, the rest dim,
in the runs of the row the page draws, the stack operations of a change
each marked on its own in its review, the title counting them, the patch
left as it was with no mark, and one accepted alone, its `✓` in green in
the runs of its row, the review closed in part from the page's keys, the
patch the work adds rejected alone, closing once its check away from the
screen is done, as the rows the page is sent say, `Move HEAD…` on the stack's menu
not grayed while it would not move HEAD, and the view of why opened from
it, then left, a link of a line of a message opened from the
menu of a right click on it and with `o` (the status line saying it is
opening it while the stand-in is held, then that it opened it), and one
of a line of a comment from its menu, by a stand-in, and the comment
closed last on a line, longer than its box in a small page, scrolled
down to its last line by the wheel over the box, sent with its cell, the
mouse in use since the click that opened the box, nothing over the top
bar, then by pgdn, by the wheel's ↓ after that key over the top bar, and
by ↓, then up a line by the wheel over the box once the mouse moved, and, with
moz-phab, the Phabricator line saying first that the revision is not
fetched yet, nothing read as the page opens; then, fetched from the
stack's `Phabricator` menu, its entry then its item clicked, by a stand-in for its database, the status
of a patch's revision after its number, a letter, then its mark (`✗`, its
diff otherwise) on the screen its Phabricator line says so, the next
patch's title after the column all the same, expanded and listing files, a
title folded there, and the Phabricator line over its message, Space on it
showing what differs on a screen of its own, Esc going back to the line,
and its revision pulled from its menu, an item clicked, by the same
stand-in, its diff a new version of the patch and a reviewer's comment
held for review, the review then opened from the banner, its operation
marked on its own; the notices, each sent as `n RUN US TITLE US BODY` (an
answer landing for review on the stack not shown, `stacked: s`, `A landed
ask waits for your review: 1 reply, asked "Why add b?".`, then a change
failing on the stack shown, `stacked: t`, `The agent's change failed: the
agent failed (1): rate limited.`, each alone, and the answer's clicked
(`n RUN`), its stack shown and the review of its work open), none for
what was there as `stacked web` started (work waiting for review, a
failure), the next landing alone, and a second page, come after it, told
none of it, but the landing after, which the first page is told too; and,
in `stacked web` runs for real, the bell (`b LEVEL`) as the settings say:
`web.bell = true`, an answer and a failure landed at once, in one edit,
told by their two notices, then by one bell after both, at 100, before the
screen that says them, then the next landing alone by its notice and one
bell; `web.bell = false`, the notice alone; `web.notifications = false`
and `web.bell = 50`, no notice but one bell at 50 for each landing, the landing
and the failure said on the screen all the same; both false, neither, the
landing and the failure said on the screen all the same; and, with the
`try-push` feature, a try push followed by the editor
once it knows of it, its look answered by a stand-in for Lando and
Treeherder (`try_push::web_stand_in`, and `try_push::look_every` with no
wait between looks), done on Treeherder, one of its two tasks failed, its
notice saying so (`The try push is done: 1 of its 2 tasks failed. Its
summary waits for your review.`); `$EDITOR` running elsewhere than in the
page, in a `stacked web` run for real, its stand-in a script that waits
for a file, in the terminal `stacked web` was started from, as the
settings say (`web.editor = "terminal"`), and, the settings unset, one
named `code`, with a window of its own: the first patch
edited from its menu, the page told first, `e slowedit` (`e code`), the
program it runs, a page come while it runs told it at once, after which
stacked serves it, before any screen, and,
the editor done, each page sent the screen whole, no row before it, the
bottom line saying `nothing changed`; in the terminal, an editor that
cannot run told all the same, `e noedit`, then the screen whole at once,
the bottom line saying why (`…/noedit failed; nothing changed`); what the
pages send while it runs, either way, dropped (from the first, the keys
that would write a comment on the stack and save it, and a click on the
second patch; from a second page come meanwhile, ↓ and a right click, a
menu), but a new size: each page sent the screen whole after at that size,
the bottom line saying `nothing changed`, the first patch still in the
middle, no menu, no comment, nothing written in the database, then a key
sent after done; and `$EDITOR`, a terminal one, running in the page, the
settings unset, its stand-in a script: one that writes a screen (red,
bold and underlined; 256's 196; on RGB; a wide character), its terminal's
size, then reads a line and puts it in the first patch's title, its screen
sent the page whole, no word over it, its size, then `t`, then its rows,
their runs as it wrote them, the page's size its terminal's (`size 30
120`), the cursor after what it wrote, then the line typed in the page
read, and taken as an edit of the patch is, waiting for your review in the
held work, stacked's screen sent whole after, no `t` after its size; one
that says its terminal's size each tenth of a second, a size a page sends
its terminal's, its screen sent whole at that size after `t`, a page come
meanwhile sent it whole at once, before any other screen, its own size the
terminal's then, and, the editor done, each page sent stacked's screen
whole at that size, no `t` after it, the bottom line saying `nothing
changed`; one that waits with a process it started, hung up on as `stacked
web` goes, by a ctrl-c where it runs, a kill (the editor deaf to the
hangup, killed 2 seconds after), or its last tab closed (quitting no
sooner than 2 seconds after, saying why), the page told `bye` but once
closed, `stacked web` done, and neither of the two left running; and one
that cannot run,
the bottom line saying why (`…/noedit failed; nothing changed`), no word
said over the screen;
and
`web.rs` tests the rows and the lines both ways, the line a page's events
start with as the client reads it, its build the same each time, the page
going (`g`) alone, the notices learned at once told as the settings say
(each notice's line, then one bell at its level; the notices' lines alone
at the level 0; the bell alone without the banners; nothing with neither,
nor for no notice), as the client reads them, a test button's press with
the test flag last, the screen of the editor run in the
page sent whole, its size, then `t`, then its rows, as the client reads
them, the keys the client makes while the screen is the editor's made the
bytes xterm sends (F5, ↑ in the cursor keys' application mode, ctrl-c,
Shift-Tab), a character Option made on a Mac or AltGr elsewhere, its line
as the client makes it, the same whether the screen is stacked's own or
the editor's, the same character for both (∂ for Option-D, @ for Option-L
on a German keyboard, [ for Option-Shift-5 on a French one, é for E after
the dead key Option-E, which makes no line, @ for AltGr-Q said with ctrl
and alt as Windows says it, € for AltGr-E as Linux says it): with no
modifier for stacked's own screen, which a comment's box types, its UTF-8
alone for the editor, with no Esc; and alt where Option made none (d for
Option-D) and elsewhere with a character (alt-f), alt with it for
stacked's own screen, which the box does not type, Esc first for the
editor, Option-← alt-← and `ESC [ 1 ; 3 D`; and a paste's text, as the
client sends it, read back as it
was, none for what is not hex, and a token kept for its port alone, in a file of mode
0600 whatever the umask or a mode opened meanwhile, written whole beside
it, nothing left there, none read of what is not a token, and nothing kept
nowhere, nor where a folder is in its place, with the client's own tests
(`web/client.rs`, compiled to the machine as well as to wasm), which also
test the wheel said once it turned a notch, with the cell under the mouse
(none over no cell), what it turned short of a notch kept, the wheel
turned in lines or in pages a line for each turn, whatever its size (3, 1
and −2 lines, a page), none for a turn sideways alone, nothing kept of
those turns, and what the pixels kept left as it was by turns in lines
between them, the mouse
moved with the left button down said as `d` where it would be `v`, and
the button let go with its cell (none off the screen), and
the mouse followed only while the page has the focus: the cell where
the mouse was last seen while the page did not have the focus said at once
when the page has it again (unless the mouse left the page), so that the
click that gives the page the focus back acts on that cell, whether the
browser sends the click before the focus or after; and a notice made what
the page shows of it, its tag, its title, its body and the line its click
sends the server, each on a line of its own, a new line in its title or
its body a space, and nothing for one short of a field, or whose run is
empty or holds a space, and a test's with `test` last; the bell's line, `b
LEVEL`, the bell at that level, `b LEVEL test` a test's, `bye` still the
editor gone, `b` alone, at 0, past 100 or with anything else nothing; `$EDITOR` running made the words said over
the screen, by the program it runs, `the editor` when none is named; `t`
alone, the screen the editor's own, the next size stacked's again, `t`
with more nothing; F1 to F12 and Insert the browser's, but while the
screen is the editor's, with their modifiers, ⌘ the browser's all the
same, and the other keys as ever, ctrl-c among them; a character Option
made on a Mac as it is, without alt, and elsewhere with alt (∂ for
Option-D, Î for Option-Shift-D, { for Option-5 and [ for Option-Shift-5
on a French keyboard, [ for Option-5 and @ for Option-L on a German one,
and on keys other than a letter or a digit … for Option-;, € for
Option-Shift-2 and a no-break space for Option-Space), alt kept where
Option made none, the key's own typed (d for Option-D, D for
Option-Shift-D), ctrl kept either way, a dead key (Option-E) no line,
then the character it makes, é, as it is, Option let go or held, and
Option with the arrows, Backspace and Enter alt, as ever; a character
AltGr made elsewhere as it is, without ctrl and alt (@ for AltGr-Q, said
with them as Windows says it, and alone as Linux does, € for AltGr-E, Ð
for AltGr-Shift-D), ctrl and alt as said where it made none (d), alt and
ctrl-alt without it as they are, a dead key with it no line, ← with it as
said, and on a Mac, AltGr said with Option as Firefox says it, Option
deciding; each of those lines the same whether the screen is stacked's
own or the editor's; a paste then its
text, its UTF-8 in hex, none for no text; and
which stacked serves the page made what the page keeps of it, nothing for
one short of a field or with one more, an empty field, one the page could
not say again in its address, or whether the address is kept neither 1
nor 0. `pty.rs` tests the keys made the bytes xterm sends (a character as
UTF-8, ctrl's control bytes, alt as Esc first, Enter, Tab, Shift-Tab,
Backspace, with ctrl too, and Esc; the arrows, Home and End as CSI or, in
the cursor keys' application mode, SS3, with each modifier's parameter as
CSI either way; Insert, Delete, Page Up and Down, F1 to F12, with a
modifier too; none for a line that names no key), a paste made the text
the editor is given (its new lines carriage returns, a tab as it is, Esc
dropped, so that a paste cannot end its brackets early, bracketed when
asked), the editor's screen made a buffer (its colours, 16's, 256's and
RGB; bold, underline, italic, dim and inverse; a wide character over two
cells, what comes after it in its place; the cursor where the editor put
it, none while it hides it, on a wide character's first half; the screen
resized; and a row as `stacked web` sends it), what the editor asks of
its terminal answered (DA, its version, DSR 5 and 6 and DEC's, OSC 10 and
11, several at once in order, nothing else) and its modes followed (the
cursor keys', bracketed pastes), the mouse reported as the editor asks
(nothing unasked; presses and releases as SGR's, from 1, the right button
let go at once, the wheel a report for each line, nothing over no cell nor
off the screen; the mouse moved with the left button down, then anywhere,
once asked; xterm's first encoding without SGR; presses alone, as X10's;
nothing once turned off), an editor run in a terminal of its own, a
stand-in (its size, a session it leads whose controlling terminal the
terminal is, `TERM`, `LINES` taken away, a key typed and echoed, its size
changed, how it exited, its terminal then closed and read to its end),
and an editor hung up on gone with what it started, as soon as it hears
it, or killed 2 seconds after, deaf to it. With
the `app` feature, `tests/app.rs` runs StackED's editor as its window runs
it, with no window: nothing drawn before the window's size, a key, a
click, 1 on the versions of a patch opening the dialog of the views of
the interdiffs, a click on the view opening its list and one on side by
side there setting it, the version drawn so at once, and the change from
a version to the next, the code set so on Compare and kept with Done,
drawn as a patch of its own once its files are read, and only what the
patches changed, set so on Lines and kept with Done, the lines left out
said where they were, with moz-phab the review of what a submit sends, its
menu opened by a right click, its view of the code opening the dialog,
the code set there by clicks in its list drawn unified at once and kept
with Done, and Cancel: send nothing, in the menu Esc opens, sending
nothing,
the screen sent saying HEAD's row is where a press is held for a drag,
HEAD's row pressed, dragged onto patch 1 and let go there, HEAD put down
under patch 1, a line of the review's list marked by the mouse over it,
underlined and not reversed on the screen the window paints, then picked
by a click, reversed, and so a version of a patch, the pane beside the
list showing what changed from the version before until the click, then
the version whole,
the wheel's ↑ after a key, sent with the cell under the mouse, on a
patch, listing files, going to the last file of the patch above, whatever
is under the mouse, the mouse over a patch then gone, a window
too small, the wait while `$EDITOR` runs (an editor that edits nothing,
set in the test's settings), quit, the command there, a link of a line of
a message opened from the
menu of a right click on it and with `o` (the status line saying it is
opening it while the stand-in is held, then that it opened it), and one
of a line of a comment from its menu, by a stand-in, the comment closed
last on a line, longer
than its box in a small window, scrolled down to its last line by the
wheel over the box, sent with its cell, the mouse in use since the click
that opened the box, nothing over the top bar, then by pgdn, by the
wheel's ↓ after that key over the top bar, and by ↓, then up a line by
the wheel over the box once the mouse moved, and a database
that cannot be read said before any window; two editors in one process,
as two windows, each in its own
folder (HEAD said in the checkout's, none elsewhere); a message's long
lines wrapped from its patch's menu, the edit landing for review; what
waits its turn (a reconcile with HEAD, a lint job, an edit ask) taken out
of the queue from the banner with Delete, each asked first; → opening the
comments pane of a patch with none, saying why, its menu there, and ←
putting it away; a comment on a patch the agent's work adds, written in
its review, listed with what you wrote and sending the work back, Esc's
`Send it back to the agent…` opening the ask dialog, nothing set, Send Now
and Cancel its buttons; Esc's
summary in a review, its count of the tools refused in red, then of
those allowed in green, the rest dim, cell by cell of the screen the
window paints; the stack operations of a change each marked on its own in
its review, the title counting them, the patch left as it was with no
mark, and one accepted alone, its `✓` in green on the screen the window
paints, and the review closed in part from its keys, the patch the work
adds rejected alone, closing once its check away from the screen is
done, as the screen it paints says; `Move HEAD…` on the stack's menu drawn as any item while
it would not move HEAD, and the view of why opened from it, then left; the
ask dialog opened with nothing set from a click on `Ask agent about all…`,
Ask Now and Ask Later drawn dim, Ask Now as claude is not found, which it
says, a click on its permission's line opening the list of its options
and a click on one of them picking it, Enter on its model's line and on its effort's line opening
that line's list and Enter again picking low, and a click on Ask Later
putting the comment in the batch; the agent's turns headed
with the effort it was asked at, and its landed work named with it, in
the banner and its review, a turn of before as ever; with
moz-phab, the Phabricator line saying first that the revision is not
fetched yet, nothing read as the window opens; then, fetched from the
stack's `Phabricator` menu, its entry then its item clicked, by a stand-in for its database, the status
of a patch's revision after its number, a letter, then its mark (`✗`, its
diff otherwise) on the screen its Phabricator line says so, the next
patch's title after the column all the same, expanded and listing files, a
title folded there, and the Phabricator line over its message, Space on it
showing what differs on a screen of its own, Esc going back to the line,
and its revision pulled from its menu, an item clicked, by the same
stand-in, its diff a new version of the patch and a reviewer's comment
held for review, the review then opened from the banner, its operation
marked on its own; the File menu, its
dialogs answered by the test: Open… opening each database picked in a
window of its own (one picked twice, one that cannot be read said, the
dialog cancelled), New… making an empty database that stacked reads and
never replacing a file there (its bytes the same), and Open Recent taking
a database deleted off the menu and off the state kept, then Clear Menu;
and, elsewhere than on macOS, `stacked app --bundle` making StackED's
launcher in the test's own data folder (`$XDG_DATA_HOME`, else the test's
home's ~/.local/share, else the folder named), the file the desktop
reads, this stacked in its Exec line and its icon at every size, made
again, and something else of its name left alone.
`app.rs` tests what the window paints of a screen (the colours, reversed
and dim, the faces, wide characters, the lines of boxes and blocks), what
it makes of keys, clicks, the wheel (sent with the cell the mouse was last
seen over, none off the grid or once the mouse is gone, in the background
too; in lines, 40 points a line, or in pages, what turned short of one
kept; while a comment is written too), a press where the editor drags
(HEAD's row, as the screen sent says), the editor's: the press, each
other cell the mouse goes over once, and the button let go, with its cell
or none off the grid, nothing selected, every other drag selecting text
and every other press let go where it was pressed a click, and
pastes, and the window itself, run
by egui with no window: the grid's size sent, the screen painted, what is
done sent, the wait for `$EDITOR`, and closing, and text dragged over lit
and copied, kept to its pane (a block with alt, a drag past the grid up to
its edge, a key letting it go), and the mouse followed only while the
window is the active one; and the fonts, the system's monospace font
where it has one (Menlo where macOS keeps it, its own bold and italics,
and Apple Symbols; DejaVu Sans Mono where Linux keeps it), else Hack, each
face holding every character stacked draws where a font for symbols is
found. `app/menu.rs` tests the menu bar, run by egui with no window: File
its only menu, its items with no shortcut beside them on either system;
ctrl-n, ctrl-o, ctrl-w and ctrl-q, and ⌘ with them on macOS, left as they
came, nothing picked; Tab, Enter and Space taking no item; an item
clicked once its menu opens, Open… asking the dialogs on their thread (a
stand-in's), Open Recent read as the menu opens, Close Window and Quit
StackED. `app/windows.rs` tests StackED's
windows, run by egui with no window as eframe runs them: the first's empty
with no database, its menu bar there, then a window for each database
opened (one twice), the first in the root's, each telling its own editor
its size under the bar and following the mouse only while it is the
active one, what is done in a menu open not sent the editor, ctrl-n,
ctrl-o, ctrl-w and ctrl-q sent the editor on either system and ⌘ with them
neither the editor's nor the menu's on macOS, no shortcut in the File
menu, egui's own text font drawing all the bar and the empty window show,
a window closed telling its editor to leave, the root's closed taking over
the latest other (its title, its place and its size), the last closed
letting StackED quit, its window kept, and Quit keeping the windows open,
from the File menu too, and waiting for each editor to leave. `state.rs` tests what is kept
between runs: the recent databases, the latest first, once each and eight
at most, cleared; the searches of the middle, the latest first, once each
and twenty at most, kept beside them; the effort a stacked of before kept
beside them passed over, the rest read as it is, and gone once the file
is written; those deleted taken off both lists; a file that cannot be
read; writers at once, each keeping what it wrote (with no lock, some
would not); a file of another kind (the null device) keeping nothing; and
the editor's database kept nowhere in a test's own process.
`launcher.rs` tests, on every system, when stacked is opened as macOS
opens StackED.app and as the launcher opens it, StackED.app's Info.plist,
the launcher's file (and what its Exec line escapes), the launcher and its
icon made in a data folder of the test's and made again, the terminal
found (the one `$TERMINAL` names, with its own words, else the first of
the list on `$PATH`, a file there that does not run passed over), and an
edit run in a terminal window, a stand-in for x-terminal-emulator given
`-e`, sh and the script, a terminal that fails at once said, and no
terminal found said, nothing run; on macOS, it tests that the Edit menu
put on the menu bar for the panels has the titles and shortcuts it has in
any Mac app, with actions that AppKit's text fields and windows answer. On
macOS, `tests/app.rs` makes StackED.app as macOS reads an app
(its Info.plist, its signature, its icon at every size, the stacked in it,
which runs), makes it again, and leaves alone something else of its name.
`bundle.rs` tests the login shell's environment read (a shell that never
ends given up, the shell the user's), an edit in a window of its own
waited for (in its folder, it failing, its window closed before it exits),
and the script a terminal is given
to run it (for its owner alone, removing itself as it runs the edit,
removed if the terminal cannot be opened); `refs.rs`, that what is not a
web link is never handed to the system's way of opening links;
`icon.rs`, with `web` too, what the icon shows where, at its largest (the
sheets' edges) and at its smallest (a plain white S), and that it is grey
all over; and the PNG stacked writes of it, at the page's two sizes and at
one that takes three of deflate's stored blocks: its signature, the size
and kind its IHDR says, each chunk's CRC-32, the zlib stream's header and
Adler-32 (each check first giving its definition's own example), its
blocks, and its rows read back, unfiltered, as the icon's pixels, as
eframe's reader reads them too, with `app`. With
the `demo` feature, `tests/demo.rs` takes the tour, each step done as Tab
does it: the
stand-in's fix, the board at its end, nothing made on disk; the answer
to the tour's question taken at once with Enter, the tour going on then;
and the views taught with 3, picked in the menu of 1 and making their
room on a middle too narrow, the step waiting for the hunks again;
`memory.rs` tests its diffs applied as git applies them. With the
`try-push` feature, `tests/try_push.rs` drives the editor as
`tests/tui.rs` does, a stand-in of its own for claude: a comment asked as
a try push, `try push` among the comment's choices of how, its ask in a
worktree of its own, your checkout as it was while it works and after, not
given the checkout's linter as a change is, its prompt
asking for the tasks the comment asks for, as few or as many as it says,
exact labels, why each, nothing pushed or changed; its tasks landing in
the banner as read-only work, listed in the review, why each was selected
shown as the agent's comment, marked by Enter and from the menu, answered
there and followed up as a try push, the work staying for your review,
the answer yours after the agent's reply as the thread would be, the
thread itself as it was, the follow-up told what was selected before as
you marked it, and its tasks coming into the same review, but the one the
work held already, which keeps its mark; closing with tasks accepted
saying what it pushes,
with none accepted that nothing is pushed, and past mach's 1,000 tasks,
each task counted as many times as it runs, that they would run at the
lowest priority; the first try push going at once, with no setup, the
setup's old mark passed over; and a try push batched, sent from the banner on
its own, and with the batch that `stacked send NAME --batch` sends. Before
each ask's agent starts, the tasks mach can push are listed, by the same
stand-in for mach as the push's, which has a task graph of its own when a
test gives it one, the tests of xpcshell in four chunks there: the ask
waiting for the list, held as mach lists, its comment's line and the
banner saying so in plain words, the agent not started; mach run as the
push's check runs it, with the preset of the lowest priority and the query
every label matches, stdin closed, output not held back, in a worktree for
pushes held in the ask's run and made ready as a push's, the patch alone
on its base; the labels written each once, sorted, a label a line, in the
ask's folder of its own, the worktree then given back and the presets
file gone; the stream saying what stacked did, the prompt naming the list
as where every label is and the agent given that prompt; and labels taken
from the list passing the push's check, pushed from the worktree they were
listed in. A list mach could not make, and one whose worktree could not
be made ready (the base on no remote of Firefox's, mach never run), stop
nothing: the prompt says why in a line, the labels checked against CI's
task graph as before, the stream saying why after mach's last lines, and
the ask lands its work. An ask lists in the next worktree for pushes while
a push waiting for mach's login holds the first, and a push begun as it
lists takes neither. The push runs a stand-in for mach (`$STACKED_MACH`,
which a stacked that keeps nothing, as the tests' own, needs, never
running a mach of a worktree's; the ask's wrapper is given it too, as the
command line's `send` is), the repository's remote of Firefox's a name and
a ref, never fetched: the push checked with `--no-push`, then made, mach's
stdin closed and its output not held back, the tasks in one query, the
commits it would push
the patch alone; its login's page and code in the status line and the
comment's stream, the page opened from the comment's menu; its link a
reply, a web link, the comment locked until you stop following it, which
says so with the link; past the threshold, both runs given the preset of
the lowest priority from a presets file of stacked's own, gone once the
push is done; a check that would push another task, a question mach asks
(quoted, with what to change), and a push that fails (mach's last lines)
each said in a reply, nothing pushed, the comment unlocked; a push
stopped as mach waits for the login; a base none of the remotes of
Firefox's has, or no remote at all, keeping the review open, mach never
run. The push followed answers to a stand-in for Lando and Treeherder
(`try_push::web_stand_in`, which a process that keeps nothing, as the
tests' own, needs, never reaching the web): Lando's state of it in the
banner, then its tasks as Treeherder shows them, in the banner's section
of try pushes and its first line, and in the stream, the pages read as
Lando's and Treeherder's APIs have them; done, a retry counted by its last
run, its summary held for review (the counts by result, each task that
failed with its link, what the push ran for them that failed, each a web
link), taken from the comment as an answer only's is, the comment
unlocked; the tasks Treeherder never shows counted as not seen once
nothing runs any more; a push Lando did not push to try summed up so; and
a push followed picked up by the next editor from where the database has
it. `try_push.rs` tests its tasks read from the answer (each label once,
the first reason kept, one with a space or none left out, a reason with
references), how many times each runs, from 2 to 20, the prompt, and what
a round asked again is told; the prompt with the list of the tasks mach
can push, where every label is selected from and checked against with grep
or jq, CI's lists hints beside it, and without it, why in a line, CI's
task graph what each label is checked against; mach's arguments, the tasks
in one query, and how the stream says them; mach's arguments to list every
task, the check's with the preset of the lowest priority and the query
every label matches, the labels it lists each once, sorted, and what the
stream says of a list not made; the worktree for pushes held by a push
until it pushed and by an ask while it lists, by neither once over; what
`--no-push` says it would push, read and
compared (the tasks, the runs, the priority); a question mach stopped on,
its login, its link and Lando's job read from what it writes; that a
process keeping nothing runs only a stand-in for mach, and reaches
neither Lando nor Treeherder; the remotes mach takes as Firefox's (on
GitHub, and hg.mozilla.org's but try in a git-cinnabar clone), a base none
of them has, and the commits mach would push; and a push's tasks counted as
Treeherder shows them (each by its last run, each run of a rebuild, those
not seen), its summary, and where Lando answers, as `.lando.ini` says;
and what a push over came to, as `stacked web`'s notice says it: done,
with how many of its tasks failed, none too, then how many never ended and
how many were not seen on Treeherder, or not pushed to try, as Lando says;
and nothing for one that failed, is followed no longer or is followed
still;
`ask.rs`, that a try push's ask is given what an answer is and its own
list, not the linter a change is given, and is asked for its tasks and
their runs; that its prompt names the list of the tasks mach can push in
its folder of its own, and, none made, says why in its place; and that its
stream says in plain words, on its comment's line, that stacked lists them,
then what came of it; `setup.rs`, the column of
try pushes, ticked at first as the read-only one, its own once ticked, in
what saving writes, and line by line `read only` for both read-only kinds;
`refusals.rs`, what changes things noted for a try push's selection, as
for an answer, and what can write files or run programs, what bundles its
short options, and what stacked refuses it as an answer, too;
`config.rs`, `try_tools` written and read back, and read built without
the feature too. `tests/lint.rs` drives the
editor as `tests/tui.rs` does, a stand-in run in place of the linter
(`lint::stand_in`, `$STACKED_LINT` to the job's process, which keeps
nothing and so runs no other) that takes out the white space lines end
with and reports the lines it cannot fix, exiting with 1, and a stand-in
for claude: `Lint the stack` on the stack's header alone, grayed with why
where there is no linter, where lint is off, while an edit by hand waits
for review, and on a read-only stack, and named once set; a checkout with
`./mach` at its top linting with `./mach lint --fix`, the stand-in given
mach's arguments, nothing written; each patch linted at its place, on its
files, in the stack's lint worktree, its stdin closed, the checkout not
touched, each fix folded into the earliest patch it shows in, the change
landing for review as one, named so in the banner, the review and the
history, what each of its operations needs stored as it landed, nothing
needing another, each marked on its own; a fix that would keep the patch above from applying stopping the
patches' run, the linter run once at the top, its fixes in a patch at the
end with your comment on it, ready to send and sent to no one; nothing
landing when the linter fixes and reports nothing, said quietly; what it
reports listed in the review and by `stacked landed`, and landing alone,
read only, to read and close; an edit asked while the linter runs
waiting for it, the stack not edited by hand meanwhile (the menus and the
commands saying why), an answer going at once, then the edit going on from the stack the linter made;
one lint job at a time of every stack, the next waiting its turn; a job
whose process is gone landed as failed, the stack free again; and a
linter that hangs stopped from the banner, asked first, and by `stacked
stop`, the job landing as stopped with nothing landed and the stack as it
was, the stack free again and the edit that waited for it going, and a
job waiting its turn dropped the same way, asked first, by Delete on its
line of the banner and from the banner's menu, where it is next to `Read
what the linter writes`, and the stack's, the edit asked after it going
then while another stack's linter still runs.
`tests/cli.rs` sets the linter up: in the checkout's own settings alone,
kept out of git's sight, never in the settings' file, which is passed over
and said if it sets one; printed with where it comes from; turned off,
and that kept over the default of `./mach`; `stacked config` refusing to
write it but reading it; refused in a settings file git tracks and outside
a checkout. It shows a change, of the stack or of a patch, given the
checkout's linter, with `./mach` at its top or one set over it, as the
rule a refused call of it is offered with, after what stacked gives and
before what the settings add, and its prompt naming the command; an
answer and a build and test not given it, nor a change with lint off or
no linter; and `stacked setup agent` naming it, or saying the checkout has
none. `tests/web.rs` and `tests/app.rs` show the item in the
browser and in StackED, and also that a reviewer's edit is headed by what
it edits, the turn it edits redacted, and that a pull's review rejects
nothing there either. `lint.rs` tests a command split as a shell's
words and written back, only a stand-in run from a process that keeps
nothing, what the history says of the fixes, a patch's sections spliced
with those written again, and the patches' run itself on a repository of
its own: each fix in the earliest patch it shows in, a fix that would keep
a patch above from applying stopping it with the rest in a patch at the
end (its message wrapped), a clean stack left as it is with what is
reported kept, and a fix that adds a file going at the end. In
`ask.rs`, an answer and a try push's selection are given the commands
that only read and git's that only read, `Grep` too, not `sort`, `file`
nor `git grep`, whose short options can be bundled, nor `sed`, `awk`,
`uniq`, `tree` or git at large; they are refused the options that write a
file or run a program, long or shortened (`git log --output`, `file
--compile`, `git grep --open`), none that only reads, and none twice. A
change is given what it was, and refused, as every ask but a build and
test, `rg --hostname-bin` and `sort --co` too; a build and test is refused
nothing, nor is an experiment, given Bash at large and what a change is.
In `cli.rs`, an experiment batched and sent with the batch is told it is
one and how to mark what it changed only to find out (`Throwaway: yes`),
not that it cannot build, and is given Bash at large; its change, a patch
worth keeping and a throwaway one, lands as experiment, the stack as it
was, every operation rejected, its reply to review; closed so, the reply
goes in its thread, the stack still as it was. The
stream's thinking is tested in `ask.rs`, with the prompt asking the agent
to point at the stack before the web and its answer's schema taking a web
link, the ask's model then its effort given to claude after the settings' arguments, in
place of theirs, a child process that has exited not taken for alive
though `kill` still finds it, its exit left for whoever started it, the
wrapper of an ask the process started waited for once it has exited, and
an ask whose agent you stopped landing as stopped, unless the agent was
done as you stopped it, and the thread the agent is told, a reviewer's edit
and the turn it redacts in plain words, and their comment read again with
its suggested edit and the turn it takes the place of, told for what they
are, no edit; what is said of landed work left
to review is tested in `held.rs`, with a reviewer's edit as their latest
turn, the turn it edits redacted, again on a later edit, nothing redacted
when that turn is gone; their comment read again with its suggested edit
in the place of their turn, redacted, headed for what it is, and named
and counted apart from edits, as the review's closing says it too; and a
reviewer's reply to a comment you closed
reopening it once accepted, as the review closes, taken at once or sent
back with the agent's work, as the pull kept it, as its history has it
last when written since, filed where it is open since rather than
reopened, read from a checkpoint without its history read, while the
agent's goes nowhere and a pull's parts are not rejected; and what a
follow-up brings back coming into the review of the work it follows up:
an answer's reply after the work's, your answer to the reply before it
gone with it, the thread as it would be holding each in turn, the tools
it could not use but those the work holds; a build and test's reply to
read, no part, what it found of each patch with the follow-up; nothing
from one failed with nothing to bring, nor once the work's review is
closed. Those in
`thread.rs` show the fences of a reviewer's edit and of a turn redacted
read and written again, their headings, the first line an edit quotes cut
short with `…`, and a name alone as ever; the fence of their comment read
anew with its suggested edit read and written again, headed for what it
is, the turn it replaces found by the line it quotes, and an edit's not
taken for one; and the model and the effort on the fence of
the agent's turn, after the way, read, headed and named with it, kept as
the thread is written again, a reply added, a turn edited or one holding
a code block, a turn of before read and written as it was, a comment the
agent opened at an effort or with a model, and a reviewer named `effort` or `model` a name as ever;
the stack an answer was asked on, last on its fence, after the effort or
alone, read as the way and the effort as ever, kept as the thread is
written again, a word after `stack` that is no rev read as part of the
way, and how old that stack is said; in `ask.rs`, the thread the agent is
told says none of it. The
tests in `check.rs` show
the check's index holds only what the patches touch, that it writes nothing
to the repository (nor does the tree a patch leaves, written for a
submit), how a patch's files are read before and after it, and where a
diff of the same effect as its draft does otherwise (git applying
it at another copy of its lines, it not applying), the drafts not
applying an error, and which patch of a stack is the first that does not
apply (none, the second of one patch twice, one with no diff), kept for
the process as a stack seen to apply is, a base not there no answer;
those in `operations.rs`, that each line of a stack diff is an operation
on its entry, a patch removed and one added in its place among them, the
stacks the operations leave keeping the order of both stacks (all of
them, the stack after; none, the stack before; the remove rejected, the
patch back where it was), which operation needs which (a patch added
over a new version needing it, and a patch's new version rebased on the
new version under it, whose patch as it was goes with that one, rejected
or accepted together), each decided from what is worked out of them as
the work lands (`Needs`), what refusing says, a stack that cannot be
checked marking none of its operations alone, what is worked out stored
as it is read back, for those operations alone (another change, or the
same from another stack, not theirs), and deciding from it checking
nothing, a change of messages alone worked out with no check, and two
rejections that together break a patch neither breaks alone passing
alone and found by closing's check, which says which patch and what it
needs;
those in `git.rs`, that stacked's looks at a checkout, its worktree or
yours, take no lock of its index (a file touched as it was, the index
left as it was, where git's own status writes it again);
those in `ops.rs`, that two code edits saved at once, on two threads, each
make their diff in a folder of their own and leave nothing in the
temporary directory, which diffs have the same effect (a hunk moved down
its file, under another heading or none, other blobs, a rename's
similarity, a header's line ending in a tab or not, an empty line in a
hunk for an empty context line) and which do not (another line added,
removed or kept, a file more or fewer, the files in another order, a mode
changed, a hunk split in two), which diffs make the same changes (more
context, as `-U5` and `-U10` write it, merging the hunks; a hunk split in
two or merged, starting elsewhere, under another heading, with other lines
kept around it; an empty line in a hunk for an empty context line; the
files in another order; more context reaching the end of a file that has
no newline there) and which do not (another line added or removed, white
space a line added ends with, a file more or fewer, a mode changed, a line
added with no newline at its end or with one, a run of changed lines split
in two by a line kept between them, or split across two files otherwise),
and that a draft with its patch's message,
author and effect is that patch, its diff as stored, whatever its date,
while one with another message, author or effect is a new version, as is
one that nothing can be checked on, and that a draft with the message and
the author git gave back of its patch's has them as stored, from its
commit as it was or made again, its code changed (the title git gave
back finding its patch) or nothing to check on, while one whose message
changed otherwise, if only in its white space, or with another author,
is a new version with the draft's message or author; and they show the
interdiff of two versions: a paragraph only wrapped otherwise no line
taken out or put in, there once as the new version wraps it, each line
where it is in the new mail, a word changed in it the one word marked,
each version's lines taken out and put in, a command wrapped otherwise a
change, its break marked, the diff after the `---` the same lines as the
mails diffed line by line, its hunks of its own when far from the
message, and with the title and the body the same, the mails diffed line
by line as they were; and compared to the letter, as the view of what
differs from Phabricator compares them, a paragraph only wrapped
otherwise each version's lines, nothing of them marked, but where a word
changed too, as ever; each line of an interdiff told apart as its
version's mail has it, the messages the same or not, its headers and
message, then under its `---` a file's header, a hunk's header or a line
of a hunk, and what changed in a line of a hunk taken out or put in as
its own version's diff pairs it, none in a line both have nor in a
file's header; and two diffs on other bases told by the files they read
before the patch, a line otherwise at the same place, the same hunk
elsewhere, a file one makes and the other finds, but not by more of a
file around a hunk, nor by another file; those in `submit.rs`, with a
stand-in for Phabricator that answers a fetch's reads and a submit's
writes, every send checked to read nothing, that a review reads nothing
where the database holds all it needs, the same review again, and fetches
first what it lacks, and then nothing: the revision and the one below,
nothing of either stored, in the calls Fetch the latest makes of them
alone, then the repository a new diff is made in, once; the revision's
raw latest diff not stored, or a state stored before stacked kept
whether it is a draft, its raw diff not read again; the revision below
alone; the repository alone, by its PHID; and no repository read where no
new diff is made; how long ago what it shows was fetched, as stored; a
fetch refused partway, or turned away (HTTP 429), said as such, nothing
stored, nothing sent; a repository not read, the revision's or the
callsign's, said; a revision Phabricator did not have, or with no diff,
when it was fetched, said; that a new revision is made in three writes,
the diff, its commit, and one edit of its title, summary, bug, diff,
parent added and Changes Planned, its link into the patch, nothing of it
fetched, its repository stored for the next; a draft taking a new diff and
a new parent in three, the parent in the edit; a revision in Changes
Planned taking a new diff in four, Changes Planned again in an edit of its
own, the next review, nothing fetched since, showing it as it was fetched
before, and none once it is fetched; that a latest diff with ten
lines of context, as moz-phab sends it, is the patch as it is on the base
it was made on, and nothing is sent; that once upstream has changed a line
within those ten and not within three, and the stack is rebased onto it,
the diff is known by its changes alone, never as the patch as it is, the
review saying why, and a new diff is sent, which a submit after a fetch
finds to be the patch as it is; that once the patch is edited too, its code is
compared from the version whose changes the diff makes; that a latest
diff of other changes is still refused, nothing written; that a revision
in review, accepted or needing revision is sent its title and summary
alone in one write, `WIP: ` first only where its title has it, the
reviewers its title names kept and none read, its status kept, no diff
made and its repository not read; its parent alone, in one write, when
only that differs; and nothing when only its diff differs, said
so, its latest diff left as it is whatever version it is, one known by
its changes alone with no new diff said to be sent; that a send reads
nothing, a revision retitled and accepted on Phabricator since it was
fetched sent the summary the review showed, and not the title, nothing
stored of it; that each failure says what was done and what was not: a
new revision's edit, its commit or its diff refused, a revision's one
edit refused with a new diff made or with none, nothing of it changed,
the parent with the rest, Changes Planned again refused after the rest,
and the patch changed since the review, nothing read; that a refused
parent refuses the one edit, and taken, it is set with the diff; that
a submit that changed its revision is remembered, by the link its patch
names, as of the send, until a fetch reads the revision: a new revision
made, its link the one that goes into the patch; a revision in Changes
Planned given its new diff, Changes Planned again refused after it, and
all four writes made, remembered again; a revision in review given its
title and summary in its one edit, the next review saying when it was
submitted since; and that nothing is remembered of a submit that changed
nothing of the revision: its diff refused, a diff of which no revision is
made, the edit that takes the diff refused, the one edit of a title and
summary refused, nothing to send, or the patch changed since the review;
and that a revision landed or abandoned is refused, fetched first, nothing
read but the fetch and nothing written, and refused again, nothing
read; those
in `history.rs`, what the history leaves out (comments, runs, a move to
where the stack is, a branch put straight back), how a stack diff follows
each patch by its change, a move told from a shift, and a new version
reworded when its message alone changed, its hunk moved under another
heading, one that only wraps its message otherwise changing no line of
it and one changing a word marking that word alone. Those in
`setup.rs` show the setup screen's one list, a read-only box and a change
box a row, what stacked always gives marked in its columns, Space ticking
the box of the cursor's column, ←→ and Tab moving between them, a click
ticking the box under it, and the columns' heading kept on the first line
once the list scrolls; a server's tools let all at once or one by one, a
tool of every ask's taken out of one kind; what each list starts ticked
as, the settings winning; which rules mean the same (a command's older
prefix form and the newer, a rule from several places), each one row
saying every place, a command shown and saved the newer way, agent.tools
rewritten so too, and which do not (a narrower rule and a wider, the command alone, a
`*` read as written); the rules a command added is checked against, and
its row, neither box ticked; what the checkout's claude settings allow and
deny, what its skills let run (their headers' every form, a rule several
allow, what is no rule left out), and what its instructions name (a
program on PATH, one not, a builtin and a tool stacked gives left out),
nothing read out of the checkout; the row of claude's tool for skills,
with what it lets an answer do; the rules the reviews allowed from now on,
a row each, unticked and ticked again, what saving takes out of their
list; the linter changes are given, named where the checkout is known,
with the rule that lets it run, or why none is given (none, lint off, no
narrow rule), on the screen and line by line alike; and the setup asked
line by line, a question a row for every kind.
Those in `refusals.rs` show a command line cut into the commands claude
matches apart, quotes and `$(…)` kept whole; a command's rule, its program
for a plain one, narrower for a broad one with the wider ones to pick,
what claude passes over first passed over, none for a shell's own; npx's
package and cargo's subcommand, past the options before them, kept as
written so that the rule lets the call, and the wide rule alone after an
option not known or with nothing after the options; the narrowest, none
for a broad program asked nothing narrower; the rule that lets a change
run the checkout's linter, the one a refused call of it is offered with,
which lets each call of it on files run, and not another package of
npx's nor another subcommand of cargo's; a call of it that a run of
before was refused, not given it, an item offered with that rule, npx or
cargo alone offered with the wide rule only, and one refused though given
it noted as allowed already; how a
rule lets a call (the older prefix form, a pattern, an MCP server's tools,
WebFetch's domain); the calls refused as items, one a rule, each with its
calls and the agent's word, a pipe's command given already left out; what
is noted (denied, refused by stacked, changing things for an answer, given
already), and that one call that needs it offered offers it; that an
answer is offered no rule of what bundles its short options (`sort`,
`file`, `git grep`) nor of what can write files or run programs whatever
the call (`sed -n`, `awk`, `uniq -c`, `env`, `xargs`, `python3 -m`, git at
large, what git does that changes things or does not only read), each
noted, nor such a rule to widen to, what stacked refuses it alone noted
as that (`git diff --output`), and what it refuses every ask but builds
and tests as that (`rg --hostname-bin`), while a change is offered each,
with its wider rules, as before, but for `rg --hostname-bin` and `sort
--co`, now refused it; that a command an answer is given, `rg`, is no
item where another command of its pipe needs one, and alone is noted as
allowed already; that a run of before, not given `git blame` nor `rg`, is
offered them, `git blame`'s rule never widened to the whole of git, and
`git grep` noted all the same; the three
states and the rule picked in the review's marks, and the turn the agent is
told; what the agent said of each tool, read from its answer; and what is
saved from now on, where, and why not outside a checkout. Those in
`search.rs`, which lines of a file a hunk shows
already; those in `rows.rs`, a web link's line under its comment cut to
the pane, its start kept, each row of what a comment says holding the
line it draws when that is prose (each piece of a line folded the same,
none in a code block nor on the comment's heading, a comment collapsed
the first line it says), each file whole keeping the rows of its hunks'
lines, a file too long and one past what a patch shows whole shown as
their hunks, a note saying so, and a row found again in the rows built
otherwise (whole or in hunks, folded to another width), a line in no hunk
as the closest line in one, ties above, and as comments go (a closed
comment's row on its line; one on a line the patch no longer has, or the
row saying which line that was, on the closest row over them; the rows
under them, there still); those in `whole.rs`, how a file's
hunks go in it whole (a hunk with no line on one side, git's `\ No newline
at end of file`), a file added, deleted, renamed or not text, and one that
does not fit its hunks, not shown whole; those in `progress.rs`, that a patch whose file has no newline
at its end leaves nothing to review once its lines removed and added and
its message are read (git's `\ No newline at end of file` is not a line
to read), and, with moz-phab, that a patch whose message ends with the
trailer moz-phab writes leaves nothing to review once all of it but the
trailer is read, the trailer left to review as the empty lines are when
the settings say white space needs review, which lines are that trailer
(its link to any host) and which are not (a line that says more, a link
that ends otherwise, no link, the line in the title's paragraph or in the
middle of the message), that a line of the diff saying the same is a line
to review, that without moz-phab the trailer is a line to review, and that
a paragraph a new version only wraps otherwise is read as it was, once a
mark holds its section, its lines to read again once a word of it
changes, and a mark that holds no section's key holding its lines alone;
those in `reading.rs`, Space and Delete on that trailer marking and
unmarking the message as on an empty line, the next line to read found
past the trailer, and the trailer drawn as a line that needs nothing, or
without moz-phab a line to read; those in `config.rs`, the editor told
the line it opens at as it takes it, and the program its command runs, as
StackED and `stacked web` name it, its first word's last path piece, by
which the editors with windows of their own are told apart, a file of settings over another, a
checkout's own read and written unless git tracks it, the patches
pane's `compact`, as versions before the files wrote it, read as them,
the width messages are wrapped at, one too narrow refused, the ask
dialog's `ask_model` and `ask_effort` unset unless set, set to one of the
dialog's own values read and written, another refused saying the setting
and the values it takes, the Bugzilla a
bug opens on taken when it is a web link to a site, else said and not
written, the keys listed by the context they are read in, and the sync's
keys of before said to be gone where a file names them, refused to
`stacked config` and taken out by its `--unset`, the view the interdiffs
start in, as interdiffs unless set, in columns or side by side, or the
change unified, side by side or compact, another refused and nothing
written, and their lines, every line unless set, or only what the patches
changed, other lines refused and nothing written, `stacked web`'s port,
`"random"` unless set, `"random"`, `"fixed-random"` or a port from 1 to
65535, one in quotes too, any other value, of any kind, said and
`"random"` kept, the rest of the file read, the port `"fixed-random"` kept
read beside it and one that is no port said, both written as any setting
is, the rest of the file as it was, the kept port passed over and kept
while `port` is another, values refused with the file as it was, and both
printed, built with `web`, and read back the same; its notices, sent
unless set, `true` and `false` read, any other value, of any kind, said
and the notices sent, the rest of the file read, `false` written beside
the port, the rest of the file as it was, another value refused with the
file as it was, and printed with its comment, built with `web`, and read
back the same; its bell, rung unless set, `true` and `false` read with
the notifications false, any other value, of any kind, said and the bell
rung, the rest of the file read, `false` written beside the
notifications, the rest of the file as it was, another value refused with
the file as it was, and printed with its comment after the notifications',
built with `web`, and read back the same; where its editor runs, in the
page unless set, `"page"` and `"terminal"` read, the editor of the top
level as it is, any other value, of any kind, said and the page kept, the
rest of the file read, `"terminal"` written beside the bell, the rest of
the file as it was, another value refused with the file as it was, and
printed with its comment after the bell's, built with `web`, and read back
the same; and `[web]` the
settings' file's alone: a checkout's own that sets it, wrong too, its
notifications, its bell and its editor too, as the settings' file has it, each key said to be
passed over, the rest of it read, neither marked as the checkout's nor
the one in use there, refused to `--local` for writing and for reading,
and taken out there; those in `tui/notices.rs`, a notice's body made a
sentence, its first letter a capital, a full stop after it unless it
ends with one, `?` or `…`, and in the place of the colon a summary ends
with when nothing is said after it; those in `change.rs`,
the change from one version to another on one base, each file from the
one to the other, nothing dim, the same
files no change; across a change of base, the line it made in place of
another dim on both sides, each patch's line put in its own; a line the
change of base touched where a patch takes it out, or the other keeps it,
not dim, both taking it out no change, both keeping it dim, a line each
puts in beside it its own; a line moved into a patch below, or out of
one, no change, and one moved there then changed, the old version's line
its own and the new base's dim; a file the change of base renames, the
old path's lines it made dim and the old version's own not, the new
path's dim but the new version's own, a file one version alone touches as
the other leaves it on its base, and one the old version renames and the
new one does not, its own; and read from a repository, each version on
its own base, a file only the change of base touches left out, one base
nothing dim, and a version not applying on its base said so, by name;
those in
`wrap.rs`, a message's
long lines wrapped: a paragraph of prose filled to the width (lines broken
by hand, gaps of white space), one within it left as it is, its short
lines unjoined, a list's items each a paragraph under its marker (three
spaces at most), a word longer than the width alone on its line, code (a
fenced block with an empty line, an indented command, a `$ ` line), the
title, the trailers and a quoted line left as they are, a word that would
start a line as something else kept on the line before and such a word
alone on its line kept so, a wide character two columns, what the menus
say, and messages made up of lines of every kind wrapped to the same words
as `messages.rs` compares them, with nothing more to wrap after; those in `keys.rs`,
the keys checked context by context (the defaults with no clash, one key
in two contexts no clash, two things of one context on one key said),
what the search of the middle keeps for itself refused to its own,
the sync's keys of before, keep_stack and keep_workspace, said to be gone
and passed over, and o, the key that
opens a link, read on the screens, made another key, and clashing there
alone; those in `links.rs`, the links of a line of a message: its web
links as written, less what ends a sentence or a bracket around them,
none with no site, with another scheme or with a word running into it;
a markdown link named by its text, its markup left out (a text that is
a link, or holds one, said as the link it goes to), its link up to the
`)` that closes it, its own brackets kept, a title after it not of it,
two with nothing between them two, none to no web link; one between
angle brackets whole, none with a space in it;
with moz-phab, the bugs it names, `bug` a word of its own (none in `debug
7`, which a title submitted still reads as moz-phab does), the rest as
moz-phab reads them, none in a web link nor in a markdown link's text,
on the settings' Bugzilla, and the revision its trailer names, at
its link or bare on the settings' Phabricator, as the settings name the
trailer; without moz-phab, its web links alone; those in
`messages.rs`, two versions of a commit message compared where their lines
wrap aside: a paragraph wrapped otherwise the same, a word changed marked
in each, two words in a row marked as one stretch, a paragraph split or
joined marking the break alone, white space that ends a line not read in
prose nor in code, the trailer naming the revision a line as any other
(another revision, its link alone; one alone naming it, its line and the
break before it), a command wrapped otherwise not the same while the
prose over it is, a fenced block one section of code, its empty lines and
all, a line of code (indented, after a tab, a prompt) making its whole
paragraph code, the white space within a line of code counting, its
indent too, and a section of code in one version and prose in the other
not the same; and the two as a diff of their lines: a paragraph wrapped
otherwise a run the same in both, each with its own lines; one with a
word changed a run that differs, the word alone marked; a paragraph split,
one put in or taken out, a command wrapped otherwise, runs that differ;
empty lines before every section or after the same as the other's there;
and what a section says the same however it wraps, code line by line;
those in `head.rs`, which commits are marked, `!` for one that names a
revision its patch does not, whatever else differs, and `≠` for a
message other than the patch's, empty lines in a row one as `git am`
leaves them, what `stacked head` says of each, when Move HEAD replaces
commits (as stacked left them, or on the stack with no mark) and why it
does not, and that it keeps every commit up to the last one marked `!`,
and those after it that are their patches, HEAD going nowhere below it;
and, without moz-phab, the trailer compared as any line;
those in `refs.rs`, which references a rewind or an edit detaches, which
stay (named by another turn, the agent's or by `@r*`, by the reply waiting
for review, or never named), the agent's turn written again keeping its
own, a turn deleted the first way counting nowhere, and no number given
twice, a reference detached by hand or gone with its turn; and a web link
read from its `refs` line and never as a path, kept in the database while
the comments saved before read as they did, given by the agent as
`{"url"}` only when it is of the web, named and detached as any
reference, and opened by the program that opens links, or said why not.
The tests in
`diff.rs` show each line's numbers on each side, git's
`\ No newline at end of file` counting on neither (under the old side's
last line, under the new side's, under both, or under a line both sides
have), and show that the keys of those lines, which comments and read marks
are on, are as they were when git's line counted on both sides. Those in
`blocks.rs` show that a line added after git's `\ No newline at end of file`
is in the block its own number is in, and those in `phab.rs` show that a
reviewer's comment on such a line is put on that line, found by its number
as by its text. Those in
`home.rs` show where stacked keeps what is yours (the home's folder, the
one `$STACKED_HOME` names, a file named over it, nowhere with neither);
the files of before moved in once by a run of yours and said, never over a
file there, a link moved as a link, and nothing moved that an override
names the place of; what stacked runs told the same places; the token of
`stacked web`'s address kept beside the state, wherever the state is, and
nowhere where the state is nowhere; and nowhere in a test's own process.
Those in `phab.rs` show a comment read as it was
last edited, its versions before kept with it, by when it was first
written, one removed none; from a Phabricator that gives no suggested
edits, an inline comment that is a suggested edit alone, with no words and
not removed, read saying so, that this Phabricator gives none, with its
lines, one or more, and its link, at the diff it was made on and its
anchor, a reply in its thread, its version before so too once it is edited
to have words, none once removed, one with words read with its words, each
on the new side, and a comment on the whole revision with no words none.
They show suggested edits as Mozilla's Phabricator gives them, read with
the lines they replace from the diff each was made on: D337641's suggested
edit alone, read as a diff of the lines it replaces, then of its own,
fenced, each as it is, a version of its own whose id says so, the version
it is read from before it, saying what a stacked of before filed of it;
one with words after them, trimmed, its own lines not, white space
starting and ending one kept, the version before it its words alone, and
edited since, the version before that known by both its ids; one of no
lines deleting the lines it is on, and its diff not read, said so; a
newline ending its last line rather than adding one, as on D337641, one
line in the place of two and six in the place of seven, and a line left
empty before that newline one of its lines; one on the old side of its
diff reading that side's lines and its line's text there, said to be of
the file before the patch, and sitting on the patch's line it removes, by
its text there or by its number, one on the new side on the line the patch
adds, and a line of context on both; one whose lines are not all in the
diff read, one in no hunk of it or the diff not in Phabricator's answer,
its own lines alone, said so, nothing guessed; a read of a diff's lines
that fails failing the read, saying why, Phabricator turning this machine
away there said as such, as at any of a pull's calls; and a removed comment none, whatever it suggested,
one said to suggest nothing read with its words alone, by its own id, with
none, none, and edited, its version before known by both its ids, saying
what a stacked of before said. They show a revision's status read from its
value alone, whatever Phabricator says of drafts, with its letter and its
name; the statuses of revisions read in one search, those it does not
find not known, each with its title, summary and PHID, whether it is a
draft and its repository, and a search
refused said; an editor's read by the stand-in for its database alone, one
search for each Phabricator the links name, never from a process that
keeps nothing without one; and what a fetch reads of the revisions, in a
few calls: the parents of all of them, and their latest
diffs, the newest first, each in one call, page after page, the diffs
until each revision has its newest; the parents that are none of them
named in one search, only of the revisions a submit sets one, a parent it
does not find left unnamed, none searched with none to name; then each
revision in the order given, its diff raw unless it is the one stored,
its comments from every page of its transactions, their authors and lines
not read; a revision with no diff passed over, and nothing more read after
a call that fails; a revision not modified since it was stored, its latest
diff the same, not read again, but for its parents, and one modified
since, given a new diff, never read, or whose modification is not known,
read as before; by the stand-in for the editor's database, the calls of
each Phabricator on their own, and a read turned away said as such,
whatever its calls make of it; a fetch storing what it reads, each
revision's state, comments and raw latest diff, one Phabricator does not
have as such, and the fetch, never from a process that keeps nothing,
each state saying whether its revision is a draft and its repository, as
the search said; one finding all as stored writing nothing of it but the
fetch, one finding a revision modified since reading and writing its
comments alone, one of a revision alone, as a submit fetches what it
lacks, reading no more than its search, its parents and its latest diff,
as it was not modified since, its parent named as stored, and the same
through a submit's own calls, the database's stand-in asked nothing; and
one turned away writing nothing. They show what a pull reads: the revisions asked, down their
open parents the stack does not hold, and up through the open ones
stacked on the top, each named with its parents, the older diffs of one
whose latest is no version stacked knows, to the first that is one;
more than one open parent, or more than one open revision stacked on one
where the pull goes up, read no further, saying which; and each revision
told as it starts being read, up to a call that fails. They show curl given the token and the call's parameters on its
standard input, none of them in its arguments; a call turned away (HTTP
429) an error of its own, which says so plainly, whatever Phabricator asks
to wait, after an answer that says to go on too; another status
said with the first line of its page, cut short, or that the page is
empty, as a page that is not JSON is; Conduit's own error and curl's said
as before; and curl itself, to a server on the loopback, sending the
token in the form of its request, and saying a 429, a gateway's error page
and Conduit's answer. Those in `review.rs` show the comments not
pulled yet counted from the comments open, the threads by their ids and
the pulls waiting: those filed or waiting not counted, one edited since
it was filed counted as edited, one filed without its suggested edit and
read again with it counted apart, and one edited since and read with its
suggested edit as edited, a thread closed not counted, nor a new
reply or edit there, but for one moved to the stack's comments, another
stack's pull not this one's, and one a review of before rejected counted,
as a pull brings it again; where a pull is said, one patch's revision
pulled alone too, what it reads on its line of the banner, and each of
its phases, as what changes the stack says it and as its line; and what
a pull brings said counted, the suggested edits it reads of comments
filed apart from the edits, its conflicts marked last. Those in
`tui/revisions.rs` show how long ago what the Phabricator line says was
fetched, said as the line says it, just now, then in minutes, in hours,
yesterday, then in days, a time to come just now, and how long ago alone,
as the line says when a submit was; a title in work
as moz-phab reads it; the title and summary compared to the letter as a
submit sends them, in work or not as the revision's title is, the white
space at their end aside, their lines wrapped otherwise or other white
space in them differing; what a submit sent, given back as it was, in
sync, its reviewers, its trailer, white space ending a line, a fenced
block or carriage returns and all; a title and summary only wrapped
otherwise, or only spaced otherwise, showing the lines of both versions,
nothing marked, said so, and one with a word changed too showing the
paragraph wrapped otherwise once; a title and summary only wrapped or
spaced otherwise told from one with a word changed; a diff bare of what
git derives in it, as Phabricator's raw diffs are, the lines of its hunks
as they are; the tour pulling nothing from Phabricator, a
patch's revision alone neither, nor fetching the latest; the comments to
pull, the suggested edits to pull apart from the comments edited, and
what differs said in plain words; the patches pane's mark `✓` exactly where the line
says in sync and `✗` wherever it says what differs, the parent set,
already its parent or none set, and none while that is not known or for a
revision not found; and the line folded between words, its pieces under
its first. Those in `tui.rs` show, among others, a line of a menu's note
drawn in parts folded at the spaces its text folds at, each character in
its part's colour, but never between a mark and the count after it. Those
in `tui/interdiffs.rs` show an interdiff side by side, paired as the code
view pairs a diff's lines: a heading of the two versions, the hunk's
header across, a line both have on both sides of its row, the run the old
version has beside the run the new one has after it, row by row, a line
one alone has with `~` dim on the other side, and the lines of the
interdiff each row shows; each cell as its own version's diff draws it,
dim where both have the line, what its own diff changed in it in the
colours for words, the headings in a file's name's colour and the line
between the halves in the borders'; a line of the message compared across
the halves, its words as words; a line too wide folded in its half, code
where the half ends, `↪` before what follows, a bullet's pieces under what
follows the bullet; an interdiff in a column per version, a header both
have `==`, a title each has alone `=·` and `·=`, two hunk's headers that
differ in their ranges one line with both, lines both leave or remove, a
line each version adds alone `+·` and `·+` in the colours for words, a
line the one keeps and the other removes one line, ` -`, and one only the
old version's diff shows a line the new one leaves as it is on one base
and `·` on two, a line one version adds and the other's base has one
line on two bases, `+ ` and ` +`, but two on one, a line both do alike
dim, the columns each in its own colour; what the views as an interdiff and in a column per version say
once at the top of the code of one interdiff, naming its versions, and
over the first interdiff of a stack diff alone, side by side a heading
over each; and the line a line drawn shows found again in another view,
the old version's of two, or the one a switch kept, a line of the change
by its own line, else by the line of the interdiff nearest it, kept
across a switch where no line drawn shows it, and a line no line drawn
shows found on the nearest, of two as near the one after. They show, on
interdiffs `ops` makes of two patches, what only what the patches
changed draws: across a rebase onto a new commit, of 29 lines the file's
name, the hunk's header as the new version has it, as one both have, and
the line each version makes its own, under `⋯ 3`, `⋯ 12`, `⋯ 5` and over
`⋯ 3 lines are left out here.`, in a column per version as each column
says the lines, side by side the index lines one row, `⋯ 11` and `⋯ 4`,
the lines saying so dim and showing no line, the header showing both it
stands for; a hunk's header kept only over its own lines, though the
interdiff leaves out the lines between; across a change of the patches
below, the context line they changed left out, the two lines of the run
that differ together under one line saying how many; on one base, two
hunk headers whose numbers differ one, the new version's, not the line
with both ranges, which every line draws; the messages compared and a
file one version alone makes kept, each line of it but its index line,
counted over it, git's `\` line too; an interdiff with nothing left saying
so in each view, and in a stack diff under its patch, what is left out
said once over the first; and what each view says of it. Those in
`tui/change.rs` show, with only what the patches changed, the lines the
change of base made left out of the change, unified, side by side and
compact, a hunk and a file with nothing else too, its blank line once,
each place saying how many, dim, a stop that shows no line; a change all
of whose lines the change of base made saying so; on one base nothing
left out; and what the views say on other bases, what is left out rather
than what is dim. Those in `tui/views_dialog.rs` show each value of the
dialog of the views of the interdiffs its short name, side by side among
the views of the two diffs and of the code alike, the line under each
short enough for one row of the dialog, and its words for `?` longer than
that line.

The Configure screen's tests, in `tests/tui.rs`, read and write scratch
files alone, a global file in the test's own `home/` (`App::config_file`)
and its checkout's `.stacked/local.toml`, never the settings of whoever runs
them: the button `< stacked >` clicked opens its menu next to it, titled
stacked, Configure… in it, which opens the screen; each row shows the
global file's value, the local file's and what is in effect, the default
where neither sets it; a setting of the global file alone and one of the
local file alone say so in the other column, and their menu there grays
all and says why; the token shows only that it is set, and typed is drawn
as dots; the bell's level typed, 101 and loud refused in `stacked config`'s
words, the file as it was, and 50 written, the screen and stacked web's
settings following; a choice written in the local file, then in the global
file, said to be overridden by the local file; Delete and the menu taking
it out. The test buttons send nothing and say why in the terminal, in
StackED and in a stacked web with no page; with a page, the test notice and
the bell at its level, at the level typed when clicked while it is typed,
and none at 0. `Configure…` ends every menu but the screen's own, and is
grayed in the button's menu there. `tests/web.rs` clicks the button and its
test buttons in a `stacked web` run for real with `web.bell = 40`, which
sends the test notice and `b 40 test`; in node, the page scales each
partial's peak by the square of the level (1, a quarter at 50, a hundredth
at 10), strikes nothing at 0 nor while looked at, but a test's, and shows a
test's banner while looked at, counted nowhere, asking the browser first
when it never asked, nothing once refused. `tests/app.rs` opens the screen
from the button in StackED's window, whose notice test is dim and says that
StackED shows no notices.
