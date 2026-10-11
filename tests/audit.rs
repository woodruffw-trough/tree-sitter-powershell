mod common;

use serde_json::Value;
use tree_sitter::Node;

fn fixtures() -> Vec<Value> {
    serde_json::from_str(include_str!("fixtures/audit.json")).unwrap()
}

#[test]
fn audited_lexical_and_statement_boundaries_match_powershell() {
    let cases = fixtures();
    common::assert_syntax(&cases);
    let results = common::oracle(&cases, "tests/fixtures/audit.json");
    common::assert_commands(&cases, &results);
}

fn escape_texts(node: Node<'_>, source: &str, escapes: &mut Vec<String>) {
    if node.kind() == "escape_sequence" {
        escapes.push(source[node.byte_range()].to_owned());
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        escape_texts(child, source, escapes);
    }
}

#[test]
fn unicode_escape_nodes_cover_the_entire_sequence() {
    let source = "\"a`u{10FFFF}`n`U{110000}`u{0}\"\n@\"\n`u{1F600}\n\"@\n";
    let tree = common::parser().parse(source, None).unwrap();
    assert!(
        !tree.root_node().has_error(),
        "{}",
        tree.root_node().to_sexp()
    );
    let mut escapes = Vec::new();
    escape_texts(tree.root_node(), source, &mut escapes);
    assert_eq!(escapes, ["`u{10FFFF}", "`n", "`U", "`u{0}", "`u{1F600}"]);
}

#[test]
fn malformed_unicode_escapes_report_errors_in_expandable_strings() {
    let mut parser = common::parser();
    for source in [
        "\"`uX\"",
        "\"`u{}\"",
        "\"`u{110000}\"",
        "\"`u{0000000}\"",
        "\"`u{41\"",
        "@\"\n`u{}\n\"@",
    ] {
        let tree = parser.parse(source, None).unwrap();
        assert!(tree.root_node().has_error(), "{source}");
    }
}

#[test]
fn escaped_keywords_and_numbers_remain_command_names() {
    let mut parser = common::parser();
    for source in ["i`f arg", "i`u{66} arg", "1`u{32}"] {
        let tree = parser.parse(source, None).unwrap();
        assert!(
            !tree.root_node().has_error(),
            "{}",
            tree.root_node().to_sexp()
        );
        let pipeline = tree.root_node().named_child(0).unwrap();
        let command = pipeline.named_child(0).unwrap();
        assert_eq!(command.kind(), "command", "{source}");
        let name = command.child_by_field_name("name").unwrap();
        assert_eq!(
            &source[name.byte_range()],
            source.split(' ').next().unwrap()
        );
    }
}
