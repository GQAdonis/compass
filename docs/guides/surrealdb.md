# Embedded and standalone SurrealDB

Compass can publish and query an immutable graph projection in an embedded
SurrealDB database or through a standalone server, including a local launch agent.
This support is feature-gated: ordinary builds remain free
of SurrealDB and continue to use SQLite plus portable `graph.json`.

## Build and install

SurrealKV is the production feature. RocksDB is optional and is intended for
qualification or deployments that deliberately accept its larger native build.

```bash
cargo install --locked --path crates/compass-cli --bin compass \
  --features surreal-surrealkv,surreal-remote

# Optional RocksDB support as well:
cargo install --locked --path crates/compass-cli --bin compass \
  --features surreal-surrealkv,surreal-rocksdb
```

These features link the pinned SurrealDB 3.2.4 components covered by
`THIRD_PARTY_NOTICES.md`. `surreal-remote` enables the server protocol and TLS;
it can be combined with either embedded feature in one executable.

## Standalone server: YAML, environment, or flags

Do not use `--surreal-path` for a running server's RocksDB/SurrealKV files.
Connect through its endpoint instead. For example, save a connection YAML:

```yaml
store: surreal
engine: remote
endpoint: ws://127.0.0.1:28000
namespace: compass
database: graph
auth_level: database
username: compass
password_env: COMPASS_DATABASE_PASSWORD
```

Use a dedicated Compass database/user provisioned by the server administrator.
For a root-level account, select `auth_level: root` instead. Token authentication
uses `token_env` instead of username/password. Credentials may also be supplied
as `password`/`token` YAML fields in an operator-protected file; never commit it.

```bash
compass init . --yes --surreal-config connection.yaml
compass update . --surreal-config connection.yaml
compass search caller --engine surreal --surreal-config connection.yaml
compass query --cql 'MATCH (n:Function) RETURN n.name LIMIT 10' \
  --engine surreal --surreal-config connection.yaml
compass serve --engine surreal --surreal-config connection.yaml
```

The automatic user file is `~/.compass/surreal.yaml`. Select another file with
`COMPASS_SURREAL_CONFIG` or `--surreal-config`. Connection YAML has flat fields
`store`, `engine`, `path`, `endpoint`, `namespace`, `database`, `auth_level`,
`username`, `password`, `token`, `password_env`, and `token_env`. Files are
bounded to 64 KiB; unknown fields and malformed YAML fail without printing
their contents.

Equivalent environment settings are `COMPASS_STORE`, `COMPASS_SURREAL_ENGINE`,
`COMPASS_SURREAL_PATH`, `COMPASS_SURREAL_ENDPOINT`,
`COMPASS_SURREAL_NAMESPACE`, `COMPASS_SURREAL_DATABASE`,
`COMPASS_SURREAL_AUTH_LEVEL`, `COMPASS_SURREAL_USERNAME`,
`COMPASS_SURREAL_PASSWORD`, `COMPASS_SURREAL_TOKEN`,
`COMPASS_SURREAL_PASSWORD_ENV`, and `COMPASS_SURREAL_TOKEN_ENV`.
Each connection field also has a `--surreal-<hyphenated-name>` flag, except
`store`, which uses `--store`. Prefer environment credentials to command-line
password/token values, which are visible in process listings.

Precedence is flags, environment, YAML, persisted `.compass/config.toml`
storage settings, then defaults. `init` persists only non-secret storage and
target fields. Explicit endpoint selection implies remote mode unless the same
source explicitly selects another engine. To switch back to embedded mode:

```bash
compass update . --store surreal --surreal-engine surrealkv
```

A higher-priority `password_env` or `token_env` replaces a lower-priority direct
credential, and a higher-priority direct credential replaces its lower-priority
selector. Missing selected environment variables fail closed; they do not fall
back to an older credential. `--store json` or `--store sqlite` disables inherited
Surreal storage settings, but combining either flag with an explicit Surreal
engine, path, or endpoint is an error.

Server defaults are namespace `compass`, database `graph`, root auth scope;
the endpoint is required. WS/WSS are supported; HTTP/HTTPS spellings normalize
to their WebSocket equivalents. Only loopback hosts permit plaintext. External
hosts require validated TLS. Reference targets must exactly match independently
configured endpoint/namespace/database before connecting or sending credentials.
Queries never retarget a snapshot or silently fall back. Restart MCP/Compass
when changing credentials: connection configuration is immutable per process.
Library embedders call `compass_files::configure_surreal` with validated settings
before using `SurrealProjection::open_reference` or the native query/MCP APIs.

## Configure and publish

Use the same storage flags with `init`, `update`, `extract`, and `watch`:

```bash
compass init . --yes --store surreal
compass update . --store surreal --surreal-engine surrealkv
compass watch . --store surreal --surreal-path compass-out/surreal
```

`--surreal-engine` accepts `surrealkv`, `rocksdb`, or `remote`; SurrealKV is the embedded default.
`--surreal-path` defaults to the checkout-local shared store at
`compass-out/surreal`. Explicit flags override `.compass/config.toml`; project
configuration overrides the normal SQLite build default.

New configurations use version 2:

```toml
version = 2

[build]
include = ["src/"]

[storage]
store = "surreal"
surreal_engine = "surrealkv"
surreal_path = "compass-out/surreal"
```

The persisted path cannot contain `..`. It may be absolute when a shared local
volume is intentional; review or override absolute storage settings before
building an untrusted checkout.

