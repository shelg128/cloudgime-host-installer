# Same-network transport head-of-line blocking fix

## Proven mechanism
On PC02 physical Android Direct testing (2026-10-04), an audio-data-channel send held the global `transport_sender` Mutex for2,476,040us; admission of a video decode unit waited2,442,233us at the same Host timestamp. In that interval video egress collapsed to6frames/2.644s. These are same-machine monotonic durations, not inferred one-way network delay.

The media queue and socket-write stages were otherwise short (measured queue max13.611ms, complete frame write max14.723ms, packet write max9.548ms in an earlier sampled high-load window). Android system UDP error deltas were zero in the measured windows; this is system-wide evidence, not proof no network or per-UID scheduling problem exists.

## Implementation
1. A reproduced lost-notify opportunity is fixed with a retained `notify_one` permit for the single queue consumer; the owner is not held across an idle await. FIFO, frame bytes, queue capacity, keyframe/drop rules and pacing are unchanged. This alone did not pass physical quality.
2. `transport_sender` is a read/write lock. Sending video/audio/control obtains shared access and uses existing per-lane internal synchronization. Setup, transport replacement and close retain exclusive access. This prevents an audio/control send holding a global media mutex. No extra workers/packet copies or buffer enlargement.
3. Host-created `general` channel is classified as Host origin and is no longer stored as `client_general_channel`, which previously opted an Android client into unsolicited CGA1 legacy audio copies. Only the existing Remote-origin general path retains legacy-copy compatibility. Android's normal client receives an RTP audio track and does not consume CGA1; no codec, audio RTP, mute or volume change.

## Physical scope
After shared access, observed audio sends still stalled2.676s,0.903s,0.673s,3.999s and0.804s. There were no matching video-wait events over20ms, and video egress continued about60–63frames/2s through those stalls. This supports fixing cross-lane blocking, **not** all streaming freezes. That pilot also failed its workload tick gate237ms and full quality remained unaccepted.

Default-off `cloudgime-egress-observer` records compact per2s local queue/frame/write maxima and zero/error counts for10min; slow lock stage metadata is capped180rows. It does not record payload, credentials or endpoints. Normal build omits observers.

## Tests and release status
- Observer variant35tests; default variant32tests pass, including channel origin, lost-wakeup, transport concurrency/replacement/stop models and pre-existing streamer tests.
- Android QA system UDP parser has reset/missing/precision/scope regression tests.
- Real peer shutdown races, every client type, long-session audio and complete quality acceptance remain separate requirements. The read/write lock tests model coordination; they do not substitute for all real WebRTC teardown interleavings.
- This work does not change network/router/Direct-vs-Relay selection, billing, resolution,FPS,target bitrate, RTP clock, MTU or proactive recovery traffic.
- Do not permanently deploy solely because the mechanism improved. Retain exact baseline backup and restore failed pilot binaries. No claim of<1%freeze or release readiness without measured full-window acceptance.

Evidence: workspace `06 TEST/20261004-same-network-egress/confirmed-audio-video-blocking.json`, `transport-before-after.json`, build/test records, and individual same-network run folders.
