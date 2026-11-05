package vault

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"testing"
)

func TestPruneMaxObjectsAlsoCapsUncommittedFiles(t *testing.T) {
	for _, ordinary := range []bool{false, true} {
		for _, dryRun := range []bool{false, true} {
			t.Run(fmt.Sprintf("ordinary=%v/dry=%v", ordinary, dryRun), func(t *testing.T) {
				ctx := context.Background()
				cache := newCacheWorkspace(t).Cache()
				if ordinary {
					putObject(t, cache, "ordinary", nil)
				}
				if err := os.MkdirAll(cache.stagingDir(), 0700); err != nil {
					t.Fatal(err)
				}
				for i := 0; i < 2; i++ {
					if err := os.WriteFile(filepath.Join(cache.stagingDir(), fmt.Sprintf("put-%d.part", i)), []byte("staged"), 0600); err != nil {
						t.Fatal(err)
					}
				}
				report, err := cache.Prune(ctx, PrunePolicy{TargetBytes: 0, MaxObjects: 1, IncludeUncommitted: true, DryRun: dryRun})
				if err != nil {
					t.Fatal(err)
				}
				if report.RemovedObjects+report.RemovedUncommitted != 1 || !report.Truncated || report.Complete {
					t.Fatalf("one-file bound not honored: %+v", report)
				}
				status, err := cache.Status(ctx)
				if err != nil {
					t.Fatal(err)
				}
				want := 2
				if !dryRun && !ordinary {
					want = 1
				}
				if status.UncommittedCount != want {
					t.Fatalf("staging count=%d want=%d", status.UncommittedCount, want)
				}
			})
		}
	}
}
