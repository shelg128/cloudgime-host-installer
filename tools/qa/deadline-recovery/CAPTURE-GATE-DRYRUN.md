# Full capture-gate dry-run — control path passes, visual path remains blocked

## Decision

The new clock helper is wired into a prepared, **unlaunched** pilot v3 runner. Its prebilling helper and the same capture-validation core were tested on idle PC-02, without a rental, capture request, service restart or production binary replacement.

**Do not launch v3 yet.** Measured screenshot acquisition plus downstream control latency cannot meet the unchanged five-second visual-evidence freshness budget. `capture-readiness.mjs` rejects it, and the prepared runner checks that decision **before deployment and claim**. There is no live capture or streaming-quality improvement claim.

This result intentionally stops the earlier pattern of discovering tool timing failures after billing.

## What actually ran

### Shared gate and separation of effects

- `CaptureGate.Core.ps1`: side-effect-free validator of exact context, process/SSRC/epoch, executable hash, backend binding, image/fixture scope, original evidence age, progress and quota.
- `CaptureGate.DryRun.ps1`: dedicated non-writing entry point. It reads synthetic rental/renderer/progress fixtures in an isolated QA directory; looks up and hashes the real installed web-server as its actual-process stand-in; runs the shared validator and discards the proposed request. It **cannot write `request.json`**, even when eligible.
- `CaptureGate.Live.ps1`: separate live entry point that verifies a real owned staged streamer, pins process identity/hash, reads the real peer/rental/fixture/progress files, invokes the same validator twice and creates a request once. **This entry point was not executed against a live rental in this work.** Its filesystem/owner adapter is not proven by the dry-run stand-in. It was parsed/reviewed and shares the tested pure core; do not imply a full successful live activation.
- Core and entry scripts are hash-pinned before remote use. QA staging was admin/SYSTEM-only under a unique PC-02 QA directory. No process-memory reads, public endpoints, global configuration or billing writes.

Seventeen local Windows PowerShell cases passed: valid input, stale backend/review/progress, claim/hash/process mismatch, ended rental, missing commit, Relay selection, unreviewed/background fixture, request replay, excessive quota, invalid packet rate, future timestamps. Initial incorrect `-notcmatch` syntax was caught and corrected to `-cnotmatch` locally, before remote execution.

### Real remote control timing, synthetic authority

Three complete remote dry runs used:
1. The compiled read-only clock helper over established SSH, six samples, explicit close/exit.
2. Actual test-user authentication and read-only `/session/list` requests. The real backend had **zero active rentals**. No floor/claim/session-start endpoint was called.
3. Synthetic `BILLING_RUNNING`, rental identity, visual review, and SDK progress solely as explicit validator fixtures.
4. Actual Windows web-server lookup, executable hash and fixture JSON I/O, followed by the shared validator.

Observed backend reads:66–102ms. Remote command durations about2.37–2.50s; validation saw backend/review age upper bounds2.007–2.157s. All three valid dry runs passed within the3s backend freshness requirement. An intentionally aged backend observation was rejected as `STALE_BACKEND`. All runs had `requestWritten=false`, `requestExists=false`.

The final shared Node orchestration was then exercised on PC-02, including entry-script hashing. It passed with about2.604s evidence age at the gate. The exact prepared runner's prebilling helper also passed with an88ms actual backend read,18ms clock interval width and2.524s evidence age. These margins are narrow and may change under live load; no latency guarantee is inferred.

Clock offset uncertainty remained roughly17–18ms. This is **control clock-offset uncertainty**, not video latency or one-way media latency. Sources are never restamped to current time to satisfy freshness.

## Measured visual-path blocker

The original screenshot wrapper (collapse status bar,900ms wait, remote screencap file, stat/hash/pull, local decode) was measured once on the awake physical phone lobby:

- Total **5087.265ms**.
- Screencap command2068.752ms; pull1343.608ms; other checks/conversion/wait made up the rest.

A new isolated `fresh-screenshot.mjs` was tested with one `adb exec-out screencap -p` acquisition, direct stdout bytes, PNG structure validation and local FFmpeg decode:

