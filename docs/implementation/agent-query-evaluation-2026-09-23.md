# Agent query evaluation: five repositories, five languages

## Result

Compass answers the reviewed agent questions more accurately and publishes a
better-anchored graph than Graphify on this suite, but it still spends more
tokens per answered question. The evaluation ran on 2026-09-23 against Compass
commit `3fd246dc` plus the fixes in this change, and Graphify `0.9.36`.

| Metric | Compass | Graphify |
| --- | ---: | ---: |
| Source-reviewed answers passed | 40/43 | 19/43 |
| Reviewed graph anchors present | 15/15 | 13/15 |
| Source-backed nodes | 100% | 86% |
| Median tokens per answered question | 475 | 278 |
| Broad natural questions answered | 2/5 | 5/5 |
| Paged caller questions answered | 2/2 | 0/2 |
| Compact caller questions answered | 2/2 | 0/2 |

Compass passed every `callers`, `explain_source`, `file_path`, and `negative`
row, plus every `path` and both `paged_callers` rows; Graphify passed none of
the `callers`, `paged_callers`, `explain_source`, or `file_path` rows and only
two of four `path` rows. The
remaining Compass failures are natural-language `broad` questions on Gson,
Zod, and Axum, where discovery seeded surface terms such as `json`, `object`,
`input`, and `request` instead of the domain symbols `toJson`, `JsonWriter`,
`safeParse`, and `Router`. The Zod row's oracle requires the `parse` family
specifically, so it is the weakest of the three judgments.

## Design

`benchmarks/agent_query/suite.toml` pins one checkout per language:
`spf13/cobra` (Go), `pallets/flask` (Python), `google/gson` (Java),
`colinhacks/zod` (TypeScript), and `tokio-rs/axum` (Rust). Each repository
contributes questions in ten kinds: `explain`, `explain_source`, `callers`,
`paged_callers`, `brief_callers`, `path`, `file_path`, `ambiguity`, `negative`,
and `broad`. Every question
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

Path-shaped oracles also reject a tool's own failure text. That check was added
after the first replay showed Graphify "passing" file-path rows by printing
`No directed path found between A and B`: both endpoint names appeared in the
answer, so an anchor-only judge scored a failure as a pass. The corrected
oracle fails any `path` or `file_path` row whose output contains
`NO PATH FOUND` or `No directed path found`.

The `paged_callers` questions ask for the same caller sets as `callers` but
consume the paged text output: a 400-token `--text-budget` and the
`compass.query.agent-text-page/1` cursor from the page footer. Compass answered
both in 764 (Cobra) and 377 (Axum) tokens by following the cursor, against
6,256 and 6,758 tokens for the same answers through the single-shot agent view
and no answer at all from Graphify. Paging therefore turns the most expensive
reviewed question class into one of the cheapest.

The `brief_callers` questions ask for the same caller sets through
`--format agent-json --brief`, the compact `compass.query.agent-view.brief/1`
projection that keeps status, caveats, source-located entities,
relationships, and next actions while dropping audit-only digests, record IDs,
and per-edge evidence layers. Compass answered both in 1,651 (Cobra) and 2,046
(Axum) tokens, 3.8x and 3.3x cheaper than the full projection. Median tokens
per caller answer therefore move from 6,256 (full) to 1,848 (brief) to 570
(paged) with the same reviewed anchors.

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
- Typed text output had no continuation: a capped result set could only be
  re-run with a guessed larger limit. `ask`, `search`, `callers`, `callees`,
  `impact`, `explore`, and `node` now accept `--text-budget` and `--cursor`;
  each page ends with a checksummed `compass.query.agent-text-page/1` cursor
  that continues the same deterministic ledger, and the ledger is built from
  the raw query response so paging reaches records the compact view omits.
- `compass store validate` reported `valid: true` for an artifact whose
  published graph contained records that every strict reader rejects
  (34 non-call self-loops on the Zod corpus). It now re-materializes the
  snapshot and applies the strict `compass.graph/1` validation, so the
  historical artifact fails with the offending edge IDs while
  `compass store status` keeps the cheaper digest-and-integrity check.
