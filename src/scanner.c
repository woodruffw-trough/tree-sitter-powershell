#include "tree_sitter/parser.h"
#include "variable_chars.h"
#include <assert.h>
#include <ctype.h>
#include <stdbool.h>
#include <stdlib.h>
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
  METHOD_INVOCATION_CONTEXT, // Reject trivia before a method's opening '(' or
                             // '{'.
  LINE_CONTINUATION,
  POSTFIX_CONTEXT, // Reject continuations before adjacent member/index access.
  ARGUMENT_SEPARATOR,
  SPLAT_EXPRESSION,
  VERBATIM_ARGUMENTS_START,
  VERBATIM_ARGUMENT_TEXT,
  BRACED_ARGUMENT_VARIABLE,
  FLOW_EXPRESSION_START,
  FLOW_LABEL_END,
  COMMAND_MEMBER_START,
  COMMAND_UNARY_START,
  DATA_COMMAND_NAME,
  SIGNED_NUMBER_START,
  CAST_VALUE_CONTEXT,
  SINGLE_QUOTED_STRING,
  ADJACENT_SINGLE_QUOTED_STRING,
  STRING_CONTENT,
  VARIABLE_ARGUMENT_FRAGMENT,
  GENERIC_ARGUMENT_FRAGMENT,
  DOUBLE_QUOTE,
  ADJACENT_DOUBLE_QUOTE,
  PREPARED_TOKEN,
  LEFT_PAREN,
  ADJACENT_LEFT_PAREN,
  RIGHT_PAREN,
  SUB_EXPRESSION_OPEN,
  ADJACENT_SUB_EXPRESSION_OPEN,
  SUB_EXPRESSION_END,
  ARRAY_EXPRESSION_OPEN,
  ASSEMBLY_NAME_CONTENT,
  LITERAL_PARAMETER_TOKEN,
  END_OF_PARAMETERS_TOKEN,
  ESCAPED_WHITESPACE,
  ARGUMENT_EXPRESSION_START,
  ATTRIBUTE_EXPRESSION_START,
  MEMBER_EXPRESSION_START,
  INDEX_EXPRESSION_START,
  CLAUSE_KEYWORD_END,
  SWITCH_REGEX_PARAMETER,
  SWITCH_WILDCARD_PARAMETER,
  SWITCH_EXACT_PARAMETER,
  SWITCH_CASESENSITIVE_PARAMETER,
  SWITCH_FILE_PARAMETER,
  DATA_SUPPORTED_COMMAND_PARAMETER,
  FOREACH_PARALLEL_PARAMETER,
  FOREACH_THROTTLELIMIT_PARAMETER,
  PARAM_BLOCK_KEYWORD,
  PARAM_COMMAND_NAME,
  REDIRECTION_COMMAND_LESS_THAN,
  INTERPOLATION_OPEN,
  INTERPOLATION_TAIL,
  COMMAND_INTERPOLATION_CONTEXT,
  EXTERNAL_TOKEN_COUNT
};

static bool prepared_context_token(TSSymbol symbol) {
  return symbol == SIGNED_NUMBER_START || symbol == ARGUMENT_END || symbol == LABEL_START;
}

// Offsets count included source codepoints from the preparation position. They
// remain valid across excluded ranges and never depend on physical row numbers.
typedef struct {
  bool active;
  uint32_t start;
  uint32_t end;
  TSSymbol symbol;
} PreparedToken;

typedef struct {
  uint32_t balance;
  uint32_t semantic_depth;
  bool ordinary;
  bool closed;
} Interpolation;

// A prepared token stores its active flag, two raw codepoint counts, and its
// symbol. Reserve the largest header before accepting another collector frame.
#define PREPARED_TOKEN_STATE_SIZE 11
#define INTERPOLATION_STATE_BUDGET                                                                 \
  (TREE_SITTER_SERIALIZATION_BUFFER_SIZE - PREPARED_TOKEN_STATE_SIZE)
// Each encoded run needs at least three bytes. The header takes additional
// space, so this backing array cannot fill before the serialization budget.
#define MAX_INTERPOLATION_RUNS (TREE_SITTER_SERIALIZATION_BUFFER_SIZE / 3)

typedef struct {
  uint32_t delta;
  uint32_t semantic_delta;
  uint32_t count;
  bool toggles_ordinary;
  bool toggles_closed;
} InterpolationRun;

typedef struct {
  uint32_t balance;
  uint32_t semantic_depth;
  uint32_t depth;
  uint32_t ordinary_layers;
  uint32_t run_count;
  Interpolation outermost;
  Interpolation top;
  InterpolationRun runs[MAX_INTERPOLATION_RUNS];
  PreparedToken prepared;
  bool collector_eof;
} Scanner;

static size_t unsigned_size(uint64_t value) {
  size_t size = 1;
  while (value >= 128) {
    value >>= 7;
    ++size;
  }
  return size;
}

static char *write_unsigned(char *buffer, uint64_t value) {
  while (value >= 128) {
    *buffer++ = (char)((value & 127) | 128);
    value >>= 7;
  }
  *buffer++ = (char)value;
  return buffer;
}

static bool read_unsigned(const char **cursor, const char *end, uint64_t *result) {
  uint64_t value = 0;
  unsigned shift = 0;
  while (*cursor < end && shift <= 63) {
    uint8_t byte = (uint8_t)*(*cursor)++;
    uint8_t payload = byte & 127;
    if (shift == 63 && payload > 1) {
      return false;
    }
    value |= (uint64_t)payload << shift;
    if (!(byte & 128)) {
      if (shift != 0 && payload == 0) {
        return false;
      }
      *result = value;
      return true;
    }
    shift += 7;
  }
  return false;
}

static void write_u32(char *buffer, uint32_t value) {
  for (unsigned i = 0; i < 4; ++i) {
    buffer[i] = (char)(value >> (i * 8));
  }
}

static uint32_t read_u32(const char *buffer) {
  uint32_t value = 0;
  for (unsigned i = 0; i < 4; ++i) {
    value |= (uint32_t)(uint8_t)buffer[i] << (i * 8);
  }
  return value;
}

static uint64_t encoded_delta(const InterpolationRun *run) {
  return (uint64_t)run->delta * 4 + (run->toggles_ordinary ? 2 : 0) + run->toggles_closed;
}

static size_t run_size(const InterpolationRun *run) {
  return unsigned_size(run->count) + unsigned_size(encoded_delta(run)) +
         unsigned_size(run->semantic_delta);
}

static size_t interpolation_state_size(const Scanner *scanner) {
  if (!scanner->depth) {
    return 0;
  }
  // Fixed-width current counters cannot grow the encoded state between
  // quoted interpolation starts and ends.
  size_t size = 8 + unsigned_size(scanner->outermost.balance) +
                unsigned_size(scanner->outermost.semantic_depth) + 1;
  for (uint32_t i = 0; i < scanner->run_count; ++i) {
    size += run_size(&scanner->runs[i]);
  }
  return size;
}

static Interpolation current_interpolation(const Scanner *scanner) {
  return scanner->top;
}

static uint32_t ordinary_interpolation_layers(const Scanner *scanner) {
  return scanner->ordinary_layers;
}

static bool push_interpolation(Scanner *scanner, Interpolation frame) {
  if (scanner->depth == UINT32_MAX || frame.balance == 0 || frame.semantic_depth == 0) {
    return false;
  }
  if (!scanner->depth) {
    size_t size = 8 + unsigned_size(frame.balance) + unsigned_size(frame.semantic_depth) + 1;
    if (size > INTERPOLATION_STATE_BUDGET) {
      return false;
    }
    scanner->outermost = frame;
    scanner->top = frame;
    scanner->depth = 1;
    scanner->ordinary_layers = frame.ordinary;
    scanner->run_count = 0;
    return true;
  }
  if (frame.balance <= scanner->top.balance ||
      frame.semantic_depth <= scanner->top.semantic_depth) {
    return false;
  }
  InterpolationRun next = {
      .delta = frame.balance - scanner->top.balance,
      .semantic_delta = frame.semantic_depth - scanner->top.semantic_depth,
      .count = 1,
      .toggles_ordinary = scanner->top.ordinary != frame.ordinary,
      .toggles_closed = scanner->top.closed != frame.closed,
  };
  InterpolationRun *last = scanner->run_count ? &scanner->runs[scanner->run_count - 1] : NULL;
  bool extends = last && last->delta == next.delta && last->semantic_delta == next.semantic_delta &&
                 last->toggles_ordinary == next.toggles_ordinary &&
                 last->toggles_closed == next.toggles_closed;
  size_t size = interpolation_state_size(scanner);
  if (extends) {
    size += unsigned_size(last->count + 1) - unsigned_size(last->count);
  } else {
    size += run_size(&next);
  }
  // Closing the top frame may split one run. Keep its maximum encoding free.
  if (size + 11 > INTERPOLATION_STATE_BUDGET ||
      (!extends && scanner->run_count == MAX_INTERPOLATION_RUNS)) {
    return false;
  }
  if (extends) {
    ++last->count;
  } else {
    scanner->runs[scanner->run_count++] = next;
  }
  scanner->top = frame;
  ++scanner->depth;
  scanner->ordinary_layers += frame.ordinary;
  return true;
}

static bool pop_interpolation(Scanner *scanner) {
  if (!scanner->depth) {
    return false;
  }
  scanner->ordinary_layers -= scanner->top.ordinary;
  if (scanner->depth == 1) {
    scanner->depth = 0;
    scanner->semantic_depth = 0;
    scanner->run_count = 0;
    return true;
  }
  InterpolationRun *last = &scanner->runs[scanner->run_count - 1];
  scanner->top.balance -= last->delta;
  scanner->top.semantic_depth -= last->semantic_delta;
  scanner->top.ordinary ^= last->toggles_ordinary;
  scanner->top.closed ^= last->toggles_closed;
  if (--last->count == 0) {
    --scanner->run_count;
  }
  --scanner->depth;
  return true;
}

static bool close_interpolation(Scanner *scanner) {
  if (!scanner->depth || scanner->top.closed) {
    return false;
  }
  if (scanner->depth == 1) {
    scanner->outermost.closed = true;
  } else {
    InterpolationRun *last = &scanner->runs[scanner->run_count - 1];
    if (last->count > 1) {
      if (scanner->run_count == MAX_INTERPOLATION_RUNS) {
        return false;
      }
      InterpolationRun tail = *last;
      tail.count = 1;
      tail.toggles_closed = !tail.toggles_closed;
      --last->count;
      scanner->runs[scanner->run_count++] = tail;
    } else {
      last->toggles_closed = !last->toggles_closed;
      if (scanner->run_count > 1) {
        InterpolationRun *previous = last - 1;
        if (previous->delta == last->delta && previous->semantic_delta == last->semantic_delta &&
            previous->toggles_ordinary == last->toggles_ordinary &&
            previous->toggles_closed == last->toggles_closed) {
          previous->count += last->count;
          --scanner->run_count;
        }
      }
    }
  }
  scanner->top.closed = true;
  return interpolation_state_size(scanner) <= INTERPOLATION_STATE_BUDGET;
}

typedef struct {
  TSLexer lexer;
  TSLexer *base;
  Scanner *scanner;
  uint32_t balance;
  uint32_t marked_balance;
  bool overflow;
} TrackedLexer;

static void tracked_advance(TSLexer *lexer, bool skip) {
  TrackedLexer *tracked = (TrackedLexer *)lexer;
  if (tracked->scanner->depth) {
    if (tracked->base->lookahead == '(') {
      if (tracked->balance == UINT32_MAX) {
        tracked->overflow = true;
      } else {
        ++tracked->balance;
      }
    } else if (tracked->base->lookahead == ')') {
      if (tracked->balance == 0) {
        tracked->overflow = true;
      } else {
        --tracked->balance;
      }
    }
  }
  tracked->base->advance(tracked->base, skip);
  lexer->lookahead = tracked->base->lookahead;
}

static void tracked_mark_end(TSLexer *lexer) {
  TrackedLexer *tracked = (TrackedLexer *)lexer;
  tracked->base->mark_end(tracked->base);
  tracked->marked_balance = tracked->balance;
}

static uint32_t tracked_column(TSLexer *lexer) {
  TrackedLexer *tracked = (TrackedLexer *)lexer;
  return tracked->base->get_column(tracked->base);
}

static bool tracked_range_start(const TSLexer *lexer) {
  const TrackedLexer *tracked = (const TrackedLexer *)lexer;
  return tracked->base->is_at_included_range_start(tracked->base);
}

static bool tracked_eof(const TSLexer *lexer) {
  const TrackedLexer *tracked = (const TrackedLexer *)lexer;
  if (tracked->scanner->depth && lexer->lookahead == ')' &&
      tracked->balance == current_interpolation(tracked->scanner).balance) {
    tracked->scanner->collector_eof = true;
    return true;
  }
  return tracked->base->eof(tracked->base);
}

void *tree_sitter_powershell_external_scanner_create(void) {
  return calloc(1, sizeof(Scanner));
}

void tree_sitter_powershell_external_scanner_destroy(void *payload) {
  free(payload);
}

static unsigned serialize_interpolations(Scanner *scanner, char *buffer) {
  if (!scanner->depth) {
    return 0;
  }
  write_u32(buffer, scanner->balance);
  write_u32(buffer + 4, scanner->semantic_depth);
  char *cursor = write_unsigned(buffer + 8, scanner->outermost.balance);
  cursor = write_unsigned(cursor, scanner->outermost.semantic_depth);
  *cursor++ = scanner->outermost.ordinary | (scanner->outermost.closed ? 2 : 0);
  for (uint32_t i = 0; i < scanner->run_count; ++i) {
    InterpolationRun *run = &scanner->runs[i];
    cursor = write_unsigned(cursor, run->count);
    cursor = write_unsigned(cursor, encoded_delta(run));
    cursor = write_unsigned(cursor, run->semantic_delta);
  }
  return (unsigned)(cursor - buffer);
}

