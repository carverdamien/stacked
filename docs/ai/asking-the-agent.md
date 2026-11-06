# Asking the agent

The agent (claude, unless the settings name another) only works on
comments: write one where it belongs, read it again, then ask. Where a
comment is, is what the agent may change:

- A comment on the stack: the whole stack. Rebasing, reviewing, testing and
  adding patches are stack comments: `rebase onto origin/main`, asked.
- A comment on a patch, as a whole or on a line: only that patch, which the
  agent may split in place. Its worktree is the stack up to that patch;
  the patches above it, it may read with `stacked export`, not change.
  Unless it is asked of the whole stack (below): when you know already that
  the answer needs more than the patch (another patch changed, a new one on
  top), as a comment on the stack would.

Every item that asks the agent opens the ask dialog rather than asking
from the menu: a comment's `Ask agent…` (`Ask agent, answer only…` on a
question on the history), `Ask agent about all…` on the left header (the
stack's comments) or on the comments pane's header (each comment the list
shows), the comment editor's `Ask agent…` (above), and, in the review of
the agent's work, `Send it back to the agent…`, whose dialog has Send Now
and Cancel for its buttons (What the agent brings back waits for your
review, below). Its title says what is asked about; under it are three
lines, then three buttons:

```
┌─ ask the agent about the comment on patch 1 · a.txt: TWO ────┐
│ Permission: [ — ]                                            │
│   Not set yet: ← or → sets answer only, then steps to change │
│   patch and change stack. Enter opens the list of them.      │
│                                                              │
│ Model: [ — ]                                                 │
│   Not set yet: ← or → sets claude-haiku-5-5, then steps to   │
│   claude-sonnet-5-5, claude-opus-5-5 and claude-fable-5-1.   │
│   Enter opens the list of them.                              │
│ Effort: [ — ]                                                │
│   Not set yet: ← or → sets low, then steps to medium, high,  │
│   xhigh and max. Enter opens the list of them.               │
│                                                              │
│ < Ask Now >   < Ask Later >   < Cancel >                     │
│                                                              │
│ Ask Now and Ask Later are grayed until the permission, the   │
│ model and the effort are set.                                │
└──────────────────────────────────────────────────────────────┘
```

The three lines are unset as the dialog opens, every time, but for the
model and the effort the settings name for it to open with
(`agent.ask_model`, `agent.ask_effort`: Settings, below), which are set
as it opens and changed there as any: nothing set in a dialog is kept for
the next one, for the stack or between runs, so that no ask goes as the
one before it went by mistake. On a line, ← or → sets its first option,
`answer only`, `claude-haiku-5-5` or `low`, whichever the arrow, so that
a stray key never sets the most permissive permission, the most expensive
model or the most expensive effort; then → steps to the next option and ← to the one
before, and neither goes round past either end, as the menus' dropdowns
do: ← held ends on the first, and → held on the last.

Enter on a line opens the list of its options, every one the line offers
here, in their order, just under the line and over the dialog, framed as
a menu is, the cursor on the one set, or on the first while the line is
unset:

```
┌─ ask the agent about the comment on patch 1 · a.txt: TWO ────┐
│ Permission: [ — ]                                            │
│   Not set ye┌──────────────┐nswer only, then steps to change │
│   patch and │ answer only  │ter opens the list of them.      │
│             │ change patch │                                 │
│ Model: [ — │ change stack │                                 │
│   Not set ye└──────────────┘claude-haiku-5-5, then steps to  │
│   claude-sonnet-5-5, claude-opus-5-5 and claude-fable-5-1.   │
│   Enter opens the list of them.                              │
│ Effort: [ — ]                                                │
│   Not set yet: ← or → sets low, then steps to medium, high,  │
│   xhigh and max. Enter opens the list of them.               │
│                                                              │
│ < Ask Now >   < Ask Later >   < Cancel >                     │
│                                                              │
│ Ask Now and Ask Later are grayed until the permission, the   │
│ model and the effort are set.                                │
└──────────────────────────────────────────────────────────────┘
```

In the list, ↑↓ go from option to option, round past either end as in a
menu, and Home and End go to the first and the last; Enter sets the line
to the option the cursor is on, and Esc leaves the line as it was. Either
closes the list alone: the dialog stays open, the cursor on that line.
While the list is open, the other keys of the dialog do nothing; the
bottom bar and `?` say what Enter and Esc do there. ↑↓ go from line to
line and to the buttons, where ← → go from one button to the next, past
those grayed, and Enter presses the one the cursor is on. A click on a
line opens its list, as Enter does, and a click on an option of the list
sets the line to it; while the list is open, a click anywhere else closes
the list alone, as Esc does there, and the dialog stays open. A click on
a button presses it, and a click out of the dialog cancels it, as Esc and
Cancel do: nothing is asked, and from the comment editor you are back to
writing the comment. Ask Now and Ask Later are grayed until the three
lines are set, and the dialog says which ones are missing, in the lines'
order (`grayed until the model and the effort are set`). Ask Later puts the ask
in the stack's batch (below). Ask Now is grayed as well while the agent
cannot be reached, and the dialog says why:

- `Ask Now is grayed: claude is not found on the PATH.` The program the
  asks run as claude (`$STACKED_CLAUDE`, else `agent.command`, else
  `claude`) is looked for as the dialog opens: a name in each folder of
  the PATH, a path as it is.
- `Ask Now is grayed while stacked checks that the network reaches
  api.anthropic.com.` As the dialog opens, stacked makes a TCP connection,
  away from the screen, to where claude connects first: the proxy that
  `HTTPS_PROXY` (or `https_proxy`) names, when one is set; else the host
  of `ANTHROPIC_BASE_URL`, when it is set; else `api.anthropic.com`, port
  443. Nothing is sent over it, and it is given 2 seconds. What it found
  holds for 30 seconds, so that dialogs opened one after the other do not
  check again.
- `Ask Now is grayed: the network does not reach api.anthropic.com.` No
  connection was made in time. Ask Later still puts the ask in the batch.
  Where the check is wrong though claude works, `stacked config
  agent.reach_check false` turns it off, as the dialog says too.

The tour checks neither: its agent is a stand-in, in memory.

The permission, what the agent may do, offers what applies where the
agent is asked, in this order; the line under the permission says what
the one set lets the agent do. Of the comments listed (`Ask agent about
all…`), those that can be asked so are asked: that line says how many,
and Ask Now and Ask Later are grayed while none can be.

- `answer only`: read-only, enforced by its tools: it reads the code and
  the stack, and, unless its list says otherwise (`agent.answer_tools`,
  below), it may read the web with `curl` (a bug on Bugzilla, a revision on
  Phabricator, a page of documentation), through `jq` if need be. To read,
  stacked gives it `Read`, `Grep`, `Glob` and `LS`; the commands that only
  read: `grep`, `rg`, `cat`, `head`, `tail`, `wc`, `ls`, `find`, `cut`,
  `tr`, `stat`, `which`, `dirname`, `basename`, `realpath`, `diff`, `cmp`
  and `comm`; git's commands that only read: `git show`, `log`, `diff`,
  `ls-files`, `rev-parse`, `blame`, `cat-file`, `ls-tree`, `merge-base`
  and `describe`; and stacked's commands that read the stack. The options
  of these that write a file or run a program are refused, whatever its
  lists allow: git's `--output`, and, as to every ask but a build and
  test, `find -exec`, `rg --pre` and `rg --hostname-bin`. It is not given
  `git grep`, `sort` or `file`, because their short options can be
  bundled: `sort -uo FILE` is `sort -u -o FILE`, which overwrites FILE, and
  `git grep -nOCMD` runs CMD as its pager, so that no rule can refuse the
  option that writes a file or runs a program. It searches with `grep`,
  `rg` and `Grep` instead. Should its list give them all the same, their
  options that write or run a program are refused where they stand alone:
  `sort -o`, `file -C` (`--compile`), `git grep -O`
  (`--open-files-in-pager`, however shortened). Nor is it given `sed`,
  `awk`, `uniq` or `tree`, which can write files or run programs however
  they are called. The review of its work never offers it any of these
  (The tools the agent could not use, below). It may
  also use what your own claude settings allow, and what its list and
  stacked's `agent.tools` add (the stack's checkout's own, if it sets them:
  Settings, below), your MCP servers' tools among them. What it was not let use
  is said in its stream (`⛔ not allowed: mcp__moz__get_bug: …`) and in its
  run's summary, and each tool it could not use is an item of the review of
  its work, to allow for this work or from now on (The tools the agent
  could not use, below); every ask, a change's too, is told not to stop at
  a refusal but to work around it, to say so in what it writes, the tool
  not allowed, never that the service behind it (Bugzilla, Phabricator)
  refused, and to say why it needed each tool and how it worked around
  it. It works in a worktree
  of the stack's own for answers (below), never in your checkout, the stack
  up to its place applied, as a change's worktree is: it reads the patches
  there with `git show`, and the whole stack with `stacked export`. Only the
  reply comes back. An
  answer may open comments of its own on lines within the place, as a
  review does, each about one issue (a line with two issues gets two
  comments, both on it): the asking comment then says `opened 4 comments, 1 with your
  replies`, and its menu goes to them, or closes it and them. A question
  on the history has this permission alone.
