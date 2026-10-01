package channel

import (
	"context"
	"encoding/json"
	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/live/memory"
	"path/filepath"
	"strings"
	"testing"
)

func TestIdentityAndReceiptOnlyCurrentSchema(t *testing.T) {
	i := Identity{Schema: IdentitySchema, Runtime: strings.Repeat("1", 32), Slots: bridge.SlotCount, NextSlot: 2, GUID: "g", Character: "c", Realm: "r", Build: "b", Product: "retail", Release: "3"}
	if err := i.Validate(); err != nil {
		t.Fatal(err)
	}
	for _, schema := range []string{"lycheedev.slot.identity.v1", bridge.SlotSchema, ""} {
		q := i
		q.Schema = schema
		if q.Validate() == nil {
			t.Fatalf("old/receipt identity accepted: %q", schema)
		}
	}
	q := i
	q.Slots = 64
	if q.Validate() == nil {
		t.Fatal("old 64-slot identity accepted")
	}
	e := bridge.SlotEnvelope{Schema: bridge.SlotSchema, Index: 1, Runtime: i.Runtime, GUID: i.GUID, Build: i.Build, Nonce: strings.Repeat("2", 32), Ticket: strings.Repeat("3", 32), Action: "bind"}
	i.Schema = bridge.SlotSchema
	r := Receipt{Identity: i, Nonce: e.Nonce, Ticket: e.Ticket, Action: e.Action, State: "bound"}
	nonce, _ := tokenBytes(e.Nonce)
	runtime, _ := tokenBytes(e.Runtime)
	ticket, _ := tokenBytes(e.Ticket)
	h := bridge.MemoryHeader{Kind: bridge.MemoryReceipt, State: 1, Sequence: 1, Nonce: nonce, Runtime: runtime, Ticket: ticket}
	if _, err := decodeReceipt(publicationTestRecord(t, r, h), e); err != nil {
		t.Fatal("current receipt rejected", err)
	}
	for _, schema := range []string{"lycheedev.slot.v1", "lycheedev.slot.v2", IdentitySchema} {
		r.Schema = schema
		e.Schema = schema
		if _, err := decodeReceipt(publicationTestRecord(t, r, h), e); err == nil {
			t.Fatalf("old receipt accepted: %q", schema)
		}
	}
}
func TestJournalRejectsOldNativeProtocol(t *testing.T) {
	i := Identity{Schema: IdentitySchema, Runtime: strings.Repeat("1", 32), Slots: bridge.SlotCount, NextSlot: 1, GUID: "g", Character: "c", Realm: "r", Build: "b", Product: "retail", Release: "3"}
	d, err := New(filepath.Join(t.TempDir(), "connections", "connection.jsonl"), nil, i)
	if err != nil {
		t.Fatal(err)
	}
	for _, schema := range []string{"lycheedev.slot.identity.v1", ""} {
		d.State.Identity.Schema = schema
		if err := d.Save(context.Background(), "old"); err == nil {
			t.Fatal("old protocol journal persisted")
		}
	}
	d.State.Identity = i
	d.State.Schema = "lycheedev.channel.v1"
	if d.State.validate() == nil {
		t.Fatal("old channel journal resumed")
	}
}

func TestIdentityPublicationRejectsMalformedChecksummedHeaders(t *testing.T) {
	i := Identity{Schema: IdentitySchema, Runtime: strings.Repeat("1", 32), Slots: bridge.SlotCount, NextSlot: 1, GUID: "g", Character: "c", Realm: "r", Build: "b", Product: "retail", Release: "3.1.0"}
	payload, err := json.Marshal(i)
	if err != nil {
		t.Fatal(err)
	}
	runtime, _ := tokenBytes(i.Runtime)
	base := bridge.MemoryHeader{Kind: bridge.MemoryIdentity, State: 1, Runtime: runtime}
	for _, tc := range []struct {
		name  string
		edit  func(*bridge.MemoryHeader)
		valid bool
	}{
		{"fresh-sequence-zero", func(*bridge.MemoryHeader) {}, true},
		{"published-sequence", func(h *bridge.MemoryHeader) { h.Sequence = 12 }, true},
		{"last-sequence", func(h *bridge.MemoryHeader) { h.Sequence = ^uint32(0) }, true},
		{"wrong-kind", func(h *bridge.MemoryHeader) { h.Kind = bridge.MemoryInputState }, false},
		{"wrong-state", func(h *bridge.MemoryHeader) { h.State = 2 }, false},
		{"nonzero-nonce", func(h *bridge.MemoryHeader) { h.Nonce = runtime }, false},
		{"nonzero-ticket", func(h *bridge.MemoryHeader) { h.Ticket = runtime }, false},
		{"wrong-runtime", func(h *bridge.MemoryHeader) { h.Runtime[0] ^= 1 }, false},
	} {
		t.Run(tc.name, func(t *testing.T) {
			h := base
			tc.edit(&h)
			wire, err := bridge.EncodeMemoryRecord(h, payload)
			if err != nil {
				t.Fatal(err)
			}
			decoded, err := bridge.DecodeMemoryHeader(wire[:bridge.MemoryHeaderBytes])
			if err != nil {
				t.Fatal("test must exercise a valid wire checksum", err)
			}
			_, err = decodeIdentityRecord(memory.Record{Header: decoded, Payload: payload}, i.Release)
			if (err == nil) != tc.valid {
				t.Fatal("identity header qualification", err)
			}
		})
	}
}
