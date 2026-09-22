// Package relational compiles and evaluates the toolkit's read-only SQL dialect.
package relational

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"unicode"
	"unicode/utf8"
)

var ErrSyntax = errors.New("relational.invalid_syntax")
var ErrBudget = errors.New("relational.budget_exceeded")

type Diagnostic struct {
	Cause  error
	Offset int
	Line   int
	Column int
	Detail string
}

func (e *Diagnostic) Error() string {
	return fmt.Sprintf("%v at %d:%d: %s", e.Cause, e.Line, e.Column, e.Detail)
}
func (e *Diagnostic) Unwrap() error { return e.Cause }

type lexeme struct {
	kind   byte
	text   string
	offset int
}

const (
	word      byte = 'w'
	quoted    byte = 'q'
	number    byte = 'n'
	textValue byte = 's'
	parameter byte = 'p'
	symbol    byte = 'o'
	end       byte = 'e'
)

func problem(source string, at int, cause error, detail string) error {
	prefix := source[:at]
	line := strings.Count(prefix, "\n") + 1
	column := utf8.RuneCountInString(prefix[strings.LastIndexByte(prefix, '\n')+1:]) + 1
	return &Diagnostic{Cause: cause, Offset: at, Line: line, Column: column, Detail: detail}
}

func tokenize(ctx context.Context, source string) ([]lexeme, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if len(source) > 256<<10 {
		return nil, ErrBudget
	}
	if !utf8.ValidString(source) || strings.IndexByte(source, 0) >= 0 {
		return nil, ErrSyntax
	}
	pieces := make([]lexeme, 0)
	for pos := 0; pos < len(source); {
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		if len(pieces) >= 32768 {
			return nil, ErrBudget
		}
		start := pos
		r, width := utf8.DecodeRuneInString(source[pos:])
		if unicode.IsSpace(r) {
			pos += width
			continue
		}
		if strings.HasPrefix(source[pos:], "--") {
			if n := strings.IndexByte(source[pos:], '\n'); n >= 0 {
				pos += n + 1
			} else {
				pos = len(source)
			}
			continue
		}
		if strings.HasPrefix(source[pos:], "/*") {
			depth := 1
			pos += 2
			for pos < len(source) && depth > 0 {
				if strings.HasPrefix(source[pos:], "/*") {
					depth++
					pos += 2
					if depth > 16 {
						return nil, ErrBudget
					}
				} else if strings.HasPrefix(source[pos:], "*/") {
					depth--
					pos += 2
				} else {
					pos++
				}
			}
			if depth != 0 {
				return nil, problem(source, start, ErrSyntax, "unclosed comment")
			}
			continue
		}
		if r == '\'' || r == '"' || r == '`' || r == '[' {
			closing := byte(r)
			kind := quoted
			if r == '\'' {
				kind = textValue
			}
			if r == '[' {
				closing = ']'
			}
			pos++
			var decoded strings.Builder
			closed := false
			for pos < len(source) {
				if source[pos] == closing {
					pos++
					if pos < len(source) && source[pos] == closing {
						decoded.WriteByte(closing)
						pos++
						continue
					}
					closed = true
					break
				}
				decoded.WriteByte(source[pos])
				pos++
			}
			if !closed {
				return nil, problem(source, start, ErrSyntax, "unclosed quoted value")
			}
			if kind == quoted && decoded.Len() == 0 {
				return nil, problem(source, start, ErrSyntax, "empty identifier")
			}
			pieces = append(pieces, lexeme{kind, decoded.String(), start})
			continue
		}
		if r == ':' {
			pos++
			begin := pos
			for pos < len(source) {
				r, width = utf8.DecodeRuneInString(source[pos:])
				if !nameRune(r, pos > begin) {
					break
				}
				pos += width
			}
			if begin == pos {
				return nil, problem(source, start, ErrSyntax, "missing parameter name")
			}
			pieces = append(pieces, lexeme{parameter, source[begin:pos], start})
			continue
		}
		if nameRune(r, false) {
			pos += width
			for pos < len(source) {
				r, width = utf8.DecodeRuneInString(source[pos:])
				if !nameRune(r, true) {
					break
				}
				pos += width
			}
			pieces = append(pieces, lexeme{word, source[start:pos], start})
			continue
		}
		if r >= '0' && r <= '9' || r == '.' && pos+1 < len(source) && source[pos+1] >= '0' && source[pos+1] <= '9' {
			for pos < len(source) && source[pos] >= '0' && source[pos] <= '9' {
				pos++
			}
			if pos < len(source) && source[pos] == '.' {
				pos++
				for pos < len(source) && source[pos] >= '0' && source[pos] <= '9' {
					pos++
				}
			}
			if pos < len(source) && (source[pos] == 'e' || source[pos] == 'E') {
				pos++
				if pos < len(source) && (source[pos] == '+' || source[pos] == '-') {
					pos++
				}
				exponent := pos
				for pos < len(source) && source[pos] >= '0' && source[pos] <= '9' {
					pos++
				}
				if exponent == pos {
					return nil, problem(source, start, ErrSyntax, "missing exponent")
				}
			}
			pieces = append(pieces, lexeme{number, source[start:pos], start})
			continue
		}
		if strings.ContainsRune(",.()*+-/%=<>!;|", r) {
			pos++
			if pos < len(source) && ((r == '<' && (source[pos] == '=' || source[pos] == '>')) || (r == '>' || r == '!') && source[pos] == '=' || r == '|' && source[pos] == '|') {
				pos++
			}
			value := source[start:pos]
			if value == "!" || value == "|" {
				return nil, problem(source, start, ErrSyntax, "unsupported operator")
			}
			pieces = append(pieces, lexeme{symbol, value, start})
			continue
		}
		return nil, problem(source, start, ErrSyntax, "unexpected character")
	}
	return append(pieces, lexeme{end, "", len(source)}), nil
}
func nameRune(r rune, continuation bool) bool {
	return r == '_' || unicode.IsLetter(r) || continuation && (unicode.IsDigit(r) || r == '$')
}
