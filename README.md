# tree-sitter-powershell

A tree-sitter grammar for PowerShell, with Rust bindings and a highlighting query for
`.ps1`, `.psm1`, and `.psd1` files.

The grammar covers commands, expressions, pipelines, redirection, control flow,
functions, classes, enums, types, and string interpolation. PowerShell 7 features
include pipeline chains, ternary and null-coalescing operators, null-conditional
access, generic method calls, and `clean` blocks. Tests use PowerShell 7.5.4 as the
reference parser. Standard DSC resources and legacy workflow syntax are also
represented. [Parser conformance](docs/conformance.md) describes the test coverage,
semantic boundaries, and known differences.

## Rust usage

The Rust crate is `woodruffw-trough-tree-sitter-powershell`. It requires Rust 1.90
or later and a C compiler. Add it from crates.io:

```toml
[dependencies]
tree-sitter = "0.27.1"
woodruffw-trough-tree-sitter-powershell = "0.1.5"
```

```rust
let mut parser = tree_sitter::Parser::new();
parser
    .set_language(&woodruffw_trough_tree_sitter_powershell::LANGUAGE.into())
    .unwrap();
let tree = parser
    .parse("Get-ChildItem | Where-Object { $_.Length -gt 0 }", None)
    .unwrap();
assert!(!tree.root_node().has_error());
```

The crate exports `LANGUAGE`, `NODE_TYPES`, and `HIGHLIGHTS_QUERY`. Generated parser
sources are included, so building the Rust crate does not require the tree-sitter CLI.

## Syntax and trees

The tree preserves command arguments and parameters, expression grouping, member
and index access, declarations, and interpolation. It supports error recovery and
incremental edits for editor use. PowerShell handles name resolution and execution.

Microsoft's language specification describes PowerShell 3.0. Current syntax is
checked against Microsoft's documentation, the PowerShell parser source, and
`Parser::ParseInput` in the pinned runtime. See [sources and tree design](docs/grammar-sources.md)
for references and examples.

Tests include explicit corpus trees, focused oracle cases, generated token matrices,
and complete files from PowerShell and [independent projects](tests/projects/README.md).
The oracle parses their source without running the scripts; checks compare
corresponding tree and AST structure as well as syntax acceptance.

## Contributing

[CONTRIBUTING.md](CONTRIBUTING.md) covers setup, formatting, regeneration, and tests.
Grammar generation uses tree-sitter's embedded QuickJS backend. Rust is the only
binding provided by this repository.
