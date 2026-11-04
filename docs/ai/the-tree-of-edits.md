# The tree of edits

Every edit makes a new stack whose parent is the stack edited. Undo moves
back to the parent, and redo forward to the child you came back from. An
edit made after an undo starts a branch, and the old branch stays:

```
$ stacked tree bug123
● @10   2026-10-16 22:31   3 patches   drop ae318fd6
│ ○ @8  2026-10-16 22:31   4 patches   move f32a1147 from 2 to 1
○ ┘ @6  2026-10-16 22:31   4 patches   import 4 patches
```

`select @8`, or enter on it in the editor's `Tree of edits…`, goes back to
the other branch. Deleting a stack moves its name to nothing, and `undo`
brings it back.

With the `moz-phab` feature, undo, redo and a stack picked from the tree
are refused, as every write is, when the stack they go to holds another
version of a patch that should not change, its revision Accepted or Landed
on Phabricator, than the stack they leave: the editor's dialog asks, and
the command line on its terminal, `accept` to type (Phabricator, below).
Deleting drops every patch, which changes none.
