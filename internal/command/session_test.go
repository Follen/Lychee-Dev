package command

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/json"
	"fmt"
	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/live"
	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestLiveSessionCommand(t *testing.T) {
	ctx := context.Background()
	root := filepath.Join(t.TempDir(), "workspace")
	store, err := vault.Initialize(ctx, root)
	if err != nil {
		t.Fatal(err)
	}
	metadata, err := store.OpenMetadata(ctx)
	if err != nil {
		t.Fatal(err)
	}
	defer metadata.Close()
	pin, err := selection.OpenPinner(metadata).PinSelection(ctx, selection.SelectionSpec{Source: &selection.SourcePin{Repository: "fixture", Product: "retail", ExactCommit: strings.Repeat("a", 40), ParserRevision: "1"}})
	if err != nil {
		t.Fatal(err)
	}
	target := live.ClientWindow{Client: selection.ClientInstallation{Product: "retail", FullBuild: "12.1.0.69875"}, Window: desktop.WindowIdentity{ProcessID: 2, Handle: 1, ProcessStartedAt: 3}}
	signal := bridge.Signal{Schema: "lycheedev.signal.v1", Kind: "ready", Release: "2.0.0-dev", SessionNonce: strings.Repeat("a", 32), Character: "Paladin", Realm: "Realm", GUID: "Player-1-123", Product: "retail", Build: target.Client.FullBuild, Sequence: 1, InputReady: true}
	data, _ := json.Marshal(struct {
		Schema string            `json:"schema"`
		Target live.ClientWindow `json:"target"`
		Ready  bridge.Signal     `json:"ready"`
	}{"lycheedev.window-session.v1", target, signal})
	capture, err := evidence.OpenArchive(store, metadata).CommitCapture(ctx, evidence.CaptureDraft{Reader: bytes.NewReader(data), MaxBytes: 16384, MediaType: "application/json", Complete: true, Provenance: evidence.Provenance{Kind: "decoded-window-session", Locator: "pid:2/window:1", Snapshot: pin.ID, DataBuild: signal.Build, Session: signal.SessionNonce}})
	if err != nil {
		t.Fatal(err)
	}
	record := live.SessionRecord{Schema: "lycheedev.session.v1", Snapshot: pin.ID, CaptureID: capture.ID}
	unsigned, _ := json.Marshal(record)
	record.ID = fmt.Sprintf("SESSION-%x", sha256.Sum256(unsigned))
	raw, _ := json.Marshal(record)
	if err := metadata.CommitDocuments(ctx, vault.Mutation{Key: "session/" + record.ID, Value: raw}); err != nil {
		t.Fatal(err)
	}
	result, code := invoke(t, "live", "session", record.ID, "--home", root, "--format=json")
	if code != 0 || !result.OK || result.Context["session"] != record.ID || len(result.Warnings) != 1 || result.OperationID != "" {
		t.Fatalf("%+v code=%d", result, code)
	}
	encoded, _ := json.Marshal(result.Result)
	var actual live.Connection
	if err := json.Unmarshal(encoded, &actual); err != nil {
		t.Fatal(err)
	}
	if actual.ID != record.ID || actual.CaptureID != capture.ID || actual.Snapshot != pin.ID || actual.Character != signal.Character || actual.Realm != signal.Realm || actual.Target != target {
		t.Fatal("wrong session output")
	}
	if strings.Contains(string(encoded), "sessionNonce") || strings.Contains(string(encoded), "inputReady") || strings.Contains(string(encoded), "\"ready\"") {
		t.Fatal("connection view leaked protocol or historical input authority")
	}
	for _, args := range [][]string{{"live", "session"}, {"live", "session", record.ID, "extra"}} {
		args = append(args, "--home", root, "--format=json")
		if result, code := invoke(t, args...); code != 2 || result.OK {
			t.Fatalf("arguments: %+v code=%d", result, code)
		}
	}
	if result, code := invoke(t, "live", "session", "SESSION-missing", "--home", root, "--format=json"); code == 0 || result.OK || result.Result != nil {
		t.Fatal("missing session succeeded")
	}
	missing := filepath.Join(t.TempDir(), "absent")
	if result, code := invoke(t, "live", "session", record.ID, "--home", missing, "--format=json"); code == 0 || result.OK || result.Result != nil {
		t.Fatal("missing workspace succeeded")
	}
	if _, err := os.Stat(missing); !os.IsNotExist(err) {
		t.Fatal("readonly session query created workspace")
	}
	found := false
	for _, definition := range definitions {
		if definition.Path == "live session" {
			found = true
			if definition.Mutates {
				t.Fatal("session read advertised as mutation")
			}
		}
	}
	if !found {
		t.Fatal("missing executable contract")
	}
}
