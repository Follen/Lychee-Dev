package records

import (
	"context"
	"errors"
	"fmt"
	"io"
	"math"
	"net/http"
	"strconv"
	"strings"
)

const remoteRangeMaxBytes int64 = 64 << 20
const remoteRangeMaxObjectBytes int64 = 1 << 50

var (
	ErrRemoteRange         = errors.New("records.remote_range")
	ErrRemoteObjectMissing = errors.New("records.remote_object_missing")
)

// fetchRemoteRange obtains exactly one requested byte range. The returned
// objectBytes is the archive size advertised by Content-Range.
func fetchRemoteRange(ctx context.Context, client *http.Client, locator string, offset, length, total int64) (raw []byte, objectBytes int64, err error) {
	if err := ctx.Err(); err != nil {
		return nil, 0, err
	}
	if length < 1 || length > remoteRangeMaxBytes {
		return nil, 0, ErrMetadataLimit
	}
	if offset < 0 || total < 0 || offset > math.MaxInt64-(length-1) {
		return nil, 0, ErrRemoteRange
	}
	if total > remoteRangeMaxObjectBytes {
		return nil, 0, ErrMetadataLimit
	}
	end := offset + length - 1
	if total != 0 && total <= end {
		return nil, 0, ErrRemoteRange
	}
	if client == nil {
		return nil, 0, fmt.Errorf("%w: nil client", ErrRemoteHTTP)
	}

	request, err := http.NewRequestWithContext(ctx, http.MethodGet, locator, nil)
	if err != nil {
		return nil, 0, fmt.Errorf("%w: %v", ErrRemoteRange, err)
	}
	request.Header.Set("Range", "bytes="+strconv.FormatInt(offset, 10)+"-"+strconv.FormatInt(end, 10))
	request.Header.Set("Accept-Encoding", "identity")

	// Do not change caller-owned redirect policy. A shallow client copy keeps
	// its transport, timeout, jar and other settings while making redirects
	// visible to this bounded fetch.
	requestClient := *client
	requestClient.CheckRedirect = func(*http.Request, []*http.Request) error {
		return http.ErrUseLastResponse
	}
	response, err := requestClient.Do(request)
	if err != nil {
		if response != nil && response.Body != nil {
			response.Body.Close()
		}
		if ctxErr := ctx.Err(); ctxErr != nil {
			return nil, 0, ctxErr
		}
		return nil, 0, fmt.Errorf("%w: %w", ErrRemoteHTTP, err)
	}
	if response.Body == nil {
		return nil, 0, fmt.Errorf("%w: nil response body", ErrRemoteHTTP)
	}
	defer response.Body.Close()

	switch response.StatusCode {
	case http.StatusNotFound:
		return nil, 0, ErrRemoteObjectMissing
	case http.StatusOK:
		return nil, 0, fmt.Errorf("%w: status 200 would download the entire archive", ErrRemoteRange)
	case http.StatusPartialContent:
		// Continue with strict range validation below.
	default:
		if response.StatusCode >= 300 && response.StatusCode <= 399 {
			return nil, 0, fmt.Errorf("%w: redirect status %d", ErrRemoteRange, response.StatusCode)
		}
		return nil, 0, fmt.Errorf("%w: status %d", ErrRemoteHTTP, response.StatusCode)
	}

	start, responseEnd, responseTotal, err := parseRemoteContentRange(response.Header.Values("Content-Range"))
	if err != nil {
		return nil, 0, remoteRangeContentError(offset, end, total, response.Header.Values("Content-Range"))
	}
	if responseTotal > remoteRangeMaxObjectBytes {
		return nil, 0, fmt.Errorf("%w: Content-Range total exceeds %d: received %q", ErrMetadataLimit, remoteRangeMaxObjectBytes, strings.Join(response.Header.Values("Content-Range"), ", "))
	}
	if start != offset || responseEnd != end || responseTotal <= responseEnd || total != 0 && responseTotal != total {
		return nil, 0, remoteRangeContentError(offset, end, total, response.Header.Values("Content-Range"))
	}
	contentEncodingValues := response.Header.Values("Content-Encoding")
	if len(contentEncodingValues) > 1 || len(contentEncodingValues) == 1 && strings.TrimSpace(contentEncodingValues[0]) != "" && !strings.EqualFold(strings.TrimSpace(contentEncodingValues[0]), "identity") {
		return nil, 0, fmt.Errorf("%w: compressed response", ErrRemoteRange)
	}
	if contentType := strings.TrimSpace(response.Header.Get("Content-Type")); strings.HasPrefix(strings.ToLower(contentType), "multipart/") {
		return nil, 0, fmt.Errorf("%w: multipart response", ErrRemoteRange)
	}
	if values := response.Header.Values("Content-Length"); len(values) > 0 {
		if len(values) != 1 || !decimalHeader(values[0]) {
			return nil, 0, fmt.Errorf("%w: invalid Content-Length", ErrRemoteRange)
		}
		contentLength, parseErr := strconv.ParseInt(values[0], 10, 64)
		if parseErr != nil || contentLength != length {
			return nil, 0, fmt.Errorf("%w: mismatched Content-Length", ErrRemoteRange)
		}
	}

	bounded := io.LimitReader(&metadataReader{ctx: ctx, source: response.Body}, length+1)
	raw, err = io.ReadAll(bounded)
	if err != nil {
		if ctxErr := ctx.Err(); ctxErr != nil {
			return nil, 0, ctxErr
		}
		return nil, 0, fmt.Errorf("%w: %w", ErrRemoteHTTP, err)
	}
	if ctxErr := ctx.Err(); ctxErr != nil {
		return nil, 0, ctxErr
	}
	if int64(len(raw)) != length {
		return nil, 0, fmt.Errorf("%w: response body length %d, want %d", ErrRemoteRange, len(raw), length)
	}
	return raw, responseTotal, nil
}

