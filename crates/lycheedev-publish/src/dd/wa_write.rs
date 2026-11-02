//! DD WA editor: opaque imports, parsed modern identity and optional materials.
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
use std::{collections::BTreeMap, path::Path};
mod form;
pub struct WaWrite {
    action: WriteAction,
    doc: Value,
    uploads: Vec<(String, ObjectUpload)>,
}
impl WaWrite {
    pub fn parse(
        action: WriteAction,
        bytes: &[u8],
        cwd: &Path,
        control: &OperationControl,
    ) -> Result<Self> {
        control.check()?;
        let doc = input::document(bytes)?;
        contract::input_shape(Provider::Dd, "wa", action.name())?.admit(&doc)?;
        form::nested(&doc, control)?;
        if action != WriteAction::Edit {
            form::admit_version(doc["version"].as_str().unwrap())?;
        }
        if action == WriteAction::Create {
            forms::admit_create(Resource::Wa, &doc)?;
        }
        if let Some(sn) = doc.get("sn") {
            Reference::parse(sn.as_str().unwrap())
                .map_err(|_| invalid("$.sn", "invalid opaque DD WA SN"))?;
        }
        let mut uploads = Vec::new();
        if let Some(file) = doc.get("file") {
            let path = file
                .as_str()
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| invalid("$.file", "local material file path must not be blank"))?;
            uploads.push((
                "file".into(),
                ObjectUpload::select(&cwd.join(path), UploadKind::WaMaterials, control)?,
            ));
        }
        if let Some(rows) = doc.get("display_img_files").and_then(Value::as_array) {
            for row in rows {
                uploads.push((
                    "display_img_files".into(),
                    ObjectUpload::select(
                        &cwd.join(row.as_str().unwrap()),
                        UploadKind::WaImage,
                        control,
                    )?,
                ));
            }
        }
        let request = Self {
            action,
            doc: json!(doc),
            uploads,
        };
        if action == WriteAction::Create {
            form::validate(
                &form::build(None, &request.doc, action, control)?,
                &request.doc,
            )?;
        }
        Ok(request)
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
            .filter(|(k, _)| !matches!(k.as_str(), "schema" | "file" | "display_img_files"))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect::<Map<_, _>>();
        forms::public(
            json!({"platform":"dd","resource":"wa","action":self.action,"reference":self.doc.get("sn"),"fields":fields,
            "uploads":self.uploads.iter().map(|(field,u)|json!({"field":field,"selection":u.plan()})).collect::<Vec<_>>(),"remote_read":false,"remote_write":false,"credential_reads":false,
            "execution_requires":["explicit retained DDT","fresh owned full form","strictly greater numeric version for update","live choices","full field and content readback"],"uncertain_submission_replay":false}),
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
impl Editor for WaWrite {
    fn resource(&self) -> Resource {
        Resource::Wa
    }
    fn action(&self) -> WriteAction {
        self.action
    }
    fn original_sn(&self) -> Option<&str> {
        self.doc.get("sn").and_then(Value::as_str)
    }
    fn fields(&self) -> &'static [&'static str] {
        form::FIELDS
    }
    fn verification(&self) -> &'static str {
        "stable_fields_and_original_content"
    }
    fn identity(&self, author: &str) -> Value {
        let mut doc = self.doc.clone();
        for k in ["file", "display_img_files"] {
            doc.as_object_mut().unwrap().remove(k);
        }
        let uploads = self
            .uploads
            .iter()
            .map(|(field, u)| {
                let mut v = u.plan();
                v.as_object_mut().unwrap().remove("file");
                json!({"field":field,"artifact":v})
            })
            .collect::<Vec<_>>();
        json!({"author":author,"request":doc,"uploads":uploads})
    }
    fn prepare(&self, api: &mut impl Api, control: &OperationControl) -> Result<Prepared> {
        let current = if let Some(sn) = self.original_sn() {
            let raw = read::detail(api, Resource::Wa, &Reference::parse(sn)?)?;
            if raw["is_owner"] == false {
                return Err(data(
                    "the selected DD WA is not owned by the retained author",
                ));
            }
            let rows = read::all_author(api, Resource::Wa, forms::game(&raw)?, control)?;
            if raw["is_owner"] != true && publication::selected(&rows, sn)?.is_none() {
                return Err(data(
                    "DD WA ownership is absent from complete author membership",
                ));
            }
            Some(raw)
        } else {
            None
        };
        let mut body = form::build(current.as_ref(), &self.doc, self.action, control)?;
        form::validate(&body, &self.doc)?;
        let game = forms::game(&body)?;
        let before_references = if self.action == WriteAction::Create {
            read::all_author(api, Resource::Wa, game, control)?
                .iter()
                .map(forms::reference)
                .collect::<Result<Vec<_>>>()?
        } else {
            Vec::new()
        };
        let versions = Read::GameVersions(game).execute(api, control)?;
        forms::choices(
            &versions,
            &[body["game_version"].clone()],
            &["value"],
            "$.game_version",
        )?;
        let categories = Read::WaCategories(game).execute(api, control)?;
        let selected = body["category_ids"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|v| v.as_str() != Some("ui_original"))
            .cloned()
            .collect::<Vec<_>>();
        forms::choices(&categories, &selected, &["id"], "$.category_ids")?;
        forms::optional_choices(api, &body, current.as_ref(), game, control)?;
        let games = Read::GameTypes.execute(api, control)?;
        forms::choices(&games, &[json!(game.get())], &["game_type"], "$.game_type")?;
        for (field, u) in &self.uploads {
            let receipt = api.upload(u)?;
            let url = receipt["url"]
                .as_str()
                .ok_or_else(|| data("DD WA upload omitted its permanent URL"))?;
            forms::permanent(url, "$.file")?;
            if field == "file" {
                body["file_path"] = json!(url);
            } else {
                if body.get("display_imgs").is_none() {
                    body["display_imgs"] = json!([]);
                }
                body["display_imgs"]
                    .as_array_mut()
                    .ok_or_else(|| data("DD WA images are not an array"))?
                    .push(json!(url));
            }
        }
        form::validate(&body, &json!({}))?;
        let context = Context {
            resource: Resource::Wa,
            action: self.action,
            game_type: game,
            reference: self.original_sn().map(str::to_owned),
            before_references,
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
