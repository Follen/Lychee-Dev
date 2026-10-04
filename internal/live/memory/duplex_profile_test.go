package memory

import (
	"strings"
	"testing"
)

func TestRootAndCalibrationDoNotGrantWriterCapability(t *testing.T) {
	trial := DuplexWriteCapability("d41f11de411f6fdb280a7c1c6380ba3d1b17614f03e5aca8fabce2cc715cd7dd", "12.1.0.69933", "retail")
	if !trial.CanDirectWrite() || trial.Mode != "direct" || trial.Validation != "retail_small_and_1mib_verified" || trial.ID != "retail-69933-direct-mailbox-v1-trial" {
		t.Fatal("exact direct-write trial profile missing", trial)
	}
	for _, v := range []struct{ hash, build, product string }{
		{"d41f11de411f6fdb280a7c1c6380ba3d1b17614f03e5aca8fabce2cc715cd7dd", "12.1.0.69934", "retail"},
		{"other", "12.1.0.69933", "retail"},
		{"other", "5.5.4.1", "classic"},
		{strings.Repeat("a", 64), "5.5.4.50504", "classic"},
		{strings.Repeat("b", 64), "3.8.0.38002", "titan"},
		{ForeverLuaMailboxExecutableSHA256, "1.16.0.16001", "forever"},
	} {
		p := DuplexWriteCapability(v.hash, v.build, v.product)
		if p.CanDirectWrite() || p.Validation != "not_run" {
			t.Fatal("unreviewed writer enabled", p)
		}
	}
}

func TestWriterQualificationsSeparateRecipeABIAndTrial(t *testing.T) {
	profile := DuplexWriteCapability(RetailLuaMailboxExecutableSHA256, retailReloadSourceBuild, "retail")
	observed := DuplexProfileObservation{ProcessIdentified: true, RootRecipeID: LuaMailboxRootRecipeID, TypedMailbox: true, LifecycleID: RetailReloadRecipeID, NumericRow: true}
	if q := profile.Qualify(observed); q.Level != QualificationL3 || !q.DirectWrite || len(q.Missing) == 0 {
		t.Fatal("exact trial lost restricted qualification or promoted itself to complete acceptance", q)
	}
	for _, test := range []struct {
		name   string
		change func(*DuplexProfileObservation)
		level  QualificationLevel
	}{
		{"missing_process_instance", func(o *DuplexProfileObservation) { o.ProcessIdentified = false }, QualificationUnknown},
		{"rva_without_recipe", func(o *DuplexProfileObservation) { o.RootRecipeID = "" }, QualificationL0},
		{"ambiguous_recipe", func(o *DuplexProfileObservation) { o.RootRecipeID = "ambiguous" }, QualificationL0},
		{"missing_typed_mailbox", func(o *DuplexProfileObservation) { o.TypedMailbox = false }, QualificationL0},
		{"unqualified_lifecycle", func(o *DuplexProfileObservation) { o.LifecycleID = "other" }, QualificationL0},
		{"numeric_or_freeze_failure", func(o *DuplexProfileObservation) { o.NumericRow = false }, QualificationL1},
	} {
		t.Run(test.name, func(t *testing.T) {
			o := observed
			test.change(&o)
			if q := profile.Qualify(o); q.Level != test.level || q.DirectWrite || len(q.Missing) == 0 {
				t.Fatal(q)
			}
		})
	}
	for _, client := range []struct{ hash, build, product string }{
		{strings.Repeat("a", 64), retailReloadSourceBuild, "retail"},
		{RetailLuaMailboxExecutableSHA256, "12.1.0.69934", "retail"},
		{strings.Repeat("b", 64), "5.5.4.50504", "classic"},
		{strings.Repeat("c", 64), "3.8.0.38002", "titan"},
	} {
		p := DuplexWriteCapability(client.hash, client.build, client.product)
		if q := p.Qualify(observed); q.Level != QualificationL1 || q.DirectWrite {
			t.Fatal("recipe/calibration transferred image authority", client, q)
		}
	}
	profile.ImageQualification = QualificationL2
	profile.Trial = DuplexTrialEvidence{}
	if q := profile.Qualify(observed); q.Level != QualificationL2 || q.DirectWrite {
		t.Fatal("ABI conditions alone granted trial authority", q)
	}
}

func TestNamedABIAndRealTrialEvidenceCannotBeDowngraded(t *testing.T) {
	for _, change := range []func(*DuplexWriterProfile){
		func(p *DuplexWriterProfile) { p.ABI.TValueStride = 16 },
		func(p *DuplexWriterProfile) { p.ABI.SecretOffset = 10 },
		func(p *DuplexWriterProfile) { p.ABI.FreezeSemantics = "guessed" },
		func(p *DuplexWriterProfile) { p.RecipeVersion++ },
		func(p *DuplexWriterProfile) { p.LifecycleID = "guessed" },
		func(p *DuplexWriterProfile) { p.Trial.RealClient = false },
		func(p *DuplexWriterProfile) { p.Trial.CLICommit = "" },
		func(p *DuplexWriterProfile) { p.Trial.AddonCommit = "" },
		func(p *DuplexWriterProfile) { p.Trial.Passed = []string{"small_command"} },
		func(p *DuplexWriterProfile) { p.Trial.Mode = "stopped" },
		func(p *DuplexWriterProfile) { p.ImageQualification = QualificationL4 },
	} {
		p := DuplexWriteCapability(RetailLuaMailboxExecutableSHA256, retailReloadSourceBuild, "retail")
		change(&p)
		if p.CanDirectWrite() {
			t.Fatal("missing/changed evidence allowed writer", p)
		}
	}
}
