#![allow(clippy::too_many_arguments)]
use anyhow::{bail, Context};
use std::{
    collections::HashMap,
    env,
    fs::{self, File, OpenOptions},
    io::{self, BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

// ─── ANSI ─────────────────────────────────────────────────────────────────────
const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const BLUE: &str = "\x1b[34m";
const CYAN: &str = "\x1b[36m";
const GREEN: &str = "\x1b[32m";
const RED: &str = "\x1b[31m";
const RED_BOLD: &str = "\x1b[1;31m";

// ─── MAIN ─────────────────────────────────────────────────────────────────────
fn main() {
    // restore SIGPIPE default so we exit quietly when piped to head, less, etc.
    #[cfg(unix)]
    unsafe {
        use nix::sys::signal::{signal, SigHandler, Signal};
        let _ = signal(Signal::SIGPIPE, SigHandler::SigDfl);
    }

    let argv: Vec<String> = env::args().collect();
    let exe = argv.first().cloned().unwrap_or_else(|| "orbisbox".into());

    let (cmd, args) = if argv.len() >= 2 {
        let c = argv[1].clone();
        let rest = argv[2..].to_vec();
        (c, rest)
    } else {
        let stem = Path::new(&exe)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("orbisbox")
            .to_string();
        if stem != "orbisbox" {
            (stem, Vec::new())
        } else {
            usage();
            return;
        }
    };

    let result: anyhow::Result<()> = match cmd.as_str() {
        "ls" => cmd_ls(&args),
        "cat" => cmd_cat(&args),
        "cp" => cmd_cp(&args),
        "mv" => cmd_mv(&args),
        "rm" => cmd_rm(&args),
        "mkdir" => cmd_mkdir(&args),
        "rmdir" => cmd_rmdir(&args),
        "touch" => cmd_touch(&args),
        "ln" => cmd_ln(&args),
        "stat" => cmd_stat(&args),
        "du" => cmd_du(&args),
        "df" => cmd_df(&args),
        "find" => cmd_find(&args),
        "chmod" => cmd_chmod(&args),
        "head" => cmd_head(&args),
        "tail" => cmd_tail(&args),
        "wc" => cmd_wc(&args),
        "tee" => cmd_tee(&args),
        "grep" => cmd_grep(&args),
        "sort" => cmd_sort(&args),
        "uniq" => cmd_uniq(&args),
        "cut" => cmd_cut(&args),
        "tr" => cmd_tr(&args),
        "seq" => cmd_seq(&args),
        "diff" => cmd_diff(&args),
        "xargs" => cmd_xargs(&args),
        "echo" => cmd_echo(&args),
        "pwd" => cmd_pwd(&args),
        "which" => cmd_which(&args),
        "whoami" => cmd_whoami(&args),
        "uname" => cmd_uname(&args),
        "date" => cmd_date(&args),
        "sleep" => cmd_sleep(&args),
        "yes" => cmd_yes(&args),
        "env" => cmd_env(&args),
        "id" => cmd_id(&args),
        "ps" => cmd_ps(&args),
        "kill" => cmd_kill(&args),
        "basename" => cmd_basename(&args),
        "dirname" => cmd_dirname(&args),
        "realpath" => cmd_realpath(&args),
        "true" => Ok(()),
        "false" => std::process::exit(1),
        _ => {
            eprintln!("{RED}orbisbox: unknown command: '{cmd}'{RESET}");
            std::process::exit(1);
        }
    };

    if let Err(e) = result {
        eprintln!("{RED_BOLD}orbisbox {cmd}: {e}{RESET}");
        std::process::exit(1);
    }
}

fn usage() {
    println!("{BOLD}orbisbox{RESET} — Unix utility toolbox");
    println!();
    println!("{BOLD}Usage:{RESET} orbisbox <command> [args...]");
    println!();
    println!("{BOLD}Files:{RESET}    ls cat cp mv rm mkdir rmdir touch ln chmod stat du df find");
    println!("{BOLD}Text:{RESET}     head tail wc tee grep sort uniq cut tr seq diff xargs");
    println!("{BOLD}System:{RESET}   echo pwd which whoami uname date sleep yes env id ps kill");
    println!("{BOLD}Path:{RESET}     basename dirname realpath true false");
}

// ─── FLAG PARSER ──────────────────────────────────────────────────────────────
/// Returns (flags_set, positional_args, named_values)
/// flags_set: set of single-char flags that were present
/// named_values: map of flag-char → value (for -n 5, -t ',', etc.)
/// Also handles --long-flag and --long=value
fn parse_flags(
    args: &[String],
    short_flags: &str,       // flags that take no value, e.g. "rlhasSFd1vnAbiufpRvq"
    short_valued: &str,      // flags that take a value, e.g. "ntkdofe"
) -> (std::collections::HashSet<char>, HashMap<char, String>, Vec<String>, HashMap<String, String>) {
    let mut flags: std::collections::HashSet<char> = std::collections::HashSet::new();
    let mut values: HashMap<char, String> = HashMap::new();
    let mut positional: Vec<String> = Vec::new();
    let mut long_values: HashMap<String, String> = HashMap::new();
    let mut done = false;

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if done {
            positional.push(arg.clone());
            i += 1;
            continue;
        }
        if arg == "--" {
            done = true;
            i += 1;
            continue;
        }
        if arg.starts_with("--") {
            let rest = &arg[2..];
            if let Some(eq) = rest.find('=') {
                long_values.insert(rest[..eq].to_string(), rest[eq+1..].to_string());
            } else {
                long_values.insert(rest.to_string(), String::new());
            }
            i += 1;
            continue;
        }
        if arg.starts_with('-') && arg.len() > 1 && !arg.chars().skip(1).next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
            let chars: Vec<char> = arg[1..].chars().collect();
            let mut j = 0;
            while j < chars.len() {
                let c = chars[j];
                if short_valued.contains(c) {
                    // value is rest of token or next arg
                    if j + 1 < chars.len() {
                        values.insert(c, chars[j+1..].iter().collect());
                        break;
                    } else {
                        i += 1;
                        if i < args.len() {
                            values.insert(c, args[i].clone());
                        }
                        break;
                    }
                } else if short_flags.contains(c) {
                    flags.insert(c);
                }
                j += 1;
            }
        } else {
            positional.push(arg.clone());
        }
        i += 1;
    }
    (flags, values, positional, long_values)
}

// ─── UTILITIES ────────────────────────────────────────────────────────────────
fn find_in_path(name: &str) -> Vec<PathBuf> {
    let path_var = env::var("PATH").unwrap_or_default();
    let mut results = Vec::new();
    for dir in path_var.split(':') {
        let p = Path::new(dir).join(name);
        if p.exists() {
            results.push(p);
        }
    }
    results
}

fn human_size(n: u64) -> String {
    const K: u64 = 1024;
    const M: u64 = K * 1024;
    const G: u64 = M * 1024;
    const T: u64 = G * 1024;
    if n >= T { format!("{:.1}T", n as f64 / T as f64) }
    else if n >= G { format!("{:.1}G", n as f64 / G as f64) }
    else if n >= M { format!("{:.1}M", n as f64 / M as f64) }
    else if n >= K { format!("{:.1}K", n as f64 / K as f64) }
    else { format!("{n}") }
}

fn guard_root(p: &Path) -> anyhow::Result<()> {
    let canon = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    if canon == Path::new("/") {
        bail!("won't operate on /");
    }
    Ok(())
}

/// Simple glob: * matches any sequence (not /), ** matches across /, ? matches one char
fn glob_match(pattern: &str, s: &str) -> bool {
    glob_match_inner(pattern.as_bytes(), s.as_bytes())
}

fn glob_match_inner(p: &[u8], s: &[u8]) -> bool {
    match (p.first(), s.first()) {
        (None, None) => true,
        (Some(b'*'), _) => {
            if p.len() >= 2 && p[1] == b'*' {
                // ** matches across /
                for i in 0..=s.len() {
                    if glob_match_inner(&p[2..], &s[i..]) {
                        return true;
                    }
                }
                false
            } else {
                for i in 0..=s.len() {
                    if s[..i].iter().any(|&c| c == b'/') { break; }
                    if glob_match_inner(&p[1..], &s[i..]) {
                        return true;
                    }
                }
                false
            }
        }
        (Some(b'?'), Some(_)) => glob_match_inner(&p[1..], &s[1..]),
        (Some(b'['), _) => {
            // character class
            if let Some(close) = p[1..].iter().position(|&c| c == b']') {
                let class = &p[1..close+1];
                let matched = s.first().map(|&sc| {
                    let mut i = 0;
                    let negate = class.first() == Some(&b'!') || class.first() == Some(&b'^');
                    let start = if negate { 1 } else { 0 };
                    let mut m = false;
                    while i + start < class.len() {
                        if i + start + 2 < class.len() && class[i + start + 1] == b'-' {
                            if sc >= class[i + start] && sc <= class[i + start + 2] { m = true; }
                            i += 3;
                        } else {
                            if sc == class[i + start] { m = true; }
                            i += 1;
                        }
                    }
                    if negate { !m } else { m }
                }).unwrap_or(false);
                if matched { glob_match_inner(&p[close+2..], &s[1..]) } else { false }
            } else {
                false
            }
        }
        (Some(&pc), Some(&sc)) if pc == sc => glob_match_inner(&p[1..], &s[1..]),
        _ => false,
    }
}

fn read_lines_file(path: &str) -> anyhow::Result<Vec<String>> {
    let f = File::open(path).with_context(|| format!("impossible d'ouvrir {path}"))?;
    let reader = BufReader::new(f);
    Ok(reader.lines().collect::<Result<Vec<_>, _>>()?)
}

fn stdin_lines() -> Vec<String> {
    let stdin = io::stdin();
    stdin.lock().lines().filter_map(|l| l.ok()).collect()
}

