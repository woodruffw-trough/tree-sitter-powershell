#!/usr/bin/env python3
"""Reproduce combinatorial fixtures using PowerShell's non-executing parser.

By default, check the committed combinatorial fixtures. Add --variables to
check the variable matrix instead. Pass --write to update the selected file.
PWSH_BIN or --pwsh selects the official PowerShell executable.
"""

import argparse
import json
import os
import subprocess
import tempfile
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
FIXTURES = ROOT / "tests/fixtures/generated.json"


def build_cases():
    cases = []
    seen = set()
    counts = Counter()

    def add(category, source):
        if source in seen:
            return
        seen.add(source)
        counts[category] += 1
        cases.append(
            {
                "name": f"{category} {counts[category]:03d}",
                "source": source,
                "valid": True,
            }
        )

    # Identical lexical forms change their meaning between expression and argument mode.
    numbers = [
        "0",
        "1",
        "12",
        "0x1f",
        "0XFF",
        "0b101",
        "0B10",
        "1L",
        "1l",
        "1u",
        "1ul",
        "1UL",
        "1s",
        "1us",
        "1y",
        "1uy",
        "1n",
        "1d",
        "1D",
        "1kb",
        "1MB",
        "1gb",
        "1tb",
        "1pb",
        "1.0",
        "1.",
        ".5",
        "1e2",
        "1E+2",
        "1e-2",
        "1.5d",
        "1.5kb",
        "1e2kb",
        "1dkb",
        "1lkb",
        "1ulmb",
        "0xffL",
        "0b101uy",
    ]
    for number in numbers:
        for template in ["{}", "({})", "$n = {}", "{}..3", "Write-Output {}", "[double]{}"]:
            add("numeric forms", template.format(number))
    for number in [
        "1uL",
        "1Lu",
        "1dd",
        "1e",
        "1e+",
        "0x",
        "0b2",
        "1.2.3",
        "01",
        "1_000",
        "0x1_0",
        "0b1_0",
        "1k",
        "1m",
        "1.5u",
        "1.5l",
        "1.5n",
        "1..2",
        "1...2",
        "1..-2",
    ]:
        for template in ["({})", "$n = {}", "Write-Output {}"]:
            add("numeric boundaries", template.format(number))
    operands = ["$a", "1", '"x"', "(1)", "@(1,2)", "[int]1", "$(Get-Item .)"]
    operators = [
        "+",
        "-",
        "*",
        "/",
        "%",
        "..",
        "-eq",
        "-ceq",
        "-ilike",
        "-notmatch",
        "-contains",
        "-notin",
        "-replace",
        "-split",
        "-join",
        "-and",
        "-xor",
        "-band",
        "-bor",
        "-shl",
        "-shr",
        "-as",
        "-isnot",
        "-f",
        "??",
    ]
    for operator_index, operator in enumerate(operators):
        for operand_index, left in enumerate(operands):
            right = operands[(operator_index + operand_index) % len(operands)]
            for spacing in [" ", "\n"]:
                add("binary contexts", f"({left} {operator}{spacing}{right})")
        if operator.startswith("-"):
            add("operator case", f"($a {operator.upper()} $b)")
            add("operator case", f"($a {operator.title()} $b)")
    for operator in ["+", "-", "!", "-not", "-bnot", ",", "-split", "-join"]:
        for operand in operands:
            add("unary forms", f"({operator} {operand})")
    for receiver in ["$a", "${a}", "(1)", "[int]", '"text"', "@(1,2)"]:
        for member in [
            "Length",
            "length",
            '"Length"',
            "'Length'",
            "$name",
            "($name)",
            '$("Length")',
        ]:
            for spacing in ["", " ", "\n"]:
                add("member adjacency", f"{receiver}{spacing}.{member}")
        for spacing in ["", " ", "\n"]:
            for suffix in [
                "[0]",
                "[0,1]",
                "[-1]",
                "[1..2]",
                "?.Length",
                "?[0]",
                ".ToString()",
                ".ToString (1)",
                ".ToString(1,2)",
                ".ToString(1 + 2)",
                ".ToString[int](1)",
            ]:
                add("postfix adjacency", f"{receiver}{spacing}{suffix}")
    # Command argument forms, including adjacency which forms one expandable token.
    fragments = [
        "word",
        "a-b",
        "-literal",
        "./path",
        "C:\\path",
        "a:b",
        "name=value",
        "*.*",
        "1..2",
        "$x",
        "${x}",
        '"a"',
        "'a'",
        "pre$x",
        "${x}post",
        "pre${x}post",
        '"a"b',
        'a"b"',
        "a'b'",
        'a"$x"b',
        "\"a\"'b'",
        "a` b",
        '`"quote`"',
        "[int]",
        "[int]1",
        "@(1,2)",
        "$(1+2)",
        "@{a=1}",
        "{ $x }",
        "$x.member",
        "$x[0]",
    ]
    for fragment in fragments:
        for template in [
            "Write-Output {}",
            "Write-Output -Name:{}",
            "& $cmd {}",
            ". ./script.ps1 {}",
        ]:
            add("command arguments", template.format(fragment))
    for prefix in ["pre", "$x", "${x}", '"quoted"', "'quoted'"]:
        for suffix in ["post", "$y", "${y}", '"quoted"', "'quoted'"]:
            add("argument concatenation", f"Write-Output {prefix}{suffix}")
    # Test nested argument expressions and delimiters without demanding type resolution.
    for expression in [
        "$x",
        "1+2",
        "1,2",
        "(1,2)",
        "1..2",
        "[int]1",
        "@(1,2)",
        "$(1;2)",
        "@{a=1}",
        "($a ? 1 : 2)",
        "$a ?? $b",
    ]:
        for template in ["$o.F({})", "param($x={})", "$o.F(0,{})", "[A({})]param($x)"]:
            add("argument expression modes", template.format(expression))
    keyword_sources = {
        "if": "{w} ($true) {{ 1 }} ELSE {{ 2 }}",
        "switch": "{w} -REGEX ($x) {{ default {{ 1 }} }}",
        "foreach": "{w} ($x IN $xs) {{ $x }}",
        "for": "{w} ($i=0;$i -LT 3;$i++) {{ $i }}",
        "while": "{w} ($false) {{ 1 }}",
        "do": "{w} {{ 1 }} UNTIL ($true)",
        "try": "{w} {{ 1 }} CATCH {{ 2 }} FINALLY {{ 3 }}",
        "trap": "{w} {{ continue }}",
        "function": "{w} Test {{ param($x) $x }}",
        "filter": "{w} Test {{ $_ }}",
        "param": "{w}([int]$x=1)\n$x",
        "begin": "{{ {w} {{ 1 }} }}",
        "process": "{{ {w} {{ $_ }} }}",
        "end": "{{ {w} {{ 1 }} }}",
        "clean": "{{ {w} {{ 1 }} }}",
        "using": "{w} NAMESPACE System.Text",
        "data": '{w} Test {{ "value" }}',
    }
    for word, template in keyword_sources.items():
        for variant in [word, word.upper(), word.title()]:
            add("keyword case", template.format(w=variant))
    # Real syntax errors, not compiler checks such as lvalue and type resolution.
    for source in [
        "(",
        "@(",
        "$(",
        "@{",
        "[int",
        '"unclosed',
        "'unclosed",
        "${unclosed",
        "if ($true)",
        "if $true {}",
        "if () {}",
        "foreach ($x $xs) {}",
        "foreach ($x in) {}",
        "for (;;; ) {}",
        "while () {}",
        "do {}",
        "try {}",
        "try {} catch",
        "function {}",
        "function f(",
        "param($x,)",
        "$x =",
        "($x +)",
        "($x -eq)",
        "($x ??)",
        "($x ? 1)",
        "($x ? : 1)",
        "$x[",
        "$x[]",
        "$x.F(",
        "$x.F(,)",
        "$x.F(1,)",
        "@{a}",
        "@{a=}",
        "@{a=1 b=2}",
        "1 |",
        "| 1",
        "Write-Output >",
        "Write-Output 2>&",
        "Write-Output -Name:",
        "{ begin {} process",
        "switch ($x) { default }",
    ]:
        add("incomplete syntax", source)
    # Unicode is classified by the official tokenizer, including supplementary codepoints.
    for name in ["é", "über", "日本語", "Δ", "𐐀", "😀", "𝒙"]:
        for template in [
            "$" + name,
            "${" + name + "}",
            '"$' + name + '"',
            '"${' + name + '}"',
            "Write-Output $" + name,
            "Write-Output ${" + name + "}",
            "function " + name + " { 1 }",
            "$o." + name,
            "[" + name + "]",
            "Write-Output " + name,
        ]:
            add("unicode lexical forms", template)
    for quote_set in ["'‘’‚‛", '"“”„']:
        for left in quote_set:
            for right in quote_set:
                for template in ["{l}text{r}", "Write-Output pre{l}text{r}post"]:
                    add("smart quotes", template.format(l=left, r=right))
    for dash in ["-", "–", "—", "―"]:
        for template in [
            "($a {d}eq $b)",
            "($a {d}EQ $b)",
            '($a {d}notlike "x")',
            "({d}not $a)",
            "Write-Output {d}Name value",
            "Write-Output {d}Name:$true",
            "({d}1)",
            "($a {d} $b)",
        ]:
            add("unicode dash", template.format(d=dash))
    for text in [
        "1foo",
        "123abc",
        "1.2.3",
        "1e",
        "0x",
        "0xgg",
        "1Lu",
        "1_000",
        "1.5u",
        "2>&1",
        "1..2",
    ]:
        for template in ["{}", "({})", "$n = {}", "Write-Output {}", "Write-Output pre{}post"]:
            add("numeric token boundaries", template.format(text))
    return cases


