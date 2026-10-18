#[path = "common/tree.rs"]
mod tree;
use tree::{fingerprint, point};

mod common;

use serde_json::Value;
use tree_sitter::InputEdit;

fn fixtures() -> Vec<Value> {
    serde_json::from_str(include_str!("fixtures/long_tokens.json")).unwrap()
}

#[test]
fn generic_fixture_syntax() {
    common::assert_syntax(&fixtures());
}

#[test]
fn generic_token_boundaries_match_powershell() {
    let cases = fixtures();
    let results = common::oracle(&cases, "tests/fixtures/long_tokens.json");
    common::assert_commands(&cases, &results);
    let mut parser = common::parser();
    for case in cases {
        if let Some(kind) = case["kind"].as_str() {
            let source = case["source"].as_str().unwrap();
            let tree = parser.parse(source, None).unwrap();
            let expression = tree
                .root_node()
                .named_child(0)
                .unwrap()
                .child_by_field_name("elements")
                .unwrap()
                .child_by_field_name("expression")
                .unwrap();
            assert_eq!(expression.kind(), kind, "{}", case["name"]);
            assert_eq!(expression.byte_range(), 0..source.len());
        }
    }
}

#[test]
fn huge_numeric_looking_words_keep_their_command_extents() {
    let mut parser = common::parser();
    for punctuation in ['+', '.'] {
        let source = format!(
            "{}abc{}",
            "0".repeat(65_536),
            punctuation.to_string().repeat(65_536)
        );
        let tree = parser.parse(&source, None).unwrap();
        assert!(!tree.root_node().has_error());
        let command = tree
            .root_node()
            .named_child(0)
            .unwrap()
            .child_by_field_name("elements")
            .unwrap();
        assert_eq!(command.kind(), "command");
        assert_eq!(
            command.child_by_field_name("name").unwrap().byte_range(),
            0..source.len()
        );
    }
}

#[test]
fn long_token_edits_match_fresh_parses() {
    let examples = [
        format!("{}abc arg", "1".repeat(2048)),
        format!("Write-Output {}`u{{20}}tail", "x".repeat(2048)),
        format!("{}`\n-tail arg", "木".repeat(512)),
        format!("Write-Output {}`\r\nnext", "x".repeat(2048)),
    ];
    let mut parser = common::parser();
    for source in examples {
        let original = parser.parse(&source, None).unwrap();
        for requested in [0, 127, 128, 511, 512, 513, 1024, 2048, source.len()] {
            let start = source
                .char_indices()
                .map(|(index, _)| index)
                .find(|&index| index >= requested)
                .unwrap_or(source.len());
            let char_end = source[start..]
                .chars()
                .next()
                .map_or(start, |ch| start + ch.len_utf8());
            for (end, inserted) in [
                (start, "`"),
                (start, "\r\n"),
                (start, "`u{20}"),
                (char_end, ""),
            ] {
                let edited = format!("{}{}{}", &source[..start], inserted, &source[end..]);
                let mut old_tree = original.clone();
                old_tree.edit(&InputEdit {
                    start_byte: start,
                    old_end_byte: end,
                    new_end_byte: start + inserted.len(),
                    start_position: point(&source, start),
                    old_end_position: point(&source, end),
                    new_end_position: point(&edited, start + inserted.len()),
                });
                let incremental = parser.parse(&edited, Some(&old_tree)).unwrap();
                let fresh = parser.parse(&edited, None).unwrap();
                let mut actual = String::new();
                let mut expected = String::new();
                fingerprint(incremental.root_node(), &mut actual);
                fingerprint(fresh.root_node(), &mut expected);
                assert_eq!(
                    actual,
                    expected,
                    "Edit at {start} of {} bytes",
                    source.len()
                );
            }
        }
    }
}
