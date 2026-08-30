//! Explicit, process-owned connection configuration. References never authorize
//! credential use: they must match the independently configured target.

use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[derive(Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurrealSettings {
    pub store: Option<String>,
    pub engine: Option<String>,
    pub path: Option<PathBuf>,
    pub endpoint: Option<String>,
    pub namespace: Option<String>,
    pub database: Option<String>,
    pub auth_level: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub token: Option<String>,
    pub password_env: Option<String>,
    pub token_env: Option<String>,
}

impl std::fmt::Debug for SurrealSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SurrealSettings")
            .field("engine", &self.engine)
            .field("credentials", &"<redacted>")
            .finish_non_exhaustive()
    }
}

static SETTINGS: OnceLock<SurrealSettings> = OnceLock::new();

pub fn configure_surreal(settings: SurrealSettings) -> Result<(), String> {
    let settings = settings.validated()?;
    if let Some(existing) = SETTINGS.get() {
        return if existing == &settings {
            Ok(())
        } else {
            Err("Surreal configuration is immutable within a process; start a new process to change it".into())
        };
    }
    SETTINGS
        .set(settings)
        .map_err(|_| "Surreal configuration was concurrently initialized".into())
}

pub fn surreal_settings() -> Result<&'static SurrealSettings, String> {
    SETTINGS.get().ok_or_else(|| "configure the Surreal connection with --surreal-config, COMPASS_SURREAL_ENDPOINT, or --surreal-endpoint before using a server reference".into())
}

impl SurrealSettings {
    pub fn read_yaml(path: &Path) -> Result<Self, String> {
        let bytes =
            crate::read_bytes_bounded(path, 64 * 1024).map_err(|error| error.to_string())?;
        // Parser diagnostics can contain source lines, including credentials.
        serde_yaml_ng::from_slice(&bytes).map_err(|_| "invalid Surreal YAML configuration (unknown, duplicate, or malformed fields); contents redacted".into())
    }

    pub fn overlay(&mut self, newer: Self) {
        if newer.endpoint.is_some() && newer.engine.is_none() {
            self.engine = Some("remote".into());
            self.path = None;
            if newer.store.is_none() {
                self.store = Some("surreal".into());
            }
        }
        if newer.engine.as_deref() == Some("remote") {
            self.path = None;
        }
        macro_rules! field { ($($name:ident),*) => { $(if newer.$name.is_some() { self.$name = newer.$name; })* }; }
        field!(
            store,
            engine,
            path,
            endpoint,
            namespace,
            database,
            auth_level,
            username,
            password,
            token,
            password_env,
            token_env
        );
    }

    pub fn from_environment() -> Result<Self, String> {
        fn value(name: &str) -> Result<Option<String>, String> {
            match std::env::var(name) {
                Ok(value) => Ok(Some(value)),
                Err(std::env::VarError::NotPresent) => Ok(None),
                Err(_) => Err(format!("{name} must be valid UTF-8")),
            }
        }
        Ok(Self {
            store: value("COMPASS_STORE")?,
            engine: value("COMPASS_SURREAL_ENGINE")?,
            path: value("COMPASS_SURREAL_PATH")?.map(PathBuf::from),
            endpoint: value("COMPASS_SURREAL_ENDPOINT")?,
            namespace: value("COMPASS_SURREAL_NAMESPACE")?,
            database: value("COMPASS_SURREAL_DATABASE")?,
            auth_level: value("COMPASS_SURREAL_AUTH_LEVEL")?,
            username: value("COMPASS_SURREAL_USERNAME")?,
            password: value("COMPASS_SURREAL_PASSWORD")?,
            token: value("COMPASS_SURREAL_TOKEN")?,
            password_env: value("COMPASS_SURREAL_PASSWORD_ENV")?,
            token_env: value("COMPASS_SURREAL_TOKEN_ENV")?,
        })
    }

