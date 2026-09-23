package live

import (
	"context"
	"errors"
	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/buildinfo"
	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"image"
	"time"
)

// Resume uses the saved connection, including its capture region. Completed
// work only retires ownership; unfinished work revalidates fresh game evidence.
func Resume(ctx context.Context, root, id string) (Outcome, error) {
	record, err := InspectOperation(ctx, root, id)
	if err != nil {
		return Outcome{}, err
	}
	var region image.Rectangle
	if record.Stage != "cleaned" || record.Status != "completed" && record.Status != "cancelled" {
		input, _, err := probeDefinition(record)
		if err != nil {
			return finishOutcome(ctx, root, record, err)
		}
		bound, err := ReadWindowSession(ctx, root, input.Binding)
		if err != nil {
			return finishOutcome(ctx, root, record, err)
		}
		region = bound.Record.Region
	}
	record, err = resumeLiveOperation(ctx, root, id, region, func(ctx context.Context, target ClientWindow, region image.Rectangle) (sessionFrames, error) {
		return desktop.CaptureFrames(ctx, target.Window, region)
	}, ConfirmClientWindow, desktop.QueuePreparedCommand)
	return finishOutcome(ctx, root, record, err)
}

func resumeLiveOperation(ctx context.Context, root, id string, region image.Rectangle,
	capture func(context.Context, ClientWindow, image.Rectangle) (sessionFrames, error),
	confirm func(context.Context, ClientWindow) error, send preparedInput,
) (record journal.WorkRecord, err error) {
	ctx, cancel := context.WithTimeout(ctx, 2*time.Minute)
	defer cancel()
	record, err = InspectOperation(ctx, root, id)
	if err != nil {
		return record, err
	}
	if record.Stage == "cleaned" && record.Status == "completed" {
		return ReleaseCompletedProbe(ctx, root, id)
	}
	if record.Stage == "cleaned" && record.Status == "cancelled" {
		return record, nil
	}
	if record.Status != "pending" && record.Status != "running" && record.Status != "unresolved" {
		return record, journal.ErrTransition
	}
	input, _, err := probeDefinition(record)
	if err != nil {
		return record, err
	}
	binding, err := ReadWindowSession(ctx, root, input.Binding)
	if err != nil {
		return record, err
	}
	request := WindowBindingRequest{Installation: binding.Target.Client.Directory, Snapshot: record.Intent.Snapshot, PID: binding.Target.Window.ProcessID, Character: binding.Ready.Character, Realm: binding.Ready.Realm, Nonce: binding.Ready.SessionNonce, Region: region}
	if err := request.Validate(); err != nil {
		return record, err
	}
	if binding.Ready.Release != buildinfo.Version {
		return record, errors.New("live.resume_release_mismatch")
	}
	anchor, err := operationAnchor(ctx, root, record, input, binding)
	if err != nil {
		return record, err
	}
	if err := confirm(ctx, binding.Target); err != nil {
		return record, err
	}
	frames, err := capture(ctx, binding.Target, region)
	if err != nil {
		return record, err
	}
	session := &WindowSession{target: binding.Target, region: region, ready: anchor, reader: bridge.ObserveSignals(frames), frames: frames, confirm: confirm}
	defer session.Close()
	operation, err := session.OpenOperation(ctx, root, id)
	if err != nil {
		return record, err
	}
	defer func() { err = errors.Join(err, operation.Close()) }()
	result, err := operation.execute(ctx, send)
	if result.OperationID != "" {
		record = result
	}
	return record, err
}
