package publish

import (
	"context"
	"encoding/base64"
	"io"
	"os"
	"path/filepath"
)

type bbBackupOperation struct {
	Package    *bbBackupPackage          `json:"package,omitempty"`
	Allocation string                    `json:"allocation,omitempty"`
	Maximum    int64                     `json:"maximum"`
	LimitValid bool                      `json:"limit_valid"`
	Save       string                    `json:"save,omitempty"`
	COS        map[string]map[string]any `json:"cos,omitempty"`
}

func (c *bbClient) prepareBackup(ctx context.Context, op *bbOperation) error {
	in := op.Input
	op.Backup = &bbBackupOperation{COS: map[string]map[string]any{}}
	switch op.Action {
	case "backup upload", "backup create":
		file := String(in, "file")
		if file == "" && op.Action == "backup create" {
			generated, e := bbLocalBackup(ctx, "backup generate", in)
			if e != nil {
				return e
			}
			file = String(generated.(map[string]any), "file")
		}
		p, e := bbLoadBackupPackage(ctx, file)
		if e != nil {
			return e
		}
		dir := filepath.Join(c.home, "publish", "blackbox", "artifacts", op.ID)
		if e = os.MkdirAll(dir, 0700); e != nil {
			return e
		}
		for i := range p.Archives {
			a := &p.Archives[i]
			dst := filepath.Join(dir, a.File)
			if e = bbBackupCopy(ctx, a.Local, dst); e != nil {
				return e
			}
			sha, md5, size, e := modusBackupHash(ctx, dst)
			if e != nil {
				return e
			}
			if sha != a.SHA256 || md5 != a.MD5 || size != a.Bytes {
				return failure("publish.blackbox_artifact", "backup archive changed while snapshotting")
			}
			a.Local = dst
			a.State = "prepared"
		}
		op.Backup.Package = p
		op.Backup.Allocation = "prepared"
		op.Backup.Save = "prepared"
		op.Branch = String(p.Configuration, "Strappid")
		return nil
	case "backup rename":
		op.Target = String(in, "backup_id")
		if _, e := c.backup(ctx, op.Target); e != nil {
			return e
		}
		op.Expected = map[string]any{"Name": in["name"]}
		op.Steps = []bbStep{{Route: "/wow/rename_user_configuration_backup/", Query: map[string]any{"backup_id": op.Target, "new_name": in["name"]}, Body: map[string]any{}, State: "prepared", Target: op.Target}}
		return nil
	case "backup delete":
		if in["confirm_delete"] != true {
			return invalid("$.confirm_delete", "must be true")
		}
		plan, e := c.backupDeletePlan(ctx, in)
		if e != nil {
			return e
		}
		if plan["allowed"] != true {
			return failure("publish.blackbox_backup_in_use", "backup is referenced by existing shares")
		}
		for _, r := range plan["items"].([]map[string]any) {
			id := String(r, "backup_id")
			op.Steps = append(op.Steps, bbStep{Route: "/wow/remove_user_configuration_backup/", Query: map[string]any{"backup_id": id}, Body: map[string]any{}, State: "prepared", Target: id})
		}
		return nil
	}
	return invalid("$action", "unsupported backup write")
}
func bbBackupCopy(ctx context.Context, src, dst string) error {
	f, e := openArtifact(src)
	if e != nil {
		return e
	}
	defer f.Close()
	out, e := os.OpenFile(dst, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0600)
	if e != nil {
		return e
	}
	defer out.Close()
	if _, e = io.Copy(out, f); e != nil {
		return e
	}
	if e = ctx.Err(); e != nil {
		return e
	}
	return out.Sync()
}
func (c *bbClient) continueBackup(ctx context.Context, op *bbOperation) error {
	if op.Stage == "rejected" {
		return failure("publish.blackbox_rejected", "the original write was rejected and will not be replayed")
	}
	if op.Backup == nil {
		return failure("publish.blackbox_operation", "original backup operation missing")
	}
	if op.Action != "backup upload" && op.Action != "backup create" {
		for i := range op.Steps {
			s := &op.Steps[i]
			if s.State == "verified" {
				continue
			}
			if s.State == "uncertain" || s.State == "submitted" {
				if e := c.verifyBackupStep(ctx, op, i); e != nil {
					return e
				}
				if e := c.saveOperation(ctx, op); e != nil {
					return e
				}
				continue
			}
			if op.Action == "backup delete" {
				plan, e := c.backupDeletePlan(ctx, map[string]any{"backup_ids": []any{s.Target}})
				if e != nil {
					return e
				}
				if plan["allowed"] != true {
					return failure("publish.blackbox_backup_in_use", "backup now references an existing share")
				}
			}
			s.State = "uncertain"
			op.Stage = "submitting"
			if e := c.saveOperation(ctx, op); e != nil {
				return e
			}
			if _, e := c.call(ctx, "POST", s.Route, s.Query, s.Body); e != nil {
				return c.writeFailure(ctx, op, e)
			}
			s.State = "submitted"
			if e := c.saveOperation(ctx, op); e != nil {
				return e
			}
			if e := c.verifyBackupStep(ctx, op, i); e != nil {
				return e
			}
			if e := c.saveOperation(ctx, op); e != nil {
				return e
			}
		}
		return c.verifyBackup(ctx, op)
	}
	b := op.Backup
	if b.Package == nil {
		return failure("publish.blackbox_operation", "original package missing")
	}
	p := b.Package
	if b.Allocation == "uncertain" {
		return bbVerification()
	}
	if b.Allocation == "prepared" {
		b.Allocation = "uncertain"
		op.Stage = "allocating"
		if e := c.saveOperation(ctx, op); e != nil {
			return e
		}
		r, e := c.call(ctx, "POST", "/wow/create_backup/", nil, nil)
		if e != nil {
			return c.writeFailure(ctx, op, e)
		}
		op.Target = bbID(r["ID"])
		if op.Target == "" {
			return bbVerification()
		}
		b.Allocation = "allocated"
		b.Maximum, b.LimitValid = bbBackupInteger(r["MaxSize"])
		op.Stage = "allocated"
		p.Configuration["ID"] = r["ID"]
		p.Configuration["SaveTime"] = c.now().Unix()
		if e = c.saveOperation(ctx, op); e != nil {
			return e
		}
	}
	if b.Allocation != "allocated" || op.Target == "" {
		return failure("publish.blackbox_operation", "invalid allocation state")
	}
	if !b.LimitValid || b.Maximum < 0 || p.SourceBytes > b.Maximum {
		return failure("publish.blackbox_backup_limit", "allocated backup limit is missing or below selected source bytes")
	}
	for i := range p.Archives {
		a := &p.Archives[i]
		if a.State == "uploaded" {
			continue
		}
		key := a.Field + "/" + a.Account
		if a.State == "uncertain" {
			token := b.COS[key]
			if token == nil {
				return bbVerification()
			}
			if e := c.verifyBackupObject(ctx, a, token); e != nil {
				return e
			}
			a.Host = String(token, "host")
			a.Path = String(token, "path")
			a.State = "uploaded"
		} else {
			sha, md5, n, e := modusBackupHash(ctx, a.Local)
			if e != nil {
				return e
			}
			if sha != a.SHA256 || md5 != a.MD5 || n != a.Bytes {
				return failure("publish.blackbox_artifact", "original backup bytes changed")
			}
			token, e := c.call(ctx, "POST", "/wow/get_user_backup_credential/", map[string]any{"backup_id": op.Target, "file_type": a.Type}, nil)
			if e != nil {
				return e
			}
			if _, e = bbCOSOrigin(String(token, "cos")); e != nil {
				return e
			}
			if _, e = bbCOSCredentials(token["Credentials"]); e != nil {
				return e
			}
			if !bbCOSKey(String(token, "path")) || String(token, "host") == "" {
				return failure("publish.blackbox_cos", "invalid backup upload target")
			}
			b.COS[key] = token
			a.State = "uncertain"
			op.Stage = "uploading"
			if e = c.saveOperation(ctx, op); e != nil {
				return e
			}
			host, path, e := c.uploadBackupArchive(ctx, a, token)
			if e != nil {
				return e
			}
			a.Host, a.Path, a.State = host, path, "uploaded"
		}
		desc, e := bbBackupDescriptor(p, *a)
		if e != nil {
			return e
		}
		desc["Host"], desc["Path"] = a.Host, a.Path
		if e = c.saveOperation(ctx, op); e != nil {
			return e
		}
	}
	if b.Save == "uncertain" || b.Save == "submitted" {
		return c.verifyBackup(ctx, op)
	}
	if b.Save != "prepared" {
		return failure("publish.blackbox_operation", "invalid backup save state")
	}
	op.Expected = bbClone(p.Configuration)
	for _, k := range []string{"HeyboxID", "Name", "SaveTime"} {
		delete(op.Expected, k)
	}
	b.Save = "uncertain"
	op.Stage = "saving"
	if e := c.saveOperation(ctx, op); e != nil {
		return e
	}
	encoded := base64.StdEncoding.EncodeToString([]byte(bbJSON(p.Configuration)))
	if _, e := c.call(ctx, "POST", "/wow/save_user_configuration/", map[string]any{"backup_id": op.Target}, map[string]any{"configuration": encoded}); e != nil {
		return c.writeFailure(ctx, op, e)
	}
	b.Save = "submitted"
	if e := c.saveOperation(ctx, op); e != nil {
		return e
	}
	return c.verifyBackup(ctx, op)
}
func (c *bbClient) verifyBackupStep(ctx context.Context, op *bbOperation, i int) error {
	s := &op.Steps[i]
	rows, e := c.backups(ctx)
	if e != nil {
		return e
	}
	if op.Action == "backup delete" {
		for _, r := range rows {
			if bbID(r["ID"]) == s.Target {
				return bbVerification()
			}
		}
	} else {
		r, e := bbFind(rows, "ID", s.Target)
		if e != nil {
			return e
		}
		if !bbMatches(r, op.Expected) {
			return bbVerification()
		}
	}
	s.State = "verified"
	return nil
}
func (c *bbClient) verifyBackup(ctx context.Context, op *bbOperation) error {
	if op.Stage == "rejected" {
		return failure("publish.blackbox_rejected", "the original write was rejected and will not be replayed")
	}
	if op.Action == "backup upload" || op.Action == "backup create" {
		if op.Backup == nil || op.Target == "" || (op.Backup.Save != "uncertain" && op.Backup.Save != "submitted" && op.Backup.Save != "verified") || len(op.Expected) == 0 {
			return bbVerification()
		}
		r, e := c.backup(ctx, op.Target)
		if e != nil {
			return e
		}
		if !bbMatches(r, op.Expected) {
			return bbVerification()
		}
		op.Backup.Save = "verified"
		op.Evidence = map[string]any{"backup_id": op.Target, "archive_count": len(op.Backup.Package.Archives), "verification_scope": "remote_metadata", "archive_bytes_verified": false}
	} else {
		for i, s := range op.Steps {
			if s.State == "prepared" {
				return bbVerification()
			}
			if e := c.verifyBackupStep(ctx, op, i); e != nil {
				return e
			}
		}
		op.Evidence = map[string]any{"backup_id": op.Target, "verified_items": len(op.Steps)}
	}
	op.Verified = true
	op.Stage = "verified"
	return c.saveOperation(ctx, op)
}
