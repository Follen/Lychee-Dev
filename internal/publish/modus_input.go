package publish

import "strings"

func ModusActions() []string {
	return append(append([]string{
		"session doctor", "account info", "account active-subscriptions", "project statistics", "project list", "project get", "project dependencies",
		"project create", "project edit", "project delete", "plugin list", "plugin get", "plugin upload", "plugin edit", "plugin delete",
		"options categories", "options subscription-tiers", "options subscription-game-types", "options game-versions", "options config-tags", "options wa-tags", "options wa-support-addons",
		"operation status", "operation resume", "operation verify",
	}, modusMainActions()...), modusBackupActions()...)
}
func ModusMutates(action string) bool {
	return modusBackupMutates(action) || modusMainMutates(action) || action == "project create" || action == "project edit" || action == "project delete" || action == "plugin upload" || action == "plugin edit" || action == "plugin delete" || action == "operation resume"
}
func ModusInput(action string) (InputSpec, bool) {
	if s, ok := modusBackupInput(action); ok {
		return s, true
	}
	if s, ok := modusMainInput(action); ok {
		return s, true
	}
	if s, ok := modusReadInput(action); ok {
		if action == "session doctor" || action == "account info" {
			s.Properties["session_source"] = Field{Type: "string", Enum: []string{"creator", "main"}, Description: "Explicit account source; default Creator; no fallback between accounts"}
		}
		return s, true
	}
	s := InputSpec{Properties: map[string]Field{}, Required: []string{}, Example: map[string]any{}, Effects: []string{"reads current Creator account; preserves the original request before remote writes; interrupted writes require original-operation recovery"}}
	id := Field{Type: "integer", Minimum: Int(1)}
	str := Field{Type: "string", Nonempty: true}
	project := func() {
		s.Properties["project_id"] = id
		s.Required = append(s.Required, "project_id")
		s.Example["project_id"] = 200000001
	}
	file := func() {
		s.Properties["file_id"] = id
		s.Required = append(s.Required, "file_id")
		s.Example["file_id"] = 9
	}
	switch action {
	case "operation status", "operation verify", "operation resume":
		s.Properties["request"] = Field{Type: "string", Nonempty: true, MaxLength: 128, Description: "Original local request ID"}
		s.Required = []string{"request"}
		s.Example["request"] = "release-1"
		if action == "operation verify" {
			s.Properties["remote_id"] = id
			s.Properties["remote_key"] = Field{Type: "string", Nonempty: true, Description: "Original media key or opaque main-client resource ID"}
			s.MutuallyExclusive = [][]string{{"remote_id", "remote_key"}}
		}
		s.Effects = []string{"reads original operation; verifies original account, resource and saved fields; never replays an uncertain write"}
		if action == "operation status" {
			s.Effects = []string{"reads original local operation only; no credentials or network"}
		}
	case "project dependencies":
		s.Properties["name"] = str
		s.Properties["project_ids"] = Field{Type: "array", Nonempty: true, Items: &Field{Type: "integer", Minimum: Int(1), Maximum: Int(2147483647)}}
		s.ExactlyOne = [][]string{{"name", "project_ids"}}
		s.Example["name"] = "Example"
		s.Effects = []string{"reads Creator dependency options"}
	case "project create", "project edit":
		if action == "project edit" {
			project()
		}
		license := Field{Type: "object", Properties: map[string]Field{"type": str, "holder": str, "year": str, "content": str}, Required: []string{"type"}}
		s.Properties["game"] = Field{Type: "string", Nonempty: true, Description: "Selected Creator game identity"}
		for _, n := range []string{"name", "summary"} {
			s.Properties[n] = str
		}
		s.Properties["name"] = Field{Type: "string", Nonempty: true, MaxLength: 120}
		s.Properties["summary"] = Field{Type: "string", Nonempty: true, MaxLength: 500}
		s.Properties["alt_name"] = Field{Type: "string", Nullable: action == "project edit", MaxLength: 120}
		s.Properties["categories"] = Field{Type: "array", Nonempty: true, Items: &id, Description: "At most five distinct current category IDs"}
		s.Properties["license"] = license
		s.Properties["publish_platforms"] = Field{Type: "array", Nonempty: true, Items: &Field{Type: "string", Enum: []string{"modus", "bigfoot"}}}
		s.Properties["required_tier_id"] = Field{Type: "integer", Minimum: Int(1), Nullable: true}
		s.Properties["repo_url"] = Field{Type: "string", Nullable: action == "project edit", MaxLength: 500}
		if action == "project create" {
			s.Properties["logo_base64"] = Field{Type: "string", Description: "Project logo base64; Creator detail exposes no image bytes, so image verification remains pending after submission"}
			s.Required = append(s.Required, "game", "name", "summary", "categories", "license", "publish_platforms")
			s.Example = map[string]any{"game": "wow", "name": "Example", "summary": "Example addon", "categories": []any{1}, "license": map[string]any{"type": "MIT"}, "publish_platforms": []any{"modus"}}
		} else {
			s.Properties["description"] = Field{Type: "string", Nullable: true, MaxLength: 100000}
			s.Properties["required_dependencies"] = Field{Type: "string", Nullable: true, MaxLength: 4000}
			s.Properties["images"] = Field{Type: "integer", Minimum: Int(0)}
			s.Properties["image_ops"] = Field{Type: "array", Nonempty: true, Items: &Field{Type: "object", Properties: map[string]Field{"op": {Type: "string", Enum: []string{"upload", "delete", "rename"}}, "name": str, "base64": str, "from": str, "to": str}, Required: []string{"op"}}}
			s.Requires = map[string][]string{"image_ops": {"images"}, "images": {"image_ops"}}
			s.Effects = append(s.Effects, "Creator detail exposes no image bytes; image changes remain pending verification after submission")
			s.Example["summary"] = "Updated summary"
		}
		s.DynamicOptions = map[string]string{"categories": "publish modus options categories", "required_tier_id": "publish modus options subscription-tiers"}
	case "project delete", "plugin delete":
		project()
		if strings.HasPrefix(action, "plugin") {
			file()
		}
		s.Properties["confirm"] = Field{Type: "string", Enum: []string{"DELETE"}}
		s.Required = append(s.Required, "confirm")
		s.Example["confirm"] = "DELETE"
	case "plugin upload", "plugin edit":
		project()
		if action == "plugin edit" {
			file()
		}
		s.Properties["file"] = Field{Type: "string", Nonempty: true, Description: "Original addon ZIP, at most 200 MiB. Each addon TOC outside Libs must declare supported ## Interface codes (for example 120105); multiple addons must support the same client versions. Locked, hashed and reused across stages"}
		if action == "plugin upload" {
			s.Required = append(s.Required, "file")
			s.Example["file"] = "Addon.zip"
		}
		s.Properties["version"] = Field{Type: "string", Nonempty: true, MaxLength: 120}
		s.Properties["type"] = Field{Type: "string", Nonempty: true, MaxLength: 40}
		s.Properties["changelog"] = Field{Type: "string", Nullable: true, MaxLength: 10000}
		s.Properties["supported_game_versions"] = Field{Type: "array", Nonempty: true, Items: &Field{Type: "object", Properties: map[string]Field{"gameVersion": str, "server": str}, Required: []string{"gameVersion", "server"}}}
		s.DynamicOptions = map[string]string{"supported_game_versions": "publish modus options game-versions"}
	default:
		return InputSpec{}, false
	}
	return s, true
}
