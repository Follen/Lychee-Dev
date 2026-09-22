package testkit

import (
	"bytes"
	"context"
	"crypto/md5"
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"sort"
	"testing"

	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
)

// FileObject is one synthetic CDN file. Decoded fixes the content key; Object
// optionally replaces the stored BLTE bytes (for example an encrypted frame),
// in which case Decoded is still the CKey identity the decode must produce.
type FileObject struct {
	Decoded []byte
	Object  []byte
}

// CachedFiles creates an entirely synthetic, authenticated offline CDN root
// holding several files at caller-chosen FileDataIDs. Callers exercise the
// real configuration, Encoding, Root, BLTE and evidence code; no network
// transport or application reader is replaced. Object identity follows the
// same rules as CachedAsset, generalized to N files and N encoding pages.
func CachedFiles(t testing.TB, workspace string, files map[uint32]FileObject) selection.PinnedSet {
	t.Helper()
	if len(files) == 0 {
		t.Fatal("CachedFiles needs at least one file")
	}
	ctx := context.Background()
	store, err := vault.OpenStore(workspace)
	if err != nil {
		t.Fatal(err)
	}
	metadata, err := store.OpenMetadata(ctx)
	if err != nil {
		t.Fatal(err)
	}
	defer metadata.Close()
	save := func(key string, value any) {
		t.Helper()
		raw, err := json.Marshal(value)
		if err != nil {
			t.Fatal(err)
		}
		if err := metadata.CommitDocuments(ctx, vault.Mutation{Key: key, Value: raw}); err != nil {
			t.Fatal(err)
		}
	}
	blob := func(raw []byte) vault.BlobRef {
		t.Helper()
		ref, err := store.PublishBlob(ctx, vault.BlobInput{Reader: bytes.NewReader(raw), MaxBytes: int64(len(raw)) + 1})
		if err != nil {
			t.Fatal(err)
		}
		return ref
	}
	md5key := func(raw []byte) string { digest := md5.Sum(raw); return hex.EncodeToString(digest[:]) }
	keyBytes := func(key string) []byte { raw, _ := hex.DecodeString(key); return raw }
	frame := func(raw []byte) []byte { return append([]byte{'B', 'L', 'T', 'E', 0, 0, 0, 0, 'N'}, raw...) }

	ids := make([]uint32, 0, len(files))
	for id := range files {
		if id == 0 {
			t.Fatal("FileDataID 0 is not addressable")
		}
		ids = append(ids, id)
	}
	sort.Slice(ids, func(i, j int) bool { return ids[i] < ids[j] })
	root := []byte("TSFM")
	root = binary.LittleEndian.AppendUint32(root, uint32(len(ids)))
	root = binary.LittleEndian.AppendUint32(root, 0)
	root = binary.LittleEndian.AppendUint32(root, uint32(len(ids)))
	root = binary.LittleEndian.AppendUint32(root, 0x10000000)
	root = binary.LittleEndian.AppendUint32(root, 0x40)
	next := uint32(0)
	for _, id := range ids {
		delta := id - next
		root = binary.LittleEndian.AppendUint32(root, delta)
		next = id + 1
	}
	type entry = cachedEntry
	entries := []entry{}
	for _, id := range ids {
		object := files[id].Object
		if object == nil {
			object = frame(files[id].Decoded)
		}
		entries = append(entries, entry{
			content: md5key(files[id].Decoded), encoding: md5key(object),
			decoded: len(files[id].Decoded), encoded: len(object),
		})
		root = append(root, keyBytes(entries[len(entries)-1].content)...)
	}
	rootKey := md5key(root)
	rootObject := frame(root)
	rootObjectKey := md5key(rootObject)
	entries = append(entries, entry{rootKey, rootObjectKey, len(root), len(rootObject)})
	sort.Slice(entries, func(i, j int) bool { return entries[i].content < entries[j].content })

	const pageSize = 1024
	ckeyPages := chunkEntries(entries, (pageSize-1)/38)
	encoded := make([]cachedEntry, len(entries))
	copy(encoded, entries)
	sort.Slice(encoded, func(i, j int) bool { return encoded[i].encoding < encoded[j].encoding })
	ekeyPages := chunkEntries(encoded, (pageSize-1)/25)

	ckeyStart := 22
	pagesStart := ckeyStart + len(ckeyPages)*32
	ekeyDir := pagesStart + len(ckeyPages)*pageSize
	logical := make([]byte, ekeyDir+len(ekeyPages)*32+len(ekeyPages)*pageSize)
	copy(logical, "EN")
	logical[2], logical[3], logical[4] = 1, 16, 16
	binary.BigEndian.PutUint16(logical[5:7], 1)
	binary.BigEndian.PutUint16(logical[7:9], 1)
	binary.BigEndian.PutUint32(logical[9:13], uint32(len(ckeyPages)))
	binary.BigEndian.PutUint32(logical[13:17], uint32(len(ekeyPages)))
	put40 := func(dst []byte, n int) {
		for i := 4; i >= 0; i-- {
			dst[i] = byte(n)
			n >>= 8
		}
	}
	for index, page := range ckeyPages {
		body := logical[pagesStart+index*pageSize : pagesStart+(index+1)*pageSize]
		for i, e := range page {
			pos := i * 38
			body[pos] = 1
			put40(body[pos+1:pos+6], e.decoded)
			copy(body[pos+6:pos+22], keyBytes(e.content))
			copy(body[pos+22:pos+38], keyBytes(e.encoding))
		}
		directoryEntry := ckeyStart + index*32
		copy(logical[directoryEntry:directoryEntry+16], keyBytes(page[0].content))
		digest := md5.Sum(body)
		copy(logical[directoryEntry+16:directoryEntry+32], digest[:])
	}
	for index, page := range ekeyPages {
		body := logical[ekeyDir+len(ekeyPages)*32+index*pageSize : ekeyDir+len(ekeyPages)*32+(index+1)*pageSize]
		for i, e := range page {
			pos := i * 25
			copy(body[pos:pos+16], keyBytes(e.encoding))
			put40(body[pos+20:pos+25], e.encoded)
		}
		directoryEntry := ekeyDir + index*32
		copy(logical[directoryEntry:directoryEntry+16], keyBytes(page[0].encoding))
		digest := md5.Sum(body)
		copy(logical[directoryEntry+16:directoryEntry+32], digest[:])
	}

	blocks := [][]byte{logical[:pagesStart], logical[pagesStart:]}
	headerSize := 12 + 24*len(blocks)
	encoding := make([]byte, headerSize)
	copy(encoding, "BLTE")
	binary.BigEndian.PutUint32(encoding[4:8], uint32(headerSize))
	encoding[8] = 0x0f
	encoding[11] = byte(len(blocks))
	for index, payload := range blocks {
		block := append([]byte{'N'}, payload...)
		entry := 12 + index*24
		binary.BigEndian.PutUint32(encoding[entry:entry+4], uint32(len(block)))
		binary.BigEndian.PutUint32(encoding[entry+4:entry+8], uint32(len(payload)))
		digest := md5.Sum(block)
		copy(encoding[entry+8:entry+24], digest[:])
		encoding = append(encoding, block...)
	}
	// An object's key authenticates its BLTE header extent: the whole object
	// for zero-header frames, the chunk table for framed objects.
	encodingKey := md5key(encoding[:headerSize])

	build := []byte(fmt.Sprintf("build-uid = wow\nroot = %s\nencoding = %s %s\nencoding-size = %d %d\n", rootKey, md5key(logical), encodingKey, len(logical), len(encoding)))
	cdn := []byte("archives =\n")
	buildKey, cdnKey := md5key(build), md5key(cdn)
	save("remote-config/"+buildKey, blob(build))
	save("remote-config/"+cdnKey, blob(cdn))
	save("cdn-route/wow/cn", blob([]byte("Name!STRING:0|Path!STRING:0|Hosts!STRING:0\ncn|fixture|fixture.invalid\n")))
	for _, id := range ids {
		object := files[id].Object
		if object == nil {
			object = frame(files[id].Decoded)
		}
		cacheObject(t, save, blob, cdnKey, md5key(object), object)
	}
	cacheObject(t, save, blob, cdnKey, rootObjectKey, rootObject)
	cacheObject(t, save, blob, cdnKey, encodingKey, encoding)

	pin, err := selection.OpenPinner(metadata).PinSelection(ctx, selection.SelectionSpec{Data: &selection.DataPin{Product: "retail", Region: "cn", Language: "zhCN", FullBuild: "12.1.0.69875", BuildConfig: buildKey, CDNConfig: cdnKey, DefinitionCommit: definitionCommit}})
	if err != nil {
		t.Fatal(err)
	}
	return pin
}

