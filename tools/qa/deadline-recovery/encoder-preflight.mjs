// Test-profile gate only. A capability profile alone does not prove hardware encoding
// during a session; mismatch blocks the fixed-profile experiment, not normal users.
export function assessEncoderPreflight({expected,observed,nowMs=Date.now()}){
 const reasons=[];
 if(!expected||expected.encoder!=='nvenc'||expected.capture!=='ddx'||expected.width!==720||expected.height!==1570||expected.fps!==30)reasons.push('EXPECTED_PROFILE');
 if(!observed||!Number.isSafeInteger(observed.observedAtMs)||!Number.isSafeInteger(nowMs)||observed.observedAtMs>nowMs||nowMs-observed.observedAtMs>5000)reasons.push('STALE_OBSERVATION');
 if(observed?.selectedEncoder!==expected?.encoder||observed?.configuredEncoder!==expected?.encoder||observed?.selectedCapture!==expected?.capture||observed?.configuredCapture!==expected?.capture)reasons.push('ENCODER_PROFILE_MISMATCH');
 const profileNow=observed?.hostNowMs??nowMs;if(!/^[a-f0-9]{64}$/i.test(observed?.profileSha256||'')||!Number.isSafeInteger(observed?.profileModifiedAtMs)||!Number.isSafeInteger(profileNow)||observed.profileModifiedAtMs>profileNow)reasons.push('PROFILE_IDENTITY');
 if(observed?.slotId!=='2'||observed.slotStatus!=='MAINTENANCE'||observed.currentOwner||observed.activeClaim||observed.mediaCount!==0)reasons.push('NOT_IDLE');
 return {eligible:reasons.length===0,reasons,fixedProfileOnly:true,hardwareEncoderActuallyProducingFrames:false,qualityAccepted:false};
}
export function diagnoseZeroVideo({androidLog,hostLog}){
 const rows=androidLog.split(/\r?\n/);const counters=rows.filter(l=>l.includes('rtc_counters ')).map(line=>Object.fromEntries([...line.matchAll(/\b(packets|framesReceived|framesDecoded|reportUs)=(\d+)/g)].map(m=>[m[1],Number(m[2])])));
 const pairs=rows.filter(l=>/live_stats route=direct local=srflx\/udp\/v4 remote=srflx\/udp\/v4/.test(l));
 const profile=androidLog.match(/device_display_profile.*profile=([^\s]+).*portrait=(\d+)x(\d+)/);
 const noVideo=/No video traffic was ever received from the host!/.test(hostLog)&&/Terminating connection due to lack of video traffic/.test(hostLog);
 const audioForwarded=/first audio WebRTC sample written/.test(hostLog);
 const directConnected=/peer_ice \| state=(?:CONNECTED|COMPLETED)/.test(androidLog)&&pairs.length>0;
 const completeCounterShape=counters.length>=3&&counters.every(x=>['packets','framesReceived','framesDecoded','reportUs'].every(k=>Number.isSafeInteger(x[k])));
 const zeroVideo=completeCounterShape&&counters.every(x=>x.packets===0&&x.framesReceived===0&&x.framesDecoded===0);
 const last=counters.at(-1),first=counters[0];
 return {directConnected,audioForwarded,noVideoReportedByMoonlight:noVideo,zeroObservedVideo:zeroVideo,sampleCount:counters.length,observedSpanSeconds:completeCounterShape?(last.reportUs-first.reportUs)/1e6:null,requestedProfile:profile?{key:profile[1],width:Number(profile[2]),height:Number(profile[3])}:null,failedUpstreamOfWebRtcVideo:noVideo&&audioForwarded&&zeroVideo&&directConnected,rootCauseProven:false,captureOrEncoderOrUpstreamTransportUnresolved:true,freezePercent:null,limits:['Moonlight timeout locates absence of upstream video, not a unique encoder/capture/network cause.','Signaling/media-track attachment and ICE connectivity do not prove video rendered.','No frames means freeze percentage and decoder performance are not measurable.']};
}
