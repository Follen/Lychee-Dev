package delivery

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestStageIsIndependentAndPreservesParent(t *testing.T) {
	source, inventory := payloadFixture(t)
	parent := t.TempDir()
	marker := filepath.Join(parent, "unrelated")
	if err := os.WriteFile(marker, []byte("keep"), 0600); err != nil {
		t.Fatal(err)
	}
	stage, err := StagePayload(context.Background(), source, parent, inventory)
	if err != nil {
		t.Fatal(err)
	}
	// Canonicalize both sides: runner TMP can be an 8.3 short path while the
	// staging directory resolves to its long form.
	shortParent, err := filepath.EvalSymlinks(parent)
	if err != nil {
		t.Fatal(err)
	}
	shortProbe, err := filepath.EvalSymlinks(filepath.Dir(stage))
	if err != nil {
		t.Fatal(err)
	}
	if !strings.EqualFold(shortProbe, shortParent) {
		t.Fatalf("outside parent: %s", stage)
	}
	if err := os.WriteFile(filepath.Join(source, filepath.FromSlash(inventory[0].Path)), []byte("changed"), 0600); err != nil {
		t.Fatal(err)
	}
	if err := VerifyPayload(context.Background(), stage, inventory); err != nil {
		t.Fatal(err)
	}
	if data, err := os.ReadFile(marker); err != nil || string(data) != "keep" {
		t.Fatalf("parent changed: %q %v", data, err)
	}
}

// Deterministically cancel at the first cancellation checkpoint after staging
// starts, without sleeps or a scheduler-dependent goroutine.
type cancelAfterStage struct {
	context.Context
	parent string
	cancel context.CancelFunc
}

func (c cancelAfterStage) Err() error {
	entries, _ := os.ReadDir(c.parent)
	for _, entry := range entries {
		if strings.HasPrefix(entry.Name(), ".lycheedev-stage-") {
			c.cancel()
		}
	}
	return c.Context.Err()
}

func TestStageCancellationRemovesOnlyOwnedDirectory(t *testing.T) {
	source, inventory := payloadFixture(t)
	parent := t.TempDir()
	marker := filepath.Join(parent, "keep.txt")
	if err := os.WriteFile(marker, []byte("keep"), 0600); err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	stage, err := StagePayload(cancelAfterStage{ctx, parent, cancel}, source, parent, inventory)
	if stage != "" || !errors.Is(err, context.Canceled) {
		t.Fatalf("stage=%q err=%v", stage, err)
	}
	entries, err := os.ReadDir(parent)
	if err != nil || len(entries) != 1 || entries[0].Name() != "keep.txt" {
		t.Fatalf("cleanup: %v %v", entries, err)
	}
	if err := VerifyPayload(context.Background(), source, inventory); err != nil {
		t.Fatal(err)
	}
}

func TestStageFailureDoesNotCreateOutput(t *testing.T) {
	for _, cancelled := range []bool{false, true} {
		source, inventory := payloadFixture(t)
		parent := t.TempDir()
		ctx, cancel := context.WithCancel(context.Background())
		if cancelled {
			cancel()
		} else {
			inventory[0].Bytes++
		}
		stage, err := StagePayload(ctx, source, parent, inventory)
		cancel()
		if stage != "" || err == nil {
			t.Fatalf("stage=%q err=%v", stage, err)
		}
		if cancelled && !errors.Is(err, context.Canceled) {
			t.Fatal(err)
		}
		entries, err := os.ReadDir(parent)
		if err != nil || len(entries) != 0 {
			t.Fatalf("partial output: %v %v", entries, err)
		}
	}
}
