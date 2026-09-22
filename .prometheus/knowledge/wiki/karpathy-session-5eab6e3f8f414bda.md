---
type: SessionRecord
id: karpathy-session-5eab6e3f8f414bda
title: Karpathy session 5eab6e3f8f41
tags:
- karpathy
- session-learning
sources:
- session:0cf17349-e0a0-43ed-808c-89b35adbc3e3
timestamp: 2026-09-21T16:27:29.325633+00:00
created_at: 2026-09-21T16:27:29.325633+00:00
updated_at: 2026-09-21T16:27:29.325633+00:00
revision: 0
---

## Delta

## Found and fixed a second bug of mine

The wired-surfaces step caught `supported_compassql_operators_are_differentially_identical` failing with *"CQL adjacency endpoint identity mismatch"*.

**Root cause:** my `degree` implementation reused `cql_adjacent_at`, which resolves every neighbor node's ordinal and fails closed if one isn't in the projection. That's correct for traversal, wrong for a degree — degree counts incident edges regardless of whether the far endpoint is retained by the current selection.

**Fix:** added `cql_degree_at`, which counts relation rows and stops before endpoint resolution. It keeps the same bounded read and limit-vs-empty distinction. `surreal_cql` now passes 6/6.

Both bugs I've found today came from the same gap: I verified with the feature combinations I happened to pick, not the ones CI actually runs. Running the real CI steps is what surfaced them.

**CI steps now passing locally:**

| Step | Result |
|---|---|
| `--features mem` | pass |
| `--features surrealkv` | pass |
| `surreal_cql` differential | 6/6 |
| wired `surreal-surrealkv` (core/query/cli/mcp) | re-running |

Still to do: the `cargo check` combined embedded+remote step, `compass-files` surreal config, full workspace wave, fmt/clippy, then push.

## Root Cause

No explicit root-cause section was captured; preserve this as a session record, not an inferred diagnosis.

## Corrective Actions

Review and promote only reusable findings.

## Session Metadata

- Harness: claude-code
- Session: 0cf17349-e0a0-43ed-808c-89b35adbc3e3
- Captured: 2026-09-21T16:27:27.943914Z
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
