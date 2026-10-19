#![forbid(unsafe_code)]

mod contract;
mod formal_live;
mod target_remote;
#[cfg(test)]
mod hotfix_remote_tests;
use lycheedev_core::{Invocation, Outcome, Result, ToolkitError, VERSION};
use serde_json::{Value, json};
use std::{collections::BTreeMap, io::Write, path::PathBuf};

fn home(flags: &BTreeMap<String, String>) -> PathBuf {
    flags
        .get("--home")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("LYCHEEDEV_HOME").map(PathBuf::from))
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("USERPROFILE").unwrap_or_default()).join(".lycheedev")
        })
}
// Go v3.2.0 scans the complete argv for output encoding before any other
// admission check. The last occurrence wins even when the flag is repeated.
fn requested_format(args: &[String]) -> &str {
    let mut format = "text";
    for (index, arg) in args.iter().enumerate() {
        if arg == "--format" {
            if let Some(value) = args.get(index + 1) {
                format = value;
            }
        } else if let Some(value) = arg.strip_prefix("--format=") {
            format = value;
        }
    }
    format
}
fn valid_format(format: &str) -> bool {
    ["text", "json", "jsonl"].contains(&format)
}
fn unsupported_route_flag(command: &str, flag: &str) -> ToolkitError {
    let route = if command.is_empty() { "the command" } else { command };
    ToolkitError::arguments(format!(
        "{flag} is not supported by {route}; use {route} --help"
    ))
}
fn parse(args: &[String]) -> Result<Invocation> {
    if !valid_format(requested_format(args)) {
        return Err(ToolkitError::arguments(
            "format must be text, json, or jsonl",
        ));
    }
    let all = contract::all();
    // Collect words before checking values, as the fixed Go parser does. This
    // preserves unknown-token priority and lets route admission precede values.
    let mut words = vec![];
    let mut index = 0;
    while index < args.len() {
        let token = &args[index];
        if token == "-h" || token == "--help" {
            index += 1;
            continue;
        }
        if token.starts_with('-') {
            let (key, inline) = token
                .split_once('=')
                .map(|(key, _)| (key, true))
                .unwrap_or((token.as_str(), false));
            let spec = all
                .iter()
                .flat_map(|d| d.flags.iter())
                .find(|f| f.name() == key)
                .ok_or_else(|| ToolkitError::arguments(format!("unknown flag {key}")))?;
            if !spec.takes_value() && inline {
                return Err(ToolkitError::arguments(format!("unknown flag {key}")));
            }
            if spec.takes_value() && !inline && index + 1 < args.len() {
                index += 1;
            }
        } else {
            words.push(token.clone());
        }
        index += 1;
    }
    let definition = all
        .iter()
        .filter(|d| {
            words.starts_with(
                &d.path
                    .split_whitespace()
                    .map(str::to_owned)
                    .collect::<Vec<_>>(),
            )
        })
        .max_by_key(|d| d.path.split_whitespace().count());
    if definition.is_none() && !words.is_empty() {
        return Err(ToolkitError::arguments(format!(
            "unknown command {:?}; use --help",
            words.join(" ")
        )));
    }
    let help = args.iter().any(|arg| arg == "--help" || arg == "-h");
    let (command, positional) = if let Some(def) = definition {
        let count = def.path.split_whitespace().count();
        if words.len() > count + usize::from(!def.arguments.is_empty()) {
            return Err(ToolkitError::arguments(format!(
                "unexpected argument {:?}", words[count]
            )));
        }
        if !def.arguments.is_empty() && words.len() == count && !help {
            return Err(ToolkitError::arguments(format!(
                "missing required argument {}",
                def.arguments[0]
            )));
        }
        (def.path.clone(), words.get(count).cloned())
    } else {
        (String::new(), None)
    };
    let mut flags = BTreeMap::new();
    let mut flag_order = Vec::new();
    let mut index = 0;
    while index < args.len() {
        let token = &args[index];
        if token == "-h" || token == "--help" {
            flags.insert("--help".into(), "true".into());
        } else if token.starts_with('-') {
            let (key, inline) = token
                .split_once('=')
                .map(|(k, v)| (k, Some(v)))
                .unwrap_or((token.as_str(), None));
            let spec = all
                .iter()
                .flat_map(|d| d.flags.iter())
                .find(|f| f.name() == key)
                .ok_or_else(|| ToolkitError::arguments(format!("unknown flag {key}")))?;
            let accepted = definition.map_or_else(
                || ["--home", "--format"].contains(&key),
                |def| def.flags.iter().any(|flag| flag.name() == key),
            );
            if !accepted {
                return Err(unsupported_route_flag(&command, key));
            }
            let repeatable = command == "update" && ["--path", "--installation"].contains(&key)
                || command == "data sql" && key == "--param";
            if flags.contains_key(key) && !repeatable {
                return Err(ToolkitError::arguments(format!("repeated flag {key}")));
            }
            let value = if spec.takes_value() {
                let value = match inline {
                    Some(v) => v.to_owned(),
                    None => {
                        index += 1;
                        args.get(index)
                            .filter(|v| !v.starts_with("--"))
                            .cloned()
                            .ok_or_else(|| {
                                ToolkitError::arguments(format!("missing value for {key}"))
                            })?
                    }
                };
                if value.is_empty() {
                    return Err(ToolkitError::arguments(format!("empty value for {key}")));
                }
                value
            } else {
                "true".into()
            };
            // Frozen Go validates each admitted Live value in this second
            // argv pass. A later repeated/route-invalid flag must not outrank it.
            // Other command families retain their existing validation path.
            if command.starts_with("live ") || command == "doctor" {
                formal_live::validate_option(key, &value)?;
            }
            if command == "data hotfix" {
                lycheedev_data::hotfix::remote::validate_option(key, &value)?;
            }
            flag_order.push((key.to_owned(), value.clone()));
            if let Some(previous) = flags.insert(key.into(), value.clone()) {
                let store = format!("__repeat:{key}");
                let mut values: Vec<String> = flags
                    .get(&store)
                    .map(|raw| serde_json::from_str(raw))
                    .transpose()?
                    .unwrap_or_else(|| vec![previous]);
                if values.len() >= 256 {
                    return Err(ToolkitError::arguments(
                        "repeated option exceeds 256 values",
                    ));
                }
                values.push(value);
                flags.insert(store, serde_json::to_string(&values)?);
            }
        }
        index += 1;
    }
    if words.is_empty() {
        flags.insert("--help".into(), "true".into());
    }
    // Existing typed-value validation stays in parse, with empty error context.
    // Full typed-option precedence is a separate parity audit from token admission.
    for (key, value) in &flag_order {
        let key = key.as_str();
        let typed_id = [
            "--spell-id",
            "--item-id",
            "--npc-id",
            "--display-id",
            "--journal-encounter-id",
        ]
        .contains(&key)
            || catalog_command(&command)
                && ["--id", "--file-data-id", "--model-file-data-id"].contains(&key);
        if typed_id
            && (value.is_empty()
                || !value.bytes().all(|byte| byte.is_ascii_digit())
                || value
                    .parse::<u32>()
                    .ok()
                    .is_none_or(|number| !["--npc-id", "--id"].contains(&key) && number == 0))
        {
            return Err(ToolkitError::arguments(format!("invalid {key}")));
        }
        if key == "--max-depth" && value.parse::<i64>().ok().is_none_or(|n| n < 0) {
            return Err(ToolkitError::arguments("invalid --max-depth"));
        }
        if command == "data item models"
            && ["--race-id", "--gender"].contains(&key)
            && value.parse::<i64>().is_err()
        {
            return Err(ToolkitError::arguments(format!("invalid {key}")));
        }
        if catalog_command(&command) {
            match key {
                "--limit"
                    if value
                        .parse::<i64>()
                        .ok()
                        .is_none_or(|n| !(1..=200000).contains(&n)) =>
                {
                    return Err(ToolkitError::arguments("invalid --limit"));
                }
                "--timeout-seconds"
                    if value
                        .parse::<i64>()
                        .ok()
                        .is_none_or(|n| !(1..=3600).contains(&n)) =>
                {
                    return Err(ToolkitError::arguments(
                        "--timeout-seconds must be between 1 and 3600",
                    ));
                }
                "--max-bytes"
                    if value
                        .parse::<i64>()
                        .ok()
                        .is_none_or(|n| !(1..=536870912).contains(&n)) =>
                {
                    return Err(ToolkitError::arguments(
                        "--max-bytes must be between 1 and 536870912",
                    ));
                }
                _ => {}
            }
        }
    }
    if command == "target resolve" {
        target_remote::validate_flags(&flags)?;
    }
    if command == "init" && !help
        && ["--plan", "--fresh", "--resume"].into_iter()
            .filter(|flag| flags.contains_key(*flag)).count() > 1
    {
        return Err(ToolkitError::arguments("init accepts at most one of --plan, --fresh, --resume"));
    }
    if command == "target add" && flags.get("--help").map(String::as_str) != Some("true")
        && (flags.get("--remote").map(String::as_str) == Some("true"))
            == flags.get("--installation").is_some_and(|path| !path.is_empty())
    {
        return Err(ToolkitError::arguments("target add requires exactly one of --installation or --remote"));
    }
    Ok(Invocation {
        command,
        positional,
        home: home(&flags),
        flags,
        cwd: std::env::current_dir()?,
        control: Default::default(),
    })
}
fn catalog_command(command: &str) -> bool {
    matches!(
        command,
        "data spell info"
            | "data item models"
            | "data item geosets"
            | "data item textures"
            | "data item get"
            | "data decor list"
            | "data decor get"
            | "data creature display"
            | "data creature model"
            | "data encounter get"
    )
}
// Project selection and installation discovery contribute independent context.
// Preserve the more specific handler diagnostic when adding project defaults.
fn merge_error_context(error: &mut ToolkitError, defaults: Value) {
    if !error.details.is_object() {
        error.details = json!({});
    }
    if !error.details["__commandContext"].is_object() {
        error.details["__commandContext"] = json!({});
    }
    let context = error.details["__commandContext"].as_object_mut().unwrap();
    if let Some(defaults) = defaults.as_object() {
        for (key, value) in defaults {
            context.entry(key.clone()).or_insert_with(|| value.clone());
        }
    }
}
fn dispatch_catalog(inv: &Invocation) -> Result<Outcome> {
    let mut selected = inv.clone();
    let project = lycheedev_core::selection::resolve_optional_project(inv)?;
    let mut context = json!({});
    if let Some((path, pin)) = project {
        selected.flags.insert("--snapshot".into(), pin.id.clone());
        context = json!({"project":path,"snapshot":pin.id});
    }
    let result = if selected.flag("--snapshot").is_none_or(str::is_empty)
        || !selected.flag_bool("--cdn") && selected.flag("--installation").is_none_or(str::is_empty)
    {
        Err(ToolkitError::arguments(format!(
            "{} requires --snapshot, and either --installation or --cdn",
            inv.command
        )))
    } else {
        lycheedev_data::dispatch(&selected)
    };
    match result {
        Ok(mut outcome) => {
            if context.get("project").is_some() {
                outcome.selection = context;
            }
            Ok(outcome)
        }
        Err(mut error) => {
            if context.get("project").is_some() {
                merge_error_context(&mut error, context);
            }
            Err(error)
        }
    }
}
fn validate_delivery_arguments(inv: &Invocation) -> Result<()> {
    let present = |name| inv.flag(name).is_some_and(|value| !value.is_empty());
    let install = match inv.command.as_str() { "addon install" => Some(("addon", "--installation")), "skill install" => Some(("skill", "--path")), _ => None };
    if let Some((component, selector)) = install {
        if !present(selector) || (!inv.flag_bool("--resume") && !present("--release"))
            || (inv.flag_bool("--resume") && (!present("--output") || present("--release"))) {
            return Err(ToolkitError::arguments(format!("{component} install requires {selector} and --release, or --resume with --output and no --release")));
        }
    }
    let remove = match inv.command.as_str() { "addon remove" => Some(("addon", "--installation")), "skill remove" => Some(("skill", "--path")), _ => None };
    if let Some((component, selector)) = remove {
        if !present(selector) || !present("--output") { return Err(ToolkitError::arguments(format!("{component} remove requires {selector} and --output"))); }
    }
    if inv.command == "addon status" && present("--installation") && present("--path") {
        return Err(ToolkitError::arguments("addon status accepts --installation or --path, not both"));
    }
    if matches!(inv.command.as_str(), "addon status" | "skill status")
        && !(inv.command == "addon status" && present("--installation")) && !present("--path") {
        return Err(ToolkitError::arguments("installation status requires --path"));
    }
    Ok(())
}
fn dispatch(inv: &Invocation) -> Result<Outcome> {
    if inv.flag_bool("--help") {
        let commands: Vec<_> = contract::implemented()
            .into_iter()
            .filter(|d| inv.command.is_empty() || d.path == inv.command)
            .collect();
        let mut flags = if inv.command.is_empty() {
            vec!["--home <root>".to_owned(), "--format text|json|jsonl".to_owned()]
        } else {
            commands.first().map(|definition| {
                definition.flags.iter().map(|flag| flag.0.clone()).collect()
            }).unwrap_or_default()
        };
        flags.push("--help".to_owned());
        return Ok(Outcome::new(json!({"commands":commands,"flags":flags})));
    }
    if catalog_command(&inv.command) {
        return dispatch_catalog(inv);
    }
    match inv.command.as_str() {
        "version" => Ok(Outcome::new(
            json!({"version":VERSION,"rust":env!("LYCHEEDEV_RUST_VERSION"),"platform":format!("{}/{}",std::env::consts::OS,if std::env::consts::ARCH=="x86_64"{"amd64"}else{std::env::consts::ARCH}),"commit":env!("LYCHEEDEV_BUILD_COMMIT"),"workspaceDirty":env!("LYCHEEDEV_BUILD_DIRTY")=="true"}),
        )),
        "describe" => Ok(Outcome::new(json!({
            "commands":contract::implemented(),
            "formats":["text", "json", "jsonl"],
            "development":VERSION.contains('-')
        }))),
        "target resolve" => dispatch_target(inv),
        "target available" => dispatch_available(inv),
        "doctor" => dispatch_doctor(inv),
        value if value.starts_with("source ") => dispatch_source(inv),
        value if value.starts_with("data ") || value.starts_with("asset ") => {
            lycheedev_data::dispatch(inv)
        }
        // Formal CON commands are selected by dispatch_formal_entry before
        // this Value-only path. Static probes/instances have no v4 lifecycle.
        value if value.starts_with("live ") => lycheedev_live::dispatch(inv),
        value
            if value.starts_with("addon ") || value.starts_with("skill ") || value == "update" =>
        {
            validate_delivery_arguments(inv)?;
            lycheedev_delivery::dispatch(inv)
        }
        value if value.starts_with("project ") => lycheedev_core::selection::dispatch_project(inv),
        "evidence verify" => {
            // The library retains its verification receipt. The frozen public
            // command returns the verified capture itself, just as show does.
            let mut outcome = lycheedev_core::evidence::dispatch(inv)?;
            outcome.data = outcome.data["capture"].take();
            Ok(outcome)
        }
        value if value.starts_with("evidence ") => lycheedev_core::evidence::dispatch(inv),
        value if value.starts_with("cache ") => lycheedev_core::cache::dispatch(inv),
        _ => lycheedev_core::workspace::dispatch(inv),
    }
}
fn dispatch_target(inv: &Invocation) -> Result<Outcome> {
    inv.control.check()?;
    target_remote::validate_flags(&inv.flags)?;
    if let Some(outcome) = target_remote::direct(inv)? {
        return Ok(outcome);
    }
    if inv.flag("--file").is_some() {
        return lycheedev_core::workspace::dispatch(inv);
    }
    if let Some(name) = inv.flag("--target").filter(|name| !name.is_empty()) {
        return target_remote::named(inv, name);
    }
    target_remote::local(inv)
}

