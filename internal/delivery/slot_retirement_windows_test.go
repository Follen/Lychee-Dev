//go:build windows

package delivery

import (
	"context"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/bridge"
	"golang.org/x/sys/windows"
)

func TestSlotRetirementDoesNotTreatDeniedManifestReadAsLostMetadata(t *testing.T) {
	parent := t.TempDir()
	path := filepath.Join(parent, slotMarker)
	if err := os.WriteFile(path, []byte("{}"), 0600); err != nil {
		t.Fatal(err)
	}
	name, err := windows.UTF16PtrFromString(path)
	if err != nil {
		t.Fatal(err)
	}
	// An exclusive handle produces a real read access failure on Windows,
	// independently of administrator privileges and chmod behavior.
	handle, err := windows.CreateFile(name, windows.GENERIC_READ, 0, nil, windows.OPEN_EXISTING, windows.FILE_ATTRIBUTE_NORMAL, 0)
	if err != nil {
		t.Fatal(err)
	}
	defer windows.CloseHandle(handle)
	e := bridge.SlotEnvelope{Index: 1, Runtime: strings.Repeat("8", 32), Nonce: strings.Repeat("3", 32)}
	for kind, retire := range slotRetirementCalls(parent, e) {
		if err := retire(context.Background()); err == nil {
			t.Fatal(kind, "ignored denied manifest read")
		}
	}
}
