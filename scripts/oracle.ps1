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
$fixtureDirectory = Split-Path -Parent (Resolve-Path -LiteralPath $Fixtures).Path

function Get-AstExtent($Node) {
    return [ordered]@{
        start = $Node.Extent.StartOffset
        end = $Node.Extent.EndOffset
    }
}

function Add-AstField($Fields, [string] $Name, $Value) {
    if ($Value -is [Management.Automation.Language.Ast] -or
        $Value -is [Management.Automation.Language.ITypeName]) {
        $Fields[$Name] = Get-AstExtent $Value
    } elseif ($Value -is [Collections.IEnumerable]) {
        $Fields[$Name] = @($Value | ForEach-Object {
            if ($_ -is [Management.Automation.Language.Ast] -or
                $_ -is [Management.Automation.Language.ITypeName]) {
                Get-AstExtent $_
            }
        })
    }
}

function Get-AstRecord($Node) {
    $record = [ordered]@{
        type = $Node.GetType().Name
        start = $Node.Extent.StartOffset
        end = $Node.Extent.EndOffset
    }
    $fields = [ordered]@{}
    switch ($record.type) {
        'BinaryExpressionAst' {
            Add-AstField $fields 'left' $Node.Left
            Add-AstField $fields 'right' $Node.Right
            $record.operator = $Node.Operator.ToString()
        }
        'AssignmentStatementAst' {
            Add-AstField $fields 'left' $Node.Left
            Add-AstField $fields 'right' $Node.Right
            $record.operator = $Node.Operator.ToString()
        }
        'UnaryExpressionAst' {
            Add-AstField $fields 'argument' $Node.Child
            $record.operator = $Node.TokenKind.ToString()
        }
        'TernaryExpressionAst' {
            Add-AstField $fields 'condition' $Node.Condition
            Add-AstField $fields 'consequence' $Node.IfTrue
            Add-AstField $fields 'alternative' $Node.IfFalse
        }
        'IndexExpressionAst' {
            Add-AstField $fields 'object' $Node.Target
            Add-AstField $fields 'index' $Node.Index
            $record.nullConditional = $Node.NullConditional
        }
        { $_ -in 'MemberExpressionAst', 'InvokeMemberExpressionAst' } {
            Add-AstField $fields 'object' $Node.Expression
            Add-AstField $fields 'member' $Node.Member
            $record.static = $Node.Static
            $record.nullConditional = $Node.NullConditional
            if ($record.type -eq 'InvokeMemberExpressionAst') {
                Add-AstField $fields 'arguments' $Node.Arguments
                Add-AstField $fields 'genericArguments' $Node.GenericTypeArguments
            }
        }
        'ConvertExpressionAst' {
            Add-AstField $fields 'type' $Node.Type
            Add-AstField $fields 'value' $Node.Child
        }
        'AttributedExpressionAst' {
            Add-AstField $fields 'attribute' $Node.Attribute
            Add-AstField $fields 'value' $Node.Child
        }
        'ArrayLiteralAst' {
            Add-AstField $fields 'elements' $Node.Elements
        }
        'ParameterAst' {
            Add-AstField $fields 'name' $Node.Name
            Add-AstField $fields 'default' $Node.DefaultValue
        }
        'FunctionDefinitionAst' {
            Add-AstField $fields 'body' $Node.Body
            Add-AstField $fields 'parameters' $Node.Parameters
            $record.name = $Node.Name
            $record.synthetic = ($Node.Parent -is [Management.Automation.Language.FunctionMemberAst])
        }
        'FunctionMemberAst' {
            Add-AstField $fields 'body' $Node.Body
            Add-AstField $fields 'parameters' $Node.Parameters
            $record.name = $Node.Name
        }
        'TypeDefinitionAst' {
            Add-AstField $fields 'members' $Node.Members
            $record.name = $Node.Name
        }
        'PropertyMemberAst' {
            Add-AstField $fields 'value' $Node.InitialValue
            $record.name = $Node.Name
        }
        'IfStatementAst' {
            $fields.clauses = @($Node.Clauses | ForEach-Object {
                [ordered]@{
                    condition = Get-AstExtent $_.Item1
                    body = Get-AstExtent $_.Item2
                }
            })
            Add-AstField $fields 'else' $Node.ElseClause
        }
        { $_ -in 'ForEachStatementAst', 'ForStatementAst', 'WhileStatementAst',
            'DoWhileStatementAst', 'DoUntilStatementAst' } {
            Add-AstField $fields 'condition' $Node.Condition
            Add-AstField $fields 'body' $Node.Body
            if ($record.type -eq 'ForEachStatementAst') {
                Add-AstField $fields 'variable' $Node.Variable
            } elseif ($record.type -eq 'ForStatementAst') {
                Add-AstField $fields 'initializer' $Node.Initializer
                Add-AstField $fields 'iterator' $Node.Iterator
            }
        }
        'SwitchStatementAst' {
            Add-AstField $fields 'condition' $Node.Condition
            Add-AstField $fields 'default' $Node.Default
            $fields.clauses = @($Node.Clauses | ForEach-Object {
                [ordered]@{
                    condition = Get-AstExtent $_.Item1
                    body = Get-AstExtent $_.Item2
                }
            })
        }
        { $_ -in 'TryStatementAst', 'TrapStatementAst', 'CatchClauseAst', 'DataStatementAst' } {
            Add-AstField $fields 'body' $Node.Body
            if ($record.type -eq 'TryStatementAst') {
                Add-AstField $fields 'catches' $Node.CatchClauses
                Add-AstField $fields 'finally' $Node.Finally
            }
        }
        { $_ -in 'ReturnStatementAst', 'ThrowStatementAst', 'ExitStatementAst' } {
            Add-AstField $fields 'value' $Node.Pipeline
        }
        { $_ -in 'BreakStatementAst', 'ContinueStatementAst' } {
            Add-AstField $fields 'value' $Node.Label
        }
        'VariableExpressionAst' {
            $record.name = $Node.VariablePath.UserPath
            $record.splatted = $Node.Splatted
        }
        'ConstantExpressionAst' {
            $record.constantType = if ($null -eq $Node.Value) {
                'null'
            } else {
                $Node.Value.GetType().FullName
            }
            # An omitted attribute value is synthesized by PowerShell's parser.
            $record.synthetic = (
                $Node.Parent -is [Management.Automation.Language.NamedAttributeArgumentAst] -and
                $Node.Parent.ExpressionOmitted
            )
        }
        'StringConstantExpressionAst' {
            $record.stringKind = $Node.StringConstantType.ToString()
        }
        'ExpandableStringExpressionAst' {
            Add-AstField $fields 'expressions' $Node.NestedExpressions
            $record.stringKind = $Node.StringConstantType.ToString()
        }
    }
    if ($fields.Count -ne 0) {
        $record.fields = $fields
    }
    return $record
}

foreach ($case in $cases) {
    # Large upstream fixtures refer to unmodified, attributed source files.
    if (-not $case.ContainsKey('source') -and $case.ContainsKey('path')) {
        $case.source = [IO.File]::ReadAllText(
            (Join-Path $fixtureDirectory $case.path),
            [Text.Encoding]::UTF8
        )
    }
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
        Get-AstRecord $_
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
