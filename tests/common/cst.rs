//! Compare semantic relationships while retaining tree-sitter's concrete nodes.
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use tree_sitter::Node;

type Extent = (usize, usize);
type Key = (String, Extent);

struct Source<'a> {
    text: &'a str,
    offsets: Vec<usize>,
}

impl<'a> Source<'a> {
    fn new(text: &'a str) -> Self {
        let mut offsets = vec![0; text.len() + 1];
        let mut offset = 0;
        for (byte, character) in text.char_indices() {
            offsets[byte] = offset;
            offset += character.len_utf16();
        }
        offsets[text.len()] = offset;
        Self { text, offsets }
    }

    fn extent(&self, node: Node<'_>) -> Extent {
        (
            self.offsets[node.start_byte()],
            self.offsets[node.end_byte()],
        )
    }

    fn record(&self, node: Node<'_>) -> Value {
        let (start, mut end) = self.extent(node);
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

fn unwrap_condition(node: Node<'_>) -> Node<'_> {
    if node.kind() == "condition" {
        children_of_kind(node, "pipeline")[0]
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
    node.kind() == "unary_expression"
        && node
            .child_by_field_name("operator")
            .is_some_and(|operator| matches!(source.text(operator), "+" | "-" | "–" | "—" | "―"))
        && node
            .child_by_field_name("argument")
            .is_some_and(|argument| matches!(argument.kind(), "integer_literal" | "real_literal"))
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
        "flow_control_statement" => match node.child_by_field_name("keyword")?.kind() {
            "return" => "ReturnStatementAst",
            "throw" => "ThrowStatementAst",
            "exit" => "ExitStatementAst",
            "break" => "BreakStatementAst",
            "continue" => "ContinueStatementAst",
            _ => return None,
        },
        _ => return None,
    })
}

fn collect<'a>(node: Node<'a>, source: &Source<'_>, nodes: &mut BTreeMap<Key, Node<'a>>) {
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
    let extent = ast_extent(node);
    if category == "ConstantExpressionAst" && node["synthetic"] == true {
        // Microsoft's ExpressionOmitted flag identifies the true constant
        // synthesized by a shorthand argument such as [Parameter(Mandatory)].
        return None;
    }
    if category == "VariableExpressionAst" && extent == (0, 0) && node["name"] == "this" {
        // Class constructors synthesize a receiver variable without source text.
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
            | "PipelineAst"
            | "CommandExpressionAst"
            | "CommandAst"
            | "CommandParameterAst"
            | "TypeConstraintAst"
            | "UsingExpressionAst"
            | "FileRedirectionAst"
            | "MergingRedirectionAst"
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
        ("InvokeMemberExpressionAst", "object" | "member") => {
            source.field(node.child_by_field_name("function").unwrap(), field)
        }
        ("InvokeMemberExpressionAst", "arguments") => extents(
            named_children(node).into_iter().filter(|child| {
                Some(*child) != node.child_by_field_name("function")
                    && child.kind() != "generic_type_arguments"
            }),
            source,
        ),
        ("InvokeMemberExpressionAst", "genericArguments") => {
            let arguments = children_of_kind(node, "generic_type_arguments")
                .first()
                .map(|arguments| children_of_kind(*arguments, "type_name"))
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
        ("FunctionDefinitionAst" | "FunctionMemberAst", "parameters") => {
            let parameters = children_of_kind(node, "parameter_list")
                .first()
                .map(|parameters| children_of_kind(*parameters, "parameter"))
                .unwrap_or_default();
            extents(parameters, source)
        }
        ("TypeDefinitionAst", "members") => extents(
            named_children(node).into_iter().filter(|child| {
                matches!(
                    child.kind(),
                    "class_method" | "class_property" | "enum_member"
                )
            }),
            source,
        ),
        ("ForEachStatementAst", "condition") => source.field(node, "value"),
        ("ForStatementAst", "initializer" | "condition" | "iterator") => {
            // The concrete for header retains all three semicolon-delimited
            // slots, including empty slots, rather than assigning field names.
            let desired = match field {
                "initializer" => 0,
                "condition" => 1,
                _ => 2,
            };
            let mut slot = 0;
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == ";" {
                    slot += 1;
                } else if child.kind() == "pipeline" && slot == desired {
                    return source.record(child);
                }
            }
            Value::Null
        }
        ("TryStatementAst", "catches") => extents(children_of_kind(node, "catch_clause"), source),
        ("TryStatementAst", "finally") => children_of_kind(node, "finally_clause")
            .first()
            .map(|finally| source.field(*finally, "body"))
            .unwrap_or(Value::Null),
        ("IfStatementAst", "clauses") => {
            let mut clauses = vec![clause(node, source)];
            clauses.extend(
                children_of_kind(node, "elseif_clause")
                    .into_iter()
                    .map(|node| clause(node, source)),
            );
            Value::Array(clauses)
        }
        ("IfStatementAst", "else") => children_of_kind(node, "else_clause")
            .first()
            .map(|else_clause| source.field(*else_clause, "body"))
            .unwrap_or(Value::Null),
        ("SwitchStatementAst", "condition") => {
            if let Some(condition) = children_of_kind(node, "condition").first() {
                source.record(unwrap_condition(*condition))
            } else {
                source.field(node, "file")
            }
        }
        ("SwitchStatementAst", "clauses") => Value::Array(
            children_of_kind(node, "switch_clause")
                .into_iter()
                .filter(|node| node.child_by_field_name("condition").unwrap().kind() != "default")
                .map(|node| clause(node, source))
                .collect(),
        ),
        ("SwitchStatementAst", "default") => children_of_kind(node, "switch_clause")
            .into_iter()
            .find(|node| node.child_by_field_name("condition").unwrap().kind() == "default")
            .map(|node| source.field(node, "body"))
            .unwrap_or(Value::Null),
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
        "BinaryExpressionAst" | "AssignmentStatementAst" => &["left", "right"],
        "UnaryExpressionAst" => &["argument"],
        "TernaryExpressionAst" => &["condition", "consequence", "alternative"],
        "MemberExpressionAst" => &["object", "member"],
        "InvokeMemberExpressionAst" => &["object", "member", "arguments", "genericArguments"],
        "IndexExpressionAst" => &["object", "index"],
        "ConvertExpressionAst" => &["type", "value"],
        "AttributedExpressionAst" => &["attribute", "value"],
        "ArrayLiteralAst" => &["elements"],
        "ParameterAst" => &["name", "default"],
        "FunctionDefinitionAst" | "FunctionMemberAst" => &["body", "parameters"],
        "TypeDefinitionAst" => &["members"],
        "PropertyMemberAst" => &["value"],
        "IfStatementAst" => &["clauses", "else"],
        "ForEachStatementAst" => &["variable", "condition", "body"],
        "ForStatementAst" => &["initializer", "condition", "iterator", "body"],
        "WhileStatementAst" | "DoWhileStatementAst" | "DoUntilStatementAst" => {
            &["condition", "body"]
        }
        "SwitchStatementAst" => &["condition", "clauses", "default"],
        "TryStatementAst" => &["body", "catches", "finally"],
        "CatchClauseAst" | "TrapStatementAst" | "DataStatementAst" => &["body"],
        "ReturnStatementAst"
        | "ThrowStatementAst"
        | "ExitStatementAst"
        | "BreakStatementAst"
        | "ContinueStatementAst" => &["value"],
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

fn metadata(node: Node<'_>, category: &str, source: &Source<'_>) -> BTreeMap<&'static str, Value> {
    let mut result = BTreeMap::new();
    if matches!(
        category,
        "BinaryExpressionAst" | "AssignmentStatementAst" | "UnaryExpressionAst"
    ) {
        result.insert("operator", json!(operator(node, source).to_lowercase()));
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
        let text = source.text(node);
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
        let name = source.text(node.child_by_field_name("name").unwrap());
        result.insert(
            "name",
            json!(decode_backticks(name.strip_prefix('$').unwrap_or(name))),
        );
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
        let source = Source::new(text);
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
                let expected = &ast["fields"][field];
                let mut actual = mapped_field(node, category, field, &source);
                // Omitted null collections correspond to empty concrete
                // lists; unexpected children are still compared explicitly.
                if expected.is_null() && actual.as_array().is_some_and(Vec::is_empty) {
                    actual = Value::Null;
                }
                if actual != *expected {
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
