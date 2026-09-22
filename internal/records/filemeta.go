// SPDX-License-Identifier: AGPL-3.0-or-later
// File-existence/encoding semantics adapted from wowdata; see THIRD_PARTY_NOTICES.md.
package records

import (
	"bytes"
	"context"
	"encoding/hex"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"strconv"

	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/records/archive"
	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
)

var ErrFileQuery = errors.New("records.file_query")

// File existence states are deliberately separate outcomes: a missing listfile
// entry ("unlisted") is not "absent in CASC" ("present"/"absent"), and neither
// is a source that could not be prepared at all (ErrListfileUnavailable or a
// source error).
const (
	ListfileStateListed   = "listed"
	ListfileStateUnlisted = "unlisted"

	ContentStatePresent    = "present"
	ContentStateAbsent     = "absent"
	ContentStateUnverified = "unverified"
)

// FileExistenceRequest checks one file by file data ID or by listed name
// without exporting anything. Exactly one of FileDataID or FileName is set.
type FileExistenceRequest struct {
	File       FileQuery
	Listfile   ListfileRequest
	FileDataID uint32
	FileName   string
}

// FileExistenceMatch is one candidate of a name-based existence check.
type FileExistenceMatch struct {
	FileDataID    uint32 `json:"fileDataID"`
	FileName      string `json:"fileName"`
	SourceName    string `json:"sourceName,omitempty"`
	Component     string `json:"component,omitempty"`
	MatchedVia    string `json:"matchedVia,omitempty"`
	ListfileState string `json:"listfileState"`
	ContentState  string `json:"contentState"`
	LocaleMatched bool   `json:"localeMatched"`
	Exists        bool   `json:"exists"`
}

type FileExistenceResult struct {
	Context       FileContext          `json:"context"`
	FileDataID    uint32               `json:"fileDataID,omitempty"`
	FileName      string               `json:"fileName"`
	ListfileState string               `json:"listfileState"`
	ContentState  string               `json:"contentState"`
	LocaleMatched bool                 `json:"localeMatched"`
	Exists        bool                 `json:"exists"`
	Matches       []FileExistenceMatch `json:"matches,omitempty"`
}

type FileExistenceReading struct {
	Result  FileExistenceResult
	Capture evidence.CaptureRef
}

