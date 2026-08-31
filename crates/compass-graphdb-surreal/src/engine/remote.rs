//! Process-lived, target-authorized remote connections, never database file access.
use super::database_error;
use crate::{ProjectionError, SurrealRef};
use std::collections::BTreeMap;
use std::sync::LazyLock;
use std::time::Duration;
use surrealdb::opt::{
    Config, WebsocketConfig,
    auth::{Database, Namespace, Root},
};
use surrealdb::{Surreal, engine::any::Any};
use tokio::runtime::{Builder, Runtime};
use tokio::sync::Mutex;

type Key = (String, String, String);
static CLIENTS: LazyLock<Mutex<BTreeMap<Key, Surreal<Any>>>> = LazyLock::new(Mutex::default);
static RUNTIME: LazyLock<Result<Runtime, std::io::Error>> = LazyLock::new(|| {
    Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(4)
        .thread_name("compass-surreal-remote")
        .enable_all()
        .build()
});

pub(super) async fn client(reference: &SurrealRef) -> Result<Surreal<Any>, ProjectionError> {
    // Other workspace TLS clients may enable a second provider. Select one
    // explicitly before the SDK constructs a WSS client, avoiding rustls's
    // ambiguous-provider panic. Respect an already-installed provider.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let settings = compass_files::surreal_settings().map_err(ProjectionError::InvalidReference)?;
    settings
        .authorize_target(
            &reference.location,
            &reference.namespace,
            &reference.database,
        )
        .map_err(ProjectionError::InvalidReference)?;
    let key = (
        reference.location.clone(),
        reference.namespace.clone(),
        reference.database.clone(),
    );
    let runtime = RUNTIME
        .as_ref()
        .map_err(|error| database_error("remote_runtime", error))?;
    runtime
        .spawn(async move {
            tokio::time::timeout(Duration::from_secs(30), async {
                let mut clients = CLIENTS.lock().await;
                if let Some(client) = clients.get(&key) {
                    return Ok(client.clone());
                }
                if clients.len() >= 16 {
                    return Err(ProjectionError::LimitExceeded {
                        resource: "remote connections",
                        actual: 17,
                        limit: 16,
                    });
                }
                let config = Config::new()
                    .websocket(WebsocketConfig::new().max_message_size(128 << 20))
                    .map_err(|error| database_error("remote_config", error))?;
                let client = surrealdb::engine::any::connect((key.0.as_str(), config))
                    .with_capacity(256)
                    .await
                    .map_err(|_| {
                        database_error(
                            "remote_connect",
                            "connection failed; endpoint details and credentials redacted",
                        )
                    })?;
                // Authentication errors are deliberately sanitized. The SDK/server
                // may include input data in its diagnostic payload.
                if let Some(token) = &settings.token {
                    client
                        .authenticate(token.clone())
                        .await
                        .map_err(|_| database_error("remote_authenticate", "token rejected"))?;
                } else if let (Some(username), Some(password)) =
                    (&settings.username, &settings.password)
                {
                    let result = match settings.auth_level.as_deref().unwrap_or("root") {
                        "namespace" => {
                            client
                                .signin(Namespace {
                                    namespace: key.1.clone(),
                                    username: username.clone(),
                                    password: password.clone(),
                                })
                                .await
                        }
                        "database" => {
                            client
                                .signin(Database {
                                    namespace: key.1.clone(),
                                    database: key.2.clone(),
                                    username: username.clone(),
                                    password: password.clone(),
                                })
                                .await
                        }
                        _ => {
                            client
                                .signin(Root {
                                    username: username.clone(),
                                    password: password.clone(),
                                })
                                .await
                        }
                    };
                    result.map_err(|_| {
                        database_error("remote_signin", "username/password rejected")
                    })?;
                }
                client.use_ns(&key.1).use_db(&key.2).await.map_err(|_| {
                    database_error("remote_select", "namespace/database selection failed")
                })?;
                clients.insert(key, client.clone());
                Ok(client)
            })
            .await
            .map_err(|_| {
                database_error(
                    "remote_connect",
                    "connection/authentication exceeded 30 seconds",
                )
            })?
        })
        .await
        .map_err(|error| database_error("remote_worker", error))?
}
