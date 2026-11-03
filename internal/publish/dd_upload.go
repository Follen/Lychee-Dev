package publish

import (
	"archive/zip"
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
	"strings"
	"time"

	"github.com/follenfang/lycheedev/internal/vault"
)

func ddReadFile(path string, max int64) ([]byte, error) {
	f, err := openArtifact(path)
	if err != nil {
		return nil, err
	}
	defer f.Close()
	b, err := io.ReadAll(io.LimitReader(f, max+1))
	if err != nil {
		return nil, err
	}
	if int64(len(b)) > max {
		return nil, invalid("$.file", "selected file exceeds its supported size")
	}
	return b, nil
}

type ddObjectReceipt struct {
	Author     string `json:"author"`
	SHA256     string `json:"sha256"`
	Kind       string `json:"kind"`
	Bytes      int64  `json:"bytes"`
	URL        string `json:"url,omitempty"`
	Submission string `json:"submission"`
}

func ddObjectURL(raw string, permanent bool) error {
	u, err := url.Parse(raw)
	if err != nil || u.Scheme != "https" || u.Hostname() == "" || u.User != nil || u.Fragment != "" || permanent && u.RawQuery != "" {
		return failure("publish.dd_object", "DD object address is not a supported HTTPS target")
	}
	return nil
}

type ddUploadArtifact struct {
	file         *os.File
	size         int64
	mime, digest string
}

