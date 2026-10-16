//! Command tokens retain PowerShell's context-dependent lexical boundaries.
mod common;
#[path = "common/cst.rs"]
mod cst;

use serde_json::Value;
use tree_sitter::{InputEdit, Node, Point, Query, QueryCursor, StreamingIterator};

fn fixtures() -> Vec<Value> {
    serde_json::from_str(include_str!("fixtures/command_contexts.json")).unwrap()
}

#[test]
fn command_token_ranges_and_grouping_match_powershell() {
    let cases = fixtures();
    common::assert_syntax(&cases);
    let results = common::oracle(&cases, "tests/fixtures/command_contexts.json");
    common::assert_commands(&cases, &results);
    cst::assert_structure(&cases, &results);
}

#[test]
fn static_splat_restrictions_preserve_command_syntax() {
    let mut cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/splat_semantics.json")).unwrap();
    let mut results = common::oracle(&cases, "tests/fixtures/splat_semantics.json");
    for (case, result) in cases.iter_mut().zip(&mut results) {
        let errors: Vec<_> = result["errors"]
            .as_array()
            .unwrap()
            .iter()
            .map(|error| error["id"].as_str().unwrap())
            .collect();
        let expected: &[&str] = if case["valid"] == true {
            &[]
        } else {
            &["SplattingNotPermitted"]
        };
        assert_eq!(errors, expected, "{}", case["name"]);
        // SemanticChecks rejects splats after constructing the complete AST.
        // Keep the destination and parameter-value syntax available to tools.
        case["valid"] = true.into();
        result["valid"] = true.into();
    }
    common::assert_syntax(&cases);
    common::assert_commands(&cases, &results);
    cst::assert_structure(&cases, &results);
}

