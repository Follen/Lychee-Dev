package records

import (
	"context"
	"errors"
	"fmt"
	"strconv"
	"strings"

	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/records/schema"
	"github.com/follenfang/lycheedev/internal/records/table"
	"github.com/follenfang/lycheedev/internal/vault"
)

// DB2 query modes. All four run through the same FileQuery / Reader.ReadFile
// source selection as data db2 rows, honor --offline, and return the fixed
// snapshot identity with every result. Rows keep their schema field names;
// only the mode metadata around them is new.

// DB2Field is one parsed schema field with its storage shape and keys.
type DB2Field struct {
	Name          string `json:"name"`
	Type          string `json:"type"`
	Kind          string `json:"kind"`
	Bits          uint16 `json:"bits"`
	Signed        bool   `json:"signed"`
	Elements      uint32 `json:"elements"`
	Array         bool   `json:"array"`
	Identity      bool   `json:"identity"`
	Inline        bool   `json:"inline"`
	Relation      bool   `json:"relation"`
	ForeignTable  string `json:"foreignTable,omitempty"`
	ForeignColumn string `json:"foreignColumn,omitempty"`
	Verified      bool   `json:"verified"`
}

// DB2SchemaResult is parsed table schema metadata: fields, types, arrays, keys
// and relationship fields, with table identity and DataPin preserved.
type DB2SchemaResult struct {
	DataContext
	Table              string     `json:"table"`
	RowCount           int        `json:"rowCount"`
	Fields             []DB2Field `json:"fields"`
	Keys               []string   `json:"keys"`
	RelationshipFields []string   `json:"relationshipFields"`
	ArrayFields        []string   `json:"arrayFields"`
}

// DB2SearchRequest is a case-insensitive substring search over one field.
type DB2SearchRequest struct {
	Field string `json:"field"`
	Query string `json:"query"`
	Limit int    `json:"limit"`
}

// DB2SearchResult holds up to limit matching rows in ascending record order.
type DB2SearchResult struct {
	DataContext
	Table string           `json:"table"`
	Mode  string           `json:"mode"`
	Field string           `json:"field"`
	Query string           `json:"query"`
	Limit int              `json:"limit"`
	Count int              `json:"count"`
	Rows  []map[string]any `json:"rows"`
}

// DB2ForeignKeyRequest selects rows by one foreign-key field and value.
// Limit is mandatory: legacy limit 0 returned unbounded sets.
type DB2ForeignKeyRequest struct {
	Field string `json:"field"`
	Value uint32 `json:"value"`
	Limit int    `json:"limit"`
}

// DB2ForeignKeyResult holds up to limit matching rows in ascending order.
type DB2ForeignKeyResult struct {
	DataContext
	Table string           `json:"table"`
	Mode  string           `json:"mode"`
	Field string           `json:"field"`
	Value uint32           `json:"value"`
	Limit int              `json:"limit"`
	Count int              `json:"count"`
	Rows  []map[string]any `json:"rows"`
}

// DB2StreamRequest projects fields, optionally filters with "field=value",
// and bounds the emitted records with an explicit limit.
type DB2StreamRequest struct {
	Fields []string `json:"fields,omitempty"`
	Filter string   `json:"filter,omitempty"`
	Limit  int      `json:"limit"`
}

// Stream frame contract (design.md section 7): typed begin/record/end/error
// frames; only a legal end frame followed by exit 0 is a complete success.
const (
	StreamFrameBegin  = "begin"
	StreamFrameRecord = "record"
	StreamFrameEnd    = "end"
	StreamFrameError  = "error"
)

type StreamBegin struct {
	Snapshot   string               `json:"snapshot"`
	Source     string               `json:"source"`
	Table      string               `json:"table"`
	Identity   schema.TableIdentity `json:"identity"`
	LayoutHash string               `json:"layoutHash"`
	Fields     []string             `json:"fields"`
	Filter     string               `json:"filter"`
	Limit      int                  `json:"limit"`
}

type StreamRecord struct {
	Index    int            `json:"index"`
	RecordID uint32         `json:"recordID"`
	Row      map[string]any `json:"row"`
}

