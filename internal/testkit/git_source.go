package testkit

import (
	"context"
	"io/fs"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
)

// GitSourceFixture creates a real immutable Git commit, and confines URL
// rewriting to the scratch mirror so production source sync fetches it offline.
func GitSourceFixture(t testing.TB, ctx context.Context, home, key, remoteURL, source string) string {
	t.Helper()
	repository := t.TempDir()
	err := filepath.WalkDir(source, func(name string, entry fs.DirEntry, walkErr error) error {
		if walkErr != nil {
			return walkErr
		}
		rel, err := filepath.Rel(source, name)
		if err != nil {
			return err
		}
		if rel == "." {
			return nil
		}
		if entry.Name() == ".git" && entry.IsDir() {
			return filepath.SkipDir
		}
		target := filepath.Join(repository, rel)
		if entry.IsDir() {
			return os.MkdirAll(target, 0700)
		}
		if !entry.Type().IsRegular() {
			return nil
		}
		raw, err := os.ReadFile(name)
		if err != nil {
			return err
		}
		return os.WriteFile(target, raw, 0600)
	})
	if err != nil {
		t.Fatal(err)
	}
	run := func(dir string, args ...string) string {
		t.Helper()
		cmd := exec.CommandContext(ctx, "git", append([]string{"-c", "core.hooksPath=" + os.DevNull, "-c", "commit.gpgsign=false", "-C", dir}, args...)...)
		cmd.Env = append(os.Environ(), "GIT_AUTHOR_NAME=SourceFixture", "GIT_AUTHOR_EMAIL=fixture@example.invalid", "GIT_COMMITTER_NAME=SourceFixture", "GIT_COMMITTER_EMAIL=fixture@example.invalid", "GIT_AUTHOR_DATE=2000-01-01T00:00:00Z", "GIT_COMMITTER_DATE=2000-01-01T00:00:00Z")
		raw, err := cmd.CombinedOutput()
		if err != nil {
			t.Fatalf("git %v: %v: %s", args, err, raw)
		}
		return strings.TrimSpace(string(raw))
	}
	run(repository, "init", "--quiet", "--object-format=sha1")
	run(repository, "-c", "core.autocrlf=false", "add", "--all")
	run(repository, "commit", "--quiet", "--allow-empty", "-m", "source fixture")
	commit := run(repository, "rev-parse", "HEAD")
	mirror := filepath.Join(home, "mirrors", key+".git")
	if err := os.MkdirAll(mirror, 0700); err != nil {
		t.Fatal(err)
	}
	run(mirror, "init", "--bare", "--quiet", "--object-format=sha1")
	run(mirror, "config", "protocol.file.allow", "always")
	// Each fixture replaces only its own URL rewrite; the mirror holds all prior
	// pinned commits, so later fixture preparation cannot mutate old snapshots.
	cmd := exec.CommandContext(ctx, "git", "-C", mirror, "config", "--local", "--get-regexp", `^url\..*\.insteadof$`)
	raw, err := cmd.Output()
	if err != nil {
		if exit, ok := err.(*exec.ExitError); !ok || exit.ExitCode() != 1 {
			t.Fatal(err)
		}
	}
	for _, line := range strings.Split(string(raw), "\n") {
		key, value, ok := strings.Cut(line, " ")
		if ok && value == remoteURL {
			run(mirror, "config", "--unset-all", key)
		}
	}
	run(mirror, "config", "url."+filepath.ToSlash(repository)+".insteadOf", remoteURL)
	return commit
}
