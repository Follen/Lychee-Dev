package publish

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"io"
	"os"
	"path/filepath"
	"strings"

	"github.com/follenfang/lycheedev/internal/vault"
)

func bbStorePath(home, name string) string {
	return filepath.Join(home, "publish", "blackbox", name+".bin")
}
func bbSave(ctx context.Context, home, name string, v any) error {
	raw, err := json.Marshal(v)
	if err != nil {
		return err
	}
	encrypted, err := bbProtect(raw, false)
	if err != nil {
		return err
	}
	return vault.ReplaceFile(ctx, bbStorePath(home, name), encrypted)
}
func bbLoad(home, name string, v any) error {
	f, err := os.Open(bbStorePath(home, name))
	if err != nil {
		return err
	}
	defer f.Close()
	encrypted, err := io.ReadAll(io.LimitReader(f, (8<<20)+1))
	if err != nil || len(encrypted) > 8<<20 {
		return failure("publish.blackbox_storage", "invalid encrypted record")
	}
	raw, err := bbProtect(encrypted, true)
	if err != nil {
		return err
	}
	d := json.NewDecoder(strings.NewReader(string(raw)))
	d.UseNumber()
	d.DisallowUnknownFields()
	if d.Decode(v) != nil {
		return failure("publish.blackbox_storage", "invalid encrypted record contents")
	}
	if d.Decode(new(any)) != io.EOF {
		return failure("publish.blackbox_storage", "trailing encrypted record contents")
	}
	return nil
}
func bbSaveSession(ctx context.Context, home string, s *bbSession) error {
	if s.Method != "sms" && s.Method != "wechat" {
		return failure("publish.blackbox_session", "invalid session method")
	}
	if _, err := bbIdentity(s); err != nil {
		return err
	}
	if err := bbSave(ctx, home, "session-"+s.Method, s); err != nil {
		return err
	}
	return bbSave(ctx, home, "active", map[string]string{"method": s.Method})
}
func bbLoadSession(home string) (*bbSession, error) {
	var active struct {
		Method string `json:"method"`
	}
	if err := bbLoad(home, "active", &active); err != nil {
		if errors.Is(err, os.ErrNotExist) {
			return nil, capability("publish.blackbox_login", "explicit Heybox login is required")
		}
		return nil, err
	}
	if active.Method != "sms" && active.Method != "wechat" {
		return nil, failure("publish.blackbox_session", "invalid active session source")
	}
	var s bbSession
	if err := bbLoad(home, "session-"+active.Method, &s); err != nil {
		return nil, err
	}
	if s.Schema != "lycheedev.blackbox.http-session.v1" || s.Method != active.Method {
		return nil, failure("publish.blackbox_session", "stored session source differs from active selection")
	}
	if _, err := bbIdentity(&s); err != nil {
		return nil, err
	}
	return &s, nil
}
func bbPublic(v any) any {
	switch x := v.(type) {
	case []map[string]any:
		r := make([]any, len(x))
		for i, v := range x {
			r[i] = bbPublic(v)
		}
		return r
	case map[string]any:
		r := map[string]any{}
		for k, v := range x {
			n := strings.ToLower(k)
			secret := n == "code" || n == "phone" || n == "phone_num"
			for _, marker := range []string{"token", "secret", "cookie", "nonce", "hkey", "pkey", "credential", "signature", "authorization", "authentication", "password", "device_id", "signed_url", "upload_url", "presigned"} {
				secret = secret || strings.Contains(n, marker)
			}
			if secret {
				r[k] = "[REDACTED]"
				continue
			}
			switch n {
			case "encoded", "article", "updatelog", "descriptionheybox", "desc", "description", "content", "configuration":
				raw, _ := json.Marshal(v)
				sum := sha256.Sum256(raw)
				r[k] = map[string]any{"redacted": true, "sha256": hex.EncodeToString(sum[:]), "bytes": len(raw)}
			default:
				r[k] = bbPublic(v)
			}
		}
		return r
	case []any:
		r := make([]any, len(x))
		for i, v := range x {
			r[i] = bbPublic(v)
		}
		return r
	case string:
		if strings.Contains(x, "!WA:") {
			sum := sha256.Sum256([]byte(x))
			return map[string]any{"redacted": true, "sha256": hex.EncodeToString(sum[:]), "bytes": len(x)}
		}
		if strings.HasPrefix(x, "http") && strings.Contains(x, "?") {
			sum := sha256.Sum256([]byte(x))
			return map[string]any{"urlRedacted": true, "sha256": hex.EncodeToString(sum[:])}
		}
		return x
	default:
		return v
	}
}
func bbRemove(home, name string) error {
	err := os.Remove(bbStorePath(home, name))
	if errors.Is(err, os.ErrNotExist) {
		return nil
	}
	return err
}