- `change patch`, on a patch's comment: it may change the code within the
  comment's place, its patch and no other; its reply says what changed.
  It may run the checkout's linter on the files it changes, to check them
  (Linting, below).
- `change stack`: a change, with the whole stack in its worktree: it may
  change any patch, add patches anywhere, reorder them. It is how the
  stack's own comments change the stack; on a patch's comment, the comment
  stays on its line, and its reply goes in its thread. On the command
  line, `stacked send NAME ID --whole-stack`.
- `build and test`, on the stack's comments and a patch's as a whole: it may
  run any command, in the stack's build worktree (below), by its own rules
  (CLAUDE.md), never the user's worktree; it changes nothing in the stack.
  What it found comes back in the reply and on each patch's title, saying which
  stack it tested.
- `try push`, built with the `try-push` feature, on the stack's comments
  and a patch's as a whole, as `build and test` is, never on a line or a
  file, nor on the history: it selects the CI tasks the comment asks for,
  for a try push of the patches up to the comment's (all of them, for the
  stack's), as few or as many as the comment says, each by its exact label
  and with why it is relevant. It reads as an answer reads, where an
  answer works, and is given what an answer is given and its own list
  (`agent.try_tools`); it pushes nothing and changes nothing. What it
  selects waits for your review, and stacked pushes what you accept (Try
  push, below).
