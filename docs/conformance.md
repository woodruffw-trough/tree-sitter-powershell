# Parser conformance

The compatibility target is Microsoft's **PowerShell 7.5.4 parser**, including
its expression and command parsing modes. The historical language specification
covers PowerShell 3.0; the pinned implementation supplies later syntax and
context rules. [Sources and tree design](grammar-sources.md) links the Microsoft
documentation and explains the public CST.

Tests cover individual parser productions, upstream regression inputs, complete
scripts, and incremental edits. Passing them does not establish full equivalence
with Microsoft's parser. Known differences are listed below.

The grammar targets agreement on valid syntax and CST structure and rejection
of malformed syntax. Recovery after a syntax error is best effort; damaged
subtrees and following command boundaries can differ from PowerShell's recovered
AST.

## What the tests compare

[`scripts/oracle.ps1`](../scripts/oracle.ps1) calls `Parser.ParseInput` without
executing the input. Tests compare acceptance, exact diagnostic identifiers where
specified, command elements, and mapped AST relationships and source extents.
The harness converts tree-sitter byte offsets to PowerShell's UTF-16 offsets.
Comparisons include effective switch/foreach flags, function/filter/workflow
keywords, invocation operators, explicit named-block roles, and named attribute
values and shorthand flags. Declaration comparisons include variable paths,
function name boundaries, and unqualified property names. Corpus snapshots
independently assert concrete nodes, fields, and grouping.

PowerShell sometimes constructs a complete AST before reporting a semantic
diagnostic. Fixtures record that distinction explicitly: a method parameter
block, splatted parameter, or invalid assignment target can retain its complete
CST while the oracle reports its diagnostic. This does not make tree-sitter a
semantic validator. Module loading, command resolution, parameter binding,
resource compilation, and execution remain PowerShell responsibilities. The
grammar retains `#requires` directives and region markers as comments; PowerShell
validates `#requires` metadata.

Some semantic checks replace a parsed declaration with an error AST. For example,
PowerShell validates configuration names as constant strings and rejects names
such as `1` with `InvalidConfigurationName`. The grammar retains their name and
body syntax; configuration tests assert those roles independently.

The AST comparison covers the relationships represented by its mapping, rather
than every AST property or evaluated value. Synthetic AST wrappers, background
pipeline extents, collected string text, and other source-boundary differences
have explicit normalization rules and focused tests.

## Production families

