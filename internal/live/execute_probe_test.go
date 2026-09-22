package live

import (
	"context"
	"encoding/json"
	"errors"
	"github.com/follenfang/lycheedev/internal/desktop"
	"testing"
)

func TestExecuteReconcilesSubmittedInputWithoutReplay(t *testing.T) {
	ctx := context.Background()
	root, client, _, record, input := probeOperationFixture(t)
	session, frames := operationSessionFixture(t, client, input)
	operation, err := session.OpenOperation(ctx, root, record.OperationID)
	if err != nil {
		t.Fatal(err)
	}
	defer operation.Close()
	partial := errors.New("fixture.partial_input")
	commands := []string{}
	send := func(ctx context.Context, target desktop.WindowIdentity, prepare func(context.Context) (string, error), guard func(context.Context) error) (desktop.InputReceipt, error) {
		if target != session.target.Window {
			t.Fatal("window changed")
		}
		frames.frame = makeAckFrame(t, session.ready)
		frames.frame.SystemTicks = int64(2 + len(commands)*10)
		if err := guard(ctx); err != nil {
			return desktop.InputReceipt{}, err
		}
		command, err := prepare(ctx)
		if err != nil {
			return desktop.InputReceipt{}, err
		}
		commands = append(commands, command)
		if err := guard(ctx); err != nil {
			return desktop.InputReceipt{}, err
		}
		return desktop.InputReceipt{MessagesQueued: 2}, partial
	}
	first, err := operation.execute(ctx, send)
	if !errors.Is(err, partial) || first.OperationID != record.OperationID || first.Stage != "load_requested" || first.Status != "unresolved" {
		t.Fatalf("bootstrap failure: %+v %v", first, err)
	}
	if len(commands) != 1 || commands[0] != "/dev bridge prepare "+input.Expected.RequestID+" "+input.Load.ReloadNonce {
		t.Fatal(commands)
	}
	ready := session.ready
	ready.RuntimeEpoch++
	ready.Sequence = 1
	ready.RequestID, ready.ReloadNonce = input.Expected.RequestID, input.Load.ReloadNonce
	frames.frame = makeAckFrame(t, ready)
	frames.frame.SystemTicks = 10
	second, err := operation.execute(ctx, send)
	if !errors.Is(err, partial) || second.Stage != "load_requested" || second.Status != "unresolved" {
		t.Fatalf("load failure: %+v %v", second, err)
	}
	if len(commands) != 2 || commands[1] != "/dev bridge load "+input.Expected.RequestID {
		t.Fatal("bootstrap replayed", commands)
	}
	var observed probeLoadObservation
	if err := json.Unmarshal(second.Observation, &observed); err != nil || observed.BootstrapCapture == "" || observed.LoadReadyCapture == "" || observed.LoadInput == nil || observed.BootstrapInput == nil {
		t.Fatal("missing recovery evidence", err)
	}
	frames.frame = makeAckFrame(t, ready)
	frames.frame.SystemTicks = 20
	third, err := operation.execute(ctx, send)
	if err == nil || len(commands) != 2 || third.Generation != second.Generation {
		t.Fatal("missing loaded receipt caused replay or state change", err, commands)
	}
}
