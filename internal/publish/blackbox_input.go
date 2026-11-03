package publish

import "strings"

func BlackboxActions() []string {
	return []string{"session doctor", "session login-start", "session login-submit", "session login-status", "session renew", "session forget", "options game-types", "plugin list", "plugin get", "plugin versions", "plugin edit", "plugin update", "version edit", "version delete", "config list", "config get", "config create", "config edit", "config delete", "wa list", "wa get", "wa categories", "wa create", "wa edit", "wa delete", "backup scan", "backup list", "backup get", "backup generate", "backup create", "backup upload", "backup rename", "backup delete-preview", "backup delete", "operation status", "operation resume", "operation verify"}
}
func BlackboxMutates(action string) bool {
	switch action {
	case "session login-start", "session login-submit", "session renew", "session forget", "plugin edit", "plugin update", "version edit", "version delete", "config create", "config edit", "config delete", "wa create", "wa edit", "wa delete", "backup generate", "backup create", "backup upload", "backup rename", "backup delete", "operation resume":
		return true
	}
	return false
}
func bbConfigField(create bool) Field {
	text := Field{Type: "string"}
	str := Field{Type: "string", Nonempty: true}
	file := Field{Type: "object", Properties: map[string]Field{"Name": str}, Required: []string{"Name"}}
	container := Field{Type: "object", Properties: map[string]Field{"Files": {Type: "array", Items: &file}}, Required: []string{"Files"}}
	role := Field{Type: "object", Properties: map[string]Field{"Realm": str, "Player": str}, Required: []string{"Realm", "Player"}}
	account := Field{Type: "object", Properties: map[string]Field{"Name": str, "RealmCharacters": {Type: "array", Nonempty: true, Items: &role}}, Required: []string{"Name", "RealmCharacters"}}
	known := Field{Type: "object", Properties: map[string]Field{"ID": str}, Required: []string{"ID"}}
	properties := map[string]Field{"Title": str, "Article": text, "UpdateLog": text, "ChatInviteCode": text, "KnownAddons": {Type: "array", Items: &known}, "WTFList": {Type: "array", Items: &account}, "ThumbList": {Type: "array", Items: &str}, "SubscribeLevel": {Type: "integer", Minimum: Int(0)}, "UnknownAddons": container, "Materials": container, "Fonts": container}
	properties["Title"] = Field{Type: "string", Nonempty: true, Description: "Official weighted length at most 70: Latin-1 characters count 1, other BMP characters 2, supplementary characters 4"}
	for _, k := range []string{"Open", "ClearKnownAddons", "ClearUnknownAddons", "ClearMaterials", "ClearFonts", "NeedsPush"} {
		properties[k] = Field{Type: "boolean"}
	}
	f := Field{Type: "object", Properties: properties, Description: "Explicit editor changes. Selection objects contain identifiers only; original backup metadata is retained by the provider."}
	if create {
		f.Required = []string{"Title", "Article", "Open", "ThumbList", "SubscribeLevel", "KnownAddons", "UnknownAddons", "WTFList", "Materials", "Fonts"}
	}
	return f
}
func bbWAField(create bool) Field {
	str := Field{Type: "string", Nonempty: true}
	screen := Field{Type: "object", Properties: map[string]Field{"src": str, "thumb": str}, Required: []string{"src", "thumb"}}
	f := Field{Type: "object", Properties: map[string]Field{"categories": {Type: "array", Nonempty: true, Items: &str}, "strappid": str, "open": {Type: "boolean", Description: "Opaque native option; this is not an access-control or privacy switch"}, "name": str, "descriptionHeybox": {Type: "string"}, "updateLog": {Type: "string"}, "screens": {Type: "array", Nonempty: true, Items: &screen}, "encoded": str, "subscribeLevel": {Type: "integer", Minimum: Int(0)}}, Required: []string{"strappid", "open", "subscribeLevel"}}
	if create {
		f.Required = append(f.Required, "categories", "name", "screens", "encoded")
	}
	return f
}
func BlackboxInput(action string) (InputSpec, bool) {
	if action == "backup create" {
		s, _ := BlackboxInput("backup generate")
		fields := append([]string(nil), s.Required...)
		s.Required = nil
		s.Properties["file"] = Field{Type: "string", Nonempty: true}
		s.ExactlyOne = [][]string{{"file", "game_dir"}}
		s.Requires = map[string][]string{"game_dir": fields}
		for key := range s.Properties {
			if key != "file" {
				s.MutuallyExclusive = append(s.MutuallyExclusive, []string{"file", key})
			}
		}
		s.Effects = []string{"generates an explicitly selected local package or uses the selected package file, then persists and uploads the original artifacts once"}
		return s, true
	}
	str := Field{Type: "string", Nonempty: true}
	text := Field{Type: "string"}
	id := Field{Type: "integer", Minimum: Int(1)}
	stringsField := Field{Type: "array", Items: &str}
	s := InputSpec{Properties: map[string]Field{}, Required: []string{}, Example: map[string]any{}, Effects: []string{"uses the explicitly selected private Heybox session; restores the same account once; no implicit login"}}
	add := func(name string, f Field, v any) {
		s.Properties[name] = f
		s.Required = append(s.Required, name)
		s.Example[name] = v
	}
	switch action {
	case "session doctor":
		s.Effects = []string{"reads private encrypted session metadata; no network"}
	case "session renew":
		s.Effects = []string{"restores the original account; does not send SMS or initiate OAuth"}
	case "session forget":
		add("confirm_forget", Field{Type: "boolean", Description: "Must be true to forget the selected local session"}, true)
		s.Effects = []string{"deletes only selected local encrypted session and pending login; no remote logout"}
	case "session login-start":
		add("method", Field{Type: "string", Enum: []string{"sms", "wechat"}}, "sms")
		s.Properties["phone"] = Field{Type: "string", Nonempty: true, MaxLength: 11, Description: "Required for SMS: mainland China mobile number matching 1[3-9][0-9]{9}; forbidden for WeChat"}
		s.Example["phone"] = "13800000000"
		s.Effects = []string{"starts exactly one explicitly selected SMS or WeChat login; persists the original login before network; OTP is never stored"}
	case "session login-submit":
		add("login_id", Field{Type: "string", Nonempty: true, Description: "Original login-start ID: exactly 32 lowercase hexadecimal characters"}, "0123456789abcdef0123456789abcdef")
		add("code", Field{Type: "string", Nonempty: true, MaxLength: 8, Description: "SMS code containing 4 to 8 digits"}, "123456")
		s.Effects = []string{"submits original SMS code once; unknown input is never replayed"}
	case "session login-status":
		add("login_id", Field{Type: "string", Nonempty: true, Description: "Original login-start ID: exactly 32 lowercase hexadecimal characters"}, "0123456789abcdef0123456789abcdef")
		s.Effects = []string{"polls original login; continues confirmed OAuth or account readback without resending SMS or OTP"}
	case "options game-types", "plugin list", "wa categories", "backup list":
	case "plugin get", "plugin versions":
		add("module_id", id, 123)
	case "plugin edit":
		add("id", id, 123)
		for _, k := range []string{"name", "logo_url", "desc", "official", "official_url"} {
			s.Properties[k] = text
		}
		s.Properties["category_ids"] = Field{Type: "array", Items: &id}
		s.Properties["core_folders"] = stringsField
		s.Properties["type"] = Field{Type: "integer", Minimum: Int(1), Maximum: Int(9), Description: "Only 1 or 9 is accepted; omit to preserve the current official plugin type"}
		s.Example["name"] = "Example"
	case "plugin update", "version edit":
		add("module_id", id, 123)
		add("name", str, "1.0")
		add("type", Field{Type: "integer", Minimum: Int(1), Maximum: Int(3)}, 1)
		add("game_versions", Field{Type: "array", Nonempty: true, Items: &str}, []any{"12.1.5"})
		s.Properties["file"] = str
		if action == "plugin update" {
			s.Required = append(s.Required, "file")
			s.Example["file"] = "Addon.zip"
		} else {
			add("version_id", id, 456)
		}
		s.DynamicOptions = map[string]string{"game_versions": "publish blackbox plugin versions"}
	case "version delete":
		add("module_id", id, 123)
		add("version_id", id, 456)
	case "config list", "wa list":
		add("strappid", str, "wowretail")
	case "config get":
		add("share_id", str, "share-id")
	case "config create", "config edit":
		add("configuration", bbConfigField(action == "config create"), map[string]any{"Title": "Example"})
		s.Properties["backup_id"] = str
		if action == "config create" {
			add("backup_id", str, "backup-id")
			s.Example["configuration"] = map[string]any{"Title": "Example", "Article": "<p>Example</p>", "Open": false, "ThumbList": []any{}, "SubscribeLevel": 0, "KnownAddons": []any{map[string]any{"ID": "1"}}, "WTFList": []any{}, "UnknownAddons": map[string]any{"Files": []any{}}, "Materials": map[string]any{"Files": []any{}}, "Fonts": map[string]any{"Files": []any{}}}
		} else {
			add("share_id", str, "share-id")
		}
		s.DynamicOptions = map[string]string{"backup_id": "publish blackbox backup list", "configuration": "publish blackbox backup get"}
	case "config delete":
		add("share_id", str, "share-id")
		add("confirm_delete", Field{Type: "boolean"}, true)
	case "wa get":
		add("slug", str, "example")
		add("strappid", str, "wowretail")
	case "wa create", "wa edit":
		add("scope", Field{Type: "string", Enum: []string{"public"}}, "public")
		add("configuration", bbWAField(action == "wa create"), map[string]any{"strappid": "wowretail", "open": true, "subscribeLevel": 0})
		if action == "wa edit" {
			add("slug", str, "example")
		} else {
			s.Example["configuration"] = map[string]any{"strappid": "wowretail", "open": true, "subscribeLevel": 0, "categories": []any{"example"}, "name": "Example", "screens": []any{map[string]any{"src": "https://example.com/image.png", "thumb": "https://example.com/image.png"}}, "encoded": "!WA:2!example"}
		}
		s.DynamicOptions = map[string]string{"configuration.categories": "publish blackbox wa categories", "configuration.strappid": "publish blackbox options game-types"}
	case "wa delete":
		add("slug", str, "example")
		add("strappid", str, "wowretail")
		add("confirm_delete", Field{Type: "boolean"}, true)
	case "backup scan":
		add("game_dir", str, "D:/Games/WoW/_retail_")
		add("strappid", str, "wowretail")
		s.Effects = []string{"lists local backup choices; no authentication or network"}
	case "backup get":
		add("backup_id", str, "backup-id")
	case "backup generate":
		add("game_dir", str, "D:/Games/WoW/_retail_")
		add("output_dir", str, "D:/Backups/selected")
		add("strappid", str, "wowretail")
		for _, key := range []string{"unknown_addons", "materials", "fonts"} {
			add(key, stringsField, []any{})
		}
		role := Field{Type: "object", Properties: map[string]Field{"realm": str, "player": str}, Required: []string{"realm", "player"}}
		account := Field{Type: "object", Properties: map[string]Field{"name": str, "characters": {Type: "array", Nonempty: true, Items: &role}}, Required: []string{"name", "characters"}}
		add("wtf_accounts", Field{Type: "array", Items: &account}, []any{})
		known := Field{Type: "object", Properties: map[string]Field{"ID": str, "Name": str, "VersionID": str, "Strategy": {Type: "integer", Minimum: Int(1), Maximum: Int(5)}}, Required: []string{"ID", "Name", "VersionID", "Strategy"}}
		s.Properties["known_addons"] = Field{Type: "array", Items: &known}
		s.Properties["strategies_json"] = Field{Type: "string", Description: "Original official strategy-object array as JSON; retained without guessing vendor fields"}
		s.Example["unknown_addons"] = []any{"Example"}
		s.Effects = []string{"packages only explicit local selections into a new output directory; no credentials or network"}
	case "backup upload":
		add("file", str, "D:/Backups/selected/backup.json")
	case "backup rename":
		add("backup_id", str, "backup-id")
		add("name", Field{Type: "string", Nonempty: true, MaxLength: 200}, "Selected backup")
	case "backup delete-preview":
		s.Properties["backup_ids"] = Field{Type: "array", Nonempty: true, Items: &str}
		for _, k := range []string{"name_contains", "name_equals"} {
			s.Properties[k] = str
		}
		for _, k := range []string{"created_before", "created_after"} {
			s.Properties[k] = Field{Type: "integer", Minimum: Int(0)}
		}
		s.Example["backup_ids"] = []any{"backup-id"}
		s.Effects = []string{"reads complete owned shares and backup references; provide distinct backup IDs or one or more name/date filters, never both; filters only preview, never delete"}
	case "backup delete":
		add("backup_ids", Field{Type: "array", Nonempty: true, Items: &str}, []any{"backup-id"})
		add("confirm_delete", Field{Type: "boolean"}, true)
	case "operation status", "operation resume", "operation verify":
		add("request", Field{Type: "string", Nonempty: true, MaxLength: 128}, "release-1")
		s.Effects = []string{"continues or verifies original saved operation; uncertain writes are never repeated"}
		if action == "operation status" {
			s.Effects = []string{"reads encrypted original operation metadata; no network"}
		}
	default:
		return InputSpec{}, false
	}
	if BlackboxMutates(action) && !strings.HasPrefix(action, "session ") && action != "backup generate" {
		s.Effects = append(s.Effects, "persists the original target and artifact before remote changes; original request is required for continuation")
	}
	return s, true
}
