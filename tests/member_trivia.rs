//! Command-mode member boundaries retain literal text and following commands.
mod common;
#[path = "common/cst.rs"]
mod cst;

use serde_json::Value;

#[test]
fn command_member_trivia_matches_powershell() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/member_trivia.json")).unwrap();
    common::assert_syntax(&cases);
    let results = common::oracle(&cases, "tests/fixtures/member_trivia.json");
    common::assert_commands(&cases, &results);
    assert!(cst::assert_structure(&cases, &results) > 400);
}

#[test]
fn malformed_computed_members_preserve_following_commands() {
    use tree_sitter::{Query, QueryCursor, StreamingIterator};

    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/member_recovery.json")).unwrap();
    let results = common::oracle(&cases, "tests/fixtures/member_recovery.json");
    let query = Query::new(
        &woodruffw_trough_tree_sitter_powershell::LANGUAGE.into(),
        "(command) @command",
    )
    .unwrap();
    let mut parser = common::parser();
    for (case, result) in cases.iter().zip(results) {
        let source = case["source"].as_str().unwrap();
        // The first command has the damaged member; compare its successors.
        let expected: Vec<_> = result["commands"]
            .as_array()
            .unwrap()
            .iter()
            .skip(1)
            .map(|command| {
                (
                    command["start"].as_u64().unwrap() as usize,
                    command["end"].as_u64().unwrap() as usize,
                )
            })
            .collect();
        let tail_offset = expected.first().unwrap().0;
        let tree = parser.parse(source, None).unwrap();
        let mut cursor = QueryCursor::new();
        let mut captures = cursor.captures(&query, tree.root_node(), source.as_bytes());
        let mut actual = Vec::new();
        while let Some((matched, index)) = captures.next() {
            let node = matched.captures()[*index].node;
            let extent = common::utf16_extent(node, source);
            if extent.0 >= tail_offset {
                actual.push(extent);
            }
        }
        assert_eq!(actual, expected, "{source}\n{}", tree.root_node().to_sexp());
    }
}
