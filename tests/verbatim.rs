//! Opaque native-command arguments retain queryable marker and raw-text ranges.
mod common;

use serde_json::{Value, json};
use std::collections::BTreeMap;
use tree_sitter::{InputEdit, Node, Point, Query, QueryCursor, StreamingIterator};

type VerbatimExtents = (common::Extent, Option<common::Extent>);

fn fixtures() -> Vec<Value> {
    serde_json::from_str(include_str!("fixtures/verbatim.json")).unwrap()
}

fn extent(value: &Value) -> common::Extent {
    (
        value["start"].as_u64().unwrap() as usize,
        value["end"].as_u64().unwrap() as usize,
    )
}

fn utf16_text(source: &str, (start, end): common::Extent) -> String {
    String::from_utf16(&source.encode_utf16().collect::<Vec<_>>()[start..end]).unwrap()
}

fn collect_verbatim(node: Node<'_>, source: &str, actual: &mut Vec<VerbatimExtents>) {
    assert_ne!(node.kind(), "stop_parsing", "Legacy node in public tree");
    if node.kind() == "verbatim_arguments" {
        assert_eq!(node.parent().unwrap().kind(), "command");
        let marker = node.child(0).unwrap();
        assert_eq!(marker.kind(), "--%");
        assert!(!marker.is_named());
        assert_eq!(node.start_byte(), marker.start_byte());
        assert!(matches!(
            &source[marker.byte_range()],
            "--%" | "--`%" | "`--%"
        ));
        let mut cursor = node.walk();
        let texts: Vec<_> = node.children_by_field_name("text", &mut cursor).collect();
        assert!(texts.len() <= 1, "Text must be an optional scalar field");
        let text = texts.first().copied();
        assert_eq!(node.named_child_count(), usize::from(text.is_some()));
        if let Some(text) = text {
            assert_eq!(text.kind(), "string_content");
            assert_eq!(text.child_count(), 0, "Raw text must remain opaque");
            assert!(marker.end_byte() <= text.start_byte());
            assert_eq!(node.end_byte(), text.end_byte());
            assert!(!source[text.byte_range()].is_empty());
        } else {
            assert_eq!(node.end_byte(), marker.end_byte());
        }
        actual.push((
            common::utf16_extent(marker, source),
            text.map(|text| common::utf16_extent(text, source)),
        ));
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_verbatim(child, source, actual);
    }
}

fn expected_verbatim(result: &Value) -> Vec<VerbatimExtents> {
    let source = result["source"].as_str().unwrap();
    let tokens = result["tokens"].as_array().unwrap();
    let mut expected = Vec::new();
    for command in result["commands"].as_array().unwrap() {
        let elements = command["elements"].as_array().unwrap();
        for (index, element) in elements.iter().enumerate().skip(1) {
            let marker = extent(element);
            if !matches!(utf16_text(source, marker).as_str(), "--%" | "--`%" | "`--%") {
                continue;
            }
            let marker_token = tokens
                .iter()
                .position(|token| token["kind"] == "Generic" && extent(token) == marker)
                .expect("The oracle marker must have a corresponding Generic token");
            let body = extent(&elements[index + 1]);
            let body_token = &tokens[marker_token + 1];
            assert_eq!(body_token["kind"], "Generic");
            assert_eq!(extent(body_token), body);
            assert_eq!(
                body_token["text"],
                utf16_text(source, body),
                "The token must preserve the complete raw body"
            );
            assert_eq!(command["end"], json!(body.1));
            expected.push((marker, (body.0 != body.1).then_some(body)));
            // A raw body can itself spell --%; only the first marker starts a tail.
            break;
        }
    }
    expected
}

#[test]
fn verbatim_syntax_ranges_and_command_grouping_match_live_powershell() {
    let cases = fixtures();
    common::assert_syntax(&cases);
    let results = common::oracle(&cases, "tests/fixtures/verbatim.json");
    common::assert_commands(&cases, &results);
    let mut parser = common::parser();
    for (case, result) in cases.iter().zip(&results) {
        if case["valid"] != true {
            continue;
        }
        let source = case["source"].as_str().unwrap();
        let tree = parser.parse(source, None).unwrap();
        let mut actual = Vec::new();
        collect_verbatim(tree.root_node(), source, &mut actual);
        let expected = expected_verbatim(result);
        assert_eq!(
            actual,
            expected,
            "{}: marker/body extents differ\n{source}\n{}",
            case["name"],
            tree.root_node().to_sexp()
        );
        let texts: Vec<_> = actual
            .iter()
            .map(|(_, body)| body.map(|body| utf16_text(source, body)))
            .collect();
        let expected_texts: Vec<_> = case["verbatim"]
            .as_array()
            .unwrap()
            .iter()
            .map(|text| text.as_str().map(str::to_owned))
            .collect();
        assert_eq!(texts, expected_texts, "{}", case["name"]);
    }
}