// InspectFileExistence is the existence capability: it consults the prepared
// listfile and the CASC Root for the pinned build and reports the two states
// separately instead of collapsing them into one boolean guess.
func InspectFileExistence(ctx context.Context, root, snapshot string, request FileExistenceRequest) (FileExistenceReading, error) {
	byID := request.FileDataID != 0
	byName := request.FileName != ""
	if byID == byName {
		return FileExistenceReading{}, ErrFileQuery
	}
	reading, err := PrepareListfile(ctx, root, request.Listfile)
	if err != nil {
		return FileExistenceReading{}, err
	}
	provenance := reading.Provenance
	return vault.WriteMetadata(ctx, root, func(s *vault.Store, m *vault.Metadata) (FileExistenceReading, error) {
		pin, err := pinnedDataContext(ctx, m, snapshot)
		if err != nil {
			return FileExistenceReading{}, err
		}
		_, locale, err := selection.DataIdentity(pin)
		if err != nil {
			return FileExistenceReading{}, err
		}
		result := FileExistenceResult{
			Context:       FileContext{Snapshot: snapshot, Pin: pin, Source: requestSourceLabel(&request.File), Listfile: &provenance},
			ListfileState: ListfileStateUnlisted,
			ContentState:  ContentStateUnverified,
		}
		var listed []ListfileMatch
		if byID {
			result.FileDataID = request.FileDataID
			listed = reading.Index.Names(request.FileDataID)
			result.FileName = "unknown"
			if len(listed) > 0 {
				result.FileName = listed[0].FileName
			}
		} else {
			result.FileName = request.FileName
			listed = reading.Index.ResolveName(request.FileName)
		}
		if len(listed) > 0 {
			result.ListfileState = ListfileStateListed
		}
		ids := make([]uint32, 0, len(listed)+1)
		seen := make(map[uint32]bool, len(listed))
		for _, match := range listed {
			if !seen[match.FileDataID] {
				seen[match.FileDataID] = true
				ids = append(ids, match.FileDataID)
			}
		}
		if byID {
			ids = []uint32{request.FileDataID}
		}
		if len(ids) == 0 {
			// A name without a listing has no ID to ask CASC about: the
			// listfile outcome must not be reported as a CASC outcome.
			result.Matches = []FileExistenceMatch{}
			return commitFileExistence(ctx, s, m, snapshot, pin, result)
		}
		query := request.File
		query.FileDataID = ids[0]
		entries, err := lookupRootEntries(ctx, s, pin, query, ids)
		if err != nil {
			return FileExistenceReading{}, err
		}
		result.ContentState = ContentStateAbsent
		if byID {
			state, matched := rootState(entries, request.FileDataID, locale)
			result.ContentState = state
			result.LocaleMatched = matched
			item := FileExistenceMatch{
				FileDataID:    request.FileDataID,
				FileName:      "unknown",
				ListfileState: result.ListfileState,
				ContentState:  state,
				LocaleMatched: matched,
				Exists:        result.ListfileState == ListfileStateListed && state == ContentStatePresent,
			}
			if len(listed) > 0 {
				item.FileName = listed[0].FileName
				item.SourceName = listed[0].SourceName
				item.Component = listed[0].Component
				item.MatchedVia = listed[0].MatchedVia
			}
			result.Matches = []FileExistenceMatch{item}
			result.Exists = item.Exists
		} else {
			for _, match := range listed {
				state, matched := rootState(entries, match.FileDataID, locale)
				item := FileExistenceMatch{
					FileDataID:    match.FileDataID,
					FileName:      match.FileName,
					SourceName:    match.SourceName,
					Component:     match.Component,
					MatchedVia:    match.MatchedVia,
					ListfileState: ListfileStateListed,
					ContentState:  state,
					LocaleMatched: matched,
					Exists:        state == ContentStatePresent,
				}
				result.Matches = append(result.Matches, item)
				if state == ContentStatePresent {
					result.ContentState = ContentStatePresent
					result.LocaleMatched = result.LocaleMatched || item.LocaleMatched
				}
				result.Exists = result.Exists || item.Exists
			}
		}
		return commitFileExistence(ctx, s, m, snapshot, pin, result)
	})
}

func commitFileExistence(ctx context.Context, s *vault.Store, m *vault.Metadata, snapshot string, pin selection.DataPin, result FileExistenceResult) (FileExistenceReading, error) {
	raw, err := json.Marshal(result)
	if err != nil {
		return FileExistenceReading{}, err
	}
	locator := "name:" + normalizeListfileName(result.FileName)
	if result.FileDataID != 0 {
		locator = "casc:fdid:" + strconv.FormatUint(uint64(result.FileDataID), 10)
	}
	capture, err := evidence.OpenArchive(s, m).CommitCapture(ctx, evidence.CaptureDraft{
		Reader: bytes.NewReader(raw), MaxBytes: 4 << 20, MediaType: "application/json", Complete: true,
		Provenance: evidence.Provenance{Kind: "file-existence", Locator: locator, Snapshot: snapshot, DataBuild: pin.FullBuild},
	})
	if err != nil {
		return FileExistenceReading{}, err
	}
	return FileExistenceReading{Result: result, Capture: capture}, nil
}

func rootState(entries []RootRecord, fileDataID, locale uint32) (string, bool) {
	state := ContentStateAbsent
	matched := false
	for _, entry := range entries {
		if entry.FileDataID != fileDataID {
			continue
		}
		state = ContentStatePresent
		if entry.LocaleMask&locale != 0 {
			matched = true
		}
	}
	return state, matched
}

