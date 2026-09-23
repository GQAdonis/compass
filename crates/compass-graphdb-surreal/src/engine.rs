//! Feature-gated SurrealDB execution layer.

use std::collections::BTreeSet;
use std::future::IntoFuture;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use surrealdb::engine::any::Any;
use surrealdb::types::{RecordId, SurrealValue as _, Value as DatabaseValue};
use surrealdb::{IndexedResults, Surreal};

use crate::projection::record_key;
use crate::{
    PROJECTION_SCHEMA_V2, ProjectedFile, ProjectedNode, ProjectedRelation, ProjectionBundle,
    ProjectionError, ProjectionLimits, ProjectionPlan, RelationFamily, SurrealEngine, SurrealRef,
};

mod bounded;
#[cfg(any(feature = "surrealkv", feature = "rocksdb"))]
mod persistent;
#[cfg(feature = "remote")]
mod remote;
use bounded::BoundedDatabase;
mod query;
mod reader;
pub use reader::{CqlDirection, CqlNodeSelector, EdgeSelector, NodeSelector};

pub use query::{
    CqlProjectionRequest, CqlProjectionSlice, NATIVE_RELATION_PAGE_SCHEMA_V1, RelationPage,
    RelationPageRequest,
};

const SCHEMA: &str = r#"
DEFINE TABLE OVERWRITE generation_manifest SCHEMAFULL;
DEFINE FIELD OVERWRITE repositoryId ON generation_manifest TYPE string;
DEFINE FIELD OVERWRITE directed ON generation_manifest TYPE bool;
DEFINE FIELD OVERWRITE multigraph ON generation_manifest TYPE bool;
DEFINE FIELD OVERWRITE generationId ON generation_manifest TYPE string;
DEFINE FIELD OVERWRITE schemaVersion ON generation_manifest TYPE string;
DEFINE FIELD OVERWRITE projectionFingerprint ON generation_manifest TYPE string;
DEFINE FIELD OVERWRITE graphDigest ON generation_manifest TYPE option<string>;
DEFINE FIELD OVERWRITE sourceTreeDigest ON generation_manifest TYPE string;
DEFINE FIELD OVERWRITE schemaFingerprint ON generation_manifest TYPE string;
DEFINE FIELD OVERWRITE nodeCount ON generation_manifest TYPE int;
DEFINE FIELD OVERWRITE relationCount ON generation_manifest TYPE int;
DEFINE FIELD OVERWRITE fileCount ON generation_manifest TYPE int;
DEFINE FIELD OVERWRITE metadataJson ON generation_manifest TYPE string;
DEFINE FIELD OVERWRITE cqlSchemaFingerprint ON generation_manifest TYPE string;
DEFINE FIELD OVERWRITE projectedBytes ON generation_manifest TYPE int;
DEFINE FIELD OVERWRITE complete ON generation_manifest TYPE bool;

DEFINE TABLE OVERWRITE code_file SCHEMAFULL;
DEFINE FIELD OVERWRITE recordKey ON code_file TYPE string;
DEFINE FIELD OVERWRITE repositoryId ON code_file TYPE string;
DEFINE FIELD OVERWRITE generationId ON code_file TYPE string;
DEFINE FIELD OVERWRITE path ON code_file TYPE string;
DEFINE FIELD OVERWRITE payloadJson ON code_file TYPE string;
DEFINE INDEX OVERWRITE code_file_projection ON TABLE code_file FIELDS repositoryId, generationId, path;

DEFINE TABLE OVERWRITE repository_pointer SCHEMAFULL;
DEFINE FIELD OVERWRITE repositoryId ON repository_pointer TYPE string;
DEFINE FIELD OVERWRITE generationId ON repository_pointer TYPE string;
DEFINE FIELD OVERWRITE schemaVersion ON repository_pointer TYPE string;
DEFINE FIELD OVERWRITE projectionFingerprint ON repository_pointer TYPE string;

DEFINE TABLE OVERWRITE code_node SCHEMAFULL;
DEFINE FIELD OVERWRITE recordKey ON code_node TYPE string;
DEFINE FIELD OVERWRITE repositoryId ON code_node TYPE string;
DEFINE FIELD OVERWRITE generationId ON code_node TYPE string;
DEFINE FIELD OVERWRITE schemaVersion ON code_node TYPE string;
DEFINE FIELD OVERWRITE ordinal ON code_node TYPE int;
DEFINE FIELD OVERWRITE compassNodeId ON code_node TYPE string;
DEFINE FIELD OVERWRITE kind ON code_node TYPE string;
DEFINE FIELD OVERWRITE cqlLabel ON code_node TYPE string;
DEFINE FIELD OVERWRITE name ON code_node TYPE string;
DEFINE FIELD OVERWRITE qualifiedName ON code_node TYPE string;
DEFINE FIELD OVERWRITE normalizedNames ON code_node TYPE array<string>;
DEFINE FIELD OVERWRITE searchTerms ON code_node TYPE array<string>;
DEFINE FIELD OVERWRITE searchPrefixes ON code_node TYPE array<string>;
DEFINE FIELD OVERWRITE roles ON code_node TYPE array<string>;
DEFINE FIELD OVERWRITE scope ON code_node TYPE option<string>;
DEFINE FIELD OVERWRITE language ON code_node TYPE option<string>;
DEFINE FIELD OVERWRITE sourcePath ON code_node TYPE option<string>;
DEFINE FIELD OVERWRITE confidence ON code_node TYPE string;
DEFINE FIELD OVERWRITE heuristic ON code_node TYPE bool;
DEFINE FIELD OVERWRITE payload ON code_node TYPE object FLEXIBLE;
DEFINE FIELD OVERWRITE payloadJson ON code_node TYPE string;
DEFINE INDEX OVERWRITE code_node_projection ON TABLE code_node FIELDS repositoryId, generationId, compassNodeId;
DEFINE INDEX OVERWRITE code_node_cql_order ON TABLE code_node FIELDS repositoryId, generationId, ordinal;
DEFINE INDEX OVERWRITE code_node_cql_label ON TABLE code_node FIELDS repositoryId, generationId, cqlLabel, ordinal;
DEFINE INDEX OVERWRITE code_node_cql_name ON TABLE code_node FIELDS repositoryId, generationId, name, ordinal;
DEFINE INDEX OVERWRITE code_node_cql_source ON TABLE code_node FIELDS repositoryId, generationId, sourcePath, ordinal;
DEFINE INDEX OVERWRITE code_node_normalized_names ON TABLE code_node FIELDS repositoryId, generationId, normalizedNames[*];
DEFINE INDEX OVERWRITE code_node_search_terms ON TABLE code_node FIELDS repositoryId, generationId, searchTerms[*];
DEFINE INDEX OVERWRITE code_node_search_prefixes ON TABLE code_node FIELDS repositoryId, generationId, searchPrefixes[*];
DEFINE INDEX OVERWRITE code_node_kind_scope ON TABLE code_node FIELDS repositoryId, generationId, kind, cqlLabel, scope;

DEFINE TABLE OVERWRITE structural_relation TYPE RELATION IN code_node OUT code_node SCHEMAFULL;
DEFINE TABLE OVERWRITE dependency_relation TYPE RELATION IN code_node OUT code_node SCHEMAFULL;
DEFINE TABLE OVERWRITE execution_relation TYPE RELATION IN code_node OUT code_node SCHEMAFULL;
DEFINE TABLE OVERWRITE data_flow_relation TYPE RELATION IN code_node OUT code_node SCHEMAFULL;
DEFINE TABLE OVERWRITE evidence_relation TYPE RELATION IN code_node OUT code_node SCHEMAFULL;

