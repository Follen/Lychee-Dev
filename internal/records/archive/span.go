// SPDX-License-Identifier: AGPL-3.0-or-later
// CASC format handling adapted from wowdata; see THIRD_PARTY_NOTICES.md.
package archive

import (
	"bytes"
	"context"
	"crypto/md5"
	"encoding/binary"
	"fmt"
	"io"
)

// OpenEncodedSpan verifies the full encoding key, not merely the local index's
// prefix. The caller owns the archive and must keep it immutable until finished.
// Framed EKeys cover the BLTE directory; headerless EKeys cover the whole BLTE.
// Decoded content-key verification still belongs to PayloadArchive.
func OpenEncodedSpan(ctx context.Context, source io.ReaderAt, archiveSize int64, span Span, key [16]byte, maxBytes int64) (*io.SectionReader, error) {
	if source == nil || span.Offset < 0 || span.Bytes < 38 || archiveSize < 0 || span.Offset > archiveSize || span.Bytes > archiveSize-span.Offset {
		return nil, ErrIndexFormat
	}
	if maxBytes <= 0 || maxBytes > 1<<50 || span.Bytes-30 > maxBytes {
		return nil, ErrIndexLimit
	}
	reader := io.NewSectionReader(source, span.Offset, span.Bytes)
	var header [38]byte
	if _, err := io.ReadFull(&contextReader{ctx, reader}, header[:]); err != nil {
		return nil, err
	}
	// Local preambles may store only the same nine-byte key prefix as the index.
	// Full identity is established below from BLTE, never from padded preambles.
	for i := 0; i < 9; i++ {
		if header[15-i] != key[i] {
			return nil, fmt.Errorf("%w: archive preamble key", ErrIndexIntegrity)
		}
	}
	if !bytes.Equal(header[30:34], []byte("BLTE")) {
		return nil, ErrIndexFormat
	}
	length := int64(binary.BigEndian.Uint32(header[34:38]))
	if length != 0 && (length < 12 || length > span.Bytes-30) {
		return nil, ErrIndexFormat
	}
	if length == 0 {
		length = span.Bytes - 30
	}
	hash := md5.New()
	n, err := io.Copy(hash, &contextReader{ctx, io.NewSectionReader(source, span.Offset+30, length)})
	if err != nil {
		return nil, err
	}
	if n != length || !bytes.Equal(hash.Sum(nil), key[:]) {
		return nil, fmt.Errorf("%w: BLTE encoding key", ErrIndexIntegrity)
	}
	return io.NewSectionReader(source, span.Offset+30, span.Bytes-30), nil
}

type contextReader struct {
	ctx    context.Context
	source io.Reader
}

func (r *contextReader) Read(p []byte) (int, error) {
	if err := r.ctx.Err(); err != nil {
		return 0, err
	}
	return r.source.Read(p)
}
