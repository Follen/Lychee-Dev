package publish

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"io"
	"os"
	"path/filepath"

	"github.com/follenfang/lycheedev/internal/vault"
)

type modusOperation struct {
	Schema        string         `json:"schema"`
	ID            string         `json:"id"`
	Action        string         `json:"action"`
	Account       string         `json:"account"`
	Source        string         `json:"source"`
	Input         map[string]any `json:"input"`
	InputDigest   string         `json:"inputDigest"`
	Stage         string         `json:"stage"`
	Submission    string         `json:"submission"`
	RemoteID      string         `json:"remoteId,omitempty"`
	ArchiveSHA256 string         `json:"archiveSha256,omitempty"`
	ArchiveMD5    string         `json:"archiveMd5,omitempty"`
	ArchiveBytes  int64          `json:"archiveBytes,omitempty"`
	Wire          map[string]any `json:"wire,omitempty"`
	Verified      bool           `json:"verified"`
}

func modusOperationPath(home, id string) (string, error) {
	if !validRequest(id) {
		return "", invalid("$.request", "original request ID must contain 1-128 ASCII letters, digits, underscores or hyphens")
	}
	return filepath.Join(home, "publish", "modus", "operations", id+".json"), nil
}
func modusInputDigest(input map[string]any) string {
	raw, _ := json.Marshal(input)
	sum := sha256.Sum256(raw)
	return hex.EncodeToString(sum[:])
}
func readModusOperation(home, id string) (modusOperation, error) {
	var op modusOperation
	p, err := modusOperationPath(home, id)
	if err != nil {
		return op, err
	}
	raw, err := modusReadBounded(p, 4<<20)
	if err != nil {
		return op, err
	}
	d := json.NewDecoder(bytes.NewReader(raw))
	d.UseNumber()
	d.DisallowUnknownFields()
	if err = d.Decode(&op); err != nil {
		return op, failure("publish.modus_operation", "invalid saved ModUs operation")
	}
	if err = d.Decode(new(any)); err != io.EOF {
		return op, failure("publish.modus_operation", "saved operation contains trailing data")
	}
	if op.Schema != "lycheedev.publish.modus-operation.v1" || op.ID != id || op.InputDigest != modusInputDigest(op.Input) || (op.Source != "creator" && op.Source != "main") {
		return op, failure("publish.modus_operation", "saved ModUs operation identity is invalid")
	}
	return op, nil
}
func saveModusOperation(ctx context.Context, home string, op modusOperation) error {
	p, err := modusOperationPath(home, op.ID)
	if err != nil {
		return err
	}
	raw, err := json.Marshal(op)
	if err != nil {
		return err
	}
	return vault.ReplaceFile(ctx, p, raw)
}

func modusRejected(err error) bool {
	var e *Error
	if !errors.As(err, &e) {
		return false
	}
	if e.Code == "publish.modus_rejected" {
		return true
	}
	details, _ := e.Details.(map[string]any)
	return e.Code == "publish.http_status" && (details["httpStatus"] == 401 || details["httpStatus"] == 403)
}

