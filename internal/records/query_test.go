package records

import (
	"bytes"
	"context"
	"errors"
	"path/filepath"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/records/schema"
	"github.com/follenfang/lycheedev/internal/vault"
)

func TestRecordReaderRejectsUnboundDefinitionIdentity(t *testing.T) {
	ctx := context.Background()
	store, err := vault.Initialize(ctx, filepath.Join(t.TempDir(), "workspace"))
	if err != nil {
		t.Fatal(err)
	}
	put := func(raw string) vault.BlobRef {
		t.Helper()
		ref, err := store.PublishBlob(ctx, vault.BlobInput{Reader: bytes.NewBufferString(raw), MaxBytes: 4096})
		if err != nil {
			t.Fatal(err)
		}
		return ref
	}
	manifest := put(`[{"tableName":"Class","tableHash":"12345678","db2FileDataID":10}]`)
	definition := put("COLUMNS\nint ID\n\nBUILD 12.1.0.69875\nLAYOUT 12345678\n$id$ID<32>\n")
	pin := validReaderPin()
	bundle := DefinitionBundle{Commit: pin.DefinitionCommit, Identity: schema.TableIdentity{Name: "Class", Hash: 0x12345678, DB2FileDataID: 10}, Manifest: manifest, Definition: definition}
	query := FileQuery{Installation: "must-not-be-opened", FileDataID: 10, MetadataBytes: 1 << 20, ContentBytes: 1 << 20}
	for _, mutate := range []func(*DefinitionBundle, *FileQuery){
		func(b *DefinitionBundle, q *FileQuery) { b.Commit = strings.Repeat("d", 40) },
		func(b *DefinitionBundle, q *FileQuery) { b.Identity.Hash = 0 },
		func(b *DefinitionBundle, q *FileQuery) { b.Identity.DB2FileDataID = 11 },
		func(b *DefinitionBundle, q *FileQuery) { q.FileDataID = 11 },
	} {
		b, q := bundle, query
		mutate(&b, &q)
		result, err := OpenReader(store).ReadRecord(ctx, pin, q, b, 1)
		if !errors.Is(err, ErrDefinitionIdentity) || result.Row != nil {
			t.Fatalf("unbound identity: %+v %v", result, err)
		}
	}
	corrupt := bundle
	corrupt.Definition.Bytes++
	if _, err := OpenReader(store).ReadRecord(ctx, pin, query, corrupt, 1); !errors.Is(err, vault.ErrBlobIntegrity) {
		t.Fatalf("definition integrity: %v", err)
	}
}
