package memory

import (
	"context"
	"errors"
	"testing"

	"github.com/follenfang/lycheedev/internal/bridge"
)

func inputSeedFixture(t *testing.T, size int) (*nearbyMeasuredSource, Selector, Hint) {
	t.Helper()
	s := &nearbyMeasuredSource{countedHintsSource: &countedHintsSource{fakeSource: source(size)}}
	h := bridge.MemoryHeader{Kind: bridge.MemoryInputState, State: 1, Runtime: [16]byte{1}, Nonce: [16]byte{1}}
	data, err := bridge.EncodeMemoryRecord(h, []byte("fresh"))
	if err != nil {
		t.Fatal(err)
	}
	copy(s.data[(1<<20)+4096:], data)
	r, err := ReadRecord(context.Background(), s, (1<<20)+4096, Selector{Kind: h.Kind, Runtime: h.Runtime, Nonce: h.Nonce})
	if err != nil {
		t.Fatal(err)
	}
	s.calls = 0
	s.bytes.Store(0)
	return s, Selector{Kind: h.Kind, Runtime: h.Runtime, Nonce: h.Nonce, Accept: func(Record) bool { return true }}, Hint{Address: 1 << 20, Header: r.Header}
}

func TestInputSeedFindsFreshRecordWithoutLearningOrProcessEnumeration(t *testing.T) {
	s, q, seed := inputSeedFixture(t, 2<<20)
	q.Accept = func(r Record) bool { return string(r.Payload) == "fresh" }
	session := NewSession(Budget{})
	got, err := FindNearbyInputSeeded(context.Background(), s, q, []Hint{seed}, session)
	if err != nil || len(got.Records) != 1 || got.Path != "nearby_scan" || got.Coverage.Complete || !got.Coverage.Truncated || s.regionsCalls.Load() != 0 || got.Stats.LearningBytes != 0 || got.Stats.Learned != 0 {
		t.Fatal(got, err)
	}
	if session.Stats().ReadCalls != uint64(s.calls) || session.Stats().RequestedBytes != s.bytes.Load() {
		t.Fatal("local refresh escaped shared physical accounting", session.Stats(), s.calls, s.bytes.Load())
	}
	if got.Stats.LocalStopped || got.Stats.Cancelled || got.Stats.DeadlineExceeded {
		t.Fatal("own cleanup cancellation marked successful refresh stopped", got.Stats)
	}
}

func TestInputSeedRejectsDifferentKindRuntimeNonceAndUnknownScope(t *testing.T) {
	for _, which := range []string{"body", "unknownKind", "unknownRuntime", "differentNonce", "authorizedBody", "ticket", "missingPredicate", "bodySeed", "runtimeSeed", "nonceSeed"} {
		t.Run(which, func(t *testing.T) {
			s, q, seed := inputSeedFixture(t, 2<<20)
			switch which {
			case "body":
				q.Kind = bridge.MemoryBody
			case "unknownKind":
				q.Kind = 0
			case "unknownRuntime":
				q.Runtime = [16]byte{}
			case "differentNonce":
				q.Nonce = [16]byte{2}
			case "authorizedBody":
				q.BodyAuthorized = true
			case "ticket":
				q.Ticket = [16]byte{3}
			case "missingPredicate":
				q.Accept = nil
			case "bodySeed":
				seed.Header.Kind = bridge.MemoryBody
			case "runtimeSeed":
				seed.Header.Runtime = [16]byte{2}
			case "nonceSeed":
				seed.Header.Nonce = [16]byte{2}
			}
			got, err := FindNearbyInputSeeded(context.Background(), s, q, []Hint{seed}, NewSession(Budget{}))
			if err != nil || len(got.Records) != 0 || s.calls != 0 || s.regionsCalls.Load() != 0 || got.Coverage.Complete {
				t.Fatal("ineligible seed read memory", got, err, s.calls)
			}
		})
	}
}

func TestInputSeedKeepsExactPredicateAndPhysicalBounds(t *testing.T) {
	s, q, seed := inputSeedFixture(t, 40<<20)
	q.Accept = func(Record) bool { return false }
	seeds := make([]Hint, 100)
	for i := range seeds {
		seeds[i] = seed
		seeds[i].Address = uint64((i*2 + 1) << 20)
	}
	got, err := FindNearbyInputSeeded(context.Background(), s, q, seeds, NewSession(Budget{}))
	if !errors.Is(err, ErrNearbyBudget) || len(got.Records) != 0 || s.bytes.Load() > nearbyMaxBytes || s.calls > nearbyMaxReads || s.regionsCalls.Load() != 0 || got.Coverage.Complete || got.Stats.LearningBytes != 0 {
		t.Fatal("local refresh exceeded bounds or accepted predicate failure", got, err, s.calls, s.bytes.Load())
	}
}

func TestInputSeedCannotRenewSharedBudgetAndHonorsCancellation(t *testing.T) {
	s, q, seed := inputSeedFixture(t, 2<<20)
	session := NewSession(Budget{MaxReadCalls: 1})
	_, _ = session.Source(s).Read(context.Background(), 0, make([]byte, 1))
	got, err := FindNearbyInputSeeded(context.Background(), s, q, []Hint{seed}, session)
	if !errors.Is(err, ErrBudget) || len(got.Records) != 0 || session.Stats().ReadCalls != 1 || got.Coverage.Complete {
		t.Fatal(got, err, session.Stats())
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	before := s.calls
	if _, err = FindNearbyInputSeeded(ctx, s, q, []Hint{seed}, NewSession(Budget{})); !errors.Is(err, context.Canceled) || s.calls != before {
		t.Fatal("cancelled local refresh read memory", err)
	}
}
