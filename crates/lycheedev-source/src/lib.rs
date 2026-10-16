#![forbid(unsafe_code)]
//! Immutable source research; never evaluates indexed repository programs.
pub mod batch;
pub mod catalog;
pub mod closure;
pub mod context;
pub mod diff;
pub mod environment;
#[cfg(feature = "emmy-prototype")]
mod engine_config;
pub mod facts;
pub mod flow;
pub mod git;
pub mod index;
mod lsp_frame;
pub mod lua_tree;
pub mod luac;
pub mod luals;
pub mod matrix;
#[cfg(feature = "emmy-prototype")]
pub mod native;
#[cfg(feature = "emmy-prototype")]
pub mod native_research;
#[cfg(feature = "emmy-prototype")]
pub mod native_semantic;
pub mod native_session;
mod persistence;
mod process;
pub mod prune;
mod query_store;
pub mod refs;
pub mod relations;
mod research_material;
pub mod semantic;
pub mod session;
pub mod validate;
pub mod xml_inline;
#[cfg(feature = "emmy-prototype")]
pub mod xml_load;
pub mod xml_program;
use lycheedev_core::{Invocation, Outcome, Result, ToolkitError};
#[derive(Default)]
pub struct SourceTools<'a> {
    pub luac: Option<&'a luac::Runtime>,
    pub luals: Option<&'a luals::Runtime>,
}

pub fn commands() -> &'static [&'static str] {
    &[
        "source list",
        "source sync",
        "source inspect",
        "source index",
        "source query",
        "source batch",
        "source refs",
        "source context",
        "source session status",
        "source session close",
        "source diff",
        "source prune",
        "source validate",
    ]
}

// Go 3.2.0 parseOptions defaults source query to 50, with an explicit 1..=200 bound.
fn query_limit(inv: &Invocation) -> Result<usize> {
    inv.usize_flag("--limit", 50, 1, 200)
}

