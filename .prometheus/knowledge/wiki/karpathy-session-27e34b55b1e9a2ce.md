---
type: SessionRecord
id: karpathy-session-27e34b55b1e9a2ce
title: Karpathy session 27e34b55b1e9
tags:
- karpathy
- session-learning
sources:
- session:0cf17349-e0a0-43ed-808c-89b35adbc3e3
timestamp: 2026-09-21T17:30:12.525628+00:00
created_at: 2026-09-21T17:30:12.525628+00:00
updated_at: 2026-09-21T17:30:12.525628+00:00
revision: 0
---

## Delta

## Every CI step now passes locally

This is the verification I should have done before my first "complete":

| CI step (`surrealdb-projection` job) | Result |
|---|---|
| Default feature isolation | PASS |
| `--features mem` | PASS *(was the failing step)* |
| `--features surrealkv` | PASS |
| Wired `surreal-surrealkv` (core/query/cli/mcp) | 70 suites, 421 tests, 0 failed |
| Combined embedded + standalone `cargo check` | PASS |
| Bounded YAML / endpoint config | 4/4 |
| `cargo fmt --check` | PASS |
| `clippy -D warnings` | 0 issues |

Full workspace wave is running — the last gate before I push.

**Two bugs found by doing this properly, both mine:**

1. **Trail ranking** — a genuine merge integration gap. Upstream switched the JSON engine to weighted Dijkstra; our Surreal backend still used evidence-quality BFS, so the two disagreed on parallel edges. Ported upstream's algorithm exactly (I verified the weight function and sort are byte-identical).

2. **`degree`** — I reused `cql_adjacent_at`, which resolves neighbor ordinals and fails closed when one isn't in the projection. Correct for traversal, wrong for a count. Added `cql_degree_at` that counts relation rows and stops before endpoint resolution.

Once the wave is green I'll commit, push, and re-trigger CI to get actual Windows evidence. I'm not claiming Windows works until that job reports success — last time it didn't, and that was worth knowing.

## Root Cause

No explicit root-cause section was captured; preserve this as a session record, not an inferred diagnosis.

## Corrective Actions

Review and promote only reusable findings.

## Session Metadata

- Harness: claude-code
- Session: 0cf17349-e0a0-43ed-808c-89b35adbc3e3
- Captured: 2026-09-21T17:30:10.315814Z
- Project: /Users/gqadonis/Projects/references/compass

## Changed Paths

- crates/compass-graphdb-surreal/src/engine/persistent.rs
- crates/compass-graphdb-surreal/src/engine/query.rs
- crates/compass-graphdb-surreal/src/engine/reader.rs
- crates/compass-graphdb-surreal/tests/persistent_contract.rs
- crates/compass-query/src/surreal_cql.rs
- .agents/skills/compass/
- .claude/CLAUDE.md
- .claude/settings.json
- .claude/skills/compass/
- .codex/
- .compass/
- .kbd-orchestrator/phases/compass-scoping-and-bounds/hooks.log.jsonl.lock
- .kbd-orchestrator/phases/compass-scoping-and-bounds/review/C-001/packet.json
- .kbd-orchestrator/phases/compass-scoping-and-bounds/review/C-002/packet-2.json
- .kbd-orchestrator/phases/compass-scoping-and-bounds/review/C-002/packet-2.raw.json
- .kbd-orchestrator/phases/compass-scoping-and-bounds/review/C-002/packet-3.json
- .kbd-orchestrator/phases/compass-scoping-and-bounds/review/C-002/packet-3.raw.json
- .kbd-orchestrator/phases/compass-scoping-and-bounds/review/C-002/packet-4.json
- .kbd-orchestrator/phases/compass-scoping-and-bounds/review/C-002/packet-4.raw.json
- .kbd-orchestrator/phases/compass-scoping-and-bounds/review/C-002/packet.json
- .kbd-orchestrator/phases/compass-scoping-and-bounds/review/C-003/
- .kbd-orchestrator/phases/compass-scoping-and-bounds/review/C-004/
- .kbd-orchestrator/phases/compass-scoping-and-bounds/review/C-005/packet.json
- .kbd-orchestrator/phases/compass-scoping-and-bounds/review/C-006/packet.json
- .kbd-orchestrator/phases/compass-scoping-and-bounds/review/C-007/isolated-broad-files.patch
- .kbd-orchestrator/phases/compass-scoping-and-bounds/review/C-007/packet.json
- .kbd-orchestrator/phases/compass-scoping-and-bounds/review/C-008/packet.json
- .kbd-orchestrator/phases/compass-scoping-and-bounds/review/C-008/prior-selected-file-baseline.patch
- .kbd-orchestrator/phases/compass-scoping-and-bounds/review/C-010/packet.json
- .kbd-orchestrator/phases/compass-scoping-and-bounds/review/C-012/packet.json
- .kbd-orchestrator/phases/compass-scoping-and-bounds/review/C-013/packet.json
- .kbd-orchestrator/phases/compass-scoping-and-bounds/review/C-014/packet-round1.json
- .kbd-orchestrator/phases/compass-scoping-and-bounds/review/C-014/packet-round2.json
- .kbd-orchestrator/phases/compass-scoping-and-bounds/review/C-014/packet.json
- .kbd-orchestrator/phases/compass-scoping-and-bounds/review/C-016/packet.json
- .kbd-orchestrator/phases/compass-scoping-and-bounds/review/C-016/packet.round2.json
- .kbd-orchestrator/phases/compass-scoping-and-bounds/review/C-016/packet.round3.json
- .kbd-orchestrator/phases/compass-scoping-and-bounds/review/C-017/packet.json
- .kbd-orchestrator/phases/compass-scoping-and-bounds/review/C-018/packet.json
- .kbd-orchestrator/phases/compass-scoping-and-bounds/review/C-019/packet.json
