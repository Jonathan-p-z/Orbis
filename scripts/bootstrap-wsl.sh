#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

if ! command -v rustup >/dev/null 2>&1; then
  echo "rustup introuvable. Installe Rust via rustup (recommandé), pas via apt."
  echo "https://rustup.rs/"
  exit 1
fi

rustup toolchain install 1.85.0
rustup override set 1.85.0

cargo -V
rustc -V

cargo build -p orbis-cli --bin orbis
echo "OK: lance avec: cargo run -p orbis-cli --bin orbis"
