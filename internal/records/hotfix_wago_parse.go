// SPDX-License-Identifier: AGPL-3.0-or-later
// Inertia page parsing adapted from wowdata; see THIRD_PARTY_NOTICES.md.
package records

import (
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	stdhtml "html"
	"math"
	"net/url"
	"strconv"
	"strings"

	"github.com/follenfang/lycheedev/internal/records/schema"
)

// WagoParserVersion names the strict Inertia-HTML parser used for cached page
// receipts, so a future parser change cannot silently read old receipts.
const WagoParserVersion = "lycheedev-wago-inertia-v1"

// WagoRecord is one independent hotfix record from a Wago page. Payload bytes
// (the raw JSON column payload) are preserved verbatim in Data.
type WagoRecord struct {
	ID        uint64
	PushID    int32
	RecordID  uint32
	Status    uint8
	Build     string
	TableName string
	Data      json.RawMessage
	CreatedAt string
	RegionID  uint32
	Locale    string
}

// WagoPage is one parsed page: its pagination identity keys plus the records.
type WagoPage struct {
	CurrentPage    int
	LastPage       int
	PerPage        int
	Total          int64
	NextPageURL    string
	Records        []WagoRecord
	ResponseSHA256 string
}

type wagoFlexInt int64

func (v *wagoFlexInt) UnmarshalJSON(b []byte) error {
	if bytes.Equal(b, []byte("null")) {
		*v = 0
		return nil
	}
	text := string(b)
	if len(b) > 0 && b[0] == '"' {
		var s string
		if err := json.Unmarshal(b, &s); err != nil {
			return err
		}
		text = s
	}
	n, err := strconv.ParseInt(text, 10, 64)
	if err != nil {
		return fmt.Errorf("%w: numeric field %q", ErrHotfixWagoProtocol, text)
	}
	*v = wagoFlexInt(n)
	return nil
}

type wagoFlexText string

func (v *wagoFlexText) UnmarshalJSON(b []byte) error {
	if bytes.Equal(b, []byte("null")) {
		*v = ""
		return nil
	}
	if len(b) > 0 && b[0] == '"' {
		var s string
		if err := json.Unmarshal(b, &s); err != nil {
			return err
		}
		*v = wagoFlexText(s)
		return nil
	}
	*v = wagoFlexText(string(b))
	return nil
}

type wagoRow struct {
	ID        wagoFlexInt     `json:"id"`
	PushID    wagoFlexInt     `json:"push_id"`
	RecordID  wagoFlexInt     `json:"record_id"`
	Status    wagoFlexInt     `json:"status"`
	Build     wagoFlexText    `json:"build"`
	TableName string          `json:"table_name"`
	Data      json.RawMessage `json:"data"`
	CreatedAt string          `json:"created_at"`
	RegionID  wagoFlexInt     `json:"region_id"`
	Locale    string          `json:"locale"`
}

type wagoPagination struct {
	CurrentPage int         `json:"current_page"`
	LastPage    int         `json:"last_page"`
	PerPage     int         `json:"per_page"`
	Total       wagoFlexInt `json:"total"`
	Data        []wagoRow   `json:"data"`
	NextPageURL string      `json:"next_page_url"`
}

