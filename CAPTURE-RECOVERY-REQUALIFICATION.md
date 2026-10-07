# Capture-only recovery requalification — 7 October 2026

## What was actually achieved

The five-second encoder reset symptom was reproduced and targeted with **the existing capture-only recovery patch**, not a new recovery algorithm. The older September29 work already documented it; the installed baseline did not contain the patch because full release acceptance remained open. The patch and rebuild instructions are now preserved verbatim in this Universal source lane, without merging other experimental Host/streamer changes or the separate `ec28a60` startup-impact candidate.

### Local causal discriminator

A bounded, metadata-only Desktop Duplication probe in PC-02's console desktop reproduced five `DXGI_ERROR_ACCESS_LOST (0x887A0026)` events at4999–5000ms intervals. Factory stayed current and D3D device healthy. No Sunshine stream, network video, codec, or rental was needed. Images were neither read back nor saved. The corrected probe matches Sunshine's existing process-local hybrid-GPU handling, supported formats and D3D feature levels. Initial unsupported probe is retained as a mismatch, not a product failure. Both tasks/processes were cleaned.

This localizes the repeating loss to the capture-object layer. It does **not** identify the external Windows/driver/monitoring event that invalidates duplication.

### One targeted live trial

Only Sunshine was temporarily replaced with the archived candidate:
`846ef6cf6e9a8ea15719c5a45585434481f5feff3e1c2766ffefd74715f61ef9`
(`2025.924.154138-cg-capture1`, locked upstream86188d47…). Exact patch source and manifest fingerprints were checked. Current permanent helper fe69d3bf…, web-server, streamer, APK, route/config were preserved. Prelogin Direct public srflx/UDP was used without login/billing. Same claim was cancelled and no rental billing row was created.

Full Sunshine log, saved **before rollback**, proves for78.773s:
- one H.264 encoder initialization;
- zero full encoder teardowns between initial encoder creation and disconnect;
- 16 capture-only recoveries,15 after initial encoder creation;
- recovery3–12ms, median5ms, with encoder retained.

Previous baseline's separate run had13 periodic teardown cycles with median1493ms rebuild. These are different network/time runs, not a randomized total-performance A/B; compare the logged mechanism, not infer an exact freeze-percentage gain.

Actual viewed phone image shows Sentinel prelogin and Direct P2P. Same-source counter window >=5seconds after first render contains1812 decoded frames/60.507s (~29.95FPS) but5.431s accumulated freeze (~8.98%). Startup still36.394s. Adaptive installed client started720×1570@30,2500kbps due weak-WiFi classification; H264 profile/SPS was not captured. Thus **whole stream quality, fixed5Mbps acceptance, input-to-photon latency and freeze<1% remain unproven/failed**.

### Current deployment

Temporary Sunshine was restored to baseline `c9feafa…bb833cb` under guarded rollback. The improved permanent encoder-selector helper is unchanged. The capture candidate is **not installed permanently** and its manifest remains `PARTIAL_NOT_RELEASE_ACCEPTED`. No watchdog/security service was disabled. No router/UPnP/Relay fallback change.

## Safety and packaging

The preserved recovery code checks identical input/thread desktop, adapter/output/monitor, dimensions, rotation, refresh, format, colorspace and bit depth; requires factory/device health, retains API version, and rate-limits attempts. Only ACCESS_LOST is eligible; full reset remains for changed/unknown identity or failed recovery. RAM/VRAM caches are cleared and a real fresh frame is required. A prior real mode-change regression is documented in the original README; it was not repeated in this continuation.

31 native policy checks and26 combined source/manifest/package tests passed with actual evidence enabled. New `verify-capture-candidate.mjs` rejects known/marked capture binaries if their manifest is stripped and invokes the existing full acceptance gate for candidate manifests. Portable copy validates source and destination, Universal production installer validates before sealing, and offline packaging validates even with SkipBundleBuild. This is a classification/acceptance fence, not a signature system or quality approval for unrelated runtimes. Fixture mode remains explicitly non-installable. The candidate was intentionally proven to FAIL production acceptance.

No new lower quality gate or automatic promotion is introduced. Historical manifests and patch source are not edited to manufacture acceptance. Main dirty worktree is unchanged. Corresponding source/license/notices remain with archived GPL binary.

## Next work, without rediscovering this fix

Keep the five-second capture-loss mitigation as the tested candidate baseline for the next controlled media qualification rather than reverting mentally to diagnosing NACK/decoder for this symptom. Address remaining delivery/decoder/startup issues separately. Before permanent rollout: validate the combined chosen runtime and genuine mode-change/fallback/soak behavior under fixed acceptance criteria; verify release source/artifact and update the installed-baseline manifest only after promotion approval/gates. Do not run the obsolete broad baseline restore scripts against the current permanent helper.

Evidence: `06 TEST/20261007-dxgi-reinit-matched`, `06 TEST/20261007-capture-reuse-live/targeted-conclusion.json`, full pre-rollback Sunshine log, deployment receipts, and exact-claim audit. No new full signed installer or GitHub push is claimed.
