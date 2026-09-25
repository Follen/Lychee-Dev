package live

import (
	"context"
	"errors"
	"testing"

	"github.com/follenfang/lycheedev/internal/vault"
)

func TestProbeRegistryKeepsImmutableRevisions(t *testing.T) {
	ctx := context.Background()
	root := t.TempDir() + "/workspace"
	if _, err := vault.Initialize(ctx, root); err != nil {
		t.Fatal(err)
	}
	first, err := PutProbe(ctx, root, "frames", []byte("return 1"))
	if err != nil || !first.Changed || first.Name.Revision != first.Revision.ID {
		t.Fatalf("first put = %+v, %v", first, err)
	}
	repeated, err := PutProbe(ctx, root, "frames", []byte("return 1"))
	if err != nil || repeated.Changed || repeated.Revision.ID != first.Revision.ID {
		t.Fatalf("idempotent put = %+v, %v", repeated, err)
	}
	second, err := PutProbe(ctx, root, "frames", []byte("return 2"))
	if err != nil || !second.Changed || second.Revision.ID == first.Revision.ID {
		t.Fatalf("second put = %+v, %v", second, err)
	}
	old, err := ShowProbe(ctx, root, first.Revision.ID)
	if err != nil || string(old.Revision.Code) != "return 1" {
		t.Fatalf("old revision = %+v, %v", old, err)
	}
	current, err := ShowProbe(ctx, root, "frames")
	if err != nil || current.Revision.ID != second.Revision.ID {
		t.Fatalf("current name = %+v, %v", current, err)
	}
	removed, err := RemoveProbe(ctx, root, "frames")
	if err != nil || removed.Active {
		t.Fatalf("remove = %+v, %v", removed, err)
	}
	if _, err := ShowProbe(ctx, root, "frames"); err == nil || err.Error() != "live.probe_removed" {
		t.Fatalf("removed name remained selectable: %v", err)
	}
	if old, err := ShowProbe(ctx, root, first.Revision.ID); err != nil || string(old.Revision.Code) != "return 1" {
		t.Fatalf("remove deleted immutable revision: %+v %v", old, err)
	}
	list, err := ListProbes(ctx, root, false, 100)
	if err != nil || len(list.Items) != 0 {
		t.Fatalf("active list = %+v, %v", list, err)
	}
	list, err = ListProbes(ctx, root, true, 100)
	if err != nil || len(list.Items) != 1 || list.Items[0].Active {
		t.Fatalf("removed list = %+v, %v", list, err)
	}
}

func TestProbeRegistryRejectsInvalidAndCorruptSelectors(t *testing.T) {
	ctx := context.Background()
	root := t.TempDir() + "/workspace"
	if _, err := vault.Initialize(ctx, root); err != nil {
		t.Fatal(err)
	}
	if _, err := PutProbe(ctx, root, "bad name", []byte("return true")); err == nil {
		t.Fatal("accepted invalid name")
	}
	if _, err := PutProbe(ctx, root, "empty", nil); err == nil {
		t.Fatal("accepted empty code")
	}
	if _, err := ShowProbe(ctx, root, "missing"); !errors.Is(err, vault.ErrMissingRecord) {
		t.Fatalf("missing selector = %v", err)
	}
}