pub fn dispatch(inv: &Invocation) -> Result<Outcome> {
    dispatch_with_tools(inv, SourceTools::default())
}
pub fn dispatch_with_tools(inv: &Invocation, tools: SourceTools<'_>) -> Result<Outcome> {
    dispatch_with_services(inv, tools, None)
}
pub fn dispatch_with_services(
    inv: &Invocation,
    tools: SourceTools<'_>,
    broker: Option<&session::BrokerIdentity>,
) -> Result<Outcome> {
    inv.control.check()?;
    match inv.command.as_str() {
        "source session status" | "source session close" => {
            return native_session::dispatch(inv);
        }
        "source batch" => return batch::dispatch(inv, tools),
        "source refs" => return refs::dispatch_with_services(inv, tools, broker),
        "source context" => return context::dispatch_with_services(inv, tools, broker),
        "source diff" => return diff::dispatch(inv),
        "source prune" => {
            if let Some(identity) = broker {
                let mut retiring = inv.clone();
                retiring.command = "source session close".into();
                session::dispatch(&retiring, identity)?;
            }
            return prune::dispatch(inv);
        }
        "source validate" => {
            if inv.flag("--matrix").is_some() {
                return matrix::dispatch(inv, tools);
            }
            let pin = lycheedev_core::selection::resolve_pinned(inv)?;
            let source = pin.source.as_ref().ok_or_else(|| {
                ToolkitError::new(
                    "selection.source_missing",
                    "Pinned selection has no source",
                    3,
                )
            })?;
            let path = std::path::Path::new(inv.required("--path")?);
            let root = if path.is_absolute() {
                path.to_path_buf()
            } else {
                inv.cwd.join(path)
            };
            let environment = if source.repository != "wow-ui-source" && inv.flag_bool("--semantic")
            {
                let id = inv.required("--environment")?;
                let pin = lycheedev_core::selection::load_pinned(&inv.home, id)?;
                Some(pin.spec.source.ok_or_else(|| {
                    ToolkitError::new(
                        "codebase.client_source_pin_required",
                        "Environment pin has no source",
                        3,
                    )
                })?)
            } else {
                None
            };
            let data = validate::addon_with_environment(
                &inv.home,
                source,
                &pin.id,
                &root,
                inv.required("--toc")?,
                &tools,
                inv.flag_bool("--semantic"),
                &inv.control,
                environment.as_ref(),
            )?;
            let mut out = Outcome::new(data);
            out.complete = out.data["complete"] == true;
            let capture = lycheedev_core::evidence::commit_capture_bounded(
                &inv.home,
                lycheedev_core::evidence::CaptureInput {
                    bytes: &serde_json::to_vec(&out.data)?,
                    maximum_bytes: lycheedev_core::evidence::DEFAULT_CAPTURE_BYTES,
                    media_type: "application/json",
                    provenance: serde_json::json!({"kind":"addon-static-check","locator":format!("{}/{}",inv.required("--path")?,inv.required("--toc")?),"snapshot":pin.id,"sourceCommit":source.exact_commit}),
                    complete: out.complete,
                    truncated: false,
                },
                &inv.control,
            )?;
            out.provenance.push(capture);
            out.selection = serde_json::to_value(&pin)?;
            if out.data["staticValid"] == false {
                return Err(ToolkitError::new("codebase.addon_static_check_failed","Addon validation found invalid source",4).with_details(serde_json::json!({"result":out.data,"captures":out.provenance,"context":{"selection":out.selection,"coverage":out.coverage,"snapshot":pin.id}})));
            }
            if !out.complete {
                out.warnings.push(serde_json::json!("Static coverage is incomplete; inspect unresolved references and notChecked."));
            }
            return Ok(out);
        }
        "source list" => {
            if inv.flag("--snapshot").is_none() {
                return Ok(Outcome::new(serde_json::to_value(catalog::repositories())?));
            }
            let pin = lycheedev_core::selection::resolve_pinned(inv)?;
            let source = pin.source.as_ref().ok_or_else(|| {
                ToolkitError::new(
                    "selection.source_missing",
                    "Pinned selection has no source",
                    3,
                )
            })?;
            let mut out = Outcome::new(query_store::status_with_control(
                &inv.home,
                source,
                &inv.control,
            )?);
            out.coverage = out.data["coverage"].clone();
            out.selection = serde_json::to_value(&pin)?;
            return Ok(out);
        }
        "source sync" => {
            let source = git::resolve_source_with_control(
                &inv.home,
                inv.required("--source")?,
                inv.required("--product")?,
                inv.flag("--ref").unwrap_or(""),
                &inv.control,
            )?;
            let pin = lycheedev_core::selection::pin_selection(
                &inv.home,
                lycheedev_core::selection::SelectionSpec {
                    source: Some(source),
                    ..Default::default()
                },
            )?;
            let mut result = Outcome::new(serde_json::to_value(&pin)?);
            result.selection = serde_json::to_value(pin)?;
            return Ok(result);
        }
        "source inspect" | "source index" | "source query" => {
            use lycheedev_core::{evidence, selection};
            use serde_json::json;
            let pin = selection::resolve_pinned(inv)?;
            let source = pin.source.as_ref().ok_or_else(|| {
                ToolkitError::new(
                    "selection.source_missing",
                    "Pinned selection has no source",
                    3,
                )
            })?;
            let mut out;
            if inv.command == "source inspect" && inv.flag("--path").is_some() {
                if inv.flag("--symbol").is_some() || inv.flag("--target-path").is_some() {
                    return Err(ToolkitError::arguments(
                        "source inspect requires exactly one selector",
                    ));
                }
                let path = inv.required("--path")?;
                let first = inv.usize_flag("--line", 1, 1, usize::MAX)?;
                let count = inv.usize_flag("--count", 80, 1, 2000)?;
                let (raw, _) = git::blob_with_control(
                    &inv.home,
                    source,
                    path,
                    git::MAX_SOURCE_BYTES,
                    &inv.control,
                )?;
                let text = std::str::from_utf8(&raw).map_err(|_| {
                    ToolkitError::new("codebase.source_integrity", "Source is not UTF-8", 4)
                })?;
                if raw.contains(&0) {
                    return Err(ToolkitError::new(
                        "codebase.source_integrity",
                        "Source contains NUL",
                        4,
                    ));
                }
                let mut starts = if raw.is_empty() { Vec::new() } else { vec![0] };
                for (i, b) in raw.iter().enumerate() {
                    if *b == b'\n' && i + 1 < raw.len() {
                        starts.push(i + 1);
                    }
                }
                if first > starts.len() {
                    return Err(ToolkitError::new(
                        "codebase.line_out_of_range",
                        "Requested line exceeds source",
                        2,
                    ));
                }
                let last = starts.len().min(first.saturating_add(count - 1));
                let end = starts.get(last).copied().unwrap_or(raw.len());
                let blob = evidence::publish_blob(&inv.home, &raw, git::MAX_SOURCE_BYTES)?;
                let data = json!({"repository":source.repository,"commit":source.exact_commit,"path":path,"firstLine":first,"lastLine":last,"totalLines":starts.len(),"text":&text[starts[first-1]..end],"blob":blob});
                out = Outcome::new(data);
            } else {
                let index = if inv.command == "source query" {
                    query_store::load_with_control(
                        &inv.home,
                        source,
                        inv.argument()?,
                        inv.flag("--mode").unwrap_or(""),
                        inv.flag("--topic").unwrap_or(""),
                        &inv.control,
                    )?
                } else if let Some(symbol) = inv.flag("--symbol") {
                    query_store::load_with_control(
                        &inv.home,
                        source,
                        symbol,
                        "precise",
                        "",
                        &inv.control,
                    )?
                } else {
                    index::build_with_control(&inv.home, source, &inv.control)?
                };
                let coverage = index.coverage();
                let data = match inv.command.as_str() {
                    "source index" => index.summary.clone(),
                    "source query" => index.search_with_control(
                        source,
                        &pin.id,
                        inv.argument()?,
                        inv.flag("--mode").unwrap_or(""),
                        inv.flag("--topic").unwrap_or(""),
                        query_limit(inv)?,
                        inv.flag("--cursor").unwrap_or(""),
                        &inv.control,
                    )?,
                    _ => {
                        if let Some(symbol) = inv.flag("--symbol") {
                            if inv.flag("--target-path").is_some() {
                                return Err(ToolkitError::arguments(
                                    "source inspect selector conflict",
                                ));
                            }
                            index.search_with_control(
                                source,
                                &pin.id,
                                symbol,
                                "precise",
                                "",
                                25,
                                "",
                                &inv.control,
                            )?
                        } else if let Some(path) = inv.flag("--target-path") {
                            index.search_with_control(
                                source,
                                &pin.id,
                                path,
                                "precise",
                                "asset",
                                25,
                                "",
                                &inv.control,
                            )?
                        } else {
                            return Err(ToolkitError::arguments(
                                "source inspect requires a selector",
                            ));
                        }
                    }
                };
                out = Outcome::new(data);
                out.coverage = if out.data.get("coverage").is_some() {
                    out.data["coverage"].clone()
                } else {
                    coverage
                };
                out.complete = out.data["complete"] == true;
            }
            let kind = if inv.command == "source inspect" {
                "source-excerpt"
            } else {
                "source-query"
            };
            let capture = evidence::commit_capture_bounded(
                &inv.home,
                lycheedev_core::evidence::CaptureInput {
                    bytes: &serde_json::to_vec(&out.data)?,
                    maximum_bytes: lycheedev_core::evidence::DEFAULT_CAPTURE_BYTES,
                    media_type: "application/json",
                    provenance: json!({"kind":kind,"locator":inv.flag("--path").or(inv.positional.as_deref()).unwrap_or(&source.repository),"snapshot":pin.id,"sourceCommit":source.exact_commit}),
                    complete: out.complete,
                    truncated: out.data["truncated"] == true,
                },
                &inv.control,
            )?;
            out.provenance.push(capture);
            out.selection = serde_json::to_value(pin)?;
            return Ok(out);
        }
        _ => {}
    }
    Err(ToolkitError {
        code: "command.unsupported".into(),
        message: format!(
            "Rust source command has not completed parity: {}",
            inv.command
        ),
        exit_code: 3,
        details: serde_json::Value::Null,
    })
}

