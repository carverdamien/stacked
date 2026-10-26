//! Completion for the shell: what `stacked __complete` offers for the word
//! being typed, from stacked's own commands and their options, and from the
//! database (its stacks, their patches and comments, the agent's runs);
//! and the function zsh asks it with, which `stacked completions zsh`
//! prints.

use std::path::{Path, PathBuf};

use clap::{Arg, ArgAction, Command, ValueHint};

use crate::db::{Db, RunState, short_id};

/// What is offered for the word being typed: words, each with what it is;
/// or files, which the shell completes itself.
#[derive(Debug, PartialEq)]
pub enum Offer {
    Words(Vec<(String, String)>),
    Files,
}

impl Offer {
    /// As `stacked __complete` prints it, for the zsh function: `:files`,
    /// else a line for each word, `word:what it is`, a `:` in the word
    /// escaped as zsh's `_describe` reads it.
    pub fn lines(&self) -> String {
        match self {
            Offer::Files => ":files\n".into(),
            Offer::Words(words) => words
                .iter()
                .map(|(w, about)| match about.is_empty() {
                    true => format!("{}\n", w.replace(':', "\\:")),
                    false => format!("{}:{}\n", w.replace(':', "\\:"), about.replace('\n', " ")),
                })
                .collect(),
        }
    }
}

/// The function zsh completes stacked with: what `stacked __complete` offers
/// for the words up to the cursor's. Kept where zsh finds its functions (as
/// `_stacked`, in `$fpath`), or read by `source`.
pub const ZSH: &str = r#"#compdef stacked
# stacked's completion, for zsh: what `stacked __complete` offers for the
# word being typed. Printed by `stacked completions zsh`.
_stacked() {
  local -a offered
  offered=("${(@f)$(command stacked __complete -- "${(@)words[2,CURRENT]}" 2>/dev/null)}")
  if [[ $offered[1] == :files ]]; then
    _files
  elif [[ -n $offered[1] ]]; then
    _describe stacked offered
  fi
}
if [[ $funcstack[1] == _stacked ]]; then
  _stacked "$@"
else
  compdef _stacked stacked
fi
"#;

/// What to offer for the last of `words`, the word being typed ("" for a
/// new one), the words before it on the command line after `stacked`, as
/// `cli` (built: its global options in each command) reads them. What is
/// offered from the database is from the one the words name (`-f`), else
/// from `default`'s.
pub fn complete(cli: &Command, words: &[String], default: &Path) -> Offer {
    let (typed, before) = match words.split_last() {
        Some((t, b)) => (t.as_str(), b),
        None => ("", &[][..]),
    };
    let mut cmd = cli;
    let mut file: Option<PathBuf> = None;
    let mut given: Vec<&str> = Vec::new();
    let mut options_done = false;
    let mut i = 0;
    while i < before.len() {
        let w = before[i].as_str();
        i += 1;
        if !options_done && w == "--" {
            options_done = true;
            continue;
        }
        if !options_done && w.starts_with('-') && w.len() > 1 {
            // An option: its value given with it after `=`, else the next
            // word, unless it takes none.
            let (name, with) = match w.split_once('=') {
                Some((n, v)) => (n, Some(v)),
                None => (w, None),
            };
            let Some(arg) = option(cmd, name).filter(|a| takes_value(a)) else { continue };
            let value = match with {
                Some(v) => v,
                None if i < before.len() => {
                    i += 1;
                    before[i - 1].as_str()
                }
                // Its value is the word being typed.
                None => return value_of(cmd, arg, &given, &db_at(file.as_deref(), default)),
            };
            if arg.get_id() == "file" {
                file = Some(PathBuf::from(value));
            }
            continue;
        }
        // A command of this one's, before anything else is given it.
        if given.is_empty()
            && let Some(sub) = cmd.find_subcommand(w)
        {
            cmd = sub;
            continue;
        }
        given.push(w);
    }
    let db = db_at(file.as_deref(), default);
    if !options_done && typed.starts_with('-') {
        return Offer::Words(options(cmd));
    }
    if given.is_empty() && cmd.has_subcommands() {
        // `stacked FILE` opens FILE: a path, as one is typed.
        if std::ptr::eq(cmd, cli) && (typed.contains('/') || typed.starts_with(['.', '~'])) {
            return Offer::Files;
        }
        return Offer::Words(subcommands(cmd));
    }
    let positionals: Vec<&Arg> = cmd.get_positionals().filter(|a| !a.is_hide_set()).collect();
    let arg = positionals
        .get(given.len())
        .or_else(|| positionals.last().filter(|a| matches!(a.get_action(), ArgAction::Append)));
    match arg {
        Some(arg) => value_of(cmd, arg, &given, &db),
        None => Offer::Words(Vec::new()),
    }
}

