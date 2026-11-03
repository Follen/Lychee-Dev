package publish

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"strconv"
	"strings"
)

func ddMap(v any) map[string]any {
	o, _ := v.(map[string]any)
	if o == nil {
		return map[string]any{}
	}
	return o
}
func ddSlice(v any) []any { a, _ := v.([]any); return a }
func ddGroupKey(v any, key string) string {
	if o, ok := v.(map[string]any); ok {
		if key != "" {
			return ddText(o[key])
		}
		for _, k := range []string{"name", "id"} {
			if s := ddText(o[k]); s != "" {
				return s
			}
		}
	}
	return ddText(v)
}
func ddSelectedGroup(backup, current map[string]any, group, key string, selected, updates []any) (map[string]any, error) {
	available := ddSlice(ddMap(backup[group])["items"])
	index := map[string]any{}
	versions := map[string]any{}
	old := ddMap(ddMap(current[group])["inner_version"])
	chosen := map[string]bool{}
	updated := map[string]bool{}
	for _, v := range selected {
		chosen[ddText(v)] = true
	}
	for _, v := range updates {
		k := ddText(v)
		if !chosen[k] {
			return nil, invalid("$."+group, "update markers must refer to selected content")
		}
		updated[k] = true
	}
	for _, item := range available {
		k := ddGroupKey(item, key)
		if k == "" || index[k] != nil {
			return nil, failure("publish.dd_config", "backup selection identity is missing or ambiguous")
		}
		index[k] = item
		n := ddInteger(old[k])
		if n == 0 {
			n = 1
		} else if updated[k] {
			n++
		}
		versions[k] = n
	}
	items := []any{}
	seen := map[string]bool{}
	for _, s := range selected {
		k := ddText(s)
		item, ok := index[k]
		if !ok {
			return nil, invalid("$."+group, "selection is absent from chosen backup")
		}
		if seen[k] {
			return nil, invalid("$."+group, "duplicate selected content")
		}
		seen[k] = true
		items = append(items, item)
	}
	return map[string]any{"items": items, "inner_version": versions}, nil
}

type ddRole struct {
	account, server, id, selector string
	index                         int
	value                         any
}

