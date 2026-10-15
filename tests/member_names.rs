//! Dynamic member names preserve complete operands and postfix boundaries.
mod common;
#[path = "common/cst.rs"]
mod cst;

use serde_json::Value;
use tree_sitter::{Query, QueryCursor, StreamingIterator};

fn cases() -> Vec<Value> {
    serde_json::from_str(include_str!("fixtures/member_names.json")).unwrap()
}

#[test]
fn member_name_syntax_and_fields_match_powershell() {
    let cases = cases();
    common::assert_syntax(&cases);
    let results = common::oracle(&cases, "tests/fixtures/member_names.json");
    common::assert_commands(&cases, &results);
    cst::assert_structure(&cases, &results);
}

#[test]
fn member_queries_capture_complete_operands() {
    let language = woodruffw_trough_tree_sitter_powershell::LANGUAGE.into();
    let query = Query::new(&language, "(member_access member: _ @member)").unwrap();
    let mut parser = common::parser();
    for case in cases() {
        let Some(expected) = case["members"].as_array() else {
            continue;
        };
        let source = case["source"].as_str().unwrap();
        let tree = parser.parse(source, None).unwrap();
        let mut cursor = QueryCursor::new();
        let mut captures = cursor.matches(&query, tree.root_node(), source.as_bytes());
        let mut actual = Vec::new();
        while let Some(matched) = captures.next() {
            for capture in matched.captures() {
                actual.push(&source[capture.node.byte_range()]);
            }
        }
        let expected: Vec<_> = expected
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect();
        assert_eq!(actual, expected, "{}: {source}", case["name"]);
    }
}
