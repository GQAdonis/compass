# Surreal integration acceptance contract

The binding acceptance criteria are phase 2 and its verification/delivery
requirements in `.kbd-orchestrator/phases/complete-surrealdb-integration/plan.md`.
The complete cumulative product diff, including newly added files, must be
reviewed. Prior library-only C-014 certification is not evidence that CLI,
publication, MCP, CompassQL, or operational integration is complete.

## Required implementation

1. Feature-gated `surreal-surrealkv` and `surreal-rocksdb` across core, query,
   CLI, and MCP. Default upstream builds have no SurrealDB dependency.
2. Project configuration v2 persists storage settings; existing v1 loads with
   existing behavior. Explicit CLI flags override project configuration, which
   overrides existing defaults. Support `--store surreal`, engine selection
   `--surreal-engine surrealkv|rocksdb` (SurrealKV default), and `--surreal-path`
   (checkout-local shared storage beneath compass-out by default).
3. Init, update, extract, and watch share publication. An unchanged build repairs
   missing/stale projections without source re-extraction.
4. Typed projection payloads include all CompassQL fields, deterministic search
   terms, names, scopes, and generation-filtered indexes. Stage immutable
   generations and validate them before committing the filesystem snapshot.
5. `compass.surreal.ref/1` binds engine, repository, generation, graph digest,
   projection fingerprint, counts, and location. Queries pin that exact
   generation, never a mutable database pointer. Failed staging preserves the
   old snapshot; failed filesystem commit leaves only reclaimable orphans.
   Include bounded orphan/incomplete-generation GC and idempotent repair.
6. Query, context, and MCP support `--engine surreal`. Default engine discovery
   prioritizes surreal.ref, then SQLite, then JSON. Explicit Surreal fails closed
   on absent/corrupt references. Canonical graph.json remains portable output,
   not a fallback used by default/explicit Surreal execution.
7. Ask, search, callers, callees, impact, explore, and node execute through
   generation-filtered SurrealQL indexes with identical compass.query/1 output.
8. Native CompassQL supports every currently supported logical operator through
   parameterized generation-pinned SurrealQL work. Shared Rust semantic
   evaluation may consume bounded result rows, but never materialize a complete
   graph or fall back to JSON/SQLite. Preserve types, nulls, direction, repeated
   variables, optional matches, correlated EXISTS, aggregation, ordering, unions,
   shortest paths, limits, cancellation, profiles, and stable errors.
9. MCP typed tools share the Surreal engine and connection cache. Upgrade rmcp
   from 2.2.0 to 3.1.4 with stdio and streamable HTTP protocol negotiation.
10. Store status, validate, backup, and restore support Surreal. Backups are
    versioned digest-bound Compass projection bundles, not raw database copies.
    Capabilities expose surreal_store, surreal_cql, engine availability, and the
    reference contract. Document commands, configuration, outputs, migration,
    security, performance, licensing, CI, and installation.
11. Embedded local engines only; no endpoints/credentials/TLS. Historical --at
    realization paths remain immutable, independent of active project settings.

## Certification and delivery sequence

The entire production/schema/configuration/documentation implementation precedes
the integration wave. Only full public-boundary integration suites count;
Cargo/rustc commands are host-serialized. Run the default workspace suite,
complete affected SurrealKV and optional RocksDB suites, all supported
CompassQL/OpenCypher differential scenarios (ordered rows/values/errors/limits/
profiles), product/feature isolation, formatting, and workspace lib/bin Clippy.
Cover persistence/watch, interruption recovery, exact generation pinning,
default/explicit selection, all typed CLI/MCP tools, backup/restore, and absence
of JSON fallback. The verification ledger records the exact current results.

This independent review precedes certification and publication: the fork-main
PR merge and corresponding clean upstream PR are required afterward, not claimed
as already completed. Phase 1 is separately delivered. Phase 3's legacy boundary
and the final global release installation are later separate work, outside this
phase's implementation diff. Prior library-only C-014 receipts cannot certify
these integration requirements.
