use anyhow::Context;
use rustyline::{
    completion::{Completer, Pair},
    highlight::Highlighter,
    hint::{Hinter, HistoryHinter},
    history::DefaultHistory,
    validate::{ValidationContext, ValidationResult, Validator},
    Config, Editor, Helper, Result as RlResult,
};
use orbis_core::Shell;
use std::{borrow::Cow, env, fs, path::{Path, PathBuf}};

const VERSION: &str = env!("CARGO_PKG_VERSION");

const BUILTINS: &[&str] = &[
    "cd", "cs", "exit", "export", "unset", "jobs", "fg", "bg",
    "pwd", "echo", "clear", "env", "which", "type",
    "alias", "unalias", "help", "true", "false",
];

const ORBISBOX_CMDS: &[&str] = &[
    "ls", "cat", "cp", "mv", "rm", "mkdir", "rmdir", "touch", "ln",
    "chmod", "stat", "du", "df", "find",
    "head", "tail", "wc", "tee", "grep", "sort", "uniq", "cut", "tr",
    "seq", "diff", "xargs",
    "echo", "pwd", "which", "whoami", "uname", "date", "sleep", "yes",
    "env", "id", "ps", "kill",
    "basename", "dirname", "realpath",
];

struct OrbisHelper {
    commands: Vec<String>,
    hinter: HistoryHinter,
}

impl OrbisHelper {
    fn new() -> Self {
        Self {
            commands: collect_commands(),
            hinter: HistoryHinter::new(),
        }
    }

    fn complete_path(&self, word: &str, dirs_only: bool) -> Vec<String> {
        let home = env::var("HOME").unwrap_or_default();

        let expanded: String = if word == "~" {
            home.clone()
        } else if word.starts_with("~/") {
            format!("{}{}", home, &word[1..])
        } else {
            word.to_string()
        };

        let (search_dir, name_prefix, path_prefix) = if expanded.contains('/') {
            let p = Path::new(&expanded);
            let dir = p.parent().unwrap_or(Path::new(".")).to_path_buf();
            let prefix = p.file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            let dir_str = if dir.as_os_str().is_empty() {
                PathBuf::from(".")
            } else {
                dir.clone()
            };
            let base = format!("{}/", dir.display());
            let base = if base == "./" { String::new() } else { base };
            (dir_str, prefix, base)
        } else {
            (PathBuf::from("."), expanded.clone(), String::new())
        };

        let entries = match fs::read_dir(&search_dir) {
            Ok(e) => e,
            Err(_) => return vec![],
        };

        let mut completions = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.starts_with(&name_prefix) {
                continue;
            }

            let meta = entry.metadata().ok();
            let is_dir = meta.map(|m| m.is_dir()).unwrap_or(false);

            if dirs_only && !is_dir {
                continue;
            }

            let suffix = if is_dir { "/" } else { "" };
            let full = format!("{}{}{}", path_prefix, name, suffix);

            let display = if word.starts_with("~/") || word == "~" {
                let absolute = search_dir.join(&name);
                let abs_str = absolute.to_string_lossy();
                if abs_str.starts_with(&home) {
                    format!("~{}{}", &abs_str[home.len()..], suffix)
                } else {
                    full
                }
            } else {
                full
            };

            completions.push(display);
        }

        completions.sort();
        completions
    }
}

impl Completer for OrbisHelper {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &rustyline::Context<'_>,
    ) -> RlResult<(usize, Vec<Pair>)> {
        let before_cursor = &line[..pos];

        let word_start = before_cursor
            .rfind(|c: char| matches!(c, ' ' | '\t' | '|' | '>' | '<' | '&' | ';'))
            .map(|i| i + 1)
            .unwrap_or(0);
        let word = &before_cursor[word_start..];

        let before_word = before_cursor[..word_start].trim();
        let is_command = before_word.is_empty()
            || before_word.ends_with('|')
            || before_word.ends_with(';')
            || before_word.ends_with("&&")
            || before_word.ends_with("||");

        let prev_word = before_cursor[..word_start]
            .split_whitespace()
            .last()
            .unwrap_or("");
        let dirs_only = matches!(prev_word, "cd" | "cs" | "mkdir" | "pushd");

        let candidates: Vec<String> = if is_command
            && !word.starts_with('.')
            && !word.starts_with('/')
            && !word.starts_with('~')
        {
            self.commands
                .iter()
                .filter(|c| c.starts_with(word))
                .cloned()
                .collect()
        } else {
            self.complete_path(word, dirs_only)
        };

        let pairs = candidates
            .into_iter()
            .map(|c| Pair { display: c.clone(), replacement: c })
            .collect();

        Ok((word_start, pairs))
    }
}

impl Hinter for OrbisHelper {
    type Hint = String;

    fn hint(&self, line: &str, pos: usize, ctx: &rustyline::Context<'_>) -> Option<String> {
        if pos < line.len() {
            return None;
        }
        self.hinter.hint(line, pos, ctx)
    }
}

impl Highlighter for OrbisHelper {
    fn highlight<'l>(&self, line: &'l str, _pos: usize) -> Cow<'l, str> {
        if line.trim().is_empty() {
            return Cow::Borrowed(line);
        }

        let mut words = line.splitn(2, ' ');
        let cmd = words.next().unwrap_or("");
        let rest = words.next();

        let known = self.commands.iter().any(|c| c == cmd);
        let color = if known { "\x1b[36;1m" } else { "\x1b[33m" };

        let highlighted_cmd = format!("{}{}\x1b[0m", color, cmd);
        if let Some(r) = rest {
            Cow::Owned(format!("{} {}", highlighted_cmd, r))
        } else {
            Cow::Owned(highlighted_cmd)
        }
    }

    fn highlight_hint<'h>(&self, hint: &'h str) -> Cow<'h, str> {
        Cow::Owned(format!("\x1b[38;5;240m{}\x1b[0m", hint))
    }

    fn highlight_char(&self, _line: &str, _pos: usize, _forced: bool) -> bool {
        true
    }
}

