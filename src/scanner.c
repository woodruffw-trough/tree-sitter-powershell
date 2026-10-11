#include "tree_sitter/parser.h"
#include "variable_chars.h"
#include <stdbool.h>
#include <stdlib.h>
#include <ctype.h>
#include <string.h>

enum TokenType { SINGLE_QUOTED_HERE_STRING, EXPANDABLE_HERE_STRING_START, COMMAND_NAME, BARE_WORD, INTEGER_LITERAL, REAL_LITERAL, HERE_STRING_CONTENT, HERE_STRING_END, EMPTY_EXPANDABLE_HERE_STRING, STRING_ESCAPE, VARIABLE, ADJACENT_VARIABLE, LITERAL_DOLLAR, ARGUMENT_END };

void *tree_sitter_powershell_external_scanner_create(void) { return NULL; }
void tree_sitter_powershell_external_scanner_destroy(void *payload) { (void)payload; }
unsigned tree_sitter_powershell_external_scanner_serialize(void *payload, char *buffer) {
  (void)payload; (void)buffer; return 0;
}
void tree_sitter_powershell_external_scanner_deserialize(void *payload, const char *buffer, unsigned length) {
  (void)payload; (void)buffer; (void)length;
}

static bool horizontal(int32_t c) { return c == ' ' || c == '\t' || c == '\f' || c == '\v' || c == 0xfeff; }
static bool ascii_digit(int32_t c) { return c >= '0' && c <= '9'; }
static int32_t ascii_lower(int32_t c) { return c >= 'A' && c <= 'Z' ? c + 32 : c; }
static bool ascii_hex(int32_t c) { c = ascii_lower(c); return ascii_digit(c) || (c >= 'a' && c <= 'f'); }
static bool quote(int32_t c) { return c == '\'' || c == '"' || (c >= 0x2018 && c <= 0x201f); }
static bool generic_end(int32_t c) {
  return c == 0 || horizontal(c) || c == '\r' || c == '\n' || c == ';' || c == '|' || c == '&' || c == '(' || c == ')' || c == '{' || c == '}' || c == ',' || c == '<' || c == '>' || quote(c);
}
static bool suffix_ok(const char *s) {
  if (*s == 'u') { ++s; if (*s == 'l' || *s == 's' || *s == 'y') ++s; }
  else if (*s == 'l' || *s == 's' || *s == 'y' || *s == 'n' || *s == 'd') ++s;
  if ((*s == 'k' || *s == 'm' || *s == 'g' || *s == 't' || *s == 'p') && s[1] == 'b') s += 2;
  return *s == 0;
}
static bool numeric_token(const char *s, enum TokenType *kind) {
  if (*s == '+' || *s == '-') ++s;
  const char *end = s;
  bool real = false;
  if (s[0] == '0' && s[1] == 'x') {
    end += 2;
    // PowerShell accepts the empty hexadecimal prefix as zero, including its
    // numeric suffixes. C's strtod would also admit unsupported hex floats.
    while (ascii_hex(*end)) ++end;
  } else if (s[0] == '0' && s[1] == 'b') {
    end += 2;
    if (*end != '0' && *end != '1') return false;
    while (*end == '0' || *end == '1') ++end;
  } else {
    bool digit = false;
    while (ascii_digit(*end)) { digit = true; ++end; }
    if (*end == '.') {
      real = true;
      ++end;
      while (ascii_digit(*end)) { digit = true; ++end; }
    }
    if (!digit) return false;
    if (*end == 'e') {
      real = true;
      ++end;
      if (*end == '+' || *end == '-') ++end;
      if (!ascii_digit(*end)) return false;
      while (ascii_digit(*end)) ++end;
    }
  }
  // Hexadecimal d/e are digits, not decimal suffixes or exponent markers.
  // Integral suffixes are also valid on decimal fractional/exponent forms.
  if (*end == 'd') real = true;
  if (!suffix_ok(end)) return false;
  *kind = real ? REAL_LITERAL : INTEGER_LITERAL;
  return true;
}
static bool numeric_word(const char *s) {
  enum TokenType kind;
  return numeric_token(s, &kind);
}
static bool reserved(const char *s) {
  static const char *const words[] = {"if", "elseif", "else", "switch", "foreach", "for", "while", "do", "until", "try", "catch", "finally", "trap", "function", "filter", "workflow", "class", "enum", "break", "continue", "return", "throw", "exit", "data", "using", "configuration", "param", "dynamicparam", "begin", "process", "end", "clean"};
  for (unsigned i = 0; i < sizeof(words)/sizeof(words[0]); ++i) if (strcmp(s, words[i]) == 0) return true;
  return false;
}
// PowerShell recognizes a family of interchangeable straight/smart delimiters.
static bool single_quote(int32_t c) {
  return c == '\'' || c == 0x2018 || c == 0x2019 || c == 0x201a || c == 0x201b;
}
static bool double_quote(int32_t c) {
  return c == '"' || c == 0x201c || c == 0x201d || c == 0x201e;
}
static bool here_header_space(int32_t c) {
  // Char.IsWhiteSpace, excluding PowerShell's two physical newline characters.
  return c == ' ' || c == '\t' || c == '\f' || c == '\v' || c == 0x85 ||
    c == 0xa0 || c == 0x1680 || (c >= 0x2000 && c <= 0x200a) ||
    c == 0x2028 || c == 0x2029 || c == 0x202f || c == 0x205f || c == 0x3000;
}
static bool newline(int32_t c) { return c == '\n' || c == '\r'; }
static void consume_newline(TSLexer *lexer) {
  bool carriage_return = lexer->lookahead == '\r';
  lexer->advance(lexer, false);
  if (carriage_return && lexer->lookahead == '\n') lexer->advance(lexer, false);
}

