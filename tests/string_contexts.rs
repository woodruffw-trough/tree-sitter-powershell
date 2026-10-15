//! Quoted interpolation is collected before PowerShell parses its nested script.
mod common;
#[path = "common/cst.rs"]
mod cst;

use serde_json::Value;
use std::collections::BTreeSet;
use tree_sitter::{InputEdit, Node, Point};

fn fixtures() -> Vec<Value> {
    serde_json::from_str(include_str!("fixtures/string_contexts.json")).unwrap()
}

fn interpolation_extents(
    node: Node<'_>,
    source: &str,
    result: &mut BTreeSet<(String, common::Extent)>,
) {
    if matches!(node.kind(), "variable" | "sub_expression") {
        result.insert((node.kind().to_owned(), common::utf16_extent(node, source)));
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        interpolation_extents(child, source, result);
    }
}

#[test]
fn string_contexts_match_powershell() {
    let cases = fixtures();
    common::assert_syntax(&cases);
    let results = common::oracle(&cases, "tests/fixtures/string_contexts.json");
    let mut parser = common::parser();
    for (case, result) in cases.iter().zip(&results) {
        if case["valid"] != true || case["name"] == "nested_command_name_doubled" {
            continue;
        }
        let source = case["source"].as_str().unwrap();
        let tree = parser.parse(source, None).unwrap();
        let mut actual = BTreeSet::new();
        interpolation_extents(tree.root_node(), source, &mut actual);
        let expected = result["ast"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|node| {
                let kind = match node["type"].as_str().unwrap() {
                    "VariableExpressionAst" => "variable",
                    "SubExpressionAst" => "sub_expression",
                    _ => return None,
                };
                Some((
                    kind.to_owned(),
                    (
                        node["start"].as_u64().unwrap() as usize,
                        node["end"].as_u64().unwrap() as usize,
                    ),
                ))
            })
            .collect();
        assert_eq!(
            actual, expected,
            "Interpolation differs for {}: {source}",
            case["name"]
        );
    }
    // PowerShell 7.5.4 loses a skipped-quote offset layer in this nested case.
    // Its source extents end before the physical command and interpolation close.
    // The independent CST assertions below cover that exact case instead.
    let reliable: Vec<_> = cases
        .iter()
        .zip(&results)
        .filter(|(case, _)| case["name"] != "nested_command_name_doubled")
        .map(|(case, result)| (case.clone(), result.clone()))
        .collect();
    let (cases, results): (Vec<_>, Vec<_>) = reliable.into_iter().unzip();
    common::assert_commands(&cases, &results);
    cst::assert_structure(&cases, &results);
}

fn find_kind<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    if node.kind() == kind {
        return Some(node);
    }
    let mut cursor = node.walk();
    node.named_children(&mut cursor)
        .find_map(|child| find_kind(child, kind))
}

#[test]
fn nested_command_keeps_physical_quote_extents() {
    let case = fixtures()
        .into_iter()
        .find(|case| case["name"] == "nested_command_name_doubled")
        .unwrap();
    let source = case["source"].as_str().unwrap();
    let mut parser = common::parser();
    let tree = parser.parse(source, None).unwrap();
    assert!(!tree.root_node().has_error(), "{source}");
    let command = find_kind(tree.root_node(), "command").unwrap();
    assert_eq!(&source[command.byte_range()], "foo\"\"\"\"bar baz\"\"\"\"");
    let name = command.child_by_field_name("name").unwrap();
    assert_eq!(name.byte_range(), command.byte_range());
    let outer = find_kind(tree.root_node(), "sub_expression").unwrap();
    let mut cursor = outer.walk();
    let nested = outer
        .named_children(&mut cursor)
        .find_map(|node| find_kind(node, "sub_expression"))
        .unwrap();
    assert_eq!(
        &source[nested.byte_range()],
        "$(foo\"\"\"\"bar baz\"\"\"\")"
    );
}

