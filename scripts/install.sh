#!/usr/bin/env bash
# ─────────────────────────────────────────────────────────────────
# Orbis Shell — Installation (Linux / WSL / Git Bash)
# Usage: ./scripts/install.sh [--uninstall] [--no-path] [--force]
# ─────────────────────────────────────────────────────────────────
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"

UNINSTALL=false
NO_PATH=false
FORCE=false

for arg in "$@"; do
  case "$arg" in
    --uninstall) UNINSTALL=true ;;
    --no-path)   NO_PATH=true ;;
    --force)     FORCE=true ;;
    --help|-h)
      echo "Usage: $0 [--uninstall] [--no-path] [--force]"
      exit 0
      ;;
    *)
      echo "Option inconnue: $arg  (--help pour l'aide)" >&2
      exit 1
      ;;
  esac
done

# ── Détection de l'OS ────────────────────────────────────────────
case "${OSTYPE:-}" in
  msys*|cygwin*|mingw*) OS=windows ;;
  *)
    if grep -qi microsoft /proc/version 2>/dev/null; then
      OS=wsl
    else
      OS=linux
    fi
    ;;
esac

CARGO_BIN="${HOME}/.cargo/bin"
ORBIS_BINS=(orbis orbisbox)

# ── Désinstallation ──────────────────────────────────────────────
if $UNINSTALL; then
  echo "Désinstallation d'Orbis..."
  removed=0
  for bin in "${ORBIS_BINS[@]}"; do
    if [ -f "${CARGO_BIN}/${bin}" ] || [ -f "${CARGO_BIN}/${bin}.exe" ]; then
      rm -f "${CARGO_BIN}/${bin}" "${CARGO_BIN}/${bin}.exe"
      echo "  Supprimé: ${bin}"
      removed=$((removed + 1))
    fi
  done
  [ "$removed" -eq 0 ] && echo "  Aucun binaire Orbis trouvé." || echo "OK: ${removed} binaire(s) supprimé(s)."
  exit 0
fi

# ── Prérequis ────────────────────────────────────────────────────
if ! command -v cargo >/dev/null 2>&1; then
  echo "Erreur: cargo introuvable." >&2
  echo "  Installe Rust via: https://rustup.rs/" >&2
  exit 1
fi

FORCE_FLAG=""
$FORCE && FORCE_FLAG="--force"

echo "Orbis — installation depuis ${ROOT_DIR}  (${OS})"
echo ""

cd "$ROOT_DIR"

echo "→ orbis..."
cargo install --path crates/shell-cli --bin orbis $FORCE_FLAG --quiet
echo "  OK"

echo "→ orbisbox..."
cargo install --path crates/orbisbox --bin orbisbox $FORCE_FLAG --quiet
echo "  OK"

# ── PATH ─────────────────────────────────────────────────────────
if ! $NO_PATH; then
  PATH_LINE='export PATH="$HOME/.cargo/bin:$PATH"'

  if [ "$OS" = "windows" ]; then
    # Sur Git Bash, cargo est généralement déjà dans le PATH via rustup
    echo "→ Sur Windows, relance ton terminal pour que le PATH soit à jour."
  else
    PROFILE="${HOME}/.profile"
    case "${SHELL:-}" in
      */bash) PROFILE="${HOME}/.bashrc" ;;
      */zsh)  PROFILE="${HOME}/.zshrc" ;;
      */fish)
        PROFILE="${HOME}/.config/fish/config.fish"
        PATH_LINE='set -gx PATH "$HOME/.cargo/bin" $PATH'
        ;;
    esac

    mkdir -p "$(dirname "$PROFILE")"
    touch "$PROFILE"

    if ! grep -Fq ".cargo/bin" "$PROFILE" 2>/dev/null; then
      printf "\n# Orbis Shell\n%s\n" "$PATH_LINE" >> "$PROFILE"
      echo "→ PATH configuré dans: $PROFILE"
      echo "  Recharge: source \"$PROFILE\"  (ou nouvelle session)"
    else
      echo "→ PATH déjà configuré dans: $PROFILE"
    fi
  fi
fi

echo ""
echo "Installation terminée!"
echo ""
echo "Lancer:   orbis"
echo "Aide:     orbis --help"
echo "Utils:    orbisbox"
