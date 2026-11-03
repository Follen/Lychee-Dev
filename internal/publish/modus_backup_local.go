package publish

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"io"
	"io/fs"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"time"
)

var modusBackupSections = []string{"addons", "modifiedAddons", "unknownAddons", "fonts", "materials", "wtf"}

type modusBackupCopy struct {
	Section, Name, Relative string
	Meta                    map[string]any
	Files                   []string
	Directory               bool
}

func modusBackupComponent(name string) bool {
	if name == "" || name == "." || name == ".." || strings.ContainsAny(name, "\\/:*?\"<>|,\x00\r\n") || strings.TrimRight(name, " .") != name {
		return false
	}
	for _, r := range name {
		if r < 32 {
			return false
		}
	}
	return true
}
func modusBackupPath(root, relative string) (string, error) {
	if relative == "" || strings.ContainsAny(relative, "\\:\x00") || strings.HasPrefix(relative, "/") {
		return "", invalid("$.manifest_file", "invalid backup relative path")
	}
	for _, part := range strings.Split(relative, "/") {
		if !modusBackupComponent(part) {
			return "", invalid("$.manifest_file", "invalid backup path component")
		}
	}
	target := filepath.Join(root, filepath.FromSlash(relative))
	p := target
	for {
		if err := modusBackupNoLink(p); err != nil {
			return "", err
		}
		if strings.EqualFold(p, root) {
			break
		}
		parent := filepath.Dir(p)
		if parent == p {
			return "", invalid("$.manifest_file", "backup target escapes root")
		}
		p = parent
	}
	return target, nil
}
func modusBackupFiles(ctx context.Context, source string) ([]string, error) {
	if err := modusBackupNoLink(source); err != nil {
		return nil, err
	}
	stat, err := os.Stat(source)
	if err != nil {
		return nil, invalid("$.game_root", "selected backup source is unavailable")
	}
	if stat.Mode().IsRegular() {
		return []string{source}, nil
	}
	if !stat.IsDir() {
		return nil, invalid("$.game_root", "backup source must be a regular file or directory")
	}
	files := []string{}
	err = filepath.WalkDir(source, func(p string, d fs.DirEntry, e error) error {
		if e != nil {
			return e
		}
		if err := ctx.Err(); err != nil {
			return err
		}
		if err := modusBackupNoLink(p); err != nil {
			return err
		}
		if d.IsDir() {
			return nil
		}
		info, err := d.Info()
		if err != nil {
			return err
		}
		if !info.Mode().IsRegular() {
			return invalid("$.game_root", "backup contains a nonregular file")
		}
		files = append(files, p)
		return nil
	})
	sort.Strings(files)
	return files, err
}
func modusBackupWTFFile(p string) bool {
	ext := strings.ToLower(filepath.Ext(p))
	return (ext == ".lua" || ext == ".wtf" || ext == ".txt") && !strings.EqualFold(filepath.Base(p), "BigFootSync.lua")
}
func modusBackupNames(parent string, dirsOnly bool) ([]string, error) {
	rows, err := os.ReadDir(parent)
	if os.IsNotExist(err) {
		return []string{}, nil
	}
	if err != nil {
		return nil, err
	}
	names := []string{}
	for _, r := range rows {
		p := filepath.Join(parent, r.Name())
		if modusBackupNoLink(p) != nil {
			continue
		}
		if !dirsOnly || r.IsDir() {
			names = append(names, r.Name())
		}
	}
	sort.Strings(names)
	return names, nil
}
func scanModusBackup(ctx context.Context, in map[string]any) (any, error) {
	root, err := filepath.Abs(String(in, "game_root"))
	if err != nil {
		return nil, err
	}
	if _, err = modusBackupPath(root, "Interface/AddOns"); err != nil {
		return nil, err
	}
	stat, err := os.Stat(filepath.Join(root, "Interface", "AddOns"))
	if err != nil || !stat.IsDir() {
		return nil, invalid("$.game_root", "selected branch must contain Interface/AddOns")
	}
	addons, err := modusBackupNames(filepath.Join(root, "Interface", "AddOns"), true)
	if err != nil {
		return nil, err
	}
	materials, err := modusBackupNames(filepath.Join(root, "Interface"), false)
	if err != nil {
		return nil, err
	}
	filtered := []string{}
	for _, name := range materials {
		if !strings.EqualFold(name, "AddOns") {
			filtered = append(filtered, name)
		}
	}
	fonts, err := modusBackupNames(filepath.Join(root, "Fonts"), false)
	if err != nil {
		return nil, err
	}
	fontFiles := []string{}
	for _, name := range fonts {
		info, err := os.Stat(filepath.Join(root, "Fonts", name))
		if err == nil && info.Mode().IsRegular() {
			fontFiles = append(fontFiles, name)
		}
	}
	roles := []map[string]any{}
	accounts, err := modusBackupNames(filepath.Join(root, "WTF", "Account"), true)
	if err != nil {
		return nil, err
	}
	for _, account := range accounts {
		servers, err := modusBackupNames(filepath.Join(root, "WTF", "Account", account), true)
		if err != nil {
			return nil, err
		}
		for _, server := range servers {
			if strings.EqualFold(server, "SavedVariables") {
				continue
			}
			chars, err := modusBackupNames(filepath.Join(root, "WTF", "Account", account, server), true)
			if err != nil {
				return nil, err
			}
			for _, char := range chars {
				if strings.EqualFold(char, "SavedVariables") {
					continue
				}
				if err := ctx.Err(); err != nil {
					return nil, err
				}
				roles = append(roles, map[string]any{"account": account, "server": server, "role": char})
			}
		}
	}
	return map[string]any{"server_type": in["server_type"], "game_root": root, "addon_names": addons, "font_names": fontFiles, "material_names": filtered, "roles": roles}, nil
}
func planModusBackup(ctx context.Context, in map[string]any) (string, string, []modusBackupCopy, map[string][]map[string]string, error) {
	root, err := filepath.Abs(String(in, "game_root"))
	if err != nil {
		return "", "", nil, nil, err
	}
	destination, err := filepath.Abs(String(in, "output_dir"))
	if err != nil {
		return "", "", nil, nil, err
	}
	if !backupPathOutside(root, destination) {
		return "", "", nil, nil, invalid("$.output_dir", "output must be outside game_root")
	}
	if _, err = os.Stat(destination); !os.IsNotExist(err) {
		return "", "", nil, nil, invalid("$.output_dir", "output directory must not already exist")
	}
	within, err := backupOutputWithin(root, destination)
	if err != nil {
		return "", "", nil, nil, err
	}
	if within {
		return "", "", nil, nil, invalid("$.output_dir", "resolved output must be outside selected game branch")
	}
	if _, err = scanModusBackup(ctx, in); err != nil {
		return "", "", nil, nil, err
	}
	plans := []modusBackupCopy{}
	accounts := map[string][]map[string]string{}
	for _, selection := range []struct{ Field, Section, Prefix string }{{"addon_names", "unknownAddons", "Interface/AddOns"}, {"font_names", "fonts", "Fonts"}, {"material_names", "materials", "Interface"}} {
		values, _ := in[selection.Field].([]any)
		seen := map[string]bool{}
		for _, v := range values {
			name := fmtString(v)
			norm := strings.ToLower(name)
			if !modusBackupComponent(name) || seen[norm] {
				return "", "", nil, nil, invalid("$."+selection.Field, "select unique single directory/file names")
			}
			seen[norm] = true
			if selection.Field == "material_names" && strings.EqualFold(name, "AddOns") {
				return "", "", nil, nil, invalid("$.material_names", "select addons through addon_names")
			}
			if selection.Field == "font_names" && !strings.EqualFold(filepath.Ext(name), ".ttf") && !strings.EqualFold(filepath.Ext(name), ".otf") {
				return "", "", nil, nil, invalid("$.font_names", "select TTF or OTF files")
			}
			plans = append(plans, modusBackupCopy{Section: selection.Section, Name: name, Relative: selection.Prefix + "/" + name, Meta: map[string]any{}})
		}
	}
	roles, _ := in["roles"].([]any)
	seenRoles := map[string]bool{}
	accountSpelling := map[string]string{}
	for _, v := range roles {
		role := v.(map[string]any)
		account, server, char := String(role, "account"), String(role, "server"), String(role, "role")
		if !modusBackupComponent(account) || !modusBackupComponent(server) || !modusBackupComponent(char) || strings.EqualFold(account, "SavedVariables") || strings.EqualFold(server, "SavedVariables") || strings.EqualFold(char, "SavedVariables") {
			return "", "", nil, nil, invalid("$.roles", "roles require exact account/server/character directory names")
		}
		key := strings.ToLower(account + "/" + server + "/" + char)
		if seenRoles[key] {
			return "", "", nil, nil, invalid("$.roles", "duplicate role")
		}
		seenRoles[key] = true
		if old := accountSpelling[strings.ToLower(account)]; old != "" && old != account {
			return "", "", nil, nil, invalid("$.roles", "account spelling must remain consistent")
		}
		accountSpelling[strings.ToLower(account)] = account
		accounts[account] = append(accounts[account], map[string]string{"server": server, "role": char})
		plans = append(plans, modusBackupCopy{Section: "wtf", Name: char, Relative: "WTF/Account/" + account + "/" + server + "/" + char, Meta: map[string]any{"type": "role", "account": account, "server": server}})
	}
	accountNames := []string{}
	for account := range accounts {
		accountNames = append(accountNames, account)
	}
	sort.Strings(accountNames)
	for _, account := range accountNames {
		relative := "WTF/Account/" + account + "/SavedVariables"
		if info, err := os.Stat(filepath.Join(root, filepath.FromSlash(relative))); err == nil && info.IsDir() {
			plans = append(plans, modusBackupCopy{Section: "wtf", Name: account + " SavedVariables", Relative: relative, Meta: map[string]any{"type": "account-shared", "account": account}})
		}
	}
	if len(accounts) > 0 {
		if info, err := os.Stat(filepath.Join(root, "WTF", "Config.wtf")); err == nil && info.Mode().IsRegular() {
			plans = append(plans, modusBackupCopy{Section: "wtf", Name: "Config.wtf", Relative: "WTF/Config.wtf", Meta: map[string]any{"type": "config"}})
		}
	}
	if len(plans) == 0 {
		return "", "", nil, nil, invalid("$", "select at least one addon, font, material or WTF role")
	}
	for i := range plans {
		p := &plans[i]
		source, err := modusBackupPath(root, p.Relative)
		if err != nil {
			return "", "", nil, nil, err
		}
		stat, err := os.Stat(source)
		if err != nil {
			return "", "", nil, nil, err
		}
		p.Directory = stat.IsDir()
		if p.Section == "fonts" && p.Directory || p.Meta["type"] == "role" && !p.Directory {
			return "", "", nil, nil, invalid("$", "selected backup path has the wrong type")
		}
		files, err := modusBackupFiles(ctx, source)
		if err != nil {
			return "", "", nil, nil, err
		}
		for _, file := range files {
			if p.Section != "wtf" || modusBackupWTFFile(file) {
				p.Files = append(p.Files, file)
			}
		}
	}
	return root, destination, plans, accounts, nil
}
func generateModusBackup(ctx context.Context, in map[string]any, plan bool) (any, error) {
	root, destination, plans, accounts, err := planModusBackup(ctx, in)
	if err != nil {
		return nil, err
	}
	if plan {
		count := 0
		for _, p := range plans {
			count += len(p.Files)
		}
		return map[string]any{"plan": true, "files": count, "output_dir": destination, "remoteChecked": false}, nil
	}
	if err = os.MkdirAll(filepath.Dir(destination), 0700); err != nil {
		return nil, err
	}
	if err = os.Mkdir(destination, 0700); err != nil {
		return nil, err
	}
	if request := String(in, "__request"); request != "" {
		raw, _ := json.Marshal(map[string]string{"request": request, "inputDigest": String(in, "__input_digest")})
		if err = os.WriteFile(filepath.Join(destination, ".lycheedev-create.json"), raw, 0600); err != nil {
			return nil, err
		}
	}
	sections := map[string]map[string]any{}
	for _, key := range modusBackupSections {
		sections[key] = map[string]any{"key": key, "title": key, "count": 0, "sizeBytes": int64(0), "items": []any{}}
	}
	for _, p := range plans {
		size := int64(0)
		for _, file := range p.Files {
			if err := ctx.Err(); err != nil {
				return nil, err
			}
			rel, _ := filepath.Rel(root, file)
			target := filepath.Join(destination, rel)
			if err = os.MkdirAll(filepath.Dir(target), 0700); err != nil {
				return nil, err
			}
			source, err := openArtifact(file)
			if err != nil {
				return nil, err
			}
			out, err := os.OpenFile(target, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0600)
			if err != nil {
				source.Close()
				return nil, err
			}
			n, copyErr := io.Copy(out, source)
			source.Close()
			closeErr := out.Close()
			if copyErr != nil {
				return nil, copyErr
			}
			if closeErr != nil {
				return nil, closeErr
			}
			size += n
		}
		// Empty selected directories still need to exist for manifest binding.
		if p.Directory {
			if err = os.MkdirAll(filepath.Join(destination, filepath.FromSlash(p.Relative)), 0700); err != nil {
				return nil, err
			}
		}
		if p.Section == "fonts" {
			_, digest, _, err := modusBackupHash(ctx, filepath.Join(destination, filepath.FromSlash(p.Relative)))
			if err != nil {
				return nil, err
			}
			p.Meta["md5"] = digest
		}
		item := map[string]any{"id": p.Section + ":" + p.Relative, "section": p.Section, "name": p.Name, "targetRelativePath": p.Relative, "sizeBytes": size, "meta": p.Meta}
		sections[p.Section]["items"] = append(sections[p.Section]["items"].([]any), item)
	}
	leaves := sections["wtf"]["items"].([]any)
	wtf := []any{}
	for _, v := range leaves {
		item := v.(map[string]any)
		if item["meta"].(map[string]any)["type"] == "config" {
			wtf = append(wtf, item)
		}
	}
	accountNames := []string{}
	for account := range accounts {
		accountNames = append(accountNames, account)
	}
	sort.Strings(accountNames)
	for _, account := range accountNames {
		serverNames := []string{}
		seen := map[string]bool{}
		for _, r := range accounts[account] {
			if !seen[r["server"]] {
				serverNames = append(serverNames, r["server"])
				seen[r["server"]] = true
			}
		}
		sort.Strings(serverNames)
		servers := []any{}
		accountSize := int64(0)
		for _, v := range leaves {
			item := v.(map[string]any)
			if item["meta"].(map[string]any)["account"] == account {
				accountSize += item["sizeBytes"].(int64)
			}
		}
		for _, server := range serverNames {
			children := []any{}
			size := int64(0)
			for _, v := range leaves {
				item := v.(map[string]any)
				meta := item["meta"].(map[string]any)
				if meta["account"] == account && meta["server"] == server {
					children = append(children, item)
					size += item["sizeBytes"].(int64)
				}
			}
			servers = append(servers, map[string]any{"id": "wtf:server:" + account + ":" + server, "section": "wtf", "name": server, "targetRelativePath": "WTF/Account/" + account + "/" + server, "sizeBytes": size, "children": children, "meta": map[string]any{"type": "server"}})
		}
		wtf = append(wtf, map[string]any{"id": "wtf:account:" + account, "section": "wtf", "name": account, "targetRelativePath": "WTF/Account/" + account, "sizeBytes": accountSize, "children": servers, "meta": map[string]any{"type": "account"}})
	}
	sections["wtf"]["items"] = wtf
	sectionList := []any{}
	totals := map[string]any{}
	totalSize := int64(0)
	totalCount := 0
	for _, key := range modusBackupSections {
		section := sections[key]
		items := section["items"].([]any)
		size := int64(0)
		for _, v := range items {
			size += v.(map[string]any)["sizeBytes"].(int64)
		}
		section["count"] = len(items)
		section["sizeBytes"] = size
		sectionList = append(sectionList, section)
		if key != "wtf" {
			totals[key] = len(items)
		}
		totalSize += size
		totalCount += len(items)
	}
	totals["wtfAccounts"] = len(accounts)
	totals["files"] = totalCount
	totals["sizeBytes"] = totalSize
	nonce := make([]byte, 8)
	if _, err = rand.Read(nonce); err != nil {
		return nil, err
	}
	now := time.Now().UTC().Format(time.RFC3339Nano)
	id := time.Now().UTC().Format("20060102150405") + "-" + hex.EncodeToString(nonce)
	manifest := map[string]any{"schemaVersion": "config-backup.v2", "backupId": id, "backupName": in["backup_name"], "createdAt": now, "completedAt": now, "storageTargets": []string{"local"}, "source": map[string]any{"origin": "modus", "gameId": in["server_type"]}, "sections": sectionList, "totals": totals}
	raw, err := json.MarshalIndent(manifest, "", "  ")
	if err != nil {
		return nil, err
	}
	filename := filepath.Join(destination, "manifest.v2.json")
	if err = os.WriteFile(filename, raw, 0600); err != nil {
		return nil, err
	}
	return map[string]any{"local_backup_id": id, "manifest_file": filename, "output_dir": destination, "server_type": in["server_type"], "totals": totals, "generated": true}, nil
}
