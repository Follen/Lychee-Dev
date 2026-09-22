package vault

import (
	"os"
	"path/filepath"
	"sort"
)

// LegacyRoot is a known pre-2.0 data location. Detection is limited to path
// existence and marker-name sniffing (Lstat only): legacy contents are never
// read, copied, merged, imported, or deleted, and legacy SavedVariables are
// not opened at all.
type LegacyRoot struct {
	Path     string   `json:"path"`
	Kind     string   `json:"kind"`
	Detected bool     `json:"detected"`
	Markers  []string `json:"markers,omitempty"`
	Detail   string   `json:"detail,omitempty"`
}

// LegacyRootCandidates derives detection candidates from caller-resolved
// system paths. Modules never read environment variables or user home
// directories themselves; the CLI resolves those and passes them in. The
// environment variable no longer carries the old "user home plus .lycheedev"
// meaning, so only exact absolute paths are accepted here.
func LegacyRootCandidates(userHome, localAppData string) []LegacyRoot {
	roots := make([]LegacyRoot, 0, 4)
	if userHome != "" {
		roots = append(roots,
			LegacyRoot{Path: filepath.Join(userHome, ".wowdoc"), Kind: "wowdoc"},
			LegacyRoot{Path: filepath.Join(userHome, ".wowdata"), Kind: "wowdata"},
			LegacyRoot{Path: filepath.Join(userHome, ".lycheedev"), Kind: "lycheedev-legacy"},
		)
	}
	if localAppData != "" {
		roots = append(roots, LegacyRoot{Path: filepath.Join(localAppData, "LycheeDev", "automation"), Kind: "lycheedev-automation"})
	}
	return roots
}

// legacyMarkers lists marker file or directory NAMES used for format sniffing.
// Only existence is observed; marker contents are never read.
var legacyMarkers = map[string][]string{
	"wowdoc":               {"catalog", "gitstore", "objectstore", "indexer", "store", "bin", "wowdoc.json"},
	"wowdata":              {"wowdata.config.v1", "config.json", "profiles", "builds", "cache", "storage", "state", "bin", "python"},
	"lycheedev-legacy":     {"config.json", "python", "bin", "npm", "storage", "state", "cache", "profiles"},
	"lycheedev-automation": {"automation.py", "instances.py", "python", "config.json", "task-registry.json"},
}

// InspectLegacyRoots classifies each candidate with Lstat-only existence and
// marker-name sniffing. It never opens, copies or modifies anything under a
// legacy root, so sentinel data placed there stays byte-identical.
func InspectLegacyRoots(roots []LegacyRoot) []LegacyRoot {
	result := make([]LegacyRoot, 0, len(roots))
	for _, root := range roots {
		result = append(result, inspectLegacyRoot(root))
	}
	return result
}

func inspectLegacyRoot(root LegacyRoot) LegacyRoot {
	info, err := os.Lstat(root.Path)
	if err != nil {
		root.Detected = false
		root.Detail = "absent"
		return root
	}
	if !info.IsDir() {
		root.Detected = true
		root.Detail = "present but not a directory; left untouched"
		return root
	}
	markers := sniffMarkers(root.Path, legacyMarkers[root.Kind])
	root.Markers = markers
	root.Detected = true
	switch {
	case len(markers) > 0:
		root.Detail = "legacy layout detected (markers: " + joinNames(markers) + "); isolated, never read"
	default:
		root.Detail = "present without known markers; isolated, never read"
	}
	return root
}

// sniffMarkers reports which marker names exist under path. It performs no
// reads and returns names only.
func sniffMarkers(path string, names []string) []string {
	found := make([]string, 0, len(names))
	for _, name := range names {
		if _, err := os.Lstat(filepath.Join(path, name)); err == nil {
			found = append(found, name)
		}
	}
	sort.Strings(found)
	return found
}

func joinNames(names []string) string {
	out := ""
	for i, name := range names {
		if i > 0 {
			out += ", "
		}
		out += name
	}
	return out
}
