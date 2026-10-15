//! Compare semantic relationships while retaining tree-sitter's concrete nodes.
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use tree_sitter::Node;

type Extent = (usize, usize);
type Key = (String, Extent);

struct Source<'a> {
    text: &'a str,
    offsets: Vec<usize>,
    numeric_constants: BTreeSet<Extent>,
}

impl<'a> Source<'a> {
    fn new(text: &'a str, result: &Value) -> Self {
        let mut offsets = vec![0; text.len() + 1];
        let mut offset = 0;
        for (byte, character) in text.char_indices() {
            offsets[byte] = offset;
            offset += character.len_utf16();
        }
        offsets[text.len()] = offset;
        let numeric_constants = result["ast"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|node| node["type"] == "ConstantExpressionAst")
            .map(ast_extent)
            .collect();
        Self {
            text,
            offsets,
            numeric_constants,
        }
    }

    fn extent(&self, node: Node<'_>) -> Extent {
        (
            self.offsets[node.start_byte()],
            self.offsets[node.end_byte()],
        )
    }

    fn record(&self, node: Node<'_>) -> Value {
        let (mut start, _) = self.extent(node);
        let mut end = self.offsets[semantic_end_byte(node, self.text)];
        if node.kind() == "base_constructor_call" {
            let mut cursor = node.walk();
            let keyword = node
                .children(&mut cursor)
                .find(|child| child.kind() == "base")
                .unwrap();
            start = self.extent(keyword).0;
        }
        if matches!(node.kind(), "class_property" | "enum_member") {
            // PowerShell includes an immediately following semicolon in a
            // property declaration; the CST keeps that token as a delimiter.
            // Concrete siblings preserve comments and distinguish a newline
            // before a semicolon, which ends the declaration without the ';'.
            let mut next = node.next_sibling();
            let mut previous_end = node.end_byte();
            while let Some(sibling) = next {
                if self.text[previous_end..sibling.start_byte()].contains(['\r', '\n']) {
                    break;
                }
                if sibling.is_extra() {
                    previous_end = sibling.end_byte();
                    next = sibling.next_sibling();
                } else {
                    if sibling.kind() == ";" {
                        end = self.offsets[sibling.end_byte()];
                    }
                    break;
                }
            }
        }
        json!({"start": start, "end": end})
    }

    fn field(&self, node: Node<'_>, name: &str) -> Value {
        node.child_by_field_name(name)
            .map(|child| self.record(unwrap_condition(child)))
            .unwrap_or(Value::Null)
    }

    fn text(&self, node: Node<'_>) -> &'a str {
        &self.text[node.byte_range()]
    }

    fn collected_text(&self, node: Node<'_>) -> String {
        let mut layers = 0;
        let mut ancestor = node.parent();
        while let Some(parent) = ancestor {
            if parent.kind() == "sub_expression"
                && parent
                    .parent()
                    .is_some_and(|string| string.kind() == "expandable_string")
            {
                layers += 1;
            }
            ancestor = parent.parent();
        }
        let mut text = self.text(node).to_owned();
        for _ in 0..layers {
            let mut characters = text.chars().peekable();
            let mut collected = String::new();
            while let Some(character) = characters.next() {
                if matches!(character, '`' | '"' | '“' | '”' | '„')
                    && characters
                        .peek()
                        .is_some_and(|character| double_quote(*character))
                {
                    collected.push(characters.next().unwrap());
                } else {
                    collected.push(character);
                }
            }
            text = collected;
        }
        text
    }

    fn excerpt(&self, extent: Extent) -> String {
        let units = self
            .text
            .encode_utf16()
            .skip(extent.0)
            .take(extent.1 - extent.0)
            .collect::<Vec<_>>();
        String::from_utf16_lossy(&units).chars().take(160).collect()
    }
}

fn semantic_end_byte(node: Node<'_>, source: &str) -> usize {
    if node.kind() == "background_pipeline" {
        return semantic_end_byte(node.child_by_field_name("pipeline").unwrap(), source);
    }
    if node.kind() == "command" {
        let mut cursor = node.walk();
        let empty_verbatim = node.named_children(&mut cursor).any(|child| {
            child.kind() == "verbatim_arguments"
                && child.child_by_field_name("text").is_none()
                && child.end_byte() == node.end_byte()
        });
        let verbatim_target = node
            .child_by_field_name("name")
            .filter(|name| name.kind() == "invocation_operator_command")
            .and_then(|name| name.child_by_field_name("command"))
            .is_some_and(|target| {
                target.child(0).is_some_and(|marker| marker.kind() == "--%")
                    && target.end_byte() == node.end_byte()
            });
        if empty_verbatim || verbatim_target {
            // ScanVerbatimArgument skips leading horizontal whitespace even
            // when the AST's final argument has an empty source extent.
            let tail = &source[node.end_byte()..];
            let remaining = tail.trim_start_matches(|character: char| {
                character.is_whitespace() && !matches!(character, '\r' | '\n')
            });
            return node.end_byte() + tail.len() - remaining.len();
        }
    }
    if node.kind() == "pipeline" {
        let mut cursor = node.walk();
        if let Some(element) = node.children_by_field_name("elements", &mut cursor).last() {
            return semantic_end_byte(element, source);
        }
    }
    let field = match node.kind() {
        "pipeline_chain" => "right",
        "assignment_statement" => "right",
        "return_statement" | "throw_statement" | "exit_statement" => "value",
        _ => return node.end_byte(),
    };
    // Parser.cs computes assignment/flow extents from the semantic RHS, whose
    // background extent excludes '&'. Preserve enclosing ')' and '}' tokens.
    node.child_by_field_name(field)
        .filter(|child| child.end_byte() == node.end_byte())
        .map(|child| semantic_end_byte(child, source))
        .unwrap_or_else(|| node.end_byte())
}

fn named_children(node: Node<'_>) -> Vec<Node<'_>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .filter(|child| !matches!(child.kind(), "comment" | "line_continuation"))
        .collect()
}

fn children_of_kind<'a>(node: Node<'a>, kind: &str) -> Vec<Node<'a>> {
    named_children(node)
        .into_iter()
        .filter(|child| child.kind() == kind)
        .collect()
}

