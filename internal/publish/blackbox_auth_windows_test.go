//go:build windows

package publish

import (
	"context"
	"net/http"
	"strings"
	"testing"
	"time"
)

func TestBlackboxUncertainSMSDoesNotResend(t *testing.T) {
	ctx := context.Background()
	c := newBB(t.TempDir())
	c.now = func() time.Time { return time.Unix(1700000000, 0) }
	sent := 0
	c.http.Transport = bbTestTransport(func(r *http.Request) (*http.Response, error) {
		if r.URL.Host == "login.xiaoheihe.cn" {
			return bbTestResponse(200, `<html><meta name="version" content="fixture"></html>`), nil
		}
		if r.URL.Path == "/account/get_login_code/" {
			sent++
			r.ParseForm()
			if r.PostForm.Get("phone_num") == "" || strings.Contains(r.PostForm.Get("phone_num"), "13800000000") {
				t.Fatal("plaintext phone sent")
			}
			return bbTestResponse(503, `{}`), nil
		}
		t.Fatalf("unexpected request %s", r.URL.Path)
		return nil, nil
	})
	_, err := c.auth(ctx, "session login-start", map[string]any{"method": "sms", "phone": "13800000000"})
	if err == nil {
		t.Fatal("SMS uncertainty hidden")
	}
	var p bbPending
	if err = bbLoad(c.home, "pending", &p); err != nil {
		t.Fatal(err)
	}
	if p.Status != "sms_send_uncertain" {
		t.Fatal(p.Status)
	}
	_, err = c.auth(ctx, "session login-start", map[string]any{"method": "sms", "phone": "13800000000"})
	if err == nil || sent != 1 {
		t.Fatal("SMS was repeated")
	}
	if _, err = c.auth(ctx, "session login-status", map[string]any{"login_id": p.ID}); err != nil {
		t.Fatal(err)
	}
	if sent != 1 {
		t.Fatal("status resent SMS")
	}
}
func TestBlackboxOTPCheckpointAndReadOnlyRecovery(t *testing.T) {
	ctx := context.Background()
	c := newBB(t.TempDir())
	c.now = func() time.Time { return time.Unix(1700000000, 0) }
	id := "0123456789abcdef0123456789abcdef"
	p := bbPending{Schema: "lycheedev.blackbox.pending-login.v1", ID: id, Status: "waiting_code", Created: 1700000000, Expires: 1700000300, Phone: "ciphertext", Session: bbSession{Method: "sms", Device: "fixture"}}
	if err := bbSave(ctx, c.home, "pending", p); err != nil {
		t.Fatal(err)
	}
	submitted := 0
	c.http.Transport = bbTestTransport(func(r *http.Request) (*http.Response, error) {
		switch r.URL.Path {
		case "/account/login_code/":
			submitted++
			return bbTestResponse(503, `{}`), nil
		case "/account/restore_login":
			return bbTestResponse(200, `{"status":"ok","result":{"profile":{"heybox_id":"123"},"pkey":"private-fixture","expire_at":1700000600}}`), nil
		case "/wow/open_platform/module/list/":
			return bbTestResponse(200, `{"status":"ok","result":{"moduleList":[]}}`), nil
		case "/wow/get_user_configuration_backup_list/":
			return bbTestResponse(200, `{"status":"ok","result":{"backupList":[]}}`), nil
		}
		t.Fatalf("unexpected %s", r.URL.Path)
		return nil, nil
	})
	if _, err := c.auth(ctx, "session login-submit", map[string]any{"login_id": id, "code": "123456"}); err == nil {
		t.Fatal("OTP failure hidden")
	}
	result, err := c.auth(ctx, "session login-status", map[string]any{"login_id": id})
	if err != nil || !result.Complete || submitted != 1 {
		t.Fatalf("recovery failed: %v %+v", err, result)
	}
	s, err := bbLoadSession(c.home)
	if err != nil || s.Identity != "123" {
		t.Fatalf("identity not retained: %v", err)
	}
}

func TestBlackboxUncertainWeChatCallbackIsNotReplayed(t *testing.T) {
	c := bbFixtureClient(t)
	ctx := context.Background()
	id := "fedcba9876543210fedcba9876543210"
	p := bbPending{Schema: "lycheedev.blackbox.pending-login.v1", ID: id, Status: "waiting_confirmation", Created: 1700000000, Expires: 1700000300, Confirmed: true, Next: "https://api.xiaoheihe.cn/account/wechat/login_redirect/v2/web_sso/?code=one-use&state=fixture", Session: *c.session}
	p.Session.Method = "wechat"
	if e := bbSave(ctx, c.home, "pending", p); e != nil {
		t.Fatal(e)
	}
	callbacks := 0
	c.http.Transport = bbTestTransport(func(r *http.Request) (*http.Response, error) {
		switch r.URL.Path {
		case "/account/wechat/login_redirect/v2/web_sso/":
			callbacks++
			return bbTestResponse(503, `{}`), nil
		case "/account/wechat/state/", "/account/restore_login":
			return bbRestoreReply(), nil
		case "/wow/open_platform/module/list/":
			return bbReply(map[string]any{"moduleList": []any{}}), nil
		case "/wow/get_user_configuration_backup_list/":
			return bbReply(map[string]any{"backupList": []any{}}), nil
		}
		t.Fatal(r.URL.Path)
		return nil, nil
	})
	if _, e := c.auth(ctx, "session login-status", map[string]any{"login_id": id}); e == nil {
		t.Fatal("callback uncertainty hidden")
	}
	r, e := c.auth(ctx, "session login-status", map[string]any{"login_id": id})
	if e != nil || !r.Complete || callbacks != 1 {
		t.Fatal(r, e, callbacks)
	}
}
