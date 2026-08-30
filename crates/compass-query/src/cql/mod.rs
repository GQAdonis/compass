mod cache;
mod error;
mod eval;
mod execute;
pub(crate) mod graph_access;
#[cfg(any(
    feature = "surreal-surrealkv",
    feature = "surreal-rocksdb",
    feature = "surreal-remote"
))]
pub(crate) use execute::{StorageQueryRequest, execute_on_storage};
mod profile;

pub use cache::{CacheStats, PlanCache, PlanCacheConfig};
pub use error::{QueryError, QueryErrorKind};
pub use execute::{ExplainPlan, QueryLimits, QueryRequest, QueryResult, execute};
pub use profile::{OperatorProfile, QueryProfile};
