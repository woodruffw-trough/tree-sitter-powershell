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
(attribute (type_name) @attribute)
(function_definition name: (function_name) @function)
(command_name) @function.call
(command_parameter name: _ @variable.parameter)
(class_definition name: (identifier) @type.definition)
(enum_definition name: (identifier) @type.definition)
(member_access member: (identifier) @property)
(class_property (variable) @property)
["(" ")" "[" "]" "{" "}"] @punctuation.bracket
["," ";"] @punctuation.delimiter
(binary_expression operator: _ @operator)
(assignment_statement operator: _ @operator)
(redirection operator: _ @operator)
(unary_expression operator: _ @operator)
(ternary_expression ["?" ":"] @operator)
(function_definition keyword: _ @keyword.function)
(flow_control_statement keyword: _ @keyword.return)
(named_block name: _ @keyword)
["if" "elseif" "else" "switch" "default"] @keyword.conditional
["foreach" "for" "while" "do" "until" "in"] @keyword.repeat
["try" "catch" "finally" "trap" "throw"] @keyword.exception
["class" "enum" "hidden" "static" "param" "data" "configuration" "using" "namespace" "module" "assembly"] @keyword
