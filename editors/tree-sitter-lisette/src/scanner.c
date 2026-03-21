#include "tree_sitter/alloc.h"
#include "tree_sitter/parser.h"

#include <wctype.h>

enum TokenType {
    STRING_CONTENT,
    FLOAT_LITERAL,
    AUTOMATIC_SEMICOLON,
    FORMAT_STRING_CONTENT,
    ERROR_SENTINEL,
    OPEN_ANGLE,
    BANG,
};

void *tree_sitter_lisette_external_scanner_create() { return NULL; }

void tree_sitter_lisette_external_scanner_destroy(void *payload) {}

unsigned tree_sitter_lisette_external_scanner_serialize(void *payload, char *buffer) { return 0; }

void tree_sitter_lisette_external_scanner_deserialize(void *payload, const char *buffer, unsigned length) {}

static inline bool is_num_char(int32_t c) { return c == '_' || iswdigit(c); }

static inline void advance(TSLexer *lexer) { lexer->advance(lexer, false); }

static inline void skip(TSLexer *lexer) { lexer->advance(lexer, true); }

// Scan string content: everything up to a closing quote or escape
static inline bool scan_string_content(TSLexer *lexer) {
    bool has_content = false;
    for (;;) {
        if (lexer->lookahead == '"' || lexer->lookahead == '\\') {
            break;
        }
        if (lexer->eof(lexer)) {
            return false;
        }
        has_content = true;
        advance(lexer);
    }
    lexer->result_symbol = STRING_CONTENT;
    lexer->mark_end(lexer);
    return has_content;
}

// Scan format string text content: everything up to {, }, ", or backslash
// Handles {{ and }} as escaped braces (they become part of the text content)
static inline bool scan_format_string_content(TSLexer *lexer) {
    bool has_content = false;
    for (;;) {
        if (lexer->eof(lexer)) {
            break;
        }
        if (lexer->lookahead == '\\') {
            // Escape sequence - stop here, let grammar handle it
            break;
        }
        if (lexer->lookahead == '"') {
            // End of format string - stop
            break;
        }
        if (lexer->lookahead == '{') {
            // Check for escaped brace {{
            lexer->mark_end(lexer);
            advance(lexer);
            if (lexer->lookahead == '{') {
                // {{ is an escaped brace, include both in content
                advance(lexer);
                has_content = true;
                continue;
            }
            // Single { starts interpolation - stop before it
            // We already marked the end before advancing past {
            lexer->result_symbol = FORMAT_STRING_CONTENT;
            return has_content;
        }
        if (lexer->lookahead == '}') {
            // Check for escaped brace }}
            lexer->mark_end(lexer);
            advance(lexer);
            if (lexer->lookahead == '}') {
                // }} is an escaped brace, include both in content
                advance(lexer);
                has_content = true;
                continue;
            }
            // Single } ends interpolation - stop before it
            lexer->result_symbol = FORMAT_STRING_CONTENT;
            return has_content;
        }
        has_content = true;
        advance(lexer);
    }
    lexer->result_symbol = FORMAT_STRING_CONTENT;
    lexer->mark_end(lexer);
    return has_content;
}

