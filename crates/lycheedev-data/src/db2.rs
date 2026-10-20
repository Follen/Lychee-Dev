//! Public table profiles share authenticated local or explicitly selected CDN
//! file/DBD/WDC providers, retaining authenticated missing-section provenance.
use crate::{
    capture, definitions, fault, local_query,
    resource::{Budget, Cost},
    schema, table,
};
use lycheedev_core::{Invocation, Outcome, Result, evidence, selection};
use serde_json::json;
use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, Instant},
};

#[cfg(test)]
#[path = "db2/source_freshness_tests.rs"]
mod source_freshness_tests;

#[cfg(test)]
#[path = "db2/reader_limits_tests.rs"]
mod reader_limits_tests;

#[cfg(any(test, feature = "migration-oracle"))]
pub fn dispatch(inv: &Invocation) -> Result<Outcome> {
    dispatch_after_read(inv, &mut || Ok(()))
}
#[cfg(any(test, feature = "migration-oracle"))]
pub(crate) fn dispatch_after_read(
    inv: &Invocation,
    after_read: &mut impl FnMut() -> Result<()>,
) -> Result<Outcome> {
    dispatch_finishing(inv, after_read, |scope, result| scope.finish(result))
}
// Tests may observe the real consuming finish without replacing IO or capture.
#[cfg(any(test, feature = "migration-oracle"))]
pub(crate) fn dispatch_finishing(
    inv: &Invocation,
    after_read: &mut impl FnMut() -> Result<()>,
    finish: impl for<'a> FnOnce(crate::remote::provider::Scope<'a>, Result<Outcome>) -> Result<Outcome>,
) -> Result<Outcome> {
    inv.control.check()?;
    match inv.command.as_str() {
        "data db2" => {
            if let Some(id) = inv.flag("--id") {
                uint(id)?;
            } else {
                inv.usize_flag("--limit", 50, 1, 200)?;
                if let Some(after) = inv.flag("--after-id") {
                    uint(after)?;
                }
            }
        }
        "data db2 search" | "data db2 foreign-key" => {
            inv.required("--field")?;
            inv.required("--limit")?;
            inv.usize_flag(
                "--limit",
                0,
                1,
                if inv.command == "data db2 search" {
                    100000
                } else {
                    5000
                },
            )?;
            if inv.command == "data db2 search" {
                inv.required("--query")?;
            } else {
                uint(inv.required("--value")?)?;
            }
        }
        "data db2 schema" => {}
        _ => return Err(fault("command.unsupported", 3, "Unknown DB2 profile")),
    }
    let name = if inv.command == "data db2" {
        inv.required("--table")?
    } else {
        inv.argument()?
    };
    let timeout = inv.usize_flag("--timeout-seconds", 300, 1, 3600)?;
    let budget = Arc::new(Budget::query_controlled(
        Instant::now() + Duration::from_secs(timeout as u64),
        inv.control.clone(),
    ));
    // The shared owner is source-neutral. Ordinary record/page checks its pin
    // before admission; navigator profiles check it only after admission.
    let opened = crate::remote::provider::OpenedScope::open(inv, &budget)?;
    let (mut scope, resolved) = if inv.command == "data db2" {
        let (scope, pin) =
            opened.admit_pinned(fault("command.failed", 5, "records.data_pin_required"))?;
        (scope, Some(pin))
    } else {
        (opened.admit()?, None)
    };
    let mut committed = Vec::new();
    let result = (|| {
        let resolved = match resolved {
            Some(pin) => pin,
            None => scope.resolve_pin()?,
        };
        let data = resolved
            .data
            .as_ref()
            .ok_or_else(|| fault("command.failed", 5, "records.data_pin_required"))?;
        scope.check_pin(data)?;
        let mut execute = |reader: &mut dyn crate::remote::provider::FileScope| {
            let Prepared {
                pin,
                bundle,
                file,
                layout,
                definition,
                availability,
                limits,
            } = prepare_scoped(
                inv,
                name,
                &budget,
                Some(&mut *reader),
                Some(resolved.clone()),
            )?;
            let data = pin.data.as_ref().expect("prepare checked DataPin");
            let view = table::open_budgeted(
                &file.bytes,
                &definition,
                limits,
                &availability.skipped,
                &availability.ranges,
                budget.clone(),
            )?;
            budget.check()?;
            let logical = view.logical_count();
            let partial = !view.is_complete();
            let mut proof = json!({"contentVerified":file.reading["contentVerified"],"keySource":file.reading["keySource"],"name":name,"identity":bundle.identity,"layoutHash":format!("{:08X}",layout.layout_hash),"definitionSHA256":definition.sha256,"fileDataID":bundle.identity.db2_file_data_id,"contentKey":file.reading["entry"]["contentKey"],"logicalRows":logical});
            if partial {
                proof["partialContent"] = file.reading["partialContent"].clone();
                proof["missing"] = file.reading["missing"].clone();
                proof["unavailablePartitions"] = json!(availability.partitions);
            }
            let context = || json!({"snapshot":pin.id,"pin":data,"source":file.reading["source"],"tables":[proof.clone()],"relations":[],"complete":!partial,"truncated":false,"truncationReason":"none","partial":partial,"partialReasons":if partial {json!(["encrypted_sections_unavailable"])} else {serde_json::Value::Null}});
            let (mut result, kind, locator) = match inv.command.as_str() {
                "data db2" => {
                    let mut reading = json!({"complete":!partial,"truncated":false,"file":file.reading,"sources":bundle,"layoutHash":format!("{:08X}",layout.layout_hash),"schema":definition});
                    if partial {
                        reading["unavailablePartitions"] = json!(availability.partitions);
                    }
                    if let Some(id) = inv.flag("--id") {
                        let id = uint(id)?;
                        let row = view.row(id, 1 << 20).or_else(|error| {
                            if error.code != "table.record_not_found" {
                                return Err(error);
                            }
                            // Go upgrades absence only at the direct DB2 record
                            // boundary. SQL/navigation keep missing lookup rows
                            // and report partial coverage on their whole result.
                            if availability.record_unavailable(id, &budget)? {
                                Err(fault(
                                    "container.key_unavailable",
                                    3,
                                    "Requested WDC record requires an unavailable encryption key",
                                ))
                            } else {
                                Err(fault("command.failed", 5, "table.record_not_found"))
                            }
                        })?;
                        budget.charge(Cost::retained(row_cost(&row, &budget)?))?;
                        reading["recordID"] = json!(id);
                        reading["row"] = serde_json::to_value(row)?;
                        (
                            reading,
                            "data-record",
                            format!("db2:{}:{id}", bundle.identity.name),
                        )
                    } else {
                        let limit = inv.usize_flag("--limit", 50, 1, 200)?;
                        let after = inv.flag("--after-id").map(uint).transpose()?;
                        let ids: Vec<_> = view
                            .ids()
                            .filter(|id| after.is_none_or(|a| *id > a))
                            .take(limit + 1)
                            .collect();
                        let more = ids.len() > limit;
                        let ids = &ids[..ids.len().min(limit)];
                        let mut rows = vec![];
                        let mut encoded = 2;
                        for id in ids {
                            budget.check()?;
                            let row = view.row(*id, 1 << 20)?;
                            let bytes = capture::json_size(&row, &budget)?;
                            budget.charge(Cost::retained(bytes * 4 + row.len() * 128))?;
                            encoded += bytes + usize::from(!rows.is_empty());
                            if encoded > 8 << 20 {
                                return Err(fault(
                                    "table.limit_exceeded",
                                    3,
                                    "DB2 page row-array byte budget exceeded",
                                ));
                            }
                            rows.push(row);
                        }
                        reading["complete"] = json!(!partial && after.is_none() && !more);
                        reading["truncated"] = json!(more);
                        reading["page"] = json!({"after":after,"next":if more{ids.last().copied()}else{None},"more":more,"ids":ids,"rows":rows});
                        (
                            reading,
                            "data-page",
                            format!("db2:{}", bundle.identity.name),
                        )
                    }
                }
                "data db2 schema" => {
                    let mut result = context();
                    let mut fields = vec![];
                    let mut identities = vec![];
                    let mut relations = vec![];
                    let mut arrays = vec![];
                    for field in &definition.fields {
                        let mut value = serde_json::to_value(field)?;
                        value["type"] = json!(legacy_type(field));
                        for key in ["foreignTable", "foreignColumn"] {
                            if value[key] == "" {
                                value.as_object_mut().unwrap().remove(key);
                            }
                        }
                        fields.push(value);
                        if field.identity {
                            identities.push(field.name.clone());
                        }
                        if field.relation || !field.foreign_table.is_empty() {
                            relations.push(field.name.clone());
                        }
                        if field.array {
                            arrays.push(field.name.clone());
                        }
                    }
                    result["table"] = json!(bundle.identity.name);
                    result["rowCount"] = json!(logical);
                    result["fields"] = json!(fields);
                    result["keys"] = json!(identities);
                    result["relationshipFields"] = json!(relations);
                    result["arrayFields"] = json!(arrays);
                    (result, "data-schema", format!("db2:{name}:schema"))
                }
                "data db2 search" | "data db2 foreign-key" => {
                    let search = inv.command == "data db2 search";
                    let field = inv.required("--field")?;
                    let definition_field = definition
                        .fields
                        .iter()
                        .find(|f| f.name == field)
                        .ok_or_else(|| fault("records.field_unknown", 4, "Unknown DB2 field"))?;
                    if !search && (definition_field.kind != "int" || definition_field.array) {
                        let mut error = fault(
                            "records.field_type_mismatch",
                            2,
                            format!(
                                "records.field_type_mismatch: {field} is {}",
                                definition_field.kind
                            ),
                        );
                        error.details = json!({"stage":"query"});
                        return Err(error);
                    }
                    let maximum = if search { 100000 } else { 5000 };
                    inv.required("--limit")?;
                    let limit = inv.usize_flag("--limit", 0, 1, maximum)?;
                    let query = if search { inv.required("--query")? } else { "" };
                    let want = if search {
                        0
                    } else {
                        uint(inv.required("--value")?)?
                    };
                    budget.charge(Cost::retained(query.len().checked_mul(4).ok_or_else(
                        || {
                            fault(
                                "records.query_resource_budget",
                                3,
                                "Search term size overflow",
                            )
                        },
                    )?))?;
                    let needle: String = query
                        .chars()
                        .map(|c| crate::unicode_simple::map(c, false))
                        .collect();
                    let mut rows = vec![];
                    let mut extra = false;
                    for id in view.ids() {
                        budget.check()?;
                        budget.charge(Cost::work(1))?;
                        let row = view.row(id, 1 << 20)?;
                        let value = row.get(field).ok_or_else(|| {
                            fault("records.field_unknown", 4, "Unknown decoded DB2 field")
                        })?;
                        let matches = if search {
                            let text = search_text(value);
                            budget.charge(Cost::retained(
                                text.len().checked_mul(4).ok_or_else(|| {
                                    fault(
                                        "records.query_resource_budget",
                                        3,
                                        "Search text size overflow",
                                    )
                                })?,
                            ))?;
                            let lower: String = text
                                .chars()
                                .map(|c| crate::unicode_simple::map(c, false))
                                .collect();
                            lower.contains(&needle)
                        } else {
                            match value {
                                table::Value::Int(v) => *v >= 0 && *v as u64 == want as u64,
                                table::Value::UInt(v) => *v == want as u64,
                                _ => false,
                            }
                        };
                        if !matches {
                            continue;
                        }
                        if rows.len() == limit {
                            extra = true;
                            break;
                        }
                        budget.charge(Cost::retained(row_cost(&row, &budget)?))?;
                        rows.push(row);
                    }
                    let mode = if search { "search" } else { "foreign-key" };
                    let mut result = context();
                    result["table"] = json!(bundle.identity.name);
                    result["mode"] = json!(mode);
                    result["field"] = json!(field);
                    result["limit"] = json!(limit);
                    result["count"] = json!(rows.len());
                    result["rows"] = serde_json::to_value(rows)?;
                    result["complete"] = json!(!partial && !extra);
                    result["truncated"] = json!(extra);
                    if extra {
                        result["truncationReason"] = json!("limit_reached");
                    }
                    if search {
                        result["query"] = json!(query);
                    } else {
                        result["value"] = json!(want);
                    }
                    (
                        result,
                        if search {
                            "data-search"
                        } else {
                            "data-foreign-key"
                        },
                        if search {
                            format!("db2:{name}:search:{field}")
                        } else {
                            format!("db2:{name}:foreign-key:{field}={want}")
                        },
                    )
                }
                _ => return Err(fault("command.unsupported", 3, "Unknown DB2 profile")),
            };
            after_read()?;
            reader.check_consumer(inv, &budget)?;
            reader.verify_source()?;
            budget.check()?;
            if inv.command == "data db2" {
                // Existing ordinary record/page diagnostics retain their shape.
                result["resources"] = serde_json::to_value(budget.snapshot()?)?;
            } else {
                result["resources"] = serde_json::to_value(budget.logical_snapshot()?)?;
                result["memoryBudget"] = serde_json::to_value(budget.memory_snapshot()?)?;
            }
            result["resourceAccounting"] = json!({"wholeQueryAccountingComplete":false,"strategy":"conservative cumulative admission","wdcMetadataAdmittedBytes":view.admitted_metadata_bytes(),"remainingGaps":["uninterruptible single codec call"]});
            let raw = capture::json_bytes(&result, &budget, 16 << 20, || {
                fault(
                    "records.query_resource_budget",
                    3,
                    "DB2 result evidence budget exceeded",
                )
            })?;
            let complete = result["complete"] == true;
            let truncated = result["truncated"] == true;
            // Admit the small retained identity before publication.
            capture::admit_committed_identity(&budget)?;
            let capture = evidence::commit_capture_budgeted(
                &inv.home,
                evidence::CaptureInput {
                    bytes: &raw,
                    maximum_bytes: 16 << 20,
                    media_type: "application/json",
                    provenance: json!({"kind":kind,"locator":locator,"snapshot":pin.id,"dataBuild":data.full_build}),
                    complete,
                    truncated,
                },
                &inv.control,
                budget.as_ref(),
            )?;
            committed.push(
                capture["id"]
                    .as_str()
                    .expect("committed query capture identity")
                    .to_owned(),
            );
            let mut outcome = Outcome::new(result);
            outcome.complete = complete;
            outcome.selection = serde_json::to_value(pin)?;
            outcome.provenance = vec![capture];
            if kind == "data-page"
                && (outcome.data["page"]["more"] == true
                    || !outcome.data["page"]["after"].is_null())
            {
                outcome.warnings.push(json!("This page is not the complete table; preserve the snapshot, table and cursor when continuing."));
            }
            if (kind == "data-search" || kind == "data-foreign-key") && truncated {
                outcome.warnings.push(json!("This result is truncated; preserve the snapshot and the stated truncation reason when continuing."));
            }
            Ok(outcome)
        };
        if inv.flag_bool("--cdn") {
            scope.with_session(data, |reader| execute(reader))
        } else {
            scope.with_local_session(data, |reader| execute(reader))
        }
    })();
    finish(scope, result).map_err(|error| capture::query_committed_error(error, &committed, false))
}
fn uint(text: &str) -> Result<u32> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return Err(fault(
            "command.invalid_arguments",
            2,
            "Expected unsigned 32-bit decimal",
        ));
    }
    text.parse()
        .map_err(|_| fault("command.invalid_arguments", 2, "Unsigned value exceeds u32"))
}
fn row_cost(row: &BTreeMap<String, table::Value>, budget: &Budget) -> Result<usize> {
    Ok(capture::json_size(row, budget)? * 4 + row.len() * 128)
}
fn legacy_type(field: &schema::Field) -> String {
    if !field.inline && field.relation {
        return "dbFieldRelation".into();
    }
    if !field.inline && field.identity {
        return "dbFieldNonInlineID".into();
    }
    match field.kind.as_str() {
        "string" | "locstring" => "dbFieldString".into(),
        "float" => "dbFieldFloat".into(),
        "int" => format!(
            "dbField{}Int{}",
            if field.signed { "" } else { "U" },
            field.bits
        ),
        kind => format!("dbField{kind}"),
    }
}
fn float_text(value: f64) -> String {
    let scientific = format!("{value:e}");
    let Some((mantissa, exponent)) = scientific.split_once('e') else {
        return value.to_string();
    };
    let exponent = exponent.parse::<i32>().unwrap_or(0);
    if !(-4..6).contains(&exponent) {
        format!(
            "{mantissa}e{}{abs:02}",
            if exponent < 0 { "-" } else { "+" },
            abs = exponent.unsigned_abs()
        )
    } else {
        value.to_string()
    }
}
pub(crate) fn search_text(value: &table::Value) -> String {
    match value {
        table::Value::Null => String::new(),
        table::Value::String(v) => v.clone(),
        table::Value::Int(v) => v.to_string(),
        table::Value::UInt(v) => v.to_string(),
        table::Value::Float(v) => float_text(*v),
        table::Value::Float32(v) => {
            let s = v.to_string();
            let scientific = format!("{v:e}");
            if let Some((m, e)) = scientific.split_once('e') {
                let exponent = e.parse::<i32>().unwrap_or(0);
                if !(-4..6).contains(&exponent) {
                    return format!(
                        "{m}e{}{abs:02}",
                        if exponent < 0 { "-" } else { "+" },
                        abs = exponent.unsigned_abs()
                    );
                }
            }
            s
        }
        table::Value::Array(v) => format!(
            "[{}]",
            v.iter().map(search_text).collect::<Vec<_>>().join(" ")
        ),
    }
}

