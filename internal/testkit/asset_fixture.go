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
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/selection"
	"github.com/follenfang/lycheedev/internal/vault"
)

// CachedAsset creates an entirely synthetic, authenticated offline CDN file.
// Callers exercise real configuration, Encoding, Root, BLTE and evidence code;
// no network transport or application reader is replaced. FileDataID is 11.
func CachedAsset(t testing.TB, workspace string, content []byte) selection.PinnedSet {
	t.Helper()
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
	contentKey := md5key(content)
	payload := frame(content)
	payloadKey := md5key(payload)
	root := []byte("TSFM")
	for _, value := range []uint32{1, 0, 1, 0x10000000, 0x40, 11} {
		root = binary.LittleEndian.AppendUint32(root, value)
	}
	root = append(root, keyBytes(contentKey)...)
	rootKey := md5key(root)
	rootObject := frame(root)
	rootObjectKey := md5key(rootObject)
	type entry struct {
		content, encoding string
		decoded, encoded  int
	}
	entries := []entry{{contentKey, payloadKey, len(content), len(payload)}, {rootKey, rootObjectKey, len(root), len(rootObject)}}
	sort.Slice(entries, func(i, j int) bool { return entries[i].content < entries[j].content })
	logical := make([]byte, 22+32+1024+32+1024)
	copy(logical, "EN")
	logical[2], logical[3], logical[4] = 1, 16, 16
	binary.BigEndian.PutUint16(logical[5:7], 1)
	binary.BigEndian.PutUint16(logical[7:9], 1)
	binary.BigEndian.PutUint32(logical[9:13], 1)
	binary.BigEndian.PutUint32(logical[13:17], 1)
	put40 := func(dst []byte, n int) {
		for i := 4; i >= 0; i-- {
			dst[i] = byte(n)
			n >>= 8
		}
	}
	copy(logical[22:38], keyBytes(entries[0].content))
	page := logical[54:1078]
	for i, e := range entries {
		pos := i * 38
		page[pos] = 1
		put40(page[pos+1:pos+6], e.decoded)
		copy(page[pos+6:pos+22], keyBytes(e.content))
		copy(page[pos+22:pos+38], keyBytes(e.encoding))
	}
	copy(logical[38:54], keyBytes(md5key(page)))
	sort.Slice(entries, func(i, j int) bool { return entries[i].encoding < entries[j].encoding })
	copy(logical[1078:1094], keyBytes(entries[0].encoding))
	page = logical[1110:]
	for i, e := range entries {
		pos := i * 25
		copy(page[pos:pos+16], keyBytes(e.encoding))
		put40(page[pos+20:pos+25], e.encoded)
	}
	copy(logical[1094:1110], keyBytes(md5key(page)))
	encoding := make([]byte, 36)
	copy(encoding, "BLTE")
	binary.BigEndian.PutUint32(encoding[4:8], 36)
	encoding[8], encoding[11] = 15, 1
	block := append([]byte{'N'}, logical...)
	binary.BigEndian.PutUint32(encoding[12:16], uint32(len(block)))
	binary.BigEndian.PutUint32(encoding[16:20], uint32(len(logical)))
	copy(encoding[20:36], keyBytes(md5key(block)))
	encodingKey := md5key(encoding)
	encoding = append(encoding, block...)
	build := []byte(fmt.Sprintf("build-uid = wow\nroot = %s\nencoding = %s %s\nencoding-size = %d %d\n", rootKey, md5key(logical), encodingKey, len(logical), len(encoding)))
	cdn := []byte("archives =\n")
	buildKey, cdnKey := md5key(build), md5key(cdn)
	save("remote-config/"+buildKey, blob(build))
	save("remote-config/"+cdnKey, blob(cdn))
	save("cdn-route/wow/cn", blob([]byte("Name!STRING:0|Path!STRING:0|Hosts!STRING:0\ncn|fixture|fixture.invalid\n")))
	for key, object := range map[string][]byte{payloadKey: payload, rootObjectKey: rootObject, encodingKey: encoding} {
		save("cdn-location/"+cdnKey+"/"+key, map[string]any{"object": key, "offset": 0, "bytes": len(object)})
		for start := 0; start < len(object); start += 256 << 10 {
			end := min(start+(256<<10), len(object))
			digest := sha256.Sum256([]byte(fmt.Sprintf("fixture/%s/%d/%d", key, start, end-start)))
			save("cdn-range/"+hex.EncodeToString(digest[:]), map[string]any{"objectBytes": len(object), "blob": blob(object[start:end])})
		}
	}
	pin, err := selection.OpenPinner(metadata).PinSelection(ctx, selection.SelectionSpec{Data: &selection.DataPin{Product: "retail", Region: "cn", Language: "zhCN", FullBuild: "12.1.0.69875", BuildConfig: buildKey, CDNConfig: cdnKey, DefinitionCommit: strings.Repeat("c", 40)}})
	if err != nil {
		t.Fatal(err)
	}
	return pin
}
