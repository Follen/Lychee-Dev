package live

import (
	"context"
	"errors"
	"github.com/follenfang/lycheedev/internal/bridge"
	"path/filepath"
	"sort"
	"strings"

	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
	"image"
)

// Installation states distinguish installed-but-not-running targets from
// installations that own a running candidate.
const (
	InstallationInstalled = "installed"
	InstallationRunning   = "running"
)

// Candidate is one running game window with its installation identity and one
// bounded identity observation. Windows stay distinct entries even when every
// identity field matches; nothing here is a nonce or a permission.
type Candidate struct {
	Window         desktop.WindowIdentity       `json:"window"`
	Client         selection.ClientInstallation `json:"client"`
	State          string                       `json:"state"`
	Character      string                       `json:"character,omitempty"`
	Realm          string                       `json:"realm,omitempty"`
	InputReady     bool                         `json:"inputReady"`
	NotReadyReason string                       `json:"notReadyReason,omitempty"`
	Capture        string                       `json:"capture,omitempty"`
	BusyOperation  string                       `json:"busyOperationId,omitempty"`
	ForeignOwner   bool                         `json:"foreignOwner,omitempty"`
	Reason         string                       `json:"reason,omitempty"`
	RuntimeRelease string                       `json:"runtimeRelease,omitempty"`
	GUID           string                       `json:"-"`
}

type InstallationCandidate struct {
	Client selection.ClientInstallation `json:"client"`
	State  string                       `json:"state"`
}

// CandidateReport is the discovery data callers choose from. Genuine
// ambiguity stays data here, not a bare error.
type CandidateReport struct {
	Installations []InstallationCandidate `json:"installations"`
	Candidates    []Candidate             `json:"candidates"`
}

// DiscoveryRequest bounds the scan: known roots (game roots or client
// directories) and optional window filters. It never walks whole disks.
type DiscoveryRequest struct {
	Roots        []string
	PID          uint32
	Installation string
}

// clientFolders are the only directory names inspected below a known root.
var clientFolders = []string{"_retail_", "_classic_", "_classic_titan_", "_classic_beta_", "_forever_"}

// DiscoverCandidates scans known roots for supported installations and probes
// each running game window once with a fixed identity trigger, one window at a
// time. Per-window failures stay isolated and busy windows are never typed
// into. Identity marking is the only game action performed here.
func DiscoverCandidates(ctx context.Context, root string, request DiscoveryRequest) (CandidateReport, error) {
	return discoverCandidates(ctx, root, request, nativeIO())
}

func discoverCandidates(ctx context.Context, root string, request DiscoveryRequest, io *liveIO) (CandidateReport, error) {
	var report CandidateReport
	if err := ctx.Err(); err != nil {
		return report, err
	}
	windows, err := io.list(ctx)
	if err != nil {
		return report, err
	}
	workspaceID := workspaceIdentity(ctx, root)
	running := map[string]bool{}
	runningDirs := []string{}
	for _, window := range windows {
		if !gameExecutable(window.Executable) {
			continue
		}
		if request.PID != 0 && window.ProcessID != request.PID {
			continue
		}
		candidate := Candidate{Window: window}
		client, err := windowClient(ctx, window, io)
		if err != nil {
			// One unreadable window never drops the others.
			candidate.State, candidate.Reason = CandidateUnreadable, err.Error()
			report.Candidates = append(report.Candidates, candidate)
			continue
		}
		candidate.Client = client
		if request.Installation != "" && !sameInstallationPath(client.Directory, request.Installation) {
			continue
		}
		key := strings.ToLower(canonicalPath(client.Directory))
		if !running[key] {
			runningDirs = append(runningDirs, client.Directory)
		}
		running[key] = true
		owner, occupied, foreign, err := windowOwnership(ctx, workspaceID, ClientWindow{Client: client, Window: window}, io)
		if occupied {
			candidate.State, candidate.BusyOperation, candidate.ForeignOwner = CandidateBusy, owner.OperationID, foreign
			if err != nil {
				candidate.Reason = err.Error()
			}
			report.Candidates = append(report.Candidates, candidate)
			continue
		}
		observation, err := probeIdentity(ctx, ClientWindow{Client: client, Window: window}, image.Rectangle{}, false, io)
		candidate.Capture = observation.Capture
		candidate.RuntimeRelease = observation.Signal.Release
		if err != nil {
			// No correlated receipt within the bounded window: addon missing,
			// black screen or input not delivered all land here.
			candidate.State, candidate.Reason = CandidateUnreadable, err.Error()
			var mismatch *bridge.RuntimeReleaseMismatch
			if errors.As(err, &mismatch) {
				candidate.State = "runtime_mismatch"
				candidate.Character, candidate.Realm = observation.Signal.Character, observation.Signal.Realm
				candidate.InputReady, candidate.NotReadyReason = observation.Signal.InputReady, observation.Signal.InputReason
			}
			report.Candidates = append(report.Candidates, candidate)
			continue
		}
		switch observation.Signal.ActorState {
		case "ok":
			candidate.State = CandidateIdentified
			candidate.Character, candidate.Realm, candidate.GUID = observation.Signal.Character, observation.Signal.Realm, observation.Signal.GUID
		case "no_actor":
			candidate.State = CandidateNoActor
		default:
			candidate.State = CandidateActorRestricted
		}
		candidate.InputReady = observation.Signal.InputReady
		candidate.NotReadyReason = observation.Signal.InputReason
		report.Candidates = append(report.Candidates, candidate)
	}
	report.Installations = discoverInstallations(ctx, request.Roots, runningDirs, running, io)
	return report, nil
}