fn field_children<'a>(node: Node<'a>, field: &str) -> Vec<Node<'a>> {
    let mut cursor = node.walk();
    node.children_by_field_name(field, &mut cursor).collect()
}

fn unwrap_condition(node: Node<'_>) -> Node<'_> {
    if node.kind() == "condition" {
        named_children(node)[0]
    } else {
        node
    }
}

fn is_invocation_target(node: Node<'_>) -> bool {
    node.parent().is_some_and(|parent| {
        parent.kind() == "invocation_expression"
            && parent.child_by_field_name("function") == Some(node)
    })
}

fn folded_numeric(node: Node<'_>, source: &Source<'_>) -> bool {
    if node.kind() != "unary_expression" {
        return false;
    }
    let Some(operator) = node.child_by_field_name("operator") else {
        return false;
    };
    let Some(argument) = node.child_by_field_name("argument") else {
        return false;
    };
    // PowerShell folds adjacent signs only when its current lexer mode allows
    // signed numbers. A cast can leave '+' as a unary operator after lookahead.
    // Normalize only the exact constant extent reported by the parser.
    source.numeric_constants.contains(&source.extent(node))
        && matches!(source.text(operator), "+" | "-" | "–" | "—" | "―")
        && matches!(argument.kind(), "integer_literal" | "real_literal")
        && operator.end_byte() == argument.start_byte()
}

fn composite_string(node: Node<'_>) -> bool {
    if node.kind() != "command_argument" {
        return false;
    }
    let children = named_children(node);
    children.len() > 1
        && children.iter().any(|child| {
            matches!(
                child.kind(),
                "variable" | "sub_expression" | "expandable_string" | "single_quoted_string"
            )
        })
}

fn quoted_fragment(node: Node<'_>) -> bool {
    node.parent().is_some_and(composite_string)
}

fn has_interpolation(node: Node<'_>) -> bool {
    named_children(node)
        .into_iter()
        .any(|child| match child.kind() {
            "variable" | "sub_expression" => true,
            "expandable_string" => has_interpolation(child),
            _ => false,
        })
}

fn category<'a>(node: Node<'a>, source: &Source<'_>) -> Option<&'static str> {
    Some(match node.kind() {
        "pipeline" => {
            // AssignmentStatementAst's constructor removes only a non-background
            // pipeline containing one CommandExpressionAst from its direct RHS.
            let elements = field_children(node, "elements");
            if elements.len() == 1
                && elements[0].kind() == "command_expression"
                && node.parent().is_some_and(|parent| {
                    parent.kind() == "assignment_statement"
                        && parent.child_by_field_name("right") == Some(node)
                })
            {
                return None;
            }
            "PipelineAst"
        }
        "pipeline_chain" => "PipelineChainAst",
        "command_expression" | "base_constructor_call" => "CommandExpressionAst",
        "binary_expression" => "BinaryExpressionAst",
        "assignment_statement" => "AssignmentStatementAst",
        "unary_expression" if folded_numeric(node, source) => "ConstantExpressionAst",
        "unary_expression"
            if node
                .child_by_field_name("operator")
                .is_some_and(|operator| source.text(operator) == ",") =>
        {
            "ArrayLiteralAst"
        }
        "unary_expression" => "UnaryExpressionAst",
        "integer_literal" | "real_literal"
            if node
                .parent()
                .is_some_and(|parent| folded_numeric(parent, source)) =>
        {
            return None;
        }
        "integer_literal" | "real_literal" => "ConstantExpressionAst",
        "ternary_expression" => "TernaryExpressionAst",
        "member_access" if is_invocation_target(node) => return None,
        "member_access" => "MemberExpressionAst",
        "invocation_expression" => "InvokeMemberExpressionAst",
        "element_access" => "IndexExpressionAst",
        "cast_expression" => "ConvertExpressionAst",
        "attributed_expression" => "AttributedExpressionAst",
        "array_literal_expression" => "ArrayLiteralAst",
        "array_expression" => "ArrayExpressionAst",
        "sub_expression" => "SubExpressionAst",
        "parenthesized_expression" => "ParenExpressionAst",
        "hashtable_expression" => "HashtableAst",
        "parameter" => "ParameterAst",
        "function_definition" => "FunctionDefinitionAst",
        "class_method" => "FunctionMemberAst",
        "class_property" | "enum_member" => "PropertyMemberAst",
        "class_definition" | "enum_definition" => "TypeDefinitionAst",
        "param_block" => "ParamBlockAst",
        "attribute" => "AttributeAst",
        "command_parameter" => "CommandParameterAst",
        "redirection" => {
            if node.child_by_field_name("operator")?.kind() == "file_redirection_operator" {
                "FileRedirectionAst"
            } else {
                "MergingRedirectionAst"
            }
        }
        "attribute_argument" if node.child_by_field_name("name").is_some() => {
            "NamedAttributeArgumentAst"
        }
        "using_statement" => "UsingStatementAst",
        "type_literal" => {
            if node.parent().is_some_and(|parent| {
                parent.child_by_field_name("type") == Some(node)
                    || matches!(
                        parent.kind(),
                        "type_name"
                            | "generic_type_arguments"
                            | "parameter"
                            | "class_property"
                            | "class_method"
                            | "class_definition"
                            | "trap_statement"
                            | "catch_clause"
                    )
            }) {
                return None;
            }
            "TypeExpressionAst"
        }
        "variable" => {
            if node.parent().is_some_and(|parent| {
                parent.kind() == "class_property"
                    && parent.child_by_field_name("name") == Some(node)
            }) {
                return None;
            }
            "VariableExpressionAst"
        }
        "splat_expression" => "VariableExpressionAst",
        "single_quoted_string"
        | "expandable_string"
        | "single_quoted_here_string"
        | "expandable_here_string"
            if !quoted_fragment(node) =>
        {
            "QuotedStringAst"
        }
        "command_argument" if composite_string(node) && has_interpolation(node) => {
            "CompositeStringAst"
        }
        "if_statement" => "IfStatementAst",
        "foreach_statement" => "ForEachStatementAst",
        "for_statement" => "ForStatementAst",
        "while_statement" => "WhileStatementAst",
        "do_statement" => {
            let mut cursor = node.walk();
            if node
                .children(&mut cursor)
                .any(|child| child.kind() == "until")
            {
                "DoUntilStatementAst"
            } else {
                "DoWhileStatementAst"
            }
        }
        "switch_statement" => "SwitchStatementAst",
        "try_statement" => "TryStatementAst",
        "catch_clause" => "CatchClauseAst",
        "trap_statement" => "TrapStatementAst",
        "data_statement" => "DataStatementAst",
        "return_statement" => "ReturnStatementAst",
        "throw_statement" => "ThrowStatementAst",
        "exit_statement" => "ExitStatementAst",
        "break_statement" => "BreakStatementAst",
        "continue_statement" => "ContinueStatementAst",
        _ => return None,
    })
}

fn collect<'a>(node: Node<'a>, source: &Source<'_>, nodes: &mut BTreeMap<Key, Node<'a>>) {
    if super::common::discarded_switch_file(node) {
        return;
    }
    if let Some(category) = category(node, source) {
        let mut extent = ast_extent(&source.record(node));
        if node.kind() == "param_block" {
            // ParamBlockAst's extent starts at param; its attributes are separate.
            let mut cursor = node.walk();
            if let Some(keyword) = node
                .children(&mut cursor)
                .find(|child| child.kind() == "param")
            {
                extent.0 = source.extent(keyword).0;
            }
        }
        let duplicate = nodes.insert((category.to_owned(), extent), node);
        assert!(
            duplicate.is_none(),
            "Duplicate concrete {category} at {extent:?}: {:?}",
            source.excerpt(extent)
        );
    }
    for child in named_children(node) {
        collect(child, source, nodes);
    }
}

