package publish

import (
	"bytes"
	"context"
	"encoding/json"
	"io"
	"mime/multipart"
	"net/http"
	"net/url"
	"os"
	"path/filepath"
	"time"
)

const maxResponseBytes = 8 << 20

type HTTP struct{ client *http.Client }

func NewHTTP() *HTTP {
	return &HTTP{client: &http.Client{Timeout: 90 * time.Second, CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }}}
}
func (h *HTTP) JSON(ctx context.Context, method, endpoint string, headers http.Header, body any) (map[string]any, error) {
	value, err := h.Value(ctx, method, endpoint, headers, body)
	if err != nil {
		return nil, err
	}
	result, ok := value.(map[string]any)
	if !ok {
		return nil, failure("publish.response_shape", "platform response must be an object")
	}
	return result, nil
}
func (h *HTTP) Value(ctx context.Context, method, endpoint string, headers http.Header, body any) (any, error) {
	var input io.Reader
	if body != nil {
		raw, err := json.Marshal(body)
		if err != nil {
			return nil, err
		}
		input = bytes.NewReader(raw)
	}
	return h.request(ctx, method, endpoint, headers, input, "application/json")
}
func (h *HTTP) request(ctx context.Context, method, endpoint string, headers http.Header, body io.Reader, contentType string) (any, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	req, err := http.NewRequestWithContext(ctx, method, endpoint, body)
	if err != nil {
		return nil, invalid("$endpoint", "invalid endpoint")
	}
	if req.URL.User != nil || req.URL.Host == "" {
		return nil, invalid("$endpoint", "invalid endpoint authority")
	}
	req.Header = headers.Clone()
	if req.Header == nil {
		req.Header = make(http.Header)
	}
	req.Header.Set("Accept", "application/json")
	if body != nil {
		req.Header.Set("Content-Type", contentType)
	}
	response, err := h.client.Do(req)
	if err != nil {
		if ctx.Err() != nil {
			return nil, ctx.Err()
		}
		return nil, &Error{Code: "publish.transport", Message: "platform request failed", Exit: 5, Details: map[string]any{"method": method, "host": req.URL.Host, "submission": "uncertain"}}
	}
	defer response.Body.Close()
	if response.StatusCode < 200 || response.StatusCode >= 300 {
		return nil, &Error{Code: "publish.http_status", Message: "platform returned an unsuccessful status", Exit: 5, Details: map[string]any{"httpStatus": response.StatusCode, "host": req.URL.Host}}
	}
	raw, err := io.ReadAll(io.LimitReader(response.Body, maxResponseBytes+1))
	if err != nil {
		return nil, failure("publish.response_read", "failed to read platform response")
	}
	if len(raw) > maxResponseBytes {
		return nil, failure("publish.response_limit", "platform response exceeds 8 MiB")
	}
	d := json.NewDecoder(bytes.NewReader(raw))
	d.UseNumber()
	var result any
	if err = d.Decode(&result); err != nil || result == nil {
		return nil, failure("publish.response_shape", "platform response must be JSON")
	}
	if err = d.Decode(new(any)); err != io.EOF {
		return nil, failure("publish.response_shape", "platform response contains trailing data")
	}
	return result, nil
}
func (h *HTTP) Multipart(ctx context.Context, endpoint string, headers http.Header, fields map[string]string, fileField string, file *os.File) (map[string]any, error) {
	reader, writer := io.Pipe()
	multipartWriter := multipart.NewWriter(writer)
	done := make(chan error, 1)
	go func() {
		var err error
		for key, value := range fields {
			if err = multipartWriter.WriteField(key, value); err != nil {
				break
			}
		}
		if err == nil {
			var part io.Writer
			part, err = multipartWriter.CreateFormFile(fileField, filepath.Base(file.Name()))
			if err == nil {
				_, err = io.Copy(part, file)
			}
		}
		if err == nil {
			err = multipartWriter.Close()
		}
		writer.CloseWithError(err)
		done <- err
	}()
	result, err := h.request(ctx, http.MethodPost, endpoint, headers, reader, multipartWriter.FormDataContentType())
	reader.CloseWithError(err)
	writeErr := <-done
	if err != nil {
		return nil, err
	}
	if writeErr != nil {
		return nil, failure("publish.upload_read", "failed to stream the selected archive")
	}
	object, ok := result.(map[string]any)
	if !ok {
		return nil, failure("publish.response_shape", "upload response must be an object")
	}
	return object, nil
}

// Endpoint joins a fixed platform origin with a relative path and escaped query.
// Callers never use user-supplied URLs as authenticated platform origins.
func Endpoint(origin, path string, query url.Values) string {
	u := origin + path
	if len(query) > 0 {
		u += "?" + query.Encode()
	}
	return u
}
