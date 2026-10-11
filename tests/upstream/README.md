# Upstream integration fixtures

These six files are unmodified sources from Microsoft's
[PowerShell v7.5.4](https://github.com/PowerShell/PowerShell/tree/v7.5.4).
Paths below `powershell-7.5.4/` match their original repository paths.
They are distributed under the upstream [MIT license](LICENSE.txt), and retain
Microsoft's copyright notices. `SHA256SUMS` records the downloaded bytes;
`scripts/check-generated.sh` verifies them.

The integration test parses these complete scripts and modules with both
tree-sitter and the pinned PowerShell oracle. It compares command element and
binary expression boundaries. It never executes the upstream scripts.
