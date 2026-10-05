# NVENC H.264 Baseline compatibility candidate

Default is unchanged. `nvenc_h264_baseline = enabled` is an explicit, operator-controlled option in the **patched native NVENC path only**. It selects the real NVENC Baseline profile, CAVLC, and disables weighted prediction/adaptive8x8transform/B-direct/FMO. 10bit/4:4:4 H264 requests are rejected by that branch. HEVC/AV1 unchanged. No route, billing, capture, bitrate or FPS change is introduced by this profile patch.

This is **NOT release accepted** and is NOT a session-negotiated universal setting. It currently applies to all nativeNVENCH264 sessions on a configured node, does not cover FFmpeg/AMF/QSV/software paths, and does not integrate profile selection with SDP/Moonlight negotiation. A user/device-selected end-to-end profile contract and legacy-client regression are needed before product rollout. Do not turn it on globally or infer client support simply because the phone used here works. Never rewrite High SPS to Baseline without actually encoding compatible features.

Patch tool pins upstream86188d47a7463b0f73b35de18a628353adeaa20e and rejects unexpected/previouslymodified target files; existing capture-recovery/toolchain changes in other files remain. Full build needed after applying. Preserve upstream GPL/source notices when distributing.

## Evidence5October2026

Same HP RMX5367/c2.unisoc.avc.decoder, pinnedWebRTCSdk:
- profile_idc66→77 alone (onebyte at containeroffset731), all constraints/payload/timestamps unchanged, causes a one-frame codec-output wait; FFmpegdecodedpixelsidentical.
- High-CAVLC,Main-CABAC andHighwith8slices still waited; BaselineCAVLC didnot. Constraint4/5didnotremoveHighwait. This implicates decoder profile handling rather than simply CABAC cost.
- RealNVENC output from PC02 withcandidate was SPSprofile66/PPSentropy0,verified180frameshash2e739d46d4eb35e726da68f2d2b575332cc04fc15e35fc9316dd8e8825af323f.
- LocalHP replay: 720/720codecoutputs,719/720callbacks(initialcallbackmissing),callbackmedian13.10ms. At300msinputpause, previousframeoutput~12–13ms, before nextinput. Highcontrolafterwardstill~45.62ms and~342–346msonpause.
- RealDirectstream720x1570@30,target5000kbps:100.86161scounterwindow,29.942FPS,decode rawmedian10ms,p95nearest-rank18ms,max20ms. **Freeze9.25823%FAIL<1%**, rawJBmedian88ms,p95134ms. Legacyoverlayplay/bufferarecappedandnotacceptancephotonlatency.
- BaselinevsHighlivewindowsdifferinRTT/time; cannotattributefreezereductionsolelytoprofile. Baselinechangescodingefficiency;visualqualitynotprovedequivalentatsamebitrate.

Threephysicalrentalattempts: firstADBPNGtransferfailedbeforemeasurement; secondSSHmenuhelperfailed; thirdqualitywindow+bitstreamcapturecompleted. Allsettled/cleanupCOMPLETED,allbinary/configrollbackverified,APKunchanged,probesarchived. Firstrollbackformat-onlyconfigmismatchreviewedandrestored; thirdstreamerSSHtimeoutrequiredexacthashrollbackretry. Do not call assistedrollbacksfullyautonomousPASS.

Evidenceworkspace:06 TEST/20261005-decoder-constraint45;live:06 TEST/20261005-nvenc-baseline-live3.
Tests:baseline-profile.test.mjs(sourcecontract) andtools/qa/decoder-replay/baseline-evidence.test.mjs(physicalevidence invariants). Tests are not fullencoderconformance/visualqualityacceptance.
