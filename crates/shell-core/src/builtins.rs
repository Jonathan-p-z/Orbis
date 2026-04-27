use crate::{env::ShellEnv, jobs::JobManager};
use anyhow::bail;

pub enum BuiltinResult {
    Continue,
    Exit(i32),
}

fn builtin_names() -> &'static [&'static str] {
    &[
        "cd", "cs", "exit", "export", "unset", "jobs", "fg", "bg",
        "pwd", "echo", "clear", "env", "which", "type",
        "alias", "unalias", "help", "true", "false",
    ]
}

fn is_builtin(name: &str) -> bool {
    builtin_names().iter().any(|&b| b == name)
}

fn find_in_path(prog: &str) -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    let paths = std::env::split_paths(&path);

    #[cfg(windows)]
    {
        let pathext_os = std::env::var_os("PATHEXT").unwrap_or(".EXE;.CMD;.BAT;.COM".into());
        let pathext = pathext_os.to_string_lossy().into_owned();
        let exts = pathext
            .split(';')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>();

        for dir in paths {
            let base = dir.join(prog);
            if base.is_file() {
                return Some(base);
            }
            for ext in &exts {
                let p = dir.join(format!("{prog}{ext}"));
                if p.is_file() {
                    return Some(p);
                }
            }
        }
        None
    }

    #[cfg(not(windows))]
    {
        for dir in paths {
            let p = dir.join(prog);
            if p.is_file() {
                return Some(p);
            }
        }
        None
    }
}

