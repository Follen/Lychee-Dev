//go:build windows && amd64

package publish

import (
	"context"
	"net/url"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestDDNativeSignerRejectsUnpinnedImage(t *testing.T) {
	p := filepath.Join(t.TempDir(), "libnep.dll")
	if err := os.WriteFile(p, []byte("not SDK"), 0600); err != nil {
		t.Fatal(err)
	}
	if _, err := openDDSigner(context.Background(), p); err == nil {
		t.Fatal("unverified image loaded")
	}
}
func TestDDNativeSignerPinnedLocalABI(t *testing.T) {
	p := os.Getenv("LYCHEEDEV_DD_SDK")
	if p == "" {
		t.Skip("set LYCHEEDEV_DD_SDK for local SDK ABI check")
	}
	s, err := openDDSigner(context.Background(), p)
	if err != nil {
		t.Fatal(err)
	}
	defer s.Close()
	for _, body := range [][]byte{nil, []byte(`{"name":"中文😀","value":0}`)} {
		result, err := s.Sign(context.Background(), "https://uiapi.w.163.com/server/ts?test=local", body, [][2]string{{"x-w163-token", "synthetic"}, {"x-timestamp", "1793500000"}})
		if err != nil {
			t.Fatal(err)
		}
		u, err := url.Parse(result)
		if err != nil || u.Scheme != "https" || u.Host != "uiapi.w.163.com" || u.Path != "/server/ts" || u.Query().Get("test") != "local" || len(u.RawQuery) < 20 {
			t.Fatal("signature did not preserve target")
		}
	}
	if err := s.Close(); err != nil {
		t.Fatal(err)
	}
	if _, err := s.Sign(context.Background(), "https://uiapi.w.163.com/", nil, nil); err == nil {
		t.Fatal("closed signer accepted")
	}
}
func TestDDNativeSignerCancelled(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err := openDDSigner(ctx, strings.Repeat("x", 10)); err != context.Canceled {
		t.Fatal(err)
	}
}
