# NVENC health classification — real failure evidence only

## Proven defect
`TryDetectRecentSunshineNvencFailure` used `ExtractMatchingLines`, whose generic fallback returns the entire input when no pattern matches. Any recent healthy Sunshine log mentioning NVENC therefore became a nonempty “failure”. A successful actual H264 NVENC probe was overridden and software encoding could be selected. This explains a concrete mechanism behind inconsistent encoder/geometry between tests, not every historical encoder failure or every network freeze.

Live PC-02 profile before the fix reported `sunshine_runtime_nvenc_recent_failure` followed by ordinary Sunshine version/publisher startup text, despite the FFmpeg probe succeeding. This was evidence of a false positive, not permission to force NVENC.

## Narrow fix
New pure `NvencRuntimeHealth.FailureLines` returns only lines containing the original six runtime-failure markers. No matches returns empty. `TryDetectRecentSunshineNvencFailure` uses it; other callers of the generic `ExtractMatchingLines` retain their fallback behavior.

Real encode timeout, empty-packet, D3D11 device and H264 encoder-open/create errors remain failures. Unsupported AV1 during a successful H264 probe is not converted into an H264 failure. Age limit, actual FFmpeg one-frame/sustained probes, codec selection, force-NVENC policy and display contract are unchanged.

## Tests
Seven pure tests initially reproduced3failures (healthy NVENC log, AV1-only unsupported log, unrelated startup log),4passed. Afterfix7/7passed. Release helperpublish and existing authenticatedCCDdisplaycontract/packaging checks passed. No extra package required for the console test project.

## PC-02 pilot
Only display-prepare-helper was temporarily replaced with hash guard, admin-only original backup and rollback on actual process/JSON failure. First attempt treated native stderr warning as a terminating PowerShell error: binary rolled back, but configuration had already been changed by preflight. This was not a clean no-effect attempt and is preserved in evidence. Second attempt captured stdout/stderr separately and honored process exit and JSON `ok`.

Actual preflight then returned`ffmpeg_probe:nvenc:default`, sustainedprobe`probe_ok:nvenc_stream_sanity`, selectedNVENC and config`encoder=nvenc`, no force-NVENC flag. Helperpilot hash`1162606f7c47b3003dacb6bc10d993eb7542743c33ed586ea8cd740fb49e2efd`.

New streaming runner requires NVENCand720x1570 before/afterload and refuses silently comparing software660x1440. RTPclock/media binaries are unchanged from accepted source candidate, rejected jitterQA not enabled. Physical streaming attempt stopped before rent because phone left CloudGimeforeground. No claim or billing generated in this continuation. This helperpilot is not acceptance of <=3%brief-freeze quality or broader display operations.

Evidence: workspace`06 TEST/20261001-sparse-recovery` (before/aftertests, helperbuild/contract, deployment+backups, encoderprofile); prepared matchedrunner`06 TEST/20261001-nvenc-matched`.
