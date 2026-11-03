package publish

import "strings"

var ddLifeTypes = []string{"seven_day", "fourteen_day", "thirty_day", "sixty_day", "ninety_day", "forever"}

func ddFields() map[string]Field {
	text := Field{Type: "string", MaxLength: 1 << 20}
	nonempty := Field{Type: "string", Nonempty: true, MaxLength: 4096}
	id := Field{Type: "integer", Minimum: Int(1)}
	strs := Field{Type: "array", Items: &text, MaxLength: 1000}
	ids := Field{Type: "array", Items: &id, MaxLength: 1000}
	f := map[string]Field{}
	for _, key := range strings.Fields("name title description desc brief_desc html_desc update_desc content version game_version logo detail_url file_path file_install_path room_id channel_id channel_type creation_statement backup_sn sn keyword") {
		f[key] = text
	}
	for _, key := range strings.Fields("file logo_file wa_file installation session request output root") {
		f[key] = nonempty
	}
	for _, key := range strings.Fields("game_type primary_category_id") {
		f[key] = id
	}
	for _, key := range strings.Fields("game_versions detail_imgs display_imgs detail_img_files display_img_files known_addon_ids unknown_addon_ids material_names font_names wtf_role_ids known_wa_ids unknown_wa_ids known_addon_update_ids unknown_addon_update_ids material_update_names font_update_names known_wa_update_ids unknown_wa_update_ids") {
		f[key] = strs
	}
	for _, key := range strings.Fields("second_category_ids vip_levels") {
		f[key] = ids
	}
	for _, key := range strings.Fields("need_buy jump_room sync_room with_associate need_anchor_vip with_file") {
		f[key] = Field{Type: "boolean"}
	}
	f["scope"] = Field{Type: "string", Enum: []string{"public", "private"}}
	f["release_type"] = Field{Type: "string", Enum: []string{"release", "beta", "alpha"}}
	f["share_code_life_type"] = Field{Type: "string", Enum: ddLifeTypes}
	f["buy_life_type"] = f["share_code_life_type"]
	f["price_fen"] = Field{Type: "integer", Minimum: Int(0), Maximum: Int(20000)}
	f["addon_type"] = Field{Type: "integer", Minimum: Int(0)}
	f["category_ids"] = strs
	assoc := Field{Type: "object", Properties: map[string]Field{"sn": nonempty, "act_type": {Type: "string", Enum: []string{"addon", "share", "wa"}}}, Required: []string{"sn", "act_type"}}
	f["associated_acts"] = Field{Type: "array", Items: &assoc}
	f["known_addon_ids"] = ids
	f["known_addon_update_ids"] = ids
	f["wtf_role_ids"] = Field{Type: "array", Items: &nonempty, MaxLength: 1}
	f["creation_statement"] = Field{Type: "string", Enum: []string{"original", "chinesize", "renovate", "second"}}
	f["retail_ui_config"] = Field{Type: "object", Nullable: true, Properties: map[string]Field{"edit_mode_selectors": {Type: "array", Items: &nonempty, MaxLength: 5}, "default_edit_mode_selector": text, "cool_down_selectors": strs, "enable_dd_setup_wizard": {Type: "boolean"}}}
	f["page"] = Field{Type: "integer", Minimum: Int(1), Maximum: Int(4294967295)}
	f["size"] = Field{Type: "integer", Minimum: Int(1), Maximum: Int(100)}
	descriptions := map[string]string{"session": "Retained DD task ID from session start or original operation status recoveryInput; never implicitly reacquires credentials", "sn": "Opaque DD resource SN (configuration input maps to share_sn on the wire)", "request": "Original durable operation request ID", "installation": "Optional absolute directory of pinned DD 100130 runtime", "file": "Existing local ZIP/image, or backup-bundle.json for backup upload; original bytes are bound", "wa_file": "UTF-8 WeakAuras export file; parsed as data without Lua execution", "game_type": "Positive DD game type from options game-types", "game_versions": "Current platform version strings for the selected game", "primary_category_id": "Current top-level plugin category", "second_category_ids": "Children of the selected primary category", "known_addon_ids": "Integer addon IDs selected from the exact backup", "wtf_role_ids": "At most one backup-bound WTF role selector returned by backup get", "retail_ui_config": "Omit to preserve, null to clear, or provide explicit layout selectors; nested omitted fields are preserved", "backup_sn": "Exact backup from this author's current list; changing it requires complete content reselection", "scope": "Public or private publication scope; free/paid mode remains locked after creation", "price_fen": "Price in fen: zero or 10..20000", "version": "New plugin version must be unused; WA update version must be a greater integer", "associated_acts": "Selected author resources for this game, each with sn and act_type", "output": "Empty directory outside the game root for generated backup artifacts", "root": "Explicit local game directory; backup reads only selected files"}
	for k, description := range descriptions {
		field := f[k]
		field.Description = description
		f[k] = field
	}
	return f
}
func DDInput(action string) (InputSpec, bool) {
	all := ddFields()
	s := InputSpec{Properties: map[string]Field{}, Required: []string{}, Example: map[string]any{}, Effects: []string{"uses the explicitly retained DD task session; no automatic login or write retry"}}
	add := func(keys string) {
		for _, k := range strings.Fields(keys) {
			if f, ok := all[k]; ok {
				s.Properties[k] = f
			}
		}
	}
	require := func(keys string) { add(keys); s.Required = append(s.Required, strings.Fields(keys)...) }
	switch action {
	case "session doctor":
		add("installation")
		s.Effects = []string{"local installation inspection; no SDK load, credentials or network"}
	case "session start":
		add("installation")
		s.Effects = []string{"borrows the selected GUI account or opens one native task login", "retains task-owned runtime; never logs the GUI out"}
	case "session status", "session close":
		require("session")
	case "wa parse":
		require("wa_file")
		s.Effects = []string{"reads WA export data without evaluating Lua"}
	case "options life-types":
		s.Effects = []string{"fixed official-client enum; no session or network"}
	case "options game-types", "plugin categories", "options vip-levels", "options channels", "backup list":
		require("session")
	case "options game-versions", "wa categories", "options associated-acts":
		require("session game_type")
	case "plugin list", "config list", "wa list":
		require("session game_type")
		add("keyword page size")
	case "plugin get", "config get", "wa get", "backup get":
		require("session sn")
	case "plugin versions":
		require("session sn game_type")
		add("page")
	case "plugin delete", "config delete", "wa delete", "backup delete":
		require("session sn")
	case "backup rename":
		require("session sn name")
		s.Properties["name"] = Field{Type: "string", Nonempty: true, MaxLength: 14}
	case "backup scan":
		require("root")
		s.Effects = []string{"reads local backup candidates; no credentials or network"}
	case "backup generate":
		require("root output game_type")
		for _, k := range []string{"addon_names", "unknown_addons", "materials", "fonts"} {
			s.Properties[k] = Field{Type: "array", Items: &Field{Type: "string", Nonempty: true}, MaxLength: 1000}
		}
		s.Properties["include_wtf_root"] = Field{Type: "boolean"}
		role := Field{Type: "object", Properties: map[string]Field{"server": {Type: "string", Nonempty: true}, "name": {Type: "string", Nonempty: true}}, Required: []string{"server", "name"}}
		account := Field{Type: "object", Properties: map[string]Field{"account": {Type: "string", Nonempty: true}, "include_common": {Type: "boolean"}, "roles": {Type: "array", Items: &role}}, Required: []string{"account", "include_common", "roles"}}
		s.Properties["wtf_accounts"] = Field{Type: "array", Items: &account}
		s.Effects = []string{"reads explicitly selected local files; creates selective ZIP components in an empty external output directory"}
	case "backup upload":
		require("session file")
		add("name")
		s.Properties["name"] = Field{Type: "string", Nonempty: true, MaxLength: 14}
	case "backup delete-preview", "backup delete-many":
		require("session")
		s.Properties["backup_ids"] = Field{Type: "array", Nonempty: true, Items: &Field{Type: "string", Nonempty: true}}
		if action == "backup delete-many" {
			s.Required = append(s.Required, "backup_ids", "confirm_delete")
			s.Properties["confirm_delete"] = Field{Type: "boolean"}
		} else {
			for _, key := range []string{"name_contains", "name_equals", "created_before", "created_after"} {
				s.Properties[key] = Field{Type: "string", Nonempty: true}
			}
		}
		s.Example = map[string]any{"session": "task-1", "backup_ids": []any{"backup-1"}}
		if action == "backup delete-many" {
			s.Example["confirm_delete"] = true
		}
	case "operation status", "operation verify", "operation resume":
		require("request")
		if action != "operation status" {
			require("session")
			add("sn")
		} else {
			s.Effects = []string{"reads the original local receipt without credentials or network; returns the saved session in recoveryInput when available"}
		}
	case "plugin upload", "config upload", "wa upload":
		require("session file")
		s.Effects = append(s.Effects, "binds the object receipt to author, content SHA256, kind and byte size; acknowledged objects are reused", "persists object intent before PUT; uncertain object uploads remain pending without replay; no user request operation is created")
		s.Properties["kind"] = Field{Type: "string", Enum: []string{"archive", "image"}}
		if action == "config upload" {
			s.Properties["kind"] = Field{Type: "string", Enum: []string{"image"}}
		}
		require("kind")
	case "plugin create", "plugin update", "plugin edit", "config create", "config update", "config edit", "wa create", "wa update", "wa edit":
		resource, verb, _ := strings.Cut(action, " ")
		require("session")
		if verb != "create" {
			require("sn")
		}
		common := "scope share_code_life_type need_buy price_fen buy_life_type jump_room room_id channel_id channel_type sync_room creation_statement with_associate associated_acts need_anchor_vip vip_levels"
		switch resource {
		case "plugin":
			if verb == "update" {
				require("version")
				add("game_versions detail_url file release_type update_desc")
			} else if verb == "edit" {
				add(common + " description html_desc update_desc logo logo_file")
			} else {
				add(common + " game_type game_versions addon_type name description logo logo_file detail_imgs detail_img_files primary_category_id second_category_ids detail_url file release_type version html_desc update_desc")
				require("game_type name game_versions primary_category_id release_type version description html_desc update_desc creation_statement")
			}
		case "config":
			if verb != "update" {
				add(common + " title brief_desc desc display_imgs display_img_files")
			}
			if verb != "edit" {
				add("backup_sn update_desc known_addon_ids unknown_addon_ids material_names font_names wtf_role_ids known_wa_ids unknown_wa_ids known_addon_update_ids unknown_addon_update_ids material_update_names font_update_names known_wa_update_ids unknown_wa_update_ids retail_ui_config")
			}
			if verb == "create" {
				require("backup_sn title brief_desc desc creation_statement")
			}
		case "wa":
			if verb != "update" {
				add(common + " game_type name game_version brief_desc display_imgs display_img_files category_ids desc")
			}
			if verb != "edit" {
				add("content wa_file update_desc version with_file file file_path file_install_path")
			}
			s.MutuallyExclusive = [][]string{{"content", "wa_file"}}
			if verb == "create" {
				require("game_type name game_version brief_desc desc update_desc creation_statement")
			}
		}
		s.DynamicOptions = map[string]string{"game_type": "publish dd options game-types", "game_versions": "publish dd options game-versions", "associated_acts": "publish dd options associated-acts", "vip_levels": "publish dd options vip-levels", "channel_id": "publish dd options channels"}
	default:
		return InputSpec{}, false
	}
	if ddMutation(action) && action != "plugin upload" && action != "config upload" && action != "wa upload" {
		s.Effects = append(s.Effects, "persists original request before mutation; submitted or uncertain requests are not replayed", "reads back the selected resource; receipts distinguish acknowledgement and verification")
	}
	if action == "options channels" {
		s.Effects = append(s.Effects, "borrowed GUI channel lookup can request one same-Windows-user elevation helper when access is denied; it preserves the selected GUI account")
	}
	for _, k := range s.Required {
		f := s.Properties[k]
		if f.Type == "string" {
			f.Nonempty = true
			s.Properties[k] = f
		}
		switch f.Type {
		case "string":
			s.Example[k] = "example"
		case "integer":
			s.Example[k] = 1
		case "array":
			s.Example[k] = []any{}
		}
	}
	if _, ok := s.Properties["scope"]; ok {
		s.Example["scope"] = "public"
	}
	if _, ok := s.Example["release_type"]; ok {
		s.Example["release_type"] = "release"
	}
	if _, ok := s.Example["kind"]; ok {
		s.Example["kind"] = s.Properties["kind"].Enum[0]
	}
	if _, ok := s.Example["creation_statement"]; ok {
		s.Example["creation_statement"] = "original"
	}
	if action == "backup delete-many" || action == "backup delete-preview" {
		s.Example["backup_ids"] = []any{"backup-1"}
	}
	if action == "plugin create" {
		s.Example["game_versions"] = []any{"12.1.5"}
		s.Example["logo"] = "https://example.com/logo.png"
		s.Example["detail_url"] = "https://example.com/addon.zip"
		s.Example["detail_imgs"] = []any{"https://example.com/preview.png"}
	}
	if action == "config create" || action == "wa create" {
		s.Example["display_imgs"] = []any{"https://example.com/preview.png"}
	}
	if action == "wa create" {
		s.Example["wa_file"] = "Export.txt"
	}
	return s, true
}
func ddMutation(action string) bool {
	_, verb, _ := strings.Cut(action, " ")
	return strings.Contains(" create update edit delete delete-many rename upload resume ", " "+verb+" ")
}
