use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Expand `$VAR`, `${VAR}`, `$?`, `$$`, `$0` in a raw shell line.
/// Single-quoted regions (`'...'`) are passed through literally.
/// Double-quoted and unquoted regions are expanded.
pub fn expand_vars(line: &str, env: &ShellEnv, last_exit: i32) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    let mut in_single = false;
    let mut in_double = false;

    while let Some(ch) = chars.next() {
        match ch {
            '\'' if !in_double => {
                in_single = !in_single;
                out.push(ch);
            }
            '"' if !in_single => {
                in_double = !in_double;
                out.push(ch);
            }
            '\\' if !in_single => {
                out.push(ch);
                if let Some(next) = chars.next() {
                    out.push(next);
                }
            }
            '$' if !in_single => {
                out.push_str(&read_var(&mut chars, env, last_exit));
            }
            other => out.push(other),
        }
    }
    out
}

fn read_var<I: Iterator<Item = char>>(
    chars: &mut std::iter::Peekable<I>,
    env: &ShellEnv,
    last_exit: i32,
) -> String {
    match chars.peek().copied() {
        Some('{') => {
            chars.next();
            let mut name = String::new();
            for ch in chars.by_ref() {
                if ch == '}' { break; }
                name.push(ch);
            }
            resolve_var(&name, env, last_exit)
        }
        Some('?') => { chars.next(); last_exit.to_string() }
        Some('$') => { chars.next(); std::process::id().to_string() }
        Some('0') => { chars.next(); "orbis".to_string() }
        Some(c) if c.is_ascii_alphanumeric() || c == '_' => {
            let mut name = String::new();
            while let Some(&c) = chars.peek() {
                if c.is_ascii_alphanumeric() || c == '_' {
                    name.push(c);
                    chars.next();
                } else {
                    break;
                }
            }
            resolve_var(&name, env, last_exit)
        }
        _ => "$".to_string(),
    }
}

fn resolve_var(name: &str, env: &ShellEnv, last_exit: i32) -> String {
    match name {
        "?" => last_exit.to_string(),
        "$" => std::process::id().to_string(),
        "0" => "orbis".to_string(),
        _ => env.get(name).unwrap_or("").to_string(),
    }
}

/// Expand glob patterns in a raw shell line, respecting single/double quote context.
/// - Unquoted tokens containing `*`, `?`, or `[` are expanded against `cwd`.
/// - Single-quoted and double-quoted tokens are passed through literally.
/// - If a pattern matches nothing, the original token is kept (nullglob off).
pub fn expand_globs_in_line(line: &str, cwd: &Path, home: Option<&str>) -> String {
    // Segment the raw line into (text, is_unquoted_glob_word) pairs.
    // Whitespace outside quotes is its own segment; words are collected separately.
    let mut segments: Vec<(String, bool)> = Vec::new();
    let mut current = String::new();
    let mut has_glob = false;
    let mut in_single = false;
    let mut in_double = false;
    let mut in_word = false;

    for ch in line.chars() {
        match ch {
            ' ' | '\t' if !in_single && !in_double => {
                if in_word {
                    segments.push((current.clone(), has_glob));
                    current.clear();
                    has_glob = false;
                    in_word = false;
                }
                segments.push((ch.to_string(), false));
            }
            '\'' if !in_double => {
                in_single = !in_single;
                in_word = true;
                current.push(ch);
            }
            '"' if !in_single => {
                in_double = !in_double;
                in_word = true;
                current.push(ch);
            }
            '*' | '?' | '[' if !in_single && !in_double => {
                has_glob = true;
                in_word = true;
                current.push(ch);
            }
            c => {
                in_word = true;
                current.push(c);
            }
        }
    }
    if in_word {
        segments.push((current, has_glob));
    }

    // Rebuild the line, replacing glob words with their expansions.
    let mut out = String::new();
    for (seg, is_glob) in segments {
        if is_glob {
            let matches = try_glob_word(&seg, cwd, home);
            if !matches.is_empty() {
                for (i, m) in matches.iter().enumerate() {
                    if i > 0 { out.push(' '); }
                    out.push_str(m);
                }
                continue;
            }
        }
        out.push_str(&seg);
    }
    out
}

fn try_glob_word(word: &str, cwd: &Path, home: Option<&str>) -> Vec<String> {
    // expand any leading tilde so ~/projects/*.rs works
    let pattern = expand_tilde(word, home);

    let is_abs = Path::new(pattern.as_str()).is_absolute();
    let abs_pat = if is_abs {
        pattern.clone()
    } else {
        // escape glob-special chars in the cwd so they aren't treated as wildcards
        let safe_cwd = glob::Pattern::escape(&cwd.to_string_lossy());
        format!("{safe_cwd}/{pattern}")
    };

    let paths = match glob::glob(&abs_pat) {
        Ok(p) => p,
        Err(_) => return vec![],
    };

    let mut results: Vec<String> = paths
        .filter_map(|r| r.ok())
        .filter_map(|p| {
            if is_abs {
                Some(p.to_string_lossy().into_owned())
            } else {
                p.strip_prefix(cwd)
                    .map(|rel| rel.to_string_lossy().into_owned())
                    .ok()
                    .or_else(|| Some(p.to_string_lossy().into_owned()))
            }
        })
        .collect();

    results.sort();
    results
}

pub fn expand_tilde(s: &str, home: Option<&str>) -> String {
    let home = match home {
        Some(h) if !h.is_empty() => h,
        _ => return s.to_string(),
    };
    if s == "~" {
        return home.to_string();
    }
    if let Some(rest) = s.strip_prefix("~/") {
        return format!("{home}/{rest}");
    }
    s.to_string()
}

#[derive(Debug, Default, Clone)]
pub struct ShellEnv {
    pub vars: HashMap<String, String>,
    pub cwd: PathBuf,
    pub aliases: HashMap<String, String>,
}

impl ShellEnv {
    pub fn new() -> anyhow::Result<Self> {
        #[allow(unused_mut)]
        let mut env = Self {
            vars: std::env::vars().collect(),
            cwd: std::env::current_dir()?,
            aliases: HashMap::new(),
        };

        #[cfg(windows)]
        {
            env.set_alias("ls", "dir");
            env.set_alias("cat", "type");
            env.set_alias("clear", "cls");
        }

        Ok(env)
    }

    pub fn get(&self, k: &str) -> Option<&str> {
        self.vars.get(k).map(|s| s.as_str())
    }

    pub fn set(&mut self, k: impl Into<String>, v: impl Into<String>) {
        let k = k.into();
        let v = v.into();
        std::env::set_var(&k, &v);
        self.vars.insert(k, v);
    }

    pub fn unset(&mut self, k: &str) {
        std::env::remove_var(k);
        self.vars.remove(k);
    }

    pub fn chdir(&mut self, to: impl Into<PathBuf>) -> anyhow::Result<()> {
        let to = to.into();
        std::env::set_current_dir(&to)?;
        self.cwd = std::env::current_dir()?;
        Ok(())
    }

    pub fn set_alias(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.aliases.insert(name.into(), value.into());
    }

    pub fn unset_alias(&mut self, name: &str) {
        self.aliases.remove(name);
    }

    pub fn get_alias(&self, name: &str) -> Option<&str> {
        self.aliases.get(name).map(|s| s.as_str())
    }

    pub fn list_aliases(&self) -> Vec<(String, String)> {
        let mut v: Vec<_> = self.aliases.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        v.sort_by(|a, b| a.0.cmp(&b.0));
        v
    }
}
