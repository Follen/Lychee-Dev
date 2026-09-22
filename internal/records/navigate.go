// Package records: domain relationship navigation over static DB2 tables.
//
// The navigator binds every requested table through the same FileQuery /
// Reader.ReadFile source selection used by raw queries, records the exact
// table/field that produced every relation, and reports truncation and partial
// coverage explicitly. Static DB2 results never absorb Hotfix records.
package records

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"slices"
	"strconv"

	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/records/relational"
	"github.com/follenfang/lycheedev/internal/records/schema"
	"github.com/follenfang/lycheedev/internal/records/table"
	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
)

var (
	// ErrRecordNotFound reports a requested record absent from the pinned data.
	ErrRecordNotFound = errors.New("records.record_not_found")
	// ErrRequestConflict reports ambiguous request keys (zero or several of
	// a set where exactly one is required).
	ErrRequestConflict = errors.New("records.request_conflict")
	// ErrRelationCycle reports a structural relation cycle that cannot be
	// represented as the requested tree (graph relations handle cycles
	// themselves and never raise this).
	ErrRelationCycle = errors.New("records.relation_cycle")
	// ErrLimitRequired reports a missing explicit bound; no query silently
	// returns unbounded sets.
	ErrLimitRequired = errors.New("records.limit_required")
	// ErrQueryLimit reports a caller-supplied limit outside the documented
	// range for the mode.
	ErrQueryLimit = errors.New("records.query_limit")
	// ErrFieldUnknown reports a requested field absent from the bound schema.
	ErrFieldUnknown = errors.New("records.field_unknown")
	// ErrFieldType reports a field that cannot serve the requested role
	// (for example a foreign-key match on a string or array field).
	ErrFieldType = errors.New("records.field_type_mismatch")
)

// Documented query bounds. Every mode states its bound in the result; nothing
// scans or returns beyond these budgets.
const (
	DefaultSpellDepth   = 5
	MaxSpellDepth       = 32
	MaxSearchRows       = 100000
	MaxForeignKeyRows   = 5000
	MaxStreamRows       = 200000
	MaxDecorListRows    = 5000
	MaxNavigateTables   = 32
	MaxRelatedRows      = 65536
	MaxRelationLinks    = 65536
	MaxEncounterDepth   = 32
	MaxEncounterSection = 10000
	rowTextBytes        = 1 << 20
	captureBytes        = 16 << 20
)

// Truncation reasons (snake_case enum). "none" means nothing was cut.
const (
	TruncationNone           = "none"
	TruncationLimitReached   = "limit_reached"
	TruncationMaxDepth       = "max_depth"
	TruncationRowBudget      = "row_budget"
	TruncationRelationBudget = "relation_budget"
)

// Partial reasons (snake_case enum): honest coverage gaps short of truncation.
const (
	PartialTableUnavailable   = "table_unavailable"
	PartialRelationUnresolved = "relation_unresolved"
	PartialRelationAmbiguous  = "relation_ambiguous"
	PartialRowMissing         = "row_missing"
)

// RelationLink is one domain relation with the exact table and field that
// produced it, so a later asset or spell request can trace its origin back to
// the single request that discovered it.
type RelationLink struct {
	FromTable string `json:"fromTable"`
	FromField string `json:"fromField"`
	FromID    uint64 `json:"fromID"`
	ToKind    string `json:"toKind"`
	ToID      uint64 `json:"toID"`
	Depth     int    `json:"depth"`
}

// TableProvenance pins one opened table to its resolved identities.
type TableProvenance struct {
	Name             string               `json:"name"`
	Identity         schema.TableIdentity `json:"identity"`
	LayoutHash       string               `json:"layoutHash"`
	DefinitionSHA256 string               `json:"definitionSHA256"`
	FileDataID       uint32               `json:"fileDataID"`
	ContentKey       string               `json:"contentKey"`
	LogicalRows      int                  `json:"logicalRows"`
}

// DataContext carries the fixed references and honesty flags every result
// must hold: the snapshot and DataPin it resolved, the actual source used,
// every table it read, every relation it derived, and whether the answer is
// complete, truncated or partially covered.
type DataContext struct {
	Snapshot         string            `json:"snapshot"`
	Pin              selection.DataPin `json:"pin"`
	Source           string            `json:"source"`
	Tables           []TableProvenance `json:"tables"`
	Relations        []RelationLink    `json:"relations"`
	Complete         bool              `json:"complete"`
	Truncated        bool              `json:"truncated"`
	TruncationReason string            `json:"truncationReason"`
	Partial          bool              `json:"partial"`
	PartialReasons   []string          `json:"partialReasons"`
}