// Scan float literal, disambiguating from integer.method()
static inline bool scan_float_literal(TSLexer *lexer) {
    lexer->result_symbol = FLOAT_LITERAL;

    advance(lexer);
    while (is_num_char(lexer->lookahead)) {
        advance(lexer);
    }

    bool has_fraction = false, has_exponent = false;

    if (lexer->lookahead == '.') {
        has_fraction = true;
        advance(lexer);
        // If dot is followed by a letter, it's integer.method() not a float
        if (iswalpha(lexer->lookahead) || lexer->lookahead == '_') {
            return false;
        }
        // If followed by another dot, it's a range: 1..2
        if (lexer->lookahead == '.') {
            return false;
        }
        // If followed by *, it's a deref: expr.*
        if (lexer->lookahead == '*') {
            return false;
        }
        while (is_num_char(lexer->lookahead)) {
            advance(lexer);
        }
    }

    lexer->mark_end(lexer);

    if (lexer->lookahead == 'e' || lexer->lookahead == 'E') {
        has_exponent = true;
        advance(lexer);
        if (lexer->lookahead == '+' || lexer->lookahead == '-') {
            advance(lexer);
        }
        if (!is_num_char(lexer->lookahead)) {
            return true;
        }
        advance(lexer);
        while (is_num_char(lexer->lookahead)) {
            advance(lexer);
        }
        lexer->mark_end(lexer);
    }

    if (!has_exponent && !has_fraction) {
        return false;
    }

    // Check for imaginary suffix - don't consume it, let grammar handle it
    // But do accept the float part
    return true;
}

// Scan open angle bracket for type arguments in call expressions.
// Looks ahead to verify the pattern: < types... > (
// This disambiguates generic calls like foo<int>(x) from comparisons like foo < bar.
// The emitted token is just '<' (one character), but the scanner peeks ahead to decide.
static inline bool scan_open_angle(TSLexer *lexer) {
    if (lexer->lookahead != '<') return false;

    // Consume '<' and mark it as the token end
    advance(lexer);
    lexer->mark_end(lexer);
    lexer->result_symbol = OPEN_ANGLE;

    // Now look ahead (without updating mark_end) to check for: types... > (
    // Use skip() since we're past mark_end — these chars won't be part of the token.
    int depth = 1;
    bool has_content = false;
    int budget = 256; // Bail out after 256 characters to avoid unbounded lookahead

    while (!lexer->eof(lexer) && depth > 0 && budget-- > 0) {
        // Skip whitespace
        while (lexer->lookahead == ' ' || lexer->lookahead == '\t' ||
               lexer->lookahead == '\n' || lexer->lookahead == '\r') {
            skip(lexer);
        }

        if (lexer->lookahead == '<') {
            depth++;
            has_content = true;
            skip(lexer);
        } else if (lexer->lookahead == '>') {
            depth--;
            if (depth == 0) {
                // Found closing '>' at top level — peek past it for '('
                skip(lexer);
                while (lexer->lookahead == ' ' || lexer->lookahead == '\t' ||
                       lexer->lookahead == '\n' || lexer->lookahead == '\r') {
                    skip(lexer);
                }
                return lexer->lookahead == '(' && has_content;
            }
            skip(lexer);
        } else if (lexer->lookahead == '(' || lexer->lookahead == ')' ||
                   lexer->lookahead == '{' || lexer->lookahead == '}' ||
                   lexer->lookahead == ';' || lexer->lookahead == '=' ||
                   lexer->lookahead == '!' || lexer->lookahead == '/' ||
                   lexer->lookahead == '"' || lexer->lookahead == '\'') {
            // Characters that can't appear in type arguments
            return false;
        } else if (lexer->lookahead == 0) {
            return false;
        } else {
            has_content = true;
            skip(lexer);
        }
    }

    return false;
}

