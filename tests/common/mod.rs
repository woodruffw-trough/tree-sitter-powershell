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
        .set_language(&woodruffw_trough_tree_sitter_powershell::LANGUAGE.into())
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
        let expected = syntax_valid(case);
        if valid != expected {
            if source.len() > 4096 {
                let mut errors = Vec::new();
                syntax_errors(tree.root_node(), source, &mut errors);
                failures.push(format!(
                    "{}: expected valid={}, got valid={} ({} bytes)\n{}",
                    case["name"],
                    expected,
                    valid,
                    source.len(),
                    failure_summary(&errors)
                ));
                continue;
            }
            failures.push(format!(
                "{}: expected valid={}, got valid={}\n{}\n{}",
                case["name"],
                expected,
                valid,
                source,
                tree.root_node().to_sexp()
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failure_summary(&failures));
}

pub fn syntax_valid(case: &Value) -> bool {
    case["syntax_valid"]
        .as_bool()
        .unwrap_or_else(|| case["valid"].as_bool().unwrap())
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
        if let Some(expected) = case["diagnostics"].as_array() {
            let actual: Vec<_> = result["errors"]
                .as_array()
                .unwrap()
                .iter()
                .map(|error| error["id"].clone())
                .collect();
            assert_eq!(&actual, expected, "{}: diagnostics differ", case["name"]);
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

pub fn discarded_switch_file(node: Node<'_>) -> bool {
    let Some(parent) = node
        .parent()
        .filter(|parent| parent.kind() == "switch_statement")
    else {
        return false;
    };
    let mut cursor = parent.walk();
    let files: Vec<_> = parent.children_by_field_name("file", &mut cursor).collect();
    // Microsoft's AST retains only the final -file operand. Field-query tests
    // separately verify every source operand kept in the concrete tree.
    files.contains(&node) && files.last().is_some_and(|last| *last != node)
}

fn commands(node: Node<'_>, source: &str, offsets: &[usize], result: &mut Vec<CommandExtents>) {
    if discarded_switch_file(node) {
        return;
    }
    if node.kind() == "command" {
        let mut name = node.child_by_field_name("name").unwrap();
        if name.kind() == "invocation_operator_command" {
            name = name.child_by_field_name("command").unwrap();
        }
        let verbatim_name = name.child(0).is_some_and(|marker| marker.kind() == "--%");
        let mut elements = vec![(
            name.kind() == "command_parameter",
            mapped_extent(name, offsets),
        )];
        let mut command_extent = mapped_extent(node, offsets);
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if matches!(child.kind(), "command_argument" | "command_parameter") {
                elements.push((
                    child.kind() == "command_parameter",
                    mapped_extent(child, offsets),
                ));
            } else if child.kind() == "verbatim_arguments" {
                // The grammar keeps the opaque native-command tail together;
                // Microsoft's AST exposes the --% marker and its text as two
                // separate command elements. Compare those semantic extents.
                if let Some(marker) = child.child(0).filter(|marker| marker.kind() == "--%") {
                    elements.push((false, mapped_extent(marker, offsets)));
                    if let Some(text) = child.child_by_field_name("text") {
                        elements.push((false, mapped_extent(text, offsets)));
                    } else {
                        // The AST synthesizes an empty argument after skipping
                        // horizontal whitespace. The CST ends at the marker
                        // and omits a text child when no source text follows.
                        let tail = &source[marker.end_byte()..];
                        let remaining = tail.trim_start_matches(|c: char| {
                            c.is_whitespace() && !matches!(c, '\r' | '\n')
                        });
                        let end = marker.end_byte() + tail.len() - remaining.len();
                        let offset = offsets[end];
                        elements.push((false, (offset, offset)));
                        command_extent.1 = offset;
                    }
                } else {
                    // Keep malformed recovered nodes in the comparison rather
                    // than requiring children from an incomplete parse tree.
                    elements.push((false, mapped_extent(child, offsets)));
                }
            }
        }
        if verbatim_name && elements.len() == 1 {
            // A verbatim marker may occupy the target of '&' or '.'. The
            // parser still synthesizes an empty argument after skipped spaces.
            let tail = &source[name.end_byte()..];
            let remaining =
                tail.trim_start_matches(|c: char| c.is_whitespace() && !matches!(c, '\r' | '\n'));
            let end = name.end_byte() + tail.len() - remaining.len();
            let offset = offsets[end];
            elements.push((false, (offset, offset)));
            command_extent.1 = offset;
        }
        result.push((command_extent, elements));
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
        if !syntax_valid(case) {
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
        let mut expected: Vec<_> = result["commands"]
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
        // PowerShell visits traps before the surrounding statement list,
        // regardless of their source position. Compare commands in source
        // order while preserving every command's direct element order.
        actual.sort_by_key(|(extent, _)| *extent);
        expected.sort_by_key(|(extent, _)| *extent);
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
