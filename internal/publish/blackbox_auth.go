package publish

import (
	"context"
	"crypto/rand"
	"crypto/rsa"
	"crypto/x509"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"errors"
	"html"
	"net/url"
	"os"
	"path/filepath"
	"regexp"
	"strconv"
	"strings"

	"github.com/follenfang/lycheedev/internal/vault"
	htmlparse "golang.org/x/net/html"
)

type bbPending struct {
	Schema                             string `json:"schema"`
	ID                                 string `json:"login_id"`
	Status                             string `json:"status"`
	Created, Expires                   int64
	Session                            bbSession  `json:"session"`
	Phone                              string     `json:"phone_encrypted,omitempty"`
	Submitted                          bool       `json:"submitted"`
	Candidate                          *bbSession `json:"candidate,omitempty"`
	UUID, Callback, State, Next, Image string
	Last                               int
	Confirmed                          bool
	HopUncertain                       bool `json:"hopUncertain"`
}

var bbLoginID = regexp.MustCompile(`^[a-f0-9]{32}$`)
var bbPhone = regexp.MustCompile(`^1[3-9][0-9]{9}$`)
var bbOTP = regexp.MustCompile(`^[0-9]{4,8}$`)

func bbEncryptPhone(phone string) (string, error) {
	der, _ := base64.StdEncoding.DecodeString("MIGfMA0GCSqGSIb3DQEBAQUAA4GNADCBiQKBgQDZgjVwAiKTjZ55nG+mW6r3TSU4ECvNYqDMIS/bhCj2QaH5GI/KZb2TBp+CBvUj9SLFnmJQ0kzHzHoGZCQ88VevCffF7JePGF9cmKQqotlfTKbV4oxV5iLz7JSG6b/Vg7AXtrTolNtWsa8HiB0tI0YClYaQlOXm4UxLeSxQwSFETwIDAQAB")
	key, err := x509.ParsePKIXPublicKey(der)
	if err != nil {
		return "", err
	}
	encrypted, err := rsa.EncryptPKCS1v15(rand.Reader, key.(*rsa.PublicKey), []byte("+86"+phone))
	if err != nil {
		return "", err
	}
	return base64.StdEncoding.EncodeToString(encrypted), nil
}
func bbRandomID() (string, error) {
	var b [16]byte
	if _, err := rand.Read(b[:]); err != nil {
		return "", err
	}
	return hex.EncodeToString(b[:]), nil
}
func bbTerminal(status string) bool {
	switch status {
	case "expired", "cancelled", "failed", "captcha_required", "device_verification_required", "verification_required":
		return true
	}
	return false
}
func bbLoginFailure(v map[string]any) string {
	switch v["status"] {
	case "show_captcha":
		return "captcha_required"
	case "lack_token":
		return "device_verification_required"
	case "name_verify", "need_alipay_verify", "need_bind_phone", "need_phone_code":
		return "verification_required"
	}
	return "failed"
}
func (c *bbClient) pendingResult(p bbPending) map[string]any {
	remaining := p.Expires - c.now().Unix()
	if remaining < 0 {
		remaining = 0
	}
	r := map[string]any{"login_id": p.ID, "method": p.Session.Method, "status": p.Status, "authenticated": false, "expires_in": remaining, "automatic_retry": false}
	if p.Image != "" && !bbTerminal(p.Status) {
		r["image_file"] = filepath.Join(c.home, "publish", "blackbox", "qr", p.ID+p.Image)
	}
	return r
}
func bbDeleteQR(home, id string) error {
	if !bbLoginID.MatchString(id) {
		return invalid("$.login_id", "invalid original login ID")
	}
	for _, suffix := range []string{".png", ".jpg"} {
		p := filepath.Join(home, "publish", "blackbox", "qr", id+suffix)
		if err := os.Remove(p); err != nil && !errors.Is(err, os.ErrNotExist) {
			return err
		}
	}
	return nil
}
func (c *bbClient) loadPending(id string) (*bbPending, error) {
	if !bbLoginID.MatchString(id) {
		return nil, invalid("$.login_id", "use the opaque ID returned by login-start")
	}
	var p bbPending
	if err := bbLoad(c.home, "pending", &p); err != nil {
		if errors.Is(err, os.ErrNotExist) {
			return nil, nil
		}
		return nil, err
	}
	if p.Schema != "lycheedev.blackbox.pending-login.v1" || p.ID != id || p.Expires-p.Created != 300 {
		return nil, failure("publish.blackbox_login_identity", "pending login identity is invalid")
	}
	if p.Expires <= c.now().Unix() {
		if err := bbRemove(c.home, "pending"); err != nil {
			return nil, err
		}
		return nil, bbDeleteQR(c.home, id)
	}
	return &p, nil
}
func bbOAuthAllowed(endpoint string) bool {
	u, err := url.Parse(endpoint)
	if err != nil || u.Scheme != "https" || u.User != nil || u.Fragment != "" || (u.Port() != "" && u.Port() != "443") {
		return false
	}
	switch u.Hostname() {
	case "api.xiaoheihe.cn":
		return u.Path == "/account/wechat/login/v2/web_sso/" || u.Path == "/account/wechat/login_redirect/v2/web_sso/"
	case "login.xiaoheihe.cn":
		return u.Path == "/"
	case "open.xiaoheihe.cn":
		return u.Path == "/zh_cn/workshop"
	case "open.weixin.qq.com":
		return u.Path == "/connect/qrconnect" || regexp.MustCompile(`^/connect/qrcode/[A-Za-z0-9_-]{1,200}$`).MatchString(u.Path)
	case "long.open.weixin.qq.com":
		return u.Path == "/connect/l/qrconnect"
	}
	return false
}
func (c *bbClient) fetch(ctx context.Context, s *bbSession, endpoint string) (int, map[string][]string, []byte, error) {
	if !bbOAuthAllowed(endpoint) {
		return 0, nil, nil, failure("publish.blackbox_oauth", "OAuth endpoint is outside fixed official routes")
	}
	status, headers, raw, err := c.send(ctx, s, "GET", endpoint, nil)
	return status, headers, raw, err
}
func bbValidateAuthInput(action string, in map[string]any) error {
	switch action {
	case "session forget":
		if !Bool(in, "confirm_forget") {
			return invalid("$.confirm_forget", "must be true")
		}
	case "session login-start":
		if String(in, "method") == "sms" && !bbPhone.MatchString(String(in, "phone")) {
			return invalid("$.phone", "expected mainland China mobile number")
		}
		if String(in, "method") == "wechat" {
			if _, present := in["phone"]; present {
				return invalid("$.phone", "WeChat login does not accept phone")
			}
		}
	case "session login-submit", "session login-status":
		if !bbLoginID.MatchString(String(in, "login_id")) {
			return invalid("$.login_id", "use the opaque ID returned by login-start")
		}
		if action == "session login-submit" && !bbOTP.MatchString(String(in, "code")) {
			return invalid("$.code", "expected a 4 to 8 digit SMS code")
		}
	}
	return nil
}

