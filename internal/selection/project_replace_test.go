package selection

import (
	"context"
	"fmt"
	"io"
	"path/filepath"
	"strings"
	"sync"
	"testing"
)

func TestProjectReaderRetainsOriginalAcrossReplacement(t *testing.T) {
	ctx := context.Background()
	dir := t.TempDir()
	if err := writeProjectFile(ctx, dir, projectLockFile, ProjectLock{Schema: "old"}); err != nil {
		t.Fatal(err)
	}
	f, err := openProjectFile(filepath.Join(dir, projectLockFile))
	if err != nil {
		t.Fatal(err)
	}
	defer f.Close()
	if err := writeProjectFile(ctx, dir, projectLockFile, ProjectLock{Schema: "new"}); err != nil {
		t.Fatal(err)
	}
	raw, err := io.ReadAll(f)
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(string(raw), `"schema": "old"`) {
		t.Fatalf("existing reader changed identity: %s", raw)
	}
	var current ProjectLock
	if err := readProjectFile(ctx, filepath.Join(dir, projectLockFile), &current); err != nil {
		t.Fatal(err)
	}
	if current.Schema != "new" {
		t.Fatal(current)
	}
}

func TestProjectLockReaderDuringReplacement(t *testing.T) {
	ctx := context.Background()
	dir := t.TempDir()
	value := ProjectLock{Schema: "replacement-fixture"}
	if err := writeProjectFile(ctx, dir, projectLockFile, value); err != nil {
		t.Fatal(err)
	}
	start := make(chan struct{})
	var readers sync.WaitGroup
	errorsSeen := make(chan error, 1000)
	readers.Go(func() {
		<-start
		for range 500 {
			var got ProjectLock
			if err := readProjectFile(ctx, filepath.Join(dir, projectLockFile), &got); err != nil {
				errorsSeen <- err
				continue
			}
			if got.Schema != value.Schema {
				errorsSeen <- fmt.Errorf("partial lock: %+v", got)
			}
		}
	})
	close(start)
	for range 200 {
		if err := writeProjectFile(ctx, dir, projectLockFile, value); err != nil {
			errorsSeen <- err
		}
	}
	readers.Wait()
	close(errorsSeen)
	for err := range errorsSeen {
		t.Error(err)
	}
}
