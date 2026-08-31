# Complete SurrealDB integration — precise status

Snapshot: 2026-08-30, after embedded + standalone-server release installation,
YAML/environment/flag configuration, and reduced verification.

**Latest operator instruction:** support both embedded and the existing
launch-agent server, configurable through YAML, environment, and flags;
install/check in first, then verify changed surfaces only. The combined release
is installed. Product commits `dee88248` and `90a0a4c2` are pushed to the fork
branch. The final 30 installed-binary checks and 11 configuration integration
tests passed, as did formatting and feature isolation. Two earlier live-suite
attempts timed out; their unconfirmed cause is retained in the
[server evidence](../../changes/surreal-server/verification.md).
Broad remaining gates and independent certification are deferred, not passed.
Phase 3 remains unfinished.

**One of the three requested phases is delivered to the fork's main.** The
implementation milestone counter is not a release-completion percentage.

## Phase and delivery table

| Phase/change | Implementation | Verification | Delivery | Remaining |
| --- | --- | --- | --- | --- |
| 1 — preflight-blocker | Complete | Passed, including actual-size enforcement, atomic failure, and five-estate qualification | Fork PR 2 merged; upstream PR 307 open | Upstream acceptance is external |
| 2 — surreal-integration | All planned embedded surfaces and integrity/restore repairs implemented; standalone-server extension also implemented | Earlier embedded evidence retained; final combined release passed 30 installed checks and 11 configuration integration tests. Formatting and feature isolation passed | Embedded commit `68d6341d` plus server commits `dee88248`, `90a0a4c2` pushed; combined optimized release installed globally | Fork-main PR and clean upstream PR; deferred review remains uncertified; intermittent timeout observations recorded |
| 3 — legacy-artifact-boundary | Not implemented; load paths inspected and authentic 0.3.6 release binary acquired and checksum-verified | Not run | No PR | All implementation, integration verification, review, and both PRs |
| Global installation | Current Phase 2 optimized SurrealKV + remote release installed | Final 30 changed-path installed checks passed; installed/build hashes match | Available through Cargo, Homebrew, and local-bin PATH locations | Phase 3 will require a later release install; broad end-to-end gates remain deferred |

## Phase 2 work-item table

| Work item | Status |
| --- | --- |
| Engine features and config v2 | Implemented; v1 compatibility and CLI precedence covered |
| Standalone server and YAML/environment/flags | Implemented and installed; real launch-agent publication/query/MCP/watch/backup/restore and credential precedence verified |
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
| Final formatting/Clippy/feature isolation/code-graph qualification | Formatting and feature isolation passed; broad Clippy/qualification deferred. Repository refresh completed with a partial-graph omission warning |
| Deterministic refinement and independent review | Evidence record initialized; not certified |
| Commit/push/fork-main merge/upstream PR | Product commits and fork-branch pushes complete through `90a0a4c2`; both PR delivery steps pending |

## Counters and scope

| Counter | Meaning |
| --- | --- |
| Original handoff | 18 implemented; C-015 and C-020 skipped |
| Current plan change counter | 2 of 4 including the standalone-server extension; remaining embedded evidence/review/delivery tasks keep that canonical change open |
| Current fully delivered phases | 1 of 3 |
| Individually registered successor subtasks | 19 total: 11 complete, 1 in progress, 7 pending. Phase 1: 3/3; Phase 2: 8/11; Phase 3/final install: 0/5 |
| Product check-in | Embedded integration and standalone-server/configuration changes committed and pushed; generated/runtime state excluded |
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
5. P3-05: final post-Phase-3 global release with embedded and remote support.

The authentic 0.3.6 release binary is isolated in a temporary directory; it was
not installed globally and no Phase 3 production files have been changed yet.
