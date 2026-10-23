; Zed chooses the smallest matching override range, with exclusive endpoints.
; Whole strings cover empty text and positions adjacent to delimiters; smaller
; identifier interpolation ranges restore code. Raw tokens have no inner regions.
[(string_literal) (interpolated_string) (raw_string_literal)
 (unterminated_raw_string) (char_literal) (byte_char_literal) (byte_string_literal)
 (raw_byte_string_literal) (unterminated_raw_byte_string)] @string
(interpolation) @interpolation
[(line_comment) (doc_line_comment) (block_comment) (doc_block_comment)
 (unterminated_block_comment)] @comment.inclusive
