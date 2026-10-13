# Contributing

## Setup

Clone [the repository](https://github.com/woodruffw-trough/tree-sitter-powershell)
and run the commands below from its root.

The commands below use Bash. Development uses Rust 1.90.0, tree-sitter CLI 0.27.1,
PowerShell 7.5.4, Python 3, a C compiler, and ShellCheck. `rust-toolchain.toml` selects
the Rust toolchain and its rustfmt and clippy components.

Install the CLI with its embedded QuickJS runtime:

```sh
cargo install tree-sitter-cli --version 0.27.1 --locked --no-default-features --features qjs-rt
export TREE_SITTER_JS_RUNTIME=native
```

Install [PowerShell 7.5.4](https://github.com/PowerShell/PowerShell/releases/tag/v7.5.4).
If `pwsh` is outside `PATH`, set `PWSH_BIN` to its executable:

```sh
export PWSH_BIN=/path/to/pwsh
```

For the same lint tools used by CI on Ubuntu x86-64:

```sh
python3 -m venv work/ci-tools
work/ci-tools/bin/pip install --require-hashes -r scripts/requirements-ci.txt
export PATH="$PWD/work/ci-tools/bin:$PATH"
```

The requirements pin clang-format 20.1.8, Ruff 0.14.14, and zizmor. Other platforms
need compatible installations of those versions.

## Making a grammar change

Edit `grammar.js` and, when token boundaries require it, `src/scanner.c`. Regenerate
the checked-in parser with QuickJS:

```sh
tree-sitter generate --js-runtime native
```

Add a focused case under `test/corpus` or `tests/fixtures`. Check its interpretation
with PowerShell's parser, then assert the relevant nodes, fields, and grouping.
Review tree differences before updating corpus expectations. The
[design document](docs/grammar-sources.md) explains the syntax authorities and AST
comparisons.

Imported real-project fixtures retain their original bytes, license, repository
path, revision, and hash. Their test metadata records provenance alongside the
sources.

## Formatting

Rust uses rustfmt; handwritten C uses `.clang-format`; Python uses `ruff.toml`.
The C configuration uses two-space indentation, a 100-column limit, and braces for
control bodies. The Python configuration also uses a 100-column limit.

```sh
cargo fmt --all
clang-format -i src/scanner.c
ruff format scripts
ruff check --fix scripts
```

Generated parser files, Unicode tables, and imported fixtures retain their generated
or upstream formatting. `scripts/check-generated.sh` verifies those files separately.

## Checks

Run the same checks as CI:

```sh
tree-sitter test --rebuild
cargo test --locked
python3 scripts/generate-fixtures.py
python3 scripts/generate-fixtures.py --variables
scripts/check-generated.sh
scripts/lint.sh
```

The fixture generator checks that its committed cases match the pinned oracle; its
`--write` option updates them after a deliberate change. The generated-file check
recreates the parser and Unicode tables and verifies imported fixture hashes.

Linting checks formatting, clippy and rustdoc warnings, C warnings, shell scripts,
Python, and GitHub Actions security. A local offline security check can use
`ZIZMOR_OFFLINE=true scripts/lint.sh`; CI also checks action vulnerabilities online.
