package publish

import (
	"crypto/sha256"
	"encoding/hex"
	"strconv"
	"strings"
)

type ddRetailEntry struct {
	selector, account string
	item              map[string]any
}

func ddRetailCatalog(backup map[string]any) map[string][]ddRetailEntry {
	out := map[string][]ddRetailEntry{}
	sn := ddText(backup["sn"])
	if sn == "" {
		sn = ddText(backup["backup_sn"])
	}
	retail := ddMap(backup["retail_ui_config"])
	for _, s := range [][3]string{{"editMode", "edit_mode", "em_"}, {"coolDown", "cool_down", "cd_"}} {
		for account, entries := range ddMap(retail[s[0]]) {
			for i, v := range ddSlice(entries) {
				item, ok := v.(map[string]any)
				if !ok {
					continue
				}
				hash := sha256.Sum256([]byte(strings.Join([]string{sn, s[0], account, strconv.Itoa(i)}, "\x00")))
				out[s[1]] = append(out[s[1]], ddRetailEntry{s[2] + hex.EncodeToString(hash[:])[:20], account, item})
			}
		}
	}
	return out
}
func ddRetailForm(backup, current map[string]any, selection any) (any, error) {
	if selection == nil {
		return nil, nil
	}
	if ddGame(backup) != 10001 {
		return nil, invalid("$.retail_ui_config", "retail layouts require a retail backup")
	}
	s := ddMap(selection)
	wire := ddClone(ddMap(current["retail_ui_config"]))
	catalog := ddRetailCatalog(backup)
	for _, spec := range [][2]string{{"edit_mode", "edit_mode_selectors"}, {"cool_down", "cool_down_selectors"}} {
		values, ok := s[spec[1]]
		if !ok {
			continue
		}
		chosen := ddSlice(values)
		grouped := map[string]any{}
		seen := map[string]bool{}
		specs := map[string]bool{}
		def := ddText(s["default_edit_mode_selector"])
		defaultFound := false
		for _, v := range chosen {
			selector := ddText(v)
			if seen[selector] {
				return nil, invalid("$.retail_ui_config", "duplicate layout selector")
			}
			seen[selector] = true
			var found *ddRetailEntry
			for _, e := range catalog[spec[0]] {
				if e.selector == selector {
					copy := e
					found = &copy
					break
				}
			}
			if found == nil {
				return nil, invalid("$.retail_ui_config", "layout is absent from selected backup")
			}
			item := ddClone(found.item)
			if spec[0] == "edit_mode" {
				item["is_default"] = selector == def
				defaultFound = defaultFound || selector == def
			} else {
				tag := ddText(item["spec_tag"])
				if specs[tag] {
					return nil, invalid("$.retail_ui_config", "select only one cooldown layout per specialization")
				}
				specs[tag] = true
			}
			grouped[found.account] = append(ddSlice(grouped[found.account]), item)
		}
		if spec[0] == "edit_mode" && (len(chosen) > 5 || len(chosen) > 0 && !defaultFound || len(chosen) == 0 && def != "") {
			return nil, invalid("$.retail_ui_config", "select up to five edit layouts and one selected default")
		}
		wire[spec[0]] = grouped
	}
	if _, ok := s["default_edit_mode_selector"]; ok {
		if _, ok = s["edit_mode_selectors"]; !ok {
			return nil, invalid("$.retail_ui_config", "default requires edit layout selection")
		}
	}
	if v, ok := s["enable_dd_setup_wizard"]; ok {
		wire["enable_dd_setup_wizard"] = v
	}
	return wire, nil
}
func ddBackupPublic(v map[string]any) map[string]any {
	out := ddMap(ddPublic(v))
	roles := []any{}
	for _, r := range ddRoles(v) {
		roles = append(roles, map[string]any{"selector": r.selector, "account": r.account, "server": r.server, "role_id": r.id})
	}
	out["wtf_roles"] = roles
	layouts := map[string]any{}
	for kind, entries := range ddRetailCatalog(v) {
		rows := []any{}
		for _, e := range entries {
			rows = append(rows, map[string]any{"selector": e.selector, "account": e.account, "name": e.item["name"], "spec_tag": e.item["spec_tag"]})
		}
		layouts[kind] = rows
	}
	out["retail_layouts"] = layouts
	delete(out, "retail_ui_config")
	return out
}