// ParseWagoPage strictly parses one Inertia-HTML page payload into independent
// records. Anything unexpected in the envelope is a protocol error, never a
// silently empty page.
func ParseWagoPage(raw []byte) (WagoPage, error) {
	embedded, err := findInertiaDataPage(raw)
	if err != nil {
		return WagoPage{}, err
	}
	var root struct {
		Component string `json:"component"`
		Props     struct {
			Hotfixes *wagoPagination `json:"hotfixes"`
		} `json:"props"`
	}
	if err := json.Unmarshal(embedded, &root); err != nil {
		return WagoPage{}, fmt.Errorf("%w: data-page is invalid JSON: %v", ErrHotfixWagoProtocol, err)
	}
	if root.Component != "Hotfixes" {
		return WagoPage{}, fmt.Errorf("%w: component %q is not Hotfixes", ErrHotfixWagoProtocol, root.Component)
	}
	pagination := root.Props.Hotfixes
	if pagination == nil {
		return WagoPage{}, fmt.Errorf("%w: props.hotfixes is missing", ErrHotfixWagoProtocol)
	}
	if pagination.CurrentPage < 1 || pagination.LastPage < pagination.CurrentPage || pagination.PerPage < 1 || pagination.Total < 0 {
		return WagoPage{}, fmt.Errorf("%w: invalid pagination %d/%d/%d/%d", ErrHotfixWagoProtocol, pagination.CurrentPage, pagination.LastPage, pagination.PerPage, int64(pagination.Total))
	}
	page := WagoPage{
		CurrentPage: pagination.CurrentPage,
		LastPage:    pagination.LastPage,
		PerPage:     pagination.PerPage,
		Total:       int64(pagination.Total),
		NextPageURL: pagination.NextPageURL,
		Records:     make([]WagoRecord, 0, len(pagination.Data)),
	}
	sum := sha256.Sum256(raw)
	page.ResponseSHA256 = hex.EncodeToString(sum[:])
	for _, row := range pagination.Data {
		if row.ID < 0 || row.RecordID < 0 || row.RecordID > math.MaxUint32 || row.RegionID < 0 || row.RegionID > math.MaxUint32 || row.Status < 0 || row.Status > 255 {
			return WagoPage{}, fmt.Errorf("%w: record id %d has out of range fields", ErrHotfixWagoProtocol, int64(row.ID))
		}
		if row.PushID < math.MinInt32 || row.PushID > math.MaxInt32 {
			return WagoPage{}, fmt.Errorf("%w: push %d does not fit int32", ErrHotfixWagoProtocol, int64(row.PushID))
		}
		if strings.TrimSpace(row.TableName) == "" || strings.TrimSpace(string(row.Build)) == "" {
			return WagoPage{}, fmt.Errorf("%w: record id %d has no table name or build", ErrHotfixWagoProtocol, int64(row.ID))
		}
		data := row.Data
		if len(data) == 0 || string(data) == "null" {
			data = nil
		}
		page.Records = append(page.Records, WagoRecord{
			ID:        uint64(row.ID),
			PushID:    int32(row.PushID),
			RecordID:  uint32(row.RecordID),
			Status:    uint8(row.Status),
			Build:     string(row.Build),
			TableName: row.TableName,
			Data:      append(json.RawMessage(nil), data...),
			CreatedAt: row.CreatedAt,
			RegionID:  uint32(row.RegionID),
			Locale:    row.Locale,
		})
	}
	return page, nil
}

// ValidateWagoPageSequence enforces page-identity continuity: a page whose
// identity keys or record identity keys do not follow the previous page fails
// instead of silently shifting the window.
func ValidateWagoPageSequence(previous, next WagoPage) error {
	if next.Total != previous.Total || next.LastPage != previous.LastPage {
		return fmt.Errorf("%w: total/last_page drift %d/%d -> %d/%d", ErrHotfixPageDrift, previous.Total, previous.LastPage, next.Total, next.LastPage)
	}
	if next.CurrentPage <= previous.CurrentPage {
		return fmt.Errorf("%w: duplicate or reversed page %d after %d", ErrHotfixPageDrift, next.CurrentPage, previous.CurrentPage)
	}
	if next.CurrentPage != previous.CurrentPage+1 {
		return fmt.Errorf("%w: missing page between %d and %d", ErrHotfixPageDrift, previous.CurrentPage, next.CurrentPage)
	}
	if previous.CurrentPage < previous.LastPage {
		if strings.TrimSpace(previous.NextPageURL) == "" {
			return fmt.Errorf("%w: missing next_page_url after page %d", ErrHotfixPageDrift, previous.CurrentPage)
		}
		parsed, err := url.Parse(previous.NextPageURL)
		if err != nil {
			return fmt.Errorf("%w: invalid next_page_url %q", ErrHotfixPageDrift, previous.NextPageURL)
		}
		pointed, err := strconv.Atoi(parsed.Query().Get("page"))
		if err != nil || pointed != next.CurrentPage {
			return fmt.Errorf("%w: next_page_url of page %d points to %q, expected page %d", ErrHotfixPageDrift, previous.CurrentPage, previous.NextPageURL, next.CurrentPage)
		}
	}
	return nil
}

