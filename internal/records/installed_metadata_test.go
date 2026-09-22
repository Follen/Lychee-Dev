package records_test

import (
	"bytes"
	"context"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/records"
	"github.com/follenfang/lycheedev/internal/records/container"
	"github.com/follenfang/lycheedev/internal/records/relational"
	"github.com/follenfang/lycheedev/internal/records/schema"
	"github.com/follenfang/lycheedev/internal/records/table"
	"github.com/follenfang/lycheedev/internal/vault"
)

// Opt-in read-only evidence against an explicitly selected installed build.
// It neither reads account data nor invokes game input or old tooling.
func TestInstalledBuildMetadata(t *testing.T) {
	root := os.Getenv("LYCHEEDEV_TEST_INSTALLATION")
	if root == "" {
		t.Skip("explicit installation/product/build required")
	}
	product, build := os.Getenv("LYCHEEDEV_TEST_PRODUCT"), os.Getenv("LYCHEEDEV_TEST_BUILD")
	if product == "" || build == "" {
		t.Fatal("product and full build must be explicit")
	}
	ctx := context.Background()
	fileID := uint32(1990283)
	tableName := os.Getenv("LYCHEEDEV_TEST_TABLE")
	if tableName != "" {
		if tableName != "ChrClasses" {
			t.Fatal("typed fixture supports explicit ChrClasses only")
		}
		fileID = 1361031
	}
	var keys container.KeyLookup
	if os.Getenv("LYCHEEDEV_TEST_PUBLIC_KEYS") == "1" {
		keyCtx, cancel := context.WithTimeout(ctx, 30*time.Second)
		defer cancel()
		request, err := http.NewRequestWithContext(keyCtx, http.MethodGet, "https://raw.githubusercontent.com/wowdev/TACTKeys/89627658c0480f6ec12f82693298d564895a399d/WoW.txt", nil)
		if err != nil {
			t.Fatal(err)
		}
		client := &http.Client{CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }}
		response, err := client.Do(request)
		if err != nil {
			t.Fatal("public key source request failed")
		}
		defer response.Body.Close()
		if response.StatusCode != http.StatusOK {
			t.Fatalf("public key source HTTP %d", response.StatusCode)
		}
		set, err := records.ReadKeySet(keyCtx, response.Body, "text", "80f1460f9ac2d8504479f417e198a6381a016a406adbe3958169c10d23c5f21f")
		if err != nil {
			t.Fatal(err)
		}
		keys = set.Lookup
		t.Logf("explicit public key snapshot verified: count=%d sha256=%s", set.Count(), set.SHA256())
		_, knownErr := set.Lookup(ctx, 0x14f4b11d7b067aa2)
		t.Logf("previously missing key ID present in snapshot: %t", knownErr == nil)
	}
	metadata, err := records.ResolveLocalBuild(ctx, root, product, build)
	if err != nil {
		t.Fatal(err)
	}
	store, err := vault.Initialize(ctx, filepath.Join(t.TempDir(), "workspace"))
	if err != nil {
		t.Fatal(err)
	}
	for _, document := range []records.ConfigDocument{metadata.BuildDocument, metadata.CDNDocument} {
		ref, err := store.PublishBlob(ctx, vault.BlobInput{Reader: bytes.NewReader(document.Raw), MaxBytes: 4 << 20, ExpectedSHA256: document.SHA256})
		if err != nil {
			t.Fatal(err)
		}
		if err := store.VerifyBlob(ctx, ref); err != nil {
			t.Fatal(err)
		}
	}
	t.Logf("product=%s build=%s buildConfig=%s cdnConfig=%s root=%s encoding=%s contentBytes=%d encodedBytes=%d", metadata.Installed.Product, metadata.Installed.FullBuild, metadata.Installed.BuildConfig, metadata.Installed.CDNConfig, metadata.RootContentKey, metadata.EncodingKey, metadata.ContentBytes, metadata.EncodingBytes)
	object, err := records.OpenLocalObject(ctx, root, metadata.EncodingKey, metadata.EncodingBytes)
	if err != nil {
		t.Fatal(err)
	}
	defer object.Close()
	ranges, err := container.OpenRanges(ctx, object, metadata.EncodingBytes, container.Limits{EncodedBytes: 512 << 20, DecodedBytes: 512 << 20, ChunkBytes: 64 << 20, Chunks: 65536, Depth: 16}, nil)
	if err != nil {
		t.Fatal(err)
	}
	if ranges.Size() != metadata.ContentBytes {
		t.Fatal("encoding manifest declared size mismatch")
	}
	header, err := ranges.ReadSpan(ctx, 0, 22)
	if err != nil {
		t.Fatal(err)
	}
	if string(header[:2]) != "EN" {
		t.Fatalf("not an Encoding manifest: %x", header)
	}
	t.Logf("fixed build Encoding header verified: index=%s archive=%s header=%x", object.Index, object.Span.Filename(), header)
	index, err := records.OpenEncoding(ctx, ranges)
	if err != nil {
		t.Fatal(err)
	}
	rootRecord, err := index.FindContent(ctx, metadata.RootContentKey)
	if err != nil {
		t.Fatal(err)
	}
	t.Logf("fixed build Root mapping: ckey=%s bytes=%d alternatives=%v page=%d", rootRecord.ContentKey, rootRecord.DecodedBytes, rootRecord.EncodingKeys, rootRecord.Page)
	for _, key := range rootRecord.EncodingKeys {
		encoded, err := index.FindEncoding(ctx, key)
		if err != nil {
			t.Fatal(err)
		}
		rootObject, err := records.OpenLocalObject(ctx, root, key, encoded.EncodedBytes)
		if err != nil {
			t.Fatal(err)
		}
		ref, extractErr := records.OpenPayloadArchive(store).ExtractContent(ctx, records.ContentInput{
			Encoded: rootObject, ContentKey: rootRecord.ContentKey,
			Limits: container.Limits{EncodedBytes: encoded.EncodedBytes, DecodedBytes: rootRecord.DecodedBytes, ChunkBytes: 64 << 20, Chunks: 65536, Depth: 16},
		})
		closeErr := rootObject.Close()
		if extractErr != nil {
			t.Fatal(extractErr)
		}
		if closeErr != nil {
			t.Fatal(closeErr)
		}
		if ref.Bytes != rootRecord.DecodedBytes {
			t.Fatal("Root decoded size mismatch")
		}
		if err := store.VerifyBlob(ctx, ref); err != nil {
			t.Fatal(err)
		}
		t.Logf("Root content verified and archived: ekey=%s encodedBytes=%d blob=%+v", key, encoded.EncodedBytes, ref)
		decoded, err := store.ReadBlob(ctx, ref, 128<<20)
		if err != nil {
			t.Fatal(err)
		}
		matches, err := records.LookupRoot(ctx, bytes.NewReader(decoded), int64(len(decoded)), []uint32{fileID}, records.RootLimits{Bytes: 128 << 20, Records: 10000000, Groups: 65536, Matches: 4096})
		if err != nil {
			t.Fatal(err)
		}
		if len(matches) == 0 {
			t.Fatalf("Root lookup returned no candidates for FileDataID %d", fileID)
		}
		t.Logf("Root selected candidates: %+v", matches)
		// File extraction additionally requires a key provider for encrypted
		// sections. Keep its explicit gate distinct from Root projection coverage.
		if os.Getenv("LYCHEEDEV_TEST_FILE_CONTENT") != "1" && tableName == "" {
			t.Log("selected file extraction not requested; Root projection only")
			continue
		}
		selected := 0
		for _, candidate := range matches {
			// Explicit enUS bit for this fixture, not a locale fallback policy.
			if candidate.LocaleMask&2 == 0 {
				continue
			}
			selected++
			content, err := index.FindContent(ctx, candidate.ContentKey)
			if err != nil {
				t.Fatal(err)
			}
			for _, encodingKey := range content.EncodingKeys {
				physical, err := index.FindEncoding(ctx, encodingKey)
				if err != nil {
					t.Fatal(err)
				}
				file, err := records.OpenLocalObject(ctx, root, encodingKey, physical.EncodedBytes)
				if err != nil {
					t.Fatal(err)
				}
				payload, extractErr := records.OpenPayloadArchive(store).ExtractContent(ctx, records.ContentInput{Encoded: file, ContentKey: content.ContentKey, Keys: keys, Limits: container.Limits{EncodedBytes: physical.EncodedBytes, DecodedBytes: content.DecodedBytes, ChunkBytes: 64 << 20, Chunks: 65536, Depth: 16}})
				closeErr := file.Close()
				if extractErr != nil {
					t.Fatal(extractErr)
				}
				if closeErr != nil {
					t.Fatal(closeErr)
				}
				if payload.Bytes != content.DecodedBytes {
					t.Fatal("selected file size mismatch")
				}
				if err := store.VerifyBlob(ctx, payload); err != nil {
					t.Fatal(err)
				}
				t.Logf("selected FileDataID content verified: id=%d localeMask=%d key=%s blob=%+v", candidate.FileDataID, candidate.LocaleMask, content.ContentKey, payload)
				if tableName != "" {
					raw, err := store.ReadBlob(ctx, payload, 128<<20)
					if err != nil {
						t.Fatal(err)
					}
					verifyInstalledTable(t, ctx, raw, tableName, fileID, build)
				}
			}
		}
		if selected != 1 {
			t.Fatalf("expected exactly one enUS candidate, got %d", selected)
		}
	}
}

