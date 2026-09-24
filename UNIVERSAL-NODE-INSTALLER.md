# Universal Node — automatic managed-rental cleanup verified

## Latest: 24 September 2026 Standard User full upgrade and automatic cleanup

See workspace `05 LAPORAN/20260924-STANDARD-USER-E2E-PREBILLING-RECOVERY.md` and `05 LAPORAN/20260924-UNIVERSAL-V11-FULL-UPGRADE-PC02.md`.

Universal fixes `bc205e6`, `b9323a0`, and `e707c61` close prebilling-abort debt, stale cleanup outbox reconciliation, and exact orphan SUBST recovery in the renter logon namespace. Live Standard User session `202dab43-fbc1-4ad6-a590-1c909a90cd4e` proved Direct P2P UDP 1280x720, Sentinel login, billing, portable app execution, expiry, HOST+SENTINEL cleanup receipts, folder/process/state removal, automatic claim release in 11.05 seconds, and no reboot.

Candidate-v11 / 1.4.2 contains Host SYSTEM source `ec3e3ee`, installer preflight source `ad893b7`, and Sentinel `e707c61`: `02 HASIL BUILD DAN INSTALLER/UNIVERSAL NODE 20260923/candidate-v11/CloudgimeHostSetup.exe`, 693206843 bytes, SHA-256 `8F4B088DF518E49081CFEB19AD9453C97E42DE259C6CB3C7C1F55A92AD6D9C64`, unsigned. Full silent upgrade passed on PC-02 while preserving eight identity/credential hashes, renter SID/enabled state, autologon identity, plaintext-password absence, service/task baseline, SSH, and boot timestamp. Post-upgrade Direct P2P video and fresh Host/Sentinel readiness passed; no billing was started in that post-upgrade probe. Clean install and post-upgrade cold boot remain separate gates.


See workspace `05 LAPORAN/20260924-PORTABLE-APP-RENTAL-CLEANUP.md` and `06 TEST/renter-portable-20260923/FINAL-STATUS.json`.

Steam/Discord remain manual by owner. Renter Chrome profile, Downloads, clipboard and exact session folder are cleanup-managed. User apps are supported only as bounded portable ZIP or explicit `asInvoker` EXE from rental Downloads; MSI/driver/setup/admin/unknown-manifest executables are rejected. This is managed storage/process cleanup, not arbitrary native-code filesystem/registry isolation.

Live PC-02 session `bdd1bbab-f120-4d7a-8178-e9fe01ec790e` imported ZIP fixture to `Apps`, ran EXE SHA-256 `9831A9AB4E049525BE383A0228844F6A7DA768049C35B12EA12FB7DB915B1F` as a rental Job Object process, created an in-app marker, then cleanup `6e0cd6ea-ecfc-4ea4-a105-1cac9bc61541` completed in 21.782s with HOST+SENTINEL receipts. Apps/Downloads/session folder/process vanished; an outside control file retained hash `3B76AD108FF0C97400C7C71A22FA1B11BC1735E16330269A925364BAFA843A95`; Windows boot identity stayed `2026-09-22T05:42:07.8300400Z`.

An initial attempt exposed an image-handle cleanup deadlock and required exact-bound operator recovery; it is not counted as automatic success. Fix `4cf2235` then passed on a fresh session. Portable source commits: `a5679fe`, `cc3ecf1`, `0291aa7`; latest tray-order source `603cffb` is newer than the installed live-test package unless separately deployed.

Internal latest v10/1.4.1 candidate: `02 HASIL BUILD DAN INSTALLER/UNIVERSAL NODE 20260923/candidate-v10/CloudgimeHostSetup.exe`, 693183422 bytes, SHA-256 `DE265111370C321583D1BBA09080CEB484B9664D8CAACC8E399D75C57BFF1FEA`, unsigned, full v10 installer not executed as a whole. It packages Sentinel `879f814`; browser/Downloads/portable cleanup components were live-tested separately.

## Latest: 23 September 2026 automatic turnover

See workspace `05 LAPORAN/20260923-AUTOMATIC-CLEANUP-PC02-LIVE.md` and `06 TEST/automatic-cleanup-20260923/FINAL-STATUS.json` for the current outcome. Older operator-reset descriptions below are historical.

Host `c45b2ee` and Sentinel/backend `d43c97f` (tests `8d9b18e`) were deployed to PC-02 and completed three separate rental generations automatically: 23.715s, 11.578s and 33.434s from cleanup request to claim release. The last two exercised normal installed Chrome with a session-specific profile, 10/11 owned processes, Downloads/profile marker files, expiry, frozen/disconnected Direct streaming, deleted session directory, empty local state and renewed AVAILABLE status. Clipboard was explicitly set and then observed empty on the third generation. No operator cleanup/reset or Windows reboot was used for those turnovers. All three used the same valid test account with distinct bindings/paths; a second account failed authentication before claim creation and is not cross-account E2E evidence.

