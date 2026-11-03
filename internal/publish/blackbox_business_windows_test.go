//go:build windows

package publish

import (
	"archive/zip"
	"context"
	"encoding/json"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func bbFixtureClient(t *testing.T) *bbClient {
	t.Helper()
	c := newBB(t.TempDir())
	c.now = func() time.Time { return time.Unix(1700000000, 0) }
	s := &bbSession{Method: "sms", Device: "fixture"}
	if e := bbApply(s, map[string]any{"profile": map[string]any{"heybox_id": "123"}, "pkey": "fixture-private", "expire_at": json.Number("1700003600")}, c.now(), ""); e != nil {
		t.Fatal(e)
	}
	if e := bbSaveSession(context.Background(), c.home, s); e != nil {
		t.Fatal(e)
	}
	c.session = s
	return c
}
func bbReply(v any) *http.Response {
	return bbTestResponse(200, bbJSON(map[string]any{"status": "ok", "result": v}))
}
func bbRestoreReply() *http.Response {
	return bbReply(map[string]any{"profile": map[string]any{"heybox_id": "123"}, "pkey": "fixture-private", "expire_at": 1700003600})
}

func TestBlackboxConfigClearRequiresEmptyReadback(t *testing.T) {
	c := bbFixtureClient(t)
	ctx := context.Background()
	current := map[string]any{"ShareID": "share", "BackupID": "backup", "IsClassic": false,
		"Title": "Original", "Article": "", "Open": false, "ThumbList": []any{}, "SubscribeLevel": 0,
		"KnownAddons": []any{map[string]any{"ID": "1"}}, "WTFList": []any{},
		"UnknownAddons": map[string]any{"Host": "original", "Files": []any{map[string]any{"Name": "Addon"}}}}
	backup := bbClone(current)
	backup["ID"] = "backup"
	c.http.Transport = bbTestTransport(func(r *http.Request) (*http.Response, error) {
		switch r.URL.Path {
		case "/wow/get_user_configuration_backup_list/":
			return bbReply(map[string]any{"backupList": []any{backup}}), nil
		case "/wow/v2/share/detail/":
			return bbReply(current), nil
		case "/wow/v2/share/list/":
			rows := []any{}
			if r.URL.Query().Get("offset") == "0" {
				rows = append(rows, current)
			}
			return bbReply(map[string]any{"shares": rows}), nil
		}
		t.Fatalf("unexpected request %s", r.URL.Path)
		return nil, nil
	})
	spec, _ := BlackboxInput("config edit")
	in, e := spec.Decode([]byte(`{"share_id":"share","configuration":{"UnknownAddons":{"Files":[]}}}`))
	if e != nil {
		t.Fatal(e)
	}
	op := &bbOperation{Action: "config edit", Input: in}
	if e = c.prepareConfig(ctx, op); e != nil {
		t.Fatal(e)
	}
	op.Steps[0].State = "submitted"
	if e = c.verify(ctx, op); e == nil || op.Verified {
		t.Fatalf("unchanged selected files verified: %v", e)
	}
	delete(current, "UnknownAddons")
	if e = c.verify(ctx, op); e != nil || !op.Verified {
		t.Fatalf("cleared files did not verify: %v", e)
	}
}

func TestBlackboxModuleUnknownWriteUsesOriginalReadback(t *testing.T) {
	c := bbFixtureClient(t)
	ctx := context.Background()
	writes := 0
	name := "original"
	calls := 0
	c.http.Transport = bbTestTransport(func(r *http.Request) (*http.Response, error) {
		calls++
		switch r.URL.Path {
		case "/account/restore_login":
			return bbRestoreReply(), nil
		case "/wow/open_platform/module/detail/":
			return bbReply(map[string]any{"module": map[string]any{"id": 7, "name": name, "logoUrl": "https://example.test/keep.png", "categoryIds": []int{2}, "type": 1, "desc": "keep-original-description", "official": "keep", "officialUrl": "https://example.test", "coreFolders": "Example"}}), nil
		case "/wow/open_platform/module/update/":
			writes++
			_ = r.ParseForm()
			if r.PostForm.Get("desc") != "keep-original-description" || r.PostForm.Get("coreFolders") != "Example" || r.PostForm.Get("id") != "7" {
				t.Fatal("omitted fields lost", r.PostForm)
			}
			name = r.PostForm.Get("name")
			return bbTestResponse(503, `{}`), nil
		}
		t.Fatalf("unexpected route %s", r.URL.Path)
		return nil, nil
	})
	result, e := c.run(ctx, "plugin edit", Request{RequestID: "same-op", Input: []byte(`{"id":7,"name":"new"}`)})
	if e == nil || result.Complete || writes != 1 {
		t.Fatal(result, e, writes)
	}
	originalCalls := calls
	_, e = c.run(ctx, "plugin edit", Request{RequestID: "same-op", Input: []byte(`{"id":7,"name":"different"}`)})
	if e == nil || calls != originalCalls {
		t.Fatal("changed request accepted")
	}
	result, e = c.run(ctx, "operation resume", Request{Input: []byte(`{"request":"same-op"}`)})
	if e != nil || !result.Complete || writes != 1 {
		t.Fatal(result, e, writes)
	}
	raw := bbJSON(result)
	if strings.Contains(raw, "keep-original-description") {
		t.Fatal("private content leaked")
	}
}
func TestBlackboxOperationAccountSourceIsBound(t *testing.T) {
	c := bbFixtureClient(t)
	op := &bbOperation{Schema: "lycheedev.blackbox.operation.v1", ID: "one", Action: "plugin edit", Account: "123", Method: "wechat", Input: map[string]any{}, Stage: "prepared"}
	op.Digest = bbDigest(op.Input)
	if e := c.saveOperation(context.Background(), op); e != nil {
		t.Fatal(e)
	}
	requests := 0
	c.http.Transport = bbTestTransport(func(r *http.Request) (*http.Response, error) {
		requests++
		if r.URL.Path != "/account/restore_login" {
			t.Fatal("unexpected mutation")
		}
		return bbRestoreReply(), nil
	})
	_, e := c.run(context.Background(), "operation resume", Request{Input: []byte(`{"request":"one"}`)})
	if e == nil || requests != 1 {
		t.Fatal("session source changed", e)
	}
}
func TestBlackboxVersionPaginationContinuesShortPages(t *testing.T) {
	c := bbFixtureClient(t)
	offsets := []string{}
	c.http.Transport = bbTestTransport(func(r *http.Request) (*http.Response, error) {
		offset := r.URL.Query().Get("offset")
		offsets = append(offsets, offset)
		switch offset {
		case "0":
			return bbReply(map[string]any{"versionList": []any{map[string]any{"id": 1, "moduleId": 7}}}), nil
		case "1":
			return bbReply(map[string]any{"versionList": []any{map[string]any{"id": 2, "moduleId": 7}}}), nil
		case "2":
			return bbReply(map[string]any{"versionList": []any{}}), nil
		}
		t.Fatal(offset)
		return nil, nil
	})
	rows, _, e := c.versions(context.Background(), "7")
	if e != nil || len(rows) != 2 || strings.Join(offsets, ",") != "0,1,2" {
		t.Fatal(rows, offsets, e)
	}
	c.http.Transport = bbTestTransport(func(r *http.Request) (*http.Response, error) {
		return bbReply(map[string]any{"versionList": []any{map[string]any{"id": 1, "moduleId": 7}}}), nil
	})
	if _, _, e = c.versions(context.Background(), "7"); e == nil {
		t.Fatal("duplicate page accepted")
	}
}
func TestBlackboxArchiveCanonicalizationPreservesOriginalVersion(t *testing.T) {
	options := []any{map[string]any{"strappid": "wowretail", "gameVersions": []any{"12.1.5"}}}
	old := "https://wow.max-c.com/wow/wow_module_file/wowretail/" + strings.Repeat("a", 32) + ".zip"
	newURL := "https://wow.max-c.com/wow/wow_module_file/wowretail/" + strings.Repeat("b", 32) + ".zip"
	op := &bbOperation{Action: "version edit", Input: map[string]any{"module_id": 7, "version_id": 9}, Archive: &bbArchive{}, OriginalArchive: old, OriginalBranch: "wowretail", Expected: map[string]any{"gameVersions": []any{"12.1.5"}}, Steps: []bbStep{{Body: map[string]any{"fileUrl": "https://wow.max-c.com/wow/module/upload.zip"}}}}
	row := map[string]any{"id": 9, "moduleId": 7, "fileUrl": old}
	if bbArchiveMode(op, row, options) != "" {
		t.Fatal("old archive accepted as replacement")
	}
	row["fileUrl"] = newURL
	if bbArchiveMode(op, row, options) != "server_replacement_canonicalization" {
		t.Fatal("new canonical archive rejected")
	}
	row["id"] = 10
	if bbArchiveMode(op, row, options) != "" {
		t.Fatal("different version accepted")
	}
}
func TestBlackboxConfigurationSelectionRetainsArchiveMetadata(t *testing.T) {
	backup := map[string]any{"KnownAddons": []any{map[string]any{"ID": "1", "VersionID": "v1", "Name": "Example"}}, "WTFList": []any{}, "UnknownAddons": map[string]any{"Url": "https://wow.max-c.com/original.zip", "Files": []any{map[string]any{"Name": "Custom", "MD5": "fixed-hash"}}}}
	config := map[string]any{"KnownAddons": []any{map[string]any{"ID": "1"}}, "WTFList": []any{}, "UnknownAddons": bbClone(backup["UnknownAddons"].(map[string]any))}
	bbSetConfig(config, "UnknownAddons", map[string]any{"Files": []any{map[string]any{"Name": "Custom"}}})
	if e := bbValidateSelection(config, backup); e != nil {
		t.Fatal(e)
	}
	container := config["UnknownAddons"].(map[string]any)
	if container["Url"] != "https://wow.max-c.com/original.zip" || container["Files"].([]any)[0].(map[string]any)["Name"] != "Custom" {
		t.Fatal("original archive metadata lost", container)
	}
	config["KnownAddons"] = []any{map[string]any{"ID": "other"}}
	if e := bbValidateSelection(config, backup); e == nil {
		t.Fatal("foreign addon selected")
	}
}
func TestBlackboxWAWritePreservesContentAndDoesNotClaimPrivacy(t *testing.T) {
	c := bbFixtureClient(t)
	ctx := context.Background()
	writes := 0
	name := "Original"
	secret := "!WA:2!PRIVATE-CONTENT"
	c.http.Transport = bbTestTransport(func(r *http.Request) (*http.Response, error) {
		switch r.URL.Path {
		case "/account/restore_login":
			return bbRestoreReply(), nil
		case "/wow/v2/get_user_info/":
			return bbReply(map[string]any{"heybox_id": "123"}), nil
		case "/wow/get_wa_list/":
			rows := []any{}
			if r.URL.Query().Get("offset") == "0" {
				rows = append(rows, map[string]any{"Slug": "one", "HeyboxId": "123", "Name": name, "LatestVersion": map[string]any{"Version": 1}})
			}
			return bbReply(map[string]any{"waList": rows}), nil
		case "/wow/get_wa_detail/":
			return bbReply(map[string]any{"Slug": "one", "HeyboxId": "123", "Name": name, "Description": "original description", "UpdateLog": "original log", "Categories": []any{"tag"}, "Encoded": secret, "ThumbList": []any{"https://example.test/image.png"}}), nil
		case "/wow/get_wa_tag_list/":
			return bbReply(map[string]any{"list": []any{map[string]any{"TagList": []any{map[string]any{"Key": "tag"}}}}}), nil
		case "/wow/update_wa/":
			writes++
			_ = r.ParseForm()
			var body map[string]any
			if e := json.Unmarshal([]byte(r.PostForm.Get("configuration")), &body); e != nil {
				t.Fatal(e)
			}
			if body["encoded"] != secret || body["descriptionHeybox"] != "original description" {
				t.Fatal("original content lost")
			}
			name = body["name"].(string)
			return bbReply(nil), nil
		}
		t.Fatal(r.URL.Path)
		return nil, nil
	})
	result, e := c.run(ctx, "wa edit", Request{RequestID: "wa-op", Input: []byte(`{"scope":"public","slug":"one","configuration":{"strappid":"wowretail","open":false,"subscribeLevel":0,"name":"Changed"}}`)})
	if e != nil || !result.Complete || writes != 1 {
		t.Fatal(result, e)
	}
	raw := bbJSON(result)
	if strings.Contains(raw, secret) || !strings.Contains(raw, `"anonymous_can_view":null`) || !strings.Contains(raw, `"open"`) {
		t.Fatal("content/privacy output", raw)
	}
}
func TestBlackboxArchiveSnapshotPinsBytes(t *testing.T) {
	ctx := context.Background()
	root := t.TempDir()
	path := filepath.Join(root, "original.zip")
	f, e := os.Create(path)
	if e != nil {
		t.Fatal(e)
	}
	z := zip.NewWriter(f)
	w, _ := z.Create("Example/Example.toc")
	_, _ = w.Write([]byte("## Interface: 120105"))
	_ = z.Close()
	_ = f.Close()
	a, e := bbSnapshot(ctx, root, "one", path)
	if e != nil {
		t.Fatal(e)
	}
	if e = os.WriteFile(path, []byte("changed caller path"), 0600); e != nil {
		t.Fatal(e)
	}
	if e = bbCheckArchive(a); e != nil {
		t.Fatal("snapshot changed with caller path", e)
	}
	if e = os.WriteFile(a.Path, []byte("changed original"), 0600); e != nil {
		t.Fatal(e)
	}
	if bbCheckArchive(a) == nil {
		t.Fatal("changed snapshot accepted")
	}
}
func TestBlackboxPlanNeverExposesOTP(t *testing.T) {
	c := newBB(t.TempDir())
	c.http.Transport = bbTestTransport(func(*http.Request) (*http.Response, error) { t.Fatal("plan network"); return nil, nil })
	r, e := c.run(context.Background(), "session login-submit", Request{Plan: true, Input: []byte(`{"login_id":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","code":"987654"}`)})
	if e != nil || strings.Contains(bbJSON(r), "987654") {
		t.Fatal(r, e)
	}
}
