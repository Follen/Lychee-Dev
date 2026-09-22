package records

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/vault"
)

const (
	definitionRevisionTestReference = "refs/heads/master"
	definitionRevisionTestCommitA   = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
	definitionRevisionTestCommitB   = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
)

func TestDefinitionRevisionExactCommitIsDirect(t *testing.T) {
	fixture := newDefinitionsFixture(t)

	got, err := fixture.defs.ResolveRevision(context.Background(), definitionsTestCommit, false)
	if err != nil {
		t.Fatalf("ResolveRevision exact: %v", err)
	}
	if got != definitionsTestCommit {
		t.Fatalf("exact commit = %q, want %q", got, definitionsTestCommit)
	}
	if calls := fixture.transport.callURLs(); len(calls) != 0 {
		t.Fatalf("exact commit made network calls: %#v", calls)
	}
}

func TestDefinitionRevisionOnlineThenOffline(t *testing.T) {
	fixture := newDefinitionsFixture(t)
	endpoint := definitionRevisionURL + url.PathEscape(definitionRevisionTestReference)
	fixture.transport.mu.Lock()
	fixture.transport.responses[endpoint] = definitionResponse{status: http.StatusOK, body: []byte(definitionRevisionTestCommitA + "\n")}
	fixture.transport.mu.Unlock()

	online, err := fixture.defs.ResolveRevision(context.Background(), definitionRevisionTestReference, false)
	if err != nil {
		t.Fatalf("online ResolveRevision: %v", err)
	}
	if online != definitionRevisionTestCommitA {
		t.Fatalf("online commit = %q, want %q", online, definitionRevisionTestCommitA)
	}
	calls := len(fixture.transport.callURLs())
	offline, err := fixture.defs.ResolveRevision(context.Background(), definitionRevisionTestReference, true)
	if err != nil {
		t.Fatalf("offline ResolveRevision: %v", err)
	}
	if offline != online {
		t.Fatalf("offline commit = %q, want prior online commit %q", offline, online)
	}
	if got := len(fixture.transport.callURLs()); got != calls {
		t.Fatalf("offline calls = %d, want %d", got, calls)
	}
	if got := fixture.transport.callURLs()[0]; got != endpoint {
		t.Fatalf("request URL = %q, want %q", got, endpoint)
	}
}

func TestDefinitionRevisionOnlineRefreshAndOfflineUsesLatest(t *testing.T) {
	fixture := newDefinitionsFixture(t)
	endpoint := definitionRevisionURL + url.PathEscape(definitionRevisionTestReference)
	fixture.transport.mu.Lock()
	fixture.transport.responses[endpoint] = definitionResponse{status: http.StatusOK, body: []byte(definitionRevisionTestCommitA)}
	fixture.transport.mu.Unlock()
	first, err := fixture.defs.ResolveRevision(context.Background(), definitionRevisionTestReference, false)
	if err != nil {
		t.Fatalf("first ResolveRevision: %v", err)
	}

	fixture.transport.mu.Lock()
	fixture.transport.responses[endpoint] = definitionResponse{status: http.StatusOK, body: []byte(definitionRevisionTestCommitB + "\r\n")}
	fixture.transport.mu.Unlock()
	second, err := fixture.defs.ResolveRevision(context.Background(), definitionRevisionTestReference, false)
	if err != nil {
		t.Fatalf("refresh ResolveRevision: %v", err)
	}
	if first != definitionRevisionTestCommitA || second != definitionRevisionTestCommitB {
		t.Fatalf("refresh commits = %q then %q", first, second)
	}
	offline, err := fixture.defs.ResolveRevision(context.Background(), definitionRevisionTestReference, true)
	if err != nil || offline != definitionRevisionTestCommitB {
		t.Fatalf("offline refreshed commit = %q, %v", offline, err)
	}
}

func TestDefinitionRevisionOfflineUnknown(t *testing.T) {
	fixture := newDefinitionsFixture(t)
	_, err := fixture.defs.ResolveRevision(context.Background(), definitionRevisionTestReference, true)
	if !errors.Is(err, ErrDefinitionUnavailable) {
		t.Fatalf("offline unknown error = %v, want ErrDefinitionUnavailable", err)
	}
	if calls := fixture.transport.callURLs(); len(calls) != 0 {
		t.Fatalf("offline unknown made network calls: %#v", calls)
	}
}

