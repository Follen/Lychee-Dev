use crate::Invocation;
use lycheedev_core::{Result, ToolkitError};
use lycheedev_publish::modus::{
    self, BackupCloud, BackupLocal, BackupUpload, Client, Listing, MainWrite, MediaUpload,
    PluginWrite, ProjectWrite, Query,
};
use serde_json::{Value, json};
pub(super) enum Request {
    BackupCloud {
        request: BackupCloud,
        plan: bool,
    },
    BackupLocal {
        request: BackupLocal,
        plan: bool,
    },
    BackupUpload {
        request: BackupUpload,
        plan: bool,
    },
    Plugin {
        request: PluginWrite,
        plan: bool,
        recover: Option<u64>,
    },
    Media {
        request: MediaUpload,
        plan: bool,
    },
    Doctor(Client),
    Read(Query),
    Project {
        request: ProjectWrite,
        plan: bool,
    },
    Main {
        request: MainWrite,
        plan: bool,
    },
}
fn array<T: serde::de::DeserializeOwned>(inv: &Invocation, flag: &str) -> Result<Vec<T>> {
    serde_json::from_str(inv.required(flag)?).map_err(|_| {
        ToolkitError::arguments(format!(
            "{flag} requires a JSON array; inspect this command's schema"
        ))
    })
}
fn build(inv: &Invocation) -> Result<Option<u8>> {
    inv.flag("--server-type")
        .map(|s| {
            s.parse::<u8>().ok().filter(|n| *n <= 4).ok_or_else(|| {
                ToolkitError::arguments("--server-type must be 0..4; use publish modus builds")
            })
        })
        .transpose()
}
fn number(inv: &Invocation, flag: &str) -> Result<Option<u64>> {
    inv.flag(flag)
        .map(|_| super::positive(inv, flag))
        .transpose()
}
pub(super) fn admit(inv: &Invocation) -> Result<Request> {
    if let Some(action) = backup_action(&inv.command) {
        let bytes = super::read_input(inv)?;
        let plan = inv.flag_bool("--plan");
        return match action {
            "scan" | "generate" | "create" => Ok(Request::BackupLocal {
                request: BackupLocal::parse(action, &bytes, &std::env::current_dir()?)?,
                plan,
            }),
            "upload" => Ok(Request::BackupUpload {
                request: BackupUpload::parse(&bytes, &std::env::current_dir()?)?,
                plan,
            }),
            _ => Ok(Request::BackupCloud {
                request: BackupCloud::parse(action, &bytes)?,
                plan,
            }),
        };
    }
    if inv.command == "publish modus media upload" {
        return Ok(Request::Media {
            request: MediaUpload::parse(&super::read_input(inv)?, &std::env::current_dir()?)?,
            plan: inv.flag_bool("--plan"),
        });
    }
    if let Some(action) = inv
        .command
        .strip_prefix("publish modus plugin ")
        .filter(|s| ["create", "upload", "edit", "update", "delete"].contains(s))
    {
        return Ok(Request::Plugin {
            request: PluginWrite::parse(
                action,
                &super::read_input(inv)?,
                &std::env::current_dir()?,
            )?,
            plan: inv.flag_bool("--plan"),
            recover: number(inv, "--recover-file-id")?,
        });
    }
    if let Some((resource, action)) = main_action(&inv.command) {
        return Ok(Request::Main {
            request: MainWrite::parse(resource, action, &super::read_input(inv)?)?,
            plan: inv.flag_bool("--plan"),
        });
    }
    if let Some(action) = inv
        .command
        .strip_prefix("publish modus project ")
        .filter(|s| ["create", "edit", "delete"].contains(s))
    {
        return Ok(Request::Project {
            request: ProjectWrite::parse(action, &super::read_input(inv)?)?,
            plan: inv.flag_bool("--plan"),
        });
    }
    let path = inv
        .command
        .strip_prefix("publish modus ")
        .ok_or_else(|| ToolkitError::arguments("unknown ModUs command"))?;
    if path == "session doctor" {
        return Ok(Request::Doctor(
            match inv.flag("--client").unwrap_or("creator") {
                "creator" => Client::Creator,
                "main" => Client::Main,
                _ => return Err(ToolkitError::arguments("--client must be creator or main")),
            },
        ));
    }
    let query = match path {
        "builds" => Query::Builds,
        "account info" => Query::Account,
        "account subscription-count" => Query::SubscriptionCount,
        "account statistics" => Query::Statistics,
        path if path.starts_with("options ") => Query::Options {
            action: path[8..].into(),
            keys: if inv.flag("--keys").is_some() {
                array(inv, "--keys")?
            } else {
                vec![]
            },
            author: number(inv, "--author-id")?,
            game_type: inv.flag("--game-type").map(str::to_owned),
        },
        "addon info" => Query::Addons {
            directories: array(inv, "--directories")?,
            build: build(inv)?,
        },
        "addon project-info" => Query::AddonProjects {
            projects: array(inv, "--project-ids")?,
            build: build(inv)?,
        },
        "addon history" | "project list" | "plugin list" | "plugin versions" | "config list"
        | "wa list" => {
            let kind = match path {
                "addon history" => Listing::AddonHistory(super::positive(inv, "--project-id")?),
                "project list" => Listing::Projects,
                "plugin list" | "plugin versions" => {
                    Listing::Releases(super::positive(inv, "--project-id")?)
                }
                "config list" => Listing::Configs,
                _ => Listing::Was,
            };
            let mut filters = serde_json::Map::new();
            for (flag, key) in [
                ("--keyword", "keyword"),
                ("--order-by", "orderBy"),
                ("--support-addon", "supportAddon"),
            ] {
                if let Some(value) = inv.flag(flag) {
                    filters.insert(key.into(), json!(value));
                }
            }
            for (flag, key) in [
                ("--status", "status"),
                ("--share-type", "shareType"),
                ("--is-public", "isPublic"),
                ("--is-paid", "isPaid"),
            ] {
                if let Some(value) = inv.flag(flag) {
                    let value = value.parse::<u64>().map_err(|_| {
                        ToolkitError::arguments(format!("{flag} requires a nonnegative integer"))
                    })?;
                    filters.insert(key.into(), json!(value));
                }
            }
            if let Some(value) = inv.flag("--mine") {
                filters.insert(
                    "mine".into(),
                    json!(
                        value
                            .parse::<bool>()
                            .map_err(|_| ToolkitError::arguments("--mine must be true or false"))?
                    ),
                );
            }
            if inv.flag("--tags").is_some() {
                filters.insert("tags".into(), json!(array::<String>(inv, "--tags")?));
            }
            let size = number(inv, "--page-size")?.unwrap_or(50);
            if size > 100 {
                return Err(ToolkitError::arguments("--page-size must be 1..100"));
            }
            Query::List {
                kind,
                build: build(inv)?,
                page: number(inv, "--page")?,
                size,
                filters: Value::Object(filters),
            }
        }
        "project get" => Query::Project(super::positive(inv, "--project-id")?),
        "project dependencies" => {
            let value = if let Some(raw) = inv.flag("--query") {
                if raw.starts_with(['{', '[']) {
                    serde_json::from_str(raw)
                        .map_err(|_| ToolkitError::arguments("--query JSON is invalid"))?
                } else {
                    json!(raw)
                }
            } else {
                json!(array::<u64>(inv, "--project-ids")?)
            };
            Query::Dependencies(value)
        }
        "plugin get" => Query::Release {
            project: super::positive(inv, "--project-id")?,
            file: super::positive(inv, "--file-id")?,
        },
        "config get" => Query::Config {
            id: inv.required("--share-id")?.into(),
            build: build(inv)?,
        },
        "wa get" => Query::Wa {
            id: inv.required("--import-id")?.into(),
            build: build(inv)?,
        },
        "backup list" | "config backups" => Query::Backups(build(inv)?),
        "backup get" | "config backup-get" => Query::Backup {
            id: super::positive(inv, "--backup-id")?,
            build: build(inv)?,
        },
        _ => return Err(ToolkitError::arguments("unknown ModUs command")),
    };
    Ok(Request::Read(query))
}
pub(super) fn dispatch(inv: &Invocation, request: Request) -> Result<Value> {
    match request {
        Request::BackupCloud {
            request,
            plan: true,
        } => Ok(request.plan()),
        Request::BackupCloud {
            request,
            plan: false,
        } => request.execute(&inv.home, &inv.control),
        Request::BackupLocal {
            request,
            plan: true,
        } => request.plan(&inv.control),
        Request::BackupLocal {
            request,
            plan: false,
        } => request.execute(&inv.home, &inv.control),
        Request::BackupUpload {
            request,
            plan: true,
        } => request.plan(&inv.control),
        Request::BackupUpload {
            request,
            plan: false,
        } => request.execute(&inv.home, &inv.control),
        Request::Plugin {
            request,
            plan: true,
            ..
        } => request.plan(&inv.control),
        Request::Plugin {
            request,
            plan: false,
            recover,
        } => request.execute(&inv.home, recover, &inv.control),
        Request::Media {
            request,
            plan: true,
        } => request.plan(&inv.control),
        Request::Media {
            request,
            plan: false,
        } => request.execute(&inv.home, inv.flag("--recover-key"), &inv.control),
        Request::Main {
            request,
            plan: true,
        } => Ok(request.plan()),
        Request::Main {
            request,
            plan: false,
        } => request.execute(&inv.home, &inv.control),
        Request::Project {
            request,
            plan: true,
        } => Ok(request.plan()),
        Request::Project {
            request,
            plan: false,
        } => request.execute(&inv.home, &inv.control),
        Request::Doctor(client) => modus::doctor(client, &inv.control),
        Request::Read(query) => modus::read(query, &inv.control),
    }
}

