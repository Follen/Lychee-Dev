package memory

import "testing"

func TestRootAndCalibrationDoNotGrantWriterCapability(t *testing.T) {
	trial := DuplexWriteCapability("d41f11de411f6fdb280a7c1c6380ba3d1b17614f03e5aca8fabce2cc715cd7dd", "12.1.0.69933", "retail")
	if !trial.Eligible || trial.Mode != "direct" || trial.Validation != "not_run" || trial.ID != "retail-69933-direct-mailbox-v1-trial" {
		t.Fatal("exact direct-write trial profile missing", trial)
	}
	for _, v := range []struct{ hash, build, product string }{
		{"d41f11de411f6fdb280a7c1c6380ba3d1b17614f03e5aca8fabce2cc715cd7dd", "12.1.0.69934", "retail"},
		{"other", "12.1.0.69933", "retail"},
		{"other", "5.5.4.1", "classic"},
	} {
		p := DuplexWriteCapability(v.hash, v.build, v.product)
		if p.Eligible || p.Validation != "not_run" {
			t.Fatal("unreviewed writer enabled", p)
		}
	}
}
