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

#[test]
fn colon_function_names_keep_the_first_nonidentifier_literal() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/function_raw_names.json")).unwrap();
    common::assert_syntax(&cases);
    let results = common::oracle(&cases, "tests/fixtures/function_raw_names.json");
    common::assert_commands(&cases, &results);
    let mut parser = common::parser();
    for (case, result) in cases.iter().zip(results) {
        let source = case["source"].as_str().unwrap();
        let tree = parser.parse(source, None).unwrap();
        let function = tree.root_node().named_child(0).unwrap();
        assert_eq!(function.kind(), "function_definition", "{source}");
        let expected = result["ast"]
            .as_array()
            .unwrap()
            .iter()
            .find(|node| node["type"] == "FunctionDefinitionAst")
            .unwrap();
        let name = function.child_by_field_name("name").unwrap();
        assert_eq!(name.kind(), "function_name");
        assert_eq!(name.utf8_text(source.as_bytes()).unwrap(), expected["name"]);
        let keyword_end = source.find(' ').unwrap();
        let token = result["tokens"]
            .as_array()
            .unwrap()
            .iter()
            .find(|token| token["start"].as_u64().unwrap() as usize > keyword_end)
            .unwrap();
        let extent = |node: &Value| {
            (
                node["start"].as_u64().unwrap() as usize,
                node["end"].as_u64().unwrap() as usize,
            )
        };
        assert_eq!(
            common::utf16_extent(name, source),
            extent(token),
            "{source}"
        );
        let body = function.child_by_field_name("body").unwrap();
        assert_eq!(
            common::utf16_extent(body, source),
            extent(&expected["fields"]["body"]),
            "{source}"
        );
        let mut actual_parameters = Vec::new();
        if let Some(parameters) = function.child_by_field_name("parameters") {
            let mut cursor = parameters.walk();
            for parameter in parameters.named_children(&mut cursor) {
                if parameter.kind() == "parameter" {
                    actual_parameters.push(common::utf16_extent(parameter, source));
                }
            }
        }
        let expected_parameters: Vec<_> = expected["fields"]["parameters"]
            .as_array()
            .map(|parameters| parameters.iter().map(extent).collect())
            .unwrap_or_default();
        assert_eq!(actual_parameters, expected_parameters, "{source}");
    }
}
