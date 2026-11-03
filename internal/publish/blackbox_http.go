package publish

import (
	"bytes"
	"context"
	"crypto/md5"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"strconv"
	"strings"
	"time"
)

const bbAlphabet = "AB45STUVWZEFGJ6CH01D237IXYPQRKLMN89"
const bbLogin = "https://login.xiaoheihe.cn/?origin=heybox_open&redirect_url=https%3A%2F%2Fopen.xiaoheihe.cn%2Fzh_cn%2Fworkshop"
const bbUA = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/142.0.0.0 Safari/537.36"

func bbMap(s string, limit int) string {
	table := bbAlphabet
	if limit < 0 {
		table = table[:len(table)+limit]
	}
	var b strings.Builder
	for _, r := range s {
		b.WriteByte(table[int(r)%len(table)])
	}
	return b.String()
}
func bbSign(path string, stamp int64, nonce string) string {
	path = "/" + strings.Join(strings.FieldsFunc(path, func(r rune) bool { return r == '/' }), "/") + "/"
	parts := []string{bbMap(strconv.FormatInt(stamp+1, 10), -2), bbMap(path, 0), bbMap(nonce, 0)}
	var b strings.Builder
	for i := 0; b.Len() < 20; i++ {
		done := true
		for _, p := range parts {
			if i < len(p) {
				b.WriteByte(p[i])
				done = false
			}
		}
		if done {
			break
		}
	}
	source := b.String()
	if len(source) > 20 {
		source = source[:20]
	}
	hash := md5.Sum([]byte(source))
	digest := hex.EncodeToString(hash[:])
	values := []byte(digest[len(digest)-6:])
	p := func(x byte) byte {
		if x&128 != 0 {
			return x<<1 ^ 27
		}
		return x << 1
	}
	hm := func(x byte) byte { return p(x) ^ x }
	qg := func(x byte) byte { return hm(p(x)) }
	dx := func(x byte) byte { return qg(hm(p(x))) }
	mw := func(x byte) byte { return dx(x) ^ qg(x) ^ hm(x) }
	a, c, d, e := values[0], values[1], values[2], values[3]
	sum := int(mw(a)^dx(c)^qg(d)^hm(e)) + int(hm(a)^mw(c)^dx(d)^qg(e)) + int(qg(a)^hm(c)^mw(d)^dx(e)) + int(dx(a)^qg(c)^hm(d)^mw(e)) + int(values[4]) + int(values[5])
	return bbMap(digest[:5], -4) + fmt.Sprintf("%02d", sum%100)
}

var bbRoutes = map[string]bool{}

func init() {
	for _, p := range []string{"/wow/v2/dynamic_conf/", "/wow/open_platform/module/list/", "/wow/open_platform/module/detail/", "/wow/open_platform/module_version/list/"} {
		bbRoutes["GET "+p] = true
	}
	for _, p := range []string{"/wow/open_platform/module/update/", "/wow/open_platform/module_version/upsert/", "/wow/open_platform/module_version/delete/", "/wow/cos/upload/token/", "/wow/get_user_configuration_backup_list/", "/wow/remove_user_configuration_backup/", "/wow/rename_user_configuration_backup/", "/wow/v2/share/list/", "/wow/v2/share/detail/", "/wow/create_config_share/", "/wow/update_config_share/", "/wow/create_backup/", "/wow/get_user_backup_credential/", "/wow/save_user_configuration/", "/wow/v2/get_user_info/", "/wow/get_wa_list/", "/wow/get_wa_detail/", "/wow/get_wa_string/", "/wow/get_wa_tag_list/", "/wow/create_wa/", "/wow/update_wa/"} {
		bbRoutes["POST "+p] = true
	}
}

var bbAuthRoutes = map[string]bool{"GET /account/restore_login": true, "POST /account/get_login_code/": true, "POST /account/login_code/": true, "GET /account/wechat/state/": true}

type bbCookie struct {
	Name    string `json:"name"`
	Value   string `json:"value"`
	Domain  string `json:"domain"`
	Path    string `json:"path"`
	Secure  bool   `json:"secure"`
	Expires int64  `json:"expires,omitempty"`
}
type bbSession struct {
	Schema     string     `json:"schema"`
	Identity   string     `json:"identity,omitempty"`
	Method     string     `json:"method"`
	Device     string     `json:"device_id"`
	WebVersion string     `json:"web_version"`
	Expires    int64      `json:"expire_at,omitempty"`
	Cookies    []bbCookie `json:"cookies"`
}
type bbClient struct {
	http          *http.Client
	api, workshop string
	now           func() time.Time
	session       *bbSession
	home          string
}

