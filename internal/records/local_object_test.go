package records

import (
	"bytes"
	"context"
	"crypto/md5"
	"encoding/hex"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/records/archive"
	"github.com/follenfang/lycheedev/internal/records/resource"
)

func TestLocalObjectReadsPreferredBucketWithinBudget(t *testing.T) {
	ctx := context.Background()
	raw := headerlessBLTE([]byte("local bucket budget regression 1"))
	key := md5.Sum(raw)
	var folded byte
	for _, b := range key[:9] {
		folded ^= b
	}
	preferred := (folded >> 4) ^ (folded & 15)
	if preferred == 0 {
		t.Fatal("fixture must exercise a later bucket")
	}
	root := t.TempDir()
	data := filepath.Join(root, "Data", "data")
	if err := os.MkdirAll(data, 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(data, "data.000"), append(make([]byte, 30), raw...), 0600); err != nil {
		t.Fatal(err)
	}
	dir, err := os.OpenRoot(root)
	if err != nil {
		t.Fatal(err)
	}
	defer dir.Close()
	var files []localDirectory
	for bucket := byte(0); bucket < 16; bucket++ {
		files = append(files, localDirectory{name: fmt.Sprintf("%02x00000001.idx", bucket), bucket: bucket, generation: 1})
	}
	for _, actual := range []byte{preferred, 0} {
		t.Run(fmt.Sprintf("stored-in-%02x", actual), func(t *testing.T) {
			limits := resource.DefaultLimits()
			limits.MetadataBytes = 2 << 20
			if actual != preferred {
				limits.MetadataBytes *= 16
			}
			budget := resource.New(limits)
			var visited []byte
			object, err := openLocalObject(ctx, dir, files, hex.EncodeToString(key[:]), int64(len(raw)), func(index localDirectory, candidate [16]byte) ([]archive.Span, error) {
				visited = append(visited, index.bucket)
				if err := budget.Charge(resource.Cost{MetadataBytes: 2 << 20}); err != nil {
					return nil, err
				}
				if index.bucket != actual {
					return nil, nil
				}
				return []archive.Span{{Bytes: int64(len(raw) + 30)}}, nil
			})
			if err != nil {
				t.Fatalf("lookup visited %v: %v", visited, err)
			}
			defer object.Close()
			got, err := io.ReadAll(object)
			if err != nil || !bytes.Equal(got, raw) || visited[0] != preferred {
				t.Fatalf("lookup visited %v, body=%x: %v", visited, got, err)
			}
			if actual == preferred && len(visited) != 1 {
				t.Fatalf("unrelated indexes consumed budget: %v", visited)
			}
		})
	}
}

func TestLocalDirectoryGenerations(t *testing.T) {
	root := t.TempDir()
	data := filepath.Join(root, "Data", "data")
	if err := os.MkdirAll(data, 0700); err != nil {
		t.Fatal(err)
	}
	for _, name := range []string{"0100000001.idx", "00000000ff.idx", "0100000003.idx", "0000000100.idx", "1000000001.idx", "garbage.idx", "data.001"} {
		if err := os.WriteFile(filepath.Join(data, name), nil, 0600); err != nil {
			t.Fatal(err)
		}
	}
	if err := os.Mkdir(filepath.Join(data, "0200000001.idx"), 0700); err != nil {
		t.Fatal(err)
	}
	dir, err := os.OpenRoot(root)
	if err != nil {
		t.Fatal(err)
	}
	defer dir.Close()
	selected, err := localDirectories(dir)
	if err != nil || len(selected) != 2 || selected[0].name != "0000000100.idx" || selected[1].name != "0100000003.idx" {
		t.Fatalf("%+v %v", selected, err)
	}
}

func TestLocalObjectRejectsBadInputsAndNewestIndex(t *testing.T) {
	ctx := context.Background()
	for _, key := range []string{"", strings.Repeat("a", 31), strings.Repeat("A", 32), "../../not-an-encoding-key"} {
		if object, err := OpenLocalObject(ctx, t.TempDir(), key, 10); object != nil || !errors.Is(err, ErrMetadataFormat) {
			t.Fatalf("key %q: %+v %v", key, object, err)
		}
	}
	root := t.TempDir()
	data := filepath.Join(root, "Data", "data")
	if err := os.MkdirAll(data, 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(data, "0000000001.idx"), make([]byte, 58), 0600); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(data, "0000000002.idx"), []byte("damaged latest"), 0600); err != nil {
		t.Fatal(err)
	}
	object, err := OpenLocalObject(ctx, root, strings.Repeat("a", 32), 10)
	if object != nil || !errors.Is(err, archive.ErrIndexFormat) || !strings.Contains(err.Error(), "0000000002.idx") {
		t.Fatalf("latest error hidden: %+v %v", object, err)
	}
	cancelled, cancel := context.WithCancel(ctx)
	cancel()
	if _, err := OpenLocalObject(cancelled, root, strings.Repeat("a", 32), 10); !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
}
