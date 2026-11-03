package publish

import (
	"archive/zip"
	"bytes"
	"context"
	"crypto/md5"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"io"
	"os"
	"path/filepath"
	"sort"
	"strings"
)

type modusBackupEntry struct{ File, Name string }
type modusBackupLocal struct {
	Section, Account string
	Roles            []string
	Entries          []modusBackupEntry
	Direct           string
	Type             int
	ContentType      string
}

func modusBackupHash(ctx context.Context, filename string) (string, string, int64, error) {
	f, err := openArtifact(filename)
	if err != nil {
		return "", "", 0, err
	}
	defer f.Close()
	stat, err := f.Stat()
	if err != nil || !stat.Mode().IsRegular() {
		return "", "", 0, invalid("$.manifest_file", "artifact is not a regular file")
	}
	h, m := sha256.New(), md5.New()
	buf := make([]byte, 128<<10)
	var total int64
	for {
		if err = ctx.Err(); err != nil {
			return "", "", 0, err
		}
		n, e := f.Read(buf)
		if n > 0 {
			h.Write(buf[:n])
			m.Write(buf[:n])
			total += int64(n)
		}
		if e == io.EOF {
			break
		}
		if e != nil {
			return "", "", 0, e
		}
	}
	return hex.EncodeToString(h.Sum(nil)), hex.EncodeToString(m.Sum(nil)), total, nil
}
func modusBackupObjectMap(v any) (map[string]any, error) {
	m, ok := v.(map[string]any)
	if !ok {
		return nil, invalid("$.manifest_file", "manifest item must be an object")
	}
	return m, nil
}
func loadModusBackupManifest(ctx context.Context, filename string, build int64) (map[string]any, []modusBackupLocal, error) {
	filename, err := filepath.Abs(filename)
	if err != nil {
		return nil, nil, err
	}
	root := filepath.Dir(filename)
	if err = modusBackupNoLink(filename); err != nil {
		return nil, nil, err
	}
	raw, err := modusReadBounded(filename, 4<<20)
	if err != nil {
		return nil, nil, invalid("$.manifest_file", "cannot read bounded backup manifest")
	}
	d := json.NewDecoder(bytes.NewReader(raw))
	d.UseNumber()
	count := 0
	value, err := decodeValue(d, "$", 0, &count)
	if err != nil || d.Decode(new(any)) != io.EOF {
		return nil, nil, invalid("$.manifest_file", "invalid manifest JSON")
	}
	manifest, err := modusBackupObjectMap(value)
	if err != nil {
		return nil, nil, err
	}
	source, _ := manifest["source"].(map[string]any)
	if manifest["schemaVersion"] != "config-backup.v2" || Integer(source, "gameId") != build || source["gameId"] == nil {
		return nil, nil, invalid("$.manifest_file", "manifest schema or source Build does not match")
	}
	if source["cloudBackupId"] != nil {
		return nil, nil, invalid("$.manifest_file", "backup is already linked to cloud")
	}
	storage, ok := manifest["storageTargets"].([]any)
	if !ok {
		return nil, nil, invalid("$.manifest_file", "manifest storageTargets must be an array")
	}
	for _, v := range storage {
		if v == "cloud" {
			return nil, nil, invalid("$.manifest_file", "backup is already linked to cloud")
		}
	}
	if !modusBackupComponent(String(manifest, "backupId")) || strings.TrimSpace(String(manifest, "backupName")) == "" {
		return nil, nil, invalid("$.manifest_file", "manifest requires local backup ID and name")
	}
	sectionRows, ok := manifest["sections"].([]any)
	if !ok || len(sectionRows) != 6 {
		return nil, nil, invalid("$.manifest_file", "manifest requires all six sections")
	}
	sections := map[string][]any{}
	identifiers := map[string]bool{}
	var validate func([]any, string, int) error
	validate = func(items []any, section string, depth int) error {
		if depth > 20 {
			return invalid("$.manifest_file", "manifest hierarchy exceeds limit")
		}
		for _, v := range items {
			item, err := modusBackupObjectMap(v)
			if err != nil {
				return err
			}
			id := String(item, "id")
			if id == "" || identifiers[id] || item["section"] != section || String(item, "name") == "" {
				return invalid("$.manifest_file", "invalid or duplicate manifest item identity")
			}
			identifiers[id] = true
			if meta, ok := item["meta"]; ok {
				if _, ok = meta.(map[string]any); !ok {
					return invalid("$.manifest_file", "manifest metadata must be an object")
				}
			}
			if child, ok := item["children"]; ok {
				children, ok := child.([]any)
				if !ok {
					return invalid("$.manifest_file", "manifest children must be an array")
				}
				if err = validate(children, section, depth+1); err != nil {
					return err
				}
			}
		}
		return nil
	}
	for _, v := range sectionRows {
		section, err := modusBackupObjectMap(v)
		if err != nil {
			return nil, nil, err
		}
		key := String(section, "key")
		known := false
		for _, want := range modusBackupSections {
			known = known || key == want
		}
		if !known || sections[key] != nil {
			return nil, nil, invalid("$.manifest_file", "unknown or duplicate section")
		}
		items, ok := section["items"].([]any)
		if !ok {
			return nil, nil, invalid("$.manifest_file", "section items must be arrays")
		}
		sections[key] = items
		if err = validate(items, key, 0); err != nil {
			return nil, nil, err
		}
	}
	locals := []modusBackupLocal{}
	for _, v := range sections["addons"] {
		item := v.(map[string]any)
		meta, _ := item["meta"].(map[string]any)
		if id, ok := positiveOrZero(meta["projectId"]); !ok || id == 0 {
			return nil, nil, invalid("$.manifest_file", "known addon requires positive projectId")
		}
		if locked, present := meta["isLocked"]; present {
			flag, ok := locked.(bool)
			if !ok {
				return nil, nil, invalid("$.manifest_file", "known addon isLocked must be boolean")
			}
			if flag {
				if id, ok := positiveOrZero(meta["fileId"]); !ok || id == 0 {
					return nil, nil, invalid("$.manifest_file", "locked known addon requires positive fileId")
				}
			}
		}
	}
	var leaves func([]any) []map[string]any
	leaves = func(items []any) []map[string]any {
		out := []map[string]any{}
		for _, v := range items {
			item := v.(map[string]any)
			children, _ := item["children"].([]any)
			if len(children) > 0 {
				out = append(out, leaves(children)...)
			} else {
				out = append(out, item)
			}
		}
		return out
	}
	for _, selection := range []struct {
		Section, Prefix string
		Type            int
	}{{"modifiedAddons", "Interface/AddOns/", 8}, {"unknownAddons", "Interface/AddOns/", 9}, {"materials", "Interface/", 4}, {"fonts", "Fonts/", 3}} {
		local := modusBackupLocal{Section: selection.Section, Type: selection.Type, ContentType: "application/zip"}
		seen := map[string]bool{}
		for _, item := range leaves(sections[selection.Section]) {
			name := String(item, "name")
			if !modusBackupComponent(name) || selection.Section == "materials" && strings.EqualFold(name, "AddOns") || String(item, "targetRelativePath") != selection.Prefix+name {
				return nil, nil, invalid("$.manifest_file", "item path does not match selected category")
			}
			source, err := modusBackupPath(root, selection.Prefix+name)
			if err != nil {
				return nil, nil, err
			}
			stat, err := os.Stat(source)
			if err != nil {
				return nil, nil, err
			}
			files, err := modusBackupFiles(ctx, source)
			if err != nil {
				return nil, nil, err
			}
			if selection.Section == "fonts" {
				if !stat.Mode().IsRegular() {
					return nil, nil, invalid("$.manifest_file", "font must be an individual file")
				}
				_, digest, _, err := modusBackupHash(ctx, source)
				if err != nil {
					return nil, nil, err
				}
				meta, _ := item["meta"].(map[string]any)
				if expected := String(meta, "md5"); expected != "" && !strings.EqualFold(expected, digest) {
					return nil, nil, invalid("$.manifest_file", "font hash differs from manifest")
				}
				locals = append(locals, modusBackupLocal{Section: "fonts", Direct: source, Type: 3, ContentType: "application/octet-stream"})
				continue
			}
			for _, file := range files {
				archiveName := name
				if stat.IsDir() {
					rel, _ := filepath.Rel(source, file)
					archiveName += "/" + filepath.ToSlash(rel)
				}
				norm := strings.ToLower(archiveName)
				if seen[norm] {
					return nil, nil, invalid("$.manifest_file", "duplicate archive entry")
				}
				seen[norm] = true
				local.Entries = append(local.Entries, modusBackupEntry{file, archiveName})
			}
		}
		if len(sections[selection.Section]) > 0 && selection.Section != "fonts" {
			locals = append(locals, local)
		}
	}
	wtf, err := modusBackupWTFPlan(ctx, root, sections["wtf"])
	if err != nil {
		return nil, nil, err
	}
	locals = append(locals, wtf...)
	return manifest, locals, nil
}
func modusBackupWTFPlan(ctx context.Context, root string, items []any) ([]modusBackupLocal, error) {
	locals := []modusBackupLocal{}
	extras := []modusBackupEntry{}
	seenAccounts := map[string]bool{}
	seenExtras := map[string]bool{}
	for _, v := range items {
		item := v.(map[string]any)
		meta, _ := item["meta"].(map[string]any)
		kind := String(meta, "type")
		if kind == "config" || kind == "shared-saved-variables" {
			expected := "WTF/Config.wtf"
			if kind == "shared-saved-variables" {
				expected = "WTF/Account/SavedVariables"
			}
			children, _ := item["children"].([]any)
			if seenExtras[kind] || len(children) > 0 || String(item, "targetRelativePath") != expected {
				return nil, invalid("$.manifest_file", "invalid common WTF selection")
			}
			seenExtras[kind] = true
			source, err := modusBackupPath(root, expected)
			if err != nil {
				return nil, err
			}
			stat, err := os.Stat(source)
			if err != nil {
				return nil, err
			}
			if kind == "config" && (!stat.Mode().IsRegular() || String(item, "name") != "Config.wtf") || kind != "config" && !stat.IsDir() {
				return nil, invalid("$.manifest_file", "wrong WTF common path type")
			}
			files, err := modusBackupFiles(ctx, source)
			if err != nil {
				return nil, err
			}
			for _, file := range files {
				if !modusBackupWTFFile(file) {
					return nil, invalid("$.manifest_file", "WTF contains unsupported file formats")
				}
				name := filepath.Base(source)
				if stat.IsDir() {
					rel, _ := filepath.Rel(source, file)
					name += "/" + filepath.ToSlash(rel)
				}
				extras = append(extras, modusBackupEntry{file, name})
			}
			continue
		}
		account := String(item, "name")
		if kind != "account" || !modusBackupComponent(account) || strings.EqualFold(account, "SavedVariables") || seenAccounts[strings.ToLower(account)] || String(item, "targetRelativePath") != "WTF/Account/"+account {
			return nil, invalid("$.manifest_file", "invalid or duplicate WTF account")
		}
		seenAccounts[strings.ToLower(account)] = true
		if value, ok := meta["account"]; ok && value != account {
			return nil, invalid("$.manifest_file", "WTF account metadata mismatch")
		}
		accountPath, err := modusBackupPath(root, "WTF/Account/"+account)
		if err != nil {
			return nil, err
		}
		if stat, err := os.Stat(accountPath); err != nil || !stat.IsDir() {
			return nil, invalid("$.manifest_file", "WTF account must identify a directory")
		}
		servers, ok := item["children"].([]any)
		if !ok {
			return nil, invalid("$.manifest_file", "account requires explicit server hierarchy")
		}
		selectedRoles := map[string]bool{}
		seenServers := map[string]bool{}
		roles := []string{}
		for _, v := range servers {
			server := v.(map[string]any)
			serverName := String(server, "name")
			serverMeta, _ := server["meta"].(map[string]any)
			if String(serverMeta, "type") != "server" || !modusBackupComponent(serverName) || strings.EqualFold(serverName, "SavedVariables") || seenServers[strings.ToLower(serverName)] || String(server, "targetRelativePath") != "WTF/Account/"+account+"/"+serverName {
				return nil, invalid("$.manifest_file", "invalid WTF server metadata")
			}
			seenServers[strings.ToLower(serverName)] = true
			serverPath, err := modusBackupPath(root, "WTF/Account/"+account+"/"+serverName)
			if err != nil {
				return nil, err
			}
			if stat, err := os.Stat(serverPath); err != nil || !stat.IsDir() {
				return nil, invalid("$.manifest_file", "WTF server must identify a directory")
			}
			for key, want := range map[string]string{"account": account, "server": serverName} {
				if value, ok := serverMeta[key]; ok && value != want {
					return nil, invalid("$.manifest_file", "inconsistent WTF server identity")
				}
			}
			children, ok := server["children"].([]any)
			if !ok {
				return nil, invalid("$.manifest_file", "server requires explicit role leaves")
			}
			for _, v := range children {
				role := v.(map[string]any)
				name := String(role, "name")
				meta, _ := role["meta"].(map[string]any)
				grandchildren, _ := role["children"].([]any)
				identity := strings.ToLower(serverName + "/" + name)
				expected := "WTF/Account/" + account + "/" + serverName + "/" + name
				if String(meta, "type") != "role" || !modusBackupComponent(name) || strings.EqualFold(name, "SavedVariables") || selectedRoles[identity] || len(grandchildren) > 0 || String(role, "targetRelativePath") != expected {
					return nil, invalid("$.manifest_file", "invalid WTF role identity")
				}
				for key, want := range map[string]string{"account": account, "server": serverName} {
					if value, ok := meta[key]; ok && value != want {
						return nil, invalid("$.manifest_file", "inconsistent WTF role metadata")
					}
				}
				target, err := modusBackupPath(root, expected)
				if err != nil {
					return nil, err
				}
				stat, err := os.Stat(target)
				if err != nil || !stat.IsDir() {
					return nil, invalid("$.manifest_file", "selected WTF role is not a directory")
				}
				selectedRoles[identity] = true
				roles = append(roles, serverName+"-"+name)
			}
		}
		files, err := modusBackupFiles(ctx, accountPath)
		if err != nil {
			return nil, err
		}
		local := modusBackupLocal{Section: "wtf", Account: account, Roles: roles, Type: 7, ContentType: "application/zip"}
		for _, file := range files {
			if !modusBackupWTFFile(file) {
				return nil, invalid("$.manifest_file", "WTF contains unsupported file formats")
			}
			rel, _ := filepath.Rel(accountPath, file)
			parts := strings.Split(filepath.ToSlash(rel), "/")
			common := len(parts) == 1 || strings.EqualFold(parts[0], "SavedVariables")
			serverCommon := len(parts) >= 2 && seenServers[strings.ToLower(parts[0])] && (len(parts) == 2 || strings.EqualFold(parts[1], "SavedVariables"))
			selected := len(parts) >= 3 && selectedRoles[strings.ToLower(parts[0]+"/"+parts[1])]
			if !common && !serverCommon && !selected {
				return nil, invalid("$.manifest_file", "WTF contains an unselected character or server")
			}
			local.Entries = append(local.Entries, modusBackupEntry{file, account + "/" + filepath.ToSlash(rel)})
		}
		locals = append(locals, local)
	}
	if len(extras) > 0 && len(locals) == 0 {
		return nil, invalid("$.manifest_file", "WTF common files require an explicit account selection")
	}
	for i := range locals {
		locals[i].Entries = append(locals[i].Entries, extras...)
	}
	return locals, nil
}
func writeModusBackupZIP(ctx context.Context, filename string, entries []modusBackupEntry) error {
	f, err := os.OpenFile(filename, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0600)
	if err != nil {
		return err
	}
	z := zip.NewWriter(f)
	seen := map[string]bool{}
	sort.Slice(entries, func(i, j int) bool { return entries[i].Name < entries[j].Name })
	for _, entry := range entries {
		if err = ctx.Err(); err != nil {
			break
		}
		key := strings.ToLower(entry.Name)
		if seen[key] {
			err = invalid("$.manifest_file", "duplicate ZIP entry")
			break
		}
		seen[key] = true
		var source *os.File
		source, err = openArtifact(entry.File)
		if err != nil {
			break
		}
		var target io.Writer
		target, err = z.Create(entry.Name)
		if err == nil {
			_, err = io.Copy(target, source)
		}
		source.Close()
		if err != nil {
			break
		}
	}
	ze := z.Close()
	fe := f.Close()
	if err != nil {
		return err
	}
	if ze != nil {
		return ze
	}
	return fe
}
func sanitizeModusBackup(v any) {
	switch x := v.(type) {
	case map[string]any:
		for _, k := range []string{"sourcePath", "gamePath", "gameRoot", "backupPath"} {
			delete(x, k)
		}
		for _, child := range x {
			sanitizeModusBackup(child)
		}
	case []any:
		for _, child := range x {
			sanitizeModusBackup(child)
		}
	}
}
