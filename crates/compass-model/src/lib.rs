//! Typed model for Compass node-link graphs.

/// Default bounded size accepted by current graph readers.
pub const DEFAULT_GRAPH_SIZE_CAP_BYTES: u64 = 1024 * 1024 * 1024;

mod artifact_compatibility;
pub mod code_graph;
mod document;
mod error;
mod graph;
pub mod identity;
mod lexical;
mod lexical_index;
pub mod provenance;
pub mod query_contract;
mod query_index;
pub mod search;
mod validation;

pub use artifact_compatibility::{
    MAX_GRAPH_PREAMBLE_BYTES, MIN_QUERY_BUILDER_VERSION, validate_graph_preamble,
    validate_query_builder_version,
};
pub use document::{EdgeRecord, GraphDocument, NodeRecord};
pub use error::GraphError;
pub use graph::{EdgeIndex, Graph, NodeIndex};
pub use lexical::{canonical_code_token, identifier_tokens, strip_diacritics};
pub use lexical_index::{LexicalIndex, LexicalPosting, LexicalTermFrequency};
pub use query_index::{
    QueryIndex, SchemaFingerprint, SchemaFingerprintBuilder, cypher_node_label,
    cypher_node_label_from_kind, cypher_relationship_type, cypher_relationship_type_from_relation,
};
pub use validation::{
    CodeGraphValidationError, CodeGraphValidationReport, ExtractionValidationError,
    RecordValidationErrors, assert_valid_extraction, validate_build_metadata_identity,
    validate_code_graph, validate_code_graph_records, validate_extraction,
};
