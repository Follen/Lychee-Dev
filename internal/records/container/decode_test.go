package container_test

import (
	"bytes"
	"compress/zlib"
	"context"
	"crypto/md5"
	"encoding/binary"
	"errors"
	"io"
	"testing"

	"github.com/follenfang/lycheedev/internal/records/container"
)

var generousLimits = container.Limits{
	EncodedBytes: 1 << 20,
	DecodedBytes: 1 << 20,
	ChunkBytes:   1 << 16,
	Chunks:       32,
	Depth:        8,
}

type fixtureBlock struct {
	data     []byte
	decoded  int
	checksum []byte
}

func plainBlock(value string) fixtureBlock {
	payload := []byte(value)
	return fixtureBlock{data: append([]byte{'N'}, payload...), decoded: len(payload)}
}

func zlibBlock(t *testing.T, value string) fixtureBlock {
	t.Helper()
	var compressed bytes.Buffer
	writer := zlib.NewWriter(&compressed)
	if _, err := writer.Write([]byte(value)); err != nil {
		t.Fatalf("compress %q: %v", value, err)
	}
	if err := writer.Close(); err != nil {
		t.Fatalf("close zlib stream: %v", err)
	}
	return fixtureBlock{
		data:    append([]byte{'Z'}, compressed.Bytes()...),
		decoded: len(value),
	}
}

func modeBlock(mode byte, payload []byte, decoded int) fixtureBlock {
	data := make([]byte, 1+len(payload))
	data[0] = mode
	copy(data[1:], payload)
	return fixtureBlock{data: data, decoded: decoded}
}

func zeroHeaderBLTE(block []byte) []byte {
	data := make([]byte, 8+len(block))
	copy(data, "BLTE")
	binary.BigEndian.PutUint32(data[4:8], 0)
	copy(data[8:], block)
	return data
}

func framedBLTE(blocks ...fixtureBlock) []byte {
	headerSize := 12 + 24*len(blocks)
	dataSize := headerSize
	for _, block := range blocks {
		dataSize += len(block.data)
	}
	data := make([]byte, dataSize)
	copy(data, "BLTE")
	binary.BigEndian.PutUint32(data[4:8], uint32(headerSize))
	data[8] = 0x0f
	data[9] = byte(len(blocks) >> 16)
	data[10] = byte(len(blocks) >> 8)
	data[11] = byte(len(blocks))

	dataOffset := headerSize
	for index, block := range blocks {
		entry := 12 + index*24
		binary.BigEndian.PutUint32(data[entry:entry+4], uint32(len(block.data)))
		binary.BigEndian.PutUint32(data[entry+4:entry+8], uint32(block.decoded))
		digest := block.checksum
		if digest == nil {
			hash := md5.Sum(block.data)
			digest = hash[:]
		}
		copy(data[entry+8:entry+24], digest)
		copy(data[dataOffset:], block.data)
		dataOffset += len(block.data)
	}
	return data
}

func decodeBytes(t *testing.T, input []byte, limits container.Limits) (int64, []byte, error) {
	t.Helper()
	var output bytes.Buffer
	written, err := container.Decode(context.Background(), &output, bytes.NewReader(input), limits)
	return written, output.Bytes(), err
}

func requireErrorIs(t *testing.T, err error, target error) {
	t.Helper()
	if !errors.Is(err, target) {
		t.Fatalf("error = %v, want errors.Is(..., %v)", err, target)
	}
}

func requireDecoded(t *testing.T, input []byte, want string) {
	t.Helper()
	written, got, err := decodeBytes(t, input, generousLimits)
	if err != nil {
		t.Fatalf("Decode: %v", err)
	}
	if string(got) != want {
		t.Fatalf("decoded = %q, want %q", got, want)
	}
	if written != int64(len(want)) {
		t.Fatalf("written = %d, want %d", written, len(want))
	}
}

func TestDecodePlainAndZlibZeroHeader(t *testing.T) {
	requireDecoded(t, zeroHeaderBLTE(plainBlock("plain zero header").data), "plain zero header")
	requireDecoded(t, zeroHeaderBLTE(zlibBlock(t, "zlib zero header").data), "zlib zero header")
}

func TestDecodeFramedMultiBlockPreservesOrderAndChecksums(t *testing.T) {
	input := framedBLTE(
		plainBlock("first"),
		zlibBlock(t, " second"),
		plainBlock(" third"),
	)
	requireDecoded(t, input, "first second third")
}

