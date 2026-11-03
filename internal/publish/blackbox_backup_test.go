package publish

import (
	"archive/zip"
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func bbBackupFixture(t *testing.T) (map[string]any, string) {
	t.Helper()
	base := t.TempDir()
	root := filepath.Join(base, "game")
	files := map[string]string{"Interface/AddOns/Selected/a.lua": "addon", "Interface/AddOns/Other/b.lua": "other", "Interface/Art/x.blp": "art", "Fonts/font.ttf": "font", "WTF/Account/ACC/account.wtf": "account", "WTF/Account/ACC/SavedVariables/Shared.lua": "shared", "WTF/Account/ACC/SavedVariables/HeyboxPlayerInfo.lua": "excluded", "WTF/Account/ACC/Realm/Hero/config-cache.wtf": "hero", "WTF/Account/ACC/Realm/Hero/SavedVariables/A.lua": "hero lua", "WTF/Account/ACC/Realm/Other/config-cache.wtf": "other", "WTF/Account/ACC/Realm/Other/SavedVariables/B.lua": "other lua"}
	for p, s := range files {
		path := filepath.Join(root, filepath.FromSlash(p))
		if e := os.MkdirAll(filepath.Dir(path), 0700); e != nil {
			t.Fatal(e)
		}
		if e := os.WriteFile(path, []byte(s), 0600); e != nil {
			t.Fatal(e)
		}
	}
	in := map[string]any{"game_dir": root, "output_dir": filepath.Join(base, "package"), "strappid": "wowretail", "unknown_addons": []any{"Selected"}, "materials": []any{"Art"}, "fonts": []any{"font.ttf"}, "wtf_accounts": []any{map[string]any{"name": "ACC", "characters": []any{map[string]any{"realm": "Realm", "player": "Hero"}}}}}
	return in, root
}
func TestBlackboxBackupSelectivePackageRoundtrip(t *testing.T) {
	in, root := bbBackupFixture(t)
	scan, e := bbLocalBackup(context.Background(), "backup scan", in)
	if e != nil {
		t.Fatal(e)
	}
	if len(scan.(map[string]any)["wtf_accounts"].([]any)) != 1 {
		t.Fatal(scan)
	}
	result, e := bbLocalBackup(context.Background(), "backup generate", in)
	if e != nil {
		t.Fatal(e)
	}
	p, e := bbLoadBackupPackage(context.Background(), String(result.(map[string]any), "file"))
	if e != nil {
		t.Fatal(e)
	}
	if len(p.Archives) != 4 {
		t.Fatal(len(p.Archives))
	}
	for _, a := range p.Archives {
		z, e := zip.OpenReader(a.Local)
		if e != nil {
			t.Fatal(e)
		}
		for _, entry := range z.File {
			if strings.Contains(entry.Name, "Other") || strings.Contains(strings.ToLower(entry.Name), "heyboxplayerinfo") {
				t.Fatal("unselected file", entry.Name)
			}
		}
		z.Close()
	}
	if _, e = os.Stat(filepath.Join(root, "Interface", "AddOns", "Other", "b.lua")); e != nil {
		t.Fatal("source changed")
	}
	if _, e = bbLocalBackup(context.Background(), "backup generate", in); e == nil {
		t.Fatal("existing output overwritten")
	}
}
func TestBlackboxBackupRejectsTamperedPackage(t *testing.T) {
	for _, mode := range []string{"hash", "traversal", "identity", "character", "missing"} {
		t.Run(mode, func(t *testing.T) {
			in, _ := bbBackupFixture(t)
			r, e := bbLocalBackup(context.Background(), "backup generate", in)
			if e != nil {
				t.Fatal(e)
			}
			file := String(r.(map[string]any), "file")
			raw, _ := os.ReadFile(file)
			var p bbBackupPackage
			d := json.NewDecoder(strings.NewReader(string(raw)))
			d.UseNumber()
			if e = d.Decode(&p); e != nil {
				t.Fatal(e)
			}
			switch mode {
			case "hash":
				p.Archives[0].SHA256 = strings.Repeat("0", 64)
			case "traversal":
				p.Archives[0].File = "../other.zip"
			case "identity":
				p.Configuration["HeyboxID"] = json.Number("123")
			case "character":
				a := p.Configuration["WTFList"].([]any)[0].(map[string]any)
				a["RealmCharacters"] = []any{map[string]any{"Realm": "Realm", "Player": "Other"}}
				a["Characters"] = []any{"Realm - Other"}
			case "missing":
				p.Archives = p.Archives[:3]
			}
			raw, _ = json.Marshal(p)
			os.WriteFile(file, raw, 0600)
			if _, e = bbLoadBackupPackage(context.Background(), file); e == nil {
				t.Fatal("tampered package accepted")
			}
		})
	}
}
func TestBlackboxBackupRejectsUnsafeSelection(t *testing.T) {
	for _, mode := range []string{"parent", "duplicate", "all-addons", "empty-character", "inside-output"} {
		t.Run(mode, func(t *testing.T) {
			in, root := bbBackupFixture(t)
			switch mode {
			case "parent":
				in["unknown_addons"] = []any{".."}
			case "duplicate":
				in["unknown_addons"] = []any{"Selected", "selected"}
			case "all-addons":
				in["materials"] = []any{"AddOns"}
			case "empty-character":
				in["wtf_accounts"] = []any{map[string]any{"name": "ACC", "characters": []any{}}}
			case "inside-output":
				in["output_dir"] = filepath.Join(root, "output")
			}
			if _, e := bbLocalBackup(context.Background(), "backup generate", in); e == nil {
				t.Fatal("unsafe selection accepted")
			}
		})
	}
}
