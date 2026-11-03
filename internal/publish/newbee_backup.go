package publish

import (
	"archive/zip"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"io/fs"
	"os"
	"path/filepath"
	"strings"
)

const newBeeBundleSchema = "lycheedev.newbee.backup-bundle.v1"

type newBeeBackupArtifact struct {
	File     string   `json:"file"`
	Size     int64    `json:"size"`
	SHA256   string   `json:"sha256"`
	FileType string   `json:"file_type"`
	Target   []any    `json:"target"`
	Entries  []string `json:"entries"`
}
type newBeeBackupBundle struct {
	Schema        string                 `json:"schema"`
	Manifest      map[string]any         `json:"manifest"`
	Artifacts     []newBeeBackupArtifact `json:"artifacts"`
	FontDirectory string                 `json:"font_directory,omitempty"`
	FontFiles     []newBeeBackupArtifact `json:"font_files,omitempty"`
}

func addNewBeeBackupCommands(all map[string]newBeeCreatorCommand) {
	text := Field{Type: "string", Nonempty: true}
	id := Field{Type: "integer", Minimum: Int(1)}
	texts := Field{Type: "array", Items: &text}
	boolean := Field{Type: "boolean"}
	role := Field{Type: "object", Properties: map[string]Field{"server": text, "name": text}, Required: []string{"server", "name"}}
	account := Field{Type: "object", Properties: map[string]Field{"account": text, "include_common": boolean, "roles": {Type: "array", Items: &role}}, Required: []string{"account", "include_common", "roles"}}
	nullableText := Field{Type: "string", Nullable: true}
	nullableID := id
	nullableID.Nullable = true
	known := Field{Type: "object", Properties: map[string]Field{"name": text, "id": id, "fileName": nullableText, "fileUrl": nullableText, "mod_file_id": nullableID, "mod_version": nullableText, "display_name": nullableText, "updateType": id}, Required: []string{"name", "id", "fileName", "fileUrl", "mod_file_id", "mod_version", "display_name", "updateType"}}
	all["backup scan"] = newBeeCreatorCommand{spec: InputSpec{Properties: map[string]Field{"game_dir": text}, Required: []string{"game_dir"}, Effects: []string{"list local component names without reading account or character content"}}}
	all["backup pack"] = newBeeCreatorCommand{write: true, spec: InputSpec{Properties: map[string]Field{"game_dir": text, "output_dir": text, "game_version_id": id, "unknown_plugins": texts, "materials": texts, "fonts": texts, "known_plugins": {Type: "array", Items: &known}, "wtf_accounts": {Type: "array", Items: &account}}, Required: []string{"game_dir", "output_dir", "game_version_id"}, Effects: []string{"archive only selected local plugins, materials, fonts and account/character settings"}}}
	all["backup upload"] = newBeeCreatorCommand{write: true, spec: InputSpec{Properties: map[string]Field{"bundle_file": text, "name": text}, Required: []string{"bundle_file"}, Effects: []string{"upload verified selected bundle components and create one cloud backup"}}}
	all["backup delete-preview"] = newBeeCreatorCommand{spec: InputSpec{Properties: map[string]Field{"ids": {Type: "array", Items: &id, Nonempty: true}}, Required: []string{"ids"}, Effects: []string{"read complete configuration references before selected backup deletion"}}}
	all["backup delete"] = newBeeCreatorCommand{write: true, spec: InputSpec{Properties: map[string]Field{"ids": {Type: "array", Items: &id, Nonempty: true}, "confirm_delete": boolean}, Required: []string{"ids", "confirm_delete"}, Effects: []string{"delete selected unreferenced backups with per-item durable receipts"}}}
}

