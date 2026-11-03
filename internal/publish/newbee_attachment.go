package publish

import (
	"context"
	"crypto/md5"
	"crypto/sha256"
	"encoding/hex"
	"io"
	"net/http"
	"net/url"
	"path/filepath"
)

func (c *newBeeCreator) uploadAttachment(ctx context.Context, path string) (any, error) {
	file, err := openArtifact(path)
	if err != nil {
		return nil, invalid("$.file", "attachment cannot be read")
	}
	defer file.Close()
	info, err := file.Stat()
	if err != nil || !info.Mode().IsRegular() {
		return nil, invalid("$.file", "attachment must be a regular file")
	}
	md := md5.New()
	sha := sha256.New()
	if _, err = io.Copy(io.MultiWriter(md, sha), file); err != nil {
		return nil, err
	}
	code := hex.EncodeToString(md.Sum(nil))
	digest := hex.EncodeToString(sha.Sum(nil))
	if _, err = file.Seek(0, io.SeekStart); err != nil {
		return nil, err
	}
	prepare, err := c.http.JSON(ctx, http.MethodPost, c.next+"/uploadserver/upload/v3/prepare", nil, map[string]any{"code": code, "indexType": 2, "fileName": filepath.Base(path), "files": []any{map[string]any{"fullHash": code, "totalSize": info.Size(), "chunks": []any{map[string]any{"hash": code, "size": info.Size()}}}}, "checksumAlgorithm": 1})
	if err != nil {
		return nil, err
	}
	value, err := newBeeEnvelope(prepare)
	if err != nil {
		return nil, err
	}
	prepared := newBeeObject(value)
	items, _ := prepared["items"].(map[string]any)
	item := newBeeObject(items[code])
	if item == nil {
		return nil, failure("publish.newbee_attachment", "upload preparation omitted selected object")
	}
	if !Bool(item, "exists") {
		u, err := url.Parse(String(item, "url"))
		if err != nil || u.Scheme != "https" || u.Host == "" || u.User != nil || u.Fragment != "" || String(item, "callback") == "" {
			return nil, failure("publish.newbee_attachment", "object upload credentials are invalid")
		}
		request, err := http.NewRequestWithContext(ctx, http.MethodPut, u.String(), file)
		if err != nil {
			return nil, err
		}
		request.ContentLength = info.Size()
		request.Header.Set("x-oss-callback", String(item, "callback"))
		response, err := c.http.client.Do(request)
		if err != nil {
			return nil, failure("publish.newbee_attachment", "object upload is uncertain")
		}
		_, _ = io.Copy(io.Discard, io.LimitReader(response.Body, 1<<20))
		response.Body.Close()
		if response.StatusCode < 200 || response.StatusCode >= 300 {
			return nil, failure("publish.newbee_attachment", "object upload failed")
		}
	}
	index := String(prepared, "id")
	if index == "" {
		return nil, failure("publish.newbee_attachment", "upload preparation omitted index")
	}
	response, err := c.http.JSON(ctx, http.MethodPost, c.next+"/uploadserver/upload/v3/index/get", nil, map[string]any{"code": index})
	if err != nil {
		return nil, err
	}
	value, err = newBeeEnvelope(response)
	if err != nil {
		return nil, err
	}
	data := newBeeObject(value)
	if String(data, "code") == "" || newBeeID(data["totalSize"]) != info.Size() {
		return nil, &Error{Code: "publish.newbee_verification", Message: "attachment index differs from selected file", Exit: 6}
	}
	return map[string]any{"file_id": 0, "name": filepath.Base(path), "value": String(data, "code"), "size": info.Size(), "timestamp": 0, "sha256": digest}, nil
}
