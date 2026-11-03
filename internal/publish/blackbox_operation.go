package publish

import (
	"archive/zip"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"io"
	"os"
	"path/filepath"
	"strings"

	"github.com/follenfang/lycheedev/internal/vault"
)

type bbArchive struct {
	Path   string       `json:"path"`
	Name   string       `json:"name"`
	SHA256 string       `json:"sha256"`
	Bytes  int64        `json:"bytes"`
	URL    string       `json:"url,omitempty"`
	COS    *bbCOSObject `json:"cos,omitempty"`
}
type bbStep struct {
	Route  string         `json:"route"`
	Query  map[string]any `json:"query"`
	Body   map[string]any `json:"body"`
	State  string         `json:"state"`
	Target string         `json:"target,omitempty"`
}
type bbOperation struct {
	Schema          string             `json:"schema"`
	ID              string             `json:"id"`
	Action          string             `json:"action"`
	Account         string             `json:"account"`
	Method          string             `json:"method"`
	Digest          string             `json:"digest"`
	Input           map[string]any     `json:"input"`
	Steps           []bbStep           `json:"steps"`
	Expected        map[string]any     `json:"expected"`
	Previous        []string           `json:"previous"`
	Branch          string             `json:"branch"`
	Target          string             `json:"target"`
	Archive         *bbArchive         `json:"archive,omitempty"`
	Backup          *bbBackupOperation `json:"backup,omitempty"`
	OriginalArchive string             `json:"originalArchive,omitempty"`
	OriginalBranch  string             `json:"originalBranch,omitempty"`
	Stage           string             `json:"stage"`
	Verified        bool               `json:"verified"`
	Evidence        map[string]any     `json:"evidence,omitempty"`
}

func bbDigest(in map[string]any) string {
	raw, _ := json.Marshal(in)
	hash := sha256.Sum256(raw)
	return hex.EncodeToString(hash[:])
}
func bbOperationName(id string) (string, error) {
	if !validRequest(id) {
		return "", invalid("$.request", "request requires 1-128 ASCII letters, digits, underscores or hyphens")
	}
	return "operations/" + id, nil
}
func (c *bbClient) saveOperation(ctx context.Context, op *bbOperation) error {
	name, e := bbOperationName(op.ID)
	if e != nil {
		return e
	}
	return bbSave(ctx, c.home, name, op)
}

