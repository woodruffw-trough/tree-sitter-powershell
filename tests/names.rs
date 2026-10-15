//! Generic function names and .NET horizontal whitespace boundaries.
mod common;
#[path = "common/cst.rs"]
mod cst;

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
    let escaped_names = cases
        .into_iter()
        .zip(results)
        .filter(|(case, _)| {
            case["name"]
                .as_str()
                .unwrap()
                .contains("leading escaped name")
        })
        .unzip::<_, _, Vec<_>, Vec<_>>();
    cst::assert_structure(&escaped_names.0, &escaped_names.1);
}
