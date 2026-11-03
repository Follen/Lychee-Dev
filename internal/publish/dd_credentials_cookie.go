package publish

import (
	"bytes"
	"context"
	"crypto/aes"
	"crypto/cipher"
	"crypto/sha256"
	"database/sql"
	"encoding/base64"
	"encoding/binary"
	"io/fs"
	"modernc.org/sqlite/vfs"
	"strings"
	"time"
	"unicode/utf8"
)

func ddCookieError() error {
	return capability("publish.dd_desktop_cookie", "the selected DD browser session is unavailable; no additional login was attempted")
}
func ddSQLiteImage(database, wal []byte) ([]byte, error) {
	if len(database) < 100 || len(database) > 16<<20 || string(database[:16]) != "SQLite format 3\x00" {
		return nil, ddCookieError()
	}
	pageSize := int(binary.BigEndian.Uint16(database[16:]))
	if pageSize == 1 {
		pageSize = 65536
	}
	if pageSize < 512 || pageSize > 65536 || pageSize&(pageSize-1) != 0 || len(database)%pageSize != 0 {
		return nil, ddCookieError()
	}
	result := append([]byte(nil), database...)
	if len(wal) > 0 {
		if len(wal) < 32 || len(wal) > 16<<20 {
			return nil, ddCookieError()
		}
		magic := binary.BigEndian.Uint32(wal)
		if (magic != 0x377f0682 && magic != 0x377f0683) || binary.BigEndian.Uint32(wal[4:]) != 3007000 || int(binary.BigEndian.Uint32(wal[8:])) != pageSize {
			return nil, ddCookieError()
		}
		var order binary.ByteOrder = binary.LittleEndian
		if magic == 0x377f0683 {
			order = binary.BigEndian
		}
		checksum := func(raw []byte, a, b uint32) (uint32, uint32) {
			for i := 0; i < len(raw); i += 8 {
				a += order.Uint32(raw[i:]) + b
				b += order.Uint32(raw[i+4:]) + a
			}
			return a, b
		}
		a, b := checksum(wal[:24], 0, 0)
		if a != binary.BigEndian.Uint32(wal[24:]) || b != binary.BigEndian.Uint32(wal[28:]) {
			return nil, ddCookieError()
		}
		type frame struct {
			page int
			data []byte
		}
		frames := []frame{}
		committed, finalSize := 0, 0
		for offset := 32; offset+24+pageSize <= len(wal); offset += 24 + pageSize {
			header := wal[offset : offset+24]
			if !bytes.Equal(header[8:16], wal[16:24]) {
				break
			}
			content := wal[offset+24 : offset+24+pageSize]
			na, nb := checksum(header[:8], a, b)
			na, nb = checksum(content, na, nb)
			if na != binary.BigEndian.Uint32(header[16:]) || nb != binary.BigEndian.Uint32(header[20:]) {
				break
			}
			page, size := int(binary.BigEndian.Uint32(header)), int(binary.BigEndian.Uint32(header[4:]))
			if page < 1 || page > 16<<20/pageSize || size > 16<<20/pageSize {
				return nil, ddCookieError()
			}
			a, b = na, nb
			frames = append(frames, frame{page, content})
			if size > 0 {
				committed = len(frames)
				finalSize = size
			}
		}
		if committed > 0 {
			for _, f := range frames[:committed] {
				end := f.page * pageSize
				if end > len(result) {
					result = append(result, make([]byte, end-len(result))...)
				}
				copy(result[(f.page-1)*pageSize:end], f.data)
			}
			end := finalSize * pageSize
			if end > len(result) {
				return nil, ddCookieError()
			}
			result = result[:end]
		}
	}
	result[18], result[19] = 1, 1
	return result, nil
}
func ddCookieFromSnapshot(ctx context.Context, database, wal, state []byte, unprotect func([]byte, []byte) ([]byte, error)) (string, error) {
	image, err := ddSQLiteImage(database, wal)
	if err != nil {
		return "", err
	}
	defer clear(image)
	var local map[string]any
	if len(state) > 4<<20 || ddJSON(state, &local) != nil {
		return "", ddCookieError()
	}
	crypt, _ := local["os_crypt"].(map[string]any)
	encoded, _ := crypt["encrypted_key"].(string)
	encryptedKey, err := base64.StdEncoding.Strict().DecodeString(encoded)
	if err != nil || !bytes.HasPrefix(encryptedKey, []byte("DPAPI")) {
		return "", ddCookieError()
	}
	key, err := unprotect(encryptedKey[5:], nil)
	if err != nil {
		return "", ddCookieError()
	}
	defer clear(key)
	block, err := aes.NewCipher(key)
	if err != nil {
		return "", ddCookieError()
	}
	gcm, err := cipher.NewGCM(block)
	if err != nil {
		return "", ddCookieError()
	}
	name, memory, err := vfs.New(ddCookieFS{image})
	if err != nil {
		return "", ddCookieError()
	}
	defer memory.Close()
	db, err := sql.Open("sqlite", "file:cookies.db?mode=ro&vfs="+name)
	if err != nil {
		return "", ddCookieError()
	}
	defer db.Close()
	db.SetMaxOpenConns(1)
	conn, err := db.Conn(ctx)
	if err != nil {
		return "", ddCookieError()
	}
	defer conn.Close()
	var version int
	if conn.QueryRowContext(ctx, "SELECT value FROM meta WHERE key='version'").Scan(&version) != nil {
		return "", ddCookieError()
	}
	rows, err := conn.QueryContext(ctx, "SELECT host_key,value,encrypted_value,expires_utc FROM cookies WHERE host_key='.163.com' AND name='NTES_YD_SESS'")
	if err != nil {
		return "", ddCookieError()
	}
	defer rows.Close()
	now := (time.Now().Unix() + 11644473600) * 1000000
	cookies := []string{}
	for rows.Next() {
		var host, value string
		var encrypted []byte
		var expires int64
		if rows.Scan(&host, &value, &encrypted, &expires) != nil {
			return "", ddCookieError()
		}
		if expires != 0 && expires <= now {
			continue
		}
		var plain []byte
		if bytes.HasPrefix(encrypted, []byte("v10")) {
			if len(encrypted) < 31 {
				return "", ddCookieError()
			}
			plain, err = gcm.Open(nil, encrypted[3:15], encrypted[15:], nil)
			if err != nil {
				return "", ddCookieError()
			}
			if version >= 24 {
				hash := sha256.Sum256([]byte(host))
				if len(plain) < 32 || !bytes.Equal(plain[:32], hash[:]) {
					clear(plain)
					return "", ddCookieError()
				}
				plain = plain[32:]
			}
			if !utf8.Valid(plain) {
				clear(plain)
				return "", ddCookieError()
			}
			value = string(plain)
			clear(plain)
		} else if bytes.HasPrefix(encrypted, []byte("v20")) {
			return "", capability("publish.dd_desktop_cookie", "the selected browser uses unsupported application-bound cookie encryption")
		} else if len(encrypted) > 0 {
			plain, err = unprotect(encrypted, nil)
			if err != nil || !utf8.Valid(plain) {
				return "", ddCookieError()
			}
			value = string(plain)
			clear(plain)
		}
		if value != "" && len(value) <= 16384 && !strings.ContainsAny(value, ";\r\n\x00") {
			cookies = append(cookies, value)
		}
		if len(cookies) > 1 {
			return "", ddCookieError()
		}
	}
	if rows.Err() != nil || len(cookies) != 1 {
		return "", ddCookieError()
	}
	return cookies[0], nil
}

// A read-only VFS keeps the scoped browser snapshot in memory. The existing
// sqlite driver's Deserialize allocator is not used: its Windows close path
// frees TLS-owned storage through sqlite3_free and crashes on this baseline.
type ddCookieFS struct{ image []byte }

func (f ddCookieFS) Open(name string) (fs.File, error) {
	if name != "cookies.db" {
		return nil, fs.ErrNotExist
	}
	return &ddCookieFile{Reader: bytes.NewReader(f.image), size: int64(len(f.image))}, nil
}

type ddCookieFile struct {
	*bytes.Reader
	size int64
}

func (f *ddCookieFile) Close() error               { return nil }
func (f *ddCookieFile) Stat() (fs.FileInfo, error) { return ddCookieInfo{f.size}, nil }

type ddCookieInfo struct{ size int64 }

func (ddCookieInfo) Name() string       { return "cookies.db" }
func (f ddCookieInfo) Size() int64      { return f.size }
func (ddCookieInfo) Mode() fs.FileMode  { return 0400 }
func (ddCookieInfo) ModTime() time.Time { return time.Time{} }
func (ddCookieInfo) IsDir() bool        { return false }
func (ddCookieInfo) Sys() any           { return nil }
