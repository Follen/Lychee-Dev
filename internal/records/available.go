package records

import (
	"context"
	"fmt"
	"io"

	"github.com/follenfang/lycheedev/internal/records/container"
	"github.com/follenfang/lycheedev/internal/records/table"
	"github.com/follenfang/lycheedev/internal/vault"
)

func (r *Reader) extractAvailable(ctx context.Context, source io.ReaderAt, encoded, decoded int64, keys container.KeyLookup) (vault.BlobRef, []container.MissingSpan, error) {
	ranges, err := container.OpenRanges(ctx, source, encoded, readLimits(encoded, max(1, decoded)), keys)
	if err != nil {
		return vault.BlobRef{}, nil, err
	}
	if ranges.Size() != decoded {
		return vault.BlobRef{}, nil, ErrMetadataFormat
	}
	input, output := io.Pipe()
	finished := make(chan struct{})
	var missing []container.MissingSpan
	go func() {
		defer close(finished)
		var err error
		missing, err = ranges.WriteAvailable(ctx, output)
		// A provider that changes its mind cannot bypass the full CKey check.
		if err == nil && len(missing) == 0 {
			err = container.ErrIntegrity
		}
		_ = output.CloseWithError(err)
	}()
	ref, err := r.store.PublishBlob(ctx, vault.BlobInput{Reader: input, MaxBytes: decoded})
	_ = input.CloseWithError(err)
	<-finished
	return ref, missing, err
}

type UnavailablePartition struct {
	Index       int      `json:"index"`
	KeyID       string   `json:"keyID"`
	Rows        uint32   `json:"rows"`
	Copies      uint32   `json:"copies"`
	RecordIDs   []uint32 `json:"recordIDs,omitempty"`
	IDsComplete bool     `json:"idsComplete"`
}

func unavailablePartitions(layout table.Layout, gaps []container.MissingSpan) ([]UnavailablePartition, map[int]bool, error) {
	skipped := map[int]bool{}
	for _, gap := range gaps {
		if gap.Offset < layout.MetadataEnd {
			return nil, nil, container.ErrKeyUnavailable
		}
		var covered int64
		for i, p := range layout.Partitions {
			start := int64(p.Offset)
			end := start + int64(p.Rows)*int64(layout.Stride) + int64(p.StringBytes)
			if layout.Flags&1 != 0 {
				end = int64(p.SparseOffset)
				if layout.Version == 2 {
					end += (int64(layout.LastID) - int64(layout.FirstID) + 1) * 6
				} else {
					end += int64(p.SparseCount) * 10
				}
			}
			end += int64(p.IdentityBytes) + int64(p.Copies)*8 + int64(p.RelationBytes)
			if start >= gap.Offset+gap.Bytes || end <= gap.Offset {
				continue
			}
			if p.KeyID == 0 || fmt.Sprintf("%016x", p.KeyID) != gap.KeyID {
				return nil, nil, container.ErrKeyUnavailable
			}
			covered += min(end, gap.Offset+gap.Bytes) - max(start, gap.Offset)
			skipped[i] = true
		}
		if covered != gap.Bytes {
			return nil, nil, container.ErrKeyUnavailable
		}
	}
	var result []UnavailablePartition
	for i, p := range layout.Partitions {
		if skipped[i] {
			result = append(result, UnavailablePartition{Index: i, KeyID: fmt.Sprintf("%016x", p.KeyID), Rows: p.Rows, Copies: p.Copies, RecordIDs: append([]uint32(nil), p.EncryptedIDs...), IDsComplete: p.IDsComplete})
		}
	}
	return result, skipped, nil
}

// Absence is established only when every unavailable partition has a complete
// identity list. Older formats or incomplete lists retain the unknown result.
func unavailableRecord(parts []UnavailablePartition, id uint32) (key string, unknown bool) {
	for _, p := range parts {
		for _, candidate := range p.RecordIDs {
			if candidate == id {
				return p.KeyID, false
			}
		}
		unknown = unknown || !p.IDsComplete
	}
	return "", unknown
}

// A placeholder must never be observable as a valid zero-valued field/string.
type availableReader struct {
	io.ReaderAt
	missing []container.MissingSpan
}

func (r availableReader) ReadAt(p []byte, offset int64) (int, error) {
	for _, gap := range r.missing {
		if offset < gap.Offset+gap.Bytes && gap.Offset < offset+int64(len(p)) {
			return 0, container.ErrKeyUnavailable
		}
	}
	return r.ReaderAt.ReadAt(p, offset)
}
