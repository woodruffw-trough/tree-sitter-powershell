//! Argument-mode punctuation and composite values retain their oracle boundaries.
#[path = "common/tree.rs"]
mod tree;
use tree::{fingerprint, point};

mod common;

use serde_json::Value;
use tree_sitter::{InputEdit, Node};

fn fixtures() -> Vec<Value> {
    serde_json::from_str(include_str!("fixtures/redirections.json")).unwrap()
}

fn extent(value: &Value) -> common::Extent {
    (
        value["start"].as_u64().unwrap() as usize,
        value["end"].as_u64().unwrap() as usize,
    )
}

type Redirection = (String, common::Extent, Option<common::Extent>);

fn redirections(node: Node<'_>, source: &str, result: &mut Vec<Redirection>) {
    if node.kind() == "redirection" {
        let operator = node.child_by_field_name("operator").unwrap();
        let kind = match operator.kind() {
            "file_redirection_operator" => "FileRedirectionAst",
            "merging_redirection_operator" => "MergingRedirectionAst",
            other => panic!("Unexpected redirection operator: {other}"),
        };
        result.push((
            kind.to_owned(),
            common::utf16_extent(node, source),
            node.child_by_field_name("destination")
                .map(|destination| common::utf16_extent(destination, source)),
        ));
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        redirections(child, source, result);
    }
}

#[test]
fn argument_and_redirection_boundaries_match_powershell() {
    let cases = fixtures();
    common::assert_syntax(&cases);
    let results = common::oracle(&cases, "tests/fixtures/redirections.json");
    common::assert_commands(&cases, &results);
    let mut parser = common::parser();
    for (case, result) in cases.iter().zip(&results) {
        if case["valid"] != true {
            continue;
        }
        let source = case["source"].as_str().unwrap();
        let tree = parser.parse(source, None).unwrap();
        let mut actual = Vec::new();
        redirections(tree.root_node(), source, &mut actual);
        let expected: Vec<_> = result["ast"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|node| {
                let kind = node["type"].as_str().unwrap();
                match kind {
                    "FileRedirectionAst" => Some((
                        kind.to_owned(),
                        extent(node),
                        Some(extent(&node["fields"]["destination"])),
                    )),
                    "MergingRedirectionAst" => Some((kind.to_owned(), extent(node), None)),
                    _ => None,
                }
            })
            .collect();
        assert_eq!(actual, expected, "{}:\n{source}", case["name"]);
    }
}

#[test]
fn numeric_prefixes_remain_children_of_one_complete_argument() {
    let source = "cmd 1\"b\" -Name:2$x >3'log'";
    let tree = common::parser().parse(source, None).unwrap();
    assert!(!tree.root_node().has_error());
    let command = tree
        .root_node()
        .named_child(0)
        .unwrap()
        .child_by_field_name("elements")
        .unwrap();
    let argument = command.named_child(1).unwrap();
    let parameter = command.named_child(2).unwrap();
    let redirection = command.named_child(3).unwrap();
    for (node, expected_text, expected_children) in [
        (argument, "1\"b\"", vec!["bare_word", "expandable_string"]),
        (
            parameter.child_by_field_name("value").unwrap(),
            "2$x",
            vec!["bare_word", "variable"],
        ),
        (
            redirection.child_by_field_name("destination").unwrap(),
            "3'log'",
            vec!["bare_word", "single_quoted_string"],
        ),
    ] {
        assert_eq!(node.kind(), "command_argument");
        assert_eq!(&source[node.byte_range()], expected_text);
        let mut cursor = node.walk();
        let children: Vec<_> = node
            .named_children(&mut cursor)
            .map(|child| child.kind())
            .collect();
        assert_eq!(children, expected_children);
    }
}

#[test]
fn edits_between_generic_arguments_and_redirections_match_fresh_parses() {
    let mut parser = common::parser();
    for source in [
        "cmd 1>out\nnext",
        "cmd 12>out\nnext",
        "cmd $x>out\nnext",
        "cmd -Name>out\nnext",
        "cmd a\"b\">out\nnext",
        "cmd 1\"b\"\nnext",
        "foo\"a b\" arg\nnext",
        "cmd $x.m{2}\nnext",
    ] {
        let original = parser.parse(source, None).unwrap();
        for start in 0..=source.len() {
            for (end, inserted) in [
                (start, " "),
                (start, "\n"),
                (start, "\""),
                (start, "$"),
                (start, "`"),
                ((start + 1).min(source.len()), ""),
            ] {
                let edited = format!("{}{}{}", &source[..start], inserted, &source[end..]);
                let mut old_tree = original.clone();
                old_tree.edit(&InputEdit {
                    start_byte: start,
                    old_end_byte: end,
                    new_end_byte: start + inserted.len(),
                    start_position: point(source, start),
                    old_end_position: point(source, end),
                    new_end_position: point(&edited, start + inserted.len()),
                });
                let incremental = parser.parse(&edited, Some(&old_tree)).unwrap();
                let fresh = parser.parse(&edited, None).unwrap();
                let mut actual = String::new();
                let mut expected = String::new();
                fingerprint(incremental.root_node(), &mut actual);
                fingerprint(fresh.root_node(), &mut expected);
                assert_eq!(
                    actual, expected,
                    "Edit {start}..{end} to {inserted:?} in {source:?}"
                );
            }
        }
    }
}
