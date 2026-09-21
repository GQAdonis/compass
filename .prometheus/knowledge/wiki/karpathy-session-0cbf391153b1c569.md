---
type: SessionRecord
id: karpathy-session-0cbf391153b1c569
title: Karpathy session 0cbf391153b1
tags:
- karpathy
- session-learning
sources:
- session:2fb2846a-95d4-45df-8a8a-cd053319e691
timestamp: 2026-09-18T05:03:06.547132+00:00
created_at: 2026-09-18T05:03:06.547132+00:00
updated_at: 2026-09-18T05:03:06.547132+00:00
revision: 0
---

## Delta

Privately: nothing further is needed. The push succeeded (`2d385dee..1363dcae`), the tree is clean, and I already have every source required for the feature description — both upstream released CHANGELOG sections, the fork's full Unreleased set, and ground-truth `--help` output from the installed 0.3.25 binary. No tool calls remain; this turn is the writeup.

Pushed — `2d385dee..1363dcae` to `origin/docs/claude-md` (GQAdonis/compass). Three commits from this session, tree clean:

| Commit | What |
|---|---|
| `70246b61` | Merge upstream/main (0.3.25) |
| `b2303f02` | Re-pin canonical umbrella skill digest |
| `1363dcae` | Session wiki updates |

I committed the three `.prometheus/knowledge/wiki/*` files rather than leaving them dirty — git history shows they're deliberately tracked here, with `5ca57f24 chore(knowledge): session wiki updates` as the established precedent.

---

# New features in this update

Two distinct sources landed on the branch: **upstream releases 0.3.24/0.3.25** (genuinely new to you), and **your fork's Unreleased work** (which the merge carried onto this branch). Worth separating, since only the first is new *code*.

## From upstream

### `compass ensure` — session and worktree bootstrap
The headline user-facing addition. An idempotent command that guarantees the active worktree has a current graph, reporting whether it *initialized*, *updated*, or was *already current* (a no-op).

```
compass ensure                    # defaults to Git worktree root
compass ensure . --no-viz
compass ensure ./services/api --program
```

Its real purpose is linked worktrees: it keeps output worktree-local so multiple worktrees never share one mutable `compass-out/`. Upstream also rewrote the installed agent guidance to call `ensure` instead of `update .`, and to run it whenever a session starts or moves between worktrees — that guidance edit is precisely what broke your fork's skill-digest guard during the build.

### Leiden community detection replaces Louvain
The most substantial internal change — three commits (#314, #315, #316). Production community detection for typed graphs is now deterministic native **Leiden** over a versioned typed-evidence topology, replacing Louvain. Notably it's now *faster* than the Louvain implementation it replaces on the pinned qualification corpus, which is unusual — Leiden normally trades speed for quality. They achieved it by reusing graph invariants and scratch space to cut repeated community scans.

This introduces a new artifact: **`compass.community-quality/1`**, digest-bound evidence published alongside the graph as `community-quality.json`, with frozen influence and full-quality fallback preserved during incremental updates. Fixed resolution remains the default; the bounded three-candidate selector is still gated behind qualification.

### History limit raised 512 MiB → 5 GiB
`MAX_AUTHORITATIVE_BYTES` for aggregate authoritative evidence per realization. **This one has a compatibility edge worth knowing:** older Compass readers still reject realizations above 512 MiB, so anyone sharing larger realizations needs readers carrying this widening first. Per-record and record-count bounds are unchanged.

This is directly adjacent to your `compass-scoping-and-bounds` phase work — it relaxes the *history* ceiling, while your preflight guard addresses the *canonical graph* ceiling. They're complementary, not redundant, and both survived the merge intact.

### Smaller upstream items
- **`compass review` readability** — shorter commit/report references, plain-language status labels, witness relationships summarized instead of printing opaque graph-node IDs. Canonical JSON and SARIF are byte-identical, so only human output changed.
- **Strict URI node records** preserved in review (#310).
- **14 universal-evidence producers reset to v1**, still `Qualified` — cached evidence from prior producer identities regenerates.
- Worktree-safe graph ensure plumbing (#309), yanked `chacha20` lockfile en

## Root Cause

No explicit root-cause section was captured; preserve this as a session record, not an inferred diagnosis.

## Corrective Actions

Review and promote only reusable findings.

## Session Metadata

- Harness: claude-code
- Session: 2fb2846a-95d4-45df-8a8a-cd053319e691
- Captured: 2026-09-13T21:44:29.695510Z
- Project: /Users/gqadonis/Projects/references/compass

## Changed Paths

- No changed paths detected.
