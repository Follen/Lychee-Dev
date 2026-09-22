package live

import (
	"context"
	"crypto/sha256"
	"encoding/json"
	"errors"
	"fmt"
	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"hash/adler32"
	"os"
	"path/filepath"
	"testing"
)

func TestProbeOperationOwnsLeaseButBorrowsSession(t *testing.T) {
	ctx := context.Background()
	root, client, _, record, input := probeOperationFixture(t)
	session, frames := operationSessionFixture(t, client, input)
	operation, err := session.OpenOperation(ctx, root, record.OperationID)
	if err != nil {
		t.Fatal(err)
	}
	defer operation.Close()
	if other, err := session.OpenOperation(ctx, root, record.OperationID); !errors.Is(err, journal.ErrBusy) || other != nil {
		t.Fatalf("duplicate: %v", err)
	}
	cancelled, cancel := context.WithCancel(ctx)
	cancel()
	if _, err := operation.Observe(cancelled); !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
	if err := operation.Close(); err != nil {
		t.Fatal(err)
	}
	if err := operation.Close(); err != nil {
		t.Fatal(err)
	}
	if frames.closed {
		t.Fatal("operation closed borrowed stream")
	}
	if _, err := operation.Observe(ctx); err == nil {
		t.Fatal("closed operation used")
	}
	reopened, err := session.OpenOperation(ctx, root, record.OperationID)
	if err != nil {
		t.Fatal(err)
	}
	defer reopened.Close()
	session.Close()
	if _, err := reopened.Observe(ctx); err == nil {
		t.Fatal("closed session used")
	}
}

func TestFailedOperationOpenReleasesRuntimeLease(t *testing.T) {
	ctx := context.Background()
	root, client, _, record, input := probeOperationFixture(t)
	session, _ := operationSessionFixture(t, client, input)
	checks := 0
	session.confirm = func(context.Context, ClientWindow) error {
		checks++
		if checks == 2 {
			return desktop.ErrIdentityChanged
		}
		return nil
	}
	if operation, err := session.OpenOperation(ctx, root, record.OperationID); !errors.Is(err, desktop.ErrIdentityChanged) || operation != nil {
		t.Fatalf("changed window opened: %v", err)
	}
	session.confirm = func(context.Context, ClientWindow) error { return nil }
	operation, err := session.OpenOperation(ctx, root, record.OperationID)
	if err != nil {
		t.Fatal("failed open leaked lease", err)
	}
	if err := operation.Close(); err != nil {
		t.Fatal(err)
	}
}

func TestProbeOperationPreparesFilesUnderLease(t *testing.T) {
	ctx := context.Background()
	root, client, book, record, input := probeOperationFixture(t)
	session, _ := operationSessionFixture(t, client, input)
	operation, err := session.OpenOperation(ctx, root, record.OperationID)
	if err != nil {
		t.Fatal(err)
	}
	defer operation.Close()
	cancelled, cancel := context.WithCancel(ctx)
	cancel()
	if _, err := operation.PrepareFiles(cancelled); !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
	if len(operationQueue(t, client)) != 0 {
		t.Fatal("cancelled preparation changed queue")
	}
	var generation int64
	for i := 0; i < 2; i++ {
		prepared, err := operation.PrepareFiles(ctx)
		if err != nil || prepared.Stage != "load_requested" {
			t.Fatalf("prepare: %+v %v", prepared, err)
		}
		entries := operationQueue(t, client)
		if len(entries) != 1 || entries[0].RequestID != input.Expected.RequestID {
			t.Fatal("queue not merged idempotently")
		}
		if i != 0 && prepared.Generation != generation {
			t.Fatal("retry changed durable generation")
		}
		generation = prepared.Generation
	}
	before, err := book.InspectWork(ctx, record.OperationID)
	if err != nil {
		t.Fatal(err)
	}
	session.confirm = func(context.Context, ClientWindow) error { return desktop.ErrIdentityChanged }
	if _, err := operation.PrepareFiles(ctx); !errors.Is(err, desktop.ErrIdentityChanged) {
		t.Fatal(err)
	}
	operation.Close()
	if _, err := operation.PrepareFiles(ctx); err == nil {
		t.Fatal("closed operation prepared files")
	}
	after, err := book.InspectWork(ctx, record.OperationID)
	if err != nil || after.Generation != before.Generation {
		t.Fatal("rejected preparation mutated work", err)
	}
}

