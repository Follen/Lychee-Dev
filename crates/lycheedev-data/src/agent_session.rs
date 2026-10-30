//! One caller-owned Agent worker. It installs no daemon and never shares result
//! handles across processes, snapshots or restarts. Query execution is owned by
//! `engine`, not by this transport/retention layer.
mod handles;
mod domain;
mod navigation;
mod wire_schema;
#[path = "agent_tasks.rs"]
pub mod tasks;

use crate::{
    engine::{
        DatasetCatalog, DatasetInfo, Engine, EngineLimits, OwnedProvenance, QueryRequest,
        ResultOwner, ResultPage, TableRequest,
    },
    fault,
};
use handles::Handles;
use crate::request::{DataContext, SessionConfig};
use lycheedev_core::{Result, control::OperationControl};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const PROTOCOL: &str = "lycheedev.data-agent.v1";
const MAX_HANDLES: usize = 32;
const MAX_HANDLE_BYTES: usize = 512 << 20;
const MAX_PAGE_ROWS: usize = 1000;

#[derive(Debug, Deserialize)]
pub struct Request {
    pub protocol: String,
    pub id: u64,
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
    #[serde(flatten)]
    pub task: Task,
}
fn default_timeout() -> u64 {
    30_000
}
#[derive(Debug, Deserialize)]
#[serde(tag = "task", rename_all = "snake_case", deny_unknown_fields)]
pub enum Task {
    Navigate { request: crate::engine::DomainRequest },
    ItemGet { item_id:u32, mode:crate::engine::SourceMode, #[serde(default)] hotfix_captures:Vec<String> },
    Semantics { table:TableRequest },
    Explain { query:QueryRequest, #[serde(default)] verbose:bool, #[serde(default)] analyze:bool },
    Capture {
        handle: String,
    },
    Catalog {},
    Fields {
        table: TableRequest,
    },
    Business {
        request: tasks::BusinessTask,
    },
    Discover {
        table: TableRequest,
    },
    Query {
        query: QueryRequest,
    },
    Page {
        handle: String,
        #[serde(default)]
        offset: usize,
        limit: usize,
    },
    Export {
        handle: String,
        #[serde(default)]
        offset: usize,
        limit: usize,
        format: ExportFormat,
    },
    Release {
        handle: String,
    },
    Cancel {
        request_id: u64,
    },
    Close {},
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportFormat {
    Json,
    Csv,
}

pub fn schema() -> Value {
    let query = crate::engine::query_schema();
    let business = tasks::schema();
    let navigate = navigation::schema();
    let request_schema = wire_schema::request_schema(&query, &business, &navigate);
    let task_names: Vec<_> = request_schema["oneOf"].as_array().unwrap().iter()
        .map(|variant| variant["properties"]["task"]["const"].clone()).collect();
    json!({"protocol":PROTOCOL,"transport":"stdin/stdout JSONL", "worker":"caller-owned supervisor plus one isolated reusable engine worker; no installed daemon", "processMemory":{"windows":"Job aggregate commit ceiling: --max-bytes plus 268435456 runtime/ingress bytes; not RSS", "otherTargets":"not shipped; Windows amd64 is required"},
        "request":{"protocol":PROTOCOL,"id":"strictly increasing positive u64; never replayed","timeout_ms":"1..3600000, includes queue wait; default 30000","task":task_names},
        "requestSchema":request_schema,
        "identity":"one immutable invocation snapshot/product/build/locale; explicit ordered Hotfix capture history is bound per table; handles do not survive worker restart",
        "query":query,
        "business":business,
        "navigate":navigate,
        "semantics":{"required":["table"],"source":"static with no captures only","output":"owned pin-bound DBD flags and conditions, not Hotfix row coverage"},
        "explain":{"required":["query"],"verbose":{"type":"boolean","default":false},"analyze":{"type":"boolean","default":false},"output":"normal owned result handle; page/export/capture apply","execution":"analyze=true executes under the same Engine pool and supervised deadline; provenance labels explain/analyze"},
        "item_get":{"alias":"navigate item_get","output":"owned domain report and automatic CAP"},
        "capture":{"handle":"existing session result handle","semantics":"query success and result handle do not imply CAP commit; capture persists complete result plus typed request/source provenance","maximumBytes":16777216,"onLimit":"fail without emitting a truncated CAP","failure":"capture does not consume the handle while the worker survives and TTL remains valid; wire cancellation or worker termination invalidates every handle. Inspect returned capturePublication/completedCaptures and the evidence catalog after uncertain or post-commit failures. A missing receipt does not prove no commit; never automatically replay"},
        "catalog":"fixed snapshot definition manifest directory only; complete means manifest enumeration complete, not installed availability or build readability; availability is not_checked; fields resolves actual table schema",
        "configuration":{"--max-objects":"1..1024 retained handles; default 32", "--max-bytes":"1MiB..2GiB shared session memory admission; default 512MiB", "--result-ttl-seconds":"1..3600, default 300; expiry refuses later pages rather than rerunning queries"},
        "limits":{"requestBytes":1048576,"queuedRequests":16,"resultHandles":MAX_HANDLES,"retainedResultBytes":MAX_HANDLE_BYTES,"resultTtlMs":300000,"pageRows":MAX_PAGE_ROWS},
        "lifecycle":"deadlines include queue wait and stdout backpressure; cancel of outstanding work terminates the isolated worker and invalidates every handle, without replay; release drops a result; close or stdin EOF drains submitted work before releasing the session; incomplete output frames are not results",
        "export":"bounded JSON or CSV page returned on stdout, not an arbitrary file write; retain provenance separately from rows"})
}

/// Responses retain the engine's page allocation guard through serialization
/// and flush. Never convert pages to an unowned serde_json::Value copy.
#[derive(Serialize)]
#[serde(untagged)]
pub enum Response {
    Semantics(crate::engine::OwnedSemantics),
    Domain { #[serde(flatten)] result:crate::engine::DomainResult, capture:crate::engine::OwnedCapture },
    Capture(crate::engine::OwnedCapture),
    Catalog(DatasetCatalog),
    Business {
        components: Vec<BusinessComponent>,
    },
    Value(Value),
    Query {
        result: handles::HandleReceipt,
        complete: bool,
        provenance: OwnedProvenance,
    },
    Discovery(DatasetInfo),
    Page(ResultPage),
    JsonExport {
        format: &'static str,
        page: ResultPage,
    },
    CsvExport(CsvExport),
}
#[derive(Serialize)]
pub struct BusinessComponent {
    name: &'static str,
    result: handles::HandleReceipt,
    complete: bool,
    provenance: OwnedProvenance,
}
pub struct CsvExport {
    page: ResultPage,
}
impl Serialize for CsvExport {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        struct Text<'a>(&'a ResultPage);
        impl Serialize for Text<'_> {
            fn serialize<S: serde::Serializer>(
                &self,
                serializer: S,
            ) -> std::result::Result<S::Ok, S::Error> {
                serializer.collect_str(&CsvDisplay(self.0))
            }
        }
        let mut object = serializer.serialize_struct("CsvExport", 3)?;
        object.serialize_field("format", "csv")?;
        object.serialize_field("text", &Text(&self.page))?;
        object.serialize_field("page", &self.page)?;
        object.end()
    }
}
struct CsvDisplay<'a>(&'a ResultPage);
impl std::fmt::Display for CsvDisplay<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use std::fmt::Write;
        struct Escaped<'a, 'b>(&'a mut std::fmt::Formatter<'b>);
        impl std::fmt::Write for Escaped<'_, '_> {
            fn write_str(&mut self, text: &str) -> std::fmt::Result {
                for part in text.split_inclusive('"') {
                    self.0.write_str(part)?;
                    if part.ends_with('"') {
                        self.0.write_str("\"")?;
                    }
                }
                Ok(())
            }
        }
        for (index, column) in self.0.columns.iter().enumerate() {
            if index != 0 {
                f.write_str(",")?;
            }
            f.write_str("\"")?;
            Escaped(f).write_str(column)?;
            f.write_str("\"")?;
        }
        f.write_str("\r\n")?;
        for row in &self.0.rows {
            for (index, value) in row.iter().enumerate() {
                if index != 0 {
                    f.write_str(",")?;
                }
                f.write_str("\"")?;
                match value {
                    Value::Null => {}
                    Value::String(text) => Escaped(f).write_str(text)?,
                    value => write!(Escaped(f), "{value}")?,
                }
                f.write_str("\"")?;
            }
            f.write_str("\r\n")?;
        }
        Ok(())
    }
}

