# Bounded NACK diagnostic candidate

This diagnostic adds a 4096-slot, 10-second sequence metadata ring. It records attempted original-send payload length (<=300, <=800, >800 bytes), first/repeated NACKs and age; no RTP payload or endpoint/credential is stored. Sampling is passive and does not change MTU, playout, display or selected route. Sent counts and NACK counts in one reporting window are NOT a loss percentage: feedback can refer to packets in a previous window and attempted writes may fail.

Three new unit tests cover bucket attribution, duplicate feedback, slot collisions, expiry and sequence reuse. Release build passed. Direct live probe did not reach native playback after wireless ADB went offline; it is not accepted as performance remediation.

Previous candidate `9e480b5` + capture-recovery Sunshine passed one physical Relay pacing observation: 5463 frames / 181.490806s, freeze 0.838s (0.462%). Direct was invalidated by a selected ICE-pair change; its first stable segment still had severe stutter. Do not rerun Relay unnecessarily or claim the whole product passes.

Evidence is in workspace `06 TEST/20260928-android-full-regression/capture-resize-1790749528752/`. Passing Device Match build remains deployed. This diagnostic is archived separately and not activated globally.