This verifies **managed rental scope only**, not arbitrary processes/files started from the shared Windows desktop, Steam/Discord credentials in default profiles, forensic erasure, or every crash/power-loss recovery. A lost Job Object must remain fail-closed. Runtime retry occurred and recovered; do not claim zero retries. TickCount64 in the PowerShell runner was null, so no-reboot proof is from OS event/process continuity instead, not its boolean alone.

Installer v8 / 1.3.0 includes the live-tested components: `02 HASIL BUILD DAN INSTALLER/UNIVERSAL NODE 20260923/candidate-v8/CloudgimeHostSetup.exe`, 693175370 bytes, SHA-256 `2DD068294BECDBD2442AD4F186819A966692EFF2962BA3008592AA1AAD9DA545`. Internal unsigned candidate; the full v8 executable has not been installed end-to-end, and clean install/cold boot/native clients remain separate gates. The later cosmetic retry text commit `d5e0860` is source-only, NOT included in that installer or the running PC-02 build.

Final PC-02 state: MAINTENANCE, no current renter/claim/reservation/pending cleanup, Host READY and Sentinel gate enforced. Source/admin Sentinel remains Stopped/Disabled. Test credits revoked and completed tasks removed; deployment backups retained.

## Historical v1.2 evidence

## Update 23 September 2026 — post-upgrade rental and replay verified

Read `05 LAPORAN/20260923-POST-V6-RENTAL-E2E-DAN-REPLAY-FIX.md` and `06 TEST/post-v6-rental-20260923` in the workspace for the newest evidence.

A fresh MASTER login resolved the former authorization blocker; the password was not stored. The v6 rental fixture proved Direct WebRTC UDP 1280x720, pre-login BILLING_NOT_CHARGED, login-started billing, Full Desktop with the same timer (575 -> 555 seconds), expiry settlement, disconnected WebRTC/ICE, closed WebSocket, frozen frames/video time, relock, and exact-bound operator cleanup without Windows reboot.

A separate direct-Host probe exposed a cancellation cookie revocation defect. Host `570e4cf` + backend `59b347a` now revoke exact claim-bound roots before claim release; 117 Host and 29 backend tests passed. Live tests proved one-use launch replay 409 before cancellation, both distinct same-claim device cookies 200 simultaneously, machine-only revocation rejecting renters, and both devices' old cookies/launch tokens rejected after cancellation. The post-cancel Host 404 responses explicitly identify missing stream/launch tickets; they are not generic proxy 404s.

Installer v7 (1.2.1) now contains the fix: SHA-256 `45F9DBE2EE34309A7E0AA3B0566E0AF71440C359E489C299A1C0EA596326D197`, 693144096 bytes, at `02 HASIL BUILD DAN INSTALLER/UNIVERSAL NODE 20260923/candidate-v7/CloudgimeHostSetup.exe`. It remains unsigned/internal. Full v6 installer upgrade was tested; the patched Host/backend components were live-tested; the complete v7 installer executable has NOT yet been installed as a whole.

PC-02 is back in MAINTENANCE with no claim, open rental, reservation or cleanup debt, Host READY and Sentinel gate enforced. A managed Host-service restart (no Windows reboot) was needed after the two-device cancellation test to clear a lingering `starting` readiness phase. Do not claim fully autonomous turnover from the operator cleanup or readiness recovery.


## Source and build contract

This is a linked worktree of the existing Host repository, not another canonical repository. The original dirty Host worktree is preserved and has NOT been merged or overwritten. The Universal release branch includes the tested runtime chain through `9cde566`, installer changes through `b5d4fd2`, and the passing web TypeScript compatibility fixes.

Build source components separately, then run:

1. `tools/portable/finalize-universal-bundle.ps1`: neutral copy, no enrollment/pairing state, STUN-only ICE, required files + SHA-256 manifest.
2. `tools/portable/build-universal-host-installer.ps1`: publish bootstrap, validate payload, build NSIS candidate. Never use an installed PC's enrollment state as packaging input.
3. Fixture-only builds require explicit `-FixtureOnly -InternalUnsignedCandidate`. Their executable is named `CloudgimeHostPackageFixture-NOT-INSTALLABLE.exe`; installation rejects them.

The full packaging pipeline remains an internal candidate lane, not a signed production release. Third-party Sunshine/drivers and some sidecars come from previously prepared runtime artifacts; the v6 Tunnel is the previously verified `3dc9a4e` artifact, not the older legacy release copy. Core Host binaries, Host Control, Sentinel, launcher, uninstaller and bootstrap were built locally from reviewed source. Rebuilding the web bridge outside the original source layout requires `CLOUDGIME_NATIVE_BRIDGE_SOURCE` to point at the Android source asset.

## Verified on PC-02, 23 September 2026

Internal candidate v6:

