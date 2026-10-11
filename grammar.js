// Syntax follows Microsoft's language specification and current PowerShell
// parser. See docs/grammar-sources.md for references and compatibility notes.
const {
  LETTER_CHARACTER_CLASS,
  LETTER_OR_DIGIT_CHARACTER_CLASS,
} = require('./src/identifier_chars');

// Use the pinned .NET UTF-16 classification rather than the generator's Unicode
// version. Adding punctuation preserves a single ordinary identifier word token.
const addCharacters = (characterClass, characters) =>
  characterClass.slice(0, -1) + characters + ']';
const IDENTIFIER_START = addCharacters(LETTER_CHARACTER_CLASS, '_');
const IDENTIFIER_CONTINUE = addCharacters(LETTER_OR_DIGIT_CHARACTER_CLASS, '_');
const TYPE_IDENTIFIER_CONTINUE = addCharacters(LETTER_OR_DIGIT_CHARACTER_CLASS, '._+#`\\\\');

const NEWLINE = /\r\n|\r|\n/;

// This order follows token.cs, including PowerShell's shared precedence for
// comparisons, type operators, splitting, joining, and bit shifts.
const PREC = {
  ternary: 1,
  logical: 2,
  bitwise: 3,
  comparison: 4,
  coalesce: 5,
  additive: 6,
  multiplicative: 7,
  format: 8,
  range: 9,
  array: 10,
  unary: 11,
  member: 12,
};

const commaSep1 = rule => seq(
  rule,
  repeat(seq(repeat(NEWLINE), ',', repeat(NEWLINE), rule)),
);

const argumentList = (open, argument) => seq(
  open,
  repeat(NEWLINE),
  optional(seq(commaSep1(argument), repeat(NEWLINE))),
  ')',
);

const ci = word => new RegExp(
  word.split('').map(character => /[a-z]/i.test(character)
    ? `[${character.toLowerCase()}${character.toUpperCase()}]`
    : character).join(''),
);

// Canonical aliases give case-insensitive keywords stable anonymous nodes.
const keyword = word => alias(token(prec(2, ci(word))), word);
const op = word => alias(
  token(prec(2, new RegExp(ci(word).source.replace(/-/g, '[-–—―]')))),
  word,
);
const flag = word => choice(
  ...Array.from({length: word.length - 1}, (_, i) =>
    alias(token(prec(2, ci(word.slice(0, i + 2)))), word)),
);

const CONTEXTUAL_KEYWORDS = [
  'if', 'elseif', 'else', 'switch', 'foreach', 'for', 'while', 'do', 'until',
  'try', 'catch', 'finally', 'trap', 'function', 'filter', 'workflow', 'class',
  'enum', 'break', 'continue', 'return', 'throw', 'exit', 'data', 'configuration',
  'param', 'dynamicparam', 'begin', 'process', 'end', 'clean',
];

