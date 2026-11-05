package publish

import (
	"archive/zip"
	"context"
	"encoding/json"
	"io"
	"os"
	"path/filepath"
	"reflect"
	"regexp"
	"strings"
)

func bbBackupDescriptor(p *bbBackupPackage, a bbBackupArchive) (map[string]any, error) {
	types := map[string]string{"UnknownAddons": "unknownModules", "Materials": "materials", "Fonts": "fonts", "WTFList": "wtf"}
	if types[a.Field] == "" || types[a.Field] != a.Type {
		return nil, invalid("$.file", "invalid archive category")
	}
	if a.Field != "WTFList" {
		r, ok := p.Configuration[a.Field].(map[string]any)
		if !ok {
			return nil, invalid("$.file", "missing archive category")
		}
		return r, nil
	}
	rows, ok := p.Configuration["WTFList"].([]any)
	if !ok {
		return nil, invalid("$.file", "missing account descriptors")
	}
	var found map[string]any
	for _, v := range rows {
		r, ok := v.(map[string]any)
		if !ok {
			return nil, invalid("$.file", "invalid account descriptor")
		}
		if String(r, "Name") == a.Account {
			if found != nil {
				return nil, invalid("$.file", "duplicate account descriptor")
			}
			found = r
		}
	}
	if found == nil {
		return nil, invalid("$.file", "missing archive account")
	}
	return found, nil
}
func bbBackupInteger(v any) (int64, bool) {
	n, ok := v.(json.Number)
	if ok {
		i, e := n.Int64()
		return i, e == nil
	}
	switch x := v.(type) {
	case int:
		return int64(x), true
	case int64:
		return x, true
	}
	return 0, false
}
func bbLoadBackupPackage(ctx context.Context, file string) (*bbBackupPackage, error) {
	path, e := filepath.Abs(file)
	if e != nil {
		return nil, e
	}
	f, e := openArtifact(path)
	if e != nil {
		return nil, e
	}
	defer f.Close()
	info, e := f.Stat()
	if e != nil || info.Size() > 8<<20 {
		return nil, invalid("$.file", "backup package JSON exceeds 8 MiB")
	}
	d := json.NewDecoder(io.LimitReader(f, 8<<20))
	d.UseNumber()
	d.DisallowUnknownFields()
	var p bbBackupPackage
	if e = d.Decode(&p); e != nil {
		return nil, invalid("$.file", "invalid backup package JSON")
	}
	if e = d.Decode(new(any)); e != io.EOF {
		return nil, invalid("$.file", "trailing backup package data")
	}
	if p.Schema != bbPackageSchema && p.Schema != "fupload.blackbox.backup-package.v1" {
		return nil, invalid("$.file", "unsupported backup package schema")
	}
	c := p.Configuration
	allowed := map[string]bool{}
	for _, k := range []string{"HeyboxID", "Name", "SaveTime", "IsClassic", "Strappid", "KnownAddons", "WTFList", "Strategies", "UnknownAddons", "Materials", "Fonts"} {
		allowed[k] = true
	}
	for k := range c {
		if !allowed[k] {
			return nil, invalid("$.file", "unexpected configuration field")
		}
	}
	id, ok := bbBackupInteger(c["HeyboxID"])
	branch := String(c, "Strappid")
	if !ok || id != 0 || !regexp.MustCompile(`^wow[a-z0-9_]+$`).MatchString(branch) || c["IsClassic"] != (branch != "wowretail") {
		return nil, invalid("$.file", "invalid package identity")
	}
	if e = bbBackupKnown(c["KnownAddons"]); e != nil {
		return nil, e
	}
	if _, ok := c["Name"].(string); !ok {
		return nil, invalid("$.file", "invalid backup name")
	}
	if n, ok := bbBackupInteger(c["SaveTime"]); !ok || n < 0 {
		return nil, invalid("$.file", "invalid backup timestamp")
	}
	if v, ok := c["Strategies"]; ok {
		a, ok := v.([]any)
		if !ok {
			return nil, invalid("$.file", "invalid strategies")
		}
		for _, v := range a {
			if _, ok := v.(map[string]any); !ok {
				return nil, invalid("$.file", "invalid strategy object")
			}
		}
	}
	accounts, ok := c["WTFList"].([]any)
	if !ok {
		return nil, invalid("$.file", "missing account array")
	}
	expected := map[string]bool{}
	for _, k := range []string{"UnknownAddons", "Materials", "Fonts"} {
		if _, ok := c[k]; ok {
			expected[k+"/"] = true
		}
	}
	seenAccounts := map[string]bool{}
	for _, v := range accounts {
		r, ok := v.(map[string]any)
		if !ok || !modusBackupComponent(String(r, "Name")) {
			return nil, invalid("$.file", "invalid account name")
		}
		n := String(r, "Name")
		if seenAccounts[strings.ToLower(n)] {
			return nil, invalid("$.file", "duplicate account")
		}
		seenAccounts[strings.ToLower(n)] = true
		expected["WTFList/"+n] = true
	}
	seenFiles, seenCategories := map[string]bool{}, map[string]bool{}
	var total int64
	for i := range p.Archives {
		a := &p.Archives[i]
		if !modusBackupComponent(a.File) || seenFiles[strings.ToLower(a.File)] || a.Local != "" || a.State != "" || a.Host != "" || a.Path != "" {
			return nil, invalid("$.file", "invalid or duplicate archive name")
		}
		seenFiles[strings.ToLower(a.File)] = true
		local, e := modusBackupPath(filepath.Dir(path), a.File)
		if e != nil {
			return nil, e
		}
		sha, md5, size, e := modusBackupHash(ctx, local)
		if e != nil {
			return nil, e
		}
		if size != a.Bytes || sha != a.SHA256 || md5 != a.MD5 {
			return nil, invalid("$.file", "archive hash or size mismatch")
		}
		desc, e := bbBackupDescriptor(&p, *a)
		if e != nil {
			return nil, e
		}
		n, ok := bbBackupInteger(desc["Size"])
		if !ok || n != size || desc["MD5"] != md5 || String(desc, "Host") != "" || String(desc, "Path") != "" {
			return nil, invalid("$.file", "archive descriptor mismatch")
		}
		identity := a.Field + "/" + a.Account
		if a.Field != "WTFList" && a.Account != "" || seenCategories[identity] || !expected[identity] {
			return nil, invalid("$.file", "duplicate or unexpected category")
		}
		seenCategories[identity] = true
		files, ok := desc["Files"].([]any)
		if !ok || len(files) == 0 {
			return nil, invalid("$.file", "missing source file descriptors")
		}
		sizes := map[string]int64{}
		for _, v := range files {
			r, ok := v.(map[string]any)
			if !ok || !modusBackupComponent(String(r, "Name")) {
				return nil, invalid("$.file", "invalid source descriptor")
			}
			name := String(r, "Name")
			n, ok := bbBackupInteger(r["Size"])
			if !ok || n < 0 {
				return nil, invalid("$.file", "invalid source size")
			}
			if _, ok := sizes[name]; ok {
				return nil, invalid("$.file", "duplicate source descriptor")
			}
			sizes[name] = n
		}
		chars := map[string]bool{}
		if a.Field == "WTFList" {
			if len(sizes) != 1 {
				return nil, invalid("$.file", "WTF archive requires exactly its account")
			}
			if _, ok := sizes[a.Account]; !ok {
				return nil, invalid("$.file", "WTF archive account mismatch")
			}
			pairs, ok := desc["RealmCharacters"].([]any)
			if !ok || len(pairs) == 0 {
				return nil, invalid("$.file", "explicit characters required")
			}
			names := []any{}
			for _, v := range pairs {
				r, ok := v.(map[string]any)
				if !ok || len(r) != 2 || !modusBackupComponent(String(r, "Realm")) || !modusBackupComponent(String(r, "Player")) {
					return nil, invalid("$.file", "invalid character")
				}
				key := String(r, "Realm") + "/" + String(r, "Player")
				if chars[key] {
					return nil, invalid("$.file", "duplicate character")
				}
				chars[key] = true
				names = append(names, String(r, "Realm")+" - "+String(r, "Player"))
			}
			if !reflect.DeepEqual(names, desc["Characters"]) {
				return nil, invalid("$.file", "character metadata mismatch")
			}
		}
		z, e := zip.OpenReader(local)
		if e != nil {
			return nil, invalid("$.file", "invalid ZIP")
		}
		actual := map[string]int64{}
		paths := map[string]bool{}
		var zipErr error
		for _, entry := range z.File {
			parts := strings.Split(entry.Name, "/")
			if entry.Name == "" || strings.ContainsAny(entry.Name, "\\:") || strings.HasPrefix(entry.Name, "/") || paths[strings.ToLower(entry.Name)] || entry.Mode()&os.ModeSymlink != 0 {
				zipErr = invalid("$.file", "unsafe ZIP path")
				break
			}
			paths[strings.ToLower(entry.Name)] = true
			for _, part := range parts {
				if !modusBackupComponent(part) {
					zipErr = invalid("$.file", "unsafe ZIP component")
					break
				}
			}
			if zipErr != nil {
				break
			}
			if bbBackupExcluded(entry.Name) {
				zipErr = invalid("$.file", "excluded player telemetry in archive")
				break
			}
			if a.Field == "WTFList" && len(parts) > 2 && !strings.EqualFold(parts[1], "SavedVariables") {
				if len(parts) < 4 || !chars[parts[1]+"/"+parts[2]] {
					zipErr = invalid("$.file", "unselected WTF character in archive")
					break
				}
			}
			if entry.UncompressedSize64 > uint64(^uint64(0)>>1) {
				zipErr = invalid("$.file", "invalid ZIP size")
				break
			}
			actual[parts[0]] += int64(entry.UncompressedSize64)
			stream, e := entry.Open()
			if e != nil {
				zipErr = e
				break
			}
			n, e := io.Copy(io.Discard, stream)
			stream.Close()
			if e != nil || uint64(n) != entry.UncompressedSize64 {
				zipErr = invalid("$.file", "ZIP content checksum failed")
				break
			}
			if e = ctx.Err(); e != nil {
				zipErr = e
				break
			}
		}
		z.Close()
		if zipErr != nil {
			return nil, zipErr
		}
		if !reflect.DeepEqual(actual, sizes) {
			return nil, invalid("$.file", "ZIP layout or source sizes mismatch")
		}
		for _, n := range sizes {
			total += n
		}
		a.Local = local
	}
	if !reflect.DeepEqual(expected, seenCategories) || len(expected) == 0 && len(c["KnownAddons"].([]any)) == 0 || total != p.SourceBytes {
		return nil, invalid("$.file", "incomplete package categories or source bytes")
	}
	return &p, nil
}
