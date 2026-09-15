# Pull request delivery — 2026-08-30

The operator requested commit, push, and open PRs for work not already on main.
Both product branches were already committed and pushed. New local runtime
state and the generated repository graph were preserved and excluded.

| Phase | Head branch | Target | PR | State |
| --- | --- | --- | --- | --- |
| 1 — actual graph-size cap | codex/fix-preflight-size-gate-fork | GQAdonis/compass:main | [#2](https://github.com/GQAdonis/compass/pull/2) | Merged previously |
| 2 — embedded and standalone Surreal | codex/optional-surreal-graph-backend | GQAdonis/compass:main | [#3](https://github.com/GQAdonis/compass/pull/3) | Open, not merged |
| 3 — legacy artifact boundary | codex/legacy-artifact-boundary | GQAdonis/compass:main | [#4](https://github.com/GQAdonis/compass/pull/4) | Open, not merged; depends on #3 |

Merge #3 before #4. Both explicitly target the fork's main. Phase 3 descends
from Phase 2; its current main-based PR comparison includes Phase 2 until that
ancestor lands. Its phase-only comparison is
`codex/optional-surreal-graph-backend...codex/legacy-artifact-boundary`.
The PR descriptions cross-link this dependency and distinguish reduced passing
checks from deferred certification. No PR merge was performed in this turn.

The installed dual-mode release remains unchanged. This turn adds only delivery
documentation; no Cargo/rustc command, new compilation, or test wave was run.

## Clean upstream submission remains pending

Existing upstream [#307](https://github.com/crabbuild/compass/pull/307) covers
Phase 1; [#212](https://github.com/crabbuild/compass/pull/212) covers the earlier
hardcoded build-volume fix. No duplicate upstream PR was opened.

The current feature branches include fork-only history and must not be submitted
wholesale to crabbuild/compass:main. A read-only patch applicability check used
a temporary detached worktree based on the upstream-compatible Surreal foundation
and qualification commits, then checked the integration delta from 069fb879 to
a7d715f7, excluding KBD/refiner state. It failed on nine files:

- CHANGELOG.md
- Cargo.lock
- crates/compass-cli/src/lib.rs
- crates/compass-mcp/src/code_query.rs
- crates/compass-mcp/src/lib.rs
- crates/compass-mcp/src/transport.rs
- crates/compass-mcp/tests/code_query_tools.rs
- crates/compass-model/src/lib.rs
- crates/compass-query/src/code_query.rs

The fork's MCP/dependency and related integration changes therefore require a
deliberate upstream-only port and changed-surface verification. No partially
applied patch was committed or pushed, no upstream branch was changed, and no
claim of upstream-ready compatibility is made. Phase 3's native Surreal guards
must follow that clean Phase 2 port rather than reference absent upstream APIs.