pub struct Session {
    engine: Engine,
    handles: Handles<ResultOwner>,
}
impl Session {
    pub fn new(context: DataContext, config: SessionConfig) -> Result<Self> {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| fault("data.session.clock", 3, "System clock precedes the epoch"))?
            .as_nanos();
        let epoch = format!("{}-{stamp:x}", std::process::id());
        let max_handles = config.max_handles;
        let max_bytes = config.retained_bytes;
        let limits = EngineLimits {
            session_retained_bytes: max_bytes,
            operator_bytes: (max_bytes / 4).min(128 << 20),
            result_bytes: (max_bytes / 4).min(128 << 20),
            page_bytes: (max_bytes / 8).min(8 << 20),
            ..EngineLimits::default()
        };
        Ok(Self {
            engine: Engine::new(context, limits)?,
            handles: Handles::new(
                epoch,
                max_handles,
                max_bytes,
                Duration::from_secs(config.result_ttl_seconds),
            )?,
        })
    }
    pub fn expire(&mut self) {
        self.handles.expire(Instant::now());
    }
    pub fn close(&mut self) -> Result<()> {
        self.handles.clear();
        self.engine.close()
    }
    fn publish_query(&mut self,owner:ResultOwner)->Result<Response>{
        let rows=owner.row_count();let bytes=owner.retained_bytes();let complete=owner.complete();let provenance=owner.owned_provenance()?;
        let result=self.handles.insert(owner,rows,bytes,Instant::now())?;Ok(Response::Query{result,complete,provenance})
    }
    pub async fn execute(&mut self, task: Task, control: OperationControl) -> Result<Response> {
        control.check()?;
        self.expire();
        match task {
            Task::Navigate{request}=>{let(result,capture)=navigation::execute(&mut self.engine,request,control).await?;Ok(Response::Domain{result,capture})},
            Task::ItemGet{item_id,mode,hotfix_captures}=>{let(result,capture)=navigation::execute(&mut self.engine,crate::engine::DomainRequest::ItemGet{item_id,mode,hotfix_captures},control).await?;Ok(Response::Domain{result,capture})},
            Task::Semantics{table}=>Ok(Response::Semantics(self.engine.semantics(table,control).await?)),
            Task::Explain{query,verbose,analyze}=>{let owner=self.engine.explain(query,verbose,analyze,control).await?;self.publish_query(owner)},
            Task::Capture { handle } => {
                let owner = self.handles.get(&handle, Instant::now())?;
                Ok(Response::Capture(self.engine.capture(owner, control)?))
            }
            Task::Catalog {} => Ok(Response::Catalog(self.engine.catalog(control).await?)),
            Task::Business { request } => {
                let plans = tasks::compile(request)?;
                // All source/result owners survive until every component is ready.
                // Start handle TTL at publication, never during an earlier query.
                let mut pending = Vec::with_capacity(plans.len());
                for component in plans {
                    let owner = self
                        .engine
                        .execute(
                            QueryRequest::Plan {
                                plan: component.plan,
                            },
                            control.clone(),
                        )
                        .await?;
                    let provenance = owner.owned_provenance()?;
                    pending.push((component.name, owner, provenance));
                }
                control.check()?;
                let now = Instant::now();
                let mut components: Vec<BusinessComponent> = Vec::with_capacity(pending.len());
                for (name, owner, provenance) in pending {
                    let complete = owner.complete();
                    let rows = owner.row_count();
                    let bytes = owner.retained_bytes();
                    match self.handles.insert(owner, rows, bytes, now) {
                        Ok(result) => components.push(BusinessComponent {
                            name,
                            result,
                            complete,
                            provenance,
                        }),
                        Err(error) => {
                            for component in &components {
                                let _ = self.handles.release(&component.result.handle, now);
                            }
                            return Err(error);
                        }
                    }
                }
                Ok(Response::Business { components })
            }
            Task::Discover { table } | Task::Fields { table } => Ok(Response::Discovery(
                self.engine.discover(table, control).await?,
            )),
            Task::Query { query } => {
                let owner = self.engine.execute(query, control).await?;
                self.publish_query(owner)
            }
            Task::Page {
                handle,
                offset,
                limit,
            } => {
                validate_page(offset, limit)?;
                let owner = self.handles.get(&handle, Instant::now())?;
                Ok(Response::Page(owner.page(offset, limit)?))
            }
            Task::Export {
                handle,
                offset,
                limit,
                format,
            } => {
                validate_page(offset, limit)?;
                let owner = self.handles.get(&handle, Instant::now())?;
                let page = owner.page(offset, limit)?;
                match format {
                    ExportFormat::Json => Ok(Response::JsonExport {
                        format: "json",
                        page,
                    }),
                    ExportFormat::Csv => Ok(Response::CsvExport(CsvExport { page })),
                }
            }
            Task::Release { handle } => {
                self.handles.release(&handle, Instant::now())?;
                Ok(Response::Value(json!({"released":handle})))
            }
            Task::Cancel { .. } | Task::Close {} => Err(fault(
                "data.session.transport_task",
                8,
                "Transport must own cancellation and close",
            )),
        }
    }
}
fn validate_page(offset: usize, limit: usize) -> Result<()> {
    if limit == 0 || limit > MAX_PAGE_ROWS || offset.checked_add(limit).is_none() {
        return Err(fault(
            "data.session.page_limit",
            2,
            "Page limit must be 1..1000 and offset must not overflow",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod schema_tests;

#[cfg(test)]
#[path="agent_session/navigation_tests.rs"]
mod navigation_tests;