// Reading pairs one use-case result with its committed evidence captures.
type Reading[T any] struct {
	Result   T                     `json:"result"`
	Captures []evidence.CaptureRef `json:"captures"`
}

// validateLimit enforces an explicit bound within the documented range.
func validateLimit(limit, maximum int) error {
	if limit <= 0 {
		return ErrLimitRequired
	}
	if limit > maximum {
		return fmt.Errorf("%w: %d > %d", ErrQueryLimit, limit, maximum)
	}
	return nil
}

// navigator is the deep module behind the domain entry points: it owns source
// selection, definition binding, lazy table opening, relation provenance and
// honesty flags. Callers only state their question.
type navigator struct {
	store       *vault.Store
	metadata    *vault.Metadata
	definitions *Definitions
	reader      *Reader
	pin         selection.DataPin
	snapshot    string
	query       FileQuery
	source      string
	opened      map[string]*preparedTable
	tables      []TableProvenance
	relations   []RelationLink
	partial     []string
	truncated   bool
	truncation  string
}

type preparedTable struct {
	name       string
	view       *table.View
	provenance TableProvenance
}

// openNavigator validates the request identity and prepares lazy table access.
// No table is read until a use case asks for one.
func openNavigator(ctx context.Context, store *vault.Store, metadata *vault.Metadata, snapshot string, query FileQuery) (*navigator, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if store == nil || metadata == nil {
		return nil, ErrMetadataLimit
	}
	pin, err := selection.OpenPinner(metadata).ReadPinnedSet(ctx, snapshot)
	if err != nil {
		return nil, err
	}
	if pin.Data == nil {
		return nil, errors.New("records.data_pin_required")
	}
	if _, _, err := selection.DataIdentity(*pin.Data); err != nil {
		return nil, err
	}
	return &navigator{
		store: store, metadata: metadata,
		definitions: OpenDefinitions(store, metadata),
		reader:      OpenReader(store),
		pin:         *pin.Data, snapshot: snapshot, query: query,
		opened: map[string]*preparedTable{},
	}, nil
}

// open binds one table through the shared ReadFile chain and records its
// provenance once per query.
func (n *navigator) open(ctx context.Context, name string) (*preparedTable, error) {
	if prepared, ok := n.opened[name]; ok {
		return prepared, nil
	}
	if len(n.opened) >= MaxNavigateTables {
		n.setTruncation(TruncationRelationBudget)
		return nil, relational.ErrBudget
	}
	bundle, err := n.definitions.Prepare(ctx, n.pin.DefinitionCommit, name, n.query.Offline)
	if err != nil {
		return nil, err
	}
	query := n.query
	query.FileDataID = bundle.Identity.DB2FileDataID
	view, reading, err := n.reader.PrepareTable(ctx, n.pin, query, bundle)
	if err != nil {
		return nil, err
	}
	provenance := TableProvenance{
		Name: name, Identity: bundle.Identity, LayoutHash: reading.LayoutHash,
		DefinitionSHA256: reading.Schema.SHA256, FileDataID: bundle.Identity.DB2FileDataID,
		ContentKey: reading.File.Entry.ContentKey, LogicalRows: view.LogicalCount(),
	}
	if reading.File.Source != "" {
		n.source = reading.File.Source
	}
	prepared := &preparedTable{name: name, view: view, provenance: provenance}
	n.opened[name] = prepared
	n.tables = append(n.tables, provenance)
	return prepared, nil
}

// openOptional opens a table and records an honest partial reason instead of
// failing when it is absent; binding or decoding failures stay fatal.
func (n *navigator) openOptional(ctx context.Context, name string) (*preparedTable, bool, error) {
	prepared, err := n.open(ctx, name)
	if err == nil {
		return prepared, true, nil
	}
	if errors.Is(err, ErrContentMissing) || errors.Is(err, schema.ErrMissing) || errors.Is(err, ErrDefinitionUnavailable) {
		n.addPartial(PartialTableUnavailable)
		return nil, false, nil
	}
	return nil, false, err
}

