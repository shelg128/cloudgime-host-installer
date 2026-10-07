# Permanent stability baseline — 7 October 2026

## Canonical release lane

This Universal Release worktree is the integration target for proven Host stability fixes. The original `01 CLOUDGIME HOST` worktree has substantial uncommitted owner work and is deliberately preserved. Do not overwrite it, reset it, or blindly merge the experimental RTP-CLOCK branch. New releases must use this lane's tested packaging/installer gates until the owner changes that source-of-truth decision.

## Promoted and enforced

| Fix/invariant | Permanent implementation | Regression/enforcement |
|---|---|---|
| Recovered H.264 NVENC initialization error must not keep selecting software | `NvencLogEvidence.cs`, real helper selector/cache | 42 direct helper tests, including NVIDIA/Intel/AMD/unknown GPU and genuine probe failure |
| Modern/legacy Sunshine timestamp parsing and bounded logs | DisplayPrepareHelper, default build | Same tests; no synthetic physical quality claim |
| Old cached decisions must be re-probed, never force-labeled NVENC | Versioned cache policy, default helper | Cache migration/new-failure tests |
| Old helper cannot silently enter a new release | Binary `--encoder-runtime-contract`, source+binary receipts | Mandatory portable build, Universal finalizer, NSIS packaging and installable-bundle checks |
| Machine-specific GPU/display cache cannot migrate to another PC | Neutral bundle finalizer and bundle contract | Whole inert NSIS pipeline and rejection tests |
| Display modes for multiple clients remain persistent | Existing CCD/catalog/stability implementation unchanged | 10 display-profile, 10 CCD and 4 stability tests retained |
| Existing identity, billing, cleanup and route boundaries stay unchanged | Existing Universal source retained | Installer identity/rollback/transport tests retained; no billing/rental test started for this update |

Available commands:

```
npm run test:encoder-stability
npm run test:universal-contract
npm run test:release-identity
powershell -NoProfile -File tools/portable/test-encoder-release.ps1 -OutputRoot NEW_DIRECTORY
```

CI `universal-node-contract.yml` runs qualification, artifact and installable-bundle tests for relevant pull requests and main/Universal branches; workflow is committed locally, not yet pushed/executed by GitHub. No skip switch was added to installable production packaging. Fixture mode produces an explicitly non-installable test artifact only.

## Actual PC-02 deployment

The qualified **helper only** was permanently installed during idle maintenance, SHA256:
`fe69d3bff73061e856331b0b74d82062c164a5528cacb0c3c8c8b5eb478e717b`.
It carries `cloudgime-encoder-stability-v1`, NVENC log policy1 and the existing display broker contract. Display/encoder receipts are installed beside it. Atomic replacement retained original ACL, backup and exact rollback path. No reboot, service restart, rental, Sunshine/web/streamer/APK change or credential change occurred.

A profile-only invocation of the **installed** helper ran actual probes: selected NVENC/DDX with policy1. Its temporary profile-only metadata was then restored so readiness/config semantics were not left altered. The new policy will run on normal preflight; installation alone did not rewrite the existing capability profile. Sunshine configuration stayed byte-identical. Backup: protected `stable-helper-updates/run-*` on PC-02. Old scripted test baselines referencing helper `116260...` are historical and must not replace or roll back the new permanent helper without an explicit rollback decision.

## Universal does not mean identical forced media settings

Do not force NVENC on Intel/AMD-only machines, software on a healthy hardware encoder, 720×1570 on every display, or H.264 Baseline on every device. Existing actual-probe selection is preserved. The RMX5367 Baseline/720×1570/30/5000 profile remains an experimental acceptance reference, not a global policy. Error-free internet or freeze<1% for every device is not claimed.

## Not promoted / still open

- Deadline recovery, copy budgets, sync isolation, recorder and quarantined capture orchestration remain QA experiments.
- No full signed Universal installer released/installed for this revision; only the helper is deployed. New packaging/bootstrap code is built/tested locally.
- Source main's uncommitted changes are not claimed merged; no remote push performed.
- Full live startup/rendering/freeze acceptance, cross-vendor physical GPU testing, clean install and reboot/upgrade for this revision remain separate gates.
- Daemon-aware restart and pre-rollback evidence fixes remain in the QA deployment lane; no automatic production service-lifecycle migration was silently introduced.

Before a future release or pilot: use this source lane, qualify the helper, verify actual installed hashes and maintenance state, preserve user credential/identity, keep route selection explicit, and snapshot logs before a rollback that restarts Sunshine. Do not call historical expected hashes the current baseline.