The rule names below refer to the pinned
[`Parser.cs`](https://github.com/PowerShell/PowerShell/blob/v7.5.4/src/System.Management.Automation/engine/parser/Parser.cs)
and [`tokenizer.cs`](https://github.com/PowerShell/PowerShell/blob/v7.5.4/src/System.Management.Automation/engine/parser/tokenizer.cs).
Each row identifies representative suites; their fixtures contain additional
valid, malformed, and context-dependent variants.

| Microsoft production family | Exercised behavior | Principal suites |
| --- | --- | --- |
| `ExpressionRule`, `BinaryExpressionRule`, `ArrayLiteralRule`, `UnaryExpressionRule` | Operator precedence and associativity, ternaries, comma suppression, recursive prefixes, casts and attributes | [expression_conformance](../tests/expression_conformance.rs), [expression_contexts](../tests/expression_contexts.rs), [attributes](../tests/attributes.rs) |
| `PrimaryExpressionRule`, `MemberAccessRule`, `InvokeParamParenListRule` | Member/index boundaries, computed names, method arguments, generic calls, postfix operators | [methods](../tests/methods.rs), [member_names](../tests/member_names.rs), [invocations](../tests/invocations.rs) |
| `ScanNumber`, `TryGetNumberValue`, variable and string scanners | Numeric ranges and suffixes, adjacent signs, Unicode variables, quoting, here-strings, collected interpolation | [numeric_contexts](../tests/numeric_contexts.rs), [variables](../tests/variables.rs), [string_contexts](../tests/string_contexts.rs), [here_strings](../tests/here_strings.rs) |
| `CommandRule`, `GetCommandArgument`, `ScanParameter`, `ScanGenericToken` | Command/expression dispatch, composite arguments, parameter values, splats, contextual names and invocation targets | [command_contexts](../tests/command_contexts.rs), [names](../tests/names.rs), [splats](../tests/splats.rs) |
| `PipelineRule`, `PipelineChainRule`, `RedirectionRule`, `GetVerbatimCommandArgument` | Leading pipes, chains, backgrounding, file/stream boundaries and `--%` text | [pipelines](../tests/pipelines.rs), [redirections](../tests/redirections.rs), [verbatim](../tests/verbatim.rs) |
| `StatementRule`, `LabelOrKeyRule`, control-flow rules, `HashExpressionRule` | Conditions, loops, switch clauses, flow values and labels, try/catch, assignment values and hashtable entries | [statement_conformance](../tests/statement_conformance.rs), [flow_conformance](../tests/flow_conformance.rs), [flow_labels](../tests/flow_labels.rs), [hash_contexts](../tests/hash_contexts.rs) |
| `ScriptBlockRule`, `NamedBlockListRule`, `ParamBlockRule`, `ParameterRule` | Header eligibility, parameter defaults and prefixes, named blocks, nested bodies and method contexts | [declaration_conformance](../tests/declaration_conformance.rs), [statement_conformance](../tests/statement_conformance.rs) |
| `ClassDefinitionRule`, `ClassMemberRule`, `MethodDeclarationRule`, enum rules | Declaration names, modifiers, attributes, member separators, constructors and enum bodies | [class_members](../tests/class_members.rs), [declaration_conformance](../tests/declaration_conformance.rs) |
| Type/attribute rules, `UsingStatementRule`, `DataStatementRule` | Nested generic and assembly-qualified types, attribute arguments, constant using values and aliases, data command lists | [types](../tests/types.rs), [assembly_names](../tests/assembly_names.rs), [using_statements](../tests/using_statements.rs), [data_statements](../tests/data_statements.rs) |
| `ConfigurationStatementRule`, `DynamicKeywordStatementRule`, `InlineScriptRule` | Configuration headers and resource bodies, contextual keyword scopes, workflow bodies and `InlineScript` commands | [configuration_conformance](../tests/configuration_conformance.rs), [dsc](../tests/dsc.rs), [workflow](../tests/workflow.rs) |

[Upstream cases](../tests/upstream_cases.rs) include individual parser-test inputs
as well as complete Microsoft scripts. [Project fixtures](../tests/projects/README.md)
retain their source revision, license, path, and hash. Their tests compare relevant
structure; parsing a whole file alone does not establish that every subtree is
correct. Schema and highlighting tests exercise public queries. Incremental
tests compare edited trees with fresh parses, and direct `tree-sitter fuzz` runs
exercise edits to the corpus. Long-scope and interpolation tests check
Tree-sitter robustness; they do not assert that PowerShell accepts the same
nesting depth.

## Known limits and compatibility decisions

**Constructor-name checks.** A nonstatic `:base(...)` initializer is
accepted after a method whose name differs from its enclosing class, such as
`class C { F():base() {} }`. Microsoft rejects it during parsing. Static base
initializers are rejected. Comparing the two arbitrary-length names
case-insensitively is a contextual check outside this grammar's scope.

**Dynamically registered syntax.** DSC resource grouping covers an explicit set
of standard Windows/PSDscResources and Local Configuration Manager keywords,
plus `Node` and `Import-DscResource`. Custom imported resource registries and
arbitrary embedded CIM instance classes can require different grouping. The
oracle supplies scoped keyword metadata without importing modules or running
resource hooks. Legacy workflow syntax is tested separately from PowerShell
Core's `WorkflowNotSupportedInPowerShellCore` diagnostic.

**Malformed-input recovery.** Tests check that malformed interpolation retains
an `ERROR`, the enclosing string, subsequent arguments, and following pipeline
commands. Recovery still differs for some incomplete hashtable values, member
continuations, malformed switch headers, and malformed here-string headers in
attribute arguments.
These can still absorb later commands. Some damaged command extents also differ:
malformed colon commands and empty groups after generic arguments such as
`cmd $$.M()` retain a suffix outside the command node, where PowerShell includes
it in the recovered command extent. Malformed `using module ."text"` can recover
with the quoted suffix as its value, where PowerShell retains the dot.
Tree-sitter's recovery tree does not promise Microsoft's diagnostic sequence or
recovered AST for every input. Long runs of invalid attribute prefixes can
require quadratic lookahead during recovery.
[Variable tests](../tests/variables.rs) check malformed interpolation in command
arguments, including cases that preserve following commands.
[Operator](../tests/operator_recovery.rs), [member](../tests/member_trivia.rs),
and [flow-label](../tests/flow_labels.rs) recovery tests compare following command
boundaries independently of the damaged argument and label nodes.

**Leading pipes in collected interpolation.** The grammar consistently accepts
leading-pipe continuation within interpolated subexpressions. In 7.5.4,
`StartNestedScan` retains absolute source extents while `IsPipeContinuation`
indexes the shorter collected text; inserting text before `$(` can therefore
change acceptance and produce `EmptyPipeElement`. Dedicated
[compatibility tests](../tests/pipeline_continuation_compatibility.rs) assert the
native diagnostics separately from the intended pipeline CST.

**Typed ranges in collected interpolation.** The grammar retains the intended
range expression in `"$([int] .. $other)"`. In 7.5.4, `GetLBracket` passes an
offset relative to collected text to `Resync`, which expects an absolute offset.
This can rewind into a completed type literal, producing overlapping type and
command ASTs. Adding text before `$(` can instead produce `ScriptTooComplicated`
with no AST. Dedicated [compatibility tests](../tests/typed_interpolation_compatibility.rs)
check native diagnostics and overlapping extents separately from the intended
range operands in the CST.

**Unusable native oracle results.** Certain invalid numeric hash keys, including
`@{128y = 1}`, make `ParseInput` throw before returning an AST. Registered DSC
keywords with escaped or composite spellings, such as `` `File `` and `F"ile"`,
can trigger a caught `NullReferenceException` and discard the configuration AST.
These inputs provide no reliable conformance tree and are excluded from ordinary
oracle batches rather than classified as successful parses.

**Speculative numeric-comment diagnostics.** PowerShell can retain a diagnostic
from generic-token lookahead even after restoring a valid number followed by a
comment. Examples include `-1#'`, `127y#$bad:`, and an unfinished braced variable
inside the comment. The grammar follows the final tokens and AST without adding
an error. [Numeric tests](../tests/numeric_contexts.rs) assert both that structure
and the native diagnostics.

**Nested collected-string extents.** PowerShell can lose a quote-offset layer
in nested interpolation and place AST nodes before the corresponding source text.
[String tests](../tests/string_contexts.rs) assert physical quote boundaries
independently for those cases.
