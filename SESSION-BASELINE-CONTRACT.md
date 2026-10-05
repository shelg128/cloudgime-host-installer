# Session-scoped H.264 Baseline — candidate, not release accepted

## Contract

The `cloudgime-session-baseline` streamer feature is off by default. In this candidate, an H264-only WebRTC StartStream takes the Constrained Baseline receive level from the remote SDP. It preserves this internal setting across existing settings clones/restarts and sends it through the vendored Moonlight Rust/C stack as `x-cg-video.h264BaselineLevel` in the RTSP ANNOUNCE.

The RTSP client requires `x-cg-video.h264BaselineSession:1` in DESCRIBE. An older host fails before ANNOUNCE instead of silently encoding High. The Sunshine patch stores the request in `video::config_t`, validates H264/8-bit/4:2:0 plus frame-macroblock, macroblocks/second, dimension and bitrate bounds, then selects real Baseline/CAVLC in the native NVENC encoder. Missing request is level 0 and preserves legacy behavior. Non-native-NVENC encoder paths reject a nonzero request.

The bridge checks every received SPS and requires a Constrained Baseline SPS on initial/IDR frames, with `level_idc <= remote level`. It drops mismatched compressed data before packetization and closes the candidate transport. No SPS rewriting is performed. Candidate codec advertisement has a maximum of level 4.2; the remote accepted level remains the actual bound.

**The node-global `nvenc_h264_baseline` setting is not needed and was absent in the physical acceptance pilot.** The previous opt-in setting still exists for the older candidate; do not enable it globally or confuse it with this session contract.

## Android companion

`NativeBaselineLevelFactory` preserves the SDK decoder/fallback order while advertising a conservative level (maximum 4.2) supported by all enumerated AVC decoders for Constrained Baseline. Unknown capability or native software H264 makes it preserve the original SDK level rather than guess. Decoder format/buffer settings are not changed.

On physical RMX5367, the codec list reported Constrained Baseline through Android AVCLevel52 and 720x1570@30 support; the SDK alone advertised 3.1. Level 3.1 cannot carry 720x1570 (4455 macroblocks), so the first candidate correctly received RTSP 400. With the companion APK, level 4.2 was advertised and the original NVENC output SPS was profile 66, constraint_set1=1, level 42, PPS entropy=0.

## Physical result on PC-02 → RMX5367

- One complete Direct public srflx/UDP pilot, 720x1570@30, requested 5000kbps.
- Actual wall window 105.119s; 26 RTC samples in one source; counter span 100.756335s.
- 3036 decoded frames = 30.1321FPS; decode median9.9ms/p9510.8ms.
- Five freezes, total1.4s = **1.38949%**: still FAIL target<1%.
- Raw jitter buffer median92.65ms/p95112.7ms; selected-pair RTT median85ms/p95175ms.
- Actual video median3332kbps; requested target remained5000kbps.
- Measured-v2 path estimate median154ms is not input-to-photon and not comparable directly with the old capped gameplay estimate.
- SPS capture after the window contains180frames, hash3047baaa17d9ba676226218812f2a688074e48ed37ccba32e38092861f9b5647.
- Global Sunshine config hash unchanged throughout install/restore; no global profile flag.

Four sampled intervals cover the five freezes and NACK increments of6/21/10/15. This is interval correlation, not packet-level causality. No NACK/pacing/MTU algorithm was changed or claimed fixed.

## Safety and deployment limits

- Candidate feature still opt-in, not production enabled. H264-only WebRTC tested; multi-codec offers/renegotiation, Windows native client, alternate encoders, HDR/4:4:4 and visual-quality/efficiency regressions remain unqualified.
- The SDP parser is deliberately limited to one active receive-capable video section, explicit mode1 and supported baseline levels. It is not a complete RFC6184 negotiator. Lower-level renegotiation after the saved contract must be handled before broad rollout.
- SPS validation checks identifying header/profile/level, not complete bitstream conformance or all PPS/slice constraints. Actual encoder feature restrictions and captured bitstream inspection supply the physical evidence.
- `STREAM_CONFIGURATION` is extended. **All vendored C and Rust bindings must be rebuilt together.** External/system prebuilt Moonlight libraries with the old struct are not compatible; do not mix them.
- Request generation and gating is controlled by the candidate build; no new user-facing profile choice or backend handoff field is exposed yet. Other transports retain `None`.
- The source retains ordinary control/billing/route policy. RTSP400 was still retried by the existing upstream retry logic; classifying permanent protocol failures remains separate work.

## Validation and cleanup

44 streamer candidate tests;38 normal tests;5 Node contract tests; native C++ level-policy test compiled with -Wall -Wextra -Werror and passed. Android111 unit tests +117 Node QA tests passed; debug APK build passed. `cargo check -p web-server --bin web-server` passed. Whole-workspace check fails on existing `src/bin/installer_identity_retry.rs` auto-bin without main; do not claim workspace-wide clean build.

Three pilot attempts: first unbilled claim cancelled after RTSP400; second stopped before claim due stale `starting` readiness; official `recover-failure --strategy restart_bundle` restored Ready; third streamed/billed and closed normally. Final audit16:05:19UTC: one rental ENDED/BILLING_SETTLED, cleanupCOMPLETED, claim released; PC02 MAINTENANCE and Ready; PC01 untouched; media/tasks empty. Streamer/Sunshine/web/tunnel/config and original APK hashes restored. Diagnostic thumbnail/capability jars archived.

Build provenance: physical candidate binaries precede later formatting/provenance-tool/test changes. Clean final commits are not byte-identical build claims. Reproduction manifests and full evidence are under `06 TEST/20261005-session-baseline` and the three `session-baseline-live*` directories.
