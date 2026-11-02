use super::{
    protocol::{Api, data, identity},
    *,
};
use crate::{
    contract::input_shape,
    input::{document, invalid},
    mutation::{Mutation, completed, digest, fingerprints, matches_fingerprints},
    provider::Provider,
};
use std::{collections::BTreeSet, path::Path};

pub struct ProjectWrite {
    action: String,
    doc: Value,
}
impl ProjectWrite {
    pub fn parse(action: &str, bytes: &[u8]) -> Result<Self> {
        if !["create", "edit", "delete"].contains(&action) {
            return Err(ToolkitError::arguments("unknown ModUs project operation"));
        }
        let doc = document(bytes)?;
        input_shape(Provider::Modus, "project", action)?.admit(&doc)?;
        let doc = Value::Object(doc);
        for name in ["name", "summary"] {
            if let Some(s) = doc.get(name).and_then(Value::as_str) {
                if s.trim().is_empty() {
                    return Err(invalid(&format!("$.{name}"), "value must not be blank"));
                }
            }
        }
        if doc.get("license").is_some() {
            license(&doc["license"])?;
        }
        if doc.get("categories").is_some() {
            categories(&doc["categories"], true)?;
        }
        if doc.get("publish_platforms").is_some() {
            platforms(&doc["publish_platforms"])?;
        }
        if doc.get("image_ops").is_some() {
            if doc.get("images").is_none() {
                return Err(invalid("$.images", "images is required with image_ops"));
            }
            image_ops(&doc["image_ops"])?;
        }
        if doc.get("images").is_some() && doc.get("image_ops").is_none() {
            return Err(invalid(
                "$.image_ops",
                "images must accompany explicit image operations",
            ));
        }
        if doc.get("required_tier_id").is_some_and(|v| !v.is_null())
            && doc
                .get("publish_platforms")
                .is_some_and(|p| platforms(p).is_ok_and(|s| s & 2 != 0))
        {
            return Err(invalid(
                "$.required_tier_id",
                "BigFoot selection requires required_tier_id=null",
            ));
        }
        if doc
            .get("categories")
            .is_some_and(|v| v.as_array().is_some_and(|a| a.contains(&json!(998))))
            && doc
                .get("publish_platforms")
                .is_some_and(|p| platforms(p).is_ok_and(|s| s != 2))
        {
            return Err(invalid(
                "$.publish_platforms",
                "category 998 requires only bigfoot",
            ));
        }
        if action == "delete" && doc["project_id"].as_u64().unwrap() < 200000000 {
            return Err(invalid(
                "$.project_id",
                "restricted Creator projects cannot be deleted",
            ));
        }
        if action == "edit" && doc.as_object().unwrap().len() == 2 {
            return Err(invalid("$", "supply at least one editable field"));
        }
        Ok(Self {
            action: action.into(),
            doc,
        })
    }
    pub fn schema(action: &str) -> Value {
        let mut s = input_shape(Provider::Modus, "project", action)
            .unwrap()
            .schema();
        s.as_object_mut().unwrap().remove("x-validation-scope");
        if action != "delete" {
            s["properties"]["categories"] = json!({"type":"array","minItems":1,"maxItems":5,"uniqueItems":true,"items":{"type":"integer","minimum":1},"description":"IDs from publish modus options categories; category 998 requires only bigfoot."});
            s["properties"]["publish_platforms"] = json!({"type":"array","minItems":1,"maxItems":2,"uniqueItems":true,"items":{"enum":["modus","bigfoot"]},"description":"Derives synchronizationType. Any BigFoot selection requires no required tier."});
            s["properties"]["license"] = json!({"type":"object","required":["type"],"additionalProperties":false,"properties":{"type":{"type":"string","minLength":1},"holder":{"type":"string","minLength":1},"year":{"type":"string","minLength":1},"content":{"type":"string","minLength":1}},"allOf":[{"if":{"properties":{"type":{"pattern":"^(?:[cC][uU][sS][tT][oO][mM]|自定义)$"}},"required":["type"]},"then":{"required":["content"]}}],"description":"License type and optional holder/year/content. Custom licenses require nonblank content. Omission on edit preserves the complete license."});
        }
        if action == "edit" {
            s["properties"]["image_ops"] = json!({"type":"array","minItems":1,"items":{"oneOf":[{"type":"object","required":["op","name","base64"],"additionalProperties":false,"properties":{"op":{"const":"upload"},"name":{"type":"string","minLength":1},"base64":{"type":"string","minLength":1}}},{"type":"object","required":["op","name"],"additionalProperties":false,"properties":{"op":{"const":"delete"},"name":{"type":"string","minLength":1}}},{"type":"object","required":["op","from","to"],"additionalProperties":false,"properties":{"op":{"const":"rename"},"from":{"type":"string","minLength":1},"to":{"type":"string","minLength":1}}}]},"description":"Official ordered image operations, with images count supplied separately; byte readback is not exposed by project detail."});
            s["dependentRequired"] = json!({"image_ops":["images"],"images":["image_ops"]});
        }
        s
    }
    pub fn plan(&self) -> Value {
        json!({"platform":"modus","resource":"project","action":self.action,"planned":true,"local_write":false,"credential_reads":false,"remote_read":false,"remote_write":false,"fields":self.doc.as_object().unwrap().keys().filter(|k|k.as_str()!="schema").collect::<Vec<_>>(),"target":self.doc.get("project_id"),"identity_source":"creator","next":"Execution validates current categories/tiers and the exact author project; repeat the original input for read-only recovery of unknown submissions."})
    }
    pub fn execute(&self, home: &Path, control: &OperationControl) -> Result<Value> {
        let api = Api {
            http: crate::transport::Http::new(control)?,
            session: Some(auth::Session::load(Client::Creator, control)?),
        };
        self.run(home, &api, control)
    }
    pub(super) fn run(&self, home: &Path, api: &Api, control: &OperationControl) -> Result<Value> {
        let account = api.account()?;
        let create = self.action == "create";
        let delete = self.action == "delete";
        let id = self.doc["project_id"].as_u64();
        let target = id.map(|n| n.to_string()).unwrap_or_else(|| "new".into());
        let scope = digest(
            &json!({"client":"creator","account":account,"target":target,"create_input":if create{Some(&self.doc)}else{None}}),
        )?;
        let mut m = Mutation::open(
            home,
            Provider::Modus,
            "project",
            u64::from_str_radix(&scope[..16], 16).unwrap().max(1),
            &self.action,
            &json!({"account":account,"target":target,"input":self.doc}),
            control,
        )?;
        if create || delete {
            if let Some(result) = completed(&m) {
                return Ok(result);
            }
        }
        let recovered = m.pending();
        if !recovered {
            let owned = owned(api)?;
            if let Some(id) = id {
                if !owned.contains(&id.to_string()) {
                    return Err(ToolkitError::new(
                        "publish.modus_not_owned",
                        "project is absent from the complete Creator author list",
                        5,
                    ));
                }
            }
            let current = id.map(|id| read::project(api, id)).transpose()?;
            let form = if delete {
                json!({})
            } else {
                form(&self.doc, create, current.as_ref())?
            };
            if !delete {
                validate_options(api, &self.doc, &form)?;
            }
            let mut expected = expected(&form)?;
            if create {
                expected["requiredTierId"] = self
                    .doc
                    .get("required_tier_id")
                    .cloned()
                    .unwrap_or(Value::Null);
            }
            m.prepare_with_evidence(json!({"expected":fingerprints(&expected)?,"previous":owned,"media_requested":form.get("imagesOps").is_some()||form.pointer("/screenshotBase64sReqs/screenshotBase64s").and_then(Value::as_str).is_some_and(|s|!s.is_empty())}),control)?;
            let route = if create {
                "game/data/author/project/release".into()
            } else if delete {
                format!("game/data/author/project/delete/project/{}", id.unwrap())
            } else {
                "game/data/author/project/update".into()
            };
            api.write(
                reqwest::Method::POST,
                &route,
                &[],
                if delete { None } else { Some(&form) },
                None,
            )
            .map_err(|e| m.failure(e))?;
            let mut evidence = m.evidence().unwrap().clone();
            evidence["media_acknowledged"] = json!(true);
            m.remember(evidence)?;
            m.acknowledged()?;
        }
        let result=(||{
            let evidence=m.evidence().ok_or_else(||malformed("original project request lost its readback fields"))?;
            if evidence["media_requested"]==true&&evidence["media_acknowledged"]!=true{return Err(m.pending_error("project image submission is uncertain; project metadata cannot prove image bytes or image rename/delete, and the image request will not be replayed"));}
            let owned=owned(api)?;
            if delete {
                if owned.contains(&target){return Err(m.pending_error("project deletion is pending; repeat this input to read the original author list without resending"));}
                return Ok(json!({"platform":"modus","project_id":id,"deleted":true,"verified":true,"verification_scope":"complete_owned_list_absence","operation_id":m.id(),"recovered":recovered}));
            }
            let previous=evidence["previous"].as_array().ok_or_else(||malformed("original project request lost its prior identity set"))?;
            let candidates=if create{owned.into_iter().filter(|id|!previous.contains(&json!(id))).collect::<Vec<_>>()}else{vec![target.clone()]};
            let mut found=Vec::new();
            for id in candidates {
                let number=id.parse::<u64>().map_err(|_|malformed("Creator project ID is not a positive integer"))?;
                let actual=read::project(api,number)?;
                if matches_fingerprints(&evidence["expected"],&normalize(&actual)?)?{found.push((number,actual));}
            }
            if found.len()!=1{return Err(m.pending_error("original project fields are not uniquely confirmed; repeat this exact input to read without another write"));}
            let (id,actual)=found.remove(0);
            Ok(json!({"platform":"modus","project_id":id,"verified":true,"verification_scope":"remote_metadata","verified_fields":evidence["expected"].as_object().unwrap().keys().collect::<Vec<_>>(),"media_bytes_verified":false,"media_requested":evidence["media_requested"],"operation_id":m.id(),"recovered":recovered,"readback":crate::output::public(actual,&[])}))
        })().map_err(|e|m.readback_error("ModUs project write needs original target readback; no write was replayed",e))?;
        m.verified_with(json!({"result":result}))?;
        Ok(result)
    }
}
pub(super) fn owned(api: &Api) -> Result<Vec<String>> {
    read::pages(api, Listing::Projects, None, None, 100, &json!({}))?["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            identity(
                r.get("projectId")
                    .or_else(|| r.get("id"))
                    .unwrap_or(&Value::Null),
            )
            .ok_or_else(|| malformed("Creator author list omitted project ID"))
        })
        .collect()
}
fn platforms(value: &Value) -> Result<u64> {
    let items = value
        .as_array()
        .ok_or_else(|| invalid("$.publish_platforms", "expected modus/bigfoot array"))?;
    let mut mask = 0;
    for item in items {
        let flag = match item.as_str() {
            Some("modus") => 1,
            Some("bigfoot") => 2,
            _ => {
                return Err(invalid(
                    "$.publish_platforms",
                    "platform must be modus or bigfoot",
                ));
            }
        };
        if mask & flag != 0 {
            return Err(invalid("$.publish_platforms", "duplicate platform"));
        }
        mask |= flag;
    }
    if mask == 0 {
        return Err(invalid(
            "$.publish_platforms",
            "select at least one platform",
        ));
    }
    Ok(mask)
}
fn categories(value: &Value, strict: bool) -> Result<Vec<u64>> {
    let raw = if let Some(items) = value.as_array() {
        items.clone()
    } else if !strict {
        value
            .as_str()
            .map(|s| {
                s.split(',')
                    .filter(|s| !s.trim().is_empty())
                    .map(|s| json!(s.trim()))
                    .collect()
            })
            .ok_or_else(|| malformed("Creator category selection is missing"))?
    } else {
        return Err(invalid("$.categories", "expected category IDs"));
    };
    let mut set = BTreeSet::new();
    for item in raw {
        let item = if !strict && item.is_object() {
            item.get("id")
                .or_else(|| item.get("categoryId"))
                .or_else(|| item.get("value"))
                .cloned()
                .unwrap_or(Value::Null)
        } else {
            item
        };
        let id = item
            .as_u64()
            .or_else(|| {
                if strict {
                    None
                } else {
                    item.as_str().and_then(|s| s.parse().ok())
                }
            })
            .filter(|n| *n > 0)
            .ok_or_else(|| invalid("$.categories", "category IDs must be positive integers"))?;
        if !set.insert(id) && strict {
            return Err(invalid("$.categories", "duplicate category ID"));
        }
    }
    if strict && (set.is_empty() || set.len() > 5) {
        return Err(invalid("$.categories", "select one to five categories"));
    }
    Ok(set.into_iter().collect())
}
fn license(value: &Value) -> Result<Value> {
    let obj = value
        .as_object()
        .ok_or_else(|| invalid("$.license", "license must be an object"))?;
    if obj
        .keys()
        .any(|k| !["type", "holder", "year", "content"].contains(&k.as_str()))
    {
        return Err(invalid("$.license", "unknown license field"));
    }
    for (k, v) in obj {
        if !v.as_str().is_some_and(|s| !s.trim().is_empty()) {
            return Err(invalid(
                &format!("$.license.{k}"),
                "license value must be a nonblank string",
            ));
        }
    }
    let kind = obj
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("$.license.type", "select a license type"))?;
    if (kind.eq_ignore_ascii_case("custom") || kind == "自定义") && !obj.contains_key("content")
    {
        return Err(invalid(
            "$.license.content",
            "custom license requires content",
        ));
    }
    Ok(value.clone())
}
fn image_ops(value: &Value) -> Result<()> {
    for item in value
        .as_array()
        .ok_or_else(|| invalid("$.image_ops", "expected image operations"))?
    {
        let allowed: &[&str] = match item["op"].as_str() {
            Some("upload") => &["op", "name", "base64"],
            Some("delete") => &["op", "name"],
            Some("rename") => &["op", "from", "to"],
            _ => {
                return Err(invalid(
                    "$.image_ops",
                    "op must be upload, delete or rename",
                ));
            }
        };
        let obj = item
            .as_object()
            .ok_or_else(|| invalid("$.image_ops", "image operation must be an object"))?;
        if obj.len() != allowed.len()
            || allowed.iter().any(|k| {
                !obj.get(*k)
                    .and_then(Value::as_str)
                    .is_some_and(|s| !s.trim().is_empty())
            })
        {
            return Err(invalid(
                "$.image_ops",
                "operation fields must match its declared shape",
            ));
        }
    }
    Ok(())
}
fn clear(value: &Value) -> Value {
    if value.is_null() || value.as_str().is_some_and(|s| s.trim().is_empty()) {
        json!("<null>")
    } else {
        value.clone()
    }
}
fn form(doc: &Value, create: bool, current: Option<&Value>) -> Result<Value> {
    let mut out = json!({});
    if let Some(id) = doc.get("project_id") {
        out["projectId"] = id.clone();
    }
    for (key, wire) in [
        ("name", "name"),
        ("alt_name", "altName"),
        ("summary", "summary"),
        ("repo_url", "repoUrl"),
        ("description", "description"),
        ("required_dependencies", "requiredDependencies"),
    ] {
        if let Some(value) = doc.get(key) {
            if create && key == "repo_url" && value.as_str().is_some_and(|s| s.trim().is_empty()) {
                continue;
            }
            out[wire] = if create || key == "name" {
                value.clone()
            } else {
                clear(value)
            };
        }
    }
    if let Some(v) = doc.get("categories") {
        out["categories"] = json!(
            categories(v, true)?
                .iter()
                .map(u64::to_string)
                .collect::<Vec<_>>()
                .join(",")
        );
    }
    if let Some(v) = doc.get("publish_platforms") {
        out["synchronizationType"] = json!(platforms(v)?);
    }
    if let Some(v) = doc.get("license") {
        out["license"] = json!(serde_json::to_string(&license(v)?)?);
    }
    if let Some(v) = doc.get("required_tier_id") {
        if v.is_null() {
            if !create {
                out["requiredTierId"] = json!("<null>");
            }
        } else {
            out["requiredTierId"] = if create {
                v.clone()
            } else {
                json!(v.as_u64().unwrap().to_string())
            };
        }
    }
    if create {
        out["images"] = json!(0);
        out["screenshotBase64sReqs"] = json!({"name":"logo.webp","screenshotBase64s":doc.get("logo_base64").cloned().unwrap_or(json!(""))});
    }
    if let Some(v) = doc.get("image_ops") {
        out["imagesOps"] = v.clone();
        out["images"] = doc["images"].clone();
    }
    let current = current.map(normalize).transpose()?.unwrap_or(json!({}));
    let effective_sync = out
        .get("synchronizationType")
        .or_else(|| current.get("synchronizationType"))
        .and_then(Value::as_u64);
    let tier = doc
        .get("required_tier_id")
        .or_else(|| current.get("requiredTierId"));
    if effective_sync.is_some_and(|s| s & 2 != 0)
        && tier.is_some_and(|v| !v.is_null() && v != "<null>")
    {
        return Err(invalid(
            "$.required_tier_id",
            "BigFoot publishing requires explicit clearing of the current required tier",
        ));
    }
    let cats = doc.get("categories").or_else(|| current.get("categories"));
    if cats.is_some_and(|v| v.as_array().is_some_and(|a| a.contains(&json!(998))))
        && effective_sync != Some(2)
    {
        return Err(invalid(
            "$.publish_platforms",
            "category 998 requires only bigfoot",
        ));
    }
    if doc["project_id"].as_u64().is_some_and(|n| n < 200000000) {
        let expected = expected(&out)?;
        for name in [
            "name",
            "categories",
            "license",
            "synchronizationType",
            "requiredTierId",
            "repoUrl",
            "requiredDependencies",
        ] {
            if let Some(v) = expected.get(name) {
                if current.get(name) != Some(v) {
                    return Err(invalid(
                        "$",
                        "restricted projects allow only summary, description and alt_name edits",
                    ));
                }
            }
        }
        if out.get("imagesOps").is_some() {
            return Err(invalid(
                "$.image_ops",
                "restricted Creator project images cannot be edited",
            ));
        }
        out.as_object_mut().unwrap().retain(|k, _| {
            ["projectId", "summary", "description", "altName"].contains(&k.as_str())
        });
    }
    Ok(out)
}
fn normalize(value: &Value) -> Result<Value> {
    let mut out = value.clone();
    let obj = out
        .as_object_mut()
        .ok_or_else(|| malformed("Creator project detail must be an object"))?;
    for name in ["altName", "repoUrl", "description", "requiredDependencies"] {
        let v = obj.entry(name).or_insert(Value::Null);
        if v.is_null() || v == "<null>" {
            *v = json!("");
        }
    }
    if let Some(v) = obj
        .get("categories")
        .or_else(|| obj.get("categorieStrings"))
        .cloned()
    {
        obj.insert("categories".into(), json!(categories(&v, false)?));
    }
    for name in ["synchronizationType", "requiredTierId"] {
        if let Some(v) = obj.get(name).cloned() {
            let n = v
                .as_u64()
                .or_else(|| v.as_str().and_then(|s| s.parse().ok()));
            obj.insert(name.into(), n.map(|n| json!(n)).unwrap_or(Value::Null));
        }
    }
    let raw = obj
        .get("license")
        .or_else(|| obj.get("licenseContent"))
        .cloned()
        .unwrap_or_else(|| json!({"type":obj.get("licenseName").cloned().unwrap_or(Value::Null)}));
    let raw = if let Some(s) = raw.as_str() {
        serde_json::from_str(s).unwrap_or_else(|_| json!({"type":s}))
    } else {
        raw
    };
    let mut lic = json!({});
    for name in ["type", "holder", "year", "content"] {
        if let Some(v) = raw
            .get(name)
            .filter(|v| !v.is_null() && v.as_str().is_some_and(|s| !s.is_empty()))
        {
            lic[name] = v.clone();
        }
    }
    obj.insert("license".into(), lic);
    obj.entry("requiredTierId").or_insert(Value::Null);
    Ok(out)
}
fn expected(form: &Value) -> Result<Value> {
    let mut out = normalize(form)?;
    out.as_object_mut().unwrap().retain(|k, _| {
        form.get(k).is_some()
            && !["projectId", "imagesOps", "screenshotBase64sReqs"].contains(&k.as_str())
    });
    Ok(out)
}
fn option_rows(value: &Value) -> Result<Vec<Value>> {
    let mut pending = read::rows(value)?;
    let mut rows = Vec::new();
    while let Some(item) = pending.pop() {
        if let Some(children) = item.get("children").filter(|v| !v.is_null()) {
            pending.extend(read::rows(children)?);
        }
        rows.push(item);
    }
    Ok(rows)
}
fn validate_options(api: &Api, doc: &Value, form: &Value) -> Result<()> {
    if let Some(cats) = doc.get("categories") {
        let options = option_rows(&data(api.get("plugin/list/Categories", &[], None)?))?;
        for id in categories(cats, true)? {
            if !options
                .iter()
                .any(|r| identity(&r["id"]) == Some(id.to_string()))
            {
                return Err(invalid(
                    "$.categories",
                    "select current IDs from publish modus options categories",
                ));
            }
        }
    }
    if let Some(tier) = form
        .get("requiredTierId")
        .filter(|v| !v.is_null() && *v != "<null>")
    {
        let options = read::rows(&data(api.get(
            "user/author/subscription/tiers",
            &[],
            None,
        )?))?;
        if !options.iter().any(|r| {
            identity(&r["id"]) == identity(tier)
                && r["isEnabled"] != 0
                && r["isEnabled"] != false
                && r["is_enabled"] != 0
                && r["is_enabled"] != false
        }) {
            return Err(invalid(
                "$.required_tier_id",
                "select an enabled tier from publish modus options subscription-tiers",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
