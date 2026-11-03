# Linting

`Lint the stack (./mach lint --fix)`, on the stack's header (Space there),
lints every patch of the stack with the linter of the stack's checkout,
away from the screen; what the linter fixes lands for your review, as an
edit by hand does. Nothing else lints: no patch's menu, no command, and
nothing is linted unless you choose it.

The linter is the checkout's own setting, `[lint] command` in its
`.stacked/local.toml`, which `stacked setup lint COMMAND…`, run in the
checkout, writes: a program and its arguments (`stacked setup lint eslint
--fix`), the files to lint after them. A lint job runs it itself, not the
agent, and with no shell: the command is split into words as a shell
splits them, quotes and all, and nothing else of a shell's is read, no
pipe and no variable. A checkout whose top has `./mach`, as Firefox's has,
and whose own settings set no linter, lints with `./mach lint --fix`,
nothing written. `stacked setup lint --off` writes `command = ""`: lint is
off there, `./mach` or not, until `stacked setup lint COMMAND…` sets a
linter again, or `stacked config --local --unset lint.command` takes the
setting out. With no command, `stacked setup lint` prints the linter in
use and says where it comes from, the checkout's own settings or the
default of `./mach`; with none, it says how to set one, and fails. The
setting is never written in `~/.stacked/config.toml`, nor read from it,
which says so if it sets one; `stacked config lint.command VALUE` refuses,
with `--local` too, and says that `stacked setup lint` writes it, while
`stacked config lint.command` reads it as any setting.

An ask that changes the stack may run the linter too, on the files it
changes, to check them: it is given the linter as the checkout's settings
have it when the ask starts, and no other kind of ask is (an answer and a
try push's selection only read; a build and test runs any command
already). It is given the rule the review of a refused call of the linter
would offer first (The tools the agent could not use, above), so that no
call of it is refused: `Bash(./mach lint *)` for `./mach lint --fix`, with
or without `--fix`, on any files; for a linter the checkout sets, its
program, with what it is asked to do for a broad program (`Bash(eslint *)`
for `eslint --fix`, `Bash(python3 -m black *)` for `python3 -m black`,
`Bash(npx eslint *)` for `npx eslint --fix`, `Bash(cargo clippy *)` for
`cargo clippy --fix`), so that the ask may not run another package with
npx, nor `cargo run` or `cargo install`. A linter that no narrow rule lets
run is not given: a broad program asked nothing narrower, whose rule would
let it run anything (`sh -c …`, `./mach --verbose lint`, `npx` or `cargo`
alone), or one whose program is in quotes. The ask's prompt names the
command: ``This checkout's linter is `./mach lint --fix`: you may run it
on the files you change, to check them.`` With no linter, or lint off,
nothing is given and nothing is said. `stacked setup agent` names the
linter, among what stacked always gives, in the change column (Settings,
below).

The item names the command in use; with none, it reads `Lint the stack`,
grayed, and the menu says why: `No linter is set for this checkout:
`stacked setup lint COMMAND` sets one.` It is grayed too, the menu saying
why, where lint is turned off, while the stack may not be edited by hand
(the agent changing it, a pull from Phabricator reading Phabricator,
waiting its turn or being made, a change of it waiting for your review),
on a read-only stack, and
while a lint job of the stack waits or runs. So it is
in the browser and in StackED, as in the terminal.

