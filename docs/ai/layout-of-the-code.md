# Layout of the code

- `main.rs`: the command line; the library does the rest.
- `db.rs`: the file, its records and its lock.
- `check.rs`: whether patches apply, before they go in; the files as a
  patch leaves them, and the tree, for a submit.
- `ops.rs`: edits to a stack, `$EDITOR` (the patch as mail, its code); the
  interdiff of two versions of a patch.
- `git.rs`: what stacked asks of git: mail from a range, the stack's
  worktrees and the agents', a checkout to edit a patch in.
- `ask.rs`, `claude.rs`: asking the agent about comments, from the queue to
  landing, and the folders of the agent's rebase plans; claude's command
  line, its stream and its replies.
- `reach.rs`: whether the agent can be reached, for the ask dialog's Ask
  Now: its program found, and the network reaching the host claude talks
  to, a stand-in answering for the tests.
- `held.rs`: what an ask brought back, held for your review; work sent
  back, and the follow-ups that only read asked from its review, what they
  bring back coming into it (`come_in`); and the marks of the answers made
  on the stack before a change, kept, restarted or stopped as the change
  is accepted (`ask.rs` does what each mark says).
- `head.rs`: where HEAD of your checkout is against a stack, its commits
  marked, and HEAD moved along it, only as stacked left it or with no mark;
  what the view of why Move HEAD does not move HEAD reads of the checkout.
- `reconcile.rs`: Reconcile with HEAD, the commits' messages taken into the
  stack, queued and reviewed as an edit is.
- `messages.rs`: two versions of a commit message compared word by word,
  where their lines wrap aside, and so as a diff of their lines, for the
  interdiffs and the submit's review to colour; and what
  each section of a message says, for the reading marks to hold.
- `wrap.rs`: a commit message's long lines wrapped at a width, its
  paragraphs of prose filled again, for the menus to offer.
- `links.rs`: the links a line of a commit message or of a comment's text
  has, to open in your browser: its web links, markdown's too, and, with
  the `moz-phab` feature, the bugs it names and the revision its trailer
  names.
- `review.rs`, `phab.rs`, `submit.rs`: Phabricator, built with the
  `moz-phab` feature (on by default): the reviews pulled, the stacks
  imported, a patch submitted as work in progress, or its title, summary
  and parent alone to a revision in review; Fetch the latest from
  Phabricator, every revision of a stack read in a few calls, and stored,
  only what differs from what is stored, as what a submit and a pull read
  is stored; and what the Phabricator line over a patch's message counts;
  Conduit called by curl, the token on its standard input, what
  Phabricator answered said, and its refusals of this machine (HTTP 429)
  an error of their own.