func modusWriteFailure(ctx context.Context, home string, op *modusOperation, err error) error {
	if modusRejected(err) {
		op.Submission = "rejected"
		if saveErr := saveModusOperation(ctx, home, *op); saveErr != nil {
			return saveErr
		}
	}
	return err
}
func (op modusOperation) result(action string) Result {
	verification := "pending"
	nextCommand := "publish modus operation verify"
	if op.Submission == "rejected" {
		nextCommand = ""
	}
	if op.Verified {
		verification = "remote_identity_and_fields"
	}
	return Result{Platform: "modus", Action: action, Complete: op.Verified, OperationID: op.ID, Submission: op.Submission, Verification: verification, Data: map[string]any{"originalAction": op.Action, "stage": op.Stage, "remoteId": op.RemoteID, "archiveSha256": op.ArchiveSHA256, "replayAllowed": false, "nextCommand": nextCommand}}
}
func (c *modusClient) account(ctx context.Context) (string, error) {
	v, err := c.call(ctx, "GET", "system/user/getInfo", nil, nil)
	if err != nil {
		return "", err
	}
	obj, ok := v.(map[string]any)
	if !ok {
		return "", failure("publish.modus_account", "ModUs account response is invalid")
	}
	if user, ok := obj["user"].(map[string]any); ok {
		obj = user
	}
	id := fmtModusID(obj["userId"])
	if id == "" {
		id = fmtModusID(obj["id"])
	}
	if id == "" || id == "0" {
		return "", failure("publish.modus_account", "ModUs did not identify the current account")
	}
	return id, nil
}
func RunModus(ctx context.Context, action string, r Request) (Result, error) {
	result := Result{Platform: "modus", Action: action}
	spec, ok := ModusInput(action)
	if !ok {
		return result, invalid("$action", "unknown ModUs action")
	}
	input, err := spec.Decode(r.Input)
	if err != nil {
		return result, err
	}
	if err = ctx.Err(); err != nil {
		return result, err
	}
	if action == "config create" || action == "config edit" || action == "wa create" || action == "wa edit" {
		if err = validateModusMainPlatforms(map[string]any{"platform": input["platform"], "synchronizationType": input["synchronization_type"], "requiredTierId": input["required_tier_id"]}); err != nil {
			return result, err
		}
	}
	if _, backup := modusBackupInput(action); backup {
		if r.Plan || action == "backup scan" || action == "backup generate" || action == "backup status" {
			return runModusBackup(ctx, action, r, input, nil)
		}
		c, err := modusMainClient(ctx)
		if err != nil {
			return result, err
		}
		return runModusBackup(ctx, action, r, input, c)
	}
	if r.Plan {
		data := map[string]any{"plan": true, "effects": spec.Effects, "remoteChecked": false}
		if action == "project create" || action == "project edit" {
			wire, err := modusProjectWire(input, action == "project create")
			if err != nil {
				return result, err
			}
			data["fields"] = wire
		}
		if file := String(input, "file"); file != "" && action == "media upload" {
			a, err := inspectModusMedia(ctx, file)
			if err != nil {
				return result, err
			}
			a.file.Close()
			data["media"] = map[string]any{"sha256": a.sha256, "bytes": a.size}
		} else if file != "" {
			a, err := inspectArtifact(ctx, file)
			if err != nil {
				return result, err
			}
			defer a.file.Close()
			m, err := inspectModusZIP(ctx, a)
			if err != nil {
				return result, err
			}
			data["archive"] = map[string]any{"sha256": a.sha256, "bytes": a.size, "metadata": m}
		}
		result.Complete = true
		result.Data = modusSafe(data)
		return result, nil
	}
	if action == "operation status" {
		op, err := readModusOperation(r.Home, String(input, "request"))
		if err != nil {
			return result, err
		}
		return op.result(action), nil
	}
	if action == "options builds" {
		result.Complete = true
		result.Data = modusBuilds()
		return result, nil
	}
	// Public static catalogs need neither account source nor local credentials.
	if action == "options config-tags" || action == "options wa-tags" || action == "options wa-support-addons" {
		c := &modusClient{http: NewHTTP(), static: "https://cdn.modus.cool/modus/client_static_api/"}
		data, err := c.read(ctx, action, input)
		result.Data = modusSafe(data)
		result.Complete = err == nil
		return result, err
	}
	var c *modusClient
	mainSource := String(input, "session_source") == "main" || modusMainAction(action)
	if action == "operation verify" || action == "operation resume" {
		op, err := readModusOperation(r.Home, String(input, "request"))
		if err != nil {
			return result, err
		}
		mainSource = op.Source == "main"
	}
	if mainSource {
		c, err = modusMainClient(ctx)
	} else {
		c, err = newModusCreator()
	}
	if err != nil {
		return result, err
	}
	if action == "session doctor" {
		identity, err := c.account(ctx)
		result.Complete = err == nil
		result.Data = map[string]any{"source": map[bool]string{true: "main", false: "creator"}[c.main], "accountId": identity, "apiReady": err == nil}
		return result, err
	}
	return c.run(ctx, action, r, input)
}
func (c *modusClient) run(ctx context.Context, action string, r Request, input map[string]any) (Result, error) {
	result := Result{Platform: "modus", Action: action}
	if action == "operation resume" || action == "operation verify" {
		id := String(input, "request")
		lease, err := vault.TryAcquireLease(ctx, filepath.Join(r.Home, "publish", "modus", "locks"), id)
		if err != nil {
			return result, err
		}
		defer lease.Close()
		op, err := readModusOperation(r.Home, id)
		if err != nil {
			return result, err
		}
		account, err := c.account(ctx)
		if err != nil {
			return result, err
		}
		if account != op.Account || c.main != (op.Source == "main") {
			return op.result(action), failure("publish.modus_account", "current account differs from original operation")
		}
		if action == "operation verify" {
			if key := String(input, "remote_key"); key != "" {
				if op.RemoteID != "" && op.RemoteID != key {
					return op.result(action), invalid("$.remote_key", "differs from retained remote identity")
				}
				op.RemoteID = key
				if op.Action == "media upload" {
					op.Wire = map[string]any{"key": key, "url": "https://cdn.modus.cool/" + key}
				}
			}
			if id := fmtModusID(input["remote_id"]); id != "" {
				if op.RemoteID != "" && op.RemoteID != id {
					return op.result(action), invalid("$.remote_id", "differs from retained remote identity")
				}
				op.RemoteID = id
			}
			err = c.verifyOperation(ctx, &op)
			if saveErr := saveModusOperation(ctx, r.Home, op); saveErr != nil {
				return op.result(action), saveErr
			}
			return op.result(action), err
		}
		if op.Verified {
			return op.result(action), nil
		}
		if op.Submission == "uncertain" {
			return op.result(action), &Error{Code: "publish.modus_pending", Message: "original write requires readback; it will not be replayed", Exit: 6}
		}
		err = c.continueOperation(ctx, r.Home, &op)
		return op.result(action), err
	}
	if !ModusMutates(action) {
		v, err := c.read(ctx, action, input)
		result.Data = modusSafe(v)
		result.Complete = err == nil
		return result, err
	}
	if _, err := modusOperationPath(r.Home, r.RequestID); err != nil {
		return result, err
	}
	lease, err := vault.TryAcquireLease(ctx, filepath.Join(r.Home, "publish", "modus", "locks"), r.RequestID)
	if err != nil {
		return result, err
	}
	defer lease.Close()
	if old, err := readModusOperation(r.Home, r.RequestID); err == nil {
		if old.Action != action || old.InputDigest != modusInputDigest(input) {
			return result, invalid("$request", "request ID already belongs to different original input")
		}
		if old.Submission == "rejected" {
			return old.result(action), failure("publish.modus_rejected", "the original write was rejected and will not be replayed")
		}
		if old.Verified {
			if c.main != (old.Source == "main") {
				return old.result(action), failure("publish.modus_account", "current login source differs from original operation")
			}
			account, err := c.account(ctx)
			if err != nil {
				return result, err
			}
			if account != old.Account {
				return old.result(action), failure("publish.modus_account", "current account differs from original operation")
			}
			return old.result(action), nil
		}
		return old.result(action), &Error{Code: "publish.modus_pending", Message: "request already exists; use original operation status, verify or resume", Exit: 6}
	} else if !errors.Is(err, os.ErrNotExist) {
		return result, err
	}
	account, err := c.account(ctx)
	if err != nil {
		return result, err
	}
	op := modusOperation{Schema: "lycheedev.publish.modus-operation.v1", ID: r.RequestID, Action: action, Account: account, Source: "creator", Input: input, InputDigest: modusInputDigest(input), Stage: "prepare", Submission: "not_submitted"}
	if c.main {
		op.Source = "main"
	}
	if err = saveModusOperation(ctx, r.Home, op); err != nil {
		return result, err
	}
	err = c.continueOperation(ctx, r.Home, &op)
	return op.result(action), err
}