// Bare argument fragments share their metacharacters and Unicode escape rules.
// PowerShell's lowercase `u escape accepts one to six digits up to U+10FFFF.
const ARGUMENT_CHARACTER = /[^\s;|&()<>{},$'"‘’‚‛“”„`]/.source;
const ARGUMENT_ESCAPE = /`(?:[^u\r\n]|u\{(?:[0-9A-Fa-f]{1,5}|0[0-9A-Fa-f]{5}|10[0-9A-Fa-f]{4})\})/.source;
const VARIABLE_FRAGMENT_START = /[^.\[?\s;|&()<>{},$'"‘’‚‛“”„`<>]/.source;
const QUESTION_FRAGMENT_START = /[^.\[\s;|&()<>{},$'"‘’‚‛“”„`]/.source;
const VARIABLE_ARGUMENT_FRAGMENT = new RegExp(
  `(?:(?:${VARIABLE_FRAGMENT_START}|${ARGUMENT_ESCAPE})` +
  `(?:${ARGUMENT_CHARACTER}|${ARGUMENT_ESCAPE})*|` +
  `\\?(?:${QUESTION_FRAGMENT_START}${ARGUMENT_CHARACTER}*)?)`,
);
const GENERIC_ARGUMENT_FRAGMENT = new RegExp(
  `(?:${ARGUMENT_CHARACTER}|${ARGUMENT_ESCAPE})+`,
);

const BINARY_OPERATORS = [
  [PREC.logical, ['-and', '-or', '-xor']],
  [PREC.bitwise, ['-band', '-bor', '-bxor']],
  [PREC.comparison, [
    '-eq', '-ne', '-gt', '-ge', '-lt', '-le',
    '-ieq', '-ine', '-igt', '-ige', '-ilt', '-ile',
    '-ceq', '-cne', '-cgt', '-cge', '-clt', '-cle',
    '-like', '-notlike', '-ilike', '-inotlike', '-clike', '-cnotlike',
    '-match', '-notmatch', '-imatch', '-inotmatch', '-cmatch', '-cnotmatch',
    '-contains', '-notcontains', '-icontains', '-inotcontains', '-ccontains', '-cnotcontains',
    '-in', '-notin', '-iin', '-inotin', '-cin', '-cnotin',
    '-replace', '-ireplace', '-creplace', '-split', '-isplit', '-csplit', '-join',
    '-is', '-isnot', '-as', '-shl', '-shr',
  ]],
  [PREC.coalesce, ['??']],
  [PREC.additive, ['+', '-']],
  [PREC.multiplicative, ['*', '/', '%']],
  [PREC.format, ['-f']],
  [PREC.range, ['..']],
];

const unaryExpression = ($, allowComma) => {
  const operators = ['+', op('-'), '!', op('-not'), op('-bnot'), op('-split'), op('-join')];
  if (allowComma) {
    operators.splice(3, 0, ',');
  }
  return prec(PREC.unary, choice(
    seq(field('operator', choice(...operators)), field('argument', $._unary_expression)),
    seq(
      field('operator', choice('++', op('--'))),
      field('argument', $._prefix_assignable_expression),
    ),
    seq(
      field('argument', $._assignable_expression),
      optional($._postfix_context),
      field('operator', choice(
        token.immediate(prec(5, '++')),
        alias(token.immediate(prec(5, /[-–—―][-–—―]/)), '--'),
      )),
    ),
  ));
};

const binaryExpression = (expression, newlines) => choice(
  ...BINARY_OPERATORS.map(([precedence, operators]) => {
    const alternatives = operators.map(operator => {
      if (operator.startsWith('-')) {
        return op(operator);
      }
      if (operator === '..') {
        return token(prec(5, operator));
      }
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

const scriptBlock = ($, open) => seq(
  open,
  repeat($._terminator),
  optional(seq($.param_block, repeat($._terminator))),
  optional($._script_block_body),
  '}',
);

module.exports = grammar({
  name: 'powershell',

  extras: $ => [
    /[ \t\f\v\u0085\u00A0\u1680\u2000-\u200A\u2028\u2029\u202F\u205F\u3000]+/,
    $.comment,
    $.line_continuation,
  ],

  // Keep this order synchronized with TokenType in src/scanner.c. The method
  // and postfix context tokens communicate lexical context and are not emitted.
  externals: $ => [
    $.single_quoted_here_string,
    $._expandable_here_string_start,
    $._command_name,
    $.bare_word,
    $.integer_literal,
    $.real_literal,
    $._here_string_content,
    $._here_string_end,
    $._empty_expandable_here_string,
    $._string_escape,
    $.variable,
    $._adjacent_variable,
    $._literal_dollar,
    $._argument_end,
    $._flow_label,
    $._switch_default,
    $._label_start,
    $._expression_argument_end,
    $._member_argument_end,
    $.command_parameter_name,
    $.function_name,
    $.comment,
    $._method_invocation_context,
    $.line_continuation,
    $._postfix_context,
    $._argument_separator,
    $.splat_expression,
  ],

  word: $ => $.identifier,
  supertypes: $ => [$._statement, $._expression, $._primary_expression],

  conflicts: $ => [
    [$._value_primary_expression, $.invocation_expression],
    [$._pipeline],
    [$.pipeline, $._background_pipeline],
    [$._simple_statement, $._background_pipeline],
    [$.if_statement],
    [$.try_statement],
    // Qualifiers share syntax but have distinct property and method field roles.
    [$.class_property, $.class_method],
    // Attribute lists prefix declarations and variable expressions. Preserve
    // both interpretations until a keyword or variable selects their context.
    [$.param_block, $.class_definition, $.enum_definition, $.attributed_expression],
    [$.class_definition, $.enum_definition, $.attributed_expression],
  ],

  rules: {
    source_file: $ => seq(
      repeat($._terminator),
      repeat(seq($.using_statement, repeat($._terminator))),
      optional(seq($.param_block, repeat($._terminator))),
      optional($._script_block_body),
    ),

    // Named blocks are permitted only at the beginning of a script body.
    _script_block_body: $ => choice(
      $._statement_list,
      repeat1(seq($.named_block, repeat($._terminator))),
    ),

    _statement_list: $ => choice(
      $._simple_statement,
      seq(repeat1($._terminated_statement), optional($._simple_statement)),
    ),

    // Completed block statements delimit themselves; other statements require
    // a newline or semicolon before the next statement.
    _terminated_statement: $ => choice(
      seq($._simple_statement, repeat1($._terminator)),
      seq($._block_statement, repeat($._terminator)),
    ),

    _terminator: $ => choice(';', NEWLINE),

    _newlines: $ => repeat1(NEWLINE),

    _statement: $ => choice($._block_statement, $._simple_statement),

    _block_statement: $ => choice(
      $.if_statement,
      $.switch_statement,
      $.foreach_statement,
      $.for_statement,
      $.while_statement,
      $.try_statement,
      $.trap_statement,
      $.function_definition,
      $.class_definition,
      $.enum_definition,
      $.data_statement,
      $.configuration_definition,
      alias($._break_continue_statement, $.flow_control_statement),
      alias($._background_pipeline, $.pipeline),
    ),

    _simple_statement: $ => choice(
      $.do_statement,
      $.flow_control_statement,
      alias($._pipeline, $.pipeline),
    ),

    statement_block: $ => seq('{', repeat($._terminator), optional($._statement_list), '}'),

    if_statement: $ => seq(
      keyword('if'),
      repeat(NEWLINE),
      field('condition', $.condition),
      repeat(NEWLINE),
      field('body', $.statement_block),
      repeat(seq(repeat(NEWLINE), field('elseif', $.elseif_clause))),
      optional(seq(repeat(NEWLINE), field('else', $.else_clause))),
    ),

    elseif_clause: $ => seq(
      keyword('elseif'),
      repeat(NEWLINE),
      field('condition', $.condition),
      repeat(NEWLINE),
      field('body', $.statement_block),
    ),

    else_clause: $ => seq(keyword('else'), repeat(NEWLINE), field('body', $.statement_block)),

    condition: $ => seq('(', repeat(NEWLINE), $.pipeline, repeat(NEWLINE), ')'),

    switch_statement: $ => seq(
      optional(field('label', $.label)),
      keyword('switch'),
      repeat(NEWLINE),
      choice(
        seq(repeat(field('parameters', $.switch_parameter)), field('condition', $.condition)),
        seq(
          repeat(field('parameters', $.switch_parameter)),
          $._switch_file_parameter,
          repeat(choice(field('parameters', $.switch_parameter), $._switch_file_parameter)),
        ),
      ),
      repeat(NEWLINE),
      '{',
      repeat($._terminator),
      repeat1(seq(field('clauses', $.switch_clause), repeat($._terminator))),
      '}',
    ),

    _switch_file_parameter: $ => seq(
      flag('-file'),
      repeat(NEWLINE),
      field('file', alias($._command_argument_value, $.command_argument)),
    ),

    switch_parameter: _ => choice(
      flag('-regex'),
      flag('-wildcard'),
      flag('-exact'),
      flag('-casesensitive'),
    ),

    switch_clause: $ => seq(
      field('condition', choice(
        alias($._switch_default, 'default'),
        alias($._command_argument_value, $.command_argument),
      )),
      repeat(NEWLINE),
      field('body', $.statement_block),
    ),

    label: $ => seq($._label_start, ':', field('name', $.identifier), repeat(NEWLINE)),

    foreach_statement: $ => seq(
      optional(field('label', $.label)),
      keyword('foreach'),
      repeat(NEWLINE),
      repeat(field('parameters', $.foreach_parameter)),
      '(',
      repeat(NEWLINE),
      field('variable', $.variable),
      repeat(NEWLINE),
      keyword('in'),
      repeat(NEWLINE),
      field('value', $.pipeline),
      repeat(NEWLINE),
      ')',
      repeat(NEWLINE),
      field('body', $.statement_block),
    ),

    foreach_parameter: _ => choice(op('-parallel'), op('-throttlelimit')),

    for_statement: $ => seq(
      optional(field('label', $.label)),
      keyword('for'),
      repeat(NEWLINE),
      '(',
      optional(field('initializer', $.pipeline)),
      optional(seq(
        ';',
        optional(field('condition', $.pipeline)),
        optional(seq(';', optional(field('update', $.pipeline)))),
      )),
      ')',
      repeat(NEWLINE),
      field('body', $.statement_block),
    ),

    while_statement: $ => seq(
      optional(field('label', $.label)),
      keyword('while'),
      repeat(NEWLINE),
      field('condition', $.condition),
      repeat(NEWLINE),
      field('body', $.statement_block),
    ),

    do_statement: $ => seq(
      optional(field('label', $.label)),
      keyword('do'),
      repeat(NEWLINE),
      field('body', $.statement_block),
      repeat(NEWLINE),
      choice(keyword('while'), keyword('until')),
      repeat(NEWLINE),
      field('condition', $.condition),
    ),

    try_statement: $ => seq(
      keyword('try'),
      repeat(NEWLINE),
      field('body', $.statement_block),
      choice(
        seq(
          repeat1(seq(repeat(NEWLINE), field('handlers', $.catch_clause))),
          optional(seq(repeat(NEWLINE), field('finally', $.finally_clause))),
        ),
        seq(repeat(NEWLINE), field('finally', $.finally_clause)),
      ),
    ),

    catch_clause: $ => seq(
      keyword('catch'),
      optional(commaSep1(field('types', $.type_literal))),
      repeat(NEWLINE),
      field('body', $.statement_block),
    ),

    finally_clause: $ => seq(
      keyword('finally'),
      repeat(NEWLINE),
      field('body', $.statement_block),
    ),

    trap_statement: $ => seq(
      keyword('trap'),
      optional(field('type', $.type_literal)),
      repeat(NEWLINE),
      field('body', $.statement_block),
    ),

    _break_continue_statement: $ => prec.right(
      seq(
        field('keyword', choice(keyword('break'), keyword('continue'))),
        optional(field('value', choice($._unary_expression, alias($._flow_label, $.identifier)))),
      ),
    ),

    flow_control_statement: $ => prec.right(
      seq(
        field('keyword', choice(keyword('return'), keyword('throw'), keyword('exit'))),
        optional(field('value', $.pipeline)),
      ),
    ),

    function_definition: $ => seq(
      field('keyword', choice(keyword('function'), keyword('filter'), keyword('workflow'))),
      repeat(NEWLINE),
      field('name', $.function_name),
      repeat(NEWLINE),
      optional(seq(field('parameters', $.parameter_list), repeat(NEWLINE))),
      field('body', $.script_block),
    ),

    parameter_list: $ => seq(
      '(',
      repeat(NEWLINE),
      optional(seq(commaSep1($.parameter), repeat(NEWLINE))),
      ')',
    ),

    param_block: $ => seq(
      repeat(seq(field('attributes', $.attribute), repeat(NEWLINE))),
      keyword('param'),
      repeat(NEWLINE),
      field('parameters', $.parameter_list),
    ),

    parameter: $ => seq(
      repeat(seq(choice(
        field('attributes', $.attribute),
        field('types', $.type_literal),
      ), repeat(NEWLINE))),
      field('name', $.variable),
      optional(seq('=', repeat(NEWLINE), field('default', $._argument_expression))),
    ),

    attribute: $ => seq(
      '[',
      field('name', $.type_name),
      field('arguments', alias($._attribute_argument_list, $.argument_list)),
      ']',
    ),

    _attribute_argument_list: $ => argumentList('(', $.attribute_argument),

    attribute_argument: $ => choice(
      seq(
        field('name', $.identifier),
        optional(seq('=', repeat(NEWLINE), field('value', $._argument_expression))),
      ),
      $._argument_expression,
    ),

    script_block: $ => scriptBlock($, '{'),

    // Method invocation also accepts one adjacent script block argument.
    _method_script_block: $ => scriptBlock($, token.immediate('{')),

    named_block: $ => seq(
      field(
        'name',
        choice(
          keyword('dynamicparam'),
          keyword('begin'),
          keyword('process'),
          keyword('end'),
          keyword('clean'),
        ),
      ),
      repeat(NEWLINE),
      field('body', $.statement_block),
    ),

    // Class methods delimit themselves and allow only newlines before the next
    // member. Properties accept semicolons too, or the final closing brace.
    class_definition: $ => seq(
      repeat(seq(field('attributes', $.attribute), repeat(NEWLINE))),
      keyword('class'),
      repeat(NEWLINE),
      field('name', $.identifier),
      optional(seq(':', repeat(NEWLINE), commaSep1(field('base_types', $.type_name)))),
      repeat(NEWLINE),
      field('body', $.class_body),
    ),

    class_body: $ => seq(
      '{',
      repeat(NEWLINE),
      repeat(
        choice(
          seq(field('members', $.class_property), repeat1($._terminator)),
          seq(field('members', $.class_method), repeat(NEWLINE)),
        ),
      ),
      optional(field('members', $.class_property)),
      '}',
    ),

    class_property: $ => seq(
      repeat(
        seq(
          choice(
            field('attributes', $.attribute),
            field('types', $.type_literal),
            field('modifiers', choice(keyword('hidden'), keyword('static'))),
          ),
          repeat(NEWLINE),
        ),
      ),
      field('name', $.variable),
      optional(seq('=', field('value', $._expression))),
    ),

    class_method: $ => seq(
      repeat(
        seq(
          choice(
            field('attributes', $.attribute),
            field('return_type', $.type_literal),
            field('modifiers', choice(keyword('hidden'), keyword('static'))),
          ),
          repeat(NEWLINE),
        ),
      ),
      field('name', $.identifier),
      field('parameters', $.parameter_list),
      optional(field('base_constructor', $.base_constructor_call)),
      repeat(NEWLINE),
      field('body', $.statement_block),
    ),

    base_constructor_call: $ => seq(
      ':',
      keyword('base'),
      field('arguments', alias($._base_argument_list, $.argument_list)),
    ),

    // Base constructors retain their existing non-immediate opener and newline
    // rules while sharing the public argument-list shape with ordinary calls.
    _base_argument_list: $ => seq(
      '(',
      optional(commaSep1($._argument_expression)),
      ')',
    ),

    enum_definition: $ => seq(
      repeat(seq(field('attributes', $.attribute), repeat(NEWLINE))),
      keyword('enum'),
      repeat(NEWLINE),
      field('name', $.identifier),
      optional(seq(':', repeat(NEWLINE), field('underlying_type', $.type_name))),
      repeat(NEWLINE),
      field('body', $.enum_body),
    ),

    enum_body: $ => seq(
      '{',
      repeat($._terminator),
      repeat(seq(field('members', $.enum_member), repeat1($._terminator))),
      optional(field('members', $.enum_member)),
      '}',
    ),

    enum_member: $ => seq(
      field('name', $.identifier),
      optional(seq('=', field('value', $._expression))),
    ),

    using_statement: $ => seq(
      keyword('using'),
      field('kind', choice(keyword('namespace'), keyword('module'), keyword('assembly'))),
      field(
        'value',
        choice($.bare_word, $.single_quoted_string, $.expandable_string, $.hashtable_expression),
      ),
    ),

    data_statement: $ => seq(
      keyword('data'),
      optional(field('name', $.identifier)),
      optional(
        seq(
          flag('-supportedcommand'),
          commaSep1(field('supported_commands', choice(
            $.bare_word,
            $.single_quoted_string,
            $.expandable_string,
          ))),
        ),
      ),
      repeat(NEWLINE),
      field('body', $.statement_block),
    ),

    configuration_definition: $ => seq(
      keyword('configuration'),
      field('name', choice($.identifier, $.variable)),
      repeat(NEWLINE),
      field('body', $.script_block),
    ),

    assignment_statement: $ => prec.right(
      seq(
        field('left', $._expression),
        field(
          'operator',
          choice(
            ...['=', '+=', '-=', '*=', '/=', '%=', '??='].map(o => alias(token(prec(3, o)), o)),
          ),
        ),
        repeat(NEWLINE),
        field('right', $._statement),
      ),
    ),

    pipeline: $ => choice($._pipeline, $._background_pipeline),

    _background_pipeline: $ => seq($._pipeline, '&'),

    _pipeline: $ => seq(
      choice($._pipeline_element, $.assignment_statement),
      repeat(seq(choice('|', $._leading_pipe), repeat(NEWLINE), $._pipeline_element)),
      repeat(
        seq(
          field('operator', choice('&&', '||')),
          repeat(NEWLINE),
          choice($._pipeline_element, $.assignment_statement),
          repeat(seq('|', $._pipeline_element)),
        ),
      ),
    ),

    _leading_pipe: $ => seq(NEWLINE, repeat(seq($.comment, NEWLINE)), '|'),

    _pipeline_element: $ => choice($.command, seq($._expression, repeat($.redirection))),

    command: $ => seq(
      field('name', choice($.command_name, $.invocation_operator_command)),
      repeat(choice($.command_argument, $.command_parameter, $.redirection)),
      optional(
        seq(
          alias(token(prec(3, '--')), $.command_parameter),
          repeat(choice($.command_argument, $.redirection)),
        ),
      ),
      optional($.stop_parsing),
    ),

    // Keywords remain literal command names outside their statement context.
    command_name: $ => choice(
      alias($._command_name, $.bare_word),
      alias(token(prec(-1,
        new RegExp(CONTEXTUAL_KEYWORDS.map(word => ci(word).source).join('|')),
      )), $.bare_word),
      alias($._bare_colon_word, $.bare_word),
    ),

    invocation_operator_command: $ => seq(
      field('operator', choice('&', '.')),
      // Keep the complete target in one field even when adjacent pieces expand.
      field('command', alias($._command_argument_value, $.command_argument)),
    ),

    command_parameter: $ => seq(
      field('name', $.command_parameter_name),
      optional(
        seq(
          alias(token.immediate(prec(6, ':')), ':'),
          field('value', alias($._command_parameter_value, $.command_argument)),
        ),
      ),
    ),

    _command_parameter_value: $ => choice(
      $._command_argument_value,
      alias($._command_argument_array, $.array_literal_expression),
      alias(token(prec(4, /[-–—―][^\s;|&()<>{},$'"`]+/)), $.bare_word),
    ),

    command_argument: $ => choice(
      $._command_argument_value,
      alias($._command_argument_array, $.array_literal_expression),
    ),

    // In argument mode, a comma following a value joins a single array argument.
    // A leading comma still belongs to the ordinary unary expression grammar.
    _command_argument_array: $ => seq(
      alias($._command_argument_value, $.command_argument),
      repeat1(
        seq(
          alias($._argument_separator, ','),
          repeat(NEWLINE),
          alias($._command_argument_value, $.command_argument),
        ),
      ),
    ),

    // Argument mode joins unquoted pieces and interpolation without whitespace.
    // Member access ends a variable's expandable suffix: $HOME-more is one
    // argument, whereas $HOME.Length-more has a member value and a parameter.
    _command_argument_value: $ => choice(
      seq($.splat_expression, $._expression_argument_end),
      seq($._unary_expression, $._expression_argument_end),
      seq($.member_access, $._member_argument_end),
      prec.right(1, seq($.variable, repeat($._variable_argument_continuation), $._argument_end)),
      seq(
        choice($.bare_word, alias($._bare_colon_word, $.bare_word)),
        repeat($._generic_argument_continuation),
        $._argument_end,
      ),
    ),

    _bare_colon_word: _ => token(prec(0, /:[^\s;|&()<>{},$'"‘’‚‛“”„`]*/)),

    _variable_argument_continuation: $ => choice(
      alias($._argument_continuation, $.line_continuation),
      alias($._variable_argument_fragment, $.argument_fragment),
      $._argument_interpolation,
    ),

    _generic_argument_continuation: $ => choice(
      alias($._argument_continuation, $.line_continuation),
      alias($._generic_argument_fragment, $.argument_fragment),
      $._argument_interpolation,
    ),

    _argument_continuation: _ => token.immediate(prec(1, /`[\r\n]/)),

    // Immediate tokens prevent spaces and other extras from joining fragments.
    _variable_argument_fragment: _ => token.immediate(prec(4, VARIABLE_ARGUMENT_FRAGMENT)),

    _generic_argument_fragment: _ => token.immediate(prec(4, GENERIC_ARGUMENT_FRAGMENT)),

    _argument_interpolation: $ => choice(
      alias($._adjacent_variable, $.variable),
      alias($._argument_expandable_string, $.expandable_string),
      alias(token.immediate(/['‘’‚‛](?:[^'‘’‚‛]|['‘’‚‛]['‘’‚‛])*['‘’‚‛]/), $.single_quoted_string),
      alias($._argument_sub_expression, $.sub_expression),
    ),

    _argument_expandable_string: $ => seq(
      choice(token.immediate('"'), token.immediate('“'), token.immediate('”'), token.immediate('„')),
      repeat(
        choice(
          $.string_content,
          alias($._literal_dollar, $.string_content),
          alias($._string_escape, $.escape_sequence),
          $.variable,
          $.sub_expression,
        ),
      ),
      choice(token.immediate('"'), token.immediate('“'), token.immediate('”'), token.immediate('„')),
    ),

    _argument_sub_expression: $ => seq(
      token.immediate('$('),
      repeat($._terminator),
      optional($._statement_list),
      ')',
    ),

    stop_parsing: _ => seq(token(prec(4, '--%')), optional(token(/[^\r\n|]+/))),

    redirection: $ => choice(
      field('operator', $.merging_redirection_operator),
      seq(
        field('operator', $.file_redirection_operator),
        field('destination', alias($._command_argument_value, $.command_argument)),
      ),
    ),

    merging_redirection_operator: _ => /(?:[1-6]|\*)?>&[1-6]/,

    file_redirection_operator: _ => /(?:[1-6]|\*)?>>?/,

    _expression: $ => choice(
      $.array_literal_expression,
      $._unary_expression,
      $.binary_expression,
      $.ternary_expression,
    ),

    array_literal_expression: $ => prec.left(
      PREC.array,
      seq($._unary_expression, repeat1(seq(',', repeat(NEWLINE), $._unary_expression))),
    ),

    binary_expression: $ => binaryExpression($._expression, repeat(NEWLINE)),

    ternary_expression: $ => prec.right(
      PREC.ternary,
      seq(
        field('condition', $._expression),
        '?',
        repeat(NEWLINE),
        field('consequence', $._expression),
        ':',
        repeat(NEWLINE),
        field('alternative', $._expression),
      ),
    ),

    // Commas delimit method/attribute arguments and parameter defaults. Parentheses
    // re-enter ordinary expression mode and may contain array literals.
    _argument_expression: $ => choice(
      $._unary_expression,
      alias($._argument_binary_expression, $.binary_expression),
      alias($._argument_ternary_expression, $.ternary_expression),
    ),

    _argument_binary_expression: $ => binaryExpression($._argument_expression, repeat(NEWLINE)),

    _argument_ternary_expression: $ => prec.right(
      PREC.ternary,
      seq(
        field('condition', $._argument_expression),
        '?',
        repeat(NEWLINE),
        field('consequence', $._argument_expression),
        ':',
        repeat(NEWLINE),
        field('alternative', $._argument_expression),
      ),
    ),

    _unary_expression: $ => choice(
      $._primary_expression,
      $.unary_expression,
      $.cast_expression,
      $.attributed_expression,
    ),

    unary_expression: $ => unaryExpression($, true),

    _cast_unary_expression: $ => unaryExpression($, false),

    _assignable_expression: $ => choice(
      $.variable,
      $.member_access,
      $.element_access,
      $.parenthesized_expression,
    ),

    _prefix_assignable_expression: $ => choice(
      $._assignable_expression,
      $.attributed_expression,
      alias($._attributed_variable_cast, $.cast_expression),
    ),

    cast_expression: $ => prec(
      PREC.unary,
      seq(field('type', $.type_literal), field('value', $._cast_value)),
    ),

    // Validation attributes can wrap a variable, including intervening type
    // conversions and attributes. Other expression targets are rejected.
    attributed_expression: $ => seq(
      field('attribute', $.attribute),
      repeat(NEWLINE),
      field('value', $._attributed_variable),
    ),

    _attributed_variable: $ => choice(
      $.variable,
      $.attributed_expression,
      alias($._attributed_variable_cast, $.cast_expression),
    ),

    _attributed_variable_cast: $ => seq(
      field('type', $.type_literal),
      field('value', $._attributed_variable),
    ),

    // A comma following a type ends a type expression, rather than becoming a
    // unary-comma cast value. Nested unary operators may still contain comma.
    _cast_value: $ => choice(
      $._primary_expression,
      $.cast_expression,
      $.attributed_expression,
      alias($._cast_unary_expression, $.unary_expression),
    ),

    _primary_expression: $ => choice($._value_primary_expression, $.type_literal),

    _value_primary_expression: $ => prec(
      PREC.member,
      choice(
        $.variable,
        $.integer_literal,
        $.real_literal,
        $.single_quoted_string,
        $.expandable_string,
        $.single_quoted_here_string,
        $.expandable_here_string,
        $.parenthesized_expression,
        $.sub_expression,
        $.array_expression,
        $.hashtable_expression,
        $.script_block,
        $.member_access,
        $.element_access,
        $.invocation_expression,
      ),
    ),

    parenthesized_expression: $ => seq(
      '(',
      repeat($._terminator),
      $.pipeline,
      repeat($._terminator),
      ')',
    ),

    sub_expression: $ => seq('$(', repeat($._terminator), optional($._statement_list), ')'),

    array_expression: $ => seq('@(', repeat($._terminator), optional($._statement_list), ')'),

    hashtable_expression: $ => seq(
      '@{',
      repeat($._terminator),
      repeat(seq($.hash_entry, repeat1($._terminator))),
      optional($.hash_entry),
      '}',
    ),

    hash_entry: $ => seq(
      field('key', choice($.identifier, $._unary_expression)),
      '=',
      repeat(NEWLINE),
      field('value', $._statement),
    ),

    member_access: $ => prec.left(
      PREC.member,
      seq(
        choice(
          seq(
            field('object', $._primary_expression),
            optional($._postfix_context),
            field(
              'operator',
              choice(
                token.immediate(prec(5, '.')),
                token.immediate(prec(5, '::')),
                token.immediate(prec(5, '?.')),
              ),
            ),
          ),
          seq(field('object', $.type_literal), field('operator', '?.')),
        ),
        repeat(NEWLINE),
        field(
          'member',
          choice(
            $.identifier,
            $.integer_literal,
            $.real_literal,
            $.variable,
            $.single_quoted_string,
            $.expandable_string,
            $.parenthesized_expression,
            $.sub_expression,
          ),
        ),
      ),
    ),

    element_access: $ => prec.left(
      PREC.member,
      seq(
        field('object', $._value_primary_expression),
        optional($._postfix_context),
        choice(token.immediate(prec(5, '[')), token.immediate(prec(5, '?['))),
        repeat(NEWLINE),
        field('index', $._expression),
        repeat(NEWLINE),
        ']',
      ),
    ),

    invocation_expression: $ => prec.left(
      PREC.member,
      seq(
        field('function', $.member_access),
        optional(field('type_arguments', $.generic_type_arguments)),
        optional($._method_invocation_context),
        choice(
          field('arguments', $.argument_list),
          field('arguments', alias($._method_script_block, $.script_block)),
        ),
      ),
    ),

    argument_list: $ => argumentList(token.immediate('('), $._argument_expression),

    // Generic argument lists allow physical newlines around their separators;
    // the outer brackets of a type literal and assembly qualification do not.
    generic_type_arguments: $ => seq(
      token.immediate(prec(5, '[')),
      repeat(NEWLINE),
      $.type_name,
      repeat(NEWLINE),
      repeat(seq(',', repeat(NEWLINE), $.type_name, repeat(NEWLINE))),
      ']',
    ),

    type_literal: $ => seq('[', $.type_name, optional($.assembly_name), ']'),

    assembly_name: _ => seq(
      ',',
      token(
        /(?:[^\s,\[\]]+|,+[ \t\f\v]*(?:[^\s,\[\]]+)?)(?:[ \t\f\v]*,[ \t\f\v]*(?:[^\s,\[\]]+)?)*/,
      ),
    ),

    type_name: $ => seq(
      field('name', $.type_identifier),
      optional(
        seq(
          '[',
          repeat(NEWLINE),
          choice($.type_name, $.type_literal),
          repeat(NEWLINE),
          repeat(
            seq(
              ',',
              repeat(NEWLINE),
              choice($.type_name, $.type_literal),
              repeat(NEWLINE),
            ),
          ),
          ']',
        ),
      ),
      repeat(seq('[', repeat(NEWLINE), repeat(','), ']')),
    ),

    // PowerShell scans type names using .NET UTF-16 Char.IsLetter/IsDigit,
    // with punctuation accepted only after the initial letter or underscore.
    type_identifier: _ => token(new RegExp(IDENTIFIER_START + TYPE_IDENTIFIER_CONTINUE + '*')),

    identifier: _ => new RegExp(IDENTIFIER_START + IDENTIFIER_CONTINUE + '*'),

    single_quoted_string: _ => token(/['‘’‚‛](?:[^'‘’‚‛]|['‘’‚‛]['‘’‚‛])*['‘’‚‛]/),

    // Here-string footer tokens include their preceding physical newline.
    expandable_here_string: $ => choice(
      $._empty_expandable_here_string,
      seq(
        $._expandable_here_string_start,
        repeat(
          choice(
            alias($._here_string_content, $.string_content),
            alias($._literal_dollar, $.string_content),
            alias($._string_escape, $.escape_sequence),
            $.variable,
            $.sub_expression,
          ),
        ),
        $._here_string_end,
      ),
    ),

    // Immediate closing delimiters keep trailing body whitespace as content.
    expandable_string: $ => seq(
      choice('"', '“', '”', '„'),
      repeat(
        choice(
          $.string_content,
          alias($._literal_dollar, $.string_content),
          alias($._string_escape, $.escape_sequence),
          $.variable,
          $.sub_expression,
        ),
      ),
      choice(token.immediate('"'), token.immediate('“'), token.immediate('”'), token.immediate('„')),
    ),

    string_content: _ => token(prec(1, /["“”„]["“”„]|[^"“”„$`]+/)),

    escape_sequence: _ => token(/`[\s\S]/),
  },
});