def build_variable_cases():
    """Check variable tokens in expressions, arguments, and both string forms."""
    variables = [
        "$a",
        "$1",
        "$_",
        "$?",
        "$?foo",
        "$??",
        "$a?b",
        "$a?",
        "$env:?foo",
        "$env:?",
        "$env:PATH:x",
        "$foo::bar",
        "$foo:bar:baz",
        "$foo:",
        "$:",
        "$:foo",
        "$::",
        "$::foo",
        "$global:",
        "$local:",
        "$private:",
        "$script:",
        "${name with spaces}",
        "${日本語}",
        "${𐐀}",
        "${}",
        "${env:}",
        "${global:}",
        "${:}",
        "${::}",
        "${foo::}",
        "${foo:bar:baz}",
        "${foo`}:bar}",
        "${foo`nbar}",
        "${foo`rbar}",
        "${foo`\nbar}",
        "${foo`$bar}",
        "${foo``bar}",
        "${foo`}",
        "${unclosed",
        "$𐐀",
        "$😀",
        "$²",
        "$Ⅳ",
        "$☃",
        "$©",
        "$𐐀foo",
        "$a𐐀",
        "$é",
        "$０",
        "$é",
        "$`a",
        "$a`b",
        "$name`:",
        "$name:",
        "$name::",
        "$name::foo",
        "$name:?foo",
        "$foo?bar",
        "$$",
        "$$foo",
        "$^",
        "$^foo",
        "$(",
        "`$name:",
        "$!",
        "${a{b}",
        "${a\nb}",
        "${foo`u{3a}}",
        "${foo`u{7d}}",
        "${`u{61}}",
    ]
    cases = []
    for index, variable in enumerate(variables, 1):
        contexts = [
            variable,
            "Write-Output " + variable,
            '"prefix ' + variable + ' suffix"',
            '@"\nprefix ' + variable + ' suffix\n"@',
        ]
        for context, source in enumerate(contexts, 1):
            cases.append(
                {
                    "name": f"variable lexical {index:03d}-{context}",
                    "source": source,
                    "valid": True,
                }
            )
    return cases


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--write", action="store_true", help="update the committed fixtures")
    parser.add_argument("--pwsh", default=os.environ.get("PWSH_BIN", "pwsh"))
    parser.add_argument("--variables", action="store_true", help="select the variable matrix")
    args = parser.parse_args()
    fixture_path = ROOT / "tests/fixtures/variables.json" if args.variables else FIXTURES
    cases = build_variable_cases() if args.variables else build_cases()
    with tempfile.TemporaryDirectory(prefix="powershell-fixtures-") as directory:
        candidates = Path(directory) / "candidates.json"
        candidates.write_text(json.dumps(cases, ensure_ascii=False), encoding="utf-8")
        try:
            output = subprocess.run(
                [
                    args.pwsh,
                    "-NoLogo",
                    "-NoProfile",
                    "-NonInteractive",
                    "-File",
                    str(ROOT / "scripts/oracle.ps1"),
                    "-Fixtures",
                    str(candidates),
                ],
                check=True,
                capture_output=True,
                text=True,
                encoding="utf-8",
                timeout=120,
            )
        except subprocess.CalledProcessError as error:
            parser.exit(1, f"PowerShell oracle failed:\n{error.stderr}\n")
        except (OSError, subprocess.TimeoutExpired) as error:
            parser.exit(1, f"Unable to run PowerShell oracle: {error}\n")
    oracle = json.loads(output.stdout)
    # ParseInput also performs static semantic checks. An attribute's requirement
    # for a constant expression is not a grammar rule; exclude those cases rather
    # than teaching a context-free parser to reject syntactically valid sources.
    semantic_errors = {"ParameterAttributeArgumentNeedsToBeConstantOrScriptBlock"}
    fixtures = [
        {key: case[key] for key in ("name", "source", "valid")}
        for case in oracle["results"]
        if not (case["errors"] and all(error["id"] in semantic_errors for error in case["errors"]))
    ]
    serialized = json.dumps(fixtures, ensure_ascii=False, indent=2) + "\n"
    if args.write:
        fixture_path.write_text(serialized, encoding="utf-8")
    elif fixture_path.read_text(encoding="utf-8") != serialized:
        select = " --variables" if args.variables else ""
        parser.exit(
            1, f"Generated fixtures differ; run scripts/generate-fixtures.py{select} --write\n"
        )
    print(f"{len(fixtures)} fixtures agree with PowerShell {oracle['powershellVersion']}")


if __name__ == "__main__":
    main()
