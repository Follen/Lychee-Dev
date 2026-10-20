//! Versioned, task-native query contracts. SQL is optional, never required to
//! express a typed task. Source binding comes from the enclosing session.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SourceMode {
    Static,
    Effective,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TableRequest {
    pub name: String,
    pub mode: SourceMode,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hotfix_captures: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum QueryRequest {
    Plan { plan: QueryPlan },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum QueryPlan {
    Scan {
        table: TableRequest,
    },
    Lookup {
        table: TableRequest,
        ids: Vec<u32>,
    },
    Filter {
        input: Box<QueryPlan>,
        predicate: Expression,
    },
    Project {
        input: Box<QueryPlan>,
        columns: Vec<NamedExpression>,
    },
    Join {
        left: Box<QueryPlan>,
        right: Box<QueryPlan>,
        kind: JoinKind,
        on: Vec<JoinKey>,
    },
    Aggregate {
        input: Box<QueryPlan>,
        groups: Vec<NamedExpression>,
        metrics: Vec<AggregateExpression>,
    },
    Sort {
        input: Box<QueryPlan>,
        keys: Vec<SortKey>,
    },
    Limit {
        input: Box<QueryPlan>,
        offset: usize,
        count: usize,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Expression {
    Column {
        name: String,
    },
    Literal {
        value: serde_json::Value,
    },
    Binary {
        left: Box<Expression>,
        operator: BinaryOperator,
        right: Box<Expression>,
    },
    Not {
        value: Box<Expression>,
    },
    IsNull {
        value: Box<Expression>,
    },
    In {
        value: Box<Expression>,
        values: Vec<serde_json::Value>,
    },
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BinaryOperator {
    Add,
    Subtract,
    Multiply,
    Divide,
    Modulo,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    And,
    Or,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NamedExpression {
    pub name: String,
    pub expression: Expression,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JoinKind {
    Inner,
    Left,
    Full,
    Semi,
    Anti,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JoinKey {
    pub left: String,
    pub right: String,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AggregateFunction {
    Count,
    Sum,
    Average,
    Minimum,
    Maximum,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AggregateExpression {
    pub name: String,
    pub function: AggregateFunction,
    pub expression: Option<Expression>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SortKey {
    pub expression: Expression,
    pub descending: bool,
    pub nulls_first: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DatasetInfo {
    #[serde(skip)]
    pub(super) _allocation: Option<crate::resource::OwnedRetainedReservation>,
    #[serde(skip)]
    pub(super) _lifetime: Option<std::sync::Arc<super::lifetime::RequestLifetime>>,
    pub table: TableRequest,
    pub columns: Vec<ColumnInfo>,
    pub row_count: usize,
    pub provenance: serde_json::Value,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ColumnInfo {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResultPage {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<serde_json::Value>>,
    pub offset: usize,
    pub total_rows: usize,
    pub complete: bool,
    #[serde(skip)]
    pub(crate) _allocation: crate::resource::OwnedRetainedReservation,
    #[serde(skip)]
    pub(crate) _budget: std::sync::Arc<crate::resource::Budget>,
    #[serde(skip)]
    pub(crate) _byte_limit: usize,
}
#[derive(Clone, Copy, Debug)]
pub struct EngineLimits {
    pub session_retained_bytes: usize,
    pub query_work: u64,
    pub operator_bytes: usize,
    pub page_bytes: usize,
    pub result_bytes: usize,
}
impl Default for EngineLimits {
    fn default() -> Self {
        Self {
            session_retained_bytes: 512 << 20,
            query_work: 100_000_000,
            operator_bytes: 128 << 20,
            page_bytes: 8 << 20,
            result_bytes: 128 << 20,
        }
    }
}

impl std::fmt::Debug for ResultPage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResultPage")
            .field("rows", &self.rows.len())
            .field("offset", &self.offset)
            .finish_non_exhaustive()
    }
}

/// Self-contained JSON Schema for the exact task-native wire contract. Runtime
/// admission adds whole-tree size/depth and binding checks before source I/O.
pub fn query_schema() -> serde_json::Value {
    use serde_json::{Value, json};
    fn object(properties: Value, required: &[&str]) -> Value {
        json!({"type":"object", "properties":properties, "required":required, "additionalProperties":false})
    }
    fn variant(tag: &str, name: &str, fields: Value, required: &[&str]) -> Value {
        let mut fields = fields
            .as_object()
            .expect("schema fields are objects")
            .clone();
        fields.insert(tag.into(), json!({"const":name}));
        let mut required = required.to_vec();
        required.insert(0, tag);
        object(Value::Object(fields), &required)
    }
    let expr = json!({"$ref":"#/$defs/Expression"});
    let plan = json!({"$ref":"#/$defs/QueryPlan"});
    let table = json!({"$ref":"#/$defs/TableRequest"});
    let scalar = json!({"$ref":"#/$defs/Scalar"});
    let column_name = json!({"type":"string", "minLength":1, "description":"Case-sensitive column or relation.column; each column component is at most 256 UTF-8 bytes and contains no dot/control character. No SQL parsing is performed."});
    let name = json!({"type":"string", "minLength":1, "description":"Output name: at most 256 UTF-8 bytes, no dot or control character."});
    let named =
        json!({"type":"array", "items":{"$ref":"#/$defs/NamedExpression"}, "maxItems":4096});
    let plans = vec![
        variant("op", "scan", json!({"table":table}), &["table"]),
        variant(
            "op",
            "lookup",
            json!({"table":table, "ids":{"type":"array", "items":{"type":"integer", "minimum":0, "maximum":4294967295}, "maxItems":16384, "description":"ID membership; duplicate IDs do not duplicate rows; empty IDs returns no rows."}}),
            &["table", "ids"],
        ),
        variant(
            "op",
            "filter",
            json!({"input":plan, "predicate":expr}),
            &["input", "predicate"],
        ),
        variant(
            "op",
            "project",
            json!({"input":plan, "columns":{"type":"array", "minItems":1, "maxItems":4096, "items":{"$ref":"#/$defs/NamedExpression"}}}),
            &["input", "columns"],
        ),
        variant(
            "op",
            "join",
            json!({"left":plan, "right":plan, "kind":{"enum":["inner","left","full","semi","anti"]}, "on":{"type":"array", "minItems":1,"maxItems":4096,"items":{"$ref":"#/$defs/JoinKey"}}}),
            &["left", "right", "kind", "on"],
        ),
        variant(
            "op",
            "aggregate",
            json!({"input":plan,"groups":named,"metrics":{"type":"array","maxItems":4096,"items":{"$ref":"#/$defs/AggregateExpression"}}}),
            &["input", "groups", "metrics"],
        ),
        variant(
            "op",
            "sort",
            json!({"input":plan,"keys":{"type":"array","minItems":1,"maxItems":4096,"items":{"$ref":"#/$defs/SortKey"}}}),
            &["input", "keys"],
        ),
        variant(
            "op",
            "limit",
            json!({"input":plan,"offset":{"type":"integer","minimum":0,"maximum":9223372036854775807u64},"count":{"type":"integer","minimum":0,"maximum":9223372036854775807u64}}),
            &["input", "offset", "count"],
        ),
    ];
    let expressions = vec![
        variant("kind", "column", json!({"name":column_name}), &["name"]),
        variant("kind", "literal", json!({"value":scalar}), &["value"]),
        variant(
            "kind",
            "binary",
            json!({"left":expr,"operator":{"enum":["add","subtract","multiply","divide","modulo","equal","not_equal","less","less_equal","greater","greater_equal","and","or"]},"right":expr}),
            &["left", "operator", "right"],
        ),
        variant("kind", "not", json!({"value":expr}), &["value"]),
        variant("kind", "is_null", json!({"value":expr}), &["value"]),
        variant(
            "kind",
            "in",
            json!({"value":expr,"values":{"type":"array","maxItems":16384,"items":scalar}}),
            &["value", "values"],
        ),
    ];
    json!({
        "$schema":"https://json-schema.org/draft/2020-12/schema",
        "title":"Lychee task-native Data query request",
        "description":"Typed plans execute in DataFusion without synthesized SQL. SQL is not exposed by this task interface. Objects reject unknown fields. Field names and enum spellings are snake_case.",
        "oneOf":[
            variant("kind","plan",json!({"plan":plan}), &["plan"])
        ],
        "$defs":{
            "Scalar":{"type":["null","boolean","string","number"],"description":"Integer lexemes must fit signed i64 or unsigned u64 exactly; floating-point lexemes must convert to finite f64. Arrays/objects are unsupported. Integer arithmetic and mixed-float comparisons preserve the exact numeric contract."},
            "TableRequest":object(json!({"name":{"type":"string","pattern":"^[A-Za-z_][A-Za-z0-9_]*$","maxLength":128},"mode":{"enum":["static","effective"]},"hotfix_captures":{"type":"array","maxItems":16,"uniqueItems":true,"items":{"type":"string","minLength":1},"default":[],"description":"Ordered explicit capture IDs, at most 256 UTF-8 bytes each, without controls. Static mode requires an empty list; effective mode requires 1 to 16 IDs; authenticity is checked by the source adapter."}}), &["name","mode"]),
            "QueryPlan":{"oneOf":plans},
            "Expression":{"oneOf":expressions},
            "NamedExpression":object(json!({"name":name,"expression":expr}), &["name","expression"]),
            "JoinKey":object(json!({"left":column_name,"right":column_name}), &["left","right"]),
            "AggregateExpression":object(json!({"name":name,"function":{"enum":["count","sum","average","minimum","maximum"]},"expression":{"anyOf":[expr,{"type":"null"}],"description":"Only count may omit expression or use null; this counts all rows. count(expression) counts nonnull values."}}), &["name","function"]),
            "SortKey":object(json!({"expression":expr,"descending":{"type":"boolean"},"nulls_first":{"type":"boolean"}}), &["expression","descending","nulls_first"])
        },
        "x-runtime-limits":{"maxDepth":64,"maxPlanAndExpressionNodes":4096,"maxLiteralLookupAndJoinKeyValues":16384,"maxTextBytes":1048576,"maxComputedInExpansionBytes":67108864},
        "x-semantics":[
            "Scan columns are qualified by the requested table name. A join requalifies its immediate inputs as left and right; downstream duplicate columns must use left.Column or right.Column. Project to unique names before nesting joins with duplicate names.",
            "Projection and aggregate output names must be unique. An aggregate requires at least one group or metric. Limit offset + count must fit both platform usize and signed i64. Sort direction and NULL placement are explicit.",
            "Static/effective bindings are distinct. A single request cannot bind differing capture histories to the same table name and mode. Capture order is significant.",
            "IN uses SQL NULL semantics and exact numeric equality; an empty value list returns false. Without sort, row order is unspecified."
        ]
    })
}

impl std::fmt::Debug for DatasetInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DatasetInfo")
            .field("table", &self.table)
            .field("row_count", &self.row_count)
            .finish_non_exhaustive()
    }
}
impl Drop for DatasetInfo {
    fn drop(&mut self) {
        drop(std::mem::take(&mut self.table.name));
        drop(std::mem::take(&mut self.table.hotfix_captures));
        drop(std::mem::take(&mut self.columns));
        drop(std::mem::replace(
            &mut self.provenance,
            serde_json::Value::Null,
        ));
        self._allocation.take();
        if let Some(lifetime) = &self._lifetime {
            let _ = lifetime.settle();
        }
    }
}
