package publish

// Read only the current Chromium records selected by CURRENT/MANIFEST. Deleted
// records and removed tables must not revive an older logged-in account.
import (
	"bytes"
	"context"
	"encoding/binary"
	"errors"
	"fmt"
	"hash/crc32"
	"io"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strconv"
	"strings"
	"unicode/utf16"

	"github.com/golang/snappy"
)

var errModusStorage = errors.New("invalid current Chromium storage")

type modusLDBCursor struct {
	b []byte
	p int
}

func (c *modusLDBCursor) number() (uint64, error) {
	v, n := binary.Uvarint(c.b[c.p:])
	if n <= 0 {
		return 0, errModusStorage
	}
	c.p += n
	return v, nil
}
func (c *modusLDBCursor) field() ([]byte, error) {
	n, err := c.number()
	if err != nil || n > uint64(len(c.b)-c.p) {
		return nil, errModusStorage
	}
	v := c.b[c.p : c.p+int(n)]
	c.p += int(n)
	return v, nil
}

var modusCRC = crc32.MakeTable(crc32.Castagnoli)

func modusMaskedCRC(b []byte) uint32 {
	c := crc32.Checksum(b, modusCRC)
	return (c>>15 | c<<17) + 0xa282ead8
}
func modusLogRecords(b []byte, consume func([]byte) error) error {
	var fragments []byte
	for p := 0; p < len(b); {
		remaining := 32768 - p%32768
		if remaining < 7 {
			p += remaining
			continue
		}
		if len(b)-p < 7 {
			return nil
		}
		crc := binary.LittleEndian.Uint32(b[p:])
		size := int(binary.LittleEndian.Uint16(b[p+4:]))
		kind := b[p+6]
		if crc == 0 && size == 0 && kind == 0 {
			p += remaining
			continue
		}
		if size > remaining-7 {
			return errModusStorage
		}
		p += 7
		if size > len(b)-p {
			return nil
		}
		body := b[p : p+size]
		p += size
		checked := make([]byte, 1+size)
		checked[0] = kind
		copy(checked[1:], body)
		if modusMaskedCRC(checked) != crc {
			return errModusStorage
		}
		switch kind {
		case 1:
			if fragments != nil {
				return errModusStorage
			}
			if err := consume(body); err != nil {
				return err
			}
		case 2:
			if fragments != nil {
				return errModusStorage
			}
			fragments = append([]byte{}, body...)
		case 3, 4:
			if fragments == nil || len(fragments)+len(body) > 64<<20 {
				return errModusStorage
			}
			fragments = append(fragments, body...)
			if kind == 4 {
				if err := consume(fragments); err != nil {
					return err
				}
				fragments = nil
			}
		default:
			return errModusStorage
		}
	}
	return nil
}

type modusLDBManifest struct {
	tables        map[uint64]bool
	log, previous uint64
	hasLog        bool
}

func modusManifest(raw []byte) (modusLDBManifest, error) {
	m := modusLDBManifest{tables: map[uint64]bool{}}
	err := modusLogRecords(raw, func(record []byte) error {
		c := modusLDBCursor{b: record}
		for c.p < len(c.b) {
			tag, err := c.number()
			if err != nil {
				return err
			}
			switch tag {
			case 1:
				_, err = c.field()
			case 2, 3, 4, 9:
				var n uint64
				n, err = c.number()
				if tag == 2 {
					m.log = n
					m.hasLog = true
				}
				if tag == 9 {
					m.previous = n
				}
			case 5:
				_, err = c.number()
				if err == nil {
					_, err = c.field()
				}
			case 6:
				_, err = c.number()
				if err == nil {
					var n uint64
					n, err = c.number()
					delete(m.tables, n)
				}
			case 7:
				_, err = c.number()
				var n uint64
				if err == nil {
					n, err = c.number()
				}
				if err == nil {
					_, err = c.number()
				}
				if err == nil {
					_, err = c.field()
				}
				if err == nil {
					_, err = c.field()
				}
				if err == nil {
					m.tables[n] = true
				}
			default:
				return errModusStorage
			}
			if err != nil {
				return err
			}
		}
		return nil
	})
	if err == nil && !m.hasLog {
		err = errModusStorage
	}
	return m, err
}

type modusLDBEntry func(uint64, []byte, []byte, bool) error

