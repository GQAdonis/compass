# Agent query evaluation

`benchmarks/agent-query` measures how well Compass answers the agent questions
in `suite.toml` compared with Graphify on the same pinned checkouts. It is
developer-side tooling: Compass never runs it, and it never installs Graphify.

The suite covers five real repositories in five languages:

| Repository | Language | Focus |
| --- | --- | --- |
| `spf13/cobra` | Go | CLI command resolution and execution |
| `pallets/flask` | Python | request dispatch into views |
| `google/gson` | Java | overloaded serialization APIs |
| `colinhacks/zod` | TypeScript | schema parse and safe-parse helpers |
| `tokio-rs/axum` | Rust | routing and service dispatch |

Each repository contributes source-reviewed questions across seven kinds:
`explain`, `callers`, `path`, `file_path`, `ambiguity`, `negative`, and
`broad`. Every question declares the exact per-tool argument vector, the
expected outcome, and the file, line, or symbol anchors the reviewer read in
the pinned checkout. Every repository also declares graph anchors that both
graphs must contain as source-backed nodes.

## Run

```bash
python3 benchmarks/agent-query/runner.py doctor \
  --compass-binary /path/to/compass \
  --graphify-binary /path/to/graphify \
  --source cobra=/Volumes/Workspace/Github/spf13/cobra \
  --source flask=/Volumes/Workspace/Github/pallets/flask \
  --source gson=/Volumes/Workspace/Github/google/gson \
  --source zod=/Volumes/Workspace/Github/colinhacks/zod \
  --source axum=/Volumes/Workspace/Github/tokio-rs/axum

python3 benchmarks/agent-query/runner.py run \
  --workspace /Volumes/Workspace/CrabData/compass-evaluations/agent-query \
  --compass-binary /Volumes/Workspace/crabbuild-target/compass/release/compass \
  --graphify-binary "$(command -v graphify)" \
  --source cobra=/Volumes/Workspace/Github/spf13/cobra \
  --source flask=/Volumes/Workspace/Github/pallets/flask \
  --source gson=/Volumes/Workspace/Github/google/gson \
  --source zod=/Volumes/Workspace/Github/colinhacks/zod \
  --source axum=/Volumes/Workspace/Github/tokio-rs/axum
```

`doctor` fails when a checkout is not at the suite's pinned commit. `run`
builds `compass extract --code-only --no-viz --store sqlite` and
`graphify extract --code-only` once per repository under
`WORKSPACE/artifacts`, then writes `run.json` and `REPORT.md` under
`WORKSPACE/runs/<run-id>/`.

## Metrics

- **Correctness**: bounded stdout is judged against the suite's anchors. A
  `negative` question passes only with an explicit no-match signal, and a
  `pick_list` question passes only when enough distinct candidates are shown.
  An `answer` row requires every `required` anchor and, when it also declares
  `required_one_of`/`min_one_of`, at least that many of those alternatives, so
  a question with several source-reviewed answers (for example the Zod
  validation family) accepts any reviewed one without weakening the row.
- **Tokens**: UTF-8 stdout bytes divided by four, the same approximation both
  CLIs document for their text budgets. `run.json` records the first-page cost
  and the total cost of the reviewed workflow.
- **Follow-ups**: a `broad` question that misses the oracle first runs the
  documented continuation - Compass uses the `--cursor` ledger, Graphify
  re-runs with a four-times larger budget - up to `max_follow_ups`, so
  pagination and budget guessing are priced rather than hidden.
- **Latency**: wall-clock milliseconds per tool invocation, including
  follow-ups.
- **Graph quality**: node and edge counts, source-backed node ratio, dangling
  edges, duplicate IDs, and how many reviewed anchors the graph contains.

## Limits

Anchor matching is a deterministic text-recall proxy over bounded output, not
an independent precision oracle. The suite is a focused five-repository
sample; it does not estimate population-wide accuracy. Graphify prints an
installation warning on stderr, which `run.json` records separately and the
token metric excludes.