func (c *bbClient) auth(ctx context.Context, action string, in map[string]any) (Result, error) {
	result := Result{Platform: "blackbox", Action: action}
	if err := bbValidateAuthInput(action, in); err != nil {
		return result, err
	}
	lease, err := vault.TryAcquireLease(ctx, filepath.Join(c.home, "publish", "blackbox", "locks"), "authentication")
	if err != nil {
		return result, err
	}
	defer lease.Close()
	if action == "session doctor" {
		s, err := bbLoadSession(c.home)
		if err != nil {
			var e *Error
			if errors.As(err, &e) && e.Code == "publish.blackbox_login" {
				result.Complete = true
				result.Data = map[string]any{"authenticated": false, "loginRequired": true}
				return result, nil
			}
			return result, err
		}
		result.Complete = true
		result.Data = map[string]any{"method": s.Method, "identity": s.Identity, "expires_at": s.Expires, "apiChecked": false}
		return result, nil
	}
	if action == "session renew" {
		if err = c.renew(ctx); err != nil {
			return result, err
		}
		result.Complete = true
		result.Data = map[string]any{"authenticated": true, "identity": c.session.Identity, "method": c.session.Method, "expires_at": c.session.Expires}
		return result, nil
	}
	if action == "session forget" {
		var p bbPending
		if bbLoad(c.home, "pending", &p) == nil {
			if err = bbDeleteQR(c.home, p.ID); err != nil {
				return result, err
			}
		}
		var active struct {
			Method string `json:"method"`
		}
		if bbLoad(c.home, "active", &active) == nil && (active.Method == "sms" || active.Method == "wechat") {
			if err = bbRemove(c.home, "session-"+active.Method); err != nil {
				return result, err
			}
		}
		if err = bbRemove(c.home, "active"); err != nil {
			return result, err
		}
		if err = bbRemove(c.home, "pending"); err != nil {
			return result, err
		}
		result.Complete = true
		result.Data = map[string]any{"forgotten": true, "remote_logout": false}
		return result, nil
	}
	var p *bbPending
	if action == "session login-start" {
		var old bbPending
		if err = bbLoad(c.home, "pending", &old); err == nil && old.Expires > c.now().Unix() && !bbTerminal(old.Status) {
			return result, &Error{Code: "publish.blackbox_login_pending", Message: "continue the retained login instead of starting another", Exit: 6, Details: map[string]any{"login_id": old.ID}}
		} else if err != nil && !errors.Is(err, os.ErrNotExist) {
			return result, err
		}
		method, phone := String(in, "method"), String(in, "phone")
		id, err := bbRandomID()
		if err != nil {
			return result, err
		}
		device, err := bbRandomID()
		if err != nil {
			return result, err
		}
		p = &bbPending{Schema: "lycheedev.blackbox.pending-login.v1", ID: id, Status: "starting", Created: c.now().Unix(), Expires: c.now().Unix() + 300, Session: bbSession{Method: method, Device: device, Cookies: []bbCookie{}}}
		if err = bbSave(ctx, c.home, "pending", p); err != nil {
			return result, err
		}
		status, _, raw, err := c.fetch(ctx, &p.Session, bbLogin)
		if err != nil || status != 200 {
			p.Status = "starting_uncertain"
			bbSave(ctx, c.home, "pending", p)
			if err == nil {
				err = failure("publish.blackbox_login", "login initialization failed")
			}
			return result, err
		}
		doc, _ := htmlparse.Parse(strings.NewReader(string(raw)))
		var visit func(*htmlparse.Node)
		visit = func(n *htmlparse.Node) {
			if n.Type == htmlparse.ElementNode && n.Data == "meta" {
				name, content := "", ""
				for _, a := range n.Attr {
					if a.Key == "name" {
						name = a.Val
					}
					if a.Key == "content" {
						content = a.Val
					}
				}
				if name == "version" {
					p.Session.WebVersion = content
				}
			}
			for ch := n.FirstChild; ch != nil; ch = ch.NextSibling {
				visit(ch)
			}
		}
		if doc != nil {
			visit(doc)
		}
		if method == "wechat" {
			err = c.wechatStart(ctx, p)
		} else {
			p.Phone, err = bbEncryptPhone(phone)
			if err == nil {
				p.Status = "sms_sending"
				err = bbSave(ctx, c.home, "pending", p)
			}
			if err == nil {
				var reply map[string]any
				reply, err = c.raw(ctx, &p.Session, "POST", "/account/get_login_code/", nil, map[string]any{"phone_num": p.Phone})
				if err == nil {
					p.Status = "waiting_code"
					if reply["status"] != "ok" {
						p.Status = bbLoginFailure(reply)
					}
				} else {
					p.Status = "sms_send_uncertain"
				}
			}
		}
		if saveErr := bbSave(ctx, c.home, "pending", p); saveErr != nil {
			return result, saveErr
		}
		result.Data = c.pendingResult(*p)
		return result, err
	}
	p, err = c.loadPending(String(in, "login_id"))
	if err != nil {
		return result, err
	}
	if p == nil {
		result.Complete = true
		result.Data = map[string]any{"status": "expired", "authenticated": false, "login_id": String(in, "login_id")}
		return result, nil
	}
	if action == "session login-submit" {
		if p.Session.Method != "sms" || p.Status != "waiting_code" || p.Submitted {
			return result, invalid("$.login_id", "original login is not waiting for an SMS code")
		}
		p.Submitted = true
		p.Status = "verification_required"
		if err = bbSave(ctx, c.home, "pending", p); err != nil {
			return result, err
		}
		reply, err := c.raw(ctx, &p.Session, "POST", "/account/login_code/", map[string]any{"code": String(in, "code")}, map[string]any{"phone_num": p.Phone})
		if err != nil {
			bbSave(ctx, c.home, "pending", p)
			result.Data = c.pendingResult(*p)
			return result, err
		}
		if reply["status"] != "ok" {
			p.Status = bbLoginFailure(reply)
			bbSave(ctx, c.home, "pending", p)
			result.Data = c.pendingResult(*p)
			return result, nil
		}
		data, _ := reply["result"].(map[string]any)
		return c.finishLogin(ctx, p, data)
	}
	if p.Candidate != nil {
		return c.finishLogin(ctx, p, nil)
	}
	if p.Session.Method == "sms" {
		if p.Submitted {
			return c.finishLogin(ctx, p, nil)
		}
		result.Data = c.pendingResult(*p)
		return result, nil
	}
	if bbTerminal(p.Status) {
		result.Data = c.pendingResult(*p)
		return result, nil
	}
	if p.Confirmed {
		return c.wechatFinish(ctx, p)
	}
	if !regexp.MustCompile(`^[A-Za-z0-9_-]{1,200}$`).MatchString(p.UUID) {
		return result, failure("publish.blackbox_oauth", "pending login lacks QR identity")
	}
	q := url.Values{"uuid": {p.UUID}, "_": {strconv.FormatInt(c.now().UnixMilli(), 10)}}
	if p.Last == 404 || p.Last == 408 {
		q.Set("last", strconv.Itoa(p.Last))
	}
	status, _, raw, err := c.fetch(ctx, &p.Session, "https://long.open.weixin.qq.com/connect/l/qrconnect?"+q.Encode())
	if err != nil {
		return result, err
	}
	match := regexp.MustCompile(`\bwx_errcode\s*=\s*(\d+)`).FindStringSubmatch(string(raw))
	if status != 200 || len(match) != 2 {
		return result, failure("publish.blackbox_oauth", "unrecognized QR polling response")
	}
	p.Last, _ = strconv.Atoi(match[1])
	switch p.Last {
	case 405:
		code := regexp.MustCompile(`\bwx_code\s*=\s*["']([A-Za-z0-9_-]{1,512})["']`).FindStringSubmatch(string(raw))
		if len(code) != 2 {
			return result, failure("publish.blackbox_oauth", "QR confirmation omitted OAuth code")
		}
		u, err := url.Parse(p.Callback)
		if err != nil || u.Host != "api.xiaoheihe.cn" || u.Path != "/account/wechat/login_redirect/v2/web_sso/" {
			return result, failure("publish.blackbox_oauth", "invalid retained callback")
		}
		q := u.Query()
		if q.Has("code") || q.Has("state") {
			return result, failure("publish.blackbox_oauth", "callback has conflicting fields")
		}
		q.Set("code", code[1])
		q.Set("state", p.State)
		u.RawQuery = q.Encode()
		p.Next = u.String()
		p.Confirmed = true
		p.Status = "waiting_confirmation"
		if err = bbSave(ctx, c.home, "pending", p); err != nil {
			return result, err
		}
		return c.wechatFinish(ctx, p)
	case 404:
		p.Status = "waiting_confirmation"
	case 408:
		if p.Status != "waiting_confirmation" {
			p.Status = "waiting_scan"
		}
	case 402:
		p.Status = "expired"
	case 403:
		p.Status = "cancelled"
	default:
		p.Status = "failed"
	}
	if err = bbSave(ctx, c.home, "pending", p); err != nil {
		return result, err
	}
	result.Data = c.pendingResult(*p)
	return result, nil
}
func (c *bbClient) wechatStart(ctx context.Context, p *bbPending) error {
	entry := "https://api.xiaoheihe.cn/account/wechat/login/v2/web_sso/?" + url.Values{"redirect_url": {bbLogin + "&src=1"}}.Encode()
	status, h, _, err := c.fetch(ctx, &p.Session, entry)
	if err != nil {
		return err
	}
	if status < 300 || status > 399 || len(h["Location"]) != 1 {
		return failure("publish.blackbox_oauth", "OAuth entry did not return a redirect")
	}
	u, err := url.Parse(h["Location"][0])
	if err != nil || u.Scheme != "https" || u.Host != "open.weixin.qq.com" || u.Path != "/connect/qrconnect" {
		return failure("publish.blackbox_oauth", "invalid OAuth provider")
	}
	u.Fragment = ""
	q := u.Query()
	if len(q["redirect_uri"]) != 1 || len(q["state"]) != 1 || q.Get("state") == "" || len(q.Get("state")) > 2048 {
		return failure("publish.blackbox_oauth", "incomplete OAuth state")
	}
	p.Callback, p.State = q.Get("redirect_uri"), q.Get("state")
	callback, err := url.Parse(p.Callback)
	if err != nil || callback.Scheme != "https" || callback.Host != "api.xiaoheihe.cn" || callback.Path != "/account/wechat/login_redirect/v2/web_sso/" || callback.Fragment != "" {
		return failure("publish.blackbox_oauth", "invalid OAuth callback")
	}
	status, _, raw, err := c.fetch(ctx, &p.Session, u.String())
	if err != nil {
		return err
	}
	if status != 200 {
		return failure("publish.blackbox_oauth", "QR page unavailable")
	}
	images := regexp.MustCompile(`<img[^>]+src=["']([^"']+)`).FindAllStringSubmatch(string(raw), -1)
	qr := ""
	for _, m := range images {
		v := html.UnescapeString(m[1])
		if regexp.MustCompile(`^/connect/qrcode/[A-Za-z0-9_-]{1,200}$`).MatchString(v) {
			qr = v
			break
		}
	}
	poll := regexp.MustCompile(`var\s+fordevtool\s*=\s*["'](https://long\.open\.weixin\.qq\.com/connect/l/qrconnect\?uuid=[A-Za-z0-9_-]{1,200})`).FindStringSubmatch(string(raw))
	if qr == "" || len(poll) != 2 {
		return failure("publish.blackbox_oauth", "QR page omitted session fields")
	}
	uuid := qr[strings.LastIndex(qr, "/")+1:]
	pu, _ := url.Parse(html.UnescapeString(poll[1]))
	if pu.Query().Get("uuid") != uuid {
		return failure("publish.blackbox_oauth", "QR and polling identities differ")
	}
	status, _, raw, err = c.fetch(ctx, &p.Session, "https://open.weixin.qq.com"+qr)
	if err != nil {
		return err
	}
	suffix := ""
	if strings.HasPrefix(string(raw), "\x89PNG\r\n\x1a\n") {
		suffix = ".png"
	} else if strings.HasPrefix(string(raw), "\xff\xd8\xff") {
		suffix = ".jpg"
	}
	if status != 200 || suffix == "" || len(raw) > 1<<20 {
		return failure("publish.blackbox_oauth", "invalid QR image")
	}
	dir := filepath.Join(c.home, "publish", "blackbox", "qr")
	if err = os.MkdirAll(dir, 0700); err != nil {
		return err
	}
	f, err := os.OpenFile(filepath.Join(dir, p.ID+suffix), os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0600)
	if err != nil {
		return err
	}
	_, err = f.Write(raw)
	closeErr := f.Close()
	if err != nil {
		return err
	}
	if closeErr != nil {
		return closeErr
	}
	p.UUID = uuid
	p.Image = suffix
	p.Status = "waiting_scan"
	return nil
}
func (c *bbClient) wechatFinish(ctx context.Context, p *bbPending) (Result, error) {
	result := Result{Platform: "blackbox", Action: "session login-status"}
	for i := 0; i < 4 && p.Next != "" && !p.HopUncertain; i++ {
		u, err := url.Parse(p.Next)
		if err != nil || !(u.Host == "api.xiaoheihe.cn" && u.Path == "/account/wechat/login_redirect/v2/web_sso/" || u.Host == "login.xiaoheihe.cn" && u.Path == "/" || u.Host == "open.xiaoheihe.cn" && u.Path == "/zh_cn/workshop") {
			return result, failure("publish.blackbox_oauth", "OAuth completion escaped fixed routes")
		}
		p.HopUncertain = true
		if err = bbSave(ctx, c.home, "pending", p); err != nil {
			return result, err
		}
		status, h, _, err := c.fetch(ctx, &p.Session, p.Next)
		if err != nil {
			return result, err
		}
		if status >= 300 && status < 400 && len(h["Location"]) == 1 {
			next, err := url.Parse(h["Location"][0])
			if err != nil {
				return result, err
			}
			p.Next = u.ResolveReference(next).String()
		} else if status == 200 {
			p.Next = ""
		} else {
			return result, failure("publish.blackbox_oauth", "OAuth completion returned unsuccessful status")
		}
		p.HopUncertain = false
		if err = bbSave(ctx, c.home, "pending", p); err != nil {
			return result, err
		}
	}
	if p.Next != "" && !p.HopUncertain {
		result.Data = c.pendingResult(*p)
		return result, nil
	}
	reply, err := c.raw(ctx, &p.Session, "GET", "/account/wechat/state/", map[string]any{"_notip": "true", "heybox_id": ""}, nil)
	if err != nil {
		return result, err
	}
	if reply["status"] != "ok" {
		p.Status = bbLoginFailure(reply)
		bbSave(ctx, c.home, "pending", p)
		result.Data = c.pendingResult(*p)
		return result, nil
	}
	data, _ := reply["result"].(map[string]any)
	return c.finishLogin(ctx, p, data)
}
func (c *bbClient) finishLogin(ctx context.Context, p *bbPending, data map[string]any) (Result, error) {
	result := Result{Platform: "blackbox", Action: "session login-status"}
	candidate := p.Candidate
	if candidate == nil {
		s := p.Session
		s.Cookies = append([]bbCookie(nil), p.Session.Cookies...)
		candidate = &s
	}
	expected := ""
	if data != nil && String(data, "pkey") != "" {
		copy := map[string]any{}
		for k, v := range data {
			copy[k] = v
		}
		if Integer(copy, "expire_at") == 0 {
			copy["expire_at"] = json.Number(strconv.FormatInt(c.now().Unix()+300, 10))
		}
		if err := bbApply(candidate, copy, c.now(), ""); err != nil {
			return result, err
		}
		expected = candidate.Identity
	} else {
		expected, _ = bbIdentity(candidate)
	}
	p.Candidate = candidate
	p.Status = "verifying"
	if err := bbSave(ctx, c.home, "pending", p); err != nil {
		return result, err
	}
	reply, err := c.raw(ctx, candidate, "GET", "/account/restore_login", nil, nil)
	if err != nil {
		return result, err
	}
	if reply["status"] != "ok" {
		return result, capability("publish.blackbox_login", "original login needs verification; no OTP or callback was replayed")
	}
	restored, err := bbObject(reply["result"])
	if err != nil {
		return result, err
	}
	if err = bbApply(candidate, restored, c.now(), expected); err != nil {
		return result, err
	}
	p.Candidate = candidate
	if err = bbSave(ctx, c.home, "pending", p); err != nil {
		return result, err
	}
	for _, check := range []struct{ method, path, field string }{{"GET", "/wow/open_platform/module/list/", "moduleList"}, {"POST", "/wow/get_user_configuration_backup_list/", "backupList"}} {
		reply, err = c.raw(ctx, candidate, check.method, check.path, nil, nil)
		if err != nil {
			return result, err
		}
		obj, err := bbObject(reply["result"])
		if err != nil || reply["status"] != "ok" {
			return result, capability("publish.blackbox_login", "Workshop account verification failed")
		}
		if _, ok := obj[check.field].([]any); !ok {
			return result, failure("publish.blackbox_shape", "Workshop verification list is incomplete")
		}
	}
	if c.now().Unix() >= p.Expires {
		return result, capability("publish.blackbox_login", "original login expired during verification")
	}
	if err = bbSaveSession(ctx, c.home, candidate); err != nil {
		return result, err
	}
	if err = bbDeleteQR(c.home, p.ID); err != nil {
		return result, err
	}
	if err = bbRemove(c.home, "pending"); err != nil {
		return result, err
	}
	result.Complete = true
	result.Data = map[string]any{"authenticated": true, "status": "authenticated", "identity": candidate.Identity, "method": candidate.Method, "login_id": p.ID, "expires_at": candidate.Expires}
	return result, nil
}
