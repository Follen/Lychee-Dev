use super::*;

// Official ModUs 1.2.23 edit-string-article uses IMPORT_DETAIL.versionList.
// No separate historical-version endpoint or implicit write retry is needed.
fn versions(detail: &Value) -> Result<Vec<Value>> {
    let rows=detail.get("versionList").and_then(Value::as_array).ok_or_else(||malformed("WA detail omitted versionList; query wa get for this import before deleting history"))?;
    let mut seen = std::collections::BTreeSet::new();
    for row in rows {
        let id = version_id(row).ok_or_else(|| malformed("WA history omitted a version ID"))?;
        if !seen.insert(id) {
            return Err(malformed("WA history repeated a version ID"));
        }
    }
    Ok(rows.clone())
}
fn version_id(row: &Value) -> Option<String> {
    row.get("versionId")
        .or_else(|| row.get("id"))
        .and_then(identity)
}
pub(super) fn run(
    request: &MainWrite,
    api: &Api,
    m: &mut Mutation,
    build: u8,
    recovered: bool,
    control: &OperationControl,
) -> Result<Value> {
    let delete = request.action == "version-delete";
    if !recovered {
        let candidates = if let Some(id) = request.doc.get("import_id").and_then(identity) {
            vec![id]
        } else {
            owned(api, false, build)?
        };
        let mut found = Vec::new();
        for parent in candidates {
            let detail = read::detail(api, "import", &parent, Some(build))?;
            if !delete
                || versions(&detail)?
                    .iter()
                    .any(|v| version_id(v) == identity(&request.doc["version_id"]))
            {
                found.push((parent, detail));
            }
        }
        if found.len() != 1 {
            return Err(invalid(
                "$.version_id",
                "version must identify one WA history entry; supply import_id from wa list/get to select its parent",
            ));
        }
        let (parent, current) = found.remove(0);
        let mut wire = json!({});
        if !delete {
            wire = json!({"importId":parent,"version":request.doc["version"].as_str().unwrap().trim(),"codeText":request.doc["code_text"].as_str().unwrap().trim()});
            if let Some(changelog) = request.doc["changelog"]
                .as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                wire["changelog"] = json!(changelog);
            }
        }
        let mut expected = wire.clone();
        expected.as_object_mut().unwrap().remove("importId");
        let previous = versions(&current)?
            .iter()
            .filter_map(version_id)
            .collect::<Vec<_>>();
        m.prepare_with_evidence(json!({"build":build,"parent":parent,"expected":fingerprints(&expected)?,"previous":previous,"previous_current_matches":matches_fingerprints(&fingerprints(&expected)?,&current)?}),control)?;
        api.write(
            if delete { Method::DELETE } else { Method::POST },
            if delete {
                "system/user/import/version/delete"
            } else {
                "system/user/import/version/publish"
            },
            &if delete {
                vec![("versionId", identity(&request.doc["version_id"]).unwrap())]
            } else {
                vec![]
            },
            if delete { None } else { Some(&wire) },
            Some(build),
        )
        .map_err(|e| m.failure(e))?;
        let mut evidence = m.evidence().unwrap().clone();
        evidence["acknowledged"] = json!(true);
        m.remember(evidence)?;
        m.acknowledged()?;
    }
    let result=(||{
        let e=m.evidence().ok_or_else(||malformed("original WA version operation lost its parent"))?;
        let parent=e["parent"].as_str().ok_or_else(||malformed("original WA version operation lost its parent"))?;
        let current=read::detail(api,"import",parent,Some(build))?;
        let history=versions(&current)?;
        let mut matched_id=None;
        if delete {
            if history.iter().any(|v|version_id(v)==identity(&request.doc["version_id"])) {return Err(m.pending_error("original WA version deletion is still visible; no deletion was repeated"));}
            matched_id=identity(&request.doc["version_id"]);
        } else {
            let previous=e["previous"].as_array().ok_or_else(||malformed("original WA version operation lost prior history IDs"))?;
            let new_history=history.iter().filter(|v|version_id(v).is_some_and(|id|!previous.contains(&json!(id)))).filter_map(|v| match matches_fingerprints(&e["expected"],v){Ok(true)=>Some(Ok(v)),Ok(false)=>None,Err(e)=>Some(Err(e))}).collect::<Result<Vec<_>>>()?;
            let latest_matches=matches_fingerprints(&e["expected"],&current)? && (e["previous_current_matches"]!=true || e["acknowledged"]==true);
            if new_history.len()>1 || new_history.is_empty() && !latest_matches{return Err(m.pending_error("original WA version contents are not uniquely confirmed; no version was republished"));}
            if let Some(row)=new_history.first(){matched_id=version_id(row);}
            else if latest_matches {matched_id=current.get("versionId").and_then(identity);}
        }
        Ok(json!({"platform":"modus","resource":"wa","action":request.action,"import_id":parent,"version_id":matched_id,"server_type":build,"verified":true,"deleted":delete,"verification_scope":if delete{"parent_version_list_absence"}else{"version_and_code_fields"},"operation_id":m.id(),"recovered":recovered}))
    })().map_err(|e|m.readback_error("WA version needs original-parent readback; no write was repeated",e))?;
    m.verified_with(json!({"result":result}))?;
    Ok(result)
}