- File: `02 HASIL BUILD DAN INSTALLER/UNIVERSAL NODE 20260923/candidate-v6/CloudgimeHostSetup.exe`
- Bytes: `693136990`
- SHA-256: `7EECF25EE19CBB8B64408C5F1779429E2526C3518794CCDBFDA6024F2C2B179B`
- Bootstrap/source: `b5d4fd2`; Sentinel: `6ad9bc5`; Tunnel: `3dc9a4e`.
- Authenticode: **NotSigned**.

PC-02 is the separate host `RDPGRID-COM122`, `10.144.10.10`. The source/admin PC's Sentinel remains Stopped/Disabled.

Proven:

- Incomplete-payload preflight rejects before changing installed files/services.
- Read-only idle-journal verification succeeds on the pinned console without changing the encrypted journal.
- Initial v4 upgrade failed because stopping an already-stopped Host service returned Win32 1062. The external controlled-pilot recovery restored Host, Sentinel and broker without reboot; protected identity files remained unchanged. This proves the pilot recovery, not every installer crash-recovery path.
- Fixed v6 upgrade returned exit 0 and installed the expected core Host/Tunnel hashes.
- Provisioning credentials, manifest, desktop pin, pairing data and renter journal remained unchanged.
- Existing configuration changed only in `webrtc.ice_servers`: TURN entries/credentials removed, STUN retained; assigned path and ports preserved.
- File replacement/rollback preserves the prior Windows security descriptor.
- Host local health: `healthy`, NVENC/DDX. Backend: `RUNNING/READY` and Sentinel `MVP_PRELOGIN_READY/MVP_PRELOGIN_GATE_ENFORCED` with fresh reports.
- SSH 22, RDP recovery and the running transport task preserved; PC-02 remains MAINTENANCE with no sessions, claims, reservations or cleanup debt.
- No reboot during upgrade: uptime remained >25 hours and no boot/shutdown events occurred during the pilot. WMI LastBootUpTime shifted ~1.33 seconds for the same prior-day boot; the raw `bootUnchanged:false` exact-string comparison is superseded by the uptime/event verification, not silently edited.
- Deadman rollback timer was explicitly disarmed only after post-upgrade checks.

Evidence: `06 TEST/universal-node-20260923/pc02-v6-verified.json`, `v4-recovery-result.json`, `negative-preflight-pc02.json`, `idle-probe-pc02.json`, candidate build logs and manifests. The remote backup and quarantined failed files are retained under the protected pilot directory for recovery.

## Tests run

- 32 bundle contract assertions.
- 12 encrypted IDLE journal assertions (corrupt, active, unknown formats fail closed).
- 3 file transaction assertions and 8 Direct-only migration assertions.
- Existing pure install/autologon policy/rollback tests.
- 22 Host provisioning/transport tests.
- 104 API binding generation tests.
- 9 Direct lifecycle tests.
- 17 Sentinel deferred-install/login/operator-recovery tests.
- Bootstrap/packaging/uninstaller builds passed; core Host, Host Control, static web assets and Sentinel package built successfully. Some existing compiler/dependency warnings remain.

## Unverified / do not overclaim

- Historical v6 gate: post-upgrade rental E2E was initially blocked by MASTER login; this was resolved and the bounded rental/expiry/operator-reset run passed as described above.
- The original 22 September pilot remains historical; the new 23 September v6 billing/desktop/cutoff evidence is independent.
- Clean install/activation/finalizer on disposable Windows, second installation retry, uninstall, full cold-boot recovery/autologon and native Windows/Android clients remain pending.
- Conclusive consumed-token and cancelled-claim cookie/launch replay rejection was verified after the fix, including two devices. Expired-session replay through the closed proxy was not independently verified and its generic 404 is not counted.
- Autologon is opt-in via a local password dialog, LSA secret storage. It was not configured on PC-02. Do not enable it on the source/admin PC.
- File transaction unit tests and observed controlled rollback do not prove cross-component atomicity, arbitrary power-loss recovery, or automatic recovery across every crash point.
- The Helper pipe probe from SSH timed out; gate evidence comes from fresh backend-authoritative heartbeat, not that failed pipe probe.
- Build-PC Defender was disabled/unavailable for a verdict; PC-02 Defender was observed active. No completed antivirus scan verdict was captured. No claim of antivirus-clean status.
- Build reported SharpCompress 0.37.2 moderate advisory and npm audit findings in legacy build dependencies. Production dependency/signing review remains open.

## Next authorized work

MASTER authorization is no longer blocked. Remaining gates: full v7 installer, disposable clean install/finalizer/uninstall, cold-boot secure autologon, native Windows/Android clients, dependency/signature review, and autonomous cleanup/readiness. Do not rerun the old pilot against its pre-migration config hash: v6 intentionally removed TURN. Create a new explicitly scoped baseline for any further maintenance. Passwords and runtime credentials must never be embedded in tests or logs.
