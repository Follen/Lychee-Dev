package live

import (
	"context"
	"encoding/json"
	"testing"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/vault"
)

func TestStandaloneReloadIsCorrelatedAndIdempotent(t *testing.T) {
	ctx := context.Background()
	root, _, _, pin, template, _ := unpreparedProbeFixture(t, 7)
	initial := template.ready
	template.Close()
	frames := &lifecycleFrames{t: t, signals: []bridge.Signal{initial}}
	expected := initialExpectation(initial)
	session, err := observeWindowSession(ctx, template.target, expected, frames, func(context.Context, ClientWindow) error { return nil })
	if err != nil {
		t.Fatal(err)
	}
	defer session.Close()
	record, err := session.prepareStandaloneReload(ctx, root, pin.ID, "reload-test-1")
	if err != nil {
		t.Fatal(err)
	}
	repeated, err := session.prepareStandaloneReload(ctx, root, pin.ID, "reload-test-1")
	if err != nil || repeated.OperationID != record.OperationID {
		t.Fatalf("repeated reload = %+v, %v", repeated, err)
	}
	op, err := session.openStandaloneReload(ctx, root, record.OperationID)
	if err != nil {
		t.Fatal(err)
	}
	defer op.close()
	var command string
	send := func(ctx context.Context, target desktop.WindowIdentity, prepare func(context.Context) (string, error), guard func(context.Context) error) (desktop.InputReceipt, error) {
		if err := guard(ctx); err != nil {
			return desktop.InputReceipt{}, err
		}
		command, err = prepare(ctx)
		if err != nil {
			return desktop.InputReceipt{}, err
		}
		input, err := parseStandaloneReload(record)
		if err != nil {
			return desktop.InputReceipt{}, err
		}
		ready := initial
		ready.Kind, ready.RequestID, ready.ReloadNonce = "ready", input.Expected.RequestID, input.ReloadNonce
		ready.RuntimeEpoch, ready.Sequence, ready.InputReady = initial.RuntimeEpoch+1, 1, true
		frames.signals = append(frames.signals, ready)
		if err := guard(ctx); err != nil {
			return desktop.InputReceipt{}, err
		}
		return desktop.InputReceipt{MessagesQueued: 3, SubmissionComplete: true}, nil
	}
	completed, err := op.execute(ctx, send)
	if err != nil {
		t.Fatal(err)
	}
	input, _ := parseStandaloneReload(record)
	if command != "/dev bridge refresh "+input.ReloadNonce || completed.Stage != "cleaned" || completed.Status != "completed" {
		t.Fatalf("reload command=%q completed=%+v", command, completed)
	}
	outcome, err := Status(ctx, root, completed.OperationID)
	if err != nil || !outcome.Complete || outcome.Report.State != "unavailable" {
		t.Fatalf("completed reload status = %+v, %v", outcome, err)
	}
	for _, mode := range []string{"missing-observation", "missing-capture", "other-operation", "wrong-actor", "wrong-nonce"} {
		t.Run(mode, func(t *testing.T) {
			changed := completed
			switch mode {
			case "missing-observation":
				changed.Observation = nil
			case "missing-capture":
				changed.Observation = json.RawMessage(`{"schema":"lycheedev.reload-observation.v1","readyCapture":"CAP-missing"}`)
			case "other-operation":
				changed.OperationID = "OP-other"
			case "wrong-actor", "wrong-nonce":
				intent, err := parseStandaloneReload(changed)
				if err != nil {
					t.Fatal(err)
				}
				if mode == "wrong-actor" {
					intent.GUID = "Player-1-other"
				} else {
					intent.ReloadNonce = "ffffffffffffffffffffffffffffffff"
				}
				changed.Intent.Request, err = json.Marshal(intent)
				if err != nil {
					t.Fatal(err)
				}
			}
			got, err := vault.ReadWorkspace(ctx, root, func(store *vault.Store, metadata *vault.Metadata) (Outcome, error) {
				return reloadOutcome(ctx, evidence.OpenArchive(store, metadata), changed, Outcome{})
			})
			if err == nil || got.Complete {
				t.Fatalf("invalid reload claimed complete: %+v %v", got, err)
			}
		})
	}
}

func initialExpectation(signal bridge.Signal) bridge.SignalExpectation {
	return bridge.SignalExpectation{Kind: "ready", Release: signal.Release, SessionNonce: signal.SessionNonce,
		Character: signal.Character, Realm: signal.Realm, Product: signal.Product, Build: signal.Build,
		RuntimeEpoch: signal.RuntimeEpoch, RequireInputReady: true}
}
