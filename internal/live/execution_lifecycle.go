package live

import (
	"context"
	"errors"
	"image"
	"strings"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/buildinfo"
	"github.com/follenfang/lycheedev/internal/live/journal"
)

type ExecutionRequest struct {
	Session string
	Account string
	Code    []byte
}

func (r ExecutionRequest) validate() error {
	if strings.TrimSpace(r.Session) == "" || len(r.Session) > 128 {
		return errors.New("live.session_required")
	}
	if r.Account != "" {
		if err := validateReportAccount(r.Account); err != nil {
			return err
		}
	}
	if len(r.Code) == 0 || len(r.Code) > 256<<10 {
		return errors.New("bridge.queue_invalid_code")
	}
	return nil
}

func executePrepared(ctx context.Context, root string, request ExecutionRequest,
	open func(context.Context, ClientWindow, image.Rectangle, bridge.SignalExpectation) (*WindowSession, error),
	send preparedInput,
) (record journal.WorkRecord, err error) {
	if err = request.validate(); err != nil {
		return record, err
	}
	ctx, cancel := context.WithTimeout(ctx, 2*time.Minute)
	defer cancel()
	session, snapshot, err := observeRecordedSession(ctx, root, request.Session, open)
	if err != nil {
		return record, err
	}
	defer session.Close()
	record, err = session.PrepareProbe(ctx, root, snapshot, request.Account, request.Code)
	if err != nil {
		return record, err
	}
	operation, err := session.OpenOperation(ctx, root, record.OperationID)
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

// A saved connection identifies the target; it does not authorize input.
// OpenWindowSession verifies the original process identity and observes a new
// ready frame. Request identity is never accepted from a command-line override.
func observeRecordedSession(ctx context.Context, root, id string,
	open func(context.Context, ClientWindow, image.Rectangle, bridge.SignalExpectation) (*WindowSession, error),
) (*WindowSession, string, error) {
	return observeRecordedSessionRelease(ctx, root, id, buildinfo.Version, open)
}

func observeRecordedSessionRelease(ctx context.Context, root, id, release string,
	open func(context.Context, ClientWindow, image.Rectangle, bridge.SignalExpectation) (*WindowSession, error),
) (*WindowSession, string, error) {
	bound, err := ReadWindowSession(ctx, root, id)
	if err != nil {
		return nil, "", err
	}
	if bound.Ready.Release != release {
		return nil, "", errors.New("live.session_release_mismatch")
	}
	request := WindowBindingRequest{Installation: bound.Target.Client.Directory, Snapshot: bound.Record.Snapshot, PID: bound.Target.Window.ProcessID, Character: bound.Ready.Character, Realm: bound.Ready.Realm, Nonce: bound.Ready.SessionNonce, Region: bound.Record.Region}
	if err := request.Validate(); err != nil {
		return nil, "", err
	}
	expected := bridge.SignalExpectation{Kind: "ready", Release: release, SessionNonce: bound.Ready.SessionNonce, Character: bound.Ready.Character, Realm: bound.Ready.Realm, Product: bound.Ready.Product, Build: bound.Ready.Build, RequireInputReady: true}
	session, err := open(ctx, bound.Target, bound.Record.Region, expected)
	if err != nil {
		return nil, "", err
	}
	if session.ready.GUID != bound.Ready.GUID || session.ready.RuntimeEpoch < bound.Ready.RuntimeEpoch || (session.ready.RuntimeEpoch == bound.Ready.RuntimeEpoch && session.ready.Sequence < bound.Ready.Sequence) {
		session.Close()
		return nil, "", errors.New("live.session_identity_changed")
	}
	if bound.Record.Region != (image.Rectangle{}) {
		session.region = bound.Record.Region
	}
	return session, bound.Record.Snapshot, nil
}
