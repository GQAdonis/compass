//! One bounded physical connection owner per canonical embedded store path.
//!
//! SDK routers outlive individual sessions and shut down asynchronously. They
//! must not belong to a publication's short-lived runtime: aborting that runtime
//! can strand the database lock before the next watch or MCP publication.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::LazyLock;

use surrealdb::Surreal;
use surrealdb::engine::any::Any;
use tokio::runtime::{Builder, Runtime};
use tokio::sync::Mutex;

use super::database_error;
use crate::{ProjectionError, SurrealEngine};

const MAX_STORES: usize = 16;
const ROUTER_CAPACITY: usize = 256;
type Connections = BTreeMap<PathBuf, (SurrealEngine, Surreal<Any>)>;

static RUNTIME: LazyLock<Result<Runtime, std::io::Error>> = LazyLock::new(|| {
    Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(4)
        .thread_name("compass-surreal")
        .enable_all()
        .build()
});
// Retain physical owners for the process lifetime. Evicting a client does not
// synchronously release its lock; an LRU would permit a racy reopen. Sessions
// are cloned from these owners and independently select their namespace/db.
static CONNECTIONS: LazyLock<Mutex<Connections>> = LazyLock::new(Mutex::default);

pub(super) async fn client(
    engine: SurrealEngine,
    path: &str,
    create: bool,
) -> Result<Surreal<Any>, ProjectionError> {
    let runtime = RUNTIME
        .as_ref()
        .map_err(|error| database_error("embedded_runtime", error))?;
    let path = path.to_owned();
    // The lock/open/insert sequence itself belongs to the persistent runtime.
    // Cancelling a caller must not detach an in-flight open from its owner and
    // allow the next caller to race another open against the same file lock.
    runtime
        .spawn(async move { connect(engine, &path, create).await })
        .await
        .map_err(|error| database_error("embedded_connect_worker", error))?
}

/// Render a canonical store directory as a `scheme://` address component.
///
/// The SDK splits an address on its first `://`, then treats the remainder as a
/// filesystem path. Two properties of Windows paths have to survive that split.
/// `std::fs::canonicalize` returns a verbatim `\\?\C:\...` prefix, which must be
/// preserved so long paths keep working; and a path that itself contains `://`
/// would move the split point and silently retarget the store. Reject the
/// latter instead of connecting somewhere unintended, and keep the platform's
/// own rendering of the prefix otherwise.
fn endpoint_path(path: &std::path::Path) -> Result<String, ProjectionError> {
    let rendered = path.to_str().ok_or_else(|| {
        ProjectionError::InvalidReference("embedded storage path must be UTF-8".into())
    })?;
    if rendered.contains("://") {
        return Err(ProjectionError::InvalidReference(
            "embedded storage path must not contain \"://\"".into(),
        ));
    }
    Ok(rendered.to_owned())
}

async fn connect(
    engine: SurrealEngine,
    path: &str,
    create: bool,
) -> Result<Surreal<Any>, ProjectionError> {
    if create {
        std::fs::create_dir_all(path)
            .map_err(|error| database_error("create_store_directory", error))?;
    }
    let path = std::fs::canonicalize(path)
        .map_err(|error| database_error("resolve_store_directory", error))?;
    let mut connections = CONNECTIONS.lock().await;
    if let Some((opened_engine, connection)) = connections.get(&path) {
        if *opened_engine != engine {
            return Err(ProjectionError::InvalidReference(
                "store location is already open with a different embedded engine".to_owned(),
            ));
        }
        return Ok(connection.clone());
    }
    if connections.len() >= MAX_STORES {
        return Err(ProjectionError::LimitExceeded {
            resource: "process-wide embedded store connections (restart to release stores)",
            actual: (connections.len() + 1) as u64,
            limit: MAX_STORES as u64,
        });
    }
    let store_path = endpoint_path(&path)?;
    let connection = match engine {
        #[cfg(feature = "surrealkv")]
        SurrealEngine::SurrealKv => {
            surrealdb::engine::any::connect(format!("surrealkv://{store_path}"))
                .with_capacity(ROUTER_CAPACITY)
                .await
                .map_err(|error| database_error("connect_surrealkv", error))
        }
        #[cfg(feature = "rocksdb")]
        SurrealEngine::RocksDb => {
            surrealdb::engine::any::connect(format!("rocksdb://{store_path}"))
                .with_capacity(ROUTER_CAPACITY)
                .await
                .map_err(|error| database_error("connect_rocksdb", error))
        }
        #[allow(unreachable_patterns)]
        _ => Err(ProjectionError::InvalidReference(
            "requested embedded engine was not compiled in".to_owned(),
        )),
    }?;
    let session = connection.clone();
    connections.insert(path, (engine, connection));
    Ok(session)
}