pub(crate) struct Prepared {
    pub pin: selection::PinnedSet,
    pub bundle: definitions::Bundle,
    pub file: local_query::File,
    pub layout: table::Layout,
    pub definition: schema::Definition,
    pub availability: table::Availability,
    // Preserve the exact validated request through every secondary table open,
    // including navigation batches that no longer carry an Invocation.
    pub limits: table::TableLimits,
}
pub(crate) fn prepare(inv: &Invocation, name: &str, budget: &Arc<Budget>) -> Result<Prepared> {
    Ok(prepare_many(inv, &[name], budget)?.remove(0))
}
pub(crate) fn prepare_scoped(
    inv: &Invocation,
    name: &str,
    budget: &Arc<Budget>,
    remote: Option<&mut dyn crate::remote::provider::FileScope>,
    resolved: Option<selection::PinnedSet>,
) -> Result<Prepared> {
    Ok(
        prepare_following_scoped(inv, &[name], budget, remote, resolved, &mut |_| {
            Ok(Vec::new())
        })?
        .remove(0),
    )
}

/// Resolve the immutable pin once, bind all definitions from one manifest, and
/// read all requested files in one CASC session. In particular this must not be
/// implemented as a loop over `prepare`: that would replay Encoding and Root.
pub(crate) fn prepare_many(
    inv: &Invocation,
    names: &[&str],
    budget: &Arc<Budget>,
) -> Result<Vec<Prepared>> {
    prepare_following(inv, names, budget, &mut |_| Ok(Vec::new()))
}