DEFINE FIELD OVERWRITE recordKey ON structural_relation TYPE string;
DEFINE FIELD OVERWRITE repositoryId ON structural_relation TYPE string;
DEFINE FIELD OVERWRITE generationId ON structural_relation TYPE string;
DEFINE FIELD OVERWRITE schemaVersion ON structural_relation TYPE string;
DEFINE FIELD OVERWRITE ordinal ON structural_relation TYPE int;
DEFINE FIELD OVERWRITE compassEdgeId ON structural_relation TYPE string;
DEFINE FIELD OVERWRITE family ON structural_relation TYPE string;
DEFINE FIELD OVERWRITE kind ON structural_relation TYPE string;
DEFINE FIELD OVERWRITE cqlType ON structural_relation TYPE string;
DEFINE FIELD OVERWRITE sourceNodeId ON structural_relation TYPE string;
DEFINE FIELD OVERWRITE targetNodeId ON structural_relation TYPE string;
DEFINE FIELD OVERWRITE sourceRecordKey ON structural_relation TYPE string;
DEFINE FIELD OVERWRITE targetRecordKey ON structural_relation TYPE string;
DEFINE FIELD OVERWRITE sourcePath ON structural_relation TYPE option<string>;
DEFINE FIELD OVERWRITE confidence ON structural_relation TYPE string;
DEFINE FIELD OVERWRITE heuristic ON structural_relation TYPE bool;
DEFINE FIELD OVERWRITE payload ON structural_relation TYPE object FLEXIBLE;
DEFINE FIELD OVERWRITE payloadJson ON structural_relation TYPE string;

DEFINE FIELD OVERWRITE recordKey ON dependency_relation TYPE string;
DEFINE FIELD OVERWRITE repositoryId ON dependency_relation TYPE string;
DEFINE FIELD OVERWRITE generationId ON dependency_relation TYPE string;
DEFINE FIELD OVERWRITE schemaVersion ON dependency_relation TYPE string;
DEFINE FIELD OVERWRITE ordinal ON dependency_relation TYPE int;
DEFINE FIELD OVERWRITE compassEdgeId ON dependency_relation TYPE string;
DEFINE FIELD OVERWRITE family ON dependency_relation TYPE string;
DEFINE FIELD OVERWRITE kind ON dependency_relation TYPE string;
DEFINE FIELD OVERWRITE cqlType ON dependency_relation TYPE string;
DEFINE FIELD OVERWRITE sourceNodeId ON dependency_relation TYPE string;
DEFINE FIELD OVERWRITE targetNodeId ON dependency_relation TYPE string;
DEFINE FIELD OVERWRITE sourceRecordKey ON dependency_relation TYPE string;
DEFINE FIELD OVERWRITE targetRecordKey ON dependency_relation TYPE string;
DEFINE FIELD OVERWRITE sourcePath ON dependency_relation TYPE option<string>;
DEFINE FIELD OVERWRITE confidence ON dependency_relation TYPE string;
DEFINE FIELD OVERWRITE heuristic ON dependency_relation TYPE bool;
DEFINE FIELD OVERWRITE payload ON dependency_relation TYPE object FLEXIBLE;
DEFINE FIELD OVERWRITE payloadJson ON dependency_relation TYPE string;

DEFINE FIELD OVERWRITE recordKey ON execution_relation TYPE string;
DEFINE FIELD OVERWRITE repositoryId ON execution_relation TYPE string;
DEFINE FIELD OVERWRITE generationId ON execution_relation TYPE string;
DEFINE FIELD OVERWRITE schemaVersion ON execution_relation TYPE string;
DEFINE FIELD OVERWRITE ordinal ON execution_relation TYPE int;
DEFINE FIELD OVERWRITE compassEdgeId ON execution_relation TYPE string;
DEFINE FIELD OVERWRITE family ON execution_relation TYPE string;
DEFINE FIELD OVERWRITE kind ON execution_relation TYPE string;
DEFINE FIELD OVERWRITE cqlType ON execution_relation TYPE string;
DEFINE FIELD OVERWRITE sourceNodeId ON execution_relation TYPE string;
DEFINE FIELD OVERWRITE targetNodeId ON execution_relation TYPE string;
DEFINE FIELD OVERWRITE sourceRecordKey ON execution_relation TYPE string;
DEFINE FIELD OVERWRITE targetRecordKey ON execution_relation TYPE string;
DEFINE FIELD OVERWRITE sourcePath ON execution_relation TYPE option<string>;
DEFINE FIELD OVERWRITE confidence ON execution_relation TYPE string;
DEFINE FIELD OVERWRITE heuristic ON execution_relation TYPE bool;
DEFINE FIELD OVERWRITE payload ON execution_relation TYPE object FLEXIBLE;
DEFINE FIELD OVERWRITE payloadJson ON execution_relation TYPE string;

DEFINE FIELD OVERWRITE recordKey ON data_flow_relation TYPE string;
DEFINE FIELD OVERWRITE repositoryId ON data_flow_relation TYPE string;
DEFINE FIELD OVERWRITE generationId ON data_flow_relation TYPE string;
DEFINE FIELD OVERWRITE schemaVersion ON data_flow_relation TYPE string;
DEFINE FIELD OVERWRITE ordinal ON data_flow_relation TYPE int;
DEFINE FIELD OVERWRITE compassEdgeId ON data_flow_relation TYPE string;
DEFINE FIELD OVERWRITE family ON data_flow_relation TYPE string;
DEFINE FIELD OVERWRITE kind ON data_flow_relation TYPE string;
DEFINE FIELD OVERWRITE cqlType ON data_flow_relation TYPE string;
DEFINE FIELD OVERWRITE sourceNodeId ON data_flow_relation TYPE string;
DEFINE FIELD OVERWRITE targetNodeId ON data_flow_relation TYPE string;
DEFINE FIELD OVERWRITE sourceRecordKey ON data_flow_relation TYPE string;
DEFINE FIELD OVERWRITE targetRecordKey ON data_flow_relation TYPE string;
DEFINE FIELD OVERWRITE sourcePath ON data_flow_relation TYPE option<string>;
DEFINE FIELD OVERWRITE confidence ON data_flow_relation TYPE string;
DEFINE FIELD OVERWRITE heuristic ON data_flow_relation TYPE bool;
DEFINE FIELD OVERWRITE payload ON data_flow_relation TYPE object FLEXIBLE;
DEFINE FIELD OVERWRITE payloadJson ON data_flow_relation TYPE string;

DEFINE FIELD OVERWRITE recordKey ON evidence_relation TYPE string;
DEFINE FIELD OVERWRITE repositoryId ON evidence_relation TYPE string;
DEFINE FIELD OVERWRITE generationId ON evidence_relation TYPE string;
DEFINE FIELD OVERWRITE schemaVersion ON evidence_relation TYPE string;
DEFINE FIELD OVERWRITE ordinal ON evidence_relation TYPE int;
DEFINE FIELD OVERWRITE compassEdgeId ON evidence_relation TYPE string;
DEFINE FIELD OVERWRITE family ON evidence_relation TYPE string;
DEFINE FIELD OVERWRITE kind ON evidence_relation TYPE string;
DEFINE FIELD OVERWRITE cqlType ON evidence_relation TYPE string;
DEFINE FIELD OVERWRITE sourceNodeId ON evidence_relation TYPE string;
DEFINE FIELD OVERWRITE targetNodeId ON evidence_relation TYPE string;
DEFINE FIELD OVERWRITE sourceRecordKey ON evidence_relation TYPE string;
DEFINE FIELD OVERWRITE targetRecordKey ON evidence_relation TYPE string;
DEFINE FIELD OVERWRITE sourcePath ON evidence_relation TYPE option<string>;
DEFINE FIELD OVERWRITE confidence ON evidence_relation TYPE string;
DEFINE FIELD OVERWRITE heuristic ON evidence_relation TYPE bool;
DEFINE FIELD OVERWRITE payload ON evidence_relation TYPE object FLEXIBLE;
DEFINE FIELD OVERWRITE payloadJson ON evidence_relation TYPE string;

