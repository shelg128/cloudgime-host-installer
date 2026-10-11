# Permanent stability baseline — 7 October 2026

## Latest scoped network recovery update — 11 October 2026

PC02 now runs broker/helper0c0a7736 and ASAR133d98a4 (behavior f6723e4; evidence f00d854). Host9ed42467, permanent display helperfe69d3 and shell d89d7a93 unchanged. Actual10.448s/25.424s backend route/socket faults recovered the SAME rental/commit/claim without new login, ~3s after network return.60.5s exceeded recovery and invoked official RENTER_LOGOUT3.56s after return, followed by automatic cleanup/release74.984s. Final candidate restart passed with5credential hashes unchanged. No automatic pause/refund: existing server clock remains billing authority until termination. Latest report `05 LAPORAN/20261011-NETWORK-RECOVERY-VERIFIED.md`, baseline `02 HASIL BUILD DAN INSTALLER/NETWORK RECOVERY 20261011/PC02-INSTALLED-BASELINE.json`. PC02 maintenance/no active rental/claim/hold, QA tasks/faults0, pilot disabled. No new signed installer, clean-install, full-link/media/power-loss qualification; older network failure notes below are historical.

## Restart/network test update — 11 October 2026

PC02 had two real Windows restarts, final broker/helper163b6ce8 (ef72ebb) included; Host9ed42467/displayfe69d3/shell d89d7a93 unchanged. Auto-start/autologin/Sentinel lock and credential preservation passed. Backend-control-plane outage safety lock observed; **active session network resume still failed** (10s interruption leaves login while old billing continues to stale-heartbeat stop). Cleanup-retirement bug repaired in Sentinel, final both QA claims complete; PC02 maintenance/zero claims/holds, pilot off. Use workspace `RESTART NETWORK PATCH 20261011/PC02-INSTALLED-BASELINE.json` and `05 LAPORAN/20261011-RESTART-AND-NETWORK-TEST.md`, not old helper hashes. No Host media/StarDesk settings changes or power-loss test. No implication that network or universal installer acceptance is complete.

## 11 October 2026 installed-state update (supersedes historical status below)

Canonical Host branch `chatbox/held-abort-revoke-20261011` was committed/pushed (code94fe323, docs2ef0586); PC02 web-server9ed42467 deployed with required OpenSSL DLLs. Original Sunshine, streamer/config and permanent display helperfe69d3 unchanged. Core CloudGime rental qualification now passed real website login/billing/Full Desktop/A→A→B→A managed-profile/expiry/reset/claim release, independent of third-party StarDesk password management (owner explicitly user-managed). PC02 MAINTENANCE, zero active rental/claim/cleanup; the old held abort is COMPLETED. See workspace `05 LAPORAN/20261011-RENTAL-CORE-LIVE-ACCEPTANCE.md` and `02 HASIL BUILD DAN INSTALLER/RENTAL ACCEPTANCE 20261011/PC02-INSTALLED-BASELINE.json`. No signed Universal clean-install or media-quality acceptance is implied; old no-push/helper-only statements below are historical.

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

## Capture dependency follow-up — 7 October 2026

See `CAPTURE-RECOVERY-REQUALIFICATION.md`: the existing September29 capture-only patch is now preserved under `patches/sunshine` in this release worktree. A fresh targeted physical test eliminated periodic full encoder teardown with3–12ms capture-object recovery, but residual freeze remains and production acceptance is still blocked. The archived runtime was tested temporarily and restored; the PC-02 installed baseline above is unchanged. This work is based on the permanent stability commit and does not silently merge the separate startup-impact candidate.

## Not promoted / still open

- Deadline recovery, copy budgets, sync isolation, recorder and quarantined capture orchestration remain QA experiments.
- No full signed Universal installer released/installed for this revision; only the helper is deployed. New packaging/bootstrap code is built/tested locally.
- Source main's uncommitted changes are not claimed merged; no remote push performed.
- Full live startup/rendering/freeze acceptance, cross-vendor physical GPU testing, clean install and reboot/upgrade for this revision remain separate gates.
- Daemon-aware restart and pre-rollback evidence fixes remain in the QA deployment lane; no automatic production service-lifecycle migration was silently introduced.

Before a future release or pilot: use this source lane, qualify the helper, verify actual installed hashes and maintenance state, preserve user credential/identity, keep route selection explicit, and snapshot logs before a rollback that restarts Sunshine. Do not call historical expected hashes the current baseline.