fn ast_extent(node: &Value) -> Extent {
    (
        node["start"].as_u64().unwrap() as usize,
        node["end"].as_u64().unwrap() as usize,
    )
}

fn expected_category(node: &Value) -> Option<&str> {
    let category = node["type"].as_str().unwrap();
    if matches!(category, "PipelineAst" | "CommandExpressionAst") && node["synthetic"] == true {
        // The oracle identifies -file condition wrappers and implicit constructor
        // base calls by exact runtime predicates. Explicit source is still checked;
        // filename arguments are independently verified by field queries.
        return None;
    }
    if category == "ConstantExpressionAst" && node["synthetic"] == true {
        // Microsoft's ExpressionOmitted flag identifies the true constant
        // synthesized by a shorthand argument such as [Parameter(Mandatory)].
        return None;
    }
    if category == "VariableExpressionAst" && node["synthetic"] == true {
        // Base-constructor calls synthesize a receiver, including explicit calls
        // whose receiver inherits the source extent of the 'base' keyword.
        return None;
    }
    if matches!(
        category,
        "StringConstantExpressionAst" | "ExpandableStringExpressionAst"
    ) {
        return match node["stringKind"].as_str().unwrap() {
            "BareWord" if category == "ExpandableStringExpressionAst" => Some("CompositeStringAst"),
            "BareWord" => None,
            _ => Some("QuotedStringAst"),
        };
    }
    if category == "FunctionDefinitionAst" && node["synthetic"] == true {
        // FunctionMemberAst also owns a synthetic FunctionDefinitionAst beginning
        // at its name. The actual class method is compared as FunctionMemberAst.
        return None;
    }
    // These AST nodes are semantic wrappers without a concrete grammar node.
    if matches!(
        category,
        "ScriptBlockAst"
            | "StatementBlockAst"
            | "NamedBlockAst"
            | "ScriptBlockExpressionAst"
            | "CommandAst"
            | "TypeConstraintAst"
            | "UsingExpressionAst"
    ) {
        return None;
    }
    Some(category)
}

