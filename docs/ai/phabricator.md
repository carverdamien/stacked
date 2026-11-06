# Phabricator

What follows is built by default, with the `moz-phab` feature:
`--no-default-features --features app,demo,try-push,web` builds stacked
without it (Install).

`stacked moz-phab pull NAME`, or `Pull from Phabricator` in the stack's
own `Phabricator` menu (a line then `Phabricator`, at the end of a group
of the stack's menu), pulls the stack from Phabricator, as one job held for one
review: every revision its patches name in their `Differential
Revision:` trailer (the settings can name another), with the revisions
they stack on that the stack does not hold (the open parent of each,
down to one the stack holds or to the first that is not open), and those
stacked on top of the top's revision (the open one stacked on each, while
there is one); their latest diffs, titles and summaries, as new versions
or new patches, and the reviewers' comments. It reads them over Conduit,
with the token moz-phab keeps in `~/.arcrc`, or the settings'. Every call
of Conduit is made by curl, which is given the token and the call's
parameters on its standard input, never on its command line, where any
process of this machine could read them; what went wrong is said as
Phabricator answered it (The editor, above): a status other than 200, a
page that is not JSON, or its turning away the requests of this machine
(HTTP 429).

Its run is written the moment it is asked, and it is queued then, as an
edit of the agent's is (Asking the agent, above): it holds its place in
the queue from then on, the edits asked after it waiting for it. It goes
through three phases, reading Phabricator, waiting its turn and being
made, then it is filed. It reads first, away from the screen, in the
process that asked it (the editor's, or `stacked moz-phab`'s), its run
naming that process: the revisions asked, in one search
(differential.revision.search); their parents (edge.search), named, the
parents the stack does not hold in a search of their own, then down, the
open parent of each that the stack does not hold; up, the open revision
stacked on the top's (edge.search, `revision.child`), and on each found,
with its parents; then each revision in turn, the banner saying which
(`⇣ pulling from Phabricator: D12, revision 2 of 3…`) and the top bar
`pulling…`: its diffs (differential.querydiffs), the commit its latest
diff is on, stored with its state, and the author and message of that
diff's commit as uploaded
(`local:commits`); its latest diff, raw (differential.getrawdiff),
unless it is stored; while that diff is no version of its patch stacked
knows, the older ones, the newest first, to the first that is one; and
its comments (transaction.search, user.search for their authors' names,
and differential.querydiffs, once for each diff an inline comment is on,
for the lines a comment on a line is on, on its side of the diff, and
those its suggested edit replaces, a call of it failing the read as any
call does). What it read is stored as a
fetch stores it (The editor, above): of each revision, its state and its
comments, with when it was last modified as its search said, where they
are not what is stored; each raw diff it read; the revisions asked that
Phabricator does not have, as such; and that it read every one of them,
now. A read that fails stores nothing of Phabricator (no state of a
revision, no comment, no raw diff, no fetch), and the pull's run fails,
its summary saying why, as the bottom line does: a call that fails, said
as Phabricator answered it, one turned away tried once and said plainly
(`Phabricator is turning away requests from this machine for now (HTTP
429, too many requests)`); a revision with more than one open parent, or
more than one open revision stacked on one where the walk up goes,
nothing pulled (`D11 is stacked on more than one open revision on
Phabricator, D10 and D9: a stack is one line of patches, so nothing was
pulled`, `More than one open revision is stacked on D11 on Phabricator,
D10 and D9: a stack is one line of patches, so nothing was pulled`). A
read cut short, the process reading it gone as it reads (the editor
quitting, the command killed), holds nothing back from then on, and its
run fails once a stacked sees it, as an ask whose agent is gone does
(Asking the agent, above): `the read of Phabricator was cut short: ask
the pull again`, nothing of it stored. Phabricator is read only when you
ask.

