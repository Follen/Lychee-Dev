package records

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/records/schema"
	"github.com/follenfang/lycheedev/internal/vault"
)

const definitionsTestCommit = "0123456789abcdef0123456789abcdef01234567"

var (
	definitionsManifest = []byte(`[{"tableName":"TestTable","tableHash":"12345678","db2FileDataID":123}]`)
	definitionsDBD      = []byte("COLUMNS\nint ID\n\nBUILD 12.1.0.69875\nLAYOUT 12345678\n$id$ID<32>\n")
)

type definitionResponse struct {
	status   int
	body     []byte
	location string
}

type definitionsTransport struct {
	mu        sync.Mutex
	calls     []string
	responses map[string]definitionResponse
}

func (f *definitionsTransport) RoundTrip(request *http.Request) (*http.Response, error) {
	if err := request.Context().Err(); err != nil {
		return nil, err
	}
	f.mu.Lock()
	f.calls = append(f.calls, request.URL.String())
	response, ok := f.responses[request.URL.String()]
	f.mu.Unlock()
	if !ok {
		return nil, fmt.Errorf("unexpected test URL: %s", request.URL.String())
	}
	header := make(http.Header)
	if response.location != "" {
		header.Set("Location", response.location)
	}
	return &http.Response{
		StatusCode: response.status,
		Status:     fmt.Sprintf("%d %s", response.status, http.StatusText(response.status)),
		Header:     header,
		Body:       io.NopCloser(bytes.NewReader(response.body)),
		Request:    request,
	}, nil
}

func (f *definitionsTransport) callURLs() []string {
	f.mu.Lock()
	defer f.mu.Unlock()
	return append([]string(nil), f.calls...)
}

type roundTripperFunc func(*http.Request) (*http.Response, error)

func (f roundTripperFunc) RoundTrip(request *http.Request) (*http.Response, error) {
	return f(request)
}

type definitionsFixture struct {
	store      *vault.Store
	metadata   *vault.Metadata
	defs       *Definitions
	transport  *definitionsTransport
	manifest   string
	definition string
}

func newDefinitionsFixture(t *testing.T) definitionsFixture {
	t.Helper()
	ctx := context.Background()
	store, err := vault.Initialize(ctx, filepath.Join(t.TempDir(), "vault"))
	if err != nil {
		t.Fatalf("vault.Initialize: %v", err)
	}
	metadata, err := store.OpenMetadata(ctx)
	if err != nil {
		t.Fatalf("OpenMetadata: %v", err)
	}
	t.Cleanup(func() { _ = metadata.Close() })

	manifestURL := "https://raw.githubusercontent.com/wowdev/WoWDBDefs/" + definitionsTestCommit + "/manifest.json"
	definitionURL := "https://raw.githubusercontent.com/wowdev/WoWDBDefs/" + definitionsTestCommit + "/definitions/TestTable.dbd"
	transport := &definitionsTransport{responses: map[string]definitionResponse{
		manifestURL:   {status: http.StatusOK, body: definitionsManifest},
		definitionURL: {status: http.StatusOK, body: definitionsDBD},
	}}
	defs := OpenDefinitions(store, metadata)
	// Keep every test request inside this deterministic RoundTripper.
	defs.client.Transport = transport
	return definitionsFixture{
		store:      store,
		metadata:   metadata,
		defs:       defs,
		transport:  transport,
		manifest:   manifestURL,
		definition: definitionURL,
	}
}

