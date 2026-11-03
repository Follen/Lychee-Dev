package publish

import (
	"context"
	"encoding/base64"
	"encoding/hex"
	"net/http"
	"net/url"
	"regexp"
	"strconv"
	"strings"

	cos "github.com/tencentyun/cos-go-sdk-v5"
)

var bbCOSHost = regexp.MustCompile(`^[a-z0-9-]+\.cos\.ap-(shanghai|beijing)\.myqcloud\.com$`)

type bbCOSObject struct {
	Origin      string         `json:"origin"`
	Key         string         `json:"key"`
	URL         string         `json:"url"`
	Credentials map[string]any `json:"credentials"`
	State       string         `json:"state"`
	MD5         string         `json:"md5"`
	Bytes       int64          `json:"bytes"`
}

func bbCOSOrigin(raw string) (*url.URL, error) {
	u, e := url.Parse(raw)
	if e != nil || u.Scheme != "https" || !bbCOSHost.MatchString(u.Host) || u.User != nil || u.RawQuery != "" || u.Fragment != "" || (u.Path != "" && u.Path != "/") {
		return nil, failure("publish.blackbox_cos", "invalid COS origin")
	}
	return u, nil
}
func bbCOSKey(key string) bool {
	if key == "" || strings.HasPrefix(key, "/") || strings.ContainsAny(key, "\\?#\x00\r\n") {
		return false
	}
	for _, part := range strings.Split(key, "/") {
		if part == "" || part == "." || part == ".." {
			return false
		}
	}
	return true
}
func bbCOSCredentials(v any) (map[string]any, error) {
	r, ok := v.(map[string]any)
	if !ok {
		return nil, failure("publish.blackbox_cos", "missing temporary credentials")
	}
	if nested, ok := r["Credentials"].(map[string]any); ok {
		r = nested
	}
	for _, k := range []string{"TmpSecretID", "TmpSecretKey", "Token"} {
		if String(r, k) == "" {
			return nil, failure("publish.blackbox_cos", "incomplete temporary credentials")
		}
	}
	return r, nil
}
func (c *bbClient) cosPut(ctx context.Context, origin, key, path, md5 string, credentials map[string]any) error {
	base, e := bbCOSOrigin(origin)
	if e != nil {
		return e
	}
	if !bbCOSKey(key) {
		return failure("publish.blackbox_cos", "invalid COS object path")
	}
	f, e := openArtifact(path)
	if e != nil {
		return e
	}
	defer f.Close()
	st, e := f.Stat()
	if e != nil {
		return e
	}
	hash, e := hex.DecodeString(md5)
	if e != nil || len(hash) != 16 {
		return failure("publish.blackbox_cos", "invalid archive MD5")
	}
	client := cos.NewClient(&cos.BaseURL{BucketURL: base}, &http.Client{Transport: &cos.AuthorizationTransport{SecretID: String(credentials, "TmpSecretID"), SecretKey: String(credentials, "TmpSecretKey"), SessionToken: String(credentials, "Token"), Transport: c.http.Transport}, Timeout: c.http.Timeout, CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }})
	client.Conf.RetryOpt.Count = 1
	client.Conf.RetryOpt.AutoSwitchHost = false
	response, e := client.Object.Put(ctx, key, f, &cos.ObjectPutOptions{ObjectPutHeaderOptions: &cos.ObjectPutHeaderOptions{ContentLength: st.Size(), ContentMD5: base64.StdEncoding.EncodeToString(hash), XCosStorageClass: "STANDARD"}})
	if e != nil {
		return &Error{Code: "publish.blackbox_cos_uncertain", Message: "COS upload interrupted; original object is not replayed", Exit: 6}
	}
	if response == nil || response.StatusCode < 200 || response.StatusCode >= 300 || strings.ToLower(strings.Trim(response.Header.Get("ETag"), "\"")) != md5 {
		return &Error{Code: "publish.blackbox_cos_uncertain", Message: "COS upload checksum awaits confirmation", Exit: 6}
	}
	return nil
}
func (c *bbClient) uploadModule(ctx context.Context, op *bbOperation) (string, error) {
	a := op.Archive
	if a == nil {
		return "", failure("publish.blackbox_artifact", "original module archive missing")
	}
	if e := bbCheckArchive(a); e != nil {
		return "", e
	}
	if a.COS != nil {
		o := a.COS
		if o.State != "uncertain" && o.State != "uploaded" {
			return "", failure("publish.blackbox_operation", "invalid original COS state")
		}
		if o.State == "uncertain" {
			if e := c.cosHead(ctx, o.Origin, o.Key, o.MD5, o.Bytes, o.Credentials); e != nil {
				return "", e
			}
			o.State = "uploaded"
			if e := c.saveOperation(ctx, op); e != nil {
				return "", e
			}
		}
		return o.URL, nil
	}
	token, e := c.call(ctx, "POST", "/wow/cos/upload/token/", nil, map[string]any{"jsonData": bbJSON(map[string]any{"upload_infos": []any{map[string]any{"filename": a.Name, "file_size": float64(a.Bytes) / 1024 / 1024, "type": "module"}}})})
	if e != nil {
		return "", e
	}
	info, e := bbObject(token["info"])
	if e != nil {
		return "", e
	}
	files, e := bbRows(token["files"], "")
	if e != nil || len(files) != 1 {
		return "", failure("publish.blackbox_cos", "module upload token must identify one file")
	}
	creds, e := bbCOSCredentials(info["Credentials"])
	if e != nil {
		return "", e
	}
	region := String(info, "region")
	if region == "" {
		region = "ap-shanghai"
	}
	origin := "https://" + String(info, "bucket") + ".cos." + region + ".myqcloud.com"
	key := String(files[0], "key")
	host := String(info, "host")
	if host != "wow.max-c.com" || !strings.HasPrefix(key, "wow/module/") {
		return "", failure("publish.blackbox_cos", "module token has unexpected download origin")
	}
	if _, e = bbCOSOrigin(origin); e != nil {
		return "", e
	}
	if !bbCOSKey(key) {
		return "", failure("publish.blackbox_cos", "invalid module upload key")
	}
	_, md5, _, e := modusBackupHash(ctx, a.Path)
	if e != nil {
		return "", e
	}
	a.COS = &bbCOSObject{Origin: origin, Key: key, URL: "https://" + host + "/" + key, Credentials: creds, State: "uncertain", MD5: md5, Bytes: a.Bytes}
	if e = c.saveOperation(ctx, op); e != nil {
		return "", e
	}
	if e = c.cosPut(ctx, origin, key, a.Path, md5, creds); e != nil {
		return "", e
	}
	a.COS.State = "uploaded"
	if e = c.saveOperation(ctx, op); e != nil {
		return "", e
	}
	return a.COS.URL, nil
}
func (c *bbClient) uploadBackupArchive(ctx context.Context, a *bbBackupArchive, token map[string]any) (string, string, error) {
	origin := String(token, "cos")
	if _, e := bbCOSOrigin(origin); e != nil {
		return "", "", e
	}
	creds, e := bbCOSCredentials(token["Credentials"])
	if e != nil {
		return "", "", e
	}
	host, key := String(token, "host"), String(token, "path")
	if host == "" || !bbCOSKey(key) {
		return "", "", failure("publish.blackbox_cos", "incomplete backup upload target")
	}
	sha, md5, n, e := modusBackupHash(ctx, a.Local)
	if e != nil {
		return "", "", e
	}
	if sha != a.SHA256 || md5 != a.MD5 || n != a.Bytes {
		return "", "", failure("publish.blackbox_artifact", "original backup archive changed")
	}
	if e = c.cosPut(ctx, origin, key, a.Local, md5, creds); e != nil {
		return "", "", e
	}
	return host, key, nil
}
func (c *bbClient) cosHead(ctx context.Context, origin, key, md5 string, size int64, credentials map[string]any) error {
	base, e := bbCOSOrigin(origin)
	if e != nil {
		return e
	}
	if !bbCOSKey(key) {
		return failure("publish.blackbox_cos", "invalid original object key")
	}
	if _, e = bbCOSCredentials(credentials); e != nil {
		return e
	}
	client := cos.NewClient(&cos.BaseURL{BucketURL: base}, &http.Client{Transport: &cos.AuthorizationTransport{SecretID: String(credentials, "TmpSecretID"), SecretKey: String(credentials, "TmpSecretKey"), SessionToken: String(credentials, "Token"), Transport: c.http.Transport}, Timeout: c.http.Timeout, CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }})
	client.Conf.RetryOpt.Count = 1
	client.Conf.RetryOpt.AutoSwitchHost = false
	response, e := client.Object.Head(ctx, key, nil)
	if e != nil || response == nil {
		return bbVerification()
	}
	n, e := strconv.ParseInt(response.Header.Get("Content-Length"), 10, 64)
	if e != nil || n != size || strings.ToLower(strings.Trim(response.Header.Get("ETag"), "\"")) != md5 {
		return bbVerification()
	}
	return nil
}
func (c *bbClient) verifyBackupObject(ctx context.Context, a *bbBackupArchive, token map[string]any) error {
	creds, e := bbCOSCredentials(token["Credentials"])
	if e != nil {
		return e
	}
	return c.cosHead(ctx, String(token, "cos"), String(token, "path"), a.MD5, a.Bytes, creds)
}