// Scan automatic semicolon insertion
// When the parser expects a semicolon (AUTOMATIC_SEMICOLON is valid),
// check if there's a newline in the whitespace. If so, emit the ASI token.
static inline bool scan_automatic_semicolon(TSLexer *lexer) {
    lexer->result_symbol = AUTOMATIC_SEMICOLON;
    lexer->mark_end(lexer);

    bool found_newline = false;

    for (;;) {
        if (lexer->lookahead == ' ' || lexer->lookahead == '\t' || lexer->lookahead == '\r') {
            skip(lexer);
        } else if (lexer->lookahead == '\n') {
            skip(lexer);
            found_newline = true;
        } else if (lexer->lookahead == '/') {
            // Peek ahead for line comment
            skip(lexer);
            if (lexer->lookahead == '/') {
                // Line comment - skip to end of line
                while (lexer->lookahead != '\n' && !lexer->eof(lexer)) {
                    skip(lexer);
                }
                // After comment, continue scanning whitespace
            } else {
                // Not a comment — bare '/' (division) is a continuation token.
                // Return false so the lexer resets and '/' is lexed normally.
                return false;
            }
        } else {
            break;
        }
    }

    if (lexer->eof(lexer)) {
        return true;
    }

    if (!found_newline) {
        return false;
    }

    // Don't insert ASI before tokens that continue an expression.
    // This matches Lisette's `continues_expression` function in the compiler lexer.
    switch (lexer->lookahead) {
        // Single-char operators that always continue
        case '+': // +, +=
        case '*': // *, *=
        case '%': // %, %=
        case '?': // try postfix
        case '=': // =, ==, =>
        case '<': // <, <=
        case '>': // >, >=
        case '{': // block/struct literal continuation
            return false;

        case '-': // only -= continues (bare - could be unary negation)
            advance(lexer);
            return lexer->lookahead != '=';

        // '/' is handled in the whitespace loop above (distinguishes // comments)

        case '.': // . continues (field access), but .. is range (can start expr)
            advance(lexer);
            return lexer->lookahead == '.';

        case '|': // |> and || continue, but bare | is closure
            advance(lexer);
            return lexer->lookahead != '>' && lexer->lookahead != '|';

        case '&': // && continues, but bare & is reference (prefix)
            advance(lexer);
            return lexer->lookahead != '&';

        case '!': // != continues, but bare ! is unary not
            advance(lexer);
            return lexer->lookahead != '=';

        default:
            return true;
    }
}

// Scan `!` as an external token to avoid internal lexer conflict with `!=`
static inline bool scan_bang(TSLexer *lexer) {
    if (lexer->lookahead != '!') return false;
    advance(lexer);
    lexer->mark_end(lexer);
    lexer->result_symbol = BANG;
    return true;
}

bool tree_sitter_lisette_external_scanner_scan(
    void *payload,
    TSLexer *lexer,
    const bool *valid_symbols
) {
    if (valid_symbols[ERROR_SENTINEL]) {
        return false;
    }

    // String content takes priority when valid (we're inside a string)
    if (valid_symbols[STRING_CONTENT] && !valid_symbols[FLOAT_LITERAL]) {
        return scan_string_content(lexer);
    }

    // Format string content
    if (valid_symbols[FORMAT_STRING_CONTENT] && !valid_symbols[FLOAT_LITERAL]) {
        return scan_format_string_content(lexer);
    }

    // ASI must run before whitespace is consumed (it needs to detect newlines).
    // But OPEN_ANGLE needs whitespace consumed first. So: if ASI is valid,
    // run it first — it handles its own whitespace scanning.
    // If ASI returns false (no newline), fall through to try OPEN_ANGLE.
    if (valid_symbols[AUTOMATIC_SEMICOLON]) {
        if (scan_automatic_semicolon(lexer)) {
            return true;
        }
        // ASI didn't fire (no newline). Lexer has been advanced past whitespace
        // via skip() calls. Now try other tokens at the current position.
    } else {
        // No ASI valid — skip whitespace manually for other scanners
        while (iswspace(lexer->lookahead)) {
            skip(lexer);
        }
    }

    // Bang (!) external token to avoid !/!= lex conflict
    if (valid_symbols[BANG] && lexer->lookahead == '!') {
        return scan_bang(lexer);
    }

    // Float literal disambiguation
    if (valid_symbols[FLOAT_LITERAL] && iswdigit(lexer->lookahead)) {
        return scan_float_literal(lexer);
    }

    // Open angle bracket for generic call type arguments
    if (valid_symbols[OPEN_ANGLE] && lexer->lookahead == '<') {
        return scan_open_angle(lexer);
    }

    return false;
}
