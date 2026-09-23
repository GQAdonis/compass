# Agent query evaluation: five repositories, five languages

## Result

Compass answers the reviewed agent questions more accurately and publishes a
better-anchored graph than Graphify on this suite, but it still spends more
tokens per answered question. The evaluation ran on 2026-09-23 against Compass
commit `3fd246dc` plus the fixes in this change, and Graphify `0.9.36`.

| Metric | Compass | Graphify |
| --- | ---: | ---: |
| Source-reviewed answers passed | 36/39 | 24/39 |
| Reviewed graph anchors present | 15/15 | 13/15 |
| Source-backed nodes | 100% | 86% |
| Median tokens per answered question | 432 | 121 |
| Broad natural questions answered | 2/5 | 5/5 |

Compass passed every `callers`, `explain_source`, `file_path`, and `negative`
row; Graphify passed none of the `callers` or `explain_source` rows. The
remaining Compass failures are natural-language `broad` questions on Gson,
Zod, and Axum, where discovery seeded surface terms such as `json`, `object`,
`input`, and `request` instead of the domain symbols `toJson`, `JsonWriter`,
`safeParse`, and `Router`. The Zod row's oracle requires the `parse` family
specifically, so it is the weakest of the three judgments.

## Design

`benchmarks/agent_query/suite.toml` pins one checkout per language:
`spf13/cobra` (Go), `pallets/flask` (Python), `google/gson` (Java),
`colinhacks/zod` (TypeScript), and `tokio-rs/axum` (Rust). Each repository
contributes questions in eight kinds: `explain`, `explain_source`, `callers`,
`path`, `file_path`, `ambiguity`, `negative`, and `broad`. Every question
records the exact per-tool argument vector, the expected outcome, the anchors
the reviewer read in the pinned source, and the graph anchors both graphs must
carry as source-backed nodes.

`benchmarks/agent_query/runner.py` builds both graphs, executes the suite with
hard timeouts and on-disk bounded capture, and judges stdout against those
anchors. Token cost is UTF-8 stdout bytes divided by four - the same
approximation both CLIs document for their text budgets. A `broad` question
that misses first uses the tool's documented continuation: Compass follows the
`--cursor` ledger, Graphify re-runs with a four-times larger budget. Both the
first-page cost and the total cost of the reviewed workflow are recorded.

## What the evaluation changed

Four defects surfaced by the suite were fixed in this change:

- An ambiguous typed lookup (`callers`, `callees`, `impact`) returned an empty
  result. It now publishes the exact-name candidates with their IDs, kinds, and
  source anchors, and the agent view emits `retry_with_exact_id` actions so the
  next request can disambiguate in one step.
- Ambiguous `path` endpoints reported only node IDs in a terminal error. The
  error now lists each candidate's label, file, location, and ID.
- A bounded discovery page whose first entry exceeded `--text-budget` failed
  with an empty response. Oversized entries are now truncated with an explicit
  marker so pagination still advances.
- The typed agent view listed a target twice when a real self-edge existed
  (for example `ExecuteC` calling `ExecuteC` through `Root()`). Primary results
  are deduplicated like the discovery view already did.

`compass explain --source` was added so the explain path can return the
declaration text itself. The excerpt is read below `--root`, bounded by
`--max-source-bytes` (default 4 KiB), and verified against the recorded symbol
digest before it is printed; a rewritten file fails closed with
`SOURCE unavailable: ... does not match ...`.

## Findings and follow-up

1. **Natural-query seeding is the largest remaining correctness gap.** The
   three failed `broad` rows did not fail on pagination or bounds; they seeded
   the wrong symbols. Term selection should weight rare, symbol-shaped terms
   over generic surface nouns, and must keep the reviewed relevance
   qualification corpus green.
2. **Verified answers cost more tokens than unverified ones.** Compass spends
   3.6x Graphify's median tokens per answered question, driven by `callers`
   (6.3k median tokens for 24 exact usage edges) and `explain_source` (1.4k
   median tokens including the declaration text). The medians are not
   like-for-like: Compass answered 12 rows Graphify failed, and per kind where
   both tools passed, Compass is 1.8x more expensive on `explain`, 3.8x on
   `path`, 3.3x on `ambiguity`, and cheaper on `broad` (390 versus 419 tokens).
   The extra cost buys stable IDs, digests, and source anchors that Graphify's
   rows do not carry.
3. **No-match answers are 55x Graphify's cost** (389 versus 7 median tokens).
   The agent view keeps its full identity and omission envelope even when the
   answer is "nothing matched". A compact no-match projection is the cheapest
   remaining token win.
4. **Graphify still publishes nothing for some questions.** Its `affected`
   command returned no callers for four of five repositories, and its `path`
   command failed on Python, Java, and TypeScript file endpoints. These are
   capability gaps, not recall differences.

## Reproduction boundary

The run used Apple silicon macOS with the release Compass binary at
`/Volumes/Workspace/crabbuild-target/compass-9986/release/compass` and
Graphify `0.9.36` from `~/.local/bin/graphify`. Corpus revisions are pinned in
`benchmarks/agent_query/suite.toml`; the runner refuses a checkout whose HEAD
differs. Raw evidence - per-question stdout/stderr, run metadata, graph
digests, and the generated `REPORT.md` - lives under
`/Volumes/Workspace/CrabData/compass-evaluations/agent-query-5repo-fresh/runs/`.

This is a focused five-repository sample with text-anchor judging. It is not a
population-wide accuracy estimate.
