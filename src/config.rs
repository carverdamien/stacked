//! The settings, in `~/.stacked.toml` (or the file `$STACKED_CONFIG`
//! names): the database opened by default, the editor and how it is told a
//! line, the agent claude's asks run, where reviews come from, how the
//! editor starts, and the keys. Every one is optional, and what is not set
//! is as it would be with no file. A file that cannot be read is said, and
//! not used: the defaults are. `stacked config` prints every setting as it
//! is, in the file's form.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::Deserialize;

use crate::keys::{Key, Keymap};

/// How the code is drawn at the start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Layout {
    #[default]
    Unified,
    Split,
    Compact,
}

impl Layout {
    pub fn name(self) -> &'static str {
        match self {
            Layout::Unified => "unified",
            Layout::Split => "side-by-side",
            Layout::Compact => "compact",
        }
    }

    fn parse(s: &str) -> Option<Layout> {
        match s {
            "unified" => Some(Layout::Unified),
            "side-by-side" | "split" => Some(Layout::Split),
            "compact" => Some(Layout::Compact),
            _ => None,
        }
    }
}

/// Where reviews are pulled from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Source {
    #[default]
    Phabricator,
    /// Nowhere: no pulling.
    None,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Agent {
    /// The program run as claude; else `$STACKED_CLAUDE`'s, else `claude`.
    pub command: Option<String>,
    /// Arguments added to every run.
    pub args: Vec<String>,
    /// `--model` for every run.
    pub model: Option<String>,
    /// Where the worktrees made for asks go; else the system's temp dir.
    pub worktrees: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Reviews {
    pub source: Source,
    /// The Phabricator a bare `D123` is on, and whose token is used.
    pub url: Option<String>,
    /// Where tokens are: moz-phab's and arc's file.
    pub arcrc: PathBuf,
    /// A token to use rather than the file's.
    pub token: Option<String>,
    /// The trailer naming a patch's revision.
    pub trailer: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct View {
    pub layout: Layout,
    /// Reading rather than commenting, at the start.
    pub reading: bool,
    /// The patches shown on the left, at the start.
    pub patches: bool,
    /// The comments shown on the right when there are any, at the start.
    pub comments: bool,
    /// The lines of claude's stream under a comment it works on.
    pub stream_lines: usize,
    /// The columns the middle needs for two sides.
    pub split_width: usize,
}

impl Default for View {
    fn default() -> View {
        View {
            layout: Layout::Unified,
            reading: false,
            patches: true,
            comments: true,
            stream_lines: 3,
            split_width: 60,
        }
    }
}

/// Every setting, defaults filled in.
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    /// The database opened when none is named, nor `$STACKED_FILE`.
    pub file: Option<PathBuf>,
    /// The editor; else `$VISUAL`, else `$EDITOR`, else vi.
    pub editor: Option<String>,
    pub agent: Agent,
    pub reviews: Reviews,
    pub view: View,
    pub keys: Keymap,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            file: None,
            editor: None,
            agent: Agent::default(),
            reviews: Reviews {
                source: Source::Phabricator,
                url: None,
                arcrc: home().join(".arcrc"),
                token: None,
                trailer: "Differential Revision".into(),
            },
            view: View::default(),
            keys: Keymap::default(),
        }
    }
}

// -- the file as written ------------------------------------------------- //

