package delivery

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"github.com/follenfang/lycheedev/internal/bridge"
	"io"
	"os"
	"strings"
	"unicode/utf8"
)

const installationMarker = ".lycheedev-install.json"

var ErrInstallation = errors.New("delivery.invalid_installation")

// InstallationReceipt records only files deployed by this toolkit. Paths retain
// their addon/ or skill/ prefix even though the installed directory omits it.
// This is an ownership record, not a signature or authority to overwrite edits.
type InstallationReceipt struct {
	Schema    string     `json:"schema"`
	Component string     `json:"component"`
	Version   string     `json:"version"`
	Commit    string     `json:"commit"`
	Resources []Resource `json:"resources"`
}

type InstallAssessment struct {
	State   string               `json:"state"`
	Receipt *InstallationReceipt `json:"receipt,omitempty"`
	Detail  string               `json:"detail,omitempty"`
}

// InspectInstallation is read-only. An absent directory is available; even an
// empty existing directory without a receipt is unmanaged, never adopted. A
// managed result describes this instant only: apply must recheck under its lock.
func InspectInstallation(ctx context.Context, directory, component string) (InstallAssessment, error) {
	return inspectInstallation(ctx, directory, component, false)
}

func inspectInstallation(ctx context.Context, directory, component string, runtimeQueue bool) (InstallAssessment, error) {
	if err := ctx.Err(); err != nil {
		return InstallAssessment{}, err
	}
	if component != "addon" && component != "skill" {
		return InstallAssessment{}, ErrInstallation
	}
	info, err := os.Lstat(directory)
	if errors.Is(err, os.ErrNotExist) {
		return InstallAssessment{State: "absent"}, nil
	}
	if err != nil {
		return InstallAssessment{}, err
	}
	if !info.IsDir() || info.Mode()&os.ModeSymlink != 0 {
		return InstallAssessment{State: "unmanaged", Detail: "target is not an ordinary directory"}, nil
	}
	root, err := os.OpenRoot(directory)
	if err != nil {
		return InstallAssessment{}, err
	}
	defer root.Close()
	info, err = root.Lstat(installationMarker)
	if errors.Is(err, os.ErrNotExist) {
		return InstallAssessment{State: "unmanaged"}, nil
	}
	if err != nil {
		return InstallAssessment{}, err
	}
	if !info.Mode().IsRegular() || info.Size() > 1<<20 {
		return InstallAssessment{}, fmt.Errorf("%w: receipt file", ErrInstallation)
	}
	file, err := root.Open(installationMarker)
	if err != nil {
		return InstallAssessment{}, err
	}
	defer file.Close()
	raw, err := io.ReadAll(&cancelReader{ctx: ctx, reader: io.LimitReader(file, (1<<20)+1)})
	if err != nil {
		return InstallAssessment{}, err
	}
	if len(raw) > 1<<20 || !utf8.Valid(raw) {
		return InstallAssessment{}, fmt.Errorf("%w: receipt encoding or size", ErrInstallation)
	}
	decoder := json.NewDecoder(bytes.NewReader(raw))
	if err := uniqueJSON(decoder, 0); err != nil {
		return InstallAssessment{}, fmt.Errorf("%w: %v", ErrInstallation, err)
	}
	if _, err := decoder.Token(); err != io.EOF {
		return InstallAssessment{}, fmt.Errorf("%w: trailing JSON", ErrInstallation)
	}
	decoder = json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	var receipt InstallationReceipt
	if err := decoder.Decode(&receipt); err != nil {
		return InstallAssessment{}, fmt.Errorf("%w: %v", ErrInstallation, err)
	}
	commit, err := hex.DecodeString(receipt.Commit)
	if receipt.Schema != "lycheedev.installation.v1" || receipt.Component != component || receipt.Version == "" || err != nil || len(commit) != 20 || strings.ToLower(receipt.Commit) != receipt.Commit {
		return InstallAssessment{}, fmt.Errorf("%w: receipt identity", ErrInstallation)
	}
	inventory := receipt.Resources
	if runtimeQueue {
		if component != "addon" {
			return InstallAssessment{}, ErrInstallation
		}
		inventory = append([]Resource(nil), inventory...)
		empty, _ := bridge.EncodeProbeQueue(nil)
		emptyHash := fmt.Sprintf("%x", sha256.Sum256(empty))
		queueIndex := -1
		for i, entry := range inventory {
			if entry.Path != "addon/Bridge/Definitions.lua" {
				continue
			}
			if queueIndex >= 0 || entry.Bytes != int64(len(empty)) || entry.SHA256 != emptyHash {
				return InstallAssessment{}, fmt.Errorf("%w: queue release baseline", ErrInstallation)
			}
			queueIndex = i
		}
		if queueIndex < 0 {
			return InstallAssessment{}, fmt.Errorf("%w: missing queue baseline", ErrInstallation)
		}
		info, err := root.Lstat("Bridge/Definitions.lua")
		if err != nil {
			return InstallAssessment{}, err
		}
		if !info.Mode().IsRegular() || info.Size() > bridge.MaxProbeQueueFileBytes {
			return InstallAssessment{}, ErrInstallation
		}
		queue, err := root.Open("Bridge/Definitions.lua")
		if err != nil {
			return InstallAssessment{}, err
		}
		data, readErr := io.ReadAll(&cancelReader{ctx: ctx, reader: io.LimitReader(queue, bridge.MaxProbeQueueFileBytes+1)})
		closeErr := queue.Close()
		if err := errors.Join(readErr, closeErr); err != nil {
			return InstallAssessment{}, err
		}
		definitions, err := bridge.DecodeProbeQueue(bytes.NewReader(data))
		if err != nil {
			return InstallAssessment{}, fmt.Errorf("%w: invalid runtime queue", ErrInstallation)
		}
		for _, definition := range definitions {
			if definition.Release != receipt.Version {
				return InstallAssessment{}, fmt.Errorf("%w: queue release mismatch", ErrInstallation)
			}
		}
		inventory[queueIndex].Bytes = int64(len(data))
		inventory[queueIndex].SHA256 = fmt.Sprintf("%x", sha256.Sum256(data))
	}
	if err := verifyInventory(ctx, directory, inventory, component); err != nil {
		if errors.Is(err, ErrPayload) {
			return InstallAssessment{State: "modified", Receipt: &receipt, Detail: err.Error()}, nil
		}
		return InstallAssessment{}, err
	}
	return InstallAssessment{State: "managed", Receipt: &receipt}, nil
}
