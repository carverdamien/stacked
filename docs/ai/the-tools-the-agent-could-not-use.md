# The tools the agent could not use

claude refuses a tool the agent is not let use before it runs, and its
result lists each call it refused. Each refusal is an item of the review
of the work, by the rule that would have let the call run: one item a
rule, with the calls that needed it. Five calls of `searchfox-cli` are one
item, `Bash(searchfox-cli *)`, which lists the calls. A plain program's
rule is `Bash(PROGRAM *)`. A broad program's is narrower: for `./mach` and
`git`, what it is asked to do (a refused `./mach try fuzzy --no-push -q …`
is `Bash(./mach try fuzzy *)`, `git log --oneline` is `Bash(git log *)`);
for `sh`, `bash` and `python`, the script it runs (`Bash(bash
./tools/run.sh *)`, `Bash(python3 -m pytest *)`); for `npx`, the package
it runs, with its scope or version (`npx eslint --fix` is `Bash(npx eslint
*)`, `npx @scope/pkg@2 x` is `Bash(npx @scope/pkg@2 *)`), or, after `-p
PKG`, the command it runs (`Bash(npx -p typescript tsc *)`); for `cargo`,
its subcommand (`cargo clippy --fix` is `Bash(cargo clippy *)`). The
options that npx and cargo are given before what they run stay in the
narrow rule as written, so that the rule lets the call: `npx -y eslint .`
is `Bash(npx -y eslint *)`, and `cargo +nightly --locked clippy` is
`Bash(cargo +nightly --locked clippy *)`. The options read this way are
npx's `-y`, `--yes`, `-q`, `--quiet` and `-p` (`--package`), and cargo's
toolchain (`+nightly`) and global options: `-q`, `--quiet`, `-v`, `-vv`,
`--verbose`, `--locked`, `--offline`, `--frozen`, `--color`, `--config`,
`-C` and `-Z`. After any other option, which could take the word after it
as its value, the rule is the wide one alone (`Bash(npx *)`, `Bash(cargo
*)`), as it is for `npx` or `cargo` with nothing after it. Each command of
a pipe is matched on its own, as claude matches it: of `curl … | jq .`,
only the command that was not let run is an item. An MCP server's tool is
its rule, `mcp__SERVER__TOOL`; another tool is its name (`WebFetch`).
Only the calls claude refused are items, never what the agent says it
would have liked. Work that holds an item lands for your review though
the agent failed or brought nothing back, so that a tool can be allowed
and the work sent back.

Every ask is told not to stop at a refusal: to work around it with what it
may use, and to finish what it was asked as well as it can; and to say,
for each tool it was refused, why it needed it and how it worked around
it (`"refused"` in its answer, structured output or JSON). What it said is
its comment on the item. A refusal the agent did not explain is an item
all the same, its comment saying that the agent did not say why.

The review lists the items in a section of their own, `tools the agent
could not use`, a line each: `✗ Bash(searchfox-cli *) · refused`, `✓
Bash(./mach try *) · for this work`, `✓ WebFetch · from now on`, `·
Edit · noted`. On an item, the right pane shows the agent's comment on it,
then the calls the agent tried, then where the item stands and, when the
item is not offered, why. Each item is in one of three states:

- refused, the default: the agent is not let use the tool. Delete refuses
  an item.
- allowed for this work: every round of this work may use the tool, until
  its review closes. That is the rounds sent back from its review, the
  follow-ups asked from it, and the rounds stacked starts by itself for
  that work: an answer restarted as a chained edit is accepted, an edit
  asked again once the work it went on from is rejected. claude is given
  the rule with the other tools the ask may use, and the rule is never
  saved. The stream of each such round starts by saying so (`✓ allowed
  for this work: Bash(searchfox-cli *)`), and so does the status line once
  the work is sent back. The rule ends with the review, whether the work
  is accepted or rejected.
- allowed from now on: Enter allows an item from now on, as accepting it.
  The rule is saved once the work is sent back or followed up (first, so
  that the next round may use it) or once its review closes, whether the work is
  accepted or rejected, or stacked drops the work to ask it again (its run
  saying so). It is saved only in the stack's checkout's own
  settings, `.stacked/local.toml`, never in `~/.stacked/config.toml`, nor
  in any other file of the checkout (claude's settings, the repository's
  files). It goes in a list of its own for the kind of ask the work was:
  `agent.change_accepted`, `agent.answer_accepted`, `agent.test_accepted`
  for builds and tests, and, with the `try-push` feature,
  `agent.try_accepted` for a try push's selection. That list is added to
  the tools of that kind of ask alone, after its own list, so that it never
  hides the lists `stacked setup agent` writes, global or local.

