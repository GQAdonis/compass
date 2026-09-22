//! Storage-independent record access for the CompassQL semantic executor.
//! Database engines supply bounded selectors, not a hydrated `Graph`.

use std::borrow::Cow;

use compass_cypher::{Direction, RelationshipPattern};
use compass_model::{EdgeIndex, EdgeRecord, Graph, NodeIndex, NodeRecord};

use super::QueryError;

pub(crate) enum NodeSelection<'a> {
    All,
    Id(&'a str),
    Label(&'a str),
    DisplayLabel(&'a str),
    SourceFile(&'a str),
}

pub(crate) trait QueryGraph {
    fn schema_fingerprint(&self) -> String;
    fn node(&self, index: NodeIndex) -> Result<Cow<'_, NodeRecord>, QueryError>;
    fn edge(&self, index: EdgeIndex) -> Result<Cow<'_, EdgeRecord>, QueryError>;
    fn select_nodes(&self, selection: NodeSelection<'_>) -> Result<Vec<NodeIndex>, QueryError>;
    fn adjacent(
        &self,
        node: NodeIndex,
        pattern: &RelationshipPattern,
    ) -> Result<Vec<(EdgeIndex, NodeIndex)>, QueryError>;
    /// Live incident-edge count, exposed to CompassQL as the `degree` property.
    fn degree(&self, node: NodeIndex) -> Result<usize, QueryError>;
}

impl QueryGraph for Graph {
    fn schema_fingerprint(&self) -> String {
        Graph::schema_fingerprint(self).to_hex()
    }
    fn node(&self, index: NodeIndex) -> Result<Cow<'_, NodeRecord>, QueryError> {
        Ok(Cow::Borrowed(Graph::node(self, index)))
    }
    fn edge(&self, index: EdgeIndex) -> Result<Cow<'_, EdgeRecord>, QueryError> {
        Ok(Cow::Borrowed(Graph::edge(self, index)))
    }
    fn select_nodes(&self, selection: NodeSelection<'_>) -> Result<Vec<NodeIndex>, QueryError> {
        Ok(match selection {
            NodeSelection::All => (0..self.node_count()).collect(),
            NodeSelection::Id(id) => self.node_index(id).into_iter().collect(),
            NodeSelection::Label(label) => self.query_index().nodes_with_label(label).to_vec(),
            NodeSelection::DisplayLabel(label) => {
                self.query_index().nodes_with_display_label(label).to_vec()
            }
            NodeSelection::SourceFile(path) => {
                self.query_index().nodes_with_source_file(path).to_vec()
            }
        })
    }
    fn degree(&self, node: NodeIndex) -> Result<usize, QueryError> {
        Ok(Graph::degree(self, node))
    }
    fn adjacent(
        &self,
        node: NodeIndex,
        pattern: &RelationshipPattern,
    ) -> Result<Vec<(EdgeIndex, NodeIndex)>, QueryError> {
        let outgoing = || -> Vec<EdgeIndex> {
            if self.is_directed() && !pattern.types.is_empty() {
                pattern
                    .types
                    .iter()
                    .flat_map(|relation| self.query_index().outgoing_with_type(node, relation))
                    .copied()
                    .collect()
            } else {
                self.outgoing_edges(node).collect()
            }
        };
        let incoming = || -> Vec<EdgeIndex> {
            if self.is_directed() && !pattern.types.is_empty() {
                pattern
                    .types
                    .iter()
                    .flat_map(|relation| self.query_index().incoming_with_type(node, relation))
                    .copied()
                    .collect()
            } else {
                self.incoming_edges(node).collect()
            }
        };
        let mut output = Vec::new();
        let mut append = |edges: Vec<EdgeIndex>| {
            for edge in edges {
                if let Some((source, target)) = self.edge_endpoints(edge)
                    && (pattern.types.is_empty()
                        || pattern.types.iter().any(|relation| {
                            compass_model::cypher_relationship_type(Graph::edge(self, edge))
                                == *relation
                        }))
                {
                    output.push((edge, if source == node { target } else { source }));
                }
            }
        };
        if matches!(
            pattern.direction,
            Direction::Outgoing | Direction::Undirected
        ) {
            append(outgoing());
        }
        if matches!(
            pattern.direction,
            Direction::Incoming | Direction::Undirected
        ) {
            append(incoming());
        }
        output.sort_unstable();
        output.dedup();
        Ok(output)
    }
}
