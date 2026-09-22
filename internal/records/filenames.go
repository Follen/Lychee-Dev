// SPDX-License-Identifier: AGPL-3.0-or-later
// File lookup/search/extension semantics adapted from wowdata; see THIRD_PARTY_NOTICES.md.
package records

import (
	"context"
	"errors"

	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
)

var (
	ErrSnapshotRequired = errors.New("records.snapshot_required")
	ErrDataPinRequired  = errors.New("records.data_pin_required")
)

// FileContext is the fixed identity every filename/file-metadata result
// carries: the resolved pinned snapshot and the exact listfile source that
// answered, so no result is detached from the data it was computed against.
type FileContext struct {
	Snapshot string              `json:"snapshot"`
	Pin      selection.DataPin   `json:"pin"`
	Source   string              `json:"source"`
	Listfile *ListfileProvenance `json:"listfile,omitempty"`
}

func pinnedDataContext(ctx context.Context, m *vault.Metadata, snapshot string) (selection.DataPin, error) {
	if snapshot == "" {
		return selection.DataPin{}, ErrSnapshotRequired
	}
	pin, err := selection.OpenPinner(m).ReadPinnedSet(ctx, snapshot)
	if err != nil {
		return selection.DataPin{}, err
	}
	if pin.Data == nil {
		return selection.DataPin{}, ErrDataPinRequired
	}
	if _, _, err := selection.DataIdentity(*pin.Data); err != nil {
		return selection.DataPin{}, err
	}
	return *pin.Data, nil
}

const fileResultLimitMax = 100000

func validateFileResultLimit(limit int) error {
	if limit < 1 || limit > fileResultLimitMax {
		return ErrListfileLimit
	}
	return nil
}

// FileNameLookupRequest resolves one or more file data IDs to listed names.
type FileNameLookupRequest struct {
	Snapshot    string
	Listfile    ListfileRequest
	FileDataIDs []uint32
}

// FileNameLookupEntry reports every listed name for one ID. FileName is the
// first listing (or the explicit value "unknown"); Names keeps all listings so
// conflicting sources are never silently collapsed.
type FileNameLookupEntry struct {
	FileDataID uint32          `json:"fileDataID"`
	FileName   string          `json:"fileName"`
	Names      []ListfileMatch `json:"names,omitempty"`
}

type FileNameLookupResult struct {
	Context FileContext           `json:"context"`
	Entries []FileNameLookupEntry `json:"entries"`
}

// LookupFileNames is the fileDataID -> name capability. An ID without a
// listing reports the literal "unknown"; it is never guessed.
func LookupFileNames(ctx context.Context, root string, request FileNameLookupRequest) (FileNameLookupResult, error) {
	if len(request.FileDataIDs) == 0 || len(request.FileDataIDs) > 4096 {
		return FileNameLookupResult{}, ErrListfileQuery
	}
	reading, err := PrepareListfile(ctx, root, request.Listfile)
	if err != nil {
		return FileNameLookupResult{}, err
	}
	provenance := reading.Provenance
	return vault.ReadWorkspace(ctx, root, func(s *vault.Store, m *vault.Metadata) (FileNameLookupResult, error) {
		pin, err := pinnedDataContext(ctx, m, request.Snapshot)
		if err != nil {
			return FileNameLookupResult{}, err
		}
		result := FileNameLookupResult{
			Context: FileContext{Snapshot: request.Snapshot, Pin: pin, Source: "listfile", Listfile: &provenance},
			Entries: make([]FileNameLookupEntry, 0, len(request.FileDataIDs)),
		}
		for _, id := range request.FileDataIDs {
			names := reading.Index.Names(id)
			entry := FileNameLookupEntry{FileDataID: id, FileName: "unknown", Names: names}
			if len(names) > 0 {
				entry.FileName = names[0].FileName
			}
			result.Entries = append(result.Entries, entry)
		}
		return result, nil
	})
}

// FileNameResolveRequest resolves one listed name to file data IDs.
type FileNameResolveRequest struct {
	Snapshot string
	Listfile ListfileRequest
	FileName string
}

// FileNameResolution reports every ID listed for a name with its provenance.
// Legacy model aliases (.mdl/.mdx -> .m2) are labeled "m2-alias", never
// returned as exact matches.
type FileNameResolution struct {
	Context    FileContext     `json:"context"`
	FileName   string          `json:"fileName"`
	Normalized string          `json:"normalized"`
	Resolved   bool            `json:"resolved"`
	Matches    []ListfileMatch `json:"matches"`
}