- A TypeScript project whose configuration other projects `extends` lost all of
  its own `paths` aliases: the shared config was excluded from alias selection
  even though it declared its own `include`. On `rivet-dev/actors/frontend`
  (802 aliased imports) `<root>/tsconfig.json` is extended by
  `apps/inspector/tsconfig.json`, and before the fix the graph had zero
  incoming edges to `src/lib/errors.ts` from `@/lib/errors` importers. The
  config is now selectable when it declares its own `files`/`include`, a
  same-directory extending project still wins over its base, and the corpus
  gained 3 module imports plus 11 symbol-level usages.
- File-shaped path input could still fail after that fix because TypeScript
  publishes an isolated metadata `file` node next to the `module` node that
  carries the file's contents. `compass path <file> <file>` now resolves an
  isolated file node to the single module that owns the same source file and
  shows both names, so `compass path src/app.tsx src/lib/errors.ts` reports the
  one-hop `app --imports--> errors` path on that corpus.
- Typed queries had no wall-clock bound: the slowest reviewed row (Axum
  `callers`) took 8.3 seconds in release and the debug build took about a
  minute with no feedback. Every typed command now accepts `--timeout-ms`
  (default 60000, maximum 600000), armed once per command and checked between
  resolution, candidate, relationship, impact, and path-expansion steps. An
  expired deadline fails with `code_query_timeout` and an actionable hint; the
  focused replay shows no row hitting the default, and the engine-level test
  proves an expired deadline fails closed.

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
   qualification corpus green. One bounded step landed: `route` is no longer
   discarded as a generic relational term, so the Axum question now seeds route
   symbols and its answer names `Router`; it still misses `MethodRouter`, and
   Gson still prefers `JsonObject` over the exact-name `serialize` match, so
   term weighting by document frequency remains the follow-up.

   A bounded follow-up attempt was made and reverted. It probed each matched
   concept through the bounded name index, reserved one seed slot for the most
   specific uncovered concept, and kept the reviewed relevance corpus green.
   The three broad rows still failed: the Gson answer gained
   `JsonSerializationContext::serialize` but never reached `toJson` or
   `JsonWriter`, Zod's reserved slot promoted a project-name match (`zod`), and
   Axum's promoted a truncated token (`incom`). Specificity by name-index
   frequency alone is therefore not sufficient; the next attempt needs
   project-name and partial-token awareness, and possibly deeper expansion from
   the specific verb rather than seed reordering. The reverted state keeps the
   verified behavior, and the 500-query relevance qualification still passes.
2. **Verified answers cost more tokens than unverified ones on the
   single-shot paths.** Compass spends 3.6x Graphify's median tokens per
   answered question across the whole suite, driven by `callers` (6.3k median
   tokens for 24 exact usage edges) and `explain_source` (1.4k median tokens
   including the declaration text). The medians are not
   like-for-like: Compass answered 14 rows Graphify failed, the medians mix
   paged and single-shot rows, and per kind where
   both tools passed, Compass is 1.8x more expensive on `explain`, 3.8x on
   `path`, 3.3x on `ambiguity`, and cheaper on `broad` (390 versus 419 tokens).
   The extra cost buys stable IDs, digests, and source anchors that Graphify's
   rows do not carry. The paged caller rows show the intended direction:
   570 median tokens where the single-shot agent view needed 6.3k.
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
`/Volumes/Workspace/CrabData/compass-evaluations/agent-query-final2/runs/20260923T102255Z/`.

The store self-check was verified against the historical Zod artifact at
`/Volumes/Workspace/CrabData/compass-evaluations/agent-query-5repo-20260923/zod/compass/compass-out`,
which was written by an older publisher: `compass store validate` previously
reported `valid: true` and now fails with the 34 offending self-loop edge IDs.

This is a focused five-repository sample with text-anchor judging. It is not a
population-wide accuracy estimate.
