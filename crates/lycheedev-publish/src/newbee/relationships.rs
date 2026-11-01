use super::*;
use crate::{
    contract::input_shape,
    input::{document, invalid},
    mutation::Mutation,
    provider::Provider,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Copy)]
pub enum RelationshipKind {
    Coauthors,
    References,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Coauthor {
    user_id: NonZeroU64,
    share_percent: serde_json::Number,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Reference {
    #[serde(rename = "type")]
    kind: u8,
    id: NonZeroU64,
}
enum Items {
    Coauthors(Vec<Coauthor>),
    References(Vec<Reference>),
}
pub struct RelationshipReplacement {
    resource: Resource,
    id: NonZeroU64,
    items: Items,
}
impl RelationshipReplacement {
    pub fn schema(resource: Resource, kind: RelationshipKind) -> Value {
        let (suffix, id_field) = match kind {
            RelationshipKind::Coauthors => ("co-author", "content_id"),
            RelationshipKind::References => ("reference", "source_id"),
        };
        let mut schema = input_shape(
            Provider::Newbee,
            &format!("{}-{suffix}", resource.key()),
            "set",
        )
        .unwrap()
        .schema();
        schema.as_object_mut().unwrap().remove("x-validation-scope");
        schema["properties"][id_field]["minimum"] = json!(1);
        plugin::relationship_schemas(&mut schema["properties"]);
        plugin::metadata_schema(&mut schema);
        schema
    }
    pub fn parse(resource: Resource, kind: RelationshipKind, bytes: &[u8]) -> Result<Self> {
        let value = document(bytes)?;
        let suffix = match kind {
            RelationshipKind::Coauthors => "co-author",
            RelationshipKind::References => "reference",
        };
        input_shape(
            Provider::Newbee,
            &format!("{}-{suffix}", resource.key()),
            "set",
        )?
        .admit(&value)?;
        let id_field = match kind {
            RelationshipKind::Coauthors => "content_id",
            RelationshipKind::References => "source_id",
        };
        let id = value[id_field]
            .as_u64()
            .and_then(NonZeroU64::new)
            .ok_or_else(|| {
                invalid(
                    &format!("$.{id_field}"),
                    "resource identity must be a positive integer",
                )
            })?;
        let items = match kind {
            RelationshipKind::Coauthors => {
                if value["co_authors"].as_array().is_some_and(|authors| {
                    authors
                        .iter()
                        .any(|author| !author.get("share_percent").is_some_and(Value::is_number))
                }) {
                    return Err(invalid(
                        "$.co_authors",
                        "share_percent must be a JSON number",
                    ));
                }
                let authors:Vec<Coauthor>=serde_json::from_value(value["co_authors"].clone()).map_err(|_|invalid("$.co_authors","co-authors require user_id and numeric share_percent with no unknown fields"))?;
                let mut ids = BTreeSet::new();
                let mut total = 0.0;
                for author in &authors {
                    let share = author
                        .share_percent
                        .as_f64()
                        .filter(|share| share.is_finite() && *share > 0.0 && *share <= 1.0)
                        .ok_or_else(|| invalid("$.co_authors", "share_percent must be in (0,1]"))?;
                    if !ids.insert(author.user_id) {
                        return Err(invalid("$.co_authors", "co-author user IDs must be unique"));
                    }
                    total += share;
                }
                if total > 1.000001 {
                    return Err(invalid(
                        "$.co_authors",
                        "co-author shares must not total more than 1",
                    ));
                }
                Items::Coauthors(authors)
            }
            RelationshipKind::References => {
                let references: Vec<Reference> = serde_json::from_value(
                    value["references"].clone(),
                )
                .map_err(|_| {
                    invalid(
                        "$.references",
                        "references require numeric type and positive id with no unknown fields",
                    )
                })?;
                let mut ids = BTreeSet::new();
                for reference in &references {
                    if !(1..=3).contains(&reference.kind)
                        || !ids.insert((reference.kind, reference.id))
                    {
                        return Err(invalid(
                            "$.references",
                            "reference types are plugin=1, WA=2, config=3 and targets must be unique",
                        ));
                    }
                }
                Items::References(references)
            }
        };
        Ok(Self {
            resource,
            id,
            items,
        })
    }
    fn action(&self) -> &'static str {
        match self.items {
            Items::Coauthors(_) => "co_authors",
            Items::References(_) => "references",
        }
    }
    fn context(&self) -> Value {
        match self.items {
            Items::Coauthors(_) => {
                json!({"content_type":self.resource.coauthor_type(),"content_id":self.id})
            }
            Items::References(_) => {
                json!({"content_type":self.resource.reference_type(),"content_id":self.id})
            }
        }
    }
    fn payload(&self) -> Value {
        match &self.items {
            Items::Coauthors(authors) => {
                json!({"content_type":self.resource.coauthor_type(),"content_id":self.id,"co_authors":authors})
            }
            Items::References(references) => {
                json!({"source_type":self.resource.reference_type(),"source_id":self.id,"references":references})
            }
        }
    }
    fn matches(&self, value: &Value) -> Result<bool> {
        let rows = if let Some(rows) = value.as_array() {
            rows
        } else {
            [self.action(), "list", "items", "data"]
                .iter()
                .find_map(|key| value.get(*key).and_then(Value::as_array))
                .ok_or_else(|| remote("NewBeeBox relationship response has no complete array"))?
        };
        for key in ["total", "count", "total_count"] {
            if let Some(total) = value.get(key) {
                if wire_uint(total.clone(), 0)? != rows.len() as u64 {
                    return Err(remote("NewBeeBox relationship readback is incomplete"));
                }
            }
        }
        match &self.items {
            Items::Coauthors(authors) => {
                let mut found = Vec::new();
                for row in rows {
                    let id = wire_id(pick(row, &["user_id", "t_user_id", "id"]))?;
                    let share = pick(row, &["share_percent", "t_share_percent", "ratio"]);
                    let share = share
                        .as_f64()
                        .or_else(|| share.as_str().and_then(|s| s.parse::<f64>().ok()))
                        .filter(|share| share.is_finite())
                        .ok_or_else(|| remote("NewBeeBox co-author share is invalid"))?;
                    found.push((id, share));
                }
                Ok(found.len() == authors.len()
                    && authors.iter().all(|a| {
                        found
                            .iter()
                            .filter(|(id, share)| {
                                *id == a.user_id.get() && Some(*share) == a.share_percent.as_f64()
                            })
                            .count()
                            == 1
                    }))
            }
            Items::References(references) => {
                for (key, expected) in [
                    ("source_type", self.resource.reference_type()),
                    ("content_type", self.resource.reference_type()),
                    ("source_id", self.id.get()),
                    ("content_id", self.id.get()),
                ] {
                    if let Some(value) = value.get(key) {
                        if wire_uint(value.clone(), 0)? != expected {
                            return Err(remote(
                                "NewBeeBox relationship readback belongs to a different source",
                            ));
                        }
                    }
                }
                let mut found = BTreeSet::new();
                for row in rows {
                    for (key, expected) in [
                        ("source_type", self.resource.reference_type()),
                        ("source_id", self.id.get()),
                    ] {
                        if let Some(value) = row.get(key) {
                            if wire_uint(value.clone(), 0)? != expected {
                                return Err(remote(
                                    "NewBeeBox relationship row belongs to a different source",
                                ));
                            }
                        }
                    }
                    let mut returned_type = None;
                    for key in ["type", "content_type", "t_type"] {
                        if let Some(value) = row.get(key) {
                            let value = wire_uint(value.clone(), 0)?;
                            if !(2..=4).contains(&value)
                                || returned_type.is_some_and(|prior| prior != value)
                            {
                                return Err(remote(
                                    "NewBeeBox reference display types are missing or conflicting",
                                ));
                            }
                            returned_type = Some(value);
                        }
                    }
                    let kind = returned_type
                        .ok_or_else(|| remote("NewBeeBox reference has no display type"))?
                        - 1;
                    if !found.insert((kind, wire_id(pick(row, &["id", "content_id", "t_id"]))?)) {
                        return Ok(false);
                    }
                }
                Ok(found
                    == references
                        .iter()
                        .map(|r| (r.kind as u64, r.id.get()))
                        .collect())
            }
        }
    }
}

impl Newbee {
    pub fn replace_relationships(
        &mut self,
        replacement: &RelationshipReplacement,
    ) -> Result<Value> {
        let payload = replacement.payload();
        let mut mutation = Mutation::open(
            &self.home,
            Provider::Newbee,
            replacement.resource.key(),
            replacement.id.get(),
            replacement.action(),
            &payload,
            &self.control,
        )?;
        let prefix = match replacement.items {
            Items::Coauthors(_) => "/creator/co_author",
            Items::References(_) => "/creator/content_reference",
        };
        let read_path = format!("{prefix}/list");
        let set_path = format!("{prefix}/set");
        let before = self
            .creator(&read_path, replacement.context())
            .map_err(|error| {
                if mutation.pending() {
                    mutation.readback_error(
                        "the original relationship write still needs a readable result",
                        error,
                    )
                } else {
                    error
                }
            })?;
        let matches = replacement.matches(&before).map_err(|error| {
            if mutation.pending() {
                mutation.readback_error(
                    "the original relationship write still needs a valid readback",
                    error,
                )
            } else {
                error
            }
        })?;
        if matches {
            let recovered = mutation.pending();
            if recovered {
                mutation.verified()?;
            }
            return Ok(output::public(
                json!({"operation_id":if recovered {mutation.id()}else{None},"resource":replacement.resource.key(),"id":replacement.id,
                "changed":false,"recovered":recovered,"author_writes":0,"readback":before}),
                &[],
            ));
        }
        if mutation.pending() {
            return Err(mutation.pending_error(
                "the original relationship write is not verified; no replacement was sent",
            ));
        }
        mutation.prepare(&self.control)?;
        if let Err(error) = self.post(Origin::NewbeeCreator, &set_path, &payload, true) {
            return Err(mutation.failure(error));
        }
        mutation.acknowledged().map_err(|_| {
            mutation.pending_error("relationship write was acknowledged; finish saving its receipt")
        })?;
        let readback = self
            .creator(&read_path, replacement.context())
            .map_err(|error| {
                mutation.readback_error(
                    "relationship write was acknowledged; readback is pending",
                    error,
                )
            })?;
        if !replacement.matches(&readback).map_err(|error| {
            mutation.readback_error(
                "relationship write was acknowledged; its readback is invalid",
                error,
            )
        })? {
            return Err(mutation
                .pending_error("relationship write readback does not match the full replacement"));
        }
        mutation.verified().map_err(|_| {
            mutation.pending_error("relationship readback is verified; finish saving its receipt")
        })?;
        Ok(output::public(
            json!({"operation_id":mutation.id(),"resource":replacement.resource.key(),"id":replacement.id,"changed":true,"recovered":false,"author_writes":1,"readback":readback}),
            &[],
        ))
    }
}
