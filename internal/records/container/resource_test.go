package container_test

import (
	"bytes"
	"context"
	"errors"
	"io"
	"testing"

	"github.com/follenfang/lycheedev/internal/records/container"
	"github.com/follenfang/lycheedev/internal/records/resource"
)

type observedBody struct {
	*bytes.Reader
	header    int64
	bodyReads int
}

func (r *observedBody) Read(p []byte) (int, error) {
	pos, _ := r.Seek(0, io.SeekCurrent)
	if pos >= r.header {
		r.bodyReads++
	}
	return r.Reader.Read(p)
}
func (r *observedBody) ReadAt(p []byte, off int64) (int, error) {
	if off >= r.header {
		r.bodyReads++
	}
	return r.Reader.ReadAt(p, off)
}
func queryLimits(bytesLimit int64) container.Limits {
	l := generousLimits
	l.ChunkBytes, l.DecodedBytes = 1024, 1024
	l.Query = resource.New(resource.Limits{RetainedBytes: bytesLimit, MetadataBytes: 1 << 20, DecodeWork: 1000})
	return l
}
func assertScratchReleased(t *testing.T, l container.Limits, limit int64) {
	t.Helper()
	remaining := limit - l.Query.Snapshot().RetainedBytes
	if l.Query.RemainingRetained() != remaining {
		t.Fatal("live scratch leaked", l.Query.RemainingRetained(), remaining)
	}
	release, err := l.Query.ReserveScratch(remaining)
	if err != nil {
		t.Fatal("failed to reacquire released scratch", err)
	}
	release()
}
func TestQueryBudgetRejectsBeforeChunkReadOrWrite(t *testing.T) {
	data := framedBLTE(plainBlock("hello"))
	src := &observedBody{Reader: bytes.NewReader(data), header: 36}
	l := queryLimits(100)
	var dst bytes.Buffer
	_, err := container.Decode(context.Background(), &dst, src, l)
	if !errors.Is(err, resource.ErrBudget) || src.bodyReads != 0 || dst.Len() != 0 {
		t.Fatal(err, src.bodyReads, dst.Len())
	}
	assertScratchReleased(t, l, 100)
}
func TestQueryScratchReleasesOnSuccessIntegrityAndCancellation(t *testing.T) {
	for _, test := range []string{"success", "integrity", "cancel"} {
		t.Run(test, func(t *testing.T) {
			data := framedBLTE(plainBlock("first"), zlibBlock(t, "second"))
			if test == "integrity" {
				data[len(data)-1] ^= 1
			}
			ctx, cancel := context.WithCancel(context.Background())
			defer cancel()
			if test == "cancel" {
				cancel()
			}
			l := queryLimits(1 << 20)
			_, _, err := decodeBytesWithContext(ctx, data, l)
			switch test {
			case "success":
				if err != nil {
					t.Fatal(err)
				}
			case "integrity":
				if !errors.Is(err, container.ErrIntegrity) {
					t.Fatal(err)
				}
			case "cancel":
				if !errors.Is(err, context.Canceled) {
					t.Fatal(err)
				}
			}
			assertScratchReleased(t, l, 1<<20)
		})
	}
}
func decodeBytesWithContext(ctx context.Context, data []byte, l container.Limits) (int64, []byte, error) {
	var dst bytes.Buffer
	n, err := container.Decode(ctx, &dst, bytes.NewReader(data), l)
	return n, dst.Bytes(), err
}
func TestNestedFrameUsesConcurrentScratchReservation(t *testing.T) {
	inner := framedBLTE(plainBlock("hello"))
	data := framedBLTE(modeBlock('F', inner, 5))
	l := queryLimits(2000)
	_, _, err := decodeBytesWithContext(context.Background(), data, l)
	if !errors.Is(err, resource.ErrBudget) {
		t.Fatal(err)
	}
	assertScratchReleased(t, l, 2000)
	l = queryLimits(1 << 20)
	_, got, err := decodeBytesWithContext(context.Background(), data, l)
	if err != nil || string(got) != "hello" {
		t.Fatal(err, string(got))
	}
	if l.Query.PeakScratchBytes() < 2000 {
		t.Fatal("nested buffers not jointly charged", l.Query.PeakScratchBytes())
	}
	assertScratchReleased(t, l, 1<<20)
}
func TestRangeQueryBudgetRejectsBeforeBodyAndReleases(t *testing.T) {
	data := framedBLTE(plainBlock("hello"))
	src := &observedBody{Reader: bytes.NewReader(data), header: 36}
	l := queryLimits(100)
	r, err := container.OpenRanges(context.Background(), src, int64(len(data)), l, nil)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = r.ReadSpan(context.Background(), 0, 5); !errors.Is(err, resource.ErrBudget) || src.bodyReads != 0 {
		t.Fatal(err, src.bodyReads)
	}
	assertScratchReleased(t, l, 100)
	l = queryLimits(1 << 20)
	r, err = container.OpenRanges(context.Background(), bytes.NewReader(data), int64(len(data)), l, nil)
	if err != nil {
		t.Fatal(err)
	}
	var dst bytes.Buffer
	missing, err := r.WriteAvailable(context.Background(), &dst)
	if err != nil || len(missing) != 0 || dst.String() != "hello" {
		t.Fatal(err, missing, dst.String())
	}
	assertScratchReleased(t, l, 1<<20)
}
