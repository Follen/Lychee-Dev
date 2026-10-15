// Adapted from EmmyLuaAnalyzer (MIT), upstream 08b0873962e83bb8307e9a81052e3f41578fbc78.
// Original: crates/emmylua_code_analysis/src/check/checker/global_non_module.rs
// See THIRD_PARTY_NOTICES.md for the original license and attribution.
//! # global_non_module - global variable definitions outside module top level (inside closures/nested blocks)
//!
//! M0: when an assignment target cannot be resolved to a local declaration (i.e. a global definition) and is inside a nested block/closure -> report
//! `GlobalInNonModule`。

use emmylua_parser::{LuaAssignStat, LuaAst, LuaAstNode, LuaBlock, LuaVarExpr};

use emmylua_code_analysis::DeclKind;
use emmylua_code_analysis::DiagnosticCode;
use emmylua_code_analysis::SemanticModel;

use super::{CheckContext, Checker};

pub struct GlobalInNonModuleChecker;

impl Checker for GlobalInNonModuleChecker {
    const CODES: &[DiagnosticCode] = &[DiagnosticCode::GlobalInNonModule];

    fn check(context: &mut CheckContext<'_>, semantic_model: &SemanticModel<'_>) {
        let Some(tree) = semantic_model.syntax_tree() else {
            return;
        };
        let root = tree.get_red_root();
        for assign_stat in root.descendants().filter_map(LuaAssignStat::cast) {
            check_assign(context, semantic_model, &assign_stat);
        }
    }
}

fn check_assign(
    context: &mut CheckContext<'_>,
    semantic_model: &SemanticModel<'_>,
    assign_stat: &LuaAssignStat,
) {
    let (vars, _) = assign_stat.get_var_and_expr_list();
    for var in vars {
        let range = var.get_range();
        let LuaVarExpr::NameExpr(name_expr) = var else {
            continue;
        };
        // If the resolved result is not "a new global defined by this assignment", skip:
        // - Decl(local/param): assignment to a local variable;
        // - Decl(global) with declaration position before this assignment: reassignment of an existing global.
        if let Some(decl_id) = semantic_model.resolve_name(name_expr.get_position())
            && let Some(facts) = semantic_model.file_facts()
            && let Some(decl) = facts.decl_by_id(&decl_id)
        {
            match decl.kind {
                DeclKind::Local { .. } | DeclKind::Param => continue,
                DeclKind::Global => {
                    if decl.name_range.start() < range.start() {
                        continue;
                    }
                }
            }
        }
        // Inside a nested block / closure (not at chunk top level) -> report.
        if in_nested_scope(&name_expr) {
            context.add_diagnostic(
                DiagnosticCode::GlobalInNonModule,
                range,
                t!("Global variable should only be defined in module scope."),
            );
        }
    }
}

fn in_nested_scope(name_expr: &emmylua_parser::LuaNameExpr) -> bool {
    for block in name_expr.syntax().ancestors().filter_map(LuaBlock::cast) {
        match block.syntax().parent().and_then(LuaAst::cast) {
            Some(LuaAst::LuaChunk(_)) => return false,
            Some(LuaAst::LuaClosureExpr(_)) => return true,
            _ => {}
        }
    }
    false
}
