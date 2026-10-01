package records

import (
	"bytes"
	"context"
	"errors"
	"io"
	"testing"

	"github.com/follenfang/lycheedev/internal/records/container"
)

type failingEncoded struct {
	failure, closeFailure error
	partial               int
	closed                bool
}

func (o *failingEncoded) Read(p []byte) (int, error) { return o.ReadAt(p, 0) }
func (o *failingEncoded) ReadAt(p []byte, _ int64) (int, error) {
	return min(len(p), o.partial), o.failure
}
func (o *failingEncoded) Close() error { o.closed = true; return o.closeFailure }

func TestDecodedCacheReadErrors(t *testing.T) {
	for _, failure := range []error{ErrRemoteHTTP, context.Canceled, context.DeadlineExceeded, io.EOF, nil} {
		for _, partial := range []int{0, 8} {
			q := FileQuery{keySource: &KeySource{Kind: "file", SHA256: "fixture"}}
			_, err := decodedCacheKey(context.Background(), q, &failingEncoded{failure: failure, partial: partial}, 64, "fixture")
			want := failure
			if want == nil || want == io.EOF {
				want = io.ErrUnexpectedEOF
			}
			if !errors.Is(err, want) {
				t.Fatalf("partial=%d failure=%v: got %v", partial, failure, err)
			}
		}
	}
}

func TestContentRecoveryAfterOpen(t *testing.T) {
	for _, cached := range []bool{false, true} {
		for _, scenario := range []string{"http", "missing", "all-http", "integrity", "cancelled", "deadline", "short", "close"} {
			t.Run(scenario+map[bool]string{true: "-cached", false: "-uncached"}[cached], func(t *testing.T) {
				body := []byte("healthy alternative after a lazy reader failure")
				index, encoded := recoveryEncoding(t, body)
				_, store := newCDNFilesTest(t, &cdnTestTransport{}, false, nil)
				q := FileQuery{cacheStats: &DecodedCacheStats{}}
				if cached {
					q.keySource = &KeySource{Kind: "file", SHA256: "fixture"}
				}
				failures := map[string]error{"http": ErrRemoteHTTP, "missing": ErrRemoteObjectMissing, "all-http": ErrRemoteHTTP, "integrity": container.ErrIntegrity, "cancelled": context.Canceled, "deadline": context.DeadlineExceeded, "short": io.ErrUnexpectedEOF, "close": ErrRemoteHTTP}
				bad := &failingEncoded{failure: failures[scenario]}
				if scenario == "close" {
					bad.closeFailure = errors.New("close failed")
				}
				calls := 0
				open := func(context.Context, string, int64) (encodedObject, error) {
					calls++
					if calls == 1 || scenario == "all-http" {
						return bad, nil
					}
					return &remoteEncoded{io.NewSectionReader(bytes.NewReader(encoded[1]), 0, int64(len(encoded[1])))}, nil
				}
				ref, _, err := OpenReader(store).extractContent(context.Background(), q, open, index, md5TestKey(body), 1<<20)
				if !bad.closed {
					t.Fatal("failed object was not closed")
				}
				if scenario == "http" || scenario == "missing" {
					if err != nil || calls != 2 {
						t.Fatalf("not recovered: %v calls=%d", err, calls)
					}
					got, readErr := store.ReadBlob(context.Background(), ref, 1<<20)
					if readErr != nil || !bytes.Equal(got, body) {
						t.Fatalf("wrong content: %q %v", got, readErr)
					}
				} else {
					wantCalls := 1
					if scenario == "all-http" {
						wantCalls = 2
					}
					want := failures[scenario]
					if scenario == "short" && !cached {
						want = container.ErrMalformed
					}
					if !errors.Is(err, want) || calls != wantCalls {
						t.Fatalf("classification/retry: %v calls=%d", err, calls)
					}
					if scenario == "close" && !errors.Is(err, bad.closeFailure) {
						t.Fatalf("lost close failure: %v", err)
					}
				}
			})
		}
	}
}