static bool decode_interpolation_state(Scanner *scanner, const char *buffer, unsigned length) {
  if (length < 11 || length > INTERPOLATION_STATE_BUDGET) {
    return false;
  }
  const char *cursor = buffer + 8;
  const char *end = buffer + length;
  uint64_t balance;
  uint64_t semantic_depth;
  if (!read_unsigned(&cursor, end, &balance) || balance == 0 || balance > UINT32_MAX ||
      !read_unsigned(&cursor, end, &semantic_depth) || semantic_depth == 0 ||
      semantic_depth > UINT32_MAX || cursor == end || (uint8_t)*cursor > 3) {
    return false;
  }
  scanner->balance = read_u32(buffer);
  scanner->semantic_depth = read_u32(buffer + 4);
  scanner->outermost = (Interpolation){
      .balance = (uint32_t)balance,
      .semantic_depth = (uint32_t)semantic_depth,
      .ordinary = (uint8_t)*cursor & 1,
      .closed = (uint8_t)*cursor & 2,
  };
  ++cursor;
  scanner->top = scanner->outermost;
  scanner->depth = 1;
  scanner->ordinary_layers = scanner->outermost.ordinary;
  while (cursor < end) {
    uint64_t count;
    uint64_t packed_delta;
    uint64_t semantic_delta;
    if (scanner->run_count == MAX_INTERPOLATION_RUNS || !read_unsigned(&cursor, end, &count) ||
        count == 0 || count > UINT32_MAX - scanner->depth ||
        !read_unsigned(&cursor, end, &packed_delta) ||
        !read_unsigned(&cursor, end, &semantic_delta)) {
      return false;
    }
    uint64_t delta = packed_delta >> 2;
    bool toggles_ordinary = packed_delta & 2;
    bool toggles_closed = packed_delta & 1;
    if (delta == 0 || delta > UINT32_MAX || semantic_delta == 0 || semantic_delta > UINT32_MAX ||
        count > (UINT32_MAX - scanner->top.balance) / delta ||
        count > (UINT32_MAX - scanner->top.semantic_depth) / semantic_delta) {
      return false;
    }
    InterpolationRun run = {
        .delta = (uint32_t)delta,
        .semantic_delta = (uint32_t)semantic_delta,
        .count = (uint32_t)count,
        .toggles_ordinary = toggles_ordinary,
        .toggles_closed = toggles_closed,
    };
    if (scanner->run_count) {
      InterpolationRun *previous = &scanner->runs[scanner->run_count - 1];
      if (previous->delta == run.delta && previous->semantic_delta == run.semantic_delta &&
          previous->toggles_ordinary == run.toggles_ordinary &&
          previous->toggles_closed == run.toggles_closed) {
        return false;
      }
    }
    scanner->runs[scanner->run_count++] = run;
    scanner->top.balance += (uint32_t)(delta * count);
    scanner->top.semantic_depth += (uint32_t)(semantic_delta * count);
    scanner->depth += (uint32_t)count;
    scanner->top.closed ^= toggles_closed && (count & 1);
    if (toggles_ordinary) {
      scanner->ordinary_layers += (uint32_t)(count / 2);
      if ((count & 1) && !scanner->top.ordinary) {
        ++scanner->ordinary_layers;
      }
      scanner->top.ordinary ^= (count & 1) != 0;
    } else if (scanner->top.ordinary) {
      scanner->ordinary_layers += (uint32_t)count;
    }
  }
  return (scanner->balance >= scanner->top.balance ||
          (scanner->top.closed && scanner->balance + 1 == scanner->top.balance)) &&
         scanner->semantic_depth >= scanner->top.semantic_depth;
}

unsigned tree_sitter_powershell_external_scanner_serialize(void *payload, char *buffer) {
  Scanner *scanner = payload;
  if (!scanner->depth) {
    return 0;
  }
  size_t size = interpolation_state_size(scanner) +
                (scanner->prepared.active ? PREPARED_TOKEN_STATE_SIZE : 1);
  assert(size <= TREE_SITTER_SERIALIZATION_BUFFER_SIZE);
  (void)size;
  char *cursor = buffer;
  *cursor++ = scanner->prepared.active;
  if (scanner->prepared.active) {
    write_u32(cursor, scanner->prepared.start);
    cursor += 4;
    write_u32(cursor, scanner->prepared.end);
    cursor += 4;
    *cursor++ = (char)scanner->prepared.symbol;
    *cursor++ = (char)(scanner->prepared.symbol >> 8);
  }
  return (unsigned)(cursor - buffer) + serialize_interpolations(scanner, cursor);
}

void tree_sitter_powershell_external_scanner_deserialize(void *payload, const char *buffer,
                                                         unsigned length) {
  Scanner *scanner = payload;
  memset(scanner, 0, sizeof(*scanner));
  if (!length) {
    return;
  }
  uint8_t flags = (uint8_t)*buffer++;
  --length;
  if (flags > 1) {
    return;
  }
  if (flags) {
    if (length < 10) {
      return;
    }
    scanner->prepared.active = true;
    scanner->prepared.start = read_u32(buffer);
    scanner->prepared.end = read_u32(buffer + 4);
    scanner->prepared.symbol = (uint8_t)buffer[8] | (uint16_t)(uint8_t)buffer[9] << 8;
    buffer += 10;
    length -= 10;
    if ((scanner->prepared.start > scanner->prepared.end ||
         (scanner->prepared.start == scanner->prepared.end &&
          !prepared_context_token(scanner->prepared.symbol))) ||
        (scanner->prepared.symbol >= EXTERNAL_TOKEN_COUNT ||
         scanner->prepared.symbol == PREPARED_TOKEN)) {
      memset(scanner, 0, sizeof(*scanner));
      return;
    }
  }
  if (!decode_interpolation_state(scanner, buffer, length)) {
    memset(scanner, 0, sizeof(*scanner));
  }
}

static bool at_collector_end(TSLexer *lexer) {
  TrackedLexer *tracked = (TrackedLexer *)lexer;
  return lexer->eof(lexer) && tracked->scanner->collector_eof;
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
    if (*s == 'l' || *s == 's' || *s == 'y') {
      ++s;
    }
  } else if (*s == 'l' || *s == 's' || *s == 'y' || *s == 'n' || *s == 'd') {
    ++s;
  }
  if ((*s == 'k' || *s == 'm' || *s == 'g' || *s == 't' || *s == 'p') && s[1] == 'b') {
    s += 2;
  }
  return *s == 0;
}

