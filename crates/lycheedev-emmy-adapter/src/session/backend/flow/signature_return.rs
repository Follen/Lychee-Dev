//! Guard reachability uses the non-contextual signature-shell producer, not the
//! contextual closure-return VM. Selected call/callee ownership short-circuits
//! this producer exactly at the original two manages checks.
use super::*;
use crate::session::BindingDecision;
use emmylua_parser::{LuaCallExpr, LuaClosureExpr};
impl<E> OwnedQuery<'_, '_, E> {
    pub(super) fn flow_signature_return(
        &mut self,
        call: &LuaCallExpr,
    ) -> QueryResult<Option<OwnedValue>, E> {
        let site = self.site(call.get_syntax_id());
        self.checkpoint()?;
        let managed = !matches!(
            self.session.context().call(site),
            BindingDecision::NotManaged
        );
        self.checkpoint()?;
        let managed = if managed {
            true
        } else {
            let managed = !matches!(
                self.session.context().resolve(site),
                BindingDecision::NotManaged
            );
            self.checkpoint()?;
            managed
        };
        if managed {
            return Ok(None);
        }
        let callee = call.get_prefix_expr();
        if let Some(callee) = &callee {
            self.checkpoint()?;
            let managed = !matches!(
                self.session
                    .context()
                    .resolve(self.site(callee.get_syntax_id())),
                BindingDecision::NotManaged
            );
            self.checkpoint()?;
            if managed {
                return Ok(None);
            }
        }
        let Some(LuaExpr::NameExpr(name)) = callee else {
            return Ok(None);
        };
        let Some(mut declaration) = self.model.resolve_name(name.get_position()) else {
            return Ok(None);
        };
        let mut visited = HashSet::new();
        loop {
            self.checkpoint()?;
            let SemanticId::Decl(key) = &declaration else {
                return Ok(None);
            };
            let file = key.file_id;
            let Some(facts) = self.model.file_facts_of(file) else {
                return Ok(None);
            };
            let Some(syntax) = facts
                .decl_by_id(&declaration)
                .and_then(|decl| decl.value_expr_syntax)
            else {
                return Ok(None);
            };
            let Some(node) = self
                .model
                .syntax_tree_of(file)
                .and_then(|tree| syntax.to_node_from_root(&tree.get_red_root()))
            else {
                return Ok(None);
            };
            if LuaClosureExpr::cast(node.clone()).is_some() {
                if facts.signature_by_closure(syntax).is_none() {
                    return Ok(None);
                }
                return self.raw_signature_return(file, syntax);
            }
            let Some(LuaExpr::NameExpr(alias)) = LuaExpr::cast(node) else {
                return Ok(None);
            };
            // Preserve the fixed model's current-file alias lookup quirk.
            let Some(next) = self.model.resolve_name(alias.get_position()) else {
                return Ok(None);
            };
            if !visited.insert(next.clone()) {
                return Ok(None);
            }
            declaration = next;
        }
    }
}
