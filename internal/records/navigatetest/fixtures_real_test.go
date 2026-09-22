package navigatetest

// Real fixed binary sample: the copied wowdata fixture
// (tests/fixtures/db2/JournalEncounterSection-classic-era.db2, provenance in
// tests/fixtures/provenance.json) is verified byte-for-byte against its
// manifest entry and driven through the same module entry points as the
// synthetic fixtures. Its zero-row shape matches the legacy empty oracle
// (wowdata fixtures/golden/go/encounter/get-1-classic-era.json).

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"os"
	"path/filepath"
	"testing"

	"github.com/follenfang/lycheedev/internal/records"
	"github.com/follenfang/lycheedev/internal/records/table"
)

const realSamplePath = "../../../tests/fixtures/db2/JournalEncounterSection-classic-era.db2"
const provenancePath = "../../../tests/fixtures/provenance.json"

type provenanceManifest struct {
	SourceRepository struct {
		Commit string `json:"commit"`
	} `json:"sourceRepository"`
	Fixtures []struct {
		Path       string `json:"path"`
		SourcePath string `json:"sourcePath"`
		Bytes      int    `json:"bytes"`
		SHA256     string `json:"sha256"`
		License    string `json:"license"`
	} `json:"fixtures"`
}

func TestRealSampleMatchesProvenanceManifest(t *testing.T) {
	raw, err := os.ReadFile(provenancePath)
	if err != nil {
		t.Fatal(err)
	}
	var manifest provenanceManifest
	if err := json.Unmarshal(raw, &manifest); err != nil {
		t.Fatal(err)
	}
	if manifest.SourceRepository.Commit != "6191d3dc567966b7a474849f3a11e7411390091c" {
		t.Fatalf("provenance commit = %s", manifest.SourceRepository.Commit)
	}
	found := false
	for _, entry := range manifest.Fixtures {
		if entry.Path == "" {
			continue
		}
		file := filepath.Join("..", "..", "..", "tests", "fixtures", filepath.FromSlash(entry.Path))
		data, err := os.ReadFile(file)
		if err != nil {
			t.Fatalf("manifest lists %s: %v", entry.Path, err)
		}
		digest := sha256.Sum256(data)
		if hex.EncodeToString(digest[:]) != entry.SHA256 {
			t.Fatalf("fixture %s digest drift: %x", entry.Path, digest)
		}
		if entry.License == "" {
			t.Fatalf("fixture %s lacks a license note", entry.Path)
		}
		if entry.SourcePath == "fixtures/golden/inputs/JournalEncounterSection-classic-era.db2" {
			found = true
		}
	}
	if !found {
		t.Fatal("the DB2 sample must be recorded in the provenance manifest")
	}
}

// The copied sample is a genuine WDC5 table: parsing must reproduce its exact
// structural identity instead of guessing defaults.
func TestRealSampleParsesAsWDC5(t *testing.T) {
	raw, err := os.ReadFile(realSamplePath)
	if err != nil {
		t.Fatal(err)
	}
	ctx := context.Background()
	budget := table.Budget{FileBytes: 1 << 20, MetadataBytes: 1 << 16, Rows: 1000, Columns: 100, Partitions: 100}
	layout, err := table.Inspect(ctx, bytes.NewReader(raw), int64(len(raw)), budget)
	if err != nil {
		t.Fatal(err)
	}
	if layout.Version != 5 || layout.Rows != 0 || layout.Columns != 15 || layout.Stride != 8 {
		t.Fatalf("layout = %+v", layout)
	}
	if layout.TableHash != 0x7956786c || layout.LayoutHash != 0xcb88312b {
		t.Fatalf("hashes = %08X %08X", layout.TableHash, layout.LayoutHash)
	}
	if string(bytes.TrimRight(layout.SchemaBuild[:], "\x00")) != "WOWSTATIC_1_15_8_63631" {
		t.Fatalf("schema build = %q", layout.SchemaBuild)
	}
	rows, err := table.OpenRecords(ctx, bytes.NewReader(raw), int64(len(raw)), budget)
	if err != nil {
		t.Fatal(err)
	}
	if rows == nil {
		t.Fatal("empty table must still open")
	}
}

// Full module chain over the real bytes: the empty result must match the
// legacy encounter oracle semantics
// (wowdata fixtures/golden/go/encounter/get-1-classic-era.json).
func TestRealSampleEmptyEncounterMatchesOracle(t *testing.T) {
	raw, err := os.ReadFile(realSamplePath)
	if err != nil {
		t.Fatal(err)
	}
	tables := []fixtureTable{{
		name: "JournalEncounterSection", fileDataID: 351,
		tableHash: 0x7956786c, layoutHash: 0xcb88312b, identityExternal: true,
		raw: raw,
		columns: []fixtureColumn{
			{name: "ID", kind: 'i', bits: 32, identity: true},
			{name: "JournalEncounterID", kind: 'i', bits: 32, relation: true, foreign: "JournalEncounter::ID"},
		},
		rows: [][]any{},
	}}
	fx := newFixture(t, tables)
	reading, err := records.InspectEncounter(context.Background(), fx.workspace, fx.pin.ID, fixtureQuery(),
		records.EncounterGetRequest{JournalEncounterID: 1})
	if err != nil {
		t.Fatal(err)
	}
	result := reading.Result
	if result.JournalEncounterID != 1 || result.SectionCount != 0 || result.SpellCount != 0 ||
		len(result.SpellIDs) != 0 || len(result.Sections) != 0 || len(result.RelatedSpells) != 0 {
		t.Fatalf("real sample oracle mismatch: %+v", result)
	}
	if !result.Complete || result.Truncated {
		t.Fatalf("empty is complete: %+v", result.DataContext)
	}
	schemaReading, err := records.InspectDataSchema(context.Background(), fx.workspace, fx.pin.ID, "JournalEncounterSection", fixtureQuery())
	if err != nil {
		t.Fatal(err)
	}
	if schemaReading.Result.RowCount != 0 || len(schemaReading.Result.Fields) != 2 {
		t.Fatalf("schema = %+v", schemaReading.Result)
	}
}
