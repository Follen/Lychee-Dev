package live

import (
	"context"
	"errors"
	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/buildinfo"
	"github.com/follenfang/lycheedev/internal/selection"
	"image"
	"strings"
	"time"
	"unicode/utf8"
)

type WindowBindingRequest struct {
	Installation, Snapshot, Character, Realm, Nonce string
	PID                                             uint32
	Region                                          image.Rectangle
}

func (r WindowBindingRequest) Validate() error {
	if r.Snapshot == "" || (r.Nonce != "" && (len(r.Nonce) != 32 || strings.Trim(r.Nonce, "0123456789abcdef") != "")) {
		return errors.New("live.invalid_binding_request")
	}
	for _, label := range []string{r.Character, r.Realm} {
		if !bindingLabel(label) {
			return errors.New("live.invalid_binding_identity")
		}
	}
	if r.Region == (image.Rectangle{}) {
		return nil
	}
	if r.Region.Empty() || r.Region.Min.X < 0 || r.Region.Min.Y < 0 || r.Region.Dx() > 4096 || r.Region.Dy() > 4096 || r.Region.Max.X > 16384 || r.Region.Max.Y > 16384 {
		return errors.New("live.invalid_binding_region")
	}
	return nil
}

func bindingLabel(label string) bool {
	if len(label) > 128 || !utf8.ValidString(label) {
		return false
	}
	for _, c := range label {
		if c < 32 || c == 127 {
			return false
		}
	}
	return true
}

// BindWindowSession observes an already displayed ready handshake. It never
// opts in the addon, types a command, guesses a role, or restores input authority
// from history. The native stream is closed after durable evidence is retained.
func BindWindowSession(ctx context.Context, root string, request WindowBindingRequest) (RecordedSession, error) {
	return bindWindowSession(ctx, root, request, ResolveClientWindow, OpenWindowSession)
}

func bindWindowSession(ctx context.Context, root string, request WindowBindingRequest,
	resolve func(context.Context, string, uint32) (ClientWindow, error),
	open func(context.Context, ClientWindow, image.Rectangle, bridge.SignalExpectation) (*WindowSession, error),
) (RecordedSession, error) {
	var zero RecordedSession
	if err := request.Validate(); err != nil {
		return zero, err
	}
	ctx, cancel := context.WithTimeout(ctx, 20*time.Second)
	defer cancel()
	session, err := openRequestedSession(ctx, root, request, resolve, open)
	if err != nil {
		return zero, err
	}
	defer session.Close()
	record, err := SaveWindowSession(ctx, root, request.Snapshot, session)
	if err != nil {
		return zero, err
	}
	return RecordedSession{Record: record, Target: session.Target(), Ready: session.Ready()}, nil
}

func openRequestedSession(ctx context.Context, root string, request WindowBindingRequest,
	resolve func(context.Context, string, uint32) (ClientWindow, error),
	open func(context.Context, ClientWindow, image.Rectangle, bridge.SignalExpectation) (*WindowSession, error),
) (*WindowSession, error) {
	if err := request.Validate(); err != nil {
		return nil, err
	}
	pin, err := selection.InspectSelection(ctx, root, request.Snapshot)
	if err != nil {
		return nil, err
	}
	target, err := resolve(ctx, request.Installation, request.PID)
	if err != nil {
		return nil, err
	}
	if err := sessionSnapshotMatches(pin, target); err != nil {
		return nil, err
	}
	expected := bridge.SignalExpectation{Kind: "ready", Release: buildinfo.Version, SessionNonce: request.Nonce, Character: request.Character, Realm: request.Realm, Product: target.Client.Product, Build: target.Client.FullBuild, RequireInputReady: true}
	return open(ctx, target, request.Region, expected)
}
