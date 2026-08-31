# Phase 2 verification ledger

Status: **IN PROGRESS — not certified or delivered.**

## Retained results, 2026-08-30

- The full default workspace integration-only suite passed before the latest
  repair batch. It is being rerun after that batch; the older pass is not a
  substitute for current-source verification.
- The first complete SurrealKV CLI suite exposed relative-path `explore` failure
  and watch's embedded-lock/runtime-lifetime failure. The following full
  query/core/MCP sweep exposed prefix-search parity and an MCP Surreal failure.
  All five native CompassQL integration tests passed in that earlier sweep.
- Production repairs now provide process-owned embedded connections, absolute
  source-verification anchoring, a generation-scoped short-prefix index,
  independent storage buffers and semantic memory budgets, and stream-bounded
  reference/bundle reads. Integration coverage additionally exercises runtime
  replacement, tiny memory budgets, and precise errors without JSON fallback.
- Product-boundary and CompassQL support-manifest checks passed. Viewer assets
  regenerated identically after installing the locked JavaScript dependencies.
- The MCP HTTP conformance harness passed its existing expected-failure baseline
  against the binary containing the rmcp 3.1.4 transport changes. The baseline
  explicitly excludes unavailable diagnostic tools and verifies that the
  whitespace-header case fails for invalid tool arguments, not header parsing.

## Active coordinated wave

The complete corrective source/documentation batch preceded this wave. Compass
acquired the host Rust slot only after UAR's explicit release. Commands are
serialized; no unit harnesses or test-name filters are completion evidence.

1. `CARGO_INCREMENTAL=0 cargo test --workspace --test '*' --locked --no-fail-fast`
   — **PASS**, exit 0, after the complete corrective batch. The macOS linker
   emitted its unwind-table-size warning; existing opt-in acceptance tests
   remained ignored. No library/binary unit harness was selected.
2. Complete SurrealKV plus in-memory projection integration suites finished
   with three failures. Typed-operation differential parity, watch persistence,
   MCP graph/context tools, persistent runtime replacement, five native CQL
   suites (including selected OpenCypher scenarios), and all other affected
   integration targets passed. The three failures exposed fixture defects:
   an "unchanged" CLI repair changed init's build profile; a competing immutable
   generation plan had internally inconsistent metadata; and a new limit query
   ordered by a variable no longer in projection scope. The fixture batch now
   preserves the profile, constructs a valid competing plan, and orders by its
   projected alias. The complete affected-suite rerun then **PASSED**, exit 0,
   with all three CLI tests, five memory projection tests, two persistent
   contract tests, fifteen MCP graph tests, six native CQL suites, and two typed
   differential suites passing alongside the remaining complete affected
   integration targets. No production behavior was weakened. The equivalent
   RocksDB-plus-memory full affected-suite command also **PASSED**, exit 0,
   with SurrealKV disabled so native query coverage selected RocksDB.

## Subsequent storage-integrity audit batch

After both complete engine sweeps finished, source audit identified and repaired
two additional acceptance gaps before certification: graphDigest is now bound
in the generation manifest during reference-aware staging and preserved through
restore; GC explicitly scopes candidates to one repository and excludes retained
generations before applying its finite limit. CLI, native query, and persistent
integration fixtures now cover syntactically valid digest tampering, unbound
projection rejection, cross-repository preservation, and one-candidate GC progress.
Production, schema, and documentation were completed before this new integration
coverage. The post-batch complete default workspace integration-only suite
**PASSED**, exit 0, after UAR explicitly released the Rust slot. The complete
five-package SurrealKV-plus-memory suite finished with one failing target:
CLI restore still used unbound staging. All other affected targets passed,
including both persistent-contract tests, all six native CompassQL differential
suites, both typed suites, and all fifteen MCP graph tests. The macOS linker
retained its existing unwind-table-size warning and existing opt-in external
acceptance tests remained ignored.

## CLI restore correction batch

CLI restore now stages with its new reference, preserving the admitted digest
while changing physical location. Backup manifest input is also capped at 64 KiB
using the existing stream-bounded file reader. Production and documentation
changes preceded added public integration coverage: oversized manifests fail
before destination creation, and restored-generation queries work after removal
of canonical JSON. The complete default CLI target set is running, followed by
full SurrealKV and RocksDB affected suites. Other default workspace production
code has not changed since its passing full run.

The current MCP HTTP harness independently **PASSED** its documented
expected-failure baseline, including the checked whitespace-header failure
cause. This is baseline conformance, not a claim that every upstream diagnostic
tool exists.

## Operator-directed deployment priority

The complete default CLI suite after the restore correction **PASSED**, exit 0,
including all four store-operation integration tests. The operator then directed
immediate global release installation and check-in before reduced verification.
The SurrealKV release installation completed successfully. No further broad workspace or
all-engine suite is authorized for this priority wave. After installation and
check-in, verify the changed Surreal CLI restore/publication path and perform
installed-binary smoke checks. Deferred gates below remain unverified.

### Urgent deployment result

- Product implementation committed and pushed to the fork branch as `68d6341d`.
- `cargo install --path crates/compass-cli --bin compass --features
  surreal-surrealkv --profile release --locked --force` completed successfully
  in 15m25s, retaining the configured optimized release profile and thin LTO.
- The global executable's SHA-256 matches the worktree release artifact:
  `1ca33dc45d207e6277d1ef63957be25460cf392b8e8b3188ef273ec95b31b747`.
  Common Homebrew and local-bin PATH entries resolve to the same executable.
- After installation and push, all **19 changed-path installed-binary checks
  passed**: capabilities, persisted Surreal init, all seven typed operations,
  profiled native CompassQL, unchanged reuse, tampered-digest rejection,
  extraction-free repair, portable backup, digest-bound restore to a new
  location, validation, typed/native queries without canonical JSON, and
  oversized-manifest rejection before destination creation.
- The machine-wide Rust slot was explicitly released after installation.
  No broad suite or Cargo rebuild was run for these installed-binary checks.
- This is an urgent Phase 2 deployment, not certification of deferred gates,
  a fork-main merge, or completion of Phase 3.

## Remaining gates

3. Formatting, workspace library/binary Clippy, feature isolation, affected
   qualification gates — pending on final source.
4. Deterministic refinement followed by fresh independent adversarial review —
   pending. No prior library-only review satisfies this integration gate.
5. Fork-main PR merge and clean upstream PR — pending.

Final global SurrealKV release installation follows all three delivered phases,
not this implementation counter. The Phase 3 legacy rebuild boundary is pending.
