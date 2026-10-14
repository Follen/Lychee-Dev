package vault

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"testing"
)

// The existing stopAfter="move" seam records the move before stopping. This
// reconstructs the different crash window between the rename and that record.
func TestArchiveRecoveryAfterUnrecordedRename(t *testing.T) {
	for _, unavailable := range []bool{false, true} {
		name := "archive_present"
		if unavailable {
			name = "archive_temporarily_missing"
		}
		t.Run(name, func(t *testing.T) {
			ctx := context.Background()
			root := filepath.Join(t.TempDir(), "workspace")
			want := writeSentinel(t, filepath.Join(root, "original.bin"), "original user data\x00\xff\n")
			plan, err := PlanFreshInitialize(ctx, root)
			if err != nil {
				t.Fatal(err)
			}
			if _, err := runArchive(ctx, plan, "journal"); !errors.Is(err, errArchiveInterrupt) {
				t.Fatal(err)
			}
			if err := os.Rename(root, plan.ArchivePath); err != nil {
				t.Fatal(err)
			}
			before := digestOf(t, plan.JournalPath)
			if unavailable {
				holding := plan.ArchivePath + "-unavailable"
				if err := os.Rename(plan.ArchivePath, holding); err != nil {
					t.Fatal(err)
				}
				if _, err := CompleteArchive(ctx, root); !errors.Is(err, ErrArchiveState) {
					t.Fatalf("missing recovery data accepted: %v", err)
				}
				if _, err := os.Lstat(root); !errors.Is(err, os.ErrNotExist) {
					t.Fatalf("invented workspace after missing archive: %v", err)
				}
				if got := digestOf(t, plan.JournalPath); got != before {
					t.Fatal("failed recovery changed original journal")
				}
				if got := digestOf(t, filepath.Join(holding, "original.bin")); got != want {
					t.Fatal("failed recovery changed legacy bytes")
				}
				if err := os.Rename(holding, plan.ArchivePath); err != nil {
					t.Fatal(err)
				}
			}
			result, err := CompleteArchive(ctx, root)
			if err != nil || result.Plan.State != "complete" || result.Plan.ArchivePath != plan.ArchivePath {
				t.Fatalf("resume: %+v %v", result, err)
			}
			if got := digestOf(t, filepath.Join(plan.ArchivePath, "original.bin")); got != want {
				t.Fatal("recovery modified archive bytes")
			}
			if _, err := os.Lstat(filepath.Join(root, "original.bin")); !errors.Is(err, os.ErrNotExist) {
				t.Fatalf("legacy data imported into new workspace: %v", err)
			}
			again, err := CompleteArchive(ctx, root)
			if err != nil || again.Summary.Identity != result.Summary.Identity || again.Plan.ArchivePath != plan.ArchivePath {
				t.Fatalf("repeat recovery replaced workspace identity: %+v %v", again, err)
			}
		})
	}
}
