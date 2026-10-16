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
    $end = $Node.Extent.EndOffset
    if ($Node -is [Management.Automation.Language.DynamicKeywordStatementAst] -and
        $dynamicKeywordDefinition.GetValue($Node).BodyMode -ne 'Command') {
        $end = $Node.CommandElements[2].Extent.EndOffset
    }
    return [ordered]@{
        start = $Node.Extent.StartOffset
        end = $end
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

function Test-SwitchFilePipeline($Node) {
    return (
        $Node -is [Management.Automation.Language.PipelineAst] -and
        $Node.Parent -is [Management.Automation.Language.SwitchStatementAst] -and
        ($Node.Parent.Flags -band [Management.Automation.Language.SwitchFlags]::File) -ne 0 -and
        [object]::ReferenceEquals($Node.Parent.Condition, $Node)
    )
}

function Get-AstRecord($Node) {
    $record = [ordered]@{
        type = $Node.GetType().Name
        start = $Node.Extent.StartOffset
        end = $Node.Extent.EndOffset
    }
    $fields = [ordered]@{}
    switch ($record.type) {
        'PipelineAst' {
            Add-AstField $fields 'elements' $Node.PipelineElements
            $record.background = $Node.Background
            $record.synthetic = Test-SwitchFilePipeline $Node
        }
        'PipelineChainAst' {
            Add-AstField $fields 'left' $Node.LhsPipelineChain
            Add-AstField $fields 'right' $Node.RhsPipeline
            $record.operator = $Node.Operator.ToString()
            $record.background = $Node.Background
        }
        'CommandExpressionAst' {
            Add-AstField $fields 'expression' $Node.Expression
            Add-AstField $fields 'redirections' $Node.Redirections
            $record.baseConstructor = (
                $Node.Expression -is [Management.Automation.Language.BaseCtorInvokeMemberExpressionAst]
            )
            if ($record.baseConstructor) {
                Add-AstField $fields 'baseArguments' $Node.Expression.Arguments
            }
            $record.synthetic = (
                (Test-SwitchFilePipeline $Node.Parent) -or
                ($Node.Extent.StartOffset -eq 0 -and $Node.Extent.EndOffset -eq 0 -and
                    $Node.Expression.GetType().Name -eq 'BaseCtorInvokeMemberExpressionAst')
            )
        }
        'DynamicKeywordStatementAst' {
            $elements = $Node.CommandElements
            Add-AstField $fields 'keyword' $elements[0]
            $definition = $dynamicKeywordDefinition.GetValue($Node)
            $record.bodyMode = $definition.BodyMode.ToString()
            if ($definition.BodyMode -eq 'Command') {
                Add-AstField $fields 'elements' @($elements | Select-Object -Skip 1)
            } else {
                if ($definition.NameMode -ne 'NoName') {
                    Add-AstField $fields 'name' $elements[1]
                }
                Add-AstField $fields 'body' $elements[2]
                # The semantic extent includes trivia before the next token.
                $record.end = $elements[2].Extent.EndOffset
            }
        }
        'HashtableAst' {
            $fields.entries = @($Node.KeyValuePairs | ForEach-Object {
                [ordered]@{
                    key = Get-AstExtent $_.Item1
                    value = Get-AstExtent $_.Item2
                }
            })
        }
        'BlockStatementAst' {
            Add-AstField $fields 'body' $Node.Body
            $record.kind = $Node.Kind.Kind.ToString()
        }
        'ConfigurationDefinitionAst' {
            Add-AstField $fields 'name' $Node.InstanceName
            Add-AstField $fields 'body' $Node.Body
            Add-AstField $fields 'attributes' $configurationAttributes.GetValue($Node)
        }
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
            Add-AstField $fields 'types' @($Node.Attributes | Where-Object {
                $_ -is [Management.Automation.Language.TypeConstraintAst]
            })
            Add-AstField $fields 'attributes' @($Node.Attributes | Where-Object {
                $_ -is [Management.Automation.Language.AttributeAst]
            })
        }
        'ParamBlockAst' {
            Add-AstField $fields 'parameters' $Node.Parameters
            Add-AstField $fields 'attributes' $Node.Attributes
        }
        'AttributeAst' {
            Add-AstField $fields 'positionalArguments' $Node.PositionalArguments
            Add-AstField $fields 'namedArguments' $Node.NamedArguments
        }
        'CommandParameterAst' {
            Add-AstField $fields 'value' $Node.Argument
            $record.name = $Node.ParameterName
        }
        'FileRedirectionAst' {
            Add-AstField $fields 'destination' $Node.Location
            $record.append = $Node.Append
            $record.fromStream = $Node.FromStream.ToString()
        }
        'MergingRedirectionAst' {
            $record.fromStream = $Node.FromStream.ToString()
            $record.toStream = $Node.ToStream.ToString()
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
            Add-AstField $fields 'returnType' $Node.ReturnType
            Add-AstField $fields 'attributes' $Node.Attributes
            $record.static = $Node.IsStatic
            $record.hidden = $Node.IsHidden
        }
        'TypeDefinitionAst' {
            Add-AstField $fields 'members' $Node.Members
            $record.name = $Node.Name
            Add-AstField $fields 'attributes' $Node.Attributes
            Add-AstField $fields 'baseTypes' $Node.BaseTypes
            $record.isEnum = $Node.IsEnum
        }
        'UsingStatementAst' {
            Add-AstField $fields 'name' $Node.Name
            Add-AstField $fields 'alias' $Node.Alias
            Add-AstField $fields 'module' $Node.ModuleSpecification
            $record.kind = $Node.UsingStatementKind.ToString()
        }
        'PropertyMemberAst' {
            Add-AstField $fields 'value' $Node.InitialValue
            $record.name = $Node.Name
            Add-AstField $fields 'propertyType' $Node.PropertyType
            Add-AstField $fields 'attributes' $Node.Attributes
            $record.static = $Node.IsStatic
            $record.hidden = $Node.IsHidden
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
            $record.label = $Node.Label
            if ($record.type -eq 'ForEachStatementAst') {
                Add-AstField $fields 'variable' $Node.Variable
                Add-AstField $fields 'throttleLimit' $Node.ThrottleLimit
            } elseif ($record.type -eq 'ForStatementAst') {
                Add-AstField $fields 'initializer' $Node.Initializer
                Add-AstField $fields 'iterator' $Node.Iterator
            }
        }
        'SwitchStatementAst' {
            $record.label = $Node.Label
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
            } elseif ($record.type -eq 'CatchClauseAst') {
                Add-AstField $fields 'types' $Node.CatchTypes
            } elseif ($record.type -eq 'TrapStatementAst') {
                Add-AstField $fields 'type' $Node.TrapType
            } elseif ($record.type -eq 'DataStatementAst') {
                Add-AstField $fields 'supportedCommands' $Node.CommandsAllowed
                $record.name = $Node.Variable
            }
        }
        { $_ -in 'ReturnStatementAst', 'ThrowStatementAst', 'ExitStatementAst' } {
            Add-AstField $fields 'value' $Node.Pipeline
        }
        { $_ -in 'BreakStatementAst', 'ContinueStatementAst' } {
            Add-AstField $fields 'label' $Node.Label
        }
        'VariableExpressionAst' {
            $record.name = $Node.VariablePath.UserPath
            $record.splatted = $Node.Splatted
            # Both implicit and explicit base calls synthesize their receiver.
            $record.synthetic = (
                $Node.Parent -is [Management.Automation.Language.BaseCtorInvokeMemberExpressionAst] -and
                [object]::ReferenceEquals($Node.Parent.Expression, $Node)
            )
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

$dynamicKeywordDefinition = [Management.Automation.Language.DynamicKeywordStatementAst].GetProperty(
    'Keyword', [Reflection.BindingFlags] 'Instance, NonPublic'
)
if ($null -eq $dynamicKeywordDefinition) {
    throw 'The pinned parser must expose DynamicKeywordStatementAst.Keyword.'
}

# The pinned parser stores configuration attributes outside its public visitor.
$configurationAttributes = [Management.Automation.Language.ConfigurationDefinitionAst].GetProperty(
    'CustomAttributes', [Reflection.BindingFlags] 'Instance, NonPublic'
)
if ($null -eq $configurationAttributes) {
    throw 'The pinned parser must expose ConfigurationDefinitionAst.CustomAttributes.'
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
    $hasDynamicKeywords = $case.ContainsKey('dynamic_keywords')
    if ($hasDynamicKeywords) {
        [Management.Automation.Language.DynamicKeyword]::Push()
    }
    try {
        if ($hasDynamicKeywords) {
            foreach ($metadata in $case.dynamic_keywords) {
                $keyword = [Management.Automation.Language.DynamicKeyword]::new()
                $keyword.Keyword = $metadata.keyword
                $keyword.BodyMode = [Management.Automation.Language.DynamicKeywordBodyMode] $metadata.body_mode
                $keyword.NameMode = [Management.Automation.Language.DynamicKeywordNameMode] $metadata.name_mode
                if ($metadata.ContainsKey('properties')) {
                    foreach ($name in $metadata.properties) {
                        $property = [Management.Automation.Language.DynamicKeywordProperty]::new()
                        $property.Name = $name
                        $keyword.Properties.Add($name, $property)
                    }
                }
                if ($metadata.ContainsKey('implementing_module')) {
                    $keyword.ImplementingModule = $metadata.implementing_module
                }
                if ($metadata.ContainsKey('meta_statement')) {
                    $keyword.MetaStatement = $metadata.meta_statement
                }
                [Management.Automation.Language.DynamicKeyword]::AddKeyword($keyword)
            }
        }
        $ast = [Management.Automation.Language.Parser]::ParseInput(
            $case.source,
            $case.name,
            [ref] $tokens,
            [ref] $parseErrors
        )
    } finally {
        if ($hasDynamicKeywords) {
            [Management.Automation.Language.DynamicKeyword]::Pop()
        }
    }

    $errors = @($parseErrors | ForEach-Object {
        [ordered]@{
            id = $_.ErrorId
            message = $_.Message
            start = $_.Extent.StartOffset
            end = $_.Extent.EndOffset
            incomplete = $_.IncompleteInput
        }
    })
    $astNodes = [Collections.Generic.List[Management.Automation.Language.Ast]]::new()
    $seenAstNodes = [Collections.Generic.HashSet[Management.Automation.Language.Ast]]::new()
    foreach ($node in $ast.FindAll({ param($node) $true }, $true)) {
        if ($seenAstNodes.Add($node)) {
            $astNodes.Add($node)
        }
    }
    # These AST visitors omit their header expressions. Visit the subtrees
    # explicitly so their argument boundaries and grouping are compared too.
    for ($index = 0; $index -lt $astNodes.Count; $index++) {
        $node = $astNodes[$index]
        $headerExpressions = if ($node -is [Management.Automation.Language.DataStatementAst]) {
            @($node.CommandsAllowed)
        } elseif ($node -is [Management.Automation.Language.ForEachStatementAst] -and
            $null -ne $node.ThrottleLimit) {
            @($node.ThrottleLimit)
        } elseif ($node -is [Management.Automation.Language.ConfigurationDefinitionAst]) {
            @($configurationAttributes.GetValue($node))
        } else {
            @()
        }
        foreach ($expression in $headerExpressions) {
            foreach ($child in $expression.FindAll({ param($node) $true }, $true)) {
                if ($seenAstNodes.Add($child)) {
                    $astNodes.Add($child)
                }
            }
        }
    }
    $nodes = @($astNodes | ForEach-Object {
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
    $commands = @($astNodes | Where-Object {
        $_ -is [Management.Automation.Language.CommandAst]
    } | ForEach-Object {
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
