package navigatetest

import (
	"context"
	"crypto/md5"
	"encoding/binary"
	"encoding/hex"
	"errors"
	"os"
	"path/filepath"
	"testing"

	"github.com/follenfang/lycheedev/internal/records"
	"github.com/follenfang/lycheedev/internal/records/container"
)

// Two genuine WDC3 sections: the first is readable, the second needs a key.
// The CDN fixture authenticates the BLTE header and each encoded chunk.
func encryptedSections(t *testing.T) fixtureTable {
	t.Helper()
	spec := fixtureTable{name: "Sample", fileDataID: 201, tableHash: 0xf00d0000, layoutHash: 0xabcd0000,
		columns: []fixtureColumn{{name: "ID", kind: 'i', bits: 32, identity: true}, {name: "Value", kind: 'i', bits: 32}},
		rows:    [][]any{{int64(1), int64(42)}}}
	one, _ := buildTable(t, spec)
	start := int(binary.LittleEndian.Uint32(one[80:84])) + 40
	raw := append([]byte(nil), one[:112]...)
	raw = append(raw, make([]byte, 40)...)
	raw = append(raw, one[112:]...)
	raw = binary.LittleEndian.AppendUint32(raw, 2)
	raw = binary.LittleEndian.AppendUint32(raw, 99)
	binary.LittleEndian.PutUint32(raw[4:], 2)
	binary.LittleEndian.PutUint32(raw[32:], 2)
	binary.LittleEndian.PutUint32(raw[68:], 2)
	binary.LittleEndian.PutUint32(raw[80:], uint32(start))
	binary.LittleEndian.PutUint64(raw[112:], 1)
	binary.LittleEndian.PutUint32(raw[120:], uint32(start+8))
	binary.LittleEndian.PutUint32(raw[124:], 1)
	spec.raw = raw
	secret := []byte{'E', 8, 1, 0, 0, 0, 0, 0, 0, 0, 4, 0, 0, 0, 0, 'S', 1}
	spec.object = framedChunks([][]byte{append([]byte{'N'}, raw[:start+8]...), secret}, []int{start + 8, 8})
	return spec
}

func framedChunks(chunks [][]byte, sizes []int) []byte {
	out := make([]byte, 12+24*len(chunks))
	copy(out, "BLTE")
	binary.BigEndian.PutUint32(out[4:], uint32(len(out)))
	out[8], out[11] = 15, byte(len(chunks))
	for i, chunk := range chunks {
		offset := 12 + 24*i
		binary.BigEndian.PutUint32(out[offset:], uint32(len(chunk)))
		binary.BigEndian.PutUint32(out[offset+4:], uint32(sizes[i]))
		digest := md5.Sum(chunk)
		copy(out[offset+8:], digest[:])
		out = append(out, chunk...)
	}
	return out
}

func TestMissingKeyKeepsReadableDB2Section(t *testing.T) {
	fx := newFixture(t, []fixtureTable{encryptedSections(t)})
	result, err := records.InspectDataSearch(context.Background(), fx.workspace, fx.pin.ID, "Sample", fixtureQuery(), records.DB2SearchRequest{Field: "Value", Query: "", Limit: 10})
	if err != nil {
		t.Fatalf("readable section blocked by missing key: %v", err)
	}
	if result.Result.Complete || !result.Result.Partial || len(result.Result.Rows) != 1 || result.Result.Rows[0]["Value"] != uint64(42) {
		t.Fatalf("must retain readable row and report incomplete coverage: %+v", result.Result)
	}
	if result.Captures[0].Complete || len(result.Result.Tables[0].UnavailablePartitions) != 1 {
		t.Fatal("coverage missing from evidence")
	}
	schema, err := records.InspectDataSchema(context.Background(), fx.workspace, fx.pin.ID, "Sample", fixtureQuery())
	if err != nil || schema.Result.Complete || schema.Result.RowCount != 1 {
		t.Fatalf("schema: %+v %v", schema, err)
	}
	page, err := records.InspectDataPage(context.Background(), fx.workspace, fx.pin.ID, "Sample", fixtureQuery(), nil, 10)
	if err != nil || page.Result.Complete || page.Capture.Complete || page.Result.File.ContentVerified || page.Result.File.Content.SHA256 != "" || page.Result.File.PartialContent == nil || len(page.Result.Page.Rows) != 1 {
		t.Fatalf("page: %+v %v", page, err)
	}
	_, err = records.InspectDataRecord(context.Background(), fx.workspace, fx.pin.ID, "Sample", fixtureQuery(), 2)
	if !errors.Is(err, container.ErrKeyUnavailable) {
		t.Fatalf("missing encrypted row must not claim absence: %v", err)
	}
	query, err := records.QueryData(context.Background(), fx.workspace, fx.pin.ID, fixtureQuery(), records.DataQuery{SQL: "SELECT COUNT(*) AS n FROM Sample"})
	if err != nil || query.Result.Complete || !query.Result.Partial || query.Capture.Complete {
		t.Fatalf("SQL: %+v %v", query, err)
	}
	csv, err := records.ExportQueryCSV(context.Background(), fx.workspace, fx.pin.ID, query, filepath.Join(t.TempDir(), "partial.csv"), false)
	if err != nil || csv.Manifest.Complete || csv.Capture.Complete {
		t.Fatalf("CSV: %+v %v", csv, err)
	}
	var end *records.StreamEnd
	_, err = records.StreamDataRows(context.Background(), fx.workspace, fx.pin.ID, "Sample", fixtureQuery(), records.DB2StreamRequest{Limit: 10}, func(frame records.StreamFrame) error {
		if frame.End != nil {
			end = frame.End
		}
		return nil
	})
	if err != nil || end == nil || end.Complete || !end.Partial || end.Count != 1 {
		t.Fatalf("stream: %+v %v", end, err)
	}
	asset := fixtureQuery()
	asset.FileDataID = 201
	if _, err := records.InspectAsset(context.Background(), fx.workspace, fx.pin.ID, asset); !errors.Is(err, container.ErrKeyUnavailable) {
		t.Fatalf("raw asset must stay strict: %v", err)
	}
}

