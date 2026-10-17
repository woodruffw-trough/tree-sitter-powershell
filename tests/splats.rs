//! Context-sensitive splatting versus ordinary generic command arguments.
mod common;
#[path = "common/cst.rs"]
mod cst;

use serde_json::Value;
use std::collections::BTreeSet;
use tree_sitter::Node;

fn fixtures() -> Vec<Value> {
    serde_json::from_str(include_str!("fixtures/splats.json")).unwrap()
}

#[test]
fn splat_syntax() {
    common::assert_syntax(&fixtures());
}

fn splat_extents(node: Node<'_>, source: &str, result: &mut BTreeSet<common::Extent>) {
    if node.kind() == "splat_expression" {
        result.insert(common::utf16_extent(node, source));
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        splat_extents(child, source, result);
    }
}

#[test]
fn splat_token_classification_matches_powershell() {
    let cases = fixtures();
    let results = common::oracle(&cases, "tests/fixtures/splats.json");
    let mut parser = common::parser();
    let mut failures = Vec::new();
    for (case, result) in cases.iter().zip(&results) {
        if case["valid"] != true {
            continue;
        }
        let source = case["source"].as_str().unwrap();
        let tree = parser.parse(source, None).unwrap();
        let mut actual = BTreeSet::new();
        splat_extents(tree.root_node(), source, &mut actual);
        let expected = result["tokens"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|token| token["kind"] == "SplattedVariable")
            .map(|token| {
                (
                    token["start"].as_u64().unwrap() as usize,
                    token["end"].as_u64().unwrap() as usize,
                )
            })
            .collect::<BTreeSet<_>>();
        if actual != expected {
            failures.push(format!(
                "{}: splat tokenization differs\n{}\nexpected: {:?}\nactual: {:?}\n{}",
                case["name"],
                source,
                expected,
                actual,
                tree.root_node().to_sexp()
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{}",
        common::failure_summary(&failures)
    );
    common::assert_commands(&cases, &results);
    cst::assert_structure(&cases, &results);
}