func (c *bbClient) writeFailure(ctx context.Context, op *bbOperation, err error) error {
	var e *Error
	if !errors.As(err, &e) {
		return err
	}
	details, _ := e.Details.(map[string]any)
	if e.Code == "publish.blackbox_rejected" || e.Code == "publish.blackbox_http" && (details["httpStatus"] == 401 || details["httpStatus"] == 403) {
		op.Stage = "rejected"
		if saveErr := c.saveOperation(ctx, op); saveErr != nil {
			return saveErr
		}
	}
	return err
}
func (c *bbClient) loadOperation(id string) (*bbOperation, error) {
	name, e := bbOperationName(id)
	if e != nil {
		return nil, e
	}
	var op bbOperation
	if e = bbLoad(c.home, name, &op); e != nil {
		return nil, e
	}
	if op.Schema != "lycheedev.blackbox.operation.v1" || op.ID != id || op.Digest != bbDigest(op.Input) || op.Account == "" || (op.Method != "sms" && op.Method != "wechat") {
		return nil, failure("publish.blackbox_operation", "saved operation identity is invalid")
	}
	return &op, nil
}
func (op *bbOperation) result(action string) Result {
	state := "not_submitted"
	steps := make([]any, 0, len(op.Steps))
	for _, s := range op.Steps {
		steps = append(steps, map[string]any{"state": s.State, "target": s.Target})
	}
	for _, s := range op.Steps {
		if s.State == "uncertain" {
			state = "uncertain"
			break
		}
		if s.State == "submitted" || s.State == "verified" {
			state = "submitted"
		}
	}
	absorb := func(s string) {
		if s == "uncertain" {
			state = "uncertain"
		} else if s != "" && s != "prepared" && state != "uncertain" {
			state = "submitted"
		}
	}
	if op.Archive != nil && op.Archive.COS != nil {
		absorb(op.Archive.COS.State)
	}
	if op.Backup != nil {
		absorb(op.Backup.Allocation)
		absorb(op.Backup.Save)
		if op.Backup.Package != nil {
			for _, a := range op.Backup.Package.Archives {
				absorb(a.State)
				steps = append(steps, map[string]any{"field": a.Field, "state": a.State, "sha256": a.SHA256, "bytes": a.Bytes})
			}
		}
	}
	if op.Stage == "rejected" {
		state = "rejected"
	}
	verification := "pending"
	nextCommand := "publish blackbox operation verify"
	if op.Stage == "rejected" {
		nextCommand = ""
	}
	if op.Verified {
		verification = "remote_metadata"
	}
	return Result{Platform: "blackbox", Action: action, OperationID: op.ID, Complete: op.Verified, Submission: state, Verification: verification, Data: map[string]any{"originalAction": op.Action, "stage": op.Stage, "target": op.Target, "steps": steps, "evidence": bbPublic(op.Evidence), "replayAllowed": false, "nextCommand": nextCommand}}
}
func bbOpenArchive(path string) (*os.File, error) {
	f, e := openArtifact(path)
	if e != nil {
		return nil, invalid("$.file", "archive cannot be opened")
	}
	st, e := f.Stat()
	if e != nil || !st.Mode().IsRegular() || st.Size() == 0 {
		f.Close()
		return nil, invalid("$.file", "archive must be a nonempty regular file")
	}
	z, e := zip.NewReader(f, st.Size())
	if e != nil || len(z.File) == 0 {
		f.Close()
		return nil, invalid("$.file", "archive must be a nonempty ZIP")
	}
	return f, nil
}
func bbSnapshot(ctx context.Context, home, id, path string) (*bbArchive, error) {
	f, e := bbOpenArchive(path)
	if e != nil {
		return nil, e
	}
	defer f.Close()
	dir := filepath.Join(home, "publish", "blackbox", "artifacts", id)
	if e = os.MkdirAll(dir, 0700); e != nil {
		return nil, e
	}
	dst := filepath.Join(dir, "module.zip")
	out, e := os.OpenFile(dst, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0600)
	if e != nil {
		return nil, e
	}
	ok := false
	defer func() {
		out.Close()
		if !ok {
			os.Remove(dst)
		}
	}()
	h := sha256.New()
	n, e := io.Copy(io.MultiWriter(out, h), f)
	if e != nil {
		return nil, e
	}
	if e = out.Sync(); e != nil {
		return nil, e
	}
	if e = out.Close(); e != nil {
		return nil, e
	}
	if ctx.Err() != nil {
		return nil, ctx.Err()
	}
	ok = true
	return &bbArchive{Path: dst, Name: filepath.Base(path), SHA256: hex.EncodeToString(h.Sum(nil)), Bytes: n}, nil
}
func bbCheckArchive(a *bbArchive) error {
	f, e := openArtifact(a.Path)
	if e != nil {
		return failure("publish.blackbox_artifact", "original archive is missing")
	}
	defer f.Close()
	h := sha256.New()
	n, e := io.Copy(h, f)
	if e != nil || n != a.Bytes || hex.EncodeToString(h.Sum(nil)) != a.SHA256 {
		return failure("publish.blackbox_artifact", "original archive bytes changed")
	}
	return nil
}
func RunBlackbox(ctx context.Context, action string, r Request) (Result, error) {
	return newBB(r.Home).run(ctx, action, r)
}
func (c *bbClient) run(ctx context.Context, action string, r Request) (Result, error) {
	result := Result{Platform: "blackbox", Action: action}
	spec, ok := BlackboxInput(action)
	if !ok {
		return result, invalid("$action", "unsupported Blackbox action")
	}
	in, e := spec.Decode(r.Input)
	if e != nil {
		return result, e
	}
	if r.Plan {
		if e = bbValidateAuthInput(action, in); e != nil {
			return result, e
		}
		switch action {
		case "backup scan", "backup generate":
			if _, e = bbLocalBackupMode(ctx, action, in, true); e != nil {
				return result, e
			}
		case "backup create", "backup upload":
			if file := String(in, "file"); file != "" {
				if _, e = bbLoadBackupPackage(ctx, file); e != nil {
					return result, e
				}
			} else if _, e = bbLocalBackupMode(ctx, "backup generate", in, true); e != nil {
				return result, e
			}
		case "plugin update", "version edit":
			if file := String(in, "file"); file != "" {
				f, err := bbOpenArchive(file)
				if err != nil {
					return result, err
				}
				f.Close()
			}
		case "config edit":
			if e = bbValidateConfigPatch(in["configuration"].(map[string]any)); e != nil {
				return result, e
			}
		case "wa edit":
			if e = bbValidateWAPatch(in["configuration"].(map[string]any)); e != nil {
				return result, e
			}
		case "backup delete-preview":
			if _, e = bbBackupDeleteSelection(in); e != nil {
				return result, e
			}
		case "config create":
			if e = bbValidateConfigInput(in["configuration"].(map[string]any)); e != nil {
				return result, e
			}
		case "plugin edit":
			if value, supplied := in["type"]; supplied {
				if e = bbValidatePluginType(value); e != nil {
					return result, e
				}
			}
		case "backup delete":
			if _, e = bbBackupDeleteSelection(in); e != nil {
				return result, e
			}
			if !Bool(in, "confirm_delete") {
				return result, invalid("$.confirm_delete", "must be true")
			}
		case "config delete", "wa delete":
			if !Bool(in, "confirm_delete") {
				return result, invalid("$.confirm_delete", "must be true")
			}
		case "wa create":
			if e = bbValidateWA(in["configuration"].(map[string]any)); e != nil {
				return result, e
			}
		}
		result.Complete = true
		result.Data = map[string]any{"plan": true, "effects": spec.Effects, "input": bbPublic(in), "remoteValidation": false}
		return result, nil
	}
	if strings.HasPrefix(action, "session ") {
		return c.auth(ctx, action, in)
	}
	if action == "backup scan" || action == "backup generate" {
		data, e := bbLocalBackup(ctx, action, in)
		result.Data = data
		result.Complete = e == nil
		return result, e
	}
	if action == "operation status" {
		op, e := c.loadOperation(String(in, "request"))
		if e != nil {
			return result, e
		}
		return op.result(action), nil
	}
	if !BlackboxMutates(action) && action != "operation verify" {
		var data any
		if action == "backup delete-preview" {
			data, e = c.backupDeletePlan(ctx, in)
		} else {
			data, e = c.read(ctx, action, in)
		}
		result.Data = bbPublic(data)
		result.Complete = e == nil
		return result, e
	}
	id := r.RequestID
	if strings.HasPrefix(action, "operation ") {
		id = String(in, "request")
	}
	if _, e = bbOperationName(id); e != nil {
		return result, e
	}
	lease, e := vault.TryAcquireLease(ctx, filepath.Join(c.home, "publish", "blackbox", "locks"), id)
	if e != nil {
		return result, e
	}
	defer lease.Close()
	op, e := c.loadOperation(id)
	if e != nil && !errors.Is(e, os.ErrNotExist) {
		return result, e
	}
	if op == nil && strings.HasPrefix(action, "operation ") {
		return result, failure("publish.blackbox_operation", "original operation does not exist")
	}
	if op != nil && !strings.HasPrefix(action, "operation ") && (op.Action != action || op.Digest != bbDigest(in)) {
		return result, invalid("$.request", "request already identifies different input")
	}
	if op != nil && op.Verified {
		return op.result(action), nil
	}
	if e = c.renew(ctx); e != nil {
		return result, e
	}
	if op == nil {
		op = &bbOperation{Schema: "lycheedev.blackbox.operation.v1", ID: id, Action: action, Account: c.session.Identity, Method: c.session.Method, Digest: bbDigest(in), Input: bbClone(in), Stage: "preparing", Expected: map[string]any{}, Previous: []string{}}
		if e = c.prepare(ctx, op); e != nil {
			return result, e
		}
		op.Stage = "prepared"
		if e = c.saveOperation(ctx, op); e != nil {
			return result, e
		}
	}
	if op.Account != c.session.Identity || op.Method != c.session.Method {
		return op.result(action), failure("publish.blackbox_identity", "operation belongs to a different account or login source")
	}
	if action == "operation verify" {
		e = c.verify(ctx, op)
		if e == nil {
			e = c.saveOperation(ctx, op)
		}
		return op.result(action), e
	}
	e = c.continueOperation(ctx, op)
	return op.result(action), e
}
func (c *bbClient) continueOperation(ctx context.Context, op *bbOperation) error {
	if op.Stage == "rejected" {
		return failure("publish.blackbox_rejected", "the original write was rejected and will not be replayed")
	}
	if strings.HasPrefix(op.Action, "backup ") {
		return c.continueBackup(ctx, op)
	}
	if op.Archive != nil && op.Archive.URL == "" {
		if e := bbCheckArchive(op.Archive); e != nil {
			return e
		}
		url, e := c.uploadModule(ctx, op)
		if e != nil {
			return e
		}
		op.Archive.URL = url
		op.Steps[0].Body["fileUrl"] = url
		op.Stage = "uploaded"
		if e = c.saveOperation(ctx, op); e != nil {
			return e
		}
	}
	for i := range op.Steps {
		step := &op.Steps[i]
		if step.State == "verified" {
			continue
		}
		if step.State == "uncertain" || step.State == "submitted" {
			if e := c.verify(ctx, op); e != nil {
				return e
			}
			return c.saveOperation(ctx, op)
		}
		if op.Action == "backup delete" {
			plan, e := c.backupDeletePlan(ctx, map[string]any{"backup_ids": []any{step.Target}})
			if e != nil {
				return e
			}
			if plan["allowed"] != true {
				return failure("publish.blackbox_backup_in_use", "backup is referenced by a configuration share")
			}
		}
		step.State = "uncertain"
		op.Stage = "submitting"
		if e := c.saveOperation(ctx, op); e != nil {
			return e
		}
		reply, e := c.call(ctx, "POST", step.Route, step.Query, step.Body)
		if e != nil {
			return c.writeFailure(ctx, op, e)
		}
		step.State = "submitted"
		op.Stage = "submitted"
		if op.Action == "plugin update" && op.Target == "" {
			op.Target = bbID(reply["versionId"])
			if op.Target == "" {
				op.Target = bbID(reply["id"])
			}
		}
		if e = c.saveOperation(ctx, op); e != nil {
			return e
		}
		if op.Action == "backup delete" {
			rows, e := c.backups(ctx)
			if e != nil {
				return e
			}
			for _, r := range rows {
				if bbID(r["ID"]) == step.Target {
					return bbVerification()
				}
			}
			step.State = "verified"
			if e = c.saveOperation(ctx, op); e != nil {
				return e
			}
		}
	}
	if e := c.verify(ctx, op); e != nil {
		return e
	}
	return c.saveOperation(ctx, op)
}
func bbVerification() error {
	return &Error{Code: "publish.blackbox_verification", Message: "original write awaits matching readback; no mutation was repeated", Exit: 6}
}
