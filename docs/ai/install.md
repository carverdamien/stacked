# Install

```sh
rustup target add wasm32-unknown-unknown
cargo install --git https://github.com/carverdamien/stacked
```

or, in a clone of this repository, `cargo install --path .`; or run it in
place with `target/release/stacked` after `cargo build --release`. It needs
`git`, and an agent for asks (`claude`, unless the settings name another).

The five features below are on by default, so that a build has them all.
`--no-default-features` builds stacked with none of them, and `--features`
then names those to keep: `cargo install --path . --no-default-features
--features demo,moz-phab`, for instance, builds stacked with the tour and
Phabricator, but without the editor in a browser, StackED or the try push.

The `web` feature adds the editor in a browser (`stacked web`, below). Its
browser half is compiled to wasm, with Rust's wasm32-unknown-unknown
target, which `rustup target add` installs (above). Without that target,
the build stops and says to add it, or to build stacked without `web`, as
`--no-default-features --features app,demo,moz-phab,try-push` does.

The `demo` feature adds a tour of the editor, to learn it by (`stacked
demo`, below).

The `moz-phab` feature adds Phabricator (below): the reviews pulled from
its revisions, the latest of them fetched and stored, stacks imported from
them, and a patch submitted to it as work in progress, over Conduit with
the token moz-phab (or arc) keeps,
which `curl` calls. With it, the reading mode
takes the trailer moz-phab writes in a message as an empty line, which
needs no reading (Reading). Built without it, stacked has none of it, but
reads a database that holds reviews pulled.

The `try-push` feature adds the try push of Firefox's CI (Try push,
below): a comment asked as a try push has the agent select the CI tasks it
asks for, each with why, and those tasks wait for your review, to accept,
reject or answer; those you accept are pushed to try with `./mach try
fuzzy`, its link a reply on the comment, and the push is followed on Lando
and Treeherder until its tasks are done, its summary a reply for your
review. Built without it, stacked has none
of it, but reads a database and a settings' file that a build with it
wrote: an ask of a try push is read as one to answer only, the try push's
list of tools is passed over, and so is what a push's run keeps (it reads
as a run of its own, `try-push`).

The `app` feature adds StackED, the editor in a window of its own
(`stacked app`, below). Its window is drawn by eframe (egui), on OpenGL,
its dialogs are rfd's, and its fonts are the system's, as fontdb finds
them: the build fetches them all, and on macOS and on Linux it needs
nothing more. `stacked app --bundle` then makes StackED an app of the
system's: StackED.app on macOS, for the Dock; elsewhere, a launcher among
the desktop's applications.

On Linux, StackED loads what it needs of the system only when it starts,
and a Linux desktop has all of it: X11's libraries or Wayland's,
xkbcommon's and OpenGL's (EGL or GLX), for its windows (eframe); D-Bus's
library and the desktop portal, for the Open and Save dialogs (rfd), else
zenity, which also shows the alerts; and DejaVu Sans Mono with DejaVu
Sans, for the text, else egui's own Hack. On a Debian or Ubuntu system
that lacks them, this installs them all:

```sh
sudo apt install libx11-xcb1 libxcursor1 libxi6 libxrandr2 libxkbcommon-x11-0 \
  libwayland-client0 libwayland-egl1 libegl1 libgl1 \
  libdbus-1-3 xdg-desktop-portal xdg-desktop-portal-gtk zenity fonts-dejavu-core
```

## Completion in zsh

`stacked completions zsh` prints stacked's completion for zsh. Keep it
where zsh finds its functions:

```sh
mkdir -p ~/.zfunc && stacked completions zsh > ~/.zfunc/_stacked
# and in ~/.zshrc, before compinit runs: fpath=(~/.zfunc $fpath)
```

or have ~/.zshrc read it, once compinit has run: `source <(stacked
completions zsh)`. TAB then offers stacked's commands and their options,
each with what it does, the values an option lists (`--show`'s), files
where a file goes, and from the database (`-f`'s, else the one stacked
opens) its stacks, a stack's patches with their titles, its comments (to
reply to, or send) and the agent's runs. The function asks stacked each
time (`stacked __complete`), so it is never out of date with it.
