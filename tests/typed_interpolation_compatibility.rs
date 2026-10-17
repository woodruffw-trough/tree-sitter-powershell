//! Intended range syntax despite PowerShell's collected-text resynchronization defect.
#[allow(dead_code)]
mod common;

use serde_json::Value;
use tree_sitter::{Query, QueryCursor, StreamingIterator};

#[test]
fn typed_ranges_retain_their_operands_despite_native_resynchronization_diagnostics() {
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "fixtures/typed_interpolation_compatibility.json"
    ))
    .unwrap();
    // Native diagnostics and overlapping extents are checked separately from
    // the intended CST. These are explicit compatibility exceptions.
    let results = common::oracle(
        &cases,
        "tests/fixtures/typed_interpolation_compatibility.json",
    );
    let mut parser = common::parser();
    let query = Query::new(
        &parser.language().unwrap(),
        "(binary_expression left: (type_literal) @type operator: \"..\" right: (_) @right)",
    )
    .unwrap();
    for (case, native) in cases.iter().zip(results) {
        let ast = native["ast"].as_array().unwrap();
        let overlapping = ast
            .iter()
            .filter(|node| node["type"] == "TypeExpressionAst")
            .any(|ty| {
                ast.iter()
                    .filter(|node| node["type"] == "CommandAst")
                    .any(|command| {
                        ty["start"].as_u64().unwrap() < command["start"].as_u64().unwrap()
                            && command["start"].as_u64().unwrap() < ty["end"].as_u64().unwrap()
                    })
            });
        assert_eq!(
            overlapping,
            case["native_overlap"].as_bool().unwrap(),
            "{}",
            case["name"]
        );

        let source = case["source"].as_str().unwrap();
        let tree = parser.parse(source, None).unwrap();
        assert!(
            !tree.root_node().has_error(),
            "{}: {}",
            case["name"],
            tree.root_node().to_sexp()
        );
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, tree.root_node(), source.as_bytes());
        let mut captures = Vec::new();
        while let Some(matched) = matches.next() {
            captures.extend(matched.captures().iter().map(|capture| {
                (
                    query.capture_names()[capture.index as usize],
                    &source[capture.node.byte_range()],
                )
            }));
        }
        assert_eq!(
            captures,
            [
                ("type", case["intended_type"].as_str().unwrap()),
                ("right", case["intended_right"].as_str().unwrap()),
            ],
            "{}",
            case["name"]
        );
    }
}