func verifyInstalledTable(t *testing.T, ctx context.Context, raw []byte, name string, fileID uint32, build string) {
	t.Helper()
	const source = "https://raw.githubusercontent.com/wowdev/WoWDBDefs/83057bdc0cbe13062850ebf8ad530031e128a1cd/"
	fetch := func(path string) []byte {
		t.Helper()
		bounded, cancel := context.WithTimeout(ctx, 30*time.Second)
		defer cancel()
		request, err := http.NewRequestWithContext(bounded, http.MethodGet, source+path, nil)
		if err != nil {
			t.Fatal(err)
		}
		response, err := http.DefaultClient.Do(request)
		if err != nil {
			t.Fatal(err)
		}
		defer response.Body.Close()
		if response.StatusCode != http.StatusOK {
			t.Fatalf("definition HTTP %d", response.StatusCode)
		}
		data, err := io.ReadAll(io.LimitReader(response.Body, (4<<20)+1))
		if err != nil {
			t.Fatal(err)
		}
		return data
	}
	manifest, err := schema.ParseManifest(ctx, fetch("manifest.json"))
	if err != nil {
		t.Fatalf("manifest: %v", err)
	}
	if manifest.SHA256() != "4314266fe055e30207e1c39498a7a31058a1aa2ab6cd674726491b313d135265" {
		t.Fatal("pinned manifest bytes changed")
	}
	budget := table.Budget{FileBytes: 128 << 20, MetadataBytes: 32 << 20, Rows: 1000000, Columns: 4096, Partitions: 4096}
	layout, err := table.Inspect(ctx, bytes.NewReader(raw), int64(len(raw)), budget)
	if err != nil {
		t.Fatal(err)
	}
	identity, err := manifest.Resolve(ctx, name, fileID, layout.TableHash)
	if err != nil {
		t.Fatalf("table identity hash=%08X: %v", layout.TableHash, err)
	}
	doc, err := schema.Parse(ctx, fetch("definitions/"+identity.Name+".dbd"))
	if err != nil {
		t.Fatalf("definition parse: %v", err)
	}
	rows, err := table.OpenRecords(ctx, bytes.NewReader(raw), int64(len(raw)), budget)
	if err != nil {
		t.Fatal(err)
	}
	view, err := rows.Bind(ctx, doc, build)
	if err != nil {
		t.Fatalf("definition bind: %v", err)
	}
	if view.Definition().SHA256 != "f63a29a608648c76f98cabb7b2253781d2f0c6a416bc36404adf6f17c6a99ad4" {
		t.Fatal("pinned ChrClasses definition bytes changed")
	}
	row, err := view.Row(ctx, 1, 1<<20)
	if err != nil {
		t.Fatal(err)
	}
	if row["ID"] != uint64(1) {
		t.Fatalf("wrong row identity: %v", row["ID"])
	}
	if row["Filename"] != "WARRIOR" || row["Name_lang"] != "Warrior" || len(row) != 43 {
		t.Fatalf("unexpected known class row: %v", row)
	}
	t.Logf("typed table verified: table=%s fdid=%d hash=%08X layout=%08X manifest=%s definition=%s row=%v", name, fileID, layout.TableHash, layout.LayoutHash, manifest.SHA256(), view.Definition().SHA256, row)
	tableSource, err := records.TableSource(view, 4<<20)
	if err != nil {
		t.Fatal(err)
	}
	resolve := func(_ context.Context, use relational.TableUse) (relational.Source, error) {
		if use.Catalog != "static" || use.Name != name {
			return relational.Source{}, relational.ErrBinding
		}
		return tableSource, nil
	}
	for _, sql := range []string{
		"SELECT ID, Filename FROM ChrClasses WHERE ID=1",
		"SELECT COUNT(*) AS n, MIN(ID) AS firstID, MAX(ID) AS lastID FROM ChrClasses",
		"SELECT COUNT(*) FROM ChrClasses a INNER JOIN ChrClasses b ON a.ID=b.ID",
	} {
		program, err := relational.Compile(ctx, sql)
		if err != nil {
			t.Fatal(err)
		}
		result, err := program.Execute(ctx, resolve, nil, relational.Limits{Work: 1000000, MemoryBytes: 16 << 20})
		if err != nil {
			t.Fatal(sql, err)
		}
		if len(result.Rows) != 1 {
			t.Fatal(result)
		}
		if len(result.Columns) == 2 {
			if result.Rows[0][0] != uint64(1) || result.Rows[0][1] != "WARRIOR" {
				t.Fatal(result)
			}
		} else {
			if result.Rows[0][0] != int64(15) {
				t.Fatal(result)
			}
			if len(result.Columns) == 3 && (result.Rows[0][1] != uint64(1) || result.Rows[0][2] != uint64(15)) {
				t.Fatal(result)
			}
		}
		t.Logf("real DB2 SQL verified: %s => %+v", sql, result)
	}
}
