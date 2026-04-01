use crate::{ast::*, env::ShellEnv, jobs::JobManager};

fn expand_aliases(env: &ShellEnv, pl: &mut Pipeline) {
    // cap at 8 passes to avoid infinite alias loops
    const LIMIT: usize = 8;

    for cmd in &mut pl.cmds {
        for _ in 0..LIMIT {
            let Some(head) = cmd.argv.first().cloned() else { break; };
            let Some(a) = env.get_alias(&head).map(|s| s.to_string()) else { break; };

            // expand alias value into words, then tack on the remaining args
            let mut expanded = match shell_words::split(&a) {
                Ok(v) => v,
                Err(_) => break,
            };
            expanded.extend(cmd.argv.iter().skip(1).cloned());
            cmd.argv = expanded;
        }
    }
}

#[cfg(unix)]
mod imp {
    use super::*;
    use crate::builtins::{try_run_builtin, BuiltinResult};
    use anyhow::{bail, Context};
    use nix::{
        fcntl::{fcntl, open, FcntlArg, FdFlag, OFlag},
        libc,
        sys::{
            signal::{signal, SigHandler, Signal},
            stat::Mode,
            wait::{waitpid, WaitStatus},
        },
        unistd::{dup2, execvp, fork, getpgrp, pipe2, setpgid, tcsetpgrp, ForkResult, Pid},
    };
    use std::os::fd::BorrowedFd;
    use std::ffi::CString;
    use std::os::fd::{AsRawFd, OwnedFd};

    pub struct Shell {
        pub env: ShellEnv,
        pub jobs: JobManager,
    }

    impl Shell {
        pub fn new() -> anyhow::Result<Self> {
            let mut jobs = JobManager::new();
            jobs.init_tty();
            Ok(Self { env: ShellEnv::new()?, jobs })
        }

        pub fn run_pipeline(&mut self, mut pl: Pipeline, cmdline: String) -> anyhow::Result<i32> {
            // builtins only for a single foreground command — pipes and bg go through fork
            if pl.cmds.len() == 1 && !pl.background {
                let argv = &pl.cmds[0].argv;
                if let Some(r) = try_run_builtin(&mut self.env, &mut self.jobs, argv)? {
                    return Ok(match r {
                        BuiltinResult::Continue => 0,
                        BuiltinResult::Exit(code) => code,
                    });
                }
            }

            expand_aliases(&self.env, &mut pl);

            let mut pids = Vec::new();
            let mut pgid: Option<Pid> = None;
            let mut prev_read: Option<OwnedFd> = None;

            for (idx, cmd) in pl.cmds.iter().enumerate() {
                let is_last = idx == pl.cmds.len() - 1;
                let (r, w): (Option<OwnedFd>, Option<OwnedFd>) = if !is_last {
                    let (r, w) = pipe2(OFlag::O_CLOEXEC)?;
                    (Some(r), Some(w))
                } else {
                    (None, None)
                };

                match unsafe { fork()? } {
                    ForkResult::Child => {
                        // put the child in its own process group right away
                        let mypid = nix::unistd::getpid();
                        let target_pgid = pgid.unwrap_or(mypid);
                        let _ = setpgid(mypid, target_pgid);

                        // wire up stdin/stdout to the pipe ends
                        if let Some(fd) = prev_read.as_ref() {
                            dup2(fd.as_raw_fd(), libc::STDIN_FILENO)?;
                        }
                        if let Some(fd) = w.as_ref() {
                            dup2(fd.as_raw_fd(), libc::STDOUT_FILENO)?;
                        }

                        // OwnedFd drops here, closing the fd before exec
                        apply_redirects(cmd)?;

                        // set CLOEXEC on all fds > 2 so rustyline sockets and pipe
                        // ends are not inherited by the child after execvp
                        cloexec_extra_fds();

                        // replace the process image
                        if cmd.argv.is_empty() {
                            bail!("empty command");
                        }
                        let c_argv: Vec<CString> = cmd
                            .argv
                            .iter()
                            .map(|s| CString::new(s.as_str()).context("NUL dans argv"))
                            .collect::<anyhow::Result<_>>()?;
                        execvp(&c_argv[0], &c_argv)?;
                        unreachable!("execvp failed");
                    }
                    ForkResult::Parent { child } => {
                        // parent also sets pgid to avoid a race with the child
                        if pgid.is_none() {
                            pgid = Some(child);
                        }
                        let _ = setpgid(child, pgid.unwrap());

                        prev_read = r;
                        // w drops here — write end of pipe is gone from the parent
                        pids.push(child);
                    }
                }
            }

            if pl.background {
                let id = self.jobs.add(pgid.context("pgid absent")?, pids.clone(), cmdline);
                println!("[{id}] {}", pgid.unwrap());
                Ok(0)
            } else {
                // give the terminal to the child's process group so Ctrl+C targets
                // the child and not the shell
                let shell_pgid = getpgrp();
                if let Some(pg) = pgid {
                    let tty = unsafe { BorrowedFd::borrow_raw(libc::STDIN_FILENO) };
                    let _ = tcsetpgrp(tty, pg);
                }

                // ignore SIGINT / SIGTSTP in the shell while the child is running
                let old_sigint = unsafe { signal(Signal::SIGINT, SigHandler::SigIgn) };
                let old_sigtstp = unsafe { signal(Signal::SIGTSTP, SigHandler::SigIgn) };

                // wait on all pids, only the last exit code matters
                let mut last_code = 0;
                for pid in pids {
                    match waitpid(pid, None)? {
                        WaitStatus::Exited(_, code) => last_code = code,
                        WaitStatus::Signaled(_, sig, _) => last_code = 128 + sig as i32,
                        _ => {}
                    }
                }

                // restore terminal control to the shell
                let tty = unsafe { BorrowedFd::borrow_raw(libc::STDIN_FILENO) };
                let _ = tcsetpgrp(tty, shell_pgid);

                // restore signal handlers
                if let Ok(old) = old_sigint {
                    unsafe { let _ = signal(Signal::SIGINT, old); }
                }
                if let Ok(old) = old_sigtstp {
                    unsafe { let _ = signal(Signal::SIGTSTP, old); }
                }

                Ok(last_code)
            }
        }
    }

