#include "tree_sitter/parser.h"

#include <stdbool.h>
#include <stdint.h>

enum TokenType {
  BLOCK_COMMENT,
  DOC_BLOCK_COMMENT,
  UNTERMINATED_BLOCK_COMMENT,
  STRING_CONTENT,
  FORMAT_TEXT,
  STRING_END,
  UNTERMINATED_STRING_END,
  SCANNER_RECOVERY,
};

void *tree_sitter_kagari_external_scanner_create(void) { return NULL; }

void tree_sitter_kagari_external_scanner_destroy(void *payload) { (void)payload; }

unsigned tree_sitter_kagari_external_scanner_serialize(void *payload, char *buffer) {
  (void)payload;
  (void)buffer;
  return 0;
}

void tree_sitter_kagari_external_scanner_deserialize(void *payload, const char *buffer, unsigned length) {
  (void)payload;
  (void)buffer;
  (void)length;
}

static void advance(TSLexer *lexer) { lexer->advance(lexer, false); }

// Match Rust char::is_whitespace in the source lexer and the grammar's extras.
static bool whitespace(int32_t c) {
  return (c >= 0x09 && c <= 0x0d) || c == 0x20 || c == 0x85 || c == 0xa0 ||
         c == 0x1680 || (c >= 0x2000 && c <= 0x200a) || c == 0x2028 ||
         c == 0x2029 || c == 0x202f || c == 0x205f || c == 0x3000;
}

static bool comment(TSLexer *lexer) {
  if (lexer->lookahead != '/') return false;
  advance(lexer);
  if (lexer->lookahead != '*') return false;
  advance(lexer);
  bool doc = lexer->lookahead == '!' || lexer->lookahead == '*';
  // /**/ is an ordinary empty block comment, not a documentation opener.
  if (lexer->lookahead == '*') {
    advance(lexer);
    if (lexer->lookahead == '/') {
      advance(lexer);
      lexer->result_symbol = BLOCK_COMMENT;
      return true;
    }
  }
  uint32_t depth = 1;
  while (!lexer->eof(lexer)) {
    if (lexer->lookahead == '/') {
      advance(lexer);
      if (lexer->lookahead == '*') { advance(lexer); depth++; }
    } else if (lexer->lookahead == '*') {
      advance(lexer);
      if (lexer->lookahead == '/') {
        advance(lexer);
        if (--depth == 0) {
          lexer->result_symbol = doc ? DOC_BLOCK_COMMENT : BLOCK_COMMENT;
          return true;
        }
      }
    } else {
      advance(lexer);
    }
  }
  lexer->result_symbol = UNTERMINATED_BLOCK_COMMENT;
  return true;
}

bool tree_sitter_kagari_external_scanner_scan(void *payload, TSLexer *lexer, const bool *valid) {
  (void)payload;
  // Never invent a string context when Tree-sitter enables every external token
  // during error recovery. The parse stack, not mutable scanner state, owns it.
  if (valid[SCANNER_RECOVERY]) return false;
  bool string = valid[STRING_CONTENT] || valid[FORMAT_TEXT];
  if (string) {
    if (lexer->lookahead == '"' && valid[STRING_END]) {
      advance(lexer);
      lexer->result_symbol = STRING_END;
      return true;
    }
    if ((lexer->lookahead == '\r' || lexer->lookahead == '\n' || lexer->eof(lexer)) &&
        valid[UNTERMINATED_STRING_END]) {
      // A zero-width, explicitly invalid end restores code lexing without eating
      // the newline. It cannot recur: the grammar exits this string immediately.
      lexer->result_symbol = UNTERMINATED_STRING_END;
      return true;
    }
    bool consumed = false;
    while (!lexer->eof(lexer) && lexer->lookahead != '"' && lexer->lookahead != '\\' &&
           lexer->lookahead != '\r' && lexer->lookahead != '\n') {
      if (valid[FORMAT_TEXT] && (lexer->lookahead == '{' || lexer->lookahead == '}')) {
        int32_t brace = lexer->lookahead;
        lexer->mark_end(lexer);
        advance(lexer);
        if (lexer->lookahead != brace) {
          if (!consumed) return false;
          lexer->result_symbol = FORMAT_TEXT;
          return true;
        }
        advance(lexer);
      } else {
        advance(lexer);
      }
      consumed = true;
      lexer->mark_end(lexer);
    }
    if (consumed) {
      lexer->result_symbol = valid[FORMAT_TEXT] ? FORMAT_TEXT : STRING_CONTENT;
      return true;
    }
    return false;
  }
  while (whitespace(lexer->lookahead)) {
    lexer->advance(lexer, true);
  }
  if (valid[BLOCK_COMMENT] || valid[DOC_BLOCK_COMMENT] || valid[UNTERMINATED_BLOCK_COMMENT]) {
    return comment(lexer);
  }
  return false;
}