Space has the item's menu: the three states as boxes, the one the item is
in checked, each naming the rule (`Refused: Bash(searchfox-cli *)`,
`Allowed for this work: Bash(searchfox-cli *)`, `Allowed from now on:
Bash(searchfox-cli *), saved in /src/firefox/.stacked/local.toml`); Enter
on one picks it, the menu staying. Under them, a broad program's other
rules, to allow the item with instead: `Widen the rule to Bash(./mach try
*)`, `Widen the rule to Bash(./mach *)`, then `Narrow the rule to …` back.
Then `Reply to it`: your answer to what the agent said of the tool, which
goes back to the agent with the work, as any answer written in a review
does. Where the checkout's own settings cannot be written, git tracking
`.stacked/local.toml` or the stack's checkout being no checkout any more,
`Allowed from now on` is grayed, the menu and the right pane saying why,
and Enter allows the item for this work; should a rule allowed from now on
fail to be saved as the work goes back, it is allowed for this work
instead, and the status line says so.

Noted items are said, and not offered: no state but refused, as no allow
would let the call run. They are what the checkout's claude settings deny
(`.claude/settings.local.json`, `.claude/settings.json`), a deny winning
over any allow; what stacked refuses every ask but builds and tests (`find
-exec`, `rg --pre`, `rg --hostname-bin`…), and what it refuses an
answer-only ask and a try push's selection (`git diff --output`, `sort
-o`, `git grep -O`, `file -C`); what changes things, to an answer-only ask
and to a try push's selection, which only read (`Edit`, `Write`, `rm`,
`git commit`, `sed -i`, a redirection to a file); to those two again, a
rule that is never offered them, whatever the call that needed it, so
that a refused `sed -n 1,5p f` is noted as `sed -i` is; a command the
shell runs itself (`cd` elsewhere); and a call whose rule the ask was
given already, which claude refused for a reason of its own. The rules
never offered those two are those of `sort`, `file` and `git grep`, whose
short options can be bundled, so that no rule refuses the one that
writes a file or runs a program (`sort -uo FILE`, `git grep -nOCMD`); and
the rules that can write files or run programs: those of `sed`, `awk`
(`gawk`, `mawk`, `nawk`), `uniq`, `tree`, `xargs`, `env`, `command`,
`perl`, `python`, `python3`, `ruby`, `node`, the shells (`sh`, `bash`,
`zsh`) and the programs that write files (`rm`, `mv`, `cp`, `tee`…), and
git's but for its commands that only read (`git show`, `log`, `diff`,
`ls-files`, `rev-parse`, `blame`, `cat-file`, `ls-tree`, `merge-base` and
`describe`), `Bash(git *)` among them, which a refused `git -C x log`
would need. No item offered to an answer-only ask or a try push's
selection widens to one of them either. An ask that changes the stack is
offered these rules, as before.

With a tool allowed, for this work or from now on, Esc has `Send it back
to the agent…` though you wrote nothing in the review: the agent tries
again with the tool, which is how a failed attempt goes on. The work goes
back as when you wrote in its review, from the ask dialog, and a turn of
yours in the thread of the comment that asked says what you allowed and
what you answered (`On the tools you could not use:`, then ``-
`Bash(searchfox-cli *)`: you may use it now, for this work.``); asked as a
follow-up that only reads, it is your answer after that comment's reply,
where the work going back would put it in the thread. Before it goes, the
dialog says, under its buttons, what this work may use from the next
round on, or from the follow-up on, and what is saved for good, and
where. Closing the review asks first when it saves a rule, saying what is
saved and where, and that what this work was allowed for itself ends
there.

On the command line, `stacked landed NAME` lists each item of a work on a
line of its own: `tool`, its rule, its state (`refused`, `allowed for this
work`, `allowed from now on` or `noted`) and the first call it tried, a
tab between each. `stacked review NAME --send-back`, `--accept-all` and
`--reject-all` go by the states saved in the review. The editor marks the
items, in the terminal, in the browser and in StackED alike.
