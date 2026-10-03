//go:build windows && amd64

package command

import (
	"testing"

	"github.com/follenfang/lycheedev/internal/live/duplex"
	"github.com/follenfang/lycheedev/internal/live/duplexhost"
)

func TestDoctorReadableReadyDoesNotHideUnqualifiedWriter(t *testing.T) {
	result := duplexhost.ProjectResult{Status: &duplex.Sendbox{Ready: true, ControlReady: true, Phase: "idle"},
		Diagnostics: map[string]duplexhost.Diagnostic{"writerProfile": {State: "unverified", Reason: "VM lifetime is not pinned"}}}
	checks := liveDoctorProjection(result, nil)
	if len(checks) != 2 || checks[1].OK || checks[1].Status != "error" || checks[1].Code != "live.duplex_writer_profile_unverified" {
		t.Fatalf("readable mailbox qualified writer: %+v", checks)
	}
	if checks[0].Data["diagnostics"] == nil || checks[0].Data["sendbox"] == nil {
		t.Fatal("doctor discarded target layers")
	}
	result.Diagnostics = nil
	result.Status.Ready = false
	checks = liveDoctorProjection(result, nil)
	if checks[0].Status != "warn" || checks[0].Detail == "" {
		t.Fatalf("business readiness hid independent cleanup observation: %+v", checks)
	}
}
