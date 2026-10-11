#include "tree_sitter/parser.h"
#include "variable_chars.h"
#include <stdbool.h>
#include <stdlib.h>
#include <ctype.h>
#include <string.h>

// Keep this order aligned with grammar.js's externals. Context tokens expose
// parser lookahead to the scanner and are never emitted as source tokens.
enum TokenType {
  SINGLE_QUOTED_HERE_STRING,
  EXPANDABLE_HERE_STRING_START,
  COMMAND_NAME,
  BARE_WORD,
  INTEGER_LITERAL,
  REAL_LITERAL,
  HERE_STRING_CONTENT,
  HERE_STRING_END,
  EMPTY_EXPANDABLE_HERE_STRING,
  STRING_ESCAPE,
  VARIABLE,
  ADJACENT_VARIABLE,
  LITERAL_DOLLAR,
  ARGUMENT_END,
  FLOW_LABEL,
  SWITCH_DEFAULT,
  LABEL_START,
  EXPRESSION_ARGUMENT_END,
  MEMBER_ARGUMENT_END,
  PARAMETER_NAME,
  FUNCTION_NAME,
  COMMENT,
  METHOD_INVOCATION_CONTEXT, // Reject trivia before a method's opening '('.
  LINE_CONTINUATION,
  POSTFIX_CONTEXT, // Reject continuations before adjacent member/index access.
  ARGUMENT_SEPARATOR,
  SPLAT_EXPRESSION
};

// All context comes from valid tokens and source lookahead. No mutable scanner
// state needs serialization, including during incremental parsing or recovery.
void *tree_sitter_powershell_external_scanner_create(void) {
  return NULL;
}

void tree_sitter_powershell_external_scanner_destroy(void *payload) {
  (void)payload;
}

unsigned tree_sitter_powershell_external_scanner_serialize(void *payload, char *buffer) {
  (void)payload;
  (void)buffer;
  return 0;
}

void tree_sitter_powershell_external_scanner_deserialize(void *payload, const char *buffer,
                                                         unsigned length) {
  (void)payload;
  (void)buffer;
  (void)length;
}

static bool horizontal(int32_t c) {
  return c == ' ' || c == '\t' || c == '\f' || c == '\v' || c == 0x85 || c == 0xa0 || c == 0x1680 ||
         (c >= 0x2000 && c <= 0x200a) || c == 0x2028 || c == 0x2029 || c == 0x202f || c == 0x205f ||
         c == 0x3000;
}

static bool ascii_digit(int32_t c) {
  return c >= '0' && c <= '9';
}

static int32_t ascii_lower(int32_t c) {
  return c >= 'A' && c <= 'Z' ? c + 32 : c;
}

static bool ascii_hex(int32_t c) {
  c = ascii_lower(c);
  return ascii_digit(c) || (c >= 'a' && c <= 'f');
}

static bool quote(int32_t c) {
  return c == '\'' || c == '"' || (c >= 0x2018 && c <= 0x201e);
}

static bool generic_end(int32_t c) {
  return c == 0 || horizontal(c) || c == '\r' || c == '\n' || c == ';' || c == '|' || c == '&' ||
         c == '(' || c == ')' || c == '{' || c == '}' || c == ',' || c == '<' || c == '>' ||
         quote(c);
}

static bool suffix_ok(const char *s) {
  if (*s == 'u') {
    ++s;
    if (*s == 'l' || *s == 's' || *s == 'y')
      ++s;
  } else if (*s == 'l' || *s == 's' || *s == 'y' || *s == 'n' || *s == 'd')
    ++s;
  if ((*s == 'k' || *s == 'm' || *s == 'g' || *s == 't' || *s == 'p') && s[1] == 'b')
    s += 2;
  return *s == 0;
}

static bool numeric_token(const char *s, enum TokenType *kind) {
  if (*s == '+' || *s == '-')
    ++s;
  const char *end = s;
  bool real = false;
  if (s[0] == '0' && s[1] == 'x') {
    end += 2;
    // PowerShell accepts the empty hexadecimal prefix as zero, including its
    // numeric suffixes. C's strtod would also admit unsupported hex floats.
    while (ascii_hex(*end))
      ++end;
  } else if (s[0] == '0' && s[1] == 'b') {
    end += 2;
    if (*end != '0' && *end != '1')
      return false;
    while (*end == '0' || *end == '1')
      ++end;
  } else {
    bool digit = false;
    while (ascii_digit(*end)) {
      digit = true;
      ++end;
    }
    if (*end == '.') {
      real = true;
      ++end;
      while (ascii_digit(*end)) {
        digit = true;
        ++end;
      }
    }
    if (!digit)
      return false;
    if (*end == 'e') {
      real = true;
      ++end;
      if (*end == '+' || *end == '-')
        ++end;
      if (!ascii_digit(*end))
        return false;
      while (ascii_digit(*end))
        ++end;
    }
  }
  // Hexadecimal d/e are digits, not decimal suffixes or exponent markers.
  // Integral suffixes are also valid on decimal fractional/exponent forms.
  if (*end == 'd')
    real = true;
  if (!suffix_ok(end))
    return false;
  *kind = real ? REAL_LITERAL : INTEGER_LITERAL;
  return true;
}

