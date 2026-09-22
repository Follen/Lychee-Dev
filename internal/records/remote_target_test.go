package records

import (
	"context"
	"crypto/md5"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"strings"
	"sync"
	"testing"

	"github.com/follenfang/lycheedev/internal/evidence"
	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
)

type remoteTargetFixture struct {
	definitionsFixture
	request                                   RemoteTargetRequest
	versionsURL, cdnsURL, buildURL, configURL string
}

func newRemoteTargetFixture(t *testing.T) remoteTargetFixture {
	f := remoteTargetFixture{definitionsFixture: newDefinitionsFixture(t), request: RemoteTargetRequest{Product: "retail", Region: "cn", Locale: "zhCN", Definitions: definitionsTestCommit}}
	build := []byte(fmt.Sprintf("root = %s\nencoding = %s %s\nencoding-size = 123 456\nbuild-uid = wow\n", strings.Repeat("1", 32), strings.Repeat("2", 32), strings.Repeat("3", 32)))
	cdn := []byte("archives = " + strings.Repeat("4", 32) + "\n")
	buildKey, cdnKey := fmt.Sprintf("%x", md5.Sum(build)), fmt.Sprintf("%x", md5.Sum(cdn))
	f.versionsURL = remoteVersionBase("cn") + "wow/versions"
	f.cdnsURL = remoteVersionBase("cn") + "wow/cdns"
	f.buildURL = "https://cdn.example.test/tpr/wow/config/" + buildKey[:2] + "/" + buildKey[2:4] + "/" + buildKey
	f.configURL = "https://cdn.example.test/tpr/wow/config/" + cdnKey[:2] + "/" + cdnKey[2:4] + "/" + cdnKey
	f.transport.responses = map[string]definitionResponse{
		f.versionsURL: {status: 200, body: []byte(fmt.Sprintf("Region!STRING:0|BuildConfig!HEX:16|CDNConfig!HEX:16|BuildId!DEC:4|VersionsName!STRING:0\n## seqn = 1\ncn|%s|%s|69875|12.1.0.69875\n", buildKey, cdnKey))},
		f.cdnsURL:     {status: 200, body: []byte("Name!STRING:0|Path!STRING:0|Hosts!STRING:0\ncn|tpr/wow|cdn.example.test\n")},
		f.buildURL:    {status: 200, body: build}, f.configURL: {status: 200, body: cdn},
	}
	return f
}

func (f remoteTargetFixture) resolve(ctx context.Context, request RemoteTargetRequest) (RemoteTarget, error) {
	return resolveRemoteTarget(ctx, f.store.Root(), request, f.defs.client)
}

func TestRemoteTargetFixedOnlineOfflineEvidence(t *testing.T) {
	f := newRemoteTargetFixture(t)
	ctx := context.Background()
	first, err := f.resolve(ctx, f.request)
	if err != nil {
		t.Fatal(err)
	}
	if first.Pin.Data.Product != "retail" || first.Pin.Data.FullBuild != "12.1.0.69875" || first.Pin.Data.DefinitionCommit != definitionsTestCommit || first.ObservedAt.IsZero() || first.Offline || !first.Capture.Complete {
		t.Fatal(first)
	}
	if got := f.transport.callURLs(); !equalStrings(got, []string{f.versionsURL, f.cdnsURL, f.buildURL, f.configURL}) {
		t.Fatal(got)
	}
	_, raw, err := evidence.OpenArchive(f.store, f.metadata).FetchCapture(ctx, first.Capture.ID, 65536)
	if err != nil {
		t.Fatal(err)
	}
	var proof struct {
		Pin            selection.PinnedSet
		Observation    remoteObservation
		Configurations []vault.BlobRef
	}
	if err := json.Unmarshal(raw, &proof); err != nil || proof.Pin.ID != first.Pin.ID || len(proof.Configurations) != 2 || proof.Observation.Region != "cn" {
		t.Fatal(string(raw), err)
	}
	for _, ref := range append(proof.Configurations, proof.Observation.Versions, proof.Observation.Distribution) {
		if err := f.store.VerifyBlob(ctx, ref); err != nil {
			t.Fatal(err)
		}
	}
	f.transport.responses = nil
	offline := f.request
	offline.Offline = true
	repeated, err := f.resolve(ctx, offline)
	if err != nil || repeated.Pin.ID != first.Pin.ID || repeated.ObservedAt != first.ObservedAt || !repeated.Offline || len(f.transport.callURLs()) != 4 {
		t.Fatal(repeated, err)
	}
	// An online failure cannot silently reuse the remembered observation.
	if got, err := f.resolve(ctx, f.request); !errors.Is(err, ErrRemoteHTTP) || got.Pin.ID != "" {
		t.Fatal(got, err)
	}
	wrong := offline
	wrong.FullBuild = "12.1.0.1"
	if got, err := f.resolve(ctx, wrong); !errors.Is(err, ErrBuildUnavailable) || got.Pin.ID != "" {
		t.Fatal(got, err)
	}
}

