# stacked's specification

The code is the truth: where these files and the code disagree, the files
are fixed to say what the code does.

These files are stacked's exhaustive specification. They are written for
the agents that work on stacked, and kept up to date by them: every
behaviour, every text the screen shows, every setting, the database's
format and its rules, the layout of the code, and every test. They are
long and exact rather than friendly; people start with the
[README](../../README.md) at the top of the repository.

Each file is one section of the specification, named after its heading.
The text names other sections by their headings ("(Settings, below)"):
"above" and "below" follow the order of the list here.

A change to stacked updates the file of its area here, and `tests.md` for
its tests. A new section is a new file, listed here in its place.

- [Introduction](introduction.md): what stacked is, in a paragraph, and
  its three front ends; what these files are, and for whom; and where help
  is in the editor.
- [Install](install.md): installing with cargo, the five features and what
  each adds, the wasm target, what StackED needs on Linux, and the
  completion in zsh.
- [Running](running.md): the command line by example, and which database
  a command opens.
- [A tour](a-tour.md): `stacked demo`, its fourteen sections on a game of
  tic-tac-toe, and the one setting it writes.
- [In a browser](in-a-browser.md): `stacked web`, its mouse, its token,
  and how the page and the server share the editor.
- [In a window of its own: StackED](in-a-window-of-its-own-stacked.md):
  `stacked app`, its fonts, mouse, windows and File menu, `--bundle`,
  StackED opened as an app, and its icon.
- [The editor](the-editor.md): the panes, HEAD's line, Move HEAD and
  Reconcile with HEAD, the Phabricator statuses and line, and every key.
- [Making a stack in the editor](making-a-stack-in-the-editor.md): a stack
  made by the agent, or imported from the workspace or from Phabricator.
- [Writing comments](writing-comments.md): threads, their colours and
  markdown, links, the inline editor, references, replies, turns written
  again, and conversations rewound.
- [Editing a patch by hand](editing-a-patch-by-hand.md): a file or a patch
  in `$EDITOR`, conflicts resolved by hand, a file's changes dropped, long
  message lines wrapped, and the review each edit lands for.
- [Asking the agent](asking-the-agent.md): the ask dialog, its permissions
  and efforts, what each kind of ask may use, references, the batch, the
  worktrees, the queue, chained changes, how a change lands, a comment's
  states, and stopping an ask.
- [What the agent brings back waits for your review](what-the-agent-brings-back-waits-for-your-review.md):
  the queue of landed work, the banner, the review of each part, the
  answers a change leaves behind, work sent back and follow-ups, and edits
  made from a review.
- [The tools the agent could not use](the-tools-the-agent-could-not-use.md):
  each refused call as an item of the review, its rule, and its states:
  refused, allowed for this work, allowed from now on, or noted.
- [Try push](try-push.md): a comment asking for Firefox's CI tasks, the
  agent's selection and its review, the push with `./mach try fuzzy`, and
  the push followed on Lando and Treeherder.
- [Linting](linting.md): `Lint the stack`, the checkout's linter, the lint
  job patch by patch, the fixes folded in or put in a patch on top, and
  their review.
- [Reading](reading.md): the reading mode, its marks by blocks of code, the
  message read as code is, what needs no reading, and how far the review
  is.
- [Searching](searching.md): the search's own screen, where it looks
  first, and `/`.
- [History](history.md): the tree of edits, `History…` and its stack
  diffs, how two versions of a message are compared, the versions of a
  patch, a comment's history, and questions on the history.
- [Trimming the history](trimming-the-history.md): `stacked trim`, and what
  the new database keeps.
- [Settings](settings.md): `~/.stacked/config.toml`, `stacked config`,
  every setting, `stacked setup agent`, the files of before, and a
  checkout's own `.stacked/local.toml`.
- [Only patches that apply go in](only-patches-that-apply-go-in.md): the
  check every stack passes before it is written, and read-only stacks.
- [One file, only appended to](one-file-only-appended-to.md): the
  database's records, its lock, its header and its checkpoints.
- [The tree of edits](the-tree-of-edits.md): undo, redo, branches, and
  `stacked tree`.
- [Phabricator](phabricator.md): revisions imported, reviews pulled and
  filed, reviewers' edits, links to bugs, the statuses, and a patch
  submitted as work in progress or a revision in review updated.
- [Copied from self-review-rs](copied-from-self-review-rs.md): where
  stacked comes from, and what it copies.
- [Layout of the code](layout-of-the-code.md): what each source file does.
- [Tests](tests.md): the full check of a change, how the tests keep off
  the network and your files, their stand-ins,
  `STACKED_TEST_CHECKPOINTS=1`, and what each test shows.
- [Not done yet](not-done-yet.md): what stacked does not do yet.
- [License](license.md): MIT.
