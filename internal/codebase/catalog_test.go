package codebase

import (
	"strings"
	"testing"
)

func TestRepositoriesContainAllSourceRepositories(t *testing.T) {
	repositories := Repositories()
	if len(repositories) != 5 {
		t.Fatalf("repository count = %d, want 5", len(repositories))
	}

	wantTracks := map[string]map[string]string{
		"wow-ui-source": {
			"retail": "live", "ptr": "ptr", "ptr2": "ptr2", "beta": "beta",
			"classic": "classic", "classic-ptr": "classic_ptr", "classic-beta": "classic_beta",
			"classic-era": "classic_era", "classic-era-ptr": "classic_era_ptr",
			"anniversary": "classic_anniversary", "titan": "classic_titan", "forever": "forever",
		},
		"elvui":       {"main": "main", "ptr": "ptr"},
		"weakauras":   {"main": "main"},
		"ndui":        {"main": "master", "classic": "Classic", "era": "Era", "anniversary": "Anniversary", "titan": "Titan"},
		"ellesmereui": {"main": "main"},
	}
	for _, repository := range repositories {
		tracks, ok := wantTracks[repository.Key]
		if !ok {
			t.Fatalf("unexpected repository %q", repository.Key)
		}
		if len(repository.Tracks) != len(tracks) {
			t.Fatalf("%s has %d tracks, want %d", repository.Key, len(repository.Tracks), len(tracks))
		}
		for product, branch := range tracks {
			if repository.Tracks[product] != branch {
				t.Errorf("%s track %q = %q, want %q", repository.Key, product, repository.Tracks[product], branch)
			}
		}
	}
}

func TestRepositoriesReturnDeepCopies(t *testing.T) {
	first := Repositories()
	first[0].Key = "changed"
	first[0].Tracks["retail"] = "changed"
	first[0].Tracks["injected"] = "changed"
	first = append(first, RepositorySpec{Key: "injected"})

	second := Repositories()
	if second[0].Key != "wow-ui-source" {
		t.Fatalf("repository key was mutated through returned slice: %q", second[0].Key)
	}
	if second[0].Tracks["retail"] != "live" {
		t.Fatalf("track was mutated through returned map: %q", second[0].Tracks["retail"])
	}
	if _, ok := second[0].Tracks["injected"]; ok {
		t.Fatal("injected track leaked into catalog")
	}
	if len(second) != 5 {
		t.Fatalf("appending to returned slice changed catalog length: %d", len(second))
	}

	lookup, err := LookupRepository("wow-ui-source")
	if err != nil {
		t.Fatal(err)
	}
	lookup.Tracks["retail"] = "changed"
	again, err := LookupRepository("wow-ui-source")
	if err != nil {
		t.Fatal(err)
	}
	if again.Tracks["retail"] != "live" {
		t.Fatal("lookup returned shared track map")
	}
}

func TestLookupRepositoryRequiresExactKey(t *testing.T) {
	for _, key := range []string{"missing", "WOW-UI-SOURCE", " wow-ui-source ", "World of Warcraft UI Source Mirror", ""} {
		if _, err := LookupRepository(key); err == nil {
			t.Errorf("LookupRepository(%q) unexpectedly succeeded", key)
		} else if !strings.Contains(err.Error(), "codebase.repository_not_found") {
			t.Errorf("LookupRepository(%q) error = %v, want repository_not_found", key, err)
		}
	}

	if _, err := LookupRepository("wow-ui-source"); err != nil {
		t.Fatalf("known repository lookup failed: %v", err)
	}
}
