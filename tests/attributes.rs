//! Validation attributes preserve their variable targets and intervening casts.
mod common;

use serde_json::Value;
use std::collections::BTreeSet;
use tree_sitter::Node;

type ExpressionFields = (String, common::Extent, common::Extent, common::Extent);

fn expressions(node: Node<'_>, source: &str, result: &mut BTreeSet<ExpressionFields>) {
    let fields = match node.kind() {
        "attributed_expression" => Some(("AttributedExpressionAst", "attribute")),
        "cast_expression" => Some(("ConvertExpressionAst", "type")),
        _ => None,
    };
    if let Some((kind, field)) = fields {
        result.insert((
            kind.to_owned(),
            common::utf16_extent(node, source),
            common::utf16_extent(node.child_by_field_name(field).unwrap(), source),
            common::utf16_extent(node.child_by_field_name("value").unwrap(), source),
        ));
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        expressions(child, source, result);
    }
}

fn extent(value: &Value) -> common::Extent {
    (
        value["start"].as_u64().unwrap() as usize,
        value["end"].as_u64().unwrap() as usize,
    )
}

#[test]
fn attributed_variable_fields_match_powershell() {
    let cases: Vec<Value> = serde_json::from_str(include_str!("fixtures/attributes.json")).unwrap();
    common::assert_syntax(&cases);
    let results = common::oracle(&cases, "tests/fixtures/attributes.json");
    common::assert_commands(&cases, &results);
    let mut parser = common::parser();
    for (case, result) in cases.iter().zip(results) {
        if case["valid"] != true {
            continue;
        }
        let source = case["source"].as_str().unwrap();
        let tree = parser.parse(source, None).unwrap();
        let mut actual = BTreeSet::new();
        expressions(tree.root_node(), source, &mut actual);
        let expected = result["ast"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|node| {
                let kind = node["type"].as_str().unwrap();
                let field = match kind {
                    "AttributedExpressionAst" => "attribute",
                    "ConvertExpressionAst" => "type",
                    _ => return None,
                };
                Some((
                    kind.to_owned(),
                    extent(node),
                    extent(&node["fields"][field]),
                    extent(&node["fields"]["value"]),
                ))
            })
            .collect();
        assert_eq!(
            actual, expected,
            "Attributed expression fields differ for {source}"
        );
    }
}

#[test]
fn long_attribute_chains_preserve_each_operand_field() {
    let mut parser = common::parser();
    for (prefix, count, cast) in [("[A()]", 4096, false), ("[A()][int]", 2048, true)] {
        let source = format!("{}$x", prefix.repeat(count));
        let tree = parser.parse(&source, None).unwrap();
        assert!(!tree.root_node().has_error(), "{count} prefixes");
        let mut value = tree
            .root_node()
            .named_child(0)
            .unwrap()
            .child_by_field_name("elements")
            .unwrap()
            .child_by_field_name("expression")
            .unwrap();
        for index in 0..count {
            assert_eq!(value.kind(), "attributed_expression", "prefix {index}");
            let attribute = value.child_by_field_name("attribute").unwrap();
            assert_eq!(&source[attribute.byte_range()], "[A()]");
            value = value.child_by_field_name("value").unwrap();
            if cast {
                assert_eq!(value.kind(), "cast_expression", "prefix {index}");
                let type_name = value.child_by_field_name("type").unwrap();
                assert_eq!(&source[type_name.byte_range()], "[int]");
                value = value.child_by_field_name("value").unwrap();
            }
        }
        assert_eq!(value.kind(), "variable");
        assert_eq!(&source[value.byte_range()], "$x");
    }
}
