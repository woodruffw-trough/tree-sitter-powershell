//! Clause boundaries, keyword parameters, and parameter-block fallback.
mod common;
#[path = "common/cst.rs"]
mod cst;

use serde_json::Value;
use tree_sitter::{InputEdit, Node, Point, Query, QueryCursor, StreamingIterator};

#[test]
fn control_headers_and_clause_boundaries_match_powershell() {
    let mut cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/control_review.json")).unwrap();
    let mut results = common::oracle(&cases, "tests/fixtures/control_review.json");
    for (case, result) in cases.iter_mut().zip(&mut results) {
        if let Some(expected) = case["semantic_errors"].as_array() {
            let errors: Vec<_> = result["errors"]
                .as_array()
                .unwrap()
                .iter()
                .map(|error| error["id"].clone())
                .collect();
            assert_eq!(&errors, expected, "{}", case["name"]);
            assert!(errors.iter().all(|error| matches!(
                error.as_str(),
                Some("KeywordParameterReservedForFutureUse" | "ThrottleLimitRequiresParallelFlag")
            )));
            let foreach = result["ast"]
                .as_array()
                .unwrap()
                .iter()
                .find(|node| node["type"] == "ForEachStatementAst")
                .expect("PowerShell must construct the foreach AST");
            if case["parallel_only"] == true {
                assert!(foreach["fields"]["throttleLimit"].is_null());
            } else {
                assert!(foreach["fields"]["throttleLimit"].is_object());
            }
            // PowerShell still constructs the complete foreach AST before
            // rejecting its workflow-only flags. Compare that syntax and tree
            // only after checking these exact semantic diagnostics.
            case["valid"] = true.into();
            result["valid"] = true.into();
        }
    }
    common::assert_syntax(&cases);
    common::assert_commands(&cases, &results);
    assert!(cst::assert_structure(&cases, &results) > 1500);
}

fn find_kind<'tree>(node: Node<'tree>, kind: &str) -> Option<Node<'tree>> {
    if node.kind() == kind {
        return Some(node);
    }
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .find_map(|child| find_kind(child, kind))
}

#[test]
fn keyword_parameter_fields_retain_complete_values_and_colons() {
    let language = woodruffw_trough_tree_sitter_powershell::LANGUAGE.into();
    let mut parser = common::parser();
    for (source, kind, role, value, flag_kind, flag) in [
        (
            "switch -file:$path.Length {default {}}",
            "switch_statement",
            "file",
            "$path.Length",
            "-file",
            "-file:",
        ),
        (
            "data -s:foo {}",
            "data_statement",
            "supported_commands",
            "foo",
            "-supportedcommand",
            "-s:",
        ),
        (
            "foreach -parallel -t:(1,2) ($x in 3) {}",
            "foreach_parameter",
            "value",
            "(1,2)",
            "-throttlelimit",
            "-t:",
        ),
    ] {
        let tree = parser.parse(source, None).unwrap();
        assert!(!tree.root_node().has_error(), "{source}");
        let query = Query::new(&language, &format!("({kind} {role}: _ @value)")).unwrap();
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, tree.root_node(), source.as_bytes());
        let mut values = Vec::new();
        while let Some(matched) = matches.next() {
            values.extend(matched.captures().iter().map(|capture| capture.node));
        }
        assert_eq!(values.len(), 1, "{source}");
        assert_eq!(&source[values[0].byte_range()], value, "{source}");
        let flag_node = find_kind(tree.root_node(), flag_kind).unwrap();
        assert_eq!(&source[flag_node.byte_range()], flag, "{source}");
        assert!(!flag_node.is_named());
    }
}

fn point(source: &str, offset: usize) -> Point {
    let prefix = &source.as_bytes()[..offset];
    Point::new(
        prefix.iter().filter(|&&byte| byte == b'\n').count(),
        prefix
            .iter()
            .rposition(|&byte| byte == b'\n')
            .map_or(offset, |position| offset - position - 1),
    )
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
fn clause_and_header_edits_match_fresh_parses() {
    let mut parser = common::parser();
    for source in [
        "param a\n",
        "param\n($x)\n",
        "param(1)\n",
        "param <#x#> (1)\n",
        "1;param(1)\n",
        "function f {param{}}\n",
        "try {} catch [Exception] {} finally {}\n",
        "try {} catch<#x#>[Exception] {}\n",
        "if (1) {} elseif (2) {} else {}\n",
        "switch -file:$x.Length {1 {}}\n",
        "switch (1) {default\"x\" {} default$value {} default {}}\n",
        "switch (1) {; default {}}\n",
        "switch -file input.txt {\n; default {}}\n",
        "switch —r:(1) {1 {}}\n",
        "data -s:foo,$x.Name {}\n",
        "foreach -parallel -t:(1,2) ($x in 3) {}\n",
        "\"$(try {} catch [Exception] {})\"\n",
        "\"$(switch -file:a {1 {}})\"\n",
    ] {
        let original = parser.parse(source, None).unwrap();
        for start in source
            .char_indices()
            .map(|(offset, _)| offset)
            .chain(std::iter::once(source.len()))
        {
            let end = source[start..]
                .chars()
                .next()
                .map_or(start, |character| start + character.len_utf8());
            for (end, inserted) in [
                (start, " "),
                (start, "\r\n"),
                (start, "#"),
                (start, "`"),
                (start, ":"),
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
                let mut incremental_parser = common::parser();
                let incremental = incremental_parser.parse(&edited, Some(&old_tree)).unwrap();
                let fresh = parser.parse(&edited, None).unwrap();
                let mut actual = String::new();
                let mut expected = String::new();
                fingerprint(incremental.root_node(), &mut actual);
                fingerprint(fresh.root_node(), &mut expected);
                assert_eq!(actual, expected, "Edit differs: {edited}");
            }
        }
    }
}
