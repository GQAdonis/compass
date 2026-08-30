# Complete SurrealDB integration — precise status

Snapshot: 2026-08-30, after graph-digest binding, repository-scoped GC, and CLI
restore correction; affected full-suite verification running.

**Latest operator instruction:** install the current SurrealKV release globally
and check it in immediately, then verify only changed surfaces. The release
installation is running. Broad remaining gates and review are deferred, not
passed. This is an urgent Phase 2 deployment; Phase 3 remains unfinished.

**One of the three requested phases is delivered to the fork's main.** The
implementation milestone counter is not a release-completion percentage.

## Phase and delivery table

| Phase/change | Implementation | Verification | Delivery | Remaining |
| --- | --- | --- | --- | --- |
| 1 — preflight-blocker | Complete | Passed, including actual-size enforcement, atomic failure, and five-estate qualification | Fork PR 2 merged; upstream PR 307 open | Upstream acceptance is external |
| 2 — surreal-integration | All planned surfaces and the latest integrity/restore repairs implemented | Post-integrity default workspace passed. SurrealKV passed every target except CLI restore; that correction is implemented and the complete default CLI rerun is running. Both-engine final reruns remain | Current patch uncommitted, no integration PR submitted | Finish post-repair full suites and gates, independent review, fork-main PR, clean upstream PR |
| 3 — legacy-artifact-boundary | Not implemented; load paths inspected and authentic 0.3.6 release binary acquired and checksum-verified | Not run | No PR | All implementation, integration verification, review, and both PRs |
| Final global installation | Pending final release build | Installed-binary smoke tests pending | Current Phase 2 patch not installed | Release install with SurrealKV and capabilities/publication/query/CQL/MCP/store smoke tests |

## Phase 2 work-item table

| Work item | Status |
| --- | --- |
| Engine features and config v2 | Implemented; v1 compatibility and CLI precedence covered |
| Init/update/extract/watch | Implemented; complete SurrealKV CLI suite passed |
| Missing/stale projection repair | Passing without extraction and with unchanged graph digest |
| Staging/pinning/interruption/GC | Implemented; SurrealKV persistent-contract suite passed |
| Persisted graph-digest binding | Storage/native query tampering regressions passed; CLI restore caller corrected and rerun pending |
| Repository-scoped generation GC | Cross-repository preservation and bounded candidate-progress integration coverage passed on SurrealKV |
| Typed ask/search/callers/callees/impact/explore/node | Implemented; SurrealKV differential parity passed |
| Native CompassQL | Six SurrealKV suites passed, including supported OpenCypher scenarios, limits/errors/profiles, and no JSON fallback |
| MCP/shared connections/rmcp 3.1.4 | Typed tools and stdio tests passed; HTTP passed its documented expected-failure baseline |
| Status/validate/backup/restore | Earlier round trip passed; new manifest binding exposed an unbound CLI restore caller, now corrected. Current rerun includes restored queries without JSON and bounded backup manifests |
| Capabilities/docs/config/migration/security/licensing/performance/CI | Implemented; final consistency review pending |
| Optional RocksDB | Complete affected integration sweep passed with SurrealKV disabled; post-repair rerun pending |
| Final formatting/Clippy/feature isolation/code-graph qualification | Pending |
| Deterministic refinement and independent review | Evidence record initialized; not certified |
| Commit/push/fork-main merge/upstream PR | Pending successful gates |

## Counters and scope

| Counter | Meaning |
| --- | --- |
| Original handoff | 18 implemented; C-015 and C-020 skipped |
| Current plan change counter | 1 of 3; canonical tasks now distinguish implementation from remaining verification/review/delivery work |
| Current fully delivered phases | 1 of 3 |
| Individually registered successor subtasks | 19 total: 11 complete, 1 in progress, 7 pending. Phase 1: 3/3; Phase 2: 8/11; Phase 3/final install: 0/5 |
| Uncommitted product diff | 74 product files, excluding internal tracking state; exact line count changes during repair |
| Rust ownership | Compass reacquired after UAR's explicit release; full default CLI repair verification is running |

Links: https://github.com/GQAdonis/compass/pull/2 and
https://github.com/crabbuild/compass/pull/307. This report is a dated snapshot;
the change verification ledger records later results. Full completion still
requires finishing/delivering Phase 2, all of Phase 3, and the final installation.

## Remaining work, in order

1. P2-09: full post-repair default and both-engine integration suites, final
   formatting/Clippy/feature-isolation/qualification gates.
2. P2-10: deterministic refinement and fresh independent review.
3. P2-11: commit, push, fork-main PR merge, clean upstream PR.
4. P3-01 through P3-04: implement, verify, review, and deliver the legacy boundary.
5. P3-05: final global release installation with SurrealKV and end-to-end smoke.

The authentic 0.3.6 release binary is isolated in a temporary directory; it was
not installed globally and no Phase 3 production files have been changed yet.
