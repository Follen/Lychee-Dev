//go:build windows && amd64

package channel

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/live/memory"
)

func relocationRecord(t *testing.T, address uint64, o InputObservation, sequence uint32) (memory.Record, []byte) {
	t.Helper()
	token, err := tokenBytes(o.Runtime)
	if err != nil {
		t.Fatal(err)
	}
	payload, err := json.Marshal(o)
	if err != nil {
		t.Fatal(err)
	}
	raw, err := bridge.EncodeMemoryRecord(bridge.MemoryHeader{Kind: bridge.MemoryInputState, State: 1, Runtime: token, Nonce: token, Sequence: sequence}, payload)
	if err != nil {
		t.Fatal(err)
	}
	header, payload, err := bridge.DecodeMemoryRecord(raw)
	if err != nil {
		t.Fatal(err)
	}
	return memory.Record{Address: address, Header: header, Payload: payload}, raw
}

// Reachability is supplied by each current Mailbox read. Old immutable strings
// can remain in any heap range without making address stability a requirement.
func TestMailboxReplacementFollowsPublicationAcrossAddressChanges(t *testing.T) {
	old, fixture := replacementFixture()
	initial, latest := fixture.Witness.Observation, fixture.Witness.Observation
	initial.SampleMillis, latest.SampleMillis = 10000, 11100
	before, beforeWire := relocationRecord(t, 128, initial, 1)
	const far = uint64(255 << 20)
	after, afterWire := relocationRecord(t, far+128, latest, 2)
	retainedHistory := append([]byte(nil), beforeWire...)
	now, reads, pauses := int64(10000), 0, 0
	proof, err := observeMailboxReplacement(context.Background(), old, 1, 2,
		func(context.Context) (Identity, memory.Record, error) {
			reads++
			if reads == 1 {
				return fixture.Current, before, nil
			}
			return fixture.Current, after, nil
		},
		func(context.Context) error { pauses++; now += 1100; return nil }, func() int64 { return now })
	if err != nil || proof == nil || proof.Validate(old) != nil || proof.Witness.Address != after.Address || reads != 2 || pauses != 1 {
		t.Fatal(proof, err, reads, pauses)
	}
	a, z := sha256.Sum256(retainedHistory), sha256.Sum256(afterWire)
	if proof.Witness.BeforeSHA256 != hex.EncodeToString(a[:]) || proof.Witness.AfterSHA256 != hex.EncodeToString(z[:]) || proof.Witness.Length != uint32(len(afterWire)) {
		t.Fatal("proof did not bind the two current records", proof.Witness)
	}
}

func TestMailboxReplacementSharesCallerDeadlineAndCancellation(t *testing.T) {
	for _, mode := range []string{"cancel_before_read", "cancel_wait", "deadline_wait", "cancel_latest", "cancel_before_return"} {
		t.Run(mode, func(t *testing.T) {
			old, fixture := replacementFixture()
			observation := fixture.Witness.Observation
			observation.SampleMillis = 10000
			before, _ := relocationRecord(t, 128, observation, 1)
			observation.SampleMillis = 11100
			after, _ := relocationRecord(t, 2048, observation, 2)
			duration := time.Minute
			if mode == "deadline_wait" {
				duration = 10 * time.Millisecond
			}
			ctx, cancel := context.WithTimeout(context.Background(), duration)
			defer cancel()
			if mode == "cancel_before_read" {
				cancel()
			}
			deadline, _ := ctx.Deadline()
			now, reads, pauses := int64(10000), 0, 0
			assertContext := func(c context.Context) {
				if got, ok := c.Deadline(); !ok || got != deadline {
					t.Fatal("caller deadline replaced", got, deadline)
				}
			}
			proof, err := observeMailboxReplacement(ctx, old, 1, 2,
				func(c context.Context) (Identity, memory.Record, error) {
					assertContext(c)
					reads++
					if reads == 1 {
						return fixture.Current, before, c.Err()
					}
					if mode == "cancel_latest" {
						cancel()
						return Identity{}, memory.Record{}, c.Err()
					}
					if mode == "cancel_before_return" {
						cancel()
					}
					return fixture.Current, after, nil
				}, func(c context.Context) error {
					assertContext(c)
					pauses++
					if mode == "cancel_wait" {
						cancel()
					}
					if mode == "cancel_wait" || mode == "deadline_wait" {
						<-c.Done()
						return c.Err()
					}
					now += 1100
					return nil
				}, func() int64 { return now })
			want := context.Canceled
			if mode == "deadline_wait" {
				want = context.DeadlineExceeded
			}
			if proof != nil || !errors.Is(err, want) {
				t.Fatal("termination ignored", proof, err)
			}
			if mode == "cancel_wait" || mode == "deadline_wait" {
				if !errors.Is(err, ErrPending) {
					t.Fatal("wait did not stay pending", err)
				}
			}
			wantReads, wantPauses := 2, 1
			if mode == "cancel_before_read" {
				wantReads, wantPauses = 1, 0
			}
			if mode == "cancel_wait" || mode == "deadline_wait" {
				wantReads = 1
			}
			if reads != wantReads || pauses != wantPauses {
				t.Fatal(reads, pauses)
			}
		})
	}
}
