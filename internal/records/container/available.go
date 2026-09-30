package container

import (
	"context"
	"errors"
	"fmt"
	"io"

	"github.com/follenfang/lycheedev/internal/records/resource"
)

// MissingSpan is an authenticated chunk's unavailable decoded extent. Zeros
// written there are placeholders, never content. Consumers MUST retain this map.
type MissingSpan struct {
	Chunk  int    `json:"chunk"`
	Offset int64  `json:"offset"`
	Bytes  int64  `json:"bytes"`
	KeyID  string `json:"keyID"`
}

// WriteAvailable checks every encoded chunk, retaining only missing-key gaps.
// The caller authenticates the BLTE header/EKey. All other failures are fatal.
// The output is not eligible for a full decoded CKey integrity claim.
func (r *Ranges) WriteAvailable(ctx context.Context, dst io.Writer) ([]MissingSpan, error) {
	var missing []MissingSpan
	for i, part := range r.parts {
		offset, size := r.decodedOffsets[i], int64(part.decoded)
		if size == 0 {
			return nil, ErrMalformed
		}
		gap, err := r.writeAvailablePart(ctx, dst, i, offset, size)
		if err != nil {
			return nil, err
		}
		if gap != nil {
			if err := r.limits.Query.Charge(resource.Cost{RetainedBytes: 128, MetadataBytes: 128}); err != nil {
				return nil, err
			}
			missing = append(missing, *gap)
		}
	}
	return missing, ctx.Err()
}

func (r *Ranges) writeAvailablePart(ctx context.Context, dst io.Writer, i int, offset, size int64) (*MissingSpan, error) {
	if size > r.limits.ChunkBytes {
		return nil, ErrLimit
	}
	release, err := r.limits.Query.ReserveScratch(size)
	if err != nil {
		return nil, err
	}
	defer release()
	var gap *MissingSpan
	data, err := r.ReadSpan(ctx, offset, size)
	if err != nil {
		var key *MissingKeyError
		if !errors.As(err, &key) {
			return nil, err
		}
		gap = &MissingSpan{Chunk: i, Offset: offset, Bytes: size, KeyID: fmt.Sprintf("%016x", key.ID)}
		data = make([]byte, int(size))
	}
	n, err := dst.Write(data)
	if err != nil {
		return nil, err
	}
	if n != len(data) {
		return nil, io.ErrShortWrite
	}
	return gap, ctx.Err()
}