    /// Set O_CLOEXEC on every fd > 2 in the child process so that rustyline
    /// sockets and pipe ends are closed automatically after execvp.
    fn cloexec_extra_fds() {
        let fds: Vec<i32> = match std::fs::read_dir("/proc/self/fd") {
            Ok(entries) => entries
                .filter_map(|e| e.ok())
                .filter_map(|e| e.file_name().to_str().and_then(|s| s.parse::<i32>().ok()))
                .filter(|&fd| fd > 2)
                .collect(),
            Err(_) => return,
        };
        for fd in fds {
            let _ = fcntl(fd, FcntlArg::F_SETFD(FdFlag::FD_CLOEXEC));
        }
    }

    fn apply_redirects(cmd: &Command) -> anyhow::Result<()> {
        use anyhow::bail;
        use std::os::fd::AsRawFd;

        for r in &cmd.redirects {
            let path = match &r.target {
                RedirectTarget::Path(p) => p.as_str(),
            };
            match (r.fd, r.mode) {
                (Fd::Stdin, RedirectMode::Read) => {
                    let fd = open(path, OFlag::O_RDONLY, Mode::empty())?;
                    dup2(fd.as_raw_fd(), libc::STDIN_FILENO)?;
                }
                (Fd::Stdout, RedirectMode::WriteTrunc) => {
                    let fd = open(
                        path,
                        OFlag::O_CREAT | OFlag::O_WRONLY | OFlag::O_TRUNC,
                        Mode::from_bits_truncate(0o644),
                    )?;
                    dup2(fd.as_raw_fd(), libc::STDOUT_FILENO)?;
                }
                (Fd::Stdout, RedirectMode::WriteAppend) => {
                    let fd = open(
                        path,
                        OFlag::O_CREAT | OFlag::O_WRONLY | OFlag::O_APPEND,
                        Mode::from_bits_truncate(0o644),
                    )?;
                    dup2(fd.as_raw_fd(), libc::STDOUT_FILENO)?;
                }
                (Fd::Stderr, RedirectMode::WriteTrunc) => {
                    let fd = open(
                        path,
                        OFlag::O_CREAT | OFlag::O_WRONLY | OFlag::O_TRUNC,
                        Mode::from_bits_truncate(0o644),
                    )?;
                    dup2(fd.as_raw_fd(), libc::STDERR_FILENO)?;
                }
                // other fd combos (2>&1 etc.) not worth implementing right now
                _ => bail!("unsupported redirect"),
            }
        }
        Ok(())
    }
}

#[cfg(unix)]
pub use imp::Shell;

#[cfg(not(unix))]
pub struct Shell {
    pub env: ShellEnv,
    pub jobs: JobManager,
}

