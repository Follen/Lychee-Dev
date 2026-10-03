package memory

import (
	"context"
	"encoding/binary"
	"math"
	"strings"
	"testing"
)

func duplexFixture(t *testing.T) (*mailboxFixture, *MailboxReader, uint64) {
	f := newMailboxFixture(t, 0x140000000, 0x200000000)
	array := func(label string, values []float64) uint64 {
		at := f.table(label, nil)
		ptr, b := f.allocate(len(values) * 24)
		h := f.bytes(at, 72)
		binary.LittleEndian.PutUint32(h[64:68], uint32(len(values)))
		binary.LittleEndian.PutUint64(h[32:40], ptr)
		for i, v := range values {
			binary.LittleEndian.PutUint64(b[i*24:], math.Float64bits(v))
			b[i*24+8] = 3
		}
		return at
	}
	cells := array("cells", []float64{0, 42, 4294967295})
	cal := array("cal", []float64{0, 1, 4294967295, 0.125, -13.5, 7654321})
	inbox := f.table("inbox", []mailboxFixtureEntry{{"calibration", luaValue{cal, 5}}, {"cells", luaValue{cells, 5}}})
	sendbox := f.table("sendbox", []mailboxFixtureEntry{{"status", luaValue{f.text([]byte("immutable-status")), 4}}})
	box := f.table("duplex", []mailboxFixtureEntry{{"schema", luaValue{f.text([]byte(DuplexMailboxSchema)), 4}}, {"release", luaValue{f.text([]byte("fixture-release")), 4}}, {"runtime", luaValue{f.text([]byte(strings.Repeat("a", 32))), 4}}, {"arenaGeneration", luaValue{f.text([]byte(strings.Repeat("b", 32))), 4}}, {"inbox", luaValue{inbox, 5}}, {"sendbox", luaValue{sendbox, 5}}})
	binary.LittleEndian.PutUint64(f.bytes(f.nodes["namespace.Mailbox"], 8), box)
	return f, f.reader(), cells
}

func TestDuplexArrayCalibrationAndEphemeralProof(t *testing.T) {
	f, r, cells := duplexFixture(t)
	path := []DuplexPath{{Name: "inbox"}, {Name: "cells"}}
	a, err := r.ResolveDuplexArray(context.Background(), path, strings.Repeat("a", 32), strings.Repeat("b", 32), 3)
	if err != nil {
		t.Fatal(err)
	}
	if a.Cells[1].Value != 42 || a.Cells[2].Value != math.MaxUint32 {
		t.Fatal(a.Cells)
	}
	// Numeric payload writes do not invalidate the locating proof; changing a
	// backing array does. Neither case permits caching addresses in the next call.
	copy(f.bytes(a.Cells[1].Address, 8), numericPayload(43))
	if err := a.Verify(context.Background()); err != nil {
		t.Fatal(err)
	}
	old := binary.LittleEndian.Uint64(f.bytes(cells+32, 8))
	binary.LittleEndian.PutUint64(f.bytes(cells+32, 8), old+24)
	if err := a.Verify(context.Background()); err == nil {
		t.Fatal("relocated array accepted")
	}
}

func TestDuplexRejectsWrongGenerationAndSecretCells(t *testing.T) {
	f, r, cells := duplexFixture(t)
	path := []DuplexPath{{Name: "inbox"}, {Name: "cells"}}
	if _, err := r.ResolveDuplexArray(context.Background(), path, strings.Repeat("a", 32), strings.Repeat("c", 32), 3); err == nil {
		t.Fatal("old generation accepted")
	}
	at := binary.LittleEndian.Uint64(f.bytes(cells+32, 8))
	f.bytes(at+9, 1)[0] = 1
	if _, err := r.ResolveDuplexArray(context.Background(), path, strings.Repeat("a", 32), strings.Repeat("b", 32), 3); err == nil {
		t.Fatal("secret numeric value accepted")
	}
}

func TestDuplexReadRejectsChangedRootDuringPublication(t *testing.T) {
	f, r, _ := duplexFixture(t)
	path := []DuplexPath{{Name: "sendbox"}, {Name: "status"}}
	b, err := r.ReadDuplexString(context.Background(), path, 64)
	if err != nil || string(b) != "immutable-status" {
		t.Fatal(string(b), err)
	}
	statusAt := binary.LittleEndian.Uint64(f.bytes(f.nodes["sendbox.status"], 8)) + 32
	f.beforeRead = func(at uint64, _ int) {
		if at == statusAt {
			binary.LittleEndian.PutUint64(f.rootBytes[:], f.state+16)
		}
	}
	if _, err := r.ReadDuplexString(context.Background(), path, 64); err == nil {
		t.Fatal("changed root accepted")
	}
}
