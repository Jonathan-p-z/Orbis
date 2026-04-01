//! Background job management for Orbis.
//!
//! Unix: process groups (pgid) + POSIX signals.
//! Non-Unix: basic tracking via std::process::Child handles.

#[cfg(unix)]
mod imp {
    use anyhow::Context;
    use nix::{
        libc,
        sys::{
            signal::{killpg, Signal},
            wait::{waitpid, WaitPidFlag, WaitStatus},
        },
        unistd::{tcsetpgrp, Pid},
    };
    use std::collections::BTreeMap;

    #[derive(Debug, Clone)]
    pub struct JobInfo {
        pub id: u32,
        pub pgid: Pid,
        pub pids: Vec<Pid>,
        pub cmdline: String,
        pub status: String,
    }

    #[derive(Debug, Default)]
    pub struct JobManager {
        next_id: u32,
        jobs: BTreeMap<u32, JobInfo>,
        shell_pgid: Option<Pid>,
        shell_tty: Option<i32>,
    }

    impl JobManager {
        pub fn new() -> Self {
            Self::default()
        }

        pub fn init_tty(&mut self) {
            self.shell_tty = Some(libc::STDIN_FILENO);
            self.shell_pgid = Some(nix::unistd::getpgrp());
        }

        pub fn add(&mut self, pgid: Pid, pids: Vec<Pid>, cmdline: String) -> u32 {
            self.next_id += 1;
            let id = self.next_id;
            self.jobs.insert(id, JobInfo { id, pgid, pids, cmdline, status: "Running".into() });
            id
        }

        pub fn list(&self) -> Vec<JobInfo> {
            self.jobs.values().cloned().collect()
        }

        fn parse_spec(spec: &str) -> anyhow::Result<u32> {
            let s = spec.trim().strip_prefix('%').unwrap_or(spec.trim());
            s.parse::<u32>().context("invalid job spec, use %N")
        }

        fn get_job_mut(&mut self, spec: &str) -> anyhow::Result<&mut JobInfo> {
            let id = Self::parse_spec(spec)?;
            self.jobs.get_mut(&id).context("no such job")
        }

        pub fn reap_nonblocking(&mut self) {
            let mut done = Vec::new();
            for (id, j) in self.jobs.iter_mut() {
                // Check each child pid individually — waitpid(-pgid, WNOHANG) can
                // return ECHILD if setpgid raced with execvp, silently dropping live
                // jobs. Direct pid-based waitpid is reliable because these are our
                // direct children.
                let all_done = !j.pids.is_empty() && j.pids.iter().all(|&pid| {
                    match waitpid(pid, Some(WaitPidFlag::WNOHANG)) {
                        Ok(WaitStatus::Exited(_, _)) | Ok(WaitStatus::Signaled(_, _, _)) => true,
                        Ok(WaitStatus::StillAlive) | Ok(_) => false,
                        // ECHILD = already reaped; other errors = treat as done
                        Err(_) => true,
                    }
                });
                if all_done {
                    j.status = "Done".into();
                    done.push(*id);
                }
            }
            for id in done {
                self.jobs.remove(&id);
            }
        }

        pub fn bg(&mut self, spec: &str) -> anyhow::Result<()> {
            let j = self.get_job_mut(spec)?;
            killpg(j.pgid, Signal::SIGCONT)?;
            j.status = "Running".into();
            Ok(())
        }

        pub fn fg(&mut self, spec: &str) -> anyhow::Result<()> {
            use std::os::fd::BorrowedFd;

            let tty_fd = self.shell_tty.context("tty not initialized")?;
            let shell_pgid = self.shell_pgid.context("shell pgid not set")?;

            let j = self.get_job_mut(spec)?.clone();

            // SAFETY: tty_fd is STDIN_FILENO, valid for the entire shell session
            let tty = unsafe { BorrowedFd::borrow_raw(tty_fd) };
            tcsetpgrp(tty, j.pgid)?;
            killpg(j.pgid, Signal::SIGCONT)?;

            // wait for each child pid individually — more reliable than -pgid
            for &pid in &j.pids {
                loop {
                    match waitpid(pid, None) {
                        Ok(WaitStatus::Exited(_, _)) | Ok(WaitStatus::Signaled(_, _, _)) => break,
                        Ok(_) => continue,
                        Err(_) => break, // ECHILD: already reaped
                    }
                }
            }

            let tty2 = unsafe { BorrowedFd::borrow_raw(tty_fd) };
            tcsetpgrp(tty2, shell_pgid)?;
            self.jobs.retain(|_, x| x.pgid != j.pgid);
            Ok(())
        }
    }
}

#[cfg(unix)]
pub use imp::{JobInfo, JobManager};

#[cfg(not(unix))]
use std::collections::BTreeMap;

#[cfg(not(unix))]
#[derive(Debug, Clone)]
pub struct JobInfo {
    pub id: u32,
    pub cmdline: String,
    pub status: String,
}

#[cfg(not(unix))]
struct Job {
    info: JobInfo,
    children: Vec<std::process::Child>,
}

#[cfg(not(unix))]
#[derive(Debug, Default)]
pub struct JobManager {
    next_id: u32,
    jobs: BTreeMap<u32, Job>,
}

#[cfg(not(unix))]
impl std::fmt::Debug for Job {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Job").field("info", &self.info).finish()
    }
}

#[cfg(not(unix))]
impl JobManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn init_tty(&mut self) {}

    pub fn add(&mut self, children: Vec<std::process::Child>, cmdline: String) -> u32 {
        self.next_id += 1;
        let id = self.next_id;
        let info = JobInfo { id, cmdline, status: "Running".into() };
        self.jobs.insert(id, Job { info, children });
        id
    }

    pub fn list(&self) -> Vec<JobInfo> {
        self.jobs.values().map(|j| j.info.clone()).collect()
    }

    fn parse_spec(spec: &str) -> anyhow::Result<u32> {
        let s = spec.trim().strip_prefix('%').unwrap_or(spec.trim());
        Ok(s.parse::<u32>()?)
    }

    pub fn reap_nonblocking(&mut self) {
        let mut done = Vec::new();
        for (id, job) in self.jobs.iter_mut() {
            let all_exited = job.children.iter_mut().all(|ch| {
                matches!(ch.try_wait(), Ok(Some(_)))
            });
            if all_exited {
                job.info.status = "Done".into();
                done.push(*id);
            }
        }
        for id in done {
            self.jobs.remove(&id);
        }
    }

    pub fn bg(&mut self, _spec: &str) -> anyhow::Result<()> {
        Ok(())
    }

    pub fn fg(&mut self, spec: &str) -> anyhow::Result<()> {
        let id = Self::parse_spec(spec)?;
        let mut job = self
            .jobs
            .remove(&id)
            .ok_or_else(|| anyhow::anyhow!("no such job"))?;
        for ch in job.children.iter_mut() {
            let _ = ch.wait();
        }
        Ok(())
    }
}
