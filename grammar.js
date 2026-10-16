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

// A newline advances a for-header slot, unless a preceding semicolon already
// advanced it. A semicolon after that newline therefore skips the next slot.
const forHeaderClause = (value, completed, next) => choice(
  seq(value, optional(seq(
    choice(';', repeat1(NEWLINE)),
    repeat(NEWLINE),
    optional(next),
  ))),
  seq(completed, optional(';'), repeat(NEWLINE), optional(next)),
  seq(';', repeat(NEWLINE), optional(next)),
);

// Assignment and flow-control values may end in a background pipeline. Reuse
// their syntax when recording a statement that already delimits its successor.
const assignmentStatement = ($, right) => prec.right(seq(
  field('left', $._expression),
  field('operator', choice(
    ...['=', '+=', '-=', '*=', '/=', '%=', '??='].map(operator =>
      alias(token(prec(3, operator)), operator)),
  )),
  repeat(NEWLINE),
  field('right', right),
));

const flowControlStatement = (word, value, required = false) => prec.right(seq(
  field('keyword', keyword(word)),
  required ? field('value', value) : optional(field('value', value)),
));

const breakContinueStatement = ($, word) => prec.right(seq(
  field('keyword', keyword(word)),
  choice(
    seq($._flow_expression_start, field('label', $._flow_unary_expression)),
    $._flow_label_end,
  ),
));

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

const CONTEXTUAL_KEYWORDS = [
  'if', 'elseif', 'else', 'switch', 'foreach', 'for', 'while', 'do', 'until',
  'try', 'catch', 'finally', 'trap', 'function', 'filter', 'workflow', 'class',
  'enum', 'break', 'continue', 'return', 'throw', 'exit', 'data', 'configuration',
  'dynamicparam', 'begin', 'process', 'end', 'clean',
];

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

const unaryExpression = ($, argument, allowComma, options = {}) => {
  const operators = ['+', op('-'), '!', op('-not'), op('-bnot'), op('-split'), op('-join')];
  if (allowComma) {
    operators.splice(3, 0, ',');
  }
  const argumentStart = options.argumentStart ? [options.argumentStart] : [];
  const prefixStart = options.prefixStart ? [options.prefixStart] : [];
  const alternatives = [
    seq(
      field('operator', choice(...operators)),
      repeat(NEWLINE),
      ...argumentStart,
      field('argument', argument),
    ),
    seq(
      field('operator', choice('++', op('--'))),
      repeat(NEWLINE),
      ...prefixStart,
      field('argument', options.prefixAssignable || $._prefix_assignable_expression),
    ),
  ];
  if (options.postfix !== false) {
    alternatives.push(seq(
      field('argument', options.assignable || $._assignable_expression),
      optional($._postfix_context),
      field('operator', choice(
        token(prec(5, '++')),
        alias(token(prec(5, /[-–—―][-–—―]/)), '--'),
      )),
    ));
  }
  return prec(PREC.unary, choice(...alternatives));
};

const primaryValue = ($, allowSigned = true, commandStrings = false) => choice(
  $.variable,
  $.integer_literal,
  $.real_literal,
  ...(allowSigned ? [alias($._signed_numeric_expression, $.unary_expression)] : []),
  $.single_quoted_string,
  commandStrings ? alias($._command_expandable_string, $.expandable_string) : $.expandable_string,
  $.single_quoted_here_string,
  commandStrings
    ? alias($._command_expandable_here_string, $.expandable_here_string)
    : $.expandable_here_string,
  $.parenthesized_expression,
  $.sub_expression,
  $.array_expression,
  $.hashtable_expression,
  $.script_block,
);

const valuePrimaryExpression = ($, member, element, invocation, commandStrings = false) => prec(
  PREC.member,
  choice(primaryValue($, true, commandStrings), member, element, invocation),
);

const memberAccess = ($, primary, member, start = null) => prec.left(
  PREC.member,
  seq(
    choice(
      seq(
        field('object', primary),
        optional($._postfix_context),
        field('operator', choice(
          token.immediate(prec(5, '.')),
          token.immediate(prec(5, '::')),
          token.immediate(prec(5, '?.')),
        )),
      ),
      seq(field('object', $.type_literal), field('operator', '?.')),
    ),
    repeat(NEWLINE),
    ...(start ? [start] : []),
    field('member', member),
  ),
);

const elementAccess = ($, primary) => prec.left(
  PREC.member,
  seq(
    field('object', primary),
    optional($._postfix_context),
    choice(token.immediate(prec(5, '[')), token.immediate(prec(5, '?['))),
    repeat(NEWLINE),
    field('index', $._expression_head),
    repeat(NEWLINE),
    ']',
  ),
);

const invocationExpression = ($, member, generic = true) => prec.left(
  PREC.member,
  seq(
    field('function', member),
    ...(generic ? [optional(field('type_arguments', $.generic_type_arguments))] : []),
    optional($._method_invocation_context),
    choice(
      field('arguments', $.argument_list),
      field('arguments', alias($._method_script_block, $.script_block)),
    ),
  ),
);

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

