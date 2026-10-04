package memory

// Root discovery and numeric calibration establish read observations. Neither
// grants write authority or proves the collector cannot relocate an array.
type DuplexWriterProfile struct {
	ID               string `json:"id"`
	Mode             string `json:"mode"`
	ExecutableSHA256 string `json:"executableSHA256"`
	Build            string `json:"build"`
	Product          string `json:"product"`
	LayoutEvidence   string `json:"layoutEvidence"`
	LifetimeEvidence string `json:"lifetimeEvidence"`
	Validation       string `json:"validation"`
	Eligible         bool   `json:"eligible"`
}

// A root recipe and numeric calibration never grant write authority. The
// debugger-stopped route is permanently rejected for this image. The owner
// authorized a separate, exact-image direct-write trial without suspending the
// game; that route still has an unresolved final-check-to-reload race.
func DuplexWriteCapability(executableSHA256, build, product string) DuplexWriterProfile {
	p := DuplexWriterProfile{ExecutableSHA256: executableSHA256, Build: build, Product: product, Validation: "not_run"}
	if executableSHA256 == "d41f11de411f6fdb280a7c1c6380ba3d1b17614f03e5aca8fabce2cc715cd7dd" && build == "12.1.0.69933" && product == "retail" {
		p.ID = "retail-69933-direct-mailbox-v1-trial"
		p.Mode = "direct"
		p.LayoutEvidence = "matching runtime .text static evidence: Table array=0x20, count=0x40, TValue stride=24; runtime calibration and frozen row rechecked for each write"
		p.LifetimeEvidence = "owner-authorized no-suspend trial; strong Lua roots pin ordinary GC; final-check-to-reload race remains; previous debugger-assisted trial crashed and is disabled"
		p.Eligible = true
	}
	return p
}
