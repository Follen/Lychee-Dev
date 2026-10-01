package navigatetest

import (
	"context"
	"encoding/binary"
	"fmt"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/records"
	"github.com/follenfang/lycheedev/internal/vault"
)

// This benchmark runs the actual authenticated fixture preparation chain:
// SQLite/blob -> offline CDN fragment -> Encoding/Root/BLTE -> DBD/WDC -> query
// -> evidence. Synthetic storage bytes are intentional, not a real-client or
// network latency benchmark. Clearing decoded metadata is outside the timer.
func BenchmarkDataPreparation(b *testing.B) {
	for _, physical := range []string{"offline-cdn", "local-archive"} {
		for _, scenario := range []string{"single", "join", "static-effective", "hotfix-delete", "spell-navigation"} {
			for _, state := range []string{"cold-decoded", "warm-decoded"} {
				b.Run(physical+"/"+scenario+"/"+state, func(b *testing.B) {
					specs := benchmarkTables(1000)
					if scenario == "spell-navigation" {
						specs = chargeFixtures()
					}
					fx := newFixture(b, specs)
					query := fixtureQuery()
					if physical == "local-archive" {
						query.CDN = false
						query.Installation = fixtureInstallation(b, fx)
					}
					request := records.DataQuery{SQL: "SELECT COUNT(*) AS Count FROM Sample"}
					switch scenario {
					case "join":
						request.SQL = "SELECT s.ID,r.Value FROM Sample s JOIN Related r ON s.ID=r.ID ORDER BY s.ID LIMIT 20"
					case "static-effective":
						request.SQL = "SELECT s.ID,s.Value,e.Value FROM static.Sample s LEFT JOIN effective.Sample e ON s.ID=e.ID ORDER BY s.ID LIMIT 20"
						request.Hotfix = []string{benchmarkDeletionCapture(b, fx, 500)}
					case "hotfix-delete":
						request.SQL = "SELECT COUNT(*) AS Count FROM effective.Sample"
						request.Hotfix = []string{benchmarkDeletionCapture(b, fx, 500)}
					}
					run := func() records.QueryResult {
						if scenario == "spell-navigation" {
							result, err := records.InspectSpellInfo(context.Background(), fx.workspace, fx.pin.ID, query, records.SpellInfoRequest{SpellID: 100})
							if err != nil || result.Result.TotalCount != 2 {
								b.Fatalf("navigation: %+v %v", result.Result, err)
							}
							return records.QueryResult{Resources: result.Result.DataContext.Resources}
						}
						result, err := records.QueryData(context.Background(), fx.workspace, fx.pin.ID, query, request)
						if err != nil || !result.Result.Complete {
							b.Fatalf("query: %+v %v", result.Result, err)
						}
						rows := result.Result.Result.Rows
						expected := "1000"
						if scenario == "hotfix-delete" {
							expected = "500"
						}
						if scenario == "single" || scenario == "hotfix-delete" {
							if len(rows) != 1 || len(rows[0]) != 1 || fmt.Sprint(rows[0][0]) != expected {
								b.Fatalf("unexpected aggregate %v", rows)
							}
						} else if len(rows) != 20 {
							b.Fatalf("expected 20 joined rows, got %d", len(rows))
						}
						if scenario == "static-effective" || scenario == "hotfix-delete" {
							if len(result.Result.EffectiveSources) != 1 || result.Result.EffectiveSources[0].Deleted != 500 {
								b.Fatal("Hotfix deletion scenario lost its overlay")
							}
						}
						return result.Result
					}
					if state == "warm-decoded" {
						run()
					}
					b.ReportAllocs()
					b.ResetTimer()
					var prepareMS, definitionsMS, executeMS float64
					var sourceBytes, networkBytes, networkRequests, decodeWork, cacheHits, reusedBytes int64
					var chargedRetained, chargedMetadata int64
					var hashCalls, hashBytes, decodeCalls, extractedBytes, blobReads, blobBytes, fragmentLoads, blockHits int64
					for i := 0; i < b.N; i++ {
						if state == "cold-decoded" {
							b.StopTimer()
							clearDecodedMetadata(b, fx.workspace)
							b.StartTimer()
						}
						result := run()
						prepareMS += result.Timings.PreparationMS
						definitionsMS += result.Timings.DefinitionsMS
						executeMS += result.Timings.ExecutionMS
						sourceBytes += result.Timings.DecodedSourceBytes
						networkBytes += result.Resources.NetworkBytes
						networkRequests += result.Resources.NetworkRequests
						decodeWork += result.Resources.DecodeWork
						chargedRetained += result.Resources.RetainedBytes
						chargedMetadata += result.Resources.MetadataBytes
						seen := map[uint32]bool{}
						for _, source := range result.Sources {
							id := source.File.Entry.FileDataID
							if seen[id] {
								continue
							}
							seen[id] = true
							if source.File.DecodedCache != nil {
								cacheHits += int64(source.File.DecodedCache.Hits)
								reusedBytes += source.File.DecodedCache.ReusedBytes
								s := source.File.DecodedCache
								hashCalls += s.ContentHashCalls
								hashBytes += s.ContentHashBytes
								decodeCalls += s.ContentDecodeCalls
								extractedBytes += s.ExtractedBytes
								blobReads += s.BlobReads
								blobBytes += s.BlobReadBytes
								fragmentLoads += s.FragmentLoads
								blockHits += s.BlockCacheHits
							}
						}
					}
					b.StopTimer()
					n := float64(b.N)
					b.ReportMetric(float64(networkBytes)/n, "network-B/op")
					b.ReportMetric(float64(networkRequests)/n, "requests/op")
					b.ReportMetric(float64(decodeWork)/n, "charged-work/op")
					b.ReportMetric(float64(chargedRetained)/n, "charged-retained-B/op")
					b.ReportMetric(float64(chargedMetadata)/n, "charged-metadata-B/op")
					if scenario != "spell-navigation" {
						b.ReportMetric(prepareMS/n, "prepare-ms/op")
						b.ReportMetric(definitionsMS/n, "definitions-ms/op")
						b.ReportMetric(executeMS/n, "execute-ms/op")
						b.ReportMetric(float64(sourceBytes)/n, "source-B/op")
						b.ReportMetric(float64(cacheHits)/n, "decoded-hits/op")
						b.ReportMetric(float64(reusedBytes)/n, "decoded-reused-B/op")
						b.ReportMetric(float64(hashCalls)/n, "content-hashes/op")
						b.ReportMetric(float64(hashBytes)/n, "content-hash-B/op")
						b.ReportMetric(float64(decodeCalls)/n, "content-decodes/op")
						b.ReportMetric(float64(extractedBytes)/n, "extracted-B/op")
						b.ReportMetric(float64(blobReads)/n, "verified-blob-reads/op")
						b.ReportMetric(float64(blobBytes)/n, "verified-blob-B/op")
						b.ReportMetric(float64(fragmentLoads)/n, "fragment-loads/op")
						b.ReportMetric(float64(blockHits)/n, "block-hits/op")
					}
				})
			}
		}
	}
}