The lint job runs in a process of its own (`stacked lint-job RUN`), in a
session of its own, at a low priority (`nice`), the linter's stdin closed,
so that nothing it runs has a terminal to ask anything on; and in a
worktree of the stack's own, `.stacked/NAME/lint` (under `agent.worktrees`
when that is set), kept for the next job, never in your checkout. It goes
patch by patch, from the bottom: the worktree holds the stack up to the
patch, with the fixes kept so far, and the linter is run as `COMMAND
FILE…` on the patch's files that are there. What it changes is folded into
the patch, as its new version, its message and author as they were, so
that each fix lands in the earliest patch it shows in; but only once each
patch above, as stored, is seen to still apply on it, in turn, in an index
of the check's own (the repository only read), and the new version leaves
its files as the linter did. At the first patch whose fixes would keep a
patch above from applying, none of its fixes is kept, and the patches from
it on are not linted one by one: with them as they are, the linter is run
once at the top of the stack, on their files, and what it changes goes in
a patch of its own at the end, `Lint fixes to fold into the stack`, by
you, its message saying which command made them, on which patches' files,
and why they were not folded in. A fix that adds or deletes a file, or
changes a file that is not text, a file's mode, or a file its patch
renames, is not folded in either: it stops the patches' run there the same
way, and goes in that patch at the end. That patch comes with a comment on
it, yours, ready to send but not sent: `Fold these lint fixes into the
patches they belong to, keeping every patch applying, then drop this
patch.` Asked of the whole stack, as usual, the agent folds them in.

What lands is one change of the stack, for your review: the new versions
of the patches that took fixes, and the patch at the end if there is one,
its comment one the work opened. The history calls it, as the banner and
the review do, `lint fixes in patches 1, 2 and 4, and a patch on top from
patch 5 on (./mach lint --fix)`. The linter's exit says whether it
reported something it did not fix: each run of it that exits otherwise
than with 0 is listed with the landed work, in the review under `what the
linter reported, to read`, a line for each (the patch it ran at, its files
and how it exited), what it wrote shown on the right; and under it in
`stacked landed NAME`. With nothing fixed and nothing reported, nothing
lands, and the status line says so. With nothing fixed and something
reported, that alone lands, read only, in the banner's section of what
changes nothing: its review is to read, then close. The rest is as for an
edit by hand: accepted, it goes in; rejected, nothing changes; edited by
hand from its review, it lands again with your edit.

A lint job takes its turn as a change of the agent's does, in the same
queue: once nothing changes the stack and no change of it waits for review;
and one at a time of every stack of the database, another waiting its turn.
While it waits or runs, the stack is not edited by hand, the menus saying
why (`the linter lints patch 2 of 5 (./mach lint --fix): …`), as while
the agent changes the stack. An edit asked of the
agent meanwhile waits for it (`ready (may change the code), it goes once
the linter's work on the stack lands`), then goes on from the stack the
linter made, without waiting for its review, as chained edits go on from
one another (Asking the agent, above): the linter's work rejected, it is
asked again. Answers go at once, as ever. The banner shows the job in a
section of its own, `the linter`, with where it is (`● The linter lints
patch 2 of 5 (./mach lint --fix)`, the last line it wrote after it; `◌ The
linter waits its turn to lint the stack`); Enter there shows what it
writes, all of which its run keeps (`Runs…`, `stacked transcript RUN`). A
job whose process is gone, killed, lands as failed once seen, as an ask
whose agent is gone does, and the stack is free again.

A linter that hangs does not keep the stack blocked. Delete on the job's
line of the banner, or `Stop the linter…` on the banner's menu, asks
first, then stops the linter as an ask's agent is stopped: the job's
process group, its process and the linter with what that runs, is sent
SIGTERM, then SIGKILL if it is not gone within a few seconds, and the job
lands at once, as stopped; the banner says `The linter is being stopped`
meanwhile. Nothing changes in the stack: the fixes folded so far are
thrown away, nothing lands for review, and the status line says so. The
stack can be edited by hand again, and the edits that waited for the
linter go. A job waiting its turn is dropped the same way, `Drop the
waiting lint job…`, on the banner's menu and the stack's, asking `drop the
lint job that waits its turn? Nothing was linted yet`: nothing is linted,
and the edits asked after it no longer wait for it. Delete on the job in `Runs…`,
or its menu there, does the same, and so does `stacked stop RUN`, the run
as `stacked runs` lists it. What the linter left in the worktree is thrown
away as the next job starts, so that the next job starts clean.

`$STACKED_LINT` names a program run in place of the linter's own, given
the command's arguments and the files: the tests' stand-in. From a process
that keeps nothing, as the tests' own, only that runs, never the command,
so never a `./mach`.
