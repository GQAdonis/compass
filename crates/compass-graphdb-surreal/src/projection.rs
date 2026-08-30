use std::collections::{BTreeMap, BTreeSet};

use compass_model::code_graph::{
    EdgeKind, EdgeRecord, FileRecord, GraphDocument, GraphMetadata, NodeRecord,
};
use compass_model::provenance::effective_confidence;
use compass_model::validate_code_graph;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::PROJECTION_SCHEMA_V2;

/// Finite work limits applied before activation and before materializing reads.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProjectionLimits {
    max_nodes: usize,
    max_relations: usize,
    max_projected_bytes: u64,
}

impl ProjectionLimits {
    pub fn new(
        max_nodes: usize,
        max_relations: usize,
        max_projected_bytes: u64,
    ) -> Result<Self, ProjectionError> {
        if max_nodes == 0 || max_relations == 0 || max_projected_bytes == 0 {
            return Err(ProjectionError::InvalidPlan(
                "projection limits must all be greater than zero".to_owned(),
            ));
        }
        Ok(Self {
            max_nodes,
            max_relations,
            max_projected_bytes,
        })
    }

    #[must_use]
    pub const fn max_nodes(self) -> usize {
        self.max_nodes
    }

    #[must_use]
    pub const fn max_relations(self) -> usize {
        self.max_relations
    }

    #[must_use]
    pub const fn max_projected_bytes(self) -> u64 {
        self.max_projected_bytes
    }

    #[cfg(any(feature = "mem", feature = "surrealkv", feature = "rocksdb"))]
    pub(crate) fn validate_plan(self, plan: &ProjectionPlan) -> Result<u64, ProjectionError> {
        enforce_limit("nodes", plan.nodes.len(), self.max_nodes)?;
        enforce_limit("files", plan.files.len(), self.max_nodes)?;
        enforce_limit("relations", plan.relations.len(), self.max_relations)?;
        let projected_bytes = plan.projected_bytes()?;
        if projected_bytes > self.max_projected_bytes {
            return Err(ProjectionError::LimitExceeded {
                resource: "projected bytes",
                actual: projected_bytes,
                limit: self.max_projected_bytes,
            });
        }
        Ok(projected_bytes)
    }
}

impl Default for ProjectionLimits {
    fn default() -> Self {
        Self {
            max_nodes: 1_000_000,
            max_relations: 2_500_000,
            max_projected_bytes: compass_model::DEFAULT_GRAPH_SIZE_CAP_BYTES,
        }
    }
}

/// A closed set of schemafull relation tables.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationFamily {
    Structural,
    Dependency,
    Execution,
    DataFlow,
    Evidence,
}

impl RelationFamily {
    pub const ALL: [Self; 5] = [
        Self::Structural,
        Self::Dependency,
        Self::Execution,
        Self::DataFlow,
        Self::Evidence,
    ];

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Structural => "structural_relation",
            Self::Dependency => "dependency_relation",
            Self::Execution => "execution_relation",
            Self::DataFlow => "data_flow_relation",
            Self::Evidence => "evidence_relation",
        }
    }
}

