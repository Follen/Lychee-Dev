package publish

import (
	"context"
	"encoding/json"
	"strconv"
)

// Detail and author-list caches advance independently. Keep their version
// projections separate: a new version and an old file never prove one release.
func ddPluginReadbackProjection(value map[string]any, version bool) (map[string]any, error) {
	form := ddClone(value)
	latest := ddMap(value["latest_version"])
	keys := []string{"version", "release_type"}
	if version {
		keys = append(keys, "game_versions", "update_desc")
	}
	for _, key := range keys {
		if v := latest[key]; v != nil {
			form[key] = v
		}
	}
	if v := latest["file_path"]; v != nil {
		form["detail_url"] = v
	}
	if game := ddGame(value); game > 0 {
		form["game_type"] = json.Number(strconv.FormatInt(game, 10))
	}
	for _, field := range []struct {
		key     string
		integer bool
		aliases []string
	}{
		{"game_versions", false, []string{"version", "game_version", "build", "value"}},
		{"detail_imgs", false, []string{"url", "path"}},
		{"second_category_ids", true, []string{"c_id", "category_id", "id", "value"}},
		{"vip_levels", true, []string{"level", "id", "value"}},
	} {
		if v, ok := form[field.key]; ok {
			items, err := ddArray(v, field.integer, field.aliases...)
			if err != nil {
				return nil, err
			}
			if field.key == "second_category_ids" && len(items) > 0 {
				items = items[:len(items)-1]
			}
			form[field.key] = items
		}
	}
	if v, ok := form["primary_category_id"]; ok {
		form["primary_category_id"] = ddScalar(v, "c_id", "category_id", "id", "value")
	}
	return form, nil
}

func ddPluginMatches(ctx context.Context, api ddAPI, r ddReceipt, detail map[string]any) bool {
	actual, err := ddPluginReadbackProjection(detail, false)
	if err != nil {
		return false
	}
	actualVersion, err := ddPluginReadbackProjection(detail, true)
	if err != nil {
		return false
	}
	rows, err := ddAllAuthor(ctx, api, "plugin", r.Game)
	if err != nil {
		return false
	}
	var author map[string]any
	var authorVersionFields map[string]any
	for _, row := range rows {
		if ddText(ddMap(row)["sn"]) == r.SN {
			author, err = ddPluginReadbackProjection(ddMap(row), false)
			if err != nil {
				return false
			}
			authorVersionFields, err = ddPluginReadbackProjection(ddMap(row), true)
			if err != nil {
				return false
			}
			break
		}
	}
	versionFields := map[string]bool{"game_versions": true, "detail_url": true, "release_type": true, "version": true, "update_desc": true}
	detailVersion, authorVersion := true, true
	for key, want := range r.Expected {
		if key == "sn" {
			continue
		}
		a, aok := actual[key]
		b, bok := author[key]
		amatch, bmatch := aok && ddSubset(want, a), bok && ddSubset(want, b)
		if versionFields[key] {
			av, avok := actualVersion[key]
			bv, bvok := authorVersionFields[key]
			detailVersion = detailVersion && avok && ddSubset(want, av)
			authorVersion = authorVersion && bvok && ddSubset(want, bv)
		}
		if r.Action != "plugin update" && !amatch && !bmatch {
			return false
		}
	}
	return r.Action == "plugin edit" || detailVersion || authorVersion
}
