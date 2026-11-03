// Package publish implements author-platform work independently of game Live
// and managed local Delivery. Input contracts are also used by CLI schema.
package publish

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"math"
	"strconv"
	"strings"
	"unicode/utf8"
)

const MaxInputBytes = 2 << 20

type Error struct {
	Code    string `json:"code"`
	Path    string `json:"path,omitempty"`
	Message string `json:"message"`
	Exit    int    `json:"-"`
	Details any    `json:"details,omitempty"`
}

func (e *Error) Error() string {
	if e.Path != "" {
		return e.Code + ": " + e.Path + ": " + e.Message
	}
	return e.Code + ": " + e.Message
}
func invalid(path, message string) error {
	return &Error{Code: "publish.input", Path: path, Message: message, Exit: 2}
}
func capability(code, message string) error { return &Error{Code: code, Message: message, Exit: 3} }
func failure(code, message string) error    { return &Error{Code: code, Message: message, Exit: 5} }

type Request struct {
	Home      string
	RequestID string
	Input     []byte
	Plan      bool
}
type Result struct {
	Platform       string `json:"platform"`
	Action         string `json:"action"`
	Complete       bool   `json:"complete"`
	OperationID    string `json:"operationId,omitempty"`
	Submission     string `json:"submission,omitempty"`
	Verification   string `json:"verification,omitempty"`
	PendingCleanup bool   `json:"pendingCleanup"`
	Data           any    `json:"data"`
}

// Field is the one field definition used for both admission and explanation.
// Omitting a field is distinct from providing a null, empty or zero value.
type Field struct {
	Type        string           `json:"type"`
	Description string           `json:"description"`
	Nullable    bool             `json:"nullable,omitempty"`
	Nonempty    bool             `json:"nonempty,omitempty"`
	Minimum     *int64           `json:"minimum,omitempty"`
	Maximum     *int64           `json:"maximum,omitempty"`
	MaxLength   int              `json:"maxLength,omitempty"`
	Enum        []string         `json:"enum,omitempty"`
	Items       *Field           `json:"items,omitempty"`
	Properties  map[string]Field `json:"properties,omitempty"`
	Required    []string         `json:"required,omitempty"`
}
type InputSpec struct {
	Properties        map[string]Field    `json:"properties"`
	Required          []string            `json:"required"`
	ExactlyOne        [][]string          `json:"exactlyOne,omitempty"`
	MutuallyExclusive [][]string          `json:"mutuallyExclusive,omitempty"`
	Requires          map[string][]string `json:"requires,omitempty"`
	Example           map[string]any      `json:"example"`
	Effects           []string            `json:"effects"`
	DynamicOptions    map[string]string   `json:"dynamicOptions,omitempty"`
}

