# A tour

`stacked demo` (built by default, with the `demo` feature:
`--no-default-features --features app,moz-phab,try-push,web` builds
stacked without it) teaches the editor with a game of tic-tac-toe played
in a file, board.txt, one patch per move. The tour starts from an empty
board and an empty stack, and plays five moves as patches. Each patch
shows the whole board, so the moves before it can be seen. O's fourth move
is a mistake, and X's fifth move wins because of it. The tour has 14
sections, the most useful first, and takes about fifteen minutes:

1. Read a stack and its patches.
2. Comment on a line: O's mistake.
3. Ask the agent to change the code: asked about the whole stack, because
   the fix changes X's winning move too, a stand-in for the agent rewrites
   both moves (O blocks X, then X blocks O), and you can watch it write.
   Asked about patch 4 alone, the stand-in says that the fix needs more
   than that patch; accepting that answer moves your comment to the stack.
4. Review what the agent did: its work waits, the banner says so, and Esc
   opens the banner; Enter there opens the review, its change of the
   stack patch by patch and its reply, each accepted with Enter, then
   closed.
5. Reply to the agent with a question, asked answer only, its answer
   accepted at once with Enter on the comment.
6. Keep track of what you have read, in reading.
7. Pick how the code is drawn in the menu of `1`: unified, side by side,
   compact, or unified split in two; and each file whole rather than its
   hunks.
8. The history of the stack, and the versions of a patch.
9. Move a patch: stacked refuses, because each patch only applies to the
   board that the patches before it leave; Esc stops moving it.
10. Drop a patch, undo the drop, and see the tree of edits.
11. Ask the agent to add a patch: a comment on the stack asks for O's next
    move, and the stand-in adds it as patch 6, once its review accepts it.
12. See what the agent did, in Runs….
13. Where help is (`?`), and how to change the keys.
14. Start on a project of yours. At the end, the tour shows the board as
    the stack's patches leave it.

Each step says what to press, and why, and waits for you to do it: Tab does
the step for you, and ctrl-c leaves the tour. The first time it is opened,
the tour writes `[demo] seen = true` in the settings file (below), the rest
of the file as it was. From then on it starts on the list of its sections:
↑↓ pick one and Enter starts it, on a new game played up to there, the steps
before it done for you in a moment. Esc starts at the beginning. During
that tour, shift-Tab lists the sections again, to go to any of them.

Apart from that setting, it writes nothing on disk: its database and its
repository are kept in memory, and its patches are applied there as `git
apply` would apply them. Anything that would write on disk (`$EDITOR`,
trim) or reach Phabricator says that it is not in the demo.