// discoverInstallations inspects only known roots and the fixed client folder
// names beneath them, plus the roots that are themselves client directories
// and the installations observed behind running windows.
func discoverInstallations(ctx context.Context, roots, runningDirs []string, running map[string]bool, io *liveIO) []InstallationCandidate {
	seen := map[string]bool{}
	installations := make([]InstallationCandidate, 0)
	add := func(directory string) {
		// Deduplicate on the canonical spelling: a scan root spelled through
		// an 8.3 short path (or symlink) is the same directory as its long
		// form observed behind a running window.
		key := strings.ToLower(canonicalPath(directory))
		if seen[key] {
			return
		}
		client, err := io.inspect(ctx, directory)
		if err != nil {
			return
		}
		seen[key] = true
		state := InstallationInstalled
		if running[strings.ToLower(canonicalPath(client.Directory))] {
			state = InstallationRunning
		}
		installations = append(installations, InstallationCandidate{Client: client, State: state})
	}
	for _, root := range roots {
		if root == "" {
			continue
		}
		add(root)
		for _, folder := range clientFolders {
			add(filepath.Join(root, folder))
		}
	}
	for _, directory := range runningDirs {
		add(directory)
	}
	sort.Slice(installations, func(i, j int) bool {
		return strings.ToLower(installations[i].Client.Directory) < strings.ToLower(installations[j].Client.Directory)
	})
	return installations
}

func windowClient(ctx context.Context, window desktop.WindowIdentity, io *liveIO) (selection.ClientInstallation, error) {
	executable, err := windowExecutable(window)
	if err != nil {
		return selection.ClientInstallation{}, err
	}
	return io.inspect(ctx, filepath.Dir(executable))
}

// workspaceIdentity is best effort: a missing workspace cannot claim any
// window owner as its own.
func workspaceIdentity(ctx context.Context, root string) string {
	id, err := vault.ReadWorkspace(ctx, root, func(store *vault.Store, _ *vault.Metadata) (string, error) {
		return store.Identity().WorkspaceID, nil
	})
	if err != nil {
		return ""
	}
	return id
}

// canonicalPath resolves symlinks and 8.3 short names (GitHub runner TMP is
// C:\Users\RUNNER~1\...) so identity comparisons compare one spelling. It
// falls back to a cleaned absolute path when it cannot be resolved, including for
// absent directories that are only checked for deduplication.
func canonicalPath(path string) string {
	if absolute, err := filepath.Abs(path); err == nil {
		path = absolute
	}
	path = filepath.Clean(path)
	if resolved, err := filepath.EvalSymlinks(path); err == nil {
		return resolved
	}
	return path
}

// Path spelling is not installation identity. Use the same normalization for
// discovery filters, candidate selection and saved-session constraints.
func sameInstallationPath(a, b string) bool {
	return a != "" && b != "" && strings.EqualFold(canonicalPath(a), canonicalPath(b))
}
