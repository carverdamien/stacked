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
- `Pull from Phabricator…` (built with the `moz-phab` feature, on by
  default; not while reviews come from nowhere): a revision, `D123` (or
  `123`) or its link, said as it is typed (`D12, with the revisions it
  stacks on and those stacked on it`), refused while a stack holds it, or
  the work waiting for your review on a stack adds it (`D13 is patch 3 of
  D12: pull it there`). The stack is made at once, empty, on HEAD,
  named after the revision as no stack is, with the pull's run, and
  shown. The revision is pulled away from the screen with the revisions
  it stacks on, down to the first that is not open, and those stacked on
  it while one is open, none of which a stack may hold, as a stack's pull
  pulls them (Phabricator, below); read, the stack is moved onto the
  commit the bottom one's diff is on, which the repository must have
  (`D11 is on 0123456789ab, which this repository does not have: fetch it
  first`, the stack staying, empty, as when the read fails), and the pull
  lands into it for your review, its review open: a patch to add for each
  revision, its latest diff with the author and the message of its commit
  as uploaded (or its title and summary, where Phabricator has them
  otherwise), and the reviewers' comments. `stacked moz-phab import D123`
  does the same from the command line, named after the revision, or
  `--name NAME`, and prints the run, which `stacked review NAME RUN
  --accept-all` takes.

An import from the workspace is seen before it is made: its patches,
bottom first, the commit they go on, and the name of the stack it makes,
after the branch, as no stack is (`Name it otherwise…`). `Import it as
NAME` makes it, and shows it.
