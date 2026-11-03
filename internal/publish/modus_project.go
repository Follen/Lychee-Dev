package publish

import (
	"context"
	"encoding/json"
	"reflect"
	"strconv"
	"strings"
)

func modusNull(v any) any {
	if v == nil {
		return "<null>"
	}
	if s, ok := v.(string); ok && strings.TrimSpace(s) == "" {
		return "<null>"
	}
	return v
}
func modusProjectWire(in map[string]any, create bool) (map[string]any, error) {
	w := map[string]any{}
	for k, wire := range map[string]string{"project_id": "projectId", "name": "name", "alt_name": "altName", "summary": "summary", "repo_url": "repoUrl", "description": "description", "required_dependencies": "requiredDependencies"} {
		if v, ok := in[k]; ok {
			if !create && k != "name" && k != "project_id" {
				v = modusNull(v)
			}
			w[wire] = v
		}
	}
	if categories, ok := in["categories"].([]any); ok {
		if len(categories) > 5 {
			return nil, invalid("$.categories", "at most five categories are allowed")
		}
		seen := map[string]bool{}
		ids := []string{}
		for _, v := range categories {
			id := fmtModusID(v)
			if seen[id] {
				return nil, invalid("$.categories", "duplicate category")
			}
			seen[id] = true
			ids = append(ids, id)
		}
		w["categories"] = strings.Join(ids, ",")
	}
	if platforms, ok := in["publish_platforms"].([]any); ok {
		bits := 0
		for _, v := range platforms {
			bit := 1
			if v == "bigfoot" {
				bit = 2
			}
			if bits&bit != 0 {
				return nil, invalid("$.publish_platforms", "duplicate platform")
			}
			bits |= bit
		}
		if bits&2 != 0 && in["required_tier_id"] != nil {
			return nil, invalid("$.required_tier_id", "BigFoot synchronization does not accept a required tier")
		}
		if strings.Contains(","+fmtString(w["categories"])+",", ",998,") && bits != 2 {
			return nil, invalid("$.publish_platforms", "category 998 requires only BigFoot")
		}
		w["synchronizationType"] = bits
	}
	if license, ok := in["license"].(map[string]any); ok {
		typeName := strings.ToLower(String(license, "type"))
		if (typeName == "custom" || typeName == "自定义") && String(license, "content") == "" {
			return nil, invalid("$.license.content", "custom license requires content")
		}
		raw, _ := json.Marshal(license)
		w["license"] = string(raw)
	}
	if v, ok := in["required_tier_id"]; ok {
		if !create {
			if v == nil {
				v = "<null>"
			} else {
				v = fmtModusID(v)
			}
		}
		if v != nil {
			w["requiredTierId"] = v
		}
	}
	if create {
		w["images"] = 0
		w["screenshotBase64sReqs"] = map[string]any{"name": "logo.webp", "screenshotBase64s": String(in, "logo_base64")}
	}
	if ops, ok := in["image_ops"].([]any); ok {
		for i, v := range ops {
			o := v.(map[string]any)
			action := String(o, "op")
			allowed := map[string]bool{"op": true}
			required := []string{"name"}
			if action == "rename" {
				required = []string{"from", "to"}
			} else if action == "upload" {
				required = append(required, "base64")
			}
			for _, k := range required {
				allowed[k] = true
				if String(o, k) == "" {
					return nil, invalid("$.image_ops["+strconv.Itoa(i)+"]."+k, "required for selected image action")
				}
			}
			for k := range o {
				if !allowed[k] {
					return nil, invalid("$.image_ops["+strconv.Itoa(i)+"]."+k, "not valid for selected image action")
				}
			}
		}
		w["images"] = in["images"]
		w["imagesOps"] = ops
	}
	if !create && len(w) < 2 {
		return nil, invalid("$", "provide at least one project change")
	}
	return w, nil
}
func fmtString(v any) string { s, _ := v.(string); return s }
func modusRows(v any) ([]any, error) {
	if a, ok := v.([]any); ok {
		return a, nil
	}
	if obj, ok := v.(map[string]any); ok {
		for _, key := range []string{"rows", "list", "items", "records", "data"} {
			if child, ok := obj[key]; ok {
				return modusRows(child)
			}
		}
		if _, ok := obj["id"]; ok {
			return []any{obj}, nil
		}
	}
	return nil, failure("publish.modus_shape", "unrecognized ModUs collection")
}
func modusCollectIDs(v any, ids map[string]bool) {
	switch x := v.(type) {
	case []any:
		for _, row := range x {
			modusCollectIDs(row, ids)
		}
	case map[string]any:
		if id := fmtModusID(x["id"]); id != "" {
			ids[id] = true
		}
		for _, key := range []string{"children", "rows", "list", "data", "items"} {
			if child, ok := x[key]; ok {
				modusCollectIDs(child, ids)
			}
		}
	}
}
func (c *modusClient) prepareProject(ctx context.Context, in map[string]any, create bool) (map[string]any, error) {
	w, err := modusProjectWire(in, create)
	if err != nil {
		return nil, err
	}
	if _, ok := in["categories"]; ok {
		catalog, err := c.read(ctx, "options categories", nil)
		if err != nil {
			return nil, err
		}
		ids := map[string]bool{}
		modusCollectIDs(catalog, ids)
		for _, id := range strings.Split(fmtString(w["categories"]), ",") {
			if !ids[id] {
				return nil, invalid("$.categories", "category is absent from current Creator options")
			}
		}
	}
	if in["required_tier_id"] != nil {
		catalog, err := c.read(ctx, "options subscription-tiers", nil)
		if err != nil {
			return nil, err
		}
		rows, err := modusRows(catalog)
		if err != nil {
			return nil, err
		}
		found := false
		for _, v := range rows {
			row, ok := v.(map[string]any)
			if ok && fmtModusID(row["id"]) == fmtModusID(in["required_tier_id"]) && row["isEnabled"] != false && fmtModusID(row["isEnabled"]) != "0" {
				found = true
			}
		}
		if !found {
			return nil, invalid("$.required_tier_id", "tier is absent or disabled")
		}
	}
	if !create {
		current, err := c.read(ctx, "project get", in)
		if err != nil {
			return nil, err
		}
		obj, ok := current.(map[string]any)
		if !ok {
			return nil, failure("publish.modus_shape", "project detail is not an object")
		}
		// A partial edit must respect current synchronization selection as well.
		bits := Integer(in, "synchronization_type")
		if n, ok := w["synchronizationType"].(int); ok {
			bits = int64(n)
		} else {
			bits, _ = strconv.ParseInt(fmtModusID(obj["synchronizationType"]), 10, 64)
		}
		categories := obj["categories"]
		if selected, present := w["categories"]; present {
			categories = selected
		}
		categoryIDs := []string{}
		switch values := categories.(type) {
		case string:
			categoryIDs = strings.Split(values, ",")
		case []any:
			for _, value := range values {
				if row, ok := value.(map[string]any); ok {
					value = row["id"]
				}
				categoryIDs = append(categoryIDs, fmtModusID(value))
			}
		}
		for _, id := range categoryIDs {
			if strings.TrimSpace(id) == "998" && bits != 2 {
				return nil, invalid("$.publish_platforms", "category 998 requires only BigFoot")
			}
		}
		tier := in["required_tier_id"]
		if _, present := in["required_tier_id"]; !present {
			tier = obj["requiredTierId"]
		}
		if bits&2 != 0 && tier != nil && fmtModusID(tier) != "" {
			return nil, invalid("$.required_tier_id", "BigFoot synchronization does not accept a required tier")
		}
		if Integer(in, "project_id") < 200000000 {
			for k, v := range w {
				if k == "projectId" || k == "summary" || k == "description" || k == "altName" {
					continue
				}
				if !modusSameField(k, v, obj[k]) {
					return nil, invalid("$."+k, "restricted imported project field cannot be edited")
				}
			}
			for k := range w {
				if k != "projectId" && k != "summary" && k != "description" && k != "altName" {
					delete(w, k)
				}
			}
		}
	}
	return w, nil
}
func modusSameField(key string, expected, actual any) bool {
	if expected == "<null>" {
		return actual == nil || actual == ""
	}
	if key == "categories" {
		ids := func(v any) map[string]bool {
			m := map[string]bool{}
			if s, ok := v.(string); ok {
				for _, id := range strings.Split(s, ",") {
					m[strings.TrimSpace(id)] = true
				}
			} else if a, ok := v.([]any); ok {
				for _, v := range a {
					if obj, ok := v.(map[string]any); ok {
						v = obj["id"]
					}
					m[fmtModusID(v)] = true
				}
			}
			return m
		}
		return reflect.DeepEqual(ids(expected), ids(actual))
	}
	if key == "license" {
		decode := func(v any) any {
			if s, ok := v.(string); ok {
				var obj any
				if json.Unmarshal([]byte(s), &obj) == nil {
					return obj
				}
			}
			return v
		}
		return reflect.DeepEqual(decode(expected), decode(actual))
	}
	// JSON integers may be strings on readback; comparison remains exact.
	if a, b := fmtModusID(expected), fmtModusID(actual); a != "" && b != "" {
		return a == b
	}
	a, _ := json.Marshal(expected)
	b, _ := json.Marshal(actual)
	return string(a) == string(b)
}
