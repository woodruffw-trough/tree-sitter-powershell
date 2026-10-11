// Syntax follows Microsoft's language specification and the current PowerShell
// parser. See docs/grammar-sources.md for references and compatibility notes.
const PREC = {
  ternary: 1, logical: 2, bitwise: 3, comparison: 4, coalesce: 5,
  additive: 6, multiplicative: 7, format: 8, range: 9, array: 10, unary: 11,
  member: 12,
};

const commaSep1 = rule => seq(rule, repeat(seq(repeat(/\r\n|\r|\n/), ',', repeat(/\r\n|\r|\n/), rule)));
const ci = word => new RegExp(word.split('').map(c => /[a-z]/i.test(c) ? `[${c.toLowerCase()}${c.toUpperCase()}]` : c).join(''));
const keyword = word => alias(token(prec(2, ci(word))), word);
const op = word => alias(token(prec(2, new RegExp(ci(word).source.replace(/-/g, '[-–—―]')))), word);
const flag = word => choice(...Array.from({length: word.length - 1}, (_, i) => alias(token(prec(2, ci(word.slice(0, i + 2)))), word)));

const BINARY_OPERATORS = [

        [PREC.logical, ['-and', '-or', '-xor']],
        [PREC.bitwise, ['-band', '-bor', '-bxor']],
        [PREC.comparison, ['-eq', '-ne', '-gt', '-ge', '-lt', '-le', '-ieq', '-ine', '-igt', '-ige', '-ilt', '-ile', '-ceq', '-cne', '-cgt', '-cge', '-clt', '-cle', '-like', '-notlike', '-ilike', '-inotlike', '-clike', '-cnotlike', '-match', '-notmatch', '-imatch', '-inotmatch', '-cmatch', '-cnotmatch', '-contains', '-notcontains', '-icontains', '-inotcontains', '-ccontains', '-cnotcontains', '-in', '-notin', '-iin', '-inotin', '-cin', '-cnotin', '-replace', '-ireplace', '-creplace', '-split', '-isplit', '-csplit', '-join', '-is', '-isnot', '-as', '-shl', '-shr']],
        [PREC.coalesce, ['??']],
        [PREC.additive, ['+', '-']],
        [PREC.multiplicative, ['*', '/', '%']],
        [PREC.format, ['-f']],
        [PREC.range, ['..']],

];

const binaryExpression = (expression, newlines) => choice(
  ...BINARY_OPERATORS.map(([precedence, operators]) => {
    const alternatives = operators.map(operator => {
      if (operator.startsWith('-')) return op(operator);
      if (operator === '..') return token(prec(5, operator));
      return operator;
    });
    const operator = alternatives.length === 1 ? alternatives[0] : choice(...alternatives);
    return prec.left(precedence, seq(
      field('left', expression),
      field('operator', operator),
      newlines,
      field('right', expression),
    ));
  }),
);

