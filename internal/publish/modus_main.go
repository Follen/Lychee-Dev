package publish

import (
	"context"
	"encoding/base64"
	"encoding/json"
	"net/url"
	"regexp"
	"strings"

	"golang.org/x/net/html"
)

var modusBuildCodes = []string{"wow_retail", "wow_classic_era", "wow_classic", "wow_classic_titan", "wow_anniversary"}

func modusBuilds() []map[string]any {
	out := []map[string]any{}
	for i, code := range modusBuildCodes {
		out = append(out, map[string]any{"id": i, "code": code})
	}
	return out
}
func (c *modusClient) readMain(ctx context.Context, action string, in map[string]any) (any, error) {
	build := Integer(in, "server_type")
	resource, verb, _ := strings.Cut(action, " ")
	if action == "options builds" {
		return modusBuilds(), nil
	}
	if action == "options season-subscription" {
		author := fmtModusID(in["author_id"])
		if author == "" {
			var err error
			author, err = c.account(ctx)
			if err != nil {
				return nil, err
			}
		}
		game := String(in, "game_type")
		if game == "" {
			game = modusBuildCodes[build]
		}
		games, err := c.read(ctx, "options subscription-game-types", nil)
		if err != nil {
			return nil, err
		}
		rows, err := modusRows(games)
		if err != nil {
			return nil, err
		}
		found := false
		for _, row := range rows {
			obj, _ := row.(map[string]any)
			found = found || obj["gameCode"] == game
		}
		if !found {
			return nil, invalid("$.game_type", "game code is absent from current subscription choices")
		}
		return c.call(ctx, "GET", "user/author/season-sub/detail?"+url.Values{"authorId": {author}, "gameType": {game}}.Encode(), nil, nil)
	}
	if resource == "backup" {
		route := "system/user/backup/list"
		if verb == "get" {
			route = "system/user/backup/detail/" + fmtModusID(in["backup_id"])
		}
		return c.call(ctx, "GET", route, nil, &build)
	}
	if resource == "addon" {
		var route string
		body := map[string]any{}
		switch verb {
		case "info":
			route = "plugin/list/info"
			body["pluginList"] = in["directories"]
		case "projects":
			route = "plugin/list/detail"
			body["projectIds"] = in["project_ids"]
		case "history":
			route = "plugin/project/history"
			body["projectIds"] = []any{in["project_id"]}
			body["pageNum"] = modusPage(in, "page_num", 1)
			body["pageSize"] = modusPage(in, "page_size", 5)
		}
		return c.call(ctx, "POST", route, body, &build)
	}
	path, key, ids := "share", "share_id", "shareIds"
	if resource == "wa" {
		path, key, ids = "import", "import_id", "importIds"
	}
	if verb == "get" {
		v, err := c.call(ctx, "POST", "system/user/"+path+"/detail", map[string]any{ids: []any{in[key]}}, &build)
		if err != nil {
			return nil, err
		}
		return modusOne(v, String(in, key))
	}
	if verb == "list" {
		body := map[string]any{"server": build, "pageNum": modusPage(in, "page_num", 1), "pageSize": modusPage(in, "page_size", 20), "mine": Bool(in, "mine"), "platform": modusPage(in, "platform", 0)}
		if resource == "config" {
			body["shareType"] = modusPage(in, "share_type", 0)
		} else {
			body["status"] = modusPage(in, "status", 1)
		}
		for k, wire := range map[string]string{"keyword": "keyword", "order_by": "orderBy", "is_public": "isPublic", "is_paid": "isPaid", "tags": "tags", "status": "status", "support_addon": "supportAddon"} {
			if v, ok := in[k]; ok {
				body[wire] = v
			}
		}
		return c.call(ctx, "POST", "system/user/"+path+"/list", body, &build)
	}
	return nil, invalid("$action", "unknown main-client read action")
}
func modusPage(in map[string]any, key string, defaultValue int64) int64 {
	if _, ok := in[key]; ok {
		return Integer(in, key)
	}
	return defaultValue
}
func modusOne(v any, id string) (map[string]any, error) {
	if rows, ok := v.([]any); ok {
		if len(rows) != 1 {
			return nil, failure("publish.modus_identity", "detail must contain exactly the selected record")
		}
		v = rows[0]
	}
	obj, ok := v.(map[string]any)
	if !ok || modusResponseID(obj, "id", "shareId", "importId", "backupId") != id {
		return nil, failure("publish.modus_identity", "detail identity differs from selected record")
	}
	return obj, nil
}

var modusEmbeddedWA = regexp.MustCompile(`(?i)\[\[wa:data="[^"]*"\]\]`)
var modusEmbeddedImage = regexp.MustCompile(`\[\[?img:key=(?:"[^"]+"|[^\]]+)\]\]?`)

