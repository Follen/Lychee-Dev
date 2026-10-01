package records

import (
	"context"
	"encoding/json"
	"fmt"
	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/records/relational"
	"github.com/follenfang/lycheedev/internal/records/resource"
	"github.com/follenfang/lycheedev/internal/records/schema"
	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
	"math"
	"sort"
	"strconv"
	"strings"
)

type EffectiveTable struct {
	Table           string                `json:"table"`
	Policy          string                `json:"policy"`
	Captures        []evidence.CaptureRef `json:"captures"`
	ReplacedOrAdded int                   `json:"replacedOrAdded"`
	Deleted         int                   `json:"deleted"`
}
type effectiveEntry struct {
	CacheEntry
	capture int
	layout  uint32
	source  string
	raw     []byte
}

func (r *Reader) EffectiveSource(ctx context.Context, store *vault.Store, metadata *vault.Metadata, pin selection.DataPin, base relational.Source, definition schema.Definition, bundle DefinitionBundle, captures []string) (relational.Source, EffectiveTable, error) {
	if r.session != nil {
		if r.session.closed || pin != r.session.pin || store != r.store {
			return relational.Source{}, EffectiveTable{}, ErrCacheIdentity
		}
		if r.session.hotfix == nil {
			r.session.hotfix = newHotfixSnapshots(store, metadata, pin, r.session.query.budget)
		}
		return effectiveSourceWithSnapshots(ctx, r.session.hotfix, base, definition, bundle, captures)
	}
	return effectiveSource(ctx, store, metadata, pin, base, definition, bundle, captures)
}

func effectiveSource(ctx context.Context, store *vault.Store, metadata *vault.Metadata, pin selection.DataPin, base relational.Source, definition schema.Definition, bundle DefinitionBundle, captures []string) (relational.Source, EffectiveTable, error) {
	return effectiveSourceWithSnapshots(ctx, newHotfixSnapshots(store, metadata, pin, nil), base, definition, bundle, captures)
}