func ddOpenUpload(file, kind string) (*ddUploadArtifact, error) {
	f, err := openArtifact(file)
	if err != nil {
		return nil, err
	}
	good := false
	defer func() {
		if !good {
			f.Close()
		}
	}()
	stat, err := f.Stat()
	if err != nil {
		return nil, err
	}
	image := strings.HasSuffix(kind, "image")
	max := int64(4 << 30)
	if image {
		max = 10 << 20
	}
	if kind == "wa archive" {
		max = 50 << 20
	}
	if stat.Size() <= 0 || stat.Size() > max {
		return nil, invalid("$.file", "selected object is empty or exceeds the platform limit")
	}
	suffix := strings.ToLower(filepath.Ext(file))
	mime := "application/x-zip-compressed"
	if image {
		mime = map[string]string{".png": "image/png", ".jpg": "image/jpeg", ".jpeg": "image/jpeg", ".gif": "image/gif"}[suffix]
		if mime == "" {
			return nil, invalid("$.file", "DD images require PNG, JPEG or GIF")
		}
	} else {
		if suffix != ".zip" {
			return nil, invalid("$.file", "DD archives require ZIP")
		}
		z, e := zip.NewReader(f, stat.Size())
		if e != nil {
			return nil, invalid("$.file", "invalid ZIP")
		}
		for _, entry := range z.File {
			if strings.Contains(entry.Name, "\\") {
				return nil, invalid("$.file", "ZIP entries must use forward slashes")
			}
		}
	}
	hash := sha256.New()
	if _, err = io.Copy(hash, f); err != nil {
		return nil, err
	}
	digest := hex.EncodeToString(hash.Sum(nil))
	good = true
	return &ddUploadArtifact{f, stat.Size(), mime, digest}, nil
}
func ddUpload(ctx context.Context, api ddAPI, home, file, kind string) (map[string]any, error) {
	a, err := ddOpenUpload(file, kind)
	if err != nil {
		return nil, err
	}
	defer a.file.Close()
	f, mime, digest := a.file, a.mime, a.digest
	image := strings.HasSuffix(kind, "image")
	author := ddText(api.identity()["author_id"])
	if author == "" {
		return nil, capability("publish.dd_identity", "task omitted author identity")
	}
	identity := ddDigest(map[string]any{"author": author, "sha256": digest, "kind": kind, "bytes": a.size})
	p := filepath.Join(home, "publish", "dd", "objects", identity+".json")
	lease, err := vault.AcquireLease(ctx, filepath.Join(home, "publish", "locks"), "dd-object/"+identity)
	if err != nil {
		return nil, err
	}
	defer lease.Close()
	if raw, e := os.ReadFile(p); e == nil {
		var old ddObjectReceipt
		if json.Unmarshal(raw, &old) != nil || old.Author != author || old.SHA256 != digest || old.Kind != kind || old.Bytes != a.size {
			return nil, failure("publish.dd_object_receipt", "invalid original object receipt")
		}
		if old.Submission == "acknowledged" && ddObjectURL(old.URL, true) == nil {
			return map[string]any{"url": old.URL, "sha256": digest, "bytes": a.size, "reused": true}, nil
		}
		return nil, &Error{Code: "publish.dd_object_pending", Message: "original object PUT is uncertain; retain original object receipt", Exit: 6}
	} else if !errors.Is(e, os.ErrNotExist) {
		return nil, e
	}
	query := url.Values{"file_type": {"a19-ui-res"}, "business_id": {"addon"}, "mime_type": {mime}}
	if image {
		query.Set("file_type", "a19-ui-media")
		query.Set("business_id", "img")
		if strings.HasPrefix(kind, "plugin") {
			query.Set("file_name", "")
		}
	} else if strings.HasPrefix(kind, "plugin") {
		query.Set("file_name", "addon.zip")
	} else {
		query.Set("business_id", "wa")
		query.Set("file_name", "wa_materials.zip")
	}
	v, err := api.call(ctx, "/file/upload", query, nil, false)
	if err != nil {
		return nil, err
	}
	grant, err := ddObject(v["result"])
	if err != nil {
		return nil, err
	}
	target, download := ddText(grant["url"]), ddText(grant["d_url"])
	if err = ddObjectURL(target, false); err != nil {
		return nil, err
	}
	if err = ddObjectURL(download, true); err != nil {
		return nil, err
	}
	if limit := ddInteger(grant["maxSize"]); limit > 0 && a.size > limit {
		return nil, invalid("$.file", "object exceeds DD server upload limit")
	}
	if _, err = f.Seek(0, io.SeekStart); err != nil {
		return nil, err
	}
	receipt := ddObjectReceipt{author, digest, kind, a.size, "", "uncertain"}
	if err = os.MkdirAll(filepath.Dir(p), 0700); err != nil {
		return nil, err
	}
	raw, _ := json.Marshal(receipt)
	if err = vault.ReplaceFile(ctx, p, raw); err != nil {
		return nil, err
	}
	if err = ddPut(ctx, target, mime, f, a.size); err != nil {
		return nil, &Error{Code: "publish.dd_object_pending", Message: "object PUT requires inspection before another submission", Exit: 6}
	}
	receipt.URL, receipt.Submission = download, "acknowledged"
	raw, _ = json.Marshal(receipt)
	if err = vault.ReplaceFile(ctx, p, raw); err != nil {
		return nil, &Error{Code: "publish.dd_object_pending", Message: "object PUT acknowledged; original receipt needs saving", Exit: 6}
	}
	return map[string]any{"url": download, "sha256": digest, "bytes": a.size, "reused": false}, nil
}
func ddPut(ctx context.Context, target, mime string, body io.Reader, size int64) error {
	if err := ddObjectURL(target, false); err != nil {
		return err
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodPut, target, body)
	if err != nil {
		return failure("publish.dd_object", "invalid PUT target")
	}
	req.ContentLength = size
	req.Header.Set("Content-Type", mime)
	req.Header.Set("x-amz-acl", "public-read")
	client := &http.Client{Timeout: 5 * time.Minute, CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }}
	response, err := client.Do(req)
	if err != nil {
		return failure("publish.dd_object", "object PUT transport failed")
	}
	defer response.Body.Close()
	if response.StatusCode != 200 {
		return failure("publish.dd_object", "object PUT did not return HTTP 200")
	}
	return nil
}
func ddUploadForm(ctx context.Context, api ddAPI, home, r string, form, in map[string]any) error {
	for _, field := range []struct{ input, output, kind string }{{"file", "detail_url", r + " archive"}, {"logo_file", "logo", "plugin image"}} {
		p := String(in, field.input)
		if p == "" {
			continue
		}
		out := field.output
		if r == "wa" && field.input == "file" {
			out = "file_path"
		}
		receipt, err := ddUpload(ctx, api, home, p, field.kind)
		if err != nil {
			return err
		}
		form[out] = receipt["url"]
	}
	for _, field := range []struct{ input, output string }{{"detail_img_files", "detail_imgs"}, {"display_img_files", "display_imgs"}} {
		files := ddSlice(in[field.input])
		if len(files) == 0 {
			continue
		}
		images := ddSlice(form[field.output])
		if len(files)+len(images) > 8 {
			return invalid("$."+field.input, "at most eight display images are allowed")
		}
		for _, v := range files {
			receipt, err := ddUpload(ctx, api, home, ddText(v), r+" image")
			if err != nil {
				return err
			}
			images = append(images, receipt["url"])
		}
		form[field.output] = images
	}
	return nil
}
