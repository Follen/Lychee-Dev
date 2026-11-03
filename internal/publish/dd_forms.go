package publish

import (
	"context"
	"encoding/json"
	"math/big"
	"net/url"
	"strconv"
	"strings"
)

func ddClone(o map[string]any) map[string]any {
	b, _ := json.Marshal(o)
	d := json.NewDecoder(strings.NewReader(string(b)))
	d.UseNumber()
	out := map[string]any{}
	_ = d.Decode(&out)
	return out
}
func ddTruthy(v any) bool {
	switch x := v.(type) {
	case nil:
		return false
	case bool:
		return x
	case string:
		return x != ""
	case json.Number:
		return x != "0"
	case int:
		return x != 0
	case []any:
		return len(x) > 0
	}
	return true
}
func ddGame(o map[string]any) int64 {
	if n := ddInteger(o["game_type"]); n > 0 {
		return n
	}
	if a, ok := o["game_types"].([]any); ok && len(a) > 0 {
		return ddInteger(a[0])
	}
	return 0
}
func ddScalar(v any, aliases ...string) any {
	if o, ok := v.(map[string]any); ok {
		for _, k := range aliases {
			if x := o[k]; x != nil {
				return x
			}
		}
		return nil
	}
	return v
}
func ddArray(v any, integer bool, aliases ...string) ([]any, error) {
	if v == nil {
		return []any{}, nil
	}
	a, ok := v.([]any)
	if !ok {
		return nil, failure("publish.dd_form", "remote editor field is not an array")
	}
	out := []any{}
	for _, item := range a {
		x := ddScalar(item, aliases...)
		s := ddText(x)
		if s == "" {
			return nil, failure("publish.dd_form", "remote editor field has no scalar identity")
		}
		if integer {
			n, err := strconv.ParseInt(s, 10, 64)
			if err != nil {
				return nil, failure("publish.dd_form", "remote editor identity is not an integer")
			}
			out = append(out, json.Number(strconv.FormatInt(n, 10)))
		} else {
			out = append(out, s)
		}
	}
	return out, nil
}
func ddProjectPlugin(ctx context.Context, api ddAPI, current map[string]any, sn string, authorRequired bool) (map[string]any, error) {
	form := ddClone(current)
	latest, _ := current["latest_version"].(map[string]any)
	if authorRequired {
		rows, err := ddAllAuthor(ctx, api, "plugin", ddGame(current))
		if err != nil {
			return nil, err
		}
		found := false
		for _, v := range rows {
			o, _ := v.(map[string]any)
			if ddText(o["sn"]) == sn || ddText(o["share_sn"]) == sn {
				latest, _ = o["latest_version"].(map[string]any)
				found = true
				break
			}
		}
		if !found || latest == nil {
			return nil, failure("publish.dd_form", "published plugin projection is missing")
		}
		for _, k := range []string{"version", "release_type", "file_path"} {
			if !ddTruthy(latest[k]) {
				return nil, failure("publish.dd_form", "published plugin projection omitted a version field")
			}
		}
	}
	for _, k := range []string{"version", "release_type"} {
		if v, ok := latest[k]; ok {
			form[k] = v
		}
	}
	if v, ok := latest["file_path"]; ok {
		form["detail_url"] = v
	}
	for _, k := range []string{"version", "release_type", "detail_url"} {
		if !ddTruthy(form[k]) {
			return nil, failure("publish.dd_form", "plugin published version field is missing")
		}
	}
	return form, nil
}
func ddPrepareForm(ctx context.Context, api ddAPI, r, verb string, in map[string]any) (map[string]any, int64, error) {
	current := map[string]any{}
	var err error
	sn := String(in, "sn")
	if verb != "create" {
		current, err = ddDetail(ctx, api, r, sn)
		if err != nil {
			return nil, 0, err
		}
		if err = ddRequireOwner(ctx, api, r, sn, current); err != nil {
			return nil, 0, err
		}
	}
	game := ddGame(current)
	if game == 0 {
		game = Integer(in, "game_type")
	}
	if verb == "delete" {
		if r == "backup" {
			if game == 0 {
				return nil, 0, failure("publish.dd_backup", "backup omitted game identity")
			}
			rows, e := ddBackupList(ctx, api)
			if e != nil {
				return nil, 0, e
			}
			refs, e := ddBackupReferences(ctx, api, rows)
			if e != nil {
				return nil, 0, e
			}
			if len(refs[sn]) > 0 {
				return nil, 0, invalid("$.sn", "backup is still referenced by a published configuration")
			}
		}
		if r != "backup" && game == 0 && r == "config" {
			b, e := ddDetail(ctx, api, "backup", ddText(current["backup_sn"]))
			if e != nil {
				return nil, 0, e
			}
			game = ddGame(b)
		}
		if r != "backup" && game == 0 {
			return nil, 0, failure("publish.dd_form", "delete requires the resource game identity")
		}
		return map[string]any{"sn": sn}, game, nil
	}
	if verb == "rename" {
		return map[string]any{"sn": sn, "name": in["name"]}, game, nil
	}
	form := map[string]any{}
	if verb == "create" {
		form = map[string]any{"scope": "public", "share_code_life_type": "seven_day", "buy_life_type": "seven_day", "need_buy": false, "price_fen": json.Number("0"), "with_associate": false, "vip_levels": []any{}}
		if r == "plugin" {
			form["addon_type"] = json.Number("0")
		}
		if r == "wa" {
			form["version"] = "0"
			form["category_ids"] = []any{"ui_original"}
			form["file_install_path"] = "Interface/Addons"
		}
	} else {
		if r == "plugin" {
			current, err = ddProjectPlugin(ctx, api, current, sn, verb == "edit")
			if err != nil {
				return nil, 0, err
			}
		}
		spec, _ := DDInput(r + " create")
		for k := range spec.Properties {
			if v, ok := current[k]; ok && v != nil {
				form[k] = v
			}
		}
		form["sn"] = sn
		form["game_type"] = json.Number(strconv.FormatInt(game, 10))
	}
	for k, v := range in {
		switch k {
		case "session", "file", "logo_file", "wa_file", "detail_img_files", "display_img_files":
			continue
		}
		if strings.HasSuffix(k, "_ids") || strings.HasSuffix(k, "_names") {
			if r == "config" {
				continue
			}
		}
		form[k] = v
	}
	if r == "config" {
		form, game, err = ddConfigForm(ctx, api, current, form, in)
		if err != nil {
			return nil, 0, err
		}
		form["game_type"] = json.Number(strconv.FormatInt(game, 10))
	}
	if r == "wa" {
		if verb != "create" {
			if v, ok := in["game_type"]; ok && !ddEqual(v, current["game_type"]) {
				return nil, 0, invalid("$.game_type", "game type is locked after creation")
			}
			if ddTruthy(current["assign_user_sn"]) && ddText(form["scope"]) != "public" {
				return nil, 0, invalid("$.scope", "assigned WA requires public scope")
			}
		}
		if verb == "update" {
			candidate, old := new(big.Int), new(big.Int)
			s := ddText(form["version"])
			for _, c := range s {
				if c < '0' || c > '9' {
					return nil, 0, invalid("$.version", "WA version must be an increasing integer")
				}
			}
			_, a := candidate.SetString(s, 10)
			_, b := old.SetString(ddText(current["version"]), 10)
			if !a || !b || candidate.Cmp(old) <= 0 {
				return nil, 0, invalid("$.version", "WA version must be greater than current version")
			}
		}
		if String(in, "file") != "" {
			form["with_file"] = true
		}
		if ddTruthy(form["with_file"]) && (ddText(form["file_install_path"]) == "" || ddText(form["file_path"]) == "" && String(in, "file") == "") {
			return nil, 0, invalid("$.file", "WA materials require a ZIP and installation path")
		}
		if path := String(in, "wa_file"); path != "" {
			raw, e := ddReadFile(path, 32<<20)
			if e != nil {
				return nil, 0, e
			}
			form["content"] = strings.TrimSpace(string(raw))
		}
		if content := ddText(form["content"]); content != "" {
			id, e := parseDDWA(ctx, content)
			if e != nil {
				return nil, 0, e
			}
			form["parse_wa_uid"], form["parse_wa_id"] = id.UID, id.ID
		}
	}
	for _, field := range []struct {
		k       string
		integer bool
		aliases []string
	}{{"game_versions", false, []string{"version", "build", "value"}}, {"detail_imgs", false, []string{"url", "path"}}, {"display_imgs", false, []string{"url", "path"}}, {"category_ids", false, []string{"c_id", "category_id", "id", "value"}}, {"second_category_ids", true, []string{"c_id", "category_id", "id", "value"}}, {"vip_levels", true, []string{"level", "id", "value"}}} {
		if v, ok := form[field.k]; ok {
			form[field.k], err = ddArray(v, field.integer, field.aliases...)
			if err != nil {
				return nil, 0, err
			}
		}
	}
	if r == "plugin" {
		if v, ok := form["primary_category_id"]; ok {
			form["primary_category_id"] = ddScalar(v, "c_id", "category_id", "id", "value")
		}
		if verb != "create" {
			if a, ok := form["second_category_ids"].([]any); ok && len(a) > 0 {
				if _, supplied := in["second_category_ids"]; !supplied {
					form["second_category_ids"] = a[:len(a)-1]
				}
			}
		}
		if ddTruthy(current["assign_user_sn"]) && ddText(form["scope"]) != "public" {
			return nil, 0, invalid("$.scope", "assigned plugins require public scope")
		}
		if verb == "update" {
			candidate := strings.TrimSpace(ddText(form["version"]))
			if candidate == "" || strings.EqualFold(candidate, ddText(current["version"])) {
				return nil, 0, invalid("$.version", "existing versions cannot be overwritten")
			}
			if err = ddCheckVersion(ctx, api, sn, game, candidate); err != nil {
				return nil, 0, err
			}
		}
	}
	if len(current) > 0 && (ddTruthy(current["need_buy"]) || ddTruthy(current["need_anchor_vip"])) != (ddTruthy(form["need_buy"]) || ddTruthy(form["need_anchor_vip"])) {
		return nil, 0, invalid("$.need_buy", "outer free/paid usage mode is locked after creation")
	}
	if err = ddCommercial(form, in, r, verb == "create"); err != nil {
		return nil, 0, err
	}
	if len(current) > 0 && (ddTruthy(current["need_buy"]) || ddTruthy(current["need_anchor_vip"])) != (ddTruthy(form["need_buy"]) || ddTruthy(form["need_anchor_vip"])) {
		return nil, 0, invalid("$.scope", "outer free/paid usage mode is locked after creation")
	}
	if err = ddValidateOptions(ctx, api, r, verb, form); err != nil {
		return nil, 0, err
	}
	if verb != "update" {
		if err = ddOptionalOptions(ctx, api, form, current); err != nil {
			return nil, 0, err
		}
	}
	if err = ddRequiredForm(r, form, in); err != nil {
		return nil, 0, err
	}
	if r == "config" {
		delete(form, "game_type")
		if verb != "create" {
			delete(form, "sn")
			form["share_sn"] = sn
		}
	}
	// Upload effects occur only after pure form/dependency admission. The upload
	// module retains the original bytes and has its own reusable receipt.
	return form, game, nil
}

