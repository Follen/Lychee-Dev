package publish

import (
	"bytes"
	"context"
	"crypto/aes"
	"crypto/cipher"
	"crypto/sha256"
	"database/sql"
	"encoding/base64"
	"encoding/json"
	"path/filepath"
	"testing"
)

func TestDDCredentialUsesOnlyTypedSelectedAccount(t *testing.T) {
	for _, test := range []struct {
		name              string
		method, flag, typ int
		valid             bool
	}{{"mail", 1, 0, 1, true}, {"phone", 2, 4, 2, true}, {"mismatched", 1, 0, 2, false}, {"unsupported", 3, 0, 1, false}} {
		t.Run(test.name, func(t *testing.T) {
			path := filepath.Join(t.TempDir(), "cred.db")
			db, err := sql.Open("sqlite", path)
			if err != nil {
				t.Fatal(err)
			}
			_, err = db.Exec(`CREATE TABLE config(key TEXT PRIMARY KEY,value TEXT); CREATE TABLE account(type INTEGER,name TEXT,flag INTEGER,cred_type INTEGER,cred BLOB,PRIMARY KEY(type,name));`)
			if err != nil {
				t.Fatal(err)
			}
			selected, _ := json.Marshal([]any{test.method, "chosen"})
			db.Exec("INSERT INTO config VALUES('auto_account',?)", string(selected))
			db.Exec("INSERT INTO account VALUES(?,?,?,?,?)", test.method, "chosen", test.flag, test.typ, []byte("selected-encrypted"))
			db.Exec("INSERT INTO account VALUES(1,'decoy',0,1,?)", []byte("decoy-encrypted"))
			db.Close()
			calls := 0
			credential, err := ddReadCredential(context.Background(), path, func(raw, entropy []byte) ([]byte, error) {
				calls++
				if string(raw) != "selected-encrypted" || !bytes.Equal(entropy, ddEntropy) {
					t.Fatal("wrong credential or entropy")
				}
				return []byte("synthetic-token"), nil
			})
			if test.valid {
				if err != nil || credential.account != "chosen" || credential.token != "synthetic-token" || calls != 1 {
					t.Fatalf("selected: %v", err)
				}
			} else if err == nil || calls != 0 {
				t.Fatal("invalid selected type decrypted")
			}
		})
	}
}
func ddCookieDatabase(t *testing.T, version int, value string, encrypted []byte) []byte {
	t.Helper()
	db, err := sql.Open("sqlite", ":memory:")
	if err != nil {
		t.Fatal(err)
	}
	defer db.Close()
	db.SetMaxOpenConns(1)
	conn, err := db.Conn(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	defer conn.Close()
	if _, err = conn.ExecContext(context.Background(), `CREATE TABLE meta(key TEXT,value TEXT);CREATE TABLE cookies(host_key TEXT,name TEXT,value TEXT,encrypted_value BLOB,expires_utc INTEGER);`); err != nil {
		t.Fatal(err)
	}
	conn.ExecContext(context.Background(), "INSERT INTO meta VALUES('version',?)", version)
	conn.ExecContext(context.Background(), "INSERT INTO cookies VALUES('.163.com','NTES_YD_SESS',?,?,0)", value, encrypted)
	var image []byte
	if err = conn.Raw(func(v any) error {
		var e error
		image, e = v.(interface{ Serialize() ([]byte, error) }).Serialize()
		return e
	}); err != nil {
		t.Fatal(err)
	}
	return image
}
func TestDDDesktopCookieAESDomainAndNoDiskSnapshot(t *testing.T) {
	key := bytes.Repeat([]byte{3}, 32)
	block, _ := aes.NewCipher(key)
	gcm, _ := cipher.NewGCM(block)
	nonce := bytes.Repeat([]byte{4}, 12)
	hash := sha256.Sum256([]byte(".163.com"))
	plain := append(hash[:], []byte("synthetic-cookie")...)
	encrypted := append([]byte("v10"), nonce...)
	encrypted = append(encrypted, gcm.Seal(nil, nonce, plain, nil)...)
	state, _ := json.Marshal(map[string]any{"os_crypt": map[string]any{"encrypted_key": base64.StdEncoding.EncodeToString([]byte("DPAPIencrypted-key"))}})
	unprotect := func(raw, entropy []byte) ([]byte, error) {
		if string(raw) != "encrypted-key" || entropy != nil {
			t.Fatal("incorrect selected key")
		}
		return append([]byte(nil), key...), nil
	}
	cookie, err := ddCookieFromSnapshot(context.Background(), ddCookieDatabase(t, 24, "", encrypted), nil, state, unprotect)
	if err != nil || cookie != "synthetic-cookie" {
		t.Fatalf("cookie %v", err)
	}
	encrypted[len(encrypted)-1] ^= 1
	if _, err = ddCookieFromSnapshot(context.Background(), ddCookieDatabase(t, 24, "", encrypted), nil, state, unprotect); err == nil {
		t.Fatal("accepted unauthenticated ciphertext")
	}
	if _, err = ddCookieFromSnapshot(context.Background(), ddCookieDatabase(t, 24, "", []byte("v20unsupported")), nil, state, unprotect); err == nil {
		t.Fatal("accepted app-bound cookie")
	}
}
