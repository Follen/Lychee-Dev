// SPDX-License-Identifier: MIT
// DBD parsing adapted from wowdata; see LICENSE.
package schema

import (
	"bufio"
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"regexp"
	"strconv"
	"strings"
	"unicode/utf8"
)

var (
	ErrFormat    = errors.New("schema.invalid_definition")
	ErrLimit     = errors.New("schema.limit_exceeded")
	ErrMissing   = errors.New("schema.definition_not_found")
	ErrAmbiguous = errors.New("schema.ambiguous_definition")
)

type Field struct {
	Name          string `json:"name"`
	Kind          string `json:"kind"`
	ForeignTable  string `json:"foreignTable"`
	ForeignColumn string `json:"foreignColumn"`
	Verified      bool   `json:"verified"`
	Bits          uint16 `json:"bits"`
	Signed        bool   `json:"signed"`
	Elements      uint32 `json:"elements"`
	Array         bool   `json:"array"`
	Identity      bool   `json:"identity"`
	Inline        bool   `json:"inline"`
	Relation      bool   `json:"relation"`
}

type Definition struct {
	SHA256 string  `json:"sha256"`
	Match  string  `json:"match"`
	Build  string  `json:"build"`
	Layout string  `json:"layout"`
	Fields []Field `json:"fields"`
}
type Document struct {
	digest   string
	variants []variant
}
type version [4]uint32
type interval struct{ low, high version }
type variant struct {
	builds  []interval
	layouts []string
	fields  []Field
}

var columnLine = regexp.MustCompile(`^(int|float|string|locstring)(?:<([A-Za-z_][A-Za-z0-9_]*)::([A-Za-z_][A-Za-z0-9_]*)>)?\s+([A-Za-z_][A-Za-z0-9_]*)(\?)?$`)
var fieldLine = regexp.MustCompile(`^(?:\$([^$]+)\$)?([A-Za-z_][A-Za-z0-9_]*)(?:<([uf]?)([0-9]+)>)?(?:\[([0-9]+)\])?$`)

// Parse retains the raw-byte identity without storing caller-owned buffers.
// Input is bounded to 4 MiB, 4096 variants/columns/fields per variant, and 64 KiB
// lines. Unknown syntax is an error, not a silently omitted column.
func Parse(ctx context.Context, raw []byte) (*Document, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if len(raw) > 4<<20 {
		return nil, ErrLimit
	}
	if len(raw) == 0 || !utf8.Valid(raw) || bytes.IndexByte(raw, 0) >= 0 {
		return nil, ErrFormat
	}
	sum := sha256.Sum256(raw)
	doc := &Document{digest: hex.EncodeToString(sum[:])}
	columns := make(map[string]Field)
	var chunk []string
	flush := func() error {
		if len(chunk) == 0 {
			return nil
		}
		if chunk[0] == "COLUMNS" {
			if len(columns) != 0 || len(doc.variants) != 0 || len(chunk) == 1 {
				return ErrFormat
			}
			if len(chunk) > 4097 {
				return ErrLimit
			}
			for _, line := range chunk[1:] {
				m := columnLine.FindStringSubmatch(line)
				if m == nil {
					return ErrFormat
				}
				if _, exists := columns[m[4]]; exists {
					return ErrFormat
				}
				columns[m[4]] = Field{Name: m[4], Kind: m[1], ForeignTable: m[2], ForeignColumn: m[3], Verified: m[5] == "", Inline: true, Signed: true, Elements: 1}
			}
		} else {
			if len(columns) == 0 {
				return ErrFormat
			}
			if len(doc.variants) >= 4096 {
				return ErrLimit
			}
			v, err := parseVariant(ctx, chunk, columns)
			if err != nil {
				return err
			}
			doc.variants = append(doc.variants, v)
		}
		chunk = nil
		return nil
	}
	scanner := bufio.NewScanner(bytes.NewReader(raw))
	scanner.Buffer(make([]byte, 4096), 64<<10)
	for scanner.Scan() {
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		line := strings.TrimSpace(scanner.Text())
		if line == "" {
			if err := flush(); err != nil {
				return nil, err
			}
			continue
		}
		if comment := strings.Index(line, "//"); comment >= 0 {
			line = strings.TrimSpace(line[:comment])
		}
		if line == "" {
			continue
		}
		chunk = append(chunk, line)
		if len(chunk) > 16384 {
			return nil, ErrLimit
		}
	}
	if scanner.Err() != nil {
		return nil, ErrLimit
	}
	if err := flush(); err != nil {
		return nil, err
	}
	if len(columns) == 0 || len(doc.variants) == 0 {
		return nil, ErrFormat
	}
	return doc, nil
}

