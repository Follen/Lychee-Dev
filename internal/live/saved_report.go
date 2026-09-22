package live

import (
	"bytes"
	"context"
	"crypto/sha256"
	"errors"
	"fmt"
	"github.com/follenfang/lycheedev/internal/bridge"
	"github.com/follenfang/lycheedev/internal/records"
	"io"
	"os"
	"path/filepath"
	"strings"
	"unicode/utf8"
)

type InstalledReport struct {
	Path       string
	FileSHA256 string
	Report     bridge.VerifiedReport
}

func validateReportAccount(account string) error {
	if account == "" || account == "." || account == ".." || len(account) > 128 || !utf8.ValidString(account) || !filepath.IsLocal(account) || strings.TrimSpace(account) != account || strings.HasSuffix(account, ".") || strings.ContainsAny(account, `/\:`+"\x00") {
		return errors.New("live.invalid_report_account")
	}
	for _, c := range account {
		if c < 32 || c == 127 {
			return errors.New("live.invalid_report_account")
		}
	}
	return nil
}

// ReadInstalledReport reads only the explicitly selected account's live addon
// file. It never falls back to another account, .bak, or legacy database. Matching
// the report proves integrity, not that a post-dispatch flush has occurred; the
// coordinator must establish that ordering independently before advancing state.
func ReadInstalledReport(ctx context.Context, clientDirectory, account string, code []byte, expected bridge.SignalExpectation) (InstalledReport, error) {
	state, err := readInstalledState(ctx, clientDirectory, account, expected)
	if err != nil {
		return InstalledReport{}, err
	}
	report, err := bridge.ReadPersistedReport(bytes.NewReader(state.Bytes), code, expected)
	if err != nil {
		return InstalledReport{}, err
	}
	return InstalledReport{Path: state.Path, FileSHA256: state.FileSHA256, Report: report}, nil
}

type installedState struct {
	Path, FileSHA256 string
	Bytes            []byte
}

func readInstalledState(ctx context.Context, clientDirectory, account string, expected bridge.SignalExpectation) (installedState, error) {
	var zero installedState
	if err := ctx.Err(); err != nil {
		return zero, err
	}
	if err := validateReportAccount(account); err != nil {
		return zero, err
	}
	client, err := records.InspectClientInstallation(ctx, clientDirectory)
	if err != nil {
		return zero, err
	}
	if client.Product != expected.Product || client.FullBuild != expected.Build {
		return zero, errors.New("live.report_client_mismatch")
	}
	root, err := os.OpenRoot(client.Directory)
	if err != nil {
		return zero, err
	}
	defer root.Close()
	parts := []string{"WTF", "Account", account, "SavedVariables", "Lychee Dev.lua"}
	relative, err := reportPath(root, client.Directory, parts, true)
	if err != nil {
		return zero, err
	}
	file, err := root.Open(relative)
	if err != nil {
		return zero, err
	}
	defer file.Close()
	before, err := file.Stat()
	if err != nil {
		return zero, err
	}
	limit := bridge.SavedStateLimits().FileBytes
	if !before.Mode().IsRegular() || before.Size() < 1 || before.Size() > limit {
		return zero, errors.New("live.report_file_limit")
	}
	data, err := io.ReadAll(io.LimitReader(&reportContextReader{ctx: ctx, reader: file}, limit+1))
	if err != nil {
		return zero, err
	}
	after, err := file.Stat()
	if err != nil {
		return zero, err
	}
	current, err := root.Stat(relative)
	if err != nil {
		return zero, err
	}
	if int64(len(data)) != before.Size() || before.Size() != after.Size() || !before.ModTime().Equal(after.ModTime()) || !os.SameFile(after, current) || after.Size() != current.Size() || !after.ModTime().Equal(current.ModTime()) {
		return zero, errors.New("live.report_file_changed")
	}
	if err := ctx.Err(); err != nil {
		return zero, err
	}
	return installedState{Path: filepath.Join(client.Directory, relative), FileSHA256: fmt.Sprintf("%x", sha256.Sum256(data)), Bytes: data}, nil
}

type reportContextReader struct {
	ctx    context.Context
	reader io.Reader
}

// Shared path policy for report discovery and reading. Every directory must be
// owned by the selected installation; links and junctions are not traversed.
func reportPath(root *os.Root, directory string, parts []string, regularFile bool) (string, error) {
	relative := ""
	for i, part := range parts {
		relative = filepath.Join(relative, part)
		info, err := root.Lstat(relative)
		if err != nil {
			return "", err
		}
		file := regularFile && i == len(parts)-1
		if info.Mode()&os.ModeSymlink != 0 || (!file && !info.IsDir()) || (file && !info.Mode().IsRegular()) {
			return "", errors.New("live.report_redirected_path")
		}
		resolved, err := filepath.EvalSymlinks(filepath.Join(directory, relative))
		if err != nil {
			return "", err
		}
		if !strings.EqualFold(resolved, filepath.Join(directory, relative)) {
			return "", errors.New("live.report_redirected_path")
		}
	}
	return relative, nil
}

func (r *reportContextReader) Read(p []byte) (int, error) {
	if err := r.ctx.Err(); err != nil {
		return 0, err
	}
	return r.reader.Read(p)
}
