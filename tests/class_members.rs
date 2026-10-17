//! Class member boundaries and declared names match Microsoft's parser.
mod common;
#[path = "common/cst.rs"]
mod cst;

use serde_json::Value;

#[test]
fn class_member_delimiters_match_powershell() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/class_members.json")).unwrap();
    common::assert_syntax(&cases);
    let results = common::oracle(&cases, "tests/fixtures/class_members.json");
    common::assert_commands(&cases, &results);
    cst::assert_structure(&cases, &results);
}

#[test]
fn class_property_name_queries_capture_complete_variables() {
    use tree_sitter::{Query, QueryCursor, StreamingIterator};

    let source = concat!(
        "class C {\n",
        "  $plain = 1\n",
        "  static ${x-y}\n",
        "  [string] ${x:y:z} = 'value'\n",
        "  [Obsolete()] hidden ${scope`u{3a}x} = 1\n",
        "  $Σ = 1\n",
        "  ${x`}} = 1\n",
        "}\n",
    );
    let tree = common::parser().parse(source, None).unwrap();
    assert!(
        !tree.root_node().has_error(),
        "{}",
        tree.root_node().to_sexp()
    );
    let query = Query::new(
        &woodruffw_trough_tree_sitter_powershell::LANGUAGE.into(),
        "(class_property name: (variable) @name)",
    )
    .unwrap();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), source.as_bytes());
    let mut names = Vec::new();
    while let Some(matched) = matches.next() {
        for capture in matched.captures() {
            names.push(&source[capture.node.byte_range()]);
        }
    }
    assert_eq!(
        names,
        [
            "$plain",
            "${x-y}",
            "${x:y:z}",
            "${scope`u{3a}x}",
            "$Σ",
            "${x`}}",
        ]
    );
}
