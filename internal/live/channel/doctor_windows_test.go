//go:build windows && amd64

package channel

import (
	"strings"
	"testing"
)

func TestDoctorReportsStartupSlotReloadWithoutBlockingPhysicalInput(t *testing.T) {
	blocked := false
	input := InputObservation{InputBlocked: &blocked, Bindings: primaryInputBindings(), SlotReloadRequired: true}
	r := DoctorResult{Healthy: true}
	r.inspectInput(input)
	if r.Healthy || len(r.Checks) != 2 || r.Checks[0].ID != "live.input" || r.Checks[0].State != "ok" || r.Checks[1].ID != "live.slot_activation" || r.Checks[1].State != "blocked" || *input.InputBlocked {
		t.Fatalf("startup activation and physical readiness were conflated: %+v", r)
	}
}

func TestDoctorReportsLatestCallbackFailure(t *testing.T) {
	for _, failure := range []struct{ stage, reason string }{
		{"load_failed", "slot_addon_disabled"},
		{"load_failed", "slot_load_failed"},
		{"no_dispatch", "slot_loader_no_dispatch"},
		{"exception", "slot_wake_failed"},
		{"rejected", "input_protection_busy"},
	} {
		t.Run(failure.reason, func(t *testing.T) {
			blocked := false
			d := attemptDiagnosticFixture()
			d.Stage, d.Reason = failure.stage, failure.reason
			input := InputObservation{InputBlocked: &blocked, Bindings: primaryInputBindings(), InputDiagnostic: &d}
			r := DoctorResult{Healthy: true}
			r.inspectInput(input)
			var attempt *DoctorCheck
			for i := range r.Checks {
				if r.Checks[i].ID == "live.input_attempt" {
					attempt = &r.Checks[i]
				}
			}
			if r.Healthy || attempt == nil || attempt.State != "error" || !strings.Contains(attempt.Detail, failure.reason) || !strings.Contains(attempt.Detail, "slot 7") {
				t.Fatalf("ready INPUT hid callback failure: %+v", r)
			}
			if *input.InputBlocked || d.Received || d.Stage != failure.stage {
				t.Fatal("doctor altered input or receipt authority")
			}
		})
	}
}

func TestDoctorCallbackDiagnosticDoesNotReplaceReadiness(t *testing.T) {
	for _, stage := range []string{"", "entered", "loading", "received"} {
		for _, blocked := range []bool{false, true} {
			d := attemptDiagnosticFixture()
			d.Stage, d.Reason, d.Received = stage, "", stage == "received"
			input := InputObservation{InputBlocked: &blocked, Reason: "input_combat_lockdown", Bindings: primaryInputBindings()}
			if stage != "" {
				input.InputDiagnostic = &d
			}
			r := DoctorResult{Healthy: true}
			r.inspectInput(input)
			if r.Healthy == blocked || len(r.Checks) != 1 || r.Checks[0].ID != "live.input" {
				t.Fatalf("nonfailure diagnostic changed readiness: stage=%s blocked=%v result=%+v", stage, blocked, r)
			}
		}
	}
}
