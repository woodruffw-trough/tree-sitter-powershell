//! Public flow statements retain the distinct operand roles in PowerShell's AST.
mod common;
#[path = "common/cst.rs"]
mod cst;

use serde_json::Value;
use tree_sitter::{Node, Query, QueryCursor, QueryErrorKind, StreamingIterator};

const FLOW_KINDS: [&str; 5] = [
    "return_statement",
    "throw_statement",
    "exit_statement",
    "break_statement",
    "continue_statement",
];

fn fixtures() -> Vec<Value> {
    serde_json::from_str(include_str!("fixtures/flow.json")).unwrap()
}

fn fields<'tree>(node: Node<'tree>, name: &str) -> Vec<Node<'tree>> {
    let mut cursor = node.walk();
    node.children_by_field_name(name, &mut cursor).collect()
}

fn flow_nodes<'tree>(node: Node<'tree>, result: &mut Vec<Node<'tree>>) {
    if FLOW_KINDS.contains(&node.kind()) {
        result.push(node);
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        flow_nodes(child, result);
    }
}

fn query_field<'tree>(node: Node<'tree>, source: &str, name: &str) -> Vec<Node<'tree>> {
    let language = tree_sitter_powershell::LANGUAGE.into();
    let query = Query::new(&language, &format!("({} {name}: _ @field)", node.kind())).unwrap();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, node, source.as_bytes());
    let mut fields = Vec::new();
    while let Some(matched) = matches.next() {
        fields.extend(matched.captures().iter().map(|capture| capture.node));
    }
    fields
}

#[test]
fn flow_syntax_and_semantic_relationships_match_powershell() {
    let cases = fixtures();
    common::assert_syntax(&cases);
    let results = common::oracle(&cases, "tests/fixtures/flow.json");
    common::assert_commands(&cases, &results);
    let checks = cst::assert_structure(&cases, &results);
    assert!(checks > 40, "Only {checks} semantic relationship checks");
}

#[test]
fn typed_operand_fields_and_queries_cover_complete_source_values() {
    let mut parser = common::parser();
    for case in fixtures() {
        let source = case["source"].as_str().unwrap();
        let tree = parser.parse(source, None).unwrap();
        let mut flows = Vec::new();
        flow_nodes(tree.root_node(), &mut flows);
        let expected = case["flows"].as_array().unwrap();
        assert_eq!(flows.len(), expected.len(), "{}", case["name"]);
        for (flow, expected) in flows.into_iter().zip(expected) {
            assert_eq!(flow.kind(), expected["kind"].as_str().unwrap());
            let keywords = fields(flow, "keyword");
            assert_eq!(keywords.len(), 1, "{}", flow.to_sexp());
            let keyword = keywords[0];
            let spelling = expected["keyword"].as_str().unwrap();
            assert_eq!(&source[keyword.byte_range()], spelling);
            assert_eq!(keyword.kind(), spelling.to_ascii_lowercase());
            assert!(!keyword.is_named());
            assert_eq!(
                keyword.byte_range(),
                flow.start_byte()..flow.start_byte() + spelling.len()
            );
            assert_eq!(query_field(flow, source, "keyword"), keywords);

            let (role, other_role) =
                if matches!(flow.kind(), "break_statement" | "continue_statement") {
                    ("label", "value")
                } else {
                    ("value", "label")
                };
            assert!(fields(flow, other_role).is_empty(), "{}", flow.to_sexp());
            let operands = fields(flow, role);
            assert_eq!(query_field(flow, source, role), operands);
            if let Some(text) = expected[role].as_str() {
                assert_eq!(operands.len(), 1, "{}", flow.to_sexp());
                let operand = operands[0];
                assert_eq!(&source[operand.byte_range()], text, "{}", case["name"]);
                let offset = source[flow.byte_range()].find(text).unwrap();
                let start = flow.start_byte() + offset;
                assert_eq!(operand.byte_range(), start..start + text.len());
                // Concrete background values retain '&'; the oracle comparison
                // above separately checks PowerShell's shorter semantic extents.
                assert_eq!(flow.end_byte(), operand.end_byte());
            } else {
                assert!(operands.is_empty(), "{}", flow.to_sexp());
                assert_eq!(flow.end_byte(), keyword.end_byte());
            }
        }
    }
}

#[test]
fn explicit_statement_queries_match_typed_flow_and_direct_assignment() {
    let source = "return\nthrow\nexit\nbreak\ncontinue\n$value = 1\n";
    let mut parser = common::parser();
    let tree = parser.parse(source, None).unwrap();
    assert!(!tree.root_node().has_error());
    let language = tree_sitter_powershell::LANGUAGE.into();
    // Hidden grammar helpers are not public query categories. Consumers query
    // the concrete statement kinds that carry the fields they need.
    let query = Query::new(
        &language,
        "[
            (return_statement)
            (throw_statement)
            (exit_statement)
            (break_statement)
            (continue_statement)
            (assignment_statement)
        ] @statement",
    )
    .unwrap();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), source.as_bytes());
    let mut statements = Vec::new();
    while let Some(matched) = matches.next() {
        for capture in matched.captures() {
            let node = capture.node;
            assert_eq!(node.parent(), Some(tree.root_node()));
            statements.push((node.kind(), &source[node.byte_range()]));
        }
    }
    assert_eq!(
        statements,
        [
            ("return_statement", "return"),
            ("throw_statement", "throw"),
            ("exit_statement", "exit"),
            ("break_statement", "break"),
            ("continue_statement", "continue"),
            ("assignment_statement", "$value = 1"),
        ]
    );
}

#[test]
fn public_metadata_exposes_scalar_flow_roles_and_hides_grammar_helpers() {
    let nodes: Vec<Value> = serde_json::from_str(tree_sitter_powershell::NODE_TYPES).unwrap();
    assert!(
        nodes
            .iter()
            .all(|node| node["type"] != "flow_control_statement")
    );
    let language = tree_sitter_powershell::LANGUAGE.into();
    for helper in ["_statement", "_expression", "_primary_expression"] {
        assert!(nodes.iter().all(|node| node["type"] != helper));
        let error = Query::new(&language, &format!("({helper}) @helper")).unwrap_err();
        assert_eq!(error.kind, QueryErrorKind::NodeType, "{helper}");
    }
    for kind in FLOW_KINDS {
        let node = nodes.iter().find(|node| node["type"] == kind).unwrap();
        assert_eq!(node["named"], true);
        let fields = node["fields"].as_object().unwrap();
        assert_eq!(fields["keyword"]["required"], true);
        assert_eq!(fields["keyword"]["multiple"], false);
        let (role, other_role) = if matches!(kind, "break_statement" | "continue_statement") {
            ("label", "value")
        } else {
            ("value", "label")
        };
        assert_eq!(fields[role]["required"], false, "{kind}.{role}");
        assert_eq!(fields[role]["multiple"], false, "{kind}.{role}");
        assert!(!fields.contains_key(other_role), "{kind}.{other_role}");
    }
}
