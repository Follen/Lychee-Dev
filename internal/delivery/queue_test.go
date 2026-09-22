package delivery

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/vault"
)

func queueStoreFixture(t *testing.T) string {
	t.Helper()
	root := filepath.Join(t.TempDir(), "Lychee Dev")
	if err := os.MkdirAll(filepath.Join(root, "Bridge"), 0700); err != nil {
		t.Fatal(err)
	}
	data, _ := bridge.EncodeProbeQueue(nil)
	if err := os.WriteFile(filepath.Join(root, "Bridge", "Definitions.lua"), data, 0600); err != nil {
		t.Fatal(err)
	}
	base := []byte("local _, ns = ...\n")
	if err := os.WriteFile(filepath.Join(root, "Runtime.lua"), base, 0600); err != nil {
		t.Fatal(err)
	}
	receipt := InstallationReceipt{Schema: "lycheedev.installation.v1", Component: "addon", Version: "2.0.0-dev", Commit: strings.Repeat("a", 40), Resources: []Resource{
		{Path: "addon/Bridge/Definitions.lua", Bytes: int64(len(data)), SHA256: fmt.Sprintf("%x", sha256.Sum256(data))},
		{Path: "addon/Runtime.lua", Bytes: int64(len(base)), SHA256: fmt.Sprintf("%x", sha256.Sum256(base))},
	}}
	raw, err := json.Marshal(receipt)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(root, installationMarker), raw, 0600); err != nil {
		t.Fatal(err)
	}
	return root
}
func queueFixture(id string) bridge.ProbeDefinition {
	return bridge.ProbeDefinition{RequestID: id, Release: "2.0.0-dev", SessionNonce: strings.Repeat("a", 32), ReloadNonce: strings.Repeat("b", 32), Character: "Paladin", Realm: "Realm", GUID: "Player-1-123", Product: "retail", Build: "12.1.0.12345", Code: "return 42"}
}
func readQueueFixture(t *testing.T, root string) ([]bridge.ProbeDefinition, []byte) {
	t.Helper()
	data, err := os.ReadFile(filepath.Join(root, "Bridge", "Definitions.lua"))
	if err != nil {
		t.Fatal(err)
	}
	defs, err := bridge.DecodeProbeQueue(bytes.NewReader(data))
	if err != nil {
		t.Fatal(err)
	}
	return defs, data
}
func TestQueueStoreMergeAndExactRemoval(t *testing.T) {
	root := queueStoreFixture(t)
	a, b := queueFixture("OP-a"), queueFixture("OP-b")
	for _, d := range []bridge.ProbeDefinition{a, b} {
		r, err := ChangeProbeQueue(context.Background(), root, d, false)
		if err != nil || !r.Changed {
			t.Fatalf("%+v %v", r, err)
		}
	}
	r, err := ChangeProbeQueue(context.Background(), root, a, false)
	if err != nil || r.Changed || r.Entries != 2 {
		t.Fatalf("%+v %v", r, err)
	}
	_, before := readQueueFixture(t, root)
	altered := a
	altered.Code = "return 'different'"
	for _, retire := range []bool{false, true} {
		if _, err := ChangeProbeQueue(context.Background(), root, altered, retire); !errors.Is(err, ErrQueueConflict) {
			t.Fatalf("conflict: %v", err)
		}
	}
	_, after := readQueueFixture(t, root)
	if !bytes.Equal(before, after) {
		t.Fatal("conflict changed queue")
	}
	r, err = ChangeProbeQueue(context.Background(), root, a, true)
	if err != nil || !r.Changed || r.Entries != 1 {
		t.Fatalf("%+v %v", r, err)
	}
	defs, _ := readQueueFixture(t, root)
	if len(defs) != 1 || defs[0] != b {
		t.Fatal("foreign request changed")
	}
	r, err = ChangeProbeQueue(context.Background(), root, a, true)
	if err != nil || r.Changed {
		t.Fatalf("retirement retry: %+v %v", r, err)
	}
	entries, err := os.ReadDir(filepath.Join(root, "Bridge"))
	if err != nil {
		t.Fatal(err)
	}
	if len(entries) != 1 || entries[0].Name() != "Definitions.lua" {
		t.Fatal("temporary payload leaked")
	}
}
func TestQueueStoreSharedInstallationLease(t *testing.T) {
	root := queueStoreFixture(t)
	parent, err := filepath.EvalSymlinks(filepath.Dir(root))
	if err != nil {
		t.Fatal(err)
	}
	target := filepath.Join(parent, filepath.Base(root))
	lock, err := vault.AcquireLease(context.Background(), filepath.Join(parent, ".lycheedev-locks"), "installation:"+strings.ToLower(target))
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 100*time.Millisecond)
	defer cancel()
	_, err = ChangeProbeQueue(ctx, root, queueFixture("OP-held"), false)
	if !errors.Is(err, context.DeadlineExceeded) {
		t.Fatalf("installation lock bypassed: %v", err)
	}
	if err := lock.Close(); err != nil {
		t.Fatal(err)
	}
	defs, _ := readQueueFixture(t, root)
	if len(defs) != 0 {
		t.Fatal("cancelled writer changed queue")
	}
	if _, err := ChangeProbeQueue(context.Background(), root, queueFixture("OP-held"), false); err != nil {
		t.Fatal(err)
	}
}

