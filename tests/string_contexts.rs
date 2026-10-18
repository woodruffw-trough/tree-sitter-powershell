//! Quoted interpolation is collected before PowerShell parses its nested script.
#[path = "common/tree.rs"]
mod tree;
use tree::{fingerprint, point};

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

    for (prefix, suffix, subexpression_end) in [
        ("\"$(1", ")\"", None),
        ("\"$($x.L", "ength)\"", None),
        ("\"$(i", "f($true){1})\"", None),
        ("\"$([in", "t]1)\"", None),
        ("\"$(1 -e", "q 1)\"", None),
        ("\"$($x+", "+)\"", None),
        ("\"$(1 ?", "? 2)\"", None),
        ("\"$(foo\"\"bar", " baz\"\")\"", None),
        ("\"$(cmd --% \"\"a|", "b\"\"\n)\"", None),
        ("\"$(`(1).Le", "ngth)\"", Some(7)),
        ("\"$(`(1)\"\"$", "x\"\")\"", Some(7)),
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
            assert_eq!(
                subexpression.end_byte(),
                subexpression_end.unwrap_or(source.len() - 1),
                "{source}"
            );
            let interpolation = find_kind(original.root_node(), "interpolation").unwrap();
            assert_eq!(interpolation.end_byte(), source.len() - 1, "{source}");
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

#[test]
fn interpolation_expression_has_semantic_range_within_physical_collector() {
    use tree_sitter::{Query, QueryCursor, StreamingIterator};

    for (source, kind, physical, semantic, subexpression) in [
        ("\"$(1).Length\"", "sub_expression", 1..5, 1..5, 1..5),
        ("\"$(`(1).Length)\"", "member_access", 1..15, 1..14, 1..7),
        ("\"$(`(1)[0])\"", "element_access", 1..11, 1..10, 1..7),
        (
            "\"$(`(1).M(2))\"",
            "invocation_expression",
            1..13,
            1..12,
            1..7,
        ),
        ("\"$(`(1)\"\"$x\"\")\"", "sub_expression", 1..14, 1..7, 1..7),
        (
            "cmd prefix$(`(1).Length)",
            "member_access",
            10..24,
            10..23,
            10..16,
        ),
        (
            "cmd \"$(`(1). name)\"",
            "sub_expression",
            5..18,
            5..11,
            5..11,
        ),
        (
            "@\"\n$(`(1).Length)\n\"@",
            "member_access",
            3..17,
            3..16,
            3..9,
        ),
    ] {
        let mut parser = common::parser();
        let tree = parser.parse(source, None).unwrap();
        assert!(!tree.root_node().has_error(), "{source}");
        let interpolation = find_kind(tree.root_node(), "interpolation").unwrap();
        assert_eq!(interpolation.byte_range(), physical, "{source}");
        let expression = interpolation.child_by_field_name("expression").unwrap();
        assert_eq!(expression.kind(), kind, "{source}");
        assert_eq!(expression.byte_range(), semantic, "{source}");
        let sub = find_kind(expression, "sub_expression").unwrap();
        assert_eq!(sub.byte_range(), subexpression, "{source}");
        assert!(find_kind(interpolation, "variable").is_none(), "{source}");
        let language = woodruffw_trough_tree_sitter_powershell::LANGUAGE.into();
        let query = Query::new(&language, "(interpolation expression: (_) @expression)").unwrap();
        let mut cursor = QueryCursor::new();
        let mut captures = cursor.captures(&query, tree.root_node(), source.as_bytes());
        let (capture, index) = captures.next().unwrap();
        assert_eq!(capture.captures()[*index].node, expression, "{source}");
        assert!(captures.next().is_none(), "{source}");
    }
}

#[test]
fn interpolation_tail_comments_survive_incremental_reparse() {
    let source = "\"$(`(1)<#c#>..2)\"";
    let edited = "\"$(`(1)<#c#>..2)\r\n\"";
    let mut parser = common::parser();
    let mut original = parser.parse(source, None).unwrap();
    let start = source.len() - 1;
    original.edit(&InputEdit {
        start_byte: start,
        old_end_byte: start,
        new_end_byte: start + 2,
        start_position: point(source, start),
        old_end_position: point(source, start),
        new_end_position: point(edited, start + 2),
    });
    let mut incremental_parser = common::parser();
    let incremental = incremental_parser.parse(edited, Some(&original)).unwrap();
    let fresh = parser.parse(edited, None).unwrap();
    for tree in [&incremental, &fresh] {
        assert!(!tree.root_node().has_error());
        let comment = find_kind(tree.root_node(), "comment").unwrap();
        assert_eq!(comment.byte_range(), 7..12);
        assert_eq!(comment.utf8_text(edited.as_bytes()).unwrap(), "<#c#>");
    }
    let mut actual = String::new();
    let mut expected = String::new();
    fingerprint(incremental.root_node(), &mut actual);
    fingerprint(fresh.root_node(), &mut expected);
    assert_eq!(actual, expected);
}
