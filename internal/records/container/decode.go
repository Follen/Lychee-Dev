// SPDX-License-Identifier: MIT
// BLTE format handling adapted from wowdata (see LICENSE).
// Package container decodes bounded CASC payloads without owning caches or keys.
package container

import (
	"bytes"
	"compress/zlib"
	"context"
	"crypto/md5"
	"errors"
	"fmt"
	"io"

	"github.com/follenfang/lycheedev/internal/records/resource"
)

var (
	ErrMalformed      = errors.New("malformed BLTE container")
	ErrLimit          = errors.New("BLTE resource limit exceeded")
	ErrIntegrity      = errors.New("BLTE integrity mismatch")
	ErrUnsupported    = errors.New("unsupported BLTE encoding")
	ErrKeyUnavailable = errors.New("BLTE decryption key unavailable")
)

// Limits are mandatory. ChunkBytes bounds both encoded and decoded chunk buffers;
// Chunks applies across nested frames. Depth counts the root container as one
// and each nested frame or encrypted envelope as one additional level.
// Hard ceilings prevent accidental unbounded allocations from caller configuration.
type Limits struct {
	Query        *resource.Budget
	EncodedBytes int64
	DecodedBytes int64
	ChunkBytes   int64
	Chunks       int
	Depth        int
}

// Decode writes verified chunks in order. On failure, dst may contain earlier
// chunks; callers must stage output and publish only after success. src must end
// exactly at the container boundary. The caller owns and closes src and dst.
// Cancellation is checked between reads; it cannot interrupt a blocking Reader.
// Headerless containers have no embedded checksum; callers must verify their
// external CASC identity. A zero chunk checksum likewise makes no integrity claim.
func Decode(ctx context.Context, dst io.Writer, src io.Reader, limits Limits) (int64, error) {
	return DecodeWithKeys(ctx, dst, src, limits, nil)
}

// KeyLookup resolves the numeric TACT key name to a 16- or 32-byte Salsa20 key.
// Implementations must honor ctx and must not mutate returned bytes during use.
// Provider errors are deliberately not included in diagnostics (may contain keys).
type KeyLookup func(context.Context, uint64) ([]byte, error)

// DecodeWithKeys shares Decode's budgets and validation; keys are request-local.
func DecodeWithKeys(ctx context.Context, dst io.Writer, src io.Reader, limits Limits, keys KeyLookup) (int64, error) {
	if src == nil || dst == nil || !limits.valid() {
		return 0, fmt.Errorf("%w: invalid decoding budget", ErrLimit)
	}
	state := decoder{ctx: ctx, limits: limits, keys: keys}
	input := &boundedInput{ctx: ctx, source: src, remaining: limits.EncodedBytes}
	return state.expand(dst, input, 1, limits.DecodedBytes)
}

func (l Limits) valid() bool {
	return l.EncodedBytes > 0 && l.EncodedBytes <= 1<<50 && l.DecodedBytes > 0 && l.DecodedBytes <= 1<<50 &&
		l.ChunkBytes > 0 && l.ChunkBytes <= 64<<20 && l.Chunks > 0 && l.Chunks <= 65536 && l.Depth > 0 && l.Depth <= 16
}

type boundedInput struct {
	ctx       context.Context
	source    io.Reader
	remaining int64
}

func (r *boundedInput) Read(p []byte) (int, error) {
	if err := r.ctx.Err(); err != nil {
		return 0, err
	}
	if len(p) == 0 {
		return 0, nil
	}
	if r.remaining < 0 {
		return 0, ErrLimit
	}
	if int64(len(p)) > r.remaining+1 {
		p = p[:int(r.remaining+1)]
	}
	n, err := r.source.Read(p)
	if n < 0 || n > len(p) {
		return 0, fmt.Errorf("%w: invalid source read count", ErrMalformed)
	}
	r.remaining -= int64(n)
	if r.remaining < 0 {
		return 0, ErrLimit
	}
	return n, err
}

type decoder struct {
	ctx    context.Context
	limits Limits
	chunks int
	keys   KeyLookup
}

type segment struct {
	encoded uint32
	decoded uint32
	digest  [md5.Size]byte
}

func exact(r io.Reader, p []byte) error {
	_, err := io.ReadFull(r, p)
	if errors.Is(err, io.EOF) || errors.Is(err, io.ErrUnexpectedEOF) {
		return fmt.Errorf("%w: truncated payload", ErrMalformed)
	}
	return err
}

func exhausted(r io.Reader) error {
	var probe [1]byte
	n, err := io.ReadFull(r, probe[:])
	if n != 0 {
		return fmt.Errorf("%w: trailing bytes", ErrMalformed)
	}
	if errors.Is(err, io.EOF) {
		return nil
	}
	return err
}

