package records

import (
	"bytes"
	"context"
	"errors"
	"io"
	"os"
	"path/filepath"
	"runtime"
	"sync"
	"testing"
)

func TestOpenExportDestinationRejectsInvalidPaths(t *testing.T) {
	parent := t.TempDir()
	root := filepath.VolumeName(parent)
	if root == "" {
		root = string(filepath.Separator)
	} else {
		root += string(filepath.Separator)
	}

	cases := []struct {
		name   string
		output string
	}{
		{name: "empty", output: ""},
		{name: "root", output: root},
		{name: "trailing-separator", output: filepath.Join(parent, "file.bin") + string(filepath.Separator)},
		{name: "missing-parent", output: filepath.Join(parent, "missing", "file.bin")},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			destination, err := openExportDestination(tc.output, false)
			if destination != nil || !errors.Is(err, ErrExportPath) {
				t.Fatalf("destination=%v, err=%v", destination, err)
			}
		})
	}

	if _, err := os.Stat(filepath.Join(parent, "missing")); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("invalid output created its parent: %v", err)
	}
}

func TestOpenExportDestinationCanonicalizesParentSymlink(t *testing.T) {
	root := t.TempDir()
	actualParent := filepath.Join(root, "actual")
	aliasParent := filepath.Join(root, "alias")
	if err := os.Mkdir(actualParent, 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink(actualParent, aliasParent); err != nil {
		t.Skipf("creating a directory symlink is unavailable: %v", err)
	}

	destination, err := openExportDestination(filepath.Join(aliasParent, "export.bin"), false)
	if err != nil {
		t.Fatal(err)
	}
	closeDestination := closeExportDestination(t, destination)
	wantParent, err := filepath.EvalSymlinks(actualParent)
	if err != nil {
		t.Fatal(err)
	}
	want := filepath.Clean(filepath.Join(wantParent, "export.bin"))
	if destination.path != want {
		t.Fatalf("canonical path = %q, want %q", destination.path, want)
	}
	closeDestination()
	assertDirectoryEntries(t, actualParent, nil)
}

func TestExportDestinationPublishesExactBytes(t *testing.T) {
	for _, tc := range []struct {
		name string
		raw  []byte
	}{
		{name: "arbitrary-bytes", raw: []byte{0, 1, 2, 0xff, 0, 0x7f, 0x00}},
		{name: "zero-bytes", raw: []byte{}},
	} {
		t.Run(tc.name, func(t *testing.T) {
			parent := t.TempDir()
			output := filepath.Join(parent, "export.bin")
			destination, err := openExportDestination(output, false)
			if err != nil {
				t.Fatal(err)
			}
			closeDestination := closeExportDestination(t, destination)
			if err := destination.publish(context.Background(), tc.raw); err != nil {
				t.Fatal(err)
			}
			closeDestination()

			got, err := os.ReadFile(output)
			if err != nil {
				t.Fatal(err)
			}
			if !bytes.Equal(got, tc.raw) {
				t.Fatalf("published bytes = %v, want %v", got, tc.raw)
			}
			assertDirectoryEntries(t, parent, []string{"export.bin"})
		})
	}
}

func TestExportDestinationRejectsExistingTargetWithoutOverwrite(t *testing.T) {
	parent := t.TempDir()
	output := filepath.Join(parent, "export.bin")
	original := []byte("existing export")
	if err := os.WriteFile(output, original, 0600); err != nil {
		t.Fatal(err)
	}

	destination, err := openExportDestination(output, false)
	if destination != nil || !errors.Is(err, ErrExportConflict) {
		t.Fatalf("destination=%v, err=%v", destination, err)
	}
	got, err := os.ReadFile(output)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(got, original) {
		t.Fatalf("conflict changed target to %q", got)
	}
	assertDirectoryEntries(t, parent, []string{"export.bin"})
}

func TestExportDestinationChecksConflictAgainAtPublish(t *testing.T) {
	parent := t.TempDir()
	output := filepath.Join(parent, "export.bin")
	destination, err := openExportDestination(output, false)
	if err != nil {
		t.Fatal(err)
	}
	closeDestination := closeExportDestination(t, destination)

	original := []byte("created after open")
	if err := os.WriteFile(output, original, 0600); err != nil {
		t.Fatal(err)
	}
	if err := destination.publish(context.Background(), []byte("new bytes")); !errors.Is(err, ErrExportConflict) {
		t.Fatalf("publish error = %v, want ErrExportConflict", err)
	}
	closeDestination()

	got, err := os.ReadFile(output)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(got, original) {
		t.Fatalf("late conflict changed target to %q", got)
	}
	assertDirectoryEntries(t, parent, []string{"export.bin"})
}

func TestExportDestinationOverwriteReplacesWithoutMutatingHardlink(t *testing.T) {
	parent := t.TempDir()
	output := filepath.Join(parent, "export.bin")
	alias := filepath.Join(parent, "alias.bin")
	original := []byte("old bytes")
	if err := os.WriteFile(output, original, 0600); err != nil {
		t.Fatal(err)
	}
	if err := os.Link(output, alias); err != nil {
		t.Fatal(err)
	}

	destination, err := openExportDestination(output, true)
	if err != nil {
		t.Fatal(err)
	}
	closeDestination := closeExportDestination(t, destination)
	replacement := []byte("replacement bytes")
	if err := destination.publish(context.Background(), replacement); err != nil {
		t.Fatal(err)
	}
	closeDestination()

	got, err := os.ReadFile(output)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(got, replacement) {
		t.Fatalf("replacement = %q, want %q", got, replacement)
	}
	got, err = os.ReadFile(alias)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(got, original) {
		t.Fatalf("hardlink changed to %q, want %q", got, original)
	}
	assertDirectoryEntries(t, parent, []string{"alias.bin", "export.bin"})
}

func TestOpenExportDestinationRefusesDirectoryAndSymlinkTargets(t *testing.T) {
	parent := t.TempDir()
	directory := filepath.Join(parent, "directory")
	if err := os.Mkdir(directory, 0700); err != nil {
		t.Fatal(err)
	}
	if destination, err := openExportDestination(directory, true); destination != nil || err == nil {
		t.Fatalf("directory target: destination=%v, err=%v", destination, err)
	}

	outside := filepath.Join(t.TempDir(), "outside.bin")
	original := []byte("must not be followed")
	if err := os.WriteFile(outside, original, 0600); err != nil {
		t.Fatal(err)
	}
	symlink := filepath.Join(parent, "symlink.bin")
	if err := os.Symlink(outside, symlink); err != nil {
		t.Skipf("creating a file symlink is unavailable: %v", err)
	}
	if destination, err := openExportDestination(symlink, true); destination != nil || err == nil {
		t.Fatalf("symlink target: destination=%v, err=%v", destination, err)
	}

	got, err := os.ReadFile(outside)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(got, original) {
		t.Fatalf("symlink target changed to %q", got)
	}
	info, err := os.Lstat(symlink)
	if err != nil {
		t.Fatal(err)
	}
	if info.Mode()&os.ModeSymlink == 0 {
		t.Fatalf("leaf was replaced instead of refused: mode=%v", info.Mode())
	}
	assertDirectoryEntries(t, parent, []string{"directory", "symlink.bin"})
}

func TestOpenExportDestinationRefusesWindowsDevice(t *testing.T) {
	if runtime.GOOS != "windows" {
		t.Skip("Windows device path")
	}
	special := filepath.Join(t.TempDir(), "NUL")
	if destination, err := openExportDestination(special, true); destination != nil || err == nil {
		t.Fatalf("special target: destination=%v, err=%v", destination, err)
	}
}

func TestExportDestinationCancellationPreservesTarget(t *testing.T) {
	parent := t.TempDir()
	output := filepath.Join(parent, "export.bin")
	original := []byte("keep this export")
	if err := os.WriteFile(output, original, 0600); err != nil {
		t.Fatal(err)
	}

	destination, err := openExportDestination(output, true)
	if err != nil {
		t.Fatal(err)
	}
	closeDestination := closeExportDestination(t, destination)
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if err := destination.publish(ctx, []byte("must not publish")); !errors.Is(err, context.Canceled) {
		t.Fatalf("publish error = %v, want context.Canceled", err)
	}
	closeDestination()

	got, err := os.ReadFile(output)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(got, original) {
		t.Fatalf("canceled publish changed target to %q", got)
	}
	assertDirectoryEntries(t, parent, []string{"export.bin"})
}

func TestExportDestinationConcurrentNoOverwriteHasOneCompleteWinner(t *testing.T) {
	parent := t.TempDir()
	output := filepath.Join(parent, "export.bin")
	first := []byte("first complete payload")
	second := []byte("second complete payload")

	firstDestination, err := openExportDestination(output, false)
	if err != nil {
		t.Fatal(err)
	}
	closeFirst := closeExportDestination(t, firstDestination)
	secondDestination, err := openExportDestination(output, false)
	if err != nil {
		closeFirst()
		t.Fatal(err)
	}
	closeSecond := closeExportDestination(t, secondDestination)

	start := make(chan struct{})
	results := make(chan error, 2)
	var group sync.WaitGroup
	group.Add(2)
	go func() {
		defer group.Done()
		<-start
		results <- firstDestination.publish(context.Background(), first)
	}()
	go func() {
		defer group.Done()
		<-start
		results <- secondDestination.publish(context.Background(), second)
	}()
	close(start)
	group.Wait()
	closeFirst()
	closeSecond()

	var successes int
	for range 2 {
		switch err := <-results; {
		case err == nil:
			successes++
		case errors.Is(err, ErrExportConflict):
		default:
			t.Fatalf("concurrent publish error = %v", err)
		}
	}
	if successes != 1 {
		t.Fatalf("successful publishers = %d, want 1", successes)
	}

	got, err := os.ReadFile(output)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(got, first) && !bytes.Equal(got, second) {
		t.Fatalf("winner wrote incomplete or mixed bytes: %q", got)
	}
	assertDirectoryEntries(t, parent, []string{"export.bin"})
}

func TestExportDestinationReplacementLeavesHeldRootReaderOnOldBytes(t *testing.T) {
	parent := t.TempDir()
	output := filepath.Join(parent, "export.bin")
	oldBytes := []byte("old bytes held by reader")
	newBytes := []byte("new bytes after replacement")
	if err := os.WriteFile(output, oldBytes, 0600); err != nil {
		t.Fatal(err)
	}

	root, err := os.OpenRoot(parent)
	if err != nil {
		t.Fatal(err)
	}
	defer root.Close()
	reader, err := root.Open("export.bin")
	if err != nil {
		t.Fatal(err)
	}
	defer reader.Close()

	destination, err := openExportDestination(output, true)
	if err != nil {
		t.Fatal(err)
	}
	closeDestination := closeExportDestination(t, destination)
	if err := destination.publish(context.Background(), newBytes); err != nil {
		t.Fatal(err)
	}
	closeDestination()

	held, err := io.ReadAll(reader)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(held, oldBytes) {
		t.Fatalf("held reader returned %q, want %q", held, oldBytes)
	}
	got, err := os.ReadFile(output)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(got, newBytes) {
		t.Fatalf("replacement returned %q, want %q", got, newBytes)
	}
	assertDirectoryEntries(t, parent, []string{"export.bin"})
}

func closeExportDestination(t *testing.T, destination *exportDestination) func() {
	t.Helper()
	closed := false
	close := func() {
		if closed {
			return
		}
		closed = true
		if err := destination.Close(); err != nil {
			t.Errorf("close export destination: %v", err)
		}
	}
	t.Cleanup(close)
	return close
}

func assertDirectoryEntries(t *testing.T, directory string, want []string) {
	t.Helper()
	entries, err := os.ReadDir(directory)
	if err != nil {
		t.Fatal(err)
	}
	wantSet := make(map[string]struct{}, len(want))
	for _, name := range want {
		wantSet[name] = struct{}{}
	}
	if len(entries) != len(wantSet) {
		t.Fatalf("directory entries = %v, want %v", entryNames(entries), want)
	}
	for _, entry := range entries {
		if _, ok := wantSet[entry.Name()]; !ok {
			t.Fatalf("unexpected directory entry %q; want %v", entry.Name(), want)
		}
	}
}

func entryNames(entries []os.DirEntry) []string {
	names := make([]string, 0, len(entries))
	for _, entry := range entries {
		names = append(names, entry.Name())
	}
	return names
}
