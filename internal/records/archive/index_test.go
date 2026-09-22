package archive

import (
	"bytes"
	"context"
	"crypto/md5"
	"encoding/binary"
	"errors"
	"io"
	"os"
	"path/filepath"
	"testing"

	"github.com/follenfang/lycheedev/internal/records/container"
)

func TestGuardReferenceVectors(t *testing.T) {
	// Bob Jenkins' public-domain lookup3.c driver5 reference vectors. The C
	// string is hashed without its terminating NUL (30 bytes).
	for _, v := range []struct {
		s          string
		p, q, c, b uint32
	}{
		{"", 0, 0, 0xdeadbeef, 0xdeadbeef},
		{"", 0, 0xdeadbeef, 0xbd5b7dde, 0xdeadbeef},
		{"Four score and seven years ago", 0, 0, 0x17770551, 0xce7226e6},
		{"Four score and seven years ago", 0, 1, 0xe3607cae, 0xbd371de4},
		{"Four score and seven years ago", 1, 0, 0xcd628161, 0x6cbea4b3},
	} {
		c, b := guardSum([]byte(v.s), v.p, v.q)
		if c != v.c || b != v.b {
			t.Fatalf("%q: %x %x", v.s, c, b)
		}
	}
}

func indexFixture(key [16]byte) []byte {
	data := make([]byte, 58)
	binary.LittleEndian.PutUint32(data, 16)
	copy(data[8:], []byte{7, 0, 0, 0, 4, 5, 9, 30})
	binary.LittleEndian.PutUint64(data[16:], 1<<30)
	h, _ := guardSum(data[8:24], 0, 0)
	binary.LittleEndian.PutUint32(data[4:], h)
	binary.LittleEndian.PutUint32(data[32:], 18)
	copy(data[40:49], key[:9])
	binary.BigEndian.PutUint32(data[50:], 1<<30|64)
	binary.LittleEndian.PutUint32(data[54:], 128)
	h, _ = guardSum(data[40:], 0, 0)
	binary.LittleEndian.PutUint32(data[36:], h)
	return data
}

func TestIndexVerification(t *testing.T) {
	key := [16]byte{1, 2, 3}
	fixture := indexFixture(key)
	for _, mode := range []string{"valid", "header", "entry", "truncated", "cancelled", "bucket"} {
		t.Run(mode, func(t *testing.T) {
			data := bytes.Clone(fixture)
			ctx, cancel := context.WithCancel(context.Background())
			defer cancel()
			var bucket byte
			switch mode {
			case "header":
				data[4] ^= 1
			case "entry":
				data[57] ^= 1
			case "truncated":
				data = data[:55]
			case "cancelled":
				cancel()
			case "bucket":
				bucket = 1
			}
			spans, err := FindIndexSpans(ctx, bytes.NewReader(data), int64(len(data)), key, bucket)
			if mode == "valid" {
				if err != nil || len(spans) != 1 || spans[0] != (Span{Archive: 1, Offset: 64, Bytes: 128}) {
					t.Fatalf("%+v %v", spans, err)
				}
			} else if err == nil || spans != nil {
				t.Fatalf("corrupt result %+v %v", spans, err)
			}
		})
	}
}

func TestEncodedSpanIdentity(t *testing.T) {
	body := []byte("BLTE\x00\x00\x00\x00Noriginal")
	key := md5.Sum(body)
	data := make([]byte, 64+30+len(body))
	for i := range key {
		data[64+15-i] = key[i]
	}
	clear(data[64:71]) // Installed archives may pad the unused key suffix.
	copy(data[94:], body)
	span := Span{Offset: 64, Bytes: int64(30 + len(body))}
	r, err := OpenEncodedSpan(context.Background(), bytes.NewReader(data), int64(len(data)), span, key, 4096)
	if err != nil {
		t.Fatal(err)
	}
	got, err := io.ReadAll(r)
	if err != nil || !bytes.Equal(got, body) {
		t.Fatalf("%q %v", got, err)
	}
	data[len(data)-1] ^= 1
	if _, err := OpenEncodedSpan(context.Background(), bytes.NewReader(data), int64(len(data)), span, key, 4096); !errors.Is(err, ErrIndexIntegrity) {
		t.Fatal(err)
	}
}