- `try_push.rs`: the try push, built with the `try-push` feature (on by
  default): what an ask of a try push is told and answers with, its tasks
  read from its answer, and what a round asked again is told of them; the
  push of the tasks a review accepts (its run's `push`), made by `stacked
  try-push` in the stack's worktree for it, its base checked against the
  remotes mach takes as Firefox's, `./mach try fuzzy --no-push` compared
  with what was accepted, then the push, mach's lines in its stream, its
  login and its link kept, its replies, the lock of its comment, and its
  stop; then the push followed on Lando and Treeherder (`follow`), its
  tasks counted as Treeherder shows them, and its summary, held for
  review once it is done.
- `lint.rs`: linting a stack: the linter of a checkout (its own setting,
  else `./mach lint --fix` where `./mach` is), its command split as a
  shell splits words, and the rule that lets a change run it; a lint job
  (its run's `lint`), its turn in the queue
  taken as a change's (`ask::claim`), run by `stacked lint-job` in the
  stack's lint worktree, patch by patch, each patch's fixes folded into it
  once the patches above still apply, the rest in a patch at the end with a
  comment to send; what the linter reported; what lands for your review;
  and a job whose process is gone.
- `import.rs`: what a stack made in the editor is imported from: a range of
  the checkout, its uncommitted changes on top if asked.
- `mbox.rs`: reading `git format-patch` mail, writing mail for `git am`.
- `rows.rs`, `view.rs`, `diff.rs`, `thread.rs`: the patch or the stack as
  rows, line keys, comment threads.
- `panes.rs`: the compact layout's two panes.
- `whole.rs`: each file of a patch whole, as `3` shows it: how its hunks go
  in it, and its files read once for each patch, in a thread of their own.
- `reading.rs`, `blocks.rs`: the reading mode's marks, by blocks of code.
- `progress.rs`: how far the review of a stack is.
- `tree.rs`: the tree of edits as a graph, and the lanes the history draws.
- `history.rs`: a stack's history, what changed it, and stack diffs: as the
  editor shows them and the agent is told them.
- `trim.rs`: a new database, the start of the history left out.
- `todo.rs`: the `git rebase -i` style list.
- `search.rs`: searching what stacked holds, in a thread of its own.
- `config.rs`, `keys.rs`: the settings (`~/.stacked/config.toml`, and a
  checkout's own over it), and the keys they map, each in its context.
- `home.rs`: where stacked keeps what is yours, ~/.stacked, as the binary
  works it out as it starts (nowhere, in the tests' own processes), and
  the files of before moved into it.
- `state.rs`: the databases opened last, by StackED and by stacked's
  editor, StackED's windows to open again, and the searches of the middle,
  kept between runs under a lock.
- `setup.rs`: `stacked setup agent`, what each kind of ask may use, one
  list with a box for each kind: what stacked always gives, the checkout's
  linter named; where what it lists comes from (the agent,
  the checkout's claude settings, its skills and its instructions), which
  rules mean the same and are one row, what the checkout's claude settings
  of yours deny, refused to every ask, what the reviews allowed from now
  on, to keep or take out, on a screen (`--tui`) or line by line, and what
  saving writes.
- `refusals.rs`: the tools the agent could not use, as items of the
  review of its work: the rule each call refused needs, narrow for a broad
  program and wider to pick, the narrowest also the rule that lets a change
  run the checkout's linter; one item a rule, what is noted and why, the
  agent's word on each; their three states as the review's marks; what this
  work may use for itself, round after round; and what is saved from now
  on, in the checkout's own settings.
- `editor.rs`: the inline comment editor.
- `paint.rs`: colours, and drawing text and boxes.
- `tui.rs`: the editor, and the work it does away from the screen; what a
  click is on; the views of the code, the middle split in two among them,
  each half its own (`Half`). `tui/reviewing.rs` and `tui/searching.rs`:
  its screens for reviewing what the agent brought back, and for
  searching;
  `tui/submitting.rs`, what a submit to Phabricator sends, said in a
  dialog, or reviewed on a screen of its own, before it is sent;
  `tui/revisions.rs`, the status on Phabricator of each patch's revision,
  which the patches pane shows, and the Phabricator line over the message
  of the patch the middle shows, from what is stored, with how long ago it
  was fetched, what differs on a screen of its own, the pull of its
  reviews alone, the submit item as its revision's status has it, the
  patch's `Phabricator` menu, and the fetch, asked from a menu, or after a
  submit; `tui/compared.rs`, two
  versions of a text compared word by word where their lines wrap aside,
  as rows: a title and summary in the review of a submit, a reviewer's
  comment edited on Phabricator in the review of a pull, a commit's
  message against its patch's; and a diff, or a commit, as the middle
  draws a patch; `tui/head_why.rs`, the view of why Move HEAD does not
  move HEAD, and what you can do in your checkout so that it can;
  `tui/banner.rs`, the banner of the agent's work; `tui/places.rs`, where
  each pane's cursor was, each half's of the middle split in two;
  `tui/finding.rs`, the search of the middle (`/`); `tui/ask_dialog.rs`,
  the dialog every `Ask agent…` item opens, its permission and effort set
  each time, then Ask Now, Ask Later or Cancel; and a review's `Send it
  back to the agent…`, then Send Now or Cancel.
- `web.rs`, `web/client.rs`, `web/index.html`, `build.rs`: `stacked web`,
  its server, its wasm and its page; the build script compiles the wasm,
  or, where Rust lacks the wasm target, stops the build and says what to
  run.
- `app.rs`: `stacked app`, StackED: the editor on a thread of its own, and
  the window that paints its screen and sends it what is done there; the
  fonts the system has for it, as fontdb finds them.
  `app/windows.rs`: its windows, on every system, one for each database
  opened, the first in eframe's root viewport, and what the File menu's
  items do; `app/menu.rs`, the menu bar at the top of each window (egui's),
  its one menu, File, used with the mouse; `app/dialogs.rs`, the
  system's Open and Save dialogs and its alerts (rfd's), asked on a thread
  of their own. `icon.rs`: StackED's icon, drawn at any size.
  `launcher.rs`: what StackED is to each system, made by `stacked app
  --bundle` and read when the system opens it: StackED.app on macOS, its
  launcher and icon on the desktops that follow freedesktop.org, the
  terminal `$EDITOR` runs in on each, and the Edit menu macOS's panels
  need; the one place that tells the systems apart. `bundle.rs`:
  StackED opened as an app, with no terminal: the login shell's
  environment taken, and `$EDITOR` run in a terminal window of its own.
- `demo.rs`, `memory.rs`: `stacked demo`, the tour; and the database and
  repository it keeps in memory, with a stand-in for the agent.
