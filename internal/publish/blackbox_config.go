package publish

import (
	"context"
	"strings"
)

var bbConfigFields = []string{"Open", "Title", "Article", "UpdateLog", "KnownAddons", "UnknownAddons", "WTFList", "Materials", "Fonts", "ClearKnownAddons", "ClearUnknownAddons", "ClearMaterials", "ClearFonts", "ThumbList", "SubscribeLevel", "ChatInviteCode", "NeedsPush"}
var bbSelections = []string{"KnownAddons", "UnknownAddons", "WTFList", "Materials", "Fonts"}

func bbValidateConfigPatch(config map[string]any) error {
	if bbTextWidth(String(config, "Title")) > 70 {
		return invalid("$.configuration.Title", "title exceeds official weighted 70-character limit")
	}
	if config["Open"] == true {
		if thumbs, ok := config["ThumbList"].([]any); ok && len(thumbs) == 0 {
			return invalid("$.configuration.ThumbList", "public shares require preview images")
		}
	}
	return nil
}
func bbValidateConfigInput(config map[string]any) error {
	if err := bbValidateConfigPatch(config); err != nil {
		return err
	}
	if strings.TrimSpace(String(config, "Title")) == "" {
		return invalid("$.configuration.Title", "title is required")
	}
	if _, ok := config["Open"].(bool); !ok {
		return invalid("$.configuration.Open", "explicit boolean is required")
	}
	thumbs, ok := config["ThumbList"].([]any)
	if !ok || (config["Open"] == true && len(thumbs) == 0) {
		return invalid("$.configuration.ThumbList", "public shares require preview images")
	}
	if _, ok := config["SubscribeLevel"]; !ok {
		return invalid("$.configuration.SubscribeLevel", "subscription level is required")
	}
	known, _ := config["KnownAddons"].([]any)
	count := len(known)
	for _, key := range []string{"UnknownAddons", "Materials", "Fonts"} {
		container, _ := config[key].(map[string]any)
		files, _ := container["Files"].([]any)
		count += len(files)
	}
	accounts, _ := config["WTFList"].([]any)
	for _, value := range accounts {
		account, _ := value.(map[string]any)
		characters, _ := account["RealmCharacters"].([]any)
		count += len(characters)
	}
	if count == 0 {
		return invalid("$.configuration", "select at least one backup content item")
	}
	return nil
}

