package records_test

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/records"
	"github.com/follenfang/lycheedev/internal/records/schema"
)

func hotfixFixtureDir(t *testing.T) string {
	t.Helper()
	return filepath.Join("..", "..", "tests", "fixtures", "hotfix-wago")
}

func readHotfixFixture(t *testing.T, name string) []byte {
	t.Helper()
	raw, err := os.ReadFile(filepath.Join(hotfixFixtureDir(t), name))
	if err != nil {
		t.Fatalf("read fixture %s: %v", name, err)
	}
	return raw
}

// TestHotfixWagoFixtureProvenance pins every copied fixture to the provenance
// manifest digest, source repository and commit.
func TestHotfixWagoFixtureProvenance(t *testing.T) {
	raw := readHotfixFixture(t, "manifest.json")
	var manifest struct {
		Schema       string `json:"schema"`
		SourceRepo   string `json:"sourceRepository"`
		SourceCommit string `json:"sourceCommit"`
		SourcePath   string `json:"sourcePath"`
		LicenseNote  string `json:"licenseNote"`
		Items        []struct {
			File   string `json:"file"`
			SHA256 string `json:"sha256"`
			Bytes  int64  `json:"bytes"`
		} `json:"items"`
	}
	if err := json.Unmarshal(raw, &manifest); err != nil {
		t.Fatal(err)
	}
	if manifest.Schema != "lycheedev.fixture-provenance.v1" {
		t.Fatalf("schema = %q", manifest.Schema)
	}
	if manifest.SourceCommit != "6191d3dc567966b7a474849f3a11e7411390091c" {
		t.Fatalf("sourceCommit = %q", manifest.SourceCommit)
	}
	if manifest.SourceRepo == "" || manifest.SourcePath == "" || !strings.Contains(manifest.LicenseNote, "AGPL-3.0-or-later") {
		t.Fatalf("incomplete provenance: %+v", manifest)
	}
	if len(manifest.Items) != 3 {
		t.Fatalf("items = %d", len(manifest.Items))
	}
	for _, item := range manifest.Items {
		content := readHotfixFixture(t, item.File)
		sum := sha256.Sum256(content)
		if got := hex.EncodeToString(sum[:]); got != item.SHA256 {
			t.Fatalf("%s sha256 = %s, want %s", item.File, got, item.SHA256)
		}
		if int64(len(content)) != item.Bytes {
			t.Fatalf("%s bytes = %d, want %d", item.File, len(content), item.Bytes)
		}
	}
}

// TestHotfixWagoParseRealPages cross-checks the strict parser against the
// legacy corpus oracles without copying legacy implementation behavior.
func TestHotfixWagoParseRealPages(t *testing.T) {
	cases := []struct {
		file      string
		page      int
		lastPage  int
		total     int64
		exactMeta bool
	}{
		{file: "response-page-1.html", page: 1, lastPage: 680096, total: 17002386, exactMeta: true},
		{file: "response-page-2-real.html", page: 2, lastPage: 680111, total: 17002775, exactMeta: true},
		{file: "response-page-2.html"},
	}
	for _, test := range cases {
		page, err := records.ParseWagoPage(readHotfixFixture(t, test.file))
		if err != nil {
			t.Fatalf("%s: %v", test.file, err)
		}
		if page.ResponseSHA256 == "" {
			t.Fatalf("%s: response digest missing", test.file)
		}
		sum := sha256.Sum256(readHotfixFixture(t, test.file))
		if page.ResponseSHA256 != hex.EncodeToString(sum[:]) {
			t.Fatalf("%s: response digest mismatch", test.file)
		}
		if page.CurrentPage < 1 || page.LastPage < 100000 || page.Total < 1000000 {
			t.Fatalf("%s: unexpected pagination %d/%d/%d", test.file, page.CurrentPage, page.LastPage, page.Total)
		}
		if len(page.Records) == 0 {
			t.Fatalf("%s: no records parsed", test.file)
		}
		if test.exactMeta {
			if page.CurrentPage != test.page || page.LastPage != test.lastPage || page.Total != test.total {
				t.Fatalf("%s: meta %d/%d/%d want %d/%d/%d", test.file, page.CurrentPage, page.LastPage, page.Total, test.page, test.lastPage, test.total)
			}
		}
	}
}