func newBeeLocal(root string, parts ...string) (string, error) {
	absolute, err := filepath.Abs(root)
	if err != nil {
		return "", err
	}
	path := absolute
	for _, part := range parts {
		if part == "" || part == "." || part == ".." || strings.ContainsAny(part, "/\\:\x00<>\"|?*") || strings.HasSuffix(part, ".") || strings.HasSuffix(part, " ") {
			return "", invalid("$selection", "expected one local name")
		}
		path = filepath.Join(path, part)
	}
	for current := path; ; current = filepath.Dir(current) {
		info, err := os.Lstat(current)
		if err != nil {
			return "", invalid("$selection", "selected local path is missing")
		}
		if info.Mode()&os.ModeSymlink != 0 {
			return "", invalid("$selection", "selected local path contains a link")
		}
		if current == absolute {
			break
		}
		if filepath.Dir(current) == current {
			return "", invalid("$selection", "selected path escaped root")
		}
	}
	return path, nil
}
func newBeeScan(root string) (map[string]any, error) {
	root, err := newBeeLocal(root)
	if err != nil {
		return nil, err
	}
	info, err := os.Stat(root)
	if err != nil || !info.IsDir() {
		return nil, invalid("$.game_dir", "game directory must exist")
	}
	result := map[string]any{"game_dir": root, "unknown_plugins": []any{}, "materials": []any{}, "fonts": []any{}, "wtf_accounts": []any{}}
	list := func(parts []string) []os.DirEntry {
		path := filepath.Join(append([]string{root}, parts...)...)
		rows, _ := os.ReadDir(path)
		return rows
	}
	for _, entry := range list([]string{"Interface", "AddOns"}) {
		if entry.IsDir() {
			if _, err = newBeeLocal(root, "Interface", "AddOns", entry.Name()); err != nil {
				return nil, err
			}
			result["unknown_plugins"] = append(result["unknown_plugins"].([]any), map[string]any{"name": entry.Name()})
		}
	}
	for _, entry := range list([]string{"Interface"}) {
		if !strings.EqualFold(entry.Name(), "AddOns") {
			if _, err = newBeeLocal(root, "Interface", entry.Name()); err != nil {
				return nil, err
			}
			result["materials"] = append(result["materials"].([]any), map[string]any{"name": entry.Name()})
		}
	}
	for _, entry := range list([]string{"Fonts"}) {
		if !entry.IsDir() && strings.EqualFold(filepath.Ext(entry.Name()), ".ttf") {
			path, err := newBeeLocal(root, "Fonts", entry.Name())
			if err != nil {
				return nil, err
			}
			info, err := os.Stat(path)
			if err != nil {
				return nil, err
			}
			result["fonts"] = append(result["fonts"].([]any), map[string]any{"name": entry.Name(), "size": info.Size()})
		}
	}
	for _, account := range list([]string{"WTF", "Account"}) {
		if !account.IsDir() {
			continue
		}
		if _, err = newBeeLocal(root, "WTF", "Account", account.Name()); err != nil {
			return nil, err
		}
		roles := []any{}
		common := false
		for _, server := range list([]string{"WTF", "Account", account.Name()}) {
			if !server.IsDir() || strings.EqualFold(server.Name(), "SavedVariables") {
				common = true
				continue
			}
			for _, role := range list([]string{"WTF", "Account", account.Name(), server.Name()}) {
				if !role.IsDir() {
					continue
				}
				found := false
				for _, entry := range list([]string{"WTF", "Account", account.Name(), server.Name(), role.Name()}) {
					if entry.Name() == "AddOns.txt" || entry.Name() == "bindings-cache.wtf" || entry.Name() == "config-cache.wtf" {
						found = true
					}
				}
				if found {
					if _, err = newBeeLocal(root, "WTF", "Account", account.Name(), server.Name(), role.Name()); err != nil {
						return nil, err
					}
					roles = append(roles, map[string]any{"server": server.Name(), "name": role.Name()})
				}
			}
		}
		if len(roles) > 0 || common {
			result["wtf_accounts"] = append(result["wtf_accounts"].([]any), map[string]any{"account": account.Name(), "roles": roles, "has_common": common})
		}
	}
	return result, nil
}
func newBeeArchive(ctx context.Context, root string, sources []string, destination string, excludeBak bool) (newBeeBackupArtifact, error) {
	artifact := newBeeBackupArtifact{File: filepath.Base(destination)}
	file, err := os.OpenFile(destination, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0600)
	if err != nil {
		return artifact, err
	}
	writer := zip.NewWriter(file)
	success := false
	defer func() {
		writer.Close()
		file.Close()
		if !success {
			os.Remove(destination)
		}
	}()
	for _, source := range sources {
		err = filepath.WalkDir(source, func(path string, entry fs.DirEntry, walkErr error) error {
			if walkErr != nil {
				return walkErr
			}
			if err := ctx.Err(); err != nil {
				return err
			}
			if entry.Type()&os.ModeSymlink != 0 {
				return invalid("$selection", "backup contains a link")
			}
			if entry.IsDir() {
				return nil
			}
			if excludeBak && strings.HasSuffix(strings.ToLower(entry.Name()), ".bak") {
				return nil
			}
			rel, err := filepath.Rel(root, path)
			if err != nil {
				return err
			}
			parts := strings.Split(rel, string(filepath.Separator))
			if _, err = newBeeLocal(root, parts...); err != nil {
				return err
			}
			name, err := filepath.Rel(filepath.Dir(source), path)
			if err != nil {
				return err
			}
			name = filepath.ToSlash(name)
			opened, err := openArtifact(path)
			if err != nil {
				return err
			}
			defer opened.Close()
			info, err := opened.Stat()
			if err != nil || !info.Mode().IsRegular() {
				return invalid("$selection", "backup contains a non-regular file")
			}
			header := &zip.FileHeader{Name: name, Method: zip.Deflate}
			header.SetMode(0600)
			part, err := writer.CreateHeader(header)
			if err != nil {
				return err
			}
			if _, err = io.Copy(part, opened); err != nil {
				return err
			}
			artifact.Entries = append(artifact.Entries, name)
			return nil
		})
		if err != nil {
			return artifact, err
		}
	}
	if err = writer.Close(); err != nil {
		return artifact, err
	}
	if err = file.Close(); err != nil {
		return artifact, err
	}
	if len(artifact.Entries) == 0 {
		return artifact, invalid("$selection", "selected backup archive is empty")
	}
	opened, err := os.Open(destination)
	if err != nil {
		return artifact, err
	}
	defer opened.Close()
	hash := sha256.New()
	artifact.Size, err = io.Copy(hash, opened)
	if err != nil {
		return artifact, err
	}
	artifact.SHA256 = hex.EncodeToString(hash.Sum(nil))
	success = true
	return artifact, nil
}
func newBeePack(ctx context.Context, input map[string]any) (any, error) {
	root := String(input, "game_dir")
	available, err := newBeeScan(root)
	if err != nil {
		return nil, err
	}
	root = available["game_dir"].(string)
	output, err := filepath.Abs(String(input, "output_dir"))
	if err != nil {
		return nil, err
	}
	inside, err := backupOutputWithin(root, output)
	if err != nil {
		return nil, err
	}
	if inside {
		return nil, invalid("$.output_dir", "output must be outside game directory")
	}
	if entries, e := os.ReadDir(output); e == nil && len(entries) > 0 {
		return nil, invalid("$.output_dir", "output directory must be empty")
	} else if e != nil && !os.IsNotExist(e) {
		return nil, e
	}
	selected := map[string][]string{}
	for _, field := range []string{"unknown_plugins", "materials", "fonts"} {
		allowed := map[string]bool{}
		for _, row := range available[field].([]any) {
			allowed[String(row.(map[string]any), "name")] = true
		}
		seen := map[string]bool{}
		for _, raw := range newBeeRows(input[field]) {
			name := raw.(string)
			if !allowed[name] || seen[strings.ToLower(name)] {
				return nil, invalid("$."+field, "duplicate or unavailable component")
			}
			seen[strings.ToLower(name)] = true
			selected[field] = append(selected[field], name)
		}
	}
	manifest := map[string]any{"knowplug_list": []any{}, "t_unKnown_list": []any{}, "t_font_list": []any{}, "t_material_list": []any{}, "t_WTF_list": []any{}, "versionid": input["game_version_id"], "wtfconfig": nil, "unknown_plug": nil, "material": nil, "font": nil, "font_hash": nil, "wtf": []any{}}
	if v, ok := input["known_plugins"]; ok {
		manifest["knowplug_list"] = v
	}
	type source struct {
		paths      []string
		file, kind string
		target     []any
		bak        bool
	}
	sources := []source{}
	for _, group := range []struct {
		field      string
		parts      []string
		kind, list string
	}{{"unknown_plugins", []string{"Interface", "AddOns"}, "unknown_plug", "t_unKnown_list"}, {"materials", []string{"Interface"}, "material", "t_material_list"}} {
		paths := []string{}
		for _, name := range selected[group.field] {
			path, err := newBeeLocal(root, append(append([]string{}, group.parts...), name)...)
			if err != nil {
				return nil, err
			}
			paths = append(paths, path)
			manifest[group.list] = append(manifest[group.list].([]any), map[string]any{"name": name})
		}
		if len(paths) > 0 {
			sources = append(sources, source{paths: paths, file: group.kind + ".zip", kind: group.kind, target: []any{group.kind}})
		}
	}
	seenAccounts := map[string]bool{}
	for ai, raw := range newBeeRows(input["wtf_accounts"]) {
		choice := raw.(map[string]any)
		name := String(choice, "account")
		if seenAccounts[strings.ToLower(name)] {
			return nil, invalid("$.wtf_accounts", "duplicate account")
		}
		seenAccounts[strings.ToLower(name)] = true
		var existing map[string]any
		for _, row := range available["wtf_accounts"].([]any) {
			obj := row.(map[string]any)
			if obj["account"] == name {
				existing = obj
			}
		}
		if existing == nil {
			return nil, invalid("$.wtf_accounts", "account is absent")
		}
		accountPath, err := newBeeLocal(root, "WTF", "Account", name)
		if err != nil {
			return nil, err
		}
		account := map[string]any{"id": ai + 1, "account": name, "server": []any{}, "commonConfig": ""}
		common := []string{}
		if Bool(choice, "include_common") {
			entries, err := os.ReadDir(accountPath)
			if err != nil {
				return nil, err
			}
			for _, entry := range entries {
				if !entry.IsDir() || strings.EqualFold(entry.Name(), "SavedVariables") {
					path, err := newBeeLocal(root, "WTF", "Account", name, entry.Name())
					if err != nil {
						return nil, err
					}
					common = append(common, path)
				}
			}
		}
		if len(common) > 0 {
			sources = append(sources, source{paths: common, file: fmt.Sprintf("wtf-%d-common.zip", ai), kind: "wtfserve", target: []any{"wtf", ai, "commonConfig"}, bak: true})
		}
		seenRoles := map[string]bool{}
		roles := newBeeRows(choice["roles"])
		for ri, rawRole := range roles {
			role := rawRole.(map[string]any)
			server, roleName := String(role, "server"), String(role, "name")
			key := strings.ToLower(server + "/" + roleName)
			if seenRoles[key] {
				return nil, invalid("$.wtf_accounts", "duplicate role")
			}
			seenRoles[key] = true
			found := false
			for _, availableRole := range existing["roles"].([]any) {
				if newBeeEqual(role, availableRole) {
					found = true
				}
			}
			if !found {
				return nil, invalid("$.wtf_accounts", "role is absent")
			}
			path, err := newBeeLocal(root, "WTF", "Account", name, server, roleName)
			if err != nil {
				return nil, err
			}
			servers := account["server"].([]any)
			si := -1
			for i, row := range servers {
				if row.(map[string]any)["serverName"] == server {
					si = i
				}
			}
			if si < 0 {
				si = len(servers)
				servers = append(servers, map[string]any{"serverName": server, "roleList": []any{}})
			}
			serverRow := servers[si].(map[string]any)
			roleList := serverRow["roleList"].([]any)
			index := len(roleList)
			serverRow["roleList"] = append(roleList, map[string]any{"name": roleName, "zip": ""})
			account["server"] = servers
			sources = append(sources, source{paths: []string{path}, file: fmt.Sprintf("wtf-%d-role-%d.zip", ai, ri), kind: "wtfrole", target: []any{"wtf", ai, "server", si, "roleList", index, "zip"}, bak: true})
		}
		if len(common) == 0 && len(roles) == 0 {
			return nil, invalid("$.wtf_accounts", "account selection is empty")
		}
		manifest["wtf"] = append(manifest["wtf"].([]any), account)
	}
	if len(sources) == 0 && len(newBeeRows(manifest["knowplug_list"])) == 0 && len(selected["fonts"]) == 0 {
		return nil, invalid("$", "select at least one backup component")
	}
	if err = os.MkdirAll(output, 0700); err != nil {
		return nil, err
	}
	bundle := newBeeBackupBundle{Schema: newBeeBundleSchema, Manifest: manifest, Artifacts: []newBeeBackupArtifact{}}
	for _, source := range sources {
		artifact, err := newBeeArchive(ctx, root, source.paths, filepath.Join(output, source.file), source.bak)
		if err != nil {
			return nil, err
		}
		artifact.FileType = source.kind
		artifact.Target = source.target
		bundle.Artifacts = append(bundle.Artifacts, artifact)
	}
	if len(selected["fonts"]) > 0 {
		bundle.FontDirectory = "Fonts"
		if err = os.Mkdir(filepath.Join(output, "Fonts"), 0700); err != nil {
			return nil, err
		}
		for _, name := range selected["fonts"] {
			path, err := newBeeLocal(root, "Fonts", name)
			if err != nil {
				return nil, err
			}
			raw, err := os.ReadFile(path)
			if err != nil {
				return nil, err
			}
			hash := sha256.Sum256(raw)
			if err = os.WriteFile(filepath.Join(output, "Fonts", name), raw, 0600); err != nil {
				return nil, err
			}
			bundle.FontFiles = append(bundle.FontFiles, newBeeBackupArtifact{File: name, Size: int64(len(raw)), SHA256: hex.EncodeToString(hash[:])})
			manifest["t_font_list"] = append(manifest["t_font_list"].([]any), map[string]any{"name": name, "size": len(raw)})
		}
	}
	raw, err := json.MarshalIndent(bundle, "", "  ")
	if err != nil {
		return nil, err
	}
	bundlePath := filepath.Join(output, "backup-bundle.json")
	if err = os.WriteFile(bundlePath, raw, 0600); err != nil {
		return nil, err
	}
	return map[string]any{"bundle_file": bundlePath, "artifacts": bundle.Artifacts, "font_count": len(bundle.FontFiles), "account_count": len(newBeeRows(manifest["wtf"]))}, nil
}
