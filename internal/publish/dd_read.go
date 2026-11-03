package publish

import (
	"context"
	"encoding/json"
	"fmt"
	"net/url"
	"strconv"
	"strings"
)

func ddResource(r string) string {
	if r == "plugin" {
		return "addon"
	}
	if r == "config" {
		return "share"
	}
	return r
}
func ddText(v any) string {
	switch x := v.(type) {
	case string:
		return x
	case json.Number:
		return string(x)
	case int:
		return strconv.Itoa(x)
	case int64:
		return strconv.FormatInt(x, 10)
	}
	return ""
}
func ddObject(v any) (map[string]any, error) {
	o, ok := v.(map[string]any)
	if !ok {
		return nil, failure("publish.dd_data", "DD result is not an object")
	}
	return o, nil
}
func ddItems(v any) ([]any, error) {
	if a, ok := v.([]any); ok {
		return a, nil
	}
	if o, ok := v.(map[string]any); ok {
		for _, k := range []string{"data_list", "list", "rows", "items", "wa_list", "shares", "categories", "versions"} {
			if a, ok := o[k].([]any); ok {
				return a, nil
			}
		}
	}
	return nil, failure("publish.dd_data", "DD response has no recognized list; absence is unverified")
}
func ddTotal(v map[string]any) (int64, bool, error) {
	for _, node := range []any{v, v["result"]} {
		if o, ok := node.(map[string]any); ok {
			for _, k := range []string{"total", "total_count", "count"} {
				if n, ok := o[k]; ok && n != nil {
					i, err := strconv.ParseInt(ddText(n), 10, 64)
					if err != nil || i < 0 {
						return 0, false, failure("publish.dd_data", "invalid DD list total")
					}
					return i, true, nil
				}
			}
		}
	}
	return 0, false, nil
}
func ddAuthorPage(ctx context.Context, api ddAPI, resource string, game int64, keyword string, page, size int64) (map[string]any, error) {
	if resource == "plugin" {
		return api.call(ctx, "/addon/addon_list", nil, map[string]any{"game_type": game, "origin": "created", "page": page, "size": size, "category": 0, "name_or_author_name_or_share_code": keyword, "sort_type": 2}, false)
	}
	q := url.Values{"game_type": {strconv.FormatInt(game, 10)}, "origin": {"created"}, "page": {strconv.FormatInt(page, 10)}, "size": {strconv.FormatInt(size, 10)}, "search_text": {keyword}, "sort_type": {"mtime"}}
	if resource == "wa" {
		q.Set("category_id", "")
	}
	return api.call(ctx, "/"+ddResource(resource)+"/list", q, nil, false)
}
func ddAllAuthor(ctx context.Context, api ddAPI, r string, game int64) ([]any, error) {
	var all []any
	seen := map[string]bool{}
	identities := map[string]bool{}
	for p := int64(1); p <= 1000; p++ {
		v, err := ddAuthorPage(ctx, api, r, game, "", p, 100)
		if err != nil {
			return nil, err
		}
		rows, err := ddItems(v["result"])
		if err != nil {
			return nil, err
		}
		raw, _ := json.Marshal(rows)
		key := string(raw)
		if len(rows) > 0 && seen[key] {
			return nil, failure("publish.dd_pagination", "DD repeated an author page")
		}
		seen[key] = true
		for _, row := range rows {
			o := ddMap(row)
			id := ddText(o["sn"])
			if id == "" {
				id = ddText(o["share_sn"])
			}
			if id == "" || identities[id] {
				return nil, failure("publish.dd_pagination", "DD author list has missing or duplicate identities")
			}
			identities[id] = true
		}
		all = append(all, rows...)
		total, known, err := ddTotal(v)
		if err != nil {
			return nil, err
		}
		if known && int64(len(all)) > total {
			return nil, failure("publish.dd_pagination", "DD list exceeds total")
		}
		if known && int64(len(all)) == total || !known && len(rows) < 100 {
			return all, nil
		}
		if len(rows) == 0 {
			return nil, failure("publish.dd_pagination", "DD list ended before total")
		}
	}
	return nil, failure("publish.dd_pagination", "DD author list exceeds 1000 pages")
}
func ddDetail(ctx context.Context, api ddAPI, r, sn string) (map[string]any, error) {
	path := "/" + ddResource(r) + "/detail"
	if r == "plugin" {
		path += "_v2"
	}
	v, err := api.call(ctx, path, url.Values{"sn": {sn}}, nil, false)
	if err != nil {
		return nil, err
	}
	value := v["result"]
	if r == "backup" {
		if a, ok := value.([]any); ok {
			if len(a) != 1 {
				return nil, failure("publish.dd_identity", "backup detail is not exactly one resource")
			}
			value = a[0]
		}
	}
	o, err := ddObject(value)
	if err != nil {
		return nil, err
	}
	matched := false
	keys := []string{"sn", "share_sn"}
	if r == "backup" {
		keys = append(keys, "backup_sn")
	}
	for _, k := range keys {
		if id := ddText(o[k]); id != "" && id != sn {
			return nil, failure("publish.dd_identity", "DD detail differs from selected resource")
		} else if id == sn {
			matched = true
		}
	}
	if r == "backup" && !matched {
		return nil, failure("publish.dd_identity", "backup detail omitted its identity")
	}
	return o, nil
}

