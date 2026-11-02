use super::{
    protocol::{Api, Session, identifier},
    read::Listing,
    *,
};
use crate::{
    contract::input_shape,
    input::{document, invalid},
    mutation::Mutation,
    provider::Provider,
};
use std::path::PathBuf;

pub struct Write {
    pub(super) resource: String,
    pub(super) action: String,
    pub(super) doc: Value,
    pub(super) cwd: PathBuf,
}
impl Write {
    pub fn parse(resource: &str, action: &str, bytes: &[u8], cwd: &Path) -> Result<Self> {
        if !matches!(
            (resource, action),
            ("plugin", "edit" | "update")
                | ("version", "edit" | "delete")
                | ("config" | "wa", "create" | "update" | "edit" | "delete")
        ) {
            return Err(ToolkitError::arguments("unknown Heybox author write"));
        }
        let doc = document(bytes)?;
        input_shape(Provider::Blackbox, resource, action)?.admit(&doc)?;
        let doc = Value::Object(doc);
        for key in ["share_id", "backup_id", "slug"] {
            if let Some(v) = doc.get(key) {
                read::reference(v.as_str().unwrap()).map_err(|_| {
                    invalid(
                        &format!("$.{key}"),
                        "expected the exact opaque ID from its list command",
                    )
                })?;
            }
        }
        match resource {
            "config" if action != "delete" => {
                forms::validate_config(&doc["configuration"], action == "create")?
            }
            "wa" if action != "delete" => {
                forms::validate_wa(&doc["configuration"], action == "create")?
            }
            "wa" => {
                forms::branch(&doc["strappid"], "$.strappid")?;
            }
            "plugin" | "version" => plugin::validate(resource, action, &doc)?,
            _ => {}
        }
        Ok(Self {
            resource: resource.into(),
            action: action.into(),
            doc,
            cwd: cwd.to_owned(),
        })
    }
    pub fn schema(resource: &str, action: &str) -> Value {
        let mut schema = input_shape(Provider::Blackbox, resource, action)
            .unwrap()
            .schema();
        schema.as_object_mut().unwrap().remove("x-validation-scope");
        if matches!(resource, "config" | "wa") && action != "delete" {
            schema["properties"]["configuration"] = forms::schema(resource, action == "create");
        }
        if resource == "config" && matches!(action, "edit" | "update") {
            schema["allOf"] = json!([{"if":{"required":["backup_id"]},"then":{"description":"When backup_id differs from the current share, configuration must explicitly supply KnownAddons, UnknownAddons, WTFList, Materials and Fonts. Current identity is checked during execution."}}]);
        }
        if matches!(resource, "plugin" | "version") {
            plugin::schema(&mut schema);
        }
        schema
    }
    pub fn plan(&self, control: &OperationControl) -> Result<Value> {
        let file = if let Some(path) = self.doc["file"].as_str() {
            Some(plugin::inspect_zip(&self.cwd.join(path), control)?)
        } else {
            None
        };
        Ok(
            json!({"platform":"blackbox","resource":self.resource,"action":self.action,"planned":true,"local_write":false,"credential_reads":false,"remote_read":false,"remote_write":false,"file":file,
            "fields":self.doc.as_object().unwrap().keys().filter(|k|k.as_str()!="schema").collect::<Vec<_>>(),
            "remote_validation":"Execution reads the selected account, current target, options and complete lists. Omitted fields preserve the current record; plan never reads credentials or creates an operation.",
            "scope":if self.resource=="wa" {Some("public")}else{None},"recovery":"Repeat this exact input: pending operations only read the original target; unknown POST/PUT is never resent."}),
        )
    }
    pub fn execute(&self, home: &Path, control: &OperationControl) -> Result<Value> {
        auth::Auth::open(home, control)?.finish(|auth| {
            let mut session = auth.renew(false)?;
            self.run(home, &auth.api, &mut session, control)
        })
    }
    pub(super) fn run(
        &self,
        home: &Path,
        api: &Api,
        session: &mut Session,
        control: &OperationControl,
    ) -> Result<Value> {
        if matches!(self.resource.as_str(), "plugin" | "version") {
            return plugin::execute(self, home, api, session, control);
        }
        let doc = &self.doc;
        let create = self.action == "create";
        let delete = self.action == "delete";
        let wa = self.resource == "wa";
        let target = if create {
            "new"
        } else if wa {
            doc["slug"].as_str().unwrap()
        } else {
            doc["share_id"].as_str().unwrap()
        };
        let mut mutation = open(
            home,
            session,
            &self.resource,
            target,
            &self.action,
            doc,
            control,
        )?;
        if create || delete {
            if let Some(result) = completed(&mutation) {
                return Ok(result);
            }
        }
        let recovered = mutation.pending();
        if !recovered {
            let mut selected_backup = None;
            let (branch, current, previous) = if wa {
                let branch = if delete {
                    doc["strappid"].as_str().unwrap()
                } else {
                    doc["configuration"]["strappid"].as_str().unwrap()
                };
                let rows = read::collect(api, session, Listing::Was(branch.into()))?;
                let current = if create {
                    None
                } else {
                    let summary = rows.iter().find(|r| r["Slug"] == target).ok_or_else(|| {
                        not_found("WA was not found in the current author's branch")
                    })?;
                    Some(read::wa_detail(api, session, summary)?)
                };
                (branch.to_owned(), current, ids(&rows, "Slug")?)
            } else {
                let current = if create {
                    None
                } else {
                    Some(
                        read::execute(api, session, Query::Config(target.into()))?["config"]
                            .clone(),
                    )
                };
                let branch = if delete {
                    forms::backup_branch(current.as_ref().unwrap())?
                } else {
                    let id = doc
                        .get("backup_id")
                        .and_then(identifier)
                        .or_else(|| current.as_ref().and_then(|c| identifier(&c["BackupID"])))
                        .ok_or_else(|| invalid("$.backup_id", "select a backup ID"))?;
                    let backup = read::execute(api, session, Query::Backup(id))?["backup"].clone();
                    let branch = forms::backup_branch(&backup)?;
                    selected_backup = Some(backup);
                    branch
                };
                let rows = read::collect(api, session, Listing::Configs(branch.clone()))?;
                if !create
                    && !rows
                        .iter()
                        .any(|r| identifier(&r["ShareID"]).as_deref() == Some(target))
                {
                    return Err(not_found(
                        "share is absent from the current account's published list",
                    ));
                }
                (branch, current, ids(&rows, "ShareID")?)
            };
            let mut query = Vec::new();
            let (body, expected) = if delete {
                let current = current.as_ref().unwrap();
                if wa && current["AuditState"] == 1 {
                    return Err(ToolkitError::new(
                        "publish.blackbox_review",
                        "WA deletion is disabled during active review",
                        5,
                    ));
                }
                query.push((if wa { "slug" } else { "share_id" }, target.to_owned()));
                if !wa {
                    query.push(("backup_id", identifier(&current["BackupID"]).unwrap()));
                }
                query.push(("delete", "1".into()));
                (json!({}), json!({}))
            } else if wa {
                let form = forms::wa_form(&doc["configuration"], current.as_ref())?;
                let options = read::execute(api, session, Query::WaCategories)?;
                for key in form["categories"].as_array().unwrap() {
                    if !options["items"].as_array().unwrap().iter().any(|g| {
                        g["TagList"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .any(|r| r["Key"] == *key)
                    }) {
                        return Err(invalid(
                            "$.configuration.categories",
                            "select Key values from publish blackbox wa categories",
                        ));
                    }
                }
                if !create {
                    query.push(("slug", target.to_owned()));
                }
                (
                    json!({"configuration":serde_json::to_string(&form)?}),
                    forms::wa_expected(&form),
                )
            } else {
                let id = doc
                    .get("backup_id")
                    .and_then(identifier)
                    .or_else(|| current.as_ref().and_then(|v| identifier(&v["BackupID"])))
                    .unwrap();
                let backup = selected_backup.as_ref().unwrap();
                let form = forms::config_form(&doc["configuration"], backup, current.as_ref())?;
                query.push(("backup_id", id));
                if !create {
                    query.push(("share_id", target.to_owned()));
                }
                (
                    json!({"configuration":serde_json::to_string(&form)?}),
                    forms::config_expected(&form),
                )
            };
            mutation.prepare_with_evidence(json!({"branch":branch,"target":target,"previous":previous,"expected":fingerprints(&expected)?,"delete":delete,"create":create}),control)?;
            let path = match (wa, create) {
                (true, true) => "/wow/create_wa/",
                (true, false) => "/wow/update_wa/",
                (false, true) => "/wow/create_config_share/",
                (false, false) => "/wow/update_config_share/",
            };
            api.post(session, path, &query, &body)
                .map_err(|e| mutation.failure(e))?;
            mutation.acknowledged().map_err(|_| {
                mutation.pending_error(
                    "write accepted; recover the original input to finish its receipt",
                )
            })?;
        }
        let evidence = mutation
            .evidence()
            .cloned()
            .ok_or_else(|| mutation.pending_error("original write evidence is missing"))?;
        let result=(||{
            let branch=evidence["branch"].as_str().ok_or_else(||protocol_error("original write has no branch"))?;
            let rows=read::collect(api,session,if wa {Listing::Was(branch.into())}else{Listing::Configs(branch.into())})?;
            let id_field=if wa {"Slug"}else{"ShareID"};
            if delete {
                if rows.iter().any(|r|identifier(&r[id_field]).as_deref()==Some(target)){return Err(mutation.pending_error("delete needs a complete readback proving absence; repeat the same input to check without resending"));}
                return Ok(json!({"platform":"blackbox","resource":self.resource,"deleted_id":target,"verified":true,"verification_scope":"complete_owned_list_absence","operation_id":mutation.id(),"recovered":recovered}));
            }
            let previous=evidence["previous"].as_array().ok_or_else(||protocol_error("original write has no prior identity set"))?;
            let mut matches=Vec::new();
            for summary in rows {
                let id=identifier(&summary[id_field]).unwrap();
                if (create&&previous.contains(&json!(id)))||(!create&&id!=target){continue;}
                let raw=if wa {read::wa_detail(api,session,&summary)?}else{read::execute(api,session,Query::Config(id.clone()))?["config"].clone()};
                let actual=if wa {raw}else{forms::normalize_config(raw)};
                if matches_fingerprints(&evidence["expected"],&actual)? {matches.push((id,actual));}
            }
            if matches.len()!=1 {return Err(mutation.pending_error(if matches.len()>1 {"creation matches multiple new records; original result remains pending"}else{"write needs the original target metadata readback; repeat the exact input to check without resending"}));}
            let (id,actual)=matches.remove(0);
            let mut result=json!({"platform":"blackbox","resource":self.resource,"id":id,"operation_id":mutation.id(),"verified":true,"verification_scope":"remote_metadata","verified_fields":evidence["expected"].as_object().unwrap().keys().collect::<Vec<_>>(),"recovered":recovered,"readback":crate::output::public(actual,&[])});
            if wa {result["scope"]=json!("public");result["requested_open"]=doc["configuration"]["open"].clone();result["visibility"]=json!({"verified":false,"anonymous_can_view":null});result["unverified_fields"]=json!(["subscribeLevel","screens.thumb","open"]);}
            Ok(result)
        })().map_err(|e|mutation.readback_error("original Heybox write requires readback; no request was replayed",e))?;
        mutation.verified_with(json!({"result":result}))?;
        Ok(result)
    }
}
pub(super) fn not_found(message: &str) -> ToolkitError {
    ToolkitError::new("publish.blackbox_not_found", message, 5)
}
pub(super) use crate::mutation::{completed, digest, fingerprints, matches_fingerprints};
pub(super) fn ids(rows: &[Value], field: &str) -> Result<Vec<String>> {
    rows.iter()
        .map(|r| {
            identifier(&r[field])
                .ok_or_else(|| protocol_error("owned list omitted its record identity"))
        })
        .collect()
}
pub(super) fn open(
    home: &Path,
    session: &Session,
    resource: &str,
    target: &str,
    action: &str,
    doc: &Value,
    control: &OperationControl,
) -> Result<Mutation> {
    let identity = json!({"account":session.identity()?,"target":target,"request":doc});
    let scope = digest(
        &json!({"account":session.identity()?,"target":target,"create_input":if target=="new" {Some(doc)}else{None}}),
    )?;
    let target = u64::from_str_radix(&scope[..16], 16).unwrap().max(1);
    Mutation::open(
        home,
        Provider::Blackbox,
        resource,
        target,
        action,
        &identity,
        control,
    )
}
#[cfg(test)]
mod tests;
