# Parse fixture sources with Microsoft's parser without executing them.
# Extent offsets are UTF-16 code-unit offsets, as used by PowerShell/.NET.
[CmdletBinding()]
param(
    [string] $Fixtures = (Join-Path $PSScriptRoot '../tests/fixtures/oracle.json')
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$cases = ConvertFrom-Json -InputObject ([IO.File]::ReadAllText(
    (Resolve-Path -LiteralPath $Fixtures).Path,
    [Text.Encoding]::UTF8
)) -AsHashtable
$results = [Collections.Generic.List[object]]::new()
$names = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)

foreach ($case in $cases) {
    if ($case.name -isnot [string] -or [string]::IsNullOrWhiteSpace($case.name) -or
        $case.source -isnot [string] -or $case.valid -isnot [bool]) {
        throw 'Each fixture must have a nonempty string name, string source, and boolean valid.'
    }
    if (-not $names.Add($case.name)) {
        throw "Duplicate fixture name: $($case.name)"
    }

    $tokens = $null
    $parseErrors = $null
    $ast = [Management.Automation.Language.Parser]::ParseInput(
        $case.source,
        $case.name,
        [ref] $tokens,
        [ref] $parseErrors
    )

    $errors = @($parseErrors | ForEach-Object {
        [ordered]@{
            id = $_.ErrorId
            message = $_.Message
            start = $_.Extent.StartOffset
            end = $_.Extent.EndOffset
            incomplete = $_.IncompleteInput
        }
    })
    $nodes = @($ast.FindAll({ param($node) $true }, $true) | ForEach-Object {
        [ordered]@{
            type = $_.GetType().Name
            start = $_.Extent.StartOffset
            end = $_.Extent.EndOffset
        }
    })
    $tokenExtents = @($tokens | ForEach-Object {
        [ordered]@{
            kind = $_.Kind.ToString()
            text = $_.Text
            start = $_.Extent.StartOffset
            end = $_.Extent.EndOffset
        }
    })
    $commands = @($ast.FindAll({
        param($node)
        $node -is [Management.Automation.Language.CommandAst]
    }, $true) | ForEach-Object {
        [ordered]@{
            start = $_.Extent.StartOffset
            end = $_.Extent.EndOffset
            elements = @($_.CommandElements | ForEach-Object {
                [ordered]@{
                    type = $_.GetType().Name
                    start = $_.Extent.StartOffset
                    end = $_.Extent.EndOffset
                }
            })
        }
    })

    $results.Add([ordered]@{
        name = $case.name
        source = $case.source
        valid = ($errors.Count -eq 0)
        errors = $errors
        ast = $nodes
        tokens = $tokenExtents
        commands = $commands
    })
}

[ordered]@{
    powershellVersion = $PSVersionTable.PSVersion.ToString()
    offsetEncoding = 'utf-16'
    results = $results.ToArray()
} | ConvertTo-Json -Depth 12 -Compress
