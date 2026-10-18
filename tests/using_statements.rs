mod common;
#[path = "common/cst.rs"]
mod cst;

use serde_json::Value;
use tree_sitter::{Query, QueryCursor, StreamingIterator};

#[test]
fn using_value_boundaries_match_powershell() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/using_statements.json")).unwrap();
    common::assert_syntax(&cases);
    let results = common::oracle(&cases, "tests/fixtures/using_statements.json");
    common::assert_commands(&cases, &results);
    cst::assert_structure(&cases, &results);
}

#[test]
fn dot_using_values_preserve_following_commands() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/using_dot_values.json")).unwrap();
    common::assert_syntax(&cases);
    let results = common::oracle(&cases, "tests/fixtures/using_dot_values.json");
    common::assert_commands(&cases, &results);
    cst::assert_structure(&cases, &results);

    let query = Query::new(
        &woodruffw_trough_tree_sitter_powershell::LANGUAGE.into(),
        "(command) @command",
    )
    .unwrap();
    let mut parser = common::parser();
    for (case, result) in cases.iter().zip(results) {
        let source = case["source"].as_str().unwrap();
        let line_end = source[..source.find('\n').unwrap()].encode_utf16().count();
        // A malformed first line can retain a damaged using value. Compare all
        // native commands after that line independently of the recovered value.
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
            .filter(|extent| extent.0 >= line_end)
            .collect();
        let tree = parser.parse(source, None).unwrap();
        let mut cursor = QueryCursor::new();
        let mut captures = cursor.captures(&query, tree.root_node(), source.as_bytes());
        let mut actual = Vec::new();
        while let Some((matched, index)) = captures.next() {
            let extent = common::utf16_extent(matched.captures()[*index].node, source);
            if extent.0 >= line_end {
                actual.push(extent);
            }
        }
        assert_eq!(actual, expected, "{source}\n{}", tree.root_node().to_sexp());
    }
}
