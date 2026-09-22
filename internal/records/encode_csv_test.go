package records_test

import (
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"strings"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/records"
)

func TestEncodeCSVGoldenAndManifest(t *testing.T) {
	request := records.CSVExportRequest{
		Table: records.CSVTable{
			Columns: []string{"id", "name", "note"},
			Rows: [][]string{
				{"1", "plain", "has,comma"},
				{"2", `say "hi"`, "line"},
				{"3", "multi\nline", ""},
			},
		},
		Source: records.CSVSourceIdentity{
			Provider:  "wago",
			Product:   "retail",
			FullBuild: "12.0.7.68974",
			Region:    "us",
			Locale:    "enUS",
			Ref:       strings.Repeat("a", 64),
			FetchedAt: time.Date(2026, 8, 7, 12, 0, 0, 0, time.UTC),
		},
		Complete: true,
	}
	var buffer bytes.Buffer
	manifest, err := records.EncodeCSV(&buffer, request)
	if err != nil {
		t.Fatal(err)
	}
	golden := "id,name,note\r\n" +
		"1,plain,\"has,comma\"\r\n" +
		"2,\"say \"\"hi\"\"\",line\r\n" +
		"3,\"multi\r\nline\",\r\n"
	if buffer.String() != golden {
		t.Fatalf("csv = %q, want %q", buffer.String(), golden)
	}
	sum := sha256.Sum256(buffer.Bytes())
	if manifest.Schema != records.CSVExportSchema || manifest.RowCount != 3 {
		t.Fatalf("manifest = %+v", manifest)
	}
	if manifest.Content.SHA256 != hex.EncodeToString(sum[:]) || manifest.Content.Bytes != int64(buffer.Len()) {
		t.Fatalf("manifest content = %+v for %d bytes", manifest.Content, buffer.Len())
	}
	if len(manifest.Columns) != 3 || manifest.Columns[0] != "id" || !manifest.Complete || manifest.Truncated {
		t.Fatalf("manifest = %+v", manifest)
	}
	if manifest.Source.Provider != "wago" || manifest.Source.Ref != strings.Repeat("a", 64) {
		t.Fatalf("manifest source = %+v", manifest.Source)
	}
	var dumped bytes.Buffer
	if err := records.WriteCSVManifest(&dumped, manifest); err != nil {
		t.Fatal(err)
	}
	var roundtrip records.CSVExportManifest
	if err := json.Unmarshal(dumped.Bytes(), &roundtrip); err != nil {
		t.Fatal(err)
	}
	if roundtrip.Content != manifest.Content || roundtrip.RowCount != manifest.RowCount {
		t.Fatalf("manifest roundtrip = %+v", roundtrip)
	}
}

func TestEncodeCSVRejectsBadTables(t *testing.T) {
	source := records.CSVSourceIdentity{Provider: "wago", Ref: strings.Repeat("a", 64)}
	cases := map[string]records.CSVExportRequest{
		"complete and truncated":   {Table: records.CSVTable{Columns: []string{"a"}}, Source: source, Complete: true, Truncated: true},
		"truncated without reason": {Table: records.CSVTable{Columns: []string{"a"}}, Source: source, Truncated: true},
		"no columns":               {Table: records.CSVTable{}, Source: source, Complete: true},
		"duplicate column":         {Table: records.CSVTable{Columns: []string{"a", "a"}}, Source: source, Complete: true},
		"empty column":             {Table: records.CSVTable{Columns: []string{""}}, Source: source, Complete: true},
		"row width":                {Table: records.CSVTable{Columns: []string{"a"}, Rows: [][]string{{"1", "2"}}}, Source: source, Complete: true},
		"invalid utf8":             {Table: records.CSVTable{Columns: []string{"a"}, Rows: [][]string{{string([]byte{0xff})}}}, Source: source, Complete: true},
	}
	for name, request := range cases {
		var buffer bytes.Buffer
		if _, err := records.EncodeCSV(&buffer, request); err == nil {
			t.Fatalf("%s: accepted", name)
		}
	}
}

func TestEncodeHotfixCSVColumns(t *testing.T) {
	region := uint32(71)
	result := records.RemoteHotfixResult{
		Query: records.HotfixQuery{Product: "retail", FullBuild: "12.0.7.68974", Region: "us", Locale: "enUS"},
		Records: []records.HotfixRecord{
			{Provider: "wago", ID: 25567226914, Push: 110656, RecordID: 26054, TableName: "Vehicle,Seat", Status: 1, Build: "68974", RegionID: &region, Locale: "enUS", PayloadLength: 640},
			{Provider: "dbcache", Push: -3, RecordID: 55, Status: 2, Build: "68974", PayloadLength: 2},
		},
		Change:   records.HotfixChange("wago", records.HotfixQuery{Product: "retail", FullBuild: "12.0.7.68974", Region: "us", Locale: "enUS"}, time.Date(2026, 8, 7, 12, 0, 0, 0, time.UTC), strings.Repeat("b", 64)),
		Complete: true,
	}
	var buffer bytes.Buffer
	manifest, err := records.EncodeHotfixCSV(&buffer, result)
	if err != nil {
		t.Fatal(err)
	}
	if strings.Join(manifest.Columns, ",") != strings.Join(records.HotfixCSVColumns, ",") {
		t.Fatalf("columns = %v", manifest.Columns)
	}
	lines := strings.Split(strings.TrimSuffix(buffer.String(), "\r\n"), "\r\n")
	if lines[0] != "id,push_id,record_id,table_name,status,build,region_id,locale,payload_length" {
		t.Fatalf("header = %q", lines[0])
	}
	if lines[1] != "25567226914,110656,26054,\"Vehicle,Seat\",1,68974,71,enUS,640" {
		t.Fatalf("wago row = %q", lines[1])
	}
	// Absent optional numerics render empty instead of a fabricated zero.
	if lines[2] != ",-3,55,,2,68974,,,2" {
		t.Fatalf("cache row = %q", lines[2])
	}
	if manifest.Source.Provider != "wago" || manifest.Source.Ref != strings.Repeat("b", 64) || manifest.RowCount != 2 {
		t.Fatalf("manifest = %+v", manifest)
	}
}
