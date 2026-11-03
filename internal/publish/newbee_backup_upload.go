package publish

import (
	"archive/zip"
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"os"
	"path/filepath"
	"strings"

	"github.com/follenfang/lycheedev/internal/vault"
)

func newBeeReadBundle(path string) (newBeeBackupBundle, error) {
	var bundle newBeeBackupBundle
	file, err := os.Open(path)
	if err != nil {
		return bundle, invalid("$.bundle_file", "cannot read backup bundle")
	}
	defer file.Close()
	raw, err := io.ReadAll(io.LimitReader(file, MaxInputBytes+1))
	if err != nil || len(raw) > MaxInputBytes {
		return bundle, invalid("$.bundle_file", "backup bundle exceeds input bound")
	}
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.UseNumber()
	decoder.DisallowUnknownFields()
	if err = decoder.Decode(&bundle); err != nil || bundle.Schema != newBeeBundleSchema {
		return bundle, invalid("$.bundle_file", "unsupported backup bundle")
	}
	if decoder.Decode(new(any)) != io.EOF {
		return bundle, invalid("$.bundle_file", "trailing bundle data")
	}
	keys := map[string]bool{}
	for _, key := range []string{"knowplug_list", "t_unKnown_list", "t_font_list", "t_material_list", "t_WTF_list", "versionid", "wtfconfig", "unknown_plug", "material", "font", "font_hash", "wtf"} {
		keys[key] = true
	}
	if len(bundle.Manifest) != len(keys) {
		return bundle, invalid("$.bundle_file", "invalid backup manifest")
	}
	for key := range bundle.Manifest {
		if !keys[key] {
			return bundle, invalid("$.bundle_file", "unknown backup manifest field")
		}
	}
	if newBeeID(bundle.Manifest["versionid"]) <= 0 {
		return bundle, invalid("$.bundle_file", "invalid game version")
	}
	for _, key := range []string{"wtfconfig", "unknown_plug", "material", "font", "font_hash"} {
		if bundle.Manifest[key] != nil {
			return bundle, invalid("$.bundle_file", "bundle must contain pending local uploads")
		}
	}
	for _, key := range []string{"knowplug_list", "t_unKnown_list", "t_font_list", "t_material_list", "t_WTF_list", "wtf"} {
		if _, ok := bundle.Manifest[key].([]any); !ok {
			return bundle, invalid("$.bundle_file", "manifest selections must be arrays")
		}
	}
	if len(newBeeRows(bundle.Manifest["t_WTF_list"])) != 0 {
		return bundle, invalid("$.bundle_file", "bundle contains an existing WTF object")
	}
	targets := map[string]bool{}
	names := map[string]bool{}
	root := filepath.Dir(path)
	for _, artifact := range bundle.Artifacts {
		filename, err := newBeeLocal(root, artifact.File)
		if err != nil {
			return bundle, err
		}
		if names[strings.ToLower(artifact.File)] {
			return bundle, invalid("$.bundle_file", "duplicate artifact file")
		}
		names[strings.ToLower(artifact.File)] = true
		if err = newBeeCheckArtifact(filename, artifact); err != nil {
			return bundle, err
		}
		targetKey, _ := json.Marshal(artifact.Target)
		if targets[string(targetKey)] {
			return bundle, invalid("$.bundle_file", "duplicate artifact target")
		}
		targets[string(targetKey)] = true
		allowed := false
		target := artifact.Target
		if len(target) == 1 {
			allowed = (target[0] == "unknown_plug" && artifact.FileType == "unknown_plug") || (target[0] == "material" && artifact.FileType == "material")
		}
		if len(target) == 3 {
			allowed = target[0] == "wtf" && target[2] == "commonConfig" && artifact.FileType == "wtfserve"
		}
		if len(target) == 7 {
			allowed = target[0] == "wtf" && target[2] == "server" && target[4] == "roleList" && target[6] == "zip" && artifact.FileType == "wtfrole"
		}
		if !allowed {
			return bundle, invalid("$.bundle_file", "invalid artifact target")
		}
		if err = newBeeSetTarget(bundle.Manifest, target, "", false); err != nil {
			return bundle, err
		}
		archive, err := zip.OpenReader(filename)
		if err != nil {
			return bundle, invalid("$.bundle_file", "invalid backup ZIP")
		}
		seen := map[string]bool{}
		allowedRoots := map[string]bool{}
		if artifact.FileType == "unknown_plug" || artifact.FileType == "material" {
			field := "t_unKnown_list"
			if artifact.FileType == "material" {
				field = "t_material_list"
			}
			for _, row := range newBeeRows(bundle.Manifest[field]) {
				name := String(newBeeObject(row), "name")
				if name == "" || artifact.FileType == "material" && strings.EqualFold(name, "AddOns") {
					archive.Close()
					return bundle, invalid("$.bundle_file", "invalid component selection")
				}
				allowedRoots[strings.ToLower(name)] = true
			}
		}
		if artifact.FileType == "wtfrole" {
			account := newBeeObject(newBeeRows(bundle.Manifest["wtf"])[int(newBeeID(target[1]))])
			server := newBeeObject(newBeeRows(account["server"])[int(newBeeID(target[3]))])
			role := newBeeObject(newBeeRows(server["roleList"])[int(newBeeID(target[5]))])
			allowedRoots[strings.ToLower(String(role, "name"))] = true
		}
		observedRoots := map[string]bool{}
		for _, entry := range archive.File {
			name := strings.ReplaceAll(entry.Name, "\\", "/")
			if strings.HasPrefix(name, "/") || strings.Contains(name, "../") || strings.Contains(name, ":") || entry.Mode()&os.ModeSymlink != 0 {
				archive.Close()
				return bundle, invalid("$.bundle_file", "unsafe backup ZIP member")
			}
			parts := strings.Split(strings.TrimSuffix(name, "/"), "/")
			for _, part := range parts {
				if part == "" || part == "." || part == ".." || strings.HasSuffix(part, ".") || strings.HasSuffix(part, " ") {
					archive.Close()
					return bundle, invalid("$.bundle_file", "unsafe backup ZIP name")
				}
			}
			rootName := strings.ToLower(parts[0])
			if artifact.FileType == "wtfserve" {
				if len(parts) > 1 && rootName != "savedvariables" {
					archive.Close()
					return bundle, invalid("$.bundle_file", "common account archive includes role content")
				}
			} else if !allowedRoots[rootName] {
				archive.Close()
				return bundle, invalid("$.bundle_file", "ZIP includes an unselected component")
			}
			if (artifact.FileType == "wtfserve" || artifact.FileType == "wtfrole") && strings.HasSuffix(strings.ToLower(name), ".bak") {
				archive.Close()
				return bundle, invalid("$.bundle_file", "WTF archive includes excluded backup file")
			}
			observedRoots[rootName] = true
			seen[name] = true
		}
		archive.Close()
		for name := range allowedRoots {
			if !observedRoots[name] {
				return bundle, invalid("$.bundle_file", "selected component is absent from ZIP")
			}
		}
		if len(seen) != len(artifact.Entries) {
			return bundle, invalid("$.bundle_file", "archive entries differ from bundle")
		}
		for _, entry := range artifact.Entries {
			if !seen[entry] {
				return bundle, invalid("$.bundle_file", "archive selection changed")
			}
		}
	}
	for _, key := range []string{"unknown_plug", "material"} {
		list := "t_unKnown_list"
		if key == "material" {
			list = "t_material_list"
		}
		target, _ := json.Marshal([]any{key})
		if (len(newBeeRows(bundle.Manifest[list])) > 0) != targets[string(target)] {
			return bundle, invalid("$.bundle_file", "selected component has no archive")
		}
	}
	for ai, rawAccount := range newBeeRows(bundle.Manifest["wtf"]) {
		account := newBeeObject(rawAccount)
		if account == nil || account["commonConfig"] != "" {
			return bundle, invalid("$.bundle_file", "invalid pending account")
		}
		for si, rawServer := range newBeeRows(account["server"]) {
			server := newBeeObject(rawServer)
			for ri, rawRole := range newBeeRows(server["roleList"]) {
				role := newBeeObject(rawRole)
				if role == nil || role["zip"] != "" {
					return bundle, invalid("$.bundle_file", "invalid pending role")
				}
				key, _ := json.Marshal([]any{"wtf", ai, "server", si, "roleList", ri, "zip"})
				if !targets[string(key)] {
					return bundle, invalid("$.bundle_file", "selected role has no archive")
				}
			}
		}
	}
	if (bundle.FontDirectory != "") != (len(newBeeRows(bundle.Manifest["t_font_list"])) > 0) {
		return bundle, invalid("$.bundle_file", "font selection has no local directory")
	}
	if bundle.FontDirectory != "" {
		directory, err := newBeeLocal(root, bundle.FontDirectory)
		if err != nil {
			return bundle, err
		}
		entries, err := os.ReadDir(directory)
		if err != nil || len(entries) != len(bundle.FontFiles) || len(entries) != len(newBeeRows(bundle.Manifest["t_font_list"])) {
			return bundle, invalid("$.bundle_file", "font directory differs from selection")
		}
		for _, font := range bundle.FontFiles {
			filename, err := newBeeLocal(directory, font.File)
			if err != nil {
				return bundle, err
			}
			if err = newBeeCheckArtifact(filename, font); err != nil {
				return bundle, err
			}
		}
	}
	return bundle, nil
}
func newBeeCheckArtifact(path string, artifact newBeeBackupArtifact) error {
	file, err := openArtifact(path)
	if err != nil {
		return invalid("$.bundle_file", "artifact is missing")
	}
	defer file.Close()
	hash := sha256.New()
	size, err := io.Copy(hash, file)
	if err != nil || size != artifact.Size || hex.EncodeToString(hash.Sum(nil)) != artifact.SHA256 {
		return invalid("$.bundle_file", "artifact bytes changed")
	}
	return nil
}
func newBeeSetTarget(manifest map[string]any, target []any, value any, write bool) error {
	var parent any = manifest
	for i, key := range target {
		last := i == len(target)-1
		if object, ok := parent.(map[string]any); ok {
			name, ok := key.(string)
			if !ok {
				return invalid("$.bundle_file", "invalid target key")
			}
			old, exists := object[name]
			if !exists {
				return invalid("$.bundle_file", "target is absent")
			}
			if last {
				if old != nil && old != "" {
					return invalid("$.bundle_file", "target is not pending")
				}
				if write {
					object[name] = value
				}
				return nil
			}
			parent = old
		} else if rows, ok := parent.([]any); ok {
			index := int(newBeeID(key))
			if index < 0 || index >= len(rows) || fmt.Sprint(index) != fmt.Sprint(key) {
				return invalid("$.bundle_file", "invalid target index")
			}
			parent = rows[index]
		} else {
			return invalid("$.bundle_file", "invalid target path")
		}
	}
	return invalid("$.bundle_file", "empty artifact target")
}