#[derive(Clone)]
struct DeliveryAdapter;
impl lycheedev_live::managed::ManagedInstallation for DeliveryAdapter {
    fn verify_clean(
        &self,
        path: &std::path::Path,
        release: &str,
        protocol: &str,
        slots: u16,
    ) -> Result<lycheedev_live::managed::ManagedIdentity> {
        let identity = lycheedev_delivery::verify_clean(path, release, protocol, slots)?;
        Ok(lycheedev_live::managed::ManagedIdentity {
            canonical_path: identity.canonical_path,
            manifest_sha256: identity.manifest_sha256,
            generation: identity.generation,
        })
    }
    fn publish_slot(
        &self,
        identity: &lycheedev_live::managed::ManagedIdentity,
        slot: u16,
        expected_sha256: Option<&str>,
        bytes: &[u8],
    ) -> Result<()> {
        lycheedev_delivery::slots::publish(
            &lycheedev_delivery::VerifiedInstallation {
                canonical_path: identity.canonical_path.clone(),
                manifest_sha256: identity.manifest_sha256.clone(),
                generation: identity.generation,
            },
            slot,
            expected_sha256,
            bytes,
        )
    }
}
fn dispatch_source(inv: &Invocation) -> Result<Outcome> {
    if inv.command.starts_with("source session ") {
        return lycheedev_source::native_session::dispatch(inv);
    }
    if inv.command == "source prune" {
        let packaged = std::env::current_exe()?
            .parent()
            .and_then(|path| path.parent())
            .and_then(|path| path.parent())
            .map(std::path::Path::to_path_buf);
        if let Some(root) = packaged.filter(|root| root.join("release.json").is_file()) {
            let admission = broker_client_identity(&root, &inv.control, false)?;
            return lycheedev_source::dispatch_with_services(
                inv,
                lycheedev_source::SourceTools::default(),
                Some(&admission.identity),
            );
        }
    }
    // Read and validate the whole batch before release/runtime preparation;
    // retain that exact request rather than reopening a replaceable input file.
    let batch = (inv.command == "source batch").then(|| lycheedev_source::batch::read(inv)).transpose()?;
    let needs_tools = inv.command == "source validate"
        || (["source refs", "source context"].contains(&inv.command.as_str())
            || batch.as_ref().is_some_and(|request| request.needs_research()))
            && (!inv.flag_bool("--static-only") || inv.flag_bool("--flow"));
    if !needs_tools {
        return if let Some(request) = batch {
            lycheedev_source::batch::dispatch_request(inv, request, lycheedev_source::SourceTools::default())
        } else { lycheedev_source::dispatch(inv) };
    }
    let tools_result = (|| -> Result<SourceRuntimeBundle> {
    let root = inv
        .flag("--release")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::current_exe().ok().and_then(|exe| {
                exe.parent()?
                    .parent()?
                    .parent()
                    .map(std::path::Path::to_path_buf)
            })
        })
        .ok_or_else(|| {
            ToolkitError::new(
                "delivery.invalid_release",
                "unable to resolve distribution root",
                3,
            )
        })?;
    let release_profile = (std::env::var("LYCHEEDEV_SOURCE_PHASE_PROFILE").as_deref() == Ok("1"))
        .then(std::time::Instant::now);
    let release_result = lycheedev_delivery::release::inspect_with_control(&root, VERSION, &inv.control);
    if let Some(started) = release_profile {
        eprintln!("{}", json!({
            "schema":"lycheedev.source-phase.v1",
            "phase":"main.source.release_inspect",
            "elapsedMs":started.elapsed().as_secs_f64()*1000.0,
            "status":if release_result.is_ok(){"ok"}else{"error"},
            "errorCode":release_result.as_ref().err().map(|error|error.code.as_str()),
            "files":release_result.as_ref().ok().map(|release|release.resources.len()),
        }));
    }
    let release = release_result?;
    source_runtime_bundle_from_release(&root, false, &release)
    })();
    if let Some(request) = batch {
        return match tools_result {
            Ok(tools) => lycheedev_source::batch::dispatch_request(inv, request, lycheedev_source::SourceTools {
                luac: Some(&tools.luac), luals: tools.luals.as_ref(),
            }),
            Err(error) => {
                inv.control.check()?;
                if inv.flag("--release").is_some() || lycheedev_source::batch::terminal_control_error(&error) {
                    return Err(error);
                }
                // Go Runtime.Open wraps default release/resource verification
                // failures as ErrUnavailable. Never run the invalid resource.
                lycheedev_source::batch::dispatch_unavailable(inv, request, error)
            }
        };
    }
    let tools = tools_result?;
    lycheedev_source::dispatch_with_services(
        inv,
        lycheedev_source::SourceTools {
            luac: Some(&tools.luac),
            luals: tools.luals.as_ref(),
        },
        None,
    )
}
struct BrokerAdmission {
    _exe_hold: std::fs::File,
    _release_hold: std::fs::File,
    identity: lycheedev_source::session::BrokerIdentity,
    _release: lycheedev_delivery::release::Release,
}
fn broker_client_identity(
    root: &std::path::Path,
    control: &lycheedev_core::control::OperationControl,
    full: bool,
) -> Result<BrokerAdmission> {
    let root = root.canonicalize()?;
    let exe = std::env::current_exe()?.canonicalize()?;
    let (exe_hold, exe_sha) = lycheedev_core::storage::locked_sha256(&exe, 128 << 20, control)?;
    let (release_hold, release_sha) =
        lycheedev_core::storage::locked_sha256(&root.join("release.json"), 1 << 20, control)?;
    let release = if full {
        lycheedev_delivery::release::inspect_with_control(&root, VERSION, control)?
    } else {
        lycheedev_delivery::release::read_manifest_with_control(&root, VERSION, control)?
    };
    let binary = &release.binaries["windows-amd64"];
    if root.join(&binary.path).canonicalize()? != exe || binary.sha256 != exe_sha {
        return Err(ToolkitError::invalid(
            "broker client executable is not the exact sealed release native binary",
        ));
    }
    Ok(BrokerAdmission {
        _exe_hold: exe_hold,
        _release_hold: release_hold,
        identity: lycheedev_source::session::BrokerIdentity {
            exe_path: exe,
            exe_sha256: exe_sha,
            release_root: root,
            release_manifest_sha256: release_sha,
        },
        _release: release,
    })
}
struct SourceRuntimeBundle {
    luac: lycheedev_source::luac::Runtime,
    luals: Option<lycheedev_source::luals::Runtime>,
}
fn dispatch_available(inv: &Invocation) -> Result<Outcome> {
    let region = inv
        .flag("--region")
        .filter(|region| !region.is_empty())
        .ok_or_else(|| ToolkitError::arguments("target available requires --region"))?;
    let value = lycheedev_core::availability::list(
        region,
        inv.flag_bool("--offline"),
        64,
        &inv.control,
        &mut lycheedev_data::availability::fetch_versions,
    )?;
    let failed = value["products"]
        .as_array()
        .unwrap()
        .iter()
        .any(|product| product.get("error").is_some());
    let mut result = Outcome::new(value);
    if failed {
        result.warnings.push(json!("At least one product manifest could not be read; inspect products[].error for the exact failure."));
    }
    Ok(result)
}
const EMMY_DOCTOR_FILE: &str = "doctor-smoke.lua";
const EMMY_DOCTOR_MANIFEST: &str = "DoctorSmoke.toc";
const EMMY_DOCTOR_SYMBOL: &str = "LycheeDoctorTarget";
const EMMY_DOCTOR_SOURCE: &str =
    "local function LycheeDoctorTarget()\n    return 7\nend\nlocal answer = LycheeDoctorTarget()\n";
const EMMY_DOCTOR_EXECUTION_SECONDS: u64 = 5;