type StreamEnd struct {
	Count            int    `json:"count"`
	Complete         bool   `json:"complete"`
	Truncated        bool   `json:"truncated"`
	TruncationReason string `json:"truncationReason"`
}

type StreamError struct {
	Code     string  `json:"code"`
	Message  string  `json:"message"`
	RecordID *uint32 `json:"recordID,omitempty"`
}

// StreamFrame is one JSONL frame of a bounded DB2 stream.
type StreamFrame struct {
	Frame  string        `json:"frame"`
	Begin  *StreamBegin  `json:"begin,omitempty"`
	Record *StreamRecord `json:"record,omitempty"`
	End    *StreamEnd    `json:"end,omitempty"`
	Error  *StreamError  `json:"error,omitempty"`
}

// DB2StreamSummary is the archived manifest of one stream: the begin and end
// frames carry the fixed references; the records themselves are the caller's
// streamed artifact.
type DB2StreamSummary struct {
	DataContext
	Table  string   `json:"table"`
	Mode   string   `json:"mode"`
	Limit  int      `json:"limit"`
	Fields []string `json:"fields"`
	Filter string   `json:"filter"`
	Count  int      `json:"count"`
}

// legacyFieldType keeps the legacy schema type vocabulary for compatibility
// checks against old schema oracles.
func legacyFieldType(field schema.Field) string {
	if !field.Inline && field.Relation {
		return "dbFieldRelation"
	}
	if !field.Inline && field.Identity {
		return "dbFieldNonInlineID"
	}
	switch field.Kind {
	case "string", "locstring":
		return "dbFieldString"
	case "float":
		return "dbFieldFloat"
	case "int":
		prefix := "dbFieldInt"
		if !field.Signed {
			prefix = "dbFieldUInt"
		}
		return prefix + strconv.Itoa(int(field.Bits))
	}
	return "dbField" + field.Kind
}

func schemaFields(definition schema.Definition) (fields []DB2Field, keys, relations, arrays []string) {
	fields = []DB2Field{}
	keys, relations, arrays = []string{}, []string{}, []string{}
	for _, field := range definition.Fields {
		fields = append(fields, DB2Field{
			Name: field.Name, Type: legacyFieldType(field), Kind: field.Kind,
			Bits: field.Bits, Signed: field.Signed, Elements: field.Elements,
			Array: field.Array, Identity: field.Identity, Inline: field.Inline,
			Relation: field.Relation, ForeignTable: field.ForeignTable,
			ForeignColumn: field.ForeignColumn, Verified: field.Verified,
		})
		if field.Identity {
			keys = append(keys, field.Name)
		}
		if field.Relation || field.ForeignTable != "" {
			relations = append(relations, field.Name)
		}
		if field.Array {
			arrays = append(arrays, field.Name)
		}
	}
	return fields, keys, relations, arrays
}

// InspectDataSchema returns parsed schema metadata for one bound table.
func InspectDataSchema(ctx context.Context, root, snapshot, name string, query FileQuery) (Reading[DB2SchemaResult], error) {
	return withNavigator(ctx, root, snapshot, query, func(nav *navigator) (Reading[DB2SchemaResult], error) {
		t, err := nav.open(ctx, name)
		if err != nil {
			return Reading[DB2SchemaResult]{}, err
		}
		definition := t.view.Definition()
		fields, keys, relations, arrays := schemaFields(definition)
		result := DB2SchemaResult{
			Table: t.provenance.Identity.Name, RowCount: t.provenance.LogicalRows,
			Fields: fields, Keys: keys, RelationshipFields: relations, ArrayFields: arrays,
		}
		result.DataContext = nav.resultContext()
		capture, err := nav.commitCapture(ctx, "data-schema", "db2:"+name+":schema", result, result.Complete, result.Truncated)
		if err != nil {
			return Reading[DB2SchemaResult]{}, err
		}
		return Reading[DB2SchemaResult]{Result: result, Captures: []evidence.CaptureRef{capture}}, nil
	})
}