fn extents<'a>(nodes: impl IntoIterator<Item = Node<'a>>, source: &Source<'_>) -> Value {
    Value::Array(nodes.into_iter().map(|node| source.record(node)).collect())
}

fn clause(node: Node<'_>, source: &Source<'_>) -> Value {
    json!({"condition": source.field(node, "condition"), "body": source.field(node, "body")})
}

fn interpolation_expressions(node: Node<'_>, source: &Source<'_>, expressions: &mut Vec<Value>) {
    for child in named_children(node) {
        match child.kind() {
            "variable" | "sub_expression" => expressions.push(source.record(child)),
            "expandable_string" => interpolation_expressions(child, source, expressions),
            _ => {}
        }
    }
}

fn mapped_field(node: Node<'_>, category: &str, field: &str, source: &Source<'_>) -> Value {
    match (category, field) {
        ("UsingStatementAst", "name") => {
            let value = node.child_by_field_name("value").unwrap();
            if value.kind() == "hashtable_expression" {
                Value::Null
            } else {
                source.record(value)
            }
        }
        ("UsingStatementAst", "module") => {
            let value = node.child_by_field_name("value").unwrap();
            if value.kind() == "hashtable_expression" {
                source.record(value)
            } else {
                Value::Null
            }
        }
        ("PipelineAst", "elements") => extents(field_children(node, "elements"), source),
        ("CommandExpressionAst", "redirections") => {
            extents(field_children(node, "redirections"), source)
        }
        ("CommandExpressionAst", "expression") if node.kind() == "base_constructor_call" => {
            source.record(node)
        }
        ("CommandExpressionAst", "baseArguments") if node.kind() == "base_constructor_call" => {
            extents(
                named_children(node.child_by_field_name("arguments").unwrap()),
                source,
            )
        }
        ("InvokeMemberExpressionAst", "object" | "member") => {
            source.field(node.child_by_field_name("function").unwrap(), field)
        }
        ("InvokeMemberExpressionAst", "arguments") => {
            let arguments = node.child_by_field_name("arguments").unwrap();
            extents(
                if arguments.kind() == "argument_list" {
                    named_children(arguments)
                } else {
                    vec![arguments]
                },
                source,
            )
        }
        ("InvokeMemberExpressionAst", "genericArguments") => {
            let arguments = node
                .child_by_field_name("type_arguments")
                .map(|arguments| children_of_kind(arguments, "type_name"))
                .unwrap_or_default();
            extents(arguments, source)
        }
        ("ArrayLiteralAst", "elements") => {
            if node.kind() == "unary_expression" {
                extents(node.child_by_field_name("argument"), source)
            } else {
                extents(named_children(node), source)
            }
        }
        ("FunctionDefinitionAst" | "FunctionMemberAst" | "ParamBlockAst", "parameters") => {
            let parameters = node
                .child_by_field_name("parameters")
                .map(|parameters| children_of_kind(parameters, "parameter"))
                .unwrap_or_default();
            extents(parameters, source)
        }
        ("TypeDefinitionAst", "members") => extents(
            field_children(node.child_by_field_name("body").unwrap(), "members"),
            source,
        ),
        ("TypeDefinitionAst", "baseTypes") => extents(
            if node.kind() == "enum_definition" {
                field_children(node, "underlying_type")
            } else {
                field_children(node, "base_types")
            },
            source,
        ),
        (_, "attributes" | "types") => extents(field_children(node, field), source),
        ("FunctionMemberAst", "returnType") => source.field(node, "return_type"),
        ("PropertyMemberAst", "propertyType") => field_children(node, "types")
            .last()
            .map(|node| source.record(*node))
            .unwrap_or(Value::Null),
        ("AttributeAst", "positionalArguments" | "namedArguments") => extents(
            named_children(node.child_by_field_name("arguments").unwrap())
                .into_iter()
                .filter(|argument| {
                    argument.child_by_field_name("name").is_some() == (field == "namedArguments")
                }),
            source,
        ),
        ("ForEachStatementAst", "condition") => source.field(node, "value"),
        ("ForEachStatementAst", "throttleLimit") => field_children(node, "parameters")
            .into_iter()
            .filter_map(|parameter| parameter.child_by_field_name("value"))
            .next_back()
            .map(|value| source.record(value))
            .unwrap_or(Value::Null),
        ("ForStatementAst", "iterator") => source.field(node, "update"),
        ("TryStatementAst", "catches") => extents(field_children(node, "handlers"), source),
        ("TryStatementAst", "finally") => node
            .child_by_field_name("finally")
            .map(|finally| source.field(finally, "body"))
            .unwrap_or(Value::Null),
        ("IfStatementAst", "clauses") => {
            let mut clauses = vec![clause(node, source)];
            clauses.extend(
                field_children(node, "elseif")
                    .into_iter()
                    .map(|node| clause(node, source)),
            );
            Value::Array(clauses)
        }
        ("IfStatementAst", "else") => node
            .child_by_field_name("else")
            .map(|else_clause| source.field(else_clause, "body"))
            .unwrap_or(Value::Null),
        ("SwitchStatementAst", "condition") => {
            if let Some(condition) = node.child_by_field_name("condition") {
                source.record(unwrap_condition(condition))
            } else {
                // The parser preserves only the last repeated -file operand;
                // the CST keeps every complete filename for source consumers.
                field_children(node, "file")
                    .last()
                    .map(|file| source.record(*file))
                    .unwrap_or(Value::Null)
            }
        }
        ("SwitchStatementAst", "clauses") => Value::Array(
            field_children(node, "clauses")
                .into_iter()
                .filter(|node| node.child_by_field_name("condition").unwrap().kind() != "default")
                .map(|node| clause(node, source))
                .collect(),
        ),
        ("SwitchStatementAst", "default") => field_children(node, "clauses")
            .into_iter()
            .find(|node| node.child_by_field_name("condition").unwrap().kind() == "default")
            .map(|node| source.field(node, "body"))
            .unwrap_or(Value::Null),
        ("DataStatementAst", "supportedCommands") => {
            extents(field_children(node, "supported_commands"), source)
        }
        ("QuotedStringAst" | "CompositeStringAst", "expressions") => {
            let mut expressions = Vec::new();
            interpolation_expressions(node, source, &mut expressions);
            Value::Array(expressions)
        }
        _ => source.field(node, field),
    }
}

