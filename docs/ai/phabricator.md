# Phabricator

What follows is built by default, with the `moz-phab` feature:
`--no-default-features --features app,demo,web` builds stacked without it
(Install). `stacked moz-phab import REVISION` makes a new stack of a
revision, `D123`, `123` or its link, as `Import from Phabricator…` does in
the editor (Making a stack in the editor): `imported 2 patches from
D11..D12 as D12`.

`stacked moz-phab pull NAME`, or the menu's `Pull reviews from
Phabricator`, reads the revision in
each patch's `Differential Revision:` trailer (the settings can name
another) over Conduit, with the token moz-phab keeps in `~/.arcrc`, or the
settings'. Every call of Conduit is made by curl, which is given the token
and the call's parameters on its standard input, never on its command
line, where any process of this machine could read them; what went wrong
is said as Phabricator answered it (The editor, above): a status other
than 200, a page that is not JSON, or its turning away the requests of
this machine (HTTP 429). While it reads them, the banner says which revision it reads
(`⇣ pulling the reviews from Phabricator: D12, revision 2 of 3…`), and the
top bar `pulling…`; once it is done, the bottom line says what came of it
(`1 new comment from 3 revisions: it waits for your review`), or what went
wrong. In the editor, `Pull the reviews of D12 from Phabricator`, on the
menu of a patch's Phabricator line (The editor, above) and in the patch's
own `Phabricator` menu in the patches pane, reads that patch's revision
alone, as the stack's pull reads each: the banner says so (`⇣ pulling the
reviews from Phabricator: D12…`), the bottom line says what came of it (`2
new comments from D12: they wait for your review`), what it brings is held
as a pull's. A pull, of the stack or of one revision, in the editor or by
`stacked moz-phab pull`, stores the comments it read of each revision, as
the line counts those not pulled yet (The editor, above), and nothing else
is read after it. As the stack's pull, it is not
offered while reviews come from nowhere, and not done in the tour; one
pull goes at a time (`already pulling the reviews`). What is new waits for
your review, in the queue of work the agent's
asks land in (below): nothing is filed before you close that review. A
pull's parts are all accepted, never rejected: closing its review files
every comment it pulled (`Close review`, or `stacked review NAME RUN
--accept-all`; `--reject-all` refuses). Each
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
fetched. Nothing of it is read by itself. Space there shows what differs
on a screen of its own, and its menu holds every item of the patch about
Phabricator (The editor, above).

## Submitting a patch

`Submit to Phabricator as WIP`, in a patch's own `Phabricator` menu and
on the menu of its Phabricator line (not while reviews come from nowhere,
`reviews.source = "none"`), sends that patch alone to Phabricator as work
in progress, as `moz-phab submit --wip` sends a single commit: in Changes
Planned, a status that asks no one for a review. In the browser (`stacked
web`) and in StackED, the same. Nothing is sent before it is read and
seen. First, away from the screen, the patch's revision is read, and what
is read of it stored, as a fetch stores it (The editor, above), whatever
comes of the rest: the revision its trailer names (`Differential
Revision:`), or none, and the submit makes a new one. A revision there already in work, Changes
Planned or a draft, as moz-phab takes it, is sent the patch as work in
progress; one open but not in work, Needs Review, Needs Revision or
Accepted, only the patch's title, summary and parent (below); one that
landed or was abandoned refuses (`D123 is Closed, not open: only an open
revision is updated, and nothing was sent.`). In work, its latest diff
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
replaces what, for your review: what is sent and where; then, of what
changes, the title and summary, Phabricator's then what is sent, compared
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
the patches below it as they are now. ↑↓ (pgup pgdn home end) scroll, and 1 shows the code side by
side, or unified again, the lines of the title and summary that differ
side by side too, Phabricator's on the left, coloured the same way.
Enter, or `Send it to Phabricator` in its menu, sends it; Esc, or
`Cancel: send nothing`, sends nothing. Before anything
is sent, the revision is read again, and what is read stored: if its
latest diff, its status, its title, summary or bug, or its parents changed
meanwhile, nothing is (`D123 changed on Phabricator since you reviewed it,
so nothing was sent. Submit again to review it as it is now.`). Once the
submit is done, sent or not, the revision it sent to is fetched again,
alone, as the patch names it then, a new revision too (The editor,
above).

Only what changed is sent, as moz-phab sends it: a new diff of the patch
(differential.creatediff, its commit's author, message, parent and tree
set with differential.setdiffproperty), unless the latest diff is the patch as it
is; the title (`WIP: ` and the patch's first line, its reviewers and its
bug written as moz-phab writes them), the summary (the rest of the
message, without the line naming the revision) and the bug (`Bug 123` in
the title), where they differ; and Changes Planned, kept, or put back after
a new diff, which Phabricator takes as a request for review
(differential.revision.edit). When the latest diff is the patch as it is
and nothing else changes, nothing is sent, and the bottom line says so.
Then, in an edit of its own, so that a refusal undoes none of the rest:
the parent, the revision of the patch below, when it is open (none for
the bottom patch, or when the patch below has no revision, and a
revision's parents are then left as they are). The Phabricator line takes
the parent by this same rule: it differs exactly where a submit would set
one (The editor, above). A submit as work in
progress sets no reviewers, as `moz-phab submit --wip` sets none: those
the title names (`r=alice,bob!`, `r?#build`) stay in the title, written
as moz-phab writes them, and nothing of the revision's reviewers is read,
said or changed. While it is sent, the top bar says `submitting…`; then
the bottom line says what came of it, and what was done and what was not
when a call failed partway (`D123 has the new diff of patch 3 now. D123
stays Changes Planned. But the parent of D123 was not set to D122: …`). A new revision's link goes into the patch, as a
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
a revision there already keeps its own).

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
line compares it; it goes with the summary and the bug, where they differ,
in one edit (differential.revision.edit); then the parent, in an edit of
its own, by the same rule as a submit in work's. As a submit in work, it
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

Chosen, it reads the revision away from the screen, as a submit does
(`reading Phabricator to show what updating D123 sends: nothing is sent
yet`), then shows what is sent on the same screen as a submit's review:
what is sent and where (`D123, at
https://phabricator.services.mozilla.com/D123, gets the title and summary
of patch 3, and stays Needs Review.`), then the title and summary
compared, the parent and the bug before and after, and the status, said to
stay as it is (`status: stays Needs Review`). It shows no code. When the
code differs too, the screen says so at its top, and why it is not sent
(`The code of patch 3 is not sent: D123 is Needs Review, not in work, and
only a revision in work gets a new diff.`); else that the code does not
change. Enter, or `Send it to Phabricator` in its menu, sends it, the
bottom line saying `updating D123 on Phabricator…` meanwhile; Esc, or
`Cancel: send nothing`, sends nothing. Before anything is sent, the
revision is read again, as for a submit in work: if its latest diff, its
status, its title, summary or bug, or its parents changed meanwhile,
nothing is. When nothing differs, nothing is sent, and the bottom line
says so. Once it is sent, the bottom line says what came of it, as after a
submit (`D123 has the title and summary of patch 3 now.`), and the
revision is fetched again, alone. While the revision's status is not
fetched yet, or is one stacked does not know, the menu has `Submit to
Phabricator as WIP`, and the status the submit reads decides what is
sent. For a revision that landed or was abandoned, `Submit
to Phabricator as WIP` is grayed, and the menu says why (`D123 has landed,
so stacked sends it nothing.`, `D123 was abandoned, so stacked sends it
nothing.`).
