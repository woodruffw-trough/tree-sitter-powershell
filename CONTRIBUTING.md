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
python3 -m unittest discover -s scripts -p 'test_*.py'
python3 scripts/fuzz.py
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

## Fuzzing

The pinned CLI provides [`tree-sitter fuzz`](https://github.com/tree-sitter/tree-sitter/blob/v0.27.1/docs/src/cli/fuzz.md).
It starts from the corpus in `test/corpus`, applies random text edits, checks node
sizes and changed ranges, reverses the edits, and compares the restored tree with
the corpus expectation. It also checks tree-sitter's tracked allocations for leaks.
This complements the PowerShell oracle tests: it exercises malformed editor input
and incremental parsing without running PowerShell scripts.

Run the same deterministic budget as pull-request and release CI:

```sh
python3 scripts/fuzz.py --seed 0 --iterations 100 --edits 3
```

Iterations are per corpus case. The pinned CLI performs between one and
`edits + 1` edit operations per trial. With 120 corpus cases, the default budget
exercises 12,000 edit-and-undo trials. The `Fuzz` workflow runs 5,000 iterations
per case with `--edits 8`, using its run ID as the starting seed to vary coverage.
It runs daily, on manual dispatch, and when the runner or its workflow changes on
`main`.

Use the Python runner instead of invoking the CLI directly: tree-sitter 0.27.1
can print a fuzzing failure and still exit successfully. The runner requires the
pinned CLI version, checks diagnostics and the complete trial trace, and rejects
skipped or empty runs. Its contract tests exercise these failure modes with a
stand-in CLI. The runner sets the QuickJS runtime explicitly.

Each run saves the full log, settings, and a replay command under `work/fuzz`.
GitHub Actions keeps these artifacts for seven days, including failed runs. To
reproduce a failing seed against one corpus case:

```sh
python3 scripts/fuzz.py --seed 42 --iterations 1 --edits 3 --include 'Composite quoted'
```

Use the seed and edit setting from the failure log and a case-name expression
matching that case. Keep the parser and CLI versions unchanged while reproducing;
the random edit sequence also depends on the input. Reduce a failure to a focused
incremental regression before changing the grammar or scanner.

## Releases

`.github/workflows/publish.yml` publishes tags named `v<version>`. It runs the full
CI workflow, checks that the tag matches both `Cargo.toml` and `tree-sitter.json`,
and verifies the package before authenticating with crates.io.

The publishing job uses the GitHub environment `crates.io`. GitHub creates it
when the first publishing job starts. The official `rust-lang/crates-io-auth-action`
uses [Trusted Publishing](https://crates.io/docs/trusted-publishing) to obtain a
short-lived publishing token through GitHub OIDC and revokes it after the job.

Enroll the crate's GitHub publisher on crates.io with these values:

| Field | Value |
| --- | --- |
| Repository owner | `woodruffw-trough` |
| Repository name | `tree-sitter-powershell` |
| Workflow filename | `publish.yml` |
| Environment | `crates.io` |

To release, update both version fields and the README dependency, then regenerate
the parser with `tree-sitter generate --js-runtime native` to update its embedded
version. Run the checks above, commit the change, and push an annotated
`v<version>` tag at that commit. The publishing workflow does not cancel an active
release when another run starts for the same tag.
