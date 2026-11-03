package publish

import (
	"context"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"errors"
	"net/http"
	"net/url"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/follenfang/lycheedev/internal/vault"
)

type newBeeCreator struct {
	http                         *HTTP
	api, next, metadata, authDir string
	headers                      http.Header
	access                       string
}

func newNewBeeCreator() (*newBeeCreator, error) {
	roaming, err := newBeeRoaming()
	if err != nil {
		return nil, err
	}
	return &newBeeCreator{http: NewHTTP(), api: "https://api.newbeebox.com", next: "https://api.next.newbeebox.com", metadata: "https://cdn2.newbeebox.com/modconfig.json", authDir: filepath.Join(roaming, "NewBeeBox", "auth-store")}, nil
}
func newBeeFresh(token string) bool {
	parts := strings.Split(token, ".")
	if len(parts) != 3 {
		return false
	}
	raw, err := base64.RawURLEncoding.DecodeString(parts[1])
	if err != nil {
		return false
	}
	var claims struct {
		Exp int64 `json:"exp"`
	}
	return json.Unmarshal(raw, &claims) == nil && claims.Exp > time.Now().Unix()+30
}
func (c *newBeeCreator) desktop(ctx context.Context) error {
	if c.access != "" {
		return nil
	}
	read := func(name string) string {
		raw, _ := os.ReadFile(filepath.Join(c.authDir, name))
		return strings.TrimSpace(string(raw))
	}
	access := read("access-token")
	if access == "" {
		return capability("publish.newbee_auth", "sign in using the NewBeeBox desktop client")
	}
	if !newBeeFresh(access) {
		refresh := read("refresh-token")
		if refresh == "" {
			return capability("publish.newbee_auth", "desktop session expired; sign in using NewBeeBox")
		}
		host, _ := os.Hostname()
		form := url.Values{"client_id": {"nbb-desktop"}, "grant_type": {"refresh_token"}, "refresh_token": {refresh}, "device_name": {host}, "device_type": {"desktop"}}
		proof := read("device-proof")
		if proof != "" {
			form.Set("device_proof", proof)
		}
		value, err := c.http.request(ctx, http.MethodPost, c.next+"/auth/connect/token", nil, strings.NewReader(form.Encode()), "application/x-www-form-urlencoded")
		if err != nil {
			return err
		}
		response, ok := value.(map[string]any)
		if !ok || String(response, "access_token") == "" {
			return failure("publish.newbee_auth", "desktop refresh response omitted access token")
		}
		access = String(response, "access_token")
		for field, file := range map[string]string{"access_token": "access-token", "refresh_token": "refresh-token", "device_proof": "device-proof"} {
			if value := String(response, field); value != "" {
				if err = vault.ReplaceFile(ctx, filepath.Join(c.authDir, file), []byte(value)); err != nil {
					return err
				}
			}
		}
	}
	c.access = access
	return nil
}
func newBeeEnvelope(value map[string]any) (any, error) {
	code, ok := value["code"].(json.Number)
	if !ok {
		return nil, failure("publish.newbee_response", "NewBeeBox response omitted its numeric status")
	}
	if _, err := code.Int64(); err != nil {
		return nil, failure("publish.newbee_response", "NewBeeBox response has an invalid status")
	}
	if code.String() != "1" {
		return nil, failure("publish.newbee_business", "NewBeeBox rejected the operation")
	}
	data, ok := value["data"]
	if !ok {
		return nil, failure("publish.newbee_response", "NewBeeBox response omitted data")
	}
	return data, nil
}