fn mapped_fields(category: &str) -> &'static [&'static str] {
    match category {
        "PipelineAst" => &["elements"],
        "PipelineChainAst" => &["left", "right"],
        "CommandExpressionAst" => &["expression", "redirections", "baseArguments"],
        "BinaryExpressionAst" | "AssignmentStatementAst" => &["left", "right"],
        "UnaryExpressionAst" => &["argument"],
        "TernaryExpressionAst" => &["condition", "consequence", "alternative"],
        "MemberExpressionAst" => &["object", "member"],
        "InvokeMemberExpressionAst" => &["object", "member", "arguments", "genericArguments"],
        "IndexExpressionAst" => &["object", "index"],
        "ConvertExpressionAst" => &["type", "value"],
        "AttributedExpressionAst" => &["attribute", "value"],
        "ArrayLiteralAst" => &["elements"],
        "ParameterAst" => &["name", "default", "types", "attributes"],
        "ParamBlockAst" => &["parameters", "attributes"],
        "AttributeAst" => &["positionalArguments", "namedArguments"],
        "CommandParameterAst" => &["value"],
        "FileRedirectionAst" => &["destination"],
        "FunctionDefinitionAst" => &["body", "parameters"],
        "FunctionMemberAst" => &["body", "parameters", "returnType", "attributes"],
        "TypeDefinitionAst" => &["members", "attributes", "baseTypes"],
        "PropertyMemberAst" => &["value", "propertyType", "attributes"],
        "IfStatementAst" => &["clauses", "else"],
        "ForEachStatementAst" => &["variable", "condition", "throttleLimit", "body"],
        "ForStatementAst" => &["initializer", "condition", "iterator", "body"],
        "WhileStatementAst" | "DoWhileStatementAst" | "DoUntilStatementAst" => {
            &["condition", "body"]
        }
        "SwitchStatementAst" => &["condition", "clauses", "default"],
        "TryStatementAst" => &["body", "catches", "finally"],
        "CatchClauseAst" => &["body", "types"],
        "TrapStatementAst" => &["body", "type"],
        "DataStatementAst" => &["body", "supportedCommands"],
        "UsingStatementAst" => &["name", "module"],
        "ReturnStatementAst" | "ThrowStatementAst" | "ExitStatementAst" => &["value"],
        "BreakStatementAst" | "ContinueStatementAst" => &["label"],
        "QuotedStringAst" | "CompositeStringAst" => &["expressions"],
        _ => &[],
    }
}