func modusBatches(raw []byte, consume modusLDBEntry) error {
	return modusLogRecords(raw, func(record []byte) error {
		if len(record) < 12 {
			return errModusStorage
		}
		seq := binary.LittleEndian.Uint64(record)
		count := binary.LittleEndian.Uint32(record[8:])
		c := modusLDBCursor{b: record, p: 12}
		for i := uint32(0); i < count; i++ {
			if c.p >= len(c.b) || seq > ^uint64(0)-uint64(i) {
				return errModusStorage
			}
			kind := c.b[c.p]
			c.p++
			key, err := c.field()
			if err != nil {
				return err
			}
			var value []byte
			if kind == 1 {
				value, err = c.field()
			} else if kind != 0 {
				return errModusStorage
			}
			if err != nil {
				return err
			}
			if err = consume(seq+uint64(i), key, value, kind == 0); err != nil {
				return err
			}
		}
		if c.p != len(c.b) {
			return errModusStorage
		}
		return nil
	})
}
func modusLDBBlock(raw, handle []byte) ([]byte, error) {
	c := modusLDBCursor{b: handle}
	offset, err := c.number()
	if err != nil {
		return nil, err
	}
	size, err := c.number()
	if err != nil || offset > uint64(len(raw)) || size > uint64(len(raw))-offset || uint64(len(raw))-offset-size < 5 {
		return nil, errModusStorage
	}
	b := raw[offset : offset+size]
	kind := raw[offset+size]
	if modusMaskedCRC(raw[offset:offset+size+1]) != binary.LittleEndian.Uint32(raw[offset+size+1:]) {
		return nil, errModusStorage
	}
	if kind == 0 {
		return b, nil
	}
	if kind != 1 {
		return nil, errModusStorage
	}
	n, err := snappy.DecodedLen(b)
	if err != nil || n > 64<<20 {
		return nil, errModusStorage
	}
	return snappy.Decode(nil, b)
}
func modusLDBBlockEntries(raw []byte, consume func([]byte, []byte) error) error {
	if len(raw) < 8 {
		return errModusStorage
	}
	count := uint64(binary.LittleEndian.Uint32(raw[len(raw)-4:]))
	if count < 1 || count > (uint64(len(raw))-4)/4 {
		return errModusStorage
	}
	end := len(raw) - 4 - int(count)*4
	c := modusLDBCursor{b: raw[:end]}
	var previous []byte
	for c.p < end {
		shared, err := c.number()
		if err != nil {
			return err
		}
		n, err := c.number()
		if err != nil {
			return err
		}
		v, err := c.number()
		if err != nil || shared > uint64(len(previous)) || n > uint64(end-c.p) || v > uint64(end-c.p)-n {
			return errModusStorage
		}
		key := append(append([]byte{}, previous[:shared]...), c.b[c.p:c.p+int(n)]...)
		c.p += int(n)
		value := c.b[c.p : c.p+int(v)]
		c.p += int(v)
		if err = consume(key, value); err != nil {
			return err
		}
		previous = key
	}
	return nil
}
func modusTable(raw []byte, consume modusLDBEntry) error {
	if len(raw) < 48 || !bytes.Equal(raw[len(raw)-8:], []byte{0x57, 0xfb, 0x80, 0x8b, 0x24, 0x75, 0x47, 0xdb}) {
		return errModusStorage
	}
	c := modusLDBCursor{b: raw[len(raw)-48 : len(raw)-8]}
	if _, err := c.number(); err != nil {
		return err
	}
	if _, err := c.number(); err != nil {
		return err
	}
	index, err := modusLDBBlock(raw, c.b[c.p:])
	if err != nil {
		return err
	}
	return modusLDBBlockEntries(index, func(_, handle []byte) error {
		block, err := modusLDBBlock(raw, handle)
		if err != nil {
			return err
		}
		return modusLDBBlockEntries(block, func(key, value []byte) error {
			if len(key) < 8 {
				return errModusStorage
			}
			tag := binary.LittleEndian.Uint64(key[len(key)-8:])
			kind := tag & 255
			if kind > 1 {
				return errModusStorage
			}
			return consume(tag>>8, key[:len(key)-8], value, kind == 0)
		})
	})
}
func modusChromiumString(raw []byte) (string, error) {
	if len(raw) == 0 {
		return "", errModusStorage
	}
	if raw[0] == 1 {
		r := make([]rune, len(raw)-1)
		for i, b := range raw[1:] {
			r[i] = rune(b)
		}
		return string(r), nil
	}
	if raw[0] != 0 || (len(raw)-1)%2 != 0 {
		return "", errModusStorage
	}
	u := make([]uint16, (len(raw)-1)/2)
	for i := range u {
		u[i] = binary.LittleEndian.Uint16(raw[1+i*2:])
	}
	for i := 0; i < len(u); i++ {
		if u[i] >= 0xd800 && u[i] <= 0xdbff {
			if i+1 == len(u) || u[i+1] < 0xdc00 || u[i+1] > 0xdfff {
				return "", errModusStorage
			}
			i++
		} else if u[i] >= 0xdc00 && u[i] <= 0xdfff {
			return "", errModusStorage
		}
	}
	return string(utf16.Decode(u)), nil
}
func modusReadBounded(filename string, limit int64) ([]byte, error) {
	f, err := os.Open(filename)
	if err != nil {
		return nil, err
	}
	defer f.Close()
	b, err := io.ReadAll(io.LimitReader(f, limit+1))
	if err != nil || int64(len(b)) > limit {
		return nil, errModusStorage
	}
	return b, nil
}

