package records_test

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/records"
	"github.com/follenfang/lycheedev/internal/vault"
)

type fakeListfileFetcher struct {
	calls  int
	bodies map[string][]byte
	err    error
}

func (f *fakeListfileFetcher) FetchListfile(ctx context.Context, url string, maxBytes int64) ([]byte, error) {
	f.calls++
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if f.err != nil {
		return nil, f.err
	}
	if body, ok := f.bodies[url]; ok {
		if int64(len(body)) > maxBytes {
			return nil, errors.New("too large")
		}
		return append([]byte(nil), body...), nil
	}
	return nil, errors.New("no such URL: " + url)
}

const communityURL = "https://example.invalid/community-listfile.csv"

func communityRequest(fetch records.ListfileFetcher) records.ListfileRequest {
	return records.ListfileRequest{
		Kind:  records.ListfileCommunityCSV,
		URLs:  []string{communityURL},
		Fetch: fetch,
	}
}

func TestPrepareListfileCachesAndReusesWithoutNetwork(t *testing.T) {
	ctx := context.Background()
	workspace := workspaceRoot(t)
	fetch := &fakeListfileFetcher{bodies: map[string][]byte{communityURL: []byte("11;interface/icons/test.blp\n12;other.blp\n")}}
	first, err := records.PrepareListfile(ctx, workspace, communityRequest(fetch))
	if err != nil {
		t.Fatal(err)
	}
	if fetch.calls != 1 || first.Provenance.Cache != "fetched" || first.Provenance.EntryCount != 2 {
		t.Fatalf("first prepare = calls %d %+v", fetch.calls, first.Provenance)
	}
	if len(first.Provenance.Files) != 1 || first.Provenance.Files[0].URL != communityURL || first.Provenance.Files[0].Blob.SHA256 == "" || first.Provenance.FetchedAt.IsZero() {
		t.Fatalf("provenance = %+v", first.Provenance)
	}
	second, err := records.PrepareListfile(ctx, workspace, communityRequest(fetch))
	if err != nil {
		t.Fatal(err)
	}
	if fetch.calls != 1 || second.Provenance.Cache != "verified-cache" || second.Index.Len() != 2 {
		t.Fatalf("cache reuse = calls %d %+v", fetch.calls, second.Provenance)
	}
	if second.Index.Names(11)[0].FileName != "interface/icons/test.blp" {
		t.Fatalf("cached index = %+v", second.Index.Names(11))
	}
}

func TestPrepareListfileDoesNotReuseAnotherOrigin(t *testing.T) {
	ctx := context.Background()
	workspace := workspaceRoot(t)
	other := "https://example.invalid/other.csv"
	fetch := &fakeListfileFetcher{bodies: map[string][]byte{communityURL: []byte("11;a.blp\n"), other: []byte("22;b.blp\n")}}
	if _, err := records.PrepareListfile(ctx, workspace, communityRequest(fetch)); err != nil {
		t.Fatal(err)
	}
	request := communityRequest(fetch)
	request.URLs, request.Offline = []string{other}, true
	if _, err := records.PrepareListfile(ctx, workspace, request); !errors.Is(err, records.ErrListfileUnavailable) {
		t.Fatalf("offline substituted cached origin: %v", err)
	}
	request.Offline = false
	reading, err := records.PrepareListfile(ctx, workspace, request)
	if err != nil {
		t.Fatal(err)
	}
	if fetch.calls != 2 || len(reading.Index.Names(22)) != 1 || reading.Provenance.Files[0].URL != other {
		t.Fatalf("selected origin was ignored: calls=%d provenance=%+v", fetch.calls, reading.Provenance)
	}
}

