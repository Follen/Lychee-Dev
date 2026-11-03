package publish

import (
	"context"
	"net/url"
	"regexp"
	"strings"
)

var bbBranchPattern = regexp.MustCompile(`^wow[a-z0-9_]+$`)
var bbCanonicalPattern = regexp.MustCompile(`^/wow/wow_module_file/(wow[a-z0-9_]+)/[a-f0-9]{32}\.zip$`)
var bbIntakePattern = regexp.MustCompile(`(?i)^/wow/module/[a-z0-9_%~-][a-z0-9_.%~-]*\.zip$`)

func bbArchiveURL(row map[string]any) string {
	for _, k := range []string{"fileUrlHeybox", "fileUrl", "file_url"} {
		if s := String(row, k); s != "" {
			return s
		}
	}
	return ""
}
func bbOfficialURL(s string) *url.URL {
	u, e := url.Parse(s)
	if e != nil || u.Scheme != "https" || u.Host != "wow.max-c.com" || u.RawQuery != "" || u.Fragment != "" || u.User != nil {
		return nil
	}
	return u
}
func bbCanonicalBranch(s string) string {
	u := bbOfficialURL(s)
	if u == nil {
		return ""
	}
	m := bbCanonicalPattern.FindStringSubmatch(u.Path)
	if len(m) == 2 {
		return m[1]
	}
	return ""
}
func bbGameBranch(options any, games any) string {
	rows, ok := options.([]any)
	if !ok {
		return ""
	}
	wanted, _ := bbComparable("gameVersions", games).([]string)
	if len(wanted) == 0 {
		return ""
	}
	seen := map[string]bool{}
	matches := []string{}
	for _, v := range rows {
		row, ok := v.(map[string]any)
		if !ok {
			return ""
		}
		branch := String(row, "strappid")
		versions, ok := row["gameVersions"].([]any)
		if !ok || !bbBranchPattern.MatchString(branch) || seen[branch] {
			return ""
		}
		seen[branch] = true
		available := map[string]bool{}
		for _, v := range versions {
			s, ok := v.(string)
			if !ok || s == "" {
				return ""
			}
			available[s] = true
		}
		match := true
		for _, s := range wanted {
			match = match && available[s]
		}
		if match {
			matches = append(matches, branch)
		}
	}
	if len(matches) == 1 {
		return matches[0]
	}
	return ""
}
func bbContains(a []string, s string) bool {
	for _, v := range a {
		if v == s {
			return true
		}
	}
	return false
}
func bbIDs(rows []map[string]any, key string) []string {
	out := []string{}
	for _, r := range rows {
		out = append(out, bbID(r[key]))
	}
	return out
}
func bbStepFor(route string, query, body map[string]any) bbStep {
	if query == nil {
		query = map[string]any{}
	}
	if body == nil {
		body = map[string]any{}
	}
	return bbStep{Route: route, Query: query, Body: body, State: "prepared"}
}
func bbValidatePluginType(value any) error {
	if t := bbID(value); t != "1" && t != "9" {
		return invalid("$.type", "official plugin type must be 1 or 9")
	}
	return nil
}
func (c *bbClient) prepare(ctx context.Context, op *bbOperation) error {
	in := op.Input
	switch op.Action {
	case "plugin edit":
		op.Target = bbID(in["id"])
		current, e := c.module(ctx, op.Target)
		if e != nil {
			return e
		}
		body := map[string]any{}
		defaults := map[string]any{"name": "", "logoUrl": "", "id": in["id"], "categoryIds": []any{}, "type": 1, "desc": "", "official": "", "officialUrl": "", "coreFolders": ""}
		aliases := map[string]string{"logo_url": "logoUrl", "category_ids": "categoryIds", "official_url": "officialUrl", "core_folders": "coreFolders"}
		for k, v := range defaults {
			body[k] = v
			if existing, ok := current[k]; ok {
				body[k] = existing
			}
		}
		for k, v := range in {
			if alias, ok := aliases[k]; ok {
				k = alias
			}
			body[k] = v
			op.Expected[k] = v
		}
		if e := bbValidatePluginType(body["type"]); e != nil {
			return e
		}
		if a, ok := body["coreFolders"].([]any); ok {
			s := []string{}
			for _, v := range a {
				s = append(s, bbID(v))
			}
			body["coreFolders"] = strings.Join(s, ",")
		}
		op.Steps = []bbStep{bbStepFor("/wow/open_platform/module/update/", nil, body)}
		return nil
	case "plugin update", "version edit", "version delete":
		rows, options, e := c.versions(ctx, bbID(in["module_id"]))
		if e != nil {
			return e
		}
		op.Previous = bbIDs(rows, "id")
		op.Target = bbID(in["version_id"])
		var current map[string]any
		if op.Target != "" {
			current, e = bbFind(rows, "id", op.Target)
			if e != nil {
				return e
			}
			op.OriginalArchive = bbArchiveURL(current)
			op.OriginalBranch = bbGameBranch(options, current["gameVersions"])
		}
		if op.Action == "version delete" {
			op.Steps = []bbStep{bbStepFor("/wow/open_platform/module_version/delete/", nil, map[string]any{"moduleId": in["module_id"], "versionId": in["version_id"]})}
			return nil
		}
		games := bbComparable("gameVersions", in["game_versions"]).([]string)
		op.Branch = bbGameBranch(options, in["game_versions"])
		if op.Branch == "" {
			return invalid("$.game_versions", "select game versions from one current official branch")
		}
		op.Expected = map[string]any{"name": in["name"], "type": in["type"], "gameVersions": in["game_versions"]}
		body := map[string]any{"moduleId": in["module_id"], "name": in["name"], "type": in["type"], "gameVersions": strings.Join(games, ","), "fileUrl": op.OriginalArchive}
		if op.Target != "" {
			body["versionId"] = in["version_id"]
		}
		if file := String(in, "file"); file != "" {
			if current != nil && (op.OriginalBranch == "" || bbCanonicalBranch(op.OriginalArchive) != op.OriginalBranch) {
				return failure("publish.blackbox_archive", "replacement requires original official canonical archive and branch")
			}
			op.Archive, e = bbSnapshot(ctx, c.home, op.ID, file)
			if e != nil {
				return e
			}
		} else if op.OriginalArchive == "" {
			return failure("publish.blackbox_archive", "original version omitted archive URL")
		}
		op.Steps = []bbStep{bbStepFor("/wow/open_platform/module_version/upsert/", nil, body)}
		return nil
	case "config create", "config edit", "config delete":
		return c.prepareConfig(ctx, op)
	case "wa create", "wa edit", "wa delete":
		return c.prepareWA(ctx, op)
	default:
		if strings.HasPrefix(op.Action, "backup ") {
			return c.prepareBackup(ctx, op)
		}
	}
	return invalid("$action", "unsupported Blackbox write")
}
func bbArchiveMode(op *bbOperation, actual map[string]any, options any) string {
	source := String(op.Steps[0].Body, "fileUrl")
	target := bbArchiveURL(actual)
	if source == "" || target == "" {
		return ""
	}
	module := bbID(op.Input["module_id"])
	if actual["moduleId"] != nil && bbID(actual["moduleId"]) != module {
		return ""
	}
	branch := bbGameBranch(options, op.Expected["gameVersions"])
	id := bbID(actual["id"])
	if op.Action == "plugin update" && bbContains(op.Previous, id) {
		return ""
	}
	if op.Archive != nil && op.Action == "version edit" {
		if id != bbID(op.Input["version_id"]) || target == op.OriginalArchive || branch == "" || branch != op.OriginalBranch {
			return ""
		}
		u := bbOfficialURL(source)
		if u == nil || !bbIntakePattern.MatchString(u.Path) {
			return ""
		}
		if source == target {
			return "url_exact"
		}
		if bbCanonicalBranch(target) == branch {
			return "server_replacement_canonicalization"
		}
		return ""
	}
	if source == target {
		return "url_exact"
	}
	if branch == "" || bbCanonicalBranch(target) != branch {
		return ""
	}
	u := bbOfficialURL(source)
	if u == nil {
		return ""
	}
	if op.Archive != nil && op.Action == "plugin update" && bbIntakePattern.MatchString(u.Path) {
		return "server_canonicalization"
	}
	if op.Action == "version edit" && op.Archive == nil && id == bbID(op.Input["version_id"]) && source == op.OriginalArchive && branch == op.OriginalBranch && bbCanonicalBranch(source) == branch {
		return "server_recanonicalization"
	}
	return ""
}
