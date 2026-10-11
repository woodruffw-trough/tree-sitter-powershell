#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

cargo fmt --all --check
cargo clippy --locked --all-targets -- -D warnings
RUSTDOCFLAGS='-D warnings' cargo doc --locked --no-deps
# Generated C is verified by scripts/check-generated.sh. Check all tracked
# handwritten C sources and headers so future scanner helpers follow the style.
mapfile -d '' -t handwritten_c < <(
  git ls-files -z -- '*.c' '*.h' \
    ':!:src/parser.c' ':!:src/tree_sitter/**' ':!:src/variable_chars.h'
)
if ((${#handwritten_c[@]})); then
  clang-format --dry-run --Werror "${handwritten_c[@]}"
fi
cc -fsyntax-only -std=c11 -Wall -Wextra -Werror -I src src/scanner.c
shellcheck scripts/*.sh
ruff format --check scripts
ruff check scripts
zizmor --collect all --strict-collection --pedantic --no-ignores .github
