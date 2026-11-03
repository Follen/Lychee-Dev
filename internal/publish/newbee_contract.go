package publish

import "strings"

type newBeeOfficialCommand struct {
	args          []string
	positional    []string
	flags         map[string]string
	spec          InputSpec
	write, remote bool
}

var newBeeOfficialCommands = buildNewBeeOfficialCommands()

func buildNewBeeOfficialCommands() map[string]newBeeOfficialCommand {
	all := map[string]newBeeOfficialCommand{}
	text := Field{Type: "string"}
	requiredText := Field{Type: "string", Nonempty: true}
	number := Field{Type: "integer", Minimum: Int(1)}
	boolean := Field{Type: "boolean"}
	add := func(action, cmd string, pos, required []string, fields map[string]Field, write, remote bool) {
		if fields == nil {
			fields = map[string]Field{}
		}
		fields["cli_path"] = Field{Type: "string", Nonempty: true, Description: "Explicit official ncc executable or ncc.cjs; otherwise discover installed CLI"}
		flags := map[string]string{}
		for key := range fields {
			if key != "cli_path" {
				flags[key] = "--" + strings.ReplaceAll(key, "_", "-")
			}
		}
		for _, p := range pos {
			delete(flags, p)
		}
		effects := []string{"read selected platform resource"}
		if write {
			effects = []string{"execute official CLI mutation; preserve receipt and never automatically replay"}
		}
		if !remote {
			effects = []string{"local official CLI operation"}
		}
		all[action] = newBeeOfficialCommand{args: strings.Fields(cmd), positional: pos, flags: flags, write: write, remote: remote, spec: InputSpec{Properties: fields, Required: required, Example: map[string]any{}, Effects: effects}}
	}
	add("session doctor", "", nil, nil, nil, false, false)
	add("session auth", "whoami", nil, nil, nil, false, true)
	for _, resource := range []struct{ name, native string }{{"plugin", "addons"}, {"wa", "wa"}, {"config", "uipack"}} {
		prefix := "wow " + resource.native + " "
		add(resource.name+" list", prefix+"list", []string{"page"}, nil, map[string]Field{"page": number, "keyword": text, "pagesize": number}, false, true)
		// ncc wa/uipack use a fixed page size; their CLI does not accept --pagesize.
		if resource.name != "plugin" {
			c := all[resource.name+" list"]
			delete(c.spec.Properties, "pagesize")
			delete(c.flags, "pagesize")
			all[resource.name+" list"] = c
		}
		add(resource.name+" get", prefix+"info", []string{"id"}, []string{"id"}, map[string]Field{"id": number}, false, true)
		add(resource.name+" init", prefix+"init", []string{"directory"}, []string{"id", "directory"}, map[string]Field{"id": number, "directory": requiredText}, true, true)
		if resource.name != "plugin" {
			add(resource.name+" pull", prefix+"pull", []string{"id"}, []string{"id", "out"}, map[string]Field{"id": number, "out": requiredText}, true, true)
		}
	}
	add("plugin categories", "wow addons categories", nil, nil, map[string]Field{"refresh_config": boolean}, false, true)
	add("wa categories", "wow wa categories", nil, []string{"game_version"}, map[string]Field{"game_version": requiredText, "refresh_config": boolean}, false, true)
	add("plugin versions", "wow addons versions", []string{"page"}, []string{"id"}, map[string]Field{"id": number, "page": number, "pagesize": number}, false, true)
	add("plugin changelog", "wow addons changelog", []string{"id"}, []string{"id"}, map[string]Field{"id": number}, false, true)
	add("plugin changelog-edit", "wow addons changelog", []string{"id"}, []string{"id", "set"}, map[string]Field{"id": number, "set": text}, true, true)
	add("plugin pack", "wow addons pack", []string{"directory"}, []string{"directory", "out"}, map[string]Field{"directory": requiredText, "version": text, "out": requiredText}, true, false)
	add("plugin sync", "wow addons sync", []string{"directory"}, []string{"directory", "yes"}, map[string]Field{"directory": requiredText, "yes": boolean}, true, true)
	add("plugin upload", "wow addons push", []string{"directory"}, []string{"directory"}, map[string]Field{"directory": requiredText, "version": text, "changelog": text, "game_versions": text}, true, true)
	add("wa upload", "wow wa push", []string{"id"}, []string{"id", "file", "log"}, map[string]Field{"id": number, "file": requiredText, "log": requiredText, "version": text}, true, true)
	add("config edit", "wow uipack push", []string{"id"}, []string{"id"}, map[string]Field{"id": number, "file": requiredText, "title": text, "intro": text}, true, true)
	for _, r := range []string{"plugin", "wa"} {
		native := "addons"
		if r == "wa" {
			native = "wa"
		}
		fields := map[string]Field{"name": text, "description": text, "intro": text, "categories": text, "private": boolean, "public": boolean, "subscribe_level": Field{Type: "integer", Minimum: Int(0)}, "link_channel": boolean}
		if r == "plugin" {
			fields["logo"] = text
			fields["screenshots"] = text
		} else {
			fields["thumbnail"] = text
			fields["images"] = text
			fields["price"] = Field{Type: "number", Minimum: Int(0)}
		}
		edit := map[string]Field{"id": number}
		for k, v := range fields {
			edit[k] = v
		}
		add(r+" edit", "wow "+native+" edit", []string{"id"}, []string{"id"}, edit, true, true)
		delete(fields, "public")
		fields["origin"] = Field{Type: "string", Enum: []string{"original", "repost", "localized"}}
		required := []string{"name", "description", "categories"}
		action := "create"
		if r == "plugin" {
			required = append(required, "logo", "screenshots")
			fields["home_page"] = text
			fields["home_page_link"] = text
		} else {
			action = "publish"
			required = append(required, "wa_str", "game_version", "thumbnail")
			fields["wa_str"] = requiredText
			fields["game_version"] = requiredText
			fields["changelog"] = text
			fields["time_range"] = text
		}
		add(r+" create", "wow "+native+" "+action, nil, required, fields, true, true)
	}
	add("config create", "wow uipack publish", nil, []string{"backup", "title", "content", "images"}, map[string]Field{"backup": number, "title": requiredText, "content": text, "images": text, "role": text, "intro": text, "private": boolean, "price": Field{Type: "number", Minimum: Int(0)}, "subscribe_level": Field{Type: "integer", Minimum: Int(0)}, "time_range": text, "origin": Field{Type: "string", Enum: []string{"original", "repost", "localized"}}, "link_channel": boolean}, true, true)
	add("media upload", "wow media upload", []string{"file"}, []string{"file", "for"}, map[string]Field{"file": requiredText, "for": Field{Type: "string", Enum: []string{"uipack", "addons", "wa"}}}, true, true)
	add("backup list", "wow cloudbackup list", nil, nil, nil, false, true)
	add("backup get", "wow cloudbackup info", []string{"id"}, []string{"id"}, map[string]Field{"id": number}, false, true)
	add("reference get", "wow ref", []string{"resource", "id"}, []string{"resource", "id"}, map[string]Field{"resource": Field{Type: "string", Enum: []string{"addons", "wa", "uipack"}}, "id": number, "link": boolean, "game_version": text}, false, true)
	for name, c := range all {
		if _, ok := c.spec.Properties["private"]; ok {
			if _, ok = c.spec.Properties["public"]; ok {
				c.spec.MutuallyExclusive = [][]string{{"private", "public"}}
			}
		}
		all[name] = c
	}
	return all
}
