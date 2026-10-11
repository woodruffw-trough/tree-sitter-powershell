mod common;

use serde_json::Value;
use std::collections::BTreeSet;
use tree_sitter::Node;

fn fixtures() -> Vec<Value> {
    serde_json::from_str(include_str!("fixtures/oracle.json")).unwrap()
}

#[test]
fn fixture_syntax() {
    common::assert_syntax(&fixtures());
}

fn binary_extents(node: Node<'_>, source: &str, result: &mut BTreeSet<common::Extent>) {
    if node.kind() == "binary_expression" {
        result.insert(common::utf16_extent(node, source));
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        binary_extents(child, source, result);
    }
}

#[test]
fn powershell_parser_agrees() {
    let expected = fixtures();
    let results = common::oracle(&expected, "tests/fixtures/oracle.json");
    let mut parser = common::parser();
    for (case, result) in expected.iter().zip(&results) {
        if case["checkAst"].as_bool() == Some(true) {
            let source = case["source"].as_str().unwrap();
            let tree = parser.parse(source, None).unwrap();
            let mut actual = BTreeSet::new();
            binary_extents(tree.root_node(), source, &mut actual);
            let expected: BTreeSet<_> = result["ast"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|node| node["type"] == "BinaryExpressionAst")
                .map(|node| {
                    (
                        node["start"].as_u64().unwrap() as usize,
                        node["end"].as_u64().unwrap() as usize,
                    )
                })
                .collect();
            assert_eq!(
                actual, expected,
                "AST extents disagree for {}:\n{}",
                case["name"], source
            );
        }
    }
    common::assert_commands(&expected, &results);
}
