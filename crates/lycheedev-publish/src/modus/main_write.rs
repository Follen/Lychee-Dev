use super::{
    main_form as form,
    protocol::{Api, data, identity, segment},
    *,
};
use crate::{
    contract::input_shape,
    input::{document, invalid},
    mutation::{Mutation, completed, digest, fingerprints, matches_fingerprints},
    provider::Provider,
};
use reqwest::Method;
use std::path::Path;
mod version;

pub struct MainWrite {
    resource: String,
    action: String,
    doc: Value,
}
impl MainWrite {
    pub fn parse(resource: &str, action: &str, bytes: &[u8]) -> Result<Self> {
        if !["config", "wa"].contains(&resource)
            || ![
                "create",
                "update",
                "edit",
                "delete",
                "version-publish",
                "version-delete",
            ]
            .contains(&action)
            || resource == "config" && action.starts_with("version-")
        {
            return Err(ToolkitError::arguments("unknown ModUs main-client write"));
        }
        let doc = document(bytes)?;
        input_shape(Provider::Modus, resource, action)?.admit(&doc)?;
        let doc = Value::Object(doc);
        for key in ["share_id", "import_id", "version_id"] {
            if doc.get(key).is_some_and(|v| identity(v).is_none()) {
                return Err(invalid(
                    &format!("$.{key}"),
                    "supply a nonzero exact resource ID without whitespace",
                ));
            }
        }
        form::validate(&doc)?;
        if action == "create" {
            form::form(&doc, resource == "config", None)?;
        }
        if ["edit", "update"].contains(&action)
            && !form::FIELDS.iter().any(|(local, _)| {
                !["share_id", "import_id"].contains(local) && doc.get(*local).is_some()
            })
        {
            return Err(invalid("$", "supply at least one field to update"));
        }
        Ok(Self {
            resource: resource.into(),
            action: action.into(),
            doc,
        })
    }
    pub fn schema(resource: &str, action: &str) -> Value {
        let mut schema = input_shape(Provider::Modus, resource, action)
            .unwrap()
            .schema();
        schema.as_object_mut().unwrap().remove("x-validation-scope");
        if ["create", "edit", "update"].contains(&action) {
            schema["properties"]["title"]["minLength"] = json!(6);
            schema["properties"]["content_text"]["minLength"] = json!(21);
            schema["dependentRequired"] =
                json!({"platform":["synchronization_type"],"synchronization_type":["platform"]});
            schema["allOf"] = json!([
                {"if":{"properties":{"platform":{"const":3}},"required":["platform"]},"then":{"properties":{"synchronization_type":{"const":3},"required_tier_id":{"type":"null"}}}},
                {"if":{"properties":{"platform":{"const":1}},"required":["platform"]},"then":{"properties":{"synchronization_type":{"const":1}}}}
            ]);
            if action == "create" {
                for (local, value) in [
                    ("is_paid", json!(0)),
                    ("is_public", json!(1)),
                    ("price", json!(0)),
                    ("share_type", json!(0)),
                    ("sub_type", json!(0)),
                    ("platform", json!(1)),
                    ("synchronization_type", json!(1)),
                ] {
                    schema["properties"][local]["default"] = value;
                }
                if resource == "wa" {
                    schema["properties"]["file_path"]["default"] = json!("");
                }
            }
        }
        schema
    }
    pub fn plan(&self) -> Value {
        json!({"platform":"modus","resource":self.resource,"action":self.action,"planned":true,"local_write":false,"credential_reads":false,"remote_read":false,"remote_write":false,"client":"main","server_type":self.doc.get("server_type"),"fields":self.doc.as_object().unwrap().keys().filter(|k|k.as_str()!="schema").collect::<Vec<_>>(),"target":self.doc.get("share_id").or_else(||self.doc.get("import_id")).or_else(||self.doc.get("version_id")),"next":"Execution reads the original detail and current options, preserving omitted fields. Repeat this exact input for read-only recovery; the original build is retained."})
    }
    pub fn execute(&self, home: &Path, control: &OperationControl) -> Result<Value> {
        let api = Api {
            http: crate::transport::Http::new(control)?,
            session: Some(auth::Session::load(Client::Main, control)?),
        };
        self.run(home, &api, control)
    }
    pub(super) fn run(&self, home: &Path, api: &Api, control: &OperationControl) -> Result<Value> {
        let account = api.account()?;
        let create = self.action == "create";
        let delete = self.action == "delete";
        let target = self
            .doc
            .get("version_id")
            .or_else(|| self.doc.get("share_id"))
            .or_else(|| self.doc.get("import_id"))
            .and_then(identity)
            .unwrap_or_else(|| "new".into());
        let scope = digest(
            &json!({"account":account,"client":"main","resource":self.resource,"target":target,"selected_build":self.doc.get("server_type"),"create_input":if create{Some(&self.doc)}else{None}}),
        )?;
        let mut m = Mutation::open(
            home,
            Provider::Modus,
            &format!("main-{}", self.resource),
            u64::from_str_radix(&scope[..16], 16).unwrap().max(1),
            &self.action,
            &json!({"account":account,"input":self.doc}),
            control,
        )?;
        if create || delete || self.action.starts_with("version-") {
            if let Some(done) = completed(&m) {
                return Ok(done);
            }
        }
        let recovered = m.pending();
        let build = if recovered {
            m.evidence()
                .and_then(|e| e["build"].as_u64())
                .filter(|n| *n <= 4)
                .ok_or_else(|| malformed("original main-client operation lost its build"))?
                as u8
        } else {
            api.build(self.doc["server_type"].as_u64().map(|n| n as u8))?
        };
        if self.action.starts_with("version-") {
            return version::run(self, api, &mut m, build, recovered, control);
        }
        let config = self.resource == "config";
        let kind = if config { "share" } else { "import" };
        if !recovered {
            let previous = if create {
                owned(api, config, build)?
            } else {
                vec![]
            };
            let current = if create {
                None
            } else {
                Some(read::detail(api, kind, &target, Some(build))?)
            };
            let wire = if delete {
                json!({})
            } else {
                form::form(&self.doc, config, current.as_ref())?
            };
            if !delete {
                form::options(api, &self.doc, &wire, config, build, &account)?;
            }
            let expected = if delete {
                json!({})
            } else {
                form::normalize(&wire)?
            };
            m.prepare_with_evidence(
                json!({"build":build,"previous":previous,"expected":fingerprints(&expected)?}),
                control,
            )?;
            let route = if delete {
                format!("system/user/{kind}/delete/{}", segment(&target))
            } else {
                format!(
                    "system/user/{kind}/{}",
                    if create { "create" } else { "update" }
                )
            };
            let method = if delete {
                Method::DELETE
            } else if !create && config {
                Method::PUT
            } else {
                Method::POST
            };
            let response = data(
                api.write(
                    method,
                    &route,
                    &[],
                    if delete { None } else { Some(&wire) },
                    Some(build),
                )
                .map_err(|e| m.failure(e))?,
            );
            if create {
                let id = identity(&response)
                    .or_else(|| identity(&response["id"]))
                    .or_else(|| identity(&response[if config { "shareId" } else { "importId" }]));
                if let Some(id) = id {
                    let mut evidence = m.evidence().unwrap().clone();
                    evidence["created_id"] = json!(id);
                    m.remember(evidence)?;
                }
            }
            m.acknowledged()?;
        }
        let result = (|| {
            if delete {
                if read::optional_detail(api,kind,&target,Some(build))?.is_some() {return Err(m.pending_error("original deletion is still visible; repeat this input to read without resending"));}
                return Ok(json!({"platform":"modus","resource":self.resource,"id":target,"server_type":build,"deleted":true,"verified":true,"verification_scope":"exact_detail_absence","operation_id":m.id(),"recovered":recovered}));
            }
            let evidence=m.evidence().ok_or_else(||malformed("original main-client request lost its readback fields"))?;
            let candidates = if !create {vec![target.clone()]} else if let Some(id)=evidence["created_id"].as_str(){vec![id.into()]}
                else {let previous=evidence["previous"].as_array().ok_or_else(||malformed("original creation lost its prior identities"))?;owned(api,config,build)?.into_iter().filter(|id| !previous.contains(&json!(id))).collect()};
            let mut matches=Vec::new();
            for id in candidates {
                if let Some(actual)=read::optional_detail(api,kind,&id,Some(build))? {
                    if matches_fingerprints(&evidence["expected"],&form::normalize(&actual)?)? {matches.push((id,actual));}
                }
            }
            if matches.len()!=1{return Err(m.pending_error("original main-client form is not uniquely confirmed; repeat this input for read-only recovery"));}
            let (id,actual)=matches.remove(0);
            Ok(json!({"platform":"modus","resource":self.resource,"id":id,"server_type":build,"verified":true,"verification_scope":"remote_form_fields","verified_fields":evidence["expected"].as_object().unwrap().keys().collect::<Vec<_>>(),"backup_bytes_verified":false,"media_bytes_verified":false,"operation_id":m.id(),"recovered":recovered,"readback":crate::output::public(actual,&[])}))
        })().map_err(|e|m.readback_error("ModUs main-client write needs original result readback; no write was replayed",e))?;
        m.verified_with(json!({"result":result}))?;
        Ok(result)
    }
}
fn owned(api: &Api, config: bool, build: u8) -> Result<Vec<String>> {
    read::pages(
        api,
        if config {
            Listing::Configs
        } else {
            Listing::Was
        },
        Some(build),
        None,
        100,
        &json!({"mine":true}),
    )?["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| identity(&r["id"]).ok_or_else(|| malformed("owned main-client list omitted ID")))
        .collect()
}

#[cfg(test)]
mod tests;
