package protocol_test

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"hash/adler32"
	"path/filepath"
	"testing"

	"github.com/follenfang/lycheedev/internal/buildinfo"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/vault"
)

func TestPersistedReportArchivesAndReopensExactReport(t *testing.T) {
	code, body := []byte("return 42"), []byte(`{"result":42}`)
	expected := bridge.SignalExpectation{
		Kind:          "reported",
		Release:       buildinfo.Version,
		SessionNonce:  "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
		RequestID:     "OP-persisted",
		Character:     "character",
		Realm:         "realm",
		Product:       "retail",
		Build:         "12.1.0.69875",
		AfterSequence: 3,
	}
	receipt := bridge.Signal{Schema: "lycheedev.signal.v1", Release: expected.Release, Kind: "reported",
		SessionNonce: expected.SessionNonce, RequestID: expected.RequestID, Character: expected.Character,
		Realm: expected.Realm, Product: expected.Product, Build: expected.Build, Sequence: 4,
		CodeBytes: uint32(len(code)), CodeAdler32: fmt.Sprintf("%08x", adler32.Checksum(code)),
		ReportBytes: uint32(len(body)), ReportAdler32: fmt.Sprintf("%08x", adler32.Checksum(body))}
	receiptBytes, err := json.Marshal(receipt)
	if err != nil {
		t.Fatal(err)
	}
	saved := fmt.Sprintf(`LycheeToolkitBridgeDB={schema=1,reports={[%q]={receipt=%q,body=%q}}}`,
		expected.RequestID, receiptBytes, body)
	report, err := bridge.ReadPersistedReport(bytes.NewReader([]byte(saved)), code, expected, "character-v1")
	if err != nil {
		t.Fatal(err)
	}

	ctx := context.Background()
	store, err := vault.Initialize(ctx, filepath.Join(t.TempDir(), "vault"))
	if err != nil {
		t.Fatal(err)
	}
	metadata, err := store.OpenMetadata(ctx)
	if err != nil {
		t.Fatal(err)
	}
	archive := evidence.OpenArchive(store, metadata)
	captures, err := archive.CommitReport(ctx, report.ReceiptBytes, report.Body, []byte("return 42"), expected, "OP-persisted", "PIN-persisted")
	if err != nil {
		metadata.Close()
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
	archive = evidence.OpenArchive(store, metadata)
	verified, err := archive.ReadVerifiedReport(ctx, captures.Body.ID, captures.Receipt.ID, []byte("return 42"), expected, "OP-persisted", "PIN-persisted")
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(verified.ReceiptBytes, report.ReceiptBytes) || !bytes.Equal(verified.Body, report.Body) {
		t.Fatal("archived report bytes changed")
	}

	denied, err := archive.ReadVerifiedReport(ctx, captures.Body.ID, captures.Receipt.ID, []byte("return 42"), expected, "OP-other", "PIN-persisted")
	if err == nil || len(denied.ReceiptBytes) != 0 || len(denied.Body) != 0 {
		t.Fatalf("wrong operation accepted: %+v %v", denied, err)
	}
}