func remoteRangeContentError(offset, end, total int64, received []string) error {
	wantedTotal := "*"
	if total != 0 {
		wantedTotal = strconv.FormatInt(total, 10)
	}
	wanted := "bytes " + strconv.FormatInt(offset, 10) + "-" + strconv.FormatInt(end, 10) + "/" + wantedTotal
	return fmt.Errorf("%w: invalid Content-Range: wanted %q, received %q", ErrRemoteRange, wanted, strings.Join(received, ", "))
}

func parseRemoteContentRange(values []string) (start, end, total int64, err error) {
	if len(values) != 1 {
		return 0, 0, 0, ErrRemoteRange
	}
	value := strings.TrimSpace(values[0])
	if !strings.HasPrefix(value, "bytes ") {
		return 0, 0, 0, ErrRemoteRange
	}
	value = strings.TrimPrefix(value, "bytes ")
	slash := strings.IndexByte(value, '/')
	dash := strings.IndexByte(value, '-')
	if dash <= 0 || slash <= dash+1 || strings.IndexByte(value[dash+1:], '-') >= 0 || strings.IndexByte(value[slash+1:], '/') >= 0 {
		return 0, 0, 0, ErrRemoteRange
	}
	startText, endText, totalText := value[:dash], value[dash+1:slash], value[slash+1:]
	if !decimalHeader(startText) || !decimalHeader(endText) || !decimalHeader(totalText) {
		return 0, 0, 0, ErrRemoteRange
	}
	start, err = strconv.ParseInt(startText, 10, 64)
	if err != nil {
		return 0, 0, 0, ErrRemoteRange
	}
	end, err = strconv.ParseInt(endText, 10, 64)
	if err != nil {
		return 0, 0, 0, ErrRemoteRange
	}
	total, err = strconv.ParseInt(totalText, 10, 64)
	if err != nil || start > end {
		return 0, 0, 0, ErrRemoteRange
	}
	return start, end, total, nil
}

func decimalHeader(value string) bool {
	if value == "" {
		return false
	}
	for i := 0; i < len(value); i++ {
		if value[i] < '0' || value[i] > '9' {
			return false
		}
	}
	return true
}
