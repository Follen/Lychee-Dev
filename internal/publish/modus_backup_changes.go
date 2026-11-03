package publish

import (
	"context"
	"errors"
	"strconv"
	"strings"
	"time"
)

func modusBackupID(v any) (string, error) {
	id := fmtModusID(v)
	n, err := strconv.ParseUint(id, 10, 63)
	if err != nil || n == 0 {
		return "", failure("publish.modus_backup_shape", "backup identity must be a positive integer")
	}
	return id, nil
}
func modusBackupList(ctx context.Context, c *modusClient, build int64) ([]map[string]any, error) {
	v, err := c.call(ctx, "GET", "system/user/backup/list", nil, &build)
	if err != nil {
		return nil, err
	}
	rows, err := modusRows(v)
	if err != nil {
		return nil, err
	}
	out := []map[string]any{}
	seen := map[string]bool{}
	for _, v := range rows {
		row, ok := v.(map[string]any)
		if !ok {
			return nil, failure("publish.modus_backup_shape", "invalid backup list row")
		}
		id, err := modusBackupID(modusResponseID(row, "id", "backupId"))
		if err != nil || seen[id] {
			return nil, failure("publish.modus_backup_shape", "duplicate or invalid backup list ID")
		}
		seen[id] = true
		out = append(out, row)
	}
	return out, nil
}
func modusBackupDetail(ctx context.Context, c *modusClient, id string, build int64) (map[string]any, error) {
	v, err := c.call(ctx, "GET", "system/user/backup/detail/"+id, nil, &build)
	if err != nil {
		return nil, err
	}
	if rows, ok := v.([]any); ok {
		if len(rows) != 1 {
			return nil, failure("publish.modus_backup_identity", "backup detail must contain one selected item")
		}
		v = rows[0]
	}
	row, ok := v.(map[string]any)
	if !ok || modusResponseID(row, "id", "backupId") != id {
		return nil, failure("publish.modus_backup_identity", "backup detail identity mismatch")
	}
	if n, ok := positiveOrZero(row["serverType"]); !ok || n != build {
		return nil, invalid("$.server_type", "backup belongs to another Build or lacks Build identity")
	}
	return row, nil
}
func modusBackupReferences(ctx context.Context, c *modusClient) (map[string][]map[string]any, error) {
	refs := map[string][]map[string]any{}
	globalShares := map[string]string{}
	for build := int64(0); build < 5; build++ {
		seen := map[string]bool{}
		expected := int64(-1)
		for page := int64(1); page <= 10000; page++ {
			v, err := c.call(ctx, "POST", "system/user/share/list", map[string]any{"mine": true, "server": build, "shareType": 0, "pageNum": page, "pageSize": 100, "platform": 0}, &build)
			if err != nil {
				return nil, err
			}
			rows, err := modusRows(v)
			if err != nil {
				return nil, err
			}
			envelope, ok := v.(map[string]any)
			if !ok {
				return nil, failure("publish.modus_backup_protection", "share pagination must include total")
			}
			total, ok := positiveOrZero(envelope["total"])
			if !ok || expected >= 0 && expected != total {
				return nil, failure("publish.modus_backup_protection", "share count is missing or changed")
			}
			expected = total
			ids := []any{}
			for _, v := range rows {
				row, ok := v.(map[string]any)
				if !ok {
					return nil, failure("publish.modus_backup_protection", "invalid share list row")
				}
				id := modusResponseID(row, "id", "shareId")
				if id == "" || len(id) > 256 || strings.ContainsAny(id, "\r\n\x00") || seen[id] {
					return nil, failure("publish.modus_backup_protection", "share list contains duplicate or invalid IDs")
				}
				seen[id] = true
				ids = append(ids, id)
			}
			if int64(len(seen)) > total || len(ids) == 0 && int64(len(seen)) < total {
				return nil, failure("publish.modus_backup_protection", "share pagination is incomplete")
			}
			if len(ids) > 0 {
				v, err = c.call(ctx, "POST", "system/user/share/detail", map[string]any{"shareIds": ids}, &build)
				if err != nil {
					return nil, err
				}
				details, err := modusRows(v)
				if err != nil {
					return nil, err
				}
				wanted := map[string]bool{}
				for _, v := range ids {
					wanted[v.(string)] = true
				}
				found := map[string]bool{}
				for _, v := range details {
					row, ok := v.(map[string]any)
					if !ok {
						return nil, failure("publish.modus_backup_protection", "invalid share detail")
					}
					id := modusResponseID(row, "id", "shareId")
					if !wanted[id] || found[id] {
						return nil, failure("publish.modus_backup_protection", "share detail does not match requested IDs")
					}
					found[id] = true
					backup, err := modusBackupID(row["backupId"])
					if err != nil {
						return nil, failure("publish.modus_backup_protection", "share backup association is unknown")
					}
					if old := globalShares[id]; old != "" && old != backup {
						return nil, failure("publish.modus_backup_protection", "share association differs between Builds")
					}
					if globalShares[id] == "" {
						refs[backup] = append(refs[backup], map[string]any{"share_id": id, "title": row["title"], "checked_server_type": build})
						globalShares[id] = backup
					}
				}
				if len(found) != len(wanted) {
					return nil, failure("publish.modus_backup_protection", "share detail is incomplete")
				}
			}
			if int64(len(seen)) == total {
				break
			}
			if page == 10000 {
				return nil, failure("publish.modus_backup_protection", "share pagination exceeds limit")
			}
		}
	}
	return refs, nil
}
func previewModusBackup(ctx context.Context, c *modusClient, in map[string]any) (any, error) {
	selected := map[string]bool{}
	if values, ok := in["backup_ids"].([]any); ok {
		for _, v := range values {
			id, err := modusBackupID(v)
			if err != nil {
				return nil, err
			}
			if selected[id] {
				return nil, invalid("$.backup_ids", "duplicate backup ID")
			}
			selected[id] = true
		}
	}
	if len(selected) == 0 && String(in, "name_contains") == "" && String(in, "name_equals") == "" && String(in, "created_before") == "" && String(in, "created_after") == "" {
		return nil, invalid("$", "select explicit backup IDs or a name/date filter")
	}
	times := map[string]time.Time{}
	for _, k := range []string{"created_before", "created_after"} {
		if text := String(in, k); text != "" {
			stamp, err := time.Parse(time.RFC3339, text)
			if err != nil {
				return nil, invalid("$."+k, "use an ISO 8601 timestamp with timezone")
			}
			times[k] = stamp
		}
	}
	build := Integer(in, "server_type")
	rows, err := modusBackupList(ctx, c, build)
	if err != nil {
		return nil, err
	}
	candidates := []map[string]any{}
	found := map[string]bool{}
	for _, row := range rows {
		id, _ := modusBackupID(modusResponseID(row, "id", "backupId"))
		if len(selected) > 0 && !selected[id] {
			continue
		}
		name := String(row, "backupName")
		if name == "" {
			name = String(row, "name")
		}
		if value := String(in, "name_equals"); value != "" && value != name {
			continue
		}
		if value := String(in, "name_contains"); value != "" && !strings.Contains(name, value) {
			continue
		}
		if len(times) > 0 {
			text := String(row, "createTime")
			if text == "" {
				text = String(row, "createdAt")
			}
			stamp, err := time.Parse(time.RFC3339, text)
			if err != nil {
				stamp, err = time.Parse("2006-01-02 15:04:05", text)
			}
			if err != nil {
				return nil, failure("publish.modus_backup_shape", "backup creation timestamp is invalid")
			}
			if before, ok := times["created_before"]; ok && !stamp.Before(before) {
				continue
			}
			if after, ok := times["created_after"]; ok && !stamp.After(after) {
				continue
			}
		}
		actual := row
		if actual["serverType"] == nil {
			actual, err = modusBackupDetail(ctx, c, id, build)
			if err != nil {
				return nil, err
			}
		}
		actualBuild, ok := positiveOrZero(actual["serverType"])
		if !ok || actualBuild > 4 {
			return nil, failure("publish.modus_backup_protection", "backup Build is missing or invalid")
		}
		if actualBuild != build {
			if selected[id] {
				return nil, invalid("$.server_type", "selected backup belongs to another Build")
			}
			continue
		}
		found[id] = true
		candidates = append(candidates, map[string]any{"backup_id": id, "backup_name": name, "server_type": build, "created_at": actual["createTime"]})
	}
	if len(selected) > 0 {
		for id := range selected {
			if !found[id] && String(in, "name_contains") == "" && String(in, "name_equals") == "" && len(times) == 0 {
				return nil, invalid("$.backup_ids", "selected backup is absent from current list")
			}
		}
	}
	refs := map[string][]map[string]any{}
	if len(candidates) > 0 {
		refs, err = modusBackupReferences(ctx, c)
		if err != nil {
			return nil, err
		}
	}
	deletable, protected := []string{}, []string{}
	for _, item := range candidates {
		id := item["backup_id"].(string)
		linked := refs[id]
		item["protected"] = len(linked) > 0
		item["references"] = linked
		if len(linked) > 0 {
			protected = append(protected, id)
		} else {
			deletable = append(deletable, id)
		}
	}
	return map[string]any{"server_type": build, "items": candidates, "matched_count": len(candidates), "deletable_ids": deletable, "protected_ids": protected, "protection_complete": true}, nil
}
func verifyModusBackupDeleted(ctx context.Context, c *modusClient, id string, build int64) error {
	rows, err := modusBackupList(ctx, c, build)
	if err != nil {
		return err
	}
	for _, row := range rows {
		if modusResponseID(row, "id", "backupId") == id {
			return modusBackupPending("original backup is still present; deletion will not be replayed")
		}
	}
	v, err := c.call(ctx, "GET", "system/user/backup/detail/"+id, nil, &build)
	if err == nil {
		if v == nil {
			return nil
		}
		if obj, ok := v.(map[string]any); ok && len(obj) == 0 {
			return nil
		}
		if list, ok := v.([]any); ok && len(list) == 0 {
			return nil
		}
		return modusBackupPending("backup detail is still present")
	}
	var e *Error
	if errors.As(err, &e) {
		detail, _ := e.Details.(map[string]any)
		if detail["httpStatus"] == 404 || fmtModusID(detail["businessCode"]) == "404" {
			return nil
		}
	}
	return err
}
func continueModusBackupChange(ctx context.Context, c *modusClient, home string, s *modusBackupState, verifyOnly bool) error {
	if s.Phase == "rejected" {
		return failure("publish.modus_rejected", "the original write was rejected and will not be replayed")
	}
	if s.Action == "backup rename" {
		id := fmtModusID(s.Input["backup_id"])
		if s.Phase == "prepare" {
			if verifyOnly {
				return modusBackupPending("rename has not been submitted")
			}
			if _, err := modusBackupDetail(ctx, c, id, s.Build); err != nil {
				return err
			}
			s.CloudID = id
			s.Phase = "rename-intent"
			s.Body = map[string]any{"id": s.Input["backup_id"], "backupName": s.Input["backup_name"]}
			if err := saveModusBackup(ctx, home, s); err != nil {
				return err
			}
			if _, err := c.call(ctx, "POST", "system/user/backup/update", s.Body, &s.Build); err != nil {
				return modusBackupWriteFailure(ctx, home, s, err)
			}
			s.Phase = "rename-readback"
			if err := saveModusBackup(ctx, home, s); err != nil {
				return err
			}
		}
		row, err := modusBackupDetail(ctx, c, id, s.Build)
		if err != nil {
			return err
		}
		if row["backupName"] != s.Input["backup_name"] && row["name"] != s.Input["backup_name"] {
			return modusBackupPending("original rename has not been verified; it will not be replayed")
		}
		s.Phase = "complete"
		s.Complete = true
		return saveModusBackup(ctx, home, s)
	}
	if !Bool(s.Input, "confirm_delete") {
		return invalid("$.confirm_delete", "must be true")
	}
	if s.Phase == "prepare" {
		values, _ := s.Input["backup_ids"].([]any)
		if len(values) == 0 {
			return invalid("$.backup_ids", "select explicit IDs")
		}
		seen := map[string]bool{}
		for _, v := range values {
			id, err := modusBackupID(v)
			if err != nil {
				return err
			}
			if seen[id] {
				return invalid("$.backup_ids", "duplicate backup ID")
			}
			seen[id] = true
			s.Items = append(s.Items, modusBackupDeleteItem{ID: id, Phase: "pending"})
		}
		s.Phase = "deleting"
		if err := saveModusBackup(ctx, home, s); err != nil {
			return err
		}
	}
	for i := range s.Items {
		item := &s.Items[i]
		if item.Phase == "complete" || item.Protected {
			continue
		}
		if item.Phase == "delete-intent" || item.Phase == "readback" {
			if err := verifyModusBackupDeleted(ctx, c, item.ID, s.Build); err != nil {
				return err
			}
			item.Phase = "complete"
			if err := saveModusBackup(ctx, home, s); err != nil {
				return err
			}
			continue
		}
		if verifyOnly {
			return modusBackupPending("unsubmitted backup deletion remains; resume the original operation")
		}
		if _, err := modusBackupDetail(ctx, c, item.ID, s.Build); err != nil {
			return err
		}
		refs, err := modusBackupReferences(ctx, c)
		if err != nil {
			return err
		}
		if len(refs[item.ID]) > 0 {
			item.Protected = true
			item.Phase = "protected"
			item.References = refs[item.ID]
			if err = saveModusBackup(ctx, home, s); err != nil {
				return err
			}
			continue
		}
		item.Phase = "delete-intent"
		if err = saveModusBackup(ctx, home, s); err != nil {
			return err
		}
		if _, err = c.call(ctx, "DELETE", "system/user/backup/delete/"+item.ID, nil, &s.Build); err != nil {
			return modusBackupWriteFailure(ctx, home, s, err)
		}
		item.Phase = "readback"
		if err = saveModusBackup(ctx, home, s); err != nil {
			return err
		}
		if err = verifyModusBackupDeleted(ctx, c, item.ID, s.Build); err != nil {
			return err
		}
		item.Phase = "complete"
		if err = saveModusBackup(ctx, home, s); err != nil {
			return err
		}
	}
	s.Phase = "complete"
	s.Complete = true
	if err := saveModusBackup(ctx, home, s); err != nil {
		return err
	}
	for _, item := range s.Items {
		if item.Protected {
			return failure("publish.modus_backup_in_use", "referenced backups were retained; unprotected selected backups were processed")
		}
	}
	return nil
}
