package publish

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"net/url"
	"os"
	"path/filepath"
	"strconv"
	"strings"

	"github.com/follenfang/lycheedev/internal/vault"
)

func prepareModusBackupUpload(ctx context.Context, home, dir string, s *modusBackupState) error {
	if s.Manifest == "" {
		if s.Action == "backup create" {
			in := map[string]any{}
			for k, v := range s.Input {
				in[k] = v
			}
			in["__request"] = s.ID
			in["__input_digest"] = s.InputDigest
			candidate := filepath.Join(String(in, "output_dir"), "manifest.v2.json")
			if raw, err := os.ReadFile(filepath.Join(String(in, "output_dir"), ".lycheedev-create.json")); err == nil {
				var marker map[string]string
				if json.Unmarshal(raw, &marker) != nil || marker["request"] != s.ID || marker["inputDigest"] != s.InputDigest {
					return invalid("$.output_dir", "output belongs to a different generation")
				}
				if _, err = os.Stat(candidate); err != nil {
					return capability("publish.modus_backup_local_interrupted", "local generation stopped before the manifest was completed; retained files are in the original output directory")
				}
				s.Manifest = candidate
			} else {
				v, err := generateModusBackup(ctx, in, false)
				if err != nil {
					return err
				}
				s.Manifest = v.(map[string]any)["manifest_file"].(string)
			}
		} else {
			s.Manifest = String(s.Input, "manifest_file")
		}
		absolute, err := filepath.Abs(s.Manifest)
		if err != nil {
			return err
		}
		s.Manifest = absolute
		if err = saveModusBackup(ctx, home, s); err != nil {
			return err
		}
	}
	manifestLock, err := openArtifact(s.Manifest)
	if err != nil {
		return err
	}
	defer manifestLock.Close()
	manifest, locals, err := loadModusBackupManifest(ctx, s.Manifest, s.Build)
	if err != nil {
		return err
	}
	claimPath := filepath.Join(filepath.Dir(s.Manifest), ".lycheedev-cloud-upload.json")
	absHome, err := filepath.Abs(home)
	if err != nil {
		return err
	}
	claim := map[string]string{"request": s.ID, "home": absHome}
	claimRaw, _ := json.Marshal(claim)
	f, err := os.OpenFile(claimPath, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0600)
	if errors.Is(err, os.ErrExist) {
		existing, err := modusReadBounded(claimPath, 4096)
		if err != nil || string(existing) != string(claimRaw) {
			return modusBackupPending("this manifest has an original upload operation; continue its original home and request")
		}
	} else if err != nil {
		return err
	} else {
		_, err = f.Write(claimRaw)
		closeErr := f.Close()
		if err != nil {
			return err
		}
		if closeErr != nil {
			return closeErr
		}
	}
	manifestSHA, _, _, err := modusBackupHash(ctx, s.Manifest)
	if err != nil {
		return err
	}
	s.ManifestSHA = manifestSHA
	artifactDir, err := os.MkdirTemp(dir, "artifacts-")
	if err != nil {
		return err
	}
	objects := []modusBackupObject{}
	for i, local := range locals {
		name := local.Section + "_" + filepath.Base(artifactDir) + "_" + strconv.Itoa(i) + ".zip"
		filename := filepath.Join(artifactDir, name)
		if local.Direct != "" {
			name = filepath.Base(local.Direct)
			filename = filepath.Join(artifactDir, strconv.Itoa(i)+"_"+name)
			source, err := openArtifact(local.Direct)
			if err != nil {
				return err
			}
			target, err := os.OpenFile(filename, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0600)
			if err != nil {
				source.Close()
				return err
			}
			_, copyErr := io.Copy(target, source)
			source.Close()
			closeErr := target.Close()
			if copyErr != nil {
				return copyErr
			}
			if closeErr != nil {
				return closeErr
			}
		} else if err = writeModusBackupZIP(ctx, filename, local.Entries); err != nil {
			return err
		}
		sha, digest, size, err := modusBackupHash(ctx, filename)
		if err != nil {
			return err
		}
		if size > 200<<20 {
			return invalid("$.manifest_file", "backup component exceeds 200 MiB")
		}
		if local.Section == "fonts" {
			name = digest
		}
		objects = append(objects, modusBackupObject{Name: name, File: filename, Type: local.Type, ContentType: local.ContentType, SHA256: sha, MD5: digest, Size: size, Phase: "prepared", Section: local.Section, Account: local.Account, Roles: local.Roles})
	}
	// Preserve the native manifest and strip local source paths recursively.
	manifest["storageTargets"] = append(manifest["storageTargets"].([]any), "cloud")
	sanitizeModusBackup(manifest)
	raw, err := json.MarshalIndent(manifest, "", "  ")
	if err != nil {
		return err
	}
	filename := filepath.Join(artifactDir, "manifest.v2.json")
	if err = os.WriteFile(filename, raw, 0600); err != nil {
		return err
	}
	sha, digest, size, err := modusBackupHash(ctx, filename)
	if err != nil {
		return err
	}
	objects = append(objects, modusBackupObject{Name: String(manifest, "backupId") + "_manifest.v2.json", File: filename, Type: 2, ContentType: "application/json", SHA256: sha, MD5: digest, Size: size, Phase: "prepared", Section: "manifest"})
	s.ManifestData = manifest
	s.Objects = objects
	s.Phase = "font-lookup"
	return saveModusBackup(ctx, home, s)
}
func verifyModusBackupObject(ctx context.Context, c *modusClient, o *modusBackupObject) error {
	endpoint, err := modusDownloadURL(o.Key)
	if err != nil {
		return err
	}
	req, err := http.NewRequestWithContext(ctx, "GET", endpoint, nil)
	if err != nil {
		return err
	}
	response, err := c.http.client.Do(req)
	if err != nil {
		return modusBackupPending("original object upload requires byte readback")
	}
	defer response.Body.Close()
	if response.StatusCode != 200 {
		return modusBackupPending("original object is not yet available for byte readback")
	}
	h := sha256.New()
	n, err := io.Copy(h, io.LimitReader(response.Body, o.Size+1))
	if err != nil || n != o.Size || hex.EncodeToString(h.Sum(nil)) != o.SHA256 {
		return modusBackupPending("uploaded object does not yet match original bytes")
	}
	return nil
}
func sendModusBackupObject(ctx context.Context, c *modusClient, home string, s *modusBackupState, o *modusBackupObject, verifyOnly bool) error {
	if o.Phase == "stored" || o.Phase == "existing-font" {
		return nil
	}
	if o.Phase == "storage-intent" {
		return modusBackupPending("original storage registration has an unknown outcome; it will not be replayed")
	}
	if o.Phase == "put-intent" {
		if err := verifyModusBackupObject(ctx, c, o); err != nil {
			return err
		}
		o.Phase = "put-done"
		if err := saveModusBackup(ctx, home, s); err != nil {
			return err
		}
	}
	if verifyOnly {
		return modusBackupPending("known unsubmitted backup stages remain; resume the original operation")
	}
	f, err := openArtifact(o.File)
	if err != nil {
		return invalid("$.manifest_file", "retained original artifact is unavailable")
	}
	defer f.Close()
	h := sha256.New()
	n, err := io.Copy(h, f)
	if err != nil || n != o.Size || hex.EncodeToString(h.Sum(nil)) != o.SHA256 {
		return invalid("$.manifest_file", "retained original artifact changed")
	}
	if _, err = f.Seek(0, io.SeekStart); err != nil {
		return err
	}
	if o.Phase == "prepared" {
		sum := sha256.Sum256([]byte(s.Account + o.Name + "modus_upload_sign_key_2026"))
		query := url.Values{"fileName": {o.Name}, "fileType": {strconv.Itoa(o.Type)}, "sign": {hex.EncodeToString(sum[:])}}
		v, err := c.call(ctx, "POST", "game/data/file/getSignature?"+query.Encode(), nil, nil)
		if err != nil {
			return err
		}
		signature, ok := v.(map[string]any)
		if !ok {
			return failure("publish.modus_backup_signature", "signature response must contain URL and key")
		}
		signed, key := String(signature, "signedUrl"), String(signature, "key")
		u, err := url.Parse(signed)
		if err != nil || u.Scheme != "https" || u.Hostname() == "" || u.User != nil || u.Fragment != "" || key == "" || strings.ContainsAny(key, "?#\r\n\x00") || strings.Contains(key, "://") {
			return failure("publish.modus_backup_signature", "invalid signed object endpoint or key")
		}
		o.Key = key
		o.Phase = "put-intent"
		if err = saveModusBackup(ctx, home, s); err != nil {
			return err
		}
		req, err := http.NewRequestWithContext(ctx, "PUT", signed, f)
		if err != nil {
			return failure("publish.modus_backup_signature", "invalid upload endpoint")
		}
		req.ContentLength = o.Size
		req.Header.Set("Content-Type", o.ContentType)
		response, err := c.http.client.Do(req)
		if err != nil {
			if ctx.Err() != nil {
				return ctx.Err()
			}
			return modusBackupPending("object upload outcome is unknown; verify original object")
		}
		response.Body.Close()
		if response.StatusCode < 200 || response.StatusCode >= 300 {
			return modusBackupPending("object upload returned unsuccessful status; verify original object")
		}
		o.Phase = "put-done"
		if err = saveModusBackup(ctx, home, s); err != nil {
			return err
		}
	}
	if o.Phase != "put-done" {
		return failure("publish.modus_backup_state", "unknown original object phase")
	}
	record := map[string]any{"fileType": o.Type, "cosStoreUrl": o.Key, "originalFileName": o.Name, "fileName": o.Name, "fileSize": o.Size, "md5": o.MD5}
	o.Phase = "storage-intent"
	if err = saveModusBackup(ctx, home, s); err != nil {
		return err
	}
	if _, err = c.call(ctx, "POST", "game/data/file/upload/data/storage", record, nil); err != nil {
		return modusBackupWriteFailure(ctx, home, s, err)
	}
	o.Phase = "stored"
	return saveModusBackup(ctx, home, s)
}
func modusBackupJSONString(v any) string { raw, _ := json.Marshal(v); return string(raw) }
func buildModusBackupBody(s *modusBackupState) map[string]any {
	sections := map[string][]any{}
	for _, v := range s.ManifestData["sections"].([]any) {
		section := v.(map[string]any)
		sections[String(section, "key")] = section["items"].([]any)
	}
	known := sections["addons"]
	names := []string{}
	display := []any{}
	projects, files := []string{}, []string{}
	for _, v := range known {
		item := v.(map[string]any)
		meta, _ := item["meta"].(map[string]any)
		names = append(names, String(item, "name"))
		display = append(display, map[string]any{"name": item["name"], "icon": fmtString(meta["icon"]), "author": fmtString(meta["author"]), "isLocked": meta["isLocked"] == true})
		projects = append(projects, fmtModusID(meta["projectId"]))
		file := ""
		if meta["isLocked"] == true {
			file = fmtModusID(meta["fileId"])
		}
		files = append(files, file)
	}
	knownInfo := map[string]any{"list": names, "displayList": display, "fileIds": strings.Join(files, ","), "projectIds": strings.Join(projects, ","), "projectids": strings.Join(projects, ",")}
	objects := map[string]modusBackupObject{}
	fonts, accounts := []any{}, []any{}
	manifestKey := ""
	for _, o := range s.Objects {
		switch o.Section {
		case "fonts":
			fonts = append(fonts, map[string]any{"name": filepath.Base(o.File)[strings.Index(filepath.Base(o.File), "_")+1:], "md5": o.MD5, "size": o.Size})
		case "wtf":
			accounts = append(accounts, map[string]any{"name": o.Account, "cosPath": o.Key, "md5": o.MD5, "size": o.Size, "layoutVersion": 1, "roles": o.Roles})
		case "manifest":
			manifestKey = o.Key
		default:
			objects[o.Section] = o
		}
	}
	info := func(key string) string {
		names := []any{}
		for _, v := range sections[key] {
			names = append(names, v.(map[string]any)["name"])
		}
		o := objects[key]
		return modusBackupJSONString(map[string]any{"list": names, "zipPath": o.Key, "md5": o.MD5, "size": o.Size, "layoutVersion": 1})
	}
	return map[string]any{"serverType": s.Build, "backupName": s.ManifestData["backupName"], "knownAddons": modusBackupJSONString(knownInfo), "knownAddonsNum": len(known), "modifiedNum": len(sections["modifiedAddons"]), "modifiedCosInfo": info("modifiedAddons"), "unknownAddons": info("unknownAddons"), "unknownAddonsNum": len(sections["unknownAddons"]), "materialNum": len(sections["materials"]), "materialCosInfo": info("materials"), "wtfNum": len(accounts), "wtfAccounts": modusBackupJSONString(accounts), "fonts": modusBackupJSONString(fonts), "fontNum": len(fonts), "manifestPath": manifestKey}
}
func continueModusBackupUpload(ctx context.Context, c *modusClient, home, dir string, s *modusBackupState, verifyOnly bool) error {
	if s.Phase == "rejected" {
		return failure("publish.modus_rejected", "the original write was rejected and will not be replayed")
	}
	if s.Phase == "prepare" {
		if verifyOnly {
			return modusBackupPending("backup upload has not been prepared")
		}
		if err := prepareModusBackupUpload(ctx, home, dir, s); err != nil {
			return err
		}
	}
	if s.Phase != "link" && s.Phase != "complete" {
		sha, _, _, err := modusBackupHash(ctx, s.Manifest)
		if err != nil || sha != s.ManifestSHA {
			return invalid("$.manifest_file", "original manifest changed during upload")
		}
	}
	if s.Phase == "font-lookup" {
		if verifyOnly {
			return modusBackupPending("unsubmitted backup components remain")
		}
		hashes := []string{}
		for _, o := range s.Objects {
			if o.Section == "fonts" {
				hashes = append(hashes, o.MD5)
			}
		}
		if len(hashes) > 0 {
			v, err := c.call(ctx, "POST", "game/data/file/upload/check/font", map[string]any{"fontMd5": hashes}, nil)
			if err != nil {
				return err
			}
			obj, ok := v.(map[string]any)
			if !ok {
				return failure("publish.modus_backup_fonts", "font lookup response is invalid")
			}
			missing, ok := obj["notExist"].([]any)
			if !ok {
				return failure("publish.modus_backup_fonts", "font lookup omitted notExist")
			}
			allowed := map[string]bool{}
			for _, hash := range hashes {
				allowed[hash] = true
			}
			wanted := map[string]bool{}
			for _, v := range missing {
				hash, ok := v.(string)
				if !ok || !allowed[hash] || wanted[hash] {
					return failure("publish.modus_backup_fonts", "font lookup contains duplicate or foreign hashes")
				}
				wanted[hash] = true
			}
			for i := range s.Objects {
				if o := &s.Objects[i]; o.Section == "fonts" && !wanted[o.MD5] {
					o.Phase = "existing-font"
				}
			}
		}
		s.Phase = "objects"
		if err := saveModusBackup(ctx, home, s); err != nil {
			return err
		}
	}
	if s.Phase == "objects" {
		for i := range s.Objects {
			if err := sendModusBackupObject(ctx, c, home, s, &s.Objects[i], verifyOnly); err != nil {
				return err
			}
		}
		s.Body = buildModusBackupBody(s)
		s.Phase = "register"
		if err := saveModusBackup(ctx, home, s); err != nil {
			return err
		}
	}
	if s.Phase == "register" {
		if verifyOnly {
			return modusBackupPending("all objects retained; resume to register original backup")
		}
		rows, err := modusBackupList(ctx, c, s.Build)
		if err != nil {
			return err
		}
		s.Before = []string{}
		for _, row := range rows {
			s.Before = append(s.Before, modusResponseID(row, "id", "backupId"))
		}
		s.Phase = "register-intent"
		if err = saveModusBackup(ctx, home, s); err != nil {
			return err
		}
		v, err := c.call(ctx, "POST", "system/user/backup/create", s.Body, &s.Build)
		if err != nil {
			return modusBackupWriteFailure(ctx, home, s, err)
		}
		id := modusResponseID(v, "id", "backupId")
		if id != "" {
			if _, err = modusBackupID(id); err != nil {
				return err
			}
			s.CloudID = id
		}
		s.Phase = "readback"
		if err = saveModusBackup(ctx, home, s); err != nil {
			return err
		}
	}
	if s.Phase == "register-intent" || s.Phase == "readback" {
		if err := verifyModusBackupRegistration(ctx, c, s); err != nil {
			return err
		}
		s.Phase = "link"
		if err := saveModusBackup(ctx, home, s); err != nil {
			return err
		}
	}
	if s.Phase == "link" {
		source := s.ManifestData["source"].(map[string]any)
		source["cloudBackupId"] = s.CloudID
		raw, err := json.MarshalIndent(s.ManifestData, "", "  ")
		if err != nil {
			return err
		}
		current, err := modusReadBounded(s.Manifest, 4<<20)
		if err != nil {
			return err
		}
		sum := sha256.Sum256(current)
		if hex.EncodeToString(sum[:]) != s.ManifestSHA && string(current) != string(raw) {
			return invalid("$.manifest_file", "original local manifest changed before cloud linking")
		}
		if err = vault.ReplaceFile(ctx, s.Manifest, raw); err != nil {
			return err
		}
		s.Phase = "complete"
		s.Complete = true
		return saveModusBackup(ctx, home, s)
	}
	return failure("publish.modus_backup_state", "unknown saved upload phase")
}
func verifyModusBackupRegistration(ctx context.Context, c *modusClient, s *modusBackupState) error {
	rows, err := modusBackupList(ctx, c, s.Build)
	if err != nil {
		return err
	}
	before := map[string]bool{}
	for _, id := range s.Before {
		before[id] = true
	}
	found := []string{}
	for _, row := range rows {
		id := modusResponseID(row, "id", "backupId")
		if s.CloudID != "" && id != s.CloudID {
			continue
		}
		if s.CloudID == "" && before[id] {
			continue
		}
		detail, err := modusBackupDetail(ctx, c, id, s.Build)
		if err != nil {
			return err
		}
		if detail["manifestPath"] == s.Body["manifestPath"] && detail["backupName"] == s.Body["backupName"] {
			for key, want := range s.Body {
				if !modusSameField(key, want, detail[key]) {
					return modusBackupPending("created backup fields differ from original upload: " + key)
				}
			}
			found = append(found, id)
		}
	}
	if len(found) != 1 {
		return modusBackupPending("original backup registration has not been uniquely verified; it will not be replayed")
	}
	s.CloudID = found[0]
	return nil
}
