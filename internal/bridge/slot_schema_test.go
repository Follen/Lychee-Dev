package bridge

import (
	"strings"
	"testing"
)

func TestSlotPayloadOnlyCurrentSchema(t *testing.T) {
	e := SlotEnvelope{Schema: SlotSchema, Index: 200, StartSlot: 1, Fence: 1, Runtime: strings.Repeat("1", 32), Owner: strings.Repeat("2", 32), Nonce: strings.Repeat("3", 32), Ticket: strings.Repeat("4", 32), Action: "bind"}
	if _, err := SlotPayload(e); err != nil {
		t.Fatal(err)
	}
	for _, schema := range []string{"lycheedev.slot.v1", "lycheedev.slot.v2", "", "future"} {
		e.Schema = schema
		if _, err := SlotPayload(e); err == nil {
			t.Fatalf("old/unknown schema accepted: %q", schema)
		}
	}
}