#[derive(Debug, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct Written {
    file: Option<String>,
    editor: Option<String>,
    agent: WrittenAgent,
    reviews: WrittenReviews,
    view: WrittenView,
    keys: BTreeMap<String, OneOrMore>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct WrittenAgent {
    command: Option<String>,
    args: Vec<String>,
    model: Option<String>,
    worktrees: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct WrittenReviews {
    source: Option<String>,
    url: Option<String>,
    arcrc: Option<String>,
    token: Option<String>,
    trailer: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct WrittenView {
    layout: Option<String>,
    reading: Option<bool>,
    patches: Option<bool>,
    comments: Option<bool>,
    stream_lines: Option<usize>,
    split_width: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum OneOrMore {
    One(String),
    More(Vec<String>),
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default()
}

/// `~/x` as a path in $HOME.
pub fn expand(p: &str) -> PathBuf {
    match p.strip_prefix("~/") {
        Some(rest) => home().join(rest),
        None if p == "~" => home(),
        None => PathBuf::from(p),
    }
}

impl Config {
    /// The settings `text` says, and what it got wrong: settings it got
    /// wrong keep their defaults. Unreadable as a whole, an error.
    pub fn parse(text: &str) -> Result<(Config, Vec<String>), String> {
        let w: Written = toml::from_str(text).map_err(|e| e.to_string().trim().to_string())?;
        let mut c = Config::default();
        let mut wrong = Vec::new();
        c.file = w.file.as_deref().map(expand);
        c.editor = w.editor.filter(|e| !e.trim().is_empty());
        c.agent = Agent {
            command: w.agent.command.filter(|e| !e.trim().is_empty()),
            args: w.agent.args,
            model: w.agent.model,
            worktrees: w.agent.worktrees.as_deref().map(expand),
        };
        if let Some(s) = w.reviews.source {
            match s.as_str() {
                "phabricator" => c.reviews.source = Source::Phabricator,
                "none" => c.reviews.source = Source::None,
                _ => wrong.push(format!("reviews.source: {s:?} is neither \"phabricator\" nor \"none\"")),
            }
        }
        c.reviews.url = w.reviews.url.map(|u| u.trim_end_matches('/').to_string());
        if let Some(a) = w.reviews.arcrc {
            c.reviews.arcrc = expand(&a);
        }
        c.reviews.token = w.reviews.token;
        if c.reviews.token.is_some() && c.reviews.url.is_none() {
            wrong.push("reviews.token: it needs reviews.url, the Phabricator it is for".into());
            c.reviews.token = None;
        }
        if let Some(t) = w.reviews.trailer {
            c.reviews.trailer = t.trim_end_matches(':').to_string();
        }
        let v = w.view;
        if let Some(l) = v.layout {
            match Layout::parse(&l) {
                Some(l) => c.view.layout = l,
                None => wrong.push(format!("view.layout: {l:?} is not unified, side-by-side or compact")),
            }
        }
        c.view.reading = v.reading.unwrap_or(c.view.reading);
        c.view.patches = v.patches.unwrap_or(c.view.patches);
        c.view.comments = v.comments.unwrap_or(c.view.comments);
        c.view.stream_lines = v.stream_lines.unwrap_or(c.view.stream_lines);
        if let Some(n) = v.split_width {
            if n < 20 {
                wrong.push(format!("view.split_width: {n} is too narrow for two sides"));
            } else {
                c.view.split_width = n;
            }
        }
        let named: Vec<(String, Vec<String>)> = w
            .keys
            .into_iter()
            .map(|(k, v)| {
                let v = match v {
                    OneOrMore::One(s) => vec![s],
                    OneOrMore::More(v) => v,
                };
                (k, v)
            })
            .collect();
        let (keys, bad) = Keymap::from_names(&named);
        c.keys = keys;
        wrong.extend(bad);
        Ok((c, wrong))
    }

    /// The settings in the file at `path`, if there is one; and what was
    /// wrong with it, each said with where.
    pub fn load(path: &Path) -> (Config, Vec<String>) {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (Config::default(), vec![]),
            Err(e) => return (Config::default(), vec![format!("{}: {e}", path.display())]),
        };
        match Config::parse(&text) {
            Ok((c, wrong)) => (c, wrong.into_iter().map(|w| format!("{}: {w}", path.display())).collect()),
            Err(e) => (
                Config::default(),
                vec![format!("{}: {e}; none of it is used", path.display())],
            ),
        }
    }

    /// The editor: the setting's, else $VISUAL's, $EDITOR's, or vi.
    pub fn editor(&self) -> String {
        self.editor
            .clone()
            .or_else(|| std::env::var("VISUAL").ok().filter(|e| !e.trim().is_empty()))
            .or_else(|| std::env::var("EDITOR").ok().filter(|e| !e.trim().is_empty()))
            .unwrap_or_else(|| "vi".into())
    }

    /// The shell command that edits the file `"$1"`, at `line` if there is
    /// one: the editor, with `{file}` and `{line}` put where it says; else
    /// told the line as it takes it, when it is an editor known to take
    /// one.
    pub fn editing(&self, line: Option<usize>) -> String {
        let editor = self.editor();
        let at = line.unwrap_or(1).max(1);
        if editor.contains("{file}") || editor.contains("{line}") {
            let cmd = editor.replace("{line}", &at.to_string());
            return if cmd.contains("{file}") {
                cmd.replace("{file}", "\"$1\"")
            } else {
                format!("{cmd} \"$1\"")
            };
        }
        let Some(line) = line else {
            return format!("{editor} \"$1\"");
        };
        let program = editor
            .split_whitespace()
            .next()
            .unwrap_or("")
            .rsplit('/')
            .next()
            .unwrap_or("");
        match program {
            "vi" | "vim" | "nvim" | "view" | "gvim" | "mvim" | "nano" | "pico" | "emacs" | "emacsclient" | "kak"
            | "joe" | "jed" | "mg" | "ne" => format!("{editor} +{line} \"$1\""),
            "hx" | "helix" | "subl" | "sublime_text" | "zed" | "zeditor" => format!("{editor} \"$1\":{line}"),
            "code" | "code-insiders" | "codium" | "cursor" | "windsurf" => format!("{editor} --goto \"$1\":{line}"),
            _ => format!("{editor} \"$1\""),
        }
    }

    /// The program run as claude: `$STACKED_CLAUDE`'s, else the setting's,
    /// else `claude`.
    pub fn agent_command(&self) -> String {
        std::env::var("STACKED_CLAUDE")
            .ok()
            .filter(|c| !c.is_empty())
            .or_else(|| self.agent.command.clone())
            .unwrap_or_else(|| "claude".into())
    }

    /// The arguments added to every run of claude.
    pub fn agent_args(&self) -> Vec<String> {
        let mut out = self.agent.args.clone();
        if let Some(m) = &self.agent.model {
            out.extend(["--model".to_string(), m.clone()]);
        }
        out
    }

    /// Where the worktree of ask `id` goes.
    pub fn worktree(&self, id: &str) -> PathBuf {
        self.agent
            .worktrees
            .clone()
            .unwrap_or_else(std::env::temp_dir)
            .join(format!("stacked-{id}"))
    }

    /// Every setting as it is, in the file's form, each said: what `stacked
    /// config` prints, to start a file from.
    pub fn render(&self) -> String {
        let q = |s: &str| format!("{s:?}");
        let opt = |name: &str, v: &Option<String>, unset: &str| match v {
            Some(v) => format!("{name} = {}\n", q(v)),
            None => format!("# {name} = ...   ({unset})\n"),
        };
        let path = |p: &Path| q(&p.to_string_lossy());
        let list = |v: &[String]| format!("[{}]", v.iter().map(|s| q(s)).collect::<Vec<_>>().join(", "));
        let mut out = String::new();
        out.push_str("# stacked's settings. Each one is optional: what is not set is as below.\n\n");
        out.push_str("# The database opened when none is named, nor $STACKED_FILE.\n");
        out.push_str(&match &self.file {
            Some(f) => format!("file = {}\n", path(f)),
            None => "# file = \"stacked.jsonl\"   (in the directory it is run in)\n".into(),
        });
        out.push_str(
            "\n# The editor, for comments, patches, the stack as a list and the code. {file} and\n\
             # {line} go where it says; without them, the file goes last, after the line as the\n\
             # editor takes it: +LINE for vi, vim, nvim, nano, emacs, kak; FILE:LINE for hx,\n\
             # zed, subl; --goto FILE:LINE for code.\n",
        );
        out.push_str(&opt("editor", &self.editor, "$VISUAL, else $EDITOR, else vi"));
        out.push_str("\n[agent]\n# The program asks run as claude: it is given claude's arguments.\n");
        out.push_str(&opt("command", &self.agent.command, "$STACKED_CLAUDE, else claude"));
        out.push_str("# Arguments added to every run.\n");
        out.push_str(&format!("args = {}\n", list(&self.agent.args)));
        out.push_str(&opt("model", &self.agent.model, "claude's own"));
        out.push_str("# Where the git worktrees made for asks go.\n");
        out.push_str(&match &self.agent.worktrees {
            Some(w) => format!("worktrees = {}\n", path(w)),
            None => "# worktrees = ...   (the system's temp dir)\n".into(),
        });
        out.push_str("\n[reviews]\n# Where `pull` reads reviews from: \"phabricator\", or \"none\".\n");
        out.push_str(&format!(
            "source = {}\n",
            q(match self.reviews.source {
                Source::Phabricator => "phabricator",
                Source::None => "none",
            })
        ));
        out.push_str("# The Phabricator a trailer's bare D123 is on, and whose token is used.\n");
        out.push_str(&opt("url", &self.reviews.url, "the host each trailer's link names"));
        out.push_str("# Where the tokens are, as moz-phab and arc keep them.\n");
        out.push_str(&format!("arcrc = {}\n", path(&self.reviews.arcrc)));
        out.push_str("# A token for url, rather than the file's.\n");
        out.push_str(&match &self.reviews.token {
            Some(_) => "token = \"...\"   # set\n".to_string(),
            None => "# token = \"api-...\"\n".into(),
        });
        out.push_str("# The trailer naming a patch's revision.\n");
        out.push_str(&format!("trailer = {}\n", q(&self.reviews.trailer)));
        let v = &self.view;
        out.push_str("\n[view]\n# How code is drawn at the start: \"unified\", \"side-by-side\" or \"compact\" (1 cycles them).\n");
        out.push_str(&format!("layout = {}\n", q(v.layout.name())));
        out.push_str("# Reading rather than commenting, at the start (2 switches).\n");
        out.push_str(&format!("reading = {}\n", v.reading));
        out.push_str("# The patches on the left, and the comments on the right, shown at the start.\n");
        out.push_str(&format!("patches = {}\ncomments = {}\n", v.patches, v.comments));
        out.push_str("# The lines of claude's stream under a comment it works on.\n");
        out.push_str(&format!("stream_lines = {}\n", v.stream_lines));
        out.push_str("# The columns the middle needs for two sides.\n");
        out.push_str(&format!("split_width = {}\n", v.split_width));
        out.push_str(
            "\n[keys]\n# The keys for each thing a key does, outside the comment editor (which reads\n\
             # what is typed): up, down, left, right, pageup, pagedown, home, end, enter,\n\
             # space, esc, tab, f1..f24, a character, any after ctrl- or alt-. Nothing else\n\
             # is a key.\n",
        );
        for k in Key::ALL {
            out.push_str(&format!("{} = {}\n", k.name(), list(&self.keys.names(k))));
        }
        out
    }
}

/// The settings in use, read once, and what was wrong with their file.
static LOADED: OnceLock<(Config, Vec<String>, Option<PathBuf>)> = OnceLock::new();

/// Where the settings are read from: `$STACKED_CONFIG`, else
/// `~/.stacked.toml`.
pub fn path() -> PathBuf {
    match std::env::var_os("STACKED_CONFIG") {
        Some(p) if !p.is_empty() => PathBuf::from(p),
        _ => home().join(".stacked.toml"),
    }
}

fn loaded() -> &'static (Config, Vec<String>, Option<PathBuf>) {
    LOADED.get_or_init(|| {
        let p = path();
        let (c, wrong) = Config::load(&p);
        (c, wrong, Some(p))
    })
}

/// The settings in use.
pub fn get() -> &'static Config {
    &loaded().0
}

