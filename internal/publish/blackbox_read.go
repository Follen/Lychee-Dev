package publish

import (
	"context"
	"encoding/json"
	"reflect"
	"sort"
	"strings"
)

func bbRows(v any, key string) ([]map[string]any, error) {
	a, ok := v.([]any)
	if !ok {
		return nil, failure("publish.blackbox_shape", "incomplete list: "+key)
	}
	rows := make([]map[string]any, 0, len(a))
	seen := map[string]bool{}
	for _, v := range a {
		r, err := bbObject(v)
		if err != nil {
			return nil, err
		}
		if key != "" {
			id := bbID(r[key])
			if id == "" || seen[id] {
				return nil, failure("publish.blackbox_shape", "invalid or duplicate list identity")
			}
			seen[id] = true
		}
		rows = append(rows, r)
	}
	return rows, nil
}
func bbFind(rows []map[string]any, key, id string) (map[string]any, error) {
	for _, r := range rows {
		if bbID(r[key]) == id {
			return r, nil
		}
	}
	return nil, failure("publish.blackbox_not_found", "original target is absent from the owned list")
}
func bbClone(v map[string]any) map[string]any {
	raw, _ := json.Marshal(v)
	d := json.NewDecoder(strings.NewReader(string(raw)))
	d.UseNumber()
	var out map[string]any
	_ = d.Decode(&out)
	return out
}
func bbJSON(v any) string { raw, _ := json.Marshal(v); return string(raw) }
func bbComparable(key string, v any) any {
	switch key {
	case "categoryIds", "gameVersions", "coreFolders":
		var a []any
		switch x := v.(type) {
		case string:
			for _, s := range strings.Split(x, ",") {
				if s != "" {
					a = append(a, s)
				}
			}
		case []any:
			a = x
		case nil:
		default:
			a = []any{x}
		}
		out := []string{}
		for _, x := range a {
			if o, ok := x.(map[string]any); ok {
				x = o["id"]
				if x == nil {
					x = o["value"]
				}
			}
			if s := bbID(x); s != "" {
				out = append(out, s)
			}
		}
		sort.Strings(out)
		return out
	case "id", "type", "auditState":
		return bbID(v)
	}
	return v
}
func bbMatches(actual, expected map[string]any) bool {
	for k, v := range expected {
		if !reflect.DeepEqual(bbComparable(k, actual[k]), bbComparable(k, v)) {
			return false
		}
	}
	return true
}
func (c *bbClient) module(ctx context.Context, id string) (map[string]any, error) {
	r, e := c.call(ctx, "GET", "/wow/open_platform/module/detail/", map[string]any{"moduleId": id}, nil)
	if e != nil {
		return nil, e
	}
	if r["module"] != nil {
		r, e = bbObject(r["module"])
	}
	if e == nil && bbID(r["id"]) != id {
		return nil, failure("publish.blackbox_identity", "module detail identifies another target")
	}
	return r, e
}
func (c *bbClient) versions(ctx context.Context, id string) ([]map[string]any, any, error) {
	rows := []map[string]any{}
	seen := map[string]bool{}
	total := ""
	var options any
	for page := 0; page < 100; page++ {
		r, e := c.call(ctx, "GET", "/wow/open_platform/module_version/list/", map[string]any{"moduleId": id, "offset": len(rows), "limit": 100}, nil)
		if e != nil {
			return nil, nil, e
		}
		if v, ok := r["strappidInfos"]; ok {
			options = v
		}
		for _, k := range []string{"totalCount", "total_count", "total", "count"} {
			if v, ok := r[k]; ok {
				n, ok := v.(json.Number)
				if !ok {
					return nil, nil, failure("publish.blackbox_shape", "invalid version count")
				}
				i, e := n.Int64()
				if e != nil || i < 0 || (total != "" && total != string(n)) {
					return nil, nil, failure("publish.blackbox_shape", "version count changed")
				}
				total = string(n)
			}
		}
		chunk, e := bbRows(r["versionList"], "id")
		if e != nil {
			return nil, nil, e
		}
		for _, row := range chunk {
			key := bbID(row["id"])
			if strings.Trim(key, "0123456789") != "" || strings.Trim(key, "0") == "" || seen[key] || (row["moduleId"] != nil && bbID(row["moduleId"]) != id) {
				return nil, nil, failure("publish.blackbox_identity", "invalid version page identity")
			}
			seen[key] = true
			rows = append(rows, row)
		}
		if total != "" {
			var n json.Number = json.Number(total)
			count, _ := n.Int64()
			if int64(len(rows)) > count {
				return nil, nil, failure("publish.blackbox_shape", "version list exceeds count")
			}
			if int64(len(rows)) == count {
				return rows, options, nil
			}
		}
		if len(chunk) == 0 {
			if total != "" {
				return nil, nil, failure("publish.blackbox_shape", "version list ended early")
			}
			return rows, options, nil
		}
	}
	return nil, nil, failure("publish.blackbox_incomplete", "version pagination limit reached")
}
func (c *bbClient) backups(ctx context.Context) ([]map[string]any, error) {
	r, e := c.call(ctx, "POST", "/wow/get_user_configuration_backup_list/", nil, nil)
	if e != nil {
		return nil, e
	}
	return bbRows(r["backupList"], "ID")
}
func (c *bbClient) backup(ctx context.Context, id string) (map[string]any, error) {
	r, e := c.backups(ctx)
	if e != nil {
		return nil, e
	}
	return bbFind(r, "ID", id)
}
func bbBranch(row map[string]any) (string, error) {
	if row["IsClassic"] == false {
		return "wowretail", nil
	}
	if s := String(row, "Strappid"); s != "" {
		return s, nil
	}
	if row["IsClassic"] == true {
		return "wowclassicera", nil
	}
	return "", failure("publish.blackbox_shape", "backup branch is unknown")
}
func (c *bbClient) shares(ctx context.Context, branch string) ([]map[string]any, error) {
	rows := []map[string]any{}
	seen := map[string]bool{}
	for page := 0; page < 1000; page++ {
		r, e := c.call(ctx, "POST", "/wow/v2/share/list/", map[string]any{"offset": len(rows), "limit": 20, "strappid": branch, "sort_by": "weekDownloadNum", "my_publish": 1}, nil)
		if e != nil {
			return nil, e
		}
		a, e := bbRows(r["shares"], "ShareID")
		if e != nil {
			return nil, e
		}
		if len(a) == 0 {
			return rows, nil
		}
		for _, row := range a {
			id := bbID(row["ShareID"])
			if seen[id] {
				return nil, failure("publish.blackbox_incomplete", "share pagination repeated an identity")
			}
			seen[id] = true
			rows = append(rows, row)
		}
	}
	return nil, failure("publish.blackbox_incomplete", "share pagination limit reached")
}
func (c *bbClient) share(ctx context.Context, id string) (map[string]any, error) {
	r, e := c.call(ctx, "POST", "/wow/v2/share/detail/", map[string]any{"share_id": id}, nil)
	if e == nil && (bbID(r["ShareID"]) != id || !bbStringID(r["BackupID"])) {
		return nil, failure("publish.blackbox_identity", "share detail is incomplete or identifies another target")
	}
	return r, e
}
func (c *bbClient) waOwner(ctx context.Context) (string, error) {
	r, e := c.call(ctx, "POST", "/wow/v2/get_user_info/", nil, nil)
	if e != nil {
		return "", e
	}
	id := bbID(r["heybox_id"])
	if id == "" || c.session == nil || id != c.session.Identity {
		return "", failure("publish.blackbox_identity", "WA author differs from original session")
	}
	return id, nil
}
func (c *bbClient) was(ctx context.Context, branch string) ([]map[string]any, error) {
	owner, e := c.waOwner(ctx)
	if e != nil {
		return nil, e
	}
	rows := []map[string]any{}
	seen := map[string]bool{}
	for page := 0; page < 1000; page++ {
		r, e := c.call(ctx, "POST", "/wow/get_wa_list/", map[string]any{"strappid": branch, "offset": len(rows), "limit": 30, "my_publish": 1, "sortBy": "finalScore"}, nil)
		if e != nil {
			return nil, e
		}
		a, e := bbRows(r["waList"], "Slug")
		if e != nil {
			return nil, e
		}
		if len(a) == 0 {
			return rows, nil
		}
		for _, row := range a {
			id := bbID(row["Slug"])
			if seen[id] || bbID(row["HeyboxId"]) != owner {
				return nil, failure("publish.blackbox_identity", "WA page repeats a record or identifies another author")
			}
			seen[id] = true
			rows = append(rows, row)
		}
	}
	return nil, failure("publish.blackbox_incomplete", "WA pagination limit reached")
}
func (c *bbClient) waDetail(ctx context.Context, summary map[string]any) (map[string]any, error) {
	v, e := bbObject(summary["LatestVersion"])
	if e != nil || !bbStringID(v["Version"]) {
		return nil, failure("publish.blackbox_shape", "WA latest version is missing")
	}
	r, e := c.call(ctx, "POST", "/wow/get_wa_detail/", map[string]any{"slug": summary["Slug"], "version_id": v["Version"]}, nil)
	if e == nil && (r["Slug"] != summary["Slug"] || bbID(r["HeyboxId"]) != c.session.Identity) {
		return nil, failure("publish.blackbox_identity", "WA detail differs from original author or slug")
	}
	return r, e
}
func (c *bbClient) read(ctx context.Context, action string, in map[string]any) (any, error) {
	switch action {
	case "options game-types":
		r, e := c.call(ctx, "GET", "/wow/v2/dynamic_conf/", nil, nil)
		if e != nil {
			return nil, e
		}
		rows, e := bbRows(r["strappidInfos"], "")
		if e == nil && len(rows) == 0 {
			e = failure("publish.blackbox_shape", "empty official game options")
		}
		return map[string]any{"items": rows}, e
	case "plugin list":
		r, e := c.call(ctx, "GET", "/wow/open_platform/module/list/", nil, nil)
		if e != nil {
			return nil, e
		}
		rows, e := bbRows(r["moduleList"], "id")
		return map[string]any{"plugins": rows, "total_count": len(rows)}, e
	case "plugin get", "plugin versions":
		id := bbID(in["module_id"])
		rows, options, e := c.versions(ctx, id)
		if e != nil {
			return nil, e
		}
		out := map[string]any{"versions": rows, "strappidInfos": options}
		if action == "plugin get" {
			out["module"], e = c.module(ctx, id)
		}
		return out, e
	case "backup list":
		rows, e := c.backups(ctx)
		return map[string]any{"backups": rows, "total_count": len(rows)}, e
	case "backup get":
		return c.backup(ctx, String(in, "backup_id"))
	case "config list":
		rows, e := c.shares(ctx, String(in, "strappid"))
		return map[string]any{"configs": rows, "total_count": len(rows)}, e
	case "config get":
		return c.share(ctx, String(in, "share_id"))
	case "wa list":
		rows, e := c.was(ctx, String(in, "strappid"))
		return map[string]any{"was": rows, "total_count": len(rows)}, e
	case "wa get":
		rows, e := c.was(ctx, String(in, "strappid"))
		if e != nil {
			return nil, e
		}
		row, e := bbFind(rows, "Slug", String(in, "slug"))
		if e != nil {
			return nil, e
		}
		return c.waDetail(ctx, row)
	case "wa categories":
		r, e := c.call(ctx, "POST", "/wow/get_wa_tag_list/", nil, nil)
		if e != nil {
			return nil, e
		}
		rows, e := bbRows(r["list"], "")
		for _, row := range rows {
			if _, ok := row["TagList"].([]any); !ok {
				return nil, failure("publish.blackbox_shape", "incomplete WA category group")
			}
		}
		return map[string]any{"categories": rows}, e
	}
	return nil, invalid("$action", "unsupported Blackbox read")
}
