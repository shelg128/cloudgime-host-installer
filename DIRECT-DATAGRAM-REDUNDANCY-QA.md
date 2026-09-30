# Direct loss-recovery experiment — one physical pacing PASS, QA only

## Result
`capture-resize-1790786502684`, physical Android, normal pre-existing APK (SHA256 b3833365cc687a605df701b79205e3f9a76de970fd26b9b0725c6259d8bca40c), candidate streamer SHA256 **3d944a1732098f200ddd10e18ab6e8e88e68108b98118bd67d303b9c694420b3**, with capture-recovery Sunshine **846ef6cf6e9a8ea15719c5a45585434481f5feff3e1c2766ffefd74715f61ef9**.

- 5,460 decoded frames / 181.225537 seconds = 30.128 FPS.
- Freeze count delta 0, freeze duration delta 0, loss-estimate delta 0. The counter already contained a startup freeze of 0.230s before the measurement; do not claim zero freezes from launch.
- NACK delta 129; packet/frame recovery is not perfect or zero-loss.
- Frame-arrival window maximum-gap values: p95 95ms, max 98ms. RTT median 80ms, p95 100ms; jitter-buffer median 80.1ms, max 92ms. These are not end-to-end input latency measurements.
- 13,254 eligible original datagrams and 13,254 extra copies in the sampled 180.005s Host window. Extra UDP payload 13,770,303 bytes, about **612 kbps**. Excludes IP/link overhead. Full duplication approximately doubles eligible video UDP traffic; it is not free bandwidth.
- Direct remained srflx/UDP, no automatic Relay. Mandatory billing/cleanup completed. Pilot binary rolled back after experiment; not retained or publicly released.

## Receiver-side evidence motivating the experiment
Opt-in Android decoder timing showed input gaps already present before hardware decode; EGL submission wait was 0–1ms. Decoder input occurs after WebRTC packet reassembly/jitter buffering; it is NOT raw packet-arrival time. Callback residence includes scheduling/driver behavior, not solely GPU compute. The diagnostic's `pending/oldest` entries can include frames the decoder discarded with no callback; these must not be interpreted as actual hardware queue length or frames stalled for minutes.

RTC event log + Host NIC headers in the **same run** `capture-resize-1790783509452`:
- Matching SSRC/sequence/RTP timestamp: 4,565 distinct outgoing video packets; all eventually logged by receiver, 549 sent multiple times.
- 12,282 outgoing audio packets; 10,970 logged by WebRTC, 1,312 not found in the aligned receiver log. This measures between NIC capture and WebRTC logging, not proof of the physical location/cause of loss. No new audio acceptance test was performed.
- Repeated video packets' relative arrival was generally close to the later send, consistent with waiting for retransmission. Clocks are aligned by shared single-send packets; not absolute one-way latency.
- Separate kernel-counter run `capture-resize-1790784978248`: no change to UDP InErrors/RcvbufErrors/checksum errors; five stable CloudGime UID socket inodes had zero added drops. Does not rule out driver/radio/network/SRTP drops.
- No root, VPN capture app, router configuration or physical-network switch used.

## Candidate implementation (intentionally NOT production-ready)
Feature `cloudgime-datagram-redundancy-qa` is off by default. At the UDP `Conn::send_to` boundary, an eligible encrypted video datagram is sent once normally, then copied unchanged with nonblocking `try_send_to`. Return/accounting of the original send remains unchanged; additional byte counters are logged separately. No re-encryption/nonce reuse operation, new RTP sequence, or media mutation.

This **QA heuristic** recognizes SRTP-looking packets with payload types 96–103 and length 22–1280. In this tested SDP those are video. It excludes observed audio PT111, RTCP, STUN, DTLS and TURN ChannelData. It does NOT establish authenticated per-session route identity or general negotiated payload mapping; do not market it as universal Direct/Relay isolation. TURN encapsulation was covered by classifier tests, not a new physical Relay run. The generic vendor hook applies to every matching socket in a feature-enabled process, including other peers with the same PT range. It can also copy retransmissions. These limitations prohibit enabling it by default.

Before production: explicit scoped authorization tied to selected Direct peer/SSRC/PT, loss-triggered enablement, bounded bandwidth/packet budget and accounting, queue-pressure/congestion backoff, safe route/peer teardown, installer identity coverage. Do not change route or rental as recovery. Additional egress could aggravate a congested path. Higher resolution/FPS/bitrate profiles are not covered by the single 720p30 low-motion acceptance window. No controlled same-session A/B causal claim is made.

## Targeted tests
- 5 feature-on tests: classifier bounds/exclusions, overhead accounting, byte-identical double datagram delivery, single audio delivery.
- 1 feature-off test: matching video emitted only once in normal build.
- Android diagnostic probe 10 tests; event-log delta parser 5 tests; kernel parser 4 tests. Existing accepted Relay/audio/display suites were not rerun.

Build candidate: `cargo build --offline --release -p streamer --bin streamer --features cloudgime-datagram-redundancy-qa`
Build normal: omit the feature. QA feature must not be silently added to installers.

## Final state
PC-02 remains maintenance with no rental/claim. Accepted Device Match pilot retained; original Android APK restored after each probe. All experiment media processes and captures stopped. Existing worktree `vendor/webrtc-ice/` leftovers are not dependencies of this branch and must not be added to this change. Source baseline is `3ab4142`; rejected ICE and pacing experiments remain separate.
