//! Explicit DD task-session selection. Discovery and argument admission never
//! acquire desktop cookies, saved credentials or native login capabilities.
use crate::Invocation;
use lycheedev_core::{Result, ToolkitError};
use lycheedev_publish::dd::{
    self, BackupCreate, BackupDelete, BackupGenerate, BackupRename, BackupSelection, BackupUpload,
    ConfigWrite, Delete, Page, PluginWrite, Read, Reference, Resource, WaWrite, WriteAction,
    task::{self, Client},
};
use serde_json::Value;
use std::num::NonZeroU64;

pub(super) enum Request {
    BackupScan(std::path::PathBuf),
    BackupGenerate {
        request: BackupGenerate,
        plan: bool,
    },
    BackupPublish {
        session: Option<String>,
        request: BackupPublish,
        plan: bool,
        recover_sn: Option<String>,
    },
    Doctor,
    Start,
    Status(Option<String>),
    Stop(String),
    LifeTypes,
    Channels(String),
    Write {
        session: Option<String>,
        request: AuthorWrite,
        recover_sn: Option<String>,
        plan: bool,
    },
    Delete {
        session: Option<String>,
        request: Delete,
        plan: bool,
    },
    BackupWrite {
        session: Option<String>,
        request: BackupWrite,
        plan: bool,
    },
    BackupPreview {
        session: String,
        selection: BackupSelection,
    },
    Read {
        session: String,
        request: Read,
    },
}
pub(super) enum BackupPublish {
    Upload(BackupUpload),
    Create(BackupCreate),
}
pub(super) enum BackupWrite {
    Rename(BackupRename),
    Delete(BackupDelete),
}
pub(super) enum AuthorWrite {
    Plugin(PluginWrite),
    Wa(WaWrite),
    Config(ConfigWrite),
}
fn session(inv: &Invocation) -> Result<Option<String>> {
    inv.flag("--session")
        .map(|id| {
            task::validate_session_id(id)?;
            Ok(id.to_owned())
        })
        .transpose()
}
pub(super) fn admit(inv: &Invocation) -> Result<Request> {
    if inv.command == "publish dd backup scan" {
        return Ok(Request::BackupScan(
            inv.cwd.join(inv.required("--game-dir")?),
        ));
    }
    if inv.command == "publish dd backup generate" {
        return Ok(Request::BackupGenerate {
            request: BackupGenerate::parse(&super::read_input(inv)?, &inv.cwd)?,
            plan: inv.flag_bool("--plan"),
        });
    }
    if matches!(
        inv.command.as_str(),
        "publish dd backup upload" | "publish dd backup create"
    ) {
        let session = session(inv)?;
        let plan = inv.flag_bool("--plan");
        if !plan && session.is_none() {
            return Err(ToolkitError::arguments(
                "--session is required for DD backup publishing; use --plan for local validation",
            ));
        }
        let recover_sn = inv
            .flag("--recover-sn")
            .map(|sn| Reference::parse(sn).map(|_| sn.to_owned()))
            .transpose()?;
        let bytes = super::read_input(inv)?;
        let request = if inv.command.ends_with(" upload") {
            BackupPublish::Upload(BackupUpload::parse(&bytes, &inv.cwd, &inv.control)?)
        } else {
            BackupPublish::Create(BackupCreate::parse(&bytes, &inv.cwd, &inv.control)?)
        };
        return Ok(Request::BackupPublish {
            session,
            request,
            plan,
            recover_sn,
        });
    }
    if matches!(
        inv.command.as_str(),
        "publish dd backup rename" | "publish dd backup delete"
    ) {
        let session = session(inv)?;
        let plan = inv.flag_bool("--plan");
        if !plan && session.is_none() {
            return Err(ToolkitError::arguments(
                "--session is required for DD backup writes; use --plan for a local preview",
            ));
        }
        let bytes = super::read_input(inv)?;
        let request = if inv.command.ends_with(" rename") {
            BackupWrite::Rename(BackupRename::parse(&bytes)?)
        } else {
            BackupWrite::Delete(BackupDelete::parse(&bytes)?)
        };
        return Ok(Request::BackupWrite {
            session,
            request,
            plan,
        });
    }
    if inv.command == "publish dd backup delete-preview" {
        let session = session(inv)?.ok_or_else(|| {
            ToolkitError::arguments("--session is required for DD backup reference checks")
        })?;
        let ids = inv
            .flag("--ids")
            .map(|s| {
                serde_json::from_str::<Vec<String>>(s).map_err(|_| {
                    ToolkitError::arguments(
                        "--ids requires a JSON array of exact string backup SNs",
                    )
                })
            })
            .transpose()?
            .unwrap_or_default();
        return Ok(Request::BackupPreview {
            session,
            selection: BackupSelection::new(
                ids,
                inv.flag("--name-contains").map(str::to_owned),
                inv.flag("--name-equals").map(str::to_owned),
                inv.flag("--created-before"),
                inv.flag("--created-after"),
            )?,
        });
    }
    if let Some((resource, action)) = write_operation(&inv.command) {
        let session = session(inv)?;
        let plan = inv.flag_bool("--plan");
        if !plan && session.is_none() {
            return Err(ToolkitError::arguments(
                "--session is required for DD publishing; use --plan for a local preview",
            ));
        }
        let recover_sn = inv
            .flag("--recover-sn")
            .map(|sn| -> Result<String> {
                Reference::parse(sn)?;
                Ok(sn.to_owned())
            })
            .transpose()?;
        let bytes = super::read_input(inv)?;
        let request = match resource {
            Resource::Plugin => {
                AuthorWrite::Plugin(PluginWrite::parse(action, &bytes, &inv.cwd, &inv.control)?)
            }
            Resource::Wa => {
                AuthorWrite::Wa(WaWrite::parse(action, &bytes, &inv.cwd, &inv.control)?)
            }
            Resource::Config => {
                AuthorWrite::Config(ConfigWrite::parse(action, &bytes, &inv.cwd, &inv.control)?)
            }
        };
        return Ok(Request::Write {
            session,
            request,
            recover_sn,
            plan,
        });
    }
    if let Some(resource) = delete_resource(&inv.command) {
        let session = session(inv)?;
        let plan = inv.flag_bool("--plan");
        if !plan && session.is_none() {
            return Err(ToolkitError::arguments(
                "--session is required for DD deletion; use --plan for an offline preview",
            ));
        }
        let request = Delete::parse(resource, &super::read_input(inv)?)?;
        return Ok(Request::Delete {
            session,
            request,
            plan,
        });
    }
    let session_request = match inv.command.as_str() {
        "publish dd session doctor" => Ok(Request::Doctor),
        "publish dd session start" => Ok(Request::Start),
        "publish dd session status" => Ok(Request::Status(session(inv)?)),
        "publish dd session stop" => {
            Ok(Request::Stop(session(inv)?.ok_or_else(|| {
                ToolkitError::arguments("--session is required")
            })?))
        }
        "publish dd options life-types" => Ok(Request::LifeTypes),
        "publish dd options channels" => Ok(Request::Channels(session(inv)?.ok_or_else(|| {
            ToolkitError::arguments(
                "--session is required; use the DDT identity returned by publish dd session start",
            )
        })?)),
        _ => return admit_read(inv),
    };
    session_request
}
fn delete_resource(command: &str) -> Option<Resource> {
    match command {
        "publish dd plugin delete" => Some(Resource::Plugin),
        "publish dd config delete" => Some(Resource::Config),
        "publish dd wa delete" => Some(Resource::Wa),
        _ => None,
    }
}
fn write_operation(command: &str) -> Option<(Resource, WriteAction)> {
    Some(match command {
        "publish dd plugin create" => (Resource::Plugin, WriteAction::Create),
        "publish dd plugin update" => (Resource::Plugin, WriteAction::Update),
        "publish dd plugin edit" => (Resource::Plugin, WriteAction::Edit),
        "publish dd wa create" => (Resource::Wa, WriteAction::Create),
        "publish dd wa update" => (Resource::Wa, WriteAction::Update),
        "publish dd wa edit" => (Resource::Wa, WriteAction::Edit),
        "publish dd config create" => (Resource::Config, WriteAction::Create),
        "publish dd config update" => (Resource::Config, WriteAction::Update),
        "publish dd config edit" => (Resource::Config, WriteAction::Edit),
        _ => return None,
    })
}
pub(super) fn file_input(path: &str) -> Option<Value> {
    if let Some(action) = path
        .strip_prefix("publish dd backup ")
        .filter(|s| matches!(*s, "generate" | "upload" | "create"))
    {
        let schema = match action {
            "generate" => BackupGenerate::schema(),
            "upload" => BackupUpload::schema(),
            _ => BackupCreate::schema(),
        };
        let mut example = if action == "generate" {
            serde_json::json!({"game_dir":"GAME_DIRECTORY","game_type":10001,"output_dir":"BACKUP_OUTPUT_DIRECTORY","unknown_addons":["SelectedAddon"],"wtf_accounts":[{"account":"SELECTED_ACCOUNT","include_common":false,"roles":[{"server":"SELECTED_SERVER","name":"SELECTED_CHARACTER"}]}]})
        } else {
            serde_json::json!({"bundle_file":"BACKUP_OUTPUT_DIRECTORY/backup-bundle.json","name":"My backup"})
        };
        example["schema"] = serde_json::json!(format!("lycheedev.publish.v1.dd.backup.{action}"));
        return Some(
            serde_json::json!({"flag":"--input","description":"One UTF-8 JSON object from a file or '-' for stdin. Paths resolve from the current directory. --plan is local; actual remote calls use the original explicit DDT. Preserve the generated bundle to reuse or recover its original upload.","schema":schema,"example":example}),
        );
    }
    if path == "publish dd backup rename" {
        return Some(
            serde_json::json!({"flag":"--input","description":"One UTF-8 JSON object from a file or '-' for stdin. --plan stays local. Execution uses the original explicit DDT and exact backup SN, saves the original name intent and verifies the fresh name; uncertain submission is never repeated.","schema":BackupRename::schema(),"example":{"schema":"lycheedev.publish.v1.dd.backup.rename","sn":"REPLACE_WITH_EXACT_BACKUP_SN","name":"New backup"}}),
        );
    }
    if path == "publish dd backup delete" {
        return Some(
            serde_json::json!({"flag":"--input","description":"One UTF-8 JSON object from a file or '-' for stdin. backup_ids contains 1..1000 unique exact string SNs and confirm_delete is true. --plan stays local. Every delete checks complete configuration references across all game types immediately before submission; completed items survive a partial batch failure.","schema":BackupDelete::schema(),"example":{"schema":"lycheedev.publish.v1.dd.backup.delete","backup_ids":["REPLACE_WITH_EXACT_BACKUP_SN"],"confirm_delete":true}}),
        );
    }
    if let Some((Resource::Plugin, action)) = write_operation(path) {
        let mut example = match action {
            WriteAction::Create => {
                serde_json::json!({"game_type":10001,"scope":"private","share_code_life_type":"seven_day","addon_type":0,"name":"Example Addon","description":"Author summary","logo_file":"logo.png","detail_img_files":["detail.png"],"primary_category_id":1,"second_category_ids":[2],"html_desc":"<p>Author details</p>","need_buy":false,"jump_room":false,"creation_statement":"original","with_associate":false,"need_anchor_vip":false,"game_versions":["REPLACE_WITH_LIVE_GAME_VERSION"],"file":"Addon.zip","release_type":1,"version":"1.0.0","update_desc":"First release"})
            }
            WriteAction::Update => {
                serde_json::json!({"sn":"REPLACE_WITH_EXACT_PLUGIN_SN","game_versions":["REPLACE_WITH_LIVE_GAME_VERSION"],"file":"Addon.zip","version":"1.0.1","update_desc":"Version notes"})
            }
            WriteAction::Edit => {
                serde_json::json!({"sn":"REPLACE_WITH_EXACT_PLUGIN_SN","description":"Updated author summary"})
            }
        };
        example["schema"] =
            serde_json::json!(format!("lycheedev.publish.v1.dd.plugin.{}", action.name()));
        return Some(
            serde_json::json!({"flag":"--input","description":"One UTF-8 JSON object from a file or '-' for stdin. Local files resolve from the current directory. --plan validates local input and binds original file bytes without opening a task or home. Execution reuses explicit DDT, validates live dependent choices, uploads selected artifacts once and verifies all submitted fields. Unknown creation recovers only the original input with explicit --recover-sn.","schema":PluginWrite::schema(action),"example":example}),
        );
    }
    if let Some((Resource::Wa, action)) = write_operation(path) {
        let mut example = match action {
            WriteAction::Create => {
                serde_json::json!({"game_type":10001,"scope":"private","share_code_life_type":"seven_day","name":"Example WA","game_version":"REPLACE_WITH_LIVE_GAME_VERSION","brief_desc":"Author summary","display_img_files":["wa.png"],"category_ids":["ui_original"],"desc":"<p>Author details</p>","need_buy":false,"jump_room":false,"creation_statement":"original","with_associate":false,"need_anchor_vip":false,"content":"REPLACE_WITH_OPAQUE_WA_EXPORT","update_desc":"First release","version":"1","with_file":false})
            }
            WriteAction::Update => {
                serde_json::json!({"sn":"REPLACE_WITH_EXACT_WA_SN","content":"REPLACE_WITH_OPAQUE_WA_EXPORT","update_desc":"Version notes","version":"2","with_file":false})
            }
            WriteAction::Edit => {
                serde_json::json!({"sn":"REPLACE_WITH_EXACT_WA_SN","brief_desc":"Updated author summary"})
            }
        };
        example["schema"] =
            serde_json::json!(format!("lycheedev.publish.v1.dd.wa.{}", action.name()));
        return Some(
            serde_json::json!({"flag":"--input","description":"One UTF-8 JSON object from a file or '-' for stdin. Modern import identity is parsed locally without Lua or SDK execution. Local images/material ZIPs bind original SHA256/size; plans never open a task or home. Execution reuses explicit DDT, validates dependent choices and verifies all submitted metadata plus original content. Unknown creation uses the original input and explicit --recover-sn.","schema":WaWrite::schema(action),"example":example}),
        );
    }
    if let Some((Resource::Config, action)) = write_operation(path) {
        let mut example = match action {
            WriteAction::Create => {
                serde_json::json!({"scope":"private","share_code_life_type":"seven_day","title":"Example configuration","brief_desc":"Author summary","desc":"<p>Author details</p>","display_img_files":["config.png"],"need_buy":false,"jump_room":false,"creation_statement":"original","with_associate":false,"need_anchor_vip":false,"backup_sn":"REPLACE_WITH_EXACT_BACKUP_SN","known_addon_ids":[],"unknown_addon_ids":[],"wtf_role_ids":["REPLACE_WITH_EXACT_BACKUP_ROLE_SELECTOR"],"material_names":[],"font_names":[],"known_wa_ids":[],"unknown_wa_ids":[],"retail_ui_config":null})
            }
            WriteAction::Update => {
                serde_json::json!({"share_sn":"REPLACE_WITH_EXACT_CONFIG_SN","backup_sn":"REPLACE_WITH_EXACT_BACKUP_SN","update_desc":"Configuration revision","known_addon_ids":[],"unknown_addon_ids":[],"wtf_role_ids":["REPLACE_WITH_EXACT_BACKUP_ROLE_SELECTOR"],"material_names":[],"font_names":[],"known_wa_ids":[],"unknown_wa_ids":[],"retail_ui_config":null})
            }
            WriteAction::Edit => {
                serde_json::json!({"share_sn":"REPLACE_WITH_EXACT_CONFIG_SN","title":"Updated configuration title"})
            }
        };
        example["schema"] =
            serde_json::json!(format!("lycheedev.publish.v1.dd.config.{}", action.name()));
        return Some(
            serde_json::json!({"flag":"--input","description":"One UTF-8 JSON object from a file or '-' for stdin. --plan validates local fields and original images without task/credential/network/home access. Execution reuses explicit DDT and the exact selected backup; group/role/WA-account and em_/cd_ layout choices come from that backup's current catalog. Omitted same-backup choices preserve current selections; a changed backup requires complete reselection. Raw configuration/layout data stays inside submission and digest readback. Unknown creation recovers the original input with explicit --recover-sn.","schema":ConfigWrite::schema(action),"example":example}),
        );
    }
    let resource = delete_resource(path)?;
    Some(
        serde_json::json!({"flag":"--input","description":"One UTF-8 JSON object from a file or '-' for piped stdin; confirm_delete must be true and sn must be the exact DD resource reference. --plan is fully local; execution requires the original retained DDT.",
        "schema":Delete::schema(resource),"example":{"schema":format!("lycheedev.publish.v1.dd.{}.delete",path.split_whitespace().nth(2).unwrap()),"sn":"REPLACE_WITH_EXACT_RESOURCE_SN","confirm_delete":true}}),
    )
}
fn selected_game(inv: &Invocation) -> Result<NonZeroU64> {
    Ok(NonZeroU64::new(super::positive(inv, "--game-type")?).unwrap())
}
fn admit_read(inv: &Invocation) -> Result<Request> {
    let sn = || Reference::parse(inv.required("--sn")?);
    let words = inv.command.split_whitespace().collect::<Vec<_>>();
    let resource = |name: &str| match name {
        "plugin" => Ok(Resource::Plugin),
        "config" => Ok(Resource::Config),
        "wa" => Ok(Resource::Wa),
        _ => Err(ToolkitError::arguments("select a DD resource")),
    };
    let request = match words.as_slice() {
        ["publish", "dd", "options", "game-types"] => Read::GameTypes,
        ["publish", "dd", "options", "vip-levels"] => Read::VipLevels,
        ["publish", "dd", "options", "associated-acts"] => {
            Read::AssociatedActs(selected_game(inv)?)
        }
        ["publish", "dd", "plugin", "categories"] => Read::PluginCategories,
        ["publish", "dd", "plugin", "game-versions"] => Read::GameVersions(selected_game(inv)?),
        ["publish", "dd", "wa", "categories"] => Read::WaCategories(selected_game(inv)?),
        ["publish", "dd", "plugin", "versions"] => Read::PluginVersions {
            reference: sn()?,
            game_type: selected_game(inv)?,
            page: NonZeroU64::new(if inv.flag("--page").is_some() {
                super::positive(inv, "--page")?
            } else {
                1
            })
            .unwrap(),
        },
        ["publish", "dd", "config", "backups"] | ["publish", "dd", "backup", "list"] => {
            Read::Backups
        }
        ["publish", "dd", "config", "backup-get"] | ["publish", "dd", "backup", "get"] => {
            Read::Backup(sn()?)
        }
        ["publish", "dd", kind, "get"] => Read::Detail {
            resource: resource(kind)?,
            reference: sn()?,
        },
        ["publish", "dd", kind, "list"] => {
            let keyword = inv.flag("--keyword").unwrap_or("");
            if keyword.len() > 4096 || keyword.contains('\0') {
                return Err(ToolkitError::arguments(
                    "DD keyword exceeds its supported text range",
                ));
            }
            Read::List {
                resource: resource(kind)?,
                game_type: selected_game(inv)?,
                keyword: keyword.into(),
                page: Page::new(
                    inv.usize_flag("--page", 1, 1, u32::MAX as usize)? as u64,
                    inv.usize_flag("--size", 50, 1, 100)? as u64,
                )?,
            }
        }
        _ => return Err(ToolkitError::arguments("unknown DD publishing command")),
    };
    Ok(Request::Read {
        session: session(inv)?.ok_or_else(|| {
            ToolkitError::arguments(
                "--session is required; use the DDT identity returned by publish dd session start",
            )
        })?,
        request,
    })
}
pub(super) fn dispatch(inv: &Invocation, request: Request) -> Result<Value> {
    inv.control.check()?;
    match request {
        Request::BackupScan(path) => dd::backup_scan(&path, &inv.control),
        Request::BackupGenerate { request, plan } => {
            if plan {
                Ok(request.plan())
            } else {
                request.generate(&inv.control)
            }
        }
        Request::BackupPublish {
            session,
            request,
            plan,
            recover_sn,
        } => {
            if plan {
                return Ok(match request {
                    BackupPublish::Upload(r) => r.plan(),
                    BackupPublish::Create(r) => r.plan(),
                });
            }
            let client = Client::attach(
                &inv.home,
                &std::env::current_exe()?,
                session
                    .as_deref()
                    .expect("DD backup admission requires an explicit task"),
            )?;
            match request {
                BackupPublish::Upload(r) => {
                    r.execute(&inv.home, &client, recover_sn.as_deref(), &inv.control)
                }
                BackupPublish::Create(r) => {
                    r.execute(&inv.home, &client, recover_sn.as_deref(), &inv.control)
                }
            }
        }
        Request::BackupPreview { session, selection } => selection.execute(
            &Client::attach(&inv.home, &std::env::current_exe()?, &session)?,
            &inv.control,
        ),
        Request::BackupWrite {
            session,
            request,
            plan,
        } => {
            if plan {
                return Ok(match request {
                    BackupWrite::Rename(r) => r.plan(),
                    BackupWrite::Delete(r) => r.plan(),
                });
            }
            let client = Client::attach(
                &inv.home,
                &std::env::current_exe()?,
                session
                    .as_deref()
                    .expect("backup write admission requires an explicit task"),
            )?;
            match request {
                BackupWrite::Rename(r) => r.execute(&inv.home, &client, &inv.control),
                BackupWrite::Delete(r) => r.execute(&inv.home, &client, &inv.control),
            }
        }
        Request::Doctor => dd::doctor(&inv.control),
        Request::Start => Client::start(&inv.home, &std::env::current_exe()?, &inv.control),
        Request::Status(session) => Client::status(
            &inv.home,
            &std::env::current_exe()?,
            session.as_deref(),
            &inv.control,
        ),
        Request::Stop(session) => {
            Client::attach(&inv.home, &std::env::current_exe()?, &session)?.stop(&inv.control)
        }
        Request::LifeTypes => Ok(dd::life_types()),
        Request::Write {
            session,
            request,
            recover_sn,
            plan,
        } => {
            if plan {
                let mut value = match &request {
                    AuthorWrite::Plugin(p) => p.plan(),
                    AuthorWrite::Wa(w) => w.plan(),
                    AuthorWrite::Config(c) => c.plan(),
                };
                value["recover_sn"] = serde_json::json!(recover_sn);
                Ok(value)
            } else {
                let client = Client::attach(
                    &inv.home,
                    &std::env::current_exe()?,
                    session
                        .as_deref()
                        .expect("publishing admission requires a retained task"),
                )?;
                match request {
                    AuthorWrite::Plugin(request) => {
                        request.execute(&inv.home, &client, recover_sn.as_deref(), &inv.control)
                    }
                    AuthorWrite::Wa(request) => {
                        request.execute(&inv.home, &client, recover_sn.as_deref(), &inv.control)
                    }
                    AuthorWrite::Config(request) => {
                        request.execute(&inv.home, &client, recover_sn.as_deref(), &inv.control)
                    }
                }
            }
        }
        Request::Delete {
            session,
            request,
            plan,
        } => {
            if plan {
                Ok(request.plan())
            } else {
                request.execute(
                    &inv.home,
                    &Client::attach(
                        &inv.home,
                        &std::env::current_exe()?,
                        session
                            .as_deref()
                            .expect("delete admission requires a retained task"),
                    )?,
                    &inv.control,
                )
            }
        }
        Request::Channels(session) => {
            Client::attach(&inv.home, &std::env::current_exe()?, &session)?.channels(&inv.control)
        }
        Request::Read { session, request } => dd::read(
            &Client::attach(&inv.home, &std::env::current_exe()?, &session)?,
            &request,
            &inv.control,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    struct Fixture(std::path::PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "lychee-dd-cli-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir(&root).unwrap();
            Self(root)
        }
        fn path(&self) -> &std::path::Path {
            &self.0
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }
    fn invocation(args: &[&str]) -> Invocation {
        crate::parse(
            &args
                .iter()
                .map(|value| value.to_string())
                .collect::<Vec<_>>(),
        )
        .unwrap()
    }
    #[test]
    fn backup_local_commands_and_remote_plans_use_explicit_files_without_opening_a_task() {
        let temp = Fixture::new();
        let game = temp.path().join("game");
        let output = temp.path().join("backup");
        let home = temp.path().join("absent-home");
        std::fs::create_dir_all(game.join("Interface/AddOns/Chosen")).unwrap();
        std::fs::write(
            game.join("Interface/AddOns/Chosen/Main.lua"),
            b"independent addon bytes",
        )
        .unwrap();
        let mut scan = invocation(&[
            "publish",
            "dd",
            "backup",
            "scan",
            "--game-dir",
            game.to_str().unwrap(),
        ]);
        scan.home = home.clone();
        let value = dispatch(&scan, admit(&scan).unwrap()).unwrap();
        assert_eq!(value["addon_names"], json!([{"name":"Chosen"}]));
        let file = temp.path().join("input.json");
        std::fs::write(&file,serde_json::to_vec(&json!({"schema":"lycheedev.publish.v1.dd.backup.generate","game_dir":game,"game_type":10001,"output_dir":output,"unknown_addons":["Chosen"]})).unwrap()).unwrap();
        let mut inv = invocation(&[
            "publish",
            "dd",
            "backup",
            "generate",
            "--input",
            file.to_str().unwrap(),
            "--plan",
        ]);
        inv.home = home.clone();
        assert_eq!(
            dispatch(&inv, admit(&inv).unwrap()).unwrap()["local_writes"],
            false
        );
        assert!(!output.exists());
        assert!(!home.exists());
        let mut inv = invocation(&[
            "publish",
            "dd",
            "backup",
            "generate",
            "--input",
            file.to_str().unwrap(),
        ]);
        inv.home = home.clone();
        let generated = dispatch(&inv, admit(&inv).unwrap()).unwrap();
        assert!(output.join("unknown_addon.zip").exists());
        for action in ["upload", "create"] {
            std::fs::write(&file,serde_json::to_vec(&json!({"schema":format!("lycheedev.publish.v1.dd.backup.{action}"),"bundle_file":generated["bundle_file"]})).unwrap()).unwrap();
            let mut plan = invocation(&[
                "publish",
                "dd",
                "backup",
                action,
                "--input",
                file.to_str().unwrap(),
                "--plan",
            ]);
            plan.home = home.clone();
            assert_eq!(
                dispatch(&plan, admit(&plan).unwrap()).unwrap()["remote_write"],
                false
            );
            let mut missing = invocation(&[
                "publish",
                "dd",
                "backup",
                action,
                "--input",
                file.to_str().unwrap(),
            ]);
            missing.home = home.clone();
            let error = admit(&missing).err().unwrap();
            assert_eq!(error.exit_code, 2);
            assert!(error.message.contains("--session"));
        }
        assert!(!home.exists());
    }
    #[test]
    fn config_write_contracts_expose_backup_role_layout_choices_and_keep_plans_local() {
        let temp = Fixture::new();
        let home = temp.path().join("absent-home");
        for action in ["create", "update", "edit"] {
            let schema =
                crate::dispatch(&invocation(&["schema", "publish", "dd", "config", action]))
                    .unwrap()
                    .data;
            let help = crate::dispatch(&invocation(&["publish", "dd", "config", action, "--help"]))
                .unwrap()
                .data;
            assert_eq!(schema["effects"], help["effects"]);
            assert_eq!(
                schema["input"]["requiredFlags"],
                serde_json::json!(["--input"])
            );
            assert_eq!(schema["effects"]["remoteWriteWhen"]["--plan"], false);
            let mut doc = schema["fileInput"]["example"].clone();
            if action == "create" {
                doc.as_object_mut().unwrap().remove("display_img_files");
                doc["display_imgs"] = serde_json::json!(["https://cdn.invalid/config.png"]);
            }
            let file = temp.path().join(format!("config-{action}.json"));
            let mut bytes = vec![0xef, 0xbb, 0xbf];
            bytes.extend(serde_json::to_vec(&doc).unwrap());
            std::fs::write(&file, bytes).unwrap();
            let mut inv = invocation(&[
                "publish",
                "dd",
                "config",
                action,
                "--input",
                file.to_str().unwrap(),
                "--plan",
            ]);
            inv.home = home.clone();
            let plan = super::super::dispatch(&inv).unwrap().data;
            assert_eq!(plan["resource"], "config");
            assert_eq!(plan["remote_write"], false);
            assert_eq!(plan["remote_read"], false);
            assert!(!home.exists());
            if action != "edit" {
                doc["retail_ui_config"] = serde_json::json!({"edit_mode_selectors":["em_bad"],"raw_layout":"private-secret"});
                std::fs::write(&file, serde_json::to_vec(&doc).unwrap()).unwrap();
                let e = super::super::dispatch(&inv).err().unwrap();
                assert_eq!(e.exit_code, 4);
                assert!(!e.to_string().contains("private-secret"));
                doc["retail_ui_config"] = serde_json::json!(null);
                std::fs::write(&file, serde_json::to_vec(&doc).unwrap()).unwrap();
            }
            inv.flags.remove("--plan");
            assert_eq!(super::super::dispatch(&inv).err().unwrap().exit_code, 2);
            inv.flags.insert("--session".into(), ID.into());
            assert_eq!(super::super::dispatch(&inv).err().unwrap().exit_code, 3);
            assert!(!home.exists());
        }
    }
    #[test]
    fn plugin_write_contracts_bind_nested_inputs_and_original_files_in_local_plans_before_task_access()
     {
        let temp = Fixture::new();
        let home = temp.path().join("absent-home");
        for action in ["create", "update", "edit"] {
            let schema =
                crate::dispatch(&invocation(&["schema", "publish", "dd", "plugin", action]))
                    .unwrap()
                    .data;
            let help = crate::dispatch(&invocation(&["publish", "dd", "plugin", action, "--help"]))
                .unwrap()
                .data;
            assert_eq!(schema["effects"], help["effects"]);
            assert_eq!(schema["effects"]["remoteWriteWhen"]["--plan"], false);
            assert_eq!(
                schema["effects"]["remoteAuthentication"],
                action != "update"
            );
            assert_eq!(
                schema["input"]["requiredFlags"],
                serde_json::json!(["--input"])
            );
            assert_eq!(
                schema["fileInput"]["schema"]["properties"]["game_versions"]["items"]["type"],
                if action == "edit" {
                    Value::Null
                } else {
                    serde_json::json!("string")
                }
            );
            let mut document = schema["fileInput"]["example"].clone();
            for key in ["file", "logo_file", "detail_img_files"] {
                document.as_object_mut().unwrap().remove(key);
            }
            if action == "create" {
                document["logo"] = serde_json::json!("https://cdn.invalid/logo.png");
                document["detail_imgs"] = serde_json::json!(["https://cdn.invalid/detail.png"]);
                document["file"] = serde_json::json!("selected.zip");
                let mut empty_zip = b"PK\x05\x06".to_vec();
                empty_zip.extend([0u8; 18]);
                std::fs::write(temp.path().join("selected.zip"), empty_zip).unwrap();
            }
            let input = temp.path().join(format!("{action}.json"));
            let mut bytes = vec![0xef, 0xbb, 0xbf];
            bytes.extend(serde_json::to_vec(&document).unwrap());
            std::fs::write(&input, bytes).unwrap();
            let mut inv = invocation(&[
                "publish",
                "dd",
                "plugin",
                action,
                "--input",
                input.to_str().unwrap(),
                "--plan",
            ]);
            inv.home = home.clone();
            inv.cwd = temp.path().to_owned();
            let plan = super::super::dispatch(&inv).unwrap().data;
            assert_eq!(plan["action"], action);
            assert_eq!(plan["remote_write"], false);
            assert_eq!(plan["remote_read"], false);
            assert!(!home.exists());
            if action == "create" {
                assert_eq!(plan["uploads"][0]["selection"]["bytes"], 22);
                inv.flags
                    .insert("--recover-sn".into(), "explicit-original".into());
                assert_eq!(
                    super::super::dispatch(&inv).unwrap().data["recover_sn"],
                    "explicit-original"
                );
            }
            inv.flags.remove("--plan");
            assert_eq!(super::super::dispatch(&inv).err().unwrap().exit_code, 2);
            assert!(!home.exists());
            inv.flags.insert("--session".into(), ID.into());
            assert_eq!(super::super::dispatch(&inv).err().unwrap().exit_code, 3);
            assert!(!home.exists());
        }
    }
    #[test]
    fn plugin_write_admission_rejects_foreign_fields_and_bad_recovery_without_home_effects() {
        let temp = Fixture::new();
        let home = temp.path().join("absent-home");
        let file = temp.path().join("edit.json");
        for document in [
            serde_json::json!({"schema":"lycheedev.publish.v1.dd.plugin.edit","sn":"selected","name":"create-only"}),
            serde_json::json!({"schema":"lycheedev.publish.v1.dd.plugin.edit","sn":"selected","associated_acts":[{"sn":"x","act_type":"wa","cookie":"private-secret"}]}),
            serde_json::json!({"schema":"lycheedev.publish.v1.dd.plugin.update","sn":"selected","description":"wrong operation"}),
        ] {
            std::fs::write(&file, serde_json::to_vec(&document).unwrap()).unwrap();
            let mut inv = invocation(&[
                "publish",
                "dd",
                "plugin",
                "edit",
                "--input",
                file.to_str().unwrap(),
                "--plan",
            ]);
            inv.home = home.clone();
            let error = super::super::dispatch(&inv).err().unwrap();
            assert_eq!(error.exit_code, 4);
            assert!(!error.to_string().contains("private-secret"));
            assert!(!home.exists());
        }
        let mut inv = invocation(&[
            "publish",
            "dd",
            "plugin",
            "create",
            "--input",
            file.to_str().unwrap(),
            "--plan",
            "--recover-sn",
            "bad\nprivate-secret",
        ]);
        inv.home = home.clone();
        let error = super::super::dispatch(&inv).err().unwrap();
        assert_eq!(error.exit_code, 2);
        assert!(!error.to_string().contains("private-secret"));
        assert!(!home.exists());
    }
    #[test]
    fn wa_write_schemas_plans_and_invalid_imports_keep_local_admission_separate_from_the_retained_task()
     {
        let temp = Fixture::new();
        let home = temp.path().join("absent-home");
        for action in ["create", "update", "edit"] {
            let schema = crate::dispatch(&invocation(&["schema", "publish", "dd", "wa", action]))
                .unwrap()
                .data;
            let help = crate::dispatch(&invocation(&["publish", "dd", "wa", action, "--help"]))
                .unwrap()
                .data;
            assert_eq!(schema["effects"], help["effects"]);
            assert_eq!(schema["effects"]["remoteWriteWhen"]["--plan"], false);
            assert_eq!(
                schema["input"]["requiredFlags"],
                serde_json::json!(["--input"])
            );
            let mut doc = schema["fileInput"]["example"].clone();
            if action == "create" {
                doc.as_object_mut().unwrap().remove("display_img_files");
                doc["display_imgs"] = serde_json::json!(["https://cdn.invalid/wa.png"]);
            }
            let file = temp.path().join(format!("wa-{action}.json"));
            std::fs::write(&file, serde_json::to_vec(&doc).unwrap()).unwrap();
            let mut inv = invocation(&[
                "publish",
                "dd",
                "wa",
                action,
                "--input",
                file.to_str().unwrap(),
                "--plan",
            ]);
            inv.home = home.clone();
            let plan = super::super::dispatch(&inv).unwrap().data;
            assert_eq!(plan["resource"], "wa");
            assert_eq!(plan["remote_write"], false);
            if action != "edit" {
                assert!(plan["fields"]["content"].is_object());
                doc["content"] = serde_json::json!("!WA:2!invalid");
                std::fs::write(&file, serde_json::to_vec(&doc).unwrap()).unwrap();
                assert_eq!(super::super::dispatch(&inv).err().unwrap().exit_code, 4);
            }
            inv.flags.remove("--plan");
            assert_eq!(super::super::dispatch(&inv).err().unwrap().exit_code, 2);
            inv.flags.insert("--session".into(), ID.into());
            if action == "edit" {
                assert_eq!(super::super::dispatch(&inv).err().unwrap().exit_code, 3);
            } else {
                assert_eq!(super::super::dispatch(&inv).err().unwrap().exit_code, 4);
            }
            assert!(!home.exists());
        }
    }
    const ID: &str = "DDT-0123456789abcdef0123456789abcdef";
    #[test]
    fn deletion_schema_and_offline_plans_keep_explicit_confirmation_and_never_open_a_task() {
        let temp = Fixture::new();
        let home = temp.path().join("absent-home");
        for (resource, action) in [
            ("plugin", "delete"),
            ("config", "delete"),
            ("wa", "delete"),
            ("backup", "delete"),
            ("backup", "rename"),
        ] {
            let schema =
                crate::dispatch(&invocation(&["schema", "publish", "dd", resource, action]))
                    .unwrap()
                    .data;
            assert_eq!(schema["effects"]["remoteWrite"], true);
            assert_eq!(schema["effects"]["remoteWriteWhen"]["--plan"], false);
            assert_eq!(schema["effects"]["remoteAuthentication"], false);
            assert_eq!(schema["input"]["requiredFlags"], json!(["--input"]));
            if action == "delete" {
                assert_eq!(
                    schema["fileInput"]["schema"]["properties"]["confirm_delete"]["enum"],
                    json!([true])
                );
            }
            let input = temp.path().join(format!("{resource}.json"));
            let mut doc = schema["fileInput"]["example"].clone();
            if resource == "backup" && action == "delete" {
                doc["backup_ids"] = json!(["exact-opaque-SN"]);
            } else {
                doc["sn"] = json!("exact-opaque-SN");
            }
            let bytes = serde_json::to_vec(&doc).unwrap();
            let mut bom = vec![0xef, 0xbb, 0xbf];
            bom.extend(bytes);
            std::fs::write(&input, bom).unwrap();
            let input = input.to_str().unwrap();
            let mut inv = invocation(&[
                "publish", "dd", resource, action, "--input", input, "--plan",
            ]);
            inv.home = home.clone();
            let plan = super::super::dispatch(&inv).unwrap().data;
            assert_eq!(plan["remote_write"], false);
            assert_eq!(plan["remote_read"], false);
            if resource == "backup" && action == "delete" {
                assert_eq!(plan["backup_ids"], json!(["exact-opaque-SN"]));
            } else {
                assert_eq!(
                    plan.get("reference").or_else(|| plan.get("sn")),
                    Some(&json!("exact-opaque-SN"))
                );
            }
            assert!(!home.exists());
            let mut missing = invocation(&["publish", "dd", resource, action, "--input", input]);
            missing.home = home.clone();
            assert_eq!(super::super::dispatch(&missing).err().unwrap().exit_code, 2);
            assert!(!home.exists());
            let mut selected = invocation(&[
                "publish",
                "dd",
                resource,
                action,
                "--input",
                input,
                "--session",
                ID,
            ]);
            selected.home = home.clone();
            assert_eq!(
                super::super::dispatch(&selected).err().unwrap().exit_code,
                3
            );
            assert!(!home.exists());
        }
    }
    #[test]
    fn deletion_rejects_missing_or_wrong_confirmation_and_foreign_schema_before_home_access() {
        let temp = Fixture::new();
        let home = temp.path().join("absent-home");
        let path = temp.path().join("delete.json");
        for value in [
            json!({"schema":"lycheedev.publish.v1.dd.plugin.delete","sn":"selected","confirm_delete":false}),
            json!({"schema":"lycheedev.publish.v1.dd.wa.delete","sn":"selected","confirm_delete":true}),
            json!({"schema":"lycheedev.publish.v1.dd.plugin.delete","sn":"selected","confirm_delete":true,"cookie":"private-secret"}),
        ] {
            std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
            let mut inv = invocation(&[
                "publish",
                "dd",
                "plugin",
                "delete",
                "--input",
                path.to_str().unwrap(),
                "--plan",
            ]);
            inv.home = home.clone();
            let error = super::super::dispatch(&inv).err().unwrap();
            assert_eq!(error.exit_code, 4);
            assert!(!error.to_string().contains("private-secret"));
            assert!(!home.exists());
        }
    }
    const READS: &[(&str, &[&str])] = &[
        ("options game-types", &[]),
        ("options life-types", &[]),
        ("options vip-levels", &[]),
        ("options channels", &[]),
        ("options associated-acts", &["--game-type", "10001"]),
        ("plugin list", &["--game-type", "10001"]),
        ("plugin get", &["--sn", "selected-plugin"]),
        ("plugin categories", &[]),
        ("plugin game-versions", &["--game-type", "10001"]),
        (
            "plugin versions",
            &["--game-type", "10001", "--sn", "selected-plugin"],
        ),
        ("config list", &["--game-type", "10001"]),
        ("config get", &["--sn", "selected-config"]),
        ("config backups", &[]),
        ("config backup-get", &["--sn", "selected-backup"]),
        ("wa list", &["--game-type", "10001"]),
        ("wa get", &["--sn", "selected-wa"]),
        ("wa categories", &["--game-type", "10001"]),
        ("backup list", &[]),
        ("backup get", &["--sn", "selected-backup"]),
        ("backup delete-preview", &["--ids", r#"["exact,+%备份"]"#]),
    ];

    #[test]
    fn admitted_reads_require_an_existing_task_and_the_offline_enum_has_no_home_side_effect() {
        let temp = Fixture::new();
        let home = temp.path().join("absent");
        for (tail, selection) in READS {
            let path = format!("publish dd {tail}");
            let mut words = path.split_whitespace().collect::<Vec<_>>();
            words.extend_from_slice(selection);
            if *tail != "options life-types" {
                words.extend_from_slice(&["--session", ID]);
            }
            let mut inv = invocation(&words);
            inv.home = home.clone();
            if *tail == "options life-types" {
                let value = super::super::dispatch(&inv).unwrap().data;
                assert_eq!(value["total"], 6);
                assert_eq!(value["remote_read"], false);
            } else {
                match admit(&inv).unwrap() {
                    Request::Channels(id) if *tail == "options channels" => assert_eq!(id, ID),
                    Request::BackupPreview { session, .. } if *tail == "backup delete-preview" => {
                        assert_eq!(session, ID)
                    }
                    Request::Read { session, .. } if *tail != "options channels" => {
                        assert_eq!(session, ID)
                    }
                    _ => panic!("read admission selected the wrong typed operation: {tail}"),
                }
                assert_eq!(
                    super::super::dispatch(&inv).err().unwrap().exit_code,
                    3,
                    "{tail}"
                );
            }
            assert!(!home.exists(), "{tail}");
        }
    }
    #[test]
    fn read_argument_failures_stop_before_task_or_home_lookup() {
        let temp = Fixture::new();
        let home = temp.path().join("absent");
        for words in [
            vec!["options", "game-types"],
            vec!["options", "vip-levels", "--session", "DDT-private-secret"],
            vec!["plugin", "list", "--session", ID, "--game-type", "0"],
            vec![
                "plugin",
                "list",
                "--session",
                ID,
                "--game-type",
                "10001",
                "--size",
                "101",
            ],
            vec![
                "plugin",
                "list",
                "--session",
                ID,
                "--game-type",
                "10001",
                "--page",
                "0",
            ],
            vec!["plugin", "get", "--session", ID, "--sn", ""],
            vec![
                "plugin",
                "get",
                "--session",
                ID,
                "--sn",
                "private-secret\u{001f}",
            ],
            vec![
                "plugin",
                "versions",
                "--session",
                ID,
                "--sn",
                "selected",
                "--game-type",
                "10001",
                "--page",
                "0",
            ],
            vec!["wa", "categories", "--session", ID, "--game-type", "-1"],
        ] {
            let mut args = vec!["publish", "dd"];
            args.extend(words);
            args.extend(["--home", home.to_str().unwrap()]);
            let args = args.into_iter().map(str::to_owned).collect::<Vec<_>>();
            let error = crate::parse(&args)
                .and_then(|inv| super::super::dispatch(&inv))
                .err()
                .unwrap();
            assert_eq!(error.exit_code, 2);
            assert!(!error.to_string().contains("private-secret"));
            assert!(!home.exists());
        }
    }

    #[test]
    fn invalid_selectors_are_argument_errors_before_any_home_or_session_effect() {
        let temp = Fixture::new();
        let home = temp.path().join("absent");
        for action in ["status", "stop"] {
            for id in [
                "DDT-private-secret",
                "../private-secret",
                "DDT-0123456789ABCDEF0123456789ABCDEF",
                "",
            ] {
                let args = [
                    "publish",
                    "dd",
                    "session",
                    action,
                    "--session",
                    id,
                    "--home",
                    home.to_str().unwrap(),
                ]
                .map(str::to_owned);
                let error = crate::parse(&args)
                    .and_then(|inv| super::super::dispatch(&inv))
                    .err()
                    .unwrap();
                assert_eq!(error.exit_code, 2);
                assert!(!error.to_string().contains("private-secret"));
                assert!(!home.exists());
            }
        }
        assert!(admit(&invocation(&["publish", "dd", "session", "stop"])).is_err());
    }
    #[test]
    fn status_and_missing_stop_do_not_start_a_session_or_create_home() {
        let temp = Fixture::new();
        let home = temp.path().join("absent");
        for selected in [false, true] {
            let mut inv = invocation(&["publish", "dd", "session", "status"]);
            if selected {
                inv.flags.insert("--session".into(), ID.into());
            }
            inv.home = home.clone();
            let status = super::super::dispatch(&inv).unwrap().data;
            assert_eq!(status["running"], false);
            if selected {
                assert_eq!(status["session_id"], ID);
            }
            assert!(!home.exists());
        }
        let mut inv = invocation(&["publish", "dd", "session", "stop", "--session", ID]);
        inv.home = home.clone();
        assert!(super::super::dispatch(&inv).is_err());
        assert!(!home.exists());
        let mut inv = invocation(&["publish", "dd", "session", "status"]);
        inv.home = temp.path().to_owned();
        assert_eq!(super::super::dispatch(&inv).unwrap().data["running"], false);
        assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 0);
    }
    #[test]
    fn cancelled_start_stops_before_creating_a_task_or_reading_credentials() {
        let temp = Fixture::new();
        let home = temp.path().join("absent");
        let mut inv = invocation(&["publish", "dd", "session", "start"]);
        inv.home = home.clone();
        inv.control.tracked_cancel_handle().unwrap().cancel();
        assert_eq!(super::super::dispatch(&inv).err().unwrap().exit_code, 7);
        assert!(!home.exists());
    }
}
