package publish

import "strings"

type newBeeCreatorCommand struct {
	spec     InputSpec
	endpoint string
	mapping  map[string]string
	defaults map[string]any
	write    bool
}

var newBeeCreatorCommands = buildNewBeeCreatorCommands()

func buildNewBeeCreatorCommands() map[string]newBeeCreatorCommand {
	all := map[string]newBeeCreatorCommand{}
	id := Field{Type: "integer", Minimum: Int(1)}
	text := Field{Type: "string"}
	nonempty := Field{Type: "string", Nonempty: true}
	positive := Field{Type: "integer", Minimum: Int(0)}
	boolean := Field{Type: "boolean"}
	texts := Field{Type: "array", Items: &text}
	ids := Field{Type: "array", Items: &id}
	add := func(action, path string, required []string, fields map[string]Field, mapping map[string]string, defaults map[string]any, write bool) {
		if fields == nil {
			fields = map[string]Field{}
		}
		if mapping == nil {
			mapping = map[string]string{}
			for name := range fields {
				mapping[name] = name
			}
		}
		effects := []string{"read selected Creator resource; preserve server pagination"}
		if write {
			effects = []string{"submit selected Creator mutation; verify resource readback; retain original operation"}
		}
		all[action] = newBeeCreatorCommand{spec: InputSpec{Properties: fields, Required: required, Example: map[string]any{}, Effects: effects}, endpoint: path, mapping: mapping, defaults: defaults, write: write}
	}
	add("session doctor", "", nil, nil, nil, nil, false)
	add("plugin categories", "metadata:mod_category", nil, nil, nil, nil, false)
	add("plugin game-versions", "metadata:game_version", nil, nil, nil, nil, false)
	add("options content-origins", "/v3/sys/content_origin_list", nil, nil, nil, nil, false)
	add("options subscribe-plans", "/creator/author_subscribe/plan_level_preset", nil, nil, nil, nil, false)
	add("options time-ranges", "/cloudsaveserver/GameCloudSavePublish/GetTimeRangeList", nil, nil, nil, nil, false)
	add("wa categories", "/creator/wow/wa/category", []string{"game_version_id"}, map[string]Field{"game_version_id": id}, map[string]string{"game_version_id": "game_version"}, nil, false)
	add("wa attachment-paths", "/creator/wow/wa/attachment_install_path_list", nil, nil, nil, nil, false)
	for _, r := range []struct {
		name, path, list, detail string
		co, ref                  int
	}{{"plugin", "mod", "publish_list", "publish_detail", 1, 1}, {"config", "share_config", "publish_list", "details_aps", 4, 3}, {"wa", "wa", "mtg_uc_publish_list", "detail_aps", 3, 2}} {
		prefix := "/creator/wow/" + r.path + "/"
		fields := map[string]Field{"keyword": text, "page_size": Field{Type: "integer", Minimum: Int(1), Maximum: Int(100)}}
		mapping := map[string]string{"keyword": "keyword", "page_size": "pagesize"}
		defaults := map[string]any{"keyword": "", "pagesize": 100, "game_version_id": 0}
		if r.name == "plugin" {
			fields["page"] = id
			mapping["page"] = "pagenum"
			defaults["pagenum"] = 1
			defaults["sort_by"] = "t_last_update"
			defaults["sort_order"] = "DESC"
		} else {
			fields["offset"] = positive
			mapping["offset"] = "offset"
			defaults["offset"] = 0
			defaults["sort"] = 3
		}
		add(r.name+" list", prefix+r.list, nil, fields, mapping, defaults, false)
		add(r.name+" get", prefix+r.detail, []string{"id"}, map[string]Field{"id": id}, nil, nil, false)
		deletePath := prefix + "delete"
		if r.name == "plugin" {
			deletePath = prefix + "remove"
		}
		add(r.name+" delete", deletePath, []string{"id", "confirm_delete"}, map[string]Field{"id": id, "confirm_delete": boolean}, map[string]string{"id": "id"}, nil, true)
		add(r.name+" media-upload", prefix+"upload_media", []string{"file"}, map[string]Field{"file": nonempty}, nil, nil, true)
		if r.name == "config" {
			c := all[r.name+" media-upload"]
			c.endpoint = prefix + "upload"
			all[r.name+" media-upload"] = c
		}
		for _, relation := range []struct {
			name, path string
			kind       int
		}{{"co-author", "co_author", r.co}, {"reference", "content_reference", r.ref}} {
			add(r.name+" "+relation.name+"-list", "/creator/"+relation.path+"/list", []string{"id"}, map[string]Field{"id": id}, map[string]string{"id": "content_id"}, map[string]any{"content_type": relation.kind}, false)
			endpoint := "/creator/" + relation.path + "/search"
			if relation.name == "co-author" {
				endpoint += "_user"
			}
			defaults := map[string]any{}
			if relation.name == "reference" {
				defaults["limit"] = 20
				defaults["target_types"] = []int{relation.kind}
			}
			add(r.name+" "+relation.name+"-search", endpoint, []string{"keyword"}, map[string]Field{"keyword": nonempty}, nil, defaults, false)
			row := Field{Type: "object", Properties: map[string]Field{"user_id": id, "share_percent": {Type: "number", Minimum: Int(0), Maximum: Int(100)}}, Required: []string{"user_id", "share_percent"}}
			listField := "co_authors"
			idField := "content_id"
			typeField := "content_type"
			if relation.name == "reference" {
				row = Field{Type: "object", Properties: map[string]Field{"type": {Type: "integer", Minimum: Int(1), Maximum: Int(3)}, "id": id}, Required: []string{"type", "id"}}
				listField = "references"
				idField = "source_id"
				typeField = "source_type"
			}
			add(r.name+" "+relation.name+"-set", "/creator/"+relation.path+"/set", []string{"id", listField}, map[string]Field{"id": id, listField: {Type: "array", Items: &row}}, map[string]string{"id": idField, listField: listField}, map[string]any{typeField: relation.kind}, true)
		}
	}
	add("plugin versions", "/creator/wow/mod_file/mod_file_list", []string{"id"}, map[string]Field{"id": id, "page": id, "page_size": {Type: "integer", Minimum: Int(1), Maximum: Int(100)}}, map[string]string{"id": "mod_id", "page": "pagenum", "page_size": "pagesize"}, map[string]any{"game_version_id": 0, "pagenum": 1, "pagesize": 100}, false)
	for _, r := range []string{"plugin", "wa"} {
		endpoint := "/creator/wow/mod_file/changelog_list"
		idKey := "mod_id"
		if r == "wa" {
			endpoint = "/creator/wow/wa_log/list"
			idKey = "wa_id"
		}
		add(r+" changelog-list", endpoint, []string{"id"}, map[string]Field{"id": id, "page": id, "page_size": {Type: "integer", Minimum: Int(1), Maximum: Int(100)}}, map[string]string{"id": idKey, "page": "pagenum", "page_size": "pagesize"}, map[string]any{"pagenum": 1, "pagesize": 100}, false)
	}
	add("plugin changelog-get", "/creator/wow/mod_file/get_changelog", []string{"id"}, map[string]Field{"id": id}, map[string]string{"id": "file_id"}, nil, false)
	add("plugin changelog-edit", "/creator/wow/mod_file/edit_changelog", []string{"id", "changelog"}, map[string]Field{"id": id, "changelog": text}, map[string]string{"id": "file_id", "changelog": "changelog"}, nil, true)
	add("wa changelog-latest", "/creator/wow/wa_log/latest_str_info", []string{"id"}, map[string]Field{"id": id}, map[string]string{"id": "wa_id"}, nil, false)
	add("wa changelog-edit", "/creator/wow/wa_log/edit", []string{"id", "wa_id", "wa_log"}, map[string]Field{"id": id, "wa_id": id, "wa_log": text}, map[string]string{"id": "wa_log_id", "wa_log": "content"}, nil, true)
	add("wa share-code-set", "/bannerserver/ShareCode/Set", []string{"id"}, map[string]Field{"id": id}, map[string]string{"id": "moduleId"}, map[string]any{"gameId": 1, "moduleType": 3}, true)
	add("wa attachment-upload", "", []string{"file"}, map[string]Field{"file": nonempty}, nil, nil, true)
	plugin := map[string]Field{"name": nonempty, "description": text, "intro": text, "mod_categories": ids, "content_origin": positive, "content_format": positive, "logo": nonempty, "screenshots": texts, "subscribe_plan_level": positive, "link_to_channel": boolean, "public": boolean}
	wa := map[string]Field{"game_version_id": id, "name": nonempty, "intro": text, "description": text, "content_format": positive, "thumbnail": nonempty, "images": texts, "category_id_list": ids, "content_origin": positive, "subscribe_plan_level": positive, "price": positive, "time_range": text, "public": boolean, "link_to_channel": boolean}
	attachment := Field{Type: "object", Properties: map[string]Field{"name": nonempty, "install_type": positive, "install_path": nonempty, "value": nonempty, "is_compressed": boolean, "timestamp": positive}, Required: []string{"name", "install_type", "install_path", "value", "is_compressed"}}
	wa["attachments"] = Field{Type: "array", Items: &attachment}
	linked := Field{Type: "object", Properties: map[string]Field{"mod_id": id, "mod_file_id": id, "mod_name": text, "mod_version": text, "display_name": text, "update_type": id}, Required: []string{"mod_id"}}
	config := map[string]Field{"cloud_id": id, "title": nonempty, "content": text, "content_format": positive, "intro": text, "picture_urls": texts, "content_origin": positive, "public": boolean, "link_to_channel": boolean, "subscribe_plan_level": positive, "price": positive, "time_range": text, "linked_mods": {Type: "array", Items: &linked}, "ignored_unknown_mods": texts, "ignored_materials": texts, "ignored_fronts": texts, "roleid": text}
	for _, r := range []struct {
		name, path, create, edit string
		fields                   map[string]Field
		required                 []string
	}{{"plugin", "mod", "create", "edit", plugin, []string{"name", "description", "intro", "mod_categories", "content_origin", "content_format", "logo", "screenshots", "public"}}, {"config", "share_config", "release", "update", config, []string{"cloud_id", "title", "content", "content_format", "picture_urls", "content_origin", "public", "linked_mods", "ignored_unknown_mods", "ignored_materials", "ignored_fronts", "roleid"}}, {"wa", "wa", "publish", "update", wa, []string{"game_version_id", "name", "content_format", "thumbnail", "category_id_list", "content_origin", "public", "wa_str", "wa_log", "string_mode"}}} {
		create := map[string]Field{}
		edit := map[string]Field{"id": id}
		for k, v := range r.fields {
			create[k] = v
			edit[k] = v
		}
		if r.name == "wa" {
			create["wa_str"] = nonempty
			create["wa_log"] = text
			create["wa_str_titles"] = texts
			create["string_mode"] = positive
		}
		add(r.name+" create", "/creator/wow/"+r.path+"/"+r.create, r.required, create, nil, nil, true)
		add(r.name+" edit", "/creator/wow/"+r.path+"/"+r.edit, []string{"id"}, edit, nil, nil, true)
	}
	add("plugin upload", "/creator/wow/mod_file/upload_mod_file", []string{"id", "file", "version", "game_version_list"}, map[string]Field{"id": id, "file": nonempty, "version": nonempty, "game_version_list": {Type: "array", Items: &nonempty, Nonempty: true}, "changelog": text, "link_to_channel": boolean}, nil, nil, true)
	add("wa upload", "/creator/wow/wa/update_wa_str", []string{"id", "version", "wa_str", "wa_log"}, map[string]Field{"id": id, "version": nonempty, "wa_str": nonempty, "wa_log": text, "wa_str_titles": texts, "link_to_channel": boolean}, nil, nil, true)
	add("backup list", "/v3/share/list", nil, nil, nil, nil, false)
	add("backup get", "/v3/share/list", []string{"id"}, map[string]Field{"id": id}, nil, nil, false)
	add("backup rename", "/share/update", []string{"id", "name"}, map[string]Field{"id": id, "name": nonempty}, map[string]string{"id": "t_id", "name": "t_name"}, nil, true)
	for key, c := range all {
		if strings.Contains(key, "create") || strings.Contains(key, "edit") {
			c.spec.DynamicOptions = map[string]string{"content_origin": "publish newbee creator options content-origins", "subscribe_plan_level": "publish newbee creator options subscribe-plans", "time_range": "publish newbee creator options time-ranges"}
			all[key] = c
		}
	}
	addNewBeeBackupCommands(all)
	return all
}