/// The database `-f` names, else `default`.
fn db_at(file: Option<&Path>, default: &Path) -> PathBuf {
    file.unwrap_or(default).to_path_buf()
}

/// Whether `arg` is given a value.
fn takes_value(arg: &Arg) -> bool {
    arg.get_num_args().is_some_and(|n| n.takes_values())
}

/// The option `name` (`-f`, `--file`) of `cmd`, its global ones too.
fn option<'a>(cmd: &'a Command, name: &str) -> Option<&'a Arg> {
    cmd.get_arguments().find(|a| match name.strip_prefix("--") {
        Some(long) => a.get_long() == Some(long) || a.get_all_aliases().is_some_and(|al| al.contains(&long)),
        None => name.len() == 2 && a.get_short() == name.chars().nth(1),
    })
}

/// `cmd`'s options, each with what it is for: its long name, else its
/// short one.
fn options(cmd: &Command) -> Vec<(String, String)> {
    cmd.get_arguments()
        .filter(|a| !a.is_positional() && !a.is_hide_set())
        .filter_map(|a| {
            let name = match (a.get_long(), a.get_short()) {
                (Some(l), _) => format!("--{l}"),
                (None, Some(s)) => format!("-{s}"),
                _ => return None,
            };
            Some((name, about(a.get_help().map(ToString::to_string))))
        })
        .collect()
}

/// `cmd`'s commands, each with what it does.
fn subcommands(cmd: &Command) -> Vec<(String, String)> {
    cmd.get_subcommands()
        .filter(|s| !s.is_hide_set())
        .map(|s| (s.get_name().to_string(), about(s.get_about().map(ToString::to_string))))
        .collect()
}

/// The most a menu's line says of a word, about.
const ABOUT: usize = 60;

/// What `help` says first, as a menu has room for: up to its first colon,
/// full stop or semicolon; longer than `ABOUT`, up to a comma before it.
fn about(help: Option<String>) -> String {
    let help = help.unwrap_or_default();
    let line = help.lines().next().unwrap_or("").trim();
    let cut = [": ", ". ", "; "].iter().filter_map(|s| line.find(s)).filter(|&at| at >= 3).min();
    let mut line = line[..cut.unwrap_or(line.len())].trim_end_matches('.');
    if line.chars().count() > ABOUT
        && let Some(at) = line.char_indices().take_while(|(k, _)| *k < ABOUT).filter(|(k, _)| line[*k..].starts_with(", ")).last()
    {
        line = &line[..at.0];
    }
    line.to_string()
}

