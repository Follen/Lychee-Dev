//! Asset command admission. Domain operations receive admitted typed requests.
use crate::Invocation;
use lycheedev_core::{Result, ToolkitError};
use lycheedev_data::{
    assets::request::{
        AssetRequest, DemuxInput, DemuxRequest, Encoding, ExportRequest, SearchMode, SearchRequest,
    },
    request::DataContext,
};

fn file_id(inv: &Invocation) -> Result<u32> {
    inv.flag("--file-id")
        .unwrap_or("0")
        .parse::<u32>()
        .map_err(|_| ToolkitError::arguments("invalid --file-id"))
}
fn number(inv: &Invocation, flag: &str) -> Result<usize> {
    inv.flag(flag)
        .unwrap_or("0")
        .parse::<usize>()
        .map_err(|_| ToolkitError::arguments(format!("invalid {flag}")))
}

pub(crate) fn request(inv: &Invocation) -> Result<Option<(DataContext, AssetRequest)>> {
    if !matches!(
        inv.command.as_str(),
        "asset inspect" | "asset search" | "asset export" | "asset demux"
    ) {
        return Ok(None);
    }
    inv.control.check()?;
    let source = crate::data_cli::source_options(inv)?;
    let request = match inv.command.as_str() {
        "asset inspect" => {
            let file_id = inv
                .required("--file-id")?
                .parse::<u32>()
                .ok()
                .filter(|id| *id != 0)
                .ok_or_else(|| {
                    ToolkitError::new(
                        "command.invalid_arguments",
                        "File ID must be a positive u32",
                        2,
                    )
                })?;
            AssetRequest::Inspect { file_id }
        }
        "asset search" => {
            let listfile = inv.flag("--listfile").unwrap_or("");
            let query = inv.flag("--query").filter(|value| !value.is_empty());
            let extension = inv.flag("--extension").filter(|value| !value.is_empty());
            let name = inv.flag("--name").filter(|value| !value.is_empty());
            let id = file_id(inv)?;
            if listfile.is_empty()
                || [
                    query.is_some(),
                    extension.is_some(),
                    name.is_some(),
                    id != 0,
                ]
                .into_iter()
                .filter(|yes| *yes)
                .count()
                    != 1
            {
                return Err(ToolkitError::arguments(
                    "asset search requires --listfile and exactly one of --query, --extension, --name, --file-id; select a snapshot or locked project",
                ));
            }
            let mode = if let Some(query) = query {
                SearchMode::Query(query.into())
            } else if let Some(extension) = extension {
                SearchMode::Extension(extension.into())
            } else if let Some(name) = name {
                SearchMode::Name(name.into())
            } else {
                SearchMode::FileId(id)
            };
            AssetRequest::Search(SearchRequest {
                listfile: listfile.into(),
                mode,
                limit: inv.usize_flag("--limit", 50, 1, 200)?,
                maximum_bytes: if inv.flag("--max-bytes").is_some() {
                    source.max_file_bytes
                } else {
                    256 << 20
                },
            })
        }
        "asset export" => {
            if source.cdn && source.installation.is_some() {
                return Err(ToolkitError::arguments(
                    "choose either --cdn or --installation, not both",
                ));
            }
            let file_id = file_id(inv)?;
            if (!source.cdn
                && source
                    .installation
                    .as_ref()
                    .filter(|value| !value.as_os_str().is_empty())
                    .is_none())
                || file_id == 0
            {
                return Err(ToolkitError::arguments(
                    "asset export requires --file-id and either --installation or --cdn; select a snapshot or locked project",
                ));
            }
            let output = inv
                .flag("--output")
                .filter(|value| !value.is_empty())
                .ok_or_else(|| ToolkitError::arguments("asset export requires --output"))?;
            let encoding = inv
                .flag("--encoding")
                .filter(|value| !value.is_empty())
                .unwrap_or("raw");
            let mipmap = number(inv, "--mipmap")?;
            let pixels = number(inv, "--max-pixels")?;
            let channels = inv.flag("--channels").unwrap_or("");
            if (encoding == "raw" && (mipmap != 0 || !channels.is_empty() || pixels != 0))
                || !["raw", "png", "webp"].contains(&encoding)
                || mipmap > 15
                || (encoding != "raw" && pixels > 64 << 20)
                || channels.chars().any(|name| !"rgba".contains(name))
                || channels
                    .chars()
                    .any(|name| channels.matches(name).count() > 1)
            {
                return Err(ToolkitError::new(
                    "records.export_options",
                    "records.export_options",
                    2,
                ));
            }
            AssetRequest::Export(ExportRequest {
                file_id,
                output: crate::data_cli::resolve_path(inv, output),
                encoding: match encoding {
                    "png" => Encoding::Png,
                    "webp" => Encoding::Webp,
                    _ => Encoding::Raw,
                },
                mipmap,
                channels: if channels.is_empty() {
                    "rgba".into()
                } else {
                    "rgba"
                        .chars()
                        .filter(|name| channels.contains(*name))
                        .collect()
                },
                max_pixels: if pixels == 0 { 16 << 20 } else { pixels },
                maximum_bytes: source.max_file_bytes,
                overwrite: inv.flag_bool("--overwrite"),
            })
        }
        "asset demux" => {
            let path = inv.flag("--path").filter(|value| !value.is_empty());
            let id = file_id(inv)?;
            let output = inv.flag("--output").filter(|value| !value.is_empty());
            if output.is_none()
                || path.is_some() == (id != 0)
                || (path.is_none() && !source.cdn && source.installation.is_none())
                || (path.is_some()
                    && (source.snapshot.is_some() || source.cdn || source.installation.is_some()))
            {
                return Err(ToolkitError::arguments(
                    "asset demux requires --output and exactly one input: --path, or --file-id with --installation or --cdn and a snapshot or locked project",
                ));
            }
            if path.is_none() && source.cdn && source.installation.is_some() {
                return Err(ToolkitError::arguments(
                    "choose either --cdn or --installation, not both",
                ));
            }
            if source.max_file_bytes < 12 {
                return Err(ToolkitError::new(
                    "records.demux_input_limit",
                    "records.demux_input_limit",
                    3,
                ));
            }
            AssetRequest::Demux(DemuxRequest {
                input: match path {
                    Some(path) => DemuxInput::Path(crate::data_cli::resolve_path(inv, path)),
                    None => DemuxInput::Casc { file_id: id },
                },
                output: crate::data_cli::resolve_path(inv, output.expect("admitted output")),
                maximum_bytes: source.max_file_bytes,
                max_frames: inv.usize_flag("--max-frames", 100000, 1, 1 << 24)?,
                allow_partial: inv.flag_bool("--allow-partial"),
                overwrite: inv.flag_bool("--overwrite"),
            })
        }
        _ => unreachable!(),
    };
    Ok(Some((crate::data_cli::context(inv, source), request)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    fn invocation(command: &str) -> Invocation {
        Invocation {
            command: command.into(),
            positional: None,
            flags: BTreeMap::from([
                ("--snapshot".into(), "PIN-invalid-but-present".into()),
                ("--cdn".into(), "true".into()),
                ("--file-id".into(), "7".into()),
                ("--output".into(), "unused-output".into()),
            ]),
            home: "unopened-asset-workspace".into(),
            cwd: "unopened-asset-workspace".into(),
            control: Default::default(),
        }
    }
    #[test]
    fn export_zero_id_and_missing_source_are_arguments_errors() {
        for (flag, value) in [("--file-id", "0"), ("--cdn", "false")] {
            let mut inv = invocation("asset export");
            inv.flags.insert(flag.into(), value.into());
            let error = request(&inv).err().unwrap();
            assert_eq!(error.exit_code, 2);
            assert!(error.message.contains("requires --file-id"), "{error:?}");
        }
        let mut inv = invocation("asset export");
        inv.flags.insert("--file-id".into(), "4294967296".into());
        assert_eq!(request(&inv).err().unwrap().message, "invalid --file-id");
        inv.flags.insert("--file-id".into(), "7".into());
        inv.flags.insert("--installation".into(), "source".into());
        assert!(
            request(&inv)
                .err()
                .unwrap()
                .message
                .contains("choose either")
        );
    }
    #[test]
    fn export_and_casc_demux_admit_project_selection_without_a_snapshot_flag() {
        for command in ["asset export", "asset demux"] {
            let mut inv = invocation(command);
            inv.flags.remove("--snapshot");
            let (ctx, _) = request(&inv).unwrap().unwrap();
            assert!(ctx.source.snapshot.is_none());
            assert!(ctx.source.project.is_none());
            inv.flags
                .insert("--project".into(), "selected-project".into());
            let (ctx, _) = request(&inv).unwrap().unwrap();
            assert_eq!(ctx.source.project.as_deref(), Some("selected-project"));
        }
    }
    #[test]
    fn image_option_admission_and_original_stop_precede_destination_open() {
        for (flag, value) in [
            ("--channels", "r"),
            ("--encoding", "jpeg"),
            ("--mipmap", "16"),
        ] {
            let mut inv = invocation("asset export");
            inv.flags.insert(flag.into(), value.into());
            assert_eq!(request(&inv).err().unwrap().code, "records.export_options");
        }
        let mut inv = invocation("asset export");
        inv.flags.insert("--encoding".into(), "png".into());
        inv.flags.insert("--channels".into(), "rr".into());
        assert_eq!(request(&inv).err().unwrap().code, "records.export_options");
        inv.control.tracked_cancel_handle().unwrap().cancel();
        let stopped = inv.control.check().unwrap_err();
        let actual = request(&inv).err().unwrap();
        assert_eq!(actual.code, stopped.code);
        assert_eq!(actual.exit_code, stopped.exit_code);
        assert_eq!(actual.details, stopped.details);
    }
    #[test]
    fn content_allowance_arguments_reject_outside_public_range() {
        for maximum in ["0", "536870913", "-1", "bad"] {
            let mut inv = invocation("asset inspect");
            inv.flags.insert("--max-bytes".into(), maximum.into());
            assert_eq!(request(&inv).err().unwrap().exit_code, 2);
        }
    }
    #[test]
    fn typed_search_demux_and_image_requests_keep_all_options() {
        let mut inv = invocation("asset search");
        inv.flags
            .insert("--listfile".into(), "community-csv".into());
        let (_, AssetRequest::Search(search)) = request(&inv).unwrap().unwrap() else {
            panic!("search")
        };
        assert!(matches!(search.mode, SearchMode::FileId(7)));
        assert_eq!(search.maximum_bytes, 256 << 20);
        inv.flags.insert("--query".into(), "blp".into());
        assert_eq!(request(&inv).err().unwrap().exit_code, 2);
        let mut inv = invocation("asset demux");
        inv.flags.clear();
        inv.flags.insert("--path".into(), "movie.avi".into());
        inv.flags.insert("--output".into(), "frames".into());
        inv.flags.insert("--allow-partial".into(), "true".into());
        inv.flags.insert("--max-frames".into(), "1".into());
        let (_, AssetRequest::Demux(demux)) = request(&inv).unwrap().unwrap() else {
            panic!("demux")
        };
        assert!(matches!(demux.input, DemuxInput::Path(_)));
        assert!(demux.allow_partial);
        assert_eq!(demux.max_frames, 1);
        inv.flags.insert("--file-id".into(), "7".into());
        assert_eq!(request(&inv).err().unwrap().exit_code, 2);
        let mut inv = invocation("asset export");
        inv.flags.insert("--encoding".into(), "webp".into());
        inv.flags.insert("--channels".into(), "abgr".into());
        inv.flags.insert("--mipmap".into(), "1".into());
        let (ctx, AssetRequest::Export(export)) = request(&inv).unwrap().unwrap() else {
            panic!("export")
        };
        assert_eq!(export.encoding, Encoding::Webp);
        assert_eq!(export.channels, "rgba");
        assert_eq!(export.mipmap, 1);
        assert_eq!(ctx.source.max_file_bytes, export.maximum_bytes);
    }
}
