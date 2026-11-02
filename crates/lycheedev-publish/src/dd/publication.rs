//! One durable DD publication lifecycle for the three actual resource editors.
//! Editors own forms, dependencies and readback; this boundary owns submission.
use super::{
    Reference, Resource,
    forms::{self, Api, WriteAction, data},
};
use crate::{mutation::Mutation, provider::Provider};
use lycheedev_core::{Result, ToolkitError, control::OperationControl, storage};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    num::NonZeroU64,
    path::Path,
    time::{Duration, Instant},
};
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Context {
    pub resource: Resource,
    pub action: WriteAction,
    pub game_type: NonZeroU64,
    pub reference: Option<String>,
    pub before_references: Vec<String>,
    pub fields: BTreeMap<String, String>,
    pub version_fields: BTreeMap<String, String>,
    pub verified: bool,
}
pub(super) struct Prepared {
    pub body: Value,
    pub context: Context,
}
pub(super) trait Editor {
    fn resource(&self) -> Resource;
    fn action(&self) -> WriteAction;
    fn original_sn(&self) -> Option<&str>;
    fn identity(&self, author: &str) -> Value;
    fn fields(&self) -> &'static [&'static str];
    fn prepare(&self, api: &mut impl Api, control: &OperationControl) -> Result<Prepared>;
    fn verify(
        &self,
        api: &mut impl Api,
        context: &Context,
        mutation: &Mutation,
        control: &OperationControl,
    ) -> Result<Value>;
    fn verification(&self) -> &'static str;
}
pub(super) fn execute(
    editor: &impl Editor,
    home: &Path,
    author: &str,
    api: &mut impl Api,
    recover_sn: Option<&str>,
    control: &OperationControl,
) -> Result<Value> {
    control.check()?;
    if author.trim().is_empty() || author.len() > 1024 || author.chars().any(char::is_control) {
        return Err(data("invalid retained DD author identity"));
    }
    let resource = editor.resource();
    let action = editor.action();
    if let Some(sn) = recover_sn {
        Reference::parse(sn)?;
        if action != WriteAction::Create {
            return Err(ToolkitError::arguments(
                "--recover-sn applies only to an original pending creation",
            ));
        }
    }
    let identity = editor.identity(author);
    let (record_resource, target) = if action == WriteAction::Create {
        let hash = storage::sha256(&storage::identity_json(&identity)?);
        (
            format!("{}-create", resource.name()),
            u64::from_str_radix(&hash[..16], 16).unwrap().max(1),
        )
    } else {
        (
            resource.name().into(),
            forms::target(
                author,
                resource,
                editor
                    .original_sn()
                    .ok_or_else(|| data("DD write omitted its original SN"))?,
            )?,
        )
    };
    let mut mutation = Mutation::open(
        home,
        Provider::Dd,
        &record_resource,
        target,
        action.name(),
        &identity,
        control,
    )?;
    let original = mutation
        .evidence()
        .map(|v| {
            serde_json::from_value::<Context>(v.clone())
                .map_err(|_| ToolkitError::invalid("invalid original DD publication context"))
        })
        .transpose()?;
    if let Some(mut context) = original {
        validate_context(editor, &context)?;
        if recover_sn.is_some_and(|sn| context.reference.as_ref().is_some_and(|old| old != sn)) {
            return Err(crate::input::invalid(
                "$.sn",
                "recovery SN differs from the original DD creation",
            ));
        }
        if mutation.pending() || context.verified {
            if context.reference.is_none() {
                let sn=recover_sn.ok_or_else(||mutation.pending_error("creation outcome is pending; read the author list and recover the original input with explicit --recover-sn"))?;
                if context.before_references.iter().any(|old| old == sn) {
                    return Err(
                        mutation.pending_error("recovery SN predates the original DD creation")
                    );
                }
                context.reference = Some(sn.into());
                mutation.remember(serde_json::to_value(&context)?)?;
            }
            match editor.verify(api, &context, &mutation, control) {
                Ok(value) => {
                    if mutation.pending() {
                        context.verified = true;
                        mutation
                            .verified_with(serde_json::to_value(&context)?)
                            .map_err(|_| {
                                mutation.pending_error(
                                    "DD write is verified; finish saving its original receipt",
                                )
                            })?;
                    }
                    return Ok(report(editor, &context, &mutation, value, true));
                }
                Err(error) if mutation.pending() || action == WriteAction::Create => {
                    return Err(error);
                }
                Err(error)
                    if matches!(
                        error.code.as_str(),
                        "command.cancelled" | "command.deadline"
                    ) =>
                {
                    return Err(error);
                }
                Err(_) => {} // A completed mutable edit may be explicitly applied to fresh state again.
            }
        }
    } else if mutation.pending() {
        return Err(
            mutation.pending_error("the original DD publication has no complete recovery context")
        );
    }
    if recover_sn.is_some() {
        return Err(ToolkitError::arguments(
            "--recover-sn requires the original pending creation; omit it for a new create",
        ));
    }
    let Prepared { body, mut context } = editor.prepare(api, control)?;
    validate_context(editor, &context)?;
    mutation.prepare_with_evidence(serde_json::to_value(&context)?, control)?;
    let family = match resource {
        Resource::Plugin => "addon",
        Resource::Config => "share",
        Resource::Wa => "wa",
    };
    let endpoint = format!(
        "/{family}/{}",
        if action == WriteAction::Create {
            "create"
        } else {
            "modify"
        }
    );
    let response = api
        .submit(&endpoint, &body)
        .map_err(|e| mutation.failure(e))?;
    if action == WriteAction::Create {
        let value=response.get("result").ok_or_else(||mutation.pending_error("DD create was accepted without its result; recover the original input with explicit --recover-sn"))?;
        let sn = if value.is_object() {
            forms::first(
                value,
                if resource == Resource::Config {
                    &["share_sn", "sn"]
                } else {
                    &["sn"]
                },
            )
            .and_then(forms::scalar)
        } else {
            forms::scalar(value)
        };
        if let Some(sn) = sn {
            if Reference::parse(&sn).is_err() || context.before_references.contains(&sn) {
                return Err(mutation.pending_error("DD create returned an invalid or preexisting SN; recover the original request explicitly"));
            }
            context.reference = Some(sn);
            mutation
                .remember(serde_json::to_value(&context)?)
                .map_err(|_| {
                    mutation
                        .pending_error("save the original DD creation reference before continuing")
                })?;
        }
    }
    mutation.acknowledged().map_err(|_| {
        mutation.pending_error("DD write was accepted; finish saving the acknowledgement")
    })?;
    if context.reference.is_none() {
        return Err(mutation.pending_error("DD create was accepted without an SN; inspect the author list and use explicit --recover-sn with the original input"));
    }
    let value = editor.verify(api, &context, &mutation, control)?;
    context.verified = true;
    mutation
        .verified_with(serde_json::to_value(&context)?)
        .map_err(|_| mutation.pending_error("DD write is verified; finish saving its receipt"))?;
    Ok(report(editor, &context, &mutation, value, false))
}
fn validate_context(editor: &impl Editor, context: &Context) -> Result<()> {
    if context.resource != editor.resource()
        || context.action != editor.action()
        || context.fields.is_empty()
        || context
            .fields
            .iter()
            .chain(context.version_fields.iter())
            .any(|(k, v)| {
                !editor.fields().contains(&k.as_str())
                    || v.len() != 64
                    || !v.bytes().all(|b| b.is_ascii_hexdigit())
            })
    {
        return Err(ToolkitError::invalid(
            "DD publication context differs from its original operation",
        ));
    }
    if let Some(sn) = &context.reference {
        Reference::parse(sn)
            .map_err(|_| ToolkitError::invalid("invalid original DD publication recovery SN"))?;
    }
    if editor.action() != WriteAction::Create
        && context.reference.as_deref() != editor.original_sn()
    {
        return Err(ToolkitError::invalid(
            "DD publication target differs from its original SN",
        ));
    }
    Ok(())
}
fn report(
    editor: &impl Editor,
    context: &Context,
    mutation: &Mutation,
    readback: Value,
    recovered: bool,
) -> Value {
    forms::public(
        json!({"platform":"dd","resource":context.resource,"action":context.action,"reference":context.reference,"game_type":context.game_type.get(),
        "operation_id":mutation.id(),"verification":editor.verification(),"readback":readback,"recovered":recovered,"reused":recovered,
        "submitted_this_call":!recovered,"replay_performed":false,"review_status":"not_checked","publication_visibility":"not_checked"}),
    )
}
pub(super) fn selected<'a>(rows: &'a [Value], sn: &str) -> Result<Option<&'a Value>> {
    let mut result = None;
    for row in rows {
        if forms::reference(row)? == sn {
            if result.replace(row).is_some() {
                return Err(data("DD complete author list repeated the selected SN"));
            }
        }
    }
    Ok(result)
}
pub(super) fn hashes(form: &Value, fields: &[&str]) -> Result<BTreeMap<String, String>> {
    fields
        .iter()
        .filter_map(|&k| form.get(k).map(|v| Ok((k.into(), forms::digest(v)?))))
        .collect()
}
pub(super) fn mismatches(
    expected: &BTreeMap<String, String>,
    actual: &Value,
) -> Result<Vec<String>> {
    let mut fields = Vec::new();
    for (key, hash) in expected {
        if actual.get(key).map(forms::digest).transpose()?.as_deref() != Some(hash.as_str()) {
            fields.push(key.clone());
        }
    }
    Ok(fields)
}
pub(super) fn pause(control: &OperationControl, mutation: &Mutation) -> Result<()> {
    let until = Instant::now() + Duration::from_secs(1);
    while Instant::now() < until {
        control
            .check()
            .map_err(|e| mutation.readback_error("DD field verification is pending", e))?;
        std::thread::sleep(Duration::from_millis(25));
    }
    Ok(())
}
pub(super) fn verify_fields(
    api: &mut impl Api,
    context: &Context,
    mutation: &Mutation,
    control: &OperationControl,
    project: fn(&Value) -> Result<Value>,
) -> Result<Value> {
    let sn = context
        .reference
        .as_deref()
        .ok_or_else(|| mutation.pending_error("select the original creation SN before readback"))?;
    let mut fields = Vec::new();
    for attempt in 0..6 {
        control
            .check()
            .map_err(|e| mutation.readback_error("DD field verification is pending", e))?;
        if attempt > 0 {
            pause(control, mutation)?;
        }
        let raw = super::read::detail(api, context.resource, &Reference::parse(sn)?)
            .map_err(|e| mutation.readback_error("DD resource readback is pending", e))?;
        if raw["is_owner"] == false {
            return Err(mutation.readback_error(
                "DD resource readback ownership differs",
                data("selected resource is not owned by the retained author"),
            ));
        }
        let rows = super::read::all_author(api, context.resource, context.game_type, control)
            .map_err(|e| mutation.readback_error("DD author-list readback is incomplete", e))?;
        let author = selected(&rows, sn)
            .map_err(|e| mutation.readback_error("DD readback target is ambiguous", e))?;
        if raw["is_owner"] != true && author.is_none() {
            return Err(mutation.readback_error(
                "DD readback ownership is unverified",
                data("selected SN is absent from the complete author list"),
            ));
        }
        let actual = project(&raw)
            .map_err(|e| mutation.readback_error("DD editor readback projection is invalid", e))?;
        fields = mismatches(&context.fields, &actual)?;
        if fields.is_empty() {
            return Ok(
                json!({"source":"detail","fields":context.fields.keys().collect::<Vec<_>>(),"complete_author_list":true,"detail":super::read::public_resource(context.resource,raw).map_err(|e|mutation.readback_error("DD fields matched; public metadata projection is pending",e))?}),
            );
        }
    }
    let mut cause = data("DD readback differs from submitted fields");
    cause.details = json!({"fields":fields});
    Err(mutation.readback_error(
        "DD write is accepted or uncertain; recover only by original readback",
        cause,
    ))
}
