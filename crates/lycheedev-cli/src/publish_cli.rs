//! Parse publishing argv into typed domain operations before opening sessions.
use crate::Invocation;
use lycheedev_core::{Outcome, Result, ToolkitError};
use lycheedev_publish::curseforge::{Config, Curseforge, Upload};
use std::{
    fs::File,
    io::{IsTerminal, Read},
    path::PathBuf,
};

const MAX_INPUT: u64 = 32 * 1024 * 1024;
mod blackbox_cli;
mod dd_cli;
mod modus_cli;
mod newbee_cli;
mod newbee_official_cli;
enum Request {
    Modus(modus_cli::Request),
    Blackbox(blackbox_cli::Request),
    Dd(dd_cli::Request),
    Official(newbee_official_cli::Request),
    Newbee(newbee_cli::Request),
    Doctor,
    Projects {
        author_id: Option<u64>,
    },
    GameVersions,
    Upload {
        request: Upload,
        plan: bool,
    },
    UploadStatus {
        project: u64,
        operation: String,
    },
    VerifyUpload {
        project: u64,
        operation: String,
        file: u64,
    },
}
fn positive(inv: &Invocation, flag: &str) -> Result<u64> {
    inv.required(flag)?
        .parse::<u64>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| ToolkitError::arguments(format!("{flag} requires a positive integer")))
}
fn read_input(inv: &Invocation) -> Result<Vec<u8>> {
    inv.control.check()?;
    let input = inv.required("--input")?;
    let mut bytes = Vec::new();
    let mut reader: Box<dyn Read> = if input == "-" {
        if std::io::stdin().is_terminal() {
            return Err(ToolkitError::arguments(
                "--input - requires piped JSON through EOF",
            ));
        }
        Box::new(std::io::stdin())
    } else {
        let path = PathBuf::from(input);
        Box::new(File::open(if path.is_absolute() {
            path
        } else {
            inv.cwd.join(path)
        })?)
    };
    let mut chunk = [0u8; 65536];
    loop {
        inv.control.check()?;
        let n = reader.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        if (bytes.len() as u64).saturating_add(n as u64) > MAX_INPUT {
            return Err(ToolkitError::invalid("publishing input exceeds 32 MiB"));
        }
        bytes.extend_from_slice(&chunk[..n]);
    }
    inv.control.check()?;
    if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
        bytes.drain(..3);
    }
    Ok(bytes)
}
fn admit(inv: &Invocation) -> Result<Request> {
    if inv.command.starts_with("publish modus ") {
        return modus_cli::admit(inv).map(Request::Modus);
    }
    if inv.command.starts_with("publish blackbox ") {
        return blackbox_cli::admit(inv).map(Request::Blackbox);
    }
    if inv.command.starts_with("publish dd ") {
        return dd_cli::admit(inv).map(Request::Dd);
    }
    if inv.command.starts_with("publish newbee official ") {
        return newbee_official_cli::admit(inv).map(Request::Official);
    }
    if inv.command.starts_with("publish newbee ") {
        return newbee_cli::admit(inv).map(Request::Newbee);
    }
    match inv.command.as_str() {
        "publish curseforge session doctor" => Ok(Request::Doctor),
        "publish curseforge project list" => Ok(Request::Projects {
            author_id: inv
                .flag("--author-id")
                .map(|_| positive(inv, "--author-id"))
                .transpose()?,
        }),
        "publish curseforge plugin game-versions" => Ok(Request::GameVersions),
        "publish curseforge plugin upload" => Ok(Request::Upload {
            request: Upload::parse(&read_input(inv)?, &inv.cwd)?,
            plan: inv.flag_bool("--plan"),
        }),
        "publish curseforge plugin upload-status" => Ok(Request::UploadStatus {
            project: positive(inv, "--project-id")?,
            operation: inv.required("--operation")?.to_owned(),
        }),
        "publish curseforge plugin upload-verify" => Ok(Request::VerifyUpload {
            project: positive(inv, "--project-id")?,
            operation: inv.required("--operation")?.to_owned(),
            file: positive(inv, "--file-id")?,
        }),
        _ => Err(ToolkitError::arguments("unknown publishing command")),
    }
}
pub(crate) fn dispatch(inv: &Invocation) -> Result<Outcome> {
    let request = admit(inv)?;
    inv.control.check()?;
    let result = match request {
        Request::Modus(request) => modus_cli::dispatch(inv, request)?,
        Request::Blackbox(request) => blackbox_cli::dispatch(inv, request)?,
        Request::Dd(request) => dd_cli::dispatch(inv, request)?,
        Request::Official(request) => newbee_official_cli::dispatch(inv, request)?,
        Request::Newbee(request) => newbee_cli::dispatch(inv, request)?,
        Request::Doctor => Config::load(&inv.home)?.doctor(),
        Request::Upload {
            request,
            plan: true,
        } => request.plan(&inv.control)?,
        Request::UploadStatus { project, operation } => {
            Curseforge::upload_status(&inv.home, project, &operation, &inv.control)?
        }
        request => {
            let provider = Curseforge::open(&inv.home, &inv.control)?;
            match request {
                Request::Projects { author_id } => provider.projects(author_id)?,
                Request::GameVersions => provider.game_versions()?,
                Request::Upload {
                    request,
                    plan: false,
                } => provider.upload(&request)?,
                Request::VerifyUpload {
                    project,
                    operation,
                    file,
                } => provider.verify_upload(project, &operation, file)?,
                _ => unreachable!("local publishing requests handled before opening HTTP"),
            }
        }
    };
    Ok(Outcome::new(result))
}
pub(crate) fn file_input(path: &str) -> Option<serde_json::Value> {
    blackbox_cli::file_input(path)
        .or_else(|| modus_cli::file_input(path))
        .or_else(|| dd_cli::file_input(path))
        .or_else(|| newbee_official_cli::file_input(path))
        .or_else(|| newbee_cli::file_input(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn invocation(args: &[&str]) -> Invocation {
        crate::parse(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>()).unwrap()
    }
    #[test]
    fn help_and_schema_publish_full_nested_input_and_remote_effects_without_a_session() {
        let schema = crate::dispatch(&invocation(&[
            "schema",
            "publish",
            "curseforge",
            "plugin",
            "upload",
        ]))
        .unwrap()
        .data;
        let help = crate::dispatch(&invocation(&[
            "publish",
            "curseforge",
            "plugin",
            "upload",
            "--help",
        ]))
        .unwrap()
        .data;
        assert_eq!(schema["fileInput"], help["fileInput"]);
        assert_eq!(
            schema["fileInput"]["schema"]["properties"]["relations"]["properties"]["projects"]["items"]
                ["additionalProperties"],
            false
        );
        assert_eq!(
            schema["fileInput"]["schema"]["properties"]["game_versions"]["items"]["minimum"],
            1
        );
        assert_eq!(
            schema["effects"]["remoteWriteWhen"],
            json!({"--plan":false})
        );
        assert_eq!(schema["effects"]["gameProcess"], false);
        assert_eq!(schema["effects"]["gameInput"], false);
        assert_eq!(schema["input"]["requiredFlags"], json!(["--input"]));
        assert!(
            !schema["fileInput"]["schema"]["properties"]
                .as_object()
                .unwrap()
                .contains_key("upload_token")
        );
        assert_eq!(
            crate::operation_seconds(&invocation(&[
                "publish",
                "curseforge",
                "plugin",
                "upload",
                "--input",
                "doc.json"
            ]))
            .unwrap(),
            600
        );
        assert_eq!(
            crate::operation_seconds(&invocation(&["publish", "curseforge", "project", "list"]))
                .unwrap(),
            60
        );
        assert_eq!(
            crate::operation_seconds(&invocation(&[
                "data", "agent", "serve", "--format", "jsonl"
            ]))
            .unwrap(),
            300
        );
    }
    #[test]
    fn publishing_argument_errors_do_not_echo_rejected_secret_options() {
        let args = [
            "publish",
            "curseforge",
            "plugin",
            "upload",
            "--upload-token=private-token",
        ]
        .map(str::to_owned);
        let error = crate::parse(&args).unwrap_err();
        assert_eq!(error.exit_code, 2);
        let wire = serde_json::to_string(&error).unwrap();
        assert!(!wire.contains("private-token"));
        assert!(!wire.contains("upload-token"));
        assert!(wire.contains("--input"));
    }
    #[test]
    fn typed_admission_checks_required_ids_and_input_before_opening_a_provider() {
        for args in [
            vec!["publish", "curseforge", "plugin", "upload"],
            vec![
                "publish",
                "curseforge",
                "project",
                "list",
                "--author-id",
                "0",
            ],
            vec![
                "publish",
                "curseforge",
                "plugin",
                "upload-verify",
                "--project-id",
                "719",
                "--operation",
                "OPERATION",
            ],
            vec![
                "publish",
                "curseforge",
                "plugin",
                "upload-status",
                "--project-id",
                "0",
                "--operation",
                "OPERATION",
            ],
        ] {
            let inv = invocation(&args);
            assert_eq!(admit(&inv).err().unwrap().exit_code, 2);
        }
    }
}
