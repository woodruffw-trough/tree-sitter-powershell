//! Complete, unmodified scripts from the pinned reference implementation.
mod common;

use serde_json::Value;
use std::{collections::BTreeSet, fs, path::Path};
use tree_sitter::Node;

fn fixtures() -> Vec<Value> {
    let mut cases: Vec<Value> =
        serde_json::from_str(include_str!("upstream/fixtures.json")).unwrap();
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/upstream");
    for case in &mut cases {
        let source = fs::read_to_string(directory.join(case["path"].as_str().unwrap())).unwrap();
        // .NET's ReadAllText consumes a UTF-8 BOM before ParseInput.
        case["source"] = Value::String(source.trim_start_matches('\u{feff}').to_owned());
    }
    cases
}

#[test]
fn complete_upstream_scripts_parse() {
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
fn upstream_command_and_expression_boundaries_match_powershell() {
    let cases = fixtures();
    let results = common::oracle(&cases, "tests/upstream/fixtures.json");
    common::assert_commands(&cases, &results);
    let mut parser = common::parser();
    for (case, result) in cases.iter().zip(&results) {
        let source = case["source"].as_str().unwrap();
        let tree = parser.parse(source, None).unwrap();
        let mut actual = BTreeSet::new();
        binary_extents(tree.root_node(), source, &mut actual);
        let expected = result["ast"]
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
            .collect::<BTreeSet<_>>();
        let unexpected = actual.difference(&expected).collect::<Vec<_>>();
        let missing = expected.difference(&actual).collect::<Vec<_>>();
        assert_eq!(
            actual, expected,
            "Binary expression grouping differs for {}: unexpected={unexpected:?}, missing={missing:?}",
            case["name"]
        );
    }
}
