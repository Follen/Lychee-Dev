package live

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"sort"
	"strings"

	"github.com/follenfang/lycheedev/internal/desktop"
	"github.com/follenfang/lycheedev/internal/records"
	"github.com/follenfang/lycheedev/internal/selection"
)

const (
	InstallationInstalled = "installed"
	InstallationRunning   = "running"
)

type InstallationCandidate struct {
	Client selection.ClientInstallation `json:"client"`
	State  string                       `json:"state"`
}

type WindowCandidate struct {
	Window desktop.WindowIdentity        `json:"window"`
	Client *selection.ClientInstallation `json:"client,omitempty"`
}

type CandidateReport struct {
	Installations []InstallationCandidate     `json:"installations"`
	Windows       []WindowCandidate           `json:"windows"`
	Issues        []records.InstallationIssue `json:"issues"`
}

type DiscoveryRequest struct {
	Roots []string
}

// DiscoverCandidates performs bounded, read-only installation and game-window inventory.
// It never probes the addon, sends input, or infers actor identity.
func DiscoverCandidates(ctx context.Context, _ string, request DiscoveryRequest) (CandidateReport, error) {
	windows, err := desktop.ListWindows(ctx)
	if err != nil && !errors.Is(err, desktop.ErrUnsupported) {
		return CandidateReport{}, err
	}
	return discoverCandidatesFrom(ctx, request, windows)
}

func discoverCandidatesFrom(ctx context.Context, request DiscoveryRequest, windows []desktop.WindowIdentity) (CandidateReport, error) {
	report := CandidateReport{Installations: []InstallationCandidate{}, Windows: []WindowCandidate{}, Issues: []records.InstallationIssue{}}
	byDirectory := map[string]int{}
	add := func(client selection.ClientInstallation, state string) {
		key := strings.ToLower(filepath.Clean(client.Directory))
		if index, ok := byDirectory[key]; ok {
			if state == InstallationRunning {
				report.Installations[index].State = state
			}
			return
		}
		byDirectory[key] = len(report.Installations)
		report.Installations = append(report.Installations, InstallationCandidate{Client: client, State: state})
	}
	for _, root := range request.Roots {
		if err := ctx.Err(); err != nil {
			return report, err
		}
		if strings.TrimSpace(root) == "" {
			continue
		}
		discovery, err := records.DiscoverClientInstallations(ctx, root)
		if err != nil {
			if !errors.Is(err, records.ErrInstallationMissing) {
				report.Issues = append(report.Issues, records.InstallationIssue{Directory: root, Reason: err.Error()})
			}
			continue
		}
		report.Issues = append(report.Issues, discovery.Issues...)
		for _, client := range discovery.Candidates {
			add(client, InstallationInstalled)
		}
	}
	for _, window := range windows {
		if err := ctx.Err(); err != nil {
			return report, err
		}
		candidate := WindowCandidate{Window: window}
		if gameExecutable(window.Executable) {
			client, err := records.InspectClientInstallation(ctx, filepath.Dir(window.Executable))
			if err == nil {
				candidate.Client = &client
				add(client, InstallationRunning)
			} else if !errors.Is(err, os.ErrNotExist) {
				report.Issues = append(report.Issues, records.InstallationIssue{Directory: filepath.Dir(window.Executable), ProductCode: "", Reason: err.Error()})
			}
		}
		report.Windows = append(report.Windows, candidate)
	}
	sort.Slice(report.Installations, func(i, j int) bool {
		return strings.ToLower(report.Installations[i].Client.Directory) < strings.ToLower(report.Installations[j].Client.Directory)
	})
	return report, nil
}
