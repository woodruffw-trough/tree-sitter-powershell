//! Flow operands and colon-label fallback follow PowerShell's command-mode peek.
mod common;
#[path = "common/cst.rs"]
mod cst;

use serde_json::Value;
use tree_sitter::{Query, QueryCursor, StreamingIterator};

#[test]
fn flow_labels_and_following_statements_match_powershell() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/flow_labels.json")).unwrap();
    common::assert_syntax(&cases);
    let results = common::oracle(&cases, "tests/fixtures/flow_labels.json");
    common::assert_commands(&cases, &results);
    let checks = cst::assert_structure(&cases, &results);
    assert!(checks > 200, "Only {checks} semantic relationship checks");
}

#[test]
fn malformed_flow_labels_preserve_following_commands() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/flow_label_recovery.json")).unwrap();
    let results = common::oracle(&cases, "tests/fixtures/flow_label_recovery.json");
    let query = Query::new(
        &woodruffw_trough_tree_sitter_powershell::LANGUAGE.into(),
        "(command) @command",
    )
    .unwrap();
    let mut parser = common::parser();
    for (case, result) in cases.iter().zip(results) {
        let source = case["source"].as_str().unwrap();
        let boundary = source.find("Test-RecoveryAfterLabel").unwrap();
        let tail_offset = source[..boundary].encode_utf16().count();
        let expected: Vec<_> = result["commands"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|command| {
                let start = command["start"].as_u64().unwrap() as usize;
                let end = command["end"].as_u64().unwrap() as usize;
                (start >= tail_offset).then_some((start, end))
            })
            .collect();
        assert!(!expected.is_empty(), "{source}");
        let tree = parser.parse(source, None).unwrap();
        let mut cursor = QueryCursor::new();
        let mut captures = cursor.captures(&query, tree.root_node(), source.as_bytes());
        let mut actual = Vec::new();
        while let Some((matched, index)) = captures.next() {
            let extent = common::utf16_extent(matched.captures()[*index].node, source);
            if extent.0 >= tail_offset {
                actual.push(extent);
            }
        }
        assert_eq!(actual, expected, "{source}\n{}", tree.root_node().to_sexp());
    }
}
