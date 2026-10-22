//! Static documents for the real owned Function/call backend; no semantic mock.
#![allow(dead_code)]
#[path = "query_backend_cases.rs"]
mod query;
use emmylua_code_analysis::EmmyLuaAnalysis;
use emmylua_parser::{LuaAstNode, LuaCallExpr, LuaClosureExpr};
use lycheedev_emmy_adapter::session::*;
pub use query::*;

pub fn bind_function(provider: &mut Provider, target: BindingSite, origin: BindingSite) {
    provider.values.insert(
        target,
        BindingDecision::Bound(BoundValue {
            identity: "selected-function".into(),
            proof: "selected-function-proof".into(),
            value: SelectedValue::Function(FunctionRef {
                file_id: origin.file_id,
                closure: origin.syntax,
                declaration: None,
            }),
        }),
    );
}

pub fn load_function(
    source: &str,
    text: &str,
) -> (EmmyLuaAnalysis, BindingSite, BindingSite, Provider) {
    let (analysis, site, mut provider) = load(source, text);
    let model = analysis.semantic_model(site.file_id);
    let root = model.syntax_tree().unwrap().get_red_root();
    let closure = root.descendants().find_map(LuaClosureExpr::cast).unwrap();
    let origin = BindingSite {
        file_id: site.file_id,
        syntax: closure.get_syntax_id(),
    };
    let target = site
        .syntax
        .to_node_from_root(&root)
        .and_then(LuaCallExpr::cast)
        .map(|call| BindingSite {
            file_id: site.file_id,
            syntax: call.get_prefix_expr().unwrap().get_syntax_id(),
        })
        .unwrap_or(site);
    bind_function(&mut provider, target, origin);
    (analysis, site, origin, provider)
}

pub fn ordinary_call_bodies() -> [&'static str; 8] {
    [
        "",
        "return",
        "return 4",
        "return nil",
        "return true",
        "return 'text'",
        "return 1,nil,3",
        "local function nested() return 99 end; return 7",
    ]
}