The storage directory must be shared state outside `compass-out/snapshots/`;
Compass rejects a path that resolves to the output container itself or one of
its immutable snapshot directories.

Version-1 configurations still load with their previous storage behavior.

Publication stages and validates an immutable generation before committing the
filesystem snapshot. The snapshot's `surreal.ref` uses
`compass.surreal.ref/1` and binds the embedded engine, repository identity,
generation, graph digest, projection fingerprint, counts, and connection
location. The referenced projection is `compass.graph.surreal/2`; legacy
library-only v1 projections require republishing and are never migrated in
place. Queries pin that exact generation and never follow a mutable database
pointer or redefine the projection schema when opening it. An interrupted stage
leaves the preceding snapshot active; a later unchanged update repairs a
missing or stale projection without re-extracting source. Embedded bounded GC removes
unreferenced complete or incomplete generations of the publishing repository
only. Other checkouts sharing an explicitly configured store are never GC
candidates; retained generations are excluded before the bounded candidate read.
Automatic remote GC is disabled: this machine cannot know which generations
other machines retain. Coordinate remote reclamation administratively.

The admitted canonical graph digest is persisted with the generation manifest.
Every reference load compares that binding, so changing a digest to another
valid-looking SHA-256 value fails closed without a JSON read. Library-only
unbound stages cannot be opened through `surreal.ref`; core publication and
portable bundle restore use reference-aware staging.

Unchanged publication and `store validate` audit the persisted payloads, not
only the manifest. Repair reuses the exact claimed generation content and
marks a damaged generation incomplete until all node, relationship, and file
records have been restored and validated. A valid preceding generation is
never modified by staging another generation.

## Query

The current-project engine precedence is Surreal, SQLite, then JSON:

```bash
compass search PaymentService --engine surreal
compass callers PaymentService.charge --engine surreal
compass query --cql "MATCH (n:Function) RETURN n.name AS name ORDER BY name" \
  --engine surreal --format json
compass context modify PaymentService.charge --engine surreal
compass serve --engine surreal --transport stdio
```

Explicit `--engine surreal` fails closed if `surreal.ref` is absent, corrupt,
or names an engine not compiled into the binary. It never reads JSON or SQLite
as a fallback. Typed search, callers, callees, impact, explore, node trails,
task context, and MCP graph tools issue generation-filtered SurrealQL reads.
CompassQL lowers its graph scans and expansions to parameterized, bounded
Surreal rows and then applies the shared deterministic semantic evaluator for
typing, nulls, joins, optional matches, correlated `EXISTS`, aggregation,
ordering, unions, and shortest paths.

The CompassQL storage view is lazy: node scans return ordinal rows, each
expansion reads only the selected node's relationships, and property reads
fetch individual typed records. It never constructs a whole `Graph` or reads
a portable graph as a query prerequisite. Its per-query record cache is capped
at 256 records and an 8 MiB estimated size. Storage buffers and identity-only
reads have separate finite bounds; the query memory budget accounts for
semantic rows identically on every backend. Deadline and cancellation checks
also cover pending database reads.
The staged schema fingerprint keys the CLI plan cache.

Typed queries share Compass's existing ranking, ambiguity, truncation,
heuristic, and source-verification semantics. Projection search terms include
identifier subwords, diacritic normalization, and resolved alias names. A
generation-scoped short-prefix index narrows partial-token searches before
the complete prefix predicate, preserving JSON/SQLite recall and ranking.
Generation metadata is a separately bounded 4 MiB header; file records and
their source digests live in a generation/path index rather than that header.
File records are covered by the projection fingerprint and backup bundle.

Projection records retain canonical node and relationship ordinals, including
when a query reads only a bounded subset. Returned node, relationship, and path
values therefore retain the same snapshot-local identities and stable ordering
as the canonical JSON and SQLite engines.

Publication and query sessions share one process-wide physical connection
owner per canonical store path, running on a dedicated long-lived runtime.
Watch updates and MCP publication therefore do not depend on a caller runtime
remaining alive. Physical owners are bounded to 16 stores and hold their locks
until process exit; restart the process to release unused stores. Each session
selects its own namespace/database without changing another session's binding.

The MCP query cache shares one connection per physical store binding and pins
each requested generation independently. Concurrent first-open requests are
serialized, and publishing a new reference does not rebind an older reader.
The cache admits at most 16 distinct embedded stores and reports a capacity
error rather than evicting a connection that may still have pinned readers.

Historical `--at` queries retain their immutable history realization and do
not use the active project's embedded database.

## Validate, back up, and restore

```bash
compass store status compass-out --format json
compass store validate compass-out --engine surreal --format json
compass store backup compass-out --engine surreal --output /safe/compass-backup
compass store restore --from /safe/compass-backup --into restored-out
```

The Surreal backup is a `compass.surreal.backup/1` directory containing
`graph.json`, a manifest, and a digest-bound `compass.surreal.bundle/1`
projection. It does not copy an undocumented embedded-database directory.
Restore validates every binding, stages the projection into the selected store,
writes a new location-bound reference, and accepts only a new or empty output
directory.

Pass `--surreal-config connection.yaml` to restore into a configured remote
database, or `--surreal-engine surrealkv` to restore a remote bundle into a new
embedded store. The destination server must be explicitly configured; bundle
contents never authorize credential forwarding or deletion of remote data.