pub fn try_run_builtin(
    env: &mut ShellEnv,
    jobs: &mut JobManager,
    argv: &[String],
) -> anyhow::Result<Option<BuiltinResult>> {
    let Some(cmd) = argv.first().map(|s| s.as_str()) else { return Ok(None); };

    match cmd {
        "cd" => {
            let to: String = argv
                .get(1)
                .map(|s| crate::env::expand_tilde(s, env.get("HOME")))
                .or_else(|| env.get("HOME").map(|s| s.to_string()))
                .unwrap_or_else(|| "/".to_string());
            env.chdir(to)?;
            Ok(Some(BuiltinResult::Continue))
        }

        // cs = cd + ls, I added this because I kept typing them separately
        "cs" => {
            let to: String = argv
                .get(1)
                .map(|s| crate::env::expand_tilde(s, env.get("HOME")))
                .or_else(|| env.get("HOME").map(|s| s.to_string()))
                .unwrap_or_else(|| "/".to_string());
            env.chdir(&to)?;
            match std::fs::read_dir(&env.cwd) {
                Ok(entries) => {
                    let mut names: Vec<String> = entries
                        .filter_map(|e| e.ok())
                        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
                        .map(|e| {
                            let n = e.file_name().to_string_lossy().to_string();
                            if e.path().is_dir() { format!("{n}/") } else { n }
                        })
                        .collect();
                    names.sort();
                    for n in names { println!("{n}"); }
                }
                Err(e) => bail!("cs: {e}"),
            }
            Ok(Some(BuiltinResult::Continue))
        }
        "exit" => {
            let code = argv.get(1).and_then(|s| s.parse::<i32>().ok()).unwrap_or(0);
            Ok(Some(BuiltinResult::Exit(code)))
        }
        "export" => {
            for kv in argv.iter().skip(1) {
                if let Some((k, v)) = kv.split_once('=') {
                    env.set(k, v);
                } else {
                    // export KEY — re-export with current value, or empty string
                    let v = env.get(kv).unwrap_or("").to_string();
                    env.set(kv.as_str(), v);
                }
            }
            Ok(Some(BuiltinResult::Continue))
        }
        "unset" => {
            for k in argv.iter().skip(1) {
                env.unset(k);
            }
            Ok(Some(BuiltinResult::Continue))
        }

        "pwd" => {
            println!("{}", env.cwd.display());
            Ok(Some(BuiltinResult::Continue))
        }
        "echo" => {
            let s = argv.iter().skip(1).cloned().collect::<Vec<_>>().join(" ");
            println!("{s}");
            Ok(Some(BuiltinResult::Continue))
        }
        "clear" => {
            use std::io::Write;
            print!("\x1b[2J\x1b[H");
            let _ = std::io::stdout().flush();
            Ok(Some(BuiltinResult::Continue))
        }
        "env" => {
            let mut v = env.vars.iter().map(|(k, v)| (k, v)).collect::<Vec<_>>();
            v.sort_by(|a, b| a.0.cmp(b.0));
            for (k, val) in v {
                println!("{k}={val}");
            }
            Ok(Some(BuiltinResult::Continue))
        }
        "which" => {
            let Some(name) = argv.get(1).map(|s| s.as_str()) else { bail!("which: missing argument"); };
            if let Some(p) = find_in_path(name) {
                println!("{}", p.display());
                Ok(Some(BuiltinResult::Continue))
            } else {
                bail!("which: not found: {name}");
            }
        }
        "type" => {
            let Some(name) = argv.get(1).map(|s| s.as_str()) else { bail!("type: missing argument"); };
            if is_builtin(name) {
                println!("{name} is a shell builtin");
                Ok(Some(BuiltinResult::Continue))
            } else if let Some(p) = find_in_path(name) {
                println!("{name} is {}", p.display());
                Ok(Some(BuiltinResult::Continue))
            } else {
                bail!("type: not found: {name}");
            }
        }
        "alias" => {
            // no args = list all, otherwise set or query
            if argv.len() == 1 {
                for (k, v) in env.list_aliases() {
                    println!("alias {k}='{v}'");
                }
                return Ok(Some(BuiltinResult::Continue));
            }
            for a in argv.iter().skip(1) {
                if let Some((k, v)) = a.split_once('=') {
                    env.set_alias(k, v);
                } else {
                    // just querying a single alias by name
                    if let Some(val) = env.get_alias(a) {
                        println!("alias {a}='{val}'");
                    } else {
                        bail!("alias: use NAME=VALUE to set, or alias NAME to query");
                    }
                }
            }
            Ok(Some(BuiltinResult::Continue))
        }
        "unalias" => {
            let Some(name) = argv.get(1).map(|s| s.as_str()) else { bail!("unalias: missing argument"); };
            env.unset_alias(name);
            Ok(Some(BuiltinResult::Continue))
        }
        "help" => {
            println!("Orbis — minimal shell written in Rust");
            println!();
            println!("Builtins:");
            println!("  cd [dir]        Change directory (default: $HOME)");
            println!("  cs [dir]        cd + ls combined");
            println!("  pwd             Print working directory");
            println!("  echo [args]     Print arguments");
            println!("  clear           Clear the terminal");
            println!("  env             List environment variables");
            println!("  export KEY=VAL  Set an environment variable");
            println!("  unset KEY       Remove a variable");
            println!("  alias [K=V]     Set or list aliases");
            println!("  unalias NAME    Remove an alias");
            println!("  which NAME      Find an executable in PATH");
            println!("  type NAME       Show what a command resolves to");
            println!("  jobs            List background jobs");
            println!("  fg [%n]         Bring job to foreground");
            println!("  bg [%n]         Resume job in background");
            println!("  exit [code]     Exit the shell");
            println!("  true / false    Return 0 / 1");
            println!("  help            This help");
            println!();
            println!("Operators:");
            println!("  cmd | cmd       Pipeline");
            println!("  cmd > file      Redirect stdout (truncate)");
            println!("  cmd >> file     Redirect stdout (append)");
            println!("  cmd < file      Redirect stdin");
            println!("  cmd 2> file     Redirect stderr");
            println!("  cmd &           Run in background");
            println!();
            println!("Utilities (orbisbox): ls, cat, cp, mv, rm, mkdir, grep, sort, wc, ...");
            Ok(Some(BuiltinResult::Continue))
        }
        "true" => Ok(Some(BuiltinResult::Continue)),
        "false" => Ok(Some(BuiltinResult::Exit(1))),

        "jobs" => {
            jobs.reap_nonblocking();
            for j in jobs.list() {
                println!("[{}] {}  {}", j.id, j.status, j.cmdline);
            }
            Ok(Some(BuiltinResult::Continue))
        }
        "fg" => {
            let spec = match argv.get(1) {
                Some(s) => s.clone(),
                None => match jobs.current_job() {
                    Some(s) => s,
                    None => bail!("fg: no current job"),
                },
            };
            jobs.fg(&spec)?;
            Ok(Some(BuiltinResult::Continue))
        }
        "bg" => {
            let spec = match argv.get(1) {
                Some(s) => s.clone(),
                None => match jobs.current_job() {
                    Some(s) => s,
                    None => bail!("bg: no current job"),
                },
            };
            jobs.bg(&spec)?;
            Ok(Some(BuiltinResult::Continue))
        }

        _ => Ok(None),
    }
}