DEFINE INDEX OVERWRITE structural_relation_projection ON TABLE structural_relation FIELDS repositoryId, generationId, cqlType, compassEdgeId;
DEFINE INDEX OVERWRITE structural_relation_cql_order ON TABLE structural_relation FIELDS repositoryId, generationId, ordinal;
DEFINE INDEX OVERWRITE dependency_relation_projection ON TABLE dependency_relation FIELDS repositoryId, generationId, cqlType, compassEdgeId;
DEFINE INDEX OVERWRITE dependency_relation_cql_order ON TABLE dependency_relation FIELDS repositoryId, generationId, ordinal;
DEFINE INDEX OVERWRITE execution_relation_projection ON TABLE execution_relation FIELDS repositoryId, generationId, cqlType, compassEdgeId;
DEFINE INDEX OVERWRITE execution_relation_cql_order ON TABLE execution_relation FIELDS repositoryId, generationId, ordinal;
DEFINE INDEX OVERWRITE data_flow_relation_projection ON TABLE data_flow_relation FIELDS repositoryId, generationId, cqlType, compassEdgeId;
DEFINE INDEX OVERWRITE data_flow_relation_cql_order ON TABLE data_flow_relation FIELDS repositoryId, generationId, ordinal;
DEFINE INDEX OVERWRITE evidence_relation_projection ON TABLE evidence_relation FIELDS repositoryId, generationId, cqlType, compassEdgeId;
DEFINE INDEX OVERWRITE evidence_relation_cql_order ON TABLE evidence_relation FIELDS repositoryId, generationId, ordinal;
DEFINE INDEX OVERWRITE structural_relation_incoming ON TABLE structural_relation FIELDS repositoryId, generationId, targetNodeId, kind, compassEdgeId;
DEFINE INDEX OVERWRITE structural_relation_outgoing ON TABLE structural_relation FIELDS repositoryId, generationId, sourceNodeId, kind, compassEdgeId;
DEFINE INDEX OVERWRITE dependency_relation_incoming ON TABLE dependency_relation FIELDS repositoryId, generationId, targetNodeId, kind, compassEdgeId;
DEFINE INDEX OVERWRITE dependency_relation_outgoing ON TABLE dependency_relation FIELDS repositoryId, generationId, sourceNodeId, kind, compassEdgeId;
DEFINE INDEX OVERWRITE execution_relation_incoming ON TABLE execution_relation FIELDS repositoryId, generationId, targetNodeId, kind, compassEdgeId;
DEFINE INDEX OVERWRITE execution_relation_outgoing ON TABLE execution_relation FIELDS repositoryId, generationId, sourceNodeId, kind, compassEdgeId;
DEFINE INDEX OVERWRITE data_flow_relation_incoming ON TABLE data_flow_relation FIELDS repositoryId, generationId, targetNodeId, kind, compassEdgeId;
DEFINE INDEX OVERWRITE data_flow_relation_outgoing ON TABLE data_flow_relation FIELDS repositoryId, generationId, sourceNodeId, kind, compassEdgeId;
DEFINE INDEX OVERWRITE evidence_relation_incoming ON TABLE evidence_relation FIELDS repositoryId, generationId, targetNodeId, kind, compassEdgeId;
DEFINE INDEX OVERWRITE evidence_relation_outgoing ON TABLE evidence_relation FIELDS repositoryId, generationId, sourceNodeId, kind, compassEdgeId;
"#;

const UPSERT_RECORD: &str = "UPSERT $record CONTENT $payload";
const CLAIM_GENERATION: &str = "INSERT INTO generation_manifest $payload ON DUPLICATE KEY UPDATE repositoryId = repositoryId RETURN NONE";
const INSERT_NODE_BATCH: &str = r#"INSERT INTO code_node $payloads ON DUPLICATE KEY UPDATE
recordKey = $input.recordKey, repositoryId = $input.repositoryId,
generationId = $input.generationId, schemaVersion = $input.schemaVersion, ordinal = $input.ordinal,
compassNodeId = $input.compassNodeId, kind = $input.kind, cqlLabel = $input.cqlLabel, name = $input.name,
qualifiedName = $input.qualifiedName, normalizedNames = $input.normalizedNames,
searchTerms = $input.searchTerms, searchPrefixes = $input.searchPrefixes, roles = $input.roles, scope = $input.scope,
language = $input.language, sourcePath = $input.sourcePath,
confidence = $input.confidence, heuristic = $input.heuristic,
payload = $input.payload, payloadJson = $input.payloadJson RETURN NONE"#;

macro_rules! relation_insert_statement {
    ($table:literal) => {
        concat!(
            "INSERT RELATION INTO ",
            $table,
            " $payloads ON DUPLICATE KEY UPDATE ",
            "in = $input.in, out = $input.out, recordKey = $input.recordKey, ",
            "repositoryId = $input.repositoryId, generationId = $input.generationId, ",
            "schemaVersion = $input.schemaVersion, ordinal = $input.ordinal, compassEdgeId = $input.compassEdgeId, ",
            "family = $input.family, kind = $input.kind, cqlType = $input.cqlType, sourceNodeId = $input.sourceNodeId, ",
            "targetNodeId = $input.targetNodeId, sourceRecordKey = $input.sourceRecordKey, ",
            "targetRecordKey = $input.targetRecordKey, sourcePath = $input.sourcePath, ",
            "confidence = $input.confidence, heuristic = $input.heuristic, ",
            "payload = $input.payload, payloadJson = $input.payloadJson RETURN NONE"
        )
    };
}

