package publish

import (
	"context"
	"net/url"
)

func bbTextWidth(text string) int {
	weight := 0
	for _, r := range text {
		switch {
		case r <= 255:
			weight++
		case r <= 65535:
			weight += 2
		default:
			weight += 4
		}
	}
	return weight
}

func bbValidateWA(config map[string]any) error {
	spec := bbWAField(true)
	if e := spec.validate(config, "$.configuration"); e != nil {
		return e
	}
	return bbValidateWAPatch(config)
}
func bbValidateWAPatch(config map[string]any) error {
	if branch, ok := config["strappid"]; ok && !bbBranchPattern.MatchString(branch.(string)) {
		return invalid("$.configuration.strappid", "select an official WoW branch")
	}
	if bbTextWidth(String(config, "name")) > 70 {
		return invalid("$.configuration.name", "name exceeds official weighted 70-character limit")
	}
	seen := map[string]bool{}
	categories, _ := config["categories"].([]any)
	for _, v := range categories {
		s := v.(string)
		if seen[s] {
			return invalid("$.configuration.categories", "category keys must be distinct")
		}
		seen[s] = true
	}
	screens, _ := config["screens"].([]any)
	for _, v := range screens {
		r := v.(map[string]any)
		for _, k := range []string{"src", "thumb"} {
			u, e := url.Parse(String(r, k))
			if e != nil || u.Scheme != "https" || u.Hostname() == "" || u.User != nil || (u.Port() != "" && u.Port() != "443") {
				return invalid("$.configuration.screens", "preview references must be HTTPS URLs")
			}
		}
	}
	return nil
}
func (c *bbClient) prepareWA(ctx context.Context, op *bbOperation) error {
	in := op.Input
	patch, _ := in["configuration"].(map[string]any)
	branch := String(in, "strappid")
	if patch != nil {
		branch = String(patch, "strappid")
	}
	op.Branch = branch
	rows, e := c.was(ctx, branch)
	if e != nil {
		return e
	}
	op.Previous = bbIDs(rows, "Slug")
	op.Target = String(in, "slug")
	var current map[string]any
	if op.Target != "" {
		summary, e := bbFind(rows, "Slug", op.Target)
		if e != nil {
			return e
		}
		current, e = c.waDetail(ctx, summary)
		if e != nil {
			return e
		}
	}
	if op.Action == "wa delete" {
		if in["confirm_delete"] != true {
			return invalid("$.confirm_delete", "must be true")
		}
		if bbID(current["AuditState"]) == "1" {
			return failure("publish.blackbox_review", "official client disables deletion during active review")
		}
		op.Steps = []bbStep{bbStepFor("/wow/update_wa/", map[string]any{"slug": op.Target, "delete": 1}, nil)}
		return nil
	}
	config := map[string]any{}
	if current != nil {
		for wire, key := range map[string]string{"name": "Name", "descriptionHeybox": "Description", "updateLog": "UpdateLog", "categories": "Categories", "encoded": "Encoded"} {
			v, ok := current[key]
			if !ok {
				return failure("publish.blackbox_shape", "WA detail lacks original editor content")
			}
			config[wire] = v
		}
		thumbs, ok := current["ThumbList"].([]any)
		if !ok {
			return failure("publish.blackbox_shape", "WA detail lacks original images")
		}
		screens := []any{}
		for _, v := range thumbs {
			screens = append(screens, map[string]any{"src": v, "thumb": v})
		}
		config["screens"] = screens
	}
	for k, v := range patch {
		config[k] = v
	}
	if e = bbValidateWA(config); e != nil {
		return e
	}
	categories, e := c.call(ctx, "POST", "/wow/get_wa_tag_list/", nil, nil)
	if e != nil {
		return e
	}
	groups, e := bbRows(categories["list"], "")
	if e != nil {
		return e
	}
	tags := map[string]bool{}
	for _, group := range groups {
		rows, e := bbRows(group["TagList"], "Key")
		if e != nil {
			return e
		}
		for _, row := range rows {
			tags[String(row, "Key")] = true
		}
	}
	for _, tag := range config["categories"].([]any) {
		if !tags[tag.(string)] {
			return invalid("$.configuration.categories", "select current official WA category keys")
		}
	}
	op.Expected = map[string]any{"Name": config["name"], "Categories": config["categories"], "Encoded": config["encoded"]}
	thumbs := []any{}
	for _, v := range config["screens"].([]any) {
		thumbs = append(thumbs, v.(map[string]any)["src"])
	}
	op.Expected["ThumbList"] = thumbs
	for k, v := range map[string]string{"descriptionHeybox": "Description", "updateLog": "UpdateLog"} {
		if value, ok := config[k]; ok {
			op.Expected[v] = value
		}
	}
	route := "/wow/create_wa/"
	query := map[string]any{}
	if current != nil {
		route = "/wow/update_wa/"
		query["slug"] = op.Target
	}
	op.Steps = []bbStep{bbStepFor(route, query, map[string]any{"configuration": bbJSON(config)})}
	return nil
}
