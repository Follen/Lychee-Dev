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
	"regexp"
	"strings"
	"time"

	"github.com/follenfang/lycheedev/internal/vault"
)

type ddReceipt struct {
	Schema       string            `json:"schema"`
	Request      string            `json:"request"`
	Session      string            `json:"session,omitempty"`
	Author       string            `json:"author"`
	Action       string            `json:"action"`
	Digest       string            `json:"digest"`
	SN           string            `json:"sn,omitempty"`
	Game         int64             `json:"game_type,omitempty"`
	Submission   string            `json:"submission"`
	Verification string            `json:"verification"`
	Expected     map[string]any    `json:"expected,omitempty"`
	Created      time.Time         `json:"created"`
	Batch        *ddDeleteBatch    `json:"batch,omitempty"`
	Backup       *ddBackupProgress `json:"backup,omitempty"`
}

var ddRequestPattern = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$`)

func ddReceiptPath(home, request string) (string, error) {
	if !ddRequestPattern.MatchString(request) {
		return "", invalid("$request", "request must be 1..128 letters, digits, dot, underscore or hyphen")
	}
	return filepath.Join(home, "publish", "dd", "operations", request+".json"), nil
}
func ddSave(ctx context.Context, home string, r ddReceipt) error {
	p, err := ddReceiptPath(home, r.Request)
	if err != nil {
		return err
	}
	if err = os.MkdirAll(filepath.Dir(p), 0700); err != nil {
		return err
	}
	b, err := json.Marshal(r)
	if err != nil {
		return err
	}
	return vault.ReplaceFile(ctx, p, b)
}
func ddLoad(home, request string) (ddReceipt, error) {
	var r ddReceipt
	p, err := ddReceiptPath(home, request)
	if err != nil {
		return r, err
	}
	b, err := os.ReadFile(p)
	if err != nil {
		return r, err
	}
	d := json.NewDecoder(bytes.NewReader(b))
	d.UseNumber()
	if len(b) > 4<<20 || d.Decode(&r) != nil || d.Decode(new(any)) != io.EOF || r.Schema != "lycheedev.publish.dd-operation.v1" || r.Request != request {
		return r, failure("publish.dd_receipt", "invalid original DD operation receipt")
	}
	return r, nil
}
func (r ddReceipt) result(action string) Result {
	data := map[string]any{"request": r.Request, "original_action": r.Action, "sn": r.SN, "author_id": r.Author, "batch": r.Batch, "replayAllowed": false}
	if r.Session != "" {
		data["recoveryInput"] = map[string]any{"request": r.Request, "session": r.Session}
		data["sessionGuidance"] = "Use recoveryInput with operation verify or operation resume in the same --home; inspect the retained task with session status. The session ID does not prove that the task is still available."
	} else {
		data["sessionGuidance"] = "This older receipt has no session ID. Recover the retained task ID from the original session start output and inspect it with session status in the same --home; do not start a new session or resubmit the original operation."
	}
	return Result{Platform: "dd", Action: action, Complete: r.Verification == "verified", OperationID: r.Request, Submission: r.Submission, Verification: r.Verification, Data: data}
}

func ddDigest(v any) string {
	b, _ := json.Marshal(v)
	s := sha256.Sum256(b)
	return hex.EncodeToString(s[:])
}
func ddPending(r ddReceipt, action string) (Result, error) {
	if r.Submission == "rejected" {
		return r.result(action), failure("publish.dd_rejected", "DD rejected the original operation; retain its receipt and use a new request for corrected input")
	}
	return r.result(action), &Error{Code: "publish.dd_pending", Message: "retain the original request; inspect operation status and verify without resubmitting", Exit: 6}
}
func ddMutationFailure(ctx context.Context, home string, r ddReceipt, action string, err error) (Result, error) {
	var pe *Error
	if errors.As(err, &pe) && pe.Code == "publish.dd_rejected" {
		r.Submission = "rejected"
		if saveErr := ddSave(context.WithoutCancel(ctx), home, r); saveErr != nil {
			return r.result(action), saveErr
		}
	}
	return ddPending(r, action)
}
func ddMutationRun(ctx context.Context, api ddAPI, req Request, action string, in map[string]any) (Result, error) {
	if _, err := ddReceiptPath(req.Home, req.RequestID); err != nil {
		return Result{}, err
	}
	author := ddText(api.identity()["author_id"])
	if author == "" {
		return Result{}, capability("publish.dd_identity", "task omitted author identity")
	}
	local := map[string]any{}
	for k, v := range in {
		if k != "session" {
			local[k] = v
		}
	}
	files := []*os.File{}
	defer func() {
		for _, f := range files {
			f.Close()
		}
	}()
	selected := map[string]any{}
	for _, key := range []string{"file", "logo_file", "wa_file", "detail_img_files", "display_img_files"} {
		paths := []any{}
		if p := String(in, key); p != "" {
			paths = append(paths, p)
		} else {
			paths = ddSlice(in[key])
		}
		for _, v := range paths {
			p := ddText(v)
			f, e := openArtifact(p)
			if e != nil {
				return Result{}, e
			}
			files = append(files, f)
			hash := sha256.New()
			if _, e = io.Copy(hash, f); e != nil {
				return Result{}, e
			}
			selected[p] = hex.EncodeToString(hash.Sum(nil))
		}
	}
	local["original_file_hashes"] = selected
	digest := ddDigest(map[string]any{"action": action, "author": author, "input": local})
	lease, err := vault.AcquireLease(ctx, filepath.Join(req.Home, "publish", "locks"), "dd/"+author)
	if err != nil {
		return Result{}, err
	}
	defer lease.Close()
	old, err := ddLoad(req.Home, req.RequestID)
	if err == nil {
		if old.Digest != digest || old.Author != author || old.Action != action {
			return Result{}, invalid("$request", "original request identifies different input or author")
		}
		if old.Verification == "verified" {
			return old.result(action), nil
		}
		return ddPending(old, action)
	}
	if !errors.Is(err, os.ErrNotExist) {
		return Result{}, err
	}
	r, verb, _ := strings.Cut(action, " ")
	form, game, err := ddPrepareForm(ctx, api, r, verb, in)
	if err != nil {
		return Result{}, err
	}
	if err = ddUploadForm(ctx, api, req.Home, r, form, in); err != nil {
		return Result{}, err
	}
	receipt := ddReceipt{Schema: "lycheedev.publish.dd-operation.v1", Request: req.RequestID, Session: String(in, "session"), Author: author, Action: action, Digest: digest, SN: String(in, "sn"), Game: game, Submission: "intent_recorded", Verification: "pending", Expected: form, Created: time.Now().UTC()}
	if err = ddSave(ctx, req.Home, receipt); err != nil {
		return Result{}, err
	}
	path := "/" + ddResource(r) + "/modify"
	if verb == "create" {
		path = "/" + ddResource(r) + "/create"
	}
	if verb == "delete" {
		path = "/" + ddResource(r) + "/delete"
	}
	if verb == "rename" {
		path = "/backup/modify_name"
	}
	receipt.Submission = "uncertain"
	if err = ddSave(ctx, req.Home, receipt); err != nil {
		return receipt.result(action), err
	}
	response, err := api.call(ctx, path, nil, form, true)
	if err != nil {
		return ddMutationFailure(ctx, req.Home, receipt, action, err)
	}
	receipt.Submission = "acknowledged"
	if o, ok := response["result"].(map[string]any); ok {
		for _, k := range []string{"sn", "share_sn"} {
			if sn := ddText(o[k]); sn != "" {
				receipt.SN = sn
				break
			}
		}
	}
	if err = ddSave(ctx, req.Home, receipt); err != nil {
		return ddPending(receipt, action)
	}
	return ddVerify(ctx, api, req.Home, receipt, action)
}
func ddVerify(ctx context.Context, api ddAPI, home string, r ddReceipt, action string) (Result, error) {
	if ddText(api.identity()["author_id"]) != r.Author {
		return Result{}, invalid("$.session", "selected DD author differs from original operation")
	}
	if r.Submission == "intent_recorded" || r.Submission == "rejected" {
		return ddPending(r, action)
	}
	if r.SN == "" {
		return ddPending(r, action)
	}
	resource, verb, _ := strings.Cut(r.Action, " ")
	if verb == "delete" {
		var rows []any
		var err error
		if resource == "backup" {
			v, e := api.call(ctx, "/backup/list", nil, nil, false)
			err = e
			if err == nil {
				rows, err = ddItems(v["result"])
				if total, known, e := ddTotal(v); e != nil || known && total != int64(len(rows)) {
					return ddPending(r, action)
				}
			}
		} else {
			rows, err = ddAllAuthor(ctx, api, resource, r.Game)
		}
		if err != nil {
			return ddPending(r, action)
		}
		for _, v := range rows {
			o, _ := v.(map[string]any)
			if ddText(o["sn"]) == r.SN || ddText(o["share_sn"]) == r.SN {
				return ddPending(r, action)
			}
		}
	} else {
		current, err := ddDetail(ctx, api, resource, r.SN)
		if err != nil {
			return ddPending(r, action)
		}
		if resource == "plugin" {
			if !ddPluginMatches(ctx, api, r, current) {
				return ddPending(r, action)
			}
		} else {
			if resource == "config" {
				// The editor sends 0/1, while share detail returns a boolean.
				if paid, ok := current["need_buy"].(bool); ok {
					current["need_buy"] = json.Number("0")
					if paid {
						current["need_buy"] = json.Number("1")
					}
				}
				for _, group := range []string{"known_addon", "unknown_addon", "material", "font", "known_wa", "unknown_wa"} {
					if current[group] == nil {
						current[group] = map[string]any{"items": []any{}, "inner_version": map[string]any{}}
					}
				}
				if current["wtf"] == nil {
					current["wtf"] = map[string]any{"accounts": []any{}}
				}
				if current["vip_levels"] == nil {
					current["vip_levels"] = []any{}
				}
			}
			for _, field := range []struct {
				k       string
				integer bool
				aliases []string
			}{{"game_versions", false, []string{"version", "game_version", "build", "value"}}, {"detail_imgs", false, []string{"url", "path"}}, {"display_imgs", false, []string{"url", "path"}}, {"category_ids", false, []string{"c_id", "category_id", "id", "value"}}, {"vip_levels", true, []string{"level", "id", "value"}}} {
				if v, ok := current[field.k]; ok {
					a, e := ddArray(v, field.integer, field.aliases...)
					if e != nil {
						return ddPending(r, action)
					}
					current[field.k] = a
				}
			}
			for k, want := range r.Expected {
				if k == "sn" || k == "share_sn" {
					continue
				}
				got, ok := current[k]
				if !ok || !ddSubset(want, got) {
					return ddPending(r, action)
				}
			}
		}
	}
	r.Verification = "verified"
	if err := ddSave(ctx, home, r); err != nil {
		return ddPending(r, action)
	}
	return r.result(action), nil
}
func ddEqual(a, b any) bool {
	if ddText(a) != "" && ddText(a) == ddText(b) {
		return true
	}
	return ddDigest(a) == ddDigest(b)
}
