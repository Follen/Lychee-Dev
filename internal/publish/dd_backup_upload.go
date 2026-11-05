package publish

import (
	"archive/zip"
	"bytes"
	"context"
	"crypto/md5"
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

type ddBackupOpen struct {
	bundle ddBackupBundle
	files  map[string]*os.File
}

func (b *ddBackupOpen) close() {
	for _, f := range b.files {
		f.Close()
	}
}
func ddWTFMember(parts []string, selection map[string]any) bool {
	if len(parts) == 2 && parts[0] == "WTF" {
		return Bool(selection, "include_wtf_root") && !strings.EqualFold(parts[1], "Account")
	}
	if len(parts) < 4 || parts[0] != "WTF" || parts[1] != "Account" {
		return false
	}
	for _, v := range ddSlice(selection["wtf_accounts"]) {
		a := ddMap(v)
		if ddText(a["account"]) != parts[2] {
			continue
		}
		common := Bool(a, "include_common")
		if len(parts) == 4 {
			return common && !strings.EqualFold(parts[3], "SavedVariables")
		}
		if strings.EqualFold(parts[3], "SavedVariables") {
			return common
		}
		for _, r := range ddSlice(a["roles"]) {
			role := ddMap(r)
			if ddText(role["server"]) != parts[3] {
				continue
			}
			if len(parts) == 5 {
				return common && !strings.EqualFold(parts[4], "SavedVariables") && ddText(role["name"]) != parts[4]
			}
			if strings.EqualFold(parts[4], "SavedVariables") {
				return common
			}
			if ddText(role["name"]) == parts[4] {
				return true
			}
		}
	}
	return false
}
func ddLoadBundle(path string) (*ddBackupOpen, error) {
	raw, err := ddReadFile(path, 4<<20)
	if err != nil {
		return nil, err
	}
	var b ddBackupBundle
	d := json.NewDecoder(bytes.NewReader(raw))
	d.UseNumber()
	d.DisallowUnknownFields()
	if err = d.Decode(&b); err != nil || b.Schema != "lycheedev.publish.dd-backup.v1" || ddGame(b.Manifest) <= 0 {
		return nil, invalid("$.file", "invalid DD backup bundle")
	}
	if d.Decode(new(any)) != io.EOF {
		return nil, invalid("$.file", "trailing DD backup bundle data")
	}
	opened := &ddBackupOpen{bundle: b, files: map[string]*os.File{}}
	good := false
	defer func() {
		if !good {
			opened.close()
		}
	}()
	if len(b.Manifest) != 6 || len(b.Artifacts) == 0 || len(b.Artifacts) > 5 {
		return nil, invalid("$.file", "invalid backup component set")
	}
	root, err := filepath.Abs(filepath.Dir(path))
	if err != nil {
		return nil, err
	}
	groups := map[string]bool{}
	for _, g := range ddBackupGroups {
		groups[g] = true
		group := ddMap(b.Manifest[g])
		field := "items"
		if g == "wtf" {
			field = "accounts"
		}
		if len(group) != 3 || group[field] == nil || group["md5"] == nil || group["size"] == nil {
			return nil, invalid("$.file", "invalid backup component metadata")
		}
	}
	// The selected account/role tree must match the uploaded metadata exactly.
	wantedAccounts := []any{}
	for _, v := range ddSlice(b.Selection["wtf_accounts"]) {
		a := ddMap(v)
		servers := []any{}
		index := map[string]map[string]any{}
		for _, r := range ddSlice(a["roles"]) {
			role := ddMap(r)
			s, n := ddText(role["server"]), ddText(role["name"])
			if !ddName(s) || !ddName(n) {
				return nil, invalid("$.file", "invalid selected backup role")
			}
			row := index[s]
			if row == nil {
				row = map[string]any{"name": s, "items": []any{}}
				index[s] = row
				servers = append(servers, row)
			}
			row["items"] = append(ddSlice(row["items"]), n)
		}
		wantedAccounts = append(wantedAccounts, map[string]any{"name": a["account"], "servers": servers, "wa_list": []any{}})
	}
	if !ddEqual(wantedAccounts, ddMap(b.Manifest["wtf"])["accounts"]) {
		return nil, invalid("$.file", "WTF selection differs from native metadata")
	}
	for _, a := range b.Artifacts {
		if !groups[a.Group] || opened.files[a.Group] != nil || !ddName(a.File) {
			return nil, invalid("$.file", "duplicate or unknown backup artifact")
		}
		full, err := ddBackupPath(root, a.File)
		if err != nil {
			return nil, err
		}
		f, err := openArtifact(full)
		if err != nil {
			return nil, err
		}
		opened.files[a.Group] = f
		info, err := f.Stat()
		if err != nil {
			return nil, err
		}
		if info.Size() != a.Size || a.Size <= 0 {
			return nil, invalid("$.file", "backup artifact size changed")
		}
		m, s := md5.New(), sha256.New()
		if _, err = io.Copy(io.MultiWriter(m, s), f); err != nil {
			return nil, err
		}
		if hex.EncodeToString(m.Sum(nil)) != a.MD5 || hex.EncodeToString(s.Sum(nil)) != a.SHA256 || ddText(ddMap(b.Manifest[a.Group])["md5"]) != a.MD5 || ddInteger(ddMap(b.Manifest[a.Group])["size"]) != a.Size {
			return nil, invalid("$.file", "backup artifact digest changed")
		}
		z, err := zip.NewReader(f, a.Size)
		if err != nil {
			return nil, invalid("$.file", "backup artifact is not ZIP")
		}
		allowed := map[string]bool{}
		for _, v := range ddSlice(ddMap(b.Manifest[a.Group])["items"]) {
			if a.Group == "known_addon" {
				for _, n := range ddSlice(ddMap(v)["dirs"]) {
					allowed[ddText(n)] = true
				}
			} else {
				allowed[ddGroupKey(v, "name")] = true
			}
		}
		seen := map[string]bool{}
		roles := map[string]bool{}
		for _, entry := range z.File {
			if entry.FileInfo().IsDir() {
				continue
			}
			parts := strings.Split(entry.Name, "/")
			for _, p := range parts {
				if !ddName(p) {
					return nil, invalid("$.file", "backup ZIP has unsafe paths")
				}
			}
			key := strings.ToLower(entry.Name)
			if seen[key] || entry.Mode()&os.ModeSymlink != 0 {
				return nil, invalid("$.file", "backup ZIP has duplicate paths or links")
			}
			seen[key] = true
			selected := allowed[parts[0]]
			if a.Group == "font" {
				selected = len(parts) == 2 && parts[0] == "Fonts" && allowed[parts[1]]
			}
			if a.Group == "wtf" {
				selected = ddWTFMember(parts, b.Selection)
				if len(parts) >= 6 {
					roles[strings.Join(parts[2:5], "/")] = true
				}
			}
			if !selected {
				return nil, invalid("$.file", "backup ZIP contains unselected content")
			}
		}
		if a.Group == "wtf" {
			for _, v := range ddSlice(b.Selection["wtf_accounts"]) {
				account := ddMap(v)
				for _, r := range ddSlice(account["roles"]) {
					role := ddMap(r)
					if !roles[ddText(account["account"])+"/"+ddText(role["server"])+"/"+ddText(role["name"])] {
						return nil, invalid("$.file", "backup ZIP omitted selected role")
					}
				}
			}
		}
		if _, err = f.Seek(0, io.SeekStart); err != nil {
			return nil, err
		}
	}
	for _, g := range ddBackupGroups {
		if opened.files[g] == nil {
			group := ddMap(b.Manifest[g])
			field := "items"
			if g == "wtf" {
				field = "accounts"
			}
			if ddInteger(group["size"]) != 0 || ddText(group["md5"]) != "" || len(ddSlice(group[field])) > 0 {
				return nil, invalid("$.file", "selected backup group has no artifact")
			}
		}
	}
	good = true
	return opened, nil
}
func ddBackupUpload(ctx context.Context, api ddAPI, req Request, in map[string]any) (Result, error) {
	b, err := ddLoadBundle(String(in, "file"))
	if err != nil {
		return Result{}, err
	}
	defer b.close()
	author := ddText(api.identity()["author_id"])
	digest := ddDigest(map[string]any{"bundle": b.bundle, "name": in["name"]})
	lease, err := vault.AcquireLease(ctx, filepath.Join(req.Home, "publish", "locks"), "dd/"+author)
	if err != nil {
		return Result{}, err
	}
	defer lease.Close()
	old, err := ddLoad(req.Home, req.RequestID)
	if err == nil {
		if old.Author != author || old.Digest != digest || old.Action != "backup upload" {
			return Result{}, invalid("$request", "request identifies different backup")
		}
		if old.Verification == "verified" {
			return old.result("backup upload"), nil
		}
		return ddPending(old, "backup upload")
	}
	if !errors.Is(err, os.ErrNotExist) {
		return Result{}, err
	}
	options, err := api.call(ctx, "/game_type/list", nil, nil, false)
	if err != nil {
		return Result{}, err
	}
	games, err := ddItems(options["result"])
	if err != nil {
		return Result{}, err
	}
	available := false
	for _, game := range games {
		if ddGame(ddMap(game)) == ddGame(b.bundle.Manifest) {
			available = true
			break
		}
	}
	if !available {
		return Result{}, invalid("$.game_type", "backup game type is absent from current platform options")
	}
	file, err := filepath.Abs(String(in, "file"))
	if err != nil {
		return Result{}, err
	}
	expected := ddClone(b.bundle.Manifest)
	if name := String(in, "name"); name != "" {
		expected["name"] = name
	}
	r := ddReceipt{Schema: "lycheedev.publish.dd-operation.v1", Request: req.RequestID, Session: String(in, "session"), Author: author, Action: "backup upload", Digest: digest, Game: ddGame(b.bundle.Manifest), Expected: expected, Submission: "uncertain", Verification: "pending", Created: time.Now().UTC(), Backup: &ddBackupProgress{File: file, Stage: "allocate", Components: map[string]string{}}}
	if err = ddSave(ctx, req.Home, r); err != nil {
		return Result{}, err
	}
	v, err := api.call(ctx, "/backup/pre_create", nil, b.bundle.Manifest, true)
	if err != nil {
		return ddMutationFailure(ctx, req.Home, r, "backup upload", err)
	}
	grant, err := ddObject(v["result"])
	if err != nil {
		return ddPending(r, "backup upload")
	}
	r.SN = ddText(grant["sn"])
	if r.SN == "" {
		return ddPending(r, "backup upload")
	}
	raw, err := json.Marshal(grant)
	if err != nil {
		return ddPending(r, "backup upload")
	}
	r.Backup.Grant, err = ddBackupProtect(raw, false)
	if err != nil {
		return ddPending(r, "backup upload")
	}
	r.Backup.Stage = "components"
	r.Submission = "components_pending"
	if err = ddSave(ctx, req.Home, r); err != nil {
		return ddPending(r, "backup upload")
	}
	return ddContinueBackup(ctx, api, req.Home, r, b)
}

func ddBackupPut(ctx context.Context, target string, f *os.File, size int64) error {
	u, err := url.Parse(target)
	if err != nil {
		return failure("publish.dd_backup", "invalid object grant")
	}
	if u.Scheme == "http" && u.Hostname() == "a19-ui-res.s3v2.nie.netease.com" && (u.Port() == "" || u.Port() == "80") && u.User == nil {
		u.Scheme = "https"
		u.Host = u.Hostname()
	}
	if err = ddObjectURL(u.String(), false); err != nil {
		return err
	}
	headers := http.Header{"Content-Type": {"application/x-zip-compressed"}, "X-Amz-Acl": {"public-read"}}
	for k, values := range u.Query() {
		if strings.EqualFold(k, "content-type") || strings.EqualFold(k, "x-amz-acl") {
			if len(values) != 1 || strings.ContainsAny(values[0], "\r\n") || values[0] == "" {
				return failure("publish.dd_backup", "ambiguous upload headers")
			}
			headers.Set(k, values[0])
		}
	}
	request, err := http.NewRequestWithContext(ctx, http.MethodPut, u.String(), f)
	if err != nil {
		return err
	}
	request.Header = headers
	request.ContentLength = size
	client := &http.Client{Timeout: 10 * time.Minute, CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }}
	response, err := client.Do(request)
	if err != nil {
		return failure("publish.dd_backup", "backup component PUT is uncertain")
	}
	defer response.Body.Close()
	if response.StatusCode != 200 && response.StatusCode != 201 {
		return failure("publish.dd_backup", "backup component PUT was not acknowledged")
	}
	return nil
}
func ddSubset(expected, actual any) bool {
	switch e := expected.(type) {
	case map[string]any:
		a, ok := actual.(map[string]any)
		if !ok {
			return false
		}
		for k, v := range e {
			if !ddSubset(v, a[k]) {
				return false
			}
		}
		return true
	case []any:
		a, ok := actual.([]any)
		if !ok || len(e) != len(a) {
			return false
		}
		used := make([]bool, len(a))
		for _, v := range e {
			found := false
			for i, w := range a {
				if !used[i] && ddSubset(v, w) {
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
		return ddEqual(expected, actual)
	}
}
func ddBackupMatches(ctx context.Context, api ddAPI, r ddReceipt, withName bool) (bool, error) {
	if r.SN == "" {
		return false, nil
	}
	current, err := ddDetail(ctx, api, "backup", r.SN)
	if err != nil || ddGame(current) != r.Game {
		return false, err
	}
	if name, ok := r.Expected["name"]; withName && ok && !ddEqual(name, current["name"]) {
		return false, nil
	}
	for _, g := range ddBackupGroups {
		field := "items"
		if g == "wtf" {
			field = "accounts"
		}
		if !ddSubset(ddSlice(ddMap(r.Expected[g])[field]), ddSlice(ddMap(current[g])[field])) {
			return false, nil
		}
	}
	rows, err := ddBackupList(ctx, api)
	if err != nil {
		return false, err
	}
	for _, v := range rows {
		if ddText(ddMap(v)["sn"]) == r.SN {
			return true, nil
		}
	}
	return false, nil
}
func ddVerifyBackup(ctx context.Context, api ddAPI, home string, r ddReceipt, action string) (Result, error) {
	if ddText(api.identity()["author_id"]) != r.Author {
		return Result{}, invalid("$.session", "author differs from original backup")
	}
	if r.Submission == "rejected" {
		return ddPending(r, action)
	}
	if r.Backup != nil && (r.Backup.Stage == "allocate" || r.Backup.Stage == "components" || r.Backup.Stage == "finish") {
		return ddPending(r, action)
	}
	if ok, _ := ddBackupMatches(ctx, api, r, true); !ok {
		return ddPending(r, action)
	}
	r.Verification = "verified"
	if err := ddSave(ctx, home, r); err != nil {
		return ddPending(r, action)
	}
	return r.result(action), nil
}