func (n *navigator) link(fromTable, fromField string, fromID uint64, toKind string, toID uint64, depth int) {
	if len(n.relations) >= MaxRelationLinks {
		n.setTruncation(TruncationRelationBudget)
		return
	}
	n.relations = append(n.relations, RelationLink{
		FromTable: fromTable, FromField: fromField, FromID: fromID,
		ToKind: toKind, ToID: toID, Depth: depth,
	})
}

func (n *navigator) addPartial(reason string) {
	for _, existing := range n.partial {
		if existing == reason {
			return
		}
	}
	n.partial = append(n.partial, reason)
}

func (n *navigator) setTruncation(reason string) {
	n.truncated = true
	if n.truncation == TruncationNone || n.truncation == "" {
		n.truncation = reason
	}
}

// context assembles the fixed references and honesty flags of one result.
func (n *navigator) resultContext() DataContext {
	reason := n.truncation
	if reason == "" {
		reason = TruncationNone
	}
	relations := n.relations
	if relations == nil {
		relations = []RelationLink{}
	}
	tables := n.tables
	if tables == nil {
		tables = []TableProvenance{}
	}
	partial := append([]string(nil), n.partial...)
	return DataContext{
		Snapshot: n.snapshot, Pin: n.pin, Source: n.source,
		Tables: tables, Relations: relations,
		Complete:  !n.truncated && len(partial) == 0,
		Truncated: n.truncated, TruncationReason: reason,
		Partial: len(partial) > 0, PartialReasons: partial,
	}
}

// walkRows visits matching rows in ascending ID order with an explicit bound
// and exact truncation probing: after `limit` matches it only looks for one
// further match to decide complete versus truncated.
func (n *navigator) walkRows(ctx context.Context, t *preparedTable, limit int, match func(map[string]any) (bool, error), keep func(uint32, map[string]any) error) (int, bool, error) {
	if limit <= 0 {
		return 0, false, ErrLimitRequired
	}
	collected := 0
	extra := false
	err := t.view.ScanWithIDs(ctx, int64(t.provenance.LogicalRows)*4, rowTextBytes, func(id uint32, row map[string]any) error {
		matched, err := match(row)
		if err != nil {
			return err
		}
		if !matched {
			return nil
		}
		if collected == limit {
			extra = true
			return errWalkStop
		}
		collected++
		return keep(id, row)
	})
	if err != nil && !errors.Is(err, errWalkStop) {
		return 0, false, err
	}
	return collected, extra, nil
}

var errWalkStop = errors.New("records.walk_complete")

// rowWithID pairs a decoded row with its logical record ID.
type rowWithID struct {
	id  uint32
	row map[string]any
}

// rowByID decodes one row; a missing record is (nil, false), not an error, so
// domain queries can distinguish absent rows from decode failures.
func (n *navigator) rowByID(ctx context.Context, t *preparedTable, id uint32) (map[string]any, bool, error) {
	row, err := t.view.Row(ctx, id, rowTextBytes)
	if errors.Is(err, table.ErrRecordMissing) {
		return nil, false, nil
	}
	if err != nil {
		return nil, false, err
	}
	return row, true, nil
}

// rowsByField collects rows whose scalar field equals value, in ascending
// record order, bounded and truncation-probed like walkRows.
func (n *navigator) rowsByField(ctx context.Context, t *preparedTable, field string, value uint32, limit int) ([]rowWithID, bool, error) {
	if err := requireField(t, field, true); err != nil {
		return nil, false, err
	}
	collected := []rowWithID{}
	_, extra, err := n.walkRows(ctx, t, limit,
		func(row map[string]any) (bool, error) { return scalarMatches(row[field], value), nil },
		func(id uint32, row map[string]any) error {
			collected = append(collected, rowWithID{id: id, row: row})
			return nil
		})
	if err != nil {
		return nil, false, err
	}
	return collected, extra, nil
}

// commitCapture archives one result as evidence with its fixed locator.
func (n *navigator) commitCapture(ctx context.Context, kind, locator string, result any, complete, truncated bool) (evidence.CaptureRef, error) {
	raw, err := marshalBounded(result, captureBytes)
	if err != nil {
		return evidence.CaptureRef{}, err
	}
	return evidence.OpenArchive(n.store, n.metadata).CommitCapture(ctx, evidence.CaptureDraft{
		Reader: bytes.NewReader(raw), MaxBytes: captureBytes, MediaType: "application/json",
		Complete: complete, Truncated: truncated,
		Provenance: evidence.Provenance{Kind: kind, Locator: locator, Snapshot: n.snapshot, DataBuild: n.pin.FullBuild},
	})
}

