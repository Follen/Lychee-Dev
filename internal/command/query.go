package command

import (
	"bytes"
	"encoding/json"
	"errors"
	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/live"
	"github.com/follenfang/lycheedev/internal/records"
	"github.com/follenfang/lycheedev/internal/records/container"
	"github.com/follenfang/lycheedev/internal/records/relational"
	"github.com/follenfang/lycheedev/internal/records/schema"
	"github.com/follenfang/lycheedev/internal/records/texture"
	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
	"io"
	"os"
	"path/filepath"
	"strings"
	"unicode/utf8"
)

type QueryLocation struct {
	Offset int `json:"offset"`
	Line   int `json:"line"`
	Column int `json:"column"`
}

// errDoctorUnhealthy reports that doctor found at least one check with error
// status. The checks themselves stay in the result; the sentinel only pins the
// documented capability/environment exit code.
var errDoctorUnhealthy = errors.New("doctor.check_failed")

func queryFault(err error) (int, string, bool) {
	for _, entry := range []struct {
		cause error
		exit  int
		code  string
	}{
		{errDoctorUnhealthy, 3, "doctor.check_failed"},
		{live.ErrAckReadinessPending, 6, "live.ack_readiness_pending"},
		{live.ErrReceiptHidePending, 6, "live.receipt_hide_pending"},
		{live.ErrReceiptWindowBusy, 3, "live.receipt_window_busy"},
		{selection.ErrTargetMissing, 3, "selection.target_missing"},
		{selection.ErrTargetExists, 3, "selection.target_exists"},
		{selection.ErrTargetAmbiguous, 2, "selection.target_ambiguous"},
		{selection.ErrTargetInUse, 3, "selection.target_in_use"},
		{selection.ErrTargetFormat, 2, "selection.target_format"},
		{selection.ErrListingOffline, 3, "selection.listing_requires_network"},
		{selection.ErrManifestFormat, 4, "selection.manifest_format"},
		{selection.ErrManifestLimit, 3, "selection.manifest_limit"},
		{vault.ErrCacheIntegrity, 4, "vault.cache_integrity"},
		{vault.ErrCacheLimit, 3, "vault.cache_limit"},
		{vault.ErrCacheBudget, 3, "vault.cache_budget_exceeded"},
		{vault.ErrCacheMissing, 3, "vault.cache_object_missing"},
		{vault.ErrCacheProtection, 4, "vault.cache_protection"},
		{vault.ErrConfigFormat, 4, "vault.config_format"},
		{vault.ErrConfigSchemaNewer, 3, "vault.config_schema_newer"},
		{vault.ErrWorkspaceExists, 3, "vault.workspace_exists"},
		{vault.ErrArchiveState, 3, "vault.archive_state"},
		{texture.ErrFormat, 4, "texture.invalid_format"},
		{texture.ErrUnsupported, 3, "texture.unsupported_format"},
		{texture.ErrMipmap, 3, "texture.mipmap_unavailable"},
		{texture.ErrLimit, 3, "texture.pixel_limit"},
		{records.ErrImageOutputLimit, 3, "records.image_output_limit"},
		{records.ErrQueryCSVLimit, 3, "records.query_csv_limit"},
		{evidence.ErrBundleSelection, 2, "evidence.bundle_selection"},
		{evidence.ErrBundleOutput, 2, "evidence.bundle_output"},
		{evidence.ErrBundleLimit, 3, "evidence.bundle_limit"},
		{selection.ErrProjectMissing, 3, "selection.project_missing"},
		{selection.ErrProjectUnlocked, 3, "selection.project_unlocked"},
		{selection.ErrProjectFormat, 4, "selection.project_format"},
		{selection.ErrProjectConflict, 4, "selection.project_conflict"},
		{records.ErrRemoteRange, 4, "records.remote_range"},
		{records.ErrRemoteObjectMissing, 3, "records.remote_object_missing"},
		{records.ErrRemoteUnavailable, 3, "records.remote_unavailable_offline"},
		{records.ErrRemoteIdentity, 4, "records.remote_identity"},
		{records.ErrRemoteHTTP, 5, "records.remote_http"},
		{container.ErrKeyUnavailable, 3, "container.key_unavailable"},
		{container.ErrUnsupported, 3, "container.unsupported_encoding"},
		{container.ErrLimit, 3, "container.resource_limit"},
		{container.ErrMalformed, 4, "container.invalid_format"},
		{container.ErrIntegrity, 4, "container.integrity_mismatch"},
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

// readSelectedDataQuery preserves the JSON query-document contract while also
// accepting plain SQL from a file, stdin or an inline flag. Source selection is
// checked by dispatch before this helper runs; no fallback to ambient input.
func readSelectedDataQuery(opts Options) (records.DataQuery, error) {
	if opts.file != "" && strings.EqualFold(filepath.Ext(opts.file), ".json") {
		if len(opts.parameters) != 0 {
			return records.DataQuery{}, errors.New("--param cannot be combined with a JSON query document")
		}
		return readDataQuery(opts.file)
	}
	var raw []byte
	var err error
	switch {
	case opts.sql != "":
		raw = []byte(opts.sql)
	case opts.stdin:
		raw, err = io.ReadAll(io.LimitReader(os.Stdin, (1<<20)+1))
	case opts.file != "":
		file, openErr := os.Open(opts.file)
		if openErr != nil {
			return records.DataQuery{}, openErr
		}
		defer file.Close()
		info, statErr := file.Stat()
		if statErr != nil {
			return records.DataQuery{}, statErr
		}
		if !info.Mode().IsRegular() || info.Size() > 1<<20 {
			return records.DataQuery{}, errors.New("SQL file must be regular and at most 1 MiB")
		}
		raw, err = io.ReadAll(io.LimitReader(file, (1<<20)+1))
	}
	if err != nil {
		return records.DataQuery{}, err
	}
	if len(raw) > 1<<20 || !utf8.Valid(raw) || strings.TrimSpace(string(raw)) == "" {
		return records.DataQuery{}, errors.New("SQL input must be UTF-8, nonempty and at most 1 MiB")
	}
	return records.DataQuery{SQL: string(raw), Parameters: opts.parameters}, nil
}

// Scalars use JSON spelling when it is unambiguous; all other text is a
// string, so IDs such as 001 and natural-language values are not mangled.
func parseQueryScalar(raw string) any {
	var value any
	decoder := json.NewDecoder(strings.NewReader(raw))
	decoder.UseNumber()
	if err := decoder.Decode(&value); err != nil {
		return raw
	}
	if _, err := decoder.Token(); err != io.EOF {
		return raw
	}
	switch value.(type) {
	case nil, string, bool, json.Number:
		return value
	default:
		return raw
	}
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
