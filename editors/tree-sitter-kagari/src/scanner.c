#include "tree_sitter/parser.h"

#include <stdbool.h>
#include <stdint.h>
#include <string.h>

enum TokenType {
  BLOCK_COMMENT,
  DOC_BLOCK_COMMENT,
  UNTERMINATED_BLOCK_COMMENT,
  STRING_CONTENT,

  STRING_END,
  UNTERMINATED_STRING_END,
  SCANNER_RECOVERY,
  CHAR_BOUNDARY,
  RAW_STRING_LITERAL,
  UNTERMINATED_RAW_STRING,
  HOLE_BOUNDARY,
  HOLE_NAME,
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

static bool identifier_start(int32_t c) {
  return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || c == '_';
}

// Immediate internal tokens bypass Tree-sitter's keyword extraction. Recognize
// this name here so both adjacency and the normal reserved-word policy hold.
static bool hole_name(TSLexer *lexer) {
  if (!identifier_start(lexer->lookahead)) return false;
  char prefix[9] = {0};
  unsigned length = 0;
  while (identifier_start(lexer->lookahead) ||
         (lexer->lookahead >= '0' && lexer->lookahead <= '9')) {
    if (length < sizeof(prefix) - 1) prefix[length++] = (char)lexer->lookahead;
    else length = sizeof(prefix);
    advance(lexer);
  }
  static const char *keywords[] = {
    "as", "async", "await", "break", "const", "continue", "crate", "else",
    "enum", "false", "fn", "for", "trait", "if", "impl", "in", "loop",
    "match", "mod", "pub", "return", "self", "static", "struct", "super",
    "true", "use", "val", "var", "type", "where", "while",
  };
  if (length < sizeof(prefix)) {
    for (unsigned i = 0; i < sizeof(keywords) / sizeof(keywords[0]); i++) {
      if (strcmp(prefix, keywords[i]) == 0) return false;
    }
  }
  lexer->result_symbol = HOLE_NAME;
  return true;
}

// Scan one whole raw token: delimiter state never survives an incremental edit.
static bool raw_string(TSLexer *lexer) {
  if (lexer->lookahead != 'r') return false;
  advance(lexer);
  unsigned hashes = 0;
  while (lexer->lookahead == '#') {
    if (++hashes > 255) return false;
    advance(lexer);
  }
  if (lexer->lookahead != '"') return false;
  advance(lexer);
  while (!lexer->eof(lexer)) {
    if (lexer->lookahead == '"') {
      advance(lexer);
      unsigned matched = 0;
      while (matched < hashes && lexer->lookahead == '#') {
        advance(lexer);
        matched++;
      }
      if (matched == hashes) {
        lexer->result_symbol = RAW_STRING_LITERAL;
        return true;
      }
    } else if (lexer->lookahead == '\r') {
      advance(lexer);
      if (lexer->lookahead != '\n') return false;
      advance(lexer);
    } else {
      advance(lexer);
    }
  }
  lexer->result_symbol = UNTERMINATED_RAW_STRING;
  return true;
}

bool tree_sitter_kagari_external_scanner_scan(void *payload, TSLexer *lexer, const bool *valid) {
  (void)payload;
  // Never invent a string context when Tree-sitter enables every external token
  // during error recovery. The parse stack, not mutable scanner state, owns it.
  if (valid[SCANNER_RECOVERY] || valid[CHAR_BOUNDARY]) return false;
  if (valid[HOLE_NAME]) return hole_name(lexer);
  if (valid[HOLE_BOUNDARY]) return false;
  bool string = valid[STRING_CONTENT];
  if (string) {
    if (lexer->lookahead == '"' && valid[STRING_END]) {
      advance(lexer);
      lexer->result_symbol = STRING_END;
      return true;
    }
    if (lexer->eof(lexer) &&
        valid[UNTERMINATED_STRING_END]) {
      // Only EOF proves a multiline string is unterminated.
      lexer->result_symbol = UNTERMINATED_STRING_END;
      return true;
    }
    bool consumed = false;
    while (!lexer->eof(lexer) && lexer->lookahead != '"' && lexer->lookahead != '\\') {
      if (lexer->lookahead == '{' || lexer->lookahead == '}') {
        int32_t brace = lexer->lookahead;
        lexer->mark_end(lexer);
        advance(lexer);
        if (lexer->lookahead != brace) {
          if (!consumed) return false;
          lexer->result_symbol = STRING_CONTENT;
          return true;
        }
        advance(lexer);
      } else if (lexer->lookahead == '\r') {
        lexer->mark_end(lexer);
        advance(lexer);
        if (lexer->lookahead != '\n') {
          if (!consumed) return false;
          lexer->result_symbol = STRING_CONTENT;
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
      lexer->result_symbol = STRING_CONTENT;
      return true;
    }
    return false;
  }
  while (whitespace(lexer->lookahead)) {
    lexer->advance(lexer, true);
  }
  if (valid[RAW_STRING_LITERAL] && lexer->lookahead == 'r') return raw_string(lexer);
  if (valid[BLOCK_COMMENT] || valid[DOC_BLOCK_COMMENT] || valid[UNTERMINATED_BLOCK_COMMENT]) {
    return comment(lexer);
  }
  return false;
}