#[cfg(test)]
mod public_query_defaults {
    use super::*;
    #[test]
    fn query_default_and_explicit_limits_match_fixed_go() {
        let mut inv = Invocation {
            command: "source query".into(),
            positional: Some("Frame".into()),
            flags: Default::default(),
            home: Default::default(),
            cwd: Default::default(),
            control: Default::default(),
        };
        assert_eq!(query_limit(&inv).unwrap(), 50);
        for (value, expected) in [("1", 1), ("10", 10), ("50", 50), ("200", 200)] {
            inv.flags.insert("--limit".into(), value.into());
            assert_eq!(query_limit(&inv).unwrap(), expected);
        }
        for value in ["0", "201", "-1", "", "not-an-integer"] {
            inv.flags.insert("--limit".into(), value.into());
            assert_eq!(query_limit(&inv).unwrap_err().exit_code, 2);
        }
    }
}

#[cfg(test)]
mod public_command_registry {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn implemented_source_commands_cover_fixed_go_inventory() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tests/rust-parity/go-3.2.0.manifest.json"
        ))
        .unwrap();
        assert_eq!(
            oracle["oracle"]["commit"],
            "3057a9666ab078270b63c7f06de748069b0d1f4b"
        );
        let expected: BTreeSet<_> = oracle["commands"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|entry| entry["path"].as_str())
            .filter(|path| path.starts_with("source "))
            .collect();
        let actual: BTreeSet<_> = commands().iter().copied().collect();
        assert_eq!(
            actual.len(),
            commands().len(),
            "duplicate Source registration"
        );
        assert_eq!(actual, expected);
    }
}