func Int(n int64) *int64 { return &n }
func (s InputSpec) Decode(raw []byte) (map[string]any, error) {
	if len(raw) == 0 {
		raw = []byte("{}")
	}
	if len(raw) > MaxInputBytes || !utf8.Valid(raw) {
		return nil, invalid("$", "input must be UTF-8 JSON within 2 MiB")
	}
	d := json.NewDecoder(bytes.NewReader(raw))
	d.UseNumber()
	count := 0
	v, err := decodeValue(d, "$", 0, &count)
	if err != nil {
		return nil, err
	}
	if _, err = d.Token(); err != io.EOF {
		return nil, invalid("$", "trailing JSON input")
	}
	object, ok := v.(map[string]any)
	if !ok {
		return nil, invalid("$", "expected an object")
	}
	if err = validateObject(object, s.Properties, s.Required, "$"); err != nil {
		return nil, err
	}
	for _, group := range s.ExactlyOne {
		if presentCount(object, group) != 1 {
			return nil, invalid("$", "provide exactly one of "+strings.Join(group, ", "))
		}
	}
	for _, group := range s.MutuallyExclusive {
		if presentCount(object, group) > 1 {
			return nil, invalid("$", "fields are mutually exclusive: "+strings.Join(group, ", "))
		}
	}
	for key, required := range s.Requires {
		if _, ok := object[key]; ok {
			for _, name := range required {
				if _, ok := object[name]; !ok {
					return nil, invalid("$."+name, "required when "+key+" is present")
				}
			}
		}
	}
	return object, nil
}
func presentCount(v map[string]any, names []string) int {
	n := 0
	for _, name := range names {
		if _, ok := v[name]; ok {
			n++
		}
	}
	return n
}
func decodeValue(d *json.Decoder, path string, depth int, count *int) (any, error) {
	if depth > 32 {
		return nil, invalid(path, "maximum nesting depth is 32")
	}
	*count++
	if *count > 100000 {
		return nil, invalid(path, "too many JSON values")
	}
	t, err := d.Token()
	if err != nil {
		return nil, invalid(path, "invalid JSON")
	}
	delim, ok := t.(json.Delim)
	if !ok {
		return t, nil
	}
	switch delim {
	case '{':
		result := map[string]any{}
		for d.More() {
			key, err := d.Token()
			if err != nil {
				return nil, invalid(path, "invalid object key")
			}
			name, ok := key.(string)
			if !ok {
				return nil, invalid(path, "invalid object key")
			}
			if _, ok := result[name]; ok {
				return nil, invalid(path+"."+name, "duplicate field")
			}
			v, err := decodeValue(d, path+"."+name, depth+1, count)
			if err != nil {
				return nil, err
			}
			result[name] = v
		}
		if close, err := d.Token(); err != nil || close != json.Delim('}') {
			return nil, invalid(path, "unclosed object")
		}
		return result, nil
	case '[':
		result := []any{}
		for d.More() {
			v, err := decodeValue(d, fmt.Sprintf("%s[%d]", path, len(result)), depth+1, count)
			if err != nil {
				return nil, err
			}
			result = append(result, v)
		}
		if close, err := d.Token(); err != nil || close != json.Delim(']') {
			return nil, invalid(path, "unclosed array")
		}
		return result, nil
	default:
		return nil, invalid(path, "invalid container")
	}
}
func validateObject(v map[string]any, fields map[string]Field, required []string, path string) error {
	for _, name := range required {
		if _, ok := v[name]; !ok {
			return invalid(path+"."+name, "required field")
		}
	}
	for name, value := range v {
		field, ok := fields[name]
		if !ok {
			return invalid(path+"."+name, "unknown field")
		}
		if err := field.validate(value, path+"."+name); err != nil {
			return err
		}
	}
	return nil
}
func (f Field) validate(v any, path string) error {
	if v == nil {
		if f.Nullable {
			return nil
		}
		return invalid(path, "null is not allowed")
	}
	var length int
	switch f.Type {
	case "string":
		s, ok := v.(string)
		if !ok {
			return invalid(path, "expected string")
		}
		length = utf8.RuneCountInString(s)
		if f.Nonempty && strings.TrimSpace(s) == "" {
			return invalid(path, "must not be empty")
		}
		if len(f.Enum) > 0 {
			found := false
			for _, choice := range f.Enum {
				found = found || s == choice
			}
			if !found {
				return invalid(path, "expected one of "+strings.Join(f.Enum, ", "))
			}
		}
	case "integer":
		n, ok := v.(json.Number)
		if !ok {
			return invalid(path, "expected integer")
		}
		i, err := strconv.ParseInt(string(n), 10, 64)
		if err != nil {
			return invalid(path, "expected signed 64-bit integer")
		}
		if f.Minimum != nil && i < *f.Minimum || f.Maximum != nil && i > *f.Maximum {
			return invalid(path, "integer outside allowed range")
		}
	case "number":
		n, ok := v.(json.Number)
		if !ok {
			return invalid(path, "expected number")
		}
		x, err := n.Float64()
		if err != nil || math.IsInf(x, 0) || math.IsNaN(x) {
			return invalid(path, "expected finite number")
		}
		if f.Minimum != nil && x < float64(*f.Minimum) || f.Maximum != nil && x > float64(*f.Maximum) {
			return invalid(path, "number outside allowed range")
		}
	case "boolean":
		if _, ok := v.(bool); !ok {
			return invalid(path, "expected boolean")
		}
	case "array":
		a, ok := v.([]any)
		if !ok {
			return invalid(path, "expected array")
		}
		length = len(a)
		if f.Nonempty && length == 0 {
			return invalid(path, "array must not be empty")
		}
		for i, item := range a {
			if f.Items != nil {
				if err := f.Items.validate(item, fmt.Sprintf("%s[%d]", path, i)); err != nil {
					return err
				}
			}
		}
	case "object":
		o, ok := v.(map[string]any)
		if !ok {
			return invalid(path, "expected object")
		}
		length = len(o)
		if err := validateObject(o, f.Properties, f.Required, path); err != nil {
			return err
		}
	default:
		return errors.New("publish: invalid field contract")
	}
	if f.MaxLength > 0 && length > f.MaxLength {
		return invalid(path, "value exceeds allowed length")
	}
	return nil
}
func String(v map[string]any, key string) string { s, _ := v[key].(string); return s }
func Integer(v map[string]any, key string) int64 {
	n, _ := v[key].(json.Number)
	i, _ := n.Int64()
	return i
}
func Bool(v map[string]any, key string) bool { b, _ := v[key].(bool); return b }
