package bridge

import (
	"bytes"
	"errors"
	"fmt"
	"io"
	"math"
	"strconv"
	"strings"
	"unicode/utf8"
)

type DecodeLimits struct {
	FileBytes                   int64
	StringBytes, Entries, Depth int
}

func SavedStateLimits() DecodeLimits {
	return DecodeLimits{FileBytes: 256 << 20, StringBytes: 64 << 20, Entries: 2000000, Depth: 200}
}

// LiteralTable preserves Lua key types; numeric 1 and string "1" cannot collide.
// Only literal data is accepted. No Lua interpreter is used to read disk state.
type LiteralTable map[any]any

// ReadToolkitState is the only game database entrypoint. A syntactically valid
// legacy root still fails: fresh Toolkit state never imports or aliases it.
func ReadToolkitState(reader io.Reader, limits DecodeLimits) (LiteralTable, error) {
	globals, err := DecodeSavedState(reader, limits)
	if err != nil {
		return nil, err
	}
	table, ok := globals["LycheeToolkitDB"].(LiteralTable)
	if !ok || len(globals) != 1 {
		return nil, errors.New("bridge.invalid_database_root")
	}
	return table, nil
}

func DecodeSavedState(reader io.Reader, limits DecodeLimits) (map[string]any, error) {
	if reader == nil || limits.FileBytes < 1 || limits.FileBytes > 256<<20 || limits.StringBytes < 1 || limits.StringBytes > 64<<20 || limits.Entries < 1 || limits.Depth < 1 || limits.Depth > 200 {
		return nil, errors.New("bridge.invalid_decode_budget")
	}
	data, err := io.ReadAll(io.LimitReader(reader, limits.FileBytes+1))
	if err != nil {
		return nil, err
	}
	if int64(len(data)) > limits.FileBytes {
		return nil, errors.New("bridge.saved_state_limit")
	}
	if !utf8.Valid(data) {
		return nil, errors.New("bridge.saved_state_encoding")
	}
	p := literalDecoder{data: data, limits: limits}
	globals := make(map[string]any)
	seen := make(map[string]bool)
	for p.space(); p.pos < len(p.data); p.space() {
		name := p.name()
		if name == "" || seen[name] || !p.take('=') {
			return nil, p.failure("invalid global assignment")
		}
		seen[name] = true
		if err := p.countEntry(); err != nil {
			return nil, err
		}
		value, err := p.value(0)
		if err != nil {
			return nil, err
		}
		if value != nil {
			globals[name] = value
		}
		p.take(';')
	}
	return globals, nil
}

type literalDecoder struct {
	data         []byte
	pos, entries int
	limits       DecodeLimits
}

