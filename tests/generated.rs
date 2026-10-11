//! Cross-product fixtures for PowerShell's context-sensitive lexer and grammar.
//! Their validity was obtained with Parser::ParseInput, never by execution.
mod common;

use serde_json::Value;

fn fixtures() -> Vec<Value> {
    serde_json::from_str(include_str!("fixtures/generated.json")).unwrap()
}

#[test]
fn generated_syntax() {
    common::assert_syntax(&fixtures());
}

#[test]
fn generated_oracle_expectations() {
    let expected = fixtures();
    let results = common::oracle(&expected, "tests/fixtures/generated.json");
    common::assert_commands(&expected, &results);
}
