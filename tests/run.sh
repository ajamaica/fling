#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cargo build --manifest-path "$ROOT/Cargo.toml"
exec python3 -m unittest discover -s "$(dirname "$0")" -p 'test_*.py' -v