func TestQueueDefinitionCannotBeReplaced(t *testing.T) {
	ctx := context.Background()
	root := queueStoreFixture(t)
	a, b := queueFixture("OP-a"), queueFixture("OP-b")
	for _, d := range []bridge.ProbeDefinition{a, b} {
		if _, err := ChangeProbeQueue(ctx, root, d, false); err != nil {
			t.Fatal(err)
		}
	}
	for i := 0; i < 2; i++ {
		r, err := ChangeProbeQueue(ctx, root, a, false)
		if err != nil || r.Changed || r.Entries != 2 {
			t.Fatalf("transition %d: %+v %v", i, r, err)
		}
	}
	defs, before := readQueueFixture(t, root)
	if len(defs) != 2 || defs[0] != a || defs[1] != b {
		t.Fatal("wrong phase or foreign request changed")
	}
	changed := a
	changed.Code += " -- changed"
	for _, retire := range []bool{false, true} {
		if _, err := ChangeProbeQueue(ctx, root, changed, retire); !errors.Is(err, ErrQueueConflict) {
			t.Fatalf("stale probe: %v", err)
		}
	}
	_, after := readQueueFixture(t, root)
	if !bytes.Equal(before, after) {
		t.Fatal("conflict changed queue")
	}
	if _, err := ChangeProbeQueue(ctx, root, a, true); err != nil {
		t.Fatal(err)
	}
	defs, _ = readQueueFixture(t, root)
	if len(defs) != 1 || defs[0] != b {
		t.Fatal("retirement changed foreign request")
	}
}
func TestVerifyProbePreparedRejectsMissingWithoutChangingQueue(t *testing.T) {
	root := queueStoreFixture(t)
	present := queueFixture("OP-present")
	missing := queueFixture("OP-missing")
	if _, err := ChangeProbeQueue(context.Background(), root, present, false); err != nil {
		t.Fatal(err)
	}
	_, before := readQueueFixture(t, root)

	if _, err := VerifyProbePrepared(context.Background(), root, missing); !errors.Is(err, ErrQueueConflict) {
		t.Fatalf("missing definition accepted: %v", err)
	}
	_, after := readQueueFixture(t, root)
	if !bytes.Equal(before, after) {
		t.Fatal("missing definition changed queue")
	}
}

func TestVerifyProbePreparedMatchesWithoutChangingQueue(t *testing.T) {
	root := queueStoreFixture(t)
	definition := queueFixture("OP-match")
	if _, err := ChangeProbeQueue(context.Background(), root, definition, false); err != nil {
		t.Fatal(err)
	}
	_, before := readQueueFixture(t, root)

	revision, err := VerifyProbePrepared(context.Background(), root, definition)
	if err != nil {
		t.Fatal(err)
	}
	if revision.Entries != 1 || revision.Changed {
		t.Fatalf("unexpected verification revision: %+v", revision)
	}
	_, after := readQueueFixture(t, root)
	if !bytes.Equal(before, after) {
		t.Fatal("matching verification changed queue")
	}
}

