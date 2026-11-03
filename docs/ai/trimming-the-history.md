# Trimming the history

`stacked trim NEW --from @N`, or `Start a new database here…` on an entry of
`History…`, writes a new database: the history from `@N` on, and before it,
as its start, what there was then. The database it is made from is only
read; NEW must not be there (the editor names it `db-from-N.jsonl`, by the
old one). Without `--from`, only what there is now is kept.

The start holds each stack as it was, and any earlier state a later record
names (the parent of a later edit, where a later undo goes, where an ask
started), the patches they need, the comments as they stood, the read marks
standing, each stack's filter, what stands of each revision read from
Phabricator (its state and its comments, each as the read that stored it
left it, the raw diff its state names, and when it was last fetched), and
the runs still going with what their agent had written. Runs that had
landed, earlier states no one names, the earlier versions of comments and
patches, the raw diffs of a revision's diffs before its latest: that is
what is left out. Every
record from `@N` on follows, its revs made the new file's. The new file is
written whole, then read back: every stack must be as in the old one, or
nothing is written.
