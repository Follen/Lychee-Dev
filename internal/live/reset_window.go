package live

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"errors"
	"fmt"
	"image"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/buildinfo"
	"github.com/follenfang/lycheedev/internal/selection"
)

// ResetRequest carries the same user constraints as a connect. Only unowned
// windows are eligible; a disk-owned window is another operation's authority.
type ResetRequest struct {
	Snapshot     string
	Character    string
	Realm        string
	PID          uint32
	Installation string
}

func (r ResetRequest) Validate() error {
	if r.Snapshot == "" {
		return errors.New("live.reset_requires_snapshot")
	}
	for _, label := range []string{r.Character, r.Realm} {
		if !bindingLabel(label) {
			return errors.New("live.invalid_binding_identity")
		}
	}
	return nil
}

// ResetOutcome reports one recovery attempt: the window whose in-game queue
// was unblocked and the fresh connection established through the normal
// bootstrap afterwards.
type ResetOutcome struct {
	Connection Connection `json:"connection"`
	Reset      bool       `json:"reset"`
}

// ResetWindow unblocks a runtime whose in-game queue blocks identity after its
// disk queue entry was retired or its receipt was lost. It sends exactly one
// fixed, nonce-correlated bootstrap reset per unowned matching window, expects
// the correlated receipt, then connects through the normal bootstrap. Windows
// owned by a disk marker are never touched.
func ResetWindow(ctx context.Context, root string, request ResetRequest) (ResetOutcome, error) {
	return resetWindow(ctx, root, request, nativeIO())
}

func resetWindow(ctx context.Context, root string, request ResetRequest, io *liveIO) (ResetOutcome, error) {
	var outcome ResetOutcome
	if err := request.Validate(); err != nil {
		return outcome, err
	}
	ctx, cancel := context.WithTimeout(ctx, 2*time.Minute)
	defer cancel()
	pin, err := selection.InspectSelection(ctx, root, request.Snapshot)
	if err != nil {
		return outcome, err
	}
	windows, err := io.list(ctx)
	if err != nil {
		return outcome, err
	}
	workspaceID := workspaceIdentity(ctx, root)
	var lastErr error
	for _, window := range windows {
		if request.PID != 0 && window.ProcessID != request.PID {
			continue
		}
		client, err := windowClient(ctx, window, io)
		if err != nil {
			lastErr = err
			continue
		}
		if request.Installation != "" && !sameInstallationPath(client.Directory, request.Installation) {
			continue
		}
		target := ClientWindow{Client: client, Window: window}
		if err := sessionSnapshotMatches(pin, target); err != nil {
			lastErr = err
			continue
		}
		_, occupied, _, err := windowOwnership(ctx, workspaceID, target, io)
		if err != nil {
			lastErr = err
			continue
		}
		if occupied {
			lastErr = fmt.Errorf("journal.window_occupied: %s", windowResource(target))
			continue
		}
		reset, err := resetCandidate(ctx, target, image.Rectangle{}, io)
		if err != nil {
			lastErr = err
			continue
		}
		connection, err := connectCandidate(ctx, root, request.Snapshot, image.Rectangle{}, Candidate{
			Window: window, Client: client,
			Character: reset.Character, Realm: reset.Realm, GUID: reset.GUID,
		}, io)
		if err != nil {
			lastErr = err
			continue
		}
		return ResetOutcome{Connection: connection, Reset: true}, nil
	}
	if lastErr != nil {
		return outcome, lastErr
	}
	return outcome, &CandidateSelectionError{}
}

// resetCandidate sends one fixed bootstrap reset and accepts only the
// nonce-correlated receipt. Input readiness is deliberately not required: the
// stuck runtime cannot display an input-ready card until the reset lands.
func resetCandidate(ctx context.Context, target ClientWindow, region image.Rectangle, io *liveIO) (bridge.Signal, error) {
	entropy := make([]byte, 16)
	if _, err := rand.Read(entropy); err != nil {
		return bridge.Signal{}, err
	}
	nonce := hex.EncodeToString(entropy)
	if _, err := io.send(ctx, target.Window, "/dev bridge reset "+nonce); err != nil {
		return bridge.Signal{}, err
	}
	frames, err := io.capture(ctx, target.Window, region)
	if err != nil {
		return bridge.Signal{}, err
	}
	defer frames.Close()
	hints := &captureHints{feed: frames}
	reader := bridge.ObserveSignals(hints)
	wait, cancel := context.WithTimeout(ctx, io.wait)
	defer cancel()
	return reader.DiscoverReset(wait, bridge.SignalExpectation{
		Kind: "reset", Release: buildinfo.Version, ProbeNonce: nonce,
		Product: target.Client.Product, Build: target.Client.FullBuild,
	})
}
