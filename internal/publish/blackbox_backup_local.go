package publish

import (
	"archive/zip"
	"context"
	"encoding/json"
	"io"
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"time"
)

const bbPackageSchema = "lycheedev.blackbox.backup-package.v1"

type bbBackupArchive struct {
	Field   string `json:"field"`
	Account string `json:"account_name,omitempty"`
	Type    string `json:"file_type"`
	File    string `json:"file"`
	Bytes   int64  `json:"bytes"`
	MD5     string `json:"md5"`
	SHA256  string `json:"sha256"`
	Local   string `json:"local_path,omitempty"`
	State   string `json:"state,omitempty"`
	Host    string `json:"host,omitempty"`
	Path    string `json:"path,omitempty"`
}
type bbBackupPackage struct {
	Schema        string            `json:"schema"`
	Configuration map[string]any    `json:"configuration"`
	Archives      []bbBackupArchive `json:"archives"`
	SourceBytes   int64             `json:"source_bytes"`
}
type bbBackupJob struct {
	Field, Type, File, Account, Base string
	Sources                          []string
	Entries                          []modusBackupEntry
	Files                            []any
	Characters                       []any
}

func bbBackupExcluded(p string) bool {
	n := strings.ToLower(filepath.Base(p))
	return n == "heyboxplayerinfo.lua" || n == "heyboxplayerinfo.lua.bak"
}
func bbBackupFiles(ctx context.Context, p string) ([]string, error) {
	a, e := modusBackupFiles(ctx, p)
	if e != nil {
		return nil, e
	}
	out := []string{}
	for _, f := range a {
		if !bbBackupExcluded(f) {
			out = append(out, f)
		}
	}
	return out, nil
}
func bbBackupChildren(root, rel string, files bool) ([]string, error) {
	p := filepath.Join(root, filepath.FromSlash(rel))
	if _, e := os.Stat(p); os.IsNotExist(e) {
		return []string{}, nil
	}
	if _, e := modusBackupPath(root, rel); e != nil {
		return nil, e
	}
	rows, e := os.ReadDir(p)
	if e != nil {
		return nil, e
	}
	out := []string{}
	for _, r := range rows {
		if _, e := modusBackupPath(root, rel+"/"+r.Name()); e != nil {
			return nil, e
		}
		if r.IsDir() != files {
			out = append(out, r.Name())
		}
	}
	return out, nil
}
func bbBackupCharacters(root, account string) ([]any, error) {
	base := "WTF/Account/" + account
	realms, e := bbBackupChildren(root, base, false)
	if e != nil {
		return nil, e
	}
	out := []any{}
	for _, realm := range realms {
		if strings.EqualFold(realm, "SavedVariables") {
			continue
		}
		players, e := bbBackupChildren(root, base+"/"+realm, false)
		if e != nil {
			return nil, e
		}
		for _, player := range players {
			files, e := bbBackupChildren(root, base+"/"+realm+"/"+player, true)
			if e != nil {
				return nil, e
			}
			for _, f := range files {
				switch strings.ToLower(f) {
				case "addons.txt", "cache.md5", "chat-cache.txt", "config-cache.wtf", "layout-local.txt":
					out = append(out, map[string]any{"Realm": realm, "Player": player})
					goto next
				}
			}
		next:
		}
	}
	return out, nil
}
func bbBackupKnown(v any) error {
	a, ok := v.([]any)
	if !ok {
		return invalid("$.known_addons", "requires an array")
	}
	seen := map[string]bool{}
	for _, v := range a {
		r, ok := v.(map[string]any)
		if !ok {
			return invalid("$.known_addons", "invalid addon brief")
		}
		id := bbID(r["ID"])
		n := bbID(r["Strategy"])
		if id == "" || String(r, "Name") == "" || bbID(r["VersionID"]) == "" || !strings.Contains("12345", n) || len(n) != 1 || seen[id] {
			return invalid("$.known_addons", "invalid or duplicated addon brief")
		}
		seen[id] = true
	}
	return nil
}
func bbLocalBackup(ctx context.Context, action string, in map[string]any) (any, error) {
	return bbLocalBackupMode(ctx, action, in, false)
}
func bbLocalBackupMode(ctx context.Context, action string, in map[string]any, plan bool) (any, error) {
	root, e := filepath.Abs(String(in, "game_dir"))
	if e != nil {
		return nil, e
	}
	if e = modusBackupNoLink(root); e != nil {
		return nil, e
	}
	st, e := os.Stat(root)
	if e != nil || !st.IsDir() {
		return nil, invalid("$.game_dir", "requires an existing directory")
	}
	branch := String(in, "strappid")
	if !regexp.MustCompile(`^wow[a-z0-9_]+$`).MatchString(branch) {
		return nil, invalid("$.strappid", "requires official WoW branch identity")
	}
	if action == "backup scan" {
		out := map[string]any{"game_dir": root, "strappid": branch, "addon_classification": "folder_candidates_without_remote_module_identification"}
		for _, s := range []struct {
			key, rel string
			files    bool
		}{{"unknown_addons", "Interface/AddOns", false}, {"materials", "Interface", false}, {"fonts", "Fonts", true}} {
			names, e := bbBackupChildren(root, s.rel, s.files)
			if e != nil {
				return nil, e
			}
			selected := []string{}
			for _, n := range names {
				if s.key == "materials" && strings.EqualFold(n, "AddOns") || s.key == "unknown_addons" && strings.EqualFold(n, "HeyboxPlayerInfo") || s.key == "fonts" && !strings.EqualFold(filepath.Ext(n), ".ttf") {
					continue
				}
				selected = append(selected, n)
			}
			out[s.key] = selected
		}
		names, e := bbBackupChildren(root, "WTF/Account", false)
		if e != nil {
			return nil, e
		}
		accounts := []any{}
		for _, n := range names {
			if strings.EqualFold(n, "SavedVariables") {
				continue
			}
			chars, e := bbBackupCharacters(root, n)
			if e != nil {
				return nil, e
			}
			if len(chars) > 0 {
				lower := []any{}
				for _, v := range chars {
					r := v.(map[string]any)
					lower = append(lower, map[string]any{"realm": r["Realm"], "player": r["Player"]})
				}
				accounts = append(accounts, map[string]any{"name": n, "characters": lower})
			}
		}
		out["wtf_accounts"] = accounts
		return out, nil
	}
	known := in["known_addons"]
	if known == nil {
		known = []any{}
	}
	if e = bbBackupKnown(known); e != nil {
		return nil, e
	}
	jobs := []bbBackupJob{}
	for _, s := range []struct{ key, field, kind, base, file string }{{"unknown_addons", "UnknownAddons", "unknownModules", "Interface/AddOns", "unknown.zip"}, {"materials", "Materials", "materials", "Interface", "materials.zip"}, {"fonts", "Fonts", "fonts", "Fonts", "fonts.zip"}} {
		names, ok := in[s.key].([]any)
		if !ok {
			return nil, invalid("$."+s.key, "explicit selection required")
		}
		job := bbBackupJob{Field: s.field, Type: s.kind, File: s.file, Base: filepath.Join(root, filepath.FromSlash(s.base))}
		seen := map[string]bool{}
		for _, v := range names {
			n, ok := v.(string)
			if !ok || !modusBackupComponent(n) || seen[strings.ToLower(n)] {
				return nil, invalid("$."+s.key, "invalid or duplicated name")
			}
			seen[strings.ToLower(n)] = true
			if s.key == "materials" && strings.EqualFold(n, "AddOns") || s.key == "unknown_addons" && strings.EqualFold(n, "HeyboxPlayerInfo") {
				return nil, invalid("$."+s.key, "excluded directory")
			}
			p, e := modusBackupPath(root, s.base+"/"+n)
			if e != nil {
				return nil, e
			}
			st, e := os.Stat(p)
			if e != nil || s.key == "fonts" && (!st.Mode().IsRegular() || !strings.EqualFold(filepath.Ext(n), ".ttf")) || s.key != "fonts" && !st.IsDir() {
				return nil, invalid("$."+s.key, "source type mismatch")
			}
			job.Sources = append(job.Sources, p)
		}
		if len(job.Sources) > 0 {
			jobs = append(jobs, job)
		}
	}
	accounts, ok := in["wtf_accounts"].([]any)
	if !ok {
		return nil, invalid("$.wtf_accounts", "explicit selection required")
	}
	seenAccounts := map[string]bool{}
	for _, v := range accounts {
		r := v.(map[string]any)
		name := String(r, "name")
		if !modusBackupComponent(name) || seenAccounts[strings.ToLower(name)] {
			return nil, invalid("$.wtf_accounts", "invalid or duplicate account")
		}
		seenAccounts[strings.ToLower(name)] = true
		source, e := modusBackupPath(root, "WTF/Account/"+name)
		if e != nil {
			return nil, e
		}
		available, e := bbBackupCharacters(root, name)
		if e != nil {
			return nil, e
		}
		valid := map[string]bool{}
		for _, v := range available {
			r := v.(map[string]any)
			valid[String(r, "Realm")+"/"+String(r, "Player")] = true
		}
		chars, ok := r["characters"].([]any)
		if !ok || len(chars) == 0 {
			return nil, invalid("$.wtf_accounts", "explicit characters required")
		}
		job := bbBackupJob{Field: "WTFList", Type: "wtf", File: "account_" + name + ".zip", Account: name, Base: filepath.Dir(source), Sources: []string{source}}
		seen := map[string]bool{}
		selected := []string{}
		for _, v := range chars {
			r := v.(map[string]any)
			realm, player := String(r, "realm"), String(r, "player")
			id := realm + "/" + player
			if !valid[id] || seen[id] {
				return nil, invalid("$.wtf_accounts", "unavailable or duplicate character")
			}
			seen[id] = true
			job.Characters = append(job.Characters, map[string]any{"Realm": realm, "Player": player})
			p, e := modusBackupPath(root, "WTF/Account/"+name+"/"+id)
			if e != nil {
				return nil, e
			}
			selected = append(selected, p)
		}
		children, e := os.ReadDir(source)
		if e != nil {
			return nil, e
		}
		for _, r := range children {
			if !r.IsDir() || strings.EqualFold(r.Name(), "SavedVariables") {
				p, e := modusBackupPath(root, "WTF/Account/"+name+"/"+r.Name())
				if e != nil {
					return nil, e
				}
				selected = append(selected, p)
			}
		}
		for _, p := range selected {
			files, e := bbBackupFiles(ctx, p)
			if e != nil {
				return nil, e
			}
			for _, f := range files {
				rel, _ := filepath.Rel(job.Base, f)
				job.Entries = append(job.Entries, modusBackupEntry{File: f, Name: filepath.ToSlash(rel)})
			}
		}
		jobs = append(jobs, job)
	}
	var total int64
	for i := range jobs {
		j := &jobs[i]
		if j.Field != "WTFList" {
			for _, s := range j.Sources {
				files, e := bbBackupFiles(ctx, s)
				if e != nil {
					return nil, e
				}
				var size int64
				for _, f := range files {
					st, e := os.Stat(f)
					if e != nil {
						return nil, e
					}
					size += st.Size()
					rel, _ := filepath.Rel(j.Base, f)
					j.Entries = append(j.Entries, modusBackupEntry{File: f, Name: filepath.ToSlash(rel)})
				}
				j.Files = append(j.Files, map[string]any{"Name": filepath.Base(s), "Size": size})
				total += size
			}
		} else {
			var size int64
			for _, f := range j.Entries {
				st, e := os.Stat(f.File)
				if e != nil {
					return nil, e
				}
				size += st.Size()
			}
			j.Files = []any{map[string]any{"Name": j.Account, "Size": size}}
			total += size
		}
		if len(j.Entries) == 0 {
			return nil, invalid("$", "selected category has no files")
		}
	}
	if len(jobs) == 0 && len(known.([]any)) == 0 {
		return nil, invalid("$", "at least one selection required")
	}
	output, e := filepath.Abs(String(in, "output_dir"))
	if e != nil {
		return nil, e
	}
	within, e := backupOutputWithin(root, output)
	if e != nil {
		return nil, e
	}
	if within {
		return nil, invalid("$.output_dir", "must be outside game directory")
	}
	if _, e = os.Stat(output); !os.IsNotExist(e) {
		return nil, invalid("$.output_dir", "must not already exist")
	}
	var strategies []any
	if s := String(in, "strategies_json"); s != "" {
		if e = json.Unmarshal([]byte(s), &strategies); e != nil {
			return nil, invalid("$.strategies_json", "invalid strategy array")
		}
		for _, v := range strategies {
			if _, ok := v.(map[string]any); !ok {
				return nil, invalid("$.strategies_json", "requires official strategy objects")
			}
		}
	}
	if plan {
		return map[string]any{"plan": true, "archive_count": len(jobs), "source_bytes": total}, nil
	}
	if e = os.MkdirAll(filepath.Dir(output), 0700); e != nil {
		return nil, e
	}
	if e = os.Mkdir(output, 0700); e != nil {
		return nil, e
	}
	p := bbBackupPackage{Schema: bbPackageSchema, Configuration: map[string]any{"HeyboxID": 0, "Name": "", "SaveTime": time.Now().Unix(), "IsClassic": branch != "wowretail", "Strappid": branch, "KnownAddons": known, "WTFList": []any{}}, Archives: []bbBackupArchive{}, SourceBytes: total}
	if len(strategies) > 0 {
		p.Configuration["Strategies"] = strategies
	}
	for _, j := range jobs {
		path := filepath.Join(output, j.File)
		if e = bbBackupZIP(ctx, path, j.Entries); e != nil {
			return nil, e
		}
		sha, md5, size, e := modusBackupHash(ctx, path)
		if e != nil {
			return nil, e
		}
		d := map[string]any{"Files": j.Files, "Size": size, "MD5": md5, "Host": "", "Path": ""}
		if j.Field == "WTFList" {
			names := []any{}
			for _, v := range j.Characters {
				r := v.(map[string]any)
				names = append(names, String(r, "Realm")+" - "+String(r, "Player"))
			}
			d["Name"] = j.Account
			d["Characters"] = names
			d["RealmCharacters"] = j.Characters
			p.Configuration["WTFList"] = append(p.Configuration["WTFList"].([]any), d)
		} else {
			p.Configuration[j.Field] = d
		}
		p.Archives = append(p.Archives, bbBackupArchive{Field: j.Field, Account: j.Account, Type: j.Type, File: j.File, Bytes: size, MD5: md5, SHA256: sha})
	}
	raw, e := json.MarshalIndent(p, "", "  ")
	if e != nil {
		return nil, e
	}
	file := filepath.Join(output, "backup.json")
	if e = os.WriteFile(file, raw, 0600); e != nil {
		return nil, e
	}
	return map[string]any{"created": true, "file": file, "strappid": branch, "archive_count": len(jobs), "source_bytes": total}, nil
}
func bbBackupZIP(ctx context.Context, path string, entries []modusBackupEntry) error {
	f, e := os.OpenFile(path, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0600)
	if e != nil {
		return e
	}
	defer f.Close()
	z := zip.NewWriter(f)
	for _, entry := range entries {
		if e = ctx.Err(); e != nil {
			break
		}
		source, err := openArtifact(entry.File)
		if err != nil {
			e = err
			break
		}
		before, err := source.Stat()
		if err != nil {
			source.Close()
			e = err
			break
		}
		target, err := z.CreateHeader(&zip.FileHeader{Name: entry.Name, Method: zip.Deflate})
		if err == nil {
			_, err = io.Copy(target, source)
		}
		after, statErr := source.Stat()
		source.Close()
		if err != nil {
			e = err
			break
		}
		if statErr != nil || before.Size() != after.Size() || !before.ModTime().Equal(after.ModTime()) {
			e = failure("publish.blackbox_source_changed", "backup source changed while packaging")
			break
		}
	}
	closeErr := z.Close()
	if e != nil {
		return e
	}
	if closeErr != nil {
		return closeErr
	}
	return f.Sync()
}
