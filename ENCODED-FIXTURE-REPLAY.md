# Encoded synthetic fixture replay — diagnostic only

Feature `cloudgime-fixture-capture` is **off by default**. It captures at most180 encoded H264 frames/16MiB for <=12seconds, starting at IDR, with bounded writer queue and create-new output. A single IDR request is made only when the explicit capture request arms. This changes the diagnostic run; do not count its smoothness as release acceptance.

Local request lives under ProgramData/Cloudgime/qa-encoded-fixture. Runner restricts ACL to System/Admin/renter, validates the official session/claim and fixture screenshot, and supplies <=60s expiry. Source requires exact fixture/rental paths, session/claim equality, fresh fixture file, foreground=true and stress=true, checked every250ms. Foreground is sampled by the interactive fixture UI thread: the bridge may run in session0, where GetForegroundWindow would observe the wrong desktop. This is trusted-operator QA gating, not a hardened hostile-renter security boundary. A change between checks can take up to250ms to stop capture; do not use for ordinary user content.

Original frame bytes and mapped RTPstamp are saved before packetization, not receiver-recovered bitstream. Writer framing: u32LElength,u32LErtp,u64LEelapsedMicros,u32LEidr,then bytes. Parser must reject truncation, no initial IDR, excessive lengths and clock discontinuity. No decrypted credentials/audio, no automatic capture in production build.

Physical PC02 fixture capture succeeded5October2026 after two failed setup/validation trials. 180frames,2401632payloadbytes,5.891311sRTPspan. Fixture hash and source/build identities recorded under workspace `06 TEST/20261005-decoder-isolation`.

Same HP RMX5367 + c2.unisoc.avc.decoder + google-webrtc1.0.43590:
- x264 no-B synthetic local replay callbackmedian~14.5ms, no next-input dependency at injected300ms gap;
- actual PC02 NVENC fixture callbackmedian~46.4–46.6ms, codec-outputmedian~44ms; boundaryoutput~346ms at injected300ms gap and~13ms after following input;
- all baseline Host replay runs returned179of180frames before shutdown, consistent with one held output; noEOSwas sent by this SDK harness, so do not call the last output permanently lost;
- low-latency/fps/priority hints, locked-fixture SPS/VUI restriction edits, AUDprefix, constraint3, and CSD experiments did not remove dependency;
- CSD final comparison has two incomplete default controls and is not accepted performanceA/B.

These results establish bitstream-dependent decoder behavior without media network delivery, not the unique syntax cause and not complete live freeze causality. Hypotheses are not production fixes. Host/web/tunnel/APK restored and all three rentals settled/cleaned. Do not promote diagnostics as release.

Source tests exercise IDs, exactbinding, foreground/stress, missing flags, expiry and done-state. Wider file/ACL/lifetime/IO tests and review are required before reuse beyond this controlled fixture. Capture implementation/build used live predates the final pure-validator extraction/tests; preserve dirty-source build provenance.
