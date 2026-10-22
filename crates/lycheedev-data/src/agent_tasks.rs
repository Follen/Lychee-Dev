//! Task templates compile to the one production typed engine. No rows are read
//! here. Raw-record bundles preserve evidence and the domain tasks provide full presentations.
use crate::{
    engine::{Expression, QueryPlan, SourceMode, TableRequest},
    fault,
};
use lycheedev_core::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum BusinessTask {
    BatchIds {
        table: TableRequest,
        ids: Vec<u32>,
    },
    Related {
        table: TableRequest,
        field: String,
        ids: Vec<u32>,
    },
    Navigate {
        template: NavigationTemplate,
        ids: Vec<u32>,
        mode: SourceMode,
        #[serde(default)]
        hotfix_captures: Vec<String>,
    },
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NavigationTemplate {
    ItemRecords,
    SpellEffects,
    DecorRecords,
    CreatureDisplays,
}
pub struct Component {
    pub name: &'static str,
    pub plan: QueryPlan,
}
pub fn compile(task: BusinessTask) -> Result<Vec<Component>> {
    fn ids_valid(ids: &[u32]) -> Result<()> {
        if ids.is_empty() || ids.len() > 16384 {
            return Err(fault(
                "data.task.ids",
                2,
                "IDs must contain 1..16384 exact u32 values",
            ));
        }
        Ok(())
    }
    match task {
        BusinessTask::BatchIds { table, ids } => {
            ids_valid(&ids)?;
            Ok(vec![Component {
                name: "records",
                plan: QueryPlan::Lookup { table, ids },
            }])
        }
        BusinessTask::Related { table, field, ids } => {
            ids_valid(&ids)?;
            if field.is_empty() || field.len() > 256 {
                return Err(fault(
                    "data.task.field",
                    2,
                    "Field name must contain 1..256 bytes",
                ));
            }
            Ok(vec![Component {
                name: "related",
                plan: related(table, field, ids),
            }])
        }
        BusinessTask::Navigate {
            template,
            ids,
            mode,
            hotfix_captures,
        } => {
            ids_valid(&ids)?;
            let table = |name: &str| TableRequest {
                name: name.into(),
                mode: mode.clone(),
                hotfix_captures: hotfix_captures.clone(),
            };
            let lookup = |name, source| Component {
                name,
                plan: QueryPlan::Lookup {
                    table: table(source),
                    ids: ids.clone(),
                },
            };
            Ok(match template {
                NavigationTemplate::ItemRecords => {
                    vec![lookup("item", "Item"), lookup("localized", "ItemSparse")]
                }
                NavigationTemplate::SpellEffects => vec![Component {
                    name: "effects",
                    plan: related(table("SpellEffect"), "SpellID".into(), ids),
                }],
                NavigationTemplate::DecorRecords => vec![lookup("decor", "HouseDecor")],
                NavigationTemplate::CreatureDisplays => vec![
                    lookup("display", "CreatureDisplayInfo"),
                    Component {
                        name: "geosets",
                        plan: related(
                            table("CreatureDisplayInfoGeosetData"),
                            "CreatureDisplayInfoID".into(),
                            ids,
                        ),
                    },
                ],
            })
        }
    }
}
fn related(table: TableRequest, field: String, ids: Vec<u32>) -> QueryPlan {
    QueryPlan::Filter {
        input: Box::new(QueryPlan::Scan { table }),
        predicate: Expression::In {
            value: Box::new(Expression::Column { name: field }),
            values: ids.into_iter().map(serde_json::Value::from).collect(),
        },
    }
}
pub fn schema() -> serde_json::Value {
    use serde_json::{Value, json};
    fn variant(kind: &str, fields: Value, required: &[&str]) -> Value {
        let mut properties = fields.as_object().expect("literal schema object").clone();
        properties.insert("kind".into(), json!({"const":kind}));
        let mut required = required.to_vec();
        required.insert(0, "kind");
        json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
    }
    let query = crate::engine::query_schema();
    let table = query["$defs"]["TableRequest"].clone();
    let ids = json!({"type":"array","minItems":1,"maxItems":16384,"items":{"type":"integer","minimum":0,"maximum":4294967295u64},"description":"Exact ID membership; duplicates do not duplicate source records. Missing IDs yield no fabricated rows and absence requires complete source coverage."});
    json!({
        "$schema":"https://json-schema.org/draft/2020-12/schema",
        "title":"Lychee Agent business task request",
        "description":"BusinessTask object inside the business wire task request field. Templates return named raw-record evidence; navigate tasks provide domain reports. Every component has its own complete/provenance; handle publication is all-or-error. At most two components share the deadline and retained-memory admission; query_work is per component. No implicit row limit.",
        "oneOf":[
            variant("batch_ids",json!({"table":{"$ref":"#/$defs/TableRequest"},"ids":ids}), &["table","ids"]),
            variant("related",json!({"table":{"$ref":"#/$defs/TableRequest"},"field":{"type":"string","minLength":1,"maxLength":256,"description":"Exact column name; runtime additionally limits UTF-8 byte count to 256 and validates the Engine column grammar."},"ids":ids}), &["table","field","ids"]),
            variant("navigate",json!({"template":{"enum":["item_records","spell_effects","decor_records","creature_displays"]},"ids":ids,"mode":{"enum":["static","effective"]},"hotfix_captures":table["properties"]["hotfix_captures"]}), &["template","ids","mode"])
        ],
        "$defs":{"TableRequest":table},
        "examples":[{"kind":"navigate","template":"item_records","ids":[19019],"mode":"static"}],
        "x-wire-example":{"protocol":"lycheedev.data-agent.v1","id":1,"task":"business","request":{"kind":"navigate","template":"item_records","ids":[19019],"mode":"static"}}
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn table() -> TableRequest {
        TableRequest {
            name: "Sample".into(),
            mode: SourceMode::Static,
            hotfix_captures: vec![],
        }
    }
    #[test]
    fn batch_bound_matches_typed_engine_and_never_silently_slices() {
        assert!(
            compile(BusinessTask::BatchIds {
                table: table(),
                ids: vec![1; 16384]
            })
            .is_ok()
        );
        assert!(
            compile(BusinessTask::BatchIds {
                table: table(),
                ids: vec![1; 16385]
            })
            .is_err()
        );
        assert!(
            compile(BusinessTask::BatchIds {
                table: table(),
                ids: vec![]
            })
            .is_err()
        );
    }
    #[test]
    fn schema_examples_decode_and_variants_require_discriminator() {
        let schema = schema();
        for example in schema["examples"].as_array().unwrap() {
            let task: BusinessTask = serde_json::from_value(example.clone()).unwrap();
            assert!(compile(task).is_ok());
        }
        for variant in schema["oneOf"].as_array().unwrap() {
            assert_eq!(variant["additionalProperties"], false);
            assert!(
                variant["required"]
                    .as_array()
                    .unwrap()
                    .contains(&serde_json::json!("kind"))
            );
            assert!(variant["properties"]["kind"]["const"].is_string());
            let maximum = &variant["properties"]["ids"]["items"]["maximum"];
            assert_eq!(maximum.as_u64(), Some(u64::from(u32::MAX)));
            assert_eq!(serde_json::to_string(maximum).unwrap(), "4294967295");
        }
    }
    #[test]
    fn business_wire_rejects_unknown_tasks_and_fields() {
        assert!(
            serde_json::from_value::<BusinessTask>(
                serde_json::json!({"kind":"sql","sql":"select 1"})
            )
            .is_err()
        );
        assert!(serde_json::from_value::<BusinessTask>(serde_json::json!({"kind":"batch_ids","table":{"name":"Sample","mode":"static"},"ids":[1],"unknown":true})).is_err());
    }
}
