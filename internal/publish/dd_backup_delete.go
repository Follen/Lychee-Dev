package publish

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"time"

	"github.com/follenfang/lycheedev/internal/vault"
)

func ddBackupList(ctx context.Context, api ddAPI) ([]any, error) {
	v, err := api.call(ctx, "/backup/list", nil, nil, false)
	if err != nil {
		return nil, err
	}
	rows, err := ddItems(v["result"])
	if err != nil {
		return nil, err
	}
	total, known, err := ddTotal(v)
	if err != nil {
		return nil, err
	}
	if known && total != int64(len(rows)) {
		return nil, failure("publish.dd_pagination", "backup list is incomplete")
	}
	seen := map[string]bool{}
	for _, v := range rows {
		id := ddText(ddMap(v)["sn"])
		if id == "" || seen[id] {
			return nil, failure("publish.dd_pagination", "backup list has missing or duplicate IDs")
		}
		seen[id] = true
	}
	return rows, nil
}

func ddBackupReferences(ctx context.Context, api ddAPI, backups []any) (map[string][]any, error) {
	v, err := api.call(ctx, "/game_type/list", nil, nil, false)
	if err != nil {
		return nil, err
	}
	rows, err := ddItems(v["result"])
	if err != nil {
		return nil, err
	}
	games := map[int64]bool{}
	for _, v := range rows {
		if g := ddGame(ddMap(v)); g > 0 {
			games[g] = true
		}
	}
	if len(games) == 0 {
		return nil, failure("publish.dd_reference", "current game types are unavailable")
	}
	for _, v := range backups {
		if g := ddGame(ddMap(v)); g > 0 {
			games[g] = true
		}
	}
	ordered := []int64{}
	for g := range games {
		ordered = append(ordered, g)
	}
	sort.Slice(ordered, func(i, j int) bool { return ordered[i] < ordered[j] })
	refs := map[string][]any{}
	for _, g := range ordered {
		seen := map[string]bool{}
		expected := int64(-1)
		for page := int64(1); page <= 1000; page++ {
			v, err := ddAuthorPage(ctx, api, "config", g, "", page, 100)
			if err != nil {
				return nil, err
			}
			rows, err := ddItems(v["result"])
			if err != nil {
				return nil, err
			}
			total, known, err := ddTotal(v)
			if err != nil {
				return nil, err
			}
			if !known || expected >= 0 && expected != total {
				return nil, failure("publish.dd_reference", "configuration reference total is absent or changed")
			}
			expected = total
			for _, v := range rows {
				row := ddMap(v)
				id := ddText(row["sn"])
				if id == "" {
					id = ddText(row["share_sn"])
				}
				if id == "" || seen[id] {
					return nil, failure("publish.dd_reference", "configuration references repeat or omit identities")
				}
				seen[id] = true
				backup := ddText(row["backup_sn"])
				if backup == "" {
					detail, err := ddDetail(ctx, api, "config", id)
					if err != nil {
						return nil, err
					}
					if ddText(detail["sn"]) != id && ddText(detail["share_sn"]) != id {
						return nil, failure("publish.dd_reference", "configuration detail omitted original identity")
					}
					backup = ddText(detail["backup_sn"])
				}
				if backup == "" {
					return nil, failure("publish.dd_reference", "configuration backup association is unknown")
				}
				refs[backup] = append(refs[backup], map[string]any{"sn": id, "game_type": g})
			}
			if int64(len(seen)) == total {
				break
			}
			if len(rows) == 0 || int64(len(seen)) > total || page == 1000 {
				return nil, failure("publish.dd_reference", "configuration reference pagination is incomplete")
			}
		}
	}
	return refs, nil
}

func ddBackupDeletePreview(ctx context.Context, api ddAPI, in map[string]any) (map[string]any, error) {
	rows, err := ddBackupList(ctx, api)
	if err != nil {
		return nil, err
	}
	ids := []string{}
	selected := map[string]bool{}
	for _, v := range ddSlice(in["backup_ids"]) {
		id := ddText(v)
		if id == "" || selected[id] {
			return nil, invalid("$.backup_ids", "choose unique nonempty IDs")
		}
		ids = append(ids, id)
		selected[id] = true
	}
	before, after := time.Time{}, time.Time{}
	for k, dst := range map[string]*time.Time{"created_before": &before, "created_after": &after} {
		if s := String(in, k); s != "" {
			parsed, e := time.Parse(time.RFC3339, s)
			if e != nil {
				parsed, e = time.Parse("2006-01-02", s)
			}
			if e != nil {
				return nil, invalid("$."+k, "expected an ISO creation date")
			}
			*dst = parsed
		}
	}
	_, contains := in["name_contains"]
	_, equals := in["name_equals"]
	filtered := contains || equals || !before.IsZero() || !after.IsZero()
	if len(ids) == 0 && !filtered {
		return nil, invalid("$.backup_ids", "provide selected IDs or a preview filter")
	}
	backups := map[string]map[string]any{}
	if filtered {
		ids = nil
	}
	for _, v := range rows {
		row := ddMap(v)
		id := ddText(row["sn"])
		backups[id] = row
		if !filtered {
			continue
		}
		if len(selected) > 0 && !selected[id] {
			continue
		}
		name := ddText(row["name"])
		if contains && !strings.Contains(name, String(in, "name_contains")) || equals && name != String(in, "name_equals") {
			continue
		}
		if !before.IsZero() || !after.IsZero() {
			n := ddInteger(row["ctime"])
			if n <= 0 {
				return nil, failure("publish.dd_reference", "backup creation time is unavailable")
			}
			created := time.Unix(n, 0)
			if !before.IsZero() && !created.Before(before) || !after.IsZero() && !created.After(after) {
				continue
			}
		}
		ids = append(ids, id)
	}
	refs, err := ddBackupReferences(ctx, api, rows)
	if err != nil {
		return nil, err
	}
	items := []any{}
	allowed := len(ids) > 0
	for _, id := range ids {
		row, present := backups[id]
		deletable := present && len(refs[id]) == 0
		allowed = allowed && deletable
		items = append(items, map[string]any{"sn": id, "name": row["name"], "present": present, "references": refs[id], "deletable": deletable})
	}
	return map[string]any{"items": items, "deletable": allowed, "references_complete": true, "matched": len(ids)}, nil
}