// ─── LS ───────────────────────────────────────────────────────────────────────
fn cmd_ls(args: &[String]) -> anyhow::Result<()> {
    let (flags, _, mut positional, _) = parse_flags(args, "alhrtSFd1RvA", "");
    if positional.is_empty() {
        positional.push(".".to_string());
    }
    let show_all = flags.contains(&'a') || flags.contains(&'A');
    let long = flags.contains(&'l');
    let human = flags.contains(&'h');
    let reverse = flags.contains(&'r');
    let sort_time = flags.contains(&'t');
    let sort_size = flags.contains(&'S');
    let classify = flags.contains(&'F');
    let list_dir = flags.contains(&'d');
    let one_per_line = flags.contains(&'1');

    struct Entry {
        name: String,
        path: PathBuf,
        meta: Option<fs::Metadata>,
    }

    let mut entries: Vec<Entry> = Vec::new();

    for target in &positional {
        let p = Path::new(target);
        if list_dir || !p.is_dir() {
            let meta = fs::symlink_metadata(p).ok();
            entries.push(Entry { name: target.clone(), path: p.to_path_buf(), meta });
        } else {
            let rd = fs::read_dir(p).with_context(|| format!("ls: impossible de lire {target}"))?;
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if !show_all && name.starts_with('.') { continue; }
                let meta = e.metadata().ok();
                entries.push(Entry { name, path: e.path(), meta });
            }
        }
    }

    // Sort
    if sort_time {
        entries.sort_by(|a, b| {
            let ta = a.meta.as_ref().and_then(|m| m.modified().ok());
            let tb = b.meta.as_ref().and_then(|m| m.modified().ok());
            tb.cmp(&ta)
        });
    } else if sort_size {
        entries.sort_by(|a, b| {
            let sa = a.meta.as_ref().map(|m| m.len()).unwrap_or(0);
            let sb = b.meta.as_ref().map(|m| m.len()).unwrap_or(0);
            sb.cmp(&sa)
        });
    } else {
        entries.sort_by(|a, b| a.name.cmp(&b.name));
    }
    if reverse { entries.reverse(); }

    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());

    if long {
        for e in &entries {
            let meta = match &e.meta {
                Some(m) => m,
                None => { writeln!(out, "? {}", e.name)?; continue; }
            };
            #[cfg(unix)]
            let (perm_str, nlink, uid, gid, inode) = {
                let mode = meta.mode();
                let nl = meta.nlink();
                let u = meta.uid();
                let g = meta.gid();
                let ino = meta.ino();
                (format_mode(mode), nl, u, g, ino)
            };
            #[cfg(not(unix))]
            let (perm_str, nlink, uid, gid, inode) = ("---------".to_string(), 1u64, 0u32, 0u32, 0u64);
            let _ = inode;
            let size = meta.len();
            let size_str = if human { human_size(size) } else { size.to_string() };
            let mtime = meta.modified().ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| {
                    let secs = d.as_secs();
                    let dt = unix_to_datetime(secs);
                    format!("{:04}-{:02}-{:02} {:02}:{:02}", dt.0, dt.1, dt.2, dt.3, dt.4)
                })
                .unwrap_or_else(|| "????-??-?? ??:??".to_string());
            let ftype = if meta.is_dir() { "d" } else if meta.file_type().is_symlink() { "l" } else { "-" };
            let colored = color_name(&e.name, &e.path, meta);
            let indicator = if classify { type_indicator(&e.path, meta) } else { "" };
            writeln!(out, "{ftype}{perm_str} {nlink:>3} {uid:<6} {gid:<6} {size_str:>8} {mtime} {colored}{indicator}")?;
        }
    } else if one_per_line {
        for e in &entries {
            let meta = e.meta.as_ref();
            let colored = if let Some(m) = meta { color_name(&e.name, &e.path, m) } else { e.name.clone() };
            let indicator = if classify { meta.map(|m| type_indicator(&e.path, m)).unwrap_or("") } else { "" };
            writeln!(out, "{colored}{indicator}")?;
        }
    } else {
        let names: Vec<String> = entries.iter().map(|e| {
            let meta = e.meta.as_ref();
            let colored = if let Some(m) = meta { color_name(&e.name, &e.path, m) } else { e.name.clone() };
            let indicator = if classify { meta.map(|m| type_indicator(&e.path, m)).unwrap_or("") } else { "" };
            format!("{colored}{indicator}")
        }).collect();
        // Simple column output
        let term_width = 80usize;
        let max_name_len = entries.iter().map(|e| e.name.len()).max().unwrap_or(0) + 2;
        let cols = (term_width / max_name_len.max(1)).max(1);
        for (i, n) in names.iter().enumerate() {
            let visible_len = entries[i].name.len() + if classify { 1 } else { 0 };
            if i % cols == cols - 1 || i == names.len() - 1 {
                writeln!(out, "{n}")?;
            } else {
                write!(out, "{n}{}", " ".repeat(max_name_len.saturating_sub(visible_len)))?;
            }
        }
    }
    Ok(())
}

fn color_name(name: &str, path: &Path, meta: &fs::Metadata) -> String {
    if meta.is_dir() {
        format!("{BLUE}{BOLD}{name}{RESET}")
    } else if meta.file_type().is_symlink() {
        format!("{CYAN}{name}{RESET}")
    } else {
        #[cfg(unix)]
        {
            let mode = meta.mode();
            if mode & 0o111 != 0 {
                return format!("{GREEN}{name}{RESET}");
            }
        }
        let _ = path;
        name.to_string()
    }
}

fn type_indicator(path: &Path, meta: &fs::Metadata) -> &'static str {
    if meta.is_dir() { "/" }
    else if meta.file_type().is_symlink() { "@" }
    else {
        #[cfg(unix)]
        {
            if meta.mode() & 0o111 != 0 { return "*"; }
        }
        let _ = path;
        ""
    }
}

#[cfg(unix)]
fn format_mode(mode: u32) -> String {
    let chars = [
        (0o400, 'r'), (0o200, 'w'), (0o100, 'x'),
        (0o040, 'r'), (0o020, 'w'), (0o010, 'x'),
        (0o004, 'r'), (0o002, 'w'), (0o001, 'x'),
    ];
    chars.iter().map(|(bit, c)| if mode & bit != 0 { *c } else { '-' }).collect()
}

#[cfg(not(unix))]
fn format_mode(_mode: u32) -> String { "---------".to_string() }

// ─── CAT ──────────────────────────────────────────────────────────────────────
fn cmd_cat(args: &[String]) -> anyhow::Result<()> {
    let (flags, _, positional, _) = parse_flags(args, "nAsb", "");
    let number = flags.contains(&'n');
    let show_ends = flags.contains(&'A');
    let squeeze = flags.contains(&'s');
    let number_nonblank = flags.contains(&'b');

    let process = |reader: &mut dyn BufRead| -> anyhow::Result<()> {
        let stdout = io::stdout();
        let mut out = io::BufWriter::new(stdout.lock());
        let mut lineno = 1usize;
        let mut prev_blank = false;
        for line in reader.lines() {
            let line = line?;
            let blank = line.trim().is_empty();
            if squeeze && blank && prev_blank { continue; }
            prev_blank = blank;
            let suffix = if show_ends { "$" } else { "" };
            if number || (number_nonblank && !blank) {
                writeln!(out, "{:>6}\t{line}{suffix}", lineno)?;
                lineno += 1;
            } else {
                if number_nonblank && blank { lineno += 1; }
                writeln!(out, "{line}{suffix}")?;
            }
        }
        Ok(())
    };

    if positional.is_empty() {
        process(&mut io::BufReader::new(io::stdin()))?;
    } else {
        for f in &positional {
            if f == "-" {
                process(&mut io::BufReader::new(io::stdin()))?;
            } else {
                let file = File::open(f).with_context(|| format!("cat: {f}"))?;
                process(&mut io::BufReader::new(file))?;
            }
        }
    }
    Ok(())
}

// ─── CP ───────────────────────────────────────────────────────────────────────
fn cmd_cp(args: &[String]) -> anyhow::Result<()> {
    let (flags, _, positional, _) = parse_flags(args, "rRvifupP", "");
    let recursive = flags.contains(&'r') || flags.contains(&'R');
    let verbose = flags.contains(&'v');
    let interactive = flags.contains(&'i');
    let update = flags.contains(&'u');
    let preserve = flags.contains(&'p') || flags.contains(&'P');
    let force = flags.contains(&'f');

    if positional.len() < 2 {
        bail!("cp: arguments manquants");
    }
    let dest = Path::new(positional.last().unwrap());
    let sources = &positional[..positional.len()-1];

    for src_str in sources {
        let src = Path::new(src_str);
        let dst = if dest.is_dir() {
            dest.join(src.file_name().context("cp: nom source invalide")?)
        } else {
            dest.to_path_buf()
        };
        cp_entry(src, &dst, recursive, verbose, interactive, update, preserve, force)?;
    }
    Ok(())
}

fn cp_entry(src: &Path, dst: &Path, recursive: bool, verbose: bool, interactive: bool, update: bool, preserve: bool, force: bool) -> anyhow::Result<()> {
    let src_meta = fs::symlink_metadata(src).with_context(|| format!("cp: cannot stat '{}'", src.display()))?;
    if src_meta.is_dir() {
        if !recursive {
            bail!("cp: '{}' est un dossier (utilise -r)", src.display());
        }
        fs::create_dir_all(dst)?;
        for e in fs::read_dir(src)?.flatten() {
            let child_dst = dst.join(e.file_name());
            cp_entry(&e.path(), &child_dst, recursive, verbose, interactive, update, preserve, force)?;
        }
        return Ok(());
    }
    // File
    if dst.exists() {
        if interactive && !force {
            eprint!("cp: overwrite '{}'? [y/N] ", dst.display());
            io::stdout().flush()?;
            let mut ans = String::new();
            io::stdin().read_line(&mut ans)?;
            if !ans.trim().eq_ignore_ascii_case("o") && !ans.trim().eq_ignore_ascii_case("y") { return Ok(()); }
        }
        if update {
            let src_mtime = src_meta.modified().ok();
            let dst_mtime = fs::metadata(dst).ok().and_then(|m| m.modified().ok());
            if let (Some(s), Some(d)) = (src_mtime, dst_mtime) {
                if d >= s { return Ok(()); }
            }
        }
    }
    if verbose { println!("'{}' -> '{}'", src.display(), dst.display()); }
    fs::copy(src, dst).with_context(|| format!("cp: '{}' -> '{}'", src.display(), dst.display()))?;
    if preserve {
        #[cfg(unix)]
        {
            let perm = src_meta.permissions();
            fs::set_permissions(dst, perm)?;
        }
    }
    Ok(())
}

// ─── MV ───────────────────────────────────────────────────────────────────────
fn cmd_mv(args: &[String]) -> anyhow::Result<()> {
    let (flags, _, positional, _) = parse_flags(args, "vifub", "");
    let verbose = flags.contains(&'v');
    let interactive = flags.contains(&'i');
    let update = flags.contains(&'u');
    let force = flags.contains(&'f');
    let backup = flags.contains(&'b');

    if positional.len() < 2 {
        bail!("mv: arguments manquants");
    }
    let dest = Path::new(positional.last().unwrap());
    let sources = &positional[..positional.len()-1];

    for src_str in sources {
        let src = Path::new(src_str);
        let dst = if dest.is_dir() {
            dest.join(src.file_name().context("mv: nom invalide")?)
        } else {
            dest.to_path_buf()
        };
        if dst.exists() {
            if interactive && !force {
                eprint!("mv: overwrite '{}'? [y/N] ", dst.display());
                io::stdout().flush()?;
                let mut ans = String::new();
                io::stdin().read_line(&mut ans)?;
                if !ans.trim().eq_ignore_ascii_case("o") && !ans.trim().eq_ignore_ascii_case("y") { continue; }
            }
            if update {
                let src_meta = fs::metadata(src).ok();
                let dst_meta = fs::metadata(&dst).ok();
                if let (Some(sm), Some(dm)) = (src_meta, dst_meta) {
                    if dm.modified().ok() >= sm.modified().ok() { continue; }
                }
            }
            if backup {
                let bak = dst.with_extension("~");
                fs::rename(&dst, &bak)?;
            }
        }
        if verbose { println!("'{}' -> '{}'", src.display(), dst.display()); }
        fs::rename(src, &dst).with_context(|| format!("mv: '{}' -> '{}'", src.display(), dst.display()))?;
    }
    Ok(())
}

