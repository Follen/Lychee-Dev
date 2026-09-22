package journal

import (
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

	"github.com/follenfang/lycheedev/internal/vault"
)

func windowBook(t *testing.T) (*Book, *vault.Store) {
	t.Helper()
	store, err := vault.Initialize(context.Background(), filepath.Join(t.TempDir(), "home"))
	if err != nil {
		t.Fatal(err)
	}
	metadata, err := store.OpenMetadata(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { metadata.Close() })
	return OpenBook(metadata), store
}

func windowIntent(resource string) WorkIntent {
	return WorkIntent{Kind: "probe", Resource: resource, Snapshot: "PIN-test", Session: "session", Request: json.RawMessage(`{}`)}
}

func TestWindowReservationAcrossWorkspaces(t *testing.T) {
	ctx := context.Background()
	parent := t.TempDir()
	first, firstStore := windowBook(t)
	second, secondStore := windowBook(t)
	intent := windowIntent("window/1/2/3")
	record, err := first.BeginWindowWork(ctx, parent, firstStore.Identity().WorkspaceID, intent)
	if err != nil {
		t.Fatal(err)
	}
	for _, contender := range []struct {
		book    *Book
		store   *vault.Store
		foreign bool
	}{{first, firstStore, false}, {second, secondStore, true}} {
		_, err := contender.book.BeginWindowWork(ctx, parent, contender.store.Identity().WorkspaceID, intent)
		var occupied *WindowOccupied
		if !errors.As(err, &occupied) || !errors.Is(err, ErrBusy) || occupied.Foreign != contender.foreign || occupied.Owner.OperationID != record.OperationID || occupied.Owner.WorkspaceID != firstStore.Identity().WorkspaceID {
			t.Fatalf("owner: %v", err)
		}
	}
	if _, err := second.BeginWindowWork(ctx, parent, secondStore.Identity().WorkspaceID, windowIntent("window/1/2/4")); err != nil {
		t.Fatal("different window blocked", err)
	}
	// Querying local work does not need the shared admission lock.
	if _, err := first.InspectWork(ctx, record.OperationID); err != nil {
		t.Fatal(err)
	}
}

func TestIncompleteWindowMarkerCannotBeTakenOver(t *testing.T) {
	book, store := windowBook(t)
	parent := t.TempDir()
	scope := filepath.Join(parent, ".lycheedev-window-owners")
	if err := os.Mkdir(scope, 0700); err != nil {
		t.Fatal(err)
	}
	intent := windowIntent("window/1/2/3")
	marker := filepath.Join(scope, fmt.Sprintf("%x.json", sha256.Sum256([]byte(intent.Resource))))
	if err := os.WriteFile(marker, []byte(`{"schema":`), 0600); err != nil {
		t.Fatal(err)
	}
	if _, err := book.BeginWindowWork(context.Background(), parent, store.Identity().WorkspaceID, intent); err == nil {
		t.Fatal("incomplete claim ignored")
	}
	data, err := os.ReadFile(marker)
	if err != nil || string(data) != `{"schema":` {
		t.Fatal("incomplete marker overwritten")
	}
}

func TestWindowOwnerReleaseRequiresCleanedOwner(t *testing.T) {
	ctx := context.Background()
	parent := t.TempDir()
	book, store := windowBook(t)
	other, otherStore := windowBook(t)
	intent := windowIntent("window/1/2/3")
	record, err := book.BeginWindowWork(ctx, parent, store.Identity().WorkspaceID, intent)
	if err != nil {
		t.Fatal(err)
	}
	if err := book.RetireWindowWork(ctx, parent, store.Identity().WorkspaceID, record.OperationID); !errors.Is(err, ErrTransition) {
		t.Fatal("released unfinished owner", err)
	}
	// State-machine fixture only; this does not assert real game cleanup.
	for _, stage := range []string{"load_requested", "loaded", "dispatch_requested", "reported", "flush_requested", "persisted", "verified", "ack_requested", "acknowledged", "cleaned"} {
		status := "running"
		if stage == "cleaned" {
			status = "completed"
		}
		if err := book.AdvanceStage(ctx, StageChange{OperationID: record.OperationID, ExpectedGeneration: record.Generation, ExpectedStage: record.Stage, Stage: stage, Status: status, Observation: json.RawMessage(`null`)}); err != nil {
			t.Fatal(err)
		}
		record, err = book.InspectWork(ctx, record.OperationID)
		if err != nil {
			t.Fatal(err)
		}
	}
	if err := book.RetireWindowWork(ctx, parent, otherStore.Identity().WorkspaceID, record.OperationID); !errors.Is(err, ErrBusy) {
		t.Fatal("foreign workspace released owner", err)
	}
	for i := 0; i < 2; i++ {
		if err := book.RetireWindowWork(ctx, parent, store.Identity().WorkspaceID, record.OperationID); err != nil {
			t.Fatal(err)
		}
	}
	if _, err := book.InspectWork(ctx, record.OperationID); err != nil {
		t.Fatal("release deleted audit record", err)
	}
	if _, err := other.BeginWindowWork(ctx, parent, otherStore.Identity().WorkspaceID, intent); err != nil {
		t.Fatal("completed release did not unblock", err)
	}
	if err := book.RetireWindowWork(ctx, parent, store.Identity().WorkspaceID, record.OperationID); !errors.Is(err, ErrBusy) {
		t.Fatal("old cleanup removed new owner", err)
	}
}

