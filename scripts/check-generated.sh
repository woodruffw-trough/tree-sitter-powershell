#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

python3 - <<'PY'
import json
from pathlib import Path

metadata = json.loads(Path("tree-sitter.json").read_text(encoding="utf-8"))
enabled = {name for name, value in metadata["bindings"].items() if value}
if enabled != {"rust"}:
    raise SystemExit(f"Only Rust bindings may be enabled; found {sorted(enabled)}")
bindings = {path.name for path in Path("bindings").iterdir()}
if bindings != {"rust"}:
    raise SystemExit(f"Only bindings/rust may exist; found {sorted(bindings)}")
PY

generated=$(mktemp -d "${TMPDIR:-/tmp}/powershell-generated.XXXXXX")
trap 'rm -rf -- "$generated"' EXIT

# The runtime flag is explicit even if the caller overrides the environment.
tree-sitter generate --js-runtime native --output "$generated/src"

for file in parser.c grammar.json node-types.json \
  tree_sitter/alloc.h tree_sitter/array.h tree_sitter/parser.h; do
  diff -u -- "src/$file" "$generated/src/$file"
done

# Match the pinned .NET tokenizer's BMP character classification.
"${PWSH_BIN:-pwsh}" -NoLogo -NoProfile -NonInteractive \
  -File scripts/generate-unicode.ps1 > "$generated/variable_chars.h"
diff -u -- src/variable_chars.h "$generated/variable_chars.h"
"${PWSH_BIN:-pwsh}" -NoLogo -NoProfile -NonInteractive \
  -File scripts/generate-unicode.ps1 -JavaScript > "$generated/identifier_chars.js"
diff -u -- src/identifier_chars.js "$generated/identifier_chars.js"

# Integration sources stay identical to the attributed upstream release.
sha256sum --check --strict tests/upstream/SHA256SUMS