// ─── RM ───────────────────────────────────────────────────────────────────────
fn cmd_rm(args: &[String]) -> anyhow::Result<()> {
    let (flags, _, positional, _) = parse_flags(args, "rRfvi", "");
    let recursive = flags.contains(&'r') || flags.contains(&'R');
    let force = flags.contains(&'f');
    let verbose = flags.contains(&'v');
    let interactive = flags.contains(&'i');

    for f in &positional {
        let p = Path::new(f);
        guard_root(p)?;
        if interactive {
            eprint!("rm: supprimer '{}'? [o/N] ", p.display());
            io::stdout().flush()?;
            let mut ans = String::new();
            io::stdin().read_line(&mut ans)?;
            if !ans.trim().eq_ignore_ascii_case("o") && !ans.trim().eq_ignore_ascii_case("y") { continue; }
        }
        if p.is_dir() {
            if !recursive {
                if !force { bail!("rm: '{}' est un dossier (utilise -r)", p.display()); }
                continue;
            }
            if verbose { println!("rm: suppression de '{}'", p.display()); }
            fs::remove_dir_all(p).with_context(|| format!("rm: {f}"))?;
        } else {
            if !p.exists() && force { continue; }
            if verbose { println!("rm: suppression de '{}'", p.display()); }
            fs::remove_file(p).with_context(|| format!("rm: {f}"))?;
        }
    }
    Ok(())
}

// ─── MKDIR ────────────────────────────────────────────────────────────────────
fn cmd_mkdir(args: &[String]) -> anyhow::Result<()> {
    let (flags, _, positional, _) = parse_flags(args, "pv", "");
    let parents = flags.contains(&'p');
    let verbose = flags.contains(&'v');
    for d in &positional {
        let p = Path::new(d);
        if parents {
            fs::create_dir_all(p).with_context(|| format!("mkdir: {d}"))?;
        } else {
            fs::create_dir(p).with_context(|| format!("mkdir: {d}"))?;
        }
        if verbose { println!("mkdir: created '{d}'"); }
    }
    Ok(())
}

// ─── RMDIR ────────────────────────────────────────────────────────────────────
fn cmd_rmdir(args: &[String]) -> anyhow::Result<()> {
    for d in args {
        fs::remove_dir(d).with_context(|| format!("rmdir: {d}"))?;
    }
    Ok(())
}

// ─── TOUCH ────────────────────────────────────────────────────────────────────
fn cmd_touch(args: &[String]) -> anyhow::Result<()> {
    for f in args {
        if Path::new(f).exists() {
            // update timestamps
            #[cfg(unix)]
            {
                let now = nix::sys::time::TimeSpec::from_duration(
                    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default()
                );
                let times = [now, now];
                let _ = nix::sys::stat::utimensat(
                    None,
                    Path::new(f),
                    &times[0],
                    &times[1],
                    nix::sys::stat::UtimensatFlags::NoFollowSymlink,
                );
            }
        } else {
            File::create(f).with_context(|| format!("touch: {f}"))?;
        }
    }
    Ok(())
}

// ─── LN ───────────────────────────────────────────────────────────────────────
fn cmd_ln(args: &[String]) -> anyhow::Result<()> {
    let (flags, _, positional, _) = parse_flags(args, "sfb", "");
    let symbolic = flags.contains(&'s');
    let force = flags.contains(&'f');
    let backup = flags.contains(&'b');

    if positional.len() < 2 {
        bail!("ln: arguments manquants");
    }
    let target = Path::new(&positional[0]);
    let link = Path::new(&positional[1]);

    if link.exists() {
        if backup {
            fs::rename(link, link.with_extension("~"))?;
        } else if force {
            fs::remove_file(link)?;
        }
    }

    #[cfg(unix)]
    {
        if symbolic {
            std::os::unix::fs::symlink(target, link)?;
        } else {
            fs::hard_link(target, link)?;
        }
    }
    #[cfg(not(unix))]
    {
        if symbolic { bail!("ln: symlinks not supported on this platform"); }
        fs::hard_link(target, link)?;
    }
    Ok(())
}

// ─── CHMOD ────────────────────────────────────────────────────────────────────
fn cmd_chmod(args: &[String]) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        let (flags, _, positional, _) = parse_flags(args, "Rv", "");
        let recursive = flags.contains(&'R');
        let verbose = flags.contains(&'v');
        if positional.len() < 2 { bail!("chmod: arguments manquants"); }
        let mode_str = &positional[0];
        let files = &positional[1..];
        for f in files {
            chmod_path(Path::new(f), mode_str, recursive, verbose)?;
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = args;
        bail!("chmod: not supported on this platform");
    }
}

#[cfg(unix)]
fn chmod_path(p: &Path, mode_str: &str, recursive: bool, verbose: bool) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let meta = fs::symlink_metadata(p)?;
    let current = meta.mode();
    let new_mode = parse_chmod_mode(mode_str, current)?;
    let perm = fs::Permissions::from_mode(new_mode);
    fs::set_permissions(p, perm)?;
    if verbose { println!("chmod: '{}' -> {:o}", p.display(), new_mode); }
    if recursive && meta.is_dir() {
        for e in fs::read_dir(p)?.flatten() {
            chmod_path(&e.path(), mode_str, recursive, verbose)?;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn parse_chmod_mode(s: &str, current: u32) -> anyhow::Result<u32> {
    // Try octal first
    if s.chars().all(|c| c.is_ascii_digit()) {
        return Ok(u32::from_str_radix(s, 8).with_context(|| format!("chmod: mode invalide: {s}"))?);
    }
    // Symbolic: [ugoa][+-=][rwxXst]+,...
    let mut mode = current;
    for part in s.split(',') {
        let part = part.trim();
        if part.is_empty() { continue; }
        let (who_end, op_pos) = part.char_indices()
            .find(|(_, c)| matches!(c, '+' | '-' | '='))
            .map(|(i, _)| (i, i))
            .unwrap_or((0, 0));
        let who = &part[..who_end];
        let op = part.chars().nth(op_pos).unwrap_or('+');
        let perms = &part[op_pos+1..];

        let u_mask: u32 = if who.is_empty() || who.contains('a') || who.contains('u') { 0o700 } else { 0 };
        let g_mask: u32 = if who.is_empty() || who.contains('a') || who.contains('g') { 0o070 } else { 0 };
        let o_mask: u32 = if who.is_empty() || who.contains('a') || who.contains('o') { 0o007 } else { 0 };

        let mut bits: u32 = 0;
        for c in perms.chars() {
            match c {
                'r' => bits |= 0o444 & (u_mask | g_mask | o_mask),
                'w' => bits |= 0o222 & (u_mask | g_mask | o_mask),
                'x' => bits |= 0o111 & (u_mask | g_mask | o_mask),
                's' => bits |= 0o6000,
                't' => bits |= 0o1000,
                _ => {}
            }
        }
        match op {
            '+' => mode |= bits,
            '-' => mode &= !bits,
            '=' => {
                let mask = u_mask | g_mask | o_mask;
                mode = (mode & !mask) | (bits & mask);
            }
            _ => {}
        }
    }
    Ok(mode)
}

// ─── STAT ─────────────────────────────────────────────────────────────────────
fn cmd_stat(args: &[String]) -> anyhow::Result<()> {
    for f in args {
        let meta = fs::symlink_metadata(f).with_context(|| format!("stat: {f}"))?;
        println!("  File: {f}");
        println!("  Size: {}", meta.len());
        let ftype = if meta.is_dir() { "directory" } else if meta.file_type().is_symlink() { "symbolic link" } else { "regular file" };
        println!("  Type: {ftype}");
        #[cfg(unix)]
        {
            println!(" Inode: {}", meta.ino());
            println!("  Mode: {:o}", meta.mode());
            println!(" Links: {}", meta.nlink());
            println!("   Uid: {}", meta.uid());
            println!("   Gid: {}", meta.gid());
            let mtime = unix_to_datetime(meta.mtime() as u64);
            let atime = unix_to_datetime(meta.atime() as u64);
            let ctime = unix_to_datetime(meta.ctime() as u64);
            println!("Access: {:04}-{:02}-{:02} {:02}:{:02}:{:02}", atime.0, atime.1, atime.2, atime.3, atime.4, atime.5);
            println!("Modify: {:04}-{:02}-{:02} {:02}:{:02}:{:02}", mtime.0, mtime.1, mtime.2, mtime.3, mtime.4, mtime.5);
            println!("Change: {:04}-{:02}-{:02} {:02}:{:02}:{:02}", ctime.0, ctime.1, ctime.2, ctime.3, ctime.4, ctime.5);
        }
    }
    Ok(())
}

// ─── DU ───────────────────────────────────────────────────────────────────────
fn cmd_du(args: &[String]) -> anyhow::Result<()> {
    let (flags, values, mut positional, long_values) = parse_flags(args, "hsacx", "d");
    let human = flags.contains(&'h');
    let summarize = flags.contains(&'s');
    let all_files = flags.contains(&'a');
    let grand_total = flags.contains(&'c');
    let max_depth: Option<usize> = values.get(&'d')
        .or_else(|| long_values.get("max-depth"))
        .and_then(|v| v.parse().ok());

    if positional.is_empty() { positional.push(".".to_string()); }

    let mut total = 0u64;
    for p in &positional {
        let size = du_path(Path::new(p), human, summarize, all_files, max_depth, 0)?;
        total += size;
    }
    if grand_total {
        let s = if human { human_size(total) } else { (total / 1024).to_string() };
        println!("{s}\ttotal");
    }
    Ok(())
}

fn du_path(p: &Path, human: bool, summarize: bool, all_files: bool, max_depth: Option<usize>, depth: usize) -> anyhow::Result<u64> {
    let meta = fs::symlink_metadata(p)?;
    let mut size = meta.len();
    if meta.is_dir() {
        for e in fs::read_dir(p)?.flatten() {
            size += du_path(&e.path(), human, summarize, all_files, max_depth, depth + 1)?;
        }
        let within_depth = max_depth.map(|d| depth <= d).unwrap_or(true);
        if within_depth && !summarize {
            let s = if human { human_size(size) } else { (size / 1024 + 1).to_string() };
            println!("{s}\t{}", p.display());
        } else if summarize && depth == 0 {
            let s = if human { human_size(size) } else { (size / 1024 + 1).to_string() };
            println!("{s}\t{}", p.display());
        }
    } else if all_files {
        let s = if human { human_size(size) } else { (size / 1024 + 1).to_string() };
        println!("{s}\t{}", p.display());
    }
    Ok(size)
}

// ─── DF ───────────────────────────────────────────────────────────────────────
fn cmd_df(args: &[String]) -> anyhow::Result<()> {
    #[cfg(target_os = "linux")]
    {
        let (flags, _, positional, _) = parse_flags(args, "hT", "");
        let human = flags.contains(&'h');
        let show_type = flags.contains(&'T');

        // Read /proc/mounts
        let mounts_str = fs::read_to_string("/proc/mounts").unwrap_or_default();
        let mut mounts: Vec<(String, String, String)> = Vec::new(); // device, mountpoint, fstype
        for line in mounts_str.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 {
                mounts.push((parts[0].to_string(), parts[1].to_string(), parts[2].to_string()));
            }
        }

        let targets: Vec<String> = if positional.is_empty() {
            mounts.iter().map(|(_, mp, _)| mp.clone()).collect()
        } else {
            positional.clone()
        };

        if show_type {
            println!("{:<20} {:<10} {:>10} {:>10} {:>10} {:>6} {}", "Filesystem", "Type", "1K-blocks", "Used", "Available", "Use%", "Mounted on");
        } else {
            println!("{:<20} {:>10} {:>10} {:>10} {:>6} {}", "Filesystem", "1K-blocks", "Used", "Available", "Use%", "Mounted on");
        }

        let mut seen = std::collections::HashSet::new();
        for mp in &targets {
            if !seen.insert(mp.clone()) { continue; }
            if let Ok(stat) = nix::sys::statvfs::statvfs(mp.as_str()) {
                let bsize = stat.block_size() as u64;
                let total = stat.blocks() * bsize / 1024;
                let free = stat.blocks_free() * bsize / 1024;
                let avail = stat.blocks_available() * bsize / 1024;
                let used = total.saturating_sub(free);
                let pct = if total > 0 { used * 100 / total } else { 0 };
                let device = mounts.iter().find(|(_, m, _)| m == mp).map(|(d, _, _)| d.as_str()).unwrap_or("?");
                let fstype = mounts.iter().find(|(_, m, _)| m == mp).map(|(_, _, t)| t.as_str()).unwrap_or("?");
                if show_type {
                    if human {
                        println!("{:<20} {:<10} {:>10} {:>10} {:>10} {:>5}% {}", device, fstype, human_size(total*1024), human_size(used*1024), human_size(avail*1024), pct, mp);
                    } else {
                        println!("{:<20} {:<10} {:>10} {:>10} {:>10} {:>5}% {}", device, fstype, total, used, avail, pct, mp);
                    }
                } else if human {
                    println!("{:<20} {:>10} {:>10} {:>10} {:>5}% {}", device, human_size(total*1024), human_size(used*1024), human_size(avail*1024), pct, mp);
                } else {
                    println!("{:<20} {:>10} {:>10} {:>10} {:>5}% {}", device, total, used, avail, pct, mp);
                }
            }
        }
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = args;
        bail!("df: not supported on this platform");
    }
}

// ─── FIND ─────────────────────────────────────────────────────────────────────
fn cmd_find(args: &[String]) -> anyhow::Result<()> {
    // find [path...] [options]
    // Separate paths from options
    let mut paths: Vec<String> = Vec::new();
    let mut opts: Vec<String> = Vec::new();
    let mut in_opts = false;
    for a in args {
        if a.starts_with('-') || a == "!" { in_opts = true; }
        if in_opts { opts.push(a.clone()); } else { paths.push(a.clone()); }
    }
    if paths.is_empty() { paths.push(".".to_string()); }

    // Parse options
    let mut name_pattern: Option<String> = None;
    let mut ftype: Option<char> = None; // f, d, l
    let mut maxdepth: Option<usize> = None;
    let mut mindepth: Option<usize> = None;
    let mut size_filter: Option<(char, u64)> = None; // (+/-/=, bytes)
    let mut newer: Option<PathBuf> = None;
    let mut empty = false;
    let mut do_delete = false;
    let mut exec_cmd: Vec<String> = Vec::new();
    let mut print = true;
    let mut print0 = false;

    let mut i = 0;
    while i < opts.len() {
        match opts[i].as_str() {
            "-name" => { i += 1; name_pattern = opts.get(i).cloned(); }
            "-type" => { i += 1; ftype = opts.get(i).and_then(|s| s.chars().next()); }
            "-maxdepth" => { i += 1; maxdepth = opts.get(i).and_then(|s| s.parse().ok()); }
            "-mindepth" => { i += 1; mindepth = opts.get(i).and_then(|s| s.parse().ok()); }
            "-newer" => { i += 1; newer = opts.get(i).map(PathBuf::from); }
            "-empty" => { empty = true; }
            "-delete" => { do_delete = true; print = false; }
            "-print" => { print = true; }
            "-print0" => { print0 = true; print = false; }
            "-size" => {
                i += 1;
                if let Some(s) = opts.get(i) {
                    let (sign, rest) = if s.starts_with('+') { ('+', &s[1..]) }
                        else if s.starts_with('-') { ('-', &s[1..]) }
                        else { ('=', s.as_str()) };
                    let (num, mult): (u64, u64) = if rest.ends_with('c') {
                        (rest[..rest.len()-1].parse().unwrap_or(0), 1)
                    } else if rest.ends_with('k') {
                        (rest[..rest.len()-1].parse().unwrap_or(0), 1024)
                    } else if rest.ends_with('M') {
                        (rest[..rest.len()-1].parse().unwrap_or(0), 1024*1024)
                    } else if rest.ends_with('G') {
                        (rest[..rest.len()-1].parse().unwrap_or(0), 1024*1024*1024)
                    } else {
                        (rest.parse().unwrap_or(0), 512)
                    };
                    size_filter = Some((sign, num * mult));
                }
            }
            "-exec" => {
                i += 1;
                exec_cmd.clear();
                while i < opts.len() && opts[i] != ";" {
                    exec_cmd.push(opts[i].clone());
                    i += 1;
                }
                print = false;
            }
            _ => {}
        }
        i += 1;
    }

    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());

    let newer_mtime = newer.as_ref().and_then(|p| fs::metadata(p).ok())
        .and_then(|m| m.modified().ok());

    for start in &paths {
        find_walk(Path::new(start), 0, &name_pattern, ftype, maxdepth, mindepth,
            &size_filter, newer_mtime, empty, do_delete, &exec_cmd, print, print0, &mut out)?;
    }
    Ok(())
}

