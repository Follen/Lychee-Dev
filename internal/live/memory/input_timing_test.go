package memory

import (
	"context"
	"encoding/json"
	"errors"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
)

type timingVerifySource struct {
	*fakeSource
	verifies int
	finalErr error
}

func TestInputTimingAbsentForMissAuditAndOtherKinds(t *testing.T) {
	for _, which := range []string{"miss", "audit", "otherKind"} {
		t.Run(which, func(t *testing.T) {
			s, q, _ := refreshFixture(t, 1)
			s.regions[0].End = uint64(len(s.data))
			first := true
			switch which {
			case "miss":
				q.Accept = func(Record) bool { return false }
			case "audit":
				first = false
			case "otherKind":
				q.Kind = bridge.MemoryReceipt
			}
			_, coverage, err := Lookup(context.Background(), s, q, Options{Workers: 1}, first)
			if err != nil || coverage.InputTiming != nil {
				t.Fatal("nonpositive INPUT lookup retained timing", coverage, err)
			}
		})
	}
}

func (s *timingVerifySource) Verify(ctx context.Context) error {
	s.verifies++
	if s.verifies == 2 {
		time.Sleep(20 * time.Millisecond)
		return s.finalErr
	}
	return ctx.Err()
}

func TestInputTimingIncludesFinalVerifyWithoutChangingError(t *testing.T) {
	for _, failure := range []error{nil, errors.New("fixture.process_changed")} {
		s, q, fresh := refreshFixture(t, 1)
		s.regions[0].End = uint64(len(s.data))
		src := &timingVerifySource{fakeSource: s, finalErr: failure}
		records, coverage, err := Lookup(context.Background(), src, q, Options{Workers: 1}, true)
		if !errors.Is(err, failure) || len(records) != 1 || records[0].Address != fresh {
			t.Fatal("timing changed acceptance or final error", records, err)
		}
		data, _ := json.Marshal(coverage)
		var value map[string]any
		_ = json.Unmarshal(data, &value)
		timing, ok := value["inputTiming"].(map[string]any)
		if !ok {
			t.Fatal("missing INPUT acceptance/drain timing", string(data))
		}
		if timing["acceptedAtOffsetMillis"].(float64) < 0 || timing["drainMillis"].(float64) < 20 || len(timing) != 2 {
			t.Fatal("timing lost final Verify or retained nonrelative values", timing)
		}
	}
}
