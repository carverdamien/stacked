# Try push

What follows is built by default, with the `try-push` feature:
`--no-default-features --features app,demo,moz-phab,web` builds stacked
without it (Install). A comment on the stack, or on a patch as a whole
(its title), asks which of Firefox's CI tasks to run on a try push: `Run
the tests a.txt touches, on Linux and Windows`, or `as many as you see
fit`. A comment on a line or a file is not asked as a try push, as it is
not sent to build and test: its `Ask agent…` dialog has no `try push`. One that an
older stacked batched or queued as a try push is refused as it would go,
from the banner's batch, from `stacked send NAME --batch` or from the
queue, saying why: `A try push is asked of a patch as a whole or of the
stack, not of a line or a file: comment on the patch's title or on the
stack.` It stays a comment, its ask failed with that reason, and the rest
of the batch goes. Asked as a try push (`Ask agent…`, its permission set
to `try push`, then Ask Now; or Ask Later, in the batch, whose `Send 2 try
pushes, each on its own` sends each
in an ask of its own), the agent
selects the tasks the comment asks for, as few or as many as it says, each
by its exact label in the task graph (`test-linux2404-64/opt-xpcshell-1`;
a chunked test is a task for each chunk, its label ending with the chunk's
number, and no label is a pattern), with why it is relevant to what the
patches change and to what the comment asks; and, when the comment asks
for repeated runs (an intermittent failure), how many times each runs,
from 2 to 20, as mach's `--rebuild` takes it. It reads as an answer does,
where an answer works, with the tools of its own list (`agent.try_tools`,
Settings, below), and pushes nothing, nor runs `./mach`. When it may run
curl, it is told where the labels of the tasks of mozilla-central's latest
push are listed (its `target-tasks.json`), and autoland's; and where that
push's full task graph is (its `full-task-graph.json`), whose keys are the
labels of every task `./mach try fuzzy --all-tasks` can select, to check
each label it selects against it before it answers. The graph is large:
the agent downloads it once, into a folder of the ask's own that its
prompt names, the one place it may write, and reads that copy with jq, all
its labels in one go. The folder is beside the rebase plans' folders
(`$TMPDIR/stacked-todo/…`, Asking the agent, above), outside your checkout
and any worktree, so that neither shows anything new; stacked lets claude
write there (`--add-dir`), and removes it once the ask lands, however it
ends: done, failed, stopped, or found with its agent gone. Two asks at
once each have their own. If the download fails, the agent says so in its
answer, and answers without it. It gives the tasks with its replies, as
structured output, else in the JSON it ends with, whatever prose is around
it; a label given twice is taken once, and one with white space in it,
which could not select its task alone, is left out.

What it selects lands for your review, as any of its work does: the banner
says so (`the next: 6 tasks to push to try, 1 reply`), in its section of
read-only work. The review lists the tasks on the left, under `tasks to
push to try` (`· each runs 3 times`, when they run more than once), each
marked as a part is: `·` to review, `✓` accepted, `✗` rejected. Enter
accepts the task the cursor is on, or puts it back to review; Space has
`Mark as accepted` and `Mark as rejected`, `Mark all tasks accepted` and
`Mark all tasks rejected`; Delete rejects it. On the right is why the
agent selected it, as the agent's comment under the task's label. Space's
`Reply to it` answers that comment: your answer shows under it, and on the
left under `what you wrote, to send back`, where Enter writes it again and
Delete takes it back. With an answer of yours, the work is not accepted:
Esc has `Send it back to the agent…`, whose dialog sets how the comment
that asked is asked again, your answers after the agent's reply in its
thread, as a turn of yours (`On the tasks the agent selected:`, then each
task's label with what you said). Asked as a try push again, a follow-up
that only reads, the work stays waiting for your review, its marks as
they are: the follow-up is told the tasks the work selected, each as you
marked it, with why, and selects them again, and what it selects comes
into the same review, but the tasks the work holds already, which keep
their marks. Asked with a permission that changes the stack, the work is
rejected, and the round's work lands for review in its place.

Closing the review with tasks accepted is where they are pushed to try,
with the patches up to the comment's (`patch 1 alone`, `patches 1 to 3`,
or every patch of the stack, for a comment on the stack): the question
before closing says which, and that `./mach try fuzzy --no-push` checks
first that it selects them. Past the 1,000 tasks mach pushes without
asking (push.py's threshold, each task counted as many times as it runs),
it says so too: `These 1,240 tasks are more than the 1,000 mach pushes
without asking: they would run at the lowest priority, as mach does when
you say yes.` Closing with no task accepted pushes nothing, as a
rejection does: the question says so. The comment that asked stays open,
even marked to close with the review: the push's replies go there.

mach pushes every commit of HEAD that none of the remotes it takes as
Firefox's has (those fetching from github.com/mozilla-firefox/, and, in a
git-cinnabar clone, from hg.mozilla.org but its try; with none of those,
every remote), so the stack's base must be on one of them, for the push to
hold the stack's patches alone. A base that is not keeps the review open,
as the question and the status line say: `the stack's base, commit
1a2b3c4d5e6f, is not on origin, the remote mach takes as Firefox's: mach
would push 3 commits of the base's besides the stack's patches. Fetch that
remote (`git fetch`), or put the stack on a commit that it has.`

The push is made away from the screen, by a stacked of its own (`stacked
try-push RUN`, which the editor starts), from a worktree of the stack's
own, `.stacked/NAME/try` (`try-2`… while another push is made from it; under
`agent.worktrees` when that is set): the stack's patches as commits on its
base, up to the comment's. The commits mach would push are checked there
to be those patches alone. Then `./mach try fuzzy --no-push` runs, the
tasks in one query, each its own exact term, fzf's OR between them (`-q
'^a$ | ^b$'`, which fzf is given whole, so that it runs once), the labels
with their chunk numbers (`--show-chunk-numbers`), from every task of the graph
(`--all-tasks`), with `--rebuild N` when each runs more than once; what it
says it would push (its try_task_config.json) is compared with what you
accepted: the tasks, how many times each runs, and the priority. Anything
that differs is said, and nothing is pushed: `This task you accepted is
not in what mach would push, as the task graph it reads does not have it:
`test-linux2404-64/opt-not-a-task-1`.` Then `./mach try fuzzy` pushes, the
same way. Past the threshold, mach is given the lowest priority
beforehand, as it sets it itself once told yes: a preset of stacked's
(`--preset stacked-lowest`), in a presets file of its own that
`MACH_TRY_PRESET_PATHS` names, in `stacked-try/RUN` beside the database,
which is gone once the push is done with it. Your own presets are not
read then, and nothing is written in ~/.mozbuild by stacked.

mach reads nothing: its stdin is closed, and the push runs in a session of
its own, with no terminal, so that nothing mach runs (ssh, git) asks
anything on yours either. A question mach asks stops it there, before
anything is pushed (each of them comes before the push), and the reply
says so, the question quoted, with what to change: `mach asked a question that stacked cannot answer, "Do you have an
@mozilla.com address? [Y/n]", and stopped, as stacked gives it nothing to
read. Nothing was pushed.` That one comes with Pernosco asked for
(`MOZ_USE_PERNOSCO` set, or `--pernosco`) when `ssh -G hg.mozilla.org` names
no user of yours: set your hg.mozilla.org user in ~/.ssh/config, or unset
`MOZ_USE_PERNOSCO`. mach asks the other when it finds no fzf: install fzf,
or run `./mach try fuzzy` once in a terminal, where it asks to install it.

What mach writes is the comment's stream, as an agent's is, each command
stacked runs as `$ …`: under the comment, the last lines, and `Read what
mach writes for the try push` on its menu, all of them. The comment is
locked meanwhile, as one the agent works on (`try push: mach checks the
tasks`, `try push: mach waits for your login`, `try push: pushed`). Its
login (Auth0's device code) shows where you see it: the page to open and
the code to enter in the status line (`mach waits for you to log in to
push to try: open https://… and enter the code ABCD-EFGH`) and in the
stream, and the comment's menu has `Open the login page…`, which shows the
page's link, for Enter to open it in your browser. mach opens the page in
the browser itself too, unless its `try.nobrowser` setting is on; once you
are logged in, it caches its token in ~/.mozbuild, which stacked never
reads, and asks for no login until it expires.

