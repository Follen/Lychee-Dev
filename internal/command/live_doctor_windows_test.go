//go:build windows && amd64

package command

import (
	"errors"
	"testing"

	"github.com/follenfang/lycheedev/internal/live/duplex"
	"github.com/follenfang/lycheedev/internal/live/duplexhost"
	"github.com/follenfang/lycheedev/internal/live/memory"
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

func TestDoctorNativeWorldSurvivesMissingAddonAndUnknownIsNotFalse(t *testing.T) {
	world := false
	r := duplexhost.ProjectResult{NativeReload: &memory.ReloadObservation{State: "no_reload_observed", WorldState: "not_in_world", WorldReady: &world}}
	checks := liveDoctorProjection(r, errors.New("live.duplex_runtime_unavailable"))
	if len(checks) != 3 || checks[0].OK || !checks[1].OK || checks[2].Status != "warn" || checks[2].Code != "live.duplex_character_not_in_world" {
		t.Fatalf("native world lost behind absent addon: %+v", checks)
	}
	if checks[0].Data["nativeReload"] == nil {
		t.Fatal("native evidence discarded")
	}
	r.NativeReload.WorldReady = nil
	r.NativeReload.WorldState = "unknown"
	checks = liveDoctorProjection(r, nil)
	if checks[2].OK || checks[2].Status != "error" || checks[2].Code != "live.duplex_world_state_unavailable" {
		t.Fatalf("unknown native world treated as false or ready: %+v", checks)
	}
	world = true
	r.NativeReload.WorldReady, r.NativeReload.WorldState = &world, "world_ready"
	r.NativeReload.State = "reloading"
	checks = liveDoctorProjection(r, nil)
	if checks[1].OK || checks[1].Code != "live.duplex_runtime_reloading" || !checks[2].OK {
		t.Fatalf("world=true concealed reload: %+v", checks)
	}
}