func TestDataPreparationFixtureCacheStates(t *testing.T) {
	for _, physical := range []string{"offline-cdn", "local-archive"} {
		t.Run(physical, func(t *testing.T) {
			fx := newFixture(t, benchmarkTables(1000))
			query := fixtureQuery()
			if physical == "local-archive" {
				query.CDN = false
				query.Installation = fixtureInstallation(t, fx)
			}
			for iteration := 0; iteration < 2; iteration++ {
				result, err := records.QueryData(context.Background(), fx.workspace, fx.pin.ID, query, records.DataQuery{SQL: "SELECT COUNT(*) FROM Sample"})
				if err != nil {
					t.Fatal(err)
				}
				if !result.Result.Complete || len(result.Result.Sources) != 1 || fmt.Sprint(result.Result.Result.Rows[0][0]) != "1000" {
					t.Fatalf("source/result: %+v", result.Result)
				}
				stats := result.Result.Sources[0].File.DecodedCache
				if stats == nil || stats.ContentHashCalls != 2 || stats.ContentHashBytes == 0 {
					t.Fatalf("missing hash measurement: %+v", stats)
				}
				if iteration == 0 && (stats.ContentDecodeCalls != 2 || stats.Hits != 0) {
					t.Fatalf("cold decode: %+v", stats)
				}
				if iteration == 1 && (stats.ContentDecodeCalls != 0 || stats.Hits != 2) {
					t.Fatalf("warm decode: %+v", stats)
				}
				if result.Result.Resources.NetworkBytes != 0 || result.Result.Resources.NetworkRequests != 0 {
					t.Fatal("offline fixture unexpectedly used network")
				}
			}
		})
	}
}