static bool scan_here_string(TSLexer *lexer, const bool *valid) {
  lexer->advance(lexer, false);
  bool single = single_quote(lexer->lookahead);
  if (!single && !double_quote(lexer->lookahead)) return false;
  if (!(single ? valid[SINGLE_QUOTED_HERE_STRING] : valid[EXPANDABLE_HERE_STRING_START])) return false;
  lexer->advance(lexer, false);
  while (here_header_space(lexer->lookahead)) lexer->advance(lexer, false);
  if (!newline(lexer->lookahead)) return false;
  consume_newline(lexer);
  lexer->mark_end(lexer);

  if (!single) {
    // An empty body's footer has no body newline of its own. This separate
    // token lets all nonempty footers include their preceding physical newline.
    if (double_quote(lexer->lookahead)) {
      lexer->advance(lexer, false);
      if (lexer->lookahead == '@' && valid[EMPTY_EXPANDABLE_HERE_STRING]) {
        lexer->advance(lexer, false);
        lexer->mark_end(lexer);
        lexer->result_symbol = EMPTY_EXPANDABLE_HERE_STRING;
        return true;
      }
    }
    lexer->result_symbol = EXPANDABLE_HERE_STRING_START;
    return true;
  }

  bool line_start = true;
  while (!lexer->eof(lexer)) {
    if (line_start && single_quote(lexer->lookahead)) {
      lexer->advance(lexer, false);
      if (lexer->lookahead == '@') {
        lexer->advance(lexer, false);
        lexer->mark_end(lexer);
        lexer->result_symbol = SINGLE_QUOTED_HERE_STRING;
        return true;
      }
      line_start = false;
    } else if (newline(lexer->lookahead)) {
      consume_newline(lexer);
      line_start = true;
    } else {
      line_start = false;
      lexer->advance(lexer, false);
    }
  }
  return false;
}

static bool variable_character(int32_t c) {
  if (c == '_' || c == '?') return true;
  if (c < 0 || c > 0xffff) return false;
  unsigned left = 0, right = sizeof(VARIABLE_CHARACTER_RANGES) / sizeof(VARIABLE_CHARACTER_RANGES[0]);
  while (left < right) {
    unsigned middle = left + (right - left) / 2;
    if (c < VARIABLE_CHARACTER_RANGES[middle][0]) right = middle;
    else if (c > VARIABLE_CHARACTER_RANGES[middle][1]) left = middle + 1;
    else return true;
  }
  return false;
}

