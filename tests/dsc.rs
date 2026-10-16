//! Standard DSC syntax and its concrete roles match Microsoft's parser.
mod common;
#[path = "common/cst.rs"]
mod cst;

use serde_json::Value;
use tree_sitter::{Query, QueryCursor, StreamingIterator};

#[test]
fn standard_dsc_syntax_and_structure_match_powershell() {
    let cases: Vec<Value> = serde_json::from_str(include_str!("fixtures/dsc.json")).unwrap();
    common::assert_syntax(&cases);
    let results = common::oracle(&cases, "tests/fixtures/dsc.json");
    common::assert_commands(&cases, &results);
    assert!(cst::assert_structure(&cases, &results) > 1400);
}

#[test]
fn dsc_queries_capture_complete_names_bodies_and_properties() {
    let source = "configuration Example { Import-DscResource -ModuleName PSDscResources; Node $AllNodes.NodeName { File \"asset-$name\" { DestinationPath = $path; Ensure = 'Present' } } }\nFile outside {}\nNode outside {}\nImport-DscResource outside";
    let mut parser = common::parser();
    let language = parser.language().unwrap();
    let query = Query::new(
        &language,
        "(node_statement keyword: _ @node.keyword name: _ @node.name body: (script_block) @node.body)
         (resource_statement type: (type_identifier) @resource.type
           name: _ @resource.name body: (resource_body) @resource.body)
         (resource_body properties: (hash_entry key: _ @property.key value: _ @property.value))
         (import_dsc_resource_statement keyword: _ @import.keyword elements: _ @import.element)
         (command name: (command_name) @command.name)",
    )
    .unwrap();
    let tree = parser.parse(source, None).unwrap();
    assert!(!tree.root_node().has_error());
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), source.as_bytes());
    let mut captures = Vec::new();
    while let Some(matched) = matches.next() {
        for capture in matched.captures() {
            captures.push((
                query.capture_names()[capture.index as usize],
                &source[capture.node.byte_range()],
            ));
        }
    }
    assert_eq!(
        captures
            .iter()
            .filter(|(role, _)| *role == "resource.type")
            .count(),
        1
    );
    assert_eq!(
        captures
            .iter()
            .filter(|(role, _)| *role == "node.name")
            .count(),
        1
    );
    for expected in [
        ("node.keyword", "Node"),
        ("node.name", "$AllNodes.NodeName"),
        ("import.keyword", "Import-DscResource"),
        ("import.element", "-ModuleName"),
        ("import.element", "PSDscResources"),
        ("command.name", "File"),
        ("command.name", "Node"),
        ("command.name", "Import-DscResource"),
        ("resource.type", "File"),
        ("resource.name", "\"asset-$name\""),
        (
            "resource.body",
            "{ DestinationPath = $path; Ensure = 'Present' }",
        ),
        ("property.key", "DestinationPath"),
        ("property.value", "$path"),
        ("property.key", "Ensure"),
        ("property.value", "'Present'"),
    ] {
        assert!(captures.contains(&expected), "Missing {expected:?}");
    }
}
