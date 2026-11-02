// Adapted from EmmyLuaAnalyzer (MIT), upstream 08b0873962e83bb8307e9a81052e3f41578fbc78.
// Original: crates/emmylua_code_analysis/src/check/checker/unused.rs
// See THIRD_PARTY_NOTICES.md for the original license and attribution.
//! unused: local variables never referenced after declaration.

use emmylua_code_analysis::DiagnosticCode;
use emmylua_code_analysis::SemanticModel;

use super::{CheckContext, Checker};

pub struct UnusedChecker;

impl Checker for UnusedChecker {
    const CODES: &[DiagnosticCode] = &[DiagnosticCode::Unused];

    fn check(context: &mut CheckContext<'_>, semantic_model: &SemanticModel<'_>) {
        let Some(decls) = semantic_model.decls() else {
            return;
        };
        for decl in decls {
            // Only local variables are checked; `_` prefix is treated as intentional.
            if !decl.kind.is_local() || decl.name.starts_with('_') {
                continue;
            }
            if semantic_model.decl_references(&decl.id).is_empty() {
                if decl.is_implicit_self {
                    context.add_diagnostic(
                        DiagnosticCode::Unused,
                        decl.name_range,
                        t!("Implicit self is never used, if this is intentional, please use '.' instead of ':' to define the method"),
                    );
                } else {
                    context.add_diagnostic(
                        DiagnosticCode::Unused,
                        decl.name_range,
                        t!("%{name} is never used, if this is intentional, prefix it with an underscore: _%{name}", name = decl.name),
                    );
                }
            }
        }
    }
}
