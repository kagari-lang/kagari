; General names precede structural overrides. No symbol-resolution guesses.
(identifier) @variable
(type_identifier) @type
(wildcard_pattern) @variable

(function_item name: (identifier) @function)
(method_item name: (identifier) @function)
(struct_item name: (type_identifier) @type)
(enum_item name: (type_identifier) @enum)
(trait_item name: (type_identifier) @type)
(module_item name: (identifier) @label)
(const_item name: (identifier) @constant)
(associated_const name: (identifier) @constant)
(enum_variant name: (identifier) @variant)
(parameter name: (identifier) @variable.parameter)
(closure_parameter name: (identifier) @variable.parameter)
(field_declaration name: (identifier) @property)
(field_expression field: (identifier) @property)
(field_initializer name: (identifier) @property)
(field_pattern name: (identifier) @property)
(struct_pattern type: (path (identifier) @type .))
(struct_pattern type: (generic_path name: (identifier) @type))
(attribute name: (path (identifier) @attribute))
(attribute_argument name: (identifier) @property)

["as" "async" "await" "break" "const" "continue" "crate" "else"
 "enum" "fn" "for" "trait" "if" "impl" "in" "loop" "match" "mod"
 "pub" "return" "self" "static" "struct" "super" "use" "val" "var" "type"
 "where" "while"] @keyword

(boolean_literal) @boolean
[(integer_literal) (float_literal)] @number
[(line_comment) (block_comment) (unterminated_block_comment)] @comment
[(doc_line_comment) (doc_block_comment)] @comment.doc

; Do not capture the interpolated_string parent: expression holes remain code.
[(string_content) (format_text)] @string
(escape_sequence) @string.escape
(string_literal "\"" @string)
(interpolated_string "f\"" @string)
(interpolated_string "\"" @string)


["=" "+" "-" "*" "/" "%" "!" "==" "!=" "===" "!=="
 "<" "<=" ">" ">=" "&&" "||" "&" "|" "^" "<<" ">>"
 "+=" "-=" "*=" "/=" "%=" "&=" "|=" "^=" "<<=" ">>="
 "?" ".." "..="] @operator
["(" ")" "[" "]" "{" "}"] @punctuation.bracket
["," ";" ":" "." "::"] @punctuation.delimiter
["->" "=>" "#"] @punctuation.special
(generic_arguments ["<" ">"] @punctuation.bracket)
(generic_parameters ["<" ">"] @punctuation.bracket)
(interpolation ["{" "}"] @punctuation.special)
(interpolation_format [":" "?"] @punctuation.special)
