# Running

```sh
stacked import bug123 main..HEAD   # the commits main..HEAD as stack bug123
git format-patch --stdout main | stacked import bug123 --base main   # or mail
stacked new bug124 --base main     # an empty stack, to add patches to
stacked stacks.jsonl               # open the editor on that database
stacked                            # ... on $STACKED_FILE, the settings' file, else ./stacked.jsonl
stacked app                        # ... in a window of its own: StackED (app)
stacked app --bundle               # StackED as an app: StackED.app for the Dock on macOS, a launcher elsewhere
stacked show bug123                # its patches, bottom first
stacked show bug123 2 --path a.txt # a file as patch 2 leaves it, no checkout made
stacked edit bug123                # as `git rebase -i`: reorder, drop, reword, edit
stacked edit bug123 2 --path a.txt --line 12   # a file as patch 2 leaves it, at line 12
stacked edit bug123 2 --path a.txt --in worktree   # ... in the stack's own worktree, .stacked/bug123/user
stacked edit bug123 2 --line 30    # patch 2 as mail, at line 30
stacked review bug123 --accept-all # an edit lands for review first: accepted, it goes in
stacked comment bug123 rebase onto origin/main        # a comment on the stack
stacked comment bug123 --patch 2 split this in two    # on a patch as a whole
stacked comment bug123 --patch 2 --line a.txt:+TWO why?   # on one of its lines
stacked comment bug123 --patch 2 --path a.txt why here?   # on one of its files, as a whole
stacked comments bug123            # every comment: short id, where, state, thread
stacked comments bug123 --show closed    # the closed ones, as each was
stacked send bug123 4f1c2a --wait  # the agent works on it; its answer lands in the thread
stacked send bug123 --all --answer-only   # every comment that can be sent
stacked send bug123 4f1c2a --later   # in the stack's batch, to go with the rest
stacked send bug123 --batch --fan-out   # the batch: edits to one agent, answers an agent each
stacked send bug123 4f1c2a --effort max   # at claude's --effort max, rather than high
stacked stop 1761650000000-4242    # the agent of that run (stacked runs lists them) stopped: its ask lands as failed at once
stacked reply bug123 4f1c2a and keep the title
stacked moz-phab pull bug123       # reviewers' comments from Phabricator, held for your review (moz-phab)
stacked moz-phab import D123       # a new stack of D123, those it stacks on and those on it (moz-phab)
stacked export bug123 | git am     # or put it anywhere by hand
stacked trim new.jsonl --from @40  # a new database, the history before @40 left out
stacked config > ~/.stacked/config.toml   # every setting, to start a settings file from (once ~/.stacked is made)
stacked config editor hx           # one written in the settings file; KEY alone reads it
stacked config --unset editor      # ... taken out of it
stacked config --local editor vim  # ... in this checkout's own settings, over the file's here
stacked setup agent bug123         # what each kind of ask may use, one list of tools, checked line by line, for its checkout
stacked setup agent bug123 --tui   # ... on a screen, a box for each kind of ask on each tool, and the rules reviews allowed, to keep or take out
stacked setup lint eslint --fix    # the linter of this checkout, which the stack's menu runs: Lint the stack (eslint --fix)
stacked setup lint                 # ... the one in use, and where it comes from (./mach lint --fix where ./mach is)
```

`-f FILE` points any command at a database. A first argument that is not a
command is the database to open, if it names one: a file there, or a path
to one (`./bug123`, `bug123.jsonl`); `stacked FILE` is then `stacked open
FILE`. Any other word is a command mistyped: stacked says so, and which
command is near it (`stacked shwo`: `show`), rather than open a new
database of that name.
