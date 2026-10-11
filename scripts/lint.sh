#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

cargo fmt --all --check
cargo clippy --locked --all-targets -- -D warnings
RUSTDOCFLAGS='-D warnings' cargo doc --locked --no-deps
cc -fsyntax-only -std=c11 -Wall -Wextra -Werror -I src src/scanner.c
shellcheck scripts/*.sh
python3 - <<'PY'
from pathlib import Path

for source in sorted(Path("scripts").glob("*.py")):
    compile(source.read_bytes(), str(source), "exec")
PY
zizmor --collect all --strict-collection --pedantic --no-ignores .github
