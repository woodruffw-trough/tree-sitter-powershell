//! Generic function names and .NET horizontal whitespace boundaries.
mod common;

use serde_json::Value;

fn fixtures() -> Vec<Value> {
    serde_json::from_str(include_str!("fixtures/names.json")).unwrap()
}

#[test]
fn fixture_syntax() {
    common::assert_syntax(&fixtures());
}

#[test]
fn powershell_parser_agrees() {
    let cases = fixtures();
    let results = common::oracle(&cases, "tests/fixtures/names.json");
    common::assert_commands(&cases, &results);
}
