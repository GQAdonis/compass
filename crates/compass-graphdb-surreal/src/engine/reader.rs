//! Indexed record reads for the shared Compass query semantics. Every selector
//! carries the exact immutable reference; no active pointer or graph file is read.

use compass_model::code_graph::{EdgeKind, EdgeRecord, FileRecord, GraphMetadata, NodeRecord};
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::{SurrealProjection, database_error, plus_one};
use crate::{
    ProjectedFile, ProjectedNode, ProjectedRelation, ProjectionError, RelationFamily, SurrealRef,
    relation_family,
};

/// Closed selectors prevent callers from supplying executable SurrealQL.
pub enum NodeSelector<'a> {
    Id(&'a str),
    NormalizedName(&'a str),
    AllTerms(&'a [String]),
}

pub enum EdgeSelector<'a> {
    Id(&'a str),
    Adjacent {
        node: &'a str,
        inbound: bool,
        kinds: &'a [EdgeKind],
    },
    Incident {
        node: &'a str,
        include_heuristic: bool,
    },
}

pub enum CqlNodeSelector<'a> {
    All,
    Id(&'a str),
    Label(&'a str),
    DisplayLabel(&'a str),
    SourceFile(&'a str),
}

#[derive(Clone, Copy)]
pub enum CqlDirection {
    Incoming,
    Outgoing,
    Both,
}

impl SurrealProjection {
    pub async fn cql_is_directed_at(
        &self,
        reference: &SurrealRef,
    ) -> Result<bool, ProjectionError> {
        Ok(self.manifest_for_reference(reference).await?.directed)
    }

    pub async fn cql_schema_fingerprint_at(
        &self,
        reference: &SurrealRef,
    ) -> Result<String, ProjectionError> {
        Ok(self
            .manifest_for_reference(reference)
            .await?
            .cql_schema_fingerprint)
    }

    pub async fn cql_node_at(
        &self,
        reference: &SurrealRef,
        ordinal: usize,
    ) -> Result<ProjectedNode, ProjectionError> {
        self.validate_reference(reference).await?;
        let mut response = self.database.query("SELECT * OMIT id FROM code_node WHERE repositoryId = $repository AND generationId = $generation AND ordinal = $ordinal LIMIT 2")
            .bind(("repository", reference.repository_id.as_str())).bind(("generation", reference.generation_id.as_str())).bind(("ordinal", ordinal))
            .await.map_err(|error| database_error("cql_node", error))?;
        let values: Vec<Value> = response
            .take(0)
            .map_err(|error| database_error("cql_node", error))?;
        exactly_one(decode_rows(values)?, "node ordinal")
    }

    pub async fn cql_edge_at(
        &self,
        reference: &SurrealRef,
        ordinal: usize,
    ) -> Result<ProjectedRelation, ProjectionError> {
        self.validate_reference(reference).await?;
        let mut rows = Vec::new();
        for family in RelationFamily::ALL {
            let mut response = self.database.query("SELECT * OMIT id, in, out FROM type::table($table) WHERE repositoryId = $repository AND generationId = $generation AND ordinal = $ordinal LIMIT 2")
                .bind(("table", family.as_str())).bind(("repository", reference.repository_id.as_str())).bind(("generation", reference.generation_id.as_str())).bind(("ordinal", ordinal))
                .await.map_err(|error| database_error("cql_edge", error))?;
            let values: Vec<Value> = response
                .take(0)
                .map_err(|error| database_error("cql_edge", error))?;
            rows.extend(decode_rows::<ProjectedRelation>(values)?);
        }
        exactly_one(rows, "relationship ordinal")
    }

    /// Return only ordered identity rows for a logical node scan. Payloads are
    /// fetched on demand as semantic operators inspect individual bindings.
    pub async fn cql_scan_at(
        &self,
        reference: &SurrealRef,
        selector: CqlNodeSelector<'_>,
        limit: usize,
    ) -> Result<Vec<usize>, ProjectionError> {
        self.validate_reference(reference).await?;
        let limit = limit.min(self.limits.max_nodes());
        let (condition, value) = match selector {
            CqlNodeSelector::All => ("true", ""),
            CqlNodeSelector::Id(id) => ("compassNodeId = $value", id),
            CqlNodeSelector::Label(label) => ("cqlLabel = $value", label),
            CqlNodeSelector::DisplayLabel(label) => ("name = $value", label),
            CqlNodeSelector::SourceFile(path) => ("sourcePath = $value", path),
        };
        let statement = format!(
            "SELECT VALUE ordinal FROM code_node WHERE repositoryId = $repository AND generationId = $generation AND {condition} ORDER BY ordinal LIMIT $limit"
        );
        let mut response = self
            .database
            .query(statement)
            .bind(("repository", reference.repository_id.as_str()))
            .bind(("generation", reference.generation_id.as_str()))
            .bind(("value", value))
            .bind(("limit", plus_one(limit)?))
            .await
            .map_err(|error| database_error("cql_scan", error))?;
        let rows: Vec<usize> = response
            .take(0)
            .map_err(|error| database_error("cql_scan", error))?;
        enforce_rows(rows.len(), limit)?;
        Ok(rows)
    }

    pub async fn cql_adjacent_at(
        &self,
        reference: &SurrealRef,
        node: usize,
        direction: CqlDirection,
        types: &[String],
        limit: usize,
    ) -> Result<Vec<(usize, usize)>, ProjectionError> {
        let current = self.cql_node_at(reference, node).await?;
        let limit = limit.min(self.limits.max_relations());
        let condition = match direction {
            CqlDirection::Incoming => "targetNodeId = $node",
            CqlDirection::Outgoing => "sourceNodeId = $node",
            CqlDirection::Both => "(sourceNodeId = $node OR targetNodeId = $node)",
        };
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct AdjacentRow {
            ordinal: u64,
            source_node_id: String,
            target_node_id: String,
        }
        let mut relations = Vec::new();
        for family in RelationFamily::ALL {
            let remaining = limit.saturating_sub(relations.len());
            let statement = format!(
                "SELECT ordinal, sourceNodeId, targetNodeId FROM type::table($table) WHERE repositoryId = $repository AND generationId = $generation AND {condition} AND ($allTypes OR cqlType IN $types) ORDER BY ordinal LIMIT $limit"
            );
            let mut response = self
                .database
                .query(statement)
                .bind(("table", family.as_str()))
                .bind(("repository", reference.repository_id.as_str()))
                .bind(("generation", reference.generation_id.as_str()))
                .bind(("node", current.compass_node_id.as_str()))
                .bind(("types", types.to_vec()))
                .bind(("allTypes", types.is_empty()))
                .bind(("limit", plus_one(remaining)?))
                .await
                .map_err(|error| database_error("cql_adjacency", error))?;
            let values: Vec<Value> = response
                .take(0)
                .map_err(|error| database_error("cql_adjacency", error))?;
            relations.extend(decode_rows::<AdjacentRow>(values)?);
            enforce_rows(relations.len(), limit)?;
        }
        let ids = relations
            .iter()
            .map(|edge| {
                if edge.source_node_id == current.compass_node_id {
                    edge.target_node_id.clone()
                } else {
                    edge.source_node_id.clone()
                }
            })
            .collect::<std::collections::BTreeSet<_>>();
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Endpoint {
            compass_node_id: String,
            ordinal: usize,
        }
        let mut response = self.database.query("SELECT compassNodeId, ordinal FROM code_node WHERE repositoryId = $repository AND generationId = $generation AND compassNodeId IN $ids LIMIT $limit")
            .bind(("repository", reference.repository_id.as_str())).bind(("generation", reference.generation_id.as_str()))
            .bind(("ids", ids.iter().cloned().collect::<Vec<_>>())).bind(("limit", plus_one(ids.len())?))
            .await.map_err(|error| database_error("cql_endpoints", error))?;
        let values: Vec<Value> = response
            .take(0)
            .map_err(|error| database_error("cql_endpoints", error))?;
        let endpoints = decode_rows::<Endpoint>(values)?;
        let ordinals = endpoints
            .into_iter()
            .map(|endpoint| (endpoint.compass_node_id, endpoint.ordinal))
            .collect::<std::collections::BTreeMap<_, _>>();
        if ordinals.len() != ids.len() {
            return Err(ProjectionError::InvalidPlan(
                "CQL adjacency endpoint identity mismatch".to_owned(),
            ));
        }
        let mut result = relations
            .into_iter()
            .map(|edge| {
                let neighbor = if edge.source_node_id == current.compass_node_id {
                    &edge.target_node_id
                } else {
                    &edge.source_node_id
                };
                let ordinal = ordinals.get(neighbor).copied().ok_or_else(|| {
                    ProjectionError::InvalidPlan("missing CQL endpoint".to_owned())
                })?;
                Ok((
                    usize::try_from(edge.ordinal).map_err(|_| {
                        ProjectionError::InvalidPlan("CQL relationship ordinal overflow".to_owned())
                    })?,
                    ordinal,
                ))
            })
            .collect::<Result<Vec<_>, ProjectionError>>()?;
        result.sort_unstable();
        result.dedup();
        Ok(result)
    }
    pub async fn metadata_at(
        &self,
        reference: &SurrealRef,
    ) -> Result<GraphMetadata, ProjectionError> {
        let manifest = self.manifest_for_reference(reference).await?;
        crate::projection::decode_metadata(&manifest.metadata_json)
    }

    pub async fn nodes_at(
        &self,
        reference: &SurrealRef,
        selector: NodeSelector<'_>,
        limit: usize,
    ) -> Result<(Vec<NodeRecord>, bool), ProjectionError> {
        self.validate_reference(reference).await?;
        check_limit(limit, self.limits.max_nodes())?;
        let (condition, symbol, terms) = match selector {
            NodeSelector::Id(id) => ("compassNodeId = $symbol", id, &[][..]),
            NodeSelector::NormalizedName(name) => {
                ("normalizedNames CONTAINS $symbol", name, &[][..])
            }
            NodeSelector::AllTerms(terms) => {
                if terms.is_empty() {
                    return Ok((Vec::new(), false));
                }
                // Match the JSON/SQLite token-prefix contract, including a
                // partial final identifier component (list -> listing). The
                // short-prefix index narrows the pinned record set before the
                // full-prefix predicate. Only bounded matched rows leave DB.
                (
                    "searchPrefixes CONTAINSALL $prefixes AND array::all($terms, |$prefix| array::any(searchTerms, |$term| string::starts_with($term, $prefix)))",
                    "",
                    terms,
                )
            }
        };
        let statement = format!(
            "SELECT * OMIT id FROM code_node WHERE repositoryId = $repository AND generationId = $generation AND {condition} ORDER BY compassNodeId LIMIT $limit"
        );
        let mut response = self
            .database
            .query(statement)
            .bind(("repository", reference.repository_id.as_str()))
            .bind(("generation", reference.generation_id.as_str()))
            .bind(("symbol", symbol))
            .bind(("terms", terms.to_vec()))
            .bind((
                "prefixes",
                terms
                    .iter()
                    .map(|term| term.chars().take(3).collect::<String>())
                    .collect::<Vec<_>>(),
            ))
            .bind(("limit", plus_one(limit)?))
            .await
            .map_err(|error| database_error("indexed_nodes", error))?;
        let values: Vec<Value> = response
            .take(0)
            .map_err(|error| database_error("indexed_nodes", error))?;
        let mut rows = decode_rows::<ProjectedNode>(values)?;
        let truncated = rows.len() > limit;
        rows.truncate(limit);
        let nodes = rows
            .into_iter()
            .map(|row| {
                let node = row.decode()?;
                if row.repository_id != reference.repository_id
                    || row.generation_id != reference.generation_id
                    || row.compass_node_id != node.id
                {
                    return Err(ProjectionError::InvalidPlan(
                        "indexed node identity mismatch".to_owned(),
                    ));
                }
                Ok(node)
            })
            .collect::<Result<_, _>>()?;
        Ok((nodes, truncated))
    }

    pub async fn edges_at(
        &self,
        reference: &SurrealRef,
        selector: EdgeSelector<'_>,
        limit: usize,
    ) -> Result<(Vec<EdgeRecord>, bool), ProjectionError> {
        self.validate_reference(reference).await?;
        check_limit(limit, self.limits.max_relations())?;
        let (condition, symbol, kinds, include_heuristic) = match &selector {
            EdgeSelector::Id(id) => ("compassEdgeId = $symbol", *id, Vec::new(), true),
            EdgeSelector::Adjacent {
                node,
                inbound,
                kinds,
            } => (
                if *inbound {
                    "targetNodeId = $symbol AND kind IN $kinds"
                } else {
                    "sourceNodeId = $symbol AND kind IN $kinds"
                },
                *node,
                kinds.iter().map(|kind| kind.as_str()).collect::<Vec<_>>(),
                true,
            ),
            EdgeSelector::Incident {
                node,
                include_heuristic,
            } => (
                "(sourceNodeId = $symbol OR targetNodeId = $symbol) AND ($includeHeuristic OR heuristic = false)",
                *node,
                Vec::new(),
                *include_heuristic,
            ),
        };
        let mut rows = Vec::new();
        let mut truncated = false;
        for family in RelationFamily::ALL {
            if let EdgeSelector::Adjacent { kinds, .. } = &selector
                && !kinds.iter().any(|kind| relation_family(*kind) == family)
            {
                continue;
            }
            let statement = format!(
                "SELECT * OMIT id, in, out FROM type::table($table) WHERE repositoryId = $repository AND generationId = $generation AND {condition} ORDER BY compassEdgeId LIMIT $limit"
            );
            let mut response = self
                .database
                .query(statement)
                .bind(("table", family.as_str()))
                .bind(("repository", reference.repository_id.as_str()))
                .bind(("generation", reference.generation_id.as_str()))
                .bind(("symbol", symbol))
                .bind(("kinds", kinds.clone()))
                .bind(("includeHeuristic", include_heuristic))
                .bind(("limit", plus_one(limit)?))
                .await
                .map_err(|error| database_error("indexed_edges", error))?;
            let values: Vec<Value> = response
                .take(0)
                .map_err(|error| database_error("indexed_edges", error))?;
            rows.extend(decode_rows::<ProjectedRelation>(values)?);
            rows.sort_by(|left, right| left.compass_edge_id.cmp(&right.compass_edge_id));
            truncated |= rows.len() > limit;
            rows.truncate(limit);
        }
        let edges = rows
            .into_iter()
            .map(|row| {
                let edge = row.decode()?;
                if row.repository_id != reference.repository_id
                    || row.generation_id != reference.generation_id
                    || row.compass_edge_id != edge.id
                {
                    return Err(ProjectionError::InvalidPlan(
                        "indexed edge identity mismatch".to_owned(),
                    ));
                }
                Ok(edge)
            })
            .collect::<Result<_, _>>()?;
        Ok((edges, truncated))
    }

    pub async fn file_at(
        &self,
        reference: &SurrealRef,
        path: &str,
    ) -> Result<Option<FileRecord>, ProjectionError> {
        self.validate_reference(reference).await?;
        let mut response = self.database.query("SELECT * OMIT id FROM code_file WHERE repositoryId = $repository AND generationId = $generation AND path = $path LIMIT 2")
            .bind(("repository", reference.repository_id.as_str()))
            .bind(("generation", reference.generation_id.as_str()))
            .bind(("path", path))
            .await.map_err(|error| database_error("indexed_file", error))?;
        let values: Vec<Value> = response
            .take(0)
            .map_err(|error| database_error("indexed_file", error))?;
        let mut rows = decode_rows::<ProjectedFile>(values)?;
        if rows.len() > 1 {
            return Err(ProjectionError::InvalidPlan(
                "duplicate file identity".to_owned(),
            ));
        }
        rows.pop()
            .map(|row| {
                let file = row.decode()?;
                if row.repository_id != reference.repository_id
                    || row.generation_id != reference.generation_id
                    || file.path != path
                    || row.path != path
                {
                    return Err(ProjectionError::InvalidPlan(
                        "indexed file identity mismatch".to_owned(),
                    ));
                }
                Ok(file)
            })
            .transpose()
    }
}

fn decode_rows<T: DeserializeOwned>(rows: Vec<Value>) -> Result<Vec<T>, ProjectionError> {
    rows.into_iter()
        .map(|row| serde_json::from_value(row).map_err(Into::into))
        .collect()
}

fn check_limit(limit: usize, cap: usize) -> Result<(), ProjectionError> {
    if limit > cap {
        return Err(ProjectionError::LimitExceeded {
            resource: "indexed query rows",
            actual: limit as u64,
            limit: cap as u64,
        });
    }
    Ok(())
}

fn exactly_one<T>(mut rows: Vec<T>, kind: &str) -> Result<T, ProjectionError> {
    if rows.len() != 1 {
        return Err(ProjectionError::InvalidPlan(format!(
            "expected exactly one {kind}, found {}",
            rows.len()
        )));
    }
    rows.pop()
        .ok_or_else(|| ProjectionError::InvalidPlan(format!("missing {kind}")))
}

fn enforce_rows(actual: usize, limit: usize) -> Result<(), ProjectionError> {
    if actual > limit {
        return Err(ProjectionError::LimitExceeded {
            resource: "CQL identity rows",
            actual: actual as u64,
            limit: limit as u64,
        });
    }
    Ok(())
}