Read, the pull waits its turn as an edit of the agent's does (Asking the
agent, above), the first asked first: unchained, until the change of the
stack waiting for your review is reviewed (`the pull from Phabricator
waits its turn: it is made once you have reviewed the agent's change of
patch 2, and what it brings then waits for your review`); chained
(`agent.chain`), at once, from the stack that change makes, and reviewed
after it. Waiting, it has a line of the banner (`◌ The pull from
Phabricator, waiting its turn: the stack's revisions, with their parents
and those stacked on top`), whose menu, and the stack's own
`Phabricator` menu, have `Drop the waiting pull from Phabricator…`, which
asks first, as a patch's own `Phabricator` menu, and its line's, have it
when it is a pull of that patch's revision; dropped, what it
read stays stored, and nothing of it waits for your review; it is
dropped only then (`the pull from Phabricator is reading Phabricator: it
can be dropped once it waits its turn`, `the pull from Phabricator is
being made, so it can no longer be dropped: what it brings waits for your
review once it is made`). Its line says each phase as it is, from when it
is asked: `⇣ The pull from Phabricator, reading Phabricator: …`, or, read
by this editor, what it reads (`⇣ The pull from Phabricator, reading D12,
revision 2 of 3: …`), the banner's own line saying `pulling from
Phabricator: D12, revision 2 of 3…`; `◌ …, waiting its turn: …`; and, its
turn come, `● …, being made: …`; Enter there says where it is (`The pull
from Phabricator is reading Phabricator: then it waits its turn, and what
it brings waits for your review once it is made`, `The pull from
Phabricator is being made: what it brings then waits for your review`),
and opens its menu while it waits its turn. What the pull waits for is
what is ahead of it in the queue: a reconcile with HEAD or a lint job
asked after it waits for it, and holds it back only once under way. One
pull of a stack at a time, from when it is asked to when it is filed (`a
pull from Phabricator of this stack waits its turn already: one at a
time`, `… is reading Phabricator already: …`, `… is being made already:
…`), and the editor reads one pull at a time (`already pulling from
Phabricator: one pull at a time`). From when it is asked to when it is
filed, reading Phabricator, waiting its turn and being made, it changes
the stack as the agent's edit does: the stack is not edited by hand
meanwhile (Asking the agent, above), the menus saying why (`a pull from
Phabricator is reading Phabricator (the stack's revisions, with their
parents and those stacked on top): the stack can be edited by hand again
once the pull is made and you have reviewed its change of the stack, if
it makes one`; read, `a pull from Phabricator waits its turn (…): …`;
its turn come, `a pull from Phabricator is being made (…): …`), and the
same commands refuse on the command line. It is made from the stack as it
is at its turn, which no edit by hand changed since it was asked. At its
turn, it is made away from the database's lock, on a copy of the
database, then filed:

- A patch whose revision is open takes as its new version its revision's
  latest diff, where that is no version of the patch stacked knows (as a
  submit knows one: Submitting a patch, below), with the author of its
  commit as uploaded; and its title and summary, as its message, its
  trailer kept, where no version of the patch would send them (a submit's
  `WIP: ` aside). A version stacked knows takes nothing: the stack is
  ahead of it.
- A patch changed in the stack since the last diff of its revision
  stacked knows, its revision given a new diff since: Phabricator's is
  taken all the same, and the review shows what the patch had since that
  version, which the pulled one lacks.
- A revision the stack does not hold, open, is added: a parent below the
  patch of the revision it is the parent of, one stacked on another above
  that one's patch. The patch is its latest diff, with the author of its
  commit as uploaded, and that commit's message while the revision's
  title and summary are what it says, else those, the trailer naming the
  revision put in.
- Then each patch is made to apply on those under it, from the bottom. A
  patch above the pulled versions that no longer applies on them is made
  again on them, as `git merge-file` makes a file, its conflicts marked
  where they disagree (`<<<<<<< the stack below, as the pull leaves it`,
  `>>>>>>> patch 3`, and `||||||| the stack below, as it was` with the
  repository's `merge.conflictStyle` diff3 or zdiff3), so that the stack
  the change makes still applies; a pulled diff that does not apply on the
  patches under it is made again on them the same way (`>>>>>>> D12's
  latest diff`). Where that cannot be done either, what keeps it from
  applying is not taken, and the review says so.
- A revision that landed or was abandoned leaves its patch as it is, and
  a parent Phabricator gives that the stack's order disagrees with leaves
  the order as it is: the review says each, under `what the pull found,
  to read`.

What each operation of its change needs is worked out on that copy too,
as every landing works it out (What the agent brings back, above), and
stored with it. Filed, what it brings waits for your review, in the queue
of work the agent's asks land in: its change of the stack, if it
makes one, and the comments, anchored on the patches as the pull leaves
them; nothing is filed before you close that review. The bottom line
says what came of it (`1 new version, 2 new patches and 1 new comment
from 4 revisions: they wait for your review`; `nothing new from 3
revisions`; `…: it waits for your review; patch 3 no longer applies on
the pulled versions: its conflicts are marked, to resolve`). It is filed
only on the very stack it was made from: its patches, its base and its
repository. Chained, the work it goes on from accepted whole while it is
made, the stack is now the one that work made: it is filed on it, and
reviewed as any. That work rejected, accepted in part, sent back or
edited by hand from its review, the pull is made again from the stack as
it then is, as a chained edit of the agent's is dropped and asked again:
what it brings is then what it pulls onto that stack. Should the stack
move all the same while it is made, it is made again too.

Its review is the review of a change of the stack (What the agent brings
back, above), each operation marked on its own, and what it takes from
Phabricator said after it: `modify  3 Bug 3 - …, from D13's latest diff`,
`reword  2 Bug 2 - …, from D12's title and summary`, `add     1 Bug 1 -
…, D11, the parent of D12`, `add     4 Bug 4 - …, D14, stacked on D13`,
`modify  3 Bug 3 - …, no longer applies on the pulled version`. On an
operation, the right pane says where its patch comes from, and why, and,
for a patch changed in the stack since, what it had since then that the
pulled version lacks: the interdiff from that version to the patch as the
stack has it, in the view of the interdiffs that `1` sets there, every
line or only what the patches changed (The editor, above), the two named
`version 3` and `this version`. A patch whose conflicts are marked does not go in:
closing with its operation standing is refused, the review's `Close
review` and `Accept all remaining and Close` grayed, saying why (`patch 3
(Bug 13 - eleven) holds the conflicts the pull marked where it does not
apply on the patches under it: reject its operation, resolve them by hand
in the patch, or send the pull back to the agent`). Rejecting its
operation takes the operation that breaks it along, as the patch as it
was does not apply on the pulled version (`reject patch 3's operation
with patch 2's operation?`). Resolved by hand from the review (`Edit
this patch in $EDITOR`), the edit lands in the place of the pull's work,
a pull's still (`modify  3 Bug 3 - …, its conflicts resolved by hand`),
which then closes as any. `Send it back to the agent…`, offered then,
with a permission to change (`change patch` for one patch, `change
stack`), has the agent rework it from the stack the pull makes, a comment
of yours handing the conflicts over, on the patch, or on the stack for
more than one, as the linter's work hands its conflicts over (Linting,
above): the review closes, its comments filed and its change not applied,
and the agent's work lands for review in its turn. A pull's comments are
never rejected: closing its review files each one (`Close review`, or
`stacked review NAME RUN --accept-all`); `--reject-all` rejects its
change alone, and is refused for a pull that brought comments alone. A
comment on a patch the pull adds is filed only with that patch: rejected,
the next pull brings both again. Each
new thread is a comment opened, on its line, or on the patch's title for
one on the whole revision, its replies pulled with it turns of it; a reply
to a thread there already is a reply to it, the reviewer's turn (`ann's
reply waits for your review`, `Accept ann's reply`). The banner counts the
pulls apart (`1 pull from Phabricator waits for your review`), and the
review says what was pulled. Pulling again holds only what is new: a
comment is passed over when a thread holds it, open or closed (as it was
when you closed it, read from the history, which a pull reads), or when a
pull whose review is still open holds it; a comment some of which still
waits in a pull's review is pulled once that review closes. Nothing
remembers a rejection: a comment a stacked of before let you reject, which
no thread holds, comes back once, with the next pull, and is filed as its
review closes; a reply or an edit as well.

