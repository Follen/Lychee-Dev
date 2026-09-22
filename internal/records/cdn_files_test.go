package records

import (
	"bytes"
	"context"
	"crypto/md5"
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"os"
	"path"
	"path/filepath"
	"strconv"
	"strings"
	"sync"
	"testing"

	"github.com/follenfang/lycheedev/internal/records/container"
	"github.com/follenfang/lycheedev/internal/vault"
)

type cdnTestTransport struct {
	mu      sync.Mutex
	objects map[string][]byte
	calls   []cdnTestCall
}

type cdnTestCall struct {
	object string
	offset int64
	length int64
}

func (t *cdnTestTransport) RoundTrip(request *http.Request) (*http.Response, error) {
	value := request.Header.Get("Range")
	if !strings.HasPrefix(value, "bytes=") {
		return nil, fmt.Errorf("missing range header: %q", value)
	}
	parts := strings.Split(strings.TrimPrefix(value, "bytes="), "-")
	if len(parts) != 2 {
		return nil, fmt.Errorf("invalid range header: %q", value)
	}
	offset, err := strconv.ParseInt(parts[0], 10, 64)
	if err != nil {
		return nil, fmt.Errorf("invalid range offset: %q", value)
	}
	end, err := strconv.ParseInt(parts[1], 10, 64)
	if err != nil || offset < 0 || end < offset {
		return nil, fmt.Errorf("invalid range end: %q", value)
	}
	object := path.Base(request.URL.Path)
	t.mu.Lock()
	data, ok := t.objects[object]
	if ok {
		data = bytes.Clone(data)
	}
	t.calls = append(t.calls, cdnTestCall{object: object, offset: offset, length: end - offset + 1})
	t.mu.Unlock()
	if !ok {
		return &http.Response{StatusCode: http.StatusNotFound, Request: request, Body: io.NopCloser(strings.NewReader("not found"))}, nil
	}
	if end >= int64(len(data)) {
		return nil, fmt.Errorf("range %q exceeds %s size %d", value, object, len(data))
	}
	body := bytes.Clone(data[offset : end+1])
	response := &http.Response{
		StatusCode:    http.StatusPartialContent,
		Request:       request,
		Body:          io.NopCloser(bytes.NewReader(body)),
		ContentLength: int64(len(body)),
		Header:        make(http.Header),
	}
	response.Header.Set("Content-Range", fmt.Sprintf("bytes %d-%d/%d", offset, end, len(data)))
	response.Header.Set("Content-Length", strconv.Itoa(len(body)))
	return response, nil
}

func (t *cdnTestTransport) callCount() int {
	t.mu.Lock()
	defer t.mu.Unlock()
	return len(t.calls)
}

func (t *cdnTestTransport) callsFor(object string) []cdnTestCall {
	t.mu.Lock()
	defer t.mu.Unlock()
	result := make([]cdnTestCall, 0)
	for _, call := range t.calls {
		if call.object == object {
			result = append(result, call)
		}
	}
	return result
}

func newCDNFilesTest(t *testing.T, transport http.RoundTripper, offline bool, fields map[string][]string) (*cdnFiles, *vault.Store) {
	t.Helper()
	ctx := context.Background()
	store, err := vault.Initialize(ctx, t.TempDir()+"/workspace")
	if err != nil {
		t.Fatal(err)
	}
	return newCDNFilesOnStoreTest(t, store, transport, offline, fields), store
}

func newCDNFilesOnStoreTest(t *testing.T, store *vault.Store, transport http.RoundTripper, offline bool, fields map[string][]string) *cdnFiles {
	t.Helper()
	ctx := context.Background()
	metadata, err := store.OpenMetadata(ctx)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = metadata.Close() })
	return &cdnFiles{
		ctx:      ctx,
		store:    store,
		metadata: metadata,
		client:   &http.Client{Transport: transport},
		route:    distributionRoute{Path: "test-route", Hosts: []string{"cdn.example.test"}},
		config:   ConfigDocument{Key: strings.Repeat("a", 32), Fields: fields},
		offline:  offline,
	}
}

func md5TestKey(raw []byte) string {
	sum := md5.Sum(raw)
	return hex.EncodeToString(sum[:])
}

func headerlessBLTE(body []byte) []byte {
	raw := make([]byte, 9+len(body))
	copy(raw, "BLTE")
	copy(raw[8:], append([]byte{'N'}, body...))
	return raw
}

