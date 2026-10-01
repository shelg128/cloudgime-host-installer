# Access-unit boundary integrity — reproduced locally, physical validation pending

Base: `38e034e`, branch `chatbox/frame-boundary-integrity`.

## Reproduced packetizer bugs
Three tests failed against the prior `send_single_frame` packetization logic:
- H.264 slice followed by an ignored AUD: no RTP marker was emitted.
- H.264 slice followed by a cached SPS: no RTP marker was emitted.
- A packetizer error in one NAL was logged/skipped while the rest of the picture was enqueued as if complete.

The shared access-unit packetizer now marks the last **emitted** packet after all NALs have been processed. Packetization errors reject the whole access unit before enqueue and request an IDR. Metadata-only units do not clear an existing queued keyframe or enqueue an empty video frame. No RTP sequence, timestamps, crypto, MTU, route or bandwidth limits are changed.

Five targeted tests passed after the change (previously 2 passed/3 failed), including multislice fragmented keyframes, constant timestamp/MTU bounds, and metadata-only access units. Release streamer build passed. Proof is in workspace `06 TEST/20261001-scoped-recovery/boundary-before.log`, `boundary-after.log`, `boundary-build.log`.

## Relation to live issue — not yet established
High-entropy Direct load at requested 5Mbps produced actual median ~3.25Mbps, but decoded frames froze for >52 seconds while packet counters continued advancing. Budget limits worked; quality did not. Freeze-duration alone understated the frozen tail; assessment was corrected to require ongoing decoded progress and target FPS, preserving original results.

The original run had a functional explicit reconnect (5.936 seconds from action to new frame) with the same claim/rental/billing. No need to repeat that acceptance unnecessarily.

A separate Android bounded keyframe-request experiment sent three requests through the existing authorized general channel. Host confirmed IDR requests; no decoder recovery was proven. Its rebuilt APK also selected a different viewport (660x1440 vs baseline 720x1570), so it is not a controlled apples-to-apples comparison. Android experiment default-disabled, original installed APK restored and hash-verified.

This packetizer fix has NOT been demonstrated to explain or fix the high-load stall. Physical candidate test remains pending: Android shell DNS worked but `run-as` CloudGime app UID could not resolve `cloudgime.my.id`. Do not remove DNS/network restrictions or skip the preflight to force the test.

No broad release promotion. Runtime Host/streamer/Sunshine and installed Android APK were restored to accepted Device Match baseline; PC-02 maintenance, cleanup settled, PC-01 untouched. Lower-load Direct pass remains independently recorded on base `38e034e`. New shared packetizer code affects codec packet boundaries, so directly affected codec/route paths need validation if promoted; do not inherit all previous passes blindly.
