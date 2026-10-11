//! An invocation operator's command field spans the complete expanded target.
mod common;

use serde_json::Value;
use std::collections::BTreeSet;
use tree_sitter::Node;

fn targets(node: Node<'_>, source: &str, result: &mut BTreeSet<common::Extent>) {
    if node.kind() == "invocation_operator_command" {
        let command = node.child_by_field_name("command").unwrap();
        assert_eq!(command.kind(), "command_argument");
        assert_eq!(
            node.children_by_field_name("command", &mut node.walk())
                .count(),
            1
        );
        result.insert(common::utf16_extent(command, source));
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        targets(child, source, result);
    }
}

#[test]
fn invocation_target_field_matches_powershell() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/invocations.json")).unwrap();
    common::assert_syntax(&cases);
    let results = common::oracle(&cases, "tests/fixtures/invocations.json");
    common::assert_commands(&cases, &results);
    let mut parser = common::parser();
    for (case, result) in cases.iter().zip(results) {
        let source = case["source"].as_str().unwrap();
        let tree = parser.parse(source, None).unwrap();
        let mut actual = BTreeSet::new();
        targets(tree.root_node(), source, &mut actual);
        let expected = result["commands"]
            .as_array()
            .unwrap()
            .iter()
            .map(|command| {
                let target = &command["elements"][0];
                (
                    target["start"].as_u64().unwrap() as usize,
                    target["end"].as_u64().unwrap() as usize,
                )
            })
            .collect();
        assert_eq!(
            actual, expected,
            "Invocation target fields differ for {source}"
        );
    }
}
