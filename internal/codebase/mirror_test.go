package codebase

import (
	"context"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
)

func TestParseLSRemoteHeadShapes(t *testing.T) {
	const head = "0123456789012345678901234567890123456789"
	out := []byte(head + "\trefs/heads/live\n" +
		"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\trefs/tags/v1^{}\n" +
		"garbage line\n" +
		"short\trefs/heads/live\n")
	if got, ok := parseLSRemoteHead(out, "refs/heads/live"); !ok || got != head {
		t.Fatalf("head = %q %v", got, ok)
	}
	if _, ok := parseLSRemoteHead(out, "refs/heads/missing"); ok {
		t.Fatal("absent ref reported found")
	}
	if _, ok := parseLSRemoteHead(nil, "refs/heads/live"); ok {
		t.Fatal("empty output reported found")
	}
	peeled := []byte("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\trefs/tags/v1^{}\n")
	if _, ok := parseLSRemoteHead(peeled, "refs/tags/v1"); ok {
		t.Fatal("peeled annotation line matched the tag ref")
	}
}

// bareRepositoryAt creates a minimal bare repository with one branch commit.
// Fixed author timestamps make equal inputs produce equal commit ids.
func bareRepositoryAt(t *testing.T, dir, branch, message string) string {
	t.Helper()
	if _, err := gitBytes(context.Background(), "", 4096, "init", "--bare", dir); err != nil {
		t.Fatal(err)
	}
	run := func(input string, args ...string) string {
		t.Helper()
		cmd := exec.Command("git", append([]string{"--git-dir=" + dir}, args...)...)
		cmd.Env = append(os.Environ(),
			"GIT_AUTHOR_NAME=MirrorTest", "GIT_AUTHOR_EMAIL=mirror@example.invalid",
			"GIT_COMMITTER_NAME=MirrorTest", "GIT_COMMITTER_EMAIL=mirror@example.invalid",
			"GIT_AUTHOR_DATE=2026-01-01T00:00:00Z", "GIT_COMMITTER_DATE=2026-01-01T00:00:00Z",
		)
		cmd.Stdin = strings.NewReader(input)
		out, err := cmd.CombinedOutput()
		if err != nil {
			t.Fatalf("git %v: %v: %s", args, err, out)
		}
		return strings.TrimSpace(string(out))
	}
	blob := run("content\n", "hash-object", "-w", "--stdin")
	tree := run("100644 blob "+blob+"\tfile.lua\n", "mktree")
	commit := run(message+"\n", "commit-tree", tree)
	run("", "update-ref", "refs/heads/"+branch, commit)
	return commit
}

func TestCheckMirrorHealthLocalRemote(t *testing.T) {
	ctx := context.Background()
	home := t.TempDir()
	remoteDir := filepath.Join(t.TempDir(), "remote.git")
	remoteCommit := bareRepositoryAt(t, remoteDir, "live", "remote head")
	spec := RepositorySpec{Key: "fixture-remote", URL: remoteDir, Tracks: map[string]string{"retail": "live"}}

	health, err := CheckMirrorHealth(ctx, home, spec, "retail", true)
	if err != nil {
		t.Fatal(err)
	}
	if health.RemoteStatus != "skipped_offline" || health.RemoteCommit != "" || health.Initialized || health.UpdateAvailable {
		t.Fatalf("offline probe touched the network or faked facts: %+v", health)
	}

	health, err = CheckMirrorHealth(ctx, home, spec, "retail", false)
	if err != nil {
		t.Fatal(err)
	}
	if health.RemoteStatus != "checked" || health.RemoteCommit != remoteCommit || health.Initialized || health.UpdateAvailable {
		t.Fatalf("uninitialized mirror health = %+v", health)
	}
	if health.SourceID != "fixture-remote" || health.Product != "retail" || health.Branch != "live" {
		t.Fatalf("identity = %+v", health)
	}

	mirror := filepath.Join(home, "mirrors", "fixture-remote.git")
	if commit := bareRepositoryAt(t, mirror, "live", "remote head"); commit != remoteCommit {
		t.Fatalf("fixture commits differ: %s vs %s", commit, remoteCommit)
	}
	health, err = CheckMirrorHealth(ctx, home, spec, "retail", false)
	if err != nil {
		t.Fatal(err)
	}
	if !health.Initialized || health.LocalCommit != remoteCommit || health.UpdateAvailable {
		t.Fatalf("up-to-date mirror reported update: %+v", health)
	}

	stale := filepath.Join(t.TempDir(), "stale-home")
	if err := os.MkdirAll(filepath.Join(stale, "mirrors"), 0700); err != nil {
		t.Fatal(err)
	}
	if commit := bareRepositoryAt(t, filepath.Join(stale, "mirrors", "fixture-remote.git"), "live", "local head"); commit == remoteCommit {
		t.Fatal("stale fixture unexpectedly matched")
	}
	health, err = CheckMirrorHealth(ctx, stale, spec, "retail", false)
	if err != nil {
		t.Fatal(err)
	}
	if !health.Initialized || !health.UpdateAvailable || health.LocalCommit == health.RemoteCommit {
		t.Fatalf("stale mirror not reported: %+v", health)
	}
}

func TestCheckMirrorHealthUnknownRemoteStaysUnknown(t *testing.T) {
	home := t.TempDir()
	remoteDir := filepath.Join(t.TempDir(), "remote.git")
	bareRepositoryAt(t, remoteDir, "live", "remote head")
	spec := RepositorySpec{Key: "fixture-remote", URL: remoteDir, Tracks: map[string]string{"retail": "nope"}}

	health, err := CheckMirrorHealth(context.Background(), home, spec, "retail", false)
	if err == nil || !strings.Contains(err.Error(), "codebase.ref_not_found") {
		t.Fatalf("missing remote branch = %v", err)
	}
	if health.RemoteStatus != "unknown" || health.RemoteCommit != "" || health.UpdateAvailable {
		t.Fatalf("unknown remote faked facts: %+v", health)
	}

	if _, err := CheckMirrorHealth(context.Background(), home, spec, "unknown-product", false); err == nil || !strings.Contains(err.Error(), "codebase.unknown_product") {
		t.Fatalf("unknown product accepted: %v", err)
	}
}
