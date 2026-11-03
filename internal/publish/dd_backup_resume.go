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
	"time"

	"github.com/follenfang/lycheedev/internal/vault"
)

type ddBackupProgress struct {
	File       string            `json:"file"`
	Grant      []byte            `json:"grant,omitempty"`
	Components map[string]string `json:"components"`
	Stage      string            `json:"stage"`
}

func ddResumeBackup(ctx context.Context, api ddAPI, home string, r ddReceipt) (Result, error) {
	if ddText(api.identity()["author_id"]) != r.Author {
		return Result{}, invalid("$.session", "author differs from original backup")
	}
	lease, err := vault.AcquireLease(ctx, filepath.Join(home, "publish", "locks"), "dd/"+r.Author)
	if err != nil {
		return Result{}, err
	}
	defer lease.Close()
	r, err = ddLoad(home, r.Request)
	if err != nil {
		return Result{}, err
	}
	if r.Submission == "rejected" {
		return ddPending(r, "operation resume")
	}
	if r.Verification == "verified" {
		return r.result("operation resume"), nil
	}
	if r.Backup == nil || r.Backup.Stage == "allocate" {
		return ddPending(r, "operation resume")
	}
	b, err := ddLoadBundle(r.Backup.File)
	if err != nil {
		return Result{}, err
	}
	defer b.close()
	if ddDigest(map[string]any{"bundle": b.bundle, "name": r.Expected["name"]}) != r.Digest {
		return Result{}, invalid("$.file", "original backup bundle changed")
	}
	result, err := ddContinueBackup(ctx, api, home, r, b)
	result.Action = "operation resume"
	return result, err
}

func ddContinueBackup(ctx context.Context, api ddAPI, home string, r ddReceipt, b *ddBackupOpen) (Result, error) {
	if r.Backup.Stage == "components" {
		raw, err := ddBackupProtect(r.Backup.Grant, true)
		if err != nil {
			return Result{}, err
		}
		var grant map[string]any
		if json.Unmarshal(raw, &grant) != nil {
			return Result{}, failure("publish.dd_backup", "invalid retained backup grant")
		}
		for _, a := range b.bundle.Artifacts {
			state := r.Backup.Components[a.Group]
			if state == "acknowledged" {
				continue
			}
			component, ok := grant[a.Group].(map[string]any)
			if !ok {
				return ddPending(r, "backup upload")
			}
			target, ok := component["u_url"].(string)
			if component["u_url"] != nil && !ok {
				return ddPending(r, "backup upload")
			}
			if target != "" {
				if state == "uncertain" {
					if !ddBackupObjectMatches(ctx, target, a) {
						return ddPending(r, "backup upload")
					}
				} else {
					r.Backup.Components[a.Group] = "uncertain"
					r.Submission = "components_pending"
					if err = ddSave(ctx, home, r); err != nil {
						return Result{}, err
					}
					if _, err = b.files[a.Group].Seek(0, io.SeekStart); err != nil {
						return Result{}, err
					}
					if err = ddBackupPut(ctx, target, b.files[a.Group], a.Size); err != nil {
						return ddPending(r, "backup upload")
					}
				}
			}
			r.Backup.Components[a.Group] = "acknowledged"
			if err = ddSave(ctx, home, r); err != nil {
				return Result{}, err
			}
		}
		r.Backup.Stage = "finish"
		r.Submission = "prepared"
		if err := ddSave(ctx, home, r); err != nil {
			return Result{}, err
		}
	}
	if r.Backup.Stage == "finish" {
		r.Backup.Stage = "finish_uncertain"
		r.Submission = "finish_uncertain"
		if err := ddSave(ctx, home, r); err != nil {
			return Result{}, err
		}
		if _, err := api.call(ctx, "/backup/finish_create", nil, map[string]any{"sn": r.SN}, true); err != nil {
			return ddMutationFailure(ctx, home, r, "backup upload", err)
		}
		r.Backup.Stage = "rename"
		r.Submission = "acknowledged"
		if err := ddSave(ctx, home, r); err != nil {
			return Result{}, err
		}
	}
	if r.Backup.Stage == "finish_uncertain" {
		if ok, _ := ddBackupMatches(ctx, api, r, false); !ok {
			return ddPending(r, "backup upload")
		}
		r.Backup.Stage = "rename"
		r.Submission = "acknowledged"
		if err := ddSave(ctx, home, r); err != nil {
			return Result{}, err
		}
	}
	if r.Backup.Stage == "rename" {
		if name := ddText(r.Expected["name"]); name != "" {
			r.Backup.Stage = "rename_uncertain"
			r.Submission = "rename_uncertain"
			if err := ddSave(ctx, home, r); err != nil {
				return Result{}, err
			}
			if _, err := api.call(ctx, "/backup/modify_name", nil, map[string]any{"sn": r.SN, "name": name}, true); err != nil {
				return ddMutationFailure(ctx, home, r, "backup upload", err)
			}
		}
		r.Backup.Stage = "readback"
		r.Submission = "acknowledged"
		if err := ddSave(ctx, home, r); err != nil {
			return Result{}, err
		}
	}
	return ddVerifyBackup(ctx, api, home, r, "backup upload")
}

func ddBackupObjectMatches(ctx context.Context, target string, a ddBackupArtifact) bool {
	u, err := url.Parse(target)
	if err != nil {
		return false
	}
	if u.Scheme == "http" && u.Hostname() == "a19-ui-res.s3v2.nie.netease.com" {
		u.Scheme = "https"
		u.Host = u.Hostname()
	}
	u.RawQuery = ""
	if ddObjectURL(u.String(), true) != nil {
		return false
	}
	req, err := http.NewRequestWithContext(ctx, "GET", u.String(), nil)
	if err != nil {
		return false
	}
	client := http.Client{Timeout: 10 * time.Minute, CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }}
	response, err := client.Do(req)
	if err != nil {
		return false
	}
	defer response.Body.Close()
	if response.StatusCode != 200 {
		return false
	}
	hash := sha256.New()
	n, err := io.Copy(hash, io.LimitReader(response.Body, a.Size+1))
	return err == nil && n == a.Size && hex.EncodeToString(hash.Sum(nil)) == a.SHA256
}
