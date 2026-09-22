package command

import (
	"bytes"
	"encoding/json"
	"errors"
	"github.com/follenfang/lycheedev/internal/records"
	"github.com/follenfang/lycheedev/internal/records/relational"
	"github.com/follenfang/lycheedev/internal/records/schema"
	"github.com/follenfang/lycheedev/internal/records/texture"
	"github.com/follenfang/lycheedev/internal/selection"
	"io"
	"os"
	"unicode/utf8"
)

type QueryLocation struct {
	Offset int `json:"offset"`
	Line   int `json:"line"`
	Column int `json:"column"`
}

func queryFault(err error) (int, string, bool) {
	for _, entry := range []struct {
		cause error
		exit  int
		code  string
	}{
		{texture.ErrFormat, 4, "texture.invalid_format"},
		{texture.ErrUnsupported, 3, "texture.unsupported_format"},
		{texture.ErrMipmap, 3, "texture.mipmap_unavailable"},
		{texture.ErrLimit, 3, "texture.pixel_limit"},
		{records.ErrImageOutputLimit, 3, "records.image_output_limit"},
		{selection.ErrProjectMissing, 3, "selection.project_missing"},
		{selection.ErrProjectUnlocked, 3, "selection.project_unlocked"},
		{selection.ErrProjectFormat, 4, "selection.project_format"},
		{selection.ErrProjectConflict, 4, "selection.project_conflict"},
		{records.ErrRemoteRange, 4, "records.remote_range"},
		{records.ErrRemoteObjectMissing, 3, "records.remote_object_missing"},
		{records.ErrRemoteUnavailable, 3, "records.remote_unavailable_offline"},
		{records.ErrRemoteIdentity, 4, "records.remote_identity"},
		{records.ErrRemoteHTTP, 5, "records.remote_http"},
		{records.ErrMetadataFormat, 4, "records.invalid_metadata"},
		{records.ErrMetadataLimit, 3, "records.metadata_limit"},
		{records.ErrConfigurationIntegrity, 4, "records.configuration_integrity"},
		{records.ErrBuildUnavailable, 3, "records.build_unavailable"},
		{records.ErrBuildAmbiguous, 4, "records.build_ambiguous"},
		{records.ErrPinnedBuildChanged, 4, "records.pinned_build_changed"},
		{records.ErrCacheFormat, 4, "records.hotfix_format"},
		{records.ErrCacheFields, 4, "records.hotfix_fields"},
		{records.ErrDefinitionUnavailable, 3, "records.definition_unavailable_offline"},
		{records.ErrDefinitionIdentity, 4, "records.definition_identity"},
		{schema.ErrMissing, 3, "schema.definition_not_found"},
		{schema.ErrAmbiguous, 4, "schema.ambiguous_definition"},
		{schema.ErrFormat, 4, "schema.invalid_definition"},
		{schema.ErrLimit, 3, "schema.limit_exceeded"},
		{records.ErrCacheVersion, 3, "records.hotfix_version"},
		{records.ErrCacheBuild, 4, "records.hotfix_build_mismatch"},
		{records.ErrCacheIdentity, 4, "records.hotfix_identity"},
		{records.ErrCacheLimit, 3, "records.hotfix_limit"},
		{records.ErrLimitRequired, 2, "records.limit_required"},
		{records.ErrQueryLimit, 2, "records.query_limit"},
		{records.ErrFieldUnknown, 2, "records.field_unknown"},
		{records.ErrFieldType, 2, "records.field_type_mismatch"},
		{records.ErrRequestConflict, 2, "records.request_conflict"},
		{records.ErrRecordNotFound, 3, "records.record_not_found"},
		{records.ErrRelationCycle, 4, "records.relation_cycle"},
		{records.ErrHotfixQuery, 2, "records.hotfix_query"},
		{records.ErrHotfixFilter, 2, "records.hotfix_filter_unsupported"},
		{records.ErrHotfixCursor, 2, "records.hotfix_cursor"},
		{records.ErrHotfixOffline, 3, "records.hotfix_offline"},
		{records.ErrHotfixCacheMiss, 3, "records.hotfix_cache_miss"},
		{records.ErrHotfixBudget, 3, "records.hotfix_budget"},
		{records.ErrHotfixSnapshotStale, 3, "records.hotfix_snapshot_stale"},
		{records.ErrHotfixPageDrift, 4, "records.hotfix_page_drift"},
		{records.ErrHotfixCacheCorrupt, 4, "records.hotfix_cache_corrupt"},
		{records.ErrHotfixWagoProtocol, 4, "records.hotfix_wago_protocol"},
		{records.ErrHotfixWagoHTTP, 5, "records.hotfix_wago_http"},
		{records.ErrExportEncoding, 4, "records.export_encoding"},
		{relational.ErrSyntax, 2, "query.invalid_syntax"},
		{relational.ErrBinding, 2, "query.unresolved_binding"},
		{relational.ErrUnsupported, 3, "query.unsupported_expression"},
		{relational.ErrBudget, 3, "query.budget_exceeded"},
		{relational.ErrType, 4, "query.type_mismatch"},
		{relational.ErrNumericRange, 4, "query.numeric_range"},
		{relational.ErrCardinality, 4, "query.invalid_cardinality"},
	} {
		if errors.Is(err, entry.cause) {
			return entry.exit, entry.code, true
		}
	}
	return 0, "", false
}