func ddRoles(backup map[string]any) []ddRole {
	var out []ddRole
	accounts := ddSlice(ddMap(backup["wtf"])["accounts"])
	if len(accounts) == 0 {
		accounts = ddSlice(backup["wtf_accounts"])
	}
	sn := ddText(backup["sn"])
	if sn == "" {
		sn = ddText(backup["backup_sn"])
	}
	for _, a := range accounts {
		account := ddMap(a)
		an := ddText(account["name"])
		for _, s := range ddSlice(account["servers"]) {
			server := ddMap(s)
			name := ddText(server["name"])
			for i, role := range ddSlice(server["items"]) {
				id := ddText(role)
				if o, ok := role.(map[string]any); ok {
					for _, k := range []string{"role_id", "id", "name"} {
						if id = ddText(o[k]); id != "" {
							break
						}
					}
				}
				hash := sha256.Sum256([]byte(strings.Join([]string{sn, an, name, strconv.Itoa(i), id}, "\x00")))
				out = append(out, ddRole{an, name, id, "wtf_" + hex.EncodeToString(hash[:])[:20], i, role})
			}
		}
	}
	return out
}
func ddWTFTree(backup map[string]any, selected []any) (map[string]any, error) {
	roles := ddRoles(backup)
	want := map[string]bool{}
	for _, v := range selected {
		s := ddText(v)
		var matches []ddRole
		for _, r := range roles {
			if r.selector == s {
				matches = []ddRole{r}
				break
			}
			if r.id == s {
				matches = append(matches, r)
			}
		}
		if len(matches) != 1 {
			return nil, invalid("$.wtf_role_ids", "role is absent or ambiguous; use its backup selector")
		}
		want[matches[0].selector] = true
	}
	accounts := ddSlice(ddMap(backup["wtf"])["accounts"])
	if len(accounts) == 0 {
		accounts = ddSlice(backup["wtf_accounts"])
	}
	out := []any{}
	for _, a := range accounts {
		account := ddClone(ddMap(a))
		servers := []any{}
		for _, s := range ddSlice(account["servers"]) {
			server := ddClone(ddMap(s))
			items := []any{}
			for i, item := range ddSlice(server["items"]) {
				for _, r := range roles {
					if r.account == ddText(account["name"]) && r.server == ddText(server["name"]) && r.index == i && want[r.selector] {
						items = append(items, item)
						break
					}
				}
			}
			if len(items) > 0 {
				server["items"] = items
				servers = append(servers, server)
			}
		}
		if len(servers) > 0 {
			account["servers"] = servers
			out = append(out, account)
		}
	}
	return map[string]any{"accounts": out}, nil
}
func ddCurrentRoles(backup, current map[string]any) ([]any, error) {
	available := ddRoles(backup)
	selected := []any{}
	for _, r := range ddRoles(current) {
		matches := []ddRole{}
		for _, a := range available {
			if a.account == r.account && a.server == r.server && a.id == r.id {
				matches = append(matches, a)
			}
		}
		if len(matches) != 1 {
			return nil, invalid("$.wtf_role_ids", "current role is absent or ambiguous in selected backup")
		}
		selected = append(selected, matches[0].selector)
	}
	return selected, nil
}
func ddSelectedAccount(f map[string]any) string {
	a := ddSlice(ddMap(f["wtf"])["accounts"])
	if len(a) == 1 {
		return ddText(ddMap(a[0])["name"])
	}
	return ""
}
func ddConfigForm(ctx context.Context, api ddAPI, current, form, in map[string]any) (map[string]any, int64, error) {
	sn := ddText(form["backup_sn"])
	if sn == "" {
		return nil, 0, invalid("$.backup_sn", "configuration requires a selected backup")
	}
	backup, err := ddDetail(ctx, api, "backup", sn)
	if err != nil {
		return nil, 0, err
	}
	if err = ddRequireOwner(ctx, api, "backup", sn, backup); err != nil {
		return nil, 0, err
	}
	changed := sn != ddText(current["backup_sn"])
	if changed && len(current) > 0 {
		for _, k := range []string{"known_addon_ids", "unknown_addon_ids", "wtf_role_ids", "material_names", "font_names", "known_wa_ids", "unknown_wa_ids"} {
			if _, ok := in[k]; !ok {
				return nil, 0, invalid("$."+k, "changing backup requires complete reselection")
			}
		}
	}
	for _, g := range [][4]string{{"known_addon", "addon_id", "known_addon_ids", "known_addon_update_ids"}, {"unknown_addon", "", "unknown_addon_ids", "unknown_addon_update_ids"}, {"material", "name", "material_names", "material_update_names"}, {"font", "", "font_names", "font_update_names"}} {
		selected, provided := in[g[2]]
		if !provided {
			a := []any{}
			for _, item := range ddSlice(ddMap(current[g[0]])["items"]) {
				a = append(a, ddGroupKey(item, g[1]))
			}
			selected = a
		}
		group, e := ddSelectedGroup(backup, current, g[0], g[1], ddSlice(selected), ddSlice(in[g[3]]))
		if e != nil {
			return nil, 0, e
		}
		form[g[0]] = group
	}
	roles, provided := in["wtf_role_ids"]
	if !provided {
		roles, err = ddCurrentRoles(backup, current)
		if err != nil {
			return nil, 0, err
		}
	}
	form["wtf"], err = ddWTFTree(backup, ddSlice(roles))
	if err != nil {
		return nil, 0, err
	}
	account := ddSelectedAccount(form)
	accountChanged := provided && account != ddSelectedAccount(current)
	for _, g := range []string{"known_wa", "unknown_wa"} {
		selected, provided := in[g+"_ids"]
		if !provided {
			a := []any{}
			if !accountChanged {
				for _, item := range ddSlice(ddMap(current[g])["items"]) {
					a = append(a, ddText(ddMap(item)["uid"]))
				}
			}
			selected = a
		}
		choices := ddSlice(selected)
		if len(choices) > 0 && account == "" {
			return nil, 0, invalid("$.wtf_role_ids", "WA content requires exactly one selected WTF account")
		}
		group, e := ddSelectedGroup(backup, current, g, "uid", choices, ddSlice(in[g+"_update_ids"]))
		if e != nil {
			return nil, 0, e
		}
		mapping := map[string]any{}
		for _, v := range ddSlice(ddMap(ddMap(backup["extra"])["wa_account_info"])[account]) {
			o := ddMap(v)
			mapping[ddText(o["uid"])] = o["id"]
		}
		items := []any{}
		for _, item := range ddSlice(group["items"]) {
			o := ddClone(ddMap(item))
			id, ok := mapping[ddText(o["uid"])]
			if !ok {
				return nil, 0, invalid("$."+g+"_ids", "WA is unavailable for selected account")
			}
			if g == "unknown_wa" {
				o["id"] = id
			}
			items = append(items, o)
		}
		group["items"] = items
		form[g] = group
	}
	if v, ok := current["retail_ui_config"]; ok {
		form["retail_ui_config"] = v
	}
	if ddGame(backup) == 10001 && changed {
		if _, ok := in["retail_ui_config"]; !ok {
			return nil, 0, invalid("$.retail_ui_config", "retail backup requires explicit layout selection or null")
		}
	}
	if selection, ok := in["retail_ui_config"]; ok {
		existing := current
		if changed {
			existing = map[string]any{}
		}
		form["retail_ui_config"], err = ddRetailForm(backup, existing, selection)
		if err != nil {
			return nil, 0, err
		}
	} else if ddGame(backup) != 10001 {
		delete(form, "retail_ui_config")
	}
	hasAddons := len(ddSlice(ddMap(form["known_addon"])["items"])) > 0 || len(ddSlice(ddMap(form["unknown_addon"])["items"])) > 0
	hasRoles := len(ddSlice(ddMap(form["wtf"])["accounts"])) > 0
	if hasAddons && !hasRoles {
		return nil, 0, invalid("$.wtf_role_ids", "addon content requires selected WTF roles")
	}
	if !hasAddons && !hasRoles && len(ddSlice(ddMap(form["material"])["items"])) == 0 && len(ddSlice(ddMap(form["font"])["items"])) == 0 {
		return nil, 0, invalid("$.known_addon_ids", "configuration cannot contain only WA selections")
	}
	return form, ddGame(backup), nil
}