// TestHotfixWagoParseRecordShape checks that independent records preserve the
// raw payload bytes and every identity field of the real page.
func TestHotfixWagoParseRecordShape(t *testing.T) {
	page, err := records.ParseWagoPage(readHotfixFixture(t, "response-page-1.html"))
	if err != nil {
		t.Fatal(err)
	}
	first := page.Records[0]
	if first.ID != 25567226914 || first.PushID != 110656 || first.RecordID != 26054 || first.Status != 1 {
		t.Fatalf("first record identity = %+v", first)
	}
	if first.Build != "68974" || first.TableName != "VehicleSeat" || first.RegionID != 71 || first.Locale != "enUS" {
		t.Fatalf("first record context = %+v", first)
	}
	if first.CreatedAt != "2026-08-06 20:40:16" {
		t.Fatalf("first record created_at = %q", first.CreatedAt)
	}
	if len(first.Data) == 0 || first.Data[0] != '[' {
		t.Fatalf("first record payload not preserved: %q", first.Data)
	}
	empty := 0
	for _, record := range page.Records {
		if record.Status == 3 && len(record.Data) == 0 {
			empty++
		}
	}
	if empty == 0 {
		t.Fatal("expected status 3 records without payload")
	}
}

func TestHotfixWagoParseRejectsProtocolChanges(t *testing.T) {
	good := wagoFixturePage(1, 1, 1, wagoRow(1, 10, 55, 1, "SpellEffect", `[1,2]`, "2026-08-05 22:09:07", 71, "enUS"))
	cases := map[string][]byte{
		"missing data-page": []byte(`<html><body><div id="app"></div></body></html>`),
		"wrong component":   wagoFixturePageComponent("Other", 1, 1, 1, ""),
		"broken json":       []byte(`<html><body><div id="app" data-page="{oops"></div></body></html>`),
		"zero page":         wagoFixturePage(0, 1, 1, ""),
		"reversed pages":    wagoFixturePage(3, 2, 1, ""),
		"negative id":       wagoFixturePage(1, 1, 1, wagoRow(-1, 10, 55, 1, "SpellEffect", "null", "2026-08-05 22:09:07", 71, "enUS")),
		"status over 255":   wagoFixturePage(1, 1, 1, wagoRow(1, 10, 55, 300, "SpellEffect", "null", "2026-08-05 22:09:07", 71, "enUS")),
		"no table name":     wagoFixturePage(1, 1, 1, wagoRow(1, 10, 55, 1, "", "null", "2026-08-05 22:09:07", 71, "enUS")),
	}
	for name, raw := range cases {
		if _, err := records.ParseWagoPage(raw); err == nil {
			t.Fatalf("%s: accepted", name)
		} else if !errors.Is(err, records.ErrHotfixWagoProtocol) {
			t.Fatalf("%s: error = %v, want records.hotfix_wago_protocol", name, err)
		}
	}
	if _, err := records.ParseWagoPage(good); err != nil {
		t.Fatalf("control page rejected: %v", err)
	}
}

func TestHotfixWagoValidatePageSequence(t *testing.T) {
	page := func(current, last int, total int64, next string) records.WagoPage {
		return records.WagoPage{CurrentPage: current, LastPage: last, Total: total, NextPageURL: next}
	}
	if err := records.ValidateWagoPageSequence(page(1, 3, 60, "/hotfixes?page=2"), page(2, 3, 60, "/hotfixes?page=3")); err != nil {
		t.Fatalf("valid sequence rejected: %v", err)
	}
	cases := map[string]error{
		"total drift":      records.ValidateWagoPageSequence(page(1, 3, 60, "/hotfixes?page=2"), page(2, 3, 61, "")),
		"last page drift":  records.ValidateWagoPageSequence(page(1, 3, 60, "/hotfixes?page=2"), page(2, 4, 60, "")),
		"missing page":     records.ValidateWagoPageSequence(page(1, 3, 60, "/hotfixes?page=2"), page(3, 3, 60, "")),
		"duplicate page":   records.ValidateWagoPageSequence(page(2, 3, 60, "/hotfixes?page=3"), page(2, 3, 60, "")),
		"missing next url": records.ValidateWagoPageSequence(page(1, 3, 60, ""), page(2, 3, 60, "")),
		"wrong next url":   records.ValidateWagoPageSequence(page(1, 3, 60, "/hotfixes?page=9"), page(2, 3, 60, "")),
	}
	for name, err := range cases {
		if err == nil || !errors.Is(err, records.ErrHotfixPageDrift) {
			t.Fatalf("%s: error = %v, want records.hotfix_page_drift", name, err)
		}
	}
}

