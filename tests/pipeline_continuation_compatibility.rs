//! Intentional syntax compatibility despite PowerShell's nested source-offset bug.
// Compatibility cases use the oracle without asserting identical syntax.
#[allow(dead_code)]
mod common;

use serde_json::Value;
use tree_sitter::{Query, QueryCursor, StreamingIterator};

#[test]
fn interpolated_continuation_keeps_one_pipeline_despite_native_offset_diagnostics() {
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "fixtures/pipeline_continuation_compatibility.json"
    ))
    .unwrap();
    // These fixtures record native validity and exact diagnostics. They are
    // deliberate compatibility exceptions, separate from conformance tests.
    let results = common::oracle(
        &cases,
        "tests/fixtures/pipeline_continuation_compatibility.json",
    );
    let mut parser = common::parser();
    let query = Query::new(
        &parser.language().unwrap(),
        "(interpolation expression: (sub_expression) @body)",
    )
    .unwrap();
    for (case, native) in cases.iter().zip(results) {
        let expected_diagnostics = case["diagnostics"].as_array().unwrap();
        let diagnostics: Vec<_> = native["errors"]
            .as_array()
            .unwrap()
            .iter()
            .map(|error| error["id"].clone())
            .collect();
        assert_eq!(&diagnostics, expected_diagnostics, "{}", case["name"]);

        let source = case["source"].as_str().unwrap();
        let tree = parser.parse(source, None).unwrap();
        assert!(
            !tree.root_node().has_error(),
            "{}: intended continuation must parse\n{}",
            case["name"],
            tree.root_node().to_sexp()
        );
        let mut query_cursor = QueryCursor::new();
        let mut matches = query_cursor.matches(&query, tree.root_node(), source.as_bytes());
        let mut bodies = Vec::new();
        while let Some(matched) = matches.next() {
            bodies.extend(matched.captures().iter().map(|capture| capture.node));
        }
        assert_eq!(bodies.len(), 1, "{}", case["name"]);
        let mut cursor = bodies[0].walk();
        let statements: Vec<_> = bodies[0]
            .named_children(&mut cursor)
            .filter(|child| !child.is_extra())
            .collect();
        assert_eq!(statements.len(), 1, "{}", case["name"]);
        let pipeline = if let Some(flow_kind) = case["flow"].as_str() {
            assert_eq!(statements[0].kind(), flow_kind, "{}", case["name"]);
            statements[0].child_by_field_name("value").unwrap()
        } else {
            statements[0]
        };
        assert_eq!(pipeline.kind(), "pipeline", "{}", case["name"]);
        let mut cursor = pipeline.walk();
        let elements: Vec<_> = pipeline
            .children_by_field_name("elements", &mut cursor)
            .collect();
        let expected_commands = case["intended_commands"].as_array().unwrap();
        assert_eq!(elements.len(), expected_commands.len(), "{}", case["name"]);
        for (element, expected) in elements.iter().zip(expected_commands) {
            assert_eq!(element.kind(), "command", "{}", case["name"]);
            assert_eq!(&source[element.byte_range()], expected.as_str().unwrap());
            assert!(element.child_by_field_name("name").is_some());
        }
    }
}