// InspectDataSearch searches one field case-insensitively for a substring,
// like the legacy db2 search mode, with an explicit bound.
func InspectDataSearch(ctx context.Context, root, snapshot, name string, query FileQuery, request DB2SearchRequest) (Reading[DB2SearchResult], error) {
	if err := validateLimit(request.Limit, MaxSearchRows); err != nil {
		return Reading[DB2SearchResult]{}, err
	}
	needle := strings.ToLower(request.Query)
	return withNavigator(ctx, root, snapshot, query, func(nav *navigator) (Reading[DB2SearchResult], error) {
		t, err := nav.open(ctx, name)
		if err != nil {
			return Reading[DB2SearchResult]{}, err
		}
		if err := requireField(t, request.Field, false); err != nil {
			return Reading[DB2SearchResult]{}, err
		}
		rows := []map[string]any{}
		count, extra, err := nav.walkRows(ctx, t, request.Limit,
			func(row map[string]any) (bool, error) {
				return strings.Contains(strings.ToLower(searchText(row[request.Field])), needle), nil
			},
			func(_ uint32, row map[string]any) error {
				rows = append(rows, row)
				return nil
			})
		if err != nil {
			return Reading[DB2SearchResult]{}, err
		}
		if extra {
			nav.setTruncation(TruncationLimitReached)
		}
		result := DB2SearchResult{
			Table: t.provenance.Identity.Name, Mode: "search",
			Field: request.Field, Query: request.Query, Limit: request.Limit,
			Count: count, Rows: rows,
		}
		result.DataContext = nav.resultContext()
		capture, err := nav.commitCapture(ctx, "data-search", fmt.Sprintf("db2:%s:search:%s", name, request.Field), result, result.Complete, result.Truncated)
		if err != nil {
			return Reading[DB2SearchResult]{}, err
		}
		return Reading[DB2SearchResult]{Result: result, Captures: []evidence.CaptureRef{capture}}, nil
	})
}

// InspectDataForeignKey selects rows by one foreign-key field and value with a
// mandatory explicit limit and exact complete/truncated reporting.
func InspectDataForeignKey(ctx context.Context, root, snapshot, name string, query FileQuery, request DB2ForeignKeyRequest) (Reading[DB2ForeignKeyResult], error) {
	if err := validateLimit(request.Limit, MaxForeignKeyRows); err != nil {
		return Reading[DB2ForeignKeyResult]{}, err
	}
	return withNavigator(ctx, root, snapshot, query, func(nav *navigator) (Reading[DB2ForeignKeyResult], error) {
		t, err := nav.open(ctx, name)
		if err != nil {
			return Reading[DB2ForeignKeyResult]{}, err
		}
		if err := requireField(t, request.Field, true); err != nil {
			return Reading[DB2ForeignKeyResult]{}, err
		}
		rows := []map[string]any{}
		count, extra, err := nav.walkRows(ctx, t, request.Limit,
			func(row map[string]any) (bool, error) {
				return scalarMatches(row[request.Field], request.Value), nil
			},
			func(_ uint32, row map[string]any) error {
				rows = append(rows, row)
				return nil
			})
		if err != nil {
			return Reading[DB2ForeignKeyResult]{}, err
		}
		if extra {
			nav.setTruncation(TruncationLimitReached)
		}
		result := DB2ForeignKeyResult{
			Table: t.provenance.Identity.Name, Mode: "foreign-key",
			Field: request.Field, Value: request.Value, Limit: request.Limit,
			Count: count, Rows: rows,
		}
		result.DataContext = nav.resultContext()
		capture, err := nav.commitCapture(ctx, "data-foreign-key", fmt.Sprintf("db2:%s:foreign-key:%s=%d", name, request.Field, request.Value), result, result.Complete, result.Truncated)
		if err != nil {
			return Reading[DB2ForeignKeyResult]{}, err
		}
		return Reading[DB2ForeignKeyResult]{Result: result, Captures: []evidence.CaptureRef{capture}}, nil
	})
}