func TestDefinitionsPrepareOnlineThenOfflineCache(t *testing.T) {
	fixture := newDefinitionsFixture(t)
	ctx := context.Background()

	bundle, err := fixture.defs.Prepare(ctx, definitionsTestCommit, "testtable", false)
	if err != nil {
		t.Fatalf("online Prepare: %v", err)
	}
	wantURLs := []string{fixture.manifest, fixture.definition}
	if got := fixture.transport.callURLs(); !equalStrings(got, wantURLs) {
		t.Fatalf("online URLs = %#v, want %#v", got, wantURLs)
	}
	if bundle.Commit != definitionsTestCommit {
		t.Fatalf("bundle commit = %q, want %q", bundle.Commit, definitionsTestCommit)
	}
	wantIdentity := schema.TableIdentity{Name: "TestTable", Hash: 0x12345678, DB2FileDataID: 123}
	if bundle.Identity != wantIdentity {
		t.Fatalf("bundle identity = %+v, want %+v", bundle.Identity, wantIdentity)
	}
	if bundle.Manifest.SHA256 == "" || bundle.Definition.SHA256 == "" {
		t.Fatalf("incomplete DefinitionBundle: %+v", bundle)
	}
	raw, err := fixture.store.ReadBlob(ctx, bundle.Definition, 4<<20)
	if err != nil {
		t.Fatal(err)
	}
	document, err := schema.Parse(ctx, raw)
	if err != nil {
		t.Fatal(err)
	}
	selected, err := document.Select(ctx, "12.1.0.69875", "12345678")
	if err != nil {
		t.Fatalf("select prepared definition: %v", err)
	}
	if len(selected.Fields) != 1 || selected.Fields[0].Name != "ID" || !selected.Fields[0].Identity || selected.Fields[0].Bits != 32 {
		t.Fatalf("selected definition = %+v, want one 32-bit ID field", selected)
	}

	callsBeforeOffline := len(fixture.transport.callURLs())
	offline, err := fixture.defs.Prepare(ctx, definitionsTestCommit, "TestTable", true)
	if err != nil {
		t.Fatalf("offline Prepare from cache: %v", err)
	}
	if got := len(fixture.transport.callURLs()); got != callsBeforeOffline {
		t.Fatalf("offline transport calls = %d, want %d", got, callsBeforeOffline)
	}
	if offline.Manifest != bundle.Manifest || offline.Definition != bundle.Definition || offline.Identity != bundle.Identity {
		t.Fatalf("offline bundle differs from cached online bundle: online=%+v offline=%+v", bundle, offline)
	}
}

func TestDefinitionsRejectIdentityBeforeRequest(t *testing.T) {
	fixture := newDefinitionsFixture(t)
	ctx := context.Background()
	for _, test := range []struct {
		name   string
		commit string
		table  string
	}{
		{name: "empty commit", commit: "", table: "TestTable"},
		{name: "short commit", commit: "0123", table: "TestTable"},
		{name: "uppercase commit", commit: strings.ToUpper(definitionsTestCommit), table: "TestTable"},
		{name: "non-hex commit", commit: strings.Repeat("g", 40), table: "TestTable"},
		{name: "empty name", commit: definitionsTestCommit, table: ""},
		{name: "path name", commit: definitionsTestCommit, table: "Test/Table"},
		{name: "leading digit", commit: definitionsTestCommit, table: "1TestTable"},
		{name: "oversize name", commit: definitionsTestCommit, table: strings.Repeat("a", 129)},
	} {
		t.Run(test.name, func(t *testing.T) {
			_, err := fixture.defs.Prepare(ctx, test.commit, test.table, false)
			if !errors.Is(err, ErrDefinitionIdentity) {
				t.Fatalf("Prepare error = %v, want ErrDefinitionIdentity", err)
			}
		})
	}
	if got := fixture.transport.callURLs(); len(got) != 0 {
		t.Fatalf("invalid identities made requests: %#v", got)
	}
}

func TestDefinitionsHTTPErrorAndRedirectAreNotFollowed(t *testing.T) {
	for _, test := range []struct {
		name     string
		response definitionResponse
		wantCode int
	}{
		{name: "http error", response: definitionResponse{status: http.StatusInternalServerError, body: []byte("server error")}, wantCode: http.StatusInternalServerError},
		{name: "redirect", response: definitionResponse{status: http.StatusFound, location: "https://example.invalid/redirected"}, wantCode: http.StatusFound},
	} {
		t.Run(test.name, func(t *testing.T) {
			fixture := newDefinitionsFixture(t)
			fixture.transport.mu.Lock()
			fixture.transport.responses[fixture.manifest] = test.response
			fixture.transport.mu.Unlock()

			_, err := fixture.defs.Prepare(context.Background(), definitionsTestCommit, "TestTable", false)
			want := fmt.Sprintf("records.definition_http: %d", test.wantCode)
			if err == nil || !strings.Contains(err.Error(), want) {
				t.Fatalf("Prepare error = %v, want %q", err, want)
			}
			if got := fixture.transport.callURLs(); !equalStrings(got, []string{fixture.manifest}) {
				t.Fatalf("requests = %#v, want only the original manifest URL", got)
			}
		})
	}
}

