# Copied from self-review-rs

stacked started from self-review-rs, a reviewer of your own changes in the
terminal (the worktree's, or a stack's, `main..HEAD`), whose comments claude
is asked about as stacked's are. It lives in
[my-moz-tools](https://github.com/carverdamien/my-moz-tools), beside
self-review, the Python tool it rewrites in Rust; "as in self-review" here
means both.

stacked does not depend on self-review-rs. It has its own copies of
`diff.rs` (lines keyed by their text, so a comment, or a read mark, stays on
a line the next version keeps), `claude.rs` (allowed tools, reading replies
and stream events) and `blocks.rs` (code blocks, from tree-sitter grammars
or braces). It also takes the Conduit half of `phab.rs`, the thread fences
of `thread.rs`, the side-by-side pairing of `rows.rs`, the compact layout of
`nav.rs` (`panes.rs`), the reading view's marks of `review.rs`
(`reading.rs`), the inline editor (`editor.rs`), the rule for which asks go
together, the filter of the comments pane, and what a range is on (a
branch on its upstream, else the remote's HEAD; a range typed, said as it
is typed: `import.rs`). A fix to one may belong in the other.