func TestHotfixWagoPositionalDecode(t *testing.T) {
	definition := schema.Definition{
		Fields: []schema.Field{
			{Name: "ID", Kind: "int", Bits: 32, Elements: 1, Identity: true},
			{Name: "Flags", Kind: "int", Bits: 8, Signed: true, Elements: 3, Array: true},
			{Name: "Chance", Kind: "float", Bits: 32, Elements: 1},
			{Name: "Name", Kind: "locstring", Elements: 1},
			{Name: "BigID", Kind: "int", Bits: 64, Elements: 1},
		},
	}
	fields, err := records.DecodeHotfixPositional(json.RawMessage(`[26054,["0","-2","3"],"-0.69999998807907","O, \"quoted\"",25567226914]`), definition)
	if err != nil {
		t.Fatal(err)
	}
	if fields["ID"] != uint64(26054) {
		t.Fatalf("ID = %#v", fields["ID"])
	}
	flags, ok := fields["Flags"].([]any)
	if !ok || len(flags) != 3 || flags[1] != int64(-2) {
		t.Fatalf("Flags = %#v", fields["Flags"])
	}
	if fields["Chance"] != -0.69999998807907 {
		t.Fatalf("Chance = %#v", fields["Chance"])
	}
	if fields["Name"] != `O, "quoted"` {
		t.Fatalf("Name = %#v", fields["Name"])
	}
	// Exact integer precision beyond float64 survives.
	if fields["BigID"] != uint64(25567226914) {
		t.Fatalf("BigID = %#v", fields["BigID"])
	}
}

func TestHotfixWagoPositionalDecodeRejectsMisalignment(t *testing.T) {
	definition := schema.Definition{Fields: []schema.Field{
		{Name: "ID", Kind: "int", Bits: 32, Elements: 1, Identity: true},
		{Name: "Name", Kind: "string", Elements: 1},
	}}
	decode := records.DecodeHotfixPositional
	if _, err := decode(json.RawMessage(`[1,"a",99]`), definition); !errors.Is(err, records.ErrCacheFields) {
		t.Fatalf("surplus columns error = %v", err)
	}
	if _, err := decode(json.RawMessage(`[1,42]`), definition); !errors.Is(err, records.ErrCacheFields) {
		t.Fatalf("type mismatch error = %v", err)
	}
	if _, err := decode(json.RawMessage(`["notanid","a"]`), definition); !errors.Is(err, records.ErrCacheFields) {
		t.Fatalf("non numeric int error = %v", err)
	}
	if _, err := decode(json.RawMessage(`[99999999999,"a"]`), definition); !errors.Is(err, records.ErrCacheFields) {
		t.Fatalf("int overflow error = %v", err)
	}
	arrayDefinition := schema.Definition{Fields: []schema.Field{
		{Name: "Pair", Kind: "int", Bits: 32, Elements: 2, Array: true},
	}}
	if _, err := decode(json.RawMessage(`[[1]]`), arrayDefinition); !errors.Is(err, records.ErrCacheFields) {
		t.Fatalf("array length mismatch error = %v", err)
	}
	// Object payloads pass through as named fields with exact JSON numbers.
	object, err := decode(json.RawMessage(`{"ID":1}`), definition)
	if err != nil || fmt.Sprint(object["ID"]) != "1" {
		t.Fatalf("object payload = %#v err = %v", object, err)
	}
}
