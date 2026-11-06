# Layout of the code

- `main.rs`: the command line; the library does the rest; and a write
  refused as it would change patches that should not change asked about on
  its terminal (`edit_asking`).
- `complete.rs`: the shell's completion: what `stacked __complete` offers
  for the word being typed, from stacked's commands and their options, and
  from the database (its stacks, a stack's patches and comments, the
  agent's runs); and the function for zsh that `stacked completions zsh`
  prints.
- `db.rs`: the file, its records and its lock; every write of a stack
  guarded, refused as an error of its own kind when it changes patches that
  should not change and were not confirmed for it (`edit_confirmed`).
- `protected.rs`: the patches that should not change, their revisions
  Accepted or Landed on Phabricator as last fetched (`protecting`, the one
  place that says which statuses); what changes one; those an edit by hand
  may change; those a pull brings to what Phabricator has of them; and the
  refusal, said, for the dialog and the command line.
- `check.rs`: whether patches apply, before they go in; the files as a
  patch leaves them, and the tree, for a submit; the files around a
  version, before and after it on its base, and a file from one of them
  to another as git writes a diff.
- `change.rs`: the change from one version of a patch to another as a
  patch of its own, each version's files read on its own base, nothing
  made again on another; what of it the change of base made, drawn dim;
  and the changes read away from the screen, a few at a time.
- `ops.rs`: edits to a stack, `$EDITOR` (the patch as mail, its code); the
  interdiff of two versions of a patch, each of its lines told apart by
  what part of its version's mail it is, and what changed in a line of a
  hunk as its own version's diff pairs it, and where it is in its
  version's diff, under which file's and hunk's headers, for the views of
  the interdiffs to draw; where each of its lines is in each version's code
  (`code_places`); whether two diffs of a patch are on two bases
  (`other_bases`); a patch made again on other patches under it, its
  conflicts marked where they disagree, as a pull makes one (`rebased`).
- `git.rs`: what stacked asks of git: mail from a range, the stack's
  worktrees and the agents', a checkout to edit a patch in.
- `ask.rs`, `claude.rs`: asking the agent about comments, from the queue to
  landing, and the folders of the agent's rebase plans; claude's command
  line, its stream and its replies.
- `reach.rs`: whether the agent can be reached, for the ask dialog's Ask
  Now: its program found, and the network reaching the host claude talks
  to, a stand-in answering for the tests.
- `held.rs`: what an ask brought back, held for your review, a pull's too,
  what each of its patches takes from Phabricator and the conflicts it
  marked, which keep its review from closing with them; work sent
  back, and the follow-ups that only read asked from its review, what they
  bring back coming into it (`come_in`); and the marks of the answers made
  on the stack before a change, kept, restarted or stopped as the change
  is accepted (`ask.rs` does what each mark says).
- `operations.rs`: the operations of a change of the stack, a line of its
  stack diff each, as its review marks them one by one; the stack the
  operations accepted leave; what each needs to apply, worked out as the
  work lands by the check of `check.rs`, which keeps what it found of each
  stack it applied, and stored with the work (`Needs`); what the review
  decides from it, at once, with what refusing to mark one alone says, and
  what goes with it; why a change is marked as a whole instead; and the
  check closing in part makes, away from the screen, of the stack the
  operations accepted leave.
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
  `moz-phab` feature (on by default): a pull, of a stack, of one patch's
  revision or into a new stack, queued as an edit as it is asked, read
  (`phab.rs`), its read cut short landed as failed, then made on a copy
  of the database and filed for one review, its new
  versions, the patches it adds, those it makes again on what it changed
  under them, and the reviewers' comments (`review.rs`); a patch submitted
  as work in progress, or its title, summary and parent alone to a
  revision in review, worked out from what is stored of Phabricator, what
  it lacks fetched first, and sent in the writes moz-phab makes, nothing
  read, and, once a write changed the revision, remembered with when, so
  that the revision is marked `↻` until a read of it is stored after that
  (`submit.rs`); Fetch the latest from Phabricator, every revision of
  a stack read in a few calls, and stored, only what differs from what is
  stored, as what a pull reads is stored, and the fetch a submit makes
  first, its calls the submit's own; and what the Phabricator line over a
  patch's message counts, and when what it says was fetched, as the
  review of a submit says it too (`phab::fetched_at`);
  Conduit called by curl, the token on its standard input, what
  Phabricator answered said, and its refusals of this machine (HTTP 429)
  an error of their own.
- `bug.rs`: a bug filed for a patch whose title names none, as Firefox's
  bug-filing skill files one: what an ask drafting it is told and answers
  with (`Way::Bug`, with the `moz-phab` feature), the draft read from its
  answer, the link of Bugzilla's form filled in with it, the bug's number
  read from what is typed, and the number put in the patch's title.
- `try_push.rs`: the try push, built with the `try-push` feature (on by
  default): what an ask of a try push is told and answers with, its tasks
  read from its answer, and what a round asked again is told of them; the
  list of the tasks mach can push, made before the ask's agent starts
  (`list`), in a worktree for pushes held as a push holds one; the
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
- `markdown.rs`: the markdown of comments, as the editor draws it: each
  piece of a turn's text marked as what it is (code, bold, italics, a link
  and where it goes, a heading, a quote's bar, a list's bullet, a line of a
  code block, a rule, a mention of references), for the editor to colour;
  the lines broken where the text breaks them and again where they are too
  wide, a code block's lines keeping their spaces, a table's cells folded
  within their columns; and a comment said in a line, its markup left out.
