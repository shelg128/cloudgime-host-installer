# Physical decoder replay probes (QA only)

These standalone Java entry points run on the physical Android test device using `app_process` under shell UID. They do not install/replace the CloudGime APK. They exercise MediaCodec or the pinned google-webrtc1.0.43590 AndroidVideoDecoder class, not a complete RTC receiver or actual display presentation. No secrets/network media are recorded by the replay.

- DecoderReplay: MediaExtractor MP4 -> MediaCodec -> EGL SurfaceTexture,90inputframes; normal/low-latency request and300msinputgap.
- WebRtcReplay: exact named c2.unisoc.avc.decoder using SDK factory color selection, Annex-B inputs from MP4 or diagnostic `.cgr` records; codec dequeues plus SDKcallbacks recorded separately; noEOSinSDKprobe,450msdrain thenrelease.
- Modes: default,low,fps,priority,csd. CLI fourth argument is comma-separated mode list; PowerShell caller must quote it. Verify actual mode names in output before claiming a comparison.
- Input resolution720x1570 and30FPScadence are fixed for this fixture, not a generic decoder benchmark.
- No input-to-photon claims. ShellUID, synthetic/EGLsurface, missingliveCPUload, nativeRTCmetadata and packetnetwork limitations apply.

Build with installed JDK17 + Android34android.jar; dex with installed build-tools36 (34D8 crashes on a nestedSDKclass), minimum26. Bundle SDKclasses.jar, use SDKnativearm64library; do not silently substitute SDKrevision. Final transfer used `llvm-strip --strip-debug` on a copy of library due large-transfer failures; hash and final controls recorded. Exactcommands and hashes archived in workspace `06 TEST/20261005-decoder-isolation/reproduction` and manifest.

Known limitations: bounded fixed-run replay but no overall in-process watchdog around vendor codec calls; externalrunner has timeout; stop/reconcile stale probe processes before retry. Initial constructor colorformat mismatch produced16emptyruns, invalidnotpass. FinalCSDcomparison includes two defective defaultcontrols; do not promote partial output as successfulperfA/B. Missing output/duplicatePTS must be reported, not imputed.

Fixture SPS experiment scripts are hash-locked, hard-coded **offline diagnosis only**. Never use them on untrusted/liveSPS or infer general H264bitstream compliance from FFmpegacceptance/identicaldecodedpixels. None removed the oneframe wait, so none is a productionfix.

Hostcapture is a separate default-offfeature documented in `ENCODED-FIXTURE-REPLAY.md`. Captured180frames are syntheticfixture; do not capture ordinarydesktop/logincontent. Source revisions/build stages and failedcaptureattempts must remain in the report.
