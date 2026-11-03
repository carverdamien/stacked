# Not done yet

- Turns deleted by stacked's first way of deleting one (```` ```deleted ````)
  are still read, only to be left out. Once no database holds them, that
  code goes, and with it the split of a thread's turns between those that
  stand and all of them.
- A stack-wide review runs one agent; one per patch is not done.
- An agent other than claude must speak claude's command line and stream,
  and take `--json-schema`, unless `agent.schema` is off.
- Only git mail is read: no `hg export`.
- A patch is submitted to Phabricator one at a time, as work in progress
  alone (Changes Planned), and so with no reviewers; a revision in review
  gets its title, summary and parent alone: a stack at once, a revision
  sent for review with the reviewers its title names, a new diff of a
  revision in review, a file that is not text, and each file whole as the
  diff's context (so that reviewers can see more of it) are still
  moz-phab's, once `stacked head NAME --to N` (or `Move HEAD…`) has put
  the stack in your checkout.
- A database is not opened by dropping it on StackED's icon, nor by
  double-clicking it in the Finder or in a file manager (the launcher names
  no kind of file), and there is no Window menu. Its text is not yet a
  setting: it is Menlo on macOS, DejaVu Sans Mono on Linux (else Hack), at
  14 points before ⌘+ and ⌘−, which change it in every window at once.
  The editor the settings name for
  `$EDITOR` is read where StackED runs (where `stacked app` was started,
  the home folder opened as an app), not in each window's checkout.
