package live

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/buildinfo"
	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
)

func bootstrapClientDirectory(t *testing.T) string {
	t.Helper()
	directory := filepath.Join(t.TempDir(), "_retail_")
	if err := os.MkdirAll(filepath.Join(directory, "Interface", "AddOns"), 0700); err != nil {
		t.Fatal(err)
	}
	return directory
}

func TestBootstrapUnknownInputRequiresExplicitAbandon(t *testing.T) {
	ctx := context.Background()
	root := filepath.Join(t.TempDir(), "workspace")
	if _, err := vault.Initialize(ctx, root); err != nil {
		t.Fatal(err)
	}
	target := ClientWindow{Client: selection.ClientInstallation{Directory: bootstrapClientDirectory(t)}, Window: desktop.WindowIdentity{ProcessID: 42, Handle: 101, ProcessStartedAt: 123}}
	first, err := BeginBootstrapReceiver(ctx, root, target, "/dev bridge identify 11111111111111111111111111111111")
	if err != nil {
		t.Fatal(err)
	}
	if _, err := BeginBootstrapReceiver(ctx, root, target, "/dev bridge identify 22222222222222222222222222222222"); err == nil {
		t.Fatal("unknown first wake allowed a new identity input")
	} else {
		var pending *BootstrapPendingError
		if !errors.As(err, &pending) || pending.ID != first.ID {
			t.Fatalf("wrong pending attempt: %v", err)
		}
	}
	if _, err := BeginBootstrapReceiver(ctx, root, target, "/dev connect"); err == nil {
		t.Fatal("pending identify allowed connect input on same window")
	}
	if _, err := ResumeBootstrapReceiver(ctx, root, first.ID); err == nil {
		t.Fatal("read-only resume claimed unknown acceptance")
	}
	abandoned, err := AbandonBootstrapReceiver(ctx, root, first.ID)
	if err != nil || abandoned.Phase != "abandoned" {
		t.Fatalf("abandon = %+v, %v", abandoned, err)
	}
	if again, err := AbandonBootstrapReceiver(ctx, root, first.ID); err != nil || again.Phase != "abandoned" {
		t.Fatalf("repeat abandon = %+v, %v", again, err)
	}
	second, err := BeginBootstrapReceiver(ctx, root, target, "/dev bridge identify 22222222222222222222222222222222")
	if err != nil || second.ID == first.ID {
		t.Fatalf("new deliberate attempt = %+v, %v", second, err)
	}
	retained, err := InspectBootstrapReceiver(ctx, root, first.ID)
	if err != nil || retained.Phase != "abandoned" {
		t.Fatalf("lost first evidence: %+v, %v", retained, err)
	}
	if _, err := BeginBootstrapReceiver(ctx, root, target, "/dev bridge reset 33333333333333333333333333333333"); err == nil {
		t.Fatal("pending second identify allowed reset")
	}
}

