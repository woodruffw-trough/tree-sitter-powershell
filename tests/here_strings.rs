use serde_json::Value;
use std::{collections::BTreeSet, path::Path, process::Command};
use tree_sitter::{InputEdit, Node, Parser, Point};

fn parser() -> Parser {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_powershell::LANGUAGE.into())
        .unwrap();
    parser
}

fn fixtures() -> Vec<Value> {
    serde_json::from_str(include_str!("fixtures/here_strings.json")).unwrap()
}

#[test]
fn here_string_syntax() {
    let mut parser = parser();
    for case in fixtures() {
        let source = case["source"].as_str().unwrap();
        let tree = parser.parse(source, None).unwrap();
        assert_eq!(
            !tree.root_node().has_error(),
            case["valid"].as_bool().unwrap(),
            "{}: {}\n{}",
            case["name"],
            source,
            tree.root_node().to_sexp()
        );
    }
}

fn interpolation_extents(
    node: Node<'_>,
    source: &str,
    extents: &mut BTreeSet<(String, usize, usize)>,
) {
    if matches!(node.kind(), "variable" | "sub_expression") {
        extents.insert((
            node.kind().to_owned(),
            source[..node.start_byte()].encode_utf16().count(),
            source[..node.end_byte()].encode_utf16().count(),
        ));
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        interpolation_extents(child, source, extents);
    }
}

#[test]
fn powershell_agrees_on_here_string_interpolation() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let pwsh = std::env::var_os("PWSH_BIN").unwrap_or_else(|| "pwsh".into());
    let output = Command::new(pwsh)
        .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-File"])
        .arg(root.join("scripts/oracle.ps1"))
        .arg("-Fixtures")
        .arg(root.join("tests/fixtures/here_strings.json"))
        .current_dir(root)
        .output()
        .expect("Install PowerShell 7.5.4 or set PWSH_BIN to its executable");
    assert!(
        output.status.success(),
        "PowerShell oracle failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let results: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(results["offsetEncoding"], "utf-16");
    let cases = fixtures();
    let results = results["results"].as_array().unwrap();
    assert_eq!(cases.len(), results.len());
    let mut parser = parser();
    for (case, result) in cases.iter().zip(results) {
        assert_eq!(case["name"], result["name"]);
        assert_eq!(case["valid"], result["valid"], "{}", case["name"]);
        if !case["valid"].as_bool().unwrap() {
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
                    node["start"].as_u64().unwrap() as usize,
                    node["end"].as_u64().unwrap() as usize,
                ))
            })
            .collect();
        assert_eq!(
            actual, expected,
            "Interpolation differs for {}",
            case["name"]
        );
    }
}

#[test]
fn here_string_body_preserves_whitespace_and_escapes() {
    let source = "@\"\n  lead $name\t $(1)`n\n\"@";
    let mut parser = parser();
    let tree = parser.parse(source, None).unwrap();
    assert!(!tree.root_node().has_error());
    let here = tree
        .root_node()
        .named_child(0)
        .unwrap()
        .named_child(0)
        .unwrap();
    assert_eq!(here.kind(), "expandable_here_string");
    let mut cursor = here.walk();
    let children: Vec<_> = here
        .named_children(&mut cursor)
        .map(|node| (node.kind(), &source[node.byte_range()]))
        .collect();
    assert_eq!(
        children,
        [
            ("string_content", "  lead "),
            ("variable", "$name"),
            ("string_content", "\t "),
            ("sub_expression", "$(1)"),
            ("escape_sequence", "`n"),
        ]
    );
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
fn here_string_edits_recover_without_retaining_scanner_state() {
    for source in [
        "@\"\n\"@",
        "@„\r$name\r”@",
        "@\"\n$( @\"\n$name\n\"@ )\n\"@",
        "@\"\nfirst`\n\"@ literal\n\"@",
        "@\"\nbody\n \"@",
        "@\" text\nbody\n\"@",
        "@\"\n$(1\n\"@",
    ] {
        let mut parser = parser();
        let original = parser.parse(source, None).unwrap();
        for start in source
            .char_indices()
            .map(|(index, _)| index)
            .chain(std::iter::once(source.len()))
        {
            let end = source[start..]
                .chars()
                .next()
                .map_or(start, |ch| start + ch.len_utf8());
            for (end, inserted) in [(start, "\r\n"), (start, "`"), (start, "\"@"), (end, "")] {
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
                assert_eq!(actual, expected, "Incremental parse disagrees for {edited}");
            }
        }
    }
}