fn operator(node: Node<'_>, source: &Source<'_>) -> String {
    let operator = node.child_by_field_name("operator").unwrap();
    let text = source
        .text(operator)
        .replace(['–', '—', '―'], "-")
        .to_lowercase();
    let canonical = match text.as_str() {
        "+" => "Plus",
        "-" => "Minus",
        "*" => "Multiply",
        "/" => "Divide",
        "%" => "Rem",
        ".." => "DotDot",
        "??" => "QuestionQuestion",
        "=" => "Equals",
        "+=" => "PlusEquals",
        "-=" => "MinusEquals",
        "*=" => "MultiplyEquals",
        "/=" => "DivideEquals",
        "%=" => "RemainderEquals",
        "??=" => "QuestionQuestionEquals",
        "&&" => "AndAnd",
        "||" => "OrOr",
        "!" => "Exclaim",
        "-f" => "Format",
        "-eq" => "Ieq",
        "-ne" => "Ine",
        "-lt" => "Ilt",
        "-le" => "Ile",
        "-gt" => "Igt",
        "-ge" => "Ige",
        "-like" => "Ilike",
        "-notlike" => "Inotlike",
        "-match" => "Imatch",
        "-notmatch" => "Inotmatch",
        "-replace" => "Ireplace",
        "-split" => "Isplit",
        "-contains" => "Icontains",
        "-notcontains" => "Inotcontains",
        "-in" => "Iin",
        "-notin" => "Inotin",
        "++" | "--" => {
            let postfix =
                operator.start_byte() > node.child_by_field_name("argument").unwrap().start_byte();
            return format!(
                "{}{}",
                if postfix { "Postfix" } else { "" },
                if text == "++" {
                    "PlusPlus"
                } else {
                    "MinusMinus"
                }
            );
        }
        _ => return text.trim_start_matches('-').to_owned(),
    };
    canonical.to_owned()
}

fn decode_backticks(text: &str) -> String {
    let mut characters = text.chars();
    let mut decoded = String::new();
    while let Some(character) = characters.next() {
        if character != '`' {
            decoded.push(character);
            continue;
        }
        let Some(character) = characters.next() else {
            // A trailing backtick in a generic name is retained literally.
            decoded.push('`');
            break;
        };
        if character == 'u' {
            assert_eq!(characters.next(), Some('{'));
            let mut codepoint = 0;
            for digit in characters.by_ref() {
                if digit == '}' {
                    break;
                }
                codepoint = codepoint * 16 + digit.to_digit(16).unwrap();
            }
            decoded.push(char::from_u32(codepoint).unwrap_or('\u{fffd}'));
        } else {
            decoded.push(match character {
                '0' => '\0',
                'a' => '\u{7}',
                'b' => '\u{8}',
                'e' => '\u{1b}',
                'f' => '\u{c}',
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                'v' => '\u{b}',
                _ => character,
            });
        }
    }
    decoded
}

fn double_quote(character: char) -> bool {
    matches!(character, '"' | '“' | '”' | '„')
}

fn single_quote(character: char) -> bool {
    matches!(character, '\'' | '‘' | '’' | '‚' | '‛')
}

