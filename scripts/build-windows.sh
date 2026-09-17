#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
RUSTC_PATH="$(rustup which rustc)"

cd "$ROOT_DIR"
RUSTC="$RUSTC_PATH" rustup run stable cargo build --release --target x86_64-pc-windows-gnu

echo "Windows executable: $ROOT_DIR/target/x86_64-pc-windows-gnu/release/FileManager.exe"
