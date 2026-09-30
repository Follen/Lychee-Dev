package channel

import (
	"context"
	"errors"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

type currentRuntimeWaitPeer struct {
	pendingBackend
	currentChecks, discoveries int
}

func (p *currentRuntimeWaitPeer) Observe(context.Context, ObservationQuery) (Observation, error) {
	p.reads++
	return Observation{}, ErrPending
}

func (p *currentRuntimeWaitPeer) CurrentRuntime(context.Context, Identity) (bool, error) {
	p.currentChecks++
	return true, nil
}
func (p *currentRuntimeWaitPeer) RuntimeCandidate(context.Context, Identity) (*Identity, error) {
	p.discoveries++
	p.cancel()
	return nil, nil
}

// A missing result is ordinary asynchronous protocol progress. When a verified
// current-runtime input record exists, the scheduler must not begin a full
// unknown-runtime scan before giving that operation another observation turn.
func TestWaitKnownCurrentRuntimeDoesNotEnterExpensiveDiscovery(t *testing.T) {
	p := &currentRuntimeWaitPeer{}
	i := Identity{Runtime: strings.Repeat("1", 32), NextSlot: 4, Slots: 200, GUID: "g", Character: "c", Realm: "r", Build: "120100", Product: "retail", Release: "fixture", Fence: 1}
	d, err := New(filepath.Join(t.TempDir(), "connections", "wait.jsonl"), p, i)
	if err != nil {
		t.Fatal(err)
	}
	d.State.Bound = true
	d.State.Identity.Owner = d.State.Owner
	if err := d.PrepareOperation(context.Background(), "return {}", 1, "observation"); err != nil {
		t.Fatal(err)
	}
	d.State.Operation.Stage = "running"
	d.State.Operation.PreparedNonce = strings.Repeat("3", 32)
	d.State.Operation.Challenge = strings.Repeat("4", 32)
	ctx, cancel := context.WithTimeout(context.Background(), 60*time.Millisecond)
	defer cancel()
	p.cancel = cancel
	err = d.Continue(ctx)
	if !errors.Is(err, ErrPending) {
		t.Fatal(err)
	}
	if p.discoveries != 0 || p.currentChecks == 0 {
		t.Fatalf("fresh current runtime triggered costly whole-process discovery: discoveries=%d current_checks=%d", p.discoveries, p.currentChecks)
	}
	if p.sends != 0 || p.publishes != 0 || d.State.Operation.Stage != "running" || d.State.Transaction != nil {
		t.Fatal("wait replayed unknown input or changed operation")
	}
}
