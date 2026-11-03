package records

import (
	"context"
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"testing"

	"github.com/follenfang/lycheedev/internal/records/schema"
	"github.com/follenfang/lycheedev/internal/records/sqlengine"
	"github.com/follenfang/lycheedev/internal/selection"
	fielddigest "github.com/follenfang/lycheedev/tests/dbcd/digest"
)

// The independent oracle is opt-in locally and required by the dedicated DBCD
// CI check. It applies real XFTH bytes through DBCD and the production overlay.
func TestPinnedDBCDHotfixDifferential(t *testing.T) {
	oracle, dotnet, fixtures := os.Getenv("LYCHEEDEV_DBCD_ORACLE"), os.Getenv("LYCHEEDEV_DOTNET"), os.Getenv("LYCHEEDEV_DBCD_FIXTURES")
	if oracle == "" || dotnet == "" || fixtures == "" {
		if os.Getenv("LYCHEEDEV_REQUIRE_DBCD") == "1" {
			t.Fatal("pinned DBCD oracle, dotnet and fixture paths required")
		}
		t.Skip("pinned DBCD oracle paths not configured")
	}
	oracle, _ = filepath.Abs(oracle)
	dotnet, _ = filepath.Abs(dotnet)
	fixtures, _ = filepath.Abs(fixtures)
	ctx := context.Background()
	for _, version := range []uint32{7, 8, 9} {
		for _, tc := range []struct {
			name   string
			states []uint8
		}{
			{"base", []uint8{4}}, {"valid", []uint8{1, 4}}, {"deleted", []uint8{2, 4}}, {"then-valid", []uint8{4, 1}},
		} {
			t.Run(fmt.Sprintf("v%d/%s", version, tc.name), func(t *testing.T) {
				raw := dbcdCache(version, tc.states)
				dir := t.TempDir()
				cache := filepath.Join(dir, "cache.bin")
				output := filepath.Join(dir, "oracle.json")
				if err := os.WriteFile(cache, raw, 0600); err != nil {
					t.Fatal(err)
				}
				cmd := exec.CommandContext(ctx, dotnet, oracle, fixtures, "CommonBoth", "12.1.0.69875", output, "-", cache)
				if log, err := cmd.CombinedOutput(); err != nil {
					t.Fatalf("oracle: %v\n%s", err, log)
				}
				body, err := os.ReadFile(output)
				if err != nil {
					t.Fatal(err)
				}
				var expected struct {
					SourceCommit string
					FieldDigest  string
					Count        int
					HotfixSHA256 string
				}
				if err := json.Unmarshal(body, &expected); err != nil {
					t.Fatal(err)
				}
				sum := sha256.Sum256(raw)
				if expected.SourceCommit != "c0aeb5f6be0e9fbffc8eadb4f8b9b4e36b42af7c" || expected.HotfixSHA256 != hex.EncodeToString(sum[:]) {
					t.Fatal("oracle identity mismatch")
				}
				page, err := InspectCache(ctx, raw, CacheFilter{Build: 69875, Limit: 200})
				if err != nil {
					t.Fatal(err)
				}
				snapshots := newHotfixSnapshots(nil, nil, selection.DataPin{}, nil)
				snapshots.captures["cache"] = &hotfixSnapshot{raw: raw, layout: version, tables: map[uint32][]CacheEntry{0xaabbccdd: page.Entries}}
				base := sqlengine.Source{Columns: []string{"ID", "Value"}, Scan: func(_ context.Context, y func([]any) error) error {
					for _, row := range [][]any{{uint64(7), uint64(99)}, {uint64(900), uint64(77)}} {
						if err := y(row); err != nil {
							return err
						}
					}
					return nil
				}}
				definition := schema.Definition{Fields: []schema.Field{{Name: "ID", Kind: "int", Identity: true, Bits: 32, Elements: 1}, {Name: "Value", Kind: "int", Inline: true, Bits: 32, Elements: 1}}}
				bundle := DefinitionBundle{}
				bundle.Identity.Hash = 0xaabbccdd
				source, _, err := effectiveSourceWithSnapshots(ctx, snapshots, base, definition, bundle, []string{"cache"})
				if err != nil {
					t.Fatal(err)
				}
				digest := sha256.New()
				count := 0
				err = source.Scan(ctx, func(row []any) error {
					count++
					fielddigest.Row(digest, uint32(row[0].(uint64)), map[string]any{"ID": row[0], "Value": row[1]})
					return nil
				})
				if err != nil || count != expected.Count || hex.EncodeToString(digest.Sum(nil)) != expected.FieldDigest {
					t.Fatalf("count=%d digest=%x oracle=%+v err=%v", count, digest.Sum(nil), expected, err)
				}
			})
		}
	}
}
func dbcdCache(version uint32, states []uint8) []byte {
	raw := make([]byte, 44)
	copy(raw, "XFTH")
	binary.LittleEndian.PutUint32(raw[4:], version)
	binary.LittleEndian.PutUint32(raw[8:], 69875)
	for i, state := range states {
		header := make([]byte, 24+4*(version-7))
		copy(header, "XFTH")
		push, unique, table, record, size, status := 4, -1, 8, 12, 16, 20
		if version == 8 {
			unique, table, record, size, status = 8, 12, 16, 20, 24
		}
		if version == 9 {
			push, unique, table, record, size, status = 8, 12, 16, 20, 24, 28
		}
		binary.LittleEndian.PutUint32(header[push:], uint32(i+1))
		if unique >= 0 {
			binary.LittleEndian.PutUint32(header[unique:], uint32(i+1))
		}
		binary.LittleEndian.PutUint32(header[table:], 0xaabbccdd)
		binary.LittleEndian.PutUint32(header[record:], 7)
		header[status] = state
		var payload []byte
		if state == 1 {
			payload = binary.LittleEndian.AppendUint32(nil, 123)
			binary.LittleEndian.PutUint32(header[size:], 4)
		}
		raw = append(raw, header...)
		raw = append(raw, payload...)
	}
	return raw
}
