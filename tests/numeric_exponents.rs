//! Numeric signs and subtraction accept the same dashes as PowerShell's tokenizer.
mod common;
#[path = "common/cst.rs"]
mod cst;

use serde_json::Value;
use tree_sitter::{InputEdit, Node, Point};

fn numeric_tokens(node: Node<'_>, source: &str, tokens: &mut Vec<(String, common::Extent)>) {
    if matches!(node.kind(), "integer_literal" | "real_literal") {
        tokens.push((node.kind().to_owned(), common::utf16_extent(node, source)));
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        numeric_tokens(child, source, tokens);
    }
}

#[test]
fn exponent_kinds_ranges_and_grouping_match_powershell() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/numeric_exponents.json")).unwrap();
    common::assert_syntax(&cases);
    let results = common::oracle(&cases, "tests/fixtures/numeric_exponents.json");
    common::assert_commands(&cases, &results);
    cst::assert_structure(&cases, &results);

    let mut parser = common::parser();
    for (case, result) in cases.iter().zip(&results) {
        if case["valid"] != true {
            continue;
        }
        let source = case["source"].as_str().unwrap();
        let tree = parser.parse(source, None).unwrap();
        let mut actual = Vec::new();
        numeric_tokens(tree.root_node(), source, &mut actual);
        let expected: Vec<_> = result["tokens"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|token| token["kind"] == "Number")
            .map(|token| {
                let text = token["text"].as_str().unwrap();
                let mut start = token["start"].as_u64().unwrap() as usize;
                let end = token["end"].as_u64().unwrap() as usize;
                // The CST preserves a leading sign as a unary operator, while
                // PowerShell includes an adjacent sign in its Number token.
                if text.starts_with(['+', '-', '–', '—', '―']) {
                    start += 1;
                }
                let kind = if text.contains(['.', 'e', 'E']) {
                    "real_literal"
                } else {
                    "integer_literal"
                };
                (kind.to_owned(), (start, end))
            })
            .collect();
        assert_eq!(actual, expected, "{}\n{source}", case["name"]);
    }
}

fn point(source: &str, offset: usize) -> Point {
    let prefix = &source.as_bytes()[..offset];
    Point::new(
        prefix.iter().filter(|&&byte| byte == b'\n').count(),
        prefix
            .iter()
            .rposition(|&byte| byte == b'\n')
            .map_or(offset, |index| offset - index - 1),
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
    for child in node.children(&mut cursor) {
        fingerprint(child, output);
    }
    output.push(')');
}

#[test]
fn exponent_and_numeric_boundary_edits_match_fresh_parses() {
    let mut parser = common::parser();
    for source in [
        "1e-2",
        "Write-Output 1e-2",
        "[double]1e-2",
        "[T]::M(1e-2)",
        "1e-2.Length",
        "1e-2-1",
        "Write-Output 1-2",
        "-1e-2.Length",
        "+1e-2.Length",
        "[int]-1e-2.Length",
        "[int]+1e-2.Length",
        "Write-Output +1dd",
        "Write-Output -0bu",
        "-0bu",
    ] {
        let original = parser.parse(source, None).unwrap();
        for (start, _) in source
            .char_indices()
            .filter(|(_, c)| matches!(c, '+' | '-'))
        {
            for replacement in ["-", "–", "—", "―", "−", "+", "", "x", "`-", "\n"] {
                let end = start + 1;
                let edited = format!("{}{}{}", &source[..start], replacement, &source[end..]);
                let mut old_tree = original.clone();
                old_tree.edit(&InputEdit {
                    start_byte: start,
                    old_end_byte: end,
                    new_end_byte: start + replacement.len(),
                    start_position: point(source, start),
                    old_end_position: point(source, end),
                    new_end_position: point(&edited, start + replacement.len()),
                });
                let incremental = parser.parse(&edited, Some(&old_tree)).unwrap();
                let fresh = parser.parse(&edited, None).unwrap();
                let mut actual = String::new();
                let mut expected = String::new();
                fingerprint(incremental.root_node(), &mut actual);
                fingerprint(fresh.root_node(), &mut expected);
                assert_eq!(
                    actual, expected,
                    "Edit at {start}: {source:?} -> {edited:?}"
                );
            }
        }
    }
}
