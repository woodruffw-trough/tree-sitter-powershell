//! Variable tokenization, interpolation, and malformed variable-path coverage.
mod common;

use serde_json::Value;
use std::collections::BTreeSet;
use tree_sitter::Node;

fn fixtures() -> Vec<Value> {
    serde_json::from_str(include_str!("fixtures/variables.json")).unwrap()
}

#[test]
fn variable_syntax() {
    common::assert_syntax(&fixtures());
}

fn variable_extents(node: Node<'_>, source: &str, result: &mut BTreeSet<common::Extent>) {
    if node.kind() == "variable" {
        result.insert(common::utf16_extent(node, source));
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        variable_extents(child, source, result);
    }
}

fn interpolation_coverage(node: Node<'_>, source: &str, failures: &mut Vec<String>) {
    let body = if node.kind() == "expandable_string" && node.child_count() >= 2 {
        Some((
            node.child(0).unwrap().end_byte(),
            node.child(node.child_count() - 1).unwrap().start_byte(),
        ))
    } else if node.kind() == "expandable_here_string" {
        // The scanner's hidden header/footer tokens are absent from the tree.
        // Their physical newlines define the body that must be fully covered.
        let raw = &source[node.byte_range()];
        let first = raw.find(['\r', '\n']).unwrap();
        let newline_width = if raw[first..].starts_with("\r\n") {
            2
        } else {
            1
        };
        let mut last = raw.rfind(['\r', '\n']).unwrap();
        if raw.as_bytes()[last] == b'\n' && last != 0 && raw.as_bytes()[last - 1] == b'\r' {
            last -= 1;
        }
        let start = node.start_byte() + first + newline_width;
        Some((start, (node.start_byte() + last).max(start)))
    } else {
        None
    };
    if let Some((mut offset, end)) = body {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.start_byte() != offset {
                failures.push(format!("String content missing at byte {offset}"));
            }
            offset = child.end_byte();
        }
        if offset != end {
            failures.push(format!("String content missing at byte {offset}"));
        }
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        interpolation_coverage(child, source, failures);
    }
}

#[test]
fn variable_oracle_extents() {
    let expected = fixtures();
    let results = common::oracle(&expected, "tests/fixtures/variables.json");
    let mut parser = common::parser();
    let mut failures = Vec::new();
    for (case, result) in expected.iter().zip(&results) {
        if case["valid"] != true {
            continue;
        }
        let source = case["source"].as_str().unwrap();
        let tree = parser.parse(source, None).unwrap();
        let mut actual = BTreeSet::new();
        variable_extents(tree.root_node(), source, &mut actual);
        let expected: BTreeSet<_> = result["ast"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|node| node["type"] == "VariableExpressionAst")
            .map(|node| {
                (
                    node["start"].as_u64().unwrap() as usize,
                    node["end"].as_u64().unwrap() as usize,
                )
            })
            .collect();
        let mut content_failures = Vec::new();
        interpolation_coverage(tree.root_node(), source, &mut content_failures);
        if actual != expected || !content_failures.is_empty() {
            failures.push(format!(
                "{}: variable interpolation differs\n{}\nexpected: {:?}\nactual: {:?}\n{}\n{}",
                case["name"],
                source,
                expected,
                actual,
                content_failures.join("\n"),
                tree.root_node().to_sexp()
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{}",
        common::failure_summary(&failures)
    );
    common::assert_commands(&expected, &results);
}
