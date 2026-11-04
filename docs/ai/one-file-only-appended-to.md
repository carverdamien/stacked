# One file, only appended to

Everything is in the database, one JSON object per line, and a line once
written is never changed, but the header: patches, every state of every
stack, which state each stack is on, comments and their replies, every run
(an ask, a lint job, a pull) with what came of it, every line the agent
wrote while it worked, what each stack's comments pane shows, what is
read, and what was read of Phabricator. A record is named by its place
among the records, its rev: `@12`.
The first line, the header, is `@1`: it says the file's format, and is 128
bytes long, padded with spaces.

- A patch is stored once, by the sha1 of its content. An edited patch is a
  new patch with the same change, so `versions` and `interdiff` can follow a
  patch through its versions.
- A stack record is the whole stack after an edit, with the stack it was
  edited from as its parent. A head record moves a name to another stack:
  undo, redo, a stack picked from the tree, a delete.
- A comment record is the whole comment as it is now; its earlier records
  are its history. The agent's turns are fenced ```` ```agent ````
  (```` ```claude ````, as the first files have them, reads the same) and
  the way they were asked (```` ```agent answer ````), then, when their run
  was asked at an effort, `effort` and its name (```` ```agent answer
  effort max ````), a turn of before having none; then, an answer-only
  ask's, `stack` and the rev of the stack it was asked on (```` ```agent
  answer effort max stack 12 ````), which its heading counts the edits
  since, by the stacks' index alone, no record read; and
  a reviewer's ```` ```reviewer ```` and their name; a reviewer's turn an
  edit of it replaces has `redacted` after the name (```` ```reviewer ann
  redacted ````), and their edit `edit` and the first line of the comment it
  edits (```` ```reviewer ann edit Hi Da ````), both when it is an edit
  redacted in its turn; their comment read again with the suggested edit
  Phabricator gives with it, which no one edited, `anew` in the place of
  `edit` (```` ```reviewer ann anew Hi Da ````); a turn holding fences of its own (a
  code block) is fenced with one backtick more than they have, and only a
  line of as many backticks ends it. A turn of yours is as you wrote it,
  unless a line of it would read as one of those fences: then it is fenced
  too, ```` ```you ````, so that it stays yours whole. A
  turn deleted by stacked's first way of deleting one is fenced ````
  ```deleted ````, each line of it quoted with `>`: it is read only to be
  left out, and nothing writes one any more. A
  comment collapsed, or expanded, is a record naming it, apart from its
  history. A read mark holds the lines it read; taking it back is a record
  naming it.
- An ask's run, once landed, holds what it brought back for review: the
  stack its change makes (its patches stored already, by their content),
  each reply (an answer-only ask's with the rev of the stack it was asked
  on), each comment it opened. Nothing of it is applied before its
  review closes. With its change of the stack, it holds what each
  operation of that change needs, worked out as it landed (`needs`, and so
  does any run holding a change of the stack: a lint job's, an edit by
  hand's, a reconcile with HEAD's, a pull's): the operations it was worked out for,
  by a digest of the two stacks they follow from, their repository, bases
  and patches (`of`), as another change, or the same from another stack,
  is not theirs; and each operation that something needs, or that
  something goes with, rejected alone (`alone`): its key (`op CHANGE`,
  `op base`), the operations whose patches then no longer apply
  (`needed_by`), those that go with it, each with the operation whose
  patch as it was needs it (`with`: `key`, `because`, `by`), and the
  operation whose patch as it was nothing makes apply (`stuck`); or, could
  it not be worked out, why (`failed`). Its review decides from it. A run
  a stacked of before landed holds none, nor is one stored for it later:
  its change is reviewed as a whole. It is part of the run's record,
  written again with it: nothing of it is indexed, so the checkpoints are
  as they were. Bringing nothing back, it holds none of them, and is held
  all the same while a comment it was asked about is still open. A
  follow-up asked from a review is queued as the comments it asks about,
  each naming the work it follows up (`on`) and saying it is one
  (`follow`); landed, its run holds nothing for review, and the work's run
  is written again with what it brought: its replies after the work's (a
  build and test's marked `read`), the comments it opened, a try push's
  tasks, the tools it could not use, and, among the work's follow-ups,
  which comments it was asked about and what a build and test found of
  each patch. A lint
  job's run holds the command it runs and where it stands, waiting its turn
  or running, and the stack it lints once it runs; landed, what it brought
  back, as an ask's does, with what the linter reported. A review
  record holds the marks saved for a run's review (each operation of its
  change of the stack, accepted or rejected, by the change of its patch,
  `op CHANGE`, and the base's, `op base`; a change with no operation to
  mark, and a change closed whole, marked as a whole too, `stack`, which
  is how a stacked of before marked every change, a mark each of its
  operations takes when such a review is opened again; each answer made on
  the
  stack before its change, kept, restarted or stopped, by that answer's
  run: `keep RUN`, `restart RUN`, `stop RUN`),
  the replies taken already, whether it is closed, and, of an edit by hand,
  the patches that should not change that accept was typed for before
  $EDITOR opened on it, by their versions (`confirmed`, Phabricator below),
  which its close changes with no one asked again; the last one is what
  stands. A refusal of a write that would change such patches writes
  nothing at all.
- A reconcile with HEAD's run holds what it takes, each patch's change,
  its commit and that commit's message, and the work it went on from, if it
  was chained; made, its change of the stack, as an ask's.
- A pull from Phabricator's run (`"what":"pull"`) holds its job (`pull`,
  in a stacked of before none: such a run holds the comments it pulled
  alone): what it was asked (`asked`: `"stack"`, `{"patch":{"change":…,
  "children":true}}`, `{"new":"D123"}`); each revision it read, in the
  order it read them (`read`: its `D123`, link and PHID, its status,
  title, summary and bug, its parents, how the pull came to it,
  `"reached"`: `"asked"`, `{"parent":"D12"}`, `{"child":"D12"}`, the id of
  its latest diff and of the older diffs it read, the commit its latest
  diff is on, the author and message of that diff's commit as uploaded,
  its comments, and when it was last modified); the work it went on from,
  if it was chained (`after`); and, made with a change of the stack, what
  each patch of it takes from Phabricator (`took`: by the patch's change,
  its revision, whether its diff and its message are Phabricator's, how
  it was added, the version the patch was at the last diff stacked knew
  since when it changed in the stack, and, made again on what the pull
  changed under it, the files whose conflicts it marks), and what it found
  that changes nothing, a sentence each (`found`). Waiting its turn, it is
  started; made, done, its change of the stack and the comments it pulled
  held as an ask's work is. The raw diffs it read are rawdiff records,
  older ones too, and what it read of each revision revision records, as a
  fetch writes them (below).
- A moved record says where stacked left HEAD of a checkout on a stack
  (`{"t":"moved","stack":"s","checkout":"/path/to/top","commit":"…"}`):
  the commit HEAD was at right after Move HEAD, or HEAD following a review,
  moved it. The last one of a stack and checkout is what stands. A file a
  stacked of before wrote has none, and reads as if stacked had left HEAD
  nowhere.
- A revision record holds what a read of Phabricator learnt of a
  revision, by the link its patch's trailer gives
  (`{"t":"revision","link":"https://…/D123","d":"D123","state":{…},"comments":{…},"time":…}`):
  its state, its status as Phabricator writes its value (`needs-review`),
  its title, summary and bug, its PHID, its parents, each its PHID and its
  `D123`, the id of its latest diff, the commit that diff is on as
  Phabricator names it (`"base"`, none in a state a stacked of before
  wrote, or where Phabricator says none), whether it is a draft
  (`"draft"`, its search's `isDraft`, none in a state a stacked of before
  wrote, which a submit fetches again first) and the PHID of its
  repository (`"repository"`, its search's `repositoryPHID`), or that
  Phabricator does not have it (`"state":{"missing":true}`); and its comments, each the id of its
  version as it was last edited, those of its versions before and the
  comment it replies to, which the Phabricator line counts those not
  pulled yet by, with when the revision was last modified as they were
  read (`dateModified`; none from a pull of a stacked of before). A read
  writes only the parts that differ from what is stored, and nothing for a
  revision it found as it was stored: `Fetch the latest from Phabricator`,
  the fetch a submit makes first and a pull write both; a submit writes
  nothing else of a revision. The latest record with
  each part is what stands. A rawdiff record holds a revision's raw diff
  (`{"t":"rawdiff","link":…,"diff":456,"raw":"…"}`), its latest, or an
  older one a pull read, written once for each diff id, as a diff never
  changes; and a fetched record says when a fetch, or a pull, read
  revisions (`{"t":"fetched","links":[…],"time":…}`), the latest that
  names a revision what stands of it. A repository record holds a
  repository on Phabricator a submit read once to make a new diff in it
  (`{"t":"repository","phid":"PHID-REPO-…","callsign":"MOZILLACENTRAL","vcs":"hg","time":…}`):
  its PHID, its callsign, as the search gave it or as it was asked by,
  and what holds it, `git` or `hg`; none of it changes, so it is written
  once, and every submit after takes it from there, the latest of a PHID
  what stands. A file a stacked of before wrote
  has none of them, and reads as if nothing had been fetched yet; a
  stacked of before passes over these lines as lines it cannot read, with
  its warning (`stacked check` lists them), and reads the rest of the file
  as ever.
- A crash can only cut the last line short. A line that cannot be read is
  passed over with a warning (`stacked check` lists them, by line) and
  keeps its rev, so every rev after it stays the same. The next write ends
  a cut line with a newline; nothing is ever cut off the file.
- Every write holds the file's lock (flock), so two editors, the CLI and
  landing asks can share it. Every read holds the lock too, shared: a
  reader of the file, or of what was appended to it since it last read,
  waits for a write under way to end, and never reads a record half
  written. Readers do not wait for one another, and a writer waits only
  for the reads under way. While stacked holds the lock to write, it reads
  the file under that lock, and asks for no other, which would wait on
  itself. So a line cut short is only ever what a writer killed midway
  left.
- A file an older stacked wrote, its header shorter, is written again once,
  the first time something is written to it: the same lines under the new
  header, renamed over the old file, every rev the same. Not while an ask
  is going, as its process could be an older stacked's, which reads no
  file of a newer format; then the next write does it. An editor with the
  old file open reads the new one whole, and a process waiting for the
  lock opens the new file once it has the old one's lock. An older stacked
  does not read the new file.
- The header points at the last checkpoint: a line of its own, which
  takes no rev, saying where in the file the records are that the state
  stands on (each stack's head, the stacks one undo and one redo go to,
  the comments open, the asks going or waiting for your review and what
  they need, what is chosen and marked read, where stacked last left HEAD
  of each checkout, what stands of each revision read from Phabricator,
  the raw diff its state names and when it was last fetched, and each
  repository read from it), and how the indexes stood; a checkpoint of
  before, with no index of where stacked left HEAD, or of what was read of
  Phabricator, or of its repositories, reads as one with none.
  stacked reads the header, that checkpoint, the records it says, and the
  lines after it: not the whole file. What needs the history (an older
  stack, the log and the tree, the versions of a patch, a comment's
  history, the comments closed, the runs done with and what their agent
  wrote) reads the whole file the first time it is asked.
- A checkpoint is written, then the header pointed at it, once the file
  has grown four times the last one's length past it (and at least
  256 KB), and when a stack's head moves. A crash in between leaves the
  header pointing at the one before. A header or a checkpoint that does
  not check out (half written, pointing past the end, its sum wrong) is
  passed over, and the file read whole; a header half written is written
  again.

The file only grows; `stacked trim` makes a new, shorter one. An ask's
events include the agent's tool output, so a long ask can add a few
megabytes.
