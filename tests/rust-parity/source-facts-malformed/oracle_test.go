// This test is an explicit test-only overlay on fixed Go v3.2.3. It never
// replaces scanner production files or changes their expected observations.
package addoncheck

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"
	"runtime"
	"testing"
)

func TestFixedGoPublicFactsMalformedOracle(t *testing.T) {
	const commit = "3766f9cacb52e93a803a626187e2a1d54a445468"
	if runtime.Version() != "go1.27.1" {
		t.Fatalf("fixed Go1.27.1 required, got %s", runtime.Version())
	}
	sources := map[string]string{
		"analyze.go":     "88f6a291d14cad1b985e0d092d0b4cef7f3ee904b7a8445ae3f825003919a0cc",
		"facts.go":       "cb5e9fb10277f65e03c04363893f2f127bf3f3b2671f80577c5da9ad583e9caf",
		"lua.go":         "93a5002a06931c1ddd535f2277cdc4c2100b42b648f13ec37eeff6413a0cf60e",
		"xml.go":         "30474f75cb96a47d14bb7c53f441d817c9bf3c45e756aa820451a33d9faa0246",
		"toc.go":         "024edf6dcef4c338063232826ac5815d51a7548dae6c55915456de89849c07eb",
		"descriptors.go": "fb191f20a65572979ee3bab7d66551b5e182b69aa49baa5eb22e1787276a3231",
	}
	for path, expected := range sources {
		raw, err := os.ReadFile(path)
		if err != nil {
			t.Fatal(err)
		}
		hash := sha256.Sum256(raw)
		if hex.EncodeToString(hash[:]) != expected {
			t.Fatalf("fixed scanner bytes differ: %s", path)
		}
	}
	raw, err := os.ReadFile(os.Getenv("LYCHEEDEV_SOURCE_FACTS_CASES"))
	if err != nil {
		t.Fatal(err)
	}
	var input struct {
		Schema       string `json:"schema"`
		OracleCommit string `json:"oracleCommit"`
		Cases        []struct {
			ID     string `json:"id"`
			Path   string `json:"path"`
			Source string `json:"source"`
		} `json:"cases"`
	}
	if err := json.Unmarshal(raw, &input); err != nil {
		t.Fatal(err)
	}
	if input.Schema != "lycheedev.source-facts-malformed-cases.v1" || input.OracleCommit != commit || len(input.Cases) == 0 {
		t.Fatal("fixture identity")
	}
	observations := []map[string]any{}
	seen := map[string]bool{}
	for _, item := range input.Cases {
		if item.ID == "" || seen[item.ID] {
			t.Fatal("duplicate or empty case")
		}
		seen[item.ID] = true
		facts, failure := AnalyzeDocument(context.Background(), item.Path, []byte(item.Source))
		var failureText any
		if failure != nil {
			failureText = failure.Error()
		}
		row := map[string]any{"id": item.ID, "path": item.Path, "facts": facts, "error": failureText}
		encoded, err := json.Marshal(row)
		if err != nil {
			t.Fatal(err)
		}
		fmt.Printf("SOURCE_FACT_ORACLE_JSON %s\n", encoded)
		observations = append(observations, row)
	}
	hash := sha256.Sum256(raw)
	report := map[string]any{"schema": "lycheedev.source-facts-malformed-observations.v1", "oracleCommit": commit, "goVersion": runtime.Version(), "goos": runtime.GOOS, "casesSHA256": hex.EncodeToString(hash[:]), "observations": observations}
	encoded, err := json.MarshalIndent(report, "", "  ")
	if err != nil {
		t.Fatal(err)
	}
	path := os.Getenv("LYCHEEDEV_SOURCE_FACTS_REPORT")
	if path == "" {
		t.Fatal("owned output path required")
	}
	file, err := os.OpenFile(path, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0600)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := file.Write(append(encoded, '\n')); err != nil {
		file.Close()
		t.Fatal(err)
	}
	if err := file.Close(); err != nil {
		t.Fatal(err)
	}
}
