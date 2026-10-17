# Upstream integration fixtures

These seven files are unmodified sources from Microsoft's
[PowerShell v7.5.4](https://github.com/PowerShell/PowerShell/tree/v7.5.4).
Paths below `powershell-7.5.4/` match their original repository paths.
They are distributed under the upstream [MIT license](LICENSE.txt), and retain
Microsoft's copyright notices. `SHA256SUMS` records the downloaded bytes;
`scripts/check-generated.sh` verifies them.

The integration test parses these complete scripts and modules with both
tree-sitter and the pinned PowerShell oracle. It compares command elements and
mapped AST/CST fields, source ranges, and operator metadata. It never executes
the upstream scripts.

`parser_cases.json` also contains individual literal inputs from the Language
tests: parser test helpers, `Parser.ParseInput`, `ScriptBlock.Create`, and explicit
script/code test data. Each record identifies its original file, line, and input
role. These inputs retain Microsoft's MIT license
and copyright notice above. The tests check syntax, exact diagnostic identifiers,
command boundaries, and mapped tree structure for complete syntax.

`valid` records whether `ParseInput` reports any errors. An explicit
`syntax_valid` records complete syntax that receives a separate semantic
diagnostic, such as an invalid assignment target. The diagnostic remains checked;
the tree comparisons also check the syntax retained by Microsoft's parser.
