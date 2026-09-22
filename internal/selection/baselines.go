package selection

// ClientBaseline is a project-approved Interface observation at an exact source
// commit. It is not a formula derived from a version string, and does not prove
// which client is currently running. Unknown commits remain unverified.
type ClientBaseline struct {
	Product      string `json:"product"`
	Interface    int    `json:"interface"`
	SourceCommit string `json:"sourceCommit"`
	ProductCode  string `json:"productCode"`
	BuildSeries  string `json:"buildSeries"`
	TOC          string `json:"toc"`
}

func VerifiedClientBaselines() []ClientBaseline {
	return []ClientBaseline{
		{Product: "retail", ProductCode: "wow", BuildSeries: "12.1.0", TOC: "Lychee Dev_Mainline.toc", Interface: 120100, SourceCommit: "31c7f7b9cc79e56c986b365c06a6afbcf3c9177b"},
		{Product: "classic", ProductCode: "wow_classic", BuildSeries: "5.5.4", TOC: "Lychee Dev_Mists.toc", Interface: 50504, SourceCommit: "1028c1e687f721ba9d3af14d1b12a5745e4227c7"},
		{Product: "titan", ProductCode: "wow_classic_titan", BuildSeries: "3.80.2", TOC: "Lychee Dev_Wrath.toc", Interface: 38002, SourceCommit: "825d29d3662b372f0bead725ee6abd339e4a77b5"},
		{Product: "forever", ProductCode: "wow_forever", BuildSeries: "1.60.1", TOC: "Lychee Dev_Forever.toc", Interface: 16001, SourceCommit: "4d5d706b8e01c5ebe01c8dd9b7a07151d8d37069"},
	}
}
func SourceInterface(pin SourcePin) (int, bool) {
	if pin.Repository != "wow-ui-source" {
		return 0, false
	}
	for _, baseline := range VerifiedClientBaselines() {
		if pin.Product == baseline.Product && pin.ExactCommit == baseline.SourceCommit {
			return baseline.Interface, true
		}
	}
	return 0, false
}