func TestVerifyProbePreparedRejectsConflictingDefinition(t *testing.T) {
	root := queueStoreFixture(t)
	definition := queueFixture("OP-conflict")
	if _, err := ChangeProbeQueue(context.Background(), root, definition, false); err != nil {
		t.Fatal(err)
	}
	_, before := readQueueFixture(t, root)
	conflicting := definition
	conflicting.Code = "return 'different'"

	if _, err := VerifyProbePrepared(context.Background(), root, conflicting); !errors.Is(err, ErrQueueConflict) {
		t.Fatalf("conflicting definition accepted: %v", err)
	}
	_, after := readQueueFixture(t, root)
	if !bytes.Equal(before, after) {
		t.Fatal("conflicting verification changed queue")
	}
}

func TestVerifyProbePreparedRetainsOtherRequests(t *testing.T) {
	root := queueStoreFixture(t)
	definition, other := queueFixture("OP-verify"), queueFixture("OP-other")
	for _, entry := range []bridge.ProbeDefinition{definition, other} {
		if _, err := ChangeProbeQueue(context.Background(), root, entry, false); err != nil {
			t.Fatal(err)
		}
	}
	_, before := readQueueFixture(t, root)

	if _, err := VerifyProbePrepared(context.Background(), root, definition); err != nil {
		t.Fatal(err)
	}
	defs, after := readQueueFixture(t, root)
	if !bytes.Equal(before, after) {
		t.Fatal("verification changed queue bytes")
	}
	if len(defs) != 2 || !((defs[0] == definition && defs[1] == other) || (defs[0] == other && defs[1] == definition)) {
		t.Fatalf("other request was not retained: %+v", defs)
	}
}

func TestQueueStoreRejectsUnownedBytes(t *testing.T) {
	root := queueStoreFixture(t)
	path := filepath.Join(root, "Bridge", "Definitions.lua")
	bad := []byte("-- user-owned file\nreturn {}\n")
	if err := os.WriteFile(path, bad, 0600); err != nil {
		t.Fatal(err)
	}
	if _, err := ChangeProbeQueue(context.Background(), root, queueFixture("OP-a"), false); err == nil {
		t.Fatal("noncanonical file replaced")
	}
	after, err := os.ReadFile(path)
	if err != nil || !bytes.Equal(after, bad) {
		t.Fatal("user bytes changed")
	}
}

func TestQueueStoreCapacityAndMissingFilePreserveState(t *testing.T) {
	root := queueStoreFixture(t)
	for i := 0; i < 16; i++ {
		if _, err := ChangeProbeQueue(context.Background(), root, queueFixture(fmt.Sprintf("OP-%02d", i)), false); err != nil {
			t.Fatal(err)
		}
	}
	_, before := readQueueFixture(t, root)
	if _, err := ChangeProbeQueue(context.Background(), root, queueFixture("OP-extra"), false); err == nil {
		t.Fatal("capacity exceeded")
	}
	_, after := readQueueFixture(t, root)
	if !bytes.Equal(before, after) {
		t.Fatal("capacity failure altered queue")
	}
	path := filepath.Join(root, "Bridge", "Definitions.lua")
	if err := os.Remove(path); err != nil {
		t.Fatal(err)
	}
	if _, err := ChangeProbeQueue(context.Background(), root, queueFixture("OP-extra"), false); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("missing queue adopted: %v", err)
	}
	if _, err := os.Stat(path); !errors.Is(err, os.ErrNotExist) {
		t.Fatal("missing queue recreated")
	}
}