type newBeeBackupReceipt struct {
	Session     string         `json:"session"`
	Fingerprint string         `json:"fingerprint"`
	Manifest    map[string]any `json:"manifest"`
	Uploaded    int            `json:"uploaded"`
	Stage       string         `json:"stage"`
	Before      []int64        `json:"before"`
	ID          int64          `json:"id,omitempty"`
	Complete    bool           `json:"complete"`
	Rejected    bool           `json:"rejected,omitempty"`
	Result      Result         `json:"result"`
}

func (c *newBeeCreator) runBackupUpload(ctx context.Context, input map[string]any, request Request) (Result, error) {
	result := Result{Platform: "newbee", Action: "creator backup upload"}
	session, err := c.sessionIdentity(ctx)
	if err != nil {
		return result, err
	}
	bundle, err := newBeeReadBundle(String(input, "bundle_file"))
	if err != nil {
		return result, err
	}
	raw, _ := json.Marshal(map[string]any{"bundle": bundle, "input": input})
	sum := sha256.Sum256(raw)
	fingerprint := hex.EncodeToString(sum[:])
	key := sha256.Sum256([]byte(request.RequestID))
	operation := hex.EncodeToString(key[:])
	result.OperationID = operation
	file := filepath.Join(request.Home, "publish", "newbee", operation+".json")
	lease, err := vault.AcquireLease(ctx, filepath.Join(request.Home, "publish", "locks"), "newbee/"+operation)
	if err != nil {
		return result, err
	}
	defer lease.Close()
	receipt := newBeeBackupReceipt{Session: session, Fingerprint: fingerprint, Manifest: bundle.Manifest, Result: result}
	if raw, e := os.ReadFile(file); e == nil {
		if newBeeDecodeStored(raw, &receipt) != nil || receipt.Fingerprint != fingerprint {
			return result, invalid("$request", "request is bound to another bundle")
		}
		if err := newBeeCheckSession(receipt.Session, session); err != nil {
			return result, err
		}
		if receipt.Complete {
			return receipt.Result, nil
		}
		if receipt.Rejected {
			return receipt.Result, failure("publish.newbee_business", "original backup operation was rejected; it will not be replayed or verified from current fields")
		}
		if receipt.Stage == "uploading" || receipt.Stage == "fonts" {
			result.Submission = "uncertain"
			return result, &Error{Code: "publish.newbee_pending", Message: "original object upload is uncertain; retain original bundle and request", Exit: 6}
		}
	} else if !os.IsNotExist(e) {
		return result, e
	}
	save := func() error {
		raw, _ := json.Marshal(receipt)
		if err := os.MkdirAll(filepath.Dir(file), 0700); err != nil {
			return err
		}
		return vault.ReplaceFile(ctx, file, raw)
	}
	reject := func(cause error) (Result, error) {
		if newBeeRejected(cause) {
			receipt.Rejected = true
			result.Submission = "rejected"
			receipt.Result = result
			if err := save(); err != nil {
				return result, err
			}
		}
		return result, cause
	}
	if receipt.Stage == "" {
		if bundle.FontDirectory != "" {
			if err = newBeeFontReady(); err != nil {
				return result, err
			}
		}
		versions, err := c.read(ctx, "plugin game-versions", nil)
		if err != nil {
			return result, err
		}
		if err = newBeeRequireOptions(versions, []any{bundle.Manifest["versionid"]}, []string{"id"}, "$.bundle_file.manifest.versionid"); err != nil {
			return result, err
		}
		value, err := c.read(ctx, "backup list", nil)
		if err != nil {
			return result, err
		}
		for _, row := range newBeeRows(value) {
			receipt.Before = append(receipt.Before, newBeeID(newBeePick(newBeeObject(row), "t_id", "id")))
		}
		receipt.Stage = "prepared"
		if err = save(); err != nil {
			return result, err
		}
	}
	if receipt.ID == 0 && receipt.Stage != "creating" {
		for i := receipt.Uploaded; i < len(bundle.Artifacts); i++ {
			artifact := bundle.Artifacts[i]
			receipt.Stage = "uploading"
			if err = save(); err != nil {
				return result, err
			}
			object, err := c.uploadBackupObject(ctx, filepath.Join(filepath.Dir(String(input, "bundle_file")), artifact.File), artifact)
			if err != nil {
				return reject(err)
			}
			if err = newBeeSetTarget(receipt.Manifest, artifact.Target, object, true); err != nil {
				return result, err
			}
			receipt.Uploaded = i + 1
			receipt.Stage = "prepared"
			if err = save(); err != nil {
				return result, err
			}
		}
		if bundle.FontDirectory != "" && receipt.Manifest["font_hash"] == nil {
			receipt.Stage = "fonts"
			if err = save(); err != nil {
				return result, err
			}
			index, err := newBeeUploadFonts(ctx, filepath.Join(filepath.Dir(String(input, "bundle_file")), bundle.FontDirectory))
			if err != nil {
				return reject(err)
			}
			receipt.Manifest["font_hash"] = index
			receipt.Stage = "prepared"
			if err = save(); err != nil {
				return result, err
			}
		}
		receipt.Stage = "creating"
		if err = save(); err != nil {
			return result, err
		}
		if _, err = c.backupPost(ctx, "/v3/share/create", receipt.Manifest); err != nil {
			return reject(err)
		}
	}
	if receipt.ID == 0 {
		value, err := c.read(ctx, "backup list", nil)
		if err != nil {
			return result, err
		}
		before := map[int64]bool{}
		for _, id := range receipt.Before {
			before[id] = true
		}
		matches := []int64{}
		for _, row := range newBeeRows(value) {
			obj := newBeeObject(row)
			id := newBeeID(newBeePick(obj, "t_id", "id"))
			if !before[id] && newBeeManifestMatches(receipt.Manifest, obj) {
				matches = append(matches, id)
			}
		}
		if len(matches) != 1 {
			result.Submission = "uncertain"
			return result, &Error{Code: "publish.newbee_verification", Message: "created backup could not be uniquely identified; retain original request", Exit: 6}
		}
		receipt.ID = matches[0]
		receipt.Stage = "created"
		if err = save(); err != nil {
			return result, err
		}
	}
	if name := String(input, "name"); name != "" && receipt.Stage != "renaming" && receipt.Stage != "renamed" {
		receipt.Stage = "renaming"
		if err = save(); err != nil {
			return result, err
		}
		if _, err = c.backupPost(ctx, "/share/update", map[string]any{"t_id": receipt.ID, "t_name": name}); err != nil {
			return reject(err)
		}
		receipt.Stage = "renamed"
		if err = save(); err != nil {
			return result, err
		}
	}
	value, err := c.read(ctx, "backup get", map[string]any{"id": json.Number(fmt.Sprint(receipt.ID))})
	if err != nil {
		return result, err
	}
	if name := String(input, "name"); name != "" && fmt.Sprint(newBeePick(newBeeObject(value), "t_name", "name")) != name {
		return result, &Error{Code: "publish.newbee_verification", Message: "backup name differs from readback", Exit: 6}
	}
	result.Complete = true
	result.Submission = "accepted"
	result.Verification = "metadata-readback"
	result.Data = map[string]any{"id": receipt.ID, "uploaded": receipt.Uploaded, "readback": newBeePublic(value)}
	receipt.Result = result
	receipt.Complete = true
	if err = save(); err != nil {
		result.Complete = false
		return result, err
	}
	return result, nil
}
func newBeeContains(expected, actual any) bool {
	actual = newBeeDecoded(actual)
	switch wanted := expected.(type) {
	case map[string]any:
		got, ok := actual.(map[string]any)
		if !ok {
			return false
		}
		for key, value := range wanted {
			if key == "select" || key == "size" || key == "editModeLayouts" {
				continue
			}
			if !newBeeContains(value, got[key]) {
				return false
			}
		}
		return true
	case []any:
		got, ok := actual.([]any)
		if len(wanted) == 0 && (actual == nil || actual == "") {
			return true
		}
		if !ok || len(wanted) != len(got) {
			return false
		}
		used := make([]bool, len(got))
		for _, value := range wanted {
			found := false
			for i, item := range got {
				if !used[i] && newBeeContains(value, item) {
					used[i] = true
					found = true
					break
				}
			}
			if !found {
				return false
			}
		}
		return true
	default:
		if expected == nil {
			return actual == nil || actual == ""
		}
		return newBeeEqual(expected, actual)
	}
}
func newBeeManifestMatches(manifest, row map[string]any) bool {
	for key, remote := range map[string]string{"knowplug_list": "t_Known_plug", "t_unKnown_list": "t_unKnown_list", "t_material_list": "t_material_list", "t_font_list": "t_font_list", "unknown_plug": "t_unKnown_plug", "material": "t_material", "font_hash": "t_font_ncs_hash"} {
		if !newBeeContains(manifest[key], row[remote]) {
			return false
		}
	}
	accounts := []any{}
	for _, raw := range newBeeRows(manifest["wtf"]) {
		copy := map[string]any{}
		for k, v := range newBeeObject(raw) {
			if k != "id" {
				copy[k] = v
			}
		}
		accounts = append(accounts, copy)
	}
	if !newBeeContains(accounts, newBeePick(row, "wtflist", "t_WTF_list")) {
		return false
	}
	return newBeeID(manifest["versionid"]) == newBeeID(row["t_Versionid"])
}
func (c *newBeeCreator) uploadBackupObject(ctx context.Context, path string, artifact newBeeBackupArtifact) (string, error) {
	value, err := c.backupPost(ctx, "/share/generate_upload_url_v2", map[string]any{"file_type": artifact.FileType, "file_size": artifact.Size})
	if err != nil {
		return "", err
	}
	object := newBeeObject(value)
	remote := String(object, "file_path")
	u, err := url.Parse(String(object, "upload_url"))
	if err != nil || remote == "" {
		return "", failure("publish.newbee_upload", "upload preparation omitted required values")
	}
	if u.Scheme == "http" && u.Host == "wclboxdata.cn-hangzhou.oss.aliyuncs.com" {
		u.Scheme = "https"
	}
	if u.Scheme != "https" || u.Host == "" || u.User != nil || u.Fragment != "" {
		return "", failure("publish.newbee_upload", "backup object upload URL must be HTTPS")
	}
	file, err := openArtifact(path)
	if err != nil {
		return "", err
	}
	defer file.Close()
	// Recheck the exact locked handle that will be streamed: bundle admission
	// can precede slow preparation requests or uploads of other selected items.
	hash := sha256.New()
	size, err := io.Copy(hash, file)
	if err != nil || size != artifact.Size || hex.EncodeToString(hash.Sum(nil)) != artifact.SHA256 {
		return "", invalid("$.bundle_file", "artifact bytes changed")
	}
	if _, err = file.Seek(0, io.SeekStart); err != nil {
		return "", err
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodPut, u.String(), file)
	if err != nil {
		return "", err
	}
	req.ContentLength = artifact.Size
	req.Header.Set("Content-Type", "application/x-www-form-urlencoded")
	response, err := c.http.client.Do(req)
	if err != nil {
		return "", failure("publish.newbee_upload", "backup object upload is uncertain")
	}
	defer response.Body.Close()
	_, _ = io.Copy(io.Discard, io.LimitReader(response.Body, 1<<20))
	if response.StatusCode != 200 {
		if response.StatusCode == http.StatusUnauthorized || response.StatusCode == http.StatusForbidden {
			return "", &Error{Code: "publish.http_status", Message: "object storage rejected the backup upload", Exit: 5, Details: map[string]any{"httpStatus": response.StatusCode}}
		}
		return "", failure("publish.newbee_upload", "backup object upload failed")
	}
	return remote, nil
}
