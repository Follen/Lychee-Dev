package records

import (
	"context"
	"errors"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync/atomic"
	"testing"
)

func TestFetchRemoteRange(t *testing.T) {
	const (
		offset = int64(3)
		length = int64(5)
		total  = int64(20)
	)
	var gotRange, gotEncoding string
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		gotRange = r.Header.Get("Range")
		gotEncoding = r.Header.Get("Accept-Encoding")
		w.Header().Set("Content-Range", "bytes 3-7/20")
		w.Header().Set("Content-Length", "5")
		w.WriteHeader(http.StatusPartialContent)
		_, _ = io.WriteString(w, "fghij")
	}))
	defer server.Close()

	raw, gotTotal, err := fetchRemoteRange(context.Background(), server.Client(), server.URL, offset, length, total)
	if err != nil {
		t.Fatal(err)
	}
	if string(raw) != "fghij" || gotTotal != total {
		t.Fatalf("result = %q, %d", raw, gotTotal)
	}
	if gotRange != "bytes=3-7" || gotEncoding != "identity" {
		t.Fatalf("request headers = Range %q, Accept-Encoding %q", gotRange, gotEncoding)
	}
}

func TestFetchRemoteRangeUnknownSize(t *testing.T) {
	const objectSize = int64(1 << 50)
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Range", "bytes 8-10/1125899906842624")
		w.Header().Set("Content-Length", "3")
		w.WriteHeader(http.StatusPartialContent)
		_, _ = io.WriteString(w, "xyz")
	}))
	defer server.Close()

	raw, gotTotal, err := fetchRemoteRange(context.Background(), server.Client(), server.URL, 8, 3, 0)
	if err != nil || string(raw) != "xyz" || gotTotal != objectSize {
		t.Fatalf("result = %q, %d, %v", raw, gotTotal, err)
	}
}

func TestFetchRemoteRangeRejectsObjectTotalAboveMaximum(t *testing.T) {
	t.Run("request", func(t *testing.T) {
		called := atomic.Int32{}
		client := &http.Client{Transport: roundTripperFunc(func(r *http.Request) (*http.Response, error) {
			called.Add(1)
			return nil, errors.New("unexpected request")
		})}
		raw, objectSize, err := fetchRemoteRange(context.Background(), client, "http://example.test/object", 0, 1, remoteRangeMaxObjectBytes+1)
		if raw != nil || objectSize != 0 || !errors.Is(err, ErrMetadataLimit) || called.Load() != 0 {
			t.Fatalf("result = %q, %d, %v; calls = %d", raw, objectSize, err, called.Load())
		}
	})

	t.Run("response", func(t *testing.T) {
		server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			w.Header().Set("Content-Range", "bytes 0-0/1125899906842625")
			w.Header().Set("Content-Length", "1")
			w.WriteHeader(http.StatusPartialContent)
			_, _ = io.WriteString(w, "x")
		}))
		defer server.Close()

		raw, objectSize, err := fetchRemoteRange(context.Background(), server.Client(), server.URL, 0, 1, 0)
		if raw != nil || objectSize != 0 || !errors.Is(err, ErrMetadataLimit) {
			t.Fatalf("result = %q, %d, %v", raw, objectSize, err)
		}
	})
}

func TestFetchRemoteRangeStatuses(t *testing.T) {
	tests := []struct {
		name string
		code int
		want error
	}{
		{name: "full response", code: http.StatusOK, want: ErrRemoteRange},
		{name: "not found", code: http.StatusNotFound, want: ErrRemoteObjectMissing},
		{name: "server failure", code: http.StatusServiceUnavailable, want: ErrRemoteHTTP},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				w.WriteHeader(test.code)
				_, _ = io.WriteString(w, "not a range")
			}))
			defer server.Close()
			_, _, err := fetchRemoteRange(context.Background(), server.Client(), server.URL, 0, 3, 0)
			if !errors.Is(err, test.want) {
				t.Fatalf("error = %v, want %v", err, test.want)
			}
		})
	}
}

func TestFetchRemoteRangeRejectsRedirect(t *testing.T) {
	var targetCalls atomic.Int32
	target := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		targetCalls.Add(1)
		w.WriteHeader(http.StatusPartialContent)
	}))
	defer target.Close()
	redirect := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		http.Redirect(w, r, target.URL, http.StatusFound)
	}))
	defer redirect.Close()

	client := &http.Client{}
	_, _, err := fetchRemoteRange(context.Background(), client, redirect.URL, 0, 1, 0)
	if !errors.Is(err, ErrRemoteRange) || targetCalls.Load() != 0 || client.CheckRedirect != nil {
		t.Fatalf("error = %v, target calls = %d, CheckRedirect changed = %t", err, targetCalls.Load(), client.CheckRedirect != nil)
	}
}

