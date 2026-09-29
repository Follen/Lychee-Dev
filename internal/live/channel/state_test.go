package channel

import (
	"context"
	"encoding/json"
	"hash/adler32"
	"path/filepath"
	"strings"
	"testing"
)

func TestResultPersistenceKeepsExactWhitespaceAndHTMLBytes(t *testing.T) {
	i := Identity{Runtime: strings.Repeat("1", 32), NextSlot: 9, Slots: 200, GUID: "g", Character: "c", Realm: "r", Build: "b", Product: "retail", Release: "2.5.1"}
	d, err := New(filepath.Join(t.TempDir(), "connections", "c.jsonl"), nil, i)
	if err != nil {
		t.Fatal(err)
	}
	b := []byte("{ \"message\": \"<荔枝>&\", \"line\": \"\u2028\" }\n")
	d.State.Bound = true
	d.State.Operation = &Operation{ID: "LMO-" + strings.Repeat("2", 32), Ticket: strings.Repeat("3", 32), Code: "return true", Budget: 5, Policy: "observation", Stage: "complete", PreparedNonce: strings.Repeat("4", 32), Challenge: strings.Repeat("5", 32), ReportBytes: uint32(len(b)), ReportChecksum: adler32.Checksum(b), Result: b}
	if err = d.Save(context.Background(), "result"); err != nil {
		t.Fatal(err)
	}
	restored, err := Load(d.Log, nil)
	if err != nil || string(restored.State.Operation.Result) != string(b) {
		t.Fatalf("payload changed: %v", err)
	}
	d.State.Operation.ID = "LMO-../../outside"
	if err = d.Save(context.Background(), "invalid"); err == nil {
		t.Fatal("accepted path traversal")
	}
}

// Pre-release journals used RawMessage; accept their original bytes only when
// the normal state validator can still prove the declared digest and length.
func TestLegacyResultBytes(t *testing.T) {
	var op Operation
	if err := json.Unmarshal([]byte(`{"result":{"ok":true}}`), &op); err != nil || string(op.Result) != `{"ok":true}` {
		t.Fatalf("%s %v", op.Result, err)
	}
}
