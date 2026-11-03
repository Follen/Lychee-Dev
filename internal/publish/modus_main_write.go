package publish

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"io"
	"net/http"
	"net/url"
	"path/filepath"
	"strconv"
	"strings"
)

func inspectModusMedia(ctx context.Context, path string) (artifact, error) {
	var a artifact
	f, err := openArtifact(path)
	if err != nil {
		return a, invalid("$.file", "cannot open image")
	}
	ok := false
	defer func() {
		if !ok {
			f.Close()
		}
	}()
	stat, err := f.Stat()
	if err != nil || !stat.Mode().IsRegular() || stat.Size() < 1 || stat.Size() > 20<<20 {
		return a, invalid("$.file", "image must be a regular file within 20 MiB")
	}
	if err = ctx.Err(); err != nil {
		return a, err
	}
	h := sha256.New()
	if _, err = io.Copy(h, f); err != nil {
		return a, err
	}
	if _, err = f.Seek(0, io.SeekStart); err != nil {
		return a, err
	}
	ok = true
	return artifact{file: f, name: filepath.Base(path), size: stat.Size(), sha256: hex.EncodeToString(h.Sum(nil))}, nil
}
func (c *modusClient) continueMain(ctx context.Context, home string, op *modusOperation) error {
	if op.Action == "media upload" {
		return c.continueMedia(ctx, home, op)
	}
	resource, verb, _ := strings.Cut(op.Action, " ")
	build := Integer(op.Input, "server_type")
	key, path := "share_id", "share"
	if resource == "wa" {
		key, path = "import_id", "import"
	}
	if op.Stage == "prepare" {
		if verb == "create" || verb == "edit" {
			wire, err := c.prepareMain(ctx, op)
			if err != nil {
				return err
			}
			op.Wire = wire
		} else {
			current, err := c.readMain(ctx, resource+" get", op.Input)
			if err != nil {
				return err
			}
			if verb == "version-delete" {
				// The deletion endpoint takes only versionId. Bind that ID to
				// the selected WA before submitting a destructive request.
				versions, ok := current.(map[string]any)["versions"].([]any)
				if !ok {
					return failure("publish.modus_identity", "WA detail does not provide version identities")
				}
				found := false
				for _, row := range versions {
					if modusResponseID(row, "versionId", "id") == String(op.Input, "version_id") {
						found = true
					}
				}
				if !found {
					return invalid("$.version_id", "version does not belong to the selected WA")
				}
			}
		}
		if verb != "create" {
			op.RemoteID = String(op.Input, key)
		}
		if verb == "version-publish" {
			op.Wire = map[string]any{"importId": op.Input["import_id"], "version": op.Input["version"], "codeText": op.Input["code_text"]}
			if v, ok := op.Input["changelog"]; ok {
				op.Wire["changelog"] = v
			}
		}
		op.Stage = "write"
		if err := saveModusOperation(ctx, home, *op); err != nil {
			return err
		}
	}
	if op.Stage == "write" {
		route, method := "system/user/"+path+"/create", "POST"
		switch verb {
		case "edit":
			route = "system/user/" + path + "/update"
			if resource == "config" {
				method = "PUT"
			}
		case "delete":
			method = "DELETE"
			route = "system/user/" + path + "/delete/" + url.PathEscape(op.RemoteID)
		case "version-publish":
			route = "system/user/import/version/publish"
		case "version-delete":
			method = "DELETE"
			route = "system/user/import/version/delete?" + url.Values{"versionId": {String(op.Input, "version_id")}}.Encode()
		}
		op.Submission = "uncertain"
		if err := saveModusOperation(ctx, home, *op); err != nil {
			return err
		}
		v, err := c.call(ctx, method, route, op.Wire, &build)
		if err != nil {
			return modusWriteFailure(ctx, home, op, err)
		}
		if verb == "create" {
			op.RemoteID = modusResponseID(v, "id", "shareId", "importId")
		}
		op.Stage = "readback"
		op.Submission = "submitted"
		if err = saveModusOperation(ctx, home, *op); err != nil {
			return err
		}
	}
	if op.Stage != "readback" {
		return failure("publish.modus_operation", "unknown saved main-client stage")
	}
	err := c.verifyMain(ctx, op)
	if e := saveModusOperation(ctx, home, *op); e != nil {
		return e
	}
	return err
}
func (c *modusClient) continueMedia(ctx context.Context, home string, op *modusOperation) error {
	if op.Stage == "prepare" {
		a, err := inspectModusMedia(ctx, String(op.Input, "file"))
		if err != nil {
			return err
		}
		defer a.file.Close()
		op.ArchiveSHA256 = a.sha256
		op.ArchiveBytes = a.size
		op.Stage = "write"
		op.Submission = "uncertain"
		if err = saveModusOperation(ctx, home, *op); err != nil {
			return err
		}
		headers := http.Header{"Authorization": {c.token}}
		if c.device != "" {
			headers.Set("X-Device-Id", c.device)
		}
		v, err := c.http.Multipart(ctx, c.origin+"game/data/file/upload/file/image", headers, nil, "file", a.file)
		if err != nil {
			return modusWriteFailure(ctx, home, op, err)
		}
		code := fmtModusID(v["code"])
		if v["success"] == false || (code != "" && code != "200") {
			n, parseErr := strconv.ParseInt(code, 10, 64)
			if v["success"] == false || parseErr == nil && n >= 400 {
				return modusWriteFailure(ctx, home, op, failure("publish.modus_rejected", "platform rejected image"))
			}
			return failure("publish.modus_media", "platform rejected image")
		}
		data, ok := v["data"].(map[string]any)
		if !ok {
			return failure("publish.modus_media", "image upload response has no object")
		}
		key := String(data, "cosStoreKey")
		if key == "" {
			key = String(data, "cosStoreUrl")
		}
		if key == "" {
			key = String(data, "key")
		}
		public := String(data, "downloadUrl")
		if public == "" {
			public = String(data, "url")
		}
		if public == "" {
			public = String(data, "fileUrl")
		}
		if key == "" && public != "" {
			u, err := url.Parse(public)
			if err == nil {
				key = strings.TrimLeft(u.Path, "/")
			}
		}
		if key == "" {
			return failure("publish.modus_media", "image upload omitted reusable object key")
		}
		if public == "" {
			public = "https://cdn.modus.cool/" + strings.TrimLeft(key, "/")
		}
		op.RemoteID = key
		op.Wire = map[string]any{"url": public, "key": key}
		op.Stage = "readback"
		op.Submission = "submitted"
		if err = saveModusOperation(ctx, home, *op); err != nil {
			return err
		}
	}
	err := c.verifyMain(ctx, op)
	if e := saveModusOperation(ctx, home, *op); e != nil {
		return e
	}
	return err
}
func (c *modusClient) verifyMain(ctx context.Context, op *modusOperation) error {
	pending := func(message string) error { return &Error{Code: "publish.modus_pending", Message: message, Exit: 6} }
	if op.RemoteID == "" {
		return pending("original remote ID or media key is required for readback")
	}
	if op.Action == "media upload" {
		endpoint, err := modusDownloadURL(String(op.Wire, "url"))
		if err != nil {
			return err
		}
		req, err := http.NewRequestWithContext(ctx, "GET", endpoint, nil)
		if err != nil {
			return err
		}
		resp, err := c.http.client.Do(req)
		if err != nil {
			return pending("public image readback failed")
		}
		defer resp.Body.Close()
		if resp.StatusCode != 200 {
			return pending("public image is not yet available")
		}
		h := sha256.New()
		n, err := io.Copy(h, io.LimitReader(resp.Body, op.ArchiveBytes+1))
		if err != nil || n != op.ArchiveBytes || hex.EncodeToString(h.Sum(nil)) != op.ArchiveSHA256 {
			return pending("public image bytes differ from original upload")
		}
		op.Verified = true
		op.Stage = "complete"
		return nil
	}
	resource, verb, _ := strings.Cut(op.Action, " ")
	key := "share_id"
	if resource == "wa" {
		key = "import_id"
	}
	in := map[string]any{"server_type": op.Input["server_type"], key: op.RemoteID}
	if verb == "delete" {
		seen := map[string]bool{}
		var expected int64 = -1
		for page := int64(1); page <= 1000; page++ {
			in["page_num"] = json.Number(strconv.FormatInt(page, 10))
			in["page_size"] = json.Number("100")
			in["mine"] = true
			v, err := c.readMain(ctx, resource+" list", in)
			if err != nil {
				return err
			}
			rows, err := modusRows(v)
			if err != nil {
				return err
			}
			obj, ok := v.(map[string]any)
			if !ok {
				return pending("deletion needs complete pagination")
			}
			total, ok := positiveOrZero(obj["total"])
			if !ok || expected >= 0 && expected != total {
				return pending("deletion list count is incomplete or changed")
			}
			expected = total
			for _, row := range rows {
				id := modusResponseID(row, "id", "shareId", "importId")
				if id == "" || seen[id] {
					return pending("deletion list has duplicate or missing identity")
				}
				if id == op.RemoteID {
					return pending("selected resource is still present")
				}
				seen[id] = true
			}
			if int64(len(seen)) == total {
				op.Verified = true
				op.Stage = "complete"
				return nil
			}
			if len(rows) == 0 || int64(len(seen)) > total {
				return pending("deletion pagination is incomplete")
			}
		}
		return pending("deletion exceeded pagination bound")
	}
	v, err := c.readMain(ctx, resource+" get", in)
	if err != nil {
		return err
	}
	obj := v.(map[string]any)
	if verb == "version-publish" || verb == "version-delete" {
		versions, ok := obj["versions"].([]any)
		if !ok {
			return pending("WA detail does not provide version readback")
		}
		found := false
		for _, row := range versions {
			version, _ := row.(map[string]any)
			if verb == "version-delete" {
				if modusResponseID(version, "versionId", "id") == String(op.Input, "version_id") {
					return pending("selected WA version is still present")
				}
			} else if version["version"] == op.Input["version"] && version["codeText"] == op.Input["code_text"] {
				if want, present := op.Input["changelog"]; present && !modusSameField("changelog", want, version["changelog"]) {
					continue
				}
				found = true
			}
		}
		if verb == "version-publish" && !found {
			return pending("WA version content differs from original request")
		}
	} else {
		for k, want := range op.Wire {
			if k == "id" {
				continue
			}
			actual := obj[k]
			if k == "contentText" && actual == nil {
				actual = modusPlaintext(fmtString(obj["content"]))
			}
			if !modusSameField(k, want, actual) {
				return pending("saved main-client field differs: " + k)
			}
		}
		if _, cleared := op.Input["required_tier_id"]; cleared && op.Input["required_tier_id"] == nil && obj["requiredTierId"] != nil {
			return pending("required tier was not cleared")
		}
	}
	op.Verified = true
	op.Stage = "complete"
	op.Submission = "submitted"
	return nil
}
