# Lazy pipeline startup — source fix, physical rollout pending

## Verified defect and change
Stream startup called `gatherPipeInfo()` before selection. This awaited every video/audio capability probe, including four worker pipelines and optional data decoders irrelevant to native WebRTC tracks. One hung unused probe blocked a supported video_element/audio_element pipeline. Probe workers also lacked timeout/termination and threw from message callbacks without settling the promise.

Selection now queries `getPipeInfo(pipe)` only when it reaches that candidate, preserving candidate order, codec intersection, selected renderer, canvas preference and transport. Shared per-pipe promises are evicted on rejection. Probe-only workers install handlers before sending, settle malformed/error/messageerror/timeout paths, ignore informational log messages and terminate on every settled outcome. No active media worker is terminated by the probe lifecycle.

## Evidence
Real Chromium fault injection: original stayed pending after2509ms and created4unused workers; fixed selected the same H264 video_element/audio_element in3ms with0workers. This timing is a controlled component test, **not** a measured end-to-end latency improvement. Repository browser regression covers9scenarios; source/patch contract suite4tests. Run `node --test tests/lazy-pipeline-startup.test.cjs` and `PLAYWRIGHT_MODULE=<installed module path> node tools/test_lazy_pipeline_browser.cjs`.

Physical prelogin baseline reached first Host video after74s from document load; screenshot showed Sentinel and receiver stats87decoded frames after the observation deadline. A fixed-static pilot was then blocked by origin502 for HTML/JS, before pipeline code ran. Host tunnel logs show heartbeat_timeout during the same delivery incident. No claim that this patch fixes tunnel loss or Android brief-freezes.

## Permanent/reproducible but not silently promoted
Source and tests are version controlled. `tools/apply_lazy_pipeline_static.cjs` applies a narrow compiled patch to the deployed root module and generated capability modules. It refuses unknown contracts and preserves already-deployed explicit Direct/Relay policy and mobile code absent from this older source snapshot. Do NOT deploy the entire compiled `dist` from this snapshot: it would regress unrelated existing route behavior. Current PC02 pilot files were fully rolled back because physical acceptance did not pass.

The package build path compiles these source files normally; a complete release must first reconcile the pre-existing source/deployed route drift. No claim of a released installer, universal production deployment, restart/upgrade acceptance or <1%freeze. Android APK, Host25 recovery binary and quality settings unchanged.