module.exports = grammar({
  name: 'powershell',

  extras: $ => [/[ \t\f\v\u0085\u00A0\u1680\u2000-\u200A\u2028\u2029\u202F\u205F\u3000]+/, $.comment, $.line_continuation],
  externals: $ => [$.single_quoted_here_string, $._expandable_here_string_start, $._command_name, $.bare_word, $.integer_literal, $.real_literal, $._here_string_content, $._here_string_end, $._empty_expandable_here_string, $._string_escape, $.variable, $._adjacent_variable, $._literal_dollar, $._argument_end, $._flow_label, $._switch_default, $._label_start, $._expression_argument_end, $._member_argument_end, $.command_parameter_name, $.function_name, $.comment, $._method_invocation_context, $.line_continuation, $._postfix_context, $._argument_separator],
  word: $ => $.identifier,
  supertypes: $ => [$._statement, $._expression, $._primary_expression],

  conflicts: $ => [
    [$._value_primary_expression, $.invocation_expression],
    [$._pipeline],
    [$.pipeline, $._background_pipeline],
    [$._simple_statement, $._background_pipeline],
    [$.if_statement],
    [$.try_statement],
  ],

  rules: {
    source_file: $ => seq(repeat($._terminator), repeat(seq($.using_statement, repeat($._terminator))), optional(seq($.param_block, repeat($._terminator))), optional($._script_block_body)),
    _script_block_body: $ => choice($._statement_list, repeat1(seq($.named_block, repeat($._terminator)))),
    _statement_list: $ => choice($._simple_statement, seq(repeat1($._terminated_statement), optional($._simple_statement))),
    _terminated_statement: $ => choice(seq($._simple_statement, repeat1($._terminator)), seq($._block_statement, repeat($._terminator))),
    _terminator: $ => choice(';', /\r\n|\r|\n/),
    _newlines: $ => repeat1(/\r\n|\r|\n/),


    _statement: $ => choice($._block_statement, $._simple_statement),
    _block_statement: $ => choice(
      $.if_statement, $.switch_statement, $.foreach_statement,
      $.for_statement, $.while_statement,
      $.try_statement, $.trap_statement, $.function_definition,
      $.class_definition, $.enum_definition,
      $.data_statement, $.configuration_definition, alias($._break_continue_statement, $.flow_control_statement), alias($._background_pipeline, $.pipeline),
    ),
    _simple_statement: $ => choice($.do_statement, $.flow_control_statement, alias($._pipeline, $.pipeline)),
    statement_block: $ => seq('{', repeat($._terminator), optional($._statement_list), '}'),

    if_statement: $ => seq(keyword('if'), repeat(/\r\n|\r|\n/), field('condition', $.condition), repeat(/\r\n|\r|\n/), field('body', $.statement_block), repeat(seq(repeat(/\r\n|\r|\n/), $.elseif_clause)), optional(seq(repeat(/\r\n|\r|\n/), $.else_clause))),
    elseif_clause: $ => seq(keyword('elseif'), repeat(/\r\n|\r|\n/), field('condition', $.condition), repeat(/\r\n|\r|\n/), field('body', $.statement_block)),
    else_clause: $ => seq(keyword('else'), repeat(/\r\n|\r|\n/), field('body', $.statement_block)),
    condition: $ => seq('(', repeat(/\r\n|\r|\n/), $.pipeline, repeat(/\r\n|\r|\n/), ')'),

    switch_statement: $ => seq(optional($.label), keyword('switch'), repeat(/\r\n|\r|\n/), choice(seq(repeat($.switch_parameter), $.condition), seq(repeat($.switch_parameter), $._switch_file_parameter, repeat(choice($.switch_parameter, $._switch_file_parameter)))), repeat(/\r\n|\r|\n/), '{', repeat($._terminator), repeat1(seq($.switch_clause, repeat($._terminator))), '}'),
    _switch_file_parameter: $ => seq(flag('-file'), repeat(/\r\n|\r|\n/), field('file', $._command_argument_value)),
    switch_parameter: _ => choice(flag('-regex'), flag('-wildcard'), flag('-exact'), flag('-casesensitive')),
    switch_clause: $ => seq(field('condition', choice(alias($._switch_default, 'default'), $._command_argument_value)), repeat(/\r\n|\r|\n/), field('body', $.statement_block)),
    label: $ => seq($._label_start, ':', $.identifier, repeat(/\r\n|\r|\n/)),
    foreach_statement: $ => seq(optional($.label), keyword('foreach'), repeat(/\r\n|\r|\n/), repeat($.foreach_parameter), '(', repeat(/\r\n|\r|\n/), field('variable', $.variable), repeat(/\r\n|\r|\n/), keyword('in'), repeat(/\r\n|\r|\n/), field('value', $.pipeline), repeat(/\r\n|\r|\n/), ')', repeat(/\r\n|\r|\n/), field('body', $.statement_block)),
    foreach_parameter: _ => choice(op('-parallel'), op('-throttlelimit')),
    for_statement: $ => seq(optional($.label), keyword('for'), repeat(/\r\n|\r|\n/), '(', optional($.pipeline), optional(seq(';', optional($.pipeline), optional(seq(';', optional($.pipeline))))), ')', repeat(/\r\n|\r|\n/), field('body', $.statement_block)),
    while_statement: $ => seq(optional($.label), keyword('while'), repeat(/\r\n|\r|\n/), field('condition', $.condition), repeat(/\r\n|\r|\n/), field('body', $.statement_block)),
    do_statement: $ => seq(optional($.label), keyword('do'), repeat(/\r\n|\r|\n/), field('body', $.statement_block), repeat(/\r\n|\r|\n/), choice(keyword('while'), keyword('until')), repeat(/\r\n|\r|\n/), field('condition', $.condition)),
    try_statement: $ => seq(keyword('try'), repeat(/\r\n|\r|\n/), field('body', $.statement_block), choice(seq(repeat1(seq(repeat(/\r\n|\r|\n/), $.catch_clause)), optional(seq(repeat(/\r\n|\r|\n/), $.finally_clause))), seq(repeat(/\r\n|\r|\n/), $.finally_clause))),
    catch_clause: $ => seq(keyword('catch'), optional(commaSep1($.type_literal)), repeat(/\r\n|\r|\n/), field('body', $.statement_block)),
    finally_clause: $ => seq(keyword('finally'), repeat(/\r\n|\r|\n/), field('body', $.statement_block)),
    trap_statement: $ => seq(keyword('trap'), optional($.type_literal), repeat(/\r\n|\r|\n/), field('body', $.statement_block)),
    _break_continue_statement: $ => prec.right(seq(field('keyword', choice(keyword('break'), keyword('continue'))), optional(field('value', choice($._unary_expression, alias($._flow_label, $.identifier)))))),
    flow_control_statement: $ => prec.right(seq(field('keyword', choice(keyword('return'), keyword('throw'), keyword('exit'))), optional(field('value', $.pipeline)))),

    function_definition: $ => seq(field('keyword', choice(keyword('function'), keyword('filter'), keyword('workflow'))), repeat(/\r\n|\r|\n/), field('name', $.function_name), repeat(/\r\n|\r|\n/), optional(seq($.parameter_list, repeat(/\r\n|\r|\n/))), field('body', $.script_block)),
    parameter_list: $ => seq('(', repeat(/\r\n|\r|\n/), optional(seq(commaSep1($.parameter), repeat(/\r\n|\r|\n/))), ')'),
    param_block: $ => seq(repeat(seq($.attribute, repeat(/\r\n|\r|\n/))), keyword('param'), repeat(/\r\n|\r|\n/), $.parameter_list),
    parameter: $ => seq(repeat(seq(choice($.attribute, $.type_literal), repeat(/\r\n|\r|\n/))), field('name', $.variable), optional(seq('=', repeat(/\r\n|\r|\n/), field('default', $._argument_expression)))),
    attribute: $ => seq('[', field('name', $.type_name), '(', repeat(/\r\n|\r|\n/), optional(seq(commaSep1($.attribute_argument), repeat(/\r\n|\r|\n/))), ')', ']'),
    attribute_argument: $ => choice(seq(field('name', $.identifier), optional(seq('=', repeat(/\r\n|\r|\n/), field('value', $._argument_expression)))), $._argument_expression),
    script_block: $ => seq('{', repeat($._terminator), optional(seq($.param_block, repeat($._terminator))), optional($._script_block_body), '}'),
    named_block: $ => seq(field('name', choice(keyword('dynamicparam'), keyword('begin'), keyword('process'), keyword('end'), keyword('clean'))), repeat(/\r\n|\r|\n/), field('body', $.statement_block)),

    class_definition: $ => seq(repeat(seq($.attribute, repeat(/\r\n|\r|\n/))), keyword('class'), repeat(/\r\n|\r|\n/), field('name', $.identifier), optional(seq(':', repeat(/\r\n|\r|\n/), commaSep1($.type_name))), repeat(/\r\n|\r|\n/), '{', repeat($._terminator), repeat(choice(seq($.class_property, repeat1($._terminator)), seq($.class_method, repeat($._terminator)))), optional($.class_property), '}'),
    class_property: $ => seq(repeat(seq(choice($.attribute, $.type_literal, keyword('hidden'), keyword('static')), repeat(/\r\n|\r|\n/))), field('name', $.variable), optional(seq('=', field('value', $._expression)))),
    class_method: $ => seq(repeat(seq(choice($.attribute, $.type_literal, keyword('hidden'), keyword('static')), repeat(/\r\n|\r|\n/))), field('name', $.identifier), $.parameter_list, optional($.base_constructor_call), repeat(/\r\n|\r|\n/), field('body', $.statement_block)),
    base_constructor_call: $ => seq(':', keyword('base'), '(', optional(commaSep1($._argument_expression)), ')'),
    enum_definition: $ => seq(repeat(seq($.attribute, repeat(/\r\n|\r|\n/))), keyword('enum'), repeat(/\r\n|\r|\n/), field('name', $.identifier), optional(seq(':', repeat(/\r\n|\r|\n/), $.type_name)), repeat(/\r\n|\r|\n/), '{', repeat($._terminator), repeat(seq($.enum_member, repeat1($._terminator))), optional($.enum_member), '}'),
    enum_member: $ => seq(field('name', $.identifier), optional(seq('=', field('value', $._expression)))),
    using_statement: $ => seq(keyword('using'), field('kind', choice(keyword('namespace'), keyword('module'), keyword('assembly'))), field('value', choice($.bare_word, $.single_quoted_string, $.expandable_string, $.hashtable_expression))),
    data_statement: $ => seq(keyword('data'), optional($.identifier), optional(seq(flag('-supportedcommand'), commaSep1(choice($.bare_word, $.single_quoted_string, $.expandable_string)))), repeat(/\r\n|\r|\n/), $.statement_block),
    configuration_definition: $ => seq(keyword('configuration'), field('name', choice($.identifier, $.variable)), repeat(/\r\n|\r|\n/), $.script_block),

    assignment_statement: $ => prec.right(seq(field('left', $._expression), field('operator', choice(...['=', '+=', '-=', '*=', '/=', '%=', '??='].map(o => alias(token(prec(3, o)), o)))), repeat(/\r\n|\r|\n/), field('right', $._statement))),
    pipeline: $ => choice($._pipeline, $._background_pipeline),
    _background_pipeline: $ => seq($._pipeline, '&'),
    _pipeline: $ => seq(choice($._pipeline_element, $.assignment_statement), repeat(seq(choice('|', $._leading_pipe), repeat(/\r\n|\r|\n/), $._pipeline_element)), repeat(seq(field('operator', choice('&&', '||')), repeat(/\r\n|\r|\n/), choice($._pipeline_element, $.assignment_statement), repeat(seq('|', $._pipeline_element))))),
    _leading_pipe: $ => seq(/\r\n|\r|\n/, repeat(seq($.comment, /\r\n|\r|\n/)), '|'),
    _pipeline_element: $ => choice($.command, seq($._expression, repeat($.redirection))),
    command: $ => seq(field('name', choice($.command_name, $.invocation_operator_command)), repeat(choice($.command_argument, $.command_parameter, $.redirection)), optional(seq(alias(token(prec(3, '--')), $.command_parameter), repeat(choice($.command_argument, $.redirection)))), optional($.stop_parsing)),
    command_name: $ => choice(alias($._command_name, $.bare_word), alias(token(prec(-1, new RegExp(['if', 'elseif', 'else', 'switch', 'foreach', 'for', 'while', 'do', 'until', 'try', 'catch', 'finally', 'trap', 'function', 'filter', 'workflow', 'class', 'enum', 'break', 'continue', 'return', 'throw', 'exit', 'data', 'configuration', 'param', 'dynamicparam', 'begin', 'process', 'end', 'clean'].map(word => ci(word).source).join('|')))), $.bare_word), alias($._bare_colon_word, $.bare_word)),
    invocation_operator_command: $ => seq(field('operator', choice('&', '.')), field('command', $._command_argument_value)),
    command_parameter: $ => seq(field('name', $.command_parameter_name), optional(seq(alias(token.immediate(prec(6, ':')), ':'), field('value', choice($._command_argument_value, alias($._command_argument_array, $.array_literal_expression), alias(token(prec(4, /[-–—―][^\s;|&()<>{},$'"`]+/)), $.bare_word)))))),
    command_argument: $ => choice($._command_argument_value, alias($._command_argument_array, $.array_literal_expression)),
    _command_argument_array: $ => seq($._command_argument_value, repeat1(seq(alias($._argument_separator, ','), repeat(/\r\n|\r|\n/), $._command_argument_value))),
    _command_argument_value: $ => choice(seq($.splat_expression, $._expression_argument_end), seq($._unary_expression, $._expression_argument_end), seq($.member_access, $._member_argument_end), prec.right(1, seq($.variable, repeat($._variable_argument_continuation), $._argument_end)), seq(choice($.bare_word, alias($._bare_colon_word, $.bare_word)), repeat($._generic_argument_continuation), $._argument_end)),
    _bare_colon_word: _ => token(prec(0, /:[^\s;|&()<>{},$'"‘’‚‛“”„`]*/)),
    _variable_argument_continuation: $ => choice(alias($._argument_continuation, $.line_continuation), alias($._variable_argument_fragment, $.argument_fragment), $._argument_interpolation),
    _generic_argument_continuation: $ => choice(alias($._argument_continuation, $.line_continuation), alias($._generic_argument_fragment, $.argument_fragment), $._argument_interpolation),
    _argument_continuation: _ => token.immediate(prec(1, /`[\r\n]/)),
    _variable_argument_fragment: _ => token.immediate(prec(4, /(?:(?:[^.\[?\s;|&()<>{},$'"‘’‚‛“”„`<>]|`(?:[^u\r\n]|u\{(?:[0-9A-Fa-f]{1,5}|0[0-9A-Fa-f]{5}|10[0-9A-Fa-f]{4})\}))(?:[^\s;|&()<>{},$'"‘’‚‛“”„`]|`(?:[^u\r\n]|u\{(?:[0-9A-Fa-f]{1,5}|0[0-9A-Fa-f]{5}|10[0-9A-Fa-f]{4})\}))*|\?(?:[^.\[\s;|&()<>{},$'"‘’‚‛“”„`][^\s;|&()<>{},$'"‘’‚‛“”„`]*)?)/)),
    _generic_argument_fragment: _ => token.immediate(prec(4, /(?:[^\s;|&()<>{},$'"‘’‚‛“”„`]|`(?:[^u\r\n]|u\{(?:[0-9A-Fa-f]{1,5}|0[0-9A-Fa-f]{5}|10[0-9A-Fa-f]{4})\}))+/)),
    _argument_interpolation: $ => choice(alias($._adjacent_variable, $.variable), alias($._argument_expandable_string, $.expandable_string), alias(token.immediate(/['‘’‚‛](?:[^'‘’‚‛]|['‘’‚‛]['‘’‚‛])*['‘’‚‛]/), $.single_quoted_string), alias($._argument_sub_expression, $.sub_expression)),
    _argument_expandable_string: $ => seq(choice(token.immediate('"'), token.immediate('“'), token.immediate('”'), token.immediate('„')), repeat(choice($.string_content, alias($._literal_dollar, $.string_content), alias($._string_escape, $.escape_sequence), $.variable, $.sub_expression)), choice(token.immediate('"'), token.immediate('“'), token.immediate('”'), token.immediate('„'))),
    _argument_sub_expression: $ => seq(token.immediate('$('), repeat($._terminator), optional($._statement_list), ')'),
    stop_parsing: _ => seq(token(prec(4, '--%')), optional(token(/[^\r\n|]+/))),
    redirection: $ => choice(field('operator', $.merging_redirection_operator), seq(field('operator', $.file_redirection_operator), field('destination', $._command_argument_value))),
    merging_redirection_operator: _ => /(?:[1-6]|\*)?>&[1-6]/,
    file_redirection_operator: _ => /(?:[1-6]|\*)?>>?/,

    _expression: $ => choice($.array_literal_expression, $._unary_expression, $.binary_expression, $.ternary_expression),
    array_literal_expression: $ => prec.left(PREC.array, seq($._unary_expression, repeat1(seq(',', repeat(/\r\n|\r|\n/), $._unary_expression)))),
    binary_expression: $ => binaryExpression($._expression, repeat(/\r\n|\r|\n/)),
    ternary_expression: $ => prec.right(PREC.ternary, seq(field('condition', $._expression), '?', repeat(/\r\n|\r|\n/), field('consequence', $._expression), ':', repeat(/\r\n|\r|\n/), field('alternative', $._expression))),
    // Commas delimit method/attribute arguments and parameter defaults. Parentheses
    // re-enter ordinary expression mode and may contain array literals.
    _argument_expression: $ => choice($._unary_expression, alias($._argument_binary_expression, $.binary_expression), alias($._argument_ternary_expression, $.ternary_expression)),
    _argument_binary_expression: $ => binaryExpression($._argument_expression, repeat(/\r\n|\r|\n/)),
    _argument_ternary_expression: $ => prec.right(PREC.ternary, seq(field('condition', $._argument_expression), '?', repeat(/\r\n|\r|\n/), field('consequence', $._argument_expression), ':', repeat(/\r\n|\r|\n/), field('alternative', $._argument_expression))),
    _unary_expression: $ => choice($._primary_expression, $.unary_expression, $.cast_expression),
    unary_expression: $ => prec(PREC.unary, choice(
      seq(field('operator', choice('+', op('-'), '!', ',', op('-not'), op('-bnot'), op('-split'), op('-join'))), field('argument', $._unary_expression)),
      seq(field('operator', choice('++', op('--'))), field('argument', $._assignable_expression)),
      seq(field('argument', $._assignable_expression), optional($._postfix_context), field('operator', choice(token.immediate(prec(5, '++')), alias(token.immediate(prec(5, /[-–—―][-–—―]/)), '--')))),
    )),
    _assignable_expression: $ => choice($.variable, $.member_access, $.element_access, $.parenthesized_expression),
    cast_expression: $ => prec(PREC.unary, seq(field('type', $.type_literal), field('value', $._unary_expression))),
    _primary_expression: $ => choice($._value_primary_expression, $.type_literal),
    _value_primary_expression: $ => prec(PREC.member, choice($.variable, $.integer_literal, $.real_literal, $.single_quoted_string, $.expandable_string, $.single_quoted_here_string, $.expandable_here_string, $.parenthesized_expression, $.sub_expression, $.array_expression, $.hashtable_expression, $.script_block, $.member_access, $.element_access, $.invocation_expression)),
    parenthesized_expression: $ => seq('(', repeat($._terminator), $.pipeline, repeat($._terminator), ')'),
    sub_expression: $ => seq('$(', repeat($._terminator), optional($._statement_list), ')'),
    array_expression: $ => seq('@(', repeat($._terminator), optional($._statement_list), ')'),
    hashtable_expression: $ => seq('@{', repeat($._terminator), repeat(seq($.hash_entry, repeat1($._terminator))), optional($.hash_entry), '}'),
    hash_entry: $ => seq(field('key', choice($.identifier, $._unary_expression)), '=', repeat(/\r\n|\r|\n/), field('value', $._statement)),
    member_access: $ => prec.left(PREC.member, seq(choice(seq(field('object', $._primary_expression), optional($._postfix_context), field('operator', choice(token.immediate(prec(5, '.')), token.immediate(prec(5, '::')), token.immediate(prec(5, '?.'))))), seq(field('object', $.type_literal), field('operator', '?.'))), repeat(/\r\n|\r|\n/), field('member', choice($.identifier, $.integer_literal, $.real_literal, $.variable, $.single_quoted_string, $.expandable_string, $.parenthesized_expression, $.sub_expression)))),
    element_access: $ => prec.left(PREC.member, seq(field('object', $._value_primary_expression), optional($._postfix_context), choice(token.immediate(prec(5, '[')), token.immediate(prec(5, '?['))), repeat(/\r\n|\r|\n/), field('index', $._expression), repeat(/\r\n|\r|\n/), ']')),
    invocation_expression: $ => prec.left(PREC.member, seq(field('function', $.member_access), optional($.generic_type_arguments), optional($._method_invocation_context), token.immediate('('), repeat(/\r\n|\r|\n/), optional(seq(commaSep1($._argument_expression), repeat(/\r\n|\r|\n/))), ')')),
    // Generic argument lists allow physical newlines around their separators;
    // the outer brackets of a type literal and assembly qualification do not.
    generic_type_arguments: $ => seq(token.immediate(prec(5, '[')), repeat(/\r\n|\r|\n/), $.type_name, repeat(/\r\n|\r|\n/), repeat(seq(',', repeat(/\r\n|\r|\n/), $.type_name, repeat(/\r\n|\r|\n/))), ']'),
    type_literal: $ => seq('[', $.type_name, optional($.assembly_name), ']'),
    assembly_name: _ => seq(',', token(/(?:[^\s,\[\]]+|,+[ \t\f\v]*(?:[^\s,\[\]]+)?)(?:[ \t\f\v]*,[ \t\f\v]*(?:[^\s,\[\]]+)?)*/)),
    type_name: $ => seq(
      field('name', $.type_identifier),
      optional(seq('[', repeat(/\r\n|\r|\n/), choice($.type_name, $.type_literal), repeat(/\r\n|\r|\n/), repeat(seq(',', repeat(/\r\n|\r|\n/), choice($.type_name, $.type_literal), repeat(/\r\n|\r|\n/))), ']')),
      repeat(seq('[', repeat(/\r\n|\r|\n/), repeat(','), ']')),
    ),
    type_identifier: _ => token(/[[\p{L}_]&&[\u0000-\uFFFF]][[[\p{L}\p{Nd}]&&[\u0000-\uFFFF]]._+#`\\]*/),
    identifier: _ => /[[\p{L}_]&&[\u0000-\uFFFF]][[\p{L}\p{Nd}_]&&[\u0000-\uFFFF]]*/,
    splat_expression: _ => token(/@(?:[$^?]|(?:[A-Za-z0-9_?\u0080-\uFFFF]+:)?[A-Za-z0-9_?\u0080-\uFFFF]+)/),
    single_quoted_string: _ => token(/['‘’‚‛](?:[^'‘’‚‛]|['‘’‚‛]['‘’‚‛])*['‘’‚‛]/),
    expandable_here_string: $ => choice($._empty_expandable_here_string, seq(
      $._expandable_here_string_start,
      repeat(choice(alias($._here_string_content, $.string_content), alias($._literal_dollar, $.string_content), alias($._string_escape, $.escape_sequence), $.variable, $.sub_expression)),
      $._here_string_end,
    )),
    expandable_string: $ => seq(choice('"', '“', '”', '„'), repeat(choice($.string_content, alias($._literal_dollar, $.string_content), alias($._string_escape, $.escape_sequence), $.variable, $.sub_expression)), choice(token.immediate('"'), token.immediate('“'), token.immediate('”'), token.immediate('„'))),
    string_content: _ => token(prec(1, /["“”„]["“”„]|[^"“”„$`]+/)),
    escape_sequence: _ => token(/`[\s\S]/),
  },
});
