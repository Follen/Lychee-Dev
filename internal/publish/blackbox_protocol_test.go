package publish

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"strings"
	"testing"
	"time"
)

func TestBlackboxSignerMatchesFixedFuploader(t *testing.T) {
	for _, tc := range []struct {
		path        string
		stamp       int64
		nonce, want string
	}{{"/wow/v2/dynamic_conf/", 1700000000, "0123456789ABCDEF0123456789ABCDEF", "UZ1UT09"}, {"/account/restore_login", 1700000001, "ABCDEF", "T10US41"}, {"//wow//get_wa_list/", 1793600000, "F0F0F0F0", "3V03Z43"}} {
		if got := bbSign(tc.path, tc.stamp, tc.nonce); got != tc.want {
			t.Fatalf("fixed signer mismatch: %s != %s", got, tc.want)
		}
	}
}
func TestBlackboxIdentityAndCookieScope(t *testing.T) {
	now := time.Unix(1700000000, 0)
	s := &bbSession{}
	data := map[string]any{"profile": map[string]any{"heybox_id": "123"}, "pkey": "private-test", "expire_at": json.Number("1700000300")}
	if err := bbApply(s, data, now, ""); err != nil {
		t.Fatal(err)
	}
	if uid, err := bbIdentity(s); err != nil || uid != "123" {
		t.Fatal(uid, err)
	}
	if err := bbApply(s, data, now, "456"); err == nil {
		t.Fatal("account switch accepted")
	}
	s.Cookies[0].Domain = "evil.example"
	if _, err := bbIdentity(s); err == nil {
		t.Fatal("foreign cookie domain accepted")
	}
}

type bbTestTransport func(*http.Request) (*http.Response, error)

func (f bbTestTransport) RoundTrip(r *http.Request) (*http.Response, error) { return f(r) }
func bbTestResponse(status int, raw string) *http.Response {
	return &http.Response{StatusCode: status, Header: make(http.Header), Body: io.NopCloser(strings.NewReader(raw))}
}
func TestBlackboxHTTPEncodingAndNoRedirect(t *testing.T) {
	c := newBB(t.TempDir())
	c.now = func() time.Time { return time.Unix(1700000000, 0) }
	s := &bbSession{Device: "test-device", Cookies: []bbCookie{}}
	calls := 0
	c.http.Transport = bbTestTransport(func(r *http.Request) (*http.Response, error) {
		calls++
		if r.URL.Host != "workshopapi.xiaoheihe.cn" || r.URL.Path != "/wow/update_wa/" {
			t.Fatal("wrong route")
		}
		if r.URL.Query().Get("hkey") == "" {
			t.Fatal("missing signature")
		}
		r.ParseForm()
		if r.PostForm.Get("configuration") != `{"name":"中文+&"}` {
			t.Fatalf("form corrupted: %s", r.PostForm.Get("configuration"))
		}
		res := bbTestResponse(302, "")
		res.Header.Set("Location", "https://evil.example/")
		return res, nil
	})
	_, err := c.raw(context.Background(), s, "POST", "/wow/update_wa/", map[string]any{"slug": "one"}, map[string]any{"configuration": `{"name":"中文+&"}`})
	if err == nil || calls != 1 {
		t.Fatal("redirect followed or hidden")
	}
}
