# Persistent managed display — CCD candidate

## Scope
This branch integrates the shared display-state broker, additive device-mode catalog, exact CCD mode activation, encoder renegotiation and build identity checks. It is not a PC-specific workaround. It must not stop/suspend StarDesk or any third-party remote service as normal product behavior.

Android stores its installation-local screen profile. Each Host registers missing native dimensions and their transpose once, retaining existing modes. Windows still selects the appropriate mode when device/orientation changes; registration reuse does not mean one physical desktop can have multiple simultaneous source resolutions.

## Permanent activation
`DisplayConfigModeSetter.ApplyExact` uses `QueryDisplayConfig` / `SetDisplayConfig` with `SDC_USE_SUPPLIED_DISPLAY_CONFIG`, validation, apply and save-to-database. Only the named managed VDD is selected; source/target IDs bind the operation. A cloned/shared source is rejected. Source position, other output modes and opaque timing data are retained. Actual source geometry, orientation and refresh are checked; unrelated changes or invalid timing fail rather than silently yielding landscape fallback. A separate bounded stability observation rejects rapid external mode resets.

The helper remains in the interactive standard-user session. The SYSTEM broker retains state and capture configuration ownership. No directory ACL is widened, no GPU/PC ID is hardcoded, and no continuous resize loop is added.

## Evidence and limits (2026-09-30)
- Idle PC-02 CCD probe accepted 720x1570@30 and held it for all 60 observations (~15 s), while StarDeskServer and Sunshine remained running. Original snapshot was restored successfully.
- This supersedes the earlier hypothesis that StarDesk must be disabled: the previous DEVMODE-only path was transient and incompatible with its display behavior. CCD coexistence is promising, not a guarantee against arbitrary administrator-level display changes.
- Ten new CCD planning/ABI/isolation tests PASS.
- Three new packaging identity tests PASS.
- Web-server, streamer and helper built successfully from this branch.
- **Physical Android edge-to-edge content PASS:** `capture-resize-1790741698797`, source commit `7ca4b41`: actual desktop, decoded frame and viewport agree at 720x1570 -> 1570x720 -> 720x1570. All four colored desktop edges reach the viewport boundary. Screenshot pixel checks and direct visual review both pass; controller/menu overlays are intentional overlays, not empty padding. StarDeskServer PID 4044 stayed running throughout; no pause/reconfiguration was used.
- The earlier images containing landscape content inside portrait frames remain FAIL; this new run supersedes their unresolved screen-content status, not their historical results.
- Third-party remote apps have not been disabled or reconfigured.
- Other existing acceptance results were not rerun. Full release remains blocked by the previously open pacing gate and production native-stop rollout.

## Build integrity
`build_portable_host_bundle.ps1` now fails on missing helper source or failed helper publish. It records the helper's `--display-runtime-contract`, binary SHA256 and normalized source hashes. `build-host-offline-installer.ps1` verifies the installed bundle's manifest even when `SkipBundleBuild` is selected. A source manifest is provenance, not a signed trust attestation or proof of live acceptance.

## Integration boundaries
The release branch retains its pre-stream input-drop handling, ordered upstream shutdown, rental cleanup receipts and claim-cancel support. Primary-worktree unrelated changes are not imported wholesale.

## Retained pilot and remaining work
The exact visually tested candidate is retained on PC-02 in maintenance, with backup and hash verification. Deployment is a pilot of this universal source, not a PC-specific fix and not a public release. Helper SHA256 `d32d6d3520039c5d11404e1b85d90623a82dc00f037af376d140fa9de0061323`, web-server `8b4d66bc2faa85ecc6428dad1882c74256724b25b47565cbc410f353cb55b8ea`, streamer `af431737df7b84bf8b5994d49fcf17b35e9d9d47d5667650f75a2a51b4f97a2c`.

Do not rerun the passed portrait/landscape/profile cases unless affected source changes. Residual FPS/jitter, public native-stop deployment and a full installer/cold-boot run remain separate open acceptance items. Edge-to-edge capture does not guarantee every game supports arbitrary aspect ratios or prevent an administrator application from intentionally replacing display topology. No claim of all GPUs/phones validated.