const INSERT_STRUCTURAL_RELATION_BATCH: &str = relation_insert_statement!("structural_relation");
const INSERT_DEPENDENCY_RELATION_BATCH: &str = relation_insert_statement!("dependency_relation");
const INSERT_EXECUTION_RELATION_BATCH: &str = relation_insert_statement!("execution_relation");
const INSERT_DATA_FLOW_RELATION_BATCH: &str = relation_insert_statement!("data_flow_relation");
const INSERT_EVIDENCE_RELATION_BATCH: &str = relation_insert_statement!("evidence_relation");
const PROJECTION_WRITE_BATCH: usize = 512;
const INSERT_FILE_BATCH: &str = "INSERT INTO code_file $payloads ON DUPLICATE KEY UPDATE recordKey = $input.recordKey, repositoryId = $input.repositoryId, generationId = $input.generationId, path = $input.path, payloadJson = $input.payloadJson";
const SELECT_FILES: &str = "SELECT * OMIT id FROM code_file WHERE repositoryId = $repository AND generationId = $generation ORDER BY path LIMIT $limit";
const SELECT_NODES: &str = "SELECT * OMIT id FROM code_node WHERE repositoryId = $repository AND generationId = $generation ORDER BY compassNodeId LIMIT $limit";
const SELECT_RELATIONS: &str = "SELECT * OMIT id, in, out FROM type::table($table) WHERE repositoryId = $repository AND generationId = $generation ORDER BY compassEdgeId LIMIT $limit";
const SELECT_GENERATION_MANIFESTS: &str = "SELECT * OMIT id FROM generation_manifest WHERE repositoryId = $repository AND generationId NOT IN $retained ORDER BY generationId LIMIT $limit";
const DELETE_GENERATION_RECORDS: &str =
    "DELETE type::table($table) WHERE repositoryId = $repository AND generationId = $generation";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActivationOutcome {
    pub generation_id: String,
    pub nodes: usize,
    pub relations: usize,
    pub already_present: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GenerationGcStats {
    pub generations_deleted: usize,
    pub incomplete_generations_deleted: usize,
}

/// Test fault-injection point counted across node and relation mutations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InterruptAfter(pub usize);

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GenerationManifest {
    directed: bool,
    multigraph: bool,
    repository_id: String,
    generation_id: String,
    schema_version: String,
    projection_fingerprint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    graph_digest: Option<String>,
    source_tree_digest: String,
    schema_fingerprint: String,
    node_count: usize,
    relation_count: usize,
    file_count: usize,
    metadata_json: String,
    cql_schema_fingerprint: String,
    projected_bytes: u64,
    complete: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ActivePointer {
    repository_id: String,
    generation_id: String,
    schema_version: String,
    projection_fingerprint: String,
}

/// A local SurrealDB projection client with a closed operation surface.
#[derive(Clone)]
pub struct SurrealProjection {
    database: BoundedDatabase,
    limits: ProjectionLimits,
    namespace: String,
    database_name: String,
    storage_identity: Option<(SurrealEngine, std::path::PathBuf)>,
}

impl SurrealProjection {
    /// Open exactly the reference's connection; read paths never define schema.
    pub async fn open_reference(
        reference: &SurrealRef,
        create: bool,
    ) -> Result<Self, ProjectionError> {
        reference.validate()?;
        match reference.engine {
            #[cfg(feature = "surrealkv")]
            SurrealEngine::SurrealKv => {
                if create {
                    Self::surrealkv(
                        &reference.location,
                        &reference.namespace,
                        &reference.database,
                    )
                    .await
                } else {
                    Self::surrealkv_existing(
                        &reference.location,
                        &reference.namespace,
                        &reference.database,
                    )
                    .await
                }
            }
            #[cfg(feature = "rocksdb")]
            SurrealEngine::RocksDb => {
                if create {
                    Self::rocksdb(
                        &reference.location,
                        &reference.namespace,
                        &reference.database,
                    )
                    .await
                } else {
                    Self::rocksdb_existing(
                        &reference.location,
                        &reference.namespace,
                        &reference.database,
                    )
                    .await
                }
            }
            #[cfg(feature = "remote")]
            SurrealEngine::Remote => {
                let client = remote::client(reference).await?;
                let mut projection = if create {
                    Self::initialize(
                        client,
                        &reference.namespace,
                        &reference.database,
                        ProjectionLimits::default(),
                    )
                    .await?
                } else {
                    Self::select_database(
                        client,
                        &reference.namespace,
                        &reference.database,
                        ProjectionLimits::default(),
                    )
                    .await?
                };
                projection.storage_identity =
                    Some((SurrealEngine::Remote, reference.connection_location()?));
                Ok(projection)
            }
            #[allow(unreachable_patterns)]
            _ => Err(ProjectionError::InvalidReference(format!(
                "Surreal {} support was not compiled in",
                reference.engine.as_str()
            ))),
        }
    }

    #[cfg(feature = "mem")]
    pub async fn memory(namespace: &str, database: &str) -> Result<Self, ProjectionError> {
        Self::memory_with_limits(namespace, database, ProjectionLimits::default()).await
    }

    #[cfg(feature = "mem")]
    pub async fn memory_with_limits(
        namespace: &str,
        database: &str,
        limits: ProjectionLimits,
    ) -> Result<Self, ProjectionError> {
        let client = surrealdb::engine::any::connect("mem://")
            .await
            .map_err(|error| database_error("connect_mem", error))?;
        Self::initialize(client, namespace, database, limits).await
    }

    #[cfg(feature = "surrealkv")]
    pub async fn surrealkv(
        path: &str,
        namespace: &str,
        database: &str,
    ) -> Result<Self, ProjectionError> {
        Self::surrealkv_with_limits(path, namespace, database, ProjectionLimits::default()).await
    }

    #[cfg(feature = "surrealkv")]
    pub async fn surrealkv_with_limits(
        path: &str,
        namespace: &str,
        database: &str,
        limits: ProjectionLimits,
    ) -> Result<Self, ProjectionError> {
        let client = persistent::client(SurrealEngine::SurrealKv, path, true).await?;
        Self::initialize(client, namespace, database, limits)
            .await?
            .with_storage_identity(SurrealEngine::SurrealKv, path)
    }

    /// Open an already-published SurrealKV projection without issuing schema
    /// definition statements. Query and validation paths use this constructor
    /// so opening an immutable generation does not mutate its database schema.
    #[cfg(feature = "surrealkv")]
    pub async fn surrealkv_existing(
        path: &str,
        namespace: &str,
        database: &str,
    ) -> Result<Self, ProjectionError> {
        validate_existing_storage_path(path)?;
        let client = persistent::client(SurrealEngine::SurrealKv, path, false).await?;
        Self::select_database(client, namespace, database, ProjectionLimits::default())
            .await?
            .with_storage_identity(SurrealEngine::SurrealKv, path)
    }

    #[cfg(feature = "rocksdb")]
    pub async fn rocksdb(
        path: &str,
        namespace: &str,
        database: &str,
    ) -> Result<Self, ProjectionError> {
        Self::rocksdb_with_limits(path, namespace, database, ProjectionLimits::default()).await
    }

    #[cfg(feature = "rocksdb")]
    pub async fn rocksdb_with_limits(
        path: &str,
        namespace: &str,
        database: &str,
        limits: ProjectionLimits,
    ) -> Result<Self, ProjectionError> {
        let client = persistent::client(SurrealEngine::RocksDb, path, true).await?;
        Self::initialize(client, namespace, database, limits)
            .await?
            .with_storage_identity(SurrealEngine::RocksDb, path)
    }

    /// Open an already-published RocksDB projection without issuing schema
    /// definition statements.
    #[cfg(feature = "rocksdb")]
    pub async fn rocksdb_existing(
        path: &str,
        namespace: &str,
        database: &str,
    ) -> Result<Self, ProjectionError> {
        validate_existing_storage_path(path)?;
        let client = persistent::client(SurrealEngine::RocksDb, path, false).await?;
        Self::select_database(client, namespace, database, ProjectionLimits::default())
            .await?
            .with_storage_identity(SurrealEngine::RocksDb, path)
    }

    async fn initialize(
        client: Surreal<Any>,
        namespace: &str,
        database: &str,
        limits: ProjectionLimits,
    ) -> Result<Self, ProjectionError> {
        let projection = Self::select_database(client, namespace, database, limits).await?;
        projection
            .database
            .query(SCHEMA)
            .await
            .and_then(IndexedResults::check)
            .map_err(|error| database_error("define_schema", error))?;
        Ok(projection)
    }

    async fn select_database(
        client: Surreal<Any>,
        namespace: &str,
        database: &str,
        limits: ProjectionLimits,
    ) -> Result<Self, ProjectionError> {
        if namespace.trim().is_empty() || database.trim().is_empty() {
            return Err(ProjectionError::InvalidPlan(
                "namespace and database must not be empty".to_owned(),
            ));
        }
        tokio::time::timeout(
            std::time::Duration::from_secs(30),
            client.use_ns(namespace).use_db(database).into_future(),
        )
        .await
        .map_err(|_| database_error("select_namespace_database", "selection exceeded 30 seconds"))?
        .map_err(|error| database_error("select_namespace_database", error))?;
        Ok(Self {
            database: BoundedDatabase::new(client),
            limits,
            namespace: namespace.to_owned(),
            database_name: database.to_owned(),
            storage_identity: None,
        })
    }

    #[cfg(any(feature = "surrealkv", feature = "rocksdb"))]
    fn with_storage_identity(
        mut self,
        engine: SurrealEngine,
        path: &str,
    ) -> Result<Self, ProjectionError> {
        let path = std::fs::canonicalize(path).map_err(|error| {
            ProjectionError::InvalidReference(format!("cannot resolve store location: {error}"))
        })?;
        self.storage_identity = Some((engine, path));
        Ok(self)
    }

    pub async fn activate(
        &self,
        plan: &ProjectionPlan,
    ) -> Result<ActivationOutcome, ProjectionError> {
        self.activate_with_interrupt(plan, None).await
    }

    pub async fn activate_with_interrupt(
        &self,
        plan: &ProjectionPlan,
        interrupt: Option<InterruptAfter>,
    ) -> Result<ActivationOutcome, ProjectionError> {
        let outcome = self.stage_with_interrupt(plan, interrupt).await?;
        upsert_pointer(&self.database, plan).await?;
        Ok(outcome)
    }

    /// Stage an unbound library projection without changing the legacy database
    /// pointer. Filesystem publication must use `stage_for_reference` to bind its
    /// admitted canonical artifact digest before committing `surreal.ref`.
    pub async fn stage(&self, plan: &ProjectionPlan) -> Result<ActivationOutcome, ProjectionError> {
        self.stage_with_interrupt(plan, None).await
    }

    pub async fn stage_with_interrupt(
        &self,
        plan: &ProjectionPlan,
        interrupt: Option<InterruptAfter>,
    ) -> Result<ActivationOutcome, ProjectionError> {
        plan.validate()?;
        let projected_bytes = self.limits.validate_plan(plan)?;
        stage_generation(&self.database, plan, projected_bytes, interrupt, None).await
    }

    /// Stage a generation together with the publisher's admitted graph digest.
    /// The digest is immutable generation metadata, checked by every pinned
    /// reader without opening canonical JSON. Connection locations are validated
    /// at query open, not here, so portable bundle restore can change location.
    pub async fn stage_for_reference(
        &self,
        plan: &ProjectionPlan,
        reference: &SurrealRef,
    ) -> Result<ActivationOutcome, ProjectionError> {
        reference.validate_plan(plan)?;
        plan.validate()?;
        let projected_bytes = self.limits.validate_plan(plan)?;
        stage_generation(
            &self.database,
            plan,
            projected_bytes,
            None,
            Some(&reference.graph_digest),
        )
        .await
    }

    /// Validate the exact immutable generation named by a filesystem
    /// reference. The mutable repository pointer is intentionally ignored.
    pub async fn validate_reference(&self, reference: &SurrealRef) -> Result<(), ProjectionError> {
        reference.validate()?;
        let manifest = self.manifest_for_reference(reference).await?;
        if manifest.projection_fingerprint != reference.projection_fingerprint
            || u64::try_from(manifest.file_count).unwrap_or(u64::MAX) != reference.file_count
            || u64::try_from(manifest.node_count).unwrap_or(u64::MAX) != reference.node_count
            || u64::try_from(manifest.relation_count).unwrap_or(u64::MAX)
                != reference.relation_count
        {
            return Err(ProjectionError::InvalidReference(
                "generation manifest does not match surreal.ref".to_owned(),
            ));
        }
        validate_manifest_limits(&manifest, self.limits)
    }

    /// Validate every persisted payload against the generation fingerprint.
    /// Operational validation and repair use this bounded full audit; ordinary
    /// indexed queries only validate the reference/manifest before row reads.
    pub async fn validate_contents(&self, reference: &SurrealRef) -> Result<(), ProjectionError> {
        self.validate_reference(reference).await?;
        let plan = self
            .read_generation_plan(&reference.repository_id, &reference.generation_id)
            .await?;
        if plan.projection_fingerprint != reference.projection_fingerprint {
            return Err(ProjectionError::FingerprintMismatch);
        }
        Ok(())
    }

    pub async fn read_generation_projection(
        &self,
        reference: &SurrealRef,
    ) -> Result<ProjectionPlan, ProjectionError> {
        self.validate_reference(reference).await?;
        self.read_generation_plan(&reference.repository_id, &reference.generation_id)
            .await
    }

    pub async fn export_bundle(
        &self,
        reference: &SurrealRef,
    ) -> Result<ProjectionBundle, ProjectionError> {
        let plan = self.read_generation_projection(reference).await?;
        ProjectionBundle::new(reference.clone(), plan)
    }

    pub async fn restore_bundle(
        &self,
        bundle: &ProjectionBundle,
    ) -> Result<ActivationOutcome, ProjectionError> {
        bundle.validate()?;
        self.stage_for_reference(&bundle.plan, &bundle.reference)
            .await
    }

    /// Reclaim a bounded number of one repository's generations not retained by
    /// filesystem references. Other repositories in the same store are never
    /// candidates; retained generations are excluded before the candidate limit.
    pub async fn garbage_collect_generations(
        &self,
        repository_id: &str,
        retained: &[SurrealRef],
        max_generations: usize,
    ) -> Result<GenerationGcStats, ProjectionError> {
        if repository_id.trim().is_empty() {
            return Err(ProjectionError::EmptyRepositoryId);
        }
        if max_generations == 0 {
            return Err(ProjectionError::InvalidPlan(
                "Surreal generation GC bound must be greater than zero".to_owned(),
            ));
        }
        if max_generations > self.limits.max_nodes() || retained.len() > self.limits.max_nodes() {
            return Err(ProjectionError::LimitExceeded {
                resource: "generation GC candidates or retained references",
                actual: max_generations.max(retained.len()) as u64,
                limit: self.limits.max_nodes() as u64,
            });
        }
        let retained = retained
            .iter()
            .map(|reference| {
                reference.validate()?;
                if reference.repository_id != repository_id {
                    return Err(ProjectionError::InvalidReference(
                        "GC retained reference belongs to another repository".to_owned(),
                    ));
                }
                Ok(reference.generation_id.clone())
            })
            .collect::<Result<BTreeSet<_>, ProjectionError>>()?;
        let mut response = self
            .database
            .query(SELECT_GENERATION_MANIFESTS)
            .bind(("repository", repository_id))
            .bind(("retained", retained.iter().cloned().collect::<Vec<_>>()))
            .bind(("limit", max_generations))
            .await
            .map_err(|error| database_error("list_generation_manifests", error))?;
        let values: Vec<Value> = response
            .take(0)
            .map_err(|error| database_error("list_generation_manifests", error))?;
        let manifests = values
            .into_iter()
            .map(serde_json::from_value)
            .collect::<Result<Vec<GenerationManifest>, _>>()?;
        let mut stats = GenerationGcStats::default();
        for manifest in manifests.into_iter().take(max_generations) {
            if manifest.repository_id != repository_id || retained.contains(&manifest.generation_id)
            {
                return Err(ProjectionError::InvalidPlan(
                    "GC candidate is outside the requested repository or is retained".to_owned(),
                ));
            }
            for table in ["code_node", "code_file"]
                .into_iter()
                .chain(RelationFamily::ALL.into_iter().map(RelationFamily::as_str))
            {
                self.database
                    .query(DELETE_GENERATION_RECORDS)
                    .bind(("table", table))
                    .bind(("repository", manifest.repository_id.as_str()))
                    .bind(("generation", manifest.generation_id.as_str()))
                    .await
                    .and_then(IndexedResults::check)
                    .map_err(|error| database_error("delete_generation_records", error))?;
            }
            self.database
                .delete(RecordId::new(
                    "generation_manifest",
                    manifest_key(&manifest.repository_id, &manifest.generation_id),
                ))
                .await
                .map_err(|error| database_error("delete_generation_manifest", error))?;
            stats.generations_deleted = stats.generations_deleted.saturating_add(1);
            if !manifest.complete {
                stats.incomplete_generations_deleted =
                    stats.incomplete_generations_deleted.saturating_add(1);
            }
        }
        Ok(stats)
    }

    pub async fn active_generation(
        &self,
        repository_id: &str,
    ) -> Result<Option<String>, ProjectionError> {
        let pointer = select_record::<ActivePointer>(
            &self.database,
            RecordId::new("repository_pointer", pointer_key(repository_id)),
            "read_active_pointer",
        )
        .await?;
        Ok(pointer.map(|pointer| pointer.generation_id))
    }

    pub async fn read_active_projection(
        &self,
        repository_id: &str,
    ) -> Result<Option<ProjectionPlan>, ProjectionError> {
        let Some(pointer) = select_record::<ActivePointer>(
            &self.database,
            RecordId::new("repository_pointer", pointer_key(repository_id)),
            "read_active_pointer",
        )
        .await?
        else {
            return Ok(None);
        };
        let manifest = select_record::<GenerationManifest>(
            &self.database,
            RecordId::new(
                "generation_manifest",
                manifest_key(repository_id, &pointer.generation_id),
            ),
            "read_generation_manifest",
        )
        .await?
        .ok_or_else(|| {
            ProjectionError::InvalidPlan("active generation manifest is missing".to_owned())
        })?;
        if !manifest.complete || manifest.schema_version != PROJECTION_SCHEMA_V2 {
            return Err(ProjectionError::InvalidPlan(
                "active generation manifest is incomplete or unsupported".to_owned(),
            ));
        }
        validate_manifest_limits(&manifest, self.limits)?;
        self.read_generation_plan(repository_id, &pointer.generation_id)
            .await
            .map(Some)
    }

    async fn manifest_for_reference(
        &self,
        reference: &SurrealRef,
    ) -> Result<GenerationManifest, ProjectionError> {
        reference.validate()?;
        let location = reference.connection_location()?;
        if reference.namespace != self.namespace
            || reference.database != self.database_name
            || self.storage_identity.as_ref() != Some(&(reference.engine, location))
        {
            return Err(ProjectionError::InvalidReference(
                "reference engine, location, namespace, or database does not match the open connection"
                    .to_owned(),
            ));
        }
        let manifest = select_record::<GenerationManifest>(
            &self.database,
            RecordId::new(
                "generation_manifest",
                manifest_key(&reference.repository_id, &reference.generation_id),
            ),
            "read_generation_manifest",
        )
        .await?
        .ok_or_else(|| {
            ProjectionError::InvalidReference(
                "referenced Surreal generation manifest is missing".to_owned(),
            )
        })?;
        if !manifest.complete
            || manifest.schema_version != reference.projection_schema
            || manifest.repository_id != reference.repository_id
            || manifest.generation_id != reference.generation_id
            || manifest.projection_fingerprint != reference.projection_fingerprint
            || manifest.graph_digest.as_deref() != Some(reference.graph_digest.as_str())
            || u64::try_from(manifest.node_count).unwrap_or(u64::MAX) != reference.node_count
            || u64::try_from(manifest.relation_count).unwrap_or(u64::MAX)
                != reference.relation_count
            || u64::try_from(manifest.file_count).unwrap_or(u64::MAX) != reference.file_count
        {
            return Err(ProjectionError::InvalidReference(
                "referenced Surreal generation is incomplete or incompatible".to_owned(),
            ));
        }
        validate_manifest_limits(&manifest, self.limits)?;
        Ok(manifest)
    }

    async fn read_generation_plan(
        &self,
        repository_id: &str,
        generation_id: &str,
    ) -> Result<ProjectionPlan, ProjectionError> {
        let manifest = select_record::<GenerationManifest>(
            &self.database,
            RecordId::new(
                "generation_manifest",
                manifest_key(repository_id, generation_id),
            ),
            "read_generation_manifest",
        )
        .await?
        .ok_or_else(|| ProjectionError::InvalidPlan("generation manifest is missing".to_owned()))?;
        if !manifest.complete || manifest.schema_version != PROJECTION_SCHEMA_V2 {
            return Err(ProjectionError::InvalidPlan(
                "generation manifest is incomplete or unsupported".to_owned(),
            ));
        }
        validate_manifest_limits(&manifest, self.limits)?;
        let nodes = read_nodes(
            &self.database,
            repository_id,
            generation_id,
            self.limits.max_nodes(),
        )
        .await?;
        let files = read_files(
            &self.database,
            repository_id,
            generation_id,
            self.limits.max_nodes(),
        )
        .await?;
        let mut relations = Vec::new();
        for family in RelationFamily::ALL {
            let remaining = self.limits.max_relations().saturating_sub(relations.len());
            relations.extend(
                read_relations(
                    &self.database,
                    repository_id,
                    generation_id,
                    family,
                    remaining,
                )
                .await?,
            );
        }
        relations.sort_by(|left, right| left.compass_edge_id.cmp(&right.compass_edge_id));
        let plan = ProjectionPlan {
            directed: manifest.directed,
            multigraph: manifest.multigraph,
            repository_id: repository_id.to_owned(),
            generation_id: generation_id.to_owned(),
            schema_version: manifest.schema_version,
            source_tree_digest: manifest.source_tree_digest,
            schema_fingerprint: manifest.schema_fingerprint,
            projection_fingerprint: manifest.projection_fingerprint,
            metadata_json: manifest.metadata_json,
            cql_schema_fingerprint: manifest.cql_schema_fingerprint,
            files,
            nodes,
            relations,
        };
        plan.validate()?;
        let projected_bytes = self.limits.validate_plan(&plan)?;
        if plan.nodes.len() != manifest.node_count
            || plan.files.len() != manifest.file_count
            || plan.relations.len() != manifest.relation_count
            || projected_bytes != manifest.projected_bytes
        {
            return Err(ProjectionError::InvalidPlan(
                "active generation records do not match the manifest".to_owned(),
            ));
        }
        Ok(plan)
    }
}

#[cfg(any(feature = "surrealkv", feature = "rocksdb"))]
fn validate_existing_storage_path(path: &str) -> Result<(), ProjectionError> {
    let metadata = std::fs::metadata(path).map_err(|error| {
        ProjectionError::InvalidReference(format!(
            "embedded SurrealDB storage is unavailable: {error}"
        ))
    })?;
    if !metadata.is_dir() {
        return Err(ProjectionError::InvalidReference(
            "embedded SurrealDB storage must be an existing directory".to_owned(),
        ));
    }
    Ok(())
}

async fn stage_generation(
    database: &BoundedDatabase,
    plan: &ProjectionPlan,
    projected_bytes: u64,
    interrupt: Option<InterruptAfter>,
    graph_digest: Option<&str>,
) -> Result<ActivationOutcome, ProjectionError> {
    let manifest_id = RecordId::new(
        "generation_manifest",
        manifest_key(&plan.repository_id, &plan.generation_id),
    );
    let mut incomplete_manifest = GenerationManifest {
        directed: plan.directed,
        multigraph: plan.multigraph,
        repository_id: plan.repository_id.clone(),
        generation_id: plan.generation_id.clone(),
        schema_version: plan.schema_version.clone(),
        projection_fingerprint: plan.projection_fingerprint.clone(),
        graph_digest: graph_digest.map(str::to_owned),
        source_tree_digest: plan.source_tree_digest.clone(),
        schema_fingerprint: plan.schema_fingerprint.clone(),
        node_count: plan.nodes.len(),
        relation_count: plan.relations.len(),
        file_count: plan.files.len(),
        metadata_json: plan.metadata_json.clone(),
        cql_schema_fingerprint: plan.cql_schema_fingerprint.clone(),
        projected_bytes,
        complete: false,
    };
    claim_generation(database, manifest_id.clone(), &incomplete_manifest).await?;
    let existing = select_record::<GenerationManifest>(
        database,
        manifest_id.clone(),
        "read_candidate_manifest",
    )
    .await?
    .ok_or_else(|| ProjectionError::InvalidPlan("generation claim is missing".to_owned()))?;
    if !manifest_matches(&existing, &incomplete_manifest) {
        return Err(ProjectionError::InvalidPlan(
            "immutable generation already exists with different content".to_owned(),
        ));
    }
    if incomplete_manifest.graph_digest.is_none() {
        incomplete_manifest
            .graph_digest
            .clone_from(&existing.graph_digest);
    }
    if existing.complete {
        match validate_candidate(database, plan).await {
            Ok(()) => {
                return Ok(ActivationOutcome {
                    generation_id: plan.generation_id.clone(),
                    nodes: plan.nodes.len(),
                    relations: plan.relations.len(),
                    already_present: true,
                });
            }
            Err(ProjectionError::InvalidPlan(_)) | Err(ProjectionError::Serialization(_)) => {
                // Repair only the already-claimed exact content. Never let a
                // reader treat an incomplete repair as a complete generation.
                upsert_payload(
                    database,
                    manifest_id.clone(),
                    serde_json::to_value(&incomplete_manifest)?,
                    "mark_generation_for_repair",
                )
                .await?;
                for table in RelationFamily::ALL
                    .iter()
                    .map(|family| family.as_str())
                    .chain(["code_node", "code_file"])
                {
                    database.query("DELETE type::table($table) WHERE repositoryId = $repository AND generationId = $generation RETURN NONE")
                        .bind(("table", table)).bind(("repository", plan.repository_id.as_str())).bind(("generation", plan.generation_id.as_str()))
                        .await.and_then(IndexedResults::check).map_err(|error| database_error("clear_damaged_generation", error))?;
                }
            }
            Err(error) => return Err(error),
        }
    }

    let mut mutations = 0_usize;
    let mut file_offset = 0_usize;
    while file_offset < plan.files.len() {
        interrupt_if_requested(interrupt, mutations)?;
        let batch_len = write_batch_len(interrupt, mutations, plan.files.len() - file_offset);
        let payloads = plan.files[file_offset..file_offset + batch_len]
            .iter()
            .map(|file| {
                database_record_value(
                    serde_json::to_value(file)?,
                    RecordId::new("code_file", file.record_key.clone()),
                    None,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        insert_batch(database, INSERT_FILE_BATCH, payloads, "write_file_batch").await?;
        file_offset += batch_len;
        mutations += batch_len;
    }
    let mut node_offset = 0_usize;
    while node_offset < plan.nodes.len() {
        interrupt_if_requested(interrupt, mutations)?;
        let batch_len = write_batch_len(interrupt, mutations, plan.nodes.len() - node_offset);
        insert_node_batch(database, &plan.nodes[node_offset..node_offset + batch_len]).await?;
        node_offset += batch_len;
        mutations += batch_len;
    }
    let mut relation_offset = 0_usize;
    while relation_offset < plan.relations.len() {
        interrupt_if_requested(interrupt, mutations)?;
        let family = plan.relations[relation_offset].family;
        let family_remaining = plan.relations[relation_offset..]
            .iter()
            .take_while(|relation| relation.family == family)
            .count();
        let batch_len = write_batch_len(interrupt, mutations, family_remaining);
        insert_relation_batch(
            database,
            &plan.relations[relation_offset..relation_offset + batch_len],
        )
        .await?;
        relation_offset += batch_len;
        mutations += batch_len;
    }
    interrupt_if_requested(interrupt, mutations)?;
    validate_candidate(database, plan).await?;
    let manifest = GenerationManifest {
        complete: true,
        ..incomplete_manifest
    };
    upsert_payload(
        database,
        manifest_id,
        serde_json::to_value(manifest)?,
        "write_generation_manifest",
    )
    .await?;
    Ok(ActivationOutcome {
        generation_id: plan.generation_id.clone(),
        nodes: plan.nodes.len(),
        relations: plan.relations.len(),
        already_present: false,
    })
}

fn manifest_matches(left: &GenerationManifest, right: &GenerationManifest) -> bool {
    left.directed == right.directed
        && left.multigraph == right.multigraph
        && left.repository_id == right.repository_id
        && left.generation_id == right.generation_id
        && left.schema_version == right.schema_version
        && left.projection_fingerprint == right.projection_fingerprint
        && (right.graph_digest.is_none() || left.graph_digest == right.graph_digest)
        && left.source_tree_digest == right.source_tree_digest
        && left.schema_fingerprint == right.schema_fingerprint
        && left.node_count == right.node_count
        && left.relation_count == right.relation_count
        && left.file_count == right.file_count
        && left.metadata_json == right.metadata_json
        && left.cql_schema_fingerprint == right.cql_schema_fingerprint
        && left.projected_bytes == right.projected_bytes
}

async fn claim_generation(
    database: &BoundedDatabase,
    record: RecordId,
    manifest: &GenerationManifest,
) -> Result<(), ProjectionError> {
    let payload = database_record_value(serde_json::to_value(manifest)?, record, None)?;
    database
        .query(CLAIM_GENERATION)
        .bind(("payload", payload))
        .await
        .and_then(IndexedResults::check)
        .map(|_response| ())
        .map_err(|error| database_error("claim_generation", error))
}

fn interrupt_if_requested(
    interrupt: Option<InterruptAfter>,
    mutations: usize,
) -> Result<(), ProjectionError> {
    if interrupt.is_some_and(|interrupt| mutations >= interrupt.0) {
        return Err(ProjectionError::Interrupted {
            completed_mutations: mutations,
        });
    }
    Ok(())
}

fn write_batch_len(
    interrupt: Option<InterruptAfter>,
    completed_mutations: usize,
    remaining: usize,
) -> usize {
    let until_interrupt = interrupt
        .map(|value| value.0.saturating_sub(completed_mutations))
        .unwrap_or(PROJECTION_WRITE_BATCH);
    remaining.min(PROJECTION_WRITE_BATCH).min(until_interrupt)
}

async fn insert_node_batch(
    database: &BoundedDatabase,
    nodes: &[ProjectedNode],
) -> Result<(), ProjectionError> {
    let payloads = nodes
        .iter()
        .map(|node| {
            database_record_value(
                serde_json::to_value(node)?,
                RecordId::new("code_node", node.record_key.clone()),
                None,
            )
        })
        .collect::<Result<Vec<_>, ProjectionError>>()?;
    insert_batch(database, INSERT_NODE_BATCH, payloads, "write_node_batch").await
}

async fn insert_relation_batch(
    database: &BoundedDatabase,
    relations: &[ProjectedRelation],
) -> Result<(), ProjectionError> {
    let Some(first) = relations.first() else {
        return Ok(());
    };
    if relations
        .iter()
        .any(|relation| relation.family != first.family)
    {
        return Err(ProjectionError::InvalidPlan(
            "one projection write batch crossed relation families".to_owned(),
        ));
    }
    let statement = match first.family {
        RelationFamily::Structural => INSERT_STRUCTURAL_RELATION_BATCH,
        RelationFamily::Dependency => INSERT_DEPENDENCY_RELATION_BATCH,
        RelationFamily::Execution => INSERT_EXECUTION_RELATION_BATCH,
        RelationFamily::DataFlow => INSERT_DATA_FLOW_RELATION_BATCH,
        RelationFamily::Evidence => INSERT_EVIDENCE_RELATION_BATCH,
    };
    let payloads = relations
        .iter()
        .map(|relation| {
            database_record_value(
                serde_json::to_value(relation)?,
                RecordId::new(relation.family.as_str(), relation.record_key.clone()),
                Some((
                    RecordId::new("code_node", relation.source_record_key.clone()),
                    RecordId::new("code_node", relation.target_record_key.clone()),
                )),
            )
        })
        .collect::<Result<Vec<_>, ProjectionError>>()?;
    insert_batch(database, statement, payloads, "write_relation_batch").await
}

fn database_record_value(
    payload: Value,
    id: RecordId,
    endpoints: Option<(RecordId, RecordId)>,
) -> Result<DatabaseValue, ProjectionError> {
    let mut value = payload.into_value();
    let DatabaseValue::Object(object) = &mut value else {
        return Err(ProjectionError::InvalidPlan(
            "projected database payload is not an object".to_owned(),
        ));
    };
    object.insert("id", id);
    if let Some((source, target)) = endpoints {
        object.insert("in", source);
        object.insert("out", target);
    }
    Ok(value)
}

async fn insert_batch(
    database: &BoundedDatabase,
    statement: &'static str,
    payloads: Vec<DatabaseValue>,
    stage: &'static str,
) -> Result<(), ProjectionError> {
    database
        .query(statement)
        .bind(("payloads", payloads))
        .await
        .and_then(IndexedResults::check)
        .map(|_response| ())
        .map_err(|error| database_error(stage, error))
}

async fn upsert_pointer(
    database: &BoundedDatabase,
    plan: &ProjectionPlan,
) -> Result<(), ProjectionError> {
    let pointer = ActivePointer {
        repository_id: plan.repository_id.clone(),
        generation_id: plan.generation_id.clone(),
        schema_version: plan.schema_version.clone(),
        projection_fingerprint: plan.projection_fingerprint.clone(),
    };
    upsert_payload(
        database,
        RecordId::new(
            "repository_pointer",
            pointer_key(plan.repository_id.as_str()),
        ),
        serde_json::to_value(pointer)?,
        "activate_generation",
    )
    .await
}

async fn upsert_payload(
    database: &BoundedDatabase,
    record: RecordId,
    payload: Value,
    stage: &'static str,
) -> Result<(), ProjectionError> {
    database
        .query(UPSERT_RECORD)
        .bind(("record", record))
        .bind(("payload", payload))
        .await
        .and_then(IndexedResults::check)
        .map(|_response| ())
        .map_err(|error| database_error(stage, error))
}

async fn validate_candidate(
    database: &BoundedDatabase,
    plan: &ProjectionPlan,
) -> Result<(), ProjectionError> {
    let files = read_files(
        database,
        &plan.repository_id,
        &plan.generation_id,
        plan.files.len(),
    )
    .await?;
    if files != plan.files {
        return Err(ProjectionError::InvalidPlan(
            "staged files do not match the candidate".to_owned(),
        ));
    }
    let nodes = read_nodes(
        database,
        &plan.repository_id,
        &plan.generation_id,
        plan.nodes.len(),
    )
    .await?;
    if nodes != plan.nodes {
        return Err(ProjectionError::InvalidPlan(
            "staged node payloads do not match the candidate".to_owned(),
        ));
    }
    let mut relations = Vec::new();
    for family in RelationFamily::ALL {
        relations.extend(
            read_relations(
                database,
                &plan.repository_id,
                &plan.generation_id,
                family,
                plan.relations.len().saturating_sub(relations.len()),
            )
            .await?,
        );
    }
    relations.sort_by(|left, right| left.compass_edge_id.cmp(&right.compass_edge_id));
    if relations != plan.relations {
        return Err(ProjectionError::InvalidPlan(
            "staged relationship payloads do not match the candidate".to_owned(),
        ));
    }
    Ok(())
}

async fn select_record<T>(
    database: &BoundedDatabase,
    record: RecordId,
    stage: &'static str,
) -> Result<Option<T>, ProjectionError>
where
    T: for<'de> Deserialize<'de>,
{
    let value: Option<Value> = database
        .select(record)
        .await
        .map_err(|error| database_error(stage, error))?;
    value
        .map(serde_json::from_value)
        .transpose()
        .map_err(Into::into)
}

async fn read_nodes(
    database: &BoundedDatabase,
    repository_id: &str,
    generation_id: &str,
    limit: usize,
) -> Result<Vec<ProjectedNode>, ProjectionError> {
    query_projected(
        database,
        SELECT_NODES,
        repository_id,
        generation_id,
        None,
        limit,
        "read_nodes",
    )
    .await
}

async fn read_files(
    database: &BoundedDatabase,
    repository_id: &str,
    generation_id: &str,
    limit: usize,
) -> Result<Vec<ProjectedFile>, ProjectionError> {
    query_projected(
        database,
        SELECT_FILES,
        repository_id,
        generation_id,
        None,
        limit,
        "read_files",
    )
    .await
}

async fn read_relations(
    database: &BoundedDatabase,
    repository_id: &str,
    generation_id: &str,
    family: RelationFamily,
    limit: usize,
) -> Result<Vec<ProjectedRelation>, ProjectionError> {
    query_projected(
        database,
        SELECT_RELATIONS,
        repository_id,
        generation_id,
        Some(family),
        limit,
        "read_relations",
    )
    .await
}

async fn query_projected<T>(
    database: &BoundedDatabase,
    statement: &'static str,
    repository_id: &str,
    generation_id: &str,
    family: Option<RelationFamily>,
    limit: usize,
    stage: &'static str,
) -> Result<Vec<T>, ProjectionError>
where
    T: for<'de> Deserialize<'de>,
{
    let mut query = database
        .query(statement)
        .bind(("repository", repository_id))
        .bind(("generation", generation_id))
        .bind(("limit", plus_one(limit)?));
    if let Some(family) = family {
        query = query.bind(("table", family.as_str()));
    }
    let mut response = query.await.map_err(|error| database_error(stage, error))?;
    let values: Vec<Value> = response
        .take(0)
        .map_err(|error| database_error(stage, error))?;
    values
        .into_iter()
        .map(serde_json::from_value)
        .collect::<Result<_, _>>()
        .map_err(Into::into)
}

fn validate_manifest_limits(
    manifest: &GenerationManifest,
    limits: ProjectionLimits,
) -> Result<(), ProjectionError> {
    let metadata = crate::projection::decode_metadata(&manifest.metadata_json)?;
    if metadata.build.generation_id != manifest.generation_id
        || metadata.build.source_tree_digest != manifest.source_tree_digest
        || metadata.build.schema_fingerprint != manifest.schema_fingerprint
        || manifest.cql_schema_fingerprint.len() != 64
        || !manifest
            .cql_schema_fingerprint
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(ProjectionError::InvalidPlan(
            "generation metadata identity mismatch".to_owned(),
        ));
    }
    for (resource, actual, limit) in [
        ("nodes", manifest.node_count, limits.max_nodes()),
        ("files", manifest.file_count, limits.max_nodes()),
        ("relations", manifest.relation_count, limits.max_relations()),
    ] {
        if actual > limit {
            return Err(ProjectionError::LimitExceeded {
                resource,
                actual: u64::try_from(actual).unwrap_or(u64::MAX),
                limit: u64::try_from(limit).unwrap_or(u64::MAX),
            });
        }
    }
    if manifest.projected_bytes > limits.max_projected_bytes() {
        return Err(ProjectionError::LimitExceeded {
            resource: "projected bytes",
            actual: manifest.projected_bytes,
            limit: limits.max_projected_bytes(),
        });
    }
    Ok(())
}

fn plus_one(limit: usize) -> Result<usize, ProjectionError> {
    limit.checked_add(1).ok_or(ProjectionError::LimitExceeded {
        resource: "query rows",
        actual: u64::MAX,
        limit: u64::try_from(limit).unwrap_or(u64::MAX),
    })
}

fn pointer_key(repository_id: &str) -> String {
    record_key("repository", &[repository_id])
}

fn manifest_key(repository_id: &str, generation_id: &str) -> String {
    record_key("generation", &[repository_id, generation_id])
}

fn database_error(stage: &'static str, error: impl std::fmt::Display) -> ProjectionError {
    ProjectionError::Database {
        stage,
        message: error.to_string(),
    }
}
