package publish

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"net/http"
	"strconv"
	"strings"
)

type modusClient struct {
	http   *HTTP
	origin string
	static string
	token  string
	device string
	main   bool
}

func newModusCreator() (*modusClient, error) {
	token, err := modusCreatorToken()
	if err != nil {
		return nil, err
	}
	return &modusClient{http: NewHTTP(), origin: "https://app.modus.cool/api/", static: "https://cdn.modus.cool/modus/client_static_api/", token: token}, nil
}
func (c *modusClient) call(ctx context.Context, method, route string, body any, build *int64) (any, error) {
	token := c.token
	if !c.main && !strings.HasPrefix(strings.ToLower(token), "bearer ") {
		token = "Bearer " + token
	}
	headers := http.Header{"Authorization": []string{token}}
	if c.device != "" {
		headers.Set("X-Device-Id", c.device)
	}
	if build != nil {
		headers.Set("X-Server-Type", strconv.FormatInt(*build, 10))
	}
	v, err := c.http.Value(ctx, method, c.origin+route, headers, body)
	if err != nil {
		return nil, err
	}
	if object, ok := v.(map[string]any); ok {
		code := fmtModusID(object["code"])
		n, _ := strconv.ParseInt(code, 10, 64)
		if object["success"] == false || n >= 400 {
			return nil, &Error{Code: "publish.modus_rejected", Message: "ModUs rejected the request", Exit: 5, Details: map[string]any{"businessCode": code}}
		}
		if data, ok := object["data"]; ok {
			return data, nil
		}
	}
	return v, nil
}
func modusSafe(v any) any {
	switch x := v.(type) {
	case map[string]any:
		r := map[string]any{}
		for k, v := range x {
			n := strings.ToLower(strings.ReplaceAll(k, "-", "_"))
			switch n {
			case "token", "access_token", "accesstoken", "refresh_token", "authorization", "cookie", "signedurl", "signed_url", "upload_url", "password", "secret":
				r[k] = "[REDACTED]"
			case "code_text", "codetext", "import_string", "wa_str", "encoded", "base64", "logo_base64", "screenshot_base64s", "screenshotbase64s":
				if content, ok := v.(string); ok {
					digest := sha256.Sum256([]byte(content))
					r[k] = map[string]any{"bytes": len(content), "sha256": hex.EncodeToString(digest[:])}
				} else {
					r[k] = "[REDACTED]"
				}
			default:
				r[k] = modusSafe(v)
			}
		}
		return r
	case []any:
		r := make([]any, len(x))
		for i, v := range x {
			r[i] = modusSafe(v)
		}
		return r
	default:
		return v
	}
}

