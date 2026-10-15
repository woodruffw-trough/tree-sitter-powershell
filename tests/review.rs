//! Targeted syntax and grouping review beyond the combinatorial token matrices.
mod common;

use serde_json::Value;
use std::collections::BTreeSet;
use tree_sitter::Node;

fn fixtures() -> Vec<Value> {
    serde_json::from_str(include_str!("fixtures/review.json")).unwrap()
}

#[test]
fn fixture_syntax() {
    common::assert_syntax(&fixtures());
}

// Compare selected statement and expression boundaries. This catches a switch
// clause such as `1+2` being parsed as arithmetic instead of a bare string, and
// verifies statements captured by an assignment remain complete statements.
fn reviewed_extents(node: Node<'_>, source: &str, result: &mut BTreeSet<(String, common::Extent)>) {
    let ast_kind = match node.kind() {
        "binary_expression" => Some("BinaryExpressionAst"),
        "assignment_statement" => Some("AssignmentStatementAst"),
        "if_statement" => Some("IfStatementAst"),
        "switch_statement" => Some("SwitchStatementAst"),
        "foreach_statement" => Some("ForEachStatementAst"),
        "for_statement" => Some("ForStatementAst"),
        "while_statement" => Some("WhileStatementAst"),
        "do_statement" => {
            let mut cursor = node.walk();
            if node
                .children(&mut cursor)
                .any(|child| child.kind() == "until")
            {
                Some("DoUntilStatementAst")
            } else {
                Some("DoWhileStatementAst")
            }
        }
        "function_definition" | "class_method" => Some("FunctionDefinitionAst"),
        "class_definition" | "enum_definition" => Some("TypeDefinitionAst"),
        "data_statement" => Some("DataStatementAst"),
        "trap_statement" => Some("TrapStatementAst"),
        "try_statement" => Some("TryStatementAst"),
        "break_statement" => Some("BreakStatementAst"),
        "continue_statement" => Some("ContinueStatementAst"),
        "return_statement" => Some("ReturnStatementAst"),
        "throw_statement" => Some("ThrowStatementAst"),
        "exit_statement" => Some("ExitStatementAst"),
        _ => None,
    };
    if let Some(kind) = ast_kind {
        let extent = if node.kind() == "class_method" {
            // PowerShell's underlying FunctionDefinitionAst begins at the
            // method name; the CST member includes return types and modifiers.
            let name = node.child_by_field_name("name").unwrap();
            let body = node.child_by_field_name("body").unwrap();
            (
                common::utf16_extent(name, source).0,
                common::utf16_extent(body, source).1,
            )
        } else {
            common::utf16_extent(node, source)
        };
        result.insert((kind.to_owned(), extent));
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        reviewed_extents(child, source, result);
    }
}

#[test]
fn powershell_parser_agrees() {
    let cases = fixtures();
    let results = common::oracle(&cases, "tests/fixtures/review.json");
    common::assert_commands(&cases, &results);

    let kinds = [
        "BinaryExpressionAst",
        "AssignmentStatementAst",
        "IfStatementAst",
        "SwitchStatementAst",
        "ForEachStatementAst",
        "ForStatementAst",
        "WhileStatementAst",
        "DoWhileStatementAst",
        "DoUntilStatementAst",
        "FunctionDefinitionAst",
        "TypeDefinitionAst",
        "DataStatementAst",
        "TrapStatementAst",
        "TryStatementAst",
        "BreakStatementAst",
        "ContinueStatementAst",
        "ReturnStatementAst",
        "ThrowStatementAst",
        "ExitStatementAst",
    ];
    let mut parser = common::parser();
    for (case, result) in cases.iter().zip(&results) {
        if case["valid"] != true {
            continue;
        }
        let source = case["source"].as_str().unwrap();
        let tree = parser.parse(source, None).unwrap();
        let mut actual = BTreeSet::new();
        reviewed_extents(tree.root_node(), source, &mut actual);
        let expected = result["ast"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|node| kinds.contains(&node["type"].as_str().unwrap()))
            .map(|node| {
                (
                    node["type"].as_str().unwrap().to_owned(),
                    (
                        node["start"].as_u64().unwrap() as usize,
                        node["end"].as_u64().unwrap() as usize,
                    ),
                )
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(
            actual, expected,
            "AST grouping differs for {}:\n{}",
            case["name"], source
        );
    }
}

fn expandable_body_coverage(node: Node<'_>, source: &str, failures: &mut Vec<String>) {
    if node.kind() == "expandable_string" {
        let text = &source[node.byte_range()];
        let mut chars = text.chars();
        if let (Some(open), Some(close)) = (chars.next(), chars.next_back()) {
            let mut offset = node.start_byte() + open.len_utf8();
            let end = node.end_byte() - close.len_utf8();
            let mut cursor = node.walk();
            for child in node.named_children(&mut cursor) {
                if child.start_byte() != offset {
                    failures.push(format!("String content missing at byte {offset}"));
                }
                if !matches!(
                    child.kind(),
                    "string_content" | "escape_sequence" | "variable" | "interpolation"
                ) {
                    failures.push(format!(
                        "Unexpected {} in string body at byte {}",
                        child.kind(),
                        child.start_byte()
                    ));
                }
                offset = child.end_byte();
            }
            if offset != end {
                failures.push(format!("String content missing at byte {offset}"));
            }
        }
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        expandable_body_coverage(child, source, failures);
    }
}

#[test]
fn expandable_strings_preserve_every_body_byte() {
    let mut parser = common::parser();
    let mut failures = Vec::new();
    for case in fixtures() {
        if case["valid"] != true {
            continue;
        }
        let source = case["source"].as_str().unwrap();
        let tree = parser.parse(source, None).unwrap();
        let mut gaps = Vec::new();
        expandable_body_coverage(tree.root_node(), source, &mut gaps);
        if !gaps.is_empty() {
            failures.push(format!(
                "{}:\n{}\n{}\n{}",
                case["name"],
                source,
                gaps.join("\n"),
                tree.root_node().to_sexp()
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{}",
        common::failure_summary(&failures)
    );
}