func marshalBounded(value any, maximum int) ([]byte, error) {
	raw, err := json.Marshal(value)
	if err != nil {
		return nil, err
	}
	if len(raw) > maximum {
		return nil, relational.ErrBudget
	}
	return raw, nil
}

// Field access coercions over decoded rows. Decoded rows carry Go 64-bit
// integer types; adapters must never round them through floating point.
func rowUint32(row map[string]any, name string) uint32 {
	switch value := row[name].(type) {
	case uint64:
		if value <= 0xffffffff {
			return uint32(value)
		}
	case int64:
		if value > 0 && value <= 0xffffffff {
			return uint32(value)
		}
	case uint32:
		return value
	case int32:
		if value > 0 {
			return uint32(value)
		}
	case float32:
		if value > 0 && value <= 0xffffffff {
			return uint32(value)
		}
	}
	return 0
}

// rowInt preserves legacy truncating semantics for numeric presentation
// fields (an integral float is presented as its truncated integer).
func rowInt(row map[string]any, name string) int {
	switch value := row[name].(type) {
	case uint64:
		if value <= uint64(^uint(0)>>1) {
			return int(value)
		}
	case int64:
		return int(value)
	case float32:
		return int(value)
	case uint32:
		return int(value)
	case int32:
		return int(value)
	}
	return 0
}

func rowString(row map[string]any, name string) string {
	if value, ok := row[name].(string); ok {
		return value
	}
	return ""
}

// rowFirstUint32 reads the first array element like the legacy coercion:
// non-positive signed entries present as zero, unsigned entries pass through.
func rowFirstUint32(row map[string]any, name string) uint32 {
	switch value := row[name].(type) {
	case []any:
		if len(value) > 0 {
			return scalarUint32(value[0])
		}
	case []uint32:
		if len(value) > 0 {
			return value[0]
		}
	case []int64:
		if len(value) > 0 {
			return scalarUint32(value[0])
		}
	default:
		return rowUint32(row, name)
	}
	return 0
}

func scalarUint32(value any) uint32 {
	switch typed := value.(type) {
	case uint64:
		if typed <= 0xffffffff {
			return uint32(typed)
		}
	case int64:
		if typed > 0 && typed <= 0xffffffff {
			return uint32(typed)
		}
	case uint32:
		return typed
	case int32:
		if typed > 0 {
			return uint32(typed)
		}
	}
	return 0
}

// rowUint32List keeps positive entries, matching the legacy projection of
// resource arrays where zero marks an unused slot.
func rowUint32List(row map[string]any, name string) []uint32 {
	out := []uint32{}
	appendValue := func(value any) {
		if id := scalarUint32(value); id != 0 {
			out = append(out, id)
		}
	}
	switch values := row[name].(type) {
	case []any:
		for _, value := range values {
			appendValue(value)
		}
	case []uint32:
		for _, value := range values {
			if value != 0 {
				out = append(out, value)
			}
		}
	case []int64:
		for _, value := range values {
			appendValue(value)
		}
	}
	return out
}

// rowIntList keeps every element including zeros (array-valued fields such as
// EffectMiscValue keep their exact shape).
func rowIntList(row map[string]any, name string) []int {
	out := []int{}
	switch values := row[name].(type) {
	case []any:
		for _, value := range values {
			out = append(out, scalarInt(value))
		}
	case []int64:
		for _, value := range values {
			out = append(out, int(value))
		}
	case []uint64:
		for _, value := range values {
			out = append(out, int(value))
		}
	}
	return out
}

func scalarInt(value any) int {
	switch typed := value.(type) {
	case int64:
		return int(typed)
	case uint64:
		return int(typed)
	case float32:
		return int(typed)
	case int32:
		return int(typed)
	case uint32:
		return int(typed)
	}
	return 0
}

func nullableString(value string) *string {
	if value == "" {
		return nil
	}
	return &value
}

func nullableUint32(value uint32) *uint32 {
	if value == 0 {
		return nil
	}
	return &value
}

func sortedIDs(set map[uint32]bool) []uint32 {
	out := make([]uint32, 0, len(set))
	for id := range set {
		out = append(out, id)
	}
	slices.Sort(out)
	return out
}

func uint32Text(value uint32) string { return strconv.FormatUint(uint64(value), 10) }
