# Sources and tree design

## Syntax references

Microsoft's [PowerShell language specification](https://learn.microsoft.com/en-us/powershell/scripting/lang-spec/chapter-15)
describes PowerShell 3.0 and has not been updated for later releases. It provides the
historical grammar. PowerShell 7 syntax is checked against the documentation and
implementation below.

| Area | Specification | Current Microsoft documentation |
| --- | --- | --- |
| Tokens, variables, strings, numbers | [Chapter 2](https://learn.microsoft.com/en-us/powershell/scripting/lang-spec/chapter-02) | [Quoting rules](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_quoting_rules), [numeric literals](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_numeric_literals) |
| Expressions and precedence | [Chapter 7](https://learn.microsoft.com/en-us/powershell/scripting/lang-spec/chapter-07) | [Operators](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_operators), [operator precedence](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_operator_precedence) |
| Commands, pipelines, statements | [Chapter 8](https://learn.microsoft.com/en-us/powershell/scripting/lang-spec/chapter-08) | [Parsing](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_parsing), [pipeline chains](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_pipeline_chain_operators), [redirection](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_redirection) |
| Functions and parameters | [Chapter 8](https://learn.microsoft.com/en-us/powershell/scripting/lang-spec/chapter-08) | [Functions](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_functions), [advanced functions](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_functions_advanced), [named blocks](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_functions_advanced_methods) |
| Classes, enums, generics | Added after PowerShell 3.0 | [Classes](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_classes), [enums](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_enum), [generic method calls](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_calling_generic_methods) |

The implementation reference is PowerShell 7.5.4:

- [Parser.cs](https://github.com/PowerShell/PowerShell/blob/v7.5.4/src/System.Management.Automation/engine/parser/Parser.cs) defines expressions, statements, and parser contexts.
- [tokenizer.cs](https://github.com/PowerShell/PowerShell/blob/v7.5.4/src/System.Management.Automation/engine/parser/tokenizer.cs) defines token boundaries, quoting, escapes, and character classes.
- [token.cs](https://github.com/PowerShell/PowerShell/blob/v7.5.4/src/System.Management.Automation/engine/parser/token.cs) defines token kinds and operator precedence.

The upstream [Parsing.Tests.ps1](https://github.com/PowerShell/PowerShell/blob/v7.5.4/test/powershell/Language/Parser/Parsing.Tests.ps1),
[Parser.Tests.ps1](https://github.com/PowerShell/PowerShell/blob/v7.5.4/test/powershell/Language/Parser/Parser.Tests.ps1),
and [LineContinuance.Tests.ps1](https://github.com/PowerShell/PowerShell/blob/v7.5.4/test/powershell/Language/Parser/LineContinuance.Tests.ps1)
provide regression examples. When historical prose and the pinned implementation
differ, tests follow the pinned parser.

## Concrete syntax tree

Named nodes preserve constructs useful to editors: commands, individual arguments
and parameters, expressions, statements, declarations, and string interpolation.
Fields such as `name`, `left`, `right`, `condition`, `body`, and `value` identify their
relationships. [`src/node-types.json`](../src/node-types.json) lists the node schema;
[`queries/highlights.scm`](../queries/highlights.scm) supplies highlighting captures.

The node design follows tree-sitter's guidance on
[recognizable constructs](https://tree-sitter.github.io/tree-sitter/creating-parsers/3-writing-the-grammar.html),
[fields](https://tree-sitter.github.io/tree-sitter/creating-parsers/3-writing-the-grammar.html#using-fields),
and [hidden supertypes](https://tree-sitter.github.io/tree-sitter/creating-parsers/3-writing-the-grammar.html#using-supertypes).
Names use `snake_case`. Abstract expression and statement categories are hidden;
operators and punctuation are anonymous children. PowerShell-specific names such
as `invocation_expression`, `member_access`, and `element_access` follow the
language specification's expression productions.

Calls expose `function`, optional `type_arguments`, and `arguments`. Parenthesized
arguments form an `argument_list`; the shorthand `$items.Where{ $_.Enabled }`
instead has one `script_block` as its `arguments`. Functions and methods expose
`parameters` and `body`. Classes and enums have a `class_body` or `enum_body`, whose
`members` fields identify the declarations between their braces. These list and
body nodes preserve source boundaries, including empty lists and bodies.
The official [C# grammar](https://github.com/tree-sitter/tree-sitter-c-sharp/blob/8c0abe0b84a3681d3e6852ce2e2a1eecb4f731c5/grammar.js)
uses the same argument-list and declaration-body conventions.

`for_statement` exposes optional `initializer`, `condition`, and `update` fields,
plus `body`. This matches the roles used by the official
[Bash grammar](https://github.com/tree-sitter/tree-sitter-bash/blob/a06c2e4415e9bc0346c6b86d401879ffb44058f7/grammar.js#L211-L229).
Declaration names, parameter defaults, class property values, catch types, and
conditional clauses also have fields, so queries can select their syntactic role.

A `pipeline` exposes complete `elements`: a `command`, or a `command_expression`
with `expression` and optional `redirections`. Pipes connect these elements.
`pipeline_chain` instead exposes `left`, `operator`, and `right`, so
`a | b || c | d && e` groups as `((a | b) || (c | d)) && e`.
The chain operators have equal precedence and associate left, as described in
[Microsoft's pipeline-chain documentation](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_pipeline_chain_operators#operator-syntax-and-precedence).
`background_pipeline` wraps the complete pipeline or chain in its `pipeline`
field and retains the trailing `&` in `operator`.

An `assignment_statement` is a statement with `left`, `operator`, and `right`;
its right side can be another assignment, a pipeline chain, or a control
statement. In `$result = a && b & next`, the assignment's right side is a
`background_pipeline`, and `next` starts another statement. The concrete
assignment includes `&`, while PowerShell's assignment and pipeline-chain AST
ranges end before that marker. The oracle comparison accounts for this source
boundary explicitly.

For example, `Write-Output prefix"$name"` has one command argument containing a
literal fragment and an expandable string. Its named tree is:

```scheme
(source_file
  (pipeline
    elements: (command
      name: (command_name
        (bare_word))
      (command_argument
        (bare_word)
        (expandable_string
          (variable))))))
```

Composite values stay together when used as a parameter's `value`, a redirection's
`destination`, or a switch clause's `condition`. For example,
`Write-Output -Path:prefix"$name".txt` has one `command_argument` spanning
`prefix"$name".txt` in the parameter's `value` field. In command-mode comma arrays,
each element is also a complete `command_argument`: `Write-Output pre$x,post$y`
has two array elements. `node-types.json` records whether each field is scalar or
repeated; repeated fields can be retrieved with `children_by_field_name`.

In `Write-Output $HOME.Length-more`, the member-access argument ends before the
`-more` parameter. Expandable here strings expose
`variable`, `sub_expression`, `string_content`, and `escape_sequence` children in the
same way as ordinary expandable strings, including literal whitespace.

Operator grouping follows the pinned parser. For example,
`$true -or $false -and $false` groups as `($true -or $false) -and $false`, and
`1 -eq 2 -shl 3` groups as `(1 -eq 2) -shl 3`. Null coalescing binds above comparison
and below addition. Braced variables can contain supplementary Unicode characters:
`${𐐀}` is a variable, while `$𐐀` is a command token.

PowerShell's semantic AST includes implicit wrappers and combines some source
fragments. The CST preserves the original spelling and punctuation. PowerShell
performs name resolution and execution; tree-sitter provides syntax and recovery
for incomplete editor input.

## Checking the tree

[`scripts/oracle.ps1`](../scripts/oracle.ps1) calls
`System.Management.Automation.Language.Parser.ParseInput` and records diagnostics,
tokens, and AST ranges without running fixture scripts. Tests compare syntax
acceptance, command elements and parameter classification, expression grouping,
and corresponding declaration or control-flow ranges. PowerShell reports UTF-16
code-unit offsets; the Rust harness converts tree-sitter's UTF-8 byte offsets before
comparing them.

Corpus tests assert explicit trees. Focused fixtures and generated token matrices
cover context-dependent syntax, and complete project files exercise interactions
across larger scripts. Imported sources carry their provenance and hashes with the
fixture collection; see the [PowerShell fixtures](../tests/upstream/README.md) and
[external project fixtures](../tests/projects/README.md).
Contributor commands are in [CONTRIBUTING.md](../CONTRIBUTING.md).
