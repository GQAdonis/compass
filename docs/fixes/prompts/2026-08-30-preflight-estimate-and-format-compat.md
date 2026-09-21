# Fix prompt — preflight estimate, format compatibility, orphaned Surreal crate

- **Raised:** 2026-08-30
- **Against:** compass `0.3.23`, branch `docs/claude-md`, HEAD `2d385dee`
- **Raised by:** the graph-explorer team (downstream consumer)
- **Status:** open

Defect 1 is a hard blocker for the downstream consumer — they cannot re-index
their corpus at all. Defects 2 and 3 are real but have workarounds.

Every claim below was reproduced against the installed `0.3.23` binary. Verify
them yourself before changing code; do not take this document on trust.

---

## Defect 1 (BLOCKER) — the preflight size estimate is ~46× too high

`compass update .` at the root of a five-estate tree fails in ~2 seconds:

```
error: snapshot limit exceeded: canonical graph exceeds the 2147483648-byte
limit; retry or rebuild with a smaller scope using --exclude <pattern> or
persistent patterns in .compassignore, or explicitly raise the bound with
COMPASS_MAX_GRAPH_BYTES=<bytes|NMB|NGB>
```

### Reproduce

```bash
cd /Users/gqadonis/Projects/know-me/randy-project/asset-intertech/active
compass update . --force --store json --out /tmp/probe
```

### Evidence that it is a false positive

Each estate indexes successfully **on its own**:

| Estate | Files | Resulting `graph.json` |
|---|---:|---:|
| ara-scanworks-ui | 234 | 4 MB |
| ara-pm | 438 | 14 MB |
| captivebrowser-133 | 1,348 | 395 MB |
| venus | 623 | 7 MB |
| solaris-platform | 2,309 | 86 MB |
| **Sum** | **4,952** | **505 MB** |

The limit is 2,048 MB. The parts sum to 505 MB; the whole is refused as
exceeding 2,048 MB. It fails in ~2 s — far too fast to have walked 10,613
files — so it is failing on an **estimate**, not a measurement.

### Root cause

`crates/compass-core/src/pipeline.rs`:

```
line   98:  const PREFLIGHT_GRAPH_BYTES_PER_SOURCE_BYTE: u64 = 60;
line   99:  const PREFLIGHT_PARTIAL_FILE_BYTES: u64 = 512;
line 4372:  source_estimate = metadata.len()
                .saturating_mul(PREFLIGHT_GRAPH_BYTES_PER_SOURCE_BYTE)
line 4387:  if estimated > maximum {
                return Err(CoreError::Snapshot(
                    SnapshotError::canonical_graph_too_large(maximum)))
            }
```

394 MB of source × 60 = **23.6 GB estimated** against **505 MB actual**. The
multiplier is wrong by roughly 46×, so compass refuses work it could complete.

### Required

1. **Derive the real ratio from measured corpora, not a guess.** The five
   estates above are ground truth. Measure `graph bytes / source bytes` per
   estate and report the distribution — it varies enormously:
   captivebrowser-133 is 395 MB from 89 MB source (≈4.4×) while venus is 7 MB
   from 139 MB (≈0.05×). **A single linear multiplier may be the wrong model
   entirely.** If it is, say so rather than retuning the constant.
2. **A preflight that can be wrong must not be fatal on its own.** Either make
   it a warning that proceeds and enforces the true limit at write time, or
   make it fatal only when the estimate exceeds the cap by more than the
   measured error bound. State which you chose and why.
3. **Regression test:** a fixture whose *estimate* exceeds the cap but whose
   *actual* output does not must index successfully.
4. `compass update` at the root of the five-estate tree must succeed.

---

## Defect 2 (BLOCKER for existing artifacts) — 0.3.23 cannot read 0.3.6 graphs

```
compass callers <nodeId> --graph <0.3.6-built graph.json> --format json
→ error: store_graph_snapshot_failed: snapshot capability unavailable:
  edge_id_ordered_adjacency_unavailable; rebuild the graph store with this
  Compass version
```

Every artifact in a 47-repo corpus (`~/.compass/global-manifest.json`, indexed
2026-08-26) is unreadable. There is no migration path and no version
precondition check — the failure surfaces at *query* time, deep inside a
workflow, rather than at load.

### Required

1. Detect the old format **at load time** and fail with a message naming the
   artifact's version, the required version, and the exact rebuild command.
2. Decide and document: migrate in place, or require a rebuild? If rebuild,
   confirm `compass update --force` is sufficient and say so in the error text.
3. Regression test against a 0.3.6-era fixture.

---

## Defect 3 (DESIGN GAP, not a crash) — `compass-graphdb-surreal` is orphaned

```
grep -rln 'compass-graphdb-surreal' crates/*/Cargo.toml
  → crates/compass-graphdb-surreal/Cargo.toml   (itself only)
```

No crate depends on it, `compass-cli` included. The CLI exposes only:

```
compass update --store <json|sqlite>
compass search --engine <default|json|store>
```

A downstream consumer was told this fork "supports SurrealDB for the graph
data (the original only supports JSON files and sqlite)" and planned a storage
architecture around it. The crate exists; the capability is unreachable.

### Required

State which is true:

- **(a)** in progress, wiring is planned;
- **(b)** consumed out-of-process (e.g. by `compass serve`) in a way a manifest
  grep would not reveal;
- **(c)** landed on a branch not merged into `docs/claude-md`.

If **(a)**, wire it: `--store surreal` and `--engine surreal`, with a
connection option, documented in `docs/reference/commands.md`.

---

## Constraints

- **Do not raise `COMPASS_MAX_GRAPH_BYTES` as the fix for Defect 1.** The cap
  is reasonable; the estimate is wrong. Raising it hides the bug.
- Defect 1 is the only true blocker. Ship it first and separately if that is
  faster.
- Every fix needs a test that **fails before** the change and **passes after**.
- Report the measured ratios from Defect 1 step 1 even if the fix turns out
  simple — a downstream team is sizing hardware against them.

---

## Downstream note

The consumer added a `.compassignore` to their tree excluding `compass-out/`
and `fetch-*.log`. It did **not** fix Defect 1 (the estimate fires before the
walk), but it is correct on its own merits: the 0.3.6 run excluded those paths
implicitly — its manifest holds 0 entries under `compass-out/` — and that
exclusion was never written down.