func TestExplicitKeyFileDecryptsCompleteTable(t *testing.T) {
	spec := encryptedSections(t)
	binary.LittleEndian.PutUint64(spec.raw[112:], 0x1234567890abcdef)
	// Independent PyCryptodome Salsa20 fixture keystream (cipher_test.go),
	// plaintext N + little-endian ID=2, Value=99, chunk index 1.
	ciphertext, _ := hex.DecodeString("c6ab63b7daa05cd383")
	secret := append([]byte{'E', 8, 0xef, 0xcd, 0xab, 0x90, 0x78, 0x56, 0x34, 0x12, 4, 0x11, 0x22, 0x33, 0x44, 'S'}, ciphertext...)
	end := len(spec.raw) - 8
	spec.object = framedChunks([][]byte{append([]byte{'N'}, spec.raw[:end]...), secret}, []int{end, 8})
	fx := newFixture(t, []fixtureTable{spec})
	keyFile := filepath.Join(t.TempDir(), "WoW.txt")
	if err := os.WriteFile(keyFile, []byte("1234567890abcdef 000102030405060708090a0b0c0d0e0f\n"), 0600); err != nil {
		t.Fatal(err)
	}
	q := fixtureQuery()
	q.KeyFile = keyFile
	page, err := records.InspectDataPage(context.Background(), fx.workspace, fx.pin.ID, "Sample", q, nil, 10)
	if err != nil || !page.Result.Complete || !page.Capture.Complete || !page.Result.File.ContentVerified || page.Result.File.PartialContent != nil || len(page.Result.Page.Rows) != 2 || page.Result.Page.Rows[1]["Value"] != uint64(99) {
		t.Fatalf("key file did not restore complete verified content: %+v %v", page, err)
	}
	warm, err := records.InspectDataPage(context.Background(), fx.workspace, fx.pin.ID, "Sample", q, nil, 10)
	if err != nil || warm.Result.File.DecodedCache.Hits != 2 {
		t.Fatalf("complete decode not reused: %+v %v", warm, err)
	}
	if err := os.WriteFile(keyFile, []byte("1234567890abcdef ffffffffffffffffffffffffffffffff\n"), 0600); err != nil {
		t.Fatal(err)
	}
	if changed, err := records.InspectDataPage(context.Background(), fx.workspace, fx.pin.ID, "Sample", q, nil, 10); err == nil || changed.Result.Complete {
		t.Fatal("changed key identity reused previous decrypted bytes")
	}
}

func TestMissingKeyNeverHidesCorruption(t *testing.T) {
	for _, mode := range []string{"checksum", "metadata", "wrong-section-key", "provider", "cancel"} {
		t.Run(mode, func(t *testing.T) {
			spec := encryptedSections(t)
			q := fixtureQuery()
			ctx, cancel := context.WithCancel(context.Background())
			defer cancel()
			switch mode {
			case "checksum":
				spec.object[len(spec.object)-1] ^= 1
			case "metadata":
				spec.object = framedChunks([][]byte{spec.object[len(spec.object)-17:]}, []int{len(spec.raw)})
			case "wrong-section-key":
				binary.LittleEndian.PutUint64(spec.raw[112:], 2)
				end := len(spec.raw) - 8
				spec.object = framedChunks([][]byte{append([]byte{'N'}, spec.raw[:end]...), spec.object[len(spec.object)-17:]}, []int{end, 8})
			case "provider":
				q.Keys = func(context.Context, uint64) ([]byte, error) { return nil, errors.New("private key source failed") }
			case "cancel":
				q.Keys = func(context.Context, uint64) ([]byte, error) { cancel(); return nil, container.ErrKeyUnavailable }
			}
			fx := newFixture(t, []fixtureTable{spec})
			if _, err := records.InspectDataSchema(ctx, fx.workspace, fx.pin.ID, "Sample", q); err == nil {
				t.Fatal("invalid or cancelled source accepted as a partial table")
			}
		})
	}
}
