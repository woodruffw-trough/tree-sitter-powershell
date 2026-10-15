use tree_sitter::{InputEdit, Node, Parser, Point};

fn parser() -> Parser {
    let mut parser = Parser::new();
    parser
        .set_language(&woodruffw_trough_tree_sitter_powershell::LANGUAGE.into())
        .unwrap();
    parser
}

fn point(source: &str, offset: usize) -> Point {
    let prefix = &source.as_bytes()[..offset];
    let row = prefix.iter().filter(|&&byte| byte == b'\n').count();
    let column = prefix
        .iter()
        .rposition(|&byte| byte == b'\n')
        .map_or(offset, |position| offset - position - 1);
    Point::new(row, column)
}

fn fingerprint(node: Node<'_>, output: &mut String) {
    use std::fmt::Write;
    write!(
        output,
        "({}:{}..{}:{:?}..{:?}:{}:{}",
        node.kind(),
        node.start_byte(),
        node.end_byte(),
        node.start_position(),
        node.end_position(),
        node.is_missing(),
        node.is_error()
    )
    .unwrap();
    let mut cursor = node.walk();
    for (index, child) in node.children(&mut cursor).enumerate() {
        write!(output, "{:?}", node.field_name_for_child(index as u32)).unwrap();
        fingerprint(child, output);
    }
    output.push(')');
}

fn assert_same_tree(incremental: Node<'_>, fresh: Node<'_>, source: &str) {
    let mut actual = String::new();
    let mut expected = String::new();
    fingerprint(incremental, &mut actual);
    fingerprint(fresh, &mut expected);
    assert_eq!(actual, expected, "Incremental parse disagrees for {source}");
}

#[test]
fn edited_trees_match_fresh_parses() {
    let examples = [
        ("$x = 1 + 2 * 3\n", "2", "$value"),
        ("Write-Output \"hello $name\"\n", "hello", "世界"),
        ("$x = @'\nfirst\n'@\n$x\n", "first", "first\nsecond"),
        ("$x = @\"\n$name\n\"@\n$x\n", "$name", "$(1 + 2)"),
        ("if ($true) { Get-Item . }\n", "$true", "$false"),
        (
            "<# comment #>\n$a = @(1, 2)\n",
            "comment",
            "changed\ncomment",
        ),
    ];
    for (source, old, new) in examples {
        let mut parser = parser();
        let mut tree = parser.parse(source, None).unwrap();
        assert!(!tree.root_node().has_error(), "{source}");
        let start = source.find(old).unwrap();
        let end = start + old.len();
        let edited = format!("{}{}{}", &source[..start], new, &source[end..]);
        tree.edit(&InputEdit {
            start_byte: start,
            old_end_byte: end,
            new_end_byte: start + new.len(),
            start_position: point(source, start),
            old_end_position: point(source, end),
            new_end_position: point(&edited, start + new.len()),
        });
        let incremental = parser.parse(&edited, Some(&tree)).unwrap();
        let fresh = parser.parse(&edited, None).unwrap();
        assert!(!fresh.root_node().has_error(), "{edited}");
        assert_same_tree(incremental.root_node(), fresh.root_node(), &edited);
    }
}

