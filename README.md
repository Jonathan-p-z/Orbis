# Orbis

![Rust](https://img.shields.io/badge/rust-1.85.0-orange?style=flat-square&logo=rust)
![License](https://img.shields.io/badge/license-MIT-blue?style=flat-square)
![Platform](https://img.shields.io/badge/platform-Linux%20%7C%20WSL%20%7C%20Windows-informational?style=flat-square)

A shell written in Rust. The starting point was understanding how a shell actually works — fork, exec, pipes, job control, all of it. The scope grew from there, so there is now a CLI and a Unix utilities sandbox.

The scope is intentionally limited to an MVP. Anything the parser cannot handle (`&&`, `||`, subshells...) is delegated to bash rather than crashing.

---

## Structure

The project is a Rust workspace with 3 crates:

```
crates/
├── shell-core/    core library: parser, AST, execution, jobs, env
├── shell-cli/     interactive REPL + script execution
└── orbisbox/      reimplementation of ~40 Unix utilities
```

---

## Requirements

**Rust 1.85.0** (see `rust-toolchain.toml`)

---

## Installation

```bash
./scripts/install.sh
```

Works on Linux, WSL and Git Bash (Windows). The script installs `orbis` and `orbisbox` via `cargo install` and configures PATH if needed.

Options: `--force` to reinstall, `--uninstall` to remove everything, `--no-path` to leave the shell profile untouched.

---

## Usage

### Interactive REPL

```bash
orbis
```

The prompt shows the last exit code when it is non-zero. Tab completion on commands and paths, history stored in `~/.local/share/orbis/history`.

### Run a script

```bash
orbis my_script.orbis
```

Scripts are executed line by line, no extended bash syntax.

### orbisbox

```bash
orbisbox ls /some/path
orbisbox grep "pattern" file.txt
orbisbox sort -r file.txt
```

Available utilities: `ls`, `cat`, `cp`, `mv`, `rm`, `mkdir`, `rmdir`, `touch`, `ln`, `chmod`, `stat`, `grep`, `sort`, `uniq`, `cut`, `tr`, `head`, `tail`, `wc`, `echo`, `pwd`, `whoami`, `uname`, `date`, `sleep`, `yes`, `env`, `id`, `ps`, `kill`, `basename`, `dirname`, `realpath`, and a few more.

---

## Demo

```
orbis:~$  ls src/ | grep ".rs" | sort     ← known command, cyan prompt
ast.rs
builtins.rs
env.rs
exec.rs
jobs.rs
lib.rs
parser.rs

orbis:~/src$ cat nonexistent.txt
orbis: No such file or directory

orbis:~/src [1]$                           ← non-zero exit code shown in prompt

orbis:~/src [1]$ cd ../scr<TAB>            ← tab completion
scripts/

orbis:~$ jobs
[1] Running   sleep 60 &
```

---

## Features

### Natively supported syntax

```bash
# Pipelines
ls -la | grep ".rs" | sort

# Redirections
command > out.txt
command >> out.txt
command < input.txt
command 2> err.txt

# Background
long_command &

# Job control
jobs
fg %1
bg %1
```

### Builtins

`cd`, `cs` (cd + ls), `pwd`, `export`, `unset`, `env`, `alias`, `unalias`, `which`, `type`, `echo`, `clear`, `exit`, `true`, `false`, `jobs`, `fg`, `bg`, `help`

### What is delegated to bash

Anything the minimal parser intentionally does not cover: `&&`, `||`, `;`, `$()`, backticks, globbing (`*`, `?`), `2>&1`... This avoids crashing on unknown syntax, but it is not magic — if bash is not available, it will fail.

---

## Internals

### Parser

The parser tokenises with `shell-words` (for quote handling), identifies operators (`|`, `>`, `>>`, `<`, `2>`, `&`), and builds a minimal AST:

```
Pipeline → [Command, Command, ...]
Command  → argv + [Redirect, ...]
Redirect → (fd, mode, path)
```

### Execution (Unix)

For each pipeline, the shell forks child processes using `nix`. Pipes are pairs of file descriptors redistributed with `dup2` before calling `execvp`. Each pipeline runs in its own process group for job control. `tcsetpgrp` hands the terminal to the foreground process and reclaims it on return.

### Alias expansion

Aliases are expanded as a pre-processing step before parsing, capped at 8 passes to prevent infinite loops.

### REPL (rustyline)

The interactive helper implements rustyline's `Helper` trait directly. Completion covers builtins, orbisbox commands, and all executables found in PATH. Path completion supports `~` expansion and appends `/` to directories. History hints are shown in grey. Syntax highlighting colours the command name cyan if recognised, yellow if unknown.

---

## Known limitations

- Variable expansion is context-limited — `$HOME` works in `cd` and the prompt, but general `$VAR` substitution is not implemented
- No native globbing
- No `2>&1` (fd-to-fd redirection)
- Job control is functional but basic
- Single-threaded / blocking execution

---

## License

MIT — see `LICENSE`.
