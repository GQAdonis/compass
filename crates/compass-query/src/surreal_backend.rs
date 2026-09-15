//! Synchronous adapter used only inside `spawn_blocking`. Shared query semantics
//! perform bounded indexed reads on the caller's live async runtime.

use std::sync::Arc;

use compass_graphdb_surreal::{EdgeSelector, NodeSelector, SurrealProjection, SurrealRef};
use compass_model::code_graph::{EdgeKind, EdgeRecord, FileRecord, GraphMetadata, NodeRecord};

use crate::code_query::TermCandidateRead;
use crate::surreal::projection_error;
use crate::{QueryError, QueryErrorKind};

pub(crate) struct SurrealCodeBackend {
    pub(crate) projection: Arc<SurrealProjection>,
    pub(crate) reference: SurrealRef,
    pub(crate) metadata: GraphMetadata,
    pub(crate) runtime: tokio::runtime::Handle,
}

impl SurrealCodeBackend {
    pub(crate) fn node_by_id(&self, id: &str) -> Result<Option<NodeRecord>, QueryError> {
        let (mut nodes, truncated) = self
            .runtime
            .block_on(
                self.projection
                    .nodes_at(&self.reference, NodeSelector::Id(id), 1),
            )
            .map_err(projection_error)?;
        if truncated {
            return Err(duplicate_identity("node"));
        }
        Ok(nodes.pop())
    }

    pub(crate) fn edge_by_id(&self, id: &str) -> Result<Option<EdgeRecord>, QueryError> {
        let (mut edges, truncated) = self
            .runtime
            .block_on(
                self.projection
                    .edges_at(&self.reference, EdgeSelector::Id(id), 1),
            )
            .map_err(projection_error)?;
        if truncated {
            return Err(duplicate_identity("edge"));
        }
        Ok(edges.pop())
    }

    pub(crate) fn nodes_by_normalized_name(
        &self,
        name: &str,
        limit: usize,
    ) -> Result<(Vec<NodeRecord>, bool), QueryError> {
        self.runtime
            .block_on(self.projection.nodes_at(
                &self.reference,
                NodeSelector::NormalizedName(name),
                limit,
            ))
            .map_err(projection_error)
    }

    pub(crate) fn matching_bounded(
        &self,
        node: &str,
        inbound: bool,
        kinds: &[EdgeKind],
        include_heuristic: bool,
        limit: usize,
    ) -> Result<(Vec<EdgeRecord>, bool), QueryError> {
        let (mut edges, truncated) = self
            .runtime
            .block_on(self.projection.edges_at(
                &self.reference,
                EdgeSelector::Adjacent {
                    node,
                    inbound,
                    kinds,
                },
                limit,
            ))
            .map_err(projection_error)?;
        if !include_heuristic {
            edges.retain(|edge| {
                !edge.evidence.iter().any(|evidence| {
                    evidence.origin == compass_model::provenance::EvidenceOrigin::Heuristic
                })
            });
        }
        Ok((edges, truncated))
    }

    pub(crate) fn incident_bounded(
        &self,
        node: &str,
        include_heuristic: bool,
        limit: usize,
    ) -> Result<(Vec<EdgeRecord>, bool), QueryError> {
        self.runtime
            .block_on(self.projection.edges_at(
                &self.reference,
                EdgeSelector::Incident {
                    node,
                    include_heuristic,
                },
                limit,
            ))
            .map_err(projection_error)
    }

    pub(crate) fn file_by_path(&self, path: &str) -> Result<Option<FileRecord>, QueryError> {
        self.runtime
            .block_on(self.projection.file_at(&self.reference, path))
            .map_err(projection_error)
    }

    pub(crate) fn term_candidates(
        &self,
        terms: &[String],
        limit: usize,
    ) -> Result<TermCandidateRead, QueryError> {
        let (nodes, truncated) = self
            .runtime
            .block_on(self.projection.nodes_at(
                &self.reference,
                NodeSelector::AllTerms(terms),
                limit,
            ))
            .map_err(projection_error)?;
        let concepts = terms
            .iter()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        Ok(TermCandidateRead {
            matched_concepts: nodes
                .iter()
                .map(|node| (node.id.clone(), concepts.clone()))
                .collect(),
            node_ids_decoded: u64::try_from(nodes.len()).unwrap_or(u64::MAX),
            chunks_decoded: 0,
            nodes,
            truncated,
        })
    }
}

fn duplicate_identity(kind: &str) -> QueryError {
    QueryError::new(
        QueryErrorKind::CorruptArtifact,
        "surreal_duplicate_identity",
        format!("Surreal projection has duplicate {kind} identities"),
    )
}
