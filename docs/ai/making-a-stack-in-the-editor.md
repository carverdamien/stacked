# Making a stack in the editor

With no stack yet, the menu (Space, or Enter) has the ways to make one;
with stacks, the stack's own menu has them under `New stack…`:

- `Ask the agent to make a stack…`: named after the branch checked out, as
  no stack is, the stack is made empty, on HEAD of the checkout stacked
  runs in, and a comment on it opens, to say what the agent should make of
  it. Esc, then `Ask agent…`, opens the ask dialog, which saves the comment
  and asks the agent; what it makes waits for your review, as any of its
  work does.
- `Import from the workspace…`: the branch checked out, on its upstream
  (else `origin/HEAD`, else main or master: `feature on origin/main: 3
  commits`); `Another branch…`, each on its own; or `A range…`, typed, and
  said as it is typed (`unknown ref`, `0 commits: nothing to import`, `2
  commits`), taken only once it holds a commit. The staged changes, and the
  unstaged ones, when there are, may each be checked to go on top of what
  ends at HEAD, a patch of their own (`Staged changes`, `Unstaged changes`).
- `Import from Phabricator…` (built with the `moz-phab` feature, on by
  default): a revision, `D123` (or `123`) or its link, read away from the
  screen with the revisions it stacks on, down to the first that is not
  open, and those stacked on it while one is open; each its latest diff,
  with the message and author of its commit as uploaded. They go on the
  commit the bottom one is on, which the repository must have (`D11 is on
  0123456789ab, which this repository does not have: fetch it first`).
  `stacked moz-phab import D123` makes the same stack from the command
  line, at once, named after the revision as no stack is, or `--name
  NAME`.

An import is seen before it is made: its patches, bottom first, the commit
they go on, and the name of the stack it makes, after the branch or the
revision, as no stack is (`Name it otherwise…`). `Import it as NAME` makes
it, and shows it.