static bool variable_start(int32_t c) {
  return variable_character(c) || c == '{' || c == '$' || c == '^' || c == ':';
}

static bool variable_escape(TSLexer *lexer, int32_t *decoded) {
  lexer->advance(lexer, false); // Backtick.
  if (lexer->eof(lexer)) return false;
  int32_t c = lexer->lookahead;
  lexer->advance(lexer, false);
  if (c != 'u') { *decoded = c; return true; }
  if (lexer->lookahead != '{') return false;
  lexer->advance(lexer, false);
  uint32_t value = 0;
  unsigned digits = 0;
  while (ascii_hex(lexer->lookahead)) {
    if (++digits > 6) return false;
    c = ascii_lower(lexer->lookahead);
    value = value * 16 + (uint32_t)(ascii_digit(c) ? c - '0' : c - 'a' + 10);
    lexer->advance(lexer, false);
  }
  if (digits == 0 || value > 0x10ffff || lexer->lookahead != '}') return false;
  lexer->advance(lexer, false);
  *decoded = (int32_t)value;
  return true;
}

// The caller has consumed '$'. Scope/drive qualification requires a name
// after the first colon, even inside braced and expandable-string variables.
static bool scan_variable_after_dollar(TSLexer *lexer, const bool *valid, enum TokenType kind) {
  if (!valid[kind]) return false;
  int32_t first = lexer->lookahead;
  if (first == '?' || first == '$' || first == '^') {
    lexer->advance(lexer, false);
  } else if (first == '{') {
    lexer->advance(lexer, false);
    unsigned length = 0, colon = 0;
    bool qualified = false;
    while (!lexer->eof(lexer) && lexer->lookahead != '}') {
      int32_t c = lexer->lookahead;
      if (c == '{') return false;
      if (c == '`') {
        if (!variable_escape(lexer, &c)) return false;
      } else lexer->advance(lexer, false);
      ++length;
      if (c == ':' && !qualified) { qualified = true; colon = length; }
    }
    if (lexer->lookahead != '}' || length == 0 || (qualified && colon == length)) return false;
    lexer->advance(lexer, false);
  } else {
    if (!variable_character(first) && first != ':') return false;
    unsigned length = 0, colon = 0;
    bool qualified = false;
    while (variable_character(lexer->lookahead) || lexer->lookahead == ':') {
      int32_t c = lexer->lookahead;
      if (c == ':') {
        lexer->mark_end(lexer);
        lexer->advance(lexer, false);
        // A double colon after an existing name begins static member access.
        if (length != 0 && lexer->lookahead == ':') {
          if (qualified && colon == length) return false;
          lexer->result_symbol = kind;
          return true;
        }
      } else lexer->advance(lexer, false);
      ++length;
      if (c == ':' && !qualified) { qualified = true; colon = length; }
    }
    if (qualified && colon == length) return false;
  }
  lexer->mark_end(lexer);
  lexer->result_symbol = kind;
  return true;
}

static bool scan_string_dollar(TSLexer *lexer, const bool *valid) {
  lexer->advance(lexer, false);
  if (variable_start(lexer->lookahead)) {
    return scan_variable_after_dollar(lexer, valid, VARIABLE);
  }
  if (lexer->lookahead == '(' || !valid[LITERAL_DOLLAR]) return false;
  lexer->mark_end(lexer);
  lexer->result_symbol = LITERAL_DOLLAR;
  return true;
}

static bool scan_string_escape(TSLexer *lexer) {
  if (lexer->lookahead != '`') return false;
  lexer->advance(lexer, false);
  if (lexer->eof(lexer)) return false;
  // PowerShell escapes one character. For a CRLF pair this consumes only CR;
  // the remaining LF may start a here-string footer.
  lexer->advance(lexer, false);
  lexer->mark_end(lexer);
  lexer->result_symbol = STRING_ESCAPE;
  return true;
}

