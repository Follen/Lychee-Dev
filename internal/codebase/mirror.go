package codebase

import (
	"context"
	"errors"
	"path/filepath"
	"strings"
	"time"
)

// MirrorHealth is the read model of `source check` mirror health. It compares
// the local mirror head of one product branch with the remote head that
// `git ls-remote` reports, without fetching.
type MirrorHealth struct {
	SourceID        string `json:"sourceId"`
	Product         string `json:"product"`
	Branch          string `json:"branch"`
	LocalCommit     string `json:"localCommit"`
	RemoteCommit    string `json:"remoteCommit"`
	UpdateAvailable bool   `json:"updateAvailable"`
	Initialized     bool   `json:"initialized"`
	// RemoteStatus is "checked" after a successful probe, "skipped_offline"
	// when offline prevented any network request, and "unknown" when the probe
	// failed. Unknown never fabricates remote or update facts.
	RemoteStatus string `json:"remoteStatus"`
}

const mirrorProbeTimeout = 60 * time.Second

// CheckMirrorHealth reports mirror health for one repository product. With
// offline true it performs zero network requests and reports remoteStatus
// "skipped_offline". The probe is bounded by a timeout and by output size and
// honors context cancellation.
func CheckMirrorHealth(ctx context.Context, home string, repo RepositorySpec, product string, offline bool) (MirrorHealth, error) {
	branch, ok := repo.Tracks[product]
	if !ok {
		return MirrorHealth{}, errors.New("codebase.unknown_product")
	}
	health := MirrorHealth{SourceID: repo.Key, Product: product, Branch: branch, RemoteStatus: "skipped_offline"}
	mirror := filepath.Join(home, "mirrors", repo.Key+".git")
	if out, err := gitProbe(ctx, mirror, 128, mirrorProbeTimeout, "rev-parse", "--verify", "refs/heads/"+branch+"^{commit}"); err == nil {
		health.LocalCommit = strings.TrimSpace(string(out))
	}
	health.Initialized = health.LocalCommit != ""
	if offline {
		return health, nil
	}
	health.RemoteStatus = "unknown"
	ref := "refs/heads/" + branch
	// Local transports stay enabled so mirror health is testable and usable
	// against local remotes; every other non-HTTPS protocol stays blocked by
	// the shared Git transport configuration.
	out, err := gitProbe(ctx, "", 4096, mirrorProbeTimeout, "-c", "protocol.file.allow=always", "ls-remote", repo.URL, ref)
	if err != nil {
		return health, err
	}
	remote, found := parseLSRemoteHead(out, ref)
	if !found {
		return health, errors.New("codebase.ref_not_found: remote branch not found: " + branch)
	}
	health.RemoteCommit = remote
	health.RemoteStatus = "checked"
	health.UpdateAvailable = health.Initialized && health.LocalCommit != remote
	return health, nil
}

// parseLSRemoteHead extracts the object id of one ref from `git ls-remote`
// output ("<object> <ref>" per line). Peeled "^{}" annotation lines and
// malformed lines are ignored, and an absent ref reports false instead of an
// empty commit.
func parseLSRemoteHead(out []byte, ref string) (string, bool) {
	for _, line := range strings.Split(string(out), "\n") {
		fields := strings.Fields(line)
		if len(fields) != 2 || fields[1] != ref {
			continue
		}
		if objectID(fields[0]) {
			return fields[0], true
		}
	}
	return "", false
}