fn decode_name(text: &str) -> String {
    let mut characters = text.chars().peekable();
    let mut quote = None;
    let mut decoded = String::new();
    while let Some(character) = characters.next() {
        if character == '`' && quote != Some('\'') {
            let mut escape = String::from("`");
            if let Some(character) = characters.next() {
                escape.push(character);
                if character == 'u' {
                    for character in characters.by_ref() {
                        escape.push(character);
                        if character == '}' {
                            break;
                        }
                    }
                }
            }
            decoded.push_str(&decode_backticks(&escape));
            continue;
        }
        let kind = if single_quote(character) {
            Some('\'')
        } else if double_quote(character) {
            Some('"')
        } else {
            None
        };
        if let Some(kind) = kind {
            if quote == Some(kind) {
                if characters.peek().is_some_and(|next| {
                    if kind == '\'' {
                        single_quote(*next)
                    } else {
                        double_quote(*next)
                    }
                }) {
                    decoded.push(characters.next().unwrap());
                } else {
                    quote = None;
                }
            } else if quote.is_none() {
                quote = Some(kind);
            } else {
                decoded.push(character);
            }
        } else {
            decoded.push(character);
        }
    }
    decoded
}

fn metadata(node: Node<'_>, category: &str, source: &Source<'_>) -> BTreeMap<&'static str, Value> {
    let mut result = BTreeMap::new();
    if category == "CommandExpressionAst" {
        result.insert(
            "baseConstructor",
            json!(node.kind() == "base_constructor_call"),
        );
    }
    if matches!(
        category,
        "BinaryExpressionAst"
            | "AssignmentStatementAst"
            | "UnaryExpressionAst"
            | "PipelineChainAst"
    ) {
        result.insert("operator", json!(operator(node, source).to_lowercase()));
    }
    if matches!(category, "PipelineAst" | "PipelineChainAst") {
        result.insert(
            "background",
            json!(
                node.parent()
                    .is_some_and(|parent| parent.kind() == "background_pipeline"
                        && parent.child_by_field_name("pipeline") == Some(node))
            ),
        );
    }
    if matches!(
        category,
        "MemberExpressionAst" | "InvokeMemberExpressionAst"
    ) {
        let member = if category == "InvokeMemberExpressionAst" {
            node.child_by_field_name("function").unwrap()
        } else {
            node
        };
        let operator = source.text(member.child_by_field_name("operator").unwrap());
        result.insert("static", json!(operator == "::"));
        result.insert("nullConditional", json!(operator == "?."));
    }
    if category == "IndexExpressionAst" {
        let target = node.child_by_field_name("object").unwrap();
        let suffix = &source.text[target.end_byte()..node.end_byte()];
        result.insert(
            "nullConditional",
            json!(suffix.trim_start().starts_with("?[")),
        );
    }
    if category == "VariableExpressionAst" {
        result.insert("splatted", json!(node.kind() == "splat_expression"));
        let text = source.collected_text(node);
        let name = text[1..]
            .strip_prefix('{')
            .and_then(|name| name.strip_suffix('}'))
            .unwrap_or(&text[1..]);
        let name = decode_backticks(name);
        let name = if name
            .get(..6)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("using:"))
        {
            &name[6..]
        } else {
            &name
        };
        result.insert("name", json!(name));
    }
    if matches!(
        category,
        "FunctionDefinitionAst" | "FunctionMemberAst" | "PropertyMemberAst" | "TypeDefinitionAst"
    ) {
        let name = source.collected_text(node.child_by_field_name("name").unwrap());
        let name = name.strip_prefix('$').unwrap_or(&name);
        result.insert("name", json!(decode_name(name)));
    }
    if category == "UsingStatementAst" {
        let kind = node.child_by_field_name("kind").unwrap();
        let text = source.text(kind).to_ascii_lowercase();
        let kind = match text.as_str() {
            "namespace" => "Namespace",
            "module" => "Module",
            "assembly" => "Assembly",
            _ => unreachable!("unknown using statement kind"),
        };
        result.insert("kind", json!(kind));
    }
    if category == "QuotedStringAst" {
        let kind = match node.kind() {
            "single_quoted_string" => "SingleQuoted",
            "expandable_string" => "DoubleQuoted",
            "single_quoted_here_string" => "SingleQuotedHereString",
            _ => "DoubleQuotedHereString",
        };
        result.insert("stringKind", json!(kind));
    }
    if category == "CompositeStringAst" {
        result.insert("stringKind", json!("BareWord"));
    }
    if category == "CommandParameterAst" {
        let name = node
            .child_by_field_name("name")
            .map(|name| source.text(name))
            .unwrap_or_else(|| source.text(node));
        let dash = name.chars().next().unwrap();
        assert!(matches!(dash, '-' | '–' | '—' | '―'));
        // ScanParameter consumes exactly the initial dash and keeps subsequent
        // characters literally, including dollars, backticks, and another dash.
        result.insert("name", json!(&name[dash.len_utf8()..]));
    }
    if matches!(category, "FileRedirectionAst" | "MergingRedirectionAst") {
        let operator = source.text(node.child_by_field_name("operator").unwrap());
        let stream = |symbol| match symbol {
            Some('2') => "Error",
            Some('3') => "Warning",
            Some('4') => "Verbose",
            Some('5') => "Debug",
            Some('6') => "Information",
            Some('*') => "All",
            _ => "Output",
        };
        result.insert("fromStream", json!(stream(operator.chars().next())));
        if category == "FileRedirectionAst" {
            result.insert("append", json!(operator.contains(">>")));
        } else {
            result.insert("toStream", json!(stream(operator.chars().last())));
        }
    }
    if matches!(category, "FunctionMemberAst" | "PropertyMemberAst") {
        let modifiers = field_children(node, "modifiers");
        for field in ["static", "hidden"] {
            result.insert(
                field,
                json!(
                    (field == "static" && node.kind() == "enum_member")
                        || modifiers
                            .iter()
                            .any(|modifier| source.text(*modifier).eq_ignore_ascii_case(field))
                ),
            );
        }
    }
    if category == "TypeDefinitionAst" {
        result.insert("isEnum", json!(node.kind() == "enum_definition"));
    }
    if matches!(
        category,
        "ForEachStatementAst"
            | "ForStatementAst"
            | "WhileStatementAst"
            | "DoWhileStatementAst"
            | "DoUntilStatementAst"
            | "SwitchStatementAst"
    ) {
        let label = node
            .child_by_field_name("label")
            .map(|label| decode_backticks(source.text(label.child_by_field_name("name").unwrap())));
        result.insert("label", json!(label));
    }
    if category == "DataStatementAst" {
        result.insert(
            "name",
            json!(
                node.child_by_field_name("name")
                    .map(|name| decode_backticks(source.text(name)))
            ),
        );
    }
    result
}