func TestQueueStoreRequiresOwnedUnmodifiedInstallation(t *testing.T) {
	for _, variant := range []string{"no-receipt", "invalid-receipt", "edited-code", "extra-file", "wrong-version", "queue-baseline", "foreign-release"} {
		t.Run(variant, func(t *testing.T) {
			root := queueStoreFixture(t)
			marker := filepath.Join(root, installationMarker)
			definition := queueFixture("OP-a")
			switch variant {
			case "no-receipt":
				if err := os.Remove(marker); err != nil {
					t.Fatal(err)
				}
			case "invalid-receipt":
				if err := os.WriteFile(marker, []byte("{}"), 0600); err != nil {
					t.Fatal(err)
				}
			case "edited-code", "extra-file":
				name := "Runtime.lua"
				if variant == "extra-file" {
					name = "Foreign.lua"
				}
				if err := os.WriteFile(filepath.Join(root, name), []byte("-- user bytes\n"), 0600); err != nil {
					t.Fatal(err)
				}
			case "wrong-version":
				definition.Release = "9.0.0"
			case "queue-baseline":
				raw, err := os.ReadFile(marker)
				if err != nil {
					t.Fatal(err)
				}
				var receipt InstallationReceipt
				if err := json.Unmarshal(raw, &receipt); err != nil {
					t.Fatal(err)
				}
				receipt.Resources[0].SHA256 = strings.Repeat("a", 64)
				raw, err = json.Marshal(receipt)
				if err != nil {
					t.Fatal(err)
				}
				if err := os.WriteFile(marker, raw, 0600); err != nil {
					t.Fatal(err)
				}
			case "foreign-release":
				foreign := queueFixture("OP-foreign")
				foreign.Release = "9.0.0"
				data, err := bridge.EncodeProbeQueue([]bridge.ProbeDefinition{foreign})
				if err != nil {
					t.Fatal(err)
				}
				if err := os.WriteFile(filepath.Join(root, "Bridge", "Definitions.lua"), data, 0600); err != nil {
					t.Fatal(err)
				}
			}
			_, before := readQueueFixture(t, root)
			if _, err := ChangeProbeQueue(context.Background(), root, definition, false); err == nil {
				t.Fatal("unsafe installation accepted")
			}
			_, after := readQueueFixture(t, root)
			if !bytes.Equal(before, after) {
				t.Fatal("refused change altered queue")
			}
		})
	}
}

func TestActiveQueueDoesNotRelaxRemoval(t *testing.T) {
	root := queueStoreFixture(t)
	ctx := context.Background()
	definition := queueFixture("OP-active")
	if _, err := ChangeProbeQueue(ctx, root, definition, false); err != nil {
		t.Fatal(err)
	}
	assessment, err := InspectInstallation(ctx, root, "addon")
	if err != nil || assessment.State != "modified" {
		t.Fatalf("ordinary installation gate relaxed: %+v %v", assessment, err)
	}
	archive := filepath.Join(t.TempDir(), "removed")
	if _, err := RemoveInstallation(ctx, root, archive, "addon"); !errors.Is(err, ErrConflict) {
		t.Fatalf("active queue removed: %v", err)
	}
	if _, err := os.Stat(archive); !errors.Is(err, os.ErrNotExist) {
		t.Fatal("refused removal created archive")
	}
	if _, err := ChangeProbeQueue(ctx, root, definition, true); err != nil {
		t.Fatal(err)
	}
	assessment, err = InspectInstallation(ctx, root, "addon")
	if err != nil || assessment.State != "managed" {
		t.Fatalf("exact cleanup did not restore baseline: %+v %v", assessment, err)
	}
	if _, err := RemoveInstallation(ctx, root, archive, "addon"); err != nil {
		t.Fatal(err)
	}
}
func TestQueueStoreChild(t *testing.T) {
	root := os.Getenv("LYCHEEDEV_QUEUE_TEST_ROOT")
	if root == "" {
		return
	}
	id := os.Getenv("LYCHEEDEV_QUEUE_TEST_ID")
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	if _, err := ChangeProbeQueue(ctx, root, queueFixture(id), false); err != nil {
		t.Fatal(err)
	}
}
func TestQueueStoreAcrossProcesses(t *testing.T) {
	root := queueStoreFixture(t)
	var wg sync.WaitGroup
	failures := make(chan string, 8)
	for i := 0; i < 8; i++ {
		wg.Add(1)
		go func(index int) {
			defer wg.Done()
			ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
			defer cancel()
			cmd := exec.CommandContext(ctx, os.Args[0], "-test.run=^TestQueueStoreChild$")
			cmd.Env = append(os.Environ(), "LYCHEEDEV_QUEUE_TEST_ROOT="+root, fmt.Sprintf("LYCHEEDEV_QUEUE_TEST_ID=OP-%d", index))
			if output, err := cmd.CombinedOutput(); err != nil {
				failures <- fmt.Sprintf("%v: %s", err, output)
			}
		}(i)
	}
	wg.Wait()
	close(failures)
	for failure := range failures {
		t.Error(failure)
	}
	defs, _ := readQueueFixture(t, root)
	if len(defs) != 8 {
		t.Fatalf("lost requests: %d", len(defs))
	}
	for i, d := range defs {
		if d != queueFixture(fmt.Sprintf("OP-%d", i)) {
			t.Fatal("request changed")
		}
	}
}