#[test]
fn public_verbatim_fields_capture_complete_raw_text_in_nested_commands() {
    let source = concat!(
        "function F {\n native -Before value --%  $x (raw) \"a|b\"  | next\n}\n",
        "$(other --% 世界 && last\n)\n",
        "& \"native\" --% \n",
    );
    let language = woodruffw_trough_tree_sitter_powershell::LANGUAGE.into();
    let query = Query::new(
        &language,
        r#"
        (command
          (verbatim_arguments
            "--%" @marker
            text: (string_content) @text) @verbatim) @command
        (verbatim_arguments !text) @empty
        "#,
    )
    .unwrap();
    let tree = common::parser().parse(source, None).unwrap();
    assert!(
        !tree.root_node().has_error(),
        "{}",
        tree.root_node().to_sexp()
    );
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.captures(&query, tree.root_node(), source.as_bytes());
    let mut actual = BTreeMap::<String, Vec<String>>::new();
    while let Some((matched, index)) = matches.next() {
        let capture = matched.captures()[*index];
        actual
            .entry(query.capture_names()[capture.index as usize].to_owned())
            .or_default()
            .push(source[capture.node.byte_range()].to_owned());
    }
    let expected = [
        ("marker", vec!["--%", "--%"]),
        ("text", vec!["$x (raw) \"a|b\"  ", "世界 "]),
        ("verbatim", vec!["--%  $x (raw) \"a|b\"  ", "--% 世界 "]),
        (
            "command",
            vec![
                "native -Before value --%  $x (raw) \"a|b\"  ",
                "other --% 世界 ",
            ],
        ),
        ("empty", vec!["--%"]),
    ]
    .into_iter()
    .map(|(name, texts)| {
        (
            name.to_owned(),
            texts.into_iter().map(str::to_owned).collect::<Vec<_>>(),
        )
    })
    .collect::<BTreeMap<_, _>>();
    assert_eq!(actual, expected);
}

#[test]
fn public_schema_has_optional_scalar_text_and_anonymous_marker() {
    let nodes: Vec<Value> =
        serde_json::from_str(woodruffw_trough_tree_sitter_powershell::NODE_TYPES).unwrap();
    assert!(!nodes.iter().any(|node| node["type"] == "stop_parsing"));
    let verbatim = nodes
        .iter()
        .find(|node| node["type"] == "verbatim_arguments" && node["named"] == true)
        .unwrap();
    assert_eq!(verbatim["fields"].as_object().unwrap().len(), 1);
    assert_eq!(
        verbatim["fields"]["text"],
        json!({
            "multiple": false,
            "required": false,
            "types": [{"type": "string_content", "named": true}],
        })
    );
    assert!(
        nodes
            .iter()
            .any(|node| node["type"] == "--%" && node["named"] == false)
    );
    let command = nodes
        .iter()
        .find(|node| node["type"] == "command" && node["named"] == true)
        .unwrap();
    assert!(
        command["children"]["types"]
            .as_array()
            .unwrap()
            .contains(&json!({"type": "verbatim_arguments", "named": true}))
    );
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
fn targeted_marker_quote_boundary_and_incomplete_edits_match_fresh_trees() {
    let cases = fixtures();
    for (before, old, new, after) in [
        (
            "ordinary argument before pipe",
            "raw",
            "--% raw",
            "tail before pipe",
        ),
        (
            "tail before pipe",
            "--% ",
            "",
            "ordinary argument before pipe",
        ),
        (
            "tail before pipe",
            "raw",
            "",
            "empty tail after body deletion",
        ),
        (
            "empty tail after body deletion",
            "  ",
            " raw ",
            "tail before pipe",
        ),
        ("tail before pipe", "|", "&&", "tail before chain"),
        (
            "tail before chain",
            "&&",
            "&",
            "lone ampersand before command spelling",
        ),
        (
            "lone ampersand before command spelling",
            "&",
            "&&",
            "tail before chain",
        ),
        (
            "tail before pipe",
            "--%",
            "--",
            "marker replaced with end of parameters",
        ),
        ("tail before pipe", "raw", "变量😀", "unicode tail edit"),
        (
            "straight double quoted boundaries",
            "\"",
            "'",
            "double quote converted to single",
        ),
        (
            "double quote converted to single",
            "'",
            "\"",
            "straight double quoted boundaries",
        ),
        ("lf boundary", "\n", "\r\n", "crlf boundary"),
        (
            "missing command after pipe",
            "|",
            "| next",
            "tail before pipe",
        ),
        (
            "tail before chain",
            "next",
            "",
            "missing chain after rhs deletion",
        ),
        (
            "consumed closing brace remains incomplete",
            "}",
            "\n}",
            "completed consumed closing brace",
        ),
    ] {
        let initial = cases.iter().find(|case| case["name"] == before).unwrap();
        let completed = cases.iter().find(|case| case["name"] == after).unwrap();
        let source = initial["source"].as_str().unwrap();
        let mut parser = common::parser();
        let mut original = parser.parse(source, None).unwrap();
        assert_eq!(!original.root_node().has_error(), initial["valid"] == true);
        let start = source.find(old).unwrap();
        let end = start + old.len();
        let edited = format!("{}{}{}", &source[..start], new, &source[end..]);
        assert_eq!(edited, completed["source"].as_str().unwrap(), "{before}");
        original.edit(&InputEdit {
            start_byte: start,
            old_end_byte: end,
            new_end_byte: start + new.len(),
            start_position: point(source, start),
            old_end_position: point(source, end),
            new_end_position: point(&edited, start + new.len()),
        });
        let incremental = parser.parse(&edited, Some(&original)).unwrap();
        let fresh = parser.parse(&edited, None).unwrap();
        assert_eq!(!fresh.root_node().has_error(), completed["valid"] == true);
        let mut actual = String::new();
        let mut expected = String::new();
        fingerprint(incremental.root_node(), &mut actual);
        fingerprint(fresh.root_node(), &mut expected);
        assert_eq!(actual, expected, "Incremental tree differs for {after}");
    }
}
