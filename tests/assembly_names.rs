mod common;
#[path = "common/cst.rs"]
mod cst;

use serde_json::Value;

#[test]
fn assembly_name_boundaries_match_powershell() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/assembly_names.json")).unwrap();
    common::assert_syntax(&cases);
    let results = common::oracle(&cases, "tests/fixtures/assembly_names.json");
    common::assert_commands(&cases, &results);
    cst::assert_structure(&cases, &results);
}

#[test]
fn assembly_node_spans_complete_metadata() {
    fn assembly_node(node: tree_sitter::Node<'_>) -> Option<tree_sitter::Node<'_>> {
        if node.kind() == "assembly_name" {
            return Some(node);
        }
        let mut cursor = node.walk();
        node.named_children(&mut cursor).find_map(assembly_node)
    }

    let mut parser = common::parser();
    for (source, assembly) in [
        ("[int, x, y = z]", ", x, y = z"),
        ("[List[[int, x, y = z]]]", ", x, y = z"),
        ("[int, x[y]", ", x[y"),
        ("[int, x, y =]", ", x, y ="),
        ("[int, x, y\u{00a0}=\u{2028}z]", ", x, y\u{00a0}=\u{2028}z"),
    ] {
        let tree = parser.parse(source, None).unwrap();
        assert!(!tree.root_node().has_error(), "{source}");
        let node = assembly_node(tree.root_node()).unwrap();
        let start = source.find(assembly).unwrap();
        assert_eq!(node.byte_range(), start..start + assembly.len(), "{source}");
        assert_eq!(node.utf8_text(source.as_bytes()).unwrap(), assembly);
        let literal = node.parent().unwrap();
        assert_eq!(literal.kind(), "type_literal");
        assert_eq!(source.as_bytes()[literal.start_byte()], b'[');
        assert_eq!(source.as_bytes()[literal.end_byte() - 1], b']');
    }
}