impl Validator for OrbisHelper {
    fn validate(&self, _ctx: &mut ValidationContext) -> RlResult<ValidationResult> {
        Ok(ValidationResult::Valid(None))
    }
}

impl Helper for OrbisHelper {}

fn collect_commands() -> Vec<String> {
    let mut cmds: Vec<String> = BUILTINS
        .iter()
        .chain(ORBISBOX_CMDS.iter())
        .map(|s| s.to_string())
        .collect();

    if let Ok(path_var) = env::var("PATH") {
        for dir in env::split_paths(&path_var) {
            if let Ok(entries) = fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    if let Ok(name) = entry.file_name().into_string() {
                        #[cfg(unix)]
                        {
                            use std::os::unix::fs::PermissionsExt;
                            if let Ok(meta) = entry.metadata() {
                                if meta.permissions().mode() & 0o111 != 0 {
                                    cmds.push(name);
                                }
                            }
                        }
                        #[cfg(not(unix))]
                        cmds.push(name);
                    }
                }
            }
        }
    }

    cmds.sort();
    cmds.dedup();
    cmds
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let args: Vec<String> = env::args().skip(1).collect();

    for a in &args {
        match a.as_str() {
            "--version" | "-V" => {
                println!("orbis {VERSION}");
                return Ok(());
            }
            "--help" | "-h" => {
                println!("Usage: orbis [OPTIONS] [SCRIPT]");
                println!();
                println!("Options:");
                println!("  -h, --help       Affiche cette aide");
                println!("  -V, --version    Affiche la version");
                println!();
                println!("Arguments:");
                println!("  SCRIPT           Fichier de script à exécuter");
                return Ok(());
            }
            _ => {}
        }
    }

    let mut shell = Shell::new()?;

    if let Some(path) = args.iter().find(|a| !a.starts_with('-')) {
        return run_script(&mut shell, path);
    }

    run_interactive(&mut shell)
}

fn run_interactive(shell: &mut Shell) -> anyhow::Result<()> {
    let history_path = history_file();

    let config = Config::builder()
        .max_history_size(1000)?
        .history_ignore_dups(true)?
        .history_ignore_space(true)
        .completion_type(rustyline::CompletionType::List)
        .edit_mode(rustyline::EditMode::Emacs)
        .build();

    let helper = OrbisHelper::new();
    let mut rl = Editor::<OrbisHelper, DefaultHistory>::with_config(config)?;
    rl.set_helper(Some(helper));

    if let Some(ref p) = history_path {
        let _ = rl.load_history(p);
    }

    let mut last_exit: i32 = 0;

    loop {
        shell.jobs.reap_nonblocking();

        let prompt = build_prompt(&shell.env.cwd.display().to_string(), last_exit);

        match rl.readline(&prompt) {
            Ok(line) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }

                let _ = rl.add_history_entry(trimmed);

                match shell.run_line(trimmed) {
                    Ok(code) => {
                        last_exit = code;
                        if trimmed.starts_with("exit") {
                            if let Some(ref p) = history_path {
                                let _ = rl.save_history(p);
                            }
                            std::process::exit(code);
                        }
                    }
                    Err(e) => {
                        eprintln!("\x1b[31morbis: {e}\x1b[0m");
                        last_exit = 1;
                    }
                }
            }
            Err(rustyline::error::ReadlineError::Interrupted) => {
                last_exit = 130;
                continue;
            }
            Err(rustyline::error::ReadlineError::Eof) => break,
            Err(e) => return Err(e).context("erreur readline"),
        }
    }

    if let Some(ref p) = history_path {
        let _ = rl.save_history(p);
    }

    Ok(())
}

fn run_script(shell: &mut Shell, path: &str) -> anyhow::Result<()> {
    let content =
        fs::read_to_string(path).with_context(|| format!("lecture script: {path}"))?;

    for (lineno, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        match shell.run_line(trimmed) {
            Ok(code) => {
                if trimmed.starts_with("exit") {
                    std::process::exit(code);
                }
            }
            Err(e) => {
                eprintln!("orbis: {}:{}: {e}", path, lineno + 1);
                std::process::exit(1);
            }
        }
    }

    Ok(())
}

fn build_prompt(cwd: &str, last_exit: i32) -> String {
    let color = if last_exit == 0 { "\x1b[32m" } else { "\x1b[31m" };
    let reset = "\x1b[0m";
    let bold = "\x1b[1m";
    let cyan = "\x1b[36m";

    let home = env::var("HOME").unwrap_or_default();
    let display_cwd = if !home.is_empty() && cwd.starts_with(&home) {
        format!("~{}", &cwd[home.len()..])
    } else {
        cwd.to_string()
    };

    format!("{bold}{cyan}orbis{reset}:{color}{display_cwd}{reset}{bold}${reset} ")
}

fn history_file() -> Option<PathBuf> {
    let base = home::home_dir()?;
    let dir = base.join(".local").join("share").join("orbis");
    fs::create_dir_all(&dir).ok()?;
    Some(dir.join("history"))
}