func newBeeRejected(err error) bool {
	var platformError *Error
	if !errors.As(err, &platformError) {
		return false
	}
	if platformError.Code == "publish.newbee_business" {
		return true
	}
	if platformError.Code == "publish.http_status" {
		details, _ := platformError.Details.(map[string]any)
		status, _ := details["httpStatus"].(int)
		return status == http.StatusUnauthorized || status == http.StatusForbidden
	}
	return false
}
func (c *newBeeCreator) authenticate(ctx context.Context) error {
	if c.headers != nil {
		return nil
	}
	if err := c.desktop(ctx); err != nil {
		return err
	}
	call := func(path string, headers http.Header, body any) (map[string]any, error) {
		response, err := c.http.JSON(ctx, http.MethodPost, c.api+path, headers, body)
		if err != nil {
			return nil, err
		}
		data, err := newBeeEnvelope(response)
		if err != nil {
			return nil, err
		}
		object, ok := data.(map[string]any)
		if !ok {
			return nil, failure("publish.newbee_auth", "invalid Creator authentication response")
		}
		return object, nil
	}
	handoff, err := call("/v3/user/auth2web", http.Header{"Authorization": {"Bearer " + c.access}, "Boxversion": {"1.1.17"}, "Accept-Language": {"zh-CN"}}, map[string]any{})
	if err != nil {
		return err
	}
	if String(handoff, "code") == "" {
		return failure("publish.newbee_auth", "desktop handoff omitted code")
	}
	exchange, err := call("/v3/user/exchange_web_code", http.Header{"Appid": {"6"}, "Accept-Language": {"zh-CN"}}, map[string]any{"code": String(handoff, "code")})
	if err != nil {
		return err
	}
	if String(exchange, "token") == "" {
		return failure("publish.newbee_auth", "Creator exchange omitted author token")
	}
	headers := http.Header{"Appid": {"6"}, "Token": {String(exchange, "token")}, "Accept-Language": {"zh-CN"}}
	if jwt := String(exchange, "jwtToken"); jwt != "" {
		headers.Set("Authorization", "Bearer "+jwt)
	}
	resource, err := call("/v3/user/refresh_web_resource_token", headers, map[string]any{})
	if err != nil {
		return err
	}
	if String(resource, "resource_token") == "" {
		return failure("publish.newbee_auth", "Creator exchange omitted resource token")
	}
	headers.Set("Authorization", "Bearer "+String(resource, "resource_token"))
	c.headers = headers
	return nil
}
func (c *newBeeCreator) post(ctx context.Context, path string, body any) (any, error) {
	if err := c.authenticate(ctx); err != nil {
		return nil, err
	}
	origin := c.api
	if strings.HasPrefix(path, "/cloudsaveserver/") || strings.HasPrefix(path, "/bannerserver/") {
		origin = c.next
	}
	value, err := c.http.JSON(ctx, http.MethodPost, origin+path, c.headers, body)
	if err != nil {
		return nil, err
	}
	return newBeeEnvelope(value)
}
func (c *newBeeCreator) backupPost(ctx context.Context, path string, body any) (any, error) {
	if err := c.desktop(ctx); err != nil {
		return nil, err
	}
	value, err := c.http.JSON(ctx, http.MethodPost, c.api+path, http.Header{"Authorization": {"Bearer " + c.access}, "Boxversion": {"1.1.17"}, "Accept-Language": {"zh-CN"}}, body)
	if err != nil {
		return nil, err
	}
	return newBeeEnvelope(value)
}
func (c *newBeeCreator) upload(ctx context.Context, path, file string, fields map[string]string) (any, error) {
	if err := c.authenticate(ctx); err != nil {
		return nil, err
	}
	f, err := openArtifact(file)
	if err != nil {
		return nil, invalid("$.file", "cannot read selected file")
	}
	defer f.Close()
	value, err := c.http.Multipart(ctx, c.api+path, c.headers, fields, "file", f)
	if err != nil {
		return nil, err
	}
	return newBeeEnvelope(value)
}
func newBeeObject(value any) map[string]any {
	if rows, ok := value.([]any); ok && len(rows) > 0 {
		value = rows[0]
	}
	v, _ := value.(map[string]any)
	return v
}
func newBeePick(value map[string]any, keys ...string) any {
	for _, key := range keys {
		if v, ok := value[key]; ok {
			return v
		}
	}
	return nil
}
func newBeeDecoded(value any) any {
	if text, ok := value.(string); ok {
		var decoded any
		d := json.NewDecoder(strings.NewReader(text))
		d.UseNumber()
		if d.Decode(&decoded) == nil {
			return decoded
		}
	}
	return value
}
func newBeeRows(value any) []any {
	value = newBeeDecoded(value)
	if rows, ok := value.([]any); ok {
		return rows
	}
	if obj, ok := value.(map[string]any); ok {
		for _, key := range []string{"list", "items", "data"} {
			if rows, ok := newBeeDecoded(obj[key]).([]any); ok {
				return rows
			}
		}
	}
	return nil
}

// Bind recovery to the authenticated desktop session. The pinned vendor contract
// exposes tokens but no stable account identifier; renewal therefore requires
// reconciling the original operation instead of continuing under a new token.
func (c *newBeeCreator) sessionIdentity(ctx context.Context) (string, error) {
	if err := c.desktop(ctx); err != nil {
		return "", err
	}
	sum := sha256.Sum256([]byte(c.access))
	return hex.EncodeToString(sum[:]), nil
}
func newBeeCheckSession(stored, current string) error {
	if stored == "" || stored != current {
		return capability("publish.newbee_session_changed", "original desktop session differs; retain the original request and reconcile its resource before further writes")
	}
	return nil
}
