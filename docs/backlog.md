# Backlog

What is asked for and not built yet, in the words it was asked in, with what
was found about it. Two lists: **To build**, asked for, waiting their turn;
**Parked**, asked to be kept for later, started only when asked. An item that
is built leaves this file in the commit that builds it.

## To build

1. **Reword and HEAD** (2026-11-05: "Can we double check that accepting a
   Reword work should offer sync HEAD if it can do it safely.").
   - **Checked by reading: yes.** The review's Esc menu has
     `HEAD [x] sync [ ] no sync`, shown when `head_can_follow` holds
     (`src/tui/reviewing.rs`): a stack change not rejected, HEAD seen in sync
     at the stack's top, a clean checkout. Closing accepted runs
     `head::follow` (`src/head.rs`), and `put` rewrites from the first marked
     commit up. Nothing compares diffs, so a message-only change follows like
     any other. Nothing follows from a close from the banner, nor from
     `stacked review`.
   - **To build:**
     - (a) A test of a reword-only review with HEAD following; the three in
       `tests/tui.rs` that cover HEAD following land a Modify.
     - (b) A reword that changes or drops the `Differential Revision:`
       trailer marks its commit `!`. `put` then keeps it and says "HEAD is at
       patch N of s already" while HEAD keeps the old message, though sync
       was offered. The fix: `head_can_follow` declines then, or `follow`
       says why.

2. **A first submit sets the status icons** (2026-11-05: "When submitting a
   pach for the first time, the status icons should be set.").
   - **Today:** a new revision's line shows the letter cell blank, then `↻`.
     The `↻` comes from the Submitted record (`sync_mark`,
     `src/tui/revisions.rs`); the letter comes from a fetched state only
     (`revision_status`), and none is stored for a new revision.
   - **Proposal:** `Record::Submitted` keeps the status the submit set
     (`changes-planned` for a new revision, `#[serde(default)]`). The letter
     is drawn from it while nothing of the revision is fetched: `[W]↻` until a
     fetch.
   - **To settle:** `[W]↻`, or `[W]✓`, as stacked knows Phabricator has
     exactly this version.

3. **Needs Review protects a patch too** (2026-11-05: "Do we have a feature to
   warn/block editting a patch which is marked "Needs Review"?", then "Add it
   to the backlog").
   - **Today:** `protecting()` (`src/protected.rs`) lists `accepted` and
     `published` alone. A submit sends no new diff to a revision that is not
     in work (`src/submit.rs`), so a local edit stays local.
   - **The change:** one arm for `needs-review` in `protecting()`, with its
     words ("Patch 3, D123, is in review on Phabricator: a reviewer may be
     reading this version."); the dialog's per-status tests extended;
     `docs/ai/phabricator.md` ("Patches that should not change") and the
     README.
   - **To settle:** strict like the others (the default), or a lighter
     warning for Needs Review alone.

4. **Reading: a permanent footer** (2026-11-05: "In reading mode. Pressing
   SPACE can mark lines that are not currently visible. Add footer and/or
   header when such lines exists.", then "Ok, let's have a permanent footer
   that tracks the totals. Mark X lines as read or Unmark Y lines", "We don't
   need the arrows anymore", "Show only the part that applies").
   - **Today:** Space marks the whole code block at the cursor (`mark_read`,
     `src/tui.rs`; `reading_at` → `Reading::to_mark`), which can run past the
     pane's top or bottom; only after does the bottom line say
     "read: {what}, {n} lines". Delete unmarks (`to_unmark`).
   - **Decided:** a footer always there in reading mode, under the middle's
     code, following the cursor: "Mark X lines as read or Unmark Y lines",
     X what Space would mark there, Y what Delete would unmark. Only the part
     that applies: "Mark X lines as read", or "Unmark Y lines". No header, no
     arrows, no off-screen counts.
   - **Assumed, unconfirmed:** with neither, the row stays, empty, so that
     the code does not move.

5. **Questions open on what was merged on 2026-11-06:**
   - `--model` on `stacked send` and `stacked review --send-back` (beside
     their `--effort`): keep, or drop as a CLI duplicate of the dialog?
   - The Ask Agent dialog's unset Model line wraps to three lines, so the
     dialog covers the comment being written on a 30-row screen: shorten it
     to one line?
   - A protected child whose diff's context lines change because its parent
     changed still asks for `accept` (`protected::rebased` compares context
     lines too): keep, or compare only the added and removed lines?
   - Editing the stack as a list still asks about every protected patch
     before `$EDITOR` opens: keep?

6. **UI pieces built in several places** (the audit of 2026-11-06, at
   82c8acd, after "whenever we change a small piece of UI somewhere, it
   impacts all the places that uses that piece"). Ranked; none started,
   waiting for a pick. Each needs a test that the places stay alike.
   1. Three dialogs (ask, views, protected; now also the bug dialog) built
      apart, their keys, hover, clicks and help each three times. Drift: Tab
      moves between lines in the protected dialog only; ← → skip grayed
      buttons differently; views never opens beside a click; the protected
      dialog's `?` help is never shown. One `src/tui/dialog.rs`.
   2. Key hints and help sentences written apart; "?: what it does" in menus
      and dialogs, "?: what can I do here" on screens.
   3. Marks ✓ ✗ ↻ and their colours in about eight places: green and red in
      the review, plain in the patches pane; `MARKS` kept in step by hand.
      One `src/marks.rs`. To settle: colour everywhere, or plain everywhere.
   4. Phabricator statuses named by hand in `protected.rs` and `submit.rs`,
      not by `Status::name()`: "is Accepted on Phabricator" but "is Landed".
   5. "Submitted after fetched" said twice, in opposite orders.
   6. Choices drawn three ways: `Effort [high]` in a menu, `Effort: [high]`
      in the dialog.
   7. Pane headers painted `color::file()` in some places, `header()` in
      others; seen only once `[colors] header` is set.
   8. stacked web and StackED copy their palette, dim, notch, cursor and
      modifier bits; they differ in the largest size (400×200 against
      1000×500), the `$EDITOR` message and the wheel.
   9. "X is grayed: why" phrased inline about ten times; the ask dialog
      finds a button's reason by matching its text.
   10. Dropping a run waiting its turn built three to five times: "Let it
       run" for the agent, "Let it go on" for the linter and the try push.
   11. Menu labels as literals ("Move HEAD…" three times, "Edit this patch in
       $EDITOR" three times); the tour picks items by label prefix, so a
       rename breaks it silently.
   12. Keeping the cursor in sight written eleven times.
   13. The list beside its detail written four times.
   14. `and()`, `plural()`, capitalizing written four to six times.
   15. Confirm questions built two ways.
   16. A grayed or picked item's style computed four times.
   17. Box sizes and places inline.
   18. The `< X >` button frame written three times.

7. **Remove promoting a comment to the stack** (2026-11-09: "Promoting a
   comment to the stack. Remove this feature. Permission is enough. Moving is
   disturbing, the comment disapears.").
   - **Today:** a patch-change ask whose work would not fit its patch (the
     patches above would not apply, per `stacked fits`, or another patch must
     change too) changes nothing and answers `promote` with why (the prompt
     in `src/ask.rs`, the answer schema's `promote`). Accepting the reply
     moves the comment from the patch to the stack (`ask::promote`,
     `held.rs`, `Reply.promote`; `Note.was` keeps where it was, and
     `ask.rs` can move it back). The comment leaves the patch the user put
     it on.
   - **The change:** no promotion. The comment stays where it is; the user
     asks again with the permission that fits (change the stack). To settle
     when built: what the agent answers instead (a reply saying the work
     needs the stack, with why, the comment left open), and what becomes of
     comments promoted already (`Note.was`: left where they are, or moved
     back once).

8. **"File a bug" on the stack's comments** (2026-11-09: "Is it offered on
   the stack?", then "Backlog it").
   - **Today:** offered only on a comment on a patch: `ask::can_bug` refuses
     a stack comment (`!n.on_stack()`, `src/ask.rs`), and the dialog for a
     comment being written offers it only on a patch (`written_bug`,
     `src/tui/ask_dialog.rs`). The flow ends by putting `Bug N - …` in a
     patch's title, which a stack comment has none of.
   - **Proposed:** offered on a stack comment when your request says the
     word bug, as on a patch; the agent drafts a bug for the stack; closing
     the review opens Bugzilla's form, and nothing more (no number asked, no
     title changed), as for a patch whose title names a bug already.
   - **To settle:** whether the draft links the stack's patches' bugs
     (`see_also`, `blocked`), and whether the number is asked after all to
     reply on the comment.

9. **Accept or reject the bug to file** (2026-11-09: "When reviewing "File a
   Bug", I should be able to accept/reject "the bug to file".").
   - **Today:** the agent's draft lands for review as a section to read only,
     `the bug to file, to read`, its fields in the right pane, with nothing
     to accept or reject (`Held.bug`, `src/tui/reviewing.rs`). Closing the
     review always opens Bugzilla's form (`src/tui/bug_dialog.rs`).
   - **The change:** the section is an item marked accept or reject, as a
     review's other items are. Accepted: closing opens the form, then what
     follows today (the number asked and the title changed, or form only).
     Rejected: no form opens and nothing follows; the review closes as
     rejected for the bug, or sends it back with a comment, as the review
     already lets for other items.
   - **To settle:** whether the review can close with the bug left unmarked
     (as other items, no), and whether `stacked review --accept-all` takes
     the bug (it opens no form on the command line today).

10. **Remove egui; File moves into the `< stacked >` menu** (2026-11-09:
    "Remove egui. Move "File" in < stacked > where "Configure" is.").
    - **Today:** StackED is an egui window (`src/app.rs`, `src/app/menu.rs`,
      `src/app/windows.rs`, `src/launcher.rs`, `src/icon.rs`) with a File
      menu of its own: New…, Open…, Open Recent ⏵, Close Window, Quit
      StackED. The `< stacked >` button on the top bar offers Configure.
    - **The change:** the egui app and its dependency go. The File items
      join Configure under `< stacked >`, from the shared menu builder, in
      the TUI and in stacked web. The docs follow
      (`docs/ai/in-a-window-of-its-own-stacked.md`, `install.md`,
      `layout-of-the-code.md`, `tests.md`).
    - **To settle:** which File items still make sense without a window of
      their own (New…, Open… and Open Recent in a terminal or a browser
      tab; Close Window and Quit as one Quit); whether anything replaces
      StackED's launcher and icon on macOS and Linux; and that StackED's
      state directory is left alone, as the user's.

11. **A referenced comment previews instead of jumping** (2026-11-09:
    "Jumping to an attached comment breaks the workflow. Instead, show a
    preview window like attached code.").
    - **Today:** Enter on a reference to a comment (`r1 ↗ patch 2 · a.txt:
      …`) leaves the comment being read or written and goes to the other
      one, opening it if closed (`docs/ai/writing-comments.md`). Enter on
      a reference to lines opens a preview over the screen instead
      (`Mode::Preview`, `Preview` in `src/tui.rs`), which Esc closes,
      leaving you where you were.
    - **The change:** a comment's reference opens the same preview, through
      the same builder: the comment's turns as it stands (closed, as it
      was last), with its patch and file line at the top. Esc closes it
      and you are back where you were, the comment being written
      untouched. The wheel and scrollbar treat it as the lines' preview.
    - **To settle:** whether the preview still offers a way to go to the
      comment (an item or a key in it), and what the same reference does
      in the agent's reply waiting for review and in a menu naming several.

12. **Watch landing** (2026-11-09: "Offer "Watch landing". So that we can
    monitor Accpted -> Close -> Landed.").
    - **Today:** a patch's letter shows its revision's status as last
      fetched, `[A]` Accepted, `[L]` Closed (landed)
      (`docs/ai/the-editor.md`), and changes only when the stack is fetched
      again. Nothing follows a landing once it is asked on Lando. A try push
      is followed on Lando and Treeherder until it is done, then its
      summary waits for review (`follow`, `docs/ai/layout-of-the-code.md`).
    - **The change:** a "Watch landing" item, in the Phabricator submenu
      (`phab_menus.rs`, the same for the stack and for a patch), offered
      for Accepted revisions. It follows each revision from Accepted to
      Closed to Landed, in the background, the way a try push is followed,
      read-only: no write to Phabricator or Lando. The letter and the
      Phabricator line show each step as it is seen, a notification (and
      the bell in stacked web) says when a revision lands or the landing
      fails, and the watch stops by itself then.
    - **To settle:** what "Landed" is past Closed (Lando's landing job done
      on autoland, or merged to mozilla-central), the step it shows between
      them, and whether it needs a new letter; how often it reads, keeping
      to the rate limit (the 406 of 2026-11-04); whether the watch outlives
      quitting stacked; and whether a backout is watched too.

13. **Build and test on a workspace over ssh** (2026-11-09: "Allow build
    and test to run on ssh remote workspace.").
    - **Today:** a build and test always runs in the stack's build worktree
      on the machine stacked runs on, `.stacked/NAME/build`, kept for the
      next build to build on (`docs/ai/asking-the-agent.md`). The agent may
      run any command there.
    - **The change:** a setting names a remote workspace over ssh (a host
      and a folder there). The build and test sends the patches up to the
      comment's there, the stack's base and patches as the remote checkout
      takes them, and runs the build and the tests there, its output in the
      ask's stream and what it found coming back as today: in the reply and
      on each patch's title. The remote build folder is kept for the next
      build, as the local one is. Nothing runs in the remote user's own
      checkout.
    - **To settle:** where the agent runs (on this machine, its commands
      sent over ssh, or on the remote machine itself); how the patches get
      there (a git bundle, a push to a branch of the remote's clone, or
      the diffs applied); whether the setting is global, per checkout, or
      chosen per ask; what happens when the host cannot be reached; and
      whether a try push or a lint may use the same workspace.
    - **Losing the connection** (2026-11-09: "Also consider losing
      connection to the host."): the connection dropping in the middle of
      a build and test, through sleep, a network change or the host
      rebooting. To settle: whether the remote build keeps running on its
      own (in a session that outlives the connection, its output kept in a
      file there), so that stacked reconnects and goes on reading it, or is
      lost; how long stacked retries and how the stream and the status line
      say so meanwhile; how the ask lands when the host never comes back
      (failed, its partial output kept, as a stopped ask lands); that Stop
      still stops the remote build once the host is back; and that a
      restart of stacked finds a remote build still going, as it finds a
      local one by its pid today.

14. **The Phabricator line pinned at the top, as the title is**
    (2026-11-09: "Make the Phabricator status line stick to the top like
    the commit title.").
    - **Today:** the Phabricator line over a patch's message (`Phabricator:
      D12 · Needs Review · in sync · …`) "is not pinned, and scrolls away
      with the rows, the title pinned in its place"
      (`docs/ai/the-editor.md`, the line's rows in `src/tui/revisions.rs`,
      `line_rows`); the title alone stays at the top of the middle as you
      scroll.
    - **The change:** the line stays pinned at the top with the title, as
      the title does, in every view of the middle and in stacked web; its
      mark and its menu (Space) work there as on the line in place.
    - **To settle:** above or below the title; whether it stays one row,
      cut to the pane's width, when it folds over several; and whether the
      age line under it (`fetched N minutes ago`) is pinned too.

## Parked

1. **Comments outside the patch:** on lines of a file the patch touches but
   outside its hunks, and on files the patch does not touch.

2. **Reducing the agent's token use** (2026-11-03: "What can we do to reduce
   token usage in stacked? Discuss only." then "Park it for later").
   - **Measured** on one database over 22 to 30 October: 187 runs cost $113
     (changes 82 runs, $64, median 15 turns; answers 104 runs, $49, median
     7.5 turns). Cache reads dominate (163M): every turn re-sends the
     conversation, 70 to 90 thousand tokens a turn. The first turn is about
     30,000 tokens: claude's instructions, 28 to 34 tools, the checkout's MCP
     servers, CLAUDE.md/AGENTS.md. Read back most: `stacked show` 971 KB in
     215 calls, `git show` 961 KB, `Read` 905 KB, `git log` 572 KB. 474
     refused calls cost about a turn each.
   - **Levers, ranked:** fewer refusals (one plain command per call,
     stacked's commands in the form their rules allow); the essentials in the
     prompt (the patch or hunk, the thread, the stack's list); focused
     stacked output (`--stat`, `--file`); a smaller fixed context (only the
     tools and MCP servers an ask needs); a cheaper model per kind of ask;
     cost shown beside the effort; one session across rounds; batching
     related comments. The first four first.

3. **The pid reuse gap** (2026-11-03: "Park the 'Fix the pid reuse gap'").
   A run's process is judged alive by its pid alone (`ask::alive`). If it
   dies while no front end runs and the system gives its pid to another
   process first, the run looks alive: a pull holds its stack with no way to
   drop it, an ask or a lint job looks running. The fix: store the process's
   start time with its pid and judge it alive only if both match (macOS
   `proc_pidinfo`, Linux `/proc/PID/stat` field 22).

4. **Colours of a suggested edit's diff in a thread** (2026-11-03: "Park the
   preview for later"). A pulled suggestion is a ```diff block in its
   comment, its `-` and `+` lines drawn in the code's one colour. The offer:
   the diff view's removed and added colours, previewed first with a script
   run in the user's own terminal.

5. **Esc without the keyboard, in the mouse workflow** (2026-11-04: "For
   mouse workflow, we are missing something to avoid pressing ESC", then
   "Park the ESC issue with mouse flow."). In stacked web and StackED,
   leaving a full screen (History, Runs, Versions, a review, an interdiff
   screen, `?`) needs the key. The options: the bottom line's key hints
   become buttons, a click pressing that key (recommended); the mouse's back
   button as Esc, as a complement; a "‹ Back" button at the left of the top
   bar (now `< stacked >` sits there).

6. **The reconnecting tab's two edge cases** (2026-11-04: "Park the two
   reconnect edge cases"). stacked web quits once a page said it goes (the
   `pagehide` beacon) at least 2 s ago, no events stream came since, none is
   open, and one ever was (`src/web.rs`).
   - A tab dropped silently (the laptop slept), then another closed, quits,
     though the first would come back. Fix: a stream that ends without the
     beacon counts as a tab that may come back.
   - A reload slower than 2 s quits: only the new page's events stream
     cancels the beacon, not the page and its wasm, which come first. Fix:
     any request with the token cancels it.

7. **The openpty close-on-exec race** (2026-11-04: "Park the openpty
   close-on-exec race"). `open()` in `src/pty.rs` calls `libc::openpty`, then
   sets `FD_CLOEXEC`; another thread spawning a process between the two
   leaves it the pseudo-terminal's descriptors until it exits. Small: the
   editor is still hung up on by signals and its exit found by `try_wait`.
   Ways out: `posix_openpt` with `O_CLOEXEC` (macOS may refuse it there); one
   process-wide lock around `openpty` and every spawn; `posix_spawn` with
   `POSIX_SPAWN_CLOEXEC_DEFAULT` (macOS only).

8. **The pull from Phabricator puts unwanted sections in the message**
   (2026-11-06: "Import from phabricator adds sections in the message bedy
   that we don't want: "Summuray, Test Plan, Reviewers, Subscribers, Bug
   #:""). None of those words is in stacked's source, so they come from what
   Phabricator returns, its commit-message template, not the message
   moz-phab sent (`local:commits` holds that one). To build: find which call
   the pull takes the message from; take the title, the summary and the
   `Differential Revision:` trailer only, or `local:commits`' message; a test
   with a stored template-shaped answer.

9. **No submit for a revision you did not author** (2026-11-06: "Do not
   offer "Submit to Phabricator" when user is not the author."). The submit
   item (`submit_item`, in the shared `phab_items`,
   `src/tui/phab_menus.rs`) absent when the revision's author is not the
   user. Needs the author's PHID stored with the revision's state (from the
   fetch and pull searches) and the user's own (`user.whoami`, read once and
   stored, never at Send). To settle: absent ("do not offer") or grayed with
   why; a patch with no revision yet is always offered.

10. **The import from Phabricator should pull comments too** (2026-11-06:
    "Import from phabricator should pull comments too."). Against the spec:
    `docs/ai/phabricator.md` says a pull into an empty stack, and
    `stacked moz-phab import`, bring the comments with the patches. So an
    import that brought none is a bug, or brings only some kinds (inline or
    general, done ones). To build: which revision and which comments were
    missing; reproduce on a copy of the database with stored answers.
