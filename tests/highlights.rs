//! Execute the shipped editor query against independently checked PowerShell sources.

use std::{collections::BTreeSet, ops::Range};
use tree_sitter::{Parser, Point, Query, QueryCursor, StreamingIterator};

#[derive(Debug)]
struct Capture {
    name: String,
    bytes: Range<usize>,
}

fn point(source: &str, offset: usize) -> Point {
    let prefix = &source.as_bytes()[..offset];
    Point::new(
        prefix.iter().filter(|&&byte| byte == b'\n').count(),
        prefix
            .iter()
            .rposition(|&byte| byte == b'\n')
            .map_or(offset, |newline| offset - newline - 1),
    )
}

fn captures(source: &str) -> Vec<Capture> {
    let language = woodruffw_trough_tree_sitter_powershell::LANGUAGE.into();
    let mut parser = Parser::new();
    parser.set_language(&language).unwrap();
    let tree = parser.parse(source, None).unwrap();
    assert!(
        !tree.root_node().has_error(),
        "Highlight fixture must parse completely:\n{source}\n{}",
        tree.root_node().to_sexp()
    );

    let query = Query::new(
        &language,
        woodruffw_trough_tree_sitter_powershell::HIGHLIGHTS_QUERY,
    )
    .expect("The shipped highlighting query must compile");
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.captures(&query, tree.root_node(), source.as_bytes());
    let mut captures = Vec::new();
    while let Some((query_match, index)) = matches.next() {
        let capture = query_match.captures()[*index];
        let bytes = capture.node.byte_range();
        assert_eq!(capture.node.start_position(), point(source, bytes.start));
        assert_eq!(capture.node.end_position(), point(source, bytes.end));
        captures.push(Capture {
            name: query.capture_names()[capture.index as usize].to_owned(),
            bytes,
        });
    }
    captures
}

fn span(source: &str, context: &str, text: &str) -> (Range<usize>, String) {
    let matches = source.match_indices(context).collect::<Vec<_>>();
    assert_eq!(
        matches.len(),
        1,
        "Expected a unique source context: {context}"
    );
    let start = matches[0].0 + context.find(text).unwrap();
    (start..start + text.len(), text.to_owned())
}

fn assert_category(
    captures: &[Capture],
    source: &str,
    category: &str,
    expected: &[(Range<usize>, String)],
) {
    // Specific and general query patterns may intentionally overlap. Compare
    // distinct ranges rather than depending on the query engine's match order.
    let actual = captures
        .iter()
        .filter(|capture| capture.name == category)
        .map(|capture| {
            (
                capture.bytes.start,
                capture.bytes.end,
                source[capture.bytes.clone()].to_owned(),
            )
        })
        .collect::<BTreeSet<_>>();
    let expected = expected
        .iter()
        .map(|(bytes, text)| (bytes.start, bytes.end, text.clone()))
        .collect::<BTreeSet<_>>();
    assert_eq!(actual, expected, "Wrong @{category} captures in:\n{source}");
}

#[test]
fn declaration_and_member_roles_preserve_unicode_ranges() {
    let source = concat!(
        "CLASS Café {\n",
        "  [object]$Value = $seed\n",
        "  [object] Écho([object]$item) { RETURN $item }\n",
        "}\n",
        "$instance.Écho($argument)\n",
        "$instance.Value\n",
    );
    let captures = captures(source);

    assert_category(
        &captures,
        source,
        "type.definition",
        &[span(source, "CLASS Café", "Café")],
    );
    assert_category(
        &captures,
        source,
        "function.method",
        &[span(source, "Écho([object]$item)", "Écho")],
    );
    assert_category(
        &captures,
        source,
        "function.method.call",
        &[span(source, "$instance.Écho($argument)", "Écho")],
    );
    assert_category(&captures, source, "function.call", &[]);
    assert_category(
        &captures,
        source,
        "variable.parameter",
        &[span(source, "Écho([object]$item)", "$item")],
    );
    assert_category(
        &captures,
        source,
        "property",
        &[
            span(source, "$Value = $seed", "$Value"),
            // Both patterns match this range. The shipped query places the
            // method-call pattern last so the highlighter selects that role.
            span(source, "$instance.Écho($argument)", "Écho"),
            span(source, "$instance.Value", "Value"),
        ],
    );
    assert_category(
        &captures,
        source,
        "variable",
        &[
            span(source, "$Value = $seed", "$Value"),
            span(source, "$Value = $seed", "$seed"),
            span(source, "Écho([object]$item)", "$item"),
            span(source, "RETURN $item", "$item"),
            span(source, "$instance.Écho($argument)", "$instance"),
            span(source, "$instance.Écho($argument)", "$argument"),
            span(source, "$instance.Value", "$instance"),
        ],
    );
}