/// What was wrong with the settings' file, if anything.
pub fn warnings() -> &'static [String] {
    &loaded().1
}

/// Uses `c` rather than any file, unless the settings are read already:
/// for the tests, which share one process.
pub fn init(c: Config) {
    let _ = LOADED.set((c, vec![], None));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_sets_what_it_says_and_says_what_is_wrong() {
        let (c, wrong) = Config::parse(
            r#"
            file = "~/stacks.jsonl"
            editor = "hx {file}:{line}"
            [agent]
            command = "my-claude"
            model = "opus"
            [reviews]
            source = "nowhere"
            url = "https://phab.example.com/"
            [view]
            layout = "compact"
            reading = true
            [keys]
            up = ["up", "k"]
            menu = "m"
            "#,
        )
        .unwrap();
        assert_eq!(c.file, Some(home().join("stacks.jsonl")));
        assert_eq!(c.agent_args(), ["--model", "opus"]);
        assert_eq!(c.reviews.url.as_deref(), Some("https://phab.example.com"));
        assert_eq!(c.reviews.source, Source::Phabricator, "what is wrong keeps its default");
        assert_eq!(wrong, ["reviews.source: \"nowhere\" is neither \"phabricator\" nor \"none\""]);
        assert_eq!((c.view.layout, c.view.reading), (Layout::Compact, true));
        assert_eq!(c.keys.label(Key::Menu), "m");
        assert_eq!(c.editing(Some(12)), "hx \"$1\":12");
        // What is not a setting at all: the file is not used.
        let e = Config::parse("colour = \"red\"\n").unwrap_err();
        assert!(e.contains("unknown field `colour`"), "{e}");
    }

    #[test]
    fn the_editor_is_told_the_line_as_it_takes_it() {
        let with = |e: &str| Config {
            editor: Some(e.into()),
            ..Config::default()
        };
        assert_eq!(with("vim").editing(Some(7)), "vim +7 \"$1\"");
        assert_eq!(with("/usr/bin/nvim -u NONE").editing(Some(7)), "/usr/bin/nvim -u NONE +7 \"$1\"");
        assert_eq!(with("hx").editing(Some(7)), "hx \"$1\":7");
        assert_eq!(with("code --wait").editing(Some(7)), "code --wait --goto \"$1\":7");
        assert_eq!(with("ed").editing(Some(7)), "ed \"$1\"", "one not known is given the file alone");
        assert_eq!(with("vim").editing(None), "vim \"$1\"");
        assert_eq!(with("kak +{line}").editing(Some(3)), "kak +3 \"$1\"");
    }

    #[test]
    fn what_is_printed_reads_back_the_same() {
        let (c, wrong) = Config::parse(
            "editor = \"vim\"\n[agent]\nargs = [\"--verbose\"]\n[view]\nsplit_width = 80\n[keys]\ndown = [\"down\", \"j\"]\n",
        )
        .unwrap();
        assert!(wrong.is_empty(), "{wrong:?}");
        let (again, wrong) = Config::parse(&c.render()).unwrap();
        assert!(wrong.is_empty(), "{wrong:?}");
        assert_eq!(again, c);
        let (defaults, _) = Config::parse(&Config::default().render()).unwrap();
        assert_eq!(defaults, Config::default());
    }
}
