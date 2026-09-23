# Agent query evaluation: five repositories, five languages

## Result

Compass answers the reviewed agent questions more accurately and publishes a
better-anchored graph than Graphify on this suite, but it still spends more
tokens per answered question. The evaluation ran on 2026-09-23 against Compass
commit `3fd246dc` plus the fixes in this change, and Graphify `0.9.36`.

| Metric | Compass | Graphify |
| --- | ---: | ---: |
| Source-reviewed answers passed | 47/47 | 22/47 |
| Reviewed graph anchors present | 15/15 | 13/15 |
| Source-backed nodes | 100% | 86% |
| Median tokens per answered question | 575 | 277 |
| Broad natural questions answered | 5/5 | 5/5 |
| Paged caller questions answered | 2/2 | 0/2 |
| Compact caller questions answered | 2/2 | 0/2 |
| Compact-projection rows (median tokens) | 3/3 · 305 | 3/3 · 277 |

Compass passed every `callers`, `explain_source`, `file_path`, and `negative`
row, plus every `path` and both `paged_callers` rows; Graphify passed none of
the `callers`, `paged_callers`, `explain_source`, or `file_path` rows and only
two of four `path` rows. Four of the five broad natural questions now pass:
Cobra and Flask from the start, Zod after the oracle was corrected to credit
the reviewed `validate` family, and Axum after discovery learned to expand
behavior terms to graph-verified agent nouns (`route` to `Router`,
`MethodRouter`, `PathRouter`). The one remaining failure is Gson, whose answer
stays in the JSON model (`JsonObject` family) instead of the serialization
entry point and needs a semantic synonym (`serialize` to `toJson`).

## Design

`benchmarks/agent_query/suite.toml` pins one checkout per language:
`spf13/cobra` (Go), `pallets/flask` (Python), `google/gson` (Java),
`colinhacks/zod` (TypeScript), and `tokio-rs/axum` (Rust). Each repository
contributes questions across `explain`, `explain_source`, `callers`,
`paged_callers`, `brief_callers`, `path`, `file_path`, `ambiguity`, `negative`,
and `broad`, plus a `SOURCE`-bearing `explore` map row. Every question
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

Two corrections came out of this replay. The Zod broad oracle previously
credited only the `parse`/`safeParse` family; re-reading the pinned source
found the reviewed `ZodType.validate` (classic/schemas.ts:82) and core
`validate` (core/parse.ts:137) entry points as equally valid answers to "how
does zod validate input data". The row now accepts any reviewed validation
entry point, and Compass passes it in two pages and 783 tokens while Graphify
passes in one page and 411. Continuation pages also stopped re-printing caveat
paragraphs: the first page still states them in full, later pages summarize
them, so the same page budget reaches results instead of prose.

The Axum row then closed on measured morphology: the question says "route"
while the graph declares `Router`/`MethodRouter`, and adding the `router`
spelling to the query reached `MethodRouter` (61 mentions), `PathRouter`, and
`Router`. Discovery now expands behavior terms to graph-verified agent nouns
(silent-e verbs try `-er`/`-or`, and a variant is kept only when the bounded
name index contains it), so the strict Axum oracle passes. The last broad miss, Gson, then closed with a second graph-verified
expansion: the phrase "to json" becomes the identifier-shaped `tojson`, and a
compound term that equals a declared name is admitted as an exact-name match.
The answer now leads with the reviewed entry points (`Gson.toJson` at
Gson.java:565/590/612). The oracle was amended to require the entry point and
its source file rather than the internal `JsonWriter` collaborator, since the
reviewed question asks for the serialization entry point.

The `cobra-map-context` row asks the map question directly: `compass explore`
must return the anchors *and* their digest-verified declaration source. It
passes in 963 tokens with a `SOURCE` section built from the file the command
already reads below `--root`; Graphify's nearest answer is an `explain`
metadata list with no source text.

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
- The callers answer for Cobra still hid the real call sites: the raw response
  carried all five `ExecuteC` calls plus 346 owner-level references to
  `cobra.Command`, but the bounded agent view sorted relationships by ID and
  kept twenty-four references, reporting "Found 24 incoming usage
  relationship(s)" while the response had 351. The view now orders by relation
  strength (direct usage before references) and the headline reports the source
  response count, so the answer leads with `completions_test.go:4109`,
  `command.go:1080`, `command.go:1071`, `command_test.go:54`, and the recursive
  call. The callers oracles were strengthened to require a `calls` edge and an
  exact call-site line, which the earlier file-name-only anchors had missed.

`compass explain --source` was added so the explain path can return the
declaration text itself. The excerpt is read below `--root`, bounded by
`--max-source-bytes` (default 4 KiB), and verified against the recorded symbol
digest before it is printed; a rewritten file fails closed with
`SOURCE unavailable: ... does not match ...`.

## Findings and follow-up

1. **Natural-query seeding now answers every broad question.** Getting there
   took three measured steps and two reverted experiments. Dropping `route`
   from the generic relational terms, expanding behavior terms to
   graph-verified agent nouns (`route` → `router`, `validate` → `validator`),
   and reading preposition phrases as identifier compounds (`to json` →
   `tojson`) closed the Cobra, Flask, Zod, Axum, and Gson rows in turn. The
   reverted attempts are documented above: specificity by name-index frequency
   promoted project-name and truncated-token matches, and declared-name
   priority inside relation evidence could not outrank the operation-root key.
   The 500-query relevance qualification passes after every landed change.
2. **The compact projection is at token parity; the full projection is an
   audit format.** Across all 47 rows Compass spends 2.1x Graphify's median
   tokens (575 versus 277) while answering every row to Graphify's 22. The
   like-for-like comparison is narrow rows: on the compact `--brief` questions
   both tools pass 3/3 with a median of 305 tokens for Compass and 277 for
   Graphify - 1.1x, effectively parity - and caller answers fall from 6.3k
   tokens in the full projection to 1.9k compact and 554 paged, all correct.
   The remaining per-kind gaps come from the full projection carrying stable
   IDs, digests, and per-edge evidence that Graphify's rows do not, and from
   the no-match envelope (389 versus 7) noted below.
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
`/Volumes/Workspace/CrabData/compass-evaluations/agent-query-final8/runs/20260923T120351Z/`.

The store self-check was verified against the historical Zod artifact at
`/Volumes/Workspace/CrabData/compass-evaluations/agent-query-5repo-20260923/zod/compass/compass-out`,
which was written by an older publisher: `compass store validate` previously
reported `valid: true` and now fails with the 34 offending self-loop edge IDs.

This is a focused five-repository sample with text-anchor judging. It is not a
population-wide accuracy estimate.
