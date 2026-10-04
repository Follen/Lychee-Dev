package memory

// Root discovery and numeric calibration establish read observations. Neither
// grants write authority or proves the collector cannot relocate an array.
type DuplexWriterProfile struct {
	ID               string `json:"id"`
	ExecutableSHA256 string `json:"executableSHA256"`
	Build            string `json:"build"`
	Product          string `json:"product"`
	LayoutEvidence   string `json:"layoutEvidence"`
	LifetimeEvidence string `json:"lifetimeEvidence"`
	Validation       string `json:"validation"`
	Eligible         bool   `json:"eligible"`
}

// Reviewed profiles must be immutable, build AND executable-bound, with
// independent ABI/lifetime evidence. An unknown build never inherits one from
// a successful RVA recipe, a familiar product or a six-number calibration.
func DuplexWriteCapability(executableSHA256, build, product string) DuplexWriterProfile {
	p := DuplexWriterProfile{ExecutableSHA256: executableSHA256, Build: build, Product: product, Validation: "not_run"}
	if executableSHA256 == "d41f11de411f6fdb280a7c1c6380ba3d1b17614f03e5aca8fabce2cc715cd7dd" && build == "12.1.0.69933" && product == "retail" {
		p.ID = "retail-69933-numeric-inbox-candidate"
		p.LayoutEvidence = "matching runtime .text static evidence: Table array=0x20, count=0x40, TValue stride=24; current-client calibration not_run"
		p.LifetimeEvidence = "strong roots/freeze do not pin backing storage or VM lifetime across final guard and WPM; collector guarantees unproved"
	}
	return p
}
