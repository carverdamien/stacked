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
