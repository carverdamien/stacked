# Settings

`~/.stacked/config.toml` holds the settings, in stacked's own folder,
beside what stacked keeps between runs (`state.json`: the databases opened
last, StackED above; and the searches of the middle, The editor above; not
how the agent is asked, nor at what effort, which each ask dialog sets
anew, Asking the agent above);
`$STACKED_HOME` names another folder than ~/.stacked, and `$STACKED_CONFIG`
another file. A checkout's own settings, `.stacked/local.toml`, override
them there (below); each is optional, and `stacked config` prints them all
as they are, in the file's form, with what is wrong with the file. As `git
config` does, `stacked config KEY` prints one (`view.layout`, `keys.down`:
each value on a line of its own), `stacked config KEY VALUE…` writes it in
the file (several values for a list: `stacked config keys.down down j`),
the file and its folder made if they are not there, and `stacked config
--unset KEY` takes it out. The rest of the file stays as it was, comments
too. What is written is checked first: a setting there is not, a value of
the wrong kind, or one stacked would not take is refused, with why, and
nothing is written; nor anything in a file stacked cannot read. The
settings:

- `file`: the database opened when none is named, nor `$STACKED_FILE`.
- `editor`: the editor, for comments, patches, the stack as a list and the
  code, with `{file}` and `{line}` where they go (`hx {file}:{line}`); else
  `$VISUAL`, `$EDITOR`, vi, told the line as each takes it. `stacked web`
  runs a terminal one in its page, unless `[web]`'s `editor` says otherwise
  (below).
- `[agent]`: `command`, the program run as the agent; `args`, added to
  every run, but an `--effort` they give, which the ask's own effort takes
  the place of (Asking the agent, above); `worktree` and
  `answer_only_worktree`, when the worktree a change works in, and the one
  an answer works in, is made: `ondemand` or `precreate` (Asking the agent,
  above; `ondemand` unless set), as every ask runs in a worktree of its
  own, never in your checkout (a file of before that says `pwd` is read as
  `ondemand`, and stacked says so); `model`. What each kind of ask may use
  besides what stacked gives it (Asking the agent, above), as claude names
  the tools for `--allowedTools`: all of an MCP server's
  (`mcp__bugzilla__*`; there is no way to name every server's at once), one
  of its tools (`mcp__bugzilla__get_bug`), a command (`Bash(jq *)`, each
  command of a pipe needing its own). `change_tools`, for the asks that
  change the stack: nothing unless set. `answer_tools`, for answer-only
  asks: `["Bash(curl *)", "Bash(jq *)"]` unless set, which answers could
  always run before there were lists; taken out, an answer is no longer
  told it may read the web with curl. `try_tools` (built with the
  `try-push` feature; read either way, so one file serves both builds),
  for the asks that select a try push's tasks: `["Bash(curl *)", "Bash(jq
  *)"]` unless set, as for answers. `tools`, for every ask, builds and
  tests too, as before there were lists: an answer may use them too, so give
  only tools that read. `change_accepted`, `answer_accepted`,
  `test_accepted` and `try_accepted`: what the reviews of the agent's work
  allowed from now on for each kind of ask, added to its tools (The tools
  the agent could not use, above), which a review writes in a checkout's own
  settings alone.

  `stacked setup agent [NAME]` sets the lists up with you. It starts the agent in the stack's repository
  (else where it is run), where it has your MCP servers, and stops it as
  soon as it has said what tools it has. The lists are shown as one list
  of tools, a row each, with a box for each list under the columns'
  headings: `read only`, for answer-only asks (`answer_tools`); `change`,
  for the asks that change the stack (`change_tools`); and, built with the
  `try-push` feature, `try push`, for the asks that select a try push's
  tasks (`try_tools`). The boxes are ticked as the settings say, else as
  stacked has it: nothing for changes; for answers, `curl` and `jq`, and
  what the checkout's own claude settings allow, as you allow it yourself,
  but for what changes things; for try pushes, as the column for answers
  starts, as both only read, for you to check before the first try push.
  First comes
  what stacked always gives, which does not change: `always` in the column
  of each kind of ask that has it, greyed, with nothing to tick. Its last
  line is the checkout's linter, which changes are given (Linting, above),
  named with the rule that lets it run: `this checkout's linter, ./mach
  lint --fix, to run on the files a change changes (Bash(./mach lint *))`;
  or, with none given, why: `the checkout's linter, when it has one: this
  checkout has none`, `lint is turned off in this checkout`, or that no
  narrow rule lets it run. Then the
  list holds what the agent says it has (each MCP server, its tools all at
  once or one by one, and the agent's other tools, but those stacked gives
  every ask, said `one of the agent's tools`); what the checkout's own
  claude settings allow (`permissions.allow` of
  `.claude/settings.local.json`, yours, then of `.claude/settings.json`;
  their `deny` is left alone), each rule as written, but for a command's
  older form (below), said `allowed in .claude/settings.local.json` (or in
  both files, if both allow it): claude applies them when it runs in the
  checkout, but every ask runs in a worktree of the stack's own, which does
  not have the first, as git does not track it; what the checkout's skills let run, unticked,
  as several skills change things (they commit, rewrite history, file
  bugs); and the programs the checkout's instructions to the agent name,
  unticked, as stacked cannot know what a program does. A skill's rules
  are the `allowed-tools` of the header of its
  `.claude/skills/NAME/SKILL.md`, on that line (a space or a comma between
  each) or listed under it, each as written, but for a command's older
  form, said `allowed by the skill NAME` (or by the skills that allow it,
  several of them); what is not a rule (`[Agent]`) is left out, and a skill a link
  takes out of the checkout is not read. Those instructions are `CLAUDE.md`,
  `.claude/CLAUDE.md`, `AGENTS.md` and `AGENT.md` at the checkout's top, and
  what a `CLAUDE.md` imports (`@path`), one level down, inside the checkout:
  a link or an import out of it, as to `~/.claude`, is not read. A program
  is the first word of what they say in code: a span, a command of a block
  of shell (past its prompt, `$ `, and what it sets first, `A=b`), or a line
  of an unmarked block. It is listed as `Bash(PROGRAM *)`, said `named in
  AGENTS.md` (or in each of the files that name it), if it is there to run,
  on PATH or in the checkout (`./mach`);
  a block of shell's that is not on PATH is listed too, said `named in
  AGENTS.md, not found on PATH`, to install and tick if it should be. What
  stacked gives already (git, grep, rg, cat…), shells and their builtins
  (cd, echo…) and what stacked refuses are left out, of the instructions,
  the skills and the claude settings alike. Last come what the settings let that
  none of these lists, said `in the settings`, to keep or take out.

  Rules that mean the same to claude are one row: a command's older prefix
  form, `Bash(searchfox-cli:*)`, and the newer, `Bash(searchfox-cli *)`,
  which claude matches alike (the command alone, or followed by a space and
  more); all of an MCP server's tools, `mcp__SERVER` and `mcp__SERVER__*`;
  and a rule that several places give. The row says where each comes from
  (`allowed in .claude/settings.local.json; allowed by the skill
  spec-trace; named in AGENTS.md`), and its boxes are ticked as for any
  row: as the settings say, else as stacked has it for any of those
  places. A command is shown and saved the newer way, which reads better,
  however its places write it: `Bash(searchfox-cli *)`, in each list it is
  ticked for. A `Bash(cmd:*)` that stacked's own settings hold (`tools`,
  `answer_tools`, `change_tools`) is rewritten so the next time the setup
  is saved, as claude reads the two alike; your claude settings are never
  written. All of an MCP server's tools are written as the settings write
  them, if they do, else as all their places do, if they agree, else
  `mcp__SERVER__*`. Any other rule is shown and saved as it is written.
  Nothing else is the same: a narrower rule is never merged into a wider
  one (`Bash(./mach file-info:*)`, shown `Bash(./mach file-info *)`, and
  `Bash(./mach *)` are two rows), nor the command alone into the command
  and more (`Bash(make)` and `Bash(make *)`), nor the older form of a
  command that holds a `*` into the newer (claude reads the `*` of
  `Bash(git * push:*)` as it is written, so that rule stays as it is).

  Of the agent's tools, the list says which can reach the network, for a
  change (Bash at large, `WebFetch`, `WebSearch`), and which change things,
  for an answer (Bash at large, `Edit`, `Write`…). The row of claude's
  `Skill` tool, `claude's tool for the checkout's skills`, is there whether
  the agent says it has the tool or not, unticked unless the settings say:
  its read-only box gives answer-only asks the skills, written in
  `answer_tools` as `Skill`, and the lines under it say what that lets in:
  a skill may run what its own allowed-tools let it, commands that change
  things too (rewriting history, filing a bug); an answer runs in a worktree
  of its own, never in your checkout. Its change
  box gives the skills to changes, written in `change_tools`. Whether what
  stacked refuses still holds against what a skill's allowed-tools let it
  run, claude's own text does not say, so stacked does not claim it. What
  the checkout's claude settings of yours deny is said above the list,
  `denied in .claude/settings.local.json, refused to every ask`, each rule
  once however it is written, to read, not to tick: every ask is refused it
  (Asking the agent, above). A tool of `tools` shows ticked in every
  column; taken out of one, it leaves `tools` and goes on in the other
  lists alone. A command or a tool of your own is added as a row, typed as
  claude writes a permission rule: a tool (`WebFetch`), a tool and what it
  may do between parentheses (`Bash(make check *)`,
  `WebFetch(domain:example.com)`), all of an MCP server's tools
  (`mcp__SERVER__*`, `mcp__SERVER`) or one (`mcp__SERVER__TOOL`); what does
  not read as one is refused, and said why (`make check is not a tool's
  name: a command is let run as Bash(make check)`). The row comes last,
  said `added here`, neither box ticked, the cursor on it, for you to tick
  what should use it; a rule that a row lets use already, however it is
  written, is that row, as it is. Saving writes each list, and `tools` if it
  changed, in that checkout's own settings (below; with `--global`, in
  `~/.stacked/config.toml`, for the checkouts without their own), the rest
  of the file as it was. Outside a checkout, only `--global` can be
  written.

  Above the list, what the reviews of the agent's work allowed from now on,
  as the checkout's own settings say it, under `Allowed from now on in
  reviews of the agent's work, in …/.stacked/local.toml: untick a rule to
  take it out`: a row a rule, its one box ticked, and the kind of ask it is
  for (`Bash(searchfox-cli *)   for answer-only asks`). Space unticks a
  row, and saving takes that rule out of its list, in the checkout's own
  settings, with `--global` too; the bottom line says what is left of each
  list a rule was taken out of. Asked line by line, each is asked in turn
  (`Keep Bash(searchfox-cli *), allowed from now on in a review, for
  answer-only asks? [Y/n]`).

  Asked line by line, the setup shows the list, a line a row as the screen
  draws it, to keep as it is (Enter) or to go through a row at a time. Each
  row is one question for every kind of ask, `Which asks may use Bash(jq
  *), stacked's default for answers? [R: read only, c: change, b: both, n:
  neither]`, the capital letter being what Enter keeps; built with the
  `try-push` feature, `read only` is for answers and try pushes both, as
  both only read, and `both` for every kind (the screen's own column tells
  try pushes apart); an MCP server's
  tools are asked all at once, or each in turn (`e`). Then it asks for a
  command or a tool to add, until a line is left empty, and asks the same
  question of each. With `--tui`, the setup is a screen of its own, with what saving writes, in the file
  it writes, at the bottom; once the list scrolls past the columns'
  heading, the heading stays on its first line. ↑↓ pick a row; ← and →, or
  Tab and shift-Tab, the column; Space ticks the box of the row picked in
  that column, or not. In the browser and in StackED, a click on a box
  ticks it or not, a click on `+ add a command or a tool…` opens the line
  to type one in, and the mouse over a box picks its row and its column.
  `a` (or Enter on `+ add a command or a tool…`) opens that line too, and
  `s` saves. Esc (or `q`) is as in the comment editor: with something
  changed, a menu, `Save it`, `Throw it away`, `Keep choosing` (Esc there
  keeps choosing too); with nothing changed, out, nothing written. Ctrl-c
  leaves at once, nothing written.

  `worktrees`, where the agents' worktrees go, else `.stacked/` at the top
  of the stack's repository. `chain`, whether a stack's asks are chained
  (Asking the agent, above; on unless set): a change does not wait for the
  review of the change before it, but goes from the stack that change
  makes, as if you accept it, one change running at a time, and an answer
  runs alongside the changes, on the stack as it is. `false` turns
  chaining off: a change then waits for the review of the change before it
  and for the running answers to land, and an answer waits for a running
  change to land. `schema`, whether the agent gives its answer
  as structured output, checked against what the ask asks for (claude's
  `--json-schema`: its replies by the comments' labels, an answer's
  comments, a build and test's findings by the patches' numbers; on
  unless set): off for an agent that does not take it, its answer is read
  from what it writes, the JSON it ends with, the prose around it left
  out. Given up on by claude, the ask fails and says so. `reach_check`,
  whether the ask dialog checks, as it opens, that the network reaches the
  host claude talks to, and grays Ask Now while it does not (Asking the
  agent, above; on unless set): `false` turns that check off, for a setup
  where it is wrong though claude works, a proxy the check does not go
  through, say. The program run as claude is looked for all the same.
- `[reviews]` (built with the `moz-phab` feature, on by default; read
  either way, so one file serves both builds): `source`, `"phabricator"`
  or `"none"` (no pulling, of a stack or into a new one, no statuses of
  revisions read);
  `url`, the Phabricator a bare
  `D123` trailer is on; `arcrc`, where the tokens are; `token`, one for
  `url` instead; `trailer`, the trailer naming a revision
  (`Differential Revision`); `bugzilla`, the Bugzilla that a bug a commit
  message names opens on in your browser (`https://bugzilla.mozilla.org`;
  one that is not a web link to a site keeps it, and is said).