const grammarDefinition = {
  name: 'powershell',

  extras: $ => [
    /[ \t\f\v\u0085\u00A0\u1680\u2000-\u200A\u2028\u2029\u202F\u205F\u3000]+/,
    $.comment,
    $.line_continuation,
    // NextToken discards a token-leading backtick followed by horizontal
    // whitespace. Generic words still consume their own interior escapes.
    $._escaped_whitespace,
    $._prepared_token,
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
    $._verbatim_arguments_start,
    $._verbatim_argument_text,
    $._braced_argument_variable,
    $._flow_expression_start,
    $._flow_label_end,
    $._command_member_start,
    $._command_unary_start,
    $._data_command_name,
    $._signed_number_start,
    $._cast_value_context,
    $.single_quoted_string,
    $._adjacent_single_quoted_string,
    $.string_content,
    $._variable_argument_fragment,
    $._generic_argument_fragment,
    $._double_quote,
    $._adjacent_double_quote,
    $._prepared_token,
    $._left_paren,
    $._adjacent_left_paren,
    $._right_paren,
    $._sub_expression_open,
    $._adjacent_sub_expression_open,
    $._sub_expression_end,
    $._array_expression_open,
    $._assembly_name_content,
    $._literal_parameter_token,
    $._end_of_parameters_token,
    $._escaped_whitespace,
    $._argument_expression_start,
    $._attribute_expression_start,
    $._member_expression_start,
    $._index_expression_start,
    $._clause_keyword_end,
    $._switch_regex_parameter,
    $._switch_wildcard_parameter,
    $._switch_exact_parameter,
    $._switch_casesensitive_parameter,
    $._switch_file_parameter_token,
    $._data_supported_command_parameter,
    $._foreach_parallel_parameter,
    $._foreach_throttlelimit_parameter,
    $._param_block_keyword,
    $._param_command_name,
    $._redirection_command_less_than,
    $._interpolation_open,
    $._interpolation_tail,
    $._command_interpolation_context,
  ],

  word: $ => $.identifier,

  conflicts: $ => [
    [$._interpolation_primary_expression, $._interpolation_invocation_expression],
    [$._command_interpolation_primary_expression, $._command_interpolation_invocation_expression],
    [$._value_primary_expression, $.invocation_expression],
    [$._argument_value_primary_expression, $._argument_invocation_expression],
    [$._command_value_primary_expression, $._command_invocation_expression],
    [$._assignable_expression, $._value_primary_expression, $.invocation_expression],
    [
      $._argument_assignable_expression,
      $._argument_value_primary_expression,
      $._argument_invocation_expression,
    ],
    [
      $._command_assignable_expression,
      $._command_value_primary_expression,
      $._command_invocation_expression,
    ],
    [$.pipeline],
    [$._pipeline_entry, $._completed_pipeline_entry],
    [$._statement, $._completed_assignment_statement],
    [$.if_statement],
    [$.try_statement],
    [$.parameter],
    [$.for_statement],
    [$._for_header_initializer],
    [$._for_header_condition],
    [$._for_header_initializer, $._for_header_condition],
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
      repeat(seq($.using_statement, repeat1($._terminator))),
      optional(choice(
        $.using_statement,
        seq($.param_block, repeat($._terminator), optional($._script_block_body)),
        $._script_block_body,
      )),
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
      $.do_statement,
      $.try_statement,
      $.trap_statement,
      $.function_definition,
      $.class_definition,
      $.enum_definition,
      $.data_statement,
      $.configuration_definition,
      $.break_statement,
      $.continue_statement,
      $.background_pipeline,
      alias($._completed_assignment_statement, $.assignment_statement),
      alias($._completed_return_statement, $.return_statement),
      alias($._completed_throw_statement, $.throw_statement),
      alias($._completed_exit_statement, $.exit_statement),
    ),

    _simple_statement: $ => choice(
      $.return_statement,
      $.throw_statement,
      $.exit_statement,
      $.assignment_statement,
      $.pipeline,
      $.pipeline_chain,
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
      $._clause_keyword_end,
      repeat(NEWLINE),
      field('condition', $.condition),
      repeat(NEWLINE),
      field('body', $.statement_block),
    ),

    else_clause: $ => seq(
      keyword('else'),
      $._clause_keyword_end,
      repeat(NEWLINE),
      field('body', $.statement_block),
    ),

    condition: $ => seq('(', repeat(NEWLINE), $._pipeline_entry, repeat(NEWLINE), ')'),

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
      repeat(NEWLINE),
      repeat1(seq(field('clauses', $.switch_clause), repeat($._terminator))),
      '}',
    ),

    _switch_file_parameter: $ => seq(
      alias($._switch_file_parameter_token, '-file'),
      repeat(NEWLINE),
      field('file', alias($._single_command_argument_value, $.command_argument)),
    ),

    switch_parameter: $ => choice(
      alias($._switch_regex_parameter, '-regex'),
      alias($._switch_wildcard_parameter, '-wildcard'),
      alias($._switch_exact_parameter, '-exact'),
      alias($._switch_casesensitive_parameter, '-casesensitive'),
    ),

    switch_clause: $ => seq(
      field('condition', choice(
        alias($._switch_default, 'default'),
        alias($._single_command_argument_value, $.command_argument),
      )),
      repeat(NEWLINE),
      field('body', $.statement_block),
    ),

    label: $ => seq($._label_start, ':', field('name', $.identifier), repeat(NEWLINE)),

    foreach_statement: $ => seq(
      optional(field('label', $.label)),
      keyword('foreach'),
      repeat(NEWLINE),
      repeat(seq(field('parameters', $.foreach_parameter), repeat(NEWLINE))),
      '(',
      repeat(NEWLINE),
      field('variable', $.variable),
      repeat(NEWLINE),
      keyword('in'),
      repeat(NEWLINE),
      field('value', $._pipeline_entry),
      repeat(NEWLINE),
      ')',
      repeat(NEWLINE),
      field('body', $.statement_block),
    ),

    foreach_parameter: $ => choice(
      alias($._foreach_parallel_parameter, '-parallel'),
      seq(
        alias($._foreach_throttlelimit_parameter, '-throttlelimit'),
        repeat(NEWLINE),
        field('value', alias($._command_parameter_value, $.command_argument)),
      ),
    ),

    for_statement: $ => seq(
      optional(field('label', $.label)),
      keyword('for'),
      repeat(NEWLINE),
      '(',
      repeat(NEWLINE),
      optional($._for_header_initializer),
      repeat(NEWLINE),
      ')',
      repeat(NEWLINE),
      field('body', $.statement_block),
    ),

    _for_header_initializer: $ => forHeaderClause(
      field('initializer', $._pipeline_entry),
      field('initializer', $._completed_pipeline_entry),
      $._for_header_condition,
    ),

    _for_header_condition: $ => forHeaderClause(
      field('condition', $._pipeline_entry),
      field('condition', $._completed_pipeline_entry),
      field('update', $._pipeline_entry),
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
      $._clause_keyword_end,
      repeat(NEWLINE),
      optional(seq(commaSep1(field('types', $.type_literal)), repeat(NEWLINE))),
      field('body', $.statement_block),
    ),

    finally_clause: $ => seq(
      keyword('finally'),
      $._clause_keyword_end,
      repeat(NEWLINE),
      field('body', $.statement_block),
    ),

    trap_statement: $ => seq(
      keyword('trap'),
      repeat(NEWLINE),
      optional(seq(field('type', $.type_literal), repeat(NEWLINE))),
      field('body', $.statement_block),
    ),

    break_statement: $ => breakContinueStatement($, 'break'),

    continue_statement: $ => breakContinueStatement($, 'continue'),

    return_statement: $ => flowControlStatement('return', $._pipeline_entry),

    throw_statement: $ => flowControlStatement('throw', $._pipeline_entry),

    exit_statement: $ => flowControlStatement('exit', $._pipeline_entry),

    _completed_return_statement: $ => prec.dynamic(1,
      flowControlStatement('return', $._completed_pipeline_entry, true)),

    _completed_throw_statement: $ => prec.dynamic(1,
      flowControlStatement('throw', $._completed_pipeline_entry, true)),

    _completed_exit_statement: $ => prec.dynamic(1,
      flowControlStatement('exit', $._completed_pipeline_entry, true)),

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

    // The scanner commits to a leading parameter block when its list begins.
    param_block: $ => seq(
      repeat(seq(field('attributes', $.attribute), repeat(NEWLINE))),
      alias($._param_block_keyword, 'param'),
      repeat(NEWLINE),
      field('parameters', $.parameter_list),
    ),

    parameter: $ => seq(
      repeat(seq(choice(
        field('attributes', $.attribute),
        field('types', $.type_literal),
      ), repeat(NEWLINE))),
      field('name', $.variable),
      optional(seq(
        repeat(NEWLINE),
        '=',
        repeat(NEWLINE),
        field('default', $._argument_expression_head),
      )),
    ),

    attribute: $ => seq(
      '[',
      repeat(NEWLINE),
      field('name', $.type_name),
      field('arguments', alias($._attribute_argument_list, $.argument_list)),
      repeat(NEWLINE),
      ']',
    ),

    _attribute_argument_list: $ => seq(
      '(',
      repeat(NEWLINE),
      optional(seq(
        optional(seq(',', repeat(NEWLINE))),
        commaSep1($.attribute_argument),
        repeat(NEWLINE),
      )),
      ')',
    ),

    attribute_argument: $ => choice(
      seq(
        field('name', $.identifier),
        optional(seq('=', repeat(NEWLINE), field('value', $._attribute_expression_head))),
      ),
      $._attribute_expression_head,
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
      repeat(NEWLINE),
      optional(seq(
        ':', repeat(NEWLINE), commaSep1(field('base_types', $.type_name)), repeat(NEWLINE),
      )),
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
      optional(seq('=', repeat(NEWLINE), field('value', $._expression))),
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
      repeat(NEWLINE),
      optional(seq(field('base_constructor', $.base_constructor_call), repeat(NEWLINE))),
      field('body', $.statement_block),
    ),

    base_constructor_call: $ => seq(
      ':',
      repeat(NEWLINE),
      keyword('base'),
      repeat(NEWLINE),
      field('arguments', alias($._base_argument_list, $.argument_list)),
    ),

    _base_argument_list: $ => argumentList('(', $._argument_expression_head),

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
      choice(
        seq(
          field('kind', keyword('module')),
          field('value', choice($._using_argument_value, $.hashtable_expression)),
        ),
        seq(
          field('kind', choice(keyword('namespace'), keyword('assembly'))),
          field('value', $._using_argument_value),
        ),
      ),
    ),

    // Using values begin in command-argument mode. Unquoted text can include
    // adjacent quote fragments; a quoted leading value instead ends as one string.
    _using_argument_value: $ => choice(
      seq(alias($._using_generic_argument_value, $.command_argument), $._argument_end),
      seq(choice($.single_quoted_string, $.expandable_string), $._argument_end),
    ),

    _using_generic_argument_value: $ => seq(
      choice($.bare_word, alias($._bare_colon_word, $.bare_word)),
      repeat($._generic_argument_continuation),
    ),

    data_statement: $ => seq(
      keyword('data'),
      repeat(NEWLINE),
      optional(seq(field('name', $.identifier), repeat(NEWLINE))),
      optional(
        seq(
          alias($._data_supported_command_parameter, '-supportedcommand'),
          repeat(NEWLINE),
          field('supported_commands', $._data_supported_command),
          repeat(seq(
            ',',
            repeat(NEWLINE),
            field('supported_commands', $._data_supported_command),
          )),
          repeat(NEWLINE),
        ),
      ),
      field('body', $.statement_block),
    ),

    // Supported commands use command-name mode: generic text stays literal,
    // while a standalone variable, quoted value, or grouped value is a primary
    // expression. Unary operators are literal names in this context too.
    _data_supported_command: $ => choice(
      alias($._data_command_name, $.bare_word),
      $._data_command_primary_expression,
    ),

    _data_command_primary_expression: $ => prec(PREC.member, choice(
      primaryValue($, false, true),
      $.splat_expression,
      alias($._data_command_member_access, $.member_access),
      alias($._data_command_element_access, $.element_access),
      alias($._data_command_invocation_expression, $.invocation_expression),
    )),

    _data_command_member_access: $ => memberAccess(
      $, $._data_command_primary_expression, $._command_member_name, $._command_member_start,
    ),

    _data_command_element_access: $ => elementAccess($, $._data_command_primary_expression),

    _data_command_invocation_expression: $ => invocationExpression(
      $, alias($._data_command_member_access, $.member_access), false,
    ),

    configuration_definition: $ => seq(
      keyword('configuration'),
      field('name', choice($.identifier, $.variable)),
      repeat(NEWLINE),
      field('body', $.script_block),
    ),

    assignment_statement: $ => assignmentStatement($, $._statement),

    _completed_assignment_statement: $ => prec.dynamic(1,
      assignmentStatement($, $._block_statement)),

    // Assignments are recognized before pipeline chaining by PowerShell. They
    // are available at the start of a statement or pipeline-value context, but
    // a chain's right operand is always an ordinary pipeline.
    _pipeline_entry: $ => choice(
      $.assignment_statement,
      $.pipeline,
      $.pipeline_chain,
      $.background_pipeline,
    ),

    _completed_pipeline_entry: $ => choice(
      $.background_pipeline,
      alias($._completed_assignment_statement, $.assignment_statement),
    ),

    pipeline: $ => seq(
      field('elements', choice($.command, $.command_expression)),
      repeat(seq(
        choice('|', $._leading_pipe),
        repeat(NEWLINE),
        field('elements', $.command),
      )),
    ),

    pipeline_chain: $ => prec.left(seq(
      field('left', choice($.pipeline, $.pipeline_chain)),
      field('operator', choice('&&', '||')),
      repeat(NEWLINE),
      field('right', $.pipeline),
    )),

    background_pipeline: $ => seq(
      field('pipeline', choice($.pipeline, $.pipeline_chain)),
      field('operator', '&'),
    ),

    _leading_pipe: $ => seq(NEWLINE, repeat(seq($.comment, NEWLINE)), '|'),

    command_expression: $ => seq(
      field('expression', $._expression),
      repeat(field('redirections', $.redirection)),
    ),

    command: $ => choice(
      seq(
        field('name', choice($.command_name, $.invocation_operator_command)),
        repeat(choice($.command_argument, $.command_parameter, $.redirection)),
        optional(seq(
          alias($._end_of_parameters, $.command_parameter),
          repeat($._command_element_after_end_of_parameters),
        )),
        optional($.verbatim_arguments),
      ),
      seq(
        field('name', alias(
          $._invocation_end_of_parameters_command, $.invocation_operator_command,
        )),
        repeat($._command_element_after_end_of_parameters),
        optional($.verbatim_arguments),
      ),
      seq(
        field('name', alias($._invocation_verbatim_command, $.invocation_operator_command)),
        optional(alias($._verbatim_command_argument, $.command_argument)),
      ),
    ),

    _command_element_after_end_of_parameters: $ => choice(
      $.command_argument,
      alias($._literal_parameter_value, $.command_argument),
      alias($._subsequent_end_of_parameters, $.command_argument),
      $.redirection,
    ),

    _invocation_end_of_parameters_command: $ => seq(
      field('operator', choice('&', '.')),
      field('command', alias($._end_of_parameters, $.command_parameter)),
    ),

    _invocation_verbatim_command: $ => seq(
      field('operator', choice('&', '.')),
      field('command', alias($._verbatim_command_name, $.command_argument)),
    ),

    _verbatim_command_name: $ => alias($._verbatim_arguments_start, '--%'),

    _verbatim_command_argument: $ => alias($._verbatim_argument_text, $.string_content),

    // The end marker is a parameter too, with the same complete name field.
    _end_of_parameters: $ => seq(
      field('name', alias($._end_of_parameters_token, $.command_parameter_name)),
    ),

    // Keywords remain literal command names outside their statement context.
    command_name: $ => choice(
      alias($._command_name, $.bare_word),
      alias($._param_command_name, $.bare_word),
      alias(token(prec(-1,
        new RegExp(CONTEXTUAL_KEYWORDS.map(word => ci(word).source).join('|')),
      )), $.bare_word),
      alias($._bare_colon_word, $.bare_word),
      $._redirection_command_name,
    ),

    _redirection_command_name: $ => choice(
      alias($.file_redirection_operator, $.bare_word),
      alias($.merging_redirection_operator, $.bare_word),
      alias($._redirection_command_less_than, $.bare_word),
    ),

    invocation_operator_command: $ => seq(
      field('operator', choice('&', '.')),
      // Keep the complete target in one field even when adjacent pieces expand.
      field('command', alias(choice(
        $._single_command_argument_value,
        $._redirection_command_name,
      ), $.command_argument)),
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

    // Parameter tokens retain their raw lexical boundaries when used as
    // values. Their dollars and backticks are literal, and a trailing colon
    // belongs to the token rather than attaching another argument.
    _literal_parameter_value: $ => alias($._literal_parameter_token, $.bare_word),

    _single_command_argument_value: $ => choice(
      $._command_argument_value,
      $._literal_parameter_value,
    ),

    _subsequent_end_of_parameters: $ => alias($._end_of_parameters_token, $.bare_word),

    _command_parameter_value: $ => choice(
      $._single_command_argument_value,
      alias($._command_parameter_array, $.array_literal_expression),
    ),

    _command_parameter_array: $ => seq(
      alias($._single_command_argument_value, $.command_argument),
      repeat1(seq(
        alias($._argument_separator, ','),
        repeat(NEWLINE),
        alias($._single_command_argument_value, $.command_argument),
      )),
    ),

    command_argument: $ => choice(
      $._command_argument_value,
      alias($._command_argument_array, $.array_literal_expression),
    ),

    // In argument mode, a comma following a value joins one array argument.
    // A comma cannot start a command argument.
    _command_argument_array: $ => seq(
      alias($._command_argument_value, $.command_argument),
      repeat1(
        seq(
          alias($._argument_separator, ','),
          repeat(NEWLINE),
          alias($._single_command_argument_value, $.command_argument),
        ),
      ),
    ),

    // Argument mode joins unquoted pieces and interpolation without whitespace.
    // Member access ends a variable's expandable suffix: $HOME-more is one
    // argument, whereas $HOME.Length-more has a member value and a parameter.
    _command_argument_value: $ => choice(
      seq($.splat_expression, $._expression_argument_end),
      seq($._command_argument_unary_expression, $._expression_argument_end),
      seq(alias($._command_member_access, $.member_access), $._member_argument_end),
      prec.right(1, seq(
        $.variable,
        optional(seq(
          $._variable_argument_continuation,
          repeat($._generic_argument_continuation),
        )),
        $._argument_end,
      )),
      // A braced variable followed immediately by '=' continues a generic
      // argument. An unbraced variable ends before that character instead.
      prec.right(1, seq(
        alias($._braced_argument_variable, $.variable),
        repeat($._generic_argument_continuation),
        $._argument_end,
      )),
      seq(
        choice($.bare_word, alias($._bare_colon_word, $.bare_word)),
        repeat($._generic_argument_continuation),
        $._argument_end,
      ),
    ),

    _bare_colon_word: _ => token(prec(0, /:[^\s;|&()<>{},$'"‘’‚‛“”„]*/)),

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

    _argument_continuation: _ => token.immediate(prec(1, /`(?:\r\n|\r|\n)/)),

    // Immediate external tokens keep spaces and extras from joining fragments.

    _argument_interpolation: $ => choice(
      alias($._adjacent_variable, $.variable),
      alias(token.immediate('$'), $.argument_fragment),
      alias($._argument_expandable_string, $.expandable_string),
      alias($._adjacent_single_quoted_string, $.single_quoted_string),
      alias($._command_interpolation, $.interpolation),
    ),

    _argument_expandable_string: $ => seq(
      alias($._adjacent_double_quote, '"'),
      repeat(
        choice(
          $.string_content,
          alias($._literal_dollar, $.string_content),
          alias($._string_escape, $.escape_sequence),
          $.variable,
          alias($._command_interpolation, $.interpolation),
        ),
      ),
      alias($._double_quote, '"'),
    ),

    _command_expandable_string: $ => seq(
      alias($._double_quote, '"'),
      repeat(
        choice(
          $.string_content,
          alias($._literal_dollar, $.string_content),
          alias($._string_escape, $.escape_sequence),
          $.variable,
          alias($._command_interpolation, $.interpolation),
        ),
      ),
      alias($._double_quote, '"'),
    ),

    verbatim_arguments: $ => seq(
      alias($._verbatim_arguments_start, '--%'),
      optional(field('text', alias($._verbatim_argument_text, $.string_content))),
    ),

    redirection: $ => choice(
      field('operator', $.merging_redirection_operator),
      seq(
        field('operator', $.file_redirection_operator),
        field('destination', alias($._single_command_argument_value, $.command_argument)),
      ),
    ),

    merging_redirection_operator: _ => /(?:[1-6]>&[12]|\*>&1)/,

    file_redirection_operator: _ => /(?:[1-6]|\*)?>>?/,

    _expression_head: $ => seq(
      $._index_expression_start,
      $._expression,
    ),

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
        repeat(NEWLINE),
        ':',
        repeat(NEWLINE),
        field('alternative', $._expression),
      ),
    ),

    // Commas delimit method/attribute arguments and parameter defaults. Parentheses
    // re-enter ordinary expression mode and may contain array literals.
    _attribute_expression_head: $ => seq(
      $._attribute_expression_start,
      $._argument_expression,
    ),

    _argument_expression_head: $ => seq(
      $._argument_expression_start,
      $._argument_expression,
    ),

    _argument_expression: $ => choice(
      $._argument_unary_expression,
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
        repeat(NEWLINE),
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

    unary_expression: $ => unaryExpression($, $._unary_expression, true),

    // An adjacent numeric sign belongs to the primary before member access.
    // A spaced sign remains an ordinary unary operator over its full operand.
    _signed_numeric_expression: $ => prec(PREC.member, seq(
      $._signed_number_start,
      field('operator', choice('+', op('-'))),
      field('argument', choice(
        $.integer_literal,
        $.real_literal,
        // PowerShell accepts an empty binary prefix only with an adjacent sign
        // and an unsigned suffix; unsigned bare 0bu remains a generic word.
        alias(token.immediate(/0[bB][uU][lLsSyY]?(?:[kKmMgGtTpP][bB])?/), $.integer_literal),
      )),
    )),

    _cast_unary_expression: $ => unaryExpression($, $._unary_expression, false),

    // The parser disables unary comma recursively in method arguments,
    // attribute arguments, and parameter defaults. Parentheses, indexers,
    // subexpressions, arrays, and script block expressions restore it.
    _argument_unary_expression: $ => choice(
      $._argument_primary_expression,
      alias($._argument_unary_operator_expression, $.unary_expression),
      alias($._argument_cast_expression, $.cast_expression),
      $.attributed_expression,
    ),

    _argument_unary_operator_expression: $ => unaryExpression(
      $, $._argument_unary_expression, false, {
        assignable: $._argument_assignable_expression,
        prefixAssignable: $._argument_prefix_assignable_expression,
      },
    ),

    _flow_unary_expression: $ => choice(
      alias($._flow_label, $.identifier),
      $._comma_free_command_primary_expression,
      alias($._flow_unary_operator_expression, $.unary_expression),
      alias($._flow_cast_expression, $.cast_expression),
      $.attributed_expression,
    ),

    _flow_unary_operator_expression: $ => unaryExpression(
      $, $._flow_unary_expression, false, {
        argumentStart: $._flow_expression_start,
        assignable: $._comma_free_command_assignable_expression,
        prefixStart: $._flow_expression_start,
        prefixAssignable: $._comma_free_command_prefix_assignable_expression,
      },
    ),

    _flow_cast_expression: $ => prec(
      PREC.unary,
      seq(
        field('type', $.type_literal),
        $._flow_expression_start,
        optional($._cast_value_context),
        field('value', $._flow_unary_expression),
      ),
    ),

    _argument_cast_expression: $ => prec(
      PREC.unary,
      seq(
        field('type', $.type_literal),
        optional($._cast_value_context),
        field('value', $._argument_unary_expression),
      ),
    ),

    // Command mode first peeks at a complete token. A glued '-1' or '$name-tail'
    // can be generic text where the same spelling starts an expression elsewhere.
    // After a prefix operator, recursive unary parsing also enables signed numbers.
    _command_argument_unary_expression: $ => choice(
      $._command_primary_expression,
      alias($._command_argument_unary_operator_expression, $.unary_expression),
      alias($._command_cast_expression, $.cast_expression),
      $.attributed_expression,
    ),

    _command_argument_unary_operator_expression: $ => unaryExpression(
      $, $._command_unary_expression, false, {
        argumentStart: $._command_unary_start,
        assignable: $._command_assignable_expression,
        prefixStart: $._command_unary_start,
        prefixAssignable: $._command_prefix_assignable_expression,
      },
    ),

    _command_unary_expression: $ => choice(
      $._command_primary_expression,
      alias($._command_unary_operator_expression, $.unary_expression),
      alias($._command_cast_expression, $.cast_expression),
      $.attributed_expression,
    ),

    _command_unary_operator_expression: $ => unaryExpression(
      $, $._command_unary_expression, true, {
        argumentStart: $._command_unary_start,
        assignable: $._command_assignable_expression,
        prefixStart: $._command_unary_start,
        prefixAssignable: $._command_prefix_assignable_expression,
      },
    ),

    _argument_command_unary_expression: $ => choice(
      $._comma_free_command_primary_expression,
      alias($._argument_command_unary_operator_expression, $.unary_expression),
      alias($._argument_command_cast_expression, $.cast_expression),
      $.attributed_expression,
    ),

    _argument_command_unary_operator_expression: $ => unaryExpression(
      $, $._argument_command_unary_expression, false, {
        argumentStart: $._command_unary_start,
        assignable: $._comma_free_command_assignable_expression,
        prefixStart: $._command_unary_start,
        prefixAssignable: $._comma_free_command_prefix_assignable_expression,
      },
    ),

    _command_cast_expression: $ => prec(
      PREC.unary,
      seq(
        field('type', $.type_literal),
        optional($._cast_value_context),
        field('value', $._command_cast_value),
      ),
    ),

    _command_cast_value: $ => choice(
      $._command_primary_expression,
      alias($._command_cast_expression, $.cast_expression),
      $.attributed_expression,
      alias($._command_argument_unary_operator_expression, $.unary_expression),
    ),

    _argument_command_cast_expression: $ => prec(
      PREC.unary,
      seq(
        field('type', $.type_literal),
        $._command_unary_start,
        optional($._cast_value_context),
        field('value', $._argument_command_unary_expression),
      ),
    ),

    _assignable_expression: $ => choice(
      $.variable,
      $.member_access,
      $.element_access,
      $.invocation_expression,
      $.parenthesized_expression,
    ),

    _argument_assignable_expression: $ => choice(
      $.variable,
      alias($._argument_member_access, $.member_access),
      alias($._argument_element_access, $.element_access),
      alias($._argument_invocation_expression, $.invocation_expression),
      $.parenthesized_expression,
    ),

    _argument_prefix_assignable_expression: $ => choice(
      $._argument_assignable_expression,
      $.attributed_expression,
      alias($._attributed_variable_cast, $.cast_expression),
    ),

    _command_assignable_expression: $ => choice(
      $.variable,
      alias($._command_member_access, $.member_access),
      alias($._command_element_access, $.element_access),
      alias($._command_invocation_expression, $.invocation_expression),
      $.parenthesized_expression,
    ),

    _command_prefix_assignable_expression: $ => choice(
      $._command_assignable_expression,
      $.attributed_expression,
      alias($._attributed_variable_cast, $.cast_expression),
    ),

    _prefix_assignable_expression: $ => choice(
      $._assignable_expression,
      $.attributed_expression,
      alias($._attributed_variable_cast, $.cast_expression),
    ),

    cast_expression: $ => prec(
      PREC.unary,
      seq(
        field('type', $.type_literal),
        optional($._cast_value_context),
        field('value', $._cast_value),
      ),
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

    _value_primary_expression: $ => valuePrimaryExpression(
      $, $.member_access, $.element_access, $.invocation_expression,
    ),

    _argument_primary_expression: $ => choice(
      $._argument_value_primary_expression,
      $.type_literal,
    ),

    _argument_value_primary_expression: $ => valuePrimaryExpression(
      $,
      alias($._argument_member_access, $.member_access),
      alias($._argument_element_access, $.element_access),
      alias($._argument_invocation_expression, $.invocation_expression),
    ),

    _command_primary_expression: $ => choice(
      $._command_value_primary_expression,
      $.type_literal,
    ),

    _command_value_primary_expression: $ => valuePrimaryExpression(
      $,
      alias($._command_member_access, $.member_access),
      alias($._command_element_access, $.element_access),
      alias($._command_invocation_expression, $.invocation_expression), true,
    ),

    parenthesized_expression: $ => seq(
      '(',
      repeat(NEWLINE),
      $._pipeline_entry,
      repeat(NEWLINE),
      ')',
    ),

    sub_expression: $ => seq(
      '$(',
      repeat($._terminator),
      optional($._statement_list),
      alias($._sub_expression_end, ')'),
    ),

    interpolation: $ => seq(
      field('expression', $._interpolation_primary_expression),
      $._interpolation_tail,
    ),

    _collected_sub_expression: $ => seq(
      alias($._interpolation_open, '$('),
      repeat($._terminator),
      optional($._statement_list),
      alias($._sub_expression_end, ')'),
    ),

    _interpolation_primary_expression: $ => prec(PREC.member, choice(
      alias($._collected_sub_expression, $.sub_expression),
      alias($._interpolation_member_access, $.member_access),
      alias($._interpolation_element_access, $.element_access),
      alias($._interpolation_invocation_expression, $.invocation_expression),
    )),

    _interpolation_member_access: $ => prec.left(PREC.member, seq(
      field('object', $._interpolation_primary_expression),
      optional($._postfix_context),
      field('operator', choice(
        token.immediate(prec(5, '.')),
        token.immediate(prec(5, '::')),
        token.immediate(prec(5, '?.')),
      )),
      repeat(NEWLINE),
      field('member', $._member_name),
    )),

    _interpolation_element_access: $ => elementAccess($, $._interpolation_primary_expression),

    _interpolation_invocation_expression: $ => invocationExpression(
      $, alias($._interpolation_member_access, $.member_access),
    ),

    _command_interpolation: $ => seq(
      field('expression', $._command_interpolation_primary_expression),
      $._interpolation_tail,
    ),

    _command_interpolation_primary_expression: $ => prec(PREC.member, choice(
      alias($._collected_sub_expression, $.sub_expression),
      alias($._command_interpolation_member_access, $.member_access),
      alias($._command_interpolation_element_access, $.element_access),
      alias($._command_interpolation_invocation_expression, $.invocation_expression),
    )),

    _command_interpolation_member_access: $ => prec.left(PREC.member, seq(
      field('object', $._command_interpolation_primary_expression),
      optional($._postfix_context),
      optional($._command_interpolation_context),
      field('operator', choice(
        token.immediate(prec(5, '.')),
        token.immediate(prec(5, '::')),
        token.immediate(prec(5, '?.')),
      )),
      repeat(NEWLINE),
      field('member', $._command_member_name),
    )),

    _command_interpolation_element_access: $ => elementAccess($, $._command_interpolation_primary_expression),

    _command_interpolation_invocation_expression: $ => invocationExpression(
      $, alias($._command_interpolation_member_access, $.member_access), false,
    ),

    array_expression: $ => seq('@(', repeat($._terminator), optional($._statement_list), ')'),

    hashtable_expression: $ => seq(
      '@{',
      repeat(NEWLINE),
      repeat(seq($.hash_entry, repeat1($._terminator))),
      optional($.hash_entry),
      '}',
    ),

    hash_entry: $ => seq(
      field('key', choice($.identifier, $._argument_unary_expression)),
      '=',
      repeat(NEWLINE),
      field('value', $._statement),
    ),

    member_access: $ => memberAccess($, $._primary_expression, $._member_name),

    _argument_member_access: $ => memberAccess(
      $, $._argument_primary_expression, $._argument_member_name,
    ),

    _command_member_access: $ => memberAccess(
      $, $._command_primary_expression, $._command_member_name, $._command_member_start,
    ),

    // Simple member values stop before the next postfix operator. Prefix
    // operators and casts instead consume a complete recursive unary operand.
    _member_name: $ => seq(
      $._member_expression_start,
      choice(
        $.identifier,
        primaryValue($),
        $.type_literal,
        alias($._member_name_unary_expression, $.unary_expression),
        $.cast_expression,
        $.attributed_expression,
      ),
    ),

    _member_name_unary_expression: $ => unaryExpression(
      $, $._unary_expression, true, {postfix: false},
    ),

    _argument_member_name: $ => seq(
      $._member_expression_start,
      choice(
        $.identifier,
        primaryValue($),
        $.type_literal,
        alias($._argument_member_name_unary_expression, $.unary_expression),
        alias($._argument_cast_expression, $.cast_expression),
        $.attributed_expression,
      ),
    ),

    _argument_member_name_unary_expression: $ => unaryExpression(
      $, $._argument_unary_expression, false, {
        postfix: false,
        prefixAssignable: $._argument_prefix_assignable_expression,
      },
    ),

    _command_member_name: $ => choice(
      $.identifier,
      primaryValue($, true, true),
      $.type_literal,
      alias($._command_member_name_unary_expression, $.unary_expression),
      alias($._command_cast_expression, $.cast_expression),
      $.attributed_expression,
    ),

    _command_member_name_unary_expression: $ => unaryExpression(
      $, $._command_unary_expression, true, {
        postfix: false,
        argumentStart: $._command_unary_start,
        prefixStart: $._command_unary_start,
        prefixAssignable: $._command_prefix_assignable_expression,
      },
    ),

    _argument_command_member_name_unary_expression: $ => unaryExpression(
      $, $._argument_command_unary_expression, false, {
        postfix: false,
        argumentStart: $._command_unary_start,
        prefixStart: $._command_unary_start,
        prefixAssignable: $._comma_free_command_prefix_assignable_expression,
      },
    ),

    element_access: $ => elementAccess($, $._value_primary_expression),

    _argument_element_access: $ => elementAccess($, $._argument_value_primary_expression),

    _command_element_access: $ => elementAccess($, $._command_value_primary_expression),

    invocation_expression: $ => invocationExpression($, $.member_access),

    _argument_invocation_expression: $ => invocationExpression(
      $, alias($._argument_member_access, $.member_access),
    ),

    _command_invocation_expression: $ => invocationExpression(
      $, alias($._command_member_access, $.member_access), false,
    ),

    argument_list: $ => argumentList(token.immediate('('), $._argument_expression_head),

    // Generic lists allow newlines around separators. Bracketed generic arguments
    // forbid a newline immediately after their opener; assembly names stay inline.
    generic_type_arguments: $ => seq(
      token.immediate(prec(5, '[')),
      repeat(NEWLINE),
      $.type_name,
      repeat(NEWLINE),
      repeat(seq(',', repeat(NEWLINE), $.type_name, repeat(NEWLINE))),
      ']',
    ),

    type_literal: $ => seq('[', repeat(NEWLINE), $.type_name, optional($.assembly_name), ']'),

    _generic_type_literal: $ => seq('[', $.type_name, optional($.assembly_name), ']'),

    assembly_name: $ => seq(',', $._assembly_name_content),

    type_name: $ => seq(
      field('name', $.type_identifier),
      optional(
        seq(
          '[',
          repeat(NEWLINE),
          choice($.type_name, alias($._generic_type_literal, $.type_literal)),
          repeat(NEWLINE),
          repeat(
            seq(
              ',',
              repeat(NEWLINE),
              choice($.type_name, alias($._generic_type_literal, $.type_literal)),
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
            $.interpolation,
          ),
        ),
        $._here_string_end,
      ),
    ),

    _command_expandable_here_string: $ => choice(
      $._empty_expandable_here_string,
      seq(
        $._expandable_here_string_start,
        repeat(
          choice(
            alias($._here_string_content, $.string_content),
            alias($._literal_dollar, $.string_content),
            alias($._string_escape, $.escape_sequence),
            $.variable,
            alias($._command_interpolation, $.interpolation),
          ),
        ),
        $._here_string_end,
      ),
    ),

    // Immediate closing delimiters keep trailing body whitespace as content.
    expandable_string: $ => seq(
      alias($._double_quote, '"'),
      repeat(
        choice(
          $.string_content,
          alias($._literal_dollar, $.string_content),
          alias($._string_escape, $.escape_sequence),
          $.variable,
          $.interpolation,
        ),
      ),
      alias($._double_quote, '"'),
    ),

  },
};

// Hashtable statements inherit PowerShell's comma-operator setting. Keep one
// rule builder for each construct and reuse it in the comma-suppressed context.
const COMMA_CONTEXT_RULES = new Set([
  '_script_block_body', '_statement_list', '_terminated_statement',
  '_statement', '_block_statement', '_simple_statement', 'statement_block',
  'if_statement', 'elseif_clause', 'else_clause', 'condition',
  'switch_statement', '_switch_file_parameter', 'switch_clause',
  'foreach_statement', 'foreach_parameter', 'for_statement',
  '_for_header_initializer', '_for_header_condition',
  'while_statement', 'do_statement',
  'try_statement', 'catch_clause', 'finally_clause', 'trap_statement',
  'return_statement', 'throw_statement', 'exit_statement',
  '_completed_return_statement', '_completed_throw_statement', '_completed_exit_statement',
  'function_definition', 'script_block', 'named_block',
  'class_definition', 'class_body', 'class_property', 'class_method',
  'enum_definition', 'enum_body', 'enum_member', 'data_statement',
  '_data_supported_command', '_data_command_primary_expression',
  '_data_command_member_access', '_data_command_element_access',
  '_data_command_invocation_expression',
  'assignment_statement', '_completed_assignment_statement',
  '_pipeline_entry', '_completed_pipeline_entry', 'pipeline', 'pipeline_chain',
  'background_pipeline', 'command_expression', 'command',
  'invocation_operator_command', 'command_parameter', '_command_parameter_value',
  '_command_parameter_array', '_single_command_argument_value',
  '_command_element_after_end_of_parameters', '_invocation_end_of_parameters_command',
  '_invocation_verbatim_command',
  'command_argument', '_command_argument_array', '_command_argument_value',
  'redirection', 'hashtable_expression', 'hash_entry',
  '_command_argument_unary_expression', '_command_argument_unary_operator_expression',
  '_command_primary_expression', '_command_value_primary_expression',
  '_command_assignable_expression', '_command_prefix_assignable_expression',
  '_command_member_access', '_command_member_name',
  '_command_element_access', '_command_invocation_expression',
]);

const COMMA_CONTEXT_EXPRESSIONS = {
  _expression: '_argument_expression',
  _unary_expression: '_argument_unary_expression',
  _primary_expression: '_argument_primary_expression',
  _value_primary_expression: '_argument_value_primary_expression',
  _assignable_expression: '_argument_assignable_expression',
  _prefix_assignable_expression: '_argument_prefix_assignable_expression',
  _cast_value: '_argument_unary_expression',
  _member_name: '_argument_member_name',
  member_access: '_argument_member_access',
  element_access: '_argument_element_access',
  invocation_expression: '_argument_invocation_expression',
  cast_expression: '_argument_cast_expression',
  unary_expression: '_argument_unary_operator_expression',
  _command_unary_expression: '_argument_command_unary_expression',
  _command_unary_operator_expression: '_argument_command_unary_operator_expression',
  _command_cast_expression: '_argument_command_cast_expression',
  _command_cast_value: '_argument_command_unary_expression',
  _command_member_name_unary_expression: '_argument_command_member_name_unary_expression',
};

const commaContextName = (name, owner) => {
  // Script block expressions reset comma suppression; a function declaration's
  // body inherits it. Parentheses, subexpressions, arrays, and indexers already
  // use their ordinary builders at their respective reset boundaries.
  if (name === 'script_block' && owner !== 'function_definition') {
    return name;
  }
  if (name === '_expression' &&
      (owner === '_command_element_access' || owner === '_data_command_element_access')) {
    return name;
  }
  return COMMA_CONTEXT_EXPRESSIONS[name] ||
    (COMMA_CONTEXT_RULES.has(name) ? `_comma_free_${name.replace(/^_/, '')}` : name);
};

const commaContext = ($, rule, owner) => {
  if (rule.type === 'SYMBOL') {
    const name = commaContextName(rule.name, owner);
    if (name === rule.name) {
      return rule;
    }
    return rule.name.startsWith('_') ? $[name] : alias($[name], $[rule.name]);
  }
  // Preserve alias names and field names while replacing their rule contents.
  if (rule.content) {
    return {...rule, content: commaContext($, rule.content, owner)};
  }
  if (rule.members) {
    return {...rule, members: rule.members.map(member => commaContext($, member, owner))};
  }
  return rule;
};

for (const name of COMMA_CONTEXT_RULES) {
  const builder = grammarDefinition.rules[name];
  grammarDefinition.rules[`_comma_free_${name.replace(/^_/, '')}`] = $ =>
    commaContext($, builder($), name);
}

// Method/default/attribute expressions and member names are the entry points
// that can introduce a hashtable while comma suppression is already enabled.
for (const name of ['_argument_value_primary_expression', '_argument_member_name']) {
  const builder = grammarDefinition.rules[name];
  grammarDefinition.rules[name] = $ => commaContext($, builder($), name);
}

const ordinaryConflicts = grammarDefinition.conflicts;
grammarDefinition.conflicts = $ => {
  const conflicts = ordinaryConflicts($);
  const contextual = conflicts.map(conflict => conflict.map(rule =>
    $[commaContextName(rule.name)],
  ));
  const seen = new Set();
  return [...conflicts, ...contextual].filter(conflict => {
    const key = conflict.map(rule => rule.name).sort().join(',');
    if (seen.has(key)) {
      return false;
    }
    seen.add(key);
    return true;
  });
};

// PowerShell's collector counts parentheses inside lexical leaves. Keep
// punctuation external so every parenthesis updates the scanner balance.
const PARENTHESIS_TOKENS = {
  '(': '_left_paren',
  ')': '_right_paren',
  '$(': '_sub_expression_open',
  '@(': '_array_expression_open',
};

const ADJACENT_PARENTHESIS_TOKENS = {
  '(': '_adjacent_left_paren',
  '$(': '_adjacent_sub_expression_open',
};

const parenthesisTokens = ($, rule) => {
  if (rule.type === 'IMMEDIATE_TOKEN' && rule.content.type === 'STRING') {
    const name = ADJACENT_PARENTHESIS_TOKENS[rule.content.value];
    if (name) {
      return alias($[name], rule.content.value);
    }
  }
  if (rule.type === 'STRING' && PARENTHESIS_TOKENS[rule.value]) {
    return alias($[PARENTHESIS_TOKENS[rule.value]], rule.value);
  }
  if (rule.content) {
    return {...rule, content: parenthesisTokens($, rule.content)};
  }
  if (rule.members) {
    return {
      ...rule,
      members: rule.members.map(member => parenthesisTokens($, member)),
    };
  }
  return rule;
};

for (const [name, builder] of Object.entries(grammarDefinition.rules)) {
  grammarDefinition.rules[name] = $ => parenthesisTokens($, builder($));
}

module.exports = grammar(grammarDefinition);
