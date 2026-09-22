//! Closed generation-pinned native graph reads.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap, VecDeque};

use compass_model::code_graph::{EdgeKind, EdgeRecord, NodeRecord};
use compass_model::provenance::EvidenceConfidence;
use compass_model::query_contract::{
    CallRequest, CodeQueryLimits, CodeQueryOperation, CodeQueryResponse, ExploreRequest,
    ImpactRequest, NodeTrailRequest, QueryDiagnostic, QueryDiagnosticCode, QueryEdge, SearchHit,
    SearchRequest, normalize_query_symbol, query_edge_from_record, query_node_from_record,
    query_path_from_records,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::{
    ActivePointer, GenerationManifest, SurrealProjection, database_error, manifest_key, plus_one,
    pointer_key, select_record, validate_manifest_limits,
};
use crate::{
    PROJECTION_SCHEMA_V2, ProjectedNode, ProjectedRelation, ProjectionError, RelationFamily,
    SurrealRef, relation_family,
};

pub const NATIVE_RELATION_PAGE_SCHEMA_V1: &str = "compass.surreal-relation-page/1";
const CURSOR_SCHEMA_V1: &str = "compass.surreal-query-cursor/1";
const CURSOR_OPERATION_RELATIONS: &str = "relations";
const MAX_CODE_QUERY_CANDIDATES: u32 = 256;
const MAX_CURSOR_BYTES: usize = 8 * 1024;

const SELECT_NODE_BY_ID: &str = "SELECT * OMIT id FROM code_node WHERE repositoryId = $repository AND generationId = $generation AND compassNodeId = $symbol ORDER BY compassNodeId LIMIT 2";
const SELECT_NODES_BY_EXACT_NAME: &str = "SELECT * OMIT id FROM code_node WHERE repositoryId = $repository AND generationId = $generation AND normalizedNames CONTAINS $symbol ORDER BY compassNodeId LIMIT $limit";
const SELECT_NODES_BY_SEARCH_TERM: &str = "SELECT * OMIT id FROM code_node WHERE repositoryId = $repository AND generationId = $generation AND (searchTerms CONTAINS $symbol OR normalizedNames CONTAINS $symbol OR string::lowercase(name) CONTAINS $symbol OR string::lowercase(qualifiedName) CONTAINS $symbol) ORDER BY compassNodeId LIMIT $limit";
const SELECT_CQL_NODES: &str = "SELECT * OMIT id FROM code_node WHERE repositoryId = $repository AND generationId = $generation AND ($allLabels OR cqlLabel IN $labels) ORDER BY ordinal LIMIT $limit";
const SELECT_CQL_ENDPOINTS: &str = "SELECT * OMIT id FROM code_node WHERE repositoryId = $repository AND generationId = $generation AND compassNodeId IN $nodeIds ORDER BY ordinal LIMIT $limit";
const SELECT_CQL_RELATIONS: &str = "SELECT * OMIT id, in, out FROM type::table($table) WHERE repositoryId = $repository AND generationId = $generation AND ($allTypes OR cqlType IN $types) ORDER BY ordinal LIMIT $limit";

#[derive(Clone, Copy)]
struct FamilyStatements {
    family: RelationFamily,
    incoming: &'static str,
    outgoing: &'static str,
    page: &'static str,
}

const FAMILY_STATEMENTS: [FamilyStatements; 5] = [
    FamilyStatements {
        family: RelationFamily::Structural,
        incoming: "SELECT * OMIT id, in, out FROM structural_relation WHERE repositoryId = $repository AND generationId = $generation AND targetNodeId = $node AND kind IN $kinds AND ($includeHeuristic OR heuristic = false) ORDER BY compassEdgeId LIMIT $limit",
        outgoing: "SELECT * OMIT id, in, out FROM structural_relation WHERE repositoryId = $repository AND generationId = $generation AND sourceNodeId = $node AND kind IN $kinds AND ($includeHeuristic OR heuristic = false) ORDER BY compassEdgeId LIMIT $limit",
        page: "SELECT * OMIT id, in, out FROM structural_relation WHERE repositoryId = $repository AND generationId = $generation AND compassEdgeId > $after AND ($includeHeuristic OR heuristic = false) ORDER BY compassEdgeId LIMIT $limit",
    },
    FamilyStatements {
        family: RelationFamily::Dependency,
        incoming: "SELECT * OMIT id, in, out FROM dependency_relation WHERE repositoryId = $repository AND generationId = $generation AND targetNodeId = $node AND kind IN $kinds AND ($includeHeuristic OR heuristic = false) ORDER BY compassEdgeId LIMIT $limit",
        outgoing: "SELECT * OMIT id, in, out FROM dependency_relation WHERE repositoryId = $repository AND generationId = $generation AND sourceNodeId = $node AND kind IN $kinds AND ($includeHeuristic OR heuristic = false) ORDER BY compassEdgeId LIMIT $limit",
        page: "SELECT * OMIT id, in, out FROM dependency_relation WHERE repositoryId = $repository AND generationId = $generation AND compassEdgeId > $after AND ($includeHeuristic OR heuristic = false) ORDER BY compassEdgeId LIMIT $limit",
    },
    FamilyStatements {
        family: RelationFamily::Execution,
        incoming: "SELECT * OMIT id, in, out FROM execution_relation WHERE repositoryId = $repository AND generationId = $generation AND targetNodeId = $node AND kind IN $kinds AND ($includeHeuristic OR heuristic = false) ORDER BY compassEdgeId LIMIT $limit",
        outgoing: "SELECT * OMIT id, in, out FROM execution_relation WHERE repositoryId = $repository AND generationId = $generation AND sourceNodeId = $node AND kind IN $kinds AND ($includeHeuristic OR heuristic = false) ORDER BY compassEdgeId LIMIT $limit",
        page: "SELECT * OMIT id, in, out FROM execution_relation WHERE repositoryId = $repository AND generationId = $generation AND compassEdgeId > $after AND ($includeHeuristic OR heuristic = false) ORDER BY compassEdgeId LIMIT $limit",
    },
    FamilyStatements {
        family: RelationFamily::DataFlow,
        incoming: "SELECT * OMIT id, in, out FROM data_flow_relation WHERE repositoryId = $repository AND generationId = $generation AND targetNodeId = $node AND kind IN $kinds AND ($includeHeuristic OR heuristic = false) ORDER BY compassEdgeId LIMIT $limit",
        outgoing: "SELECT * OMIT id, in, out FROM data_flow_relation WHERE repositoryId = $repository AND generationId = $generation AND sourceNodeId = $node AND kind IN $kinds AND ($includeHeuristic OR heuristic = false) ORDER BY compassEdgeId LIMIT $limit",
        page: "SELECT * OMIT id, in, out FROM data_flow_relation WHERE repositoryId = $repository AND generationId = $generation AND compassEdgeId > $after AND ($includeHeuristic OR heuristic = false) ORDER BY compassEdgeId LIMIT $limit",
    },
    FamilyStatements {
        family: RelationFamily::Evidence,
        incoming: "SELECT * OMIT id, in, out FROM evidence_relation WHERE repositoryId = $repository AND generationId = $generation AND targetNodeId = $node AND kind IN $kinds AND ($includeHeuristic OR heuristic = false) ORDER BY compassEdgeId LIMIT $limit",
        outgoing: "SELECT * OMIT id, in, out FROM evidence_relation WHERE repositoryId = $repository AND generationId = $generation AND sourceNodeId = $node AND kind IN $kinds AND ($includeHeuristic OR heuristic = false) ORDER BY compassEdgeId LIMIT $limit",
        page: "SELECT * OMIT id, in, out FROM evidence_relation WHERE repositoryId = $repository AND generationId = $generation AND compassEdgeId > $after AND ($includeHeuristic OR heuristic = false) ORDER BY compassEdgeId LIMIT $limit",
    },
];

const ALL_EDGE_KINDS: &[EdgeKind] = &[
    EdgeKind::Contains,
    EdgeKind::Embeds,
    EdgeKind::Calls,
    EdgeKind::Imports,
    EdgeKind::Exports,
    EdgeKind::Extends,
    EdgeKind::Implements,
    EdgeKind::MixesIn,
    EdgeKind::References,
    EdgeKind::TypeOf,
    EdgeKind::Returns,
    EdgeKind::Instantiates,
    EdgeKind::Overrides,
    EdgeKind::Decorates,
    EdgeKind::RoutesTo,
    EdgeKind::Reads,
    EdgeKind::Writes,
    EdgeKind::Aliases,
    EdgeKind::Registers,
    EdgeKind::Handles,
    EdgeKind::Publishes,
    EdgeKind::Subscribes,
    EdgeKind::Produces,
    EdgeKind::Consumes,
    EdgeKind::Schedules,
    EdgeKind::Triggers,
    EdgeKind::Tests,
    EdgeKind::Renders,
    EdgeKind::DependsOn,
    EdgeKind::Documents,
    EdgeKind::MapsTo,
];

const IMPACT_KINDS: &[EdgeKind] = &[
    EdgeKind::Calls,
    EdgeKind::RoutesTo,
    EdgeKind::Imports,
    EdgeKind::Exports,
    EdgeKind::References,
    EdgeKind::DependsOn,
    EdgeKind::Reads,
    EdgeKind::Writes,
    EdgeKind::Publishes,
    EdgeKind::Subscribes,
    EdgeKind::Produces,
    EdgeKind::Consumes,
    EdgeKind::Schedules,
    EdgeKind::Triggers,
    EdgeKind::MapsTo,
];

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RelationPageRequest {
    pub max_items: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    #[serde(default)]
    pub include_heuristic: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RelationPage {
    pub schema: String,
    pub repository_id: String,
    pub generation_id: String,
    pub relations: Vec<QueryEdge>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// A bounded set of immutable Surreal rows used by the CompassQL semantic
/// evaluator. This is deliberately not a database snapshot: every row is
/// selected by a generation-pinned, parameterized SurrealQL statement and an
/// overflow is an error rather than an incomplete result.
#[derive(Clone, Debug, PartialEq)]
pub struct CqlProjectionSlice {
    pub schema_fingerprint: String,
    pub source_tree_digest: String,
    pub node_ordinals: Vec<u64>,
    pub relation_ordinals: Vec<u64>,
    pub nodes: Vec<NodeRecord>,
    pub relations: Vec<EdgeRecord>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CqlProjectionRequest {
    pub node_labels: BTreeSet<String>,
    pub relation_types: BTreeSet<String>,
    pub include_nodes: bool,
    pub include_relations: bool,
    pub max_nodes: usize,
    pub max_relations: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct QuerySelector {
    repository_id: String,
    pointer: ActivePointer,
    require_current_pointer: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CursorPayload {
    schema: String,
    repository_digest: String,
    generation_id: String,
    operation: String,
    last_identity: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CursorEnvelope {
    payload: CursorPayload,
    checksum: String,
}

#[derive(Clone, Copy)]
struct TraversalBudget {
    remaining_nodes: usize,
    remaining_edges: usize,
}

impl TraversalBudget {
    fn new(limits: &CodeQueryLimits) -> Self {
        Self {
            remaining_nodes: usize::try_from(limits.max_nodes).unwrap_or(usize::MAX),
            remaining_edges: usize::try_from(limits.max_edges).unwrap_or(usize::MAX),
        }
    }

    fn consume_node(&mut self) -> bool {
        if self.remaining_nodes == 0 {
            false
        } else {
            self.remaining_nodes -= 1;
            true
        }
    }

    fn consume_edge(&mut self) -> bool {
        if self.remaining_edges == 0 {
            false
        } else {
            self.remaining_edges -= 1;
            true
        }
    }
}

impl SurrealProjection {
    /// Select the bounded row set needed by a compiled CompassQL plan.
    ///
    /// The mutable active pointer is never consulted. Relationship endpoints
    /// are loaded explicitly so the caller can evaluate direction, repeated
    /// variables, optional patterns, correlated EXISTS, and paths without a
    /// second storage backend.
    pub async fn cql_projection_at(
        &self,
        reference: &SurrealRef,
        request: &CqlProjectionRequest,
    ) -> Result<CqlProjectionSlice, ProjectionError> {
        if (request.include_nodes && request.max_nodes == 0)
            || (request.include_relations && request.max_relations == 0)
            || (request.include_relations && !request.include_nodes)
        {
            return Err(ProjectionError::InvalidQuery(
                "CompassQL Surreal row selection and bounds are inconsistent".to_owned(),
            ));
        }
        let selector = self.pin_reference_selector(reference).await?;
        let manifest = self.manifest_for_reference(reference).await?;
        // CompassQL labels and relationship types are case-sensitive.
        let labels = request.node_labels.iter().cloned().collect::<Vec<_>>();
        let mut nodes = if request.include_nodes {
            let mut response = self
                .database
                .query(SELECT_CQL_NODES)
                .bind(("repository", selector.repository_id.as_str()))
                .bind(("generation", selector.pointer.generation_id.as_str()))
                .bind(("allLabels", labels.is_empty()))
                .bind(("labels", labels))
                .bind(("limit", plus_one(request.max_nodes)?))
                .await
                .map_err(|error| database_error("cql_select_nodes", error))?;
            let nodes = decode_values::<ProjectedNode>(&mut response, "cql_select_nodes")?;
            if nodes.len() > request.max_nodes {
                return Err(ProjectionError::LimitExceeded {
                    resource: "CompassQL Surreal candidate nodes",
                    actual: u64::try_from(nodes.len()).unwrap_or(u64::MAX),
                    limit: u64::try_from(request.max_nodes).unwrap_or(u64::MAX),
                });
            }
            nodes
        } else {
            Vec::new()
        };

        let mut relations = Vec::<ProjectedRelation>::new();
        if request.include_relations {
            let types = request.relation_types.iter().cloned().collect::<Vec<_>>();
            for family in RelationFamily::ALL {
                let remaining = request.max_relations.saturating_sub(relations.len());
                let mut response = self
                    .database
                    .query(SELECT_CQL_RELATIONS)
                    .bind(("table", family.as_str()))
                    .bind(("repository", selector.repository_id.as_str()))
                    .bind(("generation", selector.pointer.generation_id.as_str()))
                    .bind(("allTypes", types.is_empty()))
                    .bind(("types", types.clone()))
                    .bind(("limit", plus_one(remaining)?))
                    .await
                    .map_err(|error| database_error("cql_select_relations", error))?;
                let mut family_rows =
                    decode_values::<ProjectedRelation>(&mut response, "cql_select_relations")?;
                if family_rows.len() > remaining {
                    return Err(ProjectionError::LimitExceeded {
                        resource: "CompassQL Surreal candidate relationships",
                        actual: u64::try_from(relations.len() + family_rows.len())
                            .unwrap_or(u64::MAX),
                        limit: u64::try_from(request.max_relations).unwrap_or(u64::MAX),
                    });
                }
                relations.append(&mut family_rows);
            }
        }
        relations.sort_by_key(|relation| relation.ordinal);

        let present = nodes
            .iter()
            .map(|node| node.compass_node_id.clone())
            .collect::<BTreeSet<_>>();
        let missing = relations
            .iter()
            .flat_map(|relation| {
                [
                    relation.source_node_id.clone(),
                    relation.target_node_id.clone(),
                ]
            })
            .filter(|identity| !present.contains(identity))
            .collect::<BTreeSet<_>>();
        if !missing.is_empty() {
            let remaining = request.max_nodes.saturating_sub(nodes.len());
            if missing.len() > remaining {
                return Err(ProjectionError::LimitExceeded {
                    resource: "CompassQL Surreal candidate nodes",
                    actual: u64::try_from(nodes.len() + missing.len()).unwrap_or(u64::MAX),
                    limit: u64::try_from(request.max_nodes).unwrap_or(u64::MAX),
                });
            }
            let mut response = self
                .database
                .query(SELECT_CQL_ENDPOINTS)
                .bind(("repository", selector.repository_id.as_str()))
                .bind(("generation", selector.pointer.generation_id.as_str()))
                .bind(("nodeIds", missing.into_iter().collect::<Vec<_>>()))
                .bind(("limit", plus_one(remaining)?))
                .await
                .map_err(|error| database_error("cql_select_endpoints", error))?;
            let mut endpoints =
                decode_values::<ProjectedNode>(&mut response, "cql_select_endpoints")?;
            if endpoints.len() > remaining {
                return Err(ProjectionError::LimitExceeded {
                    resource: "CompassQL Surreal candidate nodes",
                    actual: u64::try_from(nodes.len() + endpoints.len()).unwrap_or(u64::MAX),
                    limit: u64::try_from(request.max_nodes).unwrap_or(u64::MAX),
                });
            }
            nodes.append(&mut endpoints);
            nodes.sort_by_key(|node| node.ordinal);
            nodes.dedup_by(|left, right| left.compass_node_id == right.compass_node_id);
        }

        Ok(CqlProjectionSlice {
            schema_fingerprint: manifest.schema_fingerprint,
            source_tree_digest: manifest.source_tree_digest,
            node_ordinals: nodes.iter().map(|node| node.ordinal).collect(),
            relation_ordinals: relations.iter().map(|relation| relation.ordinal).collect(),
            nodes: nodes
                .into_iter()
                .map(|node| node.decode())
                .collect::<Result<Vec<_>, _>>()?,
            relations: relations
                .into_iter()
                .map(|relation| relation.decode())
                .collect::<Result<Vec<_>, _>>()?,
        })
    }

    pub async fn search_at(
        &self,
        reference: &SurrealRef,
        request: SearchRequest,
    ) -> Result<CodeQueryResponse, ProjectionError> {
        let selector = self.pin_reference_selector(reference).await?;
        self.search_with_selector(selector, request).await
    }

    pub async fn search(
        &self,
        repository_id: &str,
        request: SearchRequest,
    ) -> Result<CodeQueryResponse, ProjectionError> {
        let selector = self.pin_query_selector(repository_id).await?;
        self.search_with_selector(selector, request).await
    }

    async fn search_with_selector(
        &self,
        selector: QuerySelector,
        request: SearchRequest,
    ) -> Result<CodeQueryResponse, ProjectionError> {
        validate_query_limits(&request.limits)?;
        let term = normalize_query_symbol(&request.query);
        if term.is_empty() {
            return Err(ProjectionError::InvalidQuery(
                "search query must not be empty".to_owned(),
            ));
        }
        let limit = usize::try_from(request.limits.max_candidates).unwrap_or(usize::MAX);
        let mut projected = self
            .query_nodes(
                &selector,
                SELECT_NODES_BY_SEARCH_TERM,
                &term,
                plus_one(limit)?,
                "search_nodes",
            )
            .await?;
        let truncated = projected.len() > limit;
        if truncated {
            projected.truncate(limit);
        }
        let mut response = CodeQueryResponse::empty(CodeQueryOperation::Search, request.limits);
        response.truncated = truncated;
        for node in projected {
            let exact_name = node.normalized_names.iter().any(|value| value == &term);
            let matched_fields = if exact_name {
                vec!["name".to_owned()]
            } else {
                vec!["searchTerms".to_owned()]
            };
            response.results.push(SearchHit {
                node_id: node.compass_node_id.clone(),
                score: if exact_name { 1.0 } else { 0.5 },
                matched_fields,
            });
            response.nodes.push(query_node_from_record(&node.decode()?));
        }
        if response.nodes.is_empty() {
            response.diagnostics.push(QueryDiagnostic {
                code: QueryDiagnosticCode::NoMatch,
                message: format!("No symbol matched {:?}", request.query),
                node_id: None,
                path: None,
            });
        }
        self.publish_response(&selector, response).await
    }

    pub async fn callers(
        &self,
        repository_id: &str,
        request: CallRequest,
    ) -> Result<CodeQueryResponse, ProjectionError> {
        let selector = self.pin_query_selector(repository_id).await?;
        self.call_neighbors(selector, request, true).await
    }

    pub async fn callers_at(
        &self,
        reference: &SurrealRef,
        request: CallRequest,
    ) -> Result<CodeQueryResponse, ProjectionError> {
        let selector = self.pin_reference_selector(reference).await?;
        self.call_neighbors(selector, request, true).await
    }

    pub async fn callees(
        &self,
        repository_id: &str,
        request: CallRequest,
    ) -> Result<CodeQueryResponse, ProjectionError> {
        let selector = self.pin_query_selector(repository_id).await?;
        self.call_neighbors(selector, request, false).await
    }

    pub async fn callees_at(
        &self,
        reference: &SurrealRef,
        request: CallRequest,
    ) -> Result<CodeQueryResponse, ProjectionError> {
        let selector = self.pin_reference_selector(reference).await?;
        self.call_neighbors(selector, request, false).await
    }

    async fn call_neighbors(
        &self,
        selector: QuerySelector,
        request: CallRequest,
        inbound: bool,
    ) -> Result<CodeQueryResponse, ProjectionError> {
        validate_query_limits(&request.limits)?;
        let operation = if inbound {
            CodeQueryOperation::Callers
        } else {
            CodeQueryOperation::Callees
        };
        let mut response = CodeQueryResponse::empty(operation, request.limits.clone());
        let Some(seed) = self
            .resolve_symbol(&selector, &request.symbol, &mut response)
            .await?
        else {
            return self.publish_response(&selector, response).await;
        };
        let kinds: &[EdgeKind] = if inbound {
            &[EdgeKind::Calls, EdgeKind::RoutesTo]
        } else {
            &[EdgeKind::Calls]
        };
        let max_edges = usize::try_from(request.limits.max_edges).unwrap_or(usize::MAX);
        let (edges, truncated) = self
            .matching_relations(
                &selector,
                &seed,
                inbound,
                kinds,
                request.include_heuristic,
                max_edges,
            )
            .await?;
        response.truncated |= truncated;
        let mut node_ids = BTreeSet::from([seed]);
        for edge in &edges {
            node_ids.insert(edge.source.clone());
            node_ids.insert(edge.target.clone());
        }
        response.edges = edges.iter().map(query_edge_from_record).collect();
        response.nodes = self.load_query_nodes(&selector, &node_ids).await?;
        self.publish_response(&selector, response).await
    }

    pub async fn impact(
        &self,
        repository_id: &str,
        request: ImpactRequest,
    ) -> Result<CodeQueryResponse, ProjectionError> {
        let selector = self.pin_query_selector(repository_id).await?;
        self.impact_with_selector(selector, request).await
    }

    pub async fn impact_at(
        &self,
        reference: &SurrealRef,
        request: ImpactRequest,
    ) -> Result<CodeQueryResponse, ProjectionError> {
        let selector = self.pin_reference_selector(reference).await?;
        self.impact_with_selector(selector, request).await
    }

    async fn impact_with_selector(
        &self,
        selector: QuerySelector,
        request: ImpactRequest,
    ) -> Result<CodeQueryResponse, ProjectionError> {
        validate_query_limits(&request.limits)?;
        let mut response =
            CodeQueryResponse::empty(CodeQueryOperation::Impact, request.limits.clone());
        let Some(seed) = self
            .resolve_symbol(&selector, &request.symbol, &mut response)
            .await?
        else {
            return self.publish_response(&selector, response).await;
        };
        let max_depth = usize::try_from(request.limits.max_depth).unwrap_or(usize::MAX);
        let max_nodes = usize::try_from(request.limits.max_nodes).unwrap_or(usize::MAX);
        let max_edges = usize::try_from(request.limits.max_edges).unwrap_or(usize::MAX);
        let mut queue =
            VecDeque::from([(seed.clone(), Vec::<String>::new(), Vec::<String>::new())]);
        let mut visited = BTreeSet::from([seed.clone()]);
        let mut selected = BTreeMap::<String, EdgeRecord>::new();
        while let Some((node, path_nodes, path_edges)) = queue.pop_front() {
            if path_edges.len() >= max_depth {
                continue;
            }
            let remaining = max_edges.saturating_sub(selected.len());
            if remaining == 0 {
                response.truncated = true;
                break;
            }
            let (incoming, truncated) = self
                .matching_relations(
                    &selector,
                    &node,
                    true,
                    IMPACT_KINDS,
                    request.include_heuristic,
                    remaining,
                )
                .await?;
            response.truncated |= truncated;
            for edge in incoming {
                if selected.len() >= max_edges {
                    response.truncated = true;
                    break;
                }
                let edge_id = edge.id.clone();
                if visited.insert(edge.source.clone()) {
                    if visited.len() > max_nodes {
                        visited.remove(&edge.source);
                        response.truncated = true;
                        break;
                    }
                    let mut nodes = path_nodes.clone();
                    if nodes.is_empty() {
                        nodes.push(seed.clone());
                    }
                    nodes.push(edge.source.clone());
                    let mut edges = path_edges.clone();
                    edges.push(edge_id.clone());
                    let mut path_records = edges
                        .iter()
                        .filter_map(|id| selected.get(id).cloned())
                        .collect::<Vec<_>>();
                    path_records.push(edge.clone());
                    response
                        .paths
                        .push(query_path_from_records(&nodes, &edges, &path_records));
                    queue.push_back((edge.source.clone(), nodes, edges));
                }
                selected.insert(edge_id, edge);
            }
            if response.truncated {
                break;
            }
        }
        response.nodes = self.load_query_nodes(&selector, &visited).await?;
        response.edges = selected.values().map(query_edge_from_record).collect();
        self.publish_response(&selector, response).await
    }

    pub async fn node_trail(
        &self,
        repository_id: &str,
        request: NodeTrailRequest,
    ) -> Result<CodeQueryResponse, ProjectionError> {
        let selector = self.pin_query_selector(repository_id).await?;
        self.node_trail_with_selector(selector, request).await
    }

    pub async fn node_trail_at(
        &self,
        reference: &SurrealRef,
        request: NodeTrailRequest,
    ) -> Result<CodeQueryResponse, ProjectionError> {
        let selector = self.pin_reference_selector(reference).await?;
        self.node_trail_with_selector(selector, request).await
    }

    async fn node_trail_with_selector(
        &self,
        selector: QuerySelector,
        request: NodeTrailRequest,
    ) -> Result<CodeQueryResponse, ProjectionError> {
        validate_query_limits(&request.limits)?;
        let mut response =
            CodeQueryResponse::empty(CodeQueryOperation::NodeTrail, request.limits.clone());
        let Some(source) = self
            .resolve_symbol(&selector, &request.source, &mut response)
            .await?
        else {
            return self.publish_response(&selector, response).await;
        };
        let Some(target) = self
            .resolve_symbol(&selector, &request.target, &mut response)
            .await?
        else {
            return self.publish_response(&selector, response).await;
        };
        let mut budget = TraversalBudget::new(&request.limits);
        let (path, selected, truncated) = self
            .shortest_path(
                &selector,
                &source,
                &target,
                request.include_heuristic,
                &request.limits,
                &mut budget,
                true,
            )
            .await?;
        response.truncated |= truncated;
        if let Some((nodes, edges)) = path {
            let node_ids = nodes.iter().cloned().collect::<BTreeSet<_>>();
            response.nodes = self.load_query_nodes(&selector, &node_ids).await?;
            response.edges = selected.values().map(query_edge_from_record).collect();
            let path_records = edges
                .iter()
                .filter_map(|edge| selected.get(edge).cloned())
                .collect::<Vec<_>>();
            response
                .paths
                .push(query_path_from_records(&nodes, &edges, &path_records));
        } else if !truncated {
            let (undirected, _selected, mismatch_truncated) = self
                .shortest_path(
                    &selector,
                    &source,
                    &target,
                    request.include_heuristic,
                    &request.limits,
                    &mut budget,
                    false,
                )
                .await?;
            response.truncated |= mismatch_truncated;
            if undirected.is_some() {
                response.diagnostics.push(QueryDiagnostic {
                    code: QueryDiagnosticCode::DirectionMismatch,
                    message: format!(
                        "A trail connects {source} and {target}, but not in the requested source-to-target direction"
                    ),
                    node_id: Some(source),
                    path: None,
                });
            } else if !mismatch_truncated {
                response.diagnostics.push(QueryDiagnostic {
                    code: QueryDiagnosticCode::NoMatch,
                    message: format!("No bounded directed trail connects {source} to {target}"),
                    node_id: Some(source),
                    path: None,
                });
            }
        }
        self.publish_response(&selector, response).await
    }

    pub async fn structural_subgraph(
        &self,
        repository_id: &str,
        request: ExploreRequest,
    ) -> Result<CodeQueryResponse, ProjectionError> {
        let selector = self.pin_query_selector(repository_id).await?;
        self.structural_subgraph_with_selector(selector, request)
            .await
    }

    pub async fn structural_subgraph_at(
        &self,
        reference: &SurrealRef,
        request: ExploreRequest,
    ) -> Result<CodeQueryResponse, ProjectionError> {
        let selector = self.pin_reference_selector(reference).await?;
        self.structural_subgraph_with_selector(selector, request)
            .await
    }

    async fn structural_subgraph_with_selector(
        &self,
        selector: QuerySelector,
        request: ExploreRequest,
    ) -> Result<CodeQueryResponse, ProjectionError> {
        validate_query_limits(&request.limits)?;
        if request.symbols.len()
            > usize::try_from(request.limits.max_candidates).unwrap_or(usize::MAX)
        {
            return Err(ProjectionError::InvalidQuery(format!(
                "subgraph requested {} symbols but maxCandidates is {}",
                request.symbols.len(),
                request.limits.max_candidates
            )));
        }
        let mut response =
            CodeQueryResponse::empty(CodeQueryOperation::Explore, request.limits.clone());
        let mut seeds = Vec::new();
        for symbol in &request.symbols {
            if let Some(seed) = self
                .resolve_symbol(&selector, symbol, &mut response)
                .await?
            {
                seeds.push(seed);
            }
        }
        seeds.sort();
        seeds.dedup();
        let mut node_ids = seeds.iter().cloned().collect::<BTreeSet<_>>();
        let mut selected = BTreeMap::<String, EdgeRecord>::new();
        let mut budget = TraversalBudget::new(&request.limits);
        for pair in seeds.windows(2) {
            let [source, target] = pair else {
                continue;
            };
            if budget.remaining_nodes == 0 || budget.remaining_edges == 0 {
                response.truncated = true;
                break;
            }
            let (path, path_edges, truncated) = self
                .shortest_path(
                    &selector,
                    source,
                    target,
                    request.include_heuristic,
                    &request.limits,
                    &mut budget,
                    false,
                )
                .await?;
            response.truncated |= truncated;
            if let Some((nodes, edges)) = path {
                selected.extend(path_edges);
                node_ids.extend(nodes.iter().cloned());
                let records = edges
                    .iter()
                    .filter_map(|edge| selected.get(edge).cloned())
                    .collect::<Vec<_>>();
                response
                    .paths
                    .push(query_path_from_records(&nodes, &edges, &records));
            }
        }
        response.nodes = self.load_query_nodes(&selector, &node_ids).await?;
        response.edges = selected.values().map(query_edge_from_record).collect();
        self.publish_response(&selector, response).await
    }

    pub async fn read_relation_page(
        &self,
        repository_id: &str,
        request: RelationPageRequest,
    ) -> Result<RelationPage, ProjectionError> {
        let selector = self.pin_query_selector(repository_id).await?;
        self.read_relation_page_with_selector(selector, request)
            .await
    }

    pub async fn read_relation_page_at(
        &self,
        reference: &SurrealRef,
        request: RelationPageRequest,
    ) -> Result<RelationPage, ProjectionError> {
        let selector = self.pin_reference_selector(reference).await?;
        self.read_relation_page_with_selector(selector, request)
            .await
    }

    async fn read_relation_page_with_selector(
        &self,
        selector: QuerySelector,
        request: RelationPageRequest,
    ) -> Result<RelationPage, ProjectionError> {
        if request.max_items == 0 {
            return Err(ProjectionError::InvalidQuery(
                "relation page maxItems must be greater than zero".to_owned(),
            ));
        }
        let max_items = usize::try_from(request.max_items).unwrap_or(usize::MAX);
        if max_items > self.limits.max_relations() {
            return Err(ProjectionError::LimitExceeded {
                resource: "relation page items",
                actual: u64::from(request.max_items),
                limit: u64::try_from(self.limits.max_relations()).unwrap_or(u64::MAX),
            });
        }
        let after = request
            .cursor
            .as_deref()
            .map(|cursor| decode_cursor(cursor, &selector))
            .transpose()?
            .unwrap_or_default();
        let (relations, truncated) = self
            .page_relations(&selector, &after, request.include_heuristic, max_items)
            .await?;
        self.ensure_selector_current(&selector).await?;
        let next_cursor = if truncated {
            relations
                .last()
                .map(|relation| encode_cursor(&selector, &relation.id))
                .transpose()?
        } else {
            None
        };
        Ok(RelationPage {
            schema: NATIVE_RELATION_PAGE_SCHEMA_V1.to_owned(),
            repository_id: selector.repository_id,
            generation_id: selector.pointer.generation_id,
            relations,
            next_cursor,
        })
    }

    async fn pin_query_selector(
        &self,
        repository_id: &str,
    ) -> Result<QuerySelector, ProjectionError> {
        if repository_id.trim().is_empty() {
            return Err(ProjectionError::EmptyRepositoryId);
        }
        let pointer = select_record::<ActivePointer>(
            &self.database,
            surrealdb::types::RecordId::new("repository_pointer", pointer_key(repository_id)),
            "pin_query_generation",
        )
        .await?
        .ok_or_else(|| ProjectionError::ActiveGenerationUnavailable {
            repository_id: repository_id.to_owned(),
        })?;
        if pointer.schema_version != PROJECTION_SCHEMA_V2 {
            return Err(ProjectionError::UnsupportedProjectionSchema(
                pointer.schema_version,
            ));
        }
        let manifest = select_record::<GenerationManifest>(
            &self.database,
            surrealdb::types::RecordId::new(
                "generation_manifest",
                manifest_key(repository_id, &pointer.generation_id),
            ),
            "pin_query_manifest",
        )
        .await?
        .ok_or_else(|| ProjectionError::ActiveGenerationUnavailable {
            repository_id: repository_id.to_owned(),
        })?;
        if !manifest.complete
            || manifest.schema_version != PROJECTION_SCHEMA_V2
            || manifest.projection_fingerprint != pointer.projection_fingerprint
        {
            return Err(ProjectionError::ActiveGenerationUnavailable {
                repository_id: repository_id.to_owned(),
            });
        }
        validate_manifest_limits(&manifest, self.limits)?;
        Ok(QuerySelector {
            repository_id: repository_id.to_owned(),
            pointer,
            require_current_pointer: true,
        })
    }

    async fn pin_reference_selector(
        &self,
        reference: &SurrealRef,
    ) -> Result<QuerySelector, ProjectionError> {
        self.validate_reference(reference).await?;
        Ok(QuerySelector {
            repository_id: reference.repository_id.clone(),
            pointer: ActivePointer {
                repository_id: reference.repository_id.clone(),
                generation_id: reference.generation_id.clone(),
                schema_version: reference.projection_schema.clone(),
                projection_fingerprint: reference.projection_fingerprint.clone(),
            },
            require_current_pointer: false,
        })
    }

    async fn ensure_selector_current(
        &self,
        selector: &QuerySelector,
    ) -> Result<(), ProjectionError> {
        if !selector.require_current_pointer {
            return Ok(());
        }
        let current = select_record::<ActivePointer>(
            &self.database,
            surrealdb::types::RecordId::new(
                "repository_pointer",
                pointer_key(&selector.repository_id),
            ),
            "revalidate_query_generation",
        )
        .await?;
        if current.as_ref() != Some(&selector.pointer) {
            return Err(ProjectionError::ActiveGenerationChanged {
                repository_id: selector.repository_id.clone(),
            });
        }
        Ok(())
    }

    async fn resolve_symbol(
        &self,
        selector: &QuerySelector,
        symbol: &str,
        response: &mut CodeQueryResponse,
    ) -> Result<Option<String>, ProjectionError> {
        let by_id = self
            .query_nodes(selector, SELECT_NODE_BY_ID, symbol, 2, "query_node_by_id")
            .await?;
        if let [node] = by_id.as_slice() {
            return Ok(Some(node.compass_node_id.clone()));
        }
        let candidate_limit = usize::try_from(response.limits.max_candidates).unwrap_or(usize::MAX);
        let normalized = normalize_query_symbol(symbol);
        let mut exact = self
            .query_nodes(
                selector,
                SELECT_NODES_BY_EXACT_NAME,
                &normalized,
                plus_one(candidate_limit)?,
                "query_nodes_by_name",
            )
            .await?;
        let truncated = exact.len() > candidate_limit;
        if truncated {
            exact.truncate(candidate_limit);
            response.truncated = true;
        }
        match exact.as_slice() {
            [node] if !truncated => Ok(Some(node.compass_node_id.clone())),
            [] => {
                response.diagnostics.push(QueryDiagnostic {
                    code: QueryDiagnosticCode::NoMatch,
                    message: format!("No symbol matched {symbol:?}"),
                    node_id: None,
                    path: None,
                });
                Ok(None)
            }
            _ => {
                response.diagnostics.push(QueryDiagnostic {
                    code: QueryDiagnosticCode::AmbiguousMatch,
                    message: if truncated {
                        format!(
                            "Symbol {symbol:?} exceeded the {}-candidate resolution bound",
                            response.limits.max_candidates
                        )
                    } else {
                        format!("Symbol {symbol:?} matched {} nodes", exact.len())
                    },
                    node_id: None,
                    path: None,
                });
                Ok(None)
            }
        }
    }

    async fn query_nodes(
        &self,
        selector: &QuerySelector,
        statement: &'static str,
        symbol: &str,
        limit: usize,
        stage: &'static str,
    ) -> Result<Vec<ProjectedNode>, ProjectionError> {
        let mut response = self
            .database
            .query(statement)
            .bind(("repository", selector.repository_id.as_str()))
            .bind(("generation", selector.pointer.generation_id.as_str()))
            .bind(("symbol", symbol))
            .bind(("limit", limit))
            .await
            .map_err(|error| database_error(stage, error))?;
        decode_values(&mut response, stage)
    }

    async fn load_query_nodes(
        &self,
        selector: &QuerySelector,
        identities: &BTreeSet<String>,
    ) -> Result<Vec<compass_model::query_contract::QueryNode>, ProjectionError> {
        let mut nodes = Vec::with_capacity(identities.len());
        for identity in identities {
            let records = self
                .query_nodes(selector, SELECT_NODE_BY_ID, identity, 2, "load_query_node")
                .await?;
            let [projected] = records.as_slice() else {
                return Err(ProjectionError::InvalidQuery(format!(
                    "active generation is missing node {identity}"
                )));
            };
            let node: NodeRecord = projected.decode()?;
            nodes.push(query_node_from_record(&node));
        }
        Ok(nodes)
    }

    async fn matching_relations(
        &self,
        selector: &QuerySelector,
        node: &str,
        inbound: bool,
        kinds: &[EdgeKind],
        include_heuristic: bool,
        limit: usize,
    ) -> Result<(Vec<EdgeRecord>, bool), ProjectionError> {
        if limit == 0 {
            return Ok((Vec::new(), true));
        }
        let requested_families = kinds
            .iter()
            .map(|kind| relation_family(*kind))
            .collect::<BTreeSet<_>>();
        let kind_names = kinds
            .iter()
            .map(|kind| kind.as_str().to_owned())
            .collect::<Vec<_>>();
        let mut projected = Vec::new();
        for statements in FAMILY_STATEMENTS {
            if !requested_families.contains(&statements.family) {
                continue;
            }
            let statement = if inbound {
                statements.incoming
            } else {
                statements.outgoing
            };
            let mut response = self
                .database
                .query(statement)
                .bind(("repository", selector.repository_id.as_str()))
                .bind(("generation", selector.pointer.generation_id.as_str()))
                .bind(("node", node))
                .bind(("kinds", kind_names.clone()))
                .bind(("includeHeuristic", include_heuristic))
                .bind(("limit", plus_one(limit)?))
                .await
                .map_err(|error| database_error("query_adjacency", error))?;
            projected.extend(decode_values::<ProjectedRelation>(
                &mut response,
                "query_adjacency",
            )?);
        }
        projected.sort_by(|left, right| left.compass_edge_id.cmp(&right.compass_edge_id));
        let truncated = projected.len() > limit;
        if truncated {
            projected.truncate(limit);
        }
        projected
            .into_iter()
            .map(|relation| relation.decode())
            .collect::<Result<Vec<_>, _>>()
            .map(|relations| (relations, truncated))
    }

    async fn incident_relations(
        &self,
        selector: &QuerySelector,
        node: &str,
        include_heuristic: bool,
        limit: usize,
    ) -> Result<(Vec<EdgeRecord>, bool), ProjectionError> {
        if limit == 0 {
            return Ok((Vec::new(), true));
        }
        let (incoming, incoming_truncated) = self
            .matching_relations(
                selector,
                node,
                true,
                ALL_EDGE_KINDS,
                include_heuristic,
                limit,
            )
            .await?;
        let (outgoing, outgoing_truncated) = self
            .matching_relations(
                selector,
                node,
                false,
                ALL_EDGE_KINDS,
                include_heuristic,
                limit,
            )
            .await?;
        let mut merged = incoming
            .into_iter()
            .chain(outgoing)
            .map(|edge| (edge.id.clone(), edge))
            .collect::<BTreeMap<_, _>>()
            .into_values()
            .collect::<Vec<_>>();
        let truncated = incoming_truncated || outgoing_truncated || merged.len() > limit;
        if merged.len() > limit {
            merged.truncate(limit);
        }
        Ok((merged, truncated))
    }

    // Every parameter is a distinct traversal bound or selector that the
    // caller owns; grouping them into a struct would only move the same
    // values behind one more indirection.
    #[allow(clippy::too_many_arguments)]
    async fn shortest_path(
        &self,
        selector: &QuerySelector,
        source: &str,
        target: &str,
        include_heuristic: bool,
        limits: &CodeQueryLimits,
        budget: &mut TraversalBudget,
        directed: bool,
    ) -> Result<
        (
            Option<(Vec<String>, Vec<String>)>,
            BTreeMap<String, EdgeRecord>,
            bool,
        ),
        ProjectionError,
    > {
        if !budget.consume_node() {
            return Ok((None, BTreeMap::new(), true));
        }
        let max_depth = usize::try_from(limits.max_depth).unwrap_or(usize::MAX);
        // Weighted-shortest trail selection, mirroring the JSON engine's
        // Dijkstra in `compass-query::code_query::CodeQueryEngine::shortest_path`.
        // The priority key is (cost, depth, path_key, node); `best` records the
        // winning (cost, depth, path_key) per node so a stale heap entry is
        // discarded on pop. Every order-dependent decision comes from that
        // explicit total ordering, never from map iteration order.
        let mut queue = BinaryHeap::from([Reverse((
            0_u32,
            0_usize,
            source.to_owned(),
            source.to_owned(),
        ))]);
        let mut best = BTreeMap::from([(source.to_owned(), (0_u32, 0_usize, source.to_owned()))]);
        let mut admitted = BTreeSet::from([source.to_owned()]);
        let mut predecessor = BTreeMap::<String, (String, String)>::new();
        let mut selected = BTreeMap::<String, EdgeRecord>::new();
        let mut truncated = false;
        while let Some(Reverse((cost, depth, path_key, node))) = queue.pop() {
            if best.get(&node).is_none_or(|current| {
                current.0 != cost || current.1 != depth || current.2 != path_key
            }) {
                continue;
            }
            if node == target {
                let mut nodes = vec![target.to_owned()];
                let mut edges = Vec::new();
                let mut cursor = target;
                while cursor != source {
                    let Some((previous, edge)) = predecessor.get(cursor) else {
                        return Ok((None, selected, truncated));
                    };
                    edges.push(edge.clone());
                    nodes.push(previous.clone());
                    cursor = previous;
                }
                nodes.reverse();
                edges.reverse();
                selected.retain(|identity, _| edges.contains(identity));
                return Ok((Some((nodes, edges)), selected, truncated));
            }
            if depth >= max_depth {
                continue;
            }
            let (relations, relation_truncated) = if directed {
                self.matching_relations(
                    selector,
                    &node,
                    false,
                    ALL_EDGE_KINDS,
                    include_heuristic,
                    budget.remaining_edges,
                )
                .await?
            } else {
                self.incident_relations(selector, &node, include_heuristic, budget.remaining_edges)
                    .await?
            };
            truncated |= relation_truncated;
            let mut adjacent = Vec::new();
            for edge in relations {
                if !budget.consume_edge() {
                    truncated = true;
                    break;
                }
                let next = if edge.source == node {
                    Some(edge.target.clone())
                } else if !directed && edge.target == node {
                    Some(edge.source.clone())
                } else {
                    None
                };
                if let Some(next) = next {
                    adjacent.push((next, edge));
                }
            }
            adjacent.sort_by(|left, right| {
                code_relation_weight(left.1.kind)
                    .cmp(&code_relation_weight(right.1.kind))
                    .then_with(|| evidence_quality(&right.1).cmp(&evidence_quality(&left.1)))
                    .then_with(|| left.0.cmp(&right.0))
                    .then_with(|| left.1.id.cmp(&right.1.id))
            });
            for (next, edge) in adjacent {
                let next_depth = depth.saturating_add(1);
                let next_cost = cost.saturating_add(code_relation_weight(edge.kind));
                let next_key = format!("{path_key}\0{}\0{next}", edge.id);
                let candidate = (next_cost, next_depth, next_key.clone());
                if best
                    .get(&next)
                    .is_some_and(|current| candidate >= current.clone())
                {
                    continue;
                }
                if admitted.insert(next.clone()) && !budget.consume_node() {
                    truncated = true;
                    continue;
                }
                best.insert(next.clone(), candidate);
                predecessor.insert(next.clone(), (node.clone(), edge.id.clone()));
                selected.insert(edge.id.clone(), edge);
                queue.push(Reverse((next_cost, next_depth, next_key, next)));
            }
        }
        Ok((None, selected, truncated))
    }

    async fn page_relations(
        &self,
        selector: &QuerySelector,
        after: &str,
        include_heuristic: bool,
        limit: usize,
    ) -> Result<(Vec<QueryEdge>, bool), ProjectionError> {
        let mut projected = Vec::new();
        for statements in FAMILY_STATEMENTS {
            let mut response = self
                .database
                .query(statements.page)
                .bind(("repository", selector.repository_id.as_str()))
                .bind(("generation", selector.pointer.generation_id.as_str()))
                .bind(("after", after))
                .bind(("includeHeuristic", include_heuristic))
                .bind(("limit", plus_one(limit)?))
                .await
                .map_err(|error| database_error("page_relations", error))?;
            projected.extend(decode_values::<ProjectedRelation>(
                &mut response,
                "page_relations",
            )?);
        }
        projected.sort_by(|left, right| left.compass_edge_id.cmp(&right.compass_edge_id));
        let truncated = projected.len() > limit;
        if truncated {
            projected.truncate(limit);
        }
        projected
            .into_iter()
            .map(|relation| relation.decode().map(|edge| query_edge_from_record(&edge)))
            .collect::<Result<Vec<_>, _>>()
            .map(|relations| (relations, truncated))
    }

    async fn publish_response(
        &self,
        selector: &QuerySelector,
        mut response: CodeQueryResponse,
    ) -> Result<CodeQueryResponse, ProjectionError> {
        apply_response_bounds(&mut response);
        if response.truncated {
            response.diagnostics.push(QueryDiagnostic {
                code: QueryDiagnosticCode::BoundedTruncation,
                message: "One or more query bounds truncated the response".to_owned(),
                node_id: None,
                path: None,
            });
        }
        response.sort_stable();
        let response_bytes = u64::try_from(serde_json::to_vec(&response)?.len()).map_err(|_| {
            ProjectionError::LimitExceeded {
                resource: "query response bytes",
                actual: u64::MAX,
                limit: response.limits.max_response_bytes,
            }
        })?;
        if response_bytes > response.limits.max_response_bytes {
            return Err(ProjectionError::LimitExceeded {
                resource: "query response bytes",
                actual: response_bytes,
                limit: response.limits.max_response_bytes,
            });
        }
        self.ensure_selector_current(selector).await?;
        Ok(response)
    }
}

fn validate_query_limits(limits: &CodeQueryLimits) -> Result<(), ProjectionError> {
    if !limits.is_valid() {
        return Err(ProjectionError::InvalidQuery(
            "every native query limit must be greater than zero".to_owned(),
        ));
    }
    if limits.max_candidates > MAX_CODE_QUERY_CANDIDATES {
        return Err(ProjectionError::LimitExceeded {
            resource: "query candidates",
            actual: u64::from(limits.max_candidates),
            limit: u64::from(MAX_CODE_QUERY_CANDIDATES),
        });
    }
    Ok(())
}

fn apply_response_bounds(response: &mut CodeQueryResponse) {
    response.sort_stable();
    let max_nodes = usize::try_from(response.limits.max_nodes).unwrap_or(usize::MAX);
    if response.nodes.len() > max_nodes {
        response.nodes.truncate(max_nodes);
        response.truncated = true;
    }
    let node_ids = response
        .nodes
        .iter()
        .map(|node| node.id.as_str())
        .collect::<BTreeSet<_>>();
    let max_edges = usize::try_from(response.limits.max_edges).unwrap_or(usize::MAX);
    let edge_count = response.edges.len();
    response.edges.retain(|edge| {
        node_ids.contains(edge.source.as_str()) && node_ids.contains(edge.target.as_str())
    });
    if response.edges.len() != edge_count {
        response.truncated = true;
    }
    if response.edges.len() > max_edges {
        response.edges.truncate(max_edges);
        response.truncated = true;
    }
    let edge_ids = response
        .edges
        .iter()
        .map(|edge| edge.id.as_str())
        .collect::<BTreeSet<_>>();
    let path_count = response.paths.len();
    response.paths.retain(|path| {
        path.node_ids
            .iter()
            .all(|node| node_ids.contains(node.as_str()))
            && path
                .edge_ids
                .iter()
                .all(|edge| edge_ids.contains(edge.as_str()))
    });
    if response.paths.len() != path_count {
        response.truncated = true;
    }
    let max_paths = usize::try_from(response.limits.max_paths).unwrap_or(usize::MAX);
    if response.paths.len() > max_paths {
        response.paths.truncate(max_paths);
        response.truncated = true;
    }
}

fn evidence_quality(edge: &EdgeRecord) -> u8 {
    edge.evidence
        .iter()
        .map(|evidence| match evidence.confidence {
            EvidenceConfidence::Exact => 3,
            EvidenceConfidence::Inferred => 2,
            EvidenceConfidence::Ambiguous => 1,
        })
        .max()
        .unwrap_or(0)
}

/// Traversal cost of one relation kind during weighted trail selection.
///
/// Source of truth: `code_relation_weight` in
/// `crates/compass-query/src/code_query.rs`. That function is private to
/// `compass-query`, and `compass-graphdb-surreal` depends on it only as a
/// dev-dependency, so the mapping is replicated verbatim here rather than
/// widening the public contract or adding a normal dependency edge. Both
/// engines must agree exactly: any change there must be mirrored here.
const fn code_relation_weight(kind: EdgeKind) -> u32 {
    match kind {
        EdgeKind::Contains
        | EdgeKind::Calls
        | EdgeKind::Imports
        | EdgeKind::Extends
        | EdgeKind::Implements
        | EdgeKind::RoutesTo
        | EdgeKind::Handles
        | EdgeKind::DependsOn => 1,
        EdgeKind::References | EdgeKind::Documents => 4,
        _ => 2,
    }
}

fn encode_cursor(selector: &QuerySelector, last_identity: &str) -> Result<String, ProjectionError> {
    let payload = CursorPayload {
        schema: CURSOR_SCHEMA_V1.to_owned(),
        repository_digest: repository_digest(&selector.repository_id),
        generation_id: selector.pointer.generation_id.clone(),
        operation: CURSOR_OPERATION_RELATIONS.to_owned(),
        last_identity: last_identity.to_owned(),
    };
    let checksum = digest_bytes(&serde_json::to_vec(&payload)?);
    let encoded = serde_json::to_vec(&CursorEnvelope { payload, checksum })?;
    Ok(hex_encode(&encoded))
}

fn decode_cursor(cursor: &str, selector: &QuerySelector) -> Result<String, ProjectionError> {
    if cursor.len() > MAX_CURSOR_BYTES.saturating_mul(2) {
        return Err(ProjectionError::InvalidCursor(
            "cursor exceeds the encoded byte limit".to_owned(),
        ));
    }
    let decoded = hex_decode(cursor)?;
    let envelope = serde_json::from_slice::<CursorEnvelope>(&decoded)
        .map_err(|error| ProjectionError::InvalidCursor(error.to_string()))?;
    let expected_checksum = digest_bytes(&serde_json::to_vec(&envelope.payload)?);
    if envelope.checksum != expected_checksum
        || envelope.payload.schema != CURSOR_SCHEMA_V1
        || envelope.payload.repository_digest != repository_digest(&selector.repository_id)
        || envelope.payload.generation_id != selector.pointer.generation_id
        || envelope.payload.operation != CURSOR_OPERATION_RELATIONS
        || envelope.payload.last_identity.is_empty()
    {
        return Err(ProjectionError::InvalidCursor(
            "cursor does not match the selected repository, generation, or operation".to_owned(),
        ));
    }
    Ok(envelope.payload.last_identity)
}

fn repository_digest(repository_id: &str) -> String {
    digest_bytes(repository_id.as_bytes())
}

fn digest_bytes(bytes: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update(bytes);
    format!("sha256:{}", hex_encode(&digest.finalize()))
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

fn hex_decode(value: &str) -> Result<Vec<u8>, ProjectionError> {
    if !value.len().is_multiple_of(2) {
        return Err(ProjectionError::InvalidCursor(
            "cursor hexadecimal length is invalid".to_owned(),
        ));
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = hex_nibble(pair[0])?;
            let low = hex_nibble(pair[1])?;
            Ok((high << 4) | low)
        })
        .collect()
}

fn hex_nibble(value: u8) -> Result<u8, ProjectionError> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        b'A'..=b'F' => Ok(value - b'A' + 10),
        _ => Err(ProjectionError::InvalidCursor(
            "cursor contains a non-hexadecimal character".to_owned(),
        )),
    }
}

fn decode_values<T>(
    response: &mut surrealdb::IndexedResults,
    stage: &'static str,
) -> Result<Vec<T>, ProjectionError>
where
    T: for<'de> Deserialize<'de>,
{
    let values: Vec<Value> = response
        .take(0)
        .map_err(|error| database_error(stage, error))?;
    values
        .into_iter()
        .map(serde_json::from_value)
        .collect::<Result<_, _>>()
        .map_err(Into::into)
}
