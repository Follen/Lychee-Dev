package channel

import (
	"context"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestJournalReferencesPayloadAndReadsLegacySnapshots(t *testing.T) {
	ctx := context.Background()
	i := Identity{Runtime: strings.Repeat("1", 32), NextSlot: 1, Slots: 200, GUID: "g", Character: "c", Realm: "r", Build: "b", Product: "retail", Release: "2.5.1"}
	d, err := New(filepath.Join(t.TempDir(), "c.jsonl"), &pendingBackend{}, i)
	if err != nil {
		t.Fatal(err)
	}
	d.State.Bound = true
	code := "return '" + strings.Repeat("payload", 20000) + "'"
	if err = d.PrepareRequest(ctx, "large", code, 5, "observation"); err != nil {
		t.Fatal(err)
	}
	before, _ := os.Stat(d.Log)
	for n := 0; n < 4; n++ {
		if err = d.Save(ctx, "observation"); err != nil {
			t.Fatal(err)
		}
	}
	after, _ := os.Stat(d.Log)
	if after.Size()-before.Size() > 12000 {
		t.Fatal("payload duplicated in observations")
	}
	restored, err := Load(d.Log, d.Backend)
	if err != nil || restored.State.Operation.Code != code {
		t.Fatal("hydrate", err)
	}
	historical, err := requestState(d.Log, "large")
	if err != nil || historical.Operation.Code != code {
		t.Fatal("history", err)
	}
	legacy := filepath.Join(t.TempDir(), "legacy.jsonl")
	if err = journal.AppendMemoryEvent(ctx, legacy, "legacy", d.State); err != nil {
		t.Fatal(err)
	}
	if restored, err = Load(legacy, d.Backend); err != nil || restored.State.Operation.Code != code {
		t.Fatal("legacy", err)
	}
	files, _ := os.ReadDir(filepath.Join(filepath.Dir(d.Log), "artifacts"))
	if len(files) != 1 {
		t.Fatal("expected one content blob", len(files))
	}
	if err = os.WriteFile(filepath.Join(filepath.Dir(d.Log), "artifacts", files[0].Name()), []byte("corrupt"), 0600); err != nil {
		t.Fatal(err)
	}
	if _, err = Load(d.Log, d.Backend); err == nil {
		t.Fatal("corrupt blob accepted")
	}
}