func modusPlaintext(content string) string {
	if !strings.Contains(content, "<") {
		if raw, err := base64.StdEncoding.DecodeString(content); err == nil {
			content = string(raw)
		}
	}
	content = modusEmbeddedWA.ReplaceAllString(content, "")
	content = modusEmbeddedImage.ReplaceAllString(content, "")
	doc, err := html.Parse(strings.NewReader(content))
	if err != nil {
		return ""
	}
	var b strings.Builder
	var visit func(*html.Node)
	visit = func(n *html.Node) {
		if n.Type == html.ElementNode && (n.Data == "script" || n.Data == "style" || n.Data == "iframe" || n.Data == "object") {
			return
		}
		if n.Type == html.TextNode {
			b.WriteString(n.Data)
		}
		for child := n.FirstChild; child != nil; child = child.NextSibling {
			visit(child)
		}
	}
	visit(doc)
	return strings.ReplaceAll(b.String(), "\u00a0", " ")
}

var modusMainAliases = map[string]string{"share_id": "id", "import_id": "id", "addons_id": "addonsId", "backup_id": "backupId", "account_name": "accountName", "content_text": "contentText", "image_url": "imageUrl", "is_paid": "isPaid", "is_public": "isPublic", "share_type": "shareType", "exclude_wtf": "excludeWtf", "role_name": "roleName", "required_tier_id": "requiredTierId", "sub_type": "subType", "synchronization_type": "synchronizationType", "code_text": "codeText", "file_path": "filePath", "support_addon": "supportAddon"}

func modusMainFieldNames(resource string) map[string]bool {
	keys := strings.Fields("id addonsId content contentText imageUrl isPaid isPublic price shareType tags title requiredTierId subType platform synchronizationType")
	if resource == "config" {
		keys = append(keys, strings.Fields("backupId accountName roleName excludeWtf")...)
	} else {
		keys = append(keys, strings.Fields("codeText filePath supportAddon version")...)
	}
	allowed := map[string]bool{}
	for _, k := range keys {
		allowed[k] = true
	}
	return allowed
}
func (c *modusClient) prepareMain(ctx context.Context, op *modusOperation) (map[string]any, error) {
	resource, verb, _ := strings.Cut(op.Action, " ")
	allowed := modusMainFieldNames(resource)
	wire := map[string]any{}
	if verb == "edit" {
		v, err := c.readMain(ctx, resource+" get", op.Input)
		if err != nil {
			return nil, err
		}
		if resource == "config" && op.Input["backup_id"] != nil && fmtModusID(op.Input["backup_id"]) != fmtModusID(v.(map[string]any)["backupId"]) {
			for _, key := range []string{"addons_id", "exclude_wtf", "account_name", "role_name"} {
				if _, ok := op.Input[key]; !ok {
					return nil, invalid("$."+key, "changing backup requires explicit addon and WTF reselection")
				}
			}
		}
		for k, v := range v.(map[string]any) {
			if allowed[k] && v != nil {
				wire[k] = v
			}
		}
	}
	for k, v := range op.Input {
		key := modusMainAliases[k]
		if key == "" {
			key = k
		}
		if allowed[key] {
			if v == nil {
				delete(wire, key)
			} else {
				wire[key] = v
			}
		}
	}
	if verb == "create" {
		if wire["platform"] == nil {
			wire["platform"] = json.Number("1")
		}
		if resource == "wa" {
			if wire["filePath"] == nil {
				wire["filePath"] = ""
			}
			if wire["synchronizationType"] == nil {
				wire["synchronizationType"] = wire["platform"]
			}
		}
	}
	if err := validateModusMainPlatforms(wire); err != nil {
		return nil, err
	}
	if _, supplied := op.Input["content_text"]; !supplied {
		if _, changed := op.Input["content"]; changed || wire["contentText"] == nil {
			wire["contentText"] = modusPlaintext(fmtString(wire["content"]))
		}
	}
	if resource == "config" && fmtModusID(wire["excludeWtf"]) == "1" {
		if String(op.Input, "account_name") != "" || String(op.Input, "role_name") != "" {
			return nil, invalid("$.account_name", "excluded WTF cannot include account or role")
		}
		wire["accountName"] = ""
		wire["roleName"] = ""
	}
	required := strings.Fields("addonsId content contentText imageUrl isPaid isPublic price shareType tags title")
	if resource == "config" {
		required = append(required, "backupId", "excludeWtf")
	} else {
		required = append(required, "filePath", "supportAddon", "version")
	}
	for _, k := range required {
		if _, ok := wire[k]; !ok {
			return nil, failure("publish.modus_preserve", "current detail lacks required preserved field "+k)
		}
	}
	if err := c.validateMainOptions(ctx, op, wire); err != nil {
		return nil, err
	}
	return wire, nil
}

