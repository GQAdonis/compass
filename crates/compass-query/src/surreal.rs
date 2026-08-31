use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;

use compass_graphdb_surreal::{SURREAL_REF_FILE_NAME, SurrealProjection, SurrealRef};
use compass_model::query_contract::{
    CallRequest, CodeQueryResponse, ExploreRequest, ImpactRequest, NodeTrailRequest, SearchRequest,
};

use crate::code_query::CodeGraphBackend;
use crate::surreal_backend::SurrealCodeBackend;
use crate::{CodeQueryEngine, NaturalQueryRequest, QueryEngineKind, QueryError, QueryErrorKind};

#[derive(Clone)]
pub struct SurrealQueryEngine {
    reference: SurrealRef,
    pub(crate) projection: Arc<SurrealProjection>,
    graph_path: PathBuf,
}

impl std::fmt::Debug for SurrealQueryEngine {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SurrealQueryEngine")
            .field("repository_id", &self.reference.repository_id)
            .field("generation_id", &self.reference.generation_id)
            .field("engine", &self.reference.engine)
            .finish()
    }
}

impl SurrealQueryEngine {
    pub async fn open(graph_path: &Path) -> Result<Self, QueryError> {
        Self::open_reference_at(read_surreal_ref(graph_path)?, Some(graph_path)).await
    }

    pub async fn open_reference(reference: SurrealRef) -> Result<Self, QueryError> {
        Self::open_reference_at(reference, None).await
    }

    async fn open_reference_at(
        reference: SurrealRef,
        graph_path: Option<&Path>,
    ) -> Result<Self, QueryError> {
        reference.validate().map_err(projection_error)?;
        let projection = SurrealProjection::open_reference(&reference, false)
            .await
            .map_err(projection_error)?;
        Self::from_projection_at(reference, projection, graph_path).await
    }

    /// Construct a generation-pinned query engine from an already-open local
    /// projection. Embedded engines permit only one datastore handle per path,
    /// so in-process publishers can hand their validated connection directly
    /// to the query layer instead of reopening the same location.
    pub async fn from_projection(
        reference: SurrealRef,
        projection: SurrealProjection,
    ) -> Result<Self, QueryError> {
        Self::from_projection_at(reference, projection, None).await
    }

    async fn from_projection_at(
        reference: SurrealRef,
        projection: SurrealProjection,
        graph_path: Option<&Path>,
    ) -> Result<Self, QueryError> {
        reference.validate().map_err(projection_error)?;
        projection
            .validate_reference(&reference)
            .await
            .map_err(projection_error)?;
        let metadata = projection
            .metadata_at(&reference)
            .await
            .map_err(projection_error)?;
        crate::graph_engine::validate_builder(&metadata.build.builder_version, graph_path)?;
        Ok(Self {
            reference,
            projection: Arc::new(projection),
            graph_path: graph_path.unwrap_or_else(|| Path::new("")).to_path_buf(),
        })
    }

    /// Bind source-file provenance to the actual snapshot path without reading
    /// its canonical JSON. A bare reference has no trusted source root.
    #[must_use]
    pub fn with_graph_path(mut self, graph_path: &Path) -> Self {
        self.graph_path = graph_path.to_path_buf();
        self
    }

    #[must_use]
    pub fn reference(&self) -> &SurrealRef {
        &self.reference
    }

    /// Pin another immutable generation on the same embedded connection. The
    /// returned engine never changes the generation pinned by this engine.
    pub async fn with_reference(&self, reference: SurrealRef) -> Result<Self, QueryError> {
        reference.validate().map_err(projection_error)?;
        if SurrealConnectionKey::from_reference(&reference)?
            != SurrealConnectionKey::from_reference(&self.reference)?
        {
            return Err(QueryError::new(
                QueryErrorKind::InvalidParameter,
                "surreal_connection_mismatch",
                "a pinned Surreal connection cannot be rebound to another store or namespace",
            ));
        }
        self.projection
            .validate_reference(&reference)
            .await
            .map_err(projection_error)?;
        let metadata = self
            .projection
            .metadata_at(&reference)
            .await
            .map_err(projection_error)?;
        crate::graph_engine::validate_builder(
            &metadata.build.builder_version,
            Some(&self.graph_path),
        )?;
        Ok(Self {
            reference,
            projection: Arc::clone(&self.projection),
            graph_path: self.graph_path.clone(),
        })
    }