static bool numeric_word(const char *s) {
  enum TokenType kind;
  return numeric_token(s, &kind);
}

static bool reserved(const char *s) {
  static const char *const words[] = {
      "if",     "elseif",   "else",  "switch", "foreach",       "for",      "while",
      "do",     "until",    "try",   "catch",  "finally",       "trap",     "function",
      "filter", "workflow", "class", "enum",   "break",         "continue", "return",
      "throw",  "exit",     "data",  "using",  "configuration", "param",    "dynamicparam",
      "begin",  "process",  "end",   "clean"};
  for (unsigned i = 0; i < sizeof(words) / sizeof(words[0]); ++i)
    if (strcmp(s, words[i]) == 0)
      return true;
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
  return c == ' ' || c == '\t' || c == '\f' || c == '\v' || c == 0x85 || c == 0xa0 || c == 0x1680 ||
         (c >= 0x2000 && c <= 0x200a) || c == 0x2028 || c == 0x2029 || c == 0x202f || c == 0x205f ||
         c == 0x3000;
}

static bool newline(int32_t c) {
  return c == '\n' || c == '\r';
}
// Comments remain named extras. The method context is a lookahead sentinel
// that the grammar never needs to consume: a block comment may precede member
// access, but PowerShell forbids one between a method name and its opening '('.
static bool scan_comment(TSLexer *lexer, const bool *valid) {
  if (lexer->lookahead == '#') {
    do
      lexer->advance(lexer, false);
    while (!lexer->eof(lexer) && !newline(lexer->lookahead));
    lexer->mark_end(lexer);
    lexer->result_symbol = COMMENT;
    return true;
  }
  if (lexer->lookahead != '<')
    return false;
  lexer->advance(lexer, false);
  if (lexer->lookahead != '#')
    return false;
  lexer->advance(lexer, false);
  while (!lexer->eof(lexer)) {
    if (lexer->lookahead == '#') {
      lexer->advance(lexer, false);
      if (lexer->lookahead == '>') {
        lexer->advance(lexer, false);
        lexer->mark_end(lexer);
        if (valid[METHOD_INVOCATION_CONTEXT]) {
          while (horizontal(lexer->lookahead))
            lexer->advance(lexer, false);
          if (lexer->lookahead == '(')
            return false;
        }
        lexer->result_symbol = COMMENT;
        return true;
      }
    } else
      lexer->advance(lexer, false);
  }
  return false;
}

static void consume_newline(TSLexer *lexer) {
  bool carriage_return = lexer->lookahead == '\r';
  lexer->advance(lexer, false);
  if (carriage_return && lexer->lookahead == '\n')
    lexer->advance(lexer, false);
}

