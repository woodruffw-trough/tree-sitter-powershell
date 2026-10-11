//! Type literals must stay separate from following argument and array commas.
mod common;

use serde_json::Value;
use std::collections::BTreeSet;
use tree_sitter::Node;

fn fixtures() -> Vec<Value> {
    serde_json::from_str(include_str!("fixtures/casts.json")).unwrap()
}

type CastFields = (common::Extent, common::Extent, common::Extent);

fn cast_extents(node: Node<'_>, source: &str, result: &mut BTreeSet<CastFields>) {
    if node.kind() == "cast_expression" {
        result.insert((
            common::utf16_extent(node, source),
            common::utf16_extent(node.child_by_field_name("type").unwrap(), source),
            common::utf16_extent(node.child_by_field_name("value").unwrap(), source),
        ));
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        cast_extents(child, source, result);
    }
}

#[test]
fn cast_boundaries_match_powershell() {
    let cases = fixtures();
    common::assert_syntax(&cases);
    let results = common::oracle(&cases, "tests/fixtures/casts.json");
    common::assert_commands(&cases, &results);
    let mut parser = common::parser();
    for (case, result) in cases.iter().zip(results) {
        if case["valid"] != true {
            continue;
        }
        let source = case["source"].as_str().unwrap();
        let tree = parser.parse(source, None).unwrap();
        let mut actual = BTreeSet::new();
        cast_extents(tree.root_node(), source, &mut actual);
        let expected = result["ast"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|node| node["type"] == "ConvertExpressionAst")
            .map(|node| {
                (
                    (
                        node["start"].as_u64().unwrap() as usize,
                        node["end"].as_u64().unwrap() as usize,
                    ),
                    (
                        node["fields"]["type"]["start"].as_u64().unwrap() as usize,
                        node["fields"]["type"]["end"].as_u64().unwrap() as usize,
                    ),
                    (
                        node["fields"]["value"]["start"].as_u64().unwrap() as usize,
                        node["fields"]["value"]["end"].as_u64().unwrap() as usize,
                    ),
                )
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(
            actual, expected,
            "Cast boundaries and child fields differ for {source}"
        );
    }
}
