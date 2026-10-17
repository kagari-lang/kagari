; Zed chooses the smallest matching override range, with exclusive endpoints.
; Whole strings cover empty text and positions adjacent to delimiters; smaller
; interpolation ranges restore code, then nested strings/comments override again.
[(string_literal) (interpolated_string)] @string
(interpolation) @interpolation
[(line_comment) (doc_line_comment) (block_comment) (doc_block_comment)
 (unterminated_block_comment)] @comment.inclusive