- `[view]`: `layout` at the start (`unified`, `side-by-side`, `compact`,
  `unified-split`); `interdiff`, the view every interdiff starts in, on
  every screen that shows one (The editor, above): `"interdiff"`, a sign
  for which version has a line before that version's own sign, as stacked
  always drew them, unless set; `"columns"`, a column per version, what
  each one's patch does with each line; `"side-by-side"`, the old version
  on the left and the new one on the right; or the change from the old
  version to the new one as a patch of its own, `"change-unified"`,
  `"change-side-by-side"` or `"change-compact"` (what the dialog of `1`
  sets there holds while the editor is open, and is not written);
  `interdiff_lines`, the lines every interdiff starts with, in every view
  of them (The editor, above): `"every"` line, as stacked always drew
  them, unless set; or only what the patches `"changed"`, the lines both
  versions have alike, the context lines of their diffs, their index lines
  and the old one of two hunk headers that differ only in their numbers
  left out, and of the change, the lines the change of base made (what the
  dialog of `1` sets there holds while the editor is open, and is not
  written);
  `reading`, to start reading; `whitespace_needs_review`, whether changed
  lines that are empty or white space alone, the message's empty lines and
  moz-phab's trailer need review in the reading mode (`false`: they need no
  mark, and no count holds them); `patches`, the
  patches pane at the start, `"expanded"`, `"files"` (the files of the
  patch picked listed under it; `"compact"`, as versions before the files
  wrote it, is read as them) or `"hidden"`; `comments`, the comments pane
  at the start, `"expanded"`, `"compact"` or `"hidden"` (for both, `true`
  and `false`, as before, are expanded and hidden); `stream_lines`, the
  agent's lines under a comment;
  `split_width`, the columns two sides, or two halves, need, the middle's
  for the code and a pane's for its interdiffs side by side, or the change
  side by side or compact; and
  `message_width`, the columns the menus wrap a commit message's long
  lines at (Editing a patch by hand, above): 72 unless set, and 20 at
  least.
