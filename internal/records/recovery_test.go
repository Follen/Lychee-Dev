package records

import (
	"bytes"
	"context"
	"crypto/md5"
	"encoding/binary"
	"errors"
	"io"
	"sort"
	"testing"

	"github.com/follenfang/lycheedev/internal/records/container"
)

// A content row with two real encodings and an authenticated physical index.
func recoveryEncoding(t *testing.T, body []byte) (*EncodingIndex, [][]byte) {
	t.Helper()
	encoded := [][]byte{headerlessBLTE(body), frameCDNFilesEncodingTest(body)}
	raw := make([]byte, 22+32+1024+32+1024)
	copy(raw, "EN")
	raw[2], raw[3], raw[4] = 1, 16, 16
	binary.BigEndian.PutUint16(raw[5:7], 1)
	binary.BigEndian.PutUint16(raw[7:9], 1)
	binary.BigEndian.PutUint32(raw[9:13], 1)
	binary.BigEndian.PutUint32(raw[13:17], 1)
	ckey := md5.Sum(body)
	copy(raw[22:38], ckey[:])
	page := raw[54:1078]
	page[0] = 2
	putUint40Test(page[1:6], uint64(len(body)))
	copy(page[6:22], ckey[:])
	for i, e := range encoded {
		key := md5.Sum(e[:8])
		if binary.BigEndian.Uint32(e[4:8]) == 0 {
			key = md5.Sum(e)
		} else {
			key = md5.Sum(e[:36])
		}
		copy(page[22+i*16:], key[:])
	}
	sum := md5.Sum(page)
	copy(raw[38:54], sum[:])
	physical := append([][]byte(nil), encoded...)
	ekey := func(e []byte) []byte {
		n := len(e)
		if binary.BigEndian.Uint32(e[4:8]) != 0 {
			n = 36
		}
		k := md5.Sum(e[:n])
		return k[:]
	}
	sort.Slice(physical, func(i, j int) bool { return bytes.Compare(ekey(physical[i]), ekey(physical[j])) < 0 })
	copy(raw[1078:1094], ekey(physical[0]))
	physicalPage := raw[1110:]
	for i, e := range physical {
		at := i * 25
		copy(physicalPage[at:], ekey(e))
		putUint40Test(physicalPage[at+20:at+25], uint64(len(e)))
	}
	sum = md5.Sum(physicalPage)
	copy(raw[1094:1110], sum[:])
	framed := frameCDNFilesEncodingTest(raw)
	ranges, err := container.OpenRanges(context.Background(), bytes.NewReader(framed), int64(len(framed)), readLimits(1<<20, 1<<20), nil)
	if err != nil {
		t.Fatal(err)
	}
	index, err := OpenEncoding(context.Background(), ranges)
	if err != nil {
		t.Fatal(err)
	}
	return index, encoded
}

func TestContentRecoveryAcrossEncodings(t *testing.T) {
	for _, scenario := range []string{"http-then-success", "all-remote-missing", "http-then-missing", "integrity", "cancelled"} {
		t.Run(scenario, func(t *testing.T) {
			body := []byte("same content, alternative encoding")
			index, encoded := recoveryEncoding(t, body)
			_, store := newCDNFilesTest(t, &cdnTestTransport{}, false, nil)
			reader := OpenReader(store)
			calls := 0
			open := func(context.Context, string, int64) (encodedObject, error) {
				calls++
				if scenario == "all-remote-missing" {
					return nil, ErrRemoteObjectMissing
				}
				if scenario == "integrity" {
					return nil, container.ErrIntegrity
				}
				if scenario == "cancelled" {
					return nil, context.Canceled
				}
				if calls == 1 {
					return nil, ErrRemoteHTTP
				}
				if scenario == "http-then-missing" {
					return nil, ErrRemoteObjectMissing
				}
				return &remoteEncoded{io.NewSectionReader(bytes.NewReader(encoded[1]), 0, int64(len(encoded[1])))}, nil
			}
			ref, _, err := reader.extractContent(context.Background(), FileQuery{cacheStats: &DecodedCacheStats{}}, open, index, md5TestKey(body), 1<<20)
			switch scenario {
			case "http-then-success":
				if err != nil || calls != 2 {
					t.Fatalf("recovery: %v, calls=%d", err, calls)
				}
				got, err := store.ReadBlob(context.Background(), ref, 1<<20)
				if err != nil || !bytes.Equal(got, body) {
					t.Fatalf("content: %q %v", got, err)
				}
			case "all-remote-missing":
				if !errors.Is(err, ErrRemoteObjectMissing) || errors.Is(err, ErrObjectUnavailable) || calls != 2 {
					t.Fatalf("source lost: %v, calls=%d", err, calls)
				}
			case "http-then-missing":
				if !errors.Is(err, ErrRemoteHTTP) || calls != 2 {
					t.Fatalf("uncertainty lost: %v calls=%d", err, calls)
				}
			case "integrity":
				if !errors.Is(err, container.ErrIntegrity) || calls != 1 {
					t.Fatalf("integrity bypassed: %v", err)
				}
			case "cancelled":
				if !errors.Is(err, context.Canceled) || calls != 1 {
					t.Fatalf("cancellation ignored: %v", err)
				}
			}
		})
	}
}