Pushed, the push's link is a reply on the comment that asked, a web link
under it: `stacked pushed the 3 tasks you accepted to try, with patch 1
alone. Treeherder shows the push: @r1.` The comment stays locked while the
push is followed; its menu has `Open the try push on Treeherder…` and
`Stop following the try push…`, which cannot stop the push in CI: it
unlocks the comment, a reply saying `stacked stopped following the try
push: it goes on in CI, and Treeherder shows it: @r2.`, the link again.
Before mach says it pushed, the menu has `Stop the try push…`, which stops
mach (`nothing was pushed`, unless mach had started to send the push to
Lando, which it says). A push that fails says why in a reply, with mach's
last lines, and the comment unlocks: reply, and ask for the try push
again, for the agent to select the tasks again.

The push is followed while stacked runs, the editor looking at it every
few minutes, through what Lando and Treeherder show anyone, plain GETs of
their public pages, nothing of yours sent: Lando's job, at the address
`.lando.ini` gives its instance (`https://lando.moz.tools/landing_jobs/ID/`),
until Lando says it pushed it to try, and as which revision; then
Treeherder's push of that revision, then its jobs, page after page. All it
saw is kept in the database: the next editor picks it up from there, after
a restart too. Each change is a line of the comment's stream (`stacked:
Lando pushed it to try, as revision abcdef012345`, `stacked: 12 of 30
tasks done, 1 failed`), and the banner tracks it, in a section of its own,
`try pushes, followed` (`⇡ Try push of patch 1 · "Run the tests…": 12 of
30 tasks done, 1 failed`), and in its first line (`1 try push: 12 of 30
tasks done, 1 failed`); Enter there goes to the comment, Delete stops
following it. Its tasks are those you accepted, each as many times as it
runs, each by its last run (a retry's). It is done once every one of them
is, or once two looks in a row find nothing of the push pending or running
and nothing new: a task Treeherder never shows then, as it shows no task
that was never scheduled, is counted as not seen on Treeherder.

Done, its summary lands as the agent's answers do, for your review, in the
banner's read-only section, and the comment unlocks: `The try push is
done. Treeherder shows it: @r2. Of its 30 tasks, Treeherder shows 27 as
success and 2 as testfailed; 1 was not seen on Treeherder.`, then each task
that failed, with its link on Treeherder (`- `test-linux2404-64/opt-xpcshell-1`
(testfailed): @r3`), the tasks the push ran for them that failed (a build,
the decision task), and those not seen on Treeherder; each link a web link
under the reply. As an answer only's reply, it is taken at once from the
comment, Enter there putting it in the thread, or reviewed with the rest.
A push that Lando does not push to try (`FAILED`, `CANCELLED`, `ABORTED`)
lands a summary saying so, with what Lando says and its page there.