func ddRequireOwner(ctx context.Context, api ddAPI, r, sn string, current map[string]any) error {
	if r != "backup" {
		if owner, ok := current["is_owner"].(bool); ok {
			if owner {
				return nil
			}
			return invalid("$.sn", "selected resource belongs to a different author")
		}
	}
	var rows []any
	var err error
	if r == "backup" {
		v, e := api.call(ctx, "/backup/list", nil, nil, false)
		if e != nil {
			return e
		}
		rows, err = ddItems(v["result"])
		total, known, e := ddTotal(v)
		if e != nil {
			return e
		}
		if known && total != int64(len(rows)) {
			return failure("publish.dd_pagination", "backup membership requires a complete list")
		}
	} else {
		game := ddGame(current)
		if game == 0 && r == "config" {
			backup, e := ddDetail(ctx, api, "backup", ddText(current["backup_sn"]))
			if e != nil {
				return e
			}
			game = ddGame(backup)
		}
		rows, err = ddAllAuthor(ctx, api, r, game)
	}
	if err != nil {
		return err
	}
	for _, v := range rows {
		o := ddMap(v)
		if ddText(o["sn"]) == sn || ddText(o["share_sn"]) == sn {
			return nil
		}
	}
	return invalid("$.sn", "selected resource is absent from this author's complete list")
}

