package schema_test

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/records/schema"
)

const manifestFixture = `[
  {"tableName":"Character","tableHash":"A1B2C3D4","db2FileDataID":123,"dbcFileDataID":456},
  {"tableName":"Other_Table2","tableHash":"a1b2c3d4","db2FileDataID":789},
  {"tableName":"Legacy","tableHash":"00000001","dbcFileDataID":999}
]`

func TestManifestSelectHash(t *testing.T) {
	doc, err := schema.ParseManifest(context.Background(), []byte(manifestFixture))
	if err != nil {
		t.Fatal(err)
	}
	if _, err := doc.SelectHash(context.Background(), 0xA1B2C3D4); !errors.Is(err, schema.ErrAmbiguous) {
		t.Fatal(err)
	}
	entry, err := doc.SelectHash(context.Background(), 1)
	if err != nil || entry.Name != "Legacy" || entry.DBCFileDataID != 999 {
		t.Fatal(entry, err)
	}
	if _, err := doc.SelectHash(context.Background(), 0); !errors.Is(err, schema.ErrMissing) {
		t.Fatal(err)
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err := doc.SelectHash(ctx, 1); !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
}

func TestManifestSHA256ResolutionAndOwnership(t *testing.T) {
	raw := []byte(manifestFixture)
	doc, err := schema.ParseManifest(context.Background(), raw)
	if err != nil {
		t.Fatal(err)
	}
	if doc == nil {
		t.Fatal("ParseManifest returned a nil manifest")
	}

	digest := sha256.Sum256(raw)
	if got, want := doc.SHA256(), hex.EncodeToString(digest[:]); got != want {
		t.Fatalf("SHA256() = %q, want %q", got, want)
	}

	identity, err := doc.Resolve(context.Background(), "cHaRaCtEr", 123, 0xA1B2C3D4)
	if err != nil {
		t.Fatal(err)
	}
	want := schema.TableIdentity{Name: "Character", Hash: 0xA1B2C3D4, DB2FileDataID: 123, DBCFileDataID: 456}
	if identity != want {
		t.Fatalf("Resolve() = %+v, want %+v", identity, want)
	}

	// A returned identity is a value owned by the caller; changing it must not
	// alter the manifest's indexed identity.
	identity.Name = "changed"
	identity.Hash = 0
	identity.DB2FileDataID = 0
	identity.DBCFileDataID = 0
	again, err := doc.Resolve(context.Background(), "CHARACTER", 123, 0xA1B2C3D4)
	if err != nil {
		t.Fatal(err)
	}
	if again != want {
		t.Fatalf("Resolve() after mutating prior result = %+v, want %+v", again, want)
	}

	// The manifest must not retain the caller's raw byte slice as mutable state.
	raw[0] = 'x'
	if got, want := doc.SHA256(), hex.EncodeToString(digest[:]); got != want {
		t.Fatalf("SHA256() after mutating raw input = %q, want %q", got, want)
	}

	// Equal hashes are allowed when the table identities differ.
	if got, err := doc.Resolve(context.Background(), "other_table2", 789, 0xA1B2C3D4); err != nil || got.Name != "Other_Table2" {
		t.Fatalf("hash collision lookup = %+v, %v", got, err)
	}
}

func TestManifestAcceptsHyphenatedTableName(t *testing.T) {
	doc, err := schema.ParseManifest(context.Background(), []byte(`[{"tableName":"Item-sparse","tableHash":"10203040","db2FileDataID":321}]`))
	if err != nil {
		t.Fatal(err)
	}
	got, err := doc.Resolve(context.Background(), "item-SPARSE", 321, 0x10203040)
	if err != nil {
		t.Fatal(err)
	}
	want := schema.TableIdentity{Name: "Item-sparse", Hash: 0x10203040, DB2FileDataID: 321}
	if got != want {
		t.Fatalf("Resolve() = %+v, want %+v", got, want)
	}
}

func TestManifestResolveErrorsAndCancellation(t *testing.T) {
	doc, err := schema.ParseManifest(context.Background(), []byte(manifestFixture))
	if err != nil {
		t.Fatal(err)
	}

	for _, test := range []struct {
		name   string
		fileID uint32
		hash   uint32
		want   error
	}{
		{name: "missing", fileID: 123, hash: 0xA1B2C3D4, want: schema.ErrMissing},
		{name: "", fileID: 123, hash: 0xA1B2C3D4, want: schema.ErrFormat},
		{name: "Character", fileID: 0, hash: 0xA1B2C3D4, want: schema.ErrFormat},
		{name: "Character", fileID: 124, hash: 0xA1B2C3D4, want: schema.ErrFormat},
		{name: "Character", fileID: 123, hash: 0xA1B2C3D5, want: schema.ErrFormat},
	} {
		t.Run(test.name, func(t *testing.T) {
			got, err := doc.Resolve(context.Background(), test.name, test.fileID, test.hash)
			if !errors.Is(err, test.want) || got != (schema.TableIdentity{}) {
				t.Fatalf("Resolve() = %+v, %v; want zero identity and %v", got, err, test.want)
			}
		})
	}

	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if got, err := schema.ParseManifest(ctx, []byte(manifestFixture)); !errors.Is(err, context.Canceled) || got != nil {
		t.Fatalf("canceled ParseManifest() = %v, %v; want context.Canceled and nil", got, err)
	}
	if got, err := doc.Resolve(ctx, "Character", 123, 0xA1B2C3D4); !errors.Is(err, context.Canceled) || got != (schema.TableIdentity{}) {
		t.Fatalf("canceled Resolve() = %+v, %v; want context.Canceled and zero identity", got, err)
	}
}

func TestManifestRejectsInvalidVariants(t *testing.T) {
	tooLongName := strings.Repeat("a", 129)
	invalid := []string{
		"null",
		"{}",
		"[null]",
		"[[]]",
		"[{}]",
		`[{"tableHash":"00000001"}]`,
		`[{"tableName":null,"tableHash":"00000001"}]`,
		`[{"tableName":1,"tableHash":"00000001"}]`,
		`[{"tableName":"","tableHash":"00000001"}]`,
		`[{"tableName":"1A","tableHash":"00000001"}]`,
		`[{"tableName":"bad name","tableHash":"00000001"}]`,
		fmt.Sprintf(`[{"tableName":%q,"tableHash":"00000001"}]`, tooLongName),
		`[{"tableName":"é","tableHash":"00000001"}]`,
		`[{"tableName":"A","tableHash":null}]`,
		`[{"tableName":"A","tableHash":1}]`,
		`[{"tableName":"A","tableHash":"1234567"}]`,
		`[{"tableName":"A","tableHash":"123456789"}]`,
		`[{"tableName":"A","tableHash":"GGGGGGGG"}]`,
		`[{"tableName":"A","tableHash":"00000001","db2FileDataID":null}]`,
		`[{"tableName":"A","tableHash":"00000001","db2FileDataID":0}]`,
		`[{"tableName":"A","tableHash":"00000001","db2FileDataID":-1}]`,
		`[{"tableName":"A","tableHash":"00000001","db2FileDataID":4294967296}]`,
		`[{"tableName":"A","tableHash":"00000001","db2FileDataID":1.5}]`,
		`[{"tableName":"A","tableHash":"00000001","db2FileDataID":"1"}]`,
		`[{"tableName":"A","tableHash":"00000001","dbcFileDataID":null}]`,
		`[{"tableName":"A","tableHash":"00000001","dbcFileDataID":0}]`,
		`[{"tableName":"A","tableHash":"00000001","dbcFileDataID":-1}]`,
		`[{"tableName":"A","tableHash":"00000001","dbcFileDataID":4294967296}]`,
		`[{"tableName":"A","tableHash":"00000001","dbcFileDataID":1.5}]`,
		`[{"tableName":"A","tableHash":"00000001","unexpected":true}]`,
		`[{"tableName":"A","tableName":"B","tableHash":"00000001"}]`,
		`[{"tableName":"A","tableHash":"00000001","db2FileDataID":1,"db2FileDataID":2}]`,
		`[{"tableName":"A","tableHash":"00000001","dbcFileDataID":1,"dbcFileDataID":2}]`,
		`[{"tableName":"A","tableHash":"00000001"},{"tableName":"a","tableHash":"00000002"}]`,
		`[{"tableName":"A","tableHash":"00000001","db2FileDataID":1},{"tableName":"B","tableHash":"00000002","db2FileDataID":1}]`,
		`[{"tableName":"A","tableHash":"00000001","dbcFileDataID":1},{"tableName":"B","tableHash":"00000002","dbcFileDataID":1}]`,
	}
	for index, raw := range invalid {
		t.Run(fmt.Sprintf("invalid_%02d", index), func(t *testing.T) {
			got, err := schema.ParseManifest(context.Background(), []byte(raw))
			if !errors.Is(err, schema.ErrFormat) || got != nil {
				t.Fatalf("ParseManifest(%s) = %v, %v; want ErrFormat and nil", raw, got, err)
			}
		})
	}
}

func TestManifestLimitsAndEntryBoundary(t *testing.T) {
	if got, err := schema.ParseManifest(context.Background(), bytes.Repeat([]byte{' '}, (4<<20)+1)); !errors.Is(err, schema.ErrLimit) || got != nil {
		t.Fatalf("oversized ParseManifest() = %v, %v; want ErrLimit and nil", got, err)
	}

	within := manifestEntries(16384)
	if got, err := schema.ParseManifest(context.Background(), within); err != nil || got == nil {
		t.Fatalf("16384-entry ParseManifest() = %v, %v; want success", got, err)
	}
	over := manifestEntries(16385)
	if got, err := schema.ParseManifest(context.Background(), over); !errors.Is(err, schema.ErrLimit) || got != nil {
		t.Fatalf("16385-entry ParseManifest() = %v, %v; want ErrLimit and nil", got, err)
	}
}

func manifestEntries(count int) []byte {
	var builder strings.Builder
	builder.Grow(count * 72)
	builder.WriteByte('[')
	for index := 0; index < count; index++ {
		if index != 0 {
			builder.WriteByte(',')
		}
		fmt.Fprintf(&builder, `{"tableName":"T%d","tableHash":"%08X","db2FileDataID":%d}`, index, index+1, index+1)
	}
	builder.WriteByte(']')
	return []byte(builder.String())
}

func FuzzManifestBoundedRaw(f *testing.F) {
	f.Add([]byte(manifestFixture))
	f.Add([]byte(`[]`))
	f.Add([]byte(`[{"tableName":"A","tableHash":"00000001","db2FileDataID":1}]`))
	f.Fuzz(func(t *testing.T, raw []byte) {
		if len(raw) > 4<<20 {
			t.Skip()
		}
		doc, err := schema.ParseManifest(context.Background(), raw)
		if err != nil {
			return
		}
		if doc == nil {
			t.Fatal("successful ParseManifest returned nil manifest")
		}
		_, _ = doc.Resolve(context.Background(), "A", 1, 1)
	})
}
