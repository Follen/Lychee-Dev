package publish

import (
	"context"
	"encoding/json"
	"strings"
)

func bbBackupDeleteSelection(in map[string]any) ([]string, error) {
	ids := []string{}
	raw, explicit := in["backup_ids"]
	filters := false
	for _, k := range []string{"name_contains", "name_equals", "created_before", "created_after"} {
		_, ok := in[k]
		filters = filters || ok
	}
	if explicit && filters {
		return nil, invalid("$", "IDs and filters cannot be combined")
	}
	if explicit {
		a, ok := raw.([]any)
		if !ok || len(a) == 0 {
			return nil, invalid("$.backup_ids", "explicit IDs required")
		}
		seen := map[string]bool{}
		for _, v := range a {
			id, ok := v.(string)
			if !ok || id == "" {
				return nil, invalid("$.backup_ids", "invalid backup ID")
			}
			if seen[id] {
				return nil, invalid("$.backup_ids", "duplicate backup ID")
			}
			seen[id] = true
			ids = append(ids, id)
		}
	} else if !filters {
		return nil, invalid("$", "explicit IDs or filters required")
	}
	return ids, nil
}

func (c *bbClient) backupDeletePlan(ctx context.Context, in map[string]any) (map[string]any, error) {
	ids, e := bbBackupDeleteSelection(in)
	if e != nil {
		return nil, e
	}
	rows, e := c.backups(ctx)
	if e != nil {
		return nil, e
	}
	if _, explicit := in["backup_ids"]; !explicit {
		for _, r := range rows {
			name, ok := r["Name"].(string)
			if !ok {
				return nil, failure("publish.blackbox_shape", "backup name missing")
			}
			if s := String(in, "name_contains"); s != "" && !strings.Contains(name, s) {
				continue
			}
			if s := String(in, "name_equals"); s != "" && name != s {
				continue
			}
			match := true
			for _, key := range []string{"created_before", "created_after"} {
				if v, ok := in[key]; ok {
					n, ok := r["SaveTime"].(json.Number)
					if !ok {
						return nil, failure("publish.blackbox_shape", "backup timestamp missing")
					}
					a, err := n.Float64()
					if err != nil {
						return nil, err
					}
					b, ok := bbBackupInteger(v)
					if !ok {
						return nil, invalid("$."+key, "invalid timestamp")
					}
					if key == "created_before" && a >= float64(b) || key == "created_after" && a <= float64(b) {
						match = false
					}
				}
			}
			if match {
				ids = append(ids, bbID(r["ID"]))
			}
		}
	}
	refs := map[string][]string{}
	branches := map[string]bool{}
	selected := map[string]any{}
	for _, id := range ids {
		if _, ok := refs[id]; ok {
			return nil, invalid("$.backup_ids", "duplicate backup ID")
		}
		row, err := bbFind(rows, "ID", id)
		if err != nil {
			return nil, err
		}
		b, err := bbBranch(row)
		if err != nil {
			return nil, err
		}
		branches[b] = true
		refs[id] = []string{}
		selected[id] = row["Name"]
	}
	for b := range branches {
		shares, err := c.shares(ctx, b)
		if err != nil {
			return nil, err
		}
		for _, s := range shares {
			if !bbStringID(s["BackupID"]) {
				s, err = c.share(ctx, bbID(s["ShareID"]))
				if err != nil {
					return nil, err
				}
			}
			id := bbID(s["BackupID"])
			if _, ok := refs[id]; ok {
				refs[id] = append(refs[id], bbID(s["ShareID"]))
			}
		}
	}
	items := []map[string]any{}
	allowed := true
	for _, id := range ids {
		allowed = allowed && len(refs[id]) == 0
		items = append(items, map[string]any{"backup_id": id, "name": selected[id], "referenced_by": refs[id], "deletable": len(refs[id]) == 0})
	}
	return map[string]any{"items": items, "complete": true, "allowed": allowed}, nil
}