func parseVariant(ctx context.Context, lines []string, columns map[string]Field) (variant, error) {
	var out variant
	seen := make(map[string]bool)
	identity := false
	layoutSeen := false
	for _, line := range lines {
		if err := ctx.Err(); err != nil {
			return variant{}, err
		}
		if strings.HasPrefix(line, "COMMENT ") {
			continue
		}
		if strings.HasPrefix(line, "BUILD ") {
			if len(out.fields) > 0 {
				return variant{}, ErrFormat
			}
			for _, part := range strings.Split(strings.TrimPrefix(line, "BUILD "), ",") {
				ends := strings.Split(strings.TrimSpace(part), "-")
				if len(ends) > 2 {
					return variant{}, ErrFormat
				}
				low, err := parseVersion(strings.TrimSpace(ends[0]))
				if err != nil {
					return variant{}, err
				}
				high := low
				if len(ends) == 2 {
					high, err = parseVersion(strings.TrimSpace(ends[1]))
					if err != nil {
						return variant{}, err
					}
				}
				if compare(low, high) > 0 {
					return variant{}, ErrFormat
				}
				out.builds = append(out.builds, interval{low, high})
			}
			continue
		}
		if strings.HasPrefix(line, "LAYOUT ") {
			if layoutSeen || len(out.fields) > 0 {
				return variant{}, ErrFormat
			}
			layoutSeen = true
			for _, value := range strings.Split(strings.TrimPrefix(line, "LAYOUT "), ",") {
				h, err := layoutKey(strings.TrimSpace(value))
				if err != nil {
					return variant{}, err
				}
				out.layouts = append(out.layouts, h)
			}
			continue
		}
		m := fieldLine.FindStringSubmatch(line)
		if m == nil {
			return variant{}, ErrFormat
		}
		field, exists := columns[m[2]]
		if !exists || seen[field.Name] {
			return variant{}, ErrFormat
		}
		seen[field.Name] = true
		annotations := make(map[string]bool)
		if m[1] != "" {
			for _, a := range strings.Split(m[1], ",") {
				a = strings.TrimSpace(a)
				if annotations[a] {
					return variant{}, ErrFormat
				}
				annotations[a] = true
				switch a {
				case "id":
					field.Identity = true
				case "noninline":
					field.Inline = false
				case "relation":
					field.Relation = true
				default:
					return variant{}, ErrFormat
				}
			}
		}
		if field.Identity {
			if identity || field.Kind != "int" {
				return variant{}, ErrFormat
			}
			identity = true
		}
		if !field.Inline && (!field.Identity && !field.Relation || field.Kind != "int") {
			return variant{}, ErrFormat
		}
		if m[4] != "" {
			bits, err := strconv.ParseUint(m[4], 10, 16)
			if err != nil || (bits != 8 && bits != 16 && bits != 32 && bits != 64) {
				return variant{}, ErrFormat
			}
			field.Bits = uint16(bits)
			if field.Kind == "int" {
				if m[3] == "f" {
					return variant{}, ErrFormat
				}
				field.Signed = m[3] != "u"
			} else if field.Kind == "float" {
				if bits != 32 || m[3] == "u" {
					return variant{}, ErrFormat
				}
			} else {
				return variant{}, ErrFormat
			}
		} else {
			if field.Kind == "int" && field.Inline {
				return variant{}, ErrFormat
			}
			if field.Kind == "int" || field.Kind == "float" {
				field.Bits = 32
			}
		}
		if m[5] != "" {
			count, err := strconv.ParseUint(m[5], 10, 32)
			if err != nil || count == 0 || count > 65536 {
				return variant{}, ErrLimit
			}
			field.Elements = uint32(count)
			field.Array = true
		}
		if !field.Inline && field.Array {
			return variant{}, ErrFormat
		}
		out.fields = append(out.fields, field)
		if len(out.fields) > 4096 {
			return variant{}, ErrLimit
		}
	}
	if len(out.fields) == 0 || (len(out.builds) == 0 && len(out.layouts) == 0) {
		return variant{}, ErrFormat
	}
	return out, nil
}

// Select requires an exact full build. A supplied layout is authoritative and
// never falls back to build matching. Multiple matching definitions are errors.
// Returned fields are a copy, safe for callers to bind independently.
func (d *Document) Select(ctx context.Context, build, layout string) (Definition, error) {
	if err := ctx.Err(); err != nil {
		return Definition{}, err
	}
	v, err := parseVersion(build)
	if err != nil {
		return Definition{}, err
	}
	if layout != "" {
		layout, err = layoutKey(layout)
		if err != nil {
			return Definition{}, err
		}
	}
	var selected *variant
	for i := range d.variants {
		if err := ctx.Err(); err != nil {
			return Definition{}, err
		}
		entry := &d.variants[i]
		matches := false
		if layout != "" {
			for _, h := range entry.layouts {
				matches = matches || h == layout
			}
		} else {
			for _, r := range entry.builds {
				matches = matches || (compare(v, r.low) >= 0 && compare(v, r.high) <= 0)
			}
		}
		if matches {
			if selected != nil {
				return Definition{}, ErrAmbiguous
			}
			selected = entry
		}
	}
	if selected == nil {
		return Definition{}, ErrMissing
	}
	mode := "build"
	if layout != "" {
		mode = "layout"
	}
	return Definition{SHA256: d.digest, Match: mode, Build: build, Layout: layout, Fields: append([]Field(nil), selected.fields...)}, nil
}

func layoutKey(value string) (string, error) {
	if len(value) != 8 {
		return "", ErrFormat
	}
	if _, err := hex.DecodeString(value); err != nil {
		return "", ErrFormat
	}
	return strings.ToUpper(value), nil
}
func parseVersion(value string) (version, error) {
	var result version
	parts := strings.Split(value, ".")
	if len(parts) != 4 {
		return result, ErrFormat
	}
	for i, part := range parts {
		if part == "" {
			return result, ErrFormat
		}
		for _, c := range part {
			if c < '0' || c > '9' {
				return result, ErrFormat
			}
		}
		v, err := strconv.ParseUint(part, 10, 32)
		if err != nil {
			return result, ErrFormat
		}
		result[i] = uint32(v)
	}
	return result, nil
}
func compare(a, b version) int {
	for i := range a {
		if a[i] < b[i] {
			return -1
		}
		if a[i] > b[i] {
			return 1
		}
	}
	return 0
}
