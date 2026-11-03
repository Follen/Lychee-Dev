package selection

// SourceLoadContext selects source layout facts, independently of installation
// folders and numeric version comparisons. Unknown layouts remain unresolved.
func SourceLoadContext(pin SourcePin) (family, game string) {
	if pin.Repository != "wow-ui-source" {
		return "", ""
	}
	switch SourceProductFamily(pin.Product) {
	case "retail":
		return "Mainline", "Standard"
	case "classic":
		// The rolling Classic track also contains older Cata/Wrath trees.
		if pin.ExactCommit == "1028c1e687f721ba9d3af14d1b12a5745e4227c7" {
			return "Classic", "Mists"
		}
		return "Classic", ""
	case "classic-era":
		return "Classic", "Vanilla"
	case "anniversary":
		// Anniversary moved from Vanilla to TBC; do not relabel old pins.
		if pin.ExactCommit == "2cfec7c00f0f24762d5ca35a4beb1e93c0cde328" {
			return "Classic", "TBC"
		}
		return "Classic", ""
	case "titan":
		return "Classic", ""
	case "forever":
		// 4d5d706b8e01c5ebe01c8dd9b7a07151d8d37069 FrameXML.toc
		// selects Mainline shared files and Camelot [Game] overrides.
		return "Mainline", "Camelot"
	}
	return "", ""
}
