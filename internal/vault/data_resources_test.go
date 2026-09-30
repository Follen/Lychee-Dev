package vault

import (
	"bufio"
	"context"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"strings"
	"testing"
	"time"
)

func TestDataResourcesCapacityCancellationAndMeasurement(t *testing.T) {
	store, err := Initialize(context.Background(), t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	first, err := store.AcquireDataResources(context.Background(), DataOrdinary)
	if err != nil {
		t.Fatal(err)
	}
	defer first.Close()
	second, err := store.AcquireDataResources(context.Background(), DataOrdinary)
	if err != nil {
		t.Fatal(err)
	}
	defer second.Close()
	for _, policy := range []DataResourcePolicy{DataOrdinary, DataMeasurement} {
		ctx, cancel := context.WithTimeout(context.Background(), 60*time.Millisecond)
		blocked, err := store.AcquireDataResources(ctx, policy)
		cancel()
		if blocked != nil || !errors.Is(err, context.DeadlineExceeded) {
			t.Fatalf("%s admission=%v err=%v", policy, blocked, err)
		}
	}
	if err := first.Close(); err != nil {
		t.Fatal(err)
	}
	if err := second.Close(); err != nil {
		t.Fatal(err)
	}
	measure, err := store.AcquireDataResources(WithDataResourcePolicy(context.Background(), DataMeasurement), DataOrdinary)
	if err != nil {
		t.Fatal(err)
	}
	defer measure.Close()
	ctx, cancel := context.WithTimeout(context.Background(), 60*time.Millisecond)
	defer cancel()
	if _, err := store.AcquireDataResources(ctx, DataOrdinary); !errors.Is(err, context.DeadlineExceeded) {
		t.Fatal(err)
	}
}

func TestDataResourcesConfiguredSingleWorkerAndNoMetadataLock(t *testing.T) {
	ctx := context.Background()
	store, err := Initialize(ctx, t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	cfg := DefaultConfig()
	cfg.Download.Workers = 1
	if _, err := WriteConfig(ctx, store.Root(), cfg); err != nil {
		t.Fatal(err)
	}
	holder, err := store.AcquireDataResources(ctx, DataOrdinary)
	if err != nil {
		t.Fatal(err)
	}
	defer holder.Close()
	waiting, cancel := context.WithTimeout(ctx, 100*time.Millisecond)
	defer cancel()
	done := make(chan error, 1)
	go func() {
		lease, err := store.AcquireDataResources(waiting, DataOrdinary)
		if lease != nil {
			lease.Close()
		}
		done <- err
	}()
	metadata, err := store.OpenMetadata(ctx)
	if err != nil {
		t.Fatal(err)
	}
	defer metadata.Close()
	if err := metadata.CommitDocuments(ctx, Mutation{Key: "data-resources-test", Value: []byte(`"metadata remains writable"`)}); err != nil {
		t.Fatal(err)
	}
	if err := <-done; !errors.Is(err, context.DeadlineExceeded) {
		t.Fatal(err)
	}
}

func TestDataResourcesCrossProcessAndKilledOwner(t *testing.T) {
	store, err := Initialize(context.Background(), t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	cmd := exec.Command(os.Args[0], "-test.run=^TestDataResourceHelperProcess$")
	cmd.Env = append(os.Environ(), "LYCHEE_DEV_DATA_RESOURCE_HELPER="+store.Root())
	stdout, err := cmd.StdoutPipe()
	if err != nil {
		t.Fatal(err)
	}
	cmd.Stderr = os.Stderr
	if err := cmd.Start(); err != nil {
		t.Fatal(err)
	}
	stopped := false
	defer func() {
		if !stopped {
			_ = cmd.Process.Kill()
			_ = cmd.Wait()
		}
	}()
	ready := make(chan string, 1)
	go func() { line, _ := bufio.NewReader(stdout).ReadString('\n'); ready <- strings.TrimSpace(line) }()
	select {
	case line := <-ready:
		if line != "ready" {
			t.Fatal(line)
		}
	case <-time.After(5 * time.Second):
		t.Fatal("helper never ready")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 100*time.Millisecond)
	lease, err := store.AcquireDataResources(ctx, DataOrdinary)
	cancel()
	if lease != nil || !errors.Is(err, context.DeadlineExceeded) {
		t.Fatalf("cross-process admission %v %v", lease, err)
	}
	if err := cmd.Process.Kill(); err != nil {
		t.Fatal(err)
	}
	_ = cmd.Wait()
	stopped = true
	lease, err = store.AcquireDataResources(context.Background(), DataOrdinary)
	if err != nil {
		t.Fatal(err)
	}
	defer lease.Close()
}

func TestDataResourceHelperProcess(t *testing.T) {
	root := os.Getenv("LYCHEE_DEV_DATA_RESOURCE_HELPER")
	if root == "" {
		return
	}
	store, err := OpenStore(root)
	if err != nil {
		t.Fatal(err)
	}
	lease, err := store.AcquireDataResources(context.Background(), DataMeasurement)
	if err != nil {
		t.Fatal(err)
	}
	defer lease.Close()
	fmt.Println("ready")
	time.Sleep(time.Minute)
}