// findInertiaDataPage extracts the escaped data-page attribute of div#app. The
// production page carries a large head before div#app; the exact id marker is
// located first and the tag is then still parsed and unescaped as a tag.
func findInertiaDataPage(raw []byte) ([]byte, error) {
	if marker := bytes.Index(raw, []byte(`id="app"`)); marker >= 0 {
		if div := bytes.LastIndex(raw[:marker], []byte("<div")); div >= 0 && marker-div <= 4096 {
			if page, ok := inertiaPageFromTag(raw, div); ok {
				return page, nil
			}
		}
	}
	for offset := 0; offset < len(raw); {
		index := bytes.IndexByte(raw[offset:], '<')
		if index < 0 {
			break
		}
		start := offset + index
		if page, ok := inertiaPageFromTag(raw, start); ok {
			return page, nil
		}
		offset = start + 1
	}
	return nil, fmt.Errorf("%w: div#app data-page not found", ErrHotfixWagoProtocol)
}

// inertiaPageFromTag parses one start tag and returns the unescaped data-page
// payload when the tag is div#app.
func inertiaPageFromTag(raw []byte, start int) ([]byte, bool) {
	i := start + 1
	nameStart := i
	for i < len(raw) && (raw[i] >= 'a' && raw[i] <= 'z' || raw[i] >= 'A' && raw[i] <= 'Z' || raw[i] >= '0' && raw[i] <= '9' || raw[i] == ':' || raw[i] == '-') {
		i++
	}
	if i == nameStart || !strings.EqualFold(string(raw[nameStart:i]), "div") {
		return nil, false
	}
	id, payload := "", ""
	for i < len(raw) {
		for i < len(raw) && (raw[i] == ' ' || raw[i] == '\t' || raw[i] == '\n' || raw[i] == '\r' || raw[i] == '\f') {
			i++
		}
		if i >= len(raw) {
			return nil, false
		}
		if raw[i] == '>' {
			break
		}
		if raw[i] == '/' {
			i++
			continue
		}
		keyStart := i
		for i < len(raw) && raw[i] != '=' && raw[i] != ' ' && raw[i] != '\t' && raw[i] != '\n' && raw[i] != '\r' && raw[i] != '\f' && raw[i] != '>' && raw[i] != '/' {
			i++
		}
		key := strings.ToLower(string(raw[keyStart:i]))
		if key == "" {
			return nil, false
		}
		for i < len(raw) && (raw[i] == ' ' || raw[i] == '\t' || raw[i] == '\n' || raw[i] == '\r' || raw[i] == '\f') {
			i++
		}
		value := ""
		if i < len(raw) && raw[i] == '=' {
			i++
			for i < len(raw) && (raw[i] == ' ' || raw[i] == '\t' || raw[i] == '\n' || raw[i] == '\r' || raw[i] == '\f') {
				i++
			}
			if i >= len(raw) {
				return nil, false
			}
			switch raw[i] {
			case '"', '\'':
				quote := raw[i]
				i++
				valueStart := i
				for i < len(raw) && raw[i] != quote {
					i++
				}
				if i >= len(raw) {
					return nil, false
				}
				value = string(raw[valueStart:i])
				i++
			default:
				valueStart := i
				for i < len(raw) && raw[i] != ' ' && raw[i] != '\t' && raw[i] != '\n' && raw[i] != '\r' && raw[i] != '\f' && raw[i] != '>' {
					i++
				}
				value = string(raw[valueStart:i])
			}
		}
		switch key {
		case "id":
			id = value
		case "data-page":
			payload = value
		}
	}
	if id == "app" && payload != "" {
		return []byte(stdhtml.UnescapeString(payload)), true
	}
	return nil, false
}

