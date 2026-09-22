package live

import (
	"context"
	"encoding/json"
	"errors"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
	"path/filepath"
	"strings"
	"testing"
)

func TestPrepareProbeFreezesSessionAndCodeWithoutQueueEffect(t *testing.T) {
	ctx := context.Background()
	root, client, _, record, input := probeOperationFixture(t)
	if record.Stage != "prepared" || record.Status != "pending" || input.Binding == "" || !strings.HasPrefix(input.Expected.RequestID, "REQ-") || input.Expected.RequestID == record.OperationID || input.Load.ReloadNonce == input.Expected.SessionNonce {
		t.Fatalf("incomplete prepared intent: %+v %+v", record, input)
	}
	if len(operationQueue(t, client)) != 0 {
		t.Fatal("prepare edited addon queue")
	}
	bound, err := ReadWindowSession(ctx, root, input.Binding)
	if err != nil || bound.Ready.GUID != input.Load.GUID || bound.Record.Snapshot != record.Intent.Snapshot {
		t.Fatalf("binding: %+v %v", bound, err)
	}
	input.Code[0] = '!'
	stored, err := InspectOperation(ctx, root, record.OperationID)
	if err != nil {
		t.Fatal(err)
	}
	frozen, _, err := probeDefinition(stored)
	if err != nil || string(frozen.Code) != "return {answer=42}" {
		t.Fatal("caller changed frozen code")
	}
	session, _ := operationSessionFixture(t, client, frozen)
	if _, err := session.PrepareProbe(ctx, root, record.Intent.Snapshot, frozen.Load.Account, frozen.Code); !errors.Is(err, journal.ErrBusy) {
		t.Fatalf("repeated preparation: %v", err)
	}
	if len(operationQueue(t, client)) != 0 {
		t.Fatal("busy preparation edited queue")
	}
}

func TestPrepareProbeDoesNotBypassForeignWindowOwner(t *testing.T) {
	ctx := context.Background()
	_, client, _, first, input := probeOperationFixture(t)
	store, err := vault.Initialize(ctx, filepath.Join(t.TempDir(), "other-home"))
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
	session, _ := operationSessionFixture(t, client, input)
	_, err = session.PrepareProbe(ctx, store.Root(), pin.ID, input.Load.Account, input.Code)
	var occupied *journal.WindowOccupied
	if !errors.As(err, &occupied) || !occupied.Foreign || occupied.Owner.OperationID != first.OperationID {
		t.Fatalf("cross-home bypass: %v", err)
	}
	docs, err := metadata.ListDocuments(ctx, "work/", "", 10)
	if err != nil || len(docs) != 0 {
		t.Fatal("foreign contender created work", err)
	}
	if len(operationQueue(t, client)) != 0 {
		t.Fatal("foreign contender modified queue")
	}
}

func TestSessionOperationRequiresArchivedBinding(t *testing.T) {
	ctx := context.Background()
	root, client, book, prepared, input := probeOperationFixture(t)
	session, _ := operationSessionFixture(t, client, input)
	for _, binding := range []string{"", "SESSION-missing"} {
		input.Binding = binding
		raw, _ := json.Marshal(input)
		record, err := book.BeginWork(ctx, journal.WorkIntent{Kind: "probe", Resource: "binding-test/" + binding, Snapshot: prepared.Intent.Snapshot, Session: input.Expected.SessionNonce, Request: raw})
		if err != nil {
			t.Fatal(err)
		}
		if _, err := session.observeOperation(ctx, root, record.OperationID, nil); err == nil || errors.Is(err, journal.ErrTransition) {
			t.Fatalf("missing evidence reached phase dispatch: %v", err)
		}
	}
}

func TestPrepareProbeRejectsInvalidInputs(t *testing.T) {
	root, client, _, record, input := probeOperationFixture(t)
	session, _ := operationSessionFixture(t, client, input)
	for _, test := range []struct {
		account, snapshot string
		code              []byte
	}{
		{"", record.Intent.Snapshot, input.Code},
		{"../other", record.Intent.Snapshot, input.Code},
		{"Account-A", record.Intent.Snapshot, nil},
		{"Account-A", record.Intent.Snapshot, []byte{27, 1}},
		{"Account-A", record.Intent.Snapshot, []byte(strings.Repeat("a", 256<<10+1))},
		{"Account-A", "PIN-missing", input.Code},
	} {
		if _, err := session.PrepareProbe(context.Background(), root, test.snapshot, test.account, test.code); err == nil || errors.Is(err, journal.ErrBusy) {
			t.Fatalf("validation deferred past reservation: %v", err)
		}
	}
	session.Close()
	if _, err := session.PrepareProbe(context.Background(), root, record.Intent.Snapshot, input.Load.Account, input.Code); err == nil {
		t.Fatal("closed session prepared")
	}
}
