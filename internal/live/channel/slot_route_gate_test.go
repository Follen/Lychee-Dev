package channel

import (
	"encoding/json"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/live/memory"
)

func TestSkippedSlotInputRequiresExactFreshOrigin(t *testing.T) {
	blocked := false
	e := bridge.SlotEnvelope{Schema: bridge.SlotSchema, Index: 200, StartSlot: 2, Runtime: strings.Repeat("1", 32), Owner: "owner", Fence: 1, GUID: "g", Build: "b"}
	for _, tc := range []struct {
		name   string
		next   int
		sample int64
		valid  bool
	}{
		{"origin", 2, 1200, true},
		{"before-origin", 1, 1200, false},
		{"inside-route", 3, 1200, false},
		{"destination", 200, 1200, false},
		{"stale-origin", 2, 700, false},
		{"before-previous-input", 2, 1050, false},
	} {
		t.Run(tc.name, func(t *testing.T) {
			s := InputObservation{Schema: "lycheedev.input.v1", Runtime: e.Runtime, Owner: e.Owner, Fence: 1, NextSlot: tc.next, GUID: e.GUID, Build: e.Build, SampleMillis: tc.sample, InputBlocked: &blocked}
			payload, err := json.Marshal(s)
			if err != nil {
				t.Fatal(err)
			}
			_, err = inputObservation(memory.Record{Header: bridge.MemoryHeader{Kind: bridge.MemoryInputState}, Payload: payload}, e, 1000, 1250)
			if (err == nil) != tc.valid {
				t.Fatalf("valid=%v error=%v", tc.valid, err)
			}
		})
	}
}
