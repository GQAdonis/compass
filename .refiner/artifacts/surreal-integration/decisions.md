# Surreal integration refinement decisions

## Iteration 1

- Continue until all blocking repository constraints have current evidence.
- Preserve the phase-first/integration-only doctrine. The refiner's generic unit
  test examples do not override the explicit user and repository rules.
- Preserve the existing source ownership boundaries; no generated source copies
  are placed in dist. The distributable refinement artifact is the proof receipt.
- Tooling schema mismatch: the canonical refiner supports code in its SKILL.md
  and manifest schema, but its constraints schema enum omits code. Keep the
  repository's established constraints receipt contract (schemaVersion 1), with
  constraints sourced verbatim from the repository, rather than misclassify code
  as visual content. Do not claim validation against that incompatible enum.
- The later independent review is a fresh packet-only call, not self-certification.

## Source audit follow-up — graph digest binding

The reference carries graphDigest, but generation_manifest currently persists
only the projection fingerprint and counts. A syntactically valid changed graph
digest would pass query-engine loading. This violates the exact reference
binding requirement even though normal publication computes the correct digest.
Complete the active RocksDB verification command without interruption, then batch
the repair: persist the admitted digest with generation staging; require it at
reference loading; preserve it through bundle restore; cover valid-looking digest
tampering without reading canonical JSON. Certification remains pending.

## Storage integrity batch completed

The production/schema/documentation batch now binds graphDigest during
reference-aware staging and requires the same persisted value on query loading.
Bundle restore retains the binding while permitting a new physical location.
Generation GC is repository-scoped and removes retained IDs before the candidate
limit, preventing cross-checkout deletion and retained-generation starvation.
Public integration fixtures were updated only after this implementation batch.
The post-batch full default workspace suite is now running; complete SurrealKV
and RocksDB suites and final gates follow serially. Earlier passes do not certify
this latest batch.

## Deployment priority override

The operator explicitly requested immediate release installation and check-in
before reduced verification. Honor that order. The complete default workspace
and subsequent default CLI passes are retained; run only changed-surface checks
after deployment. Independent review and unrun broad gates remain pending rather
than becoming an implicit approval or a completion claim.