fn find_walk(
    p: &Path, depth: usize,
    name_pattern: &Option<String>,
    ftype: Option<char>,
    maxdepth: Option<usize>,
    mindepth: Option<usize>,
    size_filter: &Option<(char, u64)>,
    newer_mtime: Option<std::time::SystemTime>,
    empty: bool,
    do_delete: bool,
    exec_cmd: &[String],
    print: bool,
    print0: bool,
    out: &mut impl Write,
) -> anyhow::Result<()> {
    if let Some(md) = maxdepth { if depth > md { return Ok(()); } }

    let meta = match fs::symlink_metadata(p) { Ok(m) => m, Err(_) => return Ok(()) };
    let fname = p.file_name().and_then(|n| n.to_str()).unwrap_or("");

    let mut matches = true;

    if let Some(pat) = name_pattern {
        if !glob_match(pat, fname) { matches = false; }
    }
    if let Some(ft) = ftype {
        let ok = match ft {
            'f' => meta.is_file(),
            'd' => meta.is_dir(),
            'l' => meta.file_type().is_symlink(),
            _ => true,
        };
        if !ok { matches = false; }
    }
    if let Some((sign, size)) = size_filter {
        let fsize = meta.len();
        let ok = match sign {
            '+' => fsize > *size,
            '-' => fsize < *size,
            _ => fsize == *size,
        };
        if !ok { matches = false; }
    }
    if let Some(nmt) = newer_mtime {
        let fmtime = meta.modified().ok();
        match fmtime {
            Some(ft) => if ft <= nmt { matches = false; },
            None => { matches = false; }
        }
    }
    if empty {
        let is_empty = if meta.is_dir() {
            fs::read_dir(p).map(|mut rd| rd.next().is_none()).unwrap_or(false)
        } else {
            meta.len() == 0
        };
        if !is_empty { matches = false; }
    }
    if mindepth.map(|md| depth < md).unwrap_or(false) { matches = false; }

    if matches {
        if print {
            writeln!(out, "{}", p.display())?;
        } else if print0 {
            write!(out, "{}\0", p.display())?;
        }
        if do_delete {
            if meta.is_dir() { let _ = fs::remove_dir(p); }
            else { let _ = fs::remove_file(p); }
        }
        if !exec_cmd.is_empty() {
            let path_str = p.to_string_lossy().to_string();
            let cmd_args: Vec<String> = exec_cmd.iter().map(|a| {
                if a == "{}" { path_str.clone() } else { a.clone() }
            }).collect();
            if !cmd_args.is_empty() {
                let _ = std::process::Command::new(&cmd_args[0]).args(&cmd_args[1..]).status();
            }
        }
    }

    if meta.is_dir() {
        if let Ok(rd) = fs::read_dir(p) {
            for e in rd.flatten() {
                find_walk(&e.path(), depth + 1, name_pattern, ftype, maxdepth, mindepth,
                    size_filter, newer_mtime, empty, do_delete, exec_cmd, print, print0, out)?;
            }
        }
    }
    Ok(())
}

// ─── HEAD ─────────────────────────────────────────────────────────────────────
fn cmd_head(args: &[String]) -> anyhow::Result<()> {
    let (_, values, positional, _) = parse_flags(args, "", "nc");
    let n: usize = values.get(&'n').and_then(|v| v.parse().ok()).unwrap_or(10);
    let bytes: Option<usize> = values.get(&'c').and_then(|v| v.parse().ok());

    let process = |reader: &mut dyn BufRead| -> anyhow::Result<()> {
        if let Some(b) = bytes {
            let mut buf = vec![0u8; b];
            let read = reader.read(&mut buf)?;
            io::stdout().write_all(&buf[..read])?;
        } else {
            for line in reader.lines().take(n) {
                println!("{}", line?);
            }
        }
        Ok(())
    };

    if positional.is_empty() {
        process(&mut io::BufReader::new(io::stdin()))?;
    } else {
        let multi = positional.len() > 1;
        for f in &positional {
            if multi { println!("==> {f} <=="); }
            if f == "-" {
                process(&mut io::BufReader::new(io::stdin()))?;
            } else {
                let file = File::open(f).with_context(|| format!("head: {f}"))?;
                process(&mut io::BufReader::new(file))?;
            }
        }
    }
    Ok(())
}

// ─── TAIL ─────────────────────────────────────────────────────────────────────
fn cmd_tail(args: &[String]) -> anyhow::Result<()> {
    let (flags, values, positional, _) = parse_flags(args, "f", "nc");
    let n: usize = values.get(&'n').and_then(|v| v.parse().ok()).unwrap_or(10);
    let bytes: Option<usize> = values.get(&'c').and_then(|v| v.parse().ok());
    let follow = flags.contains(&'f');

    let process_lines = |reader: &mut dyn BufRead| -> anyhow::Result<()> {
        let lines: Vec<String> = reader.lines().collect::<Result<Vec<_>, _>>()?;
        let skip = lines.len().saturating_sub(n);
        for l in &lines[skip..] { println!("{l}"); }
        Ok(())
    };

    let process_bytes = |b: usize, reader: &mut dyn BufRead| -> anyhow::Result<()> {
        let mut data = Vec::new();
        reader.read_to_end(&mut data)?;
        let skip = data.len().saturating_sub(b);
        io::stdout().write_all(&data[skip..])?;
        Ok(())
    };

    if positional.is_empty() {
        if let Some(b) = bytes {
            process_bytes(b, &mut io::BufReader::new(io::stdin()))?;
        } else {
            process_lines(&mut io::BufReader::new(io::stdin()))?;
        }
    } else {
        let multi = positional.len() > 1;
        for f in &positional {
            if multi { println!("==> {f} <=="); }
            if let Some(b) = bytes {
                let file = File::open(f).with_context(|| format!("tail: {f}"))?;
                process_bytes(b, &mut io::BufReader::new(file))?;
            } else {
                let file = File::open(f).with_context(|| format!("tail: {f}"))?;
                process_lines(&mut io::BufReader::new(file))?;
            }
        }
        if follow && !positional.is_empty() {
            let fname = positional.last().unwrap();
            let mut file = File::open(fname).with_context(|| format!("tail -f: {fname}"))?;
            file.seek(std::io::SeekFrom::End(0))?;
            loop {
                let mut buf = String::new();
                let mut reader = io::BufReader::new(&file);
                match reader.read_line(&mut buf) {
                    Ok(0) => std::thread::sleep(Duration::from_millis(200)),
                    Ok(_) => print!("{buf}"),
                    Err(_) => break,
                }
                io::stdout().flush()?;
            }
        }
    }
    Ok(())
}

use std::io::Seek;

