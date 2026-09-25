package parity

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"

	"github.com/follenfang/lycheedev/internal/command"
)

// Status describes what the offline parity suite can honestly prove. It never
// means that a retired executable was invoked.
type Status string

const (
	StatusPassed            Status = "passed"
	StatusFixtureBacked     Status = "fixture-backed"
	StatusFixtureBackedPart Status = "fixture-backed-partial"
	StatusIntentionalChange Status = "intentional-change"
	StatusNotRun            Status = "not_run"
	StatusNotApplicable     Status = "not_applicable"
)

var validStatuses = map[Status]bool{
	StatusPassed:            true,
	StatusFixtureBacked:     true,
	StatusFixtureBackedPart: true,
	StatusIntentionalChange: true,
	StatusNotRun:            true,
	StatusNotApplicable:     true,
}

// Case is one historical business behavior, not merely a command name.
type Case struct {
	ID         string
	Domain     string
	Legacy     string
	Current    []string
	Assertions []string
	Evidence   []string
	Status     Status
	Note       string
}

type Catalog struct {
	Schema      string
	Purpose     string
	Offline     bool
	Cases       []Case
	Normalizers []string
}

type CaseResult struct {
	CaseID        string
	Status        Status
	Current       []string
	Evidence      []string
	Missing       []string
	MissingSource []string
}

type Report struct {
	Schema  string
	Cases   []CaseResult
	Summary Summary
}

type Summary struct {
	Total                int
	Passed               int
	FixtureBacked        int
	FixtureBackedPartial int
	IntentionalChange    int
	NotRun               int
	NotApplicable        int
}

func Load(path string) (Catalog, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return Catalog{}, err
	}
	var catalog Catalog
	decoder := json.NewDecoder(bytes.NewReader(data))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&catalog); err != nil {
		return Catalog{}, fmt.Errorf("decode parity catalog: %w", err)
	}
	return catalog, nil
}

// Validate checks the catalog against the current public command directory and
// repository evidence. It does not run any legacy command or read legacy home
// data. A report is returned even when cases are missing evidence so callers
// can display the complete recovery ledger.
func Validate(catalog Catalog, root string, commands map[string]bool) (Report, error) {
	if catalog.Schema != "lycheedev.semantic-parity-coverage.v2" || !catalog.Offline {
		return Report{}, fmt.Errorf("unsupported parity catalog: schema=%q offline=%v", catalog.Schema, catalog.Offline)
	}
	if len(catalog.Cases) == 0 {
		return Report{}, fmt.Errorf("parity catalog has no cases")
	}
	seen := map[string]bool{}
	report := Report{Schema: "lycheedev.semantic-parity-report.v1"}
	for _, item := range catalog.Cases {
		if item.ID == "" || seen[item.ID] {
			return Report{}, fmt.Errorf("duplicate or empty parity case id %q", item.ID)
		}
		seen[item.ID] = true
		if item.Domain == "" || item.Legacy == "" || len(item.Current) == 0 || len(item.Assertions) == 0 || len(item.Evidence) == 0 {
			return Report{}, fmt.Errorf("incomplete parity case %q", item.ID)
		}
		if !validStatuses[item.Status] {
			return Report{}, fmt.Errorf("unknown status %q for %s", item.Status, item.ID)
		}
		if requiresNote(item.Status) && strings.TrimSpace(item.Note) == "" {
			return Report{}, fmt.Errorf("status %q for %s requires a note", item.Status, item.ID)
		}

		result := CaseResult{CaseID: item.ID, Status: item.Status, Current: append([]string(nil), item.Current...), Evidence: append([]string(nil), item.Evidence...)}
		for _, path := range item.Evidence {
			matches, err := filepath.Glob(filepath.Join(root, filepath.FromSlash(path)))
			if err != nil || len(matches) == 0 {
				result.Missing = append(result.Missing, path)
			}
		}
		for _, path := range item.Current {
			if !commands[path] {
				result.MissingSource = append(result.MissingSource, path)
			}
		}
		report.Cases = append(report.Cases, result)
		if len(result.Missing) != 0 || len(result.MissingSource) != 0 {
			return report, fmt.Errorf("parity case %s is not grounded: missing evidence=%v commands=%v", item.ID, result.Missing, result.MissingSource)
		}
		report.Summary.add(item.Status)
	}
	report.Summary.Total = len(report.Cases)
	sort.Slice(report.Cases, func(i, j int) bool { return report.Cases[i].CaseID < report.Cases[j].CaseID })
	return report, nil
}

func requiresNote(status Status) bool {
	return status == StatusFixtureBackedPart || status == StatusIntentionalChange || status == StatusNotRun || status == StatusNotApplicable
}

func (s *Summary) add(status Status) {
	switch status {
	case StatusPassed:
		s.Passed++
	case StatusFixtureBacked:
		s.FixtureBacked++
	case StatusFixtureBackedPart:
		s.FixtureBackedPartial++
	case StatusIntentionalChange:
		s.IntentionalChange++
	case StatusNotRun:
		s.NotRun++
	case StatusNotApplicable:
		s.NotApplicable++
	}
}

// DiscoverCommands observes the same public describe surface that agents use.
// Keeping this at the seam means parity tests do not duplicate the command
// contract and cannot silently drift when a command is renamed.
func DiscoverCommands(ctx context.Context) (map[string]bool, error) {
	var stdout, stderr bytes.Buffer
	if code := command.Execute(ctx, []string{"describe", "--format=json"}, &stdout, &stderr); code != 0 {
		return nil, fmt.Errorf("describe failed with exit %d: %s", code, stderr.String())
	}
	var envelope struct {
		OK     bool
		Result struct {
			Commands []struct {
				Path string
			}
		}
	}
	if err := json.Unmarshal(stdout.Bytes(), &envelope); err != nil {
		return nil, fmt.Errorf("decode describe: %w", err)
	}
	if !envelope.OK {
		return nil, fmt.Errorf("describe returned ok=false")
	}
	paths := make(map[string]bool, len(envelope.Result.Commands))
	for _, item := range envelope.Result.Commands {
		paths[item.Path] = true
	}
	return paths, nil
}