In the editor, `Pull D12 from Phabricator`, on the menu of a patch's
Phabricator line (The editor, above) and in the patch's own `Phabricator`
menu in the patches pane, pulls that patch's revision with its parents
that the stack does not hold, or, as its choice picks (`with its
parents`, `and the revisions on it`, kept for the stack), with the
revisions stacked on it too, as the stack's pull pulls them: the banner
says so (`⇣ pulling from Phabricator: D12…`), and the bottom line what
came of it (`2 new comments from D12: they wait for your review`). No
pull is offered while reviews come from nowhere (`reviews.source =
"none"`, Settings), nor in the tour, and `stacked moz-phab` refuses then
(`reviews come from nowhere: …`, `revisions come from nowhere: …`).

`Pull from Phabricator…`, under the stack menu's `New stack…` and on the
menu of an editor with no stack (Making a stack in the editor), pulls a
new stack: it asks for a revision (`a revision of Phabricator, to pull
into a new stack: D123, or its link`), saying what it gives as it is
typed (`D12, with the revisions it stacks on and those stacked on it`;
`give a revision: D123, or its link`), and refusing one a stack holds
(`D13 is patch 3 of D12: pull it there`) or one that the work waiting for
your review on a stack adds (`D31 would be patch 1 of fix once the work
waiting for your review there is accepted: review that work first`). It
makes the stack at once, empty, on HEAD of the checkout, named after the
revision (`D12`, else the first of `D12-2`, `D12-3`, … that no stack has),
with the pull's run, and shows it; the pull reads the revision with its
parents and those stacked on it (`pulling D12 from Phabricator, with the
revisions it stacks on and those stacked on it…`), none of which a stack
may hold; read, it moves the stack onto the commit the bottom revision's
diff is on, which the repository must have (`D21 is on 0123456789ab,
which this repository does not have: fetch it first`), and lands into it,
its review open: a patch to add for each revision (`3 new patches and 1
new comment from D11..D13: they wait for your review`). Its read failing,
or its review rejected, the stack stays, empty.
`stacked moz-phab import REVISION [--name NAME]`, of `D123`, `123` or its
link, does the same from the command line and prints the run, which
`stacked review NAME RUN --accept-all` takes.

A comment the reviewer edited on Phabricator since it was filed (each edit
makes a version of it, with an id of its own) is held as an edit of their
turn (`ann's edit waits for your review`, `Accept ann's edit`). Its review
shows the comment as it was filed, then as it is now, one above the other,
compared word by word where their lines wrap aside, as two versions of a
message are (`ann's comment as it was filed, then as it was edited on
Phabricator`), then the thread as it would be. Accepted, the edit is the
reviewer's latest turn, headed by what it edits: `ann's edit of the comment
that said "Hi Da"`, the comment's first line, cut with `…` past 60
characters. The turn it edits, the latest version of the comment, stays as
it was, for what answered it to read as it was written, and is redacted:
headed `ann (redacted)` in the thread, `ann's comment (redacted)` where a
turn is named, and told to the agent as replaced by the edit below it. A
later edit redacts the latest version's turn the same way, and is added
after the rest. With its turn gone (rewound), the edit is the reviewer's
last turn, and nothing is redacted.

