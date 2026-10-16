//! Configuration name validation happens after the header has been parsed.
mod common;
#[path = "common/cst.rs"]
mod cst;

use serde_json::Value;
use tree_sitter::{Query, QueryCursor, StreamingIterator};

#[test]
fn invalid_constant_configuration_names_retain_concrete_header_and_body() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/config_invalid_names.json")).unwrap();
    common::assert_syntax(&cases);
    let results = common::oracle(&cases, "tests/fixtures/config_invalid_names.json");
    let mut parser = common::parser();
    let language = parser.language().unwrap();
    let query = Query::new(
        &language,
        "(configuration_definition name: _ @name body: (script_block) @body)",
    )
    .unwrap();
    for (case, result) in cases.iter().zip(&results) {
        let errors: Vec<_> = result["errors"]
            .as_array()
            .unwrap()
            .iter()
            .map(|error| error["id"].as_str().unwrap())
            .collect();
        assert_eq!(errors, ["InvalidConfigurationName"], "{}", case["name"]);
        let ast = result["ast"].as_array().unwrap();
        assert!(
            ast.iter().any(|node| node["type"] == "ErrorStatementAst"),
            "{}",
            case["name"]
        );
        assert!(
            !ast.iter()
                .any(|node| node["type"] == "ConfigurationDefinitionAst"),
            "{}",
            case["name"]
        );
        // ConfigurationStatementRule calls this a semantic check and discards
        // the declaration AST. The diagnostic still identifies the full name.
        // Query the actual source roles without claiming an AST that is absent.
        let source = case["source"].as_str().unwrap();
        let tree = parser.parse(source, None).unwrap();
        assert!(!tree.root_node().has_error(), "{}", case["name"]);
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, tree.root_node(), source.as_bytes());
        let mut names = Vec::new();
        let mut bodies = Vec::new();
        while let Some(matched) = matches.next() {
            for capture in matched.captures() {
                let text = &source[capture.node.byte_range()];
                match query.capture_names()[capture.index as usize] {
                    "name" => {
                        assert_eq!(text, case["configuration_name"].as_str().unwrap());
                        let extent = common::utf16_extent(capture.node, source);
                        assert_eq!(
                            extent.0 as u64,
                            result["errors"][0]["start"].as_u64().unwrap()
                        );
                        assert_eq!(
                            extent.1 as u64,
                            result["errors"][0]["end"].as_u64().unwrap()
                        );
                        names.push(text);
                    }
                    "body" => bodies.push(text),
                    _ => unreachable!(),
                }
            }
        }
        assert_eq!(names.len(), 1, "{}", case["name"]);
        assert_eq!(bodies, [case["configuration_body"].as_str().unwrap()]);
    }
    // The parser discards these declaration ASTs but preserves later commands.
    common::assert_commands(&cases, &results);
}

#[test]
fn configuration_semantic_diagnostics_with_retained_ast_keep_matching_structure() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/config_semantic_ast.json")).unwrap();
    common::assert_syntax(&cases);
    let results = common::oracle(&cases, "tests/fixtures/config_semantic_ast.json");
    for result in &results {
        assert!(
            result["ast"]
                .as_array()
                .unwrap()
                .iter()
                .any(|node| { node["type"] == "ConfigurationDefinitionAst" })
        );
    }
    common::assert_commands(&cases, &results);
    cst::assert_structure(&cases, &results);
}
