//go:build casc_oracle

package main

import (
	"os"
	"path/filepath"
	"testing"
)

func TestManifestJSONRejectsUnknownTrailingAndOversized(t *testing.T) {
	for _, raw := range []string{`{"unexpected":true}`, `{} {}`, string(make([]byte, (1<<20)+1))} {
		path := filepath.Join(t.TempDir(), "case.json")
		if err := os.WriteFile(path, []byte(raw), 0600); err != nil {
			t.Fatal(err)
		}
		var m manifest
		if err := readJSON(path, &m); err == nil {
			t.Fatalf("invalid manifest accepted: %d bytes", len(raw))
		}
	}
}

func TestComparisonRequiresExplicitFiles(t *testing.T) {
	if err := compare("", "", ""); err == nil {
		t.Fatal("missing required manifest/oracle/output accepted")
	}
}
