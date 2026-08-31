# Both embedded and standalone SurrealDB

The operator corrects the earlier embedded-only scope: support the existing
launch-agent SurrealDB server as well as embedded SurrealKV/RocksDB. Configure
the connection through YAML, environment variables, or command flags, with
flags taking precedence over environment, YAML, persisted project settings,
and defaults. Never copy server database files or change its launch agent.

Implement the entire connection/configuration/publication/query/MCP/operations
slice first. Preserve immutable generation pinning and portable backups. Keep
credentials out of references, project settings, logs, and backup bundles.
Bound network connection and operation time and response sizes; require TLS
for non-loopback servers. A published reference cannot redirect configured
credentials to another endpoint or namespace/database.

After implementation, build/install the release with embedded and remote
features and commit/push first, then verify only changed public integration
surfaces against embedded storage and a disposable database on the approved
local server. Broad phase gates and independent certification remain deferred
under the operator's urgent-deployment instruction.
