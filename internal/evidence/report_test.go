package evidence

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"hash/adler32"
	"path/filepath"
	"testing"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/vault"
)

func TestArchivedReportAcknowledgementRequiresDurablePair(t *testing.T) {
	ctx := context.Background()
	store, err := vault.Initialize(ctx, filepath.Join(t.TempDir(), "vault"))
	if err != nil {
		t.Fatal(err)
	}
	metadata, err := store.OpenMetadata(ctx)
	if err != nil {
		t.Fatal(err)
	}
	archive := OpenArchive(store, metadata)
	body := []byte("{ \"answer\": 42 }\r\n")
	signal := bridge.Signal{Schema: "lycheedev.signal.v1", Release: "2.0.0-dev", Kind: "reported", SessionNonce: "session", RequestID: "request", Character: "paladin", Realm: "realm", Product: "retail", Build: "12.1.0.69875", Sequence: 1, ReportBytes: uint32(len(body)), ReportAdler32: fmt.Sprintf("%08x", adler32.Checksum(body))}
	receipt, err := json.MarshalIndent(signal, "", "  ")
	if err != nil {
		t.Fatal(err)
	}
	expected := bridge.SignalExpectation{Kind: signal.Kind, Release: signal.Release, SessionNonce: signal.SessionNonce, RequestID: signal.RequestID, Character: signal.Character, Realm: signal.Realm, Product: signal.Product, Build: signal.Build}
	pair, err := archive.CommitReport(ctx, receipt, body, nil, expected, "OP-test", "PIN-test")
	if err != nil {
		t.Fatal(err)
	}
	if err := metadata.Close(); err != nil {
		t.Fatal(err)
	}
	metadata, err = store.OpenMetadata(ctx)
	if err != nil {
		t.Fatal(err)
	}
	defer metadata.Close()
	archive = OpenArchive(store, metadata)
	verified, err := archive.ReadVerifiedReport(ctx, pair.Body.ID, pair.Receipt.ID, nil, expected, "OP-test", "PIN-test")
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(verified.Body, body) || !bytes.Equal(verified.ReceiptBytes, receipt) {
		t.Fatal("original bytes changed")
	}
	for _, mode := range []string{"missing-receipt", "swapped", "operation", "snapshot", "session", "code", "cancelled"} {
		t.Run(mode, func(t *testing.T) {
			bodyID, receiptID, operation, snapshot := pair.Body.ID, pair.Receipt.ID, "OP-test", "PIN-test"
			check := expected
			var code []byte
			callCtx := ctx
			switch mode {
			case "missing-receipt":
				receiptID = "CAP-missing"
			case "swapped":
				bodyID, receiptID = receiptID, bodyID
			case "operation":
				operation = "OP-other"
			case "snapshot":
				snapshot = "PIN-other"
			case "session":
				check.SessionNonce = "other"
			case "code":
				code = []byte("return 99")
			case "cancelled":
				var cancel context.CancelFunc
				callCtx, cancel = context.WithCancel(ctx)
				cancel()
			}
			got, err := archive.ReadVerifiedReport(callCtx, bodyID, receiptID, code, check, operation, snapshot)
			if err == nil || len(got.ReceiptBytes) != 0 || len(got.Body) != 0 {
				t.Fatalf("ACK material returned: %+v %v", got, err)
			}
		})
	}
	bad, err := archive.CommitReport(ctx, receipt, []byte("{}"), nil, expected, "OP-test", "PIN-test")
	if err == nil || bad.Body.ID != "" || bad.Receipt.ID != "" {
		t.Fatal("invalid report archived")
	}
}
