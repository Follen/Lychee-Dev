//! CLI admission for Data. Session memory and selected-file limits are
//! separate typed values before any worker or data owner is created.
use crate::Invocation;
use lycheedev_core::{Outcome, Result, ToolkitError};
use lycheedev_data::request::{
    CachePrepareRequest, ContentVariant, DataContext, SessionConfig, SourceOptions,
};
use std::{path::PathBuf, time::Duration};

pub(crate) fn context(inv: &Invocation, source: SourceOptions) -> DataContext {
    DataContext {
        home: inv.home.clone(),
        cwd: inv.cwd.clone(),
        control: inv.control.clone(),
        source,
    }
}

pub(crate) fn resolve_path(inv: &Invocation, path: &str) -> PathBuf {
    let path = PathBuf::from(path);
    if path.is_absolute() { path } else { inv.cwd.join(path) }
}

fn selected_source(inv: &Invocation) -> Result<SourceOptions> {
    let content_variant = match inv.flag("--content-variant").unwrap_or("") {
        "" => ContentVariant::Unspecified,
        "standard" => ContentVariant::Standard,
        "low-violence" => ContentVariant::LowViolence,
        _ => return Err(ToolkitError::arguments("Invalid content variant")),
    };
    Ok(SourceOptions {
        snapshot: inv.flag("--snapshot").map(str::to_owned),
        project: inv.flag("--project").map(str::to_owned),
        cdn: inv.flag_bool("--cdn"),
        installation: inv
            .flag("--installation")
            .filter(|path| !path.is_empty())
            .map(|path| resolve_path(inv, path)),
        offline: inv.flag_bool("--offline"),
        key_file: inv.flag("--key-file").map(|path| resolve_path(inv, path).to_string_lossy().into_owned()),
        content_variant,
        ..SourceOptions::default()
    })
}

pub(crate) fn source_options(inv: &Invocation) -> Result<SourceOptions> {
    let mut source = selected_source(inv)?;
    source.max_file_bytes = inv.usize_flag("--max-bytes", 128 << 20, 1, 512 << 20)?;
    Ok(source)
}

pub(crate) fn admit_session(inv: &Invocation) -> Result<(DataContext, SessionConfig)> {
    if inv.flag("--format") != Some("jsonl") {
        return Err(ToolkitError::arguments("data agent serve requires JSONL"));
    }
    let config = SessionConfig {
        max_handles: inv.usize_flag("--max-objects", 32, 1, 1024)?,
        retained_bytes: inv.usize_flag("--max-bytes", 512 << 20, 1 << 20, 2usize << 30)?,
        result_ttl_seconds: inv.usize_flag("--result-ttl-seconds", 300, 1, 3600)? as u64,
    };
    Ok((context(inv, selected_source(inv)?), config))
}

pub(crate) fn prepare_request(inv: &Invocation) -> Result<CachePrepareRequest> {
    let table = inv.argument()?;
    if !lycheedev_data::definitions::valid_name(table) {
        return Err(ToolkitError::arguments(
            "data cache prepare requires one valid table name",
        ));
    }
    Ok(CachePrepareRequest {
        table: table.into(),
        timeout: Duration::from_secs(inv.usize_flag("--timeout-seconds", 300, 1, 3600)? as u64),
    })
}

pub(crate) fn prune(inv: &Invocation) -> Result<Outcome> {
    if inv.flag_bool("--uncommitted") {
        return Err(ToolkitError::arguments(
            "--data-derived does not include --uncommitted staging",
        ));
    }
    let options = lycheedev_core::workspace::data_disk::DerivedRetirementOptions {
        target_bytes: inv.usize_flag("--target-bytes", 0, 0, usize::MAX)? as u64,
        max_objects: inv.usize_flag("--max-objects", 0, 0, 100000)?,
        dry_run: inv.flag_bool("--dry-run"),
    };
    lycheedev_data::prune_derived_cache(&context(inv, SourceOptions::default()), options)
}