func TestFetchRemoteRangeRejectsInvalidResponseMetadata(t *testing.T) {
	tests := []struct {
		name   string
		header string
		value  string
		body   string
		length string
		type_  string
	}{
		{name: "wrong start", header: "Content-Range", value: "bytes 1-4/10", body: "abcde", length: "5"},
		{name: "wrong end", header: "Content-Range", value: "bytes 0-3/10", body: "abcde", length: "5"},
		{name: "wrong known total", header: "Content-Range", value: "bytes 0-4/11", body: "abcde", length: "5"},
		{name: "unknown response total", header: "Content-Range", value: "bytes 0-4/*", body: "abcde", length: "5"},
		{name: "multipart range", header: "Content-Range", value: "bytes 0-4/10", body: "abcde", length: "5", type_: "multipart/byteranges"},
		{name: "compressed", header: "Content-Range", value: "bytes 0-4/10", body: "abcde", length: "5", type_: "application/octet-stream"},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				w.Header().Set(test.header, test.value)
				if test.type_ != "" {
					w.Header().Set("Content-Type", test.type_)
				}
				if test.name == "compressed" {
					w.Header().Set("Content-Encoding", "gzip")
				}
				w.Header().Set("Content-Length", test.length)
				w.WriteHeader(http.StatusPartialContent)
				_, _ = io.WriteString(w, test.body)
			}))
			defer server.Close()
			_, _, err := fetchRemoteRange(context.Background(), server.Client(), server.URL, 0, 5, 10)
			if !errors.Is(err, ErrRemoteRange) {
				t.Fatalf("error = %v, want ErrRemoteRange", err)
			}
		})
	}
}

func TestFetchRemoteRangeContentRangeErrorIncludesSafeDiagnostics(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Range", "bytes 1-4/10")
		w.Header().Set("Content-Length", "5")
		w.WriteHeader(http.StatusPartialContent)
		_, _ = io.WriteString(w, "abcde")
	}))
	defer server.Close()

	_, _, err := fetchRemoteRange(context.Background(), server.Client(), server.URL, 0, 5, 10)
	if !errors.Is(err, ErrRemoteRange) {
		t.Fatalf("error = %v, want ErrRemoteRange", err)
	}
	message := err.Error()
	if !strings.Contains(message, `wanted "bytes 0-4/10"`) || !strings.Contains(message, `received "bytes 1-4/10"`) || strings.Contains(message, server.URL) {
		t.Fatalf("diagnostic = %q", message)
	}
}

func TestFetchRemoteRangeRejectsShortAndLongBody(t *testing.T) {
	for _, body := range []string{"abcd", "abcdef"} {
		t.Run(body, func(t *testing.T) {
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				w.Header().Set("Content-Range", "bytes 0-4/10")
				w.WriteHeader(http.StatusPartialContent)
				_, _ = io.WriteString(w, body)
			}))
			defer server.Close()
			raw, _, err := fetchRemoteRange(context.Background(), server.Client(), server.URL, 0, 5, 10)
			if raw != nil || !errors.Is(err, ErrRemoteRange) {
				t.Fatalf("result = %q, error = %v", raw, err)
			}
		})
	}
}

func TestFetchRemoteRangeCancellationBeforeIO(t *testing.T) {
	called := atomic.Int32{}
	client := &http.Client{Transport: roundTripperFunc(func(r *http.Request) (*http.Response, error) {
		called.Add(1)
		return nil, errors.New("unexpected request")
	})}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()

	raw, objectSize, err := fetchRemoteRange(ctx, client, "http://example.test/object", 0, 1, 0)
	if raw != nil || objectSize != 0 || !errors.Is(err, context.Canceled) || called.Load() != 0 {
		t.Fatalf("result = %q, %d, %v; calls = %d", raw, objectSize, err, called.Load())
	}
}

func TestFetchRemoteRangeCancellationWhileReading(t *testing.T) {
	started := make(chan struct{})
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Range", "bytes 0-4/10")
		w.Header().Set("Content-Length", "5")
		w.WriteHeader(http.StatusPartialContent)
		flusher, _ := w.(http.Flusher)
		_, _ = io.WriteString(w, "a")
		flusher.Flush()
		close(started)
		<-r.Context().Done()
	}))
	defer server.Close()

	ctx, cancel := context.WithCancel(context.Background())
	result := make(chan error, 1)
	go func() {
		_, _, err := fetchRemoteRange(ctx, server.Client(), server.URL, 0, 5, 10)
		result <- err
	}()
	<-started
	cancel()
	if err := <-result; !errors.Is(err, context.Canceled) {
		t.Fatalf("error = %v, want context.Canceled", err)
	}
}

func TestFetchRemoteRangeClosesResponseBody(t *testing.T) {
	body := &remoteRangeTestBody{Reader: strings.NewReader("abcde")}
	client := &http.Client{Transport: roundTripperFunc(func(r *http.Request) (*http.Response, error) {
		return &http.Response{
			StatusCode: http.StatusPartialContent,
			Header: http.Header{
				"Content-Range":  []string{"bytes 0-4/10"},
				"Content-Length": []string{"5"},
			},
			Body:    body,
			Request: r,
		}, nil
	})}
	if _, _, err := fetchRemoteRange(context.Background(), client, "http://example.test/object", 0, 5, 10); err != nil {
		t.Fatal(err)
	}
	if !body.closed {
		t.Fatal("response body was not closed")
	}
}

func TestFetchRemoteRangeOversizeDoesNotMakeRequest(t *testing.T) {
	called := atomic.Int32{}
	client := &http.Client{Transport: roundTripperFunc(func(r *http.Request) (*http.Response, error) {
		called.Add(1)
		return nil, errors.New("unexpected request")
	})}
	raw, objectSize, err := fetchRemoteRange(context.Background(), client, "http://example.test/object", 0, remoteRangeMaxBytes+1, 0)
	if raw != nil || objectSize != 0 || !errors.Is(err, ErrMetadataLimit) || called.Load() != 0 {
		t.Fatalf("result = %q, %d, %v; calls = %d", raw, objectSize, err, called.Load())
	}
}

type remoteRangeTestBody struct {
	io.Reader
	closed bool
}

func (b *remoteRangeTestBody) Close() error {
	b.closed = true
	return nil
}
