//! Full DD plugin editor, using the retained author and original upload bytes.
use super::{
    ObjectUpload, Read, Reference, Resource, UploadKind,
    forms::{self, Api, WriteAction, data},
    publication::{self, Context, Prepared, hashes, selected},
    read::{self, Attached},
    task::Client,
};
use crate::{
    contract,
    input::{self, invalid},
    mutation::Mutation,
    provider::Provider,
};
use lycheedev_core::{Result, control::OperationControl, storage};
use serde_json::{Map, Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    num::NonZeroU64,
    path::Path,
    time::{Duration, Instant},
};
mod form;

pub struct PluginWrite {
    action: WriteAction,
    doc: Value,
    uploads: Vec<(String, ObjectUpload)>,
}
impl PluginWrite {
    pub fn parse(
        action: WriteAction,
        bytes: &[u8],
        cwd: &Path,
        control: &OperationControl,
    ) -> Result<Self> {
        control.check()?;
        let doc = input::document(bytes)?;
        contract::input_shape(Provider::Dd, "plugin", action.name())?.admit(&doc)?;
        form::nested(&doc)?;
        if action == WriteAction::Create {
            forms::admit_create(Resource::Plugin, &doc)?;
        }
        if let Some(sn) = doc.get("sn") {
            Reference::parse(sn.as_str().unwrap())
                .map_err(|_| invalid("$.sn", "invalid opaque DD plugin SN"))?;
        }
        let mut uploads = Vec::new();
        for (key, kind) in [
            ("file", UploadKind::PluginArchive),
            ("logo_file", UploadKind::PluginImage),
        ] {
            if let Some(path) = doc.get(key) {
                let path = path
                    .as_str()
                    .filter(|s| !s.trim().is_empty())
                    .ok_or_else(|| {
                        invalid(&format!("$.{key}"), "local file path must not be blank")
                    })?;
                let upload = if key == "file" {
                    ObjectUpload::plugin_archive(&cwd.join(path), control)?
                } else {
                    ObjectUpload::select(&cwd.join(path), kind, control)?
                };
                uploads.push((key.into(), upload));
            }
        }
        if let Some(rows) = doc.get("detail_img_files").and_then(Value::as_array) {
            for path in rows {
                uploads.push((
                    "detail_img_files".into(),
                    ObjectUpload::select(
                        &cwd.join(path.as_str().unwrap()),
                        UploadKind::PluginImage,
                        control,
                    )?,
                ));
            }
        }
        let doc = json!(doc);
        if action == WriteAction::Create {
            form::validate(&create_form(&doc)?, &doc)?;
        }
        Ok(Self {
            action,
            doc,
            uploads,
        })
    }
    pub fn schema(action: WriteAction) -> Value {
        form::schema(action)
    }
    pub fn plan(&self) -> Value {
        let fields = self
            .doc
            .as_object()
            .unwrap()
            .iter()
            .filter(|(k, _)| {
                !matches!(
                    k.as_str(),
                    "schema" | "file" | "logo_file" | "detail_img_files"
                )
            })
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect::<Map<_, _>>();
        forms::public(
            json!({"platform":"dd","resource":"plugin","action":self.action,"reference":self.doc.get("sn"),
            "fields":fields,"uploads":self.uploads.iter().map(|(field,upload)|json!({"field":field,"selection":upload.plan()})).collect::<Vec<_>>(),
            "remote_read":false,"remote_write":false,"credential_reads":false,
            "execution_requires":["explicit original DDT","fresh ownership and full current form","live dependent options","complete stable-field and published-version readback"],"uncertain_submission_replay":false}),
        )
    }
    pub fn execute(
        &self,
        home: &Path,
        client: &Client,
        recover_sn: Option<&str>,
        control: &OperationControl,
    ) -> Result<Value> {
        control.check()?;
        let identity = client.author_identity(control)?;
        let author = identity
            .pointer("/author/author_id")
            .and_then(Value::as_str)
            .ok_or_else(|| data("the retained DD task omitted its author identity"))?;
        self.execute_with(
            home,
            author,
            &mut Attached { client, control },
            recover_sn,
            control,
        )
    }
    fn identity(&self, author: &str) -> Value {
        let mut doc = self.doc.clone();
        for key in ["file", "logo_file", "detail_img_files"] {
            doc.as_object_mut().unwrap().remove(key);
        }
        let uploads = self
            .uploads
            .iter()
            .map(|(field, upload)| {
                let mut value = upload.plan();
                value.as_object_mut().unwrap().remove("file");
                json!({"field":field,"artifact":value})
            })
            .collect::<Vec<_>>();
        json!({"author":author,"request":doc,"uploads":uploads})
    }
    fn execute_with(
        &self,
        home: &Path,
        author: &str,
        api: &mut impl Api,
        recover_sn: Option<&str>,
        control: &OperationControl,
    ) -> Result<Value> {
        publication::execute(self, home, author, api, recover_sn, control)
    }
}
impl publication::Editor for PluginWrite {
    fn resource(&self) -> Resource {
        Resource::Plugin
    }
    fn action(&self) -> WriteAction {
        self.action
    }
    fn original_sn(&self) -> Option<&str> {
        self.doc.get("sn").and_then(Value::as_str)
    }
    fn identity(&self, author: &str) -> Value {
        PluginWrite::identity(self, author)
    }
    fn fields(&self) -> &'static [&'static str] {
        form::FIELDS
    }
    fn verification(&self) -> &'static str {
        "stable_fields_and_published_version"
    }
    fn verify(
        &self,
        api: &mut impl Api,
        context: &Context,
        mutation: &Mutation,
        control: &OperationControl,
    ) -> Result<Value> {
        verify(api, context, mutation, control)
    }
    fn prepare(&self, api: &mut impl Api, control: &OperationControl) -> Result<Prepared> {
        let (current, mut body, before_references) = if self.action == WriteAction::Create {
            let body = create_form(&self.doc)?;
            let rows = read::all_author(api, Resource::Plugin, forms::game(&body)?, control)?;
            let ids = rows
                .iter()
                .map(forms::reference)
                .collect::<Result<Vec<_>>>()?;
            (None, body, ids)
        } else {
            let sn = self.doc["sn"].as_str().unwrap();
            let current = read::detail(api, Resource::Plugin, &Reference::parse(sn)?)?;
            if current["is_owner"] == false {
                return Err(data("the selected DD plugin is not owned by this author"));
            }
            let game = forms::game(&current)?;
            let rows = read::all_author(api, Resource::Plugin, game, control)?;
            let listing = selected(&rows, sn)?;
            if current["is_owner"] != true && listing.is_none() {
                return Err(data(
                    "DD plugin ownership is absent from the complete author list",
                ));
            }
            if self.action == WriteAction::Edit {
                let published = form::version(listing.ok_or_else(|| {
                    data("editing requires the published author-list plugin version")
                })?)?;
                if ["detail_url", "release_type", "version"]
                    .iter()
                    .any(|k| published.get(*k).is_none())
                {
                    return Err(data(
                        "author-list projection omitted a published plugin version field",
                    ));
                }
            }
            let mut body = form::project(
                &current,
                if self.action == WriteAction::Edit {
                    listing
                } else {
                    None
                },
            )?;
            for (key, value) in self.doc.as_object().unwrap() {
                if form::FIELDS.contains(&key.as_str()) {
                    body[key] = value.clone();
                }
            }
            body["sn"] = self.doc["sn"].clone();
            forms::normalize(body.as_object_mut().unwrap(), Resource::Plugin, false)?;
            if forms::truth(&current["assign_user_sn"]) && body["scope"] != "public" {
                return Err(invalid(
                    "$.scope",
                    "assigned plugins can only use public scope",
                ));
            }
            forms::locked_usage(&current, &body, &self.doc)?;
            if self.action == WriteAction::Update {
                let candidate = body["version"].as_str().unwrap().trim().to_lowercase();
                let versions = [
                    form::version(&current)?,
                    form::version(listing.unwrap_or(&Value::Null))?,
                ];
                if versions.iter().any(|v| {
                    v["version"]
                        .as_str()
                        .is_some_and(|s| s.trim().to_lowercase() == candidate)
                }) {
                    return Err(invalid(
                        "$.version",
                        "version already exists; overwrite is not allowed",
                    ));
                }
                if history(api, sn, game, control)?.contains(&candidate) {
                    return Err(invalid(
                        "$.version",
                        "version already exists in history; overwrite is not allowed",
                    ));
                }
            }
            (Some(current), body, Vec::new())
        };
        form::validate(&body, &self.doc)?;
        options(api, &body, current.as_ref(), self.action, control)?;
        for (key, upload) in &self.uploads {
            let value = api.upload(upload)?;
            let url = value["url"]
                .as_str()
                .ok_or_else(|| data("DD object upload omitted its permanent receipt"))?;
            forms::permanent(url, "$.file")?;
            match key.as_str() {
                "file" => body["detail_url"] = json!(url),
                "logo_file" => body["logo"] = json!(url),
                "detail_img_files" => {
                    if body.get("detail_imgs").is_none() {
                        body["detail_imgs"] = json!([]);
                    }
                    body["detail_imgs"]
                        .as_array_mut()
                        .ok_or_else(|| data("DD plugin images are not an array"))?
                        .push(json!(url));
                }
                _ => unreachable!(),
            }
        }
        form::validate(&body, &json!({}))?;
        let fields = if self.action == WriteAction::Update {
            hashes(&body, form::VERSION_FIELDS)?
        } else {
            hashes(&body, form::FIELDS)?
        };
        let context = Context {
            resource: Resource::Plugin,
            action: self.action,
            game_type: forms::game(&body)?,
            reference: self
                .doc
                .get("sn")
                .and_then(Value::as_str)
                .map(str::to_owned),
            before_references,
            fields,
            version_fields: if self.action == WriteAction::Edit {
                BTreeMap::new()
            } else {
                hashes(&body, form::VERSION_FIELDS)?
            },
            verified: false,
        };
        Ok(Prepared { body, context })
    }
}
fn create_form(doc: &Value) -> Result<Value> {
    let mut form = json!({"share_code_life_type":"seven_day","addon_type":0,"buy_life_type":"seven_day","need_buy":false,"with_associate":false,"second_category_ids":[],"vip_levels":[]});
    for &key in form::FIELDS {
        if let Some(value) = doc.get(key) {
            form[key] = value.clone();
        }
    }
    forms::normalize(form.as_object_mut().unwrap(), Resource::Plugin, true)?;
    Ok(form)
}
fn options(
    api: &mut impl Api,
    form: &Value,
    current: Option<&Value>,
    action: WriteAction,
    control: &OperationControl,
) -> Result<()> {
    let game = forms::game(form)?;
    let rows = Read::GameVersions(game).execute(api, control)?;
    forms::choices(
        &rows,
        form["game_versions"]
            .as_array()
            .ok_or_else(|| data("DD game versions are not an array"))?,
        &["value"],
        "$.game_versions",
    )?;
    let rows = Read::PluginCategories.execute(api, control)?;
    let primary = forms::scalar(&form["primary_category_id"])
        .ok_or_else(|| data("DD primary category is invalid"))?;
    let parent = rows["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"].as_str() == Some(&primary))
        .ok_or_else(|| {
            invalid(
                "$.primary_category_id",
                "select a live top-level plugin category",
            )
        })?;
    let children = form["second_category_ids"]
        .as_array()
        .ok_or_else(|| data("DD child categories are not an array"))?;
    let available = parent["children"].as_array().unwrap();
    if (!available.is_empty() && children.is_empty())
        || children.iter().any(|v| {
            forms::scalar(v).is_none_or(|v| !available.iter().any(|r| r["id"].as_str() == Some(&v)))
        })
    {
        return Err(invalid(
            "$.second_category_ids",
            "select live child categories belonging to the selected primary category",
        ));
    }
    if action != WriteAction::Update {
        forms::optional_choices(api, form, current, game, control)?;
    }
    let rows = Read::GameTypes.execute(api, control)?;
    forms::choices(&rows, &[json!(game.get())], &["game_type"], "$.game_type")
}
fn history(
    api: &mut impl Api,
    sn: &str,
    game: NonZeroU64,
    control: &OperationControl,
) -> Result<BTreeSet<String>> {
    let mut versions = BTreeSet::new();
    let mut pages = BTreeSet::new();
    let mut known_total = None;
    let mut count = 0u64;
    for page in 1..=1000 {
        control.check()?;
        let payload = api.get(
            "/addon/addon_versions",
            &[
                ("sn", sn.into()),
                ("game_type", game.to_string()),
                ("page", page.to_string()),
            ],
        )?;
        let rows = read::items(&payload)?;
        let total = read::total(&payload)?;
        if let (Some(a), Some(b)) = (known_total, total) {
            if a != b {
                return Err(data("DD version-history total changed during pagination"));
            }
        }
        if total.is_some() {
            known_total = total;
        }
        if !rows.is_empty()
            && !pages.insert(storage::sha256(&storage::identity_json(&json!(rows))?))
        {
            return Err(data("DD version-history pagination repeated a page"));
        }
        for row in rows {
            let version = row["version"]
                .as_str()
                .filter(|v| !v.trim().is_empty())
                .ok_or_else(|| data("DD version history omitted a scalar version"))?;
            versions.insert(version.trim().to_lowercase());
        }
        count = count
            .checked_add(rows.len() as u64)
            .ok_or_else(|| data("DD history row count overflow"))?;
        if known_total.is_some_and(|n| count > n) {
            return Err(data("DD version history exceeded its reported total"));
        }
        if known_total == Some(count) || (known_total.is_none() && rows.is_empty()) {
            return Ok(versions);
        }
        if rows.is_empty() {
            return Err(data("DD version history ended before its reported total"));
        }
    }
    Err(data("DD version history exceeded 1000 pages"))
}
fn mismatches(expected: &BTreeMap<String, String>, actual: &Value) -> Result<BTreeSet<String>> {
    let mut fields = BTreeSet::new();
    for (key, hash) in expected {
        let actual = actual.get(key).map(forms::digest).transpose()?;
        if actual.as_deref() != Some(hash.as_str()) {
            fields.insert(key.clone());
        }
    }
    Ok(fields)
}
fn verify(
    api: &mut impl Api,
    context: &Context,
    mutation: &Mutation,
    control: &OperationControl,
) -> Result<Value> {
    let sn = context
        .reference
        .as_deref()
        .ok_or_else(|| mutation.pending_error("select the original creation SN before readback"))?;
    let readback = || data("DD write readback does not match all submitted fields");
    let mut unresolved = BTreeSet::new();
    for attempt in 0..6 {
        control
            .check()
            .map_err(|e| mutation.readback_error("DD plugin verification is pending", e))?;
        if attempt > 0 {
            let until = Instant::now() + Duration::from_secs(1);
            while Instant::now() < until {
                control
                    .check()
                    .map_err(|e| mutation.readback_error("DD plugin verification is pending", e))?;
                std::thread::sleep(Duration::from_millis(25));
            }
        }
        let raw = read::detail(api, Resource::Plugin, &Reference::parse(sn)?)
            .map_err(|e| mutation.readback_error("DD plugin detail readback is pending", e))?;
        if raw["is_owner"] == false {
            return Err(mutation.readback_error(
                "DD plugin readback author differs from its original owner",
                data("readback target is not owned by this author"),
            ));
        }
        let rows =
            read::all_author(api, Resource::Plugin, context.game_type, control).map_err(|e| {
                mutation.readback_error("DD plugin author-list readback is incomplete", e)
            })?;
        let author = selected(&rows, sn).map_err(|e| {
            mutation.readback_error("DD plugin author-list identity is ambiguous", e)
        })?;
        if raw["is_owner"] != true && author.is_none() {
            return Err(mutation.readback_error(
                "DD plugin readback ownership is unverified",
                data("selected SN is absent from the complete author list"),
            ));
        }
        let mut detail = form::project(&raw, None)
            .map_err(|e| mutation.readback_error("DD plugin detail projection is invalid", e))?;
        let mut author_projection = author
            .map(|v| form::project(v, None))
            .transpose()
            .map_err(|e| mutation.readback_error("DD plugin author projection is invalid", e))?
            .unwrap_or(Value::Null);
        if !context.version_fields.is_empty() {
            for (projection, raw) in [
                (&mut detail, &raw),
                (&mut author_projection, author.unwrap_or(&Value::Null)),
            ] {
                if projection.is_object() {
                    let version = form::version(raw).map_err(|e| {
                        mutation.readback_error("DD published version projection is invalid", e)
                    })?;
                    for &key in form::VERSION_FIELDS {
                        if let Some(v) = version.get(key) {
                            projection[key] = v.clone();
                        }
                    }
                }
            }
        }
        let detail_mismatch = mismatches(&context.fields, &detail)?;
        let author_mismatch = mismatches(&context.fields, &author_projection)?;
        unresolved = detail_mismatch
            .intersection(&author_mismatch)
            .cloned()
            .collect();
        let mut version_source = Value::Null;
        if !context.version_fields.is_empty() {
            let a = form::version(&raw).map_err(|e| {
                mutation.readback_error("DD detail version projection is invalid", e)
            })?;
            let b = author
                .map(form::version)
                .transpose()
                .map_err(|e| mutation.readback_error("DD author version projection is invalid", e))?
                .unwrap_or(Value::Null);
            let ma = mismatches(&context.version_fields, &a)?;
            let mb = mismatches(&context.version_fields, &b)?;
            if ma.is_empty() {
                version_source = json!("detail_v2");
            } else if mb.is_empty() {
                version_source = json!("author_list");
            } else {
                unresolved.extend(ma.union(&mb).cloned());
            }
        }
        if unresolved.is_empty() {
            let sources = context
                .fields
                .keys()
                .map(|k| {
                    (
                        k.clone(),
                        json!(if detail_mismatch.contains(k) {
                            "author_list"
                        } else {
                            "detail_v2"
                        }),
                    )
                })
                .collect::<Map<_, _>>();
            return Ok(
                json!({"sources":sources,"version_source":version_source,"complete_author_list":true,
                "detail":read::public_detail(raw),"author":author.cloned().map(read::public_detail)}),
            );
        }
    }
    let mut cause = readback();
    cause.details = json!({"fields":unresolved});
    Err(mutation.readback_error(
        "DD plugin write is accepted or uncertain; recover only by original readback",
        cause,
    ))
}
#[cfg(test)]
mod tests;
