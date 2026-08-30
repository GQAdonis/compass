#![forbid(unsafe_code)]

//! Optional generation-atomic SurrealDB graph projection for Compass.
//!
//! Projection planning is available without an engine feature. The SurrealDB
//! SDK is compiled only when `mem`, `surrealkv`, or `rocksdb` is selected.

mod projection;
mod reference;

pub use projection::{
    ProjectedFile, ProjectedNode, ProjectedRelation, ProjectionError, ProjectionLimits,
    ProjectionPlan, RelationFamily, relation_family,
};
pub use reference::{
    MAX_SURREAL_REF_BYTES, ProjectionBundle, SURREAL_BUNDLE_SCHEMA_V1, SURREAL_DATABASE,
    SURREAL_NAMESPACE, SURREAL_REF_FILE_NAME, SURREAL_REF_SCHEMA_V1, SurrealEngine, SurrealRef,
};

#[cfg(any(feature = "mem", feature = "surrealkv", feature = "rocksdb"))]
mod engine;

#[cfg(any(feature = "mem", feature = "surrealkv", feature = "rocksdb"))]
pub use engine::{
    ActivationOutcome, CqlDirection, CqlNodeSelector, CqlProjectionRequest, CqlProjectionSlice,
    EdgeSelector, GenerationGcStats, InterruptAfter, NATIVE_RELATION_PAGE_SCHEMA_V1, NodeSelector,
    RelationPage, RelationPageRequest, SurrealProjection,
};

/// Legacy library-only projection schema, retained for explicit rejection.
pub const PROJECTION_SCHEMA_V1: &str = "compass.graph.surreal/1";

/// Fully wired Compass-to-Surreal projection schema.
pub const PROJECTION_SCHEMA_V2: &str = "compass.graph.surreal/2";

/// Exact reviewed SurrealDB release selected by every engine feature.
pub const SURREALDB_VERSION: &str = "3.2.4";
