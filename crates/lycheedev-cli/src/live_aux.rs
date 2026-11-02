//! CLI admission for passive discovery and the local probe registry.
use crate::Invocation;
use lycheedev_core::{Outcome, Result, ToolkitError};
use lycheedev_live::request::{InstancesRequest, LiveContext, LiveRequest, ProbeRequest};
use std::path::PathBuf;

pub(crate) fn context(inv: &Invocation) -> LiveContext {
    LiveContext {
        home: inv.home.clone(),
        control: inv.control.clone(),
    }
}

fn input_path(inv: &Invocation, value: &str) -> PathBuf {
    let path = PathBuf::from(value);
    if path.is_absolute() {
        path
    } else {
        inv.cwd.join(path)
    }
}

pub(crate) fn admit(inv: &Invocation) -> Result<LiveRequest> {
    inv.control.check()?;
    let request = match inv.command.as_str() {
        "live instances" => LiveRequest::Instances(InstancesRequest {
            installation: inv
                .flag("--installation")
                .filter(|path| !path.is_empty())
                .map(|path| input_path(inv, path)),
        }),
        "live probe put" => LiveRequest::Probe(ProbeRequest::Put {
            name: inv.required("--name")?.into(),
            file: input_path(inv, inv.required("--file")?),
        }),
        "live probe list" => LiveRequest::Probe(ProbeRequest::List {
            limit: inv.usize_flag("--limit", 100, 1, 1000)?,
            include_removed: inv.flag_bool("--include-removed"),
        }),
        "live probe show" => LiveRequest::Probe(ProbeRequest::Show {
            selector: inv.argument()?.into(),
        }),
        "live probe remove" => LiveRequest::Probe(ProbeRequest::Remove {
            name: inv.argument()?.into(),
        }),
        command
            if crate::contract::all().iter().any(|definition| {
                definition.path == command && definition.path.starts_with("live ")
            }) =>
        {
            return Err(ToolkitError::new(
                "live.services_required",
                "native command requires an injected managed installation and verified Windows services",
                3,
            ));
        }
        _ => return Err(ToolkitError::arguments("unknown Live command")),
    };
    Ok(request)
}