/// What `arg` of `cmd` may be, the words `given` it before: what it lists
/// (`--show`'s), files, or what the database at `path` holds: the stacks,
/// the patches of the stack given first, its comments, the runs.
fn value_of(cmd: &Command, arg: &Arg, given: &[&str], path: &Path) -> Offer {
    let listed: Vec<(String, String)> = arg
        .get_possible_values()
        .iter()
        .filter(|v| !v.is_hide_set())
        .map(|v| (v.get_name().to_string(), about(v.get_help().map(ToString::to_string))))
        .collect();
    if !listed.is_empty() {
        return Offer::Words(listed);
    }
    let id = arg.get_id().as_str();
    if matches!(arg.get_value_hint(), ValueHint::AnyPath | ValueHint::FilePath | ValueHint::DirPath)
        || matches!(id, "file" | "new" | "bundle")
    {
        return Offer::Files;
    }
    let Ok(db) = Db::load(path) else { return Offer::Words(Vec::new()) };
    let stack = given.first().copied().unwrap_or("");
    let words = match (id, cmd.get_name()) {
        // A new stack's name is the user's.
        ("name", "new") => Vec::new(),
        ("name", _) => db
            .stacks()
            .iter()
            .map(|s| (s.name.clone(), format!("{} patch{}", s.patches.len(), if s.patches.len() == 1 { "" } else { "es" })))
            .collect(),
        ("patch" | "patches", _) => db
            .stack(stack)
            .map(|s| {
                s.patches
                    .iter()
                    .enumerate()
                    .map(|(k, p)| ((k + 1).to_string(), db.patch(p).title().to_string()))
                    .collect()
            })
            .unwrap_or_default(),
        ("id" | "ids", "reply" | "send") => db
            .stack_notes(stack)
            .iter()
            .map(|n| (short_id(&n.change, &n.id), crate::thread::said_lines(&n.text).into_iter().next().unwrap_or_default()))
            .collect(),
        ("id", _) | ("run", _) => db
            .runs()
            .iter()
            .filter(|r| id == "id" || r.stack == stack)
            .rev()
            .map(|r| {
                let state = match r.state {
                    RunState::Started => "running",
                    RunState::Done => "done",
                    RunState::Failed => "failed",
                };
                (r.id.clone(), format!("{} · {state}", r.what))
            })
            .collect(),
        _ => Vec::new(),
    };
    Offer::Words(words)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::{Parser, Subcommand, ValueEnum};

    /// A command line as stacked's is made, smaller.
    #[derive(Parser)]
    struct Cli {
        /// The database.
        #[arg(short, long, global = true)]
        file: Option<PathBuf>,
        #[command(subcommand)]
        cmd: Option<Cmd>,
    }

    #[derive(Clone, Copy, ValueEnum)]
    enum Which {
        /// Every one.
        All,
        /// Not sent: yours to send.
        ToSend,
    }

    #[derive(Subcommand)]
    enum Cmd {
        /// Its patches: bottom first.
        Show {
            name: String,
            patch: Option<String>,
            /// A file as the patch leaves it.
            #[arg(long)]
            path: Option<String>,
        },
        /// Comments, as they stand.
        Comments {
            name: String,
            #[arg(long)]
            show: Option<Which>,
        },
        /// A new, empty stack.
        New { name: String },
        #[command(hide = true)]
        Hidden,
    }

    fn cli() -> Command {
        let mut c = <Cli as clap::CommandFactory>::command();
        c.build();
        c
    }

    fn offered(words: &[&str], db: &Path) -> Offer {
        let words: Vec<String> = words.iter().map(|w| w.to_string()).collect();
        complete(&cli(), &words, db)
    }

    fn names(o: Offer) -> Vec<String> {
        match o {
            Offer::Words(w) => w.into_iter().map(|(w, _)| w).collect(),
            Offer::Files => vec![":files".into()],
        }
    }

    #[test]
    fn the_commands_and_options_are_offered_as_clap_reads_them() {
        let none = Path::new("/nonexistent/stacked.jsonl");
        // The commands, but those hidden, each with what it does.
        match offered(&[""], none) {
            Offer::Words(w) => {
                assert_eq!(w.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(), ["show", "comments", "new", "help"]);
                assert_eq!(w[0].1, "Its patches");
            }
            o => panic!("{o:?}"),
        }
        // A path, to open: files.
        assert_eq!(offered(&["./st"], none), Offer::Files);
        // The options of the command given, the global ones too.
        let o = names(offered(&["show", "--"], none));
        assert!(o.contains(&"--path".into()) && o.contains(&"--file".into()) && o.contains(&"--help".into()), "{o:?}");
        // What an option lists: its values.
        assert_eq!(names(offered(&["comments", "s", "--show", ""], none)), ["all", "to-send"]);
        assert_eq!(names(offered(&["comments", "s", "--show=", ""], none)), Vec::<String>::new(), "given with =: done");
        // A file's: files.
        assert_eq!(offered(&["-f", ""], none), Offer::Files);
        assert_eq!(offered(&["show", "--file", ""], none), Offer::Files);
        // Past what a command takes: nothing.
        assert_eq!(names(offered(&["new", "x", ""], none)), Vec::<String>::new());
        assert_eq!(
            Offer::Words(vec![("a:b".into(), "it is".into()), ("c".into(), String::new())]).lines(),
            "a\\:b:it is\nc\n"
        );
    }
}
