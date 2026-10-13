package codebase

import (
	"context"
	"slices"
	"sort"
	"strings"
)

// These postings address the authenticated offsets in their original order.
// They are built once per opened snapshot, without another on-disk format.
type recordField uint8

const (
	recordKind recordField = iota
	recordDeclarationPath
	recordName
	recordTarget
	recordID
	recordDocument
	recordLoadTarget
	recordSearchName
	recordSearchPrefix
)

type recordLookupKey struct {
	field recordField
	value string
}

type recordLookups struct {
	postings          map[recordLookupKey][]int
	names             []string
	namesSorted       bool
	declarationsReady bool
}

func (c *snapshotCache) buildLookups(ctx context.Context) error {
	if c.lookups != nil {
		return ctx.Err()
	}
	index := &recordLookups{postings: make(map[recordLookupKey][]int)}
	add := func(field recordField, value string, position int) {
		if value == "" {
			return
		}
		key := recordLookupKey{field, value}
		if (field == recordName || field == recordSearchName) && index.postings[key] == nil {
			index.names = append(index.names, value)
		}
		index.postings[key] = append(index.postings[key], position)
	}
	for i, e := range c.offsets {
		if err := ctx.Err(); err != nil {
			return err
		}
		add(recordKind, e.Kind, i)
		if e.Kind == "document" {
			add(recordDocument, e.Path, i)
		}
		if e.Kind != "symbol" {
			continue
		}
		add(recordName, e.Name, i)
		add(recordTarget, e.Target, i)
		if e.SymbolKind == "load" {
			add(recordLoadTarget, loadedPath(e.Path, e.Target), i)
		}
		if e.SymbolKind == "declaration" {
			// Search accepts a qualified name or any suffix after a dot.
			for j := 0; j < len(e.Name); j++ {
				if e.Name[j] == '.' {
					add(recordSearchName, e.Name[j+1:], i)
				}
			}
		}
	}
	c.lookups = index
	c.work.LookupBuilds++
	c.work.LookupEntries += len(c.offsets)
	return ctx.Err()
}

func (c *snapshotCache) lookupOffsets(ctx context.Context, keys []recordLookupKey) ([]int, error) {
	if err := c.buildLookups(ctx); err != nil {
		return nil, err
	}
	var selected []int
	for _, key := range keys {
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		if key.field == recordSearchPrefix {
			if !c.lookups.namesSorted {
				sort.Strings(c.lookups.names)
				c.lookups.names = slices.Compact(c.lookups.names)
				c.lookups.namesSorted = true
			}
			names := c.lookups.names
			for i := sort.SearchStrings(names, key.value); i < len(names) && strings.HasPrefix(names[i], key.value); i++ {
				if err := ctx.Err(); err != nil {
					return nil, err
				}
				selected = append(selected, c.lookups.postings[recordLookupKey{recordSearchName, names[i]}]...)
				selected = append(selected, c.lookups.postings[recordLookupKey{recordName, names[i]}]...)
			}
		} else {
			if (key.field == recordID || key.field == recordDeclarationPath) && key.value != "" && !c.lookups.declarationsReady {
				// Name queries do not pay to allocate declaration-ID postings.
				for i, e := range c.offsets {
					if err := ctx.Err(); err != nil {
						return nil, err
					}
					if e.Kind == "symbol" && e.SymbolKind == "declaration" {
						for _, entry := range []recordLookupKey{{recordID, e.ID}, {recordDeclarationPath, e.Path}} {
							if entry.value != "" {
								c.lookups.postings[entry] = append(c.lookups.postings[entry], i)
							}
						}
					}
				}
				c.lookups.declarationsReady = true
			}
			selected = append(selected, c.lookups.postings[key]...)
		}
	}
	sort.Ints(selected)
	return slices.Compact(selected), ctx.Err()
}

func nameLookupKeys(names map[string]bool, fields ...recordField) []recordLookupKey {
	keys := make([]recordLookupKey, 0, len(names)*len(fields))
	for name := range names {
		for _, field := range fields {
			keys = append(keys, recordLookupKey{field, name})
		}
	}
	// An empty selection is not a request for a full scan.
	if len(keys) == 0 {
		keys = append(keys, recordLookupKey{recordName, ""})
	}
	return keys
}
