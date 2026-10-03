package memory

import "testing"

func TestRootAndCalibrationDoNotGrantWriterCapability(t *testing.T) {
	for _, v := range []struct{ hash, build, product string }{
		{"d41f11de411f6fdb280a7c1c6380ba3d1b17614f03e5aca8fabce2cc715cd7dd", "12.1.0.69933", "retail"},
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