func readDataQuery(path string) (records.DataQuery, error) {
	file, err := os.Open(path)
	if err != nil {
		return records.DataQuery{}, err
	}
	defer file.Close()
	info, err := file.Stat()
	if err != nil {
		return records.DataQuery{}, err
	}
	if !info.Mode().IsRegular() || info.Size() > 1<<20 {
		return records.DataQuery{}, errors.New("query file must be a regular JSON file of at most 1 MiB")
	}
	raw, err := io.ReadAll(io.LimitReader(file, (1<<20)+1))
	if err != nil {
		return records.DataQuery{}, err
	}
	if len(raw) > 1<<20 || !utf8.Valid(raw) {
		return records.DataQuery{}, errors.New("invalid query file encoding or size")
	}
	return decodeDataQuery(raw)
}

// Tokens permit duplicate-key rejection, including parameter names. Numeric
// parameters stay json.Number, never a float64 JSON intermediary.
func decodeDataQuery(raw []byte) (records.DataQuery, error) {
	bad := errors.New("query JSON requires sql string and optional scalar parameters object; duplicate/unknown fields are invalid")
	d := json.NewDecoder(bytes.NewReader(raw))
	d.UseNumber()
	start, err := d.Token()
	if err != nil || start != json.Delim('{') {
		return records.DataQuery{}, bad
	}
	result := records.DataQuery{}
	seen := map[string]bool{}
	for d.More() {
		token, err := d.Token()
		if err != nil {
			return records.DataQuery{}, bad
		}
		key, ok := token.(string)
		if !ok || seen[key] {
			return records.DataQuery{}, bad
		}
		seen[key] = true
		switch key {
		case "sql":
			if err := d.Decode(&result.SQL); err != nil || result.SQL == "" {
				return records.DataQuery{}, bad
			}
		case "parameters":
			token, err := d.Token()
			if err != nil || token != json.Delim('{') {
				return records.DataQuery{}, bad
			}
			result.Parameters = map[string]any{}
			for d.More() {
				token, err := d.Token()
				if err != nil {
					return records.DataQuery{}, bad
				}
				name, ok := token.(string)
				if !ok {
					return records.DataQuery{}, bad
				}
				if _, found := result.Parameters[name]; found {
					return records.DataQuery{}, bad
				}
				value, err := d.Token()
				if err != nil {
					return records.DataQuery{}, bad
				}
				switch value.(type) {
				case nil, string, bool, json.Number:
				default:
					return records.DataQuery{}, bad
				}
				result.Parameters[name] = value
			}
			if token, err := d.Token(); err != nil || token != json.Delim('}') {
				return records.DataQuery{}, bad
			}
		default:
			return records.DataQuery{}, bad
		}
	}
	if token, err := d.Token(); err != nil || token != json.Delim('}') {
		return records.DataQuery{}, bad
	}
	if _, err := d.Token(); err != io.EOF {
		return records.DataQuery{}, bad
	}
	if !seen["sql"] {
		return records.DataQuery{}, bad
	}
	return result, nil
}
