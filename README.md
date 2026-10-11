# tree-sitter-powershell

A tree-sitter grammar for PowerShell, with Rust bindings.

## Authoritative sources

The grammar follows Microsoft's [PowerShell language specification](https://learn.microsoft.com/en-us/powershell/scripting/lang-spec/chapter-15), especially its [lexical structure](https://learn.microsoft.com/en-us/powershell/scripting/lang-spec/chapter-02), [expressions](https://learn.microsoft.com/en-us/powershell/scripting/lang-spec/chapter-07), and [statements](https://learn.microsoft.com/en-us/powershell/scripting/lang-spec/chapter-08).
That specification describes PowerShell 3.0. For current PowerShell syntax, use the [about topics](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_parsing), the upstream [parser](https://github.com/PowerShell/PowerShell/blob/master/src/System.Management.Automation/engine/parser/Parser.cs) and [tokenizer](https://github.com/PowerShell/PowerShell/blob/master/src/System.Management.Automation/engine/parser/tokenizer.cs), and the PowerShell parser itself as an oracle.

## Development policy

Only Rust bindings are enabled in `tree-sitter.json`. The C sources in `src/` are the generated parser implementation used by Rust, not a separate binding.

Install tree-sitter's CLI with its QuickJS backend only:

```sh
cargo install tree-sitter-cli --version 0.27.1 --locked --no-default-features --features qjs-rt
tree-sitter generate --js-runtime native
```

`native` selects the embedded QuickJS runtime. Always pass this option; the CLI's default runtime is Node. This repository does not use Node or npm.