// DecodeHotfixPositional names a Wago positional JSON payload using the pinned
// DBD definition. Integer precision and sign come from the exact JSON numbers,
// nested arrays are preserved, and surplus or misaligned columns are refused
// instead of silently shifting fields.
func DecodeHotfixPositional(payload json.RawMessage, definition schema.Definition) (map[string]any, error) {
	if len(payload) == 0 || string(payload) == "null" {
		return nil, nil
	}
	if len(definition.Fields) == 0 {
		return nil, ErrCacheFields
	}
	decoder := json.NewDecoder(bytes.NewReader(payload))
	decoder.UseNumber()
	var values any
	if err := decoder.Decode(&values); err != nil {
		return nil, fmt.Errorf("%w: positional payload is invalid JSON: %v", ErrCacheFields, err)
	}
	if object, ok := values.(map[string]any); ok {
		return object, nil
	}
	columns, ok := values.([]any)
	if !ok {
		return nil, fmt.Errorf("%w: expected positional array or object", ErrCacheFields)
	}
	if len(columns) > len(definition.Fields) {
		return nil, fmt.Errorf("%w: %d values for %d definition fields", ErrCacheFields, len(columns), len(definition.Fields))
	}
	result := make(map[string]any, len(definition.Fields))
	for index, value := range columns {
		field := definition.Fields[index]
		converted, err := positionalValue(value, field)
		if err != nil {
			return nil, fmt.Errorf("%w: %s: %v", ErrCacheFields, field.Name, err)
		}
		result[field.Name] = converted
	}
	return result, nil
}

func positionalValue(value any, field schema.Field) (any, error) {
	if value == nil {
		return nil, nil
	}
	if field.Array {
		items, ok := value.([]any)
		if !ok {
			return nil, fmt.Errorf("array field carries %T", value)
		}
		if uint32(len(items)) != field.Elements {
			return nil, fmt.Errorf("array length %d does not match %d elements", len(items), field.Elements)
		}
		converted := make([]any, 0, len(items))
		for _, item := range items {
			scalar, err := positionalScalar(item, field)
			if err != nil {
				return nil, err
			}
			converted = append(converted, scalar)
		}
		return converted, nil
	}
	return positionalScalar(value, field)
}

func positionalScalar(value any, field schema.Field) (any, error) {
	if value == nil {
		return nil, nil
	}
	switch field.Kind {
	case "int":
		number, ok := positionalNumber(value)
		if !ok {
			return nil, fmt.Errorf("expected integer, got %T", value)
		}
		if field.Signed {
			signed, err := strconv.ParseInt(number, 10, 64)
			if err != nil {
				return nil, fmt.Errorf("expected integer, got %q", number)
			}
			if field.Bits < 64 && (signed < -(int64(1)<<(field.Bits-1)) || signed > (int64(1)<<(field.Bits-1))-1) {
				return nil, fmt.Errorf("value %d does not fit int%d", signed, field.Bits)
			}
			return signed, nil
		}
		unsigned, err := strconv.ParseUint(number, 10, 64)
		if err != nil {
			return nil, fmt.Errorf("expected unsigned integer, got %q", number)
		}
		if field.Bits < 64 && unsigned >= uint64(1)<<field.Bits {
			return nil, fmt.Errorf("value %d does not fit uint%d", unsigned, field.Bits)
		}
		return unsigned, nil
	case "float":
		number, ok := positionalNumber(value)
		if !ok {
			return nil, fmt.Errorf("expected number, got %T", value)
		}
		parsed, err := strconv.ParseFloat(number, 64)
		if err != nil || math.IsInf(parsed, 0) || math.IsNaN(parsed) {
			return nil, fmt.Errorf("expected finite number, got %q", number)
		}
		return parsed, nil
	case "string", "locstring":
		text, ok := value.(string)
		if !ok {
			return nil, fmt.Errorf("expected string, got %T", value)
		}
		return text, nil
	default:
		return nil, fmt.Errorf("unsupported definition kind %q", field.Kind)
	}
}

func positionalNumber(value any) (string, bool) {
	switch typed := value.(type) {
	case json.Number:
		return typed.String(), true
	case string:
		return typed, true
	}
	return "", false
}
