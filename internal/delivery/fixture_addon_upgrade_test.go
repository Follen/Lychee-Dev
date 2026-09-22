package delivery_test

import (
	"encoding/json"
	"github.com/follenfang/lycheedev/internal/delivery"
	"os"
	"path/filepath"
	"testing"
)

func addonUpgradeCommit(t *testing.T, releaseDirectory, commit string) error {
	t.Helper()
	path := filepath.Join(releaseDirectory, "release.json")
	raw, err := os.ReadFile(path)
	if err != nil {
		return err
	}
	var release delivery.Release
	if err := json.Unmarshal(raw, &release); err != nil {
		return err
	}
	release.Commit = commit
	raw, err = json.Marshal(release)
	if err != nil {
		return err
	}
	return os.WriteFile(path, raw, 0600)
}
