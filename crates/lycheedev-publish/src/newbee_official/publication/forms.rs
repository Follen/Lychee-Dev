use super::*;

pub(super) fn schema(resource: Resource, create: bool) -> Value {
    let text = json!({"oneOf":[{"type":"string","minLength":1},{"type":"object","properties":{"file":{"type":"string","minLength":1}},"required":["file"],"additionalProperties":false}],"description":"Inline text or a UTF-8 file (at most 32 MiB), held unchanged through the operation. Relative paths resolve from the invocation directory."});
    let media = json!({"type":"string","minLength":1,"description":"Reusable public HTTP(S) URL or local image/MP4 path. Local media is uploaded once and receipts are reused for this author and original bytes."});
    let categories = json!({"type":"array","items":{"type":"integer","minimum":1},"minItems":1,"uniqueItems":true});
    let mut properties = json!({"schema":{"const":format!("lycheedev.publish.v1.newbee.official.{}.{}",resource.key(),if create{"create"}else{"edit"})},"intro":{"type":"string"},"visibility":{"enum":["public","private"],"description":"Explicit publish intent; public content can enter review. API acceptance is separate from public visibility."},"subscribe_level":{"type":"integer","minimum":0},"link_channel":{"type":"boolean"}});
    let mut required = vec!["schema"];
    if !create {
        properties["id"] = json!({"type":"integer","minimum":1});
        required.push("id");
    }
    match resource {
        Resource::Plugin => {
            properties["name"] = json!({"type":"string","minLength":1});
            properties["description"] = text.clone();
            properties["categories"] = categories.clone();
            properties["logo"] = media.clone();
            properties["screenshots"] = json!({"type":"array","items":media});
            if create {
                properties["screenshots"]["minItems"] = json!(1);
                properties["home_page"] = json!({"type":"string"});
                properties["home_page_link"] = json!({"type":"string","format":"uri"});
                required.extend([
                    "name",
                    "description",
                    "categories",
                    "logo",
                    "screenshots",
                    "visibility",
                ]);
            }
        }
        Resource::Wa => {
            properties["name"] = json!({"type":"string","minLength":1});
            properties["description"] = text.clone();
            properties["categories"] = categories;
            properties["categories"]["maxItems"] = json!(5);
            properties["thumbnail"] = media.clone();
            properties["images"] = json!({"type":"array","items":media});
            if create {
                properties["game_version_id"] = json!({"type":"integer","minimum":1});
                properties["wa_str"] = text.clone();
                let mut log = text.clone();
                log["oneOf"][0].as_object_mut().unwrap().remove("minLength");
                properties["wa_log"] = log;
                required.extend([
                    "name",
                    "description",
                    "game_version_id",
                    "wa_str",
                    "categories",
                    "thumbnail",
                    "visibility",
                ]);
            }
        }
        Resource::Config => {
            properties["backup_id"] = json!({"type":"integer","minimum":1});
            properties["role_id"] = json!({"type":"integer","minimum":1,"description":"Select an exact role ID from the current backup; required when it contains WTF roles, omitted when it has none."});
            properties["title"] = json!({"type":"string","minLength":1});
            properties["content"] = text;
            properties["images"] = json!({"type":"array","items":media});
            required.extend(["backup_id", "title", "content", "visibility"]);
        }
    }
    if resource != Resource::Plugin {
        properties["price_yuan"] = json!({"type":"string","pattern":"^[0-9]+(?:\\.[0-9]+)?$","description":"Nonnegative decimal yuan, rounded half up to integer fen without floating point. 0 removes single-item payment."});
        if create {
            properties["time_range"] = json!({"type":"string"});
        }
    }
    if create {
        properties["origin"] =
            json!({"enum":["original","repost","localized"],"default":"original"});
    }
    let fields = properties
        .as_object()
        .unwrap()
        .keys()
        .filter(|key| !matches!(key.as_str(), "schema" | "id"))
        .map(|key| json!({"required":[key]}))
        .collect::<Vec<_>>();
    let mut result = json!({"$schema":"https://json-schema.org/draft/2020-12/schema","type":"object","properties":properties,"required":required,"additionalProperties":false});
    if !create {
        result["anyOf"] = json!(fields);
    }
    result
}