static bool numeric_token(const char *s, enum TokenType *kind) {
  bool signed_number = *s == '+' || *s == '-';
  if (signed_number) {
    ++s;
  }
  const char *end = s;
  bool real = false;
  if (s[0] == '0' && s[1] == 'x') {
    end += 2;
    // PowerShell accepts the empty hexadecimal prefix as zero, including its
    // numeric suffixes. C's strtod would also admit unsupported hex floats.
    while (ascii_hex(*end)) {
      ++end;
    }
  } else if (s[0] == '0' && s[1] == 'b') {
    end += 2;
    if (*end != '0' && *end != '1' && !(signed_number && *end == 'u')) {
      return false;
    }
    while (*end == '0' || *end == '1') {
      ++end;
    }
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
    if (!digit) {
      return false;
    }
    if (*end == 'e') {
      real = true;
      ++end;
      if (*end == '+' || *end == '-') {
        ++end;
      }
      if (!ascii_digit(*end)) {
        return false;
      }
      while (ascii_digit(*end)) {
        ++end;
      }
    }
  }
  // Hexadecimal d/e are digits, not decimal suffixes or exponent markers.
  // Integral suffixes are also valid on decimal fractional/exponent forms.
  if (*end == 'd') {
    real = true;
  }
  if (!suffix_ok(end)) {
    return false;
  }
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
  for (unsigned i = 0; i < sizeof(words) / sizeof(words[0]); ++i) {
    if (strcmp(s, words[i]) == 0) {
      return true;
    }
  }
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

// Match GetVerbatimCommandArgument in PowerShell 7.5.4's tokenizer.cs. Only
// double quotes change the boundary rules; backticks and single quotes are
// text.
static bool scan_verbatim_argument_text(TSLexer *lexer) {
  while (horizontal(lexer->lookahead)) {
    lexer->advance(lexer, true);
  }
  bool content = false;
  bool in_quotes = false;
  while ((!lexer->eof(lexer) || at_collector_end(lexer)) && !newline(lexer->lookahead)) {
    // Verbatim tails include closing parentheses. Consuming the collector's
    // physical close here leaves the required syntactic close missing.
    if (at_collector_end(lexer)) {
      lexer->advance(lexer, false);
      lexer->mark_end(lexer);
      lexer->result_symbol = VERBATIM_ARGUMENT_TEXT;
      return true;
    }
    int32_t c = lexer->lookahead;
    if (!in_quotes && c == '|') {
      break;
    }
    if (!in_quotes && c == '&') {
      // Preserve a lone ampersand while leaving both bytes of && for the
      // ordinary lexer. mark_end still points before the first ampersand.
      lexer->advance(lexer, false);
      if (lexer->lookahead == '&') {
        break;
      }
    } else {
      if (double_quote(c)) {
        in_quotes = !in_quotes;
      }
      lexer->advance(lexer, false);
    }
    content = true;
    lexer->mark_end(lexer);
  }
  if (!content) {
    return false;
  }
  lexer->result_symbol = VERBATIM_ARGUMENT_TEXT;
  return true;
}
// Comments remain named extras. The method context is a lookahead sentinel
// that the grammar never needs to consume: a block comment may precede member
// access, but PowerShell forbids one between a method name and its argument
// opener.
static bool scan_comment(TSLexer *lexer, const bool *valid) {
  if (lexer->lookahead == '#') {
    do {
      lexer->advance(lexer, false);
    } while (!lexer->eof(lexer) && !newline(lexer->lookahead));
    if (at_collector_end(lexer)) {
      return false;
    }
    lexer->mark_end(lexer);
    lexer->result_symbol = COMMENT;
    return true;
  }
  if (lexer->lookahead != '<') {
    return false;
  }
  lexer->advance(lexer, false);
  if (lexer->lookahead != '#') {
    if (!valid[REDIRECTION_COMMAND_LESS_THAN]) {
      return false;
    }
    lexer->mark_end(lexer);
    lexer->result_symbol = REDIRECTION_COMMAND_LESS_THAN;
    return true;
  }
  // Once '<#' begins a comment, an unterminated body cannot become a
  // command named '<' followed by a line comment.
  lexer->advance(lexer, false);
  while (!lexer->eof(lexer)) {
    if (lexer->lookahead == '#') {
      lexer->advance(lexer, false);
      if (lexer->lookahead == '>') {
        lexer->advance(lexer, false);
        lexer->mark_end(lexer);
        if (valid[METHOD_INVOCATION_CONTEXT]) {
          while (horizontal(lexer->lookahead)) {
            lexer->advance(lexer, false);
          }
          if (lexer->lookahead == '(' || lexer->lookahead == '{') {
            return false;
          }
        }
        lexer->result_symbol = COMMENT;
        return true;
      }
    } else {
      lexer->advance(lexer, false);
    }
  }
  return false;
}

static void consume_newline(TSLexer *lexer) {
  bool carriage_return = lexer->lookahead == '\r';
  lexer->advance(lexer, false);
  if (carriage_return && lexer->lookahead == '\n') {
    lexer->advance(lexer, false);
  }
}

static bool scan_here_string_after_at(TSLexer *lexer, const bool *valid) {
  bool single = single_quote(lexer->lookahead);
  if (!single && !double_quote(lexer->lookahead)) {
    return false;
  }
  if (!(single ? valid[SINGLE_QUOTED_HERE_STRING] : valid[EXPANDABLE_HERE_STRING_START])) {
    return false;
  }
  lexer->advance(lexer, false);
  while (here_header_space(lexer->lookahead)) {
    lexer->advance(lexer, false);
  }
  if (!newline(lexer->lookahead)) {
    return false;
  }
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
  if (!valid[LINE_CONTINUATION] || !newline(lexer->lookahead)) {
    return false;
  }
  consume_newline(lexer);
  lexer->mark_end(lexer);
  if (valid[POSTFIX_CONTEXT] || valid[METHOD_INVOCATION_CONTEXT]) {
    while (horizontal(lexer->lookahead)) {
      lexer->advance(lexer, false);
    }
    int32_t c = lexer->lookahead;
    if (c == '.' || c == '[' || c == '(' || (c == '{' && valid[METHOD_INVOCATION_CONTEXT])) {
      return false;
    }
    if (c == '?') {
      lexer->advance(lexer, false);
      if (lexer->lookahead == '.' || lexer->lookahead == '[') {
        return false;
      }
    }
  }
  lexer->result_symbol = LINE_CONTINUATION;
  return true;
}

static bool variable_character(int32_t c) {
  if (c == '_' || c == '?') {
    return true;
  }
  if (c < 0 || c > 0xffff) {
    return false;
  }
  unsigned left = 0,
           right = sizeof(VARIABLE_CHARACTER_RANGES) / sizeof(VARIABLE_CHARACTER_RANGES[0]);
  while (left < right) {
    unsigned middle = left + (right - left) / 2;
    if (c < VARIABLE_CHARACTER_RANGES[middle][0]) {
      right = middle;
    } else if (c > VARIABLE_CHARACTER_RANGES[middle][1]) {
      left = middle + 1;
    } else {
      return true;
    }
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
    if (c >= starts[i] && c < starts[i] + 10) {
      return true;
    }
  }
  return false;
}

static bool scan_flow_label(TSLexer *lexer) {
  int32_t c = lexer->lookahead;
  if (!variable_character(c) || c == '?' || decimal_character(c)) {
    return false;
  }
  do {
    lexer->advance(lexer, false);
    c = lexer->lookahead;
  } while (variable_character(c) && c != '?');
  lexer->mark_end(lexer);
  lexer->result_symbol = FLOW_LABEL;
  return true;
}

// Peek past trivia without moving the zero-width label-start marker. A label
// is only a label when PowerShell's next token begins a loop or switch.
static bool label_statement_follows(TSLexer *lexer) {
  while (!lexer->eof(lexer)) {
    if (horizontal(lexer->lookahead) || newline(lexer->lookahead)) {
      lexer->advance(lexer, false);
    } else if (lexer->lookahead == '#') {
      while (!lexer->eof(lexer) && !newline(lexer->lookahead)) {
        lexer->advance(lexer, false);
      }
    } else if (lexer->lookahead == '<') {
      lexer->advance(lexer, false);
      if (lexer->lookahead != '#') {
        return false;
      }
      lexer->advance(lexer, false);
      bool closed = false;
      while (!lexer->eof(lexer)) {
        int32_t c = lexer->lookahead;
        lexer->advance(lexer, false);
        if (c == '#' && lexer->lookahead == '>') {
          lexer->advance(lexer, false);
          closed = true;
          break;
        }
      }
      if (!closed) {
        return false;
      }
    } else if (lexer->lookahead == '`') {
      lexer->advance(lexer, false);
      if (!newline(lexer->lookahead)) {
        return false;
      }
      consume_newline(lexer);
    } else {
      break;
    }
  }
  char word[8] = {0};
  size_t length = 0;
  while (lexer->lookahead >= 'A' && lexer->lookahead <= 'z') {
    if (length + 1 == sizeof(word)) {
      return false;
    }
    word[length++] = (char)ascii_lower(lexer->lookahead);
    lexer->advance(lexer, false);
  }
  return generic_end(lexer->lookahead) && !quote(lexer->lookahead) && lexer->lookahead != '<' &&
         lexer->lookahead != '>' &&
         (strcmp(word, "for") == 0 || strcmp(word, "foreach") == 0 || strcmp(word, "while") == 0 ||
          strcmp(word, "do") == 0 || strcmp(word, "switch") == 0);
}

static bool scan_label_start(TSLexer *lexer) {
  lexer->mark_end(lexer);
  lexer->advance(lexer, false); // Colon, followed by an entire simple name.
  int32_t c = lexer->lookahead;
  if (!variable_character(c) || c == '?' || decimal_character(c)) {
    return false;
  }
  do {
    lexer->advance(lexer, false);
    c = lexer->lookahead;
  } while (variable_character(c) && c != '?');
  if (!generic_end(c) || !label_statement_follows(lexer)) {
    return false;
  }
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
  if (physical_newline) {
    *physical_newline = false;
  }
  if (lexer->eof(lexer)) {
    return false;
  }
  int32_t c = lexer->lookahead;
  if (physical_newline) {
    *physical_newline = c == '\r' || c == '\n';
  }
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
  if (lexer->lookahead != '{') {
    return false;
  }
  lexer->advance(lexer, false);
  uint32_t value = 0;
  unsigned digits = 0;
  while (ascii_hex(lexer->lookahead)) {
    if (++digits > 6) {
      return false;
    }
    c = ascii_lower(lexer->lookahead);
    value = value * 16 + (uint32_t)(ascii_digit(c) ? c - '0' : c - 'a' + 10);
    lexer->advance(lexer, false);
  }
  if (digits == 0 || value > 0x10ffff || lexer->lookahead != '}') {
    return false;
  }
  lexer->advance(lexer, false);
  *decoded = (int32_t)value;
  return true;
}

static bool scan_backtick_escape(TSLexer *lexer, int32_t *decoded, bool *physical_newline) {
  if (lexer->lookahead != '`') {
    return false;
  }
  lexer->advance(lexer, false);
  return scan_backtick_escape_after_tick(lexer, decoded, physical_newline);
}

// The caller has consumed '$'. Scope/drive qualification requires a name
// after the first colon, even inside braced and expandable-string variables.
static bool scan_variable_after_dollar(TSLexer *lexer, const bool *valid, enum TokenType kind) {
  if (!valid[kind]) {
    return false;
  }
  int32_t first = lexer->lookahead;
  if (first == '?' || first == '$' || first == '^') {
    lexer->advance(lexer, false);
  } else if (first == '{') {
    lexer->advance(lexer, false);
    unsigned length = 0, colon = 0;
    bool qualified = false;
    while (!lexer->eof(lexer) && lexer->lookahead != '}') {
      int32_t c = lexer->lookahead;
      if (c == '{') {
        return false;
      }
      if (c == '`') {
        if (!scan_backtick_escape(lexer, &c, NULL)) {
          return false;
        }
      } else {
        lexer->advance(lexer, false);
      }
      ++length;
      if (c == ':' && !qualified) {
        qualified = true;
        colon = length;
      }
    }
    if (lexer->lookahead != '}' || length == 0 || (qualified && colon == length)) {
      return false;
    }
    lexer->advance(lexer, false);
  } else {
    if (!variable_character(first) && first != ':') {
      return false;
    }
    unsigned length = 0, colon = 0;
    bool qualified = false;
    while (variable_character(lexer->lookahead) || lexer->lookahead == ':') {
      int32_t c = lexer->lookahead;
      if (c == ':') {
        lexer->mark_end(lexer);
        lexer->advance(lexer, false);
        // A double colon after an existing name begins static member access.
        if (length != 0 && lexer->lookahead == ':') {
          if (qualified && colon == length) {
            return false;
          }
          lexer->result_symbol = kind;
          return true;
        }
      } else {
        lexer->advance(lexer, false);
      }
      ++length;
      if (c == ':' && !qualified) {
        qualified = true;
        colon = length;
      }
    }
    if (qualified && colon == length) {
      return false;
    }
  }
  lexer->mark_end(lexer);
  if (kind == BRACED_ARGUMENT_VARIABLE && lexer->lookahead != '=') {
    if (!valid[VARIABLE]) {
      return false;
    }
    kind = VARIABLE;
  }
  lexer->result_symbol = kind;
  return true;
}

static bool command_mode_number_follows(TSLexer *lexer, int32_t sign);

// LabelOrKeyRule first peeks in command mode, before UnaryExpressionRule can
// enable signed numbers. Glued operator/variable fragments are generic text in
// that mode, so they begin the next statement rather than becoming a label.
static bool command_mode_expression_follows(TSLexer *lexer, bool allow_signed_numbers,
                                            bool allow_comma) {
  while (!lexer->eof(lexer)) {
    if (horizontal(lexer->lookahead)) {
      lexer->advance(lexer, false);
    } else if (lexer->lookahead == '<') {
      lexer->advance(lexer, false);
      if (lexer->lookahead != '#') {
        return false;
      }
      lexer->advance(lexer, false);
      bool closed = false;
      while (!lexer->eof(lexer)) {
        int32_t c = lexer->lookahead;
        lexer->advance(lexer, false);
        if (c == '#' && lexer->lookahead == '>') {
          lexer->advance(lexer, false);
          closed = true;
          break;
        }
      }
      if (!closed) {
        return false;
      }
    } else if (lexer->lookahead == '`') {
      lexer->advance(lexer, false);
      if (!newline(lexer->lookahead)) {
        return false;
      }
      consume_newline(lexer);
    } else {
      break;
    }
  }
  int32_t c = lexer->lookahead;
  if (!allow_signed_numbers && variable_character(c) && c != '?' && !decimal_character(c)) {
    return true;
  }
  if (c == ',') {
    return allow_comma;
  }
  if (c == '+' || c == '-' || c == 0x2013 || c == 0x2014 || c == 0x2015 || c == '!') {
    lexer->advance(lexer, false);
    if (allow_signed_numbers && c != '!' &&
        (ascii_digit(lexer->lookahead) || lexer->lookahead == '.')) {
      return command_mode_number_follows(lexer, c == '+' ? '+' : '-');
    }
    if (c != '!' && (lexer->lookahead == c ||
                     (c != '+' && (lexer->lookahead == '-' || lexer->lookahead == 0x2013 ||
                                   lexer->lookahead == 0x2014 || lexer->lookahead == 0x2015)))) {
      lexer->advance(lexer, false);
    }
    return generic_end(lexer->lookahead) && !quote(lexer->lookahead);
  }
  if (c == '[') {
    lexer->advance(lexer, false);
    return generic_end(lexer->lookahead) && !quote(lexer->lookahead);
  }
  if (c == '$') {
    lexer->advance(lexer, false);
    c = lexer->lookahead;
    if (c == '(') {
      return true;
    }
    if (c == '{') {
      lexer->advance(lexer, false);
      while (!lexer->eof(lexer) && lexer->lookahead != '}') {
        if (lexer->lookahead == '`') {
          lexer->advance(lexer, false);
          if (lexer->eof(lexer)) {
            return false;
          }
        }
        lexer->advance(lexer, false);
      }
      if (lexer->lookahead != '}') {
        return false;
      }
      lexer->advance(lexer, false);
    } else if (c == '$' || c == '?' || c == '^') {
      lexer->advance(lexer, false);
    } else {
      while (variable_character(lexer->lookahead) || lexer->lookahead == ':') {
        c = lexer->lookahead;
        lexer->advance(lexer, false);
        if (c == ':' && lexer->lookahead == ':') {
          return true;
        }
      }
    }
    c = lexer->lookahead;
    return (generic_end(c) && !quote(c) && c != '<' && c != '>') || c == '.' || c == '[' ||
           c == '=';
  }
  if (c == '@') {
    lexer->advance(lexer, false);
    return lexer->lookahead == '(' || lexer->lookahead == '{' || quote(lexer->lookahead);
  }
  if (ascii_digit(c) || c == '.') {
    return command_mode_number_follows(lexer, 0);
  }
  return c == '(' || c == '{' || quote(c);
}

// The caller consumed '@'. A splat uses the same UTF-16 name characters as
// a variable. In argument mode an adjacent non-name character instead turns
// the entire token into generic text (for example @args/path or @args😀).
static bool scan_splat_after_at(TSLexer *lexer, const bool *valid) {
  int32_t first = lexer->lookahead;
  bool special = first == '?' || first == '$' || first == '^';
  if (!variable_character(first) && first != ':' && !special) {
    return false;
  }
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
    if (!generic && !special && (c == '.' || c == '[' || c == '=')) {
      break;
    }
    if (generic_end(c) && c != '<' && c != '>') {
      break;
    }
    if (!generic && !special && c == ':') {
      lexer->mark_end(lexer);
      lexer->advance(lexer, false);
      if (length != 0 && lexer->lookahead == ':') {
        if (!valid[SPLAT_EXPRESSION] || (qualified && colon == length)) {
          return false;
        }
        lexer->result_symbol = SPLAT_EXPRESSION;
        return true;
      }
    } else if (c == '`') {
      lexer->advance(lexer, false);
      generic = true;
      if (lexer->eof(lexer)) {
        if (at_collector_end(lexer)) {
          return false;
        }
        break;
      }
      bool physical_newline = false;
      if (!scan_backtick_escape_after_tick(lexer, &c, &physical_newline)) {
        return false;
      }
      if (physical_newline) {
        continue;
      }
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
  if (length == 0) {
    return false;
  }
  if (!generic && qualified && colon == length) {
    return false;
  }
  enum TokenType kind = generic ? BARE_WORD : SPLAT_EXPRESSION;
  if (!valid[kind]) {
    return false;
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
  if (lexer->lookahead == '(' || !valid[LITERAL_DOLLAR]) {
    return false;
  }
  lexer->mark_end(lexer);
  lexer->result_symbol = LITERAL_DOLLAR;
  return true;
}

// A quoted piece in a literal name stays within the same token. Single and
// double delimiters may be doubled; only double-quoted pieces process escapes.
static bool scan_name_quoted_piece(TSLexer *lexer) {
  bool single = single_quote(lexer->lookahead);
  lexer->advance(lexer, false);
  while (!lexer->eof(lexer)) {
    int32_t c = lexer->lookahead;
    if (single ? single_quote(c) : double_quote(c)) {
      lexer->advance(lexer, false);
      if (single ? single_quote(lexer->lookahead) : double_quote(lexer->lookahead)) {
        lexer->advance(lexer, false);
        continue;
      }
      return true;
    }
    if (!single && c == '$') {
      lexer->advance(lexer, false);
      if (lexer->lookahead == '(') {
        // ScanSubExpression counts physical parentheses even inside its
        // quotes. Its contents are reparsed later by PowerShell's parser.
        unsigned depth = 0;
        do {
          c = lexer->lookahead;
          if (c == '(') {
            ++depth;
          } else if (c == ')') {
            --depth;
          }
          lexer->advance(lexer, false);
        } while (depth != 0 && !lexer->eof(lexer));
        if (depth != 0) {
          return false;
        }
      } else if (lexer->lookahead == '{') {
        // A braced variable's name can contain quotes and spaces. Those
        // characters cannot close the enclosing quoted name piece.
        lexer->advance(lexer, false);
        while (!lexer->eof(lexer) && lexer->lookahead != '}') {
          if (lexer->lookahead == '`') {
            int32_t decoded;
            if (!scan_backtick_escape(lexer, &decoded, NULL)) {
              return false;
            }
          } else {
            lexer->advance(lexer, false);
          }
        }
        if (lexer->lookahead != '}') {
          return false;
        }
        lexer->advance(lexer, false);
      }
      continue;
    }
    if (!single && c == '`') {
      int32_t decoded;
      if (!scan_backtick_escape(lexer, &decoded, NULL)) {
        return false;
      }
    } else {
      lexer->advance(lexer, false);
    }
  }
  return false;
}

static bool scan_escaped_whitespace_after_tick(TSLexer *lexer) {
  if (!horizontal(lexer->lookahead)) {
    return false;
  }
  do {
    lexer->advance(lexer, false);
  } while (horizontal(lexer->lookahead));
  lexer->mark_end(lexer);
  lexer->result_symbol = ESCAPED_WHITESPACE;
  return true;
}

// Function names are literal generic tokens. Quoted pieces and escaped
// characters can occur after an unquoted prefix, and embedded dollar
// subexpressions remain part of the name rather than evaluated children.
static bool scan_function_name(TSLexer *lexer, const bool *valid) {
  int32_t c = lexer->lookahead;
  if (lexer->eof(lexer) || (generic_end(c) && c != 0) || c == '$' || c == '@' || c == '#') {
    return false;
  }
  // NextToken passes one non-EOF NUL into ScanGenericToken as its first
  // character. Another NUL later in the name still starts a separate token.
  if (c == 0) {
    lexer->advance(lexer, false);
  } else if (c == '`') {
    lexer->advance(lexer, false);
    if (newline(lexer->lookahead)) {
      return scan_line_continuation_after_tick(lexer, valid);
    }
    if (horizontal(lexer->lookahead)) {
      return scan_escaped_whitespace_after_tick(lexer);
    }
    int32_t decoded;
    if (!scan_backtick_escape_after_tick(lexer, &decoded, NULL)) {
      return false;
    }
  }
  bool previous_dollar = false;
  while (!lexer->eof(lexer)) {
    c = lexer->lookahead;
    if (c == '`') {
      int32_t decoded;
      bool physical_newline;
      if (!scan_backtick_escape(lexer, &decoded, &physical_newline)) {
        if (!lexer->eof(lexer)) {
          return false;
        }
      }
      previous_dollar = false;
      continue;
    }
    if (single_quote(c) || double_quote(c)) {
      if (!scan_name_quoted_piece(lexer)) {
        return false;
      }
      previous_dollar = false;
      continue;
    }
    if (c == '(' && previous_dollar) {
      unsigned depth = 0;
      do {
        c = lexer->lookahead;
        if (c == '(') {
          ++depth;
        } else if (c == ')') {
          --depth;
        }
        lexer->advance(lexer, false);
      } while (depth != 0 && !lexer->eof(lexer));
      if (depth != 0) {
        return false;
      }
      previous_dollar = false;
      continue;
    }
    if (generic_end(c) && c != '<' && c != '>') {
      break;
    }
    previous_dollar = c == '$';
    lexer->advance(lexer, false);
  }
  lexer->mark_end(lexer);
  lexer->result_symbol = FUNCTION_NAME;
  return true;
}

static bool scan_string_escape(TSLexer *lexer) {
  if (lexer->lookahead != '`') {
    return false;
  }
  // PowerShell escapes one character. For a CRLF pair this consumes only CR;
  // the remaining LF may start a here-string footer.
  int32_t decoded;
  if (!scan_backtick_escape(lexer, &decoded, NULL)) {
    return false;
  }
  lexer->mark_end(lexer);
  lexer->result_symbol = STRING_ESCAPE;
  return true;
}

static bool scan_here_string_content(TSLexer *lexer, const bool *valid) {
  bool content = false;
  while (!lexer->eof(lexer)) {
    int32_t c = lexer->lookahead;
    if (c == '`') {
      break;
    }
    if (c == '$') {
      lexer->advance(lexer, false);
      c = lexer->lookahead;
      if (variable_character(c) || c == '{' || c == '(' || c == '$' || c == '^' || c == ':') {
        // Return any preceding text, leaving the interpolation for the grammar.
        // With no preceding text, a failed external scan rewinds to the dollar.
        if (!content || !valid[HERE_STRING_CONTENT]) {
          return false;
        }
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
          if (!valid[HERE_STRING_END]) {
            return false;
          }
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
  if (!content || !valid[HERE_STRING_CONTENT]) {
    return false;
  }
  lexer->result_symbol = HERE_STRING_CONTENT;
  return true;
}

// PowerShell's tokenizer switches to argument mode after the command name.
// This token preserves punctuation in arguments (2+2 is text, not arithmetic)
// while stopping at interpolation and quoted pieces, represented by grammar
// rules.
typedef struct {
  char *data;
  size_t length;
  size_t capacity;
  char inline_data[128];
} GenericWord;

static bool generic_dash(int32_t c) {
  return c == '-' || c == 0x2013 || c == 0x2014 || c == 0x2015;
}

static bool generic_word_append(GenericWord *word, int32_t c) {
  if (word->length + 1 >= word->capacity) {
    if (word->capacity > SIZE_MAX / 2) {
      return false;
    }
    size_t capacity = word->capacity * 2;
    char *data;
    if (word->data == word->inline_data) {
      data = malloc(capacity);
      if (data != NULL) {
        memcpy(data, word->data, word->length + 1);
      }
    } else {
      data = realloc(word->data, capacity);
    }
    if (data == NULL) {
      return false;
    }
    word->data = data;
    word->capacity = capacity;
  }
  // Number scanning accepts smart dashes in exponent signs and after a numeric
  // prefix. Fold them only in numeric-looking words: generic text such as
  // -\u2014% must not become the ASCII-only verbatim-argument marker.
  bool numeric_start =
      word->length > 0 && (ascii_digit(word->data[0]) || word->data[0] == '.' ||
                           ((word->data[0] == '+' || word->data[0] == '-') && word->length > 1 &&
                            (ascii_digit(word->data[1]) || word->data[1] == '.')));
  if (generic_dash(c) && numeric_start) {
    c = '-';
  }
  // Classification otherwise needs folded ASCII. Keep non-ASCII and cooked NUL
  // visibly nonnumeric without inserting an embedded C-string terminator.
  word->data[word->length++] = c > 0 && c < 128 ? (char)ascii_lower(c) : '?';
  word->data[word->length] = 0;
  return true;
}

// A numeric prefix becomes generic text when the command-mode token continues
// into a suffix or interpolation. Look ahead without changing the marked end:
// callers emit a zero-width context token before the ordinary number lexer.
static bool command_mode_number_follows(TSLexer *lexer, int32_t sign) {
  GenericWord word = {0};
  word.data = word.inline_data;
  word.capacity = sizeof(word.inline_data);
  bool complete = sign == 0 || generic_word_append(&word, sign);
  while (complete && !lexer->eof(lexer)) {
    int32_t c = lexer->lookahead;
    if (generic_end(c) && c != '<' && c != '>') {
      break;
    }
    complete = generic_word_append(&word, c);
    lexer->advance(lexer, false);
  }
  bool result = complete && !quote(lexer->lookahead) && numeric_word(word.data);
  if (word.data != word.inline_data) {
    free(word.data);
  }
  return result;
}

static bool generic_numeric_pending(GenericWord *word) {
  size_t n = word->length;
  if ((n == 1 && word->data[0] == '.') ||
      (n == 2 && (word->data[0] == '+' || word->data[0] == '-') && word->data[1] == '.') ||
      (n == 2 && word->data[0] == '0' && word->data[1] == 'b')) {
    return true;
  }
  size_t pending = 0;
  char last = word->data[n - 1];
  if (last == 'e' || strchr("kmgtp", last) != NULL) {
    pending = 1;
  } else if (n > 1 && (last == '+' || last == '-') && word->data[n - 2] == 'e') {
    pending = 2;
  }
  if (pending == 0 || pending >= n) {
    return false;
  }
  char saved = word->data[n - pending];
  word->data[n - pending] = 0;
  bool possible = numeric_word(word->data);
  word->data[n - pending] = saved;
  return possible;
}

static bool generic_numeric_boundary(int32_t c) {
  return generic_dash(c) || (c > 0 && c < 128 && strchr("+*/%.<>#", (char)c) != NULL);
}

static bool generic_unary_operator(const char *word) {
  return strcmp(word, "-") == 0 || strcmp(word, "--") == 0 || strcmp(word, "-not") == 0 ||
         strcmp(word, "-bnot") == 0 || strcmp(word, "-split") == 0 || strcmp(word, "-join") == 0;
}

// ParamBlockRule commits when the next token is '(', even when the
// parameter list itself is invalid. Its fallback to a command named param
// applies only when no list begins after intervening trivia and newlines.
static bool parameter_list_follows(TSLexer *lexer) {
  while (!lexer->eof(lexer)) {
    if (horizontal(lexer->lookahead) || newline(lexer->lookahead)) {
      lexer->advance(lexer, false);
    } else if (lexer->lookahead == '#') {
      while (!lexer->eof(lexer) && !newline(lexer->lookahead)) {
        lexer->advance(lexer, false);
      }
    } else if (lexer->lookahead == '<') {
      lexer->advance(lexer, false);
      if (lexer->lookahead != '#') {
        return false;
      }
      lexer->advance(lexer, false);
      bool closed = false;
      while (!lexer->eof(lexer)) {
        int32_t c = lexer->lookahead;
        lexer->advance(lexer, false);
        if (c == '#' && lexer->lookahead == '>') {
          lexer->advance(lexer, false);
          closed = true;
          break;
        }
      }
      if (!closed) {
        return false;
      }
    } else if (lexer->lookahead == '`') {
      lexer->advance(lexer, false);
      if (!horizontal(lexer->lookahead) && !newline(lexer->lookahead)) {
        return false;
      }
      lexer->advance(lexer, false);
    } else {
      break;
    }
  }
  return lexer->lookahead == '(';
}

static bool scan_generic_word(TSLexer *lexer, const bool *valid, bool immediate,
                              GenericWord *buffer, bool continuation) {
  bool data_name = valid[DATA_COMMAND_NAME] && !immediate;
  int32_t first = lexer->lookahead;
  bool signed_start = valid[SIGNED_NUMBER_START] && !immediate && !continuation && !data_name &&
                      (!valid[BARE_WORD] || valid[COMMAND_NAME]) &&
                      (generic_dash(first) || (first == '+' && !valid[CAST_VALUE_CONTEXT]));
  bool name =
      !immediate && (valid[COMMAND_NAME] || data_name || (signed_start && !valid[BARE_WORD]));
  enum TokenType kind = data_name ? DATA_COMMAND_NAME : name ? COMMAND_NAME : BARE_WORD;
  // Preserve the initial position when classifying an adjacent signed primary.
  // A failed probe must still be able to return a generic command name.
  if (signed_start) {
    lexer->mark_end(lexer);
  }
  if (!continuation && !data_name &&
      ((first == '+' && valid[CAST_VALUE_CONTEXT]) || (name && first == '!'))) {
    return false;
  }
  if (!continuation && !data_name && name && first == '+') {
    if (!generic_word_append(buffer, '+')) {
      return false;
    }
    lexer->advance(lexer, false);
    if (!ascii_digit(lexer->lookahead) && lexer->lookahead != '.') {
      return false;
    }
  }
  if (!continuation &&
      ((generic_end(first) && !(first == 0 && !lexer->eof(lexer))) || first == '#' ||
       first == '@' || (!immediate && !data_name && first == ':') ||
       ((first == '[' || first == ']') && name && !data_name))) {
    return false;
  }
  bool dollar_prefix = false;
  if (!continuation && first == '$') {
    lexer->advance(lexer, false);
    if (variable_start(lexer->lookahead)) {
      if (lexer->lookahead == '{' && valid[BRACED_ARGUMENT_VARIABLE]) {
        return scan_variable_after_dollar(lexer, valid, BRACED_ARGUMENT_VARIABLE);
      }
      return scan_variable_after_dollar(lexer, valid,
                                        valid[VARIABLE] ? VARIABLE : ADJACENT_VARIABLE);
    }
    if (lexer->lookahead == '(') {
      return false;
    }
    dollar_prefix = true;
  }
  bool leading_dash = !continuation && !immediate && generic_dash(first);
  if (leading_dash) {
    if (!generic_word_append(buffer, '-')) {
      return false;
    }
    lexer->advance(lexer, false);
    int32_t next = lexer->lookahead;
    if (name && !data_name &&
        !((next >= 'a' && next <= 'z') || (next >= 'A' && next <= 'Z') || decimal_character(next) ||
          next == '.')) {
      return false;
    }
    if (generic_dash(next)) {
      if (!generic_word_append(buffer, next)) {
        return false;
      }
      lexer->advance(lexer, false);
      int32_t following = lexer->lookahead;
      if (generic_end(following) && !quote(following) && following != '<' && following != '>') {
        if (!valid[END_OF_PARAMETERS_TOKEN]) {
          return false;
        }
        lexer->mark_end(lexer);
        lexer->result_symbol = END_OF_PARAMETERS_TOKEN;
        return true;
      }
    }
    if ((valid[PARAMETER_NAME] || valid[LITERAL_PARAMETER_TOKEN] || name) &&
        variable_character(next) && !decimal_character(next)) {
      // Parameter names are raw text: dollars, backticks, and comment markers
      // are literal. A quote changes the whole argument to a generic token.
      while (!lexer->eof(lexer)) {
        int32_t c = lexer->lookahead;
        // At a command's start, an adjacent nonletter still ends a unary
        // operator; parameter values retain their complete literal token.
        if (name && !data_name && buffer->length > 1 && generic_unary_operator(buffer->data) &&
            !((c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z'))) {
          return false;
        }
        if (quote(c)) {
          // Quoting changes a parameter token into one generic token. Names
          // consume its quoted pieces below; argument values retain fragments.
          if (name) {
            break;
          }
          lexer->mark_end(lexer);
          lexer->result_symbol = BARE_WORD;
          return valid[BARE_WORD];
        }
        if (c == '.' || c == '[' || c == ':' || (generic_end(c) && c != '<' && c != '>')) {
          break;
        }
        if (!generic_word_append(buffer, c)) {
          return false;
        }
        lexer->advance(lexer, false);
      }
      if (buffer->length <= 1) {
        return false;
      }
      if (!quote(lexer->lookahead)) {
        if (name && !data_name && generic_unary_operator(buffer->data)) {
          return false;
        }
        enum TokenType parameter_kind = LITERAL_PARAMETER_TOKEN;
        if (name) {
          parameter_kind = kind;
        } else if (valid[PARAMETER_NAME]) {
          parameter_kind = PARAMETER_NAME;
        }
        if (parameter_kind != PARAMETER_NAME && lexer->lookahead == ':') {
          lexer->advance(lexer, false);
        }
        lexer->mark_end(lexer);
        lexer->result_symbol = parameter_kind;
        return valid[parameter_kind];
      }
    }
  }
  if (!continuation && first == 0 && !lexer->eof(lexer)) {
    if (!generic_word_append(buffer, 0)) {
      return false;
    }
    lexer->advance(lexer, false);
  }
  if (dollar_prefix && !generic_word_append(buffer, '$')) {
    return false;
  }
  char *word = buffer->data;
  size_t n = buffer->length;
  bool any_escape = continuation;
  enum TokenType numeric_kind = INTEGER_LITERAL;
  bool numeric_start =
      !continuation && (ascii_digit(first) || first == '.' ||
                        ((leading_dash || first == '+') &&
                         (ascii_digit(lexer->lookahead) || lexer->lookahead == '.')));
  bool numeric_live = numeric_start;
  size_t numeric_prefix = 0;
  while (!lexer->eof(lexer)) {
    int32_t c = lexer->lookahead;
    if (name && !data_name && leading_dash && n > 1 && !any_escape &&
        generic_unary_operator(word) && !((c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z'))) {
      return false;
    }
    if (name && !data_name && n == 1 && word[0] == '.' && c == '$') {
      return false;
    }
    if (name && c == '(' && n > 0 && word[n - 1] == '$') {
      unsigned depth = 0;
      do {
        c = lexer->lookahead;
        if (c == '(') {
          ++depth;
        } else if (c == ')') {
          --depth;
        }
        if (!generic_word_append(buffer, c)) {
          return false;
        }
        word = buffer->data;
        n = buffer->length;
        lexer->advance(lexer, false);
      } while (depth > 0 && !lexer->eof(lexer));
      continue;
    }
    if (name && quote(c)) {
      if (!data_name && n == 1 && word[0] == '.') {
        return false;
      }
      if (!scan_name_quoted_piece(lexer) || !generic_word_append(buffer, '?')) {
        return false;
      }
      word = buffer->data;
      n = buffer->length;
      numeric_live = false;
      numeric_prefix = 0;
      continue;
    }
    if ((generic_end(c) && c != '<' && c != '>') || (!name && c == '$')) {
      break;
    }
    if (c == '<' || c == '>') {
      enum TokenType candidate_kind = numeric_kind;
      bool numeric = !any_escape && numeric_live && numeric_token(word, &candidate_kind);
      if (name && n > 0 && numeric && c == '>') {
        numeric_kind = candidate_kind;
        if (!signed_start) {
          lexer->mark_end(lexer);
        }
        lexer->advance(lexer, false);
        if (lexer->lookahead != '&') {
          lexer->result_symbol = signed_start ? SIGNED_NUMBER_START : numeric_kind;
          return signed_start || valid[numeric_kind];
        }
        if (!generic_word_append(buffer, '>') || !generic_word_append(buffer, '&')) {
          return false;
        }
        word = buffer->data;
        n = buffer->length;
        lexer->advance(lexer, false);
        numeric_start = false;
        numeric_live = false;
        numeric_prefix = 0;
        continue;
      }
      if (n == 0 || (name && numeric)) {
        break;
      }
      if (!name && c == '>' && !any_escape &&
          ((n == 1 && word[0] >= '1' && word[0] <= '6') || (n == 1 && word[0] == '*'))) {
        // Numbered and all-stream redirections are recognized only at the
        // start of a token. Other numeric-looking arguments keep '<' and '>'
        // as generic text, just like ordinary words do.
        return false;
      }
    }
    if (c == '`') {
      lexer->advance(lexer, false);
      if (lexer->eof(lexer)) {
        if (at_collector_end(lexer)) {
          return false;
        }
        // A trailing backtick belongs to a nonempty generic token; a lone
        // backtick is incomplete input.
        if (n == 0 || !generic_word_append(buffer, '`')) {
          return false;
        }
        word = buffer->data;
        n = buffer->length;
        any_escape = true;
        break;
      }
      bool physical_newline = false;
      if (n == 0 && newline(lexer->lookahead)) {
        return scan_line_continuation_after_tick(lexer, valid);
      }
      if (n == 0 && horizontal(lexer->lookahead)) {
        return scan_escaped_whitespace_after_tick(lexer);
      }
      if (!scan_backtick_escape_after_tick(lexer, &c, &physical_newline)) {
        return false;
      }
      any_escape = true;
      numeric_live = false;
      // Backtick escapes one physical character. For CRLF only CR is joined;
      // the LF remains a statement terminator. Cooked `n is token content.
      if (physical_newline) {
        continue;
      }
    } else {
      lexer->advance(lexer, false);
    }
    if (!generic_word_append(buffer, c)) {
      return false;
    }
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
        if (!signed_start) {
          lexer->mark_end(lexer);
        }
      } else {
        numeric_live = generic_numeric_pending(buffer);
      }
    }
  }
  if (n == 0) {
    return false;
  }
  if (!immediate && !name && valid[VERBATIM_ARGUMENTS_START] && (!leading_dash || first == '-') &&
      strcmp(word, "--%") == 0 && !quote(lexer->lookahead) && lexer->lookahead != '$') {
    // Recognize the complete cooked argument, including escaped spellings.
    // A following quote or interpolation belongs to the same argument. Smart
    // dashes are normalized for parameter scanning but cannot start this
    // marker.
    lexer->mark_end(lexer);
    lexer->result_symbol = VERBATIM_ARGUMENTS_START;
    return true;
  }
  if (!any_escape && valid[SWITCH_DEFAULT] && strcmp(word, "default") == 0) {
    lexer->mark_end(lexer);
    lexer->result_symbol = SWITCH_DEFAULT;
    return true;
  }
  if (!immediate) {
    enum TokenType full_kind = numeric_kind;
    if (!name && (quote(lexer->lookahead) || lexer->lookahead == '$')) {
      // Quoting and interpolation continue the same generic argument, even
      // when its unquoted prefix happens to be a valid numeric literal.
      lexer->mark_end(lexer);
      lexer->result_symbol = BARE_WORD;
      return valid[BARE_WORD];
    }
    bool empty_binary = n >= 3 && (word[0] == '+' || word[0] == '-') && word[1] == '0' &&
                        word[2] == 'b' && (n == 3 || (word[3] != '0' && word[3] != '1'));
    if (!name && empty_binary) {
      lexer->mark_end(lexer);
      lexer->result_symbol = BARE_WORD;
      return valid[BARE_WORD];
    }
    if (!name && (leading_dash || first == '+') && !any_escape && numeric_token(word, &full_kind)) {
      lexer->mark_end(lexer);
      lexer->result_symbol = BARE_WORD;
      return valid[BARE_WORD];
    }
    if (!any_escape && (!data_name || (!leading_dash && first != '+')) &&
        numeric_token(word, &full_kind)) {
      if (signed_start) {
        lexer->result_symbol = SIGNED_NUMBER_START;
        return true;
      }
      // Signed numbers retain the grammar's unary-expression structure.
      if (leading_dash) {
        return false;
      }
      if (!valid[full_kind]) {
        return false;
      }
      lexer->mark_end(lexer);
      lexer->result_symbol = full_kind;
      return true;
    }
    if (data_name && !any_escape && strcmp(word, "--") == 0) {
      return false;
    }
    if (!any_escape && strcmp(word, "param") == 0 &&
        (valid[PARAM_BLOCK_KEYWORD] || valid[PARAM_COMMAND_NAME])) {
      lexer->mark_end(lexer);
      enum TokenType parameter_kind = valid[PARAM_BLOCK_KEYWORD] && parameter_list_follows(lexer)
                                          ? PARAM_BLOCK_KEYWORD
                                          : PARAM_COMMAND_NAME;
      lexer->result_symbol = parameter_kind;
      return valid[parameter_kind];
    }
    if (name && !data_name && !any_escape &&
        (reserved(word) || strcmp(word, ".") == 0 || generic_unary_operator(word))) {
      return false;
    }
    // A prefix is recorded before any escape changes numeric classification.
    // Later escaping cannot remove an earlier numeric/operator boundary.
    if (name && !data_name && numeric_start && numeric_prefix != 0 && numeric_prefix < n &&
        strchr("+-*/%.#", word[numeric_prefix]) != NULL) {
      if (signed_start) {
        lexer->result_symbol = SIGNED_NUMBER_START;
        return true;
      }
      if (leading_dash) {
        return false;
      }
      if (!valid[numeric_kind]) {
        return false;
      }
      lexer->result_symbol = numeric_kind;
      return true;
    }
  }
  if (name && !data_name && !any_escape && (strcmp(word, "-0b") == 0 || strcmp(word, "+0b") == 0)) {
    return false;
  }
  lexer->mark_end(lexer);
  lexer->result_symbol = kind;
  return valid[kind];
}

static bool scan_generic(TSLexer *lexer, const bool *valid, bool immediate) {
  GenericWord word = {0};
  word.data = word.inline_data;
  word.capacity = sizeof(word.inline_data);
  bool result = scan_generic_word(lexer, valid, immediate, &word, false);
  if (word.data != word.inline_data) {
    free(word.data);
  }
  return result;
}

static bool scan_generic_after_prefix(TSLexer *lexer, const bool *valid, int32_t prefix) {
  GenericWord word = {0};
  word.data = word.inline_data;
  word.capacity = sizeof(word.inline_data);
  if (!generic_word_append(&word, prefix)) {
    return false;
  }
  bool result = scan_generic_word(lexer, valid, false, &word, true);
  if (word.data != word.inline_data) {
    free(word.data);
  }
  return result;
}

// In command-name mode a variable followed by generic text is a literal name.
// Keep standalone variables available to the primary-expression grammar.
static bool scan_data_variable_name_after_prefix(TSLexer *lexer, const bool *valid, bool splat) {
  int32_t first = lexer->lookahead;
  if (first == '(' || (splat && first == '{')) {
    return false;
  }
  if (splat && quote(first)) {
    return scan_here_string_after_at(lexer, valid);
  }
  if (splat && !variable_start(first)) {
    return false;
  }
  bool braced = !splat && first == '{';
  if (variable_start(first)) {
    if (!scan_variable_after_dollar(lexer, valid, splat ? SPLAT_EXPRESSION : VARIABLE)) {
      return false;
    }
    int32_t next = lexer->lookahead;
    if ((generic_end(next) && !quote(next) && next != '<' && next != '>') || next == '.' ||
        next == '[' || (!braced && next == '=')) {
      return true;
    }
  }
  // The consumed prefix guarantees a nonnumeric, nonkeyword generic name.
  // Its exact cooked value is immaterial to the remaining boundary scan.
  return scan_generic_after_prefix(lexer, valid, '?');
}

static bool scan_data_variable_name(TSLexer *lexer, const bool *valid) {
  bool splat = lexer->lookahead == '@';
  lexer->advance(lexer, false);
  return scan_data_variable_name_after_prefix(lexer, valid, splat);
}

typedef struct {
  bool end_on_ternary;
  bool mark_end;
  bool allow_empty_binary;
} NumberContext;

// ScanNumberHelper accepts an expression-mode number only when its suffix
// ends at a token boundary. A fresh argument does not yet force '?' or ':'
// to terminate a number; recursive unary/binary/ternary operands do.
static bool scan_number(TSLexer *lexer, const bool *valid, NumberContext context) {
  bool real = false, digit = false;
  int32_t c = lexer->lookahead;
  if (c == '.') {
    lexer->advance(lexer, false);
    if (!ascii_digit(lexer->lookahead)) {
      return false;
    }
    real = true;
  } else if (!ascii_digit(c)) {
    return false;
  }
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
      if (!digit && !hex && !(context.allow_empty_binary && ascii_lower(lexer->lookahead) == 'u')) {
        return false;
      }
      goto suffix;
    }
  }
  while (ascii_digit(lexer->lookahead)) {
    digit = true;
    lexer->advance(lexer, false);
  }
  if (!digit) {
    return false;
  }
  if (context.mark_end) {
    lexer->mark_end(lexer);
  }
  if (!real && lexer->lookahead == '.') {
    lexer->advance(lexer, false);
    if (lexer->lookahead == '.') {
      if (!valid[INTEGER_LITERAL]) {
        return false;
      }
      lexer->result_symbol = INTEGER_LITERAL;
      return true;
    }
    real = true;
    while (ascii_digit(lexer->lookahead)) {
      lexer->advance(lexer, false);
    }
  }
  if (lexer->lookahead == 'e' || lexer->lookahead == 'E') {
    lexer->advance(lexer, false);
    if (lexer->lookahead == '+' || generic_dash(lexer->lookahead)) {
      lexer->advance(lexer, false);
    }
    if (!ascii_digit(lexer->lookahead)) {
      return false;
    }
    while (ascii_digit(lexer->lookahead)) {
      lexer->advance(lexer, false);
    }
    real = true;
  }
suffix:
  c = ascii_lower(lexer->lookahead);
  if (c == 'u') {
    lexer->advance(lexer, false);
    c = ascii_lower(lexer->lookahead);
    if (c == 'l' || c == 's' || c == 'y') {
      lexer->advance(lexer, false);
    }
  } else if (c == 'l' || c == 's' || c == 'y' || c == 'n' || c == 'd') {
    if (c == 'd') {
      real = true;
    }
    lexer->advance(lexer, false);
  }
  c = ascii_lower(lexer->lookahead);
  if (c == 'k' || c == 'm' || c == 'g' || c == 't' || c == 'p') {
    lexer->advance(lexer, false);
    if (ascii_lower(lexer->lookahead) != 'b') {
      return false;
    }
    lexer->advance(lexer, false);
  }
  c = lexer->lookahead;
  bool boundary = (generic_end(c) && !quote(c)) || generic_dash(c) ||
                  (c > 0 && c < 128 && strchr("!#%*+./=]", (char)c) != NULL) ||
                  (context.end_on_ternary && (c == '?' || c == ':'));
  if (!boundary) {
    return false;
  }
  enum TokenType kind = real ? REAL_LITERAL : INTEGER_LITERAL;
  if (!valid[kind]) {
    return false;
  }
  if (context.mark_end) {
    lexer->mark_end(lexer);
  }
  lexer->result_symbol = kind;
  return true;
}

// Arguments and defaults begin with a token peek before recursive expression
// parsing. Preserve that initial boundary without consuming the source token.
static bool scan_expression_start(TSLexer *lexer, enum TokenType kind) {
  lexer->mark_end(lexer);
  int32_t first = lexer->lookahead;
  if (lexer->eof(lexer) || newline(first) || first == ')' || first == ']' || first == '}' ||
      (first == ',' && kind != MEMBER_EXPRESSION_START && kind != INDEX_EXPRESSION_START) ||
      first == ';') {
    return false;
  }
  // Attribute names use SimpleNameRule before trying an expression. Leave
  // the identifier branch available instead of committing to an expression.
  if (kind == ATTRIBUTE_EXPRESSION_START && variable_character(first) && first != '?' &&
      !decimal_character(first)) {
    return false;
  }
  bool signed_number =
      (first == '+' && (kind == ARGUMENT_EXPRESSION_START || kind == INDEX_EXPRESSION_START)) ||
      generic_dash(first);
  if (first == '+' && !signed_number) {
    lexer->result_symbol = kind;
    return true;
  }
  if (signed_number) {
    lexer->advance(lexer, false);
  }
  if (ascii_digit(lexer->lookahead) || lexer->lookahead == '.') {
    bool numeric[EXTERNAL_TOKEN_COUNT] = {0};
    numeric[INTEGER_LITERAL] = true;
    numeric[REAL_LITERAL] = true;
    NumberContext context = {.allow_empty_binary = signed_number};
    if (!scan_number(lexer, numeric, context)) {
      return false;
    }
  }
  lexer->result_symbol = kind;
  return true;
}

// A collector removes one layer of quote/backtick pairs before its nested
// script is lexed. Keep one pending character per layer. Checking a flushed
// copy at each raw prefix identifies complete tokens without losing raw
// extents.
typedef struct {
  int32_t *pending;
  int32_t output[16];
  unsigned depth;
  unsigned length;
} QuoteStream;

static void quote_emit(QuoteStream *stream, int32_t c) {
  if (stream->length < 16) {
    stream->output[stream->length] = c;
  }
  if (stream->length < 17) {
    ++stream->length;
  }
}

static void quote_feed_special(QuoteStream *stream, unsigned layer, int32_t c) {
  for (; layer < stream->depth; ++layer) {
    int32_t pending = stream->pending[layer];
    if (!pending) {
      stream->pending[layer] = c;
      return;
    }
    stream->pending[layer] = 0;
    if (!double_quote(c)) {
      stream->pending[layer] = c;
      c = pending;
    }
  }
  quote_emit(stream, c);
}

static void quote_feed(QuoteStream *stream, unsigned layer, int32_t c) {
  if (c == '`' || double_quote(c)) {
    quote_feed_special(stream, layer, c);
    return;
  }
  for (; layer < stream->depth && stream->length < 16; ++layer) {
    if (stream->pending[layer]) {
      int32_t pending = stream->pending[layer];
      stream->pending[layer] = 0;
      quote_feed_special(stream, layer + 1, pending);
    }
  }
  quote_emit(stream, c);
}

static QuoteStream quote_flushed(QuoteStream stream, int32_t *pending) {
  memcpy(pending, stream.pending, stream.depth * sizeof(*pending));
  stream.pending = pending;
  for (unsigned i = 0; i < stream.depth; ++i) {
    if (stream.pending[i]) {
      int32_t c = stream.pending[i];
      stream.pending[i] = 0;
      quote_feed(&stream, i + 1, c);
    }
  }
  return stream;
}

static unsigned quote_layers(TSLexer *lexer) {
  TrackedLexer *tracked = (TrackedLexer *)lexer;
  return ordinary_interpolation_layers(tracked->scanner);
}

enum QuotedToken {
  NO_QUOTED_TOKEN,
  ONE_QUOTE,
  TWO_QUOTES,
  QUOTED_ESCAPE
};

static enum QuotedToken scan_quoted_token(TSLexer *lexer, unsigned maximum_quotes, bool escapes,
                                          int32_t *decoded) {
  size_t layers = quote_layers(lexer);
  if (layers > (SIZE_MAX / sizeof(int32_t) - 1) / 2) {
    return NO_QUOTED_TOKEN;
  }
  int32_t *pending = calloc((size_t)layers * 2 + 1, sizeof(*pending));
  if (!pending) {
    return NO_QUOTED_TOKEN;
  }
  QuoteStream stream = {.depth = layers, .pending = pending};
  enum QuotedToken accepted = NO_QUOTED_TOKEN;
  while (!lexer->eof(lexer)) {
    int32_t c = lexer->lookahead;
    if (accepted && !double_quote(c) && c != '`') {
      break;
    }
    if (!escapes && !double_quote(c) && c != '`') {
      break;
    }
    lexer->advance(lexer, false);
    quote_feed(&stream, 0, c);
    if (stream.length > 10) {
      break;
    }
    bool stable_quotes = stream.length <= maximum_quotes;
    for (unsigned i = 0; i < stream.length; ++i) {
      stable_quotes &= double_quote(stream.output[i]);
    }
    bool stable_escape = escapes && (!stream.length || stream.output[0] == '`');
    if (stream.length > 2 && stream.output[1] != 'u') {
      stable_escape = false;
    }
    if (!stable_quotes && !stable_escape) {
      break;
    }
    QuoteStream prefix = quote_flushed(stream, pending + layers);
    bool quotes = prefix.length && prefix.length <= maximum_quotes;
    for (unsigned i = 0; i < prefix.length && quotes; ++i) {
      quotes &= double_quote(prefix.output[i]);
    }
    if (quotes) {
      accepted = prefix.length == 2 ? TWO_QUOTES : ONE_QUOTE;
      lexer->mark_end(lexer);
      continue;
    }
    if (!escapes || prefix.length < 2 || prefix.output[0] != '`') {
      continue;
    }
    int32_t value = prefix.output[1];
    bool complete = prefix.length == 2 && value != 'u';
    if (value == 'u' && prefix.length >= 5 && prefix.length <= 10 && prefix.output[2] == '{' &&
        prefix.output[prefix.length - 1] == '}') {
      uint32_t unicode = 0;
      complete = true;
      for (unsigned i = 3; i + 1 < prefix.length; ++i) {
        int32_t digit = ascii_lower(prefix.output[i]);
        if (!ascii_hex(digit)) {
          complete = false;
          break;
        }
        unicode = unicode * 16 + (ascii_digit(digit) ? digit - '0' : digit - 'a' + 10);
      }
      complete &= unicode <= 0x10ffff;
      value = unicode;
    }
    if (complete) {
      lexer->mark_end(lexer);
      *decoded = value;
      accepted = QUOTED_ESCAPE;
      if (value != '`' && !double_quote(value)) {
        break;
      }
    }
  }
  free(pending);
  return accepted;
}

static bool scan_literal(TSLexer *lexer) {
  lexer->advance(lexer, false);
  while (!lexer->eof(lexer)) {
    int32_t c = lexer->lookahead;
    lexer->advance(lexer, false);
    if (single_quote(c)) {
      if (single_quote(lexer->lookahead)) {
        lexer->advance(lexer, false);
      } else {
        lexer->mark_end(lexer);
        return true;
      }
    }
  }
  return false;
}

static bool fragment_character(int32_t c) {
  return c != 0 && !horizontal(c) && !newline(c) && c != ';' && c != '|' && c != '&' && c != '(' &&
         c != ')' && c != '{' && c != '}' && c != ',' && c != '$' && !quote(c) && c != '`';
}

static bool scan_fragment(TSLexer *lexer, const bool *valid) {
  bool variable = valid[VARIABLE_ARGUMENT_FRAGMENT];
  int32_t first = lexer->lookahead;
  bool content = false;
  if (variable && (first == '=' || first == '.' || first == '[')) {
    return false;
  }
  if (first == ':' && valid[ARGUMENT_END]) {
    lexer->mark_end(lexer);
    lexer->advance(lexer, false);
    if (lexer->lookahead == ':') {
      lexer->advance(lexer, false);
      if (horizontal(lexer->lookahead) || newline(lexer->lookahead) ||
          (lexer->eof(lexer) && !at_collector_end(lexer))) {
        lexer->result_symbol = ARGUMENT_END;
        return true;
      }
      return false;
    }
    lexer->mark_end(lexer);
    content = true;
  } else if (variable && first == '?') {
    lexer->advance(lexer, false);
    lexer->mark_end(lexer);
    content = true;
    if (lexer->lookahead == '.' || lexer->lookahead == '[') {
      lexer->result_symbol = VARIABLE_ARGUMENT_FRAGMENT;
      return true;
    }
  } else if (!fragment_character(first) && first != '`') {
    return false;
  }
  while (!lexer->eof(lexer)) {
    if (lexer->lookahead == '`') {
      lexer->mark_end(lexer);
      lexer->advance(lexer, false);
      if (newline(lexer->lookahead)) {
        break;
      }
      int32_t decoded;
      if (!scan_backtick_escape_after_tick(lexer, &decoded, NULL)) {
        return false;
      }
      lexer->mark_end(lexer);
      content = true;
    } else if (fragment_character(lexer->lookahead)) {
      lexer->advance(lexer, false);
      lexer->mark_end(lexer);
      content = true;
    } else {
      break;
    }
  }
  if (!content) {
    return false;
  }
  lexer->result_symbol = variable ? VARIABLE_ARGUMENT_FRAGMENT : GENERIC_ARGUMENT_FRAGMENT;
  return true;
}

// GetAssemblyNameSpec uses commas between metadata properties and allows one
// equals sign within each property. Opening brackets are ordinary assembly text.
static bool assembly_word(int32_t c) {
  return c != 0 && !horizontal(c) && !newline(c) && c != ',' && c != '=' && c != ']';
}

static void scan_assembly_word(TSLexer *lexer) {
  while (!lexer->eof(lexer) && assembly_word(lexer->lookahead)) {
    lexer->advance(lexer, false);
  }
  lexer->mark_end(lexer);
  while (horizontal(lexer->lookahead)) {
    lexer->advance(lexer, false);
  }
}

static bool scan_assembly_name(TSLexer *lexer) {
  if (!assembly_word(lexer->lookahead) && lexer->lookahead != ',') {
    return false;
  }
  scan_assembly_word(lexer);
  while (lexer->lookahead == ',') {
    lexer->advance(lexer, false);
    while (horizontal(lexer->lookahead)) {
      lexer->advance(lexer, false);
    }
    scan_assembly_word(lexer);
    if (lexer->lookahead == '=') {
      lexer->advance(lexer, false);
      while (horizontal(lexer->lookahead)) {
        lexer->advance(lexer, false);
      }
      scan_assembly_word(lexer);
    }
  }
  lexer->result_symbol = ASSEMBLY_NAME_CONTENT;
  return true;
}

// ScanParameter consumes an optional colon and terminates on punctuation
// that can start a following expression. Other punctuation, including a
// backtick or quote, prevents a keyword flag from matching a shorter prefix.
static bool keyword_parameter_boundary(int32_t c) {
  return c == 0 || horizontal(c) || newline(c) || c == '{' || c == '}' || c == '(' || c == ')' ||
         c == ';' || c == ',' || c == '|' || c == '&' || c == '.' || c == '[';
}

static bool scan_keyword_parameter(TSLexer *lexer, const bool *valid) {
  static const struct {
    const char *name;
    enum TokenType kind;
  } parameters[] = {
      {"regex", SWITCH_REGEX_PARAMETER},
      {"wildcard", SWITCH_WILDCARD_PARAMETER},
      {"exact", SWITCH_EXACT_PARAMETER},
      {"casesensitive", SWITCH_CASESENSITIVE_PARAMETER},
      {"file", SWITCH_FILE_PARAMETER},
      {"supportedcommand", DATA_SUPPORTED_COMMAND_PARAMETER},
      {"parallel", FOREACH_PARALLEL_PARAMETER},
      {"throttlelimit", FOREACH_THROTTLELIMIT_PARAMETER},
  };
  lexer->advance(lexer, false);
  char word[sizeof("supportedcommand")] = {0};
  size_t length = 0;
  while (ascii_lower(lexer->lookahead) >= 'a' && ascii_lower(lexer->lookahead) <= 'z') {
    if (length + 1 == sizeof(word)) {
      return false;
    }
    word[length++] = (char)ascii_lower(lexer->lookahead);
    lexer->advance(lexer, false);
  }
  if (length == 0) {
    return false;
  }
  if (lexer->lookahead == ':') {
    lexer->advance(lexer, false);
  } else if (!keyword_parameter_boundary(lexer->lookahead)) {
    return false;
  }
  for (size_t i = 0; i < sizeof(parameters) / sizeof(parameters[0]); ++i) {
    if (valid[parameters[i].kind] && strncmp(word, parameters[i].name, length) == 0) {
      lexer->mark_end(lexer);
      lexer->result_symbol = parameters[i].kind;
      return true;
    }
  }
  return false;
}

static bool prepare_transformed_token(Scanner *, TSLexer *, const bool *, size_t, PreparedToken *,
                                      bool);
static bool emit_prepared_token(TSLexer *, const bool *, size_t, PreparedToken *);

static bool request_preparation(Scanner *scanner, TSLexer *lexer, const bool *valid) {
  if (!prepare_transformed_token(scanner, lexer, valid, EXTERNAL_TOKEN_COUNT, &scanner->prepared,
                                 false)) {
    return false;
  }
  lexer->result_symbol = PREPARED_TOKEN;
  return true;
}

static bool scan_subexpression_end(Scanner *scanner, TSLexer *lexer, const bool *valid) {
  if (!valid[SUB_EXPRESSION_END] || lexer->lookahead != ')') {
    return false;
  }
  if (valid[VERBATIM_ARGUMENT_TEXT] && !valid[VERBATIM_ARGUMENTS_START]) {
    return false;
  }
  TrackedLexer *tracked = (TrackedLexer *)lexer;
  if (scanner->depth) {
    Interpolation frame = current_interpolation(scanner);
    bool collector = !frame.closed && scanner->semantic_depth == frame.semantic_depth;
    if (collector) {
      if (tracked->balance < frame.balance) {
        return false;
      }
    } else if (scanner->semantic_depth < frame.semantic_depth ||
               tracked->balance <= frame.balance) {
      return false;
    }
    lexer->advance(lexer, false);
    if (collector) {
      if (!close_interpolation(scanner)) {
        return false;
      }
    } else {
      --scanner->semantic_depth;
    }
  } else {
    lexer->advance(lexer, false);
  }
  lexer->mark_end(lexer);
  lexer->result_symbol = SUB_EXPRESSION_END;
  return true;
}

static bool scan_subexpression_start(Scanner *scanner, TSLexer *lexer, const bool *valid) {
  if (lexer->lookahead != '$' ||
      (!valid[SUB_EXPRESSION_OPEN] && !valid[ADJACENT_SUB_EXPRESSION_OPEN] &&
       !valid[INTERPOLATION_OPEN])) {
    return false;
  }
  TrackedLexer *tracked = (TrackedLexer *)lexer;
  lexer->mark_end(lexer);
  lexer->advance(lexer, false);
  if (lexer->lookahead != '(') {
    // A literal-dollar name must use the same transformed input as any other
    // command name. Preparation starts before the already-consumed dollar.
    if (valid[PREPARED_TOKEN] && ordinary_interpolation_layers(scanner) &&
        (valid[COMMAND_NAME] || valid[BARE_WORD] || valid[DATA_COMMAND_NAME])) {
      if (!prepare_transformed_token(scanner, lexer, valid, EXTERNAL_TOKEN_COUNT,
                                     &scanner->prepared, true)) {
        return false;
      }
      lexer->result_symbol = PREPARED_TOKEN;
      return true;
    }
    if (valid[DATA_COMMAND_NAME] && !valid[COMMAND_NAME]) {
      return scan_data_variable_name_after_prefix(lexer, valid, false);
    }
    if (variable_start(lexer->lookahead)) {
      return scan_variable_after_dollar(lexer, valid,
                                        lexer->lookahead == '{' && valid[BRACED_ARGUMENT_VARIABLE]
                                            ? BRACED_ARGUMENT_VARIABLE
                                        : valid[VARIABLE] ? VARIABLE
                                                          : ADJACENT_VARIABLE);
    }
    if (!valid[LITERAL_DOLLAR] && (valid[BARE_WORD] || valid[COMMAND_NAME])) {
      return scan_generic_after_prefix(lexer, valid, '$');
    }
    lexer->mark_end(lexer);
    lexer->result_symbol = LITERAL_DOLLAR;
    return valid[LITERAL_DOLLAR];
  }
  bool quoted = valid[INTERPOLATION_OPEN] && !valid[EXPANDABLE_HERE_STRING_START];
  lexer->advance(lexer, false);
  if (scanner->depth) {
    if (scanner->semantic_depth == UINT32_MAX) {
      return false;
    }
    ++scanner->semantic_depth;
  } else if (quoted) {
    tracked->balance = 1;
    scanner->semantic_depth = 1;
  }
  if (quoted &&
      !push_interpolation(scanner, (Interpolation){.balance = tracked->balance,
                                                   .semantic_depth = scanner->semantic_depth,
                                                   .ordinary = !valid[HERE_STRING_CONTENT] &&
                                                               !valid[HERE_STRING_END]})) {
    return false;
  }
  lexer->mark_end(lexer);
  lexer->result_symbol = valid[INTERPOLATION_OPEN]             ? INTERPOLATION_OPEN
                         : valid[ADJACENT_SUB_EXPRESSION_OPEN] ? ADJACENT_SUB_EXPRESSION_OPEN
                                                               : SUB_EXPRESSION_OPEN;
  return true;
}

static bool scan_interpolation_tail(Scanner *scanner, TSLexer *lexer, const bool *valid) {
  if (!valid[INTERPOLATION_TAIL] || valid[EXPANDABLE_HERE_STRING_START] || !scanner->depth ||
      !scanner->top.closed) {
    return false;
  }
  TrackedLexer *tracked = (TrackedLexer *)lexer;
  Interpolation frame = scanner->top;
  if (tracked->balance >= frame.balance) {
    bool ignored = false;
    // GetMemberAccessOperator skips block comments even when no member follows.
    // Other trailing text is not reparsed after the primary expression ends.
    if (valid[POSTFIX_CONTEXT] && lexer->lookahead == '<') {
      lexer->advance(lexer, false);
      if (lexer->lookahead == '#') {
        lexer->advance(lexer, false);
        bool closed = false;
        while (!lexer->eof(lexer)) {
          int32_t c = lexer->lookahead;
          lexer->advance(lexer, false);
          if (c == '#' && lexer->lookahead == '>') {
            lexer->advance(lexer, false);
            closed = true;
            break;
          }
        }
        if (!closed) {
          return false;
        }
        lexer->mark_end(lexer);
        if (valid[METHOD_INVOCATION_CONTEXT]) {
          while (horizontal(lexer->lookahead)) {
            lexer->advance(lexer, false);
          }
          // A comment ends adjacency between a member name and its call.
          // The rest of the collected text remains outside the expression.
          ignored = lexer->lookahead == '(' || lexer->lookahead == '{';
        }
        if (!ignored) {
          lexer->result_symbol = COMMENT;
          return valid[COMMENT];
        }
      } else {
        ignored = true;
      }
    }
    if (!ignored && valid[POSTFIX_CONTEXT]) {
      int32_t c = lexer->lookahead;
      bool command = valid[COMMAND_INTERPOLATION_CONTEXT];
      if (c == '[') {
        return false;
      }
      if (c == '.' || c == ':' || c == '?') {
        lexer->advance(lexer, false);
        if (c == '?' && (lexer->lookahead == '.' || lexer->lookahead == '[')) {
          return false;
        }
        if (c == ':' && lexer->lookahead == ':') {
          lexer->advance(lexer, false);
        } else if (c != '.' || lexer->lookahead == '.') {
          ignored = true;
        }
        if (!ignored && (!command || (lexer->lookahead != 0 && !horizontal(lexer->lookahead) &&
                                      !newline(lexer->lookahead)))) {
          return false;
        }
        ignored = true;
      }
    }
    if (!ignored && valid[METHOD_INVOCATION_CONTEXT] &&
        (lexer->lookahead == '(' || lexer->lookahead == '{')) {
      return false;
    }
    while (!tracked->base->eof(tracked->base) && tracked->balance >= frame.balance) {
      lexer->advance(lexer, false);
    }
    if (tracked->balance >= frame.balance) {
      return false;
    }
  }
  --scanner->semantic_depth;
  pop_interpolation(scanner);
  lexer->mark_end(lexer);
  lexer->result_symbol = INTERPOLATION_TAIL;
  return true;
}

static bool scan(Scanner *scanner, TSLexer *lexer, const bool *valid) {
  if (valid[INTERPOLATION_TAIL] && !valid[EXPANDABLE_HERE_STRING_START] && scanner->depth &&
      scanner->top.closed) {
    return scan_interpolation_tail(scanner, lexer, valid);
  }
  // Clause keywords are scanned in command mode. A glued comment, quote,
  // backtick, or bracket continues their generic token instead of ending it.
  if (valid[CLAUSE_KEYWORD_END] && !valid[HERE_STRING_CONTENT]) {
    int32_t c = lexer->lookahead;
    if (!generic_end(c) || quote(c) || c == '<' || c == '>') {
      return false;
    }
    lexer->mark_end(lexer);
    lexer->result_symbol = CLAUSE_KEYWORD_END;
    return true;
  }
  if (scan_subexpression_end(scanner, lexer, valid)) {
    return true;
  }
  if (lexer->lookahead == '$' &&
      (valid[SUB_EXPRESSION_OPEN] || valid[ADJACENT_SUB_EXPRESSION_OPEN] ||
       valid[INTERPOLATION_OPEN])) {
    return scan_subexpression_start(scanner, lexer, valid);
  }
  // Punctuation remains visible during recovery, where every external is valid.
  if (lexer->lookahead == '(' && (valid[LEFT_PAREN] || valid[ADJACENT_LEFT_PAREN])) {
    lexer->advance(lexer, false);
    lexer->mark_end(lexer);
    lexer->result_symbol = valid[ADJACENT_LEFT_PAREN] ? ADJACENT_LEFT_PAREN : LEFT_PAREN;
    return true;
  }
  if (lexer->lookahead == ')' && valid[RIGHT_PAREN]) {
    if (lexer->eof(lexer)) {
      return false;
    }
    lexer->advance(lexer, false);
    lexer->mark_end(lexer);
    lexer->result_symbol = RIGHT_PAREN;
    return true;
  }
  bool preparing = valid[PREPARED_TOKEN] && ordinary_interpolation_layers(scanner);
  bool here_body = (valid[HERE_STRING_CONTENT] || valid[HERE_STRING_END]) &&
                   !valid[EXPANDABLE_HERE_STRING_START];
  bool fragments = (valid[VARIABLE_ARGUMENT_FRAGMENT] || valid[GENERIC_ARGUMENT_FRAGMENT]) &&
                   !valid[COMMAND_NAME] && !valid[HERE_STRING_CONTENT] &&
                   (valid[GENERIC_ARGUMENT_FRAGMENT] || lexer->lookahead != '=') &&
                   (fragment_character(lexer->lookahead) || lexer->lookahead == '`');
  bool verbatim = valid[VERBATIM_ARGUMENT_TEXT] && !valid[VERBATIM_ARGUMENTS_START];
  if (preparing && (here_body || fragments || verbatim)) {
    return request_preparation(scanner, lexer, valid);
  }
  if ((valid[DOUBLE_QUOTE] || valid[ADJACENT_DOUBLE_QUOTE] || valid[STRING_ESCAPE]) &&
      !valid[EXPANDABLE_HERE_STRING_START] &&
      ((double_quote(lexer->lookahead) && (valid[DOUBLE_QUOTE] || valid[ADJACENT_DOUBLE_QUOTE])) ||
       (lexer->lookahead == '`' && valid[STRING_ESCAPE]))) {
    int32_t decoded;
    unsigned maximum =
        (valid[DOUBLE_QUOTE] || valid[ADJACENT_DOUBLE_QUOTE]) ? valid[STRING_CONTENT] ? 2 : 1 : 0;
    enum QuotedToken token = scan_quoted_token(lexer, maximum, valid[STRING_ESCAPE], &decoded);
    if (!token) {
      return false;
    }
    lexer->result_symbol = token == TWO_QUOTES            ? STRING_CONTENT
                           : token == QUOTED_ESCAPE       ? STRING_ESCAPE
                           : valid[ADJACENT_DOUBLE_QUOTE] ? ADJACENT_DOUBLE_QUOTE
                                                          : DOUBLE_QUOTE;
    return true;
  }
  if (valid[STRING_CONTENT] && !valid[EXPANDABLE_HERE_STRING_START] && lexer->lookahead != '`' &&
      lexer->lookahead != '$') {
    bool content = false;
    while (!lexer->eof(lexer) && !double_quote(lexer->lookahead) && lexer->lookahead != '$' &&
           lexer->lookahead != '`') {
      content = true;
      lexer->advance(lexer, false);
    }
    if (!content) {
      return false;
    }
    lexer->mark_end(lexer);
    lexer->result_symbol = STRING_CONTENT;
    return true;
  }
  if (valid[ADJACENT_SINGLE_QUOTED_STRING] && single_quote(lexer->lookahead)) {
    if (!scan_literal(lexer)) {
      return false;
    }
    lexer->result_symbol = ADJACENT_SINGLE_QUOTED_STRING;
    return true;
  }
  if ((valid[VARIABLE_ARGUMENT_FRAGMENT] || valid[GENERIC_ARGUMENT_FRAGMENT]) &&
      !valid[COMMAND_NAME] && !valid[HERE_STRING_CONTENT] &&
      (valid[GENERIC_ARGUMENT_FRAGMENT] || lexer->lookahead != '=') &&
      (fragment_character(lexer->lookahead) || lexer->lookahead == '`')) {
    return scan_fragment(lexer, valid);
  }

  // During error recovery tree-sitter enables every external token. Do not
  // reinterpret arbitrary malformed source as a here-string body in that state.
  if (valid[VERBATIM_ARGUMENT_TEXT] && !valid[VERBATIM_ARGUMENTS_START]) {
    return scan_verbatim_argument_text(lexer);
  }
  if (valid[STRING_ESCAPE] && lexer->lookahead == '`' && !valid[EXPANDABLE_HERE_STRING_START]) {
    return scan_string_escape(lexer);
  }
  if (valid[LITERAL_DOLLAR] && !valid[EXPANDABLE_HERE_STRING_START] && lexer->lookahead == '$') {
    return scan_string_dollar(lexer, valid);
  }
  if ((valid[HERE_STRING_CONTENT] || valid[HERE_STRING_END]) &&
      !valid[EXPANDABLE_HERE_STRING_START]) {
    return scan_here_string_content(lexer, valid);
  }
  if (valid[LITERAL_DOLLAR] && !valid[EXPANDABLE_HERE_STRING_START]) {
    return false;
  }
  if ((valid[COMMAND_MEMBER_START] || valid[COMMAND_UNARY_START]) && !valid[HERE_STRING_CONTENT]) {
    lexer->mark_end(lexer);
    if (!command_mode_expression_follows(lexer, valid[COMMAND_UNARY_START], true)) {
      return false;
    }
    lexer->result_symbol = valid[COMMAND_UNARY_START] ? COMMAND_UNARY_START : COMMAND_MEMBER_START;
    return true;
  }
  if ((valid[FLOW_EXPRESSION_START] || valid[FLOW_LABEL_END]) && !valid[HERE_STRING_CONTENT]) {
    lexer->mark_end(lexer);
    if (valid[FLOW_EXPRESSION_START] &&
        command_mode_expression_follows(lexer, !valid[FLOW_LABEL_END], false)) {
      lexer->result_symbol = FLOW_EXPRESSION_START;
      return true;
    }
    if (valid[FLOW_LABEL_END]) {
      lexer->result_symbol = FLOW_LABEL_END;
      return true;
    }
    return false;
  }
  if (valid[EXPRESSION_ARGUMENT_END] && !valid[HERE_STRING_CONTENT] && lexer->lookahead == '=') {
    lexer->mark_end(lexer);
    lexer->result_symbol = EXPRESSION_ARGUMENT_END;
    return true;
  }
  // These zero-width endings let the grammar stop a completed expression
  // without absorbing an adjacent operator into its enclosing command argument.
  if ((valid[MEMBER_ARGUMENT_END] || (valid[EXPRESSION_ARGUMENT_END] && !valid[ARGUMENT_END])) &&
      !valid[HERE_STRING_CONTENT]) {
    lexer->mark_end(lexer);
    int32_t c = lexer->lookahead;
    if (c == '.' || c == '[' || (valid[MEMBER_ARGUMENT_END] && (c == '(' || c == '{'))) {
      return false;
    }
    if (c == ':') {
      lexer->advance(lexer, false);
      if (lexer->lookahead == ':') {
        return false;
      }
    }
    if (c == '?') {
      lexer->advance(lexer, false);
      if (lexer->lookahead == '.' || lexer->lookahead == '[') {
        return false;
      }
    }
    lexer->result_symbol =
        valid[MEMBER_ARGUMENT_END] ? MEMBER_ARGUMENT_END : EXPRESSION_ARGUMENT_END;
    return true;
  }
  if (valid[ARGUMENT_END] && !valid[HERE_STRING_CONTENT]) {
    lexer->mark_end(lexer);
    if (generic_end(lexer->lookahead) && !quote(lexer->lookahead) && lexer->lookahead != '<' &&
        lexer->lookahead != '>') {
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
        if (horizontal(lexer->lookahead) || newline(lexer->lookahead) ||
            (lexer->eof(lexer) && !at_collector_end(lexer))) {
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
  if (valid[ARGUMENT_END] && !valid[HERE_STRING_CONTENT]) {
    return false;
  }
  if (valid[POSTFIX_CONTEXT] && !valid[HERE_STRING_CONTENT] &&
      (lexer->lookahead == '.' || lexer->lookahead == '[')) {
    return false;
  }
  bool skipped_trivia = false;
  while (horizontal(lexer->lookahead)) {
    skipped_trivia = true;
    lexer->advance(lexer, true);
  }
  // The preparation extra must not hide trivia from an adjacent method opener.
  if (skipped_trivia && valid[METHOD_INVOCATION_CONTEXT] &&
      (lexer->lookahead == '(' || lexer->lookahead == '{')) {
    return false;
  }
  // Let extras consume trivia before classifying the following expression.
  if (!valid[HERE_STRING_CONTENT] && lexer->lookahead != '#' && lexer->lookahead != '<' &&
      lexer->lookahead != '`' &&
      (valid[ARGUMENT_EXPRESSION_START] || valid[ATTRIBUTE_EXPRESSION_START] ||
       valid[MEMBER_EXPRESSION_START] || valid[INDEX_EXPRESSION_START])) {
    enum TokenType kind;
    if (valid[ARGUMENT_EXPRESSION_START]) {
      kind = ARGUMENT_EXPRESSION_START;
    } else if (valid[ATTRIBUTE_EXPRESSION_START]) {
      kind = ATTRIBUTE_EXPRESSION_START;
    } else if (valid[MEMBER_EXPRESSION_START]) {
      kind = MEMBER_EXPRESSION_START;
    } else {
      kind = INDEX_EXPRESSION_START;
    }
    return scan_expression_start(lexer, kind);
  }
  if (scan_subexpression_end(scanner, lexer, valid)) {
    return true;
  }
  if (lexer->lookahead == '$' && valid[SUB_EXPRESSION_OPEN]) {
    return scan_subexpression_start(scanner, lexer, valid);
  }
  if (preparing && !newline(lexer->lookahead) &&
      (valid[FUNCTION_NAME] || valid[COMMAND_NAME] || valid[BARE_WORD] ||
       valid[DATA_COMMAND_NAME] || valid[PARAM_BLOCK_KEYWORD] || valid[PARAM_COMMAND_NAME] ||
       (valid[SIGNED_NUMBER_START] &&
        (lexer->lookahead == '+' || generic_dash(lexer->lookahead))) ||
       (lexer->lookahead == '@' &&
        (valid[SINGLE_QUOTED_HERE_STRING] || valid[EXPANDABLE_HERE_STRING_START])))) {
    return request_preparation(scanner, lexer, valid);
  }
  if (lexer->lookahead == '(' && valid[LEFT_PAREN]) {
    lexer->advance(lexer, false);
    lexer->mark_end(lexer);
    lexer->result_symbol = LEFT_PAREN;
    return true;
  }
  if (lexer->lookahead == ')' && valid[RIGHT_PAREN]) {
    if (lexer->eof(lexer)) {
      return false;
    }
    lexer->advance(lexer, false);
    lexer->mark_end(lexer);
    lexer->result_symbol = RIGHT_PAREN;
    return true;
  }
  // Do not scan permissive assembly text in the all-token recovery state.
  if (valid[ASSEMBLY_NAME_CONTENT] && !valid[HERE_STRING_CONTENT]) {
    return scan_assembly_name(lexer);
  }
  if (valid[DOUBLE_QUOTE] &&
      (double_quote(lexer->lookahead) || (lexer->lookahead == '`' && quote_layers(lexer)))) {
    int32_t decoded;
    if (scan_quoted_token(lexer, 1, false, &decoded) != ONE_QUOTE) {
      return false;
    }
    lexer->result_symbol = DOUBLE_QUOTE;
    return true;
  }
  if (valid[SINGLE_QUOTED_STRING] && single_quote(lexer->lookahead)) {
    if (!scan_literal(lexer)) {
      return false;
    }
    lexer->result_symbol = SINGLE_QUOTED_STRING;
    return true;
  }
  if (valid[ARGUMENT_SEPARATOR] && lexer->lookahead == ',') {
    lexer->advance(lexer, false);
    lexer->mark_end(lexer);
    lexer->result_symbol = ARGUMENT_SEPARATOR;
    return true;
  }
  if (valid[LINE_CONTINUATION] && !valid[COMMAND_NAME] && !valid[BARE_WORD] &&
      !valid[DATA_COMMAND_NAME] && !valid[FUNCTION_NAME] && lexer->lookahead == '`') {
    lexer->advance(lexer, false);
    if (horizontal(lexer->lookahead)) {
      return scan_escaped_whitespace_after_tick(lexer);
    }
    return scan_line_continuation_after_tick(lexer, valid);
  }
  // Do not let a recovery comment swallow quotes or bypass a required boundary.
  if (valid[COMMENT] && !valid[HERE_STRING_CONTENT] &&
      (lexer->lookahead == '#' || lexer->lookahead == '<')) {
    return scan_comment(lexer, valid);
  }
  if (generic_dash(lexer->lookahead) &&
      (valid[SWITCH_REGEX_PARAMETER] || valid[SWITCH_WILDCARD_PARAMETER] ||
       valid[SWITCH_EXACT_PARAMETER] || valid[SWITCH_CASESENSITIVE_PARAMETER] ||
       valid[SWITCH_FILE_PARAMETER] || valid[DATA_SUPPORTED_COMMAND_PARAMETER] ||
       valid[FOREACH_PARALLEL_PARAMETER] || valid[FOREACH_THROTTLELIMIT_PARAMETER])) {
    return scan_keyword_parameter(lexer, valid);
  }
  if (valid[FUNCTION_NAME] && !valid[COMMAND_NAME]) {
    return scan_function_name(lexer, valid);
  }
  if (valid[DATA_COMMAND_NAME] && !valid[COMMAND_NAME] && lexer->lookahead == '$') {
    return scan_data_variable_name(lexer, valid);
  }
  if (valid[LABEL_START] && !valid[HERE_STRING_CONTENT] && lexer->lookahead == ':') {
    return scan_label_start(lexer);
  }
  if (valid[FLOW_LABEL] && !valid[HERE_STRING_CONTENT] && variable_character(lexer->lookahead) &&
      lexer->lookahead != '?' && !decimal_character(lexer->lookahead)) {
    return scan_flow_label(lexer);
  }
  if ((valid[VARIABLE] || valid[BRACED_ARGUMENT_VARIABLE]) && !valid[COMMAND_NAME] &&
      !valid[BARE_WORD] && lexer->lookahead == '$') {
    lexer->advance(lexer, false);
    if (lexer->lookahead == '{' && valid[BRACED_ARGUMENT_VARIABLE]) {
      return scan_variable_after_dollar(lexer, valid, BRACED_ARGUMENT_VARIABLE);
    }
    return scan_variable_after_dollar(lexer, valid, VARIABLE);
  }
  if (lexer->lookahead == '@' &&
      (valid[SINGLE_QUOTED_HERE_STRING] || valid[EXPANDABLE_HERE_STRING_START] ||
       valid[SPLAT_EXPRESSION] || valid[BARE_WORD] || valid[ARRAY_EXPRESSION_OPEN])) {
    lexer->advance(lexer, false);
    if (valid[ARRAY_EXPRESSION_OPEN] && lexer->lookahead == '(') {
      lexer->advance(lexer, false);
      lexer->mark_end(lexer);
      lexer->result_symbol = ARRAY_EXPRESSION_OPEN;
      return true;
    }
    if (valid[DATA_COMMAND_NAME] && !valid[COMMAND_NAME]) {
      return scan_data_variable_name_after_prefix(lexer, valid, true);
    }
    if (quote(lexer->lookahead)) {
      return scan_here_string_after_at(lexer, valid);
    }
    return scan_splat_after_at(lexer, valid);
  }
  if (valid[COMMAND_NAME] || valid[BARE_WORD] || valid[DATA_COMMAND_NAME] ||
      valid[PARAM_BLOCK_KEYWORD] || valid[PARAM_COMMAND_NAME] ||
      (valid[SIGNED_NUMBER_START] && (lexer->lookahead == '+' || generic_dash(lexer->lookahead)))) {
    // A failed generic scan cannot be rewound. Numeric starts are handled by
    // the ordinary lexer unless an argument-mode bare token is required.
    if (ascii_digit(lexer->lookahead) || lexer->lookahead == '.') {
      if (valid[BARE_WORD] || valid[COMMAND_NAME] || valid[DATA_COMMAND_NAME]) {
        return scan_generic(lexer, valid, false);
      }
    } else {
      return scan_generic(lexer, valid, false);
    }
  }
  if (valid[INTEGER_LITERAL] || valid[REAL_LITERAL]) {
    NumberContext context = {.end_on_ternary = true, .mark_end = true};
    return scan_number(lexer, valid, context);
  }
  return false;
}

typedef struct {
  int32_t character;
  uint32_t end;
} PreparationCharacter;

typedef struct {
  TSLexer lexer;
  const PreparationCharacter *characters;
  size_t length;
  size_t position;
  size_t marked;
  size_t start;
  uint32_t column;
  bool included_range_start;
} PreparationLexer;

static void preparation_advance(TSLexer *lexer, bool skip) {
  PreparationLexer *prepared = (PreparationLexer *)lexer;
  if (prepared->position < prepared->length) {
    if (lexer->lookahead == '\n') {
      prepared->column = 0;
    } else {
      ++prepared->column;
    }
    ++prepared->position;
  }
  if (skip) {
    prepared->start = prepared->position;
  }
  lexer->lookahead = prepared->position < prepared->length
                         ? prepared->characters[prepared->position].character
                         : 0;
}

static void preparation_mark_end(TSLexer *lexer) {
  PreparationLexer *prepared = (PreparationLexer *)lexer;
  prepared->marked = prepared->position;
}

static uint32_t preparation_column(TSLexer *lexer) {
  PreparationLexer *prepared = (PreparationLexer *)lexer;
  return prepared->column;
}

static bool preparation_range_start(const TSLexer *lexer) {
  const PreparationLexer *prepared = (const PreparationLexer *)lexer;
  return prepared->position == 0 && prepared->included_range_start;
}

static bool preparation_eof(const TSLexer *lexer) {
  const PreparationLexer *prepared = (const PreparationLexer *)lexer;
  return prepared->position == prepared->length;
}

/* Temporary prefixes contain only output that later input cannot change.
 * Withhold each layer's trailing tick/quote until the real collector ends. */
static size_t preparation_transform(PreparationCharacter *characters, size_t length,
                                    uint32_t layers, bool complete) {
  for (uint32_t layer = 0; layer < layers && length; ++layer) {
    size_t output = 0;
    bool changed = false;
    for (size_t input = 0; input < length; ++input) {
      int32_t character = characters[input].character;
      if (character == '`' || double_quote(character)) {
        if (input + 1 == length && !complete) {
          changed = true;
          break;
        }
        if (input + 1 < length && double_quote(characters[input + 1].character)) {
          ++input;
          changed = true;
        }
      }
      characters[output++] = characters[input];
    }
    length = output;
    if (!changed) {
      /* No remaining pair can change on a later identical pass. */
      break;
    }
  }
  return length;
}

static bool prepare_transformed_token(Scanner *scanner, TSLexer *lexer, const bool *valid,
                                      size_t valid_count, PreparedToken *result,
                                      bool dollar_prefix) {
  uint32_t layers = ordinary_interpolation_layers(scanner);
  if (!layers || valid_count <= SUB_EXPRESSION_OPEN || valid_count <= SUB_EXPRESSION_END) {
    return false;
  }
  PreparedToken prepared = {0};
  bool range_start = lexer->is_at_included_range_start(lexer);
  if (!dollar_prefix) {
    lexer->mark_end(lexer);
  }
  PreparationCharacter *raw = NULL;
  PreparationCharacter *transformed = NULL;
  bool *virtual_valid = malloc(valid_count * sizeof(*virtual_valid));
  if (!virtual_valid) {
    return false;
  }
  memcpy(virtual_valid, valid, valid_count * sizeof(*virtual_valid));
  virtual_valid[SUB_EXPRESSION_OPEN] = false;
  virtual_valid[ADJACENT_SUB_EXPRESSION_OPEN] = false;
  virtual_valid[SUB_EXPRESSION_END] = false;
  virtual_valid[INTERPOLATION_OPEN] = false;
  virtual_valid[INTERPOLATION_TAIL] = false;
  size_t length = 0;
  size_t capacity = 0;
  size_t target = 32;
  bool success = false;
  bool collected_verbatim_close = false;
  for (;;) {
    if (target > SIZE_MAX / sizeof(*raw)) {
      break;
    }
    if (target > capacity) {
      PreparationCharacter *larger = realloc(raw, target * sizeof(*raw));
      if (!larger) {
        break;
      }
      raw = larger;
      larger = realloc(transformed, target * sizeof(*transformed));
      if (!larger) {
        break;
      }
      transformed = larger;
      capacity = target;
    }
    if (dollar_prefix && length == 0) {
      raw[length++] = (PreparationCharacter){.character = '$', .end = 1};
    }
    while (length < target) {
      if (lexer->eof(lexer) && !(valid[VERBATIM_ARGUMENT_TEXT] &&
                                 !valid[VERBATIM_ARGUMENTS_START] && at_collector_end(lexer))) {
        break;
      }
      if (length == UINT32_MAX) {
        goto cleanup;
      }
      bool closing = at_collector_end(lexer);
      int32_t character = lexer->lookahead;
      lexer->advance(lexer, false);
      raw[length] = (PreparationCharacter){
          .character = character,
          .end = (uint32_t)length + 1,
      };
      ++length;
      if (closing) {
        // Let the virtual verbatim lexer retain this byte, just as the direct
        // lexer does. The subsequent parse then diagnoses its missing close.
        collected_verbatim_close = true;
        break;
      }
    }
    bool complete = collected_verbatim_close || lexer->eof(lexer);
    memcpy(transformed, raw, length * sizeof(*raw));
    size_t virtual_length = preparation_transform(transformed, length, layers, complete);
    PreparationLexer source = {
        .lexer =
            {
                .lookahead = virtual_length ? transformed[0].character : 0,
                .advance = preparation_advance,
                .mark_end = preparation_mark_end,
                .get_column = preparation_column,
                .is_at_included_range_start = preparation_range_start,
                .eof = preparation_eof,
            },
        .characters = transformed,
        .length = virtual_length,
        .included_range_start = range_start,
    };
    Scanner virtual_scanner = {.collector_eof = complete && !collected_verbatim_close};
    TrackedLexer virtual_lexer = {
        .lexer =
            {
                .lookahead = source.lexer.lookahead,
                .advance = tracked_advance,
                .mark_end = tracked_mark_end,
                .get_column = tracked_column,
                .is_at_included_range_start = tracked_range_start,
                .eof = tracked_eof,
            },
        .base = &source.lexer,
        .scanner = &virtual_scanner,
    };
    bool accepted = scan(&virtual_scanner, &virtual_lexer.lexer, virtual_valid);
    if (complete || source.position < virtual_length) {
      if (accepted &&
          (source.marked > source.start ||
           (source.marked == source.start &&
            prepared_context_token(virtual_lexer.lexer.result_symbol))) &&
          source.marked <= virtual_length && virtual_lexer.lexer.result_symbol < valid_count &&
          valid[virtual_lexer.lexer.result_symbol]) {
        prepared.start = source.start ? transformed[source.start - 1].end : 0;
        prepared.end = source.marked ? transformed[source.marked - 1].end : 0;
        prepared.symbol = virtual_lexer.lexer.result_symbol;
        prepared.active = true;
        *result = prepared;
        success = true;
      }
      break;
    }
    if (target > SIZE_MAX / 2) {
      break;
    }
    target *= 2;
  }
cleanup:
  free(virtual_valid);
  free(transformed);
  free(raw);
  return success;
}

static bool emit_prepared_token(TSLexer *lexer, const bool *valid, size_t valid_count,
                                PreparedToken *prepared) {
  if (!prepared->active || prepared->symbol >= valid_count || !valid[prepared->symbol] ||
      (prepared->start > prepared->end ||
       (prepared->start == prepared->end && !prepared_context_token(prepared->symbol)))) {
    return false;
  }
  for (uint32_t position = 0; position < prepared->end; ++position) {
    if (lexer->eof(lexer) &&
        !(prepared->symbol == VERBATIM_ARGUMENT_TEXT && at_collector_end(lexer))) {
      return false;
    }
    lexer->advance(lexer, position < prepared->start);
  }
  lexer->mark_end(lexer);
  lexer->result_symbol = prepared->symbol;
  prepared->active = false;
  return true;
}

bool tree_sitter_powershell_external_scanner_scan(void *payload, TSLexer *lexer,
                                                  const bool *valid) {
  Scanner candidate = *(Scanner *)payload;
  candidate.collector_eof = false;
  TrackedLexer tracked = {.lexer =
                              {
                                  .lookahead = lexer->lookahead,
                                  .advance = tracked_advance,
                                  .mark_end = tracked_mark_end,
                                  .get_column = tracked_column,
                                  .is_at_included_range_start = tracked_range_start,
                                  .eof = tracked_eof,
                              },
                          .base = lexer,
                          .scanner = &candidate,
                          .balance = candidate.balance,
                          .marked_balance = candidate.balance};
  bool accepted;
  if (candidate.prepared.active) {
    accepted =
        emit_prepared_token(&tracked.lexer, valid, EXTERNAL_TOKEN_COUNT, &candidate.prepared);
  } else {
    accepted = scan(&candidate, &tracked.lexer, valid);
  }
  if (!accepted || tracked.overflow) {
    return false;
  }
  candidate.balance = candidate.depth ? tracked.marked_balance : 0;
  *(Scanner *)payload = candidate;
  lexer->result_symbol = tracked.lexer.result_symbol;
  return true;
}
