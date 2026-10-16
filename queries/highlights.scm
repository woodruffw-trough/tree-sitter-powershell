(comment) @comment
(variable) @variable
(splat_expression) @variable
(integer_literal) @number
(real_literal) @number
(single_quoted_string) @string
(expandable_string) @string
(single_quoted_here_string) @string
(expandable_here_string) @string
(string_content) @string
(escape_sequence) @string.escape
(type_identifier) @type
(attribute name: (type_name) @attribute)
(function_definition name: (function_name) @function)
(command_name) @function.call
(command_parameter name: _ @variable.parameter)
"--%" @keyword
(parameter name: [(variable) (splat_expression)] @variable.parameter)
(class_definition name: (identifier) @type.definition)
(enum_definition name: (identifier) @type.definition)
(enum_member name: (identifier) @constant)
(member_access member: (identifier) @property)
(class_property name: (variable) @property)
(class_method name: (identifier) @function.method)
(invocation_expression
  function: (member_access member: (identifier) @function.method.call))
["(" ")" "[" "]" "{" "}" "@{" "@(" "$(" "?["] @punctuation.bracket
["," ";"] @punctuation.delimiter
(binary_expression operator: _ @operator)
(assignment_statement operator: _ @operator)
(redirection operator: _ @operator)
(unary_expression operator: _ @operator)
(member_access operator: _ @operator)
(invocation_operator_command operator: _ @operator)
["|" "&&" "||" "&"] @operator
(ternary_expression ["?" ":"] @operator)
(function_definition keyword: _ @keyword.function)
(return_statement keyword: _ @keyword.return)
(exit_statement keyword: _ @keyword.return)
(break_statement keyword: _ @keyword.repeat)
(continue_statement keyword: _ @keyword.repeat)
(named_block name: _ @keyword)
["if" "elseif" "else" "switch" "default"] @keyword.conditional
["foreach" "for" "while" "do" "until" "in"] @keyword.repeat
["try" "catch" "finally" "trap" "throw"] @keyword.exception
["class" "enum" "hidden" "static" "base" "param" "data" "configuration" "node" "Import-DscResource" "parallel" "sequence" "using" "namespace" "module" "assembly" "type" "command"] @keyword
