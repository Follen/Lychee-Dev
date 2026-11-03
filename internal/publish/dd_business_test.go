package publish

import (
	"bytes"
	"compress/flate"
	"context"
	"encoding/json"
	"errors"
	"net/url"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

type ddFake struct {
	calls int
	fn    func(string, url.Values, any, bool) (map[string]any, error)
}

func (f *ddFake) call(_ context.Context, p string, q url.Values, b any, m bool) (map[string]any, error) {
	f.calls++
	return f.fn(p, q, b, m)
}
func (f *ddFake) channels(context.Context) (any, error) { return []any{}, nil }
func (f *ddFake) identity() map[string]any              { return map[string]any{"author_id": "author-1"} }
func (f *ddFake) close() error                          { return nil }
func ddEnvelope(v any) map[string]any                   { return map[string]any{"code": json.Number("200"), "result": v} }
func TestDDWAFixedVendorSerializerFixtures(t *testing.T) {
	for _, name := range []string{"modern.txt", "modern-serialization-v2.txt", "legacy.txt", "legacy-explicit.txt"} {
		t.Run(name, func(t *testing.T) {
			raw, err := os.ReadFile(filepath.Join("dd", "testdata", "wa", name))
			if err != nil {
				t.Fatal(err)
			}
			id, err := parseDDWA(context.Background(), string(raw))
			if err != nil {
				t.Fatal(err)
			}
			if id.UID != "SYNTHETIC-uid-001" || id.ID != "Synthetic WA 中文 ^ ~ 😀" {
				t.Fatalf("wrong identity: %#v", id)
			}
		})
	}
}
func ddEncodeWA(raw []byte) string {
	var b bytes.Buffer
	z, _ := flate.NewWriter(&b, flate.DefaultCompression)
	z.Write(raw)
	z.Close()
	alphabet := "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789()"
	var out strings.Builder
	var bits uint32
	var n uint
	for _, v := range b.Bytes() {
		bits |= uint32(v) << n
		n += 8
		for n >= 6 {
			out.WriteByte(alphabet[bits&63])
			bits >>= 6
			n -= 6
		}
	}
	if n > 0 {
		out.WriteByte(alphabet[bits])
	}
	return out.String()
}
func TestDDWARejectsMalformedWithoutEvaluation(t *testing.T) {
	for _, s := range []string{"", "!WA:3!a", "!WA:no!a", "!WA:2!@", "!WA:2!" + ddEncodeWA([]byte{1, 208, 0}), "!" + ddEncodeWA([]byte("^1^Sreturn~`os.execute('PRIVATE')^^"))} {
		if _, err := parseDDWA(context.Background(), s); err == nil || strings.Contains(err.Error(), "PRIVATE") {
			t.Fatalf("bad export result: %v", err)
		}
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err := parseDDWA(ctx, "garbage"); !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
}
func TestDDStrictInputAndEditBoundary(t *testing.T) {
	for _, action := range []string{"plugin create", "plugin edit", "plugin update", "config create", "wa edit", "backup generate", "backup upload", "plugin upload"} {
		s, ok := DDInput(action)
		if !ok {
			t.Fatal(action)
		}
		raw, _ := json.Marshal(s.Example)
		if _, err := s.Decode(raw); err != nil {
			t.Fatalf("%s example: %v", action, err)
		}
	}
	s, _ := DDInput("plugin edit")
	for _, raw := range []string{`{"session":"s","sn":"x","version":"2"}`, `{"session":"s","sn":"x","description":null}`, `{"session":"s","sn":"x","description":"a","description":"b"}`} {
		if _, err := s.Decode([]byte(raw)); err == nil {
			t.Fatal(raw)
		}
	}
	v, err := s.Decode([]byte(`{"session":"s","sn":"x","description":"","with_associate":false,"associated_acts":[]}`))
	if err != nil || v["description"] != "" || v["with_associate"] != false {
		t.Fatal(v, err)
	}
}
func TestDDTransportSignedRoutePreserved(t *testing.T) {
	for _, signed := range []string{"https://other.invalid/x?a=1", "https://uiapi.w.163.com/x?a=2", "https://uiapi.w.163.com/y?a=1"} {
		if ddSignedTarget("https://uiapi.w.163.com/x?a=1", signed) == nil {
			t.Fatal(signed)
		}
	}
	if err := ddSignedTarget("https://uiapi.w.163.com/x?a=1", "https://uiapi.w.163.com/x?a=1&signature=x"); err != nil {
		t.Fatal(err)
	}
}
func TestDDPaginationDoesNotTreatRepeatedOrMalformedPagesAsAbsence(t *testing.T) {
	f := &ddFake{fn: func(string, url.Values, any, bool) (map[string]any, error) {
		return ddEnvelope(map[string]any{"list": []any{map[string]any{"sn": "x"}}, "total": json.Number("2")}), nil
	}}
	if _, err := ddAllAuthor(context.Background(), f, "plugin", 1); err == nil || f.calls != 2 {
		t.Fatalf("repeated page: %v, %d", err, f.calls)
	}
	f.fn = func(string, url.Values, any, bool) (map[string]any, error) { return ddEnvelope(map[string]any{}), nil }
	if _, err := ddRead(context.Background(), f, "plugin list", map[string]any{"game_type": json.Number("1")}); err == nil {
		t.Fatal("malformed empty list accepted")
	}
}
func TestDDOriginalUnknownOperationIsNotReplayed(t *testing.T) {
	home := t.TempDir()
	input := map[string]any{"session": "s", "sn": "backup-1", "name": "New"}
	writes := 0
	f := &ddFake{fn: func(path string, _ url.Values, body any, mutation bool) (map[string]any, error) {
		if mutation {
			writes++
			return nil, errors.New("response lost")
		}
		if path == "/backup/list" {
			return ddEnvelope([]any{map[string]any{"sn": "backup-1"}}), nil
		}
		return ddEnvelope(map[string]any{"sn": "backup-1", "name": "Old"}), nil
	}}
	req := Request{Home: home, RequestID: "rename-1"}
	first, err := ddMutationRun(context.Background(), f, req, "backup rename", input)
	if err == nil || first.Submission != "uncertain" {
		t.Fatal(first, err)
	}
	before := f.calls
	second, err := ddMutationRun(context.Background(), f, req, "backup rename", input)
	if err == nil || writes != 1 || f.calls != before || second.OperationID != first.OperationID {
		t.Fatal(second, err, writes, f.calls)
	}
	input["name"] = "Different"
	if _, err = ddMutationRun(context.Background(), f, req, "backup rename", input); err == nil {
		t.Fatal("changed original input accepted")
	}
}
func TestDDSelectedGroupsPreserveVersionsAndExplicitEmpty(t *testing.T) {
	backup := map[string]any{"font": map[string]any{"items": []any{"A.ttf", "B.ttf"}}}
	current := map[string]any{"font": map[string]any{"items": []any{"A.ttf"}, "inner_version": map[string]any{"A.ttf": json.Number("4"), "B.ttf": json.Number("2")}}}
	g, err := ddSelectedGroup(backup, current, "font", "", []any{"A.ttf"}, []any{"A.ttf"})
	if err != nil || ddMap(g["inner_version"])["A.ttf"] != int64(5) {
		t.Fatal(g, err)
	}
	g, err = ddSelectedGroup(backup, current, "font", "", []any{}, nil)
	if err != nil || len(ddSlice(g["items"])) != 0 {
		t.Fatal(g, err)
	}
	if _, err = ddSelectedGroup(backup, current, "font", "", []any{"A.ttf"}, []any{"B.ttf"}); err == nil {
		t.Fatal("unselected update marker accepted")
	}
}
func TestDDSelectiveBackupRoundTripAndTamper(t *testing.T) {
	root := t.TempDir()
	write := func(path, content string) {
		t.Helper()
		p := filepath.Join(root, filepath.FromSlash(path))
		if err := os.MkdirAll(filepath.Dir(p), 0700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(p, []byte(content), 0600); err != nil {
			t.Fatal(err)
		}
	}
	write("Interface/AddOns/Selected/main.lua", "selected")
	write("Interface/AddOns/Other/main.lua", "private-unselected")
	write("WTF/Account/ACCOUNT/Realm/Chosen/config.wtf", "chosen")
	write("WTF/Account/ACCOUNT/Realm/Other/config.wtf", "private-other-role")
	input := map[string]any{"root": root, "output": filepath.Join(t.TempDir(), "bundle"), "game_type": json.Number("1"), "unknown_addons": []any{"Selected"}, "wtf_accounts": []any{map[string]any{"account": "ACCOUNT", "include_common": false, "roles": []any{map[string]any{"server": "Realm", "name": "Chosen"}}}}}
	result, err := ddBackupGenerate(context.Background(), input)
	if err != nil {
		t.Fatal(err)
	}
	path := ddText(ddMap(result)["bundle_file"])
	bundle, err := ddLoadBundle(path)
	if err != nil {
		t.Fatal(err)
	}
	if len(bundle.files) != 2 {
		t.Fatal(bundle.bundle)
	}
	bundle.close()
	artifact := filepath.Join(filepath.Dir(path), "unknown_addon.zip")
	if err = os.WriteFile(artifact, []byte("changed"), 0600); err != nil {
		t.Fatal(err)
	}
	if _, err = ddLoadBundle(path); err == nil {
		t.Fatal("tampered backup accepted")
	}
}

func TestDDConfigEditPreservesNestedSelectionAndUsesShareSN(t *testing.T) {
	current := map[string]any{"sn": "share-1", "is_owner": true, "backup_sn": "backup-1", "game_type": json.Number("10002"), "scope": "public", "title": "Old", "brief_desc": "Brief", "desc": "Detail", "display_imgs": []any{"https://assets.example/image.png"}, "creation_statement": "original", "need_buy": json.Number("0"), "font": map[string]any{"items": []any{"A.ttf"}, "inner_version": map[string]any{"A.ttf": json.Number("7")}}, "wtf": map[string]any{"accounts": []any{}}, "retail_ui_config": map[string]any{"opaque": "legacy"}}
	backup := map[string]any{"sn": "backup-1", "game_type": json.Number("10002"), "font": map[string]any{"items": []any{"A.ttf", "B.ttf"}}, "wtf": map[string]any{"accounts": []any{}}}
	f := &ddFake{fn: func(p string, _ url.Values, _ any, m bool) (map[string]any, error) {
		if m {
			t.Fatal("prepare sent mutation")
		}
		switch p {
		case "/share/detail":
			return ddEnvelope(current), nil
		case "/backup/detail":
			return ddEnvelope(backup), nil
		case "/backup/list":
			return ddEnvelope([]any{map[string]any{"sn": "backup-1"}}), nil
		}
		t.Fatalf("unexpected dependency %s", p)
		return nil, nil
	}}
	form, game, err := ddPrepareForm(context.Background(), f, "config", "edit", map[string]any{"sn": "share-1", "title": "New", "with_associate": false, "associated_acts": []any{}})
	if err != nil {
		t.Fatal(err)
	}
	if form["share_sn"] != "share-1" || form["sn"] != nil || form["game_type"] != nil || game != 10002 {
		t.Fatal(form, game)
	}
	if !ddSubset(current["font"], form["font"]) {
		t.Fatal("selection/version lost", form)
	}
	if form["retail_ui_config"] != nil {
		t.Fatal("nonretail copied retail layout")
	}
}
func TestDDRetailLayoutOmissionClearAndDefault(t *testing.T) {
	backup := map[string]any{"sn": "b", "game_type": json.Number("10001"), "retail_ui_config": map[string]any{"editMode": map[string]any{"ACCOUNT": []any{map[string]any{"name": "layout", "opaque": "retained"}}}}}
	catalog := ddRetailCatalog(backup)
	selector := catalog["edit_mode"][0].selector
	current := map[string]any{"retail_ui_config": map[string]any{"enable_dd_setup_wizard": true, "unrelated": map[string]any{"keep": true}}}
	v, err := ddRetailForm(backup, current, map[string]any{"edit_mode_selectors": []any{selector}, "default_edit_mode_selector": selector})
	if err != nil || ddMap(v)["unrelated"] == nil || !Bool(ddMap(v), "enable_dd_setup_wizard") {
		t.Fatal(v, err)
	}
	if _, err = ddRetailForm(backup, current, map[string]any{"edit_mode_selectors": []any{selector}}); err == nil {
		t.Fatal("missing default admitted")
	}
	v, err = ddRetailForm(backup, current, nil)
	if err != nil || v != nil {
		t.Fatal(v, err)
	}
}
func TestDDBackupReusedComponentsAndReadbackNoReplay(t *testing.T) {
	root := t.TempDir()
	path := filepath.Join(root, "Interface", "AddOns", "A", "A.lua")
	os.MkdirAll(filepath.Dir(path), 0700)
	os.WriteFile(path, []byte("return 1"), 0600)
	v, err := ddBackupGenerate(context.Background(), map[string]any{"root": root, "output": filepath.Join(t.TempDir(), "bundle"), "game_type": json.Number("1"), "unknown_addons": []any{"A"}})
	if err != nil {
		t.Fatal(err)
	}
	file := ddText(ddMap(v)["bundle_file"])
	opened, err := ddLoadBundle(file)
	if err != nil {
		t.Fatal(err)
	}
	manifest := ddClone(opened.bundle.Manifest)
	opened.close()
	manifest["sn"] = "created-backup"
	writes := 0
	f := &ddFake{fn: func(p string, _ url.Values, b any, m bool) (map[string]any, error) {
		if m {
			writes++
		}
		switch p {
		case "/game_type/list":
			return ddEnvelope([]any{map[string]any{"game_type": json.Number("1")}}), nil
		case "/backup/pre_create":
			return ddEnvelope(map[string]any{"sn": "created-backup", "unknown_addon": map[string]any{"u_url": ""}}), nil
		case "/backup/finish_create":
			return ddEnvelope(map[string]any{}), nil
		case "/backup/detail":
			return ddEnvelope(manifest), nil
		case "/backup/list":
			return ddEnvelope([]any{map[string]any{"sn": "created-backup"}}), nil
		}
		t.Fatalf("unexpected %s", p)
		return nil, nil
	}}
	req := Request{Home: t.TempDir(), RequestID: "backup-1"}
	result, err := ddBackupUpload(context.Background(), f, req, map[string]any{"file": file})
	if err != nil || !result.Complete || writes != 2 {
		t.Fatal(result, err, writes)
	}
	calls := f.calls
	result, err = ddBackupUpload(context.Background(), f, req, map[string]any{"file": file})
	if err != nil || !result.Complete || f.calls != calls {
		t.Fatal(result, err, f.calls, calls)
	}
}
func TestDDBackupDeleteStopsAtReferencedConfiguration(t *testing.T) {
	writes := 0
	f := &ddFake{fn: func(p string, _ url.Values, _ any, m bool) (map[string]any, error) {
		if m {
			writes++
		}
		switch p {
		case "/backup/detail":
			return ddEnvelope(map[string]any{"sn": "b", "game_type": json.Number("1")}), nil
		case "/backup/list":
			return ddEnvelope([]any{map[string]any{"sn": "b"}}), nil
		case "/share/list":
			return ddEnvelope([]any{map[string]any{"sn": "s"}}), nil
		case "/share/detail":
			return ddEnvelope(map[string]any{"sn": "s", "backup_sn": "b"}), nil
		}
		return nil, errors.New("unexpected")
	}}
	_, err := ddMutationRun(context.Background(), f, Request{Home: t.TempDir(), RequestID: "delete-1"}, "backup delete", map[string]any{"sn": "b"})
	if err == nil || writes != 0 {
		t.Fatal(err, writes)
	}
}
