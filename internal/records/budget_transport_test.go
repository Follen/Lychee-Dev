package records

import (
	"context"
	"errors"
	"io"
	"net/http"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/records/resource"
)

func TestBudgetTransportMetadata404AndRequestExhaustion(t *testing.T) {
	limits := resource.DefaultLimits()
	limits.NetworkRequests = 1
	budget := resource.New(limits)
	calls := 0
	transport := newBudgetTransport(roundTripperFunc(func(request *http.Request) (*http.Response, error) {
		calls++
		return &http.Response{StatusCode: 404, Body: io.NopCloser(strings.NewReader("missing")), Request: request}, nil
	}), budget)
	request, _ := http.NewRequestWithContext(context.Background(), "GET", "https://test.invalid/config", nil)
	response, err := transport.RoundTrip(request)
	if err != nil {
		t.Fatal(err)
	}
	raw, err := io.ReadAll(response.Body)
	response.Body.Close()
	if string(raw) != "missing" || err != nil || budget.Snapshot().NetworkBytes != 7 {
		t.Fatalf("%q %v %+v", raw, err, budget.Snapshot())
	}
	if _, err := transport.RoundTrip(request); !errors.Is(err, resource.ErrBudget) || calls != 1 {
		t.Fatalf("request cap %d %v", calls, err)
	}
}

func TestBudgetTransportByteLimitAndExactEOF(t *testing.T) {
	for _, test := range []struct {
		body      string
		wantError bool
	}{{"abc", false}, {"abcd", true}} {
		limits := resource.DefaultLimits()
		limits.NetworkBytes = 3
		budget := resource.New(limits)
		transport := newBudgetTransport(roundTripperFunc(func(request *http.Request) (*http.Response, error) {
			return &http.Response{StatusCode: 200, Body: io.NopCloser(strings.NewReader(test.body)), Request: request}, nil
		}), budget)
		request, _ := http.NewRequestWithContext(context.Background(), "GET", "https://test.invalid/object", nil)
		response, err := transport.RoundTrip(request)
		if err != nil {
			t.Fatal(err)
		}
		_, err = io.ReadAll(response.Body)
		response.Body.Close()
		if errors.Is(err, resource.ErrBudget) != test.wantError {
			t.Fatalf("%q: %v", test.body, err)
		}
	}
}

func TestBudgetTransportRetainsPartialNetworkErrorAndCancellation(t *testing.T) {
	for _, failure := range []error{ErrRemoteHTTP, context.Canceled, context.DeadlineExceeded} {
		limits := resource.DefaultLimits()
		limits.NetworkBytes = 3
		budget := resource.New(limits)
		body := &budgetFailedBody{failure: failure}
		transport := newBudgetTransport(roundTripperFunc(func(request *http.Request) (*http.Response, error) {
			return &http.Response{StatusCode: 200, Body: body, Request: request}, nil
		}), budget)
		request, _ := http.NewRequestWithContext(context.Background(), "GET", "https://test.invalid/object", nil)
		response, err := transport.RoundTrip(request)
		if err != nil {
			t.Fatal(err)
		}
		p := make([]byte, 8)
		n, err := response.Body.Read(p)
		if n != 2 || !errors.Is(err, failure) || budget.Snapshot().NetworkBytes != 2 {
			t.Fatalf("partial error: %d %v %+v", n, err, budget.Snapshot())
		}
		n, err = response.Body.Read(p)
		if n != 2 || !errors.Is(err, failure) || !errors.Is(err, resource.ErrBudget) {
			t.Fatalf("joined failure: %d %v", n, err)
		}
		response.Body.Close()
		if !body.closed {
			t.Fatal("body not closed")
		}
	}
}

func TestBudgetTransportChecksContextBeforeRequestAndRead(t *testing.T) {
	limits := resource.DefaultLimits()
	budget := resource.New(limits)
	calls := 0
	transport := newBudgetTransport(roundTripperFunc(func(request *http.Request) (*http.Response, error) {
		calls++
		return &http.Response{Body: io.NopCloser(strings.NewReader("body"))}, nil
	}), budget)
	ctx, cancel := context.WithCancel(context.Background())
	request, _ := http.NewRequestWithContext(ctx, "GET", "https://test.invalid/object", nil)
	response, err := transport.RoundTrip(request)
	if err != nil {
		t.Fatal(err)
	}
	cancel()
	if _, err := response.Body.Read(make([]byte, 8)); !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
	response.Body.Close()
	if _, err := transport.RoundTrip(request); !errors.Is(err, context.Canceled) || calls != 1 {
		t.Fatalf("cancelled request %d %v", calls, err)
	}
	if budget.Snapshot().NetworkRequests != 1 || budget.Snapshot().NetworkBytes != 0 {
		t.Fatal(budget.Snapshot())
	}
}

type budgetFailedBody struct {
	failure error
	closed  bool
}

func (b *budgetFailedBody) Read(p []byte) (int, error) { n := copy(p, "ab"); return n, b.failure }
func (b *budgetFailedBody) Close() error               { b.closed = true; return nil }
