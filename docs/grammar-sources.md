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
| Data sections | [Chapter 8](https://learn.microsoft.com/en-us/powershell/scripting/lang-spec/chapter-08) | [Data sections](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_data_sections) |
| Classes, enums, generics | Added after PowerShell 3.0 | [Classes](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_classes), [enums](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_enum), [generic method calls](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_calling_generic_methods) |
| DSC configurations and resources | Added after PowerShell 3.0 | [Configurations](https://learn.microsoft.com/en-us/powershell/dsc/configurations/configurations?view=dsc-1.1), [Windows resources](https://learn.microsoft.com/en-us/powershell/dsc/reference/resources/windows/?view=dsc-1.1), [LCM settings](https://learn.microsoft.com/en-us/powershell/dsc/managing-nodes/metaconfig?view=dsc-1.1) |

The implementation reference is PowerShell 7.5.4:

- [Parser.cs](https://github.com/PowerShell/PowerShell/blob/v7.5.4/src/System.Management.Automation/engine/parser/Parser.cs) defines expressions, statements, and parser contexts.
- [tokenizer.cs](https://github.com/PowerShell/PowerShell/blob/v7.5.4/src/System.Management.Automation/engine/parser/tokenizer.cs) defines token boundaries, quoting, escapes, and character classes.
- [token.cs](https://github.com/PowerShell/PowerShell/blob/v7.5.4/src/System.Management.Automation/engine/parser/token.cs) defines token kinds and operator precedence.

The upstream [Parsing.Tests.ps1](https://github.com/PowerShell/PowerShell/blob/v7.5.4/test/powershell/Language/Parser/Parsing.Tests.ps1),
[Parser.Tests.ps1](https://github.com/PowerShell/PowerShell/blob/v7.5.4/test/powershell/Language/Parser/Parser.Tests.ps1),
and [LineContinuance.Tests.ps1](https://github.com/PowerShell/PowerShell/blob/v7.5.4/test/powershell/Language/Parser/LineContinuance.Tests.ps1)
provide regression examples. When historical prose and the pinned implementation
differ, tests follow the pinned parser. The speculative numeric-comment diagnostic
described below is checked separately from syntax acceptance.
[Parser conformance](conformance.md) lists the test scope, known limits, and
explicit compatibility decisions.

## Concrete syntax tree

Named nodes preserve constructs useful to editors: commands, individual arguments
and parameters, expressions, statements, declarations, and string interpolation.
Fields such as `name`, `left`, `right`, `condition`, `body`, and `value` identify their
relationships. [`src/node-types.json`](../src/node-types.json) lists the node schema;
[`queries/highlights.scm`](../queries/highlights.scm) supplies highlighting captures.

The node design follows tree-sitter's guidance on
[recognizable constructs](https://tree-sitter.github.io/tree-sitter/creating-parsers/3-writing-the-grammar.html),
[fields](https://tree-sitter.github.io/tree-sitter/creating-parsers/3-writing-the-grammar.html#using-fields),
and [hidden rules](https://tree-sitter.github.io/tree-sitter/creating-parsers/3-writing-the-grammar.html#hiding-rules).
Names use `snake_case`. Abstract expression and statement rules are internal;
operators and punctuation are anonymous children. PowerShell-specific names such
as `invocation_expression`, `member_access`, and `element_access` follow the
language specification's expression productions.

Queries use the concrete node names and fields in `node-types.json`. The grammar
does not expose abstract query supertypes: PowerShell's statement delimiters and
argument-expression contexts use different internal productions. For example,
`[(return_statement) (throw_statement)] @flow` selects both statement kinds
consistently in script bodies, nested blocks, and assignment values.

Calls expose `function`, optional `type_arguments`, and `arguments`. Parenthesized
arguments form an `argument_list`; the shorthand `$items.Where{ $_.Enabled }`
instead has one `script_block` as its `arguments`. Functions and methods expose
`parameters` and `body`. Classes and enums have a `class_body` or `enum_body`, whose
`members` fields identify the declarations between their braces. These list and
body nodes preserve source boundaries, including empty lists and bodies.
Class method bodies retain parameter blocks and named blocks in their
`statement_block`, even when PowerShell reports that those constructs are not
allowed in a method. Parameters likewise retain splatted variable syntax before
PowerShell reports `SplattingNotPermitted`. A `param_block` also retains preceding
type literals in `types`, alongside `attributes`, before the native parser
reports `TypeNotAllowedBeforeParam`. Static methods cannot have a
`base_constructor_call`. Enum bodies allow newlines before the first member;
semicolons separate members after the first.

Member names can also be expressions. In `$obj.-$name.Length`, the member is the
complete unary expression `-$name.Length`; in `$obj.$name.Length`, the final
`.Length` instead accesses the result of `$obj.$name`.
A type-leading member operand follows the same recursive rule: in
`$obj.[int].Name`, the complete member is `[int].Name`. A following script block
can instead be its conversion value, as in `$obj.[int]{1}`. Unary and type-leading
member operands cannot be invoked directly. Parentheses make a computed operand
invocable, as in `$obj.(-$name)()`. In command arguments, parentheses after an
unparenthesized unary member start another argument: `cmd $obj.- $name(1)` has
separate `$obj.- $name` and `(1)` arguments. These distinctions follow
`MemberNameRule`, `UnaryExpressionRule`, and `MemberAccessRule` in the pinned parser.
Generic method type arguments accept intervening whitespace and comments, as in
`$obj.Method [int](1)`. The call opener must touch the method name or closing
type bracket. Block comments preserve member/index adjacency but separate a
method name from its call. These rules follow `GenericMethodArgumentsRule`,
`GetMemberAccessOperator`, and `GetInvokeMemberOpenParen`.
The official [C# grammar](https://github.com/tree-sitter/tree-sitter-c-sharp/blob/8c0abe0b84a3681d3e6852ce2e2a1eecb4f731c5/grammar.js)
uses the same argument-list and declaration-body conventions.

`for_statement` exposes optional `initializer`, `condition`, and `update` fields,
plus `body`. This matches the roles used by the official
[Bash grammar](https://github.com/tree-sitter/tree-sitter-bash/blob/a06c2e4415e9bc0346c6b86d401879ffb44058f7/grammar.js#L211-L229).
PowerShell allows physical newlines as well as semicolons between header clauses;
the clause positions determine these fields.
Declaration names, parameter defaults, class property values, catch types, and
conditional clauses also have fields, so queries can select their syntactic role.
A function name is one literal `function_name`, including any embedded variable
or subexpression spelling. For example, `function f${name}($p) { $p }` has name
`f${name}` and a separate parameter list and body. A variable by itself cannot
name a function; adjacent generic text makes `${name}-tail` a literal name.
These boundaries follow `ScanGenericToken` and `FunctionDeclarationRule` in the
pinned PowerShell parser.
Control-header flags retain their complete spelling, including an optional colon.
For example, `foreach_parameter.value` contains the whole operand of
`-ThrottleLimit:`. PowerShell recognizes workflow-only foreach flags before
reporting that they are reserved; the tests check those diagnostics separately
from the resulting syntax tree. These operands follow
[`ForeachStatementRule`](https://github.com/PowerShell/PowerShell/blob/v7.5.4/src/System.Management.Automation/engine/parser/Parser.cs#L3358-L3516).

In a `data_statement`, each `supported_commands` field contains one complete
command name or expression. Literal names such as `foo$bar` remain `bare_word`
nodes; quoted strings, variables, member access, and grouped expressions retain
their usual structure. The statement's `body` is a `statement_block`.

`configuration_definition` exposes `attributes`, a complete `name`, and a
`script_block` as its `body`. Within a configuration, `node_statement` has a
`name` and script-block `body`; `resource_statement` has a resource `type`, an
optional `name`, and a `resource_body`. Each resource property is a `hash_entry`
under the body's `properties` field. `import_dsc_resource_statement` preserves
its command-like `elements`. These forms follow `ConfigurationStatementRule`
and `DynamicKeywordStatementRule` in the pinned parser.

Resource grouping covers Microsoft's standard Windows/PSDscResources and LCM
keywords. `File` and `Node` remain ordinary commands outside configurations;
`Script` resource property bags also use ordinary PowerShell syntax. Arbitrary
module-provided keyword registries are outside this fixed syntax policy. The
oracle supplies scoped keyword metadata without loading resource modules or
running their parsing hooks. The registry construction is defined by
[`CimDSCParser.CreateKeywordFromCimClass`](https://github.com/PowerShell/PowerShell/blob/v7.5.4/src/System.Management.Automation/DscSupport/CimDSCParser.cs).

Legacy `parallel_statement` and `sequence_statement` nodes each expose a
`statement_block` as their `body`. Tests distinguish their syntax from
PowerShell Core's workflow-support diagnostics.
Within a workflow, `InlineScript` is a command with a required script-block
argument; a newline may precede that block and later arguments remain part of
the same command. It is an ordinary command name outside workflows. Nested
functions and filters clear the workflow context for their bodies and restore
it on exit; ordinary script blocks inherit their enclosing context. These
boundaries follow `InlineScriptRule` and `FunctionDeclarationRule`.



Assembly-qualified types keep the assembly spelling in one `assembly_name`.
Commas separate metadata properties, each of which can contain one equals sign;
spaces, including .NET Unicode separators, are allowed around those separators.
Opening brackets remain literal assembly text. These lexical boundaries follow
`GetAssemblyNameSpec` in the pinned tokenizer; assembly resolution happens later.

A `using_statement` exposes `kind`, an optional introduced `alias`, and one
complete `value`. Values beginning with unquoted text use a `command_argument`, so `using namespace System."Text"`
keeps the literal and quoted fragments together. A newline or semicolon separates
a using declaration from the next statement. Quoted values, including here
strings, must be constant; interpolation retains an error. Namespace, type,
command, and module alias forms preserve both names, including module
specifications written as hashtables. These rules follow Microsoft's
[`about_Using`](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_using)
and `UsingStatementRule` in the pinned parser.

A `pipeline` exposes complete `elements`: a `command`, or a `command_expression`
with `expression` and optional `redirections`. Pipes connect these elements.
`pipeline_chain` instead exposes `left`, `operator`, and `right`, so
`a | b || c | d && e` groups as `((a | b) || (c | d)) && e`.
The chain operators have equal precedence and associate left, as described in
[Microsoft's pipeline-chain documentation](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_pipeline_chain_operators#operator-syntax-and-precedence).
`background_pipeline` wraps the complete pipeline or chain in its `pipeline`
field and retains the trailing `&` in `operator`.
A line beginning with `|` or `||` continues the preceding pipeline when no blank
line intervenes; comment-only lines are allowed. `&&` must follow the left
pipeline before a newline. These boundaries follow `IsPipeContinuation` in the
pinned tokenizer.

An `assignment_statement` is a statement with `left`, `operator`, and `right`;
its right side can be another assignment, a pipeline chain, or a control
statement. In `$result = a && b & next`, the assignment's right side is a
`background_pipeline`, and `next` starts another statement. The concrete
assignment includes `&`, while PowerShell's assignment and pipeline-chain AST
ranges end before that marker. The oracle comparison accounts for this source
boundary explicitly.

Flow control uses distinct `return_statement`, `throw_statement`, `exit_statement`,
`break_statement`, and `continue_statement` nodes. The first three expose an
optional `value`; `break` and `continue` expose an optional `label`. Queries can
select each statement's operand by role.
Command parameters consistently expose their complete spelling in `name`,
including the `--` marker that ends parameter recognition.
In `& -- arg`, the invocation's `command` field is that `command_parameter`.
Parameter-shaped literal values keep the tokenizer's boundaries: in
`cmd -- -foo.bar`, `-foo` and `.bar` are separate arguments. Dollars and backticks
inside such a parameter token remain literal. These rules follow
[`ScanParameter`](https://github.com/PowerShell/PowerShell/blob/v7.5.4/src/System.Management.Automation/engine/parser/tokenizer.cs#L3185-L3336)
and [`GetCommandArgument`](https://github.com/PowerShell/PowerShell/blob/v7.5.4/src/System.Management.Automation/engine/parser/Parser.cs#L6295-L6440).
At statement entry, `from`, `define`, and `var` are reserved. They remain valid
command names in pipeline elements and control-header expressions. An initial
`!` can also belong to a literal command name, as in `!name` or `!0x1`; its
numeric lookahead follows `NextToken`. A merging-redirection token at command
position retains its fixed boundary: `2>&1a` has command name `2>&1` and argument
`a`, while `11>& next` retains an error for its missing redirection destination.

Token-leading backtick-escaped horizontal whitespace is skipped by
[`NextToken`](https://github.com/PowerShell/PowerShell/blob/v7.5.4/src/System.Management.Automation/engine/parser/tokenizer.cs#L4688-L4705);
the same escape within a word remains part of that word. Inside a generic word,
backtick escapes only the CR of a CRLF pair, so the remaining LF ends the command.
A token-leading continuation consumes the complete CRLF pair.

The [stop-parsing token](https://learn.microsoft.com/en-us/powershell/module/microsoft.powershell.core/about/about_parsing?view=powershell-7.5#the-stop-parsing-token)
starts `verbatim_arguments`, containing an anonymous `--%` marker and an optional
`text` field with one `string_content` node. For example, in
`native --% "$name|literal" && next`, the quoted pipe and `$name` stay in the
raw text, while `&&` starts a pipeline chain. Straight and smart double quotes
toggle quote state; single quotes and backticks remain literal. Outside double
quotes, a pipe or `&&` ends the raw text. A physical newline always ends it,
including inside an unmatched double quote. These boundaries follow
`GetVerbatimCommandArgument` in the pinned tokenizer.

The text field excludes leading whitespace after the marker and retains trailing
whitespace before its boundary. When the tail is empty, the CST ends at the marker
and omits `text`; PowerShell's AST instead adds an empty command argument after
skipping whitespace. Oracle comparisons account for that synthetic argument.
When `--%` itself is the target of `&` or `.`, the marker is a `command_argument`
and its opaque tail is a separate argument, matching PowerShell's command elements.

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

A numeric-looking prefix followed by quotes or interpolation is also part of one
argument: `Write-Output 1"$name"` contains one composite `command_argument`.
At command position, `1.${name}` is one literal `command_name`, with the braced
spelling inside its `bare_word`. `1+${name}` remains arithmetic; `cmd 1.${name}`
contains an interpolated argument.
Argument mode treats `<` and `>` as literal characters after a generic word,
simple variable, or parameter name. Thus `native $name>out` has one argument,
while `native $name.Length>out` has a member-access argument and a redirection.

In `Write-Output $HOME.Length-more`, the member-access argument ends before the
`-more` parameter. Expandable here strings expose
`variable`, `interpolation`, `string_content`, and `escape_sequence` children in the
same way as ordinary expandable strings, including literal whitespace.

Inside expandable strings, PowerShell first collects the text of `$()` and then
parses it. The collector counts physical parentheses, including those in quotes
or comments. In ordinary strings it also reduces `""` and `` `" `` to one `"`
before parsing the collected text; here strings preserve those pairs. The CST
retains the original spelling and nested expressions. For example, `"$(""x"")"`
contains an inner `expandable_string` whose delimiters each span two raw quotes.
Straight and smart double quotes use the same anonymous `"` node kind.
These rules follow
[`ScanSubExpression`](https://github.com/PowerShell/PowerShell/blob/v7.5.4/src/System.Management.Automation/engine/parser/tokenizer.cs#L2362-L2447).

Expandable strings and composite command arguments contain `interpolation` nodes
for collected `$()` text. Each has one `expression` field: a `sub_expression`,
`member_access`, `element_access`, or `invocation_expression`. The interpolation
spans the complete physical collector, while its expression retains the nested
parser's source range. Text PowerShell ignores after that expression creates no
additional expression children. Standalone `$()` remains a `sub_expression`.
This distinction follows
[`ParseNestedExpressions`](https://github.com/PowerShell/PowerShell/blob/v7.5.4/src/System.Management.Automation/engine/parser/Parser.cs#L7621-L7669),
which parses one primary expression from each collected token.

Increment and decrement form `unary_expression` nodes with complete `++` and
`--` operator tokens. PowerShell checks whether the operand can be assigned in
[`SemanticChecks.VisitUnaryExpression`](https://github.com/PowerShell/PowerShell/blob/v7.5.4/src/System.Management.Automation/engine/parser/SemanticChecks.cs#L891-L907),
after parsing. Thus `1.++` has a complete tree and reports
`OperatorRequiresVariableOrProperty`; `1.++$x` is a syntax error because another
expression follows without a statement separator.

`UnaryExpressionRule` applies one optional postfix operator after each recursive
unary parse. In `$obj.[int].Name++++`, the first increment belongs to the complete
member operand `[int].Name`, and the second increments `$obj.[int].Name++`.
Contiguous type and attribute prefixes form one list before that optional postfix
is considered. Attributed operands retain their complete expression, including
`[A()]$x.Name`; `SemanticChecks.VisitAttributedExpression` validates the target
after the AST has been constructed.

Numeric conversion limits also determine tokenization. For example, `128y` is
an invalid signed-byte literal in an expression, while `cmd 128y` has a literal
text argument. The scanner checks integer ranges, binary and hexadecimal sign
extension, decimal bounds, multipliers, and the round-to-even conversion of real
values to integer suffixes. These follow `tokenizer.TryGetNumberValue` and
[`Utils.AsBigInt`](https://github.com/PowerShell/PowerShell/blob/v7.5.4/src/System.Management.Automation/engine/Utils.cs).
An adjacent sign participates in numeric conversion, so `-128y` is valid while
`- 128y` retains an invalid positive literal. Member names and hashtable keys
use the parser's distinct initial lookahead for unary plus and minus.
Named operators remain available after type literals: `[int] -or $x` is binary,
while `[int] -not $x` and `[int] -csplit $x` are casts over unary expressions.

Operator grouping follows the pinned parser. For example,
`$true -or $false -and $false` groups as `($true -or $false) -and $false`, and
`1 -eq 2 -shl 3` groups as `(1 -eq 2) -shl 3`. Null coalescing binds above comparison
and below addition. An adjacent numeric sign forms a `unary_expression` before
member access: in `-1e-2.ToString()`, the call's object is `-1e-2`. A spaced sign
instead applies to the complete operand. Numeric-looking command arguments still
follow command-mode token boundaries.
At the beginning of a method argument, parameter default, index, or member name,
the tokenizer uses a different numeric lookahead. Thus `[T]::M(1? 2 : 3)` is
invalid, while `[T]::M(1 ? 2 : 3)` and `[T]::M(1 + 2? 3 : 4)` are valid.
These boundaries follow
[`ExpressionRule`](https://github.com/PowerShell/PowerShell/blob/v7.5.4/src/System.Management.Automation/engine/parser/Parser.cs#L6696-L6786)
and [`UnaryExpressionRule`](https://github.com/PowerShell/PowerShell/blob/v7.5.4/src/System.Management.Automation/engine/parser/Parser.cs#L7062-L7085).

PowerShell 7.5.4 reports a speculative unterminated-string diagnostic for `-1#"`,
although its final tokens and AST contain a signed number followed by a line
comment. The CST treats the quote as comment text. A dedicated regression checks
the oracle's diagnostic and its final number/comment boundaries; ordinary syntax
comparisons remain strict. The tokenizer's
[`Resync`](https://github.com/PowerShell/PowerShell/blob/v7.5.4/src/System.Management.Automation/engine/parser/tokenizer.cs#L1031-L1099)
restores token state after speculative parsing.

Braced variables can contain supplementary Unicode characters:
`${𐐀}` is a variable, while `$𐐀` is a command token.

Postfix `++` and `--` can follow whitespace or comments after their operand.
Ordinary parentheses can contain a pipeline, pipeline chain, or assignment, with
optional surrounding newlines. Semicolons belong in statement-list constructs
such as `$()` instead.
These rules follow
[`PrimaryExpressionRule`](https://github.com/PowerShell/PowerShell/blob/v7.5.4/src/System.Management.Automation/engine/parser/Parser.cs#L7194-L7220)
and [`ParenthesizedExpressionRule`](https://github.com/PowerShell/PowerShell/blob/v7.5.4/src/System.Management.Automation/engine/parser/Parser.cs#L7573-L7618).

At the start of a script body, `param` begins a parameter block when the next
nontrivia token is `(`, including when that block is malformed. Otherwise it can
name a command, as in `param a`. Continuation keywords such as `else` and `catch`
also require a token boundary; a glued comment or quote can continue a generic
command word. These distinctions follow
[`ParamBlockRule`](https://github.com/PowerShell/PowerShell/blob/v7.5.4/src/System.Management.Automation/engine/parser/Parser.cs#L845-L879)
and [`ScanIdentifier`](https://github.com/PowerShell/PowerShell/blob/v7.5.4/src/System.Management.Automation/engine/parser/tokenizer.cs#L4402-L4458).

A leading semicolon ends parameter-block and using-declaration eligibility.
Thus `;param($x)` calls the command `param`, while `using namespace System;param($x)`
still has a parameter block following the using declaration. A switch permits
one exact, unescaped `default` clause; quoted and composite conditions such as
`'default'` and `default"suffix"` remain ordinary conditions. A splatted loop
variable is retained in the `foreach_statement.variable` field before the
oracle reports `SplattingNotPermitted`.

Method arguments, parameter defaults, attribute arguments, and hashtable keys
disable the comma operator. That setting also applies inside hashtable statement
values and their nested control-flow or declaration bodies. Parenthesized
expressions, array expressions, subexpressions, script-block expressions, and
index operands restore ordinary expression parsing. Command-mode comma lists
still form array arguments.

PowerShell's semantic AST includes implicit wrappers and combines some source
fragments. The CST preserves the original spelling and punctuation. PowerShell
performs name resolution and execution; tree-sitter provides syntax and recovery
for incomplete editor input.

## Checking the tree

[`scripts/oracle.ps1`](https://github.com/woodruffw-trough/tree-sitter-powershell/blob/main/scripts/oracle.ps1) calls
`System.Management.Automation.Language.Parser.ParseInput` and records diagnostics,
tokens, and AST ranges without running fixture scripts. Tests compare syntax
acceptance, command elements and parameter classification, expression grouping,
and corresponding declaration or control-flow ranges. PowerShell reports UTF-16
code-unit offsets; the Rust harness converts tree-sitter's UTF-8 byte offsets before
comparing them.

Focused fixtures can record exact diagnostic identifiers separately from syntax
acceptance. This checks the complete syntax that PowerShell retains when it
reports a semantic error, including invalid assignment targets and method
parameter blocks. Individual inputs copied from Microsoft's class and enum
tests retain their original file and line in the fixture's `upstream` field;
their Microsoft copyright and MIT license are retained in
[`tests/upstream`](../tests/upstream).

Corpus tests assert explicit trees. Focused fixtures and generated token matrices
cover context-dependent syntax, and complete project files exercise interactions
across larger scripts. Imported sources carry their provenance and hashes with the
fixture collection; see the [PowerShell fixtures](https://github.com/woodruffw-trough/tree-sitter-powershell/blob/main/tests/upstream/README.md) and
[external project fixtures](https://github.com/woodruffw-trough/tree-sitter-powershell/blob/main/tests/projects/README.md).
Contributor commands are in [CONTRIBUTING.md](../CONTRIBUTING.md).

Leading-pipe continuation is accepted inside interpolated subexpressions, including
values of `return`, `throw`, and `exit`. This deliberately differs from PowerShell
7.5.4: `"$(echo before` followed by a newline and `| next)"` produces
`EmptyPipeElement` there, while this grammar retains one pipeline. In the pinned
[`StartNestedScan`](https://github.com/PowerShell/PowerShell/blob/v7.5.4/src/System.Management.Automation/engine/parser/tokenizer.cs#L799-L819),
the tokenizer switches to the collected subexpression text but keeps absolute
source extents.
[`IsPipeContinuation`](https://github.com/PowerShell/PowerShell/blob/v7.5.4/src/System.Management.Automation/engine/parser/tokenizer.cs#L1360-L1434)
uses those offsets to index the shorter collected text. Consequently, inserting
text before `$(` can change whether an otherwise identical pipeline is accepted.
The grammar follows the intended continuation syntax consistently; dedicated
compatibility tests preserve the exact native diagnostics separately from
conformance checks.

Malformed operators leave following statement boundaries available for recovery.
For example, `$x++++` retains the valid `$x++` postfix expression and an `ERROR`
for the extra operator; commands on the next line keep their names, arguments,
and pipeline positions. Attribute-prefix lookahead preserves the same boundary
while allowing complete multiline expression and declaration prefixes.