// ─── WC ───────────────────────────────────────────────────────────────────────
fn cmd_wc(args: &[String]) -> anyhow::Result<()> {
    let (flags, _, positional, _) = parse_flags(args, "lwcmL", "");
    let count_lines = flags.contains(&'l');
    let count_words = flags.contains(&'w');
    let count_bytes = flags.contains(&'c');
    let count_chars = flags.contains(&'m');
    let longest = flags.contains(&'L');
    let all = !count_lines && !count_words && !count_bytes && !count_chars && !longest;

    let count = |content: &str| -> (usize, usize, usize, usize, usize) {
        let lines = content.lines().count();
        let words = content.split_whitespace().count();
        let bytes = content.len();
        let chars = content.chars().count();
        let maxlen = content.lines().map(|l| l.len()).max().unwrap_or(0);
        (lines, words, bytes, chars, maxlen)
    };

    let print_counts = |name: &str, lines: usize, words: usize, bytes: usize, chars: usize, maxlen: usize| {
        let mut parts = Vec::new();
        if all || count_lines { parts.push(format!("{lines:>8}")); }
        if all || count_words { parts.push(format!("{words:>8}")); }
        if all || count_bytes { parts.push(format!("{bytes:>8}")); }
        if count_chars { parts.push(format!("{chars:>8}")); }
        if longest { parts.push(format!("{maxlen:>8}")); }
        if name.is_empty() { println!("{}", parts.join("")); }
        else { println!("{} {}", parts.join(""), name); }
    };

    if positional.is_empty() {
        let mut content = String::new();
        io::stdin().read_to_string(&mut content)?;
        let (l, w, b, c, m) = count(&content);
        print_counts("", l, w, b, c, m);
    } else {
        let (mut tl, mut tw, mut tb, mut tc, mut tm) = (0, 0, 0, 0, 0);
        for f in &positional {
            let content = fs::read_to_string(f).with_context(|| format!("wc: {f}"))?;
            let (l, w, b, c, mx) = count(&content);
            tl += l; tw += w; tb += b; tc += c; tm = tm.max(mx);
            print_counts(f, l, w, b, c, mx);
        }
        if positional.len() > 1 {
            print_counts("total", tl, tw, tb, tc, tm);
        }
    }
    Ok(())
}

// ─── TEE ──────────────────────────────────────────────────────────────────────
fn cmd_tee(args: &[String]) -> anyhow::Result<()> {
    let (flags, _, positional, _) = parse_flags(args, "a", "");
    let append = flags.contains(&'a');
    let mut files: Vec<Box<dyn Write>> = positional.iter().map(|f| -> anyhow::Result<Box<dyn Write>> {
        let file = OpenOptions::new().write(true).create(true).append(append).truncate(!append).open(f)
            .with_context(|| format!("tee: {f}"))?;
        Ok(Box::new(file))
    }).collect::<anyhow::Result<Vec<_>>>()?;

    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = stdout.lock();
    for line in stdin.lock().lines() {
        let line = line?;
        writeln!(out, "{line}")?;
        for f in &mut files { writeln!(f, "{line}")?; }
    }
    Ok(())
}

// ─── GREP ─────────────────────────────────────────────────────────────────────
fn cmd_grep(args: &[String]) -> anyhow::Result<()> {
    let (flags, values, mut positional, _) = parse_flags(args, "ivnclqrREFowh", "ACBme");
    let ignore_case = flags.contains(&'i');
    let invert = flags.contains(&'v');
    let line_numbers = flags.contains(&'n');
    let count_only = flags.contains(&'c');
    let files_with_matches = flags.contains(&'l');
    let quiet = flags.contains(&'q');
    let recursive = flags.contains(&'r') || flags.contains(&'R');
    let use_regex = flags.contains(&'E');
    let fixed = flags.contains(&'F');
    let only_matching = flags.contains(&'o');
    let word = flags.contains(&'w');

    let after: usize = values.get(&'A').and_then(|v| v.parse().ok()).unwrap_or(0);
    let before: usize = values.get(&'B').and_then(|v| v.parse().ok()).unwrap_or(0);
    let context_n: usize = values.get(&'C').and_then(|v| v.parse().ok()).unwrap_or(0);
    let max_matches: Option<usize> = values.get(&'m').and_then(|v| v.parse().ok());
    let after = after.max(context_n);
    let before = before.max(context_n);

    // Patterns: -e can be specified multiple times, handled via last value
    let mut patterns: Vec<String> = Vec::new();
    if let Some(p) = values.get(&'e') { patterns.push(p.clone()); }

    if patterns.is_empty() {
        if positional.is_empty() { bail!("grep: pattern manquant"); }
        patterns.push(positional.remove(0));
    }

    let match_fn: Box<dyn Fn(&str) -> Option<(usize, usize)>> = if use_regex {
        let pats: Vec<regex::Regex> = patterns.iter().map(|p| {
            let p = if ignore_case { format!("(?i){p}") } else { p.clone() };
            regex::Regex::new(&p).expect("regex invalide")
        }).collect();
        Box::new(move |line: &str| {
            for re in &pats {
                if let Some(m) = re.find(line) { return Some((m.start(), m.end())); }
            }
            None
        })
    } else if fixed {
        let pats = patterns.clone();
        Box::new(move |line: &str| {
            for p in &pats {
                let (l, pat) = if ignore_case { (line.to_lowercase(), p.to_lowercase()) } else { (line.to_string(), p.clone()) };
                if let Some(pos) = l.find(&pat) { return Some((pos, pos + pat.len())); }
            }
            None
        })
    } else {
        // literal or basic
        let pats = patterns.clone();
        Box::new(move |line: &str| {
            for p in &pats {
                let (l, pat) = if ignore_case { (line.to_lowercase(), p.to_lowercase()) } else { (line.to_string(), p.clone()) };
                if let Some(pos) = l.find(pat.as_str()) { return Some((pos, pos + pat.len())); }
            }
            None
        })
    };

    let check_match = |line: &str| -> bool {
        let m = match_fn(line);
        let found = if word {
            if let Some((s, e)) = m {
                let before_ok = s == 0 || !line.chars().nth(s.saturating_sub(1)).map(|c| c.is_alphanumeric() || c == '_').unwrap_or(false);
                let after_ok = e >= line.len() || !line.chars().nth(e).map(|c| c.is_alphanumeric() || c == '_').unwrap_or(false);
                before_ok && after_ok
            } else { false }
        } else { m.is_some() };
        if invert { !found } else { found }
    };

    let highlight = |line: &str| -> String {
        if quiet || count_only || files_with_matches { return line.to_string(); }
        if let Some((s, e)) = match_fn(line) {
            format!("{}{RED_BOLD}{}{RESET}{}", &line[..s], &line[s..e], &line[e..])
        } else {
            line.to_string()
        }
    };

    let mut found_any = false;

    let process_lines = |lines: &[String], fname: &str, show_fname: bool| -> anyhow::Result<bool> {
        let mut count = 0usize;
        let mut match_count = 0usize;
        let mut context_buf: Vec<(usize, String)> = Vec::new();
        let mut pending_after = 0usize;
        let mut any = false;

        for (lineno, line) in lines.iter().enumerate() {
            let matched = check_match(line);
            if matched {
                if let Some(mm) = max_matches { if match_count >= mm { break; } }
                any = true;
                count += 1;
                match_count += 1;

                if !count_only && !files_with_matches && !quiet {
                    // print before context
                    for (bn, bl) in &context_buf {
                        let prefix = if show_fname { format!("{fname}-{bn}: ") } else if line_numbers { format!("{bn}: ") } else { String::new() };
                        println!("{prefix}{bl}");
                    }
                    context_buf.clear();

                    let prefix = if show_fname { format!("{fname}:{}: ", lineno+1) } else if line_numbers { format!("{}: ", lineno+1) } else { String::new() };
                    if only_matching {
                        if let Some((s, e)) = match_fn(line) {
                            println!("{prefix}{RED_BOLD}{}{RESET}", &line[s..e]);
                        }
                    } else {
                        println!("{prefix}{}", highlight(line));
                    }
                    pending_after = after;
                }
            } else {
                if pending_after > 0 {
                    if !count_only && !files_with_matches && !quiet {
                        let prefix = if show_fname { format!("{fname}-{}: ", lineno+1) } else if line_numbers { format!("{}: ", lineno+1) } else { String::new() };
                        println!("{prefix}{line}");
                    }
                    pending_after -= 1;
                    context_buf.clear();
                } else {
                    context_buf.push((lineno+1, line.clone()));
                    if context_buf.len() > before { context_buf.remove(0); }
                }
            }
        }

        if count_only {
            if show_fname { println!("{fname}:{count}"); } else { println!("{count}"); }
        } else if files_with_matches && any {
            println!("{fname}");
        }
        Ok(any)
    };

    let mut files: Vec<(String, bool)> = Vec::new(); // (path, show_fname)
    if positional.is_empty() {
        let lines = stdin_lines();
        let any = process_lines(&lines, "", false)?;
        if any { found_any = true; }
    } else {
        // Expand recursive
        let mut expanded: Vec<String> = Vec::new();
        for p in &positional {
            if recursive && Path::new(p).is_dir() {
                grep_walk(Path::new(p), &mut expanded);
            } else {
                expanded.push(p.clone());
            }
        }
        let show_fname = expanded.len() > 1;
        for f in &expanded { files.push((f.clone(), show_fname)); }
    }

    for (f, show_fname) in &files {
        match read_lines_file(f) {
            Ok(lines) => {
                let any = process_lines(&lines, f, *show_fname)?;
                if any { found_any = true; }
            }
            Err(e) => eprintln!("{RED}grep: {e}{RESET}"),
        }
    }

    if !found_any && !quiet { std::process::exit(1); }
    Ok(())
}

fn grep_walk(p: &Path, out: &mut Vec<String>) {
    if p.is_dir() {
        if let Ok(rd) = fs::read_dir(p) {
            for e in rd.flatten() { grep_walk(&e.path(), out); }
        }
    } else {
        out.push(p.to_string_lossy().to_string());
    }
}

// ─── SORT ─────────────────────────────────────────────────────────────────────
fn cmd_sort(args: &[String]) -> anyhow::Result<()> {
    let (flags, values, positional, _) = parse_flags(args, "runfb", "kto");
    let reverse = flags.contains(&'r');
    let unique = flags.contains(&'u');
    let numeric = flags.contains(&'n');
    let fold = flags.contains(&'f');
    let _ignore_blanks = flags.contains(&'b');
    let key: Option<String> = values.get(&'k').cloned();
    let sep: Option<char> = values.get(&'t').and_then(|v| v.chars().next());
    let output: Option<String> = values.get(&'o').cloned();

    let mut lines: Vec<String> = if positional.is_empty() {
        stdin_lines()
    } else {
        let mut all = Vec::new();
        for f in &positional { all.extend(read_lines_file(f)?); }
        all
    };

    lines.sort_by(|a, b| {
        let ka = extract_key(a, &key, sep);
        let kb = extract_key(b, &key, sep);
        if numeric {
            let na: f64 = ka.trim().parse().unwrap_or(0.0);
            let nb: f64 = kb.trim().parse().unwrap_or(0.0);
            na.partial_cmp(&nb).unwrap_or(std::cmp::Ordering::Equal)
        } else if fold {
            ka.to_lowercase().cmp(&kb.to_lowercase())
        } else {
            ka.cmp(&kb)
        }
    });
    if reverse { lines.reverse(); }
    if unique { lines.dedup_by(|a, b| a == b); }

    let content = lines.join("\n") + "\n";
    if let Some(out_file) = output {
        fs::write(out_file, content)?;
    } else {
        print!("{content}");
    }
    Ok(())
}

