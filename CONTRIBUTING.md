# Contributing

## Setup

Install Rust through [rustup](https://rustup.rs/), a C compiler, Python 3,
ShellCheck, and [PowerShell 7.5.4](https://github.com/PowerShell/PowerShell/releases/tag/v7.5.4).
The repository's `rust-toolchain.toml` selects the Rust toolchain. Run the following
commands from the repository root in Bash.

Install tree-sitter with QuickJS:

```sh
cargo install tree-sitter-cli --version 0.27.1 --locked --no-default-features --features qjs-rt
export TREE_SITTER_JS_RUNTIME=native
```

If `pwsh` is outside `PATH`, set `PWSH_BIN` to its executable.
On Ubuntu x86-64, install the lint tools with:

```sh
python3 -m venv work/ci-tools
work/ci-tools/bin/pip install --require-hashes -r scripts/requirements-ci.txt
export PATH="$PWD/work/ci-tools/bin:$PATH"
```

On other platforms, install the tool versions listed in `scripts/requirements-ci.txt`.

## Development

The grammar is in `grammar.js`; external tokenization is in `src/scanner.c`.
Rust is the only supported binding.
Regenerate the parser after changing the grammar:

```sh
tree-sitter generate --js-runtime native
```

Include a regression case in `test/corpus` or `tests/fixtures` for syntax changes.
The [tree design](docs/grammar-sources.md) describes node conventions and how tests
compare the grammar with PowerShell's parser.

Format changes with:

```sh
cargo fmt --all
clang-format -i src/scanner.c
ruff format scripts
ruff check --fix scripts
```

## Checks

Run the same checks as CI:

```sh
tree-sitter test --rebuild
TREE_SITTER_SEED=0 tree-sitter fuzz --rebuild --iterations 100 --edits 3
cargo test --locked
python3 scripts/generate-fixtures.py
python3 scripts/generate-fixtures.py --variables
scripts/check-generated.sh
scripts/lint.sh
```

The PowerShell tests parse fixture scripts without executing them.
`scripts/check-generated.sh` verifies generated files and imported fixture hashes;
`scripts/lint.sh` checks formatting, compiler warnings, and workflow security.

To reproduce a fuzz failure, use the reported seed, the same edit setting, and a
case-name filter:

```sh
TREE_SITTER_SEED=42 tree-sitter fuzz --rebuild --iterations 1 --edits 3 --include 'Composite quoted'
```

Tree-sitter 0.27.1 can report fuzz failures while exiting zero, so check its output.
CI also checks the log.

## Releases

Update the version in `Cargo.toml`, `Cargo.lock`, `tree-sitter.json`, and the README
dependency example. Regenerate the parser, run the checks above, commit, and push
an annotated `v<version>` tag. The [publish workflow](.github/workflows/publish.yml)
publishes to crates.io using Trusted Publishing.
