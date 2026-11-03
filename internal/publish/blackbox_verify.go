package publish

import (
	"context"
	"strings"
)

func (c *bbClient) verify(ctx context.Context, op *bbOperation) error {
	if op.Stage == "rejected" {
		return failure("publish.blackbox_rejected", "the original write was rejected and will not be replayed")
	}
	if strings.HasPrefix(op.Action, "backup ") {
		return c.verifyBackup(ctx, op)
	}
	if len(op.Steps) == 0 || op.Steps[0].State == "prepared" {
		return bbVerification()
	}
	var actual map[string]any
	var e error
	matched := false
	switch op.Action {
	case "plugin edit":
		actual, e = c.module(ctx, op.Target)
		matched = e == nil && bbMatches(actual, op.Expected)
	case "plugin update", "version edit", "version delete":
		rows, options, err := c.versions(ctx, bbID(op.Input["module_id"]))
		if err != nil {
			return err
		}
		candidates := []map[string]any{}
		for _, row := range rows {
			if op.Target != "" {
				if bbID(row["id"]) == op.Target {
					candidates = append(candidates, row)
				}
			} else if !bbContains(op.Previous, bbID(row["id"])) && bbMatches(row, op.Expected) {
				candidates = append(candidates, row)
			}
		}
		if op.Action == "version delete" {
			matched = len(candidates) == 0 || (len(candidates) == 1 && bbID(candidates[0]["auditState"]) == "4")
			break
		}
		if len(candidates) != 1 {
			return bbVerification()
		}
		actual = candidates[0]
		matched = bbMatches(actual, op.Expected)
		mode := bbArchiveMode(op, actual, options)
		matched = matched && mode != ""
		if matched {
			op.Target = bbID(actual["id"])
			op.Evidence = map[string]any{"archiveMode": mode, "archiveBytesVerified": false, "publicDownloadVerified": false, "verifiedFields": []string{"version_id", "name", "type", "gameVersions", "archive_url"}, "unverifiedFields": []string{"archive_bytes"}}
		}
	case "config delete":
		rows, err := c.shares(ctx, op.Branch)
		if err != nil {
			return err
		}
		matched = true
		for _, row := range rows {
			if bbID(row["ShareID"]) == op.Target {
				matched = false
			}
		}
	case "config create", "config edit":
		rows, err := c.shares(ctx, op.Branch)
		if err != nil {
			return err
		}
		matches := []map[string]any{}
		for _, row := range rows {
			id := bbID(row["ShareID"])
			if op.Target != "" && id != op.Target {
				continue
			}
			if op.Target == "" && bbContains(op.Previous, id) {
				continue
			}
			detail, err := c.share(ctx, id)
			if err != nil {
				return err
			}
			if bbID(detail["BackupID"]) != bbID(op.Expected["ID"]) {
				continue
			}
			test := bbClone(detail)
			if level, ok := test["SubscribeLevel"].(map[string]any); ok {
				test["SubscribeLevel"] = level["level"]
			}
			want := map[string]any{}
			for _, key := range bbConfigFields {
				if key != "NeedsPush" {
					if v, ok := op.Expected[key]; ok {
						want[key] = v
					}
				}
			}
			cleared := true
			for _, key := range []string{"UnknownAddons", "Materials", "Fonts"} {
				if _, selected := op.Expected[key]; selected || test[key] == nil {
					continue
				}
				container, ok := test[key].(map[string]any)
				files, hasFiles := container["Files"].([]any)
				if !ok || !hasFiles || len(files) != 0 {
					cleared = false
				}
			}
			if cleared && bbMatches(test, want) {
				matches = append(matches, detail)
			}
		}
		matched = len(matches) == 1
		if matched {
			actual = matches[0]
			op.Target = bbID(actual["ShareID"])
		}
	case "wa create", "wa edit", "wa delete":
		rows, err := c.was(ctx, op.Branch)
		if err != nil {
			return err
		}
		if op.Action == "wa delete" {
			matched = true
			for _, row := range rows {
				if bbID(row["Slug"]) == op.Target {
					matched = false
				}
			}
			break
		}
		matches := []map[string]any{}
		for _, row := range rows {
			id := bbID(row["Slug"])
			if op.Target != "" && id != op.Target {
				continue
			}
			if op.Target == "" && (bbContains(op.Previous, id) || row["Name"] != op.Expected["Name"]) {
				continue
			}
			detail, err := c.waDetail(ctx, row)
			if err != nil {
				return err
			}
			if bbMatches(detail, op.Expected) {
				matches = append(matches, detail)
			}
		}
		matched = len(matches) == 1
		if matched {
			actual = matches[0]
			op.Target = bbID(actual["Slug"])
			op.Evidence = map[string]any{"scope": "public", "visibility": map[string]any{"verified": false, "anonymous_can_view": nil}, "unverifiedFields": []string{"subscribeLevel", "screens.thumb", "open"}}
		}
	}
	if e != nil {
		return e
	}
	if !matched {
		return bbVerification()
	}
	op.Verified = true
	op.Stage = "verified"
	for i := range op.Steps {
		op.Steps[i].State = "verified"
	}
	if op.Evidence == nil {
		op.Evidence = map[string]any{}
	}
	op.Evidence["readback"] = actual
	return nil
}
