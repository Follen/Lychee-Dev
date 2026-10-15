// Adapted from EmmyLuaAnalyzer (MIT), upstream 08b0873962e83bb8307e9a81052e3f41578fbc78.
// Original: crates/emmylua_code_analysis/src/check/checker/undefined_doc_param.rs
// See THIRD_PARTY_NOTICES.md for the original license and attribution.
//! undefined_doc_param: a name declared by `---@param` is not in the function's actual parameter list.

use emmylua_code_analysis::DiagnosticCode;
use emmylua_code_analysis::SemanticModel;

use super::{CheckContext, Checker};

pub struct UndefinedDocParamChecker;

impl Checker for UndefinedDocParamChecker {
    const CODES: &[DiagnosticCode] = &[DiagnosticCode::UndefinedDocParam];

    fn check(context: &mut CheckContext<'_>, semantic_model: &SemanticModel<'_>) {
        let Some(signatures) = semantic_model.signatures() else {
            return;
        };
        for signature in signatures {
            let Some(docs) = &signature.docs else {
                continue;
            };
            for (param_name, type_syntax) in &docs.param_types {
                if !signature.param_names.iter().any(|p| p == param_name) {
                    context.add_diagnostic(
                        DiagnosticCode::UndefinedDocParam,
                        type_syntax.get_range(),
                        t!("Undefined doc param: `%{name}`", name = param_name),
                    );
                }
            }
        }
    }
}