func (d *decoder) expand(dst io.Writer, src io.Reader, depth int, allowance int64) (int64, error) {
	if err := d.ctx.Err(); err != nil {
		return 0, err
	}
	if depth > d.limits.Depth {
		return 0, ErrLimit
	}
	parts, header, err := d.layout(src, allowance, true)
	if err != nil {
		return 0, err
	}
	d.chunks += len(parts)
	if d.chunks > d.limits.Chunks {
		return 0, ErrLimit
	}
	var written int64
	for index, part := range parts {
		if err := d.ctx.Err(); err != nil {
			return written, err
		}
		budget := min(d.limits.ChunkBytes, allowance-written)
		n, err := d.writePart(dst, src, part, header, depth, budget, index, index == len(parts)-1)
		written += n
		if err != nil {
			return written, err
		}
	}
	return written, nil
}

func (d *decoder) writePart(dst io.Writer, src io.Reader, part segment, header uint64, depth int, allowance int64, index int, last bool) (int64, error) {
	encoded := int64(part.encoded)
	decoded := allowance
	if header == 0 {
		encoded = min(d.limits.ChunkBytes, d.limits.EncodedBytes)
		decoded = allowance
	}
	// io.ReadAll and bytes.Buffer grow geometrically. Reserve conservatively for
	// raw/decoded capacity and retain it through the destination write. Nested
	// frames reserve their own live buffers before allocating them.
	release, err := d.limits.Query.ReserveScratch(2*encoded + 2*decoded + 1024)
	if err != nil {
		return 0, err
	}
	defer release()
	var raw []byte
	if header == 0 {
		raw, err = d.collect(src, d.limits.ChunkBytes)
	} else {
		raw = make([]byte, int(part.encoded))
		err = exact(src, raw)
	}
	if err != nil {
		return 0, err
	}
	if part.digest != ([md5.Size]byte{}) && md5.Sum(raw) != part.digest {
		return 0, fmt.Errorf("%w: chunk %d checksum", ErrIntegrity, index)
	}
	out, err := d.unpack(raw, depth, allowance, index)
	if err != nil {
		return 0, fmt.Errorf("chunk %d: %w", index, err)
	}
	if header != 0 && int64(len(out)) != int64(part.decoded) {
		return 0, fmt.Errorf("%w: chunk %d decoded length", ErrIntegrity, index)
	}
	if last {
		if err := exhausted(src); err != nil {
			return 0, err
		}
	}
	if err := d.ctx.Err(); err != nil {
		return 0, err
	}
	n, err := dst.Write(out)
	if n < 0 || n > len(out) {
		return 0, fmt.Errorf("%w: invalid destination write count", ErrMalformed)
	}
	if err == nil && n != len(out) {
		err = io.ErrShortWrite
	}
	return int64(n), err
}

func (d *decoder) collect(src io.Reader, limit int64) ([]byte, error) {
	data, err := io.ReadAll(io.LimitReader(&boundedInput{ctx: d.ctx, source: src, remaining: limit}, limit+1))
	if err != nil {
		return nil, err
	}
	return data, nil
}

func (d *decoder) unpack(raw []byte, depth int, allowance int64, index int) ([]byte, error) {
	if err := d.ctx.Err(); err != nil {
		return nil, err
	}
	if depth > d.limits.Depth {
		return nil, ErrLimit
	}
	if len(raw) == 0 {
		return nil, ErrMalformed
	}
	switch raw[0] {
	case 'N':
		if int64(len(raw)-1) > allowance {
			return nil, ErrLimit
		}
		return raw[1:], nil
	case 'Z':
		// zlib owns a history window and Huffman tables in addition to the
		// caller's raw/output buffers. Reserve before constructing the reader.
		release, err := d.limits.Query.ReserveScratch(128 << 10)
		if err != nil {
			return nil, err
		}
		defer release()
		compressed := bytes.NewReader(raw[1:])
		stream, err := zlib.NewReader(compressed)
		if err != nil {
			return nil, fmt.Errorf("%w: zlib header: %v", ErrMalformed, err)
		}
		decoded, err := d.collect(stream, allowance)
		closeErr := stream.Close()
		if err != nil {
			return nil, err
		}
		if closeErr != nil {
			return nil, closeErr
		}
		if compressed.Len() != 0 {
			return nil, fmt.Errorf("%w: trailing zlib bytes", ErrMalformed)
		}
		return decoded, nil
	case 'F':
		var result bytes.Buffer
		_, err := d.expand(&result, bytes.NewReader(raw[1:]), depth+1, allowance)
		if err != nil {
			return nil, err
		}
		return result.Bytes(), nil
	case 'E':
		release, err := d.limits.Query.ReserveScratch(int64(len(raw)))
		if err != nil {
			return nil, err
		}
		defer release()
		decoded, err := d.unlock(raw[1:], index)
		if err != nil {
			return nil, err
		}
		return d.unpack(decoded, depth+1, allowance, index)
	default:
		return nil, fmt.Errorf("%w: 0x%02x", ErrUnsupported, raw[0])
	}
}
