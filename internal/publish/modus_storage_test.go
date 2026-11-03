package publish

import (
	"context"
	"encoding/binary"
	"hash/crc32"
	"os"
	"path/filepath"
	"testing"
)

func modusFixtureVarint(n uint64) []byte { return binary.AppendUvarint(nil, n) }
func modusFixtureField(b []byte) []byte  { return append(modusFixtureVarint(uint64(len(b))), b...) }
func modusFixtureLog(record []byte) []byte {
	b := make([]byte, 7)
	b[6] = 1
	binary.LittleEndian.PutUint16(b[4:], uint16(len(record)))
	c := crc32.Checksum(append([]byte{1}, record...), crc32.MakeTable(crc32.Castagnoli))
	binary.LittleEndian.PutUint32(b, ((c>>15)|(c<<17))+0xa282ead8)
	return append(b, record...)
}
func modusFixtureBatch(seq uint64, key string, value *string) []byte {
	r := make([]byte, 12)
	binary.LittleEndian.PutUint64(r, seq)
	binary.LittleEndian.PutUint32(r[8:], 1)
	kind := byte(0)
	if value != nil {
		kind = 1
	}
	r = append(r, kind)
	r = append(r, modusFixtureField([]byte(key))...)
	if value != nil {
		r = append(r, modusFixtureField(append([]byte{1}, []byte(*value)...))...)
	}
	return modusFixtureLog(r)
}
func TestModusCurrentStorageTombstoneOriginAndCorruption(t *testing.T) {
	dir := t.TempDir()
	write := func(name string, b []byte) {
		t.Helper()
		if err := os.WriteFile(filepath.Join(dir, name), b, 0600); err != nil {
			t.Fatal(err)
		}
	}
	write("CURRENT", []byte("MANIFEST-000001\n"))
	write("MANIFEST-000001", modusFixtureLog([]byte{2, 2}))
	key := "_http://tongji.modus.cool\x00\x01user"
	old := `{"token":"old-account"}`
	current := `{"token":"current-account"}`
	foreign := `{"token":"foreign-account"}`
	// Old WALs and a different web origin must never supply this session.
	write("000001.log", modusFixtureBatch(100, key, &old))
	wal := append(modusFixtureBatch(1, key, &current), modusFixtureBatch(200, "_https://app.modus.cool\x00\x01user", &foreign)...)
	write("000002.log", wal)
	v, err := readModusLocalStorage(context.Background(), dir)
	if err != nil || v["user"] != current {
		t.Fatalf("selected stale or foreign credentials: %v", err)
	}
	wal = append(wal, modusFixtureBatch(2, key, nil)...)
	write("000002.log", wal)
	v, err = readModusLocalStorage(context.Background(), dir)
	if err != nil || len(v) != 0 {
		t.Fatalf("tombstone resurrected session: %v", err)
	}
	wal[8] ^= 1
	write("000002.log", wal)
	if _, err = readModusLocalStorage(context.Background(), dir); err == nil {
		t.Fatal("accepted damaged WAL")
	}
}
