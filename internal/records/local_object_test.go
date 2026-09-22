package records

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/records/archive"
)

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
