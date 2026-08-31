# Execution contract

Backend: native-tool, using the Codex goal and plan plus canonical KBD tracking.
The explicit user plan in `plan.md` supersedes the old waypoint for this work.
Production code is in the optional-Surreal worktree; development and integration
artifacts stay checkout-local. Coordinate host-wide Cargo/rustc ownership with
the Universal Agent Runtime task before every verification wave.

Current work: Phase 2 dual-mode release installed and reduced verification
recorded; fork-main/upstream delivery and Phase 3 remain. Repairs are batched
before complete affected integration suites. Implementation, evidence, certification, fork PR
merge, upstream PR submission, and final installation are independent states.
Do not infer completion of one from another.

## Operator priority override — immediate deployment

The operator explicitly changed the order: build/install the current SurrealKV
and standalone-server
release globally and check it in first; afterward run only verification for
changed surfaces. Do not start another broad workspace or all-engine suite.
The completed default workspace and subsequent complete default CLI passes
remain evidence. CLI restore and both deployment modes passed the final
30-check installed suite; the two changed configuration targets passed all
11 tests. Formatting and feature isolation passed. Earlier timeout attempts
remain recorded with an unconfirmed cause. Optional RocksDB final rerun, broad Clippy/qualification,
refinement certification, and independent review are deferred, not passed.
Phase 3 remains unimplemented; this urgent Phase 2 installation is not a claim
that the original three-phase plan is complete.