type ddDeleteBatch struct {
	IDs       []string `json:"ids"`
	Completed []string `json:"completed"`
	Current   string   `json:"current,omitempty"`
}

func ddDeleteBackups(ctx context.Context, api ddAPI, req Request, in map[string]any, resume, readOnly bool) (Result, error) {
	action := "backup delete-many"
	if resume {
		action = "operation resume"
	}
	if readOnly {
		action = "operation verify"
	}
	id := req.RequestID
	if resume {
		id = String(in, "request")
	}
	if _, err := ddReceiptPath(req.Home, id); err != nil {
		return Result{}, err
	}
	author := ddText(api.identity()["author_id"])
	if author == "" {
		return Result{}, capability("publish.dd_identity", "task omitted author identity")
	}
	lease, err := vault.AcquireLease(ctx, filepath.Join(req.Home, "publish", "locks"), "dd/"+author)
	if err != nil {
		return Result{}, err
	}
	defer lease.Close()
	r, err := ddLoad(req.Home, id)
	if errors.Is(err, os.ErrNotExist) && !resume {
		if in["confirm_delete"] != true {
			return Result{}, invalid("$.confirm_delete", "must be true")
		}
		p, e := ddBackupDeletePreview(ctx, api, in)
		if e != nil {
			return Result{}, e
		}
		if p["deletable"] != true {
			return Result{}, invalid("$.backup_ids", "selected backups are absent or referenced")
		}
		ids := []string{}
		for _, v := range p["items"].([]any) {
			ids = append(ids, ddText(ddMap(v)["sn"]))
		}
		r = ddReceipt{Schema: "lycheedev.publish.dd-operation.v1", Request: id, Session: String(in, "session"), Author: author, Action: "backup delete-many", Digest: ddDigest(in["backup_ids"]), Submission: "prepared", Verification: "pending", Batch: &ddDeleteBatch{IDs: ids}, Created: time.Now().UTC()}
		if err = ddSave(ctx, req.Home, r); err != nil {
			return Result{}, err
		}
	} else if err != nil {
		return Result{}, err
	}
	if r.Author != author || r.Action != "backup delete-many" || r.Batch == nil || !resume && r.Digest != ddDigest(in["backup_ids"]) {
		return Result{}, invalid("$.request", "original batch has different input or author")
	}
	if r.Submission == "rejected" {
		return ddPending(r, action)
	}
	if r.Verification == "verified" {
		return r.result(action), nil
	}
	for len(r.Batch.Completed) < len(r.Batch.IDs) {
		target := r.Batch.IDs[len(r.Batch.Completed)]
		if r.Batch.Current != "" {
			rows, e := ddBackupList(ctx, api)
			if e != nil {
				return ddPending(r, action)
			}
			for _, v := range rows {
				if ddText(ddMap(v)["sn"]) == target {
					return ddPending(r, action)
				}
			}
		} else {
			if readOnly {
				return ddPending(r, action)
			}
			p, e := ddBackupDeletePreview(ctx, api, map[string]any{"backup_ids": []any{target}})
			if e != nil {
				return Result{}, e
			}
			if p["deletable"] != true {
				return ddPending(r, action)
			}
			r.Batch.Current = target
			r.Submission = "uncertain"
			if e = ddSave(ctx, req.Home, r); e != nil {
				return Result{}, e
			}
			if _, e = api.call(ctx, "/backup/delete", nil, map[string]any{"sn": target}, true); e != nil {
				return ddMutationFailure(ctx, req.Home, r, action, e)
			}
			continue
		}
		r.Batch.Completed = append(r.Batch.Completed, target)
		r.Batch.Current = ""
		r.Submission = "prepared"
		if err = ddSave(ctx, req.Home, r); err != nil {
			return Result{}, err
		}
	}
	r.Submission = "acknowledged"
	r.Verification = "verified"
	if err = ddSave(ctx, req.Home, r); err != nil {
		return Result{}, err
	}
	return r.result(action), nil
}