func TestDefinitionRevisionRejectsBadReferencesBeforeRequest(t *testing.T) {
	fixture := newDefinitionsFixture(t)
	for _, reference := range []string{
		"master",
		"refs/tags/",
		"refs/heads/",
		"refs/heads//main",
		"refs/heads/main/",
		"refs/heads/../main",
		"refs/heads/main/../../other",
		"refs/heads/.hidden",
		"refs/heads/main.lock",
		"refs/heads/main?format=sha",
		"refs/heads/main\nother",
		"refs/remotes/main",
	} {
		t.Run(reference, func(t *testing.T) {
			_, err := fixture.defs.ResolveRevision(context.Background(), reference, false)
			if !errors.Is(err, ErrDefinitionIdentity) {
				t.Fatalf("reference %q error = %v, want ErrDefinitionIdentity", reference, err)
			}
		})
	}
	if calls := fixture.transport.callURLs(); len(calls) != 0 {
		t.Fatalf("bad references made network calls: %#v", calls)
	}
}

func TestDefinitionRevisionHTTPFailuresDoNotCache(t *testing.T) {
	for _, test := range []struct {
		name     string
		response definitionResponse
		wantErr  error
	}{
		{name: "status", response: definitionResponse{status: http.StatusBadGateway, body: []byte("bad")}},
		{name: "redirect", response: definitionResponse{status: http.StatusFound, location: "https://example.invalid/other"}},
		{name: "invalid sha", response: definitionResponse{status: http.StatusOK, body: []byte(strings.Repeat("g", 40))}, wantErr: ErrDefinitionIdentity},
		{name: "oversize", response: definitionResponse{status: http.StatusOK, body: bytes.Repeat([]byte{'a'}, definitionRevisionMaxBody+1)}, wantErr: ErrMetadataLimit},
	} {
		t.Run(test.name, func(t *testing.T) {
			fixture := newDefinitionsFixture(t)
			endpoint := definitionRevisionURL + url.PathEscape(definitionRevisionTestReference)
			fixture.transport.mu.Lock()
			fixture.transport.responses[endpoint] = test.response
			fixture.transport.mu.Unlock()

			_, err := fixture.defs.ResolveRevision(context.Background(), definitionRevisionTestReference, false)
			if test.wantErr != nil {
				if !errors.Is(err, test.wantErr) {
					t.Fatalf("error = %v, want %v", err, test.wantErr)
				}
			} else if err == nil || !strings.Contains(err.Error(), "records.definition_http: "+fmt.Sprint(test.response.status)) {
				t.Fatalf("error = %v, want records.definition_http: %d", err, test.response.status)
			}
			if _, readErr := fixture.metadata.ReadDocument(context.Background(), definitionRevisionKey(definitionRevisionTestReference)); !errors.Is(readErr, vault.ErrMissingRecord) {
				t.Fatalf("failed response cached: %v", readErr)
			}
		})
	}
}

func TestDefinitionRevisionCancellation(t *testing.T) {
	fixture := newDefinitionsFixture(t)
	started := make(chan struct{})
	fixture.defs.client.Transport = roundTripperFunc(func(request *http.Request) (*http.Response, error) {
		close(started)
		<-request.Context().Done()
		return nil, request.Context().Err()
	})
	ctx, cancel := context.WithCancel(context.Background())
	result := make(chan error, 1)
	go func() {
		_, err := fixture.defs.ResolveRevision(ctx, definitionRevisionTestReference, false)
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
			t.Fatalf("canceled ResolveRevision error = %v, want context.Canceled", err)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("canceled ResolveRevision did not return")
	}
	if _, err := fixture.metadata.ReadDocument(context.Background(), definitionRevisionKey(definitionRevisionTestReference)); !errors.Is(err, vault.ErrMissingRecord) {
		t.Fatalf("canceled resolution cached a record: %v", err)
	}
}

