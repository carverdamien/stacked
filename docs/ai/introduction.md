# stacked

stacked is a stack editor. It runs in a terminal, in a browser (`stacked
web`) or in a window of its own, StackED (`stacked app`), the same editor
with the same keys in all three. It keeps patches, the stacks they make
and every edit made to them, comments on them, and what the agent (claude)
and Phabricator said, all in one file that is only ever appended to (its
header aside), so that any edit can be undone and the history read back. A
comment can be asked of the agent, which answers it or changes the
patches; what it brings back waits for your review. Your checkout is not
touched unless you ask: every ask of the agent runs in a worktree of its
own, never in your checkout.

The files of `docs/ai/`, this one first, are stacked's specification, a
section each, in the order of their [index](README.md). They are written
for the agents that work on stacked, and say exactly what stacked does, in
the terminal, in the browser and in StackED; people start with the
[README](../../README.md) at the top of the repository. Press `?` in the
editor: it says what can be done where the cursor is.