    pub async fn query_natural(
        &self,
        request: NaturalQueryRequest,
    ) -> Result<CodeQueryResponse, QueryError> {
        self.typed_query(move |engine| engine.query_natural(request))
            .await
    }

    pub async fn search(&self, request: SearchRequest) -> Result<CodeQueryResponse, QueryError> {
        self.typed_query(move |engine| engine.search(request)).await
    }

    pub async fn callers(&self, request: CallRequest) -> Result<CodeQueryResponse, QueryError> {
        self.typed_query(move |engine| engine.callers(request))
            .await
    }

    pub async fn callees(&self, request: CallRequest) -> Result<CodeQueryResponse, QueryError> {
        self.typed_query(move |engine| engine.callees(request))
            .await
    }

    pub async fn impact(&self, request: ImpactRequest) -> Result<CodeQueryResponse, QueryError> {
        self.typed_query(move |engine| engine.impact(request)).await
    }

    pub async fn explore(&self, request: ExploreRequest) -> Result<CodeQueryResponse, QueryError> {
        self.typed_query(move |engine| engine.explore(request))
            .await
    }

    pub async fn node_trail(
        &self,
        request: NodeTrailRequest,
    ) -> Result<CodeQueryResponse, QueryError> {
        self.typed_query(move |engine| engine.node_trail(request))
            .await
    }

    async fn typed_query<F>(&self, operation: F) -> Result<CodeQueryResponse, QueryError>
    where
        F: FnOnce(&CodeQueryEngine) -> Result<CodeQueryResponse, QueryError> + Send + 'static,
    {
        let metadata = self
            .projection
            .metadata_at(&self.reference)
            .await
            .map_err(projection_error)?;
        let partial_graph_message = metadata
            .diagnostics
            .iter()
            .rfind(|diagnostic| diagnostic.code == "publication_omission_summary")
            .map(|diagnostic| {
                format!(
                    "Published graph coverage is incomplete: {}",
                    diagnostic.message
                )
            });
        let backend = SurrealCodeBackend {
            projection: Arc::clone(&self.projection),
            reference: self.reference.clone(),
            metadata,
            runtime: tokio::runtime::Handle::current(),
        };
        // Snapshot paths passed by the CLI may be relative. Source verification
        // derives the checkout root from this path and needs an absolute anchor,
        // just like the JSON and SQLite engine loaders.
        let graph_path = std::path::absolute(&self.graph_path).map_err(|error| {
            QueryError::new(
                QueryErrorKind::Internal,
                "surreal_graph_path_unresolved",
                format!("cannot resolve graph path: {error}"),
            )
        })?;
        let index_path = PathBuf::from(&self.reference.location);
        let graph_identity = self.reference.graph_digest.clone();
        let build_generation_identity = self.reference.generation_id.clone();
        // SQLite connections and a materialized graph are deliberately absent.
        // Construct inside the blocking worker so no synchronous DB read can
        // block the async reactor that drives the embedded connection.
        tokio::task::spawn_blocking(move || {
            let engine = CodeQueryEngine {
                backend: CodeGraphBackend::Surreal(Box::new(backend)),
                program: None,
                connection: None,
                graph_path,
                index_path,
                partial_graph_message,
                engine_kind: QueryEngineKind::Surreal,
                graph_identity,
                build_generation_identity,
                search_query_cache: std::sync::Mutex::new(Default::default()),
                fuzzy_lookup_cache: std::sync::Mutex::new(Default::default()),
            };
            operation(&engine)
        })
        .await
        .map_err(|error| {
            QueryError::new(
                QueryErrorKind::Internal,
                "surreal_query_worker_failed",
                error.to_string(),
            )
        })?
    }
}

const MAX_SURREAL_QUERY_CONNECTIONS: usize = 16;