func TestDefinitionRevisionCorruptCacheFailsWithoutNetwork(t *testing.T) {
	fixture := newDefinitionsFixture(t)
	endpoint := definitionRevisionURL + url.PathEscape(definitionRevisionTestReference)
	fixture.transport.mu.Lock()
	fixture.transport.responses[endpoint] = definitionResponse{status: http.StatusOK, body: []byte(definitionRevisionTestCommitA)}
	fixture.transport.mu.Unlock()
	if _, err := fixture.defs.ResolveRevision(context.Background(), definitionRevisionTestReference, false); err != nil {
		t.Fatalf("seed ResolveRevision: %v", err)
	}
	doc, err := fixture.metadata.ReadDocument(context.Background(), definitionRevisionKey(definitionRevisionTestReference))
	if err != nil {
		t.Fatal(err)
	}
	if err := fixture.metadata.CommitDocuments(context.Background(), vault.Mutation{
		Key:                doc.Key,
		ExpectedGeneration: doc.Generation,
		Value:              json.RawMessage(`{"reference":"refs/heads/master","commit":"bad"}`),
	}); err != nil {
		t.Fatal(err)
	}
	calls := len(fixture.transport.callURLs())
	_, err = fixture.defs.ResolveRevision(context.Background(), definitionRevisionTestReference, false)
	if !errors.Is(err, ErrDefinitionIdentity) {
		t.Fatalf("corrupt cache error = %v, want ErrDefinitionIdentity", err)
	}
	if got := len(fixture.transport.callURLs()); got != calls {
		t.Fatalf("corrupt cache made %d new calls, want none", got-calls)
	}
}

func TestDefinitionRevisionConcurrentCASPreservesEachObservedCommit(t *testing.T) {
	fixture := newDefinitionsFixture(t)
	var calls atomic.Int32
	release := make(chan struct{})
	fixture.defs.client.Transport = roundTripperFunc(func(request *http.Request) (*http.Response, error) {
		n := calls.Add(1)
		if n == 2 {
			close(release)
		}
		<-release
		body := definitionRevisionTestCommitA
		if n == 2 {
			body = definitionRevisionTestCommitB
		}
		return &http.Response{StatusCode: http.StatusOK, Header: make(http.Header), Body: io.NopCloser(strings.NewReader(body)), Request: request}, nil
	})

	results := make(chan string, 2)
	errorsCh := make(chan error, 2)
	var group sync.WaitGroup
	group.Add(2)
	for range 2 {
		go func() {
			defer group.Done()
			commit, err := fixture.defs.ResolveRevision(context.Background(), definitionRevisionTestReference, false)
			results <- commit
			errorsCh <- err
		}()
	}
	group.Wait()
	close(results)
	close(errorsCh)
	var got []string
	for commit := range results {
		got = append(got, commit)
	}
	for err := range errorsCh {
		if err != nil {
			t.Fatalf("concurrent ResolveRevision error: %v", err)
		}
	}
	if len(got) != 2 || got[0] == got[1] || (got[0] != definitionRevisionTestCommitA && got[0] != definitionRevisionTestCommitB) || (got[1] != definitionRevisionTestCommitA && got[1] != definitionRevisionTestCommitB) {
		t.Fatalf("concurrent commits = %#v, want each caller's observation", got)
	}
	if calls.Load() != 2 {
		t.Fatalf("concurrent network calls = %d, want 2", calls.Load())
	}
	if _, err := fixture.metadata.ReadDocument(context.Background(), definitionRevisionKey(definitionRevisionTestReference)); err != nil {
		t.Fatalf("winner metadata missing: %v", err)
	}
}

func TestDefinitionRevisionEscapesValidRefNames(t *testing.T) {
	fixture := newDefinitionsFixture(t)
	for _, reference := range []string{"refs/heads/has#hash", "refs/heads/@", "refs/tags/release/nested"} {
		endpoint := definitionRevisionURL + url.PathEscape(reference)
		fixture.transport.responses[endpoint] = definitionResponse{status: http.StatusOK, body: []byte(definitionRevisionTestCommitA)}
		commit, err := fixture.defs.ResolveRevision(context.Background(), reference, false)
		if err != nil || commit != definitionRevisionTestCommitA {
			t.Fatal(reference, commit, err)
		}
	}
}
