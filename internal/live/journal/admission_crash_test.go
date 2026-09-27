package journal

import (
	"context"
	"errors"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/vault"
)

func crashAdmissionIntent() WorkIntent {
	intent := windowIntent("window/17/18/19")
	intent.RequestKey, intent.RequestDigest = "crash-admission", strings.Repeat("a", 64)
	return intent
}

// Exit without deferred closes: the parent must recover only from persisted
// state, including SQLite WAL and the separately published shared claim.
func TestAdmissionCrashHelper(t *testing.T) {
	mode := os.Getenv("LYCHEEDEV_ADMISSION_CRASH")
	if mode == "" {
		t.Skip("subprocess helper")
	}
	ctx := context.Background()
	store, err := vault.OpenStore(os.Getenv("LYCHEEDEV_ADMISSION_HOME"))
	if err != nil {
		t.Fatal(err)
	}
	metadata, err := store.OpenMetadata(ctx)
	if err != nil {
		t.Fatal(err)
	}
	book := OpenBook(metadata)
	scope, _, err := lockWindowScope(ctx, os.Getenv("LYCHEEDEV_ADMISSION_PARENT"))
	if err != nil {
		t.Fatal(err)
	}
	intent := crashAdmissionIntent()
	intent.Admission = &WindowAdmission{Parent: filepath.Dir(scope), WorkspaceID: store.Identity().WorkspaceID}
	_, err = book.beginWork(ctx, intent, func(record WorkRecord) error {
		if mode == "before-claim" {
			os.Exit(91)
		}
		if _, err := book.ensureWindowClaim(ctx, scope, store.Identity().WorkspaceID, record); err != nil {
			return err
		}
		os.Exit(91)
		return nil
	})
	t.Fatalf("crash point not reached: %v", err)
}

func TestWindowAdmissionCrashRecovery(t *testing.T) {
	for _, mode := range []string{"before-claim", "after-claim"} {
		t.Run(mode, func(t *testing.T) {
			ctx := context.Background()
			book, store := windowBook(t)
			parent := t.TempDir()
			cmd := exec.Command(os.Args[0], "-test.run=^TestAdmissionCrashHelper$")
			cmd.Env = append(os.Environ(), "LYCHEEDEV_ADMISSION_CRASH="+mode,
				"LYCHEEDEV_ADMISSION_HOME="+store.Root(), "LYCHEEDEV_ADMISSION_PARENT="+parent)
			output, err := cmd.CombinedOutput()
			var exited *exec.ExitError
			if !errors.As(err, &exited) || exited.ExitCode() != 91 {
				t.Fatalf("child did not exit at crash boundary: %v %s", err, output)
			}
			record, found, err := book.LookupRequest(ctx, crashAdmissionIntent())
			if err != nil || !found || record.Stage != "prepared" {
				t.Fatalf("durable intent lost: %+v %t %v", record, found, err)
			}
			_, occupied, err := InspectWindowOwner(ctx, parent, record.Intent.Resource)
			if err != nil || occupied != (mode == "after-claim") {
				t.Fatalf("unexpected shared publication: %t %v", occupied, err)
			}
			run, err := book.AcquireWindowWork(ctx, parent, store.Identity().WorkspaceID, record.OperationID)
			if err != nil {
				t.Fatal("same-owner recovery failed", err)
			}
			if _, err := run.Check(ctx); err != nil {
				t.Fatal(err)
			}
			if err := run.Close(); err != nil {
				t.Fatal(err)
			}
			again, err := book.BeginWindowWork(ctx, parent, store.Identity().WorkspaceID, crashAdmissionIntent())
			if err != nil || again.OperationID != record.OperationID {
				t.Fatalf("retry replaced operation: %+v %v", again, err)
			}
			other, otherStore := windowBook(t)
			if _, err := other.BeginWindowWork(ctx, parent, otherStore.Identity().WorkspaceID, crashAdmissionIntent()); !errors.Is(err, ErrBusy) {
				t.Fatal("foreign admission took unresolved work", err)
			}
		})
	}
}

func TestFailedIntentCannotPublishWindowClaim(t *testing.T) {
	book, store := windowBook(t)
	parent := t.TempDir()
	if err := book.metadata.Close(); err != nil {
		t.Fatal(err)
	}
	_, err := book.BeginWindowWork(context.Background(), parent, store.Identity().WorkspaceID, windowIntent("window/21/22/23"))
	if err == nil {
		t.Fatal("closed metadata accepted intent")
	}
	_, occupied, inspectErr := InspectWindowOwner(context.Background(), parent, "window/21/22/23")
	if inspectErr != nil || occupied {
		t.Fatalf("failed intent published a claim: %t %v", occupied, inspectErr)
	}
}