#[cfg(not(unix))]
impl Shell {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self { env: ShellEnv::new()?, jobs: JobManager::new() })
    }

    pub fn run_pipeline(&mut self, mut pl: Pipeline, cmdline: String) -> anyhow::Result<i32> {
        use crate::builtins::{try_run_builtin, BuiltinResult};
        use anyhow::{bail, Context};
        use std::{
            fs::OpenOptions,
            process::{Command, Stdio},
        };

        // builtins only for single foreground commands
        if pl.cmds.len() == 1 && !pl.background {
            let argv = &pl.cmds[0].argv;
            if let Some(r) = try_run_builtin(&mut self.env, &mut self.jobs, argv)? {
                return Ok(match r {
                    BuiltinResult::Continue => 0,
                    BuiltinResult::Exit(code) => code,
                });
            }
        }

        expand_aliases(&self.env, &mut pl);

        let mut children: Vec<std::process::Child> = Vec::new();
        let mut prev_stdout: Option<std::process::ChildStdout> = None;

        for (idx, cmd) in pl.cmds.iter().enumerate() {
            let is_last = idx == pl.cmds.len() - 1;
            if cmd.argv.is_empty() {
                bail!("empty command");
            }

            let mut c = Command::new(&cmd.argv[0]);
            c.args(&cmd.argv[1..]);
            c.current_dir(&self.env.cwd);
            c.envs(self.env.vars.iter().map(|(k, v)| (k, v)));

            if let Some(out) = prev_stdout.take() {
                c.stdin(Stdio::from(out));
            } else {
                c.stdin(Stdio::inherit());
            }

            c.stderr(Stdio::inherit());
            if !is_last {
                c.stdout(Stdio::piped());
            } else {
                c.stdout(Stdio::inherit());
            }

            for r in &cmd.redirects {
                let path = match &r.target {
                    RedirectTarget::Path(p) => p,
                };

                match (r.fd, r.mode) {
                    (Fd::Stdin, RedirectMode::Read) => {
                        let f = OpenOptions::new()
                            .read(true)
                            .open(path)
                            .with_context(|| format!("open < {path}"))?;
                        c.stdin(Stdio::from(f));
                    }
                    (Fd::Stdout, RedirectMode::WriteTrunc) => {
                        let f = OpenOptions::new()
                            .create(true)
                            .write(true)
                            .truncate(true)
                            .open(path)
                            .with_context(|| format!("open > {path}"))?;
                        c.stdout(Stdio::from(f));
                    }
                    (Fd::Stdout, RedirectMode::WriteAppend) => {
                        let f = OpenOptions::new()
                            .create(true)
                            .write(true)
                            .append(true)
                            .open(path)
                            .with_context(|| format!("open >> {path}"))?;
                        c.stdout(Stdio::from(f));
                    }
                    (Fd::Stderr, RedirectMode::WriteTrunc) => {
                        let f = OpenOptions::new()
                            .create(true)
                            .write(true)
                            .truncate(true)
                            .open(path)
                            .with_context(|| format!("open 2> {path}"))?;
                        c.stderr(Stdio::from(f));
                    }
                    _ => bail!("unsupported redirect"),
                }
            }

            let mut child = c.spawn().with_context(|| format!("spawn {}", cmd.argv[0]))?;

            if !is_last {
                prev_stdout = child.stdout.take();
            }

            children.push(child);
        }

        if pl.background {
            let id = self.jobs.add(children, cmdline);
            println!("[{id}] Running");
            Ok(0)
        } else {
            let mut last_code = 0;
            for (i, mut ch) in children.into_iter().enumerate() {
                let st = ch.wait()?;
                if i == pl.cmds.len().saturating_sub(1) {
                    last_code = st.code().unwrap_or(1);
                }
            }
            Ok(last_code)
        }
    }

    pub fn run_line(&mut self, line: &str) -> anyhow::Result<i32> {
        run_line_impl(self, line)
    }
}

