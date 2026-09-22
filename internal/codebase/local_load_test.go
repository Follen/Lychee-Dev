package codebase

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"os"
	"path/filepath"
	"testing"

	"github.com/follenfang/lycheedev/internal/vault"
)

func TestLocalLoadDoesNotClaimArchivedEvidence(t *testing.T) {
	files := map[string][]byte{"Addon.toc": []byte("UI.xml\n"), "UI.xml": []byte(`<Ui><Script file="Core.lua" /></Ui>`), "Core.lua": []byte("local name, ns = ...\n")}
	root := writeClosureFixture(t, files)
	input := AddonInput{Root: root, Manifest: "Addon.toc"}
	local, err := InspectLocalLoad(context.Background(), input)
	if err != nil || !local.LoadValid || local.CompatibilityComplete || len(local.Documents) != 3 {
		t.Fatalf("%+v %v", local, err)
	}
	for _, document := range local.Documents {
		digest := sha256.Sum256(files[document.Path])
		if document.Archived || document.Blob.SHA256 != hex.EncodeToString(digest[:]) {
			t.Fatalf("bad evidence: %+v", document)
		}
	}
	entries, err := os.ReadDir(root)
	if err != nil || len(entries) != len(files) {
		t.Fatalf("unexpected writes: %v %v", entries, err)
	}
	store, err := vault.Initialize(context.Background(), filepath.Join(t.TempDir(), "home"))
	if err != nil {
		t.Fatal(err)
	}
	archived, err := OpenChecker(store).InspectAddon(context.Background(), input)
	if err != nil {
		t.Fatal(err)
	}
	for index, document := range archived.Documents {
		if !document.Archived || document.Blob != local.Documents[index].Blob {
			t.Fatalf("archive distinction: %+v", document)
		}
	}
	if _, err := OpenChecker(nil).InspectAddon(context.Background(), input); err == nil {
		t.Fatal("archiving silently skipped")
	}
}
