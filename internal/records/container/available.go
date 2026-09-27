package container

import (
	"context"
	"errors"
	"fmt"
	"io"
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
		data, err := r.ReadSpan(ctx, offset, size)
		if err != nil {
			var key *MissingKeyError
			if !errors.As(err, &key) {
				return nil, err
			}
			missing = append(missing, MissingSpan{Chunk: i, Offset: offset, Bytes: size, KeyID: fmt.Sprintf("%016x", key.ID)})
			data = make([]byte, int(size))
		}
		n, err := dst.Write(data)
		if err != nil {
			return nil, err
		}
		if n != len(data) {
			return nil, io.ErrShortWrite
		}
	}
	return missing, ctx.Err()
}
