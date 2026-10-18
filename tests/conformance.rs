//! Compare syntax, command boundaries, and CST relationships with PowerShell.
mod common;
#[path = "common/cst.rs"]
mod cst;

use serde_json::Value;

fn assert_conformance(path: &str, fixtures: &str) -> usize {
    let cases: Vec<Value> = serde_json::from_str(fixtures).unwrap();
    common::assert_syntax(&cases);
    let results = common::oracle(&cases, path);
    common::assert_commands(&cases, &results);
    cst::assert_structure(&cases, &results)
}

#[test]
fn attribute_roles_match_powershell() {
    let checks = assert_conformance(
        "tests/fixtures/attribute_roles.json",
        include_str!("fixtures/attribute_roles.json"),
    );
    assert!(checks > 500, "only {checks} semantic relationship checks");
}

#[test]
fn colon_command_names_and_arguments_match_powershell() {
    let checks = assert_conformance(
        "tests/fixtures/colon_contexts.json",
        include_str!("fixtures/colon_contexts.json"),
    );
    assert!(checks > 400, "only {checks} semantic relationship checks");
}

#[test]
fn configuration_headers_and_block_statements_match_powershell() {
    assert_conformance(
        "tests/fixtures/configuration_conformance.json",
        include_str!("fixtures/configuration_conformance.json"),
    );
}

#[test]
fn control_contexts_match_powershell() {
    assert_conformance(
        "tests/fixtures/control_contexts.json",
        include_str!("fixtures/control_contexts.json"),
    );
}

#[test]
fn data_statements_match_powershell() {
    assert_conformance(
        "tests/fixtures/data_statements.json",
        include_str!("fixtures/data_statements.json"),
    );
}

#[test]
fn empty_arguments_match_powershell() {
    let checks = assert_conformance(
        "tests/fixtures/empty_arguments.json",
        include_str!("fixtures/empty_arguments.json"),
    );
    assert!(checks > 500, "only {checks} semantic relationship checks");
}

#[test]
fn expression_boundaries_match_powershell() {
    assert_conformance(
        "tests/fixtures/expression_boundaries.json",
        include_str!("fixtures/expression_boundaries.json"),
    );
}

#[test]
fn unary_and_attributed_expressions_match_powershell() {
    assert_conformance(
        "tests/fixtures/expression_conformance.json",
        include_str!("fixtures/expression_conformance.json"),
    );
}

#[test]
fn expression_contexts_match_powershell() {
    assert_conformance(
        "tests/fixtures/expression_contexts.json",
        include_str!("fixtures/expression_contexts.json"),
    );
}

#[test]
fn flow_control_continuation_and_structure_match_powershell() {
    assert_conformance(
        "tests/fixtures/flow_conformance.json",
        include_str!("fixtures/flow_conformance.json"),
    );
}

#[test]
fn for_header_syntax_and_clause_fields_match_powershell() {
    let checks = assert_conformance(
        "tests/fixtures/for_loops.json",
        include_str!("fixtures/for_loops.json"),
    );
    assert!(checks > 200, "only {checks} semantic relationship checks");
}

#[test]
fn keyword_flags_and_function_modes_match_powershell() {
    let checks = assert_conformance(
        "tests/fixtures/keyword_flags.json",
        include_str!("fixtures/keyword_flags.json"),
    );
    assert!(checks > 2000, "only {checks} semantic relationship checks");
}

#[test]
fn named_invocation_roles_match_powershell() {
    let checks = assert_conformance(
        "tests/fixtures/named_invocation_roles.json",
        include_str!("fixtures/named_invocation_roles.json"),
    );
    assert!(checks > 700, "only {checks} semantic relationship checks");
}

#[test]
fn leading_pipeline_trivia_matches_powershell() {
    assert_conformance(
        "tests/fixtures/pipeline_boundaries.json",
        include_str!("fixtures/pipeline_boundaries.json"),
    );
}

#[test]
fn signed_numeric_members_and_generic_words_match_powershell() {
    assert_conformance(
        "tests/fixtures/signed_numbers.json",
        include_str!("fixtures/signed_numbers.json"),
    );
}

#[test]
fn statement_syntax_and_fields_match_powershell() {
    assert_conformance(
        "tests/fixtures/statement_conformance.json",
        include_str!("fixtures/statement_conformance.json"),
    );
}

#[test]
fn mapped_semantic_relationships_match_powershell() {
    assert_conformance(
        "tests/fixtures/structure.json",
        include_str!("fixtures/structure.json"),
    );
}

#[test]
fn upstream_parser_inputs_match_powershell() {
    assert_conformance(
        "tests/upstream/parser_cases.json",
        include_str!("upstream/parser_cases.json"),
    );
}

#[test]
fn workflow_inline_script_contexts_match_powershell() {
    assert_conformance(
        "tests/fixtures/workflow.json",
        include_str!("fixtures/workflow.json"),
    );
}
