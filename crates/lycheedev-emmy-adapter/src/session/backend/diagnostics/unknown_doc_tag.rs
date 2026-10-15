// Adapted from EmmyLuaAnalyzer (MIT), upstream 08b0873962e83bb8307e9a81052e3f41578fbc78.
// Original: crates/emmylua_code_analysis/src/check/checker/unknown_doc_tag.rs
// See THIRD_PARTY_NOTICES.md for the original license and attribution.
//! unknown_doc_tag: checks for unknown doc annotations (`---@foobar`).
//!
//! Mirrors the old `diagnostic::checker::unknown_doc_tag`: reports a `LuaDocTagOther` tag token
//! if it is not in `emmyrc.doc.known_tags`. Disabled by default (`is_code_default_enable` -> false).

use emmylua_parser::{LuaAstNode, LuaAstToken, LuaDocTagOther, LuaTokenKind};

use emmylua_code_analysis::DiagnosticCode;
use emmylua_code_analysis::SemanticModel;

use super::{CheckContext, Checker};

pub struct UnknownDocTagChecker;

impl Checker for UnknownDocTagChecker {
    const CODES: &[DiagnosticCode] = &[
        DiagnosticCode::UndefinedDocParam,
        DiagnosticCode::UnknownDocTag,
    ];

    fn check(context: &mut CheckContext<'_>, semantic_model: &SemanticModel<'_>) {
        let Some(tree) = semantic_model.syntax_tree() else {
            return;
        };
        let root = tree.get_red_root();
        for tag_other in root.descendants().filter_map(LuaDocTagOther::cast) {
            check_tag(context, semantic_model, &tag_other);
        }
    }
}

fn check_tag(
    context: &mut CheckContext<'_>,
    semantic_model: &SemanticModel<'_>,
    tag_other: &LuaDocTagOther,
) {
    if let Some(token) = tag_other.token_by_kind(LuaTokenKind::TkTagOther)
        && !semantic_model.is_known_doc_tag(token.get_text())
    {
        context.add_diagnostic_with_data(
            DiagnosticCode::UnknownDocTag,
            token.get_range(),
            t!("Unknown doc tag: `%{name}`", name = token.get_text()).to_string(),
            Some(serde_json::Value::String(token.get_text().to_string())),
        );
    }
}
