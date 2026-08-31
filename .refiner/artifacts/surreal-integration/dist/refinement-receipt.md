# Surreal integration refinement receipt

Status: **PENDING — not certified or delivered.**
Date: 2026-08-30

Retained checks: the complete default workspace integration suite passed after
graph-digest binding and repository-scoped GC. The subsequent full SurrealKV
affected-suite run passed all storage, MCP, typed-query, and native CompassQL
targets, but exposed one unbound CLI restore caller. That caller is corrected;
the complete default CLI suite then passed, including bounded backup-manifest
coverage. Earlier complete RocksDB and SurrealKV sweeps passed before the latest
integrity batch. Product boundary, support manifest, deterministic viewer assets,
and current MCP HTTP expected-failure-baseline conformance passed.

The operator explicitly prioritized global SurrealKV release installation and
check-in before changed-surface-only verification. The Surreal-enabled CLI
restore check and installed-binary smoke remain pending. Broad repeated suites,
final Clippy/qualification, constraint reconciliation, and fresh independent
review are deferred. This receipt does not certify or waive them.

The canonical artifact manifest is used. Constraint results retain the established
repository receipt schema because the installed canonical constraints enum omits
the code domain despite supporting it in the skill contract. This is an explicit
tooling fallback, not a waiver of any repository constraint.
