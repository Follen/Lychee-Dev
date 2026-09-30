//go:build windows && amd64

package channel

import (
	"context"
	"encoding/json"
	"errors"
	"sync/atomic"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/live/memory"
)

func inputDiagnosticJSON(t *testing.T, n *Native) map[string]any {
	t.Helper()
	data, err := json.Marshal(n.observationMetrics())
	if err != nil {
		t.Fatal(err)
	}
	var metrics map[string]any
	if err = json.Unmarshal(data, &metrics); err != nil {
		t.Fatal(err)
	}
	diag, ok := metrics["inputPredicates"].(map[string]any)
	if !ok {
		t.Fatal("missing bounded input predicate diagnostics")
	}
	return diag
}

func TestInputTimingSamplesAreBoundedAndDoNotRetainAbsoluteClocks(t *testing.T) {
	n := &Native{}
	n.memorySession()
	for i := 0; i < inputTimingLimit+2; i++ {
		var c inputPredicateCollector
		state := InputObservation{SampleMillis: 900000 + int64(i)}
		c.candidate(state, nil, bridge.SlotEnvelope{}, 0, state.SampleMillis+100)
		payload, _ := json.Marshal(state)
		c.positive([]memory.Record{{Payload: payload}}, &memory.InputLookupTiming{AcceptedAtOffsetMillis: 17, DrainMillis: 23}, state.SampleMillis+123, nil, false)
		n.recordInputPredicates(&c)
	}
	for i := 0; i < 30; i++ {
		n.recordInputPredicates(&inputPredicateCollector{})
	}
	metrics := n.observationMetrics()
	if len(metrics.InputTimings.Samples) != inputTimingLimit || !metrics.InputTimings.Truncated {
		t.Fatal("timing samples exceeded cap or hid truncation", metrics.InputTimings)
	}
	data, _ := json.Marshal(metrics.InputTimings)
	var raw map[string]any
	_ = json.Unmarshal(data, &raw)
	for _, entry := range raw["samples"].([]any) {
		sample := entry.(map[string]any)
		if len(sample) != 5 || sample["outcome"] != "fresh" {
			t.Fatal("timing retained unbounded metadata", sample)
		}
		for _, field := range []string{"acceptedAtOffsetMillis", "drainMillis", "acceptedSampleAgeMillis", "finishSampleAgeMillis"} {
			value, ok := sample[field].(float64)
			if !ok || value < 0 || value > 123 {
				t.Fatal("timing retained an absolute clock or raw value", sample)
			}
		}
	}
}

func TestInputTimingFreshStampEvictionAndMissingRecordUseNullAges(t *testing.T) {
	var c inputPredicateCollector
	for i := 0; i < 9; i++ {
		c.candidate(InputObservation{SampleMillis: int64(i)}, nil, bridge.SlotEnvelope{}, 0, int64(i)+100)
	}
	payload, _ := json.Marshal(InputObservation{SampleMillis: 0})
	for _, records := range [][]memory.Record{{{Payload: payload}}, nil} {
		c.positive(records, &memory.InputLookupTiming{AcceptedAtOffsetMillis: 1, DrainMillis: 2}, 1000, errors.New("fixture.Verify"), true)
		if c.freshCount != 8 || !c.haveTiming || c.timing.AcceptedSampleAgeMillis != nil || c.timing.FinishSampleAgeMillis != nil || c.timing.Outcome != "lookup_error" {
			t.Fatal("evicted/missing stamp invented an age", c.timing)
		}
	}
}

func TestInputRefreshDiagnosticCountersAccumulateWithoutRawErrors(t *testing.T) {
	n := &Native{}
	n.memorySession()
	for i := 0; i < 2; i++ {
		var c inputPredicateCollector
		c.refreshAttempt()
		c.refreshHit(false)
		c.refreshHit(true)
		c.edgeWait(nil)
		c.edgeWait(context.DeadlineExceeded)
		c.edgeWait(errors.New("fixture.raw_error_must_not_survive"))
		n.recordInputPredicates(&c)
	}
	d := n.observationMetrics().InputRefresh
	if d.Attempts != 2 || d.ImmediateHit != 2 || d.AfterEdgeHit != 2 || d.EdgeWaitSucceeded != 2 || d.EdgeWaitExpired != 2 || d.EdgeWaitOther != 2 {
		t.Fatal("refresh counter scope reset or mixed outcomes", d)
	}
	if len(n.observationMetrics().InputTimings.Samples) != 0 {
		t.Fatal("no-hit observations crowded positive timing samples")
	}
}

