# Phase 3 — installation and reduced verification

Date: 2026-08-30. Product commit: `e5bb7e4a`.
Status: installed and pushed; all reduced integration checks passed.

All production code and documentation were implemented before compilation.
The release was installed globally and the commit pushed to
`GQAdonis/compass:codex/legacy-artifact-boundary` before integration tests ran.
Cargo commands remained serialized under the coordinated machine-wide slot.

## Installation

`cargo install --path crates/compass-cli --bin compass --features
surreal-surrealkv,surreal-remote --profile release --locked --force` passed in
9m05s. Explicit CARGO_TARGET_DIR selected this worktree's target directory;
the existing global configuration retains intermediate artifacts under its
workspace-hashed Cargo build cache. No shared target override or cleanup ran.

Installed executable and local release artifact SHA-256:

`517eeec672ed9e78b397129e889218cb1d0466f99c1288d97b20ad8ffaafc3be`

The Cargo-bin executable and existing local-bin/Homebrew symlinks expose the
same release. Capabilities report Compass 0.3.23, JSON, SQLite, SurrealKV and
remote Surreal enabled. Embedded RocksDB is not compiled into this installation.

## Completed checks

- `cargo test -p compass-model --test code_graph_loading --test legacy_artifacts
  --profile release --locked`: **13 passed**, zero failures. Both complete
  integration targets ran. Covers the authentic old graph at all nine public
  loaders, poisoned record tails, bounded prefix reads, exact version floor,
  current cache reuse, old-artifact cache rejection, validated provenance,
  missing/invalid provenance, symlink rejection, and escaped recovery commands.
- `COMPASS_TEST_BINARY=<installed compass> node scripts/verify_legacy_artifacts.mjs`:
  **14 passed**. Seven typed CLI operations, native CompassQL entry, real MCP
  stdio rejection, missing-root guidance, the exact forced rebuild over the
  authentic artifact, and post-rebuild default typed/CompassQL/MCP queries.
- `cargo test -p compass-query --test legacy_artifacts --features
  surreal-surrealkv,surreal-remote --locked`: **4 passed**, zero failures in the
  complete final target. Covers load-time JSON rejection, native SQLite and
  Surreal metadata rejection without graph.json, cached Surreal generation
  repinning, preservation of a prior valid cached generation after rejection,
  reopening the old Surreal generation, and verified-document cache guards.
- `cargo fmt --all -- --check`: **PASS**.
- `sh scripts/check_product_boundary.sh`: **PASS**.
- `git diff --check`: **PASS**.
- `compass update . --store sqlite`: **completed**, 180.66 seconds, 2,794 files
  (207 extracted / 2,587 cached), 161,667 nodes and 248,859 edges. The refresh
  explicitly reported a partial graph: 4 nodes and 3 edges omitted, zero
  identity collisions quarantined. This warning is not hidden as full coverage.

The authentic fixture was produced by the official 0.3.6 release; its archive
digest and generation command are in the fixture README. Only a trailing
newline was added to the graph for check-in. The installed-product fixture is
retained in a uniquely named system temporary directory, not in tracked source.

## Pending and deliberately deferred

- No broad workspace, TCK, all-engine, or Clippy rerun: the operator explicitly
  reduced verification to changed surfaces after urgent deployment.
- Independent refinement/review certification remains pending, not passed.
- Phase 2 and Phase 3 fork-main merge and clean upstream PR delivery remain
  separate unfinished steps. Pushing a branch is not merging it into main.

## Retained failed attempt

The first complete query target passed 3 tests and failed 1 during fixture
setup: the verified-document cache fixture supplied `same-identity`, while the
public API requires a 64-character hexadecimal digest. The fixture now computes
the actual SHA-256 of its modern graph. No production code or assertion was
weakened. The complete four-test target then passed, including the cache-bypass
regression. No additional release rebuild was needed for this test-only change.