#[test]
fn command_roles_keep_complete_literal_tokens() {
    let source = "cmd -- -foo$x -bar.baz\ncmd -p:-foo.bar\ncmd >-foo:bar\n& 2>&1 x\n& -- -foo.bar\n& --% a|next";
    let tree = common::parser().parse(source, None).unwrap();
    assert!(
        !tree.root_node().has_error(),
        "{}",
        tree.root_node().to_sexp()
    );
    let mut cursor = tree.root_node().walk();
    let commands: Vec<_> = tree
        .root_node()
        .named_children(&mut cursor)
        .filter(|node| node.kind() == "pipeline")
        .flat_map(|node| {
            let mut cursor = node.walk();
            node.children_by_field_name("elements", &mut cursor)
                .collect::<Vec<_>>()
        })
        .collect();
    assert_eq!(commands.len(), 7);
    let arguments = |command: Node<'_>| {
        let mut cursor = command.walk();
        command
            .named_children(&mut cursor)
            .filter(|node| node.kind() == "command_argument")
            .map(|node| source[node.byte_range()].to_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(arguments(commands[0]), ["-foo$x", "-bar", ".baz"]);
    let mut cursor = commands[1].walk();
    let parameter = commands[1]
        .named_children(&mut cursor)
        .find(|node| node.kind() == "command_parameter")
        .unwrap();
    assert_eq!(
        &source[parameter.child_by_field_name("value").unwrap().byte_range()],
        "-foo"
    );
    let mut cursor = commands[2].walk();
    let redirection = commands[2]
        .named_children(&mut cursor)
        .find(|node| node.kind() == "redirection")
        .unwrap();
    assert_eq!(
        &source[redirection
            .child_by_field_name("destination")
            .unwrap()
            .byte_range()],
        "-foo:"
    );
    for (command, expected_kind, expected_text) in [
        (commands[3], "command_argument", "2>&1"),
        (commands[4], "command_parameter", "--"),
        (commands[5], "command_argument", "--%"),
    ] {
        let invocation = command.child_by_field_name("name").unwrap();
        let target = invocation.child_by_field_name("command").unwrap();
        assert_eq!(target.kind(), expected_kind);
        assert_eq!(&source[target.byte_range()], expected_text);
    }
    assert_eq!(arguments(commands[4]), ["-foo", ".bar"]);
    assert_eq!(arguments(commands[5]), ["a"]);
    let language = woodruffw_trough_tree_sitter_powershell::LANGUAGE.into();
    let query = Query::new(
        &language,
        "(invocation_operator_command command: [(command_argument) (command_parameter)] @target)",
    )
    .unwrap();
    let mut query_cursor = QueryCursor::new();
    let mut captures = query_cursor.captures(&query, tree.root_node(), source.as_bytes());
    let mut targets = Vec::new();
    while let Some((matched, index)) = captures.next() {
        targets.push(&source[matched.captures()[*index].node.byte_range()]);
    }
    assert_eq!(targets, ["2>&1", "--", "--%"]);
}

fn point(source: &str, offset: usize) -> Point {
    let prefix = &source.as_bytes()[..offset];
    let row = prefix.iter().filter(|&&byte| byte == b'\n').count();
    let column = prefix
        .iter()
        .rposition(|&byte| byte == b'\n')
        .map_or(offset, |position| offset - position - 1);
    Point::new(row, column)
}

fn fingerprint(node: Node<'_>, output: &mut String) {
    use std::fmt::Write;
    write!(
        output,
        "({}:{}..{}:{:?}..{:?}:{}:{}",
        node.kind(),
        node.start_byte(),
        node.end_byte(),
        node.start_position(),
        node.end_position(),
        node.is_missing(),
        node.is_error()
    )
    .unwrap();
    let mut cursor = node.walk();
    for (index, child) in node.children(&mut cursor).enumerate() {
        write!(output, "{:?}", node.field_name_for_child(index as u32)).unwrap();
        fingerprint(child, output);
    }
    output.push(')');
}

#[test]
fn command_token_boundary_edits_match_fresh_parses() {
    for source in [
        "cmd -- -foo.bar -foo$x\n",
        "1.++\n",
        "++[int]1\n",
        "1. + ++$y\n",
        "1.${name}.n next | Out-Null\n",
        ".1.${name}\n",
        "1+${name}\n",
        "cmd 1.${name}\n",
        "cmd $x`\r\n-Name:1\n",
        "cmd a${x}tail`\r\n| next\n",
        "cmd -p:-foo:bar,baz\n",
        "cmd >-foo.bar\n",
        "& 2>&1 x\n",
        "& -- -foo.bar\n",
        "& --% a|next\n",
        "cmd `  -P v\n",
        "function `  f {}\n",
        "data `  x {1}\n",
        "cmd a$ $x$ --%$ -$-%\n",
        "\"$(cmd -- -foo.bar)\"",
        "\"$(cmd `  -P v)\"",
        "<#comment#> < next\n",
        "& <#comment#> < next\n",
        "\"$(& <#comment#> < next)\"",
    ] {
        let parser = &mut common::parser();
        let original = parser.parse(source, None).unwrap();
        assert!(!original.root_node().has_error(), "{source}");
        let boundaries: Vec<_> = source
            .char_indices()
            .map(|(index, _)| index)
            .chain(std::iter::once(source.len()))
            .collect();
        for (index, &start) in boundaries.iter().enumerate() {
            for replacement in ["", "$", ":", "` ", ".", "|", ",", "\n", "界"] {
                let end = if replacement.is_empty() {
                    let Some(&end) = boundaries.get(index + 1) else {
                        continue;
                    };
                    end
                } else {
                    start
                };
                let edited = format!("{}{}{}", &source[..start], replacement, &source[end..]);
                let mut old = original.clone();
                old.edit(&InputEdit {
                    start_byte: start,
                    old_end_byte: end,
                    new_end_byte: start + replacement.len(),
                    start_position: point(source, start),
                    old_end_position: point(source, end),
                    new_end_position: point(&edited, start + replacement.len()),
                });
                // Reuse serialized scanner state in a separate parser instance.
                let incremental = common::parser().parse(&edited, Some(&old)).unwrap();
                let fresh = parser.parse(&edited, None).unwrap();
                let mut actual = String::new();
                let mut expected = String::new();
                fingerprint(incremental.root_node(), &mut actual);
                fingerprint(fresh.root_node(), &mut expected);
                assert_eq!(
                    actual, expected,
                    "Edited {source:?} at {start}..{end} with {replacement:?}"
                );
            }
        }
    }
}
