# Complete SurrealDB integration — precise status

Snapshot: 2026-08-30, after graph-digest binding, repository-scoped GC, and CLI
restore correction; urgent release deployment and reduced checks completed.

**Latest operator instruction:** install the current SurrealKV release globally
and check it in immediately, then verify only changed surfaces. The release
installation succeeded; all 19 changed-path installed-binary checks passed.
Implementation commit `68d6341d` is pushed to the fork branch. Broad remaining gates and review are deferred, not
passed. This is an urgent Phase 2 deployment; Phase 3 remains unfinished.

**One of the three requested phases is delivered to the fork's main.** The
implementation milestone counter is not a release-completion percentage.

## Phase and delivery table

| Phase/change | Implementation | Verification | Delivery | Remaining |
| --- | --- | --- | --- | --- |
| 1 — preflight-blocker | Complete | Passed, including actual-size enforcement, atomic failure, and five-estate qualification | Fork PR 2 merged; upstream PR 307 open | Upstream acceptance is external |
| 2 — surreal-integration | All planned surfaces and the latest integrity/restore repairs implemented | Post-integrity default workspace and post-restore default CLI suites passed. All 19 reduced installed-SurrealKV checks passed. Broad remaining gates deferred by operator | Committed/pushed as `68d6341d`; optimized SurrealKV release installed globally; no integration PR submitted | Fork-main PR and clean upstream PR; deferred verification/review remain uncertified |
| 3 — legacy-artifact-boundary | Not implemented; load paths inspected and authentic 0.3.6 release binary acquired and checksum-verified | Not run | No PR | All implementation, integration verification, review, and both PRs |
| Global installation | Current Phase 2 optimized SurrealKV release installed | 19 changed-path installed-binary checks passed | Available through Cargo, Homebrew, and local-bin PATH locations | Phase 3 will require a later release install; broad end-to-end gates remain deferred |

## Phase 2 work-item table

| Work item | Status |
| --- | --- |
| Engine features and config v2 | Implemented; v1 compatibility and CLI precedence covered |
| Init/update/extract/watch | Implemented; complete SurrealKV CLI suite passed |
| Missing/stale projection repair | Passing without extraction and with unchanged graph digest |
| Staging/pinning/interruption/GC | Implemented; SurrealKV persistent-contract suite passed |
| Persisted graph-digest binding | Storage/native query tampering regressions passed; installed CLI repair/restore checks passed |
| Repository-scoped generation GC | Cross-repository preservation and bounded candidate-progress integration coverage passed on SurrealKV |
| Typed ask/search/callers/callees/impact/explore/node | Implemented; SurrealKV differential parity passed |
| Native CompassQL | Six SurrealKV suites passed, including supported OpenCypher scenarios, limits/errors/profiles, and no JSON fallback |
| MCP/shared connections/rmcp 3.1.4 | Typed tools and stdio tests passed; HTTP passed its documented expected-failure baseline |
| Status/validate/backup/restore | Corrected digest-bound CLI restore passed on the installed release, including validation, restored queries without JSON, and bounded backup manifests |
| Capabilities/docs/config/migration/security/licensing/performance/CI | Implemented; final consistency review pending |
| Optional RocksDB | Complete affected integration sweep passed with SurrealKV disabled; post-repair rerun pending |
| Final formatting/Clippy/feature isolation/code-graph qualification | Pending |
| Deterministic refinement and independent review | Evidence record initialized; not certified |
| Commit/push/fork-main merge/upstream PR | Commit and fork-branch push complete (`68d6341d`); both PR delivery steps pending |

## Counters and scope

| Counter | Meaning |
| --- | --- |
| Original handoff | 18 implemented; C-015 and C-020 skipped |
| Current plan change counter | 1 of 3; canonical tasks now distinguish implementation from remaining verification/review/delivery work |
| Current fully delivered phases | 1 of 3 |
| Individually registered successor subtasks | 19 total: 11 complete, 1 in progress, 7 pending. Phase 1: 3/3; Phase 2: 8/11; Phase 3/final install: 0/5 |
| Product check-in | 74 product files committed in `68d6341d`, alongside 13 portable specification/evidence files; generated/runtime state excluded |
| Rust ownership | Explicitly released to UAR after the successful release installation; no further Cargo/rustc command running |

Links: https://github.com/GQAdonis/compass/pull/2 and
https://github.com/crabbuild/compass/pull/307. This report is a dated snapshot;
the change verification ledger records later results. Full completion still
requires finishing/delivering Phase 2, all of Phase 3, and the final installation.

## Remaining work, in order

1. P2-09: operator-requested reduced installed checks completed; broad remaining
   verification is deferred, not passed.
2. P2-10: deterministic refinement and fresh independent review deferred.
3. P2-11: fork-main PR merge and clean upstream PR (commit/push complete).
4. P3-01 through P3-04: implement, verify, review, and deliver the legacy boundary.
5. P3-05: final global release installation with SurrealKV and end-to-end smoke.

The authentic 0.3.6 release binary is isolated in a temporary directory; it was
not installed globally and no Phase 3 production files have been changed yet.