func TestPrepareListfileOfflineSemantics(t *testing.T) {
	ctx := context.Background()
	workspace := workspaceRoot(t)
	fetch := &fakeListfileFetcher{bodies: map[string][]byte{communityURL: []byte("11;a.blp\n")}}
	request := communityRequest(fetch)
	request.Offline = true
	// Offline with no cache is a precise missing-resource failure and must not
	// touch the network at all.
	if _, err := records.PrepareListfile(ctx, workspace, request); !errors.Is(err, records.ErrListfileUnavailable) {
		t.Fatalf("offline without cache: %v", err)
	}
	if fetch.calls != 0 {
		t.Fatalf("offline fetched %d times", fetch.calls)
	}
	online := communityRequest(fetch)
	if _, err := records.PrepareListfile(ctx, workspace, online); err != nil {
		t.Fatal(err)
	}
	offline, err := records.PrepareListfile(ctx, workspace, request)
	if err != nil {
		t.Fatal(err)
	}
	if fetch.calls != 1 || offline.Provenance.Cache != "offline-cache" || offline.Index.Len() != 1 {
		t.Fatalf("offline cache = calls %d %+v", fetch.calls, offline.Provenance)
	}
	// Offline never refreshes or switches sources.
	request.Refresh = true
	if _, err := records.PrepareListfile(ctx, workspace, request); !errors.Is(err, records.ErrListfileQuery) {
		t.Fatalf("offline refresh: %v", err)
	}
}

func TestPrepareListfileCorruptCache(t *testing.T) {
	ctx := context.Background()
	workspace := workspaceRoot(t)
	fetch := &fakeListfileFetcher{bodies: map[string][]byte{communityURL: []byte("11;a.blp\n")}}
	if _, err := records.PrepareListfile(ctx, workspace, communityRequest(fetch)); err != nil {
		t.Fatal(err)
	}
	blobPath := cachedBlobPath(t, workspace)
	if err := os.Remove(blobPath); err != nil {
		t.Fatal(err)
	}
	// Offline reports a precise unusable-cache failure instead of guessing.
	offline := communityRequest(fetch)
	offline.Offline = true
	if _, err := records.PrepareListfile(ctx, workspace, offline); !errors.Is(err, records.ErrListfileUnavailable) {
		t.Fatalf("offline corrupt cache: %v", err)
	}
	if fetch.calls != 1 {
		t.Fatalf("offline corrupt cache fetched %d times", fetch.calls)
	}
	// Online re-fetches the same source and re-verifies the cache; it never
	// substitutes another source.
	refetched, err := records.PrepareListfile(ctx, workspace, communityRequest(fetch))
	if err != nil {
		t.Fatal(err)
	}
	if fetch.calls != 2 || refetched.Provenance.Cache != "fetched" || refetched.Index.Len() != 1 {
		t.Fatalf("online refetch = calls %d %+v", fetch.calls, refetched.Provenance)
	}
	reused, err := records.PrepareListfile(ctx, workspace, communityRequest(fetch))
	if err != nil || reused.Provenance.Cache != "verified-cache" || fetch.calls != 2 {
		t.Fatalf("cache re-verified = calls %d %+v %v", fetch.calls, reused.Provenance, err)
	}
}

func TestPrepareListfileTamperedBlobIsNeverTrusted(t *testing.T) {
	ctx := context.Background()
	workspace := workspaceRoot(t)
	fetch := &fakeListfileFetcher{bodies: map[string][]byte{communityURL: []byte("11;a.blp\n")}}
	if _, err := records.PrepareListfile(ctx, workspace, communityRequest(fetch)); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(cachedBlobPath(t, workspace), []byte("tampered"), 0600); err != nil {
		t.Fatal(err)
	}
	offline := communityRequest(fetch)
	offline.Offline = true
	if _, err := records.PrepareListfile(ctx, workspace, offline); !errors.Is(err, records.ErrListfileUnavailable) {
		t.Fatalf("offline tampered cache: %v", err)
	}
	// A tampered content-addressed object is reported as an integrity failure,
	// never silently repaired or replaced by unchecked bytes.
	if _, err := records.PrepareListfile(ctx, workspace, communityRequest(fetch)); !errors.Is(err, vault.ErrBlobIntegrity) {
		t.Fatalf("online tampered cache: %v", err)
	}
}

