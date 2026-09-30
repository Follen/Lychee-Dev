package memory

import (
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"testing"

	"github.com/follenfang/lycheedev/internal/bridge"
)

func TestGeneralHintsRejectPersistedAndConstructedBody(t *testing.T) {
	hints := LoadHints("missing", "scope")
	hints.Entries = []Hint{{Address: 128, Header: bridge.MemoryHeader{Kind: bridge.MemoryBody}}}
	path := filepath.Join(t.TempDir(), "hints.json")
	if err := hints.Save(context.Background(), path); err == nil {
		t.Fatal("persisted general BODY hint")
	}
	b, err := json.Marshal(hints)
	if err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(path, b, 0600); err != nil {
		t.Fatal(err)
	}
	if len(LoadHints(path, "scope").Entries) != 0 || len(hints.entries()) != 0 {
		t.Fatal("loaded/scheduled general BODY hint")
	}
	hints.learn(Record{Address: 256, Header: bridge.MemoryHeader{Kind: bridge.MemoryReceipt}})
	if len(hints.Entries) != 1 || hints.Entries[0].Header.Kind == bridge.MemoryBody {
		t.Fatal("merge retained constructed BODY hint")
	}
}

func TestHintsAreOptionalAndCannotResurrectOldRecord(t *testing.T) {
	s := source(1 << 20)
	h := bridge.MemoryHeader{Kind: bridge.MemoryReceipt, State: 1}
	copy(h.Nonce[:], "nonce")
	copy(h.Runtime[:], "runtime")
	copy(h.Ticket[:], "ticket")
	b, _ := bridge.EncodeMemoryRecord(h, []byte("proof"))
	copy(s.data[1234:], b)
	selector := Selector{Nonce: h.Nonce, Runtime: h.Runtime, Ticket: h.Ticket, Kind: h.Kind}
	hints := LoadHints("missing", "process/build/wire")
	cold, err := Find(context.Background(), s, selector, hints, true)
	if err != nil || len(cold.Records) != 1 {
		t.Fatalf("%+v %v", cold, err)
	}
	warm, err := Find(context.Background(), s, selector, hints, true)
	if err != nil || warm.Path != "cache_hit" || warm.Coverage.Complete {
		t.Fatalf("%+v %v", warm, err)
	}
	for i := 1234; i < 1234+len(b); i++ {
		s.data[i] = 0
	}
	copy(s.data[6789:], b)
	moved, err := Find(context.Background(), s, selector, hints, true)
	if err != nil || len(moved.Records) != 1 || moved.Records[0].Address != 6789 || moved.Path == "cache_hit" {
		t.Fatalf("%+v %v", moved, err)
	}
	audit, err := Find(context.Background(), s, selector, hints, false)
	if err != nil || !audit.Coverage.Complete {
		t.Fatalf("%+v %v", audit, err)
	}
	off, err := Find(context.Background(), s, selector, nil, true)
	if err != nil || len(off.Records) != 1 {
		t.Fatalf("%+v %v", off, err)
	}
	path := filepath.Join(t.TempDir(), "hints.json")
	if err = hints.Save(context.Background(), path); err != nil {
		t.Fatal(err)
	}
	if len(LoadHints(path, "another process").Entries) != 0 {
		t.Fatal("reused process cache")
	}
	if err = os.WriteFile(path, []byte("broken"), 0600); err != nil {
		t.Fatal(err)
	}
	if len(LoadHints(path, hints.Scope).Entries) != 0 {
		t.Fatal("corrupt hints accepted")
	}
}