fn emmy_doctor_request() -> Result<lycheedev_source::native_research::WorkerRequest> {
    use lycheedev_source::{
        index::Symbol,
        native::LuacIdentity,
        native_research::{LoadEvidence, QueryRequest, SnapshotRequest, WorkerRequest},
    };
    // The native input contract needs a pinned syntax-checker identity. This is
    // admission metadata only: this smoke neither loads nor verifies luac.
    let pinned: Value = serde_json::from_str(include_str!("../../../release/tools/luac.json"))?;
    let luac: lycheedev_source::luac::Manifest = serde_json::from_value(pinned["runtime"].clone())?;
    Ok(WorkerRequest {
        snapshot: SnapshotRequest {
            environment_documents: BTreeMap::new(),
            files: BTreeMap::from([(EMMY_DOCTOR_FILE.into(), EMMY_DOCTOR_SOURCE.into())]),
            load_documents: BTreeMap::from([(
                EMMY_DOCTOR_MANIFEST.into(),
                format!("{EMMY_DOCTOR_FILE}\n"),
            )]),
            // This product belongs solely to the built-in fixture's load rules;
            // doctor does not select or access a user's client/source snapshot.
            source: json!({"repository":"lycheedev-doctor-smoke","product":"retail","sourceExecution":false}),
            luac: LuacIdentity {
                version: luac.version,
                platform: luac.platform,
                executable_sha256: luac.sha256,
                executable_bytes: luac.bytes,
            },
            api: vec![],
            environment_manifest: None,
            load_plans: vec![LoadEvidence {
                manifest: EMMY_DOCTOR_MANIFEST.into(),
                ordered: vec![EMMY_DOCTOR_FILE.into()],
                issues: vec![],
            }],
            input_gaps: vec![],
        },
        query: QueryRequest {
            symbol: Symbol {
                id: String::new(),
                kind: "function".into(),
                name: EMMY_DOCTOR_SYMBOL.into(),
                target: String::new(),
                category: String::new(),
                confidence: String::new(),
                path: EMMY_DOCTOR_FILE.into(),
                line: 1,
                end_line: 3,
                signature: String::new(),
                scope: String::new(),
                ordinal: 0,
            },
            direction: "incoming".into(),
        },
    })
}
fn validate_emmy_doctor_result(value: &Value) -> Result<()> {
    let expected_call = value["relations"].as_array().is_some_and(|rows| {
        rows.iter().any(|row| {
            row["kind"] == "call"
                && row["state"] == "resolved"
                && row["target"] == EMMY_DOCTOR_SYMBOL
                && row["path"] == EMMY_DOCTOR_FILE
                && row["line"] == 4
        })
    });
    if value["schema"] != "lycheedev.source-native-research.v1"
        || value["analysis"]["engine"] != "native_common_facts"
        || value["analysis"]["state"] != "complete"
        || value["analysis"]["sourceExecution"] != false
        || value["analysis"]["sourceFiles"] != 1
        || !lycheedev_core::selection::valid_digest(
            value["inputIdentity"].as_str().unwrap_or(""),
            64,
        )
        || value["lifecycle"]["owner"] != "source_owned_process"
        || value["lifecycle"]["mode"] != "one_shot"
        || value["lifecycle"]["retired"] != true
        || !expected_call
    {
        return Err(ToolkitError::new(
            "source.emmy_doctor_mismatch",
            "embedded Emmy worker did not prove the expected bounded fixture call and retirement",
            3,
        ));
    }
    Ok(())
}
fn emmy_doctor_check_with(
    inv: &Invocation,
    run: impl FnOnce(&Invocation, lycheedev_source::native_research::WorkerRequest) -> Result<Value>,
) -> Result<Value> {
    inv.control.check()?;
    let mut probe = inv.clone();
    probe.control = inv.control.ordered_child(
        std::time::Instant::now() + std::time::Duration::from_secs(EMMY_DOCTOR_EXECUTION_SECONDS),
    )?;
    let observed = (|| -> Result<Value> {
        let value = run(&probe, emmy_doctor_request()?)?;
        probe.control.check()?;
        validate_emmy_doctor_result(&value)?;
        Ok(value)
    })();
    // Caller cancellation/deadline still terminates doctor; only a probe-local
    // failure becomes a warning. The owned process runner retains its separate
    // bounded retirement grace after terminating a timed-out child.
    inv.control.check()?;
    let mut check = json!({
        "id":"emmy.runtime",
        "scope":"embedded-worker-fixture",
        "engineCommit":lycheedev_source::native::ENGINE_COMMIT,
        "executionBudgetSeconds":EMMY_DOCTOR_EXECUTION_SECONDS,
        "retirementGraceSeconds":5,
        "sourceExecution":false,
        "notChecked":["persistent_session","luac_runtime","user_source_analysis"],
        "releaseIntegrityCheck":"release.integrity"
    });
    match observed {
        Ok(value) => {
            check["ok"] = json!(true);
            check["detail"] = json!(
                "embedded Emmy worker parsed and resolved the built-in call fixture and retired"
            );
            check["inputIdentity"] = value["inputIdentity"].clone();
            check["lifecycle"] = value["lifecycle"].clone();
        }
        Err(error) if error.exit_code == 7 => return Err(error),
        Err(error) => {
            check["ok"] = json!(false);
            check["status"] = json!("warn");
            check["detail"] = json!(error.message);
            check["code"] = json!(error.code);
            if !error.details.is_null() {
                check["details"] = error.details;
            }
            check["nextStep"] = json!(
                "inspect the embedded Emmy worker failure and reinstall a verified Rust candidate if needed; this check does not validate persistent sessions or the bundled syntax compiler"
            );
        }
    }
    Ok(check)
}
fn release_doctor_check_with(
    inv: &Invocation,
    inspect: impl FnOnce() -> Result<lycheedev_delivery::release::Release>,
) -> Result<Value> {
    inv.control.check()?;
    let observed = inspect();
    inv.control.check()?;
    match observed {
        Ok(release) => Ok(json!({
            "id":"release.integrity","ok":true,
            "scope":"manifest-and-payload-inventory",
            "detail":"distribution manifest and complete payload inventory verified",
            "version":release.version,"resourceCount":release.resources.len()
        })),
        Err(error) if error.exit_code == 7 || error.code == "command.deadline" => Err(error),
        Err(error) => Ok(json!({
            "id":"release.integrity","ok":false,"status":"warn",
            "scope":"manifest-and-payload-inventory",
            "detail":error.message,"code":error.code,
            "nextStep":"install the complete verified lycheedev npm package or native release; inspect missing or modified release resources"
        })),
    }
}
fn doctor_provider_checks_with(
    inv: &Invocation,
    inspect: impl FnOnce() -> Result<lycheedev_delivery::release::Release>,
    run: impl FnOnce(&Invocation, lycheedev_source::native_research::WorkerRequest) -> Result<Value>,
) -> Result<Vec<Value>> {
    // Package integrity remains an independent diagnostic. A broken package
    // warns without suppressing the embedded worker smoke or workspace checks.
    let release = release_doctor_check_with(inv, inspect)?;
    let engine = emmy_doctor_check_with(inv, run)?;
    Ok(vec![release, engine])
}
fn dispatch_doctor(inv: &Invocation) -> Result<Outcome> {
    inv.control.check()?;
    #[cfg(windows)]
    let (home_key, home_display) = ("USERPROFILE", "%userprofile%");
    #[cfg(not(windows))]
    let (home_key, home_display) = ("HOME", "$HOME");
    let user_home = std::env::var_os(home_key)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            ToolkitError::new(
                "command.failed",
                format!("{home_display} is not defined"),
                5,
            )
        })?;
    let local_app_data = std::env::var_os("LOCALAPPDATA");
    let legacy = lycheedev_core::doctor::legacy_candidates(
        Some(std::path::Path::new(&user_home)),
        local_app_data.as_deref().map(std::path::Path::new),
    );
    lycheedev_core::doctor::inspect_with_provider(inv, &legacy, || {
        let mut providers = doctor_provider_checks_with(
            inv,
            || {
                // Preserve the original installed-layout lookup and full, bounded
                // inventory verification, under the caller's unchanged operation budget.
                let executable = std::env::current_exe()?;
                let root = executable
                    .parent()
                    .and_then(|path| path.parent())
                    .and_then(|path| path.parent())
                    .ok_or_else(|| {
                        ToolkitError::new(
                            "delivery.invalid_release",
                            "complete distribution root unavailable",
                            3,
                        )
                    })?;
                lycheedev_delivery::release::inspect_with_control(root, VERSION, &inv.control)
            },
            lycheedev_source::native_research::one_shot,
        )?;
        providers.extend(formal_live::doctor_checks(inv));
        let project = lycheedev_core::selection::find_project_controlled(&inv.cwd, &inv.control)
            .and_then(|directory| {
                lycheedev_core::selection::inspect_project_controlled(&directory, &inv.control)
            });
        inv.control.check()?;
        if let Ok(project) = project
            && let Some(product) = project["project"]["product"].as_str()
        {
            let observed = lycheedev_source::git::mirror_health_with_control(
                &inv.home,
                "wow-ui-source",
                product,
                &inv.control,
            );
            inv.control.check()?;
            let detail = match &observed {
                Ok(value) if value["exists"] == false => {
                    format!("mirror for {product} is not initialized yet")
                }
                Ok(_) => format!(
                    "mirror for {product} is a contained bare SHA-1 repository; commit/object authentication is not covered by this format check"
                ),
                Err(error) => format!("mirror health probe failed: {}", error.message),
            };
            providers.push(json!({"id":"codebase.source_mirror","ok":true,"status":"unknown","detail":format!("{detail}; remote currency {}",if inv.flag_bool("--offline"){"not checked (--offline)"}else{"unknown"}),"nextStep":format!("run source sync --source wow-ui-source --product {product} to prepare or refresh the mirror"),"scope":"repository-format","objectsVerified":false}));
        }
        Ok(providers)
    })
}
fn source_runtime_bundle_from_release(
    root: &std::path::Path,
    semantic: bool,
    release: &lycheedev_delivery::release::Release,
) -> Result<SourceRuntimeBundle> {
    let identity: Value = serde_json::from_str(include_str!("../../../release/tools/luac.json"))?;
    let runtime: lycheedev_source::luac::Manifest =
        serde_json::from_value(identity["runtime"].clone())?;
    let packaged: Value =
        lycheedev_core::storage::read_json(&root.join("payload/tool/luac/runtime.json"))?;
    if packaged != identity["runtime"] {
        return Err(ToolkitError::new(
            "luac.unavailable",
            "bundled compiler manifest differs from pinned identity",
            3,
        ));
    }
    let luac = lycheedev_source::luac::Runtime::verified(&root.join("payload/tool/luac"), runtime)?;
    let luals = if semantic {
        let inventory = release
            .resources
            .iter()
            .filter_map(|resource| {
                resource
                    .path
                    .strip_prefix("tool/luals/")
                    .map(|name| (name.to_owned(), resource.sha256.clone()))
            })
            .collect();
        Some(lycheedev_source::luals::Runtime::prepare_inspected_release(
            &root.join("payload/tool/luals"),
            inventory,
        )?)
    } else {
        None
    };
    Ok(SourceRuntimeBundle { luac, luals })
}
fn broker_options(args: &[String]) -> Result<BTreeMap<String, String>> {
    let accepted = [
        "--home",
        "--release",
        "--parent-pid",
        "--parent-created",
        "--start-token",
    ];
    if args.len() != 11
        || args
            .first()
            .is_none_or(|arg| arg != "--source-broker-server")
    {
        return Err(ToolkitError::arguments(
            "private Source broker startup requires exact owned arguments",
        ));
    }
    let mut options = BTreeMap::new();
    for pair in args[1..].as_chunks::<2>().0 {
        if !accepted.contains(&pair[0].as_str())
            || pair[1].is_empty()
            || options.insert(pair[0].clone(), pair[1].clone()).is_some()
        {
            return Err(ToolkitError::arguments(
                "private Source broker startup contains unknown or duplicate arguments",
            ));
        }
    }
    Ok(options)
}
fn broker_server(args: &[String]) -> Result<()> {
    let flags = broker_options(args)?;
    let token = &flags["--start-token"];
    if !lycheedev_core::selection::valid_digest(token, 32)
        || std::env::var("LYCHEEDEV_BROKER_START").ok().as_ref() != Some(token)
    {
        return Err(ToolkitError::invalid("broker parent startup token differs"));
    }
    let pid = flags["--parent-pid"]
        .parse::<u32>()
        .ok()
        .filter(|pid| *pid != 0)
        .ok_or_else(|| ToolkitError::arguments("broker parent PID is invalid"))?;
    let created = flags["--parent-created"]
        .parse::<u64>()
        .ok()
        .filter(|value| *value != 0)
        .ok_or_else(|| ToolkitError::arguments("broker parent creation identity is invalid"))?;
    let fault = |message: String| ToolkitError::new("luals.unavailable", message, 3);
    let console = lycheedev_windows::cancellation::ConsoleCancellation::install().map_err(fault)?;
    let control = lycheedev_core::control::OperationControl::new(
        std::time::Instant::now() + std::time::Duration::from_secs(600),
        console.token(),
    );
    let own = lycheedev_windows::Process::open(std::process::id()).map_err(fault)?;
    if own.parent_pid().map_err(fault)? != pid {
        return Err(ToolkitError::invalid("broker actual parent PID differs"));
    }
    let parent = lycheedev_windows::Process::open(pid).map_err(fault)?;
    if parent.identity().created != created {
        return Err(ToolkitError::invalid(
            "broker parent creation identity differs",
        ));
    }
    let exe = std::env::current_exe()?.canonicalize()?;
    if parent.image_path().map_err(fault)?.canonicalize()? != exe {
        return Err(ToolkitError::invalid(
            "broker parent executable path differs",
        ));
    }
    let root = PathBuf::from(&flags["--release"]).canonicalize()?;
    let home = PathBuf::from(&flags["--home"]).canonicalize()?;
    if !PathBuf::from(&flags["--home"]).is_absolute()
        || !PathBuf::from(&flags["--release"]).is_absolute()
    {
        return Err(ToolkitError::arguments("broker roots must be absolute"));
    }
    let (_exe_hold, exe_sha) = lycheedev_core::storage::locked_sha256(&exe, 128 << 20, &control)?;
    let (_release_hold, release_sha) =
        lycheedev_core::storage::locked_sha256(&root.join("release.json"), 1 << 20, &control)?;
    let release = lycheedev_delivery::release::inspect_with_control(&root, VERSION, &control)?;
    let binary = &release.binaries["windows-amd64"];
    if root.join(&binary.path).canonicalize()? != exe || binary.sha256 != exe_sha {
        return Err(ToolkitError::invalid(
            "broker executable is not this sealed release's native binary",
        ));
    }
    let inv = Invocation {
        command: String::new(),
        positional: None,
        flags,
        home,
        cwd: std::env::current_dir()?,
        control,
    };
    let tools = source_runtime_bundle_from_release(&root, true, &release)?;
    parent.verify().map_err(fault)?;
    let identity = lycheedev_source::session::BrokerIdentity {
        exe_path: exe,
        exe_sha256: exe_sha,
        release_root: root,
        release_manifest_sha256: release_sha,
    };
    lycheedev_source::session::consume_startup(
        &inv,
        &identity,
        pid,
        created,
        &inv.flags["--start-token"],
    )?;
    lycheedev_source::session::serve(
        &inv,
        lycheedev_source::SourceTools {
            luac: Some(&tools.luac),
            luals: tools.luals.as_ref(),
        },
        identity,
    )
}
fn envelope(result: &Result<Outcome>) -> Value {
    match result {
        Ok(outcome) => {
            let mut context = if outcome.selection["schema"] == "lycheedev.selection.v1" {
                json!({"snapshot":outcome.selection["id"]})
            } else {
                outcome.selection.clone()
            };
            if !outcome.coverage.as_object().is_some_and(|m| m.is_empty())
                && outcome.data.get("coverage") != Some(&outcome.coverage)
            {
                context["coverage"] = outcome.coverage.clone();
            }
            json!({"schema":"lycheedev.result.v1","ok":true,"operationId":outcome.operation["operationId"].as_str().unwrap_or(""),"context":context,"result":outcome.data,"captures":outcome.provenance,"warnings":outcome.warnings,"error":null})
        }
        Err(error) => {
            let mut fault = json!({"code":error.code,"message":error.message,"stage":error.details["stage"].as_str().unwrap_or("dispatch"),"retryable":error.exit_code==6,"resumeOperationId":error.details["operationId"].as_str().unwrap_or("")});
            if let Some(location) = error.details.get("location") {
                fault["location"] = location.clone();
            }
            if let Some(details) = error.details.as_object() {
                let mut public = details.clone();
                public.remove("stage");
                public.remove("__commandContext");
                public.remove("location");
                if !public.is_empty() {
                    fault["details"] = Value::Object(public);
                }
            } else if !error.details.is_null() {
                fault["details"] = error.details.clone();
            }
            json!({"schema":"lycheedev.result.v1","ok":false,"operationId":error.details["operationId"].as_str().unwrap_or(""),"context":error.details.get("__commandContext").or_else(||error.details.get("context")).cloned().unwrap_or(json!({})),"result":error.details.get("result").cloned().unwrap_or(Value::Null),"captures":error.details.get("captures").cloned().unwrap_or(json!([])),"warnings":error.details.get("warnings").and_then(Value::as_array).filter(|warnings|warnings.iter().all(Value::is_string)).cloned().unwrap_or_default(),"error":fault})
        }
    }
}
fn render(out: &mut impl Write, format: &str, result: &Result<Outcome>) -> Result<()> {
    render_with_context(out, format, result, None)
}
fn render_with_context(
    out: &mut impl Write,
    format: &str,
    result: &Result<Outcome>,
    invocation: Option<&Invocation>,
) -> Result<()> {
    let mut value = envelope(result);
    if let Some(inv) = invocation
        && !inv.flag_bool("--help")
        && let Some(seconds) = data_timeout_seconds(inv)?
    {
        value["context"]["timeoutSeconds"] = json!(seconds);
    }
    use lycheedev_core::public_json::{self, NumberLayout};
    let numbers = result.as_ref().map(|outcome| &outcome.public_numbers)
        .unwrap_or(&NumberLayout::Verbatim);
    match format {
        "json" => public_json::write_line_at(out, &value, &["result"], numbers)?,
        "jsonl" => {
            public_json::write_line(out, &json!({"type":"begin","schema":"lycheedev.result.v1"}), &NumberLayout::Verbatim)?;
            match result {
                Ok(outcome) => {
                    public_json::write_line_at(out, &json!({"type":"record","result":outcome.data}), &["result"], numbers)?;
                    public_json::write_line_at(out, &json!({"type":"end","envelope":value}), &["envelope", "result"], numbers)?;
                }
                Err(_) => public_json::write_line(out, &json!({"type":"error","envelope":value}), &NumberLayout::Verbatim)?,
            }
        }
        _ => match result {
            Ok(outcome) => public_json::write_pretty_line(out, &outcome.data, numbers)?,
            Err(error) => writeln!(out, "{error}")?,
        },
    }
    out.flush()?;
    Ok(())
}
fn data_timeout_seconds(inv: &Invocation) -> Result<Option<u64>> {
    if contract::all().iter().any(|definition| {
        definition.path == inv.command
            && definition
                .flags
                .iter()
                .any(|flag| flag.name() == "--timeout-seconds")
    }) {
        Ok(Some(
            inv.usize_flag("--timeout-seconds", 300, 1, 3600)? as u64
        ))
    } else {
        Ok(None)
    }
}
fn operation_seconds(inv: &Invocation) -> Result<u64> {
    if matches!(inv.command.as_str(), "source refs" | "source context" | "source batch") { return Ok(90); }
    if inv.command == "update" {
        return Ok(600);
    }
    let command = contract::all()
        .iter()
        .find(|definition| definition.path == inv.command);
    if let Some(seconds) = data_timeout_seconds(inv)? {
        return Ok(seconds);
    }
    if command.as_ref().is_some_and(|definition| {
        definition
            .flags
            .iter()
            .any(|flag| flag.name() == "--wait-seconds")
    }) {
        return Ok(inv.usize_flag("--wait-seconds", 120, 1, 600)? as u64 + 60);
    }
    Ok(180)
}
/// The normal CLI entry's one cancellation scope. Keep registration before
/// budget validation so its existing error priority and guard lifetime remain
/// unchanged. The installer may signal before the control is constructed.
fn install_invocation_control<G>(
    parsed: &mut Result<Invocation>,
    install: impl FnOnce(lycheedev_core::control::TrackedCancellation) -> std::result::Result<G, String>,
) -> Option<G> {
    let Ok(inv) = parsed else {
        return None;
    };
    if ["", "version", "describe"].contains(&inv.command.as_str()) || inv.flag_bool("--help") {
        return None;
    }
    let cancellation = lycheedev_core::control::TrackedCancellation::new();
    let handler = match install(cancellation.clone()) {
        Ok(handler) => handler,
        Err(message) => {
            *parsed = Err(ToolkitError::new("windows.console_unavailable", message, 3));
            return None;
        }
    };
    let seconds = match operation_seconds(inv) {
        Ok(seconds) => seconds,
        Err(error) => {
            *parsed = Err(error);
            return None;
        }
    };
    if seconds == 0 {
        return None;
    }
    inv.control = lycheedev_core::control::OperationControl::tracked(
        std::time::Instant::now() + std::time::Duration::from_secs(seconds),
        cancellation,
    );
    Some(handler)
}
// This marker is emitted only after Data receives a committed capture ID.
// Validate the typed facts, not prose, before retaining the late-failure context.
fn asset_committed_context(error: &ToolkitError) -> bool {
    let details = &error.details;
    if details["schema"] != "lycheedev.asset-committed-error.v1"
        || !details["outputPublished"].is_boolean()
    {
        return false;
    }
    let Ok(primary) = serde_json::from_value::<ToolkitError>(details["primary"].clone()) else {
        return false;
    };
    let Some(captures) = details["completedCaptures"].as_array() else {
        return false;
    };
    primary.code == error.code
        && primary.exit_code == error.exit_code
        && (1..=3).contains(&captures.len())
        && captures.iter().all(|id| {
            id.as_str().is_some_and(|id| {
                id.len() == 68
                    && id.starts_with("CAP-")
                    && id.as_bytes()[4..].iter().all(|byte| {
                        byte.is_ascii_digit() || (b'a'..=b'f').contains(byte)
                    })
            })
        })
}
fn capture_commit_leaf(error: &ToolkitError) -> bool {
    let details = &error.details;
    if details["schema"] != "lycheedev.capture-commit-error.v1" {
        return false;
    }
    let Ok(primary) = serde_json::from_value::<ToolkitError>(details["primary"].clone()) else {
        return false;
    };
    let Some(id) = details["capturePublication"]["id"].as_str() else {
        return false;
    };
    if primary.code != error.code || primary.exit_code != error.exit_code
        || id.len() != 68 || !id.starts_with("CAP-")
        || !id.as_bytes()[4..].iter().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
    {
        return false;
    }
    let Some(completed) = details["completedCaptures"].as_array() else {
        return false;
    };
    match details["capturePublication"]["state"].as_str() {
        Some("committed") => completed.len() == 1 && completed[0].as_str() == Some(id),
        Some("unknown") => completed.is_empty(),
        _ => false,
    }
}
// Publication facts use the existing error.details envelope. Demux has one
// source, up to 2^24 frames and one manifest; its bound is not export's three.
fn asset_publication_context(error: &ToolkitError) -> bool {
    let details = &error.details;
    let Ok(primary) = serde_json::from_value::<ToolkitError>(details["primary"].clone()) else {
        return false;
    };
    if primary.code != error.code || primary.exit_code != error.exit_code {
        return false;
    }
    let nonempty_path = |value: &Value| value.as_str().is_some_and(|path| !path.is_empty());
    match details["schema"].as_str() {
        Some("lycheedev.asset-publication-error.v1") => {
            matches!(details["publication"]["state"].as_str(),
                Some("not_published" | "published" | "unknown"))
                && nonempty_path(&details["publication"]["path"])
        }
        Some("lycheedev.demux-committed-error.v1") => {
            let (Some(captures), Some(outputs)) =
                (details["completedCaptures"].as_array(), details["publishedOutputs"].as_array())
            else { return false; };
            (1..=((1usize << 24) + 2)).contains(&captures.len())
                && outputs.len() <= (1usize << 24) + 1
                && outputs.iter().all(nonempty_path)
                && captures.iter().all(|value| value.as_str().is_some_and(|id| {
                    id.len() == 68 && id.starts_with("CAP-")
                        && id.as_bytes()[4..].iter().all(|byte|
                            byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
                }))
        }
        _ => false,
    }
}
// Traverse only the typed cleanup envelopes emitted by the actual owner
// boundaries. Arbitrary nested JSON cannot authenticate publication facts.
fn capture_commit_context(error: &ToolkitError) -> bool {
    let mut pending = vec![error.clone()];
    while let Some(error) = pending.pop() {
        if capture_commit_leaf(&error) || asset_committed_context(&error) || asset_publication_context(&error) {
            return true;
        }
        let details = &error.details;
        let values: Vec<&Value> = match details["schema"].as_str() {
            Some("lycheedev.file-cleanup.v1") => {
                let mut values = Vec::new();
                for key in ["primary", "cleanup", "control"] {
                    let Some(value) = details.get(key) else { return false; };
                    if !value.is_null() { values.push(value); }
                }
                values
            }
            Some("lycheedev.admitted-workspace-errors.v1") => {
                let Some(causes) = details["causes"].as_array() else { return false; };
                causes.iter().collect()
            }
            _ => continue,
        };
        if values.len() < 2 { return false; }
        let Ok(causes) = values.into_iter()
            .map(|value| serde_json::from_value::<ToolkitError>(value.clone()))
            .collect::<std::result::Result<Vec<_>, _>>() else { return false; };
        if causes[0].code != error.code || causes[0].exit_code != error.exit_code {
            return false;
        }
        pending.extend(causes.into_iter().rev());
    }
    false
}
fn command_fault(inv: &Invocation, mut error: ToolkitError) -> ToolkitError {
    if inv.command == "target resolve" && inv.flag("--target").is_some_and(|name| !name.is_empty()) {
        error = target_remote::named_fault(error);
    }
    if matches!(inv.command.as_str(), "target list" | "target add" | "target show" | "target remove") {
        if matches!(error.code.as_str(), "selection.target_missing" | "selection.target_exists" | "selection.target_ambiguous" | "selection.target_in_use" | "selection.target_format") {
            if !error.details.is_object() {
                error.details = if error.details.is_null() { json!({}) } else { json!({"primaryDetails":error.details}) };
            }
            error.details["stage"] = json!("query");
        } else if matches!(error.code.as_str(), "selection.unsupported_data_product" | "selection.unsupported_data_language" | "selection.unsupported_data_region") {
            error.code = "command.failed".into();
            error.exit_code = 5;
        }
    }
    if inv.command == "target resolve" && inv.flag("--file").is_some() {
        // Selection-file errors are ordinary Go errors rather than queryFault
        // sentinels. Keep full typed cleanup/publication facts at this boundary.
        if error.code.starts_with("selection.") || matches!(error.code.as_str(), "input.invalid" | "external.io" | "vault.metadata") {
            error.code = "command.failed".into();
            error.exit_code = 5;
        }
    }
    if inv.command == "target resolve" && inv.flag("--file").is_some() && error.code == "vault.workspace_schema_newer" {
        error.code = "vault.unsupported_workspace".into();
    }
    if inv.command.starts_with("project ") {
        if matches!(error.code.as_str(), "selection.project_missing" | "selection.project_unlocked" | "selection.project_format" | "selection.project_conflict") {
            if !error.details.is_object() { error.details = if error.details.is_null() { json!({}) } else { json!({"primaryDetails":error.details}) }; }
            error.details["stage"] = json!("selection");
        } else if matches!(error.code.as_str(), "external.io" | "input.invalid" | "vault.metadata") {
            error.code = "command.failed".into();
            error.exit_code = 5;
        } else if error.code == "vault.workspace_schema_newer" {
            error.code = "vault.unsupported_workspace".into();
        }
    }
    // Go entry.queryFault projects installation selection errors for every
    // command, including addon handlers, rather than only catalog queries.
    if matches!(error.code.as_str(), "selection.installation_missing" | "selection.installation_ambiguous" | "selection.installation_product_mismatch" | "selection.installation_metadata_conflict") {
        if !error.details.is_object() { error.details = json!({}); }
        error.details["stage"] = json!(if inv.command == "target resolve" { "selection" } else { "query" });
    }
    // Frozen Go's selected-content byte boundary is a query failure. Keep the
    // diagnostic inside Data, and project the observed public asset envelope.
    if matches!(inv.command.as_str(), "asset inspect" | "asset export")
        && error.code == "records.metadata_limit"
    {
        if asset_committed_context(&error) || capture_commit_context(&error) {
            // Keep the completed side effects and every original cleanup cause;
            // the public error message tells callers not to replay this work.
            error.details["stage"] = json!("query");
        } else {
            error.message = error.code.clone();
            error.details = json!({"stage":"query"});
        }
    }
    if catalog_command(&inv.command) {
        if error.code == "schema.definition_missing" {
            error.code = "schema.definition_not_found".into();
        }
        if matches!(
            error.code.as_str(),
            "schema.definition_not_found"
                | "schema.invalid_definition"
                | "schema.ambiguous_definition"
                | "schema.limit_exceeded"
                | "records.definition_unavailable_offline"
        ) {
            error.message = error.code.clone();
        }
        // Frozen Go queryFault deliberately does not map table failures. Keep
        // typed library errors internally and translate only this public seam.
        let stage = match error.code.as_str() {
            code if code.starts_with("table.") => {
                error.message = error.code.clone();
                error.code = "command.failed".into();
                error.exit_code = 5;
                Some("dispatch")
            }
            "records.content_missing" | "records.file_ambiguous" => {
                error.message = if error.code == "records.content_missing" {
                    "records.content_not_found".into()
                } else {
                    error.code.clone()
                };
                error.code = "command.failed".into();
                error.exit_code = 5;
                Some("dispatch")
            }
            "selection.project_missing"
            | "selection.project_unlocked"
            | "selection.project_format"
            | "selection.project_conflict" => Some("selection"),
            "records.query_limit"
            | "records.request_conflict"
            | "records.field_unknown"
            | "records.field_type_mismatch"
            | "records.record_not_found"
            | "records.relation_cycle"
            | "query.budget_exceeded"
            | "records.definition_unavailable_offline"
            | "records.definition_identity"
            | "schema.definition_not_found"
            | "schema.invalid_definition"
            | "schema.ambiguous_definition"
            | "schema.limit_exceeded"
            | "records.query_resource_budget"
            | "records.pinned_build_changed"
            | "records.local_object_unavailable"
            | "records.configuration_integrity"
            | "records.invalid_metadata"
            | "records.metadata_limit"
            | "records.build_unavailable"
            | "records.build_ambiguous"
            | "container.key_unavailable"
            | "container.unsupported_encoding"
            | "container.resource_limit"
            | "container.invalid_format"
            | "container.integrity_mismatch"
            | "selection.installation_missing"
            | "selection.installation_ambiguous"
            | "selection.installation_product_mismatch"
            | "selection.installation_metadata_conflict"
            | "selection.data_product_build_mismatch" => Some("query"),
            _ => None,
        };
        if let Some(stage) = stage {
            if !error.details.is_object() {
                error.details = json!({});
            }
            error.details["stage"] = json!(stage);
        }
    }
    if inv.command == "target available" && error.code == "selection.listing_requires_network" {
        error.details = json!({"stage":"query"});
    }
    if inv.command == "asset export"
        && (error.code.starts_with("texture.")
            || [
                "records.image_output_limit",
                "records.export_conflict",
                "records.export_path",
                "records.export_options",
            ]
            .contains(&error.code.as_str()))
    {
        if !error.details.is_object() {
            error.details = json!({});
        }
        error.details["stage"] = json!("export");
    }
    if ["source refs", "source context"].contains(&inv.command.as_str())
        && ["codebase.invalid_symbol", "codebase.invalid_search_cursor"]
            .contains(&error.code.as_str())
    {
        error.message = error.code.clone();
        if !error.details.is_object() {
            error.details = json!({});
        }
        error.details["stage"] = json!("query");
        error.details["__commandContext"] =
            json!({"snapshot":inv.flag("--snapshot").unwrap_or("")});
    }
    if inv.command == "source batch" && error.code.starts_with("codebase.") {
        if !error.details.is_object() { error.details = json!({}); }
        error.details["stage"] = json!("query");
        if let Some(snapshot) = inv.flag("--snapshot") {
            error.details["__commandContext"] = json!({"snapshot":snapshot});
        }
    }
    match error.code.as_str() {
        "process.cancelled" | "operation.cancelled" => {
            error.code = "command.cancelled".into();
            error.exit_code = 7;
        }
        "vault.generation_conflict" => error.exit_code = 3,
        "vault.record_missing" | "evidence.capture_referenced" => error.exit_code = 2,
        "evidence.reference_scan_limit" => error.exit_code = 3,
        "codebase.incompatible_comparison"
        | "codebase.incomplete_index"
        | "codebase.source_pin_required"
        | "codebase.index_not_ready"
            if inv.command == "source diff" =>
        {
            error.code = "command.failed".into()
        }
        _ => {}
    }
    if error.message == "External process execution deadline exceeded"
        && let Err(control) = inv.control.check()
    {
        error.code = control.code;
        error.exit_code = control.exit_code;
    }
    if error.code == "command.deadline" && inv.command.starts_with("live ") {
        error.code = "live.deadline_pending".into();
        error.exit_code = 6;
    }
    error
}
struct StreamWriter<'a, W: Write> {
    out: &'a mut W,
    yielded: bool,
}
impl<W: Write> StreamWriter<'_, W> {
    fn emit(&mut self, event: lycheedev_data::stream::StreamEvent<'_>) -> Result<()> {
        self.emit_value(&lycheedev_data::stream::wire::event(event))
    }
    fn emit_value(&mut self, frame: &Value) -> Result<()> {
        self.yielded = true;
        lycheedev_core::public_json::write_line(
            self.out, frame, &lycheedev_core::public_json::NumberLayout::Verbatim,
        )?;
        self.out.flush()?;
        Ok(())
    }
}
fn stream_fault(mut error: ToolkitError, yielded: bool) -> ToolkitError {
    if yielded && error.code == "command.deadline" {
        error.exit_code = 7;
    }
    error
}
/// Shared production/test dispatch boundary. Formal results must never enter
/// Outcome.data or the generic Value renderer used by Source/Data.
fn dispatch_formal_entry_with(
    inv: &Invocation,
    run: impl FnOnce(&Invocation) -> formal_live::Response,
) -> Option<formal_live::Response> {
    if !formal_live::handles(inv) { return None; }
    Some(match inv.control.check() {
        Ok(()) => run(inv),
        Err(error) => formal_live::Response::failure(command_fault(inv, error)),
    })
}
// Stable ExitCode represents u8 statuses. Keep wider statuses intact for the
// existing process::exit fallback rather than silently truncating them.
fn normal_exit_code(code: i32) -> std::result::Result<std::process::ExitCode, i32> {
    u8::try_from(code).map(std::process::ExitCode::from).map_err(|_| code)
}
fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args
        .first()
        .is_some_and(|value| value == "--source-native-session-server")
    {
        let result = lycheedev_source::native_session::server(&args);
        if let Err(error) = &result {
            let _ = writeln!(std::io::stderr(), "{error}");
        }
        std::process::exit(result.err().map_or(0, |error| error.exit_code));
    }
    if args.first().is_some_and(|value| value == "--source-native-session-worker") {
        let result = lycheedev_source::native_session::analysis_worker(&args);
        if let Err(error) = &result { let _ = writeln!(std::io::stderr(), "{error}"); }
        std::process::exit(result.err().map_or(0, |error| error.exit_code));
    }
    if args.first().is_some_and(|value| value == "--source-native-batch-worker") {
        let result = if args.len() == 1 { lycheedev_source::batch::worker_stdio() }
            else { Err(ToolkitError::arguments("private Source batch worker accepts stdin only")) };
        if let Err(error) = &result { let _ = writeln!(std::io::stderr(), "{error}"); }
        std::process::exit(result.err().map_or(0, |error| error.exit_code));
    }
    if args
        .first()
        .is_some_and(|value| value == "--source-native-worker")
    {
        let result = if args.len() == 1 {
            lycheedev_source::native_research::worker_stdio()
        } else {
            Err(ToolkitError::arguments(
                "private Source native worker accepts stdin only",
            ))
        };
        if let Err(error) = &result {
            let _ = writeln!(std::io::stderr(), "{error}");
        }
        std::process::exit(result.err().map_or(0, |error| error.exit_code));
    }
    if args
        .first()
        .is_some_and(|value| value == "--source-flow-worker")
    {
        let result = if args.len() == 1 {
            lycheedev_source::flow::public::worker_stdio()
        } else {
            Err(ToolkitError::arguments(
                "private Source flow worker accepts stdin only",
            ))
        };
        if let Err(error) = &result {
            let _ = writeln!(std::io::stderr(), "{error}");
        }
        std::process::exit(result.err().map_or(0, |error| error.exit_code));
    }
    if args
        .first()
        .is_some_and(|value| value == "--source-broker-server")
    {
        let result = broker_server(&args);
        if let Err(error) = &result {
            let _ = writeln!(std::io::stderr(), "{error}");
        }
        std::process::exit(result.err().map_or(0, |error| error.exit_code));
    }
    if args
        .first()
        .is_some_and(|value| value == "__lycheedev_update_apply")
    {
        let mut format = String::from("json");
        let result = (|| -> Result<Outcome> {
            if args.len() != 3 {
                return Err(ToolkitError::arguments(
                    "private update handoff requires exact file and nonce",
                ));
            }
            let path = PathBuf::from(&args[1]).canonicalize()?;
            format = lycheedev_delivery::npm_update::helper_format(&path)?;
            let console = lycheedev_windows::cancellation::ConsoleCancellation::install()
                .map_err(|error| ToolkitError::new("windows.console_unavailable", error, 3))?;
            let control = lycheedev_delivery::npm_update::helper_control(&path, console.token())?;
            lycheedev_delivery::npm_update::apply_handoff(&path, &args[2], &control)
        })();
        if let Err(error) = render(&mut std::io::stdout().lock(), &format, &result) {
            let _ = writeln!(std::io::stderr(), "output: {error}");
            std::process::exit(5);
        }
        std::process::exit(result.err().map(|error| error.exit_code).unwrap_or(0));
    }
    let mut parsed = parse(&args);
    let console = install_invocation_control(&mut parsed, |signal| {
        lycheedev_windows::cancellation::ConsoleCancellation::install_callback(move || signal.cancel())
    });
    let requested = requested_format(&args);
    let format = if valid_format(requested) { requested } else { "json" };
    let mut stdout = std::io::stdout().lock();
    if let Some(response) = parsed.as_ref().ok().and_then(|inv| {
        dispatch_formal_entry_with(inv, formal_live::dispatch)
    }) {
        if let Err(error) = formal_live::render(&mut stdout, format, &response) {
            let _ = writeln!(std::io::stderr(), "command.output_failed: {}", error.message);
            std::process::exit(5);
        }
        let exit = response.exit_code();
        drop(console);
        std::process::exit(exit);
    }
    let mut yielded = false;
    let result = parsed.as_ref().map_err(Clone::clone).and_then(|inv| {
        inv.control
            .check()
            .and_then(|_| {
                if inv.command == "data db2 stream" && !inv.flag_bool("--help") {
                    let mut stream = StreamWriter {
                        out: &mut stdout,
                        yielded: false,
                    };
                    let result = lycheedev_data::stream::StreamRequest::from_invocation(inv)
                        .and_then(|request| lycheedev_data::stream::run(request, &mut |event| stream.emit(event)))
                        .map(lycheedev_data::stream::wire::into_outcome);
                    yielded = stream.yielded;
                    result.map_err(|error| stream_fault(error, yielded))
                } else if inv.command == "data hotfix"
                    && matches!(inv.flag("--source"), Some("wago" | "raidbots"))
                    && inv.flag("--encoding") != Some("csv")
                    && !inv.flag_bool("--help")
                {
                    let result = lycheedev_data::hotfix::remote::render(inv, data_timeout_seconds(inv)?, &mut stdout);
                    yielded = result.is_ok();
                    if let Err(error) = &result && error.code == "command.output_failed" {
                        let _ = writeln!(std::io::stderr(), "command.output_failed: {}", error.message);
                        yielded = true;
                    }
                    result
                } else {
                    dispatch(inv)
                }
            })
            .map_err(|error| command_fault(inv, error))
    });
    if !yielded
        && let Err(error) = render_with_context(&mut stdout, format, &result, parsed.as_ref().ok())
    {
        let _ = writeln!(std::io::stderr(), "output: {error}");
        std::process::exit(5);
    }
    let exit = result.err().map(|e| e.exit_code).unwrap_or(0);
    drop(console);
    // The ordinary result and console registration are already retired. Return
    // through the CRT so its atexit callbacks (including LLVM profile writing)
    // run on Windows; do not change the early exits' local-drop behavior.
    normal_exit_code(exit).unwrap_or_else(|code| std::process::exit(code))
}

