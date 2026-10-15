//! Stable fields give query consumers complete source values and list wrappers.
mod common;
#[path = "common/cst.rs"]
mod cst;

use serde_json::{Value, json};
use tree_sitter::{Point, Query, QueryCursor, StreamingIterator};

fn cases() -> Vec<Value> {
    serde_json::from_str(include_str!("fixtures/schema.json")).unwrap()
}

#[test]
fn complete_field_values_and_semantic_relationships_match_powershell() {
    let cases = cases();
    common::assert_syntax(&cases);
    let results = common::oracle(&cases, "tests/fixtures/schema.json");
    common::assert_commands(&cases, &results);
    let checks = cst::assert_structure(&cases, &results);
    assert!(checks > 100, "Only {checks} semantic field comparisons");
}

#[test]
fn consumer_field_queries_capture_complete_values_and_lists() {
    let mut parser = common::parser();
    let language = woodruffw_trough_tree_sitter_powershell::LANGUAGE.into();
    for case in cases() {
        let source = case["source"].as_str().unwrap();
        let tree = parser.parse(source, None).unwrap();
        for check in case["fields"].as_array().unwrap() {
            let kind = check["node"].as_str().unwrap();
            let field = check["field"].as_str().unwrap();
            let query_source = if field.is_empty() {
                // Argument-mode array elements each own a complete wrapper.
                format!("({kind} (command_argument) @value)")
            } else {
                format!("({kind} {field}: _ @value)")
            };
            let query = Query::new(&language, &query_source).unwrap();
            let mut cursor = QueryCursor::new();
            let mut matches = cursor.matches(&query, tree.root_node(), source.as_bytes());
            let mut captures = Vec::new();
            while let Some(matched) = matches.next() {
                for capture in matched.captures() {
                    captures.push(source[capture.node.byte_range()].to_owned());
                }
            }
            let expected: Vec<_> = check["texts"]
                .as_array()
                .unwrap()
                .iter()
                .map(|text| text.as_str().unwrap())
                .collect();
            assert_eq!(
                captures, expected,
                "{}: query {query_source:?} must capture complete source values",
                case["name"]
            );
        }
    }
}

#[test]
fn concrete_expression_queries_cover_calls_and_unary_and_cast_operands() {
    let source = "1+2; [Math]::Abs(3+4); -(5+6); [int](7+8)";
    let mut parser = common::parser();
    let tree = parser.parse(source, None).unwrap();
    assert!(!tree.root_node().has_error());
    let query = Query::new(
        &woodruffw_trough_tree_sitter_powershell::LANGUAGE.into(),
        "(binary_expression) @binary\n(integer_literal) @integer",
    )
    .unwrap();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), source.as_bytes());
    let mut actual = Vec::new();
    while let Some(matched) = matches.next() {
        for capture in matched.captures() {
            let node = capture.node;
            assert_eq!(node.start_position(), Point::new(0, node.start_byte()));
            assert_eq!(node.end_position(), Point::new(0, node.end_byte()));
            actual.push((node.kind(), node.start_byte(), node.end_byte()));
        }
    }
    let mut expected = Vec::new();
    for (kind, spellings) in [
        ("binary_expression", vec!["1+2", "3+4", "5+6", "7+8"]),
        (
            "integer_literal",
            vec!["1", "2", "3", "4", "5", "6", "7", "8"],
        ),
    ] {
        for spelling in spellings {
            let start = source.find(spelling).unwrap();
            expected.push((kind, start, start + spelling.len()));
        }
    }
    actual.sort_unstable();
    expected.sort_unstable();
    assert_eq!(actual, expected);
}

