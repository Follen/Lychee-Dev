package live

import (
	"context"
	"errors"
	"io"
	"os"
	"sort"
)

var ErrAccountScanLimit = errors.New("live.account_scan_limit: specify --account instead of scanning more than 256 entries")

// AccountSelectionError asks for a caller choice before a game operation exists.
// Candidates are matching directory names, not authenticated login identities.
type AccountSelectionError struct{ Candidates []string }

func (e *AccountSelectionError) Error() string {
	if len(e.Candidates) == 0 {
		return "live.account_selection_required: no stored directory matches this character and realm; specify --account"
	}
	return "live.account_selection_required: multiple stored accounts match this character and realm; specify --account"
}

// resolveReportAccount discovers a report location, never a source of session
// authority. It reads directory metadata only. Explicit selection also supports
// a first login whose character directory has not yet been written by WoW.
func resolveReportAccount(ctx context.Context, clientDirectory, character, realm, explicit string) (string, error) {
	if err := ctx.Err(); err != nil {
		return "", err
	}
	if explicit != "" {
		if err := validateReportAccount(explicit); err != nil {
			return "", err
		}
		return explicit, nil
	}
	if validateReportAccount(character) != nil || validateReportAccount(realm) != nil {
		return "", errors.New("live.invalid_report_identity")
	}
	root, err := os.OpenRoot(clientDirectory)
	if err != nil {
		return "", err
	}
	defer root.Close()
	relative, err := reportPath(root, clientDirectory, []string{"WTF", "Account"}, false)
	if errors.Is(err, os.ErrNotExist) {
		return "", &AccountSelectionError{Candidates: []string{}}
	}
	if err != nil {
		return "", err
	}
	directory, err := root.Open(relative)
	if err != nil {
		return "", err
	}
	defer directory.Close()
	entries, err := directory.ReadDir(257)
	if err != nil && !errors.Is(err, io.EOF) {
		return "", err
	}
	if len(entries) > 256 {
		return "", ErrAccountScanLimit
	}
	matches := make([]string, 0)
	for _, entry := range entries {
		if err := ctx.Err(); err != nil {
			return "", err
		}
		// Windows junctions may be reported as irregular rather than symlink
		// entries. They must reach path validation, not disappear as noise.
		if entry.Type().IsRegular() {
			continue
		}
		if err := validateReportAccount(entry.Name()); err != nil {
			return "", err
		}
		_, err := reportPath(root, clientDirectory, []string{"WTF", "Account", entry.Name(), realm, character}, false)
		if errors.Is(err, os.ErrNotExist) {
			continue
		}
		if err != nil {
			return "", err
		}
		matches = append(matches, entry.Name())
	}
	if err := ctx.Err(); err != nil {
		return "", err
	}
	if len(matches) != 1 {
		sort.Strings(matches)
		return "", &AccountSelectionError{Candidates: matches}
	}
	return matches[0], nil
}