func TestBootstrapConnectRejectsPriorReadyCardAfterCommitUnknown(t *testing.T) {
	ctx := context.Background()
	root := filepath.Join(t.TempDir(), "workspace")
	if _, err := vault.Initialize(ctx, root); err != nil {
		t.Fatal(err)
	}
	target := ClientWindow{Client: selection.ClientInstallation{Directory: bootstrapClientDirectory(t), Product: "retail", FullBuild: "12.1.0.69875"}, Window: desktop.WindowIdentity{ProcessID: 43, Handle: 102, ProcessStartedAt: 124}}
	attempt, err := BeginBootstrapReceiver(ctx, root, target, "/dev connect")
	if err != nil {
		t.Fatal(err)
	}
	old := strings.Repeat("a", 32)
	if err := updateBootstrapReceiver(ctx, root, attempt.ID, func(a *BootstrapReceiverAttempt) error {
		a.Phase = "commit_requested"
		a.Receiver = receiverAttempt{Phase: "commit_requested", Epoch: 9}
		a.ActorGUID, a.ActorCharacter, a.ActorRealm = "Player-1-123", "Paladin", "Realm"
		a.PriorSessionNonce = old
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	ready := bridge.Signal{Kind: "ready", Release: buildinfo.Version, Product: "retail", Build: "12.1.0.69875", GUID: "Player-1-123", Character: "Paladin", Realm: "Realm", SessionNonce: old, RuntimeEpoch: 10, InputReady: true}
	if err := confirmBootstrapReceiver(ctx, root, attempt.ID, ready); err == nil {
		t.Fatal("old ready card confirmed new connect")
	}
	if err := updateBootstrapReceiver(ctx, root, attempt.ID, func(a *BootstrapReceiverAttempt) error {
		a.PriorReadyNonce, a.PriorReadyEpoch, a.PriorReadySequence = old, 10, 2
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	ready.Sequence = 2
	if err := confirmBootstrapReceiver(ctx, root, attempt.ID, ready); err == nil {
		t.Fatal("same session's stale ready card confirmed reconnect")
	}
	ready.Sequence = 3
	if err := confirmBootstrapReceiver(ctx, root, attempt.ID, ready); err != nil {
		t.Fatalf("fresh ready for idempotent reconnect: %v", err)
	}
}

func TestBootstrapFirstContactRequiresFreshReadyForAlreadyActiveSession(t *testing.T) {
	ctx := context.Background()
	root := filepath.Join(t.TempDir(), "workspace")
	if _, err := vault.Initialize(ctx, root); err != nil {
		t.Fatal(err)
	}
	target := ClientWindow{Client: selection.ClientInstallation{Directory: bootstrapClientDirectory(t), Product: "retail", FullBuild: "12.1.0.69933"},
		Window: desktop.WindowIdentity{ProcessID: 43, Handle: 102, ProcessStartedAt: 124}}
	const nonce = "138ac781ecd0730f0000000000000001"
	attempt, err := BeginBootstrapReceiver(ctx, root, target, "/dev connect")
	if err != nil {
		t.Fatal(err)
	}
	if err := updateBootstrapReceiver(ctx, root, attempt.ID, func(a *BootstrapReceiverAttempt) error {
		a.Phase = "commit_requested"
		a.Receiver = receiverAttempt{Phase: "commit_requested", Epoch: 230}
		a.PriorSessionNonce = nonce
		a.ActorGUID, a.ActorCharacter, a.ActorRealm = "Player-707-0708329D", "晴昼秋岚", "白银之手"
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	ready := bridge.Signal{Kind: "ready", Release: buildinfo.Version, Product: "retail", Build: "12.1.0.69933", GUID: "Player-707-0708329D",
		Character: "晴昼秋岚", Realm: "白银之手", SessionNonce: nonce, RuntimeEpoch: 230, Sequence: 4, InputReady: true}
	wrongRuntime := ready
	wrongRuntime.RuntimeEpoch++
	if err := confirmBootstrapReceiver(ctx, root, attempt.ID, wrongRuntime); err == nil {
		t.Fatal("different runtime's old ready card confirmed first-contact connect")
	}
	if err := confirmBootstrapReceiver(ctx, root, attempt.ID, ready); err == nil {
		t.Fatal("same runtime's old ready card confirmed without a pre-commit sequence fence")
	}
	if err := updateBootstrapReceiver(ctx, root, attempt.ID, func(a *BootstrapReceiverAttempt) error {
		a.PriorReadyNonce, a.PriorReadyEpoch, a.PriorReadySequence = nonce, 230, 3
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	if err := confirmBootstrapReceiver(ctx, root, attempt.ID, ready); err != nil {
		t.Fatalf("same runtime's active session was rejected: %v", err)
	}
}

func TestConnectRefusesActiveReceiverWithoutPriorSessionCapability(t *testing.T) {
	ready := bridge.Signal{Kind: "receiver_ready", ReceiverProtocol: "intent-v2", SessionNonce: strings.Repeat("a", 32), RuntimeEpoch: 230}
	if err := validateReceiverReadyForAction("connect", ready); err == nil || err.Error() != "live.receiver_prior_session_capability_missing" {
		t.Fatalf("old receiver accepted before stage: %v", err)
	}
	ready.PriorSessionEpoch = 230
	if err := validateReceiverReadyForAction("connect", ready); err != nil {
		t.Fatalf("current receiver rejected: %v", err)
	}
	ready.SessionNonce, ready.PriorSessionEpoch = "", 0
	if err := validateReceiverReadyForAction("connect", ready); err != nil {
		t.Fatalf("new-session connect rejected: %v", err)
	}
}

func TestLegacyBootstrapReconnectRecoversPriorReadyFromSavedSession(t *testing.T) {
	client, fake, root := connectFixture(t)
	if err := os.MkdirAll(filepath.Join(client, "Interface", "AddOns"), 0700); err != nil {
		t.Fatal(err)
	}
	fake.windows = []desktop.WindowIdentity{testWindow(1, client)}
	fake.respond[1] = func(command string) []*desktop.CapturedFrame {
		if command == "/dev connect" {
			return []*desktop.CapturedFrame{fake.frame(1, readyReceipt(func(signal *bridge.Signal) { signal.InputReady, signal.Sequence, signal.RuntimeEpoch = true, 2, 230 }))}
		}
		return identityResponder(fake, 1, nil)(command)
	}
	connection, err := connectWindow(context.Background(), root, ConnectRequest{Snapshot: testPin(t, root)}, fake.io())
	if err != nil {
		t.Fatal(err)
	}
	bound, err := ReadWindowSession(context.Background(), root, connection.ID)
	if err != nil {
		t.Fatal(err)
	}
	// A workspace can retain many pre-receiver session records. They are not
	// candidates for this exact reconnect and must not block its recovery.
	if _, err := vault.WriteMetadata(context.Background(), root, func(_ *vault.Store, metadata *vault.Metadata) (struct{}, error) {
		return struct{}{}, metadata.CommitDocuments(context.Background(), vault.Mutation{Key: "session/SESSION-0000-legacy", Value: []byte(`{"schema":"lycheedev.session.v1","id":"SESSION-0000-legacy"}`)})
	}); err != nil {
		t.Fatal(err)
	}
	attempt, err := BeginBootstrapReceiver(context.Background(), root, connection.Target, "/dev connect")
	if err != nil {
		t.Fatal(err)
	}
	if err := updateBootstrapReceiver(context.Background(), root, attempt.ID, func(a *BootstrapReceiverAttempt) error {
		a.Phase = "commit_requested"
		a.Receiver = receiverAttempt{Phase: "commit_requested", Epoch: bound.Ready.RuntimeEpoch}
		a.PriorSessionNonce = bound.Ready.SessionNonce
		a.ActorGUID, a.ActorCharacter, a.ActorRealm = bound.Ready.GUID, bound.Ready.Character, bound.Ready.Realm
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	attempt, err = InspectBootstrapReceiver(context.Background(), root, attempt.ID)
	if err != nil {
		t.Fatal(err)
	}
	prior, err := recoverBootstrapPriorReady(context.Background(), root, attempt)
	if err != nil || prior.SessionNonce != bound.Ready.SessionNonce || prior.RuntimeEpoch != bound.Ready.RuntimeEpoch || prior.Sequence != bound.Ready.Sequence {
		t.Fatalf("recovered prior ready = %+v, %v", prior, err)
	}
}

func TestBootstrapIdentifyConfirmsExactOpticalIdentityAfterCommit(t *testing.T) {
	ctx := context.Background()
	root := filepath.Join(t.TempDir(), "workspace")
	if _, err := vault.Initialize(ctx, root); err != nil {
		t.Fatal(err)
	}
	target := ClientWindow{Client: selection.ClientInstallation{
		Directory: bootstrapClientDirectory(t), Product: "retail", FullBuild: "12.1.0.69933",
	}, Window: desktop.WindowIdentity{ProcessID: 61784, Handle: 616501952, ProcessStartedAt: 134349776305986076}}
	const nonce = "a9cfce9db1144e177501e868d7616c90"
	attempt, err := BeginBootstrapReceiver(ctx, root, target, "/dev bridge identify "+nonce)
	if err != nil {
		t.Fatal(err)
	}
	if err := updateBootstrapReceiver(ctx, root, attempt.ID, func(a *BootstrapReceiverAttempt) error {
		a.Phase = "commit_requested"
		a.Receiver = receiverAttempt{Phase: "commit_requested", Epoch: 229}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	signal := bridge.Signal{Kind: "identity", Release: buildinfo.Version, Product: "retail", Build: "12.1.0.69933", ProbeNonce: nonce,
		ActorState: "ok", GUID: "Player-707-0708329D", Character: "晴昼秋岚", Realm: "白银之手", InputReady: true}
	wrong := signal
	wrong.ProbeNonce = strings.Repeat("f", 32)
	if err := confirmBootstrapReceiver(ctx, root, attempt.ID, wrong); err == nil || err.Error() != "live.bootstrap_receipt_mismatch.probe_nonce" {
		t.Fatalf("foreign identity mismatch = %v", err)
	}
	if err := confirmBootstrapReceiver(ctx, root, attempt.ID, signal); err != nil {
		t.Fatal(err)
	}
	confirmed, err := InspectBootstrapReceiver(ctx, root, attempt.ID)
	if err != nil || confirmed.Phase != "confirmed" || confirmed.Business == nil || confirmed.Business.ProbeNonce != nonce {
		t.Fatalf("exact identity did not preserve business proof for close: %+v, %v", confirmed, err)
	}
}

func TestReceiverAttemptCannotReplayAcceptedAction(t *testing.T) {
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
	stage := bridge.ReceiverStage{Action: "run", RequestID: "OP-1", Arg: "-", AttemptID: "1234567890abcdef"}
	for _, phase := range []string{"wake_requested", "stage_requested", "submit_requested", "commit_requested", "accepted", "dismissed"} {
		if err := saveReceiverAttempt(ctx, metadata, "OP-1", phase, stage, 0, bridge.Signal{}); err != nil {
			t.Fatalf("%s: %v", phase, err)
		}
	}
	if err := saveReceiverAttempt(ctx, metadata, "OP-1", "wake_requested", stage, 1, bridge.Signal{}); err == nil {
		t.Fatal("accepted run was replayable")
	}
	// A different action in the same operation remains independently eligible.
	stage.Action, stage.AttemptID = "ack", "fedcba0987654321"
	if err := saveReceiverAttempt(ctx, metadata, "OP-1", "wake_requested", stage, 0, bridge.Signal{}); err != nil {
		t.Fatal(err)
	}
}

func TestCustomWakeCanEqualDefaultClose(t *testing.T) {
	bindings, err := requestedReceiverBindings("ALT-CTRL-[")
	if err != nil {
		t.Fatal(err)
	}
	if bindings.WakeBinding != "ALT-CTRL-[" || bindings.CloseBinding == bindings.WakeBinding {
		t.Fatalf("provisional bindings: %+v", bindings)
	}
	shifted, err := requestedReceiverBindings("ALT-CTRL-SHIFT-[")
	if err != nil {
		t.Fatal(err)
	}
	if shifted.CloseBinding == "ALT-CTRL-[" {
		t.Fatalf("shifted wake shared close terminal: %+v", shifted)
	}
	ready := bridge.Signal{WakeBinding: "ALT-CTRL-[", SubmitBinding: "ALT-CTRL-F3", CloseBinding: "ALT-CTRL-F4"}
	if effective, err := effectiveReceiverBindings(bindings, ready); err != nil || effective.CloseBinding != "ALT-CTRL-F4" {
		t.Fatalf("effective profile: %+v, %v", effective, err)
	}
	ready.WakeBinding = "ALT-CTRL-F2"
	if _, err := effectiveReceiverBindings(bindings, ready); err == nil {
		t.Fatal("mismatched wake allowed stage")
	}
}

func TestPreWakeOpticalReadyRejectsRephotographedCard(t *testing.T) {
	ready := bridge.Signal{Schema: "lycheedev.signal.v1", Kind: "receiver_ready", Release: buildinfo.Version,
		Product: "retail", Build: "12.1.0.69875", Sequence: 1, RuntimeEpoch: 2,
		ReceiverNonce: strings.Repeat("a", 32), InputReady: true,
		WakeBinding: "ALT-CTRL-]", SubmitBinding: "ALT-CTRL-SHIFT-]", CloseBinding: "ALT-CTRL-["}
	frames := &lifecycleFrames{t: t, signals: []bridge.Signal{ready}}
	before, err := visibleReceiverReady(context.Background(), frames)
	if err != nil || before.ReceiverNonce != ready.ReceiverNonce {
		t.Fatalf("pre-wake sample: %+v, %v", before, err)
	}
	if err := receiverWakeFresh(before, ready); err == nil {
		t.Fatal("new screenshot of unchanged ready was treated as fresh wake")
	}
	after := ready
	after.ReceiverNonce = strings.Repeat("b", 32)
	if err := receiverWakeFresh(before, after); err != nil {
		t.Fatal(err)
	}
}

func TestProductionReceiverPreflightUsesLegalWorstCaseEpoch(t *testing.T) {
	nonce := strings.Repeat("a", 32)
	commands := []string{
		"/dev bridge identify " + nonce, "/dev connect", "/dev bridge reset " + nonce,
		"/dev bridge refresh " + nonce, "/dev bridge hide",
		"/dev bridge load REQ_one", "/dev bridge run REQ_one", "/dev bridge reload REQ_one",
		"/dev bridge verify REQ_one " + nonce, "/dev bridge prepare REQ_one " + nonce,
		"/dev bridge clean REQ_one " + nonce, "/dev bridge flush REQ_one " + nonce,
		"/dev bridge ack REQ_one 42", "/dev bridge bugs REQ_one 5", "/dev bridge bugs-ack REQ_one 42",
	}
	for _, command := range commands {
		t.Run(command, func(t *testing.T) {
			action, request, arg, err := parseBridgeCommand(command)
			if err != nil {
				t.Fatal(err)
			}
			if err := preflightReceiverAction(action, request, arg); err != nil {
				t.Fatal(err)
			}
		})
	}
	if err := preflightReceiverAction("identify", "-", "not-a-nonce"); err == nil || strings.Contains(err.Error(), "wire_capacity") {
		t.Fatalf("invalid action mislabeled capacity: %v", err)
	}
}

func TestEffectiveReceiverBindingsPersistWithSession(t *testing.T) {
	ctx := context.Background()
	root, _, _, pin, session, _ := unpreparedProbeFixture(t, 1)
	defer session.Close()
	session.bindings = desktop.ReceiverBindings{WakeBinding: "ALT-CTRL-F2", SubmitBinding: "ALT-CTRL-F3", CloseBinding: "ALT-CTRL-F4"}
	record, err := SaveWindowSession(ctx, root, pin.ID, session)
	if err != nil {
		t.Fatal(err)
	}
	bound, err := ReadWindowSession(ctx, root, record.ID)
	if err != nil || bound.Record.Bindings != session.bindings {
		t.Fatalf("retained profile: %+v, %v", bound.Record.Bindings, err)
	}
}

func TestAbandonedFixedReloadRetiresMarkerAfterInterruptedCleanup(t *testing.T) {
	ctx := context.Background()
	root, client, book, _, session, _ := unpreparedProbeFixture(t, 1)
	defer session.Close()
	store, err := vault.OpenStore(root)
	if err != nil {
		t.Fatal(err)
	}
	intent := fixedReloadIntent{Schema: "lycheedev.fixed-reload.v1", Target: session.target, Release: buildinfo.Version, IdentityNonce: "1234567890abcdef1234567890abcdef"}
	raw, _ := json.Marshal(intent)
	parent := filepath.Join(client, "Interface", "AddOns")
	record, err := book.BeginWindowWork(ctx, parent, store.Identity().WorkspaceID, journal.WorkIntent{Kind: "reload", Resource: windowResource(session.target), Request: raw, RequestKey: "fixed-test", RequestDigest: strings.Repeat("a", 64), Goal: "cleaned"})
	if err != nil {
		t.Fatal(err)
	}
	observed := fixedReloadObservation{Schema: "lycheedev.fixed-reload-observation.v1", NextStep: 5}
	metadata, err := store.OpenMetadata(ctx)
	if err != nil {
		t.Fatal(err)
	}
	defer metadata.Close()
	record, err = fixedReloadUpdate(ctx, metadata, record, "abandoning", "running", observed)
	if err != nil {
		t.Fatal(err)
	}
	record, err = fixedReloadUpdate(ctx, metadata, record, "abandoned", "abandoned", observed)
	if err != nil {
		t.Fatal(err)
	}
	if _, occupied, err := journal.InspectWindowOwner(ctx, parent, record.Intent.Resource); err != nil || !occupied {
		t.Fatalf("crash fixture lost marker: occupied=%v err=%v", occupied, err)
	}
	if _, err := Abandon(ctx, root, record.OperationID); err != nil {
		t.Fatal(err)
	}
	if _, occupied, err := journal.InspectWindowOwner(ctx, parent, record.Intent.Resource); err != nil || occupied {
		t.Fatalf("marker survived recovery: occupied=%v err=%v", occupied, err)
	}
	if again, err := Abandon(ctx, root, record.OperationID); err != nil || again.Stage != "abandoned" {
		t.Fatalf("repeated recovery = %+v, %v", again, err)
	}
}

func TestBootstrapBusinessProofCompletesWithoutInputCleanup(t *testing.T) {
	ctx := context.Background()
	root := filepath.Join(t.TempDir(), "workspace")
	if _, err := vault.Initialize(ctx, root); err != nil {
		t.Fatal(err)
	}
	target := ClientWindow{Client: selection.ClientInstallation{Directory: bootstrapClientDirectory(t), Product: "retail", FullBuild: "12.1.0.69933"}, Window: desktop.WindowIdentity{ProcessID: 42, Handle: 101, ProcessStartedAt: 123}}
	const nonce = "1234567890abcdef1234567890abcdef"
	attempt, err := BeginBootstrapReceiver(ctx, root, target, "/dev bridge identify "+nonce)
	if err != nil {
		t.Fatal(err)
	}
	stage := bridge.ReceiverStage{ReceiverNonce: strings.Repeat("a", 32), RuntimeEpoch: 2, Action: "identify", RequestID: "-", Arg: nonce, AttemptID: strings.Repeat("b", 16)}
	if err := updateBootstrapReceiver(ctx, root, attempt.ID, func(current *BootstrapReceiverAttempt) error {
		current.Phase = "commit_requested"
		current.Bindings = desktop.DefaultReceiverBindings()
		current.Receiver = receiverAttempt{Schema: "lycheedev.receiver-attempt.v1", OperationID: attempt.ID, Action: stage.Action, RequestID: stage.RequestID, Arg: stage.Arg,
			Attempt: 1, AttemptID: stage.AttemptID, Nonce: stage.ReceiverNonce, Epoch: stage.RuntimeEpoch, Phase: "commit_requested", Sequence: 3}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	business := bridge.Signal{Schema: "lycheedev.signal.v1", Kind: "identity", Release: buildinfo.Version, Product: "retail", Build: target.Client.FullBuild,
		ProbeNonce: nonce, ActorState: "ok", Character: "Paladin", Realm: "Realm", GUID: "Player-1-123", InputReady: true}
	if err := confirmBootstrapReceiver(ctx, root, attempt.ID, business); err != nil {
		t.Fatal(err)
	}
	attempt, err = InspectBootstrapReceiver(ctx, root, attempt.ID)
	if err != nil || attempt.Phase != "confirmed" || attempt.Business == nil {
		t.Fatalf("business proof did not complete: %+v %v", attempt, err)
	}
	if _, err := BeginBootstrapReceiver(ctx, root, target, "/dev bridge identify "+strings.Repeat("c", 32)); err != nil {
		t.Fatalf("verified business was gated by input UI cleanup: %v", err)
	}
}
