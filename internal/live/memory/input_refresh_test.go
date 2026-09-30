package memory

import (
	"context"
	"errors"
	"sync/atomic"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
)

func refreshFixture(t *testing.T, chunks int) (*fakeSource, Selector, uint64) {
	t.Helper()
	s := source((chunks + 1) << 20)
	s.regions[0].End = uint64(chunks << 20)
	h := bridge.MemoryHeader{Kind: bridge.MemoryInputState, State: 1, Runtime: [16]byte{1}, Nonce: [16]byte{1}}
	put := func(address uint64, payload string) {
		data, err := bridge.EncodeMemoryRecord(h, []byte(payload))
		if err != nil {
			t.Fatal(err)
		}
		copy(s.data[address:], data)
	}
	for i := 0; i < chunks; i++ {
		put(uint64(i<<20)+4096, "old")
	}
	fresh := uint64(chunks<<20) + 4096
	put(fresh, "fresh")
	return s, Selector{Kind: h.Kind, Runtime: h.Runtime, Nonce: h.Nonce, Accept: func(r Record) bool { return string(r.Payload) == "fresh" }}, fresh
}

func TestInputRefreshStopsQueuedScopesAfterPositiveMatch(t *testing.T) {
	s, q, fresh := refreshFixture(t, 4)
	var entered atomic.Int32
	allEntered := make(chan struct{})
	firstAcquired := make(chan struct{})
	var budget InputRefreshBudget
	q.RefreshBudget = &budget
	q.RefreshRejected = func(ctx context.Context, src Source, r Record) (uint64, error) {
		order := entered.Add(1)
		if order == 4 {
			close(allEntered)
		}
		if order != 1 {
			<-firstAcquired
		}
		bounded, err := BeginInputRefresh(ctx, src, r.Address, false)
		if err != nil {
			return 0, err
		}
		if order == 1 {
			close(firstAcquired)
			<-allEntered
			return fresh, nil
		}
		<-bounded.Done()
		return 0, bounded.Err()
	}
	ctx := context.Background()
	session := NewSession(Budget{})
	started := time.Now()
	records, coverage, err := Lookup(ctx, s, q, Options{Workers: 4, Session: session}, true)
	if err != nil || len(records) != 1 || records[0].Address != fresh || !coverage.StoppedEarly {
		t.Fatal(records, coverage, err)
	}
	if time.Since(started) >= 500*time.Millisecond {
		t.Fatal("queued refresh scopes aged the first positive match", time.Since(started))
	}
	if session.Stats().Cancelled || session.Stats().DeadlineExceeded || ctx.Err() != nil {
		t.Fatal("normal early stop cancelled the invocation", session.Stats())
	}
}

func TestInputRefreshMissContinuesOriginalTraversal(t *testing.T) {
	s, q, fresh := refreshFixture(t, 2)
	data, err := bridge.EncodeMemoryRecord(bridge.MemoryHeader{Kind: q.Kind, State: 1, Runtime: q.Runtime, Nonce: q.Nonce}, []byte("fresh"))
	if err != nil {
		t.Fatal(err)
	}
	copy(s.data[(1<<20)+8192:], data)
	var budget InputRefreshBudget
	q.RefreshBudget = &budget
	q.RefreshRejected = func(ctx context.Context, src Source, r Record) (uint64, error) {
		_, err := BeginInputRefresh(ctx, src, r.Address, false)
		return 0, err
	}
	records, _, err := Lookup(context.Background(), s, q, Options{Workers: 1}, true)
	if err != nil || len(records) != 1 || records[0].Address == fresh || records[0].Address != (1<<20)+8192 {
		t.Fatal("refresh miss discarded remaining original traversal", records, err)
	}
}

func TestInputRefreshReturnedAddressIsIndependentlyValidated(t *testing.T) {
	for _, which := range []string{"stale", "crc", "identity", "unopened"} {
		t.Run(which, func(t *testing.T) {
			s, q, fresh := refreshFixture(t, 1)
			switch which {
			case "stale":
				data, err := bridge.EncodeMemoryRecord(bridge.MemoryHeader{Kind: q.Kind, State: 1, Runtime: q.Runtime, Nonce: q.Nonce}, []byte("old"))
				if err != nil {
					t.Fatal(err)
				}
				copy(s.data[fresh:], data)
			case "crc":
				s.data[fresh+bridge.MemoryHeaderBytes] ^= 1
			case "identity":
				s.data[fresh+8] ^= 1
			}
			var budget InputRefreshBudget
			q.RefreshBudget = &budget
			q.RefreshRejected = func(ctx context.Context, src Source, r Record) (uint64, error) {
				if which != "unopened" {
					if _, err := BeginInputRefresh(ctx, src, r.Address, false); err != nil {
						return 0, err
					}
				}
				return fresh, nil
			}
			records, coverage, err := Lookup(context.Background(), s, q, Options{Workers: 1}, true)
			if err != nil || len(records) != 0 || !coverage.Complete {
				t.Fatal("untrusted address bypassed strict reread or original traversal", records, coverage, err)
			}
			r, err := ReadRecord(context.Background(), s, 4096, q)
			if !errors.Is(err, errPredicateMismatch) || r.Payload != nil || r.Address != 0 {
				t.Fatal("public ReadRecord leaked rejected payload", r, err)
			}
		})
	}
}