pub(super) fn file_input(path: &str) -> Option<Value> {
    if let Some(action) = backup_action(path) {
        let schema = match action {
            "scan" | "generate" | "create" => BackupLocal::schema(action),
            "upload" => BackupUpload::schema(),
            _ => BackupCloud::schema(action),
        };
        let mut example = match action {
            "scan" => json!({"game_root":"D:/WorldOfWarcraft/_retail_","server_type":0}),
            "generate" | "create" => {
                json!({"game_root":"D:/WorldOfWarcraft/_retail_","output_dir":"D:/AuthorBackups/ModUs","backup_name":"Author configuration","server_type":0,"addon_names":["AuthorAddon"],"roles":[{"account":"ACCOUNT","server":"Realm","role":"Character"}]})
            }
            "upload" => {
                json!({"manifest_file":"D:/AuthorBackups/ModUs/manifest.v2.json","server_type":0})
            }
            "rename" => {
                json!({"backup_id":719,"backup_name":"Updated backup name","server_type":0})
            }
            "delete" => json!({"backup_ids":[719],"server_type":0,"confirm":"DELETE"}),
            _ => json!({"backup_ids":[719],"server_type":0}),
        };
        example["schema"] = json!(format!("lycheedev.publish.v1.modus.backup.{action}"));
        return Some(
            json!({"flag":"--input","stdin":"-","encoding":"UTF-8 JSON","description":"Native ModUs backup. config backup-edit and backup-delete are aliases using the canonical backup rename/delete schema. Explicit selections only; original local manifest, cloud IDs, objects and completed batch items are reused. --plan does not read credentials or contact the platform.","schema":schema,"example":example}),
        );
    }
    if path == "publish modus media upload" {
        return Some(
            json!({"flag":"--input","stdin":"-","encoding":"UTF-8 JSON","description":"One local image; the native media receipt is reused. A lost response uses --recover-key to check original bytes, never another upload.","schema":MediaUpload::schema(),"example":{"schema":"lycheedev.publish.v1.modus.media.upload","file":"author.png"}}),
        );
    }
    if let Some(action) = path
        .strip_prefix("publish modus plugin ")
        .filter(|s| ["create", "upload", "edit", "update", "delete"].contains(s))
    {
        let mut example = match action {
            "create" | "upload" => {
                json!({"project_id":200000719,"file":"AuthorAddon.zip","version":"1.0.0"})
            }
            "delete" => json!({"project_id":200000719,"file_id":720,"confirm":"DELETE"}),
            _ => {
                json!({"project_id":200000719,"file_id":720,"changelog":"<p>Updated release notes.</p>"})
            }
        };
        example["schema"] = json!(format!("lycheedev.publish.v1.modus.plugin.{action}"));
        return Some(
            json!({"flag":"--input","stdin":"-","encoding":"UTF-8 JSON","description":"Creator release form. Existing file_id stays fixed on update. ZIP-derived fields must match actual bytes; current game versions are selected at execution. Repeat the exact input for pending readback, without another uncertain metadata or object write.","schema":PluginWrite::schema(action),"example":example}),
        );
    }
    if let Some((resource, action)) = main_action(path) {
        let key = if resource == "config" {
            "share_id"
        } else {
            "import_id"
        };
        let mut example = match action {
            "create" => {
                let mut value = json!({"title":"Author configuration","content":"<p>A complete author description for this configuration.</p>","image_url":"image-reference-from-media-upload","tags":"1","addons_id":"22"});
                if resource == "config" {
                    value["backup_id"] = json!(719);
                    value["exclude_wtf"] = json!(1);
                } else {
                    value["code_text"] = json!("Author import string");
                    value["support_addon"] = json!("WeakAuras");
                    value["version"] = json!("1.0.0");
                }
                value
            }
            "delete" => json!({key:"719","confirm":"DELETE"}),
            "version-publish" => {
                json!({"import_id":"719","version":"1.0.1","code_text":"Updated author import string"})
            }
            "version-delete" => json!({"version_id":"720","import_id":"719","confirm":"DELETE"}),
            _ => json!({key:"719","title":"Updated author title"}),
        };
        example["schema"] = json!(format!("lycheedev.publish.v1.modus.{resource}.{action}"));
        return Some(
            json!({"flag":"--input","stdin":"-","encoding":"UTF-8 JSON","description":"Main-client form. Omitted update fields preserve the original detail; content_text is derived when omitted. Current options and backup/role linkage are checked at execution. Repeat the exact request for read-only recovery in the original build.","schema":MainWrite::schema(resource,action),"example":example}),
        );
    }
    let action = path
        .strip_prefix("publish modus project ")
        .filter(|s| ["create", "edit", "delete"].contains(s))?;
    let mut example = match action {
        "create" => {
            json!({"name":"AuthorAddon","summary":"An author addon","categories":[1],"license":{"type":"MIT"},"publish_platforms":["modus"],"required_tier_id":null})
        }
        "edit" => json!({"project_id":200000719,"alt_name":"作者插件"}),
        _ => json!({"project_id":200000719,"confirm":"DELETE"}),
    };
    example["schema"] = json!(format!("lycheedev.publish.v1.modus.project.{action}"));
    Some(
        json!({"flag":"--input","stdin":"-","encoding":"UTF-8 JSON","description":"Native Creator project form. Omitted fields preserve values; explicit blank/null clearing is field-specific. Current categories and tiers come from options. Repeating the exact request reads its original pending result; no unknown submission is replayed.","schema":ProjectWrite::schema(action),"example":example}),
    )
}
fn backup_action(path: &str) -> Option<&str> {
    match path {
        "publish modus config backup-edit" => Some("rename"),
        "publish modus config backup-delete" => Some("delete"),
        _ => path.strip_prefix("publish modus backup ").filter(|s| {
            [
                "scan",
                "generate",
                "create",
                "upload",
                "rename",
                "delete",
                "delete-preview",
            ]
            .contains(s)
        }),
    }
}
fn main_action(path: &str) -> Option<(&str, &str)> {
    let (resource, action) = path.strip_prefix("publish modus ")?.split_once(' ')?;
    if ["config", "wa"].contains(&resource)
        && (["create", "update", "edit", "delete"].contains(&action)
            || resource == "wa" && ["version-publish", "version-delete"].contains(&action))
    {
        Some((resource, action))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request(args: &[&str]) -> Result<Request> {
        let args = [vec!["publish", "modus"], args.to_vec()]
            .concat()
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        admit(&crate::parse(&args)?)
    }
    #[test]
    fn main_filter_zero_false_and_creator_dependency_selection_reach_typed_admission() {
        let Request::Read(Query::List {
            build,
            page,
            size,
            filters,
            ..
        }) = request(&[
            "wa",
            "list",
            "--server-type",
            "0",
            "--mine",
            "false",
            "--status",
            "0",
            "--is-paid",
            "0",
            "--tags",
            r#"["团队"]"#,
            "--page",
            "3",
            "--page-size",
            "7",
        ])
        .unwrap()
        else {
            panic!("list admission");
        };
        assert_eq!(build, Some(0));
        assert_eq!(page, Some(3));
        assert_eq!(size, 7);
        assert_eq!(
            filters,
            json!({"mine":false,"status":0,"isPaid":0,"tags":["团队"]})
        );
        let Request::Read(Query::Dependencies(value)) = request(&[
            "project",
            "dependencies",
            "--project-ids",
            "[200000719,200000720]",
        ])
        .unwrap() else {
            panic!("dependency admission");
        };
        assert_eq!(value, json!([200000719, 200000720]));
        for args in [
            vec!["project", "dependencies"],
            vec!["addon", "info", "--directories", "not-json"],
            vec!["config", "get", "--share-id", "id", "--server-type", "9"],
            vec!["plugin", "get", "--project-id", "0", "--file-id", "7"],
        ] {
            assert_eq!(request(&args).err().unwrap().exit_code, 2);
        }
    }
}
