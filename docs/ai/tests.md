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
read by stand-ins, and what a submit sends is answered by one, which keeps
every call made to it, so that a test can see that a refusal wrote
nothing, and which, as Phabricator does, makes no revision of a diff whose
commit has no tree; what a fetch reads of a stack's revisions is answered
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
the binary starts (a try push's keeps its copy of the task graph there),
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
with none; and the edits since a state
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
`--effort` says otherwise; the effort an ask was asked at said by
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
edited by hand while the agent changes it, failures kept to look at, comments staying on their line
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
moved there, as its history has it last when written since), a stack
imported from Phabricator's revisions (by
number or link, named or not), Phabricator's commands there only when
built with it, and
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
the dialog open on that line, and Ask Now and Ask Later grayed until both
lines are set; ↑↓ going round to the buttons, ← → there past those
grayed; Ask Now saving what changed first and asking at the permission
and the effort set, the stand-in given that `--effort` and the tools of
that way; Ask Later putting it in the batch; Cancel and Esc asking
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
buttons Send Now and Cancel, Send Now grayed until both lines are set and
pressed grayed saying why, the line under the permission saying what each
does with the work, `?` what Send Now does as the permission set does it,
Cancel and a click out of it sending nothing, the review as it was, a
second dialog unset; Send Now grayed, saying why, while the network does
not reach the agent, nothing sent), the effort an ask goes at
(the stand-in started at the effort the dialog set, the settings' own
taken out; a batched comment, sent now or with the batch, at the effort
it was batched at, one waiting its turn at its own, one queued by an
older stacked at the agent's default; work sent back at the effort its
dialog sets, the work asked at low, its item no dropdown of its own; the
effort the agent was asked at shown wherever its work is
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
and said, and the layout a setting), what changed in a line almost the same as the
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
as they were before a rebase, no code under it, then sending one,
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
from Phabricator, its revisions read by a stand-in; or by the agent), a
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
comment, and a click in the box putting the cursor there), the comments pane's previews,
moving a patch, the long lines of messages wrapped (a patch's menu
offering it only when its message has some at the settings' width, the
note saying how many lines of which patches; the edit landing for review,
the stack as it was until it is accepted, then the new messages, as the
history says; the stack's header wrapping every message that has some, as
one edit; offered but not to be chosen, the menu saying why, on a
read-only stack and while an edit by hand waits for review), searching (what is in sight first, the patches then the
files they touch, the history), the agents' worktrees made ahead as the
stack's checkout says, the banner of the agent's work (Esc to it and
back, open a line for each, Enter on each; the reviews pulled waiting in
it as the agent's work does, their review rejecting nothing (no Delete, no
item, nothing to reject as it closes), a reply to a comment you closed
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
marked yet, patch 5's marked rejected), the
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
under way; its review saying what it takes, stacked's comment saying what
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
patch), the
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
asked again; a submit fetching the revision it sent to again, alone, once
it is done, a new revision's status shown at once; Changes Planned shown
as such though Phabricator says it is a draft; a fetch that fails keeping
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
review showing them so too, and in sync once it is submitted; a patch
submitted and unchanged since in sync, Phabricator
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
versions and what it shows said first: the code as an interdiff of the
two diffs, what git derives in them left out, and the title and summary
compared where their lines wrap aside, what only each has marked in the
colours for words; Esc going back to the line; in sync, Space saying so,
the parent named, and opening nothing; not fetched yet, Space saying that; reading, Space
marking nothing read and Delete taking no mark back; Enter opening the
menu, titled for the revision, every item of the patch about Phabricator
first, its submit, what differs, the pull of its reviews and the fetch,
and the bar and `?` saying what Space does and what the menu has; the
patch's own `Phabricator` menu offering those, the patch's menu none of
them; what differs grayed, the menu saying why, while it is not fetched yet, and
for a revision Phabricator does not have; in sync, the screen saying that
nothing differs, the parent with the rest, Esc going back to the patch; neither item while reviews
come from nowhere; a latest diff on another base, each file as each
leaves it, said, or the interdiff all the same, said too, where the
files cannot be read so; the reviews of one patch's revision pulled
alone, from the line's menu and from the patch's `Phabricator` menu, that
revision read and no other, the banner naming it, one pull at a time, what
it brings held for review as a pull's, the comments it read stored and
nothing else read, and the stack's pull still reading every revision;
every revision of the stack fetched, the parents and the latest diffs of
all of them in one call each, by their PHIDs, then each, bottom first,
each patch's line whole as soon as it is shown, nothing read for it, nor
as patches are shown again, nor after a pull, which stores what it read,
all of it read again once it is fetched again, a raw diff not read again
while it is stored; a revision whose search says when it was last
modified not read again while that stays the same, only its parents and
its latest diff read, with the others', and no record of it written again,
only the fetch's; one modified since read again, alone, its new comment
counted; one submitted, fetched again alone once the submit is done, its
comments read again though its search says it was not modified since;
one pulled alone, read by the next fetch alone, as the pull stored its
comments with no time of when it was modified; a fetch that fails partway
storing nothing and saying why, the line not fetched yet, the next one
storing it all, one failing later keeping what is stored, a revision
never fetched not fetched yet; a fetch turned away (HTTP 429) said
plainly, tried once, its first call turned away and nothing asked after
it, nothing written, the statuses and the lines as they were, nothing
waited out nor read by itself after it, a pull asked made all the same,
nothing read after it, and a fetch after it reading all of it at once;
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
the same items directly, the stack's menu the fetch after the pull; what a
submit reads stored, its revision's state and raw diff, though nothing is
sent, and what it reads again before it would send, and the comments a
pull reads, with no time of when the revision was modified, none written
again when they are the same; the
parent: a reordered stack's revisions differing by their parents alone,
nothing read for it, Space and the menu's item showing the parent first,
Phabricator's then the one a submit sets, and nothing under it, `?`
saying so, a submit setting it and nothing else, then in sync, its
parents fetched again with the rest, Space naming the parent; none
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
not sent and why, the status staying as it is, no code under it, Enter
sending the summary alone, no diff, no Changes Planned and no reviewers,
the bottom line saying what came of it, and once it is fetched again
after the submit, the diff alone differing, the update grayed, the menu
saying why), and the comments pane's states (expanded,
compact, hidden) and the compact view of the code as they were. With the `web` feature, `tests/web.rs`
serves the editor and asks it over HTTP as a browser would: the token
needed, the page, the wasm, the rows as they change, a key and a click,
the wheel's ↑ on a patch, listing files, going to the last file of the
patch above, and the editor left, the page told, the worktree made ahead
removed (Quit in the page; `stacked web` itself sent SIGINT, SIGTERM or
SIGHUP, the first while `$EDITOR` runs left to `$EDITOR`, the second
waiting for it), and two
`stacked web` at once, each putting its database, named from where it
runs, on the list StackED's Open Recent shows, the ask dialog (opened with
nothing set from `Ask agent about all…`, clicked, its permission set by
clicks on its line and on an option of the list that opens under it, then
stepped by → and back by ←, its effort picked in the list Enter opens, ↑
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
the runs of its row, `Move HEAD…` on the stack's menu
not grayed while it would not move HEAD, and the view of why opened from
it, then left, a link of a line of a message opened from the
menu of a right click on it and with `o` (the status line saying it is
opening it while the stand-in is held, then that it opened it), and one
of a line of a comment from its menu, by a stand-in, and the comment
closed last on a line, longer than its box in a small page, scrolled
down to its last line with
↓, the wheel's ↓ and pgdn, and, with
moz-phab, the Phabricator line saying first that the revision is not
fetched yet, nothing read as the page opens; then, fetched from the
stack's menu, its item clicked, by a stand-in for its database, the status
of a patch's revision after its number, a letter, then its mark (`✗`, its
diff otherwise) on the screen its Phabricator line says so, the next
patch's title after the column all the same, expanded and listing files, a
title folded there, and the Phabricator line over its message, Space on it
showing what differs on a screen of its own, Esc going back to the line,
and the reviews of its revision pulled alone from its menu, an item
clicked, by a stand-in too; and
`web.rs` tests the rows and the lines both ways, with the client's own tests
(`web/client.rs`, compiled to the machine as well as to wasm), which also
test the mouse followed only while the page has the focus: the cell where
the mouse was last seen while the page did not have the focus said at once
when the page has it again (unless the mouse left the page), so that the
click that gives the page the focus back acts on that cell, whether the
browser sends the click before the focus or after. With
the `app` feature, `tests/app.rs` runs StackED's editor as its window runs
it, with no window: nothing drawn before the window's size, a key, a
click, ↑ on a patch, listing files, going to the last file of the patch
above, as the wheel's ↑ does, the mouse over a patch then gone, a window
too small, the wait while `$EDITOR` runs (an editor that edits nothing,
set in the test's settings), quit, the command there, a link of a line of
a message opened from the
menu of a right click on it and with `o` (the status line saying it is
opening it while the stand-in is held, then that it opened it), and one
of a line of a comment from its menu, by a stand-in, the comment closed
last on a line, longer
than its box in a small window, scrolled down to its last line with ↓ and
pgdn, as the wheel's ↓ do, and a database
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
paints; `Move HEAD…` on the stack's menu drawn as any item while
it would not move HEAD, and the view of why opened from it, then left; the
ask dialog opened with nothing set from a click on `Ask agent about all…`,
Ask Now and Ask Later drawn dim, Ask Now as claude is not found, which it
says, a click on its permission's line opening the list of its options
and a click on one of them picking it, Enter on its effort's line opening
that line's list and Enter again picking low, and a click on Ask Later
putting the comment in the batch; the agent's turns headed
with the effort it was asked at, and its landed work named with it, in
the banner and its review, a turn of before as ever; with
moz-phab, the Phabricator line saying first that the revision is not
fetched yet, nothing read as the window opens; then, fetched from the
stack's menu, its item clicked, by a stand-in for its database, the status
of a patch's revision after its number, a letter, then its mark (`✗`, its
diff otherwise) on the screen its Phabricator line says so, the next
patch's title after the column all the same, expanded and listing files, a
title folded there, and the Phabricator line over its message, Space on it
showing what differs on a screen of its own, Esc going back to the line,
and the reviews of its revision pulled alone from its menu, an item
clicked, by a stand-in too; the File menu, its
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
it makes of keys, clicks, the wheel (while a comment is written too) and
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
the editors with windows of their own, and the script a terminal is given
to run it (for its owner alone, removing itself as it runs the edit,
removed if the terminal cannot be opened); `refs.rs`, that what is not a
web link is never handed to the system's way of opening links;
`icon.rs`, what the icon shows where, at its largest (the sheets' edges)
and at its smallest (a plain white S), and that it is grey all over. With
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
its own, and with the batch that `stacked send NAME --batch` sends. The
push runs a stand-in for mach (`$STACKED_MACH`, which a stacked that keeps
nothing, as the tests' own, needs, never running a mach of a worktree's),
the repository's remote of Firefox's a name and a ref, never fetched: the
push checked with `--no-push`, then made, mach's stdin closed and its
output not held back, the tasks in one query, the commits it would push
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
a round asked again is told; mach's arguments, the tasks in one query, and
how the stream says them; what `--no-push` says it would push, read and
compared (the tasks, the runs, the priority); a question mach stopped on,
its login, its link and Lando's job read from what it writes; that a
process keeping nothing runs only a stand-in for mach, and reaches
neither Lando nor Treeherder; the remotes mach takes as Firefox's (on
GitHub, and hg.mozilla.org's but try in a git-cinnabar clone), a base none
of them has, and the commits mach would push; and a push's tasks counted as
Treeherder shows them (each by its last run, each run of a rebuild, those
not seen), its summary, and where Lando answers, as `.lando.ini` says;
`ask.rs`, that a try push's ask is given what an answer is and its own
list, not the linter a change is given, and is asked for its tasks and
their runs; `setup.rs`, the column of
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
history; a fix that would keep the patch above from applying stopping the
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
nothing. The
stream's thinking is tested in `ask.rs`, with the prompt asking the agent
to point at the stack before the web and its answer's schema taking a web
link, the ask's effort given to claude after the settings' arguments, in
place of theirs, a child process that has exited not taken for alive
though `kill` still finds it, its exit left for whoever started it, the
wrapper of an ask the process started waited for once it has exited, and
an ask whose agent you stopped landing as stopped, unless the agent was
done as you stopped it, and the thread the agent is told, a reviewer's edit
and the turn it redacts in plain words; what is said of landed work left
to review is tested in `held.rs`, with a reviewer's edit as their latest
turn, the turn it edits redacted, again on a later edit, nothing redacted
when that turn is gone; and a reviewer's reply to a comment you closed
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
short with `…`, and a name alone as ever; and the effort on the fence of
the agent's turn, after the way, read, headed and named with it, kept as
the thread is written again, a reply added, a turn edited or one holding
a code block, a turn of before read and written as it was, a comment the
agent opened at an effort, and a reviewer named `effort` a name as ever;
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
or accepted together), what refusing says, and a stack that cannot be
checked marking none of its operations alone;
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
changed too, as ever; those in `submit.rs`, that a latest diff with ten
lines of context, as moz-phab sends it, is the patch as it is on the base
it was made on, and nothing is sent; that once upstream has changed a line
within those ten and not within three, and the stack is rebased onto it,
the diff is known by its changes alone, never as the patch as it is, the
review saying why, and a new diff is sent, which a submit after it finds
to be the patch as it is; that once the patch is edited too, its code is
compared from the version whose changes the diff makes; that a latest
diff of other changes is still refused, nothing written; that a revision
in review, accepted or needing revision is sent its title and summary
alone in one edit, `WIP: ` first only where its title has it, the
reviewers its title names kept and none read, its status kept, no diff
made and its repository not read; its parent alone, in an edit of its
own, when only that differs; and nothing when only its diff differs, said
so, its latest diff left as it is whatever version it is, one known by
its changes alone with no new diff said to be sent, nor once it changed
on Phabricator since it was read, retitled or accepted; and that a
revision landed or abandoned is refused, nothing read but the revision and
nothing written; those
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
without moz-phab a line to read; those in `config.rs`, a file of settings over another, a
checkout's own read and written unless git tracks it, the patches
pane's `compact`, as versions before the files wrote it, read as them,
the width messages are wrapped at, one too narrow refused, the Bugzilla a
bug opens on taken when it is a web link to a site, else said and not
written, the keys listed by the context they are read in, and the sync's
keys of before said to be gone where a file names them, refused to
`stacked config` and taken out by its `--unset`; those in
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
names the place of; what stacked runs told the same places; and nowhere in
a test's own process. Those in `phab.rs` show a revision's status read from
its value alone, whatever Phabricator says of drafts, with its letter and
its name; the statuses of revisions read in one search, those it does not
find not known, each with its title, summary and PHID, and a search
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
have as such, and the fetch, never from a process that keeps nothing; one
finding all as stored writing nothing of it but the fetch, one finding a
revision modified since reading and writing its comments alone, one of a
revision alone, as after a submit, reading its comments again whatever
its search says, its parent named as stored; and one turned away writing
nothing. They show curl given the token and the call's parameters on its
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
it was filed counted as edited, a thread closed not counted, nor a new
reply or edit there, but for one moved to the stack's comments, another
stack's pull not this one's, and one a review of before rejected counted,
as a pull brings it again; and where a pull is said, one patch's revision
pulled alone too. Those in
`tui/revisions.rs` show how long ago what the Phabricator line says was
fetched, said as the line says it, just now, then in minutes, in hours,
yesterday, then in days, a time to come just now; a title in work
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
as they are; two diffs on other bases told by the files they read before
the patch, a line otherwise at the same place, the same hunk elsewhere, a
file one makes and the other finds, but not by more of a file around a
hunk, nor by another file; the tour pulling nothing from Phabricator, a
patch's revision alone neither, nor fetching the latest; the comments to
pull and what differs
said in plain words; the patches pane's mark `✓` exactly where the line
says in sync and `✗` wherever it says what differs, the parent set,
already its parent or none set, and none while that is not known or for a
revision not found; and the line folded between words, its pieces under
its first. Those in `tui.rs` show, among others, a line of a menu's note
drawn in parts folded at the spaces its text folds at, each character in
its part's colour, but never between a mark and the count after it.
