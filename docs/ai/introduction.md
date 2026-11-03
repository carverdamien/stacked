# stacked

A stack editor, in the terminal. Patches, the stacks they make and every
edit made to them, comments on them, and what the agent (claude) and
Phabricator said: all in one file that is only ever appended to (its
header aside), so that any edit can be undone and the history read back. A
comment can be asked of the agent, which answers it or changes the
patches; what it brings back waits for your review. Your checkout is not
touched unless you ask: every ask of the agent runs in a worktree of its
own, never in your checkout.

What follows are the notes on using it that do not belong in its `--help`.
Press `?` in the editor: it says what can be done where the cursor is.