func bbSetConfig(config map[string]any, key string, value any) {
	if key == "UnknownAddons" || key == "Materials" || key == "Fonts" {
		if existing, ok := config[key].(map[string]any); ok {
			if patch, ok := value.(map[string]any); ok {
				merged := bbClone(existing)
				for k, v := range patch {
					merged[k] = v
				}
				config[key] = merged
				return
			}
		}
	}
	config[key] = value
}
func bbValidateSelection(config, backup map[string]any) error {
	known, e := bbRows(backup["KnownAddons"], "ID")
	if e != nil {
		return e
	}
	selected, e := bbRows(config["KnownAddons"], "ID")
	if e != nil {
		return e
	}
	for _, r := range selected {
		_, e := bbFind(known, "ID", bbID(r["ID"]))
		if e != nil {
			return invalid("$.configuration.KnownAddons", "select original backup addon IDs")
		}
	}
	for _, key := range []string{"UnknownAddons", "Materials", "Fonts"} {
		container, ok := config[key].(map[string]any)
		if !ok {
			continue
		}
		files, e := bbRows(container["Files"], "Name")
		if e != nil {
			return e
		}
		original, ok := backup[key].(map[string]any)
		if !ok && len(files) > 0 {
			return invalid("$.configuration."+key, "selected container is absent from backup")
		}
		available := []map[string]any{}
		if original != nil {
			available, e = bbRows(original["Files"], "Name")
			if e != nil {
				return e
			}
		}
		for _, file := range files {
			_, e := bbFind(available, "Name", String(file, "Name"))
			if e != nil {
				return invalid("$.configuration."+key, "select original backup file names")
			}
		}
	}
	accounts, e := bbRows(backup["WTFList"], "Name")
	if e != nil {
		return e
	}
	chosen, e := bbRows(config["WTFList"], "Name")
	if e != nil {
		return e
	}
	for _, selection := range chosen {
		account, e := bbFind(accounts, "Name", String(selection, "Name"))
		if e != nil {
			return invalid("$.configuration.WTFList", "select original backup account names")
		}
		chars, ok := selection["RealmCharacters"].([]any)
		if !ok || len(chars) == 0 {
			return invalid("$.configuration.WTFList", "each account requires explicit characters")
		}
		available := map[string]bool{}
		if rows, ok := account["RealmCharacters"].([]any); ok {
			for _, v := range rows {
				r, ok := v.(map[string]any)
				if ok {
					available[String(r, "Realm")+"\x00"+String(r, "Player")] = true
				}
			}
		}
		if rows, ok := account["Characters"].([]any); ok {
			for _, v := range rows {
				if s, ok := v.(string); ok {
					parts := strings.SplitN(s, " - ", 2)
					if len(parts) == 2 {
						available[parts[0]+"\x00"+parts[1]] = true
					}
				}
			}
		}
		for _, v := range chars {
			r, ok := v.(map[string]any)
			if !ok || !available[String(r, "Realm")+"\x00"+String(r, "Player")] {
				return invalid("$.configuration.WTFList", "select original backup characters")
			}
		}
	}
	return nil
}
func (c *bbClient) prepareConfig(ctx context.Context, op *bbOperation) error {
	in := op.Input
	var current map[string]any
	var e error
	op.Target = String(in, "share_id")
	if op.Target != "" {
		current, e = c.share(ctx, op.Target)
		if e != nil {
			return e
		}
		branch, e := bbBranch(current)
		if e != nil {
			return e
		}
		rows, e := c.shares(ctx, branch)
		if e != nil {
			return e
		}
		if _, e = bbFind(rows, "ShareID", op.Target); e != nil {
			return e
		}
		op.Branch = branch
	}
	if op.Action == "config delete" {
		if in["confirm_delete"] != true {
			return invalid("$.confirm_delete", "must be true")
		}
		op.Steps = []bbStep{bbStepFor("/wow/update_config_share/", map[string]any{"share_id": op.Target, "backup_id": current["BackupID"], "delete": 1}, nil)}
		return nil
	}
	backupID := String(in, "backup_id")
	if backupID == "" && current != nil {
		backupID = bbID(current["BackupID"])
	}
	backup, e := c.backup(ctx, backupID)
	if e != nil {
		return e
	}
	op.Branch, e = bbBranch(backup)
	if e != nil {
		return e
	}
	patch, _ := in["configuration"].(map[string]any)
	config := bbClone(backup)
	changed := current != nil && bbID(current["BackupID"]) != backupID
	if changed {
		for _, k := range bbSelections {
			if _, ok := patch[k]; !ok {
				return invalid("$.configuration."+k, "changing backup requires all explicit selections")
			}
		}
	}
	if current != nil {
		for _, key := range []string{"Title", "Article", "Open", "KnownAddons", "WTFList", "ThumbList", "SubscribeLevel"} {
			if _, ok := current[key]; !ok {
				return failure("publish.blackbox_shape", "share detail lacks original editor fields")
			}
		}
		for _, k := range bbConfigFields {
			if k == "NeedsPush" || (changed && bbContains(bbSelections, k)) {
				continue
			}
			if v, ok := current[k]; ok {
				bbSetConfig(config, k, v)
			} else if k == "UnknownAddons" || k == "Materials" || k == "Fonts" {
				delete(config, k)
			}
		}
		if level, ok := config["SubscribeLevel"].(map[string]any); ok {
			config["SubscribeLevel"] = level["level"]
		}
	} else {
		config["UpdateLog"] = "<p><br></p>"
		for _, k := range []string{"ClearKnownAddons", "ClearUnknownAddons", "ClearMaterials", "ClearFonts"} {
			config[k] = false
		}
	}
	for k, v := range patch {
		bbSetConfig(config, k, v)
	}
	if e = bbValidateConfigInput(config); e != nil {
		return e
	}
	if e = bbValidateSelection(config, backup); e != nil {
		return e
	}
	for _, key := range []string{"UnknownAddons", "Materials", "Fonts"} {
		if container, ok := config[key].(map[string]any); ok {
			files, _ := container["Files"].([]any)
			if len(files) == 0 {
				delete(config, key)
			}
		}
	}
	rows, e := c.shares(ctx, op.Branch)
	if e != nil {
		return e
	}
	op.Previous = bbIDs(rows, "ShareID")
	op.Expected = config
	route := "/wow/create_config_share/"
	query := map[string]any{"backup_id": backupID}
	if current != nil {
		route = "/wow/update_config_share/"
		query["share_id"] = op.Target
	}
	op.Steps = []bbStep{bbStepFor(route, query, map[string]any{"configuration": bbJSON(config)})}
	return nil
}
