# tree-sitter-powershell

A tree-sitter grammar for PowerShell, with Rust bindings.

The grammar covers command and expression modes, pipelines and redirection, control flow, advanced functions, classes and enums, types, strings, and PowerShell 7 operators. See [grammar sources and design](docs/grammar-sources.md) for the authority behind each area and the oracle methodology.

## Authoritative sources

The grammar follows Microsoft's [PowerShell language specification](https://learn.microsoft.com/en-us/powershell/scripting/lang-spec/chapter-15), especially its [lexical structure](https://learn.microsoft.com/en-us/powershell/scripting/lang-spec/chapter-02), [expressions](https://learn.microsoft.com/en-us/powershell/scripting/lang-spec/chapter-07), and [statements](https://learn.microsoft.com/en-us/powershell/scripting/lang-spec/chapter-08).
That specification describes PowerShell 3.0. For current PowerShell syntax, this grammar uses the [about topics](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_parsing), the upstream [parser](https://github.com/PowerShell/PowerShell/blob/v7.5.4/src/System.Management.Automation/engine/parser/Parser.cs) and [tokenizer](https://github.com/PowerShell/PowerShell/blob/v7.5.4/src/System.Management.Automation/engine/parser/tokenizer.cs), and PowerShell 7.5.4 itself as a pinned oracle.

## Development policy

Only Rust bindings are enabled in `tree-sitter.json`. The C sources in `src/` are the generated parser implementation used by Rust, not a separate binding.

Install tree-sitter's CLI with its QuickJS backend only:

```sh
cargo install tree-sitter-cli --version 0.27.1 --locked --no-default-features --features qjs-rt
export TREE_SITTER_JS_RUNTIME=native
tree-sitter generate --js-runtime native
```

`native` selects the embedded QuickJS runtime. Always pass this option; the CLI's default runtime is Node. Grammar generation and Rust consumption do not require Node or npm.

## Rust usage

```rust
let mut parser = tree_sitter::Parser::new();
parser.set_language(&tree_sitter_powershell::LANGUAGE.into())?;
let tree = parser.parse("Get-ChildItem | Where-Object { $_.Length -gt 0 }", None).unwrap();
assert!(!tree.root_node().has_error());
```

The crate exports `LANGUAGE`, `NODE_TYPES`, and `HIGHLIGHTS_QUERY`.

## Validation

Development uses Rust 1.90.0, tree-sitter CLI 0.27.1, and PowerShell 7.5.4. Install PowerShell or set `PWSH_BIN` to its executable. The oracle parses fixture sources without running them.

```sh
tree-sitter generate --js-runtime native
tree-sitter test --rebuild
cargo test --locked
python3 scripts/generate-fixtures.py
python3 scripts/generate-fixtures.py --variables
scripts/check-generated.sh
```

Corpus tests assert complete trees. Rust tests cover valid and invalid oracle fixtures, command argument grouping and parameter classification, operator precedence, variable and here-string interpolation, type boundaries, Unicode offsets and escapes, long tokens, incremental edits, incomplete input, and query compilation. Integration tests also parse six complete, attributed scripts from PowerShell's own repository and compare their command and expression boundaries with the oracle. `scripts/check-generated.sh` verifies committed output matches a fresh QuickJS generation, checks that Rust is the only binding, and verifies upstream fixture hashes.

For linting, install ShellCheck and the pinned security linter, then run:

```sh
python3 -m venv work/ci-tools
work/ci-tools/bin/pip install --require-hashes -r scripts/requirements-ci.txt
PATH="$PWD/work/ci-tools/bin:$PATH" scripts/lint.sh
```

Linting checks Rust formatting, clippy, documentation, the scanner's C warnings, shell scripts, Python syntax, and GitHub Actions security with strict, pedantic zizmor. `ZIZMOR_OFFLINE=true` permits a local offline audit; CI includes the online action-vulnerability audit.