pub(crate) fn dispatch(inv: &Invocation) -> Result<Outcome> {
    match inv.command.as_str() {
        "data agent schema" => {
            inv.control.check()?;
            Ok(Outcome::new(lycheedev_data::agent_session::schema()))
        }
        "data agent serve" => Err(ToolkitError::new(
            "data.session.supervisor_required",
            "Data Agent serve requires the CLI-owned supervised process entry",
            3,
        )),
        "data cache prepare" => lycheedev_data::prepare_cache(
            &context(inv, source_options(inv)?),
            &prepare_request(inv)?,
        ),
        "data hotfix" => crate::hotfix_cli::dispatch(inv),
        "asset inspect" | "asset search" | "asset export" | "asset demux" => {
            let (ctx, request) = crate::asset_cli::request(inv)?.expect("selected asset command");
            lycheedev_data::assets::dispatch(&ctx, &request)
        }
        _ => Err(ToolkitError::arguments("Unknown Data command")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn invocation(words: &[&str]) -> Invocation {
        crate::parse(&words.iter().map(|word| (*word).into()).collect::<Vec<_>>()).unwrap()
    }

    #[test]
    fn session_requires_explicit_jsonl_through_public_parser_and_admission() {
        let explicit = invocation(&["data", "agent", "serve", "--format", "jsonl"]);
        let (context, config) = admit_session(&explicit).unwrap();
        assert_eq!(config.max_handles, 32);
        assert!(context.control.same_scope(&explicit.control));

        for words in [
            vec!["data", "agent", "serve"],
            vec!["data", "agent", "serve", "--format", "json"],
        ] {
            let error = crate::parse(&words.iter().map(|word| (*word).into()).collect::<Vec<_>>())
                .and_then(|inv| admit_session(&inv))
                .err()
                .expect("serve must require the advertised explicit JSONL transport");
            assert_eq!((error.code.as_str(), error.exit_code), ("command.invalid_arguments", 2));
        }
    }

    #[test]
    fn session_memory_does_not_become_the_physical_file_limit() {
        let inv = invocation(&[
            "data",
            "agent",
            "serve",
            "--format",
            "jsonl",
            "--max-bytes",
            "1073741824",
            "--max-objects",
            "17",
            "--result-ttl-seconds",
            "41",
        ]);
        let (ctx, config) = admit_session(&inv).unwrap();
        assert_eq!(config.retained_bytes, 1 << 30);
        assert_eq!(config.max_handles, 17);
        assert_eq!(config.result_ttl_seconds, 41);
        assert_eq!(ctx.source.max_file_bytes, 128 << 20);
        let prepare = invocation(&["data", "cache", "prepare", "Map", "--max-bytes", "1048576"]);
        assert_eq!(source_options(&prepare).unwrap().max_file_bytes, 1 << 20);
        assert_eq!(prepare_request(&prepare).unwrap().table, "Map");
    }

    #[test]
    fn source_admission_preserves_target_and_control_without_opening_data() {
        let mut inv = invocation(&[
            "data",
            "agent",
            "serve",
            "--format",
            "jsonl",
            "--snapshot",
            "PIN-explicit",
            "--project",
            "project",
            "--installation",
            "client",
            "--offline",
            "--key-file",
            "keys.json",
        ]);
        inv.cwd = std::env::temp_dir().join("lycheedev-data-cli-admission");
        let (ctx, _) = admit_session(&inv).unwrap();
        assert_eq!(ctx.source.snapshot.as_deref(), Some("PIN-explicit"));
        assert_eq!(ctx.source.project.as_deref(), Some("project"));
        assert_eq!(ctx.source.installation, Some(inv.cwd.join("client")));
        assert!(ctx.source.offline);
        assert_eq!(ctx.source.key_file, Some(inv.cwd.join("keys.json").to_string_lossy().into_owned()));
        assert!(ctx.control.same_scope(&inv.control));
        assert!(!inv.cwd.exists());
    }

    #[test]
    fn derived_retirement_rejects_staging_before_opening_the_workspace() {
        let inv = invocation(&["cache", "prune", "--data-derived", "--uncommitted"]);
        let error = prune(&inv).unwrap_err();
        assert_eq!(error.code, "command.invalid_arguments");
        assert_eq!(
            error.message,
            "--data-derived does not include --uncommitted staging"
        );
    }
}