func TestIndexCollisionsAndOrdering(t *testing.T) {
	key := [16]byte{5, 4, 3}
	data := indexFixture(key)
	data = append(data, bytes.Clone(data[40:58])...)
	binary.LittleEndian.PutUint32(data[32:], 36)
	binary.BigEndian.PutUint32(data[68:], 2<<30|128)
	seal := func() {
		var p, q uint32
		for i := 40; i < len(data); i += 18 {
			p, q = guardSum(data[i:i+18], p, q)
		}
		binary.LittleEndian.PutUint32(data[36:], p)
	}
	seal()
	spans, err := FindIndexSpans(context.Background(), bytes.NewReader(data), int64(len(data)), key, 0)
	if err != nil || len(spans) != 2 || spans[0].Archive != 1 || spans[1].Archive != 2 {
		t.Fatalf("collision lost: %+v %v", spans, err)
	}
	data[58] = 1
	seal()
	if _, err := FindIndexSpans(context.Background(), bytes.NewReader(data), int64(len(data)), key, 0); !errors.Is(err, ErrIndexFormat) {
		t.Fatalf("unsorted index: %v", err)
	}
}

func FuzzLocalIndex(f *testing.F) {
	f.Add(indexFixture([16]byte{1}))
	f.Fuzz(func(t *testing.T, data []byte) {
		spans, err := FindIndexSpans(context.Background(), bytes.NewReader(data), int64(len(data)), [16]byte{1}, 0)
		if err != nil && spans != nil {
			t.Fatal("partial candidate list on error")
		}
		if len(spans) > 64 {
			t.Fatal("unbounded candidates")
		}
		for _, span := range spans {
			if span.Offset < 0 || span.Bytes < 38 || span.Archive > 4095 {
				t.Fatalf("bad span %+v", span)
			}
		}
	})
}

func TestInstalledIndexReadOnly(t *testing.T) {
	path := os.Getenv("LYCHEEDEV_TEST_INDEX")
	if path == "" {
		t.Skip("explicit local index fixture path required")
	}
	f, err := os.Open(path)
	if err != nil {
		t.Fatal(err)
	}
	defer f.Close()
	var sample [58]byte
	if _, err := f.ReadAt(sample[:], 0); err != nil {
		t.Fatal(err)
	}
	var key [16]byte
	copy(key[:9], sample[40:49])
	info, err := f.Stat()
	if err != nil {
		t.Fatal(err)
	}
	spans, err := FindIndexSpans(context.Background(), f, info.Size(), key, sample[10])
	if err != nil || len(spans) == 0 {
		t.Fatalf("local index: %+v %v", spans, err)
	}
	for _, span := range spans {
		file, err := os.Open(filepath.Join(filepath.Dir(path), span.Filename()))
		if err != nil {
			t.Fatal(err)
		}
		var preamble [16]byte
		_, err = file.ReadAt(preamble[:], span.Offset)
		if err != nil {
			file.Close()
			t.Fatal(err)
		}
		if span.Bytes > 64<<20 {
			file.Close()
			t.Fatal("local fixture exceeds budget")
		}
		encoded := make([]byte, int(span.Bytes-30))
		if _, err := file.ReadAt(encoded, span.Offset+30); err != nil {
			file.Close()
			t.Fatal(err)
		}
		headerSize := int64(binary.BigEndian.Uint32(encoded[4:8]))
		if headerSize < 0 || headerSize > int64(len(encoded)) {
			file.Close()
			t.Fatal("bad BLTE header")
		}
		if headerSize == 0 {
			headerSize = int64(len(encoded))
		}
		fullkey := md5.Sum(encoded[:headerSize])
		if !bytes.Equal(fullkey[:9], key[:9]) {
			file.Close()
			t.Fatal("BLTE identity does not match index prefix")
		}
		key = fullkey
		stat, err := file.Stat()
		if err != nil {
			file.Close()
			t.Fatal(err)
		}
		reader, openErr := OpenEncodedSpan(context.Background(), file, stat.Size(), span, key, 64<<20)
		if openErr != nil {
			file.Close()
			t.Fatal(openErr)
		}
		decoded, decodeErr := container.Decode(context.Background(), io.Discard, reader, container.Limits{EncodedBytes: 64 << 20, DecodedBytes: 64 << 20, ChunkBytes: 64 << 20, Chunks: 65536, Depth: 16})
		file.Close()
		if decodeErr != nil {
			t.Fatal(decodeErr)
		}
		t.Logf("verified index, matching encoding identity and BLTE decode: %s offset=%d bytes=%d decoded=%d", span.Filename(), span.Offset, span.Bytes, decoded)
	}
}
