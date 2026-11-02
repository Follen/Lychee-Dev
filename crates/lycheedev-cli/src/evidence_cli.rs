//! Command admission stays in the CLI; Core owns storage operations.
use crate::Invocation;
use lycheedev_core::{Result, ToolkitError};
use std::path::PathBuf;

fn parse_request(inv: &Invocation) -> Result<lycheedev_core::evidence::EvidenceRequest> {
    use lycheedev_core::evidence::EvidenceRequest as E;
    Ok(match inv.command.as_str() {
        "evidence show" => E::Show {
            id: inv.argument()?.into(),
        },
        "evidence verify" => E::Verify {
            id: inv.argument()?.into(),
        },
        "evidence list" => E::List {
            limit: inv.usize_flag("--limit", 100, 1, 1000)?,
        },
        "evidence keep" => E::Keep {
            id: inv.argument()?.into(),
        },
        "evidence remove" => E::Remove {
            id: inv.argument()?.into(),
        },
        "evidence bundle" => {
            let ids: Vec<String> = inv
                .required("--ids")?
                .split(',')
                .map(str::to_owned)
                .collect();
            let output = PathBuf::from(inv.required("--output")?);
            let output = if output.is_absolute() {
                output
            } else {
                inv.cwd.join(output)
            };
            E::Bundle { ids, output }
        }
        _ => return Err(ToolkitError::arguments("unknown evidence command")),
    })
}

pub(crate) fn dispatch(inv: &Invocation) -> Result<lycheedev_core::Outcome> {
    let ctx = crate::core_cli::context(inv);
    lycheedev_core::evidence::dispatch(&ctx, parse_request(inv)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_arguments_and_bad_limits_fail_before_storage() {
        for words in [
            vec!["evidence", "show"],
            vec!["evidence", "verify"],
            vec!["evidence", "keep"],
            vec!["evidence", "remove"],
            vec!["evidence", "list", "--limit", "0"],
            vec!["evidence", "list", "--limit", "1001"],
            vec!["evidence", "list", "--limit", "bad"],
            vec!["evidence", "bundle", "--ids", "CAP-x"],
            vec!["evidence", "bundle", "--output", "bundle.zip"],
        ] {
            let args = words
                .iter()
                .map(|word| word.to_string())
                .collect::<Vec<_>>();
            let error = crate::parse(&args)
                .and_then(|inv| parse_request(&inv))
                .unwrap_err();
            assert_eq!(
                (error.code.as_str(), error.exit_code),
                ("command.invalid_arguments", 2)
            );
        }
    }
    #[test]
    fn bundle_admission_freezes_output_against_invocation_directory() {
        let mut inv = crate::parse(
            &[
                "evidence",
                "bundle",
                "--ids",
                "CAP-a,CAP-b",
                "--output",
                "../proof.zip",
            ]
            .map(str::to_owned),
        )
        .unwrap();
        inv.cwd = PathBuf::from(r"D:\investigation\nested");
        let request = parse_request(&inv).unwrap();
        assert_eq!(
            request,
            lycheedev_core::evidence::EvidenceRequest::Bundle {
                ids: vec!["CAP-a".into(), "CAP-b".into()],
                output: inv.cwd.join("../proof.zip"),
            }
        );
    }
}