func TestRemoteTargetRejectsBadDownloadsWithoutObservation(t *testing.T) {
	for _, scenario := range []string{"redirect", "integrity", "oversize", "truncated", "bad-cdns", "unknown-build"} {
		t.Run(scenario, func(t *testing.T) {
			f := newRemoteTargetFixture(t)
			want := ErrMetadataFormat
			switch scenario {
			case "redirect":
				f.transport.responses[f.versionsURL] = definitionResponse{status: 302, location: "https://other.example.test/versions"}
				want = ErrRemoteHTTP
			case "integrity":
				f.transport.responses[f.buildURL] = definitionResponse{status: 200, body: []byte("wrong")}
				want = ErrConfigurationIntegrity
			case "oversize":
				f.transport.responses[f.versionsURL] = definitionResponse{status: 200, body: []byte(strings.Repeat("x", (1<<20)+1))}
				want = ErrMetadataLimit
			case "truncated":
				r := f.transport.responses[f.versionsURL]
				r.body = append(r.body, []byte("broken|tail\n")...)
				f.transport.responses[f.versionsURL] = r
			case "bad-cdns":
				f.transport.responses[f.cdnsURL] = definitionResponse{status: 200, body: []byte("Name|Path|Hosts\ncn|../private|localhost\n")}
			case "unknown-build":
				f.request.FullBuild = "12.1.0.1"
				want = ErrBuildUnavailable
			}
			result, err := f.resolve(context.Background(), f.request)
			if !errors.Is(err, want) || result.Pin.ID != "" || result.Capture.ID != "" {
				t.Fatal(result, err, want)
			}
			if _, err := f.metadata.ReadDocument(context.Background(), "remote-catalog/wow/cn"); !errors.Is(err, vault.ErrMissingRecord) {
				t.Fatal("failed request remembered", err)
			}
			if scenario == "redirect" && len(f.transport.callURLs()) != 1 {
				t.Fatal(f.transport.callURLs())
			}
		})
	}
}

func TestRemoteTargetParentAndConcurrentReaders(t *testing.T) {
	f := newRemoteTargetFixture(t)
	ctx := context.Background()
	parent, err := selection.OpenPinner(f.metadata).PinSelection(ctx, selection.SelectionSpec{Source: &selection.SourcePin{Product: "retail", Repository: "fixture", ExactCommit: strings.Repeat("a", 40), ParserRevision: "fixture-v1"}})
	if err != nil {
		t.Fatal(err)
	}
	f.request.Parent = parent.ID
	var wg sync.WaitGroup
	results := make(chan RemoteTarget, 8)
	failures := make(chan error, 8)
	for range 8 {
		wg.Go(func() {
			result, err := f.resolve(ctx, f.request)
			if err != nil {
				failures <- err
			} else {
				results <- result
			}
		})
	}
	wg.Wait()
	close(results)
	close(failures)
	for err := range failures {
		t.Fatal(err)
	}
	id := ""
	for result := range results {
		if id != "" && id != result.Pin.ID || result.Pin.Source == nil || *result.Pin.Source != *parent.Source {
			t.Fatal(result)
		}
		id = result.Pin.ID
	}
	if id == "" {
		t.Fatal("no result")
	}
	counts := map[string]int{}
	for _, url := range f.transport.callURLs() {
		counts[url]++
	}
	if counts[f.buildURL] != 1 || counts[f.configURL] != 1 {
		t.Fatal("duplicate immutable downloads", counts)
	}
	// Existing data retains its definition pin; default does not refresh master.
	r := f.request
	r.Parent = id
	r.Offline = true
	r.Definitions = ""
	derived, err := f.resolve(ctx, r)
	if err != nil || derived.Pin.Data.DefinitionCommit != definitionsTestCommit {
		t.Fatal(derived, err)
	}
	r.Locale = "enUS"
	if _, err := f.resolve(ctx, r); err == nil || err.Error() != "selection.fixed_data_conflict" {
		t.Fatal(err)
	}
}

