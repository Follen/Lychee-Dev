package navigatetest

import (
	"context"
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"testing"

	"github.com/follenfang/lycheedev/internal/vault"
)

// Materialize the same authenticated fixture objects in a local archive/index,
// so local and offline-CDN cases differ only in physical source preparation.
// The source is synthetic; this is not installed-client acceptance.
func fixtureInstallation(t testing.TB, fx *fixture) string {
	t.Helper()
	ctx := context.Background()
	root := t.TempDir()
	store, err := vault.OpenStore(fx.workspace)
	if err != nil {
		t.Fatal(err)
	}
	metadata, err := store.OpenMetadata(ctx)
	if err != nil {
		t.Fatal(err)
	}
	defer metadata.Close()
	write := func(name string, raw []byte) {
		t.Helper()
		path := filepath.Join(root, name)
		if err := os.MkdirAll(filepath.Dir(path), 0700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(path, raw, 0600); err != nil {
			t.Fatal(err)
		}
	}
	for _, key := range []string{fx.pin.Data.BuildConfig, fx.pin.Data.CDNConfig} {
		doc, err := metadata.ReadDocument(ctx, "remote-config/"+key)
		if err != nil {
			t.Fatal(err)
		}
		var ref vault.BlobRef
		if err := json.Unmarshal(doc.Value, &ref); err != nil {
			t.Fatal(err)
		}
		raw, err := store.ReadBlob(ctx, ref, 4<<20)
		if err != nil {
			t.Fatal(err)
		}
		write(filepath.Join("Data", "config", key[:2], key[2:4], key), raw)
	}
	write(".build.info", []byte(fmt.Sprintf("Product!STRING:0|Version!STRING:0|Build Key!HEX:16|CDN Key!HEX:16|Active!DEC:1\nwow|%s|%s|%s|1\n", fx.pin.Data.FullBuild, fx.pin.Data.BuildConfig, fx.pin.Data.CDNConfig)))
	docs, err := metadata.ListDocuments(ctx, "cdn-location/", "", 1000)
	if err != nil {
		t.Fatal(err)
	}
	type entry struct {
		key            []byte
		offset, length int64
	}
	var entries []entry
	var archive []byte
	for _, doc := range docs {
		var location struct {
			Object        string
			Offset, Bytes int64
		}
		if err := json.Unmarshal(doc.Value, &location); err != nil {
			t.Fatal(err)
		}
		if location.Offset != 0 || location.Bytes > 256<<10 {
			t.Fatal("benchmark local fixture requires one bounded fragment per object")
		}
		key, err := hex.DecodeString(location.Object)
		if err != nil || len(key) != 16 {
			t.Fatal("invalid object key")
		}
		fragmentKey := sha256.Sum256([]byte(fmt.Sprintf("fixture/%s/0/%d", location.Object, location.Bytes)))
		fragment, err := metadata.ReadDocument(ctx, "cdn-range/"+hex.EncodeToString(fragmentKey[:]))
		if err != nil {
			t.Fatal(err)
		}
		var cached struct{ Blob vault.BlobRef }
		if err := json.Unmarshal(fragment.Value, &cached); err != nil {
			t.Fatal(err)
		}
		raw, err := store.ReadBlob(ctx, cached.Blob, 256<<10)
		if err != nil {
			t.Fatal(err)
		}
		entries = append(entries, entry{key: key, offset: int64(len(archive)), length: int64(len(raw) + 30)})
		archive = append(archive, make([]byte, 30)...)
		archive = append(archive, raw...)
	}
	sort.Slice(entries, func(i, j int) bool {
		return strings.Compare(hex.EncodeToString(entries[i].key), hex.EncodeToString(entries[j].key)) < 0
	})
	index := make([]byte, 40+len(entries)*18)
	binary.LittleEndian.PutUint32(index, 16)
	copy(index[8:], []byte{7, 0, 0, 0, 4, 5, 9, 30})
	binary.LittleEndian.PutUint64(index[16:], 1<<30)
	checksum, _ := fixtureGuardSum(index[8:24], 0, 0)
	binary.LittleEndian.PutUint32(index[4:], checksum)
	binary.LittleEndian.PutUint32(index[32:], uint32(len(entries)*18))
	var primary, secondary uint32
	for i, e := range entries {
		out := index[40+i*18 : 40+(i+1)*18]
		copy(out[:9], e.key[:9])
		binary.BigEndian.PutUint32(out[10:14], uint32(e.offset))
		binary.LittleEndian.PutUint32(out[14:], uint32(e.length))
		primary, secondary = fixtureGuardSum(out, primary, secondary)
	}
	binary.LittleEndian.PutUint32(index[36:], primary)
	write(filepath.Join("Data", "data", "data.000"), archive)
	write(filepath.Join("Data", "data", "0000000001.idx"), index)
	return root
}