func ddOptionalOptions(ctx context.Context, api ddAPI, f, current map[string]any) error {
	if ddTruthy(f["need_anchor_vip"]) {
		v, err := api.call(ctx, "/anchor_vip/level/list", url.Values{"enrich_acts": {"false"}}, nil, false)
		if err != nil {
			return err
		}
		allowed := map[string]bool{}
		ddCollectValues(v["result"], []string{"id", "level", "value"}, allowed)
		for _, v := range ddSlice(f["vip_levels"]) {
			if !allowed[ddText(v)] {
				return invalid("$.vip_levels", "VIP level is unavailable")
			}
		}
	}
	changed := false
	for _, k := range []string{"jump_room", "room_id", "channel_id", "channel_type"} {
		changed = changed || !ddEqual(f[k], current[k])
	}
	if ddTruthy(f["jump_room"]) && changed {
		v, err := api.channels(ctx)
		if err != nil {
			return err
		}
		rows, err := ddItems(v)
		if err != nil {
			return err
		}
		found := false
		for _, v := range rows {
			o := ddMap(v)
			if ddText(o["room_id"]) == ddText(f["room_id"]) && ddText(o["channel_id"]) == ddText(f["channel_id"]) && ddText(o["channel_type"]) == ddText(f["channel_type"]) {
				found = true
				break
			}
		}
		if !found {
			return invalid("$.channel_id", "room/channel selection is unavailable")
		}
	}
	if ddTruthy(f["with_associate"]) {
		v, err := ddRead(ctx, api, "options associated-acts", map[string]any{"game_type": json.Number(strconv.FormatInt(ddGame(f), 10))})
		if err != nil {
			return err
		}
		available := map[string]bool{}
		for _, v := range ddSlice(ddMap(v)["items"]) {
			o := ddMap(v)
			available[ddText(o["act_type"])+"/"+ddText(o["sn"])] = true
		}
		for _, v := range ddSlice(f["associated_acts"]) {
			o := ddMap(v)
			if !available[ddText(o["act_type"])+"/"+ddText(o["sn"])] {
				return invalid("$.associated_acts", "association is unavailable for selected author and game")
			}
		}
	}
	return nil
}
func ddCommercial(f, in map[string]any, r string, create bool) error {
	if !ddTruthy(f["scope"]) {
		f["scope"] = "public"
	}
	if _, ok := f["price_fen"]; !ok {
		f["price_fen"] = json.Number("0")
	}
	if _, ok := f["vip_levels"]; !ok {
		f["vip_levels"] = []any{}
	}
	if Bool(in, "need_buy") {
		for _, k := range []string{"price_fen", "buy_life_type"} {
			if _, ok := in[k]; !ok {
				return invalid("$."+k, "explicit paid mode requires this field")
			}
		}
	}
	if v := ddInteger(f["price_fen"]); v != 0 && (v < 10 || v > 20000) {
		return invalid("$.price_fen", "price must be zero or 10..20000")
	}
	if ddText(f["scope"]) == "private" {
		f["sync_room"], f["need_anchor_vip"], f["vip_levels"] = false, false, []any{}
	} else if r == "config" {
		delete(f, "share_code_life_type")
	} else {
		f["share_code_life_type"] = "forever"
	}
	if !ddTruthy(f["jump_room"]) {
		f["room_id"], f["channel_id"], f["channel_type"], f["sync_room"] = "", "", "", false
	} else if ddText(f["room_id"]) == "" {
		return invalid("$.room_id", "room is required")
	}
	if !ddTruthy(f["with_associate"]) {
		f["associated_acts"] = []any{}
	} else if !ddTruthy(f["associated_acts"]) {
		return invalid("$.associated_acts", "selected associations are required")
	} else {
		items := []any{}
		for _, v := range ddSlice(f["associated_acts"]) {
			o := ddMap(v)
			sn, kind := ddText(o["sn"]), ddText(o["act_type"])
			if sn == "" || kind != "addon" && kind != "share" && kind != "wa" {
				return invalid("$.associated_acts", "association needs resource type and SN")
			}
			items = append(items, map[string]any{"sn": sn, "act_type": kind})
		}
		f["associated_acts"] = items
	}
	if r == "config" {
		if ddTruthy(f["need_buy"]) {
			f["need_buy"] = json.Number("1")
		} else {
			f["need_buy"] = json.Number("0")
		}
		if !ddTruthy(f["buy_life_type"]) {
			f["buy_life_type"] = "seven_day"
		}
	}
	if create && !ddTruthy(f["need_buy"]) {
		f["price_fen"] = json.Number("0")
	}
	return nil
}
func ddRequiredForm(r string, f, in map[string]any) error {
	for _, k := range []string{"logo", "detail_url", "file_path"} {
		if s := ddText(f[k]); s != "" {
			if err := ddObjectURL(s, true); err != nil {
				return invalid("$."+k, "use a permanent HTTPS artifact URL")
			}
		}
	}
	for _, k := range []string{"detail_imgs", "display_imgs"} {
		if len(ddSlice(f[k])) > 8 {
			return invalid("$."+k, "at most eight images are allowed")
		}
		for _, v := range ddSlice(f[k]) {
			if err := ddObjectURL(ddText(v), true); err != nil {
				return invalid("$."+k, "use permanent HTTPS image URLs")
			}
		}
	}
	required := "creation_statement"
	switch r {
	case "plugin":
		required += " game_versions name description primary_category_id release_type version html_desc update_desc"
		if !ddTruthy(f["logo"]) && !ddTruthy(in["logo_file"]) {
			return invalid("$.logo", "plugin logo is required")
		}
		if !ddTruthy(f["detail_url"]) && !ddTruthy(in["file"]) {
			return invalid("$.file", "plugin archive is required")
		}
	case "config":
		required += " backup_sn title brief_desc desc"
	case "wa":
		required += " name game_version brief_desc category_ids content desc update_desc"
	}
	for _, k := range strings.Fields(required) {
		if !ddTruthy(f[k]) {
			return invalid("$."+k, "field is required by the DD editor")
		}
	}
	images := "display_imgs"
	files := "display_img_files"
	if r == "plugin" {
		images = "detail_imgs"
		files = "detail_img_files"
	}
	if !ddTruthy(f[images]) && !ddTruthy(in[files]) {
		return invalid("$."+images, "at least one display image is required")
	}
	return nil
}
func ddCheckVersion(ctx context.Context, api ddAPI, sn string, game int64, want string) error {
	seen := map[string]bool{}
	count := int64(0)
	for p := 1; p <= 1000; p++ {
		v, err := api.call(ctx, "/addon/addon_versions", url.Values{"sn": {sn}, "game_type": {strconv.FormatInt(game, 10)}, "page": {strconv.Itoa(p)}}, nil, false)
		if err != nil {
			return err
		}
		rows, err := ddItems(v["result"])
		if err != nil {
			return err
		}
		hash := ddDigest(rows)
		if len(rows) > 0 && seen[hash] {
			return failure("publish.dd_pagination", "version listing repeated a page")
		}
		seen[hash] = true
		for _, row := range rows {
			o, _ := row.(map[string]any)
			if strings.EqualFold(strings.TrimSpace(ddText(o["version"])), want) {
				return invalid("$.version", "version already exists")
			}
		}
		count += int64(len(rows))
		total, known, err := ddTotal(v)
		if err != nil {
			return err
		}
		if len(rows) == 0 || known && count >= total {
			return nil
		}
	}
	return failure("publish.dd_pagination", "version listing exceeded page limit")
}
func ddValidateOptions(ctx context.Context, api ddAPI, r, verb string, f map[string]any) error {
	game := ddGame(f)
	if r == "config" {
		return nil
	}
	v, err := api.call(ctx, "/game_versions/list", url.Values{"game_type": {strconv.FormatInt(game, 10)}}, nil, false)
	if err != nil {
		return err
	}
	allowed := map[string]bool{}
	ddCollectValues(v["result"], []string{"game_version", "version", "build", "value"}, allowed)
	versions := []any{f["game_version"]}
	if r == "plugin" {
		versions, _ = f["game_versions"].([]any)
	}
	for _, version := range versions {
		if !allowed[ddText(version)] {
			return invalid("$.game_versions", "selected version is absent from current platform options")
		}
	}
	path := "/addon/category"
	q := url.Values{}
	if r == "wa" {
		path = "/wa/categories"
		q.Set("game_type", strconv.FormatInt(game, 10))
	}
	v, err = api.call(ctx, path, q, nil, false)
	if err != nil {
		return err
	}
	allowed = map[string]bool{}
	ddCollectValues(v["result"], []string{"c_id", "category_id", "id", "value"}, allowed)
	if r == "plugin" {
		rows, e := ddItems(v["result"])
		if e != nil {
			return e
		}
		primary := ddText(f["primary_category_id"])
		var children map[string]bool
		for _, row := range rows {
			o := ddMap(row)
			if ddText(ddScalar(o, "id", "c_id", "category_id", "value")) == primary {
				children = map[string]bool{}
				ddCollectValues(o["children"], []string{"id", "c_id", "category_id", "value"}, children)
				break
			}
		}
		if children == nil {
			return invalid("$.primary_category_id", "select a current top-level category")
		}
		selected := ddSlice(f["second_category_ids"])
		if len(children) > 0 && len(selected) == 0 {
			return invalid("$.second_category_ids", "select at least one child category")
		}
		for _, child := range selected {
			if !children[ddText(child)] {
				return invalid("$.second_category_ids", "selected child does not belong to primary category")
			}
		}
	}
	cats := []any{f["primary_category_id"]}
	if r == "wa" {
		cats, _ = f["category_ids"].([]any)
	}
	for _, c := range cats {
		if r == "wa" && ddText(c) == "ui_original" {
			continue
		}
		if !allowed[ddText(c)] {
			return invalid("$.category_ids", "selected category is absent from current platform options")
		}
	}
	return nil
}
func ddCollectValues(v any, keys []string, out map[string]bool) {
	switch x := v.(type) {
	case []any:
		for _, v := range x {
			if s := ddText(v); s != "" {
				out[s] = true
			}
			ddCollectValues(v, keys, out)
		}
	case map[string]any:
		for _, k := range keys {
			if s := ddText(x[k]); s != "" {
				out[s] = true
			}
		}
		for _, v := range x {
			ddCollectValues(v, keys, out)
		}
	}
}