#[test]
fn insertions_and_deletions_preserve_tree_positions_and_recovery() {
    for source in [
        "$x = @'\nhello\n'@\nWrite-Output $x\n",
        "$x = @\"\n世界 $name $(1 + 2)\n\"@\n",
        "Write-Output \"$($obj.Name):`n$name\"\n",
        "<# multiline\ncomment #>\n$x = 1..3 | ForEach-Object { $_ + 1 }\n",
        "$items.Where{$_ -gt 0}\n",
        "& .\\$installScript -Force\n",
        "[T]::M([string], $value)\n",
        "[ValidateNotNull()]$x = 1\n",
        "class C {$x=1;;C(){}M(){}$y=2;}\n",
        "Write-X $obj.- $name\n",
        "Write-X $obj.$name=more\n",
        "$obj.F($obj.(,1))\n",
        "$obj.F(1)++\n",
        "$obj.-$name.Length\n",
        "data\nMessages\n-supportedcommand\nfoo$bar,$name.Length,(Get-Item .)\n{}\n",
        "data -supportedcommand @name.Length,${name}-tail,-not,[string] {}\n",
        "function `u{41} {}\n",
        "function `\r\n foo {}\n",
        "-1e-2.ToString()\n",
        "[double]+1e-2.Length\n",
        "[double]-1e-2.Length\n",
        "+0bu\nWrite-Output -0bu\n",
    ] {
        let mut parser = parser();
        let original = parser.parse(source, None).unwrap();
        assert!(!original.root_node().has_error(), "{source}");
        for start in source
            .char_indices()
            .map(|(index, _)| index)
            .chain(std::iter::once(source.len()))
        {
            let char_end = source[start..]
                .chars()
                .next()
                .map_or(start, |ch| start + ch.len_utf8());
            for (end, inserted) in [
                (start, "\n"),
                (start, "$"),
                (start, "`"),
                (start, "<#comment#>"),
                (char_end, ""),
            ] {
                let edited = format!("{}{}{}", &source[..start], inserted, &source[end..]);
                let mut old_tree = original.clone();
                old_tree.edit(&InputEdit {
                    start_byte: start,
                    old_end_byte: end,
                    new_end_byte: start + inserted.len(),
                    start_position: point(source, start),
                    old_end_position: point(source, end),
                    new_end_position: point(&edited, start + inserted.len()),
                });
                let incremental = parser.parse(&edited, Some(&old_tree)).unwrap();
                let fresh = parser.parse(&edited, None).unwrap();
                assert_same_tree(incremental.root_node(), fresh.root_node(), &edited);
            }
        }
    }
}

#[test]
fn partial_input_recovers_after_completion() {
    let mut parser = parser();
    for source in [
        "if ($true) {",
        "\"unterminated",
        "$x = @(1,",
        "$x = @'\ntext",
    ] {
        let tree = parser.parse(source, None).unwrap();
        assert!(tree.root_node().has_error(), "{source}");
        assert_eq!(tree.root_node().end_byte(), source.len());
    }
    let tree = parser.parse("$x = 1\nGet-Item .", None).unwrap();
    assert!(!tree.root_node().has_error());
}

#[test]
fn highlighting_query_compiles() {
    tree_sitter::Query::new(
        &woodruffw_trough_tree_sitter_powershell::LANGUAGE.into(),
        woodruffw_trough_tree_sitter_powershell::HIGHLIGHTS_QUERY,
    )
    .unwrap();
}

#[test]
fn splat_edits_preserve_contextual_classification_and_recovery() {
    for (source, old, new, valid) in [
        ("cmd @name\n", "@name", "@name😀", true),
        ("cmd @name\n", "@name", "@?name", true),
        ("cmd @env:NAME\n", "NAME", "", false),
        ("cmd @name\n", "name", "na\u{2000}me", true),
    ] {
        let mut parser = parser();
        let mut original = parser.parse(source, None).unwrap();
        assert!(!original.root_node().has_error(), "{source}");
        let start = source.find(old).unwrap();
        let end = start + old.len();
        let edited = format!("{}{}{}", &source[..start], new, &source[end..]);
        original.edit(&InputEdit {
            start_byte: start,
            old_end_byte: end,
            new_end_byte: start + new.len(),
            start_position: point(source, start),
            old_end_position: point(source, end),
            new_end_position: point(&edited, start + new.len()),
        });
        let incremental = parser.parse(&edited, Some(&original)).unwrap();
        let fresh = parser.parse(&edited, None).unwrap();
        assert_eq!(!fresh.root_node().has_error(), valid, "{edited}");
        assert_same_tree(incremental.root_node(), fresh.root_node(), &edited);
    }
}
