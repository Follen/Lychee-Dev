//go:build windows && amd64

package channel

import (
	"context"
	"errors"
	"reflect"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/live/memory"
)

func TestMailboxRuntimeReplacementRequiresAdvancingCurrentPublication(t *testing.T) {
	for _, mode := range []string{"valid", "freshness_boundary", "future_boundary", "immutable_history", "sequence_stalled", "sample_stalled", "runtime_changed", "old_runtime", "initial_expired", "latest_expired", "future", "identity_guid", "identity_owner", "identity_fence", "identity_slot", "build_changed", "product_changed", "release_changed", "no_capability", "nonce", "ticket", "kind", "sequence_zero", "sequence_wrap", "payload_limit", "missing_observation", "invalid_json"} {
		t.Run(mode, func(t *testing.T) {
			old, fixture := replacementFixture()
			current, next := fixture.Current, fixture.Current
			initial, latest := fixture.Witness.Observation, fixture.Witness.Observation
			initial.SampleMillis, latest.SampleMillis = 10000, 11100
			firstSequence, lastSequence := uint32(1), uint32(2)
			switch mode {
			case "freshness_boundary":
				initial.SampleMillis = 9500
				latest.SampleMillis = 10600
			case "future_boundary":
				latest.SampleMillis = 11200
			case "immutable_history":
				latest = initial
				lastSequence = firstSequence
			case "sequence_stalled":
				lastSequence = firstSequence
			case "sample_stalled":
				latest.SampleMillis = initial.SampleMillis
			case "runtime_changed":
				next.Runtime = strings.Repeat("2", 32)
				latest.Runtime = next.Runtime
			case "old_runtime":
				current = old
				initial.Runtime = old.Runtime
				initial.GUID = old.GUID
			case "initial_expired":
				initial.SampleMillis = 9499
			case "latest_expired":
				latest.SampleMillis = 10599
			case "future":
				latest.SampleMillis = 11201
			case "identity_guid":
				latest.GUID = "other-character"
			case "identity_owner":
				latest.Owner = strings.Repeat("3", 32)
				latest.Fence = 1
			case "identity_fence":
				next.Owner = strings.Repeat("3", 32)
				next.Fence = 2
				latest.Owner = next.Owner
				latest.Fence = 1
			case "identity_slot":
				latest.NextSlot++
			case "build_changed":
				next.Build = "other-build"
				latest.Build = next.Build
			case "product_changed":
				next.Product = "classic"
			case "release_changed":
				next.Release = "other-release"
			case "no_capability":
				next.InputState = "future-capability"
			case "sequence_zero":
				lastSequence = 0
			case "sequence_wrap":
				lastSequence = ^uint32(0)
			case "payload_limit":
				latest.Reason = strings.Repeat("x", 2200)
			case "missing_observation":
				latest.InputBlocked = nil
			}
			before, _ := relocationRecord(t, 128, initial, firstSequence)
			after, _ := relocationRecord(t, 2048, latest, lastSequence)
			switch mode {
			case "nonce":
				after.Header.Nonce = [16]byte{9}
			case "ticket":
				after.Header.Ticket = [16]byte{9}
			case "kind":
				after.Header.Kind = bridge.MemoryIdentity
			case "invalid_json":
				after.Payload = []byte("{")
			}
			now, reads, pauses := int64(10000), 0, 0
			proof, err := observeMailboxReplacement(context.Background(), old, 1, 2,
				func(context.Context) (Identity, memory.Record, error) {
					reads++
					if reads == 1 {
						return current, before, nil
					}
					if reads != 2 {
						t.Fatal("publication was polled", reads)
					}
					return next, after, nil
				}, func(context.Context) error {
					pauses++
					// Keep both samples eligible to isolate the advancement gate.
					if mode != "sample_stalled" {
						now += 1100
					}
					return nil
				}, func() int64 { return now })
			if err != nil {
				t.Fatal(err)
			}
			if mode == "valid" || mode == "freshness_boundary" || mode == "future_boundary" {
				if proof == nil || proof.Validate(old) != nil || proof.Current != next || !reflect.DeepEqual(proof.Witness.Observation, latest) || !reflect.DeepEqual(proof.Witness.First, initial) || proof.Witness.FirstSequence != firstSequence || proof.Witness.Sequence != lastSequence {
					t.Fatal(proof)
				}
			} else if proof != nil {
				t.Fatal("unsafe replacement accepted", proof)
			}
			wantReads := 2
			if mode == "old_runtime" || mode == "initial_expired" {
				wantReads = 1
			}
			if reads != wantReads || pauses != wantReads-1 {
				t.Fatal("unbounded or unnecessary publication reads", reads, pauses)
			}
		})
	}
}

func TestMailboxReplacementPropagatesPublicationGapWithoutRetry(t *testing.T) {
	for _, stage := range []string{"initial", "latest"} {
		t.Run(stage, func(t *testing.T) {
			old, fixture := replacementFixture()
			observation := fixture.Witness.Observation
			observation.SampleMillis = 10000
			record, _ := relocationRecord(t, 128, observation, 1)
			reads, pauses := 0, 0
			proof, err := observeMailboxReplacement(context.Background(), old, 1, 2,
				func(context.Context) (Identity, memory.Record, error) {
					reads++
					if stage == "initial" || reads == 2 {
						return Identity{}, memory.Record{}, ErrPending
					}
					return fixture.Current, record, nil
				}, func(context.Context) error { pauses++; return nil }, func() int64 { return 10000 })
			wantReads := 1
			if stage == "latest" {
				wantReads = 2
			}
			if proof != nil || !errors.Is(err, ErrPending) || reads != wantReads || pauses != wantReads-1 {
				t.Fatal(proof, err, reads, pauses)
			}
		})
	}
}

func TestMailboxReplacementInvalidProcessStopsBeforePublication(t *testing.T) {
	old, _ := replacementFixture()
	for _, ids := range [][2]uint64{{0, 2}, {1, 0}} {
		proof, err := observeMailboxReplacement(context.Background(), old, uint32(ids[0]), ids[1],
			func(context.Context) (Identity, memory.Record, error) {
				t.Fatal("read invalid process")
				return Identity{}, memory.Record{}, nil
			},
			func(context.Context) error { t.Fatal("wait invalid process"); return nil }, func() int64 { t.Fatal("clock invalid process"); return 0 })
		if proof != nil || err == nil {
			t.Fatal(proof, err)
		}
	}
}
