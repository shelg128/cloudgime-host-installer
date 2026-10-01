# RTP continuity across encoder restarts

## Proven defect and candidate
The native WebRTC video track and SSRC survive bitrate-triggered encoder restarts, but video RTP timestamps previously came directly from the encoder-local clock. Physical metadata capture showed a backward jump from 4,375,220 to 255,946 on the same SSRC while packet sequence continued (3901 -> 3903): approximately 45.77 seconds backwards.

`VideoRtpClock` retains the outgoing timeline for the lifetime of `WebRtcVideo`, not the encoder setup. Normal source deltas remain unchanged. Source reset/repeated timestamps advance using local monotonic elapsed ticks (minimum 1); intentional 32-bit RTP wrap remains unchanged. No sequence, SSRC, payload, MTU, codec selection, network route, rental, billing or recovery budget changes.

Tests: old direct mapping passed 3/6 clock tests and failed 3 (observed restart, repeated restarts, repeated timestamp). Candidate passes all 6; 57 targeted WebRTC tests pass including packetizer, sender lifecycle, ownership and recovery budgets. Normal release streamer built successfully.

## Physical evidence — do not overstate
Diagnostic run `capture-resize-1790825931135` used passive receiver and sender probes. During apparent stall, Java hardware decoder input AND callbacks continued around 30 fps with OK statuses, while downstream WebRTC counters and rendering stopped. All captured packets belonging to 1,758 interior video frames were received; 954 such frames followed the first stalled counter. The RTP rewind coincided with bitrate Apply/encoder restart. This localizes investigation beyond Java decoder callbacks and proves the clock defect, but does not yet prove the candidate solves the physical stall.

RTC log has an incomplete 4,402-byte final event after app stop; only complete top-level events were analyzed. FrameDecodedEvents were absent in this SDK's event output, not evidence of no decoding. Host stderr truncates long QA rows, so exact IDR/SPS/PPS packet-count correlation was not available. Do not infer missing keyframes from truncated diagnostic lines.

Candidate physical run with original APK `capture-resize-1790827010010` was invalidated by heavy loss, WebSocket 1011/refresh HTTP503, native same-Direct reconnect/counter reset and geometry change 660x1440 -> 660x1392. Quality remains NOT ACCEPTED. Follow-up app-network preflight lost 1/5 packets (20%, RTT 216–347ms); further live attempts stopped. It was not a valid A/B comparison (diagnostic run geometry720x1570 versus candidate660x1440).

## Passive QA feature
`cloudgime-frame-correlation-qa` is opt-in, absent from default features. It counts PLI and summarizes H264 access-unit headers without payload retention, crypto changes or new recovery requests. Logs bounded to 400 events/600 seconds and rate-limited. Normal candidate binary built without the QA feature.

Android probe changes remain separate, debug-only `QA_RECEIVE_TIMING`; keyframe recovery default false. Original APK restored. Diagnostic files do not constitute a production Android release.

## Evidence
Workspace:
- `06 TEST/20261001-frame-correlation`: diagnostic build, clock-before/after logs, passive receiver correlation, original-APK rollback.
- `06 TEST/20261001-rtp-clock`: normal candidate, physical run and final audit.
- `05 LAPORAN/20261001-RTP-CLOCK-REWIND-FOUND-CANDIDATE-NETWORK-BLOCKED.md`.

Next: after stable app/network preflight, one original-APK fixed-geometry test of Apply bitrate and sustained decoded progress, without unnecessary NIC capture or passed reconnect/fullscreen suites. Preserve explicit route and billing. Do not promote broadly until validated.