A new reply or edit in a thread you closed is pulled and held, with the
thread as it was when you closed it. Its review shows that thread, its
line saying `(closed)` after where the comment is, and says that
accepting it reopens the comment (`ann's reply to a comment you closed, as
it would be in its thread: accepting it reopens the comment`);
`stacked landed` says `to a comment you closed: accepting it reopens the
comment`. Accepted, as the review closes or taken at once, it reopens the
comment as `Reopen it` does, as it was when closed (as its history has it
last, when it was written since), its reply or edit in it (`reviewed: 1
reply accepted; 1 comment reopened`); until then the comment stays closed.
It is not pulled again: the pull's review holds it until it closes, and
the thread then. One moved to the stack's comments and closed there
reopens there; one open elsewhere since (moved with its lines, or back
from the stack's comments) takes the reply where it is open, and nothing
is reopened. Replies and edits to a thread moved to the stack's comments
go to it there. A comment removed on Phabricator stays in its thread as it was. A
reviewer's comment can be asked about like any.

A reviewer's suggested edit is pulled with their comment, where
Phabricator gives it, as Mozilla's does: transaction.search gives an inline
comment's suggested edit, as the comment is now, in its fields, whether it
has one (`hasSuggestion`, which alone says so, as `suggestionText` is none
without one, but empty on Phabricators of before, as it is for an edit
that deletes its lines) and what replaces the lines the comment is on
(`suggestionText`), with the side of its diff the comment is on
(`isNewFile`). The comment's text says its words, if it has any, then who
suggests the edit and of which lines, then the edit as a unified diff
writes it, between the fences of a `diff` code block: each line it
replaces, as the diff the comment was made on has it on that side, after
`-`, then each line of the edit after `+`, a newline ending the edit's
last line rather than starting another, nothing trimmed:

````
ochameau suggests this edit of lines 2906–2909:
```diff
-  // MOZ_LOG already logs the module name, so only the prefix is added.
-  const nsString& label = aIsForMozLog ? mPrefix : mLogModuleAndPrefix;
-  nsString message = (aIsForMozLog ? u""_ns : u"console."_ns) + aMethodString +
-                     u": "_ns + label + (label.IsEmpty() ? u""_ns : u": "_ns);
+  const nsString& prefix = aIsForMozLog ? mMozLogPrefix : mConsolePrefix;
+  nsString message = (aIsForMozLog ? u""_ns : u"console."_ns) + aMethodString +
+                     u": "_ns + prefix + (prefix.IsEmpty() ? u""_ns : u": "_ns);
```
````

The fence makes the diff a code block, so the editor, in the browser and
in StackED too, shows its lines as they are, as one block in the colours
for code (Writing comments, above), cut only where the pane ends: without
it, markdown would bullet each `-` and `+` line as a list's item, its
indentation lost, and fold it between words. The agent, asked about the
comment, is told the diff for what it is. An edit with no line of its
own deletes the lines: `ann suggests deleting lines 3–4:`, its `-` lines
alone. Where a line it replaces is not in the diff read (the diff not in
Phabricator's answer, or the line in no hunk of it), nothing is guessed:
`ann suggests this edit of
lines 3–4, but the lines replaced could not be read from Phabricator, so
only the new lines are shown:`, its `+` lines alone; `ann suggests
deleting lines 3–4, but those lines could not be read from Phabricator.`
A comment on the old side of its diff (`isNewFile` false), with a
suggested edit or not, is on the lines the patch found: its suggested
edit says so after its lines (`lines 2–3 of the file before the patch`),
its `-` lines that side's, and its thread is on the patch's line it is on
there, one the patch removes or one of context, found by that side's text
near that side's number, else by the number, as a comment on the new side
is found among the lines the patch leaves. Where Phabricator does not say
which side a comment is on, it is on the new side, as before.

A comment read with its suggested edit is a version of its own, its id the
latest version's and `+suggestion`, the version it is read from, as read
without it, the one before it. So a comment filed without its suggested
edit, with its words alone, or by a stacked of before that read no
suggested edit (`ann suggested an edit to line 2 here, which stacked
cannot read, as Phabricator's API does not give suggested edits: see it on
Phabricator, …`), is read anew by the next pull, which holds it to take
the place of its turn as an edit does (above). No one edited it, so
nothing says it was edited: it is counted apart from the comments edited
(`2 suggested edits to pull`, `1 edited comment and 2 suggested edits from
D11: they wait for your review`), named for what it is (`ann's suggested
edit waits for your review`, `Accept ann's suggested edit`), and its
review shows `ann's comment as it was filed, then with the suggested edit
that stacked now reads from Phabricator`, over `ann's comment, with the
suggested edit that stacked now reads from Phabricator, as it would be in
its thread`; closing that review says `reviewed: 2 suggested edits
accepted`. Accepted, it redacts the turn it takes the place of, and is
headed `ann's comment that said "Shout less here.", with its suggested
edit` (its fence says `anew` where an edit's says `edit`); the agent is
told that stacked had not read the suggested edit, and that the turn below,
the same comment read with it, replaces the one redacted. A comment
edited on Phabricator, a new version of it, with a suggested edit or not,
is an edit, said as ever; a pull that brings both counts each. A reply is
pulled into the thread of the comment it answers by either id. Phabricator gives no suggested edit of an older
version of a comment: an older version is known by both its ids, and says
its words, or with none, what a stacked of before said. A comment removed
is none, whatever it suggested. An inline comment Phabricator says has no
suggested edit is pulled with its words alone; with no words, it says
nothing, and is not pulled, as a comment on the whole revision with no
words is not.

Where Phabricator gives no suggested edits, its transactions saying
nothing of them, an inline comment that is a suggested edit alone, with no
words, and not removed, is pulled all the same, as any comment is: a
thread of its own on its line, or a reply in the thread it answers; and
the Phabricator line counts it among the comments not pulled yet. Its text
says what it is, and where Phabricator shows it: `ann suggested an edit to
lines 2906–2909 here, which stacked cannot read, as this Phabricator's API
does not give suggested edits: see it on Phabricator,
https://phabricator.example.com/D123?id=456#inline-789`, `line 2906` for
one line. The link is the revision's page showing the diff the comment was
made on (`?id=`), at the comment's anchor (`#inline-` and
transaction.search's `id` of the comment), as the revision's history on
Phabricator links an inline comment made on another diff than the one it
shows: there the comment is drawn, whatever diffs came after it. Edited on
Phabricator since to have words, it is a comment like any other: the edit
is held as an edit of the turn that says it is a suggested edit (above),
which it redacts once accepted. A suggested edit that comes with words is
not seen there: the comment is pulled with its words alone.

A line of a commit message that names a bug (`Bug 123`), or the revision
(`Differential Revision:`), opens it in your browser: `o` there, or the
line's `Open bug 123 in your browser` (The editor, above), the bug on
Bugzilla, `reviews.bugzilla` (Settings, below). A line of what a comment
says opens them from its menu too (Writing comments, above).

The patches pane shows the status of each patch's revision after its number
(`[W]`: The editor, above), as `Fetch the latest from Phabricator` read and
stored it, over Conduit with the same token, all of the stack's revisions
in one search; and the Phabricator line over the message of the patch the
middle shows says it by name, whether the revision's latest diff and its
title and summary are the patch's, its parent the one a submit sets, how
many of its comments are not pulled yet, and how long ago that was
fetched; or, once a submit changed the revision since it was last read,
that it did, and when, as the patches pane's `↻` says it (Submitting a
patch, below). Nothing of it is read by itself. Space there shows what differs
on a screen of its own, in the view of the interdiffs that `1` sets there,
every line or only what the patches changed (as an interdiff, in a column
per version, or side by side, or the change
from the latest diff, on the commit it is on, to the patch as it is, as a
patch of its own, a fetch, the one a submit makes first, and a pull
storing that commit as Phabricator names it), and its menu
holds every item of the patch about Phabricator (The editor, above).

## Patches that should not change

A patch whose revision is Accepted, or Landed, as its status was last
fetched (by `Fetch the latest from Phabricator`, a pull, or the fetch a
submit makes first), should not change: a change of an Accepted patch needs
review again, and a Landed one is in the tree already. Which statuses
protect a patch is said in one place (`protected::protecting`); every other
status, a patch that names no revision, and one whose revision nothing is
fetched of yet, change as any. Without the `moz-phab` feature, or with
reviews coming from nowhere (`reviews.source = "none"`, Settings, above),
no patch is protected.

A stack written to hold another version of such a patch, its diff or its
message differing from the version the stack holds, changes it; nothing
else does: the patch dropped from the stack, moved with its version as it
was, another version of the same diff and message (another author, say), or
left as it is while patches change around it. No such stack goes in, nor
does a name move to one of its tree (an undo, a redo, a stack picked in the
tree of edits), unless that patch was confirmed for that one operation: the
database refuses the write, nothing written, as an error of its own kind
that names the patches (`patch 3 should not change, so nothing changed: D123
is Accepted on Phabricator, and a change needs review again`; of several,
`patches 3 and 5 should not change, so nothing changed: D123 is Accepted on
Phabricator, and a change needs review again; D125 is Landed, and it is in
the tree already`). Every write of a stack checks it, once the stack is
seen to apply, a check that keeps what it found, so that the write made
again once it is confirmed checks nothing twice; a write that replaces no
version of a patch costs a look at the ids alone.

In the editor, in the browser and in StackED, a write refused so opens the
dialog that asks about those patches (The editor, above), where `accept` is
typed; once it is, what was asked is done again, as it was asked, those
patches confirmed for it: an undo or a redo, a stack picked in the tree of
edits, a version put back in the stack from the versions of a patch, and
the close of a review of a change of the stack (the agent's work, an edit
by hand, Reconcile with HEAD, a pull, a lint job), from the review, the
review staying open meanwhile, or from the banner, which rejects what is
left of the work (`Reject them and close`), the main screen staying. A move
and a drop write no other version of a patch, and never ask. Cancelled,
nothing is written, and the bottom line says that nothing changed, and why
(`nothing changed, as accept was not typed: patch 3 (D123) is Accepted on
Phabricator`).

An edit by hand in $EDITOR asks before the editor opens, so that no edit is
lost to a refusal after it: of a patch as mail, when that patch should not
change; of its code, when it or a patch above it should not, as a conflict
with one of those, resolved by hand, makes it again on the edit; of the stack
as a list, which may edit or reword any patch, when any should not; and so
from the review of landed work, of the stack the work makes, the patches
named as the stack holds them. The dialog then says that the edit may change
them (`this edit may change a patch that should not change`), and cancelled,
that it was not made (`the edit was not made, as accept was not typed: patch
3 (D123) is Accepted on Phabricator`), $EDITOR not opened. Once `accept` is
typed, $EDITOR opens, and the confirmation goes with the edit: not taken, it
is edited again, or its conflict resolved by hand, with no one asked again
of those patches; landed, its review keeps them (`held::Review`'s
`confirmed`), so that closing it changes them with no one asked again, and
an edit by hand of that work from its review hands them on to the work that
takes its place. A patch that came to be protected since is asked about as
any; cancelled then, the menu of the edit not taken comes back, the edit
kept.

A pull takes what Phabricator has: closing its review asks nothing of a
patch it brings to what Phabricator has of its revision, as the pull's read
stored it: its latest diff, raw, and the revision's title and summary as a
submit sends them (`submit::message_against`), each or both, the rest of the
patch as it was (`protected::from_phabricator`). It asks of one it changes
otherwise, as a patch made again on the pulled versions under it is, an
Accepted one or a Landed one, which the pull leaves as it is but for that.

Work landing for your review (the agent's, a pull's, a lint job's,
Reconcile with HEAD, an edit by hand) writes no stack: the stack changes
only as its review closes, which asks then, and nothing is asked before.

On the command line, every command that writes a stack (`import`, `base`,
`add`, `move`, `drop`, `undo`, `redo`, `select`, `delete`, `edit
--accept`, `review`, `send --accept`) asks the same on its terminal, when its
standard input and its standard error are one: the dialog's lines, then the
question, `accept` read to the letter:

```
stacked: this changes a patch that should not change.
Patch 1, D12, is Accepted on Phabricator: a change needs review again.
That status was fetched from Phabricator 11 minutes ago.
Type accept to change patch 1 anyway: accept
```

`accept` typed, the command is made again, those patches confirmed for it;
anything else, nothing changed (`stacked: nothing changed, as accept was not
typed: patch 1 (D12) is Accepted on Phabricator`). With no terminal to ask
on, it refuses, saying why and which patches, and that a terminal asks
(`stacked: patch 1 should not change, so nothing changed: D12 is Accepted on
Phabricator, and a change needs review again; on a terminal, stacked asks
you to type accept to change patch 1 anyway`), nothing written; `stacked
edit --accept` says too that the edit waits for your review, as its run.

## Submitting a patch

`Submit to Phabricator as WIP`, in a patch's own `Phabricator` menu and
on the menu of its Phabricator line (not while reviews come from nowhere,
`reviews.source = "none"`), sends that patch alone to Phabricator as work
in progress, as `moz-phab submit --wip` sends a single commit: in Changes
Planned, a status that asks no one for a review. In the browser (`stacked
web`) and in StackED, the same. Nothing is sent before it is worked out
and seen. First, away from the screen, what is sent is worked out from
what the database holds of Phabricator, as the last fetch stored it (The
editor, above), nothing read: the revision the patch's trailer names
(`Differential Revision:`), or none, and the submit makes a new one; the
revision of the patch below, for the parent; and the repository a new
diff is made in. What the database lacks of them is fetched into it
first, the bottom line saying so (`fetching D123 from Phabricator first,
as nothing of it is stored yet: nothing is sent yet`; `as what is stored
of it is not all that a submit needs` where some of it is), as Fetch the
latest from Phabricator fetches those revisions alone, and stored as it
stores them, with when: the revision, while nothing of it is stored, or,
open, while its latest diff is not stored raw, or what is stored of it
was stored by a stacked that did not keep whether it is a draft (the
search's `isDraft`, which a fetch and a pull store now, with its
`repositoryPHID`, no call added); and the revision of the patch below,
while nothing of it is stored. A repository is read once, with
diffusion.repository.search, by its PHID, the revision's, or for a new
revision by the callsign the checkout's `.arcconfig` names
(`repository.callsign`), and stored, as none of it changes; only where a
new diff is made in it. When the database holds all of it, nothing is
read of Phabricator (`working out what submitting patch 3 sends: nothing
is sent yet`). A fetch that fails sends nothing, and the bottom line says
why, Phabricator turning this machine away said as such (`D123 could not
be fetched from Phabricator, so nothing was sent: Phabricator is turning
away requests from this machine for now (HTTP 429, too many requests)`);
a repository not read too (`The repository of D123 could not be read from
Phabricator, so nothing was sent: …`). A revision Phabricator did not have
when it was fetched, or that had no diff, is said so (`Phabricator had no
revision D123 when it was last fetched, so nothing was sent.`). A revision
there already in work, Changes
Planned or a draft, as moz-phab takes it, is sent the patch as work in
progress; one open but not in work, Needs Review, Needs Revision or
Accepted, only the patch's title, summary and parent (below); one that
landed or was abandoned refuses, its status said as the Phabricator line
says it (`D123 is Landed, not open: only an open revision is updated, and
nothing was sent.`). In work, its latest diff
must be a version of the
patch the database holds, the one in the stack or an earlier one: of the
same effect, or leaving the same files once both are applied on the
patches below, so that a diff moz-phab sent with more context is still the
version it was made from. When neither rule finds one, a third looks for
the version whose changes it makes: file by file, the same lines removed
and added one after another, whatever lines of context are around them and
wherever its hunks start. moz-phab sends a diff with more context than
stacked keeps, so once a rebase has changed lines of that context, the
diff no longer applies on the patches below, though its changes are that
version's. A diff known by this rule is never the patch as it is, as it
was made on the patches below as they were before: a new diff of the
patch is sent, made on them as they are now. Else it came from elsewhere,
and nothing is sent
(`D123's latest diff is not a version of patch 3 that stacked knows: it
came from elsewhere, and nothing was sent.`). And while HEAD is on the
stack, a patch whose commit in your workspace names a revision the patch
does not, none or another, as after `moz-phab submit` there, whatever else
the two messages say, is not submitted before anything is read: its menu grays the item and says to
give the patch that revision first, so that no second revision is made
(`Patch 3 names no revision in the stack and D34 in your workspace's
commit: give the patch D34 first, with Reconcile with HEAD on its menu, so
that no second revision is made.`); the other patches, and other differences, are not held back.

For a new revision, a dialog then says what is sent and where (`Patch 3 is
sent to Phabricator, at https://phabricator.services.mozilla.com, as a new
revision, with the status Changes Planned.`), with its title and its
parent. For a revision there already, a screen of its own shows what
replaces what, for your review: what is sent and where, and how long ago
what it shows of the revision was fetched, in the Phabricator line's
words, worked out as it is drawn, so that it grows while the screen
stays open (`What is shown of D123 here was fetched 11 minutes ago.`);
then, of what changes, the title and summary, Phabricator's then what is
sent, compared
as an interdiff compares two messages (History, above: a paragraph
only wrapped otherwise once, as it is sent; a section that differs,
Phabricator's lines then those sent, what only one has in the colours for
words; and when nothing differs but where their lines wrap, or their
white space, that is said, and each paragraph written otherwise is there
as each writes it, Phabricator's lines then those sent, no word in the
colours for words); the parent and the bug, before then after, and the
status; and the code, each file as the latest diff leaves it, then as the
patch does, both on the patches below it, a file that only one of them
touches too. When the code is the patch's already, the screen says so at
its top. When the latest diff was known by the third rule, the screen says
at its top why a new diff is sent (`The latest diff of D123 makes the same
changes as version 2 of patch 3, but it was made on the patches below
patch 3 as they were before, and it no longer applies on them as they are
now, so a new diff of patch 3 is sent, made on them.`), and the code is
then each file as that version leaves it, then as the patch does, both on
the patches below it as they are now. ↑↓ (pgup pgdn home end) scroll. 1
opens the dialog of the views of the interdiffs (The editor, above), as
on every screen of them, and the code is drawn in the view it sets.
Compared as two diffs, it is drawn as the screen of what differs from
Phabricator draws its code for that revision and that patch, by the same
code: Phabricator's side is its latest diff, or, where that diff was
known by its changes alone, the version of the patch whose changes it
makes, as the screen says at its top; the other side is the diff the
submit sends, the patch's as the stack has it. On one base, the two are
an interdiff, what git derives in them left out of both (an `index`
line, the heading after a hunk's `@@`), under `the code: the two diffs,
Phabricator's then what is sent` (side by side, `the code: the two
diffs, Phabricator's on the left, what is sent on the right`), said so
(`The code is shown as an interdiff of the latest diff of D13 and this
version's diff.`), and named `D13 on Phabricator, fetched 11 minutes ago`,
with how long ago it was fetched, as the screen says it (or `version 2 of
patch 3`), and `what is sent`: as an interdiff, in a column per version,
or side by side, as there, with every line or only what the patches
changed, which leaves out the lines both have alike, the context lines
and the index lines, and so what the move of a base made in the lines
around the patch's. On other bases, each file as that diff leaves it,
then as the patch does, both on the patches below it, as it is, or each
file side by side, under `the code: Phabricator's, then what is sent`
(side by side, `the code: Phabricator's on the left, what is sent on the
right`); where the files cannot be read so, the interdiff all the same;
each said as there (`The latest diff of D13 is on another base than this
version, so the code shown is each file as that diff leaves it, then as
this version leaves it, both on the patches below patch 3.`). Compared as
the code, it is each file as the latest diff leaves it, then as the
patch does, both on the patches below it, as the middle draws a patch,
unified, side by side, or side by side and compact, under `the code:
Phabricator's, then what is sent`; every line of it is drawn, whatever
lines the dialog sets, as both are on the patches below: nothing a change
of base made is left out. From one view to another, the screen keeps its
place, the cursor on the line it was on, a line of the code on the line
of the interdiff nearest it, as on the screen of what differs, and back.
Side by side, the lines of the title and
summary that differ are side by side too, Phabricator's on the left,
coloured the same way, and so are those of what else changes. The top
bar says the view, as on the screens of interdiffs; `3` shows no file
whole there. Esc opens the screen's menu, and so does a click with the
right button, next to the cell clicked; Enter and Space do nothing there,
and a click with the left button does nothing either. The menu has
`Send it to Phabricator`, which sends it, `Cancel: send nothing`, which
sends nothing, and `The view of the code…`, which opens the dialog as 1
does; on a line of the title and summary that has links, those links
after them. Esc in the menu closes it, back on the screen. The bottom bar
says `↑↓: scroll · esc: menu · 1: view · ?: what can I do here`, and `?`
says the same. When no code is sent, as when the code is the patch's
already, there is no view to set: 1 does nothing, the bottom bar names
no 1, the menu has no view of the code, and what the screen says is
unified. Nothing of Phabricator is read before it is sent, nor after: it
is sent as the screen shows it, against the revision as it was fetched,
and what changed on Phabricator since is not seen, which the age the
screen says is there for; Fetch the latest from Phabricator, before the
submit, reads it again. Only the patch is seen to be as it was (`Patch 3
changed since what is sent was read, so nothing was sent. Submit it again
to see what is sent now.`). Once it is sent, what stacked shows of the
revision, its Phabricator line, the patches pane's letter and the next
submit's review, is what was fetched before the submit, with how long ago
that was, until the next read of it, as the bottom line says after what
was sent (`What is shown of D123 was fetched before this submit.`; for a
new revision, `Nothing of D124 is fetched from Phabricator yet.`), and
nothing is fetched by itself. A submit that changed the revision, its one
edit made or its edit back to Changes Planned, or the revision made by it,
is remembered, with when, by the link the patch names it by, a new
revision's as it goes into the patch (One file, only appended to, above),
whatever came of the rest: from then on the patches pane marks the
revision `↻`, in the place of its `✓` or `✗`, a new one with no letter
(The editor, above); its Phabricator line says it in its own words, in the
place of whether it is in sync and of how long ago it was fetched
(`Phabricator: D123 · Changes Planned · no new comments · D123 was
submitted 2 minutes ago, after it was last fetched, 11 minutes ago: Fetch
the latest from Phabricator to see it as it is now`; for a new revision,
`Phabricator: D124 · D124 was submitted just now, and nothing of it is
fetched yet: …`); and the review of a submit of it adds it to how long
ago what it shows was fetched (`What is shown of D123 here was fetched 11
minutes ago, before it was last submitted, 2 minutes ago.`), and so does
its `?`. Any read that stores the revision after it, `Fetch the latest
from Phabricator`, the fetch a submit makes first or a pull, brings its
`✓` or `✗` back, from what it read, and the line and the review as they
were: no record is taken out, the order of the records decides. A submit
that changed nothing of the revision is not remembered: one that failed
before any write, one whose diff was made and whose edit was refused, or
one with nothing to send. Should the database not take the record, the
bottom line says so before its last sentence (`But the database could not
keep that D123 was submitted: …`).

Only what changed is sent, in the writes moz-phab makes, and nothing
else: a new diff of the patch (differential.creatediff), unless the latest
diff is the patch as it is, then its commit's author, message, parent and
tree (differential.setdiffproperty, `local:commits`); then one edit
(differential.revision.edit) that holds every transaction, as moz-phab's
`edit_revision` makes it: the title (`WIP: ` and the patch's first line,
its reviewers and its bug written as moz-phab writes them), the summary
(the rest of the message, without the line naming the revision) and the
bug (`Bug 123` in the title), where they differ from what is stored; the
new diff; the parent, the revision of the patch below, when it is open
(none for the bottom patch, or when the patch below has no revision, and
a revision's parents are then left as they are), set where it changes,
added to a new revision; and Changes Planned, for a new revision or a
draft. A revision in Changes Planned that takes a new diff is put in
Needs Review by Phabricator, which refuses Changes Planned in that same
edit, as the revision is Changes Planned still when it starts: Changes
Planned goes back in an edit of its own, after it. So a submit makes four
writes at most: three for a new revision, or for a draft taking a new
diff; four for a revision in Changes Planned taking a new diff; one for a
revision that gets its title and summary alone, or its parent alone. A
refused edit leaves the revision as it was, the parent with the rest
(`The new diff of patch 3 was made on Phabricator, but the edit of D123
that takes it failed, so nothing of D123 changed: …`; with no new diff,
`The edit of D123 failed, so nothing of D123 changed: …`). When the
latest diff is the patch as it is and nothing else changes, nothing is
sent, and the bottom line says so. The Phabricator line takes the parent
by this same rule: it differs exactly where a submit would set one (The
editor, above). A submit as work in
progress sets no reviewers, as `moz-phab submit --wip` sets none: those
the title names (`r=alice,bob!`, `r?#build`) stay in the title, written
as moz-phab writes them, and nothing of the revision's reviewers is read,
said or changed. While it is sent, the top bar says `submitting…`; then
the bottom line says what came of it, and what was done and what was not
when a call failed partway (`D123 has the new diff of patch 3 now. But
D123 was not put back in Changes Planned, and may be Needs Review now: …`),
then that what stacked shows of the revision was fetched before (above). A new revision's link goes into the patch, as a
new version taken at once, which the history says (`submitted patch 3 as
D124, in work`). Its trailer goes where moz-phab puts one: at the end of
the message's last paragraph when that paragraph is trailers
(`Reviewed-by: …`), else after an empty line. The workspace's commits are
not touched: the patch's commit there then has a message other than the
stack's, and `Move HEAD…` writes the stack's message, the trailer with it,
there, while HEAD is where stacked last left it. When HEAD is
on the stack and has the patch's commit, the diff is said to be that
commit, made on the commit below it (on the stack's base for the bottom
patch), with that commit's tree, as moz-phab says it. Else the diff is
said to be the patch's own sha1, made on the stack's base, and its tree,
the files as the patch leaves them on the patches below it, is written in
a check index of its own, the repository only read; when that tree cannot
be written, nothing is sent, and the bottom line says why. The diff is in
the repository the checkout's `.arcconfig` names (`repository.callsign`;
a revision there already keeps its own), as the database holds it (above).

For a revision open but not in work (Needs Review, Needs Revision,
Accepted), the patch's `Phabricator` menu, and the menu of its Phabricator
line, have, in the place of `Submit to Phabricator as WIP`, an item that
sends the revision the patch's title, summary and
parent alone, named for what differs as the Phabricator line says it (The
editor, above): `Update D123 on Phabricator: its title and summary`,
`Update D123 on Phabricator: its parent`, or `Update D123 on Phabricator:
its title, summary and parent`. It sends no new diff, as only a revision
in work gets one, and no status, so the revision stays as it is. The
title is written as a submit in work writes it, but for `WIP: `, which
goes first only when the revision's title has it now, as the Phabricator
line compares it; it goes with the summary, the bug and the parent, the
parent by the same rule as a submit in work's, where they differ, in one
edit (differential.revision.edit), one write. As a submit in work, it
reads and changes no reviewers: those the title names stay in it, written
as moz-phab writes them. The revision's latest diff is left as it is,
whatever version of the patch it is, and its repository is not read. The
item is grayed, and the menu says why, when only the diff differs (`D123
is Needs Review, so stacked sends it no new diff, as only a revision in
work gets one, and its title, summary and parent are those of patch 3
already.`), when nothing differs (`D123 is in sync with patch 3, so there
is nothing to send.`), and while what differs is not read and compared
yet (`D123 is Needs Review, so only its title, summary and parent would be
sent, and they are not read from Phabricator and compared with patch 3
yet.`). `?` on the item says what it sends.

Chosen, it works out what is sent away from the screen, as a submit does,
from what the database holds, what it lacks fetched first (`working out
what updating D123 sends: nothing is sent yet`), then shows what is sent
on the same screen as a submit's review: what is sent and where (`D123,
at https://phabricator.services.mozilla.com/D123, gets the title and
summary of patch 3, and stays Needs Review.`), and how long ago what it
shows of the revision was fetched, then the title and summary
compared, the parent and the bug before and after, and the status, said to
stay as it is (`status: stays Needs Review`). It shows no code. When the
code differs too, the screen says so at its top, and why it is not sent
(`The code of patch 3 is not sent: D123 is Needs Review, not in work, and
only a revision in work gets a new diff.`); else that the code does not
change. With no code, there is no view to set: 1 does nothing there, and
the bottom bar names no 1 (`↑↓: scroll · esc: menu · ?: what can I do
here`). `Send it to Phabricator`, in the menu Esc or a right click opens,
sends it, the bottom line saying `updating D123 on Phabricator…`
meanwhile; `Cancel: send nothing` sends nothing; that menu has no view of
the code. Nothing of Phabricator is read before it is sent, nor after, as
for a submit in work. When nothing differs, nothing is sent, and the
bottom line says so. Once it is sent, the bottom line says what came of
it, as after a submit (`D123 has the title and summary of patch 3 now.
What is shown of D123 was fetched before this submit.`), and nothing is
fetched: the edit made, it is remembered as a submit's is, the patches
pane marking D123 `↻` until a read of it. While the revision's status is not
fetched yet, or is one stacked does not know, the menu has `Submit to
Phabricator as WIP`, and the status the submit reads decides what is
sent. For a revision that landed or was abandoned, `Submit
to Phabricator as WIP` is grayed, and the menu says why (`D123 has landed,
so stacked sends it nothing.`, `D123 was abandoned, so stacked sends it
nothing.`).