func effectiveSourceWithSnapshots(ctx context.Context, snapshots *hotfixSnapshots, base relational.Source, definition schema.Definition, bundle DefinitionBundle, captures []string) (relational.Source, EffectiveTable, error) {
	report := EffectiveTable{Table: bundle.Identity.Name, Policy: "push-ascending/capture-order/physical-index; valid-payload replaces; invalid deletes; cached-key exception"}
	if len(captures) == 0 || len(captures) > 16 {
		return relational.Source{}, report, fmt.Errorf("%w: effective catalog requires hotfix capture IDs in query JSON", ErrCacheIdentity)
	}
	var entries []effectiveEntry
	seenCaptures := map[string]bool{}
	for ordinal, id := range captures {
		if seenCaptures[id] {
			return relational.Source{}, report, ErrCacheIdentity
		}
		seenCaptures[id] = true
		snapshot, err := snapshots.load(ctx, id)
		if err != nil {
			return relational.Source{}, report, err
		}
		selected := snapshot.tables[bundle.Identity.Hash]
		if len(selected) > 100000-len(entries) {
			return relational.Source{}, report, ErrCacheLimit
		}
		if err := snapshots.budget.Charge(resource.Cost{RetainedBytes: int64(len(selected)) * 384, MetadataBytes: int64(len(selected)) * 384, DecodeWork: int64(len(selected))}); err != nil {
			return relational.Source{}, report, err
		}
		for _, entry := range selected {
			if err := ctx.Err(); err != nil {
				return relational.Source{}, report, err
			}
			if snapshot.layout >= 7 && entry.Status > 4 {
				return relational.Source{}, report, fmt.Errorf("%w: unsupported effective status %d", ErrCacheFormat, entry.Status)
			}
			entries = append(entries, effectiveEntry{CacheEntry: entry, capture: ordinal, layout: snapshot.layout, source: id, raw: snapshot.raw})
		}
		report.Captures = append(report.Captures, snapshot.source)
	}
	valid := func(e effectiveEntry) bool {
		return e.PayloadBytes > 0 && (e.layout >= 7 && e.Status == 1 || e.layout < 7 && e.Status != 0)
	}
	shouldDelete := true
	if bundle.Identity.Hash == 3744420815 || bundle.Identity.Hash == 35137211 {
		for _, e := range entries {
			if e.Push == -1 && valid(e) {
				shouldDelete = false
			}
		}
	}
	sort.SliceStable(entries, func(i, j int) bool {
		if entries[i].Push != entries[j].Push {
			return entries[i].Push < entries[j].Push
		}
		if entries[i].capture != entries[j].capture {
			return entries[i].capture < entries[j].capture
		}
		return entries[i].Index < entries[j].Index
	})
	identity := ""
	for _, f := range definition.Fields {
		if f.Identity {
			identity = f.Name
		}
	}
	identityIndex := -1
	for i, c := range base.Columns {
		if c == identity {
			identityIndex = i
		}
		if strings.HasPrefix(c, "__hotfix_") {
			return relational.Source{}, report, ErrDefinitionIdentity
		}
	}
	if identityIndex < 0 || len(base.Columns) > 4093 {
		return relational.Source{}, report, ErrDefinitionIdentity
	}
	type replacement struct {
		values []any
		entry  effectiveEntry
	}
	patches := map[uint32]replacement{}
	var fieldsBytes int
	for _, e := range entries {
		if err := ctx.Err(); err != nil {
			return relational.Source{}, report, err
		}
		if !valid(e) {
			if shouldDelete {
				if err := snapshots.budget.Charge(resource.Cost{RetainedBytes: 384, MetadataBytes: 384}); err != nil {
					return relational.Source{}, report, err
				}
				patches[e.RecordID] = replacement{entry: effectiveEntry{CacheEntry: e.CacheEntry, source: e.source}}
			}
			continue
		}
		if err := snapshots.budget.Charge(resource.Cost{RetainedBytes: int64(e.PayloadBytes)*4 + int64(len(base.Columns))*64 + 384, MetadataBytes: int64(len(base.Columns))*64 + 384, DecodeWork: int64(len(base.Columns)) + 1}); err != nil {
			return relational.Source{}, report, err
		}
		fields, err := DecodeHotfixFields(ctx, e.raw[int(e.PayloadOffset):int(e.PayloadOffset)+e.PayloadBytes], e.RecordID, definition)
		if err != nil {
			return relational.Source{}, report, fmt.Errorf("effective record %d: %w", e.RecordID, err)
		}
		encoded, _ := json.Marshal(fields)
		fieldsBytes += len(encoded)
		if fieldsBytes > 32<<20 {
			return relational.Source{}, report, ErrCacheLimit
		}
		values, err := flattenEffective(base.Columns, fields)
		if err != nil {
			return relational.Source{}, report, err
		}
		patches[e.RecordID] = replacement{values: values, entry: effectiveEntry{CacheEntry: e.CacheEntry, source: e.source}}
	}
	for _, p := range patches {
		if p.values == nil {
			report.Deleted++
		} else {
			report.ReplacedOrAdded++
		}
	}
	columns := append(append([]string{}, base.Columns...), "__hotfix_source", "__hotfix_push", "__hotfix_index")
	augment := func(row []any, p *replacement) []any {
		out := append([]any{}, row...)
		if p == nil {
			return append(out, nil, nil, nil)
		}
		return append(out, p.entry.source, int64(p.entry.Push), uint64(p.entry.Index))
	}
	rowID := func(v any) (uint32, error) {
		switch n := v.(type) {
		case float64:
			if n >= 0 && n <= math.MaxUint32 && n == math.Trunc(n) {
				return uint32(n), nil
			}
		case uint64:
			if n <= 1<<32-1 {
				return uint32(n), nil
			}
		case int64:
			if n >= 0 && n <= 1<<32-1 {
				return uint32(n), nil
			}
		}
		return 0, ErrCacheFields
	}
	output := relational.Source{Columns: columns, Scan: func(ctx context.Context, y func([]any) error) error {
		visited := map[uint32]bool{}
		err := base.Scan(ctx, func(row []any) error {
			id, err := rowID(row[identityIndex])
			if err != nil {
				return err
			}
			if p, ok := patches[id]; ok {
				visited[id] = true
				if p.values == nil {
					return nil
				}
				return y(augment(p.values, &p))
			}
			return y(augment(row, nil))
		})
		if err != nil {
			return err
		}
		ids := []uint32{}
		for id, p := range patches {
			if !visited[id] && p.values != nil {
				ids = append(ids, id)
			}
		}
		sort.Slice(ids, func(i, j int) bool { return ids[i] < ids[j] })
		for _, id := range ids {
			if err := ctx.Err(); err != nil {
				return err
			}
			p := patches[id]
			if err := y(augment(p.values, &p)); err != nil {
				return err
			}
		}
		return nil
	}}
	output.Equal = func(ctx context.Context, column string, value any, y func([]any) error) (bool, error) {
		if !strings.EqualFold(column, identity) || base.Equal == nil {
			return false, nil
		}
		id, err := rowID(value)
		if err == nil {
			if p, ok := patches[id]; ok {
				if p.values == nil {
					return true, nil
				}
				return true, y(augment(p.values, &p))
			}
		}
		return base.Equal(ctx, column, value, func(row []any) error { return y(augment(row, nil)) })
	}
	return output, report, nil
}

func flattenEffective(columns []string, row map[string]any) ([]any, error) {
	values := make([]any, len(columns))
	for i, c := range columns {
		name, index, has := strings.Cut(c, "[")
		v, ok := row[name]
		if !ok {
			return nil, ErrCacheFields
		}
		if has {
			n, err := strconv.Atoi(strings.TrimSuffix(index, "]"))
			if err != nil || n < 0 {
				return nil, ErrCacheFields
			}
			switch a := v.(type) {
			case []any:
				if n >= len(a) {
					return nil, ErrCacheFields
				}
				v = a[n]
			case []string:
				if n >= len(a) {
					return nil, ErrCacheFields
				}
				v = a[n]
			default:
				return nil, ErrCacheFields
			}
		}
		values[i] = v
	}
	return values, nil
}
