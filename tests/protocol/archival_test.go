package protocol_test

import (
	"bytes"
	"context"
	"os/exec"
	"path/filepath"
	"testing"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/vault"
)

func TestLuaPersistenceArchivesAndReopensExactReport(t *testing.T) {
	lua := luaRuntime(t)

	output, err := exec.Command(lua, "persistence.lua", "../../addon").CombinedOutput()
	if err != nil {
		t.Fatalf("%v\n%s", err, output)
	}
	expected := bridge.SignalExpectation{
		Kind:          "reported",
		Release:       "2.0.0-dev",
		SessionNonce:  "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
		RequestID:     "OP-persisted",
		Character:     "character",
		Realm:         "realm",
		Product:       "retail",
		Build:         "12.1.0.69875",
		AfterSequence: 3,
	}
	report, err := bridge.ReadPersistedReport(bytes.NewReader(output), []byte("return 42"), expected)
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