func TestCDNFilesOpenLooseHeaderlessBLTEAndOfflineCache(t *testing.T) {
	body := []byte("loose headerless object")
	raw := headerlessBLTE(body)
	key := md5TestKey(raw)
	transport := &cdnTestTransport{objects: map[string][]byte{key: raw}}
	files, _ := newCDNFilesTest(t, transport, false, nil)

	object, err := files.open(context.Background(), key, int64(len(raw)))
	if err != nil {
		t.Fatal(err)
	}
	got, err := io.ReadAll(object)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(got, raw) {
		t.Fatalf("object = %x, want %x", got, raw)
	}
	onlineCalls := transport.callCount()
	if onlineCalls == 0 {
		t.Fatal("online open made no transport calls")
	}
	_ = object.Close()
	_ = files.metadata.Close()

	offlineTransport := &cdnTestTransport{objects: map[string][]byte{}}
	offlineMetadata, err := files.store.OpenMetadata(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	defer offlineMetadata.Close()
	offlineFiles := &cdnFiles{
		ctx:      context.Background(),
		store:    files.store,
		metadata: offlineMetadata,
		client:   &http.Client{Transport: offlineTransport},
		route:    files.route,
		config:   files.config,
		offline:  true,
	}

	cached, err := offlineFiles.open(context.Background(), key, int64(len(raw)))
	if err != nil {
		t.Fatal(err)
	}
	defer cached.Close()
	got, err = io.ReadAll(cached)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(got, raw) || offlineTransport.callCount() != 0 {
		t.Fatalf("offline cached read = %x, calls = %d", got, offlineTransport.callCount())
	}
}

func TestCDNFilesOpenOfflineCacheMiss(t *testing.T) {
	transport := &cdnTestTransport{objects: map[string][]byte{}}
	files, _ := newCDNFilesTest(t, transport, true, nil)
	key := strings.Repeat("1", 32)
	if _, err := files.open(context.Background(), key, 8); !errors.Is(err, ErrRemoteUnavailable) {
		t.Fatalf("open error = %v, want ErrRemoteUnavailable", err)
	}
	if transport.callCount() != 0 {
		t.Fatalf("offline cache miss made %d HTTP calls", transport.callCount())
	}
}

func TestCDNFilesSharedRangeCacheCoalescesConcurrentCallers(t *testing.T) {
	data := []byte("one shared range fetched exactly once")
	object := fmt.Sprintf("%032x", 5)
	transport := &cdnTestTransport{objects: map[string][]byte{object: data}}
	first, store := newCDNFilesTest(t, transport, false, nil)
	second := newCDNFilesOnStoreTest(t, store, transport, false, nil)

	start := make(chan struct{})
	type result struct {
		data []byte
		size int64
		err  error
	}
	results := make(chan result, 2)
	read := func(files *cdnFiles) {
		<-start
		got, size, err := files.rangeBytes(object, 0, int64(len(data)), int64(len(data)))
		results <- result{data: got, size: size, err: err}
	}
	go read(first)
	go read(second)
	close(start)
	for i := 0; i < 2; i++ {
		got := <-results
		if got.err != nil || got.size != int64(len(data)) || !bytes.Equal(got.data, data) {
			t.Fatalf("shared range result = %q, %d, %v", got.data, got.size, got.err)
		}
	}
	if calls := transport.callsFor(object); len(calls) != 1 {
		t.Fatalf("shared range HTTP calls = %#v, want exactly one", calls)
	}

	offlineTransport := &cdnTestTransport{objects: map[string][]byte{}}
	offline := newCDNFilesOnStoreTest(t, store, offlineTransport, true, nil)
	got, size, err := offline.rangeBytes(object, 0, int64(len(data)), int64(len(data)))
	if err != nil || size != int64(len(data)) || !bytes.Equal(got, data) || offlineTransport.callCount() != 0 {
		t.Fatalf("offline shared range = %q, %d, %v; HTTP calls = %d", got, size, err, offlineTransport.callCount())
	}
}

func TestCDNFilesCorruptCachedFragmentFailsWithoutDownload(t *testing.T) {
	data := []byte("cached fragment whose bytes will be corrupted")
	object := fmt.Sprintf("%032x", 6)
	transport := &cdnTestTransport{objects: map[string][]byte{object: data}}
	files, store := newCDNFilesTest(t, transport, false, nil)
	if _, _, err := files.rangeBytes(object, 0, int64(len(data)), int64(len(data))); err != nil {
		t.Fatal(err)
	}

	identity := fmt.Sprintf("%s/%s/%d/%d", files.route.Path, object, 0, len(data))
	sum := sha256.Sum256([]byte(identity))
	doc, err := files.metadata.ReadDocument(context.Background(), "cdn-range/"+hex.EncodeToString(sum[:]))
	if err != nil {
		t.Fatal(err)
	}
	var saved cdnRange
	if err := json.Unmarshal(doc.Value, &saved); err != nil {
		t.Fatal(err)
	}
	blobPath := filepath.Join(store.Root(), "blobs", saved.Blob.SHA256[:2], saved.Blob.SHA256[2:])
	corrupt := bytes.Repeat([]byte{'x'}, int(saved.Blob.Bytes))
	if err := os.WriteFile(blobPath, corrupt, 0600); err != nil {
		t.Fatal(err)
	}

	checkTransport := &cdnTestTransport{objects: map[string][]byte{object: data}}
	check := newCDNFilesOnStoreTest(t, store, checkTransport, false, nil)
	got, _, err := check.rangeBytes(object, 0, int64(len(data)), int64(len(data)))
	if got != nil || !errors.Is(err, vault.ErrBlobIntegrity) || checkTransport.callCount() != 0 {
		t.Fatalf("corrupt cached range = %q, %v; HTTP calls = %d", got, err, checkTransport.callCount())
	}

	offlineTransport := &cdnTestTransport{objects: map[string][]byte{}}
	offline := newCDNFilesOnStoreTest(t, store, offlineTransport, true, nil)
	got, _, err = offline.rangeBytes(object, 0, int64(len(data)), int64(len(data)))
	if got != nil || !errors.Is(err, vault.ErrBlobIntegrity) || offlineTransport.callCount() != 0 {
		t.Fatalf("offline corrupt cached range = %q, %v; HTTP calls = %d", got, err, offlineTransport.callCount())
	}
}

func TestCDNFilesOpenArchiveFallbackAuthenticatesIndexAndPage(t *testing.T) {
	body := []byte("archive encoded body")
	encoded := headerlessBLTE(body)
	ekey := md5TestKey(encoded)
	offset := uint64(37)
	index, archiveKey := cdnIndexFixture(4, []string{ekey}, []uint32{uint32(len(encoded))}, []uint64{offset})
	archive := make([]byte, int(offset)+len(encoded)+5)
	copy(archive[offset:], encoded)
	indexKey := archiveKey
	transport := &cdnTestTransport{objects: map[string][]byte{
		archiveKey:          archive,
		indexKey + ".index": index,
	}}
	files, _ := newCDNFilesTest(t, transport, false, map[string][]string{
		"archives": {archiveKey},
		// Deliberately wrong: the authenticated footer and HTTP total use len(index).
		"archives-index-size": {strconv.Itoa(len(index) - 1)},
	})

	object, err := files.open(context.Background(), ekey, int64(len(encoded)))
	if err != nil {
		t.Fatalf("open error = %v; index calls = %#v; archive calls = %#v", err, transport.callsFor(indexKey+".index"), transport.callsFor(archiveKey))
	}
	defer object.Close()
	got, err := io.ReadAll(object)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(got, encoded) {
		t.Fatalf("archive object = %x, want %x", got, encoded)
	}
	if len(transport.callsFor(indexKey+".index")) < 3 {
		t.Fatalf("index requests = %#v, want authenticated footer, TOC and page", transport.callsFor(indexKey+".index"))
	}
}

func TestCDNFilesOpenRejectsCorruptEKey(t *testing.T) {
	body := []byte("corrupt EKey")
	raw := headerlessBLTE(body)
	key := fmt.Sprintf("%032x", 3)
	transport := &cdnTestTransport{objects: map[string][]byte{key: raw}}
	files, _ := newCDNFilesTest(t, transport, false, nil)
	if _, err := files.open(context.Background(), key, int64(len(raw))); !errors.Is(err, container.ErrIntegrity) {
		t.Fatalf("open error = %v, want container.ErrIntegrity", err)
	}
}

func TestCDNFilesExtractContentRejectsWrongCKey(t *testing.T) {
	body := []byte("content whose key is intentionally wrong")
	raw := headerlessBLTE(body)
	ekey := md5TestKey(raw)
	wrongCKey := fmt.Sprintf("%032x", 4)
	index := openCDNFilesEncodingTestIndex(t, wrongCKey, ekey, int64(len(body)))
	transport := &cdnTestTransport{objects: map[string][]byte{ekey: raw}}
	files, store := newCDNFilesTest(t, transport, false, nil)
	reader := &Reader{store: store}
	_, _, err := reader.extractContent(context.Background(), FileQuery{MetadataBytes: 1 << 20, ContentBytes: 1 << 20}, func(ctx context.Context, key string, size int64) (encodedObject, error) {
		return files.open(ctx, key, size)
	}, index, wrongCKey, 1<<20)
	if !errors.Is(err, container.ErrIntegrity) {
		t.Fatalf("extractContent error = %v, want container.ErrIntegrity", err)
	}
}

func openCDNFilesEncodingTestIndex(t *testing.T, contentKey, encodingKey string, decodedBytes int64) *EncodingIndex {
	t.Helper()
	logical := make([]byte, 22+32+1024+32+1024)
	copy(logical[:2], "EN")
	logical[2], logical[3], logical[4] = 1, 16, 16
	binary.BigEndian.PutUint16(logical[5:7], 1)
	binary.BigEndian.PutUint16(logical[7:9], 1)
	binary.BigEndian.PutUint32(logical[9:13], 1)
	binary.BigEndian.PutUint32(logical[13:17], 1)
	copy(logical[22:38], mustDecodeTestKey(contentKey))
	page := logical[54:1078]
	page[0] = 1
	putUint40Test(page[1:6], uint64(decodedBytes))
	copy(page[6:22], mustDecodeTestKey(contentKey))
	copy(page[22:38], mustDecodeTestKey(encodingKey))
	digest := md5.Sum(page)
	copy(logical[38:54], digest[:])
	physicalDirectory := 54 + 1024
	copy(logical[physicalDirectory:physicalDirectory+16], mustDecodeTestKey(encodingKey))
	physicalPage := physicalDirectory + 32
	copy(logical[physicalPage:physicalPage+16], mustDecodeTestKey(encodingKey))
	binary.BigEndian.PutUint32(logical[physicalPage+16:physicalPage+20], 0)
	putUint40Test(logical[physicalPage+20:physicalPage+25], uint64(decodedBytes+9))
	physicalDigest := md5.Sum(logical[physicalPage : physicalPage+1024])
	copy(logical[physicalDirectory+16:physicalDirectory+32], physicalDigest[:])
	raw := frameCDNFilesEncodingTest(logical)
	ranges, err := container.OpenRanges(context.Background(), bytes.NewReader(raw), int64(len(raw)), container.Limits{EncodedBytes: 1 << 20, DecodedBytes: 1 << 20, ChunkBytes: 1 << 20, Chunks: 16, Depth: 4}, nil)
	if err != nil {
		t.Fatal(err)
	}
	index, err := OpenEncoding(context.Background(), ranges)
	if err != nil {
		t.Fatal(err)
	}
	return index
}

func frameCDNFilesEncodingTest(logical []byte) []byte {
	headerSize := 36
	block := append([]byte{'N'}, logical...)
	raw := make([]byte, headerSize+len(block))
	copy(raw[:4], "BLTE")
	binary.BigEndian.PutUint32(raw[4:8], uint32(headerSize))
	raw[8], raw[9], raw[10], raw[11] = 0x0f, 0, 0, 1
	binary.BigEndian.PutUint32(raw[12:16], uint32(len(block)))
	binary.BigEndian.PutUint32(raw[16:20], uint32(len(logical)))
	digest := md5.Sum(block)
	copy(raw[20:36], digest[:])
	copy(raw[36:], block)
	return raw
}

func mustDecodeTestKey(value string) []byte {
	decoded, err := hex.DecodeString(value)
	if err != nil || len(decoded) != md5.Size {
		panic("invalid test key")
	}
	return decoded
}

func putUint40Test(dst []byte, value uint64) {
	for i := 4; i >= 0; i-- {
		dst[i] = byte(value)
		value >>= 8
	}
}
