//! Shared checks against Microsoft's parser. Sources are parsed, never executed.
use serde_json::Value;
use std::{path::Path, process::Command};
use tree_sitter::{Node, Parser};

pub type Extent = (usize, usize);
type CommandExtents = (Extent, Vec<(bool, Extent)>);
const POWERSHELL_VERSION: &str = "7.5.4";

pub fn parser() -> Parser {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_powershell::LANGUAGE.into())
        .unwrap();
    parser
}

pub fn failure_summary(failures: &[String]) -> String {
    let limit = std::env::var("PWSH_TEST_FAILURE_LIMIT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(20);
    let shown = failures.len().min(limit);
    format!(
        "{} fixture failures (showing {}):\n\n{}",
        failures.len(),
        shown,
        failures[..shown].join("\n\n")
    )
}

pub fn assert_syntax(cases: &[Value]) {
    let mut parser = parser();
    let mut failures = Vec::new();
    for case in cases {
        let source = case["source"].as_str().unwrap();
        let tree = parser.parse(source, None).unwrap();
        let valid = !tree.root_node().has_error();
        if valid != case["valid"].as_bool().unwrap() {
            if source.len() > 4096 {
                let mut errors = Vec::new();
                syntax_errors(tree.root_node(), source, &mut errors);
                failures.push(format!(
                    "{}: expected valid={}, got valid={} ({} bytes)\n{}",
                    case["name"],
                    case["valid"],
                    valid,
                    source.len(),
                    failure_summary(&errors)
                ));
                continue;
            }
            failures.push(format!(
                "{}: expected valid={}, got valid={}\n{}\n{}",
                case["name"],
                case["valid"],
                valid,
                source,
                tree.root_node().to_sexp()
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failure_summary(&failures));
}

fn syntax_errors(node: Node<'_>, source: &str, result: &mut Vec<String>) {
    if node.is_error() || node.is_missing() {
        let preview = source[node.byte_range()]
            .chars()
            .take(120)
            .collect::<String>();
        result.push(format!("{} {:?}: {preview:?}", node.kind(), node.range()));
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        syntax_errors(child, source, result);
    }
}

pub fn oracle(cases: &[Value], fixture_path: &str) -> Vec<Value> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let pwsh = std::env::var_os("PWSH_BIN").unwrap_or_else(|| "pwsh".into());
    let output = Command::new(pwsh)
        .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-File"])
        .arg(root.join("scripts/oracle.ps1"))
        .arg("-Fixtures")
        .arg(root.join(fixture_path))
        .current_dir(root)
        .output()
        .expect("Install PowerShell 7.5.4 or set PWSH_BIN to its executable");
    assert!(
        output.status.success(),
        "PowerShell oracle failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let oracle: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(oracle["powershellVersion"], POWERSHELL_VERSION);
    assert_eq!(oracle["offsetEncoding"], "utf-16");
    let results = oracle["results"].as_array().unwrap();
    assert_eq!(results.len(), cases.len());
    let mut failures = Vec::new();
    for (case, result) in cases.iter().zip(results) {
        assert_eq!(result["name"], case["name"]);
        assert_eq!(result["source"], case["source"]);
        if result["valid"] != case["valid"] {
            failures.push(format!(
                "{}: oracle expectation is stale\n{}\n{}",
                case["name"], case["source"], result["errors"]
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failure_summary(&failures));
    results.clone()
}

pub fn utf16_extent(node: Node<'_>, source: &str) -> Extent {
    // PowerShell/.NET reports UTF-16 offsets; tree-sitter reports UTF-8 bytes.
    (
        source[..node.start_byte()].encode_utf16().count(),
        source[..node.end_byte()].encode_utf16().count(),
    )
}

// Map offsets once for complete modules with thousands of command elements.
fn utf16_offsets(source: &str) -> Vec<usize> {
    let mut offsets = vec![0; source.len() + 1];
    let mut offset = 0;
    for (byte, character) in source.char_indices() {
        offsets[byte] = offset;
        offset += character.len_utf16();
    }
    offsets[source.len()] = offset;
    offsets
}

fn mapped_extent(node: Node<'_>, offsets: &[usize]) -> Extent {
    (offsets[node.start_byte()], offsets[node.end_byte()])
}

fn commands(node: Node<'_>, source: &str, offsets: &[usize], result: &mut Vec<CommandExtents>) {
    if node.kind() == "command" {
        let mut name = node.child_by_field_name("name").unwrap();
        if name.kind() == "invocation_operator_command" {
            name = name.child_by_field_name("command").unwrap();
        }
        let mut elements = vec![(false, mapped_extent(name, offsets))];
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if matches!(child.kind(), "command_argument" | "command_parameter") {
                elements.push((
                    child.kind() == "command_parameter",
                    mapped_extent(child, offsets),
                ));
            } else if child.kind() == "stop_parsing" {
                // The grammar keeps the opaque native-command tail together;
                // Microsoft's AST exposes the --% marker and its text as two
                // separate command elements. Compare those semantic extents.
                let text = &source[child.byte_range()];
                if let Some(tail) = text.strip_prefix("--%") {
                    let marker_end = child.start_byte() + 3;
                    elements.push((false, (offsets[child.start_byte()], offsets[marker_end])));
                    let leading = tail.len() - tail.trim_start().len();
                    if leading != tail.len() {
                        elements.push((
                            false,
                            (offsets[marker_end + leading], offsets[child.end_byte()]),
                        ));
                    }
                } else {
                    // Keep malformed recovered nodes in the comparison rather
                    // than requiring children from an incomplete parse tree.
                    elements.push((false, mapped_extent(child, offsets)));
                }
            }
        }
        result.push((mapped_extent(node, offsets), elements));
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        commands(child, source, offsets, result);
    }
}

pub fn assert_commands(cases: &[Value], results: &[Value]) {
    let mut parser = parser();
    let mut failures = Vec::new();
    for (case, result) in cases.iter().zip(results) {
        if case["valid"] != true {
            continue;
        }
        let source = case["source"].as_str().unwrap();
        let tree = parser.parse(source, None).unwrap();
        let mut actual = Vec::new();
        let offsets = utf16_offsets(source);
        debug_assert_eq!(
            mapped_extent(tree.root_node(), &offsets),
            utf16_extent(tree.root_node(), source)
        );
        commands(tree.root_node(), source, &offsets, &mut actual);
        let expected: Vec<_> = result["commands"]
            .as_array()
            .unwrap()
            .iter()
            .map(|command| {
                let extent = (
                    command["start"].as_u64().unwrap() as usize,
                    command["end"].as_u64().unwrap() as usize,
                );
                let elements = command["elements"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|element| {
                        (
                            element["type"] == "CommandParameterAst",
                            (
                                element["start"].as_u64().unwrap() as usize,
                                element["end"].as_u64().unwrap() as usize,
                            ),
                        )
                    })
                    .collect::<Vec<_>>();
                (extent, elements)
            })
            .collect();
        if actual != expected {
            if source.len() > 4096 {
                let first_differences = expected
                    .iter()
                    .zip(&actual)
                    .enumerate()
                    .filter(|(_, (expected, actual))| expected != actual)
                    .take(10)
                    .collect::<Vec<_>>();
                failures.push(format!(
                    "{}: command grouping or parameter classification differs; expected {} commands, got {}\nfirst differences (index, (expected, actual)): {first_differences:?}",
                    case["name"], expected.len(), actual.len(),
                ));
                continue;
            }
            failures.push(format!(
                "{}: command grouping or parameter classification differs (true denotes a parameter)\n{}\nexpected: {:?}\nactual: {:?}\n{}",
                case["name"],
                source,
                expected,
                actual,
                tree.root_node().to_sexp()
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failure_summary(&failures));
}
