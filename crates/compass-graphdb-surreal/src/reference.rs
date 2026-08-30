use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{PROJECTION_SCHEMA_V2, ProjectionError, ProjectionPlan};

pub const SURREAL_REF_FILE_NAME: &str = "surreal.ref";
pub const SURREAL_REF_SCHEMA_V1: &str = "compass.surreal.ref/1";
pub const SURREAL_BUNDLE_SCHEMA_V1: &str = "compass.surreal.bundle/1";
pub const SURREAL_NAMESPACE: &str = "compass";
pub const SURREAL_DATABASE: &str = "graph";
pub const MAX_SURREAL_REF_BYTES: u64 = 32 * 1024;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SurrealEngine {
    SurrealKv,
    RocksDb,
}

impl SurrealEngine {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SurrealKv => "surrealkv",
            Self::RocksDb => "rocksdb",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SurrealRef {
    pub schema: String,
    pub projection_schema: String,
    pub engine: SurrealEngine,
    pub repository_id: String,
    pub generation_id: String,
    pub graph_digest: String,
    pub projection_fingerprint: String,
    pub node_count: u64,
    pub relation_count: u64,
    pub file_count: u64,
    pub location: String,
    pub namespace: String,
    pub database: String,
}

impl SurrealRef {
    pub fn from_plan(
        plan: &ProjectionPlan,
        engine: SurrealEngine,
        location: &Path,
        graph_digest: impl Into<String>,
    ) -> Result<Self, ProjectionError> {
        let location = location.to_str().ok_or_else(|| {
            ProjectionError::InvalidReference(
                "SurrealDB connection location must be valid UTF-8".to_owned(),
            )
        })?;
        let reference = Self {
            schema: SURREAL_REF_SCHEMA_V1.to_owned(),
            projection_schema: PROJECTION_SCHEMA_V2.to_owned(),
            engine,
            repository_id: plan.repository_id.clone(),
            generation_id: plan.generation_id.clone(),
            graph_digest: graph_digest.into(),
            projection_fingerprint: plan.projection_fingerprint.clone(),
            node_count: u64::try_from(plan.nodes.len()).unwrap_or(u64::MAX),
            relation_count: u64::try_from(plan.relations.len()).unwrap_or(u64::MAX),
            file_count: u64::try_from(plan.files.len()).unwrap_or(u64::MAX),
            location: location.to_owned(),
            namespace: SURREAL_NAMESPACE.to_owned(),
            database: SURREAL_DATABASE.to_owned(),
        };
        reference.validate()?;
        Ok(reference)
    }

    pub fn validate(&self) -> Result<(), ProjectionError> {
        if self.schema != SURREAL_REF_SCHEMA_V1 {
            return Err(ProjectionError::InvalidReference(format!(
                "expected reference schema {SURREAL_REF_SCHEMA_V1}, found {}",
                self.schema
            )));
        }
        if self.projection_schema != PROJECTION_SCHEMA_V2 {
            return Err(ProjectionError::UnsupportedProjectionSchema(
                self.projection_schema.clone(),
            ));
        }
        for (name, value) in [
            ("repository identity", self.repository_id.as_str()),
            ("generation identity", self.generation_id.as_str()),
            (
                "projection fingerprint",
                self.projection_fingerprint.as_str(),
            ),
            ("connection location", self.location.as_str()),
            ("namespace", self.namespace.as_str()),
            ("database", self.database.as_str()),
        ] {
            if value.trim().is_empty() {
                return Err(ProjectionError::InvalidReference(format!(
                    "{name} must not be empty"
                )));
            }
        }
        if !valid_sha256_digest(&self.graph_digest)
            || !valid_sha256_digest(&self.projection_fingerprint)
        {
            return Err(ProjectionError::InvalidReference(
                "graph digest and projection fingerprint must be sha256:<64 lowercase hexadecimal> values".to_owned(),
            ));
        }
        Ok(())
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, ProjectionError> {
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_SURREAL_REF_BYTES {
            return Err(ProjectionError::LimitExceeded {
                resource: "surreal reference bytes",
                actual: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
                limit: MAX_SURREAL_REF_BYTES,
            });
        }
        let reference: Self = serde_json::from_slice(bytes)?;
        reference.validate()?;
        Ok(reference)
    }

    pub fn encode(&self) -> Result<Vec<u8>, ProjectionError> {
        self.validate()?;
        let mut bytes = serde_json::to_vec_pretty(self)?;
        bytes.push(b'\n');
        Ok(bytes)
    }

    pub(crate) fn validate_plan(&self, plan: &ProjectionPlan) -> Result<(), ProjectionError> {
        self.validate()?;
        if self.repository_id != plan.repository_id
            || self.generation_id != plan.generation_id
            || self.projection_fingerprint != plan.projection_fingerprint
            || self.node_count != u64::try_from(plan.nodes.len()).unwrap_or(u64::MAX)
            || self.file_count != u64::try_from(plan.files.len()).unwrap_or(u64::MAX)
            || self.relation_count != u64::try_from(plan.relations.len()).unwrap_or(u64::MAX)
        {
            return Err(ProjectionError::InvalidReference(
                "reference does not describe the projection plan".to_owned(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectionBundle {
    pub schema: String,
    pub reference: SurrealRef,
    pub plan: ProjectionPlan,
    pub digest: String,
}

impl ProjectionBundle {
    pub fn new(reference: SurrealRef, plan: ProjectionPlan) -> Result<Self, ProjectionError> {
        let mut bundle = Self {
            schema: SURREAL_BUNDLE_SCHEMA_V1.to_owned(),
            reference,
            plan,
            digest: String::new(),
        };
        bundle.digest = bundle.expected_digest()?;
        bundle.validate()?;
        Ok(bundle)
    }

    pub fn validate(&self) -> Result<(), ProjectionError> {
        if self.schema != SURREAL_BUNDLE_SCHEMA_V1 {
            return Err(ProjectionError::InvalidBundle(format!(
                "expected bundle schema {SURREAL_BUNDLE_SCHEMA_V1}, found {}",
                self.schema
            )));
        }
        self.plan.validate()?;
        self.reference
            .validate_plan(&self.plan)
            .map_err(|error| ProjectionError::InvalidBundle(error.to_string()))?;
        if self.digest != self.expected_digest()? {
            return Err(ProjectionError::InvalidBundle(
                "bundle digest does not match its reference and projection".to_owned(),
            ));
        }
        Ok(())
    }

    fn expected_digest(&self) -> Result<String, ProjectionError> {
        let payload = serde_json::to_vec(&(&self.schema, &self.reference, &self.plan))?;
        Ok(format!("sha256:{:x}", Sha256::digest(payload)))
    }
}

fn valid_sha256_digest(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}
