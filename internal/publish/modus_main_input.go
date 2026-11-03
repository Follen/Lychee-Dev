package publish

import "strings"

func modusMainActions() []string {
	return []string{"config list", "config get", "config create", "config edit", "config delete", "wa list", "wa get", "wa create", "wa edit", "wa delete", "wa version-publish", "wa version-delete", "backup list", "backup get", "media upload", "options builds", "options season-subscription", "addon info", "addon projects", "addon history"}
}
func modusMainAction(action string) bool {
	for _, a := range modusMainActions() {
		if action == a {
			return true
		}
	}
	return false
}
func modusMainMutates(action string) bool {
	return strings.HasSuffix(action, " create") || strings.HasSuffix(action, " edit") || strings.HasSuffix(action, " delete") || action == "wa version-publish" || action == "wa version-delete" || action == "media upload"
}
func modusMainInput(action string) (InputSpec, bool) {
	if !modusMainAction(action) {
		return InputSpec{}, false
	}
	text := Field{Type: "string"}
	str := Field{Type: "string", Nonempty: true}
	id := Field{Type: "integer", Minimum: Int(1)}
	bit := Field{Type: "integer", Minimum: Int(0), Maximum: Int(1)}
	s := InputSpec{Properties: map[string]Field{"server_type": {Type: "integer", Minimum: Int(0), Maximum: Int(4), Description: "Explicit ModUs main-client Build ID; options builds lists the choices"}}, Required: []string{"server_type"}, Example: map[string]any{"server_type": 0}, Effects: []string{"uses only the current ModUs main-client account; no Creator account fallback"}}
	resource, verb, _ := strings.Cut(action, " ")
	key := "share_id"
	if resource == "wa" {
		key = "import_id"
	}
	switch action {
	case "options builds":
		s.Properties = map[string]Field{}
		s.Required = []string{}
		s.Example = map[string]any{}
		s.Effects = []string{"offline platform Build catalog"}
	case "options season-subscription":
		s.Properties["author_id"] = id
		s.Properties["game_type"] = str
	case "backup list":
	case "backup get":
		s.Properties["backup_id"] = id
		s.Required = append(s.Required, "backup_id")
		s.Example["backup_id"] = 1
	case "addon info":
		s.Properties["directories"] = Field{Type: "array", Nonempty: true, Items: &str}
		s.Required = append(s.Required, "directories")
		s.Example["directories"] = []any{"Example"}
	case "addon projects":
		s.Properties["project_ids"] = Field{Type: "array", Nonempty: true, Items: &id}
		s.Required = append(s.Required, "project_ids")
		s.Example["project_ids"] = []any{1}
	case "addon history":
		s.Properties["project_id"] = id
		s.Properties["page_num"] = id
		s.Properties["page_size"] = Field{Type: "integer", Minimum: Int(1), Maximum: Int(100)}
		s.Required = append(s.Required, "project_id")
		s.Example["project_id"] = 1
	case "media upload":
		s.Properties = map[string]Field{"file": str}
		s.Required = []string{"file"}
		s.Example = map[string]any{"file": "image.webp"}
	case "wa version-publish":
		s.Properties["import_id"] = str
		s.Properties["version"] = str
		s.Properties["code_text"] = str
		s.Properties["changelog"] = text
		s.Required = append(s.Required, "import_id", "version", "code_text")
		s.Example["import_id"] = "example"
		s.Example["version"] = "1"
		s.Example["code_text"] = "!WA:2!example"
	case "wa version-delete":
		s.Properties["version_id"] = str
		s.Properties["import_id"] = str
		s.Properties["confirm"] = Field{Type: "string", Enum: []string{"DELETE"}}
		s.Required = append(s.Required, "version_id", "import_id", "confirm")
		s.Example["version_id"] = "1"
		s.Example["import_id"] = "example"
		s.Example["confirm"] = "DELETE"
	default:
		switch verb {
		case "list":
			for _, k := range []string{"page_num", "page_size"} {
				s.Properties[k] = Field{Type: "integer", Minimum: Int(1), Maximum: Int(100)}
			}
			for _, k := range []string{"keyword", "order_by"} {
				s.Properties[k] = text
			}
			s.Properties["mine"] = Field{Type: "boolean"}
			s.Properties["status"] = Field{Type: "integer", Minimum: Int(0)}
			s.Properties["tags"] = Field{Type: "array", Nonempty: true, Items: &str}
			s.Properties["is_paid"] = bit
			s.Properties["platform"] = Field{Type: "integer", Minimum: Int(0)}
			if resource == "config" {
				s.Properties["is_public"] = bit
				s.Properties["share_type"] = Field{Type: "integer", Minimum: Int(0)}
			} else {
				s.Properties["support_addon"] = str
			}
		case "get", "delete":
			s.Properties[key] = str
			s.Required = append(s.Required, key)
			s.Example[key] = "example"
			if verb == "delete" {
				s.Properties["confirm"] = Field{Type: "string", Enum: []string{"DELETE"}}
				s.Required = append(s.Required, "confirm")
				s.Example["confirm"] = "DELETE"
			}
		case "create", "edit":
			for _, k := range []string{"content", "content_text", "image_url", "tags", "title"} {
				s.Properties[k] = str
			}
			s.Properties["addons_id"] = text
			s.Properties["is_paid"] = bit
			s.Properties["is_public"] = bit
			s.Properties["price"] = Field{Type: "number", Minimum: Int(0)}
			s.Properties["share_type"] = Field{Type: "integer", Minimum: Int(0)}
			s.Properties["required_tier_id"] = Field{Type: "integer", Minimum: Int(1), Nullable: true}
			s.Properties["sub_type"] = bit
			for _, k := range []string{"platform", "synchronization_type"} {
				s.Properties[k] = Field{Type: "integer", Minimum: Int(1), Maximum: Int(3), Description: "Platform wire value 1 or 3"}
			}
			if resource == "config" {
				s.Properties["backup_id"] = id
				s.Properties["account_name"] = text
				s.Properties["role_name"] = text
				s.Properties["exclude_wtf"] = bit
			} else {
				s.Properties["code_text"] = str
				s.Properties["file_path"] = Field{Type: "string", Enum: []string{""}}
				s.Properties["support_addon"] = str
				s.Properties["version"] = str
			}
			if verb == "edit" {
				s.Properties[key] = str
				s.Required = append(s.Required, key)
				s.Example[key] = "example"
				s.Example["title"] = "Updated"
			} else {
				s.Required = append(s.Required, "content", "content_text", "image_url", "tags", "title", "addons_id", "is_paid", "is_public", "price", "share_type")
				if resource == "config" {
					s.Required = append(s.Required, "backup_id", "exclude_wtf")
				} else {
					s.Required = append(s.Required, "code_text", "support_addon", "version")
				}
				s.Example = map[string]any{"server_type": 0, "content": "<p>Example</p>", "content_text": "Example", "image_url": "https://cdn.modus.cool/example.webp", "tags": "1", "title": "Example", "addons_id": "1", "is_paid": 0, "is_public": 1, "price": 0, "share_type": 0}
				if resource == "config" {
					s.Example["backup_id"] = 1
					s.Example["exclude_wtf"] = 1
				} else {
					s.Example["code_text"] = "!WA:2!example"
					s.Example["support_addon"] = "WeakAuras"
					s.Example["version"] = "1"
				}
			}
		default:
			return InputSpec{}, false
		}
	}
	return s, true
}