func requestSourceLabel(query *FileQuery) string {
	if query.CDN {
		return "cdn"
	}
	return "installation"
}

// lookupRootEntries projects Root records for explicit IDs without extracting
// any file content. "Not in Root" is reported to the caller as an empty list.
func lookupRootEntries(ctx context.Context, s *vault.Store, pin selection.DataPin, query FileQuery, ids []uint32) ([]RootRecord, error) {
	reader := OpenReader(s)
	src, err := prepareFileSource(ctx, s, pin, query)
	if err != nil {
		return nil, err
	}
	defer src.done()
	index, closeIndex, err := src.encodingIndex(ctx, query)
	if err != nil {
		return nil, err
	}
	defer closeIndex()
	root, _, err := reader.extractContent(ctx, query, src.open, index, src.meta.RootContentKey, query.MetadataBytes)
	if err != nil {
		return nil, err
	}
	raw, err := s.ReadBlob(ctx, root, query.MetadataBytes)
	if err != nil {
		return nil, err
	}
	return LookupRoot(ctx, bytes.NewReader(raw), int64(len(raw)), ids, RootLimits{Bytes: query.MetadataBytes, Records: 10000000, Groups: 65536, Matches: 4096})
}

// ArchiveLocation is one local archive extent for an encoding key. It reports
// where the encoded BLTE object physically lives, including its 30-byte local
// preamble, and is metadata only; no payload bytes are read.
type ArchiveLocation struct {
	EncodingKey string `json:"encodingKey"`
	Index       string `json:"index"`
	Archive     uint16 `json:"archive"`
	File        string `json:"file"`
	Offset      int64  `json:"offset"`
	Bytes       int64  `json:"bytes"`
}

// FileEncodingRequest inspects CASC key metadata for one file data ID.
type FileEncodingRequest struct {
	File       FileQuery
	FileDataID uint32
}

// FileEncodingResult is CASC metadata only: Root content identity, every
// encoding-key alternative with its physical size, and local archive extents.
// No decoded content is fetched or described beyond its declared byte counts.
type FileEncodingResult struct {
	Context      FileContext       `json:"context"`
	FileDataID   uint32            `json:"fileDataID"`
	ContentKey   string            `json:"contentKey"`
	RootEntry    RootRecord        `json:"rootEntry"`
	DecodedBytes int64             `json:"decodedBytes"`
	EncodingKeys []string          `json:"encodingKeys"`
	Encoded      []EncodedRecord   `json:"encoded"`
	Archives     []ArchiveLocation `json:"archives,omitempty"`
}

type FileEncodingReading struct {
	Result  FileEncodingResult
	Capture evidence.CaptureRef
}