static bool scan_here_string_content(TSLexer *lexer, const bool *valid) {
  bool content = false;
  while (!lexer->eof(lexer)) {
    int32_t c = lexer->lookahead;
    if (c == '`') break;
    if (c == '$') {
      lexer->advance(lexer, false);
      c = lexer->lookahead;
      if (variable_character(c) || c == '{' || c == '(' || c == '$' || c == '^' || c == ':') {
        // Return any preceding text, leaving the interpolation for the grammar.
        // With no preceding text, a failed external scan rewinds to the dollar.
        if (!content || !valid[HERE_STRING_CONTENT]) return false;
        lexer->result_symbol = HERE_STRING_CONTENT;
        return true;
      }
      // A dollar followed by punctuation/whitespace is ordinary string content.
      content = true;
      lexer->mark_end(lexer);
      continue;
    }
    if (newline(c)) {
      // Keep the physical newline in the footer token. A newline consumed by
      // an escape_sequence cannot start a footer, even at physical column zero.
      consume_newline(lexer);
      if (double_quote(lexer->lookahead)) {
        lexer->advance(lexer, false);
        if (lexer->lookahead == '@') {
          if (content && valid[HERE_STRING_CONTENT]) {
            lexer->result_symbol = HERE_STRING_CONTENT;
            return true;
          }
          if (!valid[HERE_STRING_END]) return false;
          lexer->advance(lexer, false);
          lexer->mark_end(lexer);
          lexer->result_symbol = HERE_STRING_END;
          return true;
        }
      }
    } else {
      lexer->advance(lexer, false);
    }
    content = true;
    lexer->mark_end(lexer);
  }
  if (!content || !valid[HERE_STRING_CONTENT]) return false;
  lexer->result_symbol = HERE_STRING_CONTENT;
  return true;
}

// PowerShell's tokenizer switches to argument mode after the command name.
// This token preserves punctuation in arguments (2+2 is text, not arithmetic)
// while stopping at interpolation and quoted pieces, represented by grammar rules.
static bool scan_generic(TSLexer *lexer, const bool *valid, bool immediate) {
  bool name = valid[COMMAND_NAME] && !immediate;
  enum TokenType kind = name ? COMMAND_NAME : BARE_WORD;
  int32_t first = lexer->lookahead;
  if (name && (first == '+' || first == '!')) return false;
  if (generic_end(first) || first == '#' || first == '@' || (!immediate && first == ':') || ((first == '[' || first == ']') && name)) return false;
  bool dollar_prefix = false;
  if (first == '$') {
    lexer->advance(lexer, false);
    if (variable_start(lexer->lookahead)) return scan_variable_after_dollar(lexer, valid, valid[VARIABLE] ? VARIABLE : ADJACENT_VARIABLE);
    if (lexer->lookahead == '(') return false;
    dollar_prefix = true;
  }
  if (!immediate && (first == '-' || first == 0x2013 || first == 0x2014 || first == 0x2015)) return false;
  char word[512]; unsigned n = 0; bool escape = false;
  if (dollar_prefix) word[n++] = '$';
  word[n] = 0;
  enum TokenType numeric_kind = INTEGER_LITERAL;
  bool numeric_start = ascii_digit(first) || first == '.';
  unsigned numeric_prefix = 0;
  bool continuation = false;
  while (!lexer->eof(lexer)) {
    int32_t c = lexer->lookahead;
    if (name && n == 1 && word[0] == '.' && c == '$') return false;
    if (name && c == '(' && n > 0 && word[n - 1] == '$') {
      unsigned depth = 0;
      do {
        c = lexer->lookahead;
        if (c == '(') ++depth;
        else if (c == ')') --depth;
        if (n + 1 < sizeof(word)) word[n++] = c < 128 ? (char)ascii_lower(c) : '?';
        lexer->advance(lexer, false);
      } while (depth > 0 && !lexer->eof(lexer));
      word[n] = 0;
      continue;
    }
    if (!escape && ((generic_end(c) && c != '<' && c != '>') || (!name && c == '$'))) break;
    if (c == '<' || c == '>') {
      if (name && n > 0 && numeric_word(word) && c == '>') {
        lexer->advance(lexer, false);
        if (lexer->lookahead != '&') {
          lexer->result_symbol = numeric_kind;
          return valid[numeric_kind];
        }
        if (n + 2 < sizeof(word)) { word[n++] = '>'; word[n++] = '&'; }
        lexer->advance(lexer, false);
        numeric_start = false;
        numeric_prefix = 0;
        word[n] = 0;
        continue;
      }
      if (n == 0 || numeric_word(word) || word[0] == '*') break;
    }
    if (c == '`' && !escape) {
      lexer->mark_end(lexer); lexer->advance(lexer, false);
      if (lexer->lookahead == '\r' || lexer->lookahead == '\n') { continuation = true; break; }
      escape = true; continue;
    }
    if (n + 1 < sizeof(word)) word[n++] = c < 128 ? (char)tolower((unsigned char)c) : '?';
    lexer->advance(lexer, false); escape = false;
    word[n] = 0;
    if (numeric_start && !(c == '.' && lexer->lookahead == '.') && numeric_token(word, &numeric_kind)) {
      numeric_prefix = n;
      lexer->mark_end(lexer);
    }
  }
  if (n == 0) return false;
  word[n] = 0;
  if (word[0] == '*' && (lexer->lookahead == '>' || lexer->lookahead == '<')) return false;
  if (!immediate) {
    if (numeric_word(word)) {
      if (valid[BARE_WORD] && (lexer->lookahead == '>' || lexer->lookahead == '<')) return false;
      if (!valid[numeric_kind]) return false;
      lexer->mark_end(lexer); lexer->result_symbol = numeric_kind; return true;
    }
    if (name && (reserved(word) || strcmp(word, ".") == 0)) return false;
    if (name && numeric_start && numeric_prefix != 0 && numeric_prefix < n &&
        strchr("+-*/%.", word[numeric_prefix]) != NULL) {
      if (!valid[numeric_kind]) return false;
      lexer->result_symbol = numeric_kind; return true;
    }
  }
  if (!continuation) lexer->mark_end(lexer);
  lexer->result_symbol = kind;
  return true;
}

