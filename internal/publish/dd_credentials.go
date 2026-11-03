package publish

import (
	"context"
	"database/sql"
	"encoding/json"
	"fmt"
	_ "modernc.org/sqlite"
	"net/url"
	"strings"
	"unicode/utf8"
)

type ddCredential struct {
	method         int
	account, token string
}

func ddCredentialError() error {
	return capability("publish.dd_saved_login", "selected DD auto-login credential is unavailable or incompatible; sign in with the DD desktop client")
}
func (c ddCredential) loginType() string {
	if c.method == 1 {
		return "ursMailToken"
	}
	return "ursPhoneTokenNew"
}
func (c ddCredential) username() string {
	if c.method == 1 {
		return c.account
	}
	return c.account + "@mobile.163.com"
}

var ddEntropy = []byte{0x47, 0x78, 0x9a, 0xa3, 0x33, 0x5b, 0xbb, 0xd0, 0xc0, 0x56, 0x95, 0xbf, 0x0e, 0x87, 0x62, 0x52, 0xca, 0xde, 0x9d, 0x94, 0x65, 0x9b, 0x23, 0x93, 0x92, 0x6f, 0x4f, 0xfd, 0xe4, 0x72, 0xa0, 0x3c}

func ddReadCredential(ctx context.Context, path string, unprotect func([]byte, []byte) ([]byte, error)) (ddCredential, error) {
	var out ddCredential
	if err := ctx.Err(); err != nil {
		return out, err
	}
	path = strings.ReplaceAll(path, "\\", "/")
	if !strings.HasPrefix(path, "/") {
		path = "/" + path
	}
	u := url.URL{Scheme: "file", Path: path}
	q := u.Query()
	q.Set("mode", "ro")
	q.Add("_pragma", "busy_timeout(150)")
	u.RawQuery = q.Encode()
	db, err := sql.Open("sqlite", u.String())
	if err != nil {
		return out, ddCredentialError()
	}
	defer db.Close()
	db.SetMaxOpenConns(1)
	tx, err := db.BeginTx(ctx, &sql.TxOptions{ReadOnly: true})
	if err != nil {
		return out, ddCredentialError()
	}
	defer tx.Rollback()
	var raw string
	if tx.QueryRowContext(ctx, "SELECT value FROM config WHERE key='auto_account'").Scan(&raw) != nil || len(raw) > 4096 {
		return out, ddCredentialError()
	}
	var selected []any
	if ddJSON([]byte(raw), &selected) != nil || len(selected) != 2 {
		return out, ddCredentialError()
	}
	method, ok := selected[0].(json.Number)
	if !ok {
		return out, ddCredentialError()
	}
	m, e := method.Int64()
	name, ok := selected[1].(string)
	if e != nil || (m != 1 && m != 2) || !ok || strings.TrimSpace(name) == "" || len(name) > 2048 || strings.ContainsRune(name, 0) {
		return out, ddCredentialError()
	}
	var flag, typ int
	var cipher []byte
	if tx.QueryRowContext(ctx, "SELECT flag,cred_type,cred FROM account WHERE type=? AND name=?", m, name).Scan(&flag, &typ, &cipher) != nil {
		return out, ddCredentialError()
	}
	if !((m == 1 && flag == 0 && typ == 1) || (m == 2 && (flag == 0 || flag == 1 || flag == 4) && typ == 2)) || len(cipher) == 0 || len(cipher) > 131072 {
		return out, ddCredentialError()
	}
	if tx.Commit() != nil {
		return out, ddCredentialError()
	}
	if err = ctx.Err(); err != nil {
		return out, err
	}
	plain, err := unprotect(cipher, ddEntropy)
	if err != nil {
		return out, ddCredentialError()
	}
	defer clear(plain)
	if !utf8.Valid(plain) || len(plain) > 65536 || strings.TrimSpace(string(plain)) == "" || strings.ContainsRune(string(plain), 0) {
		return out, ddCredentialError()
	}
	return ddCredential{method: int(m), account: name, token: string(plain)}, nil
}
func ddIdentityText(value any) (string, error) {
	switch v := value.(type) {
	case string:
		if strings.TrimSpace(v) != "" && len(v) <= 4096 && !strings.ContainsRune(v, 0) {
			return v, nil
		}
	case json.Number:
		if ddDecimal(string(v)) {
			return string(v), nil
		}
	}
	return "", fmt.Errorf("invalid DD account identity")
}
