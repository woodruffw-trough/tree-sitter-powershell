//! Adjacent script blocks are method arguments, just like parenthesized values.
mod common;

use serde_json::Value;
use std::collections::BTreeSet;
use tree_sitter::Node;

type MethodFields = (
    common::Extent,
    common::Extent,
    common::Extent,
    Vec<common::Extent>,
);

fn methods(node: Node<'_>, source: &str, result: &mut BTreeSet<MethodFields>) {
    if node.kind() == "invocation_expression" {
        let function = node.child_by_field_name("function").unwrap();
        let arguments = node.child_by_field_name("arguments").unwrap();
        let mut cursor = node.walk();
        let arguments = if arguments.kind() == "argument_list" {
            arguments
                .named_children(&mut cursor)
                .filter(|child| !matches!(child.kind(), "comment" | "line_continuation"))
                .map(|child| common::utf16_extent(child, source))
                .collect()
        } else {
            vec![common::utf16_extent(arguments, source)]
        };
        result.insert((
            common::utf16_extent(node, source),
            common::utf16_extent(function.child_by_field_name("object").unwrap(), source),
            common::utf16_extent(function.child_by_field_name("member").unwrap(), source),
            arguments,
        ));
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        methods(child, source, result);
    }
}

fn extent(value: &Value) -> common::Extent {
    (
        value["start"].as_u64().unwrap() as usize,
        value["end"].as_u64().unwrap() as usize,
    )
}

#[test]
fn method_arguments_match_powershell() {
    let cases: Vec<Value> = serde_json::from_str(include_str!("fixtures/methods.json")).unwrap();
    common::assert_syntax(&cases);
    let results = common::oracle(&cases, "tests/fixtures/methods.json");
    common::assert_commands(&cases, &results);
    let mut parser = common::parser();
    for (case, result) in cases.iter().zip(results) {
        if case["valid"] != true {
            continue;
        }
        let source = case["source"].as_str().unwrap();
        let tree = parser.parse(source, None).unwrap();
        let mut actual = BTreeSet::new();
        methods(tree.root_node(), source, &mut actual);
        let expected = result["ast"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|node| node["type"] == "InvokeMemberExpressionAst")
            .map(|node| {
                (
                    extent(node),
                    extent(&node["fields"]["object"]),
                    extent(&node["fields"]["member"]),
                    node["fields"]["arguments"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(extent)
                        .collect(),
                )
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(actual, expected, "Method fields differ for {source}");
    }
}
