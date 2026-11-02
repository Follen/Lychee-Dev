use crate::Invocation;
use lycheedev_core::{Result, ToolkitError};
use lycheedev_publish::blackbox::{
    self, AuthAction, AuthWrite, BackupCreate, BackupGenerate, BackupUpload, BackupWrite, Query,
    Write,
};
use serde_json::{Value, json};
pub(super) enum Request {
    Doctor,
    Status(String),
    Auth {
        request: AuthWrite,
        plan: bool,
    },
    Read(Query),
    Write {
        request: Write,
        plan: bool,
    },
    Scan {
        root: std::path::PathBuf,
        branch: String,
    },
    Backup {
        request: BackupWrite,
        plan: bool,
    },
    Generate {
        request: BackupGenerate,
        plan: bool,
    },
    Upload {
        request: BackupUpload,
        plan: bool,
    },
    Create {
        request: BackupCreate,
        plan: bool,
    },
}
fn action(path: &str) -> Option<AuthAction> {
    match path.strip_prefix("publish blackbox session ")? {
        "login-start" => Some(AuthAction::Start),
        "login-submit" => Some(AuthAction::Submit),
        "renew" => Some(AuthAction::Renew),
        "forget" => Some(AuthAction::Forget),
        _ => None,
    }
}
pub(super) fn admit(inv: &Invocation) -> Result<Request> {
    if let Some(action) = inv.command.strip_prefix("publish blackbox backup ") {
        let plan = inv.flag_bool("--plan");
        match action {
            "scan" => {
                return Ok(Request::Scan {
                    root: inv.cwd.join(inv.required("--game-dir")?),
                    branch: inv.required("--strappid")?.into(),
                });
            }
            "rename" | "delete" | "delete-preview" => {
                return Ok(Request::Backup {
                    request: BackupWrite::parse(action, &super::read_input(inv)?)?,
                    plan,
                });
            }
            "generate" => {
                return Ok(Request::Generate {
                    request: BackupGenerate::parse(&super::read_input(inv)?, &inv.cwd)?,
                    plan,
                });
            }
            "upload" => {
                return Ok(Request::Upload {
                    request: BackupUpload::parse(&super::read_input(inv)?, &inv.cwd, &inv.control)?,
                    plan,
                });
            }
            "create" => {
                return Ok(Request::Create {
                    request: BackupCreate::parse(&super::read_input(inv)?, &inv.cwd, &inv.control)?,
                    plan,
                });
            }
            _ => {}
        }
    }
    if let Some(action) = action(&inv.command) {
        return Ok(Request::Auth {
            request: AuthWrite::parse(action, &super::read_input(inv)?)?,
            plan: inv.flag_bool("--plan"),
        });
    }
    if let Some((resource, action)) = write_action(&inv.command) {
        return Ok(Request::Write {
            request: Write::parse(resource, action, &super::read_input(inv)?, &inv.cwd)?,
            plan: inv.flag_bool("--plan"),
        });
    }
    let value = |flag: &str| inv.required(flag).map(str::to_owned);
    let query = match inv.command.as_str() {
        "publish blackbox session doctor" => return Ok(Request::Doctor),
        "publish blackbox session login-status" => {
            return Ok(Request::Status(value("--login-id")?));
        }
        "publish blackbox options game-types" => Query::GameTypes,
        "publish blackbox plugin list" => Query::Plugins,
        "publish blackbox plugin get" => Query::Plugin(super::positive(inv, "--module-id")?),
        "publish blackbox plugin versions" => {
            Query::PluginVersions(super::positive(inv, "--module-id")?)
        }
        "publish blackbox config list" => Query::Configs(value("--strappid")?),
        "publish blackbox config get" => Query::Config(value("--share-id")?),
        "publish blackbox backup list" => Query::Backups,
        "publish blackbox backup get" => Query::Backup(value("--backup-id")?),
        "publish blackbox wa list" => Query::Was(value("--strappid")?),
        "publish blackbox wa get" => Query::Wa {
            branch: value("--strappid")?,
            slug: value("--slug")?,
        },
        "publish blackbox wa categories" => Query::WaCategories,
        _ => {
            return Err(ToolkitError::arguments(
                "unknown Heybox publishing operation",
            ));
        }
    };
    Ok(Request::Read(query))
}
pub(super) fn dispatch(inv: &Invocation, request: Request) -> Result<Value> {
    match request {
        Request::Scan { root, branch } => blackbox::scan_backup(&root, &branch, &inv.control),
        Request::Backup {
            request,
            plan: true,
        } => Ok(request.plan()),
        Request::Backup {
            request,
            plan: false,
        } => request.execute(&inv.home, &inv.control),
        Request::Generate {
            request,
            plan: true,
        } => request.plan(&inv.control),
        Request::Generate {
            request,
            plan: false,
        } => request.generate(&inv.control),
        Request::Upload {
            request,
            plan: true,
        } => Ok(request.plan()),
        Request::Upload {
            request,
            plan: false,
        } => request.execute(&inv.home, inv.flag("--recover-backup-id"), &inv.control),
        Request::Create {
            request,
            plan: true,
        } => request.plan(&inv.control),
        Request::Create {
            request,
            plan: false,
        } => request.execute(&inv.home, inv.flag("--recover-backup-id"), &inv.control),
        Request::Doctor => blackbox::doctor(&inv.home, &inv.control),
        Request::Status(id) => blackbox::login_status(&inv.home, &id, &inv.control),
        Request::Auth {
            request,
            plan: true,
        } => Ok(request.plan()),
        Request::Auth {
            request,
            plan: false,
        } => request.execute(&inv.home, &inv.control),
        Request::Read(query) => blackbox::read(&inv.home, query, &inv.control),
        Request::Write {
            request,
            plan: true,
        } => request.plan(&inv.control),
        Request::Write {
            request,
            plan: false,
        } => request.execute(&inv.home, &inv.control),
    }
}
pub(super) fn file_input(path: &str) -> Option<Value> {
    if let Some(action) = path.strip_prefix("publish blackbox backup ") {
        let input = match action {
            "generate" => Some((
                BackupGenerate::schema(),
                json!({"game_dir":"D:/World of Warcraft/_retail_","output_dir":"D:/Backups/author","strappid":"wowretail","unknown_addons":["AuthorAddon"],"materials":[],"fonts":[],"wtf_accounts":[]}),
            )),
            "upload" => Some((
                BackupUpload::schema(),
                json!({"bundle_file":"D:/Backups/author/backup-bundle.json"}),
            )),
            "create" => Some((
                BackupCreate::schema(),
                json!({"bundle_file":"D:/Backups/author/backup-bundle.json"}),
            )),
            "rename" => Some((
                BackupWrite::schema(action),
                json!({"backup_id":"EXACT_BACKUP_ID","name":"Author backup"}),
            )),
            "delete-preview" => Some((
                BackupWrite::schema(action),
                json!({"backup_ids":["EXACT_BACKUP_ID"]}),
            )),
            "delete" => Some((
                BackupWrite::schema(action),
                json!({"backup_ids":["EXACT_BACKUP_ID"],"confirm_delete":true}),
            )),
            _ => None,
        };
        if let Some((schema, mut example)) = input {
            example["schema"] = json!(format!("lycheedev.publish.v1.blackbox.backup.{action}"));
            return Some(
                json!({"flag":"--input","description":"One UTF-8 JSON file or '-' for stdin. --plan validates locally without credentials or network. Generation requires explicit scan selections; create/upload reuse the original bundle. Cloud writes use the selected CLI account and retain original allocation/object/registration identities. Repeat the original input to resume. Delete-preview performs remote reference checks; it never removes a backup.","schema":schema,"example":example}),
            );
        }
    }
    if let Some((resource, action)) = write_action(path) {
        let mut example = match (resource, action) {
            ("plugin", "edit") => json!({"id":719,"desc":"Updated description"}),
            ("plugin", "update") => {
                json!({"module_id":719,"name":"1.0.1","type":1,"game_versions":["12.0.0"],"file":"AuthorAddon.zip"})
            }
            ("version", "edit") => {
                json!({"module_id":719,"version_id":17,"name":"1.0.1","type":1,"game_versions":["12.0.0"]})
            }
            ("version", "delete") => json!({"module_id":719,"version_id":17}),
            ("config", "create") => {
                json!({"backup_id":"EXACT_BACKUP_ID","configuration":{"Title":"Author configuration","Article":"<p>Description</p>","Open":false,"ThumbList":[],"SubscribeLevel":0,"KnownAddons":[{"ID":719,"Name":"AuthorAddon","Strategy":1,"VersionID":17}],"UnknownAddons":{"Files":[]},"Materials":{"Files":[]},"Fonts":{"Files":[]},"WTFList":[]}})
            }
            ("config", "delete") => json!({"share_id":"EXACT_SHARE_ID","confirm_delete":true}),
            ("config", _) => {
                json!({"share_id":"EXACT_SHARE_ID","configuration":{"Title":"Updated title"}})
            }
            ("wa", "create") => {
                json!({"scope":"public","configuration":{"categories":["SELECT_CURRENT_TAG_KEY"],"strappid":"wowretail","open":true,"name":"Author WA","screens":[{"src":"https://example.invalid/image.png","thumb":"https://example.invalid/image.png"}],"encoded":"REPLACE_WITH_ORIGINAL_WA_IMPORT","subscribeLevel":0}})
            }
            ("wa", "delete") => {
                json!({"slug":"EXACT_WA_SLUG","strappid":"wowretail","confirm_delete":true})
            }
            ("wa", _) => {
                json!({"slug":"EXACT_WA_SLUG","scope":"public","configuration":{"strappid":"wowretail","open":true,"subscribeLevel":0,"name":"Updated title"}})
            }
            _ => unreachable!(),
        };
        example["schema"] = json!(format!("lycheedev.publish.v1.blackbox.{resource}.{action}"));
        return Some(
            json!({"flag":"--input","description":"One UTF-8 JSON file or '-' for stdin. --plan admits local fields/files only, without credentials or network. Use query commands for exact account-owned IDs and dynamic choices. Repeat the original input to recover its saved request; unknown writes are not replayed. Public WA is not private storage; config hiding is not access control.","schema":Write::schema(resource,action),"example":example}),
        );
    }
    let action = action(path)?;
    let mut example = match action {
        AuthAction::Start => json!({"method":"wechat"}),
        AuthAction::Submit => {
            json!({"login_id":"0123456789abcdef0123456789abcdef","code":"123456"})
        }
        AuthAction::Renew => json!({}),
        AuthAction::Forget => json!({"confirm_forget":true}),
    };
    example["schema"] = json!(format!(
        "lycheedev.publish.v1.blackbox.session.{}",
        action.key()
    ));
    Some(
        json!({"flag":"--input","description":"One UTF-8 JSON file or '-' for stdin; --plan validates locally without credentials, network or home writes. SMS and WeChat are explicit short-lived login steps. Continue the original login_id, keep its original deadline, and never resend after an unknown SMS/code/callback result. Login uses current-user encrypted CLI storage; only the official desktop device-risk field may be read, without borrowing its account cookies.","schema":AuthWrite::schema(action),"example":example}),
    )
}
fn write_action(path: &str) -> Option<(&str, &str)> {
    let (resource, action) = path.strip_prefix("publish blackbox ")?.split_once(' ')?;
    matches!(
        (resource, action),
        ("plugin", "edit" | "update")
            | ("version", "edit" | "delete")
            | ("config" | "wa", "create" | "update" | "edit" | "delete")
    )
    .then_some((resource, action))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn plans_admit_public_examples_and_reject_conditional_fields_without_creating_home() {
        let root = std::env::temp_dir().join(format!(
            "lychee-heybox-cli-{}",
            lycheedev_windows::random::token().unwrap()
        ));
        std::fs::create_dir(&root).unwrap();
        let input = root.join("input.json");
        let home = root.join("unused-home");
        for action in [
            "session login-start",
            "session login-submit",
            "session renew",
            "session forget",
            "plugin edit",
            "version edit",
            "version delete",
            "config create",
            "config update",
            "config edit",
            "config delete",
            "wa create",
            "wa update",
            "wa edit",
            "wa delete",
            "backup rename",
            "backup delete-preview",
            "backup delete",
        ] {
            let path = format!("publish blackbox {action}");
            let example = file_input(&path).unwrap()["example"].clone();
            std::fs::write(&input, serde_json::to_vec(&example).unwrap()).unwrap();
            let mut args = path
                .split_whitespace()
                .map(str::to_owned)
                .collect::<Vec<_>>();
            args.extend([
                "--input".into(),
                input.display().to_string(),
                "--home".into(),
                home.display().to_string(),
                "--plan".into(),
                "--format".into(),
                "json".into(),
            ]);
            let inv = crate::parse(&args).unwrap();
            let result = super::super::dispatch(&inv).unwrap();
            assert_eq!(result.data["credential_reads"], false);
            assert_eq!(result.data["remote_write"], false);
            assert!(!home.exists());
        }
        for value in [
            json!({"method":"wechat","phone":"13812345678"}),
            json!({"method":"sms"}),
            json!({"method":"sms","phone":"138秘密"}),
        ] {
            let mut value = value;
            value["schema"] = json!("lycheedev.publish.v1.blackbox.session.login-start");
            assert!(
                AuthWrite::parse(AuthAction::Start, &serde_json::to_vec(&value).unwrap()).is_err()
            );
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
