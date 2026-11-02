//! Configuration editor bound to an exact current backup and its selectors.
use super::{
    ObjectUpload, Read, Reference, Resource, UploadKind,
    forms::{self, Api, WriteAction, data},
    publication::{self, Context, Editor, Prepared},
    read::{self, Attached},
    task::Client,
};
use crate::{
    contract,
    input::{self, invalid},
    mutation::Mutation,
    provider::Provider,
};
use lycheedev_core::{Result, control::OperationControl};
use serde_json::{Map, Value, json};
use std::{
    collections::BTreeMap,
    path::Path,
    time::{Duration, Instant},
};
mod form;
mod selection;
pub struct ConfigWrite {
    action: WriteAction,
    doc: Value,
    uploads: Vec<ObjectUpload>,
}
impl ConfigWrite {
    pub fn parse(
        action: WriteAction,
        bytes: &[u8],
        cwd: &Path,
        control: &OperationControl,
    ) -> Result<Self> {
        control.check()?;
        let doc = input::document(bytes)?;
        contract::input_shape(Provider::Dd, "config", action.name())?.admit(&doc)?;
        form::nested(&doc)?;
        if action == WriteAction::Create {
            forms::admit_create(Resource::Config, &doc)?;
            let count = |k: &str| doc.get(k).and_then(Value::as_array).map_or(0, Vec::len);
            if count("display_imgs") + count("display_img_files") == 0 {
                return Err(invalid(
                    "$.display_imgs",
                    "configuration creation requires at least one display image",
                ));
            }
            if [
                "known_addon_ids",
                "unknown_addon_ids",
                "wtf_role_ids",
                "material_names",
                "font_names",
            ]
            .iter()
            .all(|k| count(k) == 0)
            {
                return Err(invalid(
                    "$.known_addon_ids",
                    "configuration content cannot contain only WA selections",
                ));
            }
            if count("wtf_role_ids") == 0
                && [
                    "known_addon_ids",
                    "unknown_addon_ids",
                    "known_wa_ids",
                    "unknown_wa_ids",
                ]
                .iter()
                .any(|k| count(k) > 0)
            {
                return Err(invalid(
                    "$.wtf_role_ids",
                    "selected addon/WA content requires a WTF role",
                ));
            }
        }
        for key in ["share_sn", "backup_sn"] {
            if let Some(v) = doc.get(key) {
                Reference::parse(v.as_str().unwrap())
                    .map_err(|_| invalid(&format!("$.{key}"), "invalid opaque DD SN"))?;
            }
        }
        let mut uploads = Vec::new();
        if let Some(a) = doc.get("display_img_files").and_then(Value::as_array) {
            for file in a {
                uploads.push(ObjectUpload::select(
                    &cwd.join(file.as_str().unwrap()),
                    UploadKind::ConfigImage,
                    control,
                )?);
            }
        }
        Ok(Self {
            action,
            doc: json!(doc),
            uploads,
        })
    }
    pub fn schema(action: WriteAction) -> Value {
        form::schema(action)
    }
    pub fn plan(&self) -> Value {
        let mut fields = self.doc.clone();
        fields.as_object_mut().unwrap().remove("display_img_files");
        forms::public(
            json!({"platform":"dd","resource":"config","action":self.action,"reference":self.doc.get("share_sn"),"fields":fields,"uploads":self.uploads.iter().map(ObjectUpload::plan).collect::<Vec<_>>(),"remote_write":false,"remote_read":false,"credential_reads":false,
            "execution_requires":["explicit retained DDT","fresh ownership","exact backup membership and selectors","complete reselection when changing backup","account-bound WA and retail layouts","full field readback"],"uncertain_submission_replay":false}),
        )
    }
    pub fn execute(
        &self,
        home: &Path,
        client: &Client,
        recover_sn: Option<&str>,
        control: &OperationControl,
    ) -> Result<Value> {
        let identity = client.author_identity(control)?;
        let author = identity
            .pointer("/author/author_id")
            .and_then(Value::as_str)
            .ok_or_else(|| data("retained DD task omitted its author identity"))?;
        publication::execute(
            self,
            home,
            author,
            &mut Attached { client, control },
            recover_sn,
            control,
        )
    }
}
impl Editor for ConfigWrite {
    fn resource(&self) -> Resource {
        Resource::Config
    }
    fn action(&self) -> WriteAction {
        self.action
    }
    fn original_sn(&self) -> Option<&str> {
        self.doc.get("share_sn").and_then(Value::as_str)
    }
    fn fields(&self) -> &'static [&'static str] {
        form::FIELDS
    }
    fn verification(&self) -> &'static str {
        "stable_fields_and_selected_backup_content"
    }
    fn identity(&self, author: &str) -> Value {
        let mut doc = self.doc.clone();
        doc.as_object_mut().unwrap().remove("display_img_files");
        let uploads = self
            .uploads
            .iter()
            .map(|u| {
                let mut p = u.plan();
                p.as_object_mut().unwrap().remove("file");
                p
            })
            .collect::<Vec<_>>();
        json!({"author":author,"request":doc,"uploads":uploads})
    }
    fn prepare(&self, api: &mut impl Api, control: &OperationControl) -> Result<Prepared> {
        let current = if let Some(sn) = self.original_sn() {
            let mut v = read::detail(api, Resource::Config, &Reference::parse(sn)?)?;
            if v["is_owner"] == false {
                return Err(data(
                    "the selected DD configuration is not owned by the retained author",
                ));
            }
            if !v["backup_sn"].as_str().is_some_and(|s| !s.is_empty()) {
                let until = Instant::now() + Duration::from_secs(1);
                while Instant::now() < until {
                    control.check()?;
                    std::thread::sleep(Duration::from_millis(25));
                }
                v = read::detail(api, Resource::Config, &Reference::parse(sn)?)?;
                if v["is_owner"] == false {
                    return Err(data(
                        "the selected DD configuration is not owned by the retained author",
                    ));
                }
            }
            if !v["backup_sn"].as_str().is_some_and(|s| !s.is_empty()) {
                return Err(data("DD configuration omitted its current backup SN"));
            }
            v
        } else {
            Value::Null
        };
        let sn = self
            .doc
            .get("backup_sn")
            .and_then(Value::as_str)
            .or_else(|| current["backup_sn"].as_str())
            .ok_or_else(|| invalid("$.backup_sn", "select an exact current DD backup"))?;
        let changed =
            self.action == WriteAction::Create || Some(sn) != current["backup_sn"].as_str();
        if changed {
            for key in [
                "known_addon_ids",
                "unknown_addon_ids",
                "wtf_role_ids",
                "material_names",
                "font_names",
                "known_wa_ids",
                "unknown_wa_ids",
            ] {
                if self.doc.get(key).is_none() {
                    return Err(invalid(
                        &format!("$.{key}"),
                        "changing backup requires complete content and role reselection",
                    ));
                }
            }
        }
        let backup = super::backup::selected(api, sn)?;
        let backup_game = forms::game(&backup)?;
        let game = if current.get("game_type").is_some_and(forms::truth) {
            forms::game(&current)?
        } else {
            backup_game
        };
        let before = read::all_author(api, Resource::Config, game, control)?;
        if let Some(target) = self.original_sn() {
            let owned = publication::selected(&before, target)?.is_some();
            if current["is_owner"] != true && !owned {
                return Err(data(
                    "DD configuration ownership is absent from complete author membership",
                ));
            }
        }
        let mut body = form::build(&current, &backup, &self.doc, changed)?;
        body["backup_sn"] = json!(sn);
        if let Some(target) = self.original_sn() {
            body["share_sn"] = json!(target);
        }
        form::validate(&body, &self.doc)?;
        let mut validation = body.clone();
        validation["game_type"] = json!(game.get());
        forms::optional_choices(
            api,
            &validation,
            if current.is_null() {
                None
            } else {
                Some(&current)
            },
            game,
            control,
        )?;
        let games = Read::GameTypes.execute(api, control)?;
        forms::choices(&games, &[json!(game.get())], &["game_type"], "$.game_type")?;
        for u in &self.uploads {
            let uploaded = api.upload(u)?;
            let url = uploaded["url"]
                .as_str()
                .ok_or_else(|| data("DD configuration image upload omitted its permanent URL"))?;
            forms::permanent(url, "$.display_imgs")?;
            body["display_imgs"]
                .as_array_mut()
                .unwrap()
                .push(json!(url));
        }
        form::validate(&body, &json!({}))?;
        let context = Context {
            resource: Resource::Config,
            action: self.action,
            game_type: game,
            reference: self.original_sn().map(str::to_owned),
            before_references: if self.action == WriteAction::Create {
                before
                    .iter()
                    .map(forms::reference)
                    .collect::<Result<Vec<_>>>()?
            } else {
                Vec::new()
            },
            fields: publication::hashes(&body, form::FIELDS)?,
            version_fields: BTreeMap::new(),
            verified: false,
        };
        Ok(Prepared { body, context })
    }
    fn verify(
        &self,
        api: &mut impl Api,
        context: &Context,
        mutation: &Mutation,
        control: &OperationControl,
    ) -> Result<Value> {
        publication::verify_fields(api, context, mutation, control, form::project)
    }
}
#[cfg(test)]
mod tests;
