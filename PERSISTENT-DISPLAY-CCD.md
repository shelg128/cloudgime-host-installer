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
- **Physical Android edge-to-edge content test is pending: phone locked.** Never count mode metadata, a desktop probe or successful compilation as visual acceptance. The earlier images containing landscape content inside portrait frames remain FAIL.
- Third-party remote apps have not been disabled or reconfigured.
- Other existing acceptance results were not rerun. Full release remains blocked by the previously open pacing gate and production native-stop rollout.

## Build integrity
`build_portable_host_bundle.ps1` now fails on missing helper source or failed helper publish. It records the helper's `--display-runtime-contract`, binary SHA256 and normalized source hashes. `build-host-offline-installer.ps1` verifies the installed bundle's manifest even when `SkipBundleBuild` is selected. A source manifest is provenance, not a signed trust attestation or proof of live acceptance.

## Integration boundaries
The release branch retains its pre-stream input-drop handling, ordered upstream shutdown, rental cleanup receipts and claim-cancel support. Primary-worktree unrelated changes are not imported wholesale.

## Next targeted validation
Unlock the authorized physical phone; use the candidate artifacts from this branch. Test only: actual desktop target at all four viewport edges in portrait, landscape and portrait return, with StarDeskServer active; no stretching/cropping and no black/white bars. Compare live desktop geometry as well as decoded dimensions and screenshot pixels. Record same rental/cleanup as setup/safety evidence, not as a rerun of the full regression suite. Do not promote a public installer before these gates pass.
