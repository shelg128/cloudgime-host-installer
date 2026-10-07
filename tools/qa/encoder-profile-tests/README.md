# NVENC profile downgrade caused by order-insensitive log evidence

## What was reproduced and fixed

The existing DisplayPrepareHelper executes a fresh FFmpeg NVENC sanity probe, then consults Sunshine logs. Its previous log veto selected *any* matching error in the recent tail, including an initialization failure followed by successful H.264 NVENC discovery. That changed a passing NVENC probe into a failed one, allowing the existing selector to choose software. The modern Sunshine timestamp format (`yyyy-MM-dd HH:mm:ss.fff`) was also rejected by its legacy-only parser.

Reproduction uses the actual unmodified helper methods, not a reimplemented selector. With a synthetic failure→success log and fresh-probe success as an input assumption, old code reports `veto=true`, `selectedEncoder=software`, `modernTimestampParsed=false`. The preserved original source and fixture are under `06 TEST/20261007-encoder-profile-recovery`.

The fix:
- Reads a bounded 160KB tail instead of the entire log file, discarding a partial first line.
- Evaluates log order and timestamps. Only a later exact H.264 NVENC discovery clears prior initialization failure. UI readiness, trying/creating encoder, HEVC/AV1 success, undated evidence or timestamp rewind cannot clear it.
- Runtime encode timeout/empty-packet failures require a later startup boundary **and** H.264 discovery, not discovery alone.
- Leaves a newer unresolved failure blocking NVENC. Fresh FFmpeg probe failure is never overridden by successful logs.
- Recognizes both existing timestamp formats, retaining local-time semantics of the original parser.
- Versions log-policy decisions in cached profiles. Old decisions force one real re-probe. A current NVENC cache also re-probes on an unresolved new failure. A software cache rejected specifically by this log policy re-probes after the veto disappears; unrelated API/driver probe failures are not repeatedly retried or relabeled.

No stream resolution, FPS, bitrate, NACK, pacing, buffer, codec profile, routing, billing or fixed test baseline is changed. No Force-NVENC policy is enabled. The normal software fallback policy is unchanged for genuine probe failures. The diagnostic fixed-profile preclaim gate must still reject software for this experiment.

## Verification

Run from repository root:

```
dotnet run --project tools/qa/encoder-profile-tests/EncoderProfileTests.csproj -c Release -- RESULT.json
```

31 cases exercise the real helper selector/cache/log reader via reflection plus timestamp/failure-order tests. No test framework package is required. `RESULT.json` must be an explicit writable path. Full helper publish also passed.

A separate self-contained QA harness used the patched helper's **actual** `RunNvencSustainedProbe` on idle PC-02: 120 synthetic frames at1280×720,8Mbps, same sanity probe the helper already uses. This is NOT a new streaming profile or gaming benchmark. Probe completed successfully in1165ms. With synthetic recovered log input the actual selector retained NVENC; with unresolved failure input it rejected NVENC. Production helper, capability profile and Sunshine configuration hashes were unchanged; no service restart, claim or rental was performed. No probe process remained.

## Limits / rollout

The deployed helper is **not replaced** by this change. A release artifact exists but is not production acceptance. The original failed session's complete Sunshine log and probe records were lost during earlier rollback. Therefore this proves a real downgrade mechanism and regression fix, **not that it was definitively the cause of that particular zero-video event**. Persisted runtime evidence from that event showed `encoder=software` while the current restored profile is NVENC; both facts are retained.

Before rollout use the existing idle-maintenance backup/rollback path, verify installed helper provenance, fixed profile (NVENC/DDX/H264 Baseline/720×1570/30/5000/shared-sync/Direct), and preserve helper/probe/Sunshine logs before any restart. Do not edit `selected_encoder` or cached probe results to force a pass, and do not disable genuine errors to obtain video. Hardware-in-loop video startup, sustained live stream quality, and freeze<1% remain unproved.
