package publish

import (
	"context"
	"encoding/json"
	"fmt"
	"strings"
)

func newBeeCollect(value any, keys ...string) map[string]bool {
	result := map[string]bool{}
	var walk func(any)
	walk = func(value any) {
		switch v := newBeeDecoded(value).(type) {
		case []any:
			for _, row := range v {
				walk(row)
			}
		case map[string]any:
			for _, key := range keys {
				if x, ok := v[key]; ok {
					switch x.(type) {
					case string, json.Number, float64, int, int64:
						result[fmt.Sprint(x)] = true
					}
				}
			}
			for _, row := range v {
				switch row.(type) {
				case []any, map[string]any:
					walk(row)
				}
			}
		}
	}
	walk(value)
	return result
}
func newBeeRequireOptions(value, selected any, keys []string, path string) error {
	available := newBeeCollect(value, keys...)
	seen := map[string]bool{}
	for _, raw := range newBeeRows(selected) {
		key := fmt.Sprint(raw)
		if !available[key] || seen[key] {
			return invalid(path, "selection contains a duplicate or unavailable option")
		}
		seen[key] = true
	}
	return nil
}
func (c *newBeeCreator) validateResource(ctx context.Context, resource string, body, input map[string]any) error {
	if resource == "plugin" {
		if _, ok := input["mod_categories"]; ok {
			value, err := c.read(ctx, "plugin categories", nil)
			if err != nil {
				return err
			}
			if err = newBeeRequireOptions(value, body["mod_categories"], []string{"t_id", "id"}, "$.mod_categories"); err != nil {
				return err
			}
		}
	}
	if resource == "wa" {
		if _, game := input["game_version_id"]; game {
			value, err := c.read(ctx, "plugin game-versions", nil)
			if err != nil {
				return err
			}
			if err = newBeeRequireOptions(value, []any{body["game_version_id"]}, []string{"id"}, "$.game_version_id"); err != nil {
				return err
			}
		}
		_, categories := input["category_id_list"]
		_, game := input["game_version_id"]
		if categories || game {
			value, err := c.read(ctx, "wa categories", map[string]any{"game_version_id": body["game_version_id"]})
			if err != nil {
				return err
			}
			if err = newBeeRequireOptions(value, body["category_id_list"], []string{"id", "t_id", "category_id", "value"}, "$.category_id_list"); err != nil {
				return err
			}
		}
		if raw, changed := input["attachments"]; changed {
			value, err := c.read(ctx, "wa attachment-paths", nil)
			if err != nil {
				return err
			}
			paths := newBeeCollect(value, "path", "install_path", "value")
			for _, row := range newBeeRows(raw) {
				object := newBeeObject(row)
				path := String(object, "install_path")
				if !paths[path] {
					return invalid("$.attachments", "install path is absent from current platform options")
				}
			}
		}
	}
	if resource == "config" {
		changed := false
		for _, key := range []string{"cloud_id", "linked_mods", "ignored_unknown_mods", "ignored_materials", "ignored_fronts", "roleid"} {
			if _, ok := input[key]; ok {
				changed = true
			}
		}
		if !changed {
			return nil
		}
		value, err := c.read(ctx, "backup get", map[string]any{"id": body["cloud_id"]})
		if err != nil {
			return err
		}
		backup := newBeeObject(value)
		known := map[int64]map[string]any{}
		for _, raw := range newBeeRows(backup["t_Known_plug"]) {
			row := newBeeObject(raw)
			known[newBeeID(newBeePick(row, "id", "mod_id"))] = row
		}
		canonical := []any{}
		seen := map[int64]bool{}
		for _, raw := range newBeeRows(body["linked_mods"]) {
			row := newBeeObject(raw)
			id := newBeeID(row["mod_id"])
			remote, ok := known[id]
			if !ok || seen[id] {
				return invalid("$.linked_mods", "plugin is absent from backup or repeated")
			}
			seen[id] = true
			canonical = append(canonical, map[string]any{"mod_id": id, "mod_name": newBeePick(remote, "name", "mod_name"), "mod_file_id": remote["mod_file_id"], "mod_version": remote["mod_version"], "display_name": remote["display_name"], "updateType": newBeePick(remote, "updateType", "update_type")})
		}
		body["linked_mods"] = canonical
		for field, remote := range map[string]string{"ignored_unknown_mods": "t_unKnown_list", "ignored_materials": "t_material_list", "ignored_fronts": "t_font_list"} {
			available := newBeeCollect(newBeeDecoded(backup[remote]), "name")
			for _, name := range newBeeRows(body[field]) {
				if !available[fmt.Sprint(name)] {
					return invalid("$."+field, "component is absent from selected backup")
				}
			}
		}
		if role := fmt.Sprint(body["roleid"]); role != "" && role != "<nil>" {
			roles := newBeeCollect(newBeeDecoded(newBeePick(backup, "wtflist", "t_WTF_list")), "role_id")
			if !roles[role] {
				return invalid("$.roleid", "role is absent from selected backup")
			}
		}
	}
	return nil
}

// The Creator wire contract has no duration for a zero one-time price.
// Normalize before validating options, including when only metadata changes.
func newBeeNormalizePrice(body map[string]any) {
	if _, hasPrice := body["price"]; hasPrice && newBeeID(body["price"]) == 0 {
		body["time_range"] = ""
	}
}
func (c *newBeeCreator) validateBuilds(ctx context.Context, selected any) error {
	value, err := c.read(ctx, "plugin game-versions", nil)
	if err != nil {
		return err
	}
	available := newBeeCollect(value, "version", "build", "support_version", "value")
	// Metadata can use an array of literal build values under each game version.
	var walk func(any)
	walk = func(v any) {
		switch x := v.(type) {
		case map[string]any:
			for key, child := range x {
				if key == "version" {
					for _, build := range newBeeRows(child) {
						switch build.(type) {
						case string, json.Number:
							available[fmt.Sprint(build)] = true
						}
					}
				}
				walk(child)
			}
		case []any:
			for _, child := range x {
				walk(child)
			}
		}
	}
	walk(value)
	seen := map[string]bool{}
	for _, raw := range newBeeRows(selected) {
		build := strings.TrimSpace(fmt.Sprint(raw))
		if !available[build] || seen[build] {
			return invalid("$.game_version_list", "build is unavailable or repeated")
		}
		seen[build] = true
	}
	return nil
}
