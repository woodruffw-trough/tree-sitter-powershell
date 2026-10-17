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
    for (source, valid) in [
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
        "$obj.[int].Name\n",
        "$obj.Method [int](1)\n",
        "class C { F() { param($x) $x } }\n",
        "class C { F() { begin { 1 } end { 2 } } }\n",
        "function f(@x) { $x }\n",
        "enum E { A; B }\n",
        "!name tail | next\n",
        "!0x1 tail | next\n",
        "!1.$v tail | next\n",
        "!1+$v | next\n",
        "!1..2\n",
        "*>&1a tail | next\n",
        "2>&1$(inner) tail | next\n",
        "cmd !name !1x !1+$v\n",
        "fromsuffix | define | var\n",
        ";param($x)\n",
        "workflow w { InlineScript\n{ inner } tail | next }",
        "workflow w { function f { InlineScript }; InlineScript\n{} tail }",
        "function f { workflow w { InlineScript\n{} tail } }",
        "workflow w { class C { [void] M() { InlineScript\n{} tail } } }",
        "workflow w { $obj.M(@{ Key = InlineScript\n{ inner } tail }) }",
        "workflow w { \"$(InlineScript\n{ inner } tail)\" }",
        "workflow w($x={ InlineScript }) { InlineScript\n{} tail }",
        "workflow w { function f($x={ InlineScript\n{} tail }) {} }",
        "workflow w { InlineScript\n{ InlineScript\n{} innerTail } outerTail }",
        "configuration Example { Node localhost { File Asset { Ensure = 'Present' } } }\nFile outside {}\n",
        "configuration C { Script S { GetScript = { File ordinary {} } } }\nNode outside {}\n",
        "[Some()]\nconfiguration\nExample\n{}\n",
        "configuration one,two {}\n",
        "configuration C { Settings { RefreshMode='Push' } }\n",
        "parallel { sequence { 1 } }\n",
        "return\n| echo x | next\n",
        "throw\n# comment\n| echo x\n",
        "exit\r\n| 1 | next\n",
        "\"$(return\n| echo x | next)\"\n",
        "function f { ;param($x) }\n",
        "using namespace System;param($x) $x\n",
        "switch (1) { default {} 'default' {} }\n",
        "foreach (@x in 1) { $x }\n",
        "$obj.[int].Name++++\n",
        "$obj.-$x++++\n",
        "[int][A()]$x++++\n",
        "[A()]$x.Name\n",
        "[A()][int]\n$x\n",
        "cmd 256uy tail | next\n",
        "$obj.-128y\n",
        "$obj.+1++\n",
        "@{+1++=1}\n",
        "$obj.Method(-32768s)\n",
        "\"$(-128y)\"\n",
        "0e2147483647n\n",
        "$obj.Method` [int](1)\n",
        "$obj.Method`\r\n[int](1)\n",
        "[int]` (1)\n",
        "[int] <#comment#>.Name\n",
        "$obj<#comment#>?[0]\n",
        "cmd $obj.(-$name)<#comment#>.Name tail | next\n",
        "cmd $obj.Name<#comment#>?word tail | next\n",
        "cmd $obj.Name<#comment#>..2 tail | next\n",
        "$obj.[int]{1}\n",
        "Write-Output $obj.- $name(1)\n",
        "data\nMessages\n-supportedcommand\nfoo$bar,$name.Length,(Get-Item .)\n{}\n",
        "data -supportedcommand @name.Length,${name}-tail,-not,[string] {}\n",
        "function `u{41} {}\n",
        "function f${a\"b}($p) { $p }; function f$$() {}\n",
        "function :f$(${a)b}) {}; Write-Output after\n",
        "filter :f`u{zz} {}; Write-Output after\n",
        "function :名\"word($p) {}; Write-Output after\n",
        "filter ${name}-tail {}; function f\"${a`u{7d}b}\" {}\n",
        "function `\r\n foo {}\n",
        "-1e-2.ToString()\n",
        "[double]+1e-2.Length\n",
        "[double]-1e-2.Length\n",
        "+0bu\nWrite-Output -0bu\n",
        "$row ++ ; $startRow ++\n",
        "[T]::GetAddress(1,($i ++))\n",
        "(1)\n",
        "[T]::M(1 + 2?3:4,[int]+1)\n",
        "param($x = 1 ? 2 : 3)\n",
        "$x[1 ? 2 : 3]\n",
        "$x.5\n",
        "cmd -1 +1 >-1\n",
        "1#comment\n2\n",
        "\"$([T]::M(1 ? 2 : 3))\"\n",
        "using namespace System.\"Text\".More\nparam($x)\n$x\n",
        "using namespace Collections = System.Collections\nparam($x)\n",
        "using module Utility = @{ModuleName='Microsoft.PowerShell.Utility'}\n",
        "using namespace @\"\nSystem.Text\n\"@\n",
        "[int][CmdletBinding()]param($x)\n$x\n",
        "[int]\n[string]\nparam($x)\n",
        "using namespace Sys'tem'.Text;using namespace Other\n1\n",
        "[int, x, y = z]\n[List[[string, a[b, Culture = neutral]]]\n",
        "param([int, x, y = z]$x)\n$x\n",
        "function \0f {}\nfilter \0\"name\" {}\n",
        "$x.M( ); [int]::new(\t); $x.M[int](\u{a0})\n",
        "[CmdletBinding( )]param([Parameter( )]$x)\n",
        "class C { C():base( ) {} }\n",
        "[A()]\n[B()]\n$x\n",
        "[A()][int][B()]$x\n",
        "[A()]\n[B(@{a=']'})]\n$x\n",
        "[A()]\n[B(@'\n' ]foo\n'@)]\n$x\n",
        "[A()]\n[B(@\"\n$(\"]\") ] body\n\"@)]\n$x\n",
        "cmd $x.\nfoo tail | next\n",
        "cmd $x. <#c#>foo tail | next\n",
        "cmd ${x}:: foo tail | next\n",
        "cmd $x::` (cmd) tail | next\n",
        "cmd ($x)?.\n<# comment #>\nName tail | next\n",
        "cmd ($x)?. <# outer <# inner #> #>\nName tail | next\n",
        "cmd $x.foo?.\n# comment\n$y tail | next\n",
        "cmd ($x)?.`\n<# comment #>\nName tail | next\n",
        "cmd $$[1]; cmd ${$}[1]; cmd $x..M\n",
        "data X -s $x::M() {}; data Y -s $$.M {}\n",
        "data X -s $x.<##>\nM {}\n",
        "data X -s $x.M(), $y.Name {}\n",
        "break $x.M; continue $$.M\n",
        "break $x..M; continue $x::M()\n",
        "break ${a`u{7d}b}; continue ${x}=value\n",
        "break ($x)<# retained #>.M; continue foo.M\n",
        "break ($x)++; continue ($x)--\n",
        "break ($x)++$x; continue ($x)--$x\n",
        "1.`\n-1 tail | next\n",
        "1='value'; 0x1+=Get-'Item'\n",
        "cmd 1=1 1]foo | & 1=1 tail | next\n",
        "cmd 128y.M 256uy+1 | & 128y=1 tail | next\n",
        "return 127y#'text\nWrite-Output after | next\n",
        "throw -128y#\"text\nWrite-Output after | next\n",
        "$answer = -1.`u{002d}+1 tail | next\n",
        "cmd @$foo::bar tail | next\n",
        ":name${x}\n",
        ":$x[0]\n",
        ":outer while(1) {break}\n",
        "echo before `\n| next after\n",
        "echo before\n# retained\n| next after\n",
        "echo before\n<# retained #>\n|| next after\n",
        "switch -r -w -e -c -p (1) { default {} }\n",
        "switch -par -file \"$path\" { default {} }\n",
        "foreach -p -t 2 ($x in 1) {}\n",
        "workflow w { function f {} filter g {} }\n",
        "[int[,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,]]\n",
    ]
    .into_iter()
    .map(|source| (source, true))
    .chain(
        [
            "[int[,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,,]]; Write-Output after | next\n",
            "'Hello'\n    `\n    | ForEach-Object {\"$_ world\"}\n",
            "return\n`\n| next after\n",
            ":$x[1,2]\nWrite-Output after | next\n",
            ":$x[0].Foo()\nWrite-Output after | next\n",
            "[A()]\n[B()]\nWrite-Output after | next\n",
            "function :f$(1) {}; Write-Output after\n",
            "break ${a`u{7d}b}.\nWrite-Output after | next\n",
            "cmd $x.`\n$y++\nTest-RecoveryNewline item | Test-RecoveryNext\n",
            "cmd $x::`\n<# retained #>\n$y++\nTest-RecoveryNewline item | Test-RecoveryNext\n",
            "1=;\nWrite-Output after | next\n",
            "1]foo\nWrite-Output after | next\n",
            "cmd | 1=1 tail | next\n",
            "128y.M\nWrite-Output after | next\n",
            "-129y+1\nWrite-Output after | next\n",
            "256uy>out\nWrite-Output after | next\n",
            "128y#\"text\"\nWrite-Output after | next\n",
            "data X -s $x:: M {}\nWrite-Output after | next\n",
            "data X -s ($x). <##>\nM {}\nWrite-Output after | next\n",
            "[A()]\n[B(@'\n' ]foo\n'@)]\nWrite-Output after | next\n",
            "[A()]\n[B(@\"\n$(\"]\") ] body\n\"@)]\nWrite-Output after | next\n",
            "[A()]\n[B`\nC(1)]\nparam($x)\n",
            "$x++++\nWrite-Output after | next\n",
            "$x +\nWrite-Output after | next\n",
            "$x[\nWrite-Output after | next\n",
            "[A()] +\nWrite-Output after | next\n",
            "workflow w { InlineScript -Name:x }; next",
        ]
        .map(|source| (source, false)),
    ) {
        let mut parser = parser();
        let original = parser.parse(source, None).unwrap();
        assert_eq!(!original.root_node().has_error(), valid, "{source}");
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
                (start, ";"),
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

#[test]
fn deep_scopes_restore_modes_after_incremental_edits() {
    for (open, close, depth, innermost_kind) in [
        ("configuration C { ", " }", 1024, "resource_statement"),
        (
            "configuration C { Script S { GetScript = { ",
            " } } }",
            1024,
            "command",
        ),
        (
            "workflow w { configuration C { Script S { GetScript = { function f { ",
            " } } } } }",
            64,
            "command",
        ),
    ] {
        let body = "File deepest {}";
        let source = format!(
            "{}{}{}\nFile outside {{}}\nNode outside {{}}\nInlineScript outside",
            open.repeat(depth),
            body,
            close.repeat(depth)
        );
        let mut parser = parser();
        let mut original = parser.parse(&source, None).unwrap();
        assert!(!original.root_node().has_error(), "depth {depth}");
        let start = source.find(body).unwrap();
        let inner = original
            .root_node()
            .named_descendant_for_byte_range(start, start + body.len())
            .unwrap();
        assert_eq!(inner.kind(), innermost_kind);
        let replacement = "Script";
        let end = start + "File".len();
        let edited = format!("{}{}{}", &source[..start], replacement, &source[end..]);
        original.edit(&InputEdit {
            start_byte: start,
            old_end_byte: end,
            new_end_byte: start + replacement.len(),
            start_position: point(&source, start),
            old_end_position: point(&source, end),
            new_end_position: point(&edited, start + replacement.len()),
        });
        let incremental = parser.parse(&edited, Some(&original)).unwrap();
        let fresh = parser.parse(&edited, None).unwrap();
        assert!(!fresh.root_node().has_error(), "depth {depth}");
        let (actual, expected) = (incremental.root_node(), fresh.root_node());
        assert_eq!(actual.to_sexp(), expected.to_sexp());
        assert_eq!(actual.range(), expected.range());
        let inner_end = start + body.len() + replacement.len() - "File".len();
        assert_same_tree(
            actual
                .named_descendant_for_byte_range(start, inner_end)
                .unwrap(),
            expected
                .named_descendant_for_byte_range(start, inner_end)
                .unwrap(),
            &edited,
        );
        for (offset, name) in ["File", "Node", "InlineScript"].into_iter().enumerate() {
            let index = (actual.named_child_count() - 3 + offset)
                .try_into()
                .unwrap();
            let tail = actual.named_child(index).unwrap();
            assert_eq!(tail.kind(), "pipeline");
            let command = tail.child_by_field_name("elements").unwrap();
            assert_eq!(command.kind(), "command");
            let command_name = command.child_by_field_name("name").unwrap();
            assert_eq!(&edited[command_name.byte_range()], name);
            assert_same_tree(tail, expected.named_child(index).unwrap(), &edited);
        }
    }
}
