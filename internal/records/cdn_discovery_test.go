package records

import (
	"context"
	"errors"
	"fmt"
	"io"
	"net/http"
	"sync/atomic"
	"testing"
	"time"

	"github.com/follenfang/lycheedev/internal/records/resource"
)

func TestCDNArchiveDiscoveryPreservesOrderedIntegrityErrors(t *testing.T) {
	for _, badFirst := range []bool{true, false} {
		t.Run(fmt.Sprint(badFirst), func(t *testing.T) {
			wanted := fmt.Sprintf("%032x", 7)
			transport := &cdnTestTransport{objects: map[string][]byte{}}
			var archives []string
			for i := 0; i < 2; i++ {
				index, archive := cdnIndexFixture(4, []string{wanted}, []uint32{128}, []uint64{uint64(i)})
				if (i == 0) == badFirst {
					index[len(index)-1] ^= 1
				}
				archives = append(archives, archive)
				transport.objects[archive+".index"] = index
			}
			files, _ := newCDNFilesTest(t, transport, false, map[string][]string{"archives": archives})
			files.budget = resource.New(resource.DefaultLimits())
			files.client.Transport = newBudgetTransport(transport, files.budget)
			span, err := files.locate(wanted, 128)
			if badFirst {
				if !errors.Is(err, ErrConfigurationIntegrity) {
					t.Fatalf("later result hid earlier corruption: %+v %v", span, err)
				}
			} else if err != nil || span.Object != archives[0] {
				t.Fatalf("speculative later failure changed first match: %+v %v", span, err)
			}
		})
	}
}

func TestCDNArchiveDiscoveryBatchesSmallIndexes(t *testing.T) {
	raw := headerlessBLTE([]byte("late archive on a remote CDN"))
	wanted := md5TestKey(raw)
	base := &cdnTestTransport{objects: map[string][]byte{}}
	var archives []string
	for i := 0; i < 8; i++ {
		key := fmt.Sprintf("%032x", i+1)
		if i == 7 {
			key = wanted
		}
		index, archive := cdnIndexFixture(4, []string{key}, []uint32{uint32(len(raw))}, []uint64{0})
		archives = append(archives, archive)
		base.objects[archive+".index"] = index
		if i == 7 {
			base.objects[archive] = raw
		}
	}
	var active, peak atomic.Int32
	transport := roundTripperFunc(func(r *http.Request) (*http.Response, error) {
		n := active.Add(1)
		defer active.Add(-1)
		for previous := peak.Load(); n > previous && !peak.CompareAndSwap(previous, n); previous = peak.Load() {
		}
		select {
		case <-r.Context().Done():
			return nil, r.Context().Err()
		case <-time.After(5 * time.Millisecond):
		}
		return base.RoundTrip(r)
	})
	files, _ := newCDNFilesTest(t, transport, false, map[string][]string{"archives": archives})
	limits := resource.DefaultLimits()
	limits.NetworkRequests = 20
	files.budget = resource.New(limits)
	files.client.Transport = newBudgetTransport(transport, files.budget)
	files.stats = &DecodedCacheStats{}
	object, err := files.open(context.Background(), wanted, int64(len(raw)))
	if err != nil {
		t.Fatalf("late archive did not fit request budget: %v (usage=%+v)", err, files.budget.Snapshot())
	}
	defer object.Close()
	got, err := io.ReadAll(object)
	if err != nil || string(got) != string(raw) {
		t.Fatalf("content mismatch: %x %v", got, err)
	}
	if peak.Load() < 2 || peak.Load() > 8 {
		t.Fatalf("archive lookup concurrency=%d", peak.Load())
	}
	for _, archive := range archives {
		if calls := base.callsFor(archive + ".index"); len(calls) != 2 {
			t.Fatalf("small index required %d requests", len(calls))
		}
	}
	if files.requests != base.callCount() || files.budget.Snapshot().NetworkRequests != int64(base.callCount()) {
		t.Fatalf("worker accounting lost: requests=%d transport=%d budget=%+v", files.requests, base.callCount(), files.budget.Snapshot())
	}
	before := base.callCount()
	files.offline = true
	last := archives[len(archives)-1]
	if _, err := files.lookupIndex(wanted, int64(len(raw)), archives, last, last, 0); err != nil || base.callCount() != before {
		t.Fatalf("cached indexes unavailable offline: %v", err)
	}
}

func TestCDNIndexReadsOldSparseCacheOffline(t *testing.T) {
	wanted := fmt.Sprintf("%032x", 7)
	index, archive := cdnIndexFixture(4, []string{wanted}, []uint32{128}, []uint64{0})
	transport := &cdnTestTransport{objects: map[string][]byte{archive + ".index": index}}
	files, _ := newCDNFilesTest(t, transport, false, nil)
	source, err := files.source(archive+".index", 0, true)
	if err != nil {
		t.Fatal(err)
	}
	parsed, err := openCDNIndex(context.Background(), source, source.size, archive)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := parsed.locate(context.Background(), wanted, []string{archive}, archive); err != nil {
		t.Fatal(err)
	}
	before := transport.callCount()
	files.offline = true
	if _, err := files.lookupIndex(wanted, 128, []string{archive}, archive, archive, 0); err != nil || transport.callCount() != before {
		t.Fatalf("old sparse cache unusable: %v", err)
	}
}
