package channel

import (
	"context"
	"encoding/json"
	"errors"
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
	for _, schema := range []string{"lycheedev.slot.identity.v1", legacyIdentitySchema, "lycheedev.slot.v3", ""} {
		q := i
		q.Schema = schema
		if q.Validate() == nil {
			t.Fatalf("old identity without upgrade capability accepted: %q", schema)
		}
	}
	for _, slots := range []int{64, 200} {
		q := i
		q.Slots = slots
		if q.Validate() == nil {
			t.Fatalf("old %d-slot identity accepted", slots)
		}
	}
	for _, cursor := range []int{500, 501, 502} {
		q := i
		q.NextSlot = cursor
		if (q.Validate() == nil) != (cursor <= 501) {
			t.Fatalf("wrong upper cursor bound: %d", cursor)
		}
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
	for _, schema := range []string{"lycheedev.slot.v1", "lycheedev.slot.v2", "lycheedev.slot.v3", legacyIdentitySchema} {
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

func TestLegacyIdentityOnlySelectsKnownUpgradeCapabilities(t *testing.T) {
	i := Identity{Schema: legacyIdentitySchema, Runtime: strings.Repeat("1", 32), Slots: bridge.SlotCount, NextSlot: 1, GUID: "g", Character: "c", Realm: "r", Build: "b", Product: "retail", Release: "3.1.2"}
	runtime, _ := tokenBytes(i.Runtime)
	h := bridge.MemoryHeader{Kind: bridge.MemoryIdentity, State: 1, Runtime: runtime}
	for _, capability := range []string{legacyInputSchema, "lycheedev.input.hybrid.v2", "future", "", InputSchema} {
		i.InputState = capability
		payload, err := json.Marshal(i)
		if err != nil {
			t.Fatal(err)
		}
		got, err := decodeIdentityRecord(memory.Record{Header: h, Payload: payload}, i.Release)
		if (err == nil) != upgradeInputCapability(capability) {
			t.Fatalf("upgrade capability %q: %v", capability, err)
		}
		if err == nil && (got.Schema != legacyIdentitySchema || got.InputState != capability || inputActionCapabilityError(capability, "bind") == nil) {
			t.Fatal("old identity relabelled or granted business input", got)
		}
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

func identityPublicationTestRecord(t *testing.T, i Identity, h bridge.MemoryHeader) memory.Record {
	t.Helper()
	payload, err := json.Marshal(i)
	if err != nil {
		t.Fatal(err)
	}
	wire, err := bridge.EncodeMemoryRecord(h, payload)
	if err != nil {
		t.Fatal(err)
	}
	h, err = bridge.DecodeMemoryHeader(wire[:bridge.MemoryHeaderBytes])
	if err != nil {
		t.Fatal(err)
	}
	return memory.Record{Header: h, Payload: payload}
}

func publishedLegacyIdentityRecord(t *testing.T) memory.Record {
	t.Helper()
	// The shipped 9d3d3598 addon publishes this identity shape: 200 slots,
	// identity.v2 and hybrid INPUT. It is readable upgrade evidence only.
	i := Identity{Schema: legacyIdentitySchema, Runtime: strings.Repeat("1", 32), Slots: 200, NextSlot: 1, GUID: "g", Character: "c", Realm: "r", Build: "b", Product: "retail", Release: "3.1.2", InputState: "lycheedev.input.hybrid.v2"}
	inventory := 200
	i.Inventory = &inventory
	runtime, _ := tokenBytes(i.Runtime)
	return identityPublicationTestRecord(t, i, bridge.MemoryHeader{Kind: bridge.MemoryIdentity, State: 1, Runtime: runtime})
}

func TestPublishedLegacyIdentityRequiresManualActivation(t *testing.T) {
	for _, tc := range []struct {
		name         string
		edit         func(*Identity, *bridge.MemoryHeader)
		incompatible bool
	}{
		{"published-200-hybrid", func(*Identity, *bridge.MemoryHeader) {}, true},
		{"malformed-current-200", func(i *Identity, _ *bridge.MemoryHeader) { i.Schema, i.InputState = IdentitySchema, InputSchema }, false},
		{"other-capacity", func(i *Identity, _ *bridge.MemoryHeader) { i.Slots = 64; i.Inventory = nil }, false},
		{"unknown-capability", func(i *Identity, _ *bridge.MemoryHeader) { i.InputState = "future" }, false},
		{"inventory-overflow", func(i *Identity, _ *bridge.MemoryHeader) { inventory := 201; i.Inventory = &inventory }, false},
		{"cursor-overflow", func(i *Identity, _ *bridge.MemoryHeader) { i.NextSlot = 202 }, false},
		{"missing-actor", func(i *Identity, _ *bridge.MemoryHeader) { i.GUID = "" }, false},
		{"wrong-release", func(i *Identity, _ *bridge.MemoryHeader) { i.Release = "other" }, false},
		{"wrong-runtime", func(_ *Identity, h *bridge.MemoryHeader) { h.Runtime[0] ^= 1 }, false},
		{"nonzero-nonce", func(_ *Identity, h *bridge.MemoryHeader) { h.Nonce = h.Runtime }, false},
		{"wrong-kind", func(_ *Identity, h *bridge.MemoryHeader) { h.Kind = bridge.MemoryInputState }, false},
	} {
		t.Run(tc.name, func(t *testing.T) {
			r := publishedLegacyIdentityRecord(t)
			var i Identity
			if err := json.Unmarshal(r.Payload, &i); err != nil {
				t.Fatal(err)
			}
			tc.edit(&i, &r.Header)
			r = identityPublicationTestRecord(t, i, r.Header)
			got, err := decodeIdentityRecord(r, "3.1.2")
			if err == nil || errors.Is(err, memory.ErrMailboxIncompatible) != tc.incompatible {
				t.Fatalf("manual-activation classification=%t: %v", tc.incompatible, err)
			}
			if tc.incompatible && (got.Slots != 200 || got.Validate() == nil || inputActionCapabilityError(got.InputState, "reload") == nil) {
				t.Fatal("legacy discovery evidence was promoted to current identity or input authority", got)
			}
		})
	}
}
