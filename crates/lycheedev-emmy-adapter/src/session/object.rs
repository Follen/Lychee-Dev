//! Selected object projection at the original producer positions.
use super::{
    BindingCoverage, BindingDecision, BindingSite, FieldPresence, ObjectOrigin, ObjectRef,
    OwnedValue, ProjectionResult, SelectedExpressionBackend, SelectedProjector, ShapeEnumeration,
};
use emmylua_code_analysis::{LuaMemberKey, LuaType};
use std::collections::HashSet;

impl SelectedProjector<'_> {
    pub(crate) fn project_owned_object<B: SelectedExpressionBackend<Value = OwnedValue>>(
        &self,
        site: BindingSite,
        object: &ObjectRef,
        backend: &mut B,
    ) -> ProjectionResult<OwnedValue, B::Error> {
        backend.checkpoint()?;
        if object.view.context != self.session().identity()
            || object.identity.is_empty()
            || object.epoch.is_empty()
        {
            return Ok(Err("object_context_or_epoch_mismatch"));
        }
        match &object.origin {
            ObjectOrigin::Lua(origin) => {
                let model = self.analysis().semantic_model(origin.file_id);
                let valid = model
                    .syntax_tree()
                    .and_then(|tree| origin.syntax.to_node_from_root(&tree.get_red_root()))
                    .is_some();
                backend.checkpoint()?;
                if !valid {
                    return Ok(Err("object_source_origin_missing"));
                }
            }
            ObjectOrigin::Xml(origin) => {
                if origin.path.is_empty() || origin.content_sha256.len() != 64 {
                    return Ok(Err("object_external_origin_invalid"));
                }
            }
        }
        let Some(_guard) = self
            .session()
            .enter_object(object.view.clone(), || backend.checkpoint())?
        else {
            return Ok(Err("object_projection_cycle_or_depth"));
        };
        let frozen = match self
            .session()
            .freeze_shape(object, || backend.checkpoint())?
        {
            Ok(frozen) => frozen,
            Err(reason) => return Ok(Err(reason)),
        };
        let mut fields = Vec::with_capacity(frozen.shape.fields.len());
        let mut seen = HashSet::new();
        let mut complete = matches!(frozen.shape.enumeration, ShapeEnumeration::Complete);
        for field in &frozen.shape.fields {
            backend.checkpoint()?;
            if !seen.insert(field.key.clone()) {
                return Ok(Err("object_duplicate_field"));
            }
            if field.presence == FieldPresence::Absent {
                if !matches!(field.value, BindingDecision::KnownAbsent { .. }) {
                    complete = false;
                }
                continue;
            }
            let value = if field.presence == FieldPresence::Unavailable {
                complete = false;
                backend.plain(LuaType::Unknown)
            } else {
                self.project_decision(site, field.value.clone(), "object_field", backend)?
                    .unwrap_or_else(|| backend.plain(LuaType::Unknown))
            };
            // Exact old direct Unknown test: Any or a nested incomplete object
            // does not by itself make the containing shape incomplete.
            if matches!(value.structural_type(), LuaType::Unknown) {
                complete = false;
            }
            fields.push((LuaMemberKey::Name(field.key.clone().into()), value));
        }
        self.session().record(
            BindingCoverage {
                site,
                operation: "object_shape",
                state: if complete { "bound" } else { "unavailable" },
                proof: frozen.shape.proof.clone(),
                reason: (!complete).then(|| "object_shape_incomplete".into()),
                parameter: None,
                parameter_index: None,
                argument: None,
            },
            || backend.checkpoint(),
        )?;
        backend.checkpoint()?;
        Ok(Ok(OwnedValue::object(frozen, site, complete, fields)))
    }
}