static bool scan_number(TSLexer *lexer, const bool *valid) {
  bool real = false, digit = false;
  int32_t c = lexer->lookahead;
  if (c == '.') {
    lexer->advance(lexer, false);
    if (!ascii_digit(lexer->lookahead)) return false;
    real = true;
  } else if (!ascii_digit(c)) return false;
  if (lexer->lookahead == '0' && !real) {
    lexer->advance(lexer, false); digit = true;
    c = lexer->lookahead;
    if (c == 'x' || c == 'X' || c == 'b' || c == 'B') {
      bool hex = c == 'x' || c == 'X'; lexer->advance(lexer, false); digit = false;
      while ((hex && ascii_hex(lexer->lookahead)) || (!hex && (lexer->lookahead == '0' || lexer->lookahead == '1'))) { digit = true; lexer->advance(lexer, false); }
      if (!digit && !hex) return false;
      goto suffix;
    }
  }
  while (ascii_digit(lexer->lookahead)) { digit = true; lexer->advance(lexer, false); }
  if (!digit) return false;
  lexer->mark_end(lexer);
  if (!real && lexer->lookahead == '.') {
    lexer->advance(lexer, false);
    if (lexer->lookahead == '.') {
      if (!valid[INTEGER_LITERAL]) return false;
      lexer->result_symbol = INTEGER_LITERAL; return true;
    }
    real = true;
    while (ascii_digit(lexer->lookahead)) lexer->advance(lexer, false);
  }
  if (lexer->lookahead == 'e' || lexer->lookahead == 'E') {
    lexer->advance(lexer, false);
    if (lexer->lookahead == '+' || lexer->lookahead == '-') lexer->advance(lexer, false);
    if (!ascii_digit(lexer->lookahead)) return false;
    while (ascii_digit(lexer->lookahead)) lexer->advance(lexer, false);
    real = true;
  }
 suffix:
  c = ascii_lower(lexer->lookahead);
  if (c == 'u') { lexer->advance(lexer, false); c = ascii_lower(lexer->lookahead); if (c == 'l' || c == 's' || c == 'y') lexer->advance(lexer, false); }
  else if (c == 'l' || c == 's' || c == 'y' || c == 'n' || c == 'd') { if (c == 'd') real = true; lexer->advance(lexer, false); }
  c = ascii_lower(lexer->lookahead);
  if (c == 'k' || c == 'm' || c == 'g' || c == 't' || c == 'p') {
    lexer->advance(lexer, false); if (ascii_lower(lexer->lookahead) != 'b') return false; lexer->advance(lexer, false);
  }
  enum TokenType kind = real ? REAL_LITERAL : INTEGER_LITERAL;
  if (!valid[kind]) return false;
  lexer->mark_end(lexer); lexer->result_symbol = kind; return true;
}