func TestRemoteTargetCorruptObservationAndCancelled(t *testing.T) {
	f := newRemoteTargetFixture(t)
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err := f.resolve(ctx, f.request); !errors.Is(err, context.Canceled) || len(f.transport.callURLs()) != 0 {
		t.Fatal(err)
	}
	if err := f.metadata.CommitDocuments(context.Background(), vault.Mutation{Key: "remote-catalog/wow/cn", Value: []byte(`{"productCode":"other"}`)}); err != nil {
		t.Fatal(err)
	}
	f.request.Offline = true
	if _, err := f.resolve(context.Background(), f.request); !errors.Is(err, ErrRemoteIdentity) || len(f.transport.callURLs()) != 0 {
		t.Fatal(err)
	}
}

func TestRemoteTargetCorruptCachedConfigurationIsNotReplaced(t *testing.T) {
	f := newRemoteTargetFixture(t)
	ctx := context.Background()
	first, err := f.resolve(ctx, f.request)
	if err != nil {
		t.Fatal(err)
	}
	key := "remote-config/" + first.Pin.Data.BuildConfig
	doc, err := f.metadata.ReadDocument(ctx, key)
	if err != nil {
		t.Fatal(err)
	}
	var ref vault.BlobRef
	if err := json.Unmarshal(doc.Value, &ref); err != nil {
		t.Fatal(err)
	}
	ref.Bytes++
	raw, _ := json.Marshal(ref)
	if err := f.metadata.CommitDocuments(ctx, vault.Mutation{Key: key, ExpectedGeneration: doc.Generation, Value: raw}); err != nil {
		t.Fatal(err)
	}
	request := f.request
	request.Offline = true
	for _, offline := range []bool{true, false} {
		request.Offline = offline
		got, err := f.resolve(ctx, request)
		if !errors.Is(err, vault.ErrBlobIntegrity) || got.Pin.ID != "" {
			t.Fatal(got, err)
		}
	}
	count := 0
	for _, url := range f.transport.callURLs() {
		if url == f.buildURL {
			count++
		}
	}
	if count != 1 {
		t.Fatal("corrupt config silently downloaded again", count)
	}
}

func TestRemoteConfigurationMirrorFallback(t *testing.T) {
	f := newRemoteTargetFixture(t)
	f.transport.responses[f.cdnsURL] = definitionResponse{status: 200, body: []byte("Name!STRING:0|Path!STRING:0|Hosts!STRING:0\ncn|tpr/wow|unavailable.example.test cdn.example.test\n")}
	for _, url := range []string{f.buildURL, f.configURL} {
		f.transport.responses[strings.Replace(url, "cdn.example.test", "unavailable.example.test", 1)] = definitionResponse{status: 503}
	}
	if _, err := f.resolve(context.Background(), f.request); err != nil {
		t.Fatal(err)
	}
	if got := len(f.transport.callURLs()); got != 6 {
		t.Fatal(got)
	}
}

type brokenRemoteBody struct{ closed bool }

func (b *brokenRemoteBody) Read([]byte) (int, error) { return 0, io.ErrUnexpectedEOF }
func (b *brokenRemoteBody) Close() error             { b.closed = true; return nil }

func TestRemoteResponseReadFailureClosesBody(t *testing.T) {
	body := &brokenRemoteBody{}
	client := &http.Client{Transport: roundTripperFunc(func(r *http.Request) (*http.Response, error) {
		return &http.Response{StatusCode: 200, Body: body, Header: make(http.Header), Request: r}, nil
	})}
	if _, err := fetchRemoteMetadata(context.Background(), client, "https://example.test/metadata", 1<<20); !errors.Is(err, ErrRemoteHTTP) || !errors.Is(err, io.ErrUnexpectedEOF) || !body.closed {
		t.Fatal(err, body.closed)
	}
}