// Read contracts use explicit pages, with platform pagination returned intact.
// The complete flag means this requested page was fetched, not all pages.
func modusReadInput(action string) (InputSpec, bool) {
	s := InputSpec{Properties: map[string]Field{}, Required: []string{}, Example: map[string]any{}, Effects: []string{"reads the selected ModUs Creator login and platform data"}}
	id := Field{Type: "integer", Minimum: Int(1), Description: "Positive ModUs identifier"}
	str := Field{Type: "string", Nonempty: true, Description: "Explicit platform value"}
	page := func() {
		s.Properties["page_num"] = Field{Type: "integer", Minimum: Int(1)}
		s.Properties["page_size"] = Field{Type: "integer", Minimum: Int(1), Maximum: Int(100)}
	}
	project := func() {
		s.Properties["project_id"] = id
		s.Required = append(s.Required, "project_id")
		s.Example["project_id"] = 200000001
	}
	switch action {
	case "session doctor", "account info", "project statistics", "account active-subscriptions":
	case "project list":
		page()
		s.Properties["keyword"] = str
		s.Properties["order_by"] = str
		s.Properties["ascending"] = Field{Type: "boolean"}
	case "project get":
		project()
	case "plugin list":
		project()
		page()
	case "plugin get":
		project()
		s.Properties["file_id"] = id
		s.Required = append(s.Required, "file_id")
		s.Example["file_id"] = 1
	case "options categories", "options subscription-tiers", "options subscription-game-types", "options config-tags", "options wa-tags", "options wa-support-addons":
	case "options game-versions":
		s.Properties["keys"] = Field{Type: "array", Nonempty: true, Items: &str}
		s.Required = []string{"keys"}
		s.Example["keys"] = []any{"wow_builds"}
	default:
		return InputSpec{}, false
	}
	if strings.HasPrefix(action, "options config-") || action == "options wa-tags" || action == "options wa-support-addons" {
		s.Effects = []string{"reads public ModUs static options; no credentials"}
	}
	return s, true
}
func (c *modusClient) read(ctx context.Context, action string, input map[string]any) (any, error) {
	if modusMainAction(action) {
		return c.readMain(ctx, action, input)
	}
	id := strconv.FormatInt(Integer(input, "project_id"), 10)
	page, size := Integer(input, "page_num"), Integer(input, "page_size")
	if page == 0 {
		page = 1
	}
	if size == 0 {
		size = 50
	}
	switch action {
	case "account info":
		return c.call(ctx, "GET", "system/user/getInfo", nil, nil)
	case "account active-subscriptions":
		return c.call(ctx, "GET", "user/author/subscription/active/count", nil, nil)
	case "project statistics":
		return c.call(ctx, "GET", "game/data/author/project/statistics", nil, nil)
	case "project list":
		body := map[string]any{"pageNum": page, "pageSize": size}
		for key, wire := range map[string]string{"keyword": "keyword", "order_by": "orderByColumn", "ascending": "isAsc"} {
			if v, ok := input[key]; ok {
				body[wire] = v
			}
		}
		return c.call(ctx, "POST", "game/data/author/project/list", body, nil)
	case "project get":
		v, err := c.call(ctx, "GET", "game/data/author/project/detail/"+id, nil, nil)
		if err == nil && modusResponseID(v, "projectId", "id") != id {
			return nil, failure("publish.modus_identity", "project detail does not identify the requested project")
		}
		return v, err
	case "project dependencies":
		body := map[string]any{}
		if value, ok := input["name"]; ok {
			body["name"] = value
		} else {
			body["projectIds"] = input["project_ids"]
		}
		return c.call(ctx, "POST", "game/data/author/project/dependency/query", body, nil)
	case "plugin list":
		return c.call(ctx, "POST", "game/data/author/project/file/list", map[string]any{"projectId": input["project_id"], "pageNum": page, "pageSize": size}, nil)
	case "plugin get":
		v, err := c.call(ctx, "GET", "game/data/author/project/file/detail/"+strconv.FormatInt(Integer(input, "file_id"), 10), nil, nil)
		if err != nil {
			return nil, err
		}
		obj, ok := v.(map[string]any)
		fileID := strconv.FormatInt(Integer(input, "file_id"), 10)
		if !ok || modusResponseID(obj, "fileId", "id") != fileID || (obj["projectId"] != nil && fmtModusID(obj["projectId"]) != id) {
			return nil, failure("publish.modus_identity", "release detail does not identify the selected project and file")
		}
		if obj["projectId"] == nil {
			found := false
			for page := int64(1); page <= 1000; page++ {
				list, err := c.call(ctx, "POST", "game/data/author/project/file/list", map[string]any{"projectId": input["project_id"], "pageNum": page, "pageSize": 100}, nil)
				if err != nil {
					return nil, err
				}
				rows, err := modusRows(list)
				if err != nil {
					return nil, err
				}
				for _, row := range rows {
					if modusResponseID(row, "fileId", "id") == fileID {
						found = true
						break
					}
				}
				if found || len(rows) < 100 {
					break
				}
			}
			if !found {
				return nil, failure("publish.modus_identity", "selected file was not found in the project release list")
			}
		}
		return v, nil
	case "options game-versions":
		return c.call(ctx, "POST", "game/data/config/detail", map[string]any{"keys": input["keys"]}, nil)
	}
	routes := map[string]string{"options categories": "plugin/list/Categories", "options subscription-tiers": "user/author/subscription/tiers", "options subscription-game-types": "user/game-type/list"}
	if route, ok := routes[action]; ok {
		return c.call(ctx, "GET", route, nil, nil)
	}
	files := map[string]string{"options config-tags": "share_tags.json", "options wa-tags": "imports_tags.json", "options wa-support-addons": "imports_support_addons.json"}
	if file, ok := files[action]; ok {
		return c.http.Value(ctx, "GET", c.static+file, nil, nil)
	}
	return nil, invalid("$action", "unknown ModUs read action")
}
func fmtModusID(v any) string {
	switch x := v.(type) {
	case json.Number:
		return string(x)
	case string:
		return x
	}
	return ""
}
