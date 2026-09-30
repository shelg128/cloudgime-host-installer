# Sender queue lifecycle — candidate, not pacing accepted

## Scope
Branch `chatbox/sender-queue-lifecycle` builds on the accepted display-source commits `7ca4b41` / `7d42dc1`. No display mode, route policy, billing, bitrate target or MTU changes are included.

## Source fixes
- One owned media sender and RTCP reader per track; replacement awaits task cancellation. Dropping the owner aborts pending workers.
- Queue waiting uses `Notify::notify_one`'s retained permit. Frames arriving between empty-pop and wait cannot be stranded until another frame arrives.
- Missing queue exits instead of a tight `continue` loop.
- New keyframes replace older queued keyframes/dependent frames rather than growing an unbounded important-frame backlog. In-flight frame packets are not reordered.
- Track replacement reuses its established RTP sender and sequence counter; sequence numbers do not rewind on the existing SSRC. Independent senders keep independent counters.
- Compact `pace video|audio q_us=... send_us=... frames=... zero=...` telemetry fits Host's line truncation. Zero-byte writes are not counted as successful RTP writes.

## Targeted validation
- Eight new queue/lifecycle unit tests PASS.
- Three new sequence-continuity tests PASS; only these tests were rerun after adding the sequence counter.
- Final release build PASS. SHA256 `af39c7a2dbf48ca89b7b6f994a9080ae60ea6eebcb5dc5f0324fe4bd73991843`.
- Final sequence/compact-telemetry build has NOT completed a valid physical pacing measurement. Do not substitute earlier candidate evidence.

## Live results: still FAIL
First queue candidate plus capture-recovery Sunshine:
`06 TEST/20260928-android-full-regression/capture-resize-1790745379309`.
- Direct: 43.586 seconds of freeze / 177.484445 seconds; FAIL unchanged 3% gate.
- Explicit Relay: 7.236 / 177.474152 seconds; FAIL unchanged 3% gate.
- Frame totals are ~29.5 / 30.1 decoded FPS, but arrivals are bursty. Mean FPS is not smoothness.
- Host reported ~30 FPS. Capture-only recovery was active; periodic full encoder teardown was avoided. Direct NACK/retransmission pressure is much higher than Relay. This is not conclusive attribution to Wi-Fi, Internet path, sender, or decoder.
- Initial telemetry was truncated by Host before queue timing fields; the final build uses a compact line.
- Final build attempt `capture-resize-1790747764717` did not enter native pacing because phone focus moved to another application. NOT a pacing pass and not evidence of a streamer failure. No attempt to take focus back from the user.
- All test binaries were restored to the retained display baseline. The passing display candidate remains installed; no public release/pacing promotion.

## Next targeted work
Use the physical phone without other app use during measurement. Collect compact queue timing, actual sender throughput, decoder/frame-arrival/NACK statistics on a stable Direct pair; compare a baseline and candidate under comparable conditions. The existing earlier runs differ in screen modes/time/network conditions and are not a controlled A/B.
Do not change MTU based solely on NACKs: installed RTP dependency currently uses 1200-byte packetization; actual path/framing needs measurement. No automatic Relay fallback or router changes.
Avoid rerunning Device Match, audio tones, or full billing regression. Login/claim/cleanup remain necessary setup/safety steps. Overall pacing and production native-stop remain open acceptance items.
