//! Release compatibility is checked before graph records or cached indexes load.
use std::fs::{self, File};
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::{Component, Path};

use serde::Deserializer;
use serde::de::{DeserializeSeed, Error, IgnoredAny, MapAccess, Visitor};

use crate::GraphError;

pub const MIN_QUERY_BUILDER_VERSION: &str = "0.3.23";
pub const MAX_GRAPH_PREAMBLE_BYTES: u64 = 64 * 1024;
const MAX_SOURCE_ROOT_BYTES: u64 = 16 * 1024;

/// Check recognized release versions. Non-release producer labels retain their
/// existing behavior; this is a release compatibility boundary, not authenticity.
pub fn validate_query_builder_version(
    found: &str,
    graph_path: Option<&Path>,
) -> Result<(), GraphError> {
    let release = found.strip_prefix('v').unwrap_or(found);
    let release = release.split('+').next().unwrap_or(release);
    let (numbers, prerelease) = release
        .split_once('-')
        .map_or((release, false), |(v, _)| (v, true));
    let mut parts = numbers.split('.');
    let version = match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(major), Some(minor), Some(patch), None) => {
            match (
                major.parse::<u64>(),
                minor.parse::<u64>(),
                patch.parse::<u64>(),
            ) {
                (Ok(major), Ok(minor), Ok(patch)) => (major, minor, patch),
                _ => return Ok(()),
            }
        }
        _ => return Ok(()),
    };
    if version < (0, 3, 23) || (version == (0, 3, 23) && prerelease) {
        return Err(GraphError::LegacyArtifact {
            found: found
                .chars()
                .take(128)
                .flat_map(char::escape_default)
                .collect(),
            minimum: MIN_QUERY_BUILDER_VERSION,
            recovery: rebuild_instruction(graph_path),
        });
    }
    Ok(())
}

/// Inspect only a bounded JSON prefix. The visitor deliberately stops as soon
/// as graph.build.builderVersion is found; it never decodes node/edge records.
pub fn validate_graph_preamble<R: Read>(reader: R, path: &Path) -> Result<(), GraphError> {
    let mut found = None;
    let mut limited = reader.take(MAX_GRAPH_PREAMBLE_BYTES);
    let result = HeaderSeed {
        keys: &["graph", "build", "builderVersion"],
        found: &mut found,
    }
    .deserialize(&mut serde_json::Deserializer::from_reader(&mut limited));
    if let Some(version) = found {
        return validate_query_builder_version(&version, Some(path));
    }
    if limited.limit() == 0 {
        return Err(GraphError::GraphPreambleLimit {
            limit: MAX_GRAPH_PREAMBLE_BYTES,
            recovery: rebuild_instruction(Some(path)),
        });
    }
    result.map_err(GraphError::Corrupt)
}

pub(crate) fn validate_opened_graph_preamble(
    file: &mut File,
    path: &Path,
) -> Result<(), GraphError> {
    // Put the limiter below buffering so read-ahead also remains bounded.
    let result = validate_graph_preamble(
        BufReader::new((&mut *file).take(MAX_GRAPH_PREAMBLE_BYTES)),
        path,
    );
    file.seek(SeekFrom::Start(0))
        .map_err(|source| GraphError::Read {
            path: crate::graph::absolute_path(path),
            source,
        })?;
    result
}

pub(crate) fn validate_graph_path_preamble(path: &Path) -> Result<(), GraphError> {
    let mut file = File::open(path).map_err(|source| {
        let path = crate::graph::absolute_path(path);
        if source.kind() == std::io::ErrorKind::NotFound {
            GraphError::NotFound(path)
        } else {
            GraphError::Read { path, source }
        }
    })?;
    validate_opened_graph_preamble(&mut file, path)
}

struct HeaderSeed<'a> {
    keys: &'a [&'a str],
    found: &'a mut Option<String>,
}

impl<'de> DeserializeSeed<'de> for HeaderSeed<'_> {
    type Value = ();
    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_map(self)
    }
}

impl<'de> Visitor<'de> for HeaderSeed<'_> {
    type Value = ();
    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("a graph metadata object")
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        while let Some(key) = map.next_key::<String>()? {
            if Some(key.as_str()) == self.keys.first().copied() {
                if self.keys.len() == 1 {
                    *self.found = Some(map.next_value::<String>()?);
                    // A deliberate short circuit, distinguished by `found`, not
                    // by matching a parser's human-readable error message.
                    return Err(A::Error::custom("graph builder preamble complete"));
                }
                map.next_value_seed(HeaderSeed {
                    keys: &self.keys[1..],
                    found: self.found,
                })?;
            } else {
                map.next_value::<IgnoredAny>()?;
            }
        }
        Ok(())
    }
}

fn rebuild_instruction(graph_path: Option<&Path>) -> String {
    if let Some(root) = graph_path.and_then(validated_source_root) {
        return format!("Rebuild with: compass update \"{root}\" --force");
    }
    "Snapshot source-root.txt provenance is absent or invalid; supply the project root explicitly: compass update \"<source-root>\" --force".into()
}

fn validated_source_root(graph_path: &Path) -> Option<String> {
    // Use only the selected snapshot's sibling provenance. Never guess from
    // cwd, ancestor projects, graph payload paths, or another active snapshot.
    let snapshot = graph_path.parent()?;
    let path = snapshot.join("source-root.txt");
    let metadata = fs::symlink_metadata(&path).ok()?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() > MAX_SOURCE_ROOT_BYTES
    {
        return None;
    }
    let mut raw = String::new();
    let file = File::open(&path).ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    file.take(MAX_SOURCE_ROOT_BYTES + 1)
        .read_to_string(&mut raw)
        .ok()?;
    if raw.len() as u64 > MAX_SOURCE_ROOT_BYTES {
        return None;
    }
    let raw = raw
        .strip_suffix("\r\n")
        .or_else(|| raw.strip_suffix('\n'))
        .unwrap_or(&raw);
    if raw.is_empty() || raw.chars().any(char::is_control) {
        return None;
    }
    let root = Path::new(raw);
    if !root.is_absolute() || root.components().any(|c| matches!(c, Component::ParentDir)) {
        return None;
    }
    let canonical = fs::canonicalize(root).ok()?;
    if !canonical.is_dir() {
        return None;
    }
    let root = canonical.to_str()?;
    if root.chars().any(char::is_control) {
        return None;
    }
    #[cfg(windows)]
    {
        // Do not emit a command that cmd.exe would expand or reinterpret.
        if root.contains(['"', '%', '!', '^', '&', '|', '<', '>']) {
            return None;
        }
        Some(root.to_owned())
    }
    #[cfg(not(windows))]
    {
        Some(
            root.replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('$', "\\$")
                .replace('`', "\\`"),
        )
    }
}
