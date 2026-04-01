# Orbis

Un shell écrit en Rust. L'idée de départ c'était de comprendre comment fonctionne vraiment un shell — fork, exec, pipes, job control, tout ça. Ça a fini par grossir un peu, donc il y a maintenant un CLI et un bac à sable d'utilitaires Unix.

Le périmètre est volontairement limité à un MVP. Tout ce que le parser ne sait pas gérer (`&&`, `||`, subshells...) est délégué à bash plutôt que de planter.

---

## Structure

Le projet est un workspace Rust avec 3 crates :

```
crates/
├── shell-core/    bibliothèque centrale : parser, AST, exécution, jobs, env
├── shell-cli/     REPL interactif + exécution de scripts
└── orbisbox/      réimplémentation de ~40 utilitaires Unix
```

---

## Prérequis

**Rust 1.85.0** (voir `rust-toolchain.toml`)

---

## Installation

```bash
./scripts/install.sh
```

Fonctionne sur Linux, WSL et Git Bash (Windows). Le script installe `orbis` et `orbisbox` via `cargo install` et configure le PATH si nécessaire.

Options : `--force` pour réinstaller, `--uninstall` pour tout supprimer, `--no-path` pour ne pas toucher au profil shell.

---

## Utilisation

### REPL interactif

```bash
orbis
```

Le prompt affiche le code de retour de la dernière commande si il est non-nul. Complétion sur les commandes et les chemins, historique dans `~/.local/share/orbis/history`.

### Exécuter un script

```bash
orbis mon_script.orbis
```

Les scripts sont exécutés ligne par ligne, pas de syntaxe bash étendue.

### orbisbox

```bash
orbisbox ls /some/path
orbisbox grep "pattern" file.txt
orbisbox sort -r file.txt
```

Les utilitaires disponibles : `ls`, `cat`, `cp`, `mv`, `rm`, `mkdir`, `rmdir`, `touch`, `ln`, `chmod`, `stat`, `grep`, `sort`, `uniq`, `cut`, `tr`, `head`, `tail`, `wc`, `echo`, `pwd`, `whoami`, `uname`, `date`, `sleep`, `yes`, `env`, `id`, `ps`, `kill`, `basename`, `dirname`, `realpath`, et quelques autres.

---

## Fonctionnalités

### Syntaxe supportée nativement

```bash
# Pipelines
ls -la | grep ".rs" | sort

# Redirections
command > out.txt
command >> out.txt
command < input.txt
command 2> err.txt

# Arrière-plan
long_command &

# Job control
jobs
fg %1
bg %1
```

### Builtins

`cd`, `cs` (cd + ls), `pwd`, `export`, `unset`, `env`, `alias`, `unalias`, `which`, `type`, `echo`, `clear`, `exit`, `true`, `false`, `jobs`, `fg`, `bg`, `help`

### Ce qui est délégué à bash

Tout ce que le parser minimal ne couvre pas intentionnellement : `&&`, `||`, `;`, `$()`, backticks, globbing (`*`, `?`), `2>&1`... Ça évite de planter bêtement, mais c'est pas magique non plus — si bash n'est pas dispo, ça échouera.

---

## Fonctionnement interne (pour ceux que ça intéresse)

### Parser

Le parser tokenise avec `shell-words` (pour la gestion des quotes), repère les opérateurs (`|`, `>`, `>>`, `<`, `2>`, `&`), et construit un AST minimal :

```
Pipeline → [Command, Command, ...]
Command  → argv + [Redirect, ...]
Redirect → (fd, mode, chemin)
```

### Exécution (Unix)

Pour chaque pipeline, le shell fork des processus fils avec `nix`. Les pipes sont des paires de file descriptors qu'on redistribue avec `dup2` avant d'appeler `execvp`. Chaque pipeline tourne dans son propre process group pour le job control. `tcsetpgrp` permet de donner/reprendre le terminal au foreground.

### Alias

Expansion en pré-traitement, limitée à 8 passes pour éviter les boucles infinies.

---

## Limitations connues

- Pas de substitution de variables (`$VAR` n'est pas développé, sauf `$HOME` dans certains contextes)
- Pas de globbing natif
- Pas de `2>&1` (redirection de descripteurs entre eux)
- Job control fonctionnel mais basique
- Exécution single-threaded / bloquante

---

## Licence

MIT — voir `LICENSE`.
