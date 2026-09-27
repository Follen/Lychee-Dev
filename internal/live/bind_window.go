package live

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"errors"
	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/buildinfo"
	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/selection"
	"image"
	"strings"
	"time"
	"unicode/utf8"
)

type WindowBindingRequest struct {
	Installation, Snapshot, Character, Realm, Nonce string
	WakeBinding                                     string
	PID                                             uint32
	Region                                          image.Rectangle
	resolveProfile                                  func(context.Context, *WindowSession) (desktop.ReceiverBindings, error)
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
	if _, err := requestedReceiverBindings(r.WakeBinding); err != nil {
		return err
	}
	return validateCaptureArea(r.Region)
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

// BindWindowSession observes an already displayed ready handshake, then uses
// one nonce-correlated receiver identity query to prove the effective chords
// before retaining the connection. It never opts in the addon or restores
// input authority from history.
func BindWindowSession(ctx context.Context, root string, request WindowBindingRequest) (RecordedSession, error) {
	request.resolveProfile = func(ctx context.Context, session *WindowSession) (desktop.ReceiverBindings, error) {
		return discoverBoundReceiverProfile(ctx, root, session, request.WakeBinding)
	}
	return bindWindowSession(ctx, root, request, ResolveClientWindow, OpenWindowSession)
}

func discoverBoundReceiverProfile(ctx context.Context, root string, session *WindowSession, wake string) (desktop.ReceiverBindings, error) {
	bindings, err := requestedReceiverBindings(wake)
	if err != nil {
		return bindings, err
	}
	var nonce [16]byte
	if _, err := rand.Read(nonce[:]); err != nil {
		return bindings, err
	}
	probeNonce := hex.EncodeToString(nonce[:])
	_, attemptID, err := sendBootstrapReceiver(withReceiverBindings(ctx, bindings), root, session.target, session.region, "/dev bridge identify "+probeNonce)
	if err != nil {
		return bindings, err
	}
	frames, err := desktop.CaptureFrames(ctx, session.target.Window, session.region)
	if err != nil {
		return bindings, err
	}
	defer frames.Close()
	reader := bridge.ObserveSignals(frames)
	wait, cancel := context.WithTimeout(ctx, 15*time.Second)
	defer cancel()
	signal, err := reader.DiscoverIdentity(wait, bridge.SignalExpectation{Kind: "identity", Release: buildinfo.Version, ProbeNonce: probeNonce, Product: session.ready.Product, Build: session.ready.Build})
	if err != nil {
		return bindings, err
	}
	if signal.GUID != session.ready.GUID || signal.Character != session.ready.Character || signal.Realm != session.ready.Realm {
		return bindings, ErrActorChanged
	}
	if err := confirmBootstrapReceiver(ctx, root, attemptID, signal); err != nil {
		return bindings, err
	}
	attempt, err := InspectBootstrapReceiver(ctx, root, attemptID)
	if err != nil {
		return bindings, err
	}
	if err := desktop.ValidateReceiverBindings(attempt.Bindings); err != nil {
		return bindings, err
	}
	return attempt.Bindings, nil
}

func bindWindowSession(ctx context.Context, root string, request WindowBindingRequest,
	resolve func(context.Context, string, uint32) (ClientWindow, error),
	open func(context.Context, ClientWindow, image.Rectangle, bridge.SignalExpectation) (*WindowSession, error),
) (RecordedSession, error) {
	var zero RecordedSession
	if err := request.Validate(); err != nil {
		return zero, err
	}
	ctx, cancel := context.WithTimeout(ctx, 90*time.Second)
	defer cancel()
	session, err := openRequestedSession(ctx, root, request, resolve, open)
	if err != nil {
		return zero, err
	}
	defer session.Close()
	if request.resolveProfile != nil {
		bindings, err := request.resolveProfile(ctx, session)
		if err != nil {
			return zero, err
		}
		session.bindings = bindings
	}
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
