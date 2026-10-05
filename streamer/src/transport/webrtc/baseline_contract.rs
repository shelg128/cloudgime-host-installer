//! Opt-in H264 Baseline contract. Never rewrite compressed picture data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BaselineSps { pub constraints:u8, pub level:u8 }
pub fn inspect_sps(frame:&[u8])->Result<Option<BaselineSps>, &'static str>{
 let mut found=None;let mut i=0;
 while i+3<frame.len(){let start=if frame[i..].starts_with(&[0,0,0,1]){Some(i+4)}else if frame[i..].starts_with(&[0,0,1]){Some(i+3)}else{None};
  if let Some(n)=start{if n>=frame.len(){return Err("truncated_nal");}if frame[n]&0x80!=0{return Err("forbidden_nal");}if frame[n]&31==7 {if n+4>frame.len(){return Err("truncated_sps");}let s=BaselineSps{constraints:frame[n+2],level:frame[n+3]};if frame[n+1]!=66||s.constraints&0x40==0||s.constraints&3!=0{return Err("not_constrained_baseline");}if !matches!(s.level,10|11|12|13|20|21|22|30|31|32|40|41|42){return Err("unsupported_level");}if found.is_some_and(|v|v!=s){return Err("conflicting_sps");}found=Some(s);}i=n+1;}else{i+=1;}}
 Ok(found)
}
/// Read only accepted H264 video payloads in the remote SDP. Limit to 4.2;
/// reject missing/ambiguous profile constraints instead of borrowing another codec.
pub fn accepted_baseline_level(sdp:&str)->Result<u8,&'static str>{
 if sdp.len()>256*1024{return Err("sdp_too_large");}let mut video=false;let mut accepted=Vec::new();let mut h264=Vec::new();let mut formats=Vec::new();
 for line in sdp.lines().map(str::trim){if line.starts_with("m="){if line.starts_with("m=video "){if video||!accepted.is_empty(){return Err("multiple_video_sections");}let p:Vec<_>=line.split_whitespace().collect();if p.len()<4||p[1]=="0"{return Err("video_rejected");}video=true;accepted=p[3..].iter().map(|x|x.to_string()).collect();}else{video=false;}continue;}if !video{continue;}
 if let Some(v)=line.strip_prefix("a=rtpmap:"){if let Some((pt,codec))=v.split_once(' '){if codec.eq_ignore_ascii_case("H264/90000")&&accepted.iter().any(|x|x==pt){h264.push(pt.to_string());}}}
 if let Some(v)=line.strip_prefix("a=fmtp:"){if let Some((pt,fmtp))=v.split_once(' '){formats.push((pt.to_string(),fmtp.to_string()));}}
 if line=="a=inactive"||line=="a=sendonly"{return Err("remote_not_receiving");}}
 let mut level=None;for pt in h264{let mut matches=formats.iter().filter(|x|x.0==pt);let Some((_,fmtp))=matches.next()else{continue;};if matches.next().is_some(){return Err("duplicate_fmtp");}let mut profile=None;let mut mode=None;for field in fmtp.split(';').map(str::trim){let Some((k,v))=field.split_once('=')else{continue;};match k{ "profile-level-id"=>{if profile.replace(v).is_some(){return Err("duplicate_profile");}},"packetization-mode"=>{if mode.replace(v).is_some(){return Err("duplicate_mode");}},_=>{}}}let Some(p)=profile else{continue;};if mode!=Some("1")||p.len()!=6{continue;}let Ok(bits)=u32::from_str_radix(p,16)else{continue;};let profile=(bits>>16)as u8;let constraints=(bits>>8)as u8;let l=bits as u8;
 if profile==66&&constraints&0x40!=0&&constraints&3==0&&matches!(l,31|32|40|41|42){level=Some(level.map_or(l,|old:u8|old.min(l)));}}
 level.ok_or("baseline_not_accepted")
}
#[cfg(test)]mod tests{use super::*;
 fn sdp(fmt:&str)->String{format!("v=0\r\nm=video 9 UDP/TLS/RTP/SAVPF 96\r\na=recvonly\r\na=rtpmap:96 H264/90000\r\na=fmtp:96 {fmt}\r\n")}
 #[test]fn actual_baseline_header(){assert_eq!(inspect_sps(&[0,0,0,1,0x67,66,0xc0,32,0xac]),Ok(Some(BaselineSps{constraints:0xc0,level:32})));}
 #[test]fn rejects_high_main_unconstrained_and_truncated(){for x in [&[0,0,1,0x67,100,0,32][..],&[0,0,1,0x67,77,0,32],&[0,0,1,0x67,66,0,32],&[0,0,1,0x67,66]]{assert!(inspect_sps(x).is_err());}}
 #[test]fn p_frame_no_sps(){assert_eq!(inspect_sps(&[0,0,1,0x41,1,2]),Ok(None));}
 #[test]fn sdp_accepts_only_explicit_baseline_mode1(){assert_eq!(accepted_baseline_level(&sdp("profile-level-id=42e020;packetization-mode=1")),Ok(32));for p in ["profile-level-id=640020;packetization-mode=1","profile-level-id=42e02a;packetization-mode=0","packetization-mode=1"]{assert!(accepted_baseline_level(&sdp(p)).is_err());}}
 #[test]fn sdp_rejects_duplicate_parameters_and_rejected_media(){assert!(accepted_baseline_level(&sdp("profile-level-id=42e020;profile-level-id=42e02a;packetization-mode=1")).is_err());assert!(accepted_baseline_level(&sdp("profile-level-id=42e020;packetization-mode=1").replace("m=video 9","m=video 0")).is_err());}
 #[test]fn audio_fmtp_is_not_video_acceptance(){assert!(accepted_baseline_level("m=audio 9 UDP/TLS/RTP/SAVPF 96\na=rtpmap:96 H264/90000\na=fmtp:96 profile-level-id=42e02a;packetization-mode=1").is_err());}
}