fn point(source: &str, offset: usize) -> Point {
    let prefix = &source.as_bytes()[..offset];
    Point::new(
        prefix.iter().filter(|&&c| c == b'\n').count(),
        prefix
            .iter()
            .rposition(|&c| c == b'\n')
            .map_or(offset, |i| offset - i - 1),
    )
}

fn fingerprint(node: Node<'_>, result: &mut String) {
    use std::fmt::Write;
    let mut cursor = node.walk();
    loop {
        let node = cursor.node();
        write!(
            result,
            "({:?}:{}:{}..{}:{:?}..{:?}:{}:{}",
            cursor.field_name(),
            node.kind(),
            node.start_byte(),
            node.end_byte(),
            node.start_position(),
            node.end_position(),
            node.is_error(),
            node.is_missing()
        )
        .unwrap();
        if cursor.goto_first_child() {
            continue;
        }
        result.push(')');
        loop {
            if cursor.goto_next_sibling() {
                break;
            }
            if !cursor.goto_parent() {
                return;
            }
            result.push(')');
        }
    }
}

#[test]
fn string_context_edits_match_fresh_parses() {
    let mut parser = common::parser();
    for case in fixtures() {
        let source = case["source"].as_str().unwrap();
        let original = parser.parse(source, None).unwrap();
        for start in source
            .char_indices()
            .map(|(i, _)| i)
            .chain(std::iter::once(source.len()))
        {
            let end = source[start..]
                .chars()
                .next()
                .map_or(start, |c| start + c.len_utf8());
            for (end, inserted) in [
                (start, "\r\n"),
                (start, "`"),
                (start, "\""),
                (start, "("),
                (start, ")"),
                (end, ""),
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
                // Trees may be reused with another parser instance; scanner state
                // must contain the complete context rather than an in-memory ID.
                let mut incremental_parser = common::parser();
                let incremental = incremental_parser.parse(&edited, Some(&old_tree)).unwrap();
                let fresh = parser.parse(&edited, None).unwrap();
                let mut actual = String::new();
                let mut expected = String::new();
                fingerprint(incremental.root_node(), &mut actual);
                fingerprint(fresh.root_node(), &mut expected);
                assert_eq!(
                    actual, expected,
                    "Incremental parse differs for {}: {edited}",
                    case["name"]
                );
            }
        }
    }
}

#[test]
fn deep_interpolation_has_no_fixed_frame_limit() {
    for (depth, mode) in [(65, 0), (512, 0), (2048, 0), (512, 1), (128, 2)] {
        let mut source = String::from("1");
        for layer in 0..depth {
            if mode == 1 || (mode == 2 && layer % 3 == 0) {
                source = format!("$({source})");
            }
            source = if mode != 0 && layer % 2 == 1 {
                format!("@\"\n$( {source} )\n\"@")
            } else {
                format!("\"$( {source} )\"")
            };
        }
        let mut parser = common::parser();
        let tree = parser.parse(&source, None).unwrap();
        assert!(
            !tree.root_node().has_error(),
            "{depth} nested interpolations"
        );
        let edited = format!("\n{source}");
        let mut old_tree = tree;
        old_tree.edit(&InputEdit {
            start_byte: 0,
            old_end_byte: 0,
            new_end_byte: 1,
            start_position: Point::new(0, 0),
            old_end_position: Point::new(0, 0),
            new_end_position: Point::new(1, 0),
        });
        let incremental = parser.parse(&edited, Some(&old_tree)).unwrap();
        let fresh = parser.parse(&edited, None).unwrap();
        assert!(!fresh.root_node().has_error());
        let mut actual = String::new();
        let mut expected = String::new();
        fingerprint(incremental.root_node(), &mut actual);
        fingerprint(fresh.root_node(), &mut expected);
        assert_eq!(
            actual, expected,
            "{depth} nested interpolations after newline insertion"
        );
    }
}

