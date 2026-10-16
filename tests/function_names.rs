mod common;
#[path = "common/cst.rs"]
mod cst;

use serde_json::Value;

#[test]
fn literal_function_name_boundaries_match_powershell() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/function_names.json")).unwrap();
    common::assert_syntax(&cases);
    let results = common::oracle(&cases, "tests/fixtures/function_names.json");
    common::assert_commands(&cases, &results);
    cst::assert_structure(&cases, &results);
}

#[test]
fn leading_nul_stays_in_the_complete_function_name() {
    let mut parser = common::parser();
    for (keyword, name) in [
        ("function", "\0f"),
        ("filter", "\0"),
        ("function", "\0\"f\""),
        ("filter", "\0$f"),
    ] {
        let source = format!("{keyword} {name} {{}}");
        let tree = parser.parse(&source, None).unwrap();
        assert!(!tree.root_node().has_error(), "{source:?}");
        let function = tree.root_node().named_child(0).unwrap();
        assert_eq!(function.kind(), "function_definition");
        let actual = function.child_by_field_name("name").unwrap();
        let start = keyword.len() + 1;
        assert_eq!(actual.byte_range(), start..start + name.len());
        assert_eq!(actual.utf8_text(source.as_bytes()).unwrap(), name);
        assert_eq!(actual.kind(), "function_name");
    }
}

#[test]
fn interpolation_fragments_preserve_name_and_body_boundaries() {
    let mut parser = common::parser();
    for (keyword, name, parameters) in [
        ("function", "f${name}", ""),
        ("filter", "f${a b}", "($p)"),
        ("function", "f${a\"b}", ""),
        ("function", "f${a`u{7d}b}", ""),
        ("filter", "${name}-tail", ""),
        ("function", "f$$", "()"),
    ] {
        let source = format!("{keyword} {name}{parameters} {{}}; Write-Output after");
        let tree = parser.parse(&source, None).unwrap();
        let root = tree.root_node();
        assert!(!root.has_error(), "{source}");
        assert_eq!(root.named_child_count(), 2, "{source}");
        let function = root.named_child(0).unwrap();
        assert_eq!(function.kind(), "function_definition");
        let actual = function.child_by_field_name("name").unwrap();
        let start = keyword.len() + 1;
        assert_eq!(actual.kind(), "function_name");
        assert_eq!(actual.byte_range(), start..start + name.len());
        assert_eq!(actual.utf8_text(source.as_bytes()).unwrap(), name);
        let body = function.child_by_field_name("body").unwrap();
        assert_eq!(body.utf8_text(source.as_bytes()).unwrap(), "{}");
        let actual_parameters = function.child_by_field_name("parameters");
        if parameters.is_empty() {
            assert!(actual_parameters.is_none(), "{source}");
        } else {
            assert_eq!(
                actual_parameters
                    .unwrap()
                    .utf8_text(source.as_bytes())
                    .unwrap(),
                parameters
            );
        }
    }
}
