package journal

import (
	"bytes"
	"context"
	"os"
	"path/filepath"
	"testing"
)

func TestRecoverMemoryTailPreservesEvidenceAndCommittedPrefix(t *testing.T) {
	ctx := context.Background()
	path := filepath.Join(t.TempDir(), "events.jsonl")
	if err := AppendMemoryEvent(ctx, path, "intent", map[string]string{"nonce": "same"}); err != nil {
		t.Fatal(err)
	}
	prefix, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	damaged := append(append([]byte(nil), prefix...), []byte(`{"sequence":2`)...)
	if err = os.WriteFile(path, damaged, 0600); err != nil {
		t.Fatal(err)
	}
	if err = RecoverMemoryTail(ctx, path); err != nil {
		t.Fatal(err)
	}
	restored, _ := os.ReadFile(path)
	if !bytes.Equal(restored, prefix) {
		t.Fatal("committed prefix changed")
	}
	files, _ := filepath.Glob(path + ".torn-*")
	if len(files) != 1 {
		t.Fatal("missing original evidence")
	}
	retained, _ := os.ReadFile(files[0])
	if !bytes.Equal(retained, damaged) {
		t.Fatal("original evidence changed")
	}
	if err = AppendMemoryEvent(ctx, path, "resumed", map[string]string{"nonce": "same"}); err != nil {
		t.Fatal(err)
	}
	if err = RecoverMemoryTail(ctx, path); err != nil {
		t.Fatal(err)
	}
	bad := append([]byte(nil), prefix...)
	bad[40] ^= 1
	if err = os.WriteFile(path, bad, 0600); err != nil {
		t.Fatal(err)
	}
	if err = RecoverMemoryTail(ctx, path); err == nil {
		t.Fatal("repaired a corrupted committed event")
	}
}

func TestMemoryLogDetectsCorruptionAndTornTail(t *testing.T) {
	path := filepath.Join(t.TempDir(), "events.jsonl")
	ctx := context.Background()
	for _, kind := range []string{"intent", "input", "result_durable"} {
		if err := AppendMemoryEvent(ctx, path, kind, map[string]string{"operation": "same"}); err != nil {
			t.Fatal(err)
		}
	}
	events, err := ReadMemoryLog(path)
	if err != nil || len(events) != 3 {
		t.Fatalf("%+v %v", events, err)
	}
	b, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(path, append(b, []byte(`{"sequence":4`)...), 0600); err != nil {
		t.Fatal(err)
	}
	events, err = ReadMemoryLog(path)
	if err == nil || len(events) != 3 {
		t.Fatal("torn tail not preserved", err)
	}
	if err = AppendMemoryEvent(ctx, path, "new", nil); err == nil {
		t.Fatal("silently appended to broken log")
	}
	b[40] ^= 1
	if err = os.WriteFile(path, b, 0600); err != nil {
		t.Fatal(err)
	}
	if _, err = ReadMemoryLog(path); err == nil {
		t.Fatal("corruption accepted")
	}
}