- `refs.rs`: the references of comments: those the agent attaches to what
  it writes, in a `refs` block (lines of a file, as a patch, the base or
  the whole stack has it; another comment of the stack; a web link), and
  those you attach to a comment, numbered `r1`, `r2` once and for all,
  which mentions name (`@r1`, `@r*`); those that a rewind or an edit
  detaches, as it takes away their last mention; and a web link checked,
  then opened in your browser by `$STACKED_OPEN`'s program, else as the
  system opens one.
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
  checkout's own over it, but for `[web]`, the settings' file's alone), the
  program the editor's command runs, as StackED and `stacked web` name it
  while it runs, and whether that program opens a window of its own; and
  the keys they map, each in its context.
- `home.rs`: where stacked keeps what is yours, ~/.stacked, as the binary
  works it out as it starts (nowhere, in the tests' own processes), the
  files of before moved into it, and where `stacked web` keeps the token
  of its address, beside the state.
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
  dialog, or reviewed on a screen of its own, before it is sent, with how
  long ago what it shows of the revision was fetched, its code in the view
  of the interdiffs that 1 sets, its menu on Esc;
  `tui/revisions.rs`, the status on Phabricator of each patch's revision,
  which the patches pane shows, and the Phabricator line over the message
  of the patch the middle shows, from what is stored, with how long ago it
  was fetched, or, a submit having changed it since, the pane's `↻` and
  what the line says of it, what differs on a screen of its own, a revision's code
  drawn as the views of the interdiffs draw it there and on the review of
  a submit, the interdiff of its latest diff and the patch's, or each file
  as each leaves it, the pulls, of the
  stack, of the patch's revision, with its choice, or into a new stack,
  and the stack's pull under way, reading Phabricator, waiting its turn
  or being made, its line's menu and its drop, the submit item as
  its revision's status has it, and the
  fetch, asked from a menu; `tui/phab_menus.rs`, every `Phabricator`
  menu made in one place, the stack's, a patch's and its line's: its
  entry, its title, its items in their order and its note, with the
  feature's and the settings' gates; `tui/compared.rs`, two
  versions of a text compared word by word where their lines wrap aside,
  as rows: a title and summary in the review of a submit, a reviewer's
  comment edited on Phabricator, or read again with its suggested edit,
  in the review of a pull, a commit's
  message against its patch's; and a diff, or a commit, as the middle
  draws a patch; `tui/interdiffs.rs`, the views of an interdiff that `1`
  sets on every screen that shows one, as an interdiff, in a column per
  version or side by side, what says how each reads, what of an interdiff
  they draw with only what the patches changed, and the line saying how
  many are left out, and where a screen's place is found again in another
  view; `tui/views_dialog.rs`, the dialog of them that `1` opens there,
  laid out as the ask dialog: what is compared, the view and the lines,
  each set at once, the line under each saying what its value draws, then
  Done or Cancel;
  `tui/protected_dialog.rs`, the dialog asking before patches that should
  not change are changed, laid out as the ask dialog: a line for each
  patch, how long ago their statuses were fetched, a field where `accept`
  is typed, then Change them or Cancel; what is done again once it is
  typed, as it was asked; and the edits by hand in $EDITOR asked about
  before the editor opens;
  `tui/bug_dialog.rs`, the dialog that takes the number of the bug the
  agent drafted once its review closed and its form opened, laid out as
  that one: a field for the number or a link to the bug, then Put it in
  the title, Open the form again or Cancel;
  `tui/change.rs`, the change from one version to another as a patch of
  its own in the views of the code there, the lines the change of base
  made left out with only what the patches changed, what each screen
  knows of each version's base, and the line of the interdiff nearest each
  line of it;
  `tui/head_why.rs`, the view of why Move HEAD does not
  move HEAD, and what you can do in your checkout so that it can;
  `tui/banner.rs`, the banner of the agent's work; `tui/notices.rs`, the
  notices of `stacked web`, what the editor learns of every stack as it
  reads the database again, said for the system's banners and kept for
  them or for the bell, and a click on one in the page; `tui/places.rs`, where
  each pane's cursor was, each half's of the middle split in two;
  `tui/wheel.rs`, the wheel of the browser and of StackED: whether the
  mouse is in use, what the wheel scrolls under it then, and ↑↓ after a
  key; `tui/drag.rs`, HEAD's row dragged with the mouse there, a press on
  it held until it is a click or a drag; `tui/scrollbar.rs`, the scroll
  bar of the place the cursor is in while what it holds does not fit,
  where each place draws it, and its thumb;
  `tui/finding.rs`, the search of the middle (`/`); `tui/ask_dialog.rs`,
  the dialog every `Ask agent…` item opens, its permission and effort set
  each time, then Ask Now, Ask Later or Cancel; and a review's `Send it
  back to the agent…`, then Send Now or Cancel; the lists of a dialog's
  lines, and its buttons, as the dialog of the views of the interdiffs
  draws them too.
- `web.rs`, `web/client.rs`, `web/index.html`, `build.rs`: `stacked web`,
  its server and the port it listens on, the token it keeps with a port
  fixed and the tab it waits for, its wasm and its page, the
  notices it sends the page, which shows them as the system's banners,
  and the bell it rings for them, made by the page with the Web Audio API,
  the editor run in the page, its screen sent as stacked's own is, what the
  pages send made its keys, pastes and mouse, the pages lent it meanwhile
  (`in_page`), what the page says over its screen while `$EDITOR` runs
  where `stacked web` was started, or once stacked is gone, the page asking
  again until it answers, and `stacked web` quit once its last page is
  closed, StackED's icon on its tab (`icon.rs`, below); the build script
  compiles the wasm, or, where Rust lacks the wasm target, stops the build
  and says what to run. `pty.rs`, built with `web`: the editor's terminal
  in the page, a pseudo-terminal of stacked's own, the editor in a session
  of its own there, read on a thread of its own, resized, and hung up on as
  it goes, nothing of it left running; its screen as vt100 makes it of what
  the editor writes, a buffer for `web.rs` to send, what the editor asks of
  its terminal answered as xterm answers it; and the bytes xterm sends for
  a key, a paste and the mouse, as the editor asks for them. The same code
  on Linux and macOS, whose libc differ.
- `app.rs`: `stacked app`, StackED: the editor on a thread of its own, and
  the window that paints its screen and sends it what is done there; the
  fonts the system has for it, as fontdb finds them.
  `app/windows.rs`: its windows, on every system, one for each database
  opened, the first in eframe's root viewport, and what the File menu's
  items do; `app/menu.rs`, the menu bar at the top of each window (egui's),
  its one menu, File, used with the mouse; `app/dialogs.rs`, the
  system's Open and Save dialogs and its alerts (rfd's), asked on a thread
  of their own. `icon.rs`: StackED's icon, drawn at any size, and written
  as a PNG of stacked's own for `stacked web`'s page, which is why `web`
  builds it too.
  `launcher.rs`: what StackED is to each system, made by `stacked app
  --bundle` and read when the system opens it: StackED.app on macOS, its
  launcher and icon on the desktops that follow freedesktop.org, the
  terminal `$EDITOR` runs in on each, and the Edit menu macOS's panels
  need; the one place that tells the systems apart. `bundle.rs`:
  StackED opened as an app, with no terminal: the login shell's
  environment taken, and `$EDITOR` run in a terminal window of its own.
- `demo.rs`, `memory.rs`: `stacked demo`, the tour; and the database and
  repository it keeps in memory, with a stand-in for the agent.