func validateModusMainPlatforms(wire map[string]any) error {
	for _, key := range []string{"platform", "synchronizationType"} {
		if v := fmtModusID(wire[key]); v != "" && v != "1" && v != "3" {
			return invalid("$."+key, "expected platform value 1 or 3")
		}
	}
	if wire["requiredTierId"] != nil && (fmtModusID(wire["platform"]) == "3" || fmtModusID(wire["synchronizationType"]) == "3") {
		return invalid("$.required_tier_id", "BigFoot synchronization does not accept a tier")
	}
	return nil
}

func (c *modusClient) validateMainOptions(ctx context.Context, op *modusOperation, w map[string]any) error {
	resource, _, _ := strings.Cut(op.Action, " ")
	tagsAction := "options config-tags"
	if resource == "wa" {
		tagsAction = "options wa-tags"
	}
	v, err := c.read(ctx, tagsAction, nil)
	if err != nil {
		return err
	}
	ids := map[string]bool{}
	modusCollectIDs(v, ids)
	for _, tag := range strings.Split(fmtString(w["tags"]), ",") {
		if !ids[strings.TrimSpace(tag)] {
			return invalid("$.tags", "tag is absent from current platform options")
		}
	}
	if w["requiredTierId"] != nil {
		if fmtModusID(w["subType"]) == "1" {
			v, err := c.readMain(ctx, "options season-subscription", op.Input)
			if err != nil {
				return err
			}
			obj, ok := v.(map[string]any)
			if !ok || fmtModusID(obj["id"]) != fmtModusID(w["requiredTierId"]) || fmtModusID(obj["isEnabled"]) != "1" || fmtModusID(obj["platformSeasonStatus"]) == "2" {
				return invalid("$.required_tier_id", "season tier is not enabled for this account and game")
			}
		} else {
			v, err := c.read(ctx, "options subscription-tiers", nil)
			if err != nil {
				return err
			}
			rows, err := modusRows(v)
			if err != nil {
				return err
			}
			found := false
			for _, row := range rows {
				obj, _ := row.(map[string]any)
				if fmtModusID(obj["id"]) == fmtModusID(w["requiredTierId"]) && obj["isEnabled"] != false && fmtModusID(obj["isEnabled"]) != "0" {
					found = true
				}
			}
			if !found {
				return invalid("$.required_tier_id", "tier is absent or disabled")
			}
		}
	}
	if resource == "wa" {
		v, err := c.read(ctx, "options wa-support-addons", nil)
		if err != nil {
			return err
		}
		rows, err := modusRows(v)
		if err != nil {
			return err
		}
		for _, row := range rows {
			obj, _ := row.(map[string]any)
			if obj["name"] == w["supportAddon"] && fmtModusID(obj["id"]) == fmtModusID(w["addonsId"]) {
				return nil
			}
		}
		return invalid("$.support_addon", "addon name and ID do not match current options")
	}
	in := map[string]any{"server_type": op.Input["server_type"], "backup_id": w["backupId"]}
	v, err = c.readMain(ctx, "backup get", in)
	if err != nil {
		return err
	}
	backup, err := modusOne(v, fmtModusID(w["backupId"]))
	if err != nil {
		return err
	}
	if fmtModusID(backup["serverType"]) != fmtModusID(op.Input["server_type"]) {
		return invalid("$.backup_id", "backup belongs to another Build")
	}
	if raw, ok := backup["knownAddons"].(string); ok && raw != "" {
		var known map[string]any
		d := json.NewDecoder(strings.NewReader(raw))
		d.UseNumber()
		if d.Decode(&known) != nil {
			return failure("publish.modus_shape", "invalid backup addon selection")
		}
		if known["projectids"] != nil && fmtModusID(known["projectids"]) != fmtModusID(w["addonsId"]) {
			return invalid("$.addons_id", "addon selection differs from selected backup")
		}
	}
	if fmtModusID(w["excludeWtf"]) == "1" {
		return nil
	}
	accounts := backup["wtfAccounts"]
	if raw, ok := accounts.(string); ok {
		if json.Unmarshal([]byte(raw), &accounts) != nil {
			return failure("publish.modus_shape", "invalid backup accounts")
		}
	}
	rows, ok := accounts.([]any)
	if !ok {
		return invalid("$.account_name", "selected backup has no account choices")
	}
	for _, row := range rows {
		account, _ := row.(map[string]any)
		matched := false
		for _, key := range []string{"id", "accountId", "name", "characterName"} {
			matched = matched || (fmtModusID(account[key]) != "" && fmtModusID(account[key]) == fmtString(w["accountName"]))
		}
		if !matched {
			continue
		}
		roles, _ := account["roles"].([]any)
		if len(roles) == 0 && fmtString(w["roleName"]) == "" {
			return nil
		}
		for _, role := range roles {
			if fmtString(role) == fmtString(w["roleName"]) {
				return nil
			}
		}
		return invalid("$.role_name", "role is absent from selected backup account")
	}
	return invalid("$.account_name", "account is absent from selected backup")
}
