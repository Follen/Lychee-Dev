package main

import (
	"encoding/json"
	"testing"
)

func TestCompareRejectsUnpinnedOracleAndTailDifferences(t *testing.T) {
	a := map[string]any{"sourceCommit": "c0aeb5f6be0e9fbffc8eadb4f8b9b4e36b42af7c", "build": "12.1.0.69875", "db2SHA256": "db2", "dbdSHA256": "dbd", "hotfixSHA256": "", "fieldDigest": "full", "fieldBlocks": []any{"block-0", "block-1"}, "rows": []any{map[string]any{"Value": json.Number("18446744073709551615")}}}
	b := map[string]any{}
	for k, v := range a {
		if k != "sourceCommit" {
			b[k] = v
		}
	}
	if err := compare(a, b); err != nil {
		t.Fatal(err)
	}
	a["sourceCommit"] = "e732093f8864240fc5884bd1bba6b02f3dfc0d56"
	if err := compare(a, b); err == nil {
		t.Fatal("accepted stale oracle")
	}
	a["sourceCommit"] = "c0aeb5f6be0e9fbffc8eadb4f8b9b4e36b42af7c"
	b["fieldBlocks"] = []any{"block-0", "changed-after-row-256"}
	if err := compare(a, b); err == nil {
		t.Fatal("accepted changed unsampled block")
	}
	b["fieldBlocks"] = a["fieldBlocks"]
	b["rows"] = []any{map[string]any{"Value": json.Number("18446744073709551614")}}
	if err := compare(a, b); err == nil {
		t.Fatal("lost uint64 precision")
	}
	delete(a, "fieldDigest")
	if err := compare(a, b); err == nil {
		t.Fatal("accepted sampled-only oracle")
	}
}