bool tree_sitter_powershell_external_scanner_scan(void *payload, TSLexer *lexer, const bool *valid) {
  (void)payload;
  // During error recovery tree-sitter enables every external token. Do not
  // reinterpret arbitrary malformed source as a here-string body in that state.
  if (valid[STRING_ESCAPE] && lexer->lookahead == '`' &&
      !valid[EXPANDABLE_HERE_STRING_START]) return scan_string_escape(lexer);
  if (valid[LITERAL_DOLLAR] && !valid[EXPANDABLE_HERE_STRING_START] && lexer->lookahead == '$') return scan_string_dollar(lexer, valid);
  if ((valid[HERE_STRING_CONTENT] || valid[HERE_STRING_END]) &&
      !valid[EXPANDABLE_HERE_STRING_START]) {
    return scan_here_string_content(lexer, valid);
  }
  if (valid[LITERAL_DOLLAR] && !valid[EXPANDABLE_HERE_STRING_START]) return false;
  if (valid[ARGUMENT_END] && !valid[HERE_STRING_CONTENT]) {
    lexer->mark_end(lexer);
    if (generic_end(lexer->lookahead) && !quote(lexer->lookahead)) {
      lexer->result_symbol = ARGUMENT_END;
      return true;
    }
    if (lexer->lookahead == '`') {
      lexer->advance(lexer, false);
      if (newline(lexer->lookahead)) {
        lexer->result_symbol = ARGUMENT_END;
        return true;
      }
      return false;
    }
    if (lexer->lookahead == ':') {
      lexer->advance(lexer, false);
      if (lexer->lookahead == ':') {
        lexer->advance(lexer, false);
        if (generic_end(lexer->lookahead) && !quote(lexer->lookahead)) {
          lexer->result_symbol = ARGUMENT_END;
          return true;
        }
      }
      return false;
    }
  }
  if (valid[ADJACENT_VARIABLE] && lexer->lookahead == '$') {
    lexer->advance(lexer, false);
    return scan_variable_after_dollar(lexer, valid, ADJACENT_VARIABLE);
  }
  // While an argument is open, adjacent fragments and member operators are
  // handled by immediate grammar tokens. This stops bare-word tokens winning
  // over those tokens merely because the external scanner runs first.
  if (valid[ARGUMENT_END] && !valid[HERE_STRING_CONTENT]) return false;
  while (horizontal(lexer->lookahead)) lexer->advance(lexer, true);
  if (valid[VARIABLE] && !valid[COMMAND_NAME] && !valid[BARE_WORD] && lexer->lookahead == '$') {
    lexer->advance(lexer, false);
    return scan_variable_after_dollar(lexer, valid, VARIABLE);
  }
  if (lexer->lookahead == '@' && (valid[SINGLE_QUOTED_HERE_STRING] || valid[EXPANDABLE_HERE_STRING_START])) return scan_here_string(lexer, valid);
  if (valid[COMMAND_NAME] || valid[BARE_WORD]) {
    // A failed generic scan cannot be rewound. Numeric starts are handled by
    // the ordinary lexer unless an argument-mode bare token is required.
    if (ascii_digit(lexer->lookahead) || lexer->lookahead == '.') {
      if (valid[BARE_WORD] || valid[COMMAND_NAME]) return scan_generic(lexer, valid, false);
    } else return scan_generic(lexer, valid, false);
  }
  if (valid[INTEGER_LITERAL] || valid[REAL_LITERAL]) return scan_number(lexer, valid);
  return false;
}
