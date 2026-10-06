# Retained audio-track RTP clock — candidate with wire validation

## Problem and change

`WebRtcAudio` preserves the WebRTC track on upstream reconfiguration, but previously set `next_rtp_timestamp = 0` on every audio setup. This rewound the RTP timeline without replacing the track. It also added `samples_per_frame` directly even though the Opus RTP clock is always 48 kHz and PCM input can have a different sample rate.

`AudioRtpClock` now owns the timeline for the lifetime of the audio track:

- Valid packet duration is converted to 48 kHz ticks using integer arithmetic.
- Normal packets advance by the configured duration, **not arrival jitter**.
- First packet after explicit setup preserves at least the prior packet boundary; if the upstream had a real gap, track-relative monotonic time preserves that elapsed gap.
- New tracks start independently; natural u32 wrap is allowed.
- Invalid sample-rate/duration fails setup and stops sending until a valid setup arrives. Correct supported-rate list includes 8000, not the prior 80000 typo.
- The candidate was built with `cloudgime-session-baseline` only. **Independent audio sync is not enabled.** The shared `moonlight` stream ID is unchanged.

This repairs a timeline invariant; it does not claim to repair physical lip-sync, clock-rate drift, packet loss, or steady-state receiver sync delay. Input has no original capture timestamp in the current callback, so using monotonic inter-arrival time only across explicit setup is an approximation of the upstream gap, not an audio/video common capture clock. Extremely long gaps (at least half the u32 tick cycle) retain the normal boundary instead of introducing an ambiguous jump; long suspend/resume requires separate acceptance.

## Tests

- 8 clock tests: reset, elapsed setup gap, normal jitter invariance, all supported PCM rates, natural rollover, duration change, invalid setup, independent tracks, repeated setup (some assertions share test cases).
- Complete normal streamer suite: 46 tests PASS.
- Session-baseline suite: 46 tests PASS.
- 3 source integration tests and 3 physical-evidence tests PASS.

The wire-evidence test is an explicit fixture/evidence test requiring `CG_WORKSPACE`; it is not a generic unit test runnable without saved pilot evidence.

## Physical pilot: PC-02 → RMX5367

Two billed pilot attempts and one pre-deployment ADB failure, all stopped and cleaned. Direct public P2P, per-session H264 Baseline, same Android capability APK, no Relay or router change.

Final wire capture covers about 60.49 s:
- One audio SSRC: **300623751**.
- **9,040 packets**, sequence14836→23875, no duplicates, no out-of-order observations.
- **9,035 normal intervals at 240 RTP ticks** (=5 ms at48 kHz).
- Four reconfiguration gaps advance by86029,270857,294709,87012ticks. RTP-derived gaps1.7923/5.6429/6.1398/1.8128s agree with Host NIC observation gaps within1ms.
- No RTP rewind or duplicate timestamp. PacketMonitor reported no capture events lost.

This is wire-timeline PASS only. `pass:true` in `wire-clock-evidence.json` must not be interpreted as A/V or release acceptance.

## Incomplete acceptance and QA issues

The planned PCM beep+flash comparison was **not recorded**:
- Pilot1's gate looked for `rxSize` in live_stats, which does not include that field, and did not wait properly for recovery.
- Pilot3 corrected part of the gate, but still looked for `rxSize` in gameplay_metrics in an APK version that does not log it there. Additionally, actual late receive-FPS samples were17–22, below the27–35 diagnostic settle check.
- The scene was still mostly static with adaptiveFPS enabled. Low measured FPS cannot be attributed to the audio-clock patch or called a renderer regression from these logs alone.
- FPS24→30 mode changes also changed the clarity preset to3500kbps; runner explicitly requested5000 again before comparison. The final settle gate still failed; no recording or quality PASS was fabricated.
- These setup exercises intentionally introduce upstream gaps. They are not a sustained-latency/quality benchmark.

Next physical work should correct the geometry readback and distinguish configured cadence from static-scene adaptive FPS before using that gate. Do not keep rerunning rentals with an invalid gate.

## Cleanup and runtime display exception

Baseline streamer/Sunshine/Android APK restored. Both billed rentals ENDED/BILLING_SETTLED, cleanupCOMPLETED, claimreleased. PC02 MAINTENANCE/Ready; PC01 untouched. Media, QA tasks, PacketMonitor and filters all stopped/cleared.

Sunshine `output_name` changed from `\\.\DISPLAY12` to `\\.\DISPLAY14` when the virtual display was re-enumerated. Interactive renter-session readback showed DISPLAY14 is the only active primary720x1570 display; DISPLAY12 no longer exists. All other configuration bytes matched the original after exactly substituting that one line. The agent did **not** restore an obsolete display name.

Accordingly:
- `final-audit.json` retains `clean=false` because the original byte-level config check failed.
- `config-reconciled.json` records the exact one-key difference and active monitor evidence.
- `reconciled-final-audit.json` says `operationallyRestored=true`, **`byteIdenticalConfigRestored=false`**.
- Future tests must preflight the active monitor and current config hash, not blindly expect the historical DISPLAY12/hash41F7 baseline.

Evidence: `06 TEST/20261006-audio-track-clock` and `...-live`, `...-live2`, `...-live3`. No candidate remains installed. No reduction of the earlier ~95 ms receiver synchronization wait or physical A/V offset has been demonstrated by this patch.
