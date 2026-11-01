use super::{positive, read_input};
use crate::Invocation;
use lycheedev_core::{Result, ToolkitError};
use lycheedev_publish::newbee_official::{
    AuthorList, ChangelogEdit, ConfigTextUpdate, MediaUpload, Official, PluginPack, PluginVersion,
    PluginWorkspace, Publication, Query, ReplyStatus, Resource, ReviewReply, TextProject,
    WaFileUpdate,
};
use serde_json::{Value, json};
use std::num::NonZeroU64;
pub(super) enum Request {
    PluginWorkspace(PluginWorkspace),
    TextProject(TextProject),
    Pack(PluginPack),
    PluginVersion {
        request: PluginVersion,
        recover_file: Option<NonZeroU64>,
    },
    Publication {
        request: Publication,
        recover_id: Option<NonZeroU64>,
    },
    ConfigText(ConfigTextUpdate),
    WaFile(WaFileUpdate),
    Review(ReviewReply),
    Changelog(ChangelogEdit),
    Query(Query),
    Media(MediaUpload),
    Plan(Value),
}
fn resource(value: &str) -> Result<Resource> {
    match value {
        "plugin" => Ok(Resource::Plugin),
        "wa" => Ok(Resource::Wa),
        "config" => Ok(Resource::Config),
        _ => Err(ToolkitError::arguments(
            "select official plugin, wa or config",
        )),
    }
}
fn id(inv: &Invocation, flag: &str) -> Result<NonZeroU64> {
    Ok(NonZeroU64::new(positive(inv, flag)?).unwrap())
}
fn page(inv: &Invocation) -> Result<NonZeroU64> {
    inv.flag("--page")
        .map(|_| id(inv, "--page"))
        .unwrap_or(Ok(NonZeroU64::new(1).unwrap()))
}
fn size(inv: &Invocation) -> Result<Option<u16>> {
    inv.flag("--size")
        .map(|_| {
            u16::try_from(positive(inv, "--size")?)
                .map_err(|_| ToolkitError::arguments("official plugin size must be 10..2000"))
        })
        .transpose()
}
pub(super) fn admit(inv: &Invocation) -> Result<Request> {
    let words = inv
        .command
        .strip_prefix("publish newbee official ")
        .unwrap()
        .split_whitespace()
        .collect::<Vec<_>>();
    let request = match words.as_slice() {
        ["plugin", action @ ("init" | "sync")] => {
            let request = PluginWorkspace::parse(
                *action == "init",
                &read_input(inv)?,
                &inv.cwd,
                &inv.control,
            )?;
            if inv.flag_bool("--plan") {
                return Ok(Request::Plan(request.plan(&inv.control)?));
            }
            Request::PluginWorkspace(request)
        }
        [kind @ ("wa" | "config"), action @ ("pull" | "init")] => {
            let request = TextProject::parse(
                resource(kind)?,
                *action == "init",
                &read_input(inv)?,
                &inv.cwd,
            )?;
            if inv.flag_bool("--plan") {
                return Ok(Request::Plan(request.plan()?));
            }
            Request::TextProject(request)
        }
        ["plugin", "pack"] => {
            let request = PluginPack::parse(&read_input(inv)?, &inv.cwd, &inv.control)?;
            if inv.flag_bool("--plan") {
                return Ok(Request::Plan(request.plan(&inv.control)?));
            }
            Request::Pack(request)
        }
        ["plugin", "version", "upload"] => {
            let request = PluginVersion::parse(&read_input(inv)?, &inv.cwd, &inv.control)?;
            let recover_file = inv
                .flag("--file-id")
                .map(|_| id(inv, "--file-id"))
                .transpose()?;
            if inv.flag_bool("--plan") {
                return Ok(Request::Plan(request.plan(&inv.control)?));
            }
            Request::PluginVersion {
                request,
                recover_file,
            }
        }
        [kind, action @ ("create" | "edit")] => {
            let resource = resource(kind)?;
            let create = *action == "create";
            let request = Publication::parse(resource, create, &read_input(inv)?, &inv.cwd)?;
            let recover_id = inv.flag("--id").map(|_| id(inv, "--id")).transpose()?;
            if inv.flag_bool("--plan") {
                return Ok(Request::Plan(request.plan(&inv.control)?));
            }
            Request::Publication {
                request,
                recover_id,
            }
        }
        ["config", "update"] => {
            let request = ConfigTextUpdate::parse(&read_input(inv)?, &inv.cwd)?;
            if inv.flag_bool("--plan") {
                return Ok(Request::Plan(request.plan(&inv.control)?));
            }
            Request::ConfigText(request)
        }
        ["wa", "update"] => {
            let request = WaFileUpdate::parse(&read_input(inv)?, &inv.cwd)?;
            if inv.flag_bool("--plan") {
                return Ok(Request::Plan(request.plan(&inv.control)?));
            }
            Request::WaFile(request)
        }
        ["plugin", "changelog", "edit"] => {
            let request = ChangelogEdit::parse(&read_input(inv)?)?;
            if inv.flag_bool("--plan") {
                return Ok(Request::Plan(request.plan()));
            }
            Request::Changelog(request)
        }
        [kind, "comments", "reply"] => {
            resource(kind)?;
            let request = ReviewReply::parse(&read_input(inv)?)?;
            if inv.flag_bool("--plan") {
                return Ok(Request::Plan(request.plan()));
            }
            Request::Review(request)
        }
        ["session", "doctor"] => Request::Query(Query::Doctor),
        ["backup", "list"] => Request::Query(Query::Backups),
        ["backup", "get"] => Request::Query(Query::Backup {
            id: id(inv, "--id")?,
        }),
        ["media", "upload"] => {
            let request = MediaUpload::parse(&read_input(inv)?, &inv.cwd)?;
            if inv.flag_bool("--plan") {
                return Ok(Request::Plan(request.plan(&inv.control)?));
            }
            Request::Media(request)
        }
        ["plugin", "versions"] => {
            let size = size(inv)?.unwrap_or(20);
            if !(10..=2000).contains(&size) {
                return Err(ToolkitError::arguments(
                    "official plugin version size must be 10..2000",
                ));
            }
            Request::Query(Query::PluginVersions {
                id: id(inv, "--id")?,
                page: page(inv)?,
                size,
            })
        }
        ["plugin", "changelog", "get"] => Request::Query(Query::PluginChangelog {
            file_id: id(inv, "--file-id")?,
        }),
        ["wa", "categories"] => Request::Query(Query::WaCategories {
            game_version: id(inv, "--game-version-id")?,
        }),
        [kind, "list"] => {
            if inv.flag_bool("--paid") && inv.flag_bool("--free") {
                return Err(ToolkitError::arguments("choose one of --paid or --free"));
            }
            let paid = if inv.flag_bool("--paid") {
                Some(true)
            } else if inv.flag_bool("--free") {
                Some(false)
            } else {
                None
            };
            Request::Query(Query::List(AuthorList::new(
                resource(kind)?,
                page(inv)?,
                size(inv)?,
                inv.flag("--keyword").unwrap_or("").to_owned(),
                paid,
            )?))
        }
        [kind, "get"] => Request::Query(Query::Detail {
            resource: resource(kind)?,
            id: id(inv, "--id")?,
        }),
        [kind, "comments", "list"] => {
            if inv.flag_bool("--unreplied") && inv.flag_bool("--replied") {
                return Err(ToolkitError::arguments(
                    "choose one of --unreplied or --replied",
                ));
            }
            let status = if inv.flag_bool("--unreplied") {
                ReplyStatus::Unreplied
            } else if inv.flag_bool("--replied") {
                ReplyStatus::Replied
            } else {
                ReplyStatus::All
            };
            Request::Query(Query::Comments {
                resource: resource(kind)?,
                id: id(inv, "--id")?,
                page: page(inv)?,
                status,
            })
        }
        [kind, "reference", "code"] => Request::Query(Query::Reference {
            resource: resource(kind)?,
            id: id(inv, "--id")?,
            link: inv.flag_bool("--link"),
            game_version: inv
                .flag("--game-version-id")
                .map(|_| id(inv, "--game-version-id"))
                .transpose()?,
        }),
        _ => {
            return Err(ToolkitError::arguments(
                "unknown official NewBee author command",
            ));
        }
    };
    Ok(request)
}
pub(super) fn dispatch(inv: &Invocation, request: Request) -> Result<Value> {
    if let Request::Plan(value) = request {
        return Ok(value);
    }
    if let Request::Pack(request) = request {
        return request.run(&inv.control);
    }
    let mut provider = Official::open(&inv.home, &inv.control)?;
    match request {
        Request::PluginWorkspace(request) => provider.plugin_workspace(&request),
        Request::TextProject(request) => provider.export_text_project(&request),
        Request::PluginVersion {
            request,
            recover_file,
        } => provider.upload_plugin_version(&request, recover_file),
        Request::Publication {
            request,
            recover_id,
        } => provider.publish(&request, recover_id),
        Request::ConfigText(request) => provider.update_config_text(&request),
        Request::WaFile(request) => provider.update_wa_file(&request),
        Request::Review(request) => provider.reply_review(&request),
        Request::Changelog(request) => provider.edit_changelog(&request),
        Request::Query(query) => provider.query(&query),
        Request::Media(request) => provider.upload_media(&request),
        Request::Plan(_) | Request::Pack(_) => unreachable!(),
    }
}
pub(super) fn file_input(path: &str) -> Option<Value> {
    let tail = path.strip_prefix("publish newbee official ")?;
    let schema = match tail {
        "plugin init" => PluginWorkspace::schema(true),
        "plugin sync" => PluginWorkspace::schema(false),
        "wa pull" => TextProject::schema(Resource::Wa, false),
        "wa init" => TextProject::schema(Resource::Wa, true),
        "config pull" => TextProject::schema(Resource::Config, false),
        "config init" => TextProject::schema(Resource::Config, true),
        "plugin pack" => PluginPack::schema(),
        "plugin version upload" => PluginVersion::schema(),
        "plugin create" => Publication::schema(Resource::Plugin, true),
        "plugin edit" => Publication::schema(Resource::Plugin, false),
        "wa create" => Publication::schema(Resource::Wa, true),
        "wa edit" => Publication::schema(Resource::Wa, false),
        "config create" => Publication::schema(Resource::Config, true),
        "media upload" => MediaUpload::schema(),
        "wa update" => WaFileUpdate::schema(),
        "config update" => ConfigTextUpdate::schema(),
        "plugin changelog edit" => ChangelogEdit::schema(),
        "plugin comments reply" | "wa comments reply" | "config comments reply" => {
            ReviewReply::schema()
        }
        _ => return None,
    };
    if tail == "plugin pack" {
        return Some(
            json!({"flag":"--input","description":"One UTF-8 JSON object from a file or '-' for piped stdin. Paths resolve from the invocation directory. Purely offline: uses no credentials, HTTP session, network or home. --plan validates selected files/layout without creating a ZIP; execution reuses an existing matching ZIP or publishes a new output without overwriting unrelated files.","schema":schema}),
        );
    }
    Some(
        json!({"flag":"--input","description":"One UTF-8 JSON document from a file or '-' for piped stdin through EOF, at most 32 MiB. Paths resolve from the invocation directory. --plan admits local fields and selected file bytes without credentials, network or home writes. Execution uses NCC_TOKEN or the official local ncc login; there is no desktop-token fallback.","schema":schema}),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    fn invocation(words: &[&str]) -> Invocation {
        crate::parse(
            &words
                .iter()
                .map(|word| word.to_string())
                .collect::<Vec<_>>(),
        )
        .unwrap()
    }
    #[test]
    fn official_queries_reject_invalid_filters_and_sizes_before_authentication() {
        for words in [
            vec!["plugin", "list", "--size", "9"],
            vec!["wa", "list", "--paid", "--free"],
            vec![
                "config",
                "comments",
                "list",
                "--id",
                "91",
                "--replied",
                "--unreplied",
            ],
            vec!["plugin", "versions", "--id", "91", "--size", "2001"],
        ] {
            let mut args = vec!["publish", "newbee", "official"];
            args.extend(words);
            assert_eq!(admit(&invocation(&args)).err().unwrap().exit_code, 2);
        }
        let schema = crate::dispatch(&invocation(&[
            "schema", "publish", "newbee", "official", "media", "upload",
        ]))
        .unwrap()
        .data;
        assert_eq!(
            schema["fileInput"]["schema"]["properties"]["resource"]["enum"],
            json!(["plugin", "wa", "config"])
        );
        assert_eq!(schema["effects"]["credentialStoreWrite"], false);
        assert_eq!(
            schema["effects"]["remoteWriteWhen"],
            json!({"--plan":false})
        );
    }
    #[test]
    fn official_author_plans_use_the_public_entry_without_opening_a_session_or_creating_home() {
        struct Fixture(std::path::PathBuf);
        impl Drop for Fixture {
            fn drop(&mut self) {
                std::fs::remove_dir_all(&self.0).unwrap();
            }
        }
        let root = Fixture(std::env::temp_dir().join(format!(
                "lychee-official-plan-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            )));
        std::fs::create_dir_all(&root.0).unwrap();
        std::fs::write(root.0.join("image.png"), b"fixture media").unwrap();
        std::fs::write(root.0.join("input.json"),br#"{"schema":"lycheedev.publish.v1.newbee.official.media.upload","resource":"wa","file":"image.png"}"#).unwrap();
        let mut call = invocation(&[
            "publish",
            "newbee",
            "official",
            "media",
            "upload",
            "--input",
            "input.json",
            "--plan",
        ]);
        call.cwd = root.0.clone();
        call.home = root.0.join("absent-home");
        let result = crate::dispatch(&call).unwrap().data;
        assert_eq!(result["credential_reads"], false);
        assert_eq!(result["author_writes"], 0);
        assert_eq!(result["resource"], "wa");
        assert_eq!(result["size"], 13);
        assert!(!call.home.exists());
        std::fs::write(root.0.join("wa.txt"), "!WA:2!PRIVATE_FILE_PLAN").unwrap();
        std::fs::write(root.0.join("body.md"), "PRIVATE_CONFIGURATION_PLAN").unwrap();
        std::fs::create_dir(root.0.join("Addon")).unwrap();
        std::fs::write(root.0.join("Addon/Addon.toc"), b"## Interface: 120000\n").unwrap();
        std::fs::create_dir(root.0.join("SyncAddon")).unwrap();
        std::fs::write(
            root.0.join("SyncAddon/SyncAddon.toc"),
            b"## Interface: 120000\n",
        )
        .unwrap();
        std::fs::write(
            root.0.join("SyncAddon/ncc.json"),
            br#"{"id":91,"version":"next","game_versions":["12.0.0"]}"#,
        )
        .unwrap();
        PluginPack::parse(br#"{"schema":"lycheedev.publish.v1.newbee.official.plugin.pack","dir":"Addon","version":"fixture","out":"fixture.zip"}"#,&root.0,&call.control).unwrap().run(&call.control).unwrap();
        for (tail, doc) in [
            (
                vec!["plugin", "init"],
                json!({"schema":"lycheedev.publish.v1.newbee.official.plugin.init","id":91,"dir":"Addon"}),
            ),
            (
                vec!["plugin", "sync"],
                json!({"schema":"lycheedev.publish.v1.newbee.official.plugin.sync","dir":"SyncAddon"}),
            ),
            (
                vec!["wa", "pull"],
                json!({"schema":"lycheedev.publish.v1.newbee.official.wa.pull","id":92,"out":"pull-wa.txt"}),
            ),
            (
                vec!["wa", "init"],
                json!({"schema":"lycheedev.publish.v1.newbee.official.wa.init","id":92,"dir":"new-wa-project"}),
            ),
            (
                vec!["config", "pull"],
                json!({"schema":"lycheedev.publish.v1.newbee.official.config.pull","id":99}),
            ),
            (
                vec!["config", "init"],
                json!({"schema":"lycheedev.publish.v1.newbee.official.config.init","id":99,"dir":"new-config-project"}),
            ),
            (
                vec!["plugin", "pack"],
                json!({"schema":"lycheedev.publish.v1.newbee.official.plugin.pack","dir":"Addon","version":"fixture","out":"plan-only.zip"}),
            ),
            (
                vec!["plugin", "version", "upload"],
                json!({"schema":"lycheedev.publish.v1.newbee.official.plugin-version.upload","id":91,"version":"fixture","game_versions":"auto","changelog":"PRIVATE_LOG","file":"fixture.zip"}),
            ),
            (
                vec!["plugin", "create"],
                json!({"schema":"lycheedev.publish.v1.newbee.official.plugin.create","name":"Fixture","description":{"file":"body.md"},"categories":[7],"logo":"image.png","screenshots":["image.png"],"visibility":"private"}),
            ),
            (
                vec!["plugin", "edit"],
                json!({"schema":"lycheedev.publish.v1.newbee.official.plugin.edit","id":91,"intro":""}),
            ),
            (
                vec!["wa", "create"],
                json!({"schema":"lycheedev.publish.v1.newbee.official.wa.create","name":"Fixture","description":{"file":"body.md"},"wa_str":{"file":"wa.txt"},"categories":[7],"game_version_id":2,"thumbnail":"image.png","visibility":"private","price_yuan":"1.005"}),
            ),
            (
                vec!["wa", "edit"],
                json!({"schema":"lycheedev.publish.v1.newbee.official.wa.edit","id":92,"images":[],"price_yuan":"0","link_channel":false}),
            ),
            (
                vec!["config", "create"],
                json!({"schema":"lycheedev.publish.v1.newbee.official.config.create","backup_id":88,"title":"Fixture","content":{"file":"body.md"},"visibility":"private"}),
            ),
            (
                vec!["config", "update"],
                json!({"schema":"lycheedev.publish.v1.newbee.official.config.update","id":91,"file":"body.md","intro":""}),
            ),
            (
                vec!["wa", "update"],
                json!({"schema":"lycheedev.publish.v1.newbee.official.wa.update","id":91,"file":"wa.txt","wa_log":"Changes"}),
            ),
            (
                vec!["plugin", "comments", "reply"],
                json!({"schema":"lycheedev.publish.v1.newbee.official.review.reply","review_id":9007199254740993u64,"content":"!WA:2!PRIVATE_REPLY_PLAN"}),
            ),
            (
                vec!["plugin", "changelog", "edit"],
                json!({"schema":"lycheedev.publish.v1.newbee.official.plugin-changelog.edit","file_id":991,"changelog":""}),
            ),
        ] {
            std::fs::write(root.0.join("input.json"), serde_json::to_vec(&doc).unwrap()).unwrap();
            let mut args = vec!["publish", "newbee", "official"];
            args.extend(tail);
            args.extend(["--input", "input.json", "--plan"]);
            let mut call = invocation(&args);
            call.cwd = root.0.clone();
            call.home = root.0.join("absent-home");
            let result = crate::dispatch(&call).unwrap().data;
            assert_eq!(result["credential_reads"], false);
            assert_eq!(result["author_writes"], 0);
            assert!(!result.to_string().contains("PRIVATE_"));
            assert!(!call.home.exists());
        }
    }
}