| Trial | Acquisition | Total verified thumbnail |
|---|---:|---:|
| 1 |4995.953ms|5200.502ms|
| 2 |4459.025ms|4655.609ms|

It was **not faster in these measurements** and is not promoted as a speed fix. No recapture retries, remote file writes, or device security changes. Actual image viewing confirmed the returned content was the CloudGime lobby; it was not a game/stream fixture review. Content, compression and device conditions differ across acquisitions; this is not a randomized screenshot-method benchmark.

The readiness assessment conservatively combines:
- worst observed new screenshot5200.502ms;
- worst qualified downstream review-age2524.013ms;
- an explicit1000ms review allowance (assumption, not measured human/assistant reaction);
-250ms safety margin.

Estimated total **8974.515ms**, exceeding the unchanged5000ms visual budget. Even the fastest measured capture4655.609ms plus downstreamgate~2000ms leaves no review time. This ~9s number is a budget estimate from separate idle measurements, **not a single measured end-to-end live sequence**. Old-image actual timestamp with a new review was additionally tested: rejected locally with `EVIDENCE_EXPIRED_BEFORE_DISPATCH`; no remote gate call occurred.

Do not "fix" this by dating evidence at review completion or by widening freshness tolerances without an independently justified protocol change. A source identity/continuity design or a demonstrably faster validated visual path is needed before using this protocol live. That design is not implemented in this milestone.

## Runner integration completed, not launched

Prepared paths:
- `06 TEST/20261007-scoped-pilot-v3`
- `06 TEST/20261007-scoped-pilot-v3-run`

The generated runner:
- checks `requireCaptureReadiness` before deployment;
- stages hash-pinned clock/core/live/dry-run helpers along with the existing reversible deployment;
- verifies actual parent startup activation before billing;
- executes the same authenticated read-only prebilling dry-run before making the PC available/claiming;
- checks that clock result is still fresh immediately before `/floor/claim`;
- refreshes clock near capture after login (old prebilling mapping is not reused indefinitely);
- uses original screenshot **start** timestamp rather than later review timestamp;
- rechecks wall/monotonic elapsed time to reject observed local clock steps;
- invokes one hash-pinned live gate without auto-retry. An uncertain live reply must be reconciled; a request is never resent blindly.

The prepared readiness-decision file currently says `eligible=false`, so the runner cannot start its deployment or claim path. No execution.json or launch exists for v3. The live writer/deployment/ticket path was not retested here. The helper and validator fixture integration pass does not attest actual active-rental permissions, OS ACL security or renderer continuity. Caller-supplied boolean/provenance strings remain test assertions, not cryptographic authority.

## Source / tests / retained limits

New code is under `tools/qa/deadline-recovery`; no streamer/encoder/Android production source modified.143NodeQA tests passed with retained previous evidence and current dry-run evidence enabled, no skips;17PowerShell validator cases passed. These counts are code/tool validation, not quality acceptance.

`fresh-screenshot.mjs` remains an isolated candidate with negative timing results retained. No installed APK or decoder patched. All failed/slow outcomes and synthetic assumptions are preserved.

The current live-gate script retains the trusted local QA boundary. It does not independently authenticate backend evidence or operator review. Process hash/identity and file snapshots can still change between checks; mutable snapshots are rechecked before creation, but there is no transaction spanning backend authority, OS files and process lifecycle. The existing streamer context/expiry and parent cancellation guard remain essential. No release security certification is claimed.

## Final audit

PC-02 ready/maintenance, no owner/claim, baseline web/streamer/Sunshine/config unchanged; zero QA environment variables, zero media/ClockProbe processes, zero request/capture files in this work's QA directory. Phone baselineAPK, unlocked, screenON, lobby `mHoldScreenWindow`, wirelessADB fresh. V3 runner not started. Dry-run fixtures and helper files remain in private QA staging, not running. No new rental, restart, production replacement, or live capture occurred.

Evidence root: `06 TEST/20261007-capture-gate-dryrun` — local gate tests, raw remote rounds, integrated helper, actual screenshot timings/images, blocked readiness decision, stale actual-image rejection and final audit. Hash manifest includes the prepared runner separately from executed artifacts.
