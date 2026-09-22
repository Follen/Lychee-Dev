// Package codebase contains versioned source-repository metadata.
package codebase

import "fmt"

// RepositorySpec identifies a source repository and the product tracks that it
// exposes. The map value is the Git branch for the corresponding product key.
type RepositorySpec struct {
	Key    string            `json:"key"`
	Label  string            `json:"label"`
	URL    string            `json:"url"`
	Tracks map[string]string `json:"tracks"`
}

var repositoryCatalog = []RepositorySpec{
	{
		Key:   "wow-ui-source",
		Label: "World of Warcraft UI Source Mirror",
		URL:   "https://github.com/Gethe/wow-ui-source.git",
		Tracks: map[string]string{
			"retail":          "live",
			"ptr":             "ptr",
			"ptr2":            "ptr2",
			"beta":            "beta",
			"classic":         "classic",
			"classic-ptr":     "classic_ptr",
			"classic-beta":    "classic_beta",
			"classic-era":     "classic_era",
			"classic-era-ptr": "classic_era_ptr",
			"anniversary":     "classic_anniversary",
			"titan":           "classic_titan",
			"forever":         "forever",
		},
	},
	{
		Key:   "elvui",
		Label: "ElvUI",
		URL:   "https://github.com/tukui-org/ElvUI.git",
		Tracks: map[string]string{
			"main": "main",
			"ptr":  "ptr",
		},
	},
	{
		Key:   "weakauras",
		Label: "WeakAuras2",
		URL:   "https://github.com/WeakAuras/WeakAuras2.git",
		Tracks: map[string]string{
			"main": "main",
		},
	},
	{
		Key:   "ndui",
		Label: "NDui",
		URL:   "https://github.com/siweia/NDui.git",
		Tracks: map[string]string{
			"main":        "master",
			"classic":     "Classic",
			"era":         "Era",
			"anniversary": "Anniversary",
			"titan":       "Titan",
		},
	},
	{
		Key:   "ellesmereui",
		Label: "EllesmereUI",
		URL:   "https://github.com/EllesmereGaming/EllesmereUI.git",
		Tracks: map[string]string{
			"main": "main",
		},
	},
}

// Repositories returns an independent snapshot of the repository catalog.
func Repositories() []RepositorySpec {
	result := make([]RepositorySpec, len(repositoryCatalog))
	for i, repository := range repositoryCatalog {
		result[i] = repository
		result[i].Tracks = make(map[string]string, len(repository.Tracks))
		for product, branch := range repository.Tracks {
			result[i].Tracks[product] = branch
		}
	}
	return result
}

// LookupRepository returns the repository with the exact requested key.
func LookupRepository(key string) (RepositorySpec, error) {
	for _, repository := range Repositories() {
		if repository.Key == key {
			return repository, nil
		}
	}
	return RepositorySpec{}, fmt.Errorf("codebase.repository_not_found: %q", key)
}
