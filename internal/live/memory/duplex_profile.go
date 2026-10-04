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

// Trial profiles are immutable, build AND executable-bound. This exact Retail
// candidate may write only through the stopped helper, which repeats runtime,
// lifecycle and row validation while the target cannot free its Lua arena.
// An unknown build never inherits eligibility from a recipe or calibration.
func DuplexWriteCapability(executableSHA256, build, product string) DuplexWriterProfile {
	p := DuplexWriterProfile{ExecutableSHA256: executableSHA256, Build: build, Product: product, Validation: "not_run"}
	if executableSHA256 == "d41f11de411f6fdb280a7c1c6380ba3d1b17614f03e5aca8fabce2cc715cd7dd" && build == "12.1.0.69933" && product == "retail" {
		p.ID = "retail-69933-stopped-mailbox-v1-trial"
		p.LayoutEvidence = "matching runtime .text static evidence: Table array=0x20, count=0x40, TValue stride=24; runtime calibration and frozen row rechecked for each write"
		p.LifetimeEvidence = "owned-process debugger detach/crash/watchdog tests passed; stopped helper rechecks Lua root, actor and reload before one WPM; real client not_run"
		p.Eligible = true
	}
	return p
}