/// Discover further tables from a decoded batch without reopening the source.
/// The pin and budget are fixed for the entire request, including definitions.
pub(crate) fn prepare_following(
    inv: &Invocation,
    names: &[&str],
    budget: &Arc<Budget>,
    following: &mut dyn FnMut(&[Prepared]) -> Result<Vec<&'static str>>,
) -> Result<Vec<Prepared>> {
    prepare_following_scoped(inv, names, budget, None, None, following)
}
/// The explicit outer scope owns admission through the caller's scan and capture.
/// Legacy callers retain their previous path; this helper never acquires a lease.
pub(crate) fn prepare_following_scoped(
    inv: &Invocation,
    names: &[&str],
    budget: &Arc<Budget>,
    mut remote: Option<&mut dyn crate::remote::provider::FileScope>,
    resolved: Option<selection::PinnedSet>,
    following: &mut dyn FnMut(&[Prepared]) -> Result<Vec<&'static str>>,
) -> Result<Vec<Prepared>> {
    budget.check()?;
    if let Some(scope) = remote.as_ref() {
        scope.check_consumer(inv, budget)?;
    }
    let pin = match resolved {
        Some(pin) => pin,
        None => match remote.as_ref() {
            Some(scope) => scope.resolve_pin()?,
            None => selection::resolve_pinned(inv)?,
        },
    };
    let data = pin
        .data
        .as_ref()
        .ok_or_else(|| fault("command.failed", 5, "records.data_pin_required"))?;
    if let Some(scope) = remote.as_ref() {
        scope.check_pin(data)?;
    }
    let mut bundles = definitions::prepare_many_controlled(
        &inv.home,
        &data.definition_commit,
        names,
        inv.flag_bool("--offline"),
        budget.deadline,
        Some(budget),
        Some(&inv.control),
    )?;
    let ids: Vec<_> = bundles
        .iter()
        .map(|(bundle, _)| bundle.identity.db2_file_data_id)
        .collect();
    let mut prepared = Vec::new();
    let mut accept = |files: Vec<local_query::File>| {
        if files.len() != bundles.len() {
            return Err(fault(
                "records.internal",
                8,
                "Table file batch cardinality differs",
            ));
        }
        let batch_start = prepared.len();
        for ((bundle, raw), file) in std::mem::take(&mut bundles).into_iter().zip(files) {
            budget.check()?;
            // The physical provider has already validated this flag. Resolve it
            // here, not before source/definition preparation, to preserve errors.
            let limits = local_query::table_limits(inv.usize_flag(
                "--max-bytes",
                128 << 20,
                1,
                512 << 20,
            )?);
            let (layout, availability) = table::inspect_available_budgeted(
                &file.bytes,
                file.bytes.missing(),
                limits,
                budget.clone(),
            )?;
            if layout.table_hash != bundle.identity.hash {
                return Err(fault(
                    "schema.invalid_definition",
                    4,
                    "schema.invalid_definition",
                ));
            }
            let definition = schema::Document::parse_with_check(&raw, || {
                inv.control.check()?;
                budget.check()
            })?
            .select(&data.full_build, &format!("{:08X}", layout.layout_hash))?;
            prepared.push(Prepared {
                pin: pin.clone(),
                bundle,
                file,
                layout,
                definition,
                availability,
                limits,
            });
        }
        let names = following(&prepared[batch_start..])?;
        if names.is_empty() {
            return Ok(Vec::new());
        }
        budget.check()?;
        bundles = definitions::prepare_many_controlled(
            &inv.home,
            &data.definition_commit,
            &names,
            inv.flag_bool("--offline"),
            budget.deadline,
            Some(budget),
            Some(&inv.control),
        )?;
        if bundles
            .iter()
            .any(|(bundle, _)| bundle.manifest != prepared[0].bundle.manifest)
        {
            return Err(fault(
                "records.definition_identity",
                4,
                "Definition manifest changed between table batches",
            ));
        }
        Ok(bundles
            .iter()
            .map(|(bundle, _)| bundle.identity.db2_file_data_id)
            .collect())
    };
    match remote.as_mut() {
        Some(scope) => scope.read_table_batches(data, &ids, &mut accept)?,
        None => local_query::read_table_batches_budgeted(inv, data, &ids, budget, &mut accept)?,
    }
    Ok(prepared)
}
