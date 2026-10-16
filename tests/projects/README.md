# Complete external project fixtures

These are complete, unmodified PowerShell source files from sixteen
independently maintained projects. The test harness parses them with tree-sitter and the pinned
PowerShell 7.5.4 parser; it never dot-sources or executes their code.

| Project and pinned revision | License | Files and coverage |
| --- | --- | --- |
| [Scoop](https://github.com/ScoopInstaller/Scoop/tree/e6aa3b366bdee8ed138c1e0f7b85192ebdd35d0f) | MIT or Unlicense | `lib/core.ps1`, `lib/install.ps1`, `libexec/scoop-update.ps1`: command-heavy package management, native command arguments, parallel script blocks, advanced functions, strings and pipelines. |
| [Pester](https://github.com/pester/Pester/tree/98e56d65da1642a7158992b50ded240c42a3ac18) | Apache-2.0 | `src/Pester.Runtime.ps1`, `src/functions/Mock.ps1`, `src/TypeClass.ps1`: large function bodies, runtime code generation, attributes, nested script blocks, splatting, and typed reflection calls. |
| [PSReadLine](https://github.com/PowerShell/PSReadLine/tree/2984546f62da9f63c31aba965ec3008fcb107024) | BSD-2-Clause | `PSReadLine/SamplePSReadLineProfile.ps1`, `tools/helper.psm1`, `tools/CheckHelp.ps1`: interactive profile functions, script-block method arguments, composite invocation names, module functions, classes, and help validation. |
| [posh-git](https://github.com/dahlbyk/posh-git/tree/bbc5ac380018239e0ac70411a59f4e367ca87f2d) | MIT | `src/PoshGitTypes.ps1`, `src/GitPrompt.ps1`, `src/GitTabExpansion.ps1`: enums, overloaded constructors, class methods, interpolated prompts, nested expressions, and command completion. |
| [dbatools](https://github.com/dataplat/dbatools/tree/347fb1820030113278fb41911815febcd48f54d0) | MIT | `public/Invoke-DbaQuery.ps1`, `public/Restore-DbaDatabase.ps1`, `public/Write-DbaDbTableData.ps1`: advanced functions, SQL here-strings, typed data access, parameter sets, nested pipelines, and error handling. |
| [PSResourceGet](https://github.com/PowerShell/PSResourceGet/tree/2abcb8e1cb307ca3d65d051f13e6aa32b90035ba) | MIT | `buildtools.psm1`, `src/Microsoft.PowerShell.PSResourceGet.psm1`, `src/dsc/psresourceget.ps1`: module setup and build functions, enums, classes, typed methods, resource dispatch, and a top-level trap. |
| [Invoke-Build](https://github.com/nightroman/Invoke-Build/tree/16e0279db6dcc3845d0a550373e1f9f137f2f51b) | Apache-2.0 | `Invoke-Build.ps1`: dynamic parameters, braced and scoped variables, punctuation in function names, task dependencies, and script-block invocation. |
| [Chocolatey](https://github.com/chocolatey/choco/tree/c8659d49508d2a1dcb61ef3548f90552ff86c5c8) | Apache-2.0 | `nuspec/chocolatey/chocolatey/tools/chocolateysetup.psm1`: Windows installation functions, registry paths, native command arguments, and nested exception handling. |
| [PSScriptAnalyzer](https://github.com/PowerShell/PSScriptAnalyzer/tree/e14c382a7c2ad0883e98aee611eee9b875d8bb30) | MIT | `Utils/RuleMaker.psm1`: C# here-string templates, XML member access, method calls, and source-file generation. |
| [PowerShellForGitHub](https://github.com/microsoft/PowerShellForGitHub/tree/fd4fa65f23ca30f18f85312a85348e660c287a9c) | MIT | `GitHubGraphQl.ps1`: advanced API functions, parameter sets, GraphQL examples, hashtables, and splatting. |
| [DscResource.Common](https://github.com/dsccommunity/DscResource.Common/tree/2430c508e03908cc3d08705d87a915006302f3e3) | MIT | `source/Public/Compare-DscParameterState.ps1`: typed parameters, recursive comparison, splatting, arrays, hashtables, and multiline conditions. |
| [oh-my-posh](https://github.com/JanDeDobbeleer/oh-my-posh/tree/913014b4abdb5c6be2fcd6e759d242168fa390e8) | MIT | `src/shell/scripts/omp.ps1`: scoped functions, module script blocks, native arguments, callbacks, and interpolated prompt strings. |
| [ImportExcel](https://github.com/dfinke/ImportExcel/tree/5387c061461c811d106b125588f064c4739342cb) | Apache-2.0 | `Public/Export-Excel.ps1`: pipeline functions, validation attributes, typed method calls, multidimensional indexing, and spaced postfix increments. |
| [Pode](https://github.com/Badgerati/Pode/tree/89797a301afec7611c55cd395f841fe78812ae01) | MIT | `src/Private/Authentication.ps1`: nested authentication callbacks, member and index access, interpolated URLs, C# here-strings, and exception handling. |
| [PSWriteHTML](https://github.com/EvotecIT/PSWriteHTML/tree/e3eb7a7ec7cc580f32ab9563990fefefcdb9d740) | MIT | `Public/New-HTMLTable.ps1`: stacked parameter attributes, nested HTML-generation script blocks, composite strings, splatting, and collection manipulation. |
| [Plaster](https://github.com/PowerShell/Plaster/tree/104eec83b857766f22f668476c16c2f6793b4874) | MIT | `Plaster/Public/Invoke-Plaster.ps1`: dynamic parameters, generic collections, XML and JSON member access, named blocks, and exception handling. |

Each project retains its original license file. Pester's copyright notice and
third-party notices are retained, including the upstream copy of the full
Apache-2.0 terms at its original path. Chocolatey's copyright notice is also
retained. These snapshots remain covered by their upstream licenses; the grammar
repository's license does not replace them.

`manifest.json` records every original path, repository, commit, Git blob ID,
SHA-256 digest, byte count, and source/notice role. `SHA256SUMS` covers the original
source and notice bytes. Git attributes disable line-ending conversion for the
sixteen snapshot directories, including their UTF-8 byte-order marks, CRLF
endings, and trailing whitespace. The Rust reader consumes one leading UTF-8 BOM
to match .NET's `ReadAllText` before passing source text to the two parsers.

The shared structural comparison in `../common/cst.rs` checks mapped node kinds and UTF-16 source
extents in both directions, then checks child fields and operator metadata. This
covers binary, unary, assignment and ternary grouping; member, method and index
access; generic method type arguments; casts and attributed variables; arrays,
hashtables and subexpressions; function, parameter, class
and enum declarations; branch and loop conditions and bodies; exception handling;
flow control; quoted and composite strings; variables and splatting. The shared
command comparison separately checks every command and direct command element,
including parameter classification.

The matcher accounts for these concrete/semantic representation differences:

- Tree-sitter retains condition parentheses, the member-access part of a method
  invocation, and quote fragments within composite command arguments. PowerShell
  exposes the condition pipeline, one invocation AST, and one full argument AST.
  Literal fragments within a bare argument have no independent string AST;
  interpolated arguments compare their full extent and nested expressions.
- PowerShell folds signed numeric literals into constants and represents unary
  comma as an array literal. The concrete tree retains the unary operator.
  A real literal such as `1.5l` can produce an integral value after suffix
  conversion, so the numeric comparison uses source extents rather than assuming
  that the semantic value's .NET type determines the lexical node kind.
- A class method also creates a synthetic function AST, and some constructors
  create a synthetic `$this` variable with the empty extent `(0, 0)`. Property
  declarations contain a concrete variable name but no variable-expression AST.
- PowerShell's class property and enum member extents include an optional
  terminating semicolon; the CST keeps it as a separate delimiter.
  Its parameter-block extent begins at `param`, excluding preceding attributes.
- A shorthand named attribute argument such as `[Parameter(Mandatory)]` creates
  an implicit true constant at the name's extent. The oracle's
  `NamedAttributeArgumentAst.ExpressionOmitted` property identifies it exactly.
- Type constraints and nested generic type names lack a type-expression AST.
  Implicit script-block, pipeline and command-expression wrappers are not treated
  as independently written syntax nodes.

These adjustments describe representations; actual argument lists, expression
grouping, statement bodies, names, string kinds, and variable/splat classification
are compared with the live parser.
