package command

import (
	"os"
	"testing"
)

func TestHotfixFullScanResumeKeepsCaptureAndFilters(t *testing.T) {
	root, pin, file, _ := hotfixCommandFixture(t)
	first, code := invoke(t, "data", "hotfix", "--scan", "--source", "dbcache", "--snapshot", pin, "--dbcache", file, "--limit", "1", "--max-pages", "1", "--home", root, "--format", "json")
	if code != 0 {
		t.Fatalf("%d %+v", code, first)
	}
	outer := first.Result.(map[string]any)
	state := outer["result"].(map[string]any)
	if state["complete"] != false || state["returned"] != float64(1) {
		t.Fatal(state)
	}
	cursor := outer["resume"].(string)
	derived := state["snapshot"].(string)
	source := state["source"].(map[string]any)["id"].(string)
	if err := os.WriteFile(file, []byte("changed"), 0600); err != nil {
		t.Fatal(err)
	}
	args := []string{"data", "hotfix", "--scan", "--source", "dbcache", "--snapshot", derived, "--from", source, "--cursor", cursor, "--limit", "1", "--home", root, "--format", "json"}
	changed, exit := invoke(t, append(append([]string{}, args...), "--record", "42")...)
	if exit == 0 {
		t.Fatal("changed filter accepted", changed)
	}
	done, exit := invoke(t, args...)
	if exit != 0 {
		t.Fatalf("%d %+v", exit, done)
	}
	final := done.Result.(map[string]any)["result"].(map[string]any)
	if final["complete"] != true || final["returned"] != float64(3) || len(final["pages"].([]any)) != 3 {
		t.Fatal(final)
	}
	// Replaying the same immutable checkpoint gives the same coverage, not duplicates.
	again, exit := invoke(t, args...)
	if exit != 0 || again.Result.(map[string]any)["result"].(map[string]any)["returned"] != float64(3) {
		t.Fatal(again)
	}
}