// StreamDataRows emits a bounded JSONL frame stream through yield and archives
// only its begin/end manifest. A failure emits an error frame and no end frame.
func StreamDataRows(ctx context.Context, root, snapshot, name string, query FileQuery, request DB2StreamRequest, yield func(StreamFrame) error) (Reading[DB2StreamSummary], error) {
	if yield == nil {
		return Reading[DB2StreamSummary]{}, ErrRequestConflict
	}
	if err := validateLimit(request.Limit, MaxStreamRows); err != nil {
		return Reading[DB2StreamSummary]{}, err
	}
	filterField, filterValue, err := parseStreamFilter(request.Filter)
	if err != nil {
		return Reading[DB2StreamSummary]{}, err
	}
	return withNavigator(ctx, root, snapshot, query, func(nav *navigator) (Reading[DB2StreamSummary], error) {
		t, err := nav.open(ctx, name)
		if err != nil {
			return Reading[DB2StreamSummary]{}, err
		}
		if filterField != "" {
			if err := requireField(t, filterField, false); err != nil {
				return Reading[DB2StreamSummary]{}, err
			}
		}
		fields := append([]string(nil), request.Fields...)
		if len(fields) > 0 {
			for _, field := range fields {
				if err := requireField(t, field, false); err != nil {
					return Reading[DB2StreamSummary]{}, err
				}
			}
		} else {
			for _, field := range t.view.Definition().Fields {
				fields = append(fields, field.Name)
			}
		}
		begin := StreamBegin{
			Snapshot: snapshot, Source: nav.source, Table: t.provenance.Identity.Name,
			Identity: t.provenance.Identity, LayoutHash: t.provenance.LayoutHash,
			Fields: fields, Filter: request.Filter, Limit: request.Limit,
		}
		if err := yield(StreamFrame{Frame: StreamFrameBegin, Begin: &begin}); err != nil {
			return Reading[DB2StreamSummary]{}, err
		}
		count := 0
		var failure *StreamError
		extra := false
		walkErr := t.view.ScanWithIDs(ctx, int64(t.provenance.LogicalRows)*4, rowTextBytes, func(id uint32, row map[string]any) error {
			if filterField != "" && !filterMatches(row[filterField], filterValue) {
				return nil
			}
			if count == request.Limit {
				extra = true
				return errWalkStop
			}
			projected := make(map[string]any, len(fields))
			for _, field := range fields {
				projected[field] = row[field]
			}
			record := StreamRecord{Index: count, RecordID: id, Row: projected}
			count++
			return yield(StreamFrame{Frame: StreamFrameRecord, Record: &record})
		})
		if walkErr != nil && !errors.Is(walkErr, errWalkStop) {
			code := stableErrorCode(walkErr)
			failure = &StreamError{Code: code, Message: walkErr.Error()}
			if err := yield(StreamFrame{Frame: StreamFrameError, Error: failure}); err != nil {
				return Reading[DB2StreamSummary]{}, err
			}
			return Reading[DB2StreamSummary]{}, walkErr
		}
		if extra {
			nav.setTruncation(TruncationLimitReached)
		}
		contextFrame := nav.resultContext()
		end := StreamEnd{
			Count: count, Complete: contextFrame.Complete, Truncated: contextFrame.Truncated,
			TruncationReason: contextFrame.TruncationReason,
		}
		if err := yield(StreamFrame{Frame: StreamFrameEnd, End: &end}); err != nil {
			return Reading[DB2StreamSummary]{}, err
		}
		result := DB2StreamSummary{
			Table: t.provenance.Identity.Name, Mode: "stream", Limit: request.Limit,
			Fields: fields, Filter: request.Filter, Count: count,
		}
		result.DataContext = contextFrame
		manifest := struct {
			Begin StreamBegin `json:"begin"`
			End   StreamEnd   `json:"end"`
		}{Begin: begin, End: end}
		capture, err := nav.commitCapture(ctx, "data-stream-manifest", fmt.Sprintf("db2:%s:stream", name), manifest, end.Complete, end.Truncated)
		if err != nil {
			return Reading[DB2StreamSummary]{}, err
		}
		return Reading[DB2StreamSummary]{Result: result, Captures: []evidence.CaptureRef{capture}}, nil
	})
}

