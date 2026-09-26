package live

import (
	"context"
	"errors"
	"image"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/buildinfo"
)

// Each new action refreshes readiness through the same identity/opt-in flow as
// connect. A previous action may have left its request-scoped receipt displayed.
// Do not reinterpret that receipt as a generic ready session, or rediscover a
// different window. Unfinished operations use Resume, never this path.
func reconnectSession(ctx context.Context, root, id string, io *liveIO) (*WindowSession, string, error) {
	return reconnectSessionRelease(ctx, root, id, buildinfo.Version, io)
}

func reconnectSessionRelease(ctx context.Context, root, id, release string, io *liveIO) (*WindowSession, string, error) {
	bound, err := ReadWindowSession(ctx, root, id)
	if err != nil {
		return nil, "", err
	}
	chosen := Candidate{Window: bound.Target.Window, Client: bound.Target.Client,
		Character: bound.Ready.Character, Realm: bound.Ready.Realm, GUID: bound.Ready.GUID}
	connection, err := connectCandidateRelease(ctx, root, bound.Record.Snapshot, bound.Record.Region, chosen, release, io)
	if err != nil {
		return nil, "", err
	}
	current, err := ReadWindowSession(ctx, root, connection.ID)
	if err != nil {
		return nil, "", err
	}
	session, snapshot, err := observeRecordedSessionRelease(ctx, root, connection.ID, release,
		func(ctx context.Context, target ClientWindow, region image.Rectangle, expected bridge.SignalExpectation) (*WindowSession, error) {
			frames, err := io.capture(ctx, target.Window, region)
			if err != nil {
				return nil, err
			}
			return observeWindowSessionAt(ctx, target, region, expected, sessionSignalIdentity(current.Ready), frames, io.confirm)
		})
	if err != nil {
		return nil, "", err
	}
	if session.ready.RuntimeEpoch < bound.Ready.RuntimeEpoch ||
		session.ready.RuntimeEpoch == bound.Ready.RuntimeEpoch && session.ready.Sequence < bound.Ready.Sequence {
		session.Close()
		return nil, "", errors.New("live.session_identity_changed")
	}
	return session, snapshot, nil
}