#[test]
fn public_node_types_distinguish_scalar_roles_from_repeated_lists() {
    let nodes: Vec<Value> =
        serde_json::from_str(woodruffw_trough_tree_sitter_powershell::NODE_TYPES).unwrap();
    for (kind, field, required, multiple, types) in [
        (
            "interpolation",
            "expression",
            true,
            false,
            vec![
                "element_access",
                "invocation_expression",
                "member_access",
                "sub_expression",
            ],
        ),
        (
            "command_parameter",
            "name",
            true,
            false,
            vec!["command_parameter_name"],
        ),
        (
            "command_parameter",
            "value",
            false,
            false,
            vec!["command_argument"],
        ),
        (
            "redirection",
            "destination",
            false,
            false,
            vec!["command_argument"],
        ),
        (
            "invocation_operator_command",
            "command",
            true,
            false,
            vec!["command_argument", "command_parameter"],
        ),
        (
            "foreach_parameter",
            "value",
            false,
            false,
            vec!["command_argument"],
        ),
        (
            "switch_clause",
            "condition",
            true,
            false,
            vec!["command_argument", "default"],
        ),
        (
            "switch_statement",
            "file",
            false,
            true,
            vec!["command_argument"],
        ),
        (
            "switch_statement",
            "condition",
            false,
            false,
            vec!["condition"],
        ),
        (
            "switch_statement",
            "clauses",
            true,
            true,
            vec!["switch_clause"],
        ),
        (
            "invocation_expression",
            "arguments",
            true,
            false,
            vec!["argument_list", "script_block"],
        ),
        (
            "invocation_expression",
            "type_arguments",
            false,
            false,
            vec!["generic_type_arguments"],
        ),
        ("attribute", "arguments", true, false, vec!["argument_list"]),
        (
            "base_constructor_call",
            "arguments",
            true,
            false,
            vec!["argument_list"],
        ),
        (
            "function_definition",
            "parameters",
            false,
            false,
            vec!["parameter_list"],
        ),
        (
            "class_method",
            "parameters",
            true,
            false,
            vec!["parameter_list"],
        ),
        (
            "param_block",
            "parameters",
            true,
            false,
            vec!["parameter_list"],
        ),
        ("class_definition", "body", true, false, vec!["class_body"]),
        ("enum_definition", "body", true, false, vec!["enum_body"]),
        (
            "class_body",
            "members",
            false,
            true,
            vec!["class_method", "class_property"],
        ),
        ("enum_body", "members", false, true, vec!["enum_member"]),
        (
            "class_definition",
            "attributes",
            false,
            true,
            vec!["attribute"],
        ),
        (
            "class_definition",
            "base_types",
            false,
            true,
            vec!["type_name"],
        ),
        (
            "enum_definition",
            "underlying_type",
            false,
            false,
            vec!["type_name"],
        ),
        ("parameter", "types", false, true, vec!["type_literal"]),
        ("parameter", "attributes", false, true, vec!["attribute"]),
        ("class_property", "types", false, true, vec!["type_literal"]),
        (
            "for_statement",
            "initializer",
            false,
            false,
            vec![
                "assignment_statement",
                "background_pipeline",
                "pipeline",
                "pipeline_chain",
            ],
        ),
        (
            "for_statement",
            "condition",
            false,
            false,
            vec![
                "assignment_statement",
                "background_pipeline",
                "pipeline",
                "pipeline_chain",
            ],
        ),
        (
            "for_statement",
            "update",
            false,
            false,
            vec![
                "assignment_statement",
                "background_pipeline",
                "pipeline",
                "pipeline_chain",
            ],
        ),
        (
            "pipeline",
            "elements",
            true,
            true,
            vec!["command", "command_expression"],
        ),
        (
            "pipeline_chain",
            "left",
            true,
            false,
            vec!["pipeline", "pipeline_chain"],
        ),
        ("pipeline_chain", "right", true, false, vec!["pipeline"]),
        (
            "background_pipeline",
            "pipeline",
            true,
            false,
            vec!["pipeline", "pipeline_chain"],
        ),
        (
            "command_expression",
            "expression",
            true,
            false,
            vec![
                "array_expression",
                "array_literal_expression",
                "attributed_expression",
                "binary_expression",
                "cast_expression",
                "element_access",
                "expandable_here_string",
                "expandable_string",
                "hashtable_expression",
                "integer_literal",
                "invocation_expression",
                "member_access",
                "parenthesized_expression",
                "real_literal",
                "script_block",
                "single_quoted_here_string",
                "single_quoted_string",
                "sub_expression",
                "ternary_expression",
                "type_literal",
                "unary_expression",
                "variable",
            ],
        ),
        (
            "command_expression",
            "redirections",
            false,
            true,
            vec!["redirection"],
        ),
        ("for_statement", "label", false, false, vec!["label"]),
        ("label", "name", true, false, vec!["identifier"]),
        ("if_statement", "elseif", false, true, vec!["elseif_clause"]),
        ("if_statement", "else", false, false, vec!["else_clause"]),
        (
            "try_statement",
            "handlers",
            false,
            true,
            vec!["catch_clause"],
        ),
        (
            "try_statement",
            "finally",
            false,
            false,
            vec!["finally_clause"],
        ),
        ("catch_clause", "types", false, true, vec!["type_literal"]),
        ("trap_statement", "type", false, false, vec!["type_literal"]),
        ("data_statement", "name", false, false, vec!["identifier"]),
        (
            "data_statement",
            "supported_commands",
            false,
            true,
            vec![
                "array_expression",
                "bare_word",
                "element_access",
                "expandable_here_string",
                "expandable_string",
                "hashtable_expression",
                "integer_literal",
                "invocation_expression",
                "member_access",
                "parenthesized_expression",
                "real_literal",
                "script_block",
                "single_quoted_here_string",
                "single_quoted_string",
                "splat_expression",
                "sub_expression",
                "variable",
            ],
        ),
        (
            "data_statement",
            "body",
            true,
            false,
            vec!["statement_block"],
        ),
        (
            "configuration_definition",
            "body",
            true,
            false,
            vec!["script_block"],
        ),
    ] {
        let node = nodes
            .iter()
            .find(|node| node["named"] == true && node["type"] == kind)
            .unwrap();
        let role = &node["fields"][field];
        assert_eq!(role["required"], json!(required), "{kind}.{field}");
        assert_eq!(role["multiple"], json!(multiple), "{kind}.{field}");
        let actual_types: Vec<_> = role["types"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["type"].as_str().unwrap())
            .collect();
        assert_eq!(actual_types, types, "{kind}.{field}");
    }
    for kind in [
        "return_statement",
        "throw_statement",
        "exit_statement",
        "break_statement",
        "continue_statement",
    ] {
        let node = nodes
            .iter()
            .find(|node| node["named"] == true && node["type"] == kind)
            .unwrap();
        let keyword = &node["fields"]["keyword"];
        assert_eq!(keyword["multiple"], false, "{kind}.keyword");
        assert_eq!(keyword["required"], true, "{kind}.keyword");
        assert_eq!(
            keyword["types"],
            json!([{ "type": kind.strip_suffix("_statement").unwrap(), "named": false }])
        );
        let role = if matches!(kind, "break_statement" | "continue_statement") {
            "label"
        } else {
            "value"
        };
        assert_eq!(node["fields"][role]["multiple"], false, "{kind}.{role}");
        assert_eq!(node["fields"][role]["required"], false, "{kind}.{role}");
    }
}
