package publish

import (
	"context"
	"strings"
)

func RunDD(ctx context.Context, action string, req Request) (result Result, err error) {
	result = Result{Platform: "dd", Action: action}
	spec, ok := DDInput(action)
	if !ok {
		return result, invalid("$action", "unknown DD action")
	}
	input, err := spec.Decode(req.Input)
	if err != nil {
		return result, err
	}
	if err = ctx.Err(); err != nil {
		return result, err
	}
	if req.Plan {
		local, e := ddLocalPlan(ctx, action, input)
		if e != nil {
			return result, e
		}
		result.Complete = true
		result.Data = map[string]any{"input": ddPublic(input), "effects": spec.Effects, "local": local, "remote_write": false, "authentication": false, "remote_validation": false}
		return result, nil
	}
	if strings.HasPrefix(action, "session ") {
		return runDDSession(ctx, action, req, input)
	}
	switch action {
	case "wa parse":
		raw, e := ddReadFile(String(input, "wa_file"), 32<<20)
		if e != nil {
			return result, e
		}
		result.Data, err = parseDDWA(ctx, string(raw))
		result.Complete = err == nil
		return result, err
	case "options life-types":
		result.Complete = true
		result.Data = map[string]any{"items": ddLifeTypes, "remote_read": false}
		return result, nil
	case "backup scan":
		result.Data, err = ddBackupScan(String(input, "root"))
		result.Complete = err == nil
		return result, err
	case "backup generate":
		result.Data, err = ddBackupGenerate(ctx, input)
		result.Complete = err == nil
		return result, err
	case "operation status":
		r, e := ddLoad(req.Home, String(input, "request"))
		if e != nil {
			return result, e
		}
		return r.result(action), nil
	}
	api, err := attachDDSession(ctx, req.Home, String(input, "session"))
	if err != nil {
		return result, err
	}
	defer func() {
		if e := api.close(); e != nil {
			result.PendingCleanup = true
			if err == nil {
				err = e
			}
		}
	}()
	if action == "backup delete-preview" {
		result.Data, err = ddBackupDeletePreview(ctx, api, input)
		result.Complete = err == nil
		return result, err
	}
	if action == "backup delete-many" {
		return ddDeleteBackups(ctx, api, req, input, false, false)
	}
	if action == "operation resume" {
		r, e := ddLoad(req.Home, String(input, "request"))
		if e != nil {
			return result, e
		}
		if r.Action == "backup delete-many" {
			return ddDeleteBackups(ctx, api, req, input, true, false)
		}
		if r.Action == "backup upload" {
			return ddResumeBackup(ctx, api, req.Home, r)
		}
		return ddPending(r, action)
	}
	if action == "operation verify" {
		r, e := ddLoad(req.Home, String(input, "request"))
		if e != nil {
			return result, e
		}
		if r.Action == "backup delete-many" {
			return ddDeleteBackups(ctx, api, req, input, true, true)
		}
		if sn := String(input, "sn"); sn != "" {
			if r.SN != "" && r.SN != sn {
				return result, invalid("$.sn", "original operation has a different resource identity")
			}
			r.SN = sn
		}
		if r.Action == "backup upload" {
			return ddVerifyBackup(ctx, api, req.Home, r, action)
		}
		return ddVerify(ctx, api, req.Home, r, action)
	}
	if action == "backup upload" {
		return ddBackupUpload(ctx, api, req, input)
	}
	if strings.HasSuffix(action, " upload") {
		r, _, _ := strings.Cut(action, " ")
		result.Data, err = ddUpload(ctx, api, req.Home, String(input, "file"), r+" "+String(input, "kind"))
		result.Complete = err == nil
		result.Submission = "acknowledged"
		if err != nil {
			result.Submission = "uncertain"
		}
		return result, err
	}
	if ddMutation(action) {
		return ddMutationRun(ctx, api, req, action, input)
	}
	result.Data, err = ddRead(ctx, api, action, input)
	if err != nil {
		return result, err
	}
	result.Data = ddPublic(result.Data)
	result.Complete = true
	return result, nil
}