func newBB(home string) *bbClient {
	transport := http.DefaultTransport.(*http.Transport).Clone()
	transport.Proxy = nil
	return &bbClient{http: &http.Client{Transport: transport, Timeout: 35 * time.Second, CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }}, api: "https://api.xiaoheihe.cn", workshop: "https://workshopapi.xiaoheihe.cn", now: time.Now, home: home}
}
func bbID(v any) string {
	switch x := v.(type) {
	case string:
		return x
	case json.Number:
		return string(x)
	case int64:
		return strconv.FormatInt(x, 10)
	case int:
		return strconv.Itoa(x)
	}
	return ""
}
func bbObject(v any) (map[string]any, error) {
	o, ok := v.(map[string]any)
	if !ok {
		return nil, failure("publish.blackbox_shape", "platform response must contain an object")
	}
	return o, nil
}
func bbStringID(v any) bool { s := bbID(v); return s != "" && !strings.ContainsAny(s, "\r\n\x00") }
func bbCookieDomain(domain string) bool {
	switch strings.TrimPrefix(domain, ".") {
	case "xiaoheihe.cn", "api.xiaoheihe.cn", "login.xiaoheihe.cn", "open.xiaoheihe.cn", "workshopapi.xiaoheihe.cn", "weixin.qq.com", "open.weixin.qq.com", "long.open.weixin.qq.com":
		return true
	}
	return false
}
func bbValidCookie(c bbCookie) bool {
	return c.Name != "" && !strings.ContainsAny(c.Name, " =;\r\n\x00") && !strings.ContainsAny(c.Value, ";\r\n\x00") && bbCookieDomain(c.Domain) && strings.HasPrefix(c.Path, "/") && !strings.ContainsAny(c.Path, "\r\n\x00")
}
func bbIdentity(s *bbSession) (string, error) {
	uids, keys := map[string]bool{}, map[string]bool{}
	for _, c := range s.Cookies {
		if !bbValidCookie(c) {
			return "", failure("publish.blackbox_session", "invalid stored cookie")
		}
		switch c.Name {
		case "heybox_id", "user_heybox_id", "pkey", "user_pkey":
			if strings.TrimPrefix(c.Domain, ".") != "xiaoheihe.cn" || c.Path != "/" {
				return "", failure("publish.blackbox_session", "identity cookie has invalid scope")
			}
			if c.Name == "heybox_id" || c.Name == "user_heybox_id" {
				uids[c.Value] = true
			} else {
				keys[c.Value] = true
			}
		}
	}
	if len(uids) != 1 || len(keys) != 1 || keys[""] {
		return "", capability("publish.blackbox_login", "explicit Heybox login is required")
	}
	for uid := range uids {
		if uid == "" || strings.Trim(uid, "0123456789") != "" || len(uid) > 32 || (s.Identity != "" && s.Identity != uid) {
			return "", failure("publish.blackbox_identity", "stored session identity differs from its cookies")
		}
		return uid, nil
	}
	return "", capability("publish.blackbox_login", "explicit Heybox login is required")
}
func bbApply(s *bbSession, result map[string]any, now time.Time, expected string) error {
	profile, _ := result["profile"].(map[string]any)
	detail, _ := result["account_detail"].(map[string]any)
	uid := bbID(profile["heybox_id"])
	if uid == "" {
		uid = bbID(detail["userid"])
	}
	key := String(result, "pkey")
	expiry := Integer(result, "expire_at")
	if uid == "" || strings.Trim(uid, "0123456789") != "" || len(uid) > 32 || key == "" || strings.ContainsAny(key, ";\r\n\x00") || expiry <= now.Unix() || expiry > now.Add(366*24*time.Hour).Unix() {
		return failure("publish.blackbox_identity", "server did not return a complete identity and expiry")
	}
	if expected != "" && uid != expected {
		return failure("publish.blackbox_identity", "restored account differs from original account")
	}
	out := []bbCookie{}
	for _, c := range s.Cookies {
		if c.Name != "heybox_id" && c.Name != "user_heybox_id" && c.Name != "pkey" && c.Name != "user_pkey" {
			out = append(out, c)
		}
	}
	for n, v := range map[string]string{"heybox_id": uid, "user_heybox_id": uid, "pkey": key, "user_pkey": key} {
		out = append(out, bbCookie{n, v, ".xiaoheihe.cn", "/", true, expiry})
	}
	s.Cookies = out
	s.Identity = uid
	s.Expires = expiry
	s.Schema = "lycheedev.blackbox.http-session.v1"
	return nil
}
func (c *bbClient) send(ctx context.Context, s *bbSession, method, endpoint string, body io.Reader) (int, http.Header, []byte, error) {
	req, err := http.NewRequestWithContext(ctx, method, endpoint, body)
	if err != nil {
		return 0, nil, nil, invalid("$endpoint", "invalid platform endpoint")
	}
	req.Header.Set("User-Agent", bbUA)
	req.Header.Set("Origin", "https://login.xiaoheihe.cn")
	req.Header.Set("Referer", bbLogin)
	if body != nil {
		req.Header.Set("Content-Type", "application/x-www-form-urlencoded;charset=UTF-8")
	}
	for _, cookie := range s.Cookies {
		if !bbValidCookie(cookie) {
			return 0, nil, nil, failure("publish.blackbox_session", "invalid stored cookie")
		}
		domain := strings.TrimPrefix(cookie.Domain, ".")
		host := req.URL.Hostname()
		match := host == domain || (strings.HasPrefix(cookie.Domain, ".") && strings.HasSuffix(host, "."+domain))
		pathMatch := req.URL.Path == cookie.Path || strings.HasPrefix(req.URL.Path, strings.TrimSuffix(cookie.Path, "/")+"/")
		if match && pathMatch && (!cookie.Secure || req.URL.Scheme == "https") && (cookie.Expires == 0 || cookie.Expires > c.now().Unix()) {
			req.AddCookie(&http.Cookie{Name: cookie.Name, Value: cookie.Value})
		}
	}
	res, err := c.http.Do(req)
	if err != nil {
		if ctx.Err() != nil {
			return 0, nil, nil, ctx.Err()
		}
		return 0, nil, nil, failure("publish.blackbox_transport", "platform request was interrupted; it was not retried")
	}
	defer res.Body.Close()
	for _, cookie := range res.Cookies() {
		domain := cookie.Domain
		if domain == "" {
			domain = req.URL.Hostname()
		}
		cookieHost := strings.TrimPrefix(domain, ".")
		if req.URL.Hostname() != cookieHost && !strings.HasSuffix(req.URL.Hostname(), "."+cookieHost) {
			return 0, nil, nil, failure("publish.blackbox_cookie", "response cookie belongs to another origin")
		}
		p := cookie.Path
		if p == "" {
			p = "/"
		}
		row := bbCookie{cookie.Name, cookie.Value, domain, p, cookie.Secure, 0}
		if !cookie.Expires.IsZero() {
			row.Expires = cookie.Expires.Unix()
		}
		if !bbValidCookie(row) {
			return 0, nil, nil, failure("publish.blackbox_cookie", "platform returned an invalid cookie scope")
		}
		if cookie.MaxAge > 0 {
			row.Expires = c.now().Add(time.Duration(cookie.MaxAge) * time.Second).Unix()
		}
		out := s.Cookies[:0]
		for _, old := range s.Cookies {
			if old.Name != row.Name || old.Domain != row.Domain || old.Path != row.Path {
				out = append(out, old)
			}
		}
		s.Cookies = out
		if cookie.MaxAge >= 0 && (row.Expires == 0 || row.Expires > c.now().Unix()) {
			s.Cookies = append(s.Cookies, row)
		}
		if len(s.Cookies) > 256 {
			return 0, nil, nil, failure("publish.blackbox_cookie", "too many cookies")
		}
	}
	raw, err := io.ReadAll(io.LimitReader(res.Body, (32<<20)+1))
	if err != nil || len(raw) > 32<<20 {
		return 0, nil, nil, failure("publish.blackbox_response", "platform response is incomplete or too large")
	}
	return res.StatusCode, res.Header, raw, nil
}
func bbForm(values map[string]any) url.Values {
	q := url.Values{}
	for k, v := range values {
		if a, ok := v.([]any); ok {
			for _, x := range a {
				q.Add(k, bbWireText(x))
			}
		} else {
			q.Set(k, bbWireText(v))
		}
	}
	return q
}
func bbWireText(v any) string {
	switch x := v.(type) {
	case string:
		return x
	case json.Number:
		return string(x)
	case bool:
		if x {
			return "True"
		}
		return "False"
	case nil:
		return ""
	default:
		return fmt.Sprint(x)
	}
}
func (c *bbClient) raw(ctx context.Context, s *bbSession, method, route string, query, body map[string]any) (map[string]any, error) {
	origin := c.workshop
	if bbAuthRoutes[method+" "+route] {
		origin = c.api
	} else if !bbRoutes[method+" "+route] {
		return nil, invalid("$route", "route is outside the fixed Heybox API")
	}
	stamp := c.now().Unix()
	var random [16]byte
	if _, err := rand.Read(random[:]); err != nil {
		return nil, err
	}
	digest := md5.Sum(random[:])
	nonce := strings.ToUpper(hex.EncodeToString(digest[:]))
	params := url.Values{"app": {"heybox"}, "heybox_id": {s.Identity}, "os_type": {"web"}, "x_app": {"heybox_website"}, "x_client_type": {"weboutapp"}, "x_os_type": {"Windows"}, "web_version": {s.WebVersion}, "device_id": {s.Device}, "version": {"999.0.4"}, "hkey": {bbSign(route, stamp, nonce)}, "_time": {strconv.FormatInt(stamp, 10)}, "nonce": {nonce}}
	for k, v := range bbForm(query) {
		if _, reserved := params[k]; reserved && !(route == "/account/wechat/state/" && k == "heybox_id" && len(v) == 1 && v[0] == "") {
			return nil, invalid("$query", "reserved protocol query field")
		}
		params[k] = v
	}
	var input io.Reader
	if method == "POST" {
		input = strings.NewReader(bbForm(body).Encode())
	} else if len(body) > 0 {
		return nil, invalid("$body", "GET does not accept a body")
	}
	status, _, raw, err := c.send(ctx, s, method, origin+route+"?"+params.Encode(), input)
	if err != nil {
		return nil, err
	}
	if status != 200 {
		return nil, &Error{Code: "publish.blackbox_http", Message: "Heybox returned unsuccessful HTTP status", Exit: 5, Details: map[string]any{"httpStatus": status, "route": route}}
	}
	d := json.NewDecoder(bytes.NewReader(raw))
	d.UseNumber()
	var out map[string]any
	if d.Decode(&out) != nil || out == nil || d.Decode(new(any)) != io.EOF {
		return nil, failure("publish.blackbox_shape", "Heybox returned invalid JSON")
	}
	return out, nil
}
func (c *bbClient) renew(ctx context.Context) error {
	s, err := bbLoadSession(c.home)
	if err != nil {
		return err
	}
	uid, err := bbIdentity(s)
	if err != nil {
		return err
	}
	reply, err := c.raw(ctx, s, "GET", "/account/restore_login", nil, nil)
	if err != nil {
		return err
	}
	if reply["status"] != "ok" {
		return capability("publish.blackbox_login", "stored Heybox session needs explicit login")
	}
	data, err := bbObject(reply["result"])
	if err != nil {
		return err
	}
	if err = bbApply(s, data, c.now(), uid); err != nil {
		return err
	}
	if err = bbSaveSession(ctx, c.home, s); err != nil {
		return err
	}
	c.session = s
	return nil
}
func (c *bbClient) call(ctx context.Context, method, route string, query, body map[string]any) (map[string]any, error) {
	if c.session == nil {
		if err := c.renew(ctx); err != nil {
			return nil, err
		}
	}
	reply, err := c.raw(ctx, c.session, method, route, query, body)
	if err != nil {
		return nil, err
	}
	status, validStatus := reply["status"].(string)
	if !validStatus || status == "" {
		return nil, failure("publish.blackbox_shape", "Heybox response omitted a valid business status")
	}
	if status != "ok" {
		return nil, &Error{Code: "publish.blackbox_rejected", Message: "Heybox rejected the request; it was not retried", Exit: 5, Details: map[string]any{"route": route, "status": bbPublic(reply["status"])}}
	}
	if reply["result"] == nil {
		return map[string]any{}, nil
	}
	return bbObject(reply["result"])
}
