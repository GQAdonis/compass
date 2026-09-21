//! Native CompassQL execution over generation-pinned Surreal record selectors.
//! The semantic operators are shared with JSON/SQLite, but storage access is
//! lazy: no graph document, adjacency index, or JSON/SQLite fallback is built.

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use compass_cypher::{CompiledQuery, Direction, Parameters, RelationshipPattern};
use compass_graphdb_surreal::{
    CqlDirection, CqlNodeSelector, ProjectionError, ProjectionLimits, SurrealProjection, SurrealRef,
};
use compass_model::{EdgeIndex, EdgeRecord, NodeIndex, NodeRecord};

use crate::cql::graph_access::{NodeSelection, QueryGraph};
use crate::cql::{StorageQueryRequest, execute_on_storage};
use crate::{QueryError, QueryErrorKind, QueryLimits, QueryResult, SurrealQueryEngine};

pub struct SurrealCqlRequest<'a> {
    pub compiled: &'a CompiledQuery,
    pub parameters: &'a Parameters,
    pub limits: QueryLimits,
    pub cancellation: &'a AtomicBool,
}

impl SurrealQueryEngine {
    /// The schema fingerprint is staged with the generation; reading it does
    /// not scan graph records or open the portable canonical JSON.
    pub async fn schema_fingerprint(&self) -> Result<compass_model::SchemaFingerprint, QueryError> {
        let value = self
            .projection
            .cql_schema_fingerprint_at(self.reference())
            .await
            .map_err(storage_error)?;
        if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(corrupt("invalid CompassQL schema fingerprint".to_owned()));
        }
        let mut bytes = [0_u8; 32];
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16)
                .map_err(|error| corrupt(error.to_string()))?;
        }
        Ok(compass_model::SchemaFingerprint::from_bytes(bytes))
    }

    /// Run every CompassQL operator against the pinned Surreal storage view.
    /// Scans return bounded ordinals; expansion executes indexed SurrealQL;
    /// scalar, join, optional, aggregate, sort and union semantics consume only
    /// those bounded binding rows through the common semantic executor.
    pub async fn execute_cql(
        &self,
        request: SurrealCqlRequest<'_>,
    ) -> Result<QueryResult, QueryError> {
        if request.cancellation.load(Ordering::Relaxed) {
            return Err(cancelled());
        }
        let compiled = request.compiled.clone();
        let parameters = request.parameters.clone();
        let limits = request.limits;
        let cancellation = Arc::new(AtomicBool::new(false));
        let worker_cancellation = Arc::clone(&cancellation);
        let projection = Arc::clone(&self.projection);
        let reference = self.reference().clone();
        let runtime = tokio::runtime::Handle::current();
        let mut worker = tokio::task::spawn_blocking(move || {
            let mut graph = SurrealCqlGraph {
                projection,
                reference,
                runtime,
                limits,
                cancellation: Arc::clone(&worker_cancellation),
                fingerprint: String::new(),
                directed: true,
                cache: RefCell::new(RecordCache::default()),
            };
            graph.fingerprint =
                graph.read(graph.projection.cql_schema_fingerprint_at(&graph.reference))?;
            graph.directed = graph.read(graph.projection.cql_is_directed_at(&graph.reference))?;
            execute_on_storage(StorageQueryRequest {
                compiled: &compiled,
                graph: &graph,
                parameters: &parameters,
                limits,
                cancellation: &worker_cancellation,
            })
        });
        // Forward the caller's borrowed cancellation flag into the owned worker.
        // A cancelled request waits for the bounded DB operation to unwind,
        // rather than leaking a detached worker that keeps an embedded lock.
        let mut ticker = tokio::time::interval(Duration::from_millis(5));
        loop {
            tokio::select! {
                result = &mut worker => return result.map_err(|error| QueryError::new(
                    QueryErrorKind::Internal, "surreal_cql_worker_failed", error.to_string()))?,
                _ = ticker.tick() => {
                    if request.cancellation.load(Ordering::Relaxed) {
                        cancellation.store(true, Ordering::Relaxed);
                    }
                }
            }
        }
    }
}

struct SurrealCqlGraph {
    projection: Arc<SurrealProjection>,
    reference: SurrealRef,
    runtime: tokio::runtime::Handle,
    limits: QueryLimits,
    cancellation: Arc<AtomicBool>,
    fingerprint: String,
    directed: bool,
    cache: RefCell<RecordCache>,
}

#[derive(Clone)]
enum CachedRecord {
    Node(NodeRecord),
    Edge(EdgeRecord),
}

#[derive(Default)]
struct RecordCache {
    records: BTreeMap<(bool, usize), (CachedRecord, usize)>,
    order: VecDeque<(bool, usize)>,
    bytes: usize,
}

impl RecordCache {
    fn insert(&mut self, key: (bool, usize), record: CachedRecord, bytes: usize, cap: usize) {
        if bytes > cap {
            return;
        }
        while self.bytes.saturating_add(bytes) > cap || self.records.len() >= 256 {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            if let Some((_, size)) = self.records.remove(&oldest) {
                self.bytes = self.bytes.saturating_sub(size);
            }
        }
        self.order.push_back(key);
        self.records.insert(key, (record, bytes));
        self.bytes = self.bytes.saturating_add(bytes);
    }
}