func TestDecodeFramedAllowsAnAbsentChecksum(t *testing.T) {
	block := plainBlock("checksum is optional")
	block.checksum = make([]byte, md5.Size)
	requireDecoded(t, framedBLTE(block), "checksum is optional")
}

func TestDecodeNestedBLTE(t *testing.T) {
	inner := zeroHeaderBLTE(plainBlock("nested payload").data)
	outer := zeroHeaderBLTE(modeBlock('F', inner, len("nested payload")).data)
	requireDecoded(t, outer, "nested payload")
}

func TestDecodeRejectsWrongChecksumAndSizeBeforeWritingCurrentChunk(t *testing.T) {
	t.Run("checksum", func(t *testing.T) {
		bad := framedBLTE(plainBlock("verified prefix"), plainBlock("must not appear"))
		bad[20+24] ^= 0xff

		written, got, err := decodeBytes(t, bad, generousLimits)
		requireErrorIs(t, err, container.ErrIntegrity)
		if string(got) != "verified prefix" {
			t.Fatalf("output = %q, want only verified prefix", got)
		}
		if written != int64(len("verified prefix")) {
			t.Fatalf("written = %d, want %d", written, len("verified prefix"))
		}
	})

	t.Run("decoded size", func(t *testing.T) {
		bad := plainBlock("declared too short")
		bad.decoded--
		input := framedBLTE(bad)

		written, got, err := decodeBytes(t, input, generousLimits)
		requireErrorIs(t, err, container.ErrIntegrity)
		if len(got) != 0 || written != 0 {
			t.Fatalf("unverified output = %q, written %d", got, written)
		}
	})
}

func TestDecodeRejectsMalformedFramingAndTrailingBytes(t *testing.T) {
	valid := framedBLTE(plainBlock("framed"))
	truncatedHeader := make([]byte, 9)
	copy(truncatedHeader, "BLTE")
	binary.BigEndian.PutUint32(truncatedHeader[4:8], 12)
	truncatedHeader[8] = 0x0f

	truncatedEntry := make([]byte, 12+23)
	copy(truncatedEntry, "BLTE")
	binary.BigEndian.PutUint32(truncatedEntry[4:8], 36)
	truncatedEntry[8] = 0x0f
	truncatedEntry[11] = 1

	truncatedPayload := valid[:len(valid)-1]
	withTrailingBytes := append(append([]byte(nil), valid...), 0xde, 0xad)

	for name, input := range map[string][]byte{
		"truncated header":  truncatedHeader,
		"truncated entry":   truncatedEntry,
		"truncated payload": truncatedPayload,
		"trailing bytes":    withTrailingBytes,
	} {
		t.Run(name, func(t *testing.T) {
			written, got, err := decodeBytes(t, input, generousLimits)
			requireErrorIs(t, err, container.ErrMalformed)
			if written != 0 || len(got) != 0 {
				t.Fatalf("output on malformed input = %q, written %d", got, written)
			}
		})
	}
}

func TestDecodeRejectsUnsupportedMode(t *testing.T) {
	input := zeroHeaderBLTE(modeBlock('X', []byte("unsupported"), len("unsupported")).data)
	written, got, err := decodeBytes(t, input, generousLimits)
	requireErrorIs(t, err, container.ErrUnsupported)
	if written != 0 || len(got) != 0 {
		t.Fatalf("unsupported output = %q, written %d", got, written)
	}
}

func TestDecodeEncryptedModeReturnsKeyUnavailableWithoutZeroFill(t *testing.T) {
	input := zeroHeaderBLTE(modeBlock('E', append([]byte{8, 1, 0, 0, 0, 0, 0, 0, 0, 4, 0, 0, 0, 0, 'S'}, []byte("ciphertext")...), len("plaintext")).data)
	written, got, err := decodeBytes(t, input, generousLimits)
	requireErrorIs(t, err, container.ErrKeyUnavailable)
	if written != 0 || len(got) != 0 {
		t.Fatalf("encrypted output = %q, written %d", got, written)
	}
}

