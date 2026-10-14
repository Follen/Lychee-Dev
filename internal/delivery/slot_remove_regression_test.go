package delivery

import (
	"bytes"
	"context"
	"os"
	"path/filepath"
	"testing"
)

func TestRemovalAcceptsRecognizedHistoricalSlotTOCBytes(t *testing.T) {
	for _, transport := range []string{"Lycheedev mailbox protocol v1", "memory-slot-v3", "lycheedev-mailbox-v1"} {
		t.Run(transport, func(t *testing.T) {
			dir := filepath.Join(t.TempDir(), "slot")
			if err := os.Mkdir(dir, 0700); err != nil {
				t.Fatal(err)
			}
			var toc []byte
			for name, content := range slotStatic(1, "3.1.2") {
				if filepath.Ext(name) == ".toc" {
					content = bytes.ReplaceAll(content, []byte(slotInterfaces), []byte(previousSlotInterfaces))
					content = bytes.ReplaceAll(content, []byte("Lycheedev mailbox protocol v1"), []byte(transport))
					toc = content
				}
				if err := os.WriteFile(filepath.Join(dir, name), content, 0600); err != nil {
					t.Fatal(err)
				}
			}
			payload := []byte("LycheeDevSlotEnvelope = nil\n")
			if err := os.WriteFile(filepath.Join(dir, "Payload.lua"), payload, 0600); err != nil {
				t.Fatal(err)
			}
			entry := SlotFile{PayloadHash: slotDigest(payload)}
			if err := validateRemovalSlot(dir, 1, "3.1.2", slotPoolSchema, entry); err != nil {
				t.Fatal(err)
			}
			archive := filepath.Join(t.TempDir(), "moved")
			if err := moveRemovalMember(context.Background(), dir, archive); err != nil {
				t.Fatal(err)
			}
			if err := validateRemovalSlot(archive, 1, "3.1.2", slotPoolSchema, entry); err != nil {
				t.Fatal(err)
			}
			got, _ := os.ReadFile(filepath.Join(archive, "Lychee Dev Slot 01.toc"))
			if !bytes.Equal(got, toc) {
				t.Fatal("archive altered original TOC")
			}
			if err := os.WriteFile(filepath.Join(archive, "Loader.lua"), []byte("modified"), 0600); err != nil {
				t.Fatal(err)
			}
			if err := validateRemovalSlot(archive, 1, "3.1.2", slotPoolSchema, entry); err == nil {
				t.Fatal("accepted modified loader")
			}
		})
	}
}