/// Shared run_line logic — platform-specific dispatch happens below.
fn run_line_impl(shell: &mut Shell, line: &str) -> anyhow::Result<i32> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return Ok(0);
    }

    let (background, core_line) = {
        let t = line.trim_end();
        if t.ends_with('&') && !t.ends_with("&&") {
            (true, t.trim_end_matches('&').trim().to_string())
        } else {
            (false, t.to_string())
        }
    };

    #[cfg(windows)]
    if should_delegate_to_cmd(&core_line) {
        return shell.run_via_cmd_exe(&core_line, background); // let cmd.exe handle it
    }

    #[cfg(unix)]
    if should_delegate_to_posix_shell(&core_line) {
        return shell.run_via_bash_or_sh(&core_line, background); // bash handles what we don't
    }

    match crate::parse_line(&core_line) {
        Ok(Some(pl)) => {
            let mut pl = pl;
            pl.background = background;

            match shell.run_pipeline(pl, core_line.clone()) {
                Ok(code) => Ok(code),

                #[cfg(windows)]
                Err(e) => {
                    if looks_like_not_found(&e) {
                        return shell.run_via_cmd_exe(&core_line, background);
                    }
                    Err(e)
                }

                #[cfg(unix)]
                Err(e) => {
                    if looks_like_not_found(&e) {
                        return shell.run_via_bash_or_sh(&core_line, background);
                    }
                    Err(e)
                }

                #[cfg(not(any(windows, unix)))]
                Err(e) => Err(e),
            }
        }

        Ok(None) => Ok(0),

        Err(_) => {
            #[cfg(windows)]
            return shell.run_via_cmd_exe(&core_line, background);

            #[cfg(unix)]
            return shell.run_via_bash_or_sh(&core_line, background);

            #[cfg(not(any(windows, unix)))]
            anyhow::bail!("Orbis: unsupported platform");
        }
    }
}

#[cfg(windows)]
fn should_delegate_to_cmd(line: &str) -> bool {
    // anything with &&, ||, or parens is beyond what the MVP parser handles
    if line.contains("&&") || line.contains("||") || line.contains('(') || line.contains(')') {
        return true;
    }
    // & in cmd is a command separator — if it's not at the end (background), let cmd handle it
    if let Some(pos) = line.find('&') {
        let last = line.trim_end().len().saturating_sub(1);
        if pos != last {
            return true;
        }
    }

    // cmd-internal commands that don't exist as standalone executables
    let head = line.split_whitespace().next().unwrap_or("").trim_matches('"').to_ascii_lowercase();
    matches!(
        head.as_str(),
        "dir" | "copy" | "del" | "erase" | "move" | "type" | "ren" | "rename" |
        "md" | "mkdir" | "rd" | "rmdir" | "cls" | "set" | "call" | "start" |
        "title" | "ver" | "vol" | "path" | "assoc" | "ftype" | "pushd" | "popd"
    )
}

#[cfg(unix)]
fn should_delegate_to_posix_shell(line: &str) -> bool {
    // anything the MVP parser can't handle gets delegated to bash
    line.contains("&&")
        || line.contains("||")
        || line.contains(';')
        || line.contains("$(")
        || line.contains('`')
        || line.contains("{")
        || line.contains("}")
}

#[cfg(any(windows, unix))]
fn looks_like_not_found(e: &anyhow::Error) -> bool {
    if let Some(ioe) = e.downcast_ref::<std::io::Error>() {
        return ioe.kind() == std::io::ErrorKind::NotFound;
    }
    let msg = e.to_string().to_ascii_lowercase();
    msg.contains("not found") || msg.contains("no such file")
}

#[cfg(windows)]
impl Shell {
    fn run_via_cmd_exe(&mut self, line: &str, background: bool) -> anyhow::Result<i32> {
        use std::process::Command;

        let mut child = Command::new("cmd.exe")
            .args(["/C", line])
            .current_dir(&self.env.cwd)
            .envs(self.env.vars.iter().map(|(k, v)| (k, v)))
            .spawn()?;

        if background {
            let _id = self.jobs.add(vec![child], line.to_string());
            return Ok(0);
        }

        let st = child.wait()?;
        Ok(st.code().unwrap_or(1))
    }
}

#[cfg(unix)]
impl Shell {
    pub fn run_line(&mut self, line: &str) -> anyhow::Result<i32> {
        run_line_impl(self, line)
    }

    fn run_via_bash_or_sh(&mut self, line: &str, background: bool) -> anyhow::Result<i32> {
        use nix::unistd::Pid;
        use std::process::Command;

        let mut cmd = Command::new("bash");
        cmd.args(["-c", line])
            .current_dir(&self.env.cwd)
            .envs(self.env.vars.iter().map(|(k, v)| (k, v)));

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(_) => {
                let mut sh = Command::new("sh");
                sh.args(["-c", line])
                    .current_dir(&self.env.cwd)
                    .envs(self.env.vars.iter().map(|(k, v)| (k, v)));
                sh.spawn()?
            }
        };

        if background {
            let pid = Pid::from_raw(child.id() as i32);
            let _id = self.jobs.add(pid, vec![pid], line.to_string());
            return Ok(0);
        }

        let st = child.wait()?;
        Ok(st.code().unwrap_or(1))
    }
}
