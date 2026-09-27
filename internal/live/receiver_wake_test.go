package live

import (
	"context"
	"errors"
	"testing"

	"github.com/follenfang/lycheedev/internal/bridge"
)

func TestReceiverWakeSurvivesLoadingWithoutResendingBusiness(t *testing.T) {
	wakes, observations := 0, 0
	ready, err := awaitReceiverWake(context.Background(), func() error { wakes++; return nil }, func(context.Context) (bridge.Signal, error) {
		observations++
		if observations == 1 {
			return bridge.Signal{}, context.DeadlineExceeded
		}
		return bridge.Signal{Kind: "receiver_ready", RuntimeEpoch: 2}, nil
	})
	if err != nil || ready.RuntimeEpoch != 2 || wakes != 2 {
		t.Fatalf("ready=%+v wakes=%d err=%v", ready, wakes, err)
	}
}

func TestReceiverWakeBoundAndFailure(t *testing.T) {
	for _, terminal := range []error{context.DeadlineExceeded, errors.New("capture_invalid")} {
		wakes := 0
		_, err := awaitReceiverWake(context.Background(), func() error { wakes++; return nil }, func(context.Context) (bridge.Signal, error) { return bridge.Signal{}, terminal })
		want := 1
		if errors.Is(terminal, context.DeadlineExceeded) {
			want = 3
		}
		if !errors.Is(err, terminal) || wakes != want {
			t.Fatalf("wakes=%d want=%d err=%v", wakes, want, err)
		}
	}
}

func TestFixedReloadEpochScope(t *testing.T) {
	for _, tc := range []struct {
		oldScope, nextScope string
		old, next           uint64
		valid               bool
	}{
		{"", "", 230, 231, true}, {"", "", 230, 1, false},
		{"", "character-v1", 230, 1, true},
		{"character-v1", "character-v1", 3, 3, false},
		{"character-v1", "character-v1", 3, 4, true},
		{"character-v1", "", 3, 4, false}, {"", "character-v1", 230, 0, false},
	} {
		err := validateFixedReloadEpoch(fixedReloadIntent{OldEpoch: tc.old, OldReportScope: tc.oldScope}, receiverAttempt{Epoch: tc.next, ReportScope: tc.nextScope})
		if (err == nil) != tc.valid {
			t.Fatalf("%+v: %v", tc, err)
		}
	}
}
