package codebase

import (
	"context"
	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/testkit"
	"os"
	"path/filepath"
	"testing"
)

type gitFixtureIndex struct {
	Pin      selection.SourcePin
	Snapshot selection.PinnedSet
	Summary  IndexSummary
}

func indexGitFixture(t testing.TB, ctx context.Context, root, repository, product, sourcePath string) (gitFixtureIndex, error) {
	t.Helper()
	repo, err := LookupRepository(repository)
	if err != nil {
		return gitFixtureIndex{}, err
	}
	commit := testkit.GitSourceFixture(t, ctx, root, repository, repo.URL, sourcePath)
	snapshot, err := SynchronizeSource(ctx, root, repository, product, commit)
	if err != nil {
		return gitFixtureIndex{}, err
	}
	summary, err := BuildSourceIndex(ctx, root, snapshot.ID)
	return gitFixtureIndex{Pin: *snapshot.Source, Snapshot: snapshot, Summary: summary}, err
}

func TestGitFixturePinsKeepOriginalBytesAfterSourceChanges(t *testing.T) {
	ctx := context.Background()
	home, _, _ := workspaceFixture(t)
	source := t.TempDir()
	file := filepath.Join(source, "Addon.lua")
	if err := os.WriteFile(file, []byte("function Original() end\n"), 0600); err != nil {
		t.Fatal(err)
	}
	first, err := indexGitFixture(t, ctx, home, "wow-ui-source", "retail", source)
	if err != nil {
		t.Fatal(err)
	}
	repeat, err := indexGitFixture(t, ctx, home, "wow-ui-source", "retail", source)
	if err != nil {
		t.Fatal(err)
	}
	if repeat.Pin.ExactCommit != first.Pin.ExactCommit || repeat.Snapshot.ID != first.Snapshot.ID {
		t.Fatal("identical source produced different Git identity")
	}
	if err := os.WriteFile(file, []byte("function Changed() end\n"), 0600); err != nil {
		t.Fatal(err)
	}
	next, err := indexGitFixture(t, ctx, home, "wow-ui-source", "retail", source)
	if err != nil {
		t.Fatal(err)
	}
	if next.Pin.ExactCommit == first.Pin.ExactCommit {
		t.Fatal("different bytes share identity")
	}
	for _, tc := range []struct{ snapshot, symbol string }{{first.Snapshot.ID, "Original"}, {next.Snapshot.ID, "Changed"}} {
		reading, err := QuerySource(ctx, home, tc.snapshot, SearchQuery{Mode: SearchModePrecise, Text: tc.symbol})
		if err != nil || len(reading.Result.Results) == 0 {
			t.Fatalf("%s: %+v %v", tc.symbol, reading.Result, err)
		}
	}
}
