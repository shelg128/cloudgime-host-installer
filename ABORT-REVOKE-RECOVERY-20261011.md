# Host abort-revocation recovery — 11 October 2026

Canonical Universal worktree fix94fe323 preserves issued sibling handoff IDs per claim/native root, bounds retained IDs, and installs an atomic deny tombstone for a never-opened verified root. Unknown/foreign handoffs are still rejected. The old scope stop API remains strict; only the backend-authenticated root revocation path uses the new method.

Targeted regression:12 tests passed, including issued sibling before first stream and exact replay. Release build completed using the pinned nightly toolchain and existing OpenSSL3.6 runtime. No claim/billing/security bypass introduced.

PC02 web-server deployed SHA256 `9ed42467082bbc1494817bf49c3b76c8a181f18b483fef497905082e321357a4`. Adjacent DLLs now required: `libssl-3-x64.dll` SHA256 `7c9c796f9590bb8e8a5c881ef697df33864847e80f8cf641028be9bbfbae0332` and `libcrypto-3-x64.dll` SHA256 `2c00c1bac843c0e5803d48c539e2e7e411c8bc3db77ea0210cc945358fe5c90b`. Exact previous web-server/rollback preserved in machine-private QA directory. Sunshine/config, streamer and permanent display-helper hashes unchanged. This dependency-carrying recovery build is not a signed universal installer release.

Real Host worker subsequently produced an authenticated stored HOST receipt for the ended never-billed QA abort. That validates the missing-root-after-process-restart recovery path on PC02; issued-sibling retention is unit-tested, not independently proven live without restart by this pilot.

Full rental acceptance remains pending: PC02 retains a cleanup hold awaiting Sentinel reset/profile proof. No claim release or fake evidence was performed. Full media quality was not retested and is not implied by this revocation fix. Before future actions read workspace `05 LAPORAN/20261011-LIVE-ROLLOUT-ABORT-RECOVERY-HOLD.md` and current installed recovery baseline, not historical expected hashes.