pub(crate) fn dispatch(inv: &Invocation) -> Result<Outcome> {
    let request = admit(inv)?;
    lycheedev_live::dispatch(&context(inv), &request)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    #[test]
    fn public_probe_put_flags_admit_without_reading_the_named_file() {
        let home = std::env::temp_dir().join(format!("cli-admission-unused-{}-{}",
            std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let words = ["live", "probe", "put", "--name", "audit", "--file", "not-created.lua"].map(str::to_owned);
        let mut inv = crate::parse(&words).unwrap();
        inv.home = home.clone();
        let LiveRequest::Probe(ProbeRequest::Put { name, file }) = admit(&inv).unwrap() else { panic!("probe put"); };
        assert_eq!(name, "audit");
        assert_eq!(file, inv.cwd.join("not-created.lua"));
        for missing in ["--name", "--file"] {
            let mut incomplete = words.to_vec();
            let index = incomplete.iter().position(|word| word == missing).unwrap();
            incomplete.drain(index..index + 2);
            let mut inv = crate::parse(&incomplete).unwrap();
            inv.home = home.clone();
            let error = admit(&inv).unwrap_err();
            assert_eq!(error.exit_code, 2);
            assert!(error.message.contains(missing));
        }
        assert!(!home.exists());
    }

    fn invocation(command: &str) -> Invocation {
        Invocation {
            command: command.into(),
            positional: None,
            flags: BTreeMap::new(),
            home: PathBuf::from("unused-live-home"),
            cwd: PathBuf::from("unused-live-cwd"),
            control: Default::default(),
        }
    }

    #[test]
    fn auxiliary_admission_preserves_defaults_paths_and_selectors_without_opening_inputs() {
        let mut inv = invocation("live instances");
        assert!(matches!(
            admit(&inv).unwrap(),
            LiveRequest::Instances(InstancesRequest { installation: None })
        ));
        inv.flags
            .insert("--installation".into(), "game root".into());
        let LiveRequest::Instances(request) = admit(&inv).unwrap() else {
            panic!("instances")
        };
        assert_eq!(request.installation, Some(inv.cwd.join("game root")));
        inv.command = "live probe list".into();
        assert!(matches!(
            admit(&inv).unwrap(),
            LiveRequest::Probe(ProbeRequest::List {
                limit: 100,
                include_removed: false
            })
        ));
        inv.flags.insert("--limit".into(), "1".into());
        inv.flags.insert("--include-removed".into(), "true".into());
        assert!(matches!(
            admit(&inv).unwrap(),
            LiveRequest::Probe(ProbeRequest::List {
                limit: 1,
                include_removed: true
            })
        ));
        inv.command = "live probe put".into();
        inv.flags.insert("--name".into(), "exact_probe".into());
        inv.flags
            .insert("--file".into(), "missing probe.lua".into());
        let LiveRequest::Probe(ProbeRequest::Put { name, file }) = admit(&inv).unwrap() else {
            panic!("put")
        };
        assert_eq!(name, "exact_probe");
        assert_eq!(file, inv.cwd.join("missing probe.lua"));
        inv.command = "live probe show".into();
        inv.positional = Some(format!("PRB-{}", "a".repeat(64)));
        let LiveRequest::Probe(ProbeRequest::Show { selector }) = admit(&inv).unwrap() else {
            panic!("show")
        };
        assert_eq!(selector, inv.positional.as_deref().unwrap());
        inv.command = "live probe remove".into();
        inv.positional = Some("exact_probe".into());
        let LiveRequest::Probe(ProbeRequest::Remove { name }) = admit(&inv).unwrap() else {
            panic!("remove")
        };
        assert_eq!(name, "exact_probe");
    }

    #[test]
    fn probe_list_cli_limit_keeps_integer_and_range_errors() {
        let mut inv = invocation("live probe list");
        for (value, message) in [
            ("bad", "--limit requires an integer"),
            ("0", "--limit must be in 1..=1000"),
            ("1001", "--limit must be in 1..=1000"),
        ] {
            inv.flags.insert("--limit".into(), value.into());
            let error = admit(&inv).unwrap_err();
            assert_eq!(error.code, "command.invalid_arguments");
            assert_eq!(error.exit_code, 2);
            assert_eq!(error.message, message);
        }
        for limit in [1, 1000] {
            inv.flags.insert("--limit".into(), limit.to_string());
            assert!(
                matches!(admit(&inv).unwrap(), LiveRequest::Probe(ProbeRequest::List { limit: actual, .. }) if actual == limit)
            );
        }
    }

    #[test]
    fn missing_probe_arguments_are_reported_at_cli_admission() {
        let mut inv = invocation("live probe put");
        assert_eq!(
            admit(&inv).unwrap_err().message,
            "live probe put requires --name"
        );
        inv.flags.insert("--name".into(), "probe".into());
        assert_eq!(
            admit(&inv).unwrap_err().message,
            "live probe put requires --file"
        );
        for command in ["live probe show", "live probe remove"] {
            inv.command = command.into();
            assert_eq!(
                admit(&inv).unwrap_err().message,
                format!("{command} requires an argument")
            );
        }
    }

    #[test]
    fn auxiliary_admission_keeps_cancellation_and_formal_service_route_distinct() {
        let inv = invocation("live probe put");
        inv.control.tracked_cancel_handle().unwrap().cancel();
        let error = admit(&inv).unwrap_err();
        assert_eq!(
            (error.code.as_str(), error.exit_code),
            ("command.cancelled", 7)
        );
        let error = admit(&invocation("live connect")).unwrap_err();
        assert_eq!(
            (error.code.as_str(), error.exit_code),
            ("live.services_required", 3)
        );
        assert_eq!(
            admit(&invocation("live unknown")).unwrap_err().code,
            "command.invalid_arguments"
        );
    }
}
