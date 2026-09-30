package protocol_test

import (
	"bytes"
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"testing"

	"github.com/follenfang/lycheedev/internal/bridge"
)

func TestPublicationWireLuaGoEquivalence(t *testing.T) {
	dir := t.TempDir()
	root, err := filepath.Abs(filepath.Join("..", "..", "addon"))
	if err != nil {
		t.Fatal(err)
	}
	log, err := exec.Command(luaRuntime(t), "publication_wire.lua", filepath.ToSlash(root), filepath.ToSlash(dir)).CombinedOutput()
	if err != nil {
		t.Fatalf("Lua fixture: %v\n%s", err, log)
	}
	paths, err := filepath.Glob(filepath.Join(dir, "*.bin"))
	if err != nil || len(paths) != 23 {
		t.Fatalf("records=%d err=%v", len(paths), err)
	}
	records, payloads, headers := map[string][]byte{}, map[string][]byte{}, map[string]bridge.MemoryHeader{}
	for _, path := range paths {
		name := filepath.Base(path)
		b, err := os.ReadFile(path)
		if err != nil {
			t.Fatal(err)
		}
		h, payload, err := bridge.DecodeMemoryRecord(b)
		if err != nil {
			t.Fatalf("%s: %v", name, err)
		}
		encoded, err := bridge.EncodeMemoryRecord(h, payload)
		if err != nil || !bytes.Equal(encoded, b) {
			t.Fatalf("%s Lua/Go byte mismatch: %v", name, err)
		}
		records[name], payloads[name], headers[name] = b, payload, h
	}
	for kind := 1; kind <= 5; kind++ {
		for _, sequence := range []uint32{0, 1, ^uint32(0)} {
			name := fmt.Sprintf("wire-%d-%d.bin", kind, sequence)
			want := append([]byte("荔枝\x00\xff\n\\\""), byte(kind))
			if h := headers[name]; h.Kind != bridge.MemoryKind(kind) || h.State != 255 || h.Sequence != sequence || !bytes.Equal(payloads[name], want) {
				t.Fatalf("%s: header=%+v payload=%q", name, h, payloads[name])
			}
		}
	}
	if len(payloads["maximum.bin"]) != bridge.MemoryMaxPayload {
		t.Fatal("maximum payload changed")
	}
	if !bytes.Equal(records["identity-first.bin"], records["identity-same.bin"]) {
		t.Fatal("unchanged descriptor changed framing")
	}
	inside, after := "identity-inside-bind.bin", "identity-after-bind.bin"
	if !bytes.Equal(payloads[inside], payloads[after]) || bytes.Equal(records[inside], records[after]) || headers[inside].Sequence != 0 || headers[after].Sequence != 1 {
		t.Fatal("same identity payload did not preserve fresh header sequence")
	}
	for _, test := range []struct {
		name, owner, character string
		fence, slot            int
	}{
		{"identity-first.bin", "", "荔枝\"\n\x00", 0, 1},
		{"identity-after-bind.bin", "22222222222222222222222222222222", "荔枝\"\n\x00", 1, 2},
		{"identity-empty.bin", "22222222222222222222222222222222", "荔枝\"\n\x00", 1, 3},
		{"identity-actor.bin", "22222222222222222222222222222222", "Relogin", 1, 3},
	} {
		var id struct {
			Owner, Character, Realm string
			Fence, NextSlot, Slots  int
		}
		if err := json.Unmarshal(payloads[test.name], &id); err != nil {
			t.Fatal(err)
		}
		if id.Owner != test.owner || id.Character != test.character || id.Realm != "服\\" || id.Fence != test.fence || id.NextSlot != test.slot || id.Slots != 200 {
			t.Fatalf("%s: %+v", test.name, id)
		}
	}
}