impl Official {
    pub(super) fn publication_body(
        &mut self,
        doc: &Document,
        selected: &Selected,
        current: Option<&Value>,
    ) -> Result<Value> {
        let mut body = match doc {
            Document::PluginCreate(d) => {
                self.publication_categories(&d.categories, None)?;
                let logo = self.resolve_media(Resource::Plugin, &d.logo, selected)?;
                let shots = self.resolve_media_list(Resource::Plugin, &d.screenshots, selected)?;
                let mut value = json!({"name":d.name,"description":selected.texts["description"],"mod_categories":d.categories,"logo":logo,"screenshots":shots,"share_state":d.visibility.share(),"content_origin":d.origin.code(),"content_format":2,"subscribe_plan_level":d.subscribe_level.unwrap_or(0),"link_to_channel":d.link_channel});
                if let Some(v) = &d.intro {
                    value["intro"] = json!(v);
                }
                if let Some(v) = &d.home_page {
                    value["home_page"] = json!(v);
                }
                if let Some(v) = &d.home_page_link {
                    value["home_page_link"] = json!(v);
                }
                value
            }
            Document::WaCreate(d) => {
                self.game_version(d.game_version_id)?;
                self.publication_categories(&d.categories, Some(d.game_version_id))?;
                let thumbnail = self.resolve_media(Resource::Wa, &d.thumbnail, selected)?;
                let images = self.resolve_media_list(Resource::Wa, &d.images, selected)?;
                let mut value = json!({"name":d.name,"description":selected.texts["description"],"wa_str":selected.texts["wa_str"],"wa_log":selected.texts.get("wa_log").map(String::as_str).unwrap_or(""),"game_version_id":d.game_version_id,"category_id_list":d.categories,"thumbnail":thumbnail,"images":images,"share_state":d.visibility.share(),"price":money::yuan(d.price_yuan.as_deref().unwrap_or("0"))?,"subscribe_plan_level":d.subscribe_level,"time_range":d.time_range,"content_origin":d.origin.code(),"content_format":2,"link_to_channel":d.link_channel});
                if let Some(v) = &d.intro {
                    value["intro"] = json!(v);
                }
                value
            }
            Document::ConfigCreate(d) => {
                let backup = self.call("/cloudbackup/detail", &json!({"id":d.backup_id}), false)?;
                check_identity(&backup, d.backup_id, &["id", "backup_id"])?;
                let selection = backup_selection(&backup, d.role_id)?;
                let pictures = self.resolve_media_list(Resource::Config, &d.images, selected)?;
                let mut value = json!({"cloud_id":d.backup_id,"title":d.title,"content":selected.texts["content"],"sharing":if matches!(d.visibility,Visibility::Private){0}else{1},"price":money::yuan(d.price_yuan.as_deref().unwrap_or("0"))?,"pic_url":pictures,"roleid":d.role_id.map(NonZeroU64::get).unwrap_or(0),"subscribe_plan_level":d.subscribe_level,"time_range":d.time_range,"content_origin":d.origin.code(),"content_format":2,"link_to_channel":d.link_channel,"selected_eml":[]});
                for (key, entry) in selection {
                    value_insert(&mut value, key, &entry);
                }
                if let Some(v) = &d.intro {
                    value["intro"] = json!(v);
                }
                value
            }
            Document::PluginEdit(d) => {
                let mut value = current.unwrap().clone();
                value["id"] = json!(d.id);
                if let Some(v) = &d.name {
                    value["name"] = json!(v);
                }
                if let Some(v) = selected.texts.get("description") {
                    value["description"] = json!(v);
                }
                if let Some(v) = &d.intro {
                    value["intro"] = json!(v);
                }
                if let Some(v) = &d.categories {
                    self.publication_categories(v, None)?;
                    value["mod_categories"] = json!(v);
                }
                if let Some(v) = &d.logo {
                    value["logo"] = json!(self.resolve_media(Resource::Plugin, v, selected)?);
                }
                if let Some(v) = &d.screenshots {
                    value["screenshots"] =
                        json!(self.resolve_media_list(Resource::Plugin, v, selected)?);
                }
                if let Some(v) = d.visibility {
                    value["share_state"] = json!(v.share());
                }
                if let Some(v) = d.subscribe_level {
                    value["subscribe_plan_level"] = json!(v);
                }
                if let Some(v) = d.link_channel {
                    value["link_to_channel"] = json!(v);
                }
                value
            }
            Document::WaEdit(d) => {
                let mut value = current.unwrap().clone();
                value["id"] = json!(d.id);
                if let Some(v) = &d.name {
                    value["name"] = json!(v);
                }
                if let Some(v) = selected.texts.get("description") {
                    value["description"] = json!(v);
                }
                if let Some(v) = &d.intro {
                    value["intro"] = json!(v);
                }
                if let Some(v) = &d.categories {
                    value["category_id_list"] = json!(v);
                }
                if let Some(v) = &d.thumbnail {
                    value["thumbnail"] = json!(self.resolve_media(Resource::Wa, v, selected)?);
                }
                if let Some(v) = &d.images {
                    value["images"] = json!(self.resolve_media_list(Resource::Wa, v, selected)?);
                }
                if let Some(v) = d.visibility {
                    value["share_state"] = json!(v.share());
                } else if !matches!(value["share_state"].as_u64(), Some(1 | 2)) {
                    return Err(invalid(
                        "$.visibility",
                        "current WA state is not public/private; select visibility explicitly",
                    ));
                }
                if let Some(v) = &d.price_yuan {
                    value["price"] = json!(money::yuan(v)?);
                }
                if let Some(v) = d.subscribe_level {
                    value["subscribe_plan_level"] = json!(v);
                }
                if let Some(v) = d.link_channel {
                    value["link_to_channel"] = json!(v);
                }
                value.as_object_mut().unwrap().remove("game_version_id");
                value
            }
        };
        // Canonicalize set-valued category IDs; media order remains meaningful.
        for key in ["mod_categories", "category_id_list"] {
            if let Some(values) = body.get_mut(key).and_then(Value::as_array_mut) {
                values.sort_by_key(|v| v.as_u64());
            }
        }
        Ok(body)
    }
    fn publication_categories(
        &mut self,
        ids: &[NonZeroU64],
        game: Option<NonZeroU64>,
    ) -> Result<()> {
        let raw = if let Some(game) = game {
            self.call("/wa/category", &json!({"game_version":game}), false)?
        } else {
            self.http.get(
                Origin::NewbeeMetadata,
                "/modconfig.json",
                &[],
                HeaderMap::new(),
            )?
        };
        let (rows, key) = if game.is_some() {
            (raw.as_array(), "id")
        } else {
            (raw["mod_category"].as_array(), "t_id")
        };
        let rows =
            rows.ok_or_else(|| data("official publication category metadata is incomplete"))?;
        let available = rows
            .iter()
            .map(|row| positive(&row[key]))
            .collect::<Result<BTreeSet<_>>>()?;
        if ids.iter().any(|id| !available.contains(&id.get())) {
            return Err(invalid(
                "$.categories",
                "category is absent from current platform options; inspect plugin categories or official WA categories",
            ));
        }
        Ok(())
    }
    pub(super) fn publication_state(
        &mut self,
        resource: Resource,
        id: NonZeroU64,
        create: bool,
    ) -> Result<Value> {
        let raw = self.call(
            &format!("/{}/publish_detail", resource.route()),
            &json!({"id":id}),
            false,
        )?;
        check_identity(
            &raw,
            id,
            &[
                "id",
                match resource {
                    Resource::Plugin => "mod_id",
                    Resource::Wa => "wa_id",
                    Resource::Config => "config_id",
                },
            ],
        )?;
        let mut value = match resource {
            Resource::Plugin => {
                json!({"name":required_text(&raw,"name")?,"description":required_text(&raw,"description")?,"intro":intro(&raw)?,"content_format":format(&raw)?,"mod_categories":category_ids(&raw)?,"logo":required_text(&raw,"logo")?,"screenshots":urls(&raw,"screenshots")?,"share_state":visibility(&raw)?,"subscribe_plan_level":level(&raw)?,"link_to_channel":boolean(&raw,"link_to_channel")?})
            }
            Resource::Wa => {
                match raw["is_multi_string"].as_bool() {
                    Some(false) => (),
                    Some(true) => {
                        return Err(ToolkitError::new(
                            "publish.ncc_wa_mode",
                            "official form edits support single strings; edit collections through the Creator interface",
                            3,
                        ));
                    }
                    None => return Err(data("official WA detail did not declare its string mode")),
                }
                json!({"name":required_text(&raw,"name")?,"description":required_text(&raw,"description")?,"intro":intro(&raw)?,"content_format":format(&raw)?,"category_id_list":category_ids(&raw)?,"thumbnail":required_text(&raw,"thumbnail")?,"images":urls(&raw,"images")?,"share_state":visibility(&raw)?,"price":money::wire(&raw["price"])?,"time_range":required_text(&raw,"paid_time_range")?,"subscribe_plan_level":level(&raw)?,"link_to_channel":boolean(&raw,"link_to_channel")?})
            }
            Resource::Config => {
                let text = self.call("/share_config/pull_text", &json!({"id":id}), false)?;
                check_identity(&text, id, &["id", "config_id"])?;
                let role = match raw.get("shared_role") {
                    Some(Value::Null) => 0,
                    Some(value) => positive(&value["role_id"])?,
                    None => {
                        return Err(data(
                            "official configuration detail omitted its role selection",
                        ));
                    }
                };
                json!({"title":required_text(&raw,"title")?,"intro":intro(&raw)?,"content_format":format(&text)?,"content_sha256":storage::sha256(required_text(&text,"content")?.as_bytes()),"share_state":visibility(&raw)?,"cloud_id":positive(&raw["cloud_backup"]["backup_id"])?,"roleid":role,"linked_mod_count":raw["linked_mod_count"].as_u64().ok_or_else(||data("official configuration detail omitted linked addon count"))?,"price":money::wire(&raw["price"])?,"subscribe_plan_level":level(&raw)?,"pic_url":urls(&raw,"screenshots")?,"link_to_channel":boolean(&raw,"link_to_channel")?})
            }
        };
        if resource == Resource::Wa {
            if create {
                let text = self.call("/wa/pull_str", &json!({"id":id}), false)?;
                check_identity(&text, id, &["id", "wa_id"])?;
                value["wa_str_sha256"] =
                    json!(storage::sha256(required_text(&text, "wa_str")?.as_bytes()));
            }
        }
        Ok(value)
    }
}
fn value_insert(object: &mut Value, key: String, value: &Value) {
    object[key] = value.clone();
}
fn check_identity(raw: &Value, id: NonZeroU64, keys: &[&str]) -> Result<()> {
    if !raw.is_object() {
        return Err(data("official author detail is not an object"));
    }
    for key in keys {
        if raw
            .get(*key)
            .is_some_and(|value| !positive(value).is_ok_and(|n| n == id.get()))
        {
            return Err(data("official author detail identifies another resource"));
        }
    }
    Ok(())
}
fn required_text<'a>(raw: &'a Value, key: &str) -> Result<&'a str> {
    raw[key]
        .as_str()
        .ok_or_else(|| data(&format!("official author detail omitted text field {key}")))
}
fn intro(raw: &Value) -> Result<Value> {
    match raw.get("intro") {
        Some(v) if v.is_string() || v.is_null() => Ok(v.clone()),
        _ => Err(data("official author detail omitted its summary")),
    }
}
fn format(raw: &Value) -> Result<u64> {
    let v = positive(&raw["content_format"])?;
    if !matches!(v, 1 | 2) {
        return Err(data("official author content format is unsupported"));
    }
    Ok(v)
}
fn visibility(raw: &Value) -> Result<u64> {
    raw["visibility"]
        .as_u64()
        .ok_or_else(|| data("official author detail omitted visibility"))
}
fn level(raw: &Value) -> Result<Value> {
    match raw.get("subscribe_plan_level") {
        Some(v) if v.is_null() || v.as_u64().is_some() => Ok(v.clone()),
        _ => Err(data(
            "official author detail omitted its subscription level",
        )),
    }
}
fn boolean(raw: &Value, key: &str) -> Result<bool> {
    raw[key].as_bool().ok_or_else(|| {
        data(&format!(
            "official author detail omitted boolean field {key}"
        ))
    })
}
fn category_ids(raw: &Value) -> Result<Vec<u64>> {
    let rows = raw["categories"]
        .as_array()
        .ok_or_else(|| data("official author detail omitted categories"))?;
    let mut ids = rows
        .iter()
        .map(|row| positive(&row["id"]))
        .collect::<Result<Vec<_>>>()?;
    ids.sort_unstable();
    if ids.iter().collect::<BTreeSet<_>>().len() != ids.len() {
        return Err(data("official categories contain duplicate IDs"));
    }
    Ok(ids)
}
fn urls(raw: &Value, key: &str) -> Result<Vec<String>> {
    raw[key]
        .as_array()
        .ok_or_else(|| data("official author detail omitted media array"))?
        .iter()
        .map(|value| {
            let text = value["url"]
                .as_str()
                .ok_or_else(|| data("official author media row omitted URL"))?;
            public_url(text).map_err(|_| data("official author media URL is not reusable"))?;
            Ok(text.to_owned())
        })
        .collect()
}
pub(super) fn expected_state(resource: Resource, body: &Value) -> Result<Value> {
    let mut value = body.clone();
    let map = value
        .as_object_mut()
        .ok_or_else(|| data("official author request is not an object"))?;
    let keys: &[&str] = match resource {
        Resource::Plugin => &[
            "name",
            "description",
            "intro",
            "content_format",
            "mod_categories",
            "logo",
            "screenshots",
            "share_state",
            "subscribe_plan_level",
            "link_to_channel",
        ],
        Resource::Wa => &[
            "name",
            "description",
            "intro",
            "content_format",
            "category_id_list",
            "thumbnail",
            "images",
            "share_state",
            "price",
            "time_range",
            "subscribe_plan_level",
            "link_to_channel",
        ],
        Resource::Config => &[
            "title",
            "intro",
            "content_format",
            "cloud_id",
            "roleid",
            "price",
            "subscribe_plan_level",
            "pic_url",
            "link_to_channel",
        ],
    };
    map.retain(|key, _| keys.contains(&key.as_str()));
    if !map.contains_key("intro") {
        map.insert("intro".into(), json!(""));
    }
    if resource == Resource::Config {
        value["content_sha256"] =
            json!(storage::sha256(required_text(body, "content")?.as_bytes()));
        value["share_state"] = body["sharing"].clone();
        value["linked_mod_count"] = json!(
            body["linked_mods"]
                .as_array()
                .ok_or_else(|| data("official configuration request omitted links"))?
                .len()
        );
    }
    if resource == Resource::Wa && body.get("wa_str").is_some() {
        value["wa_str_sha256"] = json!(storage::sha256(required_text(body, "wa_str")?.as_bytes()));
    }
    Ok(value)
}
fn backup_selection(raw: &Value, selected: Option<NonZeroU64>) -> Result<BTreeMap<String, Value>> {
    let rows = |key: &str| {
        raw[key]
            .as_array()
            .ok_or_else(|| data("official backup selection arrays are incomplete"))
    };
    let mut linked = Vec::new();
    for row in rows("addons")? {
        match row.get("mod_id") {
            Some(value) if value == &json!(0) || value == "0" || value.is_null() => (),
            Some(value) => {
                linked
                    .push(json!({"mod_id":positive(value)?,"mod_name":required_text(row,"name")?}));
            }
            None => return Err(data("official backup addon row omitted identity")),
        }
    }
    let names = |key: &str| -> Result<Value> {
        Ok(json!(
            rows(key)?
                .iter()
                .map(|row| required_text(row, "name").map(str::to_owned))
                .collect::<Result<Vec<_>>>()?
        ))
    };
    let mut roles = BTreeSet::new();
    for account in rows("wtf_accounts")? {
        for server in account["servers"]
            .as_array()
            .ok_or_else(|| data("official backup account omitted servers"))?
        {
            for role in server["roles"]
                .as_array()
                .ok_or_else(|| data("official backup server omitted roles"))?
            {
                let id = positive(&role["role_id"])?;
                if !roles.insert(id) {
                    return Err(data("official backup contains duplicate role IDs"));
                }
            }
        }
    }
    if !roles.is_empty() && selected.is_none() {
        return Err(invalid(
            "$.role_id",
            "backup contains WTF roles; select one exact role ID from official backup get",
        ));
    }
    if selected.is_some_and(|id| !roles.contains(&id.get())) {
        return Err(invalid(
            "$.role_id",
            "selected role is absent from the original backup",
        ));
    }
    Ok(BTreeMap::from([
        ("linked_mods".into(), json!(linked)),
        ("ignored_unknown_mods".into(), names("unknown_files")?),
        ("ignored_materials".into(), names("materials")?),
        ("ignored_fronts".into(), names("fonts")?),
    ]))
}