func benchmarkTables(count int) []fixtureTable {
	rows := make([][]any, count)
	for i := range rows {
		rows[i] = []any{int64(i + 1), int64(i * 3)}
	}
	columns := []fixtureColumn{{name: "ID", kind: 'i', bits: 32, identity: true}, {name: "Value", kind: 'i', bits: 32}}
	return []fixtureTable{{name: "Sample", fileDataID: 201, tableHash: 0xf00d0000, layoutHash: 0xabcd0000, columns: columns, rows: rows}, {name: "Related", fileDataID: 202, tableHash: 0xf00d0001, layoutHash: 0xabcd0001, columns: columns, rows: rows}}
}

func benchmarkDeletionCapture(t testing.TB, fx *fixture, deleted int) string {
	t.Helper()
	raw := make([]byte, 44)
	copy(raw, "XFTH")
	binary.LittleEndian.PutUint32(raw[4:], 9)
	pieces := strings.Split(fx.pin.Data.FullBuild, ".")
	build, _ := strconv.ParseUint(pieces[3], 10, 32)
	binary.LittleEndian.PutUint32(raw[8:], uint32(build))
	for id := 1; id <= deleted; id++ {
		header := make([]byte, 32)
		copy(header, "XFTH")
		binary.LittleEndian.PutUint32(header[8:], uint32(id))
		binary.LittleEndian.PutUint32(header[16:], 0xf00d0000)
		binary.LittleEndian.PutUint32(header[20:], uint32(id))
		header[28] = 2
		raw = append(raw, header...)
	}
	path := filepath.Join(t.TempDir(), "delete-cache.bin")
	if err := os.WriteFile(path, raw, 0600); err != nil {
		t.Fatal(err)
	}
	result, err := records.InspectHotfix(context.Background(), fx.workspace, fx.pin.ID, records.HotfixRequest{File: path, Table: "Sample", Offline: true, Filter: records.CacheFilter{Limit: 200}, MaxBytes: 1 << 20})
	if err != nil {
		t.Fatal(err)
	}
	return result.Result.Source.ID
}

func clearDecodedMetadata(t testing.TB, workspace string) {
	t.Helper()
	_, err := vault.WriteMetadata(context.Background(), workspace, func(_ *vault.Store, m *vault.Metadata) (bool, error) {
		for {
			docs, err := m.ListDocuments(context.Background(), "decoded-v1/", "", 64)
			if err != nil {
				return false, err
			}
			if len(docs) == 0 {
				return true, nil
			}
			changes := make([]vault.Deletion, 0, len(docs))
			for _, doc := range docs {
				changes = append(changes, vault.Deletion{Key: doc.Key, ExpectedGeneration: doc.Generation})
			}
			if err := m.DeleteDocuments(context.Background(), changes...); err != nil {
				return false, fmt.Errorf("clear decoded metadata: %w", err)
			}
		}
	})
	if err != nil {
		t.Fatal(err)
	}
}