func TestWindowOwnerBoundsAndCancellation(t *testing.T) {
	book, store := windowBook(t)
	parent := t.TempDir()
	cancelled, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err := book.BeginWindowWork(cancelled, parent, store.Identity().WorkspaceID, windowIntent("window/1/2/3")); !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
	scope := filepath.Join(parent, ".lycheedev-window-owners")
	if _, err := os.Stat(scope); !errors.Is(err, os.ErrNotExist) {
		t.Fatal("cancelled admission created scope", err)
	}
	if err := os.Mkdir(scope, 0700); err != nil {
		t.Fatal(err)
	}
	for i := 0; i < 256; i++ {
		if err := os.WriteFile(filepath.Join(scope, fmt.Sprintf("fixture-%d.json", i)), []byte(`{}`), 0600); err != nil {
			t.Fatal(err)
		}
	}
	if _, err := book.BeginWindowWork(context.Background(), parent, store.Identity().WorkspaceID, windowIntent("window/1/2/3")); err == nil || err.Error() != "journal.window_owner_limit" {
		t.Fatal("unbounded owner admission", err)
	}
	docs, err := book.metadata.ListDocuments(context.Background(), "work/", "", 10)
	if err != nil || len(docs) != 0 {
		t.Fatal("failed admission published work", err)
	}
}

func TestWindowOwnerProcessHelper(t *testing.T) {
	root := os.Getenv("LYCHEEDEV_OWNER_TEST_HOME")
	if root == "" {
		t.Skip("subprocess helper")
	}
	store, err := vault.OpenStore(root)
	if err != nil {
		t.Fatal(err)
	}
	metadata, err := store.OpenMetadata(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	defer metadata.Close()
	record, err := OpenBook(metadata).BeginWindowWork(context.Background(), os.Getenv("LYCHEEDEV_OWNER_TEST_PARENT"), store.Identity().WorkspaceID, windowIntent("window/7/8/9"))
	var occupied *WindowOccupied
	result := struct {
		Operation string
		Foreign   bool
		Failure   string
	}{Operation: record.OperationID}
	if errors.As(err, &occupied) {
		result.Foreign, result.Operation = occupied.Foreign, occupied.Owner.OperationID
	} else if err != nil {
		result.Failure = err.Error()
	}
	data, _ := json.Marshal(result)
	fmt.Println("WINDOW-OWNER " + string(data))
}

func TestWindowOwnerSeparateProcesses(t *testing.T) {
	parent := t.TempDir()
	_, first := windowBook(t)
	_, second := windowBook(t)
	var wg sync.WaitGroup
	outputs := make(chan []byte, 2)
	for _, store := range []*vault.Store{first, second} {
		wg.Add(1)
		go func(root string) {
			defer wg.Done()
			cmd := exec.Command(os.Args[0], "-test.run=^TestWindowOwnerProcessHelper$")
			cmd.Env = append(os.Environ(), "LYCHEEDEV_OWNER_TEST_HOME="+root, "LYCHEEDEV_OWNER_TEST_PARENT="+parent)
			output, err := cmd.CombinedOutput()
			if err != nil {
				t.Errorf("child: %v %s", err, output)
			}
			outputs <- output
		}(store.Root())
	}
	wg.Wait()
	close(outputs)
	winner, foreign, operation := 0, 0, ""
	for output := range outputs {
		found := false
		for _, line := range strings.Split(string(output), "\n") {
			if !strings.HasPrefix(line, "WINDOW-OWNER ") {
				continue
			}
			found = true
			var result struct {
				Operation string
				Foreign   bool
				Failure   string
			}
			if err := json.Unmarshal([]byte(strings.TrimPrefix(line, "WINDOW-OWNER ")), &result); err != nil || result.Failure != "" || result.Operation == "" {
				t.Fatalf("child result: %s", line)
			}
			if result.Foreign {
				foreign++
			} else {
				winner++
			}
			if operation != "" && operation != result.Operation {
				t.Fatal("different recovery operations")
			}
			operation = result.Operation
		}
		if !found {
			t.Fatalf("no child result: %s", output)
		}
	}
	if winner != 1 || foreign != 1 {
		t.Fatalf("winner=%d foreign=%d", winner, foreign)
	}
	// Both children have exited. A third workspace must still honor the marker.
	third, store := windowBook(t)
	_, err := third.BeginWindowWork(context.Background(), parent, store.Identity().WorkspaceID, windowIntent("window/7/8/9"))
	var occupied *WindowOccupied
	if !errors.As(err, &occupied) || !occupied.Foreign || occupied.Owner.OperationID != operation {
		t.Fatalf("exit lost ownership: %v", err)
	}
}
