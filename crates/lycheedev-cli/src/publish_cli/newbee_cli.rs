use super::{positive, read_input};
use crate::Invocation;
use lycheedev_core::{Result, ToolkitError};
use lycheedev_publish::newbee::{
    self, AuthorCreate, AuthorDelete, BackupCreate, BackupDelete, BackupGenerate, BackupRead,
    BackupRename, BackupSelection, BackupUpload, ConfigEdit, ConfigUpdate, Newbee, OptionKind,
    Page, PluginChangelogEdit, PluginEdit, PluginUpdate, Read, RelationshipKind,
    RelationshipReplacement, Resource, WaChangelogEdit, WaEdit, WaMediaUpload, WaShareCode,
    WaUpdate,
};
use serde_json::{Value, json};
use std::{num::NonZeroU64, path::PathBuf};

pub(super) enum Request {
    BackupRead(BackupRead),
    BackupScan(PathBuf),
    BackupGenerate(BackupGenerate),
    BackupUpload {
        request: BackupUpload,
        recover_id: Option<NonZeroU64>,
    },
    BackupCreate {
        request: BackupCreate,
        recover_id: Option<NonZeroU64>,
    },
    BackupRename(BackupRename),
    BackupDelete(BackupDelete),
    Read(Read),
    Create {
        request: AuthorCreate,
        recover_id: Option<NonZeroU64>,
    },
    Delete(AuthorDelete),
    PluginEdit(PluginEdit),
    PluginUpdate(PluginUpdate),
    ConfigEdit(ConfigEdit),
    ConfigUpdate(ConfigUpdate),
    WaEdit(WaEdit),
    WaUpdate(WaUpdate),
    PluginChangelog(PluginChangelogEdit),
    WaChangelog(WaChangelogEdit),
    Relationships(RelationshipReplacement),
    ShareCode(WaShareCode),
    Media(WaMediaUpload),
    Plan(Value),
}
fn resource(value: &str) -> Result<Resource> {
    match value {
        "plugin" => Ok(Resource::Plugin),
        "config" => Ok(Resource::Config),
        "wa" => Ok(Resource::Wa),
        _ => Err(ToolkitError::arguments("select plugin, config or wa")),
    }
}
fn id(inv: &Invocation, flag: &str) -> Result<NonZeroU64> {
    Ok(NonZeroU64::new(positive(inv, flag)?).unwrap())
}
fn natural(inv: &Invocation, flag: &str, default: u64) -> Result<u64> {
    inv.flag(flag)
        .map(|value| {
            value.parse::<u64>().map_err(|_| {
                ToolkitError::arguments(format!("{flag} requires a nonnegative integer"))
            })
        })
        .unwrap_or(Ok(default))
}
fn page(inv: &Invocation) -> Result<Page> {
    Page::new(
        u32::try_from(natural(inv, "--page", 1)?)
            .map_err(|_| ToolkitError::arguments("--page exceeds its supported range"))?,
        size(inv)?,
    )
}
fn size(inv: &Invocation) -> Result<u32> {
    let size = natural(inv, "--size", 20)?;
    if !(1..=100).contains(&size) {
        return Err(ToolkitError::arguments("--size requires 1..100"));
    }
    Ok(size as u32)
}
pub(super) fn admit(inv: &Invocation) -> Result<Request> {
    let tail = inv.command.strip_prefix("publish newbee ").unwrap();
    let words = tail.split_whitespace().collect::<Vec<_>>();
    let bytes = inv.flag("--input").map(|_| read_input(inv)).transpose()?;
    let input = || {
        bytes
            .as_deref()
            .ok_or_else(|| ToolkitError::arguments("--input is required"))
    };
    let keyword = || inv.flag("--keyword").unwrap_or("").to_owned();
    let request = match words.as_slice() {
        ["backup", "list"] => Request::BackupRead(BackupRead::List),
        ["backup", "get"] => Request::BackupRead(BackupRead::Get(id(inv, "--id")?)),
        ["backup", "scan"] => Request::BackupScan(inv.cwd.join(inv.required("--game-dir")?)),
        ["backup", "generate"] => {
            Request::BackupGenerate(BackupGenerate::parse(input()?, &inv.cwd)?)
        }
        ["backup", "upload"] => Request::BackupUpload {
            request: BackupUpload::parse(input()?, &inv.cwd)?,
            recover_id: inv.flag("--id").map(|_| id(inv, "--id")).transpose()?,
        },
        ["backup", "create"] => Request::BackupCreate {
            request: BackupCreate::parse(input()?, &inv.cwd)?,
            recover_id: inv.flag("--id").map(|_| id(inv, "--id")).transpose()?,
        },
        ["backup", "rename"] => Request::BackupRename(BackupRename::parse(input()?)?),
        ["backup", "delete"] => Request::BackupDelete(BackupDelete::parse(input()?)?),
        ["backup", "delete-preview"] => {
            let ids = inv
                .flag("--ids")
                .map(|text| {
                    text.split(',')
                        .map(|text| {
                            text.trim()
                                .parse::<u64>()
                                .ok()
                                .and_then(NonZeroU64::new)
                                .ok_or_else(|| {
                                    ToolkitError::arguments(
                                        "--ids requires comma-separated unique positive backup IDs",
                                    )
                                })
                        })
                        .collect::<Result<Vec<_>>>()
                })
                .transpose()?
                .unwrap_or_default();
            Request::BackupRead(BackupRead::DeletePreview(BackupSelection::new(
                ids,
                inv.flag("--name-contains").map(str::to_owned),
                inv.flag("--name-equals").map(str::to_owned),
                inv.flag("--created-before"),
                inv.flag("--created-after"),
            )?))
        }
        ["session", "doctor"] => Request::Read(Read::Doctor),
        ["options", kind] => Request::Read(Read::Options(match *kind {
            "content-origins" => OptionKind::ContentOrigins,
            "subscribe-plans" => OptionKind::SubscribePlans,
            "time-ranges" => OptionKind::TimeRanges,
            _ => return Err(ToolkitError::arguments("unknown NewBeeBox option group")),
        })),
        ["plugin", "categories"] => Request::Read(Read::PluginCategories),
        ["plugin", "game-versions"] => Request::Read(Read::GameVersions),
        ["plugin", "list"] => Request::Read(Read::PluginList {
            keyword: keyword(),
            page: page(inv)?,
        }),
        ["config", "list"] => Request::Read(Read::ConfigList {
            keyword: keyword(),
            offset: natural(inv, "--offset", 0)?,
            size: size(inv)?,
        }),
        ["wa", "list"] => Request::Read(Read::WaList {
            keyword: keyword(),
            offset: natural(inv, "--offset", 0)?,
            size: size(inv)?,
        }),
        [kind, "get"] => Request::Read(Read::Get {
            resource: resource(kind)?,
            id: id(inv, "--id")?,
        }),
        ["config", "backups"] => Request::Read(Read::ConfigBackups),
        ["config", "backup-get"] => Request::Read(Read::ConfigBackup {
            id: id(inv, "--id")?,
        }),
        ["plugin", "versions"] => Request::Read(Read::PluginVersions {
            id: id(inv, "--id")?,
            page: page(inv)?,
        }),
        ["plugin", "changelog", "list"] => Request::Read(Read::PluginChangelogs {
            id: id(inv, "--id")?,
            page: page(inv)?,
        }),
        ["plugin", "changelog", "get"] => Request::Read(Read::PluginChangelog {
            id: id(inv, "--file-id")?,
        }),
        ["wa", "categories"] => Request::Read(Read::WaCategories {
            game_version: id(inv, "--game-version-id")?,
        }),
        ["wa", "attachment-paths"] => Request::Read(Read::WaAttachmentPaths),
        ["wa", "changelog", "latest"] => Request::Read(Read::WaLatest {
            id: id(inv, "--id")?,
        }),
        ["wa", "changelog", "list"] => Request::Read(Read::WaChangelogs {
            id: id(inv, "--id")?,
            page: page(inv)?,
        }),
        [kind, "co-author", "search"] => {
            resource(kind)?;
            Request::Read(Read::CoauthorSearch { keyword: keyword() })
        }
        [kind, "co-author", "list"] => Request::Read(Read::Coauthors {
            resource: resource(kind)?,
            id: id(inv, "--id")?,
        }),
        [kind, "reference", "search"] => Request::Read(Read::ReferenceSearch {
            resource: resource(kind)?,
            keyword: keyword(),
        }),
        [kind, "reference", "list"] => Request::Read(Read::References {
            resource: resource(kind)?,
            id: id(inv, "--id")?,
        }),
        [kind, "create"] => Request::Create {
            request: AuthorCreate::parse(resource(kind)?, input()?, &inv.cwd)?,
            recover_id: inv.flag("--id").map(|_| id(inv, "--id")).transpose()?,
        },
        [kind, "delete"] => Request::Delete(AuthorDelete::parse(resource(kind)?, input()?)?),
        ["plugin", "edit"] => Request::PluginEdit(PluginEdit::parse(input()?, &inv.cwd)?),
        ["plugin", "update"] => Request::PluginUpdate(PluginUpdate::parse(input()?, &inv.cwd)?),
        ["config", "edit"] => Request::ConfigEdit(ConfigEdit::parse(input()?, &inv.cwd)?),
        ["config", "update"] => Request::ConfigUpdate(ConfigUpdate::parse(input()?, &inv.cwd)?),
        ["wa", "edit"] => Request::WaEdit(WaEdit::parse(input()?, &inv.cwd)?),
        ["wa", "update"] => Request::WaUpdate(WaUpdate::parse(input()?)?),
        ["plugin", "changelog", "edit"] => {
            Request::PluginChangelog(PluginChangelogEdit::parse(input()?)?)
        }
        ["wa", "changelog", "edit"] => Request::WaChangelog(WaChangelogEdit::parse(input()?)?),
        [kind, "co-author", "set"] => Request::Relationships(RelationshipReplacement::parse(
            resource(kind)?,
            RelationshipKind::Coauthors,
            input()?,
        )?),
        [kind, "reference", "set"] => Request::Relationships(RelationshipReplacement::parse(
            resource(kind)?,
            RelationshipKind::References,
            input()?,
        )?),
        ["wa", "share-code", "set"] => Request::ShareCode(WaShareCode::parse(input()?)?),
        ["wa", "media", "upload"] => Request::Media(WaMediaUpload::parse(input()?, &inv.cwd)?),
        _ => return Err(ToolkitError::arguments("unknown NewBeeBox author command")),
    };
    if inv.flag_bool("--plan") {
        if inv.flag("--id").is_some() {
            return Err(ToolkitError::arguments(
                "--plan admits a new local input; recover an existing creation without --plan",
            ));
        }
        let local = match &request {
            Request::BackupUpload { request, .. } => Some(request.plan(&inv.control)?),
            Request::BackupCreate { request, .. } => Some(request.plan(&inv.control)?),
            Request::BackupGenerate(request) => Some(request.plan(&inv.control)?),
            _ => None,
        };
        return Ok(Request::Plan(match local {
            Some(value) => value,
            None => {
                json!({"provider":"newbee","command":inv.command,"validation":"local_input","remote_validation":"not_run","author_writes":0,"credential_reads":false,"input":newbee::input_summary(input()?)?})
            }
        }));
    }
    Ok(request)
}
pub(super) fn dispatch(inv: &Invocation, request: Request) -> Result<Value> {
    if let Request::Plan(value) = request {
        return Ok(value);
    }
    let request = match request {
        Request::BackupScan(path) => return newbee::scan_backup(&path, &inv.control),
        Request::BackupGenerate(request) => return request.generate(&inv.control),
        request => request,
    };
    let mut provider = Newbee::open(&inv.home, &inv.control)?;
    match request {
        Request::BackupRead(request) => provider.read_backup(&request),
        Request::BackupUpload {
            request,
            recover_id,
        } => provider.upload_backup(&request, recover_id),
        Request::BackupCreate {
            request,
            recover_id,
        } => provider.create_backup(&request, recover_id),
        Request::BackupRename(request) => provider.rename_backup(&request),
        Request::BackupDelete(request) => provider.delete_backups(&request),
        Request::BackupScan(_) | Request::BackupGenerate(_) => unreachable!(),
        Request::Read(request) => provider.read(&request),
        Request::Create {
            request,
            recover_id,
        } => provider.create(&request, recover_id),
        Request::Delete(request) => provider.delete(&request),
        Request::PluginEdit(request) => provider.edit_plugin(&request),
        Request::PluginUpdate(request) => provider.update_plugin(&request),
        Request::ConfigEdit(request) => provider.edit_config(&request),
        Request::ConfigUpdate(request) => provider.update_config(&request),
        Request::WaEdit(request) => provider.edit_wa(&request),
        Request::WaUpdate(request) => provider.update_wa(&request),
        Request::PluginChangelog(request) => provider.edit_plugin_changelog(&request),
        Request::WaChangelog(request) => provider.edit_wa_changelog(&request),
        Request::Relationships(request) => provider.replace_relationships(&request),
        Request::ShareCode(request) => provider.set_wa_share_code(&request),
        Request::Media(request) => provider.upload_wa_media(&request),
        Request::Plan(_) => unreachable!(),
    }
}
pub(super) fn file_input(path: &str) -> Option<Value> {
    let words = path
        .strip_prefix("publish newbee ")?
        .split_whitespace()
        .collect::<Vec<_>>();
    let schema = match words.as_slice() {
        ["backup", "generate"] => BackupGenerate::schema(),
        ["backup", "upload"] => BackupUpload::schema(),
        ["backup", "create"] => BackupCreate::schema(),
        ["backup", "rename"] => BackupRename::schema(),
        ["backup", "delete"] => BackupDelete::schema(),
        [kind, "create"] => AuthorCreate::schema(resource(kind).ok()?),
        [kind, "delete"] => AuthorDelete::schema(resource(kind).ok()?),
        ["plugin", "edit"] => PluginEdit::schema(),
        ["plugin", "update"] => PluginUpdate::schema(),
        ["config", "edit"] => ConfigEdit::schema(),
        ["config", "update"] => ConfigUpdate::schema(),
        ["wa", "edit"] => WaEdit::schema(),
        ["wa", "update"] => WaUpdate::schema(),
        ["plugin", "changelog", "edit"] => PluginChangelogEdit::schema(),
        ["wa", "changelog", "edit"] => WaChangelogEdit::schema(),
        [kind, "co-author", "set"] => {
            RelationshipReplacement::schema(resource(kind).ok()?, RelationshipKind::Coauthors)
        }
        [kind, "reference", "set"] => {
            RelationshipReplacement::schema(resource(kind).ok()?, RelationshipKind::References)
        }
        ["wa", "share-code", "set"] => WaShareCode::schema(),
        ["wa", "media", "upload"] => WaMediaUpload::schema(),
        _ => return None,
    };
    let mut example = match words.as_slice() {
        ["backup", "generate"] => {
            json!({"game_dir":"game","output_dir":"backup","game_version_id":1,"unknown_plugins":["AuthorAddon"]})
        }
        ["backup", "upload" | "create"] => json!({"bundle_file":"backup/backup-bundle.json"}),
        ["backup", "rename"] => json!({"id":719,"name":"Author backup"}),
        ["backup", "delete"] => json!({"backup_ids":[719],"confirm_delete":true}),
        ["plugin", "create"] => {
            json!({"name":"Author addon","mod_categories":[1],"content_origin":1,"content_format":2,"intro":"Addon introduction","description":"Addon description","public":false,"logo_file":"image.png","screenshot_files":["image.png"]})
        }
        ["plugin", "update"] => {
            json!({"mod_id":719,"file":"AuthorAddon.zip","game_version_list":["REPLACE_WITH_CURRENT_GAME_VERSION"],"version":"1.0.0"})
        }
        ["plugin", "edit"] => json!({"id":719,"name":"Updated addon"}),
        ["plugin", "changelog", "edit"] => {
            json!({"file_id":720,"changelog":"Updated release notes"})
        }
        ["config", "create"] => {
            json!({"cloud_id":719,"title":"Author configuration","content":"Configuration description","content_format":2,"content_origin":1,"public":false,"linked_mods":[],"ignored_unknown_mods":[],"ignored_materials":[],"ignored_fronts":[],"roleid":"","picture_files":["image.png"]})
        }
        ["config", "update"] => json!({"id":719,"roleid":""}),
        ["config", "edit"] => json!({"id":719,"title":"Updated configuration"}),
        ["wa", "create"] => {
            json!({"game_version_id":1,"name":"Author aura","description":"Aura description","content_format":2,"category_id_list":[1],"content_origin":1,"public":false,"thumbnail_file":"image.png","wa_str":"!WA:2!REPLACE_WITH_YOUR_STRING","wa_log":"Initial version","string_mode":"single"})
        }
        ["wa", "update"] => {
            json!({"id":719,"wa_str":"!WA:2!REPLACE_WITH_YOUR_STRING","wa_log":"Updated version"})
        }
        ["wa", "edit"] => json!({"id":719,"name":"Updated aura"}),
        ["wa", "changelog", "edit"] => json!({"id":720,"wa_log":"Updated release notes"}),
        ["wa", "share-code", "set"] => json!({"module_id":719}),
        ["wa", "media", "upload"] => json!({"kind":"image","file":"image.png"}),
        [_, "delete"] => json!({"id":719,"confirm":"DELETE"}),
        [_, "co-author", "set"] => {
            json!({"content_id":719,"co_authors":[{"user_id":720,"share_percent":0.5}]})
        }
        [_, "reference", "set"] => json!({"source_id":719,"references":[{"type":1,"id":720}]}),
        _ => unreachable!(),
    };
    example["schema"] = schema["properties"]["schema"]["const"].clone();
    Some(
        json!({"flag":"--input","description":"One UTF-8 JSON object from a file or '-' for piped stdin through EOF, at most 32 MiB. Paths resolve from the invocation directory. Replace example IDs and game versions using the field's current option/query command, and provide your selected files/content. --plan checks local field, nested and presence admission with no credential reads, network or submission records; current remote choices and existing complete forms are validated on execution. Credentials and opaque imports never belong in argv.","schema":schema,"example":example}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture(std::path::PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root = Self(std::env::temp_dir().join(format!(
                    "lychee-newbee-plan-{}-{}",
                    std::process::id(),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_nanos()
                )));
            std::fs::create_dir_all(&root.0).unwrap();
            root
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }
    fn inv(words: &[&str]) -> Invocation {
        crate::parse(
            &words
                .iter()
                .map(|word| word.to_string())
                .collect::<Vec<_>>(),
        )
        .unwrap()
    }
    fn schema(path: &str) -> Value {
        let mut words = vec!["schema".to_owned()];
        words.extend(path.split_whitespace().map(str::to_owned));
        crate::dispatch(&crate::parse(&words).unwrap())
            .unwrap()
            .data
    }
    #[test]
    fn local_wa_plan_redacts_opaque_input_and_leaves_the_profile_absent() {
        let root = Fixture::new();
        let file = root.0.join("input.json");
        let opaque = "!WA:2!PRIVATE_PLAN_CONTENT";
        std::fs::write(&file,serde_json::to_vec(&json!({"schema":"lycheedev.publish.v1.newbee.wa.update","id":719,"wa_str":opaque,"wa_log":"Changes"})).unwrap()).unwrap();
        let mut call = inv(&[
            "publish",
            "newbee",
            "wa",
            "update",
            "--input",
            "input.json",
            "--plan",
        ]);
        call.cwd = root.0.clone();
        call.home = root.0.join("absent-home");
        let request = admit(&call).unwrap();
        assert!(
            matches!(request, Request::Plan(_)),
            "only a local plan may be executed by this offline test"
        );
        let result = dispatch(&call, request).unwrap();
        assert_eq!(result["validation"], "local_input");
        assert_eq!(result["remote_validation"], "not_run");
        assert_eq!(result["credential_reads"], false);
        assert_eq!(result["author_writes"], 0);
        assert!(!result.to_string().contains(opaque));
        assert!(result["input"]["wa_str"]["sha256"].is_string());
        assert!(!call.home.exists());
        std::fs::write(&file,br#"{"schema":"lycheedev.publish.v1.newbee.wa.update","id":719,"id":720,"wa_str":"private-duplicate","wa_log":"Changes"}"#).unwrap();
        let error = admit(&call).err().unwrap();
        assert_eq!(error.exit_code, 4);
        assert!(
            !serde_json::to_string(&error)
                .unwrap()
                .contains("private-duplicate")
        );
        assert!(!call.home.exists());
    }
    #[test]
    fn public_backup_scan_generate_and_upload_plans_use_the_local_bundle_without_a_profile() {
        let root = Fixture::new();
        std::fs::create_dir_all(root.0.join("game/Interface/AddOns/Selected")).unwrap();
        std::fs::write(
            root.0.join("game/Interface/AddOns/Selected/Selected.lua"),
            b"selected bytes",
        )
        .unwrap();
        let input = root.0.join("input.json");
        std::fs::write(&input,serde_json::to_vec(&json!({"schema":"lycheedev.publish.v1.newbee.backup.generate","game_dir":"game","output_dir":"bundle","game_version_id":2,"unknown_plugins":["Selected"]})).unwrap()).unwrap();
        let call = |action: &str, flags: &[&str]| {
            let mut args = vec!["publish", "newbee", "backup", action];
            args.extend_from_slice(flags);
            let mut call = inv(&args);
            call.cwd = root.0.clone();
            call.home = root.0.join("absent-home");
            call
        };
        let planned = crate::dispatch(&call("generate", &["--input", "input.json", "--plan"]))
            .unwrap()
            .data;
        assert_eq!(planned["local_writes"], 0);
        assert!(!root.0.join("bundle").exists());
        let scan = crate::dispatch(&call("scan", &["--game-dir", "game"]))
            .unwrap()
            .data;
        assert_eq!(scan["unknown_plugins"], json!([{"name":"Selected"}]));
        let generated = crate::dispatch(&call("generate", &["--input", "input.json"]))
            .unwrap()
            .data;
        assert_eq!(generated["remote_writes"], 0);
        for action in ["upload", "create"] {
            std::fs::write(&input,serde_json::to_vec(&json!({"schema":format!("lycheedev.publish.v1.newbee.backup.{action}"),"bundle_file":"bundle/backup-bundle.json"})).unwrap()).unwrap();
            let result = crate::dispatch(&call(action, &["--input", "input.json", "--plan"]))
                .unwrap()
                .data;
            assert_eq!(result["archives"], 1);
            assert_eq!(result["credential_reads"], false);
            assert_eq!(result["validation"], "local_bundle_and_archive_selection");
        }
        assert!(!root.0.join("absent-home").exists());
        let create = schema("publish newbee backup create");
        assert_eq!(
            create["fileInput"]["schema"]["oneOf"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            create["fileInput"]["schema"]["properties"]["wtf_accounts"]["items"]["additionalProperties"],
            false
        );
        assert_eq!(
            schema("publish newbee backup scan")["effects"]["readOnly"],
            true
        );
        std::fs::write(
            root.0.join("bundle/unknown_plug.zip"),
            b"changed after generation",
        )
        .unwrap();
        assert_eq!(
            crate::dispatch(&call("create", &["--input", "input.json", "--plan"]))
                .err()
                .unwrap()
                .exit_code,
            4
        );
    }
    #[test]
    fn published_creator_examples_enter_public_plans_without_a_profile() {
        let root = Fixture::new();
        std::fs::write(root.0.join("image.png"), b"selected media").unwrap();
        std::fs::create_dir_all(root.0.join("game/Interface/AddOns/AuthorAddon")).unwrap();
        std::fs::write(
            root.0
                .join("game/Interface/AddOns/AuthorAddon/AuthorAddon.toc"),
            b"## Interface: 120000\n",
        )
        .unwrap();
        let mut definitions = crate::contract::all().iter().collect::<Vec<_>>();
        definitions.sort_by_key(|definition| definition.path != "publish newbee backup generate");
        for definition in definitions {
            let Some(input) = file_input(&definition.path) else {
                continue;
            };
            let doc = &input["example"];
            std::fs::write(root.0.join("input.json"), serde_json::to_vec(doc).unwrap()).unwrap();
            let mut args = definition.path.split_whitespace().collect::<Vec<_>>();
            args.extend(["--input", "input.json", "--plan"]);
            let mut call = inv(&args);
            call.cwd = root.0.clone();
            call.home = root.0.join("absent-home");
            let result = crate::dispatch(&call)
                .unwrap_or_else(|error| panic!("{}: {error:?}", definition.path))
                .data;
            assert_eq!(result["credential_reads"], false, "{}", definition.path);
            assert_eq!(result["author_writes"], 0, "{}", definition.path);
            assert!(!call.home.exists());
            if definition.path == "publish newbee backup generate" {
                assert!(!root.0.join("backup").exists());
                args.pop(); // Generate only after the published plan succeeds.
                let mut generate = inv(&args);
                generate.cwd = root.0.clone();
                generate.home = call.home;
                crate::dispatch(&generate).unwrap();
                std::fs::copy(
                    root.0.join("backup/unknown_plug.zip"),
                    root.0.join("AuthorAddon.zip"),
                )
                .unwrap();
            }
        }
    }
    #[test]
    fn invalid_newbee_ids_and_page_limits_fail_before_session_selection() {
        for words in [
            vec!["publish", "newbee", "backup", "delete-preview"],
            vec![
                "publish",
                "newbee",
                "backup",
                "delete-preview",
                "--created-before",
                "invalid",
            ],
            vec![
                "publish",
                "newbee",
                "backup",
                "delete-preview",
                "--ids",
                "7,7",
            ],
            vec![
                "publish",
                "newbee",
                "backup",
                "delete-preview",
                "--ids",
                "0",
            ],
            vec![
                "publish",
                "newbee",
                "wa",
                "categories",
                "--game-version-id",
                "0",
            ],
            vec!["publish", "newbee", "config", "list", "--size", "101"],
            vec![
                "publish", "newbee", "plugin", "versions", "--id", "719", "--page", "0",
            ],
            vec![
                "publish",
                "newbee",
                "plugin",
                "get",
                "--id",
                "18446744073709551616",
            ],
        ] {
            assert_eq!(admit(&inv(&words)).err().unwrap().exit_code, 2);
        }
    }
}
