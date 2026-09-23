# Universal Node v1.2 — internal candidate and verified upgrade

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

- No post-v6 claim -> stream -> billed login -> Full Desktop -> cutoff E2E run. The attempt to authorize that test stopped at MASTER login HTTP 401 before granting fixture credit or opening maintenance. Stored server MASTER password does not match the single ACTIVE MASTER; known local cached MASTER tokens are absent. Do not reset credentials or bypass auth.
- Previous 22 September billing/desktop/cutoff evidence applies to the previous runtime pilot, not automatically to v6.
- Clean install/activation/finalizer on disposable Windows, second installation retry, uninstall, full cold-boot recovery/autologon and native Windows/Android clients remain pending.
- Conclusive old-token replay rejection remains pending.
- Autologon is opt-in via a local password dialog, LSA secret storage. It was not configured on PC-02. Do not enable it on the source/admin PC.
- File transaction unit tests and observed controlled rollback do not prove cross-component atomicity, arbitrary power-loss recovery, or automatic recovery across every crash point.
- The Helper pipe probe from SSH timed out; gate evidence comes from fresh backend-authoritative heartbeat, not that failed pipe probe.
- Build-PC Defender was disabled/unavailable for a verdict; PC-02 Defender was observed active. No completed antivirus scan verdict was captured. No claim of antivirus-clean status.
- Build reported SharpCompress 0.37.2 moderate advisory and npm audit findings in legacy build dependencies. Production dependency/signing review remains open.

## Next authorized work

Obtain a fresh valid MASTER login (no password in chat), run a bounded rental fixture with guaranteed stop/maintenance restoration, then clean-install/cold-boot/native/replay gates. Do not rerun the old pilot script against its pre-migration config hash: v6 intentionally removed TURN. Use the existing committed verification record and create a new explicitly scoped baseline for any additional maintenance.
