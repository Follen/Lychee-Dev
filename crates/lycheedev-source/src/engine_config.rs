//! Configuration policy at Source's engine-host boundary. Authentication,
//! control and error priority remain at SourceInputs::admit.
use emmylua_code_analysis::{DiagnosticCode, Emmyrc, EmmyrcLuaVersion};

pub(crate) fn research() -> Emmyrc {
    lycheedev_emmy_adapter::lua51_config()
}
pub(crate) fn addon_diagnostics() -> Emmyrc {
    let mut config = research();
    // Preserve the Source command's selected eight-code policy. The complete
    // owned registry also honors explicit file-level enables for other codes.
    config.diagnostics.disable = DiagnosticCode::all()
        .into_iter()
        .filter(|code| {
            !matches!(
                code,
                DiagnosticCode::ParamTypeMismatch
                    | DiagnosticCode::ReturnTypeMismatch
                    | DiagnosticCode::MissingParameter
                    | DiagnosticCode::RedundantParameter
                    | DiagnosticCode::CallNonCallable
                    | DiagnosticCode::MissingReturnValue
                    | DiagnosticCode::RedundantReturnValue
                    | DiagnosticCode::MissingReturn
            )
        })
        .collect();
    config.diagnostics.enables = vec![
        DiagnosticCode::ParamTypeMismatch,
        DiagnosticCode::ReturnTypeMismatch,
        DiagnosticCode::MissingParameter,
        DiagnosticCode::RedundantParameter,
        DiagnosticCode::CallNonCallable,
        DiagnosticCode::MissingReturnValue,
        DiagnosticCode::RedundantReturnValue,
        DiagnosticCode::MissingReturn,
    ];
    config
}

pub(crate) fn explicit_authenticated_inputs(config: &Emmyrc) -> bool {
    config.workspace.library.is_empty()
        && config.workspace.packages.is_empty()
        && config.workspace.workspace_roots.is_empty()
        && config.workspace.ignore_dir.is_empty()
        && config.workspace.ignore_globs.is_empty()
        && config.workspace.module_map.is_empty()
        && config.workspace.encoding == "utf-8"
        && config.runtime.version == EmmyrcLuaVersion::Lua51
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn diagnostic_config_preserves_full_prior_json_and_order() {
        let mut config = Emmyrc::default();
        config.runtime.version = EmmyrcLuaVersion::Lua51;
        // Preserve the Source command's selected eight-code policy. The complete
        // owned registry also honors explicit file-level enables for other codes.
        config.diagnostics.disable = DiagnosticCode::all()
            .into_iter()
            .filter(|code| {
                !matches!(
                    code,
                    DiagnosticCode::ParamTypeMismatch
                        | DiagnosticCode::ReturnTypeMismatch
                        | DiagnosticCode::MissingParameter
                        | DiagnosticCode::RedundantParameter
                        | DiagnosticCode::CallNonCallable
                        | DiagnosticCode::MissingReturnValue
                        | DiagnosticCode::RedundantReturnValue
                        | DiagnosticCode::MissingReturn
                )
            })
            .collect();
        config.diagnostics.enables = vec![
            DiagnosticCode::ParamTypeMismatch,
            DiagnosticCode::ReturnTypeMismatch,
            DiagnosticCode::MissingParameter,
            DiagnosticCode::RedundantParameter,
            DiagnosticCode::CallNonCallable,
            DiagnosticCode::MissingReturnValue,
            DiagnosticCode::RedundantReturnValue,
            DiagnosticCode::MissingReturn,
        ];
        let original = serde_json::to_vec(&config).unwrap();
        assert_eq!(serde_json::to_vec(&addon_diagnostics()).unwrap(), original);
        assert_eq!(
            crate::native::hash(&original),
            crate::native::hash(&serde_json::to_vec(&addon_diagnostics()).unwrap())
        );
    }
    #[test]
    fn research_configuration_and_admission_keep_original_identity() {
        let mut previous = Emmyrc::default();
        previous.runtime.version = EmmyrcLuaVersion::Lua51;
        assert_eq!(
            serde_json::to_vec(&research()).unwrap(),
            serde_json::to_vec(&previous).unwrap()
        );
        assert!(explicit_authenticated_inputs(&research()));
        assert!(explicit_authenticated_inputs(&addon_diagnostics()));
        for (field, value) in [
            ("library", serde_json::json!(["external"])),
            ("packages", serde_json::json!(["external"])),
            (
                "moduleMap",
                serde_json::json!([{"pattern": ".*", "replace": "external"}]),
            ),
        ] {
            let mut encoded = serde_json::to_value(research()).unwrap();
            encoded["workspace"][field] = value;
            let invalid: Emmyrc = serde_json::from_value(encoded).unwrap();
            assert!(!explicit_authenticated_inputs(&invalid), "{field}");
        }
        let mut invalid = research();
        invalid.workspace.encoding = "latin1".into();
        assert!(!explicit_authenticated_inputs(&invalid));
        let mut invalid = research();
        invalid.workspace.workspace_roots.push("external".into());
        assert!(!explicit_authenticated_inputs(&invalid));
        let mut invalid = research();
        invalid.workspace.ignore_dir.push("ignored".into());
        assert!(!explicit_authenticated_inputs(&invalid));
        let mut invalid = research();
        invalid.workspace.ignore_globs.push("*.lua".into());
        assert!(!explicit_authenticated_inputs(&invalid));
    }
}