func TestInputRefreshBudgetSharesBytesReadsWindowsAndDeadline(t *testing.T) {
	t.Run("reads", func(t *testing.T) {
		s := source(1 << 20)
		session := NewSession(Budget{})
		var budget InputRefreshBudget
		bounded := budget.Source(session.Source(s))
		ctx, err := BeginInputRefresh(context.Background(), bounded, 4096, false)
		if err != nil {
			t.Fatal(err)
		}
		for i := 0; i < nearbyMaxReads; i++ {
			if _, err := bounded.Read(ctx, 0, make([]byte, 1)); err != nil {
				t.Fatal(err)
			}
		}
		if _, err := bounded.Read(ctx, 0, make([]byte, 1)); !errors.Is(err, ErrNearbyBudget) {
			t.Fatal(err)
		}
		EndInputRefresh(bounded)
		if session.Stats().ReadCalls != nearbyMaxReads || s.calls != nearbyMaxReads {
			t.Fatal("read cap counted no-I/O failures or bypassed physical reads", session.Stats(), s.calls)
		}
		if _, err := BeginInputRefresh(context.Background(), budget.Source(session.Source(s)), 1<<20, true); !errors.Is(err, ErrNearbyBudget) {
			t.Fatal("new source renewed read quota", err)
		}
	})
	t.Run("bytesIncludeFinalReread", func(t *testing.T) {
		s, q, fresh := refreshFixture(t, 1)
		var budget InputRefreshBudget
		session := NewSession(Budget{})
		q.RefreshBudget = &budget
		q.RefreshRejected = func(ctx context.Context, src Source, r Record) (uint64, error) {
			bounded, err := BeginInputRefresh(ctx, src, r.Address, false)
			if err != nil {
				return 0, err
			}
			_, _ = src.Read(bounded, 0, make([]byte, nearbyMaxBytes))
			return fresh, nil
		}
		records, coverage, err := Lookup(context.Background(), s, q, Options{Workers: 1, Session: session}, true)
		if err != nil || len(records) != 0 || !coverage.Complete || budget.bytes != nearbyMaxBytes || budget.reads != 1 {
			t.Fatal("final strict reread escaped shared byte cap", records, coverage, err, budget.bytes, budget.reads)
		}
	})
	t.Run("windows", func(t *testing.T) {
		s := source(1)
		var budget InputRefreshBudget
		for i := 0; i < 4; i++ {
			bounded := budget.Source(s)
			ctx, err := BeginInputRefresh(context.Background(), bounded, uint64(i+1)<<20, false)
			if err != nil {
				t.Fatal(err)
			}
			deadline, _ := ctx.Deadline()
			again, err := BeginInputRefresh(context.Background(), bounded, uint64(i+2)<<20, false)
			second, _ := again.Deadline()
			if err != nil || deadline != second {
				t.Fatal("same attempt renewed its deadline", err)
			}
			EndInputRefresh(bounded)
			if i == 0 {
				if _, err := BeginInputRefresh(context.Background(), budget.Source(s), 1<<20, false); !errors.Is(err, ErrNearbyBudget) {
					t.Fatal("duplicate window spent a new attempt", err)
				}
			}
		}
		if budget.Available() {
			t.Fatal("four windows renewed quota")
		}
		if _, err := BeginInputRefresh(context.Background(), budget.Source(s), 5<<20, true); !errors.Is(err, ErrNearbyBudget) {
			t.Fatal(err)
		}
	})
	t.Run("totalActiveTime", func(t *testing.T) {
		var budget InputRefreshBudget
		budget.elapsed = 900 * time.Millisecond
		bounded := budget.Source(source(1))
		ctx, err := BeginInputRefresh(context.Background(), bounded, 1, false)
		if err != nil {
			t.Fatal(err)
		}
		deadline, _ := ctx.Deadline()
		if time.Until(deadline) > 100*time.Millisecond {
			t.Fatal("final scope renewed aggregate time")
		}
		<-ctx.Done()
		EndInputRefresh(bounded)
		if budget.Available() {
			t.Fatal("expired aggregate time renewed")
		}
	})
}

func TestInputRefreshLocalTimeoutContinuesWithoutCancellingInvocation(t *testing.T) {
	s, q, _ := refreshFixture(t, 2)
	data, _ := bridge.EncodeMemoryRecord(bridge.MemoryHeader{Kind: q.Kind, State: 1, Runtime: q.Runtime, Nonce: q.Nonce}, []byte("fresh"))
	copy(s.data[(1<<20)+8192:], data)
	var budget InputRefreshBudget
	q.RefreshBudget = &budget
	q.RefreshRejected = func(ctx context.Context, src Source, r Record) (uint64, error) {
		bounded, err := BeginInputRefresh(ctx, src, r.Address, false)
		if err != nil {
			return 0, err
		}
		<-bounded.Done()
		return 0, bounded.Err()
	}
	session := NewSession(Budget{})
	records, _, err := Lookup(context.Background(), s, q, Options{Workers: 1, Session: session}, true)
	if err != nil || len(records) != 1 || records[0].Address != (1<<20)+8192 || session.Err() != nil || session.Stats().Cancelled || session.Stats().DeadlineExceeded {
		t.Fatal("local timeout lost original traversal or cancelled host", records, err, session.Stats())
	}
}