- `[colors]`: what paints each part of the screen, as git writes a colour
  (`git config color.diff.new "green bold"`): a foreground, then a
  background, each a name (`black`, `red`, `green`, `yellow`, `blue`,
  `magenta`, `cyan`, `white`, any of these after `bright`, `default`,
  `normal`), a number from 0 to 255, or `#rrggbb`; and any of `bold`, `dim`,
  `italic`, `ul`, `blink`, `reverse`, `strike`. `added` (`green`), `removed`
  (`red`), `added_word` (`green 22`) and `removed_word` (`red 52`, on a
  dark tint): what changed in a line almost the same as the one paired with it,
  `hunk` (`magenta`), `file` (`cyan bold`: a file's name, the
  titles of boxes, the history's records), `meta` (`blue`: index lines,
  borders, scroll bars), `header` (`cyan bold`: each pane's first row); `you` (`172`),
  `agent` (`134`) and `reviewer` (`32`): the comments and turns of each,
  their bar and name; `note` (`yellow`: what a menu says under its items, the
  tour's box, the comment editor's frame, a run going); `bar` (`brightwhite
  blue`), `warn` (`black yellow`: the top bar when the stack cannot change),
  `status` (`black brightwhite`: the bottom line), `cursor` (`reverse`: the
  line, the item or the pick the cursor is on, in the terminal's own colours
  reversed, white on a dark theme; the cursor looks the same on every line
  whatever the line's own colours, and a line read is not dim under it, but
  what changed in a line almost the same as its pair keeps its tint under
  the cursor), `picked` (`normal 238`: what a pane has picked while the
  cursor is in another, on a grey background, the cursor's own pane drawing
  its pick as `cursor` paints it) and `code` (`252
  236`: code in what a comment says, `code` in a line and code blocks, light
  grey on a dark grey). `stacked config colors.you 208` sets one.
- `[keys]`: for each thing a key does, its keys: `down = ["down", "j"]`,
  `menu = "m"`, `find_next = "ctrl-n"`. Each thing is read in one
  context, where the editor reads its keys, and the keys are checked
  context by context: two things of one context on one key clash, which
  is said as what else is wrong is (below), and the first does it; one key
  can do a thing in each context, each read where it is. The contexts, as
  `stacked config` lists them: `screens`, the screens and the menus over
  them, wherever nothing is typed (`up`, `down`, `left`, `right`,
  `page_up`, `page_down`, `home`, `end`, `enter`, `menu`, `help`, `back`,
  `delete`, `find`, `view`, `read`, `whole`, `open_link`, `quit`); `search`, the
  search of the middle while its bar is open (`find_next`, Tab, and
  `find_previous`, shift-Tab). `keep_stack` and `keep_workspace`, the keys
  of the sync of the commit messages, went with it: a settings' file that
  still names them is told so (`keys.keep_stack is no longer supported: the
  sync of the commit messages it picked in is gone; it is passed over`),
  `stacked config` refuses to write them, and `stacked config --unset`
  takes them out. `delete` is `["delete", "backspace"]`
  unless the settings say otherwise, as a Mac's delete key is Backspace;
  `delete = "d"` makes it d alone, and Delete and Backspace then do
  nothing on the screens. The search keeps for itself Esc, Enter, ↑
  and ↓, ctrl-c, and the keys that type or edit what is typed, and
  neither of its own keys can be one of those. So `menu = "tab"` makes Tab the menu on the
  screens, and Tab still goes to the next match in the search. In a menu,
  Tab and shift-Tab go from one dropdown of an item to the next (The
  editor, above), unless a setting gives them to something on the screens.
  The comment
  editor, a line typed in a box, and the search's own screen read what is
  typed, whatever the keys, and the bottom bar names their keys as they
  are there (`enter: a new line · esc: close`).
- `[web]` (built with the `web` feature, on by default; read either way,
  so one file serves both builds), read from `~/.stacked/config.toml`
  alone: `port`, the port of 127.0.0.1 `stacked web` listens on unless
  `--port N` names one for that run (In a browser, above): `"random"`
  (unless set), any free one each time; `"fixed-random"`, any free one the
  first time, which `stacked web` then writes in the file as `kept_port`,
  beside `port`, the rest of the file as it was (as `stacked config`
  writes a setting, once what it makes reads back), and listens on each
  time after, whatever the database; or a port from 1 to 65535, a number
  (`port = 8080`; one in quotes is read as well). Any other value, of any
  kind, is said (`web.port: "sometimes" is not "random", "fixed-random" or
  a port from 1 to 65535`), and `"random"` kept; `stacked config` refuses
  to write it (`web.port is "random", "fixed-random" or a port from 1 to
  65535, not "sometimes"`). `kept_port`, the port
  `"fixed-random"` took: while `port` is another, it is passed over, and
  kept, so that going back to `"fixed-random"` finds it again; taken out
  (`stacked config --unset web.kept_port`), the next launch with
  `"fixed-random"` takes any free one again, and writes it. A file it
  cannot be written in, or that would not read back with it, is said
  (`port 51234 cannot be kept in the settings as web.kept_port, so the
  next launch with web.port = "fixed-random" takes any free port again:
  …`), and the port taken is listened on all the same. With
  `"fixed-random"` or a port, the token of the address is kept too, for the
  next launch on that port, which gives the tab already open there 2
  seconds to come back before it takes a new token and opens a new tab (In
  a browser, above): never in this file, which you may share, but in
  `web-token.json` beside `state.json`, a file that only you may read or
  write. With `"random"`, or `--port`, the token is new each launch, and
  that file is neither read nor written: a token kept there stays as it
  was, unused, until a launch on its port fixed again. A port in use, set,
  kept or given, is said in a line, `port 8080 of 127.0.0.1 is in use`,
  and `stacked web` exits with 1, trying no other. `notifications`, `true`
  unless set: the page is sent the editor's notices, of every stack, which
  it shows as the system's banners while it is not what you look at (In a
  browser, above); `false` sends none, the bell ringing all the same
  unless `bell` is `false` too. Any other value, of any kind, is
  said (`web.notifications: "off" is not true or false`), and `true` kept;
  `stacked config` refuses to write it (`web.notifications is true or
  false, not "off"`). The browser keeps its permission to show them for
  the page's address, its port included: with `port = "random"` it asks
  at each launch, and with `"fixed-random"` or a port, once. `bell`,
  `true` unless set: the page rings its bell for what the notices tell,
  once for what the editor learned at once, while it is not what you look
  at, whether `notifications` is `true` or `false`, and whether the
  browser shows the banners or refused them (In a browser, above);
  `false` rings none, and with both `false` the editor keeps no notice.
  Any other value, of any kind, is said (`web.bell: "quiet" is not true or
  false`), and `true` kept; `stacked config` refuses to write it
  (`web.bell is true or false, not "quiet"`). `true`, the bell rings only
  once a click or a key reached the page, as a browser lets a page make
  sound only then. `editor`, where a terminal editor runs (the settings'
  `editor`, else `$VISUAL`, `$EDITOR`, vi): `"page"` (unless set), in the
  page itself, in a terminal of `stacked web`'s own that the page shows and
  sends its keys to, but those with ⌘; or `"terminal"`, in the terminal
  `stacked web` was started from, the page saying so meanwhile (In a
  browser, above). An editor with a window of its own (code, zed, gvim and
  the like) opens that window either way. Any other value, of any kind, is
  said (`web.editor: "window" is not "page" or "terminal"`), and `"page"`
  kept; `stacked config` refuses to write it (`web.editor is "page" or
  "terminal", not "window"`). Nothing of
  it is kept in a database, nor in `state.json`. A checkout's own `.stacked/local.toml`
  that sets them has them passed over, each said (`web.port is read only
  from the settings' file, never from a checkout's own .stacked/local.toml:
  here it is passed over`), and not marked as the checkout's by `stacked
  config`; `stacked config --local` refuses to write or read them there,
  and takes them out.
- `[lint]`: `command`, the linter of a checkout, which the stack's `Lint the
  stack` runs, and which asks that change the stack may run on the files
  they change (Linting, above): a checkout's own setting alone, read only
  from its `.stacked/local.toml`, which `stacked setup lint` writes; `""`,
  lint turned off there; unset, `./mach lint --fix` where `./mach` is at the
  checkout's top. `stacked config` shows it as a comment, so that what it
  prints, kept as `~/.stacked/config.toml`, sets no linter there.
- `[demo]`: `seen`, written by `stacked demo` the first time it is opened:
  the tour then starts on the list of its sections. `seen = false` shows
  the first tour again.

A file stacked cannot read, or with a setting it does not know, is not
used at all: the editor's bar says why, as every command does. A setting
with a wrong value keeps its default, and that is said too.

The settings were once in `~/.stacked.toml`, and StackED's state in
~/Library/Application Support/StackED/state.json. The first run of yours
that finds either, with nothing in its place in ~/.stacked, moves it there
and says so on stderr (StackED opened as an app, from the Dock or from its
launcher, says so in an alert);
a link moves as a link, to the same file. A run that finds both reads the
one in ~/.stacked, never writes over it, and says each time that the other
is no longer read. Nothing is moved when `$STACKED_HOME`, or the file's own
override, names where it is to be; nor by what stacked runs itself (an
ask's `stacked land`, the shell's completion), which reads the file where
it is until a run of yours moves it.

A checkout's own settings, `.stacked/local.toml` at its top (beside the
stacks' worktrees, out of git's sight), override `~/.stacked/config.toml` there:
what it sets is its own, a list whole and a table's settings each (`[keys]
down` alone, the other keys as they were); what it does not set, or sets
wrong, is the other file's; but for `[web]`, which is read from
`~/.stacked/config.toml` alone (above). The settings used are those of the checkout
stacked runs in, but for what a stack's agent does (`[agent]`: its tools,
where it works, the program run): those are of the stack's checkout,
wherever stacked runs, and read when an ask starts, so that a change
counts from the next ask, even with the editor open. `stacked config --local KEY
VALUE…` writes one there, `stacked config --local --unset KEY` takes it
out, `stacked config --local KEY` reads it, and `stacked config --local`
prints the file. `stacked config` names both files and marks each setting
the checkout's own sets (`# .stacked/local.toml`); one written in
`~/.stacked/config.toml` that the checkout's own sets too is said to be the
checkout's there. A `.stacked/local.toml` that git tracks is neither read
nor written, and that is said: it came with the repository, and a setting
can name a program to run (`agent.command`, `editor`).