#[test]
fn splatted_formals_retain_the_parameter_role() {
    let source = "function f(@rest) { foreach (@item in 1) { $item } }";
    assert_category(
        &captures(source),
        source,
        "variable.parameter",
        &[span(source, "f(@rest)", "@rest")],
    );
}

#[test]
fn formal_parameters_do_not_capture_body_variables() {
    let source = concat!(
        "param([string]$scriptName)\n",
        "function Invoke-Demo([object]$formal) { $local = $formal }\n",
    );
    let captures = captures(source);
    assert_category(
        &captures,
        source,
        "variable.parameter",
        &[
            span(source, "param([string]$scriptName)", "$scriptName"),
            span(source, "Invoke-Demo([object]$formal)", "$formal"),
        ],
    );
    assert_category(
        &captures,
        source,
        "function",
        &[span(source, "function Invoke-Demo", "Invoke-Demo")],
    );
}

#[test]
fn enum_names_and_members_have_distinct_roles() {
    let source = "ENUM Mode { First = 1; Second = 2 }\n";
    let captures = captures(source);
    assert_category(
        &captures,
        source,
        "type.definition",
        &[span(source, "ENUM Mode", "Mode")],
    );
    assert_category(
        &captures,
        source,
        "constant",
        &[
            span(source, "First = 1", "First"),
            span(source, "Second = 2", "Second"),
        ],
    );
}

#[test]
fn flow_control_keywords_keep_their_case_and_roles() {
    let source = "while ($true) { BREAK; CONTINUE }\nRETURN $value\nTHROW $error\nEXIT 1\n";
    let captures = captures(source);
    assert_category(
        &captures,
        source,
        "keyword.repeat",
        &[
            span(source, "while ($true)", "while"),
            span(source, "BREAK;", "BREAK"),
            span(source, "CONTINUE }", "CONTINUE"),
        ],
    );
    assert_category(
        &captures,
        source,
        "keyword.return",
        &[
            span(source, "RETURN $value", "RETURN"),
            span(source, "EXIT 1", "EXIT"),
        ],
    );
    assert_category(
        &captures,
        source,
        "keyword.exception",
        &[span(source, "THROW $error", "THROW")],
    );
}

#[test]
fn pipeline_background_and_invocation_operators_are_complete_tokens() {
    let source = concat!(
        "'café' | Write-Output && Write-Output ok || Write-Output fallback\n",
        "Write-Output background &\n",
        "& $command\n",
        ". $script\n",
    );
    let captures = captures(source);
    assert_category(
        &captures,
        source,
        "operator",
        &[
            span(source, "'café' |", "|"),
            span(source, "&& Write-Output", "&&"),
            span(source, "|| Write-Output", "||"),
            span(source, "background &", "&"),
            span(source, "& $command", "&"),
            span(source, ". $script", "."),
        ],
    );
}

#[test]
fn verbatim_arguments_highlight_as_literal_text() {
    let source = "native --% \"$name|literal && text\" ; >file #raw & && next\n";
    let captures = captures(source);
    assert_category(
        &captures,
        source,
        "keyword",
        &[span(source, "native --%", "--%")],
    );
    assert_category(
        &captures,
        source,
        "string",
        &[span(
            source,
            "\"$name|literal && text\" ; >file #raw & ",
            "\"$name|literal && text\" ; >file #raw & ",
        )],
    );
    assert_category(&captures, source, "variable", &[]);
    assert_category(&captures, source, "comment", &[]);
    assert_category(
        &captures,
        source,
        "operator",
        &[span(source, "&& next", "&&")],
    );
}

#[test]
fn combined_opening_delimiters_are_not_split() {
    let source = concat!(
        "$map = @{ Name = 'café' }\n",
        "$array = @(1, 2)\n",
        "$value = $(Get-Item .)\n",
        "${object}?[0]\n",
    );
    let captures = captures(source);
    assert_category(
        &captures,
        source,
        "punctuation.bracket",
        &[
            span(source, "@{ Name", "@{"),
            span(source, "Name = 'café' }", "}"),
            span(source, "@(1, 2)", "@("),
            span(source, "@(1, 2)", ")"),
            span(source, "$(Get-Item .)", "$("),
            span(source, "$(Get-Item .)", ")"),
            span(source, "${object}?[0]", "?["),
            span(source, "${object}?[0]", "]"),
        ],
    );
}

#[test]
fn contextual_param_and_verbatim_invocation_keep_their_roles() {
    let source = "param($x)\nparam a\n& --% \"$HOME\"\n";
    let captures = captures(source);
    assert_category(
        &captures,
        source,
        "keyword",
        &[
            span(source, "param($x)", "param"),
            span(source, "& --%", "--%"),
        ],
    );
    assert_category(
        &captures,
        source,
        "function.call",
        &[span(source, "param a", "param")],
    );
    assert_category(
        &captures,
        source,
        "variable",
        &[span(source, "param($x)", "$x")],
    );
}