#[cfg(test)]
mod project_tests;

#[cfg(test)]
mod tests {
    #[test]
    fn normal_exit_code_returns_every_portable_status_unchanged() {
        for code in 0..=u8::MAX {
            assert_eq!(
                super::normal_exit_code(i32::from(code)),
                Ok(std::process::ExitCode::from(code)),
            );
        }
    }

    #[test]
    fn normal_exit_code_preserves_full_width_fallback_status() {
        for code in [i32::MIN, -256, -1, 256, 257, i32::MAX] {
            assert_eq!(super::normal_exit_code(code), Err(code));
        }
    }

    #[test]
    fn target_dispatch_preserves_invocation_cancellation_and_deadline() {
        use std::{sync::{Arc, atomic::AtomicBool}, time::{Duration, Instant}};
        let home = std::env::temp_dir().join(format!("lychee-cancelled-target-{}-{}",
            std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        assert!(!home.exists());
        for definition in ["a".repeat(40), "refs/heads/master".into()] {
            for (cancelled, expired, expected) in [(true, false, "command.cancelled"), (false, true, "command.deadline")] {
                let now = Instant::now();
                let control = lycheedev_core::control::OperationControl::new(
                    if expired { now - Duration::from_secs(1) } else { now + Duration::from_secs(30) },
                    Arc::new(AtomicBool::new(cancelled)),
                );
                let invocation = Invocation {
                    command: "target resolve".into(), positional: None,
                    flags: BTreeMap::from([
                        ("--installation".into(), home.join("absent-installation").to_string_lossy().into_owned()),
                        ("--product".into(), "retail".into()),
                        ("--region".into(), "cn".into()),
                        ("--locale".into(), "zhCN".into()),
                        ("--definitions".into(), definition.clone()),
                        ("--offline".into(), "true".into()),
                    ]),
                    home: home.clone(), cwd: PathBuf::from("."), control,
                };
                assert_eq!(dispatch(&invocation).unwrap_err().code, expected);
                assert!(!home.exists(), "cancelled symbolic resolution must not initialize metadata");
            }
        }
    }

    #[test]
    fn installation_error_context_survives_project_context_merge() {
        let selection = json!({"directory":"fixture", "candidates":[], "issues":[]});
        let mut error = ToolkitError::new("selection.installation_missing", "missing", 3)
            .with_details(json!({"__commandContext":{"installationSelection":selection,"requestedProduct":"titan"}}));
        merge_error_context(
            &mut error,
            json!({"project":"fixture-project", "snapshot":"PIN"}),
        );
        assert_eq!(
            error.details["__commandContext"],
            json!({"installationSelection":selection,"requestedProduct":"titan","project":"fixture-project","snapshot":"PIN"})
        );
        let envelope = envelope(&Err(error));
        assert_eq!(envelope["context"]["installationSelection"], selection);
        assert_eq!(envelope["context"]["requestedProduct"], "titan");
        assert!(envelope["error"].get("details").is_none());
    }
    use super::*;
    struct InvocationConsoleFixture {
        signal: lycheedev_core::control::TrackedCancellation,
        alive: std::sync::Arc<std::sync::atomic::AtomicBool>,
    }
    impl Drop for InvocationConsoleFixture {
        fn drop(&mut self) {
            self.alive.store(false, std::sync::atomic::Ordering::Release);
        }
    }
    #[test]
    fn normal_invocation_control_preserves_ordered_ancestry_and_guard_lifetime() {
        use lycheedev_core::control::StopCause;
        use std::{sync::{Arc, atomic::{AtomicBool, Ordering}}, time::{Duration, Instant}};
        let mut parsed = parse(&args(&["live", "execute", "--wait-seconds", "600"]));
        let original = parsed.as_ref().unwrap().control.clone();
        let alive = Arc::new(AtomicBool::new(true));
        let before = Instant::now();
        let guard = install_invocation_control(&mut parsed, |signal| {
            Ok(InvocationConsoleFixture { signal, alive: alive.clone() })
        }).unwrap();
        let after = Instant::now();
        let inv = parsed.as_ref().unwrap();
        assert!(!inv.control.same_scope(&original));
        assert!(inv.control.deadline() >= before + Duration::from_secs(660));
        assert!(inv.control.deadline() <= after + Duration::from_secs(660));
        assert_eq!(inv.control.stop_cause().unwrap(), None);
        let child = inv.control.ordered_child(inv.control.deadline()).unwrap();
        let child_signal = child.tracked_cancel_handle().unwrap();
        assert!(!inv.control.is_cancelled());
        assert!(!child.is_cancelled());
        assert!(alive.load(Ordering::Acquire));
        guard.signal.cancel();
        assert!(inv.control.is_cancelled());
        assert!(child.is_cancelled());
        assert!(!child_signal.is_cancelled());
        assert_eq!(inv.control.stop_cause().unwrap(), Some(StopCause::Cancelled));
        assert_eq!(child.stop_cause().unwrap(), Some(StopCause::Cancelled));
        let result = Err(inv.control.check().unwrap_err());
        let mut output = Vec::new();
        render_with_context(&mut output, "json", &result, Some(inv)).unwrap();
        assert!(alive.load(Ordering::Acquire), "guard must survive result rendering");
        drop(guard);
        assert!(!alive.load(Ordering::Acquire));
    }
    #[test]
    fn normal_invocation_control_keeps_signal_received_during_install() {
        use lycheedev_core::control::StopCause;
        let mut parsed = parse(&args(&["doctor"]));
        let guard = install_invocation_control(&mut parsed, |signal| {
            signal.cancel();
            Ok(signal)
        }).unwrap();
        let control = &parsed.as_ref().unwrap().control;
        assert!(guard.is_cancelled());
        assert_eq!(control.stop_cause().unwrap(), Some(StopCause::Cancelled));
        assert_eq!(control.check().unwrap_err().code, "command.cancelled");
    }
    #[test]
    fn normal_invocation_control_preserves_error_priority_and_releases_failed_guard() {
        use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
        // The public parser rejects an invalid wait value before registration.
        // Exercise the installer's defensive post-parse budget failure directly.
        let invalid_budget = || -> Result<Invocation> {
            let mut inv = parse(&args(&["live", "execute", "--wait-seconds", "1"])).unwrap();
            inv.flags.insert("--wait-seconds".into(), "-1".into());
            Ok(inv)
        };
        let mut parsed = invalid_budget();
        let expected = operation_seconds(parsed.as_ref().unwrap()).unwrap_err();
        let alive = Arc::new(AtomicBool::new(false));
        let mut installed = false;
        assert!(install_invocation_control(&mut parsed, |signal| {
            installed = true;
            alive.store(true, Ordering::Release);
            Ok(InvocationConsoleFixture { signal, alive: alive.clone() })
        }).is_none());
        assert!(installed);
        assert!(!alive.load(Ordering::Acquire));
        let error = parsed.unwrap_err();
        assert_eq!(error.code, expected.code);
        assert_eq!(error.message, expected.message);
        assert_eq!(error.exit_code, expected.exit_code);
        let mut parsed = invalid_budget();
        assert!(install_invocation_control::<()>(&mut parsed, |_| {
            Err("fixture.console_install_failed".into())
        }).is_none());
        let error = parsed.unwrap_err();
        assert_eq!(error.code, "windows.console_unavailable");
        assert_eq!(error.message, "fixture.console_install_failed");
        assert_eq!(error.exit_code, 3);
    }
    #[test]
    fn normal_invocation_control_skips_information_help_and_parse_errors() {
        for words in [vec![], vec!["version"], vec!["describe"], vec!["doctor", "--help"], vec!["not-a-command"], vec!["live", "execute", "--wait-seconds", "-1"]] {
            let mut parsed = parse(&args(&words));
            let before = parsed.as_ref().ok().map(|inv| inv.control.clone());
            assert!(install_invocation_control::<()>(&mut parsed, |_| {
                panic!("information/help/parse failure must not install a handler")
            }).is_none());
            if let Some(before) = before {
                assert!(parsed.as_ref().unwrap().control.same_scope(&before));
            } else {
                assert!(parsed.is_err());
            }
        }
    }
    #[test]
    fn installation_delivery_required_arguments_precede_identity_io() {
        for (words, message) in [
            (vec!["addon", "install", "--installation", "absent-fixture"], "addon install requires --installation and --release, or --resume with --output and no --release"),
            (vec!["addon", "install", "--installation", "absent-fixture", "--resume", "--release", "release", "--output", "archive"], "addon install requires --installation and --release, or --resume with --output and no --release"),
            (vec!["addon", "remove", "--installation", "absent-fixture"], "addon remove requires --installation and --output"),
            (vec!["skill", "install", "--path", "absent-fixture"], "skill install requires --path and --release, or --resume with --output and no --release"),
            (vec!["skill", "remove", "--path", "absent-fixture"], "skill remove requires --path and --output"),
            (vec!["addon", "status"], "installation status requires --path"),
            (vec!["skill", "status"], "installation status requires --path"),
        ] {
            let inv = parse(&args(&words)).unwrap();
            let error = dispatch(&inv).unwrap_err();
            assert_eq!((error.code.as_str(), error.exit_code, error.message.as_str()), ("command.invalid_arguments", 2, message));
            assert!(error.details.get("__commandContext").is_none());
        }
    }
    #[test]
    fn installation_addon_errors_keep_go_query_stage_and_discovery_context() {
        struct Fixture(std::path::PathBuf);
        impl Drop for Fixture { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); } }
        let fixture = Fixture(std::env::temp_dir().join(format!("lychee-addon-selection-envelope-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos())));
        std::fs::create_dir(&fixture.0).unwrap();
        for (scenario, code, exit) in [("missing", "selection.installation_missing", 3), ("conflict", "selection.installation_metadata_conflict", 4), ("ambiguous", "selection.installation_ambiguous", 2)] {
            let root = fixture.0.join(scenario);
            if scenario != "missing" {
                std::fs::create_dir(&root).unwrap();
                for name in if scenario == "ambiguous" { vec!["one", "two"] } else { vec!["one"] } {
                    let client = root.join(name);
                    std::fs::create_dir(&client).unwrap();
                    std::fs::write(client.join(".flavor.info"), if scenario == "conflict" { "wow_unknown" } else { "wow_classic_titan" }).unwrap();
                    std::fs::write(client.join("version.txt"), "3.80.2.70000").unwrap();
                }
            }
            for verb in ["status", "install", "remove"] {
                let mut arguments = args(&["addon", verb, "--installation", root.to_str().unwrap(), "--format", "json"]);
                if verb == "install" { arguments.extend(args(&["--release", fixture.0.join("release").to_str().unwrap()])); }
                if verb == "remove" { arguments.extend(args(&["--output", fixture.0.join("archive").to_str().unwrap()])); }
                let inv = parse(&arguments).unwrap();
                let error = command_fault(&inv, dispatch(&inv).unwrap_err());
                assert_eq!((error.code.as_str(), error.exit_code), (code, exit));
                let value = envelope(&Err(error));
                assert_eq!(value["error"]["stage"], "query");
                assert!(value["context"]["installationSelection"].is_object());
                assert_eq!(value["context"]["requestedProduct"], "");
                assert!(value["error"].get("details").is_none());
            }
        }
    }
    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }
    #[test]
    fn doctor_release_failure_keeps_worker_and_workspace_diagnostics() {
        let mut inv = parse(&args(&["doctor"])).unwrap();
        inv.home = std::env::temp_dir().join(format!(
            "lycheedev-doctor-absent-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut worker_ran = false;
        let providers = doctor_provider_checks_with(
            &inv,
            || {
                Err(ToolkitError::new(
                    "delivery.invalid_payload",
                    "digest differs: skill/SKILL.md",
                    4,
                ))
            },
            |_, _| {
                worker_ran = true;
                Ok(emmy_doctor_reply_fixture())
            },
        )
        .unwrap();
        assert!(worker_ran);
        assert_eq!(providers[0]["id"], "release.integrity");
        assert_eq!(providers[0]["ok"], false);
        assert_eq!(providers[0]["status"], "warn");
        assert_eq!(providers[0]["code"], "delivery.invalid_payload");
        assert_eq!(providers[1]["ok"], true);
        let result = lycheedev_core::doctor::inspect_with_checks(&inv, providers).unwrap_err();
        assert_eq!(result.code, "doctor.unhealthy");
        let checks = result.details["result"]["checks"].as_array().unwrap();
        assert!(
            checks
                .iter()
                .any(|check| check["id"] == "vault.workspace" && check["ok"] == false)
        );
        assert!(
            checks
                .iter()
                .any(|check| check["id"] == "release.integrity" && check["status"] == "warn")
        );
        assert!(
            checks
                .iter()
                .any(|check| check["id"] == "emmy.runtime" && check["ok"] == true)
        );
        assert!(!inv.home.exists());
    }
    #[test]
    fn doctor_release_inventory_checks_real_missing_and_modified_files() {
        struct Cleanup(PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let root = std::env::temp_dir().join(format!(
            "lycheedev-doctor-release-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _cleanup = Cleanup(root.clone());
        let inv = parse(&args(&["doctor"])).unwrap();
        let inspect =
            || lycheedev_delivery::release::inspect_with_control(&root, VERSION, &inv.control);
        assert_eq!(
            release_doctor_check_with(&inv, inspect).unwrap()["status"],
            "warn"
        );
        let mut resources = vec![];
        for (name, bytes) in [
            ("skill/SKILL.md", b"skill".as_slice()),
            ("addon/Lychee Dev.toc", b"addon".as_slice()),
        ] {
            let path = root.join("payload").join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, bytes).unwrap();
            resources.push(json!({"path":name,"sha256":lycheedev_core::storage::sha256(bytes),"bytes":bytes.len()}));
        }
        let manifest = json!({"schema":"lycheedev.release.v1","version":VERSION,"commit":"a".repeat(40),
            "binaries":{"windows-amd64":{"path":"native/windows-amd64/lycheedev.exe","bytes":1,"sha256":"b".repeat(64)}},"resources":resources});
        std::fs::write(
            root.join("release.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        let check = release_doctor_check_with(&inv, inspect).unwrap();
        assert_eq!(check["ok"], true);
        assert_eq!(check["scope"], "manifest-and-payload-inventory");
        assert_eq!(check["resourceCount"], 2);
        std::fs::write(root.join("payload/skill/SKILL.md"), b"other").unwrap();
        let check = release_doctor_check_with(&inv, inspect).unwrap();
        assert_eq!(check["ok"], false);
        assert_eq!(check["status"], "warn");
        assert_eq!(check["code"], "delivery.invalid_payload");
        std::fs::write(root.join("payload/skill/SKILL.md"), b"skill").unwrap();
        std::fs::remove_file(root.join("payload/addon/Lychee Dev.toc")).unwrap();
        assert_eq!(
            release_doctor_check_with(&inv, inspect).unwrap()["status"],
            "warn"
        );
    }
    #[test]
    fn doctor_release_does_not_downgrade_cancel_or_original_deadline() {
        let inv = parse(&args(&["doctor"])).unwrap();
        for error in [
            ToolkitError::new("command.deadline", "expired", 5),
            ToolkitError::new("process.cancelled", "cancelled", 7),
        ] {
            let expected = error.code.clone();
            assert_eq!(
                doctor_provider_checks_with(
                    &inv,
                    || Err(error),
                    |_, _| panic!("cancelled release check launched worker")
                )
                .unwrap_err()
                .code,
                expected
            );
        }
        let error = release_doctor_check_with(&inv, || {
            inv.control
                .cancel_handle()
                .store(true, std::sync::atomic::Ordering::Release);
            Err(ToolkitError::new(
                "delivery.invalid_payload",
                "interrupted read",
                4,
            ))
        })
        .unwrap_err();
        assert_eq!(error.code, "command.cancelled");
        let mut expired = parse(&args(&["doctor"])).unwrap();
        expired.control = lycheedev_core::control::OperationControl::new(
            std::time::Instant::now(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        assert_eq!(
            release_doctor_check_with(&expired, || panic!("expired release check started"))
                .unwrap_err()
                .code,
            "command.deadline"
        );
    }
    fn emmy_doctor_reply_fixture() -> Value {
        json!({
            "schema":"lycheedev.source-native-research.v1",
            "inputIdentity":"a".repeat(64),
            "analysis":{"engine":"native_common_facts","state":"complete","sourceFiles":1,"sourceExecution":false},
            "relations":[{"kind":"call","state":"resolved","target":EMMY_DOCTOR_SYMBOL,"path":EMMY_DOCTOR_FILE,"line":4}],
            "lifecycle":{"owner":"source_owned_process","mode":"one_shot","retired":true}
        })
    }
    #[test]
    fn emmy_doctor_validates_evidence_instead_of_a_success_exit_alone() {
        let valid = emmy_doctor_reply_fixture();
        validate_emmy_doctor_result(&valid).unwrap();
        for (pointer, replacement) in [
            ("/schema", json!("other")),
            ("/inputIdentity", json!("unverified")),
            ("/analysis/engine", json!("luals")),
            ("/analysis/state", json!("partial")),
            ("/analysis/sourceFiles", json!(2)),
            ("/analysis/sourceExecution", json!(true)),
            ("/relations/0/kind", json!("reference")),
            ("/relations/0/state", json!("static_candidate")),
            ("/relations/0/target", json!("Other")),
            ("/relations/0/path", json!("other.lua")),
            ("/relations/0/line", json!(3)),
            ("/lifecycle/owner", json!("unowned")),
            ("/lifecycle/mode", json!("persistent")),
            ("/lifecycle/retired", json!(false)),
        ] {
            let mut value = valid.clone();
            *value.pointer_mut(pointer).unwrap() = replacement;
            assert_eq!(
                validate_emmy_doctor_result(&value).unwrap_err().code,
                "source.emmy_doctor_mismatch",
                "{pointer}"
            );
        }
        for key in [
            "schema",
            "inputIdentity",
            "analysis",
            "relations",
            "lifecycle",
        ] {
            let mut value = valid.clone();
            value.as_object_mut().unwrap().remove(key);
            assert!(validate_emmy_doctor_result(&value).is_err(), "{key}");
        }
    }
    #[test]
    fn emmy_doctor_probe_is_bounded_and_distinguishes_unchecked_tools() {
        let mut inv = parse(&args(&["doctor"])).unwrap();
        inv.control = lycheedev_core::control::OperationControl::uncancelable(
            std::time::Instant::now() + std::time::Duration::from_secs(1),
        );
        let check = emmy_doctor_check_with(&inv, |probe, request| {
            assert_eq!(probe.control.deadline(), inv.control.deadline());
            assert!(!probe.control.same_scope(&inv.control));
            assert_eq!(probe.control.stop_cause().unwrap(), None);
            assert_eq!(inv.control.stop_cause().unwrap(), None);
            assert!(
                probe.control.remaining().unwrap()
                    <= std::time::Duration::from_secs(EMMY_DOCTOR_EXECUTION_SECONDS)
            );
            assert_eq!(request.snapshot.files.len(), 1);
            assert_eq!(request.snapshot.files[EMMY_DOCTOR_FILE], EMMY_DOCTOR_SOURCE);
            assert_eq!(request.snapshot.load_documents.len(), 1);
            assert_eq!(request.snapshot.load_plans[0].ordered, [EMMY_DOCTOR_FILE]);
            assert_eq!(request.query.direction, "incoming");
            assert_eq!(request.query.symbol.name, EMMY_DOCTOR_SYMBOL);
            assert_eq!(request.snapshot.luac.version, "5.1.5");
            Ok(emmy_doctor_reply_fixture())
        })
        .unwrap();
        assert_eq!(check["id"], "emmy.runtime");
        assert_eq!(check["ok"], true);
        assert_eq!(check["scope"], "embedded-worker-fixture");
        assert_eq!(
            check["notChecked"],
            json!([
                "persistent_session",
                "luac_runtime",
                "user_source_analysis"
            ])
        );
        assert!(
            check["detail"]
                .as_str()
                .unwrap()
                .contains("built-in call fixture")
        );
        let long = parse(&args(&["doctor"])).unwrap();
        emmy_doctor_check_with(&long, |probe, _| {
            assert!(probe.control.deadline() < long.control.deadline());
            assert!(probe.control.remaining().unwrap() <= std::time::Duration::from_secs(5));
            Ok(emmy_doctor_reply_fixture())
        })
        .unwrap();
    }
    #[test]
    fn emmy_doctor_failed_probe_warns_without_claiming_retirement() {
        let inv = parse(&args(&["doctor"])).unwrap();
        let check = emmy_doctor_check_with(&inv, |_, _| {
            Err(
                ToolkitError::new("source.native_worker_failed", "fixture worker failed", 5)
                    .with_details(json!({"reaped":false})),
            )
        })
        .unwrap();
        assert_eq!(check["ok"], false);
        assert_eq!(check["status"], "warn");
        assert_eq!(check["code"], "source.native_worker_failed");
        assert_eq!(check["details"]["reaped"], false);
        assert!(check.get("lifecycle").is_none());
        let check = emmy_doctor_check_with(&inv, |_, _| Ok(json!({}))).unwrap();
        assert_eq!(check["ok"], false);
        assert_eq!(check["code"], "source.emmy_doctor_mismatch");
        let check = emmy_doctor_check_with(&inv, |_, _| {
            Err(ToolkitError::new(
                "command.deadline",
                "probe-local budget exhausted",
                5,
            ))
        })
        .unwrap();
        assert_eq!(check["status"], "warn");
        assert_eq!(check["code"], "command.deadline");
        assert!(inv.control.check().is_ok());
    }
    #[test]
    fn emmy_doctor_preserves_caller_cancel_and_deadline() {
        use lycheedev_core::control::StopCause;
        let inv = parse(&args(&["doctor"])).unwrap();
        let signal = inv.control.tracked_cancel_handle().unwrap();
        let error = emmy_doctor_check_with(&inv, |probe, _| {
            signal.cancel();
            assert!(probe.control.is_cancelled());
            assert_eq!(probe.control.stop_cause().unwrap(), Some(StopCause::Cancelled));
            assert_eq!(inv.control.stop_cause().unwrap(), Some(StopCause::Cancelled));
            Ok(emmy_doctor_reply_fixture())
        })
        .unwrap_err();
        assert_eq!(error.code, "command.cancelled");
        let inv = parse(&args(&["doctor"])).unwrap();
        let error = emmy_doctor_check_with(&inv, |_, _| {
            Err(ToolkitError::new("process.cancelled", "cancelled", 7))
        })
        .unwrap_err();
        assert_eq!(error.exit_code, 7);
        let mut expired = inv;
        expired.control = lycheedev_core::control::OperationControl::new(
            std::time::Instant::now(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        assert_eq!(
            emmy_doctor_check_with(&expired, |_, _| panic!("expired doctor launched worker"))
                .unwrap_err()
                .code,
            "command.deadline"
        );
    }
    #[test]
    fn emmy_doctor_rejects_an_unordered_internal_control_before_worker_start() {
        // The normal CLI constructs a tracked control. This deliberately
        // violates the internal helper's invariant, not a public CLI mode.
        let inv = parse(&args(&["doctor"])).unwrap();
        let _raw = inv.control.cancel_handle();
        let error = emmy_doctor_check_with(&inv, |_, _| {
            panic!("an unordered internal control must not start the doctor worker")
        }).unwrap_err();
        assert_eq!(error.code, "control.cancellation_order_unavailable");
    }
    #[test]
    fn describe_includes_frozen_format_and_development_metadata() {
        let inv = parse(&args(&["describe", "--format", "json"])).unwrap();
        let outcome = dispatch(&inv).unwrap();
        assert_eq!(outcome.data["formats"], json!(["text", "json", "jsonl"]));
        assert_eq!(outcome.data["development"], VERSION.contains('-'));
        assert_eq!(
            outcome.data["commands"],
            serde_json::to_value(contract::implemented()).unwrap()
        );
    }
    #[test]
    fn help_lists_each_implemented_commands_own_flags() {
        for definition in contract::implemented() {
            let mut input: Vec<String> = definition.path.split_whitespace().map(str::to_owned).collect();
            input.push("--help".to_owned());
            let outcome = dispatch(&parse(&input).unwrap()).unwrap();
            let mut flags: Vec<String> = definition.flags.iter().map(|flag| flag.0.clone()).collect();
            flags.push("--help".to_owned());
            assert_eq!(outcome.data["flags"], json!(flags), "{}", definition.path);
            assert_eq!(outcome.data["commands"], json!([definition]));
        }
        let outcome = dispatch(&parse(&args(&["--help"])).unwrap()).unwrap();
        assert_eq!(
            outcome.data["flags"],
            json!(["--home <root>", "--format text|json|jsonl", "--help"])
        );
    }
    #[test]
    fn frozen_token_admission_errors_and_precedence() {
        for (input, expected) in [
            (vec!["version", "--home="], "empty value for --home"),
            (vec!["version", "--home", ""], "empty value for --home"),
            (vec!["source", "inspect", "--path="], "empty value for --path"),
            (vec!["version", "--home", "a", "--home", "b"], "repeated flag --home"),
            (vec!["version", "--home", "a", "--home"], "repeated flag --home"),
            (vec!["version", "--home", "a", "--home="], "repeated flag --home"),
            (vec!["version", "--format=json", "--format=text"], "repeated flag --format"),
            (vec!["live", "instances", "--passive", "--passive"], "repeated flag --passive"),
            (vec!["live", "instances", "--passive=false"], "unknown flag --passive"),
            (vec!["source", "inspect", "--path", "a", "--path", "b"], "repeated flag --path"),
            (vec!["version", "--path", "a"], "--path is not supported by version; use version --help"),
            (vec!["version", "--path="], "--path is not supported by version; use version --help"),
            (vec!["--path", "a"], "--path is not supported by the command; use the command --help"),
            (vec!["version", "--path", "a", "--output", "b"], "--path is not supported by version; use version --help"),
            (vec!["version", "--output", "b", "--path", "a"], "--output is not supported by version; use version --help"),
            (vec!["version", "--home=", "--unknown"], "unknown flag --unknown"),
            (vec!["version", "--unknown", "--format=invalid"], "format must be text, json, or jsonl"),
            (vec!["version", "--home=", "--format="], "format must be text, json, or jsonl"),
            (vec!["version", "--format=json", "--format=invalid"], "format must be text, json, or jsonl"),
            (vec!["version", "--format=invalid", "--format=json"], "repeated flag --format"),
            (vec!["evidence", "show", "--home="], "missing required argument <capture-id>"),
        ] {
            let error = parse(&args(&input)).unwrap_err();
            assert_eq!(error.message, expected, "{input:?}");
            assert_eq!(error.exit_code, 2, "{input:?}");
        }
        // Help aliases are intentionally repeatable in the fixed Go parser.
        assert!(parse(&args(&["version", "--help", "-h", "--help"])).is_ok());
    }
    #[test]
    fn output_encoding_prescan_uses_last_occurrence_even_for_errors() {
        for (input, expected) in [
            (vec!["--unknown", "--format", "jsonl"], "jsonl"),
            (vec!["--format=json", "--format=text"], "text"),
            (vec!["--format=jsonl", "--format"], "jsonl"),
            (vec!["--format=json", "--format="], ""),
            (vec!["--format", "--help"], "--help"),
            (vec!["version"], "text"),
        ] {
            assert_eq!(requested_format(&args(&input)), expected);
        }
    }
    #[test]
    fn passive_instances_public_parser_and_report_do_not_enter_target_selection() {
        use lycheedev_live::{host::{Services, Target}, lifecycle::{Session, Transport}, managed::ManagedInstallation, passive::Report};
        struct PassiveOnly;
        impl Services for PassiveOnly {
            fn instances(&self, _: &Invocation) -> Result<Report> {
                Ok(Report { installations: vec![], candidates: None })
            }
            fn targets(&self, _: &Invocation) -> Result<Vec<Target>> { panic!("passive request entered target selection") }
            fn open(&self, _: &Target, _: Option<&Session>) -> Result<Box<dyn Transport>> { panic!("passive request opened transport") }
            fn managed(&self) -> &dyn ManagedInstallation { panic!("passive request opened installation") }
        }
        for command in [args(&["live", "instances"]), args(&["--passive", "live", "instances"])] {
            let inv = parse(&command).unwrap();
            let outcome = lycheedev_live::host::dispatch_with(&inv, &PassiveOnly);
            assert_eq!(envelope(&outcome), json!({"schema":"lycheedev.result.v1","ok":true,"operationId":"","context":{},"result":{"installations":[],"candidates":null},"captures":[],"warnings":[],"error":null}));
            let error = command_fault(&inv, ToolkitError::new("command.deadline", "expired", 5));
            assert_eq!((error.code.as_str(), error.exit_code), ("live.deadline_pending", 6));
        }
        for command in [
            args(&["live", "instances", "--passive=false"]),
            args(&["live", "instances", "--passive", "--passive"]),
            args(&["live", "instances", "--pid", "12"]),
            args(&["live", "instances", "--project", "P-123"]),
            args(&["live", "connect", "--passive"]),
        ] {
            assert_eq!(parse(&command).unwrap_err().exit_code, 2);
        }
    }
    #[test]
    fn asset_capture_commit_context_preserves_committed_and_unknown_facts() {
        for command in ["inspect", "export"] {
            let inv = parse(&args(&["asset", command])).unwrap();
            for state in ["committed", "unknown"] {
                let id = format!("CAP-{}", "b".repeat(64));
                let completed = if state == "committed" { json!([id]) } else { json!([]) };
                let primary = ToolkitError::new("records.metadata_limit", "budget stopped after commit attempt", 3)
                    .with_details(json!({"causes":[{"code":"external.io","message":"checked close failed"}]}));
                let details = json!({"schema":"lycheedev.capture-commit-error.v1", "primary":primary,
                    "capturePublication":{"state":state,"id":id}, "completedCaptures":completed});
                let message = format!("capture publication {state}; do not replay {id}");
                let actual = command_fault(&inv, ToolkitError::new("records.metadata_limit", &message, 3).with_details(details.clone()));
                assert_eq!(actual.message, message);
                assert_eq!(actual.details["stage"], "query");
                let result = Err(actual);
                let public = envelope(&result);
                assert_eq!(public["error"]["details"], details);
                assert_eq!(public["captures"], json!([]));
                assert!(public.get("capturePublication").is_none());
                let mut bytes = Vec::new();
                render(&mut bytes, "jsonl", &result).unwrap();
                let frames: Vec<Value> = std::str::from_utf8(&bytes).unwrap().lines()
                    .map(|line| serde_json::from_str(line).unwrap()).collect();
                assert_eq!(frames, vec![json!({"type":"begin","schema":"lycheedev.result.v1"}),
                    json!({"type":"error","envelope":public})]);
            }
        }
    }
    #[test]
    fn asset_capture_commit_context_survives_typed_owner_cleanup() {
        use lycheedev_core::metadata::historical_write::finish_historical;
        let id = format!("CAP-{}", "d".repeat(64));
        let primary = ToolkitError::new("records.metadata_limit", "budget exhausted", 3);
        let committed = primary.clone().with_details(json!({
            "schema":"lycheedev.capture-commit-error.v1", "primary":primary,
            "capturePublication":{"state":"committed","id":id}, "completedCaptures":[id]
        }));
        let cleanup = ToolkitError::new("external.io", "directory close failed", 5);
        // This is the same typed join used by Session/Scope cleanup. Repeated
        // enclosing owners retain every cause and the original committed ID.
        let joined = finish_historical::<()>(Err(committed), Err(cleanup.clone())).unwrap_err();
        let joined = finish_historical::<()>(Err(joined), Err(cleanup.clone())).unwrap_err();
        let admitted = joined.clone().with_details(json!({
            "schema":"lycheedev.admitted-workspace-errors.v1", "causes":[joined,cleanup]
        }));
        for error in [joined, admitted] {
            assert!(capture_commit_context(&error));
            for command in ["inspect", "export"] {
                let inv = parse(&args(&["asset", command])).unwrap();
                let before = error.details.clone();
                let actual = command_fault(&inv, error.clone());
                assert_eq!(envelope(&Err(actual))["error"]["details"], before);
            }
            let mut malformed = error.clone();
            malformed.code = "external.io".into();
            assert!(!capture_commit_context(&malformed));
            let arbitrary = primary.clone().with_details(json!({"payload":error}));
            assert!(!capture_commit_context(&arbitrary));
        }
    }
    #[test]
    fn asset_publication_csv_committed_errors_preserve_all_public_details_for_sql_and_hotfix() {
        for command in ["sql", "hotfix"] {
            let inv = parse(&args(&["data", command])).unwrap();
            for count in 1..=2 {
                for state in ["not_published", "published", "unknown"] {
                    let primary = ToolkitError::new("records.metadata_limit", "stopped before checked close", 3);
                    let publication = primary.clone().with_details(json!({
                        "schema":"lycheedev.asset-publication-error.v1", "primary":primary,
                        "publication":{"state":state,"path":"C:/out/table.csv"}
                    }));
                    let ids: Vec<_> = (0..count).map(|i| format!("CAP-{}", format!("{i:x}").repeat(64))).collect();
                    let message = format!("CSV captures already committed: {}", ids.join(", "));
                    let details = json!({"schema":"lycheedev.csv-committed-error.v1",
                        "primary":publication,"completedCaptures":ids});
                    let error = ToolkitError::new("records.metadata_limit", &message, 3).with_details(details.clone());
                    let actual = command_fault(&inv, error);
                    let public = envelope(&Err(actual));
                    assert_eq!(public["error"]["code"], "records.metadata_limit");
                    assert_eq!(public["error"]["message"], message);
                    assert_eq!(public["error"]["details"], details);
                    assert_eq!(public["captures"], json!([]));
                    assert!(public.get("completedCaptures").is_none());
                }
            }
        }
    }
    #[test]
    fn asset_output_publication_and_demux_facts_survive_typed_cleanup() {
        use lycheedev_core::metadata::historical_write::finish_historical;
        let primary = ToolkitError::new("records.metadata_limit", "allocation stopped", 3);
        let cleanup = ToolkitError::new("external.io", "checked close failed", 5);
        for state in ["not_published", "published", "unknown"] {
            let publication = primary.clone().with_details(json!({
                "schema":"lycheedev.asset-publication-error.v1", "primary":primary,
                "publication":{"state":state,"path":"C:/out/帧.webp"}
            }));
            let joined = finish_historical::<()>(Err(publication.clone()), Err(cleanup.clone())).unwrap_err();
            let ids: Vec<_> = (0..5).map(|i| format!("CAP-{}", format!("{i:x}").repeat(64))).collect();
            let demux = joined.clone().with_details(json!({
                "schema":"lycheedev.demux-committed-error.v1", "primary":joined,
                "completedCaptures":ids,"publishedOutputs":["C:/out/frame-1.webp"]
            }));
            for error in [publication, joined, demux] {
                assert!(capture_commit_context(&error));
                for command in ["inspect", "export", "demux"] {
                    let inv = parse(&args(&["asset", command])).unwrap();
                    let details = error.details.clone();
                    let message = error.message.clone();
                    let actual = command_fault(&inv, error.clone());
                    assert_eq!(actual.message, message);
                    let public = envelope(&Err(actual));
                    assert_eq!(public["error"]["details"], details);
                    assert_eq!(public["captures"], json!([]));
                    assert!(public.get("publishedOutputs").is_none());
                }
            }
        }
    }
    #[test]
    fn asset_output_publication_rejects_malformed_facts() {
        let primary = ToolkitError::new("records.metadata_limit", "limit", 3);
        let inv = parse(&args(&["asset", "export"])).unwrap();
        let valid = json!({"schema":"lycheedev.asset-publication-error.v1", "primary":primary,
            "publication":{"state":"unknown","path":"out.webp"}});
        for (pointer, replacement) in [
            ("/schema", json!("unrecognized")), ("/primary", json!(null)),
            ("/primary/code", json!("external.io")), ("/primary/exit_code", json!(5)),
            ("/publication/state", json!("maybe")), ("/publication/path", json!("")),
        ] {
            let mut details = valid.clone();
            *details.pointer_mut(pointer).unwrap() = replacement;
            let error = primary.clone().with_details(details);
            assert!(!capture_commit_context(&error), "{pointer}");
            assert_eq!(command_fault(&inv, error).details, json!({"stage":"query"}));
        }
        let demux = json!({"schema":"lycheedev.demux-committed-error.v1", "primary":primary,
            "completedCaptures":[format!("CAP-{}", "a".repeat(64))], "publishedOutputs":[]});
        assert!(asset_publication_context(&primary.clone().with_details(demux.clone())));
        for (pointer, replacement) in [
            ("/completedCaptures", json!([])),
            ("/completedCaptures", json!([format!("CAP-{}", "A".repeat(64))])),
            ("/publishedOutputs", json!([""])), ("/publishedOutputs", json!(null)),
        ] {
            let mut details = demux.clone();
            *details.pointer_mut(pointer).unwrap() = replacement;
            assert!(!capture_commit_context(&primary.clone().with_details(details)), "{pointer}");
        }
    }
    #[test]
    fn asset_capture_commit_context_rejects_inconsistent_typed_facts() {
        let inv = parse(&args(&["asset", "export"])).unwrap();
        let id = format!("CAP-{}", "b".repeat(64));
        let primary = ToolkitError::new("records.metadata_limit", "limit", 3);
        let valid = json!({"schema":"lycheedev.capture-commit-error.v1", "primary":primary,
            "capturePublication":{"state":"committed","id":id}, "completedCaptures":[id]});
        for (pointer, replacement) in [
            ("/schema", json!("other")), ("/primary", json!(null)),
            ("/primary/code", json!("external.io")), ("/primary/exit_code", json!(5)),
            ("/capturePublication/state", json!("unknown")),
            ("/capturePublication/state", json!("uncommitted")),
            ("/capturePublication/id", json!("CAP-short")),
            ("/capturePublication/id", json!(format!("CAP-{}", "B".repeat(64)))),
            ("/completedCaptures", json!([])),
            ("/completedCaptures", json!([format!("CAP-{}", "c".repeat(64))])),
            ("/completedCaptures", json!([id, id])),
        ] {
            let mut details = valid.clone();
            *details.pointer_mut(pointer).unwrap() = replacement;
            let actual = command_fault(&inv, primary.clone().with_details(details));
            assert_eq!(actual.message, "records.metadata_limit", "{pointer}");
            assert_eq!(actual.details, json!({"stage":"query"}), "{pointer}");
        }
    }
    #[test]
    fn late_asset_limit_preserves_committed_ids_and_typed_cleanup_in_public_message() {
        let inv = parse(&args(&["asset", "export"])).unwrap();
        for count in 1..=3 {
            let ids: Vec<_> = (0..count).map(|i| format!("CAP-{}", char::from(b'a' + i).to_string().repeat(64))).collect();
            let primary = ToolkitError::new("records.metadata_limit", "limit after source capture", 3)
                .with_details(json!({"schema":"lycheedev.admitted-workspace-errors.v1", "causes":[
                    {"code":"records.metadata_limit","message":"limit after source capture","exit_code":3,"details":null},
                    {"code":"external.io","message":"checked close failed","exit_code":5,"details":{"owner":"fixture"}}
                ]}));
            let message = format!("{}\nasset captures already committed: {}", primary.message, ids.join(", "));
            let error = ToolkitError::new(primary.code.clone(), message.clone(), primary.exit_code)
                .with_details(json!({"schema":"lycheedev.asset-committed-error.v1", "primary":primary,
                    "completedCaptures":ids, "outputPublished":count == 3}));
            let before = error.details.clone();
            let actual = command_fault(&inv, error);
            assert_eq!(actual.message, message);
            assert_eq!(actual.details["primary"], before["primary"]);
            assert_eq!(actual.details["completedCaptures"], before["completedCaptures"]);
            assert_eq!(actual.details["outputPublished"], before["outputPublished"]);
            assert_eq!(actual.details["stage"], "query");
            let public = envelope(&Err(actual));
            assert_eq!(public["error"]["message"], message);
            assert_eq!(public["error"]["code"], "records.metadata_limit");
            // Fixed Go323 Fault already has optional details. Preserve the
            // existing JSON envelope and its typed completed-work information.
            assert_eq!(public["error"]["details"], before);
            assert!(public.get("completedCaptures").is_none());
            assert_eq!(public["captures"], json!([]));
        }
    }
    #[test]
    fn malformed_asset_marker_does_not_replace_the_pure_go_projection() {
        let inv = parse(&args(&["asset", "export"])).unwrap();
        let primary = ToolkitError::new("records.metadata_limit", "limit", 3);
        let valid = json!({"schema":"lycheedev.asset-committed-error.v1","primary":primary,
            "completedCaptures":[format!("CAP-{}", "a".repeat(64))],"outputPublished":false});
        for (field, value) in [
            ("schema", json!("other")), ("primary", json!(null)),
            ("completedCaptures", json!([])), ("completedCaptures", json!(["CAP-short"])),
            ("completedCaptures", json!([format!("CAP-{}", "A".repeat(64))])),
            ("outputPublished", json!("true"))
        ] {
            let mut details = valid.clone(); details[field] = value;
            let actual = command_fault(&inv, primary.clone().with_details(details));
            assert_eq!(actual.message, "records.metadata_limit");
            assert_eq!(actual.details, json!({"stage":"query"}));
        }
    }
    #[test]
    fn asset_content_limit_keeps_the_observed_go_error_envelope() {
        for command in ["inspect", "export"] {
            let inv = parse(&args(&["asset", command])).unwrap();
            let error = command_fault(
                &inv,
                ToolkitError::new(
                    "records.metadata_limit",
                    "Selected encoded content exceeds byte budget",
                    3,
                ),
            );
            assert_eq!(error.exit_code, 3);
            assert_eq!(
                envelope(&Err(error)),
                json!({
                    "schema":"lycheedev.result.v1","ok":false,"operationId":"",
                    "context":{},"result":null,"captures":[],"warnings":[],
                    "error":{"code":"records.metadata_limit","message":"records.metadata_limit",
                        "stage":"query","retryable":false,"resumeOperationId":""}
                })
            );
        }
    }
    #[test]
    fn catalog_depth_parse_order_and_public_fault_stages_match_go() {
        for (tail, expected) in [
            (
                vec!["--max-depth", "-1", "--journal-encounter-id", "bad"],
                "invalid --max-depth",
            ),
            (
                vec!["--journal-encounter-id", "bad", "--max-depth", "-1"],
                "invalid --journal-encounter-id",
            ),
            (
                vec!["--max-depth", "9223372036854775808"],
                "invalid --max-depth",
            ),
        ] {
            let mut input = args(&["data", "encounter", "get"]);
            input.extend(args(&tail));
            let error = parse(&input).unwrap_err();
            assert_eq!(error.message, expected);
            assert_eq!(envelope(&Err(error))["context"], json!({}));
        }
        let inv = parse(&args(&["data", "encounter", "get", "--max-depth", "+1"])).unwrap();
        for (internal, message, exit, public, stage) in [
            (
                "records.relation_cycle",
                "records.relation_cycle",
                4,
                "records.relation_cycle",
                "query",
            ),
            (
                "records.query_limit",
                "records.query_limit",
                2,
                "records.query_limit",
                "query",
            ),
            (
                "query.budget_exceeded",
                "relational.budget_exceeded",
                3,
                "query.budget_exceeded",
                "query",
            ),
            (
                "schema.definition_not_found",
                "schema.definition_not_found",
                3,
                "schema.definition_not_found",
                "query",
            ),
            (
                "table.invalid_format",
                "table.invalid_format",
                5,
                "command.failed",
                "dispatch",
            ),
            (
                "records.content_missing",
                "records.content_not_found",
                5,
                "command.failed",
                "dispatch",
            ),
            (
                "selection.project_unlocked",
                "selection.project_unlocked",
                3,
                "selection.project_unlocked",
                "selection",
            ),
        ] {
            let error = command_fault(&inv, ToolkitError::new(internal, message, exit));
            assert_eq!(
                (error.code.as_str(), error.message.as_str(), error.exit_code),
                (public, message, exit)
            );
            assert_eq!(error.details["stage"], stage);
        }
    }

    #[test]
    fn public_evidence_verify_returns_the_exact_capture_after_checking_its_blob() {
        use lycheedev_core::{evidence, workspace};
        let home = std::env::temp_dir().join(format!(
            "lycheedev-cli-verify-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        assert!(!home.exists());
        workspace::initialize(&home).unwrap();
        let capture = evidence::commit_capture(
            &home,
            b"123",
            "application/json",
            json!({"kind":"fixture","locator":"cli:verify"}),
            true,
            false,
        )
        .unwrap();
        let inv = parse(&args(&[
            "evidence",
            "verify",
            capture["id"].as_str().unwrap(),
            "--home",
            home.to_str().unwrap(),
        ]))
        .unwrap();
        let internal = evidence::dispatch(&inv).unwrap();
        assert_eq!(internal.data, json!({"capture":capture,"verified":true}));
        let result = dispatch(&inv);
        let expected = json!({"schema":"lycheedev.result.v1","ok":true,"operationId":"","context":{},"result":capture,"captures":[],"warnings":[],"error":null});
        assert_eq!(envelope(&result), expected);
        for format in ["json", "jsonl"] {
            let mut raw = Vec::new();
            render_with_context(&mut raw, format, &result, Some(&inv)).unwrap();
            let frames = String::from_utf8(raw)
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str::<Value>(line).unwrap())
                .collect::<Vec<_>>();
            if format == "json" {
                assert_eq!(frames, vec![expected.clone()]);
            } else {
                assert_eq!(
                    frames,
                    vec![
                        json!({"type":"begin","schema":"lycheedev.result.v1"}),
                        json!({"type":"record","result":capture}),
                        json!({"type":"end","envelope":expected})
                    ]
                );
            }
        }
        // Replacing the blob with equally sized bytes must still fail before
        // the presentation projection can return a successful capture.
        std::fs::write(
            evidence::blob_path(&home, capture["blob"]["sha256"].as_str().unwrap()).unwrap(),
            b"456",
        )
        .unwrap();
        assert!(dispatch(&inv).is_err());
        std::fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn public_parser_flag_value_messages_match_the_frozen_contract() {
        for flag in ["--format", "--home"] {
            let error = parse(&args(&["version", flag])).unwrap_err();
            assert_eq!(error.message, format!("missing value for {flag}"));
            assert_eq!(error.exit_code, 2);
        }
        let error = parse(&args(&["version", "--format", "not-a-format"])).unwrap_err();
        assert_eq!(error.message, "format must be text, json, or jsonl");
        assert_eq!(error.exit_code, 2);
    }
    #[test]
    fn unknown_flags_precede_empty_values_but_route_flags_follow_argument_order() {
        for (path, flag) in [
            ("data spell auras", "--spell-id"),
            ("data item get", "--item-id"),
            ("data spell summons", "--npc-id"),
            ("data creature display", "--display-id"),
            ("data encounter get", "--journal-encounter-id"),
        ] {
            let mut input: Vec<String> = path.split_whitespace().map(str::to_owned).collect();
            input.extend([format!("{flag}="), "--__rust_parity_unknown_flag".into()]);
            let error = parse(&input).unwrap_err();
            assert_eq!(error.message, "unknown flag --__rust_parity_unknown_flag");
            assert_eq!(error.exit_code, 2);
            let mut raw = Vec::new();
            render_with_context(&mut raw, "json", &Err(error), None).unwrap();
            let envelope: Value = serde_json::from_slice(&raw).unwrap();
            assert_eq!(envelope["context"], json!({}));
            assert_eq!(envelope["error"]["stage"], "dispatch");
            input.pop();
            assert_eq!(
                parse(&input).unwrap_err().message,
                format!("empty value for {flag}")
            );
            input.extend(["--output".into(), "unused".into()]);
            assert_eq!(
                parse(&input).unwrap_err().message,
                format!("empty value for {flag}")
            );
        }
    }
    #[test]
    fn bounded_data_envelopes_preserve_timeout_after_parse_on_success_and_failure() {
        let inv = parse(&args(&["data", "spell", "auras", "--spell-id", "100"])).unwrap();
        let mut outcome = Outcome::new(json!({"spellID":100}));
        outcome.selection = json!({"schema":"lycheedev.selection.v1","id":"PIN-kept"});
        let mut raw = vec![];
        render_with_context(&mut raw, "json", &Ok(outcome), Some(&inv)).unwrap();
        let value: Value = serde_json::from_slice(&raw).unwrap();
        assert_eq!(
            value["context"],
            json!({"snapshot":"PIN-kept","timeoutSeconds":300})
        );

        let missing = parse(&args(&["data", "spell", "auras"])).unwrap();
        raw.clear();
        render_with_context(&mut raw, "json", &dispatch(&missing), Some(&missing)).unwrap();
        let value: Value = serde_json::from_slice(&raw).unwrap();
        assert_eq!(value["context"], json!({"timeoutSeconds":300}));
        assert_eq!(
            value["error"]["message"],
            "data spell auras requires --spell-id"
        );

        let explicit = parse(&args(&["data", "sql", "--timeout-seconds", "1"])).unwrap();
        raw.clear();
        render_with_context(
            &mut raw,
            "jsonl",
            &Err(ToolkitError::new("command.deadline", "deadline", 5)
                .with_details(json!({"__commandContext":{"snapshot":"PIN-kept"}}))),
            Some(&explicit),
        )
        .unwrap();
        let frames: Vec<Value> = String::from_utf8(raw)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(frames.last().unwrap()["type"], "error");
        assert_eq!(
            frames.last().unwrap()["envelope"]["context"],
            json!({"snapshot":"PIN-kept","timeoutSeconds":1})
        );
    }
    #[test]
    fn parse_failures_help_and_nonquery_commands_do_not_gain_data_timeout_context() {
        for value in ["0", "-1", "+1", "4294967296", "bad"] {
            let parsed = parse(&args(&["data", "spell", "auras", "--spell-id", value]));
            assert!(parsed.is_err());
            let mut raw = vec![];
            render_with_context(&mut raw, "json", &Err(parsed.unwrap_err()), None).unwrap();
            let value: Value = serde_json::from_slice(&raw).unwrap();
            assert_eq!(value["context"], json!({}));
        }
        assert!(parse(&args(&["data", "spell", "summons", "--npc-id", "0"])).is_ok());
        assert!(
            parse(&args(&[
                "data",
                "spell",
                "summons",
                "--npc-id",
                "4294967296"
            ]))
            .is_err()
        );
        for command in [
            vec!["data", "spell", "auras", "--help"],
            vec!["data", "hotfix"],
            vec!["asset", "inspect"],
            vec!["source", "list"],
        ] {
            let inv = parse(&args(&command)).unwrap();
            let mut raw = vec![];
            render_with_context(&mut raw, "json", &Ok(Outcome::new(json!({}))), Some(&inv))
                .unwrap();
            let value: Value = serde_json::from_slice(&raw).unwrap();
            assert_eq!(value["context"], json!({}));
        }
    }
    #[test]
    fn repeated_scopes_are_retained_only_on_their_contract() {
        let inv = parse(&args(&[
            "update",
            "--path",
            "a",
            "--path",
            "b",
            "--installation",
            "c",
            "--installation",
            "d",
        ]))
        .unwrap();
        assert_eq!(inv.flag_values("--path"), ["a", "b"]);
        assert_eq!(inv.flag_values("--installation"), ["c", "d"]);
        assert!(parse(&args(&["source", "inspect", "--path", "a", "--path", "b"])).is_err());
        assert!(parse(&args(&["version", "--format", "json", "--format", "text"])).is_err());
    }
    #[test]
    fn longest_path_and_required_argument_admission_are_stable() {
        assert_eq!(
            parse(&args(&["data", "db2", "schema", "Spell"]))
                .unwrap()
                .command,
            "data db2 schema"
        );
        assert_eq!(
            parse(&args(&["evidence", "show"])).err().unwrap().exit_code,
            2
        );
        assert!(parse(&args(&["evidence", "show", "--help"])).is_ok());
        assert!(parse(&args(&["version", "extra"])).is_err());
    }
    #[test]
    fn jsonl_errors_never_claim_end_and_preserve_usable_result() {
        let result=Err(ToolkitError::new("live.pending","pending settlement",6).with_details(json!({"operationId":"CON-exact","result":{"report":{"state":"verified"},"complete":false},"captures":[{"id":"CAP-kept"}]})));
        let mut out = vec![];
        render(&mut out, "jsonl", &result).unwrap();
        let frames: Vec<Value> = String::from_utf8(out)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0]["type"], "begin");
        assert_eq!(frames[1]["type"], "error");
        assert_eq!(
            frames[1]["envelope"]["result"]["report"]["state"],
            "verified"
        );
        assert_eq!(frames[1]["envelope"]["operationId"], "CON-exact");
    }
    #[test]
    fn jsonl_complete_transport_retains_partial_business_coverage() {
        let mut out = vec![];
        let mut outcome =
            Outcome::new(json!({"complete":false,"truncated":true,"coverage":{"complete":true}}));
        outcome.complete = false;
        render(&mut out, "jsonl", &Ok(outcome)).unwrap();
        let frames: Vec<Value> = String::from_utf8(out)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(frames.len(), 3);
        assert_eq!(frames[2]["type"], "end");
        assert_eq!(frames[2]["envelope"]["result"]["complete"], false);
    }
    #[test]
    fn public_json_numbers_follow_explicit_layout_in_all_success_formats() {
        use lycheedev_core::public_json::NumberLayout;
        let mut outcome = Outcome::new(serde_json::from_str(r#"{"f":[-0.0,1.0,1e21],"raw":9007199254740993.125,"wide":18446744073709551615,"text":"<>&\u2028\u2029"}"#).unwrap());
        outcome.public_numbers = NumberLayout::field("f", NumberLayout::array(NumberLayout::Binary64));
        let expected = r#""f":[-0,1,1e+21]"#;
        for format in ["json", "jsonl", "text"] {
            let mut out = Vec::new();
            render(&mut out, format, &Ok(outcome.clone())).unwrap();
            let text = String::from_utf8(out).unwrap();
            assert!(text.ends_with('\n'));
            assert!(text.contains("9007199254740993.125"));
            assert!(text.contains("18446744073709551615"));
            assert!(text.contains(r#"<>&\u2028\u2029"#));
            if format == "text" {
                assert!(text.contains("    -0,\n    1,\n    1e+21"));
            } else {
                assert_eq!(text.matches(expected).count(), if format == "jsonl" { 2 } else { 1 });
                let frames: Vec<Value> = text.lines().map(|line| serde_json::from_str(line).unwrap()).collect();
                assert_eq!(frames.len(), if format == "jsonl" { 3 } else { 1 });
                if format == "jsonl" {
                    assert_eq!(frames.iter().map(|frame| frame["type"].as_str().unwrap()).collect::<Vec<_>>(), ["begin", "record", "end"]);
                }
            }
            assert!(!text.contains("public_numbers"));
        }
    }
    #[test]
    fn public_error_and_typed_stream_preserve_unknown_numeric_tokens() {
        let raw: Value = serde_json::from_str(r#"{"raw":9007199254740993.125,"huge":1e400,"zero":-0.0,"text":"<>&\u2028"}"#).unwrap();
        let result = Err(ToolkitError::new("live.pending", "pending", 6)
            .with_details(json!({"result":raw,"operationId":"CON-exact"})));
        let mut out = Vec::new();
        render(&mut out, "jsonl", &result).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert_eq!(text.lines().count(), 2);
        assert!(!text.contains(r#""type":"end""#));
        assert!(text.contains("9007199254740993.125"));
        assert!(text.contains("1e+400"));
        assert!(text.contains(r#""zero":-0.0"#));
        assert!(text.contains(r#"<>&\u2028"#));
        let mut out = Vec::new();
        let mut writer = StreamWriter { out: &mut out, yielded: false };
        writer.emit_value(&json!({"type":"record","record":raw})).unwrap();
        assert!(writer.yielded);
        let text = String::from_utf8(out).unwrap();
        assert_eq!(text.lines().count(), 1);
        assert!(text.contains("9007199254740993.125"));
        assert!(text.contains(r#""zero":-0.0"#));
        assert!(text.contains(r#"<>&\u2028"#));
    }
    #[test]
    fn public_jsonl_encoding_failure_never_writes_record_or_success_end() {
        use lycheedev_core::public_json::NumberLayout;
        let mut outcome = Outcome::new(json!({"f":null}));
        outcome.public_numbers = NumberLayout::field("f", NumberLayout::Binary64);
        let mut out = Vec::new();
        assert!(render(&mut out, "jsonl", &Ok(outcome)).is_err());
        let frames: Vec<Value> = String::from_utf8(out).unwrap().lines()
            .map(|line| serde_json::from_str(line).unwrap()).collect();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0]["type"], "begin");
    }
    struct Closed;
    impl Write for Closed {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "closed consumer",
            ))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    #[test]
    fn closed_output_returns_io_fault_instead_of_panicking() {
        assert_eq!(
            render(&mut Closed, "jsonl", &Ok(Outcome::new(json!({}))))
                .err()
                .unwrap()
                .exit_code,
            5
        );
    }
    #[test]
    fn default_context_keeps_legacy_identity_without_duplicate_selection() {
        let mut outcome = Outcome::new(json!({"coverage":{"complete":true}}));
        outcome.selection = json!({"schema":"lycheedev.selection.v1","id":"PIN-exact","source":{"exactCommit":"fixed"}});
        outcome.coverage = json!({"complete":true});
        let env = envelope(&Ok(outcome));
        assert_eq!(env["context"], json!({"snapshot":"PIN-exact"}));
        assert_eq!(env["result"]["coverage"]["complete"], true);
    }
    #[test]
    fn local_target_default_offline_ref_and_named_resolution_preserve_observed_identity() {
        use lycheedev_core::{metadata::Metadata, selection, storage, workspace};
        let root = std::env::temp_dir().join(format!(
            "lycheedev-cli-target-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        struct Cleanup(PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(root.clone());
        let selected = root.join("renamed-client");
        let home = root.join("workspace");
        std::fs::create_dir_all(&selected).unwrap();
        std::fs::write(
            selected.join(".flavor.info"),
            "Product Flavor!STRING:0\nwow\n",
        )
        .unwrap();
        std::fs::write(selected.join("version.txt"), "12.1.0.69933").unwrap();
        let key = "6854a2d8622b8d8a65fd8ce724f32334";
        let config_path = root.join("Data/config/68/54");
        std::fs::create_dir_all(&config_path).unwrap();
        std::fs::write(config_path.join(key), b"root = 01234567890123456789012345678901\nencoding = 01234567890123456789012345678901 abcdef01234567890123456789012345\nencoding-size = 20 30\n").unwrap();
        std::fs::write(root.join(".build.info"), format!("Product!STRING:0|Version!STRING:0|Build Key!HEX:16|CDN Key!HEX:16|Active!DEC:1\nwow|12.1.0.69933|{key}|{key}|1\n")).unwrap();
        workspace::initialize(&home).unwrap();
        let mut direct = parse(&args(&[
            "target",
            "resolve",
            "--installation",
            selected.to_str().unwrap(),
            "--region",
            "cn",
            "--locale",
            "zhCN",
            "--offline",
            "--home",
            home.to_str().unwrap(),
        ]))
        .unwrap();
        assert_eq!(
            dispatch(&direct).unwrap_err().code,
            "records.definition_unavailable_offline"
        );
        let commit = "a".repeat(40);
        Metadata::open(&home, true)
            .unwrap()
            .put(
                &format!("definition-ref/{}", storage::sha256(b"refs/heads/master")),
                &json!({"reference":"refs/heads/master","commit":commit}),
                None,
            )
            .unwrap();
        let outcome = dispatch(&direct).unwrap();
        assert_eq!(outcome.data["data"]["product"], "retail");
        assert_eq!(outcome.data["data"]["region"], "cn");
        assert_eq!(outcome.data["data"]["fullBuild"], "12.1.0.69933");
        assert_eq!(outcome.data["data"]["definitionCommit"], commit);
        assert_eq!(outcome.provenance.len(), 1);
        let id = outcome.data["id"].as_str().unwrap();
        assert!(selection::load_pinned(&home, id).is_ok());
        direct.flags.insert("--product".into(), "classic".into());
        assert_eq!(
            dispatch(&direct).unwrap_err().code,
            "records.client_identity"
        );
        let add = parse(&args(&[
            "target",
            "add",
            "fixture",
            "--installation",
            selected.to_str().unwrap(),
            "--product",
            "retail",
            "--region",
            "cn",
            "--locale",
            "zhCN",
            "--home",
            home.to_str().unwrap(),
        ]))
        .unwrap();
        dispatch(&add).unwrap();
        let named = parse(&args(&[
            "target",
            "resolve",
            "--target",
            "fixture",
            "--offline",
            "--home",
            home.to_str().unwrap(),
        ]))
        .unwrap();
        let resolved = dispatch(&named).unwrap();
        assert_eq!(resolved.data["pin"]["id"], id);
        assert_eq!(resolved.data["config"]["name"], "fixture");
        let metadata = Metadata::open(&home, false).unwrap();
        assert!(
            metadata
                .read::<Value>(&format!("target-ref/fixture/pin/{id}"))
                .is_ok()
        );
        assert!(!selected.join("LycheeDev").exists());
    }
    #[test]
    fn normal_invocation_control_public_operation_budget_honors_entire_query_and_explicit_live_wait_limits() {
        use std::time::{Duration, Instant};
        let query = parse(&args(&[
            "data",
            "sql",
            "--sql",
            "select 1",
            "--timeout-seconds",
            "3600",
        ]))
        .unwrap();
        assert_eq!(operation_seconds(&query).unwrap(), 3600);
        // Fixed Go 3.2.2 dispatch_source.go gives all three research routes
        // one 90-second total budget, including static mode and preparation.
        for words in [
            vec!["source", "refs", "--snapshot", "fixture", "--symbol", "Target"],
            vec!["source", "context", "--snapshot", "fixture", "--symbol", "Target"],
            vec!["source", "refs", "--snapshot", "fixture", "--symbol", "Target", "--static-only"],
            vec!["source", "batch", "--snapshot", "fixture", "--file", "fixture.json"],
        ] {
            let source = parse(&args(&words)).unwrap();
            assert_eq!(operation_seconds(&source).unwrap(), 90, "{:?}", words);
            let mut parsed = Ok(source);
            let before = Instant::now();
            assert!(install_invocation_control(&mut parsed, |_| Ok(())).is_some());
            let deadline = parsed.as_ref().unwrap().control.deadline();
            assert!(deadline >= before + Duration::from_secs(90));
            assert!(deadline <= Instant::now() + Duration::from_secs(90));
        }
        let live = parse(&args(&["live", "execute", "--wait-seconds", "600"])).unwrap();
        assert_eq!(operation_seconds(&live).unwrap(), 660);
        let invalid = parse(&args(&["live", "execute", "--wait-seconds", "-1"])).unwrap_err();
        assert_eq!(invalid.exit_code, 2);
        assert_eq!(invalid.code, "command.invalid_arguments");
        assert_eq!(invalid.message, "--wait-seconds must be between 1 and 600");
        // Internal callers still receive the same rejection if they bypass argv.
        let mut invalid = live.clone();
        invalid.flags.insert("--wait-seconds".into(), "-1".into());
        assert_eq!(operation_seconds(&invalid).unwrap_err().exit_code, 2);
        let deadline = ToolkitError::new("command.deadline", "exhausted", 5);
        let pending = command_fault(&live, deadline.clone());
        assert_eq!(pending.exit_code, 6);
        assert_eq!(pending.code, "live.deadline_pending");
        assert_eq!(command_fault(&query, deadline).exit_code, 5);
        assert_eq!(
            command_fault(&live, ToolkitError::new("command.cancelled", "caller", 7)).exit_code,
            7
        );
    }
    #[test]
    fn typed_stream_has_no_generic_envelope_and_maps_only_yielded_deadlines_to_seven() {
        let mut out = vec![];
        let mut writer = StreamWriter {
            out: &mut out,
            yielded: false,
        };
        writer
            .emit_value(&json!({"frame":"begin","begin":{"table":"Map"}}))
            .unwrap();
        writer
            .emit_value(&json!({"frame":"record","record":{"ID":1,"Float":-230.989}}))
            .unwrap();
        assert!(writer.yielded);
        let text = String::from_utf8(out).unwrap();
        assert!(!text.contains("lycheedev.result.v1"));
        assert!(!text.contains("\"end\""));
        let deadline = ToolkitError::new("command.deadline", "whole query expired", 5);
        assert_eq!(stream_fault(deadline.clone(), false).exit_code, 5);
        assert_eq!(stream_fault(deadline, true).exit_code, 7);
        let mut closed = Closed;
        let mut writer = StreamWriter {
            out: &mut closed,
            yielded: false,
        };
        assert_eq!(
            writer
                .emit_value(&json!({"frame":"begin"}))
                .unwrap_err()
                .exit_code,
            5
        );
        assert!(writer.yielded);
    }
    #[test]
    fn native_stream_writer_uses_shared_wire_and_flushes_each_typed_event() {
        use lycheedev_data::stream::{StreamEvent, StreamRecord, WalkSummary};
        #[derive(Default)]
        struct Output { bytes: Vec<u8>, flushes: usize }
        impl Write for Output {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.bytes.extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                self.flushes += 1;
                Ok(())
            }
        }
        let row = BTreeMap::from([
            ("ID".into(), lycheedev_data::table::Value::UInt(u64::MAX)),
            ("Float".into(), lycheedev_data::table::Value::Float(-0.0)),
        ]);
        let mut out = Output::default();
        let mut writer = StreamWriter { out: &mut out, yielded: false };
        writer.emit(StreamEvent::Record(StreamRecord { index: 0, record_id: 7, row: &row })).unwrap();
        writer.emit(StreamEvent::End(WalkSummary { partial: false, count: 1, truncated: false })).unwrap();
        assert!(writer.yielded);
        assert_eq!(out.flushes, 2);
        let text = String::from_utf8(out.bytes).unwrap();
        assert!(text.contains("18446744073709551615"));
        assert!(text.contains(r#""Float":-0.0"#));
        assert!(!text.contains("lycheedev.result.v1"));
        let frames: Vec<Value> = text.lines().map(|line| serde_json::from_str(line).unwrap()).collect();
        assert_eq!(frames[0]["frame"], "record");
        assert_eq!(frames[1]["frame"], "end");
    }
    #[test]
    fn native_stream_writer_marks_yielded_before_write_or_flush_failure() {
        use lycheedev_data::stream::{StreamEvent, WalkSummary};
        struct FlushFailure { writes: usize }
        impl Write for FlushFailure {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.writes += 1;
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Err(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "fixture flush failed"))
            }
        }
        let mut out = FlushFailure { writes: 0 };
        let mut writer = StreamWriter { out: &mut out, yielded: false };
        assert_eq!(writer.emit(StreamEvent::End(WalkSummary { partial: false, count: 0, truncated: false })).unwrap_err().exit_code, 5);
        assert!(writer.yielded);
        assert!(out.writes > 0);
        let mut out = Closed;
        let mut writer = StreamWriter { out: &mut out, yielded: false };
        assert_eq!(writer.emit(StreamEvent::Error { code: "test", message: "test" }).unwrap_err().exit_code, 5);
        assert!(writer.yielded);
    }
    #[test]
    fn query_stage_control_is_projected_once_and_business_evidence_is_kept() {
        let fault = ToolkitError::new("relational.field_unknown", "missing field", 2)
            .with_details(json!({"stage":"query"}));
        let value = envelope(&Err(fault));
        assert_eq!(value["error"]["stage"], "query");
        assert!(value["error"].get("details").is_none());
        let fault = ToolkitError::new("source.partial", "usable evidence", 3).with_details(
            json!({"stage":"query","result":{"verified":true},"captures":[{"id":"CAP-kept"}]}),
        );
        let value = envelope(&Err(fault));
        assert_eq!(value["result"]["verified"], true);
        assert_eq!(value["captures"][0]["id"], "CAP-kept");
        assert_eq!(value["error"]["details"]["result"]["verified"], true);
        assert!(value["error"]["details"].get("stage").is_none());
    }
    #[test]
    fn private_source_broker_startup_rejects_unknown_duplicate_and_missing_identity_arguments() {
        let valid = args(&[
            "--source-broker-server",
            "--home",
            "D:/fixture/home",
            "--release",
            "D:/fixture/release",
            "--parent-pid",
            "123",
            "--parent-created",
            "456",
            "--start-token",
            "0123456789abcdef0123456789abcdef",
        ]);
        assert_eq!(broker_options(&valid).unwrap().len(), 5);
        let mut unknown = valid.clone();
        unknown[3] = "--public-test-flag".into();
        assert_eq!(broker_options(&unknown).unwrap_err().exit_code, 2);
        let mut duplicate = valid.clone();
        duplicate[3] = "--home".into();
        assert_eq!(broker_options(&duplicate).unwrap_err().exit_code, 2);
        assert_eq!(broker_options(&valid[..9]).unwrap_err().exit_code, 2);
        let mut extra = valid;
        extra.push("--help".into());
        assert_eq!(broker_options(&extra).unwrap_err().exit_code, 2);
    }
    #[test]
    fn structured_query_location_is_projected_once_without_losing_business_details() {
        let location = json!({"offset":25,"line":1,"column":26});
        let error = ToolkitError::new(
            "query.invalid_syntax",
            "relational.invalid_syntax at 1:26: expected ALL",
            2,
        )
        .with_details(json!({"stage":"query","location":location}));
        let value = envelope(&Err(error));
        assert_eq!(value["error"]["location"], location);
        assert!(value["error"].get("details").is_none());
        let error = ToolkitError::new("query.invalid_syntax", "invalid", 2)
            .with_details(json!({"location":location,"evidence":"kept"}));
        assert_eq!(
            envelope(&Err(error))["error"]["details"]["evidence"],
            "kept"
        );
    }
}
#[cfg(test)]
mod doctor_cache_tests;

#[cfg(test)]
mod init_tests;
