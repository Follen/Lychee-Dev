package live

import (
	"context"
	"errors"
	"image"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/vault"
)

func TestRunAccountChoicePrecedesOperationAndInput(t *testing.T) {
	for _, mode := range []string{"missing", "ambiguous"} {
		t.Run(mode, func(t *testing.T) {
			ctx := context.Background()
			root, client, _, pin, original, input := unpreparedProbeFixture(t, 9)
			bound, err := SaveWindowSession(ctx, root, pin.ID, original)
			if err != nil {
				t.Fatal(err)
			}
			candidate := original.ready
			original.Close()
			if mode == "ambiguous" {
				for _, account := range []string{"Account-A", "Account-B"} {
					if err := os.MkdirAll(filepath.Join(client, "WTF", "Account", account, candidate.Realm, candidate.Character), 0700); err != nil {
						t.Fatal(err)
					}
				}
			}
			frames := &lifecycleFrames{t: t, signals: []bridge.Signal{candidate}}
			open := func(ctx context.Context, target ClientWindow, region image.Rectangle, expected bridge.SignalExpectation) (*WindowSession, error) {
				return observeWindowSession(ctx, target, expected, frames, func(context.Context, ClientWindow) error { return nil })
			}
			sends := 0
			send := func(context.Context, desktop.WindowIdentity, func(context.Context) (string, error), func(context.Context) error) (desktop.InputReceipt, error) {
				sends++
				return desktop.InputReceipt{}, errors.New("unexpected input")
			}
			record, err := runProbe(ctx, root, RunRequest{Session: bound.ID, Code: input.Code}, open, send)
			var choice *AccountSelectionError
			if !errors.As(err, &choice) || record.OperationID != "" || sends != 0 || !frames.closed {
				t.Fatal(record, err, sends)
			}
			count, err := vault.ReadWorkspace(ctx, root, func(_ *vault.Store, metadata *vault.Metadata) (int, error) {
				docs, err := metadata.ListDocuments(ctx, "work/", "", 10)
				return len(docs), err
			})
			if err != nil || count != 0 || len(operationQueue(t, client)) != 0 {
				t.Fatal("account choice reserved or delivered work", err, count)
			}
		})
	}
}

func TestRunReconnectsSavedSessionAndRetainsUncertainSubmission(t *testing.T) {
	for _, mode := range []string{"fresh", "new-runtime", "missing-frame", "different-character", "different-guid", "older-runtime", "older-sequence", "changed-process"} {
		t.Run(mode, func(t *testing.T) {
			ctx := context.Background()
			root, _, book, pin, original, input := unpreparedProbeFixture(t, 9)
			original.region = image.Rect(10, 20, 610, 620)
			bound, err := SaveWindowSession(ctx, root, pin.ID, original)
			if err != nil {
				t.Fatal(err)
			}
			original.Close()
			candidate := original.ready
			switch mode {
			case "new-runtime":
				candidate.RuntimeEpoch++
				candidate.Sequence = 1
			case "different-character":
				candidate.Character = "Other"
			case "different-guid":
				candidate.GUID = "Player-1-999"
			case "older-runtime":
				candidate.RuntimeEpoch = 0
			case "older-sequence":
				candidate.Sequence--
			}
			frames := &lifecycleFrames{t: t, signals: []bridge.Signal{candidate, candidate}}
			if mode == "missing-frame" {
				frames.signals = nil
			}
			open := func(ctx context.Context, target ClientWindow, region image.Rectangle, expected bridge.SignalExpectation) (*WindowSession, error) {
				if target != original.target || region != original.region || !expected.RequireInputReady {
					t.Fatal("lost saved connection identity or region")
				}
				return observeWindowSession(ctx, target, expected, frames, func(context.Context, ClientWindow) error {
					if mode == "changed-process" {
						return desktop.ErrIdentityChanged
					}
					return nil
				})
			}
			sends := 0
			uncertain := errors.New("fixture.partial_submission")
			send := func(ctx context.Context, target desktop.WindowIdentity, prepare func(context.Context) (string, error), guard func(context.Context) error) (desktop.InputReceipt, error) {
				sends++
				if target != original.target.Window {
					t.Fatal("retargeted input")
				}
				if err := guard(ctx); err != nil {
					return desktop.InputReceipt{}, err
				}
				command, err := prepare(ctx)
				if err != nil {
					return desktop.InputReceipt{}, err
				}
				if !strings.HasPrefix(command, "/dev bridge prepare ") {
					t.Fatal(command)
				}
				return desktop.InputReceipt{MessagesQueued: 1}, uncertain
			}
			record, err := runProbe(ctx, root, RunRequest{Session: bound.ID, Account: input.Load.Account, Code: input.Code}, open, send)
			if !frames.closed {
				t.Fatal("capture not closed")
			}
			if mode != "fresh" && mode != "new-runtime" {
				if err == nil || record.OperationID != "" || sends != 0 {
					t.Fatal("invalid connection produced work", record, err, sends)
				}
				return
			}
			if !errors.Is(err, uncertain) || sends != 1 || record.OperationID == "" || record.Status != "unresolved" {
				t.Fatal(record, err, sends)
			}
			intent, _, err := probeDefinition(record)
			if err != nil || record.Intent.Snapshot != pin.ID || intent.Load.Account != input.Load.Account || string(intent.Code) != string(input.Code) {
				t.Fatal("changed request", err)
			}
			outcome, err := Status(ctx, root, record.OperationID)
			if err != nil || outcome.Complete || outcome.Report.State != "unavailable" || outcome.Cleanup != "pending" {
				t.Fatal(outcome, err)
			}
			current, err := book.InspectWork(ctx, record.OperationID)
			if err != nil || current.Generation != record.Generation {
				t.Fatal("status changed operation", err)
			}
		})
	}
}
