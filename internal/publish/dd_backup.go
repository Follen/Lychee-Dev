package publish

import (
	"archive/zip"
	"context"
	"crypto/md5"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"io"
	"io/fs"
	"os"
	"path/filepath"
	"strings"
)

var ddBackupGroups = []string{"known_addon", "unknown_addon", "wtf", "material", "font"}

type ddBackupArtifact struct {
	Group   string   `json:"group"`
	File    string   `json:"file"`
	Size    int64    `json:"size"`
	MD5     string   `json:"md5"`
	SHA256  string   `json:"sha256"`
	Members []string `json:"members"`
}
type ddBackupBundle struct {
	Schema    string             `json:"schema"`
	Manifest  map[string]any     `json:"manifest"`
	Artifacts []ddBackupArtifact `json:"artifacts"`
	Selection map[string]any     `json:"selection"`
}

func ddName(name string) bool {
	if name == "" || name == "." || name == ".." || strings.TrimRight(name, " .") != name {
		return false
	}
	for _, c := range name {
		if c < 32 || c == 127 || strings.ContainsRune(`/\:<>"|?*`, c) {
			return false
		}
	}
	stem := strings.ToUpper(strings.SplitN(name, ".", 2)[0])
	if stem == "CON" || stem == "PRN" || stem == "AUX" || stem == "NUL" || len(stem) == 4 && (strings.HasPrefix(stem, "COM") || strings.HasPrefix(stem, "LPT")) && stem[3] >= '1' && stem[3] <= '9' {
		return false
	}
	return true
}
func ddBackupPath(root string, parts ...string) (string, error) {
	for _, p := range parts {
		if !ddName(p) {
			return "", invalid("$.root", "invalid backup path component")
		}
	}
	full := filepath.Join(append([]string{root}, parts...)...)
	for p := full; ; p = filepath.Dir(p) {
		info, err := os.Lstat(p)
		if err != nil {
			return "", err
		}
		if info.Mode()&os.ModeSymlink != 0 {
			return "", invalid("$.root", "backup source contains a link")
		}
		if p == root {
			break
		}
		if filepath.Dir(p) == p {
			return "", invalid("$.root", "backup path escaped its root")
		}
	}
	return full, nil
}
func ddEntries(path string) []os.DirEntry { a, _ := os.ReadDir(path); return a }
func ddBackupScan(root string) (map[string]any, error) {
	root, err := filepath.Abs(root)
	if err != nil {
		return nil, err
	}
	if _, err = ddBackupPath(root); err != nil {
		return nil, err
	}
	out := map[string]any{"root": root}
	for _, item := range []struct {
		key   string
		parts []string
	}{{"addon_names", []string{"Interface", "AddOns"}}, {"materials", []string{"Interface"}}, {"fonts", []string{"Fonts"}}} {
		rows := []any{}
		for _, entry := range ddEntries(filepath.Join(append([]string{root}, item.parts...)...)) {
			if _, err = ddBackupPath(root, append(item.parts, entry.Name())...); err != nil {
				return nil, err
			}
			if item.key == "addon_names" && !entry.IsDir() || item.key == "materials" && strings.EqualFold(entry.Name(), "AddOns") || item.key == "fonts" && (entry.IsDir() || !strings.EqualFold(filepath.Ext(entry.Name()), ".ttf")) {
				continue
			}
			rows = append(rows, map[string]any{"name": entry.Name()})
		}
		out[item.key] = rows
	}
	out["unknown_addons"] = out["addon_names"]
	accounts := []any{}
	for _, a := range ddEntries(filepath.Join(root, "WTF", "Account")) {
		if !a.IsDir() {
			continue
		}
		if _, err = ddBackupPath(root, "WTF", "Account", a.Name()); err != nil {
			return nil, err
		}
		roles := []any{}
		common := false
		for _, server := range ddEntries(filepath.Join(root, "WTF", "Account", a.Name())) {
			if !server.IsDir() || strings.EqualFold(server.Name(), "SavedVariables") {
				common = true
				continue
			}
			for _, role := range ddEntries(filepath.Join(root, "WTF", "Account", a.Name(), server.Name())) {
				if !role.IsDir() || strings.EqualFold(role.Name(), "SavedVariables") {
					continue
				}
				if _, err = ddBackupPath(root, "WTF", "Account", a.Name(), server.Name(), role.Name()); err != nil {
					return nil, err
				}
				roles = append(roles, map[string]any{"server": server.Name(), "name": role.Name()})
			}
		}
		accounts = append(accounts, map[string]any{"account": a.Name(), "roles": roles, "has_common": common})
	}
	out["wtf_accounts"] = accounts
	return out, nil
}
func ddBackupZip(ctx context.Context, root, base, destination string, sources []string) (ddBackupArtifact, error) {
	var result ddBackupArtifact
	f, err := os.OpenFile(destination, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0600)
	if err != nil {
		return result, err
	}
	z := zip.NewWriter(f)
	seen := map[string]bool{}
	members := []string{}
	write := func(path string, info fs.FileInfo) error {
		if err := ctx.Err(); err != nil {
			return err
		}
		rel, e := filepath.Rel(root, path)
		if e != nil {
			return e
		}
		if _, e = ddBackupPath(root, strings.Split(rel, string(filepath.Separator))...); e != nil {
			return e
		}
		if info.IsDir() {
			return nil
		}
		if !info.Mode().IsRegular() {
			return invalid("$.root", "backup contains a nonregular file")
		}
		name, e := filepath.Rel(base, path)
		if e != nil {
			return e
		}
		name = filepath.ToSlash(name)
		key := strings.ToLower(name)
		if seen[key] {
			return nil
		}
		seen[key] = true
		in, e := openArtifact(path)
		if e != nil {
			return e
		}
		defer in.Close()
		header, e := zip.FileInfoHeader(info)
		if e != nil {
			return e
		}
		header.Name = name
		header.Method = zip.Deflate
		out, e := z.CreateHeader(header)
		if e != nil {
			return e
		}
		_, e = io.Copy(out, in)
		if e == nil {
			members = append(members, name)
		}
		return e
	}
	for _, source := range sources {
		err = filepath.Walk(source, func(path string, info fs.FileInfo, e error) error {
			if e != nil {
				return e
			}
			return write(path, info)
		})
		if err != nil {
			break
		}
	}
	ze := z.Close()
	fe := f.Close()
	if err == nil {
		err = ze
	}
	if err == nil {
		err = fe
	}
	if err != nil {
		return result, err
	}
	read, err := openArtifact(destination)
	if err != nil {
		return result, err
	}
	defer read.Close()
	m, s := md5.New(), sha256.New()
	size, err := io.Copy(io.MultiWriter(m, s), read)
	if err != nil {
		return result, err
	}
	return ddBackupArtifact{File: filepath.Base(destination), Size: size, MD5: hex.EncodeToString(m.Sum(nil)), SHA256: hex.EncodeToString(s.Sum(nil)), Members: members}, nil
}
func ddBackupGenerate(ctx context.Context, in map[string]any) (any, error) {
	root, err := filepath.Abs(String(in, "root"))
	if err != nil {
		return nil, err
	}
	root, err = backupResolvedPath(root)
	if err != nil {
		return nil, err
	}
	available, err := ddBackupScan(root)
	if err != nil {
		return nil, err
	}
	output, err := filepath.Abs(String(in, "output"))
	if err != nil {
		return nil, err
	}
	ancestor := output
	for {
		if _, e := os.Lstat(ancestor); e == nil {
			break
		} else if !os.IsNotExist(e) {
			return nil, e
		}
		parent := filepath.Dir(ancestor)
		if parent == ancestor {
			return nil, invalid("$.output", "output has no existing ancestor")
		}
		ancestor = parent
	}
	suffix, e := filepath.Rel(ancestor, output)
	if e != nil {
		return nil, e
	}
	canonical, e := backupResolvedPath(ancestor)
	if e != nil {
		return nil, e
	}
	output = filepath.Join(canonical, suffix)
	if !backupPathOutside(root, output) {
		return nil, invalid("$.output", "backup output must be outside game directory")
	}
	if info, e := os.Lstat(output); e == nil {
		if !info.IsDir() || info.Mode()&os.ModeSymlink != 0 || len(ddEntries(output)) > 0 {
			return nil, invalid("$.output", "output must be an empty directory")
		}
	} else if !os.IsNotExist(e) {
		return nil, e
	}
	selected := map[string][]string{}
	for _, key := range []string{"addon_names", "unknown_addons", "materials", "fonts"} {
		allowed := map[string]bool{}
		for _, v := range ddSlice(available[key]) {
			allowed[ddText(ddMap(v)["name"])] = true
		}
		seen := map[string]bool{}
		for _, v := range ddSlice(in[key]) {
			s := ddText(v)
			if !ddName(s) || !allowed[s] || seen[strings.ToLower(s)] {
				return nil, invalid("$."+key, "selection contains duplicate or unavailable content")
			}
			seen[strings.ToLower(s)] = true
			selected[key] = append(selected[key], s)
		}
	}
	for _, a := range selected["addon_names"] {
		for _, b := range selected["unknown_addons"] {
			if strings.EqualFold(a, b) {
				return nil, invalid("$.unknown_addons", "addon belongs to both groups")
			}
		}
	}
	wtfSources := []string{}
	if Bool(in, "include_wtf_root") {
		for _, e := range ddEntries(filepath.Join(root, "WTF")) {
			if !e.IsDir() {
				p, err := ddBackupPath(root, "WTF", e.Name())
				if err != nil {
					return nil, err
				}
				wtfSources = append(wtfSources, p)
			}
		}
	}
	accountMetadata := []any{}
	accountsSeen := map[string]bool{}
	for _, choice := range ddSlice(in["wtf_accounts"]) {
		c := ddMap(choice)
		name := ddText(c["account"])
		if !ddName(name) || accountsSeen[strings.ToLower(name)] {
			return nil, invalid("$.wtf_accounts", "duplicate or invalid account")
		}
		accountsSeen[strings.ToLower(name)] = true
		account, err := ddBackupPath(root, "WTF", "Account", name)
		if err != nil {
			return nil, err
		}
		common := Bool(c, "include_common")
		before := len(wtfSources)
		if common {
			for _, e := range ddEntries(account) {
				if !e.IsDir() || strings.EqualFold(e.Name(), "SavedVariables") {
					wtfSources = append(wtfSources, filepath.Join(account, e.Name()))
				}
			}
		}
		servers := []any{}
		serverMap := map[string]map[string]any{}
		roleSeen := map[string]bool{}
		for _, role := range ddSlice(c["roles"]) {
			r := ddMap(role)
			server, n := ddText(r["server"]), ddText(r["name"])
			key := strings.ToLower(server + "\x00" + n)
			if roleSeen[key] || strings.EqualFold(server, "SavedVariables") || strings.EqualFold(n, "SavedVariables") {
				return nil, invalid("$.wtf_accounts", "duplicate or invalid role")
			}
			roleSeen[key] = true
			p, e := ddBackupPath(root, "WTF", "Account", name, server, n)
			if e != nil {
				return nil, e
			}
			wtfSources = append(wtfSources, p)
			row := serverMap[server]
			if row == nil {
				row = map[string]any{"name": server, "items": []any{}}
				servers = append(servers, row)
				serverMap[server] = row
				if common {
					for _, e := range ddEntries(filepath.Join(account, server)) {
						if !e.IsDir() || strings.EqualFold(e.Name(), "SavedVariables") {
							wtfSources = append(wtfSources, filepath.Join(account, server, e.Name()))
						}
					}
				}
			}
			row["items"] = append(ddSlice(row["items"]), n)
		}
		if len(wtfSources) == before {
			return nil, invalid("$.wtf_accounts", "account selection contains no files or roles")
		}
		accountMetadata = append(accountMetadata, map[string]any{"name": name, "servers": servers, "wa_list": []any{}})
	}
	if len(selected["addon_names"])+len(selected["unknown_addons"])+len(selected["materials"])+len(selected["fonts"])+len(wtfSources) == 0 {
		return nil, invalid("$", "select at least one backup component")
	}
	if err = os.MkdirAll(output, 0700); err != nil {
		return nil, err
	}
	bundle := ddBackupBundle{Schema: "lycheedev.publish.dd-backup.v1", Manifest: map[string]any{"game_type": in["game_type"]}, Artifacts: []ddBackupArtifact{}, Selection: map[string]any{"include_wtf_root": Bool(in, "include_wtf_root"), "wtf_accounts": ddSlice(in["wtf_accounts"])}}
	for _, group := range ddBackupGroups {
		field := "items"
		if group == "wtf" {
			field = "accounts"
		}
		bundle.Manifest[group] = map[string]any{"md5": "", "size": 0, field: []any{}}
	}
	for _, spec := range []struct {
		group, key string
		parts      []string
	}{{"known_addon", "addon_names", []string{"Interface", "AddOns"}}, {"unknown_addon", "unknown_addons", []string{"Interface", "AddOns"}}, {"material", "materials", []string{"Interface"}}, {"font", "fonts", []string{"Fonts"}}, {"wtf", "", nil}} {
		sources := []string{}
		items := []any{}
		base := filepath.Join(append([]string{root}, spec.parts...)...)
		for _, name := range selected[spec.key] {
			p, e := ddBackupPath(root, append(spec.parts, name)...)
			if e != nil {
				return nil, e
			}
			sources = append(sources, p)
			var item any = name
			if spec.group == "known_addon" {
				item = map[string]any{"name": name, "modified": true, "dirs": []any{name}}
			}
			if spec.group == "material" {
				item = map[string]any{"name": name, "desc": ""}
			}
			items = append(items, item)
		}
		field := "items"
		if spec.group == "font" {
			base = root
		}
		if spec.group == "wtf" {
			sources = wtfSources
			base = root
			items = accountMetadata
			field = "accounts"
		}
		if len(sources) == 0 {
			continue
		}
		artifact, e := ddBackupZip(ctx, root, base, filepath.Join(output, spec.group+".zip"), sources)
		if e != nil {
			return nil, e
		}
		artifact.Group = spec.group
		bundle.Artifacts = append(bundle.Artifacts, artifact)
		bundle.Manifest[spec.group] = map[string]any{"md5": artifact.MD5, "size": artifact.Size, field: items}
	}
	b, err := json.MarshalIndent(bundle, "", "  ")
	if err != nil {
		return nil, err
	}
	p := filepath.Join(output, "backup-bundle.json")
	f, err := os.OpenFile(p, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0600)
	if err != nil {
		return nil, err
	}
	_, err = f.Write(b)
	ce := f.Close()
	if err == nil {
		err = ce
	}
	if err != nil {
		return nil, err
	}
	return map[string]any{"bundle_file": p, "artifacts": bundle.Artifacts, "game_type": in["game_type"]}, nil
}