// definitionCommit is the WoWDBDefs commit every synthetic pin references.
// Fixture tests pre-seed definitions at exactly this commit, so preparation
// stays offline.
const definitionCommit = "cccccccccccccccccccccccccccccccccccccccc"

func cacheObject(t testing.TB, save func(string, any), blob func([]byte) vault.BlobRef, cdnKey, key string, object []byte) {
	t.Helper()
	save("cdn-location/"+cdnKey+"/"+key, map[string]any{"object": key, "offset": 0, "bytes": len(object)})
	for start := 0; start < len(object); start += 256 << 10 {
		end := min(start+(256<<10), len(object))
		digest := sha256.Sum256([]byte(fmt.Sprintf("fixture/%s/%d/%d", key, start, end-start)))
		save("cdn-range/"+hex.EncodeToString(digest[:]), map[string]any{"objectBytes": len(object), "blob": blob(object[start:end])})
	}
}

// cachedEntry is one Encoding record pairing a content key with its encoding
// key and byte counts.
type cachedEntry struct {
	content, encoding string
	decoded, encoded  int
}

// chunkEntries splits entries into page groups of at most size.
func chunkEntries(entries []cachedEntry, size int) [][]cachedEntry {
	pages := [][]cachedEntry{}
	for start := 0; start < len(entries); start += size {
		end := min(start+size, len(entries))
		pages = append(pages, entries[start:end])
	}
	return pages
}
