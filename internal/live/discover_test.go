package live

import (
	"context"
	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/live/journal"
	"path/filepath"
	"testing"
)

func TestPassiveDiscoveryKeepsDistinctWindowsAndProducts(t *testing.T) {
	game := t.TempDir()
	first := makeTestClient(t, game, "_classic_", "wow_classic", "5.5.4.69934")
	second := makeTestClient(t, game, "_retail_", "wow", "12.1.0.69875")
	io := nativeIO()
	a, b := testWindow(1, first), testWindow(2, first)
	io.list = func(context.Context) ([]desktop.WindowIdentity, error) {
		return []desktop.WindowIdentity{a, b, testWindow(3, second)}, nil
	}
	io.peek = func(context.Context, string, string) (journal.WindowOwner, bool, error) {
		return journal.WindowOwner{}, false, nil
	}
	result, err := discoverCandidates(context.Background(), "", DiscoveryRequest{Roots: []string{game, first, filepath.Join(first, ".")}}, io)
	if err != nil {
		t.Fatal(err)
	}
	if len(result.Candidates) != 3 || len(result.Installations) != 2 {
		t.Fatalf("dedup dropped windows or duplicated installation: %+v", result)
	}
	for _, candidate := range result.Candidates {
		if candidate.State != "unobserved" || candidate.InputReady || candidate.Capture != "" {
			t.Fatal("passive discovery invented readiness", candidate)
		}
	}
	filtered, err := discoverCandidates(context.Background(), "", DiscoveryRequest{PID: b.ProcessID, Installation: first}, io)
	if err != nil || len(filtered.Candidates) != 1 || filtered.Candidates[0].Window.Handle != b.Handle {
		t.Fatalf("selection %+v %v", filtered, err)
	}
}
