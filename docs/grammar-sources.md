# Grammar sources and design

## Normative references

Microsoft's [PowerShell language specification](https://learn.microsoft.com/en-us/powershell/scripting/lang-spec/chapter-15) defines the historical grammar. The specification says it describes PowerShell 3.0, was published in 2012, and is not updated for current releases. Consequently, it cannot by itself specify a current PowerShell grammar.

| Grammar area | Microsoft specification | Current documentation |
| --- | --- | --- |
| Tokens, comments, literals, variables | [Chapter 2](https://learn.microsoft.com/en-us/powershell/scripting/lang-spec/chapter-02) | [Quoting rules](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_quoting_rules), [numeric literals](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_numeric_literals) |
| Expressions, operators, precedence | [Chapter 7](https://learn.microsoft.com/en-us/powershell/scripting/lang-spec/chapter-07) | [Operators](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_operators), [operator precedence](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_operator_precedence) |
| Pipelines, statements, control flow | [Chapter 8](https://learn.microsoft.com/en-us/powershell/scripting/lang-spec/chapter-08) | [Parsing](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_parsing), [pipeline chains](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_pipeline_chain_operators), [redirection](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_redirection) |
| Functions, parameters, attributes | [Chapter 8](https://learn.microsoft.com/en-us/powershell/scripting/lang-spec/chapter-08) | [Functions](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_functions), [advanced functions](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_functions_advanced), [named blocks](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_functions_advanced_methods) |
| Classes, enums, generics | Absent from the PowerShell 3.0 specification | [Classes](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_classes), [enums](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_enum), [generic method calls](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_calling_generic_methods) |

## Runtime oracle

PowerShell 7.5.4 is the reproducible reference implementation used in CI. Its [Parser.cs](https://github.com/PowerShell/PowerShell/blob/v7.5.4/src/System.Management.Automation/engine/parser/Parser.cs), [tokenizer.cs](https://github.com/PowerShell/PowerShell/blob/v7.5.4/src/System.Management.Automation/engine/parser/tokenizer.cs), and [token.cs](https://github.com/PowerShell/PowerShell/blob/v7.5.4/src/System.Management.Automation/engine/parser/token.cs) provide the implementation-level definitions for current syntax. Useful upstream regression suites include [Parsing.Tests.ps1](https://github.com/PowerShell/PowerShell/blob/v7.5.4/test/powershell/Language/Parser/Parsing.Tests.ps1), [Parser.Tests.ps1](https://github.com/PowerShell/PowerShell/blob/v7.5.4/test/powershell/Language/Parser/Parser.Tests.ps1), and [LineContinuance.Tests.ps1](https://github.com/PowerShell/PowerShell/blob/v7.5.4/test/powershell/Language/Parser/LineContinuance.Tests.ps1).

`scripts/oracle.ps1` calls `System.Management.Automation.Language.Parser.ParseInput`. It does **not** execute fixture code. It records errors, token extents, and AST node extents. Rust tests compare validity with the checked-in fixtures and compare selected binary-expression extents to PowerShell's AST. This verifies grouping and associativity, not just whether text parses. PowerShell reports UTF-16 offsets; the Rust harness converts tree-sitter's byte offsets before comparing them.

When prose documentation disagrees with the implementation, the pinned parser and runtime oracle determine the expected syntax. For example, current `token.cs` gives the logical operators one shared left-associative level and places shifts at comparison precedence. Null coalescing binds above comparisons and below addition. These relationships have explicit oracle and corpus tests.

Lexical details also come from the runtime. For example, supplementary Unicode characters are permitted inside braced variables such as `${𐐀}`; the unbraced `$𐐀` is a command token rather than a variable. Numeric-looking text can likewise be a command name instead of a numeric expression. The combinatorial fixtures in `tests/fixtures/generated.json` cover these distinctions, with a reproducible generator in `scripts/generate-fixtures.py`.

## Concrete syntax tree

The tree retains source-level constructs useful to editors: commands and command arguments, expressions, statements, parameters, type names, comments, and string interpolation. It is a concrete syntax tree, so its nodes are not a one-to-one copy of PowerShell's semantic AST. Fields such as `left`, `operator`, `right`, `condition`, `body`, `name`, and `value` identify important relationships without requiring child-position assumptions.

The external scanner handles context-sensitive token boundaries that regular expressions cannot express faithfully, including line-sensitive here-string delimiters. Generated C files are committed so Rust consumers do not need a JavaScript runtime or the tree-sitter CLI to build the crate.

Expandable here strings expose interpolation as ordinary `variable` and `sub_expression` nodes. Their footer must follow an unescaped physical line break at column zero. The scanner uses stateless delimiter tokens so nested here strings and incremental recovery do not retain stale delimiter state. Here-string backtick escapes consume one UTF-16 character: escaping the CR in CRLF still leaves LF as a line break. Tests check this behavior against the runtime.

`src/variable_chars.h` records the pinned runtime's BMP `Char.IsLetterOrDigit` ranges. Regenerate it with `scripts/generate-unicode.ps1`; `scripts/check-generated.sh` verifies it alongside tree-sitter's generated files. This avoids locale-dependent classification and keeps unbraced variable recognition aligned with PowerShell's UTF-16 tokenizer.

This is a syntax parser. Name resolution, available commands/types, execution policy, duplicate declarations, and runtime behavior remain PowerShell's responsibility. Error recovery supports incomplete editor input; it is not a substitute for PowerShell's diagnostics.
