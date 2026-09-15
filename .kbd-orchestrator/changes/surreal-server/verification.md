# Standalone and embedded SurrealDB — deployment evidence

Date: 2026-08-30 (local). Product commits: `dee88248`, `90a0a4c2`.

The optimized release was installed globally and the product commits were
pushed to `GQAdonis/compass:codex/optional-surreal-graph-backend` before the
changed-path verification wave. The installed executable and local release
artifact have the same SHA-256:

`ca060babf23ccedff56d3934b67b6e2a8db65297933b9aa2b3d0236d03228c00`

Capabilities confirm `surreal_store`, `surreal_cql`, `surreal_remote`,
`surreal_yaml_config`, and embedded `surrealkv`. Optional embedded RocksDB is
not included in this installation. The existing standalone server uses its
own RocksDB storage, accessed only through the network endpoint.

## Exact completed checks

- Release install: `cargo install --path crates/compass-cli --bin compass
  --features surreal-surrealkv,surreal-remote --profile release --locked --force`
  with the explicit checkout-local target and existing global Cargo root.
- `cargo test -p compass-files --test surreal_configuration --test project_scope
  --locked`: **11 passed**, zero failures. Both complete integration targets
  ran; no unit harness or test-name filters were used.
- `sh scripts/check_surreal_feature_isolation.sh`: **PASS**, default CLI, MCP,
  core, query, and projection dependency closures remain SurrealDB-free.
- `cargo fmt --all -- --check`: **PASS**.
- `scripts/verify_surreal_server.mjs`, executed against the installed binary
  and the operator's existing loopback launch-agent server: **30 passed**.
  Covers YAML publication, all seven typed operations, native CompassQL,
  default selection, unchanged publication, YAML/environment/flag precedence,
  credential-selector precedence, missing-secret and bad-auth failure,
  target mismatch rejection, explicit storage conflict rejection, portable
  backup, remote restore, embedded restore, validation, no-JSON queries,
  real MCP stdio, flags-first watch from an unrelated working directory,
  and reopening the watched generation.
- `compass update . --store sqlite`: completed in 117.97 seconds; 2,784 files,
  442 extracted / 2,342 cached. The refresh explicitly reported a partial graph:
  4 nodes and 3 edges omitted, no identity collisions quarantined.
- `git diff --check`: **PASS**.

## Failed attempts retained

The initial combined release passed the earlier 26-check server suite. After
the small configuration repair, two complete-suite attempts hit the harness's
45-second subprocess deadline, first on `impact`, then on `init`. An immediate
retry of the same impact query completed in 0.173 seconds; a force rebuild of
the interrupted fixture completed in 15.171 seconds. Server health returned
HTTP 200 in about 2 milliseconds. After compilation and the repository graph
refresh finished, the unchanged full 30-check suite passed. The timeout cause
was not established; neither the production deadlines nor harness deadlines
were increased, and no retry was hidden inside a successful test assertion.

Only freshly named databases in `compass_validation` were written. No launch
agent was restarted or edited, no existing service database was modified, and
credentials were passed in subprocess environment variables without printing
or persisting them. Disposable fixtures/databases were retained for inspection.

## Remaining boundaries

This is the operator-requested reduced verification and immediate installation,
not full certification. Broad workspace/TCK/Clippy/all-engine reruns and fresh
independent certification remain deferred. Fork-main PR delivery, the clean
upstream PR, and Phase 3's legacy-artifact boundary remain unfinished.