fn extract_key(line: &str, key: &Option<String>, sep: Option<char>) -> String {
    let Some(k) = key else { return line.to_string(); };
    let parts: Vec<&str> = if let Some(s) = sep {
        line.split(s).collect()
    } else {
        line.split_whitespace().collect()
    };
    // key format: N[.M][flags] or N,M[flags]
    let clean = k.trim_end_matches(|c: char| c.is_alphabetic());
    let (start_field, end_field): (usize, Option<usize>) = if let Some(comma) = clean.find(',') {
        let s: usize = clean[..comma].parse().unwrap_or(1);
        let e: usize = clean[comma+1..].parse().unwrap_or(s);
        (s, Some(e))
    } else {
        let s: usize = clean.parse().unwrap_or(1);
        (s, None)
    };
    let si = start_field.saturating_sub(1);
    let ei = end_field.map(|e| e.min(parts.len())).unwrap_or(parts.len());
    parts.get(si..ei).map(|s| s.join(" ")).unwrap_or_default()
}

// ─── UNIQ ─────────────────────────────────────────────────────────────────────
fn cmd_uniq(args: &[String]) -> anyhow::Result<()> {
    let (flags, values, positional, _) = parse_flags(args, "cudi", "fsw");
    let count = flags.contains(&'c');
    let unique_only = flags.contains(&'u');
    let dup_only = flags.contains(&'d');
    let ignore_case = flags.contains(&'i');
    let skip_fields: usize = values.get(&'f').and_then(|v| v.parse().ok()).unwrap_or(0);
    let skip_chars: usize = values.get(&'s').and_then(|v| v.parse().ok()).unwrap_or(0);
    let cmp_chars: Option<usize> = values.get(&'w').and_then(|v| v.parse().ok());

    let lines = if positional.is_empty() {
        stdin_lines()
    } else {
        read_lines_file(&positional[0])?
    };

    let key_of = |line: &str| -> String {
        let s: Vec<&str> = line.split_whitespace().collect();
        let s = if skip_fields < s.len() { s[skip_fields..].join(" ") } else { String::new() };
        let s = if skip_chars < s.len() { s[skip_chars..].to_string() } else { String::new() };
        let s = if let Some(n) = cmp_chars { s.chars().take(n).collect() } else { s };
        if ignore_case { s.to_lowercase() } else { s }
    };

    let mut groups: Vec<(String, usize)> = Vec::new(); // (line, count)
    for line in lines {
        let k = key_of(&line);
        if let Some(last) = groups.last_mut() {
            if key_of(&last.0) == k { last.1 += 1; continue; }
        }
        groups.push((line, 1));
    }

    for (line, n) in groups {
        if unique_only && n > 1 { continue; }
        if dup_only && n == 1 { continue; }
        if count { print!("{n:>7} "); }
        println!("{line}");
    }
    Ok(())
}

// ─── CUT ──────────────────────────────────────────────────────────────────────
fn cmd_cut(args: &[String]) -> anyhow::Result<()> {
    let (flags, values, positional, _) = parse_flags(args, "s", "dfbc");
    let delim: char = values.get(&'d').and_then(|v| v.chars().next()).unwrap_or('\t');
    let fields_str = values.get(&'f').cloned();
    let bytes_str = values.get(&'b').cloned();
    let chars_str = values.get(&'c').cloned();
    let skip_no_delim = flags.contains(&'s');

    let parse_ranges = |s: &str| -> Vec<(usize, Option<usize>)> {
        s.split(',').filter_map(|part| {
            let part = part.trim();
            if let Some(dash) = part.find('-') {
                let start: usize = part[..dash].parse().unwrap_or(1);
                let end: Option<usize> = part[dash+1..].parse().ok();
                Some((start, end))
            } else {
                let n: usize = part.parse().ok()?;
                Some((n, Some(n)))
            }
        }).collect()
    };

    let in_ranges = |n: usize, ranges: &[(usize, Option<usize>)]| -> bool {
        ranges.iter().any(|(s, e)| n >= *s && e.map(|end| n <= end).unwrap_or(true))
    };

    let process = |reader: &mut dyn BufRead| -> anyhow::Result<()> {
        let stdout = io::stdout();
        let mut out = io::BufWriter::new(stdout.lock());
        for line in reader.lines() {
            let line = line?;
            if let Some(ref fs) = fields_str {
                let ranges = parse_ranges(fs);
                if !line.contains(delim) {
                    if !skip_no_delim { writeln!(out, "{line}")?; }
                    continue;
                }
                let parts: Vec<&str> = line.split(delim).collect();
                let selected: Vec<&str> = parts.iter().enumerate()
                    .filter(|(i, _)| in_ranges(i+1, &ranges))
                    .map(|(_, s)| *s).collect();
                writeln!(out, "{}", selected.join(&delim.to_string()))?;
            } else if let Some(ref bs) = bytes_str {
                let ranges = parse_ranges(bs);
                let bytes = line.as_bytes();
                let selected: Vec<u8> = bytes.iter().enumerate()
                    .filter(|(i, _)| in_ranges(i+1, &ranges))
                    .map(|(_, &b)| b).collect();
                writeln!(out, "{}", String::from_utf8_lossy(&selected))?;
            } else if let Some(ref cs) = chars_str {
                let ranges = parse_ranges(cs);
                let chars: Vec<char> = line.chars().collect();
                let selected: String = chars.iter().enumerate()
                    .filter(|(i, _)| in_ranges(i+1, &ranges))
                    .map(|(_, &c)| c).collect();
                writeln!(out, "{selected}")?;
            }
        }
        Ok(())
    };

    if positional.is_empty() {
        process(&mut io::BufReader::new(io::stdin()))?;
    } else {
        for f in &positional {
            let file = File::open(f).with_context(|| format!("cut: {f}"))?;
            process(&mut io::BufReader::new(file))?;
        }
    }
    Ok(())
}

// ─── TR ───────────────────────────────────────────────────────────────────────
fn cmd_tr(args: &[String]) -> anyhow::Result<()> {
    let (flags, _, positional, _) = parse_flags(args, "dsc", "");
    let delete = flags.contains(&'d');
    let squeeze = flags.contains(&'s');
    let complement = flags.contains(&'c');

    if positional.is_empty() { bail!("tr: SET1 manquant"); }
    let set1 = expand_tr_set(&positional[0]);
    let set2 = if positional.len() > 1 { expand_tr_set(&positional[1]) } else { Vec::new() };

    let set1_chars: Vec<char> = if complement {
        let s1: std::collections::HashSet<char> = set1.iter().cloned().collect();
        (0u32..=127).filter_map(char::from_u32).filter(|c| !s1.contains(c)).collect()
    } else { set1.clone() };

    let translate = |c: char| -> Option<char> {
        if delete {
            if set1_chars.contains(&c) { None } else { Some(c) }
        } else {
            if let Some(pos) = set1_chars.iter().position(|&x| x == c) {
                let mapped = set2.get(pos).or_else(|| set2.last()).copied().unwrap_or(c);
                Some(mapped)
            } else { Some(c) }
        }
    };

    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());
    let mut prev: Option<char> = None;
    for byte in stdin.lock().bytes() {
        let c = byte? as char;
        if let Some(tc) = translate(c) {
            if squeeze && set2.contains(&tc) && prev == Some(tc) { continue; }
            write!(out, "{tc}")?;
            prev = Some(tc);
        } else {
            prev = None;
        }
    }
    Ok(())
}

fn expand_tr_set(s: &str) -> Vec<char> {
    let mut result = Vec::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '[' {
            // POSIX class or range
            if chars.peek() == Some(&':') {
                chars.next();
                let mut class = String::new();
                loop {
                    match chars.next() {
                        Some(':') => { chars.next(); break; } // consume ]
                        Some(c) => class.push(c),
                        None => break,
                    }
                }
                result.extend(expand_posix_class(&class));
            } else {
                result.push(c);
            }
        } else if c == '\\' {
            match chars.next() {
                Some('n') => result.push('\n'),
                Some('t') => result.push('\t'),
                Some('r') => result.push('\r'),
                Some(x) => result.push(x),
                None => {}
            }
        } else if chars.peek() == Some(&'-') {
            chars.next();
            if let Some(end) = chars.next() {
                for code in (c as u32)..=(end as u32) {
                    if let Some(ch) = char::from_u32(code) { result.push(ch); }
                }
            }
        } else {
            result.push(c);
        }
    }
    result
}

fn expand_posix_class(class: &str) -> Vec<char> {
    match class {
        "alpha" => ('a'..='z').chain('A'..='Z').collect(),
        "digit" => ('0'..='9').collect(),
        "lower" => ('a'..='z').collect(),
        "upper" => ('A'..='Z').collect(),
        "space" => vec![' ', '\t', '\n', '\r', '\x0c', '\x0b'],
        "punct" => r##"!"#$%&'()*+,-./:;<=>?@[\]^_`{|}~"##.chars().collect(),
        "alnum" => ('a'..='z').chain('A'..='Z').chain('0'..='9').collect(),
        "blank" => vec![' ', '\t'],
        "print" => (' '..='~').collect(),
        "graph" => ('!'..='~').collect(),
        "cntrl" => (0u8..32u8).chain(std::iter::once(127u8)).map(|b| b as char).collect(),
        _ => Vec::new(),
    }
}

// ─── SEQ ──────────────────────────────────────────────────────────────────────
fn cmd_seq(args: &[String]) -> anyhow::Result<()> {
    let (flags, values, positional, _) = parse_flags(args, "w", "s");
    let sep = values.get(&'s').cloned().unwrap_or_else(|| "\n".to_string());
    let equalize = flags.contains(&'w');

    let (first, incr, last) = match positional.len() {
        1 => (1.0f64, 1.0, positional[0].parse::<f64>().context("seq: LAST invalide")?),
        2 => (positional[0].parse::<f64>().context("seq: FIRST invalide")?,
              1.0,
              positional[1].parse::<f64>().context("seq: LAST invalide")?),
        3 => (positional[0].parse::<f64>().context("seq: FIRST invalide")?,
              positional[1].parse::<f64>().context("seq: INCR invalide")?,
              positional[2].parse::<f64>().context("seq: LAST invalide")?),
        _ => bail!("seq: arguments invalides"),
    };

    let mut values_list: Vec<String> = Vec::new();
    let mut v = first;
    if incr > 0.0 {
        while v <= last + 1e-10 { values_list.push(format!("{v}")); v += incr; }
    } else if incr < 0.0 {
        while v >= last - 1e-10 { values_list.push(format!("{v}")); v += incr; }
    }

    if equalize {
        let max_len = values_list.iter().map(|s| s.len()).max().unwrap_or(0);
        values_list = values_list.into_iter().map(|s| format!("{s:>0$}", max_len)).collect();
    }

    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());
    for (i, v) in values_list.iter().enumerate() {
        if i > 0 { write!(out, "{sep}")?; }
        write!(out, "{v}")?;
    }
    writeln!(out)?;
    Ok(())
}

