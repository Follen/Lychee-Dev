package channel

import (
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/bridge"
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
			s := InputObservation{Schema: InputSchema, Runtime: e.Runtime, Owner: e.Owner, Fence: 1, NextSlot: tc.next, GUID: e.GUID, Build: e.Build, SampleMillis: tc.sample, InputBlocked: &blocked}
			_, err := inputObservation(inputTestRecord(t, s), e, 1000, 1250)
			if (err == nil) != tc.valid {
				t.Fatalf("valid=%v error=%v", tc.valid, err)
			}
		})
	}
}