// Project account-bearing API records before they cross the public boundary.
func ddPublic(v any) any {
	switch x := v.(type) {
	case map[string]any:
		o := map[string]any{}
		for k, v := range x {
			key := strings.ToLower(k)
			if key == "content" || key == "html_desc" || key == "desc" || key == "description" || key == "update_desc" || key == "encoded" || key == "wa_string" {
				continue
			}
			if strings.Contains(key, "token") || strings.Contains(key, "cookie") || strings.Contains(key, "password") || strings.Contains(key, "credential") || key == "mobile" || key == "phone" || key == "email" || key == "authorization" {
				continue
			}
			o[k] = ddPublic(v)
		}
		return o
	case []any:
		a := make([]any, len(x))
		for i, v := range x {
			a[i] = ddPublic(v)
		}
		return a
	case string:
		if u, e := url.Parse(x); e == nil && u.IsAbs() {
			u.User = nil
			u.RawQuery = ""
			u.Fragment = ""
			return u.String()
		}
		return x
	default:
		return x
	}
}
func ddRead(ctx context.Context, api ddAPI, action string, in map[string]any) (any, error) {
	r, verb, _ := strings.Cut(action, " ")
	q := url.Values{}
	game := Integer(in, "game_type")
	if game > 0 {
		q.Set("game_type", strconv.FormatInt(game, 10))
	}
	sn := String(in, "sn")
	if sn != "" {
		q.Set("sn", sn)
	}
	if verb == "get" {
		v, err := ddDetail(ctx, api, r, sn)
		if err != nil {
			return nil, err
		}
		if r == "backup" {
			return ddBackupPublic(v), nil
		}
		return v, nil
	}
	if action == "options channels" {
		return api.channels(ctx)
	}
	if action == "options associated-acts" {
		var out []any
		for _, resource := range []string{"plugin", "config", "wa"} {
			rows, err := ddAllAuthor(ctx, api, resource, game)
			if err != nil {
				return nil, err
			}
			for _, row := range rows {
				o, err := ddObject(row)
				if err != nil {
					return nil, err
				}
				id := ddText(o["sn"])
				if id == "" {
					id = ddText(o["share_sn"])
				}
				if id == "" {
					return nil, failure("publish.dd_data", "author row omitted SN")
				}
				name := o["name"]
				if name == nil {
					name = o["title"]
				}
				out = append(out, map[string]any{"sn": id, "act_type": ddResource(resource), "name": name})
			}
		}
		return map[string]any{"items": out, "complete": true}, nil
	}
	page, size := Integer(in, "page"), Integer(in, "size")
	if page == 0 {
		page = 1
	}
	if size == 0 {
		size = 50
	}
	if verb == "list" && r != "backup" {
		v, err := ddAuthorPage(ctx, api, r, game, String(in, "keyword"), page, size)
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
		if int64(len(rows)) > size || known && (page-1)*size+int64(len(rows)) > total && len(rows) > 0 {
			return nil, failure("publish.dd_pagination", "DD page contradicts requested size or total")
		}
		more := !known && int64(len(rows)) == size || known && page*size < total
		var next any
		if more {
			next = page + 1
		}
		return map[string]any{"items": rows, "page": page, "size": size, "has_more": more, "next_page": next, "complete_author_list": page == 1 && !more, "truncated": page != 1 || more}, nil
	}
	paths := map[string]string{"options game-types": "/game_type/list", "options game-versions": "/game_versions/list", "plugin categories": "/addon/category", "wa categories": "/wa/categories", "options vip-levels": "/anchor_vip/level/list", "plugin versions": "/addon/addon_versions", "backup list": "/backup/list"}
	path, ok := paths[action]
	if !ok {
		return nil, invalid("$action", fmt.Sprintf("unknown DD read %s", action))
	}
	if action == "options vip-levels" {
		q.Set("enrich_acts", "false")
	}
	if action == "plugin versions" {
		q.Set("page", strconv.FormatInt(page, 10))
	}
	v, err := api.call(ctx, path, q, nil, false)
	if err != nil {
		return nil, err
	}
	if action == "plugin versions" {
		rows, err := ddItems(v["result"])
		if err != nil {
			return nil, err
		}
		total, known, err := ddTotal(v)
		if err != nil {
			return nil, err
		}
		complete := page == 1 && (known && total == int64(len(rows)) || !known && len(rows) == 0)
		return map[string]any{"items": rows, "page": page, "complete_version_list": complete, "truncated": !complete}, nil
	}
	return v["result"], nil
}
