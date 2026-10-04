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

// A root recipe and numeric calibration never grant write authority. The first
// stopped-helper Retail trial ended in a client security crash; keep this exact
// image disqualified until a different lifetime mechanism is proved safe.
func DuplexWriteCapability(executableSHA256, build, product string) DuplexWriterProfile {
	p := DuplexWriterProfile{ExecutableSHA256: executableSHA256, Build: build, Product: product, Validation: "not_run"}
	if executableSHA256 == "d41f11de411f6fdb280a7c1c6380ba3d1b17614f03e5aca8fabce2cc715cd7dd" && build == "12.1.0.69933" && product == "retail" {
		p.ID = "retail-69933-stopped-mailbox-v1-rejected"
		p.LayoutEvidence = "matching runtime .text static evidence: Table array=0x20, count=0x40, TValue stride=24; runtime calibration and frozen row rechecked for each write"
		p.LifetimeEvidence = "2026-10-04 real-client trial: debugger stopped target for 37.368 ms, one 6,293,376-byte WPM and readback completed; client security crash followed about one second later; cause not isolated"
		p.Validation = "failed"
	}
	return p
}