// cachedBlobPath locates the workspace blob of the cached community listfile.
func cachedBlobPath(t *testing.T, workspace string) string {
	t.Helper()
	ctx := context.Background()
	store, err := vault.OpenStore(workspace)
	if err != nil {
		t.Fatal(err)
	}
	metadata, err := store.OpenMetadata(ctx)
	if err != nil {
		t.Fatal(err)
	}
	defer metadata.Close()
	document, err := metadata.ReadDocument(ctx, "listfile/community-csv")
	if err != nil {
		t.Fatal(err)
	}
	text := string(document.Value)
	start := strings.Index(text, `"blob":{"sha256":"`) + len(`"blob":{"sha256":"`)
	if start < len(`"blob":{"sha256":"`) {
		t.Fatalf("cache document without blob digest: %s", text)
	}
	digest := text[start : start+64]
	return filepath.Join(workspace, "blobs", digest[:2], digest[2:])
}

func TestPrepareListfileUnavailableSource(t *testing.T) {
	ctx := context.Background()
	workspace := workspaceRoot(t)
	fetch := &fakeListfileFetcher{err: errors.New("network down")}
	// Source unavailable is an error, never an empty listfile result.
	if _, err := records.PrepareListfile(ctx, workspace, communityRequest(fetch)); !errors.Is(err, records.ErrListfileUnavailable) {
		t.Fatalf("unreachable source: %v", err)
	}
	if _, err := records.PrepareListfile(ctx, workspace, records.ListfileRequest{Kind: records.ListfileCommunityCSV}); !errors.Is(err, records.ErrListfileUnavailable) {
		t.Fatalf("missing fetcher: %v", err)
	}
	if _, err := records.PrepareListfile(ctx, workspace, records.ListfileRequest{}); !errors.Is(err, records.ErrListfileQuery) {
		t.Fatalf("missing kind: %v", err)
	}
}

func TestPrepareListfileBinarySource(t *testing.T) {
	ctx := context.Background()
	workspace := workspaceRoot(t)
	components := buildBinaryListfile(t, []records.ListfileMatch{
		{FileDataID: 7, SourceName: "creature/bear.blp", Component: "listfile-pf-textures.dat"},
		{FileDataID: 8, SourceName: "interface/framexml/ui.m2", Component: "listfile-pf-models.dat"},
	})
	bodies := map[string][]byte{}
	for name, body := range components {
		bodies["https://example.invalid/bin/"+name] = body
	}
	fetch := &fakeListfileFetcher{bodies: bodies}
	request := records.ListfileRequest{
		Kind:  records.ListfileWowExportBinary,
		URLs:  []string{"https://example.invalid/bin/%s"},
		Fetch: fetch,
	}
	reading, err := records.PrepareListfile(ctx, workspace, request)
	if err != nil {
		t.Fatal(err)
	}
	if reading.Index.Len() != 2 || fetch.calls != len(records.BinaryListfileComponents()) {
		t.Fatalf("binary source = %d entries, %d fetches", reading.Index.Len(), fetch.calls)
	}
	if reading.Index.Provenance().Kind != records.ListfileWowExportBinary || len(reading.Provenance.Files) != len(records.BinaryListfileComponents()) {
		t.Fatalf("binary provenance = %+v", reading.Provenance)
	}
	// The cached binary source re-parses without any network access.
	request.Fetch = &fakeListfileFetcher{err: errors.New("must not be called")}
	request.Offline = true
	cached, err := records.PrepareListfile(ctx, workspace, request)
	if err != nil {
		t.Fatal(err)
	}
	if cached.Index.Names(8)[0].Component != "listfile-pf-models.dat" {
		t.Fatalf("cached binary entry = %+v", cached.Index.Names(8))
	}
}