func (p *literalDecoder) failure(message string) error {
	return fmt.Errorf("bridge.invalid_saved_state at byte %d: %s", p.pos, message)
}
func (p *literalDecoder) space() {
	for p.pos < len(p.data) {
		if strings.ContainsRune(" \t\r\n", rune(p.data[p.pos])) {
			p.pos++
			continue
		}
		if bytes.HasPrefix(p.data[p.pos:], []byte("--")) {
			// WoW may annotate array entries with line comments. Long bracket
			// comments are intentionally outside the generated-data grammar.
			p.pos += 2
			for p.pos < len(p.data) && p.data[p.pos] != '\n' {
				p.pos++
			}
			continue
		}
		break
	}
}
func (p *literalDecoder) take(c byte) bool {
	p.space()
	if p.pos < len(p.data) && p.data[p.pos] == c {
		p.pos++
		return true
	}
	return false
}
func (p *literalDecoder) name() string {
	p.space()
	start := p.pos
	for p.pos < len(p.data) {
		c := p.data[p.pos]
		if c == '_' || c >= 'a' && c <= 'z' || c >= 'A' && c <= 'Z' || p.pos > start && c >= '0' && c <= '9' {
			p.pos++
		} else {
			break
		}
	}
	return string(p.data[start:p.pos])
}
func (p *literalDecoder) countEntry() error {
	p.entries++
	if p.entries > p.limits.Entries {
		return p.failure("entry budget exceeded")
	}
	return nil
}
func (p *literalDecoder) value(depth int) (any, error) {
	p.space()
	if depth > p.limits.Depth {
		return nil, p.failure("depth budget exceeded")
	}
	if p.pos == len(p.data) {
		return nil, p.failure("missing value")
	}
	switch p.data[p.pos] {
	case '{':
		return p.table(depth)
	case '"', '\'':
		return p.quoted()
	}
	start := p.pos
	if c := p.data[p.pos]; c == '-' || c == '.' || c >= '0' && c <= '9' {
		for p.pos < len(p.data) && strings.ContainsRune("0123456789.eE+-", rune(p.data[p.pos])) {
			p.pos++
		}
		n, err := strconv.ParseFloat(string(p.data[start:p.pos]), 64)
		if err != nil || math.IsNaN(n) || math.IsInf(n, 0) {
			return nil, p.failure("invalid number")
		}
		return n, nil
	}
	name := p.name()
	switch name {
	case "true":
		return true, nil
	case "false":
		return false, nil
	case "nil":
		return nil, nil
	}
	return nil, p.failure("expected literal")
}
func (p *literalDecoder) table(depth int) (LiteralTable, error) {
	p.pos++
	result := make(LiteralTable)
	seen := make(map[any]bool)
	arrayIndex := float64(1)
	for !p.take('}') {
		if err := p.countEntry(); err != nil {
			return nil, err
		}
		var key, value any
		var err error
		if p.take('[') {
			key, err = p.value(depth + 1)
			if err != nil {
				return nil, err
			}
			if !p.take(']') || !p.take('=') {
				return nil, p.failure("invalid keyed field")
			}
			value, err = p.value(depth + 1)
		} else {
			saved := p.pos
			name := p.name()
			if name != "" && p.take('=') {
				key = name
				value, err = p.value(depth + 1)
			} else {
				p.pos = saved
				key = arrayIndex
				arrayIndex++
				value, err = p.value(depth + 1)
			}
		}
		if err != nil {
			return nil, err
		}
		switch key.(type) {
		case string, float64, bool:
		default:
			return nil, p.failure("invalid table key")
		}
		if seen[key] {
			return nil, p.failure("duplicate table key")
		}
		seen[key] = true
		if value != nil {
			result[key] = value
		}
		if p.take('}') {
			return result, nil
		}
		if !p.take(',') && !p.take(';') {
			return nil, p.failure("missing table separator")
		}
	}
	return result, nil
}
func (p *literalDecoder) quoted() (string, error) {
	quote := p.data[p.pos]
	p.pos++
	var out []byte
	for p.pos < len(p.data) {
		c := p.data[p.pos]
		p.pos++
		if c == quote {
			if !utf8.Valid(out) {
				return "", p.failure("invalid string encoding")
			}
			return string(out), nil
		}
		if c == '\r' || c == '\n' {
			return "", p.failure("unescaped newline")
		}
		if c == '\\' {
			if p.pos == len(p.data) {
				return "", p.failure("incomplete escape")
			}
			c = p.data[p.pos]
			p.pos++
			switch c {
			case 'a':
				c = 7
			case 'b':
				c = 8
			case 'f':
				c = 12
			case 'n':
				c = 10
			case 'r':
				c = 13
			case 't':
				c = 9
			case 'v':
				c = 11
			case '\\', '"', '\'':
			case '\n':
				c = 10
			case '\r':
				if p.pos < len(p.data) && p.data[p.pos] == '\n' {
					p.pos++
				}
				c = 10
			default:
				if c < '0' || c > '9' {
					return "", p.failure("unsupported escape")
				}
				n := int(c - '0')
				for digits := 1; digits < 3 && p.pos < len(p.data) && p.data[p.pos] >= '0' && p.data[p.pos] <= '9'; digits++ {
					n = n*10 + int(p.data[p.pos]-'0')
					p.pos++
				}
				if n > 255 {
					return "", p.failure("escape byte overflow")
				}
				c = byte(n)
			}
		}
		if len(out) >= p.limits.StringBytes {
			return "", p.failure("string budget exceeded")
		}
		out = append(out, c)
	}
	return "", p.failure("unterminated string")
}