pub fn assert_structure(cases: &[Value], results: &[Value]) -> usize {
    let mut parser = super::common::parser();
    let mut failures = Vec::new();
    let mut field_checks = 0;
    for (case, result) in cases.iter().zip(results) {
        if case["valid"] != true {
            continue;
        }
        let text = case["source"].as_str().unwrap();
        let source = Source::new(text, result);
        let tree = parser.parse(text, None).unwrap();
        let mut actual = BTreeMap::new();
        collect(tree.root_node(), &source, &mut actual);
        let mut expected_keys = BTreeSet::new();
        for ast in result["ast"].as_array().unwrap() {
            let Some(category) = expected_category(ast) else {
                continue;
            };
            let key = (category.to_owned(), ast_extent(ast));
            let unique = expected_keys.insert(key.clone());
            assert!(
                unique,
                "Duplicate oracle {} at {:?} in {}",
                category, key.1, case["name"]
            );
            let Some(node) = actual.get(&key).copied() else {
                failures.push(format!(
                    "{}: missing {} at {:?}: {:?}",
                    case["name"],
                    category,
                    key.1,
                    source.excerpt(key.1)
                ));
                continue;
            };
            for field in mapped_fields(category) {
                field_checks += 1;
                let mut expected = ast["fields"][field].clone();
                let mut actual = mapped_field(node, category, field, &source);
                if category == "CommandExpressionAst" && *field == "redirections" {
                    // Parser.cs stores redirections in FromStream-indexed slots
                    // and filters that array when constructing the AST. Compare
                    // full source extents in source order; consumer-query tests
                    // independently require the CST's original source ordering.
                    for value in [&mut expected, &mut actual] {
                        if let Some(redirections) = value.as_array_mut() {
                            redirections.sort_by_key(ast_extent);
                        }
                    }
                }
                // Omitted null collections correspond to empty concrete
                // lists; unexpected children are still compared explicitly.
                if expected.is_null() && actual.as_array().is_some_and(Vec::is_empty) {
                    actual = Value::Null;
                }
                if actual != expected {
                    failures.push(format!(
                        "{}: {}.{} at {:?}: expected {}, got {}: {:?}",
                        case["name"],
                        category,
                        field,
                        key.1,
                        expected,
                        actual,
                        source.excerpt(key.1)
                    ));
                }
            }
            for (field, actual) in metadata(node, category, &source) {
                let expected = if field == "operator" {
                    json!(ast[field].as_str().unwrap().to_lowercase())
                } else {
                    ast[field].clone()
                };
                if actual != expected {
                    failures.push(format!(
                        "{}: {}.{} at {:?}: expected {}, got {}: {:?}",
                        case["name"],
                        category,
                        field,
                        key.1,
                        expected,
                        actual,
                        source.excerpt(key.1)
                    ));
                }
            }
        }
        for ((category, extent), _) in actual
            .iter()
            .filter(|(key, _)| !expected_keys.contains(*key))
        {
            failures.push(format!(
                "{}: unexpected {} at {:?}: {:?}",
                case["name"],
                category,
                extent,
                source.excerpt(*extent)
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{}",
        super::common::failure_summary(&failures)
    );
    field_checks
}
