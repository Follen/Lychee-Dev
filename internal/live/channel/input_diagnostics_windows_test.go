//go:build windows && amd64

package channel

import (
	"context"
	"encoding/json"
	"sync/atomic"
	"testing"
	"time"
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
