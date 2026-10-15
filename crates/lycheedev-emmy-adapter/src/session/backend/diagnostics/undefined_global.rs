// Adapted from EmmyLuaAnalyzer (MIT), upstream 08b0873962e83bb8307e9a81052e3f41578fbc78.
// Original: crates/emmylua_code_analysis/src/check/checker/undefined_global.rs
// See THIRD_PARTY_NOTICES.md for the original license and attribution.
//! undefined_global: references names that are neither local, global, nor built-in.

use emmylua_parser::LuaSyntaxKind;

use emmylua_code_analysis::DiagnosticCode;
use emmylua_code_analysis::SemanticId;
use emmylua_code_analysis::SemanticModel;

use super::builtin::is_builtin_global;
use super::{CheckContext, Checker};
pub struct UndefinedGlobal;

impl Checker for UndefinedGlobal {
    const CODES: &[DiagnosticCode] = &[DiagnosticCode::UndefinedGlobal];

    fn check(context: &mut CheckContext<'_>, semantic_model: &SemanticModel<'_>) {
        let Some(uses) = semantic_model.name_uses() else {
            return;
        };
        let tree = semantic_model.syntax_tree();
        let root = tree.map(|t| t.get_red_root());
        for use_ in uses {
            // A definition position (assignment target, parent is AssignStat) is not a reference; skip it.
            if let Some(root) = &root
                && let Some(node) = use_.syntax.to_node_from_root(root)
                && node
                    .parent()
                    .is_some_and(|parent| parent.kind() == LuaSyntaxKind::AssignStat.into())
            {
                continue;
            }
            // Already resolved to a local declaration.
            if semantic_model
                .resolve_name(use_.syntax.get_range().start())
                .is_some()
            {
                continue;
            }
            // Lua built-in global.
            if is_builtin_global(use_.name.as_str()) {
                continue;
            }
            // `emmyrc.diagnostics.globals` / `globals_regex` whitelist.
            if context.is_global_disabled(use_.name.as_str()) {
                continue;
            }
            // NameUse is extracted exclusively from LuaNameExpr's name token,
            // so this is a bare identifier, never a dotted member path. The
            // upstream resolve_owner(Name) bare-name branch only reads global
            // type/declaration indexes; it does not select or infer a runtime
            // value. Keep the original index precedence and existence test.
            let owner = SemanticId::name(use_.name.clone());
            if semantic_model.resolve_owner(&owner).is_some() {
                continue;
            }
            context.add_diagnostic(
                DiagnosticCode::UndefinedGlobal,
                use_.syntax.get_range(),
                t!("undefined global variable: `%{name}`", name = use_.name),
            );
        }
    }
}