func TestDecodeRejectsNonPositiveLimits(t *testing.T) {
	input := zeroHeaderBLTE(plainBlock("valid input").data)
	fields := []struct {
		name string
		set  func(*container.Limits, int64)
	}{
		{name: "encoded bytes", set: func(l *container.Limits, value int64) { l.EncodedBytes = value }},
		{name: "decoded bytes", set: func(l *container.Limits, value int64) { l.DecodedBytes = value }},
		{name: "chunk bytes", set: func(l *container.Limits, value int64) { l.ChunkBytes = value }},
		{name: "chunks", set: func(l *container.Limits, value int64) { l.Chunks = int(value) }},
		{name: "depth", set: func(l *container.Limits, value int64) { l.Depth = int(value) }},
	}

	for _, field := range fields {
		for _, value := range []int64{0, -1} {
			t.Run(field.name+"/"+limitValueName(value), func(t *testing.T) {
				limits := generousLimits
				field.set(&limits, value)
				written, got, err := decodeBytes(t, input, limits)
				requireErrorIs(t, err, container.ErrLimit)
				if written != 0 || len(got) != 0 {
					t.Fatalf("output with invalid limit = %q, written %d", got, written)
				}
			})
		}
	}
}

func limitValueName(value int64) string {
	if value < 0 {
		return "negative"
	}
	return "zero"
}

func TestDecodeEnforcesAggregateAndStructuralLimits(t *testing.T) {
	nested := zeroHeaderBLTE(modeBlock('F', zeroHeaderBLTE(plainBlock("nested").data), len("nested")).data)
	input := framedBLTE(plainBlock("one"), plainBlock("two"))

	cases := []struct {
		name   string
		input  []byte
		limits container.Limits
	}{
		{name: "encoded bytes", input: input, limits: func() container.Limits {
			limits := generousLimits
			limits.EncodedBytes = int64(len(input) - 1)
			return limits
		}()},
		{name: "decoded bytes", input: input, limits: func() container.Limits {
			limits := generousLimits
			limits.DecodedBytes = int64(len("one") + len("two") - 1)
			return limits
		}()},
		{name: "chunk bytes", input: input, limits: func() container.Limits {
			limits := generousLimits
			limits.ChunkBytes = 1
			return limits
		}()},
		{name: "chunk count", input: input, limits: func() container.Limits {
			limits := generousLimits
			limits.Chunks = 1
			return limits
		}()},
		{name: "recursive depth", input: nested, limits: func() container.Limits {
			limits := generousLimits
			limits.Depth = 1
			return limits
		}()},
	}

	for _, testCase := range cases {
		t.Run(testCase.name, func(t *testing.T) {
			written, got, err := decodeBytes(t, testCase.input, testCase.limits)
			requireErrorIs(t, err, container.ErrLimit)
			if testCase.name == "recursive depth" && string(got) != "" {
				t.Fatalf("nested output with depth limit = %q", got)
			}
			if written < 0 {
				t.Fatalf("negative written count %d", written)
			}
		})
	}
}

func TestDecodeHonorsCancellation(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	var output bytes.Buffer
	written, err := container.Decode(ctx, &output, bytes.NewReader(zeroHeaderBLTE(plainBlock("cancelled").data)), generousLimits)
	if !errors.Is(err, context.Canceled) {
		t.Fatalf("error = %v, want context.Canceled", err)
	}
	if written != 0 || output.Len() != 0 {
		t.Fatalf("output after cancellation = %q, written %d", output.Bytes(), written)
	}
}

var errDestination = errors.New("destination failed")

type errorWriter struct{}

func (errorWriter) Write([]byte) (int, error) {
	return 0, errDestination
}

type shortWriter struct{}

func (shortWriter) Write(p []byte) (int, error) {
	if len(p) == 0 {
		return 0, nil
	}
	return len(p) - 1, nil
}

func TestDecodePropagatesDestinationErrorsAndShortWrites(t *testing.T) {
	input := zeroHeaderBLTE(plainBlock("destination").data)

	t.Run("writer error", func(t *testing.T) {
		written, err := container.Decode(context.Background(), errorWriter{}, bytes.NewReader(input), generousLimits)
		if !errors.Is(err, errDestination) {
			t.Fatalf("error = %v, want destination error", err)
		}
		if written != 0 {
			t.Fatalf("written = %d, want 0", written)
		}
	})

	t.Run("short write", func(t *testing.T) {
		written, err := container.Decode(context.Background(), shortWriter{}, bytes.NewReader(input), generousLimits)
		if !errors.Is(err, io.ErrShortWrite) {
			t.Fatalf("error = %v, want io.ErrShortWrite", err)
		}
		if written < 0 || written >= int64(len("destination")) {
			t.Fatalf("written = %d, want a partial nonnegative count", written)
		}
	})
}