- `file a bug`, built with the `moz-phab` feature, on a patch's comments
  (as a whole or on a line) when the patch's title names no bug as
  moz-phab reads one (`links::bug_in`), or when the comment says the word
  bug (`bug::mentioned`): in any case, as a word of its own or its plural
  (`bug`, `Bug:`, `BUG`, `follow-up bug`, `bug 123`, `bug123`, `bugs`),
  not inside another word (`debug`, `bugzilla`, `humbug`), in any turn of
  its thread that stands, yours, the agent's or a reviewer's, and for a
  comment being written, in what is typed so far (`ask::can_bug`, and
  `written_bug` in `tui/ask_dialog.rs` for one being written). Never on
  the stack's comments, the history, nor in the tour; absent from the list
  otherwise. The title naming no bug, it drafts the bug the patch is for;
  naming one, it drafts another one, a follow-up, as the comment asks, the
  prompt saying the title's bug and that `see_also`, `blocked` or
  `dependson` may name it where that fits. It drafts as Firefox's own
  bug-filing skill does, its rules
  in the prompt in stacked's words: the product and the component from
  `./mach file-info bugzilla-component FILE` (the closest sensible one for
  `UNKNOWN`); the type, `defect`, `enhancement` or `task`; the summary
  shaped to the type (the symptom, the capability wanted, the action to
  take), in ordinary words, no counts, no coined terms; the description
  with identifiers in backticks, and the test's dashboard link
  (`https://tests.firefox.dev/test.html?test=PATH`) for a test failure. It
  reads as an answer reads, where an answer works, given what an answer is
  given and `Bash(./mach file-info bugzilla-component *)` besides, the
  answers' lists of the settings (`agent.answer_tools`,
  `agent.answer_accepted`) its own; it files nothing, runs no
  `file-bug.py`, opens nothing. It answers with its replies and `bug`:
  `product`, `component`, `bug_type`, `short_desc`, `comment`, and, when
  the bug needs them, `blocked`, `dependson`, `see_also`. The draft lands
  for your review, a section `the bug to file, to read`, its fields on the
  right; nothing to accept or reject. Closing the review opens Bugzilla's
  form to file it (`enter_bug.cgi` on `reviews.bugzilla`, each field
  percent-encoded, as the skill's `file-bug.py` makes it) in your browser
  as a link opens (`$STACKED_OPEN`, else the system's opener): from the
  terminal, from `stacked web` (a new tab of your browser) and from
  StackED alike. Its link is long and never shown. Then a dialog, laid out
  as the dialog for patches that should not change: `The form to file the
  bug is open in your browser: Core :: Networking, “…”.`, `Review it there
  and submit it on Bugzilla, then give the bug's number here, or paste the
  link to the bug.`, a field, and `< Put it in the title >` (grayed until
  the field holds a number: `123`, `#123`, `bug 123`, a `show_bug.cgi?id=`
  link or one ending in the number), `< Open the form again >` and `<
  Cancel >`; Esc, Cancel or a click out of it change nothing, the form
  staying open in the browser. The keys go to the field as typed, `?`
  too. Put in the title, the patch's title becomes `Bug N - title`, as
  moz-phab writes it: a new version of the patch, its message alone
  changed, written at once as any change of the stack, refused while the
  agent changes the stack or once the title names a bug, and asked about
  first by the dialog for patches that should not change (`Db::guard`).
  The next submit sends the bug. The title naming a bug already, there is
  no dialog and nothing changes: the form alone opens, and the bottom line
  says `The form to file the bug is open in your browser: Core ::
  General, “…”. Patch 3's title keeps bug 1234567.` (`file_bug`). `stacked review` lists the draft (`bug`)
  but opens no form; only the editor's review does.

The model the agent works on an ask with is the dialog's second line, by
its full id, as claude's `--model` takes it, in this order, weakest to
strongest as the efforts run low to max: `claude-haiku-5-5`,
`claude-sonnet-5-5`, `claude-opus-5-5`, `claude-fable-5-1`; the list is
`Model::ALL`, in one place. There is no
default: it is picked in the dialog, or set already as it opens by the
settings' `agent.ask_model`. How hard the agent works on an ask is its
effort, as claude's `--effort` takes it: `low`, `medium`, `high`, `xhigh`
or `max`, the dialog's third line, picked there too, or set already as it
opens by the settings' `agent.ask_effort`. What sends again what was
asked before ends with the model it goes with and the effort it goes at,
`Model [claude-opus-5-5]    Effort [high]`: `Send it now` and the batch's
items (below), each with the model and at the effort it was asked with
unless others are picked there; one asked before models were picked, with
the settings' `agent.ask_model`, else the strongest, `claude-fable-5-1`,
there. On an item with choices of its own, Tab goes to the next
dropdown, shift-Tab goes back, and ← → pick in the one the menu is on. Neither goes round
past its end: ← held ends on the first, and → held on the last. A click
on `Model […]` or `Effort […]` picks the next, round from the last to
the first. What is picked there is that item's alone: no other ask starts
with it, and nothing is kept between runs. The items that open something
to write first, `Ask agent about this, answer only…` on the history and
`Ask the agent to make a stack…`, pick neither: the dialog of what you
write there does.

An ask goes with the model and at the effort set for it, whenever its
turn comes: the comment keeps them while it waits, ready or in the batch,
and the ask keeps them once it starts. A record keeps the model by its
id, not as one of the list, so that one of a model since taken out of the
list still loads and goes as it was asked. claude is given `--model ID`
then `--effort LEVEL` after the settings' `agent.args` (and `agent.model`),
each in place of one they give, and the stream's first lines say so
(`starting: claude -p … --permission-mode acceptEdits --model
claude-opus-5-5 --effort high, in …`). A comment in the batch keeps the
model and the effort it was asked with: its `Send it now` starts with
them, and each of the batch's items at the highest effort of the comments
it sends, with the strongest of their models. An ask asked again
goes with the model and at the effort it had: a change dropped because
the work it went on from was rejected, an answer restarted as a change is
accepted, a comment put back on the queue as it was. Work sent back from
its review goes with the model and at the effort its dialog sets, as
every ask's are, whatever the work was asked with; from the command line,
it goes with the model and at the effort it was asked with, unless
`--model` or `--effort` says otherwise (below). An ask queued by an older
stacked has no model nor effort of its own: claude is given no `--model`
nor `--effort` of the ask's, and it goes as the agent's default, or the
settings', say.

What the agent brings back says the model and the effort it was asked
with wherever the agent or its work is named, so that work done at `max`
is told from work done at `low`, and work done by one model from another's.
Each turn of the agent's in a thread is headed with them, the model first,
`agent(claude-opus-5-5, max):`, in the middle, in the previews of the
comments pane, in a review, in the browser and in StackED; the comments
pane says `agent(claude-opus-5-5, max)` as who opened a comment the agent
opened, and before the agent's last word (`agent(claude-opus-5-5, max): It
is a shout.`), and the comment's head says `agent's comment
(claude-opus-5-5, max)`. A turn is named with them (`Edit the agent's
reply (claude-opus-5-5, max)`, `the agent's reply (claude-opus-5-5, max):
edit it`, `you edited the agent's reply (claude-opus-5-5, max)` in the
comment's history), and a comment's state says them after how it was
asked, the effort first (`with the agent (answer only, at max with
claude-opus-5-5)`, `agent replied (answer only, at max with
claude-opus-5-5)`, `the agent's reply (claude-opus-5-5, max) waits for
your review`). The banner says them of what landed, before what was asked
(`⟳ Review 1 reply, asked at max with claude-opus-5-5: "why so loud?"`),
the review's title after whose work it is (`the agent's work
(claude-opus-5-5, max): 1 reply · asked "why so loud?"`), as the menu of
a comment its reply waits on names it (`Review the agent's work
(claude-opus-5-5, max)…`), `Runs…` and `stacked runs` after what each run
is (`answer(claude-opus-5-5, max) s "why so loud?"`), `stacked landed`
after what is left of the work to review, a column of its own (`asked at
max with claude-opus-5-5`), and `stacked comments`, the search and the
history's comments as the editor heads the turns (`agent(claude-opus-5-5,
max): It is a shout.`); the transcript's first lines give them as claude
was given them (`--model claude-opus-5-5 --effort max`). Either alone is
said alone: `agent(max)`, `asked at max`, for an ask of before models were
picked. The model and the effort go with each turn, on its fence (One
file, only appended to, below): a reply or a comment the agent opened says
them however long its work waits for review, and once accepted; so does a
reply that moves a comment to the stack's comments, and a try push's
replies, with the model and at the effort its tasks were selected with. A
turn written before efforts were picked, or by an ask that went at the
agent's own, says nothing of them, and shows as it always did. What the
agent is told of the earlier turns of a thread says none of it either.

The heading of each turn an answer-only ask brought back, its reply or a
comment it opened, also says how old the stack it was asked on is, while
the thread is open and the stack has changed since: `agent(max), on the
stack 2 edits ago:` (`on the stack 1 edit ago` for one). The edits are
counted along the tree of edits (The tree of edits, below), from the state
the answer was asked on to the state the stack is on now. Once the stack
is on a state that was not edited from that one (that edit undone, say,
and another made in its place), the heading says `agent(max), on another
version of the stack:`. On the stack it was asked on, it says nothing
more; nor does a closed thread, a turn of any other ask, a change's, a
build and test's or a try push's, nor one written before. Only the heading
in the thread says it, in the middle and in a review's right pane: the
comments pane, the search, the history, `$EDITOR` and `stacked comments`
say who wrote each turn as ever. So an answer kept as a change of the stack
is accepted (below) shows that it answered the stack as it was, for you to
ask it again on the stack as it is if need be. The stack an answer was
asked on goes with its turns, on their fence (One file, only appended to,
below).

On the command line, `stacked send` takes `--effort LEVEL` and `--model
ID`. Without `--effort`, an ask goes at `high`; without `--model`, it
goes with no model of its own (the settings' `agent.model`, else claude's
own); the dialog's settings, `agent.ask_model` and `agent.ask_effort`,
are the dialog's alone. `--batch` sends each comment with the model and
at the effort it was batched with, an ask of several at the highest
effort of theirs and with the strongest of their models.
`stacked review --send-back` takes `--effort` and `--model` too; without
them, the work goes back with the model and at the effort it was asked
with. A level claude does not take, or an id that is not one of the
dialog's models, is refused, with those it takes said, and nothing is
sent.

What each kind of ask may use is stacked's to give and yours to add to.
stacked always gives each what it needs: a change, git's history commands,
the commands that read, and its own `stacked fits` and `stacked export`,
none of which reaches the network, so that a change cannot touch a remote
(below), and the checkout's linter, when it has one, to run on the files
it changes (Linting, below); an answer, and a try push's selection,
`Read`, `Grep`, `Glob`, `LS`, the commands that only read and git's that
only read (`answer only`, above), and stacked's commands that read the
stack; a build and test, any command. What else
each may use is a list of
the settings' (Settings, below): `agent.change_tools` for the asks that
change the stack, nothing unless set; `agent.answer_tools` for answer-only
asks, `curl` and `jq` unless set, as answers could always run them before
there were lists; with the `try-push` feature, `agent.try_tools` for the
asks that select a try push's tasks, `curl` and `jq` unless set; then
what the reviews of the agent's work allowed from now on for that kind of
ask, a list of its own (`agent.change_accepted`, `agent.answer_accepted`,
`agent.test_accepted`, `agent.try_accepted`: The tools the agent could not
use, below); and `agent.tools` for every ask, builds and tests too.
`stacked setup agent` sets those lists up with you, as one list of tools
with a box for each kind of ask. Every ask is refused, too, what the stack's
checkout's claude settings of yours deny (`permissions.deny` of
`.claude/settings.local.json`), wherever it works: claude refuses it in the
checkout, and stacked has it refused in the agents' worktrees too, which do
not have that file, as git does not track it (`--disallowedTools`). What
the tracked `.claude/settings.json` denies is in the worktrees already.
Every ask but a build and test, which runs what the project needs, is
refused what would make the commands it may run run another program or
delete files (`git rebase --exec`, `find -exec`, `find -delete`, `rg
--pre`, `rg --hostname-bin`, `sort --compress-program`, however shortened,
`--co`), whatever its lists let it run. An answer and a try push's
selection are refused, too, the options that make what they are given
write a file or run a program (`answer only`, above).

No ask waits for a setup: the first goes at once, with stacked's defaults
(`curl` and `jq` for answers and for a try push's selection, nothing more
for changes), and what an ask could not use lands for your review with its
work. `stacked setup agent` sets the lists up with you whenever you want
it to. A settings' file of a stacked of before that says the setup was
done (`agent.setup`) is read all the same, that setting passed over:
`stacked config --unset agent.setup` takes it out. The tour's stand-in for
the agent needs no list.

Whatever it writes, a reply or a comment it opens, the agent may attach
references to it: lines of a file, a union of ranges (`120-140,200-210`),
or the whole file, as a patch leaves the file, as the stack's base has it,
or as the whole stack leaves it, which reaches the files the stack does
not touch too. Under the comment, a line for each: `⇢ a.txt:2-3, 10 · as
patch 1 leaves it`, `⇢ a.txt · as patch 1 leaves it` for the whole file,
which previews whole, its numbers plain. Enter on it previews those lines in a box, numbered, `⋯` between the
ranges; Enter again goes there: to the line in its patch, when the patch's
diff has it, else to the file whole, at that line, as it reads (Esc comes
back). In the box and in the file whole, the numbers of the lines it
points at are marked, drawn reversed. They are kept in the agent's turn as a `refs` block, a reference a
line (`a.txt:2-3,10 @1`; `a.txt @1` the whole file; `@0` the base, no `@`
the whole stack; a web link as it is, which is never read as a path), which
`stacked comments` shows and the agent reads back in its thread. The agent
may point at another comment of the stack too, by its label in the ask or
its id as `stacked comments` lists it: under what it wrote, `↗` and where
that comment is, Enter going there. It may point at a page of the web,
`{"url": "https://…"}`: under what it wrote, `⇗` and the link, which Enter
shows whole and then opens, as one you attached. The prompt asks it to
point at the stack first: whenever the code or the comments say what it
means, at those lines of a file, as a patch, the base or the whole stack
leaves it, or at that comment. A web link is for what nothing in the
stack, the repository or its comments says: a bug, a specification, a page
of documentation. The
references it attaches take numbers too, after those of the comment it
writes on, in the order it gives them (the prompt says the first free
one; a comment it opens numbers its own from `r1`), and one it gives that
is no reference (a comment that is none, lines that are none, a link not
of the web) leaves its number out, so that what names the others still
does. In what it writes, and in what you write after, `@r4`
names one of them as `@r1` names one you attached. They go with their
turn when a rewind takes it; when the turn is written again, it keeps
them, but one it named whose last mention the edit took away (Writing
comments, above). A turn deleted by stacked's first way of deleting one
counts nowhere: its references are not the comment's (not shown, not
listed in $EDITOR, not told the agent), and its mentions name nothing.

An ask goes now, or later, as in self-review's batch: the dialog's Ask
Now, or its Ask Later, each time, as nothing of it is kept. Asked later, a
comment waits in the stack's batch, at the permission, the model and the effort its
dialog set, marked `◇` and said `batched (may change the code)`; nothing
is started. Its menu has `Send it now (…)`, alone, as it was batched, and
`Remove it from the batch`; written again, it stays in the batch, to go as
it is then. The banner sends the batch (below), as you pick: its edits to a
single agent, in one ask, of the whole stack when they are on more than one
patch or one was batched so; its answers to a single agent too, in one ask
with the whole stack in view, or fanned out, an agent each, all at once;
each build and test on its own, and each try push on its own too (Try push,
below). Each of these items ends with the model it sends with, the
strongest of those its comments were batched with, and the effort it
sends at, the highest its comments were batched at, unless others are
picked. On the command
line, `stacked send NAME ID --later` batches (how, as `send` says it:
`--answer-only` and the rest), and `stacked send NAME --batch` sends the
batch, the answers an agent each with `--fan-out`, and the try pushes the
editor batched, each on its own.

Every ask works in a worktree of its own, never in your checkout. A
worktree is a checkout of every file, which costs time and space: so the
stack has worktrees of its own, `.stacked/NAME/agent` for changes,
`.stacked/NAME/answer-only-agent` for answers and for the selection of a
try push's tasks, and `.stacked/NAME/build` for builds and tests, beside
yours (`.stacked/NAME/user`), out of git's sight. An ask takes the first
that no running ask is in and that holds nothing uncommitted, moves it to
what it is asked about, and leaves it there when it lands, for the next
ask to take: only what differs is written. One busy, the next ask takes
`agent-2`, `agent-3`. One an agent left work unfinished in is left as it
is, for you to look at; the ask says so when it fails (`it is left in
…/.stacked/NAME/agent`), and the next takes another. Looking at a
checkout, to tell whether it holds anything uncommitted, stacked's git
takes no lock it can do without (`GIT_OPTIONAL_LOCKS=0`): an ask that
starts in a worktree as another ask's start looks at it, or a git command
of yours in your checkout as the editor looks at HEAD, never finds the
index locked by that look. A setting of
`[agent]` (the stack's checkout's own, if it sets it) says when each is
made: `worktree` for a change, and `answer_only_worktree` for an answer,
each `ondemand` or `precreate`:

- `ondemand`, unless set: the stack's own worktree, made the first time an
  ask needs it, and kept until you remove it (`git worktree remove
  .stacked/NAME/agent`).
- `precreate`: the same, made when the editor opens the stack, and each
  stack it switches to, away from the screen, so that no ask waits for it;
  removed when the editor exits, away from it too, but those an agent is
  working in or left anything uncommitted in.

A stacked of before also took `pwd`, your checkout itself. No ask works
there now: a settings' file that still says `pwd` is read as `ondemand`,
and stacked says so where it says what is wrong with the settings
(`agent.answer_only_worktree = "pwd" is no longer supported: asks run in a
worktree of their own; read as "ondemand"`); `stacked config` refuses to
write it. An ask that a stacked of before started in your checkout still
lands, or is found dead, as any other: your checkout is left as the agent
left it, never removed nor cleaned.

A build and test always works in `.stacked/NAME/build`, made the first
time and kept until you remove it (`git worktree remove --force
.stacked/NAME/build`), with what each build made in it, for the next build
to build on: only changes to the files git tracks, which a build should not
leave, keep it from the next build, which then takes `build-2`.

`agent.worktrees` puts the agents' worktrees elsewhere than `.stacked/`: in
that folder, under one of each repository's own.

Asks start one at a time, whichever process starts them: the editor, a
landing ask, which starts what waited behind it, or `stacked send`. Each
start holds the launch lock, a lock of the kernel's (flock) on a file of
its own beside the database (`stacked.jsonl.launch.lock` beside
`stacked.jsonl`), from reading the queue to the spawn of the agents it
starts: what goes is chosen, each ask's worktree is taken, made or moved to
what it is asked about, and the start is written, while no other process
does any of it. Everything else stacked does to its own worktrees holds the
lock too: filing what an ask did in its worktree, landing an ask whose
agent is gone, making the worktrees ahead and removing them, making a lint
job's worktree and a try push's ready, making `.stacked/NAME/user` the
first time you edit there. The lock is never held while an agent works, and
it is apart from the database's lock, so that the agents at work write their
streams meanwhile. It is taken before the database's lock, never under it.
The kernel gives it back however the process holding it ends. A landing
holds it twice: once to read what the ask did from its worktree, made on a
copy of the database, nothing written; then, once what each operation of
its change needs is worked out from that copy with neither lock held (What
the agent brings back waits for your review, below), to file it, as it was
read again under both locks. Landing an ask whose agent is gone, from
within a launch of its own, works that out under the launch lock held.

Several go at once, by self-review's rule: one changing ask at a time on a
stack, answer-only asks together, a build and test once no change runs on
its stack, one at a time of every stack, as builds take the machine (a
build asked on another stack meanwhile says it goes `once the running build
and test of s lands`); a change waiting holds back the answers asked after
it. An asked comment that cannot go yet waits as ready, says what it waits
on, and goes on its own when that lands; `Take it back from the agent's
queue…`, in its menu, and Delete on it, take its ask out of the queue once
asked, the comment staying open (What the agent brings back waits for your
review, below). Nothing is asked on a stack that
does not apply. A lint job of the stack takes its turn as a change does,
the changes asked after it waiting for it (Linting, below); so does a
reconcile with HEAD (The editor, above), the changes asked after it
waiting until it is reviewed; and so does a pull from Phabricator
(Phabricator, below), queued as it is asked, reading Phabricator first,
then made at its turn, under the launch lock but away from the database's
lock, on a copy of the database, and filed, the changes asked after it
waiting until it is made.

Chained (`agent.chain`, on unless set to false), a change
does not wait for the review of the change before it: it goes at once, from
the stack that change makes, as if you accept it, one change running at a
time. What it brings back is reviewed after the work it went on from: the
banner lists it after that work, `◌ … after the edit under review`, and its
review does not open before (`stacked review NAME RUN` refuses too).
Accepted, that work makes it an ask of the stack as any other. Rejected,
accepted in part (some of its operations rejected), sent back or edited
by hand, what went on from it is dropped: its agent is
stopped if it still works, what it brought back is thrown away unseen, and
its comments go back to their turn, to start again from the stack as it
then is (a round sent back goes first). It reads the stack it went on from
with `stacked export NAME --work RUN` and `stacked show NAME --work RUN`,
the stack a landed work makes. An answer-only ask runs at once, alongside
the changes, on the stack as it is. Accepting a change leaves behind the
answers made on the stack before it, still running or waiting for your
review: the review of the change lists them, and you mark each keep,
restart or stop before that review can close with the change accepted
(What the agent brings back waits for your review, below). Kept, an
answer goes on, or waits for your review, as it is, on the stack it was
asked on, and the heading of its reply says how many edits old that stack
is (below). Restarted, it is asked again on the stack the change makes.
Stopped, its agent is stopped, or what it brought back is thrown away
unseen, and its comment stays open, not asked again. Asked again, an ask
whose work had landed is shown that work, to reuse what still holds: its
replies, the comments it opened, and, for a change, what it changed. A
build and test is not asked again: it goes on, of the stack as it was,
unless you stop it as you close the review. Closing a review, or sending
the work back, says first what becomes of the asks it leaves behind, and
offers to stop a build and test going. Unchained, a change waits for the
answers running, and accepting it leaves those waiting for your review as
they are: its review lists none.

While the agent changes a stack, the stack is not edited by hand: the
agent's change is made from the stack as it took it. Nor is it while
other work is about to change it: work you sent back, waiting to go back
to the agent; a lint job of the stack, waiting its turn or running
(Linting, below); a pull from Phabricator of the stack, from when it is
asked to when it is filed, reading Phabricator, waiting its turn or being
made (Phabricator, below), as a pull is an edit too. Nor is it while a
change of the stack waits for your review, as that change was made from
the stack as it is.
The menus' edits of the stack and its patches are off meanwhile, and the
menu says why (`the agent is changing patch 2: the stack can be edited by
hand again once you have reviewed the agent's work`; `a pull from
Phabricator is reading Phabricator (the stack's revisions, with their
parents and those stacked on top): the stack can be edited by hand again
once the pull is made and you have reviewed its change of the stack, if
it makes one`, then, read, `a pull from Phabricator waits its turn (…):
…`, and, its turn come, `a pull from Phabricator is being made (…): …`):
editing a patch or one of its files, editing the stack as a list,
moving, dropping, undo and redo, picking a state from the tree or a
version, deleting the stack. The same commands refuse on the command
line. The work waiting for your review is still edited by hand from its
review, in its place: what went on from it is dropped then, as above, and
a pull made on it meanwhile is made again. Comments are written as ever,
except those the agent works on: their `Reply` and edits wait for the
agent's reply to land, as the reply is to what was asked. An edit by hand
open in $EDITOR when the agent takes the stack, or a pull is asked, is
not lost: a file's edit is kept, to `Edit it again` later; a patch's
mail, or the stack's list, opens again with why at its top (`save again
then, or empty the list to give up`). A comment waiting its turn is off
the agent's queue while you reply to it, and goes back on with your
reply.

To change commits below the top of the stack, the agent writes its rebase
plan (a `pick`, `edit`, `squash`, `fixup` or `drop` line for each commit)
to a file its prompt names, then runs a plain `git rebase -i`. The file is
`$TMPDIR/stacked-todo/…/REBASE_TODO`, one for each worktree, outside the
git directory, which claude may not write in; stacked lets claude write
there (`--add-dir`), has `GIT_SEQUENCE_EDITOR` copy the plan in, and sets
`GIT_EDITOR` to `true`, so that no editor opens. That is how self-review has
its agent rebase. Claude refuses a command that starts by setting a
variable (`GIT_SEQUENCE_EDITOR=… git rebase -i`), as it refuses any command
that no rule allows.

The plan's folder is named after the worktree's git directory: its name is
the first 12 hex digits of the SHA-1 of that directory's path, and a file
in it, `gitdir`, names the directory in full, as the hash cannot be read
back. The folder is removed once the ask lands, failed or not, unless a
rebase is still under way in its git directory, or another ask that is
running writes its plan there. Each time an ask that may change the whole
stack starts, stacked also removes every folder whose git directory is
gone, unless an ask that is running writes its plan there. A folder that
names no git directory, as an older stacked made them, is removed once
nothing in it has changed for a day; so is a try push's selection's
folder, where stacked lists the tasks mach can push and the agent
downloads the task graph (Try push, below), should its ask never have
landed. `$STACKED_TODO` names another folder to keep the plans
in, in place of `$TMPDIR/stacked-todo`.

What a change may run reaches no network, unless what its list
(`agent.change_tools`) adds does, or the checkout's linter does: stacked
ticks nothing in that list's column of the setup, and the setup says
which of the agent's tools can reach the network (Bash at large,
`WebFetch`, `WebSearch`), as a change that reaches it could touch a
remote. It may run git's history
commands (`status`, `log`, `show`, `diff`, `add`, `restore`, `reset`,
`commit`, `cherry-pick`, `apply`, `checkout`, `switch`), `git rebase` with
its plan (`-i`, and `--continue`, `--skip`, `--abort`), git's settings to
read them (`git config --get`, `--list`), commands that read (`grep`,
`rg`, `find`, `cat`, `sed`, `sort` and the like), and the checkout's
linter, when it has one, on the files it changes (Linting, below). It may
not set git's
settings (`git -c`, `git config` otherwise), as one can run a program, and
stacked has claude refuse what would make the rest run another program
(`--disallowedTools`: `git rebase --exec`, `find -exec`, `rg --pre`, `rg
--hostname-bin`, `sort --compress-program`, however shortened), as
claude's own refusals win over what it allows. The
plan is checked before git takes it: one with an `exec` line is refused, and
the rebase is not started. Claude's rules are what it is let do, not a
wall around what it runs: for that, claude's sandbox.

Changes are checked when they land, like any edit. A patch's change slots
into the stack in its place. Before it finishes,
the agent runs `stacked fits`, which says whether the patches above still
apply on its change; if they would not, or the comment needs more than its
patch, it changes nothing and the comment moves to the stack's comments,
with the agent's why as its last turn and `moved from patch 2 · a.txt: TWO`
on its first line, to be asked again from there (asked of the whole stack
from the start, it would not have had to). The cursor goes with it if it
was on it, and the bottom line says so. `Go to where it came from`
and `Move it back to patch 2` are in its menu, the second grayed while the
stack does not hold that patch. Comments on lines follow a
split or merged patch to the piece that has their line. A rebase says onto
what (the agent adds `"base"` to its answer): the stack's base moves there.

When a change lands, stacked reads the agent's commits back as mail (`git
format-patch`) and matches each commit to the patch it comes from: by the
commit stacked made of that patch, if the agent left that commit as it
was; else by the effect of its diff and its title, then by that effect
alone, then by its title alone. A commit matched so is a new version of
its patch, unless it has the patch's message and author and a diff with
the same effect: then it is the patch itself, unchanged, its diff kept as
stored, and no new version is made, whatever date the commit has. Two
diffs have the same effect when they change the same files, in the same
order, with the same paths and modes, in the same hunks, each hunk
holding the same lines, kept, added and removed. What git writes
differently once lines have moved does not count: the line numbers in a
hunk's header, the heading git writes after its second `@@`, the blobs an
`index` line names, and the white space a header's line ends with. So
when the agent rebases the stack to reword patch 2, patch 3 keeps its
version even when a later version of patch 1 moved the lines that patch 3
changes, and git now writes patch 3's hunks with other numbers: only
patch 2 is a new version (`~`). A commit with a line added, removed or
kept that its patch does not have, a file more or fewer, a mode changed,
or its hunks split or merged otherwise, is a new version.

git does not give every message back as stored: `git am`, which puts the
stack into the agent's worktree, keeps no empty line twice in a row, no
white space at the end of a line and no carriage return, and takes a
prefix and runs of spaces out of a title; `git format-patch` quotes a
name that holds a comma or a dot. So stacked also reads back the commits
it made of the patches, as it reads the agent's, and a commit whose
message or author is what git gave back of its patch's has the patch's
own, as stored: the agent left it as it was. The title git gave back
finds the patch as its own does. A message the agent changed otherwise,
if only in its white space, is the commit's: a new version of the patch,
reworded when its diff has the same effect. So when the agent rewords
patch 2 of a stack whose messages have two empty lines under their
titles, the patches below it and above it keep their versions, and only
patch 2 is a new version.

A diff kept as stored is checked before the change lands. git puts a
hunk where the hunk's lines are nearest to the line its header names, so
a diff stored before its lines moved could go to another copy of the
same lines in the file, and a hunk the agent moved to lines like its own
would go back to where it was. So stacked applies the patches it would
land, the diffs kept as stored and the new versions, in turn on the
commit that the agent's commits start from, in an index of the check's
own, nothing written in the repository; beside it, in another such index,
it applies the agent's commits as it read them back. After each patch
whose diff is kept as stored, the files that the patch touches must be
the same in both. A patch whose files are not the same is a new version
after all, its diff the agent's commit's, and the check goes on up the
stack from there. When the check cannot be made, every patch that would
keep its diff as stored is a new version instead. `stacked import NAME
RANGE --replace` matches the commits it imports to the stack's patches in
the same way, their messages and authors compared as they are, as
stacked did not make those commits, and checks the patches it keeps
against those commits on the stack's base.

Where it stands shows on the comment's first line, or under it when that is
too long: ready (and on what), with the agent (and the last lines of its
stream under the comment's turns, over its references: what it says and
runs, and while it thinks, one line
counting it, `✻ thinking · ~688 tokens`, with the words of it when claude
gives them), agent replied (changed, answered, tested), each with how it
was asked, at what effort and with what model (`with the agent (answer
only, at max with claude-opus-5-5)`, `agent replied (change, at max with
claude-opus-5-5)`), failed (and why), moved. Until the
agent writes its first line, one line says what is being run to start
it, as it runs: `starting: git worktree add --detach … BASE` (the
stack's own worktree, made the first time, which takes a while in a big
repository) or `starting: git switch --detach BASE, in WORKTREE` (taken
again), then `starting: git am --keep-non-patch, 3 patches` (the stack, up
to what it is asked about); then `starting: claude -p … --permission-mode
… --model ID --effort LEVEL, in WORKTREE`, the ask's own worktree, whatever its kind:
never your checkout. A try push's selection's wrapper then lists the tasks
mach can push before claude starts, its line saying so in stacked's own
words, no `starting:` before them (`stacked lists the tasks mach can push
from the stack, …`, Try push, below). They stay
at the top of its stream, as `$ git worktree add …`, in the stream, in
`Runs…` and in `stacked transcript RUN`. Enter on a comment the agent is
working on opens its stream in place; esc folds it, and so does the agent
being done, the bottom line saying so. The comments pane marks each (`●` with the agent, `◇`
batched, `◌` ready, `◆` its reply waits for your review, `✓` replied, `✗`
failed, `↑` moved), its header counts them (`4 comments · 1 with the agent ·
1 to review · 2 to read`), and the top bar too (`agent ●2 · batch:3 ·
ready:1`).
`Filter comments…`, on either header and on a patch's menu, picks what
both panes show, each with its count: all, to send, to review, to read
(any, from an answer, from a change), in progress, failed or moved, gone
(on a line the patch no longer has), closed, none. The counts are of the
comments the middle shows, which the comments pane lists, the patch's or
the stack's own, as the menu's title says, from either header as from a
patch's menu, that patch's (`filter comments, counted on patch 3`,
`filter comments, counted on the stack's own comments`). What is picked
is kept per stack, in the file, and filters every patch's comments and the
stack's own alike. Under `closed`, the comments pane lists those closed, as
the middle shows them, its header counting them (`1 closed comment`); it
is there for them though no comment is open, and → reaches it. A comment you are done
with is closed, from its menu (`Close this comment…`): it leaves the
comments shown. When the cursor is on the comment closed, the cursor goes
to the line the comment was on (a line of the code or of the message, the
file's name, or the patch's title), on the same line of the screen; a
comment on the stack, or on a line the patch no longer has, leaves it on
the row over the comment. A comment that `Filter comments…` hides does
the same. When
a comment is closed from elsewhere (the comments pane, the banner, another
stacked), the cursor stays where it is, as it does whenever other comments
come, go or change: hidden, moved to the stack, answered, written on, or
opened again. The comments pane then picks the next comment, or the one
before it when the comment closed was the last; with none left to list, its
cursor is on its header, which says why. A comment closed is not
lost, as the file only grows: `closed` shows each
as it was, `closed @12` on its first line, its menu (which Enter opens) to
`Reopen it` or to read its history. The agent's comments moved to the stack
are not closed, nor what is moved back. The stack's menu, on its header,
has `Close all comments…`, every open comment of the stack, its own and its
patches': it asks first, saying how many it closes (`close all 3 open
comments of the stack?`), and how many stay open, those with the agent or
whose reply waits for your review, as no comment is closed while so.

A comment you no longer need to read can be collapsed by hand: its menu's
`Collapse this comment` shows it in the middle as one quiet line, which
says how the comment starts and how many turns it has (`▸ comment: why so
loud? · 3 turns`). The cursor still goes on that line, and the comments
pane lists the comment as before. Press Enter on the line, or choose
`Expand this comment` in its menu, to show the comment whole again; writing
on it (a reply, an edit) expands it too. A comment stays collapsed until it
is expanded, opened again or not.

The commits made in the agent's worktree, stacked's and the agent's, are
not signed, whatever git's `commit.gpgsign` says: they are only read back
as patches.

The agent runs in a process group of its own. Quitting the editor leaves it
running, and what it does lands on its own; leaving (ctrl-c, or `Quit`)
lists the asks still running, each as `stacked runs` says it and where its
agent works: its worktree. A worktree with work the agent did not commit, or
that could not be filed, is left in place, and the run's summary says
where; others are kept for the next ask. `Runs…` lists every ask and pull,
each ask with the model and the effort it was asked with
(`answer(claude-opus-5-5, max) s "why so loud?"`),
as `stacked runs` does, and shows what the agent wrote. In the browser and
in StackED, the mouse over a run marks it, underlined, and picks nothing:
a click picks the run, and a second click on it is Enter. The wheel over
the list scrolls it, the run picked staying picked, in sight or not, until
a key.

`Stop the agent…`, in the menu of a comment the agent works on, and on an
ask that runs in `Runs…` (Delete does the same in both places), asks
first. Once you say so, the agent's process group is sent SIGTERM, and
SIGKILL if the agent has not exited three seconds later; the ask then
lands as failed at once, and what waited behind it goes. Meanwhile the bar
says `stopping the agent…`; once the ask has landed, the status line says
that the agent was stopped and its ask landed as failed. The comment is
free again: its stream is folded, the comment is marked failed (`✗`,
`ask failed: you stopped the agent`), and its menu offers everything
again. The run's summary says that you stopped the agent, and its stream
ends with `■ you stopped the agent`. `stacked stop RUN` does the same from
the command line, RUN being a run as `stacked runs` lists it, or the start
of its id. A build and test stopped as a review closes lands the same way.

An ask whose agent is gone without landing it (killed by something other
than stacked) lands as failed on its own: the editor looks for such asks
every few seconds, `stacked send --wait` as it waits, and `stacked runs`
before it lists the runs. So does a pull from Phabricator whose read is
cut short, the process reading it gone (Phabricator, below). The editor
also waits for each process it started for an ask once that process has
exited, so that none is left a zombie, which would look as if its agent
still ran.

`STACKED_CLAUDE` names a program to run as the agent, over the settings'
`agent.command`. The tests use it. Whatever it is, it is given claude's
arguments (`-p PROMPT --output-format stream-json …`) and must answer as
claude does.
