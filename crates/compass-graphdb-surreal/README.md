# compass-graphdb-surreal

This optional integration crate projects one validated, immutable
`compass.graph/1` generation into schemafull SurrealDB node and relation
records. Canonical JSON remains portable; a published `surreal.ref` selects
Surreal for active-project queries ahead of SQLite or JSON. The owning core,
query, CLI, and MCP crates provide the fully wired `surreal-surrealkv` and
`surreal-rocksdb` features. This crate contains no presentation logic.

The crate has no default engine feature. Enable exactly the embedded profile
you need:

- `mem` for deterministic tests and ephemeral sessions;
- `surrealkv` for an embedded SurrealKV database; or
- `rocksdb` for an embedded RocksDB database.

These features resolve exactly `surrealdb` 3.2.4. That dependency and its core
are licensed under Business Source License 1.1 before conversion, with a
Database Service restriction, Change Date 2030-01-01, and Apache-2.0 Change
License. Surreal-enabled binaries, libraries, containers, and archives must
preserve the applicable SurrealDB license and notices and must not be described
as exclusively OSI-open-source. See
[`docs/future/surrealdb-license-decision.md`](../../docs/future/surrealdb-license-decision.md)
and the exact tagged license fixture under
[`scripts/fixtures/surreal-persistent-probes/`](../../scripts/fixtures/surreal-persistent-probes/).

No API in this crate accepts arbitrary SurrealQL. Values are parameter-bound
through a closed internal statement set.

Projection publication claims an incomplete generation, stages idempotent node,
relationship, and file batches in bounded 512-record commits, validates the
exact staged payloads, and completes the immutable manifest. Core stages
through `stage_for_reference` before atomically publishing the exact generation in
`compass.surreal.ref/1`; normal query reads never use the mutable database
pointer. The standalone `activate` API retains pointer-based library behavior.
Reference-aware staging binds the admitted graph digest in the immutable
manifest; a changed digest or an unbound library-only generation fails query
loading without reading JSON. Portable bundle restore preserves this binding.
GC is explicitly scoped to one repository and excludes retained generations
before limiting candidates, preserving other repositories in a shared store.
Interrupted candidates remain invisible and can be resumed without retaining
one graph-sized transaction or rewriting the prior valid generation.

Projection `compass.graph.surreal/2` persists typed payloads, shared normalized
search/alias terms, case-sensitive CompassQL labels/types, canonical ordinals,
source-file digests, and a bounded metadata header. `compass-query` runs its
shared typed-query and CompassQL semantics over generation-filtered record
selectors. Native CompassQL scans and expansions never hydrate an entire
graph. See the [embedded Surreal guide](../../docs/guides/surrealdb.md) for
configuration, operational validation, repair, and portable bundle backup.

With an engine feature enabled, `SurrealProjection` exposes generation-pinned,
bounded native reads for callers, callees, impact, directed node trails,
connected structural subgraphs, and relation pagination. These operations
return the shared Compass structural query contract. Relation-page cursors are
opaque, checksummed, and bound to one repository, generation, and operation.