// requireField validates a caller-named field against the bound schema.
// Numeric-only roles additionally reject string and float fields.
func requireField(t *preparedTable, name string, numericOnly bool) error {
	if name == "" {
		return ErrFieldUnknown
	}
	for _, field := range t.view.Definition().Fields {
		if field.Name != name {
			continue
		}
		if numericOnly && field.Kind != "int" {
			return fmt.Errorf("%w: %s is %s", ErrFieldType, name, field.Kind)
		}
		return nil
	}
	return fmt.Errorf("%w: %s", ErrFieldUnknown, name)
}

// searchText renders a decoded value the way legacy search compares it.
func searchText(value any) string {
	if text, ok := value.(string); ok {
		return text
	}
	if value == nil {
		return ""
	}
	return fmt.Sprint(value)
}

// scalarMatches is exact numeric equality for a uint32 foreign-key value.
// Negative or over-wide decoded values never match a uint32 request.
func scalarMatches(value any, want uint32) bool {
	switch typed := value.(type) {
	case uint64:
		return typed == uint64(want)
	case int64:
		return typed >= 0 && typed == int64(want)
	case uint32:
		return typed == want
	case int32:
		return typed >= 0 && uint32(typed) == want
	}
	return false
}

// parseStreamFilter accepts the legacy "field=value" filter form.
func parseStreamFilter(filter string) (field, value string, err error) {
	filter = strings.TrimSpace(filter)
	if filter == "" {
		return "", "", nil
	}
	parts := strings.SplitN(filter, "=", 2)
	if len(parts) != 2 || strings.TrimSpace(parts[0]) == "" {
		return "", "", fmt.Errorf("%w: invalid filter %q, expected field=value", ErrFieldType, filter)
	}
	return strings.TrimSpace(parts[0]), strings.TrimSpace(parts[1]), nil
}

// filterMatches preserves legacy filter semantics: exact rendered equality,
// then typed integer/bool equality.
func filterMatches(value any, want string) bool {
	if value == nil {
		return false
	}
	if fmt.Sprint(value) == want {
		return true
	}
	switch typed := value.(type) {
	case int64:
		if parsed, err := strconv.ParseInt(want, 10, 64); err == nil {
			return typed == parsed
		}
	case uint64:
		if parsed, err := strconv.ParseUint(want, 10, 64); err == nil {
			return typed == parsed
		}
	case int32:
		if parsed, err := strconv.ParseInt(want, 10, 64); err == nil {
			return int64(typed) == parsed
		}
	case uint32:
		if parsed, err := strconv.ParseUint(want, 10, 64); err == nil {
			return uint64(typed) == parsed
		}
	}
	return false
}

// stableErrorCode presents a bounded failure with a module-qualified code.
func stableErrorCode(err error) string {
	if errors.Is(err, context.Canceled) || errors.Is(err, context.DeadlineExceeded) {
		return "records.cancelled"
	}
	for _, known := range []error{
		ErrMetadataFormat, ErrMetadataLimit, ErrRecordNotFound, ErrRequestConflict,
		ErrRelationCycle, ErrLimitRequired, ErrQueryLimit, ErrFieldUnknown, ErrFieldType,
		schema.ErrFormat, schema.ErrLimit, schema.ErrMissing, schema.ErrAmbiguous,
		table.ErrFormat, table.ErrLimit, table.ErrUnsupported, table.ErrRecordMissing,
	} {
		if errors.Is(err, known) {
			return known.Error()
		}
	}
	return "records.row_decode"
}

// withNavigator runs one use case inside a workspace metadata session with a
// freshly resolved snapshot identity.
func withNavigator[T any](ctx context.Context, root, snapshot string, query FileQuery, run func(*navigator) (Reading[T], error)) (Reading[T], error) {
	return vault.WriteMetadata(ctx, root, func(store *vault.Store, metadata *vault.Metadata) (Reading[T], error) {
		nav, err := openNavigator(ctx, store, metadata, snapshot, query)
		if err != nil {
			return Reading[T]{}, err
		}
		return run(nav)
	})
}