func TestDefinitionsInvalidOrOversizeManifestIsNotCached(t *testing.T) {
	for _, test := range []struct {
		name string
		body []byte
		want error
	}{
		{name: "invalid", body: []byte(`[{"tableName":"TestTable","tableHash":"not-a-hash"}]`), want: schema.ErrFormat},
		{name: "oversize", body: bytes.Repeat([]byte{' '}, (4<<20)+1), want: ErrMetadataLimit},
	} {
		t.Run(test.name, func(t *testing.T) {
			fixture := newDefinitionsFixture(t)
			fixture.transport.mu.Lock()
			fixture.transport.responses[fixture.manifest] = definitionResponse{status: http.StatusOK, body: test.body}
			fixture.transport.mu.Unlock()

			_, err := fixture.defs.Prepare(context.Background(), definitionsTestCommit, "TestTable", false)
			if !errors.Is(err, test.want) {
				t.Fatalf("invalid manifest error = %v, want %v", err, test.want)
			}
			if docs, listErr := fixture.metadata.ListDocuments(context.Background(), "definition/", "", 10); listErr != nil || len(docs) != 0 {
				t.Fatalf("failed manifest was cached: documents=%d error=%v", len(docs), listErr)
			}

			fixture.transport.mu.Lock()
			fixture.transport.responses[fixture.manifest] = definitionResponse{status: http.StatusOK, body: definitionsManifest}
			fixture.transport.mu.Unlock()
			bundle, err := fixture.defs.Prepare(context.Background(), definitionsTestCommit, "TestTable", false)
			if err != nil || bundle.Definition.SHA256 == "" {
				t.Fatalf("Prepare after replacing failed response = %+v, %v", bundle, err)
			}
			if got := len(fixture.transport.callURLs()); got != 3 {
				t.Fatalf("requests after failed then successful manifest = %d, want 3", got)
			}
		})
	}
}

func TestDefinitionsCancellation(t *testing.T) {
	fixture := newDefinitionsFixture(t)
	started := make(chan struct{})
	fixture.defs.client.Transport = roundTripperFunc(func(request *http.Request) (*http.Response, error) {
		close(started)
		<-request.Context().Done()
		return nil, request.Context().Err()
	})
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	result := make(chan error, 1)
	go func() {
		_, err := fixture.defs.Prepare(ctx, definitionsTestCommit, "TestTable", false)
		result <- err
	}()
	select {
	case <-started:
		cancel()
	case <-time.After(2 * time.Second):
		t.Fatal("transport was not called")
	}
	select {
	case err := <-result:
		if !errors.Is(err, context.Canceled) {
			t.Fatalf("canceled Prepare error = %v, want context.Canceled", err)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("canceled Prepare did not return")
	}
}

func TestDefinitionsCorruptedCacheDoesNotGetReplaced(t *testing.T) {
	fixture := newDefinitionsFixture(t)
	ctx := context.Background()
	bundle, err := fixture.defs.Prepare(ctx, definitionsTestCommit, "TestTable", false)
	if err != nil {
		t.Fatalf("online Prepare: %v", err)
	}
	path := filepath.Join(fixture.store.Root(), "blobs", bundle.Manifest.SHA256[:2], bundle.Manifest.SHA256[2:])
	if err := os.WriteFile(path, []byte("corrupt cache"), 0600); err != nil {
		t.Fatalf("corrupt manifest cache: %v", err)
	}

	_, err = fixture.defs.Prepare(ctx, definitionsTestCommit, "TestTable", false)
	if !errors.Is(err, vault.ErrBlobIntegrity) {
		t.Fatalf("corrupted cache error = %v, want vault.ErrBlobIntegrity", err)
	}
	if got := fixture.transport.callURLs(); !equalStrings(got, []string{fixture.manifest, fixture.definition}) {
		t.Fatalf("corrupted cache triggered replacement requests: %#v", got)
	}
}

func equalStrings(got, want []string) bool {
	if len(got) != len(want) {
		return false
	}
	for i := range got {
		if got[i] != want[i] {
			return false
		}
	}
	return true
}