impl SurrealCqlGraph {
    fn read<T>(
        &self,
        future: impl Future<Output = Result<T, ProjectionError>>,
    ) -> Result<T, QueryError> {
        if self.cancellation.load(Ordering::Relaxed) {
            return Err(cancelled());
        }
        self.runtime.block_on(async {
            tokio::pin!(future);
            loop {
                tokio::select! {
                    result = &mut future => return result.map_err(storage_error),
                    _ = tokio::time::sleep_until(tokio::time::Instant::from_std(self.limits.deadline)) => {
                        return Err(QueryError::new(QueryErrorKind::Timeout, "CQL3007", "query deadline exceeded"));
                    }
                    _ = tokio::time::sleep(Duration::from_millis(5)) => {
                        if self.cancellation.load(Ordering::Relaxed) { return Err(cancelled()); }
                    }
                }
            }
        })
    }

    fn cache_cap(&self) -> usize {
        // QueryLimits accounts for semantic binding rows identically on all
        // backends, not storage buffers (nor JSON's resident input graph).
        // Keep a separate finite storage cache; an oversized record bypasses it.
        8 * 1024 * 1024
    }
}

impl QueryGraph for SurrealCqlGraph {
    fn schema_fingerprint(&self) -> String {
        self.fingerprint.clone()
    }

    fn node(&self, index: NodeIndex) -> Result<Cow<'_, NodeRecord>, QueryError> {
        if let Some((CachedRecord::Node(node), _)) =
            self.cache.borrow().records.get(&(false, index))
        {
            return Ok(Cow::Owned(node.clone()));
        }
        let row = self.read(self.projection.cql_node_at(&self.reference, index))?;
        let bytes = row.payload_json.len().saturating_mul(4);
        let node = row
            .decode()
            .map_err(storage_error)?
            .to_query_record()
            .map_err(|error| corrupt(error.to_string()))?;
        self.cache.borrow_mut().insert(
            (false, index),
            CachedRecord::Node(node.clone()),
            bytes,
            self.cache_cap(),
        );
        Ok(Cow::Owned(node))
    }

    fn edge(&self, index: EdgeIndex) -> Result<Cow<'_, EdgeRecord>, QueryError> {
        if let Some((CachedRecord::Edge(edge), _)) = self.cache.borrow().records.get(&(true, index))
        {
            return Ok(Cow::Owned(edge.clone()));
        }
        let row = self.read(self.projection.cql_edge_at(&self.reference, index))?;
        let bytes = row.payload_json.len().saturating_mul(4);
        let edge = row
            .decode()
            .map_err(storage_error)?
            .to_query_record()
            .map_err(|error| corrupt(error.to_string()))?;
        self.cache.borrow_mut().insert(
            (true, index),
            CachedRecord::Edge(edge.clone()),
            bytes,
            self.cache_cap(),
        );
        Ok(Cow::Owned(edge))
    }

    fn select_nodes(&self, selection: NodeSelection<'_>) -> Result<Vec<NodeIndex>, QueryError> {
        let selector = match selection {
            NodeSelection::All => CqlNodeSelector::All,
            NodeSelection::Id(value) => CqlNodeSelector::Id(value),
            NodeSelection::Label(value) => CqlNodeSelector::Label(value),
            NodeSelection::DisplayLabel(value) => CqlNodeSelector::DisplayLabel(value),
            NodeSelection::SourceFile(value) => CqlNodeSelector::SourceFile(value),
        };
        // Identity-only storage reads are bounded by the publication contract,
        // independently of the common executor's semantic-row memory budget.
        let max_ordinals = ProjectionLimits::default().max_nodes();
        self.read(
            self.projection
                .cql_scan_at(&self.reference, selector, max_ordinals),
        )
    }

    fn adjacent(
        &self,
        node: NodeIndex,
        pattern: &RelationshipPattern,
    ) -> Result<Vec<(EdgeIndex, NodeIndex)>, QueryError> {
        let direction = if !self.directed {
            CqlDirection::Both
        } else {
            match pattern.direction {
                Direction::Incoming => CqlDirection::Incoming,
                Direction::Outgoing => CqlDirection::Outgoing,
                Direction::Undirected => CqlDirection::Both,
            }
        };
        let max_rows = ProjectionLimits::default().max_relations();
        self.read(self.projection.cql_adjacent_at(
            &self.reference,
            node,
            direction,
            &pattern.types,
            max_rows,
        ))
    }

    fn degree(&self, node: NodeIndex) -> Result<usize, QueryError> {
        // Match `Graph::degree`: a directed graph counts both directions, an
        // undirected one counts each incident edge once. The bounded adjacency
        // selector already fails closed with a limit error rather than
        // silently reporting a short count.
        let direction = if self.directed {
            CqlDirection::Both
        } else {
            CqlDirection::Outgoing
        };
        let max_rows = ProjectionLimits::default().max_relations();
        let adjacent: Vec<(EdgeIndex, NodeIndex)> = self.read(self.projection.cql_adjacent_at(
            &self.reference,
            node,
            direction,
            &[],
            max_rows,
        ))?;
        Ok(adjacent.len())
    }
}

fn storage_error(error: ProjectionError) -> QueryError {
    if matches!(error, ProjectionError::LimitExceeded { .. }) {
        QueryError::new(QueryErrorKind::MemoryLimit, "CQL3006", error.to_string())
    } else {
        corrupt(error.to_string())
    }
}

fn corrupt(message: String) -> QueryError {
    QueryError::new(
        QueryErrorKind::CorruptArtifact,
        "surreal_cql_projection_failed",
        message,
    )
}

fn cancelled() -> QueryError {
    QueryError::new(QueryErrorKind::Cancelled, "CQL3008", "query was cancelled")
}