// InspectFileEncoding is the encoding metadata capability (legacy
// "file encoding"): content key, encoding keys, declared sizes and local
// archive locations for one file of the pinned build.
func InspectFileEncoding(ctx context.Context, root, snapshot string, request FileEncodingRequest) (FileEncodingReading, error) {
	if request.FileDataID == 0 {
		return FileEncodingReading{}, ErrFileQuery
	}
	return vault.WriteMetadata(ctx, root, func(s *vault.Store, m *vault.Metadata) (FileEncodingReading, error) {
		pin, err := pinnedDataContext(ctx, m, snapshot)
		if err != nil {
			return FileEncodingReading{}, err
		}
		_, locale, err := selection.DataIdentity(pin)
		if err != nil {
			return FileEncodingReading{}, err
		}
		query := request.File
		query.FileDataID = request.FileDataID
		reader := OpenReader(s)
		src, err := prepareFileSource(ctx, s, pin, query)
		if err != nil {
			return FileEncodingReading{}, err
		}
		defer src.done()
		index, closeIndex, err := src.encodingIndex(ctx, query)
		if err != nil {
			return FileEncodingReading{}, err
		}
		defer closeIndex()
		rootBlob, _, err := reader.extractContent(ctx, query, src.open, index, src.meta.RootContentKey, query.MetadataBytes)
		if err != nil {
			return FileEncodingReading{}, err
		}
		raw, err := s.ReadBlob(ctx, rootBlob, query.MetadataBytes)
		if err != nil {
			return FileEncodingReading{}, err
		}
		entries, err := LookupRoot(ctx, bytes.NewReader(raw), int64(len(raw)), []uint32{request.FileDataID}, RootLimits{Bytes: query.MetadataBytes, Records: 10000000, Groups: 65536, Matches: 4096})
		if err != nil {
			return FileEncodingReading{}, err
		}
		entry, err := chooseFile(entries, request.FileDataID, locale)
		if err != nil {
			return FileEncodingReading{}, err
		}
		record, err := index.FindContent(ctx, entry.ContentKey)
		if err != nil {
			return FileEncodingReading{}, err
		}
		result := FileEncodingResult{
			Context:      FileContext{Snapshot: snapshot, Pin: pin, Source: requestSourceLabel(&query)},
			FileDataID:   request.FileDataID,
			ContentKey:   entry.ContentKey,
			RootEntry:    entry,
			DecodedBytes: record.DecodedBytes,
			EncodingKeys: record.EncodingKeys,
		}
		for _, key := range record.EncodingKeys {
			physical, err := index.FindEncoding(ctx, key)
			if err != nil {
				return FileEncodingReading{}, err
			}
			result.Encoded = append(result.Encoded, physical)
			if !query.CDN {
				locations, err := localArchiveLocations(ctx, src.root, key, physical.EncodedBytes)
				if err != nil {
					return FileEncodingReading{}, err
				}
				result.Archives = append(result.Archives, locations...)
			}
		}
		rawResult, err := json.Marshal(result)
		if err != nil {
			return FileEncodingReading{}, err
		}
		capture, err := evidence.OpenArchive(s, m).CommitCapture(ctx, evidence.CaptureDraft{
			Reader: bytes.NewReader(rawResult), MaxBytes: 4 << 20, MediaType: "application/json", Complete: true,
			Provenance: evidence.Provenance{
				Kind: "file-encoding", Locator: "casc:fdid:" + strconv.FormatUint(uint64(request.FileDataID), 10) + ":ckey:" + entry.ContentKey,
				Snapshot: snapshot, DataBuild: pin.FullBuild,
			},
		})
		if err != nil {
			return FileEncodingReading{}, err
		}
		return FileEncodingReading{Result: result, Capture: capture}, nil
	})
}

// localArchiveLocations resolves local index extents for one encoding key
// without opening or hashing the stored payload.
func localArchiveLocations(ctx context.Context, root, encodingKey string, encodedBytes int64) ([]ArchiveLocation, error) {
	key, err := hex.DecodeString(encodingKey)
	if err != nil || len(key) != 16 {
		return nil, ErrMetadataFormat
	}
	var fixed [16]byte
	copy(fixed[:], key)
	dir, err := os.OpenRoot(root)
	if err != nil {
		return nil, err
	}
	defer dir.Close()
	indexes, err := localDirectories(dir)
	if err != nil {
		return nil, err
	}
	var locations []ArchiveLocation
	for _, bucket := range indexes {
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		file, err := dir.Open(filepath.Join("Data", "data", bucket.name))
		if err != nil {
			return nil, err
		}
		stat, err := file.Stat()
		if err != nil {
			file.Close()
			return nil, err
		}
		spans, err := archive.FindIndexSpans(ctx, file, stat.Size(), fixed, bucket.bucket)
		file.Close()
		if err != nil {
			return nil, err
		}
		for _, span := range spans {
			if span.Bytes != encodedBytes+30 {
				continue
			}
			locations = append(locations, ArchiveLocation{
				EncodingKey: encodingKey,
				Index:       bucket.name,
				Archive:     span.Archive,
				File:        span.Filename(),
				Offset:      span.Offset,
				Bytes:       span.Bytes,
			})
		}
	}
	return locations, nil
}
