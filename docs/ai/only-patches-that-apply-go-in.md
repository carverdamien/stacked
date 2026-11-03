# Only patches that apply go in

A stack goes in the database only once its patches are seen to apply
cleanly, each on the ones before it, to its base: what an import, an add, a
move, a drop, an edit by hand, a todo list, a new base or the agent's edit
would make. If they do not, nothing is written and the error says which
patch failed and where (`patch 3 (...) does not apply to the patches below
it (patch failed: a.txt:12)`). Undo, redo and picking from the tree only go
to a stack that applies too. A patch with no diff is refused, as `git am`
cannot apply it.

An edit by hand that does not apply comes back in `$EDITOR` with why at its
top, as `git rebase -i` does: put it right, or empty the file to give up.
So does a todo list, with the patches already edited as you left them.

A pull from Phabricator keeps the rule too (Phabricator, below): a patch
that no longer applies on the versions it pulled under it, or a pulled
diff that does not apply on the patches under it, is made again on them,
its conflicts marked in its files where they disagree, so that the stack
its change makes applies; and such a patch does not go in while it holds
them: its review does not close with it, until its operation is
rejected, its conflicts are resolved by hand, or the agent reworks it.
What cannot be made again so is not taken, said in its review.

The check never touches the worktree, nor the repository's index or object
store. Each patch is applied with `git apply --cached` to an index of the
check's own, holding only the files the patches touch, and what it writes
goes to an object store of its own. On a repository of 300,000 files, a
move in a stack of ten patches takes 0.2s, and one refused 0.4s: a failure
is checked again against the whole base before it is believed.

The landing of a change of the stack checks the same way the stacks its
operations leave, each operation rejected alone, to work out which
operation needs which, stored with the work (What the agent brings back
waits for your review, above): which patch is the first that does not
apply, if one does not. What it finds of a stack, applying or not, holds
for good, a stack being its base and its patches by their contents: it is
kept while the process runs, and told again at once. A change of twenty
patches, each with an operation, none needing another, is 21 checks as it
lands: 0.14s in all on Linux, on a small repository; a chain of twenty,
each needing the one under it and going with it, 114 checks, 1.4s. Its
review checks nothing to mark an operation: closing it in part checks the
stack the operations accepted leave, once, away from the screen, and
refuses to close, saying why, if that stack does not apply.

A stack with no base (a stack from before the check), or whose repository
or base commit is not there (the database copied to another machine, or
the repository moved), cannot be checked, and so is read-only: comments can
still be added, the reviewers' comments pulled from Phabricator (a pull
takes no patch then), the patches read and exported, but
nothing changes the stack and the agent is asked nothing. The editor's bar
says why, in yellow (`colors.warn`). `stacked base NAME REV`, from the repository, gives it
a base it applies to, and it can change again. `stacked check` says, for
each stack, whether it applies.
