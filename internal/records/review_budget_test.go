package records

import (
	"context"
	"errors"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/records/resource"
	"github.com/follenfang/lycheedev/internal/vault"
)

func TestCDNLocateStopsJoinedTerminalFailures(t *testing.T) {
	for _, stage := range []string{"loose", "group", "archive"} {
		for _, failure := range []error{resource.ErrBudget, ErrMetadataLimit, context.Canceled, context.DeadlineExceeded} {
			t.Run(stage+"/"+failure.Error(), func(t *testing.T) {
				calls := 0
				want := 1
				if stage != "loose" {
					want = 2
				}
				fields := map[string][]string{"archives": {strings.Repeat("b", 32), strings.Repeat("c", 32)}}
				if stage == "group" {
					fields["archive-group"] = []string{strings.Repeat("d", 32)}
				}
				files, _ := newCDNFilesTest(t, roundTripperFunc(func(request *http.Request) (*http.Response, error) {
					calls++
					if calls < want {
						return &http.Response{StatusCode: 404, Header: make(http.Header), Body: io.NopCloser(strings.NewReader(""))}, nil
					}
					return nil, errors.Join(ErrRemoteHTTP, failure)
				}), false, fields)
				_, err := files.locate(strings.Repeat("a", 32), 8)
				if !errors.Is(err, failure) || calls != want {
					t.Fatalf("terminal error triggered recovery: %v calls=%d want=%d", err, calls, want)
				}
			})
		}
	}
}

func TestSemanticParsingReservesBeforeRows(t *testing.T) {
	raw := []byte(strings.Repeat("0\n", 100000))
	limits := resource.DefaultLimits()
	limits.RetainedBytes = int64(len(raw)) + 160
	budget := resource.New(limits)
	rows, err := parseSemanticValues(context.Background(), raw, "12.1.0.69933", budget)
	if rows != nil || !errors.Is(err, resource.ErrBudget) || budget.Snapshot().DecodeWork != 1 || budget.Snapshot().RetainedBytes != limits.RetainedBytes {
		t.Fatalf("rows materialized past budget: rows=%d err=%v usage=%+v", len(rows), err, budget.Snapshot())
	}
	mappingsRaw := []byte("FLAGS SpellMisc::Attributes A\nFLAGS SpellMisc::Other B\n")
	limits.RetainedBytes = int64(len(mappingsRaw)) + 256
	budget = resource.New(limits)
	mappings, err := parseSemanticMappings(mappingsRaw, "SpellMisc", budget)
	if mappings != nil || !errors.Is(err, resource.ErrBudget) || budget.Snapshot().DecodeWork != 1 {
		t.Fatalf("mapping materialized past budget: %v %v %+v", mappings, err, budget.Snapshot())
	}
	// Dense malformed lines are rejected after a bounded token prefix.
	if _, err := parseSemanticValues(context.Background(), []byte(strings.Repeat("0 ", 100000)), "12.1.0.69933"); !errors.Is(err, ErrDefinitionIdentity) {
		t.Fatal(err)
	}
}

func TestKeySourceBudgetPrecedesMaterializationAndStaysLazy(t *testing.T) {
	store, err := vault.Initialize(context.Background(), t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(t.TempDir(), "keys.txt")
	if err = os.WriteFile(path, []byte("0000000000000001 000102030405060708090a0b0c0d0e0f"), 0600); err != nil {
		t.Fatal(err)
	}
	limits := resource.DefaultLimits()
	limits.RetainedBytes = 1
	budget := resource.New(limits)
	if _, _, err = OpenReader(store).prepareKeys(context.Background(), FileQuery{KeyFile: path, budget: budget}); !errors.Is(err, resource.ErrBudget) {
		t.Fatal(err)
	}
	lookup, source, err := OpenReader(store).prepareKeys(context.Background(), FileQuery{Offline: true, budget: budget})
	if err != nil || source.State != "not_needed" || budget.Snapshot() != (resource.Usage{}) {
		t.Fatalf("public provider was not lazy: %v %+v %+v", err, source, budget.Snapshot())
	}
	if _, err = lookup(context.Background(), 1); !errors.Is(err, resource.ErrBudget) {
		t.Fatal(err)
	}
	if budget.Snapshot() != (resource.Usage{}) {
		t.Fatal("failed multidimensional reservation changed counters")
	}
	limits = resource.DefaultLimits()
	limits.NetworkRequests = 0
	budget = resource.New(limits)
	lookup, _, err = OpenReader(store).prepareKeys(context.Background(), FileQuery{budget: budget})
	if err != nil {
		t.Fatal(err)
	}
	if _, err = lookup(context.Background(), 1); !errors.Is(err, resource.ErrBudget) {
		t.Fatalf("public key download escaped request budget: %v", err)
	}
}

func TestPublicKeyHTTPUsesSharedNetworkBudget(t *testing.T) {
	for _, scenario := range []string{"requests", "bytes", "redirect"} {
		t.Run(scenario, func(t *testing.T) {
			limits := resource.DefaultLimits()
			if scenario == "requests" {
				limits.NetworkRequests = 0
			}
			if scenario == "bytes" {
				limits.NetworkBytes = 1
			}
			if scenario == "redirect" {
				limits.NetworkRequests = 1
			}
			budget := resource.New(limits)
			calls := 0
			closed := false
			client := &http.Client{Transport: newBudgetTransport(roundTripperFunc(func(request *http.Request) (*http.Response, error) {
				calls++
				if scenario == "redirect" {
					return &http.Response{StatusCode: 302, Header: http.Header{"Location": {"https://raw.githubusercontent.com/redirect"}}, Body: &reviewBudgetBody{Reader: strings.NewReader(""), closed: &closed}, Request: request}, nil
				}
				return &http.Response{StatusCode: 200, Header: make(http.Header), Body: &reviewBudgetBody{Reader: strings.NewReader("too many bytes"), closed: &closed}, Request: request}, nil
			}), budget)}
			if _, err := fetchPublicKeysWithClient(context.Background(), client); !errors.Is(err, resource.ErrBudget) {
				t.Fatal(err)
			}
			want := 1
			if scenario == "requests" {
				want = 0
			}
			if calls != want || calls > 0 && !closed {
				t.Fatalf("calls=%d closed=%t", calls, closed)
			}
		})
	}
}

type reviewBudgetBody struct {
	io.Reader
	closed *bool
}

func (r *reviewBudgetBody) Close() error { *r.closed = true; return nil }