struct CachedSurrealConnection {
    reference: SurrealRef,
    engine: Arc<SurrealQueryEngine>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct SurrealConnectionKey {
    engine: String,
    location: PathBuf,
    namespace: String,
    database: String,
}

impl SurrealConnectionKey {
    fn from_reference(reference: &SurrealRef) -> Result<Self, QueryError> {
        let location = reference.connection_location().map_err(projection_error)?;
        Ok(Self {
            engine: reference.engine.as_str().to_owned(),
            location,
            namespace: reference.namespace.clone(),
            database: reference.database.clone(),
        })
    }
}

#[derive(Default)]
pub struct SurrealQueryEngineCache {
    entries: Mutex<BTreeMap<SurrealConnectionKey, CachedSurrealConnection>>,
}

impl SurrealQueryEngineCache {
    /// Seed the shared cache with a connection transferred by an in-process
    /// publisher. No second embedded datastore is opened for that location.
    pub fn from_engine(engine: SurrealQueryEngine) -> Result<Self, QueryError> {
        let key = SurrealConnectionKey::from_reference(engine.reference())?;
        let reference = engine.reference().clone();
        Ok(Self {
            entries: Mutex::new(BTreeMap::from([(
                key,
                CachedSurrealConnection {
                    reference,
                    engine: Arc::new(engine),
                },
            )])),
        })
    }

    pub async fn get(&self, graph_path: &Path) -> Result<Arc<SurrealQueryEngine>, QueryError> {
        let reference = read_surreal_ref(graph_path)?;
        let key = SurrealConnectionKey::from_reference(&reference)?;
        // Serialize first-open and generation repinning. Two concurrent misses
        // must never race to acquire the embedded engine's exclusive file lock.
        let mut entries = self.entries.lock().await;
        if let Some(engine) = entries
            .get(&key)
            .filter(|entry| entry.reference == reference && entry.engine.graph_path == graph_path)
            .map(|entry| Arc::clone(&entry.engine))
        {
            return Ok(engine);
        }
        let engine = if let Some(entry) = entries.get(&key) {
            Arc::new(
                entry
                    .engine
                    .as_ref()
                    .clone()
                    .with_graph_path(graph_path)
                    .with_reference(reference.clone())
                    .await?,
            )
        } else {
            if entries.len() >= MAX_SURREAL_QUERY_CONNECTIONS {
                return Err(QueryError::new(
                    QueryErrorKind::MemoryLimit,
                    "surreal_query_cache_capacity",
                    format!(
                        "Surreal query cache is limited to {MAX_SURREAL_QUERY_CONNECTIONS} embedded stores"
                    ),
                ));
            }
            Arc::new(
                SurrealQueryEngine::open_reference_at(reference.clone(), Some(graph_path)).await?,
            )
        };
        entries.insert(
            key,
            CachedSurrealConnection {
                reference,
                engine: Arc::clone(&engine),
            },
        );
        Ok(engine)
    }
}

#[must_use]
pub fn has_published_surreal(graph_path: &Path) -> bool {
    surreal_ref_path(graph_path).is_file()
}

pub fn read_surreal_ref(graph_path: &Path) -> Result<SurrealRef, QueryError> {
    let path = surreal_ref_path(graph_path);
    let mut bytes = Vec::new();
    fs::File::open(&path)
        .and_then(|file| {
            file.take(compass_graphdb_surreal::MAX_SURREAL_REF_BYTES + 1)
                .read_to_end(&mut bytes)
        })
        .map_err(|error| {
            QueryError::new(
                QueryErrorKind::CorruptArtifact,
                "surreal_ref_missing",
                format!("could not read {}: {error}", path.display()),
            )
        })?;
    SurrealRef::decode(&bytes).map_err(projection_error)
}

fn surreal_ref_path(graph_path: &Path) -> PathBuf {
    graph_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(SURREAL_REF_FILE_NAME)
}

pub(crate) fn projection_error(error: compass_graphdb_surreal::ProjectionError) -> QueryError {
    QueryError::new(
        QueryErrorKind::CorruptArtifact,
        "surreal_projection_failed",
        error.to_string(),
    )
}
