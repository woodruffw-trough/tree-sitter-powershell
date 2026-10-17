//! Malformed operators preserve commands after the next statement boundary.
mod common;
#[path = "common/cst.rs"]
mod cst;

use serde_json::Value;
use tree_sitter::Node;

fn command_extents(node: Node<'_>, source: &str, output: &mut Vec<common::Extent>) {
    if node.kind() == "command" {
        output.push(common::utf16_extent(node, source));
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        command_extents(child, source, output);
    }
}

#[test]
fn malformed_expression_prefixes_retain_following_commands() {
    for (fixtures, path) in [
        (
            include_str!("fixtures/operator_recovery.json"),
            "tests/fixtures/operator_recovery.json",
        ),
        (
            include_str!("fixtures/attribute_prefix_recovery.json"),
            "tests/fixtures/attribute_prefix_recovery.json",
        ),
    ] {
        let cases: Vec<Value> = serde_json::from_str(fixtures).unwrap();
        let results = common::oracle(&cases, path);
        common::assert_syntax(&cases);
        common::assert_commands(&cases, &results);
        cst::assert_structure(&cases, &results);
        let mut parser = common::parser();
        for (case, result) in cases.iter().zip(results) {
            let source = case["source"].as_str().unwrap();
            let tree = parser.parse(source, None).unwrap();
            let mut actual = Vec::new();
            command_extents(tree.root_node(), source, &mut actual);
            let expected: Vec<_> = result["commands"]
                .as_array()
                .unwrap()
                .iter()
                .map(|command| {
                    (
                        command["start"].as_u64().unwrap() as usize,
                        command["end"].as_u64().unwrap() as usize,
                    )
                })
                .collect();
            assert_eq!(
                actual,
                expected,
                "{}\n{}",
                case["name"],
                tree.root_node().to_sexp()
            );
        }
    }
}