func TestInputDiagnosticsClassifyWithoutChangingAcceptance(t *testing.T) {
	for _, tc := range []struct {
		name, counter    string
		tick, now, after int64
		alter            func(*InputObservation)
		age              bool
	}{
		{"staleAge", "staleAge", 1000, 1600, 0, nil, true},
		{"staleAfter", "staleAfterInput", 1000, 1100, 1100, nil, true},
		{"target", "targetChanged", 1000, 1100, 0, func(s *InputObservation) {
			s.NextSlot++
			s.Owner = "other"
			s.GUID = "other"
			s.Runtime = "other"
			s.Fence++
			s.Build = "other"
		}, false},
		{"clock", "clockInvalid", 1300, 1100, 0, nil, false},
		{"structure", "structInvalid", 1000, 1100, 0, func(s *InputObservation) { s.Schema = "invalid" }, false},
	} {
		t.Run(tc.name, func(t *testing.T) {
			n, source, e, write := cadenceFixture(t)
			n.Hints = nil
			write(512<<10, tc.tick, tc.alter)
			sample, err := n.observeInputFrom(context.Background(), source, e, tc.after, func() int64 { return tc.now }, time.Now)
			if err == nil || sample.Address != 0 {
				t.Fatal("rejected candidate authorized input", sample, err)
			}
			d := inputDiagnosticJSON(t, n)
			if d[tc.counter] != d["predicateCalls"] || d["predicateCalls"].(float64) < 1 || d["fresh"] != float64(0) {
				t.Fatal("wrong classification", d)
			}
			if (d["newestSampleAgeMillis"] != nil) != tc.age {
				t.Fatal("unqualified payload supplied age", d)
			}
			if tc.name == "target" {
				for _, field := range []string{"slotMismatch", "ownerMismatch", "actorMismatch", "runtimeMismatch", "fenceMismatch", "buildMismatch"} {
					if d[field] != float64(1) {
						t.Fatal("target mismatch values not classified", field, d)
					}
				}
			}
		})
	}
}

func TestInputDiagnosticsAccumulateAndClearUnqualifiedLatestAge(t *testing.T) {
	n, source, e, write := cadenceFixture(t)
	n.Hints = nil
	observe := func() {
		_, _ = n.observeInputFrom(context.Background(), source, e, 0, func() int64 { return 1100 }, time.Now)
	}
	observe()
	write(512<<10, 1500, nil)
	observe()
	d := inputDiagnosticJSON(t, n)
	if d["observations"] != float64(2) || d["fresh"] != float64(1) || d["clockInvalid"] != float64(1) || d["newestSampleAgeMillis"] != nil || d["scanEndSampleAgeMillis"] != nil {
		t.Fatal("scope reset or unqualified latest age", d)
	}
	for field, value := range d {
		if value != nil {
			if _, ok := value.(float64); !ok {
				t.Fatal("diagnostics retained identifying or raw values", field, value)
			}
		}
	}
}

func TestInputDiagnosticsSeparateAcceptanceFromDrainExpiry(t *testing.T) {
	n, source, e, _ := cadenceFixture(t)
	n.Hints = nil
	var calls atomic.Int32
	uptime := func() int64 {
		if calls.Add(1) == 1 {
			return 1100
		}
		return 1800
	}
	sample, err := n.observeInputFrom(context.Background(), source, e, 0, uptime, time.Now)
	if err == nil || sample.Address != 0 {
		t.Fatal("drain expiry authorized input", sample, err)
	}
	d := inputDiagnosticJSON(t, n)
	if d["fresh"] != float64(1) || d["finishStale"] != float64(1) || d["newestSampleAgeMillis"] != float64(100) || d["scanEndSampleAgeMillis"] != float64(800) {
		t.Fatal("drain expiry lost its diagnostic boundary", d)
	}
	if n.observationMetrics().Total.Validated != 1 {
		t.Fatal("accepted candidate disappeared from memory accounting")
	}
}

func TestInputTimingDiagnosticSeparatesAcceptedAndFinishedSampleAge(t *testing.T) {
	n, source, e, _ := cadenceFixture(t)
	n.Hints = nil
	var calls atomic.Int32
	uptime := func() int64 {
		if calls.Add(1) == 1 {
			return 1100
		}
		return 1800
	}
	_, _ = n.observeInputFrom(context.Background(), source, e, 0, uptime, time.Now)
	data, _ := json.Marshal(n.observationMetrics())
	var metrics map[string]any
	_ = json.Unmarshal(data, &metrics)
	timings, ok := metrics["inputTimings"].(map[string]any)
	if !ok {
		t.Fatal("missing bounded INPUT observation timing", string(data))
	}
	samples := timings["samples"].([]any)
	if len(samples) != 1 || timings["truncated"] != false {
		t.Fatal(timings)
	}
	sample := samples[0].(map[string]any)
	if sample["acceptedSampleAgeMillis"] != float64(100) || sample["finishSampleAgeMillis"] != float64(800) || sample["outcome"] != "stale" {
		t.Fatal("accepted sample age and drain expiry conflated", sample)
	}
	for _, key := range []string{"acceptedAtOffsetMillis", "drainMillis"} {
		if _, ok := sample[key].(float64); !ok {
			t.Fatal("missing relative lookup clock", key, sample)
		}
	}
}