#[test]
fn scanner_recovery_makes_progress() {
    use std::ops::ControlFlow;
    use tree_sitter::{ParseOptions, ParseState};

    for (source, error) in [
        ("\"$(('(')\"", true),
        ("\"$(if($true)x{1})\"", true),
        ("\"$(1 <# ) #>)\"", true),
        ("cmd $x`\n", false),
    ] {
        let mut parser = common::parser();
        let mut callbacks = 0;
        let mut progress = |_: &ParseState| {
            callbacks += 1;
            if callbacks > 4096 {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        };
        let mut input = |offset: usize, _| source.as_bytes().get(offset..).unwrap_or(&[]);
        let tree = parser
            .parse_with_options(
                &mut input,
                None,
                Some(ParseOptions::new().progress_callback(&mut progress)),
            )
            .expect("Scanner recovery must make progress");
        assert_eq!(tree.root_node().has_error(), error, "{source}");
        if !error {
            let argument = find_kind(tree.root_node(), "command_argument").unwrap();
            assert_eq!(argument.byte_range(), 4..source.len());
            let variable = find_kind(argument, "variable").unwrap();
            assert_eq!(variable.byte_range(), 4..6);
        }
    }
}

#[test]
fn collector_boundaries_cross_included_ranges() {
    use tree_sitter::Range;

    for (prefix, suffix) in [
        ("\"$(1", ")\""),
        ("\"$($x.L", "ength)\""),
        ("\"$(i", "f($true){1})\""),
        ("\"$([in", "t]1)\""),
        ("\"$(1 -e", "q 1)\""),
        ("\"$($x+", "+)\""),
        ("\"$(1 ?", "? 2)\""),
        ("\"$(foo\"\"bar", " baz\"\")\""),
        ("\"$(cmd --% \"\"a|", "b\"\"\n)\""),
    ] {
        for gap in ["EXCLUDED", "\nEXCLUDED\n"] {
            let source = format!("{prefix}{gap}{suffix}");
            let ranges = |text: &str, split: usize, tail: usize| {
                [
                    Range {
                        start_byte: 0,
                        end_byte: split,
                        start_point: Point::new(0, 0),
                        end_point: point(text, split),
                    },
                    Range {
                        start_byte: tail,
                        end_byte: text.len(),
                        start_point: point(text, tail),
                        end_point: point(text, text.len()),
                    },
                ]
            };
            let split = prefix.len();
            let tail = split + gap.len();
            let mut parser = common::parser();
            parser
                .set_included_ranges(&ranges(&source, split, tail))
                .unwrap();
            let original = parser.parse(&source, None).unwrap();
            assert!(!original.root_node().has_error(), "{source}");
            let subexpression = find_kind(original.root_node(), "sub_expression").unwrap();
            assert_eq!(subexpression.start_byte(), 1, "{source}");
            assert_eq!(subexpression.end_byte(), source.len() - 1, "{source}");
            for start in 0..=source.len() {
                if start > split && start < tail {
                    continue;
                }
                let edited = format!("{}x{}", &source[..start], &source[start..]);
                let mut old_tree = original.clone();
                old_tree.edit(&InputEdit {
                    start_byte: start,
                    old_end_byte: start,
                    new_end_byte: start + 1,
                    start_position: point(&source, start),
                    old_end_position: point(&source, start),
                    new_end_position: point(&edited, start + 1),
                });
                let split = split + usize::from(start <= split);
                let tail = tail + usize::from(start <= tail);
                let ranges = ranges(&edited, split, tail);
                parser.set_included_ranges(&ranges).unwrap();
                let incremental = parser.parse(&edited, Some(&old_tree)).unwrap();
                let mut fresh_parser = common::parser();
                fresh_parser.set_included_ranges(&ranges).unwrap();
                let fresh = fresh_parser.parse(&edited, None).unwrap();
                let mut actual = String::new();
                let mut expected = String::new();
                fingerprint(incremental.root_node(), &mut actual);
                fingerprint(fresh.root_node(), &mut expected);
                assert_eq!(actual, expected, "Included-range edit differs: {edited}");
            }
        }
    }
}