static bool scan_here_string_after_at(TSLexer *lexer, const bool *valid) {
  bool single = single_quote(lexer->lookahead);
  if (!single && !double_quote(lexer->lookahead))
    return false;
  if (!(single ? valid[SINGLE_QUOTED_HERE_STRING] : valid[EXPANDABLE_HERE_STRING_START]))
    return false;
  lexer->advance(lexer, false);
  while (here_header_space(lexer->lookahead))
    lexer->advance(lexer, false);
  if (!newline(lexer->lookahead))
    return false;
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

// The caller consumed the backtick. Named external extras cause tree-sitter
// to rerun the external lexer after a continuation, which is necessary for
// argument-mode words. A continuation cannot separate a postfix operator.
static bool scan_line_continuation_after_tick(TSLexer *lexer, const bool *valid) {
  if (!valid[LINE_CONTINUATION] || !newline(lexer->lookahead))
    return false;
  consume_newline(lexer);
  lexer->mark_end(lexer);
  if (valid[POSTFIX_CONTEXT] || valid[METHOD_INVOCATION_CONTEXT]) {
    while (horizontal(lexer->lookahead))
      lexer->advance(lexer, false);
    int32_t c = lexer->lookahead;
    if (c == '.' || c == '[' || c == '(')
      return false;
    if (c == '?') {
      lexer->advance(lexer, false);
      if (lexer->lookahead == '.' || lexer->lookahead == '[')
        return false;
    }
  }
  lexer->result_symbol = LINE_CONTINUATION;
  return true;
}

static bool variable_character(int32_t c) {
  if (c == '_' || c == '?')
    return true;
  if (c < 0 || c > 0xffff)
    return false;
  unsigned left = 0,
           right = sizeof(VARIABLE_CHARACTER_RANGES) / sizeof(VARIABLE_CHARACTER_RANGES[0]);
  while (left < right) {
    unsigned middle = left + (right - left) / 2;
    if (c < VARIABLE_CHARACTER_RANGES[middle][0])
      right = middle;
    else if (c > VARIABLE_CHARACTER_RANGES[middle][1])
      left = middle + 1;
    else
      return true;
  }
  return false;
}

// .NET's identifier start uses letters and underscore, while subsequent
// characters may also be decimal digits. Keep the BMP digit blocks distinct
// from the shared letter-or-digit table used for variables.
static bool decimal_character(int32_t c) {
  static const uint16_t starts[] = {
      0x0030, 0x0660, 0x06f0, 0x07c0, 0x0966, 0x09e6, 0x0a66, 0x0ae6, 0x0b66, 0x0be6,
      0x0c66, 0x0ce6, 0x0d66, 0x0de6, 0x0e50, 0x0ed0, 0x0f20, 0x1040, 0x1090, 0x17e0,
      0x1810, 0x1946, 0x19d0, 0x1a80, 0x1a90, 0x1b50, 0x1bb0, 0x1c40, 0x1c50, 0xa620,
      0xa8d0, 0xa900, 0xa9d0, 0xa9f0, 0xaa50, 0xabf0, 0xff10,
  };
  for (unsigned i = 0; i < sizeof(starts) / sizeof(starts[0]); ++i) {
    if (c >= starts[i] && c < starts[i] + 10)
      return true;
  }
  return false;
}

static bool scan_flow_label(TSLexer *lexer) {
  int32_t c = lexer->lookahead;
  if (!variable_character(c) || c == '?' || decimal_character(c))
    return false;
  do {
    lexer->advance(lexer, false);
    c = lexer->lookahead;
  } while (variable_character(c) && c != '?');
  lexer->mark_end(lexer);
  lexer->result_symbol = FLOW_LABEL;
  return true;
}

static bool scan_label_start(TSLexer *lexer) {
  lexer->mark_end(lexer);
  lexer->advance(lexer, false); // Colon, followed by an entire simple name.
  int32_t c = lexer->lookahead;
  if (!variable_character(c) || c == '?' || decimal_character(c))
    return false;
  do {
    lexer->advance(lexer, false);
    c = lexer->lookahead;
  } while (variable_character(c) && c != '?');
  if (!generic_end(c))
    return false;
  lexer->result_symbol = LABEL_START;
  return true;
}

static bool variable_start(int32_t c) {
  return variable_character(c) || c == '{' || c == '$' || c == '^' || c == ':';
}

// Called immediately after a backtick. This mirrors tokenizer.cs
// Backtick/ScanUnicodeEscape: lowercase `u requires 1..6 hexadecimal digits
// and a value <= U+10FFFF.
static bool scan_backtick_escape_after_tick(TSLexer *lexer, int32_t *decoded,
                                            bool *physical_newline) {
  if (physical_newline)
    *physical_newline = false;
  if (lexer->eof(lexer))
    return false;
  int32_t c = lexer->lookahead;
  if (physical_newline)
    *physical_newline = c == '\r' || c == '\n';
  lexer->advance(lexer, false);
  if (c != 'u') {
    switch (c) {
    case '0':
      c = 0;
      break;
    case 'a':
      c = '\a';
      break;
    case 'b':
      c = '\b';
      break;
    case 'e':
      c = 0x1b;
      break;
    case 'f':
      c = '\f';
      break;
    case 'n':
      c = '\n';
      break;
    case 'r':
      c = '\r';
      break;
    case 't':
      c = '\t';
      break;
    case 'v':
      c = '\v';
      break;
    }
    *decoded = c;
    return true;
  }
  if (lexer->lookahead != '{')
    return false;
  lexer->advance(lexer, false);
  uint32_t value = 0;
  unsigned digits = 0;
  while (ascii_hex(lexer->lookahead)) {
    if (++digits > 6)
      return false;
    c = ascii_lower(lexer->lookahead);
    value = value * 16 + (uint32_t)(ascii_digit(c) ? c - '0' : c - 'a' + 10);
    lexer->advance(lexer, false);
  }
  if (digits == 0 || value > 0x10ffff || lexer->lookahead != '}')
    return false;
  lexer->advance(lexer, false);
  *decoded = (int32_t)value;
  return true;
}

static bool scan_backtick_escape(TSLexer *lexer, int32_t *decoded, bool *physical_newline) {
  if (lexer->lookahead != '`')
    return false;
  lexer->advance(lexer, false);
  return scan_backtick_escape_after_tick(lexer, decoded, physical_newline);
}

// The caller has consumed '$'. Scope/drive qualification requires a name
// after the first colon, even inside braced and expandable-string variables.
static bool scan_variable_after_dollar(TSLexer *lexer, const bool *valid, enum TokenType kind) {
  if (!valid[kind])
    return false;
  int32_t first = lexer->lookahead;
  if (first == '?' || first == '$' || first == '^') {
    lexer->advance(lexer, false);
  } else if (first == '{') {
    lexer->advance(lexer, false);
    unsigned length = 0, colon = 0;
    bool qualified = false;
    while (!lexer->eof(lexer) && lexer->lookahead != '}') {
      int32_t c = lexer->lookahead;
      if (c == '{')
        return false;
      if (c == '`') {
        if (!scan_backtick_escape(lexer, &c, NULL))
          return false;
      } else
        lexer->advance(lexer, false);
      ++length;
      if (c == ':' && !qualified) {
        qualified = true;
        colon = length;
      }
    }
    if (lexer->lookahead != '}' || length == 0 || (qualified && colon == length))
      return false;
    lexer->advance(lexer, false);
  } else {
    if (!variable_character(first) && first != ':')
      return false;
    unsigned length = 0, colon = 0;
    bool qualified = false;
    while (variable_character(lexer->lookahead) || lexer->lookahead == ':') {
      int32_t c = lexer->lookahead;
      if (c == ':') {
        lexer->mark_end(lexer);
        lexer->advance(lexer, false);
        // A double colon after an existing name begins static member access.
        if (length != 0 && lexer->lookahead == ':') {
          if (qualified && colon == length)
            return false;
          lexer->result_symbol = kind;
          return true;
        }
      } else
        lexer->advance(lexer, false);
      ++length;
      if (c == ':' && !qualified) {
        qualified = true;
        colon = length;
      }
    }
    if (qualified && colon == length)
      return false;
  }
  lexer->mark_end(lexer);
  lexer->result_symbol = kind;
  return true;
}

// The caller consumed '@'. A splat uses the same UTF-16 name characters as
// a variable. In argument mode an adjacent non-name character instead turns
// the entire token into generic text (for example @args/path or @args😀).
static bool scan_splat_after_at(TSLexer *lexer, const bool *valid) {
  int32_t first = lexer->lookahead;
  bool special = first == '?' || first == '$' || first == '^';
  if (!variable_character(first) && first != ':' && !special)
    return false;
  bool generic = false, qualified = false;
  size_t length = 0, colon = 0;
  while (!lexer->eof(lexer)) {
    int32_t c = lexer->lookahead;
    if (length != 0 && (c == '$' || quote(c))) {
      generic = true;
      break;
    }
    // Ordinary splat names stop before member/index and assignment operators.
    // The grammar then rejects member access on a splat, as PowerShell does.
    if (!generic && !special && (c == '.' || c == '[' || c == '='))
      break;
    if (generic_end(c) && c != '<' && c != '>')
      break;
    if (!generic && !special && c == ':') {
      lexer->mark_end(lexer);
      lexer->advance(lexer, false);
      if (length != 0 && lexer->lookahead == ':') {
        if (!valid[SPLAT_EXPRESSION] || (qualified && colon == length))
          return false;
        lexer->result_symbol = SPLAT_EXPRESSION;
        return true;
      }
    } else if (c == '`') {
      lexer->advance(lexer, false);
      generic = true;
      if (lexer->eof(lexer))
        break;
      bool physical_newline = false;
      if (!scan_backtick_escape_after_tick(lexer, &c, &physical_newline))
        return false;
      if (physical_newline)
        continue;
    } else {
      lexer->advance(lexer, false);
    }
    if ((special && length != 0) ||
        (!variable_character(c) && c != ':' && !(special && length == 0))) {
      generic = true;
    }
    ++length;
    if (c == ':' && !qualified) {
      qualified = true;
      colon = length;
    }
  }
  if (length == 0)
    return false;
  if (!generic && qualified && colon == length)
    return false;
  enum TokenType kind = generic ? BARE_WORD : SPLAT_EXPRESSION;
  if (!valid[kind])
    return false;
  lexer->mark_end(lexer);
  lexer->result_symbol = kind;
  return true;
}

static bool scan_string_dollar(TSLexer *lexer, const bool *valid) {
  lexer->advance(lexer, false);
  if (variable_start(lexer->lookahead)) {
    return scan_variable_after_dollar(lexer, valid, VARIABLE);
  }
  if (lexer->lookahead == '(' || !valid[LITERAL_DOLLAR])
    return false;
  lexer->mark_end(lexer);
  lexer->result_symbol = LITERAL_DOLLAR;
  return true;
}

// Function names are literal generic tokens. Quoted pieces and escaped
// characters can occur after an unquoted prefix, and embedded dollar
// subexpressions remain part of the name rather than evaluated children.
static bool scan_function_name(TSLexer *lexer) {
  int32_t c = lexer->lookahead;
  if (generic_end(c) || c == '$' || c == '@' || c == '#')
    return false;
  bool previous_dollar = false;
  while (!lexer->eof(lexer)) {
    c = lexer->lookahead;
    if (c == '`') {
      int32_t decoded;
      bool physical_newline;
      if (!scan_backtick_escape(lexer, &decoded, &physical_newline)) {
        if (!lexer->eof(lexer))
          return false;
      }
      previous_dollar = false;
      continue;
    }
    if (single_quote(c) || double_quote(c)) {
      bool single = single_quote(c);
      lexer->advance(lexer, false);
      bool closed = false;
      while (!lexer->eof(lexer)) {
        c = lexer->lookahead;
        if (single ? single_quote(c) : double_quote(c)) {
          lexer->advance(lexer, false);
          if (single ? single_quote(lexer->lookahead) : double_quote(lexer->lookahead)) {
            lexer->advance(lexer, false);
            continue;
          }
          closed = true;
          break;
        }
        if (!single && c == '`') {
          int32_t decoded;
          if (!scan_backtick_escape(lexer, &decoded, NULL))
            return false;
        } else
          lexer->advance(lexer, false);
      }
      if (!closed)
        return false;
      previous_dollar = false;
      continue;
    }
    if (c == '(' && previous_dollar) {
      unsigned depth = 0;
      do {
        c = lexer->lookahead;
        if (c == '(')
          ++depth;
        else if (c == ')')
          --depth;
        lexer->advance(lexer, false);
      } while (depth != 0 && !lexer->eof(lexer));
      if (depth != 0)
        return false;
      previous_dollar = false;
      continue;
    }
    if (generic_end(c) && c != '<' && c != '>')
      break;
    previous_dollar = c == '$';
    lexer->advance(lexer, false);
  }
  lexer->mark_end(lexer);
  lexer->result_symbol = FUNCTION_NAME;
  return true;
}

static bool scan_string_escape(TSLexer *lexer) {
  if (lexer->lookahead != '`')
    return false;
  // PowerShell escapes one character. For a CRLF pair this consumes only CR;
  // the remaining LF may start a here-string footer.
  int32_t decoded;
  if (!scan_backtick_escape(lexer, &decoded, NULL))
    return false;
  lexer->mark_end(lexer);
  lexer->result_symbol = STRING_ESCAPE;
  return true;
}

static bool scan_here_string_content(TSLexer *lexer, const bool *valid) {
  bool content = false;
  while (!lexer->eof(lexer)) {
    int32_t c = lexer->lookahead;
    if (c == '`')
      break;
    if (c == '$') {
      lexer->advance(lexer, false);
      c = lexer->lookahead;
      if (variable_character(c) || c == '{' || c == '(' || c == '$' || c == '^' || c == ':') {
        // Return any preceding text, leaving the interpolation for the grammar.
        // With no preceding text, a failed external scan rewinds to the dollar.
        if (!content || !valid[HERE_STRING_CONTENT])
          return false;
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
          if (!valid[HERE_STRING_END])
            return false;
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
  if (!content || !valid[HERE_STRING_CONTENT])
    return false;
  lexer->result_symbol = HERE_STRING_CONTENT;
  return true;
}

// PowerShell's tokenizer switches to argument mode after the command name.
// This token preserves punctuation in arguments (2+2 is text, not arithmetic)
// while stopping at interpolation and quoted pieces, represented by grammar rules.
typedef struct {
  char *data;
  size_t length;
  size_t capacity;
  char inline_data[128];
} GenericWord;

static bool generic_word_append(GenericWord *word, int32_t c) {
  if (word->length + 1 >= word->capacity) {
    if (word->capacity > SIZE_MAX / 2)
      return false;
    size_t capacity = word->capacity * 2;
    char *data;
    if (word->data == word->inline_data) {
      data = malloc(capacity);
      if (data != NULL)
        memcpy(data, word->data, word->length + 1);
    } else
      data = realloc(word->data, capacity);
    if (data == NULL)
      return false;
    word->data = data;
    word->capacity = capacity;
  }
  // Classification only needs folded ASCII. Keep non-ASCII and cooked NUL
  // visibly nonnumeric without inserting an embedded C-string terminator.
  word->data[word->length++] = c > 0 && c < 128 ? (char)ascii_lower(c) : '?';
  word->data[word->length] = 0;
  return true;
}

static bool generic_numeric_pending(GenericWord *word) {
  size_t n = word->length;
  if ((n == 1 && word->data[0] == '.') || (n == 2 && word->data[0] == '0' && word->data[1] == 'b'))
    return true;
  size_t pending = 0;
  char last = word->data[n - 1];
  if (last == 'e' || strchr("kmgtp", last) != NULL)
    pending = 1;
  else if (n > 1 && (last == '+' || last == '-') && word->data[n - 2] == 'e')
    pending = 2;
  if (pending == 0 || pending >= n)
    return false;
  char saved = word->data[n - pending];
  word->data[n - pending] = 0;
  bool possible = numeric_word(word->data);
  word->data[n - pending] = saved;
  return possible;
}

static bool generic_numeric_boundary(int32_t c) {
  return c > 0 && c < 128 && strchr("+-*/%.<>", (char)c) != NULL;
}

static bool generic_dash(int32_t c) {
  return c == '-' || c == 0x2013 || c == 0x2014 || c == 0x2015;
}

static bool generic_unary_operator(const char *word) {
  return strcmp(word, "-") == 0 || strcmp(word, "--") == 0 || strcmp(word, "-not") == 0 ||
         strcmp(word, "-bnot") == 0 || strcmp(word, "-split") == 0 || strcmp(word, "-join") == 0;
}

static bool scan_generic_word(TSLexer *lexer, const bool *valid, bool immediate,
                              GenericWord *buffer) {
  bool name = valid[COMMAND_NAME] && !immediate;
  enum TokenType kind = name ? COMMAND_NAME : BARE_WORD;
  int32_t first = lexer->lookahead;
  if (name && (first == '+' || first == '!'))
    return false;
  if (generic_end(first) || first == '#' || first == '@' || (!immediate && first == ':') ||
      ((first == '[' || first == ']') && name))
    return false;
  bool dollar_prefix = false;
  if (first == '$') {
    lexer->advance(lexer, false);
    if (variable_start(lexer->lookahead))
      return scan_variable_after_dollar(lexer, valid,
                                        valid[VARIABLE] ? VARIABLE : ADJACENT_VARIABLE);
    if (lexer->lookahead == '(')
      return false;
    dollar_prefix = true;
  }
  bool leading_dash = !immediate && generic_dash(first);
  if (leading_dash) {
    if (!generic_word_append(buffer, '-'))
      return false;
    lexer->advance(lexer, false);
    int32_t next = lexer->lookahead;
    if (name && !((next >= 'a' && next <= 'z') || (next >= 'A' && next <= 'Z') ||
                  decimal_character(next) || next == '.'))
      return false;
    if (valid[PARAMETER_NAME] && variable_character(next) && !decimal_character(next)) {
      // Parameter names are raw text: dollars, backticks, and comment markers
      // are literal. A quote changes the whole argument to a generic token.
      while (!lexer->eof(lexer)) {
        int32_t c = lexer->lookahead;
        if (quote(c)) {
          lexer->mark_end(lexer);
          lexer->result_symbol = BARE_WORD;
          return valid[BARE_WORD];
        }
        if (c == '.' || c == '[' || c == ':' || generic_end(c))
          break;
        if (!generic_word_append(buffer, c))
          return false;
        lexer->advance(lexer, false);
      }
      if (buffer->length <= 1)
        return false;
      lexer->mark_end(lexer);
      lexer->result_symbol = PARAMETER_NAME;
      return true;
    }
  }
  if (dollar_prefix && !generic_word_append(buffer, '$'))
    return false;
  char *word = buffer->data;
  size_t n = buffer->length;
  bool any_escape = false;
  enum TokenType numeric_kind = INTEGER_LITERAL;
  bool numeric_start = ascii_digit(first) || first == '.' ||
                       (leading_dash && (ascii_digit(lexer->lookahead) || lexer->lookahead == '.'));
  bool numeric_live = numeric_start;
  size_t numeric_prefix = 0;
  while (!lexer->eof(lexer)) {
    int32_t c = lexer->lookahead;
    if (name && leading_dash && n > 1 && !any_escape && generic_unary_operator(word) &&
        !((c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z')))
      return false;
    if (name && n == 1 && word[0] == '.' && c == '$')
      return false;
    if (name && c == '(' && n > 0 && word[n - 1] == '$') {
      unsigned depth = 0;
      do {
        c = lexer->lookahead;
        if (c == '(')
          ++depth;
        else if (c == ')')
          --depth;
        if (!generic_word_append(buffer, c))
          return false;
        word = buffer->data;
        n = buffer->length;
        lexer->advance(lexer, false);
      } while (depth > 0 && !lexer->eof(lexer));
      continue;
    }
    if ((generic_end(c) && c != '<' && c != '>') || (!name && c == '$'))
      break;
    if (c == '<' || c == '>') {
      enum TokenType candidate_kind = numeric_kind;
      bool numeric = !any_escape && numeric_live && numeric_token(word, &candidate_kind);
      if (name && n > 0 && numeric && c == '>') {
        numeric_kind = candidate_kind;
        lexer->mark_end(lexer);
        lexer->advance(lexer, false);
        if (lexer->lookahead != '&') {
          lexer->result_symbol = numeric_kind;
          return valid[numeric_kind];
        }
        if (!generic_word_append(buffer, '>') || !generic_word_append(buffer, '&'))
          return false;
        word = buffer->data;
        n = buffer->length;
        lexer->advance(lexer, false);
        numeric_start = false;
        numeric_live = false;
        numeric_prefix = 0;
        continue;
      }
      if (n == 0 || numeric || word[0] == '*')
        break;
    }
    if (c == '`') {
      lexer->advance(lexer, false);
      if (lexer->eof(lexer)) {
        // A trailing backtick belongs to a nonempty generic token; a lone
        // backtick is incomplete input.
        if (n == 0 || !generic_word_append(buffer, '`'))
          return false;
        word = buffer->data;
        n = buffer->length;
        any_escape = true;
        break;
      }
      bool physical_newline = false;
      if (n == 0 && newline(lexer->lookahead))
        return scan_line_continuation_after_tick(lexer, valid);
      if (!scan_backtick_escape_after_tick(lexer, &c, &physical_newline))
        return false;
      any_escape = true;
      numeric_live = false;
      // Backtick escapes one physical character. For CRLF only CR is joined;
      // the LF remains a statement terminator. Cooked `n is token content.
      if (physical_newline)
        continue;
    } else {
      lexer->advance(lexer, false);
    }
    if (!generic_word_append(buffer, c))
      return false;
    word = buffer->data;
    n = buffer->length;
    if (numeric_live && !(c == '.' && lexer->lookahead == '.') &&
        (generic_numeric_boundary(c) || generic_numeric_boundary(lexer->lookahead))) {
      // Probe only at boundaries where a number may end. Once a prefix cannot
      // become numeric, stop probing: large malformed words stay linear too.
      enum TokenType candidate_kind = numeric_kind;
      if (numeric_token(word, &candidate_kind)) {
        numeric_kind = candidate_kind;
        numeric_prefix = n;
        lexer->mark_end(lexer);
      } else
        numeric_live = generic_numeric_pending(buffer);
    }
  }
  if (n == 0)
    return false;
  if (word[0] == '*' && (lexer->lookahead == '>' || lexer->lookahead == '<'))
    return false;
  if (!immediate && !any_escape &&
      (strcmp(word, "--%") == 0 || (valid[PARAMETER_NAME] && strcmp(word, "--") == 0)))
    return false;
  if (!any_escape && valid[SWITCH_DEFAULT] && strcmp(word, "default") == 0) {
    lexer->mark_end(lexer);
    lexer->result_symbol = SWITCH_DEFAULT;
    return true;
  }
  if (!immediate) {
    enum TokenType full_kind = numeric_kind;
    if (!any_escape && numeric_token(word, &full_kind)) {
      // Signed numbers retain the grammar's unary-expression structure.
      if (leading_dash)
        return false;
      if (valid[BARE_WORD] && (lexer->lookahead == '>' || lexer->lookahead == '<'))
        return false;
      if (!valid[full_kind])
        return false;
      lexer->mark_end(lexer);
      lexer->result_symbol = full_kind;
      return true;
    }
    if (name && !any_escape &&
        (reserved(word) || strcmp(word, ".") == 0 || generic_unary_operator(word)))
      return false;
    if (name && !any_escape && numeric_start && numeric_prefix != 0 && numeric_prefix < n &&
        strchr("+-*/%.", word[numeric_prefix]) != NULL) {
      if (leading_dash)
        return false;
      if (!valid[numeric_kind])
        return false;
      lexer->result_symbol = numeric_kind;
      return true;
    }
  }
  lexer->mark_end(lexer);
  lexer->result_symbol = kind;
  return true;
}

static bool scan_generic(TSLexer *lexer, const bool *valid, bool immediate) {
  GenericWord word = {0};
  word.data = word.inline_data;
  word.capacity = sizeof(word.inline_data);
  bool result = scan_generic_word(lexer, valid, immediate, &word);
  if (word.data != word.inline_data)
    free(word.data);
  return result;
}

static bool scan_number(TSLexer *lexer, const bool *valid) {
  bool real = false, digit = false;
  int32_t c = lexer->lookahead;
  if (c == '.') {
    lexer->advance(lexer, false);
    if (!ascii_digit(lexer->lookahead))
      return false;
    real = true;
  } else if (!ascii_digit(c))
    return false;
  if (lexer->lookahead == '0' && !real) {
    lexer->advance(lexer, false);
    digit = true;
    c = lexer->lookahead;
    if (c == 'x' || c == 'X' || c == 'b' || c == 'B') {
      bool hex = c == 'x' || c == 'X';
      lexer->advance(lexer, false);
      digit = false;
      while ((hex && ascii_hex(lexer->lookahead)) ||
             (!hex && (lexer->lookahead == '0' || lexer->lookahead == '1'))) {
        digit = true;
        lexer->advance(lexer, false);
      }
      if (!digit && !hex)
        return false;
      goto suffix;
    }
  }
  while (ascii_digit(lexer->lookahead)) {
    digit = true;
    lexer->advance(lexer, false);
  }
  if (!digit)
    return false;
  lexer->mark_end(lexer);
  if (!real && lexer->lookahead == '.') {
    lexer->advance(lexer, false);
    if (lexer->lookahead == '.') {
      if (!valid[INTEGER_LITERAL])
        return false;
      lexer->result_symbol = INTEGER_LITERAL;
      return true;
    }
    real = true;
    while (ascii_digit(lexer->lookahead))
      lexer->advance(lexer, false);
  }
  if (lexer->lookahead == 'e' || lexer->lookahead == 'E') {
    lexer->advance(lexer, false);
    if (lexer->lookahead == '+' || lexer->lookahead == '-')
      lexer->advance(lexer, false);
    if (!ascii_digit(lexer->lookahead))
      return false;
    while (ascii_digit(lexer->lookahead))
      lexer->advance(lexer, false);
    real = true;
  }
suffix:
  c = ascii_lower(lexer->lookahead);
  if (c == 'u') {
    lexer->advance(lexer, false);
    c = ascii_lower(lexer->lookahead);
    if (c == 'l' || c == 's' || c == 'y')
      lexer->advance(lexer, false);
  } else if (c == 'l' || c == 's' || c == 'y' || c == 'n' || c == 'd') {
    if (c == 'd')
      real = true;
    lexer->advance(lexer, false);
  }
  c = ascii_lower(lexer->lookahead);
  if (c == 'k' || c == 'm' || c == 'g' || c == 't' || c == 'p') {
    lexer->advance(lexer, false);
    if (ascii_lower(lexer->lookahead) != 'b')
      return false;
    lexer->advance(lexer, false);
  }
  enum TokenType kind = real ? REAL_LITERAL : INTEGER_LITERAL;
  if (!valid[kind])
    return false;
  lexer->mark_end(lexer);
  lexer->result_symbol = kind;
  return true;
}

bool tree_sitter_powershell_external_scanner_scan(void *payload, TSLexer *lexer,
                                                  const bool *valid) {
  (void)payload;
  // During error recovery tree-sitter enables every external token. Do not
  // reinterpret arbitrary malformed source as a here-string body in that state.
  if (valid[STRING_ESCAPE] && lexer->lookahead == '`' && !valid[EXPANDABLE_HERE_STRING_START])
    return scan_string_escape(lexer);
  if (valid[LITERAL_DOLLAR] && !valid[EXPANDABLE_HERE_STRING_START] && lexer->lookahead == '$')
    return scan_string_dollar(lexer, valid);
  if ((valid[HERE_STRING_CONTENT] || valid[HERE_STRING_END]) &&
      !valid[EXPANDABLE_HERE_STRING_START]) {
    return scan_here_string_content(lexer, valid);
  }
  if (valid[LITERAL_DOLLAR] && !valid[EXPANDABLE_HERE_STRING_START])
    return false;
  // These zero-width endings let the grammar stop a completed expression
  // without absorbing an adjacent operator into its enclosing command argument.
  if ((valid[MEMBER_ARGUMENT_END] || (valid[EXPRESSION_ARGUMENT_END] && !valid[ARGUMENT_END])) &&
      !valid[HERE_STRING_CONTENT]) {
    lexer->mark_end(lexer);
    int32_t c = lexer->lookahead;
    if (c == '.' || c == '[' || (valid[MEMBER_ARGUMENT_END] && c == '('))
      return false;
    if (c == ':') {
      lexer->advance(lexer, false);
      if (lexer->lookahead == ':')
        return false;
    }
    if (c == '?') {
      lexer->advance(lexer, false);
      if (lexer->lookahead == '.' || lexer->lookahead == '[')
        return false;
    }
    lexer->result_symbol =
        valid[MEMBER_ARGUMENT_END] ? MEMBER_ARGUMENT_END : EXPRESSION_ARGUMENT_END;
    return true;
  }
  if (valid[ARGUMENT_END] && !valid[HERE_STRING_CONTENT]) {
    lexer->mark_end(lexer);
    if (generic_end(lexer->lookahead) && !quote(lexer->lookahead)) {
      lexer->result_symbol = ARGUMENT_END;
      return true;
    }
    if (lexer->lookahead == '`') {
      lexer->advance(lexer, false);
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
  if (valid[ARGUMENT_END] && !valid[HERE_STRING_CONTENT])
    return false;
  while (horizontal(lexer->lookahead))
    lexer->advance(lexer, true);
  if (valid[ARGUMENT_SEPARATOR] && lexer->lookahead == ',') {
    lexer->advance(lexer, false);
    lexer->mark_end(lexer);
    lexer->result_symbol = ARGUMENT_SEPARATOR;
    return true;
  }
  if (valid[LINE_CONTINUATION] && !valid[COMMAND_NAME] && !valid[BARE_WORD] &&
      lexer->lookahead == '`') {
    lexer->advance(lexer, false);
    return scan_line_continuation_after_tick(lexer, valid);
  }
  if (valid[COMMENT] && (lexer->lookahead == '#' || lexer->lookahead == '<'))
    return scan_comment(lexer, valid);
  if (valid[FUNCTION_NAME] && !valid[COMMAND_NAME])
    return scan_function_name(lexer);
  if (valid[LABEL_START] && !valid[HERE_STRING_CONTENT] && lexer->lookahead == ':')
    return scan_label_start(lexer);
  if (valid[FLOW_LABEL] && !valid[HERE_STRING_CONTENT] && variable_character(lexer->lookahead) &&
      lexer->lookahead != '?' && !decimal_character(lexer->lookahead))
    return scan_flow_label(lexer);
  if (valid[VARIABLE] && !valid[COMMAND_NAME] && !valid[BARE_WORD] && lexer->lookahead == '$') {
    lexer->advance(lexer, false);
    return scan_variable_after_dollar(lexer, valid, VARIABLE);
  }
  if (lexer->lookahead == '@' &&
      (valid[SINGLE_QUOTED_HERE_STRING] || valid[EXPANDABLE_HERE_STRING_START] ||
       valid[SPLAT_EXPRESSION] || valid[BARE_WORD])) {
    lexer->advance(lexer, false);
    if (quote(lexer->lookahead))
      return scan_here_string_after_at(lexer, valid);
    return scan_splat_after_at(lexer, valid);
  }
  if (valid[COMMAND_NAME] || valid[BARE_WORD]) {
    // A failed generic scan cannot be rewound. Numeric starts are handled by
    // the ordinary lexer unless an argument-mode bare token is required.
    if (ascii_digit(lexer->lookahead) || lexer->lookahead == '.') {
      if (valid[BARE_WORD] || valid[COMMAND_NAME])
        return scan_generic(lexer, valid, false);
    } else
      return scan_generic(lexer, valid, false);
  }
  if (valid[INTEGER_LITERAL] || valid[REAL_LITERAL])
    return scan_number(lexer, valid);
  return false;
}
