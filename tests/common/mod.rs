//! Shared checks against Microsoft's parser. Sources are parsed, never executed.
use serde_json::Value;
use std::{path::Path, process::Command};
use tree_sitter::{Node, Parser};

pub type Extent = (usize, usize);
type CommandExtents = (Extent, Vec<Extent>);
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

fn commands(node: Node<'_>, source: &str, result: &mut Vec<CommandExtents>) {
    if node.kind() == "command" {
        let mut name = node.child_by_field_name("name").unwrap();
        if name.kind() == "invocation_operator_command" {
            name = name.child_by_field_name("command").unwrap();
        }
        let mut elements = vec![utf16_extent(name, source)];
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if matches!(child.kind(), "command_argument" | "command_parameter") {
                elements.push(utf16_extent(child, source));
            } else if child.kind() == "stop_parsing" {
                // The grammar keeps the opaque native-command tail together;
                // Microsoft's AST exposes the --% marker and its text as two
                // separate command elements. Compare those semantic extents.
                let text = &source[child.byte_range()];
                if let Some(tail) = text.strip_prefix("--%") {
                    let marker_end = child.start_byte() + 3;
                    elements.push((
                        source[..child.start_byte()].encode_utf16().count(),
                        source[..marker_end].encode_utf16().count(),
                    ));
                    let leading = tail.len() - tail.trim_start().len();
                    if leading != tail.len() {
                        elements.push((
                            source[..marker_end + leading].encode_utf16().count(),
                            source[..child.end_byte()].encode_utf16().count(),
                        ));
                    }
                } else {
                    // Keep malformed recovered nodes in the comparison rather
                    // than requiring children from an incomplete parse tree.
                    elements.push(utf16_extent(child, source));
                }
            }
        }
        result.push((utf16_extent(node, source), elements));
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        commands(child, source, result);
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
        commands(tree.root_node(), source, &mut actual);
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
                            element["start"].as_u64().unwrap() as usize,
                            element["end"].as_u64().unwrap() as usize,
                        )
                    })
                    .collect::<Vec<_>>();
                (extent, elements)
            })
            .collect();
        if actual != expected {
            failures.push(format!(
                "{}: command grouping differs\n{}\nexpected: {:?}\nactual: {:?}\n{}",
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