    pub fn validated(mut self) -> Result<Self, String> {
        if matches!(self.store.as_deref(), Some("json" | "sqlite")) {
            self.engine = None;
            self.path = None;
            self.endpoint = None;
            self.namespace = None;
            self.database = None;
            self.username = None;
            self.password = None;
            self.token = None;
        }
        if self.endpoint.is_some() && self.engine.is_none() {
            self.engine = Some("remote".into());
        }
        if self.engine.is_some() && self.store.is_none() {
            self.store = Some("surreal".into());
        }
        if let Some(store) = &self.store {
            if !matches!(store.as_str(), "json" | "sqlite" | "surreal") {
                return Err("Surreal settings store must be json, sqlite, or surreal".into());
            }
        }
        if let Some(engine) = &self.engine {
            if !matches!(engine.as_str(), "surrealkv" | "rocksdb" | "remote") {
                return Err("Surreal engine must be surrealkv, rocksdb, or remote".into());
            }
        }
        if self.engine.as_deref() == Some("remote") {
            if self.path.is_some() {
                return Err("remote SurrealDB uses an endpoint, not --surreal-path".into());
            }
            self.endpoint = Some(normalize_surreal_endpoint(self.endpoint.as_deref().ok_or(
                "remote SurrealDB requires --surreal-endpoint, COMPASS_SURREAL_ENDPOINT, or a YAML endpoint")?)?);
            self.namespace.get_or_insert_with(|| "compass".into());
            self.database.get_or_insert_with(|| "graph".into());
            for value in [&self.namespace, &self.database].into_iter().flatten() {
                if value.is_empty() || value.len() > 128 || value.chars().any(char::is_control) {
                    return Err("Surreal namespace/database must contain 1..128 bytes without control characters".into());
                }
            }
            if !matches!(
                self.auth_level.as_deref().unwrap_or("root"),
                "root" | "namespace" | "database"
            ) {
                return Err("Surreal auth_level must be root, namespace, or database".into());
            }
            for (name, target) in [
                (&self.password_env, &mut self.password),
                (&self.token_env, &mut self.token),
            ] {
                if target.is_none() {
                    if let Some(name) = name {
                        *target = Some(std::env::var(name).map_err(|_| "configured Surreal credential environment variable is missing or not UTF-8")?);
                    }
                }
            }
            if self.token.is_some() && (self.username.is_some() || self.password.is_some()) {
                return Err(
                    "Surreal token and username/password authentication are mutually exclusive"
                        .into(),
                );
            }
            if self.username.is_some() != self.password.is_some() {
                return Err("Surreal username and password must be supplied together".into());
            }
            if [&self.username, &self.password, &self.token]
                .into_iter()
                .flatten()
                .any(|value| value.is_empty() || value.len() > 16 * 1024)
            {
                return Err("Surreal credentials must contain 1..16384 bytes".into());
            }
        }
        Ok(self)
    }

    pub fn authorize_target(
        &self,
        endpoint: &str,
        namespace: &str,
        database: &str,
    ) -> Result<(), String> {
        if self.engine.as_deref() != Some("remote")
            || self.endpoint.as_deref() != Some(normalize_surreal_endpoint(endpoint)?.as_str())
            || self.namespace.as_deref() != Some(namespace)
            || self.database.as_deref() != Some(database)
        {
            return Err("Surreal reference target does not match the explicitly configured endpoint/namespace/database; refusing connection or credential forwarding".into());
        }
        Ok(())
    }
}

pub fn normalize_surreal_endpoint(value: &str) -> Result<String, String> {
    if value.len() > 2048 {
        return Err("Surreal endpoint exceeds 2048 bytes".into());
    }
    let mut url =
        url::Url::parse(value).map_err(|_| "invalid Surreal endpoint URL (contents redacted)")?;
    let scheme = match url.scheme() {
        "ws" | "http" => "ws",
        "wss" | "https" => "wss",
        _ => return Err("Surreal endpoint must use ws, wss, http, or https".into()),
    };
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(url.path(), "" | "/" | "/rpc")
    {
        return Err("Surreal endpoint must not contain credentials, query, fragment, or a path other than /rpc".into());
    }
    let loopback = match url.host() {
        Some(url::Host::Domain("localhost")) => true,
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        Some(_) => false,
        None => return Err("Surreal endpoint requires a host".into()),
    };
    if scheme == "ws" && !loopback {
        return Err("non-loopback SurrealDB requires TLS (wss or https)".into());
    }
    url.set_scheme(scheme)
        .map_err(|_| "invalid Surreal endpoint scheme")?;
    url.set_path("");
    Ok(url.as_str().trim_end_matches('/').to_owned())
}
