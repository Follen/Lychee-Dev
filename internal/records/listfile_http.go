package records

import (
	"context"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"time"
)

// PublicListfileFetcher downloads the explicitly selected public listfile.
// The caller still chooses a kind; this adapter never chooses or falls back
// between sources, and the byte limit applies after redirects as well.
type PublicListfileFetcher struct{}

func (PublicListfileFetcher) FetchListfile(ctx context.Context, locator string, maxBytes int64) ([]byte, error) {
	parsed, err := url.Parse(locator)
	if err != nil || parsed.Scheme != "https" || parsed.Host == "" || parsed.User != nil || maxBytes < 1 {
		return nil, ErrListfileQuery
	}
	client := &http.Client{Timeout: 30 * time.Second, CheckRedirect: func(req *http.Request, via []*http.Request) error {
		if len(via) >= 5 || req.URL.Scheme != "https" || req.URL.User != nil {
			return ErrListfileQuery
		}
		return nil
	}}
	request, err := http.NewRequestWithContext(ctx, http.MethodGet, locator, nil)
	if err != nil {
		return nil, err
	}
	response, err := client.Do(request)
	if err != nil {
		return nil, err
	}
	defer response.Body.Close()
	if response.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("%w: HTTP %d", ErrListfileUnavailable, response.StatusCode)
	}
	if response.ContentLength > maxBytes {
		return nil, ErrListfileLimit
	}
	raw, err := io.ReadAll(io.LimitReader(response.Body, maxBytes+1))
	if err != nil {
		return nil, err
	}
	if int64(len(raw)) > maxBytes {
		return nil, ErrListfileLimit
	}
	if len(raw) == 0 {
		return nil, errors.New("empty listfile response")
	}
	return raw, nil
}