// ─── DIFF ─────────────────────────────────────────────────────────────────────
fn cmd_diff(args: &[String]) -> anyhow::Result<()> {
    let (flags, _, positional, _) = parse_flags(args, "quib", "");
    let quiet = flags.contains(&'q');
    let ignore_case = flags.contains(&'i');
    let ignore_blanks = flags.contains(&'b');

    if positional.len() < 2 { bail!("diff: deux fichiers requis"); }
    let file1 = &positional[0];
    let file2 = &positional[1];

    let mut lines1 = read_lines_file(file1)?;
    let mut lines2 = read_lines_file(file2)?;

    if ignore_case { lines1.iter_mut().for_each(|l| *l = l.to_lowercase()); lines2.iter_mut().for_each(|l| *l = l.to_lowercase()); }
    if ignore_blanks {
        let norm = |s: &String| s.split_whitespace().collect::<Vec<_>>().join(" ");
        lines1 = lines1.iter().map(norm).collect();
        lines2 = lines2.iter().map(norm).collect();
    }

    let hunks = diff_lines(&lines1, &lines2);
    if quiet {
        if !hunks.is_empty() { println!("Files {file1} and {file2} differ"); }
        return Ok(());
    }

    if hunks.is_empty() { return Ok(()); }

    println!("--- {file1}");
    println!("+++ {file2}");
    for (r1, r2, removed, added) in &hunks {
        let l1 = r1.0 + 1; let l1e = r1.0 + r1.1;
        let l2 = r2.0 + 1; let l2e = r2.0 + r2.1;
        println!("@@ -{l1},{} +{l2},{} @@", r1.1, r2.1);
        for line in removed { println!("-{line}"); }
        for line in added { println!("+{line}"); }
        let _ = (l1e, l2e);
    }
    Ok(())
}

type DiffHunk = ((usize, usize), (usize, usize), Vec<String>, Vec<String>);

fn diff_lines(a: &[String], b: &[String]) -> Vec<DiffHunk> {
    let lcs = lcs(a, b);
    let mut hunks: Vec<DiffHunk> = Vec::new();
    let mut ai = 0usize; let mut bi = 0usize; let mut li = 0usize;
    let mut removed: Vec<String> = Vec::new();
    let mut added: Vec<String> = Vec::new();
    let mut hunk_start_a = 0usize; let mut hunk_start_b = 0usize;
    let mut in_hunk = false;

    let flush = |h: &mut Vec<DiffHunk>, rem: &mut Vec<String>, add: &mut Vec<String>, sa: usize, sb: usize, in_h: &mut bool| {
        if !rem.is_empty() || !add.is_empty() {
            h.push(((sa, rem.len()), (sb, add.len()), rem.clone(), add.clone()));
            rem.clear(); add.clear(); *in_h = false;
        }
    };

    while ai < a.len() || bi < b.len() {
        if li < lcs.len() && ai < a.len() && bi < b.len() && a[ai] == lcs[li] && b[bi] == lcs[li] {
            flush(&mut hunks, &mut removed, &mut added, hunk_start_a, hunk_start_b, &mut in_hunk);
            ai += 1; bi += 1; li += 1;
        } else {
            if !in_hunk { hunk_start_a = ai; hunk_start_b = bi; in_hunk = true; }
            // Determine what's different
            let a_in_lcs = lcs.get(li).map(|l| ai < a.len() && &a[ai] == l).unwrap_or(false);
            let b_in_lcs = lcs.get(li).map(|l| bi < b.len() && &b[bi] == l).unwrap_or(false);
            if !a_in_lcs && ai < a.len() { removed.push(a[ai].clone()); ai += 1; }
            else if !b_in_lcs && bi < b.len() { added.push(b[bi].clone()); bi += 1; }
            else { if ai < a.len() { removed.push(a[ai].clone()); ai += 1; } if bi < b.len() { added.push(b[bi].clone()); bi += 1; } }
        }
    }
    flush(&mut hunks, &mut removed, &mut added, hunk_start_a, hunk_start_b, &mut in_hunk);
    hunks
}

fn lcs(a: &[String], b: &[String]) -> Vec<String> {
    let m = a.len(); let n = b.len();
    let mut dp = vec![vec![0usize; n+1]; m+1];
    for i in 1..=m { for j in 1..=n {
        dp[i][j] = if a[i-1] == b[j-1] { dp[i-1][j-1] + 1 } else { dp[i-1][j].max(dp[i][j-1]) };
    }}
    let mut result = Vec::new();
    let (mut i, mut j) = (m, n);
    while i > 0 && j > 0 {
        if a[i-1] == b[j-1] { result.push(a[i-1].clone()); i -= 1; j -= 1; }
        else if dp[i-1][j] > dp[i][j-1] { i -= 1; } else { j -= 1; }
    }
    result.reverse();
    result
}

// ─── XARGS ────────────────────────────────────────────────────────────────────
fn cmd_xargs(args: &[String]) -> anyhow::Result<()> {
    let (flags, values, positional, _) = parse_flags(args, "p0", "nId");
    let n: usize = values.get(&'n').and_then(|v| v.parse().ok()).unwrap_or(usize::MAX);
    let replace = values.get(&'I').cloned();
    let delim: Option<char> = values.get(&'d').and_then(|v| v.chars().next());
    let null_delim = flags.contains(&'0');
    let interactive = flags.contains(&'p');

    let cmd = if positional.is_empty() { vec!["echo".to_string()] } else { positional.clone() };

    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;

    let items: Vec<String> = if null_delim {
        input.split('\0').filter(|s| !s.is_empty()).map(|s| s.to_string()).collect()
    } else if let Some(d) = delim {
        input.split(d).filter(|s| !s.is_empty()).map(|s| s.trim().to_string()).collect()
    } else {
        input.split_whitespace().map(|s| s.to_string()).collect()
    };

    if let Some(repl) = replace {
        for item in &items {
            let cmd_args: Vec<String> = cmd.iter().map(|a| a.replace(&repl, item)).collect();
            if interactive {
                eprint!("{} ?", cmd_args.join(" "));
                let mut ans = String::new();
                io::stdin().read_line(&mut ans)?;
                if !ans.trim().eq_ignore_ascii_case("y") { continue; }
            }
            std::process::Command::new(&cmd_args[0]).args(&cmd_args[1..]).status()?;
        }
    } else {
        for chunk in items.chunks(n) {
            let mut full_cmd = cmd.clone();
            full_cmd.extend_from_slice(chunk);
            if interactive {
                eprint!("{} ?", full_cmd.join(" "));
                let mut ans = String::new();
                io::stdin().read_line(&mut ans)?;
                if !ans.trim().eq_ignore_ascii_case("y") { continue; }
            }
            std::process::Command::new(&full_cmd[0]).args(&full_cmd[1..]).status()?;
        }
    }
    Ok(())
}

// ─── ECHO ─────────────────────────────────────────────────────────────────────
fn cmd_echo(args: &[String]) -> anyhow::Result<()> {
    let mut no_newline = false;
    let mut interpret = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-n" => no_newline = true,
            "-e" => interpret = true,
            "-E" => interpret = false,
            _ => break,
        }
        i += 1;
    }
    let text = args[i..].join(" ");
    let output = if interpret { interpret_escapes(&text) } else { text };
    if no_newline { print!("{output}"); } else { println!("{output}"); }
    io::stdout().flush()?;
    Ok(())
}

fn interpret_escapes(s: &str) -> String {
    let mut result = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => result.push('\n'),
                Some('t') => result.push('\t'),
                Some('r') => result.push('\r'),
                Some('\\') => result.push('\\'),
                Some('a') => result.push('\x07'),
                Some('b') => result.push('\x08'),
                Some('0') => result.push('\0'),
                Some('e') => result.push('\x1b'),
                Some(x) => { result.push('\\'); result.push(x); }
                None => result.push('\\'),
            }
        } else { result.push(c); }
    }
    result
}

// ─── PWD ──────────────────────────────────────────────────────────────────────
fn cmd_pwd(_args: &[String]) -> anyhow::Result<()> {
    println!("{}", env::current_dir()?.display());
    Ok(())
}

// ─── WHICH ────────────────────────────────────────────────────────────────────
fn cmd_which(args: &[String]) -> anyhow::Result<()> {
    let (flags, _, positional, _) = parse_flags(args, "a", "");
    let all = flags.contains(&'a');
    for name in &positional {
        let results = find_in_path(name);
        if results.is_empty() { eprintln!("which: {name}: not found"); }
        for p in &results {
            println!("{}", p.display());
            if !all { break; }
        }
    }
    Ok(())
}

// ─── UNIX USER/GROUP HELPERS ──────────────────────────────────────────────────
#[cfg(unix)]
extern "C" {
    fn getuid() -> u32;
    fn getgid() -> u32;
}

#[cfg(unix)]
unsafe fn libc_getuid() -> u32 { getuid() }
#[cfg(unix)]
unsafe fn libc_getgid() -> u32 { getgid() }

#[cfg(unix)]
fn uid_to_name(uid: u32) -> Option<String> {
    let passwd = fs::read_to_string("/etc/passwd").ok()?;
    for line in passwd.lines() {
        let parts: Vec<&str> = line.split(':').collect();
        if parts.len() >= 3 && parts[2].parse::<u32>().ok() == Some(uid) {
            return Some(parts[0].to_string());
        }
    }
    None
}

#[cfg(unix)]
fn gid_to_name(gid: u32) -> Option<String> {
    let group = fs::read_to_string("/etc/group").ok()?;
    for line in group.lines() {
        let parts: Vec<&str> = line.split(':').collect();
        if parts.len() >= 3 && parts[2].parse::<u32>().ok() == Some(gid) {
            return Some(parts[0].to_string());
        }
    }
    None
}

#[cfg(unix)]
fn get_groups() -> Vec<u32> {
    // Read from /proc/self/status
    if let Ok(s) = fs::read_to_string("/proc/self/status") {
        for line in s.lines() {
            if line.starts_with("Groups:") {
                return line[7..].split_whitespace()
                    .filter_map(|s| s.parse().ok())
                    .collect();
            }
        }
    }
    vec![unsafe { libc_getgid() }]
}

// ─── WHOAMI ───────────────────────────────────────────────────────────────────
fn cmd_whoami(_args: &[String]) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        let uid = unsafe { libc_getuid() };
        let name = uid_to_name(uid).unwrap_or_else(|| uid.to_string());
        println!("{name}");
        return Ok(());
    }
    #[cfg(not(unix))]
    {
        println!("{}", env::var("USERNAME").or_else(|_| env::var("USER")).unwrap_or_else(|_| "unknown".to_string()));
        Ok(())
    }
}

// ─── UNAME ────────────────────────────────────────────────────────────────────
fn cmd_uname(args: &[String]) -> anyhow::Result<()> {
    let (flags, _, _, _) = parse_flags(args, "asnrvmpo", "");
    let all = flags.contains(&'a');

    let sysname = "Linux";
    let nodename = fs::read_to_string("/proc/sys/kernel/hostname").unwrap_or_else(|_| "unknown".to_string()).trim().to_string();
    let release = fs::read_to_string("/proc/version")
        .unwrap_or_default()
        .split_whitespace().nth(2).unwrap_or("unknown").to_string();
    let version = fs::read_to_string("/proc/version").unwrap_or_default().trim().to_string();
    let machine = std::env::consts::ARCH;
    let os = "GNU/Linux";

    let mut parts = Vec::new();
    if all || flags.contains(&'s') { parts.push(sysname.to_string()); }
    if all || flags.contains(&'n') { parts.push(nodename); }
    if all || flags.contains(&'r') { parts.push(release); }
    if all || flags.contains(&'v') { parts.push(version); }
    if all || flags.contains(&'m') { parts.push(machine.to_string()); }
    if all || flags.contains(&'p') { parts.push(machine.to_string()); }
    if all || flags.contains(&'o') { parts.push(os.to_string()); }
    if parts.is_empty() { parts.push(sysname.to_string()); }
    println!("{}", parts.join(" "));
    Ok(())
}

