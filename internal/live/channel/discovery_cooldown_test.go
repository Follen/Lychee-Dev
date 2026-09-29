package channel

import (
	"context"
	"errors"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

type slowDiscoveryBackend struct {
	pendingBackend
	candidates   int
	receiptReads int
}

func (b *slowDiscoveryBackend) Observe(_ context.Context, q ObservationQuery) (Observation, error) {
	if q.Kind == "receipt" {
		b.receiptReads++
		if b.receiptReads == 3 {
			b.cancel()
		}
	}
	return Observation{}, ErrPending
}

func (b *slowDiscoveryBackend) RuntimeCandidate(ctx context.Context, _ Identity) (*Identity, error) {
	b.candidates++
	timer := time.NewTimer(1100 * time.Millisecond)
	defer timer.Stop()
	select {
	case <-ctx.Done():
		return nil, ctx.Err()
	case <-timer.C:
		return nil, nil
	}
}

func TestSlowDiscoveryLeavesProtocolProgressWindow(t *testing.T) {
	b := &slowDiscoveryBackend{}
	i := Identity{Runtime: strings.Repeat("1", 32), NextSlot: 1, Slots: 200, GUID: "g", Character: "c", Realm: "r", Build: "b", Product: "retail", Release: "2.5.1"}
	d, err := New(filepath.Join(t.TempDir(), "connection.jsonl"), b, i)
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 8*time.Second)
	defer cancel()
	b.cancel = cancel
	err = d.Continue(ctx)
	if !errors.Is(err, ErrPending) || b.receiptReads != 3 || b.candidates != 1 {
		t.Fatalf("err=%v receipts=%d discoveries=%d", err, b.receiptReads, b.candidates)
	}
	if d.State.Bound || b.sends != 1 {
		t.Fatal("unbound discovery was disabled or input replayed")
	}
}

func TestDiscoveryCooldownBounds(t *testing.T) {
	for _, tc := range []struct{ elapsed, want time.Duration }{{0, time.Second}, {500 * time.Millisecond, time.Second}, {12 * time.Second, 12 * time.Second}, {time.Minute, 30 * time.Second}} {
		if got := discoveryCooldown(tc.elapsed); got != tc.want {
			t.Fatalf("elapsed %s got %s want %s", tc.elapsed, got, tc.want)
		}
	}
}
