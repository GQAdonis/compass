# Execution contract

Backend: native-tool, using the Codex goal and plan plus canonical KBD tracking.
The explicit user plan in `plan.md` supersedes the old waypoint for this work.
Production code is in the optional-Surreal worktree; development and integration
artifacts stay checkout-local. Coordinate host-wide Cargo/rustc ownership with
the Universal Agent Runtime task before every verification wave.

Current work: Phase 2 integration verification, followed by deterministic
refinement and independent fresh-context review. Repairs are batched before
complete integration suites. Implementation, evidence, certification, fork PR
merge, upstream PR submission, and final installation are independent states.
Do not infer completion of one from another.

## Operator priority override — immediate deployment

The operator explicitly changed the order: build/install the current SurrealKV
release globally and check it in first; afterward run only verification for
changed surfaces. Do not start another broad workspace or all-engine suite.
The completed default workspace and subsequent complete default CLI passes
remain evidence. The CLI restore correction still needs its Surreal-enabled
integration check. Optional RocksDB final rerun, broad Clippy/qualification,
refinement certification, and independent review are deferred, not passed.
Phase 3 remains unimplemented; this urgent Phase 2 installation is not a claim
that the original three-phase plan is complete.