// ResolveFileDataIDs is the name -> fileDataID capability. Multiple IDs are
// all reported; nothing is picked silently.
func ResolveFileDataIDs(ctx context.Context, root string, request FileNameResolveRequest) (FileNameResolution, error) {
	if request.FileName == "" {
		return FileNameResolution{}, ErrListfileQuery
	}
	reading, err := PrepareListfile(ctx, root, request.Listfile)
	if err != nil {
		return FileNameResolution{}, err
	}
	provenance := reading.Provenance
	return vault.ReadWorkspace(ctx, root, func(s *vault.Store, m *vault.Metadata) (FileNameResolution, error) {
		pin, err := pinnedDataContext(ctx, m, request.Snapshot)
		if err != nil {
			return FileNameResolution{}, err
		}
		matches := reading.Index.ResolveName(request.FileName)
		return FileNameResolution{
			Context:    FileContext{Snapshot: request.Snapshot, Pin: pin, Source: "listfile", Listfile: &provenance},
			FileName:   request.FileName,
			Normalized: normalizeListfileName(request.FileName),
			Resolved:   len(matches) > 0,
			Matches:    matches,
		}, nil
	})
}

// FileSearchRequest searches listed names with a case-insensitive substring.
type FileSearchRequest struct {
	Snapshot string
	Listfile ListfileRequest
	Query    string
	Limit    int
}

type FileSearchResult struct {
	Context   FileContext     `json:"context"`
	Query     string          `json:"query"`
	Total     int             `json:"total"`
	Returned  int             `json:"returned"`
	Truncated bool            `json:"truncated"`
	Files     []ListfileMatch `json:"files"`
}

// SearchFileNames is the listfile text search capability with an explicit
// bound; Total reports the full match count so truncation stays visible.
func SearchFileNames(ctx context.Context, root string, request FileSearchRequest) (FileSearchResult, error) {
	if err := validateFileResultLimit(request.Limit); err != nil {
		return FileSearchResult{}, err
	}
	reading, err := PrepareListfile(ctx, root, request.Listfile)
	if err != nil {
		return FileSearchResult{}, err
	}
	provenance := reading.Provenance
	return vault.ReadWorkspace(ctx, root, func(s *vault.Store, m *vault.Metadata) (FileSearchResult, error) {
		pin, err := pinnedDataContext(ctx, m, request.Snapshot)
		if err != nil {
			return FileSearchResult{}, err
		}
		total, files := reading.Index.Search(request.Query, request.Limit)
		return FileSearchResult{
			Context:   FileContext{Snapshot: request.Snapshot, Pin: pin, Source: "listfile", Listfile: &provenance},
			Query:     request.Query,
			Total:     total,
			Returned:  len(files),
			Truncated: total > len(files),
			Files:     files,
		}, nil
	})
}

// FileExtensionRequest lists names with one file extension.
type FileExtensionRequest struct {
	Snapshot  string
	Listfile  ListfileRequest
	Extension string
	Limit     int
}

type FileExtensionResult struct {
	Context   FileContext     `json:"context"`
	Extension string          `json:"extension"`
	Total     int             `json:"total"`
	Returned  int             `json:"returned"`
	Truncated bool            `json:"truncated"`
	Files     []ListfileMatch `json:"files"`
}

// ListFileExtensions is the extension listing capability; results are ordered
// by name then file ID and truncated visibly at Limit.
func ListFileExtensions(ctx context.Context, root string, request FileExtensionRequest) (FileExtensionResult, error) {
	if request.Extension == "" {
		return FileExtensionResult{}, ErrListfileQuery
	}
	if err := validateFileResultLimit(request.Limit); err != nil {
		return FileExtensionResult{}, err
	}
	reading, err := PrepareListfile(ctx, root, request.Listfile)
	if err != nil {
		return FileExtensionResult{}, err
	}
	provenance := reading.Provenance
	return vault.ReadWorkspace(ctx, root, func(s *vault.Store, m *vault.Metadata) (FileExtensionResult, error) {
		pin, err := pinnedDataContext(ctx, m, request.Snapshot)
		if err != nil {
			return FileExtensionResult{}, err
		}
		total, files := reading.Index.ByExtension(request.Extension, request.Limit)
		return FileExtensionResult{
			Context:   FileContext{Snapshot: request.Snapshot, Pin: pin, Source: "listfile", Listfile: &provenance},
			Extension: request.Extension,
			Total:     total,
			Returned:  len(files),
			Truncated: total > len(files),
			Files:     files,
		}, nil
	})
}
