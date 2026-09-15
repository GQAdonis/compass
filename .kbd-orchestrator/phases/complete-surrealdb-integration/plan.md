# Complete SurrealDB Integration and Downstream Blocker Fixes

This is the user's explicit successor plan, not completion of the older
`compass-scoping-and-bounds` plan. Three implementation-first phases ship
separately. Finish each phase's production code, schemas, configuration, and
documentation before its coordinated integration wave. Cargo/rustc is serialized
across the host. Unit tests are neither added nor counted as evidence.

1. **preflight-blocker**: Remove the guessed 60x fatal estimate. Enforce the
   actual canonical JSON byte cap during full/delta staging, preserve the active
   snapshot on failure, and publish five-estate size ratios and successful
   combined-root rebuild qualification without raising the 2 GiB cap.
2. **surreal-integration**: Optional SurrealKV and RocksDB features across core,
   query, CLI, and MCP; v2 persisted configuration with v1 compatibility and
   explicit CLI precedence; identical init/update/extract/watch publication and
   unchanged repair; typed, indexed, immutable generation staging before atomic
   filesystem publication; exact `compass.surreal.ref/1` pinning; bounded GC;
   native typed operations and every supported CompassQL operator without whole
   graph hydration or JSON/SQLite fallback; MCP shared connections and rmcp
   3.1.4; status/validate/digest-bound backup/restore; capabilities; complete
   command, configuration, migration, licensing, security, performance,
   installation, and CI documentation. Embedded local engines only. Historical
   realizations remain immutable and separate from active configuration.
3. **legacy-artifact-boundary**: Reject pre-0.3.23 `graph.build.builderVersion`
   in a bounded load preamble before node/edge decoding. Report found/minimum
   versions and exact `compass update "<source-root>" --force` from validated
   `source-root.txt`, or explicitly require a root if provenance is absent.
   Cover a real bounded 0.3.6 fixture and successful force rebuild.

After implementation, each phase requires the full workspace integration-only
suite, complete affected feature suites, CompassQL/OpenCypher TCK, product and
feature-isolation gates, formatting, and workspace library/binary Clippy.
Surreal verification covers both engines, all typed CLI/MCP operations,
watch/update persistence, interrupted publication, exact generation pinning,
repair, backup/restore, no fallback, and differential rows/errors/limits/profiles.
The kbd-execute deterministic refinement and fresh independent review gates
precede certification. Failed gates are repaired in production batches, followed
by complete affected integration suites, never filtered-test shortcuts.

Push each phase branch to GQAdonis/compass, merge a PR into that fork's main,
then open its clean corresponding PR against crabbuild/compass:main. After all
phases pass, install globally in release mode with SurrealKV and smoke-test
capabilities, publication, typed queries, CompassQL, MCP, and store validation.

## Retained delivery state

- Phase 1: fork PR #2 merged; upstream PR #307 open.
- Phase 2: production implementation and corrective batch complete; integration
  verification pending. No Phase 2 certification, push, or merge claimed.
- Phase 3 and final global release installation: pending.
