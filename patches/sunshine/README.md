# Shared Windows capture recovery (candidate v1)

This is a **shared source patch**, not a PC-ID/IP/GPU-specific workaround. The validation machine is not encoded in production source. It does not change Direct/Relay routing, disable monitoring/security services, force WGC, or alter display settings.

## Why this exists

A separate DDAPI probe on the renter desktop reproduced `DXGI_ERROR_ACCESS_LOST (0x887A0026)` every five seconds even with no active CloudGime stream. DXGI factory/topology stayed current. Sunshine's existing full teardown cost approximately 1.2–1.6 seconds per occurrence. The initiating external/driver event is still unidentified.

Recoverable loss of the *capture object* must not necessarily destroy an otherwise healthy encoder. The patch replaces only the duplicator if all invariants still match. It does **not** hide/fake lost frames.

## Recovery invariants

- Only `DXGI_ERROR_ACCESS_LOST` is eligible; access denied, device removed, abandoned wait, unknown errors use full reinit.
- Same input/thread desktop, output name and monitor identity, adapter LUID, geometry/rotation, refresh rate, pixel format, colorspace and bit depth. Unknown state fails closed.
- Factory current and GPU healthy both before and after recreation.
- At most one recreation attempt per error; errors under 250ms apart escalate to full reset.
- Keep the initial successful DDAPI version. No automatic API/route fallback.
- Never call SwitchDesktop/SetThreadDesktop inside recovery; normal upstream full-reset logic remains responsible for genuine desktop changes.
- Release outstanding resource/frame ownership, clear RAM/VRAM cached images/cursors, and require a real fresh frame after recreation.
- Destructor may release resources but never recreate the duplicator.

## Reproduce

1. Checkout LizardByte/Sunshine commit `86188d47a7463b0f73b35de18a628353adeaa20e` and its pinned submodules. `upstream-lock.json` pins relevant blobs. Windows CRLF is normalized only after comparison with the committed blob.
2. `node patches/sunshine/apply-capture-recovery.mjs <checkout>`; it rejects revision/content mismatch, checks all files before writes, and validates idempotency using generated `cloudgime-capture-patch.json`.
3. Use `tools/build-sunshine-capture.ps1 -SourceRoot <checkout> -ToolchainRoot <UCRT64-prefix> -BuildRoot <short-build-path>`. Dependencies must satisfy upstream's Windows UCRT64 build. This does not install system packages or deploy.
4. `record-build.mjs` records the candidate binary SHA256, upstream/submodule identities, patch fingerprint, and `acceptance: NOT_RUN`.
5. Deploy only to an idle authorized validation host with protected backup/rollback. Verify streaming, observed capture-only recovery, display-change full reset, cleanup, and same-rental billing. Do not count synthetic fixtures/probe-only results as full Sunshine acceptance.
6. `verify-runtime.mjs <runtime>` refuses wrong hashes/current source mismatch or missing live acceptance. A manifest is build provenance, not a cryptographic release signature; trusted CI/signing remains required for public releases.

## Tests

- Native policy tests: `g++ -std=c++17 -Wall -Wextra -Werror -static tests/capture_recovery_policy_test.cpp ...` (relative to this directory). 31 checks for invalidation/error boundaries.
- Native Windows probe: `tests/capture_recovery_windows_test.cpp`; run `probe.exe output.jsonl legacy` or `output5` explicitly. It writes frame counts/timing, **not screen images or audio**.
- Integration tests: set `CG_SUNSHINE_TEST_SOURCE` to an *unpatched* upstream `src/platform/windows`, then `node --test tests/patch-integration.test.mjs tests/runtime-manifest.test.mjs`.

## Evidence as of 29 September 2026

Shared recovery module native test (legacy DDAPI) recovered **9/9 losses**, **11–47ms** per recovery, max observed frame gap **101ms** over 45 seconds, 2667 frames, no failed recovery. Output5 probe reported `DXGI_ERROR_UNSUPPORTED` in that isolated probe environment; that does not establish what API Sunshine selects on the same host.

## Full build and live evidence (29 September 2026)

A full Windows Sunshine candidate was built from the locked source and UCRT toolchain, with synthetic-pointer header feature detection for current mingw-w64. Candidate version: `2025.924.154138-cg-capture1`; SHA256 `846ef6cf6e9a8ea15719c5a45585434481f5feff3e1c2766ffefd74715f61ef9`. DLL imports and local version launch were checked. Build dependency and toolchain lock records accompany the candidate archive.

- `advanced-1790660585707`: real capture-only recovery logged every five seconds (mostly 1–15 ms), with encoder retained. Direct accumulated freeze 1.364s / 88.769s, versus about 28s / 89s baseline. Audio Direct/Relay passed digital tone verification. Relay accumulated 3.282s / 88.663s, **above** the unchanged 3% pacing gate.
- `advanced-1790661405142` with timestamp fix too: Direct 1.621s / 88.639s, Relay 2.582s / 88.651s, but fixed-bitrate Direct 5.728s / 88.623s FAILED. This is not full acceptance of either runtime bundle.
- `capture-acceptance-1790663944777`: longer pacing run still failed; one observation crossed counter generations and is invalid, another exceeded the pacing limit. Keep failed evidence.
- `mode-transition-1790671237417`: actual Windows mode 1280x720 -> 1280x800 -> 1280x720, full encoder teardown/recreation and advancing frames after restore; same claim/root/billing. PASS for preserving genuine full-reinitialization behavior, **not** for Android resize-menu acceptance.
- Android resize remains a separate open issue: Host helper cannot persist protected display state, and stale/no-op results were misreported as applied. `src/api/display_helper_contract.rs` now rejects nonzero exit, stale mutation, and resize without observed mode. Five contract tests and full Host `cargo check` pass; this Host change is not deployed.
- Android control-socket recovery is implemented separately in shared source and passed foreground recovery tests on Direct and Relay with a single rental; it does not make the capture pacing gate pass.
- The latest combined function regression stopped before billing; it is not a candidate PASS.

**NOT RELEASE ACCEPTED.** All candidate Host deployments were restored to the original binary after testing. Source and packaging changes are durable, but the capture candidate is not globally activated. `verify-runtime.mjs` and both bundle/installer builders require matching binary/source hashes and complete live acceptance including pacing and regression; the candidate manifest must remain blocked. Do not mark a subset of these tests as all-feature acceptance.

All diagnostic monitoring pauses completed with verified resume and tasks removed. No third-party service was permanently disabled. PC-02 is the validation target only; production code has no machine-name, PC-ID, IP, or specific-GPU exception.

All modified upstream code and new patch code are GPL-3.0-only; distribute upstream license/notices and corresponding source with any modified binary.
