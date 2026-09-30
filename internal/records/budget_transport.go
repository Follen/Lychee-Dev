package records

import (
	"context"
	"errors"
	"io"
	"net/http"

	"github.com/follenfang/lycheedev/internal/records/resource"
)

// newBudgetTransport accounts every HTTP attempt, including metadata routes,
// redirects and failed attempts. Body bytes are charged when actually read;
// cached fragments never pass this transport and consume no network budget.
func newBudgetTransport(base http.RoundTripper, budget *resource.Budget) http.RoundTripper {
	if base == nil {
		base = http.DefaultTransport
	}
	if budget == nil {
		return base
	}
	return &queryBudgetTransport{base: base, budget: budget}
}

type queryBudgetTransport struct {
	base   http.RoundTripper
	budget *resource.Budget
}

func (t *queryBudgetTransport) RoundTrip(request *http.Request) (*http.Response, error) {
	if err := request.Context().Err(); err != nil {
		return nil, err
	}
	if err := t.budget.Charge(resource.Cost{NetworkRequests: 1}); err != nil {
		return nil, err
	}
	response, err := t.base.RoundTrip(request)
	if response != nil && response.Body != nil {
		response.Body = &queryBudgetBody{ctx: request.Context(), source: response.Body, budget: t.budget}
	}
	return response, err
}

type queryBudgetBody struct {
	ctx    context.Context
	source io.ReadCloser
	budget *resource.Budget
}

func (r *queryBudgetBody) Read(p []byte) (int, error) {
	if err := r.ctx.Err(); err != nil {
		return 0, err
	}
	if len(p) == 0 {
		return 0, nil
	}
	remaining := r.budget.RemainingNetworkBytes()
	// One extra byte distinguishes exact-budget EOF from an unfinished body.
	// It can only be returned together with an explicit budget failure.
	if int64(len(p)) > remaining+1 {
		p = p[:remaining+1]
	}
	n, err := r.source.Read(p)
	if charge := r.budget.Charge(resource.Cost{NetworkBytes: int64(n)}); charge != nil {
		// Keep an underlying cancellation/HTTP error even when this same read also
		// exhausted the resource budget. EOF alone is not a recovery signal.
		if err != nil && !errors.Is(err, io.EOF) {
			return n, errors.Join(charge, err)
		}
		return n, charge
	}
	return n, err
}
func (r *queryBudgetBody) Close() error { return r.source.Close() }