func TestOperationCannotLockAResourceOtherThanItsWindow(t *testing.T) {
	ctx := context.Background()
	root, client, book, record, input := probeOperationFixture(t)
	session, _ := operationSessionFixture(t, client, input)
	intent := record.Intent
	intent.Resource = "window/other"
	foreign, err := book.BeginWork(ctx, intent)
	if err != nil {
		t.Fatal(err)
	}
	if operation, err := session.OpenOperation(ctx, root, foreign.OperationID); err == nil || err.Error() != "live.operation_window_resource_mismatch" || operation != nil {
		t.Fatalf("wrong lock resource: %v", err)
	}
}

func TestProbeOperationChecksOwnershipAfterFrame(t *testing.T) {
	ctx := context.Background()
	root, client, book, record, input := probeOperationFixture(t)
	if _, err := PrepareOperationQueue(ctx, root, record.OperationID); err != nil {
		t.Fatal(err)
	}
	session, frames := operationSessionFixture(t, client, input)
	operation, err := session.OpenOperation(ctx, root, record.OperationID)
	if err != nil {
		t.Fatal(err)
	}
	defer operation.Close()
	marker := filepath.Join(client, "Interface", "AddOns", ".lycheedev-window-owners", fmt.Sprintf("%x.json", sha256.Sum256([]byte(record.Intent.Resource))))
	checks := 0
	session.confirm = func(context.Context, ClientWindow) error {
		checks++
		if checks == 4 {
			raw, err := os.ReadFile(marker)
			if err != nil {
				return err
			}
			var owner journal.WindowOwner
			if err := json.Unmarshal(raw, &owner); err != nil {
				return err
			}
			owner.OperationID = "OP-00000000000000000000000000000000"
			raw, _ = json.Marshal(owner)
			return os.WriteFile(marker, raw, 0600)
		}
		return nil
	}
	e := input.Expected
	loaded := bridge.Signal{Schema: "lycheedev.signal.v1", Kind: "loaded", Release: e.Release, SessionNonce: e.SessionNonce, RequestID: e.RequestID, ReloadNonce: input.Load.ReloadNonce, Character: e.Character, Realm: e.Realm, Product: e.Product, Build: e.Build, Sequence: 2, InputReady: true, CodeBytes: uint32(len(input.Code)), CodeAdler32: fmt.Sprintf("%08x", adler32.Checksum(input.Code))}
	frames.frame = makeAckFrame(t, loaded)
	frames.frame.SystemTicks = 2
	if capture, err := operation.Observe(ctx); !errors.Is(err, journal.ErrBusy) || capture.ID != "" {
		t.Fatalf("changed owner: %+v %v", capture, err)
	}
	current, err := book.InspectWork(ctx, record.OperationID)
	if err != nil || current.Stage != "load_requested" {
		t.Fatal("changed owner advanced operation", err)
	}
}

func TestProbePreparationRechecksOwnerAfterWindowConfirmation(t *testing.T) {
	ctx := context.Background()
	root, client, book, record, input := probeOperationFixture(t)
	session, _ := operationSessionFixture(t, client, input)
	operation, err := session.OpenOperation(ctx, root, record.OperationID)
	if err != nil {
		t.Fatal(err)
	}
	defer operation.Close()
	marker := filepath.Join(client, "Interface", "AddOns", ".lycheedev-window-owners", fmt.Sprintf("%x.json", sha256.Sum256([]byte(record.Intent.Resource))))
	session.confirm = func(context.Context, ClientWindow) error {
		raw, err := os.ReadFile(marker)
		if err != nil {
			return err
		}
		var owner journal.WindowOwner
		if err := json.Unmarshal(raw, &owner); err != nil {
			return err
		}
		owner.OperationID = "OP-00000000000000000000000000000000"
		raw, _ = json.Marshal(owner)
		return os.WriteFile(marker, raw, 0600)
	}
	if _, err := operation.PrepareFiles(ctx); !errors.Is(err, journal.ErrBusy) {
		t.Fatal("changed owner prepared queue", err)
	}
	current, err := book.InspectWork(ctx, record.OperationID)
	if err != nil || current.Generation != record.Generation || len(operationQueue(t, client)) != 0 {
		t.Fatal("changed owner caused an effect", err)
	}
}