/// Map every closed Compass edge kind to exactly one relation family.
#[must_use]
pub const fn relation_family(kind: EdgeKind) -> RelationFamily {
    match kind {
        EdgeKind::Contains
        | EdgeKind::Embeds
        | EdgeKind::Extends
        | EdgeKind::Implements
        | EdgeKind::MixesIn
        | EdgeKind::TypeOf
        | EdgeKind::Overrides
        | EdgeKind::Decorates
        | EdgeKind::Aliases => RelationFamily::Structural,
        EdgeKind::Imports | EdgeKind::Exports | EdgeKind::References | EdgeKind::DependsOn => {
            RelationFamily::Dependency
        }
        EdgeKind::Calls
        | EdgeKind::Instantiates
        | EdgeKind::RoutesTo
        | EdgeKind::Registers
        | EdgeKind::Handles
        | EdgeKind::Schedules
        | EdgeKind::Triggers
        | EdgeKind::Tests
        | EdgeKind::Renders => RelationFamily::Execution,
        EdgeKind::Returns
        | EdgeKind::Reads
        | EdgeKind::Writes
        | EdgeKind::Publishes
        | EdgeKind::Subscribes
        | EdgeKind::Produces
        | EdgeKind::Consumes
        | EdgeKind::MapsTo => RelationFamily::DataFlow,
        EdgeKind::Documents => RelationFamily::Evidence,
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectedNode {
    pub record_key: String,
    pub repository_id: String,
    pub generation_id: String,
    pub schema_version: String,
    pub ordinal: u64,
    pub compass_node_id: String,
    pub kind: String,
    pub cql_label: String,
    pub name: String,
    pub qualified_name: String,
    pub normalized_names: Vec<String>,
    pub search_terms: Vec<String>,
    pub search_prefixes: Vec<String>,
    pub roles: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
    pub confidence: String,
    pub heuristic: bool,
    pub payload: serde_json::Value,
    pub payload_json: String,
}

impl ProjectedNode {
    pub fn decode(&self) -> Result<NodeRecord, ProjectionError> {
        Ok(serde_json::from_str(&self.payload_json)?)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectedRelation {
    pub record_key: String,
    pub repository_id: String,
    pub generation_id: String,
    pub schema_version: String,
    pub ordinal: u64,
    pub compass_edge_id: String,
    pub family: RelationFamily,
    pub kind: String,
    pub cql_type: String,
    pub source_node_id: String,
    pub target_node_id: String,
    pub source_record_key: String,
    pub target_record_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
    pub confidence: String,
    pub heuristic: bool,
    pub payload: serde_json::Value,
    pub payload_json: String,
}

impl ProjectedRelation {
    pub fn decode(&self) -> Result<EdgeRecord, ProjectionError> {
        Ok(serde_json::from_str(&self.payload_json)?)
    }
}

/// Deterministic database-independent plan for one immutable generation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectedFile {
    pub record_key: String,
    pub repository_id: String,
    pub generation_id: String,
    pub path: String,
    pub payload_json: String,
}

impl ProjectedFile {
    pub fn decode(&self) -> Result<FileRecord, ProjectionError> {
        Ok(serde_json::from_str(&self.payload_json)?)
    }
}

/// Deterministic database-independent plan for one immutable generation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectionPlan {
    pub directed: bool,
    pub multigraph: bool,
    pub repository_id: String,
    pub generation_id: String,
    pub schema_version: String,
    pub source_tree_digest: String,
    pub schema_fingerprint: String,
    pub projection_fingerprint: String,
    /// Bounded metadata header; file records are indexed separately.
    pub metadata_json: String,
    pub cql_schema_fingerprint: String,
    pub files: Vec<ProjectedFile>,
    pub nodes: Vec<ProjectedNode>,
    pub relations: Vec<ProjectedRelation>,
}

impl ProjectionPlan {
    pub fn from_graph(
        repository_id: impl Into<String>,
        graph: &GraphDocument,
    ) -> Result<Self, ProjectionError> {
        let repository_id = repository_id.into();
        if repository_id.trim().is_empty() {
            return Err(ProjectionError::EmptyRepositoryId);
        }
        let generation_id = graph.graph.build.generation_id.clone();
        if generation_id.trim().is_empty() {
            return Err(ProjectionError::EmptyGenerationId);
        }
        if graph.graph.build.source_tree_digest.trim().is_empty() {
            return Err(ProjectionError::InvalidPlan(
                "source tree digest must not be empty".to_owned(),
            ));
        }
        validate_code_graph(graph)?;
        let mut nodes = graph
            .nodes
            .iter()
            .enumerate()
            .map(|(ordinal, node)| {
                project_node(
                    &repository_id,
                    &generation_id,
                    u64::try_from(ordinal).map_err(|_| ProjectionError::LimitExceeded {
                        resource: "node ordinal",
                        actual: u64::MAX,
                        limit: u64::MAX - 1,
                    })?,
                    node,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        nodes.sort_by(|left, right| left.compass_node_id.cmp(&right.compass_node_id));
        let node_ids = nodes
            .iter()
            .map(|node| node.compass_node_id.as_str())
            .collect::<BTreeSet<_>>();
        let mut relations = graph
            .links
            .iter()
            .enumerate()
            .map(|(ordinal, edge)| {
                for endpoint in [&edge.source, &edge.target] {
                    if !node_ids.contains(endpoint.as_str()) {
                        return Err(ProjectionError::MissingEndpoint {
                            edge_id: edge.id.clone(),
                            endpoint: endpoint.clone(),
                        });
                    }
                }
                project_relation(
                    &repository_id,
                    &generation_id,
                    u64::try_from(ordinal).map_err(|_| ProjectionError::LimitExceeded {
                        resource: "relationship ordinal",
                        actual: u64::MAX,
                        limit: u64::MAX - 1,
                    })?,
                    edge,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        relations.sort_by(|left, right| left.compass_edge_id.cmp(&right.compass_edge_id));
        apply_alias_terms(&mut nodes, &relations);
        let mut metadata = graph.graph.clone();
        let mut files = metadata
            .files
            .iter()
            .map(|file| project_file(&repository_id, &generation_id, file))
            .collect::<Result<Vec<_>, _>>()?;
        files.sort_by(|left, right| left.path.cmp(&right.path));
        metadata.files.clear();
        let metadata_json = serde_json::to_string(&metadata)?;
        let cql_schema_fingerprint = query_schema_fingerprint(&nodes, &relations)?;
        let projection_fingerprint = fingerprint(
            &repository_id,
            &generation_id,
            (graph.directed, graph.multigraph),
            &metadata_json,
            &files,
            &nodes,
            &relations,
        )?;
        let plan = Self {
            directed: graph.directed,
            multigraph: graph.multigraph,
            repository_id,
            generation_id,
            schema_version: PROJECTION_SCHEMA_V2.to_owned(),
            source_tree_digest: graph.graph.build.source_tree_digest.clone(),
            schema_fingerprint: graph.graph.build.schema_fingerprint.clone(),
            projection_fingerprint,
            metadata_json,
            cql_schema_fingerprint,
            files,
            nodes,
            relations,
        };
        plan.validate()?;
        Ok(plan)
    }

    pub fn validate(&self) -> Result<(), ProjectionError> {
        if self.schema_version != PROJECTION_SCHEMA_V2 {
            return Err(ProjectionError::UnsupportedProjectionSchema(
                self.schema_version.clone(),
            ));
        }
        if self.repository_id.trim().is_empty() {
            return Err(ProjectionError::EmptyRepositoryId);
        }
        if self.generation_id.trim().is_empty() {
            return Err(ProjectionError::EmptyGenerationId);
        }
        if self.source_tree_digest.trim().is_empty() {
            return Err(ProjectionError::InvalidPlan(
                "source tree digest must not be empty".to_owned(),
            ));
        }
        let metadata = decode_metadata(&self.metadata_json)?;
        if self.cql_schema_fingerprint != query_schema_fingerprint(&self.nodes, &self.relations)? {
            return Err(ProjectionError::FingerprintMismatch);
        }
        if metadata.build.generation_id != self.generation_id
            || metadata.build.source_tree_digest != self.source_tree_digest
            || metadata.build.schema_fingerprint != self.schema_fingerprint
        {
            return Err(ProjectionError::InvalidPlan(
                "projection metadata identity mismatch".to_owned(),
            ));
        }
        ensure_strict_order(self.files.iter().map(|file| file.path.as_str()), "file")?;
        for file in &self.files {
            if project_file(&self.repository_id, &self.generation_id, &file.decode()?)? != *file {
                return Err(ProjectionError::InvalidPlan(
                    "projected file does not match its typed payload".to_owned(),
                ));
            }
        }
        ensure_strict_order(
            self.nodes.iter().map(|node| node.compass_node_id.as_str()),
            "node",
        )?;
        ensure_strict_order(
            self.relations
                .iter()
                .map(|relation| relation.compass_edge_id.as_str()),
            "relation",
        )?;
        let node_ids = self
            .nodes
            .iter()
            .map(|node| node.compass_node_id.as_str())
            .collect::<BTreeSet<_>>();
        ensure_complete_ordinals(
            self.nodes.iter().map(|node| node.ordinal),
            self.nodes.len(),
            "node",
        )?;
        ensure_complete_ordinals(
            self.relations.iter().map(|relation| relation.ordinal),
            self.relations.len(),
            "relationship",
        )?;
        let mut expected_nodes = Vec::with_capacity(self.nodes.len());
        for node in &self.nodes {
            let decoded = node.decode()?;
            expected_nodes.push(project_node(
                &self.repository_id,
                &self.generation_id,
                node.ordinal,
                &decoded,
            )?);
        }
        apply_alias_terms(&mut expected_nodes, &self.relations);
        if expected_nodes != self.nodes {
            return Err(ProjectionError::InvalidPlan(
                "projected nodes do not match their typed payloads and aliases".to_owned(),
            ));
        }
        for relation in &self.relations {
            for endpoint in [&relation.source_node_id, &relation.target_node_id] {
                if !node_ids.contains(endpoint.as_str()) {
                    return Err(ProjectionError::MissingEndpoint {
                        edge_id: relation.compass_edge_id.clone(),
                        endpoint: endpoint.clone(),
                    });
                }
            }
            let decoded = relation.decode()?;
            if project_relation(
                &self.repository_id,
                &self.generation_id,
                relation.ordinal,
                &decoded,
            )? != *relation
            {
                return Err(ProjectionError::InvalidPlan(format!(
                    "projected relation {} does not match its typed payload",
                    relation.compass_edge_id
                )));
            }
        }
        if fingerprint(
            &self.repository_id,
            &self.generation_id,
            (self.directed, self.multigraph),
            &self.metadata_json,
            &self.files,
            &self.nodes,
            &self.relations,
        )? != self.projection_fingerprint
        {
            return Err(ProjectionError::FingerprintMismatch);
        }
        Ok(())
    }

    #[cfg(any(feature = "mem", feature = "surrealkv", feature = "rocksdb"))]
    pub(crate) fn projected_bytes(&self) -> Result<u64, ProjectionError> {
        self.nodes
            .iter()
            .map(serialized_len)
            .chain(self.relations.iter().map(serialized_len))
            .chain(self.files.iter().map(serialized_len))
            .try_fold(
                serialized_len(&(
                    &self.metadata_json,
                    &self.cql_schema_fingerprint,
                    self.directed,
                    self.multigraph,
                ))?,
                |total, length| {
                    total
                        .checked_add(length?)
                        .ok_or(ProjectionError::LimitExceeded {
                            resource: "projected bytes",
                            actual: u64::MAX,
                            limit: u64::MAX - 1,
                        })
                },
            )
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ProjectionError {
    #[error("repository identity must not be empty")]
    EmptyRepositoryId,
    #[error("generation identity must not be empty")]
    EmptyGenerationId,
    #[error("unsupported projection schema {0}")]
    UnsupportedProjectionSchema(String),
    #[error("canonical graph validation failed: {0}")]
    GraphValidation(#[from] compass_model::CodeGraphValidationError),
    #[error("projection serialization failed: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("query property projection failed: {0}")]
    QueryProjection(#[from] compass_model::GraphError),
    #[error("edge {edge_id} references missing endpoint {endpoint}")]
    MissingEndpoint { edge_id: String, endpoint: String },
    #[error("projection {record_class} identities are not strictly ordered and unique")]
    NonDeterministicOrder { record_class: &'static str },
    #[error("projection fingerprint does not match its records")]
    FingerprintMismatch,
    #[error("projection {resource} limit exceeded: actual {actual}, limit {limit}")]
    LimitExceeded {
        resource: &'static str,
        actual: u64,
        limit: u64,
    },
    #[error("invalid projection plan: {0}")]
    InvalidPlan(String),
    #[error("invalid surreal.ref: {0}")]
    InvalidReference(String),
    #[error("invalid Surreal projection bundle: {0}")]
    InvalidBundle(String),
    #[cfg(any(feature = "mem", feature = "surrealkv", feature = "rocksdb"))]
    #[error("SurrealDB {stage} failed: {message}")]
    Database {
        stage: &'static str,
        message: String,
    },
    #[cfg(any(feature = "mem", feature = "surrealkv", feature = "rocksdb"))]
    #[error("projection staging failed ({cause}); transaction cancellation also failed: {message}")]
    Cancellation {
        cause: Box<ProjectionError>,
        message: String,
    },
    #[cfg(any(feature = "mem", feature = "surrealkv", feature = "rocksdb"))]
    #[error("projection was interrupted after {completed_mutations} mutations")]
    Interrupted { completed_mutations: usize },
    #[cfg(any(feature = "mem", feature = "surrealkv", feature = "rocksdb"))]
    #[error("repository {repository_id:?} has no complete active Surreal generation")]
    ActiveGenerationUnavailable { repository_id: String },
    #[cfg(any(feature = "mem", feature = "surrealkv", feature = "rocksdb"))]
    #[error("repository {repository_id:?} changed active generation during the query")]
    ActiveGenerationChanged { repository_id: String },
    #[cfg(any(feature = "mem", feature = "surrealkv", feature = "rocksdb"))]
    #[error("invalid native-query cursor: {0}")]
    InvalidCursor(String),
    #[cfg(any(feature = "mem", feature = "surrealkv", feature = "rocksdb"))]
    #[error("invalid native query: {0}")]
    InvalidQuery(String),
}

#[cfg(any(feature = "mem", feature = "surrealkv", feature = "rocksdb"))]
fn serialized_len<T: Serialize>(value: &T) -> Result<u64, ProjectionError> {
    u64::try_from(serde_json::to_vec(value)?.len()).map_err(|_| ProjectionError::LimitExceeded {
        resource: "projected bytes",
        actual: u64::MAX,
        limit: u64::MAX - 1,
    })
}

#[cfg(any(feature = "mem", feature = "surrealkv", feature = "rocksdb"))]
fn enforce_limit(
    resource: &'static str,
    actual: usize,
    limit: usize,
) -> Result<(), ProjectionError> {
    if actual > limit {
        return Err(ProjectionError::LimitExceeded {
            resource,
            actual: u64::try_from(actual).unwrap_or(u64::MAX),
            limit: u64::try_from(limit).unwrap_or(u64::MAX),
        });
    }
    Ok(())
}

fn project_node(
    repository_id: &str,
    generation_id: &str,
    ordinal: u64,
    node: &NodeRecord,
) -> Result<ProjectedNode, ProjectionError> {
    let mut normalized_names = [&node.name, &node.qualified_name]
        .into_iter()
        .map(|name| compass_model::query_contract::normalize_query_symbol(name))
        .collect::<Vec<_>>();
    normalized_names.sort();
    normalized_names.dedup();
    let search_terms: Vec<_> = compass_model::search::searchable_node_terms(node)
        .into_iter()
        .collect();
    let mut roles = node
        .roles
        .iter()
        .map(|role| role.as_str().to_owned())
        .collect::<Vec<_>>();
    roles.sort();
    roles.dedup();
    let scope = node
        .qualified_name
        .rsplit_once("::")
        .or_else(|| node.qualified_name.rsplit_once('.'))
        .or_else(|| node.qualified_name.rsplit_once('#'))
        .map(|(scope, _name)| scope.to_owned())
        .filter(|scope| !scope.is_empty());
    let payload = serde_json::to_value(node)?;
    Ok(ProjectedNode {
        record_key: record_key("node", &[repository_id, generation_id, &node.id]),
        repository_id: repository_id.to_owned(),
        generation_id: generation_id.to_owned(),
        schema_version: PROJECTION_SCHEMA_V2.to_owned(),
        ordinal,
        compass_node_id: node.id.clone(),
        kind: node.kind.as_str().to_owned(),
        cql_label: compass_model::cypher_node_label_from_kind(node.kind.as_str()),
        name: node.name.clone(),
        qualified_name: node.qualified_name.clone(),
        normalized_names,
        search_prefixes: search_prefixes(&search_terms),
        search_terms,
        roles,
        scope,
        language: node.language.clone(),
        source_path: node.source.as_ref().map(|source| source.file.clone()),
        confidence: confidence(&node.evidence),
        heuristic: node.evidence.iter().any(|evidence| {
            evidence.origin == compass_model::provenance::EvidenceOrigin::Heuristic
        }),
        payload_json: serde_json::to_string(node)?,
        payload,
    })
}

pub(crate) const MAX_METADATA_BYTES: usize = 4 * 1024 * 1024;

pub(crate) fn decode_metadata(value: &str) -> Result<GraphMetadata, ProjectionError> {
    if value.len() > MAX_METADATA_BYTES {
        return Err(ProjectionError::LimitExceeded {
            resource: "metadata bytes",
            actual: u64::try_from(value.len()).unwrap_or(u64::MAX),
            limit: MAX_METADATA_BYTES as u64,
        });
    }
    let metadata: GraphMetadata = serde_json::from_str(value)?;
    if !metadata.files.is_empty() {
        return Err(ProjectionError::InvalidPlan(
            "metadata header must not embed file records".to_owned(),
        ));
    }
    Ok(metadata)
}

fn project_file(
    repository_id: &str,
    generation_id: &str,
    file: &FileRecord,
) -> Result<ProjectedFile, ProjectionError> {
    Ok(ProjectedFile {
        record_key: record_key("file", &[repository_id, generation_id, &file.path]),
        repository_id: repository_id.to_owned(),
        generation_id: generation_id.to_owned(),
        path: file.path.clone(),
        payload_json: serde_json::to_string(file)?,
    })
}

fn apply_alias_terms(nodes: &mut [ProjectedNode], relations: &[ProjectedRelation]) {
    let names = nodes
        .iter()
        .map(|node| (node.compass_node_id.clone(), node.name.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut aliases = BTreeMap::<&str, BTreeSet<String>>::new();
    for relation in relations {
        if relation.kind == EdgeKind::Aliases.as_str()
            && let Some(name) = names.get(&relation.source_node_id)
        {
            let terms = aliases.entry(&relation.target_node_id).or_default();
            terms.extend(compass_model::search::token_search_terms(name));
            terms.extend(compass_model::search::identifier_search_terms(name));
        }
    }
    for node in nodes {
        if let Some(terms) = aliases.get(node.compass_node_id.as_str()) {
            node.search_terms.extend(terms.iter().cloned());
            node.search_terms.sort();
            node.search_terms.dedup();
            node.search_prefixes = search_prefixes(&node.search_terms);
        }
    }
}

/// Index the first one, two, and three Unicode scalars of each canonical term.
/// This is linear in the term count, unlike storing every possible prefix.
fn search_prefixes(terms: &[String]) -> Vec<String> {
    let mut prefixes = BTreeSet::new();
    for term in terms {
        let mut prefix = String::new();
        for character in term.chars().take(3) {
            prefix.push(character);
            prefixes.insert(prefix.clone());
        }
    }
    prefixes.into_iter().collect()
}

fn project_relation(
    repository_id: &str,
    generation_id: &str,
    ordinal: u64,
    edge: &EdgeRecord,
) -> Result<ProjectedRelation, ProjectionError> {
    let payload = serde_json::to_value(edge)?;
    Ok(ProjectedRelation {
        record_key: record_key("edge", &[repository_id, generation_id, &edge.id]),
        repository_id: repository_id.to_owned(),
        generation_id: generation_id.to_owned(),
        schema_version: PROJECTION_SCHEMA_V2.to_owned(),
        ordinal,
        compass_edge_id: edge.id.clone(),
        family: relation_family(edge.kind),
        kind: edge.kind.as_str().to_owned(),
        cql_type: compass_model::cypher_relationship_type_from_relation(edge.kind.as_str()),
        source_node_id: edge.source.clone(),
        target_node_id: edge.target.clone(),
        source_record_key: record_key("node", &[repository_id, generation_id, &edge.source]),
        target_record_key: record_key("node", &[repository_id, generation_id, &edge.target]),
        source_path: edge
            .relationship_site
            .as_ref()
            .map(|source| source.file.clone()),
        confidence: confidence(&edge.evidence),
        heuristic: edge.evidence.iter().any(|evidence| {
            evidence.origin == compass_model::provenance::EvidenceOrigin::Heuristic
        }),
        payload_json: serde_json::to_string(edge)?,
        payload,
    })
}

fn confidence(evidence: &[compass_model::provenance::Provenance]) -> String {
    effective_confidence(evidence)
        .map_or("unknown", |value| value.as_str())
        .to_owned()
}

pub(crate) fn record_key(class: &str, parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    write_part(&mut hasher, PROJECTION_SCHEMA_V2.as_bytes());
    write_part(&mut hasher, class.as_bytes());
    for part in parts {
        write_part(&mut hasher, part.as_bytes());
    }
    hex_digest(hasher.finalize())
}

fn fingerprint(
    repository_id: &str,
    generation_id: &str,
    graph_flags: (bool, bool),
    metadata_json: &str,
    files: &[ProjectedFile],
    nodes: &[ProjectedNode],
    relations: &[ProjectedRelation],
) -> Result<String, ProjectionError> {
    let mut hasher = Sha256::new();
    for value in [PROJECTION_SCHEMA_V2, repository_id, generation_id] {
        write_part(&mut hasher, value.as_bytes());
    }
    write_part(
        &mut hasher,
        &[u8::from(graph_flags.0), u8::from(graph_flags.1)],
    );
    write_part(&mut hasher, metadata_json.as_bytes());
    for file in files {
        write_part(&mut hasher, &serde_json::to_vec(file)?);
    }
    for node in nodes {
        let mut stable = node.clone();
        stable.payload = serde_json::Value::Null;
        write_part(&mut hasher, &serde_json::to_vec(&stable)?);
    }
    for relation in relations {
        let mut stable = relation.clone();
        stable.payload = serde_json::Value::Null;
        write_part(&mut hasher, &serde_json::to_vec(&stable)?);
    }
    Ok(format!("sha256:{}", hex_digest(hasher.finalize())))
}

fn query_schema_fingerprint(
    nodes: &[ProjectedNode],
    relations: &[ProjectedRelation],
) -> Result<String, ProjectionError> {
    let mut builder = compass_model::SchemaFingerprintBuilder::default();
    for node in nodes {
        builder.add_node(&node.decode()?.to_query_record()?);
    }
    for relation in relations {
        builder.add_edge(&relation.decode()?.to_query_record()?);
    }
    Ok(builder.finish().to_hex())
}

fn write_part(hasher: &mut Sha256, value: &[u8]) {
    hasher.update(u64::try_from(value.len()).unwrap_or(u64::MAX).to_be_bytes());
    hasher.update(value);
}

fn hex_digest(digest: impl AsRef<[u8]>) -> String {
    let mut encoded = String::with_capacity(digest.as_ref().len() * 2);
    for byte in digest.as_ref() {
        use std::fmt::Write as _;
        let _ = write!(encoded, "{byte:02x}");
    }
    encoded
}

fn ensure_strict_order<'a>(
    values: impl Iterator<Item = &'a str>,
    record_class: &'static str,
) -> Result<(), ProjectionError> {
    let mut previous = None;
    for value in values {
        if previous.is_some_and(|previous| previous >= value) {
            return Err(ProjectionError::NonDeterministicOrder { record_class });
        }
        previous = Some(value);
    }
    Ok(())
}

fn ensure_complete_ordinals(
    values: impl Iterator<Item = u64>,
    expected_len: usize,
    record_class: &'static str,
) -> Result<(), ProjectionError> {
    let actual = values.collect::<BTreeSet<_>>();
    let expected = (0..expected_len)
        .map(|ordinal| u64::try_from(ordinal).unwrap_or(u64::MAX))
        .collect::<BTreeSet<_>>();
    if actual != expected {
        return Err(ProjectionError::InvalidPlan(format!(
            "{record_class} ordinals must contain every value from zero through the record count exactly once"
        )));
    }
    Ok(())
}
