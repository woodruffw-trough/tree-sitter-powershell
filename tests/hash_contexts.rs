//! Hashtable values inherit comma suppression from their enclosing expression.
mod common;
#[path = "common/cst.rs"]
mod cst;

use serde_json::Value;
use std::collections::BTreeSet;
use tree_sitter::{Query, QueryCursor, StreamingIterator};

#[test]
fn inherited_comma_context_matches_powershell() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/hash_contexts.json")).unwrap();
    common::assert_syntax(&cases);
    let results = common::oracle(&cases, "tests/fixtures/hash_contexts.json");
    common::assert_commands(&cases, &results);
    cst::assert_structure(&cases, &results);
}

#[test]
fn public_field_queries_match_inside_comma_suppressed_hashtables() {
    let source = "$obj.M(@{a=Write-Output 1,2})";
    let mut parser = common::parser();
    let tree = parser.parse(source, None).unwrap();
    assert!(!tree.root_node().has_error());
    let query = Query::new(
        &woodruffw_trough_tree_sitter_powershell::LANGUAGE.into(),
        "(hash_entry
          key: (identifier) @key
          value: (pipeline
            (command
              name: (command_name) @name
              (command_argument
                (array_literal_expression) @values))) @pipeline)",
    )
    .unwrap();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), source.as_bytes());
    let mut actual = BTreeSet::new();
    while let Some(matched) = matches.next() {
        for capture in matched.captures() {
            actual.insert((
                query.capture_names()[capture.index as usize],
                &source[capture.node.byte_range()],
            ));
        }
    }
    assert_eq!(
        actual,
        BTreeSet::from([
            ("key", "a"),
            ("name", "Write-Output"),
            ("values", "1,2"),
            ("pipeline", "Write-Output 1,2"),
        ])
    );
}