var modusManifestName = regexp.MustCompile(`^MANIFEST-[0-9]+$`)
var modusLogName = regexp.MustCompile(`^[0-9]+\.log$`)

func readModusLocalStorage(ctx context.Context, dir string) (map[string]string, error) {
	// Short retries account for an ordinary live compaction, never a fallback to
	// archived files. All files are opened read-only; no DB lock is acquired.
	for attempt := 0; attempt < 3; attempt++ {
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		v, err := readModusStorageSnapshot(ctx, dir)
		if err == nil {
			return v, nil
		}
	}
	return nil, capability("publish.modus_session", "cannot read a stable current ModUs main-client session")
}
func readModusStorageSnapshot(ctx context.Context, dir string) (map[string]string, error) {
	current, err := modusReadBounded(filepath.Join(dir, "CURRENT"), 256)
	if err != nil {
		return nil, err
	}
	name := strings.TrimSpace(string(current))
	if !modusManifestName.MatchString(name) {
		return nil, errModusStorage
	}
	manifest, err := modusReadBounded(filepath.Join(dir, name), 64<<20)
	if err != nil {
		return nil, err
	}
	m, err := modusManifest(manifest)
	if err != nil {
		return nil, err
	}
	files := map[string]bool{}
	for n := range m.tables {
		filename := fmt.Sprintf("%06d.ldb", n)
		if _, err := os.Stat(filepath.Join(dir, filename)); errors.Is(err, os.ErrNotExist) {
			filename = fmt.Sprintf("%06d.sst", n)
		}
		files[filename] = true
	}
	entries, err := os.ReadDir(dir)
	if err != nil {
		return nil, err
	}
	found := m.log == 0
	for _, e := range entries {
		if !modusLogName.MatchString(e.Name()) {
			continue
		}
		n, err := strconv.ParseUint(strings.TrimSuffix(e.Name(), ".log"), 10, 64)
		if err != nil {
			return nil, err
		}
		if n >= m.log || n == m.previous {
			files[e.Name()] = false
		}
		found = found || n == m.log
	}
	if !found {
		return nil, errModusStorage
	}
	type value struct {
		seq     uint64
		raw     []byte
		deleted bool
	}
	latest := map[string]value{}
	prefix := []byte("_http://tongji.modus.cool\x00")
	consume := func(seq uint64, key, raw []byte, deleted bool) error {
		if !bytes.HasPrefix(key, prefix) {
			return nil
		}
		name, err := modusChromiumString(key[len(prefix):])
		if err != nil {
			return err
		}
		if name != "user" && name != "app" && name != "wow" {
			return nil
		}
		old, ok := latest[name]
		if !ok || seq > old.seq {
			latest[name] = value{seq, append([]byte(nil), raw...), deleted}
		} else if seq == old.seq && (deleted != old.deleted || !deleted && !bytes.Equal(raw, old.raw)) {
			return errModusStorage
		}
		return nil
	}
	names := make([]string, 0, len(files))
	for n := range files {
		names = append(names, n)
	}
	sort.Strings(names)
	stats := map[string]os.FileInfo{}
	for _, name := range names {
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		filename := filepath.Join(dir, name)
		stat, err := os.Stat(filename)
		if err != nil || !stat.Mode().IsRegular() || stat.Size() > 512<<20 {
			return nil, errModusStorage
		}
		raw, err := modusReadBounded(filename, 512<<20)
		if err != nil {
			return nil, err
		}
		if files[name] {
			err = modusTable(raw, consume)
		} else {
			err = modusBatches(raw, consume)
		}
		if err != nil {
			return nil, err
		}
		stats[name] = stat
	}
	current2, err := modusReadBounded(filepath.Join(dir, "CURRENT"), 256)
	if err != nil || !bytes.Equal(current, current2) {
		return nil, errModusStorage
	}
	manifest2, err := modusReadBounded(filepath.Join(dir, name), 64<<20)
	if err != nil || !bytes.Equal(manifest, manifest2) {
		return nil, errModusStorage
	}
	for name, before := range stats {
		after, err := os.Stat(filepath.Join(dir, name))
		if err != nil || before.Size() != after.Size() || !before.ModTime().Equal(after.ModTime()) {
			return nil, errModusStorage
		}
	}
	result := map[string]string{}
	for name, v := range latest {
		if v.deleted {
			continue
		}
		s, err := modusChromiumString(v.raw)
		if err != nil {
			return nil, err
		}
		result[name] = s
	}
	return result, nil
}
