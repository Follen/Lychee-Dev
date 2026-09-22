package container_test

import (
	"bytes"
	"context"
	"io"
	"os"
	"path/filepath"
	"testing"

	"github.com/follenfang/lycheedev/internal/records/container"
)

func TestRangesInsideArchiveSection(t *testing.T) {
	ctx := context.Background()
	inner := framedBLTE(plainBlock("nested"), zlibBlock(t, " frame"))
	object := framedBLTE(plainBlock("first|"), fixtureBlock{data: append([]byte{'F'}, inner...), decoded: 12}, plainBlock("|last"))
	prefix := bytes.Repeat([]byte{0xff}, 30)
	archive := append(append(prefix, object...), bytes.Repeat([]byte{0xff}, 100)...)
	path := filepath.Join(t.TempDir(), "data.000")
	if err := os.WriteFile(path, archive, 0600); err != nil {
		t.Fatal(err)
	}
	file, err := os.Open(path)
	if err != nil {
		t.Fatal(err)
	}
	defer file.Close()
	source := io.NewSectionReader(file, 30, int64(len(object)))
	ranges, err := container.OpenRanges(ctx, source, int64(len(object)), generousLimits, nil)
	if err != nil {
		t.Fatal(err)
	}
	want := []byte("first|nested frame|last")
	for offset := 0; offset <= len(want); offset++ {
		for length := 0; length <= len(want)-offset; length++ {
			got, err := ranges.ReadSpan(ctx, int64(offset), int64(length))
			if err != nil || !bytes.Equal(got, want[offset:offset+length]) {
				t.Fatalf("%d+%d = %q: %v", offset, length, got, err)
			}
		}
	}
	if err := file.Close(); err != nil {
		t.Fatal(err)
	}
	if data, err := ranges.ReadSpan(ctx, 0, 1); err == nil || data != nil {
		t.Fatalf("closed archive accepted: %q %v", data, err)
	}
}
