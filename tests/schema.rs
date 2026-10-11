//! Stable fields give query consumers complete source values and list wrappers.
mod common;
#[path = "common/cst.rs"]
mod cst;

use serde_json::{Value, json};
use tree_sitter::{Query, QueryCursor, StreamingIterator};

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
    let language = tree_sitter_powershell::LANGUAGE.into();
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
fn public_node_types_distinguish_scalar_roles_from_repeated_lists() {
    let nodes: Vec<Value> = serde_json::from_str(tree_sitter_powershell::NODE_TYPES).unwrap();
    for (kind, field, required, multiple, types) in [
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
            vec!["pipeline"],
        ),
        ("for_statement", "condition", false, false, vec!["pipeline"]),
        ("for_statement", "update", false, false, vec!["pipeline"]),
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
            vec!["bare_word", "expandable_string", "single_quoted_string"],
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
}