// ─── DATE ─────────────────────────────────────────────────────────────────────
fn cmd_date(args: &[String]) -> anyhow::Result<()> {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let fmt = args.iter().find(|a| a.starts_with('+')).map(|a| &a[1..]).unwrap_or("%a %b %e %T %Z %Y");
    println!("{}", format_date(fmt, secs));
    Ok(())
}

// ─── SLEEP ────────────────────────────────────────────────────────────────────
fn cmd_sleep(args: &[String]) -> anyhow::Result<()> {
    for a in args {
        let (num, mult): (f64, f64) = if a.ends_with('m') {
            (a[..a.len()-1].parse()?, 60.0)
        } else if a.ends_with('h') {
            (a[..a.len()-1].parse()?, 3600.0)
        } else if a.ends_with('s') {
            (a[..a.len()-1].parse()?, 1.0)
        } else {
            (a.parse()?, 1.0)
        };
        std::thread::sleep(Duration::from_secs_f64(num * mult));
    }
    Ok(())
}

// ─── YES ──────────────────────────────────────────────────────────────────────
fn cmd_yes(args: &[String]) -> anyhow::Result<()> {
    let word = if args.is_empty() { "y".to_string() } else { args.join(" ") };
    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());
    loop { writeln!(out, "{word}")?; }
}

// ─── ENV ──────────────────────────────────────────────────────────────────────
fn cmd_env(args: &[String]) -> anyhow::Result<()> {
    if args.is_empty() {
        for (k, v) in env::vars() { println!("{k}={v}"); }
    } else {
        for a in args {
            if let Some(val) = env::var_os(a) { println!("{}", val.to_string_lossy()); }
        }
    }
    Ok(())
}

// ─── ID ───────────────────────────────────────────────────────────────────────
fn cmd_id(_args: &[String]) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        let uid = unsafe { libc_getuid() };
        let gid = unsafe { libc_getgid() };
        let uname = uid_to_name(uid).unwrap_or_default();
        let gname = gid_to_name(gid).unwrap_or_default();
        let groups = get_groups();
        let groups_str: Vec<String> = groups.iter().map(|&g| {
            let gn = gid_to_name(g).unwrap_or_default();
            format!("{g}({gn})")
        }).collect();
        println!("uid={uid}({uname}) gid={gid}({gname}) groups={}", groups_str.join(","));
        return Ok(());
    }
    #[cfg(not(unix))]
    {
        println!("uid=0 gid=0");
        Ok(())
    }
}

// ─── PS ───────────────────────────────────────────────────────────────────────
fn cmd_ps(args: &[String]) -> anyhow::Result<()> {
    #[cfg(target_os = "linux")]
    {
        let (flags, _, _, _) = parse_flags(args, "eaulf", "");
        let all = flags.contains(&'e') || flags.contains(&'a');
        let user_fmt = flags.contains(&'u');
        let full = flags.contains(&'f');

        if user_fmt || full {
            println!("{:<8} {:>6} {:>6} {:>4} {:>4} {:>8} {}", "USER", "PID", "PPID", "%CPU", "%MEM", "TIME", "CMD");
        } else {
            println!("{:>6} {} {}", "PID", "TTY", "CMD");
        }

        let proc_dir = Path::new("/proc");
        let my_uid = unsafe { libc_getuid() };

        if let Ok(rd) = fs::read_dir(proc_dir) {
            let mut pids: Vec<u32> = rd.flatten()
                .filter_map(|e| e.file_name().to_string_lossy().parse::<u32>().ok())
                .collect();
            pids.sort();

            for pid in pids {
                let proc_path = proc_dir.join(pid.to_string());
                let stat_path = proc_path.join("stat");
                let cmdline_path = proc_path.join("cmdline");
                let status_path = proc_path.join("status");

                let stat = match fs::read_to_string(&stat_path) { Ok(s) => s, Err(_) => continue };
                let cmdline = fs::read_to_string(&cmdline_path).unwrap_or_default()
                    .replace('\0', " ").trim().to_string();
                let status = fs::read_to_string(&status_path).unwrap_or_default();

                // Get UID from status
                let proc_uid: u32 = status.lines()
                    .find(|l| l.starts_with("Uid:"))
                    .and_then(|l| l.split_whitespace().nth(1))
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(u32::MAX);

                if !all && proc_uid != my_uid { continue; }

                // Parse stat: pid (name) state ppid ...
                let paren_end = stat.rfind(')').unwrap_or(0);
                let rest: Vec<&str> = stat[paren_end+2..].split_whitespace().collect();
                let ppid: u32 = rest.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
                let paren_open = stat.find('(').unwrap_or(0);
                let comm = stat[paren_open+1..paren_end].to_string();
                let cmd = if cmdline.is_empty() { format!("[{comm}]") } else { cmdline.clone() };

                let uname = uid_to_name(proc_uid).unwrap_or_else(|| proc_uid.to_string());

                if user_fmt || full {
                    println!("{:<8} {:>6} {:>6} {:>4} {:>4} {:>8} {}", uname, pid, ppid, "0.0", "0.0", "00:00:00", cmd);
                } else {
                    println!("{:>6} {} {}", pid, "?", cmd);
                }
            }
        }
        return Ok(());
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = args;
        bail!("ps: not supported on this platform");
    }
}

// ─── KILL ─────────────────────────────────────────────────────────────────────
fn cmd_kill(args: &[String]) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use nix::sys::signal::{kill, Signal};
        use nix::unistd::Pid;

        if args.iter().any(|a| a == "-l") {
            let sigs = ["HUP","INT","QUIT","ILL","TRAP","ABRT","BUS","FPE","KILL","USR1",
                        "SEGV","USR2","PIPE","ALRM","TERM","STKFLT","CHLD","CONT","STOP",
                        "TSTP","TTIN","TTOU","URG","XCPU","XFSZ","VTALRM","PROF","WINCH",
                        "POLL","PWR","SYS"];
            for (i, s) in sigs.iter().enumerate() { print!("{:2}) SIG{s}  ", i+1); if (i+1)%4==0 { println!(); } }
            println!();
            return Ok(());
        }

        let mut sig = Signal::SIGTERM;
        let mut pids: Vec<i32> = Vec::new();

        for a in args {
            if a.starts_with('-') {
                let signame = &a[1..];
                sig = signame.parse::<i32>()
                    .ok()
                    .and_then(|n| Signal::try_from(n).ok())
                    .or_else(|| {
                        let upper = format!("SIG{}", signame.to_uppercase());
                        Signal::iterator().find(|s| s.as_str() == upper.as_str() || s.as_str() == signame.to_uppercase().as_str())
                    })
                    .unwrap_or(Signal::SIGTERM);
            } else {
                if let Ok(pid) = a.parse::<i32>() { pids.push(pid); }
            }
        }

        for pid in pids {
            kill(Pid::from_raw(pid), sig).with_context(|| format!("kill: pid {pid}"))?;
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = args;
        bail!("kill: not supported on this platform");
    }
}

// ─── BASENAME / DIRNAME / REALPATH ───────────────────────────────────────────
fn cmd_basename(args: &[String]) -> anyhow::Result<()> {
    if args.is_empty() { bail!("basename: argument manquant"); }
    let p = Path::new(&args[0]);
    let mut name = p.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
    if let Some(suffix) = args.get(1) {
        if name.ends_with(suffix.as_str()) { name = name[..name.len()-suffix.len()].to_string(); }
    }
    println!("{name}");
    Ok(())
}

fn cmd_dirname(args: &[String]) -> anyhow::Result<()> {
    if args.is_empty() { bail!("dirname: argument manquant"); }
    let p = Path::new(&args[0]);
    println!("{}", p.parent().unwrap_or(Path::new(".")).display());
    Ok(())
}

fn cmd_realpath(args: &[String]) -> anyhow::Result<()> {
    for a in args {
        let p = fs::canonicalize(a).with_context(|| format!("realpath: {a}"))?;
        println!("{}", p.display());
    }
    Ok(())
}

// ─── DATE UTILS ──────────────────────────────────────────────────────────────
/// Returns (year, month, day, hour, min, sec, weekday 0=Sun, yday)
fn unix_to_datetime(secs: u64) -> (u32, u32, u32, u32, u32, u32, u32, u32) {
    let sec = (secs % 60) as u32;
    let min = ((secs / 60) % 60) as u32;
    let hour = ((secs / 3600) % 24) as u32;
    let days = secs / 86400;
    let weekday = ((days + 4) % 7) as u32; // 0=Sun
    let mut year = 1970u32;
    let mut rem = days;
    loop {
        let dy = if is_leap(year) { 366 } else { 365 };
        if rem < dy { break; }
        rem -= dy; year += 1;
    }
    let mut month = 1u32;
    let mdays = [31, if is_leap(year) { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    for &md in &mdays {
        if rem < md { break; }
        rem -= md; month += 1;
    }
    let day = rem as u32 + 1;
    let yday = {
        let mdays2 = [31u32, if is_leap(year) { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
        mdays2[..(month as usize -1)].iter().sum::<u32>() + day
    };
    (year, month, day, hour, min, sec, weekday, yday)
}

fn is_leap(y: u32) -> bool { y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) }

fn format_date(fmt: &str, secs: u64) -> String {
    let (y, mo, d, h, mi, s, wd, yd) = unix_to_datetime(secs);
    let days = ["Sunday","Monday","Tuesday","Wednesday","Thursday","Friday","Saturday"];
    let months = ["January","February","March","April","May","June","July","August","September","October","November","December"];
    let mut result = String::new();
    let mut chars = fmt.chars();
    while let Some(c) = chars.next() {
        if c == '%' {
            match chars.next() {
                Some('Y') => result.push_str(&format!("{y:04}")),
                Some('y') => result.push_str(&format!("{:02}", y % 100)),
                Some('m') => result.push_str(&format!("{mo:02}")),
                Some('d') => result.push_str(&format!("{d:02}")),
                Some('e') => result.push_str(&format!("{d:2}")),
                Some('H') => result.push_str(&format!("{h:02}")),
                Some('M') => result.push_str(&format!("{mi:02}")),
                Some('S') => result.push_str(&format!("{s:02}")),
                Some('A') => result.push_str(days.get(wd as usize).unwrap_or(&"?")),
                Some('a') => result.push_str(&days.get(wd as usize).unwrap_or(&"?")[..3]),
                Some('B') => result.push_str(months.get(mo as usize - 1).unwrap_or(&"?")),
                Some('b') | Some('h') => result.push_str(&months.get(mo as usize - 1).unwrap_or(&"?")[..3]),
                Some('j') => result.push_str(&format!("{yd:03}")),
                Some('u') => result.push_str(&format!("{}", if wd == 0 { 7 } else { wd })),
                Some('w') => result.push_str(&format!("{wd}")),
                Some('Z') => result.push_str("UTC"),
                Some('I') => result.push_str(&format!("{:02}", if h % 12 == 0 { 12 } else { h % 12 })),
                Some('p') => result.push_str(if h < 12 { "AM" } else { "PM" }),
                Some('T') | Some('X') => result.push_str(&format!("{h:02}:{mi:02}:{s:02}")),
                Some('x') => result.push_str(&format!("{mo:02}/{d:02}/{:02}", y % 100)),
                Some('n') => result.push('\n'),
                Some('t') => result.push('\t'),
                Some('%') => result.push('%'),
                Some(x) => { result.push('%'); result.push(x); }
                None => result.push('%'),
            }
        } else { result.push(c); }
    }
    result
}
