use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{BuildScope, FileError, write_text_atomic};

pub const PROJECT_CONFIG_RELATIVE_PATH: &str = ".compass/config.toml";
pub const PROJECT_CONFIG_VERSION: u32 = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectStore {
    Json,
    Sqlite,
    Surreal,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum ProjectSurrealEngine {
    #[default]
    #[serde(rename = "surrealkv")]
    SurrealKv,
    #[serde(rename = "rocksdb")]
    RocksDb,
    #[serde(rename = "remote")]
    Remote,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectStorage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub store: Option<ProjectStore>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surreal_engine: Option<ProjectSurrealEngine>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surreal_path: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surreal_endpoint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surreal_namespace: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surreal_database: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectConfig {
    pub version: u32,
    #[serde(default)]
    pub build: BuildScope,
    #[serde(default, skip_serializing_if = "ProjectStorage::is_empty")]
    pub storage: ProjectStorage,
}

impl ProjectConfig {
    #[must_use]
    pub fn new(build: BuildScope) -> Self {
        Self {
            version: PROJECT_CONFIG_VERSION,
            build,
            storage: ProjectStorage::default(),
        }
    }

    pub fn normalize(mut self, root: &Path) -> Result<Self, FileError> {
        if !matches!(self.version, 1 | PROJECT_CONFIG_VERSION) {
            return Err(FileError::UnsupportedProjectConfig {
                path: root.join(PROJECT_CONFIG_RELATIVE_PATH),
                version: self.version,
            });
        }
        if self.version == 1 && !self.storage.is_empty() {
            return Err(FileError::InvalidProjectConfig {
                path: root.join(PROJECT_CONFIG_RELATIVE_PATH),
                reason: "version 1 project configuration cannot contain storage settings"
                    .to_owned(),
            });
        }
        self.build = self.build.normalize(root)?;
        self.storage.normalize(root)?;
        Ok(self)
    }

    pub fn load(root: &Path) -> Result<Option<Self>, FileError> {
        let path = checked_config_path(root)?;
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(source) => return Err(FileError::Io { path, source }),
        };
        let config: Self =
            toml::from_str(&text).map_err(|source| FileError::ProjectConfigToml {
                path: path.clone(),
                source: Box::new(source),
            })?;
        config.normalize(root).map(Some)
    }

    pub fn write(&self, root: &Path) -> Result<PathBuf, FileError> {
        let config = self.clone().normalize(root)?;
        let path = checked_config_path(root)?;
        let text = toml::to_string(&config).map_err(|source| FileError::ProjectConfigEncode {
            path: path.clone(),
            source: Box::new(source),
        })?;
        write_text_atomic(&path, &text)?;
        Ok(path)
    }
}

impl ProjectStorage {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.store.is_none()
            && self.surreal_engine.is_none()
            && self.surreal_path.is_none()
            && self.surreal_endpoint.is_none()
            && self.surreal_namespace.is_none()
            && self.surreal_database.is_none()
    }

    fn normalize(&mut self, root: &Path) -> Result<(), FileError> {
        if self.surreal_engine.is_some()
            || self.surreal_path.is_some()
            || self.surreal_endpoint.is_some()
            || self.surreal_namespace.is_some()
            || self.surreal_database.is_some()
        {
            match self.store {
                Some(ProjectStore::Surreal) => {}
                _ => {
                    return Err(FileError::InvalidProjectConfig {
                        path: root.join(PROJECT_CONFIG_RELATIVE_PATH),
                        reason:
                            "surreal_engine and surreal_path require storage.store = \"surreal\""
                                .to_owned(),
                    });
                }
            }
        }
        if let Some(endpoint) = &self.surreal_endpoint {
            if self.surreal_engine != Some(ProjectSurrealEngine::Remote)
                || self.surreal_path.is_some()
            {
                return Err(FileError::InvalidProjectConfig {
                    path: root.join(PROJECT_CONFIG_RELATIVE_PATH),
                    reason: "surreal_endpoint requires remote engine and excludes surreal_path"
                        .into(),
                });
            }
            self.surreal_endpoint = Some(crate::normalize_surreal_endpoint(endpoint).map_err(
                |reason| FileError::InvalidProjectConfig {
                    path: root.join(PROJECT_CONFIG_RELATIVE_PATH),
                    reason,
                },
            )?);
        }
        if let Some(path) = &self.surreal_path {
            if path.as_os_str().is_empty() {
                return Err(FileError::InvalidProjectConfig {
                    path: root.join(PROJECT_CONFIG_RELATIVE_PATH),
                    reason: "storage.surreal_path must not be empty".to_owned(),
                });
            }
            if path
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir))
            {
                return Err(FileError::InvalidProjectConfig {
                    path: root.join(PROJECT_CONFIG_RELATIVE_PATH),
                    reason: "storage.surreal_path must not contain '..' components".to_owned(),
                });
            }
        }
        Ok(())
    }
}

fn checked_config_path(root: &Path) -> Result<PathBuf, FileError> {
    let root = fs::canonicalize(root).map_err(|source| crate::io_error(root, source))?;
    let path = root.join(PROJECT_CONFIG_RELATIVE_PATH);
    if let Some(parent) = path.parent() {
        ensure_existing_path_is_inside(&root, parent)?;
    }
    ensure_existing_path_is_inside(&root, &path)?;
    Ok(path)
}

fn ensure_existing_path_is_inside(root: &Path, path: &Path) -> Result<(), FileError> {
    match fs::symlink_metadata(path) {
        Ok(_) => {
            let resolved =
                fs::canonicalize(path).map_err(|source| crate::io_error(path, source))?;
            if !resolved.starts_with(root) {
                return Err(FileError::ProjectConfigOutsideRoot {
                    path: path.to_path_buf(),
                    root: root.to_path_buf(),
                });
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => return Err(crate::io_error(path, source)),
    }
    Ok(())
}
